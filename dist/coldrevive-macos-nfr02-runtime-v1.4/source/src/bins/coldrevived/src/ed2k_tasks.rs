//! FR-25 守护进程接线：把 `coldrevive-ed2k` 的 link/download/serve 三条命令
//! 以真实后台任务的形式接到 `coldrevived` 的 `/cli` 命令面上。
//!
//! 边界照写：
//! - 三条命令都调用生产 `cr_ed2k::stack`，不复制第二份状态机。
//! - `download` 的 fileID 仍必须由本地参考文件的真实 `ed2k_hash` 导出；
//!   从 ed2k:// 链接抄 hash、无参考文件的“盲下载”没有实现，也不谎称实现。
//! - 任务状态来自 `stack::{download, serve}` 的真实返回值；没有连接/超时就是
//!   `error` 并带真实错误，不会凭空写 `complete`。
//! - 不做 server login / source exchange / credit / AICH（ADR-011 L2 排除项）。

use crate::Json;
use cr_core::unix_ts;
use cr_ed2k::kad::{Tag as KadTag, TagValue as KadTagValue};
use cr_ed2k::stack::{download, serve_with_config, ServeConfig, SharedFile, UploadQueue};
use cr_ed2k::wire::{ct, Hello};
use cr_hash::{hex, md4};
use std::fs;
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// 一条 daemon eD2k 任务。`result` 只在栈真正返回后写入，否则保留 None。
#[derive(Clone)]
pub struct Ed2kTask {
    pub id: String,
    pub kind: &'static str,
    pub status: String,
    pub created_unix: i64,
    pub file: String,
    pub host: String,
    pub port: u16,
    pub result: Option<Json>,
    pub error: String,
}

pub type SharedEd2kTasks = Arc<Mutex<Vec<Ed2kTask>>>;

/// 一次 `serve` 任务的跨连接聚合状态。真实 aMule 对 >= PARTSIZE 的文件会
/// 同时开多条 TCP 连接取片；每条连接一个线程，只有这里能决定任务何时结束。
struct ServeAggregate {
    connections: usize,
    active: usize,
    bytes_sent: usize,
    blocks_sent: usize,
    idle_resends: usize,
    deferred_bytes: usize,
    max_in_flight_peak: usize,
    peer: String,
    peer_hello: bool,
    queue_granted: bool,
    log_tail: Vec<Json>,
    errors: Vec<String>,
    last_conn: Instant,
    /// 供任务收尾时主动 shutdown 的 socket 副本；排队连接可能一直不关闭。
    streams: Vec<TcpStream>,
}

impl ServeAggregate {
    fn new() -> Self {
        ServeAggregate {
            connections: 0,
            active: 0,
            bytes_sent: 0,
            blocks_sent: 0,
            idle_resends: 0,
            deferred_bytes: 0,
            max_in_flight_peak: 0,
            peer: String::new(),
            peer_hello: false,
            queue_granted: false,
            log_tail: Vec::new(),
            errors: Vec::new(),
            last_conn: Instant::now(),
            streams: Vec::new(),
        }
    }
}

pub fn registry() -> SharedEd2kTasks {
    Arc::new(Mutex::new(Vec::new()))
}

fn opt(args: &[String], key: &str) -> Option<String> {
    args.iter().position(|a| a == key).and_then(|i| args.get(i + 1)).cloned()
}

fn name_from(path: &str, explicit: Option<String>) -> String {
    explicit.filter(|s| !s.is_empty()).unwrap_or_else(|| {
        Path::new(path).file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "sample.bin".into())
    })
}

fn hello(port: u16) -> Hello {
    Hello {
        user_hash: md4(b"coldrevive-fr25-daemon"),
        client_id: 0,
        tcp_port: port,
        tags: vec![
            KadTag { name: vec![ct::NAME], value: KadTagValue::Str("coldrevive-fr25".into()) },
            KadTag { name: vec![ct::VERSION], value: KadTagValue::U32(0x3c) },
        ],
        server_ip: 0,
        server_port: 0,
    }
}

