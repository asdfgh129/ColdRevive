//! eMule KAD 兼容轨（ADR-011 L1）的最小编解码，从 S2 `kad-mini/src/wire.rs` 逐字节搬来。
//!
//! 边界（照写）：这里只做**字节层**，不发包。生产 daemon 仍只跑 127.0.0.1 私有子网，
//! 不向公共 KAD 发布/检索任何测试 Key（cr-kad 的 T0 红线）。
//! 公共关键词检索的可达性另由 G0 `protocol_scope` 与 OPEN-QUESTIONS §1 约束，
//! 本模块的存在**不代表** FR-24 的公网检索可用——那只到「离线可验证的兼容原语」。
//!
//! A 轨 Key = `MD4(UTF-8(term))`，**无前缀**，与 cr-kad 的私有 B 轨
//! `SHA1(namespace+kw:term)` 是两套算法，不互换（ADR-011 §5）。

use cr_hash::kad_keyword_hash;

/// KAD UDP 帧魔数（aMule `PROTO_KAD = 0xE4`）。
pub const PROTO_KAD: u8 = 0xE4;
pub const OP_SEARCH_KEY_REQ: u8 = 0x33;
pub const OP_SEARCH_RES: u8 = 0x3B;
pub const ID_LEN: usize = 16;

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
}
