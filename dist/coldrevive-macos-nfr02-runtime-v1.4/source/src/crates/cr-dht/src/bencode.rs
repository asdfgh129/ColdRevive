//! bencode（BEP 3）编解码，只覆盖 KRPC 实际用到的四种类型。
//!
//! 为什么自己写而不用 `cr-torrent`：那边只有**编码**（生成种子用），而 KRPC 需要
//! 解析对端发来的任意字典（`r` / `a` 都是嵌套 dict，键是字节串不是 `String`）。
//! 两个方向的语义不同，硬塞进种子生成器只会让两边都难改。
//!
//! 纪律：
//! - **字典键按字节序排序**编码（BEP 3 要求）。用 `BTreeMap<Vec<u8>, _>` 天然满足，
//!   不需要调用方记得排序——排序错了的字典会被严格实现拒收，而本机测试看不出来。
//! - 解码有**深度上限**。KRPC 是公网无鉴权流量，深嵌套 `llllll…` 能把递归解码器打爆
//!   栈；这类输入必须报错，而不是「尽力解出来」。
//! - 不接受前导零整数（`i03e`）与非规范长度，避免同一份报文有多种编码（可被用来
//!   绕过「同一 tid 只处理一次」这类去重）。

use std::collections::BTreeMap;

/// 解码深度上限。合法 KRPC 报文最多 3～4 层（`d` → `r` → 值）。
pub const MAX_DEPTH: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<Value>),
    Dict(BTreeMap<Vec<u8>, Value>),
}

impl Value {
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(b) => Some(b),
            _ => None,
        }
    }

    pub fn int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn list(&self) -> Option<&[Value]> {
        match self {
            Value::List(l) => Some(l),
            _ => None,
        }
    }

    pub fn dict(&self) -> Option<&BTreeMap<Vec<u8>, Value>> {
        match self {
            Value::Dict(d) => Some(d),
            _ => None,
        }
    }

    /// 按字符串键取；键本身就是字节串，这里只做一次不产生分配的查找。
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.dict().and_then(|d| d.get(key.as_bytes()))
    }

    pub fn as_str(&self) -> Option<&str> {
        self.bytes().and_then(|b| std::str::from_utf8(b).ok())
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_into(&mut out);
        out
    }

    fn encode_into(&self, out: &mut Vec<u8>) {
        match self {
            Value::Int(n) => {
                out.push(b'i');
                out.extend_from_slice(n.to_string().as_bytes());
                out.push(b'e');
            }
            Value::Bytes(b) => {
                out.extend_from_slice(b.len().to_string().as_bytes());
                out.push(b':');
                out.extend_from_slice(b);
            }
            Value::List(l) => {
                out.push(b'l');
                for v in l {
                    v.encode_into(out);
                }
                out.push(b'e');
            }
            Value::Dict(d) => {
                out.push(b'd');
                for (k, v) in d {
                    out.extend_from_slice(k.len().to_string().as_bytes());
                    out.push(b':');
                    out.extend_from_slice(k);
                    v.encode_into(out);
                }
                out.push(b'e');
            }
        }
    }
}

/// 构造字典的便捷入口（键按字节序排序由 `BTreeMap` 保证）。
pub fn dict(pairs: Vec<(&str, Value)>) -> Value {
    Value::Dict(pairs.into_iter().map(|(k, v)| (k.as_bytes().to_vec(), v)).collect())
}

pub fn bytes(b: impl AsRef<[u8]>) -> Value {
    Value::Bytes(b.as_ref().to_vec())
}

pub fn int(n: i64) -> Value {
    Value::Int(n)
}

/// 解析一个完整值；**必须整段消费**，尾部多余字节视为错误。
pub fn decode(input: &[u8]) -> Result<Value, String> {
    let mut p = Parser { buf: input, pos: 0 };
    let v = p.value(0)?;
    if p.pos != input.len() {
        return Err(format!("bencode: 尾部有 {} 字节未消费", input.len() - p.pos));
    }
    Ok(v)
}

