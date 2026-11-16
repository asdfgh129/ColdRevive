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

/// 由 STUN 在「同一本地端口」对不同**目的地址**得到的映射地址集合判定 NAT 对称性。
///
/// `observations` 是 `(stun 服务器, 观测到的映射地址)` 列表 —— 必须带上服务器，
/// 因为「只有一个目的地址」在原理上无法区分 EIM 与按目的分配（APDM）。
///
/// - 空 → `Unknown`（STUN 不可达）
/// - 只有 **1 个不同目的地址** → `Unknown`（样本不足，不下结论）
/// - ≥2 个目的地址，映射全部相同 → `FullCone` 占位（EIM 已确认；cone 子型必须靠
///   `classify_from_filter` 实测过滤行为，不能由映射长相推出）
/// - 映射端口不同 → `Symmetric`
/// - 端口相同但 IP 不同 → 多出口 cone（少见，归入 cone）
///
/// ⚠ 这条曾经用「单样本 → FullCone」的假设：只要有多台 STUN 超时、只剩一台有应答，
/// 一台真实 symmetric 设备就会被判成 cone，再叠加过滤测试的假阳性就成了 `full-cone`。
/// 现在样本不足一律 `Unknown`，宁可说不出结论，也不编一个。
pub fn classify_from_mappings(observations: &[(String, String)]) -> NatType {
    if observations.is_empty() {
        return NatType::Unknown;
    }
    let mut servers: Vec<&str> = Vec::new();
    let mut mapped: Vec<&str> = Vec::new();
    for (srv, m) in observations {
        if !servers.contains(&srv.as_str()) {
            servers.push(srv.as_str());
        }
        if !mapped.contains(&m.as_str()) {
            mapped.push(m.as_str());
        }
    }
    if servers.len() < 2 {
        return NatType::Unknown; // 只问过一台服务器 = 只有一个目的地址，判不了对称性
    }
    if mapped.len() == 1 {
        return NatType::FullCone; // EIM 已确认，子型待过滤实测
    }
    // 比较端口
    let ports: std::collections::HashSet<&str> =
        mapped.iter().map(|m| m.rsplit(':').next().unwrap_or("")).collect();
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

impl FilterBehavior {
    pub fn label(&self) -> &'static str {
        match self {
            FilterBehavior::FullCone => "unfiltered",
            FilterBehavior::RestrictedCone => "address-restricted",
            FilterBehavior::PortRestrictedCone => "port-restricted",
            FilterBehavior::Unknown => "unknown",
        }
    }
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
    fn single_destination_sample_is_unknown_not_cone() {
        // 回归用例：只问过一台 STUN 时，原理上分不清 EIM 与 APDM。
        // 以前的实现返回 FullCone，让真实 symmetric 设备被误判成 cone。
        let one = vec![("stun.a:3478".to_string(), "1.2.3.4:5000".to_string())];
        assert_eq!(classify_from_mappings(&one), NatType::Unknown);
    }

    #[test]
    fn two_destinations_same_mapping_is_cone() {
        let obs = vec![
            ("stun.a:3478".to_string(), "1.2.3.4:5000".to_string()),
            ("stun.b:3478".to_string(), "1.2.3.4:5000".to_string()),
        ];
        assert_eq!(classify_from_mappings(&obs), NatType::FullCone);
    }

    #[test]
    fn classify_different_ports_is_symmetric() {
        let obs = vec![
            ("stun.a:3478".to_string(), "1.2.3.4:5000".to_string()),
            ("stun.b:3478".to_string(), "1.2.3.4:6000".to_string()),
        ];
        assert_eq!(classify_from_mappings(&obs), NatType::Symmetric);
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
