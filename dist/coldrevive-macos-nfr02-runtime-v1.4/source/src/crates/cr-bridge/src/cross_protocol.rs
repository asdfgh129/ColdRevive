//! 跨协议补源（FR-23）骨架状态 API。
//!
//! BT 的 infohash 与 eD2k 的 fileID 是两套不同的身份，不能靠哈希拼接互相推导。
//! 本模块只提供**诚实的未实现状态**与输入形状校验：没有任何一条路径会返回
//! 「补源成功」或凭空生成对等来源。

/// 跨协议补源的实现状态。
pub const SOURCE_SUPPLEMENT_STATUS: &str = "not_implemented";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupplementOutcome {
    /// 输入本身不合法（不是失败于补源，而是没给出可识别的身份）。
    Invalid { reason: String },
    /// 身份格式合法，但跨协议映射尚未实现——绝不返回成功。
    NotImplemented { from: &'static str, to: &'static str },
}

impl SupplementOutcome {
    pub fn status_label(&self) -> &'static str {
        match self {
            SupplementOutcome::Invalid { .. } => "invalid_input",
            SupplementOutcome::NotImplemented { .. } => SOURCE_SUPPLEMENT_STATUS,
        }
    }
    pub fn succeeded(&self) -> bool { false }
}

/// 从 BT infohash 补 eD2k source：骨架，只校验 40 位十六进制形状。
pub fn supplement_bt_to_ed2k(info_hash: &str) -> SupplementOutcome {
    if info_hash.len() != 40 || !info_hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return SupplementOutcome::Invalid {
            reason: "infohash 必须是 40 位十六进制".into(),
        };
    }
    SupplementOutcome::NotImplemented { from: "bt", to: "ed2k" }
}

/// 从 eD2k fileID 补 BT source：骨架，只校验 32 位十六进制形状。
pub fn supplement_ed2k_to_bt(file_id: &str) -> SupplementOutcome {
    if file_id.len() != 32 || !file_id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return SupplementOutcome::Invalid {
            reason: "eD2k fileID 必须是 32 位十六进制".into(),
        };
    }
    SupplementOutcome::NotImplemented { from: "ed2k", to: "bt" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skeleton_never_claims_success() {
        let good_ih = "a".repeat(40);
        let good_id = "b".repeat(32);
        let a = supplement_bt_to_ed2k(&good_ih);
        let b = supplement_ed2k_to_bt(&good_id);
        assert_eq!(a.status_label(), "not_implemented");
        assert_eq!(b.status_label(), "not_implemented");
        assert!(!a.succeeded());
        assert!(!b.succeeded());
        assert_eq!(a, SupplementOutcome::NotImplemented { from: "bt", to: "ed2k" });
    }

    #[test]
    fn malformed_identity_is_rejected_before_any_supplement() {
        assert_eq!(supplement_bt_to_ed2k("zz").status_label(), "invalid_input");
        assert_eq!(supplement_ed2k_to_bt(&"a".repeat(40)).status_label(), "invalid_input");
    }
}
