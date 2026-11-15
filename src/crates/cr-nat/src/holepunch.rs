//! M2/W3.3 — UDP 打洞与复活请求发送（真实 socket，**不合成任何响应**）。
//!
//! 关键正确性点：打洞帧与随后的 `WAKEUP_REQ` **必须复用同一个 UDP socket**。
//! 否则 cone NAT 下打洞打开的是端口 P1 的映射，而唤醒从 P2 发出，对端 NAT 会丢弃。
//!
//! 控制面新增 `NAT_PUNCH` / `NAT_PUNCH_ACK` 帧；**不属于 PROTO v1 golden**，
//! `PROTOCOL_VERSION` 与 T0 报文契约保持不变（ADR / 契约纪律）。

use crate::endpoint::Endpoint;
use crate::relay::{ephemeral_session_id, FrameKind, RelayClient, NODE_ID_LEN};
use std::net::UdpSocket;
use std::time::{Duration, Instant};

pub const PUNCH_TYPE: &str = "NAT_PUNCH";
pub const PUNCH_ACK_TYPE: &str = "NAT_PUNCH_ACK";

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

/// 打洞帧（JSON，独立于 PROTO v1）：`{"t":"NAT_PUNCH","node":"A","token":"..."}`
pub fn punch_request(node: &str, token: &str) -> Vec<u8> {
    format!(r#"{{"t":"{}","node":"{}","token":"{}"}}"#,
            PUNCH_TYPE, esc(node), esc(token)).into_bytes()
}

/// 打洞应答帧，由持有者从**监听 socket** 回发，用来打开/维持其 NAT 映射。
pub fn punch_ack(node: &str, token: &str) -> Vec<u8> {
    format!(r#"{{"t":"{}","node":"{}","token":"{}"}}"#,
            PUNCH_ACK_TYPE, esc(node), esc(token)).into_bytes()
}

/// 单帧最多携带的回打候选数。
///
/// 这是**防放大**上限：一帧入向最多引发 `MAX_BACK_TARGETS * PUNCH_BACK_FRAMES`
/// 个出向小包。取值刻意保守 —— 该帧可以被互联网上任意主机发送，不设上限就等于
/// 把持有者的唤醒端口变成反射器。
pub const MAX_BACK_TARGETS: usize = 4;

/// 每个回打候选重复发几帧（UDP 会丢，2 帧足够且不放大）。
pub const PUNCH_BACK_FRAMES: usize = 2;

/// W3.6 双向打洞：`NAT_PUNCH` 帧可选携带请求侧的**自身映射候选** `back`。
///
/// 单向打洞（W3.3）只能穿全锥持有者：请求侧的首个入向包在受限锥及以上的网关上
/// 没有 conntrack 条目、过滤集里也没有对方，直接被丢弃（W3.5② 实验室实测
/// `direct` 0/30）。让持有者先按 `back` 主动回打一包，才能在双方网关上各自开出
/// 过滤项，随后请求侧的包才进得去。
pub fn punch_request_with_back(node: &str, token: &str, back: &[String]) -> Vec<u8> {
    if back.is_empty() {
        return punch_request(node, token);
    }
    let list = back.iter().take(MAX_BACK_TARGETS)
        .map(|s| esc(s))
        .collect::<Vec<_>>()
        .join(",");
    format!(r#"{{"t":"{}","node":"{}","token":"{}","back":"{}"}}"#,
            PUNCH_TYPE, esc(node), esc(token), list).into_bytes()
}

