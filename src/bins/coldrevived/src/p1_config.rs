//! P1 opt-in 配置的读取层。
//!
//! `cr_core::Config` 属于稳定配置结构（本任务不改动它），因此这里**额外**从同一个
//! JSON 文件里读取 P1 新增段：api / qos / nat.auto_map / dht。缺省值一律关闭或
//! 保持既有行为，老配置文件逐字节不变。

use std::path::Path;

use cr_core::Json;

#[derive(Debug, Clone)]
pub struct P1Config {
    pub rpc_secret: Option<String>,
    pub web_ui: bool,
    pub websocket: bool,
    pub qos: cr_api::qos::UploadGovernorConfig,
    pub nat_auto_map: bool,
    /// 仅用于确定性测试的 UPnP LOCATION 覆盖；正常部署留空走 SSDP 发现。
    pub nat_upnp_location: Option<String>,
    pub nat_gateway: Option<String>,
    pub dht_enabled: bool,
    pub dht_bootstrap: Vec<String>,
    /// FR-18 资源治理（全部 opt-in 参数，缺省保持既有行为）。
    pub dht_timeout_ms: u64,
    pub dht_refresh_secs: u64,
    pub dht_max_bootstraps: usize,
    pub dht_max_seeds: usize,
    pub dht_max_keys: usize,
    pub dht_max_announces_per_cycle: usize,
    pub dht_min_interval_ms: u64,
    pub dht_rss_soft_skip_mb: u64,
    /// DOC-35 决定 3 选 B：公共 DHT 宣告的授权闸门。**默认关闭**；
    /// 打开后还要过每分钟 / 每日两道配额，且运行时可一键关闭。
    pub dht_announce_public: bool,
    pub dht_public_max_per_min: u64,
    pub dht_public_daily_cap: u64,
}

impl Default for P1Config {
    fn default() -> Self {
        Self {
            rpc_secret: None,
            web_ui: false,
            websocket: false,
            qos: cr_api::qos::UploadGovernorConfig::default(),
            nat_auto_map: false,
            nat_upnp_location: None,
            nat_gateway: None,
            dht_enabled: false,
            dht_bootstrap: Vec::new(),
            dht_timeout_ms: 2000,
            dht_refresh_secs: 600,
            dht_max_bootstraps: 8,
            dht_max_seeds: 50,
            dht_max_keys: 512,
            dht_max_announces_per_cycle: 16,
            dht_min_interval_ms: 50,
            dht_rss_soft_skip_mb: 0, // 0 = 不做 RSS 软跳过
            dht_announce_public: false,
            dht_public_max_per_min: 4,
            dht_public_daily_cap: 200,
        }
    }
}

fn section<'a>(j: &'a Json, key: &str) -> Option<&'a Json> {
    j.get(key).filter(|v| v.as_obj().is_some())
}

fn bool_in(j: Option<&Json>, key: &str, default: bool) -> bool {
    j.and_then(|s| s.get(key)).and_then(Json::as_bool).unwrap_or(default)
}

fn str_in(j: Option<&Json>, key: &str) -> Option<String> {
    j.and_then(|s| s.get(key)).and_then(Json::as_str)
        .filter(|s| !s.is_empty()).map(str::to_string)
}

fn f64_in(j: Option<&Json>, key: &str, default: f64) -> f64 {
    match j.and_then(|s| s.get(key)) {
        Some(Json::Num(n)) if n.is_finite() => *n,
        _ => default,
    }
}

fn u64_in(j: Option<&Json>, key: &str, default: u64) -> u64 {
    match j.and_then(|s| s.get(key)) {
        Some(Json::Num(n)) if n.is_finite() && *n >= 0.0 => *n as u64,
        _ => default,
    }
}

