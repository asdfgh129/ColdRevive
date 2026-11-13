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
}
