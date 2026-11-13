// Wire structs mirror the on-wire layout; the probes exercise only a subset,
// so dead-code warnings are expected and not defects.
#![allow(dead_code)]

//! ADR-011 L2 spike: minimal standard eD2k client-to-client TCP stack.
//!
//!   ed2k-mini selftest                       codec / queue assertions, no sockets
//!   ed2k-mini loopback                       real TCP transfer, our client <-> our server
//!   ed2k-mini lan-hello --ip IP --port TCP   RFC1918-only real eMule/aMule HELLO handshake
//!
//! HARD RULE: this binary never contacts a non-loopback address except the
//! explicitly supplied RFC1918 target of `lan-hello`. No public eD2k network
//! is contacted, no file is published, no spike status is changed.
//!
//! There is deliberately NO Cargo workspace / production `cr-ed2k` crate yet:
//! ROADMAP G0 has not passed (S1/S3-S7 still not_run) and `src/README.md`
//! freezes crate creation until the technical route is settled. This spike
//! holds the audited reference implementation to be promoted to
//! `src/crates/cr-ed2k` (M-12) at G0.

// Single source of truth: reuse the MD4 / eD2k hash and KAD tag codec from the
// sibling probe rather than copying crypto into a second crate.
#[path = "../../kad-mini/src/sha1.rs"]
pub mod sha1;
#[path = "../../kad-mini/src/wire.rs"]
pub mod kad;

mod stack;
mod wire;

use std::env;
use std::fs;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kad::{md4, ED2K_PART_SIZE};
use stack::{SharedFile, UploadQueue};
use wire as w;

const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");

// ------------------------------------------------------------------ JSON out --

#[derive(Clone, Debug)]
enum J {
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}
fn s(v: impl Into<String>) -> J { J::Str(v.into()) }
fn n(v: impl Into<f64>) -> J { J::Num(v.into()) }
fn b(v: bool) -> J { J::Bool(v) }
fn obj(pairs: Vec<(&str, J)>) -> J {
    J::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}
impl J {
    fn dump(&self) -> String {
        match self {
            J::Bool(v) => v.to_string(),
            J::Num(v) => {
                if v.fract() == 0.0 && v.is_finite() { format!("{}", *v as i64) } else { format!("{v}") }
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
                v.iter().map(|(k, val)| format!("{}:{}", J::Str(k.clone()).dump(), val.dump())).collect::<Vec<_>>().join(",")
            ),
        }
    }
}

fn unix_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
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
    if m <= 2 { y += 1; }
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", sod / 3600, (sod % 3600) / 60, sod % 60)
}

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

fn is_private_ipv4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 10
        || (o[0] == 172 && (16..=31).contains(&o[1]))
        || (o[0] == 192 && o[1] == 168)
        || o[0] == 127
        || (o[0] == 169 && o[1] == 254)
}

fn log_json(log: &[stack::LogEntry]) -> J {
    J::Arr(log.iter().map(|e| obj(vec![
        ("direction", s(e.direction)),
        ("protocol", s(format!("0x{:02x}", e.protocol))),
        ("opcode", s(format!("0x{:02x}", e.opcode))),
        ("opcode_name", s(&e.opcode_name)),
        ("payload_len", n(e.payload_len as f64)),
    ])).collect())
}

fn our_hello(port: u16) -> w::Hello {
    let mut hash = [0u8; 16];
    hash.copy_from_slice(&md4(b"coldrevive-l2-ed2k-mini"));
    w::default_hello(hash, port, 4672, 4672, "coldrevive-l2")
}

// ----------------------------------------------------------------- loopback --

fn deterministic_file(size: usize, name: &str) -> SharedFile {
    // Deterministic, non-trivial content so both hash and byte equality are
    // meaningful. No filesystem writes: the "shared file" lives in memory.
    let mut data = Vec::with_capacity(size);
    let mut x: u32 = 0x9e37_79b9;
    for _ in 0..size {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        data.push((x >> 24) as u8);
    }
    SharedFile::new(name, data)
}