pub fn task_json(t: &Ed2kTask) -> Json {
    Json::obj(vec![
        ("schema", Json::s("coldrevive.fr25.daemon-task/v1")),
        ("id", Json::s(&t.id)),
        ("kind", Json::s(t.kind)),
        ("status", Json::s(&t.status)),
        ("created_unix", Json::n(t.created_unix as f64)),
        ("file", Json::s(&t.file)),
        ("host", Json::s(&t.host)),
        ("port", Json::n(t.port as f64)),
        ("result", t.result.clone().unwrap_or(Json::Null)),
        ("error", Json::s(&t.error)),
    ])
}

fn push_task(tasks: &SharedEd2kTasks, task: Ed2kTask) -> String {
    let id = task.id.clone();
    tasks.lock().unwrap().push(task);
    id
}

fn finish(tasks: &SharedEd2kTasks, id: &str, result: Option<Json>, error: String) {
    if let Some(t) = tasks.lock().unwrap().iter_mut().find(|t| t.id == id) {
        t.result = result;
        t.error = error.clone();
        t.status = if error.is_empty() { "complete".into() } else { "error".into() };
    }
}

fn next_id(tasks: &SharedEd2kTasks, kind: &str) -> String {
    let n = tasks.lock().unwrap().len();
    format!("ed2k-{}-{kind}-{n:04x}", unix_ts())
}

/// `ed2k-link <file> [--name NAME]`：同步命令，fileID 从真实文件内容导出。
pub fn link_cmd(args: &[String]) -> Json {
    let path = match opt(args, "--file").or_else(|| args.first().cloned()) {
        Some(p) if !p.is_empty() => p,
        _ => return Json::obj(vec![("error", Json::s("用法: ed2k-link <file> [--name NAME]"))]),
    };
    let data = match fs::read(&path) {
        Ok(b) => b,
        Err(e) => return Json::obj(vec![("error", Json::s(format!("读取 {path}: {e}")))]),
    };
    let shared = SharedFile::new(name_from(&path, opt(args, "--name")), data);
    Json::obj(vec![
        ("schema", Json::s("coldrevive.fr25.link/v1")),
        ("file", Json::s(&path)),
        ("name", Json::s(&shared.name)),
        ("size", Json::n(shared.data.len() as f64)),
        ("file_id", Json::s(hex(&shared.hash()))),
        ("ed2k_link", Json::s(shared.link())),
        ("hashset_required", Json::b(shared.needs_hashset())),
    ])
}

