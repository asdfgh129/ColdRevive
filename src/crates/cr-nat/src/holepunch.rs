//! M2/W3.3 — UDP 打洞与复活请求发送（真实 socket，**不合成任何响应**）。
//!
//! 关键正确性点：打洞帧与随后的 `WAKEUP_REQ` **必须复用同一个 UDP socket**。
//! 否则 cone NAT 下打洞打开的是端口 P1 的映射，而唤醒从 P2 发出，对端 NAT 会丢弃。
//!
//! 控制面新增 `NAT_PUNCH` / `NAT_PUNCH_ACK` 帧；**不属于 PROTO v1 golden**，
//! `PROTOCOL_VERSION` 与 T0 报文契约保持不变（ADR / 契约纪律）。

use crate::endpoint::Endpoint;
use crate::relay::{FrameKind, RelayClient, NODE_ID_LEN};
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
pub fn wake_via_endpoint(
    bind: &str,
    endpoint: &Endpoint,
    node: &str,
    token: &str,
    wake: &[u8],
    punch_frames: usize,
    timeout: Duration,
) -> WakeAttempt {
    let mut out = WakeAttempt {
        endpoint: endpoint.clone(),
        punch_sent: 0,
        punch_acked: false,
        reply: None,
        rtt_ms: 0,
        error: None,
    };
    let socket = match UdpSocket::bind(format!("{}:0", bind)) {
        Ok(s) => s,
        Err(e) => { out.error = Some(format!("UDP bind 失败: {}", e)); return out; }
    };
    let target = match format!("{}:{}", endpoint.ip, endpoint.port)
        .parse::<std::net::SocketAddr>() {
        Ok(a) => a,
        Err(e) => { out.error = Some(format!("非法候选地址: {}", e)); return out; }
    };
    let punch_timeout = timeout.min(Duration::from_millis(250)).max(Duration::from_millis(20));
    let req = punch_request(node, token);
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
                // 迟到的打洞 ACK 不是业务应答，继续等真实唤醒应答。
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
    conn.register(self_id)?;
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
}
