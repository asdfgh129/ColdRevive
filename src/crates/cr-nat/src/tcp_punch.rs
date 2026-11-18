//! M2/W3.9-⑤ — TCP 同时打开（simultaneous open）打洞。
//!
//! 为什么 UDP 打洞不够：真实 BT 客户端之间大量流量走 TCP。如果两侧都是受限锥/
//! 对称 NAT，TCP 入向被过滤，只有「双方同时向对方的公网映射端口发 SYN」才能各自
//! 在网关上开出过滤项，让对方的 SYN 进来完成三次握手。这就是 RFC 793 的同时打开。
//!
//! 实现要点（每一条都对应一个真实失败形态）：
//! - **必须绑定本地端口**：不绑的话内核挑临时端口，两侧都不知道对方这次用的外部端口，
//!   打洞无从谈起。std 的 `TcpStream` 不能 bind-before-connect，且本工具链的 std
//!   没有 `TcpSocket`（实测 `std::net::TcpSocket` 不存在），所以这里用极小的
//!   socket FFI 走 `socket/bind/connect`，再交回 `TcpStream` 托管生命周期。
//! - **必须开 SO_REUSEADDR**：每次重试都要重新 bind 同一端口；上一轮残留的
//!   TIME_WAIT / FIN_WAIT 会让 bind 报 EADDRINUSE，看起来像「端口被占」而其实是自己。
//! - **非阻塞 connect + 重试**：单次 SYN 很容易与对端时序错开（对端那一刻还没发 SYN，
//!   内核回 RST）。因此按固定间隔重试，直到超时。每次都是真实 SYN，
//!   成功与否由内核握手结果决定，代码不合成任何「已连接」。
//! - **完成判据是 `poll(POLLOUT)` + `SO_ERROR`**，不是 `peer_addr()`：
//!   Windows 上 `getpeername` 在 SYN_SENT 状态就可能返回目标地址（2026-11-18 实测：
//!   对没有监听的端口也报「成功」），拿它当判据会稳定产出假阳性。poll 可写 + SO_ERROR=0
//!   才是「三次握手真的完成」的系统口径。
//!
//! 边界（写清楚，避免被读成「BT 已经能走 TCP 打洞」）：
//! 本模块只提供**可验证的传输原语**（打出一个真 TCP 洞并能在上面收发字节）。
//! 把它接进 BT 数据面还需要引擎接受「外部已建连的 socket」，而 librqbit 9.1.0
//! 不提供这个入口（ADR-015）。因此 BT-over-punched-TCP 仍是缺口，
//! ROADMAP 里按「原语完成、接线未完成」记录，不写成已完成。

use std::io;
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

/// 单轮尝试的等待窗口：窗口内没建连就换一轮（重新 bind + connect）。
pub const RETRY_INTERVAL: Duration = Duration::from_millis(150);

/// 一次成功的打洞。
#[derive(Debug)]
pub struct Punched {
    pub stream: TcpStream,
    pub local: SocketAddr,
    pub peer: SocketAddr,
    /// 一共发起了几轮尝试（每轮对每个候选各一个 SYN）。
    pub attempts: u32,
    pub elapsed: Duration,
}

/// 打洞结果。`established=None` 时 `errors` 里是真实的系统错误原文（可能为空，
/// 表示「一直没建连也没报错」，那是被静默丢弃的典型形态）。
#[derive(Debug, Default)]
pub struct PunchReport {
    pub established: Option<Punched>,
    pub attempts: u32,
    pub errors: Vec<String>,
}

