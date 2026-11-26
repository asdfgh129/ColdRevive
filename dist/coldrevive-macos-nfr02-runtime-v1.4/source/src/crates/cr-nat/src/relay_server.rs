//! W3.4 生产中继服务端 + W3.8 去单点化（std-only）。
//!
//! W3.3 的 `relay.rs` 只实现客户端帧协议，T2 验收靠本地 mock 桥「服务端自己发 UDP」
//! 才跑通中继回退——这不是可部署的中继。本模块提供一个**与业务无关**的真实帧路由器：
//!
//! - `REGISTER` 把 16 字节节点 id 绑定到当前 TCP 连接，回 `ACK`；
//! - `RELAY` 从负载前 16 字节取目标 id，在注册表里找到目标连接后封装成
//!   `DELIVERED`（`node` = **发送方**注册 id，负载 = 原始消息）投递；
//! - 目标未注册 / 写入失败时**只计 dropped，绝不伪造任何应答帧**，让调用侧如实超时；
//! - 服务端不解析唤醒 JSON、不碰 UDP、不知道 ColdRevive 业务协议。
//!
//! 持有者侧订阅线程（仓库 `coldrevived/src/relay_sub.rs`）负责把 `DELIVERED` 的负载经
//! **真实 UDP** 送进本守护进程的唤醒监听端口，再把**真实应答**用 `RELAY` 发回来源 id。
//! 因此 WAKEUP_ACK 始终由真实守护进程产生，服务端不做任何业务模拟。
//!
//! # W3.8：多实例去单点化
//!
//! W3.7 的双实例基线证明「中继挂掉就退化回只有全锥能通」的缺口**不是网络，是状态**：
//! 每个实例的注册表是私有的，跨实例的 `RELAY` 到了对端只能 `dropped`。现在补三条实例间帧
//! （见 `relay.rs`）：
//!
//! 1. `PEER_HELLO` / `ACK`：实例互相认身份（16 字节实例 id），一条 TCP 全双工，两个方向都能发；
//! 2. `PEER_SYNC`：把**本地注册表整份快照**推给对端（覆盖语义）。注册/取消注册时立即推一次，
//!    另有 `SYNC_INTERVAL` 周期兜底，因此「链路晚于注册建立」也能自愈；
//! 3. `FORWARD`（原始来源 id + 目标 id + 消息）：目标不在本地时，按快照里的归属转发给同侪，
//!    对端**只做本地投递、不再转发**——最多一跳，不存在环路。
//!
//! # W3.9：可放到公网
//!
//! 上面所有帧都可以跑在 [`crate::secure`] 的记录层里：配了 PSK 就必须先做双向握手，
//! 之后每条记录独立加密 + HMAC，序号严格递增（重放/乱序直接断链）。另外补了三件公网
//! 必须有的东西：
//!
//! 1. **连接数上限**（`--max-conns`）：超出直接关，不再无限起线程；
//! 2. **每连接速率上限**（`--rate`，令牌桶）：刷帧的连接计 `rate_limited` 后断开；
//! 3. **空闲超时**（`--idle-timeout`）：超时先发 `PING`，连续两轮无应答才断——订阅连接
//!    不需要业务保活，也不会被误杀；注册表另有 `--max-nodes` 上限。
//!
//! 未配 PSK 时是明文，只允许绑回环（策略在二进制里，见 `coldrevive-relay`）。
//!
//! 诚实边界：`FORWARD` 是一次性尽力投递，链路断在半路就如实计 `forward_dropped`，不做重传、
//! 不做多副本；多实例只解决「单实例宕机后注册表丢失」，不等于任播/共识，也不是 S6 的四类
//! NAT 实测。`S6.status` 仍为 `not_run`，不得据此宣称 G2 达标。

use crate::reach::ReachClass;
use crate::relay::{
    decode_reach_record, encode_reach_record, FrameKind, RelayFrame, NODE_ID_LEN,
    REACH_RECORD_LEN,
};
use crate::secure::{self, RecvError, SecureReader, SecureStats, SecureWriter, PSK_LEN};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// 帧头 = 1 字节 kind + 16 字节 node id + 4 字节大端长度。
pub const HEADER_LEN: usize = 1 + NODE_ID_LEN + 4;
/// 单帧最大负载（1 MiB），防止恶意长度字段导致内存耗尽。
pub const DEFAULT_MAX_FRAME: usize = 1 << 20;
/// 不超过这个负载长度的帧视为**信令**：唤醒请求/应答、注册、声明查询都是几百字节，
/// 它们绕过数据字节预算，只受每连接帧速率约束。中继是所有唤醒的信令前置依赖，
/// 不能因为数据面拥塞把信令一起掐掉。超过阈值的帧才消耗数据预算（典型数据分片 ≥16 KiB）。
pub const SIGNAL_FRAME_MAX: usize = 4096;
/// 注册表快照周期。事件驱动（注册时）已推一次，这里只做漏包/漏链的兜底。
pub const SYNC_INTERVAL: Duration = Duration::from_millis(500);
/// 同侪链路断开后的重拨间隔。
pub const PEER_RETRY: Duration = Duration::from_secs(1);
/// 到同侪 / 订阅连接的写超时：避免一个卡死的对端把写锁永久占住。
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// 握手必须在这么久内完成，否则按 slowloris 断开。
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// 公网运行的闸门：连接数、每连接帧速率、空闲回收、注册表规模，
/// 以及 W3.13 新增的**全局数据字节预算**。
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// 每连接每秒允许的帧数（令牌桶速率）
    pub rate_per_sec: u32,
    /// 令牌桶容量（允许的突发帧数）
    pub burst: u32,
    /// 空闲多久先发 PING（再一轮无应答才断）；`Duration::ZERO` = 不判空闲
    pub idle: Duration,
    /// 并发连接上限
    pub max_conns: usize,
    /// 注册表上限（本实例声明的节点数）
    pub max_nodes: usize,
    /// 握手必须在此时间内完成（slowloris 防护）
    pub handshake_timeout: Duration,
    /// **本实例**每秒允许转发的数据字节数（全局令牌桶，不是每连接）。
    /// 这是部署侧的带宽护栏，不是可达率承诺；按中继机器实际带宽用 `--data-rate` 调。
    pub data_byte_rate: u64,
    /// 数据字节令牌桶容量（允许的突发字节数）。
    pub data_burst_bytes: u64,
    /// **单个来源 IP** 的并发连接上限。
    ///
    /// 与 `max_conns` 的区别：后者是整机的总量护栏，挡不住「一台机器占满所有名额」。
    /// 取值必须留足 NAT 共享的余量——一个家庭/一间办公室通常只有一个出口 IP，
    /// 几十台设备是正常形态；默认 64 只挡「单机开几百条」这种量级。
    pub max_conns_per_ip: usize,
    /// **单个来源 IP** 每分钟允许的注册次数（`REGISTER` 与 `REGISTER_DECLARED` 合计）。
    ///
    /// 注册会写全局注册表并触发同侪快照广播，是最贵的路径。正常客户端只在启动/
    /// 重连时注册几次；用 240 留足重连抖动，同时把「狂刷注册把快照打爆」封住。
    pub register_per_ip_per_min: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            rate_per_sec: 200,
            burst: 400,
            idle: Duration::from_secs(300),
            max_conns: 512,
            max_nodes: 8192,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            // 512 KiB/s ≈ 4 Mbps，给一台小 VPS 留出信令与系统余量；突发 4 秒。
            data_byte_rate: 512 * 1024,
            data_burst_bytes: 2 * 1024 * 1024,
            max_conns_per_ip: 64,
            register_per_ip_per_min: 240,
        }
    }
}

/// 一个 IPv4 网段（`10.0.0.0/8`，或单个地址 `1.2.3.4`）。
///
/// 只做**黑名单**用途，所以只需要「给不给进」这一个判断，不实现合并/最长前缀匹配。
/// IPv6 目前不支持：本项目的公网节点与实验环境都是 IPv4，静默接受一个 v6 规则却
/// 永远匹配不上，比明确报错更糟。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpNet {
    base: [u8; 4],
    prefix: u8,
}

impl IpNet {
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let (addr, prefix) = match text.split_once('/') {
            Some((a, p)) => {
                let n: u8 = p.parse().map_err(|e| format!("前缀 {p} 非法: {e}"))?;
                (a, n)
            }
            None => (text, 32),
        };
        if prefix > 32 {
            return Err(format!("IPv4 前缀长度 {prefix} 超过 32"));
        }
        let v4: std::net::Ipv4Addr = addr.parse()
            .map_err(|e| format!("{addr} 不是 IPv4 地址（黑名单暂只支持 IPv4）: {e}"))?;
        let raw = u32::from(v4);
        let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
        Ok(Self { base: (raw & mask).to_be_bytes(), prefix })
    }

    /// 规范化文本（`base/prefix`）。用于运维核对，不用于再解析。
    pub fn to_text(&self) -> String {
        format!("{}.{}.{}.{}/{}", self.base[0], self.base[1], self.base[2], self.base[3], self.prefix)
    }

    /// 是否覆盖全部 IPv4（`0.0.0.0/0`）。这种配置会把中继变成「谁都不服务」，
    /// 启动时值得显式提醒一句。
    pub fn is_all_ipv4(&self) -> bool {
        self.prefix == 0
    }

    pub fn contains(&self, ip: std::net::IpAddr) -> bool {
        let std::net::IpAddr::V4(v4) = ip else { return false };
        let raw = u32::from(v4);
        let mask = if self.prefix == 0 { 0 } else { u32::MAX << (32 - self.prefix) };
        (raw & mask).to_be_bytes() == self.base
    }
}

/// 单个来源 IP 的连接与注册计数。
#[derive(Debug, Clone, Copy)]
struct IpEntry {
    active: u32,
    /// 注册统计窗口起点（unix 毫秒）。
    window_start_ms: u64,
    registrations: u32,
}

/// 注册表分片数。
///
/// 单把锁在 `max_nodes = 8192` 时会成为热点：每帧 RELAY 都要查一次目标，
/// 每次注册都要写一次，而投递是高频路径。按节点 ID 首字节分 16 片后，
/// 查询与写入的锁竞争降到 1/16，且**不改变任何可见语义**（同一 ID 永远落同一片）。
const REGISTRY_SHARDS: usize = 16;

/// 分片注册表：`node id → 写连接`。
struct Registry {
    shards: Vec<Mutex<HashMap<[u8; NODE_ID_LEN], Arc<Mutex<ChanWriter>>>>>,
    /// 全局条目数。分片后无法便宜地跨片计数，用原子计数器维护；
    /// 它在「替换已有 ID」时不变，在「新增/移除」时增减，因此可以作为 `max_nodes` 的准确判据。
    len: AtomicU64,
}

impl Registry {
    fn new() -> Self {
        Self {
            shards: (0..REGISTRY_SHARDS).map(|_| Mutex::new(HashMap::new())).collect(),
            len: AtomicU64::new(0),
        }
    }

    fn shard_of(&self, id: &[u8; NODE_ID_LEN]) -> &Mutex<HashMap<[u8; NODE_ID_LEN], Arc<Mutex<ChanWriter>>>> {
        &self.shards[(id[0] as usize) % REGISTRY_SHARDS]
    }

    fn get(&self, id: &[u8; NODE_ID_LEN]) -> Option<Arc<Mutex<ChanWriter>>> {
        self.shard_of(id).lock().unwrap().get(id).cloned()
    }

    fn contains(&self, id: &[u8; NODE_ID_LEN]) -> bool {
        self.shard_of(id).lock().unwrap().contains_key(id)
    }

    /// 插入或替换。`max` 是全局上限：**替换已有条目**永远允许（否则重连会失败）。
    fn insert(&self, id: [u8; NODE_ID_LEN], w: Arc<Mutex<ChanWriter>>, max: usize) -> bool {
        let mut shard = self.shard_of(&id).lock().unwrap();
        if !shard.contains_key(&id) && self.len.load(Ordering::SeqCst) as usize >= max {
            return false;
        }
        if shard.insert(id, w).is_none() {
            self.len.fetch_add(1, Ordering::SeqCst);
        }
        true
    }

    /// 仅当当前注册项仍是 `w` 时才移除（重连竞态的判据）。
    fn remove_if_same(&self, id: &[u8; NODE_ID_LEN], w: &Arc<Mutex<ChanWriter>>) -> bool {
        let mut shard = self.shard_of(id).lock().unwrap();
        let same = shard.get(id).map(|cur| Arc::ptr_eq(cur, w)).unwrap_or(false);
        if same {
            shard.remove(id);
            self.len.fetch_sub(1, Ordering::SeqCst);
        }
        same
    }

