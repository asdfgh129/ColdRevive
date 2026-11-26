//! RFC 5389 Binding 服务端 + RFC 5780 CHANGE-REQUEST 能力（**单公网 IP 变体**）。
//!
//! 存在的理由不是「顺手补一个服务端」，而是现有分类链有一个绕不过去的洞：
//!
//! 公网 STUN（Google / Cloudflare / 各类 3478 反射器）**全部忽略 CHANGE-REQUEST**。
//! 于是客户端看到「change-port 没有回包」时，无法区分两件事：
//!   ① NAT 把这个方向的包丢了 —— 真有端口过滤；
//!   ② 服务器压根没换源口 —— 服务器能力缺失。
//! `stun::filter_behavior` 在模糊情形下返回 `PortRestrictedCone`，等于把「没测到」
//! 写成「测到了」。实测基线就踩在这里：本机出口被判 `port-restricted-cone`，
//! 而这个结论的唯一样本来自忽略 CHANGE-REQUEST 的公共服务器。
//!
//! 只有自己控制回包源地址，过滤行为的结论才有依据。本模块提供：
//!   * BINDING 响应（XOR-MAPPED / MAPPED / SOURCE / RESPONSE-ORIGIN / OTHER-ADDRESS）；
//!   * CHANGE-REQUEST 的 PORT 位：**从另一个套接字回包**，源口因此确实变了；
//!   * CHANGE-REQUEST 的 IP 位：单公网 IP 做不到，返回 **420 + UNKNOWN-ATTRIBUTES(0x0003)**，
//!     让客户端明确读到「服务器不具备该能力」，而不是把它误读成「NAT 拦了」。
//!
//! ## 单 IP 的能力边界（必须写在明处）
//!
//! 只有一块公网 IP 时，能测的只有「端口」这一根轴：
//!   * 能测：入向是否允许**同 IP、异端口** —— 区分端口受限（APDF）与「非端口受限」；
//!   * 不能测：入向是否允许**异 IP** —— 无法区分 full-cone（EIF）与 address-restricted（ADF）。
//! 这两类在打洞决策上同属「可预测」，但**类型名不同**，所以客户端不得把
//! 「change-port 有应答」折成 `full-cone`；对应 `FilterBehavior::PortOpenUntestedAddress`。
//!
//! ## 反面警告（实现里刻意保留的约束）
//!
//! 回包源地址必须**真的是**另一个套接字。若为图省事从主套接字回包再改源地址字段，
//! 客户端会看到 `src == primary`，判据（`source_matches_alt`）把它挡掉并降级为 `Untested`。
//! 这是故意的：宁可不给结论，也不给一个看起来像结论的假象。

use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::stun::{
    attr, change_flags_of, ATTR_CHANGE_REQUEST, ATTR_MAPPED, ATTR_OTHER, ATTR_XOR_MAPPED,
    BINDING_REQUEST, BINDING_RESPONSE, CHANGE_IP, CHANGE_PORT, MAGIC,
};

pub const BINDING_ERROR: u16 = 0x0111;
pub const ATTR_SOURCE: u16 = 0x0004;
pub const ATTR_RESPONSE_ORIGIN: u16 = 0x802B;
pub const ATTR_ERROR_CODE: u16 = 0x0009;
pub const ATTR_UNKNOWN_ATTRIBUTES: u16 = 0x000A;
/// RFC 3489/5780：服务器不认识请求里的属性时回 420。
pub const ERR_UNKNOWN_ATTRIBUTE: u16 = 420;

/// 一次请求的处理结果（供 CLI 打日志 / 计数，不在库里直接 println）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// 普通请求，从收到它的套接字回包。
    AnsweredHere,
    /// CHANGE-PORT 请求，从另一个套接字回包（源口确实变了）。
    AnsweredFromPeerSocket,
    /// CHANGE-IP 请求：单公网 IP 做不到，回 420。
    ChangeIpRejected,
    /// 不是合法 BINDING 请求（长度/类型/magic 不对）。
    Malformed,
}

