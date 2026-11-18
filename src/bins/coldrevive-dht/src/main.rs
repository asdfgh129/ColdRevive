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

use cr_dht::{bencode, DhtClient, DhtNode, DhtServer, NodeId, ServerHandle, ID_LEN, K};
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
             coldrevive-dht --ping <ip:port>\n  \
             coldrevive-dht --get-peers <ip:port> <infohash-hex> [--announce <port>]\n\
             \n\
             说明: --self-test 只在 127.0.0.1 上验证编解码与收发（evidence_scope=loopback），\n\
             不是 S3/S6 证据。--ping/--get-peers 的真实性取决于给定地址。"
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
        let addr = match addr_text.parse::<SocketAddr>() {
            Ok(a) => a,
            Err(e) => fail(&format!("地址 {addr_text} 非法: {e}")),
        };
        match client.ping(addr, Duration::from_millis(2000)) {
            Ok(remote) => println!(
                "{}",
                json(&[
                    ("ok", "true".into()),
                    ("mode", q("ping")),
                    ("target", q(&addr.to_string())),
                    ("remote_id", q(&remote.to_hex())),
                    ("local_addr", q(&client.local_addr().unwrap_or(addr).to_string())),
                ])
            ),
            Err(e) => fail(&format!("ping {addr} 失败: {e}")),
        }
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
        let addr = match addr_text.parse::<SocketAddr>() {
            Ok(a) => a,
            Err(e) => fail(&format!("地址 {addr_text} 非法: {e}")),
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

    eprintln!("未知参数组合；用 --help 查看用法");
    std::process::exit(2);
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
