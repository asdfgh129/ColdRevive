//! KRPC 报文（BEP 5）：`ping` / `find_node` / `get_peers` / `announce_peer`。
//!
//! 只做**编解码与结构校验**，不做策略判断（token 是否有效、要不要收进路由表都在节点层）。
//! 边界划在这里的理由：报文格式是协议事实，策略是本地决定；混在一起会让
//! 「报文解析失败」和「报文合法但业务拒绝」在日志里长得一样。
//!
//! 严格点（都在测试里钉住）：
//! - `tid` 必须是 1～32 字节。空 `tid` 无法把应答对上请求；超长 `tid` 是廉价的内存放大器。
//! - `id` 必须恰好 20 字节。长度不对的报文一律当错误，不做截断/补零——补零会产生
//!   「全零 ID」这种会污染路由表的东西。
//! - 未知 `y` / 未知查询名由调用方决定怎么回（节点层回 BEP 5 的 204 错误）。

use crate::bencode::{self, dict, int, Value};
use crate::id::{decode_compact_nodes, encode_compact_nodes, Contact, NodeId, ID_LEN};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

pub const PING: &str = "ping";
pub const FIND_NODE: &str = "find_node";
pub const GET_PEERS: &str = "get_peers";
pub const ANNOUNCE_PEER: &str = "announce_peer";

/// `tid` 上限。BEP 5 没规定长度，但它是**对端提供的**、要回显的字节串。
pub const MAX_TID_LEN: usize = 32;

/// 一个查询。可选字段按查询类型使用，未使用的保持 `None`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub tid: Vec<u8>,
    pub name: String,
    pub id: NodeId,
    pub target: Option<NodeId>,
    pub info_hash: Option<[u8; ID_LEN]>,
    pub port: Option<u16>,
    pub implied_port: bool,
    pub token: Option<Vec<u8>>,
    /// `want` 里声明想要的编码（`n4`/`n6`）。只回显/解析，不影响本实现行为。
    pub want: Vec<String>,
}

impl Query {
    pub(crate) fn base(tid: Vec<u8>, name: &str, id: NodeId) -> Self {
        Self {
            tid,
            name: name.to_string(),
            id,
            target: None,
            info_hash: None,
            port: None,
            implied_port: false,
            token: None,
            want: Vec::new(),
        }
    }

    pub fn ping(tid: impl Into<Vec<u8>>, id: NodeId) -> Self {
        Self::base(tid.into(), PING, id)
    }

    pub fn find_node(tid: impl Into<Vec<u8>>, id: NodeId, target: NodeId) -> Self {
        let mut q = Self::base(tid.into(), FIND_NODE, id);
        q.target = Some(target);
        q
    }

    pub fn get_peers(tid: impl Into<Vec<u8>>, id: NodeId, info_hash: [u8; ID_LEN]) -> Self {
        let mut q = Self::base(tid.into(), GET_PEERS, id);
        q.info_hash = Some(info_hash);
        q
    }

    pub fn announce_peer(
        tid: impl Into<Vec<u8>>,
        id: NodeId,
        info_hash: [u8; ID_LEN],
        port: u16,
        token: Vec<u8>,
    ) -> Self {
        let mut q = Self::base(tid.into(), ANNOUNCE_PEER, id);
        q.info_hash = Some(info_hash);
        q.port = Some(port);
        q.token = Some(token);
        q
    }
}

/// 一个应答。`nodes` 与 `values` 按查询类型出现。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub tid: Vec<u8>,
    pub id: NodeId,
    pub token: Option<Vec<u8>>,
    pub nodes: Vec<Contact>,
    pub values: Vec<SocketAddr>,
}