    fn len(&self) -> usize {
        self.len.load(Ordering::SeqCst) as usize
    }

    fn keys(&self) -> Vec<[u8; NODE_ID_LEN]> {
        let mut out = Vec::with_capacity(self.len());
        for shard in &self.shards {
            out.extend(shard.lock().unwrap().keys().copied());
        }
        out
    }
}

/// 写侧：明文（回环/测试）或加密记录层。
pub enum ChanWriter {
    Plain(TcpStream),
    /// 与读侧共享同一个 `SecureWriter`：读侧收到 `PING` 时要能立刻回 `PONG`。
    Secure(Arc<Mutex<SecureWriter>>),
}

impl ChanWriter {
    fn send(&mut self, frame: &RelayFrame) -> bool {
        match self {
            ChanWriter::Plain(s) => write_frame(s, frame).is_ok(),
            ChanWriter::Secure(w) => match w.lock() {
                Ok(mut g) => g.send_frame(frame).is_ok(),
                Err(_) => false,
            },
        }
    }
    /// 空闲探测。明文模式没有心跳，返回 true（不参与空闲判定）。
    fn ping(&mut self) -> bool {
        match self {
            ChanWriter::Plain(_) => true,
            ChanWriter::Secure(w) => match w.lock() {
                Ok(mut g) => g.send_ping().is_ok(),
                Err(_) => false,
            },
        }
    }
    fn shutdown(&self) {
        match self {
            ChanWriter::Plain(s) => {
                let _ = s.shutdown(Shutdown::Both);
            }
            ChanWriter::Secure(w) => {
                if let Ok(g) = w.lock() {
                    g.shutdown();
                }
            }
        }
    }
}

/// 读侧：明文或加密记录层，统一成 `recv()`。
pub enum ChanReader {
    Plain { stream: TcpStream, max_frame: usize },
    Secure(SecureReader),
}

impl ChanReader {
    fn recv(&mut self) -> Result<RelayFrame, RecvError> {
        match self {
            ChanReader::Plain { stream, max_frame } => {
                read_frame(stream, *max_frame).map_err(RecvError::Protocol)
            }
            ChanReader::Secure(r) => r.recv(),
        }
    }
    fn set_read_timeout(&self, t: Option<Duration>) -> Result<(), String> {
        match self {
            ChanReader::Plain { stream, .. } => {
                stream.set_read_timeout(t).map_err(|e| e.to_string())
            }
            ChanReader::Secure(r) => r.set_read_timeout(t),
        }
    }
    fn records_in(&self) -> u64 {
        match self {
            ChanReader::Plain { .. } => 0,
            ChanReader::Secure(r) => r.counters().records_in.load(Ordering::Relaxed),
        }
    }
    fn stats(&self) -> Option<SecureStats> {
        match self {
            ChanReader::Plain { .. } => None,
            ChanReader::Secure(r) => Some(r.counters().snapshot()),
        }
    }
}

/// 每连接令牌桶。用「按时间补充」而不是「每秒清零」，避免窗口边界上放过 2 倍突发。
struct Bucket {
    tokens: f64,
    rate: f64,
    burst: f64,
    last: Instant,
}

impl Bucket {
    fn new(limits: &Limits) -> Self {
        let rate = limits.rate_per_sec.max(1) as f64;
        let burst = limits.burst.max(1) as f64;
        Self {
            tokens: burst,
            rate,
            burst,
            last: Instant::now(),
        }
    }
    fn allow(&mut self) -> bool {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + dt * self.rate).min(self.burst);
        if self.tokens < 1.0 {
            return false;
        }
        self.tokens -= 1.0;
        true
    }
}

/// 全局**数据字节**令牌桶。与 `Bucket` 分开：帧速率防刷帧，字节预算防占满中继带宽。
/// 一次性申请整帧长度，宁可丢一帧也不半途截流——半帧比没帧更难排查。
struct ByteBucket {
    tokens: f64,
    rate: f64,
    burst: f64,
    last: Instant,
}

impl ByteBucket {
    fn new(limits: &Limits) -> Self {
        let rate = limits.data_byte_rate.max(1) as f64;
        let burst = limits.data_burst_bytes.max(1) as f64;
        Self { tokens: burst, rate, burst, last: Instant::now() }
    }
    fn try_take(&mut self, bytes: usize) -> bool {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + dt * self.rate).min(self.burst);
        let n = bytes as f64;
        if self.tokens < n {
            return false;
        }
        self.tokens -= n;
        true
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelayStats {
    /// 累计接受的 TCP 连接数
    pub connections: u64,
    /// 成功的 REGISTER 次数
    pub registered: u64,
    /// 成功投递的 DELIVERED 帧数
    pub delivered: u64,
    /// 最终没能投出去的 RELAY 帧数（无任何归属 / 本地连接失效 / 同侪链路写失败）
    pub dropped: u64,
    /// 协议违规帧数（注册负载不一致、来源未注册、非法帧类型等）
    pub rejected: u64,
    /// 当前在线的同侪实例数（gauge）
    pub peers: u64,
    /// 从同侪快照里学到的「别人家的」节点数（gauge）
    pub peer_nodes: u64,
    /// 成功交给同侪转发的 RELAY 帧数
    pub forwarded: u64,
    /// 转发失败的帧数（写同侪失败 / 对端收到 FORWARD 但已无该目标）
    pub forward_dropped: u64,
    /// 安全握手成功次数
    pub secure_handshakes: u64,
    /// 安全握手失败 / 明文连到要求密钥的实例
    pub secure_rejected: u64,
    /// 记录 tag 校验失败（篡改或重放：序号被固定，重放必然过不了 tag）
    pub bad_tag: u64,
    /// 触发每连接速率上限而断开的连接数
    pub rate_limited: u64,
    /// 空闲超时回收的连接数
    pub idle_closed: u64,
    /// 因并发连接数上限被拒的连接数
    pub conn_cap_rejected: u64,
    /// 接受的可达性声明帧数（W3.12）。含同一节点重复声明。
    pub declared_updates: u64,
    /// 当前声明为「可直连」的节点数（gauge）。**是声明，不是实测**。
    pub declared_direct: u64,
    /// 当前声明为「只能中继」的节点数（gauge）。
    pub declared_relay_only: u64,
    /// 当前声明为「未知」或从未声明的节点数（gauge）。
    pub declared_unknown: u64,
    /// 查询可达性类别的次数（gauge 用途：判断这个能力到底有没有人用）
    pub reach_lookups: u64,
    /// 通过数据字节预算的帧负载总字节数（累计）。**是放行量，不是实际送达量**。
    pub data_bytes_admitted: u64,
    /// 因数据字节预算不足而未进入路由的帧数。与 `dropped` 分开记，便于区分
    /// 「找不到目标」和「中继带宽已满」——后者对用户应显示为「中继忙」而非「节点离线」。
    pub bandwidth_limited: u64,
    /// 被字节预算拒绝的帧负载字节数（累计）。
    pub bandwidth_limited_bytes: u64,
    /// 被黑名单（`--deny`）拒绝的连接数（握手前即断开）。
    pub blacklisted: u64,
    /// 因单 IP 并发连接上限被拒的连接数。
    pub ip_cap_rejected: u64,
    /// 因单 IP 注册频率上限被拒的注册次数（该连接随即断开）。
    pub ip_rate_limited: u64,
    /// 当前被跟踪的来源 IP 数（gauge，用于确认回收是否生效）。
    pub ip_tracked: u64,
}

/// 所有连接线程共享的状态。

impl Shared {
    /// 来源 IP 是否在黑名单里。
    fn ip_denied(&self, ip: std::net::IpAddr) -> bool {
        self.deny.iter().any(|n| n.contains(ip))
    }

    /// 登记一条新连接；超过单 IP 上限返回 `false`。
    fn ip_conn_open(&self, ip: std::net::IpAddr) -> bool {
        let limit = self.limits.max_conns_per_ip;
        let mut map = self.per_ip.lock().unwrap();
        let entry = map.entry(ip).or_insert(IpEntry {
            active: 0,
            window_start_ms: 0,
            registrations: 0,
        });
        if entry.active as usize >= limit {
            return false;
        }
        entry.active += 1;
        let tracked = map.len() as u64;
        drop(map);
        self.stat(|s| s.ip_tracked = tracked);
        true
    }

    /// 连接结束：减计数，并在「无活动连接 + 注册窗口过期」时回收条目。
    ///
    /// 不回收就会随扫描流量单调增长（每个源 IP 一条），这正是「小状态大泄漏」的典型形态。
    fn ip_conn_closed(&self, ip: std::net::IpAddr) {
        let now = unix_ms();
        let mut map = self.per_ip.lock().unwrap();
        let mut drop_it = false;
        if let Some(e) = map.get_mut(&ip) {
            e.active = e.active.saturating_sub(1);
            drop_it = e.active == 0 && now.saturating_sub(e.window_start_ms) >= REG_WINDOW_MS;
        }
        if drop_it {
            map.remove(&ip);
        }
        let tracked = map.len() as u64;
        drop(map);
        self.stat(|s| s.ip_tracked = tracked);
    }

    /// 单 IP 注册预算：一分钟窗口内不超过 `register_per_ip_per_min` 次。
    fn registration_allowed(&self, ip: Option<std::net::IpAddr>) -> bool {
        let Some(ip) = ip else { return true }; // 同侪链路不受客户端配额限制
        let limit = self.limits.register_per_ip_per_min;
        let now = unix_ms();
        let mut map = self.per_ip.lock().unwrap();
        let entry = map.entry(ip).or_insert(IpEntry {
            active: 0,
            window_start_ms: now,
            registrations: 0,
        });
        if now.saturating_sub(entry.window_start_ms) >= REG_WINDOW_MS {
            entry.window_start_ms = now;
            entry.registrations = 0;
        }
        if entry.registrations >= limit {
            return false;
        }
        entry.registrations += 1;
        true
    }
}

/// 注册配额窗口。
const REG_WINDOW_MS: u64 = 60_000;

fn unix_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// 单 IP 连接计数的守卫：连接线程无论怎么退出（正常 break / 提前 return / panic 展开），
/// 都必须把计数减回去，否则一次异常就会永久占掉一个名额。
struct IpConnGuard {
    shared: Arc<Shared>,
    ip: std::net::IpAddr,
}

impl Drop for IpConnGuard {
    fn drop(&mut self) {
        self.shared.ip_conn_closed(self.ip);
    }
}

struct Shared {
    instance: [u8; NODE_ID_LEN],
    /// 本实例自己的注册表：node id → 该客户端的写连接（按 ID 首字节分片）
    registry: Registry,
    /// 从同侪快照学到的归属：node id → 拥有它的实例 id
    peer_registry: Mutex<HashMap<[u8; NODE_ID_LEN], [u8; NODE_ID_LEN]>>,
    /// 同侪实例 id → 链路写连接
    peers: Mutex<HashMap<[u8; NODE_ID_LEN], Arc<Mutex<ChanWriter>>>>,
    /// 节点自报的可达性类别（W3.12）：node id → (类别, 声明时刻, 归属实例)。
    ///
    /// 中继在这里**只做存储与转发**，不做任何判断——它没有能力、也不该替客户端
    /// 认定对方自己的 NAT 行为。归属实例用于同侪快照的「整份覆盖」语义。
    declared: Mutex<HashMap<[u8; NODE_ID_LEN], (ReachClass, u64, [u8; NODE_ID_LEN])>>,
    stats: Mutex<RelayStats>,
    stop: Arc<AtomicBool>,
    max_frame: usize,
    /// `Some` = 必须双向握手；`None` = 明文（仅 `allow_insecure` 时放行）
    psk: Option<[u8; PSK_LEN]>,
    allow_insecure: bool,
    limits: Limits,
    active_conns: AtomicU64,
    /// 全实例共享的数据字节预算。信令帧不经过它。
    data_budget: Mutex<ByteBucket>,
    /// 拒绝接入的 IP 段（`--deny`）。空 = 不启用。
    deny: Vec<IpNet>,
    /// 每来源 IP 的连接/注册计数。条目在连接归零且窗口过期后回收。
    per_ip: Mutex<HashMap<std::net::IpAddr, IpEntry>>,
}

pub struct RelayServer {
    listener: TcpListener,
    instance: [u8; NODE_ID_LEN],
    peers: Vec<String>,
    max_frame: usize,
    psk: Option<[u8; PSK_LEN]>,
    allow_insecure: bool,
    limits: Limits,
    deny: Vec<IpNet>,
}

/// `spawn()` 返回的句柄；持有者可读取真实计数并请求停止所有循环。
#[derive(Clone)]
pub struct RelayHandle {
    shared: Arc<Shared>,
}

impl RelayHandle {
    pub fn stats(&self) -> RelayStats {
        self.shared.stats.lock().unwrap().clone()
    }
    /// 本实例 id（16 字节，`RLYINST` 前缀），便于日志与对端互认时核对。
    pub fn instance_id(&self) -> [u8; NODE_ID_LEN] {
        self.shared.instance
    }
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        // 叫醒阻塞在 read_exact 上的同侪链路与订阅连接：直接关闭写端。
        let links: Vec<Arc<Mutex<ChanWriter>>> = {
            let mut p = self.shared.peers.lock().unwrap();
            p.drain().map(|(_, w)| w).collect()
        };
        for w in links {
            if let Ok(g) = w.lock() {
                g.shutdown();
            }
        }
    }
    pub fn is_stopped(&self) -> bool {
        self.shared.stop.load(Ordering::Relaxed)
    }
}

