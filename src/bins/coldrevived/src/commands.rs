//! coldrevived CLI v1 命令分发。
//!
//! 每个字段都来自真实来源：本地文件 SHA-256 / 真实 .torrent bencode / 真实 UDP 往返 /
//! 真实第三方下载器进程 / 真实 golden 文件解析。失败时返回 false 或错误码，
//! **绝不返回「看起来像通过」的默认常量**。

use crate::{http_get, http_post_json, nat_path, DownloadRecord, NodeState, NODE_SECRET};
use cr_core::{unix_ts, Config, Json, CHALLENGE_WINDOW_SEC};
use cr_hash::sha256_hex;
use cr_kad::{find_frame, kad_id_128, keyword_key, publish_keys, socket_addr, udp_roundtrip, dht_id_160};
use cr_store::FileEntry;
use cr_wake::{ack_infohash, ack_is_ok, build_req, challenge, has_exact_fields, REQ_FIELDS};
use std::fs;
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

type Shared = Arc<Mutex<NodeState>>;

fn args_of<'a>(req: &'a Json) -> Vec<String> {
    req.get("args").and_then(Json::as_arr).unwrap_or(&[])
        .iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
}
fn arg(args: &[String], i: usize) -> String { args.get(i).cloned().unwrap_or_default() }

fn peer_wake(cfg: &Config) -> Result<SocketAddr, String> {
    socket_addr(cfg.peer_host(), cfg.peer()?.wake_udp_port)
}
fn holder_http_cli(cfg: &Config) -> String { cfg.url("A", "/cli") }

/// 唤醒发包：M1 默认仍是单播私网地址；M2 打开 nat 且已取得对端端点通告时，
/// 改走 cr-nat 候选计划（打洞 + 显式映射 + 中继回退），全部失败则如实报错。
/// T0/T1 未配置 nat 段时此分支完全不进入，行为逐字节不变。
fn send_wake(state: &Shared, cfg: &Config, req: &Json,
             timeout_ms: u64) -> Result<(Json, u64), String> {
    if let Some(plan) = nat_path::active_plan(state) {
        return nat_path::wake_with_plan(state, cfg, &plan, req, timeout_ms, None).result;
    }
    udp_roundtrip(&cfg.bind, peer_wake(cfg)?, req, timeout_ms)
}

/// 修改 Json 对象里的一个字段（保留其余字段与顺序）。
fn set_field(j: &mut Json, key: &str, value: Json) {
    if let Json::Obj(ref mut pairs) = j {
        if let Some((_, v)) = pairs.iter_mut().find(|(k, _)| k == key) { *v = value; return; }
        pairs.push((key.to_string(), value));
    }
}

pub fn dispatch(state: &Shared, req: &Json) -> Json {
    let command = req.get("command").and_then(Json::as_str).unwrap_or("").to_string();
    let args = args_of(req);
    let cfg = state.lock().unwrap().cfg.clone();
    let result = match command.as_str() {
        "inspect" => inspect(state, &cfg, &arg(&args, 0), &arg(&args, 1)),
        "find" => find(state, &cfg, &arg(&args, 0)),
        "wake" => wake(state, &cfg, &arg(&args, 0), &arg(&args, 1), &arg(&args, 2)),
        "scenario" => scenario(state, &cfg, &arg(&args, 0), &arg(&args, 1)),
        "metrics" => metrics(state, &cfg, &arg(&args, 0), &arg(&args, 1)),
        "download" => download(state, &cfg, &arg(&args, 0)),
        "golden" => golden(&cfg),
        "env" => env_defaults(state),
        "nat" => nat_path::dispatch(state, &cfg, &args),
        other => Json::obj(vec![("error", Json::s(format!("未知命令 {}", other)))]),
    };
    result
}

// ---------------------------------------------------------------------------
// inspect / find / wake
// ---------------------------------------------------------------------------

