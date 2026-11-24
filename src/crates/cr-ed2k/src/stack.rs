//! Minimal eD2k download/upload state machines (ADR-011 L2 spike).
//!
//! This is ONE stack serving both directions, as ADR-011 requires: `download()`
//! is capability ② (we fetch from eMule) and `serve()` is capability ④ (eMule
//! fetches from us). Both speak only standard eD2k packets -- P1 zero protocol
//! intrusion is unchanged.
//!
//! Intentionally NOT implemented (documented as scope, not gaps to hide):
//! compression (OP_COMPRESSEDPART), obfuscation, credit / secure identification,
//! source exchange, AICH, >4 GiB I64 packets, server (eD2k network) connection,
//! and NAT traversal. ADR-011 explicitly leaves credit out until M1 measures
//! its cost. The upload queue here is minimal: N slots, FIFO ranking, no credit.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::kad::{ed2k_hash, ed2k_part_hashes, ED2K_PART_SIZE};
use crate::wire::{self as w, Hash16, Hello};

// ------------------------------------------------------------- shared file --

#[derive(Clone)]
pub struct SharedFile {
    pub name: String,
    pub data: Arc<Vec<u8>>,
}

impl SharedFile {
    pub fn new(name: impl Into<String>, data: Vec<u8>) -> Self {
        SharedFile { name: name.into(), data: Arc::new(data) }
    }
    pub fn hash(&self) -> Hash16 {
        ed2k_hash(&self.data)
    }
    /// aMule's `m_iED2KPartHashCount`: includes the empty part hash when the
    /// file size is an exact multiple of PARTSIZE (Red rule, v2.4.31).
    pub fn part_hashes(&self) -> Vec<Hash16> {
        ed2k_part_hashes(&self.data)
    }
    pub fn needs_hashset(&self) -> bool {
        self.data.len() >= ED2K_PART_SIZE
    }
    pub fn link(&self) -> String {
        format!("ed2k://|file|{}|{}|{}|/", self.name, self.data.len(), crate::sha1::hex(&self.hash()))
    }
    pub fn blocks(&self) -> Vec<(u32, u32)> {
        let len = self.data.len() as u32;
        let mut out = Vec::new();
        let mut start = 0u32;
        while start < len {
            let end = (start + w::BLOCKSIZE).min(len);
            out.push((start, end));
            start = end;
        }
        out
    }
}

// ------------------------------------------------------------ upload queue --

/// Minimal FIFO upload queue. `acquire` returns `None` when a slot was granted
/// and `Some(rank)` (1-based) when the requester must wait. No credit system.
#[derive(Debug)]
pub struct UploadQueue {
    pub max_slots: usize,
    active: HashSet<u64>,
    waiting: Vec<u64>,
    next_id: u64,
}

impl UploadQueue {
    pub fn new(max_slots: usize) -> Self {
        UploadQueue { max_slots, active: HashSet::new(), waiting: Vec::new(), next_id: 1 }
    }
    pub fn new_peer(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    pub fn acquire(&mut self, id: u64) -> Option<u16> {
        if self.active.len() < self.max_slots && !self.active.contains(&id) {
            self.active.insert(id);
            self.waiting.retain(|x| *x != id);
            None
        } else {
            if !self.waiting.contains(&id) {
                self.waiting.push(id);
            }
            let pos = self.waiting.iter().position(|x| *x == id).unwrap_or(0);
            Some((pos + 1) as u16)
        }
    }
    pub fn release(&mut self, id: u64) {
        self.active.remove(&id);
    }
    pub fn queued(&self) -> usize {
        self.waiting.len()
    }
}

// ------------------------------------------------------------------ link --

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub at_ms: u64,
    pub direction: &'static str,
    pub protocol: u8,
    pub opcode: u8,
    pub opcode_name: String,
    pub payload_len: usize,
}

pub struct Link {
    pub stream: TcpStream,
    pub log: Vec<LogEntry>,
    pending: Vec<u8>,
}

impl Link {
    pub fn new(stream: TcpStream) -> Self {
        Link { stream, log: Vec::new(), pending: Vec::new() }
    }
    pub fn send(&mut self, protocol: u8, opcode: u8, payload: &[u8]) -> Result<(), String> {
        self.log.push(LogEntry {
            at_ms: 0,
            direction: "send",
            protocol,
            opcode,
            opcode_name: w::opcode_name(opcode).into(),
            payload_len: payload.len(),
        });
        w::send_packet(&mut self.stream, protocol, opcode, payload)
            .map_err(|e| format!("write eD2k frame (peer stopped reading?): {e}"))?;
        Ok(())
    }
    pub fn send_frame(&mut self, f: &[u8]) -> Result<(), String> {
        let p = w::parse_frame(f).map_err(|e| format!("internal frame: {e}"))?;
        self.send(p.protocol, p.opcode, &p.payload)
    }
    pub fn recv(&mut self) -> Result<w::Packet, String> {
        let p = w::read_packet_pending(&mut self.stream, &mut self.pending)?;
        self.log.push(LogEntry {
            at_ms: 0,
            direction: "recv",
            protocol: p.protocol,
            opcode: p.opcode,
            opcode_name: w::opcode_name(p.opcode).into(),
            payload_len: p.payload.len(),
        });
        Ok(p)
    }
}

