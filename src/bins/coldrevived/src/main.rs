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
mod nat_path;
mod relay_sub;

use cr_core::{Config, Json};
use cr_engine::{Engine, EngineOptions, RqbitEngine, TorrentSpec};
use cr_indexer::inspect as inspect_file;
use cr_kad::{find_result, publish_keys};
use cr_store::{FileEntry, ShadowIndex};
use cr_torrent::Torrent;
use cr_wake::{build_ack, reject, validate_req};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

/// UDP 监听 socket 的共享句柄。M2：`nat probe` 必须复用这个 socket 做 STUN，
/// 才能保证学习到的外部映射端口与实际唤醒端口一致。
#[derive(Clone)]
pub struct UdpHub {
    pub socket: Arc<UdpSocket>,
    /// 探测期间由 UDP 分发线程把 STUN 响应转交给 probe 的 channel。
    pub stun_sink: Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
}

/// W6.1 M3 接入层 + W5.1 引擎真下载：一条 `/jsonrpc` 任务的完整记录。
///
/// `engine_task = None` 表示这条任务**没有**接入引擎（只登记了 URI，或 `.torrent` 取回/解析失败）。
/// 这种任务永远停在 `waiting`、进度全为 0 —— 不编字节数。接入引擎后，进度与状态全部来自
/// `cr_engine::EngineProgress`（librqbit 的真实计数器）。
#[derive(Clone)]
pub struct RpcTask {
    pub gid: String,
    pub uri: String,
    /// aria2 兼容状态：waiting / active / paused / error / complete / removed。
    pub status: String,
    pub created_unix: u64,
    /// 任务若来自可解析的 `.torrent`，这里记真实值；拿不到就是空/0，绝不编造。
    pub info_hash: String,
    pub name: String,
    pub total_length: u64,
    pub piece_count: usize,
    /// 是否真的把 `.torrent` 取回来并解析过（GUI/前端据此区分「已解析」与「只登记了 URI」）。
    pub torrent_parsed: bool,
    pub note: String,
    /// 引擎内部任务号；`None` = 本任务没有数据面。
    pub engine_task: Option<u64>,
    pub output_path: String,
    pub error: String,
    /// 最近一次采样的已完成字节 / 上传字节 / 连接数，全部来自引擎。
    pub completed_bytes: u64,
    pub uploaded_bytes: u64,
    pub peers: usize,
    /// 实测速率采样点：用「两次真实采样之差 / 真实时间差」算，不猜引擎的速率单位。
    pub sample_bytes: u64,
    pub sample_unix_ms: u64,
    pub speed_bps: u64,
}

