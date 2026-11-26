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
/// RFC 3489 CHANGE-REQUEST（RFC 5780 同义）：让服务器**换一个地址/端口**回包。
/// 这是细分 cone NAT 的唯一手段 —— 见 `filter_behavior_with`。
pub const ATTR_CHANGE_REQUEST: u16 = 0x0003;
pub const CHANGE_IP: u32 = 0x04;
pub const CHANGE_PORT: u32 = 0x02;

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

/// 带 CHANGE-REQUEST 的 Binding 请求（`change_flags` 为 0 时等价于 `binding_request`）。
///
/// 注意必须回头修正头部 LENGTH 字段：`binding_request` 写的是 0，追加属性后要改成
/// 「20 字节头之后的总长度」，否则按 RFC 5389 解析的服务器会把属性当成填充丢弃。
pub fn binding_request_with(tid: &[u8; 12], change_flags: u32) -> Vec<u8> {
    let mut v = binding_request(tid);
    if change_flags != 0 {
        v.extend_from_slice(&attr(ATTR_CHANGE_REQUEST, &change_flags.to_be_bytes()));
        let len = (v.len() - 20) as u16;
        v[2..4].copy_from_slice(&len.to_be_bytes());
    }
    v
}

/// 从请求字节里读出 CHANGE-REQUEST 的 flags（没有该属性则为 0）。
pub fn change_flags_of(data: &[u8]) -> u32 {
    if data.len() < 20 {
        return 0;
    }
    let mut off = 20usize;
    while off + 4 <= data.len() {
        let atype = u16::from_be_bytes([data[off], data[off + 1]]);
        let alen = u16::from_be_bytes([data[off + 2], data[off + 3]]) as usize;
        if off + 4 + alen > data.len() {
            break;
        }
        if atype == ATTR_CHANGE_REQUEST && alen >= 4 {
            let v = &data[off + 4..off + 8];
            return u32::from_be_bytes([v[0], v[1], v[2], v[3]]);
        }
        off += 4 + ((alen + 3) / 4) * 4;
    }
    0
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

/// 单个「CHANGE-REQUEST 是否真有应答」测试。
///
/// 判据是**回包源地址确实变了**，不是「收到了任意一个 tid 匹配的包」。
///
/// 这条曾经写错过一次：早期只校验 tid，于是**忽略 CHANGE-REQUEST 的服务器**从原地址回包
/// 也被算成「change-IP 有应答」，一台真实的 symmetric 设备因此被判成 `full-cone`。
/// 现在：
///   * 正对照（flags=0）要求 `src == addr`（回答确实来自被问的那台）；
///   * change 测试要求 `src != addr`（回答来自**另一个**套接字，才说明 NAT 放行了那个方向）。
fn change_request_answered<F>(
    sock: &UdpSocket,
    addr: std::net::SocketAddr,
    timeout: Duration,
    change_flags: u32,
    recv: &mut F,
) -> bool
where
    F: FnMut(Duration) -> Option<(Vec<u8>, std::net::SocketAddr)>,
{
    let tid = rand_tid();
    let req = binding_request_with(&tid, change_flags);
    if sock.send_to(&req, addr).is_err() {
        return false;
    }
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let remain = deadline.saturating_duration_since(std::time::Instant::now());
        if remain.is_zero() {
            return false;
        }
        match recv(remain) {
            Some((data, src)) => {
                if parse_response(&data, &tid).is_none() {
                    continue;
                }
                let same_source = src == addr;
                return if change_flags == 0 { same_source } else { !same_source };
            }
            None => return false,
        }
    }
}