// --------------------------------------------------------------- download --

#[derive(Default, Debug)]
pub struct DownloadResult {
    pub peer_hello: Option<Hello>,
    pub peer_filename: Option<String>,
    pub hashset_verified: bool,
    pub queue_rank_seen: Option<u32>,
    pub bytes_received: usize,
    pub blocks_received: usize,
    pub hash_matches: bool,
    pub content_matches: bool,
    pub data: Vec<u8>,
    pub log: Vec<LogEntry>,
    pub error: Option<String>,
    pub completed: bool,
}

/// Standard eD2k download handshake and block transfer against any conforming
/// uploader. `expected` supplies the file hash (from the eD2k link / KAD
/// keyword entry) so every returned block and the final content can be checked.
pub fn download(stream: TcpStream, expected: &SharedFile, our_hello: &Hello, timeout: Duration) -> DownloadResult {
    let mut out = DownloadResult::default();
    let deadline = Instant::now() + timeout;
    // 读超时只用于轮询上传队列；半帧由 Link.pending 保留，不会再错位。
    // 5s 足够宽松，避免对真实 aMule 的慢速应答发重复 REQUESTPARTS。
    let mut link = match stream.set_read_timeout(Some(Duration::from_secs(5))) {
        Ok(()) => Link::new(stream),
        Err(e) => {
            out.error = Some(format!("set read timeout: {e}"));
            return out;
        }
    };

    let hash = expected.hash();
    let parts = expected.part_hashes();
    let mut buffer = vec![0u8; expected.data.len()];
    let mut pending = expected.blocks();
    let mut in_flight: Vec<(u32, u32)> = Vec::new();
    // 已收到的字节区间（合并后）。真实客户端的回帧粒度与请求粒度不一致，
    // 只能用「请求区间是否已被已收区间完全覆盖」来判断进度。
    let mut received: Vec<(u32, u32)> = Vec::new();
    let mut handshake_done = false;
    let mut upload_accepted = false;
    let mut last_progress = Instant::now();

    let run = (|| -> Result<(), String> {
        link.send_frame(&w::hello_frame(our_hello))?;
        while Instant::now() < deadline && !out.completed {
            let packet = match link.recv() {
                Ok(p) => p,
                Err(e) => {
                    // Read timeouts are normal while waiting in an upload queue
                    // (rank can be held for minutes). The outer `deadline`
                    // bounds the spin; a clean peer close looks the same here,
                    // so report it as a timeout rather than inventing a cause.
                    if e.contains("read eD2k") && Instant::now() < deadline {
                        // 上传端可能只发送本批请求的一部分（aMule 的流水线会
                        // 边收边追加请求，真实 eMule 也会因磁盘 I/O 拆回应）。
                        // 静默等待会永远卡住已 in_flight 的区间；按批重发。
                        if !in_flight.is_empty() && last_progress.elapsed() >= Duration::from_secs(4) {
                            link.send_frame(&w::request_parts(&hash, &in_flight))?;
                            last_progress = Instant::now();
                        }
                        continue;
                    }
                    return Err(e);
                }
            };
            match packet.opcode {
                w::op::HELLOANSWER => {
                    let h = w::parse_hello(&packet.payload, false)?;
                    out.peer_hello = Some(h);
                    handshake_done = true;
                    link.send_frame(&w::file_request(&hash))?;
                }
                w::op::REQFILENAMEANSWER => {
                    let (h, name) = w::parse_file_name_answer(&packet.payload)?;
                    if h != hash {
                        return Err("REQFILENAMEANSWER carried a different fileID".into());
                    }
                    out.peer_filename = Some(name);
                    if expected.needs_hashset() {
                        link.send_frame(&w::hashset_request(&hash))?;
                    } else {
                        link.send_frame(&w::start_upload_req(&hash))?;
                    }
                }
                w::op::HASHSETANSWER => {
                    let (h, got) = w::parse_hashset_answer(&packet.payload)?;
                    if h != hash {
                        return Err("HASHSETANSWER carried a different fileID".into());
                    }
                    if got != parts {
                        return Err(format!(
                            "hashset mismatch: got {} parts, expected {} (exact-PARTSIZE Red rule)",
                            got.len(), parts.len()
                        ));
                    }
                    out.hashset_verified = true;
                    link.send_frame(&w::start_upload_req(&hash))?;
                }
                w::op::ACCEPTUPLOADREQ => {
                    upload_accepted = true;
                    send_next_batch(&mut link, &mut pending, &mut in_flight, &hash)?;
                }
                w::op::QUEUERANKING | w::op::QUEUERANK => {
                    let rank = if packet.payload.len() >= 4 {
                        u32::from_le_bytes([packet.payload[0], packet.payload[1], packet.payload[2], packet.payload[3]])
                    } else if packet.payload.len() >= 2 {
                        u16::from_le_bytes([packet.payload[0], packet.payload[1]]) as u32
                    } else {
                        0
                    };
                    out.queue_rank_seen = Some(rank);
                }
                w::op::SENDINGPART => {
                    let (h, start, end, data) = w::parse_sending_part(&packet.payload)?;
                    if h != hash {
                        return Err("SENDINGPART carried a different fileID".into());
                    }
                    let s = start as usize;
                    let e = end as usize;
                    if e > buffer.len() || e - s != data.len() {
                        return Err(format!("SENDINGPART range {start}..{end} invalid for {} bytes", buffer.len()));
                    }
                    buffer[s..e].copy_from_slice(&data);
                    out.bytes_received += data.len();
                    out.blocks_received += 1;
                    last_progress = Instant::now();
                    // 真实 aMule 会把一个 184320 的请求拆成多条 10240 字节的
                    // SENDINGPART 送回（实测 `len=10264 range=0..10240`×18）。
                    // 覆盖判定必须是「区间 r 已被已收区间包含」，写成
                    // `r.0 >= start && r.1 <= end` 会把方向弄反，碎片永远清不掉
                    // in_flight，于是 4s 后重发同一批请求、对端最终断开。
                    merge_interval(&mut received, start, end);
                    let done = |r: &(u32, u32)| covered_by(&received, r.0, r.1);
                    in_flight.retain(|r| !done(r));
                    pending.retain(|r| !done(r));
                    if covered_by(&received, 0, buffer.len() as u32) {
                        out.data = buffer.clone();
                        out.hash_matches = ed2k_hash(&buffer) == hash;
                        out.content_matches = buffer == *expected.data;
                        link.send_frame(&w::end_of_download(&hash))?;
                        out.completed = out.hash_matches && out.content_matches;
                        if !out.completed {
                            return Err("downloaded bytes failed ed2k_hash / content check".into());
                        }
                    } else if in_flight.is_empty() {
                        send_next_batch(&mut link, &mut pending, &mut in_flight, &hash)?;
                    }
                }
                w::op::FILEREQANSNOFIL => return Err("peer does not have the file (OP_FILEREQANSNOFIL)".into()),
                w::op::CANCELTRANSFER => return Err("peer cancelled the transfer".into()),
                w::op::OUTOFPARTREQS => return Err("peer reported out-of-part-reqs".into()),
                _ => {
                    // Extended-protocol packets (EMULEINFO, comments, source
                    // exchange, ...) are legal from real clients. Our minimal
                    // stack can safely ignore ones it did not ask for.
                }
            }
        }
        if !out.completed {
            return Err(format!(
                "timeout: handshake={handshake_done} accepted={upload_accepted} pending={} in_flight={}",
                pending.len(), in_flight.len()
            ));
        }
        Ok(())
    })();

    if let Err(e) = run {
        out.error = Some(e);
    }
    out.log = link.log;
    out
}

