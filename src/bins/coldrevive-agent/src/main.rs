//! coldrevive-agent — W6.6 三进程模型的**无 GUI 看护内核**（M3 增量）。
//!
//! 边界（照写，不许放宽）：
//! - 本增量只做「启动一个子进程 + 存活巡检 + 异常退出重启 + `/health` 就绪日志」，
//!   **不是**托盘、浏览器捕获、UPnP 续租，也不是三进程全量交付。S6 已实测本机
//!   UPnP IGD / NAT-PMP 均不可用，相关功能要另找环境。
//! - 只依赖 std：不引 Tauri / tray-icon / reqwest，不碰公网，不发 UDP。
//! - `cr-guard` 握手拦截在 `coldrevived` 进程内，不是本 agent 监管的独立进程。
//! - 健康探测只读 `127.0.0.1` 上的 `/health`；探测失败不自行判定「 daemon 死了」，
//!   重启只以子进程的**异常退出**为触发，避免把慢启动误当崩溃。
//! - 无托盘 GUI 依赖，需 GUI 的项见 `docs/06-plan/M3-REMAINING-BLOCKERS-2026-11-27.md`。

use std::env;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

const DEFAULT_HEALTH_URL: &str = "http://127.0.0.1:16800/health";
const DEFAULT_POLL_MS: u64 = 1000;
const DEFAULT_BACKOFF_MS: u64 = 500;
const DEFAULT_MAX_RESTARTS: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Options {
    child: String,
    child_args: Vec<String>,
    health_url: String,
    poll_ms: u64,
    backoff_ms: u64,
    max_restarts: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            child: String::new(),
            child_args: Vec::new(),
            health_url: DEFAULT_HEALTH_URL.to_string(),
            poll_ms: DEFAULT_POLL_MS,
            backoff_ms: DEFAULT_BACKOFF_MS,
            max_restarts: DEFAULT_MAX_RESTARTS,
        }
    }
}

fn take_value(args: &[String], i: &mut usize, flag: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{flag} 缺少取值"))
}

fn parse_num<T: std::str::FromStr>(args: &[String], i: &mut usize, flag: &str) -> Result<T, String> {
    take_value(args, i, flag)?
        .parse::<T>()
        .map_err(|_| format!("{flag} 必须是数字"))
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut opt = Options::default();
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--child" => opt.child = take_value(args, &mut i, "--child")?,
            "--child-arg" => opt.child_args.push(take_value(args, &mut i, "--child-arg")?),
            "--health-url" => opt.health_url = take_value(args, &mut i, "--health-url")?,
            "--poll-ms" => opt.poll_ms = parse_num(args, &mut i, "--poll-ms")?,
            "--backoff-ms" => opt.backoff_ms = parse_num(args, &mut i, "--backoff-ms")?,
            "--max-restarts" => opt.max_restarts = parse_num(args, &mut i, "--max-restarts")?,
            other => return Err(format!("未知参数 {other}")),
        }
        i += 1;
    }
    if opt.child.is_empty() {
        return Err("必须给 --child <可执行文件路径>".to_string());
    }
    if opt.poll_ms == 0 {
        return Err("--poll-ms 必须 > 0".to_string());
    }
    if !opt.health_url.starts_with("http://") {
        return Err("--health-url 只接受 http:// 前缀（本 agent 不验证 TLS）".to_string());
    }
    Ok(opt)
}

fn usage() {
    println!("coldrevive-agent — 无 GUI 守护进程看护内核");
    println!("用法: coldrevive-agent --child <path> [--child-arg <arg>]... [选项]");
    println!("  --health-url URL     只读健康探测（默认 {DEFAULT_HEALTH_URL}）");
    println!("  --poll-ms N          存活巡检间隔（默认 {DEFAULT_POLL_MS}，必须 > 0）");
    println!("  --backoff-ms N       重启退避（默认 {DEFAULT_BACKOFF_MS}）");
    println!("  --max-restarts N     异常退出最多重启次数（默认 {DEFAULT_MAX_RESTARTS}）");
    println!("退出码: 0 干净退出 / 2 启动或巡检错误 / 3 重启预算耗尽");
}

/// 只解析 http:// 的 authority + path，不做 DNS 之外的任何网络动作。
fn health_endpoint(url: &str) -> Result<(String, String), String> {
    let rest = url.strip_prefix("http://").ok_or_else(|| "只支持 http://".to_string())?;
    let (authority, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return Err("缺少主机".to_string());
    }
    Ok((authority.to_string(), path.to_string()))
}

/// 纯函数：把原始 HTTP 响应判为「健康」。200 + `{"status":"ok"}`，两者缺一不可。
fn health_payload_ok(raw: &[u8]) -> bool {
    let head_end = raw.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|idx| idx + 4)
        .unwrap_or(raw.len());
    let head = String::from_utf8_lossy(&raw[..head_end.min(raw.len())]);
    let status_ok = head.starts_with("HTTP/1.0 200") || head.starts_with("HTTP/1.1 200");
    let body = String::from_utf8_lossy(&raw[head_end.min(raw.len())..]);
    status_ok && body.contains("\"status\":\"ok\"")
}