#[derive(Debug, Default)]
pub struct StunServerStats {
    pub requests: AtomicU64,
    pub answered_here: AtomicU64,
    pub answered_from_peer_socket: AtomicU64,
    pub change_ip_rejected: AtomicU64,
    pub malformed: AtomicU64,
}

/// 地址属性（`xor=true` 为 XOR-MAPPED 的编码方式）。
fn addr_attr(atype: u16, sa: SocketAddr, xor: bool) -> Vec<u8> {
    let (ip, port) = match sa {
        SocketAddr::V4(v4) => (*v4.ip(), v4.port()),
        SocketAddr::V6(v6) => {
            // 本服务端只承诺 IPv4（穿透栈本身也只处理 IPv4）；IPv6 映射成 0.0.0.0 会被
            // 客户端解码成错误地址，所以宁可回一个明确的占位也不编造。
            let _ = v6;
            (Ipv4Addr::UNSPECIFIED, 0)
        }
    };
    let (port, raw) = if xor {
        (port ^ (MAGIC >> 16) as u16, u32::from_be_bytes(ip.octets()) ^ MAGIC)
    } else {
        (port, u32::from_be_bytes(ip.octets()))
    };
    let mut v = vec![0u8; 8];
    v[1] = 0x01; // family = IPv4
    v[2..4].copy_from_slice(&port.to_be_bytes());
    v[4..8].copy_from_slice(&raw.to_be_bytes());
    attr(atype, &v)
}

fn header(mtype: u16, tid: &[u8; 12], attrs: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(20 + attrs.len());
    v.extend_from_slice(&mtype.to_be_bytes());
    v.extend_from_slice(&(attrs.len() as u16).to_be_bytes());
    v.extend_from_slice(&MAGIC.to_be_bytes());
    v.extend_from_slice(tid);
    v.extend_from_slice(attrs);
    v
}

/// 标准 BINDING 响应。`origin` 是**实际发出这个响应**的套接字地址，
/// `other` 是客户端可以用来做 change 测试的另一个地址（本变体：同 IP、异端口）。
pub fn binding_response(
    tid: &[u8; 12],
    peer: SocketAddr,
    origin: SocketAddr,
    other: Option<SocketAddr>,
) -> Vec<u8> {
    let mut attrs = Vec::new();
    attrs.extend(addr_attr(ATTR_XOR_MAPPED, peer, true));
    attrs.extend(addr_attr(ATTR_MAPPED, peer, false));
    attrs.extend(addr_attr(ATTR_SOURCE, origin, false));
    attrs.extend(addr_attr(ATTR_RESPONSE_ORIGIN, origin, false));
    if let Some(o) = other {
        attrs.extend(addr_attr(ATTR_OTHER, o, false));
    }
    header(BINDING_RESPONSE, tid, &attrs)
}

/// 写一个属性并在**线上**补零到 4 字节对齐。
///
/// 关键区别：补零不能算进 LENGTH 字段。若把 0 补进属性值，客户端会把它读成
/// 列表里的第二个元素（UNKNOWN-ATTRIBUTES 的 2 字节列表尤其明显：
/// 结果会变成 `[0x0003, 0x0000]`，看起来像我们报了两个未知属性）。
fn attr_wire(atype: u16, value: &[u8]) -> Vec<u8> {
    let mut v = attr(atype, value);
    while v.len() % 4 != 0 {
        v.push(0);
    }
    v
}

