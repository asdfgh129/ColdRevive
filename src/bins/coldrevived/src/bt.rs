//! 本地 tracker 的响应编码（W5.2 起只做这一件事）。
//!
//! 历史：M1 这里还有一套手写的 Peer Wire 种子端（握手 + bitfield + request/piece），
//! 用来给第三方 aria2 供片。W5.2 按 [ADR-015](../../../../docs/02-architecture/adr/ADR-015-dependency-layering.md)
//! 把做种交给 `cr-engine`（librqbit）后已删除——**生产环境只保留一条做种路径**，
//! 两套实现并存必然行为漂移。
//!
//! 仍然 std-only：tracker 只是按 query 拼 bencode，没有引入第三方依赖的必要。
//! 这里**不实现私有协议**：回答的是标准 HTTP tracker 请求，对端是标准客户端。

/// 本地 tracker 的 bencode 响应。按客户端 `compact` 参数返回紧凑或非紧凑 peer 列表。
///
/// M2：`peer=None` 表示「没有可向该请求方通告的合法端点」，此时返回**空 peer 列表**，
/// 不把私网地址发给公网对端，也不捏造占位地址。
pub fn tracker_response(query: &str, ip: &str, port: u16) -> Vec<u8> {
    tracker_response_optional(query, Some((ip, port)))
}

pub fn tracker_response_optional(query: &str, peer: Option<(&str, u16)>) -> Vec<u8> {
    let compact = query.split('&').any(|kv| kv == "compact=1")
        || !query.split('&').any(|kv| kv.starts_with("compact="));
    let mut out = Vec::new();
    out.extend_from_slice(b"d8:completei1e10:incompletei0e8:intervali1800e");
    match (compact, peer) {
        (true, Some((ip, port))) => {
            let mut peers = Vec::new();
            for part in ip.split('.') { peers.push(part.parse::<u8>().unwrap_or(127)); }
            while peers.len() < 4 { peers.push(127); }
            peers.extend_from_slice(&port.to_be_bytes());
            out.extend_from_slice(format!("5:peers{}:", peers.len()).as_bytes());
            out.extend_from_slice(&peers);
        }
        (true, None) => out.extend_from_slice(b"5:peers0:"),
        (false, Some((ip, port))) => {
            out.extend_from_slice(b"5:peers");
            out.extend_from_slice(format!(
                "ld2:ip{}:{}4:porti{}eee", ip.len(), ip, port
            ).as_bytes());
        }
        (false, None) => out.extend_from_slice(b"5:peersle"),
    }
    out.push(b'e');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_tracker_response_is_valid_bencode_prefix() {
        let r = tracker_response("compact=1&info_hash=x", "127.0.0.1", 16802);
        let s = String::from_utf8_lossy(&r);
        assert!(s.starts_with("d8:completei1e"));
        assert!(s.contains("5:peers6:"));
        assert_eq!(r.last(), Some(&b'e'));
    }

    #[test]
    fn no_peer_yields_empty_bencode_peer_list() {
        let compact = tracker_response_optional("compact=1", None);
        assert!(String::from_utf8_lossy(&compact).contains("5:peers0:"));
        assert_eq!(compact.last(), Some(&b'e'));
        let plain = tracker_response_optional("compact=0", None);
        assert!(String::from_utf8_lossy(&plain).contains("5:peersle"));
        // 旧接口字节级不变
        assert_eq!(tracker_response("compact=1", "127.0.0.1", 16802),
                   tracker_response_optional("compact=1", Some(("127.0.0.1", 16802))));
    }

    #[test]
    fn noncompact_tracker_response_contains_peer_dict() {
        let r = tracker_response("compact=0", "127.0.0.1", 16802);
        let s = String::from_utf8_lossy(&r);
        assert!(s.contains("ld2:ip9:127.0.0.14:porti16802eee"));
    }
}
