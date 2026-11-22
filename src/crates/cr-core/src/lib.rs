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
    /// 节点角色：`holder` 做种，`member` 只下载/唤醒。
    ///
    /// 缺省 `None` 时回退到旧口径（node_id == "A" 为 holder），保证老配置逐字节不变；
    /// 显式给了非法值直接报错——角色决定「是否启动做种引擎」，静默改写会变成
    /// 「以为在供片其实没有」，排查成本远高于启动失败。
    pub role: Option<String>,
    pub bind: String,
    pub nodes: BTreeMap<String, NodeSpec>,
    pub sample_path: PathBuf,
    /// P0：多样本做种（可选）。为空 = 只做 `sample_path` 这一个，行为与旧版逐字节一致。
    ///
    /// 守护进程会把这里每一个文件都登记进索引、以 `paused` 加入引擎，
    /// 唤醒时按请求的 infohash 激活**对应的那一个**。容量仍受 `engine.max_active_seeds` 约束。
    pub samples: Vec<PathBuf>,
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
    /// v2.4.86：中继的 **UDP 承载**端点（`nat.relay_udp`，逗号分隔列表）。
    ///
    /// 与 `nat.relay` 是**同一个中继的另一种承载**，不是第二个中继：
    /// 中继所在机器的入向 TCP 被掐时（本项目那台腾讯云轻量即如此），只能在 `nat.relay_udp`
    /// 里填它的 UDP 端口，订阅与唤醒都走 UDP。两者可以同时配置，各自独立工作。
    pub nat_relay_udps: Vec<String>,
    /// ADR-024 阶段 2：中继数据面承载。`"tcp"`（默认，ADR-023 既有行为）或 `"udp"`。
    ///
    /// 只有显式设为 `"udp"` 才注册 UDP 数据面会话；缺省与非法值都回落 `"tcp"`，
    /// 保证 W58/W59/W60 既有结论与默认行为逐字节不变（opt-in 硬约束）。
    /// UDP 承载用于「中继入向 TCP 被掐、UDP 可用」的部署；需要同时配 `nat.relay_udp`。
    pub nat_data_plane_carrier: String,
    /// 运维侧真实创建的 BT/TCP 显式端口映射（外部 IP:端口）。
    /// 只有配置了该值，tracker 才会向公网请求方通告公网 peer；未配置即返回空 peer 列表。
    pub nat_bt_external: Option<(String, u16)>,
    /// v2.4.92：**中继会合地址**（`nat.relay_rendezvous`，IPv4，限 RFC 5737 文档网段）。
    ///
    /// librqbit 只肯拨「看起来像公网 peer」的地址，而两台 NAT 后的机器谁都没有入向映射。
    /// 这个地址的唯一作用是让对端引擎愿意发起连接，再由**本机 SOCKS5 桥**把它改道到中继
    /// 隧道；它**不是**端口映射，也不会被通告成公网入向证据（`EndpointKind::Relay`）。
    ///
    /// 只允许 `203.0.113.0/24`、`198.51.100.0/24`、`192.0.2.0/24`：写真实 IP 会让后续的
    /// tracker 与可达性声明把它读成「真的有公网入向」，那是假证据。必须与
    /// `nat.punch_route=true` 同时使用。
    pub nat_relay_rendezvous: Option<String>,
    /// 运维侧真实创建的 HTTP/tracker 端口映射（外部 IP:端口）。
    /// 配置后伴随种子里会写入 `announce-list`（BEP 12）：LAN 一条 + 这条公网一条，
    /// 同一份种子同时服务两类客户端；tracker 仍按请求方 IP 决定回私网还是公网 peer。
    pub nat_http_external: Option<(String, u16)>,
    /// BT 监听端口上是否同时开 µTP（BEP 29，UDP 同端口）。**默认 `true`**：
    /// 标准客户端默认优先 µTP（Transmission 尤甚），只做 TCP 时要等它回落，首连体验差。
    /// 关掉只影响监听侧的 UDP 绑定，不改任何帧格式。
    pub nat_utp: bool,
    /// ADR-022：打洞优先 / 中继兜底的**双端协商**是否启用。默认 `false`，
    /// 老配置逐字节不变。启用后 BT 引擎的出向走本地 SOCKS5 桥，只有协商判定
    /// `Direct` 且 nonce 握手双端通过才登记 punched socket；否则回落中继数据面。
    ///
    /// v2.4.92 起 TCP 中继上**有** BT 数据面（ADR-023：OPEN/OPENED 握手 + 双向 splice），
    /// UDP 承载仍然没有重传层，所以「打不通就打不了」只对 UDP 路径成立。
    pub nat_punch_route: bool,
    /// M2/W5.1 BT 引擎下载目录。`engine` 段缺省时落到 `<workdir>/downloads`。
    pub engine_dir: PathBuf,
    /// 引擎入站监听地址（`engine.listen` = `"ip:port"`）。`None` = 只做出站，不监听。
    pub engine_listen: Option<SocketAddr>,
    /// FR-11：BT 握手拦截代理是否启用（`engine.guard_handshake`，默认 `false`）。
    ///
    /// 开启后由 cr-guard 占用对外 `bt_port`，读 68 B 标准握手 → 按 infohash 本地唤醒
    /// → 补回已读字节并 splice 给引擎的回环内部口。这样旧客户端**直接连入**
    /// 也能触发被动唤醒（SRS FR-11），不必先收到私有 UDP 唤醒。
    /// 默认关闭：既有 T0/T1/W5.x 证据的字节级行为不变；µTP 在代理模式下不外透，
    /// 因此该 opt-in 只宣称 TCP 握手拦截，不宣称 µTP 可达。
    pub bt_guard_handshake: bool,
    /// 做种活跃池上限（`engine.max_active_seeds`，W4.3）。`None` = 用 `cr-sched` 的默认值。
    ///
    /// 只有活跃池里的任务会 `activate`；超过上限时逐出最久未用者并暂停它。
    /// 这里用 `Option` 而不是直接落默认值，是为了让「配置缺省」和「显式配 8」在
    /// 诊断输出里可区分——默认值变更时不会悄悄改变已有部署的行为。
    pub engine_max_active_seeds: Option<usize>,
    /// FR-14：空闲多久（秒）自动把 HOT 做种任务切回 COLD。`None` = 关闭（默认）。
    ///
    /// SRS 写的是「N 分钟」，配置层用秒便于 e2e 用 2 s 复现而不必等 5 min；
    /// `engine.idle_sleep_minutes` 也接受（正整数，换算成秒），两者同时给时秒优先。
    /// 0 表示显式关闭；非法值直接报错，避免「以为会休眠其实一直在供片」。
    pub engine_idle_sleep_secs: Option<u64>,
    /// FR-09：HTTP 面按来源 IP 的令牌桶速率（`api.rate_per_sec`），单位 次/秒。
    ///
    /// `None` 或 0 = **关闭**（默认），既有部署与全部 T0/T1 证据行为不变。
    /// 限流只作用于 `/cli`、`/jsonrpc` 这类控制面；被拒时返回 429 并带
    /// `reason=rate_limited` 与 `retry_after_ms`，不伪装成 500/超时。
    pub api_rate_per_sec: Option<u32>,
    /// FR-09：突发额度（`api.rate_burst`）。缺省按 `rate_per_sec` 取同值。
    pub api_rate_burst: Option<u32>,
    /// FR-01：轮询式目录监听周期（`indexer.watch_secs`）。0/缺省 = 关闭。
    ///
    /// 开启后守护进程按周期比对共享目录快照，把新增/修改/删除落到索引。
    /// 这是**内核通知缺失时的正确兜底**（Windows 的 ReadDirectoryChangesW 尚未接入），
    /// 语义（新增/修改/删除 + 去抖）与通知式一致，代价是延迟 ≤ 周期。
    pub indexer_watch_secs: Option<u64>,
    /// FR-05/06：KAD 关键词的周期重发布间隔（`kad.republish_secs`）。0/缺省 = 关闭。
    ///
    /// 只对已经发布过（`kad_published=true`）的条目重发 `KAD_PUBLISH`，
    /// 不新增关键词、不改 TTL 口径；重发次数与最近一次结果在 `/health.kad_publish` 里可见。
    pub kad_republish_secs: Option<u64>,
    /// FR-15：允许同时处于 HOT 的做种条目上限（`limits.max_hot_seeds`）。0/缺省 = 关闭。
    ///
    /// 与 `engine.max_active_seeds` 的区别是**冲突时的行为**：那个是容量（满了逐出
    /// 最久未用者），这个是阈值（超了就**拒绝新的唤醒**，已供片的保持不动）。
    /// 计数取自索引里真实的 HOT 条目，不拿「配置了几个样本」充数。
    pub limits_max_hot_seeds: Option<usize>,
}

