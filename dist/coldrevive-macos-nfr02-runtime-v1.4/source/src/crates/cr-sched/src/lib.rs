//! cr-sched — W4.3 LRU 冷热调度器。
//!
//! 职责只有一个：**决定哪些种子任务可以处于「活跃（正在供片）」状态**。
//! 冷存储盘上可能挂着上万个种子，同时活跃的只能是少数几个——超出部分必须被
//! 暂停而不是全部打开，否则磁盘随机读、内存工作集与上传带宽会一起崩。
//!
//! 与本 crate 的边界：
//! - 这里**不做任何 I/O**，也不知道 librqbit 的存在。它只维护「活跃集合 + 使用顺序」，
//!   返回需要被暂停的任务号；真正的 `pause` / `activate` 由调用方执行（见
//!   `coldrevived` 的 `wake_seed`）。这样调度逻辑可以在没有引擎的情况下被单测覆盖。
//! - 顺序用**逻辑时钟**（每次 `activate` 自增），不用 `SystemTime`：同一毫秒内的多次
//!   唤醒若按挂钟比较会出现并列，逐出对象就取决于插入顺序这种偶然因素，测试也无法复现。
//!
//! 语义约定（都在测试里钉住）：
//! - 容量 < 1 会被夹到 1。**不存在「容量 0」**：那样任何唤醒都无法供片，
//!   只会把「配置写错」伪装成「节点坏了」。
//! - `activate` 对已在活跃集合里的任务只刷新顺序，不产生逐出（幂等）。
//! - 满载时逐出**最久未使用**的那个，并在同一次调用里把新任务放进集合。

/// 默认活跃池上限。
///
/// 取值依据 W4.3：单盘冷存储场景下同时供片 8 个种子足以覆盖「多个客户端同时唤醒」，
/// 又不至于让随机读把盘打满。**这是工程默认值，不是性能承诺**。
pub const MAX_ACTIVE_SEEDS: usize = 8;

/// 冷热标记。仅用于对外报告与索引层展示，不参与逐出决策。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Temperature {
    Cold,
    Hot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Entry {
    task: u64,
    /// 逻辑时钟：越大越「近」。
    tick: u64,
}

/// 一次 `activate` 发生了什么。调用方据此决定要不要真的调引擎。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// 本来就在活跃集合里（顺序已刷新）。**不重复调用 `activate`**——
    /// librqbit 对 live 任务 unpause 会报错，重复唤醒会把「已在供片」翻成失败。
    AlreadyActive,
    /// 有空位，直接放入。
    Activated,
    /// 满载，逐出 `evicted` 后放入。
    ActivatedEvicting { evicted: u64 },
}

#[derive(Debug, Clone)]
pub struct LruScheduler {
    cap: usize,
    entries: Vec<Entry>,
    clock: u64,
    /// 累计逐出次数（观测用）。含 `set_cap` 缩小容量导致的逐出。
    evictions: u64,
}

impl LruScheduler {
    pub fn new(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            entries: Vec::new(),
            clock: 0,
            evictions: 0,
        }
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn evictions(&self) -> u64 {
        self.evictions
    }

    pub fn is_active(&self, task: u64) -> bool {
        self.entries.iter().any(|e| e.task == task)
    }

    /// 活跃任务，按「最久未用 → 最近使用」排序。
    pub fn active(&self) -> Vec<u64> {
        let mut v = self.entries.clone();
        v.sort_by_key(|e| e.tick);
        v.into_iter().map(|e| e.task).collect()
    }

    /// 最久未用的活跃任务；空集合返回 `None`。用于诊断输出，不触发任何动作。
    pub fn oldest(&self) -> Option<u64> {
        self.entries.iter().min_by_key(|e| e.tick).map(|e| e.task)
    }

    fn touch(&mut self, task: u64) {
        self.clock += 1;
        let tick = self.clock;
        if let Some(e) = self.entries.iter_mut().find(|e| e.task == task) {
            e.tick = tick;
            return;
        }
        self.entries.push(Entry { task, tick });
    }

    /// 申请让 `task` 进入活跃集合。
    pub fn activate(&mut self, task: u64) -> Admission {
        if self.is_active(task) {
            self.touch(task);
            return Admission::AlreadyActive;
        }
        if self.entries.len() < self.cap {
            self.touch(task);
            return Admission::Activated;
        }
        // 满载：逐出最久未用者。先移出再 touch，避免把刚插入的当最旧。
        let victim = self
            .entries
            .iter()
            .min_by_key(|e| e.tick)
            .map(|e| e.task)
            .expect("满载时集合必然非空");
        self.entries.retain(|e| e.task != victim);
        self.evictions += 1;
        self.touch(task);
        Admission::ActivatedEvicting { evicted: victim }
    }

