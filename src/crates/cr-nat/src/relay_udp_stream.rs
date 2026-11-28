//! ADR-024 阶段 1：中继 UDP 承载上的**有界可靠层**（std-only，不引入 async）。
//!
//! # 为什么不直接用 TCP 中继的可靠交付
//!
//! ADR-023 / `relay_tunnel` 把 BT 字节流放在 TCP 中继帧上，靠 TCP 自己保证可靠有序。
//! 但 ADR-020 记录的部署里**入向 TCP 被边缘注入伪造 RST**、只有 UDP 可用；那条路上
//! 没有 TCP 的可靠性可用。本模块就是在 `relay_udp` 的 `DATA`/`DELIVER` 之上补一层
//! 有界的 seq/ACK/重传，把字节流变成可靠有序交付。
//!
//! # 硬约束（不可协商，见 ADR-024 §硬约束）
//!
//! - **独立于 UDP 头 seq**：`relay_udp::Session::accept_seq` 的 64 位滑窗是抗重放用的，
//!   每个 datagram 由 `UdpRelayClient::send` 递增；重传必须换新的 UDP 头 seq（否则会被
//!   当重放丢掉）。本层的 **app seq** 是另一套空间，专用于可靠交付，两者绝不混用。
//! - **STATUS 不是 ACK**：中继的 `UdpKind::Status` 只说明「中继是否转出」，不代表对端
//!   应用收到。本层**只认对端 `Segment` 里 piggyback 的 ACK**，绝不滑动发送窗口。
//!   `BandwidthLimited` 例外：它是中继明确说「这帧没转」的背压信号，用于尽快重发，
//!   仍然**不计送达、不计 ACK**。把它当 ACK 会让「中继转了但对端没收到」被误判为送达。
//! - **有界内存**：乱序重组窗口、在飞队列都受限；超出窗口的乱序段直接丢弃并计数，
//!   不无限缓存（ADR-024 否决条件之一是「无限重传 / 无界内存」）。
//! - **std-only**：只用 `std`，不引入 tokio / 第三方可靠传输库（ADR-015 依赖分层表）。
//!
//! # 线格式（放在 UDP DATA 的 data 部分，即 `send_data` 的 `data` 参数，target id 之后）
//!
//! ```text
//! 0..2   magic "US"（UDP Stream），与唤醒 JSON / 其它 DATA 区分
//! 2      kind: 1=DATA, 2=ACK, 3=CLOSE
//! 3      flags（保留，必须 0）
//! 4..8   app_seq (u32 BE)：DATA/CLOSE 是本段序号；ACK 段填 0
//! 8..12  ack (u32 BE)：累积确认 = 期望收到的下一个 seq（所有 kind 都 piggyback）
//! 12..20 sack (u64 BE)：bit i 表示 (ack + 1 + i) 已收到，有界 64 位
//! 20..   body：仅 DATA 携带用户字节；ACK/CLOSE 为空
//! ```
//!
//! DATA 有效负载上限 = `UDP_MAX_PAYLOAD - NODE_ID_LEN - SEG_HEADER` = 1132 B。
//! 所有控制段（ACK/CLOSE）都远小于 `SIGNAL_FRAME_MAX`，但 UDP 中继按**UDP 帧类型**
//! 计费（阶段 0）：本模块所有线段都装在 `UdpKind::Data` 里，因此 DATA/ACK/CLOSE
//! 一并计入字节预算。这是保守口径——不会让数据面绕过 ADR-014。

use crate::relay::NODE_ID_LEN;
use crate::relay_udp::{UdpStatus, UDP_MAX_PAYLOAD};
use std::collections::{BTreeMap, VecDeque};
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// 线段魔数：`US` = UDP Stream。
pub const SEG_MAGIC: [u8; 2] = *b"US";
pub const SEG_HEADER: usize = 20;
pub const KIND_DATA: u8 = 0x01;
pub const KIND_ACK: u8 = 0x02;
pub const KIND_CLOSE: u8 = 0x03;
/// DATA 单段用户字节上限：1152（UDP DATA 可用）− 20（线段头）。
pub const DATA_PAYLOAD_MAX: usize = UDP_MAX_PAYLOAD - NODE_ID_LEN - SEG_HEADER;

/// 发送 / 重组窗口（段数）。SACK 位图 64 位，窗口 32 留足冗余。
pub const WINDOW: u32 = 32;
/// 初始重传超时。
pub const RTO_INIT: Duration = Duration::from_millis(200);
/// 重传超时上限，避免退避到几分钟让调用方等到天荒地老。
pub const RTO_MAX: Duration = Duration::from_secs(5);
/// 连续重复 ACK 达到此数触发快重传。
pub const FAST_RETRANS_ACKS: u32 = 3;
/// v2.4.116：拥塞控制的 RTO 下限（有 RTT 样本时不再固定 200 ms）。
pub const RTO_MIN: Duration = Duration::from_millis(50);

/// ADR-025：LEDBAT 的目标排队时延（RFC 6817 `TARGET`，100 ms）。
pub const LEDBAT_TARGET: Duration = Duration::from_millis(100);
/// ADR-025：LEDBAT 的每 RTT 调整增益（RFC 6817 `GAIN`，1.0 段/RTT）。
pub const LEDBAT_GAIN: f64 = 1.0;
/// ADR-025 塌陷保护：同一段连续 RTO 达到此数且 base 未推进，就复位 cwnd。
/// 没有这条，「丢包不降窗」在真实拥塞崩溃时会把队列越堆越深。
pub const LEDBAT_RTO_COLLAPSE: u32 = 6;
/// ADR-025 基准漂移：连续这么多个样本都高于基准 2 倍，判定路径变了，重设基准。
/// 没有这条，换路由后 `base_delay` 单调不升会把「新基准」永久当成排队。
pub const LEDBAT_BASE_RESET_SAMPLES: u32 = 30;

/// ADR-025：cwnd 由什么信号驱动。
///
/// - `Aimd`：丢包驱动（v1 行为，乘性减 `beta`）。**默认**，逐字节保持既有语义。
/// - `Ledbat`：排队时延驱动；丢包只触发重传，**不降窗**，仅连续超时走塌陷复位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CcMode {
    #[default]
    Aimd,
    Ledbat,
}

impl CcMode {
    pub fn label(self) -> &'static str {
        match self {
            CcMode::Aimd => "aimd",
            CcMode::Ledbat => "ledbat",
        }
    }
    /// 从配置字符串解析。**未知取值回落 `Aimd`**：真正的校验在 `cr-core` 的配置加载里
    /// （非法值直接报错），这里只是最后一道消费侧兜底，不承担「猜用户意图」的职责。
    pub fn from_mode_str(s: &str) -> Self {
        if s.eq_ignore_ascii_case("ledbat") {
            CcMode::Ledbat
        } else {
            CcMode::Aimd
        }
    }
    pub fn as_u64(self) -> u64 {
        match self {
            CcMode::Aimd => 0,
            CcMode::Ledbat => 1,
        }
    }
    pub fn from_u64(v: u64) -> Self {
        if v == 1 {
            CcMode::Ledbat
        } else {
            CcMode::Aimd
        }
    }
}

/// ADR-024 阶段 3：可选拥塞控制参数。
///
/// **默认全关**：阶段 1/2 的既有行为（固定 32 段窗口、RTO 指数退避、不 pacing）逐字节不变。
/// 打开后发送窗口由 `mode` 决定：`Aimd` 每收一个推进 ACK 加性增 `1/cwnd`、丢包乘性减半；
/// `Ledbat`（ADR-025）按排队时延每 RTT 增减 1 段左右、丢包不降窗。
/// RTO 由 Jacobson–Karels 的 `srtt + 4*rttvar` 推导；pacing 把新段发送摊到整个 RTT。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CongestionConfig {
    pub enabled: bool,
    pub cwnd_init: u32,
    pub cwnd_min: u32,
    pub cwnd_max: u32,
    /// 乘性减小系数（典型 0.5）。**仅 `CcMode::Aimd` 使用**。
    pub beta: f64,
    /// 是否对新段做速率平滑；重传永远立即发，不受 pacing 阻挡。
    pub pacing: bool,
    /// cwnd 驱动信号，见 [`CcMode`]。
    pub mode: CcMode,
    /// LEDBAT 目标排队时延；`Aimd` 忽略本字段。
    pub ledbat_target: Duration,
    /// LEDBAT 每 RTT 增益（段）；`Aimd` 忽略本字段。
    pub ledbat_gain: f64,
}

impl Default for CongestionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cwnd_init: WINDOW,
            cwnd_min: 1,
            cwnd_max: WINDOW,
            beta: 0.5,
            pacing: false,
            mode: CcMode::Aimd,
            ledbat_target: LEDBAT_TARGET,
            ledbat_gain: LEDBAT_GAIN,
        }
    }
}

/// 拥塞控制可观测快照（不作为任何跨公网公平性结论）。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CcStats {
    pub enabled: bool,
    /// 拥塞窗口的 100 倍（避免浮点出现在 JSON 里）。
    pub cwnd_x100: u64,
    pub srtt_ms: u64,
    pub rttvar_ms: u64,
    pub loss_events: u64,
    pub rtt_samples: u64,
    pub paced_new_segments: u64,
    /// 中继明确回 `BandwidthLimited` 的次数（背压，不计入 loss_events）。
    pub relay_backpressure_events: u64,
    /// 驱动信号（`aimd` / `ledbat`）。
    pub mode: CcMode,
    /// 观测到的最小 RTT（LEDBAT 的传播时延估计）。
    pub base_delay_ms: u64,
    /// 最近一次样本相对基准的排队时延。
    pub queued_delay_ms: u64,
    /// cwnd 按时延信号调整过的次数。
    pub ledbat_updates: u64,
    /// 丢了包但时延没超目标、因此**没有**降窗的次数（LEDBAT 的关键行为证据）。
    pub loss_ignored_low_delay: u64,
    /// 连续超时触发的塌陷复位次数。
    pub collapse_resets: u64,
}

// ---- u32 序列号环绕比较（经典 TCP 差值法；窗口 32 « 2^31，安全）----

#[inline]
fn seq_lt(a: u32, b: u32) -> bool {
    (a as i32).wrapping_sub(b as i32) < 0
}
#[inline]
fn seq_gt(a: u32, b: u32) -> bool {
    seq_lt(b, a)
}
#[inline]
fn seq_le(a: u32, b: u32) -> bool {
    !seq_gt(a, b)
}
#[inline]
fn seq_ge(a: u32, b: u32) -> bool {
    !seq_lt(a, b)
}

/// 一个可靠层段（与 socket 解耦，便于注入丢包/乱序/重复做协议面测试）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Data { seq: u32, ack: u32, sack: u64, body: Vec<u8> },
    Ack { ack: u32, sack: u64 },
    Close { seq: u32, ack: u32, sack: u64 },
}

impl Segment {
    pub fn ack(&self) -> u32 {
        match self {
            Segment::Data { ack, .. } | Segment::Ack { ack, .. } | Segment::Close { ack, .. } => *ack,
        }
    }
}

/// 编码成 UDP DATA 的 `data` 部分。DATA 段负载超限时返回 Err（不静默截断）。
pub fn encode(seg: &Segment) -> Result<Vec<u8>, String> {
    let body = match seg {
        Segment::Data { body, .. } => body.as_slice(),
        _ => &[],
    };
    if body.len() > DATA_PAYLOAD_MAX {
        return Err(format!("线段负载 {} B 超过上限 {} B", body.len(), DATA_PAYLOAD_MAX));
    }
    let mut out = Vec::with_capacity(SEG_HEADER + body.len());
    out.extend_from_slice(&SEG_MAGIC);
    let (kind, seq, ack, sack) = match seg {
        Segment::Data { seq, ack, sack, .. } => (KIND_DATA, *seq, *ack, *sack),
        Segment::Ack { ack, sack } => (KIND_ACK, 0u32, *ack, *sack),
        Segment::Close { seq, ack, sack } => (KIND_CLOSE, *seq, *ack, *sack),
    };
    out.push(kind);
    out.push(0); // flags 保留
    out.extend_from_slice(&seq.to_be_bytes());
    out.extend_from_slice(&ack.to_be_bytes());
    out.extend_from_slice(&sack.to_be_bytes());
    out.extend_from_slice(body);
    Ok(out)
}

/// 严格解码：magic / flags / kind / body 长度任何不符都拒绝，不猜。
pub fn decode(buf: &[u8]) -> Result<Segment, String> {
    if buf.len() < SEG_HEADER {
        return Err(format!("线段太短: {} B", buf.len()));
    }
    if &buf[..2] != &SEG_MAGIC {
        return Err("线段 magic 不符".into());
    }
    if buf[3] != 0 {
        return Err(format!("线段 flags 非零: 0x{:02x}", buf[3]));
    }
    let seq = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let ack = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);
    let sack = u64::from_be_bytes([
        buf[12], buf[13], buf[14], buf[15], buf[16], buf[17], buf[18], buf[19],
    ]);
    let body = &buf[SEG_HEADER..];
    match buf[2] {
        KIND_DATA => Ok(Segment::Data { seq, ack, sack, body: body.to_vec() }),
        KIND_ACK => {
            if !body.is_empty() {
                return Err("ACK 段不得携带 body".into());
            }
            Ok(Segment::Ack { ack, sack })
        }
        KIND_CLOSE => {
            if !body.is_empty() {
                return Err("CLOSE 段不得携带 body".into());
            }
            Ok(Segment::Close { seq, ack, sack })
        }
        other => Err(format!("未知线段 kind: 0x{other:02x}")),
    }
}

