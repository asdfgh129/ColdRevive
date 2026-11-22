//! FR-01：共享目录增量监控（守护进程侧）。
//!
//! M1 只在启动时 `inspect()` 一次：把文件丢进共享目录后，除非重启，索引里不会多出
//! 这一条——「放进去就能被唤醒」的体验其实是重启给的。这里补上运行期增量：
//! 按 `indexer.watch_secs` 周期比对快照，把新增/修改入库、删除出库。
//!
//! 后端是 `cr_indexer::PollWatcher`（跨平台、std-only，符合 ADR-015 对该层的约束）。
//! 语义与内核通知一致（新增/修改/删除 + 去抖），代价是延迟 ≤ 周期；
//! Windows 的 ReadDirectoryChangesW 后端留待后续接入，**不因此宣称已用内核通知**。

use crate::commands;
use crate::NodeState;
use cr_indexer::PollWatcher;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// 启动监听线程。返回 `false` 表示「配置没开」或「目录不可用」，调用方照常启动。
pub fn spawn(state: Arc<Mutex<NodeState>>) -> bool {
    let (dir, period) = {
        let st = match state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let period = match st.cfg.indexer_watch_secs {
            Some(v) if v > 0 => v,
            _ => return false,
        };
        let dir = match st.cfg.sample_path.parent() {
            Some(p) if p.is_dir() => p.to_path_buf(),
            _ => {
                eprintln!("[{}] FR-01 目录监听未启动：共享目录不存在", st.cfg.node_id);
                return false;
            }
        };
        (dir, period)
    };

    let mut watcher = match PollWatcher::new(&dir, period * 1000) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[{}] FR-01 目录监听初始化失败: {e}", node_id_of(&state));
            return false;
        }
    };
    println!("[{}] FR-01 目录监听已启动: {} 每 {}s 比对一次",
             node_id_of(&state), dir.display(), period);

    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(period));
        let delta = match watcher.poll() {
            Ok(d) => d,
            Err(_) => return, // 目录消失：交给运维/重启处理，不在这里死循环刷日志
        };
        if delta.is_empty() {
            continue;
        }
        for p in delta.added.iter().chain(delta.modified.iter()) {
            // 删除与写入可能只差几毫秒：读不到就跳过，下一轮快照还会看到它。
            if !p.is_file() {
                continue;
            }
            let out = commands::index_path(&state, &p.to_string_lossy());
            let ok = out.get("indexed").and_then(cr_core::Json::as_bool).unwrap_or(false);
            let size = out.get("size").and_then(cr_core::Json::as_i64).unwrap_or(0);
            if ok {
                println!("[{}] FR-01 入库: {} ({} B)", node_id_of(&state),
                         p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                         size);
            }
        }
        for p in &delta.removed {
            let n = commands::unindex_path(&state, p);
            if n > 0 {
                println!("[{}] FR-01 出库: {}（{} 条）", node_id_of(&state), p.display(), n);
            }
        }
    });
    true
}

fn node_id_of(state: &Arc<Mutex<NodeState>>) -> String {
    state.lock().map(|s| s.cfg.node_id.clone()).unwrap_or_default()
}
