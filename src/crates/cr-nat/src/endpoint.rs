//! M2/W3.3 — 端点规划（把穿透探测结果变成可执行的候选地址）。
//!
//! 职责边界：本模块**纯计算、不发包**。输入是 `TraversalReport`（真实探测结果）与对端
//! 通告的 `PeerAdvertisement`，输出是有序候选列表与 tracker 应答地址。真实 UDP/TCP
//! 往返在 `holepunch.rs` 与守护进程中完成。
//!
//! 红线：
//! - 没有真实映射证据时，绝不凭空生成「看起来像公网」的地址；
//! - 公网请求方永远不会收到 RFC1918 私网 peer 地址（没有公网映射时宁可返回空 peer 列表）；
//! - STUN 映射只覆盖 UDP 探测 socket 的端口，不等价于 BT/TCP 端口映射，
//!   故 tracker 只使用 `internal_port == bt_port` 的显式端口映射（UPnP/NAT-PMP/PCP）。

use crate::{NatType, Strategy, TraversalReport};
use std::net::Ipv4Addr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EndpointKind {
    /// 同子网 / 本机直连地址（M1 口径）。
    PrivateLan,
    /// UPnP IGD 显式端口映射。
    UpnpMapped,
    /// NAT-PMP 显式端口映射。
    NatPmpMapped,
    /// PCP 显式端口映射。
    PcpMapped,
    /// 运维侧真实创建的 BT/TCP 显式端口映射（配置提供，不参与 UDP 唤醒候选）。
    ConfiguredMapping,
    /// STUN 观测到的外部映射（cone NAT 预测打洞用）。
    StunMapped,
    /// 经可信中继转发。
    Relay,
}

impl EndpointKind {
    pub fn label(&self) -> &'static str {
        match self {
            EndpointKind::PrivateLan => "private-lan",
            EndpointKind::UpnpMapped => "upnp-mapped",
            EndpointKind::NatPmpMapped => "natpmp-mapped",
            EndpointKind::PcpMapped => "pcp-mapped",
            EndpointKind::ConfiguredMapping => "configured-mapping",
            EndpointKind::StunMapped => "stun-mapped",
            EndpointKind::Relay => "relay",
        }
    }

    pub fn from_label(s: &str) -> Option<EndpointKind> {
        [
            EndpointKind::PrivateLan,
            EndpointKind::UpnpMapped,
            EndpointKind::NatPmpMapped,
            EndpointKind::PcpMapped,
            EndpointKind::ConfiguredMapping,
            EndpointKind::StunMapped,
            EndpointKind::Relay,
        ]
        .into_iter()
        .find(|k| k.label() == s)
    }

    /// 是否为「显式端口映射」——这类映射才能安全地用于 tracker 的 BT/TCP peer 地址。
    pub fn is_explicit_port_mapping(&self) -> bool {
        matches!(
            self,
            EndpointKind::UpnpMapped | EndpointKind::NatPmpMapped
            | EndpointKind::PcpMapped | EndpointKind::ConfiguredMapping
        )
    }
}

/// 一个可尝试的端点。`port` 是对端可见端口，`internal_port` 是本机监听端口。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub kind: EndpointKind,
    pub ip: String,
    pub port: u16,
    pub internal_port: u16,
    /// 该端点的证据来源（真实探测 / 对端通告 / 配置），不得为空。
    pub evidence: String,
}

impl Endpoint {
    pub fn addr(&self) -> String {
        format!("{}:{}", self.ip, self.port)
    }
    pub fn is_public(&self) -> bool {
        self.kind != EndpointKind::PrivateLan && is_public_ipv4(&self.ip)
    }
}

/// 是否 RFC1918 / 回环 / 链路本地 / CGNAT / 保留段等非公网 IPv4。
pub fn is_public_ipv4(ip: &str) -> bool {
    let Ok(a) = ip.parse::<Ipv4Addr>() else { return false };
    let o = a.octets();
    if a.is_loopback() || a.is_private() || a.is_link_local() || a.is_multicast()
        || a.is_unspecified() || o[0] == 0 || o[0] >= 224
    {
        return false;
    }
    // 100.64.0.0/10 (CGNAT)、192.0.2.0/24、198.18.0.0/15、198.51.100.0/24、
    // 203.0.113.0/24、169.254.0.0/16、127.0.0.0/8
    if o[0] == 100 && (o[1] & 0xC0) == 64 { return false; }
    if o[0] == 169 && o[1] == 254 { return false; }
    if o[0] == 127 { return false; }
    if o[0] == 192 && o[1] == 0 && o[2] == 2 { return false; }
    if o[0] == 198 && (o[1] == 18 || o[1] == 19 || o[1] == 51) { return false; }
    if o[0] == 203 && o[1] == 0 && o[2] == 113 { return false; }
    true
}

