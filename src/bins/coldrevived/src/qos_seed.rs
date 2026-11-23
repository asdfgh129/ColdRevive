//! FR-16：把 `cr-qos` 令牌桶接到 BT 做种上传。
//!
//! 两条真实约束：
//! 1. 引擎会话的 `upload_bps` 设为 ratio 换算出的**硬上限**（由 `start_engine_seeder` 读取）；
//! 2. 采样线程每秒从 `EngineProgress.uploaded_bytes` 读真实累计字节，差额喂给令牌桶。
//!
//! 这里**不宣称 LEDBAT 或自动退让**：S5 判定仍是 `not_run`，本实现只是字节预算与硬上限。

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cr_core::Json;
use cr_engine::Engine;

use crate::p1_config::P1Config;
use crate::NodeState;

#[derive(Debug)]
pub struct SeedingQos {
    governor: cr_api::qos::UploadGovernor,
}

impl SeedingQos {
    pub fn from_p1(p1: &P1Config) -> Self {
        Self { governor: cr_api::qos::UploadGovernor::new(p1.qos.clone()) }
    }

    /// 传给 `EngineOptions.upload_bps` 的值；`None` = 不设引擎上限。
    pub fn engine_cap_bps(&self) -> Option<u32> {
        self.governor.cap_bps().and_then(|c| u32::try_from(c).ok())
    }

    pub fn enabled(&self) -> bool { self.governor.enabled() }

    /// 记入引擎真实累计上传字节，返回本次差值是否在预算内。
    pub fn observe(&mut self, real_uploaded_bytes: u64) -> bool {
        self.governor.observe_uploaded_bytes(real_uploaded_bytes)
    }

    pub fn json(&self) -> Json {
        let cfg = self.governor.config();
        Json::obj(vec![
            ("enabled", Json::b(self.enabled())),
            ("background_upload_ratio", Json::n(cfg.ratio)),
            ("reference_link_bps", Json::n(cfg.reference_link_bps as f64)),
            ("cap_bps", match self.governor.cap_bps() {
                Some(c) => Json::n(c as f64), None => Json::Null }),
            ("observed_bytes", Json::n(self.governor.observed_bytes() as f64)),
            ("allowed_bytes", Json::n(self.governor.allowed_bytes() as f64)),
            ("denied_bytes", Json::n(self.governor.denied_bytes() as f64)),
            ("note", Json::s("字节预算 + 引擎硬上限；不是 LEDBAT，S5 仍 not_run")),
        ])
    }
}

/// 每秒采样一次真实做种上传字节。未启用时不启动线程（调用方保证）。
pub fn spawn(state: Arc<Mutex<NodeState>>) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(1));
        let total = {
            let st = state.lock().unwrap();
            let mut total = 0u64;
            if let Ok(guard) = st.engine.lock() {
                if let Some(engine) = guard.as_ref() {
                    for task in st.seed_tasks.values() {
                        if let Some(p) = engine.progress(*task) {
                            total = total.saturating_add(p.uploaded_bytes);
                        }
                    }
                }
            }
            total
        };
        let mut st = state.lock().unwrap();
        st.qos.observe(total);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ratio_sets_a_real_engine_cap() {
        let q = SeedingQos::from_p1(&P1Config::default());
        assert!(q.enabled());
        assert_eq!(q.engine_cap_bps(), Some(250_000));
        let j = q.json();
        assert_eq!(j.get("background_upload_ratio").and_then(|x| match x {
            Json::Num(n) => Some(*n), _ => None }), Some(0.02));
    }

    #[test]
    fn ratio_zero_leaves_engine_untouched() {
        let mut p1 = P1Config::default();
        p1.qos.ratio = 0.0;
        let q = SeedingQos::from_p1(&p1);
        assert!(!q.enabled());
        assert_eq!(q.engine_cap_bps(), None);
        assert_eq!(q.json().get("cap_bps").cloned(), Some(Json::Null));
    }

    #[test]
    fn observes_only_real_engine_deltas() {
        let mut p1 = P1Config::default();
        p1.qos.ratio = 1.0;
        p1.qos.reference_link_bps = 100;
        let mut q = SeedingQos::from_p1(&p1);
        assert!(q.observe(1000)); // 基线
        assert_eq!(q.json().get("observed_bytes").and_then(Json::as_i64), Some(0));
        assert!(!q.observe(1200)); // 真实多了 200 B，超过 100 B 硬上限
        let j = q.json();
        assert_eq!(j.get("observed_bytes").and_then(Json::as_i64), Some(200));
        assert_eq!(j.get("denied_bytes").and_then(Json::as_i64), Some(200));
    }
}
