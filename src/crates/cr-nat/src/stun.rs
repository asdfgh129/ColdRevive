//! STUN 客户端（RFC 5389），std-only 实现。
//!
//! 用于：① 探测 NAT 映射地址以判定 NAT 类型；② 为 UDP 打洞提供「预测外部地址」。
//! 仅实现 Binding 方法 + XOR-MAPPED-ADDRESS / MAPPED-ADDRESS / OTHER-ADDRESS 解析，
//! 足以支撑 ColdRevive 的穿透决策；不实现认证/指纹等扩展。

use std::net::{Ipv4Addr, UdpSocket};
use std::time::Duration;

pub const MAGIC: u32 = 0x2112_A442;
pub const BINDING_REQUEST: u16 = 0x0001;
pub const BINDING_RESPONSE: u16 = 0x0101;
pub const ATTR_XOR_MAPPED: u16 = 0x0020;
pub const ATTR_MAPPED: u16 = 0x0001;
pub const ATTR_OTHER: u16 = 0x802C;

#[derive(Debug, Clone)]
pub struct StunResult {
    pub server: String,
    pub mapped: Option<String>,
    pub other: Option<String>,
    pub rtt_ms: f64,
    pub ok: bool,
    pub error: String,
}

/// 在 `sock` 上向 `host:port` 发一个 Binding 请求（使用 `tid`），并等待匹配响应。
/// 复用同一 socket 才能比较不同目的地址下的映射端口（对称性判据）。
pub fn probe_one(
    sock: &UdpSocket,
    host: &str,
    port: u16,
    tid: &[u8; 12],
    timeout: Duration,
) -> StunResult {
    let mut buf = [0u8; 2048];
    probe_one_with(sock, host, port, tid, timeout, |remain| {
        let _ = sock.set_read_timeout(Some(remain));
        match sock.recv_from(&mut buf) {
            Ok((n, _)) => Some(buf[..n].to_vec()),
            Err(_) => None,
        }
    })
}

/// `probe_one` 的可插拔接收版本。
///
/// M2 唤醒接入要求 STUN 探测与唤醒复用**同一个监听 socket**（否则 cone NAT 下
/// 探测得到的外部端口与实际唤醒端口不一致）。守护进程的 UDP 分发线程把 STUN
/// 响应经 channel 交给本函数的 `recv` 闭包，业务帧仍走原有 JSON 分发路径。
pub fn probe_one_with<F>(
    sock: &UdpSocket,
    host: &str,
    port: u16,
    tid: &[u8; 12],
    timeout: Duration,
    mut recv: F,
) -> StunResult
where
    F: FnMut(Duration) -> Option<Vec<u8>>,
{
    let server = format!("{}:{}", host, port);
    let mut row = StunResult {
        server: server.clone(),
        mapped: None,
        other: None,
        rtt_ms: 0.0,
        ok: false,
        error: String::new(),
    };
    let addr = match std::net::ToSocketAddrs::to_socket_addrs(&server) {
        Ok(mut a) => match a.next() {
            Some(a) => a,
            None => {
                row.error = "dns: 无解析结果".into();
                return row;
            }
        },
        Err(e) => {
            row.error = format!("dns: {}", e);
            return row;
        }
    };
    let req = binding_request(tid);
    let t0 = std::time::Instant::now();
    if sock.send_to(&req, addr).is_err() {
        row.error = "sendto 失败".into();
        return row;
    }
    let deadline = t0 + timeout;
    loop {
        let remain = deadline.saturating_duration_since(std::time::Instant::now());
        if remain.is_zero() {
            row.error = "timeout".into();
            row.rtt_ms = timeout.as_secs_f64() * 1000.0;
            return row;
        }
        match recv(remain) {
            Some(data) => {
                if let Some(m) = parse_response(&data, tid) {
                    row.mapped = m.mapped;
                    row.other = m.other;
                    row.rtt_ms = t0.elapsed().as_secs_f64() * 1000.0;
                    row.ok = row.mapped.is_some();
                    if !row.ok {
                        row.error = "无 XOR-MAPPED/MAPPED 地址".into();
                    }
                    return row;
                }
            }
            None => {
                row.error = "timeout".into();
                row.rtt_ms = t0.elapsed().as_secs_f64() * 1000.0;
                return row;
            }
        }
    }
}