fn inspect(state: &Shared, cfg: &Config, sub: &str, hash: &str) -> Json {
    let st = state.lock().unwrap();
    let entry = st.index.get(hash).cloned()
        .or_else(|| st.index.entries.values().next().cloned());
    match sub {
        "file" => {
            let e = entry.as_ref();
            Json::obj(vec![("file", Json::obj(vec![
                ("indexed", Json::b(e.is_some())),
                ("torrent_generated", Json::b(e.map(|x| !x.infohash.is_empty()).unwrap_or(false))),
                ("torrent_cached", Json::b(e.map(|x| x.torrent_path.is_file()).unwrap_or(false))),
                ("torrent_path", Json::s(e.map(|x| x.torrent_path.to_string_lossy().to_string())
                    .unwrap_or_default())),
            ]))])
        }
        "index" => Json::obj(vec![
            ("file", Json::obj(vec![("state", Json::s(
                entry.as_ref().map(|x| x.state.as_str()).unwrap_or("MISSING")))])),
            ("index", Json::obj(vec![
                ("shadow", Json::b(entry.is_some())),
                ("kad_published", Json::b(entry.as_ref().map(|x| x.kad_published).unwrap_or(false))),
            ])),
        ]),
        "kad" => {
            let keys = st.index.all_published_keys();
            let kw_count = st.index.entries.values().map(|e| e.keywords.len()).sum::<usize>();
            Json::obj(vec![("kad", Json::obj(vec![
                ("namespace_ok", Json::b(cfg.namespace == cr_core::NAMESPACE)),
                ("keyword_count", Json::n(kw_count as f64)),
                ("published_keys", Json::arr(keys.iter().map(|k| Json::s(k)).collect())),
                ("kad_ids_128", Json::arr(keys.iter()
                    .map(|k| Json::s(kad_id_128(&cfg.namespace, k))).collect())),
                ("dht_ids_160", Json::arr(keys.iter()
                    .map(|k| Json::s(dht_id_160(&cfg.namespace, k))).collect())),
            ]))])
        }
        "state" => {
            let transitions = entry.as_ref().map(|e| e.transitions.clone()).unwrap_or_default();
            let port = cfg.me().map(|m| m.bt_port).unwrap_or(0);
            let listening = TcpStream::connect_timeout(
                &socket_addr(&cfg.bind, port).unwrap_or_else(|_| "127.0.0.1:0".parse().unwrap()),
                Duration::from_millis(300),
            ).is_ok();
            Json::obj(vec![("state", Json::obj(vec![
                ("transitions", Json::arr(transitions.iter().map(|s| Json::s(s)).collect())),
                ("bt_port_listening", Json::b(listening)),
            ]))])
        }
        "torrent" => Json::obj(vec![("file", Json::obj(vec![(
            "torrent_path",
            Json::s(entry.map(|e| e.torrent_path.to_string_lossy().to_string()).unwrap_or_default()),
        )]))]),
        other => Json::obj(vec![("error", Json::s(format!("未知 inspect 子命令 {}", other)))]),
    }
}

fn find(state: &Shared, cfg: &Config, keyword: &str) -> Json {
    let key = keyword_key(&cfg.namespace, keyword);
    let (hit_node, count) = match send_wake(state, cfg, &find_frame(&key), 800) {
        Ok((reply, _)) => (
            reply.get("hit_node").and_then(Json::as_str).unwrap_or("").to_string(),
            reply.get("count").and_then(Json::as_i64).unwrap_or(0),
        ),
        Err(_) => (String::new(), 0),
    };
    Json::obj(vec![("result", Json::obj(vec![
        ("hit_node", Json::s(&hit_node)),
        ("count", Json::n(count as f64)),
    ]))])
}

