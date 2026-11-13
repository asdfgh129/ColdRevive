//! cr-ed2k — W5.3 跨协议补源（P2 / M5 骨架）。
//! ADR-011 的 L1/L2 参考实现仍在 `tests/spikes/s2/`，按门禁未迁入生产 crate。

pub const MILESTONE: &str = "M5/W5.3 (P2)";
pub const ADR: &str = "ADR-011";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skeleton_records_adr() { assert_eq!(ADR, "ADR-011"); }
}
