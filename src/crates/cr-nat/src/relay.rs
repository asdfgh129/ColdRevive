//! 中继传输（LowID 回退），std-only。
//!
//! 当所有端口映射（UPnP/NAT-PMP/PCP）与预测打洞都失败时，退化到经可信中继服务器转发
//! 唤醒/对等消息。本模块实现**客户端侧**的帧协议与连接管理；W3.4 起中继服务端
//! 由本 crate 的 [`crate::relay_server`] 提供（真实帧路由器，与业务无关），
//! 独立部署入口为 `coldrevive-relay` 二进制。
//!
//! 帧格式（大端）：1 字节 kind + 16 字节 node id + 4 字节长度 + 负载。
//!   kind=0x01 REGISTER  负载 = 16 字节节点 id
//!   kind=0x02 RELAY     负载 = 16 字节目标 id + 任意消息
//!   kind=0x03 DELIVERED 负载 = 16 字节来源 id + 任意消息
//!   kind=0x04 ACK       负载为空
//!
//! W3.8 去单点化新增（**只在中继实例之间**使用，客户端不发这三类帧）：
//!   kind=0x05 PEER_HELLO 负载 = 16 字节呼叫方实例 id；对端回 `ACK(node=应答方实例 id)`
//!   kind=0x06 PEER_SYNC  负载 = n × 16 字节本地注册节点 id（整份快照，覆盖语义）
//!   kind=0x07 FORWARD    负载 = 16 字节原始来源 id + 16 字节目标 id + 消息
//!                        （收到后只做本地投递，**不再转发**，因此最多一跳、无环）
//!
//! W3.12 可达性声明（借 eMule 的「登录时报 LowID」）：
//!   kind=0x08 REGISTER_DECLARED 负载 = 16B node id + 1B 类别 + 8B 声明时刻
//!   kind=0x09 PEER_REACH        负载 = n × (16B id + 1B 类别 + 8B 时刻)，实例间整份覆盖
//!   kind=0x0A LOOKUP_REACH      负载 = 16B 目标 id（问「这台机器属于哪一类」）
//!   kind=0x0B REACH_RESULT      负载 = 16B 目标 id + 1B 类别 + 8B 时刻（查不到时后两段为 0）
//!
//! 四类帧都是**纯追加**：旧版中继收到 0x08..0x0B 会走 `Unknown(kind)` 分支并计数拒帧，
//! 不会误解析。代价是**部署顺序必须先升中继、再升客户端**——反过来会让新客户端
//! 在注册阶段就被断开（表现为「中继未回 ACK」）。
//!
//! # W3.9：可选的安全传输
//!
//! 配了 PSK（`--auth-key` / `nat.relay_key`）时，上面的帧在 [`crate::secure`] 的记录层里
//! 加密传输：握手双向认证、逐帧 HMAC、序号严格递增。**未配 PSK 就是明文**——服务端默认
//! 只允许明文绑回环地址，公网部署必须配密钥（见 `coldrevive-relay --allow-insecure`）。

use crate::relay_server::DEFAULT_MAX_FRAME;
use crate::secure::RecvError;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const NODE_ID_LEN: usize = 16;
/// 帧头长度：1B kind + 16B node id + 4B 大端长度。
pub const FRAME_HEADER_LEN: usize = 1 + NODE_ID_LEN + 4;

/// 为一次唤醒请求生成**临时会话 id**（std-only，无 rand 依赖）。
///
/// 持有者订阅线程用稳定 node id 注册，请求侧若也用同一个稳定 id 注册，会与自己的
/// 订阅连接抢占服务端注册表里 `HashMap<node_id, conn>` 的唯一表项。改用一次性会话 id 后：
/// 请求侧注册的 id 唯一；服务端把该 id 作为 `DELIVERED.node`（来源）交给持有者，
/// 持有者据此把真实 WAKEUP_ACK 准确回投到这次请求的 TCP 连接。
/// 明文连接没有 secure 记录层，超时分类要在这里补齐，口径与 [`RecvError`] 一致。
fn classify_plain_read(e: std::io::Error, what: &str) -> RecvError {
    let msg = format!("{what}: {e}");
    match e.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => RecvError::Timeout(msg),
        _ => RecvError::Io(msg),
    }
}

