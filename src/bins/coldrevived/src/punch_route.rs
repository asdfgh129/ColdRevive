//! ADR-022：守护进程侧的**打洞优先 / 中继兜底**控制面状态（opt-in）。
//!
//! 线格式与收发循环在 `cr_nat::punch_wire`（那里能用真 `RelayServer` 跑集成测试）；
//! 本模块只保留**守护进程观测**：是否启用、尝试/直连/中继/失败计数。
//!
//! 边界（与 `punch_wire` 保持一致，不要在这里放宽）：
//! 1. 只做判据和交接：是否打通由 `tcp_punch` 的 `poll+SO_ERROR` 决定。
//! 2. `Route::Relay` 只表示不登记 punched socket。**v2.4.92 起** TCP 中继上确实有
//!    BT 数据面（`relay_sub` 的 OPEN/splice + `data_plane` 的 OPENED/桥接），但 UDP 承载
//!    仍没有重传层，所以 `relay_data_plane_udp` 独立标 `not_implemented`——
//!    两种承载的结论不能混着读。
//! 3. 默认关闭：`nat.punch_route=false` 时 `proxy_url` 保持 None，既有部署不变。

pub use cr_nat::punch_wire::{
    exchange_verdicts, negotiate_route, session_nonce, verify_peer,
};

use cr_core::{Config, Json};
use cr_nat::punch_route::Route;
use std::sync::atomic::{AtomicU64, Ordering};

/// 守护进程侧的协商观测。只有诊断用途，不参与任何门禁判定。
#[derive(Debug, Default)]
pub struct PunchRouteState {
    pub enabled: bool,
    pub attempts: AtomicU64,
    pub direct: AtomicU64,
    pub relay: AtomicU64,
    pub failed: AtomicU64,
}

impl PunchRouteState {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            enabled: cfg.nat_punch_route,
            ..Default::default()
        }
    }

    pub fn record(&self, route: Option<Route>) {
        self.attempts.fetch_add(1, Ordering::Relaxed);
        match route {
            Some(Route::Direct) => {
                self.direct.fetch_add(1, Ordering::Relaxed);
            }
            Some(Route::Relay) => {
                self.relay.fetch_add(1, Ordering::Relaxed);
            }
            _ => {
                self.failed.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn json(&self) -> Json {
        Json::obj(vec![
            ("enabled", Json::b(self.enabled)),
            ("attempts", Json::n(self.attempts.load(Ordering::Relaxed) as f64)),
            ("direct", Json::n(self.direct.load(Ordering::Relaxed) as f64)),
            ("relay", Json::n(self.relay.load(Ordering::Relaxed) as f64)),
            ("failed", Json::n(self.failed.load(Ordering::Relaxed) as f64)),
            // 承载维度分开写：TCP 中继有真数据面（ADR-023），UDP 承载没有重传层。
            // 合成一个字段会让人把「有一条通了」读成「随便哪条都通」。
            ("relay_data_plane", Json::s("tcp-carrier-implemented")),
            ("relay_data_plane_udp", Json::s("not_implemented")),
            ("is_s6_evidence", Json::b(false)),
            ("g2_claim", Json::b(false)),
            ("s6_status", Json::s("not_run")),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonce_is_not_all_zero() {
        // 不能拿常量 0 当会话 id：那样连续两次唤醒共享同一 nonce，旧帧可冒充本轮确认。
        let n = session_nonce();
        assert!(n.iter().any(|b| *b != 0));
    }

    #[test]
    fn state_defaults_to_disabled_and_records_only_real_routes() {
        let st = PunchRouteState::default();
        let j = st.json();
        assert_eq!(j.get("enabled").and_then(Json::as_bool), Some(false));
        st.record(Some(Route::Direct));
        st.record(Some(Route::Relay));
        st.record(None);
        let j = st.json();
        assert_eq!(j.get("attempts").and_then(Json::as_i64), Some(3));
        assert_eq!(j.get("direct").and_then(Json::as_i64), Some(1));
        assert_eq!(j.get("relay").and_then(Json::as_i64), Some(1));
        assert_eq!(j.get("failed").and_then(Json::as_i64), Some(1));
        assert_eq!(j.get("g2_claim").and_then(Json::as_bool), Some(false));
        assert_eq!(j.get("relay_data_plane").and_then(Json::as_str),
                   Some("tcp-carrier-implemented"));
        assert_eq!(j.get("relay_data_plane_udp").and_then(Json::as_str),
                   Some("not_implemented"));
    }
}
