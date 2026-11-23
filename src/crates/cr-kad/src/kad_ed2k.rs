//! eMule KAD 兼容轨（ADR-011 L1）的最小编解码，从 S2 `kad-mini/src/wire.rs` 逐字节搬来。
//!
//! 边界（照写）：这里只做**字节层**，不发包。真正的发包在 `coldrevive-ed2k search`；
//! 生产 daemon 仍只做本地索引，不后台向公共 KAD 发测试 Key（T0 红线）。
//! G0 `protocol_scope` 已于 2026-11-22 由项目所有者书面解除，公共 `0x33`/`0x43`
//! 不再被行政 BLOCKED；但本模块的存在**不代表**已有公网命中证据——那只来自
//! `coldrevive-ed2k search --public` 的真实 UDP 观测。
//!
//! A 轨 Key = `MD4(UTF-8(term))`，**无前缀**，与 cr-kad 的私有 B 轨
//! `SHA1(namespace+kw:term)` 是两套算法，不互换（ADR-011 §5）。

use cr_hash::kad_keyword_hash;
use std::net::{Ipv4Addr, SocketAddrV4};

/// KAD UDP 帧魔数（aMule `PROTO_KAD = 0xE4`）。
pub const PROTO_KAD: u8 = 0xE4;
/// aMule 对 >200 B 的应答改成 `[0xE5][opcode][zlib(payload)]`。
pub const PROTO_KAD_PACKED: u8 = 0xE5;
pub const OP_BOOTSTRAP_REQ: u8 = 0x01;
pub const OP_BOOTSTRAP_RES: u8 = 0x09;
/// `KADEMLIA2_HELLO_REQ/RES/RES_ACK`。HELLO 不是索引操作的行政前置条件，
/// 但它把我们的 KadID/UDP verify key 注入对方路由表，并可完成三次握手。
pub const OP_HELLO_REQ: u8 = 0x11;
pub const OP_HELLO_RES: u8 = 0x19;
pub const OP_HELLO_RES_ACK: u8 = 0x22;
/// KADEMLIA2_REQ / RES：发布和检索前必须先迭代收敛到 Key 的责任邻域。
pub const OP_REQ: u8 = 0x21;
pub const OP_RES: u8 = 0x29;
pub const OP_SEARCH_KEY_REQ: u8 = 0x33;
pub const OP_SEARCH_RES: u8 = 0x3B;
pub const OP_PUBLISH_KEY_REQ: u8 = 0x43;
pub const OP_PUBLISH_RES: u8 = 0x4B;
/// aMule `SEARCHTOLERANCE`：节点与 Key 的 XOR 距离最高 32 位必须 ≤ 该值，
/// 否则索引操作被静默丢弃——这正是「直接把 0x33 发给 bootstrap 节点 0 命中」的原因。
pub const SEARCHTOLERANCE: u32 = 16_777_216;
pub const ID_LEN: usize = 16;
/// aMule `KADEMLIA_VERSION_DEFAULT`（Kad v8）。
pub const KAD_VERSION: u8 = 8;
/// KAD `ReadTag` 类型：UINT8/UINT16 与两个数值 tag 名。
pub const KAD_TAG_U8: u8 = 0x09;
pub const KAD_TAG_U16: u8 = 0x08;
pub const TAG_SOURCEUPORT: u8 = 0xfc;
pub const TAG_KADMISCOPTIONS: u8 = 0xf2;
/// `TAG_KADMISCOPTIONS` bit2：请求对方回 `HELLO_RES_ACK` 完成三次握手。
pub const KAD_MISCOPT_REQUEST_ACK: u8 = 0x04;

/// A 轨关键词 Key：无前缀 MD4(UTF-8(term))，128 bit。
pub fn ed2k_keyword_key(term: &str) -> [u8; ID_LEN] {
    kad_keyword_hash(term)
}

/// `[0xE4][opcode][payload]`，小端整数，ID 原样 16 B。
pub fn frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + payload.len());
    out.push(PROTO_KAD);
    out.push(opcode);
    out.extend_from_slice(payload);
    out
}

