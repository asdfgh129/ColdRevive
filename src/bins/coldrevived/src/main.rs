//! coldrevived — M1 双节点守护进程。
//!
//! 真实链路（T0 / G1）：
//!   A: 共享样本 → SHA-256 → 伴随 .torrent → COLD 影子索引 → 本地 KAD 命名空间发布
//!   B: KAD 关键词发现 → UDP WAKEUP_REQ(PROTO v1) → A 校验 Challenge/时间窗/nonce
//!      → COLD/WAKING/HOT → WAKEUP_ACK(magnet + torrent_url) → 第三方 aria2 标准 BT 下载
//!
//! 红线：只监听 127.0.0.1；不向公共 KAD 发包；所有观测取自真实文件 / 真实网络 / 真实进程，
//! 绝不返回「看起来像通过」的合成常量。

mod bt;
mod commands;

use cr_core::{Config, Json};
use cr_indexer::inspect as inspect_file;
use cr_kad::{find_result, publish_keys, udp_reply};
use cr_store::{FileEntry, ShadowIndex};
use cr_torrent::Torrent;
use cr_wake::{build_ack, reject, validate_req};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// T0 golden 的 piece_size（262144 B = 样本大小，1 片）。
pub const PIECE_SIZE: usize = 262144;
/// node_secret_seed：同一 T0 子网内两节点共享，仅用于 Challenge，不是用户口令。
pub const NODE_SECRET: &[u8] = b"coldrevive-t0-local-secret-v1";

pub struct DownloadRecord {
    pub path: PathBuf,
    pub sha256: String,
    pub piece_ok: bool,
    pub complete: bool,
    pub private_seen: bool,
}

pub struct NodeState {
    pub cfg: Config,
    pub index: ShadowIndex,
    pub nonces: HashSet<String>,
    pub remote_keys: HashMap<String, String>,
    pub last_ack: Option<Json>,
    pub sample: Option<Vec<u8>>,
    pub torrent: Option<Torrent>,
    pub bt_seeding: bool,
    pub download: Option<DownloadRecord>,
    pub stale_pids: Vec<String>,
    pub blacklist: HashSet<String>,
    pub last_file_hash: Option<String>,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let opt = |name: &str| -> Option<String> {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
    };
    let Some(cfg_path) = opt("--config") else {
        eprintln!("用法: coldrevived --config <file> --node-id <A|B> [--bind ...]");
        std::process::exit(2);
    };
    let node_id = opt("--node-id").unwrap_or_else(|| "A".to_string());
    let mut cfg = match Config::load(Path::new(&cfg_path)) {
        Ok(c) => c,
        Err(e) => { eprintln!("配置载入失败: {}", e); std::process::exit(2); }
    };
    cfg.node_id = node_id.clone();
    if let Some(b) = opt("--bind") { cfg.bind = b; }
    let me = match cfg.me() {
        Ok(m) => m.clone(),
        Err(e) => { eprintln!("{}", e); std::process::exit(2); }
    };

    let state_path = cfg.workdir.join(format!("state-{}.json", node_id));
    // 残留进程检查：harness 每次 prepare() 会清空 .run/；这里只记录启动时已存在的 .pid 文件。
    let stale_pids: Vec<String> = fs::read_dir(&cfg.workdir).ok().into_iter().flatten()
        .filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".pid")).collect();

    let mut st = NodeState {
        index: ShadowIndex::open(&state_path),
        cfg: cfg.clone(),
        nonces: HashSet::new(),
        remote_keys: HashMap::new(),
        last_ack: None,
        sample: None,
        torrent: None,
        bt_seeding: false,
        download: None,
        stale_pids,
        blacklist: HashSet::new(),
        last_file_hash: None,
    };
    if !st.stale_pids.is_empty() {
        println!("[{}] 启动时发现残留 .pid 文件: {:?}", node_id, st.stale_pids);
    }

    if node_id == "A" {
        if let Err(e) = bootstrap_holder(&cfg, &mut st) {
            eprintln!("A 节点初始化失败: {}", e);
            std::process::exit(2);
        }
        let torrent = st.torrent.clone().unwrap();
        let sample = st.sample.clone().unwrap();
        let infohash = torrent.info_hash.clone();
        let shared = Arc::new(Mutex::new(st));
        bt::spawn_seeder(cfg.clone(), infohash, sample, torrent.piece_size, shared.clone());
        spawn_udp(cfg.clone(), me.wake_udp_port, shared.clone());
        spawn_http(cfg.clone(), me.http_port, shared);
        loop { thread::sleep(Duration::from_secs(3600)); }
    } else {
        let shared = Arc::new(Mutex::new(st));
        spawn_udp(cfg.clone(), me.wake_udp_port, shared.clone());
        spawn_http(cfg.clone(), me.http_port, shared);
        loop { thread::sleep(Duration::from_secs(3600)); }
    }
}

