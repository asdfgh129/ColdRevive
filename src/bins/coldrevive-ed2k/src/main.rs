//! coldrevive-ed2k — FR-24/FR-25 命令行工具（默认子命令 = `search`）。
//!
//! - `search`（FR-24，批准后的公共轨）：真实 UDP KAD `0x33` 关键词检索 +
//!   解析 `0x3B` tag 生成标准 `ed2k://` 链接。key 用 A 轨无前缀 `MD4(UTF-8(term))`，
//!   与 cr-kad 私有 B 轨不互换。
//! - `link`：由文件内容导出真实 `ed2k_hash` 与链接（不许从别处抄 hash）。
//! - `download` / `serve`（FR-25）：生产 `cr-ed2k::stack` 的下载/上传方向，
//!   用来对真实 eMule/aMule 客户端互通。
//!
//! 边界：不实现 credit/AICH/source exchange/eD2k server；必须显式给 `--bootstrap`，
//! 避免任何后台进程悄悄向公共 KAD 发包；非 RFC1918 目标还要求 `--public`。

use cr_core::Json;
use cr_ed2k::kad::{Tag as KadTag, TagValue as KadTagValue};
use cr_ed2k::stack::{download, serve_with_config, ServeConfig, SharedFile, UploadQueue};
use cr_ed2k::wire::{ct, Hello};
use cr_hash::{hex, md4};
use cr_kad::kad_ed2k::{
    ed2k_keyword_key, find_node_req, hello_req, hello_res_ack, in_tolerance_zone,
    parse_bootstrap_res, parse_find_node_res, parse_hello_res, parse_publish_res,
    publish_key_req, search_key_req, top_chunk, unpack_frame, xor_distance, Contact,
    KAD_MISCOPT_REQUEST_ACK, OP_BOOTSTRAP_REQ, OP_BOOTSTRAP_RES, OP_HELLO_RES,
    OP_PUBLISH_RES, OP_RES, OP_SEARCH_RES,
};
use cr_kad::kad_obfuscate;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::path::Path;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TAG_HASH16: u8 = 0x01;
const TAG_STRING: u8 = 0x02;
const TAG_U32: u8 = 0x03;
const TAG_U16: u8 = 0x08;
const TAG_U8: u8 = 0x09;
const TAG_BSOB: u8 = 0x0a;
const TAG_U64: u8 = 0x0b;
const TAG_FILENAME: u8 = 0x01;
const TAG_FILESIZE: u8 = 0x02;

/// 发包节流器：公共 KAD 实验沿用 G0 确认的 0.2 pps 口径。
/// pps<=0 表示不限速（仅本地/回环）；每次 send_to 前调用 wait()。
#[derive(Debug, Clone)]
struct Pacer {
    interval: Duration,
    last: Option<Instant>,
}

impl Pacer {
    fn new(pps: f64) -> Self {
        let interval = if pps > 0.0 {
            Duration::from_secs_f64(1.0 / pps)
        } else {
            Duration::ZERO
        };
        Self { interval, last: None }
    }

    fn wait(&mut self) {
        if self.interval.is_zero() { return; }
        if let Some(last) = self.last {
            let wait = self.interval.saturating_sub(last.elapsed());
            if !wait.is_zero() { std::thread::sleep(wait); }
        }
        self.last = Some(Instant::now());
    }
}

/// 一条 KAD 报文的混淆策略。try-0 用对方 KadID 派生 RC4 key；try-2 用对方
/// verify key（需要 `--peer-udp-key` + `--our-public-ip` 才能从 aMule 公式算出）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KadEnvelope {
    Plain,
    Try0 { id: [u8; 16], receiver_key: u32 },
    Try2(u32),
}

fn kad_wire(frame: &[u8], envelope: KadEnvelope) -> Vec<u8> {
    match envelope {
        KadEnvelope::Plain => frame.to_vec(),
        KadEnvelope::Try0 { id, receiver_key } =>
            kad_obfuscate::obfuscate_with_keys(&id, receiver_key, kad_obfuscate::OUR_UDP_KEY, frame),
        KadEnvelope::Try2(key) =>
            kad_obfuscate::obfuscate_verify(key, kad_obfuscate::OUR_UDP_KEY, frame),
    }
}

#[derive(Debug, Clone)]
struct Tag {
    name: Vec<u8>,
    kind: u8,
    value: String,
    num: Option<u64>,
}

#[derive(Debug, Clone)]
struct Answer {
    file_id: [u8; 16],
    tags: Vec<Tag>,
}

