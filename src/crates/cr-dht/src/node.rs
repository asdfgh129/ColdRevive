//! DHT 节点：把 KRPC 编解码、路由表与 token 校验拼成一个可运行的 UDP 节点。
//!
//! 这是 W3.2 的实现面，**不是 S6 的判据**。S3 的证据来自 `tests/spikes/s3/mainline_probe/`
//! 里对已锁定 `librqbit-dht 9.1.0` 的公网宣告/取回闭环；本 crate 是可自持的
//! Mainline 适配层（std-only、无外部依赖），二者不要混为一谈：
//! 本 crate 的测试只证明「报文与状态机自洽」，**不证明公网可达**。
//!
//! 安全边界（都是刻意的）：
//! - `announce_peer` 必须带**由本节点按来源 IP 签发**的 token。没有 token 校验的 DHT
//!   会被任意主机用来把别人的 info_hash 填满假 peer——这是最常见的滥用形态。
//! - token 只绑 IP，不绑端口（BEP 5 如此）：同一 IP 换端口仍是同一个人。
//! - 入站查询与应答都**不改变**本地是否对外宣告任何东西；宣告由调用方显式发起。
//! - 报文大小硬上限：UDP 单包超过 `MAX_DATAGRAM` 直接丢弃，不做分片重组。

use crate::id::{Contact, NodeId, ID_LEN};
use crate::krpc::{self, Message, Query, Response};
use crate::table::{RoutingTable, K};
use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 单包上限。DHT 报文远小于此；留余量给 26 字节 × 8 的 nodes 列表。
pub const MAX_DATAGRAM: usize = 1500;
/// 每个 info_hash 最多保存的 peer 数（防单 key 内存放大）。
pub const MAX_PEERS_PER_KEY: usize = 64;
/// token 长度。BEP 5 没规定；8 字节足够挡住随手构造的 announce。
pub const TOKEN_LEN: usize = 8;

#[derive(Debug, Default)]
pub struct DhtStats {
    pub queries: u64,
    pub responses: u64,
    pub errors: u64,
    pub announces_stored: u64,
    pub announces_rejected: u64,
    pub peers_served: u64,
    /// 解析失败 / 超长 / 无法回复的入站包。
    pub dropped: u64,
}

pub struct DhtNode {
    id: NodeId,
    table: Mutex<RoutingTable>,
    /// token 的密钥。只在内存里，重启即换 —— 旧 token 失效是正确行为。
    secret: [u8; 16],
    peers: Mutex<HashMap<[u8; ID_LEN], Vec<SocketAddr>>>,
    stats: Mutex<DhtStats>,
}

