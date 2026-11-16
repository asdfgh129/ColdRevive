//! coldrevive-relay — M2/W3.4 生产中继服务端 + W3.8 多实例 + W3.9 传输安全（std-only）。
//!
//! 与业务无关的 TCP 帧路由器：把 `RELAY` 帧投递给已 `REGISTER` 的目标连接，
//! 不解析唤醒协议、不碰 UDP、不合成任何 `DELIVERED` 应答。帧路由实现见
//! `cr_nat::relay_server`，持有者侧订阅见 `coldrevived/src/relay_sub.rs`。
//!
//! W3.8 起支持多实例互联：`--peer <ip:port>` 让本实例与另一个实例互认并同步注册表快照，
//! 目标不在本地时按快照归属**单跳转发**。这样某个实例宕机不再等于「它的节点全部失联」。
//!
//! W3.9 起可放到公网：
//! - `--auth-key`（或环境变量 `CR_RELAY_KEY`、`--auth-key-file`）启用 PSK 双向握手 +
//!   逐帧加密，**绑非回环地址时没有密钥会拒绝启动**（`--allow-insecure` 才放行明文）；
//! - `--rate` / `--burst` 每连接令牌桶、`--max-conns` 并发上限、`--idle-timeout` 空闲回收、
//!   `--max-nodes` 注册表上限。
//!
//! 用法：
//!   coldrevive-relay --listen 0.0.0.0:7100 --auth-key <64hex|file:/path> \
//!                    [--peer 1.2.3.4:7100]... [--instance-id relay-a] \
//!                    [--stats-file <path>] [--max-frame 1048576]
//!
//! 启动后首行输出 `RELAY_LISTEN=<实际地址>`（`:0` 时回显内核分配的端口），
//! `RELAY_AUTH=<psk|none>` 与 `RELAY_KEY_FP=<指纹>` 供人工核对两端是否同一把钥匙。
//! `--stats-file` 每 200 ms 落一份机读计数，供测试读取**真实**投递/丢弃/转发/拒绝次数。

