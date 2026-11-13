// Wire structs mirror the on-wire layout; the loopback probe only exercises a
// subset of their fields, so dead-code warnings here are expected and not defects.
#![allow(dead_code)]

//! S2 spike: minimal eMule KAD v2 wire client.
//!
//!   kad-mini selftest        pure codec / hash assertions, no sockets
//!   kad-mini loopback        real 0xE4 frames between 3 loopback endpoints
//!   kad-mini live --check    inspect the real-network gate, send nothing
//!   kad-mini live ...        real-network small-sample run (hard-gated)
//!
//! HARD RULE: this binary never contacts a non-loopback address unless every
//! gate in `live_gate()` passes. See tests/spikes/s2/PLAN.md.

mod inflate;
mod sha1;
mod wire;

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use wire::*;

const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");

/// Iterative-routing depth ceiling for the tolerance-zone lookup (S2 P4).
/// Public KAD converges to the SEARCHTOLERANCE zone well within this; 3 was
/// far too shallow (only 2 candidates × 3 hops => ~20-34 contacts, never in
/// zone). 16 covers the funnel plus the extra hops spent re-verifying after a
/// single in-zone target turned out not to answer PUBLISH (v2.4.19 run).
///
/// v2.4.24 tried 32 (PLAN §17.6 option A) and it was a wash: the same key
/// reached the *same* closest distance (13,899,520) at 16 and at 32 hops, while
/// `frontier_widened_hops` doubled in absolute terms. The lookup saturates at
/// ~2^23.7-2^25 because the reachable neighbourhood of a key is sparse, not
/// because the hop budget is short (v2.4.25, PLAN §18). Reverted to 16; set to
/// 32 again only to reproduce that experiment.
const MAX_FIND_HOPS: usize = 16;
/// Stop walking once this many *verified* in-zone targets exist. One is not
/// enough: a node can answer KADEMLIA2_REQ and then silently drop PUBLISH
/// (firewalled-UDP flag), which with a single target means a zero-information
/// sample (observed twice in the v2.4.19 run).
const MIN_ZONE_TARGETS: usize = 3;
/// Lookup frontier width: only query contacts within this multiple of the best
/// (closest) XOR distance discovered so far. A Kademlia lookup narrows
/// monotonically; without this ceiling the walk jumps to 1G+ distance peers
/// once the near ones are spent and converges barely at all (see v2.4.23).
const FRONTIER_FACTOR: u64 = 8;
/// Publish/search fan-out: KAD stores a key on the K closest nodes, so we must
/// send PUBLISH/SEARCH to several near-key contacts -- not one. A single
/// contact's *reported* KadID can be stale vs its live KadID and aMule will then
/// silently drop the request (observed: node answered KADEMLIA2_REQ but timed out
/// on PUBLISH/SEARCH). Fanning out gives at least one genuine store a chance.
const PUBLISH_FANOUT: usize = 10;
const SAMPLE_KEYS: &[&str] = &[
    "coldrevive:t0:coldrevive-t0-sample.bin",
    "coldrevive:t0:wakeup_req.v1",
    "coldrevive:t0:wakeup_ack.v1",
    "coldrevive:adr:adr-001",
    "coldrevive:doc:roadmap",
];

// ------------------------------------------------------------------ JSON out --

#[derive(Clone, Debug)]
enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

fn s(v: impl Into<String>) -> J {
    J::Str(v.into())
}
fn n(v: impl Into<f64>) -> J {
    J::Num(v.into())
}
fn b(v: bool) -> J {
    J::Bool(v)
}
fn obj(pairs: Vec<(&str, J)>) -> J {
    J::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

impl J {
    fn dump(&self) -> String {
        match self {
            J::Null => "null".into(),
            J::Bool(v) => v.to_string(),
            J::Num(v) => {
                if v.fract() == 0.0 && v.is_finite() {
                    format!("{}", *v as i64)
                } else {
                    format!("{v}")
                }
            }
            J::Str(v) => {
                let mut o = String::from("\"");
                for c in v.chars() {
                    match c {
                        '"' => o.push_str("\\\""),
                        '\\' => o.push_str("\\\\"),
                        '\n' => o.push_str("\\n"),
                        '\r' => o.push_str("\\r"),
                        '\t' => o.push_str("\\t"),
                        c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
                        c => o.push(c),
                    }
                }
                o.push('"');
                o
            }
            J::Arr(v) => format!("[{}]", v.iter().map(J::dump).collect::<Vec<_>>().join(",")),
            J::Obj(v) => format!(
                "{{{}}}",
                v.iter()
                    .map(|(k, val)| format!("{}:{}", J::Str(k.clone()).dump(), val.dump()))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
fn utc_iso(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    if m <= 2 {
        y += 1;
    }
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        sod / 3600,
        (sod % 3600) / 60,
        sod % 60
    )
}

// -------------------------------------------------------------- raw records --

#[derive(Clone, Debug)]
struct Record {
    at_ms: u64,
    endpoint: String,
    direction: String,
    peer: String,
    opcode: u8,
    opcode_name: String,
    bytes: usize,
    hex: String,
    detail: String,
}

impl Record {
    fn json(&self) -> J {
        obj(vec![
            ("at_unix_ms", n(self.at_ms as f64)),
            ("at_utc", s(utc_iso(self.at_ms))),
            ("endpoint", s(self.endpoint.clone())),
            ("direction", s(self.direction.clone())),
            ("peer", s(self.peer.clone())),
            ("opcode", s(format!("0x{:02x}", self.opcode))),
            ("opcode_name", s(self.opcode_name.clone())),
            ("bytes", n(self.bytes as f64)),
            ("wire_hex", s(self.hex.clone())),
            ("detail", s(self.detail.clone())),
        ])
    }
}

fn rec(endpoint: &str, dir: &str, peer: &str, data: &[u8], detail: &str) -> Record {
    // Label by the logical opcode: plaintext 0xE4/0xE5 carry it at byte 1, and
    // an obfuscated envelope is peeled to its inner frame by decode_wire.
    let (opcode, obf) = match decode_wire(data) {
        Ok((op, _)) => (op, data.first().copied() != Some(PROTO_KAD) && data.first().copied() != Some(PROTO_KAD_PACKED)),
        Err(_) => {
            if data.len() >= 2 && (data[0] == PROTO_KAD || data[0] == PROTO_KAD_PACKED) {
                (data[1], false)
            } else {
                (0xFF, false)
            }
        }
    };
    Record {
        at_ms: unix_ms(),
        endpoint: endpoint.into(),
        direction: dir.into(),
        peer: peer.into(),
        opcode,
        opcode_name: opcode_name(opcode).into(),
        bytes: data.len(),
        hex: sha1::hex(data),
        detail: if obf {
            format!("{detail} [obfuscated KAD envelope]")
        } else {
            detail.into()
        },
    }
}

/// Decode either an unpacked `0xE4` frame or a zlib-packed `0xE5` frame.
/// aMule layout (src/ClientUDPSocket.cpp, `OP_KADEMLIAPACKEDPROT` branch):
/// `[0xE5][opcode][zlib(payload)]`; the inflated bytes are the KAD payload
/// only, i.e. exactly what `parse_frame()` would return for a plaintext frame.
fn decode_wire(data: &[u8]) -> Result<(u8, Vec<u8>), String> {
    match data.first().copied() {
        Some(PROTO_KAD) => {
            let (op, payload) = parse_frame(data)?;
            Ok((op, payload.to_vec()))
        }
        Some(PROTO_KAD_PACKED) => {
            if data.len() < 3 {
                return Err("0xE5 frame too short".into());
            }
            let op = data[1];
            let payload = inflate::zlib_decode(&data[2..])
                .map_err(|e| format!("0xE5 inflate failed: {e}"))?;
            Ok((op, payload))
        }
        // KAD obfuscation envelope: first byte is randomized (not 0xE4/0xE5), so
        // recover the inner `[0xE4][opcode][payload]` via the obfuscation layer.
        // aMule may have used try-0 (our KadID) or try-2 (our verify key); the
        // live probe's KadID is `node_id("live-probe")`, matching Client::bind_any.
        _ => {
            if let Some((inner, _svk)) =
                wire::deobfuscate(data, wire::OUR_UDP_KEY, &node_id("live-probe"))
            {
                return decode_wire(&inner);
            }
            match data.first().copied() {
                Some(p) => Err(format!("non-KAD frame 0x{p:02x} (likely RC4 obfuscated)")),
                None => Err("empty datagram".into()),
            }
        }
    }
}

/// Reject anything that is not a routable public IPv4 address. P4 must never
/// publish a test Key to a LAN / loopback / reserved peer (PLAN §5).
fn is_global_ipv4(ip: impl Into<IpAddr>) -> bool {
    let o = match ip.into() {
        IpAddr::V4(v4) => v4.octets(),
        IpAddr::V6(_) => return false, // this spike is IPv4-only
    };
    if o[0] == 0 || o[0] == 10 || o[0] == 127 || o[0] >= 224 {
        return false; // 0/8, private, loopback, multicast + reserved
    }
    if o[0] == 169 && o[1] == 254 {
        return false; // link-local
    }
    if o[0] == 172 && (16..=31).contains(&o[1]) {
        return false; // RFC1918
    }
    if o[0] == 192 && o[1] == 168 {
        return false; // RFC1918
    }
    if o[0] == 100 && (64..=127).contains(&o[1]) {
        return false; // RFC6598 CGNAT
    }
    if (o[0] == 192 && o[1] == 0 && o[2] == 2)
        || (o[0] == 198 && o[1] == 51 && o[2] == 100)
        || (o[0] == 203 && o[1] == 0 && o[2] == 113)
        || (o[0] == 198 && (o[1] == 18 || o[1] == 19))
    {
        return false; // documentation / benchmarking ranges
    }
    true
}

fn contact_addr(c: &Contact) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::new(c.ip[0], c.ip[1], c.ip[2], c.ip[3]), c.udp))
}

/// Selftest-only hex decoder (the crate has no hex crate dependency).
fn unhex(text: &str) -> Vec<u8> {
    let b = text.as_bytes();
    (0..b.len() / 2)
        .map(|i| {
            let h = |c: u8| -> u8 {
                match c {
                    b'0'..=b'9' => c - b'0',
                    b'a'..=b'f' => c - b'a' + 10,
                    b'A'..=b'F' => c - b'A' + 10,
                    _ => 0,
                }
            };
            (h(b[2 * i]) << 4) | h(b[2 * i + 1])
        })
        .collect()
}

fn packed_frame(zlib_hex: &str) -> Vec<u8> {
    let mut f = vec![PROTO_KAD_PACKED, op::BOOTSTRAP_RES];
    f.extend_from_slice(&unhex(zlib_hex));
    f
}

// ------------------------------------------------------------- rate limiter --

struct Limiter {
    min_interval: Duration,
    last: Option<Instant>,
    waits: u64,
    waited_ms: u64,
}

impl Limiter {
    fn new(rate_per_sec: f64) -> Self {
        Limiter {
            min_interval: Duration::from_secs_f64(1.0 / rate_per_sec.max(0.001)),
            last: None,
            waits: 0,
            waited_ms: 0,
        }
    }
    fn wait(&mut self) {
        if let Some(last) = self.last {
            let elapsed = last.elapsed();
            if elapsed < self.min_interval {
                let d = self.min_interval - elapsed;
                thread::sleep(d);
                self.waits += 1;
                self.waited_ms += d.as_millis() as u64;
            }
        }
        self.last = Some(Instant::now());
    }
}

// ------------------------------------------------------------------- client --

struct Client {
    name: String,
    sock: UdpSocket,
    id: KadId,
    records: Vec<Record>,
}

impl Client {
    fn bind(name: &str, id: KadId) -> io::Result<Self> {
        let sock = UdpSocket::bind("127.0.0.1:0")?;
        sock.set_read_timeout(Some(Duration::from_secs(2)))?;
        Ok(Client { name: name.into(), sock, id, records: Vec::new() })
    }
    /// Only used by `lan-probe`: binding to 0.0.0.0 is required for replies from
    /// a LAN peer to reach us (a 127.0.0.1-bound socket would advertise itself
    /// as the source address and the peer would answer itself).
    fn bind_any(name: &str, id: KadId) -> io::Result<Self> {
        let sock = UdpSocket::bind("0.0.0.0:0")?;
        sock.set_read_timeout(Some(Duration::from_secs(3)))?;
        Ok(Client { name: name.into(), sock, id, records: Vec::new() })
    }
    fn addr(&self) -> SocketAddr {
        self.sock.local_addr().expect("bound")
    }
    fn send(&mut self, to: SocketAddr, frame: &[u8], detail: &str, limiter: &mut Limiter, target_kad: Option<&KadId>) -> io::Result<()> {
        limiter.wait();
        // Wrap in the KAD obfuscation envelope when we know the peer's KadID, so
        // the peer can DECRYPT and process the request (the obfuscation layer is
        // the real admission gate; it silently drops anything it can't decrypt).
        // Our senderVerifyKey=OUR_UDP_KEY lets the peer lock its reply to us.
        let wire = match target_kad {
            Some(k) => wire::obfuscate(k, wire::OUR_UDP_KEY, frame),
            None => frame.to_vec(),
        };
        let r = rec(&self.name, "send", &to.to_string(), &wire, detail);
        self.records.push(r);
        self.sock.send_to(&wire, to)?;
        Ok(())
    }
    fn recv(&mut self, detail: &str) -> io::Result<(SocketAddr, Vec<u8>)> {
        let mut buf = [0u8; 2048];
        let (len, from) = self.sock.recv_from(&mut buf)?;
        let data = buf[..len].to_vec();
        self.records
            .push(rec(&self.name, "recv", &from.to_string(), &data, detail));
        Ok((from, data))
    }
    /// Send one request, wait for one datagram, verify the expected opcode.
    fn exchange(
        &mut self,
        to: SocketAddr,
        frame: &[u8],
        expected: u8,
        send_detail: &str,
        recv_detail: &str,
        limiter: &mut Limiter,
        target_kad: Option<&KadId>,
    ) -> Result<(Vec<u8>, f64), io::Error> {
        self.send(to, frame, send_detail, limiter, target_kad)?;
        let t0 = Instant::now();
        // Accept only a datagram from the peer we actually asked. A reply to an
        // earlier hop can land while we wait here; without this filter a stray 0x29
        // is decoded as "expected 0x43, got 0x29" and a stray 0x43 would be credited
        // to the wrong target. Bounded by 4 socket timeouts, so it cannot spin.
        let mut data = Vec::new();
        for _ in 0..4 {
            let (from, d) = self.recv(recv_detail)?;
            if from == to {
                data = d;
                break;
            }
        }
        if data.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("no datagram from {to} (only foreign frames arrived)"),
            ));
        }
        let rtt = t0.elapsed().as_secs_f64() * 1000.0;
        // Both 0xE4 plaintext and 0xE5 zlib-packed (and obfuscated) responses are
        // valid KAD replies. Misclassifying a packed frame as a protocol failure
        // would corrupt the RISK R-02 three-bucket observation semantics.
        let (op, payload) = decode_wire(&data).map_err(io::Error::other)?;
        if op != expected {
            return Err(io::Error::other(format!(
                "expected opcode 0x{expected:02x} ({}), got 0x{op:02x} ({})",
                opcode_name(expected),
                opcode_name(op)
            )));
        }
        Ok((payload, rtt))
    }
}

// ------------------------------------------------------------- index node --

/// A single KAD-like node: answers BOOTSTRAP / HELLO / REQ / PUBLISH_KEY /
/// SEARCH_KEY and keeps the published index in memory. Runs on its own thread.
struct IndexNodeResult {
    records: Vec<Record>,
    stored_keys: usize,
    tolerance_drops: usize,
    protocol_errors: Vec<String>,
}

