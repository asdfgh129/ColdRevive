//! 动态 TCP 映射端口发现（TCP-punch 的 STUN 语义前置）。
//!
//! 为什么需要它：真实打洞时，本地固定端口（如 `19910`）经出口 NAT 后**不一定**保持原
//! 端口。221 实测：从 `192.168.1.221:19910` 发出的 SYN 到两个独立公网观测点
//! （`82.157.68.159` / `119.29.150.254`）都被映射成同一个 `112.5.60.4:P`，但 `P` 与
//! `19910` 不同（见 `tests/ARCHIVE/evidence/tcp-eim-two-observers-tcpdump-20261124.json`）。
//! 让对端继续连 `112.5.60.4:19910` 就会 4 元组失配被 RST。正确做法是先让外部 observer
//! 看到「我从 19910 发出被映射成了什么」，再把那个端口作为**信令字段**交给对端。
//!
//! 本模块只做「发现 + 报告」，不改既有 TCP punch 判据、不宣称 S6/G2。
//! - observer 端：`serve_observer`，对每个入向连接回写 `CR-MAPPED <ip>:<port>`，其中
//!   `ip:port` 是 `accept()` 看到的源端点（即 NAT 映射后的公网端点）。
//! - 客户端：`discover_tcp_port`，从**固定本地端口**向每个 observer 发真 TCP 连接，
//!   读回映射端点；多个 observer 回写一致即判定 endpoint-independent mapping（EIM）。
//!
//! 判据严格：读不到/解析不出映射端点一律记 `error`，绝不合成「发现成功」。

