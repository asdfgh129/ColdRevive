//! coldrevive-cli — T0 CLI v1 契约的薄客户端。
//!
//! 调用形式固定为 `coldrevive-cli --json --node <A|B> <command> [args...]`，
//! stdout 只输出**一个** JSON 对象；非 0 退出 / 非法 JSON / 字段缺失由
//! `t0_cli.py` 判为「观测不可用」，绝不回填默认值。
//! 节点地址来自 `COLDREVIVE_T0_CONFIG`（由 T0 harness 在 real 模式下写入）。

use cr_core::{Config, Json};
use std::env;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(json) => { println!("{}", json.dump()); ExitCode::SUCCESS }
        Err(e) => { eprintln!("coldrevive-cli: {}", e); ExitCode::FAILURE }
    }
}

fn run() -> Result<Json, String> {
    let argv: Vec<String> = env::args().skip(1).collect();
    if argv.len() < 4 || argv[0] != "--json" || argv[1] != "--node" {
        return Err("用法: coldrevive-cli --json --node <A|B> <command> [args...]".into());
    }
    let node = argv[2].clone();
    let command = argv[3].clone();
    let args: Vec<Json> = argv[4..].iter().map(|a| Json::s(a)).collect();

    let cfg_path = env::var("COLDREVIVE_T0_CONFIG")
        .map_err(|_| "未设置 COLDREVIVE_T0_CONFIG".to_string())?;
    let cfg = Config::load(&PathBuf::from(&cfg_path))?;
    let port = cfg.node_http(&node)
        .ok_or_else(|| format!("配置中无节点 {}", node))?;

    let body = Json::obj(vec![
        ("node", Json::s(&node)),
        ("command", Json::s(&command)),
        ("args", Json::arr(args)),
    ]).dump();

    let host = cfg.host(&node);
    let mut stream = TcpStream::connect(format!("{}:{}", host, port))
        .map_err(|e| format!("连接 {}:{} 失败: {}", host, port, e))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(130)))
        .map_err(|e| e.to_string())?;
    let req = format!(
        "POST /cli HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        host, body.as_bytes().len(), body
    );
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw);
    let split = text.find("\r\n\r\n").ok_or_else(|| "HTTP 响应无头部结束符".to_string())?;
    let head = &text[..split];
    if !head.starts_with("HTTP/1.1 200") {
        return Err(format!("守护进程返回: {}", head.lines().next().unwrap_or("")));
    }
    Json::parse(&text[split + 4..]).map_err(|e| format!("响应不是合法 JSON: {}", e))
}
