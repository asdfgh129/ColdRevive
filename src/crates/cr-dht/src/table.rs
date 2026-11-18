//! 路由表：按共同前缀分桶（BEP 5 的 Kademlia 桶）。
//!
//! 结构：160 个桶，桶 `i` 存放与**本节点**共同的最高位在第 `i` 位之前的节点，
//! 每个桶最多 `K` 个联系人。同一个桶里的节点距离相近，因此「靠近自己」的桶更容易满，
//! 远处的桶通常很空——这正是 Kademlia 想要的形状：近处密、远处疏。
//!
//! 本层只管**记账**，不做存活探测：
//! - 重复 `add` 同一个 ID 只更新地址并把它挪到「最近见过」位置；
//! - 桶满时**丢弃新来者**（BEP 5 建议先 ping 桶内最旧节点再决定踢谁，那需要网络 I/O，
//!   属于节点层；本层若自行踢人就会出现「没人验证过就删掉活节点」）。
//! - 因此路由表的质量上限由调用方决定：谁在用这张表，谁就要负责探测与清理。

use crate::id::{Contact, NodeId};
use std::net::SocketAddr;

/// 每个桶的容量。BEP 5 的 `K = 8`。
pub const K: usize = 8;
/// 桶数量 = ID 位宽。
pub const BUCKETS: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddOutcome {
    /// 新联系人进表。
    Added,
    /// 已存在：只刷新地址与顺序。
    Refreshed,
    /// 桶已满，新联系人被丢弃。
    BucketFull,
    /// 是自己，直接忽略。
    SelfIgnored,
}

#[derive(Debug, Clone)]
pub struct RoutingTable {
    own: NodeId,
    buckets: Vec<Vec<Contact>>,
}

impl RoutingTable {
    pub fn new(own: NodeId) -> Self {
        Self { own, buckets: vec![Vec::new(); BUCKETS] }
    }

    pub fn own_id(&self) -> NodeId {
        self.own
    }

    pub fn bucket_index(&self, other: &NodeId) -> usize {
        // 与自己完全相同时返回 0；调用方在 add 里已挡掉这种情况。
        self.own.common_prefix_bits(other).min(BUCKETS - 1)
    }

    pub fn add(&mut self, contact: Contact) -> AddOutcome {
        if contact.id == self.own {
            return AddOutcome::SelfIgnored;
        }
        if contact.id == NodeId::ZERO {
            // 全零 ID 出现在真实网络里通常意味着对端没填 ID（或攻击者批量伪造）。
            // 收进来会污染「离自己最远」的那个桶，且永远排在最前。
            return AddOutcome::SelfIgnored;
        }
        let idx = self.bucket_index(&contact.id);
        let bucket = &mut self.buckets[idx];
        if let Some(pos) = bucket.iter().position(|c| c.id == contact.id) {
            let mut c = bucket.remove(pos);
            // 地址变了要更新：NAT 后端口会变，沿用旧地址会一直发不到。
            c.addr = contact.addr;
            bucket.push(c);
            return AddOutcome::Refreshed;
        }
        if bucket.len() >= K {
            return AddOutcome::BucketFull;
        }
        bucket.push(contact);
        AddOutcome::Added
    }

    /// 显式移除（探测失败时由节点层调用）。
    pub fn remove(&mut self, id: &NodeId) -> bool {
        if *id == self.own {
            return false;
        }
        let idx = self.bucket_index(id);
        let b = &mut self.buckets[idx];
        let before = b.len();
        b.retain(|c| c.id != *id);
        b.len() != before
    }

