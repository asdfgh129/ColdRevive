//! cr-nat — W3.3 连通性（M2 骨架）。
//!
//! S6 首个真实 NAT 先导样本（2026-11-13）：cone 类、公网 UDP 可达、
//! UPnP IGD / NAT-PMP **均不可用**、未见 CGNAT。四类 NAT + CGNAT 的
//! 可达率实测（R-04 阈值 ≥70%）属 M2 必测回填，须在 G1 通过后进行。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatType { FullCone, RestrictedCone, PortRestrictedCone, Symmetric, Cgnat }

pub const S6_REQUIRED_MATRIX: [NatType; 5] = [
    NatType::FullCone, NatType::RestrictedCone, NatType::PortRestrictedCone,
    NatType::Symmetric, NatType::Cgnat,
];
pub const R04_MIN_REACHABILITY: f64 = 0.70;
pub const MILESTONE: &str = "M2/W3.3 (S6)";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matrix_covers_all_four_nat_types_plus_cgnat() {
        assert_eq!(S6_REQUIRED_MATRIX.len(), 5);
        assert_eq!(R04_MIN_REACHABILITY, 0.70);
    }
}
