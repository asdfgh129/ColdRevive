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
//! 诚实边界：`FORWARD` 是一次性尽力投递，链路断在半路就如实计 `forward_dropped`，不做重传、
//! 不做多副本；多实例只解决「单实例宕机后注册表丢失」，不等于任播/共识，也不是 S6 的四类
//! NAT 实测。`S6.status` 仍为 `not_run`，不得据此宣称 G2 达标。

use crate::relay::{FrameKind, RelayFrame, NODE_ID_LEN};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 帧头 = 1 字节 kind + 16 字节 node id + 4 字节大端长度。
pub const HEADER_LEN: usize = 1 + NODE_ID_LEN + 4;
/// 单帧最大负载（1 MiB），防止恶意长度字段导致内存耗尽。
pub const DEFAULT_MAX_FRAME: usize = 1 << 20;
/// 注册表快照周期。事件驱动（注册时）已推一次，这里只做漏包/漏链的兜底。
pub const SYNC_INTERVAL: Duration = Duration::from_millis(500);
/// 同侪链路断开后的重拨间隔。
pub const PEER_RETRY: Duration = Duration::from_secs(1);
/// 到同侪 / 订阅连接的写超时：避免一个卡死的对端把写锁永久占住。
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

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
}

/// 所有连接线程共享的状态。
struct Shared {
    instance: [u8; NODE_ID_LEN],
    /// 本实例自己的注册表：node id → 该客户端的写连接
    registry: Mutex<HashMap<[u8; NODE_ID_LEN], Arc<Mutex<TcpStream>>>>,
    /// 从同侪快照学到的归属：node id → 拥有它的实例 id
    peer_registry: Mutex<HashMap<[u8; NODE_ID_LEN], [u8; NODE_ID_LEN]>>,
    /// 同侪实例 id → 链路写连接
    peers: Mutex<HashMap<[u8; NODE_ID_LEN], Arc<Mutex<TcpStream>>>>,
    stats: Mutex<RelayStats>,
    stop: Arc<AtomicBool>,
    max_frame: usize,
}