/// 惰性引擎槽：只有在真的收到可解析的 `.torrent` 任务时才创建。
/// T0/T1 的复活路径完全不经过它，因此 nat/engine 段缺省时行为逐字节不变。
pub type SharedEngine = Arc<Mutex<Option<RqbitEngine>>>;

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
    pub nat: nat_path::NatContext,
    pub hub: UdpHub,
    pub rpc_tasks: Vec<RpcTask>,
    /// W5.1：惰性 BT 引擎（`/jsonrpc` 收到可解析种子时才创建）。
    pub engine: SharedEngine,
    /// W5.1 对账用：内置标准 BT 种子端**真实发出**的分片字节与请求数。
    /// 下载方（引擎）报的 completedLength 是单方面说法，两边计数器对上才算证据。
    pub seeder_served_bytes: u64,
    pub seeder_piece_requests: u64,
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
    // T1 双机：本进程所在主机的对外地址覆盖 nodes.<self>.host；
    // 两端可共用同一份配置，仅 --bind / --self-host 不同。
    if let Some(h) = opt("--self-host") {
        if let Some(n) = cfg.nodes.get_mut(&node_id) { n.host = h; }
    }
    // T1 两端工作目录 / 样本 / 下载器路径可各自不同，按启动参数覆盖共享配置。
    if let Some(w) = opt("--workdir") { cfg.workdir = PathBuf::from(w); }
    if let Some(s) = opt("--sample") { cfg.sample_path = PathBuf::from(s); }
    if let Some(d) = opt("--downloader") { cfg.downloader = PathBuf::from(d); }
    // 中继 PSK：`nat.relay_key`（64 位十六进制或 file:<路径>）。写错就打字说明并**不覆盖**
    // 现有设置——静默退回明文会让「本来加密」的部署以为自己还在加密。
    match cr_nat::relay::set_psk_from_spec(cfg.nat_relay_key.as_deref()) {
        Ok(Some(k)) => eprintln!("[nat] 中继 PSK 已装载，指纹 {}（须与中继端一致）",
                                 cr_nat::secure::fingerprint(&k)),
        Ok(None) => {}
        Err(e) => eprintln!("[nat] nat.relay_key 非法: {}（中继仍按未配置处理）", e),
    }

    let me = match cfg.me() {
        Ok(m) => m.clone(),
        Err(e) => { eprintln!("{}", e); std::process::exit(2); }
    };

    let state_path = cfg.workdir.join(format!("state-{}.json", node_id));
    // 残留进程检查：harness 每次 prepare() 会清空 .run/；这里只记录启动时已存在的 .pid 文件。
    let stale_pids: Vec<String> = fs::read_dir(&cfg.workdir).ok().into_iter().flatten()
        .filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".pid")).collect();

    // UDP socket 在启动线程前绑定一次：M1 的 KAD/唤醒分发与 M2 的 STUN 探测共用它。
    let udp_socket = match UdpSocket::bind(format!("{}:{}", cfg.bind, me.wake_udp_port)) {
        Ok(s) => s,
        Err(e) => { eprintln!("[{}] UDP 绑定失败: {}", node_id, e); std::process::exit(2); }
    };
    let hub = UdpHub {
        socket: Arc::new(udp_socket),
        stun_sink: Arc::new(Mutex::new(None)),
    };

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
        nat: nat_path::NatContext::from_config(&cfg),
        hub: hub.clone(),
        rpc_tasks: Vec::new(),
        engine: Arc::new(Mutex::new(None)),
        seeder_served_bytes: 0,
        seeder_piece_requests: 0,
    };
    if !st.stale_pids.is_empty() {
        println!("[{}] 启动时发现残留 .pid 文件: {:?}", node_id, st.stale_pids);
    }
    // M2/W3.4：nat 段启用且配置了中继时，本节点作为持有者订阅中继帧。
    // nat 缺省时完全不启动，T0/T1 行为逐字节不变。
    relay_sub::spawn(&cfg);

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
        spawn_udp(hub.clone(), cfg.clone(), shared.clone());
        spawn_http(cfg.clone(), me.http_port, shared);
        loop { thread::sleep(Duration::from_secs(3600)); }
    } else {
        let shared = Arc::new(Mutex::new(st));
        spawn_udp(hub.clone(), cfg.clone(), shared.clone());
        spawn_http(cfg.clone(), me.http_port, shared);
        loop { thread::sleep(Duration::from_secs(3600)); }
    }
}