pub fn ephemeral_session_id(self_id: &[u8; NODE_ID_LEN]) -> [u8; NODE_ID_LEN] {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id() as u64;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in self_id
        .iter()
        .chain(nanos.to_be_bytes().iter())
        .chain(seq.to_be_bytes().iter())
        .chain(pid.to_be_bytes().iter())
    {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    let mut out = [0u8; NODE_ID_LEN];
    for (i, chunk) in out.chunks_mut(8).enumerate() {
        let v = h
            .rotate_left((i as u32) * 17)
            .wrapping_add(seq.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let bytes = v.to_be_bytes();
        let n = chunk.len().min(bytes.len());
        chunk[..n].copy_from_slice(&bytes[..n]);
    }
    // 最高位标记为「临时会话」，与稳定 node id（可打印标签）区分，避免任何碰撞。
    out[0] |= 0x80;
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Register,
    Relay,
    Delivered,
    Ack,
    /// 实例间握手：呼叫方声明自己的实例 id
    PeerHello,
    /// 实例间注册表快照同步（整份覆盖）
    PeerSync,
    /// 实例间单跳转发（来源 id 原样带回，不二次转发）
    Forward,
    /// **注册并声明自己的可达性类别**（W3.12）。负载 = 16B node id + 1B 类别 + 8B 声明时刻。
    ///
    /// 这是 eMule「登录时声明自己是 LowID」那一步：类别由节点自己实测得出、自己报给中继，
    /// 而不是让别的节点去猜或试。旧版本中继收到它会走 `Unknown(0x08)` 分支并拒帧
    /// （不解码、不误判），因此**部署顺序必须是先升级中继、再升级客户端**。
    RegisterDeclared,
    /// 实例间同步可达性声明（整份覆盖）。负载 = n × (16B id + 1B 类别 + 8B 时刻)。
    ///
    /// 单独一条帧而不是塞进 `PeerSync`：`PeerSync` 的负载是纯 `n × 16B`，
    /// 改它的语义会让「新旧实例互连」从「解析不了」变成「解析成错的」——后者更难查。
    PeerReach,
    /// 查询某节点的可达性类别。负载 = 16B 目标 id。
    LookupReach,
    /// `LookupReach` 的应答。负载 = 16B 目标 id + 1B 类别 + 8B 声明时刻（查不到时后两段为 0）。
    ReachResult,
    /// **服务端拒绝这次请求**，负载 = UTF-8 原因。发完即断开。
    ///
    /// 存在的理由：中继限额（单 IP 注册频率 / 注册表满 / 同连接改 ID）原来都是
    /// 「不打招呼直接关连接」，客户端只能看到 `读帧头失败: failed to fill whole buffer`，
    /// 分不清是「被配额挡了」还是「链路断了」。运维据此排查会走向完全错误的方向。
    /// 旧客户端收到 0x0c 会当作 `Unknown`，行为与以前一致（拒绝注册），不会误判成成功。
    Reject,
    Unknown(u8),
}

impl FrameKind {
    fn code(&self) -> u8 {
        match self {
            FrameKind::Register => 0x01,
            FrameKind::Relay => 0x02,
            FrameKind::Delivered => 0x03,
            FrameKind::Ack => 0x04,
            FrameKind::PeerHello => 0x05,
            FrameKind::PeerSync => 0x06,
            FrameKind::Forward => 0x07,
            FrameKind::RegisterDeclared => 0x08,
            FrameKind::PeerReach => 0x09,
            FrameKind::LookupReach => 0x0a,
            FrameKind::ReachResult => 0x0b,
            FrameKind::Reject => 0x0c,
            FrameKind::Unknown(c) => *c,
        }
    }
    fn from_code(c: u8) -> FrameKind {
        match c {
            0x01 => FrameKind::Register,
            0x02 => FrameKind::Relay,
            0x03 => FrameKind::Delivered,
            0x04 => FrameKind::Ack,
            0x05 => FrameKind::PeerHello,
            0x06 => FrameKind::PeerSync,
            0x07 => FrameKind::Forward,
            0x08 => FrameKind::RegisterDeclared,
            0x09 => FrameKind::PeerReach,
            0x0a => FrameKind::LookupReach,
            0x0b => FrameKind::ReachResult,
            0x0c => FrameKind::Reject,
            other => FrameKind::Unknown(other),
        }
    }
}

/// 一条可达性声明的线缆长度：16B id + 1B 类别 + 8B 声明时刻。
pub const REACH_RECORD_LEN: usize = NODE_ID_LEN + 1 + 8;

/// 把声明编码成 `REACH_RECORD_LEN` 字节。
pub fn encode_reach_record(
    id: &[u8; NODE_ID_LEN],
    class: crate::reach::ReachClass,
    declared_at_ms: u64,
) -> [u8; REACH_RECORD_LEN] {
    let mut out = [0u8; REACH_RECORD_LEN];
    out[..NODE_ID_LEN].copy_from_slice(id);
    out[NODE_ID_LEN] = class.wire();
    out[NODE_ID_LEN + 1..].copy_from_slice(&declared_at_ms.to_be_bytes());
    out
}

/// 解析一条声明记录。长度不足返回 `None`（调用方按「未知」处理，不猜）。
pub fn decode_reach_record(
    buf: &[u8],
) -> Option<([u8; NODE_ID_LEN], crate::reach::ReachClass, u64)> {
    if buf.len() < REACH_RECORD_LEN {
        return None;
    }
    let mut id = [0u8; NODE_ID_LEN];
    id.copy_from_slice(&buf[..NODE_ID_LEN]);
    let class = crate::reach::ReachClass::from_wire(buf[NODE_ID_LEN]);
    let mut ts = [0u8; 8];
    ts.copy_from_slice(&buf[NODE_ID_LEN + 1..REACH_RECORD_LEN]);
    Some((id, class, u64::from_be_bytes(ts)))
}

#[derive(Debug, Clone)]
pub struct RelayFrame {
    pub kind: FrameKind,
    pub node: [u8; NODE_ID_LEN],
    pub payload: Vec<u8>,
}

impl RelayFrame {
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(1 + NODE_ID_LEN + 4 + self.payload.len());
        v.push(self.kind.code());
        v.extend_from_slice(&self.node);
        v.extend_from_slice(&(self.payload.len() as u32).to_be_bytes());
        v.extend_from_slice(&self.payload);
        v
    }

    pub fn decode(data: &[u8]) -> Option<RelayFrame> {
        if data.len() < 1 + NODE_ID_LEN + 4 {
            return None;
        }
        let kind = FrameKind::from_code(data[0]);
        let mut node = [0u8; NODE_ID_LEN];
        node.copy_from_slice(&data[1..1 + NODE_ID_LEN]);
        let len = u32::from_be_bytes([
            data[1 + NODE_ID_LEN],
            data[2 + NODE_ID_LEN],
            data[3 + NODE_ID_LEN],
            data[4 + NODE_ID_LEN],
        ]) as usize;
        let start = 1 + NODE_ID_LEN + 4;
        if data.len() < start + len {
            return None;
        }
        Some(RelayFrame {
            kind,
            node,
            payload: data[start..start + len].to_vec(),
        })
    }
}

pub struct RelayClient {
    pub addr: String,
    psk: PskChoice,
}

/// 密钥来源：默认跟随进程级配置（`set_default_psk`），也可显式指定（测试用明文）。
#[derive(Debug, Clone, Copy)]
enum PskChoice {
    Default,
    Explicit(Option<[u8; crate::secure::PSK_LEN]>),
}

/// 进程级默认 PSK。由守护进程/CLI 在读配置后设置一次，避免把密钥层层透传到每个调用点。
static DEFAULT_PSK: Mutex<Option<[u8; crate::secure::PSK_LEN]>> = Mutex::new(None);

/// 设置进程级 PSK（`None` = 明文）。
/// 把 `Reject` 帧的负载翻成一句可读错误。
///
/// 空负载也要有话说：只说「被拒绝」而不说原因，等于让运维去猜。
fn reject_reason(frame: &RelayFrame) -> String {
    let reason = String::from_utf8_lossy(&frame.payload);
    if reason.trim().is_empty() {
        "中继拒绝请求（未给原因）".to_string()
    } else {
        format!("中继拒绝请求: {}", reason.trim())
    }
}

pub fn set_default_psk(psk: Option<[u8; crate::secure::PSK_LEN]>) {
    if let Ok(mut g) = DEFAULT_PSK.lock() {
        *g = psk;
    }
}

/// 当前进程级 PSK。
pub fn default_psk() -> Option<[u8; crate::secure::PSK_LEN]> {
    DEFAULT_PSK.lock().ok().and_then(|g| *g)
}

/// 从配置里的 `nat.relay_key`（64 位十六进制或 `file:<路径>`）安装进程级 PSK。
///
/// 返回错误但**不改变**当前设置：配置写错时宁可让调用方打印告警，也不要静默退回明文
/// —— 那会让「本来加密」的部署以为自己在加密。
pub fn set_psk_from_spec(spec: Option<&str>) -> Result<Option<[u8; crate::secure::PSK_LEN]>, String> {
    match spec.map(str::trim).filter(|s| !s.is_empty()) {
        Some(text) => {
            let k = crate::secure::parse_psk(text)?;
            set_default_psk(Some(k));
            Ok(Some(k))
        }
        None => {
            set_default_psk(None);
            Ok(None)
        }
    }
}

impl RelayClient {
    pub fn new(addr: String) -> Self {
        Self {
            addr,
            psk: PskChoice::Default,
        }
    }

    /// 强制明文（仅用于本地 mock 与回环诊断；公网对端会拒绝）。
    pub fn plaintext(addr: String) -> Self {
        Self {
            addr,
            psk: PskChoice::Explicit(None),
        }
    }

    pub fn with_psk(mut self, psk: Option<[u8; crate::secure::PSK_LEN]>) -> Self {
        self.psk = PskChoice::Explicit(psk);
        self
    }

    pub fn resolve_psk(&self) -> Option<[u8; crate::secure::PSK_LEN]> {
        match self.psk {
            PskChoice::Default => default_psk(),
            PskChoice::Explicit(p) => p,
        }
    }

    pub fn connect(&self, timeout: Duration) -> Result<RelayConn, String> {
        let addr = self
            .addr
            .to_socket_addrs()
            .map_err(|e| format!("dns: {}", e))?
            .next()
            .ok_or_else(|| "relay 地址无解析结果".to_string())?;
        let stream = TcpStream::connect(addr).map_err(|e| format!("connect: {}", e))?;
        let _ = stream.set_nodelay(true);
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|e| e.to_string())?;
        match self.resolve_psk() {
            Some(psk) => {
                let ch = crate::secure::client_handshake(stream, &psk, DEFAULT_MAX_FRAME)?;
                Ok(RelayConn {
                    io: ConnIo::Secure {
                        reader: ch.reader,
                        writer: ch.writer,
                        counters: ch.counters,
                    },
                })
            }
            None => Ok(RelayConn {
                io: ConnIo::Plain {
                    reader: stream.try_clone().map_err(|e| e.to_string())?,
                    writer: stream,
                },
            }),
        }
    }
}

