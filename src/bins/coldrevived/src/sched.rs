//! 做种活跃池：把 `cr-sched` 的 LRU 决策接到 `cr-engine` 的真实任务上。
//!
//! 为什么需要这一层：`Engine::activate` 只管「解除这一个任务的暂停」，它不知道全局
//! 允许几个任务同时供片。冷存储机器上如果每次唤醒都直接 activate，活跃任务会随唤醒
//! 次数单调增长，最终把磁盘随机读和上传带宽拖垮——而且没有任何一处代码会报错。
//!
//! 本模块是**唯一**允许调用 `activate` 的入口，职责是：
//! 1. 先问调度器「这个任务能不能进活跃池」；
//! 2. 若挤掉了别人，先暂停被挤掉的任务，再激活新任务；
//! 3. 返回**真实发生的事**（谁被逐出、哪一步失败），由调用方决定怎么报。
//!
//! 纪律：
//! - `AlreadyActive` 时**不再**调引擎 activate（librqbit 对 live 任务 unpause 会 bail，
//!   重复唤醒就会把「已在供片」翻成失败）。只有当引擎状态真的不是 HOT 才重试一次，
//!   这属于自愈而非重复激活。
//! - 暂停失败不阻止新任务激活：唤醒的语义是「让请求方拿到片」。但失败必须如实上报，
//!   不能吞掉——活跃数超过上限是资源问题，装成没事只会让它更晚被发现。

use cr_engine::{Engine, EngineState, RqbitEngine, TaskId};
use cr_sched::{Admission, LruScheduler};

/// 调度器需要的引擎动作。抽成 trait 是为了能在没有 librqbit 的情况下测试逐出路径。
pub trait SeedOps {
    fn seed_state(&self, task: TaskId) -> EngineState;
    fn activate_seed(&mut self, task: TaskId) -> Result<(), String>;
    fn pause_seed(&mut self, task: TaskId) -> Result<(), String>;
}

impl SeedOps for RqbitEngine {
    fn seed_state(&self, task: TaskId) -> EngineState {
        Engine::state(self, task)
    }

    fn activate_seed(&mut self, task: TaskId) -> Result<(), String> {
        Engine::activate(self, task)
    }

    fn pause_seed(&mut self, task: TaskId) -> Result<(), String> {
        Engine::pause(self, task)
    }
}

/// `wake_seed` 的结果。字段名刻意用「实际做了什么」，不用「应该会怎样」。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WakeOutcome {
    /// 请求的任务现在是否真的处于活跃（HOT）状态。
    pub serving: bool,
    /// `activated` / `already_active` / `evicted_then_activated` / `activate_failed`。
    pub admission: &'static str,
    /// 被逐出的任务号（仅满载时出现）。
    pub evicted: Option<TaskId>,
    /// 暂停被逐出者 / 激活请求者时的错误原文。空 = 全部成功。
    pub errors: Vec<String>,
}

