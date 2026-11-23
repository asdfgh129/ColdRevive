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
    fn health_never_carries_the_secret_value() {
        let j = Json::parse(r#"{"api":{"rpc_secret":"DO_NOT_LEAK"}}"#).unwrap();
        let p = P1Config::from_json(&j);
        let dump = p.health_json().dump();
        assert!(!dump.contains("DO_NOT_LEAK"));
        assert_eq!(p.health_json().get("rpc_secret").and_then(|x| x.get("enabled"))
                    .and_then(Json::as_bool), Some(true));
    }
}
