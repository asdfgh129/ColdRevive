//! coldrevive-relay — M2/W3.4 生产中继服务端 + W3.8 多实例（std-only）。
//!
//! 与业务无关的 TCP 帧路由器：把 `RELAY` 帧投递给已 `REGISTER` 的目标连接，
//! 不解析唤醒协议、不碰 UDP、不合成任何 `DELIVERED` 应答。帧路由实现见
//! `cr_nat::relay_server`，持有者侧订阅见 `coldrevived/src/relay_sub.rs`。
//!
//! W3.8 起支持多实例互联：`--peer <ip:port>` 让本实例与另一个实例互认并同步注册表快照，
//! 目标不在本地时按快照归属**单跳转发**。这样某个实例宕机不再等于「它的节点全部失联」。
//!
//! 用法：
//!   coldrevive-relay --listen 0.0.0.0:7100 [--peer 1.2.3.4:7100]... \
//!                    [--instance-id relay-a] [--stats-file <path>] [--max-frame 1048576]
//!
//! 启动后首行输出 `RELAY_LISTEN=<实际地址>`（`:0` 时回显内核分配的端口），
//! 便于验收 harness 取真实端口。`--stats-file` 每 200 ms 落一份机读计数，
//! 供测试读取**真实**投递 / 丢弃 / 转发次数，不参与业务判定。

use cr_nat::relay_server::{parse_instance_id, RelayServer};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }
    let opt = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let opt_all = |name: &str| -> Vec<String> {
        args.iter()
            .enumerate()
            .filter(|(_, a)| *a == name)
            .filter_map(|(i, _)| args.get(i + 1).cloned())
            .collect()
    };
    let listen = opt("--listen").unwrap_or_else(|| "127.0.0.1:7000".to_string());
    let max_frame = opt("--max-frame")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(cr_nat::relay_server::DEFAULT_MAX_FRAME);
    let stats_file = opt("--stats-file").map(PathBuf::from);
    let peers = opt_all("--peer");
    let instance = match opt("--instance-id") {
        Some(text) => match parse_instance_id(&text) {
            Ok(id) => Some(id),
            Err(e) => {
                eprintln!("--instance-id 非法: {}", e);
                std::process::exit(2);
            }
        },
        None => None,
    };

    let mut server = match RelayServer::bind(&listen) {
        Ok(s) => s.with_max_frame(max_frame),
        Err(e) => {
            eprintln!("中继服务启动失败: {}", e);
            std::process::exit(2);
        }
    };
    for p in &peers {
        server = server.with_peer(p);
    }
    if let Some(id) = instance {
        server = server.with_instance_id(id);
    }
    let actual = match server.local_addr() {
        Ok(a) => a.to_string(),
        Err(e) => {
            eprintln!("读取监听地址失败: {}", e);
            std::process::exit(2);
        }
    };
    let handle = server.spawn();
    let instance_label = String::from_utf8_lossy(&handle.instance_id())
        .trim_end_matches('\0')
        .to_string();

    println!("RELAY_LISTEN={}", actual);
    println!("RELAY_MAX_FRAME={}", max_frame);
    println!("RELAY_INSTANCE={}", instance_label);
    println!("RELAY_PEERS={}", peers.join(","));
    println!(
        "[coldrevive-relay] 实例 {} 帧路由监听 {}，同侪 {} 个；无业务解析、不合成应答",
        instance_label,
        actual,
        peers.len()
    );

    if let Some(path) = stats_file {
        let listen_label = actual.clone();
        let inst = instance_label.clone();
        thread::spawn(move || loop {
            let s = handle.stats();
            let body = format!(
                "{{\"listen\":\"{}\",\"instance\":\"{}\",\"connections\":{},\"registered\":{},\"delivered\":{},\"dropped\":{},\"rejected\":{},\"peers\":{},\"peer_nodes\":{},\"forwarded\":{},\"forward_dropped\":{}}}",
                listen_label,
                inst,
                s.connections,
                s.registered,
                s.delivered,
                s.dropped,
                s.rejected,
                s.peers,
                s.peer_nodes,
                s.forwarded,
                s.forward_dropped
            );
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&path, body);
            thread::sleep(Duration::from_millis(200));
        });
    }
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

fn print_usage() {
    println!("coldrevive-relay — ColdRevive M2/W3.4 生产中继服务端（W3.8 多实例）");
    println!();
    println!("用法: coldrevive-relay --listen <ip:port> [--peer <ip:port>]... \\");
    println!("                        [--instance-id <label>] [--stats-file <path>] [--max-frame <bytes>]");
    println!();
    println!("  --listen       监听地址，默认 127.0.0.1:7000；端口 0 由内核分配并在首行回显");
    println!("  --peer         同侪实例地址，可重复；自动重拨，断线自愈");
    println!("  --instance-id  固定本实例 id（1..=16 可打印 ASCII），默认由地址+时间派生");
    println!("  --stats-file   每 200ms 写一份机读计数 JSON");
    println!("                 （connections/registered/delivered/dropped/rejected/");
    println!("                   peers/peer_nodes/forwarded/forward_dropped）");
    println!("  --max-frame    单帧最大负载字节数，默认 1048576；超限帧直接断开连接");
    println!();
    println!("帧协议（大端）：1B kind + 16B node id + 4B 长度 + 负载");
    println!("  0x01 REGISTER(负载=16B node id) -> ACK(0x04)");
    println!("  0x02 RELAY(负载=16B 目标 id + 消息) -> 目标收到 DELIVERED(0x03, node=来源 id)");
    println!("  0x05 PEER_HELLO(负载=16B 呼叫方实例 id) -> ACK(node=应答方实例 id)");
    println!("  0x06 PEER_SYNC(负载=n×16B 本地节点 id，整份覆盖)");
    println!("  0x07 FORWARD(负载=16B 来源 + 16B 目标 + 消息，只投递不二次转发)");
    println!();
    println!("注意：本服务只做帧路由，不构成 S6 四类 NAT 可达率实测，S6.status 仍为 not_run。");
}
