//! FR-14：空闲超时后的 HOT → COLD 批量切出。
//!
//! 和 `sched::wake_seed` 对称：唤醒只经一个入口，休眠也只经这里。
//! 引擎状态是事实来源；调度器里「记着活跃但引擎其实已停」只能清理记账，
//! 不能当成一次成功休眠（否则 /health 会造假）。
//!
//! 影子索引条目**不删**：只把 state 翻回 COLD 并持久化，这样休眠后
//! KAD/本地检索仍能查到该节点。文件本体与伴随种子都不动，下次唤醒可立即供片。

use crate::sched::SeedOps;
use cr_engine::{EngineState, TaskId};
use cr_sched::LruScheduler;
use cr_store::{FileState, ShadowIndex};
use std::collections::HashMap;

/// 一次休眠实际做了什么。全是可观测动作，不写「应该会怎样」。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SleepOutcome {
    /// 真的被暂停并切回 COLD 的任务号。
    pub slept: Vec<TaskId>,
    /// 暂停失败时保留原状态，并记录引擎错误原文。
    pub errors: Vec<String>,
    /// 调度器里仍登记的活跃数（含暂停失败与陈旧记账）。
    pub remaining_active: usize,
}

/// 把所有活跃任务切回冷态。`index` 与 `seed_tasks` 只用于回填对应的文件条目，
/// 不会删除任何条目或文件。
pub fn sleep_hot_seeds(
    sched: &mut LruScheduler,
    ops: &mut dyn SeedOps,
    index: &mut ShadowIndex,
    seed_tasks: &HashMap<String, TaskId>,
) -> SleepOutcome {
    let mut out = SleepOutcome::default();
    for task in sched.active() {
        // 引擎才是事实来源。调度器有记录但引擎已停：清理记账即可，
        // 不调 pause（librqbit 对已停任务 pause 会报错，伪装成休眠失败）。
        if ops.seed_state(task) != EngineState::Hot {
            sched.deactivate(task);
            continue;
        }
        match ops.pause_seed(task) {
            Ok(()) => {
                sched.deactivate(task);
                out.slept.push(task);
                // `seed_tasks` 的键是 infohash（见 start_engine_seeder），
                // 而 ShadowIndex 按 file_hash 索引。按任一标识匹配，避免
                // 「任务确实暂停了，但影子索引仍写 HOT」这种假证据。
                for (task_key, mapped) in seed_tasks {
                    if *mapped != task { continue; }
                    let matched: Vec<String> = index.entries.values()
                        .filter(|e| e.infohash == *task_key || e.file_hash == *task_key)
                        .map(|e| e.file_hash.clone())
                        .collect();
                    for fh in matched {
                        if let Some(e) = index.get_mut(&fh) {
                            e.transition(FileState::Cold);
                        }
                    }
                }
            }
            Err(e) => out.errors.push(format!("休眠任务 {task} 失败: {e}")),
        }
    }
    out.remaining_active = sched.active().len();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_store::FileEntry;

    #[derive(Default)]
    struct FakeOps {
        hot: std::collections::HashSet<TaskId>,
        paused: Vec<TaskId>,
        fail_pause: Option<TaskId>,
    }

    impl SeedOps for FakeOps {
        fn seed_state(&self, task: TaskId) -> EngineState {
            if self.hot.contains(&task) { EngineState::Hot } else { EngineState::Cold }
        }
        fn activate_seed(&mut self, task: TaskId) -> Result<(), String> {
            self.hot.insert(task);
            Ok(())
        }
        fn pause_seed(&mut self, task: TaskId) -> Result<(), String> {
            if self.fail_pause == Some(task) {
                return Err("pause: disk busy".into());
            }
            self.paused.push(task);
            self.hot.remove(&task);
            Ok(())
        }
    }

    fn index_with(file_hashes: &[&str]) -> ShadowIndex {
        let mut idx = ShadowIndex::open(std::path::Path::new(
            &std::env::temp_dir().join("cr_idle_test_missing.json"),
        ));
        idx.entries.clear();
        for h in file_hashes {
            let mut e = FileEntry::new(h, std::path::Path::new(&format!("{h}.bin")), h, 1);
            e.infohash = format!("ih-{h}");
            e.transition(FileState::Hot);
            idx.insert(e);
        }
        idx
    }

    #[test]
    fn sleeps_all_hot_tasks_and_keeps_shadow_entries() {
        let mut sched = LruScheduler::new(4);
        sched.activate(11);
        sched.activate(12);
        let mut ops = FakeOps { hot: [11, 12].into_iter().collect(), ..Default::default() };
        let mut index = index_with(&["h11", "h12"]);
        let tasks = HashMap::from([("h11".to_string(), 11), ("h12".to_string(), 12)]);

        let out = sleep_hot_seeds(&mut sched, &mut ops, &mut index, &tasks);

        assert_eq!(out.slept, vec![11, 12]);
        assert!(out.errors.is_empty());
        assert_eq!(out.remaining_active, 0);
        assert_eq!(ops.paused, vec![11, 12]);
        assert_eq!(index.len(), 2, "影子索引不随休眠删除");
        assert!(index.entries.values().all(|e| e.state == FileState::Cold));
    }

    #[test]
    fn pause_failure_keeps_task_active_and_does_not_lie() {
        let mut sched = LruScheduler::new(2);
        sched.activate(21);
        let mut ops = FakeOps {
            hot: [21].into_iter().collect(),
            fail_pause: Some(21),
            ..Default::default()
        };
        let mut index = index_with(&["h21"]);
        let tasks = HashMap::from([("h21".to_string(), 21)]);

        let out = sleep_hot_seeds(&mut sched, &mut ops, &mut index, &tasks);

        assert!(out.slept.is_empty());
        assert_eq!(out.remaining_active, 1);
        assert!(out.errors[0].contains("disk busy"));
        assert_eq!(index.get("h21").unwrap().state, FileState::Hot);
    }

    #[test]
    fn stale_scheduler_record_is_cleaned_without_fake_pause() {
        let mut sched = LruScheduler::new(2);
        sched.activate(31); // 调度器说活跃，引擎其实没在跑
        let mut ops = FakeOps::default();
        let mut index = index_with(&["h31"]);
        let tasks = HashMap::from([("h31".to_string(), 31)]);

        let out = sleep_hot_seeds(&mut sched, &mut ops, &mut index, &tasks);

        assert!(out.slept.is_empty(), "没有真的暂停就不能算休眠");
        assert!(ops.paused.is_empty());
        assert_eq!(out.remaining_active, 0);
    }

    #[test]
    fn seed_tasks_keyed_by_infohash_still_updates_shadow_state() {
        // start_engine_seeder 用 infohash 当 seed_tasks 的键；这里钉住
        // 「暂停成功但索引没跟着变冷」这个真实缺陷不会回归。
        let mut sched = LruScheduler::new(2);
        sched.activate(41);
        let mut ops = FakeOps { hot: [41].into_iter().collect(), ..Default::default() };
        let mut index = index_with(&["h41"]);
        let tasks = HashMap::from([("ih-h41".to_string(), 41)]);

        let out = sleep_hot_seeds(&mut sched, &mut ops, &mut index, &tasks);

        assert_eq!(out.slept, vec![41]);
        assert_eq!(index.get("h41").unwrap().state, FileState::Cold);
        assert_eq!(index.get("h41").unwrap().transitions.last().unwrap(), "COLD");
    }
}