/// 用固定本地端口向每个候选发起同时打开，拿到第一条建立成功的连接。
pub fn punch_tcp(local: SocketAddr, peers: &[SocketAddr], timeout: Duration) -> PunchReport {
    let mut report = PunchReport::default();
    if peers.is_empty() {
        report.errors.push("没有候选地址".to_string());
        return report;
    }
    let started = Instant::now();
    let deadline = started + timeout;
    while Instant::now() < deadline {
        report.attempts += 1;
        let slice_end = (Instant::now() + RETRY_INTERVAL).min(deadline);
        let mut pending: Vec<(SocketAddr, TcpStream)> = Vec::new();
        for peer in peers {
            match open_pending(local, *peer) {
                Ok(s) => pending.push((*peer, s)),
                Err(e) => {
                    let msg = format!("向 {peer} 发起连接失败: {e}");
                    if !report.errors.contains(&msg) {
                        report.errors.push(msg);
                    }
                }
            }
        }
        // 轮询本轮的 socket：只有内核真的完成握手才算成功。
        while Instant::now() < slice_end && !pending.is_empty() {
            let mut done = None;
            let mut failed: Vec<usize> = Vec::new();
            for (i, (peer, stream)) in pending.iter().enumerate() {
                match sys::completion(stream, 0) {
                    sys::Completion::Done => {
                        done = Some(i);
                        break;
                    }
                    sys::Completion::Failed(e) => {
                        let msg = format!("向 {peer} 的连接被拒: {e}");
                        if !report.errors.contains(&msg) {
                            report.errors.push(msg);
                        }
                        failed.push(i);
                    }
                    sys::Completion::Pending => {}
                }
            }
            if let Some(i) = done {
                let (peer, stream) = pending.remove(i);
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_nodelay(true);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
                let bound = stream.local_addr().unwrap_or(local);
                report.established = Some(Punched {
                    stream,
                    local: bound,
                    peer,
                    attempts: report.attempts,
                    elapsed: started.elapsed(),
                });
                return report;
            }
            // 逆序删除，避免下标漂移。
            for i in failed.into_iter().rev() {
                pending.remove(i);
            }
            if pending.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(3));
        }
        // 本轮没成：socket 在这里 drop，端口随之释放，下一轮重新 bind。
        // 若本轮连 socket 都没建起来（例如平台初始化失败），必须退让一下，
        // 否则会在超时前空转几百万轮——那不是「重试」，是把 CPU 打满。
        if pending.is_empty() {
            std::thread::sleep(Duration::from_millis(60));
        }
    }
    report
}

/// 绑定 `local` 并向 `peer` 发起非阻塞 connect，返回处于「连接中」的流。
fn open_pending(local: SocketAddr, peer: SocketAddr) -> io::Result<TcpStream> {
    let (local_ip, peer_ip) = match (local.ip(), peer.ip()) {
        (std::net::IpAddr::V4(a), std::net::IpAddr::V4(b)) => (a, b),
        _ => return Err(io::Error::new(io::ErrorKind::InvalidInput,
                                       "TCP 打洞目前只支持 IPv4（与 STUN 侧一致）")),
    };
    let raw = sys::new_tcp_socket()?;
    if let Err(e) = sys::set_reuseaddr(raw) {
        sys::close(raw);
        return Err(e);
    }
    if let Err(e) = sys::bind_v4(raw, local_ip, local.port()) {
        sys::close(raw);
        return Err(e);
    }
    if let Err(e) = sys::set_nonblocking(raw) {
        sys::close(raw);
        return Err(e);
    }
    match sys::connect_v4(raw, peer_ip, peer.port()) {
        sys::Connect::Pending => {}
        sys::Connect::Done => {}
        sys::Connect::Failed(e) => {
            sys::close(raw);
            return Err(e);
        }
    }
    // 句柄所有权交给 std（`into_stream` 之后由 `TcpStream` 负责关闭）。
    Ok(sys::into_stream(raw))
}

