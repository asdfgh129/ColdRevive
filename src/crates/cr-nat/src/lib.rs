//! cr-nat — W3.3 连通性（M2 穿透栈，**实现中 / 合法前置**）。
//!
//! 包含：RFC 5389 STUN、RFC 6886 NAT-PMP、RFC 6887 PCP、UPnP IGD、中继回退（LowID），
//! 以及基于 STUN 映射的 NAT 类型分类与穿透策略选择。
//!
//! **门禁声明**：本实现是 M2/S6 的合法前置（与建骨架 + S5 实测同口径），用于让跨 NAT 场景
//! 在 G1 解锁后具备可达性手段；但四类 NAT（含 CGNAT）的真实可达率矩阵仍须 M2 在多个真实
//! 环境实测，**S6.status 保持 `not_run`，不宣称 G2 达标**。S6 首个先导样本见 `tests/spikes/s6/`。

pub mod classify;
pub mod census;
pub mod endpoint;
pub mod holepunch;
pub mod natpmp;
pub mod pcp;
pub mod punch_route;
pub mod punch_wire;
pub mod reach;
pub mod relay;
pub mod relay_tunnel;
pub mod relay_udp;
pub mod secure;
pub mod relay_server;
pub mod stun;
pub mod stun_server;
pub mod tcp_punch;
pub mod upnp;

pub use census::{
    double_nat, summarize, MappingBehavior, CensusRecord, CensusSummary, SampleMeta,
};

pub use classify::{
    classify_from_filter, classify_from_mappings, classify_from_probe, classify_two_axis,
    cgnat_suspected, mapping_axis, FilterBehavior, MappingAxis, NatType,
};
pub use punch_route::{
    handshake_frame, validate_handshake, NegotiateError, Route, RouteNegotiator, Stage, Verdict,
    HANDSHAKE_LEN, VERDICT_LEN,
};
pub use stun::{probe_port_filtering, FilterOutcome, FilterProbe};
pub use endpoint::{
    build_plan, is_public_ipv4, local_advertisement, node_id_16, split_mapped,
    tracker_endpoint_for, Endpoint, EndpointKind, EndpointPlan, PeerAdvertisement,
};
pub use relay_server::{
    read_frame, write_frame, RelayHandle, RelayServer, RelayStats, DEFAULT_MAX_FRAME, HEADER_LEN,
};
pub use relay_udp::{
    encode_datagram, parse_datagram, UdpFrame, UdpKind, UdpLimits, UdpParseError, UdpRelayClient,
    UdpRelayHandle, UdpRelayServer, UdpRelayStats, UdpStatus, UDP_MAX_PAYLOAD, UDP_MIN_DGRAM,
};

/// NAT 类别清单（四类 + CGNAT），**描述性用途**。
///
/// ADR-016（2026-11-17）后它**不再是 S6 的 pass 前提**：类别覆盖降级为 `--census`
/// 归档清单与报告标注，缺任何一类都不得据此判 S6 失败，也不得折算成矩阵覆盖。
/// S6 现在按 eMule 式可达语义验收（见 `S6_REACH_STANCE`）。
pub const S6_REQUIRED_MATRIX: [NatType; 5] = [
    NatType::FullCone,
    NatType::RestrictedCone,
    NatType::PortRestrictedCone,
    NatType::Symmetric,
    NatType::Cgnat,
];

/// S6 阶段性立场：**与 eMule 同语义的可达性**（ADR-016，2026-11-17 项目所有者决定）。
/// 即「每个节点都有一条可用的路（direct / relay），且失败可预知、可解释」，
/// 不是「穷举 4 类 NAT + CGNAT」，也不是任何可达率数字。
pub const S6_REACH_STANCE: &str = "emule-like-reach";

/// 类别覆盖是否构成 pass 前提：**否**。保留字段是为了让机器读到
/// 「这里本来会要求覆盖，我们明确选择不要求」，而不是以为漏了。
pub const S6_CLASS_COVERAGE_REQUIRED_FOR_PASS: bool = false;