/// 单条中继连接的两种形态：明文（回环/测试）与加密（配了 PSK）。
enum ConnIo {
    Plain {
        reader: TcpStream,
        writer: TcpStream,
    },
    Secure {
        reader: crate::secure::SecureReader,
        writer: Arc<Mutex<crate::secure::SecureWriter>>,
        counters: Arc<crate::secure::SecureCounters>,
    },
}

pub struct RelayConn {
    io: ConnIo,
}

impl RelayConn {
    pub fn is_secure(&self) -> bool {
        matches!(self.io, ConnIo::Secure { .. })
    }

    pub fn secure_stats(&self) -> Option<crate::secure::SecureStats> {
        match &self.io {
            ConnIo::Secure { counters, .. } => Some(counters.snapshot()),
            ConnIo::Plain { .. } => None,
        }
    }

    fn send(&mut self, frame: &RelayFrame) -> Result<(), String> {
        match &mut self.io {
            ConnIo::Plain { writer, .. } => {
                writer
                    .write_all(&frame.encode())
                    .and_then(|_| writer.flush())
                    .map_err(|e| e.to_string())
            }
            ConnIo::Secure { writer, .. } => {
                let mut g = writer.lock().map_err(|_| "写锁中毒".to_string())?;
                g.send_frame(frame)
            }
        }
    }