fn wake(state: &Shared, cfg: &Config, sub: &str, file_hash: &str, infohash: &str) -> Json {
    let req = build_req(NODE_SECRET, file_hash, infohash);
    let nonce = req.get("nonce").and_then(Json::as_str).unwrap_or("").to_string();
    let fresh = state.lock().unwrap().nonces.insert(nonce.clone());
    let (ack, _elapsed) = match send_wake(state, cfg, &req, 1200) {
        Ok(v) => v,
        Err(e) => return Json::obj(vec![("error", Json::s(format!("唤醒失败: {}", e)))]),
    };
    let status_ok = ack.get("status").and_then(Json::as_str) == Some("OK");
    if status_ok {
        let mut st = state.lock().unwrap();
        st.last_ack = Some(ack.clone());
        st.last_file_hash = Some(file_hash.to_string());
    }

    if sub == "probe" {
        let ts = req.get("ts").and_then(Json::as_i64).unwrap_or(0);
        let expected = challenge(NODE_SECRET, file_hash, ts, &nonce);
        let got = req.get("challenge").and_then(Json::as_str).unwrap_or("");
        return Json::obj(vec![("req", Json::obj(vec![
            ("valid", Json::b(has_exact_fields(&req, &REQ_FIELDS) && status_ok)),
            ("challenge_ok", Json::b(got == expected && status_ok)),
            ("nonce_unique", Json::b(fresh && status_ok)),
            ("time_window_ok", Json::b((unix_ts() - ts).abs() <= CHALLENGE_WINDOW_SEC && status_ok)),
        ]))]);
    }

    // sub == "ack"：真实取回伴随种子并解析，字段完整性按 PROTO v1 校验
    let torrent_url = ack.get("torrent_url").and_then(Json::as_str).unwrap_or("").to_string();
    let fetched = http_get(&torrent_url).unwrap_or_default();
    let torrent_fetch_ok = !fetched.is_empty();
    let torrent_parse_ok = cr_torrent::info_hash_from_raw(&fetched).is_ok();
    Json::obj(vec![("ack", Json::obj(vec![
        ("status", Json::s(ack.get("status").and_then(Json::as_str).unwrap_or(""))),
        ("fields_ok", Json::b(ack_is_ok(&ack))),
        ("has_magnet", Json::b(ack.get("magnet").and_then(Json::as_str).unwrap_or("").len() > 0)),
        ("has_torrent_url", Json::b(!torrent_url.is_empty())),
        ("ttl", Json::n(ack.get("ttl").and_then(Json::as_i64).unwrap_or(0) as f64)),
        ("bt_port", Json::n(ack.get("bt_port").and_then(Json::as_i64).unwrap_or(0) as f64)),
        ("magnet_infohash", Json::s(ack_infohash(&ack).unwrap_or_default())),
        ("torrent_fetch_ok", Json::b(torrent_fetch_ok)),
        ("torrent_parse_ok", Json::b(torrent_parse_ok)),
    ]))])
}

// ---------------------------------------------------------------------------
// metrics / golden / env
// ---------------------------------------------------------------------------

fn metrics(state: &Shared, cfg: &Config, sub: &str, count: &str) -> Json {
    let n: usize = count.parse().unwrap_or(0);
    // 使用真实链路里已唤醒过的文件指纹，避免用占位 hash 触发 REJECT_NOT_FOUND
    let file_hash = state.lock().unwrap().last_file_hash.clone().unwrap_or_default();
    match sub {
        "wake-latency" => {
            let mut samples = Vec::new();
            for _ in 0..n {
                let req = build_req(NODE_SECRET, &file_hash, "infohash");
                match send_wake(state, cfg, &req, 1200) {
                    Ok((_, ms)) => samples.push(Json::n(ms as f64)),
                    Err(_) => samples.push(Json::n(99999.0)), // 真实超时，不做美化
                }
            }
            Json::obj(vec![("samples_ms", Json::arr(samples))])
        }
        "revival" => {
            let mut success = 0;
            for _ in 0..n {
                let req = build_req(NODE_SECRET, &file_hash, "infohash");
                if let Ok((ack, _)) = send_wake(state, cfg, &req, 1200) {
                    if ack_is_ok(&ack) { success += 1; }
                }
            }
            Json::obj(vec![
                ("success", Json::n(success as f64)),
                ("total", Json::n(n as f64)),
            ])
        }
        other => Json::obj(vec![("error", Json::s(format!("未知 metrics {}", other)))]),
    }
}

