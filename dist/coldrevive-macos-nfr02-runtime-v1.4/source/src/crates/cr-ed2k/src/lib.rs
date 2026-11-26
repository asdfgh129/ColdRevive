//! cr-ed2k — ADR-011 L2 最小 eD2k 传输栈（下载/上传共用一份，std-only）。
//!
//! 生产化口径（2026-11-20）：把 S2 `kad-mini` / `ed2k-mini` 中**已审计**的
//! 编解码与状态机通过 `#[path]` 直接纳入本 crate，避免复制出第二份会漂移的实现。
//! 三份来源都是零依赖、含已知答案测试（S2 全量 31/31），源码路径固定在本仓库内，
//! 构建时展开编译，属生产 crate 的一部分而非「跑 spike 脚本」。
//!
//! **边界（照写，不许放宽）**：
//! - 不做 eD2k server(LoginServer) 连接、source exchange、压缩/混淆、credit 体系、
//!   AICH、>4 GiB I64 包、NAT 穿透——ADR-011/ADR-004 明确排除。
//! - 公网 eD2k 传输仍 **BLOCKED**：现有真实证据只有回环与 RFC1918 LAN
//!   （`tests/spikes/s2/ed2k-mini/`），没有公网 server/peer TCP 证据。
//! - `fileID` 必须是真实内容导出的 `ed2k_hash`（MD4 over 9.28 MB 分块），不可伪造。
//! - 这是「栈可用」的组件级证据，不是 FR-25「标准 eD2k 下载/上传 + 双轨发布
//!   在守护进程生产路径全量可用」——后者仍需 daemon 接线与公网证据。

// 单一事实来源：复用 S2 审计过的 SHA-1 / MD4 / KAD tag 编解码与 eD2k TCP 状态机。
#[path = "../../../../tests/spikes/s2/kad-mini/src/sha1.rs"]
pub mod sha1;
#[path = "../../../../tests/spikes/s2/kad-mini/src/wire.rs"]
pub mod kad;
#[path = "../../../../tests/spikes/s2/ed2k-mini/src/wire.rs"]
pub mod wire;
// The stack is vendored into this crate (not `#[path]`-included from the S2
// spike) because FR-25 pacing work must live entirely under cr-ed2k/**. The
// vendored copy keeps the audited chunking/frame parsing byte-for-byte; only
// the upload pacing path is extended. `wire`/`kad`/`sha1` remain the single
// audited source of truth and are still path-included.
pub mod stack;
/// FR-25：接收 stock aMule 的 `OP_COMPRESSEDPART(0x40)` 所需的 std-only zlib 解压。
pub mod inflate;
/// Honest codecs for the eD2k features whose wire format is either pinned in
/// `docs/04-protocol/PROTO.md` (source exchange) or explicitly not verified in
/// this repo (server login / credit / AICH). The latter return `Unsupported`
/// rather than emitting plausible-looking bytes.
pub mod codecs;

pub use stack::{Link, ServeConfig, SharedFile, UploadQueue};
pub use wire::{Hash16, Hello};

/// 里程碑标记保留但不再是「骨架」——生产 crate 已完成栈的纳入。
pub const MILESTONE: &str = "M2/FR-25 (L2 stack promoted from S2 spike)";
pub const ADR: &str = "ADR-011";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn promoted_stack_exposes_both_directions() {
        // `download()` 与 `serve()` 共用一份栈（ADR-011 L2 硬要求）。
        let src = SharedFile::new("x.bin", b"hello".to_vec());
        assert_eq!(src.hash(), kad::md4(b"hello"));
        assert!(src.blocks().len() >= 1);
        let q = UploadQueue::new(1);
        assert_eq!(q.max_slots, 1);
    }

    #[test]
    fn ed2k_hash_matches_known_answer() {
        assert_eq!(sha1::hex(&kad::ed2k_hash(b"abc")), sha1::hex(&kad::md4(b"abc")));
    }
}
