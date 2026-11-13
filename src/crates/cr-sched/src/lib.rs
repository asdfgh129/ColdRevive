//! cr-sched — W4.3 LRU 冷热调度器（M2 骨架）。
//! 真实 LRU、活跃池上限与暂停造种队列属 M2；G1 通过前不实现。

pub const MILESTONE: &str = "M2/W4.3";
pub const MAX_ACTIVE_SEEDS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Temperature { Cold, Hot }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skeleton_records_milestone() { assert_eq!(MILESTONE, "M2/W4.3"); }
}