fn bootstrap_holder(cfg: &Config, st: &mut NodeState) -> Result<(), String> {
    let facts = inspect_file(&cfg.sample_path)?;
    let announce = format!("http://{}:{}/announce", cfg.bind,
                           cfg.node_http(&cfg.node_id).unwrap_or(0));
    let torrent = cr_torrent::generate(&facts.data, &facts.name, PIECE_SIZE, &announce);
    let torrent_path = cfg.workdir.join("meta").join(format!("{}.torrent", torrent.info_hash));
    torrent.write_to(&torrent_path)?;

    let mut e = FileEntry::new(&facts.file_hash, &cfg.sample_path, &facts.name, facts.size);
    e.torrent_path = torrent_path;
    e.infohash = torrent.info_hash.clone();
    e.piece_size = PIECE_SIZE;
    e.keywords = cfg.keywords.clone();
    e.published_keys = publish_keys(&cfg.namespace, &facts.file_hash, &cfg.keywords);
    e.kad_published = true;
    st.index.insert(e);
    st.index.save()?;
    st.sample = Some(facts.data);
    st.torrent = Some(torrent);
    println!("[{}] 样本入库: {} B, infohash={}, keys={}",
             cfg.node_id, st.index.len(), st.torrent.as_ref().unwrap().info_hash,
             st.index.all_published_keys().len());
    Ok(())
}

// ---------------------------------------------------------------------------
// HTTP（最小 std-only 实现；axum / RPC / WS 属 M3）
// ---------------------------------------------------------------------------

fn spawn_http(cfg: Config, port: u16, state: Arc<Mutex<NodeState>>) {
    thread::spawn(move || {
        let listener = match TcpListener::bind(format!("{}:{}", cfg.bind, port)) {
            Ok(l) => l,
            Err(e) => { eprintln!("[{}] HTTP 绑定失败: {}", cfg.node_id, e); return; }
        };
        println!("[{}] HTTP 监听 {}:{}", cfg.node_id, cfg.bind, port);
        for stream in listener.incoming() {
            if let Ok(s) = stream {
                let st = state.clone();
                let c = cfg.clone();
                thread::spawn(move || handle_conn(s, c, st));
            }
        }
    });
}

fn handle_conn(mut stream: TcpStream, cfg: Config, state: Arc<Mutex<NodeState>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(130)));
    let req = match read_http(&mut stream) {
        Ok(r) => r,
        Err(_) => return,
    };
    let (method, path, body) = req;
    let response: (String, Vec<u8>, &'static str) = if path.starts_with("/health") {
        ("200 OK".into(),
         Json::obj(vec![("status", Json::s("ok")), ("node", Json::s(&cfg.node_id))])
             .dump().into_bytes(), "application/json")
    } else if path == "/cli" && method == "POST" {
        let out = match Json::parse(&String::from_utf8_lossy(&body)) {
            Ok(j) => commands::dispatch(&state, &j),
            Err(e) => Json::obj(vec![("error", Json::s(format!("非法请求: {}", e)))]),
        };
        ("200 OK".into(), out.dump().into_bytes(), "application/json")
    } else if path == "/inject" && method == "POST" {
        let out = handle_inject(&state, &body);
        ("200 OK".into(), out.dump().into_bytes(), "application/json")
    } else if path.starts_with("/torrent/") {
        let wanted = path.trim_start_matches("/torrent/");
        let found = {
            let st = state.lock().unwrap();
            st.index.entries.values().find(|e| e.infohash == wanted || e.file_hash == wanted)
                .map(|e| e.torrent_path.clone())
        };
        match found.and_then(|p| fs::read(&p).ok()) {
            Some(bytes) => ("200 OK".into(), bytes, "application/x-bittorrent"),
            None => ("404 Not Found".into(), b"no torrent".to_vec(), "text/plain"),
        }
    } else if path.starts_with("/announce") {
        let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
        let port = cfg.me().map(|m| m.bt_port).unwrap_or(0);
        ("200 OK".into(), bt::tracker_response(query, &cfg.bind, port), "application/x-bittorrent")
    } else {
        ("404 Not Found".into(), b"not found".to_vec(), "text/plain")
    };
    write_http(&mut stream, &response.0, response.2, &response.1);
}

