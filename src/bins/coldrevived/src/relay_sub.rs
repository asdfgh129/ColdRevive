//! M2/W3.4 持有者侧中继订阅线程。
//!
//! W3.3 的中继回退由 T2 的 mock 桥「服务端自己发 UDP」完成，服务端掺了业务逻辑，
//! 无法部署。W3.4 改为：
//!
//!   请求侧 `RELAY(target=持有者, WAKEUP_REQ)`
//!     → [`cr_nat::relay_server`] 纯帧路由，封装 `DELIVERED(node=请求会话 id)`
//!     → 本线程把负载经**真实 UDP** 送进本守护进程的唤醒监听端口
//!     → 守护进程 `handle_udp_frame` 产生**真实 WAKEUP_ACK**
//!     → 本线程 `RELAY(target=请求会话 id, WAKEUP_ACK)`
//!     → 服务端回投 `DELIVERED` 给请求侧。
//!
//! 订阅注册用稳定 node id（便于被 `node_id_16(peer.node_id)` 命中）；请求侧用一次性
//! 会话 id 注册，避免同一节点两个连接抢占注册表表项。中继服务端不解析唤醒协议。
//!
//! **红线**：本线程不构造、不改写任何业务应答；UDP 无应答时静默跳过，让请求侧如实超时。
//! `nat` 配置段缺省时完全不启动，T0/T1 行为不变。

use cr_core::Config;
use cr_nat::node_id_16;
use cr_nat::relay::{FrameKind, RelayClient, NODE_ID_LEN};
use std::net::UdpSocket;
use std::thread;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const WAKE_REPLY_TIMEOUT: Duration = Duration::from_millis(1500);
const RECONNECT_DELAY: Duration = Duration::from_secs(1);

/// 守护进程 UDP 监听绑在 `cfg.bind`；`0.0.0.0` 时本机用回环地址投递。
pub fn local_wake_host(bind: &str) -> String {
    match bind {
        "0.0.0.0" | "::" | "[::]" | "" => "127.0.0.1".to_string(),
        other => other.to_string(),
    }
}

/// 只在 `nat.enabled && nat.relay` 存在时启动；失败自动重连，不阻塞主线程。
pub fn spawn(cfg: &Config) {
    if !cfg.nat_enabled {
        return;
    }
    let Some(relay) = cfg.nat_relay.clone() else {
        return;
    };
    let wake_port = match cfg.me() {
        Ok(m) => m.wake_udp_port,
        Err(_) => return,
    };
    if wake_port == 0 {
        return;
    }
    let self_id = node_id_16(&cfg.node_id);
    let wake_host = local_wake_host(&cfg.bind);
    let label = cfg.node_id.clone();
    thread::spawn(move || loop {
        match serve(&relay, &self_id, &wake_host, wake_port) {
            Ok(()) => {}
            Err(e) => eprintln!("[{}][relay-sub] 订阅中断: {}，1s 后重连", label, e),
        }
        thread::sleep(RECONNECT_DELAY);
    });
}

fn serve(
    relay: &str,
    self_id: &[u8; NODE_ID_LEN],
    wake_host: &str,
    wake_port: u16,
) -> Result<(), String> {
    let client = RelayClient::new(relay.to_string());
    let mut conn = client.connect(CONNECT_TIMEOUT)?;
    // register 阶段保留 5s 读超时；注册成功后转入长连接阻塞等待 DELIVERED。
    conn.register(self_id)?;
    conn.set_read_timeout(None)?;

    let udp = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("订阅 UDP 绑定失败: {}", e))?;
    udp.set_read_timeout(Some(WAKE_REPLY_TIMEOUT))
        .map_err(|e| e.to_string())?;
    let target = format!("{}:{}", wake_host, wake_port);
    let mut buf = [0u8; 65535];

    loop {
        let frame = conn.recv_frame()?;
        if frame.kind != FrameKind::Delivered || frame.payload.is_empty() {
            continue;
        }
        let source = frame.node;
        udp.send_to(&frame.payload, &target)
            .map_err(|e| format!("唤醒 UDP 发送失败: {}", e))?;
        let (n, _) = match udp.recv_from(&mut buf) {
            Ok(v) => v,
            // 守护进程拒绝/忽略该帧：不合成应答，请求侧按自身超时如实失败。
            Err(_) => continue,
        };
        conn.send_relay(&source, &buf[..n])?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_bind_uses_loopback_for_local_delivery() {
        assert_eq!(local_wake_host("0.0.0.0"), "127.0.0.1");
        assert_eq!(local_wake_host("::"), "127.0.0.1");
        assert_eq!(local_wake_host(""), "127.0.0.1");
    }

    #[test]
    fn concrete_bind_is_used_verbatim() {
        assert_eq!(local_wake_host("192.168.1.210"), "192.168.1.210");
        assert_eq!(local_wake_host("127.0.0.1"), "127.0.0.1");
    }
}
