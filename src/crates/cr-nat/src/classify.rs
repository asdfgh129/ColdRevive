//! NAT 类型分类与策略选择（S6 决策核心）。

use std::net::Ipv4Addr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatType {
    FullCone,
    RestrictedCone,
    PortRestrictedCone,
    Symmetric,
    Cgnat,
    /// 已确认「映射与端口无关」，但**缺第二个公网 IP**，无法区分全锥与地址受限。
    ///
    /// 单 IP 观测点（如自建 RFC 5780 服务端）只能测端口这一根轴：change-port 有应答
    /// 说明入向不限端口，但「是否限地址」根本没法测。以前这种情形会被折成
    /// `FullCone`（乐观）或 `RestrictedCone`（悲观），两者都是编的。
    /// 打洞决策上它等价于 cone（可预测），但**类型名必须保留问号**。
    ConeUndetermined,
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
            NatType::ConeUndetermined => "cone-undetermined",
            NatType::Unknown => "unknown",
        }
    }

    /// 是否属于「可预测映射」一族（打洞可尝试直连，不必直接落中继）。
    pub fn is_cone_like(&self) -> bool {
        matches!(
            self,
            NatType::FullCone
                | NatType::RestrictedCone
                | NatType::PortRestrictedCone
                | NatType::ConeUndetermined
        )
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
    /// **端口过滤已实测不存在**，但观测点只有一个公网 IP，地址轴没测。
    ///
    /// 它不等于 `FullCone`：full-cone 要求「异 IP 也能进」，这里没测过；
    /// 也不等于 `RestrictedCone`：地址受限同样没测过。
    /// 由 `stun_server` + `stun::probe_port_filtering` 的双对照实测产生。
    PortOpenUntestedAddress,
    /// 连正对照都没通 —— STUN 不可达，不能据此推断任何过滤行为。
    Unknown,
}

impl FilterBehavior {
    pub fn label(&self) -> &'static str {
        match self {
            FilterBehavior::FullCone => "unfiltered",
            FilterBehavior::RestrictedCone => "address-restricted",
            FilterBehavior::PortRestrictedCone => "port-restricted",
            FilterBehavior::PortOpenUntestedAddress => "port-open-address-untested",
            FilterBehavior::Unknown => "unknown",
        }
    }
}

/// 把实测的过滤行为映射为 NAT 类型。
///
/// `PortOpenUntestedAddress` 只映射到 `ConeUndetermined`：它证明了两件事
/// （映射与目的端口无关、入向不禁异端口），但**没有**证明「异 IP 可进」，
/// 因此不能升级成 `FullCone`。
pub fn classify_from_filter(f: FilterBehavior) -> NatType {
    match f {
        FilterBehavior::FullCone => NatType::FullCone,
        FilterBehavior::RestrictedCone => NatType::RestrictedCone,
        FilterBehavior::PortRestrictedCone => NatType::PortRestrictedCone,
        FilterBehavior::PortOpenUntestedAddress => NatType::ConeUndetermined,
        FilterBehavior::Unknown => NatType::Unknown,
    }
}

/// 由一次双对照探测（`stun::FilterProbe`）得到过滤行为。
///
/// 判据集中在 `FilterOutcome` 里，这里只做映射，不再引入第二套判断，
/// 避免「同一个观测得出两个结论」。
pub fn classify_from_probe(p: &crate::stun::FilterProbe) -> FilterBehavior {
    use crate::stun::FilterOutcome;
    match p.outcome {
        FilterOutcome::PortFiltered => FilterBehavior::PortRestrictedCone,
        FilterOutcome::PortNotFiltered => FilterBehavior::PortOpenUntestedAddress,
        FilterOutcome::Untested => FilterBehavior::Unknown,
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


/// 映射轴（与过滤轴正交）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingAxis {
    /// 有应答的目的地址少于 2 个：分不清 EIM 与按目的分配。
    Insufficient,
    /// 同一本地 socket 对不同目的得到同一映射地址。
    EndpointIndependent,
    /// 映射地址随目的变化（端口不同或 IP 不同）→ 对称。
    AddressOrPortDependent,
}

impl MappingAxis {
    pub fn label(&self) -> &'static str {
        match self {
            MappingAxis::Insufficient => "insufficient",
            MappingAxis::EndpointIndependent => "endpoint-independent",
            MappingAxis::AddressOrPortDependent => "address-port-dependent",
        }
    }
}

/// 纯函数：由 `(服务器, 映射地址)` 观测集判断映射轴。
pub fn mapping_axis(observations: &[(String, String)]) -> MappingAxis {
    if observations.len() < 2 {
        return MappingAxis::Insufficient;
    }
    let mut uniq: Vec<&str> = Vec::new();
    for (_, m) in observations {
        if !uniq.contains(&m.as_str()) {
            uniq.push(m.as_str());
        }
    }
    if uniq.len() >= 2 {
        MappingAxis::AddressOrPortDependent
    } else {
        MappingAxis::EndpointIndependent
    }
}

