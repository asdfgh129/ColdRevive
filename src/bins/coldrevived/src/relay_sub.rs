//! M2/W3.4 持有者侧中继订阅线程 + W3.8 多中继切换。
//!
//! W3.3 的中继回退由 T2 的 mock 桥「服务端自己发 UDP」完成，服务端掺了业务逻辑，
//! 无法部署。W3.4 改为：
//!
//!   请求侧 `RELAY(target=持有者, WAKEUP_REQ)`
//!     → [`cr_nat::relay_server`] 纯帧路由，封装 `DELIVERED(node=请求会话 id)`
//!     → 本线程把负载经**真实 UDP** 送进本守护进程的唤醒监听端口
//!     → 守护进程 `handle_udp_frame` 产生**真实 WAKEUP_ACK**
//!     → 本线程 `RELAY(target=请求会话 id, WAKEUP_ACK)`
//!     → 服务端回投 `DELIVERED` 给请求侧。
//!
//! 订阅注册用稳定 node id（便于被 `node_id_16(peer.node_id)` 命中）；请求侧用一次性
//! 会话 id 注册，避免同一节点两个连接抢占注册表表项。中继服务端不解析唤醒协议。
//!
//! W3.8：`nat.relay` 可以是列表。本线程按顺序尝试，**任一个真实注册成功**就沿用；
//! 断开后从下一个开始重试，从而在单个中继实例宕机时自动切换。切换是顺序的，不做并发
//! 抢连——多中继在这里是「可切换」而不是任播。
//!
//! **红线**：本线程不构造、不改写任何业务应答；UDP 无应答时静默跳过，让请求侧如实超时。
//! `nat` 配置段缺省时完全不启动，T0/T1 行为不变。
//!
//! # v2.4.92：数据面 splice（持有者侧）
//!
//! 订阅连接收到 `DELIVERED` 后先按 magic 分成两类：
//!
//! - payload 以 `CD` 开头 → 数据面（ADR-023）：交给 [`TunnelMux`]；
//! - 其余 → 唤醒路径，原样投给本守护进程（行为与之前逐字节一致）。
//!
//! 数据面收到 `FLAG_OPEN` 时：**拨本机引擎监听口 → 拨通了才回 `OPENED` → 起线程做双向
//! splice**。顺序不能反：先回 `OPENED` 再拨号，等于对下载方谎称「已经接上了」，
//! 而它那边会把一条空隧道登记进 SOCKS5 桥，最终表现成 BT 连接凭空 EOF。
//!
//! 一条订阅连接服务所有请求方，所以数据帧的出向目的地逐帧携带，不能固定成构造时的
//! `peer`。也正因为共用一条连接，数据面不会在注册表里顶掉订阅表项。

use cr_core::Config;
use cr_nat::node_id_16;
use cr_nat::reach::{now_ms, ReachDeclaration};
use cr_nat::relay::{FrameKind, RelayClient, RelayConn, NODE_ID_LEN};
use cr_nat::relay_tunnel::{
    encode, splice_blocking_with, Inbound, TunnelFrame, TunnelMux, MAGIC, POLL_INTERVAL,
};
use cr_nat::relay_udp::{UdpKind, UdpRelayClient};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const WAKE_REPLY_TIMEOUT: Duration = Duration::from_millis(1500);
const RECONNECT_DELAY: Duration = Duration::from_secs(1);
/// 订阅连接的读超时兼「声明有没有更新」的轮询周期。
///
/// 500ms 的代价是每次超时一次系统调用；换来的是探测结果一出现（最多延迟半秒）
/// 就能把「我只能走中继」告诉中继。这个值只影响本线程，不碰业务路径。
const SUB_POLL: Duration = Duration::from_millis(500);

/// UDP 承载的会话保活间隔。
///
/// 中继按 `last_seen` 回收空闲会话（`coldrevive-relay` 默认 60 s）。订阅连接平时
/// **一条业务帧都没有**，不主动续期就会在 60 s 后被静默删掉：表现为「进程还在、
/// 日志没报错、但谁都叫不醒它」。取 20 s 是 60 s 的三分之一，够容两次报文丢失。
const UDP_KEEPALIVE: Duration = Duration::from_secs(20);

/// 最近一次探测得出的可达性声明。探测线程写入，订阅线程读取。
///
/// 用进程级槽位而不是把 NodeState 传进来：订阅线程只需要这一个字段，
/// 传整个状态会把两边的锁顺序纠缠在一起。
static LAST_DECL: Mutex<Option<ReachDeclaration>> = Mutex::new(None);

/// 持有者侧数据面的计数器。
///
/// 全部来自真实事件（收到 OPEN、拨号返回、splice 字节数），没有一个是「配置里写了」
/// 或者「理论上应该有」。`/health` 直接照抄，于是「接了但没有数据流过」一眼可见。
pub struct HolderStats {
    pub opens: AtomicU64,
    pub accepted: AtomicU64,
    pub rejected: AtomicU64,
    pub duplicate: AtomicU64,
    pub peer_mismatch: AtomicU64,
    pub unknown_channel: AtomicU64,
    pub data_frames: AtomicU64,
    pub bytes_to_engine: AtomicU64,
    pub bytes_from_engine: AtomicU64,
    pub splices_done: AtomicU64,
}

