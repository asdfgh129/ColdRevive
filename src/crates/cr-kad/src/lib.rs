//! cr-kad — W3.1 KAD 适配层（M1 本地双节点实现）。
//!
//! 边界（T0 红线）：本 crate 只跑 `127.0.0.1` 私有子网，**不向公共 KAD 发布任何测试 Key**。
//! 公共 KAD 的 `0xE4` wire 映射已在 S2 spike 验证（ADR-001 = public），M3 再接入生产网络。
//!
//! 双栈 ID 宽度（S3 的结构性证据）：KAD 用 128-bit KadID，Mainline DHT 用 160-bit ID。
//! 两者同进程共存，命名空间/宽度不同，禁止互相误投；T0 的 `inspect kad` 会同时暴露两者。

use cr_core::{Json, NAMESPACE};
use cr_hash::{sha1_hex, sha256_hex};

/// ADR-011 eMule 兼容轨的字节层原语（无前缀 MD4 Key + 0x33/0x3B 编解码）。
/// 只做离线可验证的编解码，不代表公网检索可用。
pub mod kad_ed2k;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::Duration;

/// T0 harness 的 Target_Key 口径：`SHA1(namespace || file_hash)` 的十六进制。
/// 生产公共 KAD 使用 128-bit KadID（见 `kad_id_128`），两者在 M1 显式区分，不混用。
pub fn target_key(namespace: &str, file_hash: &str) -> String {
    sha1_hex(format!("{}{}", namespace, file_hash).as_bytes())
}
pub fn keyword_key(namespace: &str, keyword: &str) -> String {
    sha1_hex(format!("{}kw:{}", namespace, keyword).as_bytes())
}
/// eMule KAD 的 128-bit ID（SHA-256 前 16 字节）。
pub fn kad_id_128(namespace: &str, seed: &str) -> String {
    let h = sha256_hex(format!("{}{}", namespace, seed).as_bytes());
    h[..32].to_string()
}
/// Mainline DHT 的 160-bit ID（SHA-1 / 20 字节）。
pub fn dht_id_160(namespace: &str, seed: &str) -> String {
    sha1_hex(format!("{}{}", namespace, seed).as_bytes())
}

pub fn publish_keys(namespace: &str, file_hash: &str, keywords: &[String]) -> Vec<String> {
    let mut keys = vec![target_key(namespace, file_hash)];
    for kw in keywords.iter().take(8) {
        keys.push(keyword_key(namespace, kw));
    }
    keys
}

/// 文件名里常见的「容器/编码/来源」噪声词。它们在任何检索里都命中一片、
/// 却不指向任何具体内容，发上去只会稀释关键词质量。
pub const NOISE_TOKENS: &[&str] = &[
    "mkv", "mp4", "avi", "wmv", "flv", "mov", "mpg", "mpeg", "iso", "rar", "zip", "7z",
    "x264", "x265", "h264", "h265", "hevc", "avc", "aac", "ac3", "dts", "flac", "mp3",
    "1080p", "720p", "480p", "2160p", "4k", "webrip", "webdl", "bluray", "bdrip", "hdtv",
    "remux", "repack", "proper", "internal", "hdr", "sdr", "yify", "rarbg",
];

/// FR-07：把文件名切成 KAD 关键词。
///
/// 规则刻意保守，且**只做确定性变换**（同一个名字永远切出同一组词，便于复算）：
/// - 只在「非字母数字」处切分；CJK 连续串会成为一个词——中文没有空格，
///   按「每字一词」切会让一个四字标题变成四条互不相干的 key；
/// - ASCII 转小写；纯数字丢掉（`2024`、`01` 这类不带内容信息）；
/// - ASCII 词短于 3 个字符丢掉，CJK 词短于 2 个字符丢掉；
/// - 命中 [`NOISE_TOKENS`] 的丢掉；
/// - 去重保持首次出现顺序，最多 `limit` 个（与 [`publish_keys`] 的 8 个上限对齐）。
///
/// 不做的：不分词性、不做同义词扩展、不做语言识别。这些都会引入不可复算的
/// 结果，而 KAD 关键词一旦错了就是「发出去也查不到」，比少发几个更糟。
pub fn tokenize_keywords(name: &str, limit: usize) -> Vec<String> {
    fn keep(tok: &str) -> bool {
        if tok.is_empty() {
            return false;
        }
        if tok.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        let ascii_only = tok.is_ascii();
        if ascii_only && tok.len() < 3 {
            return false;
        }
        if !ascii_only && tok.chars().count() < 2 {
            return false;
        }
        if ascii_only && NOISE_TOKENS.contains(&tok) {
            return false;
        }
        true
    }

    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let push = |cur: &mut String, out: &mut Vec<String>| {
        if cur.is_empty() {
            return;
        }
        let low = cur.to_lowercase();
        cur.clear();
        if keep(&low) && !out.iter().any(|x| x == &low) && out.len() < limit {
            out.push(low);
        }
    };
    for ch in name.chars() {
        if ch.is_alphanumeric() {
            cur.push(ch);
        } else {
            push(&mut cur, &mut out);
        }
    }
    push(&mut cur, &mut out);
    out
}

#[cfg(test)]
mod tokenizer_tests {
    use super::*;

