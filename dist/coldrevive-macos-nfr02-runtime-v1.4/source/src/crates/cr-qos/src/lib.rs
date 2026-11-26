//! cr-qos — W7.1 资源治理（M2 骨架 + S5 实测标定）。
//!
//! S5 真实量测（2026-11-13, `tests/spikes/s5/qos_bench.py`）：
//! `background_upload_ratio=0.2` 时前台劣化 **20.12%**（NFR-03 要求 ≤5%），
//! 0.05 → 6.19%，**0.02 → 4.03%**。故 M1/M2 默认值不得沿用 0.2。
//! 注意：本常量只是按实测回调配置，**不等于 S5 判 pass**；`S5.status` 仍 `not_run`，
//! 须在 M1/G3 真实拥塞链路 + rqbit uTP 实测 ≤5% 后由实验执行人回填。

use std::time::Instant;

pub const DEFAULT_BACKGROUND_UPLOAD_RATIO: f64 = 0.02;
pub const S5_MEASURED_DEGRADATION: [(f64, f64); 3] = [
    (0.20, 20.12), (0.05, 6.19), (0.02, 4.03),
];
pub const NFR03_MAX_DEGRADATION_PCT: f64 = 5.0;

#[derive(Debug)]
pub struct TokenBucket {
    capacity: f64,
    tokens: f64,
    rate_per_sec: f64,
    last: Instant,
}

impl TokenBucket {
    pub fn new(rate_per_sec: f64) -> Self {
        TokenBucket {
            capacity: (rate_per_sec * 0.25).max(1.0),
            tokens: (rate_per_sec * 0.25).max(1.0),
            rate_per_sec,
            last: Instant::now(),
        }
    }
    /// 申请 n 字节；不足则拒绝（调用方须在申请前让出，避免前台劣化）。
    pub fn try_take(&mut self, n: f64) -> bool {
        let elapsed = self.last.elapsed().as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.rate_per_sec).min(self.capacity);
        self.last = Instant::now();
        if self.tokens >= n { self.tokens -= n; true } else { false }
    }
}

/// 把 ratio 换算成绝对速率时使用的**名义链路参考值**（byte/s）。
///
/// 100 Mbps ≈ 12.5 MB/s。这不是「测得的家宽带宽」，只是把「后台上传占 2%」翻译成
/// 一个可执行的字节上限所需的参考；真实部署可用 `qos.reference_link_bps` 覆盖。
/// 绝不据此宣称 LEDBAT 或「自动退让」——S5 的判定仍是 `not_run`。
pub const DEFAULT_REFERENCE_LINK_BPS: u64 = 12_500_000;

#[derive(Debug, Clone)]
pub struct UploadGovernorConfig {
    /// 后台做种上传占参考链路的比例。0（或负数/非有限值）= 关闭。
    pub ratio: f64,
    /// 参考链路速率（byte/s）。
    pub reference_link_bps: u64,
}

impl Default for UploadGovernorConfig {
    fn default() -> Self {
        Self {
            ratio: DEFAULT_BACKGROUND_UPLOAD_RATIO,
            reference_link_bps: DEFAULT_REFERENCE_LINK_BPS,
        }
    }
}

impl UploadGovernorConfig {
    /// 引擎侧硬上限（byte/s）。关闭时返回 `None`，调用方不应设置 `upload_bps`。
    pub fn cap_bps(&self) -> Option<u64> {
        if !self.ratio.is_finite() || self.ratio <= 0.0 {
            return None;
        }
        let scaled = (self.ratio * self.reference_link_bps as f64).round() as u64;
        Some(scaled.clamp(1, self.reference_link_bps))
    }
}

/// 后台做种上传的字节账本。
///
/// 输入是引擎**真实累计**的 `uploaded_bytes`；每次只按与上一次的差（单调不回退）
/// 扣减令牌桶。令牌不够时返回 `false` 并累计 `denied_bytes`，调用方可据此把任务
/// 暂停/降速；这里不做任何网络发包，也不编造速率。
#[derive(Debug)]
pub struct UploadGovernor {
    config: UploadGovernorConfig,
    bucket: Option<TokenBucket>,
    last_uploaded: u64,
    baseline_set: bool,
    observed_bytes: u64,
    allowed_bytes: u64,
    denied_bytes: u64,
}

impl UploadGovernor {
    pub fn new(config: UploadGovernorConfig) -> Self {
        let bucket = config.cap_bps().map(|c| TokenBucket::new(c as f64));
        Self {
            config, bucket, last_uploaded: 0, baseline_set: false,
            observed_bytes: 0, allowed_bytes: 0, denied_bytes: 0,
        }
    }

