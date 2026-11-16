//! W3.9 中继传输安全层：PSK 双向认证 + 逐帧加密 + 重放防护（std-only）。
//!
//! # 为什么不是 TLS
//!
//! 本仓库是 **std-only 零第三方依赖**，引入 TLS 需要外部 crate 与证书体系。W3.9 的目标
//! 是让中继能放到公网而不被随意注册、窃听与重放，因此采用**预共享密钥（PSK）**方案：
//!
//! - 双方共享 32 字节密钥（`--auth-key` / 环境变量 / 文件），启动时打印**指纹**便于人工核对；
//! - 握手是双向挑战应答：客户端发 `nonce_c` 并附 `HMAC(psk, "client-hello"||nonce_c)`，
//!   服务端校验后回 `nonce_s` 并附 `HMAC(psk, "server-hello"||nonce_c||nonce_s)`；
//! - 会话密钥由 `HMAC(psk, "master"||nonce_c||nonce_s)` 派生，再分方向派生加密键与 MAC 键；
//! - 每条记录独立加密与认证（encrypt-then-MAC），序号严格递增，重复/乱序记录直接断开。
//!
//! **诚实边界**：这不是 TLS。没有证书与域名校验、没有前向保密（PSK 泄露 = 历史流量可解）、
//! 没有密钥轮换；加密原语是 HMAC-SHA256 计数器模式密钥流（PRF-CTR），强度依赖 HMAC-SHA256
//! 的伪随机性，**未经独立密码学审计**。它能挡住：无凭证注册、被动窃听、记录篡改、重放；
//! 它挡不住：持有 PSK 的内部人、流量分析、DoS（另有速率与连接数上限，但只做尽力而为）。
//!
//! # 记录格式（大端）
//!
//! ```text
//! 1B 类型 (0x00 DATA / 0x01 PING / 0x02 PONG)
//! 4B 密文长度
//! nB 密文 = 明文 XOR 密钥流(in_enc, seq, 块序号)
//! 32B tag = HMAC(in_mac, seq || 类型 || 长度 || 密文)
//! ```
//!
//! 明文是 `RelayFrame::encode()` 的字节（1B kind + 16B node + 4B len + 负载）。
//! `PING` / `PONG` 是控制记录，由读侧自动消费：服务端空闲时发 `PING`，对端回 `PONG`，
//! 因此「连接还在但进程已死」能在 `idle_timeout` 内被识别，订阅连接也不需要业务保活。

use crate::relay::{RelayFrame, FRAME_HEADER_LEN};
use cr_hash::{hex, hmac_sha256, sha256};
use std::fmt;
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// PSK 长度（字节）。也接受 64 字符十六进制文本。
pub const PSK_LEN: usize = 32;
/// 控制记录类型。
const RT_DATA: u8 = 0x00;
const RT_PING: u8 = 0x01;
const RT_PONG: u8 = 0x02;
/// 握手魔数，用于在明文误连时立刻报错而不是死等。
const HS_MAGIC: &[u8; 4] = b"CRRL";
const HS_VER: u8 = 2;
/// 客户端 hello = 4 魔数 + 1 版本 + 32 nonce + 32 tag
pub const CLIENT_HELLO_LEN: usize = 4 + 1 + 32 + 32;
/// 服务端 hello = 1 版本 + 32 nonce + 32 tag
pub const SERVER_HELLO_LEN: usize = 1 + 32 + 32;
/// 单条记录固定开销：1 类型 + 4 长度 + 32 tag
pub const RECORD_OVERHEAD: usize = 1 + 4 + 32;

/// 读侧错误：调用方需要区分「空闲超时」与「链路/协议失败」——前者可以发心跳，
/// 后者必须收尾。把两者压成一个字符串，会让空闲超时被当成断链，订阅连接被误杀。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecvError {
    /// 读超时（对端空闲，链路可能还是好的）
    Timeout(String),
    /// 对端关闭或链路错误
    Io(String),
    /// 协议违规（tag 失败 / 重放 / 非法记录 / 帧不可解析）
    Protocol(String),
}

impl RecvError {
    pub fn is_timeout(&self) -> bool {
        matches!(self, RecvError::Timeout(_))
    }
    pub fn detail(&self) -> &str {
        match self {
            RecvError::Timeout(s) | RecvError::Io(s) | RecvError::Protocol(s) => s,
        }
    }
}