impl Drop for RelayHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 从任意标签派生一个 16 字节实例 id，前缀 `RLYINST` 便于和节点 id 区分。
pub fn derive_instance_id(tag: &str) -> [u8; NODE_ID_LEN] {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in tag
        .as_bytes()
        .iter()
        .chain(nanos.to_be_bytes().iter())
        .chain(std::process::id().to_be_bytes().iter())
    {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    let mut out = [0u8; NODE_ID_LEN];
    out[..7].copy_from_slice(b"RLYINST");
    out[7..15].copy_from_slice(&h.to_be_bytes());
    out[15] = (nanos & 0xff) as u8;
    out
}

/// 把实例 id 转成**可打印标签**（日志与计数文件用）。
///
/// 派生 id 的 8 字节哈希里可能含 `0x00..0x1F`：直接塞进日志会打印乱码，塞进 `--stats-file`
/// 的 JSON 会变成**非法 JSON**（裸控制字符），让读取方每次都解析失败——T2 就因此报
/// 「中继服务端未就绪」而服务端其实好好的。协议身份仍用原始 16 字节，这里只管展示。
pub fn printable_instance(id: &[u8; NODE_ID_LEN]) -> String {
    let mut out = String::with_capacity(NODE_ID_LEN);
    for b in id {
        match b {
            // `parse_instance_id` 用 \0 右填充，尾部这些字节不是身份的一部分。
            0 => {}
            0x20..=0x7e => out.push(*b as char),
            _ => out.push('.'),
        }
    }
    out
}

/// 解析 `--instance-id`：接受 16 字节可打印标签，用 `\0` 右填充。
pub fn parse_instance_id(text: &str) -> Result<[u8; NODE_ID_LEN], String> {
    let raw = text.as_bytes();
    if raw.is_empty() || raw.len() > NODE_ID_LEN {
        return Err(format!("实例 id 需 1..={} 字节，收到 {}",
                           NODE_ID_LEN, raw.len()));
    }
    if !raw.iter().all(|b| b.is_ascii_graphic()) {
        return Err("实例 id 只接受可打印 ASCII".into());
    }
    let mut out = [0u8; NODE_ID_LEN];
    out[..raw.len()].copy_from_slice(raw);
    Ok(out)
}

impl Shared {
    fn stat<F: FnOnce(&mut RelayStats)>(&self, f: F) {
        f(&mut self.stats.lock().unwrap());
    }

    /// 数据帧入路由前申请字节额度。small frames（≤ `SIGNAL_FRAME_MAX`）不调用本函数，
    /// 因此信令永远不会因为数据面拥塞被挡住。拒绝时**不**增加 `dropped`：
    /// 那是「找不到目标」，与「中继忙」是两种完全不同的失败，不能混成一个数字。
    fn charge_data(&self, bytes: usize) -> bool {
        let allowed = self.data_budget.lock().unwrap().try_take(bytes);
        self.stat(|s| {
            if allowed {
                s.data_bytes_admitted += bytes as u64;
            } else {
                s.bandwidth_limited += 1;
                s.bandwidth_limited_bytes += bytes as u64;
            }
        });
        allowed
    }

    fn local_nodes(&self) -> Vec<[u8; NODE_ID_LEN]> {
        let mut v = self.registry.keys();
        v.sort();
        v
    }

    fn refresh_peer_gauges(&self) {
        let nodes = self.peer_registry.lock().unwrap().len();
        let links = self.peers.lock().unwrap().len();
        self.stat(|s| {
            s.peer_nodes = nodes as u64;
            s.peers = links as u64;
        });
    }

    /// 重新计算可达性声明的三个 gauge。
    ///
    /// 名字里带 `declared_` 前缀是刻意的：这三个数字是**客户端自报的声明**，
    /// 不是本实例观测到的事实。把它们读成「有 N 台机器可直连」就是误读。
    fn refresh_reach_gauges(&self) {
        let (mut d, mut r, mut u) = (0u64, 0u64, 0u64);
        {
            let decl = self.declared.lock().unwrap();
            for (_, (class, _, _)) in decl.iter() {
                match class {
                    ReachClass::Direct => d += 1,
                    ReachClass::RelayOnly => r += 1,
                    ReachClass::Unknown => u += 1,
                }
            }
            // 已注册但从未声明的节点（老客户端、或只用 REGISTER 的探测连接）计入 unknown。
            // 分片后 keys() 会逐片取锁：这里不持全局锁，避免与注册路径互相等待。
            for id in self.registry.keys() {
                if !decl.contains_key(&id) {
                    u += 1;
                }
            }
        }
        self.stat(|s| {
            s.declared_direct = d;
            s.declared_relay_only = r;
            s.declared_unknown = u;
        });
    }

    /// 记录一条本实例客户的声明。
    fn declare_local(&self, id: [u8; NODE_ID_LEN], class: ReachClass, at_ms: u64) {
        let instance = self.instance;
        {
            let mut decl = self.declared.lock().unwrap();
            decl.insert(id, (class, at_ms, instance));
        }
        self.stat(|s| s.declared_updates += 1);
        self.refresh_reach_gauges();
        if !self.peers.lock().unwrap().is_empty() {
            self.broadcast_reach();
        }
    }

    /// 节点断开时清掉**本实例**的声明；同侪的声明由同侪的快照负责。
    fn forget_local(&self, id: &[u8; NODE_ID_LEN]) {
        let instance = self.instance;
        let removed = {
            let mut decl = self.declared.lock().unwrap();
            match decl.get(id) {
                Some((_, _, owner)) if *owner == instance => decl.remove(id).is_some(),
                _ => false,
            }
        };
        if removed {
            self.refresh_reach_gauges();
            if !self.peers.lock().unwrap().is_empty() {
                self.broadcast_reach();
            }
        }
    }

    /// 把本实例的声明整份推给同侪（覆盖语义，掉线节点自然消失）。
    fn broadcast_reach(&self) {
        let instance = self.instance;
        let mut payload = Vec::new();
        {
            let decl = self.declared.lock().unwrap();
            let mut items: Vec<_> = decl
                .iter()
                .filter(|(_, (_, _, owner))| *owner == instance)
                .collect();
            items.sort_by(|a, b| a.0.cmp(b.0));
            for (id, (class, at, _)) in items {
                payload.extend_from_slice(&encode_reach_record(id, *class, *at));
            }
        }
        let frame = RelayFrame {
            kind: FrameKind::PeerReach,
            node: instance,
            payload,
        };
        let links: Vec<([u8; NODE_ID_LEN], Arc<Mutex<ChanWriter>>)> = self
            .peers
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        let mut dead = Vec::new();
        for (id, w) in links {
            if !send_frame(&w, &frame) {
                dead.push(id);
            }
        }
        for id in dead {
            self.drop_peer(&id);
        }
    }

    /// 收到同侪的声明快照：先清掉该同侪的旧声明，再整份写入。
    fn apply_peer_reach(&self, owner: [u8; NODE_ID_LEN], payload: &[u8]) {
        {
            let mut decl = self.declared.lock().unwrap();
            decl.retain(|_, (_, _, o)| *o != owner);
            let mut off = 0;
            while off + REACH_RECORD_LEN <= payload.len() {
                if let Some((id, class, at)) = decode_reach_record(&payload[off..]) {
                    decl.insert(id, (class, at, owner));
                }
                off += REACH_RECORD_LEN;
            }
        }
        self.refresh_reach_gauges();
    }

    /// 回答「这台机器属于哪一类」。查不到就明确回「不知道」（时刻为 0），不猜。
    fn answer_lookup(&self, target: &[u8; NODE_ID_LEN]) -> RelayFrame {
        let found = {
            let decl = self.declared.lock().unwrap();
            decl.get(target).map(|(c, at, _)| (*c, *at))
        };
        let payload = match found {
            Some((class, at)) => encode_reach_record(target, class, at).to_vec(),
            None => encode_reach_record(target, ReachClass::Unknown, 0).to_vec(),
        };
        self.stat(|s| s.reach_lookups += 1);
        RelayFrame {
            kind: FrameKind::ReachResult,
            node: self.instance,
            payload,
        }
    }

    fn install_peer(&self, id: [u8; NODE_ID_LEN], writer: Arc<Mutex<ChanWriter>>) -> bool {
        let mut p = self.peers.lock().unwrap();
        if p.contains_key(&id) {
            // 双方都配了 --peer 时会各拨一次；保留已有链路，避免来回抢。
            return false;
        }
        p.insert(id, writer);
        drop(p);
        self.refresh_peer_gauges();
        true
    }

    fn drop_peer(&self, id: &[u8; NODE_ID_LEN]) {
        self.peers.lock().unwrap().remove(id);
        {
            let mut pr = self.peer_registry.lock().unwrap();
            pr.retain(|_, owner| owner != id);
        }
        self.refresh_peer_gauges();
    }

    /// 把本地注册表整份快照推给所有同侪；写失败的同侪视为掉线并摘除。
    fn broadcast_snapshot(&self) {
        let nodes = self.local_nodes();
        let mut payload = Vec::with_capacity(nodes.len() * NODE_ID_LEN);
        for n in &nodes {
            payload.extend_from_slice(n);
        }
        let frame = RelayFrame {
            kind: FrameKind::PeerSync,
            node: self.instance,
            payload,
        };
        let links: Vec<([u8; NODE_ID_LEN], Arc<Mutex<ChanWriter>>)> = self
            .peers
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        let mut dead = Vec::new();
        for (id, w) in links {
            if !send_frame(&w, &frame) {
                dead.push(id);
            }
        }
        for id in dead {
            self.drop_peer(&id);
        }
    }

    /// 收到同侪快照：**整份覆盖**该同侪的归属集合，所以掉线节点会自然消失。
    fn apply_peer_sync(&self, owner: [u8; NODE_ID_LEN], nodes: &[[u8; NODE_ID_LEN]]) {
        {
            let mut pr = self.peer_registry.lock().unwrap();
            pr.retain(|_, o| *o != owner);
            for n in nodes {
                pr.insert(*n, owner);
            }
        }
        self.refresh_peer_gauges();
    }

    /// 往某条已注册连接写 DELIVERED。失败时摘除过期注册并返回 false。
    fn deliver(
        &self,
        target: &[u8; NODE_ID_LEN],
        source: [u8; NODE_ID_LEN],
        message: Vec<u8>,
    ) -> bool {
        let dest = self.registry.get(target);
        let Some(w) = dest else { return false };
        let frame = RelayFrame {
            kind: FrameKind::Delivered,
            node: source,
            payload: message,
        };
        if send_frame(&w, &frame) {
            return true;
        }
        self.registry.remove_if_same(target, &w);
        false
    }

    /// 本地优先 → 同侪归属转发（单跳）→ 都没有就如实 dropped。
    fn route_relay(
        &self,
        source: [u8; NODE_ID_LEN],
        target: [u8; NODE_ID_LEN],
        message: Vec<u8>,
    ) {
        if self.deliver(&target, source, message.clone()) {
            self.stat(|s| s.delivered += 1);
            return;
        }
        if self.registry.contains(&target) {
            // 本地有登记但写失败（deliver 已摘除），不转给别人，避免双投。
            self.stat(|s| s.dropped += 1);
            return;
        }
        let owner = self.peer_registry.lock().unwrap().get(&target).copied();
        if let Some(owner) = owner {
            let link = self.peers.lock().unwrap().get(&owner).cloned();
            if let Some(link) = link {
                let mut payload = Vec::with_capacity(2 * NODE_ID_LEN + message.len());
                payload.extend_from_slice(&source);
                payload.extend_from_slice(&target);
                payload.extend_from_slice(&message);
                let frame = RelayFrame {
                    kind: FrameKind::Forward,
                    node: self.instance,
                    payload,
                };
                if send_frame(&link, &frame) {
                    self.stat(|s| s.forwarded += 1);
                    return;
                }
            }
            self.drop_peer(&owner);
            self.stat(|s| s.forward_dropped += 1);
            self.stat(|s| s.dropped += 1);
            return;
        }
        self.stat(|s| s.dropped += 1);
    }
}

impl RelayServer {
    /// 在调用方线程绑定监听（返回立即可用的端口，避免测试里的 bind/accept 竞态）。
    pub fn bind(addr: &str) -> Result<Self, String> {
        let listener = TcpListener::bind(addr).map_err(|e| format!("relay bind {}: {}", addr, e))?;
        Ok(Self {
            listener,
            instance: derive_instance_id(addr),
            peers: Vec::new(),
            max_frame: DEFAULT_MAX_FRAME,
            psk: None,
            allow_insecure: true,
            limits: Limits::default(),
            deny: Vec::new(),
        })
    }

    /// 拒绝这些来源网段的接入（连接**在握手前**就被关掉，不浪费任何 CPU）。
    pub fn with_deny(mut self, deny: Vec<IpNet>) -> Self {
        self.deny = deny;
        self
    }

    /// 要求所有连接（含同侪链路）先做 PSK 握手。
    pub fn with_psk(mut self, psk: Option<[u8; PSK_LEN]>) -> Self {
        self.psk = psk;
        self
    }

    /// 是否允许明文连接。**二进制只在绑回环时置 true**，公网部署必须配 PSK。
    pub fn with_allow_insecure(mut self, allow: bool) -> Self {
        self.allow_insecure = allow;
        self
    }

    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    pub fn with_max_frame(mut self, max: usize) -> Self {
        self.max_frame = max.max(1);
        self
    }

    /// 指一个同侪实例（可多次调用）。启动后自动重拨，断线自动恢复。
    pub fn with_peer(mut self, addr: &str) -> Self {
        self.peers.push(addr.to_string());
        self
    }

    /// 固定实例 id（测试里需要可复现的对端身份时用）。
    pub fn with_instance_id(mut self, id: [u8; NODE_ID_LEN]) -> Self {
        self.instance = id;
        self
    }

    pub fn local_addr(&self) -> Result<SocketAddr, String> {
        self.listener.local_addr().map_err(|e| e.to_string())
    }

    pub fn spawn(self) -> RelayHandle {
        let RelayServer {
            listener,
            instance,
            peers,
            max_frame,
            psk,
            allow_insecure,
            limits,
            deny,
        } = self;
        let shared = Arc::new(Shared {
            instance,
            registry: Registry::new(),
            peer_registry: Mutex::new(HashMap::new()),
            peers: Mutex::new(HashMap::new()),
            declared: Mutex::new(HashMap::new()),
            stats: Mutex::new(RelayStats::default()),
            stop: Arc::new(AtomicBool::new(false)),
            max_frame,
            psk,
            allow_insecure,
            limits,
            active_conns: AtomicU64::new(0),
            data_budget: Mutex::new(ByteBucket::new(&limits)),
            deny,
            per_ip: Mutex::new(HashMap::new()),
        });
        let handle = RelayHandle {
            shared: shared.clone(),
        };

        // 1) accept 循环
        let acceptor = shared.clone();
        thread::spawn(move || accept_loop(listener, acceptor));

        // 2) 同侪拨号器（每个 --peer 一个线程）
        for addr in peers {
            let sh = shared.clone();
            thread::spawn(move || dial_loop(sh, addr));
        }

        // 3) 快照周期兜底（事件驱动之外的自愈路径）
        let ticker = shared.clone();
        thread::spawn(move || {
            while !ticker.stop.load(Ordering::Relaxed) {
                thread::sleep(SYNC_INTERVAL);
                if ticker.stop.load(Ordering::Relaxed) {
                    break;
                }
                if !ticker.peers.lock().unwrap().is_empty() {
                    ticker.broadcast_snapshot();
                    // 声明快照同样周期兜底：拨号方在 ACK 之后没人再推它，
                    // 只靠事件驱动会让「先声明、后建链」的那一侧永远同步不到。
                    ticker.broadcast_reach();
                }
            }
        });
        handle
    }
}

fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    let _ = listener.set_nonblocking(true);
    while !shared.stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, peer_addr)) => {
                // Windows 下 accept 出来的 socket 可能继承非阻塞标志，显式复位。
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(None);
                let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
                let _ = stream.set_nodelay(true);
                shared.stat(|s| s.connections += 1);
                // 黑名单与单 IP 上限都要在**起线程之前**判：让被拒的连接连握手都不做，
                // 否则「拒绝」本身就成了 CPU/内存放大器。
                if shared.ip_denied(peer_addr.ip()) {
                    shared.stat(|s| s.blacklisted += 1);
                    let _ = stream.shutdown(Shutdown::Both);
                    continue;
                }
                if !shared.ip_conn_open(peer_addr.ip()) {
                    shared.stat(|s| s.ip_cap_rejected += 1);
                    let _ = stream.shutdown(Shutdown::Both);
                    continue;
                }
                let sh = shared.clone();
                thread::spawn(move || conn_loop(stream, sh, None, None, Some(peer_addr.ip())));
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break,
        }
    }
}

