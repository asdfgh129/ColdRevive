//! FR-09：控制面按来源 IP 的令牌桶限流。
//!
//! 为什么放在守护进程而不是每处 handler 里各写一遍：`/cli`、`/jsonrpc`、`/inject`
//! 走的是同一个 `handle_conn`，只有在这里限流才能保证「换个路径就绕过」不会发生。
//!
//! 纪律：
//! - **默认关闭**（配置不给 `api.rate_per_sec` 就完全不建桶），既有证据行为不变；
//! - 被拒必须**说清原因**：返回 429 + `reason=rate_limited` + `retry_after_ms`，
//!   不伪装成连接重置或空响应（那会把配额问题读成「服务挂了」）；
//! - 只记来源 IP，不反查主机名、不做反向 DNS（限流器自己不能变成慢查询）。

use std::collections::HashMap;
use std::time::{Duration, Instant};

struct Bucket {
    tokens: f64,
    last: Instant,
}

pub struct IpLimiter {
    per_sec: f64,
    burst: f64,
    buckets: HashMap<String, Bucket>,
    /// 桶数量上限：来源 IP 可能是伪造的，字典不能无限长。
    max_ips: usize,
}

/// 限流判决。`Err` 带「多久之后可以重试」，直接进响应体。
pub type Verdict = Result<(), Duration>;

impl IpLimiter {
    pub fn new(per_sec: u32, burst: Option<u32>) -> IpLimiter {
        let per_sec = per_sec.max(1) as f64;
        IpLimiter {
            per_sec,
            burst: burst.unwrap_or(per_sec as u32).max(1) as f64,
            buckets: HashMap::new(),
            max_ips: 4096,
        }
    }

    pub fn per_sec(&self) -> f64 { self.per_sec }
    pub fn burst(&self) -> f64 { self.burst }
    pub fn tracked_ips(&self) -> usize { self.buckets.len() }

    /// 消耗 1 个令牌。`now` 显式传入，测试才能不 sleep 地验证补充速率。
    pub fn check_at(&mut self, ip: &str, now: Instant) -> Verdict {
        if self.buckets.len() >= self.max_ips && !self.buckets.contains_key(ip) {
            // 表满：优先淘汰「按当前速率已经回满」的桶（等价于从未用过）。
            let full_after = Duration::from_secs_f64(self.burst / self.per_sec);
            let idle: Vec<String> = self.buckets.iter()
                .filter(|(_, b)| now.duration_since(b.last) >= full_after)
                .map(|(k, _)| k.clone())
                .collect();
            for k in idle {
                self.buckets.remove(&k);
            }
            if self.buckets.len() >= self.max_ips {
                // 仍然满：拒绝新 IP，而不是把老 IP 全清掉（清掉等于给出绕过手段）。
                return Err(Duration::from_secs(1));
            }
        }
        let per_sec = self.per_sec;
        let burst = self.burst;
        let b = self.buckets.entry(ip.to_string()).or_insert(Bucket { tokens: burst, last: now });
        let elapsed = now.duration_since(b.last).as_secs_f64();
        if elapsed > 0.0 {
            b.tokens = (b.tokens + elapsed * per_sec).min(burst);
            b.last = now;
        }
        if b.tokens >= 1.0 {
            b.tokens -= 1.0;
            Ok(())
        } else {
            let need = (1.0 - b.tokens) / per_sec;
            Err(Duration::from_secs_f64(need.max(0.001)))
        }
    }

    pub fn check(&mut self, ip: &str) -> Verdict {
        self.check_at(ip, Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_is_allowed_then_rejected_with_retry_hint() {
        let mut l = IpLimiter::new(2, Some(3));
        let t0 = Instant::now();
        for i in 0..3 {
            assert!(l.check_at("10.0.0.1", t0).is_ok(), "突发第 {} 次应放行", i + 1);
        }
        let wait = l.check_at("10.0.0.1", t0).unwrap_err();
        assert!(wait > Duration::ZERO, "被拒时必须给出重试等待");
        assert!(wait <= Duration::from_millis(500), "2/s 的桶不该让人等 1s 以上: {wait:?}");
    }

    #[test]
    fn tokens_refill_over_time() {
        let mut l = IpLimiter::new(2, Some(2));
        let t0 = Instant::now();
        assert!(l.check_at("10.0.0.1", t0).is_ok());
        assert!(l.check_at("10.0.0.1", t0).is_ok());
        assert!(l.check_at("10.0.0.1", t0).is_err());
        // 500 ms 按 2/s 补 1 个
        assert!(l.check_at("10.0.0.1", t0 + Duration::from_millis(500)).is_ok());
        assert!(l.check_at("10.0.0.1", t0 + Duration::from_millis(500)).is_err());
        // 补满不会超过 burst
        let later = t0 + Duration::from_secs(60);
        for _ in 0..2 {
            assert!(l.check_at("10.0.0.1", later).is_ok());
        }
        assert!(l.check_at("10.0.0.1", later).is_err());
    }

    #[test]
    fn limits_are_per_ip() {
        let mut l = IpLimiter::new(1, Some(1));
        let t0 = Instant::now();
        assert!(l.check_at("10.0.0.1", t0).is_ok());
        assert!(l.check_at("10.0.0.1", t0).is_err());
        assert!(l.check_at("10.0.0.2", t0).is_ok(), "另一个来源不应被牵连");
        assert_eq!(l.tracked_ips(), 2);
    }

    #[test]
    fn ip_table_is_bounded() {
        let mut l = IpLimiter::new(1, Some(1));
        let t0 = Instant::now();
        l.max_ips = 4;
        for i in 0..4 {
            assert!(l.check_at(&format!("10.0.0.{i}"), t0).is_ok());
        }
        // 表满且没有可淘汰的（都很新）：拒绝新 IP，而不是清空老表。
        assert!(l.check_at("10.0.0.9", t0).is_err());
        assert_eq!(l.tracked_ips(), 4);
        // 足够久之后，老桶已回满 → 可以淘汰腾位
        let later = t0 + Duration::from_secs(30);
        assert!(l.check_at("10.0.0.9", later).is_ok());
    }
}
