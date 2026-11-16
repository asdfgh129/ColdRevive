//! cr-hash — 零依赖 SHA-1 / SHA-256 / HMAC-SHA256 / MD4（eD2k）。
//!
//! - SHA-1：伴随 `.torrent` 的逐片校验与 InfoHash（BT 规范要求）。
//! - SHA-256：文件指纹（Target_Key 输入）与唤醒 Challenge 的 HMAC 基底。
//! - MD4 / eD2k hash：标准 ed2k 链接与 KAD 关键词字典的真实 `fileID`
//!   （W3.14，从 S2 的 kad-mini 探针迁入生产 crate；旧客户端找得到但下载不了
//!   虚假 hash，所以这里必须按 aMule 的 Red 分块规则算）。
//! - 不使用第三方 crate：M1 必须能 `cargo test --offline` 复现。

pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes { s.push_str(&format!("{:02x}", b)); }
    s
}

pub fn sha256_hex(data: &[u8]) -> String { hex(&sha256(data)) }
pub fn sha1_hex(data: &[u8]) -> String { hex(&sha1(data)) }

const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    for block in padded(data) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4], block[i * 4 + 1], block[i * 4 + 2], block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K256[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g; g = f; f = e; e = d.wrapping_add(t1);
            d = c; c = b; b = a; a = t1.wrapping_add(t2);
        }
        for (i, v) in [a, b, c, d, e, f, g, hh].iter().enumerate() {
            h[i] = h[i].wrapping_add(*v);
        }
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() { out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes()); }
    out
}

pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0];
    for block in padded(data) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4], block[i * 4 + 1], block[i * 4 + 2], block[i * 4 + 3],
            ]);
        }
        for i in 16..80 { w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1); }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5a827999u32),
                20..=39 => (b ^ c ^ d, 0x6ed9eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
                _ => (b ^ c ^ d, 0xca62c1d6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e)
                .wrapping_add(k).wrapping_add(w[i]);
            e = d; d = c; c = b.rotate_left(30); b = a; a = t;
        }
        h[0] = h[0].wrapping_add(a); h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c); h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (i, v) in h.iter().enumerate() { out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes()); }
    out
}

fn padded(data: &[u8]) -> Vec<[u8; 64]> {
    let bits = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&bits.to_be_bytes());
    msg.chunks_exact(64).map(|c| {
        let mut out = [0u8; 64];
        out.copy_from_slice(c);
        out
    }).collect()
}

/// RFC 2104 HMAC-SHA256。唤醒 Challenge 由 node_secret_seed 派生，禁止明文口令。
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let block = 64usize;
    let k: Vec<u8> = if key.len() > block { sha256(key).to_vec() } else { key.to_vec() };
    let mut ipad = vec![0x36u8; block];
    let mut opad = vec![0x5cu8; block];
    for i in 0..k.len() { ipad[i] ^= k[i]; opad[i] ^= k[i]; }
    ipad.extend_from_slice(msg);
    let inner = sha256(&ipad);
    opad.extend_from_slice(&inner);
    sha256(&opad)
}

pub fn hmac_sha256_hex(key: &[u8], msg: &[u8]) -> String { hex(&hmac_sha256(key, msg)) }

// --- eD2k / MD4 (RFC 1320) ------------------------------------------------
// 从 tests/spikes/s2/kad-mini/src/wire.rs 迁入（同一仓库、零依赖，保持单一实现）。
// aMule 用 MD4 做 KadGetKeywordHash 与 9.28 MB 分块的文件 hash。

