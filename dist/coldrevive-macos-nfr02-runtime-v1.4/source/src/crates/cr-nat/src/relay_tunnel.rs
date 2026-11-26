//! ADR-023：把 BT 字节流放到**既有 TCP 中继帧**上，不新增 FrameKind、不重造 TCP。
//!
//! 为什么不用 UDP 承载：1168 B 上限、服务端不重传；要做字节流就得自己写 seq/ACK/重传，
//! 而 TCP 中继已提供可靠有序交付。腾讯云 `119.29.150.254:7100` 的入向 RST 是该路径特有，
//! 不能推广成「TCP 中继都不可用」——`82.157.68.159:7100` 双端实测 20/20。
//!
//! 线格式（`RELAY` 帧 payload）：`magic(2)="CD" | channel(4,BE) | flags(1) | body`。
//! body 按 16 KiB 分块：大于 ADR-014 的 4096 B 信令阈值，因此会被字节令牌桶计费，
//! 数据面不能伪装成信令逃过预算。
//!
//! # 两种角色（v2.4.92）
//!
//! - **发起方**：自己知道对端 id，`TunnelMux::open(channel, peer)` 后立刻 `send_open`，
//!   并 `wait_opened` 等对端确认。**没收到 OPENED 就不许把隧道当可用**——
//!   「发出去就算通」在 NAT/中继场景里是假阳性制造机。
//! - **接受方**：`TunnelMux::dispatch` 收到 `FLAG_OPEN` 时**就地开出**该 channel 并交回
//!   `RelayTunnel`，由调用方拨本地引擎监听口做 splice。OPEND 前到达的 DATA 会在
//!   channel 队列里等（无界队列），不会丢。
//!
//! `OPEN` 的 body 必须等于中继观测到的来源 id：这是唯一能防止「别人拿我的 channel 号
//! 往里灌字节」的校验点。中继本身不做归属校验，所以这一步必须在端上做。

use crate::relay::{FrameKind, RelayConn, NODE_ID_LEN};
use cr_hash::sha256;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// 帧头：`CD` + channel(4) + flags(1)。
pub const MAGIC: [u8; 2] = *b"CD";
pub const HEADER_LEN: usize = 7;
pub const FLAG_DATA: u8 = 0x01;
pub const FLAG_CLOSE: u8 = 0x02;
/// 发起方 → 接受方：请求开一条 channel，body = 发起方的**中继会话 id**。
pub const FLAG_OPEN: u8 = 0x04;
/// 接受方 → 发起方：channel 已就绪（本地已拨号并接上），body 为空。
pub const FLAG_OPENED: u8 = 0x08;
pub const MAX_BODY: usize = 16 * 1024;
pub const CHANNEL_QUEUE: usize = 64;
/// 泵每轮最多读等待这么久；太长会拖高 BT 出向延迟。
pub const POLL_INTERVAL: Duration = Duration::from_millis(20);
/// 等待对端 `OPENED` 的上限。超过就如实失败，让调用方回落直连。
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelFrame {
    pub channel: u32,
    pub flags: u8,
    pub body: Vec<u8>,
}

impl TunnelFrame {
    pub fn data(channel: u32, body: Vec<u8>) -> Self {
        Self { channel, flags: FLAG_DATA, body }
    }

    pub fn close(channel: u32) -> Self {
        Self { channel, flags: FLAG_CLOSE, body: Vec::new() }
    }

    pub fn open(channel: u32, initiator: &[u8; NODE_ID_LEN]) -> Self {
        Self { channel, flags: FLAG_OPEN, body: initiator.to_vec() }
    }

    pub fn opened(channel: u32) -> Self {
        Self { channel, flags: FLAG_OPENED, body: Vec::new() }
    }

    /// `OPEN` 帧里声明的发起方 id。其它帧返回 `None`。
    pub fn initiator(&self) -> Option<[u8; NODE_ID_LEN]> {
        if self.flags != FLAG_OPEN || self.body.len() != NODE_ID_LEN {
            return None;
        }
        let mut id = [0u8; NODE_ID_LEN];
        id.copy_from_slice(&self.body);
        Some(id)
    }
}

/// 每种 flags 对 body 长度的要求都是**硬约束**：OPEN 少了 id 就没法做归属校验，
/// CLOSE/OPENED 带了 body 就是解析歧义。全都明确拒绝，不猜也不截断。
fn check_shape(flags: u8, body_len: usize) -> Result<(), String> {
    match flags {
        FLAG_DATA => {
            if body_len > MAX_BODY {
                return Err(format!("tunnel body {body_len} B 超过上限 {MAX_BODY} B"));
            }
        }
        FLAG_OPEN => {
            if body_len != NODE_ID_LEN {
                return Err(format!("OPEN body 必须是 {NODE_ID_LEN} B 的发起方 id，实际 {body_len} B"));
            }
        }
        FLAG_CLOSE | FLAG_OPENED => {
            if body_len != 0 {
                return Err(format!("flags 0x{flags:02x} 不允许携带 body（{} B）", body_len));
            }
        }
        other => return Err(format!("未知 tunnel flags 0x{other:02x}")),
    }
    Ok(())
}

