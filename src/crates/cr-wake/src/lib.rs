//! cr-wake — W4.1 私有唤醒协议（PROTO §3，PROTOCOL_VERSION = 1）。
//!
//! 报文为 UDP 单包 JSON；`.torrent` 本体走 HTTP，UDP 只回 URL 与摘要。
//! 字段集合与 T0 golden 一一对应；任何字段增删都必须递增 PROTOCOL_VERSION 并更新 golden。

use cr_core::{nonce_hex, trace_id, unix_ts, Json, CHALLENGE_WINDOW_SEC, CLIENT_VERSION, PROTOCOL_VERSION};
use cr_hash::hmac_sha256_hex;
use std::collections::HashSet;

pub const REQ_FIELDS: [&str; 10] = [
    "v", "type", "trace_id", "file_hash", "infohash", "nonce", "ts",
    "challenge", "client_version", "capabilities",
];
pub const ACK_FIELDS: [&str; 10] = [
    "v", "type", "trace_id", "status", "magnet", "torrent_url",
    "file_size", "piece_size", "ttl", "bt_port",
];

pub fn challenge(secret: &[u8], file_hash: &str, ts: i64, nonce: &str) -> String {
    hmac_sha256_hex(secret, format!("{}|{}|{}", file_hash, ts / 86400, nonce).as_bytes())
}

pub fn build_req(secret: &[u8], file_hash: &str, infohash: &str) -> Json {
    let nonce = nonce_hex();
    let ts = unix_ts();
    Json::obj(vec![
        ("v", Json::n(PROTOCOL_VERSION as f64)),
        ("type", Json::s("WAKEUP_REQ")),
        ("trace_id", Json::s(trace_id())),
        ("file_hash", Json::s(file_hash)),
        ("infohash", Json::s(infohash)),
        ("nonce", Json::s(&nonce)),
        ("ts", Json::n(ts as f64)),
        ("challenge", Json::s(challenge(secret, file_hash, ts, &nonce))),
        ("client_version", Json::s(CLIENT_VERSION)),
        ("capabilities", Json::obj(vec![
            ("bt", Json::b(true)), ("utp", Json::b(true)), ("ed2k", Json::b(false)),
        ])),
    ])
}

pub fn build_ack(trace: &str, infohash: &str, torrent_url: &str,
                 file_size: usize, piece_size: usize, bt_port: u16) -> Json {
    let magnet = format!("magnet:?xt=urn:btih:{}&xs={}", infohash, urlencode(torrent_url));
    Json::obj(vec![
        ("v", Json::n(PROTOCOL_VERSION as f64)),
        ("type", Json::s("WAKEUP_ACK")),
        ("trace_id", Json::s(trace)),
        ("status", Json::s("OK")),
        ("magnet", Json::s(&magnet)),
        ("torrent_url", Json::s(torrent_url)),
        ("file_size", Json::n(file_size as f64)),
        ("piece_size", Json::n(piece_size as f64)),
        ("ttl", Json::n(cr_core::TTL_SEC as f64)),
        ("bt_port", Json::n(bt_port as f64)),
    ])
}

pub fn reject(trace: &str, code: &str) -> Json {
    Json::obj(vec![
        ("v", Json::n(PROTOCOL_VERSION as f64)),
        ("type", Json::s("WAKEUP_ACK")),
        ("trace_id", Json::s(trace)),
        ("status", Json::s(code)),
    ])
}

/// 接收方校验：版本 → 时间窗 → Challenge → nonce 去重 → 字段集合。
/// 返回 Err(PROTO §3.4 错误码)。任何失败都不允许激活做种。
pub fn validate_req(req: &Json, secret: &[u8], now: i64,
                    seen: &mut HashSet<String>) -> Result<(), &'static str> {
    if req.get("v").and_then(Json::as_i64) != Some(PROTOCOL_VERSION as i64) {
        return Err("REJECT_VERSION");
    }
    if !has_exact_fields(req, &REQ_FIELDS) { return Err("REJECT_BAD_CHALLENGE"); }
    if req.get("type").and_then(Json::as_str) != Some("WAKEUP_REQ") {
        return Err("REJECT_BAD_CHALLENGE");
    }
    let ts = req.get("ts").and_then(Json::as_i64).unwrap_or(0);
    if (now - ts).abs() > CHALLENGE_WINDOW_SEC { return Err("REJECT_BAD_CHALLENGE"); }
    let file_hash = req.get("file_hash").and_then(Json::as_str).unwrap_or("");
    let nonce = req.get("nonce").and_then(Json::as_str).unwrap_or("");
    let want = challenge(secret, file_hash, ts, nonce);
    if req.get("challenge").and_then(Json::as_str) != Some(want.as_str()) {
        return Err("REJECT_BAD_CHALLENGE");
    }
    if !seen.insert(nonce.to_string()) { return Err("REJECT_BAD_CHALLENGE"); }
    Ok(())
}

pub fn has_exact_fields(j: &Json, expected: &[&str]) -> bool {
    let mut got = j.keys();
    got.sort();
    let mut want: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
    want.sort();
    got == want
}

pub fn ack_is_ok(ack: &Json) -> bool {
    ack.get("status").and_then(Json::as_str) == Some("OK")
        && has_exact_fields(ack, &ACK_FIELDS)
}

pub fn ack_infohash(ack: &Json) -> Option<String> {
    let magnet = ack.get("magnet").and_then(Json::as_str)?;
    let rest = magnet.strip_prefix("magnet:?xt=urn:btih:")?;
    let h = rest.split('&').next().unwrap_or("");
    if h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit()) { Some(h.to_string()) }
    else { None }
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else { out.push_str(&format!("%{:02X}", b)); }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(secret: &[u8]) -> Json { build_req(secret, "hash", "infohash") }

    #[test]
    fn golden_field_sets_are_exact() {
        let mut seen = HashSet::new();
        let r = req(b"seed");
        assert_eq!(r.keys().len(), 10);
        assert!(validate_req(&r, b"seed", unix_ts(), &mut seen).is_ok());
        let a = build_ack("t", "0123456789abcdef0123456789abcdef01234567",
                          "http://127.0.0.1:16800/torrent/x", 262144, 262144, 16802);
        assert_eq!(a.keys().len(), 10);
        assert!(ack_is_ok(&a));
        assert_eq!(ack_infohash(&a).unwrap(), "0123456789abcdef0123456789abcdef01234567");
    }

    #[test]
    fn rejects_bad_version_challenge_window_and_replay() {
        let mut seen = HashSet::new();
        let now = unix_ts();
        let mut bad = req(b"seed");
        // 篡改 challenge
        let fields = match &mut bad { Json::Obj(p) => p, _ => unreachable!() };
        for (k, v) in fields.iter_mut() {
            if k == "challenge" { *v = Json::s("deadbeef"); }
            if k == "v" { *v = Json::n(2.0); }
        }
        assert_eq!(validate_req(&bad, b"seed", now, &mut seen), Err("REJECT_VERSION"));
        let r = req(b"seed");
        assert_eq!(validate_req(&r, b"seed", now + 3600, &mut seen),
                   Err("REJECT_BAD_CHALLENGE"));
        let r2 = req(b"seed");
        assert!(validate_req(&r2, b"seed", now, &mut seen).is_ok());
        assert_eq!(validate_req(&r2, b"seed", now, &mut seen),
                   Err("REJECT_BAD_CHALLENGE"));
    }
}
