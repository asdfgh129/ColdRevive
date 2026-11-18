//! UDP 承载的中继（W3.14）——给「入向 TCP 被边缘注入伪造 RST」的部署一条可用路径。
//!
//! 背景：2026-11-18 在一台腾讯云轻量上实测，**非 22 的入向 TCP 全部在建连后被第三方
//! 注入伪造 RST**（TTL 随机化 215/65/169、比 SYN-ACK 晚 270 µs；排除本机 netfilter、
//! 协议、端口、云防火墙、云镜），而**同一台机器的入向 UDP 全程可用**（BEP 5 DHT 与
//! RFC 5780 STUN 都跑通）。因此中继需要一层 UDP 承载：不是「换个端口再试一次 TCP」，
//! 而是换掉被掐的那一层。
//!
//! # 报文格式（大端，单报文自成一体）
//!
//! ```text
//! 0..4   魔数 "CRU1"
//! 4      1B kind
//! 5      1B flags（保留，必须 0）
//! 6..8   2B 负载长度
//! 8..24  16B session（发送方节点 id）
//! 24..32 8B seq（每 socket 单调递增，用于抗重放）
//! 32..64 32B HMAC-SHA256(psk, 前 32 字节 || 负载)
//! 64..   负载
//! ```
//!
//! 定长 64 字节头 + 最多 1200 字节负载 = 1264 字节，**留在 1280 的 IPv6 最小 MTU 之内**，
//! 不需要分片诊断就能跨过大多数隧道/云网络。
//!
//! # 与 TCP 帧协议的关系
//!
//! **刻意不复用** `relay::FrameKind` 的编号：UDP 是另一套线缆空间（魔数不同），
//! 复用编号只会让人以为两种帧可以互转。语义对齐、编号独立，见 [`UdpKind`]。
//!
//! # 拒绝必须带原因
//!
//! 与 TCP 侧同一条教训（见 `FrameKind::Reject` 的注释）：只丢包不改声，客户端看到的是
//! 「超时」，运维会去查网络。UDP 的 `REJECT` 帧带 UTF-8 原因，`STATUS` 帧带明确状态码
//! （`forwarded` / `target_absent` / `rate_limited` / `too_big`），让「中继侧拒绝」和
//! 「路径丢包」可区分。
//!
//! # 反放大
//!
//! 每个报文都要求 PSK 的 HMAC：伪造源地址的反射攻击拿不到有效 MAC，报文被静默丢弃
//! （计 `mac_fail`）。回程字节数**不大于**来程（`PONG`/`STATUS` 都短于触发它们的报文），
//! 因此本服务不构成放大器。
//!
//! # 诚实边界
//!
//! - 这里只有**可达性与往返时延**：UDP 不重传，`DATA` 丢就是丢，`STATUS` 只说明
//!   「服务端是否转出去了」，不代表对端应用收到了。
//! - 本模块**不改变** `S6.status`：它证明「这条路能走」，不构成 NAT 四类矩阵。

use crate::relay::NODE_ID_LEN;
use crate::secure::PSK_LEN;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// 魔数：`CRU1` = ColdRevive UDP v1。
pub const MAGIC: [u8; 4] = *b"CRU1";
/// 定长头：魔数 4 + kind 1 + flags 1 + len 2 + session 16 + seq 8。
pub const UDP_HEADER_LEN: usize = 4 + 1 + 1 + 2 + NODE_ID_LEN + 8;
/// MAC 长度（HMAC-SHA256 全量，不截断：UDP 头部开销我们负担得起）。
pub const UDP_MAC_LEN: usize = 32;
/// 一个报文的最小长度（头 + MAC）。
pub const UDP_MIN_DGRAM: usize = UDP_HEADER_LEN + UDP_MAC_LEN;
/// 默认单报文负载上限。+头 = 1264 字节，仍在 1280 之内。
pub const UDP_MAX_PAYLOAD: usize = 1200;

/// UDP 帧类型（**独立编号空间**，不与 TCP 的 `FrameKind` 混用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UdpKind {
    /// 注册：把 `session` 绑到来源地址。负载为空。
    Hello,
    /// 注册应答。负载 = 8B 服务端时刻（毫秒，仅供日志对齐）。
    HelloAck,
    /// 请求转发。负载 = 16B 目标 id + 消息。
    Data,
    /// 转发投递。负载 = 16B 来源 id + 消息。
    Deliver,
    /// 存活/时延探测。负载 = 8B 令牌（客户端自选，原样回显）。
    Ping,
    /// 探测应答。负载 = 8B 令牌 + 8B 服务端时刻。**不含客户端时延计算**——
    /// RTT 一律由客户端用同一个单调时钟算，不依赖两端时钟同步。
    Pong,
    /// `Data` 的处理结果。负载 = 1B 状态 + 8B 对应的 seq。
    Status,
    /// 主动注销。负载为空。
    Bye,
    /// 拒绝。负载 = UTF-8 原因。
    Reject,
    Unknown(u8),
}

impl UdpKind {
    pub fn code(&self) -> u8 {
        match self {
            UdpKind::Hello => 0x01,
            UdpKind::HelloAck => 0x02,
            UdpKind::Data => 0x03,
            UdpKind::Deliver => 0x04,
            UdpKind::Ping => 0x05,
            UdpKind::Pong => 0x06,
            UdpKind::Status => 0x07,
            UdpKind::Bye => 0x08,
            UdpKind::Reject => 0x09,
            UdpKind::Unknown(c) => *c,
        }
    }
    pub fn from_code(c: u8) -> UdpKind {
        match c {
            0x01 => UdpKind::Hello,
            0x02 => UdpKind::HelloAck,
            0x03 => UdpKind::Data,
            0x04 => UdpKind::Deliver,
            0x05 => UdpKind::Ping,
            0x06 => UdpKind::Pong,
            0x07 => UdpKind::Status,
            0x08 => UdpKind::Bye,
            0x09 => UdpKind::Reject,
            other => UdpKind::Unknown(other),
        }
    }
}

