//! FR-15 剩余阈值：**真实资源读数**与采样。
//!
//! 设计约束（都是踩过的坑换来的）：
//!
//! - **读不到 ≠ 0**。RSS 在某些平台上拿不到（macOS 没有 std-only 的读法），
//!   这时必须报 `None` 并让上层写 `enabled=false/unavailable`，不能退化成「占用 0」
//!   然后让阈值永远不触发——那种「看起来配了、其实没生效」比不配更危险。
//! - **带宽是滞后指标**。它由累计上传字节的差分算出来，描述的是「刚才发生了什么」。
//!   所以它只能作为**拒绝新唤醒**的理由，不能被读成「保证不超线」。
//! - 采样线程只读，不改索引/引擎状态；持锁时间与 `/health` 同量级。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cr_engine::Engine;

use crate::NodeState;

/// 一次采样得到的真实读数。
#[derive(Clone, Debug)]
pub struct Sample {
    /// 采样时刻（相对进程启动的秒数，便于直接进 JSON）。
    pub at_secs: u64,
    /// 进程 RSS（MiB）。`None` = 本平台拿不到，不是 0。
    pub rss_mb: Option<u64>,
    /// 做种任务的**真实**对端连接数（`EngineProgress.peers` 之和）。
    pub peers: usize,
    /// 出向带宽（byte/s），由累计 `uploaded_bytes` 差分得到。
    pub up_bps: u64,
    /// 累计上传字节（差分基准，也用于排错）。
    pub uploaded_bytes: u64,
}

impl Sample {
    pub fn unknown() -> Self {
        Sample { at_secs: 0, rss_mb: None, peers: 0, up_bps: 0, uploaded_bytes: 0 }
    }
}

/// 进程 RSS（MiB）。拿不到返回 `None`，**绝不返回 0 冒充**。
pub fn rss_mb() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        // /proc/self/status 的 VmRSS 单位是 kB，比 statm 的页数更不容易被页大小搞错。
        let text = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
                return Some(kb / 1024);
            }
        }
        None
    }
    #[cfg(windows)]
    {
        win::rss_mb()
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        // macOS 等平台没有 std-only 的读法。如实返回 None，由上层报 unavailable。
        None
    }
}

#[cfg(windows)]
mod win {
    use std::mem::{size_of, zeroed};

    /// `PROCESS_MEMORY_COUNTERS` 只需要到 `WorkingSetSize` 为止的字段，
    /// 但结构体大小必须与系统一致，否则 `GetProcessMemoryInfo` 返回失败。
    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut core::ffi::c_void,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
        fn GetCurrentProcess() -> *mut core::ffi::c_void;
    }

    pub fn rss_mb() -> Option<u64> {
        unsafe {
            let mut c: ProcessMemoryCounters = zeroed();
            c.cb = size_of::<ProcessMemoryCounters>() as u32;
            let ok = GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb);
            if ok == 0 {
                return None;
            }
            Some((c.working_set_size as u64) / (1024 * 1024))
        }
    }
}

/// 采样线程的记账。放在 `NodeState` 里，准入判定只是读它。
#[derive(Clone, Debug)]
pub struct Meter {
    pub sample: Sample,
    prev: Option<(Instant, u64)>,
    started: Instant,
}

impl Meter {
    pub fn new() -> Self {
        Meter { sample: Sample::unknown(), prev: None, started: Instant::now() }
    }

    /// 用累计上传字节推进一次采样。
    ///
    /// 第一次调用只建立基准（`up_bps` 记 0），因为「从 0 到某个值」的差分
    /// 会把进程启动以来的全部流量算进一个间隔，得到一个假的尖峰。
    pub fn observe(&mut self, uploaded_bytes: u64, peers: usize, rss: Option<u64>, now: Instant) {
        let bps = match self.prev {
            None => 0,
            Some((t0, b0)) => {
                let dt = now.saturating_duration_since(t0).as_secs_f64();
                if dt < 0.2 {
                    // 间隔太短会让瞬时抖动放大成假尖峰：这一轮不刷新速率。
                    self.prev = Some((now, uploaded_bytes));
                    self.sample.at_secs = self.started.elapsed().as_secs();
                    self.sample.peers = peers;
                    self.sample.uploaded_bytes = uploaded_bytes;
                    if rss.is_some() {
                        self.sample.rss_mb = rss;
                    }
                    return;
                }
                let delta = uploaded_bytes.saturating_sub(b0) as f64;
                (delta / dt) as u64
            }
        };
        self.prev = Some((now, uploaded_bytes));
        self.sample = Sample {
            at_secs: self.started.elapsed().as_secs(),
            rss_mb: rss.or(self.sample.rss_mb),
            peers,
            up_bps: bps,
            uploaded_bytes,
        };
    }
}

