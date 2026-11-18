//! cr-dht — W3.2 Mainline DHT（BEP 5）适配层，std-only。
//!
//! 组成：`bencode`（编解码）→ `id`（160 位 ID / XOR 距离 / compact 编码）→
//! `table`（Kademlia 桶）→ `krpc`（四种查询的报文）→ `node`（UDP 节点与服务端）。
//!
//! **命名空间隔离（S3）**：Mainline 是 160 位 ID，eMule KAD 是 128 位。两者不能互相
//! 解释——把 160 位 ID 截断成 128 位去找 KAD 节点，得到的是一张「看起来有节点、
//! 实际全是错位」的表。`DHT_ID_BITS != KAD_ID_BITS` 这条断言存在的意义就是钉住这点。
//!
//! **判据边界**：本 crate 的测试只覆盖报文与状态机的自洽（回环 UDP 真实收发）。
//! 公网宣告/取回的证据在 `tests/spikes/s3/mainline_probe/`，用的是已锁定的
//! `librqbit-dht 9.1.0`。本 crate 不宣称任何公网可达性，也不是 S6 证据。

pub mod bencode;
pub mod id;
pub mod krpc;
pub mod node;
pub mod table;

pub use bencode::Value;
pub use id::{Contact, NodeId, COMPACT_NODE_LEN, ID_LEN};
pub use krpc::{Message, Query, Response};
pub use node::{DhtClient, DhtNode, DhtServer, DhtStats, ServerHandle, MAX_DATAGRAM};
pub use table::{RoutingTable, BUCKETS, K};

/// 里程碑标签（与文档/机读清单对齐）。
pub const MILESTONE: &str = "M2/W3.2";
pub const DHT_ID_BITS: usize = 160;
pub const KAD_ID_BITS: usize = 128;

/// 本 crate 是否提供 S6 判据证据：**否**。
///
/// 保留这个常量是为了让读代码的人一眼看到边界，而不是以为「实现了 DHT 就等于
/// S3/S6 有证据」。S3 的证据是公网闭环实测，S6 是中继/直连可达性实测。
pub const IS_S6_EVIDENCE: bool = false;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dual_stack_id_widths_differ() {
        assert_ne!(DHT_ID_BITS, KAD_ID_BITS);
        assert_eq!(DHT_ID_BITS, ID_LEN * 8);
    }

    #[test]
    fn milestone_and_evidence_boundary_are_explicit() {
        assert_eq!(MILESTONE, "M2/W3.2");
        assert!(!IS_S6_EVIDENCE, "实现 DHT 不等于拿到 S6 证据");
    }
}
