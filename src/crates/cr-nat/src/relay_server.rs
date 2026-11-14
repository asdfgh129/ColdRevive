//! W3.4 生产中继服务端（std-only）。
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
//! **门禁声明**：本模块只补齐 S6 所需的「可信中继」部署组件，不构成四类 NAT 可达率实测；
//! `S6.status` 仍为 `not_run`，不得据此宣称 G2 达标。

use crate::relay::{FrameKind, RelayFrame, NODE_ID_LEN};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// 帧头 = 1 字节 kind + 16 字节 node id + 4 字节大端长度。
pub const HEADER_LEN: usize = 1 + NODE_ID_LEN + 4;
/// 单帧最大负载（1 MiB），防止恶意长度字段导致内存耗尽。
pub const DEFAULT_MAX_FRAME: usize = 1 << 20;

type Registry = Arc<Mutex<HashMap<[u8; NODE_ID_LEN], Arc<Mutex<TcpStream>>>>>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelayStats {
    /// 累计接受的 TCP 连接数
    pub connections: u64,
    /// 成功的 REGISTER 次数
    pub registered: u64,
    /// 成功投递的 DELIVERED 帧数
    pub delivered: u64,
    /// 目标未注册 / 目标连接已失效而丢弃的 RELAY 帧数（不伪造应答）
    pub dropped: u64,
    /// 协议违规帧数（注册负载不一致、来源未注册、非法帧类型等）
    pub rejected: u64,
}

pub struct RelayServer {
    listener: TcpListener,
    registry: Registry,
    stats: Arc<Mutex<RelayStats>>,
    stop: Arc<AtomicBool>,
    max_frame: usize,
}

/// `spawn()` 返回的句柄；持有者可读取真实计数并请求停止 accept 循环。
pub struct RelayHandle {
    stats: Arc<Mutex<RelayStats>>,
    stop: Arc<AtomicBool>,
}

impl RelayHandle {
    pub fn stats(&self) -> RelayStats {
        self.stats.lock().unwrap().clone()
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
    pub fn is_stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

impl Drop for RelayHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

impl RelayServer {
    /// 在调用方线程绑定监听（返回立即可用的端口，避免测试里的 bind/accept 竞态）。
    pub fn bind(addr: &str) -> Result<Self, String> {
        let listener = TcpListener::bind(addr).map_err(|e| format!("relay bind {}: {}", addr, e))?;
        Ok(Self {
            listener,
            registry: Arc::new(Mutex::new(HashMap::new())),
            stats: Arc::new(Mutex::new(RelayStats::default())),
            stop: Arc::new(AtomicBool::new(false)),
            max_frame: DEFAULT_MAX_FRAME,
        })
    }

    pub fn with_max_frame(mut self, max: usize) -> Self {
        self.max_frame = max.max(1);
        self
    }

    pub fn local_addr(&self) -> Result<SocketAddr, String> {
        self.listener.local_addr().map_err(|e| e.to_string())
    }

    pub fn stats_handle(&self) -> Arc<Mutex<RelayStats>> {
        self.stats.clone()
    }

    pub fn spawn(self) -> RelayHandle {
        let handle = RelayHandle {
            stats: self.stats.clone(),
            stop: self.stop.clone(),
        };
        thread::spawn(move || self.accept_loop());
        handle
    }

    fn accept_loop(self) {
        let _ = self.listener.set_nonblocking(true);
        while !self.stop.load(Ordering::Relaxed) {
            match self.listener.accept() {
                Ok((stream, peer)) => {
                    // Windows 下 accept 出来的 socket 可能继承非阻塞标志，显式复位。
                    let _ = stream.set_nonblocking(false);
                    // 空闲订阅连接长期阻塞在 read_exact 上，不设读超时；由进程退出回收线程。
                    let _ = stream.set_read_timeout(None);
                    let _ = stream.set_nodelay(true);
                    self.stats.lock().unwrap().connections += 1;
                    let registry = self.registry.clone();
                    let stats = self.stats.clone();
                    let max_frame = self.max_frame;
                    thread::spawn(move || {
                        handle_conn(stream, peer, registry, stats, max_frame)
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
    }
}

fn handle_conn(
    stream: TcpStream,
    _peer: SocketAddr,
    registry: Registry,
    stats: Arc<Mutex<RelayStats>>,
    max_frame: usize,
) {
    // 读写分离：reader 阻塞在读帧头时，其它线程仍可通过 writer 向本连接投递DELIVERED。
    let writer = match stream.try_clone() {
        Ok(w) => Arc::new(Mutex::new(w)),
        Err(_) => return,
    };
    let mut reader = stream;
    let mut registered: Option<[u8; NODE_ID_LEN]> = None;

    loop {
        let frame = match read_frame(&mut reader, max_frame) {
            Ok(f) => f,
            Err(_) => break,
        };
        match frame.kind {
            FrameKind::Register => {
                // 协议约定：REGISTER 负载必须回显头部里的 node id，防止伪造身份。
                if frame.payload.len() != NODE_ID_LEN || frame.payload.as_slice() != frame.node {
                    stats.lock().unwrap().rejected += 1;
                    break;
                }
                let id = frame.node;
                registry.lock().unwrap().insert(id, writer.clone());
                registered = Some(id);
                let ack = RelayFrame {
                    kind: FrameKind::Ack,
                    node: [0u8; NODE_ID_LEN],
                    payload: Vec::new(),
                };
                if write_frame(&mut *writer.lock().unwrap(), &ack).is_err() {
                    break;
                }
                stats.lock().unwrap().registered += 1;
            }
            FrameKind::Relay => {
                let Some(source) = registered else {
                    stats.lock().unwrap().rejected += 1;
                    continue;
                };
                if frame.payload.len() < NODE_ID_LEN {
                    stats.lock().unwrap().rejected += 1;
                    continue;
                }
                let mut target = [0u8; NODE_ID_LEN];
                target.copy_from_slice(&frame.payload[..NODE_ID_LEN]);
                let message = frame.payload[NODE_ID_LEN..].to_vec();
                let dest = registry.lock().unwrap().get(&target).cloned();
                match dest {
                    Some(w) => {
                        let delivered = RelayFrame {
                            kind: FrameKind::Delivered,
                            node: source,
                            payload: message,
                        };
                        if write_frame(&mut *w.lock().unwrap(), &delivered).is_ok() {
                            stats.lock().unwrap().delivered += 1;
                        } else {
                            // 连接已失效：摘除过期注册并如实计数，不回伪造帧。
                            let mut reg = registry.lock().unwrap();
                            if let Some(cur) = reg.get(&target) {
                                if Arc::ptr_eq(cur, &w) {
                                    reg.remove(&target);
                                }
                            }
                            drop(reg);
                            stats.lock().unwrap().dropped += 1;
                        }
                    }
                    None => stats.lock().unwrap().dropped += 1,
                }
            }
            _ => stats.lock().unwrap().rejected += 1,
        }
    }

    if let Some(id) = registered {
        let mut reg = registry.lock().unwrap();
        if let Some(cur) = reg.get(&id) {
            if Arc::ptr_eq(cur, &writer) {
                reg.remove(&id);
            }
        }
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

        let s = handle.stats();
        assert_eq!(s.delivered, 2);
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
}