impl HolderStats {
    const fn new() -> Self {
        Self {
            opens: AtomicU64::new(0),
            accepted: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            duplicate: AtomicU64::new(0),
            peer_mismatch: AtomicU64::new(0),
            unknown_channel: AtomicU64::new(0),
            data_frames: AtomicU64::new(0),
            bytes_to_engine: AtomicU64::new(0),
            bytes_from_engine: AtomicU64::new(0),
            splices_done: AtomicU64::new(0),
        }
    }
}

static HOLDER: HolderStats = HolderStats::new();
/// splice 是否启用（只有 holder 且开了 `nat.punch_route`）。`/health` 据此区分
/// 「没配」和「配了但没人来过」。
static HOLDER_ENABLED: Mutex<Option<bool>> = Mutex::new(None);
/// ADR-024：`/health.holder_data.carrier` 必须反映**实际 opt-in 的数据面承载**。
/// 两个承载共用 `HolderStats` 计数器，但标签不能跟着 TCP 写死——否则 UDP 服务
/// 明明搬了 1 MiB，看板还报 `relay-sub-tcp`，排障会去查错的那条路。
static HOLDER_CARRIER: Mutex<&'static str> = Mutex::new("relay-sub-tcp");

fn holder_carrier_label(carrier: &str) -> &'static str {
    if carrier.eq_ignore_ascii_case("udp") {
        "relay-sub-udp"
    } else {
        "relay-sub-tcp"
    }
}

pub fn holder_stats_json() -> cr_core::Json {
    use cr_core::Json;
    let get = |a: &AtomicU64| Json::n(a.load(Ordering::Relaxed) as f64);
    let enabled = HOLDER_ENABLED.lock().unwrap().unwrap_or(false);
    let carrier = if enabled {
        *HOLDER_CARRIER.lock().unwrap()
    } else {
        "none"
    };
    Json::obj(vec![
        ("enabled", Json::b(enabled)),
        ("carrier", Json::s(carrier)),
        ("opens", get(&HOLDER.opens)),
        ("accepted", get(&HOLDER.accepted)),
        ("rejected", get(&HOLDER.rejected)),
        ("duplicate_open", get(&HOLDER.duplicate)),
        ("peer_mismatch", get(&HOLDER.peer_mismatch)),
        ("unknown_channel", get(&HOLDER.unknown_channel)),
        ("data_frames", get(&HOLDER.data_frames)),
        ("bytes_to_engine", get(&HOLDER.bytes_to_engine)),
        ("bytes_from_engine", get(&HOLDER.bytes_from_engine)),
        ("splices_done", get(&HOLDER.splices_done)),
    ])
}

/// 本地引擎监听口的拨号地址。纯函数部分单独拿出来，便于不起进程就能测。
///
/// 引擎监听可能是 `0.0.0.0:port`、`<bind>:port` 或显式 `engine_listen`。三种都要归一成
/// 「本机确实能连上」的地址：未指定地址 → 回环；指定了具体 IP → 用该 IP
/// （绑在 LAN IP 上的监听不接回环，写死 127.0.0.1 会连不上）。
fn dial_target_parts(
    is_holder: bool,
    punch_route: bool,
    bt_port: u16,
    listen: Option<SocketAddr>,
    bind: &str,
) -> Option<SocketAddr> {
    if !is_holder || !punch_route || bt_port == 0 {
        return None;
    }
    let host = match listen {
        Some(a) if !a.ip().is_unspecified() => a.ip().to_string(),
        _ => local_wake_host(bind),
    };
    format!("{host}:{bt_port}").parse().ok()
}

fn holder_dial_target(cfg: &Config) -> Option<SocketAddr> {
    // 与 main 里 `is_holder` 同一口径（角色显式优先，缺省退回节点名字面量）。
    let is_holder = cfg.role.as_deref().map(|r| r == "holder").unwrap_or(cfg.node_id == "A");
    let bt_port = cfg.me().map(|m| m.bt_port).unwrap_or(0);
    dial_target_parts(is_holder, cfg.nat_punch_route, bt_port, cfg.engine_listen, &cfg.bind)
}

/// 探测完成后由 nat_path 调用。**只在真的探测过之后调用**——
/// 没测过就不要造一个「看起来像测过」的声明。
pub fn update_declaration(d: ReachDeclaration) {
    *LAST_DECL.lock().unwrap() = Some(d);
}

fn current_declaration() -> ReachDeclaration {
    let mut g = LAST_DECL.lock().unwrap();
    // 「没探测过」的兜底声明在这里**只盖章一次**并缓存：
    // 若每次调用都取当前时间，订阅循环会把「没有更新」误判成「有更新」，
    // 每 500ms 重声明一次直到永远——服务端计数会无声地涨满。
    if g.is_none() {
        *g = Some(ReachDeclaration::unknown(now_ms()));
    }
    g.clone().unwrap()
}

/// 守护进程 UDP 监听绑在 `cfg.bind`；`0.0.0.0` 时本机用回环地址投递。
pub fn local_wake_host(bind: &str) -> String {
    match bind {
        "0.0.0.0" | "::" | "[::]" | "" => "127.0.0.1".to_string(),
        other => other.to_string(),
    }
}

