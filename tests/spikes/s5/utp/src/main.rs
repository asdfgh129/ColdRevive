//! S5 真实 uTP 协议栈拥塞链路量测（ColdRevive M1 / G3 前置证据）。
//!
//! 与 qos_bench.py 的区别（后者是「LEDbat 机理的可执行模型」）：
//!   * 本探针使用**真实的 librqbit-utp 0.7.0 协议栈**（BEP 29），真实 UDP socket、
//!     真实 SYN/ACK/窗口/重传，不是吞吐公式或控制器模型。
//!   * 前台 TCP 流与后台 uTP 流经过**同一个真实时钟令牌桶链路**（共享瓶颈），
//!     瓶颈排队用真实 `tokio::time::sleep` 产生，因此 uTP 观测到的是真实 RTT 膨胀。
//!   * 后台另有一层 QoS 令牌桶，速率 = background_upload_ratio × 链路容量，对应 CONFIG.md 的旋钮。
//!
//! 范围红线：瓶颈是**本机用户态整形**，不是 tc/netem 内核队列，也不是跨主机真实拥塞链路；
//! 按 s5/README §6 第 3 条，本证据依然**不能支撑 S5.status=pass**，只用于验证真实 uTP 栈的
//! 退让行为、量化 NFR-03 第二半，并核实「prefer_utp 是否真的带来 LEDBAT 退让」。
//!
//! 用法:
//!   cargo run --offline --release -- --seconds 3 --rounds 3 --out <report.json>
//! 退出码 0 = 量测完成（不表示判据通过）。

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use librqbit_utp::UtpSocketUdp;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::Mutex;
use tokio::time::{sleep, Instant};

const CHUNK: usize = 16 * 1024;

// ---------------------------------------------------------------------------
// 共享瓶颈：真实时钟令牌桶，所有 transmit() 的等待都是真实 sleep
// ---------------------------------------------------------------------------

struct LinkState {
    rate_bps: f64,
    burst: f64,
    tokens: f64,
    last: Instant,
    bytes: u64,
    max_wait_ms: f64,
    sum_wait_ms: f64,
    calls: u64,
}

#[derive(Clone)]
struct Link(Arc<Mutex<LinkState>>);

impl Link {
    fn new(rate_bps: f64) -> Self {
        let burst = (rate_bps * 0.05).max(4.0 * CHUNK as f64);
        Link(Arc::new(Mutex::new(LinkState {
            rate_bps,
            burst,
            tokens: burst,
            last: Instant::now(),
            bytes: 0,
            max_wait_ms: 0.0,
            sum_wait_ms: 0.0,
            calls: 0,
        })))
    }

    /// 返回本次传输在瓶颈队列中真实等待的秒数。
    async fn transmit(&self, n: usize) -> f64 {
        let mut waited = 0.0f64;
        loop {
            let sleep_for: Option<f64> = {
                let mut l = self.0.lock().await;
                let now = Instant::now();
                let dt = now.duration_since(l.last).as_secs_f64();
                l.tokens = (l.tokens + dt * l.rate_bps).min(l.burst);
                l.last = now;
                if l.tokens >= n as f64 {
                    l.tokens -= n as f64;
                    None
                } else {
                    let deficit = n as f64 - l.tokens;
                    Some((deficit / l.rate_bps).clamp(0.0002, 0.02))
                }
            };
            match sleep_for {
                None => {
                    let mut l = self.0.lock().await;
                    l.bytes += n as u64;
                    l.calls += 1;
                    l.max_wait_ms = l.max_wait_ms.max(waited * 1000.0);
                    l.sum_wait_ms += waited * 1000.0;
                    return waited;
                }
                Some(d) => {
                    let t0 = Instant::now();
                    sleep(Duration::from_secs_f64(d)).await;
                    waited += t0.elapsed().as_secs_f64();
                }
            }
        }
    }

    async fn stats(&self) -> serde_json::Value {
        let l = self.0.lock().await;
        json!({
            "rate_bps": l.rate_bps,
            "bytes_forwarded": l.bytes,
            "max_queue_wait_ms": (l.max_wait_ms * 100.0).round() / 100.0,
            "mean_queue_wait_ms": if l.calls > 0 {
                (l.sum_wait_ms / l.calls as f64 * 100.0).round() / 100.0
            } else { 0.0 },
            "calls": l.calls,
        })
    }
}

// ---------------------------------------------------------------------------
// 单个相位
// ---------------------------------------------------------------------------

struct Phase {
    fg_bytes: u64,
    bg_bytes: u64,
    bg_forwarded_bytes: u64,
    link: serde_json::Value,
}

