//! cr-core — M1 公共类型与零依赖基础设施。
//!
//! 设计约束：M1 全部 crate 只用 `std`，不引入第三方依赖，保证 `cargo test --offline`
//! 在任意环境可复现。JSON 编解码为本 crate 自带的最小实现（覆盖 T0 契约所需子集）。

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const NAMESPACE: &str = "ColdRevive_V1:";
pub const PROTOCOL_VERSION: u32 = 1;
pub const CLIENT_VERSION: &str = "coldrevive/0.1.0";
pub const CHALLENGE_WINDOW_SEC: i64 = 300;
pub const WAKE_TIMEOUT_MS: u64 = 1500;
pub const WAKE_RETRY_ONCE: bool = true;
pub const TTL_SEC: u64 = 7200;

// ---------------------------------------------------------------------------
// 最小 JSON（对象 / 数组 / 字符串 / 数字 / bool / null）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn obj(pairs: Vec<(&str, Json)>) -> Json {
        Json::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }
    pub fn s(v: impl Into<String>) -> Json { Json::Str(v.into()) }
    pub fn n(v: impl Into<f64>) -> Json { Json::Num(v.into()) }
    pub fn b(v: bool) -> Json { Json::Bool(v) }
    pub fn arr(v: Vec<Json>) -> Json { Json::Arr(v) }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(p) => p.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Json::Str(s) = self { Some(s) } else { None }
    }
    pub fn as_i64(&self) -> Option<i64> {
        if let Json::Num(n) = self { Some(*n as i64) } else { None }
    }
    pub fn as_bool(&self) -> Option<bool> {
        if let Json::Bool(b) = self { Some(*b) } else { None }
    }
    pub fn as_arr(&self) -> Option<&[Json]> {
        if let Json::Arr(a) = self { Some(a) } else { None }
    }
    pub fn as_obj(&self) -> Option<&[(String, Json)]> {
        if let Json::Obj(o) = self { Some(o) } else { None }
    }
    pub fn keys(&self) -> Vec<String> {
        match self {
            Json::Obj(o) => o.iter().map(|(k, _)| k.clone()).collect(),
            _ => Vec::new(),
        }
    }

    /// 紧凑序列化。所有字符串做完整转义，避免契约字段被截断或注入。
    pub fn dump(&self) -> String {
        match self {
            Json::Null => "null".to_string(),
            Json::Bool(b) => b.to_string(),
            Json::Num(n) => {
                if n.fract() == 0.0 && n.is_finite() { format!("{}", *n as i64) }
                else { format!("{}", n) }
            }
            Json::Str(s) => quote(s),
            Json::Arr(a) => format!(
                "[{}]",
                a.iter().map(|v| v.dump()).collect::<Vec<_>>().join(",")
            ),
            Json::Obj(o) => format!(
                "{{{}",
                o.iter()
                    .map(|(k, v)| format!("{}:{}", quote(k), v.dump()))
                    .collect::<Vec<_>>()
                    .join(",")
            ) + "}",
        }
    }

    pub fn parse(text: &str) -> Result<Json, String> {
        let mut p = Parser { b: text.as_bytes(), i: 0 };
        let v = p.value()?;
        p.ws();
        if p.i != p.b.len() {
            return Err(format!("JSON 尾部有冗余字符 @{}", p.i));
        }
        Ok(v)
    }

    pub fn load(path: &Path) -> Result<Json, String> {
        let s = fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        Json::parse(&s)
    }
}

impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{}", self.dump()) }
}

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

struct Parser<'a> { b: &'a [u8], i: usize }