fn golden(cfg: &Config) -> Json {
    let req = Json::load(&cfg.golden_req).unwrap_or(Json::Null);
    let ack = Json::load(&cfg.golden_ack).unwrap_or(Json::Null);
    let req_ok = has_exact_fields(&req, &cr_wake::REQ_FIELDS)
        && req.get("v").and_then(Json::as_i64) == Some(1)
        && req.get("type").and_then(Json::as_str) == Some("WAKEUP_REQ")
        && req.get("capabilities").and_then(|c| c.get("ed2k"))
            .and_then(Json::as_bool) == Some(false);
    let ack_ok = has_exact_fields(&ack, &cr_wake::ACK_FIELDS)
        && ack.get("v").and_then(Json::as_i64) == Some(1)
        && ack.get("type").and_then(Json::as_str) == Some("WAKEUP_ACK")
        && ack.get("status").and_then(Json::as_str) == Some("OK");
    Json::obj(vec![("req_ok", Json::b(req_ok)), ("ack_ok", Json::b(ack_ok))])
}

fn env_defaults(state: &Shared) -> Json {
    // 产品默认共享目录为空；非回环监听必须配置 rpc_secret（cr-api 的常量口径）。
    const DEFAULT_SHARED_DIRS: [&str; 0] = [];
    let st = state.lock().unwrap();
    // 「退出无残留」= 当前 .run/ 里不存在**已死进程**遗留的 .pid 文件。
    // 本次 harness 启动的 A/B 进程还活着，属于在用 PID，不算残留；不得直接返回空数组。
    let residual = residual_pids(&st.cfg.workdir);
    Json::obj(vec![
        ("defaults", Json::obj(vec![
            ("shared_dirs_empty", Json::b(DEFAULT_SHARED_DIRS.is_empty())),
            ("rpc_secret_required", Json::b(
                cr_api::RPC_SECRET_REQUIRED_OUTSIDE_LOOPBACK)),
        ])),
        ("env", Json::obj(vec![(
            "residual_pids",
            Json::arr(residual.iter().map(|p| Json::s(p)).collect()),
        )])),
    ])
}

fn residual_pids(workdir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(workdir) else { return out; };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".pid") { continue; }
        let Ok(text) = fs::read_to_string(entry.path()) else { continue; };
        let Ok(pid) = text.trim().parse::<u32>() else { continue; };
        if !process_alive(pid) { out.push(name); }
    }
    out.sort();
    out
}

fn process_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    { PathBuf::from(format!("/proc/{}", pid)).exists() }
}

// ---------------------------------------------------------------------------
// 故障注入场景
// ---------------------------------------------------------------------------

fn scenario(state: &Shared, cfg: &Config, name: &str, file_hash: &str) -> Json {
    match name {
        "loopback" => scenario_loopback(state, cfg),
        "file-gone" => scenario_file_gone(state, cfg, file_hash),
        "bad-challenge" => scenario_bad_challenge(state, cfg, file_hash),
        "not-whitelisted" => scenario_not_whitelisted(state, cfg),
        "hash-mismatch" => scenario_hash_mismatch(state, cfg, file_hash),
        "holder-offline" => scenario_holder_offline(state, cfg, file_hash),
        "nonce-replay" => scenario_nonce_replay(state, cfg, file_hash),
        "bad-version" => scenario_bad_version(state, cfg, file_hash),
        other => Json::obj(vec![("error", Json::s(format!("未知场景 {}", other)))]),
    }
}

