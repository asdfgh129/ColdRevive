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
    /// 进程 CPU 占用（千分百分点：1000 = 单核 1%）。`None` = 本平台拿不到，
    /// **不是 0**。多线程 CPU 时间求和，因此可以超过 100_000。
    pub cpu_millipct: Option<u64>,
}

impl Sample {
    pub fn unknown() -> Self {
        Sample { at_secs: 0, rss_mb: None, peers: 0, up_bps: 0,
                 uploaded_bytes: 0, cpu_millipct: None }
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

/// 进程累计 CPU 时间（毫秒）。拿不到返回 `None`，**绝不返回 0 冒充**。
///
/// Linux 的 `utime+stime` 单位是 clock tick，必须用 `sysconf(_SC_CLK_TCK)`
/// 换算；写死 100 会在非标准 tick 的机器上把一个阈值变成另一个阈值。
/// Windows 的 `FILETIME` 单位是 100 ns。macOS 没有 std-only 的读法，如实返回 None。
#[cfg(target_os = "linux")]
extern "C" {
    fn sysconf(name: i32) -> i64;
}

#[cfg(target_os = "linux")]
pub fn cpu_ms() -> Option<u64> {
    const SC_CLK_TCK: i32 = 2;
    let text = std::fs::read_to_string("/proc/self/stat").ok()?;
    // comm 字段可能带空格和括号，必须以**最后一个** ')' 为界。
    let rest = &text[text.rfind(')')? + 1..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // ')' 之后第一个是 state（第 3 字段），所以 utime（第 14）在下标 11。
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    let hz = unsafe { sysconf(SC_CLK_TCK) };
    let hz = if hz > 0 { hz as u64 } else { 100 };
    Some(utime.saturating_add(stime).saturating_mul(1000) / hz)
}

#[cfg(windows)]
pub fn cpu_ms() -> Option<u64> {
    win::cpu_ms()
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn cpu_ms() -> Option<u64> {
    None
}

/// CPU 速率的差分记账。与上传字节分开：`None` 表示这次没读到，不清空基线，
/// 下次读到时用「两次成功读数」之间的间隔计算，避免短暂失败造出假尖峰。
#[derive(Clone, Debug)]
struct CpuClock {
    prev: Option<(Instant, u64)>,
}

impl CpuClock {
    fn new() -> Self {
        CpuClock { prev: None }
    }

    fn observe(&mut self, now: Instant, cpu_ms: Option<u64>) -> Option<u64> {
        let Some(raw) = cpu_ms else { return None };
        let Some((t0, c0)) = self.prev else {
            self.prev = Some((now, raw));
            return None;
        };
        self.prev = Some((now, raw));
        let dt_ms = now.saturating_duration_since(t0).as_millis();
        if dt_ms < 200 {
            return None;
        }
        let delta = raw.saturating_sub(c0) as u128;
        // CPU 毫秒 / 墙上毫秒 = 核数占比；*100000 换算成千分百分点。
        let v = delta.saturating_mul(100_000) / dt_ms as u128;
        Some(u64::try_from(v).unwrap_or(u64::MAX))
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

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    impl FileTime {
        fn ticks(self) -> u64 {
            ((self.high as u64) << 32) | self.low as u64
        }
    }

    extern "system" {
        fn GetProcessTimes(
            process: *mut core::ffi::c_void,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }

    pub fn cpu_ms() -> Option<u64> {
        unsafe {
            let zero = FileTime { low: 0, high: 0 };
            let (mut creation, mut exit, mut kernel, mut user) = (zero, zero, zero, zero);
            let ok = GetProcessTimes(GetCurrentProcess(), &mut creation, &mut exit,
                                     &mut kernel, &mut user);
            if ok == 0 {
                return None;
            }
            // 100 ns -> 1 ms
            Some(kernel.ticks().saturating_add(user.ticks()) / 10_000)
        }
    }
}

/// 采样线程的记账。放在 `NodeState` 里，准入判定只是读它。
#[derive(Clone, Debug)]
pub struct Meter {
    pub sample: Sample,
    prev: Option<(Instant, u64)>,
    cpu_prev: CpuClock,
    started: Instant,
}

impl Meter {
    pub fn new() -> Self {
        Meter { sample: Sample::unknown(), prev: None,
                cpu_prev: CpuClock::new(), started: Instant::now() }
    }

    /// 用累计上传字节推进一次采样。
    ///
    /// 第一次调用只建立基准（`up_bps` 记 0），因为「从 0 到某个值」的差分
    /// 会把进程启动以来的全部流量算进一个间隔，得到一个假的尖峰。
    pub fn observe(&mut self, uploaded_bytes: u64, peers: usize, rss: Option<u64>,
                   cpu_ms: Option<u64>, now: Instant) {
        // 先推进 CPU 基线：即使这一轮上传间隔太短而提前返回，CPU 基线也不能丢。
        let cpu = self.cpu_prev.observe(now, cpu_ms).or(self.sample.cpu_millipct);
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
                    self.sample.cpu_millipct = cpu;
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
            cpu_millipct: cpu,
        };
    }
}

/// 是否有任何一条需要采样的阈值被配置。没有就不起线程，空转也是噪声。
pub fn needed(cfg: &cr_core::Config) -> bool {
    cfg.limits_max_peers.is_some()
        || cfg.limits_max_upload_bytes_per_sec.is_some()
        || cfg.limits_max_rss_mb.is_some()
        || cfg.limits_max_seeding_cpu_millipct.is_some()
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
            let cpu = cpu_ms();
            let now = Instant::now();
            st.res.observe(uploaded, peers, rss, cpu, now);
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
        m.observe(1_000, 3, Some(64), Some(0), t0);
        assert_eq!(m.sample.up_bps, 0, "第一次采样不能把累计量当成速率");
        assert_eq!(m.sample.peers, 3);
        assert_eq!(m.sample.rss_mb, Some(64));
    }

    #[test]
    fn rate_is_delta_over_elapsed() {
        let mut m = Meter::new();
        let t0 = Instant::now();
        m.observe(0, 0, Some(10), Some(0), t0);
        m.observe(2_000, 1, Some(11), Some(20), t0 + Duration::from_secs(2));
        // 2000 B / 2 s = 1000 B/s（允许 1 B 的取整误差）
        assert!((999..=1001).contains(&m.sample.up_bps), "got {}", m.sample.up_bps);
        assert_eq!(m.sample.peers, 1);
    }

    #[test]
    fn too_short_interval_does_not_fabricate_spike() {
        let mut m = Meter::new();
        let t0 = Instant::now();
        m.observe(0, 0, None, Some(0), t0);
        m.observe(5_000, 0, None, Some(1), t0 + Duration::from_secs(1));
        let steady = m.sample.up_bps;
        // 同一个字节量、间隔 0 ms：不能算出「无穷大」。
        m.observe(10_000, 0, None, Some(1), t0 + Duration::from_secs(1));
        assert!(m.sample.up_bps <= steady.max(1), "短间隔放大了瞬时抖动");
    }

    #[test]
    fn cpu_rate_is_cpu_delta_over_wall_delta() {
        let mut m = Meter::new();
        let t0 = Instant::now();
        m.observe(0, 0, Some(10), Some(0), t0);
        assert_eq!(m.sample.cpu_millipct, None, "第一次只建基线");
        // 25 ms CPU / 1000 ms 墙上时间 = 2.5% = 2500 千分百分点。
        m.observe(0, 0, Some(10), Some(25), t0 + Duration::from_secs(1));
        assert_eq!(m.sample.cpu_millipct, Some(2500));
    }

    #[test]
    fn cpu_unavailable_is_not_zero_and_keeps_last_known_value() {
        let mut m = Meter::new();
        let t0 = Instant::now();
        m.observe(0, 0, None, None, t0);
        m.observe(0, 0, None, None, t0 + Duration::from_secs(1));
        assert_eq!(m.sample.cpu_millipct, None, "读不到 CPU 不能写成 0");
        m.observe(0, 0, None, Some(0), t0 + Duration::from_secs(2));
        m.observe(0, 0, None, Some(10), t0 + Duration::from_secs(3));
        assert_eq!(m.sample.cpu_millipct, Some(1000));
        m.observe(0, 0, None, None, t0 + Duration::from_secs(4));
        assert_eq!(m.sample.cpu_millipct, Some(1000), "短暂读失败不能抹掉已知值");
    }

    #[test]
    fn missing_rss_stays_none_and_is_not_zeroed() {
        let mut m = Meter::new();
        m.observe(0, 0, None, None, Instant::now());
        assert_eq!(m.sample.rss_mb, None, "读不到 RSS 不能写成 0");
        // 一旦读到过，后续某次读失败也不该把已知值抹成 0。
        m.observe(0, 0, Some(123), None, Instant::now() + Duration::from_secs(1));
        m.observe(0, 0, None, None, Instant::now() + Duration::from_secs(2));
        assert_eq!(m.sample.rss_mb, Some(123));
    }
}