/// 带 ERROR-CODE 的 BINDING 错误响应；`unknown` 非空时附加 UNKNOWN-ATTRIBUTES。
pub fn error_response(tid: &[u8; 12], code: u16, unknown: &[u16]) -> Vec<u8> {
    let class = (code / 100) as u8;
    let number = (code % 100) as u8;
    let mut value = vec![0u8, 0u8, class, number];
    let reason = match code {
        ERR_UNKNOWN_ATTRIBUTE => b"Unknown Attribute".as_slice(),
        _ => b"Error".as_slice(),
    };
    value.extend_from_slice(reason);
    let mut attrs = attr_wire(ATTR_ERROR_CODE, &value);
    if !unknown.is_empty() {
        let mut u = Vec::new();
        for a in unknown {
            u.extend_from_slice(&a.to_be_bytes());
        }
        attrs.extend(attr_wire(ATTR_UNKNOWN_ATTRIBUTES, &u));
    }
    header(BINDING_ERROR, tid, &attrs)
}

/// 收到的包属于哪一侧（决定 CHANGE-PORT 时从哪个套接字回包）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Primary,
    Alt,
}

pub struct StunServer {
    primary: Arc<UdpSocket>,
    /// 第二个套接字（同 IP、异端口）。没有它就只能测映射，不能测端口过滤。
    alt: Option<Arc<UdpSocket>>,
    pub stats: Arc<StunServerStats>,
}

impl StunServer {
    /// 绑定主套接字 + （可选）备套接字。两者必须同 IP 不同端口。
    pub fn bind(primary: SocketAddr, alt: Option<SocketAddr>) -> io::Result<Self> {
        let p = UdpSocket::bind(primary)?;
        let a = match alt {
            Some(a) => Some(Arc::new(UdpSocket::bind(a)?)),
            None => None,
        };
        Ok(Self {
            primary: Arc::new(p),
            alt: a,
            stats: Arc::new(StunServerStats::default()),
        })
    }

    /// 默认布局：`addr` 与 `addr.port + 1`。端口被占用时**不**偷偷换端口——
    /// 换端口会让客户端 ad 的 OTHER-ADDRESS 与命令行不一致，测试结果无从复核。
    pub fn bind_adjacent(addr: SocketAddr) -> io::Result<Self> {
        let alt = SocketAddr::new(addr.ip(), addr.port().saturating_add(1));
        Self::bind(addr, Some(alt))
    }

    pub fn primary_addr(&self) -> io::Result<SocketAddr> {
        self.primary.local_addr()
    }

    pub fn alt_addr(&self) -> Option<SocketAddr> {
        self.alt.as_ref().and_then(|a| a.local_addr().ok())
    }

    /// 处理一个报文，返回处理方式。`inbound` 决定「另一个套接字」是哪个。
    pub fn handle(&self, data: &[u8], peer: SocketAddr, inbound: Side) -> Decision {
        if data.len() < 20
            || u16::from_be_bytes([data[0], data[1]]) != BINDING_REQUEST
            || u32::from_be_bytes([data[4], data[5], data[6], data[7]]) != MAGIC
        {
            self.stats.malformed.fetch_add(1, Ordering::Relaxed);
            return Decision::Malformed;
        }
        self.stats.requests.fetch_add(1, Ordering::Relaxed);
        let mut tid = [0u8; 12];
        tid.copy_from_slice(&data[8..20]);
        let flags = change_flags_of(data);

        let here = match inbound {
            Side::Primary => self.primary.clone(),
            Side::Alt => match &self.alt {
                Some(a) => a.clone(),
                None => self.primary.clone(),
            },
        };
        let other = match inbound {
            Side::Primary => self.alt.clone(),
            Side::Alt => Some(self.primary.clone()),
        };
        let (here_addr, other_addr) = (
            here.local_addr().ok(),
            other.as_ref().and_then(|s| s.local_addr().ok()),
        );

        if flags & CHANGE_IP != 0 {
            // 单公网 IP：做不到。回 420，让客户端读到「能力缺失」而不是「NAT 拦了」。
            self.stats.change_ip_rejected.fetch_add(1, Ordering::Relaxed);
            let resp = error_response(&tid, ERR_UNKNOWN_ATTRIBUTE, &[ATTR_CHANGE_REQUEST]);
            let _ = here.send_to(&resp, peer);
            return Decision::ChangeIpRejected;
        }

        if flags & CHANGE_PORT != 0 {
            if let (Some(o), Some(oa)) = (&other, other_addr) {
                let resp = binding_response(&tid, peer, oa, Some(oa));
                let _ = o.send_to(&resp, peer);
                self.stats
                    .answered_from_peer_socket
                    .fetch_add(1, Ordering::Relaxed);
                return Decision::AnsweredFromPeerSocket;
            }
            // 没有备套接字：同样回 420，而不是假装回了个响应。
            self.stats.change_ip_rejected.fetch_add(1, Ordering::Relaxed);
            let resp = error_response(&tid, ERR_UNKNOWN_ATTRIBUTE, &[ATTR_CHANGE_REQUEST]);
            let _ = here.send_to(&resp, peer);
            return Decision::ChangeIpRejected;
        }

        if let Some(ha) = here_addr {
            let resp = binding_response(&tid, peer, ha, other_addr);
            let _ = here.send_to(&resp, peer);
            self.stats.answered_here.fetch_add(1, Ordering::Relaxed);
            Decision::AnsweredHere
        } else {
            Decision::Malformed
        }
    }