/// 解 KAD UDP 帧：`0xE4` 原样返回 payload；`0xE5` 先解 zlib。
/// 不解码的魔数一律拒绝，避免把「对端已应答但格式不同」误记成超时。
pub fn unpack_frame(data: &[u8]) -> Result<(u8, Vec<u8>), String> {
    if data.len() < 2 { return Err(format!("KAD 帧过短: {} B", data.len())); }
    match data[0] {
        PROTO_KAD => Ok((data[1], data[2..].to_vec())),
        PROTO_KAD_PACKED => {
            let payload = crate::kad_inflate::zlib_decode(&data[2..])
                .map_err(|e| format!("0xE5 解包失败: {e}"))?;
            Ok((data[1], payload))
        }
        other => Err(format!("非 KAD 帧头 0x{other:02x}")),
    }
}

/// KAD 路由表联系人。线上 IP 是点分十进制**反序**，解码时必须 reverse。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contact {
    pub id: [u8; ID_LEN],
    pub ip: [u8; 4],
    pub udp: u16,
    pub tcp: u16,
    pub version: u8,
}

impl Contact {
    pub fn addr(&self) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::from(self.ip), self.udp)
    }
    pub fn is_public(&self) -> bool {
        let o = self.ip;
        !(o[0] == 10 || (o[0] == 172 && (16..=31).contains(&o[1]))
          || (o[0] == 192 && o[1] == 168) || o[0] == 127 || (o[0] == 169 && o[1] == 254)
          || o[0] == 0)
    }
}