/// 故障注入控制面（仅 127.0.0.1、仅 T0 测试用）。A 节点真实移动/恢复样本文件。
fn handle_inject(state: &Arc<Mutex<NodeState>>, body: &[u8]) -> Json {
    let Ok(j) = Json::parse(&String::from_utf8_lossy(body)) else {
        return Json::obj(vec![("ok", Json::b(false))]);
    };
    let kind = j.get("kind").and_then(Json::as_str).unwrap_or("");
    let mut st = state.lock().unwrap();
    let path = st.cfg.sample_path.clone();
    let gone = path.with_extension("bin.gone");
    match kind {
        "file-gone" => {
            let ok = fs::rename(&path, &gone).is_ok();
            Json::obj(vec![("ok", Json::b(ok)), ("kind", Json::s(kind))])
        }
        "restore" => {
            let ok = if gone.exists() { fs::rename(&gone, &path).is_ok() } else { path.exists() };
            if ok {
                let (namespace, keywords) = (st.cfg.namespace.clone(), st.cfg.keywords.clone());
                let mut all_published: Vec<(String, Vec<String>)> = st.index.entries.iter()
                    .map(|(h, _)| (h.clone(), publish_keys(&namespace, h, &keywords)))
                    .collect();
                for (h, keys) in all_published.drain(..) {
                    if let Some(e) = st.index.get_mut(&h) {
                        e.kad_published = true;
                        e.published_keys = keys;
                    }
                }
                let _ = st.index.save();
            }
            Json::obj(vec![("ok", Json::b(ok)), ("kind", Json::s(kind))])
        }
        _ => Json::obj(vec![("ok", Json::b(false)), ("kind", Json::s(kind))]),
    }
}

fn read_http(stream: &mut TcpStream) -> Result<(String, String, Vec<u8>), String> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end;
    loop {
        let n = stream.read(&mut tmp).map_err(|e| e.to_string())?;
        if n == 0 { return Err("连接提前关闭".into()); }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            header_end = p + 4;
            break;
        }
        if buf.len() > 1024 * 1024 { return Err("请求头过大".into()); }
    }
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let first = head.lines().next().unwrap_or("");
    let mut it = first.split_whitespace();
    let method = it.next().unwrap_or("").to_string();
    let path = it.next().unwrap_or("").to_string();
    let len: usize = head.lines()
        .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
        .and_then(|l| l.split_once(':')).and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0);
    while buf.len() < header_end + len {
        let n = stream.read(&mut tmp).map_err(|e| e.to_string())?;
        if n == 0 { break; }
        buf.extend_from_slice(&tmp[..n]);
    }
    Ok((method, path, buf[header_end..header_end + len.min(buf.len() - header_end)].to_vec()))
}

fn write_http(stream: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status, ctype, body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

/// 真实 HTTP GET（取伴随种子、tracker 等）。失败返回 Err，绝不返回占位字节。
pub fn http_get(url: &str) -> Result<Vec<u8>, String> {
    let rest = url.strip_prefix("http://").ok_or_else(|| "仅支持 http://".to_string())?;
    let (authority, path) = rest.split_once('/').map(|(a, p)| (a, format!("/{}", p)))
        .unwrap_or((rest, "/".to_string()));
    let mut stream = TcpStream::connect(authority).map_err(|e| format!("连接 {} 失败: {}", authority, e))?;
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let req = format!("GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", path, authority);
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let pos = raw.windows(4).position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "HTTP 响应无头部".to_string())?;
    let head = String::from_utf8_lossy(&raw[..pos]).to_string();
    if !head.starts_with("HTTP/1.1 200") {
        return Err(format!("HTTP {}", head.lines().next().unwrap_or("")));
    }
    Ok(raw[pos + 4..].to_vec())
}

/// 真实 HTTP POST JSON，供 B 节点调用 A 的 /inject 与 /cli。
pub fn http_post_json(url: &str, body: &Json) -> Result<Json, String> {
    let rest = url.strip_prefix("http://").ok_or_else(|| "仅支持 http://".to_string())?;
    let (authority, path) = rest.split_once('/').map(|(a, p)| (a, format!("/{}", p)))
        .unwrap_or((rest, "/".to_string()));
    let mut stream = TcpStream::connect(authority).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let payload = body.dump();
    let req = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        path, authority, payload.as_bytes().len(), payload
    );
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let pos = raw.windows(4).position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "HTTP 响应无头部".to_string())?;
    Json::parse(&String::from_utf8_lossy(&raw[pos + 4..]))
        .map_err(|e| format!("响应非法 JSON: {}", e))
}