/// 资源上限不接受 0：0 通常意味着「配错了」，静默当成无限会让治理形同虚设。
fn positive_u64_in(j: Option<&Json>, key: &str, default: u64) -> u64 {
    match j.and_then(|s| s.get(key)) {
        Some(Json::Num(n)) if n.is_finite() && *n >= 1.0 => *n as u64,
        _ => default,
    }
}

impl P1Config {
    /// 读取同一份守护进程配置里的 P1 段。文件读取失败时返回缺省（不阻断启动），
    /// 但调用方已经用 `Config::load` 成功过，所以正常路径不会走到这里。
    pub fn load(path: &Path) -> Self {
        match Json::load(path) {
            Ok(j) => Self::from_json(&j),
            Err(_) => Self::default(),
        }
    }

    pub fn from_json(j: &Json) -> Self {
        let api = section(j, "api");
        let qos = section(j, "qos");
        let nat = section(j, "nat");
        let dht = section(j, "dht");
        let bootstrap = dht.and_then(|d| d.get("bootstrap")).and_then(Json::as_arr)
            .map(|a| a.iter().filter_map(|v| v.as_str())
                .filter(|s| !s.is_empty()).map(str::to_string).collect())
            .unwrap_or_default();
        Self {
            rpc_secret: str_in(api, "rpc_secret"),
            web_ui: bool_in(api, "web_ui", false),
            websocket: bool_in(api, "websocket", false),
            qos: cr_api::qos::UploadGovernorConfig {
                ratio: f64_in(qos, "background_upload_ratio",
                              cr_api::qos::DEFAULT_BACKGROUND_UPLOAD_RATIO),
                reference_link_bps: u64_in(qos, "reference_link_bps",
                              cr_api::qos::DEFAULT_REFERENCE_LINK_BPS),
            },
            nat_auto_map: bool_in(nat, "auto_map", false),
            nat_upnp_location: str_in(nat, "upnp_location"),
            nat_gateway: str_in(nat, "gateway"),
            dht_enabled: bool_in(dht, "enabled", false),
            dht_bootstrap: bootstrap,
            dht_timeout_ms: positive_u64_in(dht, "timeout_ms", 2000),
            dht_refresh_secs: positive_u64_in(dht, "refresh_secs", 600),
            dht_max_bootstraps: positive_u64_in(dht, "max_bootstraps", 8) as usize,
            dht_max_seeds: positive_u64_in(dht, "max_seeds", 50) as usize,
            dht_max_keys: positive_u64_in(dht, "max_keys", 512) as usize,
            dht_max_announces_per_cycle: positive_u64_in(dht, "max_announces_per_cycle", 16) as usize,
            dht_min_interval_ms: u64_in(dht, "min_interval_ms", 50),
            dht_rss_soft_skip_mb: u64_in(dht, "rss_soft_skip_mb", 0),
            // 授权默认关闭：只有显式写 true 才可能发出公共 announce。
            dht_announce_public: bool_in(dht, "announce_public", false),
            dht_public_max_per_min: positive_u64_in(dht, "public_max_per_min", 4),
            dht_public_daily_cap: u64_in(dht, "public_daily_cap", 200),
        }
    }