pub fn binding_request(tid: &[u8; 12]) -> Vec<u8> {
    let mut v = Vec::with_capacity(20);
    v.extend_from_slice(&BINDING_REQUEST.to_be_bytes());
    v.extend_from_slice(&0u16.to_be_bytes());
    v.extend_from_slice(&MAGIC.to_be_bytes());
    v.extend_from_slice(tid);
    v
}

/// 封装一个 STUN 属性：TYPE(2) + LENGTH(2) + VALUE。
pub fn attr(atype: u16, value: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(4 + value.len());
    v.extend_from_slice(&atype.to_be_bytes());
    v.extend_from_slice(&(value.len() as u16).to_be_bytes());
    v.extend_from_slice(value);
    v
}

pub struct Parsed {
    pub mapped: Option<String>,
    pub other: Option<String>,
}

pub fn parse_response(data: &[u8], tid: &[u8; 12]) -> Option<Parsed> {
    if data.len() < 20 {
        return None;
    }
    let mtype = u16::from_be_bytes([data[0], data[1]]);
    let magic = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    if mtype != BINDING_RESPONSE || magic != MAGIC || &data[8..20] != tid {
        return None;
    }
    let mut pos = 20usize;
    let mut xor = None;
    let mut plain = None;
    let mut other = None;
    while pos + 4 <= data.len() {
        let atype = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let alen = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;
        if pos + alen > data.len() {
            break;
        }
        let val = &data[pos..pos + alen];
        match atype {
            ATTR_XOR_MAPPED => xor = decode_addr(val, true),
            ATTR_MAPPED => plain = decode_addr(val, false),
            ATTR_OTHER => other = decode_addr(val, false),
            _ => {}
        }
        pos += alen + (4 - alen % 4) % 4;
    }
    Some(Parsed {
        mapped: xor.or(plain),
        other,
    })
}

/// 解析 STUN 地址属性（仅 IPv4）。`xor=true` 时对端口与地址做 magic cookie 异或还原。
fn decode_addr(v: &[u8], xor: bool) -> Option<String> {
    if v.len() < 8 || v[1] != 0x01 {
        return None; // 仅支持 IPv4（family=0x01）
    }
    let port = u16::from_be_bytes([v[2], v[3]]);
    let raw = u32::from_be_bytes([v[4], v[5], v[6], v[7]]);
    let (port, raw) = if xor {
        (port ^ (MAGIC >> 16) as u16, raw ^ MAGIC)
    } else {
        (port, raw)
    };
    Some(format!("{}:{}", Ipv4Addr::from(raw), port))
}

/// 一个 STUN 探测会话：复用同一本地 UDP socket 访问多个服务器。
pub struct StunSession {
    pub servers: Vec<(String, u16)>,
}

impl StunSession {
    pub fn new(servers: Vec<(String, u16)>) -> Self {
        Self { servers }
    }

    /// 返回每个服务器的探测结果。所有请求复用 `sock`，保证映射端口可比。
    pub fn probe(&self, sock: &UdpSocket, timeout: Duration) -> Vec<StunResult> {
        let mut buf = [0u8; 2048];
        self.probe_with(sock, timeout, |remain| {
            let _ = sock.set_read_timeout(Some(remain));
            match sock.recv_from(&mut buf) {
                Ok((n, _)) => Some(buf[..n].to_vec()),
                Err(_) => None,
            }
        })
    }

    /// 使用共享监听 socket 发包、由外部 channel 供包的探测（M2 唤醒接入）。
    pub fn probe_with<F>(&self, sock: &UdpSocket, timeout: Duration, mut recv: F) -> Vec<StunResult>
    where
        F: FnMut(Duration) -> Option<Vec<u8>>,
    {
        let mut out = Vec::new();
        for (host, port) in &self.servers {
            let tid: [u8; 12] = rand_tid();
            out.push(probe_one_with(sock, host, *port, &tid, timeout, &mut recv));
        }
        out
    }
}

