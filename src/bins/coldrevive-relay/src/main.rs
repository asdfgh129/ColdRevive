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
//!   `--max-nodes` 注册表上限；
//! - `--data-rate` / `--data-burst` 全局**数据字节**预算（W3.13）：≤4 KiB 的唤醒信令
//!   绕过预算，其余帧按字节计费，超额明确拒绝并计 `bandwidth_limited`。这是部署侧的
//!   带宽护栏，不是可达率承诺。
//!
//! 用法：
//!   coldrevive-relay --listen 0.0.0.0:7100 --auth-key <64hex|file:/path> \
//!                    [--peer 1.2.3.4:7100]... [--instance-id relay-a] \
//!                    [--stats-file <path>] [--max-frame 1048576]
//!
//! 启动后首行输出 `RELAY_LISTEN=<实际地址>`（`:0` 时回显内核分配的端口），
//! `RELAY_AUTH=<psk|none>` 与 `RELAY_KEY_FP=<指纹>` 供人工核对两端是否同一把钥匙。
//! `--stats-file` 每 200 ms 落一份机读计数，供测试读取**真实**投递/丢弃/转发/拒绝次数。

use cr_nat::relay_server::{parse_instance_id, printable_instance, IpNet, Limits, RelayServer};
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
    if let Some(v) = opt("--data-rate").and_then(|s| s.parse::<u64>().ok()) {
        limits.data_byte_rate = v.max(1);
    }
    if let Some(v) = opt("--data-burst").and_then(|s| s.parse::<u64>().ok()) {
        limits.data_burst_bytes = v.max(1);
    }
    // 单 IP 维度（W3.9 后续）：整机上限挡不住「一台机器占满所有名额」。
    if let Some(v) = opt("--max-conns-per-ip").and_then(|s| s.parse::<usize>().ok()) {
        limits.max_conns_per_ip = v.max(1);
    }
    if let Some(v) = opt("--register-per-ip-per-min").and_then(|s| s.parse::<u32>().ok()) {
        limits.register_per_ip_per_min = v.max(1);
    }
    // 黑名单：可重复 `--deny 1.2.3.0/24`。解析失败直接退出——写错的黑名单
    // 等于没有黑名单，而运维会以为已经屏蔽了。
    let mut deny: Vec<IpNet> = Vec::new();
    for text in opt_all("--deny") {
        match IpNet::parse(&text) {
            Ok(n) => deny.push(n),
            Err(e) => {
                eprintln!("--deny {} 非法: {}", text, e);
                std::process::exit(2);
            }
        }
    }

    let mut server = match RelayServer::bind(&listen) {
        Ok(s) => s
            .with_max_frame(max_frame)
            .with_psk(psk)
            .with_allow_insecure(allow_insecure || loopback)
            .with_limits(limits)
            .with_deny(deny.clone()),
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
    println!("RELAY_LIMITS=rate:{}/s,burst:{},idle:{}s,max_conns:{},max_nodes:{},data_rate:{}/s,data_burst:{},max_conns_per_ip:{},register_per_ip_per_min:{}",
             limits.rate_per_sec, limits.burst, limits.idle.as_secs(),
             limits.max_conns, limits.max_nodes, limits.data_byte_rate, limits.data_burst_bytes,
             limits.max_conns_per_ip, limits.register_per_ip_per_min);
    if deny.iter().any(|n| n.is_all_ipv4()) {
        eprintln!("[coldrevive-relay] 警告：--deny 0.0.0.0/0 会拒绝所有来源（等于不提供服务）。");
    }
    // 黑名单回显的是**解析后的网段**，让部署侧能核对「我屏蔽的到底是什么」。
    println!("RELAY_DENY={}", deny.iter().map(|n| n.to_text()).collect::<Vec<_>>().join(","));
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

    // ---- 可选：UDP 承载（W3.14）----
    //
    // 存在的理由不是「多一种传输」，而是有些云边缘对**非 22 的入向 TCP** 在建连后
    // 注入伪造 RST（2026-11-18 腾讯云轻量实测：TTL 随机化、比 SYN-ACK 晚 270 µs，
    // 停掉云镜后依然存在），而同一台机器的入向 UDP 完全可用。TCP 被掐时，UDP 是
    // 唯一还能拿到原生数据的路。
    let udp_stats_file = opt("--udp-stats-file").map(PathBuf::from);
    let mut udp_handle = None;
    if let Some(addr) = opt("--udp-listen").filter(|s| !s.trim().is_empty()) {
        let udp_loopback = is_loopback_listen(&addr);
        if psk.is_none() && !udp_loopback && !allow_insecure {
            eprintln!("拒绝启动 UDP 承载：{} 是非回环地址，但没有配置 PSK。", addr);
            eprintln!("  UDP 没有握手，无认证的开放中继可被当作反射/放大跳板（虽然本实现");
            eprintln!("  要求逐报文 HMAC、回程不大于来程）。请配 --auth-key，或加 --allow-insecure。");
            std::process::exit(2);
        }
        let mut ulim = cr_nat::relay_udp::UdpLimits::default();
        if let Some(v) = opt("--udp-max-sessions").and_then(|s| s.parse::<usize>().ok()) {
            ulim.max_sessions = v.max(1);
        }
        if let Some(v) = opt("--udp-max-per-ip").and_then(|s| s.parse::<usize>().ok()) {
            ulim.max_per_ip = v.max(1);
        }
        if let Some(v) = opt("--udp-idle-timeout").and_then(|s| s.parse::<u64>().ok()) {
            ulim.idle_timeout = Duration::from_secs(v.max(1));
        }
        if let Some(v) = opt("--udp-rate").and_then(|s| s.parse::<u32>().ok()) {
            ulim.rate_pps = v.max(1);
        }
        // ADR-024 阶段 0：UDP 数据字节预算，与 TCP 的 --data-rate/--data-burst 同口径。
        if let Some(v) = opt("--udp-data-rate").and_then(|s| s.parse::<u64>().ok()) {
            ulim.data_byte_rate = v.max(1);
        }
        if let Some(v) = opt("--udp-data-burst").and_then(|s| s.parse::<u64>().ok()) {
            ulim.data_burst_bytes = v.max(1);
        }
        let srv = match cr_nat::relay_udp::UdpRelayServer::bind(&addr) {
            Ok(s) => s.with_psk(psk).with_limits(ulim),
            Err(e) => {
                eprintln!("UDP 承载启动失败: {}", e);
                std::process::exit(2);
            }
        };
        let a = match srv.local_addr() {
            Ok(a) => a,
            Err(e) => {
                eprintln!("读取 UDP 监听地址失败: {}", e);
                std::process::exit(2);
            }
        };
        let h = srv.spawn();
        println!("RELAY_UDP_LISTEN={}", a);
        println!(
            "RELAY_UDP_LIMITS=max_sessions:{},max_per_ip:{},idle:{}s,rate:{}/s,data_rate:{}/s,data_burst:{}",
            ulim.max_sessions, ulim.max_per_ip, ulim.idle_timeout.as_secs(), ulim.rate_pps,
            ulim.data_byte_rate, ulim.data_burst_bytes
        );
        println!(
            "[coldrevive-relay] UDP 承载监听 {}（每报文 HMAC，最大负载 {} 字节）",
            a,
            ulim.max_payload
        );
        udp_handle = Some(h);
    } else {
        println!("RELAY_UDP_LISTEN=");
    }

    if let (Some(path), Some(h)) = (udp_stats_file, udp_handle.as_ref()) {
        // UDP 计数单独一份文件：TCP 的 --stats-file 是既有 schema，往里加字段会让
        // 别的读者把「多出来的键」当成协议变更。两份文件、各自演进。
        let addr = h.local_addr().to_string();
        let auth = auth_label.to_string();
        let fp_udp = fp.clone();
        let counters = h.stats_handle();
        thread::spawn(move || loop {
            let stats = counters.snapshot();
            let body = format!(
                "{{\"udp_listen\":\"{}\",\"auth\":\"{}\",\"key_fp\":\"{}\",\
                 \"datagrams\":{},\"bytes_in\":{},\"bytes_out\":{},\"hello\":{},\"hello_ack\":{},\
                 \"rebound\":{},\"data_in\":{},\"deliver_out\":{},\"status_out\":{},\"pong_out\":{},\
                 \"reject_out\":{},\"bye\":{},\"mac_fail\":{},\"bad_format\":{},\"magic_mismatch\":{},\
                 \"too_big\":{},\"replay\":{},\"rate_limited\":{},\"data_bytes_in\":{},\
                 \"data_bytes_out\":{},\"bandwidth_limited\":{},\"bandwidth_limited_bytes\":{},\
                 \"not_registered\":{},\
                 \"target_absent\":{},\"sessions_rejected\":{},\"sessions_active\":{},\
                 \"sessions_expired\":{}}}",
                addr,
                auth,
                fp_udp,
                stats.datagrams,
                stats.bytes_in,
                stats.bytes_out,
                stats.hello,
                stats.hello_ack,
                stats.rebound,
                stats.data_in,
                stats.deliver_out,
                stats.status_out,
                stats.pong_out,
                stats.reject_out,
                stats.bye,
                stats.mac_fail,
                stats.bad_format,
                stats.magic_mismatch,
                stats.too_big,
                stats.replay,
                stats.rate_limited,
                stats.data_bytes_in,
                stats.data_bytes_out,
                stats.bandwidth_limited,
                stats.bandwidth_limited_bytes,
                stats.not_registered,
                stats.target_absent,
                stats.sessions_rejected,
                stats.sessions_active,
                stats.sessions_expired
            );
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&path, body);
            thread::sleep(Duration::from_millis(200));
        });
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
                 \"declared_unknown\":{},\"reach_lookups\":{},\
                 \"data_bytes_admitted\":{},\"bandwidth_limited\":{},\"bandwidth_limited_bytes\":{},\
                 \"blacklisted\":{},\"ip_cap_rejected\":{},\"ip_rate_limited\":{},\"ip_tracked\":{}}}",
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
                s.reach_lookups,
                s.data_bytes_admitted,
                s.bandwidth_limited,
                s.bandwidth_limited_bytes,
                s.blacklisted,
                s.ip_cap_rejected,
                s.ip_rate_limited,
                s.ip_tracked
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
    println!("                        [--max-conns <n>] [--max-nodes <n>] [--handshake-timeout <秒>] \\");
    println!("                        [--data-rate <字节/秒>] [--data-burst <字节>]");
    println!("                        [--udp-listen <ip:port>] [--udp-stats-file <path>]");
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
    println!("  --data-rate           全局数据字节预算（默认 524288 B/s ≈ 4 Mbps），按机器带宽调");
    println!("  --data-burst          数据字节突发容量（默认 2097152 B）；≤4096 B 的信令帧不计费");
    println!("  --udp-listen          **额外**开一个 UDP 承载（W3.14），如 0.0.0.0:7100。");
    println!("                        不写就不开；与 TCP 同时可用，两者共用同一把 PSK 与注册语义。");
    println!("                        存在理由：有的云边缘会掐非 22 的入向 TCP，但 UDP 可用。");
    println!("  --udp-stats-file      UDP 计数单独落盘（200ms 一次），与 --stats-file 的 schema 分开");
    println!("  --udp-max-sessions / --udp-max-per-ip / --udp-idle-timeout / --udp-rate");
    println!("                        UDP 护栏：会话表上限、单 IP 会话数、空闲回收秒数、每会话报文/秒");
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
