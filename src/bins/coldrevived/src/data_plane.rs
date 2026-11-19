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
use std::collections::HashMap;
use std::io::{ErrorKind, Write};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
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
}

impl DataPlane {
    /// 创建本地桥（绑定回环临时端口），返回可给 `EngineOptions.proxy_url` 用的 URL。
    pub fn new() -> Result<Arc<Self>, String> {
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

    pub fn json(&self) -> cr_core::Json {
        cr_core::Json::obj(vec![
            ("enabled", cr_core::Json::b(true)),
            ("proxy", cr_core::Json::s(&self.proxy_url)),
            ("socks5_registered", cr_core::Json::n(self.bridge.registered() as f64)),
            ("served_registered", cr_core::Json::n(self.bridge.served_registered() as f64)),
            ("bytes_relayed", cr_core::Json::n(self.bridge.bytes_relayed() as f64)),
            ("tunnels", cr_core::Json::n(self.tunnels.lock().unwrap().len() as f64)),
            ("carrier", cr_core::Json::s("relay-tcp")),
        ])
    }
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
}