/// 取出并校验帧里的回打候选。非法地址一律丢弃，**不做任何"猜一个"的兜底**。
pub fn punch_back(data: &[u8]) -> Vec<String> {
    let text = match std::str::from_utf8(data) { Ok(t) => t, Err(_) => return Vec::new() };
    let marker = format!(r#""t":"{}""#, PUNCH_TYPE);
    if !text.contains(&marker) { return Vec::new(); }
    let key = r#""back":""#;
    let Some(pos) = text.find(key) else { return Vec::new() };
    let rest = &text[pos + key.len()..];
    let Some(end) = rest.find('"') else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    for part in rest[..end].split(',') {
        let p = part.trim();
        if p.is_empty() || p.contains('\\') || !is_punchable(p) { continue; }
        if out.iter().any(|x| x == p) { continue; }
        out.push(p.to_string());
        if out.len() >= MAX_BACK_TARGETS { break; }
    }
    out
}

/// 只接受可路由的 IPv4 主机地址。环回 / 未指定 / 组播 / 广播 / 文档地址一律拒绝，
/// 避免把持有者的唤醒 socket 用来探测本机或做无意义广播。
fn is_punchable(s: &str) -> bool {
    match s.parse::<std::net::SocketAddr>() {
        Ok(a) => match a.ip() {
            std::net::IpAddr::V4(v4) => !(v4.is_loopback() || v4.is_unspecified()
                || v4.is_multicast() || v4.is_broadcast() || v4.is_documentation()),
            // 打洞候选目前只支持 IPv4（STUN 侧同样只解析 IPv4）
            std::net::IpAddr::V6(_) => false,
        },
        Err(_) => false,
    }
}

/// 持有者侧：从**监听 socket** 向 `targets` 各发若干打洞帧，主动打开自身 NAT 过滤项。
///
/// 必须用监听 socket —— 只有从该端口出向，请求侧回打才能落到唤醒端口上。
/// 返回真实 `sendto` 成功的帧数（失败不伪造）。
pub fn punch_back_to(sock: &UdpSocket, targets: &[String], node: &str, token: &str,
                     frames: usize) -> usize {
    // 回打帧不再携带 back：否则可形成链式放大。
    let frame = punch_request(node, token);
    let mut sent = 0usize;
    for t in targets.iter().take(MAX_BACK_TARGETS) {
        let Ok(addr) = t.parse::<std::net::SocketAddr>() else { continue };
        for _ in 0..frames.min(PUNCH_BACK_FRAMES) {
            if sock.send_to(&frame, addr).is_ok() { sent += 1; }
        }
    }
    sent
}

/// 回打限流（令牌桶）。
///
/// `NAT_PUNCH` 是无需鉴权的 UDP 帧，任何主机都能发。若对每帧都无条件回打，
/// 持有者就成了放大器。但限流不能紧到影响正常业务：一次合法唤醒恰好消耗 1 个令牌，
/// 桶容量与补充速率必须明显高于正常唤醒频率。
///
/// 参数取 **容量 64 / 补充 16 每秒**：
/// - 最坏出向 = 16 次/秒 × `MAX_BACK_TARGETS(4)` × `PUNCH_BACK_FRAMES(2)` = 128 个小帧/秒，
///   放大倍数仍被硬性封死；
/// - 正常侧留出 16 次并发唤醒/秒的余量。
///
/// 早期版本用「容量 20 / 2 每秒」，在连续试验里会稳定打空 —— 实测表现为**每 3 次
/// 唤醒就有 1 次跳过回打**（实验室里 direct 成功率因此固定掉到 23/30）。限流器本身
/// 不能成为业务失败的隐蔽原因，故下调到这组参数并加 `denials` 计数以便观测。
pub struct BackRate {
    tokens: f64,
    last: std::time::Instant,
    denials: u64,
}

impl BackRate {
    pub fn new() -> Self {
        Self { tokens: 64.0, last: std::time::Instant::now(), denials: 0 }
    }

    /// 取一个令牌；无令牌时返回 false（调用方应跳过回打，并计一次拒绝）。
    pub fn allow(&mut self) -> bool {
        let now = std::time::Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = refill(self.tokens, dt, 16.0, 64.0);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            self.denials += 1;
            false
        }
    }

    /// 累计被限流拒绝的次数（0 表示限流从未影响业务）。
    pub fn denials(&self) -> u64 { self.denials }
}

/// 令牌补充（纯函数，便于用模拟时间做回归测试，避免测试里 sleep 几十秒）。
fn refill(tokens: f64, dt: f64, rate: f64, cap: f64) -> f64 {
    (tokens + dt.max(0.0) * rate).min(cap)
}