impl Config {
    /// 要做种的样本清单。`samples` 为空时退回单一的 `sample_path`（旧配置口径）。
    ///
    /// 顺序即"主样本在前"：`sample_path` 永远排第一，后面的 `samples` 依次追加，
    /// 且去重——同一个文件写两遍会让引擎加两个任务指向同一份数据。
    pub fn sample_paths(&self) -> Vec<PathBuf> {
        let mut out = vec![self.sample_path.clone()];
        for p in &self.samples {
            if !out.contains(p) {
                out.push(p.clone());
            }
        }
        out
    }
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
        // ADR-022：默认关闭。非法/缺失一律 false——静默打开会把所有出站 BT
        // 改道本地桥，而如果没有登记 socket，行为变化很难从日志里看出来。
        let nat_punch_route = nat
            .and_then(|o| o.iter().find(|(n, _)| n == "punch_route"))
            .and_then(|(_, v)| v.as_bool())
            .unwrap_or(false);
        // v2.4.92：中继会合地址。它不是映射，所以只允许文档网段——写真实 IP 会被
        // tracker 与可达性声明读成「有公网入向」，而实际上那条路只有本地桥能走。
        let nat_relay_rendezvous = match nat_get("relay_rendezvous") {
            None => None,
            Some(s) => {
                let ip: std::net::Ipv4Addr = s.parse().map_err(|_| {
                    format!("nat.relay_rendezvous 必须是 IPv4 地址（当前 \"{s}\"）")
                })?;
                let o = ip.octets();
                let doc = matches!((o[0], o[1], o[2]),
                                   (203, 0, 113) | (198, 51, 100) | (192, 0, 2));
                if !doc {
                    return Err(format!(
                        "nat.relay_rendezvous 必须是 RFC 5737 文档网段\
                         （203.0.113.0/24、198.51.100.0/24、192.0.2.0/24），当前 {ip}：\
                         会合地址不代表真实可达，写真实 IP 会被误读成端口映射"
                    ));
                }
                if !nat_punch_route {
                    return Err(
                        "nat.relay_rendezvous 需要同时开启 nat.punch_route：\
                         没有本地桥时该地址不可达，通告出去就是假 peer".into());
                }
                Some(ip.to_string())
            }
        };
        // W3.8：`nat.relay` 接受逗号 / 分号分隔的中继列表（单值等价于旧行为）。
        // 空白项与非法 `host:port` 一律丢弃，不生成占位端点。
        let split_endpoints = |key: &str| -> Vec<String> {
            nat_get(key)
                .unwrap_or_default()
                .split([',', ';'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .filter(|s| match s.rsplit_once(':') {
                    Some((h, p)) => !h.is_empty() && p.parse::<u16>().is_ok(),
                    None => false,
                })
                .map(str::to_string)
                .collect()
        };
        let nat_relays: Vec<String> = split_endpoints("relay");
        let nat_relay = nat_relays.first().cloned();
        // v2.4.86：中继的 UDP 承载端点。与 `relay` 同一套校验，非法项丢弃。
        let nat_relay_udps: Vec<String> = split_endpoints("relay_udp");
        // ADR-024：数据面承载，opt-in。缺省 / 非法 → "tcp"（回落既有行为）。
        let nat_data_plane_carrier = match nat_get("data_plane_carrier") {
            Some(s) if s.trim().eq_ignore_ascii_case("udp") => "udp".to_string(),
            Some(s) if s.trim().eq_ignore_ascii_case("tcp") => "tcp".to_string(),
            Some(s) => {
                eprintln!("[config] nat.data_plane_carrier 非法（{}），回落 tcp", s);
                "tcp".to_string()
            }
            None => "tcp".to_string(),
        };
        // M2/W5.1：`engine` 段完全可选。缺省 = 下载目录 `workdir/downloads` + **不监听入站**；
        // 引擎只在实际收到 `.torrent` 任务时惰性创建，T0/T1 的复活路径完全不经过它。
        let engine = j.get("engine").and_then(Json::as_obj);
        let engine_get = |k: &str| -> Option<String> {
            engine.and_then(|o| o.iter().find(|(n, _)| n == k))
                .and_then(|(_, v)| v.as_str()).map(str::to_string)
        };
        let workdir = PathBuf::from(get_s("workdir")?);
        // P0：多样本（`samples` 数组，可选）。缺省空数组 = 只做 `sample_path`，
        // 旧配置不写这个键时行为完全不变。空串/非字符串项丢弃，不造占位路径。
        let samples: Vec<PathBuf> = j.get("samples")
            .and_then(Json::as_arr)
            .map(|a| a.iter()
                .filter_map(Json::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .collect())
            .unwrap_or_default();
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
        // FR-14 空闲休眠：opt-in，默认关闭。分钟配置也在这里折算成秒，
        // 这样 /health 只暴露一个口径，不会因为两套单位产生对账歧义。
        let parse_idle_secs = |key: &str, raw: String| -> Result<u64, String> {
            let n = raw.trim().parse::<u64>()
                .map_err(|e| format!("engine.{key} 非法（需要非负整数秒）: {raw}: {e}"))?;
            Ok(n)
        };
        let idle_secs_raw = engine_get("idle_sleep_secs");
        // DOC-29 建议的顶层 `scheduler.idle_sleep_minutes` 也接受，
        // 优先级低于 engine 段同名项；两套都写时诊断输出仍只有一个秒数。
        let scheduler_min_raw = j.get("scheduler")
            .and_then(|s| s.get("idle_sleep_minutes"))
            .and_then(Json::as_str).map(str::to_string);
        let idle_min_raw = engine_get("idle_sleep_minutes").or(scheduler_min_raw);
        let engine_idle_sleep_secs = match (idle_secs_raw, idle_min_raw) {
            (Some(s), _) => {
                let n = parse_idle_secs("idle_sleep_secs", s)?;
                if n == 0 { None } else { Some(n) }
            }
            (None, Some(m)) => {
                let n = parse_idle_secs("idle_sleep_minutes", m)?;
                if n == 0 { None } else {
                    Some(n.checked_mul(60).ok_or_else(||
                        "engine.idle_sleep_minutes 折算成秒时溢出".to_string())?)
                }
            }
            (None, None) => None,
        };
        // FR-11 握手拦截：opt-in。engine_get 只取字符串，布尔键单独读——
        // 写成 `"false"` 字符串不生效（不是合法布尔），保持「非法即默认关」。
        let bt_guard_handshake = j.get("engine")
            .and_then(|e| e.get("guard_handshake"))
            .and_then(Json::as_bool)
            .unwrap_or(false);
        let role = match j.get("role").and_then(Json::as_str) {
            None => None,
            Some(s) => {
                let r = s.trim().to_ascii_lowercase();
                if r != "holder" && r != "member" {
                    return Err(format!("role 非法（只能是 holder 或 member）: {s}"));
                }
                Some(r)
            }
        };
        // 新增的运维开关统一按「非负整数，0 = 关闭」解析：数字与字符串两种写法都收，
        // 非法值直接报错（静默夹到默认会让「以为开了限流其实没开」）。
        let opt_num = |section: &str, key: &str| -> Result<Option<u64>, String> {
            let v = match j.get(section).and_then(|s| s.get(key)) {
                None => return Ok(None),
                Some(v) => v,
            };
            let n = if let Some(n) = v.as_i64() {
                n as u64
            } else if let Some(s) = v.as_str() {
                s.trim().parse::<u64>()
                    .map_err(|e| format!("{section}.{key} 非法（需要非负整数）: {s}: {e}"))?
            } else {
                return Err(format!("{section}.{key} 非法（需要整数）"));
            };
            Ok(if n == 0 { None } else { Some(n) })
        };
        let api_rate_per_sec = opt_num("api", "rate_per_sec")?
            .map(|v| u32::try_from(v).map_err(|_| "api.rate_per_sec 过大".to_string()))
            .transpose()?;
        let api_rate_burst = opt_num("api", "rate_burst")?
            .map(|v| u32::try_from(v).map_err(|_| "api.rate_burst 过大".to_string()))
            .transpose()?;
        let indexer_watch_secs = opt_num("indexer", "watch_secs")?;
        let kad_republish_secs = opt_num("kad", "republish_secs")?;
        // FR-15：阈值。0/缺省 = 关闭；缺省时既有部署的准入行为完全不变。
        // 注意不能把 0 夹成 1——那会让「没配」表现成「配了 1，一个都不许热」。
        let limits_max_hot_seeds = opt_num("limits", "max_hot_seeds")?.map(|n| n as usize);
        Ok(Config {
            node_id: String::new(),
            role,
            bind,
            nodes,
            sample_path: PathBuf::from(get_s("sample_path")?),
            samples,
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
            nat_relay_udps,
            nat_data_plane_carrier,
            nat_bt_external,
            nat_relay_rendezvous,
            nat_http_external,
            nat_utp,
            nat_punch_route,
            engine_dir,
            engine_listen,
            engine_max_active_seeds,
            engine_idle_sleep_secs,
            api_rate_per_sec,
            api_rate_burst,
            indexer_watch_secs,
            kad_republish_secs,
            limits_max_hot_seeds,
            bt_guard_handshake,
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

    /// v2.4.92：中继会合地址只接受文档网段，且必须与 punch_route 同时开启。
    ///
    /// 两个拒绝都是刻意的：把它写成真实 IP 会让 tracker/声明把它读成公网入向，
    /// 而那条路只有本地桥能走；没有桥就通告出去则是对端永远连不上的假 peer。
    #[test]
    fn relay_rendezvous_requires_doc_range_and_punch_route() {
        let mk = |extra: &str| -> String {
            format!(r#"{{
                "bind": "127.0.0.1",
                "nodes": {{
                    "A": {{"http_port": 16800, "wake_udp_port": 16801, "bt_port": 16802}},
                    "B": {{"http_port": 16900, "wake_udp_port": 16901, "bt_port": 16902}}
                }},
                "sample_path": "s.bin", "keywords": [], "workdir": ".run",
                "golden_req": "r.json", "golden_ack": "a.json",
                "nat": {{"enabled": true, "relay": "127.0.0.1:7000"{extra}}}
            }}"#)
        };
        let p = std::env::temp_dir().join("cr_core_rendezvous.json");
        let load = |s: String| Config::load({
            fs::write(&p, s).unwrap(); &p
        });

        let ok = load(mk(r#", "punch_route": true, "relay_rendezvous": "203.0.113.31""#)).unwrap();
        assert_eq!(ok.nat_relay_rendezvous.as_deref(), Some("203.0.113.31"));
        assert!(ok.nat_punch_route);

        // 真实公网 IP 必须被拒：它会变成「看起来有公网入向」的假证据。
        let e = load(mk(r#", "punch_route": true, "relay_rendezvous": "82.157.68.159""#))
            .expect_err("真实 IP 不得作为会合地址");
        assert!(e.contains("RFC 5737"), "{}", e);

        // 私网地址同样被拒（不在文档网段里，且不可达）。
        let e = load(mk(r#", "punch_route": true, "relay_rendezvous": "192.168.1.210""#))
            .expect_err("私网 IP 不得作为会合地址");
        assert!(e.contains("RFC 5737"), "{}", e);

        // 没有本地桥就没有数据面，通告出去是假 peer。
        let e = load(mk(r#", "relay_rendezvous": "203.0.113.31""#))
            .expect_err("未开 punch_route 时不得配置会合地址");
        assert!(e.contains("punch_route"), "{}", e);

        let e = load(mk(r#", "punch_route": true, "relay_rendezvous": "not-an-ip""#))
            .expect_err("非 IPv4 必须报错");
        assert!(e.contains("IPv4"), "{}", e);
        let _ = fs::remove_file(&p);
    }

    /// W3.8：`nat.relay` 支持列表；单值必须与旧行为逐字节等价，非法项必须被丢弃。
    #[test]
    fn relay_list_is_parsed_and_junk_is_dropped() {        let mk = |relay: &str| -> String {
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

    /// v2.4.86：`nat.relay_udp` 走同一套校验，但与 `relay` **互不影响**——
    /// 它们是同一个中继的两种承载，允许只配 UDP（中继的入向 TCP 被掐时就是这种配法）。
    #[test]
    fn relay_udp_endpoints_are_parsed_independently() {
        let mk = |relay: &str, udp: &str| -> String {
            format!(r#"{{
                "bind": "127.0.0.1",
                "nodes": {{
                    "A": {{"http_port": 16800, "wake_udp_port": 16801, "bt_port": 16802}},
                    "B": {{"http_port": 16900, "wake_udp_port": 16901, "bt_port": 16902}}
                }},
                "sample_path": "s.bin", "keywords": [], "workdir": ".run",
                "golden_req": "r.json", "golden_ack": "a.json",
                "nat": {{"enabled": true, "relay": "{}", "relay_udp": "{}"}}
            }}"#, relay, udp)
        };
        // 只配 UDP（TCP 侧被掐的中继）：relay 为空、UDP 列表照常生效
        let p = std::env::temp_dir().join("cr_core_relay_udp_only.json");
        fs::write(&p, mk("", "119.29.150.254:7101")).unwrap();
        let cfg = Config::load(&p).unwrap();
        assert!(cfg.nat_relays.is_empty(), "没配 relay 就不能凭空造 TCP 端点");
        assert!(cfg.nat_relay.is_none());
        assert_eq!(cfg.nat_relay_udps, vec!["119.29.150.254:7101".to_string()]);
        let _ = fs::remove_file(&p);

        // 两种承载同时配：各自解析，互不吞并
        let p = std::env::temp_dir().join("cr_core_relay_udp_both.json");
        fs::write(&p, mk("r.example:7100", "u.example:7101, v.example:7102")).unwrap();
        let cfg = Config::load(&p).unwrap();
        assert_eq!(cfg.nat_relays, vec!["r.example:7100".to_string()]);
        assert_eq!(cfg.nat_relay_udps, vec!["u.example:7101".to_string(),
                                            "v.example:7102".to_string()]);
        let _ = fs::remove_file(&p);

        // 非法项丢弃，不造占位端点
        let p = std::env::temp_dir().join("cr_core_relay_udp_junk.json");
        fs::write(&p, mk("", "nohost, w:0x, good:7101")).unwrap();
        let cfg = Config::load(&p).unwrap();
        assert_eq!(cfg.nat_relay_udps, vec!["good:7101".to_string()]);
        let _ = fs::remove_file(&p);
    }

    /// 旧配置（没有 `relay_udp` 键）必须得到空列表，行为逐字节不变。
    #[test]
    fn missing_relay_udp_key_means_empty() {
        let text = r#"{
            "bind": "127.0.0.1",
            "nodes": {
                "A": {"http_port": 16800, "wake_udp_port": 16801, "bt_port": 16802},
                "B": {"http_port": 16900, "wake_udp_port": 16901, "bt_port": 16902}
            },
            "sample_path": "s.bin", "keywords": [], "workdir": ".run",
            "golden_req": "r.json", "golden_ack": "a.json",
            "nat": {"enabled": true, "relay": "127.0.0.1:7100"}
        }"#;
        let p = std::env::temp_dir().join("cr_core_relay_udp_absent.json");
        fs::write(&p, text).unwrap();
        let cfg = Config::load(&p).unwrap();
        assert_eq!(cfg.nat_relays, vec!["127.0.0.1:7100".to_string()]);
        assert!(cfg.nat_relay_udps.is_empty());
        let _ = fs::remove_file(&p);
    }

    /// ADR-024 阶段 2：数据面承载 opt-in。缺省/非法 → tcp（既有行为逐字节不变），
    /// 仅显式 udp → udp。大小写不敏感。
    #[test]
    fn data_plane_carrier_is_opt_in_tcp_by_default() {
        let base = |carrier: &str| -> String {
            format!(r#"{{
                "bind": "127.0.0.1",
                "nodes": {{
                    "A": {{"http_port": 16810, "wake_udp_port": 16811, "bt_port": 16812}},
                    "B": {{"http_port": 16910, "wake_udp_port": 16911, "bt_port": 16912}}
                }},
                "sample_path": "s.bin", "keywords": [], "workdir": ".run",
                "golden_req": "r.json", "golden_ack": "a.json",
                "nat": {{"enabled": true, "relay_udp": "127.0.0.1:7101"{} }}
            }}"#, carrier)
        };
        let p = std::env::temp_dir().join("cr_core_dp_carrier.json");
        // 缺省 → tcp
        fs::write(&p, base("")).unwrap();
        assert_eq!(Config::load(&p).unwrap().nat_data_plane_carrier, "tcp");
        // 显式 udp（大小写不敏感）
        fs::write(&p, base(r#", "data_plane_carrier": "UDP""#)).unwrap();
        assert_eq!(Config::load(&p).unwrap().nat_data_plane_carrier, "udp");
        // 显式 tcp
        fs::write(&p, base(r#", "data_plane_carrier": "tcp""#)).unwrap();
        assert_eq!(Config::load(&p).unwrap().nat_data_plane_carrier, "tcp");
        // 非法值 → 回落 tcp（不静默当成 udp）
        fs::write(&p, base(r#", "data_plane_carrier": "quic""#)).unwrap();
        assert_eq!(Config::load(&p).unwrap().nat_data_plane_carrier, "tcp");
        let _ = fs::remove_file(&p);
    }

    /// FR-11：握手拦截默认关闭；只有 engine.guard_handshake 显式 true 才开。
    /// 字符串 "true" 或非布尔不得意外开启（opt-in 硬约束）。
    #[test]
    fn guard_handshake_is_opt_in_and_never_truthy_by_accident() {
        let base = r#"{
            "bind": "127.0.0.1",
            "nodes": {"A": {"host": "127.0.0.1", "http_port": 1, "wake_udp_port": 2, "bt_port": 3}},
            "sample_path": "s.bin", "keywords": [], "workdir": ".run",
            "golden_req": "r.json", "golden_ack": "a.json",
            "engine": {ENGINE}
        }"#;
        let mk = |engine: &str| {
            let txt = base.replace("{ENGINE}", engine);
            let p = std::env::temp_dir().join("cr_core_guard_optin.json");
            fs::write(&p, txt).unwrap();
            let v = Config::load(&p).unwrap().bt_guard_handshake;
            let _ = fs::remove_file(&p);
            v
        };
        assert!(!mk("{}"), "缺省必须关闭");
        assert!(!mk(r#"{"guard_handshake": false}"#));
        assert!(mk(r#"{"guard_handshake": true}"#));
        assert!(!mk(r#"{"guard_handshake": "true"}"#), "字符串不得被当成布尔 true");
    }

    /// FR-14：空闲休眠默认关闭；秒与分钟都按整数解析，0 是显式关闭。
    #[test]
    fn idle_sleep_is_opt_in_and_units_are_explicit() {
        let base = r#"{
            "bind": "127.0.0.1",
            "nodes": {"A": {"host": "127.0.0.1", "http_port": 1, "wake_udp_port": 2, "bt_port": 3}},
            "sample_path": "s.bin", "keywords": [], "workdir": ".run",
            "golden_req": "r.json", "golden_ack": "a.json",
            "engine": {ENGINE}
        }"#;
        let mk = |engine: &str| {
            let txt = base.replace("{ENGINE}", engine);
            let p = std::env::temp_dir().join("cr_core_idle_sleep.json");
            fs::write(&p, txt).unwrap();
            let v = Config::load(&p).unwrap().engine_idle_sleep_secs;
            let _ = fs::remove_file(&p);
            v
        };
        assert_eq!(mk("{}"), None);
        assert_eq!(mk(r#"{"idle_sleep_secs": "0"}"#), None);
        assert_eq!(mk(r#"{"idle_sleep_secs": "7"}"#), Some(7));
        assert_eq!(mk(r#"{"idle_sleep_minutes": "2"}"#), Some(120));
        assert_eq!(mk(r#"{"idle_sleep_secs": "3", "idle_sleep_minutes": "9"}"#), Some(3));
    }

    /// ADR-022：punch_route 默认关闭，显式 true 才开；非布尔不得被当成 true。
    #[test]
    fn punch_route_is_opt_in_and_never_truthy_by_accident() {
        let base = r#"{
            "bind": "127.0.0.1",
            "nodes": {
                "A": {"http_port": 16800, "wake_udp_port": 16801, "bt_port": 16802},
                "B": {"http_port": 16900, "wake_udp_port": 16901, "bt_port": 16902}
            },
            "sample_path": "s.bin", "keywords": [], "workdir": ".run",
            "golden_req": "r.json", "golden_ack": "a.json"
        }"#;
        let absent = std::env::temp_dir().join("cr_core_punch_route_absent.json");
        fs::write(&absent, base).unwrap();
        assert!(!Config::load(&absent).unwrap().nat_punch_route, "缺省必须关闭");
        let _ = fs::remove_file(&absent);

        for (tag, value, expected) in [
            ("on", "true", true),
            ("off", "false", false),
            ("string", "\"true\"", false),
            ("number", "1", false),
        ] {
            let p = std::env::temp_dir().join(format!("cr_core_punch_route_{tag}.json"));
            let text = base.replace(
                "\"golden_ack\": \"a.json\"",
                &format!("\"golden_ack\": \"a.json\", \"nat\": {{\"punch_route\": {value}}}"),
            );
            fs::write(&p, text).unwrap();
            assert_eq!(Config::load(&p).unwrap().nat_punch_route, expected, "tag={tag}");
            let _ = fs::remove_file(&p);
        }
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

    #[test]
    fn role_is_optional_but_never_silently_misspelled() {
        let base = r#"{
            "bind": "0.0.0.0",
            "nodes": {"222": {"host": "112.5.32.34", "http_port": 17201,
                              "wake_udp_port": 17202, "bt_port": 17200}},
            "sample_path": "s.bin", "keywords": [], "workdir": ".run",
            "golden_req": "r.json", "golden_ack": "a.json"
        }"#;
        let p = std::env::temp_dir().join("cr_core_role_test.json");
        fs::write(&p, base).unwrap();
        // 缺省必须继续走旧的 node_id 推断，老配置逐字节不变。
        assert_eq!(Config::load(&p).unwrap().role, None);

        fs::write(&p, base.replace("\"bind\"", "\"role\": \"holder\", \"bind\"")).unwrap();
        assert_eq!(Config::load(&p).unwrap().role.as_deref(), Some("holder"));
        fs::write(&p, base.replace("\"bind\"", "\"role\": \"MEMBER\", \"bind\"")).unwrap();
        assert_eq!(Config::load(&p).unwrap().role.as_deref(), Some("member"));

        // 拼错角色时启动即失败，不能静默落进 member（= 不供片却假装在做种）。
        fs::write(&p, base.replace("\"bind\"", "\"role\": \"holderr\", \"bind\"")).unwrap();
        assert!(Config::load(&p).is_err());
        let _ = fs::remove_file(&p);
    }
}