/// `Status` 的状态码。**不要合并**：`target_absent`（对端没注册）和 `rate_limited`
/// （我们挡了）和「没收到 STATUS」（路径丢了）是三种完全不同的排障方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UdpStatus {
    Forwarded,
    TargetAbsent,
    RateLimited,
    TooBig,
    NotRegistered,
}

impl UdpStatus {
    pub fn code(&self) -> u8 {
        match self {
            UdpStatus::Forwarded => 0,
            UdpStatus::TargetAbsent => 1,
            UdpStatus::RateLimited => 2,
            UdpStatus::TooBig => 3,
            UdpStatus::NotRegistered => 4,
        }
    }
    pub fn from_code(c: u8) -> Option<UdpStatus> {
        Some(match c {
            0 => UdpStatus::Forwarded,
            1 => UdpStatus::TargetAbsent,
            2 => UdpStatus::RateLimited,
            3 => UdpStatus::TooBig,
            4 => UdpStatus::NotRegistered,
            _ => return None,
        })
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            UdpStatus::Forwarded => "forwarded",
            UdpStatus::TargetAbsent => "target_absent",
            UdpStatus::RateLimited => "rate_limited",
            UdpStatus::TooBig => "too_big",
            UdpStatus::NotRegistered => "not_registered",
        }
    }
}

/// 解析失败的四类原因。**分开计数**，否则「密钥配错」会被看成「网络不通」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UdpParseError {
    /// 太短 / 长度字段与报文不符
    Format,
    /// 魔数不符（很可能打到了别的 UDP 服务）
    Magic,
    /// MAC 校验失败（密钥不一致，或被伪造）
    Mac,
    /// 负载超过上限
    TooBig,
}

impl UdpParseError {
    pub fn as_str(&self) -> &'static str {
        match self {
            UdpParseError::Format => "format",
            UdpParseError::Magic => "magic",
            UdpParseError::Mac => "mac",
            UdpParseError::TooBig => "too_big",
        }
    }
}

/// 一个已解析的报文（**MAC 已验证**）。
#[derive(Debug, Clone)]
pub struct UdpDatagram {
    pub kind: UdpKind,
    pub session: [u8; NODE_ID_LEN],
    pub seq: u64,
    pub payload: Vec<u8>,
}

fn mac_of(psk: &[u8; PSK_LEN], head: &[u8], payload: &[u8]) -> [u8; UDP_MAC_LEN] {
    // 只对「头 32 字节 + 负载」签名。**不能**直接把整段缓冲切片签名：MAC 段自身
    // 夹在头和负载之间，按长度切片会签错区域——这个错会让所有报文验签失败，
    // 表现却是「服务端不说话」，很难查。
    let mut msg = Vec::with_capacity(head.len() + payload.len());
    msg.extend_from_slice(head);
    msg.extend_from_slice(payload);
    cr_hash::hmac_sha256(psk, &msg)
}

/// 编码一个报文。`psk` 为 `None` 时 MAC 段填零（只能用于回环/测试，公网部署由二进制
/// 侧策略禁止）。
pub fn encode_datagram(
    kind: UdpKind,
    session: &[u8; NODE_ID_LEN],
    seq: u64,
    payload: &[u8],
    psk: Option<&[u8; PSK_LEN]>,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(UDP_MIN_DGRAM + payload.len());
    out.extend_from_slice(&MAGIC);
    out.push(kind.code());
    out.push(0);
    out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    out.extend_from_slice(session);
    out.extend_from_slice(&seq.to_be_bytes());
    out.extend_from_slice(&[0u8; UDP_MAC_LEN]);
    out.extend_from_slice(payload);
    if let Some(k) = psk {
        let mac = mac_of(k, &out[..UDP_HEADER_LEN], payload);
        out[UDP_HEADER_LEN..UDP_HEADER_LEN + UDP_MAC_LEN].copy_from_slice(&mac);
    }
    out
}

/// 解析并**验签**。顺序刻意先魔数再长度再 MAC：把「不是我们的报文」和「密钥不对」
/// 在计数上彻底分开。
pub fn parse_datagram(
    buf: &[u8],
    psk: Option<&[u8; PSK_LEN]>,
    max_payload: usize,
) -> Result<UdpDatagram, UdpParseError> {
    if buf.len() < UDP_MIN_DGRAM {
        return Err(UdpParseError::Format);
    }
    if buf[..4] != MAGIC {
        return Err(UdpParseError::Magic);
    }
    let len = u16::from_be_bytes([buf[6], buf[7]]) as usize;
    if buf.len() != UDP_MIN_DGRAM + len {
        return Err(UdpParseError::Format);
    }
    if len > max_payload {
        return Err(UdpParseError::TooBig);
    }
    if let Some(k) = psk {
        let want = mac_of(k, &buf[..UDP_HEADER_LEN], &buf[UDP_MIN_DGRAM..]);
        // 定长时间比较：MAC 比较不做短路，避免把「前几字节对了」泄露成计时侧信道。
        let mut diff = 0u8;
        for i in 0..UDP_MAC_LEN {
            diff |= buf[UDP_HEADER_LEN + i] ^ want[i];
        }
        if diff != 0 {
            return Err(UdpParseError::Mac);
        }
    }
    let mut session = [0u8; NODE_ID_LEN];
    session.copy_from_slice(&buf[8..8 + NODE_ID_LEN]);
    let mut seqb = [0u8; 8];
    seqb.copy_from_slice(&buf[24..32]);
    Ok(UdpDatagram {
        kind: UdpKind::from_code(buf[4]),
        session,
        seq: u64::from_be_bytes(seqb),
        payload: buf[UDP_MIN_DGRAM..].to_vec(),
    })
}