    pub fn enabled(&self) -> bool { self.bucket.is_some() }
    pub fn config(&self) -> &UploadGovernorConfig { &self.config }
    pub fn cap_bps(&self) -> Option<u64> { self.config.cap_bps() }
    pub fn observed_bytes(&self) -> u64 { self.observed_bytes }
    pub fn allowed_bytes(&self) -> u64 { self.allowed_bytes }
    pub fn denied_bytes(&self) -> u64 { self.denied_bytes }

    /// 记入一次真实观测。首帧只建立基线（没有「差值」可扣），返回 `true`。
    pub fn observe_uploaded_bytes(&mut self, real_uploaded: u64) -> bool {
        if !self.baseline_set {
            self.last_uploaded = real_uploaded;
            self.baseline_set = true;
            return true;
        }
        let delta = real_uploaded.saturating_sub(self.last_uploaded);
        self.last_uploaded = real_uploaded;
        self.observed_bytes = self.observed_bytes.saturating_add(delta);
        if delta == 0 { return true; }
        let Some(bucket) = self.bucket.as_mut() else {
            self.allowed_bytes = self.allowed_bytes.saturating_add(delta);
            return true;
        };
        if bucket.try_take(delta as f64) {
            self.allowed_bytes = self.allowed_bytes.saturating_add(delta);
            true
        } else {
            self.denied_bytes = self.denied_bytes.saturating_add(delta);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_ratio_is_calibrated_to_measured_evidence() {
        assert_eq!(DEFAULT_BACKGROUND_UPLOAD_RATIO, 0.02);
        let last = S5_MEASURED_DEGRADATION.last().unwrap();
        assert!(last.1 < NFR03_MAX_DEGRADATION_PCT);
        assert!(S5_MEASURED_DEGRADATION[0].1 > NFR03_MAX_DEGRADATION_PCT);
    }
    #[test]
    fn token_bucket_hard_cap() {
        let mut b = TokenBucket::new(1000.0);
        assert!(b.try_take(200.0));
        assert!(!b.try_take(1_000_000.0));
    }

    #[test]
    fn default_ratio_yields_a_real_bounded_cap() {
        let cfg = UploadGovernorConfig::default();
        assert_eq!(cfg.ratio, 0.02);
        assert_eq!(cfg.cap_bps(), Some(250_000));
        let mut g = UploadGovernor::new(cfg);
        assert!(g.enabled());
        assert_eq!(g.cap_bps(), Some(250_000));
        // 基线建立不记账；之后按真实累计字节的差值扣减。
        assert!(g.observe_uploaded_bytes(100));
        assert_eq!(g.observed_bytes(), 0);
        assert!(g.observe_uploaded_bytes(600));
        assert_eq!(g.observed_bytes(), 500);
        assert_eq!(g.allowed_bytes(), 500);
    }

    #[test]
    fn ratio_zero_disables_governor() {
        let cfg = UploadGovernorConfig { ratio: 0.0, ..Default::default() };
        let mut g = UploadGovernor::new(cfg);
        assert!(!g.enabled());
        assert_eq!(g.cap_bps(), None);
        assert!(g.observe_uploaded_bytes(10));
        assert!(g.observe_uploaded_bytes(10_000));
        assert_eq!(g.denied_bytes(), 0);
        assert_eq!(g.allowed_bytes(), 9_990);
    }

    #[test]
    fn hard_cap_rejects_over_budget_and_counts_real_bytes() {
        let cfg = UploadGovernorConfig { ratio: 1.0, reference_link_bps: 100, };
        let mut g = UploadGovernor::new(cfg);
        assert_eq!(g.cap_bps(), Some(100));
        assert!(g.observe_uploaded_bytes(0));
        // bucket 容量 100 B：一次真实上传 200 B 必须被拒，且按真实字节计数。
        assert!(!g.observe_uploaded_bytes(200));
        assert_eq!(g.observed_bytes(), 200);
        assert_eq!(g.denied_bytes(), 200);
        assert_eq!(g.allowed_bytes(), 0);
        // 引擎累计值回退（重启/换任务）不得变成负差值凭空放行。
        assert!(g.observe_uploaded_bytes(10));
        assert_eq!(g.observed_bytes(), 200);
    }

    #[test]
    fn invalid_ratio_is_off_not_a_guess() {
        for ratio in [-1.0, f64::NAN, f64::INFINITY] {
            let cfg = UploadGovernorConfig { ratio, ..Default::default() };
            assert_eq!(cfg.cap_bps(), None, "非法 ratio 必须按关闭处理");
        }
    }
}