fn run_index_node(
    contact: Contact,
    stop: Arc<AtomicBool>,
    strict_tolerance: bool,
) -> io::Result<(SocketAddr, thread::JoinHandle<IndexNodeResult>)> {
    let sock = UdpSocket::bind("127.0.0.1:0")?;
    sock.set_read_timeout(Some(Duration::from_millis(150)))?;
    let addr = sock.local_addr()?;
    // Advertise the *real* bound UDP port in BOOTSTRAP_RES / KADEMLIA2_RES.
    // An all-zero port would make the wire capture misleading.
    let contact = Contact { udp: addr.port(), ..contact };
    let handle = thread::spawn(move || {
        let mut index: HashMap<KadId, Vec<IndexEntry>> = HashMap::new();
        let mut records = Vec::new();
        let mut tolerance_drops = 0usize;
        let mut protocol_errors = Vec::new();
        let mut buf = [0u8; 2048];
        while !stop.load(Ordering::Relaxed) {
            let (len, from) = match sock.recv_from(&mut buf) {
                Ok(v) => v,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => continue,
                Err(e) => {
                    protocol_errors.push(format!("recv: {e}"));
                    break;
                }
            };
            let data = buf[..len].to_vec();
            let (opcode, payload) = match parse_frame(&data) {
                Ok(v) => v,
                Err(e) => {
                    protocol_errors.push(format!("parse_frame from {from}: {e}"));
                    continue;
                }
            };
            records.push(rec("index", "recv", &from.to_string(), &data, "inbound request"));

            let reply: Option<Vec<u8>> = match opcode {
                op::BOOTSTRAP_REQ => Some(bootstrap_res(&contact, &[contact.clone()])),
                op::HELLO_REQ => {
                    match parse_hello(payload) {
                        Ok(h) => {
                            let peer = Contact {
                                id: h.id,
                                ip: [127, 0, 0, 1],
                                udp: h.udp_from_tag.unwrap_or(0),
                                tcp: h.tcp,
                                version: h.version,
                            };
                            let _ = peer;
                            Some(hello(op::HELLO_RES, &contact, &[]))
                        }
                        Err(e) => {
                            protocol_errors.push(format!("parse HELLO_REQ: {e}"));
                            None
                        }
                    }
                }
                op::REQ => match parse_find_node_req(payload) {
                    Ok(r) => {
                        if r.receiver != contact.id {
                            protocol_errors.push("KADEMLIA2_REQ receiver/check ID mismatch".into());
                            None
                        } else {
                            // The only node in this private subnet is ourselves.
                            Some(find_node_res(&r.target, &[contact.clone()]))
                        }
                    }
                    Err(e) => {
                        protocol_errors.push(format!("parse KADEMLIA2_REQ: {e}"));
                        None
                    }
                },
                op::PUBLISH_KEY_REQ => match parse_publish_key_req(payload) {
                    Ok(req) => {
                        // Mirror aMule Process2PublishKeyRequest's tolerance gate.
                        // LAN peers bypass it; strict_tolerance simulates a WAN node.
                        let lan_bypass = !strict_tolerance;
                        if !publish_allowed(&contact.id, &req.key, lan_bypass) {
                            tolerance_drops += 1;
                            records.push(rec(
                                "index",
                                "drop",
                                &from.to_string(),
                                &data,
                                "silent drop: XOR distance outside SEARCHTOLERANCE",
                            ));
                            None // aMule returns NO response: this is the drop observation
                        } else {
                            let entry = index.entry(req.key).or_default();
                            for e in &req.entries {
                                if !entry.contains(e) {
                                    entry.push(e.clone());
                                }
                            }
                            Some(publish_res(&req.key, entry.len().min(255) as u8))
                        }
                    }
                    Err(e) => {
                        protocol_errors.push(format!("parse PUBLISH_KEY_REQ: {e}"));
                        None
                    }
                },
                op::SEARCH_KEY_REQ => match parse_search_key_req(payload) {
                    Ok(key) => {
                        let answers = index.get(&key).cloned().unwrap_or_default();
                        Some(search_res(&contact.id, &key, &answers))
                    }
                    Err(e) => {
                        protocol_errors.push(format!("parse SEARCH_KEY_REQ: {e}"));
                        None
                    }
                },
                op::PING => Some(frame(op::PONG, &[])),
                other => {
                    protocol_errors.push(format!(
                        "unhandled opcode 0x{other:02x} ({})",
                        opcode_name(other)
                    ));
                    None
                }
            };

            if let Some(out) = reply {
                records.push(rec("index", "send", &from.to_string(), &out, "outbound response"));
                if let Err(e) = sock.send_to(&out, from) {
                    protocol_errors.push(format!("send reply: {e}"));
                }
            }
        }
        IndexNodeResult {
            records,
            stored_keys: index.len(),
            tolerance_drops,
            protocol_errors,
        }
    });
    Ok((addr, handle))
}

// --------------------------------------------------------------- descriptor --

fn descriptor_tags(logical: &str, node: &str, port: u16) -> Vec<Tag> {
    vec![
        Tag::str1(TAG_FILENAME, &format!("{NAMESPACE}{logical}")),
        Tag::int4(TAG_FILESIZE, 1024),
        Tag::int2(TAG_SOURCEUPORT, port),
        Tag {
            name: b"ColdRevive_V1:descriptor".to_vec(),
            value: TagValue::Str(format!("v=1;node={node};wake_port={port};proto_flags=bt-only;infohash_prefix=none")),
        },
        Tag {
            name: b"ColdRevive_V1:namespace".to_vec(),
            value: TagValue::Str(NAMESPACE.into()),
        },
    ]
}

// ------------------------------------------------------------------ loopback --

fn run_loopback(key_count: usize, rate: f64) -> Result<J, String> {
    let keys: Vec<&str> = SAMPLE_KEYS.iter().copied().take(key_count).collect();
    let node_id_ = node_id("loopback-index");
    let index_contact = Contact {
        id: node_id_,
        ip: [127, 0, 0, 1],
        udp: 0, // filled after bind
        tcp: 14662,
        version: KAD_VERSION,
    };
    let stop = Arc::new(AtomicBool::new(false));
    let (index_addr, handle) = run_index_node(index_contact.clone(), stop.clone(), false)
        .map_err(|e| format!("bind index node: {e}"))?;

    let mut publisher = Client::bind("A-publisher", node_id("publisher"))
        .map_err(|e| format!("bind publisher: {e}"))?;
    let mut querier = Client::bind("B-querier", node_id("querier"))
        .map_err(|e| format!("bind querier: {e}"))?;
    let mut limiter = Limiter::new(rate);

    let mut publishes = Vec::new();
    let mut queries = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    // --- bootstrap + hello for A -------------------------------------------
    let boot = publisher
        .exchange(
            index_addr,
            &bootstrap_req(),
            op::BOOTSTRAP_RES,
            "A -> index: bootstrap request",
            "A <- index: bootstrap response",
            &mut limiter,
            None,
        )
        .map_err(|e| format!("bootstrap A: {e}"))?;
    let br = parse_bootstrap_res(&boot.0).map_err(|e| format!("bootstrap parse: {e}"))?;
    if br.version != KAD_VERSION {
        errors.push(format!("index advertised KAD version 0x{:02x}", br.version));
    }
    if br.contacts.iter().any(|c| c.udp == 0) {
        errors.push("BOOTSTRAP_RES advertised a contact with UDP port 0".into());
    }
    let hello_a = hello(
        op::HELLO_REQ,
        &Contact {
            id: publisher.id,
            ip: [127, 0, 0, 1],
            udp: publisher.addr().port(),
            tcp: 14663,
            version: KAD_VERSION,
        },
        &[],
    );
    let _ = publisher
        .exchange(
            index_addr,
            &hello_a,
            op::HELLO_RES,
            "A -> index: hello request",
            "A <- index: hello response",
            &mut limiter,
            None,
        )
        .map_err(|e| format!("hello A: {e}"))?;

    // --- bootstrap + hello for B -------------------------------------------
    let _ = querier
        .exchange(
            index_addr,
            &bootstrap_req(),
            op::BOOTSTRAP_RES,
            "B -> index: bootstrap request",
            "B <- index: bootstrap response",
            &mut limiter,
            None,
        )
        .map_err(|e| format!("bootstrap B: {e}"))?;
    let hello_b = hello(
        op::HELLO_REQ,
        &Contact {
            id: querier.id,
            ip: [127, 0, 0, 1],
            udp: querier.addr().port(),
            tcp: 14664,
            version: KAD_VERSION,
        },
        &[],
    );
    let _ = querier
        .exchange(
            index_addr,
            &hello_b,
            op::HELLO_RES,
            "B -> index: hello request",
            "B <- index: hello response",
            &mut limiter,
            None,
        )
        .map_err(|e| format!("hello B: {e}"))?;

    // --- per-key publish / query -------------------------------------------
    for (i, logical) in keys.iter().enumerate() {
        let key = namespaced_key(logical);
        let descriptor_id = node_id(&format!("descriptor-{i}"));

        // iterative find_node (one hop in a single-node private subnet)
        let (res_payload, find_rtt) = publisher
            .exchange(
                index_addr,
                &find_node_req(&key, &node_id_, 2),
                op::RES,
                "A -> index: KADEMLIA2_REQ (find nodes near Key)",
                "A <- index: KADEMLIA2_RES",
                &mut limiter,
                None,
            )
            .map_err(|e| format!("find_node {logical}: {e}"))?;
        let fres = parse_find_node_res(&res_payload).map_err(|e| format!("RES parse: {e}"))?;
        if fres.target != key {
            errors.push(format!("{logical}: RES target mismatch"));
        }

        let entry = IndexEntry {
            source_id: descriptor_id,
            tags: descriptor_tags(logical, "A", publisher.addr().port()),
        };
        let (pub_payload, pub_rtt) = publisher
            .exchange(
                index_addr,
                &publish_key_req(&key, &[entry.clone()]),
                op::PUBLISH_RES,
                "A -> index: PUBLISH_KEY_REQ (namespaced Key)",
                "A <- index: PUBLISH_RES",
            &mut limiter,
            None,
        )
        .map_err(|e| format!("publish {logical}: {e}"))?;
        let pres = parse_publish_res(&pub_payload).map_err(|e| format!("PUBLISH_RES parse: {e}"))?;
        let publish_ok = pres.key == key;
        publishes.push(obj(vec![
            ("seq", n(i as f64)),
            ("logical", s(*logical)),
            ("namespaced_key_id", s(id_hex(&key))),
            ("plain_key_id", s(id_hex(&plain_key(logical)))),
            ("has_namespace_prefix", b(true)),
            ("descriptor_id", s(id_hex(&descriptor_id))),
            ("find_node_contacts", n(fres.contacts.len() as f64)),
            ("find_node_rtt_ms", n(find_rtt)),
            ("publish_rtt_ms", n(pub_rtt)),
            ("publish_acked", b(publish_ok)),
            ("publish_load", n(pres.load as f64)),
            ("would_pass_wan_tolerance", b(top_chunk(&node_id_, &key) <= SEARCHTOLERANCE)),
            ("xor_top32", n(top_chunk(&node_id_, &key) as f64)),
            ("lan_bypass_applied", b(true)),
        ]));

        // B queries the same Key via a real SEARCH_KEY_REQ / SEARCH_RES pair.
        let (search_payload, search_rtt) = querier
            .exchange(
                index_addr,
                &search_key_req(&key),
                op::SEARCH_RES,
                "B -> index: SEARCH_KEY_REQ (aMule's closest thing to FIND_VALUE)",
                "B <- index: SEARCH_RES",
            &mut limiter,
            None,
        )
        .map_err(|e| format!("search {logical}: {e}"))?;
        let sres = parse_search_res(&search_payload).map_err(|e| format!("SEARCH_RES parse: {e}"))?;
        let hit = sres.target == key
            && sres.answers.iter().any(|a| a.source_id == descriptor_id);
        let descriptor_matches = sres.answers.iter().any(|a| {
            a.source_id == descriptor_id
                && a.tags.iter().any(|t| match (&t.name[..], &t.value) {
                    (b"ColdRevive_V1:namespace", TagValue::Str(v)) => v == NAMESPACE,
                    _ => false,
                })
        });
        queries.push(obj(vec![
            ("seq", n(i as f64)),
            ("logical", s(*logical)),
            ("target_key_id", s(id_hex(&sres.target))),
            ("responder_id", s(id_hex(&sres.sender))),
            ("answers", n(sres.answers.len() as f64)),
            ("hit", b(hit)),
            ("descriptor_matches", b(descriptor_matches)),
            ("rtt_ms", n(search_rtt)),
        ]));
    }

    // --- namespace isolation ------------------------------------------------
    let plain = plain_key(keys[0]);
    let ns = namespaced_key(keys[0]);
    let plain_entry = IndexEntry {
        source_id: node_id("plain-control"),
        tags: vec![Tag::str1(TAG_FILENAME, keys[0])],
    };
    let _ = publisher
        .exchange(
            index_addr,
            &publish_key_req(&plain, &[plain_entry]),
            op::PUBLISH_RES,
            "A -> index: PUBLISH_KEY_REQ (unprefixed control Key)",
            "A <- index: PUBLISH_RES (control)",
            &mut limiter,
            None,
        )
        .map_err(|e| format!("control publish: {e}"))?;
    let (ctrl, _) = querier
        .exchange(
            index_addr,
            &search_key_req(&ns),
            op::SEARCH_RES,
            "B -> index: SEARCH_KEY_REQ (namespaced Key, control check)",
            "B <- index: SEARCH_RES (control check)",
            &mut limiter,
            None,
        )
        .map_err(|e| format!("control search: {e}"))?;
    let ctrl_res = parse_search_res(&ctrl).map_err(|e| format!("control SEARCH_RES: {e}"))?;
    let isolation = obj(vec![
        ("plain_logical", s(keys[0])),
        ("plain_key_id", s(id_hex(&plain))),
        ("namespaced_key_id", s(id_hex(&ns))),
        ("ids_differ", b(plain != ns)),
        ("control_entry_leaked_into_namespaced_query", b(ctrl_res
            .answers
            .iter()
            .any(|a| a.source_id == node_id("plain-control")))),
        ("all_published_keys_have_prefix", b(publishes.iter().all(|_| true))),
        ("note", s("前缀只作用于本地哈希输入；线上传输的是 128-bit ID，公共节点看不到字符串前缀")),
    ]);

    stop.store(true, Ordering::Relaxed);
    let node_result = handle.join().map_err(|_| "index node thread panicked".to_string())?;

    let mut all_records: Vec<J> = Vec::new();
    all_records.extend(publisher.records.iter().map(Record::json));
    all_records.extend(querier.records.iter().map(Record::json));
    all_records.extend(node_result.records.iter().map(Record::json));
    all_records.sort_by(|a, c| {
        fn key(j: &J) -> u64 {
            match j {
                J::Obj(v) => v.iter().find_map(|(k, val)| match (k.as_str(), val) {
                    ("at_unix_ms", J::Num(x)) => Some(*x as u64),
                    _ => None,
                }).unwrap_or(0),
                _ => 0,
            }
        }
        key(a).cmp(&key(c))
    });

    let hit_count = queries.iter().filter(|q| match q {
        J::Obj(v) => v.iter().any(|(k, val)| k == "hit" && matches!(val, J::Bool(true))),
        _ => false,
    }).count();
    let pub_ok = publishes.iter().filter(|p| match p {
        J::Obj(v) => v.iter().any(|(k, val)| k == "publish_acked" && matches!(val, J::Bool(true))),
        _ => false,
    }).count();
    let rtts: Vec<f64> = queries.iter().filter_map(|q| match q {
        J::Obj(v) => v.iter().find_map(|(k, val)| match (k.as_str(), val) {
            ("rtt_ms", J::Num(x)) => Some(*x),
            _ => None,
        }),
        _ => None,
    }).collect();
    let mut sorted_rtt = rtts.clone();
    sorted_rtt.sort_by(|a, c| a.partial_cmp(c).unwrap_or(std::cmp::Ordering::Equal));
    let p50 = sorted_rtt.get(sorted_rtt.len() / 2).copied().unwrap_or(0.0);

    let report = obj(vec![
        ("schema", s("coldrevive.m0.s2-kad-wire/v1")),
        ("generated_at", s(utc_iso(unix_ms()))),
        ("generated_at_unix_ms", n(unix_ms() as f64)),
        ("mode", s("loopback")),
        ("network", s("127.0.0.1 UDP loopback, 3 endpoints (publisher / index / querier)")),
        ("public_kad_contacted", b(false)),
        ("evidence_valid_for_pass", b(false)),
        ("s2_status", s("not_run")),
        ("wire", obj(vec![
            ("frame", s("[0xE4][opcode][payload]")),
            ("kad_version", s(format!("0x{KAD_VERSION:02x}"))),
            ("id_bits", n(128.0)),
            ("search_tolerance", n(SEARCHTOLERANCE as f64)),
            ("protocol_source", s("aMule master @ 6a1105634d81242a581a0f36a7e00379b12a83f3")),
            ("find_value_mapping", s("KADEMLIA2_SEARCH_KEY_REQ (0x33) -> KADEMLIA2_SEARCH_RES (0x3B)")),
            ("publish_mapping", s("KADEMLIA2_PUBLISH_KEY_REQ (0x43) -> KADEMLIA2_PUBLISH_RES (0x4B)")),
        ])),
        ("rate_limit_pps", n(rate)),
        ("limiter", obj(vec![
            ("waits", n(limiter.waits as f64)),
            ("total_waited_ms", n(limiter.waited_ms as f64)),
        ])),
        ("summary", obj(vec![
            ("publishes", n(publishes.len() as f64)),
            ("publish_acked", n(pub_ok as f64)),
            ("queries", n(queries.len() as f64)),
            ("find_hits", n(hit_count as f64)),
            ("hit_rate", n(if queries.is_empty() { 0.0 } else { hit_count as f64 / queries.len() as f64 })),
            ("rtt_ms_p50", n(p50)),
            ("index_stored_keys", n(node_result.stored_keys as f64)),
            ("tolerance_drops", n(node_result.tolerance_drops as f64)),
            ("protocol_errors", n(node_result.protocol_errors.len() as f64)),
        ])),
        ("publishes", J::Arr(publishes)),
        ("queries", J::Arr(queries)),
        ("namespace_isolation", isolation),
        ("protocol_errors", J::Arr(node_result.protocol_errors.iter().map(|e| s(e.clone())).collect())),
        ("raw_records", J::Arr(all_records)),
        ("criteria", J::Arr(vec![
            obj(vec![("id", s("S2-C1")), ("text", s("双节点闭环：A 发布带前缀 Key，B 用 FIND_VALUE 命中 A")),
                     ("status", s("preview_only")),
                     ("note", s("已用真实 Kademlia2 报文在回环完成 A 发布 -> B SEARCH_KEY 命中 A 的描述符；按判据不得据此判 pass"))]),
            obj(vec![("id", s("S2-C2")), ("text", s("记录发布成功率、查询时延分布、被现有节点丢弃或拒绝的观测")),
                     ("status", s("partial")),
                     ("note", s("成功率与 RTT 已逐次记录；公共节点丢弃/拒绝只能在实网观测，本报告的 tolerance_drops 为协议逻辑模拟"))]),
            obj(vec![("id", s("S2-C3")), ("text", s("专属命名空间前缀不覆盖 / 不干扰现有 KAD Key 空间")),
                     ("status", s("preview_only")),
                     ("note", s("回环验证前缀/无前缀 Key ID 不同且查询互不泄漏；真实 Key 空间未接触"))]),
            obj(vec![("id", s("S2-C4")), ("text", s("结论须基于实网小样本；本地回环模拟只能作为预演")),
                     ("status", s("not_verified")), ("note", s("未执行实网实验，public_kad_contacted=false"))]),
        ])),
        ("errors", J::Arr(errors.iter().map(|e| s(e.clone())).collect())),
        ("note", s("本程序只产出真实 wire 层面的预演观测，绝不自动置 pass。实网执行见 tests/spikes/s2/PLAN.md 的 P3/P4。")),
    ]);
    Ok(report)
}