    pub fn register(&mut self, node_id: &[u8; NODE_ID_LEN]) -> Result<(), String> {
        let frame = RelayFrame {
            kind: FrameKind::Register,
            node: *node_id,
            payload: node_id.to_vec(),
        };
        self.send(&frame)?;
        // 等待 ACK（或显式拒绝）
        let resp = self.recv_frame()?;
        if resp.kind == FrameKind::Reject {
            return Err(reject_reason(&resp));
        }
        if resp.kind != FrameKind::Ack {
            return Err("relay 未回 ACK".into());
        }
        Ok(())
    }

    /// 注册并**声明**自己的可达性类别（eMule「登录时报 LowID」那一步）。
    ///
    /// 与 [`Self::register`] 的区别只有两点：帧类型是 `RegisterDeclared`，负载多带
    /// 类别与声明时刻。语义差别则很大——中继从此知道「这台机器不该被直连」，
    /// 别的节点可以查它再决定路径，而不必先试一次注定失败的直连。
    ///
    /// 类别**由节点自己实测得出**（见 [`crate::reach`]），中继只存储与转发，不做判断：
    /// 一个中继没有能力、也不该去替客户端认定它自己的 NAT 行为。
    pub fn register_declared(
        &mut self,
        node_id: &[u8; NODE_ID_LEN],
        decl: &crate::reach::ReachDeclaration,
    ) -> Result<(), String> {
        let mut payload = Vec::with_capacity(REACH_RECORD_LEN);
        payload.extend_from_slice(node_id);
        payload.push(decl.class.wire());
        payload.extend_from_slice(&decl.declared_at_ms.to_be_bytes());
        let frame = RelayFrame {
            kind: FrameKind::RegisterDeclared,
            node: *node_id,
            payload,
        };
        self.send(&frame)?;
        let resp = self.recv_frame()?;
        if resp.kind == FrameKind::Reject {
            return Err(reject_reason(&resp));
        }
        if resp.kind != FrameKind::Ack {
            return Err("relay 未回 ACK（可能是不支持声明帧的旧版中继）".into());
        }
        Ok(())
    }