/// RFC 3489 Test II / III：用 CHANGE-REQUEST 实测 NAT 的**过滤**行为。
///
/// 旧实现拿「NAT 映射地址」和「服务器 OTHER-ADDRESS」做比较来猜子类型，两者语义无关，
/// 只要服务器给了 OTHER-ADDRESS 就恒判 `RestrictedCone` —— 那是恒定值，不是测量。
/// 真正能分开三类的只有一件事：**换个地址/端口回包，这个包能不能进来**。
///
/// 顺序与判据：
/// 1. 先做**正对照**（不带 CHANGE-REQUEST）：必须能通，否则 STUN 不可达，结论只能是 `Unknown`；
/// 2. change-IP 有应答 → 入向不限源地址 → `FullCone`；
/// 3. 仅 change-port 有应答 → 限地址不限端口 → `RestrictedCone`；
/// 4. 都无应答 → 只放行已建立的映射 → `PortRestrictedCone`。
///
/// ⚠ **必须用一个"只跟主服务器说过话"的 socket**。如果这个 socket 之前已经向备用地址发过
/// 任何包（例如映射探测同时用了主备两台服务器），网关的过滤集里就有那个地址，change-IP 的
/// 回包会被放行 —— 受限锥/端口受限锥会被误判成 full cone。实验室里正是这么误判过一次，
/// 所以对外提供的入口是 `filter_behavior_fresh`（自己新建 socket）。
pub fn filter_behavior_with<F>(
    sock: &UdpSocket,
    host: &str,
    port: u16,
    timeout: Duration,
    mut recv: F,
) -> crate::classify::FilterBehavior
where
    F: FnMut(Duration) -> Option<(Vec<u8>, std::net::SocketAddr)>,
{
    use crate::classify::FilterBehavior;
    let server = format!("{}:{}", host, port);
    let addr = match std::net::ToSocketAddrs::to_socket_addrs(&server) {
        Ok(mut a) => match a.next() {
            Some(a) => a,
            None => return FilterBehavior::Unknown,
        },
        Err(_) => return FilterBehavior::Unknown,
    };
    if !change_request_answered(sock, addr, timeout, 0, &mut recv) {
        return FilterBehavior::Unknown;
    }
    if change_request_answered(sock, addr, timeout, CHANGE_IP, &mut recv) {
        return FilterBehavior::FullCone;
    }
    if change_request_answered(sock, addr, timeout, CHANGE_PORT, &mut recv) {
        return FilterBehavior::RestrictedCone;
    }
    FilterBehavior::PortRestrictedCone
}

/// 过滤行为分类的对外入口：**自建 socket**，保证测试期间只与主服务器通信过。
///
/// 为什么不复用监听/映射探测的 socket：那个 socket 往往已经联系过备用地址
/// （映射探测通常配置多台 STUN），过滤集里存在该地址会让 change-IP 测试假阳性，
/// 把 restricted / port-restricted 误判成 full cone。过滤行为是 NAT 映射的属性，
/// 不需要与唤醒端口一致，新建 socket 就能测得准。
pub fn filter_behavior_fresh(host: &str, port: u16, timeout: Duration) -> crate::classify::FilterBehavior {
    match std::net::UdpSocket::bind("0.0.0.0:0") {
        Ok(sock) => filter_behavior(&sock, host, port, timeout),
        Err(_) => crate::classify::FilterBehavior::Unknown,
    }
}

/// `filter_behavior_with` 的自带接收版本（独占 socket 时使用）。
pub fn filter_behavior(
    sock: &UdpSocket,
    host: &str,
    port: u16,
    timeout: Duration,
) -> crate::classify::FilterBehavior {
    let mut buf = [0u8; 2048];
    filter_behavior_with(sock, host, port, timeout, |remain| {
        let _ = sock.set_read_timeout(Some(remain));
        match sock.recv_from(&mut buf) {
            Ok((n, src)) => Some((buf[..n].to_vec(), src)),
            Err(_) => None,
        }
    })
}

