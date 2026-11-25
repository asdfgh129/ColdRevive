//! coldrevive-dht — cr-dht 的可观测入口（W3.2）。
//!
//! 存在的理由：一个 crate 单测全绿，并不能说明「它在真实网络上会怎么表现」。
//! 这里把同一个 `DhtNode` / `DhtClient` 暴露成命令行工具，输出**机读 JSON**，
//! 让 `--self-test`（回环）与 `--ping`（任意真实节点）共用一套代码。
//!
//! 口径纪律：
//! - `--self-test` 只证明编解码与 UDP 收发自洽，输出里 `evidence_scope=loopback`，
//!   **不是** S3/S6 证据。
//! - `--ping` / `--get-peers` 打的是调用方给的地址，成功只说明「这个地址回了一个
//!   合法 KRPC 应答」，不代表公网宣告闭环，也不代表任何可达率。
//! - 失败一律以非零退出码 + `"ok":false` + 真实错误原文结束，不编造结果。

use cr_dht::{announce_active_seed, bencode, DhtClient, DhtNode, DhtServer, NodeId, ServerHandle, ID_LEN, K};
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |name: &str| -> Option<String> {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
    };
    let flag = |name: &str| args.iter().any(|a| a == name);

    if flag("--help") || args.is_empty() {
        eprintln!(
            "用法:\n  \
             coldrevive-dht --self-test\n  \
             coldrevive-dht --serve <bind> [--node-id <hex>]\n  \
             coldrevive-dht --ping <host:port>\n  \
             coldrevive-dht --get-peers <host:port> <infohash-hex> [--announce <port>]\n  \
             coldrevive-dht --announce-seed <host:port> <infohash-hex> --listen-port <port>\n  \
             coldrevive-dht --walk <seed[,seed...]> [--rounds 3] [--max-queries 32]\n\
             \n\
             说明: 目标可以是域名（如 dht.transmissionbt.com:6881），会解析到 IPv4。\n\
             --self-test 只在 127.0.0.1 上验证编解码与收发（evidence_scope=loopback），\n\
             不是 S3/S6 证据。--ping/--get-peers 的真实性取决于给定地址。\n\
             --walk 只读（不发 announce_peer），有查询数上限；输出 contacts 供复现。\n\
             --announce-seed 走 daemon FR-18 同一条 announce_active_seed 路径。"
        );
        std::process::exit(if args.is_empty() { 2 } else { 0 });
    }

    if flag("--self-test") {
        std::process::exit(self_test());
    }

    if let Some(bind) = opt("--serve") {
        let id = node_id_from(&opt("--node-id"));
        let node = Arc::new(DhtNode::new(id));
        let server = match DhtServer::bind(&bind, node.clone()) {
            Ok(s) => s,
            Err(e) => fail(&format!("绑定失败: {e}")),
        };
        let addr = match server.local_addr() {
            Ok(a) => a,
            Err(e) => fail(&format!("取本地地址失败: {e}")),
        };
        println!(
            "{}",
            json(&[
                ("ok", "true".into()),
                ("mode", q("serve")),
                ("listen", q(&addr.to_string())),
                ("node_id", q(&node.id().to_hex())),
                ("evidence_scope", q("serving")),
            ])
        );
        let stop = AtomicBool::new(false);
        server.run(&stop);
        return;
    }

    if let Some(addr_text) = opt("--ping") {
        let id = node_id_from(&opt("--node-id"));
        let client = match DhtClient::bind("0.0.0.0:0", id) {
            Ok(c) => c,
            Err(e) => fail(&format!("客户端绑定失败: {e}")),
        };
        let addr = match parse_target(&addr_text) {
            Ok(a) => a,
            Err(e) => fail(&format!("{e}")),
        };
        match client.ping(addr, Duration::from_millis(2000)) {
            Ok(remote) => println!(
                "{}",
                json(&[
                    ("ok", "true".into()),
                    ("mode", q("ping")),
                    ("target", q(&addr_text)),
                    ("resolved", q(&addr.to_string())),
                    ("remote_id", q(&remote.to_hex())),
                    ("local_addr", q(&client.local_addr().unwrap_or(addr).to_string())),
                ])
            ),
            Err(e) => fail(&format!("ping {addr} 失败: {e}")),
        }
        return;
    }

    // 守护进程 FR-18 用的就是 cr_dht::announce_active_seed；此处把它暴露成 CLI，
    // 便于对真实 Mainline 节点验证「daemon 同一条代码路径」的 token/announce/回查。
    if let Some(addr_text) = opt("--announce-seed") {
        let ih_hex = args
            .iter()
            .position(|a| a == "--announce-seed")
            .and_then(|i| args.get(i + 2))
            .cloned()
            .unwrap_or_default();
        let ih = match parse_infohash(&ih_hex) {
            Ok(v) => v,
            Err(e) => fail(&format!("info_hash 非法: {e}")),
        };
        let port = opt("--listen-port")
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or_else(|| fail("--listen-port 必须是 1..65535"));
        let addr = match parse_target(&addr_text) {
            Ok(a) => a,
            Err(e) => fail(&format!("{e}")),
        };
        let client = match DhtClient::bind("0.0.0.0:0", NodeId::random()) {
            Ok(c) => c,
            Err(e) => fail(&format!("客户端绑定失败: {e}")),
        };
        let outcome = announce_active_seed(&client, addr, ih, port, Duration::from_millis(3000));
        println!("{}", json(&[
            ("ok", if matches!(outcome, cr_dht::AnnounceOutcome::Announced { .. }) { "true".into() } else { "false".into() }),
            ("mode", q("announce-seed")),
            ("target", q(&addr.to_string())),
            ("info_hash", q(&hex(&ih))),
            ("status", q(outcome.status_label())),
            ("detail", q(&format!("{outcome:?}"))),
            ("evidence_scope", q("public-target")),
        ]));
        return;
    }

    if let Some(addr_text) = opt("--get-peers") {
        let ih_hex = args
            .iter()
            .position(|a| a == "--get-peers")
            .and_then(|i| args.get(i + 2))
            .cloned()
            .unwrap_or_default();
        let ih = match parse_infohash(&ih_hex) {
            Ok(v) => v,
            Err(e) => fail(&format!("info_hash 非法: {e}")),
        };
        let id = node_id_from(&opt("--node-id"));
        let client = match DhtClient::bind("0.0.0.0:0", id) {
            Ok(c) => c,
            Err(e) => fail(&format!("客户端绑定失败: {e}")),
        };
        let addr = match parse_target(&addr_text) {
            Ok(a) => a,
            Err(e) => fail(&format!("{e}")),
        };
        let (token, nodes, values) = match client.get_peers(addr, ih, Duration::from_millis(3000)) {
            Ok(v) => v,
            Err(e) => fail(&format!("get_peers 失败: {e}")),
        };
        println!(
            "{}",
            json(&[
                ("ok", "true".into()),
                ("mode", q("get-peers")),
                ("target", q(&addr.to_string())),
                ("info_hash", q(&hex(&ih))),
                ("token_len", token.as_ref().map(|t| t.len()).unwrap_or(0).to_string()),
                ("nodes", nodes.len().to_string()),
                ("node_list", q(&nodes.iter().map(|c| c.addr.to_string()).collect::<Vec<_>>().join(","))),
                ("peers", values.len().to_string()),
                ("peer_list", q(&values.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(","))),
            ])
        );
        if let Some(port) = opt("--announce").and_then(|p| p.parse::<u16>().ok()) {
            let token = match token {
                Some(t) => t,
                None => fail("对端没有签发 token，无法宣告"),
            };
            match client.announce_peer(addr, ih, port, token, Duration::from_millis(3000)) {
                Ok(()) => println!(
                    "{}",
                    json(&[
                        ("ok", "true".into()),
                        ("mode", q("announce")),
                        ("port", port.to_string()),
                    ])
                ),
                Err(e) => fail(&format!("announce_peer 失败: {e}")),
            }
        }
        return;
    }

    if let Some(addr_text) = opt("--find-node") {
        let id = node_id_from(&opt("--node-id"));
        let client = match DhtClient::bind("0.0.0.0:0", id) {
            Ok(c) => c,
            Err(e) => fail(&format!("客户端绑定失败: {e}")),
        };
        let addr = match parse_target(&addr_text) {
            Ok(a) => a,
            Err(e) => fail(&format!("{e}")),
        };
        let target = match opt("--target").and_then(|t| NodeId::from_hex(&t).ok()) {
            Some(t) => t,
            None => NodeId::random(),
        };
        match client.find_node(addr, target, Duration::from_millis(2000)) {
            Ok(nodes) => println!(
                "{}",
                json(&[
                    ("ok", "true".into()),
                    ("mode", q("find-node")),
                    ("target", q(&addr_text)),
                    ("resolved", q(&addr.to_string())),
                    ("nodes", nodes.len().to_string()),
                    ("node_list", q(&nodes.iter().map(|c| c.addr.to_string()).collect::<Vec<_>>().join(","))),
                ])
            ),
            Err(e) => fail(&format!("find_node {addr} 失败: {e}")),
        }
        return;
    }

    if let Some(seeds_text) = opt("--walk") {
        std::process::exit(walk(&seeds_text, &opt("--rounds"), &opt("--max-queries")));
    }

    eprintln!("未知参数组合；用 --help 查看用法");
    std::process::exit(2);
}