    /// 阻塞式服务：两个套接字各一个线程，`stop` 置位后 200 ms 内退出。
    /// 每处理一个包回调一次 `on_event`（CLI 用它打日志，库本身不打印）。
    pub fn serve_until<F>(self: &Arc<Self>, stop: Arc<AtomicBool>, on_event: F)
    where
        F: Fn(Decision, SocketAddr) + Send + Sync + 'static,
    {
        let on_event = Arc::new(on_event);
        let mut handles = Vec::new();
        for side in [Side::Primary, Side::Alt] {
            let sock = match side {
                Side::Primary => self.primary.clone(),
                Side::Alt => match &self.alt {
                    Some(a) => a.clone(),
                    None => continue,
                },
            };
            let srv = self.clone();
            let st = stop.clone();
            let cb = on_event.clone();
            handles.push(std::thread::spawn(move || {
                let _ = sock.set_read_timeout(Some(Duration::from_millis(200)));
                let mut buf = [0u8; 2048];
                while !st.load(Ordering::Relaxed) {
                    match sock.recv_from(&mut buf) {
                        Ok((n, peer)) => {
                            let d = srv.handle(&buf[..n], peer, side);
                            cb(d, peer);
                        }
                        Err(ref e)
                            if e.kind() == io::ErrorKind::WouldBlock
                                || e.kind() == io::ErrorKind::TimedOut => {}
                        Err(_) => std::thread::sleep(Duration::from_millis(50)),
                    }
                }
            }));
        }
        for h in handles {
            let _ = h.join();
        }
    }
}

/// 便捷：解析 "ip:port"（仅 IPv4），失败返回 None。
pub fn parse_v4(s: &str) -> Option<SocketAddr> {
    match s.parse::<SocketAddr>() {
        Ok(SocketAddr::V4(_)) => s.parse().ok(),
        _ => None,
    }
}

