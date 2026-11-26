//! FR-17：启动时 opt-in 的 NAT 自动端口映射。
//!
//! 只调用 `cr-nat` 里**真实存在**的 UPnP IGD / NAT-PMP 原语。每一次尝试的成功/失败、
//! 外部端口与外部 IP 都来自原语返回值；任何没拿到映射的情况都记为失败并带原始错误，
//! 绝不写一个「看起来有映射」的占位值。S6 机器常量完全不受影响。

use std::time::Duration;

use cr_core::Json;

use crate::p1_config::P1Config;

const MAP_TIMEOUT: Duration = Duration::from_secs(2);
const LEASE_SECS: u32 = 3600;

#[derive(Debug, Clone)]
pub struct MappingAttempt {
    pub method: &'static str,
    pub proto: &'static str,
    pub internal_port: u16,
    pub external_port: Option<u16>,
    pub external_ip: Option<String>,
    /// `requested` = 按请求端口建立（UPnP AddPortMapping 成功）；
    /// `router_assigned` = 路由器在应答里真实回传（NAT-PMP）。
    pub port_source: &'static str,
    pub success: bool,
    pub error: String,
}

impl MappingAttempt {
    fn json(&self) -> Json {
        Json::obj(vec![
            ("method", Json::s(self.method)),
            ("proto", Json::s(self.proto)),
            ("internal_port", Json::n(self.internal_port as f64)),
            ("external_port", match self.external_port {
                Some(p) => Json::n(p as f64), None => Json::Null }),
            ("external_ip", match &self.external_ip {
                Some(ip) => Json::s(ip), None => Json::Null }),
            ("port_source", Json::s(self.port_source)),
            ("success", Json::b(self.success)),
            ("error", Json::s(&self.error)),
        ])
    }
}

#[derive(Debug, Clone)]
pub struct AutoMapStatus {
    pub enabled: bool,
    pub attempts: Vec<MappingAttempt>,
}

impl AutoMapStatus {
    pub fn disabled() -> Self {
        Self { enabled: false, attempts: Vec::new() }
    }

    pub fn status_label(&self) -> &'static str {
        if !self.enabled { return "disabled"; }
        if self.attempts.iter().all(|a| a.success) && !self.attempts.is_empty() {
            "mapped"
        } else if self.attempts.iter().any(|a| a.success) {
            "partial"
        } else {
            "failed"
        }
    }

    pub fn json(&self) -> Json {
        Json::obj(vec![
            ("enabled", Json::b(self.enabled)),
            ("status", Json::s(self.status_label())),
            ("attempts", Json::arr(self.attempts.iter().map(MappingAttempt::json).collect())),
            ("note", Json::s("映射仅来自路由器真实应答；不改变 s6_status")),
        ])
    }
}