fn probe_health(url: &str) -> bool {
    let (authority, path) = match health_endpoint(url) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let addrs: Vec<SocketAddr> = match authority.to_socket_addrs() {
        Ok(it) => it.collect(),
        Err(_) => return false,
    };
    let Some(addr) = addrs.first() else { return false };
    let Ok(mut stream) = TcpStream::connect_timeout(addr, Duration::from_millis(300)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(300)));
    let req = format!("GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n");
    if stream.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    health_payload_ok(&buf)
}

/// 纯函数：只有**异常退出**且还有预算才重启。干净退出视为操作员主动停止。
fn should_restart(clean_exit: bool, restarts: u32, max_restarts: u32) -> bool {
    !clean_exit && restarts < max_restarts
}

fn supervise(opt: &Options) -> i32 {
    let mut restarts = 0u32;
    let mut health_logged = false;
    loop {
        let mut child = match Command::new(&opt.child)
            .args(&opt.child_args)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("agent: 启动子进程失败: {e}");
                return 2;
            }
        };
        println!("agent: child spawned pid={} restarts={}", child.id(), restarts);

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let clean = status.success();
                    println!("agent: child exited code={} clean={}", status.code().unwrap_or(-1), clean);
                    if !should_restart(clean, restarts, opt.max_restarts) {
                        if clean {
                            println!("agent: 干净退出，不重启");
                            return 0;
                        }
                        eprintln!("agent: 重启预算耗尽 restarts={} max={}", restarts, opt.max_restarts);
                        return 3;
                    }
                    restarts += 1;
                    println!("agent: restart {} after {}ms", restarts, opt.backoff_ms);
                    thread::sleep(Duration::from_millis(opt.backoff_ms));
                    break;
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("agent: 巡检子进程失败: {e}");
                    return 2;
                }
            }
            if !health_logged && probe_health(&opt.health_url) {
                println!("agent: health ready url={}", opt.health_url);
                health_logged = true;
            }
            thread::sleep(Duration::from_millis(opt.poll_ms));
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        usage();
        return;
    }
    let opt = match parse_args(&args) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("agent: 参数错误: {e}");
            usage();
            std::process::exit(2);
        }
    };
    std::process::exit(supervise(&opt));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn parse_args_defaults_and_required_child() {
        let opt = parse_args(&argv(&["--child", "coldrevived"])).expect("parse");
        assert_eq!(opt.child, "coldrevived");
        assert_eq!(opt.health_url, DEFAULT_HEALTH_URL);
        assert_eq!(opt.poll_ms, DEFAULT_POLL_MS);
        assert_eq!(opt.backoff_ms, DEFAULT_BACKOFF_MS);
        assert_eq!(opt.max_restarts, DEFAULT_MAX_RESTARTS);
        assert!(parse_args(&argv(&[])).is_err());
        assert!(parse_args(&argv(&["--child", "x", "--poll-ms", "0"])).is_err());
        assert!(parse_args(&argv(&["--child", "x", "--health-url", "https://h/"])).is_err());
    }

    #[test]
    fn parse_args_keeps_child_args_in_order() {
        let opt = parse_args(&argv(&[
            "--child", "python3", "--child-arg", "-c", "--child-arg", "pass",
            "--max-restarts", "1", "--poll-ms", "25", "--backoff-ms", "5",
        ])).expect("parse");
        assert_eq!(opt.child, "python3");
        assert_eq!(opt.child_args, vec!["-c", "pass"]);
        assert_eq!(opt.max_restarts, 1);
        assert_eq!(opt.poll_ms, 25);
        assert_eq!(opt.backoff_ms, 5);
    }

    #[test]
    fn restart_only_on_abnormal_exit_with_budget() {
        assert!(!should_restart(true, 0, 3));
        assert!(should_restart(false, 0, 3));
        assert!(should_restart(false, 2, 3));
        assert!(!should_restart(false, 3, 3));
        assert!(!should_restart(true, 9, 3));
    }

    #[test]
    fn health_payload_requires_200_and_ok_status() {
        let ok = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"status\":\"ok\",\"node\":\"A\"}";
        assert!(health_payload_ok(ok));
        let bad_status = b"HTTP/1.1 500 Internal Server Error\r\n\r\n{\"status\":\"ok\"}";
        assert!(!health_payload_ok(bad_status));
        let bad_body = b"HTTP/1.1 200 OK\r\n\r\n{\"status\":\"starting\"}";
        assert!(!health_payload_ok(bad_body));
    }

    #[test]
    fn health_endpoint_parses_loopback_authority() {
        let (authority, path) = health_endpoint("http://127.0.0.1:16800/health").expect("url");
        assert_eq!(authority, "127.0.0.1:16800");
        assert_eq!(path, "/health");
        let (authority, path) = health_endpoint("http://127.0.0.1:16800").expect("url");
        assert_eq!(authority, "127.0.0.1:16800");
        assert_eq!(path, "/");
    }
}
