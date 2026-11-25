//! NFR-02 第二分句：**造种任务仅以 Idle 优先级运行**。
//!
//! 为什么落在**线程**上而不是进程上：同一进程里还有唤醒关键路径（cr-guard 读 68 B
//! 握手、cr-data-plane 的 UDP 数据面、HTTP API）。把整个进程降到 Idle 会把 NFR-04
//! 的唤醒 P95 一起拖下去——那是拿一个门禁换另一个门禁，不算解决。
//!
//! 为什么只能到「引擎 worker 线程」这一粒度：librqbit 只有一个线程池，做种、分片校验、
//! 下载都跑在它的 worker 上，库里没有「单 torrent 线程」这层。所以本模块能保证的边界是
//! **引擎工作线程整体 Idle**，无法再细到「只降做种任务、不降下载任务」。这条边界写在
//! `EngineOptions::idle_threads` 的文档里，`/health` 的 `idle_priority` 也如实呈现。
//!
//! 平台：
//! - **Linux**：`setpriority(PRIO_PROCESS, 0, 19)` + `sched_setscheduler(0, SCHED_IDLE)`。
//!   两者都对**调用线程**生效（Linux 的 nice 是 per-thread）。`SCHED_IDLE` 是
//!   「仅当没有别的可运行任务时才被调度」，比 nice 19 更强；若内核/权限拒绝，只保留
//!   nice 19 并如实上报成 `NiceOnly`，不假装成功。
//! - **Windows**：`SetThreadPriority(THREAD_PRIORITY_IDLE)`（-15，比
//!   `IDLE_PRIORITY_CLASS` 那档更低）。
//! - **macOS**：**未实现**，返回 `Unsupported`。darwin 有 `PRIO_DARWIN_BG` 这条路
//!   （`setpriority(PRIO_DARWIN_THREAD, 0, PRIO_DARWIN_BG)`），但本仓库没有 mac 机器
//!   实测过——按本项目的规矩，「没测过」不许写成「支持」。健康面会显式报 `unsupported`。
//!
//! **一律读回校验**：系统调用返回 0 不等于状态真的变了（cgroup、seccomp、LSM 都可能
//! 让调用「成功」却无效果）。读回的是 `/proc/thread-self/stat` 的 `nice` 与 `policy`
//! （Linux）或 `GetThreadPriority`（Windows），判定只看读回值。
//!
//! # 钩子为什么能覆盖到 worker 线程（tokio 1.54.1 的实测结论）
//!
//! 这一点不看实现会误判：`Builder::on_thread_start` 的回调在 tokio 1.54.1 里**只由
//! 阻塞池** `blocking::pool::Inner::run` 调用（`after_start`），多线程调度器的
//! `worker.rs` 里没有第二处调用点。之所以照样覆盖到 worker，是因为
//! `scheduler/multi_thread/worker.rs::Launch::launch` 把每个 worker 交给
//! `runtime::spawn_blocking(move || run(worker))` 起线程——**worker 本身就是阻塞池线程**，
//! 于是它们既拿到同一个 `thread_name`，也走同一个 `after_start`。
//!
//! 这是**版本相关的实现细节**，不是 tokio 的公开契约。所以：
//! - 代码不靠「钩子一定被调用」来声称达标，靠的是**读回**（`/health.idle_priority.applied`
//!   与探针里逐个 tid 的 nice/policy）；
//! - 升级 tokio 后若 worker 不再走阻塞池，表现会是 `applied` 变小 / 探针里出现
//!   `nice=0` 的引擎线程——**看得见**，不会静默变成「声称降了其实没降」。
//! - `runtime_worker_task_runs_at_idle_priority` 这条测试就是钉这个行为的：它在
//!   **worker 上实际执行的 task 里**读自己的线程优先级。

use std::sync::atomic::{AtomicU64, Ordering};

/// 降到最低的静态优先级（Linux `nice` 上限）。
pub const IDLE_NICE: i32 = 19;

/// 降级结果。**这是读回校验后的状态**，不是「调用有没有返回 0」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleOutcome {
    /// Linux：nice 19 **且** `SCHED_IDLE` 都生效（最强的一档）。
    NiceAndSchedIdle,
    /// Linux：nice 19 生效，但 `SCHED_IDLE` 没切上（权限/内核限制）。
    NiceOnly,
    /// Windows：`THREAD_PRIORITY_IDLE` 生效。
    ThreadIdle,
    /// 本平台未实现（当前是 macOS 及其它）。**不当作失败**，但也不当作达标。
    Unsupported,
}

