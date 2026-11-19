//! ADR-023：守护进程侧的中继数据面（opt-in，`nat.punch_route=true` 才创建）。
//!
//! 职责只有四件：
//! 1. 起一个本地 SOCKS5 桥，让 librqbit 的出向连接能改走外部 transport（ADR-021 的官方钩子）。
//! 2. 把 `cr_nat::relay_tunnel` 的 std 阻塞连接桥到 tokio `DuplexStream` 并登记进桥。
//! 3. **向持有者发 OPEN 并等 OPENED**：没确认就不登记进桥，如实回落直连。
//! 4. 持有隧道集合的生命周期；隧道一被释放，引擎侧就会看到 EOF。
//!
//! 数据面用**派生的会话 id** 注册中继（`derive_session_id(.., b"cr-dp-v1")`）：
//! 订阅线程已经用 `node_id` 占了注册表表项，同一 id 再连一条会互相顶掉。
//!
//! **默认关闭**：未启用时本模块不创建任何对象，`proxy_url` 保持 `None`，既有行为不变。

use cr_bridge::Bridge;
use cr_nat::relay::{RelayClient, NODE_ID_LEN};
use cr_nat::relay_tunnel::{derive_session_id, RelayTunnel, RelayTunnelSet, OPEN_TIMEOUT};
use cr_nat::relay_udp::UdpRelayClient;
use cr_nat::relay_udp_stream::{send_open_and_wait as udp_send_open_and_wait, UdpRelayStream};
use std::collections::HashMap;
use std::io::{ErrorKind, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::net::TcpListener;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const CHANNEL_QUEUE: usize = 64;

pub struct DataPlane {
    // 持有 runtime 是为了让 SOCKS5 accept 循环活着；没有读取需求，但不是死字段。
    #[allow(dead_code)]
    runtime: tokio::runtime::Runtime,
    bridge: Arc<Bridge>,
    proxy_url: String,
    tunnels: Mutex<HashMap<u32, RelayTunnelSet>>,
    /// ADR-024 阶段 2：UDP 承载隧道的存活标志。stream split 后读/写半边各占一线程，
    /// 泵退出时置 false；看板据此区分「登记数」与「活着数」（W59 教训）。
    udp_tunnels: Mutex<HashMap<u32, Arc<std::sync::atomic::AtomicBool>>>,
    /// 本数据面实际使用的承载（"relay-tcp" / "relay-udp"）。
    carrier: &'static str,
}

impl DataPlane {
    /// 创建本地桥（TCP 承载，默认）——保持既有调用点源码兼容。
    pub fn new() -> Result<Arc<Self>, String> {
        Self::new_with_carrier("relay-tcp")
    }

    /// 从配置选择数据面承载字符串（ADR-024 阶段 2 opt-in）。
    pub fn carrier_from_config(cfg: &cr_core::Config) -> &'static str {
        if cfg.nat_data_plane_carrier.eq_ignore_ascii_case("udp") {
            "relay-udp"
        } else {
            "relay-tcp"
        }
    }

    /// 创建本地桥，指定数据面承载（`relay-tcp` / `relay-udp`，ADR-024 阶段 2 opt-in）。
    pub fn new_with_carrier(carrier: &'static str) -> Result<Arc<Self>, String> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("cr-data-plane")
            .enable_all()
            .build()
            .map_err(|e| format!("创建数据面 runtime 失败: {e}"))?;
        let bridge = Arc::new(Bridge::new());
        let listener = runtime
            .block_on(TcpListener::bind("127.0.0.1:0"))
            .map_err(|e| format!("绑定本地 SOCKS5 失败: {e}"))?;
        let addr = listener
            .local_addr()
            .map_err(|e| format!("读取 SOCKS5 地址失败: {e}"))?;
        let b = bridge.clone();
        runtime.spawn(async move { let _ = b.serve_listener(listener).await; });

        let url = format!("socks5://{addr}");
        println!("[data-plane] SOCKS5 桥就绪: {url}（opt-in）");
        Ok(Arc::new(Self {
            runtime,
            bridge,
            proxy_url: url,
            tunnels: Mutex::new(HashMap::new()),
            udp_tunnels: Mutex::new(HashMap::new()),
            carrier,
        }))
    }

    pub fn proxy_url(&self) -> &str {
        &self.proxy_url
    }

    /// 经 TCP 中继开一条数据隧道，登记到桥上，键是**引擎将要拨打的对端地址**。
    ///
    /// 未登记的地址仍按 `cr-bridge` 既有语义回落真实直连，不影响普通 BT。
    /// 登记前必须完成 OPEN/OPENED 握手：中继只保证「帧送到了」，不保证「对端接了」。
    pub fn attach_relay_tunnel(
        &self,
        relay_addr: &str,
        self_id: &[u8; NODE_ID_LEN],
        peer_id: &[u8; NODE_ID_LEN],
        channel: u32,
        advertised_peer: SocketAddr,
    ) -> Result<(), String> {
        // 订阅线程用 node_id 注册订阅，这里必须换一个会话 id，否则两条连接互相顶掉。
        let dp_id = derive_session_id(self_id, b"cr-dp-v1");
        let mut conn = RelayClient::new(relay_addr.to_string())
            .connect(CONNECT_TIMEOUT)
            .map_err(|e| format!("数据面连接中继失败: {e}"))?;
        conn.register(&dp_id)
            .map_err(|e| format!("数据面注册中继失败: {e}"))?;
        let set = RelayTunnelSet::new(conn, *peer_id)?;
        let tunnel = set.open(channel)?;
        tunnel
            .send_open(&dp_id)
            .map_err(|e| format!("发送 OPEN 失败: {e}"))?;
        tunnel.wait_opened(OPEN_TIMEOUT).map_err(|e| {
            format!("对端未确认隧道（{e}），不登记，回落直连")
        })?;

        let (bridge_side, tunnel_side) = tokio::io::duplex(CHANNEL_QUEUE * 1024);
        self.bridge
            .register_tunnel(advertised_peer, bridge_side)
            .map_err(|e| format!("登记中继隧道失败: {e}"))?;
        spawn_tunnel_pump(tunnel_side, tunnel)?;
        self.tunnels.lock().unwrap().insert(channel, set);
        println!(
            "[data-plane] 中继隧道已确认并登记: channel={channel} peer={advertised_peer} \
             relay={relay_addr} session={}",
            cr_hash::hex(&dp_id)
        );
        Ok(())
    }

    /// ADR-024 阶段 2：经 **UDP 中继**开一条可靠数据隧道并登记到桥。
    ///
    /// 与 TCP 版同语义：用派生会话 id 注册（避免顶掉订阅）、必须 OPEN/OPENED 握手、
    /// 未确认就不登记并回落直连。差异是承载换成 UDP，可靠有序由 `relay_udp_stream` 提供。
    pub fn attach_relay_udp_tunnel(
        &self,
        relay_addr: &str,
        self_id: &[u8; NODE_ID_LEN],
        peer_id: &[u8; NODE_ID_LEN],
        channel: u32,
        advertised_peer: SocketAddr,
    ) -> Result<(), String> {
        let dp_id = derive_session_id(self_id, b"cr-dp-v1");
        // 目标必须是持有方的**派生数据面会话 id**，不是它的节点 id：
        // UDP 侧持有方把数据面注册在 `derive_session_id(self_id, b"cr-dp-v1")` 上
        // （节点 id 那条会话留给订阅/唤醒）。发到节点 id 会被订阅线程当成唤醒帧丢掉，
        // 表现成「OPEN 发出去没人回，3 s 后回落直连」——这是 v2.4.94 实施期踩到的错位。
        let peer_dp = derive_session_id(peer_id, b"cr-dp-v1");
        let client = UdpRelayClient::connect(relay_addr, CONNECT_TIMEOUT)
            .map_err(|e| format!("UDP 数据面连接中继失败: {e}"))?;
        let mut client = client;
        client
            .hello(dp_id, CONNECT_TIMEOUT)
            .map_err(|e| format!("UDP 数据面注册中继失败: {e}"))?;
        let mut stream = UdpRelayStream::new(client, peer_dp)?;
        udp_send_open_and_wait(&mut stream, channel, &dp_id)?;

        let (bridge_side, stream_side) = tokio::io::duplex(CHANNEL_QUEUE * 1024);
        self.bridge
            .register_tunnel(advertised_peer, bridge_side)
            .map_err(|e| format!("登记 UDP 中继隧道失败: {e}"))?;

        let alive = Arc::new(AtomicBool::new(true));
        if let Err(e) = spawn_udp_stream_pump(stream_side, stream, alive.clone()) {
            self.udp_tunnels.lock().unwrap().remove(&channel);
            return Err(e);
        }
        self.udp_tunnels.lock().unwrap().insert(channel, alive);
        println!(
            "[data-plane] UDP 中继隧道已确认并登记: channel={channel} peer={advertised_peer} \
             relay={relay_addr} session={}",
            cr_hash::hex(&dp_id)
        );
        Ok(())
    }

    pub fn json(&self) -> cr_core::Json {
        let t = self.tunnels.lock().unwrap();
        // v2.4.93：`tunnels` 只是**登记条数**，不保证泵还活着。旧口径下泵线程静默死亡时
        // `tunnels=1` 仍然成立，看板上完全看不出问题；这里把「活着几条」单列。
        let alive = t.values().filter(|s| s.is_running()).count();
        let total = t.len();
        drop(t);
        let u = self.udp_tunnels.lock().unwrap();
        let udp_alive = u.values()
            .filter(|a| a.load(Ordering::Relaxed)).count();
        let udp_total = u.len();
        drop(u);
        cr_core::Json::obj(vec![
            ("enabled", cr_core::Json::b(true)),
            ("proxy", cr_core::Json::s(&self.proxy_url)),
            ("socks5_registered", cr_core::Json::n(self.bridge.registered() as f64)),
            ("served_registered", cr_core::Json::n(self.bridge.served_registered() as f64)),
            ("bytes_relayed", cr_core::Json::n(self.bridge.bytes_relayed() as f64)),
            ("tunnels", cr_core::Json::n(total as f64)),
            ("tunnels_alive", cr_core::Json::n(alive as f64)),
            ("carrier", cr_core::Json::s(self.carrier)),
            // ADR-024 阶段 2：UDP 承载的隧道登记数 / 活着数（与 TCP 同口径，
            // 分开列以免把「登记」误读成「活着」）。
            ("udp_tunnels", cr_core::Json::n(udp_total as f64)),
            ("udp_tunnels_alive", cr_core::Json::n(udp_alive as f64)),
        ])
    }
}