fn addr(port: u16) -> SocketAddr {
    format!("127.0.0.1:{port}").parse().unwrap()
}

/// ratio=None → 仅前台（基线）；Some(r) → 前台 + 后台真实 uTP（后台限 r×链路）。
async fn run_phase(link_bps: f64, ratio: Option<f64>, seconds: f64,
                   base_port: u16) -> Result<Phase, String> {
    let fg_server_addr = addr(base_port);
    let shaper_tcp_addr = addr(base_port + 1);
    let shaper_in_addr = addr(base_port + 2);
    let shaper_out_addr = addr(base_port + 3);
    let utp_server_addr = addr(base_port + 4);
    let utp_client_addr = addr(base_port + 5);

    let link = Link::new(link_bps);
    let bg_cap = ratio.map(|r| Link::new(link_bps * r));
    let fg_bytes = Arc::new(AtomicU64::new(0));
    let bg_bytes = Arc::new(AtomicU64::new(0));
    let bg_forwarded = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let deadline = Instant::now() + Duration::from_secs_f64(seconds);

    let fg_server = TcpListener::bind(fg_server_addr).await
        .map_err(|e| format!("bind fg_server: {e}"))?;
    let shaper_tcp = TcpListener::bind(shaper_tcp_addr).await
        .map_err(|e| format!("bind shaper_tcp: {e}"))?;

    // 前台接收端：只管读，不计数（计数在中继上，代表真正经过瓶颈的字节）
    let server_task = tokio::spawn(async move {
        let (mut s, _) = match fg_server.accept().await { Ok(v) => v, Err(_) => return };
        let mut buf = vec![0u8; CHUNK];
        loop {
            match s.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    });

    // 前台中继：每条 chunk 都过共享瓶颈
    let l1 = link.clone();
    let fgc = fg_bytes.clone();
    let shaper_task = tokio::spawn(async move {
        let (mut c, _) = match shaper_tcp.accept().await { Ok(v) => v, Err(_) => return };
        let mut s = match TcpStream::connect(fg_server_addr).await {
            Ok(v) => v, Err(_) => return,
        };
        let mut buf = vec![0u8; CHUNK];
        loop {
            if Instant::now() >= deadline { break; }
            let n = match c.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            l1.transmit(n).await;
            if s.write_all(&buf[..n]).await.is_err() { break; }
            fgc.fetch_add(n as u64, Ordering::Relaxed);
        }
        let _ = s.shutdown().await;
    });

    // 前台发送端：窗口内尽力写
    let fg_client = tokio::spawn(async move {
        let mut c = match TcpStream::connect(shaper_tcp_addr).await {
            Ok(v) => v, Err(_) => return,
        };
        let buf = vec![0x5Au8; CHUNK];
        while Instant::now() < deadline {
            if c.write_all(&buf).await.is_err() { break; }
        }
        let _ = c.shutdown().await;
    });

    // ---- 后台真实 uTP 链路（经 UDP 中继共享同一瓶颈）----
    let mut bg_handles = Vec::new();
    if bg_cap.is_some() {
        let s_in = Arc::new(UdpSocket::bind(shaper_in_addr).await
            .map_err(|e| format!("bind shaper_in: {e}"))?);
        let s_out = Arc::new(UdpSocket::bind(shaper_out_addr).await
            .map_err(|e| format!("bind shaper_out: {e}"))?);
        let client_seen = Arc::new(Mutex::new(None::<SocketAddr>));

        let (s_in_a, s_out_a, cap_a, link_a, seen_a, bgf_a) =
            (s_in.clone(), s_out.clone(), bg_cap.clone().unwrap(), link.clone(),
             client_seen.clone(), bg_forwarded.clone());
        bg_handles.push(tokio::spawn(async move {
            let mut buf = vec![0u8; 65536];
            loop {
                let (n, src) = match s_in_a.recv_from(&mut buf).await {
                    Ok(v) => v, Err(_) => break,
                };
                *seen_a.lock().await = Some(src);
                cap_a.transmit(n).await;
                link_a.transmit(n).await;
                bgf_a.fetch_add(n as u64, Ordering::Relaxed);
                if s_out_a.send_to(&buf[..n], utp_server_addr).await.is_err() { break; }
            }
        }));

        let (s_in_b, s_out_b, cap_b, link_b, seen_b) =
            (s_in.clone(), s_out.clone(), bg_cap.clone().unwrap(), link.clone(), client_seen.clone());
        bg_handles.push(tokio::spawn(async move {
            let mut buf = vec![0u8; 65536];
            loop {
                let (n, _src) = match s_out_b.recv_from(&mut buf).await {
                    Ok(v) => v, Err(_) => break,
                };
                cap_b.transmit(n).await;
                link_b.transmit(n).await;
                if let Some(dst) = *seen_b.lock().await {
                    if s_in_b.send_to(&buf[..n], dst).await.is_err() { break; }
                }
            }
        }));

        // uTP 服务端（真实 BEP 29 socket）
        let server_addr = utp_server_addr;
        bg_handles.push(tokio::spawn(async move {
            let sock = match UtpSocketUdp::new_udp(server_addr).await {
                Ok(v) => v, Err(e) => { eprintln!("uTP server bind: {e}"); return; }
            };
            let mut stream = match tokio::time::timeout(
                Duration::from_secs(8), sock.accept()).await {
                Ok(Ok(v)) => v,
                Ok(Err(e)) => { eprintln!("uTP accept: {e}"); return; }
                Err(_) => { eprintln!("uTP accept timeout"); return; }
            };
            let mut buf = vec![0u8; CHUNK];
            loop {
                match stream.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        }));

        // uTP 客户端：窗口内持续写真实分片
        let bgc = bg_bytes.clone();
        let stop_c = stop.clone();
        bg_handles.push(tokio::spawn(async move {
            // 等中继与服务端起来
            sleep(Duration::from_millis(120)).await;
            let sock = match UtpSocketUdp::new_udp(utp_client_addr).await {
                Ok(v) => v, Err(e) => { eprintln!("uTP client bind: {e}"); return; }
            };
            let mut stream = match tokio::time::timeout(
                Duration::from_secs(8), sock.connect(shaper_in_addr)).await {
                Ok(Ok(v)) => v,
                Ok(Err(e)) => { eprintln!("uTP connect: {e}"); return; }
                Err(_) => { eprintln!("uTP connect timeout"); return; }
            };
            let buf = vec![0x3Cu8; 1400];
            while !stop_c.load(Ordering::Relaxed) {
                if stream.write_all(&buf).await.is_err() { break; }
                bgc.fetch_add(buf.len() as u64, Ordering::Relaxed);
            }
            let _ = stream.shutdown().await;
        }));
    }

    // 等窗口结束
    let wait = Duration::from_secs_f64(seconds) + Duration::from_millis(200);
    sleep(wait).await;
    stop.store(true, Ordering::Relaxed);
    let _ = fg_client.await;
    let _ = shaper_task.await;
    let _ = server_task.await;
    sleep(Duration::from_millis(150)).await;
    for h in bg_handles { h.abort(); }

    Ok(Phase {
        fg_bytes: fg_bytes.load(Ordering::Relaxed),
        bg_bytes: bg_bytes.load(Ordering::Relaxed),
        bg_forwarded_bytes: bg_forwarded.load(Ordering::Relaxed),
        link: link.stats().await,
    })
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() { return f64::NAN; }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let opt = |name: &str, default: &str| -> String {
        args.iter().position(|a| a == name)
            .and_then(|i| args.get(i + 1)).cloned().unwrap_or_else(|| default.to_string())
    };
    let link_mbps: f64 = opt("--link-mbps", "16").parse().unwrap();
    let seconds: f64 = opt("--seconds", "3").parse().unwrap();
    let rounds: usize = opt("--rounds", "3").parse().unwrap();
    let ratios_raw = opt("--ratios", "0.2,0.05,0.02");
    let ratios: Vec<f64> = ratios_raw.split(',').filter_map(|s| s.trim().parse().ok()).collect();
    let out = std::path::PathBuf::from(
        opt("--out", "tests/spikes/s5/utp/reports/s5-utp-real.json"));
    let link_bps = link_mbps * 1_000_000.0 / 8.0;

    let mut phase_seq = 0u16;

    let mut per_ratio = Vec::new();
    for ratio in ratios {
        let mut degradations = Vec::new();
        let mut bg_shares = Vec::new();
        let mut samples = Vec::new();
        for round in 0..rounds {
            let base = phase_seq * 20 + 23000;
            let baseline = match run_phase(link_bps, None, seconds, base).await {
                Ok(v) => v, Err(e) => { eprintln!("baseline 失败: {e}"); continue; }
            };
            phase_seq += 1;
            let base2 = phase_seq * 20 + 23000;
            let concurrent = match run_phase(link_bps, Some(ratio), seconds, base2).await {
                Ok(v) => v, Err(e) => { eprintln!("concurrent 失败: {e}"); continue; }
            };
            phase_seq += 1;

            let b = baseline.fg_bytes as f64;
            let t = concurrent.fg_bytes as f64;
            let deg = if b > 0.0 { ((b - t) / b * 100.0).max(0.0) } else { f64::NAN };
            let share = if concurrent.fg_bytes + concurrent.bg_forwarded_bytes > 0 {
                concurrent.bg_forwarded_bytes as f64
                    / (concurrent.fg_bytes + concurrent.bg_forwarded_bytes) as f64
            } else { 0.0 };
            degradations.push(deg);
            bg_shares.push(share);
            samples.push(json!({
                "round": round + 1,
                "baseline_fg_bytes": baseline.fg_bytes,
                "concurrent_fg_bytes": concurrent.fg_bytes,
                "background_written_bytes": concurrent.bg_bytes,
                "background_forwarded_bytes": concurrent.bg_forwarded_bytes,
                "degradation_pct": (deg * 100.0).round() / 100.0,
                "background_actual_link_share": (share * 10000.0).round() / 10000.0,
                "baseline_link": baseline.link,
                "concurrent_link": concurrent.link,
            }));
            println!("ratio={ratio:.2} round={} baseline={}B concurrent={}B degradation={deg:.2}% bg_written={}B bg_forwarded={}B",
                     round + 1, baseline.fg_bytes, concurrent.fg_bytes,
                     concurrent.bg_bytes, concurrent.bg_forwarded_bytes);
        }
        let med = median(degradations.clone());
        per_ratio.push(json!({
            "background_upload_ratio": ratio,
            "rounds": samples.len(),
            "degradation_pct_median": (med * 100.0).round() / 100.0,
            "degradation_pct_all": degradations.iter().map(|d| (d*100.0).round()/100.0)
                .collect::<Vec<f64>>(),
            "background_actual_link_share_median": (median(bg_shares.clone()) * 10000.0).round() / 10000.0,
            "meets_nfr03_5pct": med <= 5.0,
            "samples": samples,
        }));
        println!("  → ratio={ratio:.2} 中位劣化 {med:.2}% (NFR-03 ≤5%: {})",
                 if med <= 5.0 { "PASS" } else { "FAIL" });
    }

    let report = json!({
        "spike": "S5",
        "harness": "tests/spikes/s5/utp/ (cargo run --offline --release)",
        "recorded_at": chrono_like_now(),
        "method": concat!("真实 librqbit-utp 0.7.0 BEP 29 栈 + 真实 UDP/TCP socket + 共享真实时钟瓶颈令牌桶；",
                          "前台 TCP 与后台 uTP 争用同一链路，瓶颈排队由真实 sleep 产生"),
        "config": {
            "link_mbps": link_mbps,
            "seconds_per_phase": seconds,
            "rounds": rounds,
            "background_cap": "ratio × link_bps 的第二层令牌桶（对应 qos.background_upload_ratio）",
            "foreground": "TCP，无自有上限",
            "background": "librqbit-utp UtpSocketUdp 真实 uTP 流，1400B 分片持续写入"
        },
        "source_finding": {
            "crate": "librqbit-utp 0.7.0",
            "congestion_controllers": ["Cubic"],
            "ledbat_implemented": false,
            "evidence": concat!("src/lib.rs 首部注释 `// TODO: LEDBAT congestion control`；",
                                "src/socket.rs CongestionControllerKind 仅有 Cubic 变体；",
                                "src/congestion/ 仅含 cubic.rs"),
            "impact": concat!("prefer_utp=true 只切换传输层到 uTP，并不提供 LEDBAT 低优先级退让；",
                              "NFR-03 ≤5% 目前只能依赖令牌桶 ratio 降级，不可声称 uTP 自动退让")
        },
        "scope_limitation": concat!("瓶颈为本机用户态整形，不是 tc/netem 内核队列或跨主机真实拥塞链路；",
                                    "按 s5/README §6 第 3 条，本证据不能单独支撑 S5.status=pass"),
        "evidence_valid_for_pass": false,
        "per_ratio": per_ratio,
    });
    if let Some(parent) = out.parent() { let _ = std::fs::create_dir_all(parent); }
    std::fs::write(&out, serde_json::to_string_pretty(&report).unwrap() + "\n").unwrap();
    println!("report: {}", out.display());
}

fn chrono_like_now() -> String {
    // 不引入 chrono：用 SystemTime 转 ISO-8601 秒级 UTC 字符串足够做量测时间戳
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    // 简易 civil-from-days（Howard Hinnant 算法）
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    format!("{year:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
            rem / 3600, (rem % 3600) / 60, rem % 60)
}