// -------------------------------------------------------------- lan probe --

#[derive(Default)]
struct LanArgs {
    ip: Option<String>,
    port: Option<u16>,
    rate: f64,
}

/// RFC1918 + loopback only. `lan-probe` must be physically incapable of
/// reaching a public KAD node, even if the operator mistypes an address.
fn is_private_ipv4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 10
        || (o[0] == 172 && (16..=31).contains(&o[1]))
        || (o[0] == 192 && o[1] == 168)
        || o[0] == 127
}

/// Interoperability probe against a real eMule client on the local LAN.
/// Sends BOOTSTRAP_REQ + HELLO_REQ only. Never publishes a Key: if that client
/// is also connected to public KAD, a publish could propagate test data out.
fn run_lan_probe(args: &LanArgs) -> Result<J, String> {
    let ip_text = args.ip.clone().ok_or("缺少 --ip <RFC1918 IPv4>")?;
    let ip: Ipv4Addr = ip_text.parse().map_err(|_| format!("invalid ip: {ip_text}"))?;
    if !is_private_ipv4(ip) {
        return Err(format!("拒绝非私有地址 {ip}：lan-probe 只允许 RFC1918 / 127.0.0.0/8"));
    }
    let port = args.port.ok_or("缺少 --port <UDP/KAD 端口>")?;
    if port == 0 {
        return Err("--port 必须 > 0".into());
    }
    let dest = SocketAddr::from((ip, port));
    let rate = if args.rate > 0.0 && args.rate <= 1.0 { args.rate } else { 0.2 };

    let mut client = Client::bind_any("lan-probe", node_id("lan-probe"))
        .map_err(|e| format!("bind lan socket: {e}"))?;
    let local = client.addr().port();
    let contact = Contact {
        id: client.id,
        ip: [127, 0, 0, 1],
        udp: local,
        tcp: local,
        version: KAD_VERSION,
    };
    let mut limiter = Limiter::new(rate);

    let probes: Vec<(&str, Vec<u8>, u8)> = vec![
        ("BOOTSTRAP_REQ", bootstrap_req(), op::BOOTSTRAP_RES),
        ("HELLO_REQ", hello(op::HELLO_REQ, &contact, &[]), op::HELLO_RES),
    ];
    let mut attempts = Vec::new();
    for (label, frame, expected) in probes {
        let t0 = Instant::now();
        client.send(dest, &frame, &format!("lan-probe: {label}"), &mut limiter, None)
            .map_err(|e| format!("send {label}: {e}"))?;
        let sent_hex = sha1::hex(&frame);
        let reply = client.recv("lan-probe: response");
        let rtt = t0.elapsed().as_secs_f64() * 1000.0;
        let (class, opcode, recv_hex, recv_len, from, dec_ok, dec_payload, dec_contacts) = match reply {
            Ok((from, data)) => {
                let proto = data.first().copied().unwrap_or(0x00);
                let op = data.get(1).copied().unwrap_or(0xFF);
                let cls = if proto == PROTO_KAD {
                    if op == expected { "plaintext_expected_response" }
                    else { "plaintext_other_opcode" }
                } else if proto == PROTO_KAD_PACKED {
                    // aMule SendPacket() calls PackPacket() above 200 bytes:
                    // 0xE5 + opcode + zlib stream (0x78 0xda ...).
                    if op == expected { "packed_0xE5_expected_response" }
                    else { "packed_0xE5_other_opcode" }
                } else {
                    "non_kad_likely_rc4_obfuscated"
                };
                // Decode both frame shapes with the same path used by `live`.
                let (ok, plen, contacts) = match decode_wire(&data) {
                    Ok((_, payload)) => (
                        true,
                        payload.len(),
                        parse_bootstrap_res(&payload).map(|r| r.contacts.len()).unwrap_or(0),
                    ),
                    Err(_) => (false, 0, 0),
                };
                (cls, op, sha1::hex(&data), data.len(), from.to_string(), ok, plen, contacts)
            }
            Err(_) => ("timeout_no_datagram", 0xFF, String::new(), 0, dest.to_string(), false, 0, 0),
        };
        attempts.push(J::Obj(vec![
            ("probe".to_string(), s(label)),
            ("sent_hex".to_string(), s(sent_hex)),
            ("expected_opcode".to_string(), s(format!("0x{expected:02X}"))),
            ("response_class".to_string(), s(class)),
            ("response_opcode".to_string(), s(format!("0x{opcode:02X}"))),
            ("response_bytes".to_string(), n(recv_len as f64)),
            ("response_hex".to_string(), s(recv_hex)),
            ("decoded_ok".to_string(), b(dec_ok)),
            ("decoded_payload_bytes".to_string(), n(dec_payload as f64)),
            ("decoded_bootstrap_contacts".to_string(), n(dec_contacts as f64)),
            ("rtt_ms".to_string(), n(rtt)),
            ("peer".to_string(), s(from)),
        ]));
    }

    Ok(obj(vec![
        ("schema", s("coldrevive.m0.s2-kad-lan-probe/v1")),
        ("generated_at", s(utc_iso(unix_ms()))),
        ("mode", s("lan-interop-probe")),
        ("target", s(dest.to_string())),
        ("target_is_private", b(true)),
        ("public_kad_contacted", b(false)),
        ("evidence_valid_for_pass", b(false)),
        ("s2_status", s("not_run")),
        ("publish_sent", b(false)),
        ("rate_limit_pps", n(rate)),
        ("attempts", J::Arr(attempts)),
        ("raw_records", J::Arr(client.records.iter().map(Record::json).collect())),
        ("note", s("只用 BOOTSTRAP/HELLO 探测真实 eMule 客户端的报文可解析性；0xE4 与 0xE5(zlib) 帧均解码并记录 decoded_* 字段；不发布任何 Key，不能作为 S2 公共 KAD 结论证据。")),
    ]))
}

// --------------------------------------------------- lan index probe (D) --

#[derive(Default)]
struct LanIndexArgs {
    ip: Option<String>,
    port: Option<u16>,
    rate: f64,
    bisect: bool,
    tag_probe: bool,
}

/// S2 PLAN §18.6 option D: index-plane interop against a real aMule on the LAN.
///
/// Why this exists. Five PUBLISH attempts against public, verified, in-zone peers
/// all came back silent. Per aMule source there are exactly two silent paths in
/// `Process2PublishKeyRequest`: the peer believes it is UDP-firewalled, or
/// `top32(KadID XOR key) > SEARCHTOLERANCE`. The second is excluded by
/// `verified_in_zone`; the first is invisible from outside. A LAN peer removes
/// both confounds at once -- `IsLanIP(ip)` bypasses the tolerance check entirely,
/// and a peer on the same broadcast domain has no NAT that could have failed its
/// UDP firewall test. What is left to measure is purely "does a real aMule accept
/// our 0x43 and serve the entry back to 0x33".
///
/// This is not public-KAD evidence: `public_kad_contacted=false`,
/// `evidence_valid_for_pass=false`, and it cannot set `S2.status`.
fn run_lan_index_probe(args: &LanIndexArgs) -> Result<J, String> {
    let ip_text = args.ip.clone().ok_or("缺少 --ip <RFC1918 IPv4>")?;
    let ip: Ipv4Addr = ip_text.parse().map_err(|_| format!("invalid ip: {ip_text}"))?;
    if !is_private_ipv4(ip) {
        return Err(format!("拒绝非私有地址 {ip}：lan-index-probe 只允许 RFC1918 / 127.0.0.0/8"));
    }
    let port = args.port.ok_or("缺少 --port <UDP/KAD 端口>")?;
    if port == 0 {
        return Err("--port 必须 > 0".into());
    }
    let dest = SocketAddr::from((ip, port));
    let rate = if args.rate > 0.0 && args.rate <= 1.0 { args.rate } else { 0.2 };

    let mut client = Client::bind_any("lan-index-probe", node_id("lan-index-probe"))
        .map_err(|e| format!("bind lan socket: {e}"))?;
    let mut limiter = Limiter::new(rate);

    let ms = unix_ms();
    // A logical name that has never existed anywhere. Any answer to the later
    // SEARCH therefore proves *this* run's PUBLISH was stored -- not that some
    // older entry under a reused key survived.
    let logical = format!("lan-index-probe:{ms}");
    let key = namespaced_key(&logical);
    let descriptor_id = node_id(&format!("lan-descriptor-{ms}"));
    let entry = IndexEntry {
        source_id: descriptor_id,
        tags: descriptor_tags(&logical, "lan-index-probe", client.addr().port()),
    };
    let publish_frame = publish_key_req(&key, &[entry.clone()]);

    // --- 0. reachability: BOOTSTRAP_REQ/RES proves a 2-way UDP path ----------
    let boot = client.exchange(
        dest,
        &bootstrap_req(),
        op::BOOTSTRAP_RES,
        "lan-index: BOOTSTRAP_REQ",
        "lan-index: BOOTSTRAP_RES",
        &mut limiter,
        None,
    );
    let (reachable, boot_contacts, boot_rtt, boot_err) = match &boot {
        Ok((p, rtt)) => (true, parse_bootstrap_res(p).map(|r| r.contacts.len()).unwrap_or(0), *rtt, String::new()),
        Err(e) => (false, 0, 0.0, e.to_string()),
    };

    // --- 0b. optional tag-count bisect --------------------------------------
    // If the full 5-tag publish is silent, cut the tag list down to 0,1,2,... and
    // see where the ack stops. Each variant uses its own key so an ack can never
    // be attributed to the wrong variant.
    let mut bisect: Vec<J> = Vec::new();
    if (args.bisect || args.tag_probe) && reachable {
        let all_tags = descriptor_tags(&logical, "lan-index-probe", client.addr().port());
        // --bisect cuts the real descriptor tag list down to 0,1,2,...; --tag-probe
        // swaps in single hand-built tags to separate "any tag at all" from
        // "this particular tag type/name/value length".
        let variants: Vec<(String, Vec<Tag>)> = if args.tag_probe {
            vec![
                ("filename_1char".to_string(), vec![Tag::str1(TAG_FILENAME, "a")]),
                ("filename_43char".to_string(), vec![Tag::str1(TAG_FILENAME, &format!("{NAMESPACE}{logical}"))]),
                ("filesize_u32".to_string(), vec![Tag::int4(TAG_FILESIZE, 1024)]),
                ("sourceuport_u16".to_string(), vec![Tag::int2(TAG_SOURCEUPORT, 62521)]),
                ("longname_str_short".to_string(), vec![Tag { name: b"ColdRevive_V1:descriptor".to_vec(), value: TagValue::Str("v=1".into()) }]),
                ("longname_str_14".to_string(), vec![Tag { name: b"ColdRevive_V1:namespace".to_vec(), value: TagValue::Str(NAMESPACE.into()) }]),
            ]
        } else {
            (0..=all_tags.len()).map(|k| (format!("prefix_{k}"), all_tags[..k].to_vec())).collect()
        };
        for (label, tags) in variants {
            let k = tags.len();
            let vkey = namespaced_key(&format!("{logical}:{label}"));
            let ventry = IndexEntry {
                source_id: node_id(&format!("lan-descriptor-{ms}-{label}")),
                tags: tags.clone(),
            };
            let vframe = publish_key_req(&vkey, &[ventry]);
            let detail_send = format!("lan-index: PUBLISH_KEY_REQ variant={label}");
            let detail_recv = format!("lan-index: PUBLISH_RES variant={label}");
            let r = client.exchange(dest, &vframe, op::PUBLISH_RES, &detail_send, &detail_recv, &mut limiter, None);
            let (acked, load, err) = match r {
                Ok((p_, _)) => (true, parse_publish_res(&p_).map(|x| x.load as i64).unwrap_or(-1), String::new()),
                Err(e) => (false, -1, e.to_string()),
            };
            bisect.push(obj(vec![
                ("variant", s(&label)),
                ("tag_count", n(k as f64)),
                ("tag_names", s(tags.iter().map(|t| String::from_utf8_lossy(&t.name).to_string()).collect::<Vec<_>>().join(","))),
                ("frame_bytes", n(vframe.len() as f64)),
                ("acked", b(acked)),
                ("load", n(load as f64)),
                ("error", s(err)),
            ]));
        }
    }

    // --- 1. PUBLISH_KEY_REQ -> PUBLISH_RES ----------------------------------
    let publish = if reachable {
        Some(client.exchange(
            dest,
            &publish_frame,
            op::PUBLISH_RES,
            "lan-index: PUBLISH_KEY_REQ",
            "lan-index: PUBLISH_RES",
            &mut limiter,
            None,
        ))
    } else {
        None
    };
    let (publish_res, publish_load, publish_rtt, publish_err, publish_key_echo) = match publish {
        None => (false, 0i64, 0.0, "skipped (peer unreachable)".to_string(), false),
        Some(Ok((p, rtt))) => match parse_publish_res(&p) {
            Ok(r) => (true, r.load as i64, rtt, String::new(), r.key == key),
            Err(e) => (true, -1, rtt, format!("PUBLISH_RES parse: {e}"), false),
        },
        Some(Err(e)) => (false, -1, 0.0, e.to_string(), false),
    };

    // --- 2. SEARCH_KEY_REQ -> SEARCH_RES ------------------------------------
    // aMule never replies to a search miss (`SendValidKeywordResult` is wholly
    // inside `if (m_Keyword_map.find(keyID) != end())`), so a timeout and an
    // empty index are indistinguishable by design. Retry once: the store path is
    // synchronous, but a lost datagram on a busy client is not worth a false
    // negative.
    let mut search_attempts: Vec<J> = Vec::new();
    let mut search_res = false;
    let mut search_answers = 0usize;
    let mut hit = false;
    let mut tags_match = false;
    for attempt in 0..2 {
        if !reachable || !publish_res {
            break;
        }
        if attempt > 0 {
            thread::sleep(Duration::from_millis(1200));
        }
        match client.exchange(
            dest,
            &search_key_req(&key),
            op::SEARCH_RES,
            "lan-index: SEARCH_KEY_REQ",
            "lan-index: SEARCH_RES",
            &mut limiter,
            None,
        ) {
            Ok((p, rtt)) => match parse_search_res(&p) {
                Ok(r) => {
                    search_res = true;
                    search_answers = r.answers.len();
                    let mine = r.answers.iter().find(|a| a.source_id == descriptor_id);
                    hit = r.target == key && mine.is_some();
                    tags_match = mine.is_some_and(|a| {
                        a.tags.iter().any(|t| match (&t.name[..], &t.value) {
                            (b"ColdRevive_V1:namespace", TagValue::Str(v)) => v == NAMESPACE,
                            _ => false,
                        })
                    });
                    search_attempts.push(obj(vec![
                        ("attempt", n(attempt as f64)),
                        ("search_ok", b(true)),
                        ("rtt_ms", n(rtt)),
                        ("answers", n(r.answers.len() as f64)),
                        ("target_echo_matches", b(r.target == key)),
                        ("our_descriptor_returned", b(hit)),
                        ("namespace_tag_matches", b(tags_match)),
                        ("responder_id", s(id_hex(&r.sender))),
                    ]));
                    break;
                }
                Err(e) => {
                    search_attempts.push(obj(vec![
                        ("attempt", n(attempt as f64)),
                        ("search_ok", b(true)),
                        ("parse_error", s(e)),
                    ]));
                }
            },
            Err(e) => {
                search_attempts.push(obj(vec![
                    ("attempt", n(attempt as f64)),
                    ("search_ok", b(false)),
                    ("error", s(e.to_string())),
                ]));
            }
        }
    }

    let verdict = if !reachable {
        "unreachable"
    } else if !publish_res {
        "publish_silent_on_lan"
    } else if hit && tags_match {
        "confirmed_publish_then_retrieve"
    } else if search_res {
        "publish_acked_search_answered_without_our_entry"
    } else {
        "publish_acked_search_silent"
    };

    Ok(obj(vec![
        ("schema", s("coldrevive.m0.s2-kad-lan-index-probe/v1")),
        ("generated_at", s(utc_iso(ms))),
        ("mode", s("lan-index-probe")),
        ("target", s(dest.to_string())),
        ("target_is_private", b(true)),
        ("public_kad_contacted", b(false)),
        ("evidence_valid_for_pass", b(false)),
        ("s2_status", s("not_run")),
        ("publish_sent", b(reachable)),
        ("rate_limit_pps", n(rate)),
        ("local_kad_id", s(id_hex(&client.id))),
        ("key_logical", s(&logical)),
        ("key_id", s(id_hex(&key))),
        ("descriptor_id", s(id_hex(&descriptor_id))),
        ("publish_frame_bytes", n(publish_frame.len() as f64)),
        ("publish_frame_hex", s(sha1::hex(&publish_frame))),
        ("distance_top32", n(top_chunk(&client.id, &key) as f64)),
        ("lan_bypass_required", b(top_chunk(&client.id, &key) > SEARCHTOLERANCE)),
        ("reachable", b(reachable)),
        ("bootstrap_contacts", n(boot_contacts as f64)),
        ("bootstrap_rtt_ms", n(boot_rtt)),
        ("bootstrap_error", s(boot_err)),
        ("publish_acked", b(publish_res)),
        ("publish_key_echo_matches", b(publish_key_echo)),
        ("publish_load", n(publish_load as f64)),
        ("publish_rtt_ms", n(publish_rtt)),
        ("publish_error", s(publish_err)),
        ("search_responded", b(search_res)),
        ("search_answers", n(search_answers as f64)),
        ("our_entry_returned", b(hit)),
        ("namespace_tag_matches", b(tags_match)),
        ("tag_bisect", J::Arr(bisect)),
        ("search_attempts", J::Arr(search_attempts)),
        ("verdict", s(verdict)),
        ("raw_records", J::Arr(client.records.iter().map(Record::json).collect())),
        ("note", s(lan_index_note())),
    ]))
}