/// 备用端口的默认值（主端口 + 1）。
pub fn adjacent(addr: SocketAddr) -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(
        match addr {
            SocketAddr::V4(v4) => *v4.ip(),
            SocketAddr::V6(_) => Ipv4Addr::UNSPECIFIED,
        },
        addr.port().saturating_add(1),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stun::{binding_request_with, change_flags_of, parse_response};

    fn start(alt: bool) -> (Arc<StunServer>, SocketAddr, Option<SocketAddr>, Arc<AtomicBool>) {
        let primary: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let srv = Arc::new(StunServer::bind(primary, if alt { Some("127.0.0.1:0".parse().unwrap()) } else { None }).unwrap());
        let p = srv.primary_addr().unwrap();
        let a = srv.alt_addr();
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = srv.clone();
        let st = stop.clone();
        std::thread::spawn(move || s2.serve_until(st, |_, _| {}));
        (srv, p, a, stop)
    }

    fn exchange(sock: &UdpSocket, to: SocketAddr, flags: u32) -> (Vec<u8>, SocketAddr) {
        let tid = [3u8; 12];
        sock.send_to(&binding_request_with(&tid, flags), to).unwrap();
        sock.set_read_timeout(Some(Duration::from_millis(600))).unwrap();
        let mut buf = [0u8; 2048];
        let (n, src) = sock.recv_from(&mut buf).expect("应收到回包");
        (buf[..n].to_vec(), src)
    }

    #[test]
    fn plain_request_is_answered_from_the_same_socket() {
        let (_s, p, a, stop) = start(true);
        let c = UdpSocket::bind("127.0.0.1:0").unwrap();
        let (data, src) = exchange(&c, p, 0);
        assert_eq!(src, p);
        let tid = [3u8; 12];
        let parsed = parse_response(&data, &tid).expect("应解析成功");
        assert!(parsed.mapped.is_some());
        assert_eq!(parsed.other.as_deref(), a.map(|x| x.to_string()).as_deref());
        stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn change_port_comes_from_the_other_socket() {
        let (_s, p, a, stop) = start(true);
        let a = a.expect("应有备套接字");
        let c = UdpSocket::bind("127.0.0.1:0").unwrap();
        let (data, src) = exchange(&c, p, CHANGE_PORT);
        // 这一条是整个模块的存在理由：源口必须真的变。
        assert_ne!(src, p);
        assert_eq!(src, a);
        assert!(parse_response(&data, &[3u8; 12]).is_some());
        stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn change_ip_is_rejected_with_420_not_silently_ignored() {
        let (s, p, _a, stop) = start(true);
        let c = UdpSocket::bind("127.0.0.1:0").unwrap();
        let (data, src) = exchange(&c, p, CHANGE_IP);
        assert_eq!(src, p, "420 必须从收到请求的套接字回，客户端才知道是谁说的");
        assert_eq!(u16::from_be_bytes([data[0], data[1]]), BINDING_ERROR);
        let code = crate::stun::parse_error(&data, &[3u8; 12]).expect("应解析出错误码");
        assert_eq!(code.0, ERR_UNKNOWN_ATTRIBUTE);
        assert_eq!(code.1, vec![ATTR_CHANGE_REQUEST]);
        assert_eq!(s.stats.change_ip_rejected.load(Ordering::Relaxed), 1);
        stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn single_socket_mode_rejects_change_port_instead_of_faking_it() {
        let (_s, p, a, stop) = start(false);
        assert!(a.is_none());
        let c = UdpSocket::bind("127.0.0.1:0").unwrap();
        let (data, src) = exchange(&c, p, CHANGE_PORT);
        assert_eq!(src, p);
        assert_eq!(u16::from_be_bytes([data[0], data[1]]), BINDING_ERROR);
        let code = crate::stun::parse_error(&data, &[3u8; 12]).unwrap();
        assert_eq!(code.0, ERR_UNKNOWN_ATTRIBUTE);
        stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn malformed_packets_do_not_get_answers() {
        let (s, p, _a, stop) = start(true);
        let c = UdpSocket::bind("127.0.0.1:0").unwrap();
        c.send_to(b"not-stun", p).unwrap();
        c.set_read_timeout(Some(Duration::from_millis(250))).unwrap();
        let mut buf = [0u8; 64];
        assert!(c.recv_from(&mut buf).is_err(), "垃圾包不应得到响应");
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(s.stats.malformed.load(Ordering::Relaxed), 1);
        stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn change_request_attribute_length_is_still_8_bytes() {
        // 回归：加属性后必须修头部 LENGTH，否则严格解析的服务端会把属性当填充丢掉。
        let req = binding_request_with(&[1u8; 12], CHANGE_PORT);
        assert_eq!(u16::from_be_bytes([req[2], req[3]]) as usize, req.len() - 20);
        assert_eq!(change_flags_of(&req), CHANGE_PORT);
    }
}
