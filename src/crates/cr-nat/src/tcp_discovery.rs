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

/// 发现报告。`discovered_port` 是给对端的信令字段；`eim` 只在 >=2 个 observer
/// 一致回写时为 true，单 observer 时是 `null`（不冒充 EIM 已证）。
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
    let got: Vec<&str> = observations
        .iter()
        .filter_map(|o| o.mapped_endpoint.as_deref())
        .collect();
    let (eim, discovered_endpoint, discovered_port, reason) = if got.is_empty() {
        (
            Some(false),
            None,
            None,
            "所有 observer 均未回写映射端点；无证据，不合成端口".to_string(),
        )
    } else if got.len() == 1 {
        (
            None,
            Some(got[0].to_string()),
            parse_port(got[0]),
            "仅 1 个 observer 回写：可得到映射端口，但未证 EIM（需 >=2 个独立观测点）".to_string(),
        )
    } else {
        let first = got[0];
        let all_same = got.iter().all(|m| *m == first);
        (
            Some(all_same),
            Some(first.to_string()),
            parse_port(first),
            if all_same {
                format!("{} 个独立 observer 回写同一映射端点 → EIM 成立", got.len())
            } else {
                format!(
                    "{} 个 observer 回写不一致 → 非 EIM，映射随目的变化，发现端口不可外推",
                    got.len()
                )
            },
        )
    };
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
    port.parse::<u16>().ok()
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
        let local: SocketAddr = format!("127.0.0.1:{}", free_port()).parse().unwrap();
        let report = discover_tcp_port(local, &[bind], Duration::from_secs(2));
        assert_eq!(report.observations.len(), 1);
        assert!(report.observations[0].mapped_endpoint.is_some());
        assert_eq!(report.eim, None, "单 observer 不得宣称 EIM");
        assert!(report.discovered_port.is_some());
        stop.store(true, Ordering::Relaxed);
        server.join().expect("join");
    }

    #[test]
    fn eim_requires_consistent_endpoints() {
        let a = Observation {
            observer: "a".into(),
            mapped_endpoint: Some("1.2.3.4:5000".into()),
            error: None,
        };
        let b_same = Observation {
            observer: "b".into(),
            mapped_endpoint: Some("1.2.3.4:5000".into()),
            error: None,
        };
        let b_diff = Observation {
            observer: "b".into(),
            mapped_endpoint: Some("1.2.3.4:6000".into()),
            error: None,
        };
        // 直接测判定逻辑：构造最小报告。
        let mk = |obs: Vec<Observation>| {
            let got: Vec<&str> = obs.iter().filter_map(|o| o.mapped_endpoint.as_deref()).collect();
            got.len() >= 2 && got.iter().all(|m| *m == got[0])
        };
        assert!(mk(vec![a.clone(), b_same]));
        assert!(!mk(vec![a.clone(), b_diff]));
    }

    #[test]
    fn unreachable_observer_is_recorded_as_error_not_success() {
        let local: SocketAddr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), free_port());
        // 一个几乎肯定无人监听的端口。
        let dead: SocketAddr = format!("127.0.0.1:{}", free_port()).parse().unwrap();
        let report = discover_tcp_port(local, &[dead], Duration::from_millis(400));
        assert_eq!(report.eim, Some(false));
        assert!(report.discovered_port.is_none());
        assert!(report.observations[0].error.is_some());
    }
}
