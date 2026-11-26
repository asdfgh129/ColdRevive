//! cr-guard — W4.4 / FR-11 握手拦截器（opt-in，默认不启用）。
//!
//! ADR-002 的结论：librqbit **没有**握手回调钩子，所以拦截只能自实现在
//! `cr-engine` 外围。FR-11 的生产形态是「旧客户端直接连入 bt_port → 读到标准
//! BT 握手的 infohash → 本地按 infohash 唤醒对应做种任务 → 把已读字节原样补回
//! 并 splice 给引擎」。
//!
//! 本 crate 只放**可单测的纯逻辑与 socket 工具**，不含守护进程的唤醒调用——
//! 唤醒必须走 `coldrevived::sched::wake_seed`（唯一合法 activate 入口），
//! 否则 LRU 活跃池记账会漂移。
//!
//! std-only：不引入 tokio，与 coldrevived 的线程模型一致。

use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::thread;
use std::time::Duration;

/// 标准 BitTorrent 握手固定长度：pstrlen(1)+pstr(19)+reserved(8)+infohash(20)+peer_id(20)。
pub const HANDSHAKE_LEN: usize = 68;
const PSTR: &[u8] = b"BitTorrent protocol";
const COPY_BUF: usize = 32 * 1024;

/// 解析 68 字节握手里的 infohash（第 28..48 字节）。
///
/// 不 trim、不归一化、不猜测扩展握手：pstr 不符或长度不够就是「不是标准 BT 握手」。
/// 这是零信任口径——握手字节来自网络，任何「差不多就算」都会把错误 infohash 喂给唤醒。
pub fn parse_handshake(buf: &[u8]) -> Result<[u8; 20], String> {
    if buf.len() < HANDSHAKE_LEN {
        return Err(format!("BT 握手不足 {HANDSHAKE_LEN} 字节: {}", buf.len()));
    }
    if buf[0] as usize != PSTR.len() || &buf[1..1 + PSTR.len()] != PSTR {
        return Err("非标准 BitTorrent 握手（pstr 不符）".to_string());
    }
    let mut ih = [0u8; 20];
    ih.copy_from_slice(&buf[28..48]);
    Ok(ih)
}

/// 小写十六进制 infohash，与 `cr_hash::hex` / `cr_torrent` 的 `seed_tasks` 键一致。
pub fn handshake_infohash_hex(buf: &[u8]) -> Result<String, String> {
    let ih = parse_handshake(buf)?;
    let mut s = String::with_capacity(40);
    for b in ih {
        s.push_str(&format!("{b:02x}"));
    }
    Ok(s)
}

/// 构造一条标准握手（测试与 e2e 探针复用，避免两边各写一份字节序）。
pub fn handshake_bytes(infohash: &[u8; 20], peer_id: &[u8; 20]) -> [u8; HANDSHAKE_LEN] {
    let mut out = [0u8; HANDSHAKE_LEN];
    out[0] = PSTR.len() as u8;
    out[1..20].copy_from_slice(PSTR);
    // 20..28 reserved 全 0
    out[28..48].copy_from_slice(infohash);
    out[48..68].copy_from_slice(peer_id);
    out
}

/// 从连接读满一条握手，读完把读超时恢复原值。
///
/// 超时语义很重要：T0-07 的健康探测只 `connect` 不发字节；没有读超时的话
/// 这条线程会永远挂着，看起来像「服务正常」实则是线程泄漏。
pub fn read_handshake(
    stream: &mut TcpStream,
    timeout: Duration,
) -> io::Result<[u8; HANDSHAKE_LEN]> {
    let old = stream.read_timeout()?;
    stream.set_read_timeout(Some(timeout))?;
    let mut buf = [0u8; HANDSHAKE_LEN];
    let result = stream.read_exact(&mut buf);
    let _ = stream.set_read_timeout(old);
    result.map(|_| buf)
}