/// 是否有任何一条需要采样的阈值被配置。没有就不起线程，空转也是噪声。
pub fn needed(cfg: &cr_core::Config) -> bool {
    cfg.limits_max_peers.is_some()
        || cfg.limits_max_upload_bytes_per_sec.is_some()
        || cfg.limits_max_rss_mb.is_some()
}

/// 每秒采样一次真实读数写入 `NodeState::res`。返回是否真的起了线程。
pub fn spawn(state: Arc<Mutex<NodeState>>) -> bool {
    {
        let st = state.lock().unwrap();
        if !needed(&st.cfg) {
            return false;
        }
    }
    std::thread::spawn(move || loop {
        {
            let mut st = state.lock().unwrap();
            let (uploaded, peers) = {
                let mut sum = 0u64;
                let mut n = 0usize;
                if let Ok(guard) = st.engine.lock() {
                    if let Some(e) = guard.as_ref() {
                        for task in st.seed_tasks.values() {
                            if let Some(p) = e.progress(*task) {
                                sum = sum.saturating_add(p.uploaded_bytes);
                                n += p.peers as usize;
                            }
                        }
                    }
                }
                (sum, n)
            };
            let rss = rss_mb();
            let now = Instant::now();
            st.res.observe(uploaded, peers, rss, now);
        }
        std::thread::sleep(Duration::from_millis(1000));
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_observe_only_sets_baseline() {
        let mut m = Meter::new();
        let t0 = Instant::now();
        m.observe(1_000, 3, Some(64), t0);
        assert_eq!(m.sample.up_bps, 0, "第一次采样不能把累计量当成速率");
        assert_eq!(m.sample.peers, 3);
        assert_eq!(m.sample.rss_mb, Some(64));
    }

    #[test]
    fn rate_is_delta_over_elapsed() {
        let mut m = Meter::new();
        let t0 = Instant::now();
        m.observe(0, 0, Some(10), t0);
        m.observe(2_000, 1, Some(11), t0 + Duration::from_secs(2));
        // 2000 B / 2 s = 1000 B/s（允许 1 B 的取整误差）
        assert!((999..=1001).contains(&m.sample.up_bps), "got {}", m.sample.up_bps);
        assert_eq!(m.sample.peers, 1);
    }

    #[test]
    fn too_short_interval_does_not_fabricate_spike() {
        let mut m = Meter::new();
        let t0 = Instant::now();
        m.observe(0, 0, None, t0);
        m.observe(5_000, 0, None, t0 + Duration::from_secs(1));
        let steady = m.sample.up_bps;
        // 同一个字节量、间隔 0 ms：不能算出「无穷大」。
        m.observe(10_000, 0, None, t0 + Duration::from_secs(1));
        assert!(m.sample.up_bps <= steady.max(1), "短间隔放大了瞬时抖动");
    }

    #[test]
    fn missing_rss_stays_none_and_is_not_zeroed() {
        let mut m = Meter::new();
        m.observe(0, 0, None, Instant::now());
        assert_eq!(m.sample.rss_mb, None, "读不到 RSS 不能写成 0");
        // 一旦读到过，后续某次读失败也不该把已知值抹成 0。
        m.observe(0, 0, Some(123), Instant::now() + Duration::from_secs(1));
        m.observe(0, 0, None, Instant::now() + Duration::from_secs(2));
        assert_eq!(m.sample.rss_mb, Some(123));
    }
}
