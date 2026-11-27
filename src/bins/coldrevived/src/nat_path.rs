//! M2/W3.3 — cr-nat 接入 M1 CLI 复活路径。
//!
//! 关闭 `t1_progress.design_gaps[0]` 的接线层缺口：
//!   1. 跨 NAT UDP 唤醒：`wake` / `find` / `metrics` 在 `nat.enabled` 且已取得对端端点通告时，
//!      改用 `cr_nat::build_plan()` 产出的有序候选 + `holepunch::wake_via_endpoint()`
//!      （打洞与唤醒复用同一 socket）；全部候选失败则如实返回错误。
//!   2. 私网 tracker peer 地址协商：`/announce` 按请求方 IP 决定通告私网还是公网端点；
//!      公网请求方在**没有真实公网 BT 映射**时收到空 peer 列表，绝不收到 RFC1918 地址。
//!
//! **契约不变**：`nat` 段缺省（T0/T1 口径）时所有行为与 M1 逐字节一致；
//! PROTOCOL_VERSION=1 / golden 字段 / CLI v1 既有命令返回值均不变。
//! **门禁不变**：`S6.status` 仍为 `not_run`，本地 mock STUN / mock 中继不构成四类 NAT 实测。

use crate::{http_get, punch_route, NodeState, NODE_SECRET};
use cr_core::{nonce_hex, unix_ts, Config, Json};
use cr_nat::{
    build_plan, choose_strategy, classify_from_filter, classify_from_mappings, cgnat_suspected,
    holepunch, is_public_ipv4, local_advertisement, needs_relay, node_id_16,
    reach::{now_ms, ReachClass, ReachDeclaration},
    stun::StunResult, tracker_endpoint_for, Endpoint, EndpointKind, EndpointPlan, NatType,
    PeerAdvertisement, Strategy, Traversal, TraversalReport,
};
use std::fs;
use std::net::{SocketAddr, UdpSocket};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

pub type Shared = Arc<Mutex<NodeState>>;

/// 守护进程侧 NAT 上下文。所有字段来自真实探测 / 真实配置，不填占位公网地址。
#[derive(Debug, Clone)]
pub struct NatContext {
    pub enabled: bool,
    pub stun_servers: Vec<(String, u16)>,
    pub gateway: Option<String>,
    pub upnp_location: Option<String>,
    pub relay: Option<String>,
    pub report: Option<TraversalReport>,
    pub self_adv: Option<PeerAdvertisement>,
    pub peer_adv: Option<PeerAdvertisement>,
    pub last_wake_path: Option<String>,
    pub last_probe: Option<Json>,
}

impl NatContext {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            enabled: cfg.nat_enabled,
            stun_servers: if cfg.nat_stun_servers.is_empty() {
                Traversal::new().stun_servers
            } else {
                cfg.nat_stun_servers.clone()
            },
            gateway: cfg.nat_gateway.clone(),
            upnp_location: cfg.nat_upnp_location.clone(),
            relay: cfg.nat_relay.clone(),
            report: None,
            self_adv: None,
            peer_adv: None,
            last_wake_path: None,
            last_probe: None,
        }
    }

    pub fn report_or_empty(&self) -> TraversalReport {
        self.report.clone().unwrap_or_else(empty_report)
    }
}

