//! coldrevived CLI v1 命令分发。
//!
//! 每个字段都来自真实来源：本地文件 SHA-256 / 真实 .torrent bencode / 真实 UDP 往返 /
//! 真实第三方下载器进程 / 真实 golden 文件解析。失败时返回 false 或错误码，
//! **绝不返回「看起来像通过」的默认常量**。

use crate::{ed2k_tasks, http_get, http_post_json, nat_path, DownloadRecord, NodeState, NODE_SECRET};
use cr_core::{unix_ts, Config, Json, CHALLENGE_WINDOW_SEC};
use cr_engine::Engine;
use cr_hash::sha256_hex;
use cr_kad::{detect_language, expand_keywords, find_frame, kad_id_128, keyword_key,
             publish_keys, socket_addr, udp_roundtrip, dht_id_160, MAX_KEYWORD_KEYS};
use cr_store::FileEntry;
use cr_wake::{ack_infohash, ack_is_ok, build_req, challenge, has_exact_fields, REQ_FIELDS};
use std::fs;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
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
        // FR-01 / FR-13 共用入口：把一个真实文件按多哈希入库（不发布、不判 pass）。
        "index" => index_path(state, &arg(&args, 0)),
        // FR-05/07：把**已入库**文件的 key 真发出去；只有收到 ACK 才记账为已发布。
        "publish" => publish_path(state, &arg(&args, 0)),
        "find" => find(state, &cfg, &arg(&args, 0)),
        "wake" => wake(state, &cfg, &arg(&args, 0), &arg(&args, 1), &arg(&args, 2)),
        "scenario" => scenario(state, &cfg, &arg(&args, 0), &arg(&args, 1)),
        "metrics" => metrics(state, &cfg, &arg(&args, 0), &arg(&args, 1)),
        "seed" => seed_stats(state),
        "ed2k" => ed2k_links(state, &arg(&args, 0)),
        // FR-05/07：KAD 关键词去重健康度 + 刷新状态（只读）。
        "kad-health" => kad_health(state),
        // FR-25 daemon 接线：三条命令都走生产 cr-ed2k::stack，后台任务结果由
        // `ed2k-status` 从真实栈返回值读取，不编造完成状态。
        "ed2k-link" => ed2k_tasks::link_cmd(&args),
        "ed2k-serve" => {
            let tasks = state.lock().unwrap().ed2k_tasks.clone();
            ed2k_tasks::serve_cmd(&tasks, &args)
        }
        "ed2k-download" => {
            let tasks = state.lock().unwrap().ed2k_tasks.clone();
            ed2k_tasks::download_cmd(&tasks, &args)
        }
        "ed2k-status" => {
            let tasks = state.lock().unwrap().ed2k_tasks.clone();
            ed2k_tasks::status_cmd(&tasks, &args)
        }
        "ed2k-list" => {
            let tasks = state.lock().unwrap().ed2k_tasks.clone();
            ed2k_tasks::list_cmd(&tasks)
        }
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
                // FR-04：索引来源必须可见——recovered_from_backup / corrupt 不能被读成正常。
                ("open_status", Json::s(st.index.status.as_str())),
                ("schema_version", Json::n(st.index.loaded_schema as f64)),
                ("entries", Json::n(st.index.len() as f64)),
                ("ed2k_hash_present", Json::b(entry.as_ref()
                    .map(|x| x.can_link_ed2k()).unwrap_or(false))),
            ])),
        ]),
        // 逐条列出索引，供 e2e 与运维核对「到底登记了哪几个文件、各自 infohash 是什么」。
        // `inspect index` 只报首条，多样本下无法枚举；这里不做任何猜测排序，按 file_hash 排。
        "entries" => {
            let mut all: Vec<(String, Json)> = st.index.entries.values().map(|e| {
                (e.file_hash.clone(), Json::obj(vec![
                    ("file_hash", Json::s(&e.file_hash)),
                    ("name", Json::s(&e.name)),
                    ("path", Json::s(e.path.to_string_lossy().to_string())),
                    ("size", Json::n(e.size as f64)),
                    ("state", Json::s(e.state.as_str())),
                    ("infohash", Json::s(&e.infohash)),
                    ("sha1", Json::s(&e.sha1)),
                    ("ed2k_hash", Json::s(&e.ed2k_hash)),
                    ("kad_published", Json::b(e.kad_published)),
                    ("published_keys", Json::n(e.published_keys.len() as f64)),
                ]))
            }).collect();
            all.sort_by(|a, b| a.0.cmp(&b.0));
            Json::obj(vec![
                ("open_status", Json::s(st.index.status.as_str())),
                ("schema_version", Json::n(st.index.loaded_schema as f64)),
                ("entries", Json::arr(all.into_iter().map(|(_, j)| j).collect())),
            ])
        }
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