/// `ed2k-serve <file> [--port 4663] [--timeout-ms 120000] [--name NAME]
///   [--max-in-flight-bytes N | --in-flight-unlimited] [--request-gated-pacing]
///   [--compressed-parts]`
///
/// 监听与 accept 之前的步骤同步完成（端口冲突当场报错）；真正的单次供片在后台线程。
pub fn serve_cmd(tasks: &SharedEd2kTasks, args: &[String]) -> Json {
    let path = match opt(args, "--file").or_else(|| args.first().cloned()) {
        Some(p) if !p.is_empty() => p,
        _ => return Json::obj(vec![("error", Json::s("用法: ed2k-serve <file> [--port N] [--timeout-ms MS]"))]),
    };
    let port: u16 = opt(args, "--port").and_then(|s| s.parse().ok()).unwrap_or(4663);
    let timeout_ms: u64 = opt(args, "--timeout-ms").and_then(|s| s.parse().ok()).unwrap_or(120_000);
    let mut serve_config = ServeConfig::default();
    if let Some(n) = opt(args, "--max-in-flight-bytes").and_then(|s| s.parse::<usize>().ok()) {
        serve_config.max_in_flight_bytes = Some(n);
    }
    if args.iter().any(|a| a == "--in-flight-unlimited") {
        serve_config.max_in_flight_bytes = None;
    }
    // Opt-in 假说：每请求回合只送一个块，不主动 idle 重传；未对真 aMule 验证。
    serve_config.request_gated_pacing = args.iter().any(|a| a == "--request-gated-pacing");
    // FR-25 反向 A/B：stock aMule 同构的 0x40/0xC5 每块一帧（std-only zlib）。
    serve_config.compressed_parts = args.iter().any(|a| a == "--compressed-parts");
    let data = match fs::read(&path) {
        Ok(b) => b,
        Err(e) => return Json::obj(vec![("error", Json::s(format!("读取 {path}: {e}")))]),
    };
    let shared = SharedFile::new(name_from(&path, opt(args, "--name")), data);
    let listener = match TcpListener::bind(("0.0.0.0", port)) {
        Ok(l) => l,
        Err(e) => return Json::obj(vec![("error", Json::s(format!("监听 {port}: {e}")))]),
    };
    let id = next_id(tasks, "serve");
    let task = Ed2kTask {
        id: id.clone(), kind: "serve", status: "running".into(),
        created_unix: unix_ts(), file: path.clone(), host: "0.0.0.0".into(), port,
        result: None, error: String::new(),
    };
    push_task(tasks, task);
    let tasks2 = tasks.clone();
    let id2 = id.clone();
    let link = shared.link();
    let file_size = shared.data.len();
    thread::spawn(move || {
        // 真实 aMule 对 >= PARTSIZE 的文件会同时开多条 TCP 连接取片。之前在
        // 同一线程里 accept -> serve -> 再 accept：第一条连接还没结束时第二条
        // 只能排队，对端停在 17.1%（9 个 BLOCKSIZE）后 Waiting。这里改成每条
        // 连接独立线程，共享同一个 UploadQueue 和聚合状态。
        if let Err(e) = listener.set_nonblocking(true) {
            finish(&tasks2, &id2, None, format!("set_nonblocking: {e}"));
            return;
        }
        let queue: Arc<Mutex<UploadQueue>> = Arc::new(Mutex::new(UploadQueue::new(2)));
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let agg: Arc<Mutex<ServeAggregate>> = Arc::new(Mutex::new(ServeAggregate::new()));
        let mut handles = Vec::new();
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((stream, peer)) => {
                    // Windows 上 accept 到的 socket 会继承监听套接字的 nonblocking；
                    // 必须显式改回阻塞，否则 serve 的读超时失效、第一帧就读空退出。
                    if let Err(e) = stream.set_nonblocking(false) {
                        agg.lock().unwrap().errors.push(format!("set_nonblocking(false): {e}"));
                        break;
                    }
                    {
                        let mut g = agg.lock().unwrap();
                        g.connections += 1;
                        g.active += 1;
                        g.peer = peer.to_string();
                        g.last_conn = Instant::now();
                        if let Ok(h) = stream.try_clone() {
                            g.streams.push(h);
                        }
                    }
                    let agg2 = agg.clone();
                    let shared2 = shared.clone();
                    let queue2 = queue.clone();
                    let hello2 = hello(port);
                    let serve_cfg2 = serve_config.clone();
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    handles.push(thread::spawn(move || {
                        let out = serve_with_config(stream, &[shared2], &hello2, &queue2, remaining, serve_cfg2);
                        if let Ok(mut g) = agg2.lock() {
                            g.bytes_sent += out.bytes_sent;
                            g.blocks_sent += out.blocks_sent;
                            g.idle_resends += out.idle_resends;
                            g.deferred_bytes += out.deferred_bytes;
                            g.max_in_flight_peak = g.max_in_flight_peak.max(out.max_in_flight_bytes);
                            g.peer_hello |= out.peer_hello.is_some();
                            g.queue_granted |= out.queue_granted;
                            g.log_tail = out.log.iter().rev().take(12)
                                .map(|e| Json::s(format!("{e:?}"))).collect();
                            if let Some(e) = out.error {
                                g.errors.push(e);
                            }
                            g.active = g.active.saturating_sub(1);
                            g.last_conn = Instant::now();
                        }
                    }));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    let done = {
                        let g = agg.lock().unwrap();
                        g.connections > 0 && g.bytes_sent >= file_size
                            && g.last_conn.elapsed() >= Duration::from_secs(3)
                    };
                    if done {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    agg.lock().unwrap().errors.push(format!("accept: {e}"));
                    break;
                }
            }
        }
        // 排队中的连接可能一直不发 STARTUPLOADREQ，也不会自己关闭。收尾时
        // 主动 shutdown 所有已接受的 socket，避免任务拖到整体 deadline。
        let streams: Vec<TcpStream> = agg.lock().unwrap().streams.drain(..).collect();
        for s in streams {
            let _ = s.shutdown(Shutdown::Both);
        }
        let join_deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < join_deadline && agg.lock().unwrap().active > 0 {
            thread::sleep(Duration::from_millis(50));
        }
        if agg.lock().unwrap().active == 0 {
            for h in handles {
                let _ = h.join();
            }
        } else {
            agg.lock().unwrap().errors.push("connection threads did not exit after shutdown".into());
        }
        let g = agg.lock().unwrap();
        let result = Json::obj(vec![
            ("peer", Json::s(&g.peer)),
            ("connections", Json::n(g.connections as f64)),
            ("peer_hello", Json::b(g.peer_hello)),
            ("queue_granted", Json::b(g.queue_granted)),
            ("blocks_sent", Json::n(g.blocks_sent as f64)),
            ("bytes_sent", Json::n(g.bytes_sent as f64)),
            ("idle_resends", Json::n(g.idle_resends as f64)),
            ("deferred_bytes", Json::n(g.deferred_bytes as f64)),
            ("max_in_flight_peak", Json::n(g.max_in_flight_peak as f64)),
            ("request_gated_pacing", Json::b(serve_config.request_gated_pacing)),
            ("ed2k_link", Json::s(&link)),
            ("log_tail", Json::arr(g.log_tail.clone())),
        ]);
        let errors = g.errors.join("; ");
        finish(&tasks2, &id2, Some(result), errors);
    });
    Json::obj(vec![
        ("schema", Json::s("coldrevive.fr25.daemon-task-started/v1")),
        ("id", Json::s(&id)), ("kind", Json::s("serve")),
        ("file", Json::s(&path)), ("port", Json::n(port as f64)),
    ])
}