/// 解析 `"ip:port"`（仅 IPv4）。
pub fn split_mapped(s: &str) -> Option<(String, u16)> {
    let (ip, port) = s.rsplit_once(':')?;
    let port: u16 = port.parse().ok()?;
    ip.parse::<Ipv4Addr>().ok()?;
    Some((ip.to_string(), port))
}

/// 节点对外通告的端点集合。字段全部来自真实探测 / 真实配置，不填占位公网地址。
#[derive(Debug, Clone)]
pub struct PeerAdvertisement {
    pub node_id: String,
    pub private_ip: String,
    pub wake_port: u16,
    pub bt_port: u16,
    pub endpoints: Vec<Endpoint>,
    /// 仅用于 tracker 的 BT/TCP 显式端口映射（无真实映射时为 None）。
    pub bt_external: Option<Endpoint>,
    pub relay: Option<String>,
    pub nat_type: String,
    pub public_udp_egress_ok: bool,
    /// 通告生成时的 Unix 时间戳（由调用方填写，便于审计新鲜度）。
    pub probed_at: i64,
}

#[derive(Debug, Clone)]
pub struct EndpointPlan {
    pub strategy: Strategy,
    pub candidates: Vec<Endpoint>,
    /// tracker 应答可用的公网端点（仅显式端口映射且 internal_port == bt_port）。
    pub tracker_public: Option<Endpoint>,
    pub needs_relay: bool,
    pub notes: Vec<String>,
}

/// 由本机真实探测报告生成本节点通告。
///
/// `relay` 为部署侧可信中继地址（配置提供），不是探测结果；没有则为 None。
pub fn local_advertisement(
    node_id: &str,
    private_ip: &str,
    wake_port: u16,
    bt_port: u16,
    report: &TraversalReport,
    relay: Option<String>,
    probed_at: i64,
) -> PeerAdvertisement {
    let mut endpoints = Vec::new();
    let mapped = report.mapped_address.as_deref()
        .and_then(split_mapped);
    if upnp_mapping_available(report) {
        if let Some(ip) = report.upnp_external_ip.as_deref() {
            endpoints.push(Endpoint {
                kind: EndpointKind::UpnpMapped,
                ip: ip.to_string(),
                port: wake_port,
                internal_port: wake_port,
                evidence: "upnp-igd:get_external_ip".into(),
            });
        }
    }
    if report.natpmp_available {
        if let Some(ip) = report.natpmp_external_ip.as_deref() {
            endpoints.push(Endpoint {
                kind: EndpointKind::NatPmpMapped,
                ip: ip.to_string(),
                port: wake_port,
                internal_port: wake_port,
                evidence: "nat-pmp:external_address".into(),
            });
        }
    }
    if report.pcp_available {
        if let Some((ref ip, _)) = mapped {
            endpoints.push(Endpoint {
                kind: EndpointKind::PcpMapped,
                ip: ip.clone(),
                port: wake_port,
                internal_port: wake_port,
                evidence: "pcp:map".into(),
            });
        }
    }
    if let Some((ip, port)) = mapped {
        endpoints.push(Endpoint {
            kind: EndpointKind::StunMapped,
            ip,
            port,
            internal_port: wake_port,
            evidence: format!("stun:{}", report.nat_type.label()),
        });
    }
    // 同子网直连端点始终存在（M1 口径）。它是否可用由真实 UDP 往返判定，不由本模块假定。
    endpoints.insert(0, Endpoint {
        kind: EndpointKind::PrivateLan,
        ip: private_ip.to_string(),
        port: wake_port,
        internal_port: wake_port,
        evidence: "configured-private-host".into(),
    });
    PeerAdvertisement {
        node_id: node_id.to_string(),
        private_ip: private_ip.to_string(),
        wake_port,
        bt_port,
        endpoints,
        bt_external: None,
        relay,
        nat_type: report.nat_type.label().to_string(),
        public_udp_egress_ok: report.public_udp_egress_ok,
        probed_at,
    }
}

