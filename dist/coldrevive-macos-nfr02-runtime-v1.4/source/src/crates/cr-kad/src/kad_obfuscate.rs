//! eMule/aMule KAD UDP 混淆层（RC4），从 S2 `kad-mini/src/wire.rs` 逐字节提升。
//!
//! 真机判别（2026-11-23，221 `112.5.60.4`）：同一台 firewalled 真 aMule 搜 `ubuntu`
//! 命中 361；我们的明文 `0xE4` 工具连本地 aMule（开了 `CryptoKadUDPKey`）也 0 命中。
//! 抓包证实 aMule 的 0x33/0x3B 全是混淆帧。所以公网 0 命中的根因不是 NAT/责任区，
//! 而是**我们只发明文**。本模块提供 try-0（目标 KadID）与 try-2（verify key）信封，
//! 供 `coldrevive-ed2k search --public` 在已知对方 KadID 时封装/解封。
//!
//! 这是纯协议兼容实现，不是安全特性；`OUR_UDP_KEY` 是固定诊断值。

use crate::kad_ed2k::ID_LEN;

type KadId = [u8; ID_LEN];

/// our publish/search. Not security-sensitive for this probe.
pub const OUR_UDP_KEY: u32 = 0x0A0B0C0D;

/// 固定的本机 KadID。HELLO_REQ 把它报给对端，回包解密的 try-0 也必须用同一个值；
/// 旧 CLI 收包时传全零 ID，导致 aMule 用我们的真实 KadID 加密的 HELLO_RES 被静默丢弃。
pub fn our_node_id() -> KadId {
    let mut seed = b"coldrevive-kad-node-v1".to_vec();
    seed.extend_from_slice(&OUR_UDP_KEY.to_le_bytes());
    let digest = md5(&seed);
    let mut id = [0u8; ID_LEN];
    id.copy_from_slice(&digest);
    id
}

/// aMule `CPrefs::GetUDPVerifyKey`: `MD5(u64(KadUDPKey<<32 | IP_as_seen_by_peer))`
/// 的四个小端 u32 异或，再映射到 `1..=0xFFFFFFFD`。`ip_octets` 是对端实际看到的
/// 源 IP（点分十进制顺序）；aMule 内部拿到的是网络字节序内存按小端读出的 u32。
pub fn udp_verify_key(kad_udp_key: u32, ip_octets: [u8; 4]) -> u32 {
    let target_ip = u32::from_le_bytes(ip_octets);
    let buffer = (((kad_udp_key as u64) << 32) | target_ip as u64).to_le_bytes();
    let digest = md5(&buffer);
    let mut xor = 0u32;
    for word in 0..4 {
        let i = 4 * word;
        xor ^= u32::from_le_bytes([digest[i], digest[i + 1], digest[i + 2], digest[i + 3]]);
    }
    (xor % 0xFFFF_FFFE) + 1
}

/// aMule `MAGICVALUE_UDP_SYNC_CLIENT` (the obfuscated-packet marker).
pub const KAD_MAGIC: u32 = 0x395F2EC1;

/// aMule 的 KAD 混淆 RC4 base 是**不对称**的，不能用一个函数两边共用：
///
/// - 收包（`EncryptedDatagramSocket.cpp::DecryptReceivedClient`）：对端用
///   `GetKadID().StoreCryptValue()`。x86 上 `SetValueBE` 与 `StoreCryptValue`
///   各做一次 swap，净效果是线上 ID **每个 4 字节小端字反转**。
/// - 发包（`EncryptSendClient`）：对端用 `md4cpy(keyData, clientHashOrKadID)`，
///   即**原始线上 16 字节**，不做反转。
///
/// 2026-11-23 的自洽测试把两边都用「原始 ID」，所以本地回环通过、真 aMule
/// 却静默丢弃。这里拆成两个显式方向。
fn send_key_base(target_kad_id_be: &[u8; 16]) -> [u8; 16] {
    // 等价于 aMule 收包侧 StoreCryptValue：每个 4 字节字反序。
    let mut out = [0u8; 16];
    for w in 0..4 {
        let mut word = [0u8; 4];
        word.copy_from_slice(&target_kad_id_be[4 * w..4 * w + 4]);
        word.reverse();
        out[4 * w..4 * w + 4].copy_from_slice(&word);
    }
    out
}