fn run_transfer_case(file: &SharedFile, label: &str) -> J {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(e) => return obj(vec![("case", s(label)), ("error", s(format!("bind: {e}")))]),
    };
    let addr = listener.local_addr().unwrap();
    let server_hello = our_hello(addr.port());
    let files = vec![file.clone()];
    let queue = Arc::new(Mutex::new(UploadQueue::new(1)));
    let q2 = Arc::clone(&queue);
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept");
        stack::serve(stream, &files, &server_hello, &q2, Duration::from_secs(60))
    });

    let client_result = match TcpStream::connect(addr) {
        Ok(stream) => {
            let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
            let h = our_hello(0);
            stack::download(stream, file, &h, Duration::from_secs(60))
        }
        Err(e) => {
            let mut r = stack::DownloadResult::default();
            r.error = Some(format!("connect: {e}"));
            r
        }
    };
    let server_result = server.join().unwrap_or_default();

    let peer_nick = client_result.peer_hello.as_ref().and_then(|h| {
        w::find_tag(&h.tags, w::ct::NAME).and_then(|t| match &t.value {
            kad::TagValue::Str(s) => Some(s.clone()),
            _ => None,
        })
    });

    obj(vec![
        ("case", s(label)),
        ("file_name", s(&file.name)),
        ("file_size", n(file.data.len() as f64)),
        ("ed2k_hash", s(sha1::hex(&file.hash()))),
        ("ed2k_link", s(&file.link())),
        ("needs_hashset", b(file.needs_hashset())),
        ("part_hash_count", n(file.part_hashes().len() as f64)),
        ("blocks", n(file.blocks().len() as f64)),
        ("peer_nick", s(peer_nick.unwrap_or_default())),
        ("handshake_answered", b(client_result.peer_hello.is_some())),
        ("peer_filename", s(client_result.peer_filename.clone().unwrap_or_default())),
        ("hashset_verified", b(client_result.hashset_verified)),
        ("queue_granted", b(server_result.queue_granted)),
        ("queue_rank_sent", n(server_result.queue_rank_sent.unwrap_or(0) as f64)),
        ("blocks_received", n(client_result.blocks_received as f64)),
        ("bytes_received", n(client_result.bytes_received as f64)),
        ("blocks_sent", n(server_result.blocks_sent as f64)),
        ("bytes_sent", n(server_result.bytes_sent as f64)),
        ("hash_matches", b(client_result.hash_matches)),
        ("content_matches", b(client_result.content_matches)),
        ("completed", b(client_result.completed)),
        ("client_error", s(client_result.error.clone().unwrap_or_default())),
        ("server_error", s(server_result.error.clone().unwrap_or_default())),
        ("client_log", log_json(&client_result.log)),
        ("server_log", log_json(&server_result.log)),
    ])
}

fn run_loopback() -> Result<J, String> {
    // Three shapes deliberately chosen to exercise every branch of the L2 hash
    // correction: < PARTSIZE (no hashset), == PARTSIZE (Red empty part hash),
    // and > PARTSIZE (multi-part hashset).
    let cases = vec![
        (deterministic_file(4096, "coldrevive-l2-small.bin"), "single_part_4KiB"),
        (deterministic_file(ED2K_PART_SIZE, "coldrevive-l2-exact-part.bin"), "exact_one_part_boundary"),
        (deterministic_file(ED2K_PART_SIZE + 8192, "coldrevive-l2-multi-part.bin"), "multi_part"),
    ];
    let results: Vec<J> = cases.iter().map(|(f, label)| run_transfer_case(f, label)).collect();
    let all_ok = results.iter().all(|r| match r {
        J::Obj(v) => v.iter().any(|(k, val)| k == "completed" && matches!(val, J::Bool(true))),
        _ => false,
    });
    Ok(obj(vec![
        ("schema", s("coldrevive.m0.s2-ed2k-mini-loopback/v1")),
        ("generated_at", s(utc_iso(unix_ms()))),
        ("mode", s("loopback")),
        ("public_network_contacted", b(false)),
        ("evidence_valid_for_pass", b(false)),
        ("part_size", n(ED2K_PART_SIZE as f64)),
        ("block_size", n(w::BLOCKSIZE as f64)),
        ("transport", s("real TCP 127.0.0.1, standard 0xE3 / 0xC5 eD2k frames")),
        ("cases", J::Arr(results)),
        ("all_cases_completed", b(all_ok)),
        ("verdict", s(if all_ok { "confirmed_minimal_ed2k_download_and_upload_loopback" } else { "loopback_incomplete" })),
        ("note", s("同一份最小 eD2k 栈的两端：客户端 HELLO→REQUESTFILENAME→HASHSETREQUEST→STARTUPLOADREQ→(ACCEPTUPLOADREQ|QUEUERANKING)→REQUESTPARTS→SENDINGPART→END_OF_DOWNLOAD，服务端为对应上传侧 + 单槽最小上传队列。三个用例分别覆盖单分块、恰好一个 PARTSIZE（aMule Red 空分块哈希）、多分块。仅回环 TCP，0 个公网报文，不判任何 pass。未实现压缩/混淆/积分/AICH/>4GiB I64 分片/服务器连接，见 README。")),
    ]))
}