impl DhtNode {
    pub fn new(id: NodeId) -> Self {
        let mut secret = [0u8; 16];
        // 复用 NodeId 的随机源即可：token 只需要「别人猜不到」，不需要长期机密。
        secret.copy_from_slice(&NodeId::random().as_bytes()[..16]);
        Self {
            id,
            table: Mutex::new(RoutingTable::new(id)),
            secret,
            peers: Mutex::new(HashMap::new()),
            stats: Mutex::new(DhtStats::default()),
        }
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    pub fn stats(&self) -> DhtStats {
        let s = self.stats.lock().unwrap();
        DhtStats {
            queries: s.queries,
            responses: s.responses,
            errors: s.errors,
            announces_stored: s.announces_stored,
            announces_rejected: s.announces_rejected,
            peers_served: s.peers_served,
            dropped: s.dropped,
        }
    }

    pub fn table_len(&self) -> usize {
        self.table.lock().unwrap().len()
    }

    pub fn contacts(&self) -> Vec<Contact> {
        self.table.lock().unwrap().all()
    }

    /// 把一个已知联系人塞进路由表（自举/测试用）。
    ///
    /// 这只是记账，不做存活验证；调用方若传入一个早就下线的地址，后续查询会超时，
    /// 那是预期行为——本层不替调用方编造「已验证」。
    pub fn seed_contact(&self, contact: Contact) -> crate::table::AddOutcome {
        self.table.lock().unwrap().add(contact)
    }

    /// 已保存的 peer（诊断用；按 info_hash 取）。
    pub fn peers_for(&self, info_hash: &[u8; ID_LEN]) -> Vec<SocketAddr> {
        self.peers.lock().unwrap().get(info_hash).cloned().unwrap_or_default()
    }

    /// 按来源 IP 签发 token。
    fn token_for(&self, ip: IpAddr) -> Vec<u8> {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a offset basis
        for b in self.secret.iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        let ip_bytes = match ip {
            IpAddr::V4(v4) => v4.octets().to_vec(),
            IpAddr::V6(v6) => v6.octets().to_vec(),
        };
        for b in ip_bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h.to_be_bytes()[..TOKEN_LEN].to_vec()
    }

    fn token_ok(&self, ip: IpAddr, token: &[u8]) -> bool {
        let want = self.token_for(ip);
        if token.len() != want.len() {
            return false;
        }
        // 定长比较：token 不是长期机密，但随手写成短路比较会给出时序侧信道，
        // 成本只有 8 次异或，没有理由留这个口子。
        let mut diff = 0u8;
        for (a, b) in token.iter().zip(want.iter()) {
            diff |= a ^ b;
        }
        diff == 0
    }

    pub fn stat<F: FnOnce(&mut DhtStats)>(&self, f: F) {
        f(&mut self.stats.lock().unwrap());
    }

    /// 处理一个入站 UDP 包，返回需要回给 `src` 的字节（`None` = 不回）。
    ///
    /// 必须是纯函数式：不做 I/O，方便在回环测试里直接断言「回什么」。
    pub fn handle(&self, data: &[u8], src: SocketAddr) -> Option<Vec<u8>> {
        if data.len() > MAX_DATAGRAM {
            self.stat(|s| s.dropped += 1);
            return None;
        }
        let msg = match krpc::parse(data) {
            Ok(m) => m,
            Err(_) => {
                self.stat(|s| s.dropped += 1);
                return None;
            }
        };
        match msg {
            // 应答/错误是客户端侧的事：本节点没有在等任何 tid，收到就丢。
            Message::Response(_) | Message::Error { .. } => None,
            Message::Query(q) => {
                self.stat(|s| s.queries += 1);
                // 只把合法 ID 的查询者收进路由表。地址用**包来源地址**而不是报文里
                // 自报的地址——后者可以被伪造，前者是内核告诉我们的。
                if q.id != NodeId::ZERO && q.id != self.id {
                    self.table.lock().unwrap().add(Contact { id: q.id, addr: src });
                }
                let out = self.answer(&q, src);
                self.stat(|s| match &out {
                    Answer::Ok(_) => s.responses += 1,
                    Answer::Err(_) => s.errors += 1,
                });
                match out {
                    Answer::Ok(bytes) | Answer::Err(bytes) => Some(bytes),
                }
            }
        }
    }

    fn answer(&self, q: &Query, src: SocketAddr) -> Answer {
        match q.name.as_str() {
            krpc::PING => Answer::Ok(krpc::encode_response(&Response::plain(q.tid.clone(), self.id))),
            krpc::FIND_NODE => {
                let Some(target) = q.target else {
                    return Answer::Err(krpc::encode_error(&q.tid, 203, "find_node needs target"));
                };
                let nodes = self.table.lock().unwrap().closest(&target, K);
                let mut r = Response::plain(q.tid.clone(), self.id);
                r.nodes = nodes;
                Answer::Ok(krpc::encode_response(&r))
            }
            krpc::GET_PEERS => {
                let Some(ih) = q.info_hash else {
                    return Answer::Err(krpc::encode_error(&q.tid, 203, "get_peers needs info_hash"));
                };
                let mut r = Response::plain(q.tid.clone(), self.id);
                r.token = Some(self.token_for(src.ip()));
                {
                    let peers = self.peers.lock().unwrap();
                    if let Some(list) = peers.get(&ih) {
                        r.values = list.iter().take(MAX_PEERS_PER_KEY).copied().collect();
                        if !r.values.is_empty() {
                            self.stat(|s| s.peers_served += 1);
                        }
                    }
                }
                if r.values.is_empty() {
                    // 没有 peer 时给最近节点，让查询方还能继续往目标 ID 靠近。
                    r.nodes = self.table.lock().unwrap().closest(&NodeId(ih), K);
                }
                Answer::Ok(krpc::encode_response(&r))
            }
            krpc::ANNOUNCE_PEER => {
                let Some(ih) = q.info_hash else {
                    return Answer::Err(krpc::encode_error(&q.tid, 203, "announce_peer needs info_hash"));
                };
                let Some(token) = q.token.clone() else {
                    self.stat(|s| s.announces_rejected += 1);
                    return Answer::Err(krpc::encode_error(&q.tid, 203, "missing token"));
                };
                if !self.token_ok(src.ip(), &token) {
                    self.stat(|s| s.announces_rejected += 1);
                    return Answer::Err(krpc::encode_error(&q.tid, 203, "bad token"));
                }
                // implied_port=1 表示「用包的源端口」——NAT 后客户端自己不知道外部端口，
                // 这是 BEP 5 给它们留的唯一正确说法。
                let port = if q.implied_port { src.port() } else { q.port.unwrap_or(0) };
                if port == 0 {
                    self.stat(|s| s.announces_rejected += 1);
                    return Answer::Err(krpc::encode_error(&q.tid, 203, "bad port"));
                }
                let addr = SocketAddr::new(src.ip(), port);
                {
                    let mut peers = self.peers.lock().unwrap();
                    let list = peers.entry(ih).or_default();
                    if !list.contains(&addr) {
                        if list.len() >= MAX_PEERS_PER_KEY {
                            // 满了就丢最旧的（插入顺序），而不是拒绝新 peer——
                            // 拒绝会让新节点永远进不来，而旧条目多半已经下线。
                            list.remove(0);
                        }
                        list.push(addr);
                    }
                }
                self.stat(|s| s.announces_stored += 1);
                Answer::Ok(krpc::encode_response(&Response::plain(q.tid.clone(), self.id)))
            }
            _ => Answer::Err(krpc::encode_error(&q.tid, 204, "Method Unknown")),
        }
    }
}

/// 对一条查询的处理结果。没有「不回」这个分支：合法的查询永远有应答，
/// 非法报文在解析阶段就被丢弃（`handle` 返回 `None`），两者不该混在同一层。
enum Answer {
    Ok(Vec<u8>),
    Err(Vec<u8>),
}

/// 一个绑定好的 UDP 服务端。`run` 是阻塞循环；测试里用 `spawn` 拿句柄。
pub struct DhtServer {
    node: Arc<DhtNode>,
    sock: UdpSocket,
}

impl DhtServer {
    pub fn bind(addr: &str, node: Arc<DhtNode>) -> Result<Self, String> {
        let sock = UdpSocket::bind(addr).map_err(|e| format!("DHT bind {addr}: {e}"))?;
        Ok(Self { node, sock })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, String> {
        self.sock.local_addr().map_err(|e| e.to_string())
    }

    /// 阻塞服务直到 `stop` 置位。单个坏包绝不终止循环——DHT 端口面向公网，
    /// 「一个畸形包让节点退出」是最容易被利用的失败形态。
    pub fn run(&self, stop: &AtomicBool) {
        let _ = self.sock.set_read_timeout(Some(Duration::from_millis(100)));
        let mut buf = [0u8; MAX_DATAGRAM];
        while !stop.load(Ordering::Relaxed) {
            match self.sock.recv_from(&mut buf) {
                Ok((n, src)) => {
                    if let Some(out) = self.node.handle(&buf[..n], src) {
                        let _ = self.sock.send_to(&out, src);
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut => {}
                Err(_) => break,
            }
        }
    }
}

/// 测试/嵌入用的后台服务句柄。
pub struct ServerHandle {
    pub addr: SocketAddr,
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl ServerHandle {
    pub fn spawn(node: Arc<DhtNode>) -> Result<Self, String> {
        let server = DhtServer::bind("127.0.0.1:0", node)?;
        let addr = server.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let join = std::thread::spawn(move || server.run(&s));
        Ok(Self { addr, stop, join: Some(join) })
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// 客户端：一个 socket + 事务号生成。同步 API，调用方自己决定超时。
pub struct DhtClient {
    id: NodeId,
    sock: UdpSocket,
    seq: AtomicU64,
}

impl DhtClient {
    pub fn bind(addr: &str, id: NodeId) -> Result<Self, String> {
        let sock = UdpSocket::bind(addr).map_err(|e| format!("DHT client bind {addr}: {e}"))?;
        Ok(Self { id, sock, seq: AtomicU64::new(1) })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, String> {
        self.sock.local_addr().map_err(|e| e.to_string())
    }

    fn next_tid(&self) -> Vec<u8> {
        let n = self.seq.fetch_add(1, Ordering::Relaxed);
        let mut t = vec![0x43, 0x52]; // "CR"
        t.extend_from_slice(&n.to_be_bytes());
        t
    }

    /// 发一个查询并等**同 tid** 的应答。超时返回错误，不返回任何合成结果。
    fn exchange(&self, addr: SocketAddr, q: Query, timeout: Duration) -> Result<Message, String> {
        let deadline = Instant::now() + timeout;
        let bytes = krpc::encode_query(&q);
        self.sock.send_to(&bytes, addr).map_err(|e| format!("DHT send {addr}: {e}"))?;
        let mut buf = [0u8; MAX_DATAGRAM];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!("DHT 查询超时 {} ms（{}）", timeout.as_millis(), q.name));
            }
            self.sock.set_read_timeout(Some(left)).map_err(|e| e.to_string())?;
            let (n, from) = match self.sock.recv_from(&mut buf) {
                Ok(v) => v,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
                    return Err(format!("DHT 查询超时 {} ms（{}）", timeout.as_millis(), q.name)),
                Err(e) => return Err(format!("DHT recv: {e}")),
            };
            // 只接受来自目标地址的包：否则同网段任何人发一个 "tid 相同" 的应答
            // 都能把我们引到错误的节点上（tid 是明文，猜得到）。
            if from != addr {
                continue;
            }
            match krpc::parse(&buf[..n]) {
                Ok(Message::Response(r)) if r.tid == q.tid => return Ok(Message::Response(r)),
                Ok(Message::Error { tid, code, message }) if tid == q.tid =>
                    return Err(format!("DHT 错误应答 {code}: {message}")),
                _ => continue,
            }
        }
    }

    pub fn ping(&self, addr: SocketAddr, timeout: Duration) -> Result<NodeId, String> {
        let q = Query::ping(self.next_tid(), self.id);
        match self.exchange(addr, q, timeout)? {
            Message::Response(r) => Ok(r.id),
            other => Err(format!("ping 期望应答，得到 {other:?}")),
        }
    }

    pub fn find_node(&self, addr: SocketAddr, target: NodeId, timeout: Duration) -> Result<Vec<Contact>, String> {
        let q = Query::find_node(self.next_tid(), self.id, target);
        match self.exchange(addr, q, timeout)? {
            Message::Response(r) => Ok(r.nodes),
            other => Err(format!("find_node 期望应答，得到 {other:?}")),
        }
    }

    /// 返回 (token, 已知节点, 已知 peer)。
    pub fn get_peers(
        &self,
        addr: SocketAddr,
        info_hash: [u8; ID_LEN],
        timeout: Duration,
    ) -> Result<(Option<Vec<u8>>, Vec<Contact>, Vec<SocketAddr>), String> {
        let q = Query::get_peers(self.next_tid(), self.id, info_hash);
        match self.exchange(addr, q, timeout)? {
            Message::Response(r) => Ok((r.token, r.nodes, r.values)),
            other => Err(format!("get_peers 期望应答，得到 {other:?}")),
        }
    }

    pub fn announce_peer(
        &self,
        addr: SocketAddr,
        info_hash: [u8; ID_LEN],
        port: u16,
        token: Vec<u8>,
        timeout: Duration,
    ) -> Result<(), String> {
        let q = Query::announce_peer(self.next_tid(), self.id, info_hash, port, token);
        match self.exchange(addr, q, timeout)? {
            Message::Response(_) => Ok(()),
            other => Err(format!("announce_peer 期望应答，得到 {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Duration = Duration::from_millis(1500);

    fn server() -> (Arc<DhtNode>, ServerHandle) {
        let node = Arc::new(DhtNode::new(NodeId::random()));
        let h = ServerHandle::spawn(node.clone()).expect("spawn dht server");
        (node, h)
    }

    fn client() -> DhtClient {
        DhtClient::bind("127.0.0.1:0", NodeId::random()).expect("client bind")
    }

    #[test]
    fn ping_roundtrip_over_real_udp_and_learns_caller() {
        let (node, srv) = server();
        let c = client();
        let got = c.ping(srv.addr, T).expect("ping 必须成功");
        assert_eq!(got, node.id(), "应答里的 id 必须是对方真实 id");
        // 对端应把调用方收进路由表（来源地址 = 真实 socket 地址）。
        let contacts = node.contacts();
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].addr, c.local_addr().unwrap());
        assert!(node.stats().queries >= 1);
    }

    #[test]
    fn find_node_returns_compact_contacts() {
        let (node, srv) = server();
        // 预置 8 个公网地址：K=8，调用方自己会被挤到截断之外，
        // 这样断言的是「真的把已知联系人编码回去了」，不是它刚学到的那一个。
        for i in 1..=8u8 {
            node.table.lock().unwrap().add(Contact {
                id: NodeId::random(),
                addr: format!("112.5.60.{i}:6881").parse().unwrap(),
            });
        }
        // 调用方用全零 ID：`handle` 会拒绝把全零 ID 收进路由表，于是表里恰好只有
        // 上面那 8 个公网联系人，`closest(target, K)` 的结果可以被精确断言。
        // （否则随机 ID 的调用方会占掉一个名额，断言只能在 7/8 之间浮动。）
        let c = DhtClient::bind("127.0.0.1:0", NodeId::ZERO).expect("client bind");
        let nodes = c.find_node(srv.addr, NodeId::random(), T).expect("find_node");
        assert_eq!(nodes.len(), K);
        assert!(nodes.iter().all(|n| n.addr.ip().to_string().starts_with("112.5.60.")));
    }

    #[test]
    fn get_peers_token_then_announce_then_query_returns_peer() {
        let (node, srv) = server();
        let c = client();
        let ih = [0x11u8; ID_LEN];

        // 1) 先查：没有 peer，但必须给 token 和可继续查询的节点。
        let (token, _, values) = c.get_peers(srv.addr, ih, T).expect("get_peers");
        assert!(values.is_empty());
        let token = token.expect("get_peers 必须签发 token");

        // 2) 宣告（端口用真实端口，避免 implied_port 语义干扰断言）。
        let announced = c.local_addr().unwrap().port();
        c.announce_peer(srv.addr, ih, announced, token, T).expect("announce");

        // 3) 再查：必须取回刚宣告的地址。
        let (_, _, values) = c.get_peers(srv.addr, ih, T).expect("get_peers #2");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].port(), announced);
        assert_eq!(node.peers_for(&ih).len(), 1);
        assert_eq!(node.stats().announces_stored, 1);
    }

    #[test]
    fn announce_with_forged_token_is_rejected() {
        let (node, srv) = server();
        let c = client();
        let ih = [0x22u8; ID_LEN];
        let err = c.announce_peer(srv.addr, ih, 6881, b"forged!!".to_vec(), T)
            .expect_err("伪造 token 必须被拒");
        assert!(err.contains("203"), "错误应带 BEP 5 的 203 协议错误码: {err}");
        assert!(node.peers_for(&ih).is_empty(), "被拒的宣告不得落库");
        assert_eq!(node.stats().announces_rejected, 1);
    }

    #[test]
    fn announce_without_token_or_port_is_rejected() {
        let node = DhtNode::new(NodeId::random());
        let src: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        let ih = [0x33u8; ID_LEN];

        // 缺 token
        let q = Query::announce_peer(b"t1".to_vec(), NodeId::random(), ih, 6881, Vec::new());
        let out = node.handle(&krpc::encode_query(&q), src).expect("应回错误");
        assert!(matches!(krpc::parse(&out).unwrap(), Message::Error { code: 203, .. }));

        // 有 token 但 port=0
        let token = node.token_for(src.ip());
        let mut q = Query::announce_peer(b"t2".to_vec(), NodeId::random(), ih, 6881, token);
        q.port = Some(0);
        let out = node.handle(&krpc::encode_query(&q), src).expect("应回错误");
        assert!(matches!(krpc::parse(&out).unwrap(), Message::Error { code: 203, .. }));
        assert_eq!(node.stats().announces_rejected, 2);
        assert!(node.peers_for(&ih).is_empty());
    }

    #[test]
    fn implied_port_uses_source_port() {
        let node = DhtNode::new(NodeId::random());
        let src: SocketAddr = "112.5.60.4:47488".parse().unwrap();
        let ih = [0x44u8; ID_LEN];
        let token = node.token_for(src.ip());
        let mut q = Query::announce_peer(b"t".to_vec(), NodeId::random(), ih, 1, token);
        q.implied_port = true;
        let out = node.handle(&krpc::encode_query(&q), src).expect("应成功");
        assert!(matches!(krpc::parse(&out).unwrap(), Message::Response(_)));
        assert_eq!(node.peers_for(&ih), vec![src]);
    }

    #[test]
    fn token_is_bound_to_ip_not_port() {
        let node = DhtNode::new(NodeId::random());
        let a: SocketAddr = "112.5.60.4:1000".parse().unwrap();
        let b: SocketAddr = "112.5.60.4:2000".parse().unwrap();
        let c: SocketAddr = "112.5.60.5:1000".parse().unwrap();
        let t = node.token_for(a.ip());
        assert_eq!(t, node.token_for(b.ip()), "同 IP 换端口应仍是同一 token");
        assert_ne!(t, node.token_for(c.ip()), "不同 IP 必须得到不同 token");
        assert!(node.token_ok(a.ip(), &t));
        assert!(!node.token_ok(c.ip(), &t));
        assert!(!node.token_ok(a.ip(), b"short"));
    }

    #[test]
    fn unknown_method_gets_204_and_garbage_is_dropped() {
        let node = DhtNode::new(NodeId::random());
        let src: SocketAddr = "127.0.0.1:5001".parse().unwrap();
        let q = Query::base(b"t".to_vec(), "vote", NodeId::random());
        let out = node.handle(&krpc::encode_query(&q), src).expect("应回错误");
        match krpc::parse(&out).unwrap() {
            Message::Error { code, .. } => assert_eq!(code, 204),
            other => panic!("{other:?}"),
        }
        assert!(node.handle(b"not bencode", src).is_none());
        assert!(node.handle(&[b'l'; 10], src).is_none());
        assert_eq!(node.stats().dropped, 2);
    }

    #[test]
    fn peers_per_key_is_capped() {
        // 单 key 的内存放大必须被硬性封住：谁都可能对一个 info_hash 刷宣告。
        let node = DhtNode::new(NodeId::random());
        let ih = [0x55u8; ID_LEN];
        {
            let mut peers = node.peers.lock().unwrap();
            let list = peers.entry(ih).or_default();
            for i in 0..MAX_PEERS_PER_KEY {
                list.push(format!("112.5.60.4:{}", 1000 + i).parse().unwrap());
            }
        }
        let src: SocketAddr = "112.5.60.4:9000".parse().unwrap();
        let token = node.token_for(src.ip());
        let q = Query::announce_peer(b"t".to_vec(), NodeId::random(), ih, 9000, token);
        node.handle(&krpc::encode_query(&q), src).expect("应成功");
        let stored = node.peers_for(&ih);
        assert_eq!(stored.len(), MAX_PEERS_PER_KEY);
        assert!(stored.contains(&src), "新 peer 必须能进来");
        assert!(!stored.contains(&"112.5.60.4:1000".parse().unwrap()), "最旧的应被淘汰");
    }

    #[test]
    fn oversized_and_self_id_packets_are_ignored() {
        let node = DhtNode::new(NodeId::random());
        let src: SocketAddr = "127.0.0.1:5002".parse().unwrap();
        assert!(node.handle(&vec![b'x'; MAX_DATAGRAM + 1], src).is_none());
        // 自己发的查询（回环）不应把自己加进路由表。
        let q = Query::ping(b"t".to_vec(), node.id());
        let _ = node.handle(&krpc::encode_query(&q), src);
        assert_eq!(node.table_len(), 0);
    }
}
