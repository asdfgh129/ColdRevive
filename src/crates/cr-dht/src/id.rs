//! 160 位节点 ID、XOR 距离与桶下标。
//!
//! Mainline DHT（BEP 5）里「距离」是 XOR 的**无符号**数值大小，不是前缀长度。
//! 两者只在路由表分桶时相关：桶下标 = 与自身共同的**前导位数**，等价于
//! `160 - (xor 最高有效位的位置)`。这里把两件事分开实现，避免把「前缀长」当成
//! 「更近」——那样会让 `closest()` 的排序在数值上和协议不一致（对端不认，
//! 但我们自己的路由表会越用越偏）。

use std::net::SocketAddr;

pub const ID_LEN: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub [u8; ID_LEN]);

impl NodeId {
    pub const ZERO: NodeId = NodeId([0u8; ID_LEN]);

    pub fn new(bytes: [u8; ID_LEN]) -> Self {
        NodeId(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; ID_LEN] {
        &self.0
    }

    /// 随机 ID。
    ///
    /// 用 `xorshift64*` 而不是引入 `rand`：ADR-015 要求协议面 std-only，而节点 ID
    /// **不需要密码学随机性**（它只是一个 160 位标签；真正的身份校验由 PSK 层做）。
    /// 种子混合了时间、进程号、栈地址与进程内计数器——同进程内连续创建的节点
    /// 必然得到不同 ID，这在回环测试里是硬要求（两个节点同 ID 会互相覆盖路由表）。
    pub fn random() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let stack = &seq as *const u64 as u64;
        let mut s = now ^ (std::process::id() as u64).rotate_left(17) ^ stack.rotate_left(31)
            ^ seq.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        if s == 0 {
            s = 0x1234_5678_9ABC_DEF0;
        }
        let mut out = [0u8; ID_LEN];
        for chunk in out.chunks_mut(8) {
            s ^= s >> 12;
            s ^= s << 25;
            s ^= s >> 27;
            let v = s.wrapping_mul(0x2545_F491_4F6C_DD1D);
            for (i, b) in chunk.iter_mut().enumerate() {
                *b = (v >> (8 * i)) as u8;
            }
        }
        NodeId(out)
    }

    pub fn from_hex(hex: &str) -> Result<Self, String> {
        let clean: String = hex.chars().filter(|c| !c.is_ascii_whitespace()).collect();
        if clean.len() != ID_LEN * 2 {
            return Err(format!("节点 ID 需要 {} 个十六进制字符，实际 {}", ID_LEN * 2, clean.len()));
        }
        let mut out = [0u8; ID_LEN];
        for (i, slot) in out.iter_mut().enumerate() {
            let s = &clean[i * 2..i * 2 + 2];
            *slot = u8::from_str_radix(s, 16).map_err(|e| format!("节点 ID 非法 {s}: {e}"))?;
        }
        Ok(NodeId(out))
    }

    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn distance(&self, other: &NodeId) -> [u8; ID_LEN] {
        let mut d = [0u8; ID_LEN];
        for i in 0..ID_LEN {
            d[i] = self.0[i] ^ other.0[i];
        }
        d
    }

    /// 与 `other` 的共同前导位数（0..=160）。160 = 同一个 ID。
    ///
    /// 这是路由表的桶下标：前导位越多说明「越接近自己」，因此放在越靠后的桶里。
    /// 与 `distance` 是同一信息的两种读法，但**不能互相替代**：
    /// 相近的数值距离不保证相同前缀长度，而桶必须按前缀划分（对端按同一规则算）。
    pub fn common_prefix_bits(&self, other: &NodeId) -> usize {
        let mut bits = 0;
        for i in 0..ID_LEN {
            let x = self.0[i] ^ other.0[i];
            if x == 0 {
                bits += 8;
                continue;
            }
            bits += x.leading_zeros() as usize;
            break;
        }
        bits
    }

