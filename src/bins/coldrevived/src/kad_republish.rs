//! FR-05/06：KAD 关键词的周期重发布。
//!
//! M1 的发布是一次性的：`/cli index` 或启动时发一帧 `KAD_PUBLISH`，之后不再维护。
//! KAD 条目会自然过期，于是「昨天发布过」在今天就等于「查不到」。这里补一个周期任务：
//! 按 `kad.republish_secs` 把**已发布条目**的 key 重发一遍。
//!
//! 口径边界（不许被这条任务顺手放宽）：
//! - 只重发 `kad_published=true` 的条目已有 key，不新增关键词、不改 TTL 数值；
//! - `sent=true` 只表示「本机发出了帧且收到 ACK」，**不等于公网可检索**；
//!   公网 KAD 检索（FR-24）已于 2026-11-22 获所有者书面解除 G0 scope，
//!   但 ACL 解除不等于这条周期任务取得了公网命中证据，仍须另跑真实检索。
//! - 失败（无对端/超时）原样记进 `/health.kad_publish.last_error`。

use crate::commands;
use crate::NodeState;
use cr_core::Json;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

#[derive(Default)]
struct Stats {
    cycles: u64,
    last: Option<Json>,
}

fn stats() -> &'static Mutex<Stats> {
    static S: OnceLock<Mutex<Stats>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Stats::default()))
}

/// 启动周期任务。返回 `false` 表示配置未开启（默认）。
pub fn spawn(state: Arc<Mutex<NodeState>>) -> bool {
    let period = {
        let st = match state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        match st.cfg.kad_republish_secs {
            Some(v) if v > 0 => v,
            _ => return false,
        }
    };
    let node = state.lock().map(|s| s.cfg.node_id.clone()).unwrap_or_default();
    println!("[{node}] FR-05 KAD 周期重发布已启动: 每 {period}s");
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(period));
        let out = commands::republish_published_keys(&state);
        if let Ok(mut g) = stats().lock() {
            g.cycles += 1;
            g.last = Some(out.clone());
        }
        let sent = out.get("sent").and_then(Json::as_bool).unwrap_or(false);
        let keys = out.get("keys").and_then(Json::as_i64).unwrap_or(0);
        if sent {
            println!("[{node}] FR-05 重发布 {} 个 key 完成", keys);
        } else {
            let err = out.get("error").and_then(Json::as_str)
                .or_else(|| out.get("note").and_then(Json::as_str)).unwrap_or("");
            eprintln!("[{node}] FR-05 重发布未成功: {err}");
        }
    });
    true
}

/// `/health.kad_publish`：只报实际发生过的周期数与最近一次真实结果。
pub fn stats_json() -> Json {
    let g = match stats().lock() {
        Ok(g) => g,
        Err(_) => return Json::obj(vec![("cycles", Json::n(0.0))]),
    };
    let mut fields = vec![
        ("cycles", Json::n(g.cycles as f64)),
        ("last", g.last.clone().unwrap_or(Json::Null)),
    ];
    if g.cycles == 0 {
        fields.push(("note", Json::s("尚未到第一个周期")));
    }
    Json::obj(fields)
}