use cr_nat::relay_server::{parse_instance_id, printable_instance, Limits, RelayServer};
use cr_nat::secure;
use std::env;
use std::fs;
use std::net::ToSocketAddrs;
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
    let flag = |name: &str| args.iter().any(|a| a == name);

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
    let allow_insecure = flag("--allow-insecure");

    // ---- 传输层密钥 ----
    let key_spec = opt("--auth-key")
        .or_else(|| opt("--auth-key-file").map(|p| format!("file:{}", p)))
        .or_else(|| env::var("CR_RELAY_KEY").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let psk = match &key_spec {
        Some(spec) => match secure::parse_psk(spec) {
            Ok(k) => Some(k),
            Err(e) => {
                eprintln!("--auth-key 非法: {}", e);
                std::process::exit(2);
            }
        },
        None => None,
    };

    // ---- 公网策略：没密钥就不许绑非回环地址 ----
    let loopback = is_loopback_listen(&listen);
    if psk.is_none() && !loopback && !allow_insecure {
        eprintln!("拒绝启动：{} 是非回环地址，但没有配置 PSK。", listen);
        eprintln!("  公网暴露一个无认证的中继，等于给任何人一个免费的消息转发器，");
        eprintln!("  而且注册表可以被随意污染。三种选择：");
        eprintln!("  1) --auth-key <64 位十六进制>  或  --auth-key-file <path>  或环境变量 CR_RELAY_KEY");
        eprintln!("  2) 先用 --listen 127.0.0.1:7100 在本机验证");
        eprintln!("  3) 确实只想做实验：加 --allow-insecure（会打印明文警告）");
        std::process::exit(2);
    }
    if psk.is_none() {
        eprintln!("[coldrevive-relay] 警告：未配置 PSK，传输为**明文**。仅供回环/实验。");
    }

    // ---- 限额 ----
    let mut limits = Limits::default();
    if let Some(v) = opt("--rate").and_then(|s| s.parse::<u32>().ok()) {
        limits.rate_per_sec = v.max(1);
    }
    if let Some(v) = opt("--burst").and_then(|s| s.parse::<u32>().ok()) {
        limits.burst = v.max(1);
    }
    if let Some(v) = opt("--idle-timeout").and_then(|s| s.parse::<u64>().ok()) {
        limits.idle = if v == 0 { Duration::ZERO } else { Duration::from_secs(v) };
    }
    if let Some(v) = opt("--max-conns").and_then(|s| s.parse::<usize>().ok()) {
        limits.max_conns = v.max(1);
    }
    if let Some(v) = opt("--max-nodes").and_then(|s| s.parse::<usize>().ok()) {
        limits.max_nodes = v.max(1);
    }
    if let Some(v) = opt("--handshake-timeout").and_then(|s| s.parse::<u64>().ok()) {
        limits.handshake_timeout = Duration::from_secs(v.max(1));
    }

    let mut server = match RelayServer::bind(&listen) {
        Ok(s) => s
            .with_max_frame(max_frame)
            .with_psk(psk)
            .with_allow_insecure(allow_insecure || loopback)
            .with_limits(limits),
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
    // 派生 id 的哈希部分可能含控制字节：计数文件是 JSON，裸控制字符会让它整体解析失败。
    let instance_label = printable_instance(&handle.instance_id());
    let auth_label = if psk.is_some() { "psk" } else { "none" };
    let fp = psk.as_ref().map(secure::fingerprint).unwrap_or_default();

    println!("RELAY_LISTEN={}", actual);
    println!("RELAY_MAX_FRAME={}", max_frame);
    println!("RELAY_INSTANCE={}", instance_label);
    println!("RELAY_PEERS={}", peers.join(","));
    println!("RELAY_AUTH={}", auth_label);
    println!("RELAY_KEY_FP={}", fp);
    println!("RELAY_LIMITS=rate:{}/s,burst:{},idle:{}s,max_conns:{},max_nodes:{}",
             limits.rate_per_sec, limits.burst, limits.idle.as_secs(),
             limits.max_conns, limits.max_nodes);
    println!(
        "[coldrevive-relay] 实例 {} 帧路由监听 {}，同侪 {} 个；认证={}；无业务解析、不合成应答",
        instance_label,
        actual,
        peers.len(),
        if psk.is_some() { "PSK（加密）" } else { "无（明文）" }
    );
    if let Some(k) = &psk {
        println!("[coldrevive-relay] 密钥指纹 {}（两端必须一致）", secure::fingerprint(k));
    }

    if let Some(path) = stats_file {
        let listen_label = actual.clone();
        let inst = instance_label.clone();
        let auth = auth_label.to_string();
        let fp = fp.clone();
        thread::spawn(move || loop {
            let s = handle.stats();
            let body = format!(
                "{{\"listen\":\"{}\",\"instance\":\"{}\",\"auth\":\"{}\",\"key_fp\":\"{}\",\
                 \"connections\":{},\"registered\":{},\"delivered\":{},\"dropped\":{},\
                 \"rejected\":{},\"peers\":{},\"peer_nodes\":{},\"forwarded\":{},\
                 \"forward_dropped\":{},\"secure_handshakes\":{},\"secure_rejected\":{},\
                 \"bad_tag\":{},\"rate_limited\":{},\"idle_closed\":{},\"conn_cap_rejected\":{},\
                 \"declared_updates\":{},\"declared_direct\":{},\"declared_relay_only\":{},\
                 \"declared_unknown\":{},\"reach_lookups\":{}}}",
                listen_label,
                inst,
                auth,
                fp,
                s.connections,
                s.registered,
                s.delivered,
                s.dropped,
                s.rejected,
                s.peers,
                s.peer_nodes,
                s.forwarded,
                s.forward_dropped,
                s.secure_handshakes,
                s.secure_rejected,
                s.bad_tag,
                s.rate_limited,
                s.idle_closed,
                s.conn_cap_rejected,
                s.declared_updates,
                s.declared_direct,
                s.declared_relay_only,
                s.declared_unknown,
                s.reach_lookups
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

/// 判断监听地址是否只在本机可见（用于「无密钥也允许」的策略）。
fn is_loopback_listen(listen: &str) -> bool {
    let host = if let Some(rest) = listen.strip_prefix('[') {
        // [::1]:7100
        rest.split(']').next().unwrap_or(rest).to_string()
    } else {
        listen.rsplit_once(':').map(|(h, _)| h).unwrap_or(listen).to_string()
    };
    if host.is_empty() || host == "localhost" {
        return host == "localhost";
    }
    use std::net::IpAddr;
    match host.parse::<IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => {
            // 主机名：解析后全部是回环才算回环。
            let probe = format!("{}:0", host);
            match probe.to_socket_addrs() {
                Ok(addrs) => {
                    let v: Vec<_> = addrs.collect();
                    !v.is_empty() && v.iter().all(|a| a.ip().is_loopback())
                }
                Err(_) => false,
            }
        }
    }
}

fn print_usage() {
    println!("coldrevive-relay — ColdRevive M2/W3.4 生产中继服务端（W3.8 多实例 / W3.9 传输安全）");
    println!();
    println!("用法: coldrevive-relay --listen <ip:port> --auth-key <64hex|file:/path> \\");
    println!("                        [--peer <ip:port>]... [--instance-id <label>] \\");
    println!("                        [--stats-file <path>] [--max-frame <bytes>] \\");
    println!("                        [--rate <帧/秒>] [--burst <帧>] [--idle-timeout <秒>] \\");
    println!("                        [--max-conns <n>] [--max-nodes <n>] [--handshake-timeout <秒>]");
    println!();
    println!("  --listen              监听地址，默认 127.0.0.1:7000；端口 0 由内核分配并在首行回显");
    println!("  --auth-key            PSK：64 位十六进制，或 file:<路径>（文件内同样格式）");
    println!("  --auth-key-file       等价于 --auth-key file:<路径>");
    println!("                        也可用环境变量 CR_RELAY_KEY；不配则明文（只允许回环）");
    println!("  --allow-insecure      允许在非回环地址上跑明文（实验用，不推荐）");
    println!("  --peer                同侪实例地址，可重复；自动重拨，断线自愈");
    println!("  --instance-id         固定本实例 id（1..=16 可打印 ASCII），默认由地址+时间派生");
    println!("  --stats-file          每 200ms 写一份机读计数 JSON");
    println!("  --max-frame           单帧最大负载字节数，默认 1048576；超限帧直接断开连接");
    println!("  --rate / --burst      每连接令牌桶（默认 200/s、突发 400），超速即断");
    println!("  --idle-timeout        空闲秒数（默认 300，0 = 关闭）：先发 PING，两轮无应答才断");
    println!("  --max-conns           并发连接上限（默认 512）");
    println!("  --max-nodes           本实例注册表上限（默认 8192）");
    println!();
    println!("帧协议（大端）：1B kind + 16B node id + 4B 长度 + 负载，可整体跑在 PSK 记录层里");
    println!("  0x01 REGISTER(负载=16B node id) -> ACK(0x04)");
    println!("  0x02 RELAY(负载=16B 目标 id + 消息) -> 目标收到 DELIVERED(0x03, node=来源 id)");
    println!("  0x05 PEER_HELLO(负载=16B 呼叫方实例 id) -> ACK(node=应答方实例 id)");
    println!("  0x06 PEER_SYNC(负载=n×16B 本地节点 id，整份覆盖)");
    println!("  0x07 FORWARD(负载=16B 来源 + 16B 目标 + 消息，只投递不二次转发)");
    println!();
    println!("注意：本服务只做帧路由，不构成 S6 四类 NAT 可达率实测，S6.status 仍为 not_run。");
}