/// 只在 `nat.enabled` 且配了中继（`nat.relay` 或 `nat.relay_udp`）时启动；
/// 失败自动重连，不阻塞主线程。
/// UDP 可靠层拥塞控制配置（opt-in，默认关闭）；两端同一份配置语义。
fn udp_cc_from_config(cfg: &Config) -> cr_nat::relay_udp_stream::CongestionConfig {
    cr_nat::relay_udp_stream::CongestionConfig {
        enabled: cfg.nat_udp_cc_enabled,
        cwnd_init: cfg.nat_udp_cc_cwnd_init,
        cwnd_min: 1,
        cwnd_max: 32,
        beta: cfg.nat_udp_cc_beta_x100 as f64 / 100.0,
        pacing: cfg.nat_udp_cc_pacing,
    }
}

pub fn spawn(cfg: &Config) {
    if !cfg.nat_enabled {
        return;
    }
    // W3.8：列表优先；旧配置（单个 relay）自然等价于长度 1 的列表。
    let relays: Vec<String> = if cfg.nat_relays.is_empty() {
        cfg.nat_relay.clone().into_iter().collect()
    } else {
        cfg.nat_relays.clone()
    };
    let relays_udp = cfg.nat_relay_udps.clone();
    if relays.is_empty() && relays_udp.is_empty() {
        return;
    }
    let wake_port = match cfg.me() {
        Ok(m) => m.wake_udp_port,
        Err(_) => return,
    };
    if wake_port == 0 {
        return;
    }
    let self_id = node_id_16(&cfg.node_id);
    let wake_host = local_wake_host(&cfg.bind);
    // 持有者侧数据面：只有真的能拨到本机引擎监听口才启用。members 没有引擎，
    // 给它们算出地址只会让对端收到 OPENED 后才发现没有数据。
    let dial = holder_dial_target(cfg);
    *HOLDER_ENABLED.lock().unwrap() = Some(dial.is_some());
    *HOLDER_CARRIER.lock().unwrap() = holder_carrier_label(&cfg.nat_data_plane_carrier);
    if let Some(a) = dial {
        eprintln!("[relay-sub] 持有者数据面已启用: 隧道 → 本地引擎 {a}（TCP 承载）");
    }

    // TCP 承载：一条线程，按列表顺序切换。
    if !relays.is_empty() {
        let label = cfg.node_id.clone();
        let wake_host = wake_host.clone();
        thread::spawn(move || {
            let mut start = 0usize;
            loop {
                let (addr, res) =
                    serve_from(&relays, start, &self_id, &wake_host, wake_port, dial);
                if let Err(e) = res {
                    eprintln!("[{}][relay-sub] 订阅中断（{}）: {}，{}s 后切换重试",
                              label, addr, e, RECONNECT_DELAY.as_secs());
                }
                // 断开后从下一个中继开始，避免对已宕机的实例反复重连。
                start = (start + 1) % relays.len();
                thread::sleep(RECONNECT_DELAY);
            }
        });
    }

    // v2.4.86：UDP 承载独立一条线程。两种承载可以同时跑（同一节点的两条路），
    // 一条断了不影响另一条；各自有自己的重连与切换。
    if !relays_udp.is_empty() {
        let label = cfg.node_id.clone();
        let udp_for_sub = relays_udp.clone();
        thread::spawn(move || {
            let mut start = 0usize;
            loop {
                let addr = &udp_for_sub[start % udp_for_sub.len()];
                if let Err(e) = serve_udp(addr, &self_id, &wake_host, wake_port, UDP_KEEPALIVE) {
                    eprintln!("[{}][relay-sub] UDP 订阅中断（{}）: {}，{}s 后切换重试",
                              label, addr, e, RECONNECT_DELAY.as_secs());
                }
                start = (start + 1) % udp_for_sub.len();
                thread::sleep(RECONNECT_DELAY);
            }
        });
    }

    // ADR-024 阶段 2：UDP 数据面持有方。仅在 opt-in 且本机是持有者时启动。
    // 用**派生会话 id**单独注册一条 UDP 连接（与 node_id 的订阅会话分离，避免互相顶掉），
    // 收到 OPEN → 先拨通本地引擎 → 回 OPENED → 双向 splice。
    if cfg.nat_data_plane_carrier.eq_ignore_ascii_case("udp") && !relays_udp.is_empty() {
        if let Some(target) = dial {
            let label = cfg.node_id.clone();
            let relay_list = relays_udp.clone();
            let cc = udp_cc_from_config(cfg);
            thread::spawn(move || {
                let mut start = 0usize;
                loop {
                    let addr = &relay_list[start % relay_list.len()];
                    if let Err(e) = serve_udp_data_plane(addr, &self_id, target, cc) {
                        eprintln!("[{}][relay-sub] UDP 数据面中断（{}）: {}，{}s 后重连",
                                  label, addr, e, RECONNECT_DELAY.as_secs());
                    }
                    start = (start + 1) % relay_list.len();
                    thread::sleep(RECONNECT_DELAY);
                }
            });
            eprintln!("[relay-sub] UDP 数据面持有方已启用: 派生会话 → 本地引擎 {target}");
        }
    }
}

