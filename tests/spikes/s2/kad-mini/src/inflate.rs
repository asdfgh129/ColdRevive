//! Minimal zlib (RFC 1950) / DEFLATE (RFC 1951) decoder, std-only.
//!
//! Why this exists: aMule `CKademliaUDPListener::SendPacket()` calls
//! `CPacket::PackPacket()` for every KAD response larger than 200 bytes,
//! producing `[0xE5 (OP_KADEMLIAPACKEDPROT)][opcode][zlib stream]`.
//! `src/ClientUDPSocket.cpp` reverses it with
//! `uncompress(&unpack[2], &unpackedsize, decryptedBuffer + 2, packetLen - 2)`
//! then restores `unpack[0] = 0xE4; unpack[1] = opcode`.
//!
//! Without this decoder, a packed BOOTSTRAP_RES / KADEMLIA2_RES is recorded as
//! a "protocol-layer failure" even though the peer answered correctly, which
//! would corrupt the three-bucket observation semantics in RISK R-02.
//!
//! Deliberately strict and bounded: no preset dictionaries, Adler-32 verified,
//! output capped. No external crates — the whole kad-mini crate stays auditable.

const MAX_OUTPUT: usize = 1 << 20; // 1 MiB; a UDP datagram cannot exceed 64 KiB

pub fn zlib_decode(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 6 {
        return Err("zlib stream too short".into());
    }
    let (cmf, flg) = (data[0], data[1]);
    if cmf & 0x0F != 8 || cmf >> 4 > 7 {
        return Err(format!("unsupported zlib CM/CINFO 0x{cmf:02x}"));
    }
    if ((u16::from(cmf) << 8) | u16::from(flg)) % 31 != 0 {
        return Err("zlib header checksum mismatch".into());
    }
    if flg & 0x20 != 0 {
        return Err("zlib preset dictionary is not supported".into());
    }

    let body = &data[2..data.len() - 4];
    let mut br = BitReader::new(body);
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = br.bit()? == 1;
        match br.bits(2)? as u8 {
            0 => stored_block(&mut br, &mut out)?,
            1 => {
                let (lit, dist) = fixed_tables()?;
                huffman_block(&mut br, &lit, &dist, &mut out)?;
            }
            2 => {
                let (lit, dist) = dynamic_tables(&mut br)?;
                huffman_block(&mut br, &lit, &dist, &mut out)?;
            }
            _ => return Err("reserved DEFLATE block type 3".into()),
        }
        if last {
            break;
        }
    }

    let n = data.len();
    let want = u32::from_be_bytes([data[n - 4], data[n - 3], data[n - 2], data[n - 1]]);
    if adler32(&out) != want {
        return Err("zlib Adler-32 mismatch".into());
    }
    Ok(out)
}

struct BitReader<'a> {
    d: &'a [u8],
    byte: usize,
    bit: u32,
}

impl<'a> BitReader<'a> {
    fn new(d: &'a [u8]) -> Self {
        BitReader { d, byte: 0, bit: 0 }
    }
    fn bit(&mut self) -> Result<u32, String> {
        if self.byte >= self.d.len() {
            return Err("truncated DEFLATE stream".into());
        }
        let v = u32::from((self.d[self.byte] >> self.bit) & 1);
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.byte += 1;
        }
        Ok(v)
    }
    fn bits(&mut self, n: u32) -> Result<u32, String> {
        let mut v = 0u32;
        for i in 0..n {
            v |= self.bit()? << i;
        }
        Ok(v)
    }
    /// Drop the remaining bits of the current byte (stored blocks are byte-aligned).
    fn align(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.byte += 1;
        }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        self.align();
        if self.byte + n > self.d.len() {
            return Err("truncated stored block".into());
        }
        let s = &self.d[self.byte..self.byte + n];
        self.byte += n;
        Ok(s)
    }
}

fn stored_block(br: &mut BitReader<'_>, out: &mut Vec<u8>) -> Result<(), String> {
    let h = br.take(4)?;
    let len = u16::from_le_bytes([h[0], h[1]]) as usize;
    let nlen = u16::from_le_bytes([h[2], h[3]]);
    if nlen != !(len as u16) {
        return Err("stored block LEN/NLEN mismatch".into());
    }
    let s = br.take(len)?;
    out.extend_from_slice(s);
    check_size(out)
}

/// Canonical Huffman codes grouped by bit length; each entry is (code, symbol).
struct Huff {
    by_len: Vec<Vec<(u32, u16)>>,
}