impl IdleOutcome {
    /// 是否真的处于 Idle 类。`Unsupported` 一律 `false`。
    pub fn is_idle(self) -> bool {
        !matches!(self, IdleOutcome::Unsupported)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            IdleOutcome::NiceAndSchedIdle => "nice19+sched_idle",
            IdleOutcome::NiceOnly => "nice19",
            IdleOutcome::ThreadIdle => "thread_priority_idle",
            IdleOutcome::Unsupported => "unsupported",
        }
    }
}

/// 一个线程的优先级读数（读回值，不是设置意图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreadPriority {
    /// Linux `nice`（-20..19）。
    pub nice: i32,
    /// Linux 调度策略号：0 = `SCHED_OTHER`，5 = `SCHED_IDLE`。非 Linux 恒 0。
    pub policy: i32,
}

/// 进程内累计计数，供 `/health` 呈现「到底降了几个线程」。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrioStats {
    pub applied: u64,
    pub failed: u64,
    pub unsupported: u64,
}

static APPLIED: AtomicU64 = AtomicU64::new(0);
static FAILED: AtomicU64 = AtomicU64::new(0);
static UNSUPPORTED: AtomicU64 = AtomicU64::new(0);

pub fn stats() -> PrioStats {
    PrioStats {
        applied: APPLIED.load(Ordering::Relaxed),
        failed: FAILED.load(Ordering::Relaxed),
        unsupported: UNSUPPORTED.load(Ordering::Relaxed),
    }
}

/// 把**调用线程**降到 Idle。必须在目标线程内调用（不要在 A 线程里调、指望降 B）。
///
/// 返回读回校验后的真实状态；失败时返回错误文本，调用方必须如实上报，
/// 不能把 `Err` 当成「大概降了」。
pub fn set_current_thread_idle() -> Result<IdleOutcome, String> {
    match imp::down() {
        Ok(o) => {
            if o == IdleOutcome::Unsupported {
                UNSUPPORTED.fetch_add(1, Ordering::Relaxed);
            } else {
                APPLIED.fetch_add(1, Ordering::Relaxed);
            }
            Ok(o)
        }
        Err(e) => {
            FAILED.fetch_add(1, Ordering::Relaxed);
            Err(e)
        }
    }
}

/// 读回当前线程的优先级。拿不到返回 `None`（**不返回 0 冒充**）。
pub fn current_thread_priority() -> Option<ThreadPriority> {
    imp::read_back()
}

// ---------------------------------------------------------------- Linux

#[cfg(target_os = "linux")]
mod imp {
    use super::{IdleOutcome, ThreadPriority, IDLE_NICE};

    const PRIO_PROCESS: i32 = 0;
    const SCHED_IDLE: i32 = 5;

    #[repr(C)]
    struct SchedParam {
        sched_priority: i32,
    }

    extern "C" {
        fn setpriority(which: i32, who: u32, prio: i32) -> i32;
        fn sched_setscheduler(pid: i32, policy: i32, param: *const SchedParam) -> i32;
    }

    /// 读 `/proc/thread-self/stat` 的 nice 与 policy。
    ///
    /// **必须用 `thread-self`，不能用 `self`**（这是踩过的坑）：`/proc/self` 指向
    /// 线程组组长，在子线程里读它拿到的是**主线程**的 nice/policy。结果是
    /// 「降级其实成功了，读回却看到 nice=0 policy=0」，然后被误判成失败——
    /// 一个只看代码看不出来的假阴性。`thread-self` 是 Linux 3.17+ 的「调用线程」，
    /// 老内核回落到 `self`（此时子线程里的读数就不可信了，只能如实接受这个限制）。
    ///
    /// comm 字段可能含空格与括号，必须以**最后一个** `)` 为界；`)` 之后第一个
    /// token 是 state（第 3 字段），故字段 n 的下标是 n-3。
    pub(super) fn read_back() -> Option<ThreadPriority> {
        let text = std::fs::read_to_string("/proc/thread-self/stat")
            .or_else(|_| std::fs::read_to_string("/proc/self/stat"))
            .ok()?;
        let rest = &text[text.rfind(')')? + 1..];
        let f: Vec<&str> = rest.split_whitespace().collect();
        Some(ThreadPriority {
            nice: f.get(16)?.parse().ok()?,   // 第 19 字段
            policy: f.get(38)?.parse().ok()?, // 第 41 字段
        })
    }