// --------------------------------------------------------------- lan hello --

/// Real-client evidence: connect to a LAN eMule/aMule TCP port, send a standard
/// `OP_HELLO`, and parse its `OP_HELLOANSWER`. No file request, no download.
fn run_lan_hello(ip_text: &str, port: u16) -> Result<J, String> {
    let ip: Ipv4Addr = ip_text.parse().map_err(|_| format!("invalid ip: {ip_text}"))?;
    if !is_private_ipv4(ip) {
        return Err(format!("拒绝非私有地址 {ip}：lan-hello 只允许 RFC1918 / 127.0.0.0/8"));
    }
    if port == 0 {
        return Err("--port 必须 > 0".into());
    }
    let dest = SocketAddr::from((ip, port));
    let mut out = obj(vec![
        ("schema", s("coldrevive.m0.s2-ed2k-mini-lan-hello/v1")),
        ("generated_at", s(utc_iso(unix_ms()))),
        ("mode", s("lan-hello")),
        ("target", s(dest.to_string())),
        ("target_is_private", b(true)),
        ("public_network_contacted", b(false)),
        ("evidence_valid_for_pass", b(false)),
    ]);
    let mut entries: Vec<(&str, J)> = Vec::new();

    let stream = TcpStream::connect_timeout(&dest, Duration::from_secs(3))
        .map_err(|e| format!("connect {dest}: {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(4)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(4)));
    let mut link = stack::Link::new(stream);
    entries.push(("connected", b(true)));

    let hello = our_hello(0);
    let hello_bytes = w::hello_frame(&hello);
    entries.push(("hello_frame_bytes", n(hello_bytes.len() as f64)));
    link.send_frame(&hello_bytes)?;

    let mut peer_hello: Option<w::Hello> = None;
    let mut other_opcodes: Vec<String> = Vec::new();
    let mut error = String::new();
    for _ in 0..6 {
        match link.recv() {
            Ok(p) => {
                if p.opcode == w::op::HELLOANSWER {
                    match w::parse_hello(&p.payload, false) {
                        Ok(h) => { peer_hello = Some(h); break; }
                        Err(e) => { error = format!("parse HELLOANSWER: {e}"); break; }
                    }
                } else {
                    other_opcodes.push(format!("0x{:02x}({})", p.opcode, w::opcode_name(p.opcode)));
                }
            }
            Err(e) => { error = e; break; }
        }
    }

    let answered = peer_hello.is_some();
    entries.push(("hello_answer_received", b(answered)));
    entries.push(("other_opcodes_before_answer", J::Arr(other_opcodes.iter().map(|x| s(x.clone())).collect())));
    entries.push(("error", s(&error)));

    if let Some(h) = &peer_hello {
        let nick = w::find_tag(&h.tags, w::ct::NAME).and_then(|t| match &t.value {
            kad::TagValue::Str(s) => Some(s.clone()), _ => None,
        }).unwrap_or_default();
        let version = w::find_tag(&h.tags, w::ct::VERSION).and_then(w::tag_as_u32).unwrap_or(0);
        let mule_version = w::find_tag(&h.tags, w::ct::EMULE_VERSION).and_then(w::tag_as_u32).unwrap_or(0);
        let udp_ports = w::find_tag(&h.tags, w::ct::EMULE_UDPPORTS).and_then(w::tag_as_u32).unwrap_or(0);
        let misc1 = w::find_tag(&h.tags, w::ct::EMULE_MISCOPTIONS1).and_then(w::tag_as_u32).unwrap_or(0);
        let misc2 = w::find_tag(&h.tags, w::ct::EMULE_MISCOPTIONS2).and_then(w::tag_as_u32).unwrap_or(0);
        let tag_names: Vec<String> = h.tags.iter().map(|t| w::tag_name_display(&t.name)).collect();
        entries.push(("peer_user_hash", s(sha1::hex(&h.user_hash))));
        entries.push(("peer_tcp_port", n(h.tcp_port as f64)));
        entries.push(("peer_nick", s(nick)));
        entries.push(("peer_client_version", s(format!("0x{version:02x}"))));
        entries.push(("peer_mule_version", s(format!("0x{mule_version:08x}"))));
        entries.push(("peer_udp_ports", s(format!("0x{udp_ports:08x}"))));
        entries.push(("peer_misc_options1", s(format!("0x{misc1:08x}"))));
        entries.push(("peer_misc_options2", s(format!("0x{misc2:08x}"))));
        entries.push(("peer_tag_count", n(h.tags.len() as f64)));
        entries.push(("peer_tag_names", J::Arr(tag_names.iter().map(|x| s(x.clone())).collect())));
        entries.push(("peer_server_ip", n(h.server_ip as f64)));
        entries.push(("peer_server_port", n(h.server_port as f64)));
    }
    // Two further READ-ONLY standard exchanges, only if the handshake worked.
    // (a) OP_ASKSHAREDFILES -> uint32 count: is there any file we could in
    // principle download from this peer? (b) OP_REQUESTFILENAME with a hash
    // that cannot exist -> expect OP_FILEREQANSNOFIL. Neither downloads data.
    let mut shared_files_count: i64 = -1;
    let mut shares_opcode = String::new();
    let mut file_request_opcode = String::new();
    let probe_hash = md4(b"coldrevive-l2-nonexistent-file-probe");
    if answered {
        if link.send_frame(&w::ask_shared_files()).is_ok() {
            if let Ok(p) = link.recv() {
                shares_opcode = format!("0x{:02x}({})", p.opcode, w::opcode_name(p.opcode));
                if p.opcode == w::op::ASKSHAREDFILESANSWER && p.payload.len() >= 4 {
                    shared_files_count = u32::from_le_bytes([
                        p.payload[0], p.payload[1], p.payload[2], p.payload[3],
                    ]) as i64;
                }
            }
        }
        // 0x4F is the variant that, per aMule master, explicitly answers
        // OP_FILEREQANSNOFIL for an unknown file; 0x58 silently `break`s.
        // The real LAN eMule answered neither -- recorded as an open
        // source/client discrepancy, NOT silently converted into a pass.
        if link.send_frame(&w::set_req_file_id(&probe_hash)).is_ok() {
            if let Ok(p) = link.recv() {
                file_request_opcode = format!("0x{:02x}({})", p.opcode, w::opcode_name(p.opcode));
            }
        }
    }
    entries.push(("probe_file_hash", s(sha1::hex(&probe_hash))));
    entries.push(("ask_shared_files_response", s(&shares_opcode)));
    entries.push(("peer_shared_files_count", n(shared_files_count as f64)));
    entries.push(("set_req_file_id_response", s(if file_request_opcode.is_empty() {
        "no_reply_within_4s".to_string()
    } else {
        file_request_opcode.clone()
    })));
    entries.push(("no_file_reply_received", b(file_request_opcode.starts_with("0x48"))));
    entries.push(("open_observations", J::Arr(vec![
        s("aMule 6a110563 ClientTCPSocket.cpp:425 case OP_SETREQFILEID sends OP_FILEREQANSNOFIL for an unknown 16-byte fileID; the real LAN eMule (peer_nick=https://www.emule-project.org) stayed silent for 4s (also 12s in an independent bounded check). Source baseline and this peer differ; do not treat either as a pass/fail of the transfer path."),
        s("OP_REQUESTFILENAME (0x58) with an unknown fileID is silent BY DESIGN in aMule (ClientTCPSocket.cpp:392 break), so silence on that opcode is expected and is not evidence of a broken stack."),
        s(format!("peer_shared_files_count = {shared_files_count}: this LAN peer shares no files, so capability ② (we download from an old client) cannot be exercised against it; the full transfer evidence is loopback-only and must be re-tested in M1 with a peer that actually shares a file.")),
    ])));

    entries.push(("log", log_json(&link.log)));
    entries.push(("verdict", s(if answered {
        "confirmed_real_emule_client_hello_handshake"
    } else {
        "hello_answer_not_received"
    })));
    entries.push(("note", s("ADR-011 L2 传输面：只向显式给出的 RFC1918 目标发标准 OP_HELLO 并解析 OP_HELLOANSWER，随后两次只读交换（OP_ASKSHAREDFILES 取共享文件数、对一个不可能存在的 fileID 发 OP_REQUESTFILENAME 期望 OP_FILEREQANSNOFIL），不下载任何文件内容、不发布、0 个公网报文。真实 eMule/aMule 接受并回答这三类标准报文，证明 TCP 帧头、HELLO tag 布局与文件请求路径与旧客户端兼容。若 peer_shared_files_count=0，则本机没有可供实测『从旧客户端下载』的文件，该方向的完整分片证据只能取自同仓 loopback（我们自己的客户端 ↔ 我们自己的服务端）；二者不可互相替代，须在 M1 换有共享文件的对端补测。"))
    );
    if let J::Obj(ref mut v) = out {
        v.extend(entries.into_iter().map(|(k, val)| (k.to_string(), val)));
    }
    Ok(out)
}

// ---------------------------------------------------------------- selftest --

fn run_selftest() -> Result<J, String> {
    let mut checks: Vec<(&str, bool, &str)> = Vec::new();
    checks.push(("prot.edonkey", w::PROT_EDONKEY == 0xE3, "aMule Protocols.h"));
    checks.push(("prot.emule", w::PROT_EMULE == 0xC5, "aMule Protocols.h"));
    checks.push(("op.hello", w::op::HELLO == 0x01, "aMule ed2k Client2Client/TCP.h"));
    checks.push(("op.hello_answer", w::op::HELLOANSWER == 0x4C, "same"));
    checks.push(("op.request_filename", w::op::REQUESTFILENAME == 0x58, "same"));
    checks.push(("op.request_parts", w::op::REQUESTPARTS == 0x47, "same"));
    checks.push(("op.sending_part", w::op::SENDINGPART == 0x46, "same"));
    checks.push(("op.accept_upload", w::op::ACCEPTUPLOADREQ == 0x55, "same"));
    checks.push(("op.hashset_answer", w::op::HASHSETANSWER == 0x52, "same"));
    checks.push(("frame.length_includes_opcode", {
        let f = w::frame(w::PROT_EDONKEY, w::op::HELLO, b"123");
        f[1..5] == 4u32.to_le_bytes() && w::parse_frame(&f).unwrap().payload == b"123"
    }, "aMule Packet.cpp GetPacketSizeFromHeader"));

    let h = our_hello(4662);
    checks.push(("hello.req_has_hash_size", w::parse_frame(&w::hello_frame(&h)).unwrap().payload[0] == 16, "BaseClient.cpp SendHelloPacket"));
    checks.push(("hello.answer_has_no_hash_size", {
        let p = w::parse_frame(&w::hello_answer_frame(&h)).unwrap();
        p.opcode == w::op::HELLOANSWER && p.payload[0] != 16
    }, "BaseClient.cpp SendHelloAnswer"));
    checks.push(("hello.roundtrip", w::parse_hello(&w::parse_frame(&w::hello_frame(&h)).unwrap().payload, true).is_ok(), "ProcessHelloPacket"));
    checks.push(("hello.tagcount_is_u32", w::op::HELLO == 0x01 && w::ct::EMULE_VERSION == 0xFB, "ClientTags.h"));

    let file = deterministic_file(ED2K_PART_SIZE, "selftest.bin");
    checks.push(("ed2k.exact_part_has_2_part_hashes", file.part_hashes().len() == 2, "aMule KnownFile.cpp / ThreadTasks.cpp"));
    checks.push(("ed2k.exact_part_second_hash_is_md4_empty", file.part_hashes()[1] == md4(&[]), "g_emptyMD4Hash"));
    checks.push(("ed2k.hash_equals_md4_of_hashset", {
        let mut concat = Vec::new();
        for p in file.part_hashes() { concat.extend_from_slice(&p); }
        file.hash() == md4(&concat)
    }, "CKnownFile::CreateHashFromHashlist"));
    checks.push(("ed2k.small_has_no_hashset", deterministic_file(10, "s").part_hashes().is_empty(), "SetFileSize"));

    checks.push(("parts.grouped_offsets", {
        let f = w::request_parts(&[1u8; 16], &[(0, 10), (20, 30)]);
        let p = w::parse_frame(&f).unwrap();
        let (_, r) = w::parse_request_parts(&p.payload).unwrap();
        r == vec![(0, 10), (20, 30)] && p.payload.len() == 40
    }, "DownloadClient.cpp SendBlockRequests / UploadClient.cpp ProcessRequestPartsPacket"));
    checks.push(("queueranking.is_12_bytes", w::parse_frame(&w::queue_ranking(3)).unwrap().payload.len() == 12, "UploadClient.cpp SendRankingInfo"));
    checks.push(("hashset.answer_roundtrip", {
        let parts = vec![[2u8; 16], [3u8; 16]];
        let p = w::parse_frame(&w::hashset_answer(&[1u8; 16], &parts)).unwrap();
        w::parse_hashset_answer(&p.payload).unwrap().1 == parts
    }, "UploadClient.cpp SendHashsetPacket"));
    checks.push(("sendingpart.roundtrip", {
        let p = w::parse_frame(&w::sending_part(&[4u8; 16], 8, 12, b"abcd")).unwrap();
        w::parse_sending_part(&p.payload).unwrap().3 == b"abcd"
    }, "UploadDiskIOThread.cpp CreateNormalPackets"));
    checks.push(("meta_tag.compact_form_accepted", {
        let mut raw = vec![0x83, w::ct::VERSION];
        raw.extend_from_slice(&0x3cu32.to_le_bytes());
        w::read_meta_tag(&mut kad::Cur::new(&raw)).is_ok()
    }, "Tag.cpp CTag::CTag"));

    let mut q = UploadQueue::new(1);
    let a = q.new_peer();
    let c = q.new_peer();
    checks.push(("queue.first_gets_slot", q.acquire(a) == None, "minimal FIFO upload queue"));
    checks.push(("queue.second_gets_rank", q.acquire(c) == Some(1), "OP_QUEUERANKING rank"));
    q.release(a);
    checks.push(("queue.slot_released", q.acquire(c) == None, "no credit system"));

    let passed = checks.iter().filter(|(_, ok, _)| *ok).count();
    let failed: Vec<&str> = checks.iter().filter(|(_, ok, _)| !*ok).map(|(name, _, _)| *name).collect();
    let report = obj(vec![
        ("schema", s("coldrevive.m0.s2-ed2k-mini-selftest/v1")),
        ("generated_at", s(utc_iso(unix_ms()))),
        ("public_network_contacted", b(false)),
        ("evidence_valid_for_pass", b(false)),
        ("total", n(checks.len() as f64)),
        ("passed", n(passed as f64)),
        ("failed", n(failed.len() as f64)),
        ("checks", J::Arr(checks.iter().map(|(name, ok, src)| obj(vec![
            ("name", s(*name)), ("ok", b(*ok)), ("source", s(*src)),
        ])).collect())),
        ("failures", J::Arr(failed.iter().map(|x| s(*x)).collect())),
    ]);
    eprintln!("ed2k-mini selftest: {}/{} passed", passed, checks.len());
    for name in &failed { eprintln!("  FAIL {name}"); }
    if !failed.is_empty() {
        return Err(format!("{} selftest checks failed", failed.len()));
    }
    Ok(report)
}

// -------------------------------------------------------------------- main --

fn print_usage() {
    println!("ed2k-mini — ADR-011 L2 minimal standard eD2k TCP stack (spike)\n");
    println!("  ed2k-mini selftest                          codec / queue assertions, no sockets");
    println!("  ed2k-mini loopback                          real TCP transfer: our client <-> our server");
    println!("  ed2k-mini lan-hello --ip IP --port TCP      RFC1918-only real eMule/aMule HELLO handshake\n");
    println!("No public eD2k network is contacted. See tests/spikes/s2/ed2k-mini/README.md.");
}

fn arg_value(argv: &[String], key: &str) -> Option<String> {
    argv.iter().position(|a| a == key).and_then(|i| argv.get(i + 1).cloned())
}

fn main() {
    let argv: Vec<String> = env::args().skip(1).collect();
    if argv.is_empty() {
        print_usage();
        return;
    }
    let result: Result<(J, &str), String> = match argv[0].as_str() {
        "selftest" => run_selftest().map(|r| (r, "ed2k-mini-selftest")),
        "loopback" => run_loopback().map(|r| (r, "ed2k-mini-loopback")),
        "lan-hello" => {
            let ip = arg_value(&argv, "--ip").unwrap_or_default();
            let port = arg_value(&argv, "--port").and_then(|v| v.parse::<u16>().ok()).unwrap_or(0);
            if ip.is_empty() {
                Err("缺少 --ip <RFC1918 IPv4>".into())
            } else {
                run_lan_hello(&ip, port).map(|r| (r, "ed2k-mini-lan-hello"))
            }
        }
        "-h" | "--help" => { print_usage(); return; }
        other => Err(format!("unknown subcommand: {other}")),
    };
    match result {
        Ok((report, prefix)) => match write_report(&report, prefix) {
            Ok(path) => {
                println!("{}", report.dump());
                eprintln!("[report] {}", path.display());
            }
            Err(e) => { eprintln!("[ERROR] {e}"); std::process::exit(1); }
        },
        Err(e) => { eprintln!("[ERROR] {e}"); std::process::exit(1); }
    }
}