struct Cur<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Cur<'a> {
    fn new(b: &'a [u8]) -> Self { Self { b, i: 0 } }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.i + n > self.b.len() {
            return Err(format!("KAD tag 截断: 需要 {n} B，剩 {}", self.b.len() - self.i));
        }
        let out = &self.b[self.i..self.i + n];
        self.i += n;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8, String> { Ok(self.take(1)?[0]) }
    fn u16(&mut self) -> Result<u16, String> {
        let p = self.take(2)?;
        Ok(u16::from_le_bytes([p[0], p[1]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let p = self.take(4)?;
        Ok(u32::from_le_bytes([p[0], p[1], p[2], p[3]]))
    }
    fn u64(&mut self) -> Result<u64, String> {
        let p = self.take(8)?;
        Ok(u64::from_le_bytes(p.try_into().map_err(|_| "u64 长度错误".to_string())?))
    }
    fn id(&mut self) -> Result<[u8; 16], String> {
        let p = self.take(16)?;
        let mut out = [0u8; 16];
        out.copy_from_slice(p);
        Ok(out)
    }
}

fn name_display(name: &[u8]) -> String {
    if name.len() == 1 && name[0].is_ascii_graphic() {
        format!("num:0x{:02x}", name[0])
    } else {
        String::from_utf8_lossy(name).into_owned()
    }
}

fn tag_value(c: &mut Cur<'_>, kind: u8) -> Result<(String, Option<u64>), String> {
    match kind {
        TAG_HASH16 => Ok((hex(&c.take(16)?), None)),
        TAG_STRING => {
            let n = c.u16()? as usize;
            Ok((String::from_utf8_lossy(c.take(n)?).into_owned(), None))
        }
        TAG_U8 => { let v = c.u8()?; Ok((v.to_string(), Some(v as u64))) }
        TAG_U16 => { let v = c.u16()?; Ok((v.to_string(), Some(v as u64))) }
        TAG_U32 => { let v = c.u32()?; Ok((v.to_string(), Some(v as u64))) }
        TAG_U64 => { let v = c.u64()?; Ok((v.to_string(), Some(v))) }
        TAG_BSOB => { let n = c.u32()? as usize; Ok((hex(c.take(n)?), None)) }
        other => Err(format!("不支持的 KAD tag 类型 0x{other:02x}")),
    }
}

fn read_tags(c: &mut Cur<'_>) -> Result<Vec<Tag>, String> {
    let n = c.u8()? as usize;
    if n > 64 { return Err(format!("tag 数异常: {n}")); }
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let kind = c.u8()?;
        // aMule 的 Kad ReadTag 不接受 STR1..STR22 / BLOB；收到就说明包不是 KAD2 格式。
        if (0x11..=0x26).contains(&kind) || kind == 0x07 {
            return Err(format!("KAD 路径非法 tag 类型 0x{kind:02x}"));
        }
        let name_len = c.u16()? as usize;
        if name_len > 256 { return Err(format!("tag 名过长: {name_len}")); }
        let name = c.take(name_len)?.to_vec();
        let (value, num) = tag_value(c, kind)?;
        out.push(Tag { name, kind, value, num });
    }
    Ok(out)
}

/// 解析 `KADEMLIA2_SEARCH_RES` 的 payload（不含 2 字节帧头）。
fn parse_search_res(payload: &[u8]) -> Result<([u8; 16], [u8; 16], Vec<Answer>), String> {
    let mut c = Cur::new(payload);
    let sender = c.id()?;
    let target = c.id()?;
    let n = c.u16()? as usize;
    if n > 256 { return Err(format!("answer 数异常: {n}")); }
    let mut answers = Vec::with_capacity(n);
    for _ in 0..n {
        let file_id = c.id()?;
        let tags = read_tags(&mut c)?;
        answers.push(Answer { file_id, tags });
    }
    Ok((sender, target, answers))
}

fn tag_is(tag: &Tag, numeric: u8) -> bool {
    tag.name == [numeric]
}

fn ed2k_link(a: &Answer) -> Option<String> {
    let id = hex(&a.file_id);
    let name = a.tags.iter()
        .find(|t| tag_is(t, TAG_FILENAME) && t.kind == TAG_STRING)
        .map(|t| t.value.replace(['|', '/', '\n', '\r'], "_"))?;
    let size = a.tags.iter()
        .find(|t| tag_is(t, TAG_FILESIZE) && t.num.is_some())
        .and_then(|t| t.num)?;
    if name.is_empty() || size == 0 { return None; }
    Some(format!("ed2k://|file|{name}|{size}|{id}|/"))
}

fn answer_json(a: &Answer) -> Json {
    let tags = a.tags.iter().map(|t| Json::obj(vec![
        ("name", Json::s(&name_display(&t.name))),
        ("type", Json::n(t.kind as f64)),
        ("value", Json::s(&t.value)),
    ])).collect();
    Json::obj(vec![
        ("file_id", Json::s(&hex(&a.file_id))),
        ("ed2k_link", match ed2k_link(a) {
            Some(link) => Json::s(&link),
            None => Json::s(""),
        }),
        ("tags", Json::arr(tags)),
    ])
}

fn is_public(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    !(o[0] == 10 || (o[0] == 172 && (16..=31).contains(&o[1]))
      || (o[0] == 192 && o[1] == 168) || o[0] == 127 || (o[0] == 169 && o[1] == 254))
}

/// 收一个指定 opcode 的 KAD 应答。aMule 对 >200 B 的帧发 `0xE5` zlib 包，
/// 只认 `0xE4` 会把「对端已应答」误记成超时（S2 早期归因错误）。
///
/// `envelope` 决定明文 / try-0（对方 KadID）/ try-2（对方 verify key）。
/// 真机抓包证实开了 `CryptoKadUDPKey` 的节点对明文索引操作静默丢弃。
/// 收包同时尝试明文解包与混淆解包；try-0 的 base 是本工具报给对方的 KadID，
/// try-2 的 base 是我们声明的 senderVerifyKey（两者都按 aMule 字节序）。
/// BOOTSTRAP 与 0x21 问路保持明文（实测明文可应答）。
fn kad_exchange(sock: &UdpSocket, dest: SocketAddr, frame: &[u8], expected: u8,
                timeout: Duration, pacer: &mut Pacer,
                envelope: KadEnvelope) -> Result<(Vec<u8>, u64, bool, Option<u32>), String> {
    pacer.wait();
    let started = Instant::now();
    let wire = kad_wire(frame, envelope);
    sock.send_to(&wire, dest).map_err(|e| format!("send_to {dest}: {e}"))?;
    let mut buf = [0u8; 65535];
    loop {
        let left = timeout.checked_sub(started.elapsed()).unwrap_or(Duration::ZERO);
        sock.set_read_timeout(Some(left)).map_err(|e| e.to_string())?;
        let (n, src) = match sock.recv_from(&mut buf) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                   || e.kind() == std::io::ErrorKind::TimedOut => {
                return Err(format!("超时未收到 opcode 0x{expected:02x} 应答"));
            }
            Err(e) => return Err(format!("recv_from: {e}")),
        };
        if src != dest { continue; }
        // 先试明文 0xE4/0xE5；再试 RC4 混淆信封。try-0 的 base 是我们报给对方的
        // KadID，try-2 的 base 是我们声明的 senderVerifyKey，两者都必须试。
        let mut peer_svk = None;
        let decoded = match unpack_frame(&buf[..n]) {
            Ok(v) => Some(v),
            Err(_) => kad_obfuscate::deobfuscate(
                &buf[..n], kad_obfuscate::OUR_UDP_KEY, &kad_obfuscate::our_node_id()
            ).and_then(|(inner, svk)| {
                peer_svk = Some(svk);
                unpack_frame(&inner).ok()
            }),
        };
        match decoded {
            Some((op, payload)) if op == expected => {
                let obfuscated = buf[0] != 0xe4 && buf[0] != 0xe5;
                return Ok((payload, started.elapsed().as_millis() as u64, obfuscated, peer_svk));
            }
            _ => continue,
        }
    }
}