    /// `/health` 只暴露布尔开关，绝不返回 secret 值。
    pub fn health_json(&self) -> Json {
        cr_api::rpc::health_api_json(
            self.rpc_secret.as_deref().map(|s| !s.is_empty()).unwrap_or(false),
            self.web_ui,
            self.websocket,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_keep_legacy_behavior() {
        let p = P1Config::from_json(&Json::Null);
        assert_eq!(p.rpc_secret, None);
        assert!(!p.web_ui);
        assert!(!p.websocket);
        assert!(!p.nat_auto_map);
        assert!(!p.dht_enabled);
        assert!(p.dht_bootstrap.is_empty());
        assert_eq!(p.qos.ratio, 0.02);
        assert_eq!(p.qos.cap_bps(), Some(250_000));
    }

    #[test]
    fn explicit_values_are_parsed() {
        let j = Json::parse(r#"{
            "api": {"rpc_secret": "k", "web_ui": true, "websocket": true},
            "qos": {"background_upload_ratio": 0, "reference_link_bps": 1000},
            "nat": {"auto_map": true, "upnp_location": "http://1.2.3.4/d.xml"},
            "dht": {"enabled": true, "bootstrap": ["router.example:6881", ""]}
        }"#).unwrap();
        let p = P1Config::from_json(&j);
        assert_eq!(p.rpc_secret.as_deref(), Some("k"));
        assert!(p.web_ui && p.websocket);
        assert_eq!(p.qos.ratio, 0.0);
        assert_eq!(p.qos.cap_bps(), None);
        assert!(p.nat_auto_map);
        assert_eq!(p.nat_upnp_location.as_deref(), Some("http://1.2.3.4/d.xml"));
        assert!(p.dht_enabled);
        assert_eq!(p.dht_bootstrap, vec!["router.example:6881".to_string()]);
    }

    #[test]
    fn dht_public_announce_defaults_to_closed() {
        // DOC-35 决定 3：授权不等于默认开。缺省必须是关的。
        let d = P1Config::from_json(&Json::Null);
        assert!(!d.dht_announce_public, "缺省必须关闭公共宣告");
        assert_eq!(d.dht_public_max_per_min, 4);
        assert_eq!(d.dht_public_daily_cap, 200);

        let j = Json::parse(r#"{"dht":{
            "announce_public": true, "public_max_per_min": 0, "public_daily_cap": 0
        }}"#).unwrap();
        let p = P1Config::from_json(&j);
        assert!(p.dht_announce_public);
        assert_eq!(p.dht_public_max_per_min, 4, "0 是非法的，回落缺省而不是无限放行");
        assert_eq!(p.dht_public_daily_cap, 0, "日上限 0 = 不设上限，是显式语义");
    }

    #[test]
    fn dht_limits_default_and_reject_zero() {
        let d = P1Config::from_json(&Json::Null);
        assert_eq!(d.dht_timeout_ms, 2000);
        assert_eq!(d.dht_max_bootstraps, 8);
        assert_eq!(d.dht_max_seeds, 50);
        assert_eq!(d.dht_max_keys, 512);
        assert_eq!(d.dht_max_announces_per_cycle, 16);
        assert_eq!(d.dht_min_interval_ms, 50);
        assert_eq!(d.dht_rss_soft_skip_mb, 0);

        let j = Json::parse(r#"{"dht":{
            "timeout_ms": 500, "refresh_secs": 60, "max_bootstraps": 0,
            "max_seeds": 3, "max_keys": 7, "max_announces_per_cycle": 2,
            "min_interval_ms": 10, "rss_soft_skip_mb": 400
        }}"#).unwrap();
        let p = P1Config::from_json(&j);
        assert_eq!(p.dht_timeout_ms, 500);
        assert_eq!(p.dht_refresh_secs, 60);
        assert_eq!(p.dht_max_bootstraps, 8, "0 是非法的，回落缺省而不是无限");
        assert_eq!(p.dht_max_seeds, 3);
        assert_eq!(p.dht_max_keys, 7);
        assert_eq!(p.dht_max_announces_per_cycle, 2);
        assert_eq!(p.dht_min_interval_ms, 10);
        assert_eq!(p.dht_rss_soft_skip_mb, 400);
    }

    #[test]
    fn health_never_carries_the_secret_value() {
        let j = Json::parse(r#"{"api":{"rpc_secret":"DO_NOT_LEAK"}}"#).unwrap();
        let p = P1Config::from_json(&j);
        let dump = p.health_json().dump();
        assert!(!dump.contains("DO_NOT_LEAK"));
        assert_eq!(p.health_json().get("rpc_secret").and_then(|x| x.get("enabled"))
                    .and_then(Json::as_bool), Some(true));
    }
}