fn upnp_mapping_available(report: &TraversalReport) -> bool {
    report.upnp_available
}

/// 构造有序候选列表。
///
/// 排序规则：
/// 1. `Strategy::Relay`（对称 NAT / CGNAT 且无映射）→ 中继优先，直连候选仍保留作兜底；
/// 2. 其余策略：私网直连 → 显式端口映射 → STUN 预测映射 → 中继；
/// 3. 对称 NAT 下 STUN 预测映射无意义，明确剔除并记 note；
/// 4. 没有任何外部证据时不生成外部候选，只留私网直连。
pub fn build_plan(
    local: &TraversalReport,
    peer: &PeerAdvertisement,
    relay: Option<&str>,
) -> EndpointPlan {
    let mut notes = Vec::new();
    let strategy = local.recommended_strategy;
    let mut candidates: Vec<Endpoint> = Vec::new();

    let external: Vec<Endpoint> = peer.endpoints.iter()
        // ConfiguredMapping 只用于 BT/TCP tracker，不是 UDP 唤醒候选。
        .filter(|e| e.kind != EndpointKind::PrivateLan
            && e.kind != EndpointKind::ConfiguredMapping)
        .filter(|e| {
            if e.kind == EndpointKind::StunMapped
                && (local.nat_type == NatType::Symmetric || local.nat_type == NatType::Cgnat)
            {
                notes.push("对称型/CGNAT 下剔除 STUN 预测映射（映射端口不可复用）".into());
                false
            } else {
                true
            }
        })
        .cloned()
        .collect();

    let relay_endpoint = relay.and_then(|r| {
        split_mapped(r).map(|(ip, port)| Endpoint {
            kind: EndpointKind::Relay,
            ip,
            port,
            internal_port: peer.wake_port,
            evidence: "configured-relay".into(),
        })
    });

    if strategy == Strategy::Relay {
        if let Some(e) = &relay_endpoint { candidates.push(e.clone()); }
        candidates.extend(peer.endpoints.iter()
            .filter(|e| e.kind == EndpointKind::PrivateLan).cloned());
        candidates.extend(external.iter().cloned());
        notes.push("策略=relay-fallback：中继优先，直连候选仅作兜底".into());
    } else {
        candidates.extend(peer.endpoints.iter()
            .filter(|e| e.kind == EndpointKind::PrivateLan).cloned());
        candidates.extend(external.iter().cloned());
        if let Some(e) = &relay_endpoint {
            // 中继始终作为最后兜底（真实失败才轮到它）
            candidates.push(e.clone());
        }
    }

    // 去重（同一 kind+addr 只保留一次），保持顺序
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|e| seen.insert((e.kind, e.addr())));

    if external.is_empty() {
        notes.push("对端无外部映射证据：仅私网直连候选，跨 NAT 可达性未证明".into());
    }
    let needs_relay = crate::needs_relay(local);
    if relay.is_none() && (strategy == Strategy::Relay || needs_relay) {
        notes.push("需要中继但未提供中继地址：复活路径将如实失败".into());
    }

    let tracker_public = peer.bt_external.clone()
        .filter(|e| is_public_ipv4(&e.ip) && e.internal_port == peer.bt_port)
        .or_else(|| peer.endpoints.iter()
            .find(|e| e.kind.is_explicit_port_mapping()
                && e.internal_port == peer.bt_port
                && is_public_ipv4(&e.ip))
            .cloned());

    EndpointPlan {
        strategy,
        candidates,
        tracker_public,
        needs_relay,
        notes,
    }
}

/// tracker 应答地址决策（供 `/announce` 使用）。
///
/// - `requester_ip` 为私网/回环 → 返回私网 peer（M1/T0/T1 口径不变）；
/// - `requester_ip` 为公网 → 只返回显式公网端口映射；没有则返回 `None`
///   （调用方应答空 peer 列表，绝不把 RFC1918 地址发给公网对端）。
pub fn tracker_endpoint_for(
    advertisement: &PeerAdvertisement,
    requester_ip: &str,
    plan: Option<&EndpointPlan>,
) -> Option<(String, u16)> {
    let requester_public = is_public_ipv4(requester_ip);
    if !requester_public {
        return Some((advertisement.private_ip.clone(), advertisement.bt_port));
    }
    let public = advertisement.bt_external.clone()
        .filter(|e| is_public_ipv4(&e.ip) && e.internal_port == advertisement.bt_port)
        .or_else(|| plan.and_then(|p| p.tracker_public.clone()))
        .or_else(|| advertisement.endpoints.iter()
            .find(|e| e.kind.is_explicit_port_mapping()
                && e.internal_port == advertisement.bt_port
                && is_public_ipv4(&e.ip))
            .cloned());
    public.map(|e| (e.ip, e.port))
}