/// 从 `start` 开始依次尝试列表里的中继，返回实际使用的地址与结果。
fn serve_from(
    relays: &[String],
    start: usize,
    self_id: &[u8; NODE_ID_LEN],
    wake_host: &str,
    wake_port: u16,
    dial: Option<SocketAddr>,
) -> (String, Result<(), String>) {
    let n = relays.len();
    let mut errs: Vec<String> = Vec::new();
    for k in 0..n {
        let addr = &relays[(start + k) % n];
        match serve(addr, self_id, wake_host, wake_port, dial) {
            Ok(()) => return (addr.clone(), Ok(())),
            Err(e) => errs.push(format!("{}: {}", addr, e)),
        }
    }
    (relays[start % n].clone(), Err(format!("全部 {} 个中继均失败（{}）",
                                            n, errs.join("; "))))
}

/// 把出向队列排空到中继连接上。数据帧与控制帧走同一条队列，顺序天然一致。
fn flush_out(conn: &mut RelayConn, out: &Receiver<([u8; NODE_ID_LEN], TunnelFrame)>)
    -> Result<u64, String>
{
    let mut sent = 0u64;
    while let Ok((dst, tf)) = out.try_recv() {
        let payload = encode(&tf)?;
        conn.send_relay(&dst, &payload)?;
        sent += payload.len() as u64;
    }
    Ok(sent)
}

/// 处理一条数据面帧。**不阻塞订阅循环**：拨号 + splice 全在独立线程里，
/// 否则一条 BT 连接就能把唤醒路径一起卡住（唤醒与数据必须互不拖累）。
fn handle_data_frame(
    mux: &Arc<TunnelMux>,
    from: [u8; NODE_ID_LEN],
    payload: &[u8],
    dial: Option<SocketAddr>,
) {
    match mux.dispatch(from, payload) {
        Inbound::Open { channel, peer, tunnel } => {
            HOLDER.opens.fetch_add(1, Ordering::Relaxed);
            let Some(target) = dial else {
                // 本机不是持有者（或没开 `nat.punch_route`）：不回 OPENED，
                // 让对端如实超时并回落直连，而不是收下一条永远没有数据的隧道。
                HOLDER.rejected.fetch_add(1, Ordering::Relaxed);
                eprintln!("[relay-sub] 收到数据面 OPEN（channel={channel} peer={}），\
                           但本机未启用 splice，不回 OPENED", cr_hash::hex(&peer));
                return;
            };
            eprintln!("[relay-sub] 数据面 OPEN: channel={channel} peer={} → 拨本地引擎 {target}",
                      cr_hash::hex(&peer));
            thread::spawn(move || {
                // 先拨通再回 OPENED：「已确认」必须等于「本地已经接上」。
                let sock = match TcpStream::connect_timeout(&target, CONNECT_TIMEOUT) {
                    Ok(s) => s,
                    Err(e) => {
                        HOLDER.rejected.fetch_add(1, Ordering::Relaxed);
                        eprintln!("[relay-sub] 拨本地引擎 {target} 失败: {e}，不回 OPENED");
                        return;
                    }
                };
                if let Err(e) = tunnel.send_opened() {
                    HOLDER.rejected.fetch_add(1, Ordering::Relaxed);
                    eprintln!("[relay-sub] 回 OPENED 失败: {e}");
                    return;
                }
                HOLDER.accepted.fetch_add(1, Ordering::Relaxed);
                // 边传边计数：整条连接可能跑很久，只在收尾时一次性加会让 /health
                // 在传输期间一直显示 0 字节。
                let progress: cr_nat::relay_tunnel::ProgressFn = Arc::new(|to_up, n| {
                    let target = if to_up {
                        &HOLDER.bytes_to_engine
                    } else {
                        &HOLDER.bytes_from_engine
                    };
                    target.fetch_add(n, Ordering::Relaxed);
                });
                match splice_blocking_with(tunnel, sock, Some(progress)) {
                    Ok((to_engine, from_engine)) => {
                        eprintln!("[relay-sub] 数据面结束: channel={channel} \
                                   →引擎 {to_engine} B，←引擎 {from_engine} B");
                    }
                    Err(e) => {
                        HOLDER.rejected.fetch_add(1, Ordering::Relaxed);
                        eprintln!("[relay-sub] 数据面 splice 异常: channel={channel}: {e}");
                    }
                }
                HOLDER.splices_done.fetch_add(1, Ordering::Relaxed);
            });
        }
        Inbound::Data(_) => { HOLDER.data_frames.fetch_add(1, Ordering::Relaxed); }
        Inbound::DuplicateOpen(c) => { HOLDER.duplicate.fetch_add(1, Ordering::Relaxed);
                                        eprintln!("[relay-sub] 重复 OPEN，忽略: channel={c}"); }
        Inbound::PeerMismatch(c) => { HOLDER.peer_mismatch.fetch_add(1, Ordering::Relaxed);
                                       eprintln!("[relay-sub] 来源与 channel 记录不符，拒绝: channel={c}"); }
        Inbound::Unknown(c) => { HOLDER.unknown_channel.fetch_add(1, Ordering::Relaxed);
                                  eprintln!("[relay-sub] 未开 channel 就收到数据: channel={c}"); }
        Inbound::Ignored | Inbound::Close(_) | Inbound::Opened(_) => {}
    }
}

