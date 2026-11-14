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

use crate::{http_get, NodeState, NODE_SECRET};
use cr_core::{nonce_hex, unix_ts, Config, Json};
use cr_nat::{
    build_plan, choose_strategy, classify_cone_subtype, classify_from_mappings, cgnat_suspected,
    holepunch, is_public_ipv4, local_advertisement, needs_relay, node_id_16, stun::StunResult,
    tracker_endpoint_for, Endpoint, EndpointKind, EndpointPlan, NatType, PeerAdvertisement,
    Strategy, Traversal, TraversalReport,
};
use std::fs;
use std::net::UdpSocket;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

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
    let mapped: Vec<String> = results.iter().filter_map(|r| r.mapped.clone()).collect();
    if !mapped.is_empty() {
        report.public_udp_egress_ok = true;
        report.mapped_address = Some(mapped[0].clone());
        report.nat_type = classify_from_mappings(&mapped);
        if let Some(first) = results.iter().find(|r| r.ok) {
            report.nat_type = classify_cone_subtype(
                first.mapped.as_deref().unwrap_or(""), &first.other);
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
    };

    for ep in plan.candidates.iter()
        .filter(|e| only_kind.map(|k| e.kind == k).unwrap_or(true))
    {
        if ep.kind == EndpointKind::Relay {
            match holepunch::relay_wake(&ep.addr(), &self_id, &peer_id, &wake, timeout) {
                Ok(payload) => match Json::parse(&String::from_utf8_lossy(&payload)) {
                    Ok(reply) => {
                        out.attempts.push(attempt_json("relay", &ep.addr(), true,
                                                       "真实 DELIVERED 帧", 0));
                        out = finish(out, state, req, reply, 0, ep);
                        return out;
                    }
                    Err(e) => out.attempts.push(attempt_json("relay", &ep.addr(), false,
                                                             &format!("中继应答非法 JSON: {}", e), 0)),
                },
                Err(e) => out.attempts.push(attempt_json("relay", &ep.addr(), false, &e, 0)),
            }
            if out.result.is_ok() { return out; }
            continue;
        }
        let a = holepunch::wake_via_endpoint(&cfg.bind, ep, &cfg.node_id, &token,
                                             &wake, 2, timeout);
        let detail = match (&a.error, a.reply.as_ref()) {
            (Some(e), _) => e.clone(),
            (None, Some(_)) => format!("punch_sent={} punch_acked={}", a.punch_sent, a.punch_acked),
            (None, None) => "无应答".into(),
        };
        out.attempts.push(attempt_json(ep.kind.label(), &ep.addr(), a.ok(), &detail, a.rtt_ms));
        if a.ok() {
            let text = String::from_utf8_lossy(a.reply.as_ref().unwrap());
            match Json::parse(&text) {
                Ok(reply) => {
                    let o = finish(out, state, req, reply, a.rtt_ms, ep);
                    return o;
                }
                Err(e) => {
                    out.attempts.push(attempt_json(ep.kind.label(), &ep.addr(), false,
                                                   &format!("应答非法 JSON: {}", e), a.rtt_ms));
                }
            }
        }
    }
    if out.result.is_err() {
        let failed = out.attempts.len();
        out.result = Err(format!("全部 {} 个候选均失败（真实超时/拒绝），不回填默认应答", failed));
    }
    out
}

fn finish(mut out: WakeOutcome, state: &Shared, req: &Json, reply: Json,
          rtt_ms: u64, ep: &Endpoint) -> WakeOutcome {
    out.result = Ok((reply.clone(), rtt_ms));
    out.path = Some(ep.kind.label().to_string());
    let mut st = state.lock().unwrap();
    st.nat.last_wake_path = Some(ep.kind.label().to_string());
    if reply.get("status").and_then(Json::as_str) == Some("OK") {
        st.last_ack = Some(reply);
        st.last_file_hash = req.get("file_hash").and_then(Json::as_str).map(str::to_string);
    }
    out
}

// ---------------------------------------------------------------------------
// `nat` CLI 子命令分发
// ---------------------------------------------------------------------------

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
                ("ack_status", Json::s(ack.get("status").and_then(Json::as_str).unwrap_or(""))),
                ("s6_status", Json::s("not_run")),
                ("evidence_valid_for_pass", Json::b(false)),
                ("error", match o.result { Err(e) => Json::s(e), Ok(_) => Json::Null }),
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
    fn plan_json_marks_evidence_as_not_valid_for_pass() {
        let p = build_plan(&empty_report(),
            &local_advertisement("A", "127.0.0.1", 18801, 18802, &empty_report(), None, 0),
            None);
        let j = plan_json(&p);
        assert_eq!(j.get("s6_status").and_then(Json::as_str), Some("not_run"));
        assert_eq!(j.get("evidence_valid_for_pass").and_then(Json::as_bool), Some(false));
        assert_eq!(j.get("candidates").and_then(Json::as_arr).unwrap().len(), 1);
    }
}