    /// 主动移出（任务停止/销毁/暂停）。返回是否真的在集合里。
    pub fn deactivate(&mut self, task: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.task != task);
        self.entries.len() != before
    }

    /// 调整容量（配置热更新）。返回被逐出的任务，按「最久未用优先」，
    /// 调用方须逐个暂停——**只改数字不暂停引擎，等于让活跃数超过上限**。
    pub fn set_cap(&mut self, cap: usize) -> Vec<u64> {
        self.cap = cap.max(1);
        let mut evicted = Vec::new();
        while self.entries.len() > self.cap {
            let Some(victim) = self.oldest() else { break };
            self.entries.retain(|e| e.task != victim);
            self.evictions += 1;
            evicted.push(victim);
        }
        evicted
    }
}

impl Default for LruScheduler {
    fn default() -> Self {
        Self::new(MAX_ACTIVE_SEEDS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_clamps_capacity_to_one() {
        // 容量 0 会让任何唤醒都无法供片，看起来像节点坏了——夹到 1。
        let s = LruScheduler::new(0);
        assert_eq!(s.cap(), 1);
        assert!(s.is_empty());
    }

    #[test]
    fn fills_until_capacity_without_eviction() {
        let mut s = LruScheduler::new(2);
        assert_eq!(s.activate(10), Admission::Activated);
        assert_eq!(s.activate(20), Admission::Activated);
        assert_eq!(s.len(), 2);
        assert_eq!(s.evictions(), 0);
        assert_eq!(s.active(), vec![10, 20]);
    }

    #[test]
    fn full_pool_evicts_least_recently_used() {
        let mut s = LruScheduler::new(2);
        s.activate(10);
        s.activate(20);
        // 20 是最近使用；10 最旧。再来 30 必须逐出 10。
        assert_eq!(s.activate(30), Admission::ActivatedEvicting { evicted: 10 });
        assert_eq!(s.active(), vec![20, 30]);
        assert_eq!(s.evictions(), 1);
        assert!(!s.is_active(10));
    }

    #[test]
    fn repeated_activation_refreshes_order_and_is_idempotent() {
        let mut s = LruScheduler::new(2);
        s.activate(10);
        s.activate(20);
        // 唤醒 10 是正常流量（客户端重试、重复点击），必须只刷新顺序、不再 activate 引擎。
        assert_eq!(s.activate(10), Admission::AlreadyActive);
        assert_eq!(s.active(), vec![20, 10]);
        assert_eq!(s.activate(30), Admission::ActivatedEvicting { evicted: 20 });
        assert_eq!(s.active(), vec![10, 30]);
        assert_eq!(s.evictions(), 1, "AlreadyActive 不得计入逐出");
    }

    #[test]
    fn deactivate_frees_a_slot() {
        let mut s = LruScheduler::new(1);
        s.activate(7);
        assert!(s.deactivate(7));
        assert!(!s.deactivate(7));
        assert_eq!(s.activate(8), Admission::Activated);
        assert_eq!(s.evictions(), 0);
    }

    #[test]
    fn set_cap_shrinks_by_lru_order_and_reports_victims() {
        let mut s = LruScheduler::new(4);
        for t in [1, 2, 3, 4] {
            s.activate(t);
        }
        s.activate(2); // 2 变最近；最旧顺序应为 1,3,4,2
        assert_eq!(s.set_cap(2), vec![1, 3]);
        assert_eq!(s.active(), vec![4, 2]);
        assert_eq!(s.cap(), 2);
        assert_eq!(s.evictions(), 2);
        // 缩小后再放入仍受新容量约束。
        assert_eq!(s.activate(9), Admission::ActivatedEvicting { evicted: 4 });
    }

    #[test]
    fn set_cap_zero_clamps_and_keeps_newest() {
        let mut s = LruScheduler::new(3);
        for t in [1, 2, 3] {
            s.activate(t);
        }
        assert_eq!(s.set_cap(0), vec![1, 2]);
        assert_eq!(s.active(), vec![3]);
        assert_eq!(s.cap(), 1);
    }

    #[test]
    fn oldest_matches_active_front() {
        let mut s = LruScheduler::new(8);
        assert_eq!(s.oldest(), None);
        s.activate(5);
        s.activate(6);
        assert_eq!(s.oldest(), Some(5));
        assert_eq!(s.oldest(), s.active().first().copied());
    }
}