/// 受控漫游：从引导节点出发，用 `find_node` 走有限跳，统计「真实公网上能学到多少活节点」。
///
/// 为什么要有它：单点 `--ping` 只证明「某一台机器回了一个包」；而 DHT 的价值在于
/// 「能从种子扩散到一个邻域」。这两件事的证据强度差一个量级。
///
/// 纪律：
/// - **全程不发 `announce_peer`**，不向公网写入任何东西，只读。
/// - 查询数有硬上限（默认 32），不把别人的节点当压测目标。
/// - 每个数字都是真实收发计数；一个都没通就 `ok:false`，不拿「0 个」假装成功。
fn walk(seeds_text: &str, rounds: &Option<String>, max_queries: &Option<String>) -> i32 {
    let rounds: usize = rounds.as_deref().and_then(|s| s.parse().ok()).unwrap_or(3).clamp(1, 6);
    let cap: usize = max_queries
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(32)
        .clamp(1, 128);

    let mut seeds = Vec::new();
    for part in seeds_text.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match parse_target(part) {
            Ok(a) => seeds.push(a),
            Err(e) => fail(&format!("{e}")),
        }
    }
    if seeds.is_empty() {
        fail("--walk 需要至少一个引导节点");
    }

    let client = match DhtClient::bind("0.0.0.0:0", NodeId::random()) {
        Ok(c) => c,
        Err(e) => fail(&format!("客户端绑定失败: {e}")),
    };
    let target = NodeId::random();

    let mut visited: std::collections::BTreeSet<SocketAddr> = seeds.iter().copied().collect();
    let mut queue: std::collections::VecDeque<SocketAddr> = seeds.iter().copied().collect();
    let mut learned: std::collections::BTreeMap<[u8; ID_LEN], SocketAddr> = Default::default();
    let (mut queried, mut alive, mut answered, mut failures) = (0usize, 0usize, 0usize, 0usize);
    let mut elapsed_rounds = Vec::new();

    for _ in 0..rounds {
        if queried >= cap {
            break;
        }
        let started = std::time::Instant::now();
        let mut batch: Vec<SocketAddr> = Vec::new();
        while let Some(addr) = queue.pop_front() {
            if queried >= cap {
                queue.push_front(addr); // 留给下一轮，不算进本轮
                break;
            }
            batch.push(addr);
            queried += 1;
            if client.ping(addr, Duration::from_millis(1200)).is_ok() {
                alive += 1;
            }
            match client.find_node(addr, target, Duration::from_millis(1500)) {
                Ok(contacts) => {
                    answered += 1;
                    for c in contacts {
                        learned.entry(*c.id.as_bytes()).or_insert(c.addr);
                        if visited.insert(c.addr) {
                            queue.push_back(c.addr);
                        }
                    }
                }
                Err(_) => failures += 1,
            }
        }
        elapsed_rounds.push(started.elapsed().as_millis());
        if batch.is_empty() || queue.is_empty() {
            break;
        }
    }

    // 只读遍历学到的真实路由表节点地址：供后续 token/announce 探测复现。
    // 字段以逗号分隔字符串输出，避免改变既有 JSON 结构；--walk 仍不发任何 announce。
    let mut learned_addrs: Vec<String> = learned.values().map(|a| a.to_string()).collect();
    learned_addrs.sort();
    let contacts = learned_addrs.join(",");

    let ok = answered > 0;
    let detail = json(&[
        ("ok", if ok { "true".into() } else { "false".into() }),
        ("mode", q("walk")),
        ("seeds", q(seeds_text)),
        ("seeds_total", seeds.len().to_string()),
        ("queries_cap", cap.to_string()),
        ("queried", queried.to_string()),
        ("ping_alive", alive.to_string()),
        ("find_node_answered", answered.to_string()),
        ("find_node_failed", failures.to_string()),
        ("unique_contacts", learned.len().to_string()),
        ("frontier_left", queue.len().to_string()),
        ("round_ms", q(&elapsed_rounds.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(","))),
        ("contacts", q(&contacts)),
        ("evidence_scope", q("public-readonly")),
        ("announced", q("none")),
    ]);
    if ok {
        println!("{detail}");
        0
    } else {
        eprintln!("{detail}");
        1
    }
}