fn serve(
    relay: &str,
    self_id: &[u8; NODE_ID_LEN],
    wake_host: &str,
    wake_port: u16,
    dial: Option<SocketAddr>,
) -> Result<(), String> {
    let client = RelayClient::new(relay.to_string());
    let mut conn = client.connect(CONNECT_TIMEOUT)?;
    // W3.12：注册即声明。探测还没跑过时就是 unknown——中继会如实记「未声明」，
    // 而不是替我们猜一个类别。探测跑完后的更新由下面的轮询补发。
    let mut last_sent = current_declaration();
    conn.register_declared(self_id, &last_sent)?;
    eprintln!("[relay-sub] 已订阅中继 {}（可达性声明: {}）",
              relay, current_declaration().class.label());
    conn.set_read_timeout(Some(SUB_POLL))?;

    let (mux, out) = TunnelMux::new();
    let mux = Arc::new(mux);
    let mut poll = SUB_POLL;

    let udp = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("订阅 UDP 绑定失败: {}", e))?;
    udp.set_read_timeout(Some(WAKE_REPLY_TIMEOUT))
        .map_err(|e| e.to_string())?;
    let target = format!("{}:{}", wake_host, wake_port);
    let mut buf = [0u8; 65535];

    loop {
        // 出向帧优先：`send_opened` 只是进队，真正发出去靠这一句。放在读之前，
        // 这样「收到 OPEN → 回 OPENED」不会被 500 ms 读超时拖住。
        flush_out(&mut conn, &out)?;
        // 有活动 channel 时把读超时降到 20 ms：数据面出向不能被「等唤醒」的
        // 500 ms 空闲轮询拖慢。没有 channel 时保持 500 ms，省系统调用。
        let want = if mux.channel_count() > 0 { POLL_INTERVAL } else { SUB_POLL };
        if want != poll {
            conn.set_read_timeout(Some(want))?;
            poll = want;
        }

        let frame = match conn.recv_frame_optional() {
            Ok(Some(f)) => f,
            Ok(None) => {
                // 空闲读超时不等于断链：订阅本来就是「长连接 + 短读超时轮询」。
                // 旧实现在这里 return Err → 每 500ms 断一次、重连一次，
                // 服务端跟着 forget_local 删声明，查询随机落到「1 秒空窗」里变 unknown。
                let cur = current_declaration();
                if cur != last_sent {
                    eprintln!("[relay-sub] 可达性声明更新: {}", cur.class.label());
                    conn.register_declared(self_id, &cur)?;
                    last_sent = cur;
                }
                continue;
            }
            Err(e) => return Err(format!("订阅读帧失败: {}", e)),
        };
        if frame.kind != FrameKind::Delivered || frame.payload.is_empty() {
            continue;
        }
        let source = frame.node;

        // 数据面与唤醒面在**同一连接**上，靠 magic 分流：唤醒负载是 JSON，不会以 `CD` 开头。
        if frame.payload.starts_with(&MAGIC) {
            handle_data_frame(&mux, source, &frame.payload, dial);
            continue;
        }

        udp.send_to(&frame.payload, &target)
            .map_err(|e| format!("唤醒 UDP 发送失败: {}", e))?;
        // 等本机守护进程的**真实**应答。有数据面在跑时用 20 ms 小步等待，
        // 一边等一边排空出向队列——否则一次唤醒就能把数据面拖慢 1.5 s。
        let deadline = Instant::now() + WAKE_REPLY_TIMEOUT;
        loop {
            let live = mux.channel_count() > 0;
            let step = if live {
                POLL_INTERVAL
            } else {
                deadline.saturating_duration_since(Instant::now())
            };
            if step.is_zero() {
                break;
            }
            udp.set_read_timeout(Some(step)).map_err(|e| e.to_string())?;
            match udp.recv_from(&mut buf) {
                Ok((n, _)) => {
                    conn.send_relay(&source, &buf[..n])?;
                    break;
                }
                // 守护进程拒绝/忽略该帧：不合成应答，请求侧按自身超时如实失败。
                Err(_) => {
                    if live {
                        flush_out(&mut conn, &out)?;
                    }
                    if Instant::now() >= deadline {
                        break;
                    }
                }
            }
        }
        udp.set_read_timeout(Some(WAKE_REPLY_TIMEOUT)).map_err(|e| e.to_string())?;
    }
}