/// Per-requested-range upload bookkeeping for one connection.
/// 单帧写入的上限。对端不读时不能无限阻塞：见 `serve` 里的说明。
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// 对端静默多久之后认为「上一批数据被困住了」，触发一次自愈重发。
/// 取 3 s：远小于 aMule 自己的 `DOWNLOADTIMEOUT`(100 s)，也大于 LAN 上一个
/// 180 KiB 块的传输时间，不会把正常的流水线停顿误判成丢包。
const IDLE_RESEND: Duration = Duration::from_secs(3);

struct SentRange {
    sent_at: Instant,
    sent_req: u64,
    last_req: u64,
}

/// Split one requested block into wire frames of `chunk` bytes. Real eMule/aMule
/// uploaders send 10 KiB frames even when the peer requested a whole 180 KiB
/// block, so a compliant client must accept that granularity.
fn chunk_range(start: u32, end: u32, chunk: u32) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut at = start;
    while at < end {
        let stop = (at + chunk).min(end);
        out.push((at, stop));
        at = stop;
    }
    out
}

/// Insert `[start,end)` and coalesce with neighbours so coverage checks stay O(n)
/// on a merged list instead of a growing pile of fragments.
fn merge_interval(list: &mut Vec<(u32, u32)>, start: u32, end: u32) {
    if end <= start {
        return;
    }
    list.push((start, end));
    list.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(list.len());
    for (s, e) in list.drain(..) {
        match merged.last_mut() {
            Some(last) if s <= last.1 => {
                if e > last.1 {
                    last.1 = e;
                }
            }
            _ => merged.push((s, e)),
        }
    }
    *list = merged;
}

/// True when `[start,end)` is fully inside one merged received interval.
fn covered_by(list: &[(u32, u32)], start: u32, end: u32) -> bool {
    if end <= start {
        return true;
    }
    list.iter().any(|&(s, e)| s <= start && end <= e)
}