/// 一条可靠流的运动状态（纯逻辑，不碰 socket、不自己读时钟）。
///
/// 所有需要时间/发包的地方都通过参数注入（`take_transmissions(now)`），这样单测能
/// 精确制造丢包、乱序、重复、超时，而不依赖真实网络时序——这是协议面 crate 的证据文化。
#[derive(Debug)]
pub struct StreamCore {
    // ---- 发送侧 ----
    /// 下一个待分配的 app seq。
    next_seq: u32,
    /// 最小未确认 seq（连续确认边界）。
    base: u32,
    /// 在飞段（含尚未首发的），按 seq 递增。长度即发送窗口占用。
    inflight: VecDeque<SentSeg>,
    /// 连续重复 ACK 计数（快重传用）。
    dup_acks: u32,
    /// 上一次收到的 ack，用于判定「没推进」。
    last_ack: u32,
    /// 当前 RTO（指数退避）。
    rto: Duration,
    /// 快重传待办：base 段下一轮强制重发。
    fast_retransmit: bool,
    // ---- 可选拥塞控制（默认关）----
    cc: CongestionConfig,
    cwnd: f64,
    srtt: Option<Duration>,
    rttvar: Duration,
    /// 每个 RTT 最多减小一次 cwnd 的时间门。
    last_loss_at: Option<Instant>,
    loss_events: u64,
    rtt_samples: u64,
    next_send_at: Option<Instant>,
    paced_new_segments: u64,
    /// 中继背压：短时间内的多个 STATUS 只触发一次快重传，避免风暴。
    last_backpressure_at: Option<Instant>,
    relay_backpressure_events: u64,
    // ---- ADR-025：LEDBAT（时延驱动）----
    /// 观测到的最小 RTT，即传播时延基准（只降不升，路径突变时重设）。
    base_delay: Option<Duration>,
    /// 最近一次样本相对基准的排队时延。
    queued_delay: Duration,
    /// 上一次按时延信号调整 cwnd 的时刻（每个 RTT 最多调一次）。
    ledbat_last_at: Option<Instant>,
    ledbat_updates: u64,
    /// 丢了包但排队时延未超目标、因此没有降窗的次数。
    loss_ignored_low_delay: u64,
    /// 塌陷复位次数（连续 RTO）。
    collapse_resets: u64,
    /// 连续超时重传次数（base 推进即清零）。
    rto_streak: u32,
    /// 上一次因超时计「连续轮次」的时刻：同一时刻一批段超时只算一轮，
    /// 否则 8 个段一起超时会被误当成 8 次连续超时、立刻触发塌陷复位。
    last_rto_at: Option<Instant>,
    /// 连续「样本高于基准 2 倍」的次数，用于判定路径突变。
    base_drift: u32,
    // ---- 接收侧 ----
    /// 下一个期望收到的 app seq（累积确认值）。
    expected: u32,
    /// 乱序重组缓冲，仅保留 [expected, expected+WINDOW) 内的段。
    incoming: BTreeMap<u32, Vec<u8>>,
    /// 是否已收到对端 CLOSE。
    peer_closed: bool,
    // ---- 计数（可观测）----
    sent_segments: u64,
    retransmits: u64,
    fast_retransmits: u64,
    dup_dropped: u64,
    oor_dropped: u64,
    ack_received: u64,
    status_ignored: u64,
    bytes_delivered: u64,
}

#[derive(Debug)]
struct SentSeg {
    seq: u32,
    body: Vec<u8>,
    sent_at: Option<Instant>,
    retries: u32,
    /// SACK 已确认（窗口内乱序确认），等 base 推进时一并清理。
    sacked: bool,
}

impl Default for StreamCore {
    fn default() -> Self {
        Self::new(0)
    }
}

impl StreamCore {
    /// 用起始 app seq 构造。双端都从 0 开始即可（各自独立 seq 空间）。
    pub fn new(start_seq: u32) -> Self {
        Self::with_cc(start_seq, CongestionConfig::default())
    }

    /// 带可选拥塞控制构造。参数先夹到合法范围，不让配置把窗口撑爆。
    pub fn with_cc(start_seq: u32, cc: CongestionConfig) -> Self {
        let cwnd_max = cc.cwnd_max.clamp(1, WINDOW);
        let cwnd_min = cc.cwnd_min.clamp(1, cwnd_max);
        let cwnd_init = cc.cwnd_init.clamp(cwnd_min, cwnd_max);
        let cc = CongestionConfig { cwnd_min, cwnd_max, cwnd_init, beta: cc.beta.clamp(0.1, 0.9), ..cc };
        Self {
            next_seq: start_seq,
            base: start_seq,
            inflight: VecDeque::new(),
            dup_acks: 0,
            last_ack: start_seq,
            rto: RTO_INIT,
            fast_retransmit: false,
            cc,
            cwnd: cwnd_init as f64,
            srtt: None,
            rttvar: Duration::ZERO,
            last_loss_at: None,
            loss_events: 0,
            rtt_samples: 0,
            next_send_at: None,
            paced_new_segments: 0,
            last_backpressure_at: None,
            relay_backpressure_events: 0,
            base_delay: None,
            queued_delay: Duration::ZERO,
            ledbat_last_at: None,
            ledbat_updates: 0,
            loss_ignored_low_delay: 0,
            collapse_resets: 0,
            rto_streak: 0,
            last_rto_at: None,
            base_drift: 0,
            expected: start_seq,
            incoming: BTreeMap::new(),
            peer_closed: false,
            sent_segments: 0,
            retransmits: 0,
            fast_retransmits: 0,
            dup_dropped: 0,
            oor_dropped: 0,
            ack_received: 0,
            status_ignored: 0,
            bytes_delivered: 0,
        }
    }

    /// 应用写入：切成 ≤`DATA_PAYLOAD_MAX` 的段并排队。
    /// 受发送窗口限制，返回**实际接纳**的字节数（Write trait 语义：可能短写）。
    pub fn write(&mut self, data: &[u8]) -> usize {
        if data.is_empty() {
            return 0;
        }
        let window = if self.cc.enabled {
            (self.cwnd as usize).max(1).min(WINDOW as usize)
        } else {
            WINDOW as usize
        };
        let capacity = window.saturating_sub(self.inflight.len());
        if capacity == 0 {
            return 0;
        }
        let max_segments = capacity.max(1);
        let mut written = 0usize;
        let mut segments = 0usize;
        while written < data.len() && segments < max_segments {
            let n = (data.len() - written).min(DATA_PAYLOAD_MAX);
            self.inflight.push_back(SentSeg {
                seq: self.next_seq,
                body: data[written..written + n].to_vec(),
                sent_at: None,
                retries: 0,
                sacked: false,
            });
            self.next_seq = self.next_seq.wrapping_add(1);
            written += n;
            segments += 1;
        }
        written
    }

    /// 有界乱序重组：只保留窗口内的段，超窗直接丢弃并计数。
    fn trim_incoming(&mut self) {
        let hi = self.expected.wrapping_add(WINDOW);
        let mut drop = Vec::new();
        for (&s, _) in self.incoming.iter() {
            if seq_lt(s, self.expected) || seq_ge(s, hi) {
                drop.push(s);
            }
        }
        for s in drop {
            self.incoming.remove(&s);
            self.oor_dropped += 1;
        }
    }

    /// 当前累积 ACK 与 SACK 位图（供 piggyback / 独立 ACK 段使用）。
    pub fn ack_state(&self) -> (u32, u64) {
        let mut sack = 0u64;
        for i in 0..64u32 {
            let s = self.expected.wrapping_add(1 + i);
            if self.incoming.contains_key(&s) {
                sack |= 1u64 << i;
            }
        }
        (self.expected, sack)
    }

    /// 处理对端 DATA 段。返回本次按序交付给应用的数据（可能为多段拼成的一串字节）。
    /// 重复段 / 超窗段直接丢弃并计数；piggyback 的 ack 同步驱动发送窗口。
    pub fn on_data(&mut self, seq: u32, body: Vec<u8>, ack: u32, sack: u64) -> Option<Vec<u8>> {
        self.on_ack(ack, sack);
        self.trim_incoming();

        if seq_lt(seq, self.expected) || self.incoming.contains_key(&seq) {
            self.dup_dropped += 1;
            return None;
        }
        let hi = self.expected.wrapping_add(WINDOW);
        if seq_ge(seq, hi) {
            self.oor_dropped += 1;
            return None;
        }
        self.incoming.insert(seq, body);

        // 从 expected 开始连续取出交付。
        let mut delivered = Vec::new();
        while let Some(b) = self.incoming.remove(&self.expected) {
            delivered.extend_from_slice(&b);
            self.expected = self.expected.wrapping_add(1);
        }
        if delivered.is_empty() {
            None
        } else {
            self.bytes_delivered += delivered.len() as u64;
            Some(delivered)
        }
    }

    /// 处理 ACK（可能 piggyback 在 DATA/ACK/CLOSE 上）。真实时钟版本。
    pub fn on_ack(&mut self, ack: u32, sack: u64) {
        self.on_ack_at(ack, sack, Instant::now());
    }

    /// 虚拟时钟版本：单测/量具可精确注入时间，不依赖真实网络时序。
    pub fn on_ack_at(&mut self, ack: u32, sack: u64, now: Instant) {
        self.ack_received += 1;
        // ACK 必须在 [base, next_seq] 内，否则是过期/伪造，忽略推进。
        if seq_ge(ack, self.base) && seq_le(ack, self.next_seq) {
            if seq_gt(ack, self.base) {
                self.base = ack;
                if self.cc.enabled {
                    // 清掉已确认段并用它们的发送时刻采 RTT；只有真的发过的段才采样。
                    let mut last_sample: Option<Duration> = None;
                    while let Some(front) = self.inflight.front() {
                        if seq_lt(front.seq, self.base) {
                            if let Some(seg) = self.inflight.pop_front() {
                                if let Some(t) = seg.sent_at {
                                    if now >= t {
                                        let s = now.duration_since(t);
                                        self.sample_rtt(s);
                                        last_sample = Some(s);
                                    }
                                }
                            }
                        } else {
                            break;
                        }
                    }
                    match self.cc.mode {
                        CcMode::Aimd => {
                            // AIMD 的 AI：每个推进 ACK 加 1/cwnd，封顶 cwnd_max。
                            self.cwnd = (self.cwnd + 1.0 / self.cwnd.max(1.0))
                                .min(self.cc.cwnd_max as f64);
                        }
                        CcMode::Ledbat => {
                            // ADR-025：cwnd 由排队时延驱动；没有新样本时也允许按 srtt 节奏推进。
                            match last_sample {
                                Some(s) => self.ledbat_on_sample(now, s),
                                None => self.ledbat_tick(now),
                            }
                        }
                    }
                    // base 推进说明链路还活着，塌陷计数清零。
                    self.rto_streak = 0;
                    self.rto = self.derived_rto();
                } else {
                    self.rto = RTO_INIT;
                    while let Some(front) = self.inflight.front() {
                        if seq_lt(front.seq, self.base) {
                            self.inflight.pop_front();
                        } else {
                            break;
                        }
                    }
                }
                self.dup_acks = 0;
            } else if ack == self.last_ack && ack == self.base {
                // 没有推进且和上次一样 → 重复 ACK。
                self.dup_acks = self.dup_acks.saturating_add(1);
                if self.dup_acks >= FAST_RETRANS_ACKS {
                    self.fast_retransmit = true;
                    self.fast_retransmits += 1;
                    self.dup_acks = 0;
                    self.note_loss(now);
                }
            }
        }
        self.last_ack = ack;

        // SACK：bit i 对应 seq = ack + 1 + i，标记窗口内对应段。
        for i in 0..64u32 {
            if sack & (1u64 << i) == 0 {
                continue;
            }
            let s = ack.wrapping_add(1 + i);
            if let Some(seg) = self.inflight.iter_mut().find(|x| x.seq == s) {
                seg.sacked = true;
            }
        }
        // 若 SACK 把队头也确认了，base 可跳过它（仿 TCP 的 SACK 推进，但只在连续已确认时）。
        while let Some(front) = self.inflight.front() {
            if front.seq == self.base && front.sacked {
                self.base = self.base.wrapping_add(1);
                self.inflight.pop_front();
            } else {
                break;
            }
        }
    }

    /// Jacobson–Karels RTT 估计。只在实际发出过的段被确认时采样。
    fn sample_rtt(&mut self, sample: Duration) {
        self.rtt_samples += 1;
        let s = self.srtt.unwrap_or(sample);
        let srtt_us = s.as_micros() as f64;
        let sample_us = sample.as_micros() as f64;
        let rttvar_us = self.rttvar.as_micros() as f64;
        self.srtt = Some(Duration::from_micros(((7.0 * srtt_us + sample_us) / 8.0) as u64));
        self.rttvar = Duration::from_micros(
            ((3.0 * rttvar_us + (srtt_us - sample_us).abs()) / 4.0) as u64,
        );
    }

    fn derived_rto(&self) -> Duration {
        match self.srtt {
            Some(s) => (s + self.rttvar * 4).clamp(RTO_MIN, RTO_MAX),
            None => RTO_INIT,
        }
    }

    // ---- ADR-025：LEDBAT（排队时延驱动）----

    /// 新 RTT 样本到达：更新传播时延基准与排队时延，然后按 RTT 节奏调整一次窗口。
    fn ledbat_on_sample(&mut self, now: Instant, sample: Duration) {
        // 路径突变检测：样本连续高于基准 2 倍时，把基准重设到当前样本。
        // 否则「只降不升」会把新路径的正常时延永久算成排队，窗口永远回不来。
        match self.base_delay {
            Some(b) => {
                if sample > b.saturating_mul(2) {
                    self.base_drift = self.base_drift.saturating_add(1);
                } else {
                    self.base_drift = 0;
                }
                if self.base_drift >= LEDBAT_BASE_RESET_SAMPLES {
                    self.base_delay = Some(sample);
                    self.base_drift = 0;
                }
            }
            None => {}
        }
        let base = match self.base_delay {
            Some(b) if b <= sample => b,
            _ => sample,
        };
        self.base_delay = Some(base);
        self.queued_delay = sample.saturating_sub(base);
        self.ledbat_tick(now);
    }

    /// 每个 RTT 最多调一次 cwnd。空链路每 RTT 涨 `gain` 段；排队达 `target` 时停止增长；
    /// 超过 `target` 时按超出比例回退。稳定态是「把瓶颈队列填到 target 附近」。
    fn ledbat_tick(&mut self, now: Instant) {
        if !self.cc.enabled || self.cc.mode != CcMode::Ledbat {
            return;
        }
        let rtt = self.srtt.unwrap_or(RTO_INIT).max(Duration::from_millis(1));
        let due = self
            .ledbat_last_at
            .map(|t| now.duration_since(t) >= rtt)
            .unwrap_or(true);
        if !due {
            return;
        }
        self.ledbat_last_at = Some(now);
        let target = self.cc.ledbat_target.as_secs_f64().max(1e-3);
        // 排队时延超过 60 s 没有物理意义，夹一下防止异常样本把窗口一刀砍到底。
        let qd = self.queued_delay.as_secs_f64().min(60.0);
        let delta = if qd < target {
            self.cc.ledbat_gain * (target - qd) / target
        } else {
            -self.cc.ledbat_gain * (qd - target) / target
        };
        self.cwnd = (self.cwnd + delta)
            .clamp(self.cc.cwnd_min as f64, self.cc.cwnd_max as f64);
        self.ledbat_updates += 1;
    }

    /// 丢包事件：AIMD 乘性减小（同一 RTT 内不重复减小）；LEDBAT **不降窗**。
    ///
    /// LEDBAT 的理由（ADR-025）：非对称随机丢包不是拥塞信号，而 AIMD 把它当成「我该让」，
    /// 正是 EX-1 `78/80` 的失败机理。丢包只触发重传；真正的塌陷由连续超时兜底。
    fn note_loss(&mut self, now: Instant) {
        if !self.cc.enabled {
            return;
        }
        self.loss_events += 1;
        if self.cc.mode == CcMode::Ledbat {
            self.loss_ignored_low_delay += 1;
            return;
        }
        let rtt = self.srtt.unwrap_or(RTO_INIT);
        let due = self.last_loss_at
            .map(|t| now.duration_since(t) >= rtt)
            .unwrap_or(true);
        if due {
            self.cwnd = (self.cwnd * self.cc.beta).max(self.cc.cwnd_min as f64);
            self.last_loss_at = Some(now);
        }
    }