/// 只发包不等应答（HELLO_RES_ACK 没有回包）。
fn kad_send(sock: &UdpSocket, dest: SocketAddr, frame: &[u8], pacer: &mut Pacer,
            envelope: KadEnvelope) -> Result<(), String> {
    pacer.wait();
    let wire = kad_wire(frame, envelope);
    sock.send_to(&wire, dest).map_err(|e| format!("send_to {dest}: {e}"))?;
    Ok(())
}

/// aMule 要求先有 BOOTSTRAP_REQ/RES 才把发送方当作 KAD 节点处理；
/// 直接发 0x33 会被静默忽略（S2 早期探针踩过，表现为「超时=查无结果」）。
/// 同时取回该节点自报 KadID 与联系人——前者是后续 0x21 的 `receiver`。
fn bootstrap(sock: &UdpSocket, dest: SocketAddr, timeout: Duration, pacer: &mut Pacer)
    -> Result<(u64, [u8; 16], Vec<Contact>), String>
{
    let frame = [0xe4, OP_BOOTSTRAP_REQ];
    let (payload, ms, _, _) = kad_exchange(sock, dest, &frame, OP_BOOTSTRAP_RES, timeout,
                                           pacer, KadEnvelope::Plain)?;
    let (self_id, contacts) = parse_bootstrap_res(&payload)?;
    Ok((ms, self_id, contacts))
}

/// HELLO + receiver-key 握手。`peer_key`/`our_ip` 都给出时，按 aMule
/// `CPrefs::GetUDPVerifyKey` 派生 receiverVerifyKey；默认 try-0 发 HELLO_REQ，
/// `--hello-try2` 则直接用 receiverVerifyKey 作 RC4 base。真机对照表明
/// aMule 只接受 try-2（2026-11-23）。若 HELLO_RES 要求 ACK，则用回包里的
/// senderVerifyKey 回 HELLO_RES_ACK。拿不到对方 KadUDPKey 时不谎称验证通过。
fn kad_hello(sock: &UdpSocket, dest: SocketAddr, target_id: &[u8; 16],
             peer_key: Option<u32>, our_ip: Option<[u8; 4]>, udp_port: u16,
             use_try2: bool, use_plain: bool, timeout: Duration, pacer: &mut Pacer) -> Result<Json, String> {
    let receiver_key = match (peer_key, our_ip) {
        (Some(k), Some(ip)) => kad_obfuscate::udp_verify_key(k, ip),
        _ => 0,
    };
    let our_id = kad_obfuscate::our_node_id();
    let req = hello_req(&our_id, 0, udp_port, KAD_MISCOPT_REQUEST_ACK);
    let envelope = if use_plain {
        KadEnvelope::Plain
    } else if use_try2 && receiver_key != 0 {
        KadEnvelope::Try2(receiver_key)
    } else {
        KadEnvelope::Try0 { id: *target_id, receiver_key }
    };
    let (payload, ms, obfuscated, peer_svk) = kad_exchange(
        sock, dest, &req, OP_HELLO_RES, timeout, pacer, envelope,
    )?;
    let hello = parse_hello_res(&payload)?;
    let ack_sent = if hello.misc_options & KAD_MISCOPT_REQUEST_ACK != 0 {
        let ack_envelope = match peer_svk {
            Some(k) if k != 0 => KadEnvelope::Try2(k),
            _ => KadEnvelope::Plain,
        };
        kad_send(sock, dest, &hello_res_ack(&our_id), pacer, ack_envelope)?;
        true
    } else {
        false
    };
    Ok(Json::obj(vec![
        ("hello_envelope", Json::s(if use_plain { "plain" } else if use_try2 && receiver_key != 0 { "try2" } else { "try0" })),
        ("receiver_verify_key", Json::n(receiver_key as f64)),
        ("rtt_ms", Json::n(ms as f64)),
        ("obfuscated_response", Json::b(obfuscated)),
        ("peer_id", Json::s(&hex(&hello.id))),
        ("peer_version", Json::n(hello.version as f64)),
        ("peer_source_uport", Json::n(hello.source_uport.unwrap_or(0) as f64)),
        ("peer_requested_ack", Json::b(hello.misc_options & KAD_MISCOPT_REQUEST_ACK != 0)),
        ("hello_res_ack_sent", Json::b(ack_sent)),
        // 只有 try-2 信封真的换回 HELLO_RES，才算接收方 verify key 被对端接受；
        // 仅提供 CLI 参数或 try-0 成功不能证明这一点。
        ("receiver_key_verified", Json::b(receiver_key != 0 && use_try2)),
    ]))
}

fn exchange(sock: &UdpSocket, dest: SocketAddr, frame: &[u8], timeout: Duration,
            pacer: &mut Pacer, envelope: KadEnvelope)
    -> Result<([u8; 16], [u8; 16], Vec<Answer>, u64, bool), String>
{
    let (payload, ms, obfuscated, _) = kad_exchange(
        sock, dest, frame, OP_SEARCH_RES, timeout, pacer, envelope,
    )?;
    let (sender, target, answers) = parse_search_res(&payload)?;
    Ok((sender, target, answers, ms, obfuscated))
}

#[derive(Debug)]
struct LookupResult {
    hops: usize,
    sent: usize,
    answered: usize,
    verified: Vec<Contact>,
    closest_top_chunk: u64,
    in_zone: usize,
}