/// 断开前先把拒绝原因发出去。
///
/// 发失败也照样断开：拒绝的优先级高于礼貌。但**能发就必须发**——客户端只看到
/// 「连接被关」时，无法区分「配额挡了」与「网络断了」，排查方向会完全跑偏。
fn reject_before_close(w: &Arc<Mutex<ChanWriter>>, reason: &str) {
    let frame = RelayFrame {
        kind: FrameKind::Reject,
        node: [0u8; NODE_ID_LEN],
        payload: reason.as_bytes().to_vec(),
    };
    let _ = send_frame(w, &frame);
}

fn send_frame(w: &Arc<Mutex<ChanWriter>>, frame: &RelayFrame) -> bool {
    match w.lock() {
        Ok(mut g) => g.send(frame),
        Err(_) => false,
    }
}

/// 单条**新接入**连接：先过闸（连接数 / 传输层握手），再进入统一的通道循环。
fn conn_loop(
    stream: TcpStream,
    shared: Arc<Shared>,
    registered: Option<[u8; NODE_ID_LEN]>,
    peer: Option<[u8; NODE_ID_LEN]>,
    ip: Option<std::net::IpAddr>,
) {
    // 0) 并发连接上限：先占位再判，超出立刻关，避免 accept 风暴无限起线程。
    let active = shared.active_conns.fetch_add(1, Ordering::SeqCst) + 1;
    if active > shared.limits.max_conns as u64 {
        shared.active_conns.fetch_sub(1, Ordering::SeqCst);
        shared.stat(|s| s.conn_cap_rejected += 1);
        let _ = stream.shutdown(Shutdown::Both);
        return;
    }
    let _guard = ConnGuard(shared.clone());
    // `ip` 只对**入向**连接有值（accept 路径）；出向的同侪链路不占客户端配额。
    let _ip_guard = ip.map(|ip| IpConnGuard { shared: shared.clone(), ip });

    // 1) 传输层：配了 PSK 就必须握手；没配只在 allow_insecure 时放行明文。
    let (reader, writer) = match shared.psk {
        Some(psk) => {
            let _ = stream.set_read_timeout(Some(shared.limits.handshake_timeout));
            match secure::server_handshake(stream, &psk, shared.max_frame) {
                Ok(ch) => {
                    shared.stat(|s| s.secure_handshakes += 1);
                    (
                        ChanReader::Secure(ch.reader),
                        Arc::new(Mutex::new(ChanWriter::Secure(ch.writer))),
                    )
                }
                Err(e) => {
                    shared.stat(|s| s.secure_rejected += 1);
                    eprintln!("[relay] 握手失败，已断开: {}", e);
                    return;
                }
            }
        }
        None => {
            if !shared.allow_insecure {
                shared.stat(|s| s.secure_rejected += 1);
                let _ = stream.shutdown(Shutdown::Both);
                return;
            }
            let r = match stream.try_clone() {
                Ok(r) => r,
                Err(_) => return,
            };
            (
                ChanReader::Plain {
                    stream: r,
                    max_frame: shared.max_frame,
                },
                Arc::new(Mutex::new(ChanWriter::Plain(stream))),
            )
        }
    };
    serve_chan(reader, writer, shared, registered, peer, ip);
}

struct ConnGuard(Arc<Shared>);

impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.0.active_conns.fetch_sub(1, Ordering::SeqCst);
    }
}