impl<'a> Parser<'a> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\r' | b'\n') {
            self.i += 1;
        }
    }
    fn peek(&self) -> Result<u8, String> {
        self.b.get(self.i).copied().ok_or_else(|| "JSON 提前结束".to_string())
    }
    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        match self.peek()? {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => Ok(Json::Str(self.string()?)),
            b't' => { self.lit(b"true")?; Ok(Json::Bool(true)) }
            b'f' => { self.lit(b"false")?; Ok(Json::Bool(false)) }
            b'n' => { self.lit(b"null")?; Ok(Json::Null) }
            b'-' | b'0'..=b'9' => self.number(),
            c => Err(format!("非法 JSON 字符 {:?} @{}", c as char, self.i)),
        }
    }
    fn lit(&mut self, want: &[u8]) -> Result<(), String> {
        if self.b[self.i..].starts_with(want) { self.i += want.len(); Ok(()) }
        else { Err(format!("非法字面量 @{}", self.i)) }
    }
    fn object(&mut self) -> Result<Json, String> {
        self.i += 1;
        let mut pairs = Vec::new();
        self.ws();
        if self.peek()? == b'}' { self.i += 1; return Ok(Json::Obj(pairs)); }
        loop {
            self.ws();
            let k = self.string()?;
            self.ws();
            if self.peek()? != b':' { return Err(format!("对象缺 ':' @{}", self.i)); }
            self.i += 1;
            let v = self.value()?;
            pairs.push((k, v));
            self.ws();
            match self.peek()? {
                b',' => self.i += 1,
                b'}' => { self.i += 1; break; }
                _ => return Err(format!("对象分隔符非法 @{}", self.i)),
            }
        }
        Ok(Json::Obj(pairs))
    }
    fn array(&mut self) -> Result<Json, String> {
        self.i += 1;
        let mut out = Vec::new();
        self.ws();
        if self.peek()? == b']' { self.i += 1; return Ok(Json::Arr(out)); }
        loop {
            out.push(self.value()?);
            self.ws();
            match self.peek()? {
                b',' => self.i += 1,
                b']' => { self.i += 1; break; }
                _ => return Err(format!("数组分隔符非法 @{}", self.i)),
            }
        }
        Ok(Json::Arr(out))
    }
    fn string(&mut self) -> Result<String, String> {
        if self.peek()? != b'"' { return Err(format!("期望字符串 @{}", self.i)); }
        self.i += 1;
        let mut out = String::new();
        loop {
            let c = self.peek()?;
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = self.peek()?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'), b'\\' => out.push('\\'), b'/' => out.push('/'),
                        b'b' => out.push('\u{08}'), b'f' => out.push('\u{0c}'),
                        b'n' => out.push('\n'), b'r' => out.push('\r'), b't' => out.push('\t'),
                        b'u' => {
                            if self.i + 4 > self.b.len() { return Err("非法 \\u 转义".into()); }
                            let hex = std::str::from_utf8(&self.b[self.i..self.i + 4])
                                .map_err(|_| "非法 \\u 转义".to_string())?;
                            let cp = u32::from_str_radix(hex, 16)
                                .map_err(|_| "非法 \\u 码点".to_string())?;
                            self.i += 4;
                            out.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
                        }
                        _ => return Err(format!("非法转义 \\{} @{}", e as char, self.i)),
                    }
                }
                c if c < 0x20 => return Err(format!("字符串含控制字符 @{}", self.i)),
                _ => {
                    // 非 ASCII 字节按 UTF-8 逐字符回退重扫
                    self.i -= 1;
                    let rest = std::str::from_utf8(&self.b[self.i..])
                        .map_err(|_| "非法 UTF-8".to_string())?;
                    let ch = rest.chars().next().ok_or("字符串提前结束")?;
                    out.push(ch);
                    self.i += ch.len_utf8();
                }
            }
        }
        Ok(out)
    }
    fn number(&mut self) -> Result<Json, String> {
        let start = self.i;
        while self.i < self.b.len()
            && matches!(self.b[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        { self.i += 1; }
        let s = std::str::from_utf8(&self.b[start..self.i]).map_err(|_| "非法数字")?;
        s.parse::<f64>().map(Json::Num).map_err(|_| format!("非法数字 {:?}", s))
    }
}

// ---------------------------------------------------------------------------
// 标识与配置
// ---------------------------------------------------------------------------

static SEQ: AtomicU64 = AtomicU64::new(0);

fn entropy() -> u64 {
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
    t ^ (std::process::id() as u64).rotate_left(17) ^ SEQ.fetch_add(1, Ordering::Relaxed)
}

fn rand_hex(bytes: usize) -> String {
    let mut x = entropy();
    let mut out = String::with_capacity(bytes * 2);
    for _ in 0..bytes {
        x ^= x << 13; x ^= x >> 7; x ^= x << 17;
        out.push_str(&format!("{:02x}", (x & 0xff) as u8));
    }
    out
}

pub fn nonce_hex() -> String { rand_hex(16) }
pub fn trace_id() -> String {
    let h = rand_hex(16);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}
pub fn unix_ts() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

#[derive(Debug, Clone)]
pub struct NodeSpec {
    pub http_port: u16,
    pub wake_udp_port: u16,
    pub bt_port: u16,
    /// 该节点对外可达的 IP。缺省时回落到 `Config.bind`（T0 单机回环口径）。
    /// T1 双机时 A/B 各写自己的地址，两端共用同一份配置即可正确互指。
    pub host: String,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub node_id: String,
    pub bind: String,
    pub nodes: BTreeMap<String, NodeSpec>,
    pub sample_path: PathBuf,
    pub keywords: Vec<String>,
    pub namespace: String,
    pub workdir: PathBuf,
    pub golden_req: PathBuf,
    pub golden_ack: PathBuf,
    pub downloader: PathBuf,
    pub challenge_window_sec: i64,
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, String> {
        let j = Json::load(path)?;
        let get_s = |k: &str| -> Result<String, String> {
            j.get(k).and_then(Json::as_str).map(str::to_string)
                .ok_or_else(|| format!("配置缺字段 {}", k))
        };
        let bind = get_s("bind")?;
        let mut nodes = BTreeMap::new();
        let node_obj = j.get("nodes").and_then(Json::as_obj)
            .ok_or_else(|| "配置缺 nodes 对象".to_string())?;
        for (id, spec) in node_obj {
            let port = |k: &str| -> Result<u16, String> {
                spec.get(k).and_then(Json::as_i64).map(|v| v as u16)
                    .ok_or_else(|| format!("nodes.{}.{} 缺失", id, k))
            };
            nodes.insert(id.clone(), NodeSpec {
                http_port: port("http_port")?,
                wake_udp_port: port("wake_udp_port")?,
                bt_port: port("bt_port")?,
                host: spec.get("host").and_then(Json::as_str)
                    .unwrap_or(bind.as_str()).to_string(),
            });
        }
        let keywords = j.get("keywords").and_then(Json::as_arr)
            .unwrap_or(&[]).iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
        Ok(Config {
            node_id: String::new(),
            bind,
            nodes,
            sample_path: PathBuf::from(get_s("sample_path")?),
            keywords,
            namespace: j.get("namespace").and_then(Json::as_str)
                .unwrap_or(NAMESPACE).to_string(),
            workdir: PathBuf::from(get_s("workdir")?),
            golden_req: PathBuf::from(get_s("golden_req")?),
            golden_ack: PathBuf::from(get_s("golden_ack")?),
            downloader: PathBuf::from(get_s("downloader").unwrap_or_default()),
            challenge_window_sec: j.get("challenge_window_sec")
                .and_then(Json::as_i64).unwrap_or(CHALLENGE_WINDOW_SEC),
        })
    }

    pub fn me(&self) -> Result<&NodeSpec, String> {
        self.nodes.get(&self.node_id).ok_or_else(|| format!("nodes 缺 {}", self.node_id))
    }
    /// T0 固定双节点；peer 是「另一个」节点。
    pub fn peer_id(&self) -> String {
        self.nodes.keys().find(|k| **k != self.node_id).cloned().unwrap_or_default()
    }
    pub fn peer(&self) -> Result<&NodeSpec, String> {
        let id = self.peer_id();
        self.nodes.get(&id).ok_or_else(|| format!("nodes 缺 {}", id))
    }
    pub fn node_http(&self, id: &str) -> Option<u16> { self.nodes.get(id).map(|n| n.http_port) }
    /// 节点对外可达地址。T0 未配置 host 时等于 bind；T1 双机时按 nodes.<id>.host 返回。
    pub fn host(&self, id: &str) -> &str {
        self.nodes.get(id).map(|n| n.host.as_str()).unwrap_or(self.bind.as_str())
    }
    pub fn me_host(&self) -> &str { self.host(&self.node_id) }
    pub fn peer_host(&self) -> &str {
        let id = self.peer_id();
        self.host(&id)
    }
    pub fn url(&self, id: &str, path: &str) -> String {
        format!("http://{}:{}{}", self.host(id),
                self.node_http(id).unwrap_or(0), path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip() {
        let j = Json::obj(vec![
            ("v", Json::n(1.0)),
            ("type", Json::s("WAKEUP_REQ")),
            ("caps", Json::obj(vec![("ed2k", Json::b(false)), ("bt", Json::b(true))])),
            ("keys", Json::arr(vec![Json::s("a"), Json::s("b")])),
            ("note", Json::s("引号\" 与换行\n")),
        ]);
        let back = Json::parse(&j.dump()).unwrap();
        assert_eq!(j, back);
        assert_eq!(back.get("caps").unwrap().get("ed2k").unwrap().as_bool(), Some(false));
        assert_eq!(back.get("keys").unwrap().as_arr().unwrap().len(), 2);
    }

    #[test]
    fn constants_match_t0_contract() {
        assert_eq!(NAMESPACE, "ColdRevive_V1:");
        assert_eq!(PROTOCOL_VERSION, 1);
        assert_eq!(TTL_SEC, 7200);
    }

    #[test]
    fn identifiers_are_wide_enough() {
        let n = nonce_hex();
        assert_eq!(n.len(), 32);
        assert_ne!(n, nonce_hex());
        assert_eq!(trace_id().len(), 36);
    }

    #[test]
    fn per_node_host_defaults_to_bind_and_can_override() {
        let text = r#"{
            "bind": "127.0.0.1",
            "nodes": {
                "A": {"http_port": 16800, "wake_udp_port": 16801, "bt_port": 16802},
                "B": {"host": "192.168.1.211", "http_port": 16900,
                      "wake_udp_port": 16901, "bt_port": 16902}
            },
            "sample_path": "s.bin", "keywords": [], "workdir": ".run",
            "golden_req": "r.json", "golden_ack": "a.json"
        }"#;
        let p = std::env::temp_dir().join("cr_core_host_test.json");
        fs::write(&p, text).unwrap();
        let mut cfg = Config::load(&p).unwrap();
        // 未配置 host 的节点回落 bind —— T0 单机回环口径不变
        assert_eq!(cfg.host("A"), "127.0.0.1");
        assert_eq!(cfg.url("A", "/health"), "http://127.0.0.1:16800/health");
        // 显式 host 生效，供 T1 双机互指
        assert_eq!(cfg.host("B"), "192.168.1.211");
        assert_eq!(cfg.url("B", "/cli"), "http://192.168.1.211:16900/cli");
        cfg.node_id = "A".into();
        assert_eq!(cfg.me_host(), "127.0.0.1");
        assert_eq!(cfg.peer_host(), "192.168.1.211");
        let _ = fs::remove_file(&p);
    }
}