/// v2.4.86：UDP 承载上的订阅循环。语义与 [`serve`] 逐条对应：
/// 收到 `DELIVER`（对端发来的唤醒/打洞帧）→ 原样投给本机守护进程的唤醒端口 →
/// 把守护进程的**真实应答**投回来源会话。**不构造任何业务应答**。
///
/// 与 TCP 版的两处差别，都是承载本身带来的，不是实现取舍：
///  1. 没有「注册即声明」这一步——UDP 线缆格式里没有声明帧，所以走 UDP 订阅的节点
///     在中继看来 `declared=false`（`lookup` 会报 unknown）。这只影响「先查再打洞」
///     的优化（不会替对端跳过直连），不影响唤醒能不能送到。
///  2. 必须自己续期：中继按 `last_seen` 回收空闲会话，订阅平时没有业务帧，
///     所以要每 `keepalive` 发一次 `PING`。`PONG` 由本循环当普通帧忽略——
///     **不能用 `UdpRelayClient::ping()`**，那会一直读到 PONG 为止，把夹在中间的
///     `DELIVER` 吞掉（等于悄悄丢一次唤醒）。
fn serve_udp(
    relay: &str,
    self_id: &[u8; NODE_ID_LEN],
    wake_host: &str,
    wake_port: u16,
    keepalive: Duration,
) -> Result<(), String> {
    let mut conn = UdpRelayClient::connect(relay, CONNECT_TIMEOUT)?;
    conn.hello(*self_id, CONNECT_TIMEOUT)
        .map_err(|e| format!("UDP 中继注册失败: {e}"))?;
    eprintln!("[relay-sub] 已订阅 UDP 承载 {}（保活 {}s；该承载不支持可达性声明）",
              relay, keepalive.as_secs());

    let udp = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("订阅 UDP 绑定失败: {}", e))?;
    udp.set_read_timeout(Some(WAKE_REPLY_TIMEOUT))
        .map_err(|e| e.to_string())?;
    let target = format!("{}:{}", wake_host, wake_port);
    let mut buf = [0u8; 65535];
    let mut last_ping = Instant::now();

    loop {
        match conn.recv_frame(SUB_POLL) {
            Ok(Some(f)) if f.kind == UdpKind::Deliver && !f.payload.is_empty() => {
                let source = f.from;
                udp.send_to(&f.payload, &target)
                    .map_err(|e| format!("唤醒 UDP 发送失败: {}", e))?;
                // 守护进程拒绝/忽略该帧：不合成应答，请求侧按自身超时如实失败。
                if let Ok((n, _)) = udp.recv_from(&mut buf) {
                    conn.send_data(&source, &buf[..n])?;
                }
            }
            // PONG / STATUS / 其它控制帧：与订阅无关，丢掉即可。
            Ok(_) => {}
            Err(e) => return Err(format!("UDP 订阅读包失败: {}", e)),
        }
        if last_ping.elapsed() >= keepalive {
            conn.send_ping()?;
            last_ping = Instant::now();
        }
    }
}