/// 把客户端已 consumed 的握手字节补回，再双向搬运到引擎监听口。
///
/// **不能**直接复用 `cr_bridge::splice_to_listener`：那条路从连接开头搬运，
/// 握手已被 guard 读走，引擎会收到截断的握手，表现为「唤醒成功但取片 0 字节」——
/// 正是 ADR-021 记录的「只有单测、没接线」同类陷阱。
///
/// 收尾规则参考 `cr_nat::relay_tunnel::splice_blocking_with`：
/// 客户端方向 EOF → 对引擎半关写端；引擎方向 EOF → 对客户端半关写端。
/// 两个方向都尽量把已读字节写完，不因对侧关闭而丢数据。
pub fn splice_prefaced(
    client: TcpStream,
    engine_listen: SocketAddr,
    prefix: &[u8],
) -> io::Result<(u64, u64)> {
    let mut upstream = TcpStream::connect(engine_listen)?;
    let _ = client.set_nodelay(true);
    let _ = upstream.set_nodelay(true);
    upstream.write_all(prefix)?;

    let client_read = client.try_clone()?;
    let engine_read = upstream.try_clone()?;

    let c2e = thread::spawn(move || -> io::Result<u64> {
        let mut n = 0u64;
        let mut buf = vec![0u8; COPY_BUF];
        let mut client = client;
        let mut upstream = upstream;
        loop {
            match client.read(&mut buf) {
                Ok(0) => break,
                Ok(k) => {
                    upstream.write_all(&buf[..k])?;
                    n += k as u64;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        let _ = upstream.shutdown(Shutdown::Write);
        Ok(n)
    });

    let e2c = thread::spawn(move || -> io::Result<u64> {
        let mut n = 0u64;
        let mut buf = vec![0u8; COPY_BUF];
        let mut client_read = client_read;
        let mut engine_read = engine_read;
        loop {
            match engine_read.read(&mut buf) {
                Ok(0) => break,
                Ok(k) => {
                    client_read.write_all(&buf[..k])?;
                    n += k as u64;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        let _ = client_read.shutdown(Shutdown::Write);
        Ok(n)
    });

    // 对端先关时另一方向可能收到 BrokenPipe，那是正常收尾，不当失败上报。
    let up = c2e.join().unwrap_or(Ok(0)).unwrap_or(0);
    let down = e2c.join().unwrap_or(Ok(0)).unwrap_or(0);
    Ok((up, down))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn ih(byte: u8) -> [u8; 20] { [byte; 20] }

    #[test]
    fn parse_handshake_reads_infohash_at_byte_28() {
        let mut buf = [0u8; HANDSHAKE_LEN];
        buf[0] = 19;
        buf[1..20].copy_from_slice(PSTR);
        buf[28..48].copy_from_slice(&ih(0xab));
        buf[48..68].copy_from_slice(&[0xcd; 20]);
        assert_eq!(parse_handshake(&buf).unwrap(), ih(0xab));
        assert_eq!(handshake_infohash_hex(&buf).unwrap(), "ab".repeat(20));
    }

    #[test]
    fn parse_handshake_rejects_bad_pstr_and_short_buffer() {
        let buf = handshake_bytes(&ih(1), &ih(2));
        let mut bad = buf;
        bad[1] = b'X';
        assert!(parse_handshake(&bad).is_err());
        assert!(parse_handshake(&buf[..67]).is_err());
        // 多读的字节不影响解析（代理按固定 68 读）
        let mut long = buf.to_vec();
        long.extend_from_slice(b"extra");
        assert_eq!(parse_handshake(&long).unwrap(), ih(1));
    }

    #[test]
    fn handshake_bytes_roundtrip() {
        let b = handshake_bytes(&ih(0x7f), &ih(0x33));
        assert_eq!(b.len(), HANDSHAKE_LEN);
        assert_eq!(parse_handshake(&b).unwrap(), ih(0x7f));
    }

    #[test]
    fn splice_prefaced_delivers_consumed_prefix_and_followup() {
        // 三个角色各用独立 listener/socket，不能复用同一个——
        // splice_prefaced 自己会向 engine_addr 再建一条连接，
        // 若客户端那条也连同一个 listener，第二条连接永远没人 accept，join 直接挂死。
        let external = TcpListener::bind("127.0.0.1:0").unwrap();
        let external_addr = external.local_addr().unwrap();
        let engine = TcpListener::bind("127.0.0.1:0").unwrap();
        let engine_addr = engine.local_addr().unwrap();

        let prefix = handshake_bytes(&ih(0x55), &ih(0x66));
        let followup = b"hello!!";

        // 模拟 BT 客户端：连外部口，先发握手（guard 已读走），再发后续字节。
        let cli = thread::spawn(move || {
            let mut s = TcpStream::connect(external_addr).unwrap();
            s.write_all(&prefix).unwrap();
            s.write_all(followup).unwrap();
            // 读到引擎回包才退出（s 随之关闭，splice 的两个方向都能 EOF）。
            let mut reply = [0u8; 9];
            s.read_exact(&mut reply).unwrap();
            reply
        });

        // 模拟引擎：在内部口 accept，应完整收到握手 + 后续字节，再回包。
        let eng = thread::spawn(move || {
            let (mut s, _) = engine.accept().unwrap();
            let mut got = [0u8; HANDSHAKE_LEN + 7];
            s.read_exact(&mut got).unwrap();
            s.write_all(b"ENGINE-OK").unwrap();
            got
        });

        // guard：accept 外部连接后「已读走 68 B」，只把 prefix 交给 splice 补回。
        let (mut client, _) = external.accept().unwrap();
        let mut consumed = [0u8; HANDSHAKE_LEN];
        client.read_exact(&mut consumed).unwrap();
        assert_eq!(&consumed[..], &prefix[..]);

        let (up, down) = splice_prefaced(client, engine_addr, &prefix).unwrap();
        let got = eng.join().unwrap();
        let reply = cli.join().unwrap();
        assert_eq!(&got[..HANDSHAKE_LEN], &prefix[..]);
        assert_eq!(&got[HANDSHAKE_LEN..], followup);
        assert_eq!(&reply, b"ENGINE-OK");
        assert_eq!(up, followup.len() as u64);
        assert_eq!(down, 9, "引擎回包必须送达客户端，不能只算上行");
    }
}