    pub(super) fn down() -> Result<IdleOutcome, String> {
        let mut notes: Vec<String> = Vec::new();

        if unsafe { setpriority(PRIO_PROCESS, 0, IDLE_NICE) } != 0 {
            notes.push(format!("setpriority: {}", std::io::Error::last_os_error()));
        }
        let sp = SchedParam { sched_priority: 0 };
        if unsafe { sched_setscheduler(0, SCHED_IDLE, &sp) } != 0 {
            notes.push(format!("sched_setscheduler(SCHED_IDLE): {}",
                               std::io::Error::last_os_error()));
        }

        // 判定只看读回值：两个调用「返回 0」都不构成证据。
        let st = read_back().ok_or_else(|| "读回 /proc/self/stat 失败".to_string())?;
        if st.policy == SCHED_IDLE {
            return Ok(IdleOutcome::NiceAndSchedIdle);
        }
        if st.nice == IDLE_NICE {
            return Ok(IdleOutcome::NiceOnly);
        }
        Err(format!(
            "降级未生效（读回 nice={} policy={}）{}",
            st.nice, st.policy,
            if notes.is_empty() { String::new() } else { format!(": {}", notes.join("; ")) }
        ))
    }
}

// ---------------------------------------------------------------- Windows

#[cfg(windows)]
mod imp {
    use super::{IdleOutcome, ThreadPriority};

    const THREAD_PRIORITY_IDLE: i32 = -15;
    const THREAD_PRIORITY_ERROR_RETURN: i32 = 0x7fff_ffff;

    extern "system" {
        fn GetCurrentThread() -> *mut core::ffi::c_void;
        fn SetThreadPriority(h: *mut core::ffi::c_void, p: i32) -> i32;
        fn GetThreadPriority(h: *mut core::ffi::c_void) -> i32;
    }

    /// Windows 没有 `nice` 概念，`policy` 恒 0；真实值是 `GetThreadPriority`。
    /// 为了不让「policy=0」被误读成 Linux 的 `SCHED_OTHER`，这里把 Windows 的
    /// 线程优先级原样放进 `nice` 字段（-15 = `THREAD_PRIORITY_IDLE`），并在
    /// `IdleOutcome` 里用 `ThreadIdle` 明确区分。
    pub(super) fn read_back() -> Option<ThreadPriority> {
        let p = unsafe { GetThreadPriority(GetCurrentThread()) };
        if p == THREAD_PRIORITY_ERROR_RETURN {
            return None;
        }
        Some(ThreadPriority { nice: p, policy: 0 })
    }

    pub(super) fn down() -> Result<IdleOutcome, String> {
        let ok = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_IDLE) };
        if ok == 0 {
            return Err(format!("SetThreadPriority 失败: {}",
                               std::io::Error::last_os_error()));
        }
        let st = read_back().ok_or_else(|| "GetThreadPriority 读回失败".to_string())?;
        if st.nice == THREAD_PRIORITY_IDLE {
            return Ok(IdleOutcome::ThreadIdle);
        }
        Err(format!("降级未生效（读回线程优先级 {}）", st.nice))
    }
}