/// 已建立通道（明文或加密）后的统一循环。
///
/// `registered`：这条连接以哪个节点 id 注册（客户端）；`peer`：这条连接是同侪实例链路。
/// 两者互斥——同侪链路不允许发客户端 `RELAY`，否则来源身份会被实例 id 污染。
fn serve_chan(
    mut reader: ChanReader,
    writer: Arc<Mutex<ChanWriter>>,
    shared: Arc<Shared>,
    mut registered: Option<[u8; NODE_ID_LEN]>,
    mut peer: Option<[u8; NODE_ID_LEN]>,
    ip: Option<std::net::IpAddr>,
) {
    let secure_chan = matches!(reader, ChanReader::Secure(_));
    let idle = shared.limits.idle;
    let idle_enabled = secure_chan && !idle.is_zero();
    if idle_enabled {
        let _ = reader.set_read_timeout(Some(idle));
    }
    let mut missed_idle = 0u32;
    let mut bucket = Bucket::new(&shared.limits);

    loop {
        let before = reader.records_in();
        let frame = match reader.recv() {
            Ok(f) => {
                missed_idle = 0;
                f
            }
            Err(e) => {
                if !e.is_timeout() {
                    // 链路断开或协议违规（tag 失败/重放/非法记录）——如实计入统计后收尾。
                    if let Some(st) = reader.stats() {
                        shared.stat(|s| s.bad_tag += st.bad_tag);
                    }
                    break;
                }
                if !idle_enabled {
                    break;
                }
                if reader.records_in() > before {
                    // 超时期间其实收到了控制记录（PONG），链路还活着。
                    missed_idle = 0;
                    continue;
                }
                missed_idle += 1;
                if missed_idle >= 2 {
                    shared.stat(|s| s.idle_closed += 1);
                    break;
                }
                // 先发 PING 探活；发不出去就是链路已死。
                let alive = match writer.lock() {
                    Ok(mut g) => g.ping(),
                    Err(_) => false,
                };
                if !alive {
                    break;
                }
                continue;
            }
        };

        // 每连接速率上限：超了就断开，而不是无限吞帧把内存吃光。
        if !bucket.allow() {
            shared.stat(|s| s.rate_limited += 1);
            break;
        }

        match frame.kind {
            FrameKind::Register => {
                if peer.is_some() {
                    // 同侪链路不是客户端，不允许借它注册普通节点。
                    shared.stat(|s| s.rejected += 1);
                    break;
                }
                // 协议约定：REGISTER 负载必须回显头部里的 node id，防止伪造身份。
                if frame.payload.len() != NODE_ID_LEN || frame.payload.as_slice() != frame.node {
                    shared.stat(|s| s.rejected += 1);
                    break;
                }
                let id = frame.node;
                // 一条连接只认一个身份：改名会与快照归属冲突。
                if let Some(cur) = registered {
                    if cur != id {
                        shared.stat(|s| s.rejected += 1);
                        reject_before_close(&writer, "同一条连接不能改成另一个节点 ID");
                        break;
                    }
                }
                // 单 IP 注册频率：注册会写注册表并触发快照广播，是最贵的路径。
                // 超限即断开（不是「忽略这一帧」）：只忽略的话连接会一直挂着空转，
                // 反而比断开更占资源。
                if !shared.registration_allowed(ip) {
                    shared.stat(|s| s.ip_rate_limited += 1);
                    reject_before_close(&writer, "注册频率超过单 IP 配额，请退避后重试");
                    break;
                }
                if !shared.registry.insert(id, writer.clone(), shared.limits.max_nodes) {
                    shared.stat(|s| s.rejected += 1);
                    reject_before_close(&writer, "本实例注册表已满");
                    break;
                }
                registered = Some(id);
                let ack = RelayFrame {
                    kind: FrameKind::Ack,
                    node: [0u8; NODE_ID_LEN],
                    payload: Vec::new(),
                };
                if !send_frame(&writer, &ack) {
                    break;
                }
                shared.stat(|s| s.registered += 1);
                shared.refresh_reach_gauges();
                // 事件驱动同步：注册立刻推给同侪，跨实例投递不必等周期兜底。
                if !shared.peers.lock().unwrap().is_empty() {
                    shared.broadcast_snapshot();
                }
            }
            FrameKind::RegisterDeclared => {
                if peer.is_some() {
                    // 同侪链路不是客户端，不能借它声明普通节点。
                    shared.stat(|s| s.rejected += 1);
                    break;
                }
                if frame.payload.len() != REACH_RECORD_LEN {
                    shared.stat(|s| s.rejected += 1);
                    break;
                }
                let Some((id, class, at)) = decode_reach_record(&frame.payload) else {
                    shared.stat(|s| s.rejected += 1);
                    break;
                };
                // 与 REGISTER 同一条规矩：负载里的 id 必须回显帧头，防止替别人声明。
                if id != frame.node {
                    shared.stat(|s| s.rejected += 1);
                    break;
                }
                if let Some(cur) = registered {
                    if cur != id {
                        shared.stat(|s| s.rejected += 1);
                        reject_before_close(&writer, "同一条连接不能改成另一个节点 ID");
                        break;
                    }
                }
                // 单 IP 注册频率：注册会写注册表并触发快照广播，是最贵的路径。
                // 超限即断开（不是「忽略这一帧」）：只忽略的话连接会一直挂着空转，
                // 反而比断开更占资源。
                if !shared.registration_allowed(ip) {
                    shared.stat(|s| s.ip_rate_limited += 1);
                    reject_before_close(&writer, "注册频率超过单 IP 配额，请退避后重试");
                    break;
                }
                if !shared.registry.insert(id, writer.clone(), shared.limits.max_nodes) {
                    shared.stat(|s| s.rejected += 1);
                    reject_before_close(&writer, "本实例注册表已满");
                    break;
                }
                registered = Some(id);
                shared.declare_local(id, class, at);
                let ack = RelayFrame {
                    kind: FrameKind::Ack,
                    node: [0u8; NODE_ID_LEN],
                    payload: Vec::new(),
                };
                if !send_frame(&writer, &ack) {
                    break;
                }
                shared.stat(|s| s.registered += 1);
                if !shared.peers.lock().unwrap().is_empty() {
                    shared.broadcast_snapshot();
                }
            }
            FrameKind::PeerReach => {
                let Some(owner) = peer else {
                    // 声明快照只在实例之间流动，客户端发它属于违规（否则能替任意节点声明）。
                    shared.stat(|s| s.rejected += 1);
                    continue;
                };
                shared.apply_peer_reach(owner, &frame.payload);
            }
            FrameKind::LookupReach => {
                if registered.is_none() || peer.is_some() {
                    shared.stat(|s| s.rejected += 1);
                    continue;
                }
                if frame.payload.len() < NODE_ID_LEN {
                    shared.stat(|s| s.rejected += 1);
                    continue;
                }
                let mut target = [0u8; NODE_ID_LEN];
                target.copy_from_slice(&frame.payload[..NODE_ID_LEN]);
                let resp = shared.answer_lookup(&target);
                if !send_frame(&writer, &resp) {
                    break;
                }
            }
            FrameKind::Relay => {
                let Some(source) = registered else {
                    shared.stat(|s| s.rejected += 1);
                    continue;
                };
                if peer.is_some() {
                    shared.stat(|s| s.rejected += 1);
                    continue;
                }
                if frame.payload.len() < NODE_ID_LEN {
                    shared.stat(|s| s.rejected += 1);
                    continue;
                }
                let mut target = [0u8; NODE_ID_LEN];
                target.copy_from_slice(&frame.payload[..NODE_ID_LEN]);
                // 数据帧才计费；唤醒信令（几百字节）绕过预算，只受帧速率约束。
                if frame.payload.len() > SIGNAL_FRAME_MAX
                    && !shared.charge_data(frame.payload.len())
                {
                    continue;
                }
                let message = frame.payload[NODE_ID_LEN..].to_vec();
                shared.route_relay(source, target, message);
            }
            FrameKind::Forward => {
                let Some(_owner) = peer else {
                    shared.stat(|s| s.rejected += 1);
                    continue;
                };
                if frame.payload.len() < 2 * NODE_ID_LEN {
                    shared.stat(|s| s.rejected += 1);
                    continue;
                }
                let mut source = [0u8; NODE_ID_LEN];
                source.copy_from_slice(&frame.payload[..NODE_ID_LEN]);
                let mut target = [0u8; NODE_ID_LEN];
                target.copy_from_slice(&frame.payload[NODE_ID_LEN..2 * NODE_ID_LEN]);
                // 同侪转发来的数据帧同样要过本实例的预算（每个实例花自己的带宽）。
                if frame.payload.len() > SIGNAL_FRAME_MAX
                    && !shared.charge_data(frame.payload.len())
                {
                    continue;
                }
                let message = frame.payload[2 * NODE_ID_LEN..].to_vec();
                // 只做本地投递，**不再转发**：保证最多一跳，不会成环。
                if shared.deliver(&target, source, message) {
                    shared.stat(|s| s.delivered += 1);
                } else {
                    shared.stat(|s| s.forward_dropped += 1);
                }
            }
            FrameKind::PeerHello => {
                if peer.is_some() {
                    shared.stat(|s| s.rejected += 1);
                    continue;
                }
                if frame.payload.len() != NODE_ID_LEN {
                    shared.stat(|s| s.rejected += 1);
                    break;
                }
                let mut id = [0u8; NODE_ID_LEN];
                id.copy_from_slice(&frame.payload);
                if id == shared.instance {
                    // 自己连到自己（--peer 指错），直接拒绝，避免自环。
                    shared.stat(|s| s.rejected += 1);
                    break;
                }
                if !shared.install_peer(id, writer.clone()) {
                    // 已有链路：不抢，回 ACK 让呼叫方知道它是谁并自行退让。
                    let ack = RelayFrame {
                        kind: FrameKind::Ack,
                        node: shared.instance,
                        payload: Vec::new(),
                    };
                    let _ = send_frame(&writer, &ack);
                    break;
                }
                peer = Some(id);
                let ack = RelayFrame {
                    kind: FrameKind::Ack,
                    node: shared.instance,
                    payload: Vec::new(),
                };
                if !send_frame(&writer, &ack) {
                    break;
                }
                // 双向立刻对齐：先把我们的快照推过去，对方的 HELLO 之后也会推一次。
                //
                // 顺序是有讲究的：ACK **必须**先发。拨号方在 `dial_once` 里读到第一帧
                // 就断定为 ACK，任何抢在前面的帧都会让整条同侪链路被判失败并进入 1s 重拨
                // （W3.12 实际踩到：`期待 ACK，收到 PeerReach`，两个实例永远连不上）。
                shared.broadcast_snapshot();
                shared.broadcast_reach();
            }
            FrameKind::PeerSync => {
                if peer.is_none() {
                    shared.stat(|s| s.rejected += 1);
                    continue;
                }
                if frame.payload.len() % NODE_ID_LEN != 0 {
                    shared.stat(|s| s.rejected += 1);
                    continue;
                }
                let nodes: Vec<[u8; NODE_ID_LEN]> = frame
                    .payload
                    .chunks_exact(NODE_ID_LEN)
                    .map(|c| {
                        let mut n = [0u8; NODE_ID_LEN];
                        n.copy_from_slice(c);
                        n
                    })
                    .collect();
                let owner = peer.expect("peer 已判定为 Some");
                shared.apply_peer_sync(owner, &nodes);
            }
            // DELIVERED / ACK / REACH_RESULT / REJECT 是服务端→客户端的帧；
            // 客户端回送属于协议违规，忽略而不是断开（断开会让一条坏帧变成一次重连风暴）。
            FrameKind::Delivered | FrameKind::Ack | FrameKind::ReachResult | FrameKind::Reject => {}
            FrameKind::Unknown(_) => shared.stat(|s| s.rejected += 1),
        }
    }

    if let Some(id) = registered {
        // 只有「这条连接仍是该节点的当前注册项」时，它的退出才算节点掉线。
        // 重连时新连接会覆盖注册表；若旧连接随后才关掉，旧代码照样 forget_local，
        // 把新连接刚声明的 relay-only 抹成「不知道」，让 wake 随机走回直连。
        let was_current = shared.registry.remove_if_same(&id, &writer);
        if was_current {
            shared.forget_local(&id);
        }
        if !shared.peers.lock().unwrap().is_empty() {
            shared.broadcast_snapshot();
        }
    }
    if let Some(id) = peer {
        shared.drop_peer(&id);
    }
    if let Ok(g) = writer.lock() {
        g.shutdown();
    }
}

/// 拨同侪并维持一条长连接。断线按 `PEER_RETRY` 重拨；不阻塞 accept。
fn dial_loop(shared: Arc<Shared>, addr: String) {
    let mut learned: Option<[u8; NODE_ID_LEN]> = None;
    while !shared.stop.load(Ordering::Relaxed) {
        // 对端已经有活链路（很可能是对方向我们拨的）就不重复拨，避免抢链。
        if let Some(id) = learned {
            if shared.peers.lock().unwrap().contains_key(&id) {
                sleep_unless_stopped(&shared, PEER_RETRY);
                continue;
            }
        }
        match dial_once(&shared, &addr) {
            Ok((peer_id, reader, writer)) => {
                learned = Some(peer_id);
                // 拨号方在 dial_once 里已收到 ACK，这里推声明不会抢在 ACK 前面。
                if !shared.install_peer(peer_id, writer.clone()) {
                    sleep_unless_stopped(&shared, PEER_RETRY);
                    continue;
                }
                shared.broadcast_snapshot();
                // 拨号侧已经完成过握手，直接进入通道循环（不再走 conn_loop 的握手分支）。
                // 出向同侪链路：`ip=None`，不占客户端配额、也不写每 IP 计数。
                serve_chan(reader, writer, shared.clone(), None, Some(peer_id), None);
                shared.drop_peer(&peer_id);
            }
            Err(e) => {
                if !shared.is_stopped_quiet() {
                    eprintln!("[relay] 同侪 {} 连接失败: {}，{}s 后重试",
                              addr, e, PEER_RETRY.as_secs());
                }
            }
        }
        sleep_unless_stopped(&shared, PEER_RETRY);
    }
}