/// 编码成 `RELAY` 帧的 payload。超长 body 直接拒绝，不静默截断——
/// 截断会在对端变成「收到但少了一截」，比明确报错难查得多。
pub fn encode(frame: &TunnelFrame) -> Result<Vec<u8>, String> {
    check_shape(frame.flags, frame.body.len())?;
    let mut out = Vec::with_capacity(HEADER_LEN + frame.body.len());
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&frame.channel.to_be_bytes());
    out.push(frame.flags);
    out.extend_from_slice(&frame.body);
    Ok(out)
}

/// 严格解码：magic 与 flags 不对就拒绝，不猜。
pub fn decode(payload: &[u8]) -> Result<TunnelFrame, String> {
    if payload.len() < HEADER_LEN {
        return Err(format!("tunnel 帧太短: {} B", payload.len()));
    }
    if &payload[..2] != &MAGIC {
        return Err("tunnel magic 不符".into());
    }
    let channel = u32::from_be_bytes([payload[2], payload[3], payload[4], payload[5]]);
    let flags = payload[6];
    let body = &payload[HEADER_LEN..];
    check_shape(flags, body.len())?;
    Ok(TunnelFrame { channel, flags, body: body.to_vec() })
}

/// 一个 channel 的状态：谁在用它、往里投递的队列、是否已被对端确认。
struct Channel {
    sink: mpsc::Sender<Vec<u8>>,
    peer: [u8; NODE_ID_LEN],
    opened: Arc<AtomicBool>,
}

/// 入站用无界 channel：CLOSE 的 EOF 不能被「队列暂时满」丢掉。
/// 背压放在出向（sync_channel），那里阻塞只影响写方，不会伪造 EOF。
type Channels = Arc<Mutex<HashMap<u32, Channel>>>;
/// 出向队列的元素带**目的地**：一条中继连接上可能同时服务多个对端（持有者侧订阅
/// 一条连接接所有请求方），目的地不能像早期实现那样固定成构造时的 `peer`。
type OutQueue = SyncSender<([u8; NODE_ID_LEN], TunnelFrame)>;

/// 由调用方驱动的一条中继连接上的复用器。
///
/// 与 [`RelayTunnelSet`] 的区别：本类型**不拥有连接、不起线程**，收发都由调用方在
/// 自己的循环里 `dispatch` / 排空 `Receiver<([u8; NODE_ID_LEN], TunnelFrame)>`。
/// 持有者侧的订阅连接同时承担唤醒转发与数据面，必须共用一条连接（同一 node id 在
/// 中继注册表里只有一个表项，开第二条连接会互相顶掉），所以只能这样拆分。
pub struct TunnelMux {
    outgoing: OutQueue,
    channels: Channels,
}

/// `dispatch` 的判定结果。调用方据此决定「这是数据面帧」还是「继续当业务帧处理」。
///
/// 手写 `Debug`：`Open` 带着一个不可打印的 `RelayTunnel`，而诊断日志只需要 channel 号。
pub enum Inbound {
    /// 不是本协议的帧（magic/flags 不符）：中继上可能混着别的业务帧，不能算错误。
    Ignored,
    Data(u32),
    Close(u32),
    Opened(u32),
    /// 对端请求开 channel：隧道已就绪，调用方拨本地上游并 splice。
    Open { channel: u32, peer: [u8; NODE_ID_LEN], tunnel: RelayTunnel },
    DuplicateOpen(u32),
    /// channel 没开过就来了数据：对端漏发 OPEN 或 channel 号算错，如实计数。
    Unknown(u32),
    /// 来源 id 与该 channel 记录的 peer 不一致：拒绝，防串流/冒充。
    PeerMismatch(u32),
}

impl std::fmt::Debug for Inbound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Inbound::Ignored => write!(f, "Ignored"),
            Inbound::Data(c) => write!(f, "Data({c})"),
            Inbound::Close(c) => write!(f, "Close({c})"),
            Inbound::Opened(c) => write!(f, "Opened({c})"),
            Inbound::Open { channel, peer, .. } => {
                write!(f, "Open {{ channel: {channel}, peer: {:02x?} }}", peer)
            }
            Inbound::DuplicateOpen(c) => write!(f, "DuplicateOpen({c})"),
            Inbound::Unknown(c) => write!(f, "Unknown({c})"),
            Inbound::PeerMismatch(c) => write!(f, "PeerMismatch({c})"),
        }
    }
}

impl TunnelMux {
    pub fn new() -> (Self, Receiver<([u8; NODE_ID_LEN], TunnelFrame)>) {
        let (out_tx, out_rx) = mpsc::sync_channel::<([u8; NODE_ID_LEN], TunnelFrame)>(CHANNEL_QUEUE);
        (Self { outgoing: out_tx, channels: Arc::new(Mutex::new(HashMap::new())) }, out_rx)
    }

