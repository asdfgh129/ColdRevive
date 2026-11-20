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
use cr_ed2k::stack::{download, serve, SharedFile, UploadQueue};
use cr_ed2k::wire::{ct, Hello};
use cr_hash::{hex, md4};
use cr_kad::kad_ed2k::{ed2k_keyword_key, search_key_req, OP_SEARCH_RES};
use std::env;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
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
const OP_BOOTSTRAP_REQ: u8 = 0x01;
const OP_BOOTSTRAP_RES: u8 = 0x09;

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

/// aMule 要求先有 BOOTSTRAP_REQ/RES 才把发送方当作 KAD 节点处理；
/// 直接发 0x33 会被静默忽略（S2 早期探针踩过，表现为「超时=查无结果」）。
fn bootstrap(sock: &UdpSocket, dest: SocketAddr, timeout: Duration) -> Result<u64, String> {
    let started = Instant::now();
    sock.send_to(&[0xe4, OP_BOOTSTRAP_REQ], dest)
        .map_err(|e| format!("bootstrap send: {e}"))?;
    let mut buf = [0u8; 65535];
    loop {
        let left = timeout.checked_sub(started.elapsed()).unwrap_or(Duration::ZERO);
        sock.set_read_timeout(Some(left)).map_err(|e| e.to_string())?;
        let (n, src) = match sock.recv_from(&mut buf) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                   || e.kind() == std::io::ErrorKind::TimedOut => return Err("BOOTSTRAP_RES 超时".into()),
            Err(e) => return Err(format!("bootstrap recv: {e}")),
        };
        if src == dest && n >= 2 && buf[0] == 0xe4 && buf[1] == OP_BOOTSTRAP_RES {
            return Ok(started.elapsed().as_millis() as u64);
        }
    }
}

fn exchange(sock: &UdpSocket, dest: SocketAddr, frame: &[u8], timeout: Duration)
    -> Result<([u8; 16], [u8; 16], Vec<Answer>, u64), String>
{
    let started = Instant::now();
    sock.send_to(frame, dest).map_err(|e| format!("send_to {dest}: {e}"))?;
    let mut buf = [0u8; 65535];
    loop {
        let left = timeout.checked_sub(started.elapsed()).unwrap_or(Duration::ZERO);
        sock.set_read_timeout(Some(left)).map_err(|e| e.to_string())?;
        let (n, src) = match sock.recv_from(&mut buf) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                   || e.kind() == std::io::ErrorKind::TimedOut => return Err("超时未收到 SEARCH_RES".into()),
            Err(e) => return Err(format!("recv_from: {e}")),
        };
        if src != dest || n < 2 || buf[0] != 0xe4 || buf[1] != OP_SEARCH_RES { continue; }
        let (sender, target, answers) = parse_search_res(&buf[2..n])?;
        return Ok((sender, target, answers, started.elapsed().as_millis() as u64));
    }
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
    let out = serve(stream, &[shared], &our_hello, &queue, Duration::from_millis(timeout_ms));
    Ok(Json::obj(vec![
        ("schema", Json::s("coldrevive.fr25.live-upload/v1")),
        ("peer", Json::s(&peer.to_string())),
        ("peer_hello", Json::b(out.peer_hello.is_some())),
        ("queue_granted", Json::b(out.queue_granted)),
        ("queue_rank_sent", match out.queue_rank_sent { Some(r) => Json::n(r as f64), None => Json::s("") }),
        ("blocks_sent", Json::n(out.blocks_sent as f64)),
        ("bytes_sent", Json::n(out.bytes_sent as f64)),
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
    let term = opt("--term").ok_or("用法: coldrevive-ed2k search --term <词> --bootstrap ip:port [--public]")?;
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
    if !args.iter().any(|a| a == "--public") {
        for c in &contacts {
            if let SocketAddr::V4(v) = c {
                if is_public(*v.ip()) {
                    return Err(format!("{} 是公网地址；联系公共 KAD 必须显式加 --public", c));
                }
            }
        }
    }
    let timeout_ms: u64 = opt("--timeout-ms").and_then(|s| s.parse().ok()).unwrap_or(1800);
    let retries: usize = opt("--retry").and_then(|s| s.parse().ok()).unwrap_or(2);
    let max_answers: usize = opt("--max-answers").and_then(|s| s.parse().ok()).unwrap_or(20);
    let key = ed2k_keyword_key(&term);
    let frame = search_key_req(&key);
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("bind UDP: {e}"))?;
    let mut all = Vec::new();
    let mut probes = Vec::new();
    let mut first_error = String::new();
    for dest in &contacts {
        let mut contact_error = String::new();
        let boot = bootstrap(&sock, *dest, Duration::from_millis(timeout_ms));
        let boot_ms = match &boot {
            Ok(ms) => Some(*ms),
            Err(e) => {
                contact_error = format!("{dest}: {e}");
                if first_error.is_empty() { first_error = contact_error.clone(); }
                None
            }
        };
        let mut got = None;
        if boot.is_ok() {
            for attempt in 0..retries {
                if attempt > 0 {
                    std::thread::sleep(Duration::from_millis(1200));
                }
                match exchange(&sock, *dest, &frame, Duration::from_millis(timeout_ms)) {
                    Ok(v) => { got = Some(v); break; }
                    Err(e) if contact_error.is_empty() => contact_error = format!("{dest}: {e}"),
                    Err(_) => {}
                }
            }
        }
        if contact_error.is_empty() && first_error.is_empty() {
            first_error = contact_error.clone();
        }
        match got {
            Some((sender, target, answers, rtt_ms)) => {
                probes.push(Json::obj(vec![
                    ("contact", Json::s(&dest.to_string())),
                    ("bootstrap_ms", Json::n(boot_ms.unwrap_or(0) as f64)),
                    ("sender", Json::s(&hex(&sender))),
                    ("target", Json::s(&hex(&target))),
                    ("target_matches_key", Json::b(target == key)),
                    ("answers", Json::n(answers.len() as f64)),
                    ("rtt_ms", Json::n(rtt_ms as f64)),
                ]));
                all.extend(answers);
            }
            None => probes.push(Json::obj(vec![
                ("contact", Json::s(&dest.to_string())),
                ("bootstrap_ms", Json::n(boot_ms.unwrap_or(0) as f64)),
                ("error", Json::s(&contact_error)),
            ])),
        }
    }
    all.truncate(max_answers);
    let links: Vec<Json> = all.iter().filter_map(|a| ed2k_link(a).map(|l| Json::s(&l))).collect();
    Ok(Json::obj(vec![
        ("schema", Json::s("coldrevive.fr24.kad-search/v1")),
        ("term", Json::s(&term)),
        ("keyword_key", Json::s(&hex(&key))),
        ("keyword_key_is_unnamespaced_md4", Json::b(true)),
        ("public_kad_contacted", Json::b(contacts.iter().any(|c| match c {
            SocketAddr::V4(v) => is_public(*v.ip()),
            SocketAddr::V6(_) => true,
        }))),
        ("contacts", Json::arr(probes)),
        ("answers", Json::arr(all.iter().map(answer_json).collect())),
        ("answer_count", Json::n(all.len() as f64)),
        ("ed2k_links", Json::arr(links)),
        ("evidence_valid_for_pass", Json::b(all.iter().any(|a| ed2k_link(a).is_some()))),
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