/// 解析 BINDING **错误**响应（0x0111），返回 `(错误码, UNKNOWN-ATTRIBUTES 列表)`。
///
/// 存在的理由：单公网 IP 的 STUN 服务端对 CHANGE-IP 只能回 420。客户端必须能把这件事
/// 读成「服务器不具备该能力」，而不是「NAT 把包拦了」——两者在旧判据里长得一模一样。
pub fn parse_error(data: &[u8], tid: &[u8; 12]) -> Option<(u16, Vec<u16>)> {
    if data.len() < 20
        || u16::from_be_bytes([data[0], data[1]]) != 0x0111
        || u32::from_be_bytes([data[4], data[5], data[6], data[7]]) != MAGIC
        || &data[8..20] != tid
    {
        return None;
    }
    let mut pos = 20usize;
    let mut code = None;
    let mut unknown = Vec::new();
    while pos + 4 <= data.len() {
        let atype = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let alen = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;
        if pos + alen > data.len() {
            break;
        }
        let val = &data[pos..pos + alen];
        match atype {
            0x0009 if alen >= 4 => {
                let class = val[2] as u16;
                let num = val[3] as u16;
                code = Some(class * 100 + num);
            }
            0x000A => {
                let mut i = 0;
                while i + 2 <= alen {
                    unknown.push(u16::from_be_bytes([val[i], val[i + 1]]));
                    i += 2;
                }
            }
            _ => {}
        }
        pos += alen + (4 - alen % 4) % 4;
    }
    code.map(|c| (c, unknown))
}

/// 端口过滤轴的实测结论。**只有三种值**，没有第四种「大概」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOutcome {
    /// 双对照都过、change-port 确实无应答 → 入向只放行已建立的 (IP,port)。
    PortFiltered,
    /// 双对照都过、change-port 有应答且源确实是对端备口 → 端口过滤不存在。
    PortNotFiltered,
    /// 任一对照失败 / 源地址被改写 / 服务器忽略请求 → 什么都没测出来。
    Untested,
}

impl FilterOutcome {
    pub fn label(&self) -> &'static str {
        match self {
            FilterOutcome::PortFiltered => "port-filtered",
            FilterOutcome::PortNotFiltered => "port-not-filtered",
            FilterOutcome::Untested => "untested",
        }
    }
}

/// 一次「对外部观测点」的端口过滤实测，含**全部原始证据**。
///
/// 三个字段构成可复核的最小集合：两个正对照（主口可达、备口可达）说明观测点本身是通的，
/// 缺任一对照，负面结论就不可信；`source_matches_alt` 说明回包源口确实变了，
/// 否则「服务器忽略请求」会被误读成「NAT 过滤」——公共 STUN 就是这个坑。
#[derive(Debug, Clone)]
pub struct FilterProbe {
    pub vantage: String,
    pub control_primary: bool,
    pub control_alt: bool,
    pub change_port_answered: bool,
    pub change_port_source: Option<String>,
    pub source_matches_alt: bool,
    pub change_ip_error: Option<u16>,
    pub outcome: FilterOutcome,
}

fn resolve_v4(host: &str, port: u16) -> Option<std::net::SocketAddr> {
    use std::net::ToSocketAddrs;
    let it = (host, port).to_socket_addrs().ok()?;
    let mut fallback = None;
    for a in it {
        if a.is_ipv4() {
            return Some(a);
        }
        fallback.get_or_insert(a);
    }
    fallback
}

fn send_recv(
    sock: &UdpSocket,
    to: std::net::SocketAddr,
    flags: u32,
    timeout: Duration,
) -> Option<(Vec<u8>, std::net::SocketAddr)> {
    let tid = rand_tid();
    sock.send_to(&binding_request_with(&tid, flags), to).ok()?;
    let deadline = std::time::Instant::now() + timeout;
    let mut buf = [0u8; 2048];
    loop {
        let remain = deadline.saturating_duration_since(std::time::Instant::now());
        if remain.is_zero() {
            return None;
        }
        let _ = sock.set_read_timeout(Some(remain));
        match sock.recv_from(&mut buf) {
            Ok((n, src)) => {
                let data = buf[..n].to_vec();
                if parse_response(&data, &tid).is_some() || parse_error(&data, &tid).is_some() {
                    return Some((data, src));
                }
            }
            Err(_) => return None,
        }
    }
}