/// S6 声明口径：满足判据后 G2 只能这样写，不得写「NAT 矩阵通过 / G2 达标」。
pub const S6_CLAIM_WORDING: &str = "按 eMule 语义结案，带已知限制";
/// R-04 的立场：**尽力提供一种可能性，不承诺任何数值**（ADR-012，2026-11-16 项目所有者决定）。
///
/// 旧的 `R04_MIN_REACHABILITY = 0.70` 已删除。理由不是「达不到」，而是**这个数字从一开始
/// 就不该被承诺**：产品提供的是「多一条能叫醒机器的路」，不是「公网可达率 70%」这种 SLA。
/// 一旦写成阈值，「测不到 70%」就会被读成「不合格」，从而逼出两条坏路——
/// 要么为了凑数字去挑环境，要么把没测到的格子填成「看起来通过」。
/// 现在保留的是**描述性统计**（每类各自的观测成功率与样本量）与**定性要求**：
/// 失败路径必须有可用回退，且失败要能被感知和解释。
pub const R04_STANCE: &str = "best-effort";
/// 数值承诺：无。字段保留是为了让机器能读到「这里本来会有一个数，我们明确选择不给」，
/// 而不是让读方以为「漏了」。
pub const R04_NUMERIC_COMMITMENT: Option<f64> = None;
pub const MILESTONE: &str = "M2/W3.3 (S6)";

/// 穿透策略（按优先级排序的推荐结果）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    DirectLan,  // 同子网直连，无需穿透
    Upnp,       // UPnP IGD 端口映射（首选，外网端口可预测）
    NatPmp,     // NAT-PMP 端口映射
    Pcp,        // PCP 端口映射
    Predictive, // 已知 cone NAT：UDP 预测打洞
    Relay,      // 对称 NAT / CGNAT：经可信中继转发
    None,       // 无可用手段
}

impl Strategy {
    pub fn label(&self) -> &'static str {
        match self {
            Strategy::DirectLan => "direct-lan",
            Strategy::Upnp => "upnp-igd",
            Strategy::NatPmp => "nat-pmp",
            Strategy::Pcp => "pcp",
            Strategy::Predictive => "udp-predictive",
            Strategy::Relay => "relay-fallback",
            Strategy::None => "none",
        }
    }
}

/// 一次完整穿透探测的报告。
#[derive(Debug, Clone)]
pub struct TraversalReport {
    pub nat_type: NatType,
    pub public_udp_egress_ok: bool,
    pub upnp_available: bool,
    pub upnp_external_ip: Option<String>,
    pub natpmp_available: bool,
    pub natpmp_external_ip: Option<String>,
    pub pcp_available: bool,
    pub cgnat_suspected: bool,
    pub mapped_address: Option<String>,
    pub recommended_strategy: Strategy,
    /// RFC 3489 Test II/III 的实测过滤行为。**与 `nat_type` 分开存**：`nat_type` 是结论，
    /// 这个字段是结论的依据；只有 `Unknown` 时才允许 `nat_type` 停在 `full-cone` 占位。
    pub filter_behavior: FilterBehavior,
    /// 得出上述结论的原始观测（`(stun 服务器, 观测到的映射地址)`，仅成功项）。
    /// 存在这里是为了让任何一次分类都能回溯到**具体哪几条报文**——
    /// 说不出依据的分类必须返回 Unknown，而不是给一个看起来合理的结果。
    pub observations: Vec<(String, String)>,
}

/// 穿透探测配置。所有字段可选，便于在测试与受限环境逐步启用。
pub struct Traversal {
    pub stun_servers: Vec<(String, u16)>,
    pub gateway: Option<String>,
    pub upnp_location: Option<String>,
    pub relay_addr: Option<String>,
}

impl Default for Traversal {
    fn default() -> Self {
        Self {
            stun_servers: vec![
                ("stun.l.google.com".into(), 19302),
                ("stun.cloudflare.com".into(), 3478),
            ],
            gateway: None,
            upnp_location: None,
            relay_addr: None,
        }
    }
}

/// 把 "ip:port" 拆开（`StunResult.server` 的格式）。
fn split_host_port(s: &str) -> Option<(String, u16)> {
    let (h, p) = s.rsplit_once(':')?;
    Some((h.to_string(), p.parse().ok()?))
}

impl Traversal {
    pub fn new() -> Self {
        Self::default()
    }