/// aMule 发包侧 `md4cpy(clientHashOrKadID)` 的 base，即对端把我们的 KadID
/// 原样喂进 MD5；解它的回包必须用这个字节序。
fn receive_key_base(our_kad_id_be: &[u8; 16]) -> [u8; 16] {
    *our_kad_id_be
}

/// Build an obfuscated KAD datagram. `target_kad_id` is the RECEIVER's 16-byte
/// KAD ID (known from BOOTSTRAP_RES / KADEMLIA2_RES contacts); `inner` is the
/// plaintext `[0xE4][opcode][payload]`.
pub fn obfuscate(target_kad_id: &[u8; 16], our_key: u32, inner: &[u8]) -> Vec<u8> {
    obfuscate_with_keys(target_kad_id, 0, our_key, inner)
}

/// try-0 信封，但可显式写 `receiverVerifyKey`。HELLO_REQ 必须带上由对方
/// `KadUDPKey` 与我们可见 IP 派生的值，否则 `validReceiverKey=false`，
/// aMule 会把我们当未验证联系人，HELLO_RES_ACK 流程无法闭环。
pub fn obfuscate_with_keys(target_kad_id: &[u8; 16], receiver_verify_key: u32,
                           our_sender_key: u32, inner: &[u8]) -> Vec<u8> {
    // aMule uses a random 16-bit part; deterministic here for reproducibility.
    let random_key_part: u16 = 0x1234;
    let mut buf = vec![0u8; 1 + 2 + 4 + 1 + 4 + 4 + inner.len()];
    // byte 0: kad marker, low 2 bits cleared, never a protocol byte.
    buf[0] = 0x40;
    buf[1..3].copy_from_slice(&random_key_part.to_le_bytes());
    // RC4 key = MD5(对端收包侧 StoreCryptValue(base) || randomKeyPart LE)
    let mut keydata = [0u8; 18];
    keydata[..16].copy_from_slice(&send_key_base(target_kad_id));
    keydata[16..18].copy_from_slice(&random_key_part.to_le_bytes());
    let key = md5(&keydata);
    let mut rc4 = Rc4::new(&key);
    // aMule 发包：magicValue = ENDIAN_SWAP_32(MAGIC)（x86 恒等），RC4Crypt 把
    // 内存按 LE 写出；收包再按 LE 读回。因此线上字节是 MAGIC.to_le_bytes()。
    let mut magic = KAD_MAGIC.to_le_bytes();
    rc4.crypt(&mut magic);
    buf[3..7].copy_from_slice(&magic);
    // padLen = 0
    let mut pad = [0u8; 1];
    rc4.crypt(&mut pad);
    buf[7] = pad[0];
    // verify key 同理：ENDIAN_SWAP_I_32 在 x86 恒等，线上为小端字节。
    let mut rvk = receiver_verify_key.to_le_bytes();
    rc4.crypt(&mut rvk);
    let mut svk = our_sender_key.to_le_bytes();
    rc4.crypt(&mut svk);
    buf[8..12].copy_from_slice(&rvk);
    buf[12..16].copy_from_slice(&svk);
    // payload
    let mut payload = inner.to_vec();
    rc4.crypt(&mut payload);
    buf[16..].copy_from_slice(&payload);
    buf
}

