//! cr-kad — W3.1 KAD 适配层（M1 本地双节点实现）。
//!
//! 边界（T0 红线）：本 crate 只跑 `127.0.0.1` 私有子网，**不向公共 KAD 发布任何测试 Key**。
//! 公共 KAD 的 `0xE4` wire 映射已在 S2 spike 验证（ADR-001 = public），M3 再接入生产网络。
//!
//! 双栈 ID 宽度（S3 的结构性证据）：KAD 用 128-bit KadID，Mainline DHT 用 160-bit ID。
//! 两者同进程共存，命名空间/宽度不同，禁止互相误投；T0 的 `inspect kad` 会同时暴露两者。

use cr_core::{Json, NAMESPACE};
use cr_hash::{sha1_hex, sha256_hex};
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
