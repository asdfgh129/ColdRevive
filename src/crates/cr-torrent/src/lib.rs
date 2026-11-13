//! cr-torrent — 伴随种子（W2.3）。
//!
//! 生成的 `.torrent` 是**真实** bencode 文件：info 字典原始字节的 SHA-1 即 InfoHash，
//! 由 T0 的 `t0_cli.torrent_infohash()` 从本机文件独立复算，产品不能自证。
//! 数据面仍走标准 BT，私有协议不进入种子或分片。

use cr_hash::{hex, sha1, sha1_hex};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Torrent {
    pub raw: Vec<u8>,
    pub info_hash: String,
    pub name: String,
    pub length: usize,
    pub piece_size: usize,
    pub pieces: Vec<[u8; 20]>,
}

fn bstr(s: &[u8]) -> Vec<u8> { [format!("{}:", s.len()).as_bytes(), s].concat() }
fn bint(n: usize) -> Vec<u8> { format!("i{}e", n).into_bytes() }

/// 按 BT 规范生成单文件种子。字典键必须按字典序排列（announce < info）。
pub fn generate(data: &[u8], name: &str, piece_size: usize, announce: &str) -> Torrent {
    assert!(piece_size > 0, "piece_size 必须 > 0");
    let pieces: Vec<[u8; 20]> = data.chunks(piece_size).map(sha1).collect();

    let mut pieces_blob = Vec::with_capacity(pieces.len() * 20);
    for p in &pieces { pieces_blob.extend_from_slice(p); }

    let mut info = Vec::new();
    info.push(b'd');
    info.extend_from_slice(&bstr(b"length"));
    info.extend_from_slice(&bint(data.len()));
    info.extend_from_slice(&bstr(b"name"));
    info.extend_from_slice(&bstr(name.as_bytes()));
    info.extend_from_slice(&bstr(b"piece length"));
    info.extend_from_slice(&bint(piece_size));
    info.extend_from_slice(&bstr(b"pieces"));
    info.extend_from_slice(&bstr(&pieces_blob));
    info.push(b'e');

    let info_hash = sha1_hex(&info);
    let mut raw = Vec::new();
    raw.push(b'd');
    raw.extend_from_slice(&bstr(b"announce"));
    raw.extend_from_slice(&bstr(announce.as_bytes()));
    raw.extend_from_slice(&bstr(b"info"));
    raw.extend_from_slice(&info);
    raw.push(b'e');

    Torrent { raw, info_hash, name: name.to_string(), length: data.len(), piece_size, pieces }
}

impl Torrent {
    pub fn write_to(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(path, &self.raw).map_err(|e| format!("{}: {}", path.display(), e))
    }
    pub fn magnet(&self, torrent_url: &str) -> String {
        format!("magnet:?xt=urn:btih:{}&xs={}", self.info_hash, urlencode(torrent_url))
    }
}

/// 从 .torrent 原始字节中定位 info 字典并复算 InfoHash（与 Python 侧同一口径）。
pub fn info_hash_from_raw(raw: &[u8]) -> Result<String, String> {
    let marker = b"4:info";
    let pos = raw.windows(marker.len()).position(|w| w == marker)
        .ok_or_else(|| "未找到 4:info 字段".to_string())?;
    let start = pos + marker.len();
    let end = skip_bencode(raw, start)?;
    Ok(sha1_hex(&raw[start..end]))
}

pub fn parse(raw: &[u8]) -> Result<Torrent, String> {
    let info_hash = info_hash_from_raw(raw)?;
    // 只解析 T0 契约需要的字段；长度/片长按构造时的确定性取值从 pieces 反推。
    let name = find_string_value(raw, b"name").unwrap_or_else(|| "payload.bin".to_string());
    let length = find_int_value(raw, b"length").unwrap_or(0);
    let piece_size = find_int_value(raw, b"piece length").unwrap_or(0);
    let blob = find_bytes_value(raw, b"pieces").unwrap_or_default();
    let pieces: Vec<[u8; 20]> = blob.chunks_exact(20).map(|c| {
        let mut p = [0u8; 20]; p.copy_from_slice(c); p
    }).collect();
    Ok(Torrent { raw: raw.to_vec(), info_hash, name, length, piece_size, pieces })
}

/// 下载完成后逐片校验：返回是否全部 SHA-1 匹配（真实校验，不采信下载器自报）。
pub fn verify_pieces(raw_torrent: &[u8], data: &[u8]) -> bool {
    let t = match parse(raw_torrent) { Ok(t) => t, Err(_) => return false };
    if t.piece_size == 0 || data.len() != t.length { return false; }
    data.chunks(t.piece_size).zip(t.pieces.iter())
        .all(|(chunk, expect)| sha1(chunk) == *expect)
}