/// UDP 可靠流 ↔ tokio duplex 的双向泵。
/// 两个独立 std 线程各建一个 current_thread runtime：
/// - 引擎→流：异步读 duplex，阻塞写 UdpStreamWrite；
/// - 流→引擎：阻塞读 UdpStreamRead，异步写 duplex。
/// 阻塞的 std 读不跑在共享 runtime 上（W59 教训），两半互不持锁。
fn spawn_udp_stream_pump(
    duplex: DuplexStream,
    stream: UdpRelayStream,
    alive: Arc<AtomicBool>,
) -> Result<(), String> {
    let (read, write) = stream.split();
    let (mut duplex_read, mut duplex_write) = tokio::io::split(duplex);

    let a1 = alive.clone();
    thread::Builder::new().name("cr-udp-dp-to-stream".into()).spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let mut buf = [0u8; 16 * 1024];
            let mut wr = write;
            loop {
                match duplex_read.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if std::io::Write::write_all(&mut wr, &buf[..n]).is_err() { break; }
                        let _ = std::io::Write::flush(&mut wr);
                    }
                }
            }
        });
        a1.store(false, Ordering::Relaxed);
    }).map_err(|e| format!("启动 UDP 数据面 pump（引擎→流）失败: {e}"))?;

    let a2 = alive.clone();
    thread::Builder::new().name("cr-udp-dp-from-stream".into()).spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let mut buf = [0u8; 16 * 1024];
            let mut rd = read;
            loop {
                // std 阻塞读放在 tokio 的 spawn_blocking 语义外：这里整个线程是专用的，
                // 阻塞 100 ms 轮询不会饿死其它任务（本 runtime 只有这一个任务）。
                // std::io::Read 与 tokio::AsyncReadExt 同名，这里显式限定为阻塞读。
                match std::io::Read::read(&mut rd, &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if duplex_write.write_all(&buf[..n]).await.is_err() { break; }
                    }
                }
            }
        });
        a2.store(false, Ordering::Relaxed);
    }).map_err(|e| format!("启动 UDP 数据面 pump（流→引擎）失败: {e}"))?;

    Ok(())
}