    /// 拥塞控制快照（默认关闭时 cwnd=WINDOW、样本为 0）。
    pub fn cc_stats(&self) -> CcStats {
        CcStats {
            enabled: self.cc.enabled,
            cwnd_x100: (self.cwnd * 100.0) as u64,
            srtt_ms: self.srtt.map(|d| d.as_millis() as u64).unwrap_or(0),
            rttvar_ms: self.rttvar.as_millis() as u64,
            loss_events: self.loss_events,
            rtt_samples: self.rtt_samples,
            paced_new_segments: self.paced_new_segments,
            relay_backpressure_events: self.relay_backpressure_events,
            mode: self.cc.mode,
            base_delay_ms: self.base_delay.map(|d| d.as_millis() as u64).unwrap_or(0),
            queued_delay_ms: self.queued_delay.as_millis() as u64,
            ledbat_updates: self.ledbat_updates,
            loss_ignored_low_delay: self.loss_ignored_low_delay,
            collapse_resets: self.collapse_resets,
        }
    }

    /// STATUS 帧：中继转发的回执，**不是 ACK**，只记数，绝不滑动窗口。
    pub fn note_status_ignored(&mut self) {
        self.status_ignored += 1;
    }

    /// 中继明确拒收（`BandwidthLimited`）：这是背压而不是丢包。
    /// 只尽快重发被卡住的 base 段、把已经膨胀的 RTO 拉回 RTT 派生值；
    /// **不**乘性减 cwnd、**不**指数退避 RTO，否则令牌桶会把 RTT 反馈环推到秒级，
    /// 两条同速率竞争流会因随机退避错配而长期不公平。短时间内重复 STATUS
    /// 只记数，不重复入队快重传，避免中继拒收风暴。
    pub fn note_relay_backpressure(&mut self, now: Instant) {
        self.relay_backpressure_events += 1;
        let due = self
            .last_backpressure_at
            .map(|t| now.duration_since(t) >= Duration::from_millis(5))
            .unwrap_or(true);
        if !due {
            return;
        }
        self.last_backpressure_at = Some(now);
        self.fast_retransmit = true;
        self.rto = if self.cc.enabled && self.srtt.is_some() {
            self.derived_rto()
        } else {
            RTO_INIT
        };
    }

    pub fn note_peer_closed(&mut self) {
        self.peer_closed = true;
    }
    pub fn peer_closed(&self) -> bool {
        self.peer_closed
    }

    /// 取出本轮要发出的段：新段（首发）+ 超时重传 + 快重传。
    /// 用 `now` 判断超时；重传的 app seq **不变**（可靠层序号），但 UDP 头 seq 由
    /// `UdpRelayClient::send_data` 每次调用递增（抗重放），两者职责分开。
    pub fn take_transmissions(&mut self, now: Instant) -> Vec<Segment> {
        let (ack, sack) = self.ack_state();
        let mut out = Vec::new();

        // 快重传：base 段强制重发一次。
        if self.fast_retransmit {
            self.fast_retransmit = false;
            if let Some(seg) = self.inflight.iter_mut().find(|x| x.seq == self.base) {
                seg.sent_at = Some(now);
                seg.retries = seg.retries.saturating_add(1);
                self.retransmits += 1;
                // 丢失事件已在 on_ack_at 的第三次重复 ACK 处记过，这里不重复计数。
                out.push(Segment::Data {
                    seq: seg.seq,
                    ack,
                    sack,
                    body: seg.body.clone(),
                });
            }
        }

        // pacing 每轮最多放一个新段；重传不受 pacing 阻挡。关掉时行为与阶段 1/2 一致。
        let derived_rto = if self.cc.enabled && self.srtt.is_some() {
            self.derived_rto()
        } else {
            RTO_INIT
        };
        let mut new_emitted = false;
        for seg in self.inflight.iter_mut() {
            match seg.sent_at {
                None => {
                    if self.cc.enabled && self.cc.pacing {
                        if let Some(t) = self.next_send_at {
                            if now < t {
                                continue;
                            }
                        }
                        if new_emitted {
                            continue;
                        }
                        let rtt = self.srtt.unwrap_or(Duration::from_millis(25));
                        let rate = self.cwnd * DATA_PAYLOAD_MAX as f64
                            / rtt.as_secs_f64().max(0.005);
                        let gap = Duration::from_secs_f64(seg.body.len() as f64 / rate.max(1.0));
                        self.next_send_at = Some(now + gap.min(Duration::from_millis(200)));
                        self.paced_new_segments += 1;
                        new_emitted = true;
                    }
                    // 首发。
                    seg.sent_at = Some(now);
                    self.sent_segments += 1;
                    out.push(Segment::Data { seq: seg.seq, ack, sack, body: seg.body.clone() });
                }
                Some(t) if now.duration_since(t) >= self.rto => {
                    // 超时重传：RTT 派生 RTO ×2；无样本时沿用阶段 1/2 的指数退避。
                    seg.sent_at = Some(now);
                    seg.retries = seg.retries.saturating_add(1);
                    self.retransmits += 1;
                    if self.cc.enabled {
                        // 内联 note_loss：这里持有 inflight 的 iter_mut，不能借整个 self。
                        self.loss_events += 1;
                        match self.cc.mode {
                            CcMode::Aimd => {
                                let rtt = self.srtt.unwrap_or(RTO_INIT);
                                let due = self.last_loss_at
                                    .map(|t| now.duration_since(t) >= rtt)
                                    .unwrap_or(true);
                                if due {
                                    self.cwnd =
                                        (self.cwnd * self.cc.beta).max(self.cc.cwnd_min as f64);
                                    self.last_loss_at = Some(now);
                                }
                            }
                            CcMode::Ledbat => {
                                // ADR-025：丢包不降窗；只有连续超时才认塌陷并复位。
                                self.loss_ignored_low_delay += 1;
                                // 同一时刻一批段一起超时只算一轮，否则 8 段齐超会被
                                // 误判成 8 次「连续超时」，塌陷保护立刻误触发。
                                let new_round = self
                                    .last_rto_at
                                    .map(|t| now.duration_since(t) >= self.rto)
                                    .unwrap_or(true);
                                if new_round {
                                    self.last_rto_at = Some(now);
                                    self.rto_streak = self.rto_streak.saturating_add(1);
                                }
                                if self.rto_streak >= LEDBAT_RTO_COLLAPSE {
                                    self.cwnd = self.cc.cwnd_init as f64;
                                    self.rto_streak = 0;
                                    self.collapse_resets += 1;
                                }
                            }
                        }
                    }
                    self.rto = if self.cc.enabled && self.srtt.is_some() {
                        (derived_rto.max(self.rto * 2)).min(RTO_MAX)
                    } else {
                        (self.rto * 2).min(RTO_MAX)
                    };
                    out.push(Segment::Data { seq: seg.seq, ack, sack, body: seg.body.clone() });
                }
                Some(_) => {}
            }
        }
        out
    }

    /// 构造一个 CLOSE 段（关闭时发出，携带当前累积 ack）。
    pub fn close_segment(&self) -> Segment {
        let (ack, sack) = self.ack_state();
        Segment::Close { seq: self.next_seq, ack, sack }
    }

    // ---- 可观测计数 ----
    pub fn inflight_len(&self) -> usize {
        self.inflight.len()
    }
    pub fn incoming_len(&self) -> usize {
        self.incoming.len()
    }
    pub fn bytes_delivered(&self) -> u64 {
        self.bytes_delivered
    }
    pub fn retransmits(&self) -> u64 {
        self.retransmits
    }
    pub fn fast_retransmits(&self) -> u64 {
        self.fast_retransmits
    }
    pub fn dup_dropped(&self) -> u64 {
        self.dup_dropped
    }
    pub fn oor_dropped(&self) -> u64 {
        self.oor_dropped
    }
    pub fn status_ignored(&self) -> u64 {
        self.status_ignored
    }
    pub fn ack_received(&self) -> u64 {
        self.ack_received
    }
    pub fn rto(&self) -> Duration {
        self.rto
    }
}

// =========================================================================
// UdpRelayStream：把 StreamCore 粘到真 UdpRelayClient 上的 Read + Write 字节流。
// 阶段 2 的发起方 / 持有方都用它，上层可以像 TCP RelayTunnel 一样 splice。
// =========================================================================

/// 可观测计数（泵线程写、外部读，全原子量）。
#[derive(Debug, Default)]
struct StreamCounters {
    bytes_written: AtomicU64,
    bytes_read: AtomicU64,
    retransmits: AtomicU64,
    status_ignored: AtomicU64,
    sender_alive: AtomicBool,
    cc_enabled: AtomicBool,
    cc_cwnd_x100: AtomicU64,
    cc_srtt_ms: AtomicU64,
    cc_loss_events: AtomicU64,
    cc_rtt_samples: AtomicU64,
    cc_paced_new: AtomicU64,
    cc_backpressure: AtomicU64,
    /// ADR-025：0 = aimd，1 = ledbat。
    cc_mode: AtomicU64,
    cc_base_delay_ms: AtomicU64,
    cc_queued_delay_ms: AtomicU64,
    cc_ledbat_updates: AtomicU64,
    cc_loss_ignored: AtomicU64,
    cc_collapse_resets: AtomicU64,
    last_error: std::sync::Mutex<Option<String>>,
}

/// 一条经 UDP 中继的**可靠有序字节流**，实现 std `Read + Write`。
///
/// - 构造时传一个已 `hello` 过的 [`crate::relay_udp::UdpRelayClient`] 与对端会话 id。
/// - 内部一条 pump 线程：写队列 → `StreamCore::write` → 编码线段 → `send_data`；
///   同时 `recv_frame` 收对端 ACK（驱动窗口）/ DATA（组装后放入读队列）；
///   **中继 STATUS 只记数，绝不当 ACK**（ADR-024 硬约束）。
/// - `drop` 时发 CLOSE 段并停泵，对端读到 EOF。
///
/// 与 `relay_tunnel::RelayTunnel` 同为 `Read + Write`，因此可直接复用上层 splice 模式。
pub struct UdpRelayStream {
    write: UdpStreamWrite,
    read: UdpStreamRead,
}

/// 读写两半共享的生命周期状态：最后一个持有者 drop 时停泵并 join。
/// 三个字段都是独立 Arc，泵只 clone 它们（不持有 `Arc<StreamShared>`），避免循环引用
/// 导致 StreamShared 永不析构、pump 变孤儿线程。
struct StreamShared {
    stop: Arc<AtomicBool>,
    counters: Arc<StreamCounters>,
    write_closed: Arc<AtomicBool>,
    /// 中继观测到的对端会话 id（泵锁定后写入）。持有方要靠它校验 OPEN 声明的发起方，
    /// 与 ADR-023 TCP 侧「OPEN 声明的 id 必须等于中继观测来源」同一红线。
    peer: Arc<std::sync::Mutex<[u8; NODE_ID_LEN]>>,
    pump: std::sync::Mutex<Option<JoinHandle<()>>>,
}

/// 写半边。持有 `write_tx`（drop 时置 write_closed），共享 pump 生命周期。
pub struct UdpStreamWrite {
    write_tx: Option<SyncSender<Vec<u8>>>,
    shared: Arc<StreamShared>,
}

/// 读半边。持有读队列与未消费完的字节，共享 pump 生命周期。
pub struct UdpStreamRead {
    read_rx: Receiver<Option<Vec<u8>>>,
    pending: Vec<u8>,
    shared: Arc<StreamShared>,
}

/// 最后一个半边离开时停泵并 join（避免 pump 变孤儿线程）。
impl Drop for StreamShared {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(p) = self.pump.lock().unwrap().take() {
            let _ = p.join();
        }
    }
}

/// 泵队列容量（字节块数）。有界背压：写方太快时 `write` 阻塞，不会无限缓存。
const STREAM_QUEUE: usize = 128;
/// 泵每轮读超时。太长会拖高延迟，太短增加系统调用；20 ms 与 TCP 侧一致。
const STREAM_POLL: Duration = Duration::from_millis(20);

impl UdpRelayStream {
    /// 用已注册的 client 构造并启动泵。`peer` 是对端的**中继会话 id**（不是节点 id）。
    pub fn new(
        client: crate::relay_udp::UdpRelayClient,
        peer: [u8; NODE_ID_LEN],
    ) -> Result<Self, String> {
        Self::new_with_cc(client, peer, CongestionConfig::default())
    }

