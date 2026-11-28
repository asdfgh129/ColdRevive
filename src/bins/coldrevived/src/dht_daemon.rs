//! FR-18：opt-in 的 Mainline DHT daemon 接线。
//!
//! 每个活跃种子走真实的 `find_node → get_peers → announce_peer → 回查` 路径
//! （见 `cr_api::dht::announce`）。状态只报真实结果：拿到 token 且回查见自己才叫
//! `announced_verified`；公共邻居不给 token 就如实记 `token_unavailable`，
//! **不宣称公网可发现、也不改任何门禁常量**。
//!
//! v2.4.116 资源治理：有 stop 句柄、每轮宣告/发现预算、请求间隔、key 上限与
//! RSS 软跳过。网络 I/O 不再在 `NodeState` 锁内执行（旧实现会让慢查询卡住 `/health`）。

use std::collections::{HashSet, VecDeque};
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cr_core::Json;

use crate::p1_config::P1Config;
use crate::NodeState;

/// DOC-35 决定 3 选 B：公共 DHT 宣告闸门的进程级句柄。
/// 由 `spawn` 初始化，`/cli dht announce-off|on` 与 `/health` 共用同一份状态。
static PUBLIC_GATE: OnceLock<Arc<cr_api::dht::PublicAnnounceGate>> = OnceLock::new();

/// 取闸门句柄（未启用 DHT 时为 None——那是「没装闸门」，不是「闸门放行」）。
pub fn public_gate() -> Option<Arc<cr_api::dht::PublicAnnounceGate>> {
    PUBLIC_GATE.get().cloned()
}

/// 每轮最多从迭代查询中新增的候选节点数（硬上限，防拓扑遍历失控）。
const MAX_DISCOVERED: usize = 32;
/// BFS 深度上限：2 跳足够越过「引导节点不发 token」这一层。
const DISCOVER_DEPTH: u32 = 2;

#[derive(Debug, Clone, Copy)]
pub struct DhtRunConfig {
    pub timeout: Duration,
    pub refresh: Duration,
    pub max_bootstraps: usize,
    pub max_seeds: usize,
    pub max_keys: usize,
    pub max_announces_per_cycle: usize,
    pub min_interval: Duration,
    pub rss_soft_skip_mb: u64,
}