/// std 阻塞隧道 ↔ tokio duplex。专用线程 + current_thread runtime，
/// duplex 侧用 1 ms 超时轮询，tunnel 侧用非阻塞 `try_read`；不在共享 runtime 上阻塞。
fn spawn_tunnel_pump(duplex: DuplexStream, tunnel: RelayTunnel) -> Result<(), String> {
    std::thread::Builder::new()
        .name("cr-data-plane-tunnel".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tunnel pump runtime");
            rt.block_on(async move {
                let mut duplex = duplex;
                let mut tunnel = tunnel;
                let mut buf = [0u8; 16 * 1024];
                loop {
                    let mut progress = false;
                    match tokio::time::timeout(Duration::from_millis(1), duplex.read(&mut buf)).await {
                        Ok(Ok(0)) => break,
                        Ok(Ok(n)) => {
                            if tunnel.write_all(&buf[..n]).is_err() { break; }
                            progress = true;
                        }
                        Ok(Err(_)) => break,
                        Err(_) => {}
                    }
                    match tunnel.try_read(&mut buf) {
                        Ok(n) if n > 0 => {
                            if tokio::time::timeout(Duration::from_secs(2), duplex.write_all(&buf[..n]))
                                .await
                                .is_err()
                            {
                                break;
                            }
                            progress = true;
                        }
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                        Err(_) => break,
                    }
                    if !progress {
                        tokio::task::yield_now().await;
                    }
                }
            });
        })
        .map_err(|e| format!("启动隧道泵失败: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_nat::relay_server::RelayServer;

    /// 关键红线：**对端不在线时不得登记隧道**。
    ///
    /// 中继只保证「帧送到了中继」，不保证「对端接了」。如果不做 OPEN/OPENED 就登记，
    /// 一条空隧道会被写进 SOCKS5 桥，引擎拨过去只看到 EOF——现场表现是「BT 连不上」，
    /// 而日志里全是「隧道已登记」。这里断言它如实失败并说明原因。
    #[test]
    fn attach_without_peer_confirmation_fails_honestly() {
        let server = RelayServer::bind("127.0.0.1:0").unwrap();
        let addr = server.local_addr().unwrap().to_string();
        let handle = server.spawn();
        let dp = DataPlane::new().expect("数据面应能创建（回环 SOCKS5）");
        let self_id = [0xA1u8; NODE_ID_LEN];
        let peer = [0xB2u8; NODE_ID_LEN];
        let target: SocketAddr = "203.0.113.31:17902".parse().unwrap();

        let err = dp.attach_relay_tunnel(&addr, &self_id, &peer, 0x1234, target)
            .expect_err("对端未回 OPENED 时必须失败");
        assert!(err.contains("未确认"), "错误要说清是「没确认」而不是笼统失败: {err}");
        assert_eq!(dp.json().get("tunnels").and_then(cr_core::Json::as_i64), Some(0),
                   "失败的 attach 不得留下隧道条目");
        handle.stop();
    }

    /// ADR-024 阶段 2 红线（UDP 版）：持有方不回 OPENED 时，`attach_relay_udp_tunnel`
    /// 必须如实失败，不得把空隧道登记进桥。与 TCP 版同口径。
    #[test]
    fn udp_attach_without_peer_confirmation_fails_honestly() {
        let srv = cr_nat::relay_udp::UdpRelayServer::bind("127.0.0.1:0").unwrap()
            .with_psk(cr_nat::relay::default_psk());
        let addr = srv.local_addr().unwrap().to_string();
        let handle = srv.spawn();
        let dp = DataPlane::new_with_carrier("relay-udp").unwrap();
        let self_id = [0xA3u8; NODE_ID_LEN];
        let peer = [0xB3u8; NODE_ID_LEN];
        let target: SocketAddr = "203.0.113.32:17902".parse().unwrap();

        let err = dp.attach_relay_udp_tunnel(&addr, &self_id, &peer, 0x1235, target)
            .expect_err("持有方未回 OPENED 时必须失败");
        assert!(
            err.contains("OPENED") || err.contains("确认") || err.contains("超时"),
            "错误要说明是握手未确认，而不是笼统失败: {err}"
        );
        assert_eq!(
            dp.json().get("udp_tunnels").and_then(cr_core::Json::as_i64), Some(0),
            "失败的 UDP attach 不得留下隧道条目"
        );
        assert_eq!(dp.json().get("carrier").and_then(cr_core::Json::as_str), Some("relay-udp"));
        handle.stop();
    }

    /// 正例：持有方**确实**回了 OPENED 时，隧道必须登记并与「活着」分开计数。
    ///
    /// 这条用例的真正价值是钉住**寻址口径**：持有方的数据面会话注册在
    /// `derive_session_id(node_id, b"cr-dp-v1")` 上，发起方必须发到这个派生 id 而不是节点 id。
    /// 发错目标时 OPEN 会被持有方的订阅线程当唤醒帧丢掉，只表现为 3 s 超时回落直连，
    /// 看起来像「对端没上线」——没有这条正例，负例永远「通过」。
    #[test]
    fn udp_attach_registers_tunnel_when_holder_confirms() {
        let srv = cr_nat::relay_udp::UdpRelayServer::bind("127.0.0.1:0").unwrap()
            .with_psk(cr_nat::relay::default_psk());
        let addr = srv.local_addr().unwrap().to_string();
        let handle = srv.spawn();
        let dp = DataPlane::new_with_carrier("relay-udp").unwrap();
        let self_id = [0xA4u8; NODE_ID_LEN];
        let peer = [0xB4u8; NODE_ID_LEN];
        let target: SocketAddr = "203.0.113.34:17904".parse().unwrap();
        let holder_dp = derive_session_id(&peer, b"cr-dp-v1");
        let expect_initiator = derive_session_id(&self_id, b"cr-dp-v1");

        let (opened_tx, opened_rx) = std::sync::mpsc::channel();
        let holder = {
            let addr = addr.clone();
            std::thread::spawn(move || {
                let mut cb = UdpRelayClient::connect(&addr, Duration::from_millis(1500)).unwrap();
                cb.hello(holder_dp, Duration::from_millis(1500)).unwrap();
                let mut stream = UdpRelayStream::new(cb, [0u8; NODE_ID_LEN]).unwrap();
                let (channel, initiator) =
                    cr_nat::relay_udp_stream::recv_open(&mut stream).unwrap();
                assert_eq!(channel, 0x1236);
                assert_eq!(initiator, expect_initiator, "声明的发起方必须与注册会话一致");
                cr_nat::relay_udp_stream::send_opened(&mut stream).unwrap();
                opened_tx.send(channel).unwrap();
                // 保持会话活着，直到发起方 drop（收到 EOF）。
                let mut buf = [0u8; 64];
                let _ = std::io::Read::read(&mut stream, &mut buf);
            })
        };

        dp.attach_relay_udp_tunnel(&addr, &self_id, &peer, 0x1236, target)
            .expect("持有方已回 OPENED，必须登记成功");
        assert_eq!(opened_rx.recv().unwrap(), 0x1236);
        let j = dp.json();
        assert_eq!(j.get("udp_tunnels").and_then(cr_core::Json::as_i64), Some(1));
        assert_eq!(j.get("udp_tunnels_alive").and_then(cr_core::Json::as_i64), Some(1));
        drop(dp);
        holder.join().unwrap();
        handle.stop();
    }
}
