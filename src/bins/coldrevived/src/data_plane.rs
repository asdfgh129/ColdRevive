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

use cr_bridge::{splice_to_listener, Bridge};
use cr_nat::relay::{RelayClient, NODE_ID_LEN};
use cr_nat::relay_tunnel::{derive_session_id, RelayTunnel, RelayTunnelSet, OPEN_TIMEOUT};
use cr_nat::relay_udp::UdpRelayClient;
use cr_nat::relay_udp_stream::{
    send_open_and_wait as udp_send_open_and_wait, CcStats, CongestionConfig, UdpRelayStream,
};
use std::collections::HashMap;
use std::io::{ErrorKind, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::net::{TcpListener, TcpStream as TokioTcpStream};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const CHANNEL_QUEUE: usize = 64;
/// 打洞 socket 被消费之后，等生产者补装的窗口。
///
/// 取值理由：一条打洞 socket 只服务一次 CONNECT，而 BT 的 peer 会话本来就是
/// 「传一会儿、断开、再连」。窗口太短会把可恢复的重连记成网络失败；太长则
/// 让一条真正死亡的路径长时间占用引擎的连接槽。6 s 与 BT 常见的 peer 超时同量级。
/// 这只影响**登记过**的目标；未知目标仍然立刻直连（见 `Bridge::take_for_connect`）。
const PUNCHED_REARM_WAIT: Duration = Duration::from_secs(6);

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
    /// ADR-024 阶段 3：UDP 可靠层拥塞控制配置（默认关闭）。
    cc: CongestionConfig,
    /// 最近一次 UDP 流泵的 CC 快照（看板口径，不跨流汇总）。
    cc_last: Arc<Mutex<CcStats>>,
    /// ADR-021/022：打洞成功后真正登记进桥的 socket（登记数 ≠ 泵还活着）。
    punched: Mutex<HashMap<SocketAddr, Arc<AtomicBool>>>,
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

    /// 从配置创建：承载与 UDP 拥塞控制都遵循 opt-in，老配置逐字节不变。
    pub fn new_from_config(cfg: &cr_core::Config) -> Result<Arc<Self>, String> {
        let cc = CongestionConfig {
            enabled: cfg.nat_udp_cc_enabled,
            cwnd_init: cfg.nat_udp_cc_cwnd_init,
            cwnd_min: 1,
            cwnd_max: 32,
            beta: cfg.nat_udp_cc_beta_x100 as f64 / 100.0,
            pacing: cfg.nat_udp_cc_pacing,
        };
        Self::new_with_carrier_cc(Self::carrier_from_config(cfg), cc)
    }

    /// 创建本地桥，指定数据面承载（`relay-tcp` / `relay-udp`，ADR-024 阶段 2 opt-in）。
    pub fn new_with_carrier(carrier: &'static str) -> Result<Arc<Self>, String> {
        Self::new_with_carrier_cc(carrier, CongestionConfig::default())
    }

    fn new_with_carrier_cc(
        carrier: &'static str,
        cc: CongestionConfig,
    ) -> Result<Arc<Self>, String> {
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
            cc,
            cc_last: Arc::new(Mutex::new(CcStats::default())),
            punched: Mutex::new(HashMap::new()),
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
        let mut stream = UdpRelayStream::new_with_cc(client, peer_dp, self.cc)?;
        udp_send_open_and_wait(&mut stream, channel, &dp_id)?;
        *self.cc_last.lock().unwrap() = stream.cc_stats();

        let (bridge_side, stream_side) = tokio::io::duplex(CHANNEL_QUEUE * 1024);
        self.bridge
            .register_tunnel(advertised_peer, bridge_side)
            .map_err(|e| format!("登记 UDP 中继隧道失败: {e}"))?;

        let alive = Arc::new(AtomicBool::new(true));
        if let Err(e) = spawn_udp_stream_pump(
            stream_side,
            stream,
            alive.clone(),
            self.cc_last.clone(),
        ) {
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

    /// ADR-021/022：把**已经通过 poll(POLLOUT)+SO_ERROR 与 nonce 握手验证**的直连
    /// socket 登记进本地 SOCKS5 桥。此前 `nat punch-route` 验完就 drop，等于只做了判据
    /// 没接数据面；这里补上真正的 handoff。未登记的 socket 不会被引擎用到。
    pub fn attach_punched_socket(
        &self,
        advertised_peer: SocketAddr,
        stream: std::net::TcpStream,
    ) -> Result<(), String> {
        // tokio 1.54.1 的 `from_std` 会 debug_assert socket 必须是非阻塞的；
        // 这里显式设 true，不能沿用打洞阶段的阻塞状态（此前设 false 会让注册直接 panic）。
        stream.set_nonblocking(true)
            .map_err(|e| format!("punched socket 设非阻塞失败: {e}"))?;
        // `from_std` 必须在 runtime 上下文里调用；CLI/控制面不保证调用方已经在 tokio 中。
        let socket = {
            let _guard = self.runtime.enter();
            TokioTcpStream::from_std(stream)
                .map_err(|e| format!("punched socket 接入 runtime 失败: {e}"))?
        };
        let (bridge_side, socket_side) = tokio::io::duplex(CHANNEL_QUEUE * 1024);
        self.bridge
            .register_tunnel(advertised_peer, bridge_side)
            .map_err(|e| format!("登记打洞 socket 失败: {e}"))?;

        let alive = Arc::new(AtomicBool::new(true));
        let (mut from_bridge, mut to_bridge) = tokio::io::split(socket_side);
        let (mut from_peer, mut to_peer) = tokio::io::split(socket);
        let a1 = alive.clone();
        self.runtime.spawn(async move {
            let _ = tokio::io::copy(&mut from_bridge, &mut to_peer).await;
            a1.store(false, Ordering::Relaxed);
        });
        let a2 = alive.clone();
        self.runtime.spawn(async move {
            let _ = tokio::io::copy(&mut from_peer, &mut to_bridge).await;
            a2.store(false, Ordering::Relaxed);
        });
        self.punched.lock().unwrap().insert(advertised_peer, alive);
        // 打洞路径打开「等补装」窗口：一条打洞 socket 只服务一次 CONNECT，
        // BT 断开重连是常态。窗口关着的话，重连会立刻去拨一个只在打洞存活期间
        // 有效的公网映射，必然 RST —— 现场就是「传了一半再也接不上」。
        self.bridge.set_rearm_wait(PUNCHED_REARM_WAIT);
        println!(
            "[data-plane] 打洞直连 socket 已登记: peer={advertised_peer}（is_s6_evidence=false）"
        );
        Ok(())
    }

    /// ADR-021 入向半边：`punch-route` 在**持有者侧**已建立并验明身份的 socket
    /// 必须拼到 `engine.listen`，否则引擎不知道它存在——发起方 SOCKS5 桥命中后
    /// 只能连到对端公网映射，BT 握手会落到无人监听的端口。
    ///
    /// 与 `attach_punched_socket` 的完整区别：
    /// - member（发起方）登记到 SOCKS5 桥，键是引擎会拨的对端静态地址；
    /// - holder（接收方）把 socket 当作普通入站连接交给本地引擎监听口。
    ///
    /// `engine_listen` 必须是**实测**的引擎回环地址；配置缺省时调用方应报错，
    /// 不能猜 `bt_port`（打洞 socket 自己就占着它）。
    pub fn attach_punched_inbound(
        &self,
        advertised_peer: SocketAddr,
        stream: std::net::TcpStream,
        engine_listen: SocketAddr,
    ) -> Result<(), String> {
        stream.set_nonblocking(true)
            .map_err(|e| format!("punched 入向 socket 设非阻塞失败: {e}"))?;
        let socket = {
            let _guard = self.runtime.enter();
            TokioTcpStream::from_std(stream)
                .map_err(|e| format!("punched 入向 socket 接入 runtime 失败: {e}"))?
        };
        let alive = Arc::new(AtomicBool::new(true));
        let a = alive.clone();
        self.runtime.spawn(async move {
            let _ = splice_to_listener(socket, engine_listen).await;
            a.store(false, Ordering::Relaxed);
        });
        self.punched.lock().unwrap().insert(advertised_peer, alive);
        println!(
            "[data-plane] 打洞直连接入引擎监听口: peer={advertised_peer} \
             engine={engine_listen}（is_s6_evidence=false）"
        );
        Ok(())
    }

    /// 打洞入向：先看 68 字节 BT 握手，再决定唤醒与 splice。
    ///
    /// 与 `attach_punched_inbound` 的区别只有一点：这条路径**必须**先读握手。
    /// 冷种子不会响应入向握手（引擎里的任务还处于 paused），直接把 socket 拼过去
    /// 会得到「握手到了、引擎不回、10 s 后 FIN」——2026-11-27 W6.7 的现场。
    /// `wake` 返回 false 表示拒绝（FR-15 上限/非标准握手），此时按零信任关连接。
    pub fn attach_punched_inbound_waking<F>(
        &self,
        advertised_peer: SocketAddr,
        mut stream: std::net::TcpStream,
        engine_listen: SocketAddr,
        wake: F,
    ) -> Result<(), String>
    where
        F: FnOnce(&[u8]) -> bool + Send + 'static,
    {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(30)));
        let alive = Arc::new(AtomicBool::new(true));
        let a = alive.clone();
        std::thread::spawn(move || {
            // 5 s 读超时：裸 connect 与慢速客户端不能长期挂线程。
            match cr_guard::read_handshake(&mut stream, std::time::Duration::from_secs(5)) {
                Ok(prefix) => {
                    if wake(&prefix) {
                        if let Err(e) = cr_guard::splice_prefaced(stream, engine_listen, &prefix) {
                            eprintln!("[data-plane] 打洞入向 splice 失败: {e}");
                        }
                    }
                }
                Err(e) => {
                    // 读不到握手就按原样拼：由引擎自己按真实字节回答，不替它下结论。
                    eprintln!("[data-plane] 打洞入向读握手失败（按原样 splice）: {e}");
                    if let Err(e) = cr_guard::splice_prefaced(stream, engine_listen, &[]) {
                        eprintln!("[data-plane] 打洞入向 splice 失败: {e}");
                    }
                }
            }
            a.store(false, Ordering::Relaxed);
        });
        self.punched.lock().unwrap().insert(advertised_peer, alive);
        println!(
            "[data-plane] 打洞直连接入引擎监听口（先唤醒再 splice）: peer={advertised_peer} \
             engine={engine_listen}（is_s6_evidence=false）"
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
        let cc = *self.cc_last.lock().unwrap();
        let p = self.punched.lock().unwrap();
        let punched_total = p.len();
        let punched_alive = p.values()
            .filter(|a| a.load(Ordering::Relaxed)).count();
        drop(p);
        cr_core::Json::obj(vec![
            ("enabled", cr_core::Json::b(true)),
            ("proxy", cr_core::Json::s(&self.proxy_url)),
            ("socks5_registered", cr_core::Json::n(self.bridge.registered() as f64)),
            ("served_registered", cr_core::Json::n(self.bridge.served_registered() as f64)),
            ("bytes_relayed", cr_core::Json::n(self.bridge.bytes_relayed() as f64)),
            // 口径标注：bytes_relayed 是写入本地 SOCKS5/隧道的字节，不是对端确认收到。
            // 真正的"对端落地"证据只能是 e2e 里读对端文件 md5（见 W59/W61）。
            ("bytes_relayed_scope", cr_core::Json::s("local_tunnel_writes_not_peer_receipt")),
            ("tunnels", cr_core::Json::n(total as f64)),
            ("tunnels_alive", cr_core::Json::n(alive as f64)),
            ("carrier", cr_core::Json::s(self.carrier)),
            // ADR-024 阶段 2：UDP 承载的隧道登记数 / 活着数（与 TCP 同口径，
            // 分开列以免把「登记」误读成「活着」）。
            ("udp_tunnels", cr_core::Json::n(udp_total as f64)),
            ("udp_tunnels_alive", cr_core::Json::n(udp_alive as f64)),
            ("punched_registered", cr_core::Json::n(punched_total as f64)),
            ("punched_alive", cr_core::Json::n(punched_alive as f64)),
            // 可重入登记口径：`socks5_awaiting` 是「登记过、会话已结束、当前没有 socket、
            // 真的等着重新填装」的键数；`socks5_in_use` 是「正被一条 BT 会话用着」的键数。
            // 把两者混成一个数会诱导 harness 在传输正常时去重打洞——那时旧 socket
            // 还占着本地端口，重绑只会拿到 EADDRNOTAVAIL。三者都不参与 pass 判定。
            ("socks5_awaiting", cr_core::Json::n(self.bridge.awaiting() as f64)),
            ("socks5_in_use", cr_core::Json::n(self.bridge.in_use() as f64)),
            ("socks5_rearmed", cr_core::Json::n(self.bridge.rearmed() as f64)),
            ("socks5_served_after_rearm", cr_core::Json::n(self.bridge.wait_served() as f64)),
            ("socks5_rearm_wait_ms", cr_core::Json::n(self.bridge.rearm_wait().as_millis() as f64)),
            // ADR-024 阶段 3：机制可观测。这里只是本进程最近一条 UDP 流的快照，
            // 不是跨公网公平性结论，也不参与任何 pass 判定。
            ("udp_cc_enabled", cr_core::Json::b(cc.enabled)),
            ("udp_cc_cwnd_x100", cr_core::Json::n(cc.cwnd_x100 as f64)),
            ("udp_cc_srtt_ms", cr_core::Json::n(cc.srtt_ms as f64)),
            ("udp_cc_loss_events", cr_core::Json::n(cc.loss_events as f64)),
            ("udp_cc_rtt_samples", cr_core::Json::n(cc.rtt_samples as f64)),
            ("udp_cc_paced_new", cr_core::Json::n(cc.paced_new_segments as f64)),
            ("udp_cc_backpressure", cr_core::Json::n(cc.relay_backpressure_events as f64)),
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
    cc_last: Arc<Mutex<CcStats>>,
) -> Result<(), String> {
    let (read, write) = stream.split();
    let (mut duplex_read, mut duplex_write) = tokio::io::split(duplex);

    let a1 = alive.clone();
    let cc1 = cc_last.clone();
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
                        *cc1.lock().unwrap() = wr.cc_stats();
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

    /// ADR-021/022：打洞 socket 必须经过本地 SOCKS5 桥真的把字节送到引擎侧。
    /// 只断言 `/health.punched_registered` 会重犯「登记数不是活着数」的错。
    #[test]
    fn punched_socket_registration_carries_real_bytes() {
        use std::io::{Read as _, Write as _};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let listener_addr = listener.local_addr().unwrap();
        let peer_thread = std::thread::spawn(move || {
            let (mut peer, _) = listener.accept().unwrap();
            let mut buf = [0u8; 5];
            peer.read_exact(&mut buf).unwrap();
            assert_eq!(&buf, b"hello");
            peer.write_all(b"world").unwrap();
            peer.flush().unwrap();
        });

        let dp = DataPlane::new().unwrap();
        let advertised: SocketAddr = "203.0.113.9:4662".parse().unwrap();
        let socket = std::net::TcpStream::connect(listener_addr).unwrap();
        dp.attach_punched_socket(advertised, socket).unwrap();
        let j = dp.json();
        assert_eq!(j.get("punched_registered").and_then(cr_core::Json::as_i64), Some(1));
        assert_eq!(j.get("punched_alive").and_then(cr_core::Json::as_i64), Some(1));

        let proxy: SocketAddr = dp.proxy_url()
            .trim_start_matches("socks5://").parse().unwrap();
        let mut client = std::net::TcpStream::connect(proxy).unwrap();
        client.write_all(&[0x05, 0x01, 0x00]).unwrap();
        let mut neg = [0u8; 2];
        client.read_exact(&mut neg).unwrap();
        assert_eq!(neg, [0x05, 0x00], "SOCKS5 协商必须成功");
        let ip = match advertised.ip() {
            std::net::IpAddr::V4(v) => v.octets().to_vec(),
            std::net::IpAddr::V6(_) => panic!("测试用 IPv4 文档地址"),
        };
        let mut req = vec![0x05, 0x01, 0x00, 0x01];
        req.extend_from_slice(&ip);
        req.extend_from_slice(&advertised.port().to_be_bytes());
        client.write_all(&req).unwrap();
        let mut reply = [0u8; 10];
        client.read_exact(&mut reply).unwrap();
        assert_eq!(reply[1], 0x00, "已登记地址必须 CONNECT 成功");

        client.write_all(b"hello").unwrap();
        let mut echo = [0u8; 5];
        client.read_exact(&mut echo).unwrap();
        assert_eq!(&echo, b"world", "打洞 socket 必须真实承载双向字节");

        // 对端回完 world 就关闭，copy 任务随即结束；此时 alive=0 是正确收尾，不是失败。
        let j = dp.json();
        assert_eq!(j.get("punched_registered").and_then(cr_core::Json::as_i64), Some(1));
        peer_thread.join().unwrap();
    }

    /// 可重入登记（v2.4.160）：同一个目标地址，第二条打洞 socket 必须能重新填装，
    /// 并且**第二条会话的字节要真的走新的 socket**。
    ///
    /// W6.7 的现场是：第一条连接传了 512 KiB 就被对端 RST，之后 BT 重连同一目标时
    /// 桥里已经没有登记项（`take()` 取走即删），只能回落直连一个失效的公网映射。
    /// 这里钉住「填装 → 消费 → 再填装 → 再消费」这条路径。
    #[test]
    fn punched_socket_reregistration_serves_a_second_session() {
        use std::io::{Read as _, Write as _};

        // 一个只服务一次的对端：读 3 字节，回 4 字节，然后关闭。
        fn one_shot_peer(expect_in: &'static [u8], send_out: &'static [u8])
            -> (SocketAddr, std::thread::JoinHandle<()>)
        {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = l.local_addr().unwrap();
            let h = std::thread::spawn(move || {
                let (mut s, _) = l.accept().unwrap();
                let mut buf = vec![0u8; expect_in.len()];
                s.read_exact(&mut buf).unwrap();
                assert_eq!(buf, expect_in, "第一条/第二条会话必须各自看到自己的字节");
                s.write_all(send_out).unwrap();
                s.flush().unwrap();
            });
            (addr, h)
        }

        fn socks5_connect_to(proxy: SocketAddr, target: SocketAddr) -> std::net::TcpStream {
            let mut c = std::net::TcpStream::connect(proxy).unwrap();
            c.write_all(&[0x05, 0x01, 0x00]).unwrap();
            let mut neg = [0u8; 2];
            c.read_exact(&mut neg).unwrap();
            assert_eq!(neg, [0x05, 0x00]);
            let mut req = vec![0x05, 0x01, 0x00, 0x01];
            match target.ip() {
                std::net::IpAddr::V4(v) => req.extend_from_slice(&v.octets()),
                std::net::IpAddr::V6(_) => panic!("测试用 IPv4 文档地址"),
            }
            req.extend_from_slice(&target.port().to_be_bytes());
            c.write_all(&req).unwrap();
            let mut reply = [0u8; 10];
            c.read_exact(&mut reply).unwrap();
            assert_eq!(reply[1], 0x00, "已登记地址必须 CONNECT 成功");
            c
        }

        let dp = DataPlane::new().unwrap();
        let advertised: SocketAddr = "203.0.113.11:4662".parse().unwrap();
        let proxy: SocketAddr =
            dp.proxy_url().trim_start_matches("socks5://").parse().unwrap();

        // 第一条会话。
        let (peer1, h1) = one_shot_peer(b"one", b"one!");
        dp.attach_punched_socket(advertised, std::net::TcpStream::connect(peer1).unwrap())
            .unwrap();
        {
            let mut c = socks5_connect_to(proxy, advertised);
            c.write_all(b"one").unwrap();
            let mut got = [0u8; 4];
            c.read_exact(&mut got).unwrap();
            assert_eq!(&got, b"one!");
        }
        h1.join().unwrap();
        // 第一条被消费后必须留下占位，而不是让键消失。会话收尾在 copy 返回后
        // 才发生，所以给一个很短的等待窗口，不假设同一瞬间完成。
        let mut awaiting = 0i64;
        for _ in 0..200 {
            awaiting = dp.json().get("socks5_awaiting")
                .and_then(cr_core::Json::as_i64).unwrap_or(0);
            if awaiting == 1 { break; }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(awaiting, 1, "会话结束后必须留占位");

        // 第二条会话：重新打洞，同一个键必须允许填装。
        let (peer2, h2) = one_shot_peer(b"two", b"two!");
        dp.attach_punched_socket(advertised, std::net::TcpStream::connect(peer2).unwrap())
            .expect("同一目标地址的第二条打洞 socket 必须允许重新填装");
        {
            let mut c = socks5_connect_to(proxy, advertised);
            c.write_all(b"two").unwrap();
            let mut got = [0u8; 4];
            c.read_exact(&mut got).unwrap();
            assert_eq!(&got, b"two!", "第二条会话必须走新的 socket");
        }
        h2.join().unwrap();

        let j = dp.json();
        assert_eq!(j.get("socks5_rearmed").and_then(cr_core::Json::as_i64), Some(1));
        assert_eq!(j.get("served_registered").and_then(cr_core::Json::as_i64), Some(2),
                   "两次会话都必须命中桥");
        assert_eq!(j.get("socks5_rearm_wait_ms").and_then(cr_core::Json::as_i64),
                   Some(PUNCHED_REARM_WAIT.as_millis() as i64),
                   "打洞路径必须打开补装窗口");
    }

    /// ADR-021 入向半边：holder 的打洞 socket 必须经 `engine.listen` 到达真引擎，
    /// 不能只登记 SOCKS5 桥（那样对端拨静态地址时本端没人接）。
    #[test]
    fn punched_inbound_splice_reaches_engine_listener() {
        use std::io::{Read as _, Write as _};
        use std::time::Duration as StdDuration;

        // 假引擎：回环监听，读到 hello 后回 world。
        let engine = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let engine_addr = engine.local_addr().unwrap();
        let engine_thread = std::thread::spawn(move || {
            let (mut s, _) = engine.accept().unwrap();
            let mut buf = [0u8; 5];
            s.read_exact(&mut buf).unwrap();
            assert_eq!(&buf, b"hello");
            s.write_all(b"world").unwrap();
        });

        // 假对端：已建立的 socket 交给入向 splice。
        let peer_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let peer_addr = peer_listener.local_addr().unwrap();
        let dp = DataPlane::new().unwrap();
        let advertised: SocketAddr = "112.5.60.4:19910".parse().unwrap();
        let join = std::thread::spawn(move || {
            let (stream, _) = peer_listener.accept().unwrap();
            dp.attach_punched_inbound(advertised, stream, engine_addr).unwrap();
            dp
        });
        let mut client = std::net::TcpStream::connect(peer_addr).unwrap();
        client.write_all(b"hello").unwrap();
        client.set_read_timeout(Some(StdDuration::from_secs(3))).unwrap();
        let mut echo = [0u8; 5];
        client.read_exact(&mut echo).unwrap();
        assert_eq!(&echo, b"world", "打洞入向必须真的到达引擎监听口");
        engine_thread.join().unwrap();
        let dp = join.join().unwrap();
        assert_eq!(dp.json().get("punched_registered").and_then(cr_core::Json::as_i64), Some(1));
    }

    /// 冷种子唤醒的入向半边：打洞 socket 上的 68 字节握手必须先被读到、交给唤醒
    /// 裁决，然后**原样补回**引擎。丢前缀的形态与 ADR-021 记的坑一致——唤醒看起来
    /// 成功，引擎收到截断握手，取片 0 B。
    #[test]
    fn punched_inbound_waking_restores_handshake_prefix() {
        use std::io::{Read as _, Write as _};
        use std::time::Duration as StdDuration;

        let hs = cr_guard::handshake_bytes(&[0x11; 20], &[0x22; 20]);
        let engine = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let engine_addr = engine.local_addr().unwrap();
        let engine_thread = std::thread::spawn(move || {
            let (mut s, _) = engine.accept().unwrap();
            let mut buf = [0u8; cr_guard::HANDSHAKE_LEN];
            s.read_exact(&mut buf).unwrap();
            assert_eq!(buf, hs, "唤醒后必须把已读的 68 字节补回引擎");
            s.write_all(&[0xAB; cr_guard::HANDSHAKE_LEN]).unwrap();
        });

        let peer_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let peer_addr = peer_listener.local_addr().unwrap();
        let dp = DataPlane::new().unwrap();
        let advertised: SocketAddr = "112.5.60.4:19910".parse().unwrap();
        let join = std::thread::spawn(move || {
            let (stream, _) = peer_listener.accept().unwrap();
            dp.attach_punched_inbound_waking(advertised, stream, engine_addr, move |prefix| {
                assert_eq!(prefix, &hs[..], "唤醒裁决必须拿到完整握手");
                true
            })
            .unwrap();
            dp
        });

        let mut client = std::net::TcpStream::connect(peer_addr).unwrap();
        client.write_all(&hs).unwrap();
        client.set_read_timeout(Some(StdDuration::from_secs(5))).unwrap();
        let mut reply = [0u8; cr_guard::HANDSHAKE_LEN];
        client.read_exact(&mut reply).unwrap();
        assert_eq!(reply, [0xAB; cr_guard::HANDSHAKE_LEN], "引擎的回应必须原路回到打洞对端");
        engine_thread.join().unwrap();
        let dp = join.join().unwrap();
        assert_eq!(dp.json().get("punched_registered").and_then(cr_core::Json::as_i64), Some(1));
    }

    /// 负向对照：唤醒裁决拒绝（FR-15 上限/非标准握手）时不得 splice 到引擎。
    /// 否则「阈值拒绝」只是日志里一句话，连接照样被喂进引擎。
    #[test]
    fn punched_inbound_waking_refusal_does_not_splice() {
        use std::io::{Read as _, Write as _};
        use std::time::Duration as StdDuration;

        let engine = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        engine.set_nonblocking(true).unwrap();
        let engine_addr = engine.local_addr().unwrap();
        let hs = cr_guard::handshake_bytes(&[0x33; 20], &[0x44; 20]);

        let peer_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let peer_addr = peer_listener.local_addr().unwrap();
        let dp = DataPlane::new().unwrap();
        let advertised: SocketAddr = "112.5.60.4:19910".parse().unwrap();
        let join = std::thread::spawn(move || {
            let (stream, _) = peer_listener.accept().unwrap();
            dp.attach_punched_inbound_waking(advertised, stream, engine_addr, |_| false).unwrap();
            dp
        });

        let mut client = std::net::TcpStream::connect(peer_addr).unwrap();
        client.write_all(&hs).unwrap();
        client.set_read_timeout(Some(StdDuration::from_secs(3))).unwrap();
        let mut buf = [0u8; 8];
        // 被拒绝时本端只会看到连接结束，不会收到任何伪造的成功回应。
        match client.read(&mut buf) {
            Ok(0) => {}
            Ok(n) => panic!("拒绝路径不应回任何字节，实际收到 {n} 字节"),
            Err(_) => {}
        }
        std::thread::sleep(StdDuration::from_millis(300));
        assert!(engine.accept().is_err(), "拒绝路径不能把连接喂进引擎");
        let dp = join.join().unwrap();
        let _ = dp;
    }

    /// 负向对照：engine.listen 指错时不得凭空回出 world。
    /// 防止未来有人把 splice 退化成「在打洞 socket 上自己 echo 一个成功」。
    #[test]
    fn punched_inbound_wrong_engine_listen_does_not_fabricate_success() {
        use std::io::{Read as _, Write as _};
        use std::time::Duration as StdDuration;

        let wrong = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let wrong_addr = wrong.local_addr().unwrap();
        drop(wrong); // 故意让它不可连接

        let peer_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let peer_addr = peer_listener.local_addr().unwrap();
        let dp = DataPlane::new().unwrap();
        let advertised: SocketAddr = "112.5.60.4:19910".parse().unwrap();
        let join = std::thread::spawn(move || {
            let (stream, _) = peer_listener.accept().unwrap();
            // 注册本身是异步 handoff，所以这里返回 Ok；存活标志会随后归零。
            dp.attach_punched_inbound(advertised, stream, wrong_addr).unwrap();
            dp
        });
        let mut client = std::net::TcpStream::connect(peer_addr).unwrap();
        client.write_all(b"hello").unwrap();
        client.set_read_timeout(Some(StdDuration::from_secs(3))).unwrap();
        let mut buf = [0u8; 5];
        let got = client.read(&mut buf).unwrap_or(0);
        assert_ne!(&buf[..got], b"world", "错误的 engine.listen 不得伪造成功字节");
        let dp = join.join().unwrap();
        // 泵失败后 alive 必须掉下来，不能长期显示「打洞活着」。
        let deadline = std::time::Instant::now() + StdDuration::from_secs(2);
        while std::time::Instant::now() < deadline {
            if dp.json().get("punched_alive").and_then(cr_core::Json::as_i64) == Some(0) {
                return;
            }
            std::thread::sleep(StdDuration::from_millis(20));
        }
        panic!("错误 engine.listen 的 splice 泵应在 2 s 内标记死亡");
    }
}