use std::io::{self, BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::tcp_punch::connect_bound_blocking;

/// observer 回写行的前缀。客户端只认这个前缀，避免把 banner/日志误当成映射端点。
pub const MAP_PREFIX: &str = "CR-MAPPED ";

/// 单个 observer 的观测结果。
#[derive(Debug, Clone)]
pub struct Observation {
    pub observer: String,
    pub mapped_endpoint: Option<String>,
    pub error: Option<String>,
}

impl Observation {
    pub fn to_json(&self) -> String {
        format!(
            "{{\"observer\":{},\"mapped_endpoint\":{},\"error\":{}}}",
            json_str(&self.observer),
            jopt(&self.mapped_endpoint),
            jopt(&self.error)
        )
    }
}

/// 发现报告。`discovered_port` 是给对端的信令字段，只在「至少 1 个 observer
/// 成功且未被反证」时给出；>=2 个 observer 回写不一致时一律为 `None`，避免把
/// 不可外推的端口当信令。`eim` 只在 >=2 个 observer 一致回写时为 true，
/// 单 observer 时是 `null`（不冒充 EIM 已证）。
#[derive(Debug, Clone)]
pub struct DiscoveryReport {
    pub mode: &'static str,
    pub local: String,
    pub observations: Vec<Observation>,
    pub eim: Option<bool>,
    pub discovered_endpoint: Option<String>,
    pub discovered_port: Option<u16>,
    pub reason: String,
}

impl DiscoveryReport {
    pub fn to_json(&self) -> String {
        let obs: Vec<String> = self.observations.iter().map(|o| o.to_json()).collect();
        format!(
            "{{\"mode\":\"{}\",\"local\":{},\"observations\":[{}],\"eim\":{},\"discovered_endpoint\":{},\"discovered_port\":{},\"reason\":{},\"s6_status\":\"not_run\",\"g2_claim\":false,\"auto_pass\":false}}",
            self.mode,
            json_str(&self.local),
            obs.join(", "),
            self.eim.map(|b| b.to_string()).unwrap_or_else(|| "null".into()),
            jopt(&self.discovered_endpoint),
            self.discovered_port.map(|p| p.to_string()).unwrap_or_else(|| "null".into()),
            json_str(&self.reason)
        )
    }
}

/// 从 `local` 固定端口向每个 observer 发真 TCP 连接，读取它回写的映射端点。
///
/// 每个 observer 是一次独立连接；`SO_REUSEADDR` + 主动关闭使同一本地端口可重绑。
/// 判定：成功观测 >=2 且所有映射端点相同 → EIM=true；只有 1 个成功 → EIM=null
/// （「发现到了端口」但「未证 EIM」）；0 个成功 → EIM=false 且无端口。
pub fn discover_tcp_port(
    local: SocketAddr,
    observers: &[SocketAddr],
    timeout: Duration,
) -> DiscoveryReport {
    let mut observations = Vec::new();
    for obs in observers {
        observations.push(observe_one(local, *obs, timeout));
    }
    let (eim, discovered_endpoint, discovered_port, reason) = judge_observations(&observations);
    DiscoveryReport {
        mode: "discover-tcp-port",
        local: local.to_string(),
        observations,
        eim,
        discovered_endpoint,
        discovered_port,
        reason,
    }
}

/// 纯判定函数：把 observer 回写结果收敛为 EIM 结论与「可外推」的发现端口。
///
/// 与网络 I/O 解耦，便于用合成观测做定点测试。返回顺序为
/// `(eim, discovered_endpoint, discovered_port, reason)`：
/// - 0 个观测 / 全部失败 → `eim=false` 且无端口；
/// - 1 个成功 → `eim=null`，端口可用但未证 EIM；
/// - >=2 个成功且一致 → `eim=true`，端口可用；
/// - >=2 个成功但不一致 → `eim=false` 且**无端口**（映射随目的变化，不可外推）。
pub fn judge_observations(
    observations: &[Observation],
) -> (Option<bool>, Option<String>, Option<u16>, String) {
    let got: Vec<&str> = observations
        .iter()
        .filter_map(|o| o.mapped_endpoint.as_deref())
        .collect();
    if observations.is_empty() {
        return (
            Some(false),
            None,
            None,
            "未提供 observer；无证据，不合成端口".to_string(),
        );
    }
    if got.is_empty() {
        return (
            Some(false),
            None,
            None,
            "所有 observer 均未回写映射端点；无证据，不合成端口".to_string(),
        );
    }
    if got.len() == 1 {
        return (
            None,
            Some(got[0].to_string()),
            parse_port(got[0]),
            "仅 1 个 observer 回写：可得到映射端口，但未证 EIM（需 >=2 个独立观测点）".to_string(),
        );
    }
    let first = got[0];
    if !got.iter().all(|m| *m == first) {
        return (
            Some(false),
            None,
            None,
            format!(
                "{} 个 observer 回写不一致 → 非 EIM，映射随目的变化，发现端口不可外推",
                got.len()
            ),
        );
    }
    match parse_port(first) {
        Some(port) => (
            Some(true),
            Some(first.to_string()),
            Some(port),
            format!("{} 个独立 observer 回写同一映射端点 → EIM 成立", got.len()),
        ),
        None => (
            Some(false),
            None,
            None,
            format!("映射端点 {first} 无法解析出端口；不合成信令"),
        ),
    }
}

fn observe_one(local: SocketAddr, obs: SocketAddr, timeout: Duration) -> Observation {
    let tag = obs.to_string();
    let stream = match connect_bound_blocking(local, obs, timeout) {
        Ok(s) => s,
        Err(e) => {
            return Observation {
                observer: tag,
                mapped_endpoint: None,
                error: Some(format!("connect: {e}")),
            }
        }
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => Observation {
            observer: tag,
            mapped_endpoint: None,
            error: Some("observer 关闭连接且未回写任何内容".to_string()),
        },
        Ok(_) => {
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if let Some(rest) = trimmed.strip_prefix(MAP_PREFIX) {
                let rest = rest.trim();
                if parse_port(rest).is_some() {
                    Observation {
                        observer: tag,
                        mapped_endpoint: Some(rest.to_string()),
                        error: None,
                    }
                } else {
                    Observation {
                        observer: tag,
                        mapped_endpoint: None,
                        error: Some(format!("回写端点无法解析出端口: {rest}")),
                    }
                }
            } else {
                Observation {
                    observer: tag,
                    mapped_endpoint: None,
                    error: Some(format!("回写缺少 {MAP_PREFIX} 前缀: {trimmed}")),
                }
            }
        }
        Err(e) => Observation {
            observer: tag,
            mapped_endpoint: None,
            error: Some(format!("read: {e}")),
        },
    }
}

/// observer：监听 `bind`，对每个入向连接回写它看到的源端点，直到 `stop` 置位。
///
/// 返回成功回写的次数。写失败只记数、不影响其它连接；`accept()` 用 200ms 轮询超时
/// 以便响应 stop。
pub fn serve_observer(bind: SocketAddr, stop: Arc<AtomicBool>) -> io::Result<ObserverStats> {
    let listener = TcpListener::bind(bind)?;
    listener.set_nonblocking(true)?;
    let stats = ObserverStats::default();
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((mut stream, peer)) => {
                let line = format!("{MAP_PREFIX}{}\n", peer);
                match stream.write_all(line.as_bytes()).and_then(|_| stream.flush()) {
                    Ok(()) => {
                        stats.served.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(_) => {
                        stats.write_failed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(stats)
}

#[derive(Debug, Default)]
pub struct ObserverStats {
    pub served: std::sync::atomic::AtomicU64,
    pub write_failed: std::sync::atomic::AtomicU64,
}

/// 按 `--serve-secs` 自动停止的便捷包装。
pub fn serve_observer_for(bind: SocketAddr, secs: u64) -> io::Result<(ObserverStats, u64)> {
    let stop = Arc::new(AtomicBool::new(false));
    let stopper = stop.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(secs.max(1)));
        stopper.store(true, Ordering::Relaxed);
    });
    let started = Instant::now();
    let stats = serve_observer(bind, stop)?;
    Ok((stats, started.elapsed().as_millis() as u64))
}

fn parse_port(endpoint: &str) -> Option<u16> {
    let (_, port) = endpoint.rsplit_once(':')?;
    let port = port.parse::<u16>().ok()?;
    // 端口 0 不是可连接的映射端点；观测到它等价于没有可用端口。
    if port == 0 {
        None
    } else {
        Some(port)
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn jopt(s: &Option<String>) -> String {
    s.as_deref().map(json_str).unwrap_or_else(|| "null".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, TcpStream};

    fn free_port() -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let p = l.local_addr().expect("addr").port();
        drop(l);
        p
    }

    fn free_port_except(used: &[u16]) -> u16 {
        loop {
            let p = free_port();
            if !used.contains(&p) {
                return p;
            }
        }
    }

    /// 单连接 observer：按测试 intent 回写任意一行，用来覆盖异常输入路径。
    fn one_shot_observer(line: &'static str) -> (SocketAddr, std::thread::JoinHandle<()>) {
        let bind: SocketAddr = format!("127.0.0.1:{}", free_port()).parse().unwrap();
        let handle = std::thread::spawn(move || {
            let listener = TcpListener::bind(bind).expect("bind");
            let (mut stream, _) = listener.accept().expect("accept");
            stream.write_all(line.as_bytes()).expect("write");
            stream.flush().expect("flush");
        });
        (bind, handle)
    }

    fn observation(endpoint: Option<&str>) -> Observation {
        Observation {
            observer: "test-observer".into(),
            mapped_endpoint: endpoint.map(str::to_string),
            error: None,
        }
    }

    #[test]
    fn observer_writes_back_source_endpoint() {
        let port = free_port();
        let bind: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let server = std::thread::spawn(move || {
            // 只服务到第一次写入后由客户端置 stop。
            serve_observer(bind, s2).expect("serve")
        });
        let mut client = TcpStream::connect(bind).expect("connect");
        let mut line = String::new();
        BufReader::new(&mut client)
            .read_line(&mut line)
            .expect("read");
        assert!(
            line.starts_with(MAP_PREFIX),
            "回写必须带前缀，实际: {line:?}"
        );
        let mapped = line.trim_end().trim_start_matches(MAP_PREFIX);
        assert!(parse_port(mapped).is_some(), "回写端点必须含端口: {mapped}");
        stop.store(true, Ordering::Relaxed);
        server.join().expect("join");
    }

    #[test]
    fn discover_single_observer_reports_port_but_not_eim() {
        let port = free_port();
        let bind: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let server = std::thread::spawn(move || serve_observer(bind, s2).expect("serve"));
        // 回环上 source 就是 local，映射端点 == 本地端点。
        let local_port = free_port_except(&[port]);
        let local: SocketAddr = format!("127.0.0.1:{local_port}").parse().unwrap();
        let report = discover_tcp_port(local, &[bind], Duration::from_secs(2));
        assert_eq!(report.observations.len(), 1);
        assert!(report.observations[0].mapped_endpoint.is_some());
        assert_eq!(report.eim, None, "单 observer 不得宣称 EIM");
        assert_eq!(report.discovered_port, Some(local_port));
        assert!(report.to_json().contains("\"eim\":null"));
        stop.store(true, Ordering::Relaxed);
        server.join().expect("join");
    }

    #[test]
    fn two_loopback_observers_report_same_mapped_endpoint() {
        // 回环不经过 NAT：这里验证的是「双观测 + 一致判定」控制路径，
        // 不冒充真实 NAT 的 EIM 证据（公网证据另见 tcp-eim-two-observers-tcpdump）。
        let p1 = free_port();
        let p2 = free_port_except(&[p1]);
        let local_port = free_port_except(&[p1, p2]);
        let b1: SocketAddr = format!("127.0.0.1:{p1}").parse().unwrap();
        let b2: SocketAddr = format!("127.0.0.1:{p2}").parse().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let s1 = stop.clone();
        let s2 = stop.clone();
        let h1 = std::thread::spawn(move || serve_observer(b1, s1).expect("serve"));
        let h2 = std::thread::spawn(move || serve_observer(b2, s2).expect("serve"));

        let local: SocketAddr = format!("127.0.0.1:{local_port}").parse().unwrap();
        let report = discover_tcp_port(local, &[b1, b2], Duration::from_secs(2));
        assert_eq!(report.eim, Some(true), "observations={:?}", report.observations);
        assert_eq!(report.discovered_port, Some(local_port));
        assert_eq!(report.observations.len(), 2);
        assert!(report.observations.iter().all(|o| o.error.is_none()));

        stop.store(true, Ordering::Relaxed);
        h1.join().expect("join h1");
        h2.join().expect("join h2");
    }

    #[test]
    fn judge_requires_two_consistent_observers() {
        let same = "112.5.60.4:20811";
        let diff = "112.5.60.4:20812";

        let (eim, ep, port, reason) = judge_observations(&[]);
        assert_eq!(eim, Some(false));
        assert!(ep.is_none() && port.is_none());
        assert!(reason.contains("未提供 observer"));

        let (eim, ep, port, _) = judge_observations(&[observation(None), observation(None)]);
        assert_eq!(eim, Some(false));
        assert!(ep.is_none() && port.is_none());

        let (eim, ep, port, _) = judge_observations(&[observation(Some(same))]);
        assert_eq!(eim, None, "单 observer 不得宣称 EIM");
        assert_eq!(ep.as_deref(), Some(same));
        assert_eq!(port, Some(20811));

        let (eim, ep, port, _) =
            judge_observations(&[observation(Some(same)), observation(Some(same))]);
        assert_eq!(eim, Some(true));
        assert_eq!(ep.as_deref(), Some(same));
        assert_eq!(port, Some(20811));

        let (eim, ep, port, reason) =
            judge_observations(&[observation(Some(same)), observation(Some(diff))]);
        assert_eq!(eim, Some(false));
        assert!(ep.is_none(), "非 EIM 不得给出可外推端点");
        assert!(port.is_none(), "非 EIM 不得给出可外推端口");
        assert!(reason.contains("不一致"));
    }

    #[test]
    fn judge_rejects_unparsable_mapped_endpoint() {
        let bad = "1.2.3.4:not-a-port";
        let (eim, ep, port, reason) =
            judge_observations(&[observation(Some(bad)), observation(Some(bad))]);
        assert_eq!(eim, Some(false));
        assert!(ep.is_none() && port.is_none());
        assert!(reason.contains("无法解析"));
    }

    #[test]
    fn parse_port_rejects_zero_and_garbage() {
        assert_eq!(parse_port("112.5.60.4:20811"), Some(20811));
        assert_eq!(parse_port("112.5.60.4:0"), None);
        assert_eq!(parse_port("112.5.60.4:"), None);
        assert_eq!(parse_port("112.5.60.4:65536"), None);
        assert_eq!(parse_port("no-colon"), None);
    }

    #[test]
    fn malformed_observer_lines_are_errors_not_success() {
        // 覆盖无前缀、坏端口、空回写三条失败路径；任何一条都不许合成 discovered_port。
        for line in ["NOT-MAPPED 112.5.60.4:20811", "CR-MAPPED 112.5.60.4:oops", ""] {
            let (bind, server) = one_shot_observer(line);
            let local: SocketAddr =
                SocketAddr::new(Ipv4Addr::LOCALHOST.into(), free_port_except(&[bind.port()]));
            let report = discover_tcp_port(local, &[bind], Duration::from_secs(2));
            assert_eq!(report.eim, Some(false), "line={line:?}");
            assert!(report.discovered_port.is_none(), "line={line:?}");
            assert!(report.observations[0].error.is_some(), "line={line:?}");
            server.join().expect("join");
        }
    }

    #[test]
    fn unreachable_observer_is_recorded_as_error_not_success() {
        let local: SocketAddr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), free_port());
        let dead: SocketAddr = format!("127.0.0.1:{}", free_port_except(&[local.port()]))
            .parse()
            .unwrap();
        let report = discover_tcp_port(local, &[dead], Duration::from_millis(400));
        assert_eq!(report.eim, Some(false));
        assert!(report.discovered_port.is_none());
        assert!(report.observations[0].error.is_some());
    }

    #[test]
    fn report_json_keeps_gate_constants() {
        let observations = vec![
            observation(Some("112.5.60.4:20811")),
            observation(Some("112.5.60.4:20811")),
        ];
        let (eim, ep, port, reason) = judge_observations(&observations);
        let report = DiscoveryReport {
            mode: "discover-tcp-port",
            local: "192.168.1.221:19910".into(),
            observations,
            eim,
            discovered_endpoint: ep,
            discovered_port: port,
            reason,
        };
        let json = report.to_json();
        assert!(json.contains("\"eim\":true"));
        assert!(json.contains("\"s6_status\":\"not_run\""));
        assert!(json.contains("\"g2_claim\":false"));
        assert!(json.contains("\"auto_pass\":false"));
    }
}
