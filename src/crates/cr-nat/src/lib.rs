//! cr-nat — W3.3 连通性（M2 穿透栈，**实现中 / 合法前置**）。
//!
//! 包含：RFC 5389 STUN、RFC 6886 NAT-PMP、RFC 6887 PCP、UPnP IGD、中继回退（LowID），
//! 以及基于 STUN 映射的 NAT 类型分类与穿透策略选择。
//!
//! **门禁声明**：本实现是 M2/S6 的合法前置（与建骨架 + S5 实测同口径），用于让跨 NAT 场景
//! 在 G1 解锁后具备可达性手段；但四类 NAT（含 CGNAT）的真实可达率矩阵仍须 M2 在多个真实
//! 环境实测，**S6.status 保持 `not_run`，不宣称 G2 达标**。S6 首个先导样本见 `tests/spikes/s6/`。

pub mod classify;
pub mod natpmp;
pub mod pcp;
pub mod relay;
pub mod stun;
pub mod upnp;

pub use classify::{classify_cone_subtype, classify_from_mappings, cgnat_suspected, NatType};

/// S6 判据要求的 NAT 类型矩阵（四类 + CGNAT）。
pub const S6_REQUIRED_MATRIX: [NatType; 5] = [
    NatType::FullCone,
    NatType::RestrictedCone,
    NatType::PortRestrictedCone,
    NatType::Symmetric,
    NatType::Cgnat,
];
pub const R04_MIN_REACHABILITY: f64 = 0.70;
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
        if !self.stun_servers.is_empty() {
            if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0") {
                let session = stun::StunSession::new(self.stun_servers.clone());
                let results = session.probe(&sock, timeout);
                let mapped: Vec<String> = results
                    .iter()
                    .filter_map(|r| r.mapped.clone())
                    .collect();
                if !mapped.is_empty() {
                    public_udp_egress_ok = true;
                    mapped_address = Some(mapped[0].clone());
                    nat_type = classify_from_mappings(&mapped);
                    // 若首个响应带 OTHER-ADDRESS，尝试细分 cone 子型
                    if let Some(first) = results.iter().find(|r| r.ok) {
                        nat_type = classify_cone_subtype(
                            first.mapped.as_deref().unwrap_or(""),
                            &first.other,
                        );
                    }
                }
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

        // 5) CGNAT 启发式（依赖本机地址与网关；此处仅用 mapped 地址近似，精确检测在上层）
        let cgnat_suspected = match &mapped_address {
            Some(m) => {
                let ip = m.rsplit(':').next().unwrap_or("");
                cgnat_suspected(ip, self.gateway.as_deref().unwrap_or(""), "")
            }
            None => false,
        };

        // 6) 推荐策略
        let recommended_strategy = if upnp_available {
            Strategy::Upnp
        } else if natpmp_available {
            Strategy::NatPmp
        } else if pcp_available {
            Strategy::Pcp
        } else if public_udp_egress_ok && nat_type != NatType::Symmetric {
            Strategy::Predictive
        } else if self.relay_addr.is_some() {
            Strategy::Relay
        } else {
            Strategy::None
        };

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
        }
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
    fn matrix_covers_all_four_nat_types_plus_cgnat() {
        assert_eq!(S6_REQUIRED_MATRIX.len(), 5);
        assert_eq!(R04_MIN_REACHABILITY, 0.70);
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
        };
        assert!(needs_relay(&r));
    }
}