    /// opt-in 拥塞控制版本：`new` 的行为保持不变，只有显式传配置才启用 AIMD/pacing。
    pub fn new_with_cc(
        mut client: crate::relay_udp::UdpRelayClient,
        peer: [u8; NODE_ID_LEN],
        cc: CongestionConfig,
    ) -> Result<Self, String> {
        let (write_tx, write_rx) = mpsc::sync_channel::<Vec<u8>>(STREAM_QUEUE);
        let (read_tx, read_rx) = mpsc::channel::<Option<Vec<u8>>>();
        let stop: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
        let write_closed: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
        let counters: Arc<StreamCounters> = Arc::new(StreamCounters::default());
        let observed: Arc<std::sync::Mutex<[u8; NODE_ID_LEN]>> =
            Arc::new(std::sync::Mutex::new(peer));

        let s = stop.clone();
        let wc = write_closed.clone();
        let c = counters.clone();
        let obs = observed.clone();
        let pump = thread::Builder::new()
            .name("cr-udp-relay-stream".into())
            .spawn(move || {
                let mut core = StreamCore::with_cc(0, cc);
                // peer 允许构造时为全 0（持有方收到 OPEN 前不知发起方 id）：
                // 从第一个 DELIVER 的真实来源锁定，此后所有回包发往该 id。
                let mut peer = peer;
                let peer_known = peer != [0u8; NODE_ID_LEN];
                let mut peer_locked = peer_known;
                let mut app_buf: Vec<u8> = Vec::new();
                let mut closed = false;
                let mut close_deadline: Option<Instant> = None;
                while !s.load(Ordering::Relaxed) {
                    // 1) 收应用写入（非阻塞）。
                    while let Ok(chunk) = write_rx.try_recv() {
                        app_buf.extend_from_slice(&chunk);
                    }
                    let wrote = if app_buf.is_empty() {
                        0
                    } else {
                        let n = core.write(&app_buf);
                        app_buf.drain(..n);
                        n
                    };
                    c.bytes_written.fetch_add(wrote as u64, Ordering::Relaxed);

                    // 2) 取本轮要发的段（首发 + 重传）。
                    for seg in core.take_transmissions(Instant::now()) {
                        let bytes = match encode(&seg) {
                            Ok(b) => b,
                            Err(e) => {
                                *c.last_error.lock().unwrap() = Some(format!("编码线段失败: {e}"));
                                break;
                            }
                        };
                        if let Err(e) = client.send_data(&peer, &bytes) {
                            *c.last_error.lock().unwrap() = Some(format!("发送线段失败: {e}"));
                            c.sender_alive.store(false, Ordering::Relaxed);
                            break;
                        }
                    }
                    c.retransmits.store(core.retransmits(), Ordering::Relaxed);
                    let cc_s = core.cc_stats();
                    c.cc_enabled.store(cc_s.enabled, Ordering::Relaxed);
                    c.cc_cwnd_x100.store(cc_s.cwnd_x100, Ordering::Relaxed);
                    c.cc_srtt_ms.store(cc_s.srtt_ms, Ordering::Relaxed);
                    c.cc_loss_events.store(cc_s.loss_events, Ordering::Relaxed);
                    c.cc_rtt_samples.store(cc_s.rtt_samples, Ordering::Relaxed);
                    c.cc_paced_new.store(cc_s.paced_new_segments, Ordering::Relaxed);
                    c.cc_backpressure
                        .store(cc_s.relay_backpressure_events, Ordering::Relaxed);
                    c.cc_mode.store(cc_s.mode.as_u64(), Ordering::Relaxed);
                    c.cc_base_delay_ms.store(cc_s.base_delay_ms, Ordering::Relaxed);
                    c.cc_queued_delay_ms.store(cc_s.queued_delay_ms, Ordering::Relaxed);
                    c.cc_ledbat_updates.store(cc_s.ledbat_updates, Ordering::Relaxed);
                    c.cc_loss_ignored.store(cc_s.loss_ignored_low_delay, Ordering::Relaxed);
                    c.cc_collapse_resets.store(cc_s.collapse_resets, Ordering::Relaxed);

                    // 3) 收对端帧：只认 DELIVER 里的 ACK / DATA；STATUS 忽略。
                    match client.recv_frame(STREAM_POLL) {
                        Ok(Some(frame)) => match frame.kind {
                            crate::relay_udp::UdpKind::Deliver => {
                                // 持有方模式：peer 构造时为全 0，从首个真实来源锁定。
                                // 中继的 DELIVER.from 已由 MAC 验证身份，可安全用作回包目标。
                                if !peer_locked {
                                    peer = frame.from;
                                    peer_locked = true;
                                    *obs.lock().unwrap() = peer;
                                }
                                match decode(&frame.payload) {
                                    Ok(Segment::Data { seq, ack, sack, body }) => {
                                        if let Some(d) = core.on_data(seq, body, ack, sack) {
                                            c.bytes_read.fetch_add(d.len() as u64, Ordering::Relaxed);
                                            let _ = read_tx.send(Some(d));
                                        }
                                        // 主动回 ACK：纯接收方（没有反向应用数据）也必须
                                        // 确认，否则发方窗口永远不推进（单向传输死锁）。
                                        // DATA 段里的 piggyback 不够——没有反向 DATA 可捎带。
                                        let (a2, s2) = core.ack_state();
                                        if let Ok(ack_bytes) = encode(&Segment::Ack { ack: a2, sack: s2 }) {
                                            let _ = client.send_data(&peer, &ack_bytes);
                                        }
                                    }
                                    Ok(Segment::Ack { ack, sack }) => core.on_ack(ack, sack),
                                    Ok(Segment::Close { ack, sack, .. }) => {
                                        core.on_ack(ack, sack);
                                        core.note_peer_closed();
                                        // 对端停止发送 ≠ 本端停止：本端可能还在 echo 数据。
                                        // 只标记，等自己的写半边关闭且 in-flight 清空后收尾，
                                        // 让双向 drain 完成（half-close 语义）。
                                    }
                                    Err(_) => {} // 非本协议 DATA（如唤醒 JSON），忽略
                                }
                            }
                            crate::relay_udp::UdpKind::Status => {
                                // 中继回执默认只记数，绝不滑动发送窗口。
                                // BandwidthLimited 是「这帧被中继拒了」的明确信号：
                                // 触发一次快重传并拉回 RTO，但不算 ACK、不算丢包。
                                if frame.payload.first().copied()
                                    == Some(UdpStatus::BandwidthLimited.code())
                                {
                                    core.note_relay_backpressure(Instant::now());
                                } else {
                                    core.note_status_ignored();
                                }
                                c.status_ignored.store(core.status_ignored(), Ordering::Relaxed);
                            }
                            _ => {}
                        },
                        Ok(None) => {} // 读超时，正常轮询
                        Err(e) => {
                            *c.last_error.lock().unwrap() = Some(format!("接收线段失败: {e}"));
                            break;
                        }
                    }

                    // 4) 应用已关闭写半边、且没有待发数据 → 发 CLOSE（只发一次）。
                    //    half-close：发完 CLOSE 后**继续收**，直到对端也 CLOSE，
                    //    否则发起方写完就退出，会把对端还没 echo 回来的字节丢掉。
                    if !closed && wc.load(Ordering::Relaxed)
                        && app_buf.is_empty() && core.inflight_len() == 0
                    {
                        let close = encode(&core.close_segment()).unwrap();
                        let _ = client.send_data(&peer, &close);
                        closed = true; // = my_close_sent
                        // 兜底：CLOSE 段是 UDP 尽力投递，可能丢；最多再等这么多时间。
                        close_deadline = Some(Instant::now() + Duration::from_secs(5));
                    }
                    // 收尾条件（half-close）：
                    //   - 对端已 CLOSE 且本端数据已排空（inflight=0、app_buf 空）：双向 drain 完成；
                    //   - 或本端写过 CLOSE 且对端 CLOSE 已到；
                    //   - 或超时兜底（UDP CLOSE 可能丢）。
                    // 不能只等本端 write_closed：收到 CLOSE 的应用可能正卡在等本端 EOF
                    // （如 echo：读到 EOF 才关引擎连接），会形成循环依赖。
                    let drained = app_buf.is_empty() && core.inflight_len() == 0;
                    let peer_done = core.peer_closed() && drained;
                    let both_done = closed && core.peer_closed();
                    let timed_out = close_deadline.map(|d| Instant::now() >= d).unwrap_or(false);
                    if peer_done || both_done || timed_out {
                        // 正常收尾（非超时）且本端还没发过 CLOSE：补发一个，让对端
                        // 不必等 5 s 超时（双向 echo 场景里对端正卡在读 EOF）。
                        if !closed && peer_done {
                            let c = encode(&core.close_segment()).unwrap();
                            let _ = client.send_data(&peer, &c);
                        }
                        break;
                    }
                }
                // 泵退出 = 不再有数据到达：通知读侧 EOF（避免 read 永久阻塞）。
                let _ = read_tx.send(None);
                c.sender_alive.store(false, Ordering::Relaxed);
            })
            .map_err(|e| format!("启动 UDP 流泵失败: {e}"))?;

        let shared = Arc::new(StreamShared {
            stop,
            counters,
            write_closed,
            peer: observed,
            pump: std::sync::Mutex::new(Some(pump)),
        });
        Ok(Self {
            write: UdpStreamWrite { write_tx: Some(write_tx), shared: shared.clone() },
            read: UdpStreamRead { read_rx, pending: Vec::new(), shared },
        })
    }

    /// 拆成独立的读/写两半，供双向 splice 各占一条线程（互不阻塞）。
    pub fn split(self) -> (UdpStreamRead, UdpStreamWrite) {
        (self.read, self.write)
    }

    /// 关闭写半边：通知泵在排空待发数据后发 CLOSE 段。之后 `write` 返回 BrokenPipe。
    pub fn close_write(&mut self) {
        self.write.close_write();
    }

    /// 带超时的精确读（握手用）。
    pub fn read_exact_timeout(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<()> {
        self.read.read_exact_timeout(buf, timeout)
    }

    /// 对端是否已经关闭（读侧 EOF 判据之一）。
    pub fn peer_closed(&self) -> bool {
        self.read.shared.stop.load(Ordering::Relaxed)
    }

    /// 泵是否仍在运行（看板用：登记数不等于活着）。
    pub fn is_alive(&self) -> bool {
        self.read.shared.pump.lock().unwrap().as_ref()
            .map(|p| !p.is_finished()).unwrap_or(false)
    }

    /// 中继观测到的对端会话 id（泵锁定的那个）。构造时已知则立即非零；
    /// 持有方构造时传全 0，收到首个 DELIVER 后才有值。全 0 表示「尚不可确认」。
    pub fn observed_peer(&self) -> [u8; NODE_ID_LEN] {
        *self.write.shared.peer.lock().unwrap()
    }

    /// 拥塞控制快照（看板/量具用；不是跨公网公平性结论）。
    pub fn cc_stats(&self) -> CcStats {
        let c = &self.read.shared.counters;
        let g = |a: &AtomicU64| a.load(Ordering::Relaxed);
        CcStats {
            enabled: c.cc_enabled.load(Ordering::Relaxed),
            cwnd_x100: g(&c.cc_cwnd_x100),
            srtt_ms: g(&c.cc_srtt_ms),
            rttvar_ms: 0,
            loss_events: g(&c.cc_loss_events),
            rtt_samples: g(&c.cc_rtt_samples),
            paced_new_segments: g(&c.cc_paced_new),
            relay_backpressure_events: g(&c.cc_backpressure),
            mode: CcMode::from_u64(c.cc_mode.load(Ordering::Relaxed)),
            base_delay_ms: g(&c.cc_base_delay_ms),
            queued_delay_ms: g(&c.cc_queued_delay_ms),
            ledbat_updates: g(&c.cc_ledbat_updates),
            loss_ignored_low_delay: g(&c.cc_loss_ignored),
            collapse_resets: g(&c.cc_collapse_resets),
        }
    }

    /// 读计数 / 写计数 / 重传 / 最后错误（可观测，证据用）。
    pub fn stats(&self) -> (u64, u64, u64, Option<String>) {
        let c = &self.read.shared.counters;
        (
            c.bytes_read.load(Ordering::Relaxed),
            c.bytes_written.load(Ordering::Relaxed),
            c.retransmits.load(Ordering::Relaxed),
            c.last_error.lock().unwrap().clone(),
        )
    }
}

impl UdpStreamRead {
    /// 读半边也能读 CC 快照（split 后量具/看板仍可观测）。
    pub fn cc_stats(&self) -> CcStats {
        let c = &self.shared.counters;
        let g = |a: &AtomicU64| a.load(Ordering::Relaxed);
        CcStats {
            enabled: c.cc_enabled.load(Ordering::Relaxed),
            cwnd_x100: g(&c.cc_cwnd_x100),
            srtt_ms: g(&c.cc_srtt_ms),
            rttvar_ms: 0,
            loss_events: g(&c.cc_loss_events),
            rtt_samples: g(&c.cc_rtt_samples),
            paced_new_segments: g(&c.cc_paced_new),
            relay_backpressure_events: g(&c.cc_backpressure),
            mode: CcMode::from_u64(c.cc_mode.load(Ordering::Relaxed)),
            base_delay_ms: g(&c.cc_base_delay_ms),
            queued_delay_ms: g(&c.cc_queued_delay_ms),
            ledbat_updates: g(&c.cc_ledbat_updates),
            loss_ignored_low_delay: g(&c.cc_loss_ignored),
            collapse_resets: g(&c.cc_collapse_resets),
        }
    }

    /// 读半边观测到的应用字节数（split 后仍可读；量具/看板用）。
    pub fn bytes_read(&self) -> u64 {
        self.shared.counters.bytes_read.load(Ordering::Relaxed)
    }

    /// 读半边所属流的重传计数（split 后仍可读）。
    pub fn retransmits(&self) -> u64 {
        self.shared.counters.retransmits.load(Ordering::Relaxed)
    }

    /// 带超时的精确读（握手用）。std `Read::read_exact` 没有超时，而对端不回 OPENED
    /// 时必须如实超时失败、不能永久阻塞——这是 attach 不登记空隧道的前提。
    pub fn read_exact_timeout(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<()> {
        let deadline = Instant::now() + timeout;
        let mut filled = 0usize;
        while filled < buf.len() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "读超时"));
            }
            if !self.pending.is_empty() {
                let n = self.pending.len().min(buf.len() - filled);
                buf[filled..filled + n].copy_from_slice(&self.pending[..n]);
                self.pending.drain(..n);
                filled += n;
                continue;
            }
            match self.read_rx.recv_timeout(left.min(Duration::from_millis(100))) {
                Ok(Some(d)) => self.pending = d,
                Ok(None) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "对端关闭")),
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "泵退出"))
                }
            }
        }
        Ok(())
    }
}

impl UdpStreamWrite {
    /// 写半边已经纳入流的字节数（split 后仍可读；量具/看板用）。
    pub fn bytes_written(&self) -> u64 {
        self.shared.counters.bytes_written.load(Ordering::Relaxed)
    }

    /// 写半边所属流的重传计数（split 后仍可读）。
    pub fn retransmits(&self) -> u64 {
        self.shared.counters.retransmits.load(Ordering::Relaxed)
    }

    /// 写半边也能读 CC 快照（split 后看板仍可观测）。
    pub fn cc_stats(&self) -> CcStats {
        let c = &self.shared.counters;
        let g = |a: &AtomicU64| a.load(Ordering::Relaxed);
        CcStats {
            enabled: c.cc_enabled.load(Ordering::Relaxed),
            cwnd_x100: g(&c.cc_cwnd_x100),
            srtt_ms: g(&c.cc_srtt_ms),
            rttvar_ms: 0,
            loss_events: g(&c.cc_loss_events),
            rtt_samples: g(&c.cc_rtt_samples),
            paced_new_segments: g(&c.cc_paced_new),
            relay_backpressure_events: g(&c.cc_backpressure),
            mode: CcMode::from_u64(c.cc_mode.load(Ordering::Relaxed)),
            base_delay_ms: g(&c.cc_base_delay_ms),
            queued_delay_ms: g(&c.cc_queued_delay_ms),
            ledbat_updates: g(&c.cc_ledbat_updates),
            loss_ignored_low_delay: g(&c.cc_loss_ignored),
            collapse_resets: g(&c.cc_collapse_resets),
        }
    }

    /// 关闭写半边：置标志让泵排空后发 CLOSE，并 drop 发送端。
    pub fn close_write(&mut self) {
        self.shared.write_closed.store(true, Ordering::Relaxed);
        self.write_tx.take();
    }
}

impl Drop for UdpStreamWrite {
    fn drop(&mut self) {
        // 写半边离开即视为写结束：置标志让泵有机会发 CLOSE（若读半边还在，pump 继续收）。
        self.shared.write_closed.store(true, Ordering::Relaxed);
        self.write_tx.take();
    }
}

impl Read for UdpStreamRead {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if !self.pending.is_empty() {
                let n = self.pending.len().min(buf.len());
                buf[..n].copy_from_slice(&self.pending[..n]);
                self.pending.drain(..n);
                return Ok(n);
            }
            // 阻塞读（带超时用于检查 stop），使标准 read_exact / splice 能直接用。
            match self.read_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Some(d)) => self.pending = d,
                Ok(None) => return Ok(0), // 对端 CLOSE：EOF
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let pump_alive = self.shared.pump.lock().unwrap().as_ref()
                        .map(|p| !p.is_finished()).unwrap_or(false);
                    if self.shared.stop.load(Ordering::Relaxed) || !pump_alive {
                        return Ok(0);
                    }
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(0), // 泵退出 = EOF
            }
        }
    }
}