struct Parser<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.buf.get(self.pos).copied()
    }

    fn expect(&mut self, b: u8) -> Result<(), String> {
        if self.peek() == Some(b) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("bencode: 位置 {} 期望 {:?}，实际 {:?}",
                        self.pos, b as char, self.peek().map(|c| c as char)))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > MAX_DEPTH {
            return Err(format!("bencode: 嵌套超过 {MAX_DEPTH} 层，拒绝解析"));
        }
        match self.peek() {
            Some(b'i') => self.integer(),
            Some(b'l') => self.list(depth),
            Some(b'd') => self.dict(depth),
            Some(c) if c.is_ascii_digit() => self.byte_string(),
            Some(c) => Err(format!("bencode: 位置 {} 非法起始字节 {:?}", self.pos, c as char)),
            None => Err("bencode: 数据意外结束".to_string()),
        }
    }

    fn integer(&mut self) -> Result<Value, String> {
        self.expect(b'i')?;
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == b'e' {
                break;
            }
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.buf[start..self.pos])
            .map_err(|_| "bencode: 整数含非 UTF-8 字节".to_string())?;
        self.expect(b'e')?;
        if text.is_empty() {
            return Err("bencode: 空整数".to_string());
        }
        // 规范形式：不允许前导零（i03e）、不允许 i-0e。
        if (text.len() > 1 && text.starts_with('0')) || text == "-0" {
            return Err(format!("bencode: 非规范整数 {text}"));
        }
        text.parse::<i64>().map(Value::Int)
            .map_err(|e| format!("bencode: 整数 {text} 解析失败: {e}"))
    }

    fn byte_string(&mut self) -> Result<Value, String> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == b':' {
                break;
            }
            if !c.is_ascii_digit() {
                return Err(format!("bencode: 字符串长度含非法字符 {:?}", c as char));
            }
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.buf[start..self.pos])
            .map_err(|_| "bencode: 长度非 UTF-8".to_string())?;
        self.expect(b':')?;
        if text.is_empty() {
            return Err("bencode: 缺字符串长度".to_string());
        }
        if text.len() > 1 && text.starts_with('0') {
            return Err(format!("bencode: 非规范长度 {text}"));
        }
        let len: usize = text.parse().map_err(|e| format!("bencode: 长度 {text} 非法: {e}"))?;
        let end = self.pos.checked_add(len).ok_or("bencode: 长度溢出")?;
        if end > self.buf.len() {
            return Err(format!("bencode: 声明长度 {len} 超出剩余 {} 字节", self.buf.len() - self.pos));
        }
        let out = self.buf[self.pos..end].to_vec();
        self.pos = end;
        Ok(Value::Bytes(out))
    }

    fn list(&mut self, depth: usize) -> Result<Value, String> {
        self.expect(b'l')?;
        let mut items = Vec::new();
        loop {
            match self.peek() {
                Some(b'e') => {
                    self.pos += 1;
                    return Ok(Value::List(items));
                }
                None => return Err("bencode: 列表未闭合".to_string()),
                _ => items.push(self.value(depth + 1)?),
            }
        }
    }

    fn dict(&mut self, depth: usize) -> Result<Value, String> {
        self.expect(b'd')?;
        let mut map = BTreeMap::new();
        let mut last: Option<Vec<u8>> = None;
        loop {
            match self.peek() {
                Some(b'e') => {
                    self.pos += 1;
                    return Ok(Value::Dict(map));
                }
                None => return Err("bencode: 字典未闭合".to_string()),
                _ => {
                    let k = match self.byte_string()? {
                        Value::Bytes(b) => b,
                        _ => unreachable!(),
                    };
                    // 严格递增：既保证规范形式，也让重复键无从夹带。
                    if let Some(prev) = &last {
                        if k <= *prev {
                            return Err("bencode: 字典键未严格递增（非规范或重复键）".to_string());
                        }
                    }
                    last = Some(k.clone());
                    let v = self.value(depth + 1)?;
                    map.insert(k, v);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_nested() {
        let v = dict(vec![
            ("t", bytes("aa")),
            ("y", bytes("q")),
            ("a", dict(vec![("id", bytes([7u8; 20])), ("port", int(6881))])),
        ]);
        let enc = v.encode();
        // 键必须按字节序：a < t < y。
        assert!(enc.starts_with(b"d1:ad2:id20:"), "实际 {:?}", String::from_utf8_lossy(&enc));
        assert_eq!(decode(&enc).unwrap(), v);
    }

    #[test]
    fn int_and_string_edges() {
        assert_eq!(decode(b"i0e").unwrap(), Value::Int(0));
        assert_eq!(decode(b"i-42e").unwrap(), Value::Int(-42));
        assert_eq!(decode(b"0:").unwrap(), Value::Bytes(vec![]));
        assert_eq!(decode(b"3:abc").unwrap(), Value::Bytes(b"abc".to_vec()));
    }

    #[test]
    fn rejects_non_canonical_and_malformed() {
        for bad in [
            &b"i03e"[..], b"i-0e", b"03:abc", b"3:ab", b"i1", b"l1:a", b"d1:ae",
            b"d1:b0:1:a0:e", // 键逆序
            b"d1:a0:1:a0:e", // 重复键
        ] {
            assert!(decode(bad).is_err(), "应拒绝 {:?}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn rejects_trailing_bytes() {
        // 尾部垃圾必须报错：容忍它就是给了「同一 tid 两种解释」的空间。
        assert!(decode(b"i1ee").is_err());
        assert!(decode(b"1:ab").is_err());
    }

    #[test]
    fn depth_limit_stops_stack_blowup() {
        let deep = vec![b'l'; MAX_DEPTH + 4];
        assert!(decode(&deep).is_err());
    }

    #[test]
    fn len_prefix_overflow_is_rejected() {
        // 声明一个巨大的长度：必须报错而不是尝试分配。
        assert!(decode(b"99999999999999999999:x").is_err());
    }
}