// ---------------------------------------------------------------- 其它平台

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    use super::{IdleOutcome, ThreadPriority};

    pub(super) fn read_back() -> Option<ThreadPriority> {
        // 拿不到就报 None：与 sysres 的 RSS/CPU 一致，绝不编一个 0 出来。
        None
    }

    pub(super) fn down() -> Result<IdleOutcome, String> {
        Ok(IdleOutcome::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 降级必须发生在**目标线程内**。这里在自己的线程里降，避免把测试进程的主线程
    /// 永久拉低（Linux 上非特权进程把 nice 降下去后**升不回来**，会污染同二进制后续用例）。
    #[test]
    fn idle_applies_to_the_calling_thread_only() {
        let before = current_thread_priority();
        let h = std::thread::spawn(|| {
            let n0 = current_thread_priority();
            let out = set_current_thread_idle();
            (n0, out, current_thread_priority())
        });
        let (n0, out, n1) = h.join().expect("线程不应 panic");
        match out {
            Ok(IdleOutcome::Unsupported) => {
                // 未实现的平台：如实报 unsupported，不允许伪造成功。
                assert!(n1.is_none() || n0 == n1, "报 unsupported 却改了优先级");
            }
            Ok(o) => {
                assert!(o.is_idle(), "{o:?} 必须被视为 idle 类");
                let a = n0.expect("降级前应能读回");
                let b = n1.expect("降级后应能读回");
                #[cfg(target_os = "linux")]
                {
                    assert!(b.nice > a.nice, "nice 未变低: {} -> {}", a.nice, b.nice);
                    assert_eq!(b.nice, IDLE_NICE);
                }
                #[cfg(windows)]
                {
                    assert_eq!(b.nice, -15, "THREAD_PRIORITY_IDLE = -15（原 {}）", a.nice);
                }
                #[cfg(not(any(target_os = "linux", windows)))]
                {
                    let _ = (&a, &b); // 这两个平台分支都不比较数值，但读回必须成功
                }
            }
            Err(e) => panic!("降级失败: {e}"),
        }
        // 主线程不受影响（读回为 None 的平台跳过比较）。
        if let (Some(x), Some(y)) = (before, current_thread_priority()) {
            assert_eq!(x, y, "子线程降级污染了调用线程");
        }
    }

    #[test]
    fn stats_count_each_attempt() {
        // 计数器是**进程级**的，而测试默认并行：别的用例也在降级自己的线程。
        // 因此这里只能断言「对应分桶增长了」与「总量守恒（不串桶）」，
        // 不能断言「恰好 +1」——那样这条用例会因为跑了它的邻居而红灯。
        let s0 = stats();
        let out = std::thread::spawn(set_current_thread_idle).join().unwrap();
        let s1 = stats();
        let grew = |a: u64, b: u64| b > a;
        match out {
            Ok(IdleOutcome::Unsupported) => {
                assert!(grew(s0.unsupported, s1.unsupported), "unsupported 桶没增长");
            }
            Ok(_) => assert!(grew(s0.applied, s1.applied), "applied 桶没增长"),
            Err(_) => assert!(grew(s0.failed, s1.failed), "failed 桶没增长"),
        }
        // 三个桶各自单调，未失败时 failed 不得凭空增长（防止把成功记成失败）。
        assert!(s1.applied >= s0.applied && s1.failed >= s0.failed
                && s1.unsupported >= s0.unsupported);
    }

    /// 钉住「引擎钩子对 **worker** 线程有效」这一行为。
    ///
    /// 不看实现最容易搞错的一点：`on_thread_start` 在 tokio 1.54.1 里只有阻塞池调用，
    /// 多线程 worker 之所以也能被降级，是因为它本身就是阻塞池线程（`Launch::launch`
    /// 用 `spawn_blocking` 起 worker）。这条测试在**真正跑在 worker 上的 task 里**
    /// 读自己的优先级，所以只要哪天 tokio 改了实现，它会立刻变红——而不是让
    /// 「声称降级、实际没降」悄悄进证据。
    #[test]
    fn runtime_worker_task_runs_at_idle_priority() {
        let before = stats();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .on_thread_start(|| {
                let _ = set_current_thread_idle();
            })
            .build()
            .expect("运行时必须能起来");
        // 让 task 跑在 worker 上（block_on 本身在调用线程上推进，读不到 worker）。
        let h = rt.spawn(async { (current_thread_priority(), std::thread::current().name().map(str::to_string)) });
        let (prio_read, name) = rt.block_on(h).expect("task 不应 panic");
        let after = stats();
        rt.shutdown_background();

        assert!(after.applied > before.applied,
                "钩子没有降级任何线程（applied {} -> {}）", before.applied, after.applied);
        match prio_read {
            None => {
                // 平台拿不到读数：只能依据计数器，且必须如实标注未验证。
                assert!(after.unsupported > before.unsupported
                        || cfg!(not(any(target_os = "linux", windows))),
                        "读不到优先级却也没报 unsupported");
            }
            Some(p) => {
                #[cfg(target_os = "linux")]
                {
                    assert!(p.nice == IDLE_NICE || p.policy == 5,
                            "worker 线程没降下来: nice={} policy={}（线程名 {:?}）",
                            p.nice, p.policy, name);
                }
                #[cfg(windows)]
                {
                    assert_eq!(p.nice, -15,
                               "worker 线程没降下来: THREAD_PRIORITY_IDLE 期望 -15（线程名 {:?}）",
                               name);
                }
            }
        }
    }

    #[test]
    fn outcome_strings_are_stable() {
        // `/health` 与归档证据都按这些字符串判读，改名等于改口径。
        assert_eq!(IdleOutcome::NiceAndSchedIdle.as_str(), "nice19+sched_idle");
        assert_eq!(IdleOutcome::NiceOnly.as_str(), "nice19");
        assert_eq!(IdleOutcome::ThreadIdle.as_str(), "thread_priority_idle");
        assert_eq!(IdleOutcome::Unsupported.as_str(), "unsupported");
        assert!(!IdleOutcome::Unsupported.is_idle());
    }
}