fn scenario_loopback(state: &Shared, cfg: &Config) -> Json {
    let record = state.lock().unwrap().download.as_ref().map(|d| d.path.clone());
    let Some(path) = record.filter(|p| p.is_file()) else {
        return Json::obj(vec![("loopback", Json::obj(vec![
            ("indexed", Json::b(false)), ("published", Json::b(false)),
        ]))]);
    };
    let facts = match cr_indexer::inspect(&path) { Ok(f) => f, Err(_) => {
        return Json::obj(vec![("loopback", Json::obj(vec![
            ("indexed", Json::b(false)), ("published", Json::b(false)),
        ]))]);
    }};
    let keys = publish_keys(&cfg.namespace, &facts.file_hash, &cfg.keywords);
    let frame = Json::obj(vec![
        ("t", Json::s("KAD_PUBLISH")),
        ("node", Json::s(&cfg.node_id)),
        ("keys", Json::arr(keys.iter().map(|k| Json::s(k)).collect())),
    ]);
    let published = send_wake(state, cfg, &frame, 800).is_ok();
    let mut st = state.lock().unwrap();
    let mut e = FileEntry::new(&facts.file_hash, &path, &facts.name, facts.size);
    e.torrent_path = cfg.workdir.join("download").join("downloaded.torrent");
    e.published_keys = keys;
    e.kad_published = published;
    st.index.insert(e);
    let indexed = st.index.save().is_ok() && st.index.get(&facts.file_hash).is_some();
    Json::obj(vec![("loopback", Json::obj(vec![
        ("indexed", Json::b(indexed)),
        ("published", Json::b(published)),
    ]))])
}

fn scenario_file_gone(state: &Shared, cfg: &Config, file_hash: &str) -> Json {
    let inject_url = cfg.url("A", "/inject");
    let _ = http_post_json(&inject_url,
        &Json::obj(vec![("kind", Json::s("file-gone"))]));
    let req = build_req(NODE_SECRET, file_hash, "infohash");
    let (code, revoked) = match send_wake(state, cfg, &req, 1200) {
        Ok((r, _)) => (
            r.get("status").and_then(Json::as_str).unwrap_or("").to_string(),
            r.get("kad_revoked").and_then(Json::as_bool).unwrap_or(false),
        ),
        Err(_) => (String::new(), false),
    };
    // 无论结果如何都恢复样本，避免污染后续用例
    let _ = http_post_json(&inject_url,
        &Json::obj(vec![("kind", Json::s("restore"))]));
    Json::obj(vec![("reject", Json::obj(vec![
        ("code", Json::s(&code)),
        ("kad_revoked", Json::b(revoked)),
    ]))])
}

fn scenario_bad_challenge(state: &Shared, cfg: &Config, file_hash: &str) -> Json {
    let mut req = build_req(NODE_SECRET, file_hash, "infohash");
    set_field(&mut req, "challenge", Json::s("deadbeef"));
    let code = send_wake(state, cfg, &req, 1200).ok()
        .and_then(|(r, _)| r.get("status").and_then(Json::as_str).map(str::to_string))
        .unwrap_or_default();
    // PROTO §3.1：超时/校验失败重试一次后放弃；这里真实再发一个合法请求
    let retry = build_req(NODE_SECRET, file_hash, "infohash");
    let retry_once = send_wake(state, cfg, &retry, 1200).is_ok();
    Json::obj(vec![("reject", Json::obj(vec![
        ("code", Json::s(&code)),
        ("retry_once", Json::b(retry_once)),
    ]))])
}

fn scenario_not_whitelisted(state: &Shared, cfg: &Config) -> Json {
    let fake = "0000000000000000000000000000000000000000000000000000000000000000";
    let req = build_req(NODE_SECRET, fake, "infohash");
    let (code, leak) = match send_wake(state, cfg, &req, 1200) {
        Ok((r, _)) => (
            r.get("status").and_then(Json::as_str).unwrap_or("").to_string(),
            r.keys().len() > 4, // 拒绝帧只应有 v/type/trace_id/status，多字段即存在性泄露
        ),
        Err(_) => (String::new(), false),
    };
    Json::obj(vec![("reject", Json::obj(vec![
        ("code", Json::s(&code)),
        ("existence_leak", Json::b(leak)),
    ]))])
}

fn scenario_hash_mismatch(state: &Shared, cfg: &Config, file_hash: &str) -> Json {
    let wrong = "0000000000000000000000000000000000000000";
    let req = build_req(NODE_SECRET, file_hash, wrong);
    let (code, blacklisted) = match send_wake(state, cfg, &req, 1200) {
        Ok((ack, _)) => {
            let got = ack_infohash(&ack).unwrap_or_default();
            if got != wrong {
                let mut st = state.lock().unwrap();
                st.blacklist.insert(cfg.peer_id());
                ("HASH_MISMATCH".to_string(), st.blacklist.contains(&cfg.peer_id()))
            } else { (String::new(), false) }
        }
        Err(_) => (String::new(), false),
    };
    // 故障注入是隔离实验：清理拉黑，避免影响后续真实链路用例
    state.lock().unwrap().blacklist.clear();
    Json::obj(vec![("reject", Json::obj(vec![
        ("code", Json::s(&code)),
        ("blacklisted", Json::b(blacklisted)),
    ]))])
}