    /// `a` 是否比 `b` 更接近 `target`（按无符号 XOR 距离）。
    pub fn closer_to(target: &NodeId, a: &NodeId, b: &NodeId) -> bool {
        target.distance(a) < target.distance(b)
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// 路由表里的一条联系信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contact {
    pub id: NodeId,
    pub addr: SocketAddr,
}

/// compact 节点信息：20 字节 ID + 4 字节 IPv4 + 2 字节大端端口 = 26 字节（BEP 5）。
pub const COMPACT_NODE_LEN: usize = ID_LEN + 4 + 2;

pub fn encode_compact_nodes(items: &[Contact]) -> Vec<u8> {
    let mut out = Vec::with_capacity(items.len() * COMPACT_NODE_LEN);
    for c in items {
        if let std::net::IpAddr::V4(v4) = c.addr.ip() {
            out.extend_from_slice(c.id.as_bytes());
            out.extend_from_slice(&v4.octets());
            out.extend_from_slice(&c.addr.port().to_be_bytes());
        }
    }
    out
}

/// 解析 compact 节点串。任何长度不整除、或含非法端口（0）的记录都整条跳过——
/// 不做「猜一个端口」的兜底，那只会把坏数据变成假节点。
pub fn decode_compact_nodes(buf: &[u8]) -> Vec<Contact> {
    let mut out = Vec::new();
    for raw in buf.chunks_exact(COMPACT_NODE_LEN) {
        let mut id = [0u8; ID_LEN];
        id.copy_from_slice(&raw[..ID_LEN]);
        let mut ip = [0u8; 4];
        ip.copy_from_slice(&raw[ID_LEN..ID_LEN + 4]);
        let port = u16::from_be_bytes([raw[ID_LEN + 4], raw[ID_LEN + 5]]);
        if port == 0 {
            continue;
        }
        let ip = std::net::Ipv4Addr::from(ip);
        if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() || ip.is_broadcast() {
            // 协议本身只搬 4 字节地址，不保证可用性；但把不可路由地址塞进路由表
            // 只会浪费每轮查询的 fan-out，所以在入口就丢掉。
            continue;
        }
        out.push(Contact {
            id: NodeId(id),
            addr: SocketAddr::from((ip, port)),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id_with_prefix_nibble(n: u8) -> NodeId {
        let mut b = [0u8; ID_LEN];
        b[0] = n;
        NodeId(b)
    }

    #[test]
    fn hex_roundtrip() {
        let id = NodeId::random();
        assert_eq!(NodeId::from_hex(&id.to_hex()).unwrap(), id);
        assert!(NodeId::from_hex("zz").is_err());
        assert!(NodeId::from_hex("00").is_err(), "长度不足必须报错");
    }

    #[test]
    fn random_ids_differ_within_process() {
        // 回环测试里两个节点同 ID 会互相覆盖路由表，所以这里必须钉住。
        let a = NodeId::random();
        let b = NodeId::random();
        assert_ne!(a, b);
        assert_ne!(a, NodeId::ZERO);
    }

    #[test]
    fn xor_distance_is_symmetric_and_zero_for_self() {
        let a = id_with_prefix_nibble(0x10);
        let b = id_with_prefix_nibble(0x20);
        assert_eq!(a.distance(&b), b.distance(&a));
        assert_eq!(a.distance(&b)[0], 0x30);
        assert_eq!(a.distance(&a), [0u8; ID_LEN]);
    }

    #[test]
    fn prefix_bits_match_distance_high_bit() {
        let z = NodeId::ZERO;
        assert_eq!(z.common_prefix_bits(&id_with_prefix_nibble(0x80)), 0);
        assert_eq!(z.common_prefix_bits(&id_with_prefix_nibble(0x01)), 7);
        assert_eq!(z.common_prefix_bits(&NodeId::ZERO), 160);
        // 前 10 字节相同 → 80 位前缀。
        let mut b = [0u8; ID_LEN];
        b[10] = 0xFF;
        assert_eq!(z.common_prefix_bits(&NodeId(b)), 80);
    }

    #[test]
    fn closer_to_uses_unsigned_numeric_distance() {
        let target = NodeId::ZERO;
        let near = id_with_prefix_nibble(0x01);
        let far = id_with_prefix_nibble(0x80);
        assert!(NodeId::closer_to(&target, &near, &far));
        // 前缀长度与数值距离可能给出相反直觉：0x7f 与 0x80 的前缀都是 1 位，
        // 但 0x7f 在数值上更近。排序必须按数值。
        let a = id_with_prefix_nibble(0x7F);
        let b = id_with_prefix_nibble(0x80);
        assert_eq!(NodeId::ZERO.common_prefix_bits(&a), 1);
        assert_eq!(NodeId::ZERO.common_prefix_bits(&b), 0);
        assert!(NodeId::closer_to(&target, &a, &b));
    }

    #[test]
    fn compact_nodes_roundtrip_and_filter() {
        let c = Contact { id: NodeId::random(), addr: "112.5.60.4:6881".parse().unwrap() };
        let enc = encode_compact_nodes(&[c]);
        assert_eq!(enc.len(), COMPACT_NODE_LEN);
        assert_eq!(decode_compact_nodes(&enc), vec![c]);

        // 回环与 0 端口记录必须被丢掉，不能变成路由表里的假节点。
        let mut bad = Vec::new();
        bad.extend_from_slice(c.id.as_bytes());
        bad.extend_from_slice(&[127, 0, 0, 1]);
        bad.extend_from_slice(&6881u16.to_be_bytes());
        assert!(decode_compact_nodes(&bad).is_empty());

        let mut zero_port = Vec::new();
        zero_port.extend_from_slice(c.id.as_bytes());
        zero_port.extend_from_slice(&[112, 5, 60, 4]);
        zero_port.extend_from_slice(&0u16.to_be_bytes());
        assert!(decode_compact_nodes(&zero_port).is_empty());

        // 长度不整除时整条截断，不 panic。
        assert!(decode_compact_nodes(&enc[..enc.len() - 1]).is_empty());
    }
}