    /// 开一个 channel。两端用**同一个** channel id 配对（发起方给，接受方从 OPEN 学）。
    pub fn open(&self, channel: u32, peer: [u8; NODE_ID_LEN]) -> Result<RelayTunnel, String> {
        self.open_with_flag(channel, peer).map(|(t, _)| t)
    }

    fn open_with_flag(&self, channel: u32, peer: [u8; NODE_ID_LEN])
        -> Result<(RelayTunnel, Arc<AtomicBool>), String>
    {
        let (tx, rx) = mpsc::channel();
        let opened = Arc::new(AtomicBool::new(false));
        {
            let mut map = self.channels.lock().unwrap();
            if map.contains_key(&channel) {
                return Err(format!("channel {channel} 已打开"));
            }
            map.insert(channel, Channel { sink: tx, peer, opened: opened.clone() });
        }
        let reader = TunnelReader {
            channel, incoming: rx, pending: None, opened: opened.clone(),
        };
        let writer = TunnelWriter {
            channel, peer, outgoing: self.outgoing.clone(),
            channels: self.channels.clone(), closed: Arc::new(AtomicBool::new(false)),
        };
        Ok((RelayTunnel { tx: writer, rx: reader }, opened))
    }

    /// 处理一帧 `DELIVERED` 的 payload。`from` 必须是中继观测到的来源 id。
    pub fn dispatch(&self, from: [u8; NODE_ID_LEN], payload: &[u8]) -> Inbound {
        let tf = match decode(payload) {
            Ok(t) => t,
            Err(_) => return Inbound::Ignored,
        };
        match tf.flags {
            FLAG_DATA | FLAG_CLOSE => {
                let body = if tf.flags == FLAG_CLOSE { Vec::new() } else { tf.body };
                let map = self.channels.lock().unwrap();
                match map.get(&tf.channel) {
                    Some(c) if c.peer == from => {
                        let _ = c.sink.send(body);
                        if tf.flags == FLAG_CLOSE { Inbound::Close(tf.channel) } else { Inbound::Data(tf.channel) }
                    }
                    Some(_) => Inbound::PeerMismatch(tf.channel),
                    None => Inbound::Unknown(tf.channel),
                }
            }
            FLAG_OPEN => {
                // 声明的发起方必须就是中继看到的来源：中继不校验归属，端上必须校验。
                let Some(claimed) = tf.initiator() else {
                    return Inbound::Ignored; // check_shape 已保证 16 B，这里是防御式兜底
                };
                if claimed != from {
                    return Inbound::PeerMismatch(tf.channel);
                }
                if self.channels.lock().unwrap().contains_key(&tf.channel) {
                    return Inbound::DuplicateOpen(tf.channel);
                }
                match self.open(tf.channel, from) {
                    Ok(tunnel) => Inbound::Open { channel: tf.channel, peer: from, tunnel },
                    Err(_) => Inbound::DuplicateOpen(tf.channel),
                }
            }
            FLAG_OPENED => {
                let map = self.channels.lock().unwrap();
                match map.get(&tf.channel) {
                    Some(c) if c.peer == from => {
                        c.opened.store(true, Ordering::Release);
                        Inbound::Opened(tf.channel)
                    }
                    Some(_) => Inbound::PeerMismatch(tf.channel),
                    None => Inbound::Unknown(tf.channel),
                }
            }
            _ => Inbound::Ignored,
        }
    }

    pub fn channel_count(&self) -> usize {
        self.channels.lock().unwrap().len()
    }

    /// 主动丢弃一个 channel（splice 结束时用；正常路径由 `TunnelWriter::drop` 完成）。
    pub fn close_channel(&self, channel: u32) {
        self.channels.lock().unwrap().remove(&channel);
    }
}

impl Default for TunnelMux {
    fn default() -> Self {
        Self::new().0
    }
}

/// 写半边：所有出向帧都从这里进队列，`peer` 决定目的地。
pub struct TunnelWriter {
    channel: u32,
    peer: [u8; NODE_ID_LEN],
    outgoing: OutQueue,
    channels: Channels,
    closed: Arc<AtomicBool>,
}

impl TunnelWriter {
    pub fn channel(&self) -> u32 {
        self.channel
    }

    pub fn peer(&self) -> [u8; NODE_ID_LEN] {
        self.peer
    }

    fn send(&self, frame: TunnelFrame) -> io::Result<()> {
        if self.closed.load(Ordering::Acquire) {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "tunnel 已关闭"));
        }
        self.outgoing.send((self.peer, frame)).map_err(|_| {
            io::Error::new(io::ErrorKind::BrokenPipe, "relay tunnel 泵已停止")
        })
    }

    /// 发送控制帧（`OPEN` / `OPENED`）。与数据帧共用队列，顺序因此天然一致。
    pub fn send_frame(&self, frame: TunnelFrame) -> io::Result<()> {
        self.send(frame)
    }
}