/// Reverse `obfuscate` for inbound packets. aMule may have encrypted the reply
/// to us with try-0 (our own KadID, if it learned it) or try-2 (our UDP verify
/// key, which it stored from the `senderVerifyKey` we declared). We try both
/// candidate RC4 bases and accept the first that decrypts to the obfuscation
/// magic. On success returns `(inner_frame, peer_sender_verify_key)`.
pub fn deobfuscate(data: &[u8], our_key: u32, our_node_id: &KadId) -> Option<(Vec<u8>, u32)> {
    if data.len() < 16 {
        return None;
    }
    let random_key_part = u16::from_le_bytes([data[1], data[2]]);
    // Candidate obfuscation bases, in priority order:
    //   1. try-0: our own KadID (StoreCryptValue layout) — peer learned it.
    //   2. try-2: our UDP verify key — peer locked the reply to it.
    let bases: [Vec<u8>; 2] = [
        // try-0: aMule 发包侧用 md4cpy(我们的 KadID)，不做 StoreCryptValue 反转。
        receive_key_base(our_node_id).to_vec(),
        // try-2: PokeUInt32(verifyKey) 写小端字节。
        our_key.to_le_bytes().to_vec(),
    ];
    for base in &bases {
        let mut keydata = Vec::with_capacity(base.len() + 2);
        keydata.extend_from_slice(base);
        keydata.extend_from_slice(&random_key_part.to_le_bytes());
        let key = md5(&keydata);
        let mut rc4 = Rc4::new(&key);
        let mut buf = data.to_vec();
        rc4.crypt(&mut buf[3..]);
        // 收包侧先按内存 LE 读出，再 ENDIAN_SWAP_I_32（x86 恒等）比较。
        let v = u32::from_le_bytes([buf[3], buf[4], buf[5], buf[6]]);
        if v != KAD_MAGIC {
            continue;
        }
        let pad_len = buf[7] as usize;
        let start = 16 + pad_len;
        if buf.len() < start {
            continue;
        }
        // senderVerifyKey sits at offset 12..16 of the decrypted crypt header.
        let svk_off = 12 + pad_len;
        let sender_verify_key = if buf.len() >= svk_off + 4 {
            u32::from_le_bytes([buf[svk_off], buf[svk_off + 1], buf[svk_off + 2], buf[svk_off + 3]])
        } else {
            0
        };
        let inner = buf[start..].to_vec();
        return Some((inner, sender_verify_key));
    }
    None
}

/// Obfuscated KAD datagram using the RECEIVER's verify key (aMule `try 2`):
/// RC4 key = `MD5(receiverVerifyKey[4 LE] || randomKeyPart[2 LE])`. Used for
/// publish/search once we have learned the peer's verify key from a prior
/// response's `senderVerifyKey` field, so `validReceiverKey` validates true.
pub fn obfuscate_verify(receiver_verify_key: u32, our_sender_key: u32, inner: &[u8]) -> Vec<u8> {
    let random_key_part: u16 = 0x1234;
    let mut buf = vec![0u8; 1 + 2 + 4 + 1 + 4 + 4 + inner.len()];
    buf[0] = 0x42; // KAD, try-2 (receiver-key) marker: bit0=0, bit1=1
    buf[1..3].copy_from_slice(&random_key_part.to_le_bytes());
    let mut keydata = [0u8; 6];
    // PokeUInt32(verifyKey) / PokeUInt16(random) 都写小端字节（x86 恒等 swap）。
    keydata[..4].copy_from_slice(&receiver_verify_key.to_le_bytes());
    keydata[4..6].copy_from_slice(&random_key_part.to_le_bytes());
    let key = md5(&keydata);
    let mut rc4 = Rc4::new(&key);
    let mut magic = KAD_MAGIC.to_le_bytes();
    rc4.crypt(&mut magic);
    buf[3..7].copy_from_slice(&magic);
    let mut pad = [0u8; 1];
    rc4.crypt(&mut pad);
    buf[7] = pad[0];
    let mut rvk = receiver_verify_key.to_le_bytes();
    rc4.crypt(&mut rvk);
    buf[8..12].copy_from_slice(&rvk);
    let mut svk = our_sender_key.to_le_bytes();
    rc4.crypt(&mut svk);
    buf[12..16].copy_from_slice(&svk);
    let mut payload = inner.to_vec();
    rc4.crypt(&mut payload);
    buf[16..].copy_from_slice(&payload);
    buf
}

// --- RC4 (aMule uses the standard PRGA, no keystream drop) ----------------
pub struct Rc4 {
    i: u8,
    j: u8,
    s: [u8; 256],
}

