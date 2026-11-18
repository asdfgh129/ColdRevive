//! coldrevive-relaybench — 中继路径的真实测量工具（W3.9 后续）。
//!
//! 存在的理由：中继的单测能证明「帧能过去」，证明不了「真实网络下的往返有多慢、
//! 会不会丢、单 IP 上限在真实流量里到底拦不拦得住」。这三件事只能量。
//!
//! 两个阶段，都可单跑：
//! - **RTT / 丢包**：本进程起两个客户端（A 发、B 回声），都注册到同一个中继。
//!   A 逐条发帧、等回声，统计每条的往返时延与丢失。往返包含「A→中继→B→中继→A」，
//!   也就是一次完整信令往来的真实成本。
//! - **单 IP 连接上限**：并发开 N 条 TCP 到中继，统计「被对端立即关闭」的条数。
//!   这是对服务端 `max_conns_per_ip` 的**外部观测**，不读服务端内部计数。
//!
//! 纪律：
//! - 只报测到的数：没收到回声就是丢失，不把超时改写成「0 ms」。
//! - 输出机读 JSON，字段名写清测量口径（`rtt_ms_*` 是往返，不是单向）。
//! - 结束时 A、B 主动断连，避免把服务端的连接名额留在占位状态。

use cr_nat::relay::{RelayClient, RelayConn, NODE_ID_LEN};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |name: &str| -> Option<String> {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
    };
    let flag = |name: &str| args.iter().any(|a| a == name);

    if args.is_empty() || flag("--help") {
        eprintln!(
            "用法:\n  \
             coldrevive-relaybench --relay <host:port> [--rounds 50] [--payload 64] \\\n\
             \x20                     [--timeout-ms 1000] [--flood 0] [--auth-key <64hex|file:path>] \\\n\
             \x20                     [--udp] [--udp-remote-echo] [--udp-echo [--secs 30]] [--json]\n\
             \n\
             --relay     中继地址（服务端 coldrevive-relay 的监听地址）\n\
             --udp        走 UDP 承载（对应服务端 --udp-listen）。测的是 A→中继→B→中继→A 全链，\n\
             \x20           同时单独统计中继回的 STATUS（forwarded / target_absent / rate_limited），\n\
             \x20           用来区分「路径丢包」与「中继没转」。\n\
             --udp-remote-echo  对端在另一台机器上（与下面那条配合），本进程只跑 A 侧\n\
             --udp-echo   只做回声端：注册固定 id，把 DELIVER 原样回给来源，跑 --secs 秒后退出。\n\
             \x20           用途是证明「两台各自在 NAT 后的主机能同时经中继互达」——同机两个\n\
             \x20           socket 共用同一个出口，证明不了这件事。\n\
             --rounds    往返测量条数（默认 50）\n\
             --payload   每条消息负载字节（默认 64，最小 1）\n\
             --flood     并发连接数，0 = 不做单 IP 上限测试（默认 0）\n\
             --auth-key  中继 PSK（64 位十六进制或 file:<路径>）。公网中继必须配，\n\
             \x20           否则服务端拒绝在非回环地址上跑明文。\n\
             \n\
             说明: 只测量，不评估。丢包 = 在 timeout 内没收到回声的条数，如实计入。"
        );
        std::process::exit(if args.is_empty() { 2 } else { 0 });
    }

    let relay = match opt("--relay") {
        Some(v) if !v.trim().is_empty() => v,
        _ => fail("必须给 --relay <host:port>"),
    };
    let rounds: usize = opt("--rounds").and_then(|s| s.parse().ok()).unwrap_or(50).clamp(1, 5000);
    let payload_len: usize = opt("--payload").and_then(|s| s.parse().ok()).unwrap_or(64).clamp(1, 60_000);
    let timeout_ms: u64 = opt("--timeout-ms").and_then(|s| s.parse().ok()).unwrap_or(1000).clamp(50, 30_000);
    let flood: usize = opt("--flood").and_then(|s| s.parse().ok()).unwrap_or(0).min(4096);
    let as_json = flag("--json");

    // 公网中继一律要求 PSK（服务端绑非回环且无密钥直接拒绝启动），
    // 所以这个工具必须能带密钥，否则「真实网络往返」只能在回环上量——那没有意义。
    if let Some(spec) = opt("--auth-key") {
        if let Err(e) = cr_nat::relay::set_psk_from_spec(Some(&spec)) {
            fail(&format!("--auth-key 无效: {}", e));
        }
    }

    if flag("--udp") {
        if let Err(e) = run_udp(
            &relay,
            rounds,
            payload_len,
            timeout_ms,
            as_json,
            !flag("--udp-remote-echo"),
        ) {
            fail(&e);
        }
        return;
    }

    if flag("--udp-echo") {
        let secs: u64 = opt("--secs").and_then(|s| s.parse().ok()).unwrap_or(30).clamp(1, 3600);
        if let Err(e) = run_udp_echo(&relay, secs, as_json) {
            fail(&e);
        }
        return;
    }

    if let Err(e) = run(&relay, rounds, payload_len, timeout_ms, flood, as_json) {
        fail(&e);
    }
}

