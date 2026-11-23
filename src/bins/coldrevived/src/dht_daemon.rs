//! FR-18：opt-in 的 Mainline DHT daemon 接线。
//!
//! 每个活跃种子走真实的 `get_peers → announce_peer → 回查` 路径（见 cr-dht::announce）。
//! 状态只报真实结果：拿到 token 且回查见自己才叫 `announced_verified`；公共邻居不给
//! token 就如实记 `token_unavailable`，**不宣称公网可发现、也不改任何门禁常量**。

use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cr_core::Json;

use crate::p1_config::P1Config;
use crate::NodeState;

const DHT_TIMEOUT: Duration = Duration::from_secs(2);
const REFRESH: Duration = Duration::from_secs(600);

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
}

impl DhtStatus {
    pub fn disabled() -> Self {
        Self {
            enabled: false, local_addr: None, bootstrap: Vec::new(), seed_count: 0,
            attempts: 0, verified: 0, no_token: 0, failed: 0, last_error: String::new(),
        }
    }

    pub fn status_label(&self) -> &'static str {
        if !self.enabled { return "disabled"; }
        if self.verified > 0 { "announced_verified" }
        else if self.no_token > 0 { "token_unavailable" }
        else if self.failed > 0 { "failed" }
        else if self.attempts == 0 { "no_bootstrap" }
        else { "idle" }
    }

    pub fn json(&self) -> Json {
        Json::obj(vec![
            ("enabled", Json::b(self.enabled)),
            ("status", Json::s(self.status_label())),
            ("local_addr", match &self.local_addr {
                Some(a) => Json::s(a), None => Json::Null }),
            ("bootstrap", Json::arr(self.bootstrap.iter()
                .map(|s| Json::s(s)).collect())),
            ("seeds", Json::n(self.seed_count as f64)),
            ("attempts", Json::n(self.attempts as f64)),
            ("verified", Json::n(self.verified as f64)),
            ("no_token", Json::n(self.no_token as f64)),
            ("failed", Json::n(self.failed as f64)),
            ("last_error", Json::s(&self.last_error)),
            ("note", Json::s("本地 Mainline DHT 宣告；不代表公网可发现，不改 S3/S6 判据")),
        ])
    }
}

/// 一轮真实宣告。返回更新后的计数（不访问网络以外的任何“假数据”）。
fn run_cycle(
    client: &cr_api::dht::DhtClient,
    node: &cr_api::dht::DhtNode,
    bootstraps: &[SocketAddr],
    hashes: &[[u8; cr_api::dht::ID_LEN]],
    port: u16,
    status: &mut DhtStatus,
    timeout: Duration,
) {
    // ping 学到对端真实 ID，并确认它确实在线；不通就不发 announce。
    for addr in bootstraps {
        if let Ok(id) = client.ping(*addr, timeout) {
            node.seed_contact(cr_api::dht::Contact { id, addr: *addr });
        } else {
            status.failed += 1;
            status.last_error = format!("ping {addr} 失败");
            continue;
        }
        for ih in hashes {
            match cr_api::dht::announce_active_seed(client, *addr, *ih, port, timeout) {
                cr_api::dht::AnnounceOutcome::Announced { peer_verified, .. } => {
                    status.attempts += 1;
                    if peer_verified { status.verified += 1; }
                }
                cr_api::dht::AnnounceOutcome::NoToken { reason } => {
                    status.attempts += 1;
                    status.no_token += 1;
                    status.last_error = reason;
                }
                cr_api::dht::AnnounceOutcome::Failed { stage, reason } => {
                    status.attempts += 1;
                    status.failed += 1;
                    status.last_error = format!("{stage}: {reason}");
                }
            }
        }
    }
}

/// 把索引里的 40 位十六进制 infohash 变成真实 20 字节；非法值直接丢弃。
pub fn parse_infohash(s: &str) -> Option<[u8; cr_api::dht::ID_LEN]> {
    let bytes = cr_hash::unhex(s)?;
    if bytes.len() != cr_api::dht::ID_LEN { return None; }
    let mut out = [0u8; cr_api::dht::ID_LEN];
    out.copy_from_slice(&bytes);
    Some(out)
}

fn parse_bootstraps(list: &[String]) -> Vec<SocketAddr> {
    list.iter().filter_map(|s| s.to_socket_addrs().ok())
        .flatten().filter(|a| a.is_ipv4()).collect()
}

/// 启动后台宣告线程。未启用时完全不建 socket、不发包。
pub fn spawn(state: Arc<Mutex<NodeState>>, p1: P1Config, hashes: Vec<[u8; cr_api::dht::ID_LEN]>, port: u16) {
    if !p1.dht_enabled { return; }
    {
        let mut st = state.lock().unwrap();
        st.dht.enabled = true;
        st.dht.bootstrap = p1.dht_bootstrap.clone();
        st.dht.seed_count = hashes.len();
    }
    thread::spawn(move || {
        let node = Arc::new(cr_api::dht::DhtNode::new(cr_api::dht::NodeId::random()));
        let server = match cr_api::dht::DhtServer::bind("0.0.0.0:0", node.clone()) {
            Ok(s) => s,
            Err(e) => {
                let mut st = state.lock().unwrap();
                st.dht.failed += 1;
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
                st.dht.last_error = format!("DHT client bind: {e}");
                return;
            }
        };
        let bootstraps = parse_bootstraps(&p1.dht_bootstrap);
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let srv_stop = stop.clone();
        thread::spawn(move || server.run(&srv_stop));
        loop {
            {
                let mut st = state.lock().unwrap();
                let mut snapshot = st.dht.clone();
                run_cycle(&client, &node, &bootstraps, &hashes, port, &mut snapshot, DHT_TIMEOUT);
                st.dht = snapshot;
            }
            thread::sleep(REFRESH);
            if stop.load(std::sync::atomic::Ordering::Relaxed) { break; }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_api::dht::{DhtNode, NodeId, ServerHandle};

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
        let mut status = DhtStatus {
            enabled: true, local_addr: None, bootstrap: vec![server.addr.to_string()],
            seed_count: 1, attempts: 0, verified: 0, no_token: 0, failed: 0,
            last_error: String::new(),
        };
        run_cycle(&client, &node, &[server.addr], &hashes, port, &mut status,
                  Duration::from_millis(1500));
        assert_eq!(status.attempts, 1);
        assert_eq!(status.verified, 1);
        assert_eq!(status.status_label(), "announced_verified");
        assert_eq!(node.stats().announces_stored, 1);
    }

    #[test]
    fn unreachable_bootstrap_is_recorded_as_failure() {
        let node = Arc::new(DhtNode::new(NodeId::random()));
        let client = cr_api::dht::DhtClient::bind("127.0.0.1:0", NodeId::random()).unwrap();
        let mut status = DhtStatus {
            enabled: true, local_addr: None, bootstrap: vec!["127.0.0.1:9".into()],
            seed_count: 1, attempts: 0, verified: 0, no_token: 0, failed: 0,
            last_error: String::new(),
        };
        run_cycle(&client, &node, &["127.0.0.1:9".parse().unwrap()],
                  &[[0x66u8; cr_api::dht::ID_LEN]], 6881, &mut status,
                  Duration::from_millis(120));
        assert_eq!(status.attempts, 0);
        assert_eq!(status.failed, 1);
        assert_eq!(status.status_label(), "failed");
        assert!(status.last_error.contains("ping"));
    }
}