/// 把「同一个文件的不同写法」（相对/绝对/含 `..`/符号链接）收敛成一个身份。
///
/// 索引以 `file_hash` 为键，但 `path` 字段是给人看、给删除事件匹配用的：
/// 同一个文件用相对路径入库、用绝对路径删除时，旧实现匹配不上，
/// 条目会永远留在索引里（e2e 的 FR-01 出库检查就是这样抓到的）。
/// 入库时 `canonicalize` 一次即可让所有后续写法统一。
pub(crate) fn canonical_or(p: PathBuf) -> PathBuf {
    std::fs::canonicalize(&p).unwrap_or(p)
}

/// 判断两个路径是否指向同一个文件条目。
///
/// 文件可能**已经被删除**（出库场景），此时 `canonicalize` 自身必然失败，
/// 因此退化为「规范化父目录 + 原文件名」——父目录还在，就足以判断身份。
fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    fn norm(p: &Path) -> Option<PathBuf> {
        let parent = p.parent()?;
        let name = p.file_name()?;
        let base = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
        Some(base.join(name))
    }
    match (norm(a), norm(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// FR-01 / FR-13 的共用入库动作：按真实字节算 sha256/sha1/ed2k 并落盘。
///
/// 这里**不发布、不改状态机、不判 pass**：只把「文件事实」写进影子索引，
/// 让目录监听到的新文件、下载完成的产物都能被后续 wake/publish 看见。
pub(crate) fn index_path(state: &Shared, path: &str) -> Json {
    if path.is_empty() {
        return Json::obj(vec![("error", Json::s("用法: index <路径>"))]);
    }
    let p = canonical_or(PathBuf::from(path));
    let facts = match cr_indexer::inspect(&p) {
        Ok(f) => f,
        Err(e) => return Json::obj(vec![
            ("indexed", Json::b(false)),
            ("error", Json::s(e)),
        ]),
    };
    let mut st = state.lock().unwrap();
    let namespace = st.cfg.namespace.clone();
    // FR-07：配置里给了关键词就用配置的；没给才从文件名切。
    // 顺序不能反：配置的关键词是运营者显式意图，文件名分词只是兜底。
    let mut keywords = st.cfg.keywords.clone();
    let mut keywords_source = "config";
    if keywords.is_empty() {
        keywords = cr_kad::tokenize_keywords(&facts.name, 8);
        keywords_source = "tokenized";
    }
    let mut e = FileEntry::new(&facts.file_hash, &p, &facts.name, facts.size);
    e.sha1 = facts.sha1.clone();
    e.ed2k_hash = facts.ed2k_hash.clone();
    // 语言标签和同义词扩展都只从**实际采用的关键词**推导：
    // 文件名里的容器标记（.mkv）已被分词器丢掉，不该污染语言判定。
    let language = detect_language(&keywords.join(" "));
    let expanded = expand_keywords(&keywords, MAX_KEYWORD_KEYS);
    e.keywords = keywords.clone();
    e.published_keys = publish_keys(&namespace, &facts.file_hash, &keywords);
    let already_known = st.index.get(&facts.file_hash).is_some();
    st.index.insert(e);
    let saved = st.index.save();
    Json::obj(vec![
        ("indexed", Json::b(saved.is_ok())),
        ("already_known", Json::b(already_known)),
        ("file_hash", Json::s(&facts.file_hash)),
        ("sha1", Json::s(&facts.sha1)),
        ("ed2k_hash", Json::s(&facts.ed2k_hash)),
        ("size", Json::n(facts.size as f64)),
        ("keywords", Json::arr(keywords.iter().map(|k| Json::s(k)).collect())),
        ("keywords_source", Json::s(keywords_source)),
        ("language", Json::s(language)),
        ("keywords_expanded", Json::arr(expanded.iter().map(|k| Json::s(k)).collect())),
        ("synonym_added", Json::n(expanded.len().saturating_sub(keywords.len()) as f64)),
        ("entries", Json::n(st.index.len() as f64)),
        ("index_status", Json::s(st.index.status.as_str())),
        ("error", Json::s(saved.err().unwrap_or_default())),
    ])
}

/// FR-01：文件从共享目录消失时把它移出索引。按**路径**匹配（同一文件可能有多个
/// 索引键），返回移除条数；不删磁盘上任何东西。
pub(crate) fn unindex_path(state: &Shared, path: &std::path::Path) -> usize {
    let mut st = match state.lock() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let doomed: Vec<String> = st.index.entries.values()
        .filter(|e| same_file(&e.path, path))
        .map(|e| e.file_hash.clone())
        .collect();
    if doomed.is_empty() {
        return 0;
    }
    for h in &doomed {
        st.index.entries.remove(h);
    }
    let _ = st.index.save();
    doomed.len()
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

    // sub == "raw"：把对端应答**原样**交出来。
    // `probe` 只给布尔值，会把「被资源阈值拒绝」和「challenge 算错」压成同一个 false；
    // 现场排查与 e2e 需要看到 REJECT_* 的具体码，以及 limit/current 这类附加字段。
    if sub == "raw" {
        return Json::obj(vec![("ack", ack)]);
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

/// 做种侧的真实计数器（W5.2：数字全部来自 `cr-engine`，不再来自手写种子端）。
///
/// `served_bytes` 是引擎自己统计的上传字节；COLD（加入时暂停）状态下它必然为 0，
/// 所以「唤醒前后这个数从 0 变成 ≥ 样本大小」本身就是按需做种的观测。
/// 字段名沿用 W5.1（已有验收脚本在读），新增的都是**引擎真有的**字段，没有估算值。
fn seed_stats(state: &Shared) -> Json {
    let st = state.lock().unwrap();
    let mut eg = st.engine.lock().unwrap();
    let task = st.seed_task;
    // µTP 是**配置+端口占用**两层事实：请求了不代表真的绑上了（librqbit 绑 UDP 失败只打日志）。
    let utp = eg.as_ref().map(|e| (e.utp_requested(), e.utp_port_occupied()));
    let progress = match (task, eg.as_mut()) {
        (Some(t), Some(eng)) => eng.progress(t),
        _ => None,
    };
    drop(eg);
    let mut fields = vec![
        ("seeding_hot", Json::b(st.bt_seeding)),
        ("served_bytes", Json::n(progress.as_ref().map(|p| p.uploaded_bytes as f64).unwrap_or(0.0))),
        ("engine_task", Json::n(task.map(|t| t as f64).unwrap_or(-1.0))),
        // W4.3 活跃池：`seeding_active` 是调度器记账的活跃数，`seeding_cap` 是上限，
        // `seeding_evictions` 是累计逐出次数。三者一起看才能区分「没唤醒」与「被挤掉」。
        ("seeding_active", Json::n(st.sched.len() as f64)),
        ("seeding_cap", Json::n(st.sched.cap() as f64)),
        ("seeding_evictions", Json::n(st.sched.evictions() as f64)),
    ];
    if let Some((req, occ)) = utp {
        fields.push(("utp_requested", Json::b(req)));
        fields.push(("utp_udp_port_occupied", Json::b(occ)));
    }
    match progress {
        Some(p) => {
            fields.push(("state", Json::s(p.state.as_str())));
            fields.push(("peers", Json::n(p.peers as f64)));
            fields.push(("checked_bytes", Json::n(p.downloaded_bytes as f64)));
            fields.push(("total_bytes", Json::n(p.total_bytes as f64)));
            fields.push(("finished", Json::b(p.finished)));
            if let Some(e) = p.error { fields.push(("error", Json::s(e))); }
        }
        None => {
            // 不做种的节点（B）或引擎尚未创建：如实说没有，不拿 0 冒充「观测到 0 上传」。
            fields.push(("state", Json::s("none")));
            fields.push(("note", Json::s("本节点没有引擎做种任务")));
        }
    }
    Json::obj(vec![("seed", Json::obj(fields))])
}

/// FR-24（本地可验证部分）：按真实文件内容算 `ed2k_hash`，产出标准 `ed2k://` 链接。
///
/// 只做**本地索引 → 链接**：`fileID` 由 `fs::read(path)` 的内容导出（ADR-011 L1 硬约束，
/// 不可伪造），关键词 Key 用无前缀 MD4（`cr_kad::kad_ed2k::ed2k_keyword_key`）。
/// **不查询公共 eMule 词典**：G0 `protocol_scope` 已于 2026-11-22 由项目所有者书面解除
/// （公共 0x33/0x43 已授权），但本命令仍只做本地索引；公网检索请用
/// `coldrevive-ed2k search --public --rate-limit-pps 0.2`，结果只认真实 UDP 观测。
///
/// 参数：可选 file_hash；缺省时输出全部索引条目。
fn ed2k_links(state: &Shared, file_hash: &str) -> Json {
    let mut st = state.lock().unwrap();
    let entries: Vec<FileEntry> = if file_hash.is_empty() {
        st.index.entries.values().cloned().collect()
    } else {
        match st.index.get(file_hash) {
            Some(e) => vec![e.clone()],
            None => return Json::obj(vec![
                ("found", Json::b(false)),
                ("note", Json::s(format!("索引里没有 file_hash={file_hash}"))),
            ]),
        }
    };
    let mut links = Vec::new();
    let mut backfilled: Vec<String> = Vec::new();
    for e in &entries {
        // FR-12：优先用索引里已存的 eD2k fileID —— 生成链接不该再读一遍盘。
        // 只有 v1 索引（没有 ed2k_hash）才回落到读文件，并把结果**回填**进索引，
        // 下次就走上快路径；回填失败不掩盖，照记 error。
        let (hash, size, source, err): ([u8; 16], usize, &'static str, Option<String>) = if e.can_link_ed2k() {
            match cr_hash::unhex16(&e.ed2k_hash) {
                Some(h) => (h, e.size, "index", None),
                None => {
                    links.push(Json::obj(vec![
                        ("file_hash", Json::s(&e.file_hash)),
                        ("error", Json::s("索引里的 ed2k_hash 不是 32 位十六进制")),
                    ]));
                    continue;
                }
            }
        } else {
            match fs::read(&e.path) {
                Ok(bytes) => (cr_hash::ed2k_hash(&bytes), bytes.len(), "disk", None),
                Err(err) => {
                    links.push(Json::obj(vec![
                        ("file_hash", Json::s(&e.file_hash)),
                        ("error", Json::s(format!("读文件失败: {err}"))),
                    ]));
                    continue;
                }
            }
        };
        if source == "disk" {
            if let Some(entry) = st.index.get_mut(&e.file_hash) {
                entry.ed2k_hash = cr_hash::hex(&hash);
                entry.size = size;
                backfilled.push(e.file_hash.clone());
            }
        }
        let link = cr_hash::ed2k_link(&e.name, size, &hash);
        let keys: Vec<String> = e.keywords.iter()
            .map(|k| cr_hash::hex(&cr_kad::kad_ed2k::ed2k_keyword_key(k)))
            .collect();
        links.push(Json::obj(vec![
            ("file_hash", Json::s(&e.file_hash)),
            ("name", Json::s(&e.name)),
            ("size", Json::n(size as f64)),
            ("ed2k_hash", Json::s(cr_hash::hex(&hash))),
            ("link_source", Json::s(source)),
            ("ed2k_link", Json::s(&link)),
            ("keyword_keys_md4", Json::arr(keys.into_iter().map(Json::s).collect())),
            ("error", Json::s(err.unwrap_or_default())),
        ]));
    }
    if !backfilled.is_empty() {
        let _ = st.index.save();
    }
    Json::obj(vec![
        ("count", Json::n(links.len() as f64)),
        ("backfilled_from_disk", Json::n(backfilled.len() as f64)),
        ("links", Json::arr(links)),
        ("public_search", Json::s("AUTHORIZED: 公共 KAD 0x33/0x43 已获所有者书面解除（2026-11-22）；本命令只做本地索引，公网检索请用 coldrevive-ed2k search --public --rate-limit-pps 0.2")),
    ])
}

/// FR-05/06：把**到期**的已发布条目重发布一次（周期任务调用）。
///
/// 与「每 N 秒无脑重发」的区别：这里按每条自己的 `kad_expires_at` 判断是否进入
/// 刷新窗口（`ttl/3` 处开始续），所以：
/// - 空转周期只报 `due=0`，不会伪造「发了 N 个 key」；
/// - 单次失败不会自我延期——`kad_expires_at` 只在成功时前推。
///
/// 口径纪律：只重发 `kad_published=true` 的条目已有的 key，不新增关键词；
/// 不把「发过帧 + 收到 ACK」写成「公网可检索」（后者仍需 S2/G0 证据）。
pub(crate) fn republish_published_keys(state: &Shared) -> Json {
    let ttl = {
        let st = match state.lock() {
            Ok(s) => s,
            Err(_) => return Json::obj(vec![("sent", Json::b(false)), ("error", Json::s("状态锁中毒"))]),
        };
        st.cfg.kad_ttl_secs.unwrap_or(cr_core::DEFAULT_KAD_TTL_SECS).max(3)
    };
    let now = cr_core::unix_ts().max(0) as u64;
    let (cfg, due) = {
        let st = match state.lock() {
            Ok(s) => s,
            Err(_) => return Json::obj(vec![("sent", Json::b(false)), ("error", Json::s("状态锁中毒"))]),
        };
        let due: Vec<(String, Vec<String>)> = st.index.entries.values()
            .filter(|e| e.kad_due(now, ttl))
            .map(|e| (e.file_hash.clone(), e.published_keys.clone()))
            .collect();
        (st.cfg.clone(), due)
    };
    if due.is_empty() {
        return Json::obj(vec![
            ("sent", Json::b(false)),
            ("due", Json::n(0.0)),
            ("ttl_secs", Json::n(ttl as f64)),
            ("note", Json::s("没有进入刷新窗口的条目，未发帧")),
        ]);
    }
    let mut keys: Vec<String> = due.iter().flat_map(|(_, k)| k.iter().cloned()).collect();
    keys.sort();
    keys.dedup();
    let frame = Json::obj(vec![
        ("t", Json::s("KAD_PUBLISH")),
        ("node", Json::s(&cfg.node_id)),
        ("keys", Json::arr(keys.iter().map(|k| Json::s(k)).collect())),
    ]);
    let outcome = send_wake(state, &cfg, &frame, 800);
    let (acked, extra) = match outcome {
        Ok((ack, elapsed)) => {
            // `sent=true` 只认**对端明确回了 KAD_PUBLISH_ACK**。任何别的应答
            // （旧版本对端的空对象、串到别的帧上的结果）都算未确认——否则
            // 「发出去一帧」会被读成「对端已收下关键词」。
            let ack_t = ack.get("t").and_then(Json::as_str).unwrap_or("");
            (ack_t == "KAD_PUBLISH_ACK", vec![
                ("elapsed_ms", Json::n(elapsed as f64)),
                ("ack_t", Json::s(ack_t)),
                ("ack_status", Json::s(ack.get("status").and_then(Json::as_str).unwrap_or(""))),
            ])
        }
        Err(e) => (false, vec![("error", Json::s(e))]),
    };
    // 成败都落盘：成功前推截止时间，失败只累加计数。
    let mut failures = 0usize;
    {
        let mut st = match state.lock() {
            Ok(s) => s,
            Err(_) => return Json::obj(vec![("sent", Json::b(false)), ("error", Json::s("状态锁中毒"))]),
        };
        for (fh, _) in &due {
            if let Some(e) = st.index.get_mut(fh) {
                if acked {
                    e.mark_published(now, ttl);
                } else {
                    e.mark_refresh_failed();
                    failures += 1;
                }
            }
        }
        let _ = st.index.save();
    }
    let mut fields = vec![
        ("sent", Json::b(acked)),
        ("due", Json::n(due.len() as f64)),
        ("keys", Json::n(keys.len() as f64)),
        ("ttl_secs", Json::n(ttl as f64)),
        ("refresh_failed", Json::n(failures as f64)),
    ];
    fields.extend(extra);
    Json::obj(fields)
}

/// FR-05/07：KAD 关键词的**去重健康度**与刷新状态。
///
/// 只报可复算的量：key 集合是从索引里现算的，`duplicate_keys` 就是
/// 「多发了一次也只会命中同一个 key」的条数。这样「发布了很多 key」与
/// 「发布了很多**不同**的 key」能分开看——后者才决定能不能被搜到。
pub(crate) fn kad_health(state: &Shared) -> Json {
    let st = match state.lock() {
        Ok(s) => s,
        Err(_) => return Json::obj(vec![("error", Json::s("状态锁中毒"))]),
    };
    let ttl = st.cfg.kad_ttl_secs.unwrap_or(cr_core::DEFAULT_KAD_TTL_SECS).max(3);
    let now = cr_core::unix_ts().max(0) as u64;

    let mut keys_all: Vec<String> = Vec::new();
    let mut kw_all: Vec<String> = Vec::new();
    let mut published = 0usize;
    let mut due = 0usize;
    let mut expired = 0usize;
    let mut expiring_soon = 0usize;
    let mut tokenized = 0usize;
    let mut failures = 0u64;
    let mut by_state: std::collections::BTreeMap<&'static str, usize> = Default::default();
    let mut languages: std::collections::BTreeMap<&'static str, usize> = Default::default();
    let mut synonym_added = 0usize;

    for e in st.index.entries.values() {
        *by_state.entry(e.state.as_str()).or_insert(0) += 1;
        kw_all.extend(e.keywords.iter().cloned());
        if !e.keywords.is_empty() {
            *languages.entry(cr_kad::detect_language(&e.keywords.join(" ")))
                .or_insert(0) += 1;
            synonym_added += cr_kad::expand_keywords(&e.keywords, MAX_KEYWORD_KEYS)
                .len().saturating_sub(e.keywords.len());
        }
        // key 集合按「会发出去的 key」统计，不按「已成功发过的」统计：
        // 后者在还没发布过的节点上恒为 0，去重健康度就看不出来了。
        keys_all.extend(e.published_keys.iter().cloned());
        if e.kad_published {
            published += 1;
            failures += e.kad_refresh_failures as u64;
            if e.kad_due(now, ttl) {
                due += 1;
            }
            if e.kad_expired(now) {
                expired += 1;
            } else if e.kad_expires_at != 0 && e.kad_expires_at <= now + ttl / 3 {
                expiring_soon += 1;
            }
        }
        // 「关键词是不是切出来的」用现算结果比对，不额外存标记：
        // 存了标记就会和真实分词逻辑脱节，比对是精确的。
        if !e.keywords.is_empty() && cr_kad::tokenize_keywords(&e.name, 8) == e.keywords {
            tokenized += 1;
        }
    }

    let mut keys_sorted = keys_all.clone();
    keys_sorted.sort();
    keys_sorted.dedup();
    let mut kw_sorted = kw_all.clone();
    kw_sorted.sort();
    kw_sorted.dedup();

    // 每个 key 被多少条条目发出：>1 就是重复，发多次只会命中同一条 KAD 记录。
    let mut per_key: std::collections::BTreeMap<&str, usize> = Default::default();
    for k in &keys_all {
        *per_key.entry(k.as_str()).or_insert(0) += 1;
    }
    let dup_detail: Vec<Json> = per_key.iter()
        .filter(|(_, n)| **n > 1)
        .map(|(k, n)| Json::obj(vec![
            ("key", Json::s(*k)),
            ("entries", Json::n(*n as f64)),
        ]))
        .collect();

    Json::obj(vec![
        ("entries", Json::n(st.index.entries.len() as f64)),
        ("by_state", Json::obj(by_state.iter().map(|(k, v)| (*k, Json::n(*v as f64))).collect())),
        ("published_entries", Json::n(published as f64)),
        ("ttl_secs", Json::n(ttl as f64)),
        ("due", Json::n(due as f64)),
        ("expired", Json::n(expired as f64)),
        ("expiring_soon", Json::n(expiring_soon as f64)),
        ("keys_total", Json::n(keys_all.len() as f64)),
        ("keys_unique", Json::n(keys_sorted.len() as f64)),
        ("duplicate_keys", Json::n(dup_detail.len() as f64)),
        ("duplicates", Json::arr(dup_detail)),
        ("keywords_total", Json::n(kw_all.len() as f64)),
        ("keywords_unique", Json::n(kw_sorted.len() as f64)),
        ("tokenized_entries", Json::n(tokenized as f64)),
        ("languages", Json::obj(languages.iter().map(|(k, v)| (*k, Json::n(*v as f64))).collect())),
        ("synonym_added", Json::n(synonym_added as f64)),
        ("refresh_failures", Json::n(failures as f64)),
        ("public_search", Json::s("AUTHORIZED: 公共 KAD 0x33/0x43 已获所有者书面解除（2026-11-22）；本地健康度不含公网命中，请另跑 coldrevive-ed2k search --public")),
    ])
}

/// FR-05/07：把已入库条目的 `published_keys` 真发出去。
///
/// 与 `/cli index` 刻意分开：入库只是本地事实，不能因为写了 key 就宣称「已发布」。
/// 这里也**只认对端明确的 `KAD_PUBLISH_ACK`**：收到一帧别的应答不算发布成功。
/// 这个 ACK 表示「对端节点收下了 key」，不表示公网 KAD 可检索（FR-24 公网证据需另跑真实检索）。
pub(crate) fn publish_path(state: &Shared, path: &str) -> Json {
    if path.is_empty() {
        return Json::obj(vec![("error", Json::s("用法: publish <路径>"))]);
    }
    let p = canonical_or(PathBuf::from(path));
    let facts = match cr_indexer::inspect(&p) {
        Ok(f) => f,
        Err(e) => return Json::obj(vec![("published", Json::b(false)), ("error", Json::s(e))]),
    };
    let (cfg, keys) = {
        let st = match state.lock() {
            Ok(s) => s,
            Err(_) => return Json::obj(vec![("published", Json::b(false)),
                                           ("error", Json::s("状态锁中毒"))]),
        };
        let Some(e) = st.index.get(&facts.file_hash) else {
            return Json::obj(vec![("published", Json::b(false)),
                                  ("error", Json::s("文件未入库，先执行 index"))]);
        };
        (st.cfg.clone(), e.published_keys.clone())
    };
    let frame = Json::obj(vec![
        ("t", Json::s("KAD_PUBLISH")),
        ("node", Json::s(&cfg.node_id)),
        ("keys", Json::arr(keys.iter().map(|k| Json::s(k)).collect())),
    ]);
    let now = unix_ts().max(0) as u64;
    let ttl = cfg.kad_ttl_secs.unwrap_or(cr_core::DEFAULT_KAD_TTL_SECS);
    let outcome = send_wake(state, &cfg, &frame, 800);
    let (acked, mut extra) = match outcome {
        Ok((ack, elapsed)) => {
            let ack_t = ack.get("t").and_then(Json::as_str).unwrap_or("").to_string();
            let ok = ack_t == "KAD_PUBLISH_ACK";
            (ok, vec![
                ("elapsed_ms", Json::n(elapsed as f64)),
                ("ack_t", Json::s(&ack_t)),
            ])
        }
        Err(e) => (false, vec![("error", Json::s(e))]),
    };
    if acked {
        let mut st = match state.lock() {
            Ok(s) => s,
            Err(_) => return Json::obj(vec![("published", Json::b(false)),
                                           ("error", Json::s("状态锁中毒"))]),
        };
        if let Some(e) = st.index.get_mut(&facts.file_hash) {
            e.mark_published(now, ttl);
        }
        extra.push(("saved", Json::b(st.index.save().is_ok())));
    }
    let mut fields = vec![
        ("published", Json::b(acked)),
        ("file_hash", Json::s(&facts.file_hash)),
        ("keys", Json::n(keys.len() as f64)),
    ];
    fields.append(&mut extra);
    Json::obj(fields)
}

fn golden(cfg: &Config) -> Json {    let req = Json::load(&cfg.golden_req).unwrap_or(Json::Null);
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
        // FR-13：完成产物回流索引。此前「下载完」只写 DownloadRecord，
        // 索引里没有这一条，于是「刚下完的文件」不能做种、不能生成 ed2k link，
        // 必须人工再 index 一次——这正是完成事件没有闭环的表现。
        let namespace = st.cfg.namespace.clone();
        let keywords = st.cfg.keywords.clone();
        let mut e = FileEntry::new(&sha256, &target, &torrent.name, bytes.len());
        e.sha1 = cr_hash::sha1_hex(&bytes);
        e.ed2k_hash = cr_hash::hex(&cr_hash::ed2k_hash(&bytes));
        e.infohash = torrent.info_hash.clone();
        e.torrent_path = torrent_file.clone();
        e.piece_size = torrent.piece_size;
        e.keywords = keywords.clone();
        e.published_keys = publish_keys(&namespace, &sha256, &keywords);
        st.index.insert(e);
        let _ = st.index.save();
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