impl Default for BackRate {
    fn default() -> Self { Self::new() }
}

/// 从原始 UDP 报文中取出打洞帧的 token（只做最小字段匹配，不做业务校验）。
pub fn punch_token(data: &[u8], want_type: &str) -> Option<String> {
    let text = std::str::from_utf8(data).ok()?;
    let marker = format!(r#""t":"{}""#, want_type);
    if !text.contains(&marker) { return None; }
    let key = r#""token":""#;
    let start = text.find(key)? + key.len();
    let rest = &text[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

#[derive(Debug, Clone)]
pub struct WakeAttempt {
    pub endpoint: Endpoint,
    pub punch_sent: usize,
    pub punch_acked: bool,
    /// W3.6：打洞阶段真实收到了对端的 `NAT_PUNCH`（对端已按 `back` 回打到我方）。
    /// 与 `punch_acked` 分开记：仅收到对端回打但收不到 ACK，说明过滤方向不对称，
    /// 是排查「为什么 direct 仍失败」的关键证据，不能合并成一个布尔。
    pub peer_punched: bool,
    pub reply: Option<Vec<u8>>,
    pub rtt_ms: u64,
    pub error: Option<String>,
}

impl WakeAttempt {
    pub fn ok(&self) -> bool { self.reply.is_some() && self.error.is_none() }
}

/// 从一个复用 socket 向某候选端点打洞并发送唤醒帧。
///
/// - `punch_frames` 个打洞帧先发出；任何一帧收到带同 token 的 `NAT_PUNCH_ACK`
///   即认为映射已建立（**必须是真实收到的 UDP 包**）。
/// - 随后**同一个 socket** 发送 `wake` 原始字节并等待真实应答。
/// - 任何失败都返回 `error`，不回填占位响应。
///
/// W3.6：该入口每次调用都新绑一个 `:0` socket，故无法预知自身映射端口，
/// 只用于「不需要回打候选」的场景（测试、单向打洞）。生产唤醒路径改用
/// [`wake_via_socket`]，在调用方已绑定并做过 STUN 的 socket 上复用。
pub fn wake_via_endpoint(
    bind: &str,
    endpoint: &Endpoint,
    node: &str,
    token: &str,
    wake: &[u8],
    punch_frames: usize,
    timeout: Duration,
) -> WakeAttempt {
    let socket = match UdpSocket::bind(format!("{}:0", bind)) {
        Ok(s) => s,
        Err(e) => {
            return WakeAttempt {
                endpoint: endpoint.clone(),
                punch_sent: 0,
                punch_acked: false,
                peer_punched: false,
                reply: None,
                rtt_ms: 0,
                error: Some(format!("UDP bind 失败: {}", e)),
            }
        }
    };
    wake_via_socket(&socket, endpoint, node, token, &[], wake, punch_frames, timeout)
}

/// 在调用方**已绑定**的 socket 上打洞并发送唤醒帧（W3.6）。
///
/// `back` 是本侧自身映射候选，随打洞帧一起发出，让对端先回打以打开其 NAT 过滤项。
/// 之所以要求外部传入 socket：本侧映射端口必须与随后打洞 / 唤醒用的端口一致，
/// 否则携带的 `back` 就是错地址（cone NAT 下直接导致对端回打到无人监听的口）。
pub fn wake_via_socket(
    socket: &UdpSocket,
    endpoint: &Endpoint,
    node: &str,
    token: &str,
    back: &[String],
    wake: &[u8],
    punch_frames: usize,
    timeout: Duration,
) -> WakeAttempt {
    let mut out = WakeAttempt {
        endpoint: endpoint.clone(),
        punch_sent: 0,
        punch_acked: false,
        peer_punched: false,
        reply: None,
        rtt_ms: 0,
        error: None,
    };
    let target = match format!("{}:{}", endpoint.ip, endpoint.port)
        .parse::<std::net::SocketAddr>() {
        Ok(a) => a,
        Err(e) => { out.error = Some(format!("非法候选地址: {}", e)); return out; }
    };
    let punch_timeout = timeout.min(Duration::from_millis(250)).max(Duration::from_millis(20));
    let req = punch_request_with_back(node, token, back);
    let mut buf = [0u8; 65535];
    for _ in 0..punch_frames.min(8) {
        if let Err(e) = socket.send_to(&req, target) {
            out.error = Some(format!("打洞 sendto 失败: {}", e));
            return out;
        }
        out.punch_sent += 1;
        let _ = socket.set_read_timeout(Some(punch_timeout));
        if let Ok((n, _)) = socket.recv_from(&mut buf) {
            if punch_token(&buf[..n], PUNCH_ACK_TYPE).as_deref() == Some(token) {
                out.punch_acked = true;
                break;
            }
            // 对端按 back 回打过来的首个包：其过滤项已打开，直接进入唤醒阶段。
            if punch_token(&buf[..n], PUNCH_TYPE).is_some() {
                out.peer_punched = true;
                break;
            }
        }
    }

    let t0 = Instant::now();
    if let Err(e) = socket.send_to(wake, target) {
        out.error = Some(format!("唤醒 sendto 失败: {}", e));
        return out;
    }
    let _ = socket.send_to(wake, target);
    let deadline = Instant::now() + timeout;
    loop {
        let remain = deadline.saturating_duration_since(Instant::now());
        if remain.is_zero() {
            out.error = Some("唤醒无应答(超时)".into());
            break;
        }
        let _ = socket.set_read_timeout(Some(remain));
        match socket.recv_from(&mut buf) {
            Ok((n, _)) => {
                // 迟到的打洞 ACK / 对端的回打帧都不是业务应答，继续等真实唤醒应答。
                if punch_token(&buf[..n], PUNCH_ACK_TYPE).is_some()
                    || punch_token(&buf[..n], PUNCH_TYPE).is_some()
                {
                    continue;
                }
                out.rtt_ms = t0.elapsed().as_millis() as u64;
                out.reply = Some(buf[..n].to_vec());
                break;
            }
            Err(e) => {
                out.rtt_ms = t0.elapsed().as_millis() as u64;
                out.error = Some(format!("唤醒无应答: {}", e));
                break;
            }
        }
    }
    out
}

/// 「热身」打洞：只发包、不等应答。
///
/// 目的在于让本侧 NAT 记下「我向该地址发过包」的过滤项 —— 对端随后主动回打
/// 的包才不会被本侧网关当 unsolicited 丢弃。返回值是真实 `sendto` 成功帧数。
pub fn warmup_punch(socket: &UdpSocket, endpoint: &Endpoint, node: &str, token: &str,
                    frames: usize) -> usize {
    let frame = punch_request(node, token);
    let Ok(target) = format!("{}:{}", endpoint.ip, endpoint.port)
        .parse::<std::net::SocketAddr>() else { return 0 };
    let mut sent = 0usize;
    for _ in 0..frames.min(4) {
        if socket.send_to(&frame, target).is_ok() { sent += 1; }
    }
    sent
}

/// 经可信中继转发唤醒帧（TCP 帧协议见 `relay.rs`）。
/// 中继必须真实回 `DELIVERED` 帧；超时 / 未注册 / 帧异常一律 Err。
pub fn relay_wake(
    relay: &str,
    self_id: &[u8; NODE_ID_LEN],
    peer_id: &[u8; NODE_ID_LEN],
    wake: &[u8],
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let client = RelayClient::new(relay.to_string());
    let mut conn = client.connect(timeout)?;
    // W3.4：请求侧用一次性会话 id 注册，避免与本节点自身的「持有者订阅连接」
    // 抢占服务端注册表中同一个 node_id 表项；服务端会把该会话 id 作为
    // DELIVERED.node 交给持有者，持有者据此把真实 WAKEUP_ACK 回投到本连接。
    let session = ephemeral_session_id(self_id);
    conn.register(&session)?;
    conn.send_relay(peer_id, wake)?;
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() >= deadline { return Err("中继等待 DELIVERED 超时".into()); }
        let frame = conn.recv_frame()?;
        match frame.kind {
            FrameKind::Delivered => return Ok(frame.payload),
            FrameKind::Ack => continue,
            other => return Err(format!("中继返回非预期帧: {:?}", other)),
        }
    }
}

/// 按顺序尝试中继列表，返回**真实成功的那一个**地址与应答负载。
///
/// 顺序尝试而非并发：多中继是「单点宕机可切换」，不是任播。全部失败时聚合每一条的真实
/// 错误，便于判断是网络不通、未注册还是中继没同步到对端——不要压成一句「中继失败」。
pub fn relay_wake_any(
    relays: &[String],
    self_id: &[u8; NODE_ID_LEN],
    peer_id: &[u8; NODE_ID_LEN],
    wake: &[u8],
    timeout: Duration,
) -> Result<(String, Vec<u8>), String> {
    if relays.is_empty() {
        return Err("未配置任何中继端点".into());
    }
    let mut errs: Vec<String> = Vec::new();
    for r in relays {
        match relay_wake(r, self_id, peer_id, wake, timeout) {
            Ok(v) => return Ok((r.clone(), v)),
            Err(e) => errs.push(format!("{}: {}", r, e)),
        }
    }
    Err(format!("全部中继失败（{}）", errs.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endpoint::EndpointKind;
    use std::net::UdpSocket;
    use std::thread;

    fn ep(ip: &str, port: u16) -> Endpoint {
        Endpoint {
            kind: EndpointKind::PrivateLan,
            ip: ip.into(),
            port,
            internal_port: port,
            evidence: "unit-test-real-socket".into(),
        }
    }

    /// 真实回环 UDP 对端：收到 NAT_PUNCH 回 ACK，收到其它帧原样回一个最小 JSON 应答。
    fn spawn_peer() -> u16 {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = sock.local_addr().unwrap().port();
        thread::spawn(move || {
            let mut buf = [0u8; 65535];
            for _ in 0..12 {
                let (n, src) = match sock.recv_from(&mut buf) { Ok(v) => v, Err(_) => continue };
                let token = punch_token(&buf[..n], PUNCH_TYPE).unwrap_or_default();
                let out = if !token.is_empty() {
                    punch_ack("A", &token)
                } else {
                    br#"{"type":"WAKEUP_ACK","status":"OK"}"#.to_vec()
                };
                let _ = sock.send_to(&out, src);
            }
        });
        port
    }

    #[test]
    fn punch_and_wake_reuse_one_socket_and_get_real_reply() {
        let port = spawn_peer();
        let a = wake_via_endpoint("127.0.0.1", &ep("127.0.0.1", port), "B", "tok-123",
                                  br#"{"type":"WAKEUP_REQ"}"#, 2, Duration::from_millis(500));
        assert!(a.error.is_none(), "{:?}", a.error);
        assert!(a.ok());
        assert!(a.punch_acked, "应收到真实 PUNCH_ACK");
        assert_eq!(a.punch_sent, 1);
        let reply = String::from_utf8(a.reply.unwrap()).unwrap();
        assert!(reply.contains("WAKEUP_ACK"));
    }

    #[test]
    fn unreachable_candidate_is_reported_as_failure_not_fake_success() {
        // 绑定后立即释放一个端口，作为「大概率无人应答」的真实地址
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = sock.local_addr().unwrap().port();
        drop(sock);
        let a = wake_via_endpoint("127.0.0.1", &ep("127.0.0.1", port), "B", "tok-x",
                                  br#"{"type":"WAKEUP_REQ"}"#, 1, Duration::from_millis(120));
        assert!(!a.ok());
        assert!(a.reply.is_none());
        assert!(a.error.is_some());
    }

    #[test]
    fn punch_frame_token_roundtrip_and_type_isolation() {
        // 打洞 token 由调用方生成为十六进制/字母数字，不携带需转义字符。
        let req = punch_request("A", "tok-abc");
        assert_eq!(punch_token(&req, PUNCH_TYPE).unwrap(), "tok-abc");
        assert_eq!(punch_token(&req, PUNCH_ACK_TYPE), None);
        let ack = punch_ack("A", "tok-abc");
        assert_eq!(punch_token(&ack, PUNCH_ACK_TYPE).unwrap(), "tok-abc");
    }

    // ---- W3.6 双向打洞 ----

    #[test]
    fn frame_with_back_roundtrips_and_stays_parseable_as_punch() {
        let back = vec!["8.8.4.4:40001".to_string(), "1.1.1.1:40002".to_string()];
        let f = punch_request_with_back("B", "tok-1", &back);
        // 仍然是一个合法的 NAT_PUNCH 帧（token 仍能被原有解析器取出）
        assert_eq!(punch_token(&f, PUNCH_TYPE).as_deref(), Some("tok-1"));
        assert_eq!(punch_back(&f), back);
        // 不带 back 的帧解析出空列表，且字节与原实现完全一致（向后兼容）
        assert_eq!(punch_request_with_back("B", "tok-1", &[]), punch_request("B", "tok-1"));
        assert!(punch_back(&punch_request("B", "tok-1")).is_empty());
    }

    #[test]
    fn back_parser_rejects_non_routable_and_malformed_entries() {
        // 只保留可路由 IPv4 主机地址；环回/未指定/组播/广播/文档地址/垃圾串全部丢弃
        let bad = vec![
            "127.0.0.1:1000".into(),        // 环回
            "0.0.0.0:1000".into(),          // 未指定
            "224.0.0.1:1000".into(),        // 组播
            "255.255.255.255:1000".into(),  // 广播
            "203.0.113.5:1000".into(),      // 文档地址段
            "not-an-addr".into(),
            "10.0.0.1".into(),              // 缺端口
            "[::1]:1000".into(),            // IPv6 不支持
        ];
        let f = punch_request_with_back("B", "t", &bad);
        assert!(punch_back(&f).is_empty(), "全部非法项应被丢弃");

        let mixed = vec!["10.0.0.14:40000".into(), "127.0.0.1:1".into(),
                         "192.168.10.3:40001".into()];
        let got = punch_back(&punch_request_with_back("B", "t", &mixed));
        assert_eq!(got, vec!["10.0.0.14:40000".to_string(), "192.168.10.3:40001".to_string()]);
    }

    #[test]
    fn back_target_count_is_capped_and_deduplicated() {
        let many: Vec<String> = (1..=9).map(|i| format!("10.9.0.{}:4000{}", i, i)).collect();
        let f = punch_request_with_back("B", "t", &many);
        assert_eq!(punch_back(&f).len(), MAX_BACK_TARGETS);

        let dup = vec!["10.1.1.1:5000".to_string(), "10.1.1.1:5000".to_string()];
        assert_eq!(punch_back(&punch_request_with_back("B", "t", &dup)).len(), 1);
    }

    #[test]
    fn back_rate_allows_normal_traffic_but_caps_flood() {
        // 正常业务：容量 64 的突发必须全部放行，且不得产生拒绝计数。
        let mut r = BackRate::new();
        let mut allowed = 0;
        for _ in 0..64 { if r.allow() { allowed += 1; } }
        assert!(allowed >= 64, "正常突发被误伤，放行={}", allowed);
        assert_eq!(r.denials(), 0, "正常流量不应触发限流");

        // 洪泛：立即再连发 400 次，绝大多数必须被拒（补充 16/s，短窗口内可忽略）。
        let mut denied = 0;
        for _ in 0..400 { if !r.allow() { denied += 1; } }
        assert!(denied >= 380, "限流未生效，仅拒绝 {}", denied);
        assert!(r.denials() >= 380);
    }

    /// 一次唤醒实际会在打洞阶段连发 2 帧（都带 back），持有者侧就要消耗 2 个令牌。
    /// 这里用模拟时间复刻「每 0.4s 一次唤醒、每次 2 帧」的连续试验序列。
    fn simulate_wakes(cap: f64, rate: f64, interval: f64, steps: usize) -> Vec<usize> {
        let mut tokens = cap;
        let mut denied = Vec::new();
        for step in 1..=steps {
            tokens = refill(tokens, interval, rate, cap);
            let mut miss = 0;
            for _ in 0..2 {
                if tokens >= 1.0 { tokens -= 1.0; } else { miss += 1; }
            }
            if miss > 0 { denied.push(step); }
        }
        denied
    }

    #[test]
    fn back_rate_regression_old_params_starved_steady_wakes() {
        // 回归（实验室实测 bug）：容量 20 / 补充 2 每秒 在连续唤醒下会周期性打空，
        // 表现为 direct 成功率固定掉到 23/30，且失败序号呈严格周期 3
        //（实测 12/15/18/21/24/27/30 与 13/16/19/22/25/28）。
        // 旧参数下的首次打空时点取决于试验间隔（实验室实测约 0.25–0.4s），
        // 故只断言「早期就打空」且「打空后周期性复发」，不硬编码具体序号。
        let old = simulate_wakes(20.0, 2.0, 0.4, 30);
        assert!(!old.is_empty(), "旧参数应复现打空，否则回归用例失效");
        assert!(old[0] <= 20, "旧参数应在前 20 次内打空，实测 {:?}", old);
        let gaps: Vec<usize> = old.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(gaps.iter().all(|g| *g <= 5), "旧参数应在打空后周期性复发: {:?}", old);
    }

    #[test]
    fn back_rate_new_params_never_starve_steady_wakes() {
        // 新参数（64 / 16 每秒）在同样负载、甚至 200 次连续唤醒下都不得打空。
        assert!(simulate_wakes(64.0, 16.0, 0.4, 200).is_empty());
    }

    #[test]
    fn holder_punches_back_from_the_listening_socket() {
        // 真实回环：持有者的「监听 socket」向目标回打，目标必须真的收到帧
        let target = UdpSocket::bind("127.0.0.1:0").unwrap();
        let tport = target.local_addr().unwrap().port();
        let holder = UdpSocket::bind("127.0.0.1:0").unwrap();
        let sent = punch_back_to(&holder, &[format!("127.0.0.1:{}", tport)],
                                 "A", "tok-back", PUNCH_BACK_FRAMES);
        assert_eq!(sent, PUNCH_BACK_FRAMES, "回打帧数应等于真实 sendto 成功数");

        let mut buf = [0u8; 2048];
        let mut seen = 0;
        target.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
        while let Ok((n, _)) = target.recv_from(&mut buf) {
            assert_eq!(punch_token(&buf[..n], PUNCH_TYPE).as_deref(), Some("tok-back"));
            // 回打帧不得再携带 back，否则可链式放大
            assert!(punch_back(&buf[..n]).is_empty());
            seen += 1;
            if seen == PUNCH_BACK_FRAMES { break; }
        }
        assert_eq!(seen, PUNCH_BACK_FRAMES);
    }

    #[test]
    fn wake_via_socket_carries_back_all_the_way_to_the_peer() {
        // 对端：收到打洞帧时解出 back 并回 ACK，随后收到唤醒帧回业务应答
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        let pport = peer.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel::<Vec<String>>();
        thread::spawn(move || {
            let mut buf = [0u8; 65535];
            for _ in 0..6 {
                let (n, src) = match peer.recv_from(&mut buf) { Ok(v) => v, Err(_) => continue };
                let tok = punch_token(&buf[..n], PUNCH_TYPE).unwrap_or_default();
                if !tok.is_empty() {
                    let _ = tx.send(punch_back(&buf[..n]));
                    let _ = peer.send_to(&punch_ack("A", &tok), src);
                } else {
                    let _ = peer.send_to(br#"{"type":"WAKEUP_ACK","status":"OK"}"#, src);
                }
            }
        });

        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let back = vec!["10.0.0.14:40000".to_string()];
        let a = wake_via_socket(&sock, &ep("127.0.0.1", pport), "B", "tok-back2",
                                &back, br#"{"type":"WAKEUP_REQ"}"#, 2,
                                Duration::from_millis(600));
        assert!(a.ok(), "{:?}", a.error);
        assert!(a.punch_acked);
        let got = rx.recv_timeout(Duration::from_millis(500)).unwrap();
        assert_eq!(got, back, "对端应收到真实回打候选");
    }
}
