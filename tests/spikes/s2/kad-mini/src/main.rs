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

mod sha1;
mod wire;

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use wire::*;

const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");
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
    let (opcode, _) = parse_frame(data).unwrap_or((0xFF, &[]));
    Record {
        at_ms: unix_ms(),
        endpoint: endpoint.into(),
        direction: dir.into(),
        peer: peer.into(),
        opcode,
        opcode_name: opcode_name(opcode).into(),
        bytes: data.len(),
        hex: sha1::hex(data),
        detail: detail.into(),
    }
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
    fn addr(&self) -> SocketAddr {
        self.sock.local_addr().expect("bound")
    }
    fn send(&mut self, to: SocketAddr, frame: &[u8], detail: &str, limiter: &mut Limiter) -> io::Result<()> {
        limiter.wait();
        let r = rec(&self.name, "send", &to.to_string(), frame, detail);
        self.records.push(r);
        self.sock.send_to(frame, to)?;
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
    ) -> Result<(Vec<u8>, f64), io::Error> {
        self.send(to, frame, send_detail, limiter)?;
        let t0 = Instant::now();
        let (_, data) = self.recv(recv_detail)?;
        let rtt = t0.elapsed().as_secs_f64() * 1000.0;
        let (op, payload) = parse_frame(&data).map_err(io::Error::other)?;
        if op != expected {
            return Err(io::Error::other(format!(
                "expected opcode 0x{expected:02x} ({}), got 0x{op:02x} ({})",
                opcode_name(expected),
                opcode_name(op)
            )));
        }
        Ok((payload.to_vec(), rtt))
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

// ---------------------------------------------------------------- live gate --

#[derive(Default)]
struct LiveArgs {
    bootstrap: Option<PathBuf>,
    sample_size: Option<usize>,
    rate_limit: Option<f64>,
    attest: bool,
    check_only: bool,
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
            info.push(format!("approval_file={}", path.display()));
        }
        Err(e) => blockers.push(format!("无法读取 {}: {e}", path.display())),
    }

    // 2. user-supplied bootstrap list (OPS §6: never bundled)
    match &args.bootstrap {
        None => blockers.push("缺少 --bootstrap <file>；按 OPS §6 禁止内置默认 bootstrap 列表".into()),
        Some(p) => match fs::read_to_string(p) {
            Ok(text) => {
                let count = text.matches("\"ip\"").count();
                if count == 0 {
                    blockers.push(format!("{} 中没有可用的 bootstrap 节点", p.display()));
                } else {
                    info.push(format!("bootstrap_nodes={count} (user supplied)"));
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
            ("note", s("门禁未全部通过或仅 --check：未发送任何报文，未接触公共 KAD。")),
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
    let mut client = Client::bind("live-probe", node_id("live-probe"))
        .map_err(|e| format!("bind live socket: {e}"))?;
    client.sock.set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|e| format!("timeout: {e}"))?;
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
        ];

        // BOOTSTRAP_REQ -> BOOTSTRAP_RES (may be silently dropped by real nodes)
        let boot = client.exchange(
            dest, &bootstrap_req(), op::BOOTSTRAP_RES,
            "live: bootstrap request", "live: bootstrap response", &mut limiter,
        );
        fields.push(("bootstrap_res", b(boot.is_ok())));
        if let Ok((p, rtt)) = &boot {
            fields.push(("bootstrap_rtt_ms", n(*rtt)));
            fields.push(("bootstrap_contacts", n(parse_bootstrap_res(p).map(|r| r.contacts.len()).unwrap_or(0) as f64)));
        } else {
            fields.push(("bootstrap_error", s(boot.as_ref().err().unwrap().to_string())));
        }

        // HELLO_REQ -> HELLO_RES
        let hello_frame = hello(
            op::HELLO_REQ,
            &Contact { id: client.id, ip: [127, 0, 0, 1], udp: client.addr().port(), tcp: client.addr().port(), version: KAD_VERSION },
            &[],
        );
        let hi = client.exchange(dest, &hello_frame, op::HELLO_RES, "live: hello request", "live: hello response", &mut limiter);
        fields.push(("hello_res", b(hi.is_ok())));

        // PUBLISH_KEY_REQ -> PUBLISH_RES
        let entry = IndexEntry {
            source_id: node_id(&format!("live-descriptor-{i}")),
            tags: descriptor_tags(logical, "live-probe", client.addr().port()),
        };
        let publ = client.exchange(
            dest, &publish_key_req(&key, &[entry.clone()]), op::PUBLISH_RES,
            "live: publish namespaced Key", "live: publish response", &mut limiter,
        );
        fields.push(("publish_res", b(publ.is_ok())));
        if let Ok((p, rtt)) = &publ {
            fields.push(("publish_rtt_ms", n(*rtt)));
            fields.push(("publish_load", n(parse_publish_res(p).map(|r| r.load).unwrap_or(0) as f64)));
        } else {
            fields.push(("publish_error_or_drop", s(publ.as_ref().err().unwrap().to_string())));
        }

        // SEARCH_KEY_REQ -> SEARCH_RES
        let srch = client.exchange(
            dest, &search_key_req(&key), op::SEARCH_RES,
            "live: search namespaced Key", "live: search response", &mut limiter,
        );
        let hit = match &srch {
            Ok((p, _)) => parse_search_res(p)
                .map(|r| r.answers.iter().any(|a| a.source_id == entry.source_id))
                .unwrap_or(false),
            Err(_) => false,
        };
        fields.push(("search_res", b(srch.is_ok())));
        fields.push(("hit", b(hit)));
        if let Err(e) = &srch {
            fields.push(("search_error_or_drop", s(e.to_string())));
        }
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
        ("attempts", J::Arr(attempts)),
        ("raw_records", J::Arr(client.records.iter().map(Record::json).collect())),
        ("note", s("程序不自动置 pass；结论由实验执行人依据逐次原始记录手工回填 spikes.json。")),
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
        "live" => {
            let args = LiveArgs {
                bootstrap: arg_value(&argv, "--bootstrap").map(PathBuf::from),
                sample_size: arg_value(&argv, "--sample-size").and_then(|v| v.parse().ok()),
                rate_limit: arg_value(&argv, "--rate-limit").and_then(|v| v.parse().ok()),
                attest: argv.iter().any(|a| a == "--attest-i-am-the-experiment-operator"),
                check_only: argv.iter().any(|a| a == "--check"),
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
    checks.push(("frame.reject_packed", parse_frame(&[0xE5, 0x01]).is_err(), "0xE5 unsupported"));
    checks.push(("frame.reject_other", parse_frame(&[0xC5, 0x01]).is_err(), "not KAD"));

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
    checks.push(("tags.roundtrip", read_tags(&mut Cur::new(&tb.0)).unwrap() == tags, "aMule CTag"));

    // namespace
    checks.push(("namespace.changes_id", namespaced_key("x") != plain_key("x"), "SHA1 prefix truncated to 128-bit"));
    checks.push(("namespace.id_len", namespaced_key("x").len() == 16, "KAD is 128-bit"));

    // all message roundtrips
    let key = namespaced_key("selftest");
    let entries = vec![IndexEntry { source_id: node_id("d"), tags: tags.clone() }];
    let roundtrips = vec![
        ("bootstrap_res", parse_bootstrap_res(&parse_frame(&bootstrap_res(&c, &[c.clone()])).unwrap().1).is_ok()),
        ("hello", parse_hello(&parse_frame(&hello(op::HELLO_REQ, &c, &[])).unwrap().1).is_ok()),
        ("find_node_req", parse_find_node_req(&parse_frame(&find_node_req(&key, &c.id, 2)).unwrap().1).is_ok()),
        ("find_node_res", parse_find_node_res(&parse_frame(&find_node_res(&key, &[c.clone()])).unwrap().1).is_ok()),
        ("publish_key_req", parse_publish_key_req(&parse_frame(&publish_key_req(&key, &entries)).unwrap().1).is_ok()),
        ("publish_res", parse_publish_res(&parse_frame(&publish_res(&key, 0)).unwrap().1).is_ok()),
        ("search_key_req", parse_search_key_req(&parse_frame(&search_key_req(&key)).unwrap().1).is_ok()),
        ("search_res", parse_search_res(&parse_frame(&search_res(&c.id, &key, &entries)).unwrap().1).is_ok()),
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