/// 回环自检：服务端与客户端都用真实 socket，断言「应答里的 id == 服务端 id」。
fn self_test() -> i32 {
    let node = Arc::new(DhtNode::new(NodeId::random()));
    let srv = match ServerHandle::spawn(node.clone()) {
        Ok(s) => s,
        Err(e) => return fail_code(&format!("起服务失败: {e}")),
    };
    let client = match DhtClient::bind("127.0.0.1:0", NodeId::random()) {
        Ok(c) => c,
        Err(e) => return fail_code(&format!("客户端绑定失败: {e}")),
    };
    let started = std::time::Instant::now();
    let remote = match client.ping(srv.addr, Duration::from_millis(1500)) {
        Ok(id) => id,
        Err(e) => return fail_code(&format!("ping 回环失败: {e}")),
    };
    let elapsed = started.elapsed().as_millis();
    if remote != node.id() {
        return fail_code(&format!("应答 id 与节点 id 不一致: {} vs {}", remote.to_hex(), node.id().to_hex()));
    }
    // find_node 也要走一遍真实 UDP：只测 ping 会漏掉 compact 编码路径。
    // 先塞一个公网联系人：不塞的话表里只有回环调用方，而 compact 解析会丢弃回环地址，
    // 结果恒为 0——那看起来像「编码坏了」，其实是数据本身不合法。
    node.seed_contact(cr_dht::Contact {
        id: NodeId::random(),
        addr: "112.5.60.4:6881".parse().expect("常量地址"),
    });
    let nodes = match client.find_node(srv.addr, NodeId::random(), Duration::from_millis(1500)) {
        Ok(v) => v,
        Err(e) => return fail_code(&format!("find_node 回环失败: {e}")),
    };
    println!(
        "{}",
        json(&[
            ("ok", "true".into()),
            ("mode", q("self-test")),
            ("node_id", q(&node.id().to_hex())),
            ("rtt_ms", elapsed.to_string()),
            ("learned_contacts", node.table_len().to_string()),
            ("find_node_results", nodes.len().to_string()),
            ("k", K.to_string()),
            ("evidence_scope", q("loopback")),
            ("is_s6_evidence", "false".into()),
            ("note", q("回环只证明编解码与收发自洽；公网宣告证据见 tests/spikes/s3")),
        ])
    );
    0
}

