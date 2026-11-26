//! 主种子 active seed 的真实宣告路径（BEP 5）。
//!
//! 顺序固定：`get_peers`（拿 token）→ `announce_peer`（带 token）→ 再 `get_peers` 回查。
//! **没有 token 就不发 announce**，并把原因如实报成 `NoToken`——公共引导节点若只回
//! 空 token/不回 token，这是可观测的真实失败，不是「已上 DHT」。

use std::net::SocketAddr;
use std::time::Duration;

use crate::{DhtClient, ID_LEN};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnnounceOutcome {
    /// 收到 announce_peer 应答；`peer_verified` 表示回查真的看到了自己登记的端口。
    Announced { token_len: usize, peer_verified: bool },
    /// 对端没有签发 token。公共节点常见；不得据此声称宣告成功。
    NoToken { reason: String },
    /// 某一阶段真实失败（超时 / 错误帧）。
    Failed { stage: &'static str, reason: String },
}

impl AnnounceOutcome {
    /// 机器可读的短状态，供 `/health.dht` 直接展示。
    pub fn status_label(&self) -> &'static str {
        match self {
            AnnounceOutcome::Announced { peer_verified: true, .. } => "announced_verified",
            AnnounceOutcome::Announced { .. } => "announced_unverified",
            AnnounceOutcome::NoToken { .. } => "token_unavailable",
            AnnounceOutcome::Failed { .. } => "failed",
        }
    }
}

/// 纯判定：get_peers 是否拿到了可用于 announce 的 token。
pub fn token_from_get_peers(token: Option<Vec<u8>>) -> Result<Vec<u8>, AnnounceOutcome> {
    match token {
        Some(t) if !t.is_empty() => Ok(t),
        _ => Err(AnnounceOutcome::NoToken {
            reason: "get_peers 未签发 token；不执行 announce_peer".into(),
        }),
    }
}

/// 对一个活跃种子做一次完整宣告。所有字节都来自真实的 KRPC 交换。
pub fn announce_active_seed(
    client: &DhtClient,
    bootstrap: SocketAddr,
    info_hash: [u8; ID_LEN],
    port: u16,
    timeout: Duration,
) -> AnnounceOutcome {
    let (token, _, _) = match client.get_peers(bootstrap, info_hash, timeout) {
        Ok(v) => v,
        Err(e) => return AnnounceOutcome::Failed { stage: "get_peers", reason: e },
    };
    let token = match token_from_get_peers(token) {
        Ok(t) => t,
        Err(outcome) => return outcome,
    };
    let token_len = token.len();
    if let Err(e) = client.announce_peer(bootstrap, info_hash, port, token, timeout) {
        return AnnounceOutcome::Failed { stage: "announce_peer", reason: e };
    }
    let peer_verified = match client.get_peers(bootstrap, info_hash, timeout) {
        Ok((_, _, values)) => values.iter().any(|a| a.port() == port),
        Err(_) => false, // ACK 是真的，但回查失败时不谎报 verified。
    };
    AnnounceOutcome::Announced { token_len, peer_verified }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DhtNode, NodeId, ServerHandle};
    use std::sync::Arc;
    use std::time::Duration;

    const T: Duration = Duration::from_millis(1500);

    #[test]
    fn missing_token_is_reported_not_faked() {
        assert_eq!(
            token_from_get_peers(None),
            Err(AnnounceOutcome::NoToken { reason: "get_peers 未签发 token；不执行 announce_peer".into() })
        );
        assert!(token_from_get_peers(Some(Vec::new())).is_err());
        let ok = token_from_get_peers(Some(vec![1, 2, 3])).unwrap();
        assert_eq!(ok.len(), 3);
        assert_eq!(
            AnnounceOutcome::NoToken { reason: "x".into() }.status_label(),
            "token_unavailable"
        );
    }

    #[test]
    fn loopback_full_announce_path_is_verified() {
        let node = Arc::new(DhtNode::new(NodeId::random()));
        let server = ServerHandle::spawn(node.clone()).expect("spawn dht server");
        let client = DhtClient::bind("127.0.0.1:0", NodeId::random()).expect("client bind");
        let ih = [0x33u8; ID_LEN];
        let port = client.local_addr().unwrap().port();
        let out = announce_active_seed(&client, server.addr, ih, port, T);
        assert_eq!(out, AnnounceOutcome::Announced { token_len: 8, peer_verified: true });
        assert_eq!(out.status_label(), "announced_verified");
        assert_eq!(node.stats().announces_stored, 1);
    }

    #[test]
    fn unreachable_bootstrap_reports_real_failure() {
        let client = DhtClient::bind("127.0.0.1:0", NodeId::random()).expect("client bind");
        // 一个没人监听的回环端口：真实超时，不得返回 announced。
        let blackhole: SocketAddr = "127.0.0.1:9".parse().unwrap();
        let out = announce_active_seed(&client, blackhole, [0x44u8; ID_LEN], 6881,
                                       Duration::from_millis(120));
        assert_eq!(out.status_label(), "failed");
        match out {
            AnnounceOutcome::Failed { stage, .. } => assert_eq!(stage, "get_peers"),
            other => panic!("期望真实失败，得到 {other:?}"),
        }
    }
}
