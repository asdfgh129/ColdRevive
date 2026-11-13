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

use std::collections::HashSet;
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
}

impl Link {
    pub fn new(stream: TcpStream) -> Self {
        Link { stream, log: Vec::new() }
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
        w::send_packet(&mut self.stream, protocol, opcode, payload)?;
        Ok(())
    }
    pub fn send_frame(&mut self, f: &[u8]) -> Result<(), String> {
        let p = w::parse_frame(f).map_err(|e| format!("internal frame: {e}"))?;
        self.send(p.protocol, p.opcode, &p.payload)
    }
    pub fn recv(&mut self) -> Result<w::Packet, String> {
        let p = w::read_packet(&mut self.stream)?;
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
    let mut handshake_done = false;
    let mut upload_accepted = false;

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
                    in_flight.retain(|r| *r != (start, end));
                    pending.retain(|r| *r != (start, end));
                    if pending.is_empty() {
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
    pub queue_granted: bool,
    pub queue_rank_sent: Option<u16>,
    pub log: Vec<LogEntry>,
    pub error: Option<String>,
}

/// Standard eD2k upload side. `files` is the whole (public, per ADR-011 D1)
/// shared directory; lookup is by real ed2k fileID. A slot is held for the
/// connection's lifetime and released on exit.
pub fn serve(stream: TcpStream, files: &[SharedFile], our_hello: &Hello, queue: &Arc<Mutex<UploadQueue>>, timeout: Duration) -> UploadResult {
    let mut out = UploadResult::default();
    let deadline = Instant::now() + timeout;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut link = Link::new(stream);
    let peer_id = queue.lock().map(|mut q| q.new_peer()).unwrap_or(0);
    let mut held_slot = false;
    let mut current_file: Option<SharedFile> = None;

    let run = (|| -> Result<(), String> {
        while Instant::now() < deadline {
            let packet = match link.recv() {
                Ok(p) => p,
                Err(e) if e.contains("read eD2k header") => return Ok(()), // peer closed
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
                    for (start, end) in ranges {
                        let s = start as usize;
                        let e = end as usize;
                        if e > f.data.len() || e <= s {
                            continue;
                        }
                        link.send_frame(&w::sending_part(&h, start, end, &f.data[s..e]))?;
                        out.blocks_sent += 1;
                        out.bytes_sent += e - s;
                    }
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
}