fn bootstrap_holder(cfg: &Config, st: &mut NodeState) -> Result<(), String> {
    let facts = inspect_file(&cfg.sample_path)?;
    let announce = format!("http://{}:{}/announce", cfg.host(&cfg.node_id),
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
    let requester_ip = stream.peer_addr().ok()
        .map(|a| a.ip().to_string()).unwrap_or_default();
    let req = match read_http(&mut stream) {
        Ok(r) => r,
        Err(_) => return,
    };
    let (method, path, body) = req;
    let mut cors = false;
    let response: (String, Vec<u8>, &'static str) = if path.starts_with("/health") {
        // T1 双机取证：host / machine_id / pid 均取自运行环境，不接受配置伪造。
        let machine = env::var("COMPUTERNAME").or_else(|_| env::var("HOSTNAME"))
            .unwrap_or_else(|_| "unknown".to_string());
        ("200 OK".into(),
         Json::obj(vec![
             ("status", Json::s("ok")),
             ("node", Json::s(&cfg.node_id)),
             ("host", Json::s(cfg.me_host())),
             ("machine_id", Json::s(&machine)),
             ("pid", Json::n(std::process::id() as f64)),
         ]).dump().into_bytes(), "application/json")
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
    } else if path == "/nat/advertise" && method == "GET" {
        ("200 OK".into(),
         nat_path::advertisement_json(&nat_path::self_advertisement(&state, &cfg))
             .dump().into_bytes(), "application/json")
    } else if path.starts_with("/announce") {
        let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
        // M1 口径（nat 关闭）：仍走原 tracker_response，字节级不变。
        // M2 口径：按请求方 IP 分流，公网请求方且无真实公网 BT 映射时返回空 peer 列表，
        // 绝不把 RFC1918 私网地址发给公网对端，也不捏造占位地址。
        let body = if state.lock().unwrap().nat.enabled {
            let target = nat_path::tracker_target(&state, &cfg, &requester_ip);
            bt::tracker_response_optional(query, target.as_ref()
                .map(|(ip, port)| (ip.as_str(), *port)))
        } else {
            let port = cfg.me().map(|m| m.bt_port).unwrap_or(0);
            bt::tracker_response(query, cfg.host("A"), port)
        };
        ("200 OK".into(), body, "application/x-bittorrent")
    } else if path == "/jsonrpc" && method == "POST" {
        cors = true;
        let out = handle_jsonrpc(&state, &body);
        ("200 OK".into(), out.dump().into_bytes(), "application/json")
    } else if path == "/jsonrpc" && method == "OPTIONS" {
        cors = true;
        ("204 No Content".into(), Vec::new(), "text/plain")
    } else {
        ("404 Not Found".into(), b"not found".to_vec(), "text/plain")
    };
    if cors { write_http_cors(&mut stream, &response.0, response.2, &response.1) }
    else { write_http(&mut stream, &response.0, response.2, &response.1) }
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

/// 取 `aria2.addUri` 的第一个 URI。
///
/// aria2 的签名是 `addUri(secret, uris, options)`；本进程没有 RPC secret，调用方
/// （AriaNg / 浏览器插件 / 验收脚本）常直接发 `params: [[uri]]`。两种位置都接受——
/// `tellStatus` 当初只认一个下标，结果任务明明在队列里却回「任务不存在」。
fn first_uri_param(params: &[Json]) -> String {
    (0..params.len().min(2))
        .find_map(|i| {
            params
                .get(i)
                .and_then(Json::as_arr)
                .and_then(|a| a.first())
                .and_then(Json::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// aria2 兼容 JSON-RPC 子集（M3/W6.1 + M2/W5.1）。
///
/// `addUri` 收到可解析的 `.torrent` 时会把任务**真的**交给 `cr-engine`（librqbit）下载；
/// 引擎创建失败、任务接入失败都如实写进 `coldrevive.note` / `coldrevive.error`，任务状态回到
/// `waiting`（不假装在传）。`tellStatus` / `tellActive` / `tellWaiting` / `tellStopped` 每次都会
/// 从引擎**重新采样**，不缓存、不猜。
fn handle_jsonrpc(state: &Arc<Mutex<NodeState>>, body: &[u8]) -> Json {
    let rpc_err = |id: Json, code: i64, msg: &str| Json::obj(vec![
        ("jsonrpc", Json::s("2.0")), ("id", id),
        ("error", Json::obj(vec![("code", Json::n(code as f64)), ("message", Json::s(msg))])),
    ]);
    let req = match Json::parse(&String::from_utf8_lossy(body)) {
        Ok(j) => j,
        Err(e) => return rpc_err(Json::Null, -32700, &format!("parse: {e}")),
    };
    let id = req.get("id").cloned().unwrap_or(Json::Null);
    let method = req.get("method").and_then(Json::as_str).unwrap_or("");
    let params = req.get("params").and_then(Json::as_arr).unwrap_or(&[]);
    let arg = |i: usize| params.get(i).cloned().unwrap_or(Json::Null);
    let ok = |id: &Json, result: Json| Json::obj(vec![
        ("jsonrpc", Json::s("2.0")), ("id", id.clone()), ("result", result),
    ]);
    match method {
        "aria2.addUri" => {
            let uri = first_uri_param(params);
            if uri.is_empty() { return rpc_err(id, -32602, "addUri 需要至少一个 URI"); }
            // `.torrent` 链接就真的取回来解析：任务带着真实 info_hash / 长度 / 片数。
            // 解析成功才谈得上接引擎；解析失败如实记录，任务停在 waiting。
            let (parsed, fetched, mut note): (Option<Torrent>, Option<Vec<u8>>, String) =
                if uri.starts_with("http://") {
                    match http_fetch(&uri, Duration::from_secs(10)) {
                        Ok(body) => match cr_torrent::parse(&body) {
                            Ok(t) => (Some(t), Some(body), String::new()),
                            Err(e) => (None, None, format!("取回成功但解析失败: {e}")),
                        },
                        Err(e) => (None, None, format!("取回 .torrent 失败: {e}")),
                    }
                } else if uri.starts_with("https://") {
                    (None, None, "https:// 需要 TLS，本进程的最小 HTTP 客户端不支持；任务未解析".to_string())
                } else {
                    (None, None, "非 http(s) 的 .torrent 链接：只登记 URI，未解析".to_string())
                };

            // 引擎在 NodeState 锁之外操作：创建会话 / 读盘校验可能耗时数百毫秒，
            // 不能把 /health、KAD 分发一起堵住。
            let (slot, engine_dir, engine_listen) = {
                let st = state.lock().unwrap();
                (st.engine.clone(), st.cfg.engine_dir.clone(), st.cfg.engine_listen)
            };
            let mut engine_task = None;
            let mut error = String::new();
            if let Some(bytes) = fetched.as_ref() {
                let name = parsed.as_ref().map(|t| t.name.clone()).unwrap_or_default();
                match attach_to_engine(&slot, &engine_dir, engine_listen, &name, bytes) {
                    Ok(tid) => {
                        engine_task = Some(tid);
                        note = format!(
                            "已接入 BT 引擎（librqbit），下载目录 {}",
                            engine_dir.display()
                        );
                    }
                    Err(e) => {
                        error = format!("引擎接入失败: {e}");
                        note = error.clone();
                    }
                }
            }

            let mut st = state.lock().unwrap();
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
            let gid = format!("{:x}{:04x}", now.as_millis() as u64, st.rpc_tasks.len());
            let output_path = match (&parsed, engine_task.is_some()) {
                (Some(t), true) => engine_dir.join(&t.name).to_string_lossy().into_owned(),
                (Some(t), false) => t.name.clone(),
                (None, _) => String::new(),
            };
            st.rpc_tasks.push(RpcTask {
                gid: gid.clone(), uri: uri.to_string(),
                // 只有接入引擎才会立即变成 active；否则如实停在 waiting。
                status: if engine_task.is_some() { "active".into() } else { "waiting".into() },
                created_unix: now.as_secs(),
                info_hash: parsed.as_ref().map(|t| t.info_hash.clone()).unwrap_or_default(),
                name: parsed.as_ref().map(|t| t.name.clone()).unwrap_or_default(),
                total_length: parsed.as_ref().map(|t| t.length as u64).unwrap_or(0),
                piece_count: parsed.as_ref().map(|t| t.pieces.len()).unwrap_or(0),
                torrent_parsed: parsed.is_some(),
                note,
                engine_task,
                output_path,
                error,
                completed_bytes: 0, uploaded_bytes: 0, peers: 0,
                sample_bytes: 0, sample_unix_ms: now.as_millis() as u64, speed_bps: 0,
            });
            ok(&id, Json::s(gid))
        }
        "aria2.tellStatus" => {
            let gid_param = arg(1);
            // aria2 的 tellStatus 签名是 tellStatus(gid, keys)，gid 在 params[0]；
            // 早先这里错取 params[1]，导致任务明明在队列里却回「任务不存在」。
            // 两种位置都接受，避免调用方按不同习惯写就查不到。
            let first_param = arg(0);
            let gid = gid_param
                .as_str()
                .or_else(|| first_param.as_str())
                .unwrap_or("");
            let (found, slot) = {
                let st = state.lock().unwrap();
                (st.rpc_tasks.iter().find(|t| t.gid == gid).cloned(), st.engine.clone())
            };
            match found {
                Some(mut t) => {
                    refresh_task_progress(&slot, &mut t);
                    // 采样后的状态写回，tellActive/Waiting/Stopped 才不会各说各话。
                    {
                        let mut st = state.lock().unwrap();
                        if let Some(slot_task) = st.rpc_tasks.iter_mut().find(|x| x.gid == gid) {
                            *slot_task = t.clone();
                        }
                    }
                    ok(&id, rpc_task_json(&t))
                }
                None => rpc_err(id, -32001, "任务不存在"),
            }
        }
        "aria2.tellActive" | "aria2.tellWaiting" | "aria2.tellStopped" => {
            // 先全部刷新一遍：否则 tellActive 永远看不到引擎刚推进的任务。
            let slot = { state.lock().unwrap().engine.clone() };
            let mut lists: Vec<RpcTask> = {
                let st = state.lock().unwrap();
                st.rpc_tasks.clone()
            };
            for t in lists.iter_mut() { refresh_task_progress(&slot, t); }
            {
                let mut st = state.lock().unwrap();
                for t in lists.iter() {
                    if let Some(dst) = st.rpc_tasks.iter_mut().find(|x| x.gid == t.gid) {
                        *dst = t.clone();
                    }
                }
            }
            let want = if method.ends_with("tellActive") { "active" }
                       else if method.ends_with("tellWaiting") { "waiting" }
                       else { "stopped" };
            let list: Vec<Json> = lists.iter()
                .filter(|t| match want {
                    // aria2 的 paused 属于 tellWaiting，不属于 tellActive。
                    "active" => t.status == "active",
                    "waiting" => t.status == "waiting" || t.status == "paused",
                    _ => matches!(t.status.as_str(), "complete" | "error" | "removed"),
                })
                .map(rpc_task_json).collect();
            ok(&id, Json::arr(list))
        }
        "aria2.getGlobalStat" => {
            let slot = { state.lock().unwrap().engine.clone() };
            let mut tasks: Vec<RpcTask> = { state.lock().unwrap().rpc_tasks.clone() };
            for t in tasks.iter_mut() { refresh_task_progress(&slot, t); }
            let active = tasks.iter().filter(|t| t.status == "active").count();
            let waiting = tasks.iter().filter(|t| t.status == "waiting").count();
            let stopped = tasks.iter()
                .filter(|t| matches!(t.status.as_str(), "complete" | "error" | "removed"))
                .count();
            let speed: u64 = tasks.iter().filter(|t| t.status == "active").map(|t| t.speed_bps).sum();
            let upload: u64 = 0; // 引擎未对下行任务报上传速率，不编造。
            ok(&id, Json::obj(vec![
                ("downloadSpeed", Json::s(speed.to_string())),
                ("uploadSpeed", Json::s(upload.to_string())),
                ("numActive", Json::s(active.to_string())),
                ("numWaiting", Json::s(waiting.to_string())),
                ("numStopped", Json::s(stopped.to_string())),
                ("numStoppedTotal", Json::s(stopped.to_string())),
            ]))
        }
        "aria2.getVersion" => ok(&id, Json::obj(vec![
            ("version", Json::s("ColdRevive/2.4.68 (aria2-compatible subset)")),
            ("enabledFeatures", Json::arr(vec![Json::s("BitTorrent"), Json::s("JSON-RPC")])),
        ])),
        _ => rpc_err(id, -32601, &format!("不支持的方法 {method}")),
    }
}

/// 惰性创建引擎并把一个 `.torrent` 交给它。
///
/// 失败原因（目录不可写、运行时起不来、种子字节非法）**原样上抛**，由调用方记进任务里。
fn attach_to_engine(
    slot: &SharedEngine,
    dir: &Path,
    listen: Option<std::net::SocketAddr>,
    name: &str,
    bytes: &[u8],
) -> Result<u64, String> {
    let mut guard = slot.lock().unwrap();
    if guard.is_none() {
        let mut opts = EngineOptions::new(dir);
        opts.listen = listen;
        let e = RqbitEngine::new(opts)?;
        match e.listen_addr() {
            Some(a) => println!("[engine] BT 引擎已创建，监听 {}（下载目录 {}）", a, dir.display()),
            None => println!("[engine] BT 引擎已创建，不监听入站（下载目录 {}）", dir.display()),
        }
        *guard = Some(e);
    }
    let engine = guard.as_mut().expect("刚创建的引擎不会消失");
    let spec = TorrentSpec::new(name, bytes);
    engine.add_torrent(&spec)
}

fn now_unix_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

/// 从引擎取一次真实采样，刷新任务的状态与计数器，并据「两次采样之差」算实测速率。
fn refresh_task_progress(slot: &SharedEngine, t: &mut RpcTask) {
    let Some(tid) = t.engine_task else { return };
    let sample = {
        let guard = slot.lock().unwrap();
        match guard.as_ref() {
            Some(e) => e.progress(tid),
            None => None,
        }
    };
    let Some(p) = sample else {
        // 引擎对象不在了却还有 engine_task：这是真的异常，明说，不装成没任务。
        t.status = "error".into();
        t.error = format!("引擎中已找不到任务 {tid}");
        return;
    };
    if p.total_bytes > 0 { t.total_length = p.total_bytes; }
    t.completed_bytes = p.downloaded_bytes;
    t.uploaded_bytes = p.uploaded_bytes;
    t.peers = p.peers;
    let now = now_unix_ms();
    if now > t.sample_unix_ms && p.downloaded_bytes >= t.sample_bytes {
        let db = p.downloaded_bytes - t.sample_bytes;
        let dt = now - t.sample_unix_ms;
        // 字节/秒，取整数；短间隔抖动如实反映，不做平滑。
        t.speed_bps = ((db as u128) * 1000 / (dt as u128)) as u64;
        t.sample_bytes = p.downloaded_bytes;
        t.sample_unix_ms = now;
    }
    t.status = if p.finished {
        "complete".to_string()
    } else {
        match p.state.as_str() {
            "hot" | "waking" => "active",
            "cold" => "paused",
            "failed" => "error",
            _ => "removed",
        }.to_string()
    };
    if let Some(err) = p.error { t.error = err; }
}

fn rpc_task_json(t: &RpcTask) -> Json {
    let total = t.total_length.to_string();
    let done = t.completed_bytes.to_string();
    let path = if !t.output_path.is_empty() { t.output_path.clone() }
               else if !t.name.is_empty() { t.name.clone() }
               else { t.uri.clone() };
    let mut fields = vec![
        ("gid", Json::s(&t.gid)), ("status", Json::s(&t.status)),
        // 长度/进度/速率全部来自引擎真实采样；没接引擎就是 0，不猜。
        ("totalLength", Json::s(&total)), ("completedLength", Json::s(&done)),
        ("uploadLength", Json::s(&t.uploaded_bytes.to_string())),
        ("downloadSpeed", Json::s(&t.speed_bps.to_string())),
        ("uploadSpeed", Json::s("0")),
        ("connections", Json::s(&t.peers.to_string())),
        ("files", Json::arr(vec![Json::obj(vec![
            ("index", Json::s("1")), ("path", Json::s(&path)),
            ("length", Json::s(&total)), ("completedLength", Json::s(&done)),
            ("selected", Json::b(true)),
            ("uris", Json::arr(vec![Json::obj(vec![
                ("uri", Json::s(&t.uri)), ("status", Json::s("used")),
            ])])),
        ])])),
        ("coldrevive.torrentParsed", Json::b(t.torrent_parsed)),
        ("coldrevive.pieceCount", Json::s(t.piece_count.to_string())),
        ("coldrevive.engineAttached", Json::b(t.engine_task.is_some())),
        ("coldrevive.engineTask", Json::s(match t.engine_task {
            Some(v) => v.to_string(), None => String::new(),
        })),
        ("coldrevive.note", Json::s(&t.note)),
    ];
    if !t.error.is_empty() {
        fields.push(("errorMessage", Json::s(&t.error)));
        fields.push(("coldrevive.error", Json::s(&t.error)));
    }
    if !t.info_hash.is_empty() {
        fields.push(("bittorrent", Json::obj(vec![("infoHash", Json::s(&t.info_hash))])));
    }
    Json::obj(fields)
}

/// 最小 HTTP/1.1 GET，只用于取回 `.torrent`（RFC1918 / 回环的取证环境）。
///
/// 不引入第三方 HTTP 客户端是刻意的：这里只需要「发一个 GET、读到 EOF、取 body」。
/// 只支持 `http://`（取证服务器是明文 HTTP）；`https://` 会明确报错而不是静默降级。
/// 不支持 chunked 编码时如实报错，解析失败由调用方记录，绝不假装拿到完整 body。
fn http_fetch(url: &str, timeout: Duration) -> Result<Vec<u8>, String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| "只支持 http:// 链接（https:// 需要 TLS，本进程未实现）".to_string())?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return Err("URL 缺少主机名".into());
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().map_err(|_| format!("端口非法: {p}"))?),
        None => (authority, 80u16),
    };
    let addr = format!("{host}:{port}");
    let mut stream = TcpStream::connect(&addr).map_err(|e| format!("connect {addr}: {e}"))?;
    stream.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).map_err(|e| format!("write request: {e}"))?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).map_err(|e| format!("read response: {e}"))?;
    let split = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "响应缺少头部分隔".to_string())?;
    let head = String::from_utf8_lossy(&buf[..split]).to_string();
    let body = buf[split + 4..].to_vec();
    let status_line = head.lines().next().unwrap_or("").to_string();
    if !status_line.contains(" 200") {
        return Err(format!("HTTP 状态非 200: {status_line}"));
    }
    if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        return Err("响应使用 chunked 编码，本最小客户端未实现".into());
    }
    Ok(body)
}