impl Write for UdpStreamWrite {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let Some(tx) = &self.write_tx else {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "udp 流写半边已关闭"));
        };
        // 同步语义：send 进队列后，必须等泵把字节纳入 StreamCore 才返回。
        // 否则调用方紧接着 close_write() 时，队列里的数据可能还没被消费，
        // 泵会误判「已排空」而提前发 CLOSE（踩过：wrote=0 就退出）。
        let target = self.shared.counters.bytes_written.load(Ordering::Relaxed) + buf.len() as u64;
        tx.send(buf.to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "udp 流泵已退出"))?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if self.shared.counters.bytes_written.load(Ordering::Relaxed) >= target {
                return Ok(buf.len());
            }
            if self.shared.stop.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "udp 流泵在数据纳入前退出"));
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "udp 流写入纳入超时"));
            }
            thread::sleep(Duration::from_millis(1));
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// UdpRelayStream 自身也实现 Read+Write（委托给两半），供不需要 split 的调用方使用。
impl Read for UdpRelayStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read.read(buf)
    }
}
impl Write for UdpRelayStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.write.flush()
    }
}

// =========================================================================
// OPEN/OPENED 握手（ADR-024 阶段 2）：在可靠字节流上复用 ADR-023 的 CD 帧格式。
// 可靠层已保证有序送达，握手续不再需要自己重传——只有两个定长控制消息。
// =========================================================================

use crate::relay_tunnel::{self as tunnel, ProgressFn, OPEN_TIMEOUT};

/// OPEN 帧：`CD` + channel(4,BE) + FLAG_OPEN(1) + initiator(16) = 23 B 定长。
pub const OPEN_FRAME_LEN: usize = tunnel::HEADER_LEN + NODE_ID_LEN;
/// 持有方确认：单字节 `0xED`。收到即代表"本地引擎已拨通"。
pub const OPENED_BYTE: u8 = 0xED;

/// 发起方：写 OPEN 帧并等 OPENED 字节。未确认就返回错误（不把隧道当可用）。
/// **必须带超时**：对端不在线时 attach 要如实失败回落直连，不能永久阻塞。
pub fn send_open_and_wait(
    stream: &mut UdpRelayStream,
    channel: u32,
    initiator: &[u8; NODE_ID_LEN],
) -> Result<(), String> {
    let frame = tunnel::encode(&tunnel::TunnelFrame::open(channel, initiator))?;
    debug_assert_eq!(frame.len(), OPEN_FRAME_LEN);
    stream.write_all(&frame).map_err(|e| format!("发送 UDP 数据面 OPEN 失败: {e}"))?;
    stream.flush().map_err(|e| format!("flush OPEN 失败: {e}"))?;
    let mut ack = [0u8; 1];
    stream
        .read_exact_timeout(&mut ack, OPEN_TIMEOUT)
        .map_err(|e| format!("等对端 OPENED 失败（{e}），不登记，回落直连"))?;
    if ack[0] != OPENED_BYTE {
        return Err(format!("对端未确认 UDP 数据面（收到 0x{:02x}）", ack[0]));
    }
    Ok(())
}

/// 持有方：读 OPEN 帧，校验后返回 `(channel, initiator)`。非法帧直接拒绝。
/// 带超时，避免对端连上却不发 OPEN 时永久挂住订阅线程。
pub fn recv_open(stream: &mut UdpRelayStream) -> Result<(u32, [u8; NODE_ID_LEN]), String> {
    let mut head = vec![0u8; OPEN_FRAME_LEN];
    stream
        .read_exact_timeout(&mut head, OPEN_TIMEOUT)
        .map_err(|e| format!("读 UDP 数据面 OPEN 失败: {e}"))?;
    let frame = tunnel::decode(&head)?;
    let initiator = frame
        .initiator()
        .ok_or_else(|| "OPEN 帧未携带发起方 id".to_string())?;
    // 红线（与 ADR-023 TCP 侧同一条）：中继不校验归属，端上必须校验——
    // OPEN 声明的发起方 id 必须等于中继观测到的来源会话 id，否则任何人都能
    // 冒名顶替别人的数据面身份让持有者拨本地引擎。
    let observed = stream.observed_peer();
    if observed == [0u8; NODE_ID_LEN] {
        return Err("OPEN 来源不可确认（中继未给出已验签来源），拒绝握手".into());
    }
    if initiator != observed {
        let hx = |b: &[u8; NODE_ID_LEN]| {
            b.iter().map(|x| format!("{x:02x}")).collect::<String>()
        };
        return Err(format!(
            "OPEN 声明的发起方 {} 与中继观测来源 {} 不符，拒绝握手",
            hx(&initiator),
            hx(&observed)
        ));
    }
    Ok((frame.channel, initiator))
}

/// 持有方：拨通本地引擎后回 OPENED 字节。必须先拨通再确认——与 TCP 侧同语义。
pub fn send_opened(stream: &mut UdpRelayStream) -> Result<(), String> {
    stream.write_all(&[OPENED_BYTE]).map_err(|e| format!("回 OPENED 失败: {e}"))?;
    stream.flush().map_err(|e| format!("flush OPENED 失败: {e}"))?;
    Ok(())
}