fn send_next_batch(link: &mut Link, pending: &mut Vec<(u32, u32)>, in_flight: &mut Vec<(u32, u32)>, hash: &Hash16) -> Result<(), String> {
    if !in_flight.is_empty() || pending.is_empty() {
        return Ok(());
    }
    let take = pending.len().min(w::WIRE_BLOCKS);
    let batch: Vec<(u32, u32)> = pending[..take].to_vec();
    in_flight.extend_from_slice(&batch);
    link.send_frame(&w::request_parts(hash, &batch))
}

// ----------------------------------------------------------------- upload --

#[derive(Default, Debug)]
pub struct UploadResult {
    pub peer_hello: Option<Hello>,
    pub blocks_sent: usize,
    pub bytes_sent: usize,
    /// 对端停手后触发的自愈重发次数（见 `serve` 的 idle watchdog）。
    /// 这些字节会计入 `bytes_sent`，所以「发送量 > 文件大小」不一定是 bug。
    pub idle_resends: usize,
    pub queue_granted: bool,
    pub queue_rank_sent: Option<u16>,
    /// 单个请求回合内实际送出的最大字节数（即窗口峰值）。这是「有界在途」的
    /// 可观测证据，不是对端已落地的字节数。
    pub max_in_flight_bytes: usize,
    /// 会话结束时仍排队、尚未送出的请求字节数。对端停请求时该值 > 0 是预期行为。
    pub deferred_bytes: usize,
    pub log: Vec<LogEntry>,
    pub error: Option<String>,
}

/// Upload pacing controls for [`serve_with_config`].
///
/// eD2k 没有块级 ACK：对端唯一的「我还在消费」信号就是它继续发
/// `OP_REQUESTPARTS`。因此窗口按**请求回合**计，而不是按帧 ACK 计。
#[derive(Clone, Debug)]
pub struct ServeConfig {
    /// 单个请求回合允许送出的最大 payload 字节数（滑窗上界）。
    /// `None` = 关闭上限（旧行为，仅用于兼容对比）。
    ///
    /// 默认 `3 * BLOCKSIZE`：经典 `OP_REQUESTPARTS` 最多带 3 个 184320 字节的块，
    /// 所以默认值不改变合规回环客户端的字节数，但对「只请求一次就停读」的对端
    /// 给出硬上界，后续字节留在 `deferred_bytes` 里直到新请求到来。
    pub max_in_flight_bytes: Option<usize>,

    /// Opt-in：针对 W25-daemon 观察到的 aMule 2.3.3「收几块后停止读自己 socket」
    /// 的**假说**，不是已验证的修复。开启后：
    ///   ① 每个请求回合最多送一个块（`BLOCKSIZE`），把发送节奏绑在对端的
    ///      `SendBlockRequests`（每完成一块才补一块并重发请求）上；
    ///   ② 3 s idle-resend watchdog 不再主动灌字节——对端不请求时不往它已经
    ///      停滞的内核 Recv-Q 里再塞重传帧。
    /// 默认 `false`：默认路径保留已修好的 watchdog 与写超时，不把未验证的假说
    /// 当成默认行为。真实 aMule 是否有效必须由外部 e2e 判定，本 crate 不宣称。
    pub request_gated_pacing: bool,
}

impl Default for ServeConfig {
    fn default() -> Self {
        ServeConfig {
            max_in_flight_bytes: Some(3 * w::BLOCKSIZE as usize),
            request_gated_pacing: false,
        }
    }
}

/// One requested range that has not been fully handed to the wire yet.
/// `next` is the exclusive byte offset within the file; a range may be
/// partially drained when the per-turn budget runs out, so partial progress
/// is preserved instead of being dropped or double-counted.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingSend {
    start: u32,
    end: u32,
    next: u32,
    first_req: u64,
    last_req: u64,
}

/// `Link::recv` flattens `io::Error` into a string, so error-kind matching has
/// to be textual. Windows renders `WouldBlock` as "A non-blocking socket
/// operation could not be completed immediately", which the original three
/// patterns missed -- there a 1 s poll timeout was read as "peer closed" and
/// `serve` returned immediately, so the idle watchdog never ran on Windows.
/// Keep the peer-close branch strict: only known timeout wordings match.
fn is_read_timeout(err: &str) -> bool {
    let e = err.to_ascii_lowercase();
    e.contains("timed out")
        || e.contains("timedout")
        || e.contains("wouldblock")
        || e.contains("would block")
        || e.contains("resource temporarily unavailable")
        || e.contains("non-blocking socket operation")
        || e.contains("os error 10035")
        || e.contains("os error 10060")
}

/// Max bytes we may emit in one request turn.
fn turn_budget(config: &ServeConfig) -> usize {
    let base = config.max_in_flight_bytes.unwrap_or(usize::MAX);
    if config.request_gated_pacing {
        base.min(w::BLOCKSIZE as usize)
    } else {
        base
    }
}