    #[test]
    fn tokenizer_is_deterministic_and_lowercases() {
        let a = tokenize_keywords("Cold.Revive_Sample-2024.MKV", 8);
        let b = tokenize_keywords("Cold.Revive_Sample-2024.MKV", 8);
        assert_eq!(a, b, "同一名字必须切出同一组词");
        assert_eq!(a, vec!["cold", "revive", "sample"]);
    }

    #[test]
    fn tokenizer_drops_noise_digits_and_short_tokens() {
        let t = tokenize_keywords("Movie.1080p.x265.AAC-GROUP.2024.mkv", 8);
        assert!(!t.iter().any(|x| x == "mkv"), "容器名不该成为关键词: {t:?}");
        assert!(!t.iter().any(|x| x == "2024"), "纯数字不该成为关键词: {t:?}");
        assert!(!t.iter().any(|x| x == "hd"), "两字母 ASCII 词要丢掉: {t:?}");
        assert!(t.contains(&"movie".to_string()));
        assert!(t.contains(&"group".to_string()));
    }

    #[test]
    fn tokenizer_keeps_cjk_runs_whole() {
        let t = tokenize_keywords("冷启动复活-测试样本.bin", 8);
        assert!(t.contains(&"冷启动复活".to_string()), "CJK 连续串应整体保留: {t:?}");
        assert!(t.contains(&"测试样本".to_string()), "CJK 连续串应整体保留: {t:?}");
    }

    #[test]
    fn tokenizer_dedupes_and_honours_limit() {
        let t = tokenize_keywords("alpha beta alpha gamma delta epsilon zeta eta theta", 4);
        assert_eq!(t.len(), 4, "必须尊重上限");
        assert_eq!(t.iter().filter(|x| *x == "alpha").count(), 1, "必须去重");
        assert_eq!(t[0], "alpha", "去重应保留首次出现顺序");
    }

    #[test]
    fn tokenizer_handles_degenerate_input() {
        assert!(tokenize_keywords("", 8).is_empty());
        assert!(tokenize_keywords("---___...", 8).is_empty());
        assert!(tokenize_keywords("ab cd", 8).is_empty(), "全是短词时应为空");
    }
}

pub fn find_frame(key: &str) -> Json {
    Json::obj(vec![("t", Json::s("KAD_FIND")), ("key", Json::s(key))])
}
pub fn find_result(hit_node: &str, count: usize) -> Json {
    Json::obj(vec![
        ("t", Json::s("KAD_FIND_RES")),
        ("hit_node", Json::s(hit_node)),
        ("count", Json::n(count as f64)),
    ])
}

/// 单包 UDP 请求/响应。超时返回 None，由调用方决定超时/换候选，**绝不合成响应**。
pub fn udp_roundtrip(bind: &str, peer: SocketAddr, frame: &Json,
                     timeout_ms: u64) -> Result<(Json, u64), String> {
    let socket = UdpSocket::bind(format!("{}:0", bind))
        .map_err(|e| format!("UDP bind 失败: {}", e))?;
    socket.set_read_timeout(Some(Duration::from_millis(timeout_ms)))
        .map_err(|e| e.to_string())?;
    let payload = frame.dump();
    let start = std::time::Instant::now();
    socket.send_to(payload.as_bytes(), peer)
        .map_err(|e| format!("UDP send 失败: {}", e))?;
    let mut buf = [0u8; 65535];
    let (n, _) = socket.recv_from(&mut buf)
        .map_err(|e| format!("UDP 超时或失败: {}", e))?;
    let elapsed = start.elapsed().as_millis() as u64;
    let text = String::from_utf8_lossy(&buf[..n]);
    let reply = Json::parse(&text).map_err(|e| format!("非法响应 JSON: {}", e))?;
    Ok((reply, elapsed))
}

pub fn udp_reply(socket: &UdpSocket, target: SocketAddr, frame: &Json) {
    let _ = socket.send_to(frame.dump().as_bytes(), target);
}

pub fn socket_addr(ip: &str, port: u16) -> Result<SocketAddr, String> {
    let parsed: IpAddr = ip.parse().map_err(|e| format!("非法 IP {}: {}", ip, e))?;
    Ok(SocketAddr::new(parsed, port))
}

pub const DEFAULT_NAMESPACE: &str = NAMESPACE;
pub const MAX_KEYWORD_KEYS: usize = 8;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_match_t0_fixture_derivation() {
        // 与 tests/e2e/t0/t0_harness.py 的 hashlib.sha1(ns+file_hash) 一致
        let k = target_key("ColdRevive_V1:", "abc");
        assert_eq!(k.len(), 40);
        assert_eq!(k, sha1_hex(b"ColdRevive_V1:abc"));
        let kw = keyword_key("ColdRevive_V1:", "coldrevive");
        assert_eq!(kw, sha1_hex(b"ColdRevive_V1:kw:coldrevive"));
    }

    #[test]
    fn dual_stack_ids_have_distinct_widths() {
        assert_eq!(kad_id_128("ColdRevive_V1:", "x").len(), 32); // 128 bit
        assert_eq!(dht_id_160("ColdRevive_V1:", "x").len(), 40); // 160 bit
        assert_ne!(kad_id_128("ColdRevive_V1:", "x"), dht_id_160("ColdRevive_V1:", "x"));
    }

    #[test]
    fn keyword_cap_is_enforced() {
        let kws: Vec<String> = (0..20).map(|i| format!("k{}", i)).collect();
        assert_eq!(publish_keys("ColdRevive_V1:", "h", &kws).len(), 1 + MAX_KEYWORD_KEYS);
    }
}