/// 检索前的责任区收敛。S2 PLAN §6/§20 已证：`0x33` 发给不在
/// `SEARCHTOLERANCE = 2^24` 责任区内的节点会被 aMule 静默丢弃——这正是
/// 「BOOTSTRAP_RES 成功但 SEARCH_RES 0 命中」的根因。这里用 0x21/0x29
/// 迭代找节点，只对回显 target 的「已验证」节点发检索。
fn lookup_nodes(sock: &UdpSocket, seeds: Vec<Contact>, key: &[u8; 16], timeout: Duration,
                pacer: &mut Pacer, max_hops: usize, alpha: usize, allow_lan: bool) -> LookupResult {
    let mut known: Vec<Contact> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for c in seeds {
        if !allow_lan && !c.is_public() { continue; }
        let k = c.addr().to_string();
        if seen.insert(k) { known.push(c); }
    }
    let mut asked: HashSet<String> = HashSet::new();
    let mut verified: Vec<Contact> = Vec::new();
    let (mut hops, mut sent, mut answered) = (0usize, 0usize, 0usize);
    for _ in 0..max_hops {
        let mut candidates: Vec<Contact> = known.iter().copied()
            .filter(|c| !asked.contains(&c.addr().to_string()))
            .collect();
        if candidates.is_empty() { break; }
        candidates.sort_by_key(|c| xor_distance(&c.id, key));
        let take = candidates.len().min(alpha.max(1));
        hops += 1;
        for c in candidates.into_iter().take(take) {
            asked.insert(c.addr().to_string());
            sent += 1;
            let req = find_node_req(key, &c.id, 5);
            // 0x21 问路保持明文：真机实测明文可收 0x29（收敛率 55/60），
            // 只有索引操作 0x33/0x43 需要混淆。
            match kad_exchange(sock, c.addr().into(), &req, OP_RES, timeout, pacer,
                               KadEnvelope::Plain) {
                Ok((payload, _, _, _)) => {
                    answered += 1;
                    if let Ok((target, found)) = parse_find_node_res(&payload) {
                        // aMule 回显 target 才说明它认出了 receiver=自己的 KadID。
                        if target != *key { continue; }
                        verified.push(c);
                        for n in found {
                            if !allow_lan && !n.is_public() { continue; }
                            let k = n.addr().to_string();
                            if seen.insert(k) { known.push(n); }
                        }
                    }
                }
                Err(_) => {}
            }
        }
        if verified.iter().filter(|c| in_tolerance_zone(&c.id, key)).count() >= 2 { break; }
    }
    let closest = known.iter().map(|c| top_chunk(&c.id, key)).min().unwrap_or(u32::MAX) as u64;
    let in_zone = verified.iter().filter(|c| in_tolerance_zone(&c.id, key)).count();
    LookupResult { hops, sent, answered, verified, closest_top_chunk: closest, in_zone }
}

/// 由**文件内容**导出标准 `ed2k://` 链接（MD4 over 9.28 MB 分块，Red 变体规则在
/// `kad::ed2k_hash` 里）。供上传方向测试取真实 fileID 用；不接受从别处抄来的 hash。
fn run_link(args: &[String]) -> Result<Json, String> {
    let opt = |key: &str| -> Option<String> {
        args.iter().position(|a| a == key).and_then(|i| args.get(i + 1)).cloned()
    };
    let path = opt("--file").ok_or("用法: coldrevive-ed2k link --file <文件> [--name <显示名>]")?;
    let data = std::fs::read(&path).map_err(|e| format!("读取 {path}: {e}"))?;
    let name = opt("--name").unwrap_or_else(|| {
        std::path::Path::new(&path).file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "sample.bin".into())
    });
    let size = data.len();
    let shared = SharedFile::new(name.clone(), data);
    Ok(Json::obj(vec![
        ("schema", Json::s("coldrevive.fr25.link/v1")),
        ("file", Json::s(&path)),
        ("file_name", Json::s(&name)),
        ("file_size", Json::n(size as f64)),
        ("file_id", Json::s(&hex(&shared.hash()))),
        ("ed2k_link", Json::s(&shared.link())),
    ]))
}

/// FR-25 上传方向：用生产 `cr-ed2k::stack::serve` 给真实 eMule/aMule 客户端供片。
/// 只服务一个连接，收到请求后把真实观测写 JSON，不伪造成功。
fn run_serve(args: &[String]) -> Result<Json, String> {
    let opt = |key: &str| -> Option<String> {
        args.iter().position(|a| a == key).and_then(|i| args.get(i + 1)).cloned()
    };
    let path = opt("--file").ok_or("用法: coldrevive-ed2k serve --file <共享文件> --port 4663")?;
    let port: u16 = opt("--port").and_then(|s| s.parse().ok()).unwrap_or(4663);
    let timeout_ms: u64 = opt("--timeout-ms").and_then(|s| s.parse().ok()).unwrap_or(120_000);
    let mut serve_config = ServeConfig::default();
    if let Some(n) = opt("--max-in-flight-bytes").and_then(|s| s.parse::<usize>().ok()) {
        serve_config.max_in_flight_bytes = Some(n);
    }
    if args.iter().any(|a| a == "--in-flight-unlimited") {
        serve_config.max_in_flight_bytes = None;
    }
    // 未对真 aMule 验证的假说：每请求回合只送一块、不主动 idle 重传。
    serve_config.request_gated_pacing = args.iter().any(|a| a == "--request-gated-pacing");
    let data = std::fs::read(&path).map_err(|e| format!("读取共享文件 {path}: {e}"))?;
    let name = opt("--name").unwrap_or_else(|| {
        std::path::Path::new(&path).file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "sample.bin".into())
    });
    let shared = SharedFile::new(name, data);
    let our_hello = Hello {
        user_hash: md4(b"coldrevive-fr25-live-probe"),
        client_id: 0,
        tcp_port: port,
        tags: vec![
            KadTag { name: vec![ct::NAME], value: KadTagValue::Str("coldrevive-fr25".into()) },
            KadTag { name: vec![ct::VERSION], value: KadTagValue::U32(0x3c) },
        ],
        server_ip: 0,
        server_port: 0,
    };
    let listener = std::net::TcpListener::bind(("0.0.0.0", port))
        .map_err(|e| format!("监听 {port}: {e}"))?;
    // 观测行写 stderr：stdout 只留最终 JSON，报告文件才能被机器直接解析。
    eprintln!("[serve] 等待真实客户端连接 0.0.0.0:{port} file={}", shared.link());
    let (stream, peer) = listener.accept().map_err(|e| format!("accept: {e}"))?;
    let queue = Arc::new(Mutex::new(UploadQueue::new(2)));
    let request_gated_pacing = serve_config.request_gated_pacing;
    let out = serve_with_config(stream, &[shared], &our_hello, &queue, Duration::from_millis(timeout_ms), serve_config);
    Ok(Json::obj(vec![
        ("schema", Json::s("coldrevive.fr25.live-upload/v1")),
        ("peer", Json::s(&peer.to_string())),
        ("peer_hello", Json::b(out.peer_hello.is_some())),
        ("queue_granted", Json::b(out.queue_granted)),
        ("queue_rank_sent", match out.queue_rank_sent { Some(r) => Json::n(r as f64), None => Json::s("") }),
        ("blocks_sent", Json::n(out.blocks_sent as f64)),
        ("bytes_sent", Json::n(out.bytes_sent as f64)),
        ("idle_resends", Json::n(out.idle_resends as f64)),
        ("max_in_flight_bytes", Json::n(out.max_in_flight_bytes as f64)),
        ("deferred_bytes", Json::n(out.deferred_bytes as f64)),
        ("request_gated_pacing", Json::b(request_gated_pacing)),
        ("error", match out.error { Some(e) => Json::s(&e), None => Json::s("") }),
        ("log_tail", Json::arr(out.log.iter().rev().take(12).map(|e| Json::s(&format!("{:?}", e))).collect())),
    ]))
}