    /// 运行完整探测。无网络环境（无 STUN 服务器 / 无网关）时返回 Unknown + 各协议不可用，
    /// 不报错——这是受控降级，真实可用性由运行环境决定。
    pub fn run(&self, timeout: std::time::Duration) -> TraversalReport {
        // 1) STUN：复用同一 socket 探测多服务器
        let mut nat_type = NatType::Unknown;
        let mut mapped_address = None;
        let mut public_udp_egress_ok = false;
        let mut filter_behavior = FilterBehavior::Unknown;
        let mut observations: Vec<(String, String)> = Vec::new();
        if !self.stun_servers.is_empty() {
            if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0") {
                let session = stun::StunSession::new(self.stun_servers.clone());
                let results = session.probe(&sock, timeout);
                let observed: Vec<(String, String)> = results
                    .iter()
                    .filter(|r| r.ok)
                    .filter_map(|r| r.mapped.clone().map(|m| (r.server.clone(), m)))
                    .collect();
                if let Some((_, first)) = observed.first() {
                    public_udp_egress_ok = true;
                    mapped_address = Some(first.clone());
                    nat_type = classify_from_mappings(&observed);
                    // 只有确认了 EIM（≥2 个目的地址映射一致）才值得细分 cone 子型：
                    // 用 CHANGE-REQUEST 实测，不再拿 OTHER-ADDRESS 去猜（那是恒定值，不是测量）。
                    // 样本不足（Unknown）时不得用过滤结果覆盖，否则就是拿一个假设换另一个假设。
                    if nat_type == NatType::FullCone {
                        if let Some(first) = results.iter().find(|r| r.ok) {
                            if let Some((h, p)) = split_host_port(&first.server) {
                                filter_behavior = stun::filter_behavior_fresh(&h, p, timeout);
                                nat_type = classify_from_filter(filter_behavior);
                            }
                        }
                    }
                }
                observations = observed;
            }
        }

        // 2) UPnP IGD
        let mut upnp_available = false;
        let mut upnp_external_ip = None;
        let upnp_loc = match &self.upnp_location {
            Some(l) => Some(l.clone()),
            None => {
                let d = upnp::discover(timeout);
                d.locations.first().cloned()
            }
        };
        if let Some(loc) = upnp_loc {
            let client = upnp::UpnpClient::from_location(loc);
            if let Ok(ip) = client.get_external_ip(timeout) {
                upnp_available = true;
                upnp_external_ip = Some(ip);
            }
        }

        // 3) NAT-PMP
        let mut natpmp_available = false;
        let mut natpmp_external_ip = None;
        if let Some(gw) = &self.gateway {
            let client = natpmp::NatPmpClient::new(gw.clone());
            let r = client.external_address(timeout);
            if r.available {
                natpmp_available = true;
                natpmp_external_ip = r.external_ip;
            }
        }

        // 4) PCP（探测时尝试一次短时映射，便于判定可用性；真实部署应随后维护生命周期）
        let mut pcp_available = false;
        if let Some(gw) = &self.gateway {
            let client = pcp::PcpClient::new(gw.clone());
            if client
                .map(17, 17801, 45000, 120, timeout)
                .is_ok()
            {
                pcp_available = true;
            }
        }

        // 5) CGNAT 启发式：判据是路由器 WAN 侧地址（UPnP/NAT-PMP 学到的外部地址）
        // 落在 100.64/10 或私有段；LAN 侧默认网关本来就是私有地址，不能作为 CGNAT 证据。
        // 外部地址不可用时退回 STUN 映射地址（公网映射不构成 CGNAT 证据）。
        let wan_ip = upnp_external_ip.as_deref()
            .or(natpmp_external_ip.as_deref())
            .or_else(|| mapped_address.as_deref()
                .and_then(|m| m.rsplit_once(':').map(|(ip, _)| ip)))
            .unwrap_or("");
        let cgnat_suspected = cgnat_suspected(wan_ip, "", "");

        // 6) 推荐策略（抽成公开函数，供复用监听 socket 的探测结果回填策略）
        let recommended_strategy = choose_strategy(&TraversalReport {
            nat_type,
            public_udp_egress_ok,
            upnp_available,
            upnp_external_ip: upnp_external_ip.clone(),
            natpmp_available,
            natpmp_external_ip: natpmp_external_ip.clone(),
            pcp_available,
            cgnat_suspected,
            mapped_address: mapped_address.clone(),
            recommended_strategy: Strategy::None,
            filter_behavior,
            observations: observations.clone(),
        }, self.relay_addr.is_some());