/// 单独的回声端：注册固定 id，把收到的每个 `DELIVER` 原样回给来源。
///
/// 存在的理由：`--udp` 默认在同一台机器上起 A、B 两个 socket，测到的「两跳公网」
/// 是真的，但**同一个 NAT 后面的两个 socket 不能冒充「两个不同 NAT」**。要证明
/// 「两台各自在 NAT 后的主机能同时经中继互达」，必须真的在两台机器上跑两端，
/// 所以需要一个只需要 `--relay` 就能跑的回声端。id 与 `--udp` 里的 B 相同。
fn run_udp_echo(relay: &str, secs: u64, as_json: bool) -> Result<(), String> {
    use cr_nat::relay_udp::{UdpKind, UdpRelayClient};

    let id_b = node_id(b'B', 2);
    let mut b = UdpRelayClient::connect(relay, Duration::from_millis(1000))?;
    b.hello(id_b, Duration::from_millis(1500))
        .map_err(|e| format!("回声端注册失败: {e}"))?;
    let local = b
        .local_addr()
        .map(|x| x.to_string())
        .unwrap_or_else(|_| "?".into());
    if !as_json {
        println!("UDP_ECHO_READY relay={relay} local={local} secs={secs}");
    }
    let started = Instant::now();
    let mut got = 0u64;
    let mut sent = 0u64;
    let mut bytes = 0u64;
    while started.elapsed() < Duration::from_secs(secs) {
        match b.recv_frame(Duration::from_millis(200)) {
            Ok(Some(f)) if f.kind == UdpKind::Deliver => {
                got += 1;
                bytes += f.payload.len() as u64;
                if b.send_data(&f.from, &f.payload).is_ok() {
                    sent += 1;
                }
            }
            Ok(_) => continue,
            Err(e) => return Err(format!("回声端收包失败: {e}")),
        }
    }
    let _ = b.bye();
    let pairs = vec![
        ("ok", "true".to_string()),
        ("mode", q("relay-udp-echo")),
        ("relay", q(relay)),
        ("local_addr", q(&local)),
        ("secs", secs.to_string()),
        ("delivered_in", got.to_string()),
        ("echoed_out", sent.to_string()),
        ("bytes_in", bytes.to_string()),
    ];
    if as_json {
        println!("{}", json(&pairs));
    } else {
        println!(
            "UDP_ECHO relay={} local={} delivered_in={} echoed_out={} bytes_in={}",
            relay, local, got, sent, bytes
        );
    }
    Ok(())
}