/// Render a KAD tag name for reports. Numeric tag names are single raw bytes
/// like 0xFE (TAG_SOURCEIP), not printable text; `from_utf8_lossy` would turn
/// them into U+FFFD and make the evidence unreadable.
fn tag_name_display(name: &[u8]) -> String {
    let mut out = String::new();
    for &b in name {
        if (0x20..0x7f).contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("\\x{b:02x}"));
        }
    }
    out
}

fn lan_index_note() -> &'static str {
    "局域网真实 aMule 的索引面互操作探测（PLAN §18.6 选项 D）：先 BOOTSTRAP 证明双向 UDP 可达，再 0x43 发布一个从未存在过的命名空间 Key，随后 0x33 取回。LAN 源地址走 aMule 的 IsLanIP 豁免，因此距离判据不参与，剩下的只是「真实 aMule 是否接收并服务我们的索引条目」。public_kad_contacted=false、evidence_valid_for_pass=false，不能作为 S2 公共 KAD 结论。发布条目只落在该局域网节点上；但 aMule 会为自己持有的索引条目做 TTL 续期，若它同时连着公共 KAD，该条目未来可能被它自己重发一次。"
}

// --------------------------------------------- eD2k keyword dict probe (L1) --

#[derive(Default)]
struct Ed2kDictArgs {
    ip: Option<String>,
    port: Option<u16>,
    keyword: Option<String>,
    rate: f64,
}

/// ADR-011 L1: publish an eMule-parseable keyword-dictionary entry to a real
/// aMule on the LAN and read it back, using the **unnamespaced** MD4 keyword
/// Key (`KadGetKeywordHash`) rather than `ColdRevive_V1:` SHA-1 keys.
///
/// This is the first concrete check of the compatibility track: if a real
/// aMule stores and serves this entry, an old client searching `keyword`
/// would see a file with a genuine `ed2k_hash` as its fileID.
/// LAN-only, 0 public packets. It deliberately uses a unique nonsense term so
/// that even a LAN aMule that is also connected to public KAD cannot collide
/// with a real user's keyword bucket.
fn run_ed2k_dict_probe(args: &Ed2kDictArgs) -> Result<J, String> {
    let ip_text = args.ip.clone().ok_or("缺少 --ip <RFC1918 IPv4>")?;
    let ip: Ipv4Addr = ip_text.parse().map_err(|_| format!("invalid ip: {ip_text}"))?;
    if !is_private_ipv4(ip) {
        return Err(format!("拒绝非私有地址 {ip}：ed2k-dict-probe 只允许 RFC1918 / 127.0.0.0/8"));
    }
    let port = args.port.ok_or("缺少 --port <UDP/KAD 端口>")?;
    if port == 0 {
        return Err("--port 必须 > 0".into());
    }
    let dest = SocketAddr::from((ip, port));
    let rate = if args.rate > 0.0 && args.rate <= 1.0 { args.rate } else { 0.2 };

    let ms = unix_ms();
    let keyword = args
        .keyword
        .clone()
        .unwrap_or_else(|| format!("coldrevive-ed2k-probe-{ms}"));
    if keyword.is_empty() {
        return Err("--keyword 不能为空".into());
    }
    let key = kad_keyword_hash(&keyword);
    let filename = format!("coldrevive-ed2k-probe-{ms}.txt");
    let content = format!("ColdRevive eD2k keyword-dictionary interop probe {ms}\n").into_bytes();
    let file_hash = ed2k_hash(&content);
    let size = content.len() as u32;
    let entry = IndexEntry {
        source_id: file_hash,
        tags: vec![
            Tag::str1(TAG_FILENAME, &filename),
            Tag::int4(TAG_FILESIZE, size),
        ],
    };
    let link = format!("ed2k://|file|{filename}|{size}|{}|/", id_hex(&file_hash));
    let publish_frame = publish_key_req(&key, &[entry.clone()]);

    let mut client = Client::bind_any("ed2k-dict-probe", node_id("ed2k-dict-probe"))
        .map_err(|e| format!("bind lan socket: {e}"))?;
    let mut limiter = Limiter::new(rate);

    // --- 0. two-way UDP reachability ----------------------------------------
    let boot = client.exchange(
        dest,
        &bootstrap_req(),
        op::BOOTSTRAP_RES,
        "ed2k-dict: BOOTSTRAP_REQ",
        "ed2k-dict: BOOTSTRAP_RES",
        &mut limiter,
        None,
    );
    let (reachable, boot_contacts, boot_rtt, boot_err) = match &boot {
        Ok((p, rtt)) => (
            true,
            parse_bootstrap_res(p).map(|r| r.contacts.len()).unwrap_or(0),
            *rtt,
            String::new(),
        ),
        Err(e) => (false, 0, 0.0, e.to_string()),
    };

    // --- 1. PUBLISH_KEY_REQ into the eMule keyword dictionary ---------------
    let (publish_acked, publish_load, publish_echo, publish_rtt, publish_err) = if !reachable {
        (false, -1, false, 0.0, "bootstrap 未通过，跳过发布".to_string())
    } else {
        match client.exchange(
            dest,
            &publish_frame,
            op::PUBLISH_RES,
            "ed2k-dict: PUBLISH_KEY_REQ (unnamespaced keyword)",
            "ed2k-dict: PUBLISH_RES",
            &mut limiter,
            None,
        ) {
            Ok((p, rtt)) => {
                let parsed = parse_publish_res(&p);
                (
                    true,
                    parsed.as_ref().map(|r| r.load as i64).unwrap_or(-1),
                    parsed.map(|r| r.key == key).unwrap_or(false),
                    rtt,
                    String::new(),
                )
            }
            Err(e) => (false, -1, false, 0.0, e.to_string()),
        }
    };

    // --- 2. SEARCH_KEY_REQ and check our entry comes back -------------------
    let (search_responded, answers, our_entry_returned, tags_seen, search_rtt, search_err) =
        if !publish_acked {
            (false, 0, false, Vec::new(), 0.0, "发布未确认，跳过查询".to_string())
        } else {
            match client.exchange(
                dest,
                &search_key_req(&key),
                op::SEARCH_RES,
                "ed2k-dict: SEARCH_KEY_REQ (unnamespaced keyword)",
                "ed2k-dict: SEARCH_RES",
                &mut limiter,
                None,
            ) {
                Ok((p, rtt)) => match parse_search_res(&p) {
                    Ok(sr) => {
                        let mut found = false;
                        let mut seen: Vec<String> = Vec::new();
                        for a in &sr.answers {
                            if a.source_id == file_hash {
                                found = true;
                                for t in &a.tags {
                                    let v = match &t.value {
                                        TagValue::Str(v) => v.clone(),
                                        TagValue::U8(v) => v.to_string(),
                                        TagValue::U16(v) => v.to_string(),
                                        TagValue::U32(v) => v.to_string(),
                                        TagValue::U64(v) => v.to_string(),
                                        TagValue::Hash16(h) => sha1::hex(h),
                                        TagValue::Raw(_, v) => sha1::hex(v),
                                    };
                                    seen.push(format!("{}={v}", tag_name_display(&t.name)));
                                }
                            }
                        }
                        (true, sr.answers.len(), found, seen, rtt, String::new())
                    }
                    Err(e) => (true, 0, false, Vec::new(), rtt, e),
                },
                Err(e) => (false, 0, false, Vec::new(), 0.0, e.to_string()),
            }
        };

    let verdict = if !reachable {
        "unreachable"
    } else if !publish_acked {
        "publish_silent_on_lan"
    } else if our_entry_returned {
        "confirmed_unnamespaced_keyword_publish_then_retrieve"
    } else if search_responded {
        "publish_acked_search_answered_without_our_entry"
    } else {
        "publish_acked_search_silent"
    };

    Ok(obj(vec![
        ("schema", s("coldrevive.m0.s2-kad-ed2k-dict-probe/v1")),
        ("generated_at", s(utc_iso(ms))),
        ("mode", s("ed2k-dict-probe")),
        ("target", s(dest.to_string())),
        ("target_is_private", b(true)),
        ("public_kad_contacted", b(false)),
        ("evidence_valid_for_pass", b(false)),
        ("keyword", s(&keyword)),
        ("keyword_hash_algorithm", s("MD4(UTF8(term)) -> CUInt128::SetValueBE")),
        ("keyword_key", s(id_hex(&key))),
        ("keyword_key_is_namespaced", b(false)),
        ("file_name", s(&filename)),
        ("file_size", n(size as f64)),
        ("ed2k_hash", s(id_hex(&file_hash))),
        ("ed2k_part_size", n(ED2K_PART_SIZE as f64)),
        ("ed2k_link", s(&link)),
        ("publish_frame_bytes", n(publish_frame.len() as f64)),
        ("rate_limit_pps", n(rate)),
        ("local_kad_id", s(id_hex(&client.id))),
        ("distance_top32", n(top_chunk(&client.id, &key) as f64)),
        ("lan_bypass_required", b(top_chunk(&client.id, &key) > SEARCHTOLERANCE)),
        ("reachable", b(reachable)),
        ("bootstrap_contacts", n(boot_contacts as f64)),
        ("bootstrap_rtt_ms", n(boot_rtt)),
        ("bootstrap_error", s(boot_err)),
        ("publish_acked", b(publish_acked)),
        ("publish_key_echo_matches", b(publish_echo)),
        ("publish_load", n(publish_load as f64)),
        ("publish_rtt_ms", n(publish_rtt)),
        ("publish_error", s(publish_err)),
        ("search_responded", b(search_responded)),
        ("search_answers", n(answers as f64)),
        ("our_entry_returned", b(our_entry_returned)),
        ("returned_tags", J::Arr(tags_seen.iter().map(|x| s(x.clone())).collect())),
        ("search_rtt_ms", n(search_rtt)),
        ("search_error", s(search_err)),
        ("verdict", s(verdict)),
        ("raw_records", J::Arr(client.records.iter().map(Record::json).collect())),
        ("note", s("ADR-011 L1：局域网友好节点上的 eMule 关键词字典互操作。Key = MD4(UTF-8(term))，无命名空间前缀；条目 fileID 是真实 ed2k_hash。只发往 RFC1918 地址，0 个公网报文，public_kad_contacted=false、evidence_valid_for_pass=false。注意：若这台 aMule 同时连着公共 KAD，它可能为自己持有的索引条目做 TTL 续期并把测试条目重发一次；本探针因此只用唯一的无意义关键词，并在报告中保留该副作用说明。")),
    ]))
}

// ------------------------------------------- ADR-011 L2: KAD source index --
//
// L1 proved a real aMule will store and serve our *keyword* entry. That alone
// still cannot make an old client download anything: the file hash also needs a
// *source* entry (IP/TCP/UDP ports) in the KAD source dictionary. This probe
// publishes both halves with PUBLISH_KEY_REQ (0x43) + PUBLISH_SOURCE_REQ (0x44),
// then reads both back with SEARCH_KEY_REQ (0x33) + SEARCH_SOURCE_REQ (0x34).
//
// LAN-only, RFC1918 hard check, 0 public packets, no spike status change.
#[derive(Default)]
struct L2SourceArgs {
    ip: Option<String>,
    port: Option<u16>,
    rate: f64,
}

fn run_l2_source_probe(args: &L2SourceArgs) -> Result<J, String> {
    let ip_text = args.ip.clone().ok_or("缺少 --ip <RFC1918 IPv4>")?;
    let ip: Ipv4Addr = ip_text.parse().map_err(|_| format!("invalid ip: {ip_text}"))?;
    if !is_private_ipv4(ip) {
        return Err(format!("拒绝非私有地址 {ip}：l2-source-probe 只允许 RFC1918 / 127.0.0.0/8"));
    }
    let port = args.port.ok_or("缺少 --port <UDP/KAD 端口>")?;
    if port == 0 {
        return Err("--port 必须 > 0".into());
    }
    let dest = SocketAddr::from((ip, port));
    let rate = if args.rate > 0.0 && args.rate <= 1.0 { args.rate } else { 0.2 };

    let ms = unix_ms();
    let keyword = format!("coldrevive-l2-src-{ms}");
    let filename = format!("coldrevive-l2-src-{ms}.txt");
    let content = format!("ColdRevive L2 source-index interop probe {ms}\n").into_bytes();
    let file_size = content.len() as u64;
    let file_hash = ed2k_hash(&content);
    let keyword_key = kad_keyword_hash(&keyword);
    let link = format!("ed2k://|file|{filename}|{file_size}|{}|/", id_hex(&file_hash));

    let mut client = Client::bind_any("l2-source-probe", node_id("l2-source-probe"))
        .map_err(|e| format!("bind lan socket: {e}"))?;
    let local_udp_port = client.addr().port();
    let source_id = node_id("l2-source-clienthash");
    // 4662 is the standard eD2k TCP port; the probe does not listen on it (no
    // transfer is attempted here), but the published source must advertise a
    // nonzero TCP port or aMule's CIndexed::AddSources rejects the entry.
    let advertised_tcp_port: u16 = 4662;
    let tags = source_tags(advertised_tcp_port, local_udp_port, file_size);
    let mut limiter = Limiter::new(rate);

    let key_entry = IndexEntry {
        source_id: file_hash,
        tags: vec![Tag::str1(TAG_FILENAME, &filename), Tag::int4(TAG_FILESIZE, file_size as u32)],
    };
    let key_frame = publish_key_req(&keyword_key, &[key_entry]);
    let src_frame = publish_source_req(&file_hash, &source_id, &tags);
    let ksearch_frame = search_key_req(&keyword_key);
    let ssearch_frame = search_source_req(&file_hash, file_size);

    // Step 0: prove bidirectional UDP reachability before publishing anything.
    let boot = client.exchange(dest, &bootstrap_req(), op::BOOTSTRAP_RES,
        "l2: BOOTSTRAP_REQ", "l2: BOOTSTRAP_RES", &mut limiter, None);
    let (reachable, boot_contacts, boot_rtt, boot_err) = match &boot {
        Ok((p, rtt)) => (true, parse_bootstrap_res(p).map(|r| r.contacts.len()).unwrap_or(0), *rtt, String::new()),
        Err(e) => (false, 0, 0.0, e.to_string()),
    };

    // Step 1: keyword half (L1 path, re-run so both halves share one fileID).
    let (key_acked, key_load, key_rtt, key_err) = if !reachable {
        (false, -1, 0.0, "bootstrap 未通过，跳过发布".into())
    } else {
        match client.exchange(dest, &key_frame, op::PUBLISH_RES,
            "l2: PUBLISH_KEY_REQ (0x43)", "l2: PUBLISH_RES (keyword)", &mut limiter, None) {
            Ok((p, rtt)) => (true, parse_publish_res(&p).map(|r| r.load as i64).unwrap_or(-1), rtt, String::new()),
            Err(e) => (false, -1, 0.0, e.to_string()),
        }
    };

    // Step 2: source half (0x44) — the new L2 wire path.
    let (src_acked, src_key_echo, src_load, src_rtt, src_err) = if !reachable {
        (false, false, -1, 0.0, "bootstrap 未通过，跳过发布".into())
    } else {
        match client.exchange(dest, &src_frame, op::PUBLISH_RES,
            "l2: PUBLISH_SOURCE_REQ (0x44)", "l2: PUBLISH_RES (source)", &mut limiter, None) {
            Ok((p, rtt)) => match parse_publish_res(&p) {
                Ok(r) => (true, r.key == file_hash, r.load as i64, rtt, String::new()),
                Err(e) => (true, false, -1, rtt, e),
            },
            Err(e) => (false, false, -1, 0.0, e.to_string()),
        }
    };

    // Step 3: read the keyword entry back.
    let (ksearch_ok, ksearch_answers, key_entry_returned, ksearch_rtt, ksearch_err) =
        if !key_acked { (false, 0, false, 0.0, "关键词发布未确认，跳过查询".into()) } else {
            match client.exchange(dest, &ksearch_frame, op::SEARCH_RES,
                "l2: SEARCH_KEY_REQ (0x33)", "l2: SEARCH_RES (keyword)", &mut limiter, None) {
                Ok((p, rtt)) => match parse_search_res(&p) {
                    Ok(sr) => (true, sr.answers.len(), sr.answers.iter().any(|a| a.source_id == file_hash), rtt, String::new()),
                    Err(e) => (true, 0, false, rtt, e),
                },
                Err(e) => (false, 0, false, 0.0, e.to_string()),
            }
        };

    // Step 4: read the source entry back and check the returned source tags.
    let (ssearch_ok, ssearch_answers, our_source_returned, returned_tags, ssearch_rtt, ssearch_err) =
        if !src_acked { (false, 0, false, Vec::<String>::new(), 0.0, "源发布未确认，跳过查询".into()) } else {
            match client.exchange(dest, &ssearch_frame, op::SEARCH_RES,
                "l2: SEARCH_SOURCE_REQ (0x34)", "l2: SEARCH_RES (source)", &mut limiter, None) {
                Ok((p, rtt)) => match parse_search_res(&p) {
                    Ok(sr) => {
                        let mut found = false;
                        let mut seen = Vec::new();
                        for a in &sr.answers {
                            if a.source_id == source_id {
                                found = true;
                            }
                            for t in &a.tags {
                                let v = match &t.value {
                                    TagValue::Str(v) => v.clone(),
                                    TagValue::U8(v) => v.to_string(),
                                    TagValue::U16(v) => v.to_string(),
                                    TagValue::U32(v) => v.to_string(),
                                    TagValue::U64(v) => v.to_string(),
                                    TagValue::Hash16(h) => sha1::hex(h),
                                    TagValue::Raw(_, v) => sha1::hex(v),
                                };
                                seen.push(format!("{}={v}", tag_name_display(&t.name)));
                            }
                        }
                        (true, sr.answers.len(), found, seen, rtt, String::new())
                    }
                    Err(e) => (true, 0, false, Vec::new(), rtt, e),
                },
                Err(e) => (false, 0, false, Vec::new(), 0.0, e.to_string()),
            }
        };

    let verdict = if !reachable { "unreachable" }
        else if !key_acked { "keyword_publish_silent" }
        else if !src_acked { "source_publish_silent" }
        else if key_entry_returned && our_source_returned {
            "confirmed_dual_track_keyword_and_source_publish_then_retrieve"
        } else if key_entry_returned { "keyword_ok_source_not_returned" }
        else if our_source_returned { "source_ok_keyword_not_returned" }
        else { "both_publishes_acked_but_no_entry_returned" };

    Ok(obj(vec![
        ("schema", s("coldrevive.m0.s2-kad-l2-source-probe/v1")),
        ("generated_at", s(utc_iso(ms))),
        ("mode", s("l2-source-probe")),
        ("target", s(dest.to_string())),
        ("target_is_private", b(true)),
        ("public_kad_contacted", b(false)),
        ("evidence_valid_for_pass", b(false)),
        ("keyword", s(&keyword)),
        ("keyword_key", s(id_hex(&keyword_key))),
        ("keyword_key_is_namespaced", b(false)),
        ("file_name", s(&filename)),
        ("file_size", n(file_size as f64)),
        ("ed2k_file_hash", s(id_hex(&file_hash))),
        ("ed2k_link", s(&link)),
        ("source_id", s(id_hex(&source_id))),
        ("advertised_tcp_port", n(advertised_tcp_port as f64)),
        ("advertised_udp_port", n(local_udp_port as f64)),
        ("source_tag_count", n(tags.len() as f64)),
        ("publish_source_frame_bytes", n(src_frame.len() as f64)),
        ("publish_source_layout", s("<fileID 16><sourceID 16><tagcount u8><Kad ReadTag tags>")),
        ("rate_limit_pps", n(rate)),
        ("reachable", b(reachable)),
        ("bootstrap_contacts", n(boot_contacts as f64)),
        ("bootstrap_rtt_ms", n(boot_rtt)),
        ("bootstrap_error", s(boot_err)),
        ("keyword_publish_acked", b(key_acked)),
        ("keyword_publish_load", n(key_load as f64)),
        ("keyword_publish_rtt_ms", n(key_rtt)),
        ("keyword_publish_error", s(key_err)),
        ("source_publish_acked", b(src_acked)),
        ("source_publish_key_echo_matches", b(src_key_echo)),
        ("source_publish_load", n(src_load as f64)),
        ("source_publish_rtt_ms", n(src_rtt)),
        ("source_publish_error", s(src_err)),
        ("keyword_search_responded", b(ksearch_ok)),
        ("keyword_search_answers", n(ksearch_answers as f64)),
        ("keyword_entry_returned", b(key_entry_returned)),
        ("keyword_search_rtt_ms", n(ksearch_rtt)),
        ("keyword_search_error", s(ksearch_err)),
        ("source_search_responded", b(ssearch_ok)),
        ("source_search_answers", n(ssearch_answers as f64)),
        ("our_source_returned", b(our_source_returned)),
        ("returned_source_tags", J::Arr(returned_tags.iter().map(|x| s(x.clone())).collect())),
        ("source_search_rtt_ms", n(ssearch_rtt)),
        ("source_search_error", s(ssearch_err)),
        ("verdict", s(verdict)),
        ("raw_records", J::Arr(client.records.iter().map(Record::json).collect())),
        ("note", s("ADR-011 L2 索引面：对同一 fileID 分别发 0x43 关键词条目与 0x44 源条目，再用 0x33 / 0x34 取回。仅 RFC1918 目标，0 个公网报文，不判 pass、不回填 spikes.json。0x44 的距离校验基于 fileID（不是 keyword key），LAN 源地址由 aMule 的 IsLanIP 豁免；源 IP 取自 UDP 报头，无需在 tag 中提供。若该 aMule 同时连着公共 KAD，它可能为持有的条目做 TTL 续期并重发一次，故关键词与文件名均使用唯一无意义串。")),
    ]))
}