pub struct RelayServer {
    listener: TcpListener,
    instance: [u8; NODE_ID_LEN],
    peers: Vec<String>,
    max_frame: usize,
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
        let links: Vec<Arc<Mutex<TcpStream>>> = {
            let mut p = self.shared.peers.lock().unwrap();
            p.drain().map(|(_, w)| w).collect()
        };
        for w in links {
            if let Ok(s) = w.lock() {
                let _ = s.shutdown(std::net::Shutdown::Both);
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

    fn local_nodes(&self) -> Vec<[u8; NODE_ID_LEN]> {
        let mut v: Vec<_> = self.registry.lock().unwrap().keys().copied().collect();
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

    fn install_peer(&self, id: [u8; NODE_ID_LEN], writer: Arc<Mutex<TcpStream>>) -> bool {
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
        let links: Vec<([u8; NODE_ID_LEN], Arc<Mutex<TcpStream>>)> = self
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
        let dest = self.registry.lock().unwrap().get(target).cloned();
        let Some(w) = dest else { return false };
        let frame = RelayFrame {
            kind: FrameKind::Delivered,
            node: source,
            payload: message,
        };
        if send_frame(&w, &frame) {
            return true;
        }
        let mut reg = self.registry.lock().unwrap();
        if let Some(cur) = reg.get(target) {
            if Arc::ptr_eq(cur, &w) {
                reg.remove(target);
            }
        }
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
        if self.registry.lock().unwrap().contains_key(&target) {
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
        })
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
        } = self;
        let shared = Arc::new(Shared {
            instance,
            registry: Mutex::new(HashMap::new()),
            peer_registry: Mutex::new(HashMap::new()),
            peers: Mutex::new(HashMap::new()),
            stats: Mutex::new(RelayStats::default()),
            stop: Arc::new(AtomicBool::new(false)),
            max_frame,
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
            Ok((stream, _peer)) => {
                // Windows 下 accept 出来的 socket 可能继承非阻塞标志，显式复位。
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(None);
                let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
                let _ = stream.set_nodelay(true);
                shared.stat(|s| s.connections += 1);
                let sh = shared.clone();
                thread::spawn(move || conn_loop(stream, sh, None, None));
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break,
        }
    }
}

fn send_frame(w: &Arc<Mutex<TcpStream>>, frame: &RelayFrame) -> bool {
    match w.lock() {
        Ok(mut g) => write_frame(&mut *g, frame).is_ok(),
        Err(_) => false,
    }
}

/// 单条连接的处理循环。
///
/// `registered`：这条连接以哪个节点 id 注册（客户端）；`peer`：这条连接是同侪实例链路。
/// 两者互斥——同侪链路不允许发客户端 `RELAY`，否则来源身份会被实例 id 污染。
fn conn_loop(
    stream: TcpStream,
    shared: Arc<Shared>,
    mut registered: Option<[u8; NODE_ID_LEN]>,
    mut peer: Option<[u8; NODE_ID_LEN]>,
) {
    let writer = match stream.try_clone() {
        Ok(w) => Arc::new(Mutex::new(w)),
        Err(_) => return,
    };
    let mut reader = stream;

    loop {
        let frame = match read_frame(&mut reader, shared.max_frame) {
            Ok(f) => f,
            Err(_) => break,
        };
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
                shared.registry.lock().unwrap().insert(id, writer.clone());
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
                // 事件驱动同步：注册立刻推给同侪，跨实例投递不必等周期兜底。
                if !shared.peers.lock().unwrap().is_empty() {
                    shared.broadcast_snapshot();
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
                shared.broadcast_snapshot();
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
            // DELIVERED / ACK 是服务端→客户端的帧；客户端回送它们属于协议违规。
            FrameKind::Delivered | FrameKind::Ack => {}
            FrameKind::Unknown(_) => shared.stat(|s| s.rejected += 1),
        }
    }

    if let Some(id) = registered {
        let mut reg = shared.registry.lock().unwrap();
        if let Some(cur) = reg.get(&id) {
            if Arc::ptr_eq(cur, &writer) {
                reg.remove(&id);
            }
        }
        drop(reg);
        if !shared.peers.lock().unwrap().is_empty() {
            shared.broadcast_snapshot();
        }
    }
    if let Some(id) = peer {
        shared.drop_peer(&id);
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
            Ok((peer_id, stream, writer)) => {
                learned = Some(peer_id);
                if !shared.install_peer(peer_id, writer) {
                    sleep_unless_stopped(&shared, PEER_RETRY);
                    continue;
                }
                shared.broadcast_snapshot();
                conn_loop(stream, shared.clone(), None, Some(peer_id));
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
) -> Result<([u8; NODE_ID_LEN], TcpStream, Arc<Mutex<TcpStream>>), String> {
    let sock_addr = addr
        .to_socket_addrs_first()
        .ok_or_else(|| format!("同侪地址无解析结果: {}", addr))?;
    let stream = TcpStream::connect_timeout(&sock_addr, CONNECT_TIMEOUT)
        .map_err(|e| format!("connect: {}", e))?;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let _ = stream.set_read_timeout(Some(CONNECT_TIMEOUT));

    let writer = Arc::new(Mutex::new(stream.try_clone().map_err(|e| e.to_string())?));
    let hello = RelayFrame {
        kind: FrameKind::PeerHello,
        node: shared.instance,
        payload: shared.instance.to_vec(),
    };
    if !send_frame(&writer, &hello) {
        return Err("发送 PEER_HELLO 失败".into());
    }
    let mut reader = stream;
    let ack = read_frame(&mut reader, shared.max_frame)?;
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
    // 进入长连接阶段：读侧不再有握手超时（DELIVERED/FORWARD 是事件驱动的）。
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
    use std::io::Cursor;

    fn id(label: u8) -> [u8; NODE_ID_LEN] {
        let mut v = [0u8; NODE_ID_LEN];
        v[0] = label;
        v
    }

    /// 等计数追上「客户端已经收到帧」这个事实。
    ///
    /// 计数由服务端线程在投递**之后**自增，客户端 `recv_frame()` 返回时它可能还没跑到，
    /// 于是断言偶发失败（实测约 1/3）。偶发失败的测试等于没有测试，这里改成有界等待。
    fn wait_until<F: Fn() -> bool>(pred: F, timeout: Duration) -> bool {
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
}