/// 服务端护栏。与 TCP 侧 [`crate::relay_server::Limits`] 同口径，但 UDP 没有连接，
/// 所以「并发数」变成「会话表大小」，「每连接速率」变成「每会话速率」。
#[derive(Debug, Clone, Copy)]
pub struct UdpLimits {
    pub max_sessions: usize,
    pub max_per_ip: usize,
    pub idle_timeout: Duration,
    pub rate_pps: u32,
    pub max_payload: usize,
}

impl Default for UdpLimits {
    fn default() -> Self {
        Self {
            max_sessions: 4096,
            max_per_ip: 8,
            idle_timeout: Duration::from_secs(60),
            rate_pps: 2000,
            max_payload: UDP_MAX_PAYLOAD,
        }
    }
}

/// 机读计数。全用原子量，因为句柄在别的线程里读。
#[derive(Debug, Default)]
pub struct UdpStatsInner {
    pub datagrams: AtomicU64,
    pub bytes_in: AtomicU64,
    pub bytes_out: AtomicU64,
    pub hello: AtomicU64,
    pub hello_ack: AtomicU64,
    pub rebound: AtomicU64,
    pub data_in: AtomicU64,
    pub deliver_out: AtomicU64,
    pub status_out: AtomicU64,
    pub pong_out: AtomicU64,
    pub reject_out: AtomicU64,
    pub bye: AtomicU64,
    pub mac_fail: AtomicU64,
    pub bad_format: AtomicU64,
    pub magic_mismatch: AtomicU64,
    pub too_big: AtomicU64,
    pub replay: AtomicU64,
    pub rate_limited: AtomicU64,
    pub not_registered: AtomicU64,
    pub target_absent: AtomicU64,
    pub sessions_rejected: AtomicU64,
    pub sessions_active: AtomicU64,
    pub sessions_expired: AtomicU64,
}

/// 计数快照（可序列化）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UdpRelayStats {
    pub datagrams: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub hello: u64,
    pub hello_ack: u64,
    pub rebound: u64,
    pub data_in: u64,
    pub deliver_out: u64,
    pub status_out: u64,
    pub pong_out: u64,
    pub reject_out: u64,
    pub bye: u64,
    pub mac_fail: u64,
    pub bad_format: u64,
    pub magic_mismatch: u64,
    pub too_big: u64,
    pub replay: u64,
    pub rate_limited: u64,
    pub not_registered: u64,
    pub target_absent: u64,
    pub sessions_rejected: u64,
    pub sessions_active: u64,
    pub sessions_expired: u64,
}

impl UdpStatsInner {
    pub fn snapshot(&self) -> UdpRelayStats {
        let g = |a: &AtomicU64| a.load(Ordering::Relaxed);
        UdpRelayStats {
            datagrams: g(&self.datagrams),
            bytes_in: g(&self.bytes_in),
            bytes_out: g(&self.bytes_out),
            hello: g(&self.hello),
            hello_ack: g(&self.hello_ack),
            rebound: g(&self.rebound),
            data_in: g(&self.data_in),
            deliver_out: g(&self.deliver_out),
            status_out: g(&self.status_out),
            pong_out: g(&self.pong_out),
            reject_out: g(&self.reject_out),
            bye: g(&self.bye),
            mac_fail: g(&self.mac_fail),
            bad_format: g(&self.bad_format),
            magic_mismatch: g(&self.magic_mismatch),
            too_big: g(&self.too_big),
            replay: g(&self.replay),
            rate_limited: g(&self.rate_limited),
            not_registered: g(&self.not_registered),
            target_absent: g(&self.target_absent),
            sessions_rejected: g(&self.sessions_rejected),
            sessions_active: g(&self.sessions_active),
            sessions_expired: g(&self.sessions_expired),
        }
    }
}

/// 一个会话（= 一个节点 id + 当前来源地址）。
///
/// 地址会变：NAT 重绑定、移动网络切换都很正常，所以**同 id 新地址 = 漫游**，
/// 换绑并计 `rebound`，而不是拒绝。身份由 MAC 保证，地址只是投递目的地。
struct Session {
    addr: SocketAddr,
    last_seen: Instant,
    highest: u64,
    window: u64,
    out_seq: u64,
    window_start: Instant,
    window_pkts: u32,
}

impl Session {
    fn new(addr: SocketAddr, seq: u64) -> Self {
        Self {
            addr,
            last_seen: Instant::now(),
            highest: seq,
            window: 1,
            out_seq: now_ms().wrapping_mul(1024),
            window_start: Instant::now(),
            window_pkts: 1,
        }
    }

    /// 抗重放滑窗，接受乱序（UDP 本来就会乱序），只丢真正重复的。
    fn accept_seq(&mut self, seq: u64) -> bool {
        if seq > self.highest {
            let diff = seq - self.highest;
            self.window = if diff >= 64 { 0 } else { self.window << diff };
            self.window |= 1;
            self.highest = seq;
            return true;
        }
        let back = self.highest - seq;
        if back >= 64 {
            return false;
        }
        let mask = 1u64 << back;
        if self.window & mask != 0 {
            return false;
        }
        self.window |= mask;
        true
    }