/// ADR-024 阶段 2：UDP 数据面持有方循环（派生会话 id，独立连接）。
/// 只服务数据面 OPEN：校验来源 → 拨本地引擎 → 回 OPENED → 双向 splice。
/// 未握手成功前不拨引擎、不回 OPENED，让发起方如实超时回落直连。
fn serve_udp_data_plane(
    relay: &str,
    self_id: &[u8; NODE_ID_LEN],
    dial: SocketAddr,
    cc: cr_nat::relay_udp_stream::CongestionConfig,
) -> Result<(), String> {
    use cr_nat::relay_udp_stream::{
        recv_open, send_opened, splice_stream_with as splice, UdpRelayStream,
    };

    // 与发起方 derive_session_id(self_id, b"cr-dp-v1") 对应：持有方用同样的派生 id
    // 注册数据面会话，发起方 send_data 的目标就是这个派生 id。
    let dp_id = cr_nat::relay_tunnel::derive_session_id(self_id, b"cr-dp-v1");
    let mut client = UdpRelayClient::connect(relay, CONNECT_TIMEOUT)?;
    client
        .hello(dp_id, CONNECT_TIMEOUT)
        .map_err(|e| format!("UDP 数据面注册失败: {e}"))?;
    eprintln!("[relay-sub] UDP 数据面已订阅 {relay}（派生会话），等待 OPEN → 本地引擎 {dial}");

    // 一条连接服务一条 OPEN 后 splice；splice 结束（或出错）后返回，由上层重连。
    // 这与 TCP 侧「持有者 splice 在独立线程」语义一致，只是 UDP 侧本线程专职数据面。
    let mut stream = UdpRelayStream::new_with_cc(client, [0u8; NODE_ID_LEN], cc)?;
    let (channel, initiator) = recv_open(&mut stream)?;
    HOLDER.opens.fetch_add(1, Ordering::Relaxed);
    eprintln!("[relay-sub] UDP 数据面 OPEN: channel={channel} peer={}", cr_hash::hex(&initiator));

    // UdpRelayStream 以全 0 peer 构造，收到 OPEN 的首个 DELIVER 时已从真实来源
    // （= 发起方派生 id）锁定发送目标，后续 OPENED / ACK 都发往正确的对端。
    let sock = match TcpStream::connect_timeout(&dial, CONNECT_TIMEOUT) {
        Ok(s) => s,
        Err(e) => {
            HOLDER.rejected.fetch_add(1, Ordering::Relaxed);
            return Err(format!("拨本地引擎 {dial} 失败: {e}"));
        }
    };
    send_opened(&mut stream)?;
    HOLDER.accepted.fetch_add(1, Ordering::Relaxed);

    let progress: cr_nat::relay_tunnel::ProgressFn = Arc::new(|to_up, n| {
        let c = if to_up { &HOLDER.bytes_to_engine } else { &HOLDER.bytes_from_engine };
        c.fetch_add(n, Ordering::Relaxed);
    });
    let (r, w) = stream.split();
    match splice(r, w, sock, Some(progress)) {
        Ok((to_engine, from_engine)) => {
            eprintln!("[relay-sub] UDP 数据面结束: channel={channel} →引擎 {to_engine} B，←引擎 {from_engine} B");
        }
        Err(e) => {
            HOLDER.rejected.fetch_add(1, Ordering::Relaxed);
            return Err(format!("UDP 数据面 splice 异常: {e}"));
        }
    }
    HOLDER.splices_done.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_bind_uses_loopback_for_local_delivery() {
        assert_eq!(local_wake_host("0.0.0.0"), "127.0.0.1");
        assert_eq!(local_wake_host("::"), "127.0.0.1");
        assert_eq!(local_wake_host(""), "127.0.0.1");
    }

    #[test]
    fn concrete_bind_is_used_verbatim() {
        assert_eq!(local_wake_host("192.168.1.210"), "192.168.1.210");
        assert_eq!(local_wake_host("127.0.0.1"), "127.0.0.1");
    }

    /// ADR-024：UDP 承载搬的字节不能在看板上被标成 TCP，否则排障会查错路。
    #[test]
    fn holder_carrier_label_follows_opt_in_carrier() {
        assert_eq!(holder_carrier_label("udp"), "relay-sub-udp");
        assert_eq!(holder_carrier_label("UDP"), "relay-sub-udp");
        assert_eq!(holder_carrier_label("tcp"), "relay-sub-tcp");
        assert_eq!(holder_carrier_label(""), "relay-sub-tcp");
    }

    /// 拨号目标只对「持有者 + 显式开 punch_route」成立，且三种监听写法都要归一成能连上的地址。
    #[test]
    fn dial_target_only_exists_for_enabled_holder() {
        let wildcard: SocketAddr = "0.0.0.0:17900".parse().unwrap();
        let lan: SocketAddr = "192.168.1.210:17900".parse().unwrap();
        assert_eq!(dial_target_parts(false, true, 17900, None, "0.0.0.0"), None,
                   "member 没有引擎，不得算出拨号目标");
        assert_eq!(dial_target_parts(true, false, 17900, None, "0.0.0.0"), None,
                   "未开 punch_route 时数据面必须保持关闭");
        assert_eq!(dial_target_parts(true, true, 0, None, "0.0.0.0"), None,
                   "bt_port=0 说明没有监听，不能瞎拨");
        assert_eq!(dial_target_parts(true, true, 17900, None, "0.0.0.0"),
                   Some("127.0.0.1:17900".parse().unwrap()));
        assert_eq!(dial_target_parts(true, true, 17900, Some(wildcard), "0.0.0.0"),
                   Some("127.0.0.1:17900".parse().unwrap()));
        // 绑在具体 LAN IP 上的监听不接回环，必须沿用该 IP。
        assert_eq!(dial_target_parts(true, true, 17900, Some(lan), "0.0.0.0"),
                   Some(lan));
        assert_eq!(dial_target_parts(true, true, 17900, None, "192.168.1.210"),
                   Some("192.168.1.210:17900".parse().unwrap()));
    }

    /// v2.4.92 的关键证据：**生产订阅循环**（`serve` 本体，不是测试里的复制品）收到
    /// 数据面 OPEN 后，真的拨本地引擎监听口、真的回 OPENED、字节真的双向穿过。
    ///
    /// 断言的是「下载方写进去的字节原样回来」——只断言「OPENED 收到了」是不够的，
    /// 那也可能是一条接错上游的隧道。
    #[test]
    fn tcp_subscription_splices_open_to_local_engine() {
        use cr_nat::relay::RelayClient;
        use cr_nat::relay_server::RelayServer;
        use cr_nat::relay_tunnel::{OPEN_TIMEOUT, RelayTunnelSet};
        use std::io::{Read, Write};
        use std::net::TcpListener;

        // 本地「引擎监听口」：echo。
        let engine = TcpListener::bind("127.0.0.1:0").unwrap();
        let engine_addr = engine.local_addr().unwrap();
        let echo = thread::spawn(move || {
            let (mut s, _) = engine.accept().unwrap();
            let mut buf = [0u8; 8192];
            loop {
                match s.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => { if s.write_all(&buf[..n]).is_err() { break } }
                    Err(_) => break,
                }
            }
        });

        let h = RelayServer::bind("127.0.0.1:0").unwrap();
        let addr = h.local_addr().unwrap().to_string();
        let handle = h.spawn();

        let holder_id = node_id_16("holder-splice");
        {
            // 生产路径：真 `serve`，dial=Some(本地引擎)。
            let a = addr.clone();
            let id = holder_id;
            thread::spawn(move || {
                let _ = serve(&a, &id, "127.0.0.1", 1, Some(engine_addr));
            });
        }
        // 等订阅注册上（不靠猜 sleep 时长）。
        let mut ok = false;
        for _ in 0..60 {
            if handle.stats().registered >= 1 { ok = true; break; }
            thread::sleep(Duration::from_millis(50));
        }
        assert!(ok, "订阅未注册: {:?}", handle.stats());

        // 「下载方」：派生会话 id 注册（与守护进程数据面同一口径），OPEN 后写读。
        let dl_base = node_id_16("downloader-splice");
        let dp = cr_nat::relay_tunnel::derive_session_id(&dl_base, b"cr-dp-v1");
        let mut c = RelayClient::new(addr).connect(CONNECT_TIMEOUT).unwrap();
        c.register(&dp).unwrap();
        let set = RelayTunnelSet::new(c, holder_id).unwrap();
        let mut t = set.open(0x7a7a).unwrap();
        t.send_open(&dp).unwrap();
        t.wait_opened(OPEN_TIMEOUT).expect("持有者必须回 OPENED（本地已拨通）");

        let payload: Vec<u8> = (0..32 * 1024).map(|i| (i % 233) as u8).collect();
        t.write_all(&payload).unwrap();
        let mut got = vec![0u8; payload.len()];
        t.read_exact(&mut got).unwrap();
        assert_eq!(got, payload, "经订阅连接 splice 的字节必须逐字节一致");

        // 计数必须真的涨了：没涨说明走的是别的路径（或干脆没走数据面）。
        assert!(HOLDER.opens.load(Ordering::Relaxed) >= 1, "OPEN 未被计入");
        assert!(HOLDER.accepted.load(Ordering::Relaxed) >= 1, "未完成 splice");
        assert!(HOLDER.bytes_to_engine.load(Ordering::Relaxed) + 
                HOLDER.bytes_from_engine.load(Ordering::Relaxed) >= payload.len() as u64,
                "字节计数与实传不符: {:?}",
                HOLDER.bytes_to_engine.load(Ordering::Relaxed));

        drop(t);
        let _ = echo.join();
        handle.stop();
    }

    /// v2.4.86 关键回归：**UDP 订阅会因空闲被中继回收**。
    ///
    /// 这个测试把中继的空闲超时压到 1.5 s、保活压到 300 ms，跑 4 s（≈ 3 个超时周期），
    /// 然后从请求侧真的发一次唤醒。断言三件事：
    ///   1. `sessions_expired == 0`（保活真的在续期）；
    ///   2. 唤醒仍然能送到本机「守护进程」（独立 UDP socket），并拿回真实应答；
    ///   3. 应答内容就是守护进程发的，不是订阅线程编的。
    /// 去掉保活这一步，第 1、2 条都会失败——这正是「进程还活着但谁都叫不醒」的机理。
    #[test]
    fn udp_subscription_survives_idle_reaping_and_still_delivers() {
        use cr_nat::holepunch::relay_wake_udp_with_psk;
        use cr_nat::relay_udp::{UdpLimits, UdpRelayServer};

        // 「守护进程」：真实 UDP socket，收到什么就回一个固定 ACK。
        let daemon = UdpSocket::bind("127.0.0.1:0").unwrap();
        daemon.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let daemon_port = daemon.local_addr().unwrap().port();
        let ack = br#"{"type":"WAKEUP_ACK","state":"hot"}"#.to_vec();
        let ack_for_daemon = ack.clone();
        let dh = thread::spawn(move || -> usize {
            let mut buf = [0u8; 4096];
            let mut n_ans = 0usize;
            while n_ans == 0 {
                match daemon.recv_from(&mut buf) {
                    Ok((n, from)) => {
                        assert!(n > 0, "订阅线程不应投递空帧");
                        daemon.send_to(&ack_for_daemon, from).unwrap();
                        n_ans += 1;
                    }
                    Err(_) => break,
                }
            }
            n_ans
        });

        let limits = UdpLimits { idle_timeout: Duration::from_millis(1500),
                                 ..Default::default() };
        let h = UdpRelayServer::bind("127.0.0.1:0").unwrap()
            .with_limits(limits).spawn();
        let addr = h.local_addr().to_string();

        let holder = node_id_16("holder-idle");
        let sub = {
            let addr = addr.clone();
            thread::spawn(move || serve_udp(&addr, &holder, "127.0.0.1", daemon_port,
                                           Duration::from_millis(300)))
        };
        // 等订阅真的注册上（而不是靠猜一个 sleep 时长）。
        let mut ok = false;
        for _ in 0..40 {
            if h.stats().sessions_active >= 1 { ok = true; break; }
            thread::sleep(Duration::from_millis(50));
        }
        assert!(ok, "订阅未注册成功: {:?}", h.stats());

        // 跨过 3 个空闲周期：没有保活就一定会被回收。
        thread::sleep(Duration::from_secs(4));
        let s = h.stats();
        assert_eq!(s.sessions_expired, 0, "保活失效，会话被回收了: {:?}", s);
        assert!(s.sessions_active >= 1, "会话已不在: {:?}", s);

        let got = relay_wake_udp_with_psk(&addr, &node_id_16("req-idle"), &holder,
                                          br#"{"type":"WAKEUP_REQ"}"#,
                                          Duration::from_millis(1500), None)
            .expect("空闲 4 s 后仍应能唤醒");
        assert_eq!(got, ack, "应答必须来自守护进程本身");
        assert_eq!(dh.join().unwrap(), 1, "守护进程应恰好被投递一次");
        // 订阅线程会一直跑（生产里也是死循环），这里不 join，随测试进程结束。
        drop(sub);
        h.stop();
    }

    /// 全列表不可达时必须如实返回聚合错误，不能谎报成功；且起始索引要轮转。
    #[test]
    fn all_relays_down_reports_every_endpoint() {
        let relays = vec![
            // 保留端口 1/2 无人监听，connect 必然失败（不改配置、不发业务帧）
            "127.0.0.1:1".to_string(),
            "127.0.0.1:2".to_string(),
        ];
        let id = node_id_16("unit-test");
        let (addr, res) = serve_from(&relays, 0, &id, "127.0.0.1", 19999, None);
        assert_eq!(addr, "127.0.0.1:1", "返回的应是本次起始端点");
        let err = res.expect_err("全不可达必须返回错误");
        assert!(err.contains("127.0.0.1:1") && err.contains("127.0.0.1:2"),
                "聚合错误要逐条列出真实端点: {}", err);

        let (addr2, res2) = serve_from(&relays, 1, &id, "127.0.0.1", 19999, None);
        assert_eq!(addr2, "127.0.0.1:2", "起始索引轮转后先试第二个");
        assert!(res2.is_err());
    }
}