/// FR-25：用生产 `cr-ed2k::stack::download` 对真实 eMule/aMule 上传端取片。
/// `expected` 的 fileID 必须由本地参考文件的真实 `ed2k_hash` 导出，不能从链接抄。
fn run_download(args: &[String]) -> Result<Json, String> {
    let opt = |key: &str| -> Option<String> {
        args.iter().position(|a| a == key).and_then(|i| args.get(i + 1)).cloned()
    };
    let path = opt("--file").ok_or("用法: coldrevive-ed2k download --file <参考文件> --host ip:port")?;
    let host = opt("--host").ok_or("download 缺 --host ip:port")?;
    let name = opt("--name").unwrap_or_else(|| {
        std::path::Path::new(&path).file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "sample.bin".into())
    });
    let timeout_ms: u64 = opt("--timeout-ms").and_then(|s| s.parse().ok()).unwrap_or(60_000);
    let data = std::fs::read(&path).map_err(|e| format!("读取参考文件 {path}: {e}"))?;
    let expected = SharedFile::new(name, data);
    let our_hello = Hello {
        user_hash: md4(b"coldrevive-fr25-live-probe"),
        client_id: 0,
        tcp_port: 4662,
        tags: vec![
            KadTag { name: vec![ct::NAME], value: KadTagValue::Str("coldrevive-fr25".into()) },
            KadTag { name: vec![ct::VERSION], value: KadTagValue::U32(0x3c) },
        ],
        server_ip: 0,
        server_port: 0,
    };
    let addr: SocketAddr = host.parse().map_err(|_| format!("非法 --host {host}"))?;
    let stream = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(8))
        .map_err(|e| format!("连接 {addr}: {e}"))?;
    let out = download(stream, &expected, &our_hello, Duration::from_millis(timeout_ms));
    Ok(Json::obj(vec![
        ("schema", Json::s("coldrevive.fr25.live-download/v1")),
        ("host", Json::s(&addr.to_string())),
        ("file_name", Json::s(&expected.name)),
        ("file_size", Json::n(expected.data.len() as f64)),
        ("file_id", Json::s(&hex(&expected.hash()))),
        ("ed2k_link", Json::s(&expected.link())),
        ("peer_hello", Json::b(out.peer_hello.is_some())),
        ("peer_filename", match out.peer_filename { Some(s) => Json::s(&s), None => Json::s("") }),
        ("hashset_verified", Json::b(out.hashset_verified)),
        ("queue_rank_seen", match out.queue_rank_seen { Some(r) => Json::n(r as f64), None => Json::s("") }),
        ("bytes_received", Json::n(out.bytes_received as f64)),
        ("blocks_received", Json::n(out.blocks_received as f64)),
        ("hash_matches", Json::b(out.hash_matches)),
        ("content_matches", Json::b(out.content_matches)),
        ("completed", Json::b(out.completed)),
        ("error", match out.error { Some(e) => Json::s(&e), None => Json::s("") }),
        ("log_tail", Json::arr(out.log.iter().rev().take(12).map(|e| Json::s(&format!("{:?}", e))).collect())),
    ]))
}