    /// 每秒粗粒度配额。**不做平滑令牌桶**：UDP 侧的用途是「挡滥用」，不是 SLO 整形，
    /// 粗粒度意味着计数可解释、可复现。
    fn allow(&mut self, pps: u32) -> bool {
        let now = Instant::now();
        if now.duration_since(self.window_start) >= Duration::from_secs(1) {
            self.window_start = now;
            self.window_pkts = 0;
        }
        self.window_pkts = self.window_pkts.saturating_add(1);
        self.window_pkts <= pps
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// UDP 中继服务端。`process` 是纯逻辑（返回「要发给谁什么」），便于单测；
/// [`UdpRelayServer::spawn`] 只负责收包、调用 `process`、发包。
pub struct UdpRelayServer {
    sock: Arc<UdpSocket>,
    psk: Option<[u8; PSK_LEN]>,
    limits: UdpLimits,
    sessions: HashMap<[u8; NODE_ID_LEN], Session>,
    by_addr: HashMap<SocketAddr, [u8; NODE_ID_LEN]>,
    stats: Arc<UdpStatsInner>,
    running: Arc<AtomicBool>,
    last_reap: Instant,
    seed: u64,
}

/// 服务端句柄：可读计数、可停。
pub struct UdpRelayHandle {
    pub addr: SocketAddr,
    stats: Arc<UdpStatsInner>,
    running: Arc<AtomicBool>,
    sock: Arc<UdpSocket>,
    psk: Option<[u8; PSK_LEN]>,
}

impl UdpRelayHandle {
    pub fn stats(&self) -> UdpRelayStats {
        self.stats.snapshot()
    }
    /// 把计数器的共享句柄借出去，供「周期性落盘」的线程直接读，
    /// 避免每 200 ms 复制一份快照的所有权问题。
    pub fn stats_handle(&self) -> Arc<UdpStatsInner> {
        self.stats.clone()
    }
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }
    /// 本句柄所在服务端是否配了 PSK（供启动日志/自检打印）。
    pub fn has_psk(&self) -> bool {
        self.psk.is_some()
    }
    pub fn socket(&self) -> Arc<UdpSocket> {
        self.sock.clone()
    }
}

impl UdpRelayServer {
    pub fn bind(addr: &str) -> Result<Self, String> {
        let sock = UdpSocket::bind(addr).map_err(|e| format!("绑定 {addr} 失败: {e}"))?;
        sock.set_read_timeout(Some(Duration::from_millis(200)))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            sock: Arc::new(sock),
            psk: None,
            limits: UdpLimits::default(),
            sessions: HashMap::new(),
            by_addr: HashMap::new(),
            stats: Arc::new(UdpStatsInner::default()),
            running: Arc::new(AtomicBool::new(true)),
            last_reap: Instant::now(),
            seed: now_ms(),
        })
    }

    pub fn with_psk(mut self, psk: Option<[u8; PSK_LEN]>) -> Self {
        self.psk = psk;
        self
    }

    pub fn with_limits(mut self, limits: UdpLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn local_addr(&self) -> Result<SocketAddr, String> {
        self.sock.local_addr().map_err(|e| e.to_string())
    }

    fn send_counter(&mut self, id: &[u8; NODE_ID_LEN]) -> u64 {
        match self.sessions.get_mut(id) {
            Some(s) => {
                s.out_seq = s.out_seq.wrapping_add(1);
                s.out_seq
            }
            None => {
                self.seed = self.seed.wrapping_add(1);
                self.seed
            }
        }
    }

    fn reap(&mut self) {
        if self.last_reap.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_reap = Instant::now();
        let ttl = self.limits.idle_timeout;
        let mut dead: Vec<[u8; NODE_ID_LEN]> = Vec::new();
        for (id, s) in self.sessions.iter() {
            if s.last_seen.elapsed() > ttl {
                dead.push(*id);
            }
        }
        for id in dead {
            if let Some(s) = self.sessions.remove(&id) {
                self.by_addr.remove(&s.addr);
                self.stats.sessions_expired.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.stats
            .sessions_active
            .store(self.sessions.len() as u64, Ordering::Relaxed);
    }

    /// 处理一个报文，返回需要发出的 `(目标地址, 字节)` 列表。
    ///
    /// **不在这里发包**：让服务端逻辑可以被单测完整驱动（包括「不该有回包」这种断言）。
    pub fn process(&mut self, buf: &[u8], from: SocketAddr) -> Vec<(SocketAddr, Vec<u8>)> {
        let s = self.stats.clone();
        s.datagrams.fetch_add(1, Ordering::Relaxed);
        s.bytes_in
            .fetch_add(buf.len() as u64, Ordering::Relaxed);
        self.reap();

        let dg = match parse_datagram(buf, self.psk.as_ref(), self.limits.max_payload) {
            Ok(v) => v,
            Err(e) => {
                match e {
                    UdpParseError::Mac => s.mac_fail.fetch_add(1, Ordering::Relaxed),
                    UdpParseError::Magic => s.magic_mismatch.fetch_add(1, Ordering::Relaxed),
                    UdpParseError::TooBig => s.too_big.fetch_add(1, Ordering::Relaxed),
                    UdpParseError::Format => s.bad_format.fetch_add(1, Ordering::Relaxed),
                };
                return Vec::new();
            }
        };

        // HELLO 是唯一能创建会话的帧。
        if dg.kind == UdpKind::Hello {
            return self.on_hello(from, &dg);
        }

        let Some(cur_addr) = self.sessions.get(&dg.session).map(|x| x.addr) else {
            // 未注册：明确告诉它「先 HELLO」，并计 not_registered。
            // 注意：**不能**顺手替它建会话——那等于允许任意来源注入注册表。
            s.not_registered.fetch_add(1, Ordering::Relaxed);
            let seq = self.send_counter(&dg.session);
            let out = encode_datagram(
                UdpKind::Reject,
                &dg.session,
                seq,
                b"not registered: send HELLO first",
                self.psk.as_ref(),
            );
            return self.emit(from, out);
        };

        // 漫游：同 id 新地址。MAC 已经证明身份，换绑即可。
        if cur_addr != from {
            self.by_addr.remove(&cur_addr);
            if let Some(m) = self.sessions.get_mut(&dg.session) {
                m.addr = from;
            }
            self.by_addr.insert(from, dg.session);
            s.rebound.fetch_add(1, Ordering::Relaxed);
        }

        let pps = self.limits.rate_pps;
        let (fresh, allowed) = {
            let sess = self.sessions.get_mut(&dg.session).expect("上面刚确认存在");
            sess.last_seen = Instant::now();
            (sess.accept_seq(dg.seq), sess.allow(pps))
        };
        if !fresh {
            s.replay.fetch_add(1, Ordering::Relaxed);
            return Vec::new();
        }
        if !allowed {
            s.rate_limited.fetch_add(1, Ordering::Relaxed);
            let seq = self.send_counter(&dg.session);
            let mut payload = Vec::with_capacity(9);
            payload.push(UdpStatus::RateLimited.code());
            payload.extend_from_slice(&dg.seq.to_be_bytes());
            let out = encode_datagram(
                UdpKind::Status,
                &dg.session,
                seq,
                &payload,
                self.psk.as_ref(),
            );
            s.status_out.fetch_add(1, Ordering::Relaxed);
            return self.emit(from, out);
        }

        match dg.kind {
            UdpKind::Ping => {
                let mut payload = Vec::with_capacity(16);
                let token = dg.payload.get(..8).unwrap_or(&[0u8; 8]);
                payload.extend_from_slice(token);
                payload.extend_from_slice(&now_ms().to_be_bytes());
                let seq = self.send_counter(&dg.session);
                let out =
                    encode_datagram(UdpKind::Pong, &dg.session, seq, &payload, self.psk.as_ref());
                s.pong_out.fetch_add(1, Ordering::Relaxed);
                self.emit(from, out)
            }
            UdpKind::Data => {
                s.data_in.fetch_add(1, Ordering::Relaxed);
                if dg.payload.len() < NODE_ID_LEN {
                    s.bad_format.fetch_add(1, Ordering::Relaxed);
                    return Vec::new();
                }
                let mut target = [0u8; NODE_ID_LEN];
                target.copy_from_slice(&dg.payload[..NODE_ID_LEN]);
                let body = &dg.payload[NODE_ID_LEN..];
                let mut out: Vec<(SocketAddr, Vec<u8>)> = Vec::with_capacity(2);
                let target_addr = self.sessions.get(&target).map(|t| t.addr);
                let status = match target_addr {
                    Some(taddr) => {
                        let mut dpayload = Vec::with_capacity(NODE_ID_LEN + body.len());
                        dpayload.extend_from_slice(&dg.session);
                        dpayload.extend_from_slice(body);
                        let seq = self.send_counter(&target);
                        let d = encode_datagram(
                            UdpKind::Deliver,
                            &target,
                            seq,
                            &dpayload,
                            self.psk.as_ref(),
                        );
                        s.bytes_out
                            .fetch_add(d.len() as u64, Ordering::Relaxed);
                        out.push((taddr, d));
                        s.deliver_out.fetch_add(1, Ordering::Relaxed);
                        UdpStatus::Forwarded
                    }
                    None => {
                        s.target_absent.fetch_add(1, Ordering::Relaxed);
                        UdpStatus::TargetAbsent
                    }
                };
                let seq = self.send_counter(&dg.session);
                let mut sp = Vec::with_capacity(9);
                sp.push(status.code());
                sp.extend_from_slice(&dg.seq.to_be_bytes());
                let st = encode_datagram(UdpKind::Status, &dg.session, seq, &sp, self.psk.as_ref());
                s.status_out.fetch_add(1, Ordering::Relaxed);
                out.push((from, st));
                out
            }
            UdpKind::Bye => {
                self.sessions.remove(&dg.session);
                self.by_addr.remove(&from);
                s.bye.fetch_add(1, Ordering::Relaxed);
                s.sessions_active
                    .store(self.sessions.len() as u64, Ordering::Relaxed);
                Vec::new()
            }
            // 客户端不该发这些；计 bad_format 并静默，避免形成「发什么都有回包」的探针。
            _ => {
                s.bad_format.fetch_add(1, Ordering::Relaxed);
                Vec::new()
            }
        }
    }

    fn on_hello(
        &mut self,
        from: SocketAddr,
        dg: &UdpDatagram,
    ) -> Vec<(SocketAddr, Vec<u8>)> {
        let s = self.stats.clone();
        s.hello.fetch_add(1, Ordering::Relaxed);
        let id = dg.session;

        // 已有会话：刷新（含漫游），不算新会话。
        if let Some(existing) = self.sessions.get_mut(&id) {
            if existing.addr != from {
                let old = existing.addr;
                existing.addr = from;
                self.by_addr.remove(&old);
                self.by_addr.insert(from, id);
                s.rebound.fetch_add(1, Ordering::Relaxed);
            }
            existing.last_seen = Instant::now();
            existing.accept_seq(dg.seq);
            let seq = self.send_counter(&id);
            let payload = now_ms().to_be_bytes();
            let out = encode_datagram(UdpKind::HelloAck, &id, seq, &payload, self.psk.as_ref());
            s.hello_ack.fetch_add(1, Ordering::Relaxed);
            return self.emit(from, out);
        }

        let per_ip = self
            .sessions
            .values()
            .filter(|x| x.addr.ip() == from.ip())
            .count();
        let reason: Option<String> = if self.sessions.len() >= self.limits.max_sessions {
            Some(format!("会话表满（{}）", self.limits.max_sessions))
        } else if per_ip >= self.limits.max_per_ip {
            Some(format!("单 IP 会话过多（{}）", self.limits.max_per_ip))
        } else {
            None
        };
        if let Some(why) = reason {
            s.sessions_rejected.fetch_add(1, Ordering::Relaxed);
            s.reject_out.fetch_add(1, Ordering::Relaxed);
            let out = encode_datagram(
                UdpKind::Reject,
                &id,
                now_ms(),
                why.as_bytes(),
                self.psk.as_ref(),
            );
            return self.emit(from, out);
        }

        self.sessions.insert(id, Session::new(from, dg.seq));
        self.by_addr.insert(from, id);
        s.sessions_active
            .store(self.sessions.len() as u64, Ordering::Relaxed);
        let seq = self.send_counter(&id);
        let out = encode_datagram(
            UdpKind::HelloAck,
            &id,
            seq,
            &now_ms().to_be_bytes(),
            self.psk.as_ref(),
        );
        s.hello_ack.fetch_add(1, Ordering::Relaxed);
        self.emit(from, out)
    }

    fn emit(&self, to: SocketAddr, bytes: Vec<u8>) -> Vec<(SocketAddr, Vec<u8>)> {
        self.stats
            .bytes_out
            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
        vec![(to, bytes)]
    }

    pub fn spawn(self) -> UdpRelayHandle {
        let addr = self.sock.local_addr().expect("已绑定的 socket 必然有本地地址");
        let stats = self.stats.clone();
        let running = self.running.clone();
        let sock = self.sock.clone();
        let psk = self.psk;
        let mut server = self;
        thread::spawn(move || {
            let mut buf = vec![0u8; 4096];
            while server.running.load(Ordering::Relaxed) {
                match server.sock.recv_from(&mut buf) {
                    Ok((n, from)) => {
                        let outs = server.process(&buf[..n], from);
                        for (to, bytes) in outs {
                            let _ = server.sock.send_to(&bytes, to);
                        }
                    }
                    Err(e)
                        if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                    {
                        continue
                    }
                    Err(_) => break,
                }
            }
        });
        UdpRelayHandle {
            addr,
            stats,
            running,
            sock,
            psk,
        }
    }
}

/// 客户端：一个 socket 对应一个节点 id。
pub struct UdpRelayClient {
    sock: UdpSocket,
    server: SocketAddr,
    psk: Option<[u8; PSK_LEN]>,
    seq: u64,
    pub id: [u8; NODE_ID_LEN],
    pub sent: u64,
    pub received: u64,
    pub dropped_by_mac: u64,
}

/// 客户端收到的一帧（MAC 已验证）。
#[derive(Debug, Clone)]
pub struct UdpFrame {
    pub kind: UdpKind,
    pub from: [u8; NODE_ID_LEN],
    pub payload: Vec<u8>,
}

impl UdpRelayClient {
    pub fn connect(server: &str, timeout: Duration) -> Result<Self, String> {
        let addr = server
            .to_socket_addrs()
            .map_err(|e| format!("dns: {e}"))?
            .next()
            .ok_or_else(|| "中继地址无解析结果".to_string())?;
        let bind = if addr.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
        let sock = UdpSocket::bind(bind).map_err(|e| format!("bind {bind}: {e}"))?;
        sock.connect(addr).map_err(|e| format!("connect: {e}"))?;
        sock.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
        Ok(Self {
            sock,
            server: addr,
            psk: crate::relay::default_psk(),
            seq: now_ms().wrapping_mul(4096),
            id: [0u8; NODE_ID_LEN],
            sent: 0,
            received: 0,
            dropped_by_mac: 0,
        })
    }

    pub fn with_psk(mut self, psk: Option<[u8; PSK_LEN]>) -> Self {
        self.psk = psk;
        self
    }

    pub fn server_addr(&self) -> SocketAddr {
        self.server
    }

    pub fn local_addr(&self) -> Result<SocketAddr, String> {
        self.sock.local_addr().map_err(|e| e.to_string())
    }

    pub fn set_id(&mut self, id: [u8; NODE_ID_LEN]) {
        self.id = id;
    }

    fn send(&mut self, kind: UdpKind, payload: &[u8]) -> Result<u64, String> {
        self.seq = self.seq.wrapping_add(1);
        let dg = encode_datagram(kind, &self.id, self.seq, payload, self.psk.as_ref());
        self.sock
            .send(&dg)
            .map_err(|e| format!("send: {e}"))?;
        self.sent += 1;
        Ok(self.seq)
    }

    /// 注册。**必须**等到 `HelloAck` 或带原因的 `Reject`，否则调用方分不清
    /// 「服务端不在」「密钥不对」「被配额挡了」。
    pub fn hello(&mut self, id: [u8; NODE_ID_LEN], timeout: Duration) -> Result<(), String> {
        self.id = id;
        self.send(UdpKind::Hello, &[])?;
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("HELLO 无应答（超时）".into());
            }
            match self.recv_frame(left)? {
                Some(f) => match f.kind {
                    UdpKind::HelloAck => return Ok(()),
                    UdpKind::Reject => {
                        return Err(format!(
                            "中继拒绝注册: {}",
                            String::from_utf8_lossy(&f.payload)
                        ))
                    }
                    _ => continue,
                },
                None => continue,
            }
        }
    }

    pub fn send_data(&mut self, target: &[u8; NODE_ID_LEN], data: &[u8]) -> Result<u64, String> {
        if data.len() > UDP_MAX_PAYLOAD - NODE_ID_LEN {
            return Err(format!(
                "负载过大: {} > {}",
                data.len(),
                UDP_MAX_PAYLOAD - NODE_ID_LEN
            ));
        }
        let mut payload = Vec::with_capacity(NODE_ID_LEN + data.len());
        payload.extend_from_slice(target);
        payload.extend_from_slice(data);
        self.send(UdpKind::Data, &payload)
    }

    /// 一次 PING/PONG，返回微秒级往返。RTT 只用本进程单调时钟，因此**不需要**两端对时。
    pub fn ping(&mut self, timeout: Duration) -> Result<u128, String> {
        let token = self.seq.wrapping_add(1);
        let t0 = Instant::now();
        self.send(UdpKind::Ping, &token.to_be_bytes())?;
        let deadline = t0 + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("PING 超时".into());
            }
            match self.recv_frame(left)? {
                Some(f) if f.kind == UdpKind::Pong => {
                    let got = f
                        .payload
                        .get(..8)
                        .map(|s| u64::from_be_bytes(s.try_into().unwrap()))
                        .unwrap_or(0);
                    if got == token {
                        return Ok(t0.elapsed().as_micros());
                    }
                    // 迟到的旧 PONG：不是这一条的，继续等。
                    continue;
                }
                Some(_) => continue,
                None => continue,
            }
        }
    }

    pub fn bye(&mut self) -> Result<(), String> {
        self.send(UdpKind::Bye, &[]).map(|_| ())
    }

    /// 收一帧；读超时返回 `Ok(None)`（与 TCP 侧 `recv_frame_optional` 同语义）。
    pub fn recv_frame(&mut self, timeout: Duration) -> Result<Option<UdpFrame>, String> {
        self.sock
            .set_read_timeout(Some(timeout))
            .map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; UDP_MIN_DGRAM + UDP_MAX_PAYLOAD + 64];
        loop {
            match self.sock.recv(&mut buf) {
                Ok(n) => {
                    match parse_datagram(&buf[..n], self.psk.as_ref(), UDP_MAX_PAYLOAD) {
                        Ok(dg) => {
                            self.received += 1;
                            // `DELIVER` 的实体是「来源 id + 消息」，来源在负载前 16 字节
                            // （与 TCP 侧 `DELIVERED.node` 同语义）。头里的 session 是
                            // **收件人**，用来让客户端确认这帧确实发给自己的会话。
                            let (from, payload) =
                                if dg.kind == UdpKind::Deliver && dg.payload.len() >= NODE_ID_LEN {
                                    let mut src = [0u8; NODE_ID_LEN];
                                    src.copy_from_slice(&dg.payload[..NODE_ID_LEN]);
                                    (src, dg.payload[NODE_ID_LEN..].to_vec())
                                } else {
                                    (dg.session, dg.payload.clone())
                                };
                            return Ok(Some(UdpFrame {
                                kind: dg.kind,
                                from,
                                payload,
                            }));
                        }
                        Err(UdpParseError::Mac) => {
                            // 密钥不对或被伪造：计一次，继续等真包（不把 UDP 的
                            // 单个坏包升级成整条链路的错误）。
                            self.dropped_by_mac += 1;
                            continue;
                        }
                        Err(e) => {
                            return Err(format!("报文无法解析: {}", e.as_str()));
                        }
                    }
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    return Ok(None)
                }
                Err(e) => return Err(format!("recv: {e}")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(tag: u8) -> [u8; NODE_ID_LEN] {
        let mut v = [0u8; NODE_ID_LEN];
        v[0] = tag;
        v[15] = 0x5a;
        v
    }

    fn start(psk: Option<[u8; PSK_LEN]>, limits: Option<UdpLimits>) -> UdpRelayHandle {
        let srv = UdpRelayServer::bind("127.0.0.1:0").unwrap();
        let srv = match limits {
            Some(l) => srv.with_limits(l),
            None => srv,
        };
        srv.with_psk(psk).spawn()
    }

    fn client(h: &UdpRelayHandle, psk: Option<[u8; PSK_LEN]>) -> UdpRelayClient {
        UdpRelayClient::connect(&h.addr.to_string(), Duration::from_millis(500))
            .unwrap()
            .with_psk(psk)
    }

    #[test]
    fn datagram_roundtrip_and_mac() {
        let psk = [9u8; PSK_LEN];
        let dg = encode_datagram(UdpKind::Data, &ids(1), 42, b"abc", Some(&psk));
        let p = parse_datagram(&dg, Some(&psk), UDP_MAX_PAYLOAD).unwrap();
        assert_eq!(p.kind, UdpKind::Data);
        assert_eq!(p.session, ids(1));
        assert_eq!(p.seq, 42);
        assert_eq!(p.payload, b"abc");
        assert_eq!(
            parse_datagram(&dg, Some(&[8u8; PSK_LEN]), UDP_MAX_PAYLOAD).unwrap_err(),
            UdpParseError::Mac
        );
        // 改一个字节也要被 MAC 抓到
        let mut bad = dg.clone();
        bad[UDP_MIN_DGRAM] ^= 1;
        assert_eq!(
            parse_datagram(&bad, Some(&psk), UDP_MAX_PAYLOAD).unwrap_err(),
            UdpParseError::Mac
        );
    }

    #[test]
    fn hello_then_ping_measures_rtt() {
        let psk = [3u8; PSK_LEN];
        let h = start(Some(psk), None);
        let mut c = client(&h, Some(psk));
        c.hello(ids(1), Duration::from_millis(500)).unwrap();
        for _ in 0..5 {
            let rtt = c.ping(Duration::from_millis(500)).unwrap();
            assert!(rtt < 500_000, "回环 RTT 不该超过超时: {rtt}us");
        }
        assert_eq!(h.stats().pong_out, 5);
        h.stop();
    }

    #[test]
    fn data_is_forwarded_between_two_clients() {
        let psk = [4u8; PSK_LEN];
        let h = start(Some(psk), None);
        let mut a = client(&h, Some(psk));
        let mut b = client(&h, Some(psk));
        a.hello(ids(0xa1), Duration::from_millis(500)).unwrap();
        b.hello(ids(0xb2), Duration::from_millis(500)).unwrap();
        a.send_data(&ids(0xb2), b"payload-here").unwrap();
        // B 收到 DELIVER：来源 id 是 A，负载原样。
        let mut got = None;
        for _ in 0..5 {
            if let Some(f) = b.recv_frame(Duration::from_millis(500)).unwrap() {
                got = Some(f);
                break;
            }
        }
        let f = got.expect("B 应收到转发帧");
        assert_eq!(f.kind, UdpKind::Deliver);
        assert_eq!(f.from, ids(0xa1));
        assert_eq!(f.payload, b"payload-here");
        // A 收到 STATUS=forwarded
        let s = a
            .recv_frame(Duration::from_millis(500))
            .unwrap()
            .expect("A 应收到 STATUS");
        assert_eq!(s.kind, UdpKind::Status);
        assert_eq!(UdpStatus::from_code(s.payload[0]), Some(UdpStatus::Forwarded));
        h.stop();
    }

    #[test]
    fn unknown_target_yields_target_absent_not_silence() {
        let psk = [5u8; PSK_LEN];
        let h = start(Some(psk), None);
        let mut a = client(&h, Some(psk));
        a.hello(ids(0xa2), Duration::from_millis(500)).unwrap();
        a.send_data(&ids(0xde), b"x").unwrap();
        let s = a
            .recv_frame(Duration::from_millis(500))
            .unwrap()
            .expect("应收到 STATUS");
        assert_eq!(UdpStatus::from_code(s.payload[0]), Some(UdpStatus::TargetAbsent));
        h.stop();
    }

    #[test]
    fn data_without_hello_is_rejected_with_a_reason() {
        let psk = [6u8; PSK_LEN];
        let h = start(Some(psk), None);
        let mut a = client(&h, Some(psk));
        // 故意不 HELLO
        a.set_id(ids(0xa3));
        a.send_data(&ids(0xb4), b"x").unwrap();
        let f = a
            .recv_frame(Duration::from_millis(500))
            .unwrap()
            .expect("应收到 REJECT");
        assert_eq!(f.kind, UdpKind::Reject);
        assert!(
            !f.payload.is_empty(),
            "拒绝必须带原因，否则运维会把配额问题当网络故障"
        );
        h.stop();
    }

    #[test]
    fn replayed_datagram_is_dropped() {
        let psk = [7u8; PSK_LEN];
        let h = start(Some(psk), None);
        let mut a = client(&h, Some(psk));
        a.hello(ids(0xa5), Duration::from_millis(500)).unwrap();
        // 手工构造一条 PING 并原样发两次（同一 seq）→ 第二次必须被滑窗丢掉。
        // seq 必须大于该会话已见过的最大值：客户端的起始 seq 是 `now_ms()*4096` 量级，
        // 随手写个 1_000_000 会被当成「太旧」直接丢，测试就会误判成「服务端不回包」。
        let seq = 9_000_000_000_000_000_000u64;
        let dg = encode_datagram(UdpKind::Ping, &ids(0xa5), seq, &0u64.to_be_bytes(), Some(&psk));
        let sock = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        sock.send_to(&dg, h.addr).unwrap();
        sock.send_to(&dg, h.addr).unwrap();
        sock.set_read_timeout(Some(Duration::from_millis(400))).unwrap();
        let mut buf = [0u8; 2048];
        // 未 connect 的 socket 必须用 recv_from（Windows 上 recv 会直接报错，
        // 那会把「服务端没回包」的断言变成假通过/假失败）。
        assert!(sock.recv_from(&mut buf).is_ok(), "第一条应被应答");
        // 第二条不该有任何回包。用一次读超时既做断言，也顺便把时间让给服务端处理。
        sock.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        let mut buf2 = [0u8; 2048];
        assert!(
            sock.recv_from(&mut buf2).is_err(),
            "重复报文不该产生第二个 PONG"
        );
        let deadline = Instant::now() + Duration::from_millis(500);
        while h.stats().replay == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let stats = h.stats();
        assert!(stats.replay >= 1, "重复报文应计 replay，实际 {:?}", stats);
        h.stop();
    }

    #[test]
    fn oversized_payload_is_counted_and_dropped() {
        let psk = [8u8; PSK_LEN];
        let h = start(Some(psk), Some(UdpLimits { max_payload: 64, ..Default::default() }));
        let sock = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let dg = encode_datagram(UdpKind::Hello, &ids(1), 1, &[0u8; 100], Some(&psk));
        sock.send_to(&dg, h.addr).unwrap();
        sock.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        let mut buf = [0u8; 2048];
        assert!(sock.recv(&mut buf).is_err(), "超限报文不应有回包");
        assert_eq!(h.stats().too_big, 1);
        h.stop();
    }

    #[test]
    fn session_cap_rejects_with_reason() {
        let psk = [2u8; PSK_LEN];
        let limits = UdpLimits { max_per_ip: 1, ..Default::default() };
        let h = start(Some(psk), Some(limits));
        let mut a = client(&h, Some(psk));
        a.hello(ids(0xc1), Duration::from_millis(500)).unwrap();
        let mut b = client(&h, Some(psk));
        let err = b.hello(ids(0xc2), Duration::from_millis(500)).unwrap_err();
        assert!(err.contains("单 IP"), "拒绝原因要指明配额，实际: {err}");
        h.stop();
    }

    #[test]
    fn mac_mismatch_gets_no_answer() {
        let psk = [1u8; PSK_LEN];
        let h = start(Some(psk), None);
        let mut c = client(&h, Some([0u8; PSK_LEN]));
        assert!(c.hello(ids(0xd1), Duration::from_millis(300)).is_err());
        assert!(h.stats().mac_fail >= 1);
        h.stop();
    }
}