/// `ed2k-download <参考文件> --host ip:port [--timeout-ms 60000] [--name NAME]`
///
/// fileID / hashset 都来自本地参考文件的真实内容；缺参考文件就明确报错。
pub fn download_cmd(tasks: &SharedEd2kTasks, args: &[String]) -> Json {
    let path = match opt(args, "--file").or_else(|| args.first().cloned()) {
        Some(p) if !p.is_empty() => p,
        _ => return Json::obj(vec![("error", Json::s("用法: ed2k-download <参考文件> --host ip:port"))]),
    };
    let host = match opt(args, "--host") {
        Some(h) => h,
        None => return Json::obj(vec![("error", Json::s("ed2k-download 缺 --host ip:port"))]),
    };
    let addr: SocketAddr = match host.parse() {
        Ok(a) => a,
        Err(_) => return Json::obj(vec![("error", Json::s(format!("非法 --host {host}")))]),
    };
    let timeout_ms: u64 = opt(args, "--timeout-ms").and_then(|s| s.parse().ok()).unwrap_or(60_000);
    // 公网 A/B 的判据是对端落地 md5；默认不写盘保持旧行为，显式 --out 才落盘。
    let out_path = opt(args, "--out").filter(|p| !p.is_empty());
    let data = match fs::read(&path) {
        Ok(b) => b,
        Err(e) => return Json::obj(vec![("error", Json::s(format!("读取 {path}: {e}")))]),
    };
    let expected = SharedFile::new(name_from(&path, opt(args, "--name")), data);
    let id = next_id(tasks, "download");
    let task = Ed2kTask {
        id: id.clone(), kind: "download", status: "running".into(),
        created_unix: unix_ts(), file: path.clone(), host: addr.to_string(), port: addr.port(),
        result: None, error: String::new(),
    };
    push_task(tasks, task);
    let tasks2 = tasks.clone();
    let id2 = id.clone();
    let file_id = hex(&expected.hash());
    let link = expected.link();
    let size = expected.data.len();
    let out_path2 = out_path.clone();
    thread::spawn(move || {
        let outcome: Result<(Json, String), String> = (|| {
            let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(8))
                .map_err(|e| format!("连接 {addr}: {e}"))?;
            let out = download(stream, &expected, &hello(4662), Duration::from_millis(timeout_ms));
            let err = out.error.clone().unwrap_or_default();
            // 只有 ed2k_hash 与参考字节都一致才落盘；失败时绝不留一个看似完成的假文件。
            let landed = if out.completed {
                match &out_path2 {
                    Some(p) => {
                        let tmp = format!("{p}.part");
                        fs::write(&tmp, &out.data)
                            .map_err(|e| format!("写落地文件 {tmp}: {e}"))?;
                        fs::rename(&tmp, p)
                            .map_err(|e| format!("落地文件改名 {tmp} -> {p}: {e}"))?;
                        Some((p.clone(), out.data.len()))
                    }
                    None => None,
                }
            } else {
                None
            };
            let mut fields = vec![
                ("host", Json::s(addr.to_string())),
                ("file_name", Json::s(&expected.name)),
                ("file_size", Json::n(size as f64)),
                ("file_id", Json::s(&file_id)),
                ("ed2k_link", Json::s(&link)),
                ("peer_hello", Json::b(out.peer_hello.is_some())),
                ("peer_filename", match out.peer_filename { Some(s) => Json::s(s), None => Json::s("") }),
                ("hashset_verified", Json::b(out.hashset_verified)),
                ("queue_rank_seen", match out.queue_rank_seen { Some(r) => Json::n(r as f64), None => Json::s("") }),
                ("bytes_received", Json::n(out.bytes_received as f64)),
                ("blocks_received", Json::n(out.blocks_received as f64)),
                ("hash_matches", Json::b(out.hash_matches)),
                ("content_matches", Json::b(out.content_matches)),
                ("completed", Json::b(out.completed)),
                ("log_tail", Json::arr(out.log.iter().rev().take(12)
                    .map(|e| Json::s(format!("{e:?}"))).collect())),
            ];
            if let Some((p, n)) = landed {
                fields.push(("out_path", Json::s(&p)));
                fields.push(("landed_bytes", Json::n(n as f64)));
            }
            let result = Json::obj(fields);
            Ok((result, err))
        })();
        match outcome {
            Ok((r, err)) => finish(&tasks2, &id2, Some(r), err),
            Err(e) => finish(&tasks2, &id2, None, e),
        }
    });
    Json::obj(vec![
        ("schema", Json::s("coldrevive.fr25.daemon-task-started/v1")),
        ("id", Json::s(&id)), ("kind", Json::s("download")),
        ("file", Json::s(&path)), ("host", Json::s(&addr.to_string())),
    ])
}

