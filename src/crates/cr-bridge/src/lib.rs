//! 把「已经打通的 TCP socket / 中继隧道」塞进 BT 引擎，而不改引擎（ADR-021/023）。
//!
//! 引擎（librqbit）只认自己建立的连接：出站由它 `connect`，入站由它 `accept`。
//! 打洞得到的 socket 两头都不属于它，于是数据面进不去。这里用两个**官方钩子**绕开：
//!
//! 1. **出向**：`ConnectionOptions.proxy_url` 指到本 crate 的 SOCKS5 服务。
//!    命中注册表的目标地址就交出发送方的那条 socket，未命中回落真实直连。
//! 2. **入向**：把打通的 socket 连到引擎自己的监听口（`splice_to_listener`），
//!    引擎看到的就是一条普通入站连接。
//!
//! 这一层是**搬运**，不是**打通**：能否建立连接仍由 `cr_nat::tcp_punch` 负责。
//! 已知降级（ADR-021 §后果）：对端地址会显示成 `127.0.0.1`，会被引擎当本地 peer，
//! 不参与 PEX/对外通告，部分统计不计入。**不得宣称与直连完全等价。**

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf, copy_bidirectional};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

/// 落回真实直连时的连接超时。SOCKS5 没有「等待中」状态，超时必须给出明确失败码。
pub const DIAL_TIMEOUT: Duration = Duration::from_secs(10);

// SOCKS5 应答码（RFC 1928 §6）。只实现用得到的几个，其余一律 GENERAL_FAILURE。
const REP_SUCCEEDED: u8 = 0x00;
const REP_GENERAL_FAILURE: u8 = 0x01;
const REP_HOST_UNREACHABLE: u8 = 0x04;
const REP_COMMAND_UNSUPPORTED: u8 = 0x07;
const REP_ATYP_UNSUPPORTED: u8 = 0x08;

pub mod cross_protocol;

/// SOCKS5 上游：可以是打洞得到的真 socket（ADR-021），也可以是中继隧道（ADR-023）。
/// 两种都只需 `AsyncRead + AsyncWrite`，`copy_bidirectional` 不关心底层是谁。
pub enum Upstream {
    Direct(TcpStream),
    Tunnel(DuplexStream),
}