impl Rc4 {
    pub fn new(key: &[u8]) -> Self {
        let mut s = [0u8; 256];
        for (n, b) in s.iter_mut().enumerate() {
            *b = n as u8;
        }
        let mut j = 0u16;
        for n in 0..256 {
            j = (j + s[n] as u16 + key[n % key.len()] as u16) & 0xFF;
            s.swap(n, j as usize);
        }
        Rc4 { i: 0, j: 0, s }
    }
    pub fn crypt(&mut self, data: &mut [u8]) {
        for x in data.iter_mut() {
            self.i = self.i.wrapping_add(1);
            self.j = self.j.wrapping_add(self.s[self.i as usize]);
            self.s.swap(self.i as usize, self.j as usize);
            let k = self.s
                [(self.s[self.i as usize] as usize + self.s[self.j as usize] as usize) & 0xFF];
            *x ^= k;
        }
    }
}

// --- MD5 (RFC 1321) -------------------------------------------------------
pub fn md5(message: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
        14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
        21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let mut k = [0u32; 64];
    for (i, slot) in k.iter_mut().enumerate() {
        *slot = (f64::from(i as u32 + 1).sin().abs() * 4294967296.0).floor() as u32;
    }
    let mut h = [0x67452301u32, 0xEFCDAB89, 0x98BADCFE, 0x10325476];
    let mut msg = message.to_vec();
    let bit_len = (message.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 16];
        for (i, word) in w.iter_mut().enumerate() {
            *word = u32::from_le_bytes([
                chunk[4 * i],
                chunk[4 * i + 1],
                chunk[4 * i + 2],
                chunk[4 * i + 3],
            ]);
        }
        let (mut a, mut b, mut c, mut d) = (h[0], h[1], h[2], h[3]);
        for i in 0..64 {
            let f = match i {
                0..=15 => (b & c) | ((!b) & d),
                16..=31 => (d & b) | ((!d) & c),
                32..=47 => b ^ c ^ d,
                _ => c ^ (b | !d),
            };
            let g = if i < 16 {
                i
            } else if i < 32 {
                (5 * i + 1) % 16
            } else if i < 48 {
                (3 * i + 5) % 16
            } else {
                (7 * i) % 16
            };
            let f = f
                .wrapping_add(a)
                .wrapping_add(k[i])
                .wrapping_add(w[g]);
            let f = f.rotate_left(S[i]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i+2], 16).unwrap()).collect()
    }

    #[test]
    fn md5_matches_known_vectors() {
        assert_eq!(hex(&md5(b"").iter().map(|b| format!("{:02x}", b)).collect::<String>()),
                   hex("d41d8cd98f00b204e9800998ecf8427e"));
        assert_eq!(format!("{}", md5(b"abc").iter().map(|b| format!("{:02x}", b)).collect::<String>()),
                   "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn obfuscate_roundtrips_through_deobfuscate() {
        // aMule 收包用 StoreCryptValue(base)，发包用原始 ID；这里让对端 ID
        // 的 StoreCryptValue 正好等于我们的原始 ID，才能做双向自洽回环。
        let mut target = our_node_id();
        for w in 0..4 {
            let mut word = [0u8; 4];
            word.copy_from_slice(&target[4 * w..4 * w + 4]);
            word.reverse();
            target[4 * w..4 * w + 4].copy_from_slice(&word);
        }
        let inner = crate::kad_ed2k::search_key_req(&[0x11u8; ID_LEN]);
        let env = obfuscate(&target, OUR_UDP_KEY, &inner);
        assert!(env.len() >= 16 + inner.len());
        assert_ne!(env[0], 0xe4); // marker randomized, never plaintext KAD
        let (decoded, svk) = deobfuscate(&env, OUR_UDP_KEY, &our_node_id()).expect("must decode");
        assert_eq!(decoded, inner);
        assert_eq!(svk, OUR_UDP_KEY);
    }

    /// 按 aMule `EncryptedDatagramSocket::DecryptReceivedClient` 复刻收包：
    /// try-0 的 base 是对端自己 KadID 的 StoreCryptValue，magic/verify key
    /// 都是 LE；这是真机是否接受我们混淆帧的判据，不能只做自洽回环。
    fn decrypt_like_amule(env: &[u8], base: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
        let mut keydata = base.to_vec();
        keydata.extend_from_slice(&env[1..3]);
        let mut buf = env.to_vec();
        Rc4::new(&md5(&keydata)).crypt(&mut buf[3..]);
        if u32::from_le_bytes([buf[3], buf[4], buf[5], buf[6]]) != KAD_MAGIC {
            return None;
        }
        let rvk = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
        let svk = u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]);
        Some((buf[16..].to_vec(), rvk, svk))
    }

    #[test]
    fn outgoing_try0_envelope_decrypts_with_amule_receive_semantics() {
        let target = [0x37u8; ID_LEN];
        let inner = crate::kad_ed2k::search_key_req(&[0x11u8; ID_LEN]);
        let env = obfuscate_with_keys(&target, 0x11223344, OUR_UDP_KEY, &inner);
        let (decoded, rvk, svk) =
            decrypt_like_amule(&env, &send_key_base(&target)).expect("aMule must decrypt our try0");
        assert_eq!(decoded, inner);
        assert_eq!(rvk, 0x11223344);
        assert_eq!(svk, OUR_UDP_KEY);
    }

    #[test]
    fn outgoing_try2_envelope_decrypts_with_amule_receive_semantics() {
        let inner = [0xe4u8, 0x11, 0xaa, 0xbb];
        let env = obfuscate_verify(0x01020304, OUR_UDP_KEY, &inner);
        let (decoded, rvk, svk) =
            decrypt_like_amule(&env, &0x01020304u32.to_le_bytes()).expect("aMule must decrypt our try2");
        assert_eq!(decoded, inner);
        assert_eq!(rvk, 0x01020304);
        assert_eq!(svk, OUR_UDP_KEY);
    }

    #[test]
    fn decodes_real_amule_hello_res_with_raw_id_base() {
        // 2026-11-23 真 aMule 192.168.1.221:4672 的 HELLO_RES，用我们报给它的
        // KadID（原始字节，不能反转）作 base 才能解出 magic 与 0xE4/0x19 内帧。
        let env = hex("d0e472a4ac7c0409d9f41384a11ecab304090e99e947b97e41f834086ba7711a41a8dbe3c1462e6c");
        let our_id = hex("e95a736e6e3ef1095221043f1c1bb5ff");
        let mut id = [0u8; ID_LEN];
        id.copy_from_slice(&our_id);
        let (inner, svk) = deobfuscate(&env, OUR_UDP_KEY, &id).expect("real HELLO_RES must decode");
        assert_eq!(&inner[..2], &[0xe4, 0x19]);
        assert_eq!(svk, 3224252923);
    }

    #[test]
    fn obfuscate_verify_uses_try2_marker() {
        let env = obfuscate_verify(0x01020304, OUR_UDP_KEY, &[0xaa, 0xbb]);
        assert_eq!(env[0], 0x42);
        assert!(env.len() >= 18);
    }

    #[test]
    fn udp_verify_key_matches_amule_prefs_formula() {
        // 221 的 CryptoKadUDPKey=2738379211，A 的公网出口 112.5.60.4。
        // 独立用 Python 按 CPrefs::GetUDPVerifyKey 算出 3701290942。
        assert_eq!(udp_verify_key(2738379211, [112, 5, 60, 4]), 3701290942);
    }

    #[test]
    fn try2_envelope_decodes_with_little_endian_key_base() {
        let inner = [0xe4u8, 0x19, 0xa5, 0x5a];
        let receiver_key = 3701290942;
        let env = obfuscate_verify(receiver_key, OUR_UDP_KEY, &inner);
        // 接收方只知道自己的 verify key；旧实现用 BE base 会在这里失败。
        let (decoded, svk) = deobfuscate(&env, receiver_key, &our_node_id())
            .expect("try-2 envelope must decode with the receiver key");
        assert_eq!(decoded, inner);
        assert_eq!(svk, OUR_UDP_KEY);
    }

    #[test]
    fn our_node_id_is_stable_and_nonzero() {
        let id = our_node_id();
        assert_eq!(id, our_node_id());
        assert_ne!(id, [0u8; ID_LEN]);
    }
}