/// 把节点标签（"A"/"B"）映射为 16 字节中继 node id（确定性、无随机性）。
pub fn node_id_16(label: &str) -> [u8; 16] {
    let mut out = [0u8; 16];
    let n = label.len().min(16);
    out[..n].copy_from_slice(&label.as_bytes()[..n]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(nat: NatType, strategy: Strategy, mapped: Option<&str>) -> TraversalReport {
        TraversalReport {
            nat_type: nat,
            public_udp_egress_ok: mapped.is_some(),
            upnp_available: false,
            upnp_external_ip: None,
            natpmp_available: false,
            natpmp_external_ip: None,
            pcp_available: false,
            cgnat_suspected: false,
            mapped_address: mapped.map(str::to_string),
            recommended_strategy: strategy,
        }
    }

    #[test]
    fn public_ipv4_classification() {
        assert!(is_public_ipv4("8.8.8.8"));
        assert!(is_public_ipv4("112.5.60.4"));
        for ip in ["127.0.0.1", "127.0.0.2", "192.168.1.210", "10.0.0.1",
                   "172.16.0.1", "100.64.0.1", "169.254.1.1", "0.0.0.0",
                   "255.255.255.255", "198.18.0.1", "203.0.113.5", "not-an-ip"] {
            assert!(!is_public_ipv4(ip), "{} 不应判为公网", ip);
        }
    }

    #[test]
    fn advertisement_without_probe_has_no_fake_public_endpoint() {
        let r = report(NatType::Unknown, Strategy::None, None);
        let a = local_advertisement("A", "192.168.1.210", 17801, 17802, &r, None, 0);
        assert_eq!(a.endpoints.len(), 1);
        assert_eq!(a.endpoints[0].kind, EndpointKind::PrivateLan);
        assert!(a.endpoints.iter().all(|e| !e.is_public()));
    }

    #[test]
    fn stun_mapping_is_advertised_with_its_own_port() {
        let r = report(NatType::PortRestrictedCone, Strategy::Predictive,
                       Some("112.5.60.4:8749"));
        let a = local_advertisement("A", "192.168.1.210", 17801, 17802, &r, None, 0);
        let m = a.endpoints.iter().find(|e| e.kind == EndpointKind::StunMapped).unwrap();
        assert_eq!(m.addr(), "112.5.60.4:8749");
        assert_eq!(m.internal_port, 17801);
        assert!(m.is_public());
    }

    #[test]
    fn plan_order_is_private_then_mapping_then_relay() {
        let local = report(NatType::PortRestrictedCone, Strategy::Predictive,
                           Some("112.5.60.4:9001"));
        let mut peer_report = report(NatType::FullCone, Strategy::Upnp, Some("203.0.1.9:43000"));
        peer_report.upnp_available = true;
        peer_report.upnp_external_ip = Some("203.0.1.9".into());
        let peer = local_advertisement("A", "192.168.1.210", 17801, 17802,
                                       &peer_report, None, 0);
        let plan = build_plan(&local, &peer, Some("198.51.100.7:7000"));
        let kinds: Vec<_> = plan.candidates.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![
            EndpointKind::PrivateLan,
            EndpointKind::UpnpMapped,
            EndpointKind::StunMapped,
            EndpointKind::Relay,
        ]);
        assert!(!plan.needs_relay);
    }

    #[test]
    fn symmetric_nat_goes_relay_first_and_drops_predictive_mapping() {
        let local = report(NatType::Symmetric, Strategy::Relay, Some("112.5.60.4:9100"));
        let peer = local_advertisement("A", "192.168.1.210", 17801, 17802,
                                       &report(NatType::Symmetric, Strategy::Relay,
                                               Some("203.0.1.9:44000")), None, 0);
        let plan = build_plan(&local, &peer, Some("198.51.100.7:7000"));
        assert_eq!(plan.candidates[0].kind, EndpointKind::Relay);
        assert!(!plan.candidates.iter().any(|e| e.kind == EndpointKind::StunMapped));
        assert!(plan.notes.iter().any(|n| n.contains("对称型")));
        assert!(plan.needs_relay);
    }

    #[test]
    fn no_relay_configured_still_fails_honestly() {
        let local = report(NatType::Symmetric, Strategy::None, None);
        let peer = local_advertisement("A", "192.168.1.210", 17801, 17802,
                                       &report(NatType::Unknown, Strategy::None, None), None, 0);
        let plan = build_plan(&local, &peer, None);
        assert!(!plan.candidates.iter().any(|e| e.kind == EndpointKind::Relay));
        assert!(plan.notes.iter().any(|n| n.contains("未提供中继地址")));
    }

    #[test]
    fn tracker_never_advertises_private_ip_to_public_requester() {
        let r = report(NatType::Unknown, Strategy::None, None);
        let a = local_advertisement("A", "192.168.1.210", 17801, 17802, &r, None, 0);
        assert_eq!(tracker_endpoint_for(&a, "192.168.1.209", None),
                   Some(("192.168.1.210".into(), 17802)));
        assert_eq!(tracker_endpoint_for(&a, "8.8.8.8", None), None);
    }

    #[test]
    fn tracker_uses_explicit_public_mapping_only() {
        let mut r = report(NatType::FullCone, Strategy::Upnp, Some("203.0.1.9:43000"));
        r.upnp_available = true;
        r.upnp_external_ip = Some("203.0.1.9".into());
        let a = local_advertisement("A", "192.168.1.210", 17801, 17802, &r, None, 0);
        let plan = build_plan(&r, &a, None);
        // 该 UPnP 映射的 internal_port 是 wake_port，不是 bt_port → 不能给 BT 用
        assert!(plan.tracker_public.is_none());
        assert_eq!(tracker_endpoint_for(&a, "8.8.8.8", Some(&plan)), None);

        let mut bt_mapping = a.clone();
        bt_mapping.endpoints.push(Endpoint {
            kind: EndpointKind::UpnpMapped,
            ip: "203.0.1.9".into(),
            port: 17802,
            internal_port: 17802,
            evidence: "upnp-igd:add_port_mapping(bt)".into(),
        });
        let plan2 = build_plan(&r, &bt_mapping, None);
        assert_eq!(tracker_endpoint_for(&bt_mapping, "8.8.8.8", Some(&plan2)),
                   Some(("203.0.1.9".into(), 17802)));
    }

    #[test]
    fn configured_bt_mapping_serves_tracker_but_not_udp_wake_candidates() {
        let r = report(NatType::Unknown, Strategy::None, None);
        let mut a = local_advertisement("A", "192.168.1.210", 17801, 17802, &r, None, 0);
        a.bt_external = Some(Endpoint {
            kind: EndpointKind::ConfiguredMapping,
            ip: "8.8.4.4".into(),
            port: 44002,
            internal_port: 17802,
            evidence: "operator-configured-bt-mapping".into(),
        });
        let plan = build_plan(&r, &a, None);
        assert!(!plan.candidates.iter().any(|e| e.kind == EndpointKind::ConfiguredMapping));
        assert_eq!(tracker_endpoint_for(&a, "8.8.8.8", Some(&plan)),
                   Some(("8.8.4.4".into(), 44002)));
        assert_eq!(tracker_endpoint_for(&a, "192.168.1.209", Some(&plan)),
                   Some(("192.168.1.210".into(), 17802)));
    }

    #[test]
    fn endpoint_kind_labels_roundtrip_and_node_ids_are_deterministic() {
        for k in [EndpointKind::PrivateLan, EndpointKind::UpnpMapped,
                  EndpointKind::NatPmpMapped, EndpointKind::PcpMapped,
                  EndpointKind::ConfiguredMapping, EndpointKind::StunMapped,
                  EndpointKind::Relay] {
            assert_eq!(EndpointKind::from_label(k.label()), Some(k));
        }
        assert_eq!(node_id_16("A"), node_id_16("A"));
        assert_ne!(node_id_16("A"), node_id_16("B"));
        assert_eq!(split_mapped("112.5.60.4:8749"),
                   Some(("112.5.60.4".into(), 8749)));
        assert_eq!(split_mapped("bad:99999"), None);
    }
}