/// 用**自建 RFC 5780 观测点**实测端口过滤行为（这是唯一能撑住结论的路径）。
///
/// 三个 socket 的分工不能合并，否则互相污染：
///   1. 对照 A（主口直连）：证明观测点主口入向可达；
///   2. 对照 B（备口直连，**另一个 socket**）：证明备口入向也可达。绝不能复用第 3 步的
///      socket —— 一旦该 socket 向备口发过包，网关过滤集里就有备口地址，
///      「端口受限」会被测成「不限端口」，这是过滤测试最经典的假阳性；
///   3. 被测 socket：只与主口通信，再请求 CHANGE-PORT。
pub fn probe_port_filtering(
    host: &str,
    port: u16,
    alt_port: u16,
    timeout: Duration,
) -> FilterProbe {
    let mut p = FilterProbe {
        vantage: format!("{}:{}", host, port),
        control_primary: false,
        control_alt: false,
        change_port_answered: false,
        change_port_source: None,
        source_matches_alt: false,
        change_ip_error: None,
        outcome: FilterOutcome::Untested,
    };
    let primary = match resolve_v4(host, port) {
        Some(a) => a,
        None => return p,
    };
    let alt = std::net::SocketAddr::new(primary.ip(), alt_port);

    // 对照 A
    if let Ok(s1) = UdpSocket::bind("0.0.0.0:0") {
        p.control_primary = matches!(send_recv(&s1, primary, 0, timeout), Some((_, src)) if src == primary);
    }
    if !p.control_primary {
        return p;
    }
    // 对照 B（独立 socket，避免污染被测 socket 的过滤集）
    if let Ok(s2) = UdpSocket::bind("0.0.0.0:0") {
        p.control_alt = matches!(send_recv(&s2, alt, 0, timeout), Some((_, src)) if src == alt);
    }
    if !p.control_alt {
        return p; // 备口不可达时，change-port 的「无应答」不构成任何证据
    }
    // 被测：只跟主口说过话的 socket
    let tested = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return p,
    };
    if let Some((_, src)) = send_recv(&tested, primary, CHANGE_PORT, timeout) {
        p.change_port_answered = true;
        p.change_port_source = Some(src.to_string());
        p.source_matches_alt = src == alt;
    }
    // CHANGE-IP 能力询问（单 IP 观测点会回 420）
    if let Ok(s4) = UdpSocket::bind("0.0.0.0:0") {
        let tid = rand_tid();
        if s4.send_to(&binding_request_with(&tid, CHANGE_IP), primary).is_ok() {
            let _ = s4.set_read_timeout(Some(timeout));
            let mut buf = [0u8; 2048];
            if let Ok((n, _)) = s4.recv_from(&mut buf) {
                if let Some((code, _)) = parse_error(&buf[..n], &tid) {
                    p.change_ip_error = Some(code);
                }
            }
        }
    }

    p.outcome = if !p.change_port_answered {
        FilterOutcome::PortFiltered
    } else if p.source_matches_alt {
        FilterOutcome::PortNotFiltered
    } else {
        // 有回包但源没变：服务器忽略了 CHANGE-PORT，与公共 STUN 同样无从判断。
        FilterOutcome::Untested
    };
    p
}