    /// 查询某节点的可达性类别。
    ///
    /// 返回 `None` = 中继不知道（没声明过、声明早被清理、或不在本实例也不在同侪快照里）。
    /// **`None` 必须按「不知道」处理**：它不等于「连不上」。
    ///
    /// 用途是省掉一次注定超时的直连尝试，**不是**用来拒绝服务——因此调用方拿到
    /// `RelayOnly` 时仍应保留中继这条路，拿到 `None` 时两条路都要试。
    pub fn lookup_reach(
        &mut self,
        target: &[u8; NODE_ID_LEN],
        timeout: Duration,
    ) -> Result<Option<crate::reach::ReachDeclaration>, String> {
        let frame = RelayFrame {
            kind: FrameKind::LookupReach,
            node: *target,
            payload: target.to_vec(),
        };
        self.send(&frame)?;
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return Err("查询可达性超时".into());
            }
            self.set_read_timeout(Some(left))?;
            let resp = self.recv_frame()?;
            if resp.kind != FrameKind::ReachResult {
                // 同一连接上可能先来别的帧（例如 DELIVERED）；只认应答，其余忽略。
                continue;
            }
            let Some((id, class, at)) = decode_reach_record(&resp.payload) else {
                return Err("ReachResult 负载长度非法".into());
            };
            if &id != target {
                continue;
            }
            if at == 0 {
                return Ok(None);
            }
            return Ok(Some(crate::reach::ReachDeclaration {
                class,
                // 查询结果只带类别与时刻：探测时的 NAT 类型不是中继该记的东西，
                // 硬塞一个 Unknown 会让「类别来自对称型」这类溯源信息看起来像缺失。
                nat_class: crate::NatType::Unknown,
                declared_at_ms: at,
                ttl_ms: crate::reach::DEFAULT_TTL_MS,
            }));
        }
    }

    pub fn send_relay(&mut self, target: &[u8; NODE_ID_LEN], message: &[u8]) -> Result<(), String> {
        let mut payload = Vec::with_capacity(NODE_ID_LEN + message.len());
        payload.extend_from_slice(target);
        payload.extend_from_slice(message);
        let frame = RelayFrame {
            kind: FrameKind::Relay,
            node: *target,
            payload,
        };
        self.send(&frame)
    }

    /// 长连接订阅（持有者侧）用 `None` 阻塞等待 DELIVERED；探测类客户端用短超时。
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<(), String> {
        match &self.io {
            ConnIo::Plain { reader, .. } => reader.set_read_timeout(timeout).map_err(|e| e.to_string()),
            ConnIo::Secure { reader, .. } => reader.set_read_timeout(timeout),
        }
    }

    /// 仅供协议一致性单测发送畸形/自定义帧；加密连接不支持（否则会破坏序号语义）。
    pub fn write_raw(&mut self, data: &[u8]) -> Result<(), String> {
        match &mut self.io {
            ConnIo::Plain { writer, .. } => {
                writer.write_all(data).map_err(|e| e.to_string())?;
                writer.flush().map_err(|e| e.to_string())
            }
            ConnIo::Secure { .. } => Err("加密连接不支持 write_raw".into()),
        }
    }

    /// 兼容旧调用方的「阻塞读到一帧或报错」接口。
    /// 空闲超时在这里仍按错误处理；订阅线程要用 [`Self::recv_frame_optional`] 区分超时与断链。
    pub fn recv_frame(&mut self) -> Result<RelayFrame, String> {
        match self.recv_frame_inner() {
            Ok(Some(f)) => Ok(f),
            Ok(None) => Err("空闲超时".into()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// 读到一帧返回 `Some`；**读超时返回 `Ok(None)`**，链路/协议错误才返回 `Err`。
    ///
    /// 订阅连接本来就是「长连接 + 本地短读超时轮询声明」，把超时压成字符串会让
    /// `relay_sub` 每轮都误判断链、重连、被服务端 `forget_local` 清掉声明——
    /// 对外表现就是 `LOOKUP_REACH` 在「声明已传播」之后又随机变回「不知道」。
    pub fn recv_frame_optional(&mut self) -> Result<Option<RelayFrame>, String> {
        match self.recv_frame_inner() {
            Ok(v) => Ok(v),
            Err(e) => Err(e.to_string()),
        }
    }

    fn recv_frame_inner(&mut self) -> Result<Option<RelayFrame>, RecvError> {
        match &mut self.io {
            ConnIo::Plain { reader, .. } => {
                let mut head = [0u8; FRAME_HEADER_LEN];
                if let Err(e) = reader.read_exact(&mut head) {
                    let e = classify_plain_read(e, "读帧头失败");
                    return if matches!(e, RecvError::Timeout(_)) { Ok(None) } else { Err(e) };
                }
                let len = u32::from_be_bytes([
                    head[1 + NODE_ID_LEN],
                    head[2 + NODE_ID_LEN],
                    head[3 + NODE_ID_LEN],
                    head[4 + NODE_ID_LEN],
                ]) as usize;
                if len > DEFAULT_MAX_FRAME {
                    return Err(RecvError::Protocol(format!("帧体超限: {len}")));
                }
                let mut payload = vec![0u8; len];
                if len > 0 {
                    if let Err(e) = reader.read_exact(&mut payload) {
                        let e = classify_plain_read(e, "读帧体失败");
                        return if matches!(e, RecvError::Timeout(_)) { Ok(None) } else { Err(e) };
                    }
                }
                let mut raw = head.to_vec();
                raw.extend_from_slice(&payload);
                RelayFrame::decode(&raw)
                    .map(Some)
                    .ok_or_else(|| RecvError::Protocol("帧解析失败".into()))
            }
            ConnIo::Secure { reader, .. } => match reader.recv() {
                Ok(f) => Ok(Some(f)),
                Err(e) if e.is_timeout() => Ok(None),
                Err(e) => Err(e),
            },
        }
    }
}

/// 按配置顺序向多个中继查询同一节点的可达性声明。
///
/// 与 [`RelayConn::lookup_reach`] 的关系：那里是一次 TCP 连接上的**一问一答**；
/// 这里是唤醒路径要用的**决策入口**——任何一个中继给出仍然有效的 `RelayOnly`
/// 就立刻返回（可以据此跳过注定失败的直连），否则返回第一个成功应答（可能是
/// `direct`、过期的声明、或「中继不知道」）。全部连接/协议失败才返回 Err。
///
/// **`None` 与过期声明都必须按 unknown 处理**：调用方只能「不跳过直连」，
/// 不能反过来把 unknown 当成低 ID 或据此拒绝服务。
pub fn lookup_reach_any(
    relays: &[String],
    target: &[u8; NODE_ID_LEN],
    timeout: Duration,
) -> Result<(String, Option<crate::reach::ReachDeclaration>), String> {
    if relays.is_empty() {
        return Err("未配置任何中继端点".into());
    }
    let mut errs = Vec::new();
    let mut fallback: Option<(String, Option<crate::reach::ReachDeclaration>)> = None;
    for relay in relays {
        let client = RelayClient::new(relay.clone());
        let mut conn = match client.connect(timeout) {
            Ok(c) => c,
            Err(e) => { errs.push(format!("{}: connect: {}", relay, e)); continue; }
        };
        // 与 `relay_wake` 一样用一次性会话 id，避免抢占持有者侧稳定注册项。
        let session = ephemeral_session_id(target);
        if let Err(e) = conn.register(&session) {
            errs.push(format!("{}: register: {}", relay, e));
            continue;
        }
        match conn.lookup_reach(target, timeout) {
            Ok(decl) => {
                let live = decl.as_ref()
                    .map(|d| d.effective_at(crate::reach::now_ms()));
                if live == Some(crate::reach::ReachClass::RelayOnly) {
                    return Ok((relay.clone(), decl));
                }
                if fallback.is_none() {
                    fallback = Some((relay.clone(), decl));
                }
            }
            Err(e) => errs.push(format!("{}: lookup: {}", relay, e)),
        }
    }
    fallback.ok_or_else(|| format!("全部中继查询失败（{}）", errs.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    /// 在调用方线程绑定监听，把 listener 移入工作线程，返回端口，避免竞态。
    fn spawn_mock_relay() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                // 读完整 REGISTER 帧（头 + 16 字节 payload），回 ACK
                let mut head = [0u8; 1 + NODE_ID_LEN + 4];
                if stream.read_exact(&mut head).is_ok() {
                    let plen = u32::from_be_bytes([
                        head[1 + NODE_ID_LEN],
                        head[2 + NODE_ID_LEN],
                        head[3 + NODE_ID_LEN],
                        head[4 + NODE_ID_LEN],
                    ]) as usize;
                    let mut payload = vec![0u8; plen];
                    if plen == 0 || stream.read_exact(&mut payload).is_ok() {
                        let ack = RelayFrame {
                            kind: FrameKind::Ack,
                            node: [0u8; NODE_ID_LEN],
                            payload: Vec::new(),
                        };
                        let _ = stream.write_all(&ack.encode());
                        let _ = stream.flush();
                    }
                }
                // 读完整 RELAY 帧，回 DELIVERED（来源=目标，模拟回环投递）
                let mut head2 = [0u8; 1 + NODE_ID_LEN + 4];
                if stream.read_exact(&mut head2).is_ok() {
                    let plen = u32::from_be_bytes([
                        head2[1 + NODE_ID_LEN],
                        head2[2 + NODE_ID_LEN],
                        head2[3 + NODE_ID_LEN],
                        head2[4 + NODE_ID_LEN],
                    ]) as usize;
                    let mut payload = vec![0u8; plen];
                    if plen == 0 || stream.read_exact(&mut payload).is_ok() {
                        let src: [u8; NODE_ID_LEN] = payload
                            .get(..NODE_ID_LEN)
                            .and_then(|s| s.try_into().ok())
                            .unwrap_or([0u8; NODE_ID_LEN]);
                        let msg = if payload.len() > NODE_ID_LEN {
                            payload[NODE_ID_LEN..].to_vec()
                        } else {
                            Vec::new()
                        };
                        let delivered = RelayFrame {
                            kind: FrameKind::Delivered,
                            node: src,
                            payload: msg,
                        };
                        let _ = stream.write_all(&delivered.encode());
                        let _ = stream.flush();
                    }
                }
            }
        });
        port
    }

    #[test]
    fn frame_roundtrip() {
        let f = RelayFrame {
            kind: FrameKind::Relay,
            node: [7u8; NODE_ID_LEN],
            payload: b"hello".to_vec(),
        };
        let enc = f.encode();
        let dec = RelayFrame::decode(&enc).unwrap();
        assert_eq!(dec.kind, FrameKind::Relay);
        assert_eq!(dec.node, [7u8; NODE_ID_LEN]);
        assert_eq!(dec.payload, b"hello");
    }

    /// 订阅连接的读超时必须和断链分开：连续多次超时之后，同一条 TCP 连接
    /// 仍要能正常收发帧，否则服务端会把它当成掉线清掉可达性声明。
    #[test]
    fn idle_timeout_keeps_the_same_connection_usable() {
        let port = spawn_mock_relay();
        let client = RelayClient::plaintext(format!("127.0.0.1:{port}"));
        let mut conn = client.connect(Duration::from_secs(2)).unwrap();
        conn.register(&[1u8; NODE_ID_LEN]).unwrap();
        conn.set_read_timeout(Some(Duration::from_millis(120))).unwrap();
        for _ in 0..3 {
            assert!(conn.recv_frame_optional().unwrap().is_none(),
                    "空闲读超时应返回 Ok(None)，不能当成断链");
        }
        conn.send_relay(&[2u8; NODE_ID_LEN], b"after-timeout").unwrap();
        let frame = conn.recv_frame_optional().unwrap().expect("同一连接还应能收帧");
        assert_eq!(frame.kind, FrameKind::Delivered);
        assert_eq!(&frame.payload, b"after-timeout");
    }

    #[test]
    fn register_and_relay_against_mock() {
        let port = spawn_mock_relay();
        let client = RelayClient::plaintext(format!("127.0.0.1:{}", port));
        let mut conn = client.connect(Duration::from_secs(2)).unwrap();
        let id = [1u8; NODE_ID_LEN];
        conn.register(&id).expect("register 应成功");
        let target = [2u8; NODE_ID_LEN];
        conn.send_relay(&target, b"wakeup").expect("send_relay 应成功");
        let delivered = conn.recv_frame().unwrap();
        assert_eq!(delivered.kind, FrameKind::Delivered);
        assert_eq!(&delivered.payload, b"wakeup");
    }
}