/// MD4（RFC 1320），已知答案向量见本文件测试。
pub fn md4(message: &[u8]) -> [u8; 16] {
    const S1: [u32; 4] = [3, 7, 11, 19];
    const S2: [u32; 4] = [3, 5, 9, 13];
    const S3: [u32; 4] = [3, 9, 11, 15];
    const K2: u32 = 0x5A82_7999;
    const K3: u32 = 0x6ED9_EBA1;
    const R2: [usize; 16] = [0, 4, 8, 12, 1, 5, 9, 13, 2, 6, 10, 14, 3, 7, 11, 15];
    const R3: [usize; 16] = [0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15];

    let mut msg = message.to_vec();
    let bit_len = (message.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    let mut h = [0x6745_2301u32, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476];
    for chunk in msg.chunks(64) {
        let mut x = [0u32; 16];
        for (i, w) in x.iter_mut().enumerate() {
            *w = u32::from_le_bytes([
                chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3],
            ]);
        }
        let (mut a, mut b, mut c, mut d) = (h[0], h[1], h[2], h[3]);
        for i in 0..16 {
            let f = (b & c) | ((!b) & d);
            a = a.wrapping_add(f).wrapping_add(x[i]).rotate_left(S1[i % 4]);
            let t = a; a = d; d = c; c = b; b = t;
        }
        for i in 0..16 {
            let g = (b & c) | (b & d) | (c & d);
            a = a.wrapping_add(g).wrapping_add(x[R2[i]]).wrapping_add(K2).rotate_left(S2[i % 4]);
            let t = a; a = d; d = c; c = b; b = t;
        }
        for i in 0..16 {
            let f = b ^ c ^ d;
            a = a.wrapping_add(f).wrapping_add(x[R3[i]]).wrapping_add(K3).rotate_left(S3[i % 4]);
            let t = a; a = d; d = c; c = b; b = t;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

/// aMule 分块大小 = 9500 KiB = 9_728_000 B。
pub const ED2K_PART_SIZE: usize = 9_500 * 1024;

/// eD2k `fileID`：小于一块时直接 MD4；否则 MD4(各块 MD4 拼接)。
/// 文件刚好是 N 块时按 **Red** 规则再补一个空块 MD4（aMule 的做法），
/// 用 Blue 规则会算出旧客户端永远下载不了的 `fileID`。
pub fn ed2k_hash(data: &[u8]) -> [u8; 16] {
    if data.len() < ED2K_PART_SIZE { return md4(data); }
    let mut parts = Vec::with_capacity((data.len() / ED2K_PART_SIZE + 1) * 16);
    for chunk in data.chunks(ED2K_PART_SIZE) { parts.extend_from_slice(&md4(chunk)); }
    if data.len() % ED2K_PART_SIZE == 0 { parts.extend_from_slice(&md4(&[])); }
    md4(&parts)
}

/// KAD 关键词字典用的 hash：`MD4(UTF-8(term))`，大小写敏感、不做归一化。
pub fn kad_keyword_hash(term: &str) -> [u8; 16] { md4(term.as_bytes()) }

/// 标准 `ed2k://|file|...|/` 链接。文件名按 eD2k 习惯做最小百分号编码，
/// 否则文件名里的 `|` / `/` 会破坏链接字段分隔。
pub fn ed2k_link(file_name: &str, size: usize, hash: &[u8; 16]) -> String {
    let mut name = String::with_capacity(file_name.len());
    for b in file_name.as_bytes() {
        let ok = b.is_ascii_alphanumeric() || matches!(*b, b'.' | b'_' | b'-' | b' ');
        if ok { name.push(*b as char); } else { name.push_str(&format!("%{:02X}", b)); }
    }
    format!("ed2k://|file|{}|{}|{}|/", name, size, hex(hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn sha1_known_vectors() {
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[test]
    fn hmac_rfc4231_case_1() {
        assert_eq!(hmac_sha256_hex(&[0x0b; 20], b"Hi There"),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7");
    }

    #[test]
    fn padding_boundaries() {
        // 55 / 56 / 64 字节是 SHA 填充的临界长度
        for n in [0usize, 1, 55, 56, 63, 64, 65, 1000] {
            let d = vec![0x61u8; n];
            assert_eq!(sha1(&d).len(), 20);
            assert_eq!(sha256(&d).len(), 32);
        }
    }

    #[test]
    fn md4_rfc1320_known_answers() {
        assert_eq!(hex(&md4(b"")), "31d6cfe0d16ae931b73c59d7e0c089c0");
        assert_eq!(hex(&md4(b"a")), "bde52cb31de33e46245e05fbdbd6fb24");
        assert_eq!(hex(&md4(b"abc")), "a448017aaf21d8525fc10ae87aa6729d");
        assert_eq!(hex(&md4(b"message digest")), "d9130a8164549fe818874806e1c7014b");
    }

    #[test]
    fn ed2k_red_part_rule_matches_amule() {
        // 小于一块：fileID 就是 MD4 本身。
        assert_eq!(ed2k_hash(b"abc"), md4(b"abc"));
        // aMule KnownFile.cpp 文档给出的 1*PARTSIZE 文件向量。
        let part = [
            0x48, 0x91, 0xED, 0x2E, 0x5C, 0x9C, 0x49, 0xF4,
            0x42, 0x14, 0x5A, 0x3A, 0x5F, 0x60, 0x82, 0x99,
        ];
        let mut concat = Vec::from(part);
        concat.extend_from_slice(&md4(&[]));
        assert_eq!(hex(&md4(&concat)), "a72ca8df7f07154e217c236c89c17619");
        // 生产实现必须在整块边界追加空块 hash（Red），不能是 Blue。
        let exact = vec![0x33u8; ED2K_PART_SIZE];
        let mut red_input = Vec::new();
        red_input.extend_from_slice(&md4(&exact));
        red_input.extend_from_slice(&md4(&[]));
        assert_eq!(ed2k_hash(&exact), md4(&red_input));
        assert_ne!(ed2k_hash(&exact), md4(&red_input[..16]));
    }

    #[test]
    fn ed2k_link_is_standard_and_escaped() {
        let h = md4(b"abc");
        assert_eq!(
            ed2k_link("a|b/c.bin", 3, &h),
            format!("ed2k://|file|a%7Cb%2Fc.bin|3|{}|/", hex(&h))
        );
        assert_ne!(kad_keyword_hash("ABC"), kad_keyword_hash("abc"));
    }
}