impl AsyncRead for Upstream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Upstream::Direct(s) => Pin::new(s).poll_read(cx, buf),
            Upstream::Tunnel(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Upstream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Upstream::Direct(s) => Pin::new(s).poll_write(cx, buf),
            Upstream::Tunnel(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Upstream::Direct(s) => Pin::new(s).poll_flush(cx),
            Upstream::Tunnel(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Upstream::Direct(s) => Pin::new(s).poll_shutdown(cx),
            Upstream::Tunnel(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

/// 注册表：目标地址 → 一条已建立的上游连接（直连 socket 或中继隧道）。
///
/// 键是**引擎将要拨打的地址**（对端通告的 IP:端口），值是本进程持有的那条连接。
/// 一项只能被取走一次：BT 的一条 TCP 连接只服务一个 peer 会话。
pub struct Bridge {
    registry: Mutex<HashMap<SocketAddr, Upstream>>,
    /// 命中注册表、由「已打通 socket」承载的 CONNECT 次数。
    /// 这是 e2e 判定「真的走了桥」的证据；回落直连不计入。
    served_registered: AtomicUsize,
    /// 经本桥搬运的总字节数（双向合计）。
    bytes_relayed: AtomicU64,
}

impl Default for Bridge {
    fn default() -> Self {
        Self::new()
    }
}

impl Bridge {
    pub fn new() -> Self {
        Self {
            registry: Mutex::new(HashMap::new()),
            served_registered: AtomicUsize::new(0),
            bytes_relayed: AtomicU64::new(0),
        }
    }

    /// 登记一条已打通的连接。同一目标地址重复登记返回错误而不是静默覆盖——
    /// 覆盖会丢掉前一条连接，现场表现成「第一次能传、第二次卡死」。
    pub fn register(&self, peer: SocketAddr, stream: TcpStream) -> Result<(), &'static str> {
        self.register_upstream(peer, Upstream::Direct(stream))
    }

    /// 登记一条中继隧道（ADR-023）。与 `register` 共用同一张表和一次性语义。
    pub fn register_tunnel(&self, peer: SocketAddr, stream: DuplexStream) -> Result<(), &'static str> {
        self.register_upstream(peer, Upstream::Tunnel(stream))
    }

    fn register_upstream(&self, peer: SocketAddr, upstream: Upstream) -> Result<(), &'static str> {
        let mut m = self.registry.lock().unwrap();
        if m.contains_key(&peer) {
            return Err("该目标地址已登记，拒绝覆盖");
        }
        m.insert(peer, upstream);
        Ok(())
    }

    /// 取走登记项（一次性）。
    pub fn take(&self, peer: SocketAddr) -> Option<Upstream> {
        self.registry.lock().unwrap().remove(&peer)
    }

    /// 当前登记数。只用于观测，不参与判定。
    pub fn registered(&self) -> usize {
        self.registry.lock().unwrap().len()
    }

    /// 命中注册表并由已打通 socket 承载的 CONNECT 次数。
    pub fn served_registered(&self) -> usize {
        self.served_registered.load(Ordering::Relaxed)
    }

    /// 双向搬运的总字节数。
    /// 本进程经 SOCKS5 桥**写入本地隧道**的字节数。
    /// 口径：不是对端实际收到的字节；对端落地只能由 e2e 读对端文件 md5 证明。
    /// `/health.byte_accounting.tunnel_written_bytes` 引用此值并显式标注口径。
    pub fn bytes_relayed(&self) -> u64 {
        self.bytes_relayed.load(Ordering::Relaxed)
    }

    /// 在指定地址起 SOCKS5 服务并阻塞到监听失败。
    pub async fn serve(self: Arc<Self>, listen: SocketAddr) -> io::Result<()> {
        let listener = TcpListener::bind(listen).await?;
        self.serve_listener(listener).await
    }

    /// 同上，但接受调用方已绑定的 listener（测试用 `:0` 取临时端口）。
    pub async fn serve_listener(self: Arc<Self>, listener: TcpListener) -> io::Result<()> {
        loop {
            let (sock, _peer) = listener.accept().await?;
            let bridge = self.clone();
            tokio::spawn(async move {
                // 单条连接的错误不能带走整个服务：BT 会不断重连，一次失败很常见。
                let _ = handle_conn(sock, bridge).await;
            });
        }
    }
}

/// 入向：把一条已建立的连接接到引擎自己的监听口，之后双向搬运。
///
/// 用在「本端是对端要连的那一侧」：打洞成功后引擎并不知道这条 socket 存在，
/// 于是在回环上再连它一次，引擎就当普通入站连接处理。
pub async fn splice_to_listener(stream: TcpStream, engine_listen: SocketAddr) -> io::Result<()> {
    let mut downstream = TcpStream::connect(engine_listen).await?;
    let mut upstream = stream;
    copy_bidirectional(&mut upstream, &mut downstream).await?;
    Ok(())
}

async fn handle_conn(mut client: TcpStream, bridge: Arc<Bridge>) -> io::Result<()> {
    // 握手：VER=5, NMETHODS, METHODS。只支持「无认证」，且必须回显我们选中的方法。
    let mut head = [0u8; 2];
    if client.read_exact(&mut head).await.is_err() {
        return Ok(());
    }
    let (ver, nmethods) = (head[0], head[1]);
    if ver != 5 || nmethods == 0 {
        return Ok(());
    }
    let mut methods = vec![0u8; nmethods as usize];
    if client.read_exact(&mut methods).await.is_err() {
        return Ok(());
    }
    if !methods.contains(&0x00) {
        // 没有「无认证」可选：按 RFC 回 0xFF 再关，别让客户端等超时。
        client.write_all(&[0x05, 0xFF]).await?;
        return Ok(());
    }
    client.write_all(&[0x05, 0x00]).await?;

    let target = match read_request(&mut client).await? {
        Ok(t) => t,
        Err(code) => {
            reply(&mut client, code).await?;
            return Ok(());
        }
    };

    let registered = bridge.take(target);
    if registered.is_some() {
        bridge.served_registered.fetch_add(1, Ordering::Relaxed);
    }
    let mut upstream = match registered {
        Some(s) => s,
        None => match timeout(DIAL_TIMEOUT, TcpStream::connect(target)).await {
            Ok(Ok(s)) => Upstream::Direct(s),
            Ok(Err(_)) => {
                reply(&mut client, REP_HOST_UNREACHABLE).await?;
                return Ok(());
            }
            Err(_) => {
                reply(&mut client, REP_HOST_UNREACHABLE).await?;
                return Ok(());
            }
        },
    };

    reply(&mut client, REP_SUCCEEDED).await?;
    let (up, down) = copy_bidirectional(&mut client, &mut upstream).await.unwrap_or((0, 0));
    bridge.bytes_relayed.fetch_add(up + down, Ordering::Relaxed);
    Ok(())
}

/// 读 SOCKS5 请求。返回值是 `Result<目标地址, 应答码>`：
/// 协议错误不以 `io::Error` 抛出，因为「拒绝一个请求」是正常业务，不是 I/O 失败。
async fn read_request(client: &mut TcpStream) -> io::Result<Result<SocketAddr, u8>> {
    let mut head = [0u8; 4];
    client.read_exact(&mut head).await?;
    let (ver, cmd, _rsv, atyp) = (head[0], head[1], head[2], head[3]);
    if ver != 5 {
        return Ok(Err(REP_GENERAL_FAILURE));
    }
    // 地址段**必须先读完再判命令**：命令不支持时如果留半条请求在接收缓冲里就关连接，
    // Windows 会回 RST，客户端看到 `ConnectionReset` 而不是我们刚发的拒绝码——
    // 「明确拒绝」会被误读成「网络断」，正是本项目反复踩的那类伪装。
    let ip: IpAddr = match atyp {
        0x01 => {
            let mut b = [0u8; 4];
            client.read_exact(&mut b).await?;
            IpAddr::V4(Ipv4Addr::from(b))
        }
        0x03 => {
            let mut len = [0u8; 1];
            client.read_exact(&mut len).await?;
            let mut host = vec![0u8; len[0] as usize];
            client.read_exact(&mut host).await?;
            // 只接字面 IP：BT peer 地址本来就是 IP，解析域名会把「协议问题」
            // 伪装成「DNS 慢」，而且这里没有值得信任的解析器。
            match std::str::from_utf8(&host).ok().and_then(|s| s.parse::<IpAddr>().ok()) {
                Some(ip) => ip,
                None => return Ok(Err(REP_ATYP_UNSUPPORTED)),
            }
        }
        0x04 => {
            let mut b = [0u8; 16];
            client.read_exact(&mut b).await?;
            IpAddr::from(b)
        }
        _ => return Ok(Err(REP_ATYP_UNSUPPORTED)),
    };
    let mut port = [0u8; 2];
    client.read_exact(&mut port).await?;
    if cmd != 0x01 {
        // BIND(0x02)/UDP ASSOCIATE(0x03) 不支持：BT 数据面只需要 CONNECT。
        return Ok(Err(REP_COMMAND_UNSUPPORTED));
    }
    Ok(Ok(SocketAddr::new(ip, u16::from_be_bytes(port))))
}

async fn reply(client: &mut TcpStream, code: u8) -> io::Result<()> {
    // BND.ADDR 填 0.0.0.0:0：我们不承诺任何绑定地址，填假值会让人以为可以反连。
    let msg = [0x05u8, code, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
    client.write_all(&msg).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 走完 SOCKS5 握手 + CONNECT，返回应答码。用超时兜住「卡住不返回」的实现错误。
    async fn socks5_connect(client: &mut TcpStream, target: SocketAddr) -> u8 {
        client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        let mut sel = [0u8; 2];
        timeout(Duration::from_secs(5), client.read_exact(&mut sel)).await
            .expect("握手超时").unwrap();
        assert_eq!(sel, [0x05, 0x00], "服务端必须选中无认证方法");

        let mut req = vec![0x05, 0x01, 0x00];
        match target.ip() {
            IpAddr::V4(v4) => { req.push(0x01); req.extend_from_slice(&v4.octets()); }
            IpAddr::V6(v6) => { req.push(0x04); req.extend_from_slice(&v6.octets()); }
        }
        req.extend_from_slice(&target.port().to_be_bytes());
        client.write_all(&req).await.unwrap();

        let mut resp = [0u8; 10];
        timeout(Duration::from_secs(5), client.read_exact(&mut resp)).await
            .expect("CONNECT 应答超时").unwrap();
        resp[1]
    }

    async fn read_n<S: AsyncRead + Unpin>(stream: &mut S, n: usize) -> Vec<u8> {
        let mut buf = vec![0u8; n];
        timeout(Duration::from_secs(5), stream.read_exact(&mut buf)).await
            .expect("读超时").unwrap();
        buf
    }

    #[tokio::test]
    async fn registered_socket_carries_bytes_instead_of_dialling() {
        // 「引擎侧」用一个本地 listener 冒充：打通的 socket 最终要接到它上面。
        let engine_l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let engine_addr = engine_l.local_addr().unwrap();
        let punched = TcpStream::connect(engine_addr).await.unwrap();
        let (mut engine_side, _) = engine_l.accept().await.unwrap();

        let bridge = Arc::new(Bridge::new());
        // 用一个**不可达**的公网地址当键：如果实现真的去 dial，测试会超时而不是误判成功。
        let target: SocketAddr = "203.0.113.9:6881".parse().unwrap();
        bridge.register(target, punched).unwrap();

        let bl = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let baddr = bl.local_addr().unwrap();
        let b = bridge.clone();
        tokio::spawn(async move { let _ = b.serve_listener(bl).await; });

        let mut client = TcpStream::connect(baddr).await.unwrap();
        assert_eq!(socks5_connect(&mut client, target).await, REP_SUCCEEDED);

        client.write_all(b"ping").await.unwrap();
        assert_eq!(read_n(&mut engine_side, 4).await, b"ping");
        engine_side.write_all(b"pong").await.unwrap();
        assert_eq!(read_n(&mut client, 4).await, b"pong");

        assert_eq!(bridge.registered(), 0, "登记项必须被取走，不能留在表里");
        assert_eq!(bridge.served_registered(), 1, "命中注册表必须可观测");
    }

    #[tokio::test]
    async fn unregistered_target_falls_back_to_direct_dial() {
        // 回显服务：证明「没登记」时仍按普通 SOCKS5 代理工作，不影响常规 BT。
        let echo_l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let echo_addr = echo_l.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut s, _)) = echo_l.accept().await {
                let mut buf = [0u8; 4];
                if s.read_exact(&mut buf).await.is_ok() {
                    let _ = s.write_all(&buf).await;
                }
            }
        });

        let bridge = Arc::new(Bridge::new());
        let bl = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let baddr = bl.local_addr().unwrap();
        let b = bridge.clone();
        tokio::spawn(async move { let _ = b.serve_listener(bl).await; });

        let mut client = TcpStream::connect(baddr).await.unwrap();
        assert_eq!(socks5_connect(&mut client, echo_addr).await, REP_SUCCEEDED);
        client.write_all(b"abcd").await.unwrap();
        assert_eq!(read_n(&mut client, 4).await, b"abcd");
        assert_eq!(bridge.served_registered(), 0, "未登记目标回落直连不得计入桥命中");
    }

    #[tokio::test]
    async fn non_connect_command_is_refused_with_code_7() {
        let bridge = Arc::new(Bridge::new());
        let bl = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let baddr = bl.local_addr().unwrap();
        let b = bridge.clone();
        tokio::spawn(async move { let _ = b.serve_listener(bl).await; });

        let mut client = TcpStream::connect(baddr).await.unwrap();
        client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        let mut sel = [0u8; 2];
        client.read_exact(&mut sel).await.unwrap();
        // BIND(0x02) 必须被明确拒绝，而不是被当成 CONNECT 执行。
        client.write_all(&[0x05, 0x02, 0x00, 0x01, 127, 0, 0, 1, 0x1A, 0x0B]).await.unwrap();
        let mut resp = [0u8; 10];
        client.read_exact(&mut resp).await.unwrap();
        assert_eq!(resp[1], REP_COMMAND_UNSUPPORTED);
    }

    #[tokio::test]
    async fn unknown_address_type_is_refused_with_code_8() {
        let bridge = Arc::new(Bridge::new());
        let bl = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let baddr = bl.local_addr().unwrap();
        let b = bridge.clone();
        tokio::spawn(async move { let _ = b.serve_listener(bl).await; });

        let mut client = TcpStream::connect(baddr).await.unwrap();
        client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        let mut sel = [0u8; 2];
        client.read_exact(&mut sel).await.unwrap();
        client.write_all(&[0x05, 0x01, 0x00, 0x05]).await.unwrap();
        let mut resp = [0u8; 10];
        client.read_exact(&mut resp).await.unwrap();
        assert_eq!(resp[1], REP_ATYP_UNSUPPORTED);
    }

    #[tokio::test]
    async fn registering_same_peer_twice_is_refused() {
        let a = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = a.local_addr().unwrap();
        let s1 = TcpStream::connect(addr).await.unwrap();
        let s2 = TcpStream::connect(addr).await.unwrap();
        let bridge = Bridge::new();
        let key: SocketAddr = "198.51.100.7:7000".parse().unwrap();
        bridge.register(key, s1).unwrap();
        assert!(bridge.register(key, s2).is_err(), "重复登记必须报错，不能静默覆盖");
        assert_eq!(bridge.registered(), 1);
    }

    #[tokio::test]
    async fn splice_to_listener_delivers_both_directions() {
        // 模拟入向：引擎在 listen，打通的连接通过 splice 接进去。
        let engine_l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let engine_addr = engine_l.local_addr().unwrap();
        let punched_peer_l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let punched_addr = punched_peer_l.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, _) = punched_peer_l.accept().await.unwrap();
            let _ = splice_to_listener(stream, engine_addr).await;
        });

        let mut engine_side_future = tokio::spawn(async move {
            let (mut s, _) = engine_l.accept().await.unwrap();
            let got = read_n(&mut s, 4).await;
            s.write_all(b"back").await.unwrap();
            got
        });

        let mut peer = TcpStream::connect(punched_addr).await.unwrap();
        peer.write_all(b"data").await.unwrap();
        assert_eq!(read_n(&mut peer, 4).await, b"back");
        let got = timeout(Duration::from_secs(5), &mut engine_side_future).await
            .expect("引擎侧超时").unwrap();
        assert_eq!(got, b"data");
    }

    #[tokio::test]
    async fn registered_relay_tunnel_carries_bytes_instead_of_dialling() {
        // ADR-023：上游换成 duplex（由 cr-nat::relay_tunnel 驱动）后，
        // SOCKS5 前端与 copy_bidirectional 仍原样工作。
        let (tunnel_side, mut peer_side) = tokio::io::duplex(4096);
        let bridge = Arc::new(Bridge::new());
        let target: SocketAddr = "203.0.113.23:6881".parse().unwrap();
        bridge.register_tunnel(target, tunnel_side).unwrap();

        let bl = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let baddr = bl.local_addr().unwrap();
        let b = bridge.clone();
        tokio::spawn(async move { let _ = b.serve_listener(bl).await; });

        let mut client = TcpStream::connect(baddr).await.unwrap();
        assert_eq!(socks5_connect(&mut client, target).await, REP_SUCCEEDED);
        client.write_all(b"tunnel-ping").await.unwrap();
        assert_eq!(read_n(&mut peer_side, 11).await, b"tunnel-ping");
        peer_side.write_all(b"tunnel-pong").await.unwrap();
        assert_eq!(read_n(&mut client, 11).await, b"tunnel-pong");

        assert_eq!(bridge.registered(), 0);
        assert_eq!(bridge.served_registered(), 1);
    }
}