// ---------------------------------------------------------------------------
// 平台 socket 层。
//
// 为什么不用 std 的 `TcpSocket`：本工具链的 std 里没有它（`std::net::TcpSocket`
// 编译不过，2026-11-18 实测）。用 FFI 是唯一能在 bind 之后 connect 的路子。
// 句柄所有权在 `into_stream` 处一次性转交给 `TcpStream`，之后不再手动 close，
// 避免双重释放；`close` 只在放弃连接时用。
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod sys {
    use std::io;
    use std::net::{Ipv4Addr, TcpStream};
    use std::os::windows::io::{AsRawSocket, FromRawSocket};

    const AF_INET: i32 = 2;
    const SOCK_STREAM: i32 = 1;
    const IPPROTO_TCP: i32 = 6;
    const SOL_SOCKET: i32 = 0xFFFF;
    const SO_REUSEADDR: i32 = 4;
    const SO_ERROR: i32 = 0x1007;
    const FIONBIO: u32 = 0x8004_667E;
    const WSAEWOULDBLOCK: i32 = 10035;
    const WSAEINPROGRESS: i32 = 10036;
    const WSAEALREADY: i32 = 10037;
    const POLLOUT: i16 = 0x0010;

    #[repr(C)]
    struct PollFd {
        fd: usize,
        events: i16,
        revents: i16,
    }

    #[link(name = "ws2_32")]
    extern "system" {
        fn WSAStartup(version: u16, data: *mut u8) -> i32;
        fn socket(af: i32, ty: i32, proto: i32) -> usize;
        fn setsockopt(s: usize, level: i32, opt: i32, val: *const u8, len: i32) -> i32;
        fn getsockopt(s: usize, level: i32, opt: i32, val: *mut u8, len: *mut i32) -> i32;
        fn bind(s: usize, name: *const u8, len: i32) -> i32;
        fn connect(s: usize, name: *const u8, len: i32) -> i32;
        fn ioctlsocket(s: usize, cmd: u32, arg: *mut u32) -> i32;
        fn closesocket(s: usize) -> i32;
        fn WSAPoll(fds: *mut PollFd, nfds: u32, timeout: i32) -> i32;
        fn WSAGetLastError() -> i32;
    }

    pub enum Connect {
        Pending,
        Done,
        Failed(io::Error),
    }

    pub enum Completion {
        Pending,
        Done,
        Failed(io::Error),
    }

    fn last_error() -> io::Error {
        io::Error::from_raw_os_error(unsafe { WSAGetLastError() })
    }

    /// 必须显式初始化 Winsock。
    ///
    /// std 自己的 `TcpStream`/`TcpListener` 会惰性调用 `WSAStartup`；但本模块**先于**
    /// 任何 std 网络调用执行，若此处不初始化，`socket()` 直接返回
    /// WSAStartup-not-called(10093)。曾经在单测里看不到（测试前面创建过监听 socket
    /// 已经顺带初始化），只在独立二进制里暴露——典型的「测试环境掩盖的缺陷」。
    fn ensure_winsock() {
        use std::sync::Once;
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            let mut data = [0u8; 512]; // WSADATA 实际约 408 字节，留余量
            let rc = unsafe { WSAStartup(0x0202, data.as_mut_ptr()) };
            if rc != 0 {
                eprintln!("[tcp-punch] WSAStartup 失败: {rc}");
            }
        });
    }

    pub fn new_tcp_socket() -> io::Result<usize> {
        ensure_winsock();
        let s = unsafe { socket(AF_INET, SOCK_STREAM, IPPROTO_TCP) };
        if s == usize::MAX {
            return Err(last_error());
        }
        Ok(s)
    }

    pub fn set_reuseaddr(s: usize) -> io::Result<()> {
        let one: i32 = 1;
        let rc = unsafe {
            setsockopt(s, SOL_SOCKET, SO_REUSEADDR, &one as *const i32 as *const u8, 4)
        };
        if rc != 0 {
            return Err(last_error());
        }
        Ok(())
    }

    pub fn bind_v4(s: usize, ip: Ipv4Addr, port: u16) -> io::Result<()> {
        let sa = super::sockaddr_in(ip, port);
        if unsafe { bind(s, sa.as_ptr(), sa.len() as i32) } != 0 {
            return Err(last_error());
        }
        Ok(())
    }

    pub fn set_nonblocking(s: usize) -> io::Result<()> {
        let mut on: u32 = 1;
        if unsafe { ioctlsocket(s, FIONBIO, &mut on as *mut u32) } != 0 {
            return Err(last_error());
        }
        Ok(())
    }

    pub fn connect_v4(s: usize, ip: Ipv4Addr, port: u16) -> Connect {
        let sa = super::sockaddr_in(ip, port);
        if unsafe { connect(s, sa.as_ptr(), sa.len() as i32) } == 0 {
            return Connect::Done;
        }
        let err = unsafe { WSAGetLastError() };
        if err == WSAEWOULDBLOCK || err == WSAEINPROGRESS || err == WSAEALREADY {
            Connect::Pending
        } else {
            Connect::Failed(io::Error::from_raw_os_error(err))
        }
    }

    pub fn close(s: usize) {
        unsafe {
            closesocket(s);
        }
    }

    /// 句柄所有权转交给 std：之后由 `TcpStream` 负责关闭。
    pub fn into_stream(s: usize) -> TcpStream {
        unsafe { TcpStream::from_raw_socket(s as u64) }
    }

    /// 非阻塞地判断「三次握手是否完成」。
    ///
    /// 判据必须是 `WSAPoll(POLLOUT)` + `SO_ERROR`：Windows 上 `getpeername` 在
    /// SYN_SENT 阶段就会返回目标地址，用它判断会把「连接被拒」读成「已连接」。
    pub fn completion(stream: &TcpStream, timeout_ms: i32) -> Completion {
        let raw = stream.as_raw_socket() as usize;
        let mut fd = PollFd { fd: raw, events: POLLOUT, revents: 0 };
        let n = unsafe { WSAPoll(&mut fd as *mut PollFd, 1, timeout_ms) };
        if n == 0 {
            return Completion::Pending;
        }
        if n < 0 {
            return Completion::Failed(last_error());
        }
        let mut err: i32 = 0;
        let mut len: i32 = 4;
        let rc = unsafe {
            getsockopt(raw, SOL_SOCKET, SO_ERROR, &mut err as *mut i32 as *mut u8, &mut len)
        };
        if rc != 0 {
            return Completion::Failed(last_error());
        }
        if err != 0 {
            return Completion::Failed(io::Error::from_raw_os_error(err));
        }
        Completion::Done
    }
}

