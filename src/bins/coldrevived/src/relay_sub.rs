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

use cr_core::Config;
use cr_nat::node_id_16;
use cr_nat::reach::{now_ms, ReachDeclaration};
use cr_nat::relay::{FrameKind, RelayClient, NODE_ID_LEN};
use cr_nat::relay_udp::{UdpKind, UdpRelayClient};
use std::net::UdpSocket;
use std::sync::Mutex;
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

    // TCP 承载：一条线程，按列表顺序切换。
    if !relays.is_empty() {
        let label = cfg.node_id.clone();
        let wake_host = wake_host.clone();
        thread::spawn(move || {
            let mut start = 0usize;
            loop {
                let (addr, res) = serve_from(&relays, start, &self_id, &wake_host, wake_port);
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
        thread::spawn(move || {
            let mut start = 0usize;
            loop {
                let addr = &relays_udp[start % relays_udp.len()];
                if let Err(e) = serve_udp(addr, &self_id, &wake_host, wake_port, UDP_KEEPALIVE) {
                    eprintln!("[{}][relay-sub] UDP 订阅中断（{}）: {}，{}s 后切换重试",
                              label, addr, e, RECONNECT_DELAY.as_secs());
                }
                start = (start + 1) % relays_udp.len();
                thread::sleep(RECONNECT_DELAY);
            }
        });
    }
}

/// 从 `start` 开始依次尝试列表里的中继，返回实际使用的地址与结果。
fn serve_from(
    relays: &[String],
    start: usize,
    self_id: &[u8; NODE_ID_LEN],
    wake_host: &str,
    wake_port: u16,
) -> (String, Result<(), String>) {
    let n = relays.len();
    let mut errs: Vec<String> = Vec::new();
    for k in 0..n {
        let addr = &relays[(start + k) % n];
        match serve(addr, self_id, wake_host, wake_port) {
            Ok(()) => return (addr.clone(), Ok(())),
            Err(e) => errs.push(format!("{}: {}", addr, e)),
        }
    }
    (relays[start % n].clone(), Err(format!("全部 {} 个中继均失败（{}）",
                                            n, errs.join("; "))))
}

fn serve(
    relay: &str,
    self_id: &[u8; NODE_ID_LEN],
    wake_host: &str,
    wake_port: u16,
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

    let udp = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("订阅 UDP 绑定失败: {}", e))?;
    udp.set_read_timeout(Some(WAKE_REPLY_TIMEOUT))
        .map_err(|e| e.to_string())?;
    let target = format!("{}:{}", wake_host, wake_port);
    let mut buf = [0u8; 65535];

    loop {
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
        udp.send_to(&frame.payload, &target)
            .map_err(|e| format!("唤醒 UDP 发送失败: {}", e))?;
        let (n, _) = match udp.recv_from(&mut buf) {
            Ok(v) => v,
            // 守护进程拒绝/忽略该帧：不合成应答，请求侧按自身超时如实失败。
            Err(_) => continue,
        };
        conn.send_relay(&source, &buf[..n])?;
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
        let (addr, res) = serve_from(&relays, 0, &id, "127.0.0.1", 19999);
        assert_eq!(addr, "127.0.0.1:1", "返回的应是本次起始端点");
        let err = res.expect_err("全不可达必须返回错误");
        assert!(err.contains("127.0.0.1:1") && err.contains("127.0.0.1:2"),
                "聚合错误要逐条列出真实端点: {}", err);

        let (addr2, res2) = serve_from(&relays, 1, &id, "127.0.0.1", 19999);
        assert_eq!(addr2, "127.0.0.1:2", "起始索引轮转后先试第二个");
        assert!(res2.is_err());
    }
}
