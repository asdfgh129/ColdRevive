//! cr-core — M1 公共类型与零依赖基础设施。
//!
//! 设计约束：本 crate 只用 `std`，不引入第三方依赖，保证 `cargo test --offline`
//! 在任意环境可复现。JSON 编解码为本 crate 自带的最小实现（覆盖 T0 契约所需子集）。
//! 全局「零依赖」已于 2026-11-16 收窄为**分层策略**（见 ADR-015）：协议/信令 crate 继续
//! std-only，BT 引擎与守护进程可用 librqbit + tokio。

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::net::SocketAddr;
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
    /// M2 NAT 接入配置。`nat` 段缺省时全部关闭，T0/T1 行为逐字节不变。
    pub nat_enabled: bool,
    pub nat_stun_servers: Vec<(String, u16)>,
    pub nat_gateway: Option<String>,
    pub nat_upnp_location: Option<String>,
    /// 单个中继地址（首个），保留旧字段语义；多中继见 `nat_relays`。
    pub nat_relay: Option<String>,
    /// 中继 PSK：64 位十六进制，或 `file:<路径>`。`None` = 明文（仅回环/实验）。
    pub nat_relay_key: Option<String>,
    /// W3.8：中继列表（`nat.relay` 支持逗号分隔），客户端按顺序尝试并在失败时切换。
    pub nat_relays: Vec<String>,
    /// 运维侧真实创建的 BT/TCP 显式端口映射（外部 IP:端口）。
    /// 只有配置了该值，tracker 才会向公网请求方通告公网 peer；未配置即返回空 peer 列表。
    pub nat_bt_external: Option<(String, u16)>,
    /// 运维侧真实创建的 HTTP/tracker 端口映射（外部 IP:端口）。
    /// 配置后伴随种子里会写入 `announce-list`（BEP 12）：LAN 一条 + 这条公网一条，
    /// 同一份种子同时服务两类客户端；tracker 仍按请求方 IP 决定回私网还是公网 peer。
    pub nat_http_external: Option<(String, u16)>,
    /// BT 监听端口上是否同时开 µTP（BEP 29，UDP 同端口）。**默认 `true`**：
    /// 标准客户端默认优先 µTP（Transmission 尤甚），只做 TCP 时要等它回落，首连体验差。
    /// 关掉只影响监听侧的 UDP 绑定，不改任何帧格式。
    pub nat_utp: bool,
    /// M2/W5.1 BT 引擎下载目录。`engine` 段缺省时落到 `<workdir>/downloads`。
    pub engine_dir: PathBuf,
    /// 引擎入站监听地址（`engine.listen` = `"ip:port"`）。`None` = 只做出站，不监听。
    pub engine_listen: Option<SocketAddr>,
    /// 做种活跃池上限（`engine.max_active_seeds`，W4.3）。`None` = 用 `cr-sched` 的默认值。
    ///
    /// 只有活跃池里的任务会 `activate`；超过上限时逐出最久未用者并暂停它。
    /// 这里用 `Option` 而不是直接落默认值，是为了让「配置缺省」和「显式配 8」在
    /// 诊断输出里可区分——默认值变更时不会悄悄改变已有部署的行为。
    pub engine_max_active_seeds: Option<usize>,
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
        // M2：`nat` 段完全可选。任何缺失/非法字段都按「关闭 / 无证据」处理，
        // 绝不生成占位 STUN 服务器或公网映射。
        let nat = j.get("nat").and_then(Json::as_obj);
        let nat_get = |k: &str| -> Option<String> {
            nat.and_then(|o| o.iter().find(|(n, _)| n == k))
                .and_then(|(_, v)| v.as_str()).map(str::to_string)
        };
        let nat_enabled = nat
            .and_then(|o| o.iter().find(|(n, _)| n == "enabled"))
            .and_then(|(_, v)| v.as_bool()).unwrap_or(false);
        let nat_stun_servers: Vec<(String, u16)> = nat
            .and_then(|o| o.iter().find(|(n, _)| n == "stun_servers"))
            .and_then(|(_, v)| v.as_arr()).unwrap_or(&[])
            .iter().filter_map(|v| v.as_str())
            .filter_map(|s| {
                let (h, p) = s.rsplit_once(':')?;
                let port: u16 = p.parse().ok()?;
                if h.is_empty() { None } else { Some((h.to_string(), port)) }
            }).collect();
        let nat_bt_external = nat_get("bt_external").and_then(|s| {
            let (ip, p) = s.rsplit_once(':')?;
            let port: u16 = p.parse().ok()?;
            Some((ip.to_string(), port))
        });
        // 公网 HTTP/tracker 端点。非法值按「未配置」处理——宁可让公网客户端
        // 拿不到 announce-list，也不要把一个 `11:announce` 里写错的地址发出去。
        let nat_http_external = nat_get("http_external").and_then(|s| {
            let (ip, p) = s.rsplit_once(':')?;
            let port: u16 = p.parse().ok()?;
            if ip.is_empty() { None } else { Some((ip.to_string(), port)) }
        });
        // µTP 默认开：标准客户端默认先试 µTP，只做 TCP 会让它们先干等一段。
        // `nat.utp=false` 可显式关掉（只留 TCP），不影响任何帧格式。
        let nat_utp = nat
            .and_then(|o| o.iter().find(|(n, _)| n == "utp"))
            .and_then(|(_, v)| v.as_bool())
            .unwrap_or(true);
        // W3.8：`nat.relay` 接受逗号 / 分号分隔的中继列表（单值等价于旧行为）。
        // 空白项与非法 `host:port` 一律丢弃，不生成占位端点。
        let nat_relays: Vec<String> = nat_get("relay")
            .unwrap_or_default()
            .split([',', ';'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .filter(|s| match s.rsplit_once(':') {
                Some((h, p)) => !h.is_empty() && p.parse::<u16>().is_ok(),
                None => false,
            })
            .map(str::to_string)
            .collect();
        let nat_relay = nat_relays.first().cloned();
        // M2/W5.1：`engine` 段完全可选。缺省 = 下载目录 `workdir/downloads` + **不监听入站**；
        // 引擎只在实际收到 `.torrent` 任务时惰性创建，T0/T1 的复活路径完全不经过它。
        let engine = j.get("engine").and_then(Json::as_obj);
        let engine_get = |k: &str| -> Option<String> {
            engine.and_then(|o| o.iter().find(|(n, _)| n == k))
                .and_then(|(_, v)| v.as_str()).map(str::to_string)
        };
        let workdir = PathBuf::from(get_s("workdir")?);
        let engine_dir = engine_get("dir").map(PathBuf::from)
            .unwrap_or_else(|| workdir.join("downloads"));
        // 非法 `engine.listen` 直接报错：静默忽略会让「我要做种」变成「其实没监听」。
        let engine_listen = match engine_get("listen") {
            None => None,
            Some(s) => {
                let a = s.parse::<SocketAddr>()
                    .map_err(|e| format!("engine.listen 非法（需要 ip:port）: {s}: {e}"))?;
                Some(a)
            }
        };
        // 活跃池上限：非法值直接报错。0 也报错——夹到 1 会让「配错了」看起来像「配好了」，
        // 而这个数字决定节点能不能供片，不能让它在配置层被悄悄改写。
        let engine_max_active_seeds = match engine_get("max_active_seeds") {
            None => None,
            Some(s) => {
                let n = s.parse::<usize>()
                    .map_err(|e| format!("engine.max_active_seeds 非法（需要正整数）: {s}: {e}"))?;
                if n == 0 {
                    return Err("engine.max_active_seeds 必须 ≥ 1".to_string());
                }
                Some(n)
            }
        };
        Ok(Config {
            node_id: String::new(),
            bind,
            nodes,
            sample_path: PathBuf::from(get_s("sample_path")?),
            keywords,
            namespace: j.get("namespace").and_then(Json::as_str)
                .unwrap_or(NAMESPACE).to_string(),
            workdir,
            golden_req: PathBuf::from(get_s("golden_req")?),
            golden_ack: PathBuf::from(get_s("golden_ack")?),
            downloader: PathBuf::from(get_s("downloader").unwrap_or_default()),
            challenge_window_sec: j.get("challenge_window_sec")
                .and_then(Json::as_i64).unwrap_or(CHALLENGE_WINDOW_SEC),
            nat_enabled,
            nat_stun_servers,
            nat_gateway: nat_get("gateway"),
            nat_upnp_location: nat_get("upnp_location"),
            nat_relay,
            nat_relay_key: nat_get("relay_key"),
            nat_relays,
            nat_bt_external,
            nat_http_external,
            nat_utp,
            engine_dir,
            engine_listen,
            engine_max_active_seeds,
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
    fn nat_section_is_optional_and_backward_compatible() {
        let base = r#"{
            "bind": "127.0.0.1",
            "nodes": {
                "A": {"http_port": 16800, "wake_udp_port": 16801, "bt_port": 16802},
                "B": {"http_port": 16900, "wake_udp_port": 16901, "bt_port": 16902}
            },
            "sample_path": "s.bin", "keywords": [], "workdir": ".run",
            "golden_req": "r.json", "golden_ack": "a.json"
        }"#;
        let p = std::env::temp_dir().join("cr_core_nat_default.json");
        fs::write(&p, base).unwrap();
        let cfg = Config::load(&p).unwrap();
        assert!(!cfg.nat_enabled);
        assert!(cfg.nat_stun_servers.is_empty());
        assert!(cfg.nat_relay.is_none());
        assert!(cfg.nat_bt_external.is_none());
        assert!(cfg.nat_http_external.is_none());
        assert!(cfg.nat_utp, "µTP 默认必须开启：标准客户端默认先试 µTP");

        let with_nat = r#"{
            "bind": "127.0.0.1",
            "nodes": {
                "A": {"http_port": 16800, "wake_udp_port": 16801, "bt_port": 16802},
                "B": {"http_port": 16900, "wake_udp_port": 16901, "bt_port": 16902}
            },
            "sample_path": "s.bin", "keywords": [], "workdir": ".run",
            "golden_req": "r.json", "golden_ack": "a.json",
            "nat": {
                "enabled": true,
                "stun_servers": ["127.0.0.1:3478", "stun.example:19302", "bad"],
                "gateway": "192.168.1.254",
                "relay": "127.0.0.1:7000",
                "bt_external": "203.0.113.9:17802",
                "http_external": "203.0.113.9:17800",
                "utp": false
            }
        }"#;
        let p2 = std::env::temp_dir().join("cr_core_nat_enabled.json");
        fs::write(&p2, with_nat).unwrap();
        let cfg2 = Config::load(&p2).unwrap();
        assert!(cfg2.nat_enabled);
        assert_eq!(cfg2.nat_stun_servers.len(), 2);
        assert_eq!(cfg2.nat_stun_servers[0], ("127.0.0.1".into(), 3478));
        assert_eq!(cfg2.nat_bt_external, Some(("203.0.113.9".into(), 17802)));
        assert_eq!(cfg2.nat_http_external, Some(("203.0.113.9".into(), 17800)));
        assert!(!cfg2.nat_utp, "显式 utp=false 必须被采纳");
        assert_eq!(cfg2.nat_relay.as_deref(), Some("127.0.0.1:7000"));
        let _ = fs::remove_file(&p);
        let _ = fs::remove_file(&p2);
    }

    /// W3.8：`nat.relay` 支持列表；单值必须与旧行为逐字节等价，非法项必须被丢弃。
    #[test]
    fn relay_list_is_parsed_and_junk_is_dropped() {
        let mk = |relay: &str| -> String {
            format!(r#"{{
                "bind": "127.0.0.1",
                "nodes": {{
                    "A": {{"http_port": 16800, "wake_udp_port": 16801, "bt_port": 16802}},
                    "B": {{"http_port": 16900, "wake_udp_port": 16901, "bt_port": 16902}}
                }},
                "sample_path": "s.bin", "keywords": [], "workdir": ".run",
                "golden_req": "r.json", "golden_ack": "a.json",
                "nat": {{"enabled": true, "relay": "{}"}}
            }}"#, relay)
        };
        let cases: [(&str, Vec<&str>); 4] = [
            ("127.0.0.1:7000", vec!["127.0.0.1:7000"]),
            ("a.example:7000, b.example:7001", vec!["a.example:7000", "b.example:7001"]),
            ("a:1;b:2; ", vec!["a:1", "b:2"]),
            // 无端口 / 端口非数字 / 空项一律丢弃，绝不造占位端点
            ("nohost, c:99", vec!["c:99"]),
        ];
        for (raw, want) in cases {
            let p = std::env::temp_dir().join(format!(
                "cr_core_relays_{}.json", want.len()));
            fs::write(&p, mk(raw)).unwrap();
            let cfg = Config::load(&p).unwrap();
            assert_eq!(cfg.nat_relays, want, "输入 {:?}", raw);
            assert_eq!(cfg.nat_relay.as_deref(), want.first().copied(), "首个即旧字段语义");
            let _ = fs::remove_file(&p);
        }
        // 全非法 -> 空列表 / None，不 panic
        let p = std::env::temp_dir().join("cr_core_relays_zero.json");
        fs::write(&p, mk("nope, also-nope")).unwrap();
        let cfg = Config::load(&p).unwrap();
        assert!(cfg.nat_relays.is_empty());
        assert!(cfg.nat_relay.is_none());
        let _ = fs::remove_file(&p);
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