#[cfg(unix)]
mod sys {
    use std::io;
    use std::net::{Ipv4Addr, TcpStream};
    use std::os::unix::io::{AsRawFd, FromRawFd};

    const AF_INET: i32 = 2;
    const SOCK_STREAM: i32 = 1;
    const IPPROTO_TCP: i32 = 6;
    const SOL_SOCKET: i32 = 1;
    const SO_REUSEADDR: i32 = 2;
    const SO_ERROR: i32 = 4;
    const F_GETFL: i32 = 3;
    const F_SETFL: i32 = 4;
    const EINPROGRESS: i32 = 115;

    #[repr(C)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }

    extern "C" {
        fn socket(domain: i32, ty: i32, proto: i32) -> i32;
        fn setsockopt(fd: i32, level: i32, name: i32, val: *const u8, len: u32) -> i32;
        fn getsockopt(fd: i32, level: i32, name: i32, val: *mut u8, len: *mut u32) -> i32;
        fn bind(fd: i32, addr: *const u8, len: u32) -> i32;
        fn connect(fd: i32, addr: *const u8, len: u32) -> i32;
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        fn poll(fds: *mut PollFd, nfds: u64, timeout: i32) -> i32;
        // 不能直接叫 `close`：本模块要导出同名的 `pub fn close`，同作用域下
        // 两个同名项会直接编译失败（Windows 分支看不到，只有 Linux 才暴露）。
        #[link_name = "close"]
        fn libc_close(fd: i32) -> i32;
        fn __errno_location() -> *mut i32;
    }

    pub enum Connect {
        Pending,
        Done,
        Failed(io::Error),
    }

    pub enum Completion {
        Pending,
        Done,
        Failed(io::Error),
    }

    fn errno() -> i32 {
        unsafe { *__errno_location() }
    }

    fn last_error() -> io::Error {
        io::Error::from_raw_os_error(errno())
    }

    pub fn new_tcp_socket() -> io::Result<i32> {
        let fd = unsafe { socket(AF_INET, SOCK_STREAM, IPPROTO_TCP) };
        if fd < 0 {
            return Err(last_error());
        }
        Ok(fd)
    }

