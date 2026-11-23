//! cr-reputation — W4.2 限流 / 黑名单 / 信誉（M4 骨架）。
//! M1 的 fault injection 只记录 HASH_MISMATCH 拉黑事件，不做持久化信誉评分。

use std::collections::BTreeMap;

pub const MILESTONE: &str = "M4/W4.2";
pub const DEFAULT_WAKE_RATE_PER_MIN: usize = 5;

/// 跨协议补源 / 信誉的诚实状态：只有骨架，没有评分实现。
pub const REPUTATION_STATUS: &str = "not_implemented";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceKind {
    HashMismatch,
    SourceConflict,
    VerifiedTransfer,
}

impl EvidenceKind {
    pub fn label(self) -> &'static str {
        match self {
            EvidenceKind::HashMismatch => "hash_mismatch",
            EvidenceKind::SourceConflict => "source_conflict",
            EvidenceKind::VerifiedTransfer => "verified_transfer",
        }
    }
}

/// 只记账、不评分的骨架账本。
///
/// `observe` 累加真实传入的事件计数；`score` 恒为 `None`——没有持久化信誉模型时
/// 返回一个「看起来像分」的数字就是假实现。状态永远显式 `not_implemented`。
#[derive(Debug, Default)]
pub struct ReputationLedger {
    counts: BTreeMap<&'static str, u64>,
}

impl ReputationLedger {
    pub fn new() -> Self { Self { counts: BTreeMap::new() } }

    pub fn observe(&mut self, kind: EvidenceKind) {
        *self.counts.entry(kind.label()).or_insert(0) += 1;
    }

    pub fn count(&self, kind: EvidenceKind) -> u64 {
        *self.counts.get(kind.label()).unwrap_or(&0)
    }

    pub fn total(&self) -> u64 { self.counts.values().sum() }

    /// 骨架阶段永远没有分数：调用方必须据此显示「未实现」，不得显示 0 分或满分。
    pub fn score(&self, _peer: &str) -> Option<f64> { None }

    pub fn status(&self) -> &'static str { REPUTATION_STATUS }
    pub fn enabled(&self) -> bool { false }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skeleton_records_milestone() { assert_eq!(MILESTONE, "M4/W4.2"); }

    #[test]
    fn skeleton_is_explicitly_not_implemented() {
        let mut led = ReputationLedger::new();
        assert_eq!(led.status(), "not_implemented");
        assert!(!led.enabled());
        assert_eq!(led.score("peer"), None);
        led.observe(EvidenceKind::HashMismatch);
        led.observe(EvidenceKind::HashMismatch);
        led.observe(EvidenceKind::VerifiedTransfer);
        assert_eq!(led.count(EvidenceKind::HashMismatch), 2);
        assert_eq!(led.total(), 3);
        assert_eq!(led.score("peer"), None, "记账不等于评分");
    }
}