/// 跳过一个 bencode 值，返回其结束偏移。用于精确切出 info 字典原始字节。
fn skip_bencode(b: &[u8], mut i: usize) -> Result<usize, String> {
    if i >= b.len() { return Err("bencode 提前结束".into()); }
    match b[i] {
        b'i' => {
            let end = b[i..].iter().position(|&c| c == b'e')
                .ok_or_else(|| "整数缺 e".to_string())? + i + 1;
            Ok(end)
        }
        b'l' => {
            i += 1;
            while i < b.len() && b[i] != b'e' { i = skip_bencode(b, i)?; }
            Ok((i + 1).min(b.len()))
        }
        b'd' => {
            i += 1;
            while i < b.len() && b[i] != b'e' {
                i = skip_bencode(b, i)?; // key
                i = skip_bencode(b, i)?; // value
            }
            Ok((i + 1).min(b.len()))
        }
        b'0'..=b'9' => {
            let colon = b[i..].iter().position(|&c| c == b':')
                .ok_or_else(|| "字符串缺 :".to_string())? + i;
            let n: usize = std::str::from_utf8(&b[i..colon]).map_err(|_| "非法长度")?
                .parse().map_err(|_| "非法长度")?;
            Ok(colon + 1 + n)
        }
        c => Err(format!("非法 bencode 类型 {:?}", c as char)),
    }
}

fn find_key_pos(b: &[u8], key: &[u8]) -> Option<usize> {
    let pattern = bstr(key);
    b.windows(pattern.len()).position(|w| w == pattern)
}
fn find_string_value(b: &[u8], key: &[u8]) -> Option<String> {
    let pos = find_key_pos(b, key)? + bstr(key).len();
    let colon = b[pos..].iter().position(|&c| c == b':')? + pos;
    let n: usize = std::str::from_utf8(&b[pos..colon]).ok()?.parse().ok()?;
    String::from_utf8(b.get(colon + 1..colon + 1 + n)?.to_vec()).ok()
}
fn find_bytes_value(b: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let pos = find_key_pos(b, key)? + bstr(key).len();
    let colon = b[pos..].iter().position(|&c| c == b':')? + pos;
    let n: usize = std::str::from_utf8(&b[pos..colon]).ok()?.parse().ok()?;
    Some(b.get(colon + 1..colon + 1 + n)?.to_vec())
}
fn find_int_value(b: &[u8], key: &[u8]) -> Option<usize> {
    let pos = find_key_pos(b, key)? + bstr(key).len();
    if b.get(pos) != Some(&b'i') { return None; }
    let end = b[pos..].iter().position(|&c| c == b'e')? + pos;
    std::str::from_utf8(&b[pos + 1..end]).ok()?.parse().ok()
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

/// 供 tracker 响应使用的最小 bencode 编码。
pub fn bencode_str(s: &str) -> Vec<u8> { bstr(s.as_bytes()) }
pub fn bencode_int(n: usize) -> Vec<u8> { bint(n) }
pub fn hex_of(bytes: &[u8]) -> String { hex(bytes) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infohash_is_sha1_of_raw_info_dict() {
        let data = b"coldrevive-t0-payload";
        let t = generate(data, "sample.bin", 8, "http://127.0.0.1:16800/announce");
        assert_eq!(info_hash_from_raw(&t.raw).unwrap(), t.info_hash);
        assert_eq!(t.pieces.len(), 3);
    }

    #[test]
    fn piece_verification_rejects_corruption() {
        let data = vec![7u8; 300];
        let t = generate(&data, "sample.bin", 64, "http://127.0.0.1:1/announce");
        assert!(verify_pieces(&t.raw, &data));
        let mut bad = data.clone();
        bad[70] ^= 0xff;
        assert!(!verify_pieces(&t.raw, &bad));
    }

    #[test]
    fn parse_recovers_metadata() {
        let data = vec![1u8; 1000];
        let t = generate(&data, "coldrevive-t0-sample.bin", 256, "http://127.0.0.1:16800/announce");
        let p = parse(&t.raw).unwrap();
        assert_eq!(p.info_hash, t.info_hash);
        assert_eq!(p.length, 1000);
        assert_eq!(p.piece_size, 256);
        assert_eq!(p.pieces.len(), 4);
        assert_eq!(p.name, "coldrevive-t0-sample.bin");
    }
}