struct Cur<'a> { b: &'a [u8], i: usize }

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.i + n > self.b.len() {
            return Err(format!("KAD 联系人字段截断: 需要 {n} B，剩 {}", self.b.len() - self.i));
        }
        let out = &self.b[self.i..self.i + n];
        self.i += n;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8, String> { Ok(self.take(1)?[0]) }
    fn u16(&mut self) -> Result<u16, String> {
        let p = self.take(2)?;
        Ok(u16::from_le_bytes([p[0], p[1]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let p = self.take(4)?;
        Ok(u32::from_le_bytes([p[0], p[1], p[2], p[3]]))
    }
    fn id(&mut self) -> Result<[u8; ID_LEN], String> {
        let p = self.take(ID_LEN)?;
        let mut out = [0u8; ID_LEN];
        out.copy_from_slice(p);
        Ok(out)
    }
    fn contact(&mut self) -> Result<Contact, String> {
        let id = self.id()?;
        let mut ip = [0u8; 4];
        ip.copy_from_slice(self.take(4)?);
        ip.reverse();
        Ok(Contact { id, ip, udp: self.u16()?, tcp: self.u16()?, version: self.u8()? })
    }
}

/// `BOOTSTRAP_RES` payload: `<self_id 16><self_tcp 2><version 1><count 2>[contact]*`。
/// 返回的 `self_id` 是该节点自己的 KadID，后续 0x21 的 `receiver` 必须填它。
pub fn parse_bootstrap_res(payload: &[u8]) -> Result<([u8; ID_LEN], Vec<Contact>), String> {
    let mut c = Cur { b: payload, i: 0 };
    let self_id = c.id()?;
    let _self_tcp = c.u16()?;
    let _version = c.u8()?;
    let n = c.u16()? as usize;
    if n > 256 { return Err(format!("BOOTSTRAP_RES 联系人数量异常: {n}")); }
    let mut out = Vec::with_capacity(n);
    for _ in 0..n { out.push(c.contact()?); }
    Ok((self_id, out))
}

/// `KADEMLIA2_HELLO_REQ` 载荷：`<our id 16><tcp u16><version u8><tags>`。
/// 按 aMule `SendMyDetails` / `AddContact2`：`TAG_SOURCEUPORT` 是必带的内网 UDP 端口；
/// `TAG_KADMISCOPTIONS` 的 bit2 表示请求 `HELLO_RES_ACK` 三次握手。
pub fn hello_req(our_id: &[u8; ID_LEN], tcp_port: u16, udp_port: u16,
                 misc_options: u8) -> Vec<u8> {
    let mut p = Vec::with_capacity(16 + 2 + 1 + 1 + 6);
    p.extend_from_slice(our_id);
    p.extend_from_slice(&tcp_port.to_le_bytes());
    p.push(KAD_VERSION);
    let with_misc = misc_options != 0;
    p.push(if with_misc { 2 } else { 1 });
    // KAD tag framing: type u8 | name(u16 length + bytes) | value。
    p.extend_from_slice(&[KAD_TAG_U16, 1, 0, TAG_SOURCEUPORT]);
    p.extend_from_slice(&udp_port.to_le_bytes());
    if with_misc {
        p.extend_from_slice(&[KAD_TAG_U8, 1, 0, TAG_KADMISCOPTIONS, misc_options]);
    }
    frame(OP_HELLO_REQ, &p)
}

/// `KADEMLIA2_HELLO_RES_ACK` 载荷：`<our id 16><tag count 0>`（无 tag）。
pub fn hello_res_ack(our_id: &[u8; ID_LEN]) -> Vec<u8> {
    let mut p = Vec::with_capacity(ID_LEN + 1);
    p.extend_from_slice(our_id);
    p.push(0);
    frame(OP_HELLO_RES_ACK, &p)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelloRes {
    pub id: [u8; ID_LEN],
    pub tcp: u16,
    pub version: u8,
    pub source_uport: Option<u16>,
    pub misc_options: u8,
}

/// 解析 `HELLO_RES`（与 `HELLO_REQ` 同布局）。只接受 aMule KAD ReadTag 能读的
/// U8/U16/U32/U64；未知类型直接报错，避免把非 KAD 包当成握手成功。
pub fn parse_hello_res(payload: &[u8]) -> Result<HelloRes, String> {
    let mut c = Cur { b: payload, i: 0 };
    let id = c.id()?;
    let tcp = c.u16()?;
    let version = c.u8()?;
    let tag_count = c.u8()? as usize;
    if tag_count > 32 {
        return Err(format!("HELLO_RES tag 数异常: {tag_count}"));
    }
    let mut out = HelloRes { id, tcp, version, source_uport: None, misc_options: 0 };
    for _ in 0..tag_count {
        let ty = c.u8()?;
        let name_len = c.u16()? as usize;
        if name_len == 0 || name_len > 32 {
            return Err(format!("HELLO_RES tag 名长度异常: {name_len}"));
        }
        let name = c.take(name_len)?[0];
        match ty {
            KAD_TAG_U8 => {
                let v = c.u8()?;
                if name == TAG_KADMISCOPTIONS { out.misc_options = v; }
            }
            KAD_TAG_U16 => {
                let v = c.u16()?;
                if name == TAG_SOURCEUPORT { out.source_uport = Some(v); }
            }
            0x03 => { let _ = c.u32()?; }
            0x0b => { let _ = c.take(8)?; }
            other => return Err(format!("HELLO_RES 不支持的 KAD tag 类型 0x{other:02x}")),
        }
    }
    Ok(out)
}

/// `KADEMLIA2_REQ` payload: `<wanted 1><target 16><receiver 16>`。
/// `receiver` 是被问节点自身的 KadID（aMule 校验不符就静默丢弃）。
pub fn find_node_req(target: &[u8; ID_LEN], receiver: &[u8; ID_LEN], wanted: u8) -> Vec<u8> {
    let mut p = Vec::with_capacity(33);
    p.push(wanted & 0x1f);
    p.extend_from_slice(target);
    p.extend_from_slice(receiver);
    frame(OP_REQ, &p)
}

/// `KADEMLIA2_RES` payload: `<target 16><count 1>[contact]*`。
pub fn parse_find_node_res(payload: &[u8]) -> Result<([u8; ID_LEN], Vec<Contact>), String> {
    let mut c = Cur { b: payload, i: 0 };
    let target = c.id()?;
    let n = c.u8()? as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n { out.push(c.contact()?); }
    Ok((target, out))
}

/// `CUInt128::Get32BitChunk(0)` = XOR 距离的最高 32 位。aMule 用它判责任区。
pub fn top_chunk(a: &[u8; ID_LEN], b: &[u8; ID_LEN]) -> u32 {
    let mut d = [0u8; ID_LEN];
    for i in 0..ID_LEN { d[i] = a[i] ^ b[i]; }
    u32::from_be_bytes([d[0], d[1], d[2], d[3]])
}

pub fn in_tolerance_zone(node: &[u8; ID_LEN], key: &[u8; ID_LEN]) -> bool {
    top_chunk(node, key) <= SEARCHTOLERANCE
}

/// XOR 全距离排序（责任区内再按它挑最近节点）。
pub fn xor_distance(a: &[u8; ID_LEN], b: &[u8; ID_LEN]) -> u128 {
    let mut d = [0u8; ID_LEN];
    for i in 0..ID_LEN { d[i] = a[i] ^ b[i]; }
    u128::from_be_bytes(d)
}

/// `PUBLISH_KEY_REQ` 载荷：`<key 16><count u16>[<source_id 16><tags>]`。
/// tag 必须是 aMule KAD `ReadTag` 可接受的形式：`type u8 | name(u16 len + bytes) | value`。
/// 数值名就是长度 1 的字符串（例如 `TAG_FILENAME = b"\\x01"`），不是 ed2k 的 `[type|0x80]`。
pub fn publish_key_req(key: &[u8; ID_LEN], source_id: &[u8; ID_LEN],
                       filename: &str, size: u32) -> Vec<u8> {
    let mut p = Vec::with_capacity(64 + filename.len());
    p.extend_from_slice(key);
    p.extend_from_slice(&1u16.to_le_bytes()); // 最小样本：一个条目
    p.extend_from_slice(source_id);
    p.push(2); // tag 数：filename + filesize
    let name = filename.as_bytes();
    p.extend_from_slice(&[0x02, 0x01, 0x00]); // TAGTYPE_STRING + name len 1
    p.push(0x01); // name = TAG_FILENAME
    p.extend_from_slice(&(name.len() as u16).to_le_bytes());
    p.extend_from_slice(name);
    p.extend_from_slice(&[0x03, 0x01, 0x00, 0x02]); // TAGTYPE_U32 + name = TAG_FILESIZE
    p.extend_from_slice(&size.to_le_bytes());
    frame(OP_PUBLISH_KEY_REQ, &p)
}

/// `PUBLISH_RES` 载荷：`<key 16><load u8>`。只证明对端收下 key，
/// 不证明公网可检索——检索仍须真实 `SEARCH_RES`。
pub fn parse_publish_res(payload: &[u8]) -> Result<([u8; ID_LEN], u8), String> {
    if payload.len() < ID_LEN + 1 {
        return Err(format!("PUBLISH_RES 太短: {} B", payload.len()));
    }
    let mut key = [0u8; ID_LEN];
    key.copy_from_slice(&payload[..ID_LEN]);
    Ok((key, payload[ID_LEN]))
}

/// `SEARCH_KEY_REQ` 载荷：`<target 16><startPosition 2>`；startPosition=0（bit0x8000 是限制搜索表达式）。
pub fn search_key_req(key: &[u8; ID_LEN]) -> Vec<u8> {
    let mut p = Vec::with_capacity(18);
    p.extend_from_slice(key);
    p.extend_from_slice(&0u16.to_le_bytes());
    frame(OP_SEARCH_KEY_REQ, &p)
}

/// 一条关键词检索结果：只取 source_id（即 fileID），tags 留给调用方按 ADR-011 §tag 规则解析。
#[derive(Debug, Clone, PartialEq)]
pub struct KeywordAnswer {
    pub source_id: [u8; ID_LEN],
    pub tags_bytes: Vec<u8>,
}

/// 解析 `SEARCH_RES` 载荷：`<sender 16><target 16><count 2>[<answer 16><tags>]*`。
///
/// 这里只做**结构边界**解析：tags 的字节数无法从枚举头直接推知，所以返回
/// `tags_bytes = 之后到本条结束的原始切片` 由上层按 tag 类型逐条消费；为避免
/// 越界，调用方应传「一条 answer 的已知切片」。完整多 answer 解析须配合 tag
/// 解析器（S2 `read_tags`），本模块不伪装成已实现。
pub fn parse_search_key_answers(payload: &[u8]) -> Result<Vec<KeywordAnswer>, String> {
    if payload.len() < 34 {
        return Err(format!("SEARCH_RES 太短: {}", payload.len()));
    }
    let count = u16::from_le_bytes([payload[32], payload[33]]) as usize;
    let mut out = Vec::with_capacity(count);
    let mut pos = 34usize;
    for _ in 0..count {
        if pos + ID_LEN > payload.len() {
            return Err("SEARCH_RES answer 截断".into());
        }
        let mut id = [0u8; ID_LEN];
        id.copy_from_slice(&payload[pos..pos + ID_LEN]);
        pos += ID_LEN;
        // 没有 tag 长度前缀：只能整包交给上层。这里把剩余全给第一条，
        // 多 answer 的包由支持 tag 解析的调用方处理（见 README 边界）。
        out.push(KeywordAnswer { source_id: id, tags_bytes: payload[pos..].to_vec() });
        pos = payload.len();
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ed2k_keyword_key_is_unnamespaced_md4_known_answer() {
        // MD4("abc") = a448017aaf21d8525fc10ae87aa6729d（标准已知答案）
        let k = ed2k_keyword_key("abc");
        assert_eq!(cr_hash::hex(&k), "a448017aaf21d8525fc10ae87aa6729d");
        assert_ne!(ed2k_keyword_key("ABC"), k, "大小写敏感，不做归一化");
        assert_eq!(ed2k_keyword_key("冷"), kad_keyword_hash("冷"), "UTF-8 原样哈希");
    }

    #[test]
    fn search_key_req_is_byte_exact() {
        let key = ed2k_keyword_key("abc");
        let frame = search_key_req(&key);
        assert_eq!(frame[0], PROTO_KAD);
        assert_eq!(frame[1], OP_SEARCH_KEY_REQ);
        assert_eq!(&frame[2..18], &key[..]);
        assert_eq!(&frame[18..20], &0u16.to_le_bytes());
        assert_eq!(frame.len(), 20);
    }

    #[test]
    fn parse_search_key_answers_reads_source_id() {
        let mut payload = vec![0u8; 32]; // sender + target
        payload.extend_from_slice(&1u16.to_le_bytes()); // count=1
        payload.extend_from_slice(&[0xab; ID_LEN]); // answer source_id
        payload.extend_from_slice(b"tagbytes"); // 原始 tag 字节
        let got = parse_search_key_answers(&payload).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].source_id, [0xab; ID_LEN]);
        assert_eq!(got[0].tags_bytes, b"tagbytes");
        assert!(parse_search_key_answers(&payload[..10]).is_err());
    }

    #[test]
    fn contact_decoding_reverses_wire_ip_order() {
        // id(16) | ip wire 72 e3 74 5b (= reversed 91.116.227.114) | udp | tcp | ver
        let mut p = vec![0x11u8; ID_LEN];
        p.extend_from_slice(&[0x72, 0xe3, 0x74, 0x5b]);
        p.extend_from_slice(&4672u16.to_le_bytes());
        p.extend_from_slice(&4672u16.to_le_bytes());
        p.push(8);
        let mut c = Cur { b: &p, i: 0 };
        let contact = c.contact().unwrap();
        assert_eq!(contact.ip, [91, 116, 227, 114]);
        assert_eq!(contact.addr().to_string(), "91.116.227.114:4672");
        assert!(contact.is_public());
    }

    #[test]
    fn find_node_req_and_zone_check_are_byte_exact() {
        let target = [0x22u8; ID_LEN];
        let receiver = [0x33u8; ID_LEN];
        let wire = find_node_req(&target, &receiver, 5);
        assert_eq!(wire[0], PROTO_KAD);
        assert_eq!(wire[1], OP_REQ);
        assert_eq!(&wire[2], &5);
        assert_eq!(&wire[3..19], &target[..]);
        assert_eq!(&wire[19..35], &receiver[..]);
        assert_eq!(wire.len(), 35);
        // 同 id 距离为 0；只差最高字节 1 → 0x01000000，刚在 2^24 边界内。
        let mut near = target;
        near[0] ^= 1;
        assert_eq!(top_chunk(&target, &near), 0x0100_0000);
        assert!(in_tolerance_zone(&target, &near));
        near[0] ^= 0x11;
        assert!(!in_tolerance_zone(&target, &near));
    }

    #[test]
    fn publish_key_req_matches_the_amule_accepted_wire_bytes() {
        // S2 2026-11-20 LAN 实测 PUBLISH_RES 对应的请求帧（逐字节来自报告）。
        let expected = "e443ad50ee0f06284f630cdf3950a06da42e010028654c6abc132c91cdcd855084db8e7c02020100012700636f6c647265766976652d6564326b2d70726f62652d313739353138313838333335392e747874030100023f000000";
        let key = id_bytes("ad50ee0f06284f630cdf3950a06da42e");
        let source = id_bytes("28654c6abc132c91cdcd855084db8e7c");
        let got = publish_key_req(
            &key, &source,
            "coldrevive-ed2k-probe-1795181883359.txt", 63,
        );
        assert_eq!(hex_bytes(expected), got);
        let (echo, load) = parse_publish_res(&hex_bytes("ad50ee0f06284f630cdf3950a06da42e01")).unwrap();
        assert_eq!(echo, key);
        assert_eq!(load, 1);
    }

    #[test]
    fn hello_req_and_res_ack_match_amule_layout() {
        let id = id_bytes("00112233445566778899aabbccddeeff");
        let wire = hello_req(&id, 4662, 4672, KAD_MISCOPT_REQUEST_ACK);
        assert_eq!(wire[0], PROTO_KAD);
        assert_eq!(wire[1], OP_HELLO_REQ);
        assert_eq!(&wire[2..18], &id[..]);
        assert_eq!(&wire[18..20], &4662u16.to_le_bytes());
        assert_eq!(wire[20], KAD_VERSION);
        assert_eq!(wire[21], 2);
        assert_eq!(&wire[22..28], &[KAD_TAG_U16, 1, 0, TAG_SOURCEUPORT, 0x40, 0x12]);
        assert_eq!(&wire[28..], &[KAD_TAG_U8, 1, 0, TAG_KADMISCOPTIONS, KAD_MISCOPT_REQUEST_ACK]);
        let ack = hello_res_ack(&id);
        assert_eq!(ack[1], OP_HELLO_RES_ACK);
        assert_eq!(&ack[2..18], &id[..]);
        assert_eq!(ack[18], 0);
    }

    #[test]
    fn parse_hello_res_reads_source_port_and_misc_options() {
        let mut p = vec![0x44u8; ID_LEN];
        p.extend_from_slice(&4712u16.to_le_bytes());
        p.push(KAD_VERSION);
        p.push(2);
        p.extend_from_slice(&[KAD_TAG_U16, 1, 0, TAG_SOURCEUPORT]);
        p.extend_from_slice(&4672u16.to_le_bytes());
        p.extend_from_slice(&[KAD_TAG_U8, 1, 0, TAG_KADMISCOPTIONS, KAD_MISCOPT_REQUEST_ACK]);
        let h = parse_hello_res(&p).unwrap();
        assert_eq!(h.id, [0x44; ID_LEN]);
        assert_eq!(h.tcp, 4712);
        assert_eq!(h.source_uport, Some(4672));
        assert_eq!(h.misc_options, KAD_MISCOPT_REQUEST_ACK);
        assert!(parse_hello_res(&p[..10]).is_err());
    }

    #[test]
    fn unpack_frame_decodes_packed_kad_reply() {
        let mut wire = vec![PROTO_KAD_PACKED, OP_RES];
        wire.extend_from_slice(&hex_bytes("789c4bcecf49294a2dcb2c4bd5cb4e4cd12b484cce4e4dd14b2b4acc4dd52b492d2e0100ccf30c4f"));
        let (op, payload) = unpack_frame(&wire).unwrap();
        assert_eq!(op, OP_RES);
        assert_eq!(payload, b"coldrevive.kad.packed.frame.test");
        assert!(unpack_frame(&[0x42, 0x09]).is_err());
    }

    fn hex_bytes(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn id_bytes(s: &str) -> [u8; ID_LEN] {
        let v = hex_bytes(s);
        let mut out = [0u8; ID_LEN];
        out.copy_from_slice(&v);
        out
    }
}