/// 在启动阶段尝试建映射。启用但没有任何可用手段时也记录一条真实的失败。
pub fn attempt(p1: &P1Config, wake_udp_port: u16, bt_tcp_port: u16) -> AutoMapStatus {
    if !p1.nat_auto_map {
        return AutoMapStatus::disabled();
    }
    let mut attempts = Vec::new();
    if let Some(gateway) = p1.nat_gateway.as_deref() {
        let client = cr_nat::natpmp::NatPmpClient::new(gateway.to_string());
        let ext_ip = client.external_address(MAP_TIMEOUT).external_ip;
        for (proto, port, pmp) in [
            ("UDP", wake_udp_port, cr_nat::natpmp::Proto::Udp),
            ("TCP", bt_tcp_port, cr_nat::natpmp::Proto::Tcp),
        ] {
            match client.map(pmp, port, 0, LEASE_SECS, MAP_TIMEOUT) {
                Ok(m) => attempts.push(MappingAttempt {
                    method: "nat-pmp", proto, internal_port: port,
                    external_port: Some(m.external_port),
                    external_ip: ext_ip.clone(), port_source: "router_assigned",
                    success: true, error: String::new(),
                }),
                Err(e) => attempts.push(MappingAttempt {
                    method: "nat-pmp", proto, internal_port: port,
                    external_port: None, external_ip: None, port_source: "none",
                    success: false, error: e,
                }),
            }
        }
    } else {
        let discovered = match p1.nat_upnp_location.as_deref() {
            Some(loc) => cr_nat::upnp::UpnpResult {
                available: true, locations: vec![loc.to_string()], error: String::new(),
            },
            None => cr_nat::upnp::discover(MAP_TIMEOUT),
        };
        if !discovered.available || discovered.locations.is_empty() {
            attempts.push(MappingAttempt {
                method: "upnp-discovery", proto: "NONE", internal_port: 0,
                external_port: None, external_ip: None, port_source: "none", success: false,
                error: if discovered.error.is_empty() {
                    "未发现 UPnP IGD；未创建任何端口映射".into()
                } else { discovered.error },
            });
        } else {
            let client = cr_nat::upnp::UpnpClient::from_location(
                discovered.locations[0].clone());
            let ext_ip = client.get_external_ip(MAP_TIMEOUT).ok();
            for (proto, port) in [("UDP", wake_udp_port), ("TCP", bt_tcp_port)] {
                match client.add_port_mapping(proto, port, port, LEASE_SECS,
                                              "ColdRevive auto-map", MAP_TIMEOUT) {
                    Ok(()) => attempts.push(MappingAttempt {
                        method: "upnp-igd", proto, internal_port: port,
                        external_port: Some(port), external_ip: ext_ip.clone(),
                        port_source: "requested", success: true, error: String::new(),
                    }),
                    Err(e) => attempts.push(MappingAttempt {
                        method: "upnp-igd", proto, internal_port: port,
                        external_port: None, external_ip: None, port_source: "none",
                        success: false, error: e,
                    }),
                }
            }
        }
    }
    AutoMapStatus { enabled: true, attempts }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_status_is_explicit() {
        let s = AutoMapStatus::disabled();
        let j = s.json();
        assert_eq!(s.status_label(), "disabled");
        assert_eq!(j.get("enabled").and_then(Json::as_bool), Some(false));
        assert_eq!(j.get("status").and_then(Json::as_str), Some("disabled"));
        assert_eq!(j.get("attempts").and_then(Json::as_arr).unwrap().len(), 0);
    }

    #[test]
    fn explicit_unavailable_location_reports_real_failure() {
        // 指向本机一个无人监听的端口：真实连接失败，不触网、不依赖路由器。
        let p1 = P1Config {
            nat_auto_map: true,
            nat_upnp_location: Some("http://127.0.0.1:1/no-igd.xml".into()),
            ..Default::default()
        };
        let s = attempt(&p1, 17801, 17802);
        assert!(s.enabled);
        assert_eq!(s.status_label(), "failed");
        assert!(s.attempts.iter().all(|a| !a.success));
        assert!(s.attempts.iter().all(|a| a.external_port.is_none()));
        assert!(s.attempts.iter().any(|a| !a.error.is_empty()));
    }

    #[test]
    fn status_label_reflects_real_attempt_records() {
        // 这里只测「状态如何由尝试记录得出」的纯逻辑，不伪造任何映射结果。
        let ok = MappingAttempt {
            method: "upnp-igd", proto: "UDP", internal_port: 1,
            external_port: Some(1), external_ip: Some("198.51.100.1".into()),
            port_source: "requested", success: true, error: String::new(),
        };
        let s = AutoMapStatus { enabled: true, attempts: vec![ok.clone()] };
        assert_eq!(s.status_label(), "mapped");
        let bad = MappingAttempt { success: false, external_port: None, error: "x".into(), ..ok };
        let s = AutoMapStatus { enabled: true, attempts: vec![bad] };
        assert_eq!(s.status_label(), "failed");
    }
}