impl From<&P1Config> for DhtRunConfig {
    fn from(p: &P1Config) -> Self {
        Self {
            timeout: Duration::from_millis(p.dht_timeout_ms),
            refresh: Duration::from_secs(p.dht_refresh_secs),
            max_bootstraps: p.dht_max_bootstraps,
            max_seeds: p.dht_max_seeds,
            max_keys: p.dht_max_keys,
            max_announces_per_cycle: p.dht_max_announces_per_cycle,
            min_interval: Duration::from_millis(p.dht_min_interval_ms),
            rss_soft_skip_mb: p.dht_rss_soft_skip_mb,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DhtStatus {
    pub enabled: bool,
    pub local_addr: Option<String>,
    pub bootstrap: Vec<String>,
    pub seed_count: usize,
    pub attempts: usize,
    pub verified: usize,
    pub no_token: usize,
    pub failed: usize,
    pub last_error: String,
    // v2.4.116：非黏性运行状态与资源治理计数。
    pub cycles: u64,
    /// 实际发出的宣告尝试次数（每次尝试内部仍是真实 get_peers→announce→get_peers）。
    pub requests: u64,
    pub discovered: usize,
    pub resource_skipped: u64,
    pub last_cycle_at_ms: u64,
    /// 只描述**最近一轮**，不再用历史 verified 永久盖住后来的失败。
    pub last_cycle_status: String,
    // DOC-35 决定 3：公共宣告闸门（默认关闭）。
    pub public_enabled: bool,
    pub public_killed: bool,
    pub public_sent: u64,
    pub public_refused: u64,
    pub public_last_refusal: Option<&'static str>,
}

impl DhtStatus {
    pub fn disabled() -> Self {
        Self {
            enabled: false, local_addr: None, bootstrap: Vec::new(), seed_count: 0,
            attempts: 0, verified: 0, no_token: 0, failed: 0, last_error: String::new(),
            cycles: 0, requests: 0, discovered: 0, resource_skipped: 0,
            last_cycle_at_ms: 0, last_cycle_status: String::new(),
            public_enabled: false, public_killed: false,
            public_sent: 0, public_refused: 0, public_last_refusal: None,
        }
    }

    pub fn status_label(&self) -> &'static str {
        if !self.enabled { return "disabled"; }
        if self.resource_skipped > 0 && self.last_cycle_status == "resource_skipped" {
            return "resource_skipped";
        }
        match self.last_cycle_status.as_str() {
            "announced_verified" => "announced_verified",
            "token_unavailable" => "token_unavailable",
            "failed" => "failed",
            "no_candidates" => "no_candidates",
            "idle" => "idle",
            // 老配置文件/旧状态没有 last_cycle_status 时退回累计口径。
            _ if self.verified > 0 => "announced_verified",
            _ if self.no_token > 0 => "token_unavailable",
            _ if self.failed > 0 => "failed",
            _ => "no_bootstrap",
        }
    }

    pub fn json(&self) -> Json {
        Json::obj(vec![
            ("enabled", Json::b(self.enabled)),
            ("status", Json::s(self.status_label())),
            ("last_cycle_status", Json::s(&self.last_cycle_status)),
            ("local_addr", match &self.local_addr {
                Some(a) => Json::s(a), None => Json::Null }),
            ("bootstrap", Json::arr(self.bootstrap.iter()
                .map(|s| Json::s(s)).collect())),
            ("seeds", Json::n(self.seed_count as f64)),
            ("attempts", Json::n(self.attempts as f64)),
            ("verified", Json::n(self.verified as f64)),
            ("no_token", Json::n(self.no_token as f64)),
            ("failed", Json::n(self.failed as f64)),
            ("cycles", Json::n(self.cycles as f64)),
            ("requests", Json::n(self.requests as f64)),
            ("discovered", Json::n(self.discovered as f64)),
            ("resource_skipped", Json::n(self.resource_skipped as f64)),
            ("last_cycle_at_ms", Json::n(self.last_cycle_at_ms as f64)),
            ("last_error", Json::s(&self.last_error)),
            ("public_announce", Json::obj(vec![
                ("enabled", Json::b(self.public_enabled)),
                ("killed", Json::b(self.public_killed)),
                ("sent", Json::n(self.public_sent as f64)),
                ("refused", Json::n(self.public_refused as f64)),
                ("last_refusal", match self.public_last_refusal {
                    Some(r) => Json::s(r), None => Json::Null }),
                ("note", Json::s("公共宣告授权闸门：默认关闭，开启后仍受每分钟/每日配额约束")),
            ])),
            ("note", Json::s("本地 Mainline DHT 宣告；不代表公网可发现，不改 S3/S6 判据")),
        ])
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Linux 上用 `/proc/self/statm` 做 RSS 软判断（4 KiB/页）。非 Linux 读到 None
/// 时**不参与判定**，绝不当 0 让治理误以为内存充足。
fn rss_mb() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = text.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages.saturating_mul(4096) / (1024 * 1024))
}

fn pace(status: &mut DhtStatus, cfg: &DhtRunConfig) {
    if !cfg.min_interval.is_zero() {
        thread::sleep(cfg.min_interval);
    }
    status.requests += 1;
}

/// 有界的迭代 `find_node`：引导节点通常不发 token，必须先走到真正签发 token 的邻居。
/// 只做只读查询；发现到的节点仍要经过 `announce_active_seed` 的 token 门。
fn discover_contacts(
    client: &cr_api::dht::DhtClient,
    node: &cr_api::dht::DhtNode,
    bootstraps: &[SocketAddr],
    status: &mut DhtStatus,
    cfg: &DhtRunConfig,
) -> Vec<SocketAddr> {
    let mut queue: VecDeque<(SocketAddr, u32)> = bootstraps.iter()
        .copied().map(|a| (a, 0)).collect();
    let mut seen: HashSet<SocketAddr> = HashSet::new();
    // 只有真正应答 find_node 的节点才进入宣告候选：旧实现对 ping 失败者仍发 announce，
    // 会把「离线」误报成「尝试过但没 token」。
    let mut candidates = Vec::new();
    let budget = cfg.max_announces_per_cycle.min(MAX_DISCOVERED);
    while let Some((addr, depth)) = queue.pop_front() {
        if candidates.len() >= budget || status.requests >= budget as u64 {
            break;
        }
        if !seen.insert(addr) {
            continue;
        }
        match client.find_node(addr, node.id(), cfg.timeout) {
            Ok(contacts) => {
                pace(status, cfg);
                if !candidates.contains(&addr) {
                    candidates.push(addr);
                }
                for c in contacts {
                    if !c.addr.is_ipv4() || !seen.insert(c.addr) {
                        continue;
                    }
                    if depth + 1 <= DISCOVER_DEPTH {
                        queue.push_back((c.addr, depth + 1));
                    }
                    if candidates.len() < budget {
                        candidates.push(c.addr);
                    }
                }
            }
            Err(e) => {
                status.failed += 1;
                status.last_error = format!("find_node {addr}: {e}");
            }
        }
    }
    candidates
}

/// 一轮真实宣告。返回是否有在本轮内被回查确认的种子。
fn run_cycle(
    client: &cr_api::dht::DhtClient,
    node: &cr_api::dht::DhtNode,
    bootstraps: &[SocketAddr],
    hashes: &[[u8; cr_api::dht::ID_LEN]],
    port: u16,
    status: &mut DhtStatus,
    cfg: &DhtRunConfig,
    gate: &cr_api::dht::PublicAnnounceGate,
) {
    status.cycles += 1;
    status.last_cycle_at_ms = now_ms();
    let seeds = &hashes[..hashes.len().min(cfg.max_seeds)];
    let boot = &bootstraps[..bootstraps.len().min(cfg.max_bootstraps)];
    if boot.is_empty() || seeds.is_empty() {
        status.last_cycle_status = "no_candidates".into();
        return;
    }

    let failed_before = status.failed;
    let candidates = discover_contacts(client, node, boot, status, cfg);
    let discovery_failed = status.failed - failed_before;
    status.discovered = candidates.len().saturating_sub(boot.len());
    if candidates.is_empty() {
        status.last_cycle_status = if discovery_failed > 0 { "failed" } else { "no_candidates" }.into();
        return;
    }

    let mut cycle_verified = 0usize;
    let mut cycle_no_token = 0usize;
    let mut cycle_failed = 0usize;
    let mut used = 0usize;
    'outer: for addr in candidates {
        for ih in seeds {
            if used >= cfg.max_announces_per_cycle {
                break 'outer;
            }
            // DOC-35 决定 3：公共地址必须先过闸门；内网/回环/自建测试节点不过闸门
            // （那本来就不是「公共 DHT announce」）。
            if cr_api::dht::public_gate::is_public_addr(addr) {
                if let Err(r) = gate.try_acquire(ih, SystemTime::now()) {
                    status.public_refused += 1;
                    status.public_last_refusal = Some(r.label());
                    // 这四种拒绝都意味着「本地址剩余 key 再试也没用」，于是跳过该地址、
                    // 换下一个候选（不能 `break 'outer`：后面的私有候选仍应正常宣告）。
                    match r {
                        cr_api::dht::Refusal::MinuteRateLimit
                        | cr_api::dht::Refusal::DailyCap
                        | cr_api::dht::Refusal::Killed
                        | cr_api::dht::Refusal::Disabled => continue 'outer,
                    }
                }
                status.public_sent += 1;
            }
            used += 1;
            pace(status, cfg);
            match cr_api::dht::announce_active_seed(client, addr, *ih, port, cfg.timeout) {
                cr_api::dht::AnnounceOutcome::Announced { peer_verified, .. } => {
                    status.attempts += 1;
                    if peer_verified {
                        status.verified += 1;
                        cycle_verified += 1;
                    }
                }
                cr_api::dht::AnnounceOutcome::NoToken { reason } => {
                    status.attempts += 1;
                    status.no_token += 1;
                    cycle_no_token += 1;
                    status.last_error = reason;
                }
                cr_api::dht::AnnounceOutcome::Failed { stage, reason } => {
                    status.attempts += 1;
                    status.failed += 1;
                    cycle_failed += 1;
                    status.last_error = format!("{stage}: {reason}");
                }
            }
        }
    }
    status.last_cycle_status = if cycle_verified > 0 {
        "announced_verified"
    } else if cycle_no_token > 0 {
        "token_unavailable"
    } else if cycle_failed > 0 {
        "failed"
    } else {
        "idle"
    }.to_string();
}

/// 把索引里的 40 位十六进制 infohash 变成真实 20 字节；非法值直接丢弃。
pub fn parse_infohash(s: &str) -> Option<[u8; cr_api::dht::ID_LEN]> {
    let bytes = cr_hash::unhex(s)?;
    if bytes.len() != cr_api::dht::ID_LEN { return None; }
    let mut out = [0u8; cr_api::dht::ID_LEN];
    out.copy_from_slice(&bytes);
    Some(out)
}

fn parse_bootstraps(list: &[String], max: usize) -> Vec<SocketAddr> {
    list.iter().filter_map(|s| s.to_socket_addrs().ok())
        .flatten().filter(|a| a.is_ipv4()).take(max).collect()
}

/// 可停止的后台线程句柄。drop 或 `stop()` 都会通知循环退出；不强制 kill 线程，
/// 因为 socket 读超时有上限，线程会在本轮 I/O 后自然收尾。
#[allow(dead_code)] // 生命周期 API：当前主循环到进程结束才退出，测试/未来优雅关闭会调用。
pub struct DhtHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl DhtHandle {
    #[allow(dead_code)]
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for DhtHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn sleep_or_stop(stop: &AtomicBool, total: Duration) {
    let step = Duration::from_millis(100);
    let mut left = total;
    while left > Duration::ZERO && !stop.load(Ordering::Relaxed) {
        let this = left.min(step);
        thread::sleep(this);
        left = left.saturating_sub(this);
    }
}

/// 启动后台宣告线程。未启用时完全不建 socket、不发包。
pub fn spawn(
    state: Arc<Mutex<NodeState>>,
    p1: P1Config,
    hashes: Vec<[u8; cr_api::dht::ID_LEN]>,
    port: u16,
) -> Option<DhtHandle> {
    if !p1.dht_enabled { return None; }
    let cfg = DhtRunConfig::from(&p1);
    // 闸门在这里落地：即使 `dht.enabled=true`，公共宣告仍要 `announce_public=true`。
    let gate = Arc::new(cr_api::dht::PublicAnnounceGate::new(cr_api::dht::GateConfig {
        enabled: p1.dht_announce_public,
        max_per_min: p1.dht_public_max_per_min,
        daily_cap: p1.dht_public_daily_cap,
    }));
    let _ = PUBLIC_GATE.set(gate.clone());
    let cycle_gate = gate.clone();
    {
        let mut st = state.lock().unwrap();
        st.dht.enabled = true;
        st.dht.bootstrap = p1.dht_bootstrap.clone();
        st.dht.seed_count = hashes.len().min(cfg.max_seeds);
        st.dht.public_enabled = p1.dht_announce_public;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let srv_stop = stop.clone();
    let st_stop = stop.clone();
    let join = thread::Builder::new()
        .name("cr-dht-announce".into())
        .spawn(move || {
            let node = Arc::new(cr_api::dht::DhtNode::new_with_limits(
                cr_api::dht::NodeId::random(), cfg.max_keys,
            ));
            let server = match cr_api::dht::DhtServer::bind("0.0.0.0:0", node.clone()) {
                Ok(s) => s,
                Err(e) => {
                    let mut st = state.lock().unwrap();
                    st.dht.failed += 1;
                    st.dht.last_cycle_status = "failed".into();
                    st.dht.last_error = format!("DHT server bind: {e}");
                    return;
                }
            };
            if let Ok(a) = server.local_addr() {
                state.lock().unwrap().dht.local_addr = Some(a.to_string());
            }
            let client = match cr_api::dht::DhtClient::bind("0.0.0.0:0", cr_api::dht::NodeId::random()) {
                Ok(c) => c,
                Err(e) => {
                    let mut st = state.lock().unwrap();
                    st.dht.failed += 1;
                    st.dht.last_cycle_status = "failed".into();
                    st.dht.last_error = format!("DHT client bind: {e}");
                    return;
                }
            };
            let bootstraps = parse_bootstraps(&p1.dht_bootstrap, cfg.max_bootstraps);
            thread::spawn(move || server.run(&srv_stop));

            // 网络 I/O 在锁外做；只在写回快照时短暂持锁，避免慢查询卡住 /health。
            while !st_stop.load(Ordering::Relaxed) {
                if cfg.rss_soft_skip_mb > 0
                    && rss_mb().map(|m| m > cfg.rss_soft_skip_mb).unwrap_or(false)
                {
                    let mut st = state.lock().unwrap();
                    st.dht.resource_skipped += 1;
                    st.dht.last_cycle_status = "resource_skipped".into();
                    st.dht.last_cycle_at_ms = now_ms();
                    sleep_or_stop(&st_stop, cfg.refresh);
                    continue;
                }
                let mut snapshot = {
                    let st = state.lock().unwrap();
                    st.dht.clone()
                };
                run_cycle(
                    &client, &node, &bootstraps, &hashes, port, &mut snapshot, &cfg, &cycle_gate,
                );
                snapshot.public_killed = cycle_gate.killed();
                {
                    let mut st = state.lock().unwrap();
                    st.dht = snapshot;
                }
                sleep_or_stop(&st_stop, cfg.refresh);
            }
        })
        .ok();
    join.map(|j| DhtHandle { stop, join: Some(j) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_api::dht::{DhtNode, NodeId, ServerHandle};

    fn test_status() -> DhtStatus {
        DhtStatus {
            enabled: true, local_addr: None, bootstrap: Vec::new(), seed_count: 1,
            attempts: 0, verified: 0, no_token: 0, failed: 0, last_error: String::new(),
            cycles: 0, requests: 0, discovered: 0, resource_skipped: 0,
            last_cycle_at_ms: 0, last_cycle_status: String::new(),
            public_enabled: false, public_killed: false,
            public_sent: 0, public_refused: 0, public_last_refusal: None,
        }
    }

    /// 测试用闸门：**故意用默认（关闭）配置**。回环地址不过闸门，
    /// 所以这些用例仍然走真实 announce；一旦哪天有人把闸门误加到内网路径上，
    /// 这些用例会立刻失败——这正是我们要的护栏。
    fn test_gate() -> cr_api::dht::PublicAnnounceGate {
        cr_api::dht::PublicAnnounceGate::new(cr_api::dht::GateConfig::default())
    }

    fn test_cfg() -> DhtRunConfig {
        DhtRunConfig {
            timeout: Duration::from_millis(1500),
            refresh: Duration::from_secs(1),
            max_bootstraps: 8,
            max_seeds: 50,
            max_keys: 512,
            max_announces_per_cycle: 16,
            min_interval: Duration::ZERO,
            rss_soft_skip_mb: 0,
        }
    }

    #[test]
    fn disabled_status_is_explicit_and_silent() {
        let s = DhtStatus::disabled();
        assert_eq!(s.status_label(), "disabled");
        let j = s.json();
        assert_eq!(j.get("enabled").and_then(Json::as_bool), Some(false));
        assert_eq!(j.get("status").and_then(Json::as_str), Some("disabled"));
        assert_eq!(DhtStatus::disabled().attempts, 0);
    }

    #[test]
    fn loopback_cycle_announces_real_active_seed() {
        let node = Arc::new(DhtNode::new(NodeId::random()));
        let server = ServerHandle::spawn(node.clone()).unwrap();
        let client = cr_api::dht::DhtClient::bind("127.0.0.1:0", NodeId::random()).unwrap();
        let port = client.local_addr().unwrap().port();
        let hashes = [[0x55u8; cr_api::dht::ID_LEN]];
        let mut status = test_status();
        status.bootstrap = vec![server.addr.to_string()];
        run_cycle(&client, &node, &[server.addr], &hashes, port, &mut status, &test_cfg(), &test_gate());
        assert_eq!(status.attempts, 1);
        assert_eq!(status.verified, 1);
        assert_eq!(status.last_cycle_status, "announced_verified");
        assert_eq!(status.status_label(), "announced_verified");
        assert_eq!(node.stats().announces_stored, 1);
    }

    #[test]
    fn unreachable_bootstrap_is_recorded_as_failure() {
        let node = Arc::new(DhtNode::new(NodeId::random()));
        let client = cr_api::dht::DhtClient::bind("127.0.0.1:0", NodeId::random()).unwrap();
        let mut status = test_status();
        status.bootstrap = vec!["127.0.0.1:9".into()];
        let cfg = DhtRunConfig { timeout: Duration::from_millis(120), ..test_cfg() };
        run_cycle(&client, &node, &["127.0.0.1:9".parse().unwrap()],
                  &[[0x66u8; cr_api::dht::ID_LEN]], 6881, &mut status, &cfg, &test_gate());
        assert_eq!(status.attempts, 0);
        assert_eq!(status.failed, 1);
        assert_eq!(status.last_cycle_status, "failed");
        assert_eq!(status.status_label(), "failed");
        assert!(status.last_error.contains("find_node"));
    }

    #[test]
    fn cycle_budget_caps_announce_attempts() {
        let node = Arc::new(DhtNode::new(NodeId::random()));
        let server = ServerHandle::spawn(node.clone()).unwrap();
        let client = cr_api::dht::DhtClient::bind("127.0.0.1:0", NodeId::random()).unwrap();
        let mut status = test_status();
        let cfg = DhtRunConfig { max_announces_per_cycle: 2, ..test_cfg() };
        let hashes = [[0x77u8; cr_api::dht::ID_LEN]; 9];
        run_cycle(&client, &node, &[server.addr], &hashes, 6881, &mut status, &cfg, &test_gate());
        assert!(status.attempts <= 2, "预算必须硬封顶，实际 {}", status.attempts);
        assert!(status.requests <= 4, "find_node 与 announce 总请求也应受预算约束");
    }

    #[test]
    fn last_cycle_status_is_not_sticky() {
        let mut s = test_status();
        s.verified = 1;
        s.last_cycle_status = "token_unavailable".into();
        assert_eq!(s.status_label(), "token_unavailable",
            "历史成功不能永久盖住本轮失败");
        s.last_cycle_status = "announced_verified".into();
        assert_eq!(s.status_label(), "announced_verified");
    }

    #[test]
    fn run_config_follows_p1_limits() {
        let mut p = P1Config::default();
        p.dht_timeout_ms = 123;
        p.dht_max_keys = 7;
        p.dht_max_announces_per_cycle = 3;
        p.dht_min_interval_ms = 11;
        let c = DhtRunConfig::from(&p);
        assert_eq!(c.timeout, Duration::from_millis(123));
        assert_eq!(c.max_keys, 7);
        assert_eq!(c.max_announces_per_cycle, 3);
        assert_eq!(c.min_interval, Duration::from_millis(11));
    }
}