fn run() -> Result<Json, String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(|s| s.as_str()) == Some("download") {
        return run_download(&args[1..]);
    }
    if args.first().map(|s| s.as_str()) == Some("serve") {
        return run_serve(&args[1..]);
    }
    if args.first().map(|s| s.as_str()) == Some("link") {
        return run_link(&args[1..]);
    }
    let opt = |key: &str| -> Option<String> {
        args.iter().position(|a| a == key).and_then(|i| args.get(i + 1)).cloned()
    };
    let term = opt("--term").ok_or("用法: coldrevive-ed2k search --term <词> --bootstrap ip:port [--public] [--rate-limit-pps 0.2]")?;
    let mut contacts = Vec::new();
    for (i, a) in args.iter().enumerate() {
        if a != "--bootstrap" { continue; }
        let raw = args.get(i + 1).ok_or("--bootstrap 缺地址")?;
        let addr: SocketAddr = raw.parse()
            .or_else(|_| {
                let ip: Ipv4Addr = raw.parse().map_err(|_| format!("非法地址 {raw}"))?;
                Ok(SocketAddr::V4(SocketAddrV4::new(ip, 4712)))
            })
            .map_err(|e: String| e)?;
        contacts.push(addr);
    }
    if contacts.is_empty() { return Err("至少需要一个 --bootstrap ip:port".into()); }
    let is_public_run = args.iter().any(|a| a == "--public");
    if !is_public_run {
        for c in &contacts {
            if let SocketAddr::V4(v) = c {
                if is_public(*v.ip()) {
                    return Err(format!("{} 是公网地址；联系公共 KAD 必须显式加 --public", c));
                }
            }
        }
    }
    let rate_pps: f64 = opt("--rate-limit-pps")
        .and_then(|s| s.parse().ok())
        .unwrap_or(if is_public_run { 0.2 } else { 0.0 });
    if !rate_pps.is_finite() || rate_pps < 0.0 || rate_pps > 10.0 {
        return Err("--rate-limit-pps 必须在 0..=10 之间（0=不限速）".into());
    }
    let mut pacer = Pacer::new(rate_pps);
    let timeout_ms: u64 = opt("--timeout-ms").and_then(|s| s.parse().ok()).unwrap_or(1800);
    let retries: usize = opt("--retry").and_then(|s| s.parse().ok()).unwrap_or(2);
    let max_answers: usize = opt("--max-answers").and_then(|s| s.parse().ok()).unwrap_or(20);
    let max_hops: usize = opt("--max-hops").and_then(|s| s.parse().ok()).unwrap_or(6);
    let alpha: usize = opt("--alpha").and_then(|s| s.parse().ok()).unwrap_or(3);
    let no_lookup = args.iter().any(|a| a == "--no-lookup");
    // aMule KAD v8 握手：需要对方 CryptoKadUDPKey 与我们可见公网 IP 才能
    // 让 validReceiverKey=true；缺任一项仍发 try-0，但只记录未验证。
    let do_hello = args.iter().any(|a| a == "--hello");
    let hello_try2 = args.iter().any(|a| a == "--hello-try2");
    let hello_plain = args.iter().any(|a| a == "--hello-plain");
    let peer_udp_key: Option<u32> = opt("--peer-udp-key").and_then(|s| s.parse().ok());
    let our_public_ip: Option<[u8; 4]> = opt("--our-public-ip").and_then(|s| s.parse::<Ipv4Addr>().ok())
        .map(|ip| ip.octets());
    if let Some(raw) = opt("--peer-udp-key") {
        if peer_udp_key.is_none() {
            return Err(format!("--peer-udp-key 非法: {raw}"));
        }
    }
    if let Some(raw) = opt("--our-public-ip") {
        if our_public_ip.is_none() {
            return Err(format!("--our-public-ip 非法: {raw}"));
        }
    }
    // A 轨最小样本发布：一个唯一关键词、一个已验证在区节点、0.2 pps；不批量。
    let publish_file = opt("--publish-file");
    if publish_file.is_some() && (!is_public_run || rate_pps <= 0.0) {
        return Err("--publish-file 只在 --public 且 --rate-limit-pps>0 时允许（G0 最小样本）".into());
    }
    let key = ed2k_keyword_key(&term);
    let frame = search_key_req(&key);
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("bind UDP: {e}"))?;
    let mut all: Vec<Answer> = Vec::new();
    let mut probes: Vec<Json> = Vec::new();
    let mut first_error = String::new();
    let mut seeds: Vec<Contact> = Vec::new();
    let mut boot_results: Vec<(SocketAddr, Option<u64>, String)> = Vec::new();
    let mut boot_self_ids: std::collections::HashMap<SocketAddr, [u8; 16]> = std::collections::HashMap::new();
    for dest in &contacts {
        match bootstrap(&sock, *dest, Duration::from_millis(timeout_ms), &mut pacer) {
            Ok((ms, self_id, found)) => {
                boot_self_ids.insert(*dest, self_id);
                boot_results.push((*dest, Some(ms), String::new()));
                if let SocketAddr::V4(v) = dest {
                    seeds.push(Contact { id: self_id, ip: v.ip().octets(), udp: v.port(), tcp: 0, version: 0 });
                }
                seeds.extend(found);
            }
            Err(e) => {
                let msg = format!("{dest}: {e}");
                if first_error.is_empty() { first_error = msg.clone(); }
                boot_results.push((*dest, None, msg));
            }
        }
    }

    #[derive(Clone, Copy)]
    struct Target { addr: SocketAddrV4, id: [u8; 16], kind: &'static str }
    let mut targets: Vec<Target> = Vec::new();
    let lookup_json = if is_public_run && !no_lookup {
        let l = lookup_nodes(&sock, seeds, &key, Duration::from_millis(timeout_ms),
                             &mut pacer, max_hops, alpha, false);
        let mut in_zone: Vec<Contact> = l.verified.iter().copied()
            .filter(|c| in_tolerance_zone(&c.id, &key)).collect();
        in_zone.sort_by_key(|c| xor_distance(&c.id, &key));
        // G0 最小样本：最多 3 个已验证在区节点，不批量发检索。
        targets = in_zone.iter().take(3)
            .map(|c| Target { addr: c.addr(), id: c.id, kind: "verified_in_zone" }).collect();
        let target_addrs: Vec<Json> = targets.iter().map(|t| Json::s(&t.addr.to_string())).collect();
        Json::obj(vec![
            ("mode", Json::s("kad2_find_node_convergence")),
            ("max_hops", Json::n(max_hops as f64)),
            ("alpha", Json::n(alpha as f64)),
            ("hops_used", Json::n(l.hops as f64)),
            ("sent", Json::n(l.sent as f64)),
            ("answered", Json::n(l.answered as f64)),
            ("verified_contacts", Json::n(l.verified.len() as f64)),
            ("in_zone_contacts", Json::n(l.in_zone as f64)),
            ("closest_top_chunk", Json::n(l.closest_top_chunk as f64)),
            ("tolerance_zone_reached", Json::b(l.in_zone > 0)),
            ("search_targets", Json::arr(target_addrs)),
            ("note", Json::s("0x33 只发给 0x21/0x29 验证且落在 2^24 责任区内的节点；未收敛则不发，避免把静默丢弃误记为 0 命中")),
        ])
    } else {
        for (dest, boot_ms, err) in &boot_results {
            if err.is_empty() && boot_ms.is_some() {
                if let SocketAddr::V4(v) = dest {
                    // 用 BOOTSTRAP_RES 自报的 KadID 做混淆封装；拿不到才退回明文（LAN 控制臂）。
                    let id = boot_self_ids.get(dest).copied().unwrap_or([0u8; 16]);
                    targets.push(Target { addr: *v, id, kind: "bootstrap" });
                }
            }
        }
        Json::obj(vec![
            ("mode", Json::s("direct_bootstrap")),
            ("note", Json::s("非公网模式保留原有直连路径（LAN 真实 aMuleD 回归用）")),
        ])
    };

    // 每个目标最终采用的混淆信封。HELLO 不是行政前置，但显式 --hello 时
    // 会先走 v8 三次握手；后续 0x33/0x43 用 try-2（已知对方 verify key）或 try-0。
    let mut envelopes: HashMap<String, KadEnvelope> = HashMap::new();
    let mut handshake_json = Vec::new();
    let local_udp_port = sock.local_addr().map(|a| a.port()).unwrap_or(0);
    for t in &targets {
        let key = t.addr.to_string();
        let receiver_key = match (peer_udp_key, our_public_ip) {
            (Some(k), Some(ip)) => kad_obfuscate::udp_verify_key(k, ip),
            _ => 0,
        };
        let envelope = if receiver_key != 0 {
            KadEnvelope::Try2(receiver_key)
        } else if t.id != [0u8; 16] {
            KadEnvelope::Try0 { id: t.id, receiver_key: 0 }
        } else {
            KadEnvelope::Plain
        };
        envelopes.insert(key.clone(), envelope);
        if do_hello && t.id != [0u8; 16] {
            let rec = match kad_hello(&sock, t.addr.into(), &t.id, peer_udp_key, our_public_ip,
                                      local_udp_port, hello_try2, hello_plain,
                                      Duration::from_millis(timeout_ms), &mut pacer) {
                Ok(h) => {
                    let mut fields: Vec<(String, Json)> = vec![
                        ("target".into(), Json::s(&key)),
                        ("ok".into(), Json::b(true)),
                    ];
                    if let Json::Obj(items) = h { fields.extend(items); }
                    Json::Obj(fields)
                }
                Err(e) => Json::obj(vec![
                    ("target", Json::s(&key)),
                    ("ok", Json::b(false)),
                    ("error", Json::s(&e)),
                ]),
            };
            handshake_json.push(rec);
        }
    }
    let handshake = Json::obj(vec![
        ("enabled", Json::b(do_hello)),
        ("try2_requested", Json::b(hello_try2)),
        ("plain_requested", Json::b(hello_plain)),
        ("peer_udp_key_supplied", Json::b(peer_udp_key.is_some())),
        ("our_public_ip_supplied", Json::b(our_public_ip.is_some())),
        ("attempts", Json::arr(handshake_json)),
    ]);

    let publish_json = match &publish_file {
        None => Json::obj(vec![("mode", Json::s("none"))]),
        Some(path) => {
            let data = fs::read(path).map_err(|e| format!("读取 --publish-file {path}: {e}"))?;
            let name = Path::new(path).file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .ok_or("--publish-file 路径没有文件名")?;
            let file_id = SharedFile::new(name.clone(), data.clone()).hash();
            let req = publish_key_req(&key, &file_id, &name, data.len() as u32);
            let mut attempts = Vec::new();
            let mut acked = 0usize;
            // 同一个唯一 key 最多投 3 个已验证在区节点（G0 sample_size=3），
            // 因为 S2 §20.5 实测其中一部分节点「能问路但不接收索引」。
            for t in targets.iter().take(3) {
                let mut rec = vec![
                    ("target", Json::s(&t.addr.to_string())),
                    ("top_chunk", Json::n(top_chunk(&t.id, &key) as f64)),
                    ("ack", Json::b(false)),
                    ("load", Json::n(-1.0)),
                    ("rtt_ms", Json::n(0.0)),
                    ("error", Json::s("")),
                ];
                // 索引操作按握手结果走 try-2（已知对方 verify key）或 try-0。
                let env = *envelopes.get(&t.addr.to_string())
                    .unwrap_or(&KadEnvelope::Try0 { id: t.id, receiver_key: 0 });
                match kad_exchange(&sock, t.addr.into(), &req, OP_PUBLISH_RES,
                                   Duration::from_millis(timeout_ms), &mut pacer, env) {
                    Ok((payload, ms, _, _)) => match parse_publish_res(&payload) {
                        Ok((echo, load)) if echo == key => {
                            acked += 1;
                            rec[2] = ("ack", Json::b(true));
                            rec[3] = ("load", Json::n(load as f64));
                            rec[4] = ("rtt_ms", Json::n(ms as f64));
                        }
                        Ok(_) => rec[5] = ("error", Json::s("PUBLISH_RES key 回显不符")),
                        Err(e) => rec[5] = ("error", Json::s(&e)),
                    },
                    Err(e) => rec[5] = ("error", Json::s(&e)),
                }
                attempts.push(Json::obj(rec));
            }
            if attempts.is_empty() {
                attempts.push(Json::obj(vec![
                    ("target", Json::s("")),
                    ("ack", Json::b(false)),
                    ("error", Json::s("没有 2^24 责任区内已验证节点，未发布")),
                ]));
            }
            Json::obj(vec![
                ("mode", Json::s("a_track_minimal_sample")),
                ("file", Json::s(path)),
                ("file_id", Json::s(&hex(&file_id))),
                ("ack_count", Json::n(acked as f64)),
                ("attempts", Json::arr(attempts)),
                ("note", Json::s("ACK 只证明对端收下 key；公网可检索只认随后真实 SEARCH_RES")),
            ])
        }
    };

    let mut seen_files: HashSet<[u8; 16]> = HashSet::new();
    for t in &targets {
        let dest: SocketAddr = t.addr.into();
        let boot_ms = boot_results.iter()
            .find(|(d, _, _)| *d == dest).and_then(|(_, m, _)| *m).unwrap_or(0);
        let mut contact_error = String::new();
        let mut got = None;
        let envelope = *envelopes.get(&t.addr.to_string())
            .unwrap_or(&if t.id == [0u8; 16] { KadEnvelope::Plain } else {
                KadEnvelope::Try0 { id: t.id, receiver_key: 0 }
            });
        for attempt in 0..retries {
            if attempt > 0 { std::thread::sleep(Duration::from_millis(1200)); }
            match exchange(&sock, dest, &frame, Duration::from_millis(timeout_ms), &mut pacer, envelope) {
                Ok(v) => { got = Some(v); break; }
                Err(e) if contact_error.is_empty() => contact_error = format!("{dest}: {e}"),
                Err(_) => {}
            }
        }
        if !contact_error.is_empty() && first_error.is_empty() { first_error = contact_error.clone(); }
        match got {
            Some((sender, target, answers, rtt_ms, obfuscated)) => {
                probes.push(Json::obj(vec![
                    ("contact", Json::s(&dest.to_string())),
                    ("target_kind", Json::s(t.kind)),
                    ("top_chunk", Json::n(top_chunk(&t.id, &key) as f64)),
                    ("bootstrap_ms", Json::n(boot_ms as f64)),
                    ("obfuscated_response", Json::b(obfuscated)),
                    ("sender", Json::s(&hex(&sender))),
                    ("target", Json::s(&hex(&target))),
                    ("target_matches_key", Json::b(target == key)),
                    ("answers", Json::n(answers.len() as f64)),
                    ("rtt_ms", Json::n(rtt_ms as f64)),
                ]));
                for a in answers {
                    if seen_files.insert(a.file_id) { all.push(a); }
                }
            }
            None => probes.push(Json::obj(vec![
                ("contact", Json::s(&dest.to_string())),
                ("target_kind", Json::s(t.kind)),
                ("top_chunk", Json::n(top_chunk(&t.id, &key) as f64)),
                ("bootstrap_ms", Json::n(boot_ms as f64)),
                ("error", Json::s(&contact_error)),
            ])),
        }
    }
    // bootstrap 失败但没进入检索的入口也要留证据，不能只报成功路径。
    for (dest, boot_ms, err) in &boot_results {
        if !err.is_empty() && !targets.iter().any(|t| t.addr.to_string() == dest.to_string()) {
            probes.push(Json::obj(vec![
                ("contact", Json::s(&dest.to_string())),
                ("target_kind", Json::s("bootstrap_failed")),
                ("bootstrap_ms", Json::n(boot_ms.unwrap_or(0) as f64)),
                ("error", Json::s(err)),
            ]));
        }
    }
    all.truncate(max_answers);
    let links: Vec<Json> = all.iter().filter_map(|a| ed2k_link(a).map(|l| Json::s(&l))).collect();
    Ok(Json::obj(vec![
        ("schema", Json::s("coldrevive.fr24.kad-search/v1")),
        ("term", Json::s(&term)),
        ("keyword_key", Json::s(&hex(&key))),
        ("keyword_key_is_unnamespaced_md4", Json::b(true)),
        ("rate_limit_pps", Json::n(rate_pps)),
        ("rate_limited", Json::b(rate_pps > 0.0)),
        ("public_kad_contacted", Json::b(is_public_run)),
        ("lookup", lookup_json),
        ("handshake", handshake),
        ("publish", publish_json),
        ("contacts", Json::arr(probes)),
        ("answers", Json::arr(all.iter().map(answer_json).collect())),
        ("answer_count", Json::n(all.len() as f64)),
        ("ed2k_links", Json::arr(links)),
        // 红线：程序永不自证证据有效；pass 只能由执行人按真实证据回填。
        ("evidence_valid_for_pass", Json::b(false)),
        ("error", Json::s(&first_error)),
    ]))
}