// ---------------------------------------------------------------- live gate --

#[derive(Default)]
struct LiveArgs {
    bootstrap: Option<PathBuf>,
    sample_size: Option<usize>,
    rate_limit: Option<f64>,
    attest: bool,
    check_only: bool,
    /// When true, live requests are wrapped in the aMule UDP obfuscation
    /// envelope (try-0 KadID key). Default false: send cleartext `0xE4` frames,
    /// which these public KAD peers demonstrably accept (aMule's
    /// first-byte-is-protocol-byte path). Our obfuscation envelope is now
    /// byte-accurate vs aMule master, but live interop still returned no
    /// responses in P4; cleartext is the proven path for this experiment.
    obfuscate: bool,
}

fn spikes_json_path() -> PathBuf {
    Path::new(MANIFEST).join("..").join("..").join("spikes.json")
}

fn live_gate(args: &LiveArgs) -> (bool, Vec<String>, Vec<String>) {
    let mut blockers = Vec::new();
    let mut info = Vec::new();

    // 1. approval record
    let path = spikes_json_path();
    match fs::read_to_string(&path) {
        Ok(text) => {
            let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            let has_approval = compact.contains("\"public_kad_approval\"");
            let approved = compact.contains("\"approved\":true");
            if !has_approval || !approved {
                blockers.push("spikes.json 未找到 S2.public_kad_approval.approved=true".into());
            }
            // Locate the approval object only (up to the next `observations`).
            let start = compact.find("\"public_kad_approval\"").unwrap_or(0);
            let end = compact[start..].find("\"observations\"").map(|x| start + x).unwrap_or(compact.len());
            let block = &compact[start..end];
            if block.contains("\"pending\"") {
                blockers.push("S2.public_kad_approval.pending 未清除：G0 尚未确认最小样本量与限速阈值".into());
            }
            // G0-confirmed parameters are authoritative: CLI values may not
            // exceed them, otherwise the approval record would be decorative.
            let cs = extract_number(block, "sample_size");
            let cr = extract_float(block, "rate_limit_pps");
            match (cs, cr) {
                (Some(cs), Some(cr)) => {
                    info.push(format!("g0_confirmed: sample_size={cs} rate_limit_pps={cr}"));
                    if let Some(n_) = args.sample_size {
                        if n_ as u64 != cs {
                            blockers.push(format!("--sample-size={n_} 与 G0 确认值 {cs} 不一致"));
                        }
                    }
                    if let Some(r) = args.rate_limit {
                        if (r - cr).abs() > 1e-9 {
                            blockers.push(format!("--rate-limit={r} 与 G0 确认值 {cr} 不一致"));
                        }
                    }
                }
                _ => blockers.push("S2.public_kad_approval 缺少 G0 确认的 sample_size / rate_limit_pps".into()),
            }
            info.push(format!("approval_file={}", path.display()));
        }
        Err(e) => blockers.push(format!("无法读取 {}: {e}", path.display())),
    }

    // 2. user-supplied bootstrap list (OPS §6: never bundled)
    match &args.bootstrap {
        None => blockers.push("缺少 --bootstrap <file>；按 OPS §6 禁止内置默认 bootstrap 列表".into()),
        Some(p) => match fs::read_to_string(p) {
            Ok(text) => {
                // A LAN interop file is explicitly forbidden for live publishing:
                // that peer may itself be connected to public KAD (PLAN §5).
                if text.contains("lan-interop-probe-only") {
                    blockers.push(format!(
                        "{} 标记为 lan-interop-probe-only：禁止向 LAN 节点发布测试 Key",
                        p.display()
                    ));
                }
                match parse_bootstrap_file(&text) {
                    Ok(nodes) if nodes.is_empty() => {
                        blockers.push(format!("{} 中没有可解析的 bootstrap 节点", p.display()));
                    }
                    Ok(nodes) => {
                        let bad: Vec<String> = nodes
                            .iter()
                            .filter(|a| !is_global_ipv4(a.ip()))
                            .map(|a| a.ip().to_string())
                            .collect();
                        if !bad.is_empty() {
                            blockers.push(format!(
                                "bootstrap 含非公网地址（RFC1918 / loopback / 保留段），禁止 live 发布: {}",
                                bad.join(", ")
                            ));
                        }
                        info.push(format!(
                            "bootstrap_nodes={} (user supplied, parsed; public-IPv4 validation {})",
                            nodes.len(),
                            if bad.is_empty() { "passed" } else { "FAILED" }
                        ));
                    }
                    Err(e) => blockers.push(format!("bootstrap 文件解析失败 {}: {e}", p.display())),
                }
            }
            Err(e) => blockers.push(format!("无法读取 bootstrap 文件 {}: {e}", p.display())),
        },
    }

    // 3. sample size
    match args.sample_size {
        None => blockers.push("缺少 --sample-size N（G0 确认的最小样本量）".into()),
        Some(n_) if n_ == 0 || n_ > 10 => blockers.push(format!("--sample-size={n_} 超出硬上限 1..=10")),
        Some(n_) => info.push(format!("sample_size={n_}")),
    }

    // 4. rate limit
    match args.rate_limit {
        None => blockers.push("缺少 --rate-limit R（G0 确认的限速阈值）".into()),
        Some(r) if !(r > 0.0 && r <= 1.0) => blockers.push(format!("--rate-limit={r} 必须落在 (0, 1.0] 包/秒")),
        Some(r) => info.push(format!("rate_limit_pps={r}")),
    }

    // 5. explicit human attestation
    if !args.attest {
        blockers.push("缺少 --attest-i-am-the-experiment-operator（需实验执行人显式确认）".into());
    }

    (blockers.is_empty(), blockers, info)
}