pub fn rand_tid() -> [u8; 12] {    let mut tid = [0u8; 12];
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

    /// 起一个「按指定能力回包」的假 STUN：`allow_ip` / `allow_port` 决定是否回应
    /// 带 CHANGE-IP / CHANGE-PORT 的请求；回应从**另一个**端口（`alt`）发出，
    /// 客户端据此判定「回包确实换了源」。
    fn spawn_filter_server(allow_ip: bool, allow_port: bool) -> std::net::SocketAddr {
        let srv = UdpSocket::bind("127.0.0.1:0").unwrap();
        let alt = UdpSocket::bind("127.0.0.1:0").unwrap();
        let main = srv.local_addr().unwrap();
        thread::spawn(move || {
            let mut buf = [0u8; 2048];
            loop {
                let (n, peer) = match srv.recv_from(&mut buf) {
                    Ok(v) => v,
                    Err(_) => return,
                };
                if n < 20 {
                    continue;
                }
                let flags = change_flags_of(&buf[..n]);
                let tid: [u8; 12] = buf[8..20].try_into().unwrap();
                let resp = build_response(&tid, &build_xor_mapped(peer));
                if flags & CHANGE_IP != 0 {
                    if allow_ip {
                        let _ = alt.send_to(&resp, peer);
                    }
                } else if flags & CHANGE_PORT != 0 {
                    if allow_port {
                        let _ = alt.send_to(&resp, peer);
                    }
                } else {
                    let _ = srv.send_to(&resp, peer);
                }
            }
        });
        main
    }

    #[test]
    fn change_request_flags_are_encoded() {
        let tid = [7u8; 12];
        let req = binding_request_with(&tid, CHANGE_IP);
        assert_eq!(req.len(), 28, "20 字节头 + 8 字节 CHANGE-REQUEST 属性");
        assert_eq!(
            u16::from_be_bytes([req[2], req[3]]),
            8,
            "LENGTH 字段必须等于头之后的总长度"
        );
        assert_eq!(change_flags_of(&req), CHANGE_IP);
        // 不带 flags 时必须与原始请求逐字节一致（不引入额外属性）
        assert_eq!(binding_request_with(&tid, 0), binding_request(&tid));
    }

    #[test]
    fn filter_full_cone_answers_change_ip() {
        let srv = spawn_filter_server(true, false);
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        let f = filter_behavior(&client, "127.0.0.1", srv.port(), Duration::from_millis(400));
        assert_eq!(f, crate::classify::FilterBehavior::FullCone);
    }

    #[test]
    fn filter_restricted_cone_answers_only_change_port() {
        let srv = spawn_filter_server(false, true);
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        let f = filter_behavior(&client, "127.0.0.1", srv.port(), Duration::from_millis(400));
        assert_eq!(f, crate::classify::FilterBehavior::RestrictedCone);
    }

    #[test]
    fn filter_port_restricted_when_no_change_answer() {
        let srv = spawn_filter_server(false, false);
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        let f = filter_behavior(&client, "127.0.0.1", srv.port(), Duration::from_millis(400));
        assert_eq!(f, crate::classify::FilterBehavior::PortRestrictedCone);
    }

    /// 忽略 CHANGE-REQUEST 的服务器：无论请求带什么 flags，都从**原地址**回包。
    /// 这正是公共 STUN 的常见行为，也是「假 full-cone」的来源。
    #[test]
    fn filter_ignored_change_request_is_not_full_cone() {
        let srv = UdpSocket::bind("127.0.0.1:0").unwrap();
        let main = srv.local_addr().unwrap();
        thread::spawn(move || {
            let mut buf = [0u8; 2048];
            loop {
                let (n, peer) = match srv.recv_from(&mut buf) {
                    Ok(v) => v,
                    Err(_) => return,
                };
                if n < 20 {
                    continue;
                }
                let tid: [u8; 12] = buf[8..20].try_into().unwrap();
                let resp = build_response(&tid, &build_xor_mapped(peer));
                let _ = srv.send_to(&resp, peer);   // 永远从主地址回
            }
        });
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        let f = filter_behavior(&client, "127.0.0.1", main.port(), Duration::from_millis(400));
        // 关键：不能因为「收到了包」就判 full-cone，源没变 ≤ 只放行已有映射
        assert_eq!(f, crate::classify::FilterBehavior::PortRestrictedCone);
    }

    #[test]
    fn filter_unknown_when_server_unreachable() {        // 正对照都收不到应答：不得把「STUN 不可达」当成「端口受限」。
        let dead_port = {
            let s = UdpSocket::bind("127.0.0.1:0").unwrap();
            s.local_addr().unwrap().port()
        };
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        let f = filter_behavior(&client, "127.0.0.1", dead_port, Duration::from_millis(200));
        assert_eq!(f, crate::classify::FilterBehavior::Unknown);
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

    // ------------------------------------------------------------------
    // 外部观测点（自建 RFC 5780 服务端）的双对照探测
    // ------------------------------------------------------------------

    fn spawn_vantage(with_alt: bool) -> (std::sync::Arc<crate::stun_server::StunServer>, u16, Option<u16>, std::sync::Arc<std::sync::atomic::AtomicBool>) {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;
        let srv = Arc::new(
            crate::stun_server::StunServer::bind(
                "127.0.0.1:0".parse().unwrap(),
                if with_alt { Some("127.0.0.1:0".parse().unwrap()) } else { None },
            )
            .unwrap(),
        );
        let p = srv.primary_addr().unwrap().port();
        let a = srv.alt_addr().map(|x| x.port());
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = srv.clone();
        let st = stop.clone();
        thread::spawn(move || s2.serve_until(st, |_, _| {}));
        (srv, p, a, stop)
    }

    #[test]
    fn probe_against_real_vantage_reports_port_open() {
        let (_s, p, a, stop) = spawn_vantage(true);
        let a = a.expect("应有备口");
        let probe = probe_port_filtering("127.0.0.1", p, a, Duration::from_millis(500));
        assert!(probe.control_primary, "主口对照必须通");
        assert!(probe.control_alt, "备口对照必须通");
        assert_eq!(probe.outcome, FilterOutcome::PortNotFiltered);
        assert!(probe.source_matches_alt, "回包源必须真的是备口");
        assert_eq!(probe.change_ip_error, Some(420), "单 IP 观测点应明说做不到 change-IP");
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    #[test]
    fn probe_without_alt_socket_is_untested_not_port_filtered() {
        // 单套接字观测点：备口对照必然失败。此时「change-port 无应答」不是证据，
        // 只能说 untested —— 这正是拿公共 STUN 判过滤行为时会犯的错。
        let (_s, p, a, stop) = spawn_vantage(false);
        assert!(a.is_none());
        let probe = probe_port_filtering("127.0.0.1", p, p + 1, Duration::from_millis(400));
        assert!(probe.control_primary);
        assert!(!probe.control_alt);
        assert_eq!(probe.outcome, FilterOutcome::Untested);
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    #[test]
    fn ignored_change_port_is_untested_not_port_filtered() {
        // 模拟公共 STUN：主备口都能直连应答，但**忽略** CHANGE-PORT，永远从主口回包。
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let main = UdpSocket::bind("127.0.0.1:0").unwrap();
        let alt = UdpSocket::bind("127.0.0.1:0").unwrap();
        let (mp, ap) = (main.local_addr().unwrap().port(), alt.local_addr().unwrap().port());
        let stop_cell = Arc::new(AtomicBool::new(false));
        for sock in [main, alt] {
            let st = stop_cell.clone();
            thread::spawn(move || {
                let mut buf = [0u8; 2048];
                let _ = sock.set_read_timeout(Some(Duration::from_millis(150)));
                while !st.load(Ordering::Relaxed) {
                    if let Ok((n, peer)) = sock.recv_from(&mut buf) {
                        if n >= 20 {
                            let tid: [u8; 12] = buf[8..20].try_into().unwrap();
                            let resp = build_response(&tid, &build_xor_mapped(peer));
                            // 关键：不管请求带什么 flags，都从**收到请求的那个口**回
                            let _ = sock.send_to(&resp, peer);
                        }
                    }
                }
            });
        }
        let probe = probe_port_filtering("127.0.0.1", mp, ap, Duration::from_millis(400));
        assert!(probe.control_primary && probe.control_alt);
        assert!(probe.change_port_answered, "确实收到了回包");
        assert!(!probe.source_matches_alt, "但源没变");
        assert_eq!(probe.outcome, FilterOutcome::Untested);
        stop_cell.store(true, Ordering::Relaxed);
    }
}