/// Queue a requested range for sending. Returns true when a new entry was
/// added; a re-request only refreshes `last_req` so the caller can decide
/// whether to rewind it for a resend.
fn queue_range(pending: &mut VecDeque<PendingSend>, start: u32, end: u32, seq: u64) -> bool {
    if end <= start {
        return false;
    }
    if let Some(p) = pending.iter_mut().find(|p| p.start == start && p.end == end) {
        p.last_req = seq;
        return false;
    }
    pending.push_back(PendingSend { start, end, next: start, first_req: seq, last_req: seq });
    true
}

/// Rewind an already queued range to its start so it can be re-sent.
fn rewind_range(pending: &mut VecDeque<PendingSend>, start: u32, end: u32) {
    if let Some(p) = pending.iter_mut().find(|p| p.start == start && p.end == end) {
        p.next = start;
    }
}

/// Split queued ranges into wire frames under `budget`, keeping the existing
/// `UPLOAD_CHUNK` granularity. A partially consumed range is pushed back at
/// the front with `next` advanced; no byte is lost and none is sent twice in
/// the same turn.
fn plan_sends(pending: &mut VecDeque<PendingSend>, budget: usize, chunk: u32) -> (Vec<(u32, u32)>, usize) {
    let mut frames = Vec::new();
    let mut used = 0usize;
    while used < budget {
        let Some(mut p) = pending.pop_front() else { break };
        if p.next >= p.end {
            continue; // already fully drained
        }
        while p.next < p.end {
            let want = (p.next + chunk).min(p.end) - p.next;
            if want as usize > budget - used {
                break;
            }
            frames.push((p.next, p.next + want));
            p.next += want;
            used += want as usize;
        }
        if p.next < p.end {
            pending.push_front(p);
            break; // budget exhausted mid-range
        }
    }
    (frames, used)
}

/// Emit planned frames through `link` and update the honest byte counters.
fn emit_planned(
    link: &mut Link,
    frames: &[(u32, u32)],
    hash: &Hash16,
    file: &SharedFile,
    out: &mut UploadResult,
) -> Result<usize, String> {
    let mut bytes = 0usize;
    for &(cs, ce) in frames {
        let (s, e) = (cs as usize, ce as usize);
        link.send_frame(&w::sending_part(hash, cs, ce, &file.data[s..e]))?;
        out.blocks_sent += 1;
        out.bytes_sent += e - s;
        bytes += e - s;
    }
    Ok(bytes)
}

/// Standard eD2k upload side. `files` is the whole (public, per ADR-011 D1)
/// shared directory; lookup is by real ed2k fileID. A slot is held for the
/// connection's lifetime and released on exit.
///
/// Uses [`ServeConfig::default`]; call [`serve_with_config`] to opt into the
/// experimental pacing hypothesis.
pub fn serve(stream: TcpStream, files: &[SharedFile], our_hello: &Hello, queue: &Arc<Mutex<UploadQueue>>, timeout: Duration) -> UploadResult {
    serve_with_config(stream, files, our_hello, queue, timeout, ServeConfig::default())
}