impl Write for TunnelWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = buf.len().min(MAX_BODY);
        self.send(TunnelFrame::data(self.channel, buf[..n].to_vec()))?;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(()) // 有界队列 + 泵顺序发送，无用户态缓冲残留
    }
}

impl Drop for TunnelWriter {
    fn drop(&mut self) {
        if !self.closed.load(Ordering::Acquire) {
            // 必须先发 CLOSE 再置 closed：`send()` 会拒绝已关闭的 tunnel，
            // 反过来写会让 CLOSE 永远发不出去，对端只能等到超时（踩过）。
            let _ = self.send(TunnelFrame::close(self.channel));
            self.closed.store(true, Ordering::Release);
            self.channels.lock().unwrap().remove(&self.channel);
        }
    }
}

/// 读半边。
pub struct TunnelReader {
    channel: u32,
    incoming: Receiver<Vec<u8>>,
    pending: Option<Vec<u8>>,
    opened: Arc<AtomicBool>,
}

impl TunnelReader {
    pub fn channel(&self) -> u32 {
        self.channel
    }

    /// 等对端的 `OPENED`。**超时就是失败**，不返回「大概通了吧」。
    pub fn wait_opened(&self, timeout: Duration) -> io::Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.opened.load(Ordering::Acquire) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("等对端 OPENED 超时（{} ms）", timeout.as_millis()),
                ));
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    pub fn is_opened(&self) -> bool {
        self.opened.load(Ordering::Acquire)
    }

    /// 非阻塞读：给那些不能起阻塞线程、只能轮询的集成测试/运行时桥接用。
    /// 没有数据时返回 `WouldBlock`，不把「暂时没到」伪装成 EOF。
    pub fn try_read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if let Some(pending) = self.pending.take() {
                if pending.is_empty() {
                    return Ok(0);
                }
                let n = pending.len().min(buf.len());
                buf[..n].copy_from_slice(&pending[..n]);
                if n < pending.len() {
                    self.pending = Some(pending[n..].to_vec());
                }
                return Ok(n);
            }
            match self.incoming.try_recv() {
                Ok(data) => self.pending = Some(data),
                Err(mpsc::TryRecvError::Empty) => {
                    return Err(io::Error::new(io::ErrorKind::WouldBlock, "tunnel 暂无数据"))
                }
                Err(mpsc::TryRecvError::Disconnected) => return Ok(0),
            }
        }
    }
}

impl Read for TunnelReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if let Some(pending) = self.pending.take() {
                if pending.is_empty() {
                    return Ok(0); // 对端 CLOSE
                }
                let n = pending.len().min(buf.len());
                buf[..n].copy_from_slice(&pending[..n]);
                if n < pending.len() {
                    self.pending = Some(pending[n..].to_vec());
                }
                return Ok(n);
            }
            match self.incoming.recv() {
                Ok(data) => self.pending = Some(data),
                // 泵退出或 channel 被摘除：如实 EOF，不返回假数据。
                Err(_) => return Ok(0),
            }
        }
    }
}

/// 一条 BT peer 连接。实现 `Read + Write`，可直接交给 `copy_bidirectional`。
pub struct RelayTunnel {
    // tx 必须先于 rx 析构：tx 的 Drop 负责发 CLOSE 并摘掉 channel 表项，
    // 表项一摘 rx 侧就会读到 EOF，splice 的两条线程才能一起收尾。
    tx: TunnelWriter,
    rx: TunnelReader,
}

impl RelayTunnel {
    pub fn channel(&self) -> u32 {
        self.tx.channel()
    }

    pub fn peer(&self) -> [u8; NODE_ID_LEN] {
        self.tx.peer()
    }

    /// 发起方声明自己是谁（body = 本端**中继注册用的会话 id**，不是对端 id）。
    ///
    /// 接受方会拿它和中继观测到的来源比对，不一致就拒绝——所以这里传错 id 的后果是
    /// 握手失败，而不是悄悄串到别人的 channel 上。
    pub fn send_open(&self, initiator: &[u8; NODE_ID_LEN]) -> io::Result<()> {
        self.tx.send_frame(TunnelFrame::open(self.tx.channel(), initiator))
    }

    /// 接受方确认 channel 可用。必须在首字节数据之前发出。
    pub fn send_opened(&self) -> io::Result<()> {
        self.tx.send_frame(TunnelFrame::opened(self.tx.channel()))
    }

    pub fn wait_opened(&self, timeout: Duration) -> io::Result<()> {
        self.rx.wait_opened(timeout)
    }

    pub fn is_opened(&self) -> bool {
        self.rx.is_opened()
    }

    pub fn try_read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.rx.try_read(buf)
    }

    /// 拆成读/写两半，供双向 splice 各占一条线程。两半不再互相阻塞。
    pub fn split(self) -> (TunnelReader, TunnelWriter) {
        (self.rx, self.tx)
    }
}

impl Read for RelayTunnel {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.rx.read(buf)
    }
}

impl Write for RelayTunnel {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.tx.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.tx.flush()
    }
}