    pub fn set_reuseaddr(fd: i32) -> io::Result<()> {
        let one: i32 = 1;
        if unsafe { setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &one as *const i32 as *const u8, 4) } != 0 {
            return Err(last_error());
        }
        Ok(())
    }

    pub fn bind_v4(fd: i32, ip: Ipv4Addr, port: u16) -> io::Result<()> {
        let sa = super::sockaddr_in(ip, port);
        if unsafe { bind(fd, sa.as_ptr(), sa.len() as u32) } != 0 {
            return Err(last_error());
        }
        Ok(())
    }

    pub fn set_nonblocking(fd: i32) -> io::Result<()> {
        let flags = unsafe { fcntl(fd, F_GETFL) };
        if flags < 0 {
            return Err(last_error());
        }
        // O_NONBLOCK 在不同 Unix 上值不同：Linux 0o4000，macOS/BSD 0x0004。
        let nonblock = if cfg!(target_os = "linux") { 0o4000 } else { 0x0004 };
        if unsafe { fcntl(fd, F_SETFL, flags | nonblock) } < 0 {
            return Err(last_error());
        }
        Ok(())
    }

    pub fn connect_v4(fd: i32, ip: Ipv4Addr, port: u16) -> Connect {
        let sa = super::sockaddr_in(ip, port);
        if unsafe { connect(fd, sa.as_ptr(), sa.len() as u32) } == 0 {
            return Connect::Done;
        }
        let e = errno();
        if e == EINPROGRESS {
            Connect::Pending
        } else {
            Connect::Failed(io::Error::from_raw_os_error(e))
        }
    }

    pub fn close(fd: i32) {
        unsafe {
            libc_close(fd);
        }
    }

    pub fn into_stream(fd: i32) -> TcpStream {
        unsafe { TcpStream::from_raw_fd(fd) }
    }

    pub fn completion(stream: &TcpStream, timeout_ms: i32) -> Completion {
        let mut fd = PollFd { fd: stream.as_raw_fd(), events: 4 /* POLLOUT */, revents: 0 };
        let n = unsafe { poll(&mut fd as *mut PollFd, 1, timeout_ms) };
        if n == 0 {
            return Completion::Pending;
        }
        if n < 0 {
            return Completion::Failed(last_error());
        }
        let mut err: i32 = 0;
        let mut len: u32 = 4;
        if unsafe { getsockopt(fd.fd, SOL_SOCKET, SO_ERROR, &mut err as *mut i32 as *mut u8, &mut len) } != 0 {
            return Completion::Failed(last_error());
        }
        if err != 0 {
            return Completion::Failed(io::Error::from_raw_os_error(err));
        }
        Completion::Done
    }
}

#[cfg(not(any(windows, unix)))]
mod sys {
    use std::io;
    use std::net::{Ipv4Addr, TcpStream};

    pub enum Connect {
        Pending,
        Done,
        Failed(io::Error),
    }
    pub enum Completion {
        Pending,
        Done,
        Failed(io::Error),
    }

    fn unsupported() -> io::Error {
        io::Error::new(io::ErrorKind::Unsupported, "TCP 打洞未在该平台实现")
    }

    pub fn new_tcp_socket() -> io::Result<u64> {
        Err(unsupported())
    }
    pub fn set_reuseaddr(_: u64) -> io::Result<()> {
        Ok(())
    }
    pub fn bind_v4(_: u64, _: Ipv4Addr, _: u16) -> io::Result<()> {
        Ok(())
    }
    pub fn set_nonblocking(_: u64) -> io::Result<()> {
        Ok(())
    }
    pub fn connect_v4(_: u64, _: Ipv4Addr, _: u16) -> Connect {
        Connect::Pending
    }
    pub fn close(_: u64) {}
    pub fn into_stream(_: u64) -> TcpStream {
        panic!("TCP 打洞未在该平台实现")
    }
    pub fn completion(_: &TcpStream, _: i32) -> Completion {
        Completion::Pending
    }
}