impl Shared {
    fn is_stopped_quiet(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

fn dial_once(
    shared: &Arc<Shared>,
    addr: &str,
) -> Result<([u8; NODE_ID_LEN], ChanReader, Arc<Mutex<ChanWriter>>), String> {
    let sock_addr = addr
        .to_socket_addrs_first()
        .ok_or_else(|| format!("同侪地址无解析结果: {}", addr))?;
    let stream = TcpStream::connect_timeout(&sock_addr, CONNECT_TIMEOUT)
        .map_err(|e| format!("connect: {}", e))?;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let _ = stream.set_read_timeout(Some(shared.limits.handshake_timeout));

    // 同侪链路与普通连接走同一套传输层：配了 PSK 就同样要握手。
    let (mut reader, writer) = match shared.psk {
        Some(psk) => {
            let ch = secure::client_handshake(stream, &psk, shared.max_frame)?;
            (
                ChanReader::Secure(ch.reader),
                Arc::new(Mutex::new(ChanWriter::Secure(ch.writer))),
            )
        }
        None => {
            let r = stream.try_clone().map_err(|e| e.to_string())?;
            (
                ChanReader::Plain {
                    stream: r,
                    max_frame: shared.max_frame,
                },
                Arc::new(Mutex::new(ChanWriter::Plain(stream))),
            )
        }
    };
    let hello = RelayFrame {
        kind: FrameKind::PeerHello,
        node: shared.instance,
        payload: shared.instance.to_vec(),
    };
    if !send_frame(&writer, &hello) {
        return Err("发送 PEER_HELLO 失败".into());
    }
    let ack = reader.recv().map_err(|e| e.to_string())?;
    if ack.kind != FrameKind::Ack {
        return Err(format!("期待 ACK，收到 {:?}", ack.kind));
    }
    let peer_id = ack.node;
    if peer_id == [0u8; NODE_ID_LEN] {
        return Err("对端未回实例 id".into());
    }
    if peer_id == shared.instance {
        return Err("对端实例 id 与本机相同（--peer 指错或重复实例）".into());
    }
    // 进入长连接阶段：空闲判定由 serve_chan 统一接管。
    let _ = reader.set_read_timeout(None);
    Ok((peer_id, reader, writer))
}

trait ToSocketAddrFirst {
    fn to_socket_addrs_first(&self) -> Option<SocketAddr>;
}

impl ToSocketAddrFirst for str {
    fn to_socket_addrs_first(&self) -> Option<SocketAddr> {
        use std::net::ToSocketAddrs;
        self.to_socket_addrs().ok()?.next()
    }
}

fn sleep_unless_stopped(shared: &Shared, d: Duration) {
    let step = Duration::from_millis(100);
    let mut waited = Duration::ZERO;
    while waited < d {
        if shared.stop.load(Ordering::Relaxed) {
            return;
        }
        thread::sleep(step);
        waited += step;
    }
}

/// 从流中读一帧；长度字段超过 `max_frame` 立刻报错关闭连接。
pub fn read_frame<R: Read>(reader: &mut R, max_frame: usize) -> Result<RelayFrame, String> {
    let mut head = [0u8; HEADER_LEN];
    reader
        .read_exact(&mut head)
        .map_err(|e| format!("读帧头失败: {}", e))?;
    let len = u32::from_be_bytes([
        head[1 + NODE_ID_LEN],
        head[2 + NODE_ID_LEN],
        head[3 + NODE_ID_LEN],
        head[4 + NODE_ID_LEN],
    ]) as usize;
    if len > max_frame {
        return Err(format!("帧体超限: {} > {}", len, max_frame));
    }
    let mut payload = vec![0u8; len];
    if len > 0 {
        reader
            .read_exact(&mut payload)
            .map_err(|e| format!("读帧体失败: {}", e))?;
    }
    let mut raw = head.to_vec();
    raw.extend_from_slice(&payload);
    RelayFrame::decode(&raw).ok_or_else(|| "帧解析失败".into())
}

pub fn write_frame<W: Write>(writer: &mut W, frame: &RelayFrame) -> io::Result<()> {
    writer.write_all(&frame.encode())?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::RelayClient;
    use std::io::{Cursor, Read};
    use std::net::TcpStream;

    fn id(label: u8) -> [u8; NODE_ID_LEN] {
        let mut v = [0u8; NODE_ID_LEN];
        v[0] = label;
        v
    }

    /// 等计数追上「客户端已经收到帧」这个事实。
    ///
    /// 计数由服务端线程在投递**之后**自增，客户端 `recv_frame()` 返回时它可能还没跑到，
    /// 于是断言偶发失败（实测约 1/3）。偶发失败的测试等于没有测试，这里改成有界等待。
    /// 注意是 `FnMut`：跨实例那条用例要在闭包里反复发查询帧（`lookup_reach` 需要 `&mut`）。
    /// 其余调用点都是纯读，`FnMut` 完全兼容。
    fn wait_until<F: FnMut() -> bool>(mut pred: F, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if pred() {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return pred();
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn routes_frames_between_two_real_tcp_clients() {
        let server = RelayServer::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        let addr = format!("127.0.0.1:{}", port);

        let mut a = RelayClient::new(addr.clone())
            .connect(Duration::from_secs(2))
            .unwrap();
        let mut b = RelayClient::new(addr).connect(Duration::from_secs(2)).unwrap();
        let id_a = id(b'A');
        let id_b = id(b'B');
        a.register(&id_a).unwrap();
        b.register(&id_b).unwrap();

        b.send_relay(&id_a, b"WAKEUP_REQ").unwrap();
        let got = a.recv_frame().unwrap();
        assert_eq!(got.kind, FrameKind::Delivered);
        assert_eq!(got.node, id_b, "DELIVERED 的来源必须是真实发送方");
        assert_eq!(got.payload, b"WAKEUP_REQ");

        a.send_relay(&got.node, b"WAKEUP_ACK").unwrap();
        let back = b.recv_frame().unwrap();
        assert_eq!(back.kind, FrameKind::Delivered);
        assert_eq!(back.node, id_a);
        assert_eq!(back.payload, b"WAKEUP_ACK");

        assert!(wait_until(|| handle.stats().delivered == 2, Duration::from_millis(800)),
                "两帧都投递后 delivered 应为 2（当前 {:?}）", handle.stats().delivered);
        let s = handle.stats();
        assert_eq!(s.dropped, 0);
        assert_eq!(s.registered, 2);
        handle.stop();
    }

    #[test]
    fn unknown_target_is_dropped_never_faked() {
        let server = RelayServer::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        let mut b = RelayClient::new(format!("127.0.0.1:{}", port))
            .connect(Duration::from_secs(2))
            .unwrap();
        b.register(&id(b'B')).unwrap();
        b.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        b.send_relay(&id(b'Z'), b"nobody").unwrap();
        assert!(b.recv_frame().is_err(), "未注册目标不得收到任何合成应答");
        let s = handle.stats();
        assert_eq!(s.dropped, 1);
        assert_eq!(s.delivered, 0);
        assert_eq!(s.forwarded, 0, "没有任何同侪声明归属时不得计转发");
        handle.stop();
    }

    #[test]
    fn inconsistent_register_payload_is_rejected() {
        let server = RelayServer::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        let mut c = RelayClient::new(format!("127.0.0.1:{}", port))
            .connect(Duration::from_secs(2))
            .unwrap();
        let bad = RelayFrame {
            kind: FrameKind::Register,
            node: id(b'C'),
            payload: id(b'X').to_vec(),
        };
        c.write_raw(&bad.encode()).unwrap();
        c.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        assert!(c.recv_frame().is_err());
        assert_eq!(handle.stats().rejected, 1);
        assert_eq!(handle.stats().registered, 0);
        handle.stop();
    }

    /// 派生 id 含控制字节时，展示标签必须是可打印 ASCII（否则计数文件不是合法 JSON）。
    #[test]
    fn printable_instance_never_emits_control_bytes() {
        let mut id = [0u8; NODE_ID_LEN];
        id[..7].copy_from_slice(b"RLYINST");
        id[7] = 0x0a;
        id[8] = 0x01;
        id[9] = 0x7f;
        id[10] = 0x20;
        let label = printable_instance(&id);
        assert!(label.starts_with("RLYINST"), "{}", label);
        assert!(!label.chars().any(|c| (c as u32) < 0x20 || c as u32 == 0x7f),
                "标签仍含控制字符: {:?}", label);
        let named = parse_instance_id("relay-222").unwrap();
        assert_eq!(printable_instance(&named), "relay-222");
    }


    // ------------------------------------------------------------------
    // 生产化收尾（W3.9 后续）：黑名单 / 单 IP 上限 / 注册频率 / 注册表分片
    // ------------------------------------------------------------------

    /// 读一个字节判断连接是否已被服务端关闭（`Ok(0)` = 对端 FIN）。
    fn closed_by_peer(stream: &mut TcpStream) -> bool {
        let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
        let mut b = [0u8; 1];
        matches!(stream.read(&mut b), Ok(0))
    }

    #[test]
    fn ipnet_parsing_and_matching() {
        let net = IpNet::parse("112.5.60.0/24").unwrap();
        assert!(net.contains("112.5.60.4".parse().unwrap()));
        assert!(!net.contains("112.5.61.1".parse().unwrap()));
        let single = IpNet::parse("112.5.60.4").unwrap();
        assert!(single.contains("112.5.60.4".parse().unwrap()));
        assert!(!single.contains("112.5.60.5".parse().unwrap()));
        let all = IpNet::parse("0.0.0.0/0").unwrap();
        assert!(all.contains("8.8.8.8".parse().unwrap()));
        // 必须拒绝不成立的东西，而不是「宽松匹配」：写错的黑名单等于没黑名单。
        assert!(IpNet::parse("112.5.60.0/33").is_err());
        assert!(IpNet::parse("::1/128").is_err(), "IPv6 暂不支持，必须明确报错");
        assert!(IpNet::parse("not-an-ip").is_err());
    }

    #[test]
    fn denied_ip_is_closed_before_handshake() {
        let server = RelayServer::bind("127.0.0.1:0").unwrap()
            .with_deny(vec![IpNet::parse("127.0.0.0/8").unwrap()]);
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        assert!(closed_by_peer(&mut s), "黑名单来源必须被立即断开");
        assert!(wait_until(|| handle.stats().blacklisted >= 1, Duration::from_millis(800)),
                "blacklisted 计数应为 1（当前 {:?}）", handle.stats().blacklisted);
        assert_eq!(handle.stats().connections, 1);
        handle.stop();
    }

    #[test]
    fn per_ip_connection_cap_rejects_extra_connections() {
        let limits = Limits { max_conns_per_ip: 1, ..Default::default() };
        let server = RelayServer::bind("127.0.0.1:0").unwrap().with_limits(limits);
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();

        let _first = TcpStream::connect(("127.0.0.1", port)).unwrap();
        // 等第一条被 accept 记账，否则第二条可能抢在计数之前到达（那是测试的竞态，不是产品的）。
        assert!(wait_until(|| handle.stats().connections >= 1, Duration::from_millis(800)));
        let mut second = TcpStream::connect(("127.0.0.1", port)).unwrap();
        assert!(closed_by_peer(&mut second), "超过单 IP 上限的连接必须被断开");
        assert!(wait_until(|| handle.stats().ip_cap_rejected >= 1, Duration::from_millis(800)),
                "ip_cap_rejected 应为 1（当前 {:?}）", handle.stats().ip_cap_rejected);
        handle.stop();
    }

    #[test]
    fn registration_rate_limit_per_ip_closes_late_registrations() {
        let limits = Limits { register_per_ip_per_min: 1, ..Default::default() };
        let server = RelayServer::bind("127.0.0.1:0").unwrap().with_limits(limits);
        let addr = format!("127.0.0.1:{}", server.local_addr().unwrap().port());
        let handle = server.spawn();

        let mut a = RelayClient::new(addr.clone()).connect(Duration::from_secs(2)).unwrap();
        a.register(&id(b'A')).expect("第一次注册必须在配额内");

        let mut b = RelayClient::new(addr).connect(Duration::from_secs(2)).unwrap();
        let err = b.register(&id(b'B')).expect_err("同 IP 第二次注册应被拒");
        // 必须能区分「被配额拒绝」与「链路断了」：以前只有一句 failed to fill whole buffer，
        // 那种错误会把人往网络故障的方向带。
        assert!(err.contains("拒绝"), "错误必须表明是中继拒绝: {err}");
        assert!(err.contains("频率"), "错误必须带具体原因: {err}");
        assert!(wait_until(|| handle.stats().ip_rate_limited >= 1, Duration::from_millis(800)),
                "ip_rate_limited 应为 1（当前 {:?}）", handle.stats().ip_rate_limited);
        // 第一次注册仍然有效：配额只挡新注册，不清空已注册的节点。
        assert!(wait_until(|| handle.stats().registered == 1, Duration::from_millis(500)));
        handle.stop();
    }

    #[test]
    fn ip_state_is_released_after_disconnect() {
        let server = RelayServer::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        {
            let _s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            assert!(wait_until(|| handle.stats().ip_tracked >= 1, Duration::from_millis(800)),
                    "连接期间应跟踪到 1 个 IP");
        }
        // 断开后计数必须回落：条目只增不减就是「小状态大泄漏」。
        assert!(wait_until(|| handle.stats().ip_tracked == 0, Duration::from_millis(1500)),
                "断开后 ip_tracked 应回到 0（当前 {:?}）", handle.stats().ip_tracked);
        handle.stop();
    }

    #[test]
    fn registry_len_counts_replacements_once_and_registry_survives_sharding() {
        // 分片不能改变可见语义：同一 ID 反复注册只占一个名额（否则重连会把自己的
        // 名额越占越多，最终触发 max_nodes 把正常客户端挡在门外）。
        let server = RelayServer::bind("127.0.0.1:0").unwrap()
            .with_limits(Limits { max_nodes: 3, ..Default::default() });
        let addr = format!("127.0.0.1:{}", server.local_addr().unwrap().port());
        let handle = server.spawn();

        let mut a = RelayClient::new(addr.clone()).connect(Duration::from_secs(2)).unwrap();
        a.register(&id(b'A')).unwrap();
        for _ in 0..3 {
            a.register(&id(b'A')).expect("重复注册必须始终成功");
        }
        // 换一个跨片 ID 注册，仍应成功（len 记的是不同 ID 的数）。
        let mut b = RelayClient::new(addr).connect(Duration::from_secs(2)).unwrap();
        b.register(&id(b'\x80')).expect("第二个不同 ID 应能注册");

        // 跨片投递仍然成立：分片只影响锁，不影响路由。
        b.send_relay(&id(b'A'), b"ping").unwrap();
        let got = a.recv_frame().expect("A 必须收到跨片投递");
        assert_eq!(got.payload, b"ping");
        assert!(wait_until(|| handle.stats().delivered == 1, Duration::from_millis(800)));
        handle.stop();
    }

    #[test]
    fn frame_size_limit_is_enforced() {
        let mut raw = vec![0x02u8];
        raw.extend_from_slice(&[0u8; NODE_ID_LEN]);
        raw.extend_from_slice(&65u32.to_be_bytes());
        raw.extend_from_slice(&[0u8; 65]);
        assert!(read_frame(&mut Cursor::new(raw), 64).is_err());
    }

    /// W3.8 核心用例：两个**独立实例**各持一部分客户端，跨实例双向投递必须成立。
    ///
    /// 这就是 W3.7 双实例基线里 `dropped=1` 的那条帧。判据不是「看起来通了」，而是：
    /// 两端 DELIVERED 的来源 id 都是**真实对端节点**、两端 `dropped==0`、且转发计数各 ≥1。
    #[test]
    fn peer_sync_enables_cross_instance_delivery() {
        let s1 = RelayServer::bind("127.0.0.1:0").unwrap()
            .with_instance_id(parse_instance_id("relay-one").unwrap());
        let p1 = s1.local_addr().unwrap().port();
        let h1 = s1.spawn();

        let s2 = RelayServer::bind("127.0.0.1:0").unwrap()
            .with_instance_id(parse_instance_id("relay-two").unwrap())
            .with_peer(&format!("127.0.0.1:{}", p1));
        let p2 = s2.local_addr().unwrap().port();
        let h2 = s2.spawn();

        // 同侪链路必须先建起来，否则注册快照没有接收方（周期兜底要等 500ms）。
        assert!(wait_until(|| h2.stats().peers == 1 && h1.stats().peers == 1,
                           Duration::from_secs(3)),
                "两端同侪链路应各为 1（h1={:?} h2={:?}）", h1.stats(), h2.stats());

        let mut a = RelayClient::new(format!("127.0.0.1:{}", p1))
            .connect(Duration::from_secs(2)).unwrap();
        let mut b = RelayClient::new(format!("127.0.0.1:{}", p2))
            .connect(Duration::from_secs(2)).unwrap();
        let id_a = id(b'A');
        let id_b = id(b'B');
        a.register(&id_a).unwrap();
        b.register(&id_b).unwrap();
        a.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        b.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

        // A 的注册要经快照传到 h2，才可能被转发。
        assert!(wait_until(|| h2.stats().peer_nodes == 1, Duration::from_secs(3)),
                "h2 应通过快照学到 h1 的 1 个节点（当前 {:?}）", h2.stats());

        // B → A（B 挂在 h2，A 挂在 h1：h2 必须转发）
        b.send_relay(&id_a, b"WAKEUP_REQ").unwrap();
        let got = a.recv_frame().expect("跨实例投递应把 REQ 送到 A");
        assert_eq!(got.kind, FrameKind::Delivered);
        assert_eq!(got.node, id_b, "来源必须是真实发送方，不能是转发实例 id");
        assert_eq!(got.payload, b"WAKEUP_REQ");

        // A → B（反向，h1 转发）
        a.send_relay(&got.node, b"WAKEUP_ACK").unwrap();
        let back = b.recv_frame().expect("跨实例投递应把 ACK 送回来");
        assert_eq!(back.node, id_a);
        assert_eq!(back.payload, b"WAKEUP_ACK");

        // 计数在服务端线程里于「写完之后」自增，客户端可能先拿到帧；有界等待再断言。
        let converged = wait_until(
            || {
                let (a, b) = (h1.stats(), h2.stats());
                a.delivered == 1 && a.forwarded == 1 && b.delivered == 1 && b.forwarded == 1
                    && a.dropped + b.dropped == 0
            },
            Duration::from_secs(2),
        );
        let st1 = h1.stats();
        let st2 = h2.stats();
        assert!(converged, "计数应收敛到 delivered/forwarded 各 1（h1={:?} h2={:?}）", st1, st2);
        assert_eq!(st1.delivered, 1);
        assert_eq!(st1.forwarded, 1);
        assert_eq!(st2.delivered, 1);
        assert_eq!(st2.forwarded, 1);
        assert_eq!(st1.dropped + st2.dropped, 0, "跨实例路径不得出现 dropped");
        assert_eq!(st1.forward_dropped + st2.forward_dropped, 0);
        h1.stop();
        h2.stop();
    }

    /// 同侪掉线后，它的快照必须消失：不能留下「幽灵归属」把帧转进黑洞。
    ///
    /// 这条用例盯的是一个很容易被忽略的失败形态：链路断了、注册表视图没清，
    /// 之后所有投给该实例的帧都会 `forwarded += 1` 然后就没了——计数好看，业务全丢。
    #[test]
    fn peer_death_purges_its_nodes_from_registry_view() {
        let s1 = RelayServer::bind("127.0.0.1:0").unwrap()
            .with_instance_id(parse_instance_id("relay-dying").unwrap());
        let p1 = s1.local_addr().unwrap().port();
        let h1 = s1.spawn();

        let s2 = RelayServer::bind("127.0.0.1:0").unwrap()
            .with_instance_id(parse_instance_id("relay-survivor").unwrap())
            .with_peer(&format!("127.0.0.1:{}", p1));
        let p2 = s2.local_addr().unwrap().port();
        let h2 = s2.spawn();

        let mut a = RelayClient::new(format!("127.0.0.1:{}", p1))
            .connect(Duration::from_secs(2)).unwrap();
        a.register(&id(b'A')).unwrap();
        assert!(wait_until(|| h2.stats().peers == 1 && h2.stats().peer_nodes == 1,
                           Duration::from_secs(3)),
                "h2 应先学到 h1 与其 1 个节点（{:?}）", h2.stats());

        // 让 h1 整个实例下线：链路被 shutdown，h2 的读循环必须立刻结束。
        h1.stop();
        assert!(wait_until(|| h2.stats().peers == 0 && h2.stats().peer_nodes == 0,
                           Duration::from_secs(5)),
                "h1 下线后 h2 的同侪与幽灵节点都必须清零（{:?}）", h2.stats());

        // 幽灵清除之后，投给 A 必须如实 dropped，而不是算成 forwarded。
        let mut b = RelayClient::new(format!("127.0.0.1:{}", p2))
            .connect(Duration::from_secs(2)).unwrap();
        b.register(&id(b'B')).unwrap();
        b.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        b.send_relay(&id(b'A'), b"nobody-home").unwrap();
        assert!(b.recv_frame().is_err(), "对端实例已下线，不得收到任何合成应答");
        assert!(wait_until(|| h2.stats().dropped == 1, Duration::from_millis(800)),
                "应计 1 次 dropped（{:?}）", h2.stats());
        assert_eq!(h2.stats().forwarded, 0, "不能把帧转进已消失的同侪");
        h2.stop();
    }

    // ---------- W3.9：传输层与限额 ----------

    const K: [u8; PSK_LEN] = [0x5Au8; PSK_LEN];

    /// 配了 PSK 的实例：明文客户端连不上，且拒绝要能被统计到。
    #[test]
    fn secure_instance_rejects_plaintext_client() {
        let limits = Limits {
            handshake_timeout: Duration::from_millis(300),
            ..Limits::default()
        };
        let server = RelayServer::bind("127.0.0.1:0")
            .unwrap()
            .with_psk(Some(K))
            .with_limits(limits);
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        let mut c = RelayClient::plaintext(format!("127.0.0.1:{}", port))
            .connect(Duration::from_secs(2))
            .unwrap();
        // 服务端在读 hello 时就会失败/超时；客户端的 ACK 永远等不到。
        let err = c.register(&id(b'A')).err().unwrap_or_default();
        assert!(!err.is_empty(), "明文客户端不得注册成功");
        assert!(wait_until(|| handle.stats().secure_rejected == 1, Duration::from_secs(3)),
                "应计一次 secure_rejected（{:?}）", handle.stats());
        assert_eq!(handle.stats().registered, 0);
        handle.stop();
    }

    /// 两端都带同一把 PSK：投递必须成立，且握手计数为 2。
    #[test]
    fn secure_instance_routes_between_authenticated_clients() {
        let limits = Limits {
            handshake_timeout: Duration::from_millis(300),
            ..Limits::default()
        };
        let server = RelayServer::bind("127.0.0.1:0")
            .unwrap()
            .with_psk(Some(K))
            .with_limits(limits);
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        let addr = format!("127.0.0.1:{}", port);
        let mut a = RelayClient::new(addr.clone()).with_psk(Some(K))
            .connect(Duration::from_secs(3)).unwrap();
        let mut b = RelayClient::new(addr).with_psk(Some(K))
            .connect(Duration::from_secs(3)).unwrap();
        assert!(a.is_secure() && b.is_secure());
        a.register(&id(b'A')).unwrap();
        b.register(&id(b'B')).unwrap();
        b.send_relay(&id(b'A'), b"WAKEUP_REQ").unwrap();
        let got = a.recv_frame().unwrap();
        assert_eq!(got.kind, FrameKind::Delivered);
        assert_eq!(got.payload, b"WAKEUP_REQ");
        assert_eq!(got.node, id(b'B'));
        assert!(wait_until(|| handle.stats().secure_handshakes == 2, Duration::from_secs(2)),
                "两次握手都应成功（{:?}）", handle.stats());
        assert_eq!(handle.stats().secure_rejected, 0);
        handle.stop();
    }

    /// 每连接速率上限：刷帧的连接被断开，计数器可归因。
    #[test]
    fn flooding_connection_is_rate_limited() {
        let limits = Limits {
            rate_per_sec: 1,
            burst: 2,
            idle: Duration::ZERO,
            ..Limits::default()
        };
        let server = RelayServer::bind("127.0.0.1:0").unwrap().with_limits(limits);
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        let mut c = RelayClient::plaintext(format!("127.0.0.1:{}", port))
            .connect(Duration::from_secs(2)).unwrap();
        c.register(&id(b'F')).unwrap();
        for _ in 0..8 {
            // 写到被断开为止；失败也算预期（服务端已经关了连接）。
            if c.send_relay(&id(b'Z'), b"flood").is_err() {
                break;
            }
        }
        assert!(wait_until(|| handle.stats().rate_limited == 1, Duration::from_secs(3)),
                "应因超速断开一次（{:?}）", handle.stats());
        handle.stop();
    }

    /// 并发连接上限：超出的连接被立刻关闭并计数。
    #[test]
    fn connection_cap_is_enforced() {
        let limits = Limits {
            max_conns: 1,
            ..Limits::default()
        };
        let server = RelayServer::bind("127.0.0.1:0").unwrap().with_limits(limits);
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        let addr = format!("127.0.0.1:{}", port);
        let mut first = RelayClient::plaintext(addr.clone())
            .connect(Duration::from_secs(2)).unwrap();
        first.register(&id(b'1')).unwrap();
        assert!(wait_until(|| handle.stats().registered == 1, Duration::from_secs(2)));
        let mut second = RelayClient::plaintext(addr).connect(Duration::from_secs(2)).unwrap();
        let err = second.register(&id(b'2')).err().unwrap_or_default();
        assert!(!err.is_empty(), "第二个连接应被拒（实际: {}）", err);
        assert!(wait_until(|| handle.stats().conn_cap_rejected == 1, Duration::from_secs(2)),
                "应计一次 conn_cap_rejected（{:?}）", handle.stats());
        handle.stop();
    }

    // ------------------------------------------------------------------
    // W3.12 可达性声明（借 eMule「登录时报 LowID」）
    // ------------------------------------------------------------------

    fn decl_relay_only() -> crate::reach::ReachDeclaration {
        crate::reach::ReachDeclaration {
            class: ReachClass::RelayOnly,
            nat_class: crate::NatType::Symmetric,
            declared_at_ms: crate::reach::now_ms(),
            ttl_ms: crate::reach::DEFAULT_TTL_MS,
        }
    }

    fn decl_direct() -> crate::reach::ReachDeclaration {
        crate::reach::ReachDeclaration {
            class: ReachClass::Direct,
            nat_class: crate::NatType::PortRestrictedCone,
            declared_at_ms: crate::reach::now_ms(),
            ttl_ms: crate::reach::DEFAULT_TTL_MS,
        }
    }

    #[test]
    fn declared_limit_is_visible_to_another_client() {
        let server = RelayServer::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        let addr = format!("127.0.0.1:{}", port);

        let mut holder = RelayClient::plaintext(addr.clone())
            .connect(Duration::from_secs(2))
            .unwrap();
        let mut asker = RelayClient::plaintext(addr).connect(Duration::from_secs(2)).unwrap();
        let id_h = id(b'H');
        let id_q = id(b'Q');

        holder.register_declared(&id_h, &decl_relay_only()).unwrap();
        asker.register(&id_q).unwrap();
        let got = asker
            .lookup_reach(&id_h, Duration::from_secs(2))
            .unwrap()
            .expect("已声明的节点应查得到");
        assert_eq!(got.class, ReachClass::RelayOnly);
        assert!(got.effective_at(crate::reach::now_ms()).allows_skipping_direct());

        assert!(wait_until(
            || handle.stats().declared_relay_only == 1,
            Duration::from_secs(2)
        ));
        handle.stop();
    }

    /// 重连竞态：新连接刚 `REGISTER_DECLARED`，旧连接随后才关。
    /// 旧连接的退出不能把新连接刚声明的 relay-only 抹掉，否则 wake 又随机走回直连。
    #[test]
    fn stale_close_does_not_erase_reconnected_declaration() {
        let server = RelayServer::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        let addr = format!("127.0.0.1:{port}");

        let mut old = RelayClient::plaintext(addr.clone())
            .connect(Duration::from_secs(2)).unwrap();
        old.register_declared(&id(b'R'), &decl_relay_only()).unwrap();
        let mut new = RelayClient::plaintext(addr.clone())
            .connect(Duration::from_secs(2)).unwrap();
        new.register_declared(&id(b'R'), &decl_relay_only()).unwrap();
        let mut asker = RelayClient::plaintext(addr)
            .connect(Duration::from_secs(2)).unwrap();
        asker.register(&id(b'Q')).unwrap();

        drop(old);
        thread::sleep(Duration::from_millis(300));
        for _ in 0..5 {
            let got = asker.lookup_reach(&id(b'R'), Duration::from_secs(2)).unwrap()
                .expect("新连接仍在线，声明不能被旧连接的退出清掉");
            assert_eq!(got.class, ReachClass::RelayOnly);
            thread::sleep(Duration::from_millis(20));
        }
        handle.stop();
    }

    /// 「没声明」必须回「不知道」，不能回一个看起来合理的默认类别。
    /// 回 `RelayOnly` 会让所有没升级的节点被强制走中继；回 `Direct` 会让对称型
    /// 节点被白白直连一次。两种都是编造。
    #[test]
    fn lookup_of_undeclared_node_returns_unknown_not_a_guess() {
        let server = RelayServer::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        let addr = format!("127.0.0.1:{}", port);
        let mut c = RelayClient::plaintext(addr).connect(Duration::from_secs(2)).unwrap();
        c.register(&id(b'A')).unwrap();
        let got = c
            .lookup_reach(&id(b'Z'), Duration::from_secs(2))
            .unwrap();
        assert!(got.is_none(), "未声明的节点不能返回任何类别");
        assert!(wait_until(
            || handle.stats().reach_lookups == 1,
            Duration::from_secs(2)
        ));
        handle.stop();
    }

    /// 老客户端只发 REGISTER：它必须被计为「未声明」，不能默认成任何类别。
    #[test]
    fn legacy_register_counts_as_undeclared() {
        let server = RelayServer::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        let addr = format!("127.0.0.1:{}", port);
        let mut c = RelayClient::plaintext(addr).connect(Duration::from_secs(2)).unwrap();
        c.register(&id(b'L')).unwrap();
        assert!(wait_until(
            || handle.stats().declared_unknown == 1 && handle.stats().declared_direct == 0,
            Duration::from_secs(2)
        ), "老客户端应计为未声明（{:?}）", handle.stats());
        handle.stop();
    }

    /// 跨实例：声明在 A 实例、查询在 B 实例。这条用例是 PEER_REACH 的存在理由——
    /// 我们真实部署就是 222 + 209 两个实例，不能只在本地实例内可见。
    #[test]
    fn declaration_crosses_instances() {
        let s1 = RelayServer::bind("127.0.0.1:0")
            .unwrap()
            .with_instance_id(parse_instance_id("relay-one").unwrap());
        let p1 = s1.local_addr().unwrap().port();
        let h1 = s1.spawn();
        let s2 = RelayServer::bind("127.0.0.1:0")
            .unwrap()
            .with_instance_id(parse_instance_id("relay-two").unwrap())
            .with_peer(&format!("127.0.0.1:{}", p1));
        let p2 = s2.local_addr().unwrap().port();
        let h2 = s2.spawn();

        // 持有者挂在实例 1。
        let mut holder = RelayClient::plaintext(format!("127.0.0.1:{}", p1))
            .connect(Duration::from_secs(2))
            .unwrap();
        holder.register_declared(&id(b'H'), &decl_relay_only()).unwrap();
        // 等两条实例链路建立。
        assert!(wait_until(|| h2.stats().peers == 1, Duration::from_secs(5)), "实例链路未建立");
        // 请求方挂在实例 2，查实例 1 上的节点。
        let mut asker = RelayClient::plaintext(format!("127.0.0.1:{}", p2))
            .connect(Duration::from_secs(2))
            .unwrap();
        asker.register(&id(b'Q')).unwrap();
        let got = wait_until(
            || {
                asker
                    .lookup_reach(&id(b'H'), Duration::from_secs(1))
                    .ok()
                    .flatten()
                    .map(|d| d.class == ReachClass::RelayOnly)
                    .unwrap_or(false)
            },
            Duration::from_secs(5),
        );
        assert!(got, "跨实例声明未同步（instance2: {:?}）", h2.stats());
        h1.stop();
        h2.stop();
    }

    /// 负向对照：客户端自己发 PEER_REACH 帧 = 想替任意节点声明类别，必须拒。
    #[test]
    fn client_cannot_forge_peer_reach_snapshot() {
        let server = RelayServer::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        let addr = format!("127.0.0.1:{}", port);
        let mut c = RelayClient::plaintext(addr).connect(Duration::from_secs(2)).unwrap();
        c.register(&id(b'C')).unwrap();
        // 伪造一条「节点 Z 可直连」的声明。
        let fake = encode_reach_record(&id(b'Z'), ReachClass::Direct, crate::reach::now_ms());
        let frame = RelayFrame {
            kind: FrameKind::PeerReach,
            node: id(b'C'),
            payload: fake.to_vec(),
        };
        c.write_raw(&frame.encode()).unwrap();
        assert!(wait_until(
            || handle.stats().rejected >= 1,
            Duration::from_secs(2)
        ), "伪造快照应被计为 rejected（{:?}）", handle.stats());
        assert_eq!(handle.stats().declared_direct, 0, "不得采信伪造声明");
        handle.stop();
    }

    /// 负向对照：声明帧的负载 id 与帧头不一致 = 替别人声明，必须拒。
    #[test]
    fn declared_register_with_mismatched_id_is_rejected() {
        let server = RelayServer::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        let addr = format!("127.0.0.1:{}", port);
        let mut c = RelayClient::plaintext(addr).connect(Duration::from_secs(2)).unwrap();
        // 帧头 C，负载 Z：想把自己的声明记到别人名下（或反过来抢占身份）。
        let payload = encode_reach_record(&id(b'Z'), ReachClass::RelayOnly, crate::reach::now_ms());
        let frame = RelayFrame {
            kind: FrameKind::RegisterDeclared,
            node: id(b'C'),
            payload: payload.to_vec(),
        };
        c.write_raw(&frame.encode()).unwrap();
        assert!(wait_until(
            || handle.stats().rejected >= 1,
            Duration::from_secs(2)
        ), "id 不一致应被拒（{:?}）", handle.stats());
        assert_eq!(handle.stats().declared_relay_only, 0);
        handle.stop();
    }

    /// 断开后声明必须消失：一台关机/换网的机器不能永远留着「可直连」的旧声明。
    #[test]
    fn declaration_disappears_when_node_disconnects() {
        let server = RelayServer::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        let addr = format!("127.0.0.1:{}", port);
        let mut holder = RelayClient::plaintext(addr.clone())
            .connect(Duration::from_secs(2))
            .unwrap();
        holder.register_declared(&id(b'H'), &decl_direct()).unwrap();
        assert!(wait_until(|| handle.stats().declared_direct == 1, Duration::from_secs(2)));
        drop(holder);
        assert!(
            wait_until(|| handle.stats().declared_direct == 0, Duration::from_secs(5)),
            "断开后声明应被清理（{:?}）",
            handle.stats()
        );
        handle.stop();
    }

    /// W3.13：信令帧绕过数据预算，数据帧在预算耗尽时被明确拒绝（不是静默超时）。
    #[test]
    fn data_budget_blocks_payload_but_never_signaling() {
        let limits = Limits {
            data_byte_rate: 1,
            data_burst_bytes: 1, // 任何超过 1 字节的数据帧一进来就不够
            ..Limits::default()
        };
        let server = RelayServer::bind("127.0.0.1:0").unwrap().with_limits(limits);
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        let addr = format!("127.0.0.1:{}", port);
        let mut sender = RelayClient::plaintext(addr.clone()).connect(Duration::from_secs(2)).unwrap();
        let mut holder = RelayClient::plaintext(addr).connect(Duration::from_secs(2)).unwrap();
        sender.register(&id(b'S')).unwrap();
        holder.register(&id(b'H')).unwrap();
        holder.set_read_timeout(Some(Duration::from_millis(250))).unwrap();

        // 唤醒信令：远小于 4096 字节，即使预算为 1 也必须送达。
        sender.send_relay(&id(b'H'), b"WAKEUP_REQ").unwrap();
        let got = holder.recv_frame().unwrap();
        assert_eq!(got.payload, b"WAKEUP_REQ");

        // 数据帧：8 KiB 分片，预算不足 → 不投递，且计数与「找不到目标」分开。
        sender.send_relay(&id(b'H'), &vec![0x5a; 8192]).unwrap();
        let timed_out = holder.recv_frame().is_err();
        assert!(timed_out, "预算不足的数据帧不应被投递");
        let s = handle.stats();
        assert_eq!(s.bandwidth_limited, 1, "应如实记一次带宽拒绝");
        assert!(s.bandwidth_limited_bytes >= 8192);
        assert_eq!(s.data_bytes_admitted, 0, "信令不能计入数据字节");
        assert_eq!(s.dropped, 0, "带宽拒绝不能混成 dropped");
        assert_eq!(s.delivered, 1);
        handle.stop();
    }

    #[test]
    fn byte_bucket_refills_by_elapsed_time_and_never_partial() {
        let limits = Limits { data_byte_rate: 1000, data_burst_bytes: 1000, ..Limits::default() };
        let mut bucket = ByteBucket::new(&limits);
        assert!(bucket.try_take(1000));
        assert!(!bucket.try_take(1), "突发用尽后连 1 字节也不能放行");
        // 同一模块内可直接推进计时，避免用 sleep 做时序断言。
        bucket.last = Instant::now() - Duration::from_secs(1);
        assert!(bucket.try_take(1000), "1 秒应补回 1000 字节");
        let mut tiny = ByteBucket::new(&Limits {
            data_byte_rate: 1,
            data_burst_bytes: 1,
            ..Limits::default()
        });
        assert!(!tiny.try_take(2), "单帧超额必须整帧拒绝，不半途截流");
    }

}