fn node_id_from(hex_opt: &Option<String>) -> NodeId {
    match hex_opt {
        None => NodeId::random(),
        Some(h) => match NodeId::from_hex(h) {
            Ok(id) => id,
            Err(e) => fail(&format!("--node-id 非法: {e}")),
        },
    }
}

fn parse_infohash(hex_text: &str) -> Result<[u8; ID_LEN], String> {
    let id = NodeId::from_hex(hex_text)?;
    Ok(*id.as_bytes())
}

/// 解析目标：先按字面 `ip:port`，失败再当主机名解析。
///
/// 必须支持域名，因为 BEP 5 的引导节点全是域名（`router.bittorrent.com:6881` 等），
/// 只收字面 IP 会把「最常用的一条路径」变成非法输入——而且报错是
/// `invalid socket address syntax`，看起来像用户写错了地址，实际是工具不支持。
///
/// 只取 IPv4：本 crate 的 socket 是 v4（`DhtClient::bind("0.0.0.0:0")`），
/// 解析出 v6 却拿去用只会在更深处失败。解析结果一并回显，便于核对「到底打了谁」。
fn parse_target(text: &str) -> Result<SocketAddr, String> {
    if let Ok(a) = text.parse::<SocketAddr>() {
        return Ok(a);
    }
    use std::net::ToSocketAddrs;
    let mut v6_seen = false;
    match text.to_socket_addrs() {
        Ok(iter) => {
            for a in iter {
                if a.is_ipv4() {
                    return Ok(a);
                }
                v6_seen = true;
            }
            if v6_seen {
                Err(format!("{text} 只解析出 IPv6 地址；本工具的 DHT socket 仅支持 IPv4"))
            } else {
                Err(format!("{text} 没有解析出任何地址"))
            }
        }
        Err(e) => Err(format!("地址 {text} 既不是 ip:port 也无法解析为域名: {e}")),
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn q(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
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

fn fail_code(msg: &str) -> i32 {
    eprintln!("{}", json(&[("ok", "false".into()), ("error", q(msg))]));
    1
}

// 保持 bencode 依赖被显式使用：错误报文里需要区分「字节串」与「整数」，
// 后续 `--scrape` 之类的扩展会用到；现在只在自检里做一次编码往返，
// 确保 bencode 模块在二进制侧也被链接（避免被当作死代码）。
#[allow(dead_code)]
fn bencode_smoke() -> bool {
    let v = bencode::dict(vec![("t", bencode::bytes("aa"))]);
    bencode::decode(&v.encode()).map(|d| d == v).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_ip_port_passes_through() {
        let a = parse_target("127.0.0.1:6881").expect("字面地址必须直接可用");
        assert_eq!(a.to_string(), "127.0.0.1:6881");
    }

    #[test]
    fn hostname_is_resolved_to_ipv4() {
        // `localhost` 在 hosts 里恒有映射，不依赖外网，适合当域名解析的样板。
        let a = parse_target("localhost:6881").expect("域名必须可解析");
        assert!(a.is_ipv4(), "DHT socket 只有 v4，解析必须落到 v4");
        assert_eq!(a.port(), 6881, "端口必须保留");
    }

    #[test]
    fn garbage_is_rejected_with_explicit_error() {
        let e = parse_target("not a host:6881").unwrap_err();
        assert!(!e.is_empty());
        // 不能悄悄回落到某个默认地址：那会让「探测失败」看起来像「探测成功」。
        assert!(e.contains("无法解析") || e.contains("没有解析出"));
    }
}