fn empty_report() -> TraversalReport {
    TraversalReport {
        nat_type: NatType::Unknown,
        public_udp_egress_ok: false,
        upnp_available: false,
        upnp_external_ip: None,
        natpmp_available: false,
        natpmp_external_ip: None,
        pcp_available: false,
        cgnat_suspected: false,
        mapped_address: None,
        recommended_strategy: Strategy::None,
        filter_behavior: cr_nat::FilterBehavior::Unknown,
        observations: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// JSON 编解码（cr-core 的最小 Json；cr-nat 本身保持零依赖）
// ---------------------------------------------------------------------------

pub fn endpoint_json(e: &Endpoint) -> Json {
    Json::obj(vec![
        ("kind", Json::s(e.kind.label())),
        ("ip", Json::s(&e.ip)),
        ("port", Json::n(e.port as f64)),
        ("internal_port", Json::n(e.internal_port as f64)),
        ("evidence", Json::s(&e.evidence)),
    ])
}

fn endpoint_from_json(j: &Json) -> Result<Endpoint, String> {
    let kind = j.get("kind").and_then(Json::as_str)
        .and_then(EndpointKind::from_label)
        .ok_or_else(|| "端点 kind 非法".to_string())?;
    let ip = j.get("ip").and_then(Json::as_str).unwrap_or("").to_string();
    let port = j.get("port").and_then(Json::as_i64).unwrap_or(0) as u16;
    let internal_port = j.get("internal_port").and_then(Json::as_i64).unwrap_or(0) as u16;
    let evidence = j.get("evidence").and_then(Json::as_str).unwrap_or("").to_string();
    if ip.is_empty() || port == 0 || evidence.is_empty() {
        return Err("端点缺 ip/port/evidence".into());
    }
    Ok(Endpoint { kind, ip, port, internal_port, evidence })
}

pub fn advertisement_json(a: &PeerAdvertisement) -> Json {
    Json::obj(vec![
        ("node_id", Json::s(&a.node_id)),
        ("private_ip", Json::s(&a.private_ip)),
        ("wake_port", Json::n(a.wake_port as f64)),
        ("bt_port", Json::n(a.bt_port as f64)),
        ("endpoints", Json::arr(a.endpoints.iter().map(endpoint_json).collect())),
        ("bt_external", match &a.bt_external {
            Some(e) => endpoint_json(e), None => Json::Null,
        }),
        ("relay", match &a.relay { Some(r) => Json::s(r), None => Json::Null }),
        ("nat_type", Json::s(&a.nat_type)),
        ("public_udp_egress_ok", Json::b(a.public_udp_egress_ok)),
        ("probed_at", Json::n(a.probed_at as f64)),
        ("s6_status", Json::s("not_run")),
        ("evidence_valid_for_pass", Json::b(false)),
    ])
}

pub fn advertisement_from_json(j: &Json) -> Result<PeerAdvertisement, String> {
    let node_id = j.get("node_id").and_then(Json::as_str).unwrap_or("").to_string();
    if node_id.is_empty() { return Err("通告缺 node_id".into()); }
    let private_ip = j.get("private_ip").and_then(Json::as_str).unwrap_or("").to_string();
    let wake_port = j.get("wake_port").and_then(Json::as_i64).unwrap_or(0) as u16;
    let bt_port = j.get("bt_port").and_then(Json::as_i64).unwrap_or(0) as u16;
    if private_ip.is_empty() || wake_port == 0 || bt_port == 0 {
        return Err("通告缺 private_ip/wake_port/bt_port".into());
    }
    let endpoints: Vec<Endpoint> = j.get("endpoints").and_then(Json::as_arr).unwrap_or(&[])
        .iter().map(endpoint_from_json).collect::<Result<_, _>>()?;
    if endpoints.is_empty() { return Err("通告无任何端点".into()); }
    let bt_external = match j.get("bt_external") {
        Some(v) if !matches!(v, Json::Null) => Some(endpoint_from_json(v)?),
        _ => None,
    };
    Ok(PeerAdvertisement {
        node_id,
        private_ip,
        wake_port,
        bt_port,
        endpoints,
        bt_external,
        relay: j.get("relay").and_then(Json::as_str).map(str::to_string),
        nat_type: j.get("nat_type").and_then(Json::as_str).unwrap_or("unknown").to_string(),
        public_udp_egress_ok: j.get("public_udp_egress_ok")
            .and_then(Json::as_bool).unwrap_or(false),
        probed_at: j.get("probed_at").and_then(Json::as_i64).unwrap_or(0),
    })
}

pub fn plan_json(p: &EndpointPlan) -> Json {
    Json::obj(vec![
        ("strategy", Json::s(p.strategy.label())),
        ("needs_relay", Json::b(p.needs_relay)),
        ("candidates", Json::arr(p.candidates.iter().map(endpoint_json).collect())),
        ("tracker_public", match &p.tracker_public {
            Some(e) => endpoint_json(e), None => Json::Null,
        }),
        ("notes", Json::arr(p.notes.iter().map(|n| Json::s(n)).collect())),
        ("s6_status", Json::s("not_run")),
        ("evidence_valid_for_pass", Json::b(false)),
    ])
}

fn report_json(r: &TraversalReport, socket_mode: &str,
               servers: &[(String, u16)], results: &[StunResult]) -> Json {
    let stun_rows: Vec<Json> = results.iter().map(|x| Json::obj(vec![
        ("server", Json::s(&x.server)),
        ("ok", Json::b(x.ok)),
        ("mapped", match &x.mapped { Some(m) => Json::s(m), None => Json::Null }),
        ("rtt_ms", Json::n(x.rtt_ms)),
        ("error", Json::s(&x.error)),
    ])).collect();
    Json::obj(vec![
        ("nat_type", Json::s(r.nat_type.label())),
        ("public_udp_egress_ok", Json::b(r.public_udp_egress_ok)),
        ("mapped_address", match &r.mapped_address {
            Some(m) => Json::s(m), None => Json::Null,
        }),
        ("upnp_available", Json::b(r.upnp_available)),
        ("natpmp_available", Json::b(r.natpmp_available)),
        ("pcp_available", Json::b(r.pcp_available)),
        ("cgnat_suspected", Json::b(r.cgnat_suspected)),
        ("recommended_strategy", Json::s(r.recommended_strategy.label())),
        ("needs_relay", Json::b(needs_relay(r))),
        ("stun_socket", Json::s(socket_mode)),
        ("stun_servers", Json::arr(servers.iter()
            .map(|(h, p)| Json::s(format!("{}:{}", h, p))).collect())),
        ("stun_results", Json::arr(stun_rows)),
        ("s6_status", Json::s("not_run")),
        ("evidence_valid_for_pass", Json::b(false)),
        ("scope", Json::s("单次真实探测；四类 NAT 矩阵实测仍属 M2/S6")),
    ])
}

// ---------------------------------------------------------------------------
// 端点通告 / 计划
// ---------------------------------------------------------------------------

/// 本节点对外通告。无探测报告时只含私网端点（诚实，不造公网地址）。
pub fn self_advertisement(state: &Shared, cfg: &Config) -> PeerAdvertisement {
    let st = state.lock().unwrap();
    let report = st.nat.report_or_empty();
    drop(st);
    let wake_port = cfg.me().map(|m| m.wake_udp_port).unwrap_or(0);
    let bt_port = cfg.me().map(|m| m.bt_port).unwrap_or(0);
    let mut adv = local_advertisement(&cfg.node_id, cfg.me_host(), wake_port, bt_port,
                                      &report, cfg.nat_relay.clone(), unix_ts());
    if let Some((ip, port)) = &cfg.nat_bt_external {
        adv.bt_external = Some(Endpoint {
            kind: EndpointKind::ConfiguredMapping,
            ip: ip.clone(),
            port: *port,
            internal_port: bt_port,
            evidence: "operator-configured-bt-mapping".into(),
        });
    }
    // v2.4.92：没有真实映射、但开了「打洞优先 / 中继兜底」时，通告**中继会合地址**。
    // 语义是「请按这个地址拨我，本机 SOCKS5 桥会把它接到中继隧道」，不是端口映射：
    // kind=Relay 因此不进 `is_explicit_port_mapping()`，不会被读成公网入向证据。
    // 真实映射优先：两处都配时以运营侧的实际映射为准。
    if adv.bt_external.is_none() {
        if let Some(ip) = &cfg.nat_relay_rendezvous {
            adv.bt_external = Some(Endpoint {
                kind: EndpointKind::Relay,
                ip: ip.clone(),
                port: bt_port,
                internal_port: bt_port,
                evidence: "relay-rendezvous(no-inbound-mapping)".into(),
            });
        }
    }
    adv
}

/// 复活路径当前是否应走 cr-nat 候选计划。
/// 条件：`nat.enabled` + 已取到对端真实端点通告 + 计划里至少有一个候选。
pub fn active_plan(state: &Shared) -> Option<EndpointPlan> {
    let st = state.lock().unwrap();
    if !st.nat.enabled { return None; }
    let peer = st.nat.peer_adv.clone()?;
    let report = st.nat.report_or_empty();
    let relay = st.nat.relay.clone();
    drop(st);
    let plan = build_plan(&report, &peer, relay.as_deref());
    if plan.candidates.is_empty() { None } else { Some(plan) }
}

/// 公网入向的**配置态**（W5.1d）。
///
/// 这里只回答「我们打算让别人从哪儿进来」，不回答「别人真的进得来」——
/// 后者只能由外部主机实测，本机永远无法自证（绑定成功 ≠ 入向可达）。
pub fn public_status(cfg: &Config) -> Json {
    let bt_port = cfg.me().map(|m| m.bt_port).unwrap_or(0);
    let http_port = cfg.node_http(&cfg.node_id).unwrap_or(0);
    let fmt_ep = |e: &Option<(String, u16)>| match e {
        Some((ip, p)) => Json::s(format!("{}:{}", ip, p)),
        None => Json::Null,
    };
    let mut announce = vec![Json::s(format!(
        "http://{}:{}/announce", cfg.host(&cfg.node_id), http_port))];
    if let Some((ip, p)) = &cfg.nat_http_external {
        announce.push(Json::s(format!("http://{}:{}/announce", ip, p)));
    }
    let listen = match cfg.engine_listen {
        Some(a) => Json::s(a.to_string()),
        None if cfg.nat_bt_external.is_some() || cfg.nat_http_external.is_some() =>
            Json::s(format!("0.0.0.0:{}", bt_port)),
        None => Json::s(format!("{}:{}", cfg.bind, bt_port)),
    };
    Json::obj(vec![
        ("bt_external", fmt_ep(&cfg.nat_bt_external)),
        ("http_external", fmt_ep(&cfg.nat_http_external)),
        ("engine_listen", listen),
        ("announce", Json::arr(announce)),
        ("utp_requested", Json::b(cfg.nat_utp)),
        ("utp_note", Json::s(
            "µTP 与 BT 端口同号（UDP）；是否真的绑上要看系统连接表（netstat -ano -p UDP）的 PID")),
        ("inbound_verified", Json::b(false)),
        ("inbound_evidence", Json::s(
            "未由外部主机实测；本机绑定/配置正确不代表入向可达（需端口映射 + 外部验证）")),
        ("s6_status", Json::s("not_run")),
        ("evidence_valid_for_pass", Json::b(false)),
    ])
}

/// `/announce` 的 peer 地址决策（私网/公网分流 + 无公网映射时返回 None → 空 peer 列表）。
pub fn tracker_target(state: &Shared, cfg: &Config, requester_ip: &str) -> Option<(String, u16)> {
    if !state.lock().unwrap().nat.enabled {
        return Some((cfg.host("A").to_string(),
                     cfg.me().map(|m| m.bt_port).unwrap_or(0)));
    }
    let adv = self_advertisement(state, cfg);
    let (report, relay) = {
        let st = state.lock().unwrap();
        (st.nat.report_or_empty(), st.nat.relay.clone())
    };
    let plan = build_plan(&report, &adv, relay.as_deref());
    tracker_endpoint_for(&adv, requester_ip, Some(&plan))
}

// ---------------------------------------------------------------------------
// 探测：STUN 复用唤醒监听 socket
// ---------------------------------------------------------------------------

fn is_stun_response(data: &[u8]) -> bool {
    data.len() >= 20
        && data[0] == 0x01 && data[1] == 0x01
        && data[4..8] == [0x21, 0x12, 0xA4, 0x42]
}

pub fn is_stun_datagram(data: &[u8]) -> bool { is_stun_response(data) }

pub fn probe(state: &Shared, cfg: &Config, mode: &str, timeout_ms: u64) -> Json {
    let timeout = Duration::from_millis(timeout_ms.clamp(50, 10_000));
    let hub = state.lock().unwrap().hub.clone();
    let servers: Vec<(String, u16)> = {
        let st = state.lock().unwrap();
        st.nat.stun_servers.clone()
    };
    let socket_mode = if mode == "dedicated" { "dedicated" } else { "wake-listener" };

    // 1) 映射协议探测（不含 STUN，STUN 由下面复用唤醒 socket 完成）
    let mut t = Traversal::new();
    t.stun_servers.clear();
    t.gateway = cfg.nat_gateway.clone();
    t.upnp_location = cfg.nat_upnp_location.clone();
    t.relay_addr = cfg.nat_relay.clone();
    let mut report = t.run(timeout);

    // 2) STUN：listener 模式复用 UDP 监听 socket（真实映射端口与唤醒一致）
    let session = cr_nat::stun::StunSession::new(servers.clone());
    let results: Vec<StunResult> = if socket_mode == "dedicated" {
        match UdpSocket::bind("0.0.0.0:0") {
            Ok(s) => session.probe(&s, timeout),
            Err(e) => return Json::obj(vec![
                ("error", Json::s(format!("STUN socket 绑定失败: {}", e))),
                ("s6_status", Json::s("not_run")),
            ]),
        }
    } else {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        *hub.stun_sink.lock().unwrap() = Some(tx);
        let out = session.probe_with(&hub.socket, timeout, |d| rx.recv_timeout(d).ok());
        *hub.stun_sink.lock().unwrap() = None;
        out
    };

    // 3) 用真实 STUN 结果回填报告并重算策略 / CGNAT 启发式
    let observed: Vec<(String, String)> = results
        .iter()
        .filter(|r| r.ok)
        .filter_map(|r| r.mapped.clone().map(|m| (r.server.clone(), m)))
        .collect();
    if let Some((_, first)) = observed.first() {
        report.public_udp_egress_ok = true;
        report.mapped_address = Some(first.clone());
        report.nat_type = classify_from_mappings(&observed);
        // 只有「≥2 个目的地址映射一致」（= EIM 已实测）时才值得细分 cone 子型。
        // 映射样本不足（Unknown）或已判 Symmetric 时不得再拿过滤测试覆盖结论：
        // 过滤只能说明「换源回包进不进得来」，分不清 symmetric 与 port-restricted。
        if report.nat_type == NatType::FullCone {
            if let Some((h, p)) = results.iter().find(|r| r.ok).and_then(|r| {
                let (h, p) = r.server.rsplit_once(':')?;
                Some((h.to_string(), p.parse::<u16>().ok()?))
            }) {
                // 过滤测试必须用**只联系过主服务器**的 socket，否则 change-IP 会假阳性
                // （映射探测已经把备用地址写进网关过滤集）。故这里新建一个。
                let f = cr_nat::stun::filter_behavior_fresh(&h, p, timeout);
                report.nat_type = classify_from_filter(f);
            }
        }
    }
    let wan_ip = report.upnp_external_ip.as_deref()
        .or(report.natpmp_external_ip.as_deref())
        .or_else(|| report.mapped_address.as_deref()
            .and_then(|m| m.rsplit_once(':').map(|(ip, _)| ip)))
        .unwrap_or("");
    report.cgnat_suspected = cgnat_suspected(wan_ip, "", "");
    if report.nat_type == NatType::Cgnat || report.cgnat_suspected {
        report.nat_type = NatType::Cgnat;
    }
    report.recommended_strategy = choose_strategy(&report, cfg.nat_relay.is_some());

    let out = report_json(&report, socket_mode, &servers, &results);
    {
        let mut st = state.lock().unwrap();
        st.nat.report = Some(report.clone());
        st.nat.self_adv = None;
        st.nat.last_probe = Some(out.clone());
    }
    // W3.12：探测一旦得出结论，立刻把可达性声明交给订阅线程（它会在 500ms 内
    // 通过现有订阅连接重声明给中继）。这里只传递，不做任何"再判一次"。
    crate::relay_sub::update_declaration(cr_nat::reach::ReachDeclaration::from_report(
        &report,
        cr_nat::reach::now_ms(),
    ));
    // 同步刷新本节点通告（供 /nat/advertise 与 nat advertise 使用）
    let adv = self_advertisement(state, cfg);
    state.lock().unwrap().nat.self_adv = Some(adv);
    out
}

// ---------------------------------------------------------------------------
// 跨 NAT 唤醒
// ---------------------------------------------------------------------------

pub struct WakeOutcome {
    pub result: Result<(Json, u64), String>,
    pub attempts: Vec<Json>,
    pub path: Option<String>,
    pub strategy: String,
    pub notes: Vec<String>,
    /// ADR-013：唤醒前经中继查到的对端可达性决策记录。
    /// `None` = 本轮没有查（nat 未启用 / 无中继 / 验收强制路径），不是「查到 unknown」。
    pub reach_lookup: Option<Json>,
}

/// 只有**仍然有效的 relay-only 声明**才允许跳过直连。
/// `unknown`（含 TTL 过期）必须两条路都试，绝不拿它当低 ID 的证明。
fn reach_skips_direct(decl: Option<&ReachDeclaration>) -> bool {
    decl.map(|d| d.effective_at(now_ms()) == ReachClass::RelayOnly)
        .unwrap_or(false)
}

fn attempt_json(kind: &str, addr: &str, ok: bool, detail: &str, rtt_ms: u64) -> Json {
    Json::obj(vec![
        ("kind", Json::s(kind)),
        ("addr", Json::s(addr)),
        ("ok", Json::b(ok)),
        ("detail", Json::s(detail)),
        ("rtt_ms", Json::n(rtt_ms as f64)),
    ])
}

/// 在**调用方已绑定的同一 socket** 上取一次自身映射地址（W3.6 双向打洞的回打候选）。
///
/// 必须与随后打洞/唤醒复用同一 socket：cone NAT 的映射端口随本地端口走，换 socket
/// 就会报出一个对端打不进来的地址。取不到就返回空 —— 退化为单向打洞，**不伪造候选**。
fn self_mapped(sock: &UdpSocket, servers: &[(String, u16)], per_try: Duration) -> Vec<String> {
    for (host, port) in servers.iter().take(2) {
        let tid = cr_nat::stun::rand_tid();
        let r = cr_nat::stun::probe_one(sock, host, *port, &tid, per_try);
        if let Some(m) = r.mapped { return vec![m]; }
    }
    Vec::new()
}

/// 按候选计划真实发包。`only_kind` 供验收 harness 强制走某一条路径（不改变生产默认顺序）。
pub fn wake_with_plan(
    state: &Shared,
    cfg: &Config,
    plan: &EndpointPlan,
    req: &Json,
    timeout_ms: u64,
    only_kind: Option<EndpointKind>,
) -> WakeOutcome {
    let wake = req.dump().into_bytes();
    let token = nonce_hex();
    let timeout = Duration::from_millis(timeout_ms.clamp(50, 10_000));
    let self_id = node_id_16(&cfg.node_id);
    let peer_label = state.lock().unwrap().nat.peer_adv.as_ref()
        .map(|a| a.node_id.clone()).unwrap_or_default();
    let peer_id = node_id_16(&peer_label);
    let mut out = WakeOutcome {
        result: Err("无可用候选".into()),
        attempts: Vec::new(),
        path: None,
        strategy: plan.strategy.label().to_string(),
        notes: plan.notes.clone(),
        reach_lookup: None,
    };

    // W3.8：中继列表优先用配置里的多端点（可在单点宕机时切换）；未配置则回落到
    // 端点计划里那唯一一个中继候选——两者都为空时后面会如实报「无中继候选」。
    let relays: Vec<String> = if !cfg.nat_relays.is_empty() {
        cfg.nat_relays.clone()
    } else {
        plan.candidates.iter()
            .filter(|e| e.kind == EndpointKind::Relay)
            .map(|e| e.addr())
            .collect()
    };
    // v2.4.86：中继的 UDP 承载端点。与上面是**同一个中继的另一种承载**，
    // 不是第二个中继；只在 TCP 承载全部失败后才试（顺序不抢跑，避免多一次等待）。
    let relays_udp: Vec<String> = cfg.nat_relay_udps.clone();

    // ------------------------------------------------------------------
    // ADR-013：唤醒前先问中继「对端自报哪一类」，再决定要不要打洞。
    // 强制 only_kind 是 S6 量具专用的显式旁路（要分别量 direct / relay 两条路），
    // 生产默认路径没有这个旁路。
    // ------------------------------------------------------------------
    let mut skip_direct = false;
    if only_kind.is_some() {
        out.notes.push("强制候选路径：跳过可达性决策（仅验收量具用）".into());
    } else if !relays.is_empty() && !peer_label.is_empty() {
        let started = std::time::Instant::now();
        let lookup_timeout = timeout.min(Duration::from_millis(700));
        match cr_nat::relay::lookup_reach_any(&relays, &peer_id, lookup_timeout) {
            Ok((used, decl)) => {
                let class = decl.as_ref().map(|d| d.effective_at(now_ms()));
                skip_direct = reach_skips_direct(decl.as_ref());
                out.reach_lookup = Some(Json::obj(vec![
                    ("relay", Json::s(&used)),
                    ("declared", Json::b(decl.is_some())),
                    ("class", Json::s(class.map(|c| c.label()).unwrap_or("unknown"))),
                    ("declared_at_ms", Json::n(
                        decl.as_ref().map(|d| d.declared_at_ms).unwrap_or(0) as f64)),
                    ("ttl_ms", Json::n(
                        decl.as_ref().map(|d| d.ttl_ms).unwrap_or(0) as f64)),
                    ("skip_direct", Json::b(skip_direct)),
                    ("error", Json::Null),
                    ("rtt_ms", Json::n(started.elapsed().as_millis() as f64)),
                ]));
                out.notes.push(format!(
                    "可达性查询：{} 报告 peer={}（声明时刻 {}），{}",
                    used,
                    class.map(|c| c.label()).unwrap_or("unknown"),
                    decl.as_ref().map(|d| d.declared_at_ms).unwrap_or(0),
                    if skip_direct { "跳过全部直连候选与打洞准备" } else { "直连候选照常尝试" },
                ));
            }
            Err(e) => {
                out.reach_lookup = Some(Json::obj(vec![
                    ("relay", Json::Null),
                    ("declared", Json::b(false)),
                    ("class", Json::s("unknown")),
                    ("declared_at_ms", Json::n(0.0)),
                    ("ttl_ms", Json::n(0.0)),
                    ("skip_direct", Json::b(false)),
                    ("error", Json::s(&e)),
                    ("rtt_ms", Json::n(started.elapsed().as_millis() as f64)),
                ]));
                out.notes.push(format!("可达性查询失败（{}）：按 unknown 处理，不跳过直连", e));
            }
        }
    }

    let candidates: Vec<&Endpoint> = plan.candidates.iter()
        .filter(|e| only_kind.map(|k| e.kind == k).unwrap_or(true))
        .filter(|e| !skip_direct || e.kind == EndpointKind::Relay)
        .collect();

    // ------------------------------------------------------------------
    // W3.6 双向打洞准备：整个唤醒只绑定**一个** socket
    // ------------------------------------------------------------------
    // 必须单 socket：`back` 里报给对端的是"本侧映射地址"，而映射端口随本地端口走。
    // W3.3 的实现每个候选都新绑 `:0`，映射端口不可预知，因此报出去的 back 必然是
    // 错地址 —— 对端照着打只会打到无人监听的口上。
    let direct: Vec<&Endpoint> = candidates.iter().copied()
        .filter(|e| e.kind != EndpointKind::Relay)
        .collect();
    let mut sock: Option<UdpSocket> = None;
    let mut back: Vec<String> = Vec::new();
    // 纯私网候选之间没有 NAT，不需要打洞准备（省一次 STUN 与一次中继往返）。
    let need_punch_prep = direct.iter().any(|e| e.kind != EndpointKind::PrivateLan);
    if !direct.is_empty() {
        match UdpSocket::bind(format!("{}:0", cfg.bind)) {
            Ok(s) => {
                if need_punch_prep {
                    let servers = if cfg.nat_stun_servers.is_empty() {
                        Traversal::new().stun_servers
                    } else {
                        cfg.nat_stun_servers.clone()
                    };
                    back = self_mapped(&s, &servers, Duration::from_millis(250));
                    if back.is_empty() {
                        out.notes.push("双向打洞：本侧 STUN 未取得映射，退化为单向打洞".into());
                    } else {
                        out.notes.push(format!("双向打洞：本侧映射 {}", back[0]));
                        // 热身：先向直连候选各发一包，给本侧 NAT 建好过滤项，
                        // 对端随后按 back 回打的包才不会被本侧网关当 unsolicited 丢弃。
                        for ep in &direct {
                            holepunch::warmup_punch(&s, ep, &cfg.node_id, &token, 2);
                        }
                        // 经中继通知对端回打。中继路径在 W3.5② 实验室 5/5 环境 30/30，
                        // 用它做信令是唯一在所有 NAT 类型下都成立的选择。
                        let frame = holepunch::punch_request_with_back(
                            &cfg.node_id, &token, &back);
                        let sig = timeout.min(Duration::from_millis(800));
                        match holepunch::relay_wake_any(&relays, &self_id, &peer_id,
                                                        &frame, sig) {
                            Ok((used, _)) => out.notes.push(format!(
                                "双向打洞：已通过中继 {} 让对端回打", used)),
                            Err(e) => out.notes.push(
                                format!("双向打洞：中继信号失败（{}），退化为单向", e)),
                        }
                    }
                }
                sock = Some(s);
            }
            Err(e) => out.notes.push(format!("UDP bind 失败，跳过直连候选: {}", e)),
        }
    }

    for ep in candidates.iter().copied() {
        if ep.kind == EndpointKind::Relay {
            // NFR-04 公网 P95 必须量到真实中继往返。旧实现把 rtt 写死 0，
            // metrics 采样因此恒为 0 ms——那是量具假值，不是「公网零延迟」。
            let relay_started = Instant::now();
            let relay = holepunch::relay_wake_any(&relays, &self_id, &peer_id, &wake, timeout);
            let relay_rtt_ms = relay_started.elapsed().as_millis() as u64;
            match relay {
                Ok((used, payload)) => match Json::parse(&String::from_utf8_lossy(&payload)) {
                    Ok(reply) => {
                        out.attempts.push(attempt_json("relay", &used, true,
                                                       "真实 DELIVERED 帧", relay_rtt_ms));
                        out = finish(out, state, req, reply, relay_rtt_ms, ep.kind.label());
                        return out;
                    }
                    Err(e) => out.attempts.push(attempt_json(
                        "relay", &used, false,
                        &format!("中继应答非法 JSON: {}", e), relay_rtt_ms)),
                },
                Err(e) => out.attempts.push(attempt_json(
                    "relay", &ep.addr(), false, &e, relay_rtt_ms)),
            }
            if out.result.is_ok() { return out; }
            continue;
        }
        let Some(s) = sock.as_ref() else {
            out.attempts.push(attempt_json(ep.kind.label(), &ep.addr(), false,
                                           "UDP bind 失败，未尝试", 0));
            continue;
        };
        let a = holepunch::wake_via_socket(s, ep, &cfg.node_id, &token, &back,
                                           &wake, 2, timeout);
        let detail = match (&a.error, a.reply.as_ref()) {
            (Some(e), _) => format!("{}（punch_sent={} acked={} peer_punched={}）",
                                    e, a.punch_sent, a.punch_acked, a.peer_punched),
            (None, Some(_)) => format!("punch_sent={} punch_acked={} peer_punched={}",
                                       a.punch_sent, a.punch_acked, a.peer_punched),
            (None, None) => "无应答".into(),
        };
        out.attempts.push(attempt_json(ep.kind.label(), &ep.addr(), a.ok(), &detail, a.rtt_ms));
        if a.ok() {
            let text = String::from_utf8_lossy(a.reply.as_ref().unwrap());
            match Json::parse(&text) {
                Ok(reply) => {
                    let o = finish(out, state, req, reply, a.rtt_ms, ep.kind.label());
                    return o;
                }
                Err(e) => {
                    out.attempts.push(attempt_json(ep.kind.label(), &ep.addr(), false,
                                                   &format!("应答非法 JSON: {}", e), a.rtt_ms));
                }
            }
        }
    }
    if out.result.is_err() && !relays_udp.is_empty()
        && only_kind.map(|k| k == EndpointKind::Relay).unwrap_or(true)
    {
        // v2.4.86：TCP 承载全失败（或本就没配 TCP 中继）时，走中继的 UDP 承载再试一次。
        // 判据不变：只有真的收到对端应用应答才算成功，与 TCP 路径同一把尺子。
        let relay_started = Instant::now();
        let relay_udp =
            holepunch::relay_wake_udp_any(&relays_udp, &self_id, &peer_id, &wake, timeout);
        let relay_udp_rtt_ms = relay_started.elapsed().as_millis() as u64;
        match relay_udp {
            Ok((used, payload)) => match Json::parse(&String::from_utf8_lossy(&payload)) {
                Ok(reply) => {
                    out.attempts.push(attempt_json("relay-udp", &used, true,
                                                   "真实 DELIVER 帧（UDP 承载）", relay_udp_rtt_ms));
                    return finish(out, state, req, reply, relay_udp_rtt_ms, "relay-udp");
                }
                Err(e) => out.attempts.push(attempt_json(
                    "relay-udp", &used, false,
                    &format!("中继应答非法 JSON: {}", e), relay_udp_rtt_ms)),
            },
            Err(e) => out.attempts.push(attempt_json(
                "relay-udp", &relays_udp.join(","), false, &e, relay_udp_rtt_ms)),
        }
    }
    if out.result.is_err() {
        let failed = out.attempts.len();
        out.result = Err(format!("全部 {} 个候选均失败（真实超时/拒绝），不回填默认应答", failed));
    }
    out
}

fn finish(mut out: WakeOutcome, state: &Shared, req: &Json, reply: Json,
          rtt_ms: u64, path_label: &str) -> WakeOutcome {
    out.result = Ok((reply.clone(), rtt_ms));
    out.path = Some(path_label.to_string());
    let mut st = state.lock().unwrap();
    st.nat.last_wake_path = Some(path_label.to_string());
    if reply.get("status").and_then(Json::as_str) == Some("OK") {
        st.last_ack = Some(reply);
        st.last_file_hash = req.get("file_hash").and_then(Json::as_str).map(str::to_string);
    }
    drop(st);

    // ADR-023 / ADR-024：opt-in 数据面。唤醒成功后把经中继的隧道登记进本地 SOCKS5 桥，
    // 键是引擎真正会拨打的对端 BT 地址。承载由 `nat.data_plane_carrier` 决定：
    //   tcp（默认）= ADR-023 既有行为；udp = ADR-024 阶段 2 的 opt-in 可靠 UDP 承载。
    // 登记失败**如实记录并回落直连**，不能把「唤醒成功」当成「数据面已经走中继」。
    let (dp, cfg, adv) = {
        let st = state.lock().unwrap();
        (st.data_plane.clone(), st.cfg.clone(), st.nat.peer_adv.clone())
    };
    if let (Some(dp), Some(adv)) = (dp, adv) {
        let self_id = node_id_16(&cfg.node_id);
        let peer_id = node_id_16(&adv.node_id);
        let use_udp = cfg.nat_data_plane_carrier.eq_ignore_ascii_case("udp");
        let relay = if use_udp {
            cfg.nat_relay_udps.first().cloned()
        } else {
            cfg.nat_relays.first().cloned().or_else(|| cfg.nat_relay.clone())
        };
        let target = adv.bt_external.as_ref().and_then(|ep| ep.addr().parse::<SocketAddr>().ok());
        match (relay, target) {
            (Some(relay), Some(target)) => {
                let channel = u32::from_be_bytes([
                    peer_id[0], peer_id[1], self_id[0], self_id[1],
                ]);
                let res = if use_udp {
                    dp.attach_relay_udp_tunnel(&relay, &self_id, &peer_id, channel, target)
                } else {
                    dp.attach_relay_tunnel(&relay, &self_id, &peer_id, channel, target)
                };
                match res {
                    // 措辞必须是「已确认」：这一步现在要求对端真的回了 OPENED，
                    // 不是「帧发出去了」。
                    Ok(()) => out.notes.push(format!(
                        "中继数据面已确认（{}，OPEN/OPENED 双向握手完成）: channel={channel} peer={target}",
                        if use_udp { "UDP" } else { "TCP" }
                    )),
                    Err(e) => out.notes.push(format!("中继数据面未登记（回落直连）: {e}")),
                }
            }
            _ => out.notes.push(format!(
                "中继数据面未登记：缺{}中继或对端 BT 端点（回落直连）",
                if use_udp { " UDP" } else { " TCP" }
            )),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// `nat` CLI 子命令分发
// ---------------------------------------------------------------------------

/// 与 `relay_sub`/`main` 同一口径：显式 role 优先，缺省退回 node_id 字面量。
/// 抽出来是为了把「member 绝不能把打洞 socket 拼到引擎监听口」钉成单测。
fn is_holder_role(node_id: &str, role: Option<&str>) -> bool {
    role.map(|r| r == "holder").unwrap_or(node_id == "A")
}

pub fn dispatch(state: &Shared, cfg: &Config, args: &[String]) -> Json {
    let sub = args.first().map(String::as_str).unwrap_or("status");
    let p = &args[1..];
    match sub {
        "probe" => {
            let mode = p.first().map(String::as_str).unwrap_or("listener");
            let timeout = p.get(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(1500);
            probe(state, cfg, mode, timeout)
        }
        "advertise" => advertisement_json(&self_advertisement(state, cfg)),
        "public" => public_status(cfg),
        "peer" => {
            let source = p.first().cloned().unwrap_or_default();
            if source.is_empty() {
                return Json::obj(vec![("error", Json::s("用法: nat peer <url|文件>"))]);
            }
            let raw = if source.starts_with("http://") {
                match http_get(&source) { Ok(v) => v, Err(e) =>
                    return Json::obj(vec![("error", Json::s(format!("取回通告失败: {}", e)))]) }
            } else {
                match fs::read(&source) { Ok(v) => v, Err(e) =>
                    return Json::obj(vec![("error", Json::s(format!("读取通告失败: {}", e)))]) }
            };
            match Json::parse(&String::from_utf8_lossy(&raw))
                .map_err(|e| e.to_string())
                .and_then(|j| advertisement_from_json(&j))
            {
                Ok(adv) => {
                    let out = Json::obj(vec![
                        ("ok", Json::b(true)),
                        ("node_id", Json::s(&adv.node_id)),
                        ("endpoints", Json::n(adv.endpoints.len() as f64)),
                        ("bt_external", Json::b(adv.bt_external.is_some())),
                    ]);
                    state.lock().unwrap().nat.peer_adv = Some(adv);
                    out
                }
                Err(e) => Json::obj(vec![("error", Json::s(format!("通告非法: {}", e)))]),
            }
        }
        "plan" => {
            match active_plan(state) {
                Some(plan) => plan_json(&plan),
                None => Json::obj(vec![
                    ("error", Json::s("无活动 NAT 计划（nat.enabled=false 或未取得对端通告）")),
                    ("s6_status", Json::s("not_run")),
                ]),
            }
        }
        "wake" => {
            let file_hash = p.first().cloned().unwrap_or_default();
            let infohash = p.get(1).cloned().unwrap_or_default();
            let only = p.get(2).and_then(|s| EndpointKind::from_label(s));
            if p.get(2).is_some() && only.is_none() {
                return Json::obj(vec![("error", Json::s("未知候选 kind"))]);
            }
            let Some(plan) = active_plan(state) else {
                return Json::obj(vec![("error", Json::s("无活动 NAT 计划，先 nat probe / nat peer"))]);
            };
            let req = cr_wake::build_req(NODE_SECRET, &file_hash, &infohash);
            let o = wake_with_plan(state, cfg, &plan, &req, 1200, only);
            let (ack, rtt) = match &o.result {
                Ok(v) => (v.0.clone(), v.1),
                Err(_) => (Json::Null, 0),
            };
            Json::obj(vec![
                ("ok", Json::b(o.result.is_ok())),
                ("path", match &o.path { Some(v) => Json::s(v), None => Json::Null }),
                ("strategy", Json::s(&o.strategy)),
                ("rtt_ms", Json::n(rtt as f64)),
                ("attempts", Json::arr(o.attempts)),
                ("notes", Json::arr(o.notes.iter().map(|n| Json::s(n)).collect())),
                ("reach_lookup", match o.reach_lookup {
                    Some(v) => v, None => Json::Null,
                }),
                ("ack_status", Json::s(ack.get("status").and_then(Json::as_str).unwrap_or(""))),
                ("s6_status", Json::s("not_run")),
                ("evidence_valid_for_pass", Json::b(false)),
                ("error", match o.result { Err(e) => Json::s(e), Ok(_) => Json::Null }),
            ])
        }
        "punch-route-answer" => {
            // ADR-022 对端侧：在同一条中继连接上回两轮裁决，让 punch-route 能闭环。
            // local_ok=false（本侧未握手/未打洞）时如实拒绝直连，双方回落 Relay。
            if !cfg.nat_punch_route {
                return Json::obj(vec![("error", Json::s("nat.punch_route 未启用"))]);
            }
            let local_ok = p.first().map(String::as_str) == Some("ok");
            let relay_addr = cfg.nat_relays.first().cloned().or_else(|| cfg.nat_relay.clone());
            let Some(relay_addr) = relay_addr else {
                return Json::obj(vec![("error", Json::s("没有可用中继"))]);
            };
            let self_id = node_id_16(&cfg.node_id);
            let timeout = Duration::from_millis(1500);
            // 应答侧拿到的两轮是**发起方**的裁决；本端路由用它自己的 local_ok 复算，
            // 这样健康检查能直接比对两端结论是否一致（不一致就是双向黑洞）。
            let result = cr_nat::relay::RelayClient::new(relay_addr).connect(timeout)
                .and_then(|mut conn| {
                    // 新连接必须 register；未注册节点的中继帧会被服务端丢弃。
                    conn.register(&self_id)?;
                    cr_nat::punch_wire::answer_verdicts(&mut conn, &self_id, local_ok, timeout)
                })
                .and_then(|rounds| {
                    cr_nat::punch_wire::negotiate_route(
                        &self_id,
                        &rounds.0.nonce,
                        local_ok,
                        rounds,
                    )
                });
            match result {
                Ok(route) => Json::obj(vec![
                    ("ok", Json::b(true)),
                    ("local_ok", Json::b(local_ok)),
                    ("route", Json::s(route.label())),
                    ("relay_data_plane", Json::s("tcp-carrier-implemented")),
                    ("relay_data_plane_udp", Json::s("udp-carrier-implemented-rst-path-verified")),
                    ("s6_status", Json::s("not_run")),
                ]),
                Err(e) => Json::obj(vec![
                    ("ok", Json::b(false)),
                    ("local_ok", Json::b(local_ok)),
                    ("error", Json::s(e)),
                    ("s6_status", Json::s("not_run")),
                ]),
            }
        }
        "punch-route" => {
            // ADR-022：打洞优先/中继兜底的可调用控制面。opt-in，默认拒绝。
            // 只做**判据与交接决策**：返回 Route 与两轮裁决，不声称 BT 数据已走桥或中继。
            if !cfg.nat_punch_route {
                return Json::obj(vec![
                    ("error", Json::s("nat.punch_route 未启用（默认关闭）")),
                    ("s6_status", Json::s("not_run")),
                ]);
            }
            let peer_label = p.first().cloned().unwrap_or_default();
            let bind_hint = p.get(1).cloned();
            // DOC-30：发现端口作信令字段。第三个参数可覆盖对端 endpoint：
            // 只允许替换**端口**，IP 必须仍等于已登记的对端公网 IP，避免把
            // 任意攻击者地址塞进打洞目标。未提供时保持原行为（静态 bt_external）。
            let discovered_port = match parse_discovered_port(p.get(2)) {
                Ok(v) => v,
                Err(e) => return Json::obj(vec![("error", Json::s(e))]),
            };
            if peer_label.is_empty() {
                return Json::obj(vec![("error", Json::s(
                    "用法: nat punch-route <对端node_id> [本端connect地址ip:port] [发现端口] [共享nonce-hex] [打洞窗口ms]"))]);
            }
            let relay_addr = cfg.nat_relays.first().cloned()
                .or_else(|| cfg.nat_relay.clone());
            let Some(relay_addr) = relay_addr else {
                return Json::obj(vec![("error", Json::s("没有可用中继（nat.relay/relay 为空）"))]);
            };
            let self_id = node_id_16(&cfg.node_id);
            let peer_id = node_id_16(&peer_label);
            // 可选第四参数：双端真实同时打开必须由同一会话 nonce 收敛。
            // 不给时保持旧行为（本进程随机 nonce），单端 punch-route + answer 仍可用。
            let nonce = match parse_shared_nonce(p.get(3)) {
                Ok(Some(n)) => n,
                Ok(None) => punch_route::session_nonce(),
                Err(e) => return Json::obj(vec![("error", Json::s(e))]),
            };
            let timeout = Duration::from_millis(1200);
            // 可选第五参数：打洞窗口（毫秒）。公网「同时打开」能不能成，很大程度
            // 取决于两端处于 SYN_SENT 的时间有没有重叠；1.2 s 只够十几轮，实测
            // 经常一轮都撞不上（每轮都是双向 RST）。默认放到 4 s，仍可显式收紧。
            let punch_ms = p.get(4).and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(4000).clamp(300, 15_000);
            let punch_timeout = Duration::from_millis(punch_ms);

            // 1) 真实 TCP 打洞：没有外部 socket 就没有可判据的对象。
            let local: SocketAddr = bind_hint
                .as_deref()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| format!("{}:0", cfg.bind).parse().unwrap());
            let base_target: SocketAddr = match state.lock().unwrap().nat.peer_adv.as_ref()
                .and_then(|a| a.bt_external.as_ref())
            {
                Some(ep) => match ep.addr().parse() {
                    Ok(a) => a,
                    Err(_) => return Json::obj(vec![("error", Json::s("对端 BT 地址非法"))]),
                },
                None => return Json::obj(vec![("error", Json::s("对端无公网 BT 映射，先 nat peer"))]),
            };
            let targets = PunchTargets::new(base_target, discovered_port);
            let punched = cr_nat::tcp_punch::punch_tcp(local, &[targets.syn], punch_timeout);
            let Some(mut stream) = punched.established.map(|p| p.stream) else {
                state.lock().unwrap().punch_route.record(None);
                // 只写「未建立」是查不动的：同时给出发起轮数与本轮真实系统错误。
                // 抓包与报告对不上时（例如 pcap 里明明有 SYN+ACK），这两项才能定位
                // 是 bind 失败、被拒、还是内核从没报完成。
                return Json::obj(vec![
                    ("ok", Json::b(false)),
                    ("route", Json::s("relay-fallback")),
                    ("reason", Json::s("TCP 打洞未建立（poll+SO_ERROR 判据）")),
                    ("punch_local", Json::s(format!("{local}"))),
                    ("punch_target", Json::s(format!("{}", targets.syn))),
                    ("punch_attempts", Json::n(punched.attempts as f64)),
                    ("punch_errors", Json::arr(
                        punched.errors.iter().cloned().map(Json::s).collect())),
                    ("s6_status", Json::s("not_run")),
                ]);
            };
            // 2) nonce 握手：连到的必须是这次会话的对端，不是任意监听者。
            let peer = match punch_route::verify_peer(&mut stream, &self_id, &nonce, timeout) {
                Ok(p) if p == peer_id => p,
                Ok(p) => {
                    state.lock().unwrap().punch_route.record(None);
                    return Json::obj(vec![
                        ("ok", Json::b(false)),
                        ("reason", Json::s(format!("握手对端 id 不符: {:?}", p))),
                        ("s6_status", Json::s("not_run")),
                    ]);
                }
                Err(e) => {
                    state.lock().unwrap().punch_route.record(None);
                    return Json::obj(vec![
                        ("ok", Json::b(false)), ("reason", Json::s(e)),
                        ("s6_status", Json::s("not_run")),
                    ]);
                }
            };
            // 3) 经中继交换两轮裁决：双方都不接受就回落。
            let relay = cr_nat::relay::RelayClient::new(relay_addr.clone())
                .connect(timeout)
                .and_then(|mut conn| {
                    // 新连接必须 register；未注册节点的中继帧会被服务端丢弃。
                    conn.register(&self_id)?;
                    Ok(conn)
                });
            let route = match relay.and_then(|mut conn| {
                punch_route::exchange_verdicts(
                    &mut conn, &self_id, &peer, true, &nonce, timeout)
            }) {
                Ok(rounds) => match punch_route::negotiate_route(&self_id, &nonce, true, rounds) {
                    Ok(r) => r,
                    Err(e) => {
                        state.lock().unwrap().punch_route.record(None);
                        return Json::obj(vec![
                            ("ok", Json::b(false)), ("reason", Json::s(e)),
                            ("route", Json::s("relay-fallback")),
                            ("s6_status", Json::s("not_run")),
                        ]);
                    }
                },
                Err(e) => {
                    state.lock().unwrap().punch_route.record(None);
                    return Json::obj(vec![
                        ("ok", Json::b(false)), ("reason", Json::s(e)),
                        ("route", Json::s("relay-fallback")),
                        ("s6_status", Json::s("not_run")),
                    ]);
                }
            };
            // 4) 真正 handoff：Route::Direct 时把已验证 socket 接进数据面，
            //    否则它会在本函数返回时 drop——旧实现只有判据没有数据面。
            //    两端角色不同：member 把 socket 登记进 SOCKS5 桥（引擎会主动拨
            //    targets.bridge）；holder 必须把 socket 拼到自己引擎的监听口，
            //    否则 inbound BT 握手无人接收，是「握手成功、取片 0 B」的来源。
            let dp = state.lock().unwrap().data_plane.clone();
            let is_holder = is_holder_role(&cfg.node_id, cfg.role.as_deref());
            let punched_registered = if route == cr_nat::Route::Direct {
                match dp {
                    Some(dp) => {
                        let handoff = if is_holder {
                            let Some(engine_listen) = cfg.engine_listen else {
                                state.lock().unwrap().punch_route.record(None);
                                return Json::obj(vec![
                                    ("ok", Json::b(false)),
                                    ("route", Json::s("relay-fallback")),
                                    ("reason", Json::s(
                                        "holder 侧缺 engine.listen，不能猜 bt_port 做 splice")),
                                    ("s6_status", Json::s("not_run")),
                                ]);
                            };
                            // holder 必须走「先唤醒再 splice」的变体：入向握手撞到冷种子时，
                            // 引擎里的任务还是 paused，直接拼过去一个字节都不回。
                            let cfg_wake = cfg.clone();
                            let state_wake = state.clone();
                            dp.attach_punched_inbound_waking(
                                targets.bridge, stream, engine_listen,
                                move |prefix: &[u8]| {
                                    !matches!(
                                        crate::wake_for_inbound_handshake(
                                            &cfg_wake, &state_wake, prefix),
                                        crate::InboundHandshake::NonBt
                                            | crate::InboundHandshake::Refused)
                                })
                        } else {
                            dp.attach_punched_socket(targets.bridge, stream)
                        };
                        match handoff {
                            Ok(()) => true,
                            Err(e) => {
                                state.lock().unwrap().punch_route.record(None);
                                return Json::obj(vec![
                                    ("ok", Json::b(false)),
                                    ("route", Json::s("relay-fallback")),
                                    ("reason", Json::s(format!("打洞成功但登记数据面失败: {e}"))),
                                    ("s6_status", Json::s("not_run")),
                                ]);
                            }
                        }
                    }
                    None => {
                        state.lock().unwrap().punch_route.record(None);
                        return Json::obj(vec![
                            ("ok", Json::b(false)),
                            ("route", Json::s("relay-fallback")),
                            ("reason", Json::s("nat.punch_route 未创建数据面，无法登记 socket")),
                            ("s6_status", Json::s("not_run")),
                        ]);
                    }
                }
            } else {
                false
            };
            state.lock().unwrap().punch_route.record(Some(route));
            Json::obj(vec![
                ("ok", Json::b(route == cr_nat::Route::Direct && punched_registered)),
                ("route", Json::s(route.label())),
                ("peer", Json::s(&peer_label)),
                ("punched_socket_registered", Json::b(punched_registered)),
                ("tcp_punch_established", Json::b(true)),
                ("discovered_port_used", match discovered_port {
                    Some(port) => Json::n(port as f64),
                    None => Json::Null,
                }),
                ("relay_data_plane", Json::s("tcp-carrier-implemented")),
                ("relay_data_plane_udp", Json::s("udp-carrier-implemented-rst-path-verified")),
                ("is_s6_evidence", Json::b(false)),
                ("g2_claim", Json::b(false)),
                ("s6_status", Json::s("not_run")),
            ])
        }
        "tracker" => {
            let requester = p.first().map(String::as_str).unwrap_or("127.0.0.1");
            let target = tracker_target(state, cfg, requester);
            Json::obj(vec![
                ("requester_ip", Json::s(requester)),
                ("requester_is_public", Json::b(is_public_ipv4(requester))),
                ("peer_ip", match &target { Some((ip, _)) => Json::s(ip), None => Json::Null }),
                ("peer_port", Json::n(target.map(|(_, p)| p as f64).unwrap_or(0.0))),
                ("s6_status", Json::s("not_run")),
            ])
        }
        "reach" => {
            // 只查不改：给验收/运维一个「中继已经知道对端是哪一类」的独立观察点。
            // 不发包、不唤醒、不改状态，避免在量具准备阶段污染抓包窗口。
            let (peer_label, relays) = {
                let st = state.lock().unwrap();
                let peer = st.nat.peer_adv.as_ref()
                    .map(|a| a.node_id.clone()).unwrap_or_default();
                let relays = if !cfg.nat_relays.is_empty() {
                    cfg.nat_relays.clone()
                } else {
                    st.nat.relay.clone().into_iter().collect()
                };
                (peer, relays)
            };
            if peer_label.is_empty() || relays.is_empty() {
                return Json::obj(vec![("error", Json::s(
                    "先 nat peer 取到对端通告，并配置 nat.relay/nat.relays"))]);
            }
            let peer_id = node_id_16(&peer_label);
            let started = std::time::Instant::now();
            match cr_nat::relay::lookup_reach_any(&relays, &peer_id, Duration::from_millis(700)) {
                Ok((relay, decl)) => {
                    let class = decl.as_ref().map(|d| d.effective_at(now_ms()));
                    Json::obj(vec![
                        ("relay", Json::s(&relay)),
                        ("declared", Json::b(decl.is_some())),
                        ("class", Json::s(class.map(|c| c.label()).unwrap_or("unknown"))),
                        ("declared_at_ms", Json::n(
                            decl.as_ref().map(|d| d.declared_at_ms).unwrap_or(0) as f64)),
                        ("ttl_ms", Json::n(
                            decl.as_ref().map(|d| d.ttl_ms).unwrap_or(0) as f64)),
                        ("error", Json::Null),
                        ("rtt_ms", Json::n(started.elapsed().as_millis() as f64)),
                        ("s6_status", Json::s("not_run")),
                    ])
                }
                Err(e) => Json::obj(vec![
                    ("relay", Json::Null),
                    ("declared", Json::b(false)),
                    ("class", Json::s("unknown")),
                    ("error", Json::s(&e)),
                    ("rtt_ms", Json::n(started.elapsed().as_millis() as f64)),
                    ("s6_status", Json::s("not_run")),
                ]),
            }
        }
        "status" => {
            let st = state.lock().unwrap();
            let report = st.nat.report_or_empty();
            Json::obj(vec![
                ("enabled", Json::b(st.nat.enabled)),
                ("probed", Json::b(st.nat.report.is_some())),
                ("nat_type", Json::s(report.nat_type.label())),
                ("strategy", Json::s(report.recommended_strategy.label())),
                ("peer_advertised", Json::b(st.nat.peer_adv.is_some())),
                ("last_wake_path", match &st.nat.last_wake_path {
                    Some(v) => Json::s(v), None => Json::Null,
                }),
                ("s6_status", Json::s("not_run")),
            ])
        }
        other => Json::obj(vec![("error", Json::s(format!("未知 nat 子命令 {}", other)))]),
    }
}

/// punch-route 里有两个**不能混淆**的对端地址（w54 真引擎 e2e 的桥键口径）：
/// - `syn`：实际发 SYN 的目的地址。DOC-30 发现端口只改这里的端口（IP 不变）。
/// - `bridge`：socket 登记进 SOCKS5 桥的键，**必须等于引擎拨号的地址**。
///   引擎的 initial_peers 来自 `nat peer` 拿到的静态 `bt_external`（main.rs），
///   它不知道、也不会拨发现端口。若用 `syn` 当桥键，engine 拨静态地址时
///   `bridge.take()` miss → 回落真实直连 → 打洞等于白做。故两者必须分离。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PunchTargets {
    syn: SocketAddr,
    bridge: SocketAddr,
}

impl PunchTargets {
    fn new(advertised: SocketAddr, discovered_port: Option<u16>) -> Self {
        let syn = match discovered_port {
            Some(port) => SocketAddr::new(advertised.ip(), port),
            None => advertised,
        };
        // 桥键恒为静态通告地址：发 SYN 用发现端口，接引擎仍按静态地址查表。
        Self { syn, bridge: advertised }
    }
}

/// DOC-30：发现端口只替换端口，不替换 IP。拒绝 0 / 非数字 / 越界，
/// 未给参数时返回 None（保持静态 bt_external 的旧行为）。
fn parse_discovered_port(raw: Option<&String>) -> Result<Option<u16>, String> {
    match raw {
        None => Ok(None),
        Some(s) => match s.parse::<u16>() {
            Ok(port) if port != 0 => Ok(Some(port)),
            _ => Err("第三个参数必须是 1..65535 的发现端口".into()),
        },
    }
}

/// 双端真实同时打开时的共享会话 nonce（32 位十六进制 = 16 字节）。
/// 未给参数时保持旧的随机 nonce 行为；给了就必须严格解析，不猜、不补零。
fn parse_shared_nonce(raw: Option<&String>) -> Result<Option<[u8; 16]>, String> {
    let Some(s) = raw else { return Ok(None) };
    if s.len() != 32 {
        return Err("第四个参数必须是 32 位十六进制 nonce".into());
    }
    let bytes = s.as_bytes();
    let mut out = [0u8; 16];
    for i in 0..16 {
        let hi = (bytes[i * 2] as char).to_digit(16)
            .ok_or_else(|| "第四个参数含非十六进制字符".to_string())?;
        let lo = (bytes[i * 2 + 1] as char).to_digit(16)
            .ok_or_else(|| "第四个参数含非十六进制字符".to_string())?;
        out[i] = ((hi << 4) | lo) as u8;
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertisement_json_roundtrip_keeps_all_endpoints() {
        let a = local_advertisement("A", "192.168.1.210", 17801, 17802,
                                    &empty_report(), Some("127.0.0.1:7000".into()), 123);
        let back = advertisement_from_json(&advertisement_json(&a)).unwrap();
        assert_eq!(back.node_id, "A");
        assert_eq!(back.endpoints.len(), a.endpoints.len());
        assert_eq!(back.endpoints[0].kind, EndpointKind::PrivateLan);
        assert_eq!(back.relay.as_deref(), Some("127.0.0.1:7000"));
        assert_eq!(back.probed_at, 123);
    }

    #[test]
    fn tampered_advertisement_is_rejected() {
        let j = Json::obj(vec![("node_id", Json::s("A"))]);
        assert!(advertisement_from_json(&j).is_err());
        let bad = Json::obj(vec![
            ("node_id", Json::s("A")),
            ("endpoints", Json::arr(vec![Json::obj(vec![
                ("kind", Json::s("not-a-kind")), ("ip", Json::s("1.2.3.4")),
                ("port", Json::n(1234.0)), ("internal_port", Json::n(1234.0)),
                ("evidence", Json::s("x")),
            ])])),
        ]);
        assert!(advertisement_from_json(&bad).is_err());
    }

    #[test]
    fn stun_datagram_detector_matches_rfc5389_binding_response() {
        let mut d = vec![0u8; 20];
        d[0] = 0x01; d[1] = 0x01;
        d[4..8].copy_from_slice(&[0x21, 0x12, 0xA4, 0x42]);
        assert!(is_stun_datagram(&d));
        d[1] = 0x00;
        assert!(!is_stun_datagram(&d));
        assert!(!is_stun_datagram(b"{\"t\":\"KAD_FIND\"}"));
    }

    #[test]
    fn empty_report_has_no_public_egress_and_no_fake_mapping() {
        let r = empty_report();
        assert!(!r.public_udp_egress_ok);
        assert!(r.mapped_address.is_none());
        assert_eq!(r.nat_type, NatType::Unknown);
        assert_eq!(choose_strategy(&r, true), Strategy::Relay);
    }

    #[test]
    fn only_live_relay_only_declaration_skips_direct() {
        let now = now_ms();
        let relay_only = ReachDeclaration {
            class: ReachClass::RelayOnly,
            nat_class: NatType::Symmetric,
            declared_at_ms: now,
            ttl_ms: 60_000,
        };
        let direct = ReachDeclaration { class: ReachClass::Direct, ..relay_only.clone() };
        let stale = ReachDeclaration {
            declared_at_ms: now - 120_000,
            ..relay_only.clone()
        };
        assert!(reach_skips_direct(Some(&relay_only)));
        assert!(!reach_skips_direct(Some(&direct)));
        assert!(!reach_skips_direct(Some(&stale)), "TTL 过期必须退回 unknown");
        assert!(!reach_skips_direct(None), "中继没声明过 ≠ 对方连不上");
    }

    #[test]
    fn plan_json_marks_evidence_as_not_valid_for_pass() {
        let p = build_plan(&empty_report(),
            &local_advertisement("A", "127.0.0.1", 18801, 18802, &empty_report(), None, 0),
            None);
        let j = plan_json(&p);
        assert_eq!(j.get("s6_status").and_then(Json::as_str), Some("not_run"));
        assert_eq!(j.get("evidence_valid_for_pass").and_then(Json::as_bool), Some(false));
        assert_eq!(j.get("candidates").and_then(Json::as_arr).unwrap().len(), 1);
    }

    /// W5.1d：配了公网映射就必须 ① 监听 0.0.0.0 ② 种子带两条 announce
    /// ③ 仍然不许自称「入向已验证」。
    #[test]
    fn public_status_ties_config_to_listen_and_announce() {
        let dir = std::env::temp_dir().join(format!("cr_pub_status_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mk = |bind: &str, nat: &str| -> Config {
            let text = format!(r#"{{
                "bind": "{}",
                "nodes": {{
                    "A": {{"http_port": 17800, "wake_udp_port": 17801,
                           "bt_port": 17802, "host": "192.168.1.210"}},
                    "B": {{"http_port": 17900, "wake_udp_port": 17901, "bt_port": 17902}}
                }},
                "sample_path": "s.bin", "keywords": [], "workdir": ".run",
                "golden_req": "r.json", "golden_ack": "a.json"{}
            }}"#, bind, nat);
            let p = dir.join("c.json");
            std::fs::write(&p, text).unwrap();
            let mut c = Config::load(&p).unwrap();
            c.node_id = "A".into();
            c
        };

        let off = public_status(&mk("127.0.0.1", ""));
        assert_eq!(off.get("engine_listen").and_then(Json::as_str),
                   Some("127.0.0.1:17802"));
        assert_eq!(off.get("announce").and_then(Json::as_arr).unwrap().len(), 1);
        assert_eq!(off.get("inbound_verified").and_then(Json::as_bool), Some(false));

        let on = public_status(&mk("192.168.1.210",
            r#", "nat": {"enabled": true, "bt_external": "112.5.60.4:17802",
                          "http_external": "112.5.60.4:17800"}"#));
        assert_eq!(on.get("engine_listen").and_then(Json::as_str),
                   Some("0.0.0.0:17802"),
                   "有公网映射时必须监听所有接口，否则外部包进不来而本机全绿");
        assert_eq!(on.get("announce").and_then(Json::as_arr).unwrap().len(), 2);
        assert_eq!(on.get("bt_external").and_then(Json::as_str), Some("112.5.60.4:17802"));
        // 配置正确 ≠ 入向可用：这一条永远不能变成 true。
        assert_eq!(on.get("inbound_verified").and_then(Json::as_bool), Some(false));
        assert_eq!(on.get("s6_status").and_then(Json::as_str), Some("not_run"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discovered_port_override_accepts_only_nonzero_u16() {
        assert_eq!(parse_discovered_port(None), Ok(None));
        let p = |s: &str| Some(s.to_string());
        assert_eq!(parse_discovered_port(p("21249").as_ref()), Ok(Some(21249)));
        assert_eq!(parse_discovered_port(p("65535").as_ref()), Ok(Some(65535)));
        assert!(parse_discovered_port(p("0").as_ref()).is_err());
        assert!(parse_discovered_port(p("65536").as_ref()).is_err());
        assert!(parse_discovered_port(p("1.2.3.4:19910").as_ref()).is_err());
        assert!(parse_discovered_port(p("abc").as_ref()).is_err());
        // 只换端口，IP 必须保留已登记的对端公网地址。
        let base: std::net::SocketAddr = "112.5.60.4:19910".parse().unwrap();
        let overridden = std::net::SocketAddr::new(base.ip(), 21249);
        assert_eq!(overridden.ip(), base.ip());
        assert_eq!(overridden.port(), 21249);
    }

    #[test]
    fn shared_nonce_accepts_exactly_16_bytes_and_rejects_garbage() {
        assert_eq!(parse_shared_nonce(None), Ok(None));
        let n = |s: &str| Some(s.to_string());
        assert_eq!(
            parse_shared_nonce(n("00112233445566778899aabbccddeeff").as_ref()),
            Ok(Some([
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
                0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
            ]))
        );
        assert!(parse_shared_nonce(n("0011").as_ref()).is_err());
        assert!(parse_shared_nonce(n("00112233445566778899aabbccddeefg").as_ref()).is_err());
        assert!(parse_shared_nonce(n("00112233445566778899aabbccddeeff00").as_ref()).is_err());
    }

    #[test]
    fn holder_handoff_role_is_explicit_and_fail_closed() {
        // 老配置的缺省语义：A 是 holder。
        assert!(is_holder_role("A", None));
        assert!(!is_holder_role("B", None));
        // 显式 role 优先：节点名叫 A 也可以是 member，不能误拼到引擎口。
        assert!(is_holder_role("X", Some("holder")));
        assert!(!is_holder_role("A", Some("member")));
        assert!(!is_holder_role("B", Some("member")));
        // 拼错角色会让 member 的 socket 从引擎入向进来，形成无人拨号/0 B 的假成功。
        assert!(!is_holder_role("X", Some("unknown")));
    }

    #[test]
    fn punch_targets_separate_syn_port_from_engine_bridge_key() {
        // 无发现端口：与旧行为逐字节一致，syn == bridge。
        let advertised: SocketAddr = "112.5.60.4:19910".parse().unwrap();
        let plain = PunchTargets::new(advertised, None);
        assert_eq!(plain.syn, advertised);
        assert_eq!(plain.bridge, advertised);

        // 有发现端口：SYN 打到发现端口，但桥键必须仍是静态通告地址。
        // 否则引擎按静态 bt_external 拨号时 bridge.take() miss，回落直连。
        let with_disc = PunchTargets::new(advertised, Some(20811));
        assert_eq!(with_disc.syn, "112.5.60.4:20811".parse().unwrap());
        assert_eq!(with_disc.bridge, advertised);
        assert_ne!(with_disc.syn, with_disc.bridge);
    }
}