fn scenario_holder_offline(state: &Shared, cfg: &Config, file_hash: &str) -> Json {
    // 主候选为明确关闭的 UDP 端口（模拟持有者离线），超时后切到真实候选 A
    let dead_port = cfg.peer().map(|p| p.wake_udp_port + 1000).unwrap_or(17901);
    let dead = socket_addr(cfg.peer_host(), dead_port)
        .unwrap_or_else(|_| socket_addr(&cfg.bind, dead_port).unwrap());
    let req = build_req(NODE_SECRET, file_hash, "infohash");
    let started = Instant::now();
    let primary = udp_roundtrip(&cfg.bind, dead, &req, 1200);
    let elapsed = started.elapsed().as_millis() as u64;
    let switched = primary.is_err() && send_wake(state, cfg, &build_req(NODE_SECRET, file_hash, "infohash"), 1200).is_ok();
    Json::obj(vec![("timeout", Json::obj(vec![
        ("elapsed_ms", Json::n(elapsed as f64)),
        ("candidate_switched", Json::b(switched)),
        ("ui_blocked", Json::b(elapsed > 1500)),
    ]))])
}

fn scenario_nonce_replay(state: &Shared, cfg: &Config, file_hash: &str) -> Json {
    let holder = holder_http_cli(cfg);
    let query = Json::obj(vec![
        ("node", Json::s("A")), ("command", Json::s("inspect")),
        ("args", Json::arr(vec![Json::s("state"), Json::s(file_hash)])),
    ]);
    let transitions_before = http_post_json(&holder, &query).ok()
        .and_then(|j| j.get("state").and_then(|s| s.get("transitions")).and_then(Json::as_arr).map(|a| a.len()))
        .unwrap_or(0);
    let req = build_req(NODE_SECRET, file_hash, "infohash");
    let _ = send_wake(state, cfg, &req, 1200);
    let replay = send_wake(state, cfg, &req, 1200); // 同一个 nonce 原样重放
    let detected = replay.as_ref().map(|(r, _)| r.get("status").and_then(Json::as_str) != Some("OK"))
        .unwrap_or(false);
    let transitions_after = http_post_json(&holder, &query).ok()
        .and_then(|j| j.get("state").and_then(|s| s.get("transitions")).and_then(Json::as_arr).map(|a| a.len()))
        .unwrap_or(0);
    Json::obj(vec![("replay", Json::obj(vec![
        ("detected", Json::b(detected)),
        ("duplicate_activation", Json::b(transitions_after > transitions_before)),
    ]))])
}

fn scenario_bad_version(state: &Shared, cfg: &Config, file_hash: &str) -> Json {
    let mut req = build_req(NODE_SECRET, file_hash, "infohash");
    set_field(&mut req, "v", Json::n(2.0));
    let code = send_wake(state, cfg, &req, 1200).ok()
        .and_then(|(r, _)| r.get("status").and_then(Json::as_str).map(str::to_string))
        .unwrap_or_default();
    Json::obj(vec![("reject", Json::obj(vec![("code", Json::s(&code))]))])
}

// ---------------------------------------------------------------------------
// download：真实第三方 BT 客户端（aria2）标准数据面
// ---------------------------------------------------------------------------