impl Response {
    pub fn plain(tid: Vec<u8>, id: NodeId) -> Self {
        Self { tid, id, token: None, nodes: Vec::new(), values: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Query(Query),
    Response(Response),
    Error { tid: Vec<u8>, code: i64, message: String },
}

pub fn encode_query(q: &Query) -> Vec<u8> {
    let mut a: Vec<(&str, Value)> = vec![("id", bencode::bytes(q.id.as_bytes()))];
    if let Some(t) = &q.target {
        a.push(("target", bencode::bytes(t.as_bytes())));
    }
    if let Some(ih) = &q.info_hash {
        a.push(("info_hash", bencode::bytes(ih)));
    }
    if let Some(p) = q.port {
        a.push(("port", int(p as i64)));
    }
    if q.implied_port {
        a.push(("implied_port", int(1)));
    }
    if let Some(t) = &q.token {
        a.push(("token", bencode::bytes(t)));
    }
    if !q.want.is_empty() {
        a.push(("want", Value::List(q.want.iter().map(|w| bencode::bytes(w)).collect())));
    }
    dict(vec![
        ("t", bencode::bytes(&q.tid)),
        ("y", bencode::bytes("q")),
        ("q", bencode::bytes(&q.name)),
        ("a", dict(a)),
    ])
    .encode()
}

pub fn encode_response(r: &Response) -> Vec<u8> {
    let mut body: Vec<(&str, Value)> = vec![("id", bencode::bytes(r.id.as_bytes()))];
    if let Some(token) = &r.token {
        body.push(("token", bencode::bytes(token)));
    }
    if !r.nodes.is_empty() {
        body.push(("nodes", bencode::bytes(encode_compact_nodes(&r.nodes))));
    }
    if !r.values.is_empty() {
        body.push((
            "values",
            Value::List(r.values.iter().map(|a| bencode::bytes(compact_peer(a))).collect()),
        ));
    }
    dict(vec![
        ("t", bencode::bytes(&r.tid)),
        ("y", bencode::bytes("r")),
        ("r", dict(body)),
    ])
    .encode()
}

/// BEP 5 错误应答：`{"t":..,"y":"e","e":[code, message]}`。
pub fn encode_error(tid: &[u8], code: i64, message: &str) -> Vec<u8> {
    dict(vec![
        ("t", bencode::bytes(tid)),
        ("y", bencode::bytes("e")),
        ("e", Value::List(vec![int(code), bencode::bytes(message)])),
    ])
    .encode()
}

fn compact_peer(a: &SocketAddr) -> Vec<u8> {
    let mut out = Vec::with_capacity(6);
    match a.ip() {
        IpAddr::V4(v4) => out.extend_from_slice(&v4.octets()),
        IpAddr::V6(_) => out.extend_from_slice(&[0, 0, 0, 0]),
    }
    out.extend_from_slice(&a.port().to_be_bytes());
    out
}

fn decode_peer(buf: &[u8]) -> Option<SocketAddr> {
    if buf.len() != 6 {
        return None;
    }
    let port = u16::from_be_bytes([buf[4], buf[5]]);
    if port == 0 {
        return None;
    }
    let ip = Ipv4Addr::new(buf[0], buf[1], buf[2], buf[3]);
    if ip.is_unspecified() {
        return None;
    }
    Some(SocketAddr::from((ip, port)))
}

fn take_id(v: Option<&Value>, what: &str) -> Result<NodeId, String> {
    let b = v.and_then(Value::bytes).ok_or_else(|| format!("{what}: 缺 20 字节 ID"))?;
    if b.len() != ID_LEN {
        return Err(format!("{what}: ID 长度 {} 应为 {ID_LEN}", b.len()));
    }
    let mut id = [0u8; ID_LEN];
    id.copy_from_slice(b);
    Ok(NodeId(id))
}

fn take_tid(root: &Value) -> Result<Vec<u8>, String> {
    let tid = root.get("t").and_then(Value::bytes)
        .ok_or("KRPC: 缺 tid")?;
    if tid.is_empty() {
        return Err("KRPC: tid 为空".to_string());
    }
    if tid.len() > MAX_TID_LEN {
        return Err(format!("KRPC: tid 长度 {} 超过 {MAX_TID_LEN}", tid.len()));
    }
    Ok(tid.to_vec())
}

/// 解析一条 KRPC 报文。
pub fn parse(data: &[u8]) -> Result<Message, String> {
    let root = bencode::decode(data)?;
    let tid = take_tid(&root)?;
    let y = root.get("y").and_then(Value::as_str).ok_or("KRPC: 缺 y")?;
    match y {
        "q" => {
            let name = root.get("q").and_then(Value::as_str)
                .ok_or("KRPC: 查询缺 q（方法名）")?.to_string();
            let a = root.get("a").ok_or("KRPC: 查询缺 a")?;
            let id = take_id(a.get("id"), "查询 a")?;
            let target = match a.get("target") {
                None => None,
                Some(v) => Some(take_id(Some(v), "a.target")?),
            };
            let info_hash = match a.get("info_hash") {
                None => None,
                Some(v) => {
                    let b = v.bytes().ok_or("a.info_hash: 不是字节串")?;
                    if b.len() != ID_LEN {
                        return Err(format!("a.info_hash: 长度 {} 应为 {ID_LEN}", b.len()));
                    }
                    let mut ih = [0u8; ID_LEN];
                    ih.copy_from_slice(b);
                    Some(ih)
                }
            };
            let port = match a.get("port").and_then(Value::int) {
                None => None,
                Some(p) if (0..=65535).contains(&p) => Some(p as u16),
                Some(p) => return Err(format!("a.port 越界: {p}")),
            };
            let implied_port = a.get("implied_port").and_then(Value::int).unwrap_or(0) != 0;
            let token = a.get("token").and_then(Value::bytes).map(|b| b.to_vec());
            let want = a.get("want").and_then(Value::list).unwrap_or(&[])
                .iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
            Ok(Message::Query(Query {
                tid, name, id, target, info_hash, port, implied_port, token, want,
            }))
        }
        "r" => {
            let r = root.get("r").ok_or("KRPC: 应答缺 r")?;
            let id = take_id(r.get("id"), "应答 r")?;
            let token = r.get("token").and_then(Value::bytes).map(|b| b.to_vec());
            let nodes = r.get("nodes").and_then(Value::bytes)
                .map(decode_compact_nodes).unwrap_or_default();
            let values = r.get("values").and_then(Value::list).unwrap_or(&[])
                .iter().filter_map(|v| v.bytes().and_then(decode_peer)).collect();
            Ok(Message::Response(Response { tid, id, token, nodes, values }))
        }
        "e" => {
            let list = root.get("e").and_then(Value::list).ok_or("KRPC: 错误应答缺 e")?;
            let code = list.first().and_then(Value::int).unwrap_or(0);
            let message = list.get(1).and_then(Value::as_str).unwrap_or("").to_string();
            Ok(Message::Error { tid, code, message })
        }
        other => Err(format!("KRPC: 未知 y={other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> NodeId {
        NodeId([n; ID_LEN])
    }

    #[test]
    fn ping_roundtrip_keeps_tid_and_id() {
        let q = Query::ping(vec![0xaa, 0x01], id(7));
        let bytes = encode_query(&q);
        match parse(&bytes).unwrap() {
            Message::Query(got) => {
                assert_eq!(got.tid, vec![0xaa, 0x01]);
                assert_eq!(got.name, PING);
                assert_eq!(got.id, id(7));
            }
            other => panic!("应为查询，实际 {other:?}"),
        }
    }

    #[test]
    fn get_peers_and_announce_carry_20_byte_info_hash() {
        let ih = [9u8; 20];
        let q = Query::get_peers("t1", id(1), ih);
        match parse(&encode_query(&q)).unwrap() {
            Message::Query(got) => assert_eq!(got.info_hash, Some(ih)),
            other => panic!("{other:?}"),
        }
        let a = Query::announce_peer("t2", id(1), ih, 6881, b"tok".to_vec());
        match parse(&encode_query(&a)).unwrap() {
            Message::Query(got) => {
                assert_eq!(got.port, Some(6881));
                assert_eq!(got.token.as_deref(), Some(&b"tok"[..]));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn response_carries_token_nodes_and_values() {
        let r = Response {
            tid: b"xy".to_vec(),
            id: id(3),
            token: Some(b"tk".to_vec()),
            nodes: vec![Contact { id: id(4), addr: "112.5.60.4:6881".parse().unwrap() }],
            values: vec!["112.5.60.9:51413".parse().unwrap()],
        };
        match parse(&encode_response(&r)).unwrap() {
            Message::Response(got) => {
                assert_eq!(got.tid, b"xy".to_vec());
                assert_eq!(got.token.as_deref(), Some(&b"tk"[..]));
                assert_eq!(got.nodes.len(), 1);
                assert_eq!(got.nodes[0].addr.port(), 6881);
                assert_eq!(got.values[0].port(), 51413);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn error_message_roundtrip() {
        let bytes = encode_error(b"t9", 204, "Method Unknown");
        match parse(&bytes).unwrap() {
            Message::Error { tid, code, message } => {
                assert_eq!(tid, b"t9".to_vec());
                assert_eq!(code, 204);
                assert_eq!(message, "Method Unknown");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rejects_bad_tid_and_id_lengths() {
        // 空 tid：无法对请求。
        let mut v = dict(vec![
            ("t", bencode::bytes("")), ("y", bencode::bytes("q")),
            ("q", bencode::bytes("ping")),
            ("a", dict(vec![("id", bencode::bytes([1u8; 20]))])),
        ]).encode();
        assert!(parse(&v).is_err());

        // 19 字节 id：不许补零。
        v = dict(vec![
            ("t", bencode::bytes("aa")), ("y", bencode::bytes("q")),
            ("q", bencode::bytes("ping")),
            ("a", dict(vec![("id", bencode::bytes([1u8; 19]))])),
        ]).encode();
        assert!(parse(&v).is_err());
    }

    #[test]
    fn rejects_unknown_y_and_truncated_payloads() {
        let v = dict(vec![("t", bencode::bytes("aa")), ("y", bencode::bytes("z"))]).encode();
        assert!(parse(&v).is_err());
        assert!(parse(b"d1:ay").is_err());
        assert!(parse(b"").is_err());
    }

    #[test]
    fn invalid_compact_entries_are_dropped_not_invented() {
        // 端口 0 的 value 必须被丢弃；否则会把「不知道端口」变成一个假 peer。
        let mut vals = Vec::new();
        vals.push(bencode::bytes([112u8, 5, 60, 9, 0, 0]));
        vals.push(bencode::bytes([112u8, 5, 60, 9, 0x1a, 0xe1]));
        let root = dict(vec![
            ("t", bencode::bytes("aa")),
            ("y", bencode::bytes("r")),
            ("r", dict(vec![
                ("id", bencode::bytes([2u8; 20])),
                ("values", Value::List(vals)),
            ])),
        ]).encode();
        match parse(&root).unwrap() {
            Message::Response(r) => {
                assert_eq!(r.values.len(), 1);
                assert_eq!(r.values[0].port(), 6881);
            }
            other => panic!("{other:?}"),
        }
    }
}