impl fmt::Display for RecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tag = match self {
            RecvError::Timeout(_) => "空闲超时",
            RecvError::Io(_) => "链路错误",
            RecvError::Protocol(_) => "协议违规",
        };
        write!(f, "{}: {}", tag, self.detail())
    }
}

/// 安全层累计计数，便于报告「被拒的到底是密钥错、篡改还是重放」。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SecureStats {
    pub handshake_ok: u64,
    pub handshake_failed: u64,
    pub bad_tag: u64,
    pub replay: u64,
    pub ping_sent: u64,
    pub pong_sent: u64,
    pub records_in: u64,
    pub records_out: u64,
}

/// 由 `SecureReader` / `SecureWriter` 共享的计数与诊断口令。
#[derive(Debug, Default)]
pub struct SecureCounters {
    pub handshake_ok: AtomicU64,
    pub handshake_failed: AtomicU64,
    pub bad_tag: AtomicU64,
    pub replay: AtomicU64,
    pub ping_sent: AtomicU64,
    pub pong_sent: AtomicU64,
    pub records_in: AtomicU64,
    pub records_out: AtomicU64,
}

impl SecureCounters {
    pub fn snapshot(&self) -> SecureStats {
        SecureStats {
            handshake_ok: self.handshake_ok.load(Ordering::Relaxed),
            handshake_failed: self.handshake_failed.load(Ordering::Relaxed),
            bad_tag: self.bad_tag.load(Ordering::Relaxed),
            replay: self.replay.load(Ordering::Relaxed),
            ping_sent: self.ping_sent.load(Ordering::Relaxed),
            pong_sent: self.pong_sent.load(Ordering::Relaxed),
            records_in: self.records_in.load(Ordering::Relaxed),
            records_out: self.records_out.load(Ordering::Relaxed),
        }
    }
    fn bump(c: &AtomicU64) {
        c.fetch_add(1, Ordering::Relaxed);
    }
}