pub fn rand_tid() -> [u8; 12] {
    let mut tid = [0u8; 12];
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    for i in 0..12 {
        tid[i] = ((seed >> (i * 5)) as u8) ^ (i.wrapping_mul(37) as u8) ^ 0xA5;
    }
    tid
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::UdpSocket;
    use std::thread;

    fn v4_as_be(ip: Ipv4Addr) -> u32 {
        u32::from_be_bytes(ip.octets())
    }

    /// 构造 XOR-MAPPED-ADDRESS 属性（含 TYPE+LENGTH 头）。
    fn build_xor_mapped(peer: std::net::SocketAddr) -> Vec<u8> {
        let ip = match peer.ip() {
            std::net::IpAddr::V4(v4) => v4,
            _ => Ipv4Addr::new(127, 0, 0, 1),
        };
        let raw = v4_as_be(ip);
        let port = peer.port();
        let xport = port ^ (MAGIC >> 16) as u16;
        let xraw = raw ^ MAGIC;
        let mut value = vec![0u8; 8];
        value[1] = 0x01;
        value[2..4].copy_from_slice(&xport.to_be_bytes());
        value[4..8].copy_from_slice(&xraw.to_be_bytes());
        attr(ATTR_XOR_MAPPED, &value)
    }

    /// 构造 MAPPED-ADDRESS 属性（含 TYPE+LENGTH 头）。
    fn build_plain_mapped(ip: Ipv4Addr, port: u16) -> Vec<u8> {
        let raw = v4_as_be(ip);
        let mut value = vec![0u8; 8];
        value[1] = 0x01;
        value[2..4].copy_from_slice(&port.to_be_bytes());
        value[4..8].copy_from_slice(&raw.to_be_bytes());
        attr(ATTR_MAPPED, &value)
    }

    /// 把属性字节拼成完整 STUN 响应（含消息长度）。
    fn build_response(tid: &[u8; 12], attributes: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&BINDING_RESPONSE.to_be_bytes());
        data.extend_from_slice(&(attributes.len() as u16).to_be_bytes());
        data.extend_from_slice(&MAGIC.to_be_bytes());
        data.extend_from_slice(tid);
        data.extend_from_slice(attributes);
        data
    }

    #[test]
    fn parse_xor_mapped_roundtrip() {
        let ip = Ipv4Addr::new(1, 2, 3, 4);
        let attr_bytes = build_xor_mapped(std::net::SocketAddr::new(
            std::net::IpAddr::V4(ip),
            5678,
        ));
        let tid = [9u8; 12];
        let data = build_response(&tid, &attr_bytes);
        let p = parse_response(&data, &tid).unwrap();
        assert_eq!(p.mapped.as_deref(), Some("1.2.3.4:5678"));
    }

    #[test]
    fn probe_against_mock_server() {
        let srv = UdpSocket::bind("127.0.0.1:0").unwrap();
        let srv_port = srv.local_addr().unwrap().port();
        thread::spawn(move || {
            let mut buf = [0u8; 2048];
            if let Ok((n, peer)) = srv.recv_from(&mut buf) {
                if n >= 20 {
                    let attr_bytes = build_xor_mapped(peer);
                    let resp = build_response(&buf[8..20].try_into().unwrap(), &attr_bytes);
                    let _ = srv.send_to(&resp, peer);
                }
            }
        });
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        let session = StunSession::new(vec![("127.0.0.1".into(), srv_port)]);
        let results = session.probe(&client, Duration::from_secs(2));
        assert_eq!(results.len(), 1);
        assert!(results[0].ok, "mock STUN 应成功: {:?}", results[0].error);
        assert!(results[0].mapped.as_ref().unwrap().starts_with("127.0.0.1:"));
    }

    #[test]
    fn decode_plain_mapped_address() {
        let attr_bytes = build_plain_mapped(Ipv4Addr::new(9, 9, 9, 9), 1234);
        let tid = [1u8; 12];
        let data = build_response(&tid, &attr_bytes);
        let p = parse_response(&data, &tid).unwrap();
        assert_eq!(p.mapped.as_deref(), Some("9.9.9.9:1234"));
    }

    #[test]
    fn mismatched_tid_is_rejected() {
        let mut data = Vec::new();
        data.extend_from_slice(&BINDING_RESPONSE.to_be_bytes());
        data.extend_from_slice(&0u16.to_be_bytes());
        data.extend_from_slice(&MAGIC.to_be_bytes());
        data.extend_from_slice(&[0u8; 12]); // wrong tid
        assert!(parse_response(&data, &[1u8; 12]).is_none());
    }
}