        TraversalReport {
            nat_type,
            public_udp_egress_ok,
            upnp_available,
            upnp_external_ip,
            natpmp_available,
            natpmp_external_ip,
            pcp_available,
            cgnat_suspected,
            mapped_address,
            recommended_strategy,
            filter_behavior,
            observations,
        }
    }
}

/// 依据探测结果选择穿透策略。纯函数，便于在回填 STUN 结果后重新决策。
pub fn choose_strategy(report: &TraversalReport, relay_available: bool) -> Strategy {
    if report.upnp_available {
        Strategy::Upnp
    } else if report.natpmp_available {
        Strategy::NatPmp
    } else if report.pcp_available {
        Strategy::Pcp
    } else if report.public_udp_egress_ok
        && report.nat_type != NatType::Symmetric
        && report.nat_type != NatType::Cgnat
    {
        Strategy::Predictive
    } else if relay_available {
        Strategy::Relay
    } else {
        Strategy::None
    }
}

/// 依据策略选择结果决定是否需要部署中继（对称 NAT / CGNAT 且无映射协议时）。
pub fn needs_relay(report: &TraversalReport) -> bool {
    report.recommended_strategy == Strategy::Relay
        || (!report.upnp_available
            && !report.natpmp_available
            && !report.pcp_available
            && report.nat_type == NatType::Symmetric)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matrix_is_a_five_class_descriptive_inventory() {
        // ADR-016 后这个表只是描述性类别清单，不再是 pass 前提；
        // 长度断言保留是为了防止清单被人悄悄删类，而不是为了判覆盖达标。
        assert_eq!(S6_REQUIRED_MATRIX.len(), 5);
    }

    #[test]
    fn s6_stance_has_no_class_coverage_requirement_and_no_number() {
        // ADR-016 + ADR-012：类别覆盖不是 pass 前提，也不许把可达率阈值加回来。
        assert_eq!(S6_REACH_STANCE, "emule-like-reach");
        assert!(!S6_CLASS_COVERAGE_REQUIRED_FOR_PASS);
        assert_eq!(S6_CLAIM_WORDING, "按 eMule 语义结案，带已知限制");
        assert!(R04_NUMERIC_COMMITMENT.is_none());
    }

    #[test]
    fn r04_has_no_numeric_commitment() {
        // ADR-012：R-04 是「尽力」，不是 SLA。这条测试存在的目的是防止有人
        // 出于好心把阈值加回来——它一旦回来，「测不到」就会被读成「不合格」。
        assert_eq!(R04_STANCE, "best-effort");
        assert!(R04_NUMERIC_COMMITMENT.is_none());
    }

    #[test]
    fn upnp_available_means_no_relay_needed() {
        // UPnP 可用时不应退化到中继（需要中继仅在对称 NAT 且无任何映射协议时）
        let r = TraversalReport {
            nat_type: NatType::FullCone,
            public_udp_egress_ok: true,
            upnp_available: true,
            upnp_external_ip: Some("1.2.3.4".into()),
            natpmp_available: false,
            natpmp_external_ip: None,
            pcp_available: false,
            cgnat_suspected: false,
            mapped_address: Some("1.2.3.4:5000".into()),
            recommended_strategy: Strategy::Upnp,
            filter_behavior: FilterBehavior::Unknown,
            observations: Vec::new(),
        };
        assert!(!needs_relay(&r));
    }

    #[test]
    fn symmetric_without_mapping_needs_relay() {
        let r = TraversalReport {
            nat_type: NatType::Symmetric,
            public_udp_egress_ok: true,
            upnp_available: false,
            upnp_external_ip: None,
            natpmp_available: false,
            natpmp_external_ip: None,
            pcp_available: false,
            cgnat_suspected: false,
            mapped_address: Some("1.2.3.4:5000".into()),
            recommended_strategy: Strategy::Relay,
            filter_behavior: FilterBehavior::Unknown,
            observations: Vec::new(),
        };
        assert!(needs_relay(&r));
    }
}