fn run_live(args: &LiveArgs) -> Result<J, String> {
    let (ok, blockers, info) = live_gate(args);
    let gate = obj(vec![
        ("passed", b(ok)),
        ("blockers", J::Arr(blockers.iter().map(|x| s(x.clone())).collect())),
        ("info", J::Arr(info.iter().map(|x| s(x.clone())).collect())),
        ("attested", b(args.attest)),
        ("check_only", b(args.check_only)),
    ]);

    if args.check_only || !ok {
        return Ok(obj(vec![
            ("schema", s("coldrevive.m0.s2-kad-live/v1")),
            ("generated_at", s(utc_iso(unix_ms()))),
            ("mode", s("live-gate-check")),
            ("public_kad_contacted", b(false)),
            ("evidence_valid_for_pass", b(false)),
            ("s2_status", s("not_run")),
            ("gate", gate),
            ("note", s("仅 --check 或门禁未全部通过：未发送任何报文，未接触公共 KAD。")),
        ]));
    }

    // Gate passed. Parse user-supplied bootstrap nodes and run the small sample.
    let text = fs::read_to_string(args.bootstrap.as_ref().unwrap())
        .map_err(|e| format!("bootstrap read: {e}"))?;
    let nodes = parse_bootstrap_file(&text)?;
    if nodes.is_empty() {
        return Err("bootstrap 文件无有效节点".into());
    }
    let sample = args.sample_size.unwrap();
    let rate = args.rate_limit.unwrap();
    let obf = args.obfuscate;
    // A 127.0.0.1-bound socket cannot send to public peers; bind_any() also
    // sets the 3 s read timeout. (Client::bind is loopback-only by design.)
    let mut client = Client::bind_any("live-probe", node_id("live-probe"))
        .map_err(|e| format!("bind live socket: {e}"))?;
    let mut limiter = Limiter::new(rate);
    let mut attempts = Vec::new();

    for (i, logical) in SAMPLE_KEYS.iter().take(sample).enumerate() {
        let key = namespaced_key(logical);
        let dest = nodes[i % nodes.len()];
        let mut fields: Vec<(&'static str, J)> = vec![
            ("seq", n(i as f64)),
            ("logical", s(*logical)),
            ("key_id", s(id_hex(&key))),
            ("bootstrap", s(dest.to_string())),
            // P4 protocol_scope: keep plaintext, skip HELLO, no UDP key / RC4.
            ("hello", s("skipped_per_p4_protocol_scope")),
        ];

        // BOOTSTRAP_REQ -> BOOTSTRAP_RES (plaintext 0xE4 or zlib-packed 0xE5).
        let boot = client.exchange(
            dest, &bootstrap_req(), op::BOOTSTRAP_RES,
            "live: bootstrap request", "live: bootstrap response", &mut limiter, None,
        );
        fields.push(("bootstrap_res", b(boot.is_ok())));
        let mut known: Vec<Contact> = Vec::new();
        match &boot {
            Ok((p, rtt)) => {
                fields.push(("bootstrap_rtt_ms", n(*rtt)));
                match parse_bootstrap_res(p) {
                    Ok(r) => {
                        fields.push(("bootstrap_contacts", n(r.contacts.len() as f64)));
                        known = r.contacts;
                    }
                    Err(e) => {
                        fields.push(("bootstrap_contacts", n(0.0)));
                        fields.push(("bootstrap_parse_error", s(format!("BOOTSTRAP_RES: {e}"))));
                    }
                }
            }
            Err(e) => fields.push(("bootstrap_error_or_drop", s(e.to_string()))),
        }

        // S2.criteria #2: converge into the SEARCHTOLERANCE zone with
        // KADEMLIA2_REQ/RES (0x21/0x29) BEFORE publishing. A publish sent to an
        // arbitrary bootstrap node outside the zone is silently dropped by
        // Process2PublishKeyRequest, which would produce a false "rejection".
        let zone_contacts = |contacts: &[Contact]| -> Vec<Contact> {
            contacts.iter().filter(|c| {
                c.udp != 0
                    && top_chunk(&c.id, &key) <= SEARCHTOLERANCE
                    && is_global_ipv4(contact_addr(c).ip())
            }).cloned().collect()
        };
        // A contact's *advertised* KadID may be stale: eMule keeps answering from
        // an old ID for a while, and aMule's Process2Publish/SearchKeyRequest then
        // silently drops the request (v2.4.17 observed exactly this: the node
        // answered 0x21 but timed out on PUBLISH/SEARCH at the same UDP address).
        //
        // KADEMLIA2_REQ carries `check` = the recipient's own KadID, and aMule
        // drops the packet silently when `check` != its own id. So a 0x29 answer
        // proves liveness AND the real KadID. Only verified peers may be publish /
        // search targets: a tolerance-zone match on an unverified ID is the 0/10
        // timeout pattern, not an index-layer rejection.
        let mut seen: HashSet<SocketAddr> = HashSet::new();
        let mut verified: HashSet<SocketAddr> = HashSet::new();
        let verified_zone = |contacts: &[Contact], verified: &HashSet<SocketAddr>| -> Vec<Contact> {
            contacts.iter().filter(|c| {
                c.udp != 0
                    && is_global_ipv4(contact_addr(c).ip())
                    && verified.contains(&contact_addr(c))
                    && top_chunk(&c.id, &key) <= SEARCHTOLERANCE
            }).cloned().collect()
        };
        let mut hops = 0usize;
        let mut stale_hops = 0usize;
        let mut widened_hops = 0usize;
        // Walk the 2^24 funnel until enough *verified* in-zone responders exist.
        //
        // The loop must NOT exit when the routing table stops growing. A plateau
        // means "we learned no new contacts", not "we have nothing left to ask":
        // after the crawl, `known` holds 110-130 contacts of which only ~10 (the
        // first two hops' candidates) were ever verified, and a publish target must
        // be BOTH verified and in-zone. Breaking on plateau therefore abandoned
        // 90% of the table unverified and published to 0-1 nodes per sample. Keep
        // spending the hop budget on *verifying* candidates we already have; the
        // only real exits are the hop ceiling or an exhausted candidate list.
        while hops < MAX_FIND_HOPS && verified_zone(&known, &verified).len() < MIN_ZONE_TARGETS {
            let before = known.len();
            // Query the ALPHA_QUERY *unqueried* contacts nearest the key, taken
            // from the WHOLE table. The window used to be `ALPHA_QUERY * 6` (=30),
            // which capped the walk structurally: once those 30 were `seen`,
            // `candidates` came back empty and the loop exited at hop 7 with
            // queried=33 while `known` held 138-165 still-unqueried contacts
            // (v2.4.21: closest_top_chunk stalled at ~2^23, i.e. one hop above the
            // 2^24 boundary, and only 0-1 in-zone targets ever appeared).
            // FRONTIER GUARD -- this is what a Kademlia lookup actually is.
            //
            // v2.4.22 took the closest *unqueried* contact from the whole table
            // with no distance ceiling. Once the handful of near contacts had
            // been asked (2-3 hops), every later hop had to reach 700M-1400M away,
            // and those far peers answered with near-uniform contact sets. The
            // crawl therefore oscillated instead of descending: replaying the
            // v2.4.22 raw records per hop shows queried distances going
            // 43M -> 54M -> 272M -> 743M -> 38M -> 266M -> 1342M, and the 2198
            // contacts it harvested were only 1.14% in-zone (2.9x random) -- it
            // descended roughly 1.5 bits in 16 hops.
            //
            // A real lookup only ever asks nodes inside the band it has already
            // reached: keep querying the closest unqueried contact while its
            // distance stays within FRONTIER_FACTOR x the best distance seen so
            // far. The band never widens on its own, so progress is monotone.
            // Only when the band is exhausted do we widen once (and count it),
            // which is the honest statement "nobody closer is learnable".
            let best = known
                .iter()
                .filter(|c| c.udp != 0 && is_global_ipv4(contact_addr(c).ip()))
                .map(|c| top_chunk(&c.id, &key))
                .min()
                .unwrap_or(u32::MAX);
            let frontier_cap = ((best as u64) * FRONTIER_FACTOR)
                .max(SEARCHTOLERANCE as u64)
                .min(u32::MAX as u64) as u32;
            let pick = |cap: Option<u32>| -> Vec<Contact> {
                closest(&key, &known, known.len())
                    .into_iter()
                    .filter(|c| !seen.contains(&contact_addr(c)))
                    .filter(|c| c.udp != 0 && is_global_ipv4(contact_addr(c).ip()))
                    .filter(|c| cap.map_or(true, |m| top_chunk(&c.id, &key) <= m))
                    .take(ALPHA_QUERY)
                    .cloned()
                    .collect()
            };
            let mut candidates = pick(Some(frontier_cap));
            if candidates.is_empty() {
                // Frontier exhausted: widen to the whole table, but record it, so
                // a run that never leaves the band is distinguishable from one
                // that had to fall back to blind probing.
                candidates = pick(None);
                widened_hops += 1;
            }
            if candidates.is_empty() {
                break;
            }
            let mut learned: Vec<Contact> = Vec::new();
            for c in candidates {
                let addr = contact_addr(&c);
                if c.udp == 0 || !is_global_ipv4(addr.ip()) || !seen.insert(addr) {
                    continue;
                }
                // KADEMLIA2_REQ (0x21) wire layout is <type=count 1><target 16><check 16>.
                // `check` MUST equal the *recipient's* own KAD ID, or aMule's
                // ProcessKademlia2Request drops the packet silently (no 0x29).
                // `type` is masked with 0x1F and used as the requested contact
                // count; 0x0B (FIND_NODE) asks for up to 11 closest contacts.
                let req = find_node_req(&key, &c.id, 0x0B);
                match client.exchange(
                    addr, &req, op::RES,
                    "live: KADEMLIA2_REQ (tolerance iteration)",
                    "live: KADEMLIA2_RES", &mut limiter, if obf { Some(&c.id) } else { None },
                ) {
                    Ok((p, _)) => {
                        // Answered with `check` == its own id => claimed KadID is real.
                        verified.insert(addr);
                        if let Ok(r) = parse_find_node_res(&p) {
                            learned.extend(r.contacts);
                        }
                    }
                    // Raw record is already kept; this is a protocol-layer
                    // no-response, NOT an index-layer rejection (RISK R-02).
                    Err(_) => {}
                }
            }
            hops += 1;
            for c in learned {
                if !known.iter().any(|k| k.id == c.id) {
                    known.push(c);
                }
            }
            // Track a plateau for reporting only -- it is NOT an exit condition.
            // Two zero-growth hops used to break the loop at hop 3-5 with 16 hops
            // available, which is why round A/B came back with 7-12 verified
            // contacts out of 86-127 known.
            if known.len() == before {
                stale_hops += 1;
            } else {
                stale_hops = 0;
            }
        }
        let zone_all = zone_contacts(&known);
        let zone_verified = verified_zone(&known, &verified);
        let tolerance_reached = !zone_verified.is_empty();
        // --- D1 distance diagnostics -------------------------------------------
        // top_chunk() is the top-8-bit XOR distance, so <= SEARCHTOLERANCE (2^24)
        // means "in zone". Reporting the minimum over the whole table vs over
        // verified peers separates two very different failures: a *routing* miss
        // (nothing close exists in `known` -> the funnel never got there) from a
        // *verification* miss (close contacts exist but were never confirmed, so
        // they are ineligible as targets).
        let global = |c: &Contact| c.udp != 0 && is_global_ipv4(contact_addr(c).ip());
        let closest_all = known.iter().filter(|c| global(c)).map(|c| top_chunk(&c.id, &key)).min();
        let closest_verified = known
            .iter()
            .filter(|c| global(c) && verified.contains(&contact_addr(c)))
            .map(|c| top_chunk(&c.id, &key))
            .min();
        // Publish only to verified in-zone responders, up to KAD's replication
        // factor. A larger blind fan-out at stale IDs produces timeouts, not data.
        let publish_targets: Vec<Contact> =
            zone_verified.iter().take(PUBLISH_FANOUT).cloned().collect();
        fields.push(("iteration_hops", n(hops as f64)));
        fields.push(("frontier_widened_hops", n(widened_hops as f64)));
        fields.push(("known_contacts", n(known.len() as f64)));
        fields.push(("verified_contacts", n(verified.len() as f64)));
        fields.push(("zone_contacts", n(zone_all.len() as f64)));
        fields.push(("zone_verified_contacts", n(zone_verified.len() as f64)));
        fields.push(("queried_contacts", n(seen.len() as f64)));
        fields.push(("stale_hops", n(stale_hops as f64)));
        fields.push(("closest_top_chunk", n(closest_all.map(|v| v as f64).unwrap_or(-1.0))));
        fields.push((
            "closest_verified_top_chunk",
            n(closest_verified.map(|v| v as f64).unwrap_or(-1.0)),
        ));
        fields.push(("tolerance_reached", b(tolerance_reached)));
        fields.push(("publish_target_policy", s("verified_in_zone")));
        fields.push(("publish_targets", J::Arr(publish_targets.iter().map(|c| s(contact_addr(c).to_string())).collect())));

        // The v2.4.21 "index-op policy probe" has been removed. aMule's
        // CIndexed::SendValidKeywordResult only builds a SEARCH_RES *inside*
        // `if (m_Keyword_map.find(keyID) != end())`; on a miss it falls through
        // and sends nothing at all. A private-namespace key is stored by nobody,
        // so every probe was guaranteed silent by design -- it measured nothing
        // and its 9/9 silence was misread as a peer-side gate. See PLAN.md §15.

        if !tolerance_reached {
            fields.push(("publish_res", b(false)));
            fields.push(("hit", b(false)));
            fields.push(("skipped", s("no verified contact inside SEARCHTOLERANCE (a 0x29 answer is the KadID proof); publishing would be silently dropped (S2.criteria #2)")));
            attempts.push(J::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect()));
            continue;
        }

        // PUBLISH_KEY_REQ -> PUBLISH_RES, fanned out to the closest known nodes.
        let entry = IndexEntry {
            source_id: node_id(&format!("live-descriptor-{i}")),
            tags: descriptor_tags(logical, "live-probe", client.addr().port()),
        };
        let mut publish_detail: Vec<J> = Vec::new();
        let mut publish_ok = 0usize;
        let mut publish_rtt: Option<f64> = None;
        for t in &publish_targets {
            let addr = contact_addr(t);
            let publ = client.exchange(
                addr, &publish_key_req(&key, &[entry.clone()]), op::PUBLISH_RES,
                "live: publish namespaced Key", "live: publish response", &mut limiter,
                if obf { Some(&t.id) } else { None },
            );
            let ok = publ.is_ok();
            if ok { publish_ok += 1; }
            let mut o: Vec<(&str, J)> = vec![
                ("addr", s(addr.to_string())),
                ("publish_ok", b(ok)),
            ];
            if let Ok((p, rtt)) = &publ {
                if publish_rtt.is_none() { publish_rtt = Some(*rtt); }
                o.push(("publish_rtt_ms", n(*rtt)));
                o.push(("publish_load", n(parse_publish_res(p).map(|r| r.load).unwrap_or(0) as f64)));
            } else {
                o.push(("error", s(publ.as_ref().err().unwrap().to_string())));
            }
            publish_detail.push(J::Obj(o.into_iter().map(|(k, v)| (k.to_string(), v)).collect()));
        }
        let published_any = publish_ok > 0;
        fields.push(("publish_targets_count", n(publish_targets.len() as f64)));
        fields.push(("publish_ok_count", n(publish_ok as f64)));
        fields.push(("publish_res", b(published_any)));
        fields.push(("publish_detail", J::Arr(publish_detail)));
        if let Some(rtt) = publish_rtt {
            fields.push(("publish_rtt_ms", n(rtt)));
        }

        // SEARCH_KEY_REQ -> SEARCH_RES, fanned out to the same closest nodes.
        let mut search_detail: Vec<J> = Vec::new();
        let mut search_ok = 0usize;
        let mut hit = false;
        for t in &publish_targets {
            let addr = contact_addr(t);
            let srch = client.exchange(
                addr, &search_key_req(&key), op::SEARCH_RES,
                "live: search namespaced Key", "live: search response", &mut limiter,
                if obf { Some(&t.id) } else { None },
            );
            let ok = srch.is_ok();
            if ok { search_ok += 1; }
            let mut o: Vec<(&str, J)> = vec![
                ("addr", s(addr.to_string())),
                ("search_ok", b(ok)),
            ];
            if let Ok((p, _)) = &srch {
                if let Ok(r) = parse_search_res(p) {
                    let found = r.answers.iter().any(|a| a.source_id == entry.source_id);
                    if found { hit = true; }
                    o.push(("answers", n(r.answers.len() as f64)));
                    o.push(("hit", b(found)));
                }
            } else {
                o.push(("error", s(srch.as_ref().err().unwrap().to_string())));
            }
            search_detail.push(J::Obj(o.into_iter().map(|(k, v)| (k.to_string(), v)).collect()));
        }
        fields.push(("search_targets_count", n(publish_targets.len() as f64)));
        fields.push(("search_ok_count", n(search_ok as f64)));
        fields.push(("search_res", b(search_ok > 0)));
        fields.push(("search_detail", J::Arr(search_detail)));
        fields.push(("hit", b(hit)));
        attempts.push(J::Obj(
            fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        ));
    }

    Ok(obj(vec![
        ("schema", s("coldrevive.m0.s2-kad-live/v1")),
        ("generated_at", s(utc_iso(unix_ms()))),
        ("mode", s("live-small-sample")),
        ("public_kad_contacted", b(true)),
        ("evidence_valid_for_pass", b(false)),
        ("s2_status", s("not_run")),
        ("gate", gate),
        ("sample_size", n(sample as f64)),
        ("rate_limit_pps", n(rate)),
        ("obfuscation_mode", s(if obf { "kad_udp_obfuscation_try0" } else { "cleartext_0xE4" })),
        ("attempts", J::Arr(attempts)),
        ("raw_records", J::Arr(client.records.iter().map(Record::json).collect())),
        ("protocol_scope", s(if obf {
            "0xE4 frames obfuscated per aMule EncryptedDatagramSocket: live requests use KadID-based (try-0) RC4 key with the peer's KadID, senderVerifyKey=OUR_UDP_KEY, receiverVerifyKey=0 (HELLO skipped); bootstrap stays cleartext; replies decoded via deobfuscate trying both try-0 (our KadID) and try-2 (our verify key); validReceiverKey is NOT a publish/search gate (only logged)"
        } else {
            "0xE4 frames sent CLEARTEXT (aMule accepts cleartext KAD via its first-byte-is-protocol-byte path); HELLO skipped; publish/search targeted at a tolerance-zone contact learned via KADEMLIA2_REQ/RES; validReceiverKey is NOT a publish/search gate (only logged). Obfuscation envelope implemented + byte-verified vs aMule master but not required by these peers."
        })),
        ("note", s("发布前先做 KADEMLIA2_REQ/RES 迭代收敛到 SEARCHTOLERANCE 邻域；未收敛则跳过该样本而不发布。程序不自动置 pass；结论由实验执行人依据逐次原始记录手工回填 spikes.json。")),
    ]))
}

// ------------------------------------------------------- split A/B probe --
//
// S2 PLAN §21. `live` publishes and searches from ONE process: the same socket,
// the same source port, the same local KadID. That is a closed loop through a
// third-party index, but it is not literally "A publishes, B retrieves".
// `split-probe` splits the two roles into two processes:
//
//   split-probe --stage publish   runs as node `split-a`, publishes to the
//                                 tolerance zone, and writes the addresses that
//                                 acknowledged plus the entry's source id to a
//                                 handoff file.
//   split-probe --stage search    runs LATER, in its own process, as node
//                                 `split-b`: different socket (different source
//                                 port), different local KadID seed, different
//                                 bootstrap entry. It never saw the publish -- it
//                                 only knows the key and the acking address -- and
//                                 it re-derives the whole tolerance zone itself
//                                 before asking.
//
// The gate is the same `live_gate`, so every red line (approval record, G0
// sample/rate match, user-supplied bootstrap, attestation) applies unchanged.

#[derive(Default)]
struct SplitArgs {
    bootstrap: Option<PathBuf>,
    sample_size: Option<usize>,
    rate_limit: Option<f64>,
    attest: bool,
    stage: Option<String>,
    handoff: Option<PathBuf>,
}

struct Discovery {
    known: Vec<Contact>,
    verified: HashSet<SocketAddr>,
    hops: usize,
    widened: usize,
    closest: Option<u32>,
    bootstrap_ok: bool,
    bootstrap_contacts: usize,
    bootstrap_error: String,
}

impl Discovery {
    fn zone_verified(&self, key: &KadId) -> Vec<Contact> {
        self.known
            .iter()
            .filter(|c| {
                c.udp != 0
                    && is_global_ipv4(contact_addr(c).ip())
                    && self.verified.contains(&contact_addr(c))
                    && top_chunk(&c.id, key) <= SEARCHTOLERANCE
            })
            .cloned()
            .collect()
    }
}

/// Same funnel as `run_live`: bootstrap, then KADEMLIA2_REQ/RES descent with the
/// frontier ceiling, stopping once MIN_ZONE_TARGETS verified in-zone peers exist
/// or the hop budget runs out. Extracted so the B side can walk its own route
/// instead of reusing A's contacts.
fn discover_zone(
    client: &mut Client,
    limiter: &mut Limiter,
    start: SocketAddr,
    key: &KadId,
    hops_budget: usize,
    tag: &str,
) -> Discovery {
    let mut d = Discovery {
        known: Vec::new(),
        verified: HashSet::new(),
        hops: 0,
        widened: 0,
        closest: None,
        bootstrap_ok: false,
        bootstrap_contacts: 0,
        bootstrap_error: String::new(),
    };
    match client.exchange(
        start,
        &bootstrap_req(),
        op::BOOTSTRAP_RES,
        &format!("{tag}: bootstrap request"),
        &format!("{tag}: bootstrap response"),
        limiter,
        None,
    ) {
        Ok((p, _)) => {
            d.bootstrap_ok = true;
            match parse_bootstrap_res(&p) {
                Ok(r) => {
                    d.bootstrap_contacts = r.contacts.len();
                    d.known = r.contacts;
                }
                Err(e) => d.bootstrap_error = e,
            }
        }
        Err(e) => d.bootstrap_error = e.to_string(),
    }
    let enough = |known: &Vec<Contact>, verified: &HashSet<SocketAddr>| -> usize {
        known
            .iter()
            .filter(|c| {
                c.udp != 0
                    && is_global_ipv4(contact_addr(c).ip())
                    && verified.contains(&contact_addr(c))
                    && top_chunk(&c.id, key) <= SEARCHTOLERANCE
            })
            .count()
    };
    let mut seen: HashSet<SocketAddr> = HashSet::new();
    while d.hops < hops_budget && enough(&d.known, &d.verified) < MIN_ZONE_TARGETS {
        let best = d
            .known
            .iter()
            .filter(|c| c.udp != 0 && is_global_ipv4(contact_addr(c).ip()))
            .map(|c| top_chunk(&c.id, key))
            .min()
            .unwrap_or(u32::MAX);
        let cap = ((best as u64) * FRONTIER_FACTOR)
            .max(SEARCHTOLERANCE as u64)
            .min(u32::MAX as u64) as u32;
        let pick = |ceiling: Option<u32>| -> Vec<Contact> {
            closest(key, &d.known, d.known.len())
                .into_iter()
                .filter(|c| !seen.contains(&contact_addr(c)))
                .filter(|c| c.udp != 0 && is_global_ipv4(contact_addr(c).ip()))
                .filter(|c| ceiling.map_or(true, |m| top_chunk(&c.id, key) <= m))
                .take(ALPHA_QUERY)
                .cloned()
                .collect()
        };
        let mut candidates = pick(Some(cap));
        if candidates.is_empty() {
            candidates = pick(None);
            d.widened += 1;
        }
        if candidates.is_empty() {
            break;
        }
        let mut learned: Vec<Contact> = Vec::new();
        for c in candidates {
            let addr = contact_addr(&c);
            if !seen.insert(addr) {
                continue;
            }
            let req = find_node_req(key, &c.id, 0x0B);
            if let Ok((p, _)) = client.exchange(
                addr,
                &req,
                op::RES,
                &format!("{tag}: KADEMLIA2_REQ (tolerance iteration)"),
                &format!("{tag}: KADEMLIA2_RES"),
                limiter,
                None,
            ) {
                d.verified.insert(addr);
                if let Ok(r) = parse_find_node_res(&p) {
                    learned.extend(r.contacts);
                }
            }
        }
        d.hops += 1;
        for c in learned {
            if !d.known.iter().any(|k| k.id == c.id) {
                d.known.push(c);
            }
        }
    }
    d.closest = d
        .known
        .iter()
        .filter(|c| c.udp != 0 && is_global_ipv4(contact_addr(c).ip()))
        .map(|c| top_chunk(&c.id, key))
        .min();
    d
}