fn write_http_cors(stream: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) {
    // 只给本地插件/前端跨源读取；生产部署须按 ADR-008 收紧为显式白名单。
    let head = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type\r\n\
         Connection: close\r\n\r\n",
        status, ctype, body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
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

fn spawn_udp(hub: UdpHub, cfg: Config, state: Arc<Mutex<NodeState>>) {
    thread::spawn(move || {
        let sock = &hub.socket;
        println!("[{}] UDP 监听 {}:{}", cfg.node_id, cfg.bind,
                 sock.local_addr().map(|a| a.port()).unwrap_or(0));
        let mut buf = [0u8; 65535];
        // W3.6：回打限流（该帧无需鉴权，不设上限等于把唤醒端口变成放大器）
        let mut back_rate = cr_nat::holepunch::BackRate::new();
        loop {
            let (n, src) = match sock.recv_from(&mut buf) { Ok(v) => v, Err(_) => continue };
            // M2: STUN Binding 响应交给正在进行的 nat probe（复用同一监听 socket）
            if nat_path::is_stun_datagram(&buf[..n]) {
                if let Some(tx) = hub.stun_sink.lock().unwrap().as_ref() {
                    let _ = tx.send(buf[..n].to_vec());
                }
                continue;
            }
            let frame = match Json::parse(&String::from_utf8_lossy(&buf[..n])) {
                Ok(j) => j, Err(_) => continue,
            };
            // M2: 打洞帧从监听 socket 回 ACK —— 真实回包打开/维持本侧 NAT 映射。
            // 该帧不属于 PROTO v1 golden，不改变 WAKEUP_REQ/ACK 字段集合。
            let frame_type = frame.get("t").and_then(Json::as_str).unwrap_or("");
            if frame_type == cr_nat::holepunch::PUNCH_TYPE {
                let token = frame.get("token").and_then(Json::as_str).unwrap_or("");
                // W3.6 双向打洞：请求侧把自身映射候选放在 back 里（可能经中继送达），
                // 持有者据此**主动回打**，在自身网关上开出过滤项。否则受限锥及以上
                // 网关会把请求侧的首个入向包当 unsolicited 丢弃（W3.5② 实测 direct 0/30）。
                let back = cr_nat::holepunch::punch_back(&buf[..n]);
                if !back.is_empty() && back_rate.allow() {
                    let sent = cr_nat::holepunch::punch_back_to(
                        sock, &back, &cfg.node_id, token,
                        cr_nat::holepunch::PUNCH_BACK_FRAMES);
                    if sent > 0 {
                        println!("[{}] punch-back → {} 个候选，真实发出 {} 帧",
                                 cfg.node_id, back.len(), sent);
                    }
                }
                let _ = sock.send_to(
                    &cr_nat::holepunch::punch_ack(&cfg.node_id, token), src);
                continue;
            }
            if let Some(r) = handle_udp_frame(&cfg, &state, &frame) {
                let _ = sock.send_to(r.dump().as_bytes(), src);
            }
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
        let url = format!("http://{}:{}/torrent/{}", cfg.host(&cfg.node_id),
                          me.http_port, entry.infohash);
        return Some(build_ack(&trace, &entry.infohash, &url, entry.size,
                              entry.piece_size, me.bt_port));
    }
    None
}

#[cfg(test)]
mod rpc_tests {
    use super::*;

    /// 起一个只服务一次的极简 HTTP 服务，把给定 body 作为 200 响应发出去。
    fn serve_once(body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/x-bittorrent\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&body);
                let _ = s.flush();
            }
        });
        format!("http://127.0.0.1:{}/sample.torrent", addr.port())
    }

    #[test]
    fn adduri_accepts_both_param_layouts() {
        // 带 secret：params = [token, [uri]]；不带 secret：params = [[uri]]。
        // 只认一种就会让另一种调用方收到「需要至少一个 URI」。
        let with_secret = vec![Json::s("token"), Json::arr(vec![Json::s("http://h/a.torrent")])];
        let without_secret = vec![Json::arr(vec![Json::s("http://h/a.torrent")])];
        assert_eq!(first_uri_param(&with_secret), "http://h/a.torrent");
        assert_eq!(first_uri_param(&without_secret), "http://h/a.torrent");
        assert_eq!(first_uri_param(&[]), "");
        // 只有 options、没有 uris 时必须返回空，而不是把对象当成 URI。
        assert_eq!(first_uri_param(&[Json::obj(vec![("dir", Json::s("/tmp"))])]), "");
    }

    #[test]
    fn adduri_fetches_and_parses_real_torrent() {
        let data = vec![7u8; 40_000];
        let torrent = cr_torrent::generate(&data, "rpc-sample.bin", 16_384, "http://127.0.0.1:1/announce");
        let url = serve_once(torrent.raw.clone());

        let fetched = http_fetch(&url, Duration::from_secs(5)).expect("fetch");
        assert_eq!(fetched, torrent.raw, "取回的字节必须与发出的种子完全一致");

        let parsed = cr_torrent::parse(&fetched).expect("parse");
        assert_eq!(parsed.info_hash, torrent.info_hash);
        assert_eq!(parsed.length, 40_000);
        assert_eq!(parsed.name, "rpc-sample.bin");
        assert_eq!(parsed.pieces.len(), 3, "40000 / 16384 向上取整 = 3 片");
    }

    #[test]
    fn https_is_rejected_instead_of_silently_downgraded() {
        let err = http_fetch("https://127.0.0.1:1/x.torrent", Duration::from_millis(200))
            .expect_err("https 必须报错");
        assert!(err.contains("http://"), "错误信息要说清楚只支持 http://: {err}");
    }

    #[test]
    fn non_200_is_an_error_not_an_empty_torrent() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 512];
                let _ = s.read(&mut buf);
                let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                let _ = s.flush();
            }
        });
        let err = http_fetch(&format!("http://127.0.0.1:{}/missing.torrent", addr.port()), Duration::from_secs(3))
            .expect_err("404 必须报错");
        assert!(err.contains("404"), "错误信息要带上状态行: {err}");
    }
}