pub fn status_cmd(tasks: &SharedEd2kTasks, args: &[String]) -> Json {
    let id = opt(args, "--task").or_else(|| args.first().cloned()).unwrap_or_default();
    let found = tasks.lock().unwrap().iter().find(|t| t.id == id).cloned();
    match found {
        Some(t) => task_json(&t),
        None => Json::obj(vec![("error", Json::s(format!("ed2k 任务不存在: {id}")))]),
    }
}

pub fn list_cmd(tasks: &SharedEd2kTasks) -> Json {
    let all: Vec<Json> = tasks.lock().unwrap().iter().map(task_json).collect();
    Json::obj(vec![
        ("schema", Json::s("coldrevive.fr25.daemon-task-list/v1")),
        ("count", Json::n(all.len() as f64)),
        ("tasks", Json::arr(all)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(n: usize) -> Vec<u8> {
        (0..n).map(|i| ((i * 91 + 7) % 256) as u8).collect()
    }

    #[test]
    fn daemon_serve_download_roundtrip_records_real_result() {
        let dir = std::env::temp_dir().join(format!("cr-fr25-daemon-{}", unix_ts()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sample.bin");
        fs::write(&path, sample(4096)).unwrap();
        let tasks = registry();

        // 用一个空闲端口起 serve，然后在同一进程内用 download 取回。
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let started = serve_cmd(&tasks, &[
            "--file".into(), path.to_string_lossy().into_owned(),
            "--port".into(), port.to_string(),
            "--timeout-ms".into(), "8000".into(),
        ]);
        let sid = started.get("id").and_then(Json::as_str).unwrap().to_string();
        assert!(!sid.is_empty(), "{started:?}");

        let landed = dir.join("landed.bin");
        let dstarted = download_cmd(&tasks, &[
            "--file".into(), path.to_string_lossy().into_owned(),
            "--host".into(), format!("127.0.0.1:{port}"),
            "--timeout-ms".into(), "8000".into(),
            "--out".into(), landed.to_string_lossy().into_owned(),
        ]);
        let did = dstarted.get("id").and_then(Json::as_str).unwrap().to_string();

        let deadline = std::time::Instant::now() + Duration::from_secs(12);
        let (mut sdone, mut ddone) = (false, false);
        let (mut sjson, mut djson) = (Json::Null, Json::Null);
        while std::time::Instant::now() < deadline && !(sdone && ddone) {
            for (id, done, out) in [(&sid, &mut sdone, &mut sjson), (&did, &mut ddone, &mut djson)] {
                let j = status_cmd(&tasks, &[id.clone()]);
                let status = j.get("status").and_then(Json::as_str).unwrap_or("");
                if status == "complete" || status == "error" {
                    *done = true;
                    *out = j;
                }
            }
            if !(sdone && ddone) { thread::sleep(Duration::from_millis(50)); }
        }
        assert!(sdone, "serve 任务未在时限内结束: {sjson:?}");
        assert!(ddone, "download 任务未在时限内结束: {djson:?}");
        let d = djson.get("result").unwrap_or(&Json::Null);
        assert_eq!(d.get("completed").and_then(Json::as_bool), Some(true), "{djson:?}");
        assert_eq!(d.get("hash_matches").and_then(Json::as_bool), Some(true), "{djson:?}");
        assert_eq!(d.get("content_matches").and_then(Json::as_bool), Some(true), "{djson:?}");
        assert_eq!(d.get("bytes_received").and_then(Json::as_i64), Some(4096));
        assert_eq!(d.get("landed_bytes").and_then(Json::as_i64), Some(4096), "{djson:?}");
        assert_eq!(fs::read(&landed).unwrap(), sample(4096), "落地文件必须逐字节一致");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn serve_accepts_parallel_connections_and_reaps_queued_peers() {
        let dir = std::env::temp_dir().join(format!("cr-fr25-parallel-{}", unix_ts()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("parallel.bin");
        let bytes = sample(128 * 1024);
        fs::write(&path, &bytes).unwrap();
        let tasks = registry();

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let started = serve_cmd(&tasks, &[
            "--file".into(), path.to_string_lossy().into_owned(),
            "--port".into(), port.to_string(),
            "--timeout-ms".into(), "7000".into(),
        ]);
        let sid = started.get("id").and_then(Json::as_str).unwrap().to_string();

        // 第一条连接连上但不发握手，模拟真实 aMule 的排队连接。旧的单线程
        // accept->serve 循环会卡在这里，让第二条连接等到超时。
        let held = TcpStream::connect(("127.0.0.1", port)).unwrap();
        thread::sleep(Duration::from_millis(300));

        let dstarted = download_cmd(&tasks, &[
            "--file".into(), path.to_string_lossy().into_owned(),
            "--host".into(), format!("127.0.0.1:{port}"),
            "--timeout-ms".into(), "5000".into(),
        ]);
        let did = dstarted.get("id").and_then(Json::as_str).unwrap().to_string();

        let deadline = Instant::now() + Duration::from_secs(6);
        let mut d = Json::Null;
        while Instant::now() < deadline {
            let j = status_cmd(&tasks, &[did.clone()]);
            if matches!(j.get("status").and_then(Json::as_str), Some("complete") | Some("error")) {
                d = j;
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(d.get("status").and_then(Json::as_str), Some("complete"), "{d:?}");
        let dr = d.get("result").unwrap_or(&Json::Null);
        assert_eq!(dr.get("completed").and_then(Json::as_bool), Some(true), "{d:?}");
        assert_eq!(dr.get("bytes_received").and_then(Json::as_i64), Some(bytes.len() as i64), "{d:?}");
        drop(held);

        // 收尾要主动 shutdown 排队连接，不能把任务拖到 7s 整体 deadline。
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut s = Json::Null;
        while Instant::now() < deadline {
            let j = status_cmd(&tasks, &[sid.clone()]);
            if matches!(j.get("status").and_then(Json::as_str), Some("complete") | Some("error")) {
                s = j;
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(s.get("status").and_then(Json::as_str), Some("complete"), "{s:?}");
        let sr = s.get("result").unwrap_or(&Json::Null);
        assert_eq!(sr.get("connections").and_then(Json::as_i64), Some(2), "{s:?}");
        assert!(sr.get("bytes_sent").and_then(Json::as_i64).unwrap_or(0) >= bytes.len() as i64, "{s:?}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn link_uses_real_content_hash_and_reports_hashset_requirement() {
        let dir = std::env::temp_dir().join(format!("cr-fr25-link-{}", unix_ts()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("small.bin");
        fs::write(&path, b"fr25-daemon-link").unwrap();
        let j = link_cmd(&[path.to_string_lossy().into_owned()]);
        assert_eq!(j.get("file_id").and_then(Json::as_str),
                   Some(hex(&md4(b"fr25-daemon-link")).as_str()));
        assert_eq!(j.get("hashset_required").and_then(Json::as_bool), Some(false));
        fs::remove_dir_all(&dir).ok();
    }
}
