//! std-only DEFLATE/zlib 解压，用于 FR-25 接收 `OP_COMPRESSEDPART(0x40)`。
//!
//! 上传端 v2.4.114 起会按 stock aMule 的形态发 0x40：固定 Huffman 或 stored
//! DEFLATE 的 zlib 流。下载端此前只认 0x46，导致「我们的服务端 → 我们的客户端」
//! 在公网路径上被自己发出的压缩帧卡死（2026-11-24 实测）。这里补齐 stored、
//! fixed、dynamic 三种块类型，并做 Adler-32 与输出上限校验：不解压超过声明块长
//! 的字节，防止恶意压缩帧把内存撑爆。

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51,
    59, 67, 83, 99, 115, 131, 163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385,
    513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
    13, 13,
];
const CL_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

struct Bits<'a> {
    data: &'a [u8],
    byte: usize,
    bit: u8,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, byte: 0, bit: 0 }
    }

    fn one(&mut self) -> Result<u16, String> {
        if self.byte >= self.data.len() {
            return Err("DEFLATE 位流提前结束".into());
        }
        let v = (self.data[self.byte] >> self.bit) & 1;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.byte += 1;
        }
        Ok(v as u16)
    }

    fn bits(&mut self, n: u8) -> Result<u16, String> {
        let mut out = 0u16;
        for i in 0..n {
            out |= self.one()? << i;
        }
        Ok(out)
    }

    /// stored 块要求字节对齐：丢弃当前字节剩余位。
    fn align(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.byte += 1;
        }
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.byte + n > self.data.len() {
            return Err("stored DEFLATE 块长度超过可用字节".into());
        }
        let s = &self.data[self.byte..self.byte + n];
        self.byte += n;
        Ok(s)
    }
}

#[derive(Debug)]
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Self, String> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            if l > 15 {
                return Err(format!("非法的 Huffman 码长 {l}"));
            }
            counts[l as usize] += 1;
        }
        // 解码表按「码长分组、同长内按符号序」排列：短码符号先入表，
        // 不能按符号数值顺序直接写 first[len] 的位置——那会把固定树的
        // 0x3B(符号 11) 解成符号 0。
        let mut symbols = Vec::with_capacity(lengths.len());
        for len in 1..=15 {
            for (sym, &l) in lengths.iter().enumerate() {
                if l == len {
                    symbols.push(sym as u16);
                }
            }
        }
        Ok(Self { counts, symbols })
    }

    fn decode(&self, bits: &mut Bits<'_>) -> Result<u16, String> {
        let mut code = 0u16;
        let mut first = 0u16;
        let mut index = 0usize;
        for len in 1..=15 {
            code = (code << 1) | bits.one()?;
            let count = self.counts[len];
            if code >= first && code - first < count {
                return self.symbols.get(index + (code - first) as usize)
                    .copied().ok_or_else(|| "Huffman 解码索引越界".into());
            }
            index += count as usize;
            first = (first + count) << 1;
        }
        Err("Huffman 解码超过 15 位".into())
    }
}

fn fixed_tables() -> (Huffman, Huffman) {
    let mut lit = vec![0u8; 288];
    for (i, l) in lit.iter_mut().enumerate() {
        *l = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            280..=287 => 8,
            _ => 0,
        };
    }
    // 固定距离树是 32 个 5 位码（0..=31），少两个会让次代码全部查不到。
    (Huffman::new(&lit).unwrap(), Huffman::new(&[5u8; 32]).unwrap())
}

fn dynamic_tables(bits: &mut Bits<'_>) -> Result<(Huffman, Huffman), String> {
    let hlit = bits.bits(5)? as usize + 257;
    let hdist = bits.bits(5)? as usize + 1;
    let hclen = bits.bits(4)? as usize + 4;
    let mut cl = [0u8; 19];
    for i in 0..hclen {
        cl[CL_ORDER[i]] = bits.bits(3)? as u8;
    }
    let clt = Huffman::new(&cl)?;
    let mut lengths = Vec::with_capacity(hlit + hdist);
    while lengths.len() < hlit + hdist {
        match clt.decode(bits)? {
            s @ 0..=15 => lengths.push(s as u8),
            16 => {
                let prev = *lengths.last().ok_or("repeat 前没有码长")?;
                let n = bits.bits(2)? as usize + 3;
                for _ in 0..n { lengths.push(prev); }
            }
            17 => {
                let n = bits.bits(3)? as usize + 3;
                lengths.extend(std::iter::repeat(0).take(n));
            }
            18 => {
                let n = bits.bits(7)? as usize + 11;
                lengths.extend(std::iter::repeat(0).take(n));
            }
            _ => return Err("非法的码长重复符号".into()),
        }
    }
    if lengths.len() != hlit + hdist {
        return Err("动态 Huffman 码长表溢出".into());
    }
    let lit = Huffman::new(&lengths[..hlit])?;
    let dist = Huffman::new(&lengths[hlit..])?;
    Ok((lit, dist))
}

fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn decode_huffman_block(
    bits: &mut Bits<'_>,
    lit: &Huffman,
    dist: &Huffman,
    out: &mut Vec<u8>,
    max_out: usize,
) -> Result<(), String> {
    loop {
        let sym = lit.decode(bits)?;
        if sym == 256 {
            return Ok(());
        }
        if sym < 256 {
            if out.len() >= max_out {
                return Err("解压输出超过声明块长度".into());
            }
            out.push(sym as u8);
            continue;
        }
        let li = sym as usize - 257;
        if li >= LEN_BASE.len() {
            return Err("非法长度符号".into());
        }
        let len = LEN_BASE[li] as usize + bits.bits(LEN_EXTRA[li])? as usize;
        let ds = dist.decode(bits)? as usize;
        if ds >= DIST_BASE.len() {
            return Err("非法距离符号".into());
        }
        let distance = DIST_BASE[ds] as usize + bits.bits(DIST_EXTRA[ds])? as usize;
        if distance == 0 || distance > out.len() || out.len() + len > max_out {
            return Err("DEFLATE 回指越界或超过声明块长度".into());
        }
        for _ in 0..len {
            let b = out[out.len() - distance];
            out.push(b);
        }
    }
}

/// 解压完整 zlib 流；输出超过 `max_out` 立即报错。Adler-32 不符也报错。
pub fn inflate_zlib(data: &[u8], max_out: usize) -> Result<Vec<u8>, String> {
    if data.len() < 6 {
        return Err("zlib 流太短".into());
    }
    if data[0] & 0x0f != 8 || data[0] >> 4 > 7 || (data[0] as u16 * 256 + data[1] as u16) % 31 != 0 {
        return Err("非法 zlib 头".into());
    }
    if data[1] & 0x20 != 0 {
        return Err("不支持 zlib preset dictionary".into());
    }
    let mut bits = Bits::new(&data[2..data.len() - 4]);
    let (fixed_lit, fixed_dist) = fixed_tables();
    let mut out = Vec::new();
    loop {
        let final_block = bits.one()? == 1;
        let btype = bits.bits(2)?;
        match btype {
            0 => {
                bits.align();
                let len = bits.bits(16)? as usize;
                let nlen = bits.bits(16)? as usize;
                if len != (!nlen & 0xffff) {
                    return Err("stored DEFLATE 长度校验失败".into());
                }
                if out.len() + len > max_out {
                    return Err("解压输出超过声明块长度".into());
                }
                out.extend_from_slice(bits.bytes(len)?);
            }
            1 => decode_huffman_block(&mut bits, &fixed_lit, &fixed_dist, &mut out, max_out)?,
            2 => {
                let (lit, dist) = dynamic_tables(&mut bits)?;
                decode_huffman_block(&mut bits, &lit, &dist, &mut out, max_out)?;
            }
            _ => return Err("保留的 DEFLATE 块类型 3".into()),
        }
        if final_block { break; }
    }
    let expected = u32::from_be_bytes([
        data[data.len() - 4], data[data.len() - 3], data[data.len() - 2], data[data.len() - 1],
    ]);
    if adler32(&out) != expected {
        return Err("zlib Adler-32 校验失败".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expected() -> Vec<u8> {
        (0..4096).map(|i| ((i * 37 + 11) % 256) as u8).collect()
    }

    #[test]
    fn inflates_stored_fixed_and_dynamic_blocks() {
        let want = expected();
        for (name, z) in [
            ("stored", include_bytes!("../tests/fixtures/inflate-stored.z").as_slice()),
            ("fixed", include_bytes!("../tests/fixtures/inflate-fixed.z").as_slice()),
            ("dynamic", include_bytes!("../tests/fixtures/inflate-dynamic.z").as_slice()),
        ] {
            let got = inflate_zlib(z, 4096).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(got, want, "{name} 块必须逐字节还原");
        }
    }

    #[test]
    fn rejects_corrupted_adler_and_oversized_output() {
        let z = include_bytes!("../tests/fixtures/inflate-dynamic.z");
        let mut bad = z.to_vec();
        *bad.last_mut().unwrap() ^= 0x01;
        assert!(inflate_zlib(&bad, 4096).unwrap_err().contains("Adler"));
        assert!(inflate_zlib(z, 100).unwrap_err().contains("超过"));
    }
}
