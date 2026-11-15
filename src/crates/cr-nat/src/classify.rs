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
        return NatType::FullCone; // 至少是非对称（EIM）；细分必须靠 classify_from_filter 实测
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

/// RFC 3489 Test II / III 的真实观测结果（由 `stun::filter_behavior` 产出）。
///
/// 这个枚举存在的原因本身就是一条教训：细分 cone 子型**不可能**由「映射地址长什么样」
/// 推出来，只能由「换个地址/端口回包能不能进来」测出来。没有这一步，任何子类型结论
/// 都是猜的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterBehavior {
    /// change-IP 有应答：入向不限源地址。
    FullCone,
    /// 仅 change-port 有应答：限源地址、不限源端口。
    RestrictedCone,
    /// 两种 change 都无应答：只放行已有映射（端口受限）。
    PortRestrictedCone,
    /// 连正对照都没通 —— STUN 不可达，不能据此推断任何过滤行为。
    Unknown,
}

/// 把实测的过滤行为映射为 NAT 类型。
pub fn classify_from_filter(f: FilterBehavior) -> NatType {
    match f {
        FilterBehavior::FullCone => NatType::FullCone,
        FilterBehavior::RestrictedCone => NatType::RestrictedCone,
        FilterBehavior::PortRestrictedCone => NatType::PortRestrictedCone,
        FilterBehavior::Unknown => NatType::Unknown,
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
    fn filter_behavior_maps_to_nat_type() {
        assert_eq!(classify_from_filter(FilterBehavior::FullCone), NatType::FullCone);
        assert_eq!(
            classify_from_filter(FilterBehavior::RestrictedCone),
            NatType::RestrictedCone
        );
        assert_eq!(
            classify_from_filter(FilterBehavior::PortRestrictedCone),
            NatType::PortRestrictedCone
        );
    }

    #[test]
    fn unreachable_stun_is_not_a_port_restricted_conclusion() {
        // 回归用例：没有正对照（普通请求都收不到应答）时不得推断任何过滤行为。
        assert_eq!(classify_from_filter(FilterBehavior::Unknown), NatType::Unknown);
    }
}
