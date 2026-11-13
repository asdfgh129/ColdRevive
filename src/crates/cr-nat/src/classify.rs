//! NAT 类型分类与策略选择（S6 决策核心）。

use std::net::Ipv4Addr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatType {
    FullCone,
    RestrictedCone,
    PortRestrictedCone,
    Symmetric,
    Cgnat,
    Unknown,
}

impl NatType {
    pub fn label(&self) -> &'static str {
        match self {
            NatType::FullCone => "full-cone",
            NatType::RestrictedCone => "restricted-cone",
            NatType::PortRestrictedCone => "port-restricted-cone",
            NatType::Symmetric => "symmetric",
            NatType::Cgnat => "cgnat",
            NatType::Unknown => "unknown",
        }
    }
}

/// 由 STUN 在「同一本地端口」对不同目的地址得到的映射地址集合判定 NAT 对称性。
///
/// - 集合为空 → `Unknown`（STUN 不可达）
/// - 全部相同 → 非对称 NAT（cone 类；full/restricted 需 OTHER-ADDRESS 二次测试才能细分）
/// - 端口不同 → `Symmetric`
/// - 端口相同但 IP 不同 → 多出口 cone（少见，归入 cone）
pub fn classify_from_mappings(mapped: &[String]) -> NatType {
    if mapped.is_empty() {
        return NatType::Unknown;
    }
    let unique: Vec<&String> = {
        let mut v: Vec<&String> = Vec::new();
        for m in mapped {
            if !v.contains(&m) {
                v.push(m);
            }
        }
        v
    };
    if unique.len() == 1 {
        return NatType::FullCone; // 保守：至少是非对称；细分见 classify_cone_subtype
    }
    // 比较端口
    let ports: std::collections::HashSet<&str> =
        unique.iter().map(|m| m.rsplit(':').next().unwrap_or("")).collect();
    if ports.len() > 1 {
        NatType::Symmetric
    } else {
        NatType::FullCone
    }
}

/// 利用 OTHER-ADDRESS 把 cone 细分为 full / restricted / port-restricted。
/// `other` 是 STUN OTHER-ADDRESS；`mapped` 是 XOR-MAPPED-ADDRESS。
pub fn classify_cone_subtype(mapped: &str, other: &Option<String>) -> NatType {
    match other {
        None => NatType::FullCone, // 无 OTHER-ADDRESS 信息，保守归 full
        Some(o) => {
            let m_ip = mapped.split(':').next().unwrap_or("");
            let o_ip = o.split(':').next().unwrap_or("");
            let m_port = mapped.rsplit(':').next().unwrap_or("");
            let o_port = o.rsplit(':').next().unwrap_or("");
            if o_ip != m_ip {
                NatType::RestrictedCone // 不同 IP 的 OTHER-ADDRESS → 只限制 IP
            } else if o_port != m_port {
                NatType::PortRestrictedCone // 同 IP 不同端口 → 限制端口
            } else {
                NatType::FullCone
            }
        }
    }
}

/// CGNAT 启发式：本机地址落在 100.64/10，或默认网关/第二跳为私有地址。
pub fn cgnat_suspected(local_ip: &str, gateway: &str, second_hop: &str) -> bool {
    if let Ok(ip) = local_ip.parse::<Ipv4Addr>() {
        let o = ip.octets();
        // 100.64.0.0/10：首字节 100 且次字节 64..127
        if o[0] == 100 && (64..=127).contains(&o[1]) {
            return true;
        }
    }
    is_private(gateway) || is_private(second_hop)
}

fn is_private(ip: &str) -> bool {
    match ip.parse::<Ipv4Addr>() {
        Ok(a) => {
            let o = a.octets();
            o[0] == 10
                || (o[0] == 172 && (16..=31).contains(&o[1]))
                || (o[0] == 192 && o[1] == 168)
                || (o[0] == 100 && o[1] < 64)
                || o[0] == 127
                || (o[0] == 169 && o[1] == 254)
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_empty_is_unknown() {
        assert_eq!(classify_from_mappings(&[]), NatType::Unknown);
    }

    #[test]
    fn classify_single_is_cone() {
        assert_eq!(
            classify_from_mappings(&["1.2.3.4:5000".to_string()]),
            NatType::FullCone
        );
    }

    #[test]
    fn classify_different_ports_is_symmetric() {
        let m = vec!["1.2.3.4:5000".to_string(), "1.2.3.4:6000".to_string()];
        assert_eq!(classify_from_mappings(&m), NatType::Symmetric);
    }

    #[test]
    fn cgnat_detects_reserved_range() {
        assert!(cgnat_suspected("100.64.0.5", "", ""));
    }

    #[test]
    fn cgnat_detects_private_gateway() {
        assert!(cgnat_suspected("192.168.1.10", "192.168.1.1", ""));
    }

    #[test]
    fn no_cgnat_for_public() {
        assert!(!cgnat_suspected("203.0.113.5", "203.0.113.1", "198.51.100.1"));
    }

    #[test]
    fn cone_subtype_uses_other_address() {
        assert_eq!(
            classify_cone_subtype("1.2.3.4:5000", &Some("1.2.3.4:5000".into())),
            NatType::FullCone
        );
        assert_eq!(
            classify_cone_subtype("1.2.3.4:5000", &Some("9.9.9.9:5000".into())),
            NatType::RestrictedCone
        );
        assert_eq!(
            classify_cone_subtype("1.2.3.4:5000", &Some("1.2.3.4:6000".into())),
            NatType::PortRestrictedCone
        );
    }
}