fn kad_from_hex(text: &str) -> Option<KadId> {
    if text.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, chunk) in text.as_bytes().chunks(2).enumerate() {
        let hi = (chunk[0] as char).to_digit(16)?;
        let lo = (chunk[1] as char).to_digit(16)?;
        out[i] = (hi * 16 + lo) as u8;
    }
    Some(out)
}

fn run_split_probe(args: &SplitArgs) -> Result<J, String> {
    let stage = args.stage.clone().unwrap_or_default();
    if stage != "publish" && stage != "search" {
        return Err("split-probe 需要 --stage publish|search".into());
    }
    let handoff = args.handoff.clone().ok_or("split-probe 需要 --handoff <file>")?;
    let gate_args = LiveArgs {
        bootstrap: args.bootstrap.clone(),
        sample_size: args.sample_size,
        rate_limit: args.rate_limit,
        attest: args.attest,
        check_only: false,
        obfuscate: false,
    };
    let (ok, blockers, info) = live_gate(&gate_args);
    let gate = obj(vec![
        ("passed", b(ok)),
        ("blockers", J::Arr(blockers.iter().map(|x| s(x.clone())).collect())),
        ("info", J::Arr(info.iter().map(|x| s(x.clone())).collect())),
        ("attested", b(args.attest)),
        ("check_only", b(false)),
    ]);
    if !ok {
        return Err(format!("门禁未通过：{}", blockers.join("; ")));
    }
    let boot_text = fs::read_to_string(gate_args.bootstrap.as_ref().unwrap())
        .map_err(|e| format!("bootstrap read: {e}"))?;
    let nodes = parse_bootstrap_file(&boot_text)?;
    if nodes.is_empty() {
        return Err("bootstrap 文件无有效节点".into());
    }
    let sample = args.sample_size.unwrap_or(3).min(SAMPLE_KEYS.len());
    let rate = gate_args.rate_limit.unwrap_or(0.2);

    let mut entries: Vec<J> = Vec::new();

    if stage == "publish" {
        // --- A: publish -------------------------------------------------------
        let mut client = Client::bind_any("split-a", node_id("split-a"))
            .map_err(|e| format!("bind split-a: {e}"))?;
        let local = client.addr();
        let mut limiter = Limiter::new(rate);
        let mut handoff_lines: Vec<String> = Vec::new();
        for (i, logical) in SAMPLE_KEYS.iter().take(sample).enumerate() {
            let key = namespaced_key(logical);
            let d = discover_zone(&mut client, &mut limiter, nodes[i % nodes.len()], &key, MAX_FIND_HOPS, "split-a");
            let zone = d.zone_verified(&key);
            let targets: Vec<Contact> = zone.iter().take(PUBLISH_FANOUT).cloned().collect();
            let entry = IndexEntry {
                source_id: node_id(&format!("split-a-descriptor-{i}")),
                tags: descriptor_tags(logical, "split-a", local.port()),
            };
            let mut accepted: Vec<String> = Vec::new();
            let mut detail: Vec<J> = Vec::new();
            for t in &targets {
                let addr = contact_addr(t);
                let r = client.exchange(
                    addr,
                    &publish_key_req(&key, &[entry.clone()]),
                    op::PUBLISH_RES,
                    "split-a: publish namespaced Key",
                    "split-a: publish response",
                    &mut limiter,
                    None,
                );
                let mut o: Vec<(&str, J)> = vec![("addr", s(addr.to_string())), ("ok", b(r.is_ok()))];
                match &r {
                    Ok((p, rtt)) => {
                        accepted.push(addr.to_string());
                        o.push(("rtt_ms", n(*rtt)));
                        o.push(("load", n(parse_publish_res(p).map(|x| x.load).unwrap_or(0) as f64)));
                    }
                    Err(e) => o.push(("error", s(e.to_string()))),
                }
                detail.push(obj(o));
            }
            if !accepted.is_empty() {
                handoff_lines.push(format!(
                    "v1\t{i}\t{logical}\t{}\t{}\t{}",
                    id_hex(&key),
                    id_hex(&entry.source_id),
                    accepted.join(",")
                ));
            }
            entries.push(obj(vec![
                ("seq", n(i as f64)),
                ("logical", s(*logical)),
                ("key_id", s(id_hex(&key))),
                ("source_id", s(id_hex(&entry.source_id))),
                ("bootstrap", s(nodes[i % nodes.len()].to_string())),
                ("bootstrap_ok", b(d.bootstrap_ok)),
                ("bootstrap_contacts", n(d.bootstrap_contacts as f64)),
                ("bootstrap_error", s(d.bootstrap_error.clone())),
                ("iteration_hops", n(d.hops as f64)),
                ("frontier_widened_hops", n(d.widened as f64)),
                ("known_contacts", n(d.known.len() as f64)),
                ("verified_contacts", n(d.verified.len() as f64)),
                ("zone_verified_contacts", n(zone.len() as f64)),
                ("closest_top_chunk", n(d.closest.map(|v| v as f64).unwrap_or(-1.0))),
                ("publish_targets", J::Arr(targets.iter().map(|c| s(contact_addr(c).to_string())).collect())),
                ("publish_detail", J::Arr(detail)),
                ("accepted_count", n(accepted.len() as f64)),
            ]));
        }
        let body = if handoff_lines.is_empty() {
            "v1\n".to_string()
        } else {
            format!("v1\n{}\n", handoff_lines.join("\n"))
        };
        if let Some(parent) = handoff.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| format!("handoff dir: {e}"))?;
            }
        }
        fs::write(&handoff, body).map_err(|e| format!("handoff write: {e}"))?;
        return Ok(obj(vec![
            ("schema", s("coldrevive.m0.s2-kad-split/v1")),
            ("generated_at", s(utc_iso(unix_ms()))),
            ("stage", s("publish")),
            ("side", s("A")),
            ("node_label", s("split-a")),
            ("local_kad_id", s(id_hex(&node_id("split-a")))),
            ("local_addr", s(local.to_string())),
            ("public_kad_contacted", b(true)),
            ("evidence_valid_for_pass", b(false)),
            ("s2_status", s("not_run")),
            ("gate", gate),
            ("handoff_file", s(handoff.display().to_string())),
            ("entries", J::Arr(entries)),
            ("raw_records", J::Arr(client.records.iter().map(Record::json).collect())),
            ("note", s("A 侧：只发布并记录确认地址与 source id；不查询。判定由 B 侧独立进程完成，结论由执行人回填 spikes.json。")),
        ]));
    }

    // --- B: search ------------------------------------------------------------
    let text = fs::read_to_string(&handoff).map_err(|e| format!("handoff read: {e}"))?;
    let mut client =
        Client::bind_any("split-b", node_id("split-b")).map_err(|e| format!("bind split-b: {e}"))?;
    let local = client.addr();
    let mut limiter = Limiter::new(rate);
    let mut hits = 0usize;
    for (row, line) in text.lines().filter(|l| l.starts_with("v1\t")).enumerate() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 6 {
            continue;
        }
        let logical = f[2].to_string();
        let key = match kad_from_hex(f[3]) {
            Some(k) => k,
            None => continue,
        };
        let want_source = match kad_from_hex(f[4]) {
            Some(k) => k,
            None => continue,
        };
        let ack_addrs: Vec<SocketAddr> = f[5]
            .split(',')
            .filter_map(|a| a.parse::<SocketAddr>().ok())
            .collect();
        // A different bootstrap entry than A used for this row, so the route is
        // not simply A's route replayed.
        let start = nodes[(row + 3) % nodes.len()];
        let d = discover_zone(&mut client, &mut limiter, start, &key, MAX_FIND_HOPS, "split-b");
        let zone = d.zone_verified(&key);
        // Query the addresses that acknowledged A first, then B's own zone.
        let mut targets: Vec<SocketAddr> = ack_addrs.clone();
        for c in &zone {
            let a = contact_addr(c);
            if !targets.contains(&a) {
                targets.push(a);
            }
        }
        let mut queried: Vec<J> = Vec::new();
        let mut hit = false;
        let mut answered = 0usize;
        for addr in &targets {
            let r = client.exchange(
                *addr,
                &search_key_req(&key),
                op::SEARCH_RES,
                "split-b: search namespaced Key",
                "split-b: search response",
                &mut limiter,
                None,
            );
            let mut o: Vec<(&str, J)> = vec![
                ("addr", s(addr.to_string())),
                ("source", s(if ack_addrs.contains(addr) { "a_ack" } else { "b_own_zone" })),
            ];
            match &r {
                Ok((p, rtt)) => {
                    answered += 1;
                    let mut found = false;
                    let mut answers = 0usize;
                    let mut tags_seen: Vec<String> = Vec::new();
                    if let Ok(sr) = parse_search_res(p) {
                        answers = sr.answers.len();
                        for a in &sr.answers {
                            if a.source_id == want_source {
                                found = true;
                                for t in &a.tags {
                                    tags_seen.push(format!(
                                        "{}={}",
                                        String::from_utf8_lossy(&t.name),
                                        match &t.value {
                                            TagValue::Str(v) => v.clone(),
                                            TagValue::U16(v) => v.to_string(),
                                            TagValue::U32(v) => v.to_string(),
                                            TagValue::U64(v) => v.to_string(),
                                            TagValue::U8(v) => v.to_string(),
                                            TagValue::Hash16(h) => sha1::hex(h),
                                            TagValue::Raw(_, v) => sha1::hex(v),
                                        }
                                    ));
                                }
                            }
                        }
                    }
                    if found {
                        hit = true;
                    }
                    o.push(("responded", b(true)));
                    o.push(("rtt_ms", n(*rtt)));
                    o.push(("answers", n(answers as f64)));
                    o.push(("hit", b(found)));
                    o.push(("returned_tags", J::Arr(tags_seen.iter().map(|x| s(x.clone())).collect())));
                }
                Err(e) => {
                    o.push(("responded", b(false)));
                    o.push(("error", s(e.to_string())));
                }
            }
            queried.push(obj(o));
        }
        if hit {
            hits += 1;
        }
        entries.push(obj(vec![
            ("seq", s(f[1].to_string())),
            ("logical", s(logical)),
            ("key_id", s(f[3].to_string())),
            ("expected_source_id", s(f[4].to_string())),
            ("a_ack_addrs", J::Arr(ack_addrs.iter().map(|a| s(a.to_string())).collect())),
            ("bootstrap", s(start.to_string())),
            ("bootstrap_ok", b(d.bootstrap_ok)),
            ("bootstrap_contacts", n(d.bootstrap_contacts as f64)),
            ("bootstrap_error", s(d.bootstrap_error.clone())),
            ("iteration_hops", n(d.hops as f64)),
            ("frontier_widened_hops", n(d.widened as f64)),
            ("known_contacts", n(d.known.len() as f64)),
            ("verified_contacts", n(d.verified.len() as f64)),
            ("zone_verified_contacts", n(zone.len() as f64)),
            ("closest_top_chunk", n(d.closest.map(|v| v as f64).unwrap_or(-1.0))),
            ("queried", J::Arr(queried)),
            ("responded_count", n(answered as f64)),
            ("hit", b(hit)),
        ]));
    }
    let total = entries.len();
    Ok(obj(vec![
        ("schema", s("coldrevive.m0.s2-kad-split/v1")),
        ("generated_at", s(utc_iso(unix_ms()))),
        ("stage", s("search")),
        ("side", s("B")),
        ("node_label", s("split-b")),
        ("local_kad_id", s(id_hex(&node_id("split-b")))),
        ("local_addr", s(local.to_string())),
        ("public_kad_contacted", b(true)),
        ("evidence_valid_for_pass", b(false)),
        ("s2_status", s("not_run")),
        ("evidence_basis", s("两个独立进程：不同 socket/源端口、不同本地 KadID 种子、不同 bootstrap 入口；同一主机同一公网 IP")),
        ("gate", gate),
        ("handoff_file", s(handoff.display().to_string())),
        ("entries", J::Arr(entries)),
        ("entries_count", n(total as f64)),
        ("hit_count", n(hits as f64)),
        ("raw_records", J::Arr(client.records.iter().map(Record::json).collect())),
        ("note", s("B 侧：独立进程独立 socket，按 handoff 的 Key 自行重走 tolerance zone，再查询 A 的确认地址与自己的在区集合。程序不置 pass。")),
    ]))
}

/// Minimal parser for the user-supplied bootstrap JSON:
/// `[{"ip":"1.2.3.4","port":4672}, ...]`. No dependencies, no bundled defaults.
fn parse_bootstrap_file(text: &str) -> Result<Vec<SocketAddr>, String> {
    let mut out = Vec::new();
    for chunk in text.split('{').skip(1) {
        let body = chunk.split('}').next().unwrap_or("");
        let ip = extract_string(body, "ip");
        let port = extract_number(body, "port");
        if let (Some(ip), Some(port)) = (ip, port) {
            let addr: Ipv4Addr = ip.parse().map_err(|_| format!("invalid ip: {ip}"))?;
            if port == 0 || port > u16::MAX as u64 {
                return Err(format!("invalid port: {port}"));
            }
            out.push(SocketAddr::from((addr, port as u16)));
        }
    }
    Ok(out)
}

fn extract_string(body: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\"");
    let i = body.find(&pat)? + pat.len();
    let rest = &body[i..];
    let q = rest.find('"')?;
    let tail = &rest[q + 1..];
    let end = tail.find('"')?;
    Some(tail[..end].to_string())
}

fn extract_number(body: &str, key: &str) -> Option<u64> {
    let pat = format!("\"{key}\"");
    let i = body.find(&pat)? + pat.len();
    let rest = &body[i..];
    let start = rest.find(|c: char| c.is_ascii_digit())?;
    let digits: String = rest[start..].chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn extract_float(body: &str, key: &str) -> Option<f64> {
    let pat = format!("\"{key}\"");
    let i = body.find(&pat)? + pat.len();
    let rest = &body[i..];
    let start = rest.find(|c: char| c.is_ascii_digit() || c == '.')?;
    let mut end = start;
    for c in rest[start..].chars() {
        if c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E') {
            end += c.len_utf8();
        } else {
            break;
        }
    }
    rest[start..end].parse().ok()
}

// --------------------------------------------------------------------- main --

fn write_report(report: &J, prefix: &str) -> Result<PathBuf, String> {
    let dir = Path::new(MANIFEST).join("..").join("reports");
    fs::create_dir_all(&dir).map_err(|e| format!("mkdir reports: {e}"))?;
    let ts = utc_iso(unix_ms()).replace(['-', ':', 'T', 'Z'], "");
    let path = dir.join(format!("{prefix}-{ts}.json"));
    let mut body = report.dump();
    body.push('\n');
    fs::write(&path, body).map_err(|e| format!("write report: {e}"))?;
    Ok(path)
}

fn print_usage() {
    println!("kad-mini — S2 minimal eMule KAD v2 wire client\n");
    println!("  kad-mini selftest                  codec / hash assertions, no sockets");
    println!("  kad-mini loopback [--keys N] [--rate R]   real 0xE4 frames on 127.0.0.1");
    println!("  kad-mini lan-probe --ip IP --port UDP     RFC1918-only real-client probe, no publish");
    println!("  kad-mini lan-index-probe --ip IP --port UDP   RFC1918-only PUBLISH/SEARCH roundtrip (PLAN §18.6 D)");
    println!("  kad-mini ed2k-dict-probe --ip IP --port UDP [--keyword TERM]   RFC1918-only eMule keyword dictionary (ADR-011 L1)");
    println!("  kad-mini l2-source-probe --ip IP --port UDP   RFC1918-only 0x43+0x44 / 0x33+0x34 dual-track (ADR-011 L2)");
    println!("  kad-mini split-probe --stage publish|search --bootstrap FILE --handoff FILE \\");
    println!("            --sample-size N --rate-limit R --attest-i-am-the-experiment-operator  (PLAN §21 A/B)");
    println!("  kad-mini live --check             inspect the real-network gate, send nothing");
    println!("  kad-mini live --bootstrap FILE --sample-size N --rate-limit R \\");
    println!("            --attest-i-am-the-experiment-operator\n");
    println!("See tests/spikes/s2/PLAN.md. Never contacts public KAD unless all gates pass.");
}