/// 把 UDP 可靠字节流的读/写两半与本地引擎 TCP 连接双向 splice。
/// 与 `relay_tunnel::splice_blocking_with` 同语义、同进度回调，返回 `(流→引擎, 引擎→流)`。
/// 两半各自占一线程，互不持锁（W59 教训：splice 不能和 pump 抢同一把锁）。
pub fn splice_stream_with(
    read: UdpStreamRead,
    write: UdpStreamWrite,
    upstream: std::net::TcpStream,
    progress: Option<ProgressFn>,
) -> io::Result<(u64, u64)> {
    use std::net::Shutdown;
    let _ = upstream.set_nodelay(true);
    let two = upstream.try_clone()?;
    let p1 = progress.clone();
    let to_up = thread::spawn(move || -> io::Result<u64> {
        let mut up = upstream;
        let mut rd = read;
        let mut n = 0u64;
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            match rd.read(&mut buf) {
                Ok(0) => break,
                Ok(k) => {
                    up.write_all(&buf[..k])?;
                    n += k as u64;
                    if let Some(f) = &p1 { f(true, k as u64); }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => { let _ = up.shutdown(Shutdown::Both); return Err(e); }
            }
        }
        let _ = up.shutdown(Shutdown::Both);
        Ok(n)
    });
    let from_up = thread::spawn(move || -> io::Result<u64> {
        let mut up = two;
        let mut wr = write;
        let mut n = 0u64;
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            match up.read(&mut buf) {
                Ok(0) => break,
                Ok(k) => {
                    wr.write_all(&buf[..k])?;
                    n += k as u64;
                    if let Some(f) = &progress { f(false, k as u64); }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        let _ = wr.flush();
        let _ = up.shutdown(Shutdown::Write);
        Ok(n)
    });
    let a = to_up.join().map_err(|_| io::Error::new(io::ErrorKind::Other, "splice 流→引擎线程 panic"))??;
    let b = from_up.join().map_err(|_| io::Error::new(io::ErrorKind::Other, "splice 引擎→流线程 panic"))??;
    Ok((a, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cc_config() -> CongestionConfig {
        CongestionConfig {
            enabled: true,
            cwnd_init: 4,
            cwnd_min: 1,
            cwnd_max: 16,
            beta: 0.5,
            pacing: false,
            ..CongestionConfig::default()
        }
    }

    /// ADR-025：时延驱动配置（与判据 v2 的 `[arms.ledbat_params]` 一致）。
    fn ledbat_config() -> CongestionConfig {
        CongestionConfig {
            mode: CcMode::Ledbat,
            cwnd_init: 8,
            cwnd_max: 32,
            ledbat_target: LEDBAT_TARGET,
            ledbat_gain: LEDBAT_GAIN,
            ..cc_config()
        }
    }

    #[test]
    fn cc_default_off_preserves_window_and_rto() {
        let t0 = Instant::now();
        let mut core = StreamCore::new(0);
        let wrote = core.write(&vec![7u8; DATA_PAYLOAD_MAX * 40]);
        assert_eq!(wrote, DATA_PAYLOAD_MAX * WINDOW as usize);
        assert_eq!(core.inflight_len(), WINDOW as usize);
        assert_eq!(core.rto(), RTO_INIT);
        let segs = core.take_transmissions(t0);
        assert_eq!(segs.len(), WINDOW as usize);
        core.on_ack_at(WINDOW, 0, t0 + Duration::from_secs(1));
        assert_eq!(core.rto(), RTO_INIT, "关闭 CC 时 ACK 仍重置到固定 RTO_INIT");
        assert_eq!(core.cc_stats().cwnd_x100, (WINDOW as u64) * 100);
        assert_eq!(core.cc_stats().loss_events, 0);
    }

    #[test]
    fn cc_aimd_increases_on_ack_and_halves_on_loss() {
        let t0 = Instant::now();
        let mut core = StreamCore::with_cc(0, cc_config());
        let wrote = core.write(&vec![7u8; DATA_PAYLOAD_MAX * 8]);
        assert_eq!(wrote, DATA_PAYLOAD_MAX * 4, "初始 cwnd=4 必须限制写入");
        assert_eq!(core.take_transmissions(t0).len(), 4);

        core.on_ack_at(4, 0, t0 + Duration::from_millis(40));
        let s = core.cc_stats();
        assert!(s.cwnd_x100 > 400, "ACK 推进必须加性增长: {s:?}");
        assert_eq!(s.rtt_samples, 4, "四个已发送段都应贡献 RTT 样本");
        assert!(s.srtt_ms > 0 && s.rttvar_ms == 0, "快照口径为整毫秒: {s:?}");

        for _ in 0..FAST_RETRANS_ACKS {
            core.on_ack_at(4, 0, t0 + Duration::from_millis(41));
        }
        let s = core.cc_stats();
        assert_eq!(s.loss_events, 1, "三次重复 ACK 应记一次丢失事件");
        assert!(s.cwnd_x100 <= 250, "丢失后必须乘性减小: {s:?}");
        assert!(core.fast_retransmit, "快重传仍要发起，不能只改窗口");
        assert!(core.rto() >= RTO_MIN && core.rto() <= RTO_MAX);
    }

    #[test]
    fn cc_pacing_spreads_new_segments_but_not_retransmits() {
        let t0 = Instant::now();
        let cc = CongestionConfig { pacing: true, ..cc_config() };
        let mut core = StreamCore::with_cc(0, cc);
        core.write(&vec![7u8; DATA_PAYLOAD_MAX * 8]);
        assert_eq!(core.take_transmissions(t0).len(), 1, "pacing 首轮只放一个新段");
        assert!(core.take_transmissions(t0 + Duration::from_millis(1)).is_empty());
        assert_eq!(core.cc_stats().paced_new_segments, 1);
        // 200 ms 恰好触发 seq=0 的超时重传，同时 pacing 放行一个新段；两件事都该发生。
        let out = core.take_transmissions(t0 + Duration::from_millis(200));
        assert!(out.iter().any(|s| matches!(s, Segment::Data { seq: 0, .. })));
        assert_eq!(core.cc_stats().paced_new_segments, 2);

        let out = core.take_transmissions(t0 + Duration::from_secs(2));
        assert!(out.iter().any(|s| matches!(s, Segment::Data { seq: 0, .. })),
            "超时重传必须立即发出，不能被 pacing 挡住");
        assert!(core.retransmits() > 0);
    }

    // ---- ADR-025：LEDBAT（时延驱动）----
    // 这几个用例只验证「机制按书面语义动作」，不产出任何公平性结论；
    // 公平性必须由预注册判据 v2 在真 netem 上跑，单测不能代替。

    #[test]
    fn ledbat_window_grows_toward_target_then_holds_at_max() {
        let t0 = Instant::now();
        let mut core = StreamCore::with_cc(0, ledbat_config());
        core.write(&vec![7u8; DATA_PAYLOAD_MAX * 64]);
        assert_eq!(core.take_transmissions(t0).len(), 8, "初始 cwnd=8 必须限制首发段数");

        core.on_ack_at(8, 0, t0 + Duration::from_millis(40));
        let s = core.cc_stats();
        assert_eq!(s.mode, CcMode::Ledbat);
        assert_eq!(s.base_delay_ms, 40, "基准就是观测到的最小 RTT");
        assert_eq!(s.queued_delay_ms, 0, "空链路排队时延为 0");
        assert_eq!(s.cwnd_x100, 900, "qd=0 时每 RTT 涨满 gain=1 段: {s:?}");
        assert_eq!(s.ledbat_updates, 1);

        // 按 RTT 节奏继续推进：窗口单调涨到 cwnd_max(32) 后必须停住，不能越界。
        let mut ack = 8u32;
        for i in 2..=40u64 {
            let t = t0 + Duration::from_millis(40 * i);
            core.write(&vec![7u8; DATA_PAYLOAD_MAX]);
            core.take_transmissions(t);
            ack += 1;
            core.on_ack_at(ack, 0, t);
        }
        let s = core.cc_stats();
        assert_eq!(s.cwnd_x100, 3200, "必须停在 cwnd_max: {s:?}");
        assert!(s.ledbat_updates >= 24, "窗口应当被反复评估: {s:?}");
        assert_eq!(s.collapse_resets, 0, "没有连续超时就不许复位");
    }

    #[test]
    fn ledbat_does_not_shrink_on_loss_but_aimd_still_halves() {
        let t0 = Instant::now();
        let mut lb = StreamCore::with_cc(0, ledbat_config());
        lb.write(&vec![7u8; DATA_PAYLOAD_MAX * 64]);
        lb.take_transmissions(t0);
        lb.on_ack_at(8, 0, t0 + Duration::from_millis(40));
        let before = lb.cc_stats().cwnd_x100;
        for _ in 0..FAST_RETRANS_ACKS {
            lb.on_ack_at(8, 0, t0 + Duration::from_millis(41));
        }
        let s = lb.cc_stats();
        assert_eq!(s.loss_events, 1, "丢包事件仍要如实记数");
        assert_eq!(s.loss_ignored_low_delay, 1, "丢了但不降窗，必须单独计数");
        assert_eq!(s.cwnd_x100, before, "LEDBAT 不因丢包降窗（EX-1 的失败机理）");

        // 同一场景换成 AIMD：必须照旧乘性减小，证明开关没把老路径改坏。
        let mut aimd = StreamCore::with_cc(0, cc_config());
        aimd.write(&vec![7u8; DATA_PAYLOAD_MAX * 64]);
        aimd.take_transmissions(t0);
        aimd.on_ack_at(4, 0, t0 + Duration::from_millis(40));
        let before_aimd = aimd.cc_stats().cwnd_x100;
        for _ in 0..FAST_RETRANS_ACKS {
            aimd.on_ack_at(4, 0, t0 + Duration::from_millis(41));
        }
        assert!(aimd.cc_stats().cwnd_x100 < before_aimd, "AIMD 必须仍然减半");
    }

    #[test]
    fn ledbat_shrinks_when_queue_delay_exceeds_target() {
        let t0 = Instant::now();
        let mut core = StreamCore::with_cc(0, ledbat_config());
        core.write(&vec![7u8; DATA_PAYLOAD_MAX * 64]);
        core.take_transmissions(t0);
        core.on_ack_at(8, 0, t0 + Duration::from_millis(40));
        let grown = core.cc_stats().cwnd_x100;
        assert_eq!(grown, 900);

        // 下一段让 ACK 晚到 200 ms：基准仍是 40 ms，排队 160 ms > target 100 ms。
        core.write(&vec![7u8; DATA_PAYLOAD_MAX]);
        core.take_transmissions(t0 + Duration::from_millis(41));
        core.on_ack_at(9, 0, t0 + Duration::from_millis(241));
        let s = core.cc_stats();
        assert_eq!(s.base_delay_ms, 40, "基准不因一次慢样本上升");
        assert!(s.queued_delay_ms >= 150, "排队时延必须真实进快照: {s:?}");
        assert!(s.cwnd_x100 < grown, "超过 target 必须回退: {s:?}");
    }

    #[test]
    fn ledbat_collapse_reset_only_after_consecutive_timeouts() {
        let t0 = Instant::now();
        let mut core = StreamCore::with_cc(0, ledbat_config());
        core.write(&vec![7u8; DATA_PAYLOAD_MAX * 64]);
        core.take_transmissions(t0);

        // 先把窗口抬到 13，确保复位动作可观测（cwnd_init=8）。
        // 两个坑：ack 必须 ≤ next_seq（否则当伪造 ACK 忽略）；ACK 要比发送晚一个 RTT，
        // 否则样本恒为 0，基准被压成 0。
        let mut t = t0;
        core.on_ack_at(8, 0, t + Duration::from_millis(40));
        t += Duration::from_millis(40);
        for i in 1..=4u32 {
            core.write(&vec![7u8; DATA_PAYLOAD_MAX]);
            core.take_transmissions(t);
            core.on_ack_at(8 + i, 0, t + Duration::from_millis(40));
            t += Duration::from_millis(40);
        }
        let grown = core.cc_stats().cwnd_x100;
        assert!(grown > 800, "先涨窗: {grown}");

        // 重新灌满窗口再让对端静默：这样才有段可超时。
        core.write(&vec![7u8; DATA_PAYLOAD_MAX * 64]);
        let filled = core.take_transmissions(t);
        assert!(!filled.is_empty(), "必须有在飞段，否则测不到超时");

        // 连续 RTO。同一时刻一批段超时只算一轮，不能一次就复位。
        let mut silent = t + Duration::from_millis(600);
        core.take_transmissions(silent);
        assert_eq!(core.cc_stats().collapse_resets, 0, "第一轮超时不许复位");
        for _ in 1..LEDBAT_RTO_COLLAPSE {
            silent += core.rto();
            core.take_transmissions(silent);
        }
        let s = core.cc_stats();
        assert_eq!(s.collapse_resets, 1, "连续超时必须复位一次: {s:?}");
        assert_eq!(s.cwnd_x100, 800, "复位到 cwnd_init=8: {s:?}");
    }

    #[test]
    fn ledbat_is_off_by_default_and_aimd_is_unchanged() {
        let d = CongestionConfig::default();
        assert_eq!(d.mode, CcMode::Aimd, "缺省必须是 AIMD，v1 行为逐字节不变");
        assert!(!d.enabled);
        assert_eq!(CcMode::from_mode_str("LEDBAT"), CcMode::Ledbat);
        assert_eq!(CcMode::from_mode_str("aimd"), CcMode::Aimd);
        assert_eq!(CcMode::from_mode_str("vegas"), CcMode::Aimd, "未知值由 cr-core 拒绝，这里只兜底");
    }

    #[test]
    fn cc_loss_halves_at_most_once_per_rtt() {
        let t0 = Instant::now();
        let mut core = StreamCore::with_cc(0, cc_config());
        core.write(&vec![7u8; DATA_PAYLOAD_MAX * 8]);
        core.take_transmissions(t0);
        core.on_ack_at(4, 0, t0 + Duration::from_millis(40));
        let before = core.cc_stats().cwnd_x100;
        for _ in 0..FAST_RETRANS_ACKS {
            core.on_ack_at(4, 0, t0 + Duration::from_millis(41));
        }
        let after_first = core.cc_stats().cwnd_x100;
        assert_eq!(core.cc_stats().loss_events, 1);

        // 同一个 RTT 内再丢一次只记事件，不再砍窗口。
        core.fast_retransmit = false;
        for _ in 0..FAST_RETRANS_ACKS {
            core.on_ack_at(4, 0, t0 + Duration::from_millis(42));
        }
        assert_eq!(core.cc_stats().loss_events, 2);
        assert_eq!(core.cc_stats().cwnd_x100, after_first);
        assert!(after_first < before);
    }

    #[test]
    fn relay_backpressure_fast_retransmits_without_loss_or_rto_backoff() {
        let t0 = Instant::now();
        let mut core = StreamCore::with_cc(0, cc_config());
        core.write(&vec![7u8; DATA_PAYLOAD_MAX * 8]);
        core.take_transmissions(t0);
        let cwnd_before = core.cc_stats().cwnd_x100;

        core.note_relay_backpressure(t0 + Duration::from_millis(10));
        let out = core.take_transmissions(t0 + Duration::from_millis(11));
        assert!(out.iter().any(|s| matches!(s, Segment::Data { seq: 0, .. })));
        assert_eq!(core.retransmits(), 1);
        let s = core.cc_stats();
        assert_eq!(s.relay_backpressure_events, 1);
        assert_eq!(s.loss_events, 0, "中继背压不是丢包，不得乘性减窗口");
        assert_eq!(s.cwnd_x100, cwnd_before);
        assert_eq!(core.rto(), RTO_INIT, "背压不得触发 RTO 指数退避");

        // 5 ms 内的重复 STATUS 只记数，避免拒收风暴。
        core.fast_retransmit = false;
        core.note_relay_backpressure(t0 + Duration::from_millis(12));
        assert_eq!(core.cc_stats().relay_backpressure_events, 2);
        assert!(!core.fast_retransmit);
        core.note_relay_backpressure(t0 + Duration::from_millis(20));
        assert!(core.fast_retransmit);
    }

    fn seg_data(seq: u32, s: &str) -> Segment {
        Segment::Data { seq, ack: 0, sack: 0, body: s.as_bytes().to_vec() }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Stage3Action {
        Send,
        Defer,
        Drop,
    }

    #[derive(Debug, Clone, Copy)]
    struct Stage3Decision {
        action: Stage3Action,
        duplicate: bool,
    }

    /// 阶段 3 注入器：xorshift64* 产生确定性序列，按万分比注入
    /// 5% 丢包 / 2% 乱序（延后一轮）/ 1% 重复（同一 app seq 发两次）。
    /// 留在测试模块里，不进生产路径——注入只验证协议面，不改变中继行为。
    #[derive(Debug)]
    struct Stage3Injector {
        seed: u64,
        seen: u64,
        dropped: u64,
        reordered: u64,
        duplicated: u64,
    }

    impl Stage3Injector {
        fn new(seed: u64) -> Self {
            Self { seed, seen: 0, dropped: 0, reordered: 0, duplicated: 0 }
        }

        fn roll(&mut self) -> u32 {
            // xorshift64*；取高 32 位再取模，避免低位的短周期。
            let mut x = self.seed;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.seed = x;
            (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
        }

        fn decide(&mut self) -> Stage3Decision {
            self.seen += 1;
            let r = self.roll() % 10_000;
            let action = if r < 500 {
                self.dropped += 1;
                Stage3Action::Drop
            } else if r < 700 {
                self.reordered += 1;
                Stage3Action::Defer
            } else {
                Stage3Action::Send
            };
            let duplicate = self.roll() % 10_000 < 100;
            if duplicate {
                self.duplicated += 1;
            }
            Stage3Decision { action, duplicate }
        }
    }

    #[test]
    fn segment_wire_roundtrip_and_strict_decode() {
        let s = seg_data(7, "hello");
        let b = encode(&s).unwrap();
        assert_eq!(decode(&b).unwrap(), s);
        assert_eq!(&b[..2], &SEG_MAGIC);
        // magic / flags / kind / body 都对损坏零容忍。
        let mut bad = b.clone();
        bad[0] = b'X';
        assert!(decode(&bad).is_err());
        let mut bad = b.clone();
        bad[3] = 1;
        assert!(decode(&bad).is_err());
        let mut bad = b.clone();
        bad[2] = 0xff;
        assert!(decode(&bad).is_err());
        assert!(decode(&b[..SEG_HEADER - 1]).is_err());
        // ACK 带 body 要拒绝。
        let mut ack = encode(&Segment::Ack { ack: 1, sack: 0 }).unwrap();
        ack.push(0);
        assert!(decode(&ack).is_err());
    }

    #[test]
    fn data_payload_max_fits_udp_budget() {
        let s = Segment::Data { seq: 0, ack: 0, sack: 0, body: vec![0xab; DATA_PAYLOAD_MAX] };
        let b = encode(&s).unwrap();
        assert_eq!(b.len(), SEG_HEADER + DATA_PAYLOAD_MAX);
        // 线段必须装得进 UDP DATA 的 data 部分（1152 B）。
        assert_eq!(b.len(), UDP_MAX_PAYLOAD - NODE_ID_LEN);
        let mut too_big = s.clone();
        if let Segment::Data { body, .. } = &mut too_big {
            body.push(0);
        }
        assert!(encode(&too_big).is_err(), "超长必须拒绝，不静默截断");
    }

    /// 乱序 + 重复：接收核心必须按序交付、重复段丢弃、不产生重复字节。
    #[test]
    fn reorders_and_drops_duplicates_without_corruption() {
        let mut rx = StreamCore::new(0);
        // 段 2 先到（乱序），段 0、1 后到。
        assert!(rx.on_data(2, b"C".to_vec(), 0, 0).is_none());
        assert!(rx.on_data(1, b"B".to_vec(), 0, 0).is_none());
        assert_eq!(rx.on_data(1, b"B".to_vec(), 0, 0), None, "重复段不得交付");
        assert_eq!(rx.dup_dropped(), 1);
        let got = rx.on_data(0, b"A".to_vec(), 0, 0).unwrap();
        assert_eq!(got, b"ABC", "按序拼装，无重复无缺失");
        // 段 2 到齐后再来一次也是重复。
        assert!(rx.on_data(2, b"C".to_vec(), 0, 0).is_none());
        assert_eq!(rx.dup_dropped(), 2);
    }

    /// 缺口驱动 ACK：ack 是期望的下一个 seq，SACK 位图标记乱序已收的段。
    #[test]
    fn gap_driven_ack_and_bounded_sack() {
        let mut rx = StreamCore::new(100);
        rx.on_data(102, b"c".to_vec(), 0, 0); // 乱序
        rx.on_data(104, b"e".to_vec(), 0, 0); // 更远，仍在窗口内
        let (ack, sack) = rx.ack_state();
        assert_eq!(ack, 100, "段 100 没到，累积确认停在 100");
        // bit 2 对应 seq=103? 不：bit i = ack+1+i，所以 seq=102 → bit 1，seq=104 → bit 4。
        assert_ne!(sack & (1 << 1), 0, "seq 102 应被 SACK（bit = seq-ack-1 = 1）");
        assert_ne!(sack & (1 << 3), 0, "seq 104 应被 SACK（bit = seq-ack-1 = 3）");
        assert_eq!(sack & (1 << 0), 0, "seq 101 没收，不该置位");
    }

    /// 有界内存：窗口外的乱序段必须丢弃，incoming 不会无限增长。
    #[test]
    fn out_of_window_segments_are_dropped_bounded() {
        let mut rx = StreamCore::new(0);
        // 塞入远超窗口的乱序段（seq 1000..1100），全在窗口外。
        for s in 1000..1100u32 {
            rx.on_data(s, vec![0u8; 8], 0, 0);
        }
        assert_eq!(rx.incoming_len(), 0, "窗口外段不得缓存");
        assert!(rx.oor_dropped() >= 100);
        // 窗口内的最多存 WINDOW-1 个（expected 本身一到就交付）。
        for s in 1..WINDOW {
            rx.on_data(s, vec![0u8; 8], 0, 0);
        }
        assert!(rx.incoming_len() <= WINDOW as usize);
    }

    /// 超时重传：未确认段在 RTO 之后重发，指数退避且有上限；app seq 不变。
    #[test]
    fn timeout_retransmits_with_backoff_and_stable_app_seq() {
        let mut tx = StreamCore::new(0);
        assert_eq!(tx.write(b"0123456789"), 10);
        let t0 = Instant::now();
        let first = tx.take_transmissions(t0);
        assert_eq!(first.len(), 1, "首轮只发一个段（10 B < 上限）");
        let seq0 = match &first[0] {
            Segment::Data { seq, .. } => *seq,
            _ => panic!("应为 DATA"),
        };
        // 未到 RTO：不重发。
        assert!(tx.take_transmissions(t0 + RTO_INIT / 2).is_empty());
        // 到 RTO：重发，app seq 相同，RTO 退避翻倍。
        let retry = tx.take_transmissions(t0 + RTO_INIT);
        assert_eq!(retry.len(), 1);
        let seq1 = match &retry[0] {
            Segment::Data { seq, .. } => *seq,
            _ => panic!("应为 DATA"),
        };
        assert_eq!(seq0, seq1, "重传必须保持 app seq（UDP 头 seq 另算）");
        assert_eq!(tx.retransmits(), 1);
        assert_eq!(tx.rto(), RTO_INIT * 2);
    }

    /// 快重传：3 次重复 ACK 触发 base 段重发，不必等 RTO。
    #[test]
    fn three_duplicate_acks_trigger_fast_retransmit() {
        let mut tx = StreamCore::new(0);
        tx.write(b"hello");
        let t0 = Instant::now();
        tx.take_transmissions(t0);
        // 连续 3 次 ack=0（没推进）→ 快重传。
        tx.on_ack(0, 0);
        tx.on_ack(0, 0);
        assert_eq!(tx.fast_retransmits(), 0);
        tx.on_ack(0, 0);
        assert_eq!(tx.fast_retransmits(), 1);
        let out = tx.take_transmissions(t0 + Duration::from_millis(1));
        assert_eq!(out.len(), 1, "快重传应立即重发 base 段");
    }

    /// STATUS 不是 ACK：收到多少次 STATUS 都不得滑动发送窗口 / 触发快重传。
    #[test]
    fn status_never_acts_as_ack() {
        let mut tx = StreamCore::new(0);
        tx.write(b"payload");
        let t0 = Instant::now();
        tx.take_transmissions(t0);
        for _ in 0..10 {
            tx.note_status_ignored();
        }
        assert_eq!(tx.status_ignored(), 10);
        assert_eq!(tx.ack_received(), 0, "STATUS 绝不能计入 ACK");
        // _base 仍是 0：再来 3 次"ack=0"才算快重传，STATUS 一次都不该顶替。
        assert_eq!(tx.fast_retransmits(), 0);
    }

    /// 整链纯状态机：sender → receiver 丢包 + 乱序 + 重复 → ACK 回传 → 最终字节一致。
    /// 这是「丢包注入下无损坏、无重复投递」的协议面证据。
    #[test]
    fn reliable_delivery_under_loss_reorder_and_duplication() {
        use std::collections::HashSet;
        let mut tx = StreamCore::new(0);
        let mut rx = StreamCore::new(0);
        let payload: Vec<u8> = (0..64 * 1024).map(|i| (i % 251) as u8).collect();
        let mut written = 0usize;
        let mut expected = Vec::with_capacity(payload.len());
        let mut now = Instant::now();
        let mut rounds = 0usize;
        // 每个 seq **只丢第一次**：模拟真实网络「丢一个包、重传通常能到」。
        // 若按 seq 恒丢（seq%7==0 永远丢），重传保持 app seq 不变就永远到不了，
        // 测试会死循环——那不是可靠层的问题，是注入模型不真实。
        let mut dropped_once: HashSet<u32> = HashSet::new();

        while expected.len() < payload.len() && rounds < 20_000 {
            rounds += 1;
            if written < payload.len() {
                let n = tx.write(&payload[written..]);
                assert!(n > 0 || tx.inflight_len() >= WINDOW as usize,
                        "窗口应有接纳空间: written={written}, inflight={}", tx.inflight_len());
                written += n;
            }
            let segs = tx.take_transmissions(now);
            // 乱序注入：反转本轮段序，让后发的先到；接收侧靠 SACK + 重组缓冲按序交付。
            for seg in segs.into_iter().rev() {
                let Segment::Data { seq, ack, sack, body } = seg else { continue };
                let first_time = !dropped_once.contains(&seq);
                if seq % 7 == 0 && first_time {
                    dropped_once.insert(seq);
                    continue; // 本轮丢包；重传时已在集合里，放行
                }
                if let Some(d) = rx.on_data(seq, body.clone(), ack, sack) {
                    expected.extend_from_slice(&d);
                }
                if seq % 11 == 0 {
                    // 重复投递同一段：不得产生重复字节。
                    if let Some(d) = rx.on_data(seq, body, ack, sack) {
                        expected.extend_from_slice(&d);
                    }
                }
            }
            // ACK 回传（投递后立即取最新接收状态）。
            let (ack, sack) = rx.ack_state();
            tx.on_ack(ack, sack);
            now += Duration::from_millis(25);
        }
        assert_eq!(expected, payload, "丢包/乱序/重复下字节必须逐字节一致且无重复");
        assert!(tx.retransmits() > 0, "注入了丢包，必须发生重传");
        assert!(tx.inflight_len() <= WINDOW as usize, "发送窗口有界");
        assert!(rx.incoming_len() <= WINDOW as usize, "重组窗口有界");
    }

    /// 真 socket：两个 `UdpRelayClient` 经真 `UdpRelayServer`，把 `Segment` 走 DATA/DELIVER，
    /// 两端各由一个 `StreamCore` 驱动，256 KiB 逐字节一致。验证封装在真实中继上闭环，
    /// 且中继的 STATUS 回执被忽略（不当作 ACK）。带注入的版本见本模块最后一个测试。
    #[test]
    fn carries_bytes_over_real_udp_relay() {
        use crate::relay::NODE_ID_LEN;
        use crate::relay_udp::{UdpKind, UdpRelayClient, UdpRelayServer};
        use std::thread;

        let psk = [0x21u8; 32];
        let srv = UdpRelayServer::bind("127.0.0.1:0").unwrap().with_psk(Some(psk));
        let addr = srv.local_addr().unwrap().to_string();
        let h = srv.spawn();
        let timeout = Duration::from_millis(1500);
        let id_a = [0xa1u8; NODE_ID_LEN];
        let id_b = [0xb2u8; NODE_ID_LEN];

        let mut a = UdpRelayClient::connect(&addr, timeout).unwrap().with_psk(Some(psk));
        let mut b = UdpRelayClient::connect(&addr, timeout).unwrap().with_psk(Some(psk));
        a.hello(id_a, timeout).unwrap();
        b.hello(id_b, timeout).unwrap();

        let payload: Vec<u8> = (0..256 * 1024).map(|i| (i % 251) as u8).collect();
        let expected = payload.clone();
        let payload_len = payload.len();

        // 每个方向一个完整 pump：既 send 又 recv（发方必须收 ACK 才能推进窗口）。
        // 这正是阶段 2 接入数据面时的真实形态。
        let sender = thread::spawn(move || {
            let mut tx = StreamCore::new(0);
            let mut written = 0usize;
            let mut status_count = 0u64;
            let deadline = Instant::now() + Duration::from_secs(30);
            // 发方退出条件：全部写入且在飞队列清空（对端已全部确认）。
            while (written < payload.len() || tx.inflight_len() > 0)
                && Instant::now() < deadline
            {
                if written < payload.len() {
                    written += tx.write(&payload[written..]);
                }
                for seg in tx.take_transmissions(Instant::now()) {
                    let bytes = encode(&seg).unwrap();
                    a.send_data(&id_b, &bytes).unwrap();
                }
                // 收对端 ACK / 中继 STATUS。
                match a.recv_frame(Duration::from_millis(5)).unwrap() {
                    Some(f) if f.kind == UdpKind::Deliver => {
                        if let Ok(Segment::Ack { ack, sack }) = decode(&f.payload) {
                            tx.on_ack(ack, sack);
                        }
                    }
                    Some(f) if f.kind == UdpKind::Status => {
                        status_count += 1;
                        tx.note_status_ignored(); // 关键：STATUS 不滑动窗口
                    }
                    _ => {}
                }
            }
            (tx, written, status_count)
        });

        let receiver = thread::spawn(move || {
            let mut rx = StreamCore::new(0);
            let mut got = Vec::with_capacity(256 * 1024);
            let mut status_count = 0u64;
            let deadline = Instant::now() + Duration::from_secs(30);
            while got.len() < 256 * 1024 && Instant::now() < deadline {
                match b.recv_frame(Duration::from_millis(5)).unwrap() {
                    Some(f) if f.kind == UdpKind::Deliver => {
                        if let Ok(Segment::Data { seq, ack, sack, body }) = decode(&f.payload) {
                            if let Some(d) = rx.on_data(seq, body, ack, sack) {
                                got.extend_from_slice(&d);
                            }
                        }
                        let (ack, sack) = rx.ack_state();
                        let ack_seg = encode(&Segment::Ack { ack, sack }).unwrap();
                        b.send_data(&id_a, &ack_seg).unwrap();
                    }
                    Some(f) if f.kind == UdpKind::Status => {
                        status_count += 1;
                        rx.note_status_ignored();
                    }
                    _ => {}
                }
            }
            (got, status_count, rx)
        });

        let (tx, written, tx_status) = sender.join().unwrap();
        let (got, rx_status, rx) = receiver.join().unwrap();
        assert_eq!(written, payload_len, "发方应写完全部字节");
        assert_eq!(got, expected, "经真 UDP 中继的 256 KiB 必须逐字节一致");
        assert!(tx_status > 0, "中继应回过 STATUS 回执");
        assert_eq!(tx.status_ignored(), tx_status, "STATUS 全部被忽略，未计入 ACK");
        assert_eq!(rx.status_ignored(), rx_status);
        assert!(tx.ack_received() > 0, "发方应收到对端 ACK");
        assert!(tx.retransmits() < 1000, "回环不该有大量重传");
        h.stop();
    }

    /// UdpRelayStream（Read+Write 包装层）端到端：两个流经真 UDP 中继，
    /// 用 std Read/Write 语义读回 128 KiB。验证阶段 2 上层 splice 依赖的接口正确。
    #[test]
    fn udp_relay_stream_read_write_end_to_end() {
        use crate::relay_udp::UdpRelayClient;
        use std::io::{Read as _, Write as _};
        use std::thread;

        let psk = [0x22u8; 32];
        let srv = crate::relay_udp::UdpRelayServer::bind("127.0.0.1:0")
            .unwrap()
            .with_psk(Some(psk));
        let addr = srv.local_addr().unwrap().to_string();
        let h = srv.spawn();
        let timeout = Duration::from_millis(1500);
        let id_a = [0xa3u8; NODE_ID_LEN];
        let id_b = [0xb4u8; NODE_ID_LEN];

        let mut ca = UdpRelayClient::connect(&addr, timeout).unwrap().with_psk(Some(psk));
        let mut cb = UdpRelayClient::connect(&addr, timeout).unwrap().with_psk(Some(psk));
        ca.hello(id_a, timeout).unwrap();
        cb.hello(id_b, timeout).unwrap();

        let payload: Vec<u8> = (0..128 * 1024).map(|i| (i % 251) as u8).collect();
        let expected = payload.clone();

        let mut stream_a = UdpRelayStream::new(ca, id_b).unwrap();
        let mut stream_b = UdpRelayStream::new(cb, id_a).unwrap();

        let writer = thread::spawn(move || {
            stream_a.write_all(&payload).unwrap();
            stream_a.close_write();
            stream_a
        });
        let mut got = Vec::with_capacity(128 * 1024);
        let mut buf = [0u8; 8192];
        // read 现在是阻塞语义（带内部超时检查），读到 EOF 才返回 0。
        loop {
            match stream_b.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => got.extend_from_slice(&buf[..n]),
                Err(e) => panic!("读失败: {e}"),
            }
        }
        let stream_a = writer.join().unwrap();
        let (_, a_wrote, _, a_err) = stream_a.stats();
        let (_, _, _, b_err) = stream_b.stats();
        assert!(a_err.is_none() && b_err.is_none(), "流不应有错误: {a_err:?} / {b_err:?}");
        assert_eq!(a_wrote, 128 * 1024, "写计数应等于负载长度");
        assert_eq!(got, expected, "Read+Write 包装层经真中继必须逐字节一致");
        drop(stream_b);
        h.stop();
    }

    /// 阶段 2 核心组件：发起方 OPEN/OPENED 握手 + 双向 splice 经真 UDP 中继闭环。
    /// 持有方「先拨通本地引擎再回 OPENED」，字节真双向穿过。这是上层接线的证据基础。
    #[test]
    fn open_handshake_and_splice_over_real_udp_relay() {
        use crate::relay_udp::UdpRelayClient;
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        let psk = [0x23u8; 32];
        let srv = crate::relay_udp::UdpRelayServer::bind("127.0.0.1:0")
            .unwrap()
            .with_psk(Some(psk));
        let addr = srv.local_addr().unwrap().to_string();
        let h = srv.spawn();
        let timeout = Duration::from_millis(1500);
        // 发起方用派生 id，持有方用 node_id（与上层接线一致）。
        let holder_node = [0xb5u8; NODE_ID_LEN];
        let initiator = crate::relay_tunnel::derive_session_id(&[0xa5u8; NODE_ID_LEN], b"cr-dp-v1");
        let channel = 0x5151u32;

        // 本地「引擎」：echo 服务端。
        let engine = TcpListener::bind("127.0.0.1:0").unwrap();
        let engine_addr = engine.local_addr().unwrap();
        let echo = thread::spawn(move || {
            let (mut s, _) = engine.accept().unwrap();
            let mut buf = [0u8; 8192];
            loop {
                match s.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => { if s.write_all(&buf[..n]).is_err() { break; } }
                    Err(_) => break,
                }
            }
        });

        // 持有方线程：用 node_id 注册，读 OPEN → 拨引擎 → 回 OPENED → splice。
        let holder = {
            let addr = addr.clone();
            thread::spawn(move || {
                let mut cb = UdpRelayClient::connect(&addr, timeout).unwrap().with_psk(Some(psk));
                cb.hello(holder_node, timeout).unwrap();
                // 持有方 peer 传全 0：验证从首个 DELIVER 自动锁定发起方（生产接线即如此）。
                let mut stream = UdpRelayStream::new(cb, [0u8; NODE_ID_LEN]).unwrap();
                let (got_channel, got_initiator) = recv_open(&mut stream).unwrap();
                assert_eq!(got_channel, channel);
                assert_eq!(got_initiator, initiator);
                let sock = std::net::TcpStream::connect(engine_addr).unwrap();
                send_opened(&mut stream).unwrap();
                let (r, w) = stream.split();
                let (to_engine, from_engine) = splice_stream_with(r, w, sock, None).unwrap();
                (to_engine, from_engine)
            })
        };

        // 发起方：用派生 id 注册，OPEN → 等 OPENED → 写数据 → 读回声。
        let mut ca = UdpRelayClient::connect(&addr, timeout).unwrap().with_psk(Some(psk));
        ca.hello(initiator, timeout).unwrap();
        let mut stream = UdpRelayStream::new(ca, holder_node).unwrap();
        send_open_and_wait(&mut stream, channel, &initiator).unwrap();
        let payload: Vec<u8> = (0..64 * 1024).map(|i| (i % 239) as u8).collect();
        stream.write_all(&payload).unwrap();
        stream.close_write();
        let mut got = Vec::new();
        stream.read_to_end(&mut got).unwrap();
        assert_eq!(got, payload, "OPEN+splice 的字节必须原样回来");
        let (to_engine, from_engine) = holder.join().unwrap();
        assert_eq!(to_engine, payload.len() as u64);
        assert_eq!(from_engine, payload.len() as u64);
        echo.join().unwrap();
        h.stop();
    }

    /// ADR-024 阶段 3 第 1 条：1 MiB 经**真 UDP 中继**，在数据/ACK 两个方向注入
    /// 5% 丢包 / 2% 乱序 / 1% 重复。判据是字节逐字节一致、重复段被丢弃、
    /// 收发窗口始终有界，且 STATUS 全程不当 ACK。
    ///
    /// 注入点在 `StreamCore` 与 `UdpRelayClient` 之间（端点侧），中继本身保持生产行为；
    /// 这样既走真 PSK/会话/转发路径，又不为了测试给中继加后门。
    #[test]
    fn real_udp_relay_carries_1mib_under_loss_reorder_and_duplicate_injection() {
        use crate::relay_udp::{UdpKind, UdpRelayClient, UdpRelayServer};
        use std::thread;

        let psk = [0x24u8; 32];
        let srv = UdpRelayServer::bind("127.0.0.1:0")
            .unwrap()
            .with_psk(Some(psk));
        let addr = srv.local_addr().unwrap().to_string();
        let h = srv.spawn();
        let timeout = Duration::from_millis(1500);
        let id_a = [0xa6u8; NODE_ID_LEN];
        let id_b = [0xb6u8; NODE_ID_LEN];

        let mut a = UdpRelayClient::connect(&addr, timeout).unwrap().with_psk(Some(psk));
        let mut b = UdpRelayClient::connect(&addr, timeout).unwrap().with_psk(Some(psk));
        a.hello(id_a, timeout).unwrap();
        b.hello(id_b, timeout).unwrap();

        let payload: Vec<u8> = (0..1024 * 1024).map(|i| (i % 251) as u8).collect();
        let payload_len = payload.len();
        let expected = payload.clone();

        let sender = thread::spawn(move || {
            let mut tx = StreamCore::new(0);
            let mut data_inj = Stage3Injector::new(0x5eed_a1);
            let mut ack_inj = Stage3Injector::new(0x5eed_ac0c);
            let mut deferred: VecDeque<(Segment, bool)> = VecDeque::new();
            let mut deferred_acks: VecDeque<(u32, u64, bool)> = VecDeque::new();
            let mut written = 0usize;
            let mut max_inflight = 0usize;
            let mut status_count = 0u64;
            let deadline = Instant::now() + Duration::from_secs(45);

            while (written < payload_len || tx.inflight_len() > 0) && Instant::now() < deadline {
                // 上一轮被判为「乱序」的段先发；ACK 同理。
                let mut to_send: Vec<(Segment, bool)> = Vec::new();
                while let Some((seg, dup)) = deferred.pop_front() {
                    to_send.push((seg, dup));
                }
                while let Some((ack, sack, dup)) = deferred_acks.pop_front() {
                    tx.on_ack(ack, sack);
                    if dup {
                        tx.on_ack(ack, sack);
                    }
                }

                if written < payload_len {
                    written += tx.write(&payload[written..]);
                }
                for seg in tx.take_transmissions(Instant::now()) {
                    match data_inj.decide() {
                        Stage3Decision { action: Stage3Action::Drop, .. } => {}
                        Stage3Decision { action: Stage3Action::Defer, duplicate } => {
                            deferred.push_back((seg, duplicate));
                        }
                        Stage3Decision { action: Stage3Action::Send, duplicate } => {
                            to_send.push((seg, duplicate));
                        }
                    }
                }
                for (seg, duplicate) in to_send {
                    let bytes = encode(&seg).unwrap();
                    a.send_data(&id_b, &bytes).unwrap();
                    if duplicate {
                        // 同一个 app seq 再发一次；UDP 头 seq 由 send_data 各自递增。
                        a.send_data(&id_b, &bytes).unwrap();
                    }
                }
                max_inflight = max_inflight.max(tx.inflight_len());

                match a.recv_frame(Duration::from_millis(2)).unwrap() {
                    Some(f) if f.kind == UdpKind::Deliver => {
                        if let Ok(Segment::Ack { ack, sack }) = decode(&f.payload) {
                            match ack_inj.decide() {
                                Stage3Decision { action: Stage3Action::Drop, .. } => {}
                                Stage3Decision { action: Stage3Action::Defer, duplicate } => {
                                    deferred_acks.push_back((ack, sack, duplicate));
                                }
                                Stage3Decision { action: Stage3Action::Send, duplicate } => {
                                    tx.on_ack(ack, sack);
                                    if duplicate {
                                        tx.on_ack(ack, sack);
                                    }
                                }
                            }
                        }
                    }
                    Some(f) if f.kind == UdpKind::Status => {
                        status_count += 1;
                        tx.note_status_ignored();
                    }
                    _ => {}
                }
            }
            assert!(
                Instant::now() < deadline,
                "注入传输超时: written={payload_len} 中的 {written}, inflight={}, data_inj={data_inj:?}, ack_inj={ack_inj:?}",
                tx.inflight_len()
            );
            (tx, written, data_inj, ack_inj, max_inflight, status_count)
        });

        let receiver = thread::spawn(move || {
            let mut rx = StreamCore::new(0);
            let mut got = Vec::with_capacity(payload_len);
            let mut max_incoming = 0usize;
            let mut status_count = 0u64;
            let deadline = Instant::now() + Duration::from_secs(45);
            let mut complete_at: Option<Instant> = None;

            while Instant::now() < deadline {
                // 收满之后仍多服务 5 s：注入可能丢掉「收满」那一刻的 ACK，
                // 发方会重传最后几段，此时若接收方立刻退出就会假死。
                if got.len() == payload_len && complete_at.is_none() {
                    complete_at = Some(Instant::now());
                }
                if complete_at
                    .map(|t| Instant::now().duration_since(t) >= Duration::from_secs(5))
                    .unwrap_or(false)
                {
                    break;
                }
                match b.recv_frame(Duration::from_millis(2)).unwrap() {
                    Some(f) if f.kind == UdpKind::Deliver => {
                        if let Ok(Segment::Data { seq, ack, sack, body }) = decode(&f.payload) {
                            if let Some(d) = rx.on_data(seq, body, ack, sack) {
                                got.extend_from_slice(&d);
                            }
                        }
                        // 重复段也回 ACK（接收侧不重复交付，但发方需要确认清窗口）。
                        let (ack, sack) = rx.ack_state();
                        let bytes = encode(&Segment::Ack { ack, sack }).unwrap();
                        b.send_data(&id_a, &bytes).unwrap();
                    }
                    Some(f) if f.kind == UdpKind::Status => {
                        status_count += 1;
                        rx.note_status_ignored();
                    }
                    _ => {}
                }
                max_incoming = max_incoming.max(rx.incoming_len());
            }
            (got, rx, max_incoming, status_count)
        });

        let (tx, written, data_inj, ack_inj, max_inflight, tx_status) = sender.join().unwrap();
        let (got, rx, max_incoming, rx_status) = receiver.join().unwrap();
        h.stop();

        println!(
            "[stage3] data={data_inj:?} ack={ack_inj:?} retransmits={} dup_dropped={} \
             max_inflight={max_inflight} max_incoming={max_incoming} \
             status_ignored={}/{}",
            tx.retransmits(), rx.dup_dropped(), tx.status_ignored(), rx.status_ignored()
        );
        assert_eq!(written, payload_len, "1 MiB 必须全部写入可靠层");
        assert_eq!(got, expected, "5% 丢包 / 2% 乱序 / 1% 重复下仍须逐字节一致");
        assert_eq!(rx.bytes_delivered(), payload_len as u64, "重复投递不得增加交付字节");
        assert!(rx.dup_dropped() > 0, "注入了重复段，接收侧必须丢弃而不是重复交付");
        assert!(data_inj.seen > 100, "样本量太小不足以谈注入率: {data_inj:?}");
        let loss = data_inj.dropped as f64 / data_inj.seen as f64;
        let reorder = data_inj.reordered as f64 / data_inj.seen as f64;
        let duplicate = data_inj.duplicated as f64 / data_inj.seen as f64;
        assert!((loss - 0.05).abs() < 0.015, "丢包率应约 5%: {data_inj:?}");
        assert!((reorder - 0.02).abs() < 0.015, "乱序率应约 2%: {data_inj:?}");
        assert!((duplicate - 0.01).abs() < 0.015, "重复率应约 1%: {data_inj:?}");
        assert!(ack_inj.seen > 0, "ACK 路径也应实际注入: {ack_inj:?}");
        assert!(tx.retransmits() > 0, "注入了丢包，必须发生重传");
        assert!(tx.ack_received() > 0, "发方必须收到对端 ACK");
        assert_eq!(tx.status_ignored(), tx_status, "STATUS 只记数，不得滑窗");
        assert_eq!(rx.status_ignored(), rx_status);
        assert!(max_inflight <= WINDOW as usize, "发送窗口必须有界: {max_inflight}");
        assert!(max_incoming <= WINDOW as usize, "重组窗口必须有界: {max_incoming}");
    }

    /// 红线（ADR-023 同一条规矩搬到 UDP）：中继不校验会话归属，端上必须校验。
    /// 一条格式完全合法、但声称自己是别人的 OPEN，必须被拒——否则任何能访问中继的
    /// 会话都能冒名让持有者拨本地引擎。
    #[test]
    fn recv_open_rejects_initiator_that_differs_from_observed_source() {
        use crate::relay_udp::{UdpRelayClient, UdpRelayServer};

        let psk = [0x25u8; 32];
        let srv = UdpRelayServer::bind("127.0.0.1:0")
            .unwrap()
            .with_psk(Some(psk));
        let addr = srv.local_addr().unwrap().to_string();
        let h = srv.spawn();
        let timeout = Duration::from_millis(1500);
        let holder_id = [0xb7u8; NODE_ID_LEN];
        let attacker_id = [0xa7u8; NODE_ID_LEN];
        let victim_id = [0xc7u8; NODE_ID_LEN];

        let mut ch = UdpRelayClient::connect(&addr, timeout).unwrap().with_psk(Some(psk));
        ch.hello(holder_id, timeout).unwrap();
        let mut ca = UdpRelayClient::connect(&addr, timeout).unwrap().with_psk(Some(psk));
        ca.hello(attacker_id, timeout).unwrap();

        let holder = thread::spawn(move || {
            // 全 0 peer：与生产持有方一致，靠首个 DELIVER 锁定来源。
            let mut stream = UdpRelayStream::new(ch, [0u8; NODE_ID_LEN]).unwrap();
            recv_open(&mut stream).map(|_| ())
        });

        // 攻击者发合法 OPEN，但声明的是「受害者」的身份。
        // 注意要先包成可靠层线段（"US" 头），再作为 UDP DATA 发出——`recv_open` 读的是
        // **流**里的字节，不是裸 UDP 负载。
        let frame = tunnel::encode(&tunnel::TunnelFrame::open(0x4242, &victim_id)).unwrap();
        let seg = encode(&Segment::Data { seq: 0, ack: 0, sack: 0, body: frame }).unwrap();
        ca.send_data(&holder_id, &seg).unwrap();

        let err = holder
            .join()
            .unwrap()
            .expect_err("冒名的 OPEN 必须被拒，不能拨本地引擎");
        assert!(err.contains("不符"), "错误必须说清是「来源不符」而不是笼统失败: {err}");
        h.stop();
    }

    /// 公平性量具 v1-b：**可靠层窗口占用 / RTO 退避 / 重传次数的时间序列**。
    ///
    /// v0 只量中继转发层，看不到可靠层的动态；这个探针用真 `StreamCore` 驱动一条
    /// 64 KiB 流，按虚拟时钟每 25 ms 采一个点，注入 5% 丢包 / 2% 乱序 / 1% 重复
    /// （`Stage3Injector`，与阶段 3 证据同一个注入器）。输出每条样本：
    /// `inflight`（窗口占用，上限 WINDOW=32）、`rto_ms`（指数退避轨迹）、
    /// `retransmits` / `fast_retransmits`（累计）、`bytes_delivered`。
    ///
    /// 另注入 **ACK 路径延迟 30 轮 = 750 ms**：ACK 立即回传时窗口每轮都推进，
    /// RTO 永远停在 200 ms，量不到退避轨迹。750 ms 让同一批段先后在 200 ms
    /// （RTO→400）与 600 ms（RTO→800）两次超时，退避才真实可见。
    ///
    /// 这是 `#[ignore]` 探针：不连网络、不跑时钟 sleep，只打印 JSON 时间序列，
    /// **不对公平性或拥塞安全做 pass 判定**。可靠层仍无拥塞语义，不得据此宣称达标。
    ///
    /// 复现：`cargo test --offline -p cr-nat reliable_window_rto_timeseries_probe -- --ignored --nocapture`
    #[test]
    #[ignore = "window/RTO time-series probe; run with --ignored --nocapture"]
    fn reliable_window_rto_timeseries_probe() {
        let mut tx = StreamCore::new(0);
        let mut rx = StreamCore::new(0);
        let payload: Vec<u8> = (0..64 * 1024).map(|i| (i % 251) as u8).collect();
        let payload_len = payload.len();
        let mut written = 0usize;
        let mut delivered = Vec::with_capacity(payload_len);
        let mut diag = Stage3Injector::new(0x71_6d_65_31); // 数据面注入
        let mut deferred: VecDeque<(Segment, bool)> = VecDeque::new();
        let tick = Duration::from_millis(25);
        let tick_ms = 25u64;
        let ack_delay_rounds = 30u64; // 750 ms ACK 路径延迟，逼出 RTO 退避
        let mut delayed_acks: VecDeque<(u64, u32, u64)> = VecDeque::new(); // (due_round, ack, sack)
        let start = Instant::now();
        let mut now = start;
        let mut rows: Vec<String> = Vec::new();
        let mut rounds = 0u64;
        let max_rounds = 20_000u64;

        while (delivered.len() < payload_len || tx.inflight_len() > 0) && rounds < max_rounds {
            rounds += 1;
            // 到期的 ACK 先入账：延迟 30 轮让 RTO 真的退避一次再被确认。
            while matches!(delayed_acks.front(), Some((due, _, _)) if *due <= rounds) {
                if let Some((_, ack, sack)) = delayed_acks.pop_front() {
                    tx.on_ack(ack, sack);
                }
            }
            // 上一轮被判乱序的段先发。
            while let Some((seg, dup)) = deferred.pop_front() {
                match seg {
                    Segment::Data { seq, ack, sack, body } => {
                        if let Some(d) = rx.on_data(seq, body.clone(), ack, sack) {
                            delivered.extend_from_slice(&d);
                        }
                        if dup {
                            // 真重复只会被接收侧丢弃；若这里返回数据就是重复交付 bug。
                            let again = rx.on_data(seq, body, ack, sack);
                            assert!(again.is_none(), "重复段不得产生重复字节");
                        }
                    }
                    _ => {}
                }
            }
            if written < payload_len {
                written += tx.write(&payload[written..]);
            }
            let segs = tx.take_transmissions(now);
            for seg in segs {
                match diag.decide() {
                    Stage3Decision { action: Stage3Action::Drop, .. } => {}
                    Stage3Decision { action: Stage3Action::Defer, duplicate } => {
                        deferred.push_back((seg, duplicate));
                    }
                    Stage3Decision { action: Stage3Action::Send, duplicate } => {
                        let Segment::Data { seq, ack, sack, body } = seg else { continue };
                        if let Some(d) = rx.on_data(seq, body.clone(), ack, sack) {
                            delivered.extend_from_slice(&d);
                        }
                        if duplicate {
                            // 同 seq 再投一次：接收侧必须丢弃而不是重复交付。
                            let _ = rx.on_data(seq, body, ack, sack);
                        }
                    }
                }
            }
            // ACK 回传：入延迟队列，不立即滑窗（否则量不到 RTO 退避）。
            let (ack, sack) = rx.ack_state();
            delayed_acks.push_back((rounds + ack_delay_rounds, ack, sack));
            // 每轮采一个点：退避发生在少数几轮，跳采会漏掉。
            {
                rows.push(format!(
                    "{{\"t_ms\":{},\"inflight\":{},\"rto_ms\":{},\"retransmits\":{},\
                     \"fast_retransmits\":{},\"bytes_delivered\":{},\"dup_dropped\":{}}}",
                    rounds * tick_ms,
                    tx.inflight_len(),
                    tx.rto().as_millis(),
                    tx.retransmits(),
                    tx.fast_retransmits(),
                    delivered.len(),
                    rx.dup_dropped(),
                ));
            }
            now += tick;
        }
        let series = format!("[{}]", rows.join(","));
        println!(
            "RTO_TIMESERIES_PROBE {{\"payload_bytes\":{payload_len},\"written\":{written},\
             \"delivered\":{},\"complete\":{},\"rounds\":{rounds},\"tick_ms\":25,\
             \"final\":{{\"inflight\":{},\"rto_ms\":{},\"retransmits\":{},\
             \"fast_retransmits\":{},\"dup_dropped\":{}}},\
             \"injection\":{{\"seen\":{},\"dropped\":{},\"reordered\":{},\"duplicated\":{}}},\
             \"ack_delay_rounds\":{ack_delay_rounds},\"series\":{series},\
             \"scope\":\"virtual-clock StreamCore; NOT a network measurement or congestion-safety claim\"}}",
            delivered.len(),
            delivered.len() == payload_len,
            tx.inflight_len(),
            tx.rto().as_millis(),
            tx.retransmits(),
            tx.fast_retransmits(),
            rx.dup_dropped(),
            diag.seen,
            diag.dropped,
            diag.reordered,
            diag.duplicated,
        );
    }
}
