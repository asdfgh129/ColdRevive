//! cr-reputation — W4.2 限流 / 黑名单 / 信誉（M4 骨架）。
//! M1 的 fault injection 只记录 HASH_MISMATCH 拉黑事件，不做持久化信誉评分。

pub const MILESTONE: &str = "M4/W4.2";
pub const DEFAULT_WAKE_RATE_PER_MIN: usize = 5;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skeleton_records_milestone() { assert_eq!(MILESTONE, "M4/W4.2"); }
}
