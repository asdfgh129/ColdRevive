//! cr-guard — W4.4 握手拦截器（M2 骨架）。
//! ADR-002：握手拦截自实现于 `cr-engine` 外围；真实实现属 M2，G1 通过前不落地。

pub const MILESTONE: &str = "M2/W4.4";
pub const INTERCEPTS_STANDARD_BT_HANDSHAKE: bool = true;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skeleton_records_milestone() { assert_eq!(MILESTONE, "M2/W4.4"); }
}