/// 由本类型自管的 `RelayConn` + 后台泵。角色固定的场景（发起方）用它省掉驱动循环。
///
/// 单线程泵负责：收 DELIVERED → 按 channel 分发；发送队列 → `send_relay`。
/// 两个方向都不能阻塞对方：读等待用 20 ms 轮询，出向队列用有界 `sync_channel` 背压。
pub struct RelayTunnelSet {
    peer: [u8; NODE_ID_LEN],
    mux: Arc<TunnelMux>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<(), String>>>,
}

impl RelayTunnelSet {
    /// 占用一条**已连接并 register 过**的 RelayConn。对端 id 必须已知。
    pub fn new(mut conn: RelayConn, peer: [u8; NODE_ID_LEN]) -> Result<Self, String> {
        conn.set_read_timeout(Some(POLL_INTERVAL))?;
        let (mux, out_rx) = TunnelMux::new();
        let mux = Arc::new(mux);
        let stop = Arc::new(AtomicBool::new(false));

        let w_mux = mux.clone();
        let w_stop = stop.clone();
        let worker = thread::Builder::new()
            .name("cr-relay-tunnel".into())
            .spawn(move || -> Result<(), String> {
                while !w_stop.load(Ordering::Relaxed) {
                    match conn.recv_frame_optional() {
                        Ok(Some(frame))
                            if frame.kind == FrameKind::Delivered && frame.node == peer =>
                        {
                            // 来源必须是对端：别人的 DELIVERED 不能冒充本会话。
                            let _ = w_mux.dispatch(peer, &frame.payload);
                        }
                        Ok(Some(_)) => {} // 控制帧/他人帧不属于数据面
                        Ok(None) => {}
                        Err(e) => {
                            // v2.4.93：这条路径以前静默 return Err，线程死了没人知道，
                            // 外表现只剩「tunnels=1 但一个字节也不走」。必须留痕。
                            eprintln!("[relay-tunnel] 数据面收帧失败，隧道中止: {e}");
                            return Err(format!("relay 收帧失败: {e}"));
                        }
                    }
                    // 每轮把出向队列排空，避免 20 ms 轮询把每个 DATA 都拖慢一跳。
                    while let Ok((dst, tf)) = out_rx.try_recv() {
                        let payload = encode(&tf)?;
                        conn.send_relay(&dst, &payload)?;
                    }
                }
                Ok(())
            })
            .map_err(|e| format!("启动 tunnel 泵失败: {e}"))?;

        Ok(Self { peer, mux, stop, worker: Some(worker) })
    }

    /// 开一个 channel。两端用**同一个** channel id 配对（由协商阶段给定）。
    pub fn open(&self, channel: u32) -> Result<RelayTunnel, String> {
        self.mux.open(channel, self.peer)
    }

    pub fn peer(&self) -> [u8; NODE_ID_LEN] {
        self.peer
    }

    pub fn channels(&self) -> usize {
        self.mux.channel_count()
    }

    pub fn is_running(&self) -> bool {
        self.worker.as_ref().map(|w| !w.is_finished()).unwrap_or(false)
    }