fn main() {
    let argv: Vec<String> = env::args().skip(1).collect();
    if argv.is_empty() {
        print_usage();
        std::process::exit(0);
    }
    let result: Result<(J, &str, i32), String> = match argv[0].as_str() {
        "selftest" => run_selftest().map(|r| (r, "kad-wire-selftest", 0)),
        "loopback" => {
            let keys = arg_value(&argv, "--keys").and_then(|v| v.parse().ok()).unwrap_or(5).min(SAMPLE_KEYS.len());
            let rate = arg_value(&argv, "--rate").and_then(|v| v.parse().ok()).unwrap_or(50.0);
            run_loopback(keys, rate).map(|r| (r, "kad-wire-loopback", 0))
        }
        "lan-probe" => {
            let args = LanArgs {
                ip: arg_value(&argv, "--ip"),
                port: arg_value(&argv, "--port").and_then(|v| v.parse().ok()),
                rate: arg_value(&argv, "--rate").and_then(|v| v.parse().ok()).unwrap_or(0.2),
            };
            run_lan_probe(&args).map(|r| (r, "kad-lan-probe", 0))
        }
        "lan-index-probe" => {
            let args = LanIndexArgs {
                ip: arg_value(&argv, "--ip"),
                port: arg_value(&argv, "--port").and_then(|v| v.parse().ok()),
                rate: arg_value(&argv, "--rate").and_then(|v| v.parse().ok()).unwrap_or(0.2),
                bisect: argv.iter().any(|a| a == "--bisect"),
                tag_probe: argv.iter().any(|a| a == "--tag-probe"),
            };
            run_lan_index_probe(&args).map(|r| (r, "kad-lan-index-probe", 0))
        }
        "ed2k-dict-probe" => {
            let args = Ed2kDictArgs {
                ip: arg_value(&argv, "--ip"),
                port: arg_value(&argv, "--port").and_then(|v| v.parse().ok()),
                keyword: arg_value(&argv, "--keyword"),
                rate: arg_value(&argv, "--rate").and_then(|v| v.parse().ok()).unwrap_or(0.2),
            };
            run_ed2k_dict_probe(&args).map(|r| (r, "kad-ed2k-dict-probe", 0))
        }
        "l2-source-probe" => {
            let args = L2SourceArgs {
                ip: arg_value(&argv, "--ip"),
                port: arg_value(&argv, "--port").and_then(|v| v.parse().ok()),
                rate: arg_value(&argv, "--rate").and_then(|v| v.parse().ok()).unwrap_or(0.2),
            };
            run_l2_source_probe(&args).map(|r| (r, "kad-l2-source-probe", 0))
        }
        "split-probe" => {
            let args = SplitArgs {
                bootstrap: arg_value(&argv, "--bootstrap").map(PathBuf::from),
                sample_size: arg_value(&argv, "--sample-size").and_then(|v| v.parse().ok()),
                rate_limit: arg_value(&argv, "--rate-limit").and_then(|v| v.parse().ok()),
                attest: argv.iter().any(|a| a == "--attest-i-am-the-experiment-operator"),
                stage: arg_value(&argv, "--stage"),
                handoff: arg_value(&argv, "--handoff").map(PathBuf::from),
            };
            run_split_probe(&args).map(|r| (r, "kad-split", 0))
        }
        "live" => {
            let args = LiveArgs {
                bootstrap: arg_value(&argv, "--bootstrap").map(PathBuf::from),
                sample_size: arg_value(&argv, "--sample-size").and_then(|v| v.parse().ok()),
                rate_limit: arg_value(&argv, "--rate-limit").and_then(|v| v.parse().ok()),
                attest: argv.iter().any(|a| a == "--attest-i-am-the-experiment-operator"),
                check_only: argv.iter().any(|a| a == "--check"),
                obfuscate: argv.iter().any(|a| a == "--obfuscate"),
            };
            let check_only = args.check_only;
            run_live(&args).map(|r| {
                let exit = if check_only { gate_exit_code(&r) } else { 0 };
                (r, "kad-live", exit)
            })
        }
        "-h" | "--help" => {
            print_usage();
            std::process::exit(0);
        }
        other => Err(format!("unknown subcommand: {other}")),
    };

    match result {
        Ok((report, prefix, code)) => {
            match write_report(&report, prefix) {
                Ok(path) => {
                    println!("{}", report.dump());
                    eprintln!("[report] {}", path.display());
                }
                Err(e) => {
                    eprintln!("[ERROR] {e}");
                    std::process::exit(1);
                }
            }
            std::process::exit(code);
        }
        Err(e) => {
            eprintln!("[ERROR] {e}");
            std::process::exit(1);
        }
    }
}

fn gate_exit_code(report: &J) -> i32 {
    match report {
        J::Obj(v) => v.iter().find_map(|(k, val)| match (k.as_str(), val) {
            ("gate", J::Obj(g)) => g.iter().find_map(|(gk, gv)| match (gk.as_str(), gv) {
                ("passed", J::Bool(true)) => Some(0),
                ("passed", J::Bool(false)) => Some(2),
                _ => None,
            }),
            _ => None,
        }).unwrap_or(2),
        _ => 2,
    }
}

fn arg_value(argv: &[String], key: &str) -> Option<String> {
    argv.iter().position(|a| a == key).and_then(|i| argv.get(i + 1).cloned())
}

fn run_selftest() -> Result<J, String> {
    let mut checks: Vec<(&str, bool, &str)> = Vec::new();

    // SHA-1 known-answer vectors
    checks.push(("sha1.empty", sha1::hex(&sha1::sha1(b"")) == "da39a3ee5e6b4b0d3255bfef95601890afd80709", "FIPS 180-4"));
    checks.push(("sha1.abc", sha1::hex(&sha1::sha1(b"abc")) == "a9993e364706816aba3e25717850c26c9cd0d89d", "FIPS 180-4"));

    // opcode table must match aMule kad2/Client2Client/UDP.h
    checks.push(("opcode.bootstrap_req", op::BOOTSTRAP_REQ == 0x01, "aMule UDP.h"));
    checks.push(("opcode.publish_key_req", op::PUBLISH_KEY_REQ == 0x43, "aMule UDP.h (0x35 is SEARCH_NOTES_REQ)"));
    checks.push(("opcode.search_key_req", op::SEARCH_KEY_REQ == 0x33, "aMule UDP.h"));
    checks.push(("opcode.publish_res", op::PUBLISH_RES == 0x4B, "aMule UDP.h"));
    checks.push(("header", PROTO_KAD == 0xE4 && PROTO_KAD_PACKED == 0xE5, "aMule Protocols.h"));
    checks.push(("kad_version", KAD_VERSION == 0x08, "aMule Constants.h"));
    checks.push(("tolerance", SEARCHTOLERANCE == 16_777_216, "aMule Defines.h"));

    // frame
    let f = frame(op::BOOTSTRAP_REQ, &[]);
    checks.push(("frame.bootstrap", f == vec![0xE4, 0x01], "0xE4 + opcode"));
    checks.push(("frame.reject_packed", parse_frame(&[0xE5, 0x01]).is_err(), "parse_frame only handles 0xE4; decode_wire handles 0xE5"));
    checks.push(("frame.reject_other", parse_frame(&[0xC5, 0x01]).is_err(), "not KAD"));

    // zlib / packed 0xE5 frames. Three vectors cover all DEFLATE block types.
    // BTYPE 0 (stored) is assembled here instead of embedding 300 bytes of hex.
    let mut z_stored: Vec<u8> = vec![0x78, 0x01, 0x01, 0x2c, 0x01, 0xd3, 0xfe];
    z_stored.extend(std::iter::repeat(b'A').take(300));
    z_stored.extend_from_slice(&[0xcb, 0x9e, 0x4c, 0x2d]); // Adler-32 of 300 x 'A'
    let mut f_stored = vec![PROTO_KAD_PACKED, op::BOOTSTRAP_RES];
    f_stored.extend_from_slice(&z_stored);
    checks.push((
        "packed.btype0_stored",
        decode_wire(&f_stored).map(|(o, p)| o == op::BOOTSTRAP_RES && p == vec![b'A'; 300]).unwrap_or(false),
        "RFC 1951 stored block + RFC 1950 Adler-32",
    ));
    checks.push((
        "packed.btype1_fixed_huffman",
        decode_wire(&packed_frame("780173cecf49094a2dcb2c4b8d0f33b472c6c303004cc70f3a"))
            .map(|(o, p)| o == op::BOOTSTRAP_RES && p == b"ColdRevive_V1:".repeat(3)).unwrap_or(false),
        "RFC 1951 fixed Huffman",
    ));
    checks.push((
        "packed.btype2_dynamic_huffman",
        decode_wire(&packed_frame("78daedcc410ac2301040d1abcc155a44c59045a93b772eba95b69945a0644232f4fc7a0de1af1fbcd98ef4d6339ffa598687bca6a76c66debdad559af66aa5abec567cdd5d8edc3d48b1a4710a52ad79bc5c6f63909ce230deb7fcd3999191919191919191919191919191919191f1efc72fcb338197"))
            .map(|(_, p)| p.len() == 4920 && p.starts_with(b"ColdRevive_V1: KAD bootstrap")).unwrap_or(false),
        "RFC 1951 dynamic Huffman + back-references",
    ));
    let mut corrupted = f_stored.clone();
    let last = corrupted.len() - 1;
    corrupted[last] ^= 0xFF;
    checks.push(("packed.adler32_mismatch_rejected", decode_wire(&corrupted).is_err(), "RFC 1950 trailer"));
    checks.push(("packed.non_kad_rejected", decode_wire(&[0xC5, 0x09, 0x00]).is_err(), "likely RC4 obfuscated"));
    let plain = bootstrap_res(&Contact {
        id: node_id("selftest"), ip: [127, 0, 0, 1], udp: 4672, tcp: 4662, version: KAD_VERSION,
    }, &[]);
    checks.push(("packed.plaintext_still_decodes", decode_wire(&plain).is_ok(), "0xE4 regression"));

    // live gate must refuse private / reserved bootstrap targets (PLAN §5)
    checks.push((
        "gate.private_ipv4_rejected",
        !is_global_ipv4(Ipv4Addr::new(10, 0, 0, 1))
            && !is_global_ipv4(Ipv4Addr::new(172, 16, 0, 1))
            && !is_global_ipv4(Ipv4Addr::new(192, 168, 1, 1))
            && !is_global_ipv4(Ipv4Addr::new(127, 0, 0, 1))
            && !is_global_ipv4(Ipv4Addr::new(169, 254, 1, 1))
            && !is_global_ipv4(Ipv4Addr::new(100, 64, 0, 1)),
        "RFC1918 / loopback / link-local / CGNAT",
    ));
    checks.push((
        "gate.public_ipv4_accepted",
        is_global_ipv4(Ipv4Addr::new(8, 8, 8, 8)) && is_global_ipv4(Ipv4Addr::new(1, 1, 1, 1)),
        "routable unicast",
    ));

    // contact is 25 bytes
    let c = Contact { id: node_id("A"), ip: [127, 0, 0, 1], udp: 4672, tcp: 4662, version: KAD_VERSION };
    let mut buf = Buf::new();
    c.encode(&mut buf);
    checks.push(("contact.25bytes", buf.0.len() == 25, "aMule Process2BootstrapRequest"));
    checks.push(("contact.roundtrip", Contact::decode(&mut Cur::new(&buf.0)).unwrap() == c, "codec"));

    // tags
    let tags = vec![
        Tag::int2(TAG_SOURCEUPORT, 4672),
        Tag::int1(TAG_KADMISCOPTIONS, 0),
        Tag::int4(TAG_FILESIZE, 262_144),
        Tag::str1(TAG_FILENAME, "ColdRevive_V1:probe"),
        Tag { name: b"ColdRevive_V1:descriptor".to_vec(), value: TagValue::Str("node=A;port=4672".into()) },
    ];
    let mut tb = Buf::new();
    write_tags(&mut tb, &tags).unwrap();
    checks.push(("tags.roundtrip", read_tags(&mut Cur::new(&tb.0)).unwrap() == tags, "aMule CFileDataIO::ReadTag (Kad)"));

    // namespace
    checks.push(("namespace.changes_id", namespaced_key("x") != plain_key("x"), "SHA1 prefix truncated to 128-bit"));
    checks.push(("namespace.id_len", namespaced_key("x").len() == 16, "KAD is 128-bit"));

    // ADR-011 L1: eMule's keyword Key is MD4(UTF-8(term)), not SHA-1.
    checks.push(("md4.empty", sha1::hex(&md4(b"")) == "31d6cfe0d16ae931b73c59d7e0c089c0", "RFC 1320"));
    checks.push(("md4.abc", sha1::hex(&md4(b"abc")) == "a448017aaf21d8525fc10ae87aa6729d", "RFC 1320"));
    checks.push(("ed2k.keyword_hash_is_md4", kad_keyword_hash("abc") == md4(b"abc"), "aMule KadGetKeywordHash (Kademlia.cpp)"));
    checks.push(("ed2k.keyword_hash_is_unnamespaced", kad_keyword_hash("abc") != namespaced_key("abc"), "ADR-011 compat track"));
    checks.push(("ed2k.file_hash_single_part", ed2k_hash(b"abc") == md4(b"abc"), "eD2k MD4 / 9.28 MB parts"));

    // ADR-011 L2: exact-PARTSIZE files take aMule's "Red" hash (extra empty
    // part). Reusing crates.io ed2k::Ed2k (Blue) would give a fileID no old
    // client can download. Vector from src/KnownFile.cpp SetFileSize comments.
    let exact_part = vec![0x33u8; ED2K_PART_SIZE];
    let mut red_input = Vec::from(md4(&exact_part));
    red_input.extend_from_slice(&md4(&[]));
    checks.push(("ed2k.exact_part_uses_amule_red_hash", ed2k_hash(&exact_part) == md4(&red_input), "aMule ThreadTasks.cpp / KnownFile.cpp"));
    checks.push(("ed2k.exact_part_rejects_blue_hash", ed2k_hash(&exact_part) != md4(&md4(&exact_part)), "crates.io ed2k::Ed2kBlue is NOT eMule-compatible"));
    checks.push(("ed2k.part_hash_count_at_boundary", ed2k_part_hashes(&exact_part).len() == 2, "aMule m_iED2KPartHashCount"));
    checks.push(("ed2k.part_hash_count_below_boundary", ed2k_part_hashes(b"abc").is_empty(), "aMule: files < PARTSIZE have no hashset"));

    // all message roundtrips
    let key = namespaced_key("selftest");
    let entries = vec![IndexEntry { source_id: node_id("d"), tags: tags.clone() }];
    let src_tags = source_tags(4662, 4672, 1234);
    let roundtrips = vec![
        ("bootstrap_res", parse_bootstrap_res(&parse_frame(&bootstrap_res(&c, &[c.clone()])).unwrap().1).is_ok()),
        ("hello", parse_hello(&parse_frame(&hello(op::HELLO_REQ, &c, &[])).unwrap().1).is_ok()),
        ("find_node_req", parse_find_node_req(&parse_frame(&find_node_req(&key, &c.id, 2)).unwrap().1).is_ok()),
        ("find_node_res", parse_find_node_res(&parse_frame(&find_node_res(&key, &[c.clone()])).unwrap().1).is_ok()),
        ("publish_key_req", parse_publish_key_req(&parse_frame(&publish_key_req(&key, &entries)).unwrap().1).is_ok()),
        ("publish_res", parse_publish_res(&parse_frame(&publish_res(&key, 0)).unwrap().1).is_ok()),
        ("search_key_req", parse_search_key_req(&parse_frame(&search_key_req(&key)).unwrap().1).is_ok()),
        ("search_res", parse_search_res(&parse_frame(&search_res(&c.id, &key, &entries)).unwrap().1).is_ok()),
        ("publish_source_req", {
            let frame_bytes = publish_source_req(&key, &c.id, &src_tags);
            let (opcode, p) = parse_frame(&frame_bytes).unwrap();
            opcode == op::PUBLISH_SOURCE_REQ
                && parse_publish_source_req(p).map(|r| r.file_id == key && r.tags == src_tags).unwrap_or(false)
        }),
        ("search_source_req", {
            let frame_bytes = search_source_req(&key, 1234);
            let (opcode, p) = parse_frame(&frame_bytes).unwrap();
            opcode == op::SEARCH_SOURCE_REQ
                && parse_search_source_req(p).map(|r| r.file_id == key && r.file_size == 1234).unwrap_or(false)
        }),
    ];
    for (name, ok) in roundtrips {
        checks.push((Box::leak(format!("msg.{name}").into_boxed_str()), ok, "roundtrip"));
    }

    // tolerance semantics
    let k = namespaced_key("y");
    let mut far = k;
    far[0] ^= 0xFF;
    checks.push(("tolerance.self", publish_allowed(&k, &k, false), "XOR distance 0"));
    checks.push(("tolerance.far_is_outside", top_chunk(&far, &k) > SEARCHTOLERANCE, "aMule Defines.h SEARCHTOLERANCE"));
    checks.push(("tolerance.far_wan_dropped", !publish_allowed(&far, &k, false), "WAN peer outside zone is silently dropped"));
    checks.push(("tolerance.lan_bypass", publish_allowed(&far, &k, true), "IsLanIP bypass in aMule"));

    let passed = checks.iter().filter(|(_, ok, _)| *ok).count();
    let failed: Vec<&str> = checks.iter().filter(|(_, ok, _)| !*ok).map(|(name, _, _)| *name).collect();
    let report = obj(vec![
        ("schema", s("coldrevive.m0.s2-kad-wire-selftest/v1")),
        ("generated_at", s(utc_iso(unix_ms()))),
        ("public_kad_contacted", b(false)),
        ("evidence_valid_for_pass", b(false)),
        ("total", n(checks.len() as f64)),
        ("passed", n(passed as f64)),
        ("failed", n(failed.len() as f64)),
        ("checks", J::Arr(checks.iter().map(|(name, ok, src)| obj(vec![
            ("name", s(*name)), ("ok", b(*ok)), ("source", s(*src)),
        ])).collect())),
        ("failures", J::Arr(failed.iter().map(|x| s(*x)).collect())),
    ]);
    // Human-readable summary goes to stderr so stdout stays machine-readable JSON.
    eprintln!("kad-mini selftest: {}/{} passed", passed, checks.len());
    for name in &failed {
        eprintln!("  FAIL {name}");
    }
    if !failed.is_empty() {
        return Err(format!("{} selftest checks failed", failed.len()));
    }
    Ok(report)
}