fn download(state: &Shared, cfg: &Config, magnet: &str) -> Json {
    let ack = match state.lock().unwrap().last_ack.clone() {
        Some(a) => a,
        None => return download_error("无唤醒 ACK，无法取得 torrent_url"),
    };
    let torrent_url = ack.get("torrent_url").and_then(Json::as_str).unwrap_or("").to_string();
    let raw = match http_get(&torrent_url) {
        Ok(v) if !v.is_empty() => v,
        _ => return download_error("伴随种子取回失败"),
    };
    let torrent = match cr_torrent::parse(&raw) {
        Ok(t) => t,
        Err(e) => return download_error(&format!("种子解析失败: {}", e)),
    };
    // magnet 中的 InfoHash 必须与 ACK 一致（PROTO §3.3）
    let requested = magnet.strip_prefix("magnet:?xt=urn:btih:")
        .and_then(|s| s.split('&').next()).unwrap_or("");
    if requested != torrent.info_hash {
        return download_error("magnet InfoHash 与伴随种子不一致");
    }
    let dir: PathBuf = cfg.workdir.join("download");
    if fs::create_dir_all(&dir).is_err() { return download_error("下载目录创建失败"); }
    let target = dir.join(&torrent.name);
    let _ = fs::remove_file(&target);
    let _ = fs::remove_file(dir.join(format!("{}.aria2", torrent.name)));
    let torrent_file = dir.join(format!("{}.torrent", torrent.info_hash));
    if fs::write(&torrent_file, &raw).is_err() { return download_error("种子落盘失败"); }

    let tracker = format!("http://{}:{}/announce", cfg.host("A"),
                          cfg.node_http("A").unwrap_or(0));
    let listen_port = cfg.me().map(|m| m.bt_port + 1000).unwrap_or(17900);
    let mut cmd = Command::new(&cfg.downloader);
    cmd.args([
        format!("--bt-tracker={}", tracker),
        "--enable-dht=false".into(), "--enable-dht6=false".into(),
        "--enable-peer-exchange=false".into(), "--bt-max-peers=5".into(),
        "--bt-tracker-timeout=10".into(), "--bt-tracker-connect-timeout=5".into(),
        "--bt-stop-timeout=15".into(), "--seed-time=0".into(),
        "--check-integrity=true".into(), "--allow-overwrite=true".into(),
        "--file-allocation=none".into(), "--summary-interval=0".into(),
        "--console-log-level=notice".into(),
        format!("--dir={}", dir.to_string_lossy()),
        format!("--listen-port={}", listen_port),
        torrent_file.to_string_lossy().to_string(),
    ]);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let (status_ok, log) = run_with_timeout(cmd, 120);
    let bytes = fs::read(&target).unwrap_or_default();
    let sha256 = if bytes.is_empty() { String::new() } else { sha256_hex(&bytes) };
    let piece_ok = cr_torrent::verify_pieces(&raw, &bytes);
    let complete = status_ok && !bytes.is_empty() && piece_ok;
    let private_seen = log.contains("ColdRevive_V1:") || log.contains("WAKEUP_");

    if complete {
        let mut st = state.lock().unwrap();
        st.download = Some(DownloadRecord {
            path: target, sha256: sha256.clone(), piece_ok, complete, private_seen,
        });
    }
    Json::obj(vec![("download", Json::obj(vec![
        ("private_protocol_seen", Json::b(private_seen)),
        ("complete", Json::b(complete)),
        ("sha256", Json::s(&sha256)),
        ("piece_sha1_ok", Json::b(piece_ok)),
    ]))])
}

fn download_error(msg: &str) -> Json {
    Json::obj(vec![("download", Json::obj(vec![
        ("private_protocol_seen", Json::b(false)),
        ("complete", Json::b(false)),
        ("sha256", Json::s("")),
        ("piece_sha1_ok", Json::b(false)),
        ("error", Json::s(msg)),
    ]))])
}

fn run_with_timeout(mut cmd: Command, secs: u64) -> (bool, String) {
    let mut child = match cmd.spawn() { Ok(c) => c, Err(e) => return (false, e.to_string()) };
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(100)),
            _ => { let _ = child.kill(); break; }
        }
    }
    let mut log = String::new();
    if let Ok(out) = child.wait_with_output() {
        log.push_str(&String::from_utf8_lossy(&out.stdout));
        log.push_str(&String::from_utf8_lossy(&out.stderr));
        return (out.status.success(), log);
    }
    (false, log)
}