    /// 停止泵。已打开的 channel 会收到 EOF。
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.mux.channels.lock().unwrap().clear();
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

impl Drop for RelayTunnelSet {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 派生一个**独立于**订阅会话的中继会话 id。
///
/// 中继注册表按 id 唯一：守护进程的订阅连接已经占用了 `node_id`，数据面若用同一个 id
/// 再连一次，两条连接会互相顶掉（表现为「订阅随机失效」或「数据面莫名其妙不通」）。
/// 所以数据面用 `sha256(node_id || tag)` 的前 16 B 另开一个会话。
pub fn derive_session_id(base: &[u8; NODE_ID_LEN], tag: &[u8]) -> [u8; NODE_ID_LEN] {
    let mut input = Vec::with_capacity(base.len() + tag.len());
    input.extend_from_slice(base);
    input.extend_from_slice(tag);
    let d = sha256(&input);
    let mut out = [0u8; NODE_ID_LEN];
    out.copy_from_slice(&d[..NODE_ID_LEN]);
    out
}

/// 字节进度回调：`(隧道→上游?, 本次字节数)`。
///
/// 存在的理由是可观测性：整条 BT 连接可能跑几十分钟，若计数只在 `splice_blocking`
/// 返回时一次性加上，那么在传输期间 `/health` 永远显示 0——运维会以为没通。
/// 边写边报，计数与真实写入同源。
pub type ProgressFn = Arc<dyn Fn(bool, u64) + Send + Sync>;

/// 把隧道与一个本地 TCP 上游（引擎监听口）对接，两个方向各一条线程。
///
/// 收尾规则（两条都不能省，否则 join 会挂住）：
/// - 隧道方向先 EOF → `shutdown(Both)` 上游，让对侧线程的 `read` 立刻返回 0；
/// - 上游方向先 EOF → `shutdown(Write)` 上游（半关，正常语义），继续把隧道收到的字节写完。
///
/// 返回 `(隧道→上游, 上游→隧道)` 的字节数，全部是真实写入成功的字节。
pub fn splice_blocking(tunnel: RelayTunnel, upstream: TcpStream) -> io::Result<(u64, u64)> {
    splice_blocking_with(tunnel, upstream, None)
}

pub fn splice_blocking_with(
    tunnel: RelayTunnel,
    upstream: TcpStream,
    progress: Option<ProgressFn>,
) -> io::Result<(u64, u64)> {
    let _ = upstream.set_nodelay(true);
    let (mut rx, mut tx) = tunnel.split();
    let two = upstream.try_clone()?;

    let p1 = progress.clone();
    let to_up = thread::spawn(move || -> io::Result<u64> {
        let mut up = upstream;
        let mut n = 0u64;
        let mut buf = vec![0u8; 32 * 1024];
        loop {
            match rx.read(&mut buf) {
                Ok(0) => break,
                Ok(k) => {
                    up.write_all(&buf[..k])?;
                    n += k as u64;
                    if let Some(f) = &p1 {
                        f(true, k as u64);
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    let _ = up.shutdown(Shutdown::Both);
                    return Err(e);
                }
            }
        }
        let _ = up.shutdown(Shutdown::Both);
        Ok(n)
    });

    let to_tunnel = thread::spawn(move || -> io::Result<u64> {
        let mut up = two;
        let mut n = 0u64;
        let mut buf = vec![0u8; 32 * 1024];
        loop {
            match up.read(&mut buf) {
                Ok(0) => break,
                Ok(k) => {
                    tx.write_all(&buf[..k])?;
                    n += k as u64;
                    if let Some(f) = &progress {
                        f(false, k as u64);
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        let _ = tx.flush();
        let _ = up.shutdown(Shutdown::Write);
        drop(tx); // 发 CLOSE 并摘表项 → 对侧读线程收到 EOF
        Ok(n)
    });

    let a = to_up.join()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "splice 隧道→上游线程 panic"))??;
    let b = to_tunnel.join()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "splice 上游→隧道线程 panic"))??;
    Ok((a, b))
}

/// 拨本地上游再 splice。拨号失败如实返回，不建「空隧道」冒充成功。
pub fn dial_and_splice(tunnel: RelayTunnel, upstream: SocketAddr, timeout: Duration)
    -> io::Result<(u64, u64)>
{
    let sock = TcpStream::connect_timeout(&upstream, timeout)?;
    splice_blocking(tunnel, sock)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::RelayClient;
    use crate::relay_server::RelayServer;
    use std::net::TcpListener;

    fn id(b: u8) -> [u8; NODE_ID_LEN] {
        let mut v = [0u8; NODE_ID_LEN];
        v[0] = b;
        v[NODE_ID_LEN - 1] = b;
        v
    }

    #[test]
    fn frame_roundtrip_and_strict_decode() {
        let f = TunnelFrame::data(0x01020304, vec![1, 2, 3, 4]);
        let bytes = encode(&f).unwrap();
        assert_eq!(&bytes[..2], &MAGIC);
        assert_eq!(decode(&bytes).unwrap(), f);
        let close = encode(&TunnelFrame::close(7)).unwrap();
        assert_eq!(decode(&close).unwrap().flags, FLAG_CLOSE);

        // 三种损坏都不能被当成好帧。
        let mut bad = bytes.clone();
        bad[0] = b'X';
        assert!(decode(&bad).is_err());
        let mut bad = bytes.clone();
        bad[6] = 0x7f;
        assert!(decode(&bad).is_err());
        assert!(decode(&bytes[..HEADER_LEN - 1]).is_err());
    }

    #[test]
    fn oversized_body_is_rejected_not_truncated() {
        let f = TunnelFrame::data(1, vec![7u8; MAX_BODY + 1]);
        assert!(encode(&f).is_err());
        let mut bytes = encode(&TunnelFrame::data(1, vec![0u8; MAX_BODY])).unwrap();
        bytes.extend_from_slice(&[0u8; 1]);
        assert!(decode(&bytes).is_err());
    }

    #[test]
    fn handshake_frames_have_fixed_shapes() {
        let a = id(b'A');
        let open = encode(&TunnelFrame::open(3, &a)).unwrap();
        assert_eq!(decode(&open).unwrap().initiator(), Some(a));
        // OPEN 少了 id、OPENED 带了 body：都必须拒绝（否则归属校验就形同虚设）。
        let mut short = open.clone();
        short.truncate(open.len() - 1);
        assert!(decode(&short).is_err());
        let mut fat = encode(&TunnelFrame::opened(3)).unwrap();
        fat.push(0);
        assert!(decode(&fat).is_err());
        assert!(encode(&TunnelFrame { channel: 3, flags: FLAG_DATA, body: vec![] }).is_ok());
    }

    #[test]
    fn open_frame_from_other_source_is_rejected() {
        let (mux, _out) = TunnelMux::new();
        let claimed = id(b'X');
        let payload = encode(&TunnelFrame::open(5, &claimed)).unwrap();
        // 中继报的来源和帧里声明的不是同一个人 → 必须拒绝，不能开 channel。
        match mux.dispatch(id(b'Y'), &payload) {
            Inbound::PeerMismatch(5) => {}
            other => panic!("期望 PeerMismatch，实际 {other:?}"),
        }
        assert_eq!(mux.channel_count(), 0, "被拒的 OPEN 不得留下 channel");
    }

    #[test]
    fn data_on_unknown_channel_is_reported_not_silently_dropped() {
        let (mux, _out) = TunnelMux::new();
        let payload = encode(&TunnelFrame::data(9, b"hi".to_vec())).unwrap();
        match mux.dispatch(id(b'A'), &payload) {
            Inbound::Unknown(9) => {}
            other => panic!("期望 Unknown，实际 {other:?}"),
        }
        // 非本协议 payload（比如唤醒 JSON）不能被当成隧道错误。
        assert!(matches!(mux.dispatch(id(b'A'), br#"{"type":"WAKEUP_REQ"}"#), Inbound::Ignored));
    }

    #[test]
    fn opened_before_timeout_only_when_peer_confirms() {
        let (mux, _out) = TunnelMux::new();
        let a = id(b'A');
        let b = id(b'B');
        let t = mux.open(11, b).unwrap();
        // 没有 OPENED：必须超时失败，不能默认「大概通了」。
        let err = t.wait_opened(Duration::from_millis(80)).expect_err("未确认必须超时");
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);

        let payload = encode(&TunnelFrame::opened(11)).unwrap();
        assert!(matches!(mux.dispatch(b, &payload), Inbound::Opened(11)));
        t.wait_opened(Duration::from_millis(500)).expect("已确认应立即返回");
        // 来源不对的 OPENED 不能把我这条 channel 标成已确认。
        let (mux2, _o2) = TunnelMux::new();
        let t2 = mux2.open(12, b).unwrap();
        let bad = encode(&TunnelFrame::opened(12)).unwrap();
        assert!(matches!(mux2.dispatch(a, &bad), Inbound::PeerMismatch(12)));
        assert!(!t2.is_opened());
    }

    #[test]
    fn derive_session_id_is_stable_and_disjoint_from_base() {
        let base = id(b'A');
        let dp = derive_session_id(&base, b"cr-dp-v1");
        assert_eq!(dp, derive_session_id(&base, b"cr-dp-v1"));
        assert_ne!(dp, base, "数据面会话 id 不能等于订阅会话 id，否则中继注册表互相顶掉");
        assert_ne!(dp, derive_session_id(&base, b"cr-dp-v2"));
    }

    #[test]
    fn max_body_frame_fits_relay_signal_threshold_and_budget() {
        // 16 KiB > SIGNAL_FRAME_MAX(4096)：数据面必须被令牌桶计费，不能逃预算。
        let bytes = encode(&TunnelFrame::data(1, vec![0u8; MAX_BODY])).unwrap();
        assert!(bytes.len() > crate::relay_server::SIGNAL_FRAME_MAX);
        assert!(bytes.len() < crate::relay_server::DEFAULT_MAX_FRAME);
    }

    /// 真 `RelayServer` 上的双向字节流：1 MiB 逐字节一致，且另一 channel 不串字节。
    #[test]
    fn carries_one_mib_over_real_relay_without_channel_crosstalk() {
        let server = RelayServer::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        let addr = format!("127.0.0.1:{port}");
        let timeout = Duration::from_millis(1500);
        let id_a = id(b'A');
        let id_b = id(b'B');

        let mut ca = RelayClient::new(addr.clone()).connect(timeout).unwrap();
        ca.register(&id_a).unwrap();
        let mut cb = RelayClient::new(addr).connect(timeout).unwrap();
        cb.register(&id_b).unwrap();

        let set_a = RelayTunnelSet::new(ca, id_b).unwrap();
        let set_b = RelayTunnelSet::new(cb, id_a).unwrap();
        let ta = set_a.open(42).unwrap();
        let tb = set_b.open(42).unwrap();
        let mut noise_a = set_a.open(43).unwrap();
        let noise_b = set_b.open(43).unwrap();

        let payload: Vec<u8> = (0..1024 * 1024).map(|i| (i % 251) as u8).collect();
        let expected = payload.clone();
        let writer = thread::spawn(move || {
            let mut w = ta;
            w.write_all(&payload).unwrap();
            // 写完先不 drop：EOF 由下面的 close 测试单独验证。
            w
        });

        let (tx, rx) = mpsc::channel();
        let reader_expected = expected.clone();
        let reader = thread::spawn(move || {
            let mut got = vec![0u8; reader_expected.len()];
            let mut r = tb;
            r.read_exact(&mut got).unwrap();
            tx.send(got).unwrap();
            r
        });
        let got = rx.recv_timeout(Duration::from_secs(20)).expect("隧道读超时");
        assert_eq!(got, expected, "经中继的 1 MiB 必须逐字节一致");
        let _ta = writer.join().unwrap();
        let _tb = reader.join().unwrap();

        // 另一条 channel 独立：CLOSE 必须让对端读到 EOF 且只收到本 channel 的字节。
        noise_a.write_all(b"channel-43").unwrap();
        let (tx43, rx43) = mpsc::channel();
        let reader43 = thread::spawn(move || {
            let mut got = Vec::new();
            let mut r = noise_b;
            r.read_to_end(&mut got).unwrap();
            tx43.send(got).unwrap();
        });
        drop(noise_a);
        let got43 = rx43.recv_timeout(Duration::from_secs(5)).expect("channel 43 EOF 未到达");
        assert_eq!(got43, b"channel-43", "channel 串流或 CLOSE 丢失");
        reader43.join().unwrap();

        handle.stop();
    }

    /// v2.4.92 的核心证据：**发起方 OPEN → 接受方就地开出 channel → 双方 splice**
    /// 之后，字节经真中继两端来回，且本地「引擎」看到的是真实 TCP 连接。
    #[test]
    fn open_handshake_then_splice_carries_bytes_over_real_relay() {
        let server = RelayServer::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        let addr = format!("127.0.0.1:{port}");
        let timeout = Duration::from_millis(1500);

        let id_holder = id(b'H');
        let id_dl = id(b'D');

        // 本地「引擎监听口」：收到什么就 echo 回去，模拟分片服务端。
        let engine = TcpListener::bind("127.0.0.1:0").unwrap();
        let engine_addr = engine.local_addr().unwrap();
        let echo = thread::spawn(move || {
            for _ in 0..1 {
                let (mut s, _) = engine.accept().unwrap();
                let mut buf = [0u8; 4096];
                loop {
                    match s.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if s.write_all(&buf[..n]).is_err() { break; }
                        }
                        Err(_) => break,
                    }
                }
            }
        });

        // 持有者侧：订阅连接 + 驱动循环（与 relay_sub 同构）。
        let mut ch = RelayClient::new(addr.clone()).connect(timeout).unwrap();
        ch.register(&id_holder).unwrap();
        ch.set_read_timeout(Some(POLL_INTERVAL)).unwrap();
        let (holder_mux, holder_out) = TunnelMux::new();
        let stop = Arc::new(AtomicBool::new(false));

        // 驱动循环必须在 splice 期间继续跑：`send_opened` 只是进队，真正发出去要靠
        // 下一轮排空。**当初就是把 splice 放在驱动线程里同步做，OPENED 永远发不出去**，
        // 发起方只能等到超时——这正是生产里 relay_sub 必须给每个 channel 起线程的原因。
        let (done_tx, done_rx) = mpsc::channel();
        let w_stop = stop.clone();
        let driver = thread::spawn(move || -> Result<(), io::Error> {
            while !w_stop.load(Ordering::Relaxed) {
                while let Ok((dst, tf)) = holder_out.try_recv() {
                    let p = encode(&tf).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                    ch.send_relay(&dst, &p).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                }
                match ch.recv_frame_optional() {
                    Ok(Some(f)) if f.kind == FrameKind::Delivered => {
                        if let Inbound::Open { tunnel, .. } = holder_mux.dispatch(f.node, &f.payload) {
                            tunnel.send_opened()
                                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                            let tx = done_tx.clone();
                            thread::spawn(move || {
                                let r = dial_and_splice(tunnel, engine_addr, Duration::from_secs(2));
                                let _ = tx.send(r);
                            });
                        }
                    }
                    Ok(_) => {}
                    Err(e) => return Err(io::Error::new(io::ErrorKind::Other, e)),
                }
            }
            Ok(())
        });

        // 下载方：自管泵的一套（发起方角色）。
        let mut cd = RelayClient::new(addr).connect(timeout).unwrap();
        cd.register(&id_dl).unwrap();
        let dl_set = RelayTunnelSet::new(cd, id_holder).unwrap();
        let mut tunnel = dl_set.open(0x5151).unwrap();
        tunnel.send_open(&id_dl).unwrap();
        tunnel.wait_opened(Duration::from_secs(3)).expect("持有者必须确认 OPEN");

        // 发起方写 → 隧道 → 持有者 → 本地引擎 → echo 原路返回。
        let payload: Vec<u8> = (0..64 * 1024).map(|i| (i % 239) as u8).collect();
        tunnel.write_all(&payload).unwrap();
        let mut got = vec![0u8; payload.len()];
        tunnel.read_exact(&mut got).unwrap();
        assert_eq!(got, payload, "经 OPEN+splice 的字节必须逐字节回来的");

        drop(tunnel);
        let (to_up, to_tunnel) = done_rx.recv_timeout(Duration::from_secs(10))
            .expect("splice 未在 10 s 内收尾").expect("splice 应正常收尾");
        assert_eq!(to_up as usize, payload.len());
        assert_eq!(to_tunnel as usize, payload.len());
        stop.store(true, Ordering::Relaxed);
        let _ = driver.join();
        echo.join().unwrap();
        handle.stop();
    }
}