/// 两轴定性的理由码（写进报告，避免只给结论不给依据）。
pub const WHY_INSUFFICIENT: &str = "mapping-samples<2";
pub const WHY_SYMMETRIC_FILTER_UNUSED: &str = "mapping-dependent-filter-axis-not-used";
pub const WHY_TWO_AXIS: &str = "two-axis";
pub const WHY_FILTER_MISSING: &str = "filter-axis-missing";

/// **两轴定性**：映射轴优先，过滤轴只在必要时参与。
///
/// 这条规则来自一次实测踩坑：222 出口是对称 NAT，过滤探测照样回 `port-filtered`
/// —— 因为 NAT 为目的端口另开了映射，回包落在客户端从未用过的映射上，
/// 于是「收不到」。它不是过滤策略，是映射的副作用。
/// 若把它读成 `port-restricted-cone`，同一台机器就会有两个互相矛盾的类型。
///
/// 所以：映射一旦随目的变化，过滤轴的观测**一律不用**，返回 `Symmetric` + `Unknown`。
pub fn classify_two_axis(
    observations: &[(String, String)],
    filter: Option<FilterBehavior>,
) -> (NatType, FilterBehavior, &'static str) {
    match mapping_axis(observations) {
        MappingAxis::Insufficient => (NatType::Unknown, FilterBehavior::Unknown, WHY_INSUFFICIENT),
        MappingAxis::AddressOrPortDependent => (
            NatType::Symmetric,
            FilterBehavior::Unknown,
            WHY_SYMMETRIC_FILTER_UNUSED,
        ),
        MappingAxis::EndpointIndependent => match filter {
            Some(f) => (classify_from_filter(f), f, WHY_TWO_AXIS),
            None => (NatType::Unknown, FilterBehavior::Unknown, WHY_FILTER_MISSING),
        },
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
    fn symmetric_mapping_makes_filter_axis_inadmissible() {
        // 回归用例（222 实测）：映射 2050/2051，过滤探测回 port-filtered。
        // 若按过滤轴定性就会得到 port-restricted-cone，与 symmetric 直接矛盾。
        let obs = vec![
            ("v:3478".to_string(), "112.5.32.34:2050".to_string()),
            ("v:3479".to_string(), "112.5.32.34:2051".to_string()),
        ];
        let (nat, fb, why) = classify_two_axis(&obs, Some(FilterBehavior::PortRestrictedCone));
        assert_eq!(nat, NatType::Symmetric);
        assert_eq!(fb, FilterBehavior::Unknown, "过滤轴必须被丢掉，不能留着误导");
        assert_eq!(why, WHY_SYMMETRIC_FILTER_UNUSED);
    }

    #[test]
    fn endpoint_independent_plus_port_filtered_is_a_strong_conclusion() {
        let obs = vec![
            ("v:3478".to_string(), "112.5.60.4:1951".to_string()),
            ("v:3479".to_string(), "112.5.60.4:1951".to_string()),
        ];
        let (nat, fb, why) = classify_two_axis(&obs, Some(FilterBehavior::PortRestrictedCone));
        assert_eq!(nat, NatType::PortRestrictedCone);
        assert_eq!(fb, FilterBehavior::PortRestrictedCone);
        assert_eq!(why, WHY_TWO_AXIS);
    }

    #[test]
    fn endpoint_independent_without_filter_sample_is_not_a_type() {
        let obs = vec![
            ("v:3478".to_string(), "1.2.3.4:5000".to_string()),
            ("v:3479".to_string(), "1.2.3.4:5000".to_string()),
        ];
        let (nat, _, why) = classify_two_axis(&obs, None);
        assert_eq!(nat, NatType::Unknown);
        assert_eq!(why, WHY_FILTER_MISSING);
    }

    #[test]
    fn one_sample_never_yields_a_type() {
        let obs = vec![("v:3478".to_string(), "1.2.3.4:5000".to_string())];
        let (nat, _, why) = classify_two_axis(&obs, Some(FilterBehavior::FullCone));
        assert_eq!(nat, NatType::Unknown);
        assert_eq!(why, WHY_INSUFFICIENT);
        assert_eq!(mapping_axis(&obs), MappingAxis::Insufficient);
    }

    #[test]
    fn unreachable_stun_is_not_a_port_restricted_conclusion() {
        // 回归用例：没有正对照（普通请求都收不到应答）时不得推断任何过滤行为。
        assert_eq!(classify_from_filter(FilterBehavior::Unknown), NatType::Unknown);
    }
}