/// 让 `task` 进入活跃池并真正开始供片。
pub fn wake_seed(sched: &mut LruScheduler, ops: &mut dyn SeedOps, task: TaskId) -> WakeOutcome {
    let mut out = WakeOutcome::default();
    match sched.activate(task) {
        Admission::AlreadyActive => {
            out.admission = "already_active";
            if ops.seed_state(task) == EngineState::Hot {
                out.serving = true;
                return out;
            }
            // 调度器认为它在池里，引擎却不是 HOT：可能是引擎侧偶发暂停。
            // 重试一次真实激活，成功才算供片——不拿调度器的记账当引擎状态。
            match ops.activate_seed(task) {
                Ok(()) => out.serving = true,
                Err(e) => {
                    out.errors.push(format!("重新激活任务 {task} 失败: {e}"));
                    out.admission = "activate_failed";
                }
            }
            out
        }
        Admission::Activated => {
            out.admission = "activated";
            match ops.activate_seed(task) {
                Ok(()) => out.serving = true,
                Err(e) => {
                    out.errors.push(format!("激活任务 {task} 失败: {e}"));
                    out.admission = "activate_failed";
                    // 引擎侧没起来就不该占着活跃池名额，否则会挤掉真正可用的任务。
                    sched.deactivate(task);
                }
            }
            out
        }
        Admission::ActivatedEvicting { evicted } => {
            out.admission = "evicted_then_activated";
            out.evicted = Some(evicted);
            if let Err(e) = ops.pause_seed(evicted) {
                out.errors.push(format!("暂停被逐出任务 {evicted} 失败: {e}"));
            }
            match ops.activate_seed(task) {
                Ok(()) => out.serving = true,
                Err(e) => {
                    out.errors.push(format!("激活任务 {task} 失败: {e}"));
                    out.admission = "activate_failed";
                    sched.deactivate(task);
                    // 新任务没起来：把被逐出的放回池子，比让它「无主地暂停着」更接近原状。
                    sched.activate(evicted);
                }
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeOps {
        hot: std::collections::HashSet<TaskId>,
        activated: Vec<TaskId>,
        paused: Vec<TaskId>,
        fail_activate: Option<TaskId>,
        fail_pause: Option<TaskId>,
    }

    impl SeedOps for FakeOps {
        fn seed_state(&self, task: TaskId) -> EngineState {
            if self.hot.contains(&task) {
                EngineState::Hot
            } else {
                EngineState::Cold
            }
        }
        fn activate_seed(&mut self, task: TaskId) -> Result<(), String> {
            if self.fail_activate == Some(task) {
                return Err("引擎拒绝: already live".into());
            }
            self.activated.push(task);
            self.hot.insert(task);
            Ok(())
        }
        fn pause_seed(&mut self, task: TaskId) -> Result<(), String> {
            if self.fail_pause == Some(task) {
                return Err("pause: io error".into());
            }
            self.paused.push(task);
            self.hot.remove(&task);
            Ok(())
        }
    }

    #[test]
    fn first_wake_activates_once() {
        let mut s = LruScheduler::new(2);
        let mut ops = FakeOps::default();
        let o = wake_seed(&mut s, &mut ops, 1);
        assert!(o.serving);
        assert_eq!(o.admission, "activated");
        assert_eq!(ops.activated, vec![1]);
        assert!(o.errors.is_empty());
    }

    #[test]
    fn repeat_wake_does_not_re_activate_live_task() {
        // 这是 W5.2 的真实缺陷形态：重复唤醒导致 unpause 报错，供片状态被翻成 false。
        let mut s = LruScheduler::new(2);
        let mut ops = FakeOps::default();
        wake_seed(&mut s, &mut ops, 1);
        let o = wake_seed(&mut s, &mut ops, 1);
        assert!(o.serving);
        assert_eq!(o.admission, "already_active");
        assert_eq!(ops.activated, vec![1], "第二次不得再调 activate");
    }

    #[test]
    fn already_active_but_engine_cold_self_heals() {
        let mut s = LruScheduler::new(2);
        let mut ops = FakeOps::default();
        s.activate(7); // 调度器记账里有，但引擎没起来
        let o = wake_seed(&mut s, &mut ops, 7);
        assert!(o.serving);
        assert_eq!(ops.activated, vec![7]);
    }

    #[test]
    fn full_pool_pauses_the_evicted_task_before_activating() {
        let mut s = LruScheduler::new(1);
        let mut ops = FakeOps::default();
        wake_seed(&mut s, &mut ops, 1);
        let o = wake_seed(&mut s, &mut ops, 2);
        assert!(o.serving);
        assert_eq!(o.evicted, Some(1));
        assert_eq!(o.admission, "evicted_then_activated");
        assert_eq!(ops.paused, vec![1], "被逐出者必须真的被暂停");
        assert_eq!(ops.activated, vec![1, 2]);
        assert!(!ops.hot.contains(&1), "逐出后不得仍处于 HOT");
    }

    #[test]
    fn pause_failure_is_reported_but_wake_still_serves() {
        let mut s = LruScheduler::new(1);
        let mut ops = FakeOps { fail_pause: Some(1), ..Default::default() };
        wake_seed(&mut s, &mut ops, 1);
        let o = wake_seed(&mut s, &mut ops, 2);
        assert!(o.serving, "暂停失败不应让请求方拿不到片");
        assert_eq!(o.errors.len(), 1);
        assert!(o.errors[0].contains("暂停被逐出任务 1 失败"));
    }

    #[test]
    fn activate_failure_releases_the_slot_and_restores_evicted() {
        let mut s = LruScheduler::new(1);
        let mut ops = FakeOps { fail_activate: Some(2), ..Default::default() };
        wake_seed(&mut s, &mut ops, 1);
        let o = wake_seed(&mut s, &mut ops, 2);
        assert!(!o.serving);
        assert_eq!(o.admission, "activate_failed");
        assert_eq!(o.errors.len(), 1);
        // 失败的任务不得继续占名额；被逐出的回到池子里，活跃池仍是 1 个。
        assert!(!s.is_active(2));
        assert!(s.is_active(1));
        assert_eq!(s.len(), 1);
    }
}