/// `struct sockaddr_in` 的字节表示（16 字节）。
///
/// `sin_family` 用本机字节序，端口与地址用网络字节序——这是 BSD socket 的约定，
/// 写错一个就变成「连到一个看似合理但错误的地址」，而且内核不一定报错。
#[cfg(any(windows, unix))]
fn sockaddr_in(ip: std::net::Ipv4Addr, port: u16) -> [u8; 16] {
    let mut out = [0u8; 16];
    let family = 2u16.to_ne_bytes();
    out[0] = family[0];
    out[1] = family[1];
    let be = port.to_be_bytes();
    out[2] = be[0];
    out[3] = be[1];
    out[4..8].copy_from_slice(&ip.octets());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn free_port() -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let p = l.local_addr().expect("addr").port();
        drop(l);
        p
    }

    #[test]
    fn sockaddr_layout_is_network_order() {
        let sa = sockaddr_in("112.5.60.4".parse().unwrap(), 0x1AE1);
        assert_eq!(&sa[0..2], &2u16.to_ne_bytes(), "family 用本机字节序");
        assert_eq!(&sa[2..4], &[0x1A, 0xE1], "端口必须大端");
        assert_eq!(&sa[4..8], &[112, 5, 60, 4]);
        assert_eq!(&sa[8..], &[0u8; 8]);
    }

    /// 回环上的真实同时打开。两个线程各自绑定己方端口、并发向对方 SYN。
    ///
    /// 断言分两层：**至少一侧**建立（否则说明原语根本没工作），
    /// 两侧都建立时必须有真实双向字节（否则只是「握手成功但不能用」）。
    #[test]
    fn simultaneous_open_on_loopback_establishes_and_carries_bytes() {
        let pa = free_port();
        let pb = free_port();
        let a_local: SocketAddr = format!("127.0.0.1:{pa}").parse().unwrap();
        let b_local: SocketAddr = format!("127.0.0.1:{pb}").parse().unwrap();
        let a_peer: SocketAddr = format!("127.0.0.1:{pb}").parse().unwrap();
        let b_peer: SocketAddr = format!("127.0.0.1:{pa}").parse().unwrap();

        let ta = std::thread::spawn(move || punch_and_echo(a_local, a_peer, true));
        let tb = std::thread::spawn(move || punch_and_echo(b_local, b_peer, false));
        let (ra, a_bytes_ok) = ta.join().expect("thread a");
        let (rb, b_bytes_ok) = tb.join().expect("thread b");

        eprintln!(
            "[tcp-punch] A established={} attempts={} errors={:?} ; B established={} attempts={} errors={:?}",
            ra.established.is_some(), ra.attempts, ra.errors,
            rb.established.is_some(), rb.attempts, rb.errors
        );
        // 回环上两侧都必须建立：只建立一侧说明原语在某个方向上退化了。
        // 若将来某平台确实只能单向，应该是「改文档 + 改断言并说明原因」，
        // 不是把断言放宽到「至少一侧」——那样一半失败会被静默吞掉。
        let (a, b) = match (ra.established.as_ref(), rb.established.as_ref()) {
            (Some(a), Some(b)) => (a, b),
            _ => panic!("两侧必须都建立：A={} ({:?}) B={} ({:?})",
                        ra.established.is_some(), ra.errors,
                        rb.established.is_some(), rb.errors),
        };
        assert_eq!(a.peer.port(), pb, "A 的对端必须是 B 的端口");
        assert_eq!(b.peer.port(), pa, "B 的对端必须是 A 的端口");
        assert_eq!(a.local.port(), pa, "A 必须从固定本地端口发出");
        assert_eq!(b.local.port(), pb);
        assert!(a_bytes_ok && b_bytes_ok, "打通的连接必须真的能双向传字节");
    }

    /// 打洞成功后在流上做一次请求/回显，证明这条洞真的能承载字节。
    /// 第二个返回值 = 字节往返是否完整成功（False 表示「握手成功但不能传数据」）。
    fn punch_and_echo(local: SocketAddr, peer: SocketAddr, talker: bool) -> (PunchReport, bool) {
        let mut report = punch_tcp(local, &[peer], Duration::from_secs(6));
        let mut bytes_ok = false;
        if let Some(p) = report.established.as_mut() {
            let _ = p.stream.set_read_timeout(Some(Duration::from_secs(3)));
            bytes_ok = if talker {
                if p.stream.write_all(b"ping").is_ok() {
                    let mut buf = [0u8; 4];
                    p.stream.read_exact(&mut buf).is_ok() && &buf == b"pong"
                } else {
                    false
                }
            } else {
                let mut buf = [0u8; 4];
                if p.stream.read_exact(&mut buf).is_ok() && &buf == b"ping" {
                    p.stream.write_all(b"pong").is_ok()
                } else {
                    false
                }
            };
        }
        (report, bytes_ok)
    }

    #[test]
    fn punching_a_closed_port_times_out_without_inventing_success() {
        let local: SocketAddr = format!("127.0.0.1:{}", free_port()).parse().unwrap();
        let dead: SocketAddr = format!("127.0.0.1:{}", free_port()).parse().unwrap();
        let started = Instant::now();
        let r = punch_tcp(local, &[dead], Duration::from_millis(400));
        assert!(r.established.is_none(), "对方没有监听时不得报告成功（errors={:?}）", r.errors);
        assert!(started.elapsed() >= Duration::from_millis(350), "必须真的等满超时");
        assert!(r.attempts >= 2, "应当重试而不是只试一次（实际 {}）", r.attempts);
    }

    #[test]
    fn empty_peer_list_is_reported_not_guessed() {
        let r = punch_tcp("127.0.0.1:1234".parse().unwrap(), &[], Duration::from_millis(50));
        assert!(r.established.is_none());
        assert_eq!(r.errors, vec!["没有候选地址".to_string()]);
        assert_eq!(r.attempts, 0);
    }
}