/// UDP 承载下的往返测量（W3.14）。
///
/// 与 TCP 版的区别只有传输层：A 发 `DATA`，中继转给 B，B 原样回给 A，A 用**同一个
/// 单调时钟**量 A→中继→B→中继→A 的全链往返。同时单独统计中继回的 `STATUS`：
/// `forwarded` = 中继确实转出去了，`target_absent` = 对端没注册。**这三者不能混**：
/// 「没收到回声」可能是路径丢包、可能是中继没转、也可能是对端根本没在。
fn run_udp(
    relay: &str,
    rounds: usize,
    payload_len: usize,
    timeout_ms: u64,
    as_json: bool,
    local_echo: bool,
) -> Result<(), String> {
    use cr_nat::relay_udp::{UdpKind, UdpRelayClient, UdpStatus};

    let started = Instant::now();
    let id_a = node_id(b'A', 1);
    let id_b = node_id(b'B', 2);
    let timeout = Duration::from_millis(timeout_ms);

    let mut a = UdpRelayClient::connect(relay, timeout)
        .and_then(|c| {
            let mut c = c;
            c.hello(id_a, timeout)?;
            Ok(c)
        })
        .map_err(|e| format!("A 注册失败: {e}"))?;
    let local_a = a
        .local_addr()
        .map(|x| x.to_string())
        .unwrap_or_else(|_| "?".into());
    let server = a.server_addr().to_string();

    let stop = Arc::new(AtomicBool::new(false));
    let echoed = Arc::new(AtomicU64::new(0));
    let bad_echo = Arc::new(AtomicU64::new(0));
    // `local_echo = false` 时对端在**另一台机器**上（`--udp-echo`），本进程不起回声端。
    // 这条分支不是可选项的糖：同机两个 socket 共用同一个 NAT 出口，证明不了
    // 「两台各自在 NAT 后的主机能同时经中继互达」。
    let mut echo_handle = if local_echo {
        let mut b = UdpRelayClient::connect(relay, timeout)
            .and_then(|c| {
                let mut c = c;
                c.hello(id_b, timeout)?;
                Ok(c)
            })
            .map_err(|e| format!("B 注册失败: {e}"))?;
        let echo_stop = stop.clone();
        let echo_count = echoed.clone();
        let bad = bad_echo.clone();
        Some(std::thread::spawn(move || -> Result<(), String> {
            loop {
                if echo_stop.load(Ordering::Relaxed) {
                    return Ok(());
                }
                match b.recv_frame(Duration::from_millis(200)) {
                    Ok(Some(f)) if f.kind == UdpKind::Deliver => {
                        match b.send_data(&f.from, &f.payload) {
                            Ok(_) => {
                                echo_count.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(_) => {
                                bad.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                    Ok(Some(_)) => continue,
                    Ok(None) => continue,
                    Err(e) => return Err(format!("B 收包失败: {e}")),
                }
            }
        }))
    } else {
        None
    };
    // 出错路径也要先停线程再返回，否则 `Err` 会掩盖回声线程里的真实原因。
    macro_rules! bail {
        ($msg:expr) => {{
            stop.store(true, Ordering::Relaxed);
            if let Some(h) = echo_handle.take() {
                let _ = h.join();
            }
            return Err($msg);
        }};
    }

    let payload = vec![0x5Au8; payload_len];
    let mut rtts: Vec<u128> = Vec::with_capacity(rounds);
    let mut lost = 0usize;
    let mut st_forwarded = 0usize;
    let mut st_absent = 0usize;
    let mut st_limited = 0usize;
    let mut st_other = 0usize;

    for seq in 0..rounds {
        let mut msg = payload.clone();
        let head = 8.min(msg.len());
        msg[..head].copy_from_slice(&(seq as u64).to_be_bytes()[..head]);
        let t0 = Instant::now();
        if let Err(e) = a.send_data(&id_b, &msg) {
            bail!(format!("A 发第 {seq} 条失败: {e}"));
        }
        // 等到「带本条序号」的回声为止；期间出现的 STATUS 只计数，不当成回声。
        let deadline = t0 + timeout;
        let mut got = false;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match a.recv_frame(left) {
                Ok(Some(f)) => match f.kind {
                    UdpKind::Deliver => {
                        if f.payload.len() == msg.len()
                            && f.payload[..head] == msg[..head]
                            && f.from == id_b
                        {
                            rtts.push(t0.elapsed().as_micros());
                            got = true;
                            break;
                        }
                        // 长度不符/来源不符：不静默吞掉，说明协议被改坏了。
                        bail!(format!(
                            "第 {seq} 条回声不符: len={} from={:?}",
                            f.payload.len(),
                            f.from[0]
                        ));
                    }
                    UdpKind::Status => match f.payload.first().and_then(|c| UdpStatus::from_code(*c)) {
                        Some(UdpStatus::Forwarded) => st_forwarded += 1,
                        Some(UdpStatus::TargetAbsent) => st_absent += 1,
                        Some(UdpStatus::RateLimited) => st_limited += 1,
                        _ => st_other += 1,
                    },
                    UdpKind::Reject => {
                        bail!(format!(
                            "中继拒绝: {}",
                            String::from_utf8_lossy(&f.payload)
                        ));
                    }
                    _ => continue,
                },
                Ok(None) => continue,
                Err(e) => {
                    bail!(format!("A 收包失败: {e}"));
                }
            }
        }
        if !got {
            lost += 1;
        }
    }

    stop.store(true, Ordering::Relaxed);
    if let Some(h) = echo_handle.take() {
        h.join().map_err(|_| "回声线程 panic".to_string())??;
    }
    let _ = a.bye();

    rtts.sort_unstable();
    let pct = |p: f64| -> u128 {
        if rtts.is_empty() {
            return 0;
        }
        let idx = ((rtts.len() - 1) as f64 * p).round() as usize;
        rtts[idx.min(rtts.len() - 1)]
    };
    let mean = if rtts.is_empty() {
        0.0
    } else {
        rtts.iter().map(|v| *v as f64).sum::<f64>() / rtts.len() as f64
    };
    let jitter = if rtts.len() < 2 {
        0.0
    } else {
        let mut d = Vec::with_capacity(rtts.len() - 1);
        for w in rtts.windows(2) {
            d.push((w[1] as f64 - w[0] as f64).abs());
        }
        d.iter().sum::<f64>() / d.len() as f64
    };

    let total_ms = started.elapsed().as_millis();
    let ok = lost * 2 < rounds.max(1);
    let us = |v: u128| format!("{:.3}", v as f64 / 1000.0);
    let pairs: Vec<(&str, String)> = vec![
        ("ok", if ok { "true".into() } else { "false".into() }),
        ("mode", q("relay-bench-udp")),
        ("relay", q(relay)),
        ("relay_server", q(&server)),
        ("local_addr", q(&local_a)),
        ("rounds", rounds.to_string()),
        ("payload_bytes", payload_len.to_string()),
        ("timeout_ms", timeout_ms.to_string()),
        ("echoed", echoed.load(Ordering::Relaxed).to_string()),
        ("echo_fail", bad_echo.load(Ordering::Relaxed).to_string()),
        ("received", rtts.len().to_string()),
        ("lost", lost.to_string()),
        ("loss_pct", format!("{:.2}", lost as f64 * 100.0 / rounds as f64)),
        ("rtt_ms_min", us(rtts.first().copied().unwrap_or(0))),
        ("rtt_ms_p50", us(pct(0.50))),
        ("rtt_ms_p90", us(pct(0.90))),
        ("rtt_ms_p99", us(pct(0.99))),
        ("rtt_ms_max", us(rtts.last().copied().unwrap_or(0))),
        ("rtt_ms_mean", format!("{:.3}", mean / 1000.0)),
        ("jitter_ms_mean_abs", format!("{:.3}", jitter / 1000.0)),
        ("status_forwarded", st_forwarded.to_string()),
        ("status_target_absent", st_absent.to_string()),
        ("status_rate_limited", st_limited.to_string()),
        ("status_other", st_other.to_string()),
        ("total_ms", total_ms.to_string()),
        ("evidence_scope", q("real-network-roundtrip-udp-relay")),
    ];

    if as_json {
        println!("{}", json(&pairs));
    } else {
        println!(
            "RELAY_BENCH_UDP relay={} rounds={} recv={} lost={} loss={:.2}% \
             rtt_ms(min/p50/p90/p99/max)={}/{}/{}/{}/{} mean={:.3} jitter={:.3} \
             status(fwd/absent/limited/other)={}/{}/{}/{} total_ms={}",
            relay,
            rounds,
            rtts.len(),
            lost,
            lost as f64 * 100.0 / rounds as f64,
            us(rtts.first().copied().unwrap_or(0)),
            us(pct(0.50)),
            us(pct(0.90)),
            us(pct(0.99)),
            us(rtts.last().copied().unwrap_or(0)),
            mean / 1000.0,
            jitter / 1000.0,
            st_forwarded,
            st_absent,
            st_limited,
            st_other,
            total_ms
        );
    }

    if ok {
        Ok(())
    } else {
        Err(format!("丢包率过高：{lost}/{rounds}"))
    }
}

fn run(
    relay: &str,
    rounds: usize,
    payload_len: usize,
    timeout_ms: u64,
    flood: usize,
    as_json: bool,
) -> Result<(), String> {
    let started = Instant::now();

    // 两个身份：A 发、B 回声。ID 必须不同，否则中继会认为是同一节点重连。
    let id_a = node_id(b'A', 1);
    let id_b = node_id(b'B', 2);

    let mut a = RelayClient::new(relay.to_string())
        .connect(Duration::from_secs(5))
        .map_err(|e| format!("A 连接中继失败: {e}"))?;
    a.register(&id_a).map_err(|e| format!("A 注册失败: {e}"))?;

    let mut b = RelayClient::new(relay.to_string())
        .connect(Duration::from_secs(5))
        .map_err(|e| format!("B 连接中继失败: {e}"))?;
    b.register(&id_b).map_err(|e| format!("B 注册失败: {e}"))?;

    // 回声线程：B 收到谁的帧就原样发回给谁。收到 stop 前一直转。
    let stop = Arc::new(AtomicBool::new(false));
    let echoed = Arc::new(AtomicU64::new(0));
    let echo_stop = stop.clone();
    let echo_count = echoed.clone();
    let echo = std::thread::spawn(move || -> Result<(), String> {
        b.set_read_timeout(Some(Duration::from_millis(200))).ok();
        while !echo_stop.load(Ordering::Relaxed) {
            match b.recv_frame_optional() {
                Ok(Some(frame)) => {
                    let from = frame.node;
                    // 中继投递的 Data 帧带来源 ID；原样回发即「对端回声」。
                    if b.send_relay(&from, &frame.payload).is_ok() {
                        echo_count.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Ok(None) => continue,
                Err(e) => return Err(format!("B 收帧失败: {e}")),
            }
        }
        Ok(())
    });

    // A 逐条发、逐条等。读超时给出「这条丢了」，而不是把整轮挂在一条上。
    a.set_read_timeout(Some(Duration::from_millis(timeout_ms))).ok();
    let payload = vec![0x5Au8; payload_len];
    let mut rtts: Vec<u128> = Vec::with_capacity(rounds);
    let mut lost = 0usize;

    for seq in 0..rounds {
        let mut msg = payload.clone();
        // 带序号：回声回来时能确认是这一条，不是上一条的迟到回声。
        let head = 8.min(msg.len());
        msg[..head].copy_from_slice(&(seq as u64).to_be_bytes()[..head]);
        let t0 = Instant::now();
        if let Err(e) = a.send_relay(&id_b, &msg) {
            stop.store(true, Ordering::Relaxed);
            let _ = echo.join();
            return Err(format!("A 发第 {seq} 条失败: {e}"));
        }
        match a.recv_frame() {
            Ok(frame) => {
                let dt = t0.elapsed().as_micros();
                if frame.payload.len() != msg.len() {
                    stop.store(true, Ordering::Relaxed);
                    let _ = echo.join();
                    return Err(format!(
                        "第 {seq} 条回声长度不符: 发出 {} 收到 {}",
                        msg.len(),
                        frame.payload.len()
                    ));
                }
                rtts.push(dt);
            }
            Err(_) => lost += 1,
        }
    }

    stop.store(true, Ordering::Relaxed);
    let echo_result = echo.join().map_err(|_| "回声线程 panic".to_string())?;
    // 先收线程结果再删连接：否则会把「回声线程的错误」变成静默。
    echo_result?;
    // `b` 已被回声线程接管并在收线程结束时随线程销毁，这里只能显式关 A。
    drop(a);

    rtts.sort_unstable();
    let pct = |p: f64| -> u128 {
        if rtts.is_empty() {
            return 0;
        }
        let idx = ((rtts.len() - 1) as f64 * p).round() as usize;
        rtts[idx.min(rtts.len() - 1)]
    };
    let mean = if rtts.is_empty() {
        0.0
    } else {
        rtts.iter().map(|v| *v as f64).sum::<f64>() / rtts.len() as f64
    };

    // 第二阶段（可选）：单 IP 并发连接上限的外部观测。
    let flood_result = if flood > 0 {
        Some(flood_connections(relay, flood)?)
    } else {
        None
    };

    let total_ms = started.elapsed().as_millis();
    let ok = lost * 2 < rounds.max(1); // 过半丢失即视为路径不可用
    let rtt_us = |v: u128| format!("{:.3}", v as f64 / 1000.0);
    let mut pairs: Vec<(&str, String)> = vec![
        ("ok", if ok { "true".into() } else { "false".into() }),
        ("mode", q("relay-bench")),
        ("relay", q(relay)),
        ("rounds", rounds.to_string()),
        ("payload_bytes", payload_len.to_string()),
        ("timeout_ms", timeout_ms.to_string()),
        ("echoed", echoed.load(Ordering::Relaxed).to_string()),
        ("received", rtts.len().to_string()),
        ("lost", lost.to_string()),
        ("loss_pct", format!("{:.2}", lost as f64 * 100.0 / rounds as f64)),
        ("rtt_ms_min", rtt_us(rtts.first().copied().unwrap_or(0))),
        ("rtt_ms_p50", rtt_us(pct(0.50))),
        ("rtt_ms_p90", rtt_us(pct(0.90))),
        ("rtt_ms_p99", rtt_us(pct(0.99))),
        ("rtt_ms_max", rtt_us(rtts.last().copied().unwrap_or(0))),
        ("rtt_ms_mean", format!("{:.3}", mean / 1000.0)),
        ("total_ms", total_ms.to_string()),
        ("evidence_scope", q("real-network-roundtrip")),
    ];
    if let Some(f) = &flood_result {
        pairs.push(("flood_attempted", f.attempted.to_string()));
        pairs.push(("flood_established", f.established.to_string()));
        pairs.push(("flood_closed_by_peer", f.closed.to_string()));
        pairs.push(("flood_connect_failed", f.refused.to_string()));
    }

    if as_json {
        println!("{}", json(&pairs));
    } else {
        println!(
            "RELAY_BENCH relay={} rounds={} recv={} lost={} loss={:.2}% \
             rtt_ms(min/p50/p90/p99/max)={}/{}/{}/{}/{} mean={:.3} total_ms={}",
            relay,
            rounds,
            rtts.len(),
            lost,
            lost as f64 * 100.0 / rounds as f64,
            rtt_us(rtts.first().copied().unwrap_or(0)),
            rtt_us(pct(0.50)),
            rtt_us(pct(0.90)),
            rtt_us(pct(0.99)),
            rtt_us(rtts.last().copied().unwrap_or(0)),
            mean / 1000.0,
            total_ms
        );
        if let Some(f) = &flood_result {
            println!(
                "RELAY_FLOOD attempted={} established={} closed_by_peer={} connect_failed={}",
                f.attempted, f.established, f.closed, f.refused
            );
        }
    }

    if ok {
        Ok(())
    } else {
        Err(format!("丢包率过高：{lost}/{rounds}"))
    }
}

struct FloodResult {
    attempted: usize,
    established: usize,
    closed: usize,
    refused: usize,
}

/// 并发开 N 条到中继的 TCP，统计「连上就被关」的条数。
///
/// 判定口径：连接成功后设 400 ms 读超时，读到 `Ok(0)`（对端 FIN）算被拒；
/// 读到超时算「仍在服务」。这里**不**发送任何协议帧——服务端的按 IP 上限
/// 是在握手前判的，不需要我们配合。
fn flood_connections(relay: &str, n: usize) -> Result<FloodResult, String> {
    let addr: SocketAddr = match relay.parse() {
        Ok(a) => a,
        Err(_) => {
            use std::net::ToSocketAddrs;
            let mut it = relay
                .to_socket_addrs()
                .map_err(|e| format!("解析 {relay} 失败: {e}"))?;
            it.next().ok_or_else(|| format!("{relay} 没有解析出地址"))?
        }
    };
    let mut held: Vec<TcpStream> = Vec::with_capacity(n);
    let mut established = 0usize;
    let mut refused = 0usize;
    for _ in 0..n {
        match TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
            Ok(s) => {
                established += 1;
                held.push(s);
            }
            Err(_) => refused += 1,
        }
    }
    // 保持 300 ms，让服务端有机会执行「超限即关」；随后并行探测是否已被关闭。
    //
    // 必须并行：串行读 N 条、每条等一个超时，会把 64 条连接测成二十几秒，
    // 期间服务端可能因为空闲策略回收，测出来的「被关」就不再是「超限被关」。
    std::thread::sleep(Duration::from_millis(300));
    let closed = std::sync::atomic::AtomicUsize::new(0);
    let probe_timeout = Duration::from_millis(80);
    std::thread::scope(|scope| {
        let chunk = (held.len() + 15) / 16;
        for group in held.chunks_mut(chunk.max(1)) {
            let counter = &closed;
            scope.spawn(move || {
                for s in group.iter_mut() {
                    let _ = s.set_read_timeout(Some(probe_timeout));
                    let mut buf = [0u8; 1];
                    if let Ok(0) = s.read(&mut buf) {
                        counter.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
        }
    });
    let closed = closed.load(Ordering::Relaxed);
    // 主动断连，把名额还回去。
    for s in held.iter_mut() {
        let _ = s.shutdown(std::net::Shutdown::Both);
    }
    Ok(FloodResult { attempted: n, established, closed, refused })
}

fn node_id(prefix: u8, tail: u8) -> [u8; NODE_ID_LEN] {
    let mut id = [0u8; NODE_ID_LEN];
    id[0] = prefix;
    id[NODE_ID_LEN - 1] = tail;
    id
}

fn q(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json(pairs: &[(&str, String)]) -> String {
    let body: Vec<String> = pairs.iter().map(|(k, v)| format!("{}:{}", q(k), v)).collect();
    format!("{{{}}}", body.join(","))
}

fn fail(msg: &str) -> ! {
    eprintln!("{}", json(&[("ok", "false".into()), ("error", q(msg))]));
    std::process::exit(1);
}

// 保留一个 `Write` 引用：回声线程用 `send_relay` 而非裸写，这里仅用于未来 raw 模式。
#[allow(dead_code)]
fn _unused(_w: &dyn Write, _c: &RelayConn) {}