    pub fn len(&self) -> usize {
        self.buckets.iter().map(|b| b.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn all(&self) -> Vec<Contact> {
        self.buckets.iter().flatten().copied().collect()
    }

    /// 距离 `target` 最近的 `n` 个联系人（按无符号 XOR 距离升序）。
    pub fn closest(&self, target: &NodeId, n: usize) -> Vec<Contact> {
        let mut v = self.all();
        v.sort_by(|a, b| target.distance(&a.id).cmp(&target.distance(&b.id)));
        v.truncate(n);
        v
    }

    /// 诊断用：每个非空桶的 (下标, 数量)。
    pub fn bucket_sizes(&self) -> Vec<(usize, usize)> {
        self.buckets.iter().enumerate()
            .filter(|(_, b)| !b.is_empty())
            .map(|(i, b)| (i, b.len()))
            .collect()
    }
}

/// 便捷构造（测试与节点自举用）。
pub fn contact(id: NodeId, addr: &str) -> Contact {
    Contact { id, addr: addr.parse::<SocketAddr>().expect("测试地址") }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个与 `own` 恰好共享前 `prefix_bits` 位的 ID：保留该前缀，把下一位取反，
    /// 其余位清零（末字节再叠一个 `tail` 以便区分同桶内的多个节点）。
    fn id_prefixed(own: &NodeId, prefix_bits: usize, tail: u8) -> NodeId {
        let mut b = *own.as_bytes();
        let mut i = 0;
        let mut remaining = prefix_bits;
        while i < 20 && remaining >= 8 {
            i += 1;
            remaining -= 8;
        }
        if i < 20 {
            if remaining == 0 {
                b[i] ^= 0x80;
            } else {
                let keep = 0xFFu8 << (8 - remaining);
                b[i] = (b[i] & keep) | ((!b[i]) & (0x80u8 >> remaining));
            }
            for x in b.iter_mut().skip(i + 1) {
                *x = 0;
            }
        }
        b[19] = b[19].wrapping_add(tail);
        NodeId(b)
    }

    #[test]
    fn add_dedupes_and_refreshes_address() {
        let own = NodeId::random();
        let mut t = RoutingTable::new(own);
        let other = NodeId::random();
        assert_eq!(t.add(contact(other, "112.5.60.4:6881")), AddOutcome::Added);
        assert_eq!(t.add(contact(other, "112.5.60.4:7000")), AddOutcome::Refreshed);
        assert_eq!(t.len(), 1);
        assert_eq!(t.all()[0].addr.port(), 7000, "地址必须刷新");
    }

    #[test]
    fn ignores_self_and_zero_id() {
        let own = NodeId::random();
        let mut t = RoutingTable::new(own);
        assert_eq!(t.add(contact(own, "1.2.3.4:5")), AddOutcome::SelfIgnored);
        assert_eq!(t.add(contact(NodeId::ZERO, "1.2.3.4:5")), AddOutcome::SelfIgnored);
        assert!(t.is_empty());
    }

    #[test]
    fn bucket_holds_at_most_k_then_drops_newcomer() {
        let own = NodeId::random();
        let mut t = RoutingTable::new(own);
        // 同一个桶（与 own 共享 0 位前缀）里塞 K+1 个。
        let mut added = 0;
        for i in 1..=(K as u8 + 2) {
            let mut b = *own.as_bytes();
            b[0] ^= 0x80; // 首位取反 → 共享 0 位
            b[19] = b[19].wrapping_add(i);
            match t.add(contact(NodeId(b), &format!("112.5.60.{}:6881", i))) {
                AddOutcome::Added => added += 1,
                AddOutcome::BucketFull => {}
                other => panic!("意外结果 {other:?}"),
            }
        }
        assert_eq!(added, K);
        assert_eq!(t.len(), K);
    }

    #[test]
    fn bucket_index_tracks_shared_prefix() {
        let own = NodeId::ZERO;
        let t = RoutingTable::new(own);
        let mut b = [0u8; 20];
        b[0] = 0x40; // 与 0 共享 1 位（0100_0000 → 第一位不同）
        assert_eq!(t.bucket_index(&NodeId(b)), 1);
        let mut c = [0u8; 20];
        c[2] = 0x01; // 前 2 字节相同（16 位），第 3 字节只有末位不同 → 共 23 位
        assert_eq!(t.bucket_index(&NodeId(c)), 23);
        // 与自己完全相同（160 位前缀）会夹到最后一个桶；`add` 在此之前就挡掉了自己，
        // 这里只钉住「不会越界」。
        assert_eq!(t.bucket_index(&NodeId::ZERO), BUCKETS - 1);
    }

    #[test]
    fn closest_sorts_by_xor_distance_and_truncates() {
        let own = NodeId::ZERO;
        let mut t = RoutingTable::new(own);
        let mut ids = Vec::new();
        for i in 1..=5u8 {
            let mut b = [0u8; 20];
            b[0] = 0x80 | i; // 共享 0 位，但数值可排序
            b[19] = i;
            let id = NodeId(b);
            ids.push(id);
            t.add(contact(id, &format!("112.5.60.{}:6881", i)));
        }
        let target = NodeId::ZERO;
        let got = t.closest(&target, 3);
        assert_eq!(got.len(), 3);
        let mut expected = ids.clone();
        expected.sort_by(|a, b| target.distance(a).cmp(&target.distance(b)));
        let want: Vec<_> = expected.into_iter().take(3).collect();
        assert_eq!(got.iter().map(|c| c.id).collect::<Vec<_>>(), want);
    }

    #[test]
    fn remove_drops_only_the_target() {
        let own = NodeId::random();
        let mut t = RoutingTable::new(own);
        let a = NodeId::random();
        let b = NodeId::random();
        t.add(contact(a, "112.5.60.4:1"));
        t.add(contact(b, "112.5.60.5:2"));
        assert!(t.remove(&a));
        assert!(!t.remove(&a));
        assert_eq!(t.len(), 1);
        assert_eq!(t.all()[0].id, b);
    }

    #[test]
    fn prefixed_helper_matches_bucket_index() {
        // 自检构造器：它给测试用的「共享 N 位」ID 必须真的落在第 N 号桶。
        let own = NodeId::random();
        let t = RoutingTable::new(own);
        for bits in [0usize, 1, 7, 8, 9, 63] {
            let other = id_prefixed(&own, bits, 1);
            assert_eq!(t.bucket_index(&other), bits, "bits={bits}");
        }
    }
}