fn main() -> ExitCode {
    match run() {
        Ok(j) => { println!("{}", j.dump()); ExitCode::SUCCESS }
        Err(e) => { eprintln!("coldrevive-ed2k: {e}"); ExitCode::FAILURE }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pacer_defaults_match_g0_rate() {
        assert_eq!(Pacer::new(0.2).interval, Duration::from_millis(5000));
        assert!(Pacer::new(0.0).interval.is_zero());
    }

    #[test]
    fn parses_kad_search_res_with_a_real_tag_shape() {
        let mut p = Vec::new();
        p.extend_from_slice(&[0x11; 16]); // sender
        p.extend_from_slice(&[0x22; 16]); // target
        p.extend_from_slice(&1u16.to_le_bytes());
        p.extend_from_slice(&[0xab; 16]); // file_id
        p.push(2); // tag count
        // TAG_FILENAME (0x01), KAD string tag type 0x02, name len 1, name 0x01
        p.push(TAG_STRING);
        p.extend_from_slice(&1u16.to_le_bytes());
        p.push(TAG_FILENAME);
        p.extend_from_slice(&8u16.to_le_bytes());
        p.extend_from_slice(b"test.bin");
        // TAG_FILESIZE (0x02), UINT32, name len 1, value 1024
        p.push(TAG_U32);
        p.extend_from_slice(&1u16.to_le_bytes());
        p.push(TAG_FILESIZE);
        p.extend_from_slice(&1024u32.to_le_bytes());

        let (sender, target, answers) = parse_search_res(&p).unwrap();
        assert_eq!(sender, [0x11; 16]);
        assert_eq!(target, [0x22; 16]);
        assert_eq!(answers.len(), 1);
        let link = ed2k_link(&answers[0]).unwrap();
        assert!(link.contains("|1024|"), "{link}");
        assert!(link.ends_with("/"));
    }

    #[test]
    fn rejects_strn_tag_on_kad_path() {
        let mut p = vec![0u8; 32];
        p.extend_from_slice(&1u16.to_le_bytes());
        p.extend_from_slice(&[0u8; 16]);
        p.push(1);
        p.push(0x11); // STR1: invalid on KAD ReadTag path
        p.extend_from_slice(&1u16.to_le_bytes());
        p.push(0x01);
        assert!(parse_search_res(&p).is_err());
    }
}