/// 解析 PSK：64 字符十六进制，或 `file:<path>`（去掉首尾空白与换行）。
pub fn parse_psk(text: &str) -> Result<[u8; PSK_LEN], String> {
    let t = text.trim();
    let body = if let Some(rest) = t.strip_prefix("file:") {
        std::fs::read_to_string(rest.trim())
            .map_err(|e| format!("读取密钥文件 {} 失败: {}", rest.trim(), e))?
    } else if let Some(rest) = t.strip_prefix("hex:") {
        rest.trim().to_string()
    } else {
        t.to_string()
    };
    let body = body.trim();
    if body.len() != PSK_LEN * 2 {
        return Err(format!(
            "PSK 需 {} 字符十六进制（{} 字节），收到 {} 字符",
            PSK_LEN * 2,
            PSK_LEN,
            body.len()
        ));
    }
    let mut out = [0u8; PSK_LEN];
    let bytes = body.as_bytes();
    for i in 0..PSK_LEN {
        let hi = hex_val(bytes[i * 2]).ok_or_else(|| format!("PSK 第 {} 位不是十六进制", i * 2))?;
        let lo = hex_val(bytes[i * 2 + 1])
            .ok_or_else(|| format!("PSK 第 {} 位不是十六进制", i * 2 + 1))?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// PSK 指纹：`sha256(psk)` 的前 8 字节十六进制，只用于**人工核对两端是否配了同一把钥匙**。
pub fn fingerprint(psk: &[u8; PSK_LEN]) -> String {
    hex(&sha256(psk)[..8])
}

/// 生成 32 字节随机量（nonce）。
///
/// std-only 没有系统随机数接口，这里用 `RandomState`（其内部密钥由操作系统熵初始化，
/// Windows/Linux 都由 std 调用系统 CSPRNG）混合时间、pid 与进程内计数。它不是密码学
/// 级别的 RNG 断言，但作为**会话盐**足够：会话密钥仍需 PSK 才能算出，攻击者拿到 nonce
/// 也无从推出密钥；真正要避免的是「两次会话 nonce 相同导致密钥流复用」，这里用
/// 128 位以上的熵混合 + 单调计数保证不会发生。
pub fn random_nonce() -> [u8; 32] {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut acc = Vec::with_capacity(64);
    for round in 0..4u32 {
        let st = RandomState::new();
        let mut h = st.build_hasher();
        h.write_u64(nanos);
        h.write_u64(seq);
        h.write_u32(std::process::id());
        h.write_u32(round);
        acc.extend_from_slice(&h.finish().to_be_bytes());
        acc.extend_from_slice(&h.finish().to_be_bytes());
    }
    sha256(&acc)
}

fn master_key(psk: &[u8; PSK_LEN], nc: &[u8; 32], ns: &[u8; 32]) -> [u8; 32] {
    let mut msg = Vec::with_capacity(16 + 64);
    msg.extend_from_slice(b"CRRLY2 master");
    msg.extend_from_slice(nc);
    msg.extend_from_slice(ns);
    hmac_sha256(psk, &msg)
}

fn dir_key(master: &[u8; 32], dir: &[u8], purpose: &[u8]) -> [u8; 32] {
    let mut msg = Vec::with_capacity(24);
    msg.extend_from_slice(b"CRRLY2 ");
    msg.extend_from_slice(dir);
    msg.push(b' ');
    msg.extend_from_slice(purpose);
    hmac_sha256(master, &msg)
}

/// 密钥流：`HMAC(k, seq || block_index)` 串接，逐块 XOR 到缓冲区。
fn keystream_xor(key: &[u8; 32], seq: u64, buf: &mut [u8]) {
    let mut block = 0u32;
    let mut off = 0usize;
    while off < buf.len() {
        let mut msg = [0u8; 12];
        msg[..8].copy_from_slice(&seq.to_be_bytes());
        msg[8..].copy_from_slice(&block.to_be_bytes());
        let ks = hmac_sha256(key, &msg);
        let n = (buf.len() - off).min(32);
        for i in 0..n {
            buf[off + i] ^= ks[i];
        }
        off += n;
        block += 1;
    }
}

fn record_tag(mac_key: &[u8; 32], seq: u64, rtype: u8, len: usize, ct: &[u8]) -> [u8; 32] {
    let mut msg = Vec::with_capacity(8 + 1 + 4 + ct.len());
    msg.extend_from_slice(&seq.to_be_bytes());
    msg.push(rtype);
    msg.extend_from_slice(&(len as u32).to_be_bytes());
    msg.extend_from_slice(ct);
    hmac_sha256(mac_key, &msg)
}

/// 写侧：持有出方向序号与密钥，`send_frame` 逐条加密。
pub struct SecureWriter {
    stream: TcpStream,
    enc: [u8; 32],
    mac: [u8; 32],
    seq: u64,
    max_frame: usize,
    counters: Arc<SecureCounters>,
}

impl SecureWriter {
    fn write_record(&mut self, rtype: u8, plain: &[u8]) -> Result<(), String> {
        let mut ct = plain.to_vec();
        keystream_xor(&self.enc, self.seq, &mut ct);
        let tag = record_tag(&self.mac, self.seq, rtype, ct.len(), &ct);
        let mut rec = Vec::with_capacity(RECORD_OVERHEAD + ct.len());
        rec.push(rtype);
        rec.extend_from_slice(&(ct.len() as u32).to_be_bytes());
        rec.extend_from_slice(&ct);
        rec.extend_from_slice(&tag);
        self.stream
            .write_all(&rec)
            .and_then(|_| self.stream.flush())
            .map_err(|e| format!("写记录失败: {}", e))?;
        self.seq += 1;
        SecureCounters::bump(&self.counters.records_out);
        Ok(())
    }

    pub fn send_frame(&mut self, frame: &RelayFrame) -> Result<(), String> {
        let plain = frame.encode();
        if plain.len() > self.max_frame + FRAME_HEADER_LEN {
            return Err(format!("帧体超限: {} > {}", plain.len(), self.max_frame));
        }
        self.write_record(RT_DATA, &plain)
    }

    /// 发心跳（控制记录）。失败说明链路已断，调用方据此收尾。
    pub fn send_ping(&mut self) -> Result<(), String> {
        self.write_record(RT_PING, &[])?;
        SecureCounters::bump(&self.counters.ping_sent);
        Ok(())
    }

    fn send_pong(&mut self) -> Result<(), String> {
        self.write_record(RT_PONG, &[])?;
        SecureCounters::bump(&self.counters.pong_sent);
        Ok(())
    }

    pub fn shutdown(&self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// 只编码不发送、也不推进序号：给「篡改 / 重放」用例构造**合法格式但语义非法**的记录。
    #[cfg(test)]
    pub fn encode_record_for_test(&self, rtype: u8, plain: &[u8]) -> Vec<u8> {
        let mut ct = plain.to_vec();
        keystream_xor(&self.enc, self.seq, &mut ct);
        let tag = record_tag(&self.mac, self.seq, rtype, ct.len(), &ct);
        let mut rec = Vec::with_capacity(RECORD_OVERHEAD + ct.len());
        rec.push(rtype);
        rec.extend_from_slice(&(ct.len() as u32).to_be_bytes());
        rec.extend_from_slice(&ct);
        rec.extend_from_slice(&tag);
        rec
    }
}

/// 读侧：严格序号校验；`PING` 自动回 `PONG`，`PONG` 只更新计数。
pub struct SecureReader {
    stream: TcpStream,
    enc: [u8; 32],
    mac: [u8; 32],
    seq: u64,
    max_frame: usize,
    /// 回 PONG 需要写侧；用 `Weak` 避免与 `Arc<Mutex<SecureWriter>>` 形成环。
    reply: Option<Weak<Mutex<SecureWriter>>>,
    counters: Arc<SecureCounters>,
    /// 最近一次成功读到的记录时间（判断空闲）
    last_seen_ms: Arc<AtomicU64>,
}

impl SecureReader {
    pub fn recv(&mut self) -> Result<RelayFrame, RecvError> {
        loop {
            let mut head = [0u8; 5];
            self.stream
                .read_exact(&mut head)
                .map_err(|e| io_err("读记录头失败", e))?;
            let rtype = head[0];
            let len = u32::from_be_bytes([head[1], head[2], head[3], head[4]]) as usize;
            if len > self.max_frame + FRAME_HEADER_LEN {
                return Err(RecvError::Protocol(format!(
                    "记录超限: {} > {}",
                    len, self.max_frame
                )));
            }
            let mut body = vec![0u8; len];
            if len > 0 {
                self.stream
                    .read_exact(&mut body)
                    .map_err(|e| io_err("读记录体失败", e))?;
            }
            let mut tag = [0u8; 32];
            self.stream
                .read_exact(&mut tag)
                .map_err(|e| io_err("读记录 tag 失败", e))?;

            let want = record_tag(&self.mac, self.seq, rtype, len, &body);
            if !ct_eq(&want, &tag) {
                // 序号被消费掉，不再复用：篡改只能让连接报废，不能诱使重同步。
                SecureCounters::bump(&self.counters.bad_tag);
                return Err(RecvError::Protocol(format!(
                    "记录 tag 校验失败（seq={} type={}）",
                    self.seq, rtype
                )));
            }
            let seq = self.seq;
            self.seq += 1;
            self.last_seen_ms.store(now_ms(), Ordering::Relaxed);
            SecureCounters::bump(&self.counters.records_in);

            match rtype {
                RT_DATA => {
                    let mut plain = body;
                    keystream_xor(&self.enc, seq, &mut plain);
                    return RelayFrame::decode(&plain).ok_or_else(|| {
                        RecvError::Protocol("帧解析失败（明文非法）".into())
                    });
                }
                RT_PING => {
                    if let Some(w) = self.reply.as_ref().and_then(|w| w.upgrade()) {
                        // 用 try_lock：读线程绝不能因为别的线程在写而卡住。
                        if let Ok(mut g) = w.try_lock() {
                            let _ = g.send_pong();
                        }
                    }
                    continue;
                }
                RT_PONG => continue,
                other => {
                    return Err(RecvError::Protocol(format!("非法记录类型 {}", other)))
                }
            }
        }
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<(), String> {
        self.stream.set_read_timeout(timeout).map_err(|e| e.to_string())
    }

    pub fn counters(&self) -> &Arc<SecureCounters> {
        &self.counters
    }

    pub fn last_seen_ms(&self) -> u64 {
        self.last_seen_ms.load(Ordering::Relaxed)
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn shutdown(&self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

fn io_err(what: &str, e: std::io::Error) -> RecvError {
    let msg = format!("{}: {}", what, e);
    match e.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => RecvError::Timeout(msg),
        _ => RecvError::Io(msg),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// 握手成功后的一对读写端 + 诊断计数。
pub struct Channel {
    pub reader: SecureReader,
    pub writer: Arc<Mutex<SecureWriter>>,
    pub counters: Arc<SecureCounters>,
}

impl Channel {
    pub fn stats(&self) -> SecureStats {
        self.counters.snapshot()
    }
}

/// 服务端侧握手：读客户端 hello → 校验 → 回 hello → 派生会话密钥。
pub fn server_handshake(
    stream: TcpStream,
    psk: &[u8; PSK_LEN],
    max_frame: usize,
) -> Result<Channel, String> {
    server_handshake_with(stream, psk, max_frame, Arc::new(SecureCounters::default()))
}

/// 与 [`server_handshake`] 相同，但允许调用方持有计数句柄 —— 握手失败时计数也要读得到，
/// 否则「密钥错」在统计上会消失（线上表现为 rejected 为 0，看起来没人试过）。
pub fn server_handshake_with(
    mut stream: TcpStream,
    psk: &[u8; PSK_LEN],
    max_frame: usize,
    counters: Arc<SecureCounters>,
) -> Result<Channel, String> {
    let mut buf = [0u8; CLIENT_HELLO_LEN];
    if let Err(e) = stream.read_exact(&mut buf) {
        // 读不满 69 字节也是握手失败（明文误连、slowloris、半开连接都会走到这里）。
        // 不计数的话，「有人在拿明文探端口」在统计里会消失。
        SecureCounters::bump(&counters.handshake_failed);
        return Err(format!("读客户端 hello 失败: {}", e));
    }
    if &buf[..4] != HS_MAGIC {
        SecureCounters::bump(&counters.handshake_failed);
        return Err("握手魔数不匹配（对端可能未配 PSK，或连到了非中继端口）".into());
    }
    if buf[4] != HS_VER {
        SecureCounters::bump(&counters.handshake_failed);
        return Err(format!("握手版本不支持: {}", buf[4]));
    }
    let mut nc = [0u8; 32];
    nc.copy_from_slice(&buf[5..37]);
    let client_tag = &buf[37..69];
    let mut msg = Vec::with_capacity(24 + 32);
    msg.extend_from_slice(b"CRRLY2 client-hello");
    msg.extend_from_slice(&nc);
    let want = hmac_sha256(psk, &msg);
    if !ct_eq(&want, client_tag) {
        SecureCounters::bump(&counters.handshake_failed);
        return Err("客户端凭证无效（PSK 不匹配）".into());
    }

    let ns = random_nonce();
    let mut msg = Vec::with_capacity(24 + 64);
    msg.extend_from_slice(b"CRRLY2 server-hello");
    msg.extend_from_slice(&nc);
    msg.extend_from_slice(&ns);
    let server_tag = hmac_sha256(psk, &msg);
    let mut out = Vec::with_capacity(SERVER_HELLO_LEN);
    out.push(HS_VER);
    out.extend_from_slice(&ns);
    out.extend_from_slice(&server_tag);
    stream
        .write_all(&out)
        .and_then(|_| stream.flush())
        .map_err(|e| format!("写服务端 hello 失败: {}", e))?;
    SecureCounters::bump(&counters.handshake_ok);

    let master = master_key(psk, &nc, &ns);
    finish_channel(stream, &master, max_frame, counters, false)
}

/// 客户端侧握手：发 hello → 校验服务端 hello → 派生会话密钥。
pub fn client_handshake(
    mut stream: TcpStream,
    psk: &[u8; PSK_LEN],
    max_frame: usize,
) -> Result<Channel, String> {
    let counters = Arc::new(SecureCounters::default());
    let nc = random_nonce();
    let mut msg = Vec::with_capacity(24 + 32);
    msg.extend_from_slice(b"CRRLY2 client-hello");
    msg.extend_from_slice(&nc);
    let tag = hmac_sha256(psk, &msg);
    let mut out = Vec::with_capacity(CLIENT_HELLO_LEN);
    out.extend_from_slice(HS_MAGIC);
    out.push(HS_VER);
    out.extend_from_slice(&nc);
    out.extend_from_slice(&tag);
    stream
        .write_all(&out)
        .and_then(|_| stream.flush())
        .map_err(|e| format!("写客户端 hello 失败: {}", e))?;

    let mut buf = [0u8; SERVER_HELLO_LEN];
    stream
        .read_exact(&mut buf)
        .map_err(|e| format!("读服务端 hello 失败: {}", e))?;
    if buf[0] != HS_VER {
        SecureCounters::bump(&counters.handshake_failed);
        return Err(format!("服务端握手版本不支持: {}", buf[0]));
    }
    let mut ns = [0u8; 32];
    ns.copy_from_slice(&buf[1..33]);
    let server_tag = &buf[33..65];
    let mut msg = Vec::with_capacity(24 + 64);
    msg.extend_from_slice(b"CRRLY2 server-hello");
    msg.extend_from_slice(&nc);
    msg.extend_from_slice(&ns);
    let want = hmac_sha256(psk, &msg);
    if !ct_eq(&want, server_tag) {
        // 服务端未通过认证：可能是 PSK 错（对端也会在第一步拒我们），也可能是中间人。
        SecureCounters::bump(&counters.handshake_failed);
        return Err("服务端凭证无效（PSK 不匹配或存在中间人）".into());
    }
    SecureCounters::bump(&counters.handshake_ok);
    let master = master_key(psk, &nc, &ns);
    finish_channel(stream, &master, max_frame, counters, true)
}

/// 测试专用：握手后额外返回一条裸 TCP 副本，用来发送被篡改/重放的记录。
#[cfg(test)]
pub fn client_handshake_keep_raw(
    stream: TcpStream,
    psk: &[u8; PSK_LEN],
    max_frame: usize,
) -> Result<(Channel, TcpStream), String> {
    let raw = stream.try_clone().map_err(|e| e.to_string())?;
    let ch = client_handshake(stream, psk, max_frame)?;
    Ok((ch, raw))
}

fn finish_channel(
    stream: TcpStream,
    master: &[u8; 32],
    max_frame: usize,
    counters: Arc<SecureCounters>,
    is_client: bool,
) -> Result<Channel, String> {
    // 方向命名以「客户端 → 服务端」为 c2s，双方各自取自己的出/入。
    let c2s_enc = dir_key(master, b"c2s", b"enc");
    let c2s_mac = dir_key(master, b"c2s", b"mac");
    let s2c_enc = dir_key(master, b"s2c", b"enc");
    let s2c_mac = dir_key(master, b"s2c", b"mac");
    let (out_enc, out_mac, in_enc, in_mac) = if is_client {
        (c2s_enc, c2s_mac, s2c_enc, s2c_mac)
    } else {
        (s2c_enc, s2c_mac, c2s_enc, c2s_mac)
    };
    let reader_stream = stream.try_clone().map_err(|e| format!("clone 读端失败: {}", e))?;
    let writer = Arc::new(Mutex::new(SecureWriter {
        stream,
        enc: out_enc,
        mac: out_mac,
        seq: 0,
        max_frame,
        counters: counters.clone(),
    }));
    let reader = SecureReader {
        stream: reader_stream,
        enc: in_enc,
        mac: in_mac,
        seq: 0,
        max_frame,
        reply: Some(Arc::downgrade(&writer)),
        counters: counters.clone(),
        last_seen_ms: Arc::new(AtomicU64::new(now_ms())),
    };
    Ok(Channel {
        reader,
        writer,
        counters,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::NODE_ID_LEN;
    use std::net::TcpListener;
    use std::thread;

    const PSK: [u8; PSK_LEN] = [0x42; PSK_LEN];

    fn frame(payload: &[u8]) -> RelayFrame {
        let mut node = [0u8; NODE_ID_LEN];
        node.copy_from_slice(&[0xAB; NODE_ID_LEN]);
        RelayFrame {
            kind: crate::relay::FrameKind::Relay,
            node,
            payload: payload.to_vec(),
        }
    }

    /// 起一个只服务单连接的「服务端线程」，把 `SecureReader::recv` 的结果交回主线程。
    fn one_shot_server(
        psk: [u8; PSK_LEN],
    ) -> (u16, thread::JoinHandle<(Result<usize, String>, SecureStats)>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let counters = Arc::new(SecureCounters::default());
            let mut ch = match server_handshake_with(s, &psk, 1 << 20, counters.clone()) {
                Ok(c) => c,
                Err(e) => return (Err(e), counters.snapshot()),
            };
            let r = ch.reader.recv().map(|f| f.payload.len()).map_err(|e| e.to_string());
            (r, ch.stats())
        });
        (port, h)
    }

    fn client_conn(port: u16, psk: [u8; PSK_LEN]) -> (Channel, TcpStream) {
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        client_handshake_keep_raw(stream, &psk, 1 << 20).expect("客户端握手应成功")
    }

    #[test]
    fn psk_parse_accepts_hex_and_file_form() {
        let hexs = "0".repeat(63) + "f";
        let p = parse_psk(&hexs).unwrap();
        assert_eq!(p[31], 0x0f);
        assert!(parse_psk("deadbeef").is_err());
        let dir = std::env::temp_dir();
        let path = dir.join("cr_psk_test.txt");
        std::fs::write(&path, hexs.clone() + "\n").unwrap();
        let p2 = parse_psk(&format!("file:{}", path.display())).unwrap();
        assert_eq!(p, p2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fingerprint_is_stable_and_key_dependent() {
        let a = fingerprint(&[0x42; PSK_LEN]);
        assert_eq!(a, fingerprint(&[0x42; PSK_LEN]));
        assert_ne!(a, fingerprint(&[0x43; PSK_LEN]));
        assert_eq!(a.len(), 16);
    }

    /// 握手 + 双向记录往返；服务端计数能证明「一条入、一条出」。
    #[test]
    fn handshake_and_encrypted_roundtrip() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let srv = thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let mut ch = server_handshake(s, &PSK, 1 << 20).unwrap();
            let got = ch.reader.recv().unwrap();
            assert_eq!(got.payload, b"WAKEUP_REQ");
            let mut w = ch.writer.lock().unwrap();
            w.send_frame(&frame(b"WAKEUP_ACK")).unwrap();
            ch.stats()
        });
        let (mut ch, _raw) = client_conn(port, PSK);
        {
            let mut w = ch.writer.lock().unwrap();
            w.send_frame(&frame(b"WAKEUP_REQ")).unwrap();
        }
        let back = ch.reader.recv().unwrap();
        assert_eq!(back.payload, b"WAKEUP_ACK");
        let sstats = srv.join().unwrap();
        assert_eq!(sstats.handshake_ok, 1);
        assert_eq!(sstats.handshake_failed, 0);
        assert_eq!(sstats.bad_tag, 0);
        assert_eq!(sstats.records_in, 1);
        let c = ch.stats();
        assert_eq!(c.handshake_ok, 1);
        assert_eq!(c.records_in, 1);
    }

    /// 线上不得出现明文：抓到的字节里找不到 WAKEUP_REQ / 节点 id。
    #[test]
    fn payload_is_not_readable_on_the_wire() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let srv = thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let mut ch = server_handshake(s, &PSK, 1 << 20).unwrap();
            let f = ch.reader.recv().unwrap();
            f.payload
        });
        let (ch, _raw) = client_conn(port, PSK);
        let plain = frame(b"WAKEUP_REQ-SECRET").encode();
        let rec = {
            let w = ch.writer.lock().unwrap();
            w.encode_record_for_test(RT_DATA, &plain)
        };
        // 密文里不得出现明文片段（帧头 16 字节全 0xAB 也不可能原样出现）。
        assert!(!rec.windows(8).any(|w| w == b"WAKEUP_R"), "密文里出现了明文");
        assert!(!rec.windows(8).any(|w| w == &[0xABu8; 8]), "密文里出现了节点 id");
        assert!(rec.len() > plain.len(), "记录必须带 tag 开销");
        {
            let mut w = ch.writer.lock().unwrap();
            w.send_frame(&frame(b"WAKEUP_REQ-SECRET")).unwrap();
        }
        let got = srv.join().unwrap();
        assert_eq!(got, b"WAKEUP_REQ-SECRET", "对端仍应解密出原文");
    }

    #[test]
    fn wrong_key_is_rejected_at_handshake() {
        let (port, srv) = one_shot_server(PSK);
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let bad = [0x24u8; PSK_LEN];
        let err = client_handshake(stream, &bad, 1 << 20).err().unwrap_or_default();
        // 服务端先行拒绝，客户端要么读 hello 失败、要么校验失败 —— 两种都必须报错。
        assert!(!err.is_empty(), "错误密钥不得建立会话");
        let (recv_res, stats) = srv.join().unwrap();
        assert!(recv_res.is_err(), "服务端不得进入数据阶段");
        assert_eq!(stats.handshake_failed, 1, "应计一次握手失败（{:?}）", stats);
        assert_eq!(stats.handshake_ok, 0);
    }

    /// 篡改密文一个字节：tag 必须失败，并计入 `bad_tag`。
    #[test]
    fn tampered_ciphertext_is_rejected() {
        let (port, srv) = one_shot_server(PSK);
        let (ch, mut raw) = client_conn(port, PSK);
        let mut rec = {
            let w = ch.writer.lock().unwrap();
            w.encode_record_for_test(RT_DATA, &frame(b"tamper").encode())
        };
        // 翻转密文最后一个字节（tag 之前）。
        let pos = rec.len() - 33;
        rec[pos] ^= 0x01;
        raw.write_all(&rec).unwrap();
        raw.flush().unwrap();
        let (res, stats) = srv.join().unwrap();
        assert!(res.is_err(), "被篡改的记录必须被拒");
        assert_eq!(stats.bad_tag, 1, "应计一次 tag 失败（{:?}）", stats);
    }

    /// 同一条记录原样重发：第一条接受，第二条必须按序号不匹配被拒。
    #[test]
    fn replayed_record_is_rejected() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let srv = thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut ch = server_handshake(s, &PSK, 1 << 20).unwrap();
            let first = ch.reader.recv();
            let second = ch.reader.recv();
            (
                first.is_ok(),
                second.err().map(|e| e.to_string()).unwrap_or_default(),
                ch.stats(),
            )
        });
        let (ch, mut raw) = client_conn(port, PSK);
        let rec = {
            let w = ch.writer.lock().unwrap();
            w.encode_record_for_test(RT_DATA, &frame(b"once").encode())
        };
        raw.write_all(&rec).unwrap();
        raw.write_all(&rec).unwrap();
        raw.flush().unwrap();
        let (first_ok, second_err, stats) = srv.join().unwrap();
        assert!(first_ok, "第一条记录应被接受");
        assert!(second_err.contains("tag"), "重放应报 tag 失败，实际: {}", second_err);
        assert_eq!(stats.records_in, 1, "只应有 1 条记录被接受（{:?}）", stats);
        assert_eq!(stats.bad_tag, 1, "重放记录应计 tag 失败（{:?}）", stats);
    }

    /// 未配 PSK 的明文客户端连安全服务端：握手魔数不匹配，必须立刻失败而不是死等。
    #[test]
    fn plaintext_client_fails_fast_against_secure_server() {
        let (port, srv) = one_shot_server(PSK);
        use std::io::Write as _;
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        // 直接发一个明文帧（旧协议格式）。
        let mut raw = vec![0x01u8];
        raw.extend_from_slice(&[0x11; NODE_ID_LEN]);
        raw.extend_from_slice(&(NODE_ID_LEN as u32).to_be_bytes());
        raw.extend_from_slice(&[0x11; NODE_ID_LEN]);
        s.write_all(&raw).unwrap();
        let (res, stats) = srv.join().unwrap();
        assert!(res.is_err());
        assert_eq!(stats.handshake_failed, 1);
    }

    /// 服务端 PING → 客户端读侧自动回 PONG → 服务端 `records_in` 里能看到这条 PONG。
    #[test]
    fn ping_is_answered_with_pong() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let srv = thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut ch = server_handshake(s, &PSK, 1 << 20).unwrap();
            {
                let mut w = ch.writer.lock().unwrap();
                w.send_ping().unwrap();
                w.send_frame(&frame(b"probe")).unwrap();
            }
            // 客户端回 PONG（控制记录，recv 内部消费），随后回一条 DATA。
            let f = ch.reader.recv().unwrap();
            (f.payload, ch.stats())
        });
        let (mut ch, _raw) = client_conn(port, PSK);
        let got = ch.reader.recv().unwrap();
        assert_eq!(got.payload, b"probe", "PING 不得被当成业务帧交出去");
        ch.writer.lock().unwrap().send_frame(&frame(b"ack-probe")).unwrap();
        let (payload, stats) = srv.join().unwrap();
        assert_eq!(payload, b"ack-probe");
        assert_eq!(stats.ping_sent, 1);
        assert_eq!(stats.pong_sent, 0, "服务端没收到 PING，不该发 PONG");
        assert_eq!(stats.records_in, 2, "入方向应有 PONG + DATA 两条（{:?}）", stats);
    }
}