fn build_huff(lengths: &[u8]) -> Result<Huff, String> {
    let mut count = [0u32; 16];
    for &l in lengths {
        if l > 15 {
            return Err("DEFLATE code length > 15".into());
        }
        if l != 0 {
            count[l as usize] += 1;
        }
    }
    // RFC 1951 §3.2.2: bl_count[0] MUST be 0. Including unused symbols here
    // shifts every canonical code left and breaks incomplete (dynamic) trees.
    count[0] = 0;
    let mut next = [0u32; 16];
    let mut code = 0u32;
    for bits in 1..=15 {
        code = (code + count[bits - 1]) << 1;
        next[bits] = code;
    }
    let mut by_len: Vec<Vec<(u32, u16)>> = (0..16).map(|_| Vec::new()).collect();
    for (sym, &l) in lengths.iter().enumerate() {
        if l == 0 {
            continue;
        }
        let c = next[l as usize];
        next[l as usize] += 1;
        by_len[l as usize].push((c, sym as u16));
    }
    Ok(Huff { by_len })
}

fn decode_sym(br: &mut BitReader<'_>, h: &Huff) -> Result<u16, String> {
    // DEFLATE packs Huffman codes MSB-first even though the bit reader is LSB-first.
    let mut code = 0u32;
    for len in 1..=15usize {
        code = (code << 1) | br.bit()?;
        for &(c, sym) in &h.by_len[len] {
            if c == code {
                return Ok(sym);
            }
        }
    }
    Err(format!("invalid DEFLATE Huffman code 0x{code:x} (15 bits)"))
}

fn fixed_tables() -> Result<(Huff, Huff), String> {
    let mut lit = vec![0u8; 288];
    for i in 0..=143 {
        lit[i] = 8;
    }
    for i in 144..=255 {
        lit[i] = 9;
    }
    for i in 256..=279 {
        lit[i] = 7;
    }
    for i in 280..=287 {
        lit[i] = 8;
    }
    Ok((build_huff(&lit)?, build_huff(&vec![5u8; 32])?))
}

const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

fn dynamic_tables(br: &mut BitReader<'_>) -> Result<(Huff, Huff), String> {
    let hlit = br.bits(5)? as usize + 257;
    let hdist = br.bits(5)? as usize + 1;
    let hclen = br.bits(4)? as usize + 4;

    let mut cl_lengths = vec![0u8; 19];
    for i in 0..hclen {
        cl_lengths[CODE_LENGTH_ORDER[i]] = br.bits(3)? as u8;
    }
    let cl_huff = build_huff(&cl_lengths)?;

    let total = hlit + hdist;
    let mut lengths: Vec<u8> = Vec::with_capacity(total);
    while lengths.len() < total {
        match decode_sym(br, &cl_huff).map_err(|e| format!("code-length alphabet: {e}"))? {
            v @ 0..=15 => lengths.push(v as u8),
            16 => {
                let prev = *lengths.last().ok_or("repeat code with no previous length")?;
                for _ in 0..(br.bits(2)? as usize + 3) {
                    lengths.push(prev);
                }
            }
            17 => {
                for _ in 0..(br.bits(3)? as usize + 3) {
                    lengths.push(0);
                }
            }
            18 => {
                for _ in 0..(br.bits(7)? as usize + 11) {
                    lengths.push(0);
                }
            }
            _ => return Err("invalid code-length symbol".into()),
        }
    }
    if lengths.len() > total {
        return Err("code-length sequence overflow".into());
    }
    let (lit, dist) = lengths.split_at(hlit);
    Ok((build_huff(lit)?, build_huff(dist)?))
}

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
    131, 163, 195, 227, 258,
];
const LEN_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
    13, 13,
];

fn huffman_block(
    br: &mut BitReader<'_>,
    lit: &Huff,
    dist: &Huff,
    out: &mut Vec<u8>,
) -> Result<(), String> {
    loop {
        let v = decode_sym(br, lit).map_err(|e| format!("literal alphabet: {e}"))? as usize;
        match v {
            0..=255 => {
                out.push(v as u8);
                check_size(out)?;
            }
            256 => return Ok(()),
            257..=285 => {
                let i = v - 257;
                let len = LEN_BASE[i] as usize + br.bits(LEN_EXTRA[i])? as usize;
                let ds = decode_sym(br, dist).map_err(|e| format!("distance alphabet: {e}"))? as usize;
                if ds >= 30 {
                    return Err("invalid distance symbol".into());
                }
                let d = DIST_BASE[ds] as usize + br.bits(DIST_EXTRA[ds])? as usize;
                if d == 0 || d > out.len() {
                    return Err("back-reference distance exceeds output".into());
                }
                for _ in 0..len {
                    let b = out[out.len() - d];
                    out.push(b);
                }
                check_size(out)?;
            }
            _ => return Err("reserved length symbol".into()),
        }
    }
}

fn check_size(out: &[u8]) -> Result<(), String> {
    if out.len() > MAX_OUTPUT {
        Err("inflate output exceeds 1 MiB bound".into())
    } else {
        Ok(())
    }
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}