/// [`serve`] with explicit pacing controls. See [`ServeConfig`].
pub fn serve_with_config(
    stream: TcpStream,
    files: &[SharedFile],
    our_hello: &Hello,
    queue: &Arc<Mutex<UploadQueue>>,
    timeout: Duration,
    config: ServeConfig,
) -> UploadResult {
    let mut out = UploadResult::default();
    let deadline = Instant::now() + timeout;
    // 真实客户端在多条连接之间可能短暂停顿；5s 会把仍有用的连接误关。
    // 但仍受整次任务 timeout 约束，不能无限等。
    // 1 s 的读超时不是为了「判定断开」，而是给 idle watchdog 一个时间刻度：
    // `wire::read_packet_pending` 会把半帧留在 pending 里，超时本身不丢字节。
    let _ = stream.set_read_timeout(Some(timeout.min(Duration::from_secs(1))));
    // 写操作也必须有上限。W25 实测：aMule 2.3.3 在 6%–38% 之间会**停止读取自己的
    // socket**（内核 Recv-Q 里留着几百 KB 不动，客户端状态仍是 Downloading），
    // 这时我们会在 `send_frame` 里阻塞写，连 idle 重传的看门狗都轮不到执行，
    // 线程与连接一起被钉死。宁可 10s 后如实报错、让客户端重连换一条连接。
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let mut link = Link::new(stream);
    let peer_id = queue.lock().map(|mut q| q.new_peer()).unwrap_or(0);
    let mut held_slot = false;
    let mut current_file: Option<SharedFile> = None;
    // 注意（2026-11-24 反向 A/B 更正）：真 aMule 上传端发的是
    // OP_COMPRESSEDPART(0x40)/protocol 0xC5，约 1,070 B 的 zlib 帧对应
    // 184,320 B 块；不是 10,240 B 的 OP_SENDINGPART(0x46)。当前实现仍用
    // 0x46/10,240 B，aMule 下载端会在接近完成时停止请求（97.6% 停住）。
    // 旧注释「aMule 按 10240 字节一帧」是误判，保留此更正以防再次走偏。
    // 记账按「请求区间」而不是帧：同一区间重复请求多是流水线，但若连续多轮
    // 仍在 pending 里，就按重传再发一次（eD2k 无块级 ACK，只能这样自愈）。
    let mut sent_ranges: HashMap<(u32, u32), SentRange> = HashMap::new();
    let mut request_seq: u64 = 0;
    // 对端停手时的自愈（W25 实测 aMule 2.3.3 停在 10.1% 不动：窗口填满后
    // 既不确认也不再请求，eD2k 又没有块级 ACK）。读超时不再意味着「没事发生」，
    // 而是检查「上一批请求的数据是不是被困住了」。
    let mut last_request_at = Instant::now();
    let mut last_ranges: Vec<(u32, u32)> = Vec::new();
    let mut idle_resend_bytes: usize = 0;
    // 请求驱动窗口：对端请求但未送完的区间留在这里，只有新的 REQUESTPARTS
    // （或默认配置下的 idle watchdog）才授权继续送出。这是我们的「有界在途」。
    let mut pending_sends: VecDeque<PendingSend> = VecDeque::new();

    let run = (|| -> Result<(), String> {
        while Instant::now() < deadline {
            let packet =                 match link.recv() {
                    Ok(p) => p,
                    // 读超时只表示「本轮没凑齐」，pending 里已有字节要留着；
                    // 不能当作对端关闭，也不能 fallthrough 去解半帧。
                    Err(e) if is_read_timeout(&e) => {
                        // 对端静默 ≥IDLE_RESEND：把它最后请求过的区间再发一遍。
                        // 只发「最后一批请求」，且总重发量封顶 2× 文件大小，
                        // 这样重传是自愈手段，不会退化成灌包。
                        if last_request_at.elapsed() >= IDLE_RESEND && !last_ranges.is_empty() {
                            // Opt-in 假说：对端停读时不主动灌字节，等它下一次请求。
                            // 默认配置保持原有 watchdog，不撤销已修好的自愈。
                            if !config.request_gated_pacing {
                                if let Some(f) = current_file.clone() {
                                    let cap = f.data.len().saturating_mul(2);
                                    if idle_resend_bytes < cap {
                                        let budget = turn_budget(&config).min(cap - idle_resend_bytes);
                                        for (start, end) in last_ranges.clone() {
                                            if end <= start || end as usize > f.data.len() {
                                                continue;
                                            }
                                            sent_ranges.insert((start, end), SentRange {
                                                sent_at: Instant::now(),
                                                sent_req: request_seq,
                                                last_req: request_seq,
                                            });
                                            if !queue_range(&mut pending_sends, start, end, request_seq) {
                                                rewind_range(&mut pending_sends, start, end);
                                            }
                                        }
                                        let (frames, used) = plan_sends(&mut pending_sends, budget, w::UPLOAD_CHUNK);
                                        let bytes = emit_planned(&mut link, &frames, &f.hash(), &f, &mut out)?;
                                        idle_resend_bytes += bytes;
                                        if bytes > 0 {
                                            out.idle_resends += 1;
                                        }
                                        out.max_in_flight_bytes = out.max_in_flight_bytes.max(used);
                                    }
                                }
                            }
                            last_request_at = Instant::now();
                        }
                        continue;
                    }
                    Err(e) if e.contains("read eD2k") => return Ok(()), // peer closed
                    Err(e) => return Err(e),
                };
            match packet.opcode {
                w::op::HELLO => {
                    out.peer_hello = Some(w::parse_hello(&packet.payload, true)?);
                    link.send_frame(&w::hello_answer_frame(our_hello))?;
                }
                w::op::REQUESTFILENAME => {
                    if packet.payload.len() < 16 {
                        return Err("short OP_REQUESTFILENAME".into());
                    }
                    let mut h = [0u8; 16];
                    h.copy_from_slice(&packet.payload[..16]);
                    match files.iter().find(|f| f.hash() == h) {
                        Some(f) => {
                            current_file = Some(f.clone());
                            link.send_frame(&w::file_name_answer(&h, &f.name))?;
                        }
                        None => link.send_frame(&w::no_such_file(&h))?,
                    }
                }
                w::op::SETREQFILEID => {
                    // 真实 aMule 对 >= PARTSIZE 的文件用 0x4F 选文件并等 0x50。
                    // 缺这一支时它收到 REQFILENAMEANSWER 后就停住，不继续
                    // HASHSETREQUEST / STARTUPLOADREQ（W25-daemon 实测）。
                    if packet.payload.len() < 16 {
                        return Err("short OP_SETREQFILEID".into());
                    }
                    let mut h = [0u8; 16];
                    h.copy_from_slice(&packet.payload[..16]);
                    match files.iter().find(|f| f.hash() == h) {
                        Some(f) => {
                            current_file = Some(f.clone());
                            link.send_frame(&w::file_status(&h))?;
                        }
                        None => link.send_frame(&w::no_such_file(&h))?,
                    }
                }
                w::op::HASHSETREQUEST => {
                    if packet.payload.len() < 16 {
                        return Err("short OP_HASHSETREQUEST".into());
                    }
                    let mut h = [0u8; 16];
                    h.copy_from_slice(&packet.payload[..16]);
                    if let Some(f) = files.iter().find(|f| f.hash() == h) {
                        link.send_frame(&w::hashset_answer(&h, &f.part_hashes()))?;
                    }
                }
                w::op::STARTUPLOADREQ => {
                    let mut q = queue.lock().map_err(|_| "upload queue poisoned".to_string())?;
                    match q.acquire(peer_id) {
                        None => {
                            out.queue_granted = true;
                            held_slot = true;
                            link.send_frame(&w::accept_upload_req())?;
                        }
                        Some(rank) => {
                            out.queue_rank_sent = Some(rank);
                            link.send_frame(&w::queue_ranking(rank))?;
                        }
                    }
                }
                w::op::REQUESTPARTS => {
                    let (h, ranges) = w::parse_request_parts(&packet.payload)?;
                    let f = current_file.clone().ok_or("REQUESTPARTS before file selection")?;
                    if f.hash() != h {
                        return Err("REQUESTPARTS carried a different fileID".into());
                    }
                    request_seq += 1;
                    last_request_at = Instant::now();
                    last_ranges = ranges.clone();
                    for (start, end) in &ranges {
                        if *end <= *start || *end as usize > f.data.len() {
                            continue;
                        }
                        let send_now = match sent_ranges.get_mut(&(*start, *end)) {
                            None => true,
                            Some(s) => {
                                s.last_req = request_seq;
                                // 连续 3 轮仍在 pending 且距上次发送 ≥2s：
                                // 对端确实没收到，重传一次，避免永久饿死。
                                request_seq >= s.sent_req + 3
                                    && s.sent_at.elapsed() >= Duration::from_secs(2)
                            }
                        };
                        if !send_now {
                            continue;
                        }
                        sent_ranges.insert(
                            (*start, *end),
                            SentRange { sent_at: Instant::now(), sent_req: request_seq, last_req: request_seq },
                        );
                        if !queue_range(&mut pending_sends, *start, *end, request_seq) {
                            rewind_range(&mut pending_sends, *start, *end);
                        }
                    }
                    // 新请求是本回合唯一「对端仍在消费」的信号：按窗口预算放字节。
                    // 超出预算的部分留在 pending_sends 里，等下一次请求再送。
                    let budget = turn_budget(&config);
                    let (frames, used) = plan_sends(&mut pending_sends, budget, w::UPLOAD_CHUNK);
                    emit_planned(&mut link, &frames, &h, &f, &mut out)?;
                    out.max_in_flight_bytes = out.max_in_flight_bytes.max(used);
                }
                w::op::END_OF_DOWNLOAD | w::op::CANCELTRANSFER | w::op::OUTOFPARTREQS => return Ok(()),
                _ => {}
            }
        }
        Err("upload session deadline reached".into())
    })();

    if let Err(e) = run {
        out.error = Some(e);
    }
    if held_slot {
        if let Ok(mut q) = queue.lock() {
            q.release(peer_id);
        }
    }
    out.deferred_bytes = pending_sends.iter().map(|p| (p.end - p.next) as usize).sum();
    out.log = link.log;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_part_boundary_has_extra_part_hash() {
        // The L2 correction: a 1*PARTSIZE file has TWO part hashes, the second
        // being MD4 of empty input. Feeding the wrong hashset to an old client
        // makes it reject the whole file.
        let f = SharedFile::new("exact.bin", vec![0x5a; ED2K_PART_SIZE]);
        assert_eq!(f.part_hashes().len(), 2);
        assert_eq!(f.part_hashes()[1], crate::kad::md4(&[]));
        assert_eq!(f.hash(), ed2k_hash(&f.data));
        assert!(f.needs_hashset());
    }

    #[test]
    fn small_file_has_no_hashset() {
        let f = SharedFile::new("small.txt", b"hello".to_vec());
        assert!(f.part_hashes().is_empty());
        assert_eq!(f.hash(), crate::kad::md4(b"hello"));
        assert!(!f.needs_hashset());
        assert_eq!(f.blocks(), vec![(0, 5)]);
    }

    #[test]
    fn upload_queue_ranks_overflow_fifo() {
        let mut q = UploadQueue::new(1);
        let a = q.new_peer();
        let b = q.new_peer();
        let c = q.new_peer();
        assert_eq!(q.acquire(a), None); // slot granted
        assert_eq!(q.acquire(b), Some(1));
        assert_eq!(q.acquire(c), Some(2));
        assert_eq!(q.queued(), 2);
        q.release(a);
        assert_eq!(q.acquire(b), None); // FIFO: b, not c
        assert_eq!(q.acquire(c), Some(1));
    }

    #[test]
    fn upload_frames_follow_real_client_chunking() {
        // aMule 请求 184320 的一整块，自己上传时却按 10240 一帧发。
        let chunks = chunk_range(0, w::BLOCKSIZE, w::UPLOAD_CHUNK);
        assert_eq!(chunks.len(), 18);
        assert_eq!(chunks.first().unwrap(), &(0, 10_240));
        assert_eq!(chunks.last().unwrap(), &(174_080, 184_320));
        let mut at = 0;
        for (s, e) in &chunks {
            assert_eq!(*s, at);
            assert!(e > s);
            at = *e;
        }
        assert_eq!(chunk_range(5, 7, w::UPLOAD_CHUNK), vec![(5, 7)]);
    }

    #[test]
    fn blocks_cover_file_exactly() {
        let f = SharedFile::new("multi.bin", vec![7u8; w::BLOCKSIZE as usize * 2 + 17]);
        let blocks = f.blocks();
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].0, 0);
        assert_eq!(blocks.last().unwrap().1 as usize, f.data.len());
        let mut last_end = 0;
        for (s, e) in blocks {
            assert_eq!(s, last_end);
            assert!(e > s);
            last_end = e;
        }
    }

    #[test]
    fn window_plan_respects_budget_and_preserves_partial_progress() {
        let mut q = VecDeque::new();
        assert!(queue_range(&mut q, 0, 30_720, 1));
        // 预算小于一帧：一字节也不送，区间原样留在队首。
        let (frames, used) = plan_sends(&mut q, 10_000, w::UPLOAD_CHUNK);
        assert!(frames.is_empty());
        assert_eq!(used, 0);
        assert_eq!(q.front().unwrap().next, 0);

        // 一个整帧预算：只送 10_240，剩余留在队首，下次接着送。
        let (frames, used) = plan_sends(&mut q, 10_240, w::UPLOAD_CHUNK);
        assert_eq!(frames, vec![(0, 10_240)]);
        assert_eq!(used, 10_240);
        assert_eq!(q.front().unwrap().next, 10_240);

        let (f2, u2) = plan_sends(&mut q, 10_240, w::UPLOAD_CHUNK);
        assert_eq!(f2, vec![(10_240, 20_480)]);
        let (f3, u3) = plan_sends(&mut q, 20_480, w::UPLOAD_CHUNK);
        assert_eq!(f3, vec![(20_480, 30_720)]);
        assert_eq!(u2 + u3, 20_480);
        assert!(q.is_empty(), "窗口清空后不应留下幽灵区间");
    }

    #[test]
    fn window_plan_never_exceeds_budget_across_ranges() {
        let mut q = VecDeque::new();
        queue_range(&mut q, 0, 10_240, 1);
        queue_range(&mut q, 10_240, 20_480, 1);
        queue_range(&mut q, 20_480, 30_720, 1);
        let (frames, used) = plan_sends(&mut q, 15_000, w::UPLOAD_CHUNK);
        assert_eq!(frames, vec![(0, 10_240)]);
        assert_eq!(used, 10_240);
        assert_eq!(q.len(), 2, "第二个区间只被部分授权，不能整帧越界");
    }

    #[test]
    fn request_rewind_is_scoped_to_one_range() {
        let mut q = VecDeque::new();
        queue_range(&mut q, 0, 100, 1);
        queue_range(&mut q, 100, 200, 1);
        assert!(!queue_range(&mut q, 0, 100, 2)); // 重复请求只刷新 last_req
        // 测试用小 chunk 才能观察「区间内部的部分进度」；真实路径用 UPLOAD_CHUNK。
        let (frames, used) = plan_sends(&mut q, 50, 40);
        assert_eq!(frames, vec![(0, 40)]);
        assert_eq!(used, 40);
        assert_eq!(q[0].next, 40);
        rewind_range(&mut q, 0, 100);
        assert_eq!(q[0].next, 0);
        assert_eq!(q[1].next, 100);
    }

    #[test]
    fn read_timeout_classification_covers_windows() {
        assert!(is_read_timeout("read eD2k header: resource temporarily unavailable (os error 11)"));
        assert!(is_read_timeout("read eD2k header: A non-blocking socket operation could not be completed immediately (os error 10035)"));
        assert!(is_read_timeout("read eD2k body: connection timed out"));
        // A clean close must still be distinguishable from a poll timeout.
        assert!(!is_read_timeout("read eD2k header: connection closed"));
        assert!(!is_read_timeout("write eD2k frame (peer stopped reading?): broken pipe"));
    }

    #[test]
    fn serve_config_budget_defaults_and_opt_in() {
        let d = ServeConfig::default();
        // 默认仍是「经典请求最多 3 块」，不改变合规回环客户端的字节数。
        assert_eq!(turn_budget(&d), 3 * w::BLOCKSIZE as usize);
        assert!(!d.request_gated_pacing);
        let gated = ServeConfig { request_gated_pacing: true, ..d.clone() };
        assert_eq!(turn_budget(&gated), w::BLOCKSIZE as usize);
        let unlimited = ServeConfig { max_in_flight_bytes: None, ..d };
        assert_eq!(turn_budget(&unlimited), usize::MAX);
    }
}