// ---------------------------------------------------------------------------
// UDP（KAD 发现 + 私有唤醒）
// ---------------------------------------------------------------------------

fn spawn_udp(cfg: Config, port: u16, state: Arc<Mutex<NodeState>>) {
    thread::spawn(move || {
        let sock = match UdpSocket::bind(format!("{}:{}", cfg.bind, port)) {
            Ok(s) => s,
            Err(e) => { eprintln!("[{}] UDP 绑定失败: {}", cfg.node_id, e); return; }
        };
        println!("[{}] UDP 监听 {}:{}", cfg.node_id, cfg.bind, port);
        let mut buf = [0u8; 65535];
        loop {
            let (n, src) = match sock.recv_from(&mut buf) { Ok(v) => v, Err(_) => continue };
            let frame = match Json::parse(&String::from_utf8_lossy(&buf[..n])) {
                Ok(j) => j, Err(_) => continue,
            };
            let reply = handle_udp_frame(&cfg, &state, &frame);
            if let Some(r) = reply { udp_reply(&sock, src, &r); }
        }
    });
}

fn handle_udp_frame(cfg: &Config, state: &Arc<Mutex<NodeState>>, frame: &Json) -> Option<Json> {
    let t = frame.get("t").and_then(Json::as_str).unwrap_or("");
    if t == "KAD_FIND" {
        let key = frame.get("key").and_then(Json::as_str).unwrap_or("");
        let st = state.lock().unwrap();
        let keys = st.index.all_published_keys();
        let hit = keys.iter().any(|k| k == key) || st.remote_keys.contains_key(key);
        return Some(find_result(if hit { &cfg.node_id } else { "" }, if hit { 1 } else { 0 }));
    }
    if t == "KAD_PUBLISH" {
        let node = frame.get("node").and_then(Json::as_str).unwrap_or("").to_string();
        if let Some(keys) = frame.get("keys").and_then(Json::as_arr) {
            let mut st = state.lock().unwrap();
            for k in keys.iter().filter_map(Json::as_str) {
                st.remote_keys.insert(k.to_string(), node.clone());
            }
        }
        return Some(Json::obj(vec![("t", Json::s("KAD_PUBLISH_ACK"))]));
    }
    if frame.get("type").and_then(Json::as_str) == Some("WAKEUP_REQ") {
        let trace = frame.get("trace_id").and_then(Json::as_str).unwrap_or("").to_string();
        let file_hash = frame.get("file_hash").and_then(Json::as_str).unwrap_or("").to_string();
        let mut st = state.lock().unwrap();
        if let Err(code) = validate_req(frame, NODE_SECRET, cr_core::unix_ts(), &mut st.nonces) {
            return Some(reject(&trace, code));
        }
        let entry = match st.index.get(&file_hash).cloned() {
            Some(e) => e,
            None => return Some(reject(&trace, "REJECT_NOT_FOUND")),
        };
        if !entry.path.is_file() {
            if let Some(e) = st.index.get_mut(&file_hash) {
                e.kad_published = false;
                e.published_keys.clear();
            }
            let _ = st.index.save();
            let mut r = reject(&trace, "REJECT_FILE_GONE");
            if let Json::Obj(ref mut p) = r { p.push(("kad_revoked".into(), Json::b(true))); }
            return Some(r);
        }
        // 幂等激活：已处于 HOT 的持有者不因重复唤醒再次跃迁（PROTO §3.1 / T0-17）。
        // 这样 COLD → WAKING → HOT 每个激活周期只记录一次，也不会重复拉起做种。
        let already_hot = st.index.get(&file_hash)
            .map(|e| e.state == cr_store::FileState::Hot).unwrap_or(false);
        if !already_hot {
            st.index.get_mut(&file_hash).unwrap().transition(cr_store::FileState::Waking);
            st.index.get_mut(&file_hash).unwrap().transition(cr_store::FileState::Hot);
        }
        st.bt_seeding = true;
        let _ = st.index.save();
        let me = cfg.me().ok()?;
        let url = format!("http://{}:{}/torrent/{}", cfg.bind, me.http_port, entry.infohash);
        return Some(build_ack(&trace, &entry.infohash, &url, entry.size,
                              entry.piece_size, me.bt_port));
    }
    None
}
