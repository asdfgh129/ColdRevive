//! coldrevive-nat — NAT 穿透诊断工具（M2 合法前置）。
//!
//! 离线可编译（仅依赖 cr-nat，std-only），不触碰 T0 契约与 M1 复活流程。
//! 它只是把 `cr_nat::Traversal::run()` 的结果格式化输出，用于：
//!   1) 在第二台物理主机接入前，先验证本机所处 NAT 类型与可达手段；
//!   2) 为后续跨 NAT 实测（S6）提供策略选择依据。
//!
//! **门禁声明**：本工具仅做探测与策略建议，不宣称 S6/G2 达标。S6.status 仍为 `not_run`。

use std::collections::BTreeMap;
use std::process::exit;
use std::time::Duration;

use cr_nat::{needs_relay, Traversal};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut as_json = false;
    let mut timeout_ms: u64 = 3000;
    let mut stun: Vec<(String, u16)> = Vec::new();
    let mut gateway: Option<String> = None;
    let mut upnp_location: Option<String> = None;
    let mut relay_addr: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--json" => as_json = true,
            "--timeout" => {
                let v = args.get(i + 1).cloned().unwrap_or_default();
                match v.parse::<u64>() {
                    Ok(n) => timeout_ms = n,
                    Err(_) => fail(&format!("非法 --timeout 值: {}", v)),
                }
                i += 1;
            }
            "--stun" => {
                let v = args.get(i + 1).cloned().unwrap_or_default();
                match parse_host_port(&v) {
                    Some(hp) => stun.push(hp),
                    None => fail(&format!("非法 --stun 值（需 host[:port]）: {}", v)),
                }
                i += 1;
            }
            "--gateway" => {
                gateway = args.get(i + 1).cloned();
                i += 1;
            }
            "--upnp-location" => {
                upnp_location = args.get(i + 1).cloned();
                i += 1;
            }
            "--relay" => {
                relay_addr = args.get(i + 1).cloned();
                i += 1;
            }
            "--help" | "-h" => {
                print_help();
                return;
            }
            other => fail(&format!("未知参数: {}", other)),
        }
        i += 1;
    }

    let mut t = Traversal::new();
    if !stun.is_empty() {
        t.stun_servers = stun;
    }
    t.gateway = gateway;
    t.upnp_location = upnp_location;
    t.relay_addr = relay_addr;

    let report = t.run(Duration::from_millis(timeout_ms));

    if as_json {
        println!("{}", to_json(&report));
    } else {
        print_human(&report);
    }
}

fn parse_host_port(s: &str) -> Option<(String, u16)> {
    if let Some(colon) = s.rfind(':') {
        let host = &s[..colon];
        let port = s[colon + 1..].parse::<u16>().ok()?;
        if host.is_empty() {
            return None;
        }
        Some((host.to_string(), port))
    } else {
        // 仅主机名，使用默认 STUN 端口 3478
        Some((s.to_string(), 3478))
    }
}

fn jstr(s: &str) -> String {
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
    match s {
        Some(v) => jstr(v),
        None => "null".to_string(),
    }
}

fn to_json(r: &cr_nat::TraversalReport) -> String {
    // 用 BTreeMap 保证字段顺序稳定，便于脚本解析。
    let mut m = BTreeMap::new();
    m.insert("nat_type", jstr(r.nat_type.label()));
    m.insert(
        "public_udp_egress_ok",
        if r.public_udp_egress_ok { "true" } else { "false" }.to_string(),
    );
    m.insert(
        "upnp_available",
        if r.upnp_available { "true" } else { "false" }.to_string(),
    );
    m.insert("upnp_external_ip", jopt(&r.upnp_external_ip));
    m.insert(
        "natpmp_available",
        if r.natpmp_available { "true" } else { "false" }.to_string(),
    );
    m.insert("natpmp_external_ip", jopt(&r.natpmp_external_ip));
    m.insert(
        "pcp_available",
        if r.pcp_available { "true" } else { "false" }.to_string(),
    );
    m.insert(
        "cgnat_suspected",
        if r.cgnat_suspected { "true" } else { "false" }.to_string(),
    );
    m.insert("mapped_address", jopt(&r.mapped_address));
    m.insert(
        "recommended_strategy",
        jstr(r.recommended_strategy.label()),
    );
    m.insert(
        "needs_relay",
        if needs_relay(r) { "true" } else { "false" }.to_string(),
    );
    // S6 门禁声明
    m.insert("s6_status", jstr("not_run"));

    let mut out = String::from("{");
    let mut first = true;
    for (k, v) in &m {
        if !first {
            out.push_str(", ");
        }
        out.push_str(&jstr(k));
        out.push_str(": ");
        out.push_str(v);
        first = false;
    }
    out.push('}');
    out
}

fn print_human(r: &cr_nat::TraversalReport) {
    println!("ColdRevive NAT 穿透诊断（M2 合法前置，S6=not_run）");
    println!("---------------------------------------------------");
    println!("NAT 类型            : {}", r.nat_type.label());
    println!(
        "公网 UDP 出口可达   : {}",
        yn(r.public_udp_egress_ok)
    );
    println!("映射地址            : {}", r.mapped_address.as_deref().unwrap_or("-"));
    println!("UPnP IGD 可用       : {}", yn(r.upnp_available));
    if let Some(ip) = &r.upnp_external_ip {
        println!("  └ 外部 IP          : {}", ip);
    }
    println!("NAT-PMP 可用        : {}", yn(r.natpmp_available));
    if let Some(ip) = &r.natpmp_external_ip {
        println!("  └ 外部 IP          : {}", ip);
    }
    println!("PCP 可用            : {}", yn(r.pcp_available));
    println!("CGNAT 疑似          : {}", yn(r.cgnat_suspected));
    println!(
        "推荐穿透策略        : {}",
        r.recommended_strategy.label()
    );
    println!("需要中继回退        : {}", yn(needs_relay(r)));
    println!("---------------------------------------------------");
    println!("注：本工具仅做探测与策略建议，不宣称 S6/G2 达标。");
}

fn yn(b: bool) -> &'static str {
    if b {
        "是 (YES)"
    } else {
        "否 (NO)"
    }
}

fn print_help() {
    println!(
        "coldrevive-nat — NAT 穿透诊断工具（M2 合法前置）\n\
\n\
用法:\n\
  coldrevive-nat [--json] [--timeout <毫秒>] [--stun <host[:port]>...] \\\n\
                [--gateway <host[:port]>] [--upnp-location <url>] [--relay <addr>]\n\
\n\
选项:\n\
  --json            以 JSON 输出（字段顺序稳定，供脚本解析）\n\
  --timeout <毫秒> 每次探测超时（默认 3000）\n\
  --stun <h[:p]>    追加 STUN 服务器（可多次，默认 3478 端口）\n\
  --gateway <h[:p]> 网关地址（启用 NAT-PMP / PCP 探测，默认端口 5351）\n\
  --upnp-location <url>  直接指定 UPnP IGD 控制地址（跳过 SSDP 发现）\n\
  --relay <addr>    中继地址（用于策略评估：对称 NAT 且无映射时回退）\n\
  -h, --help        显示本帮助\n\
\n\
注意：S6.status 始终为 not_run，本工具不宣称 G2 达标。"
    );
}

fn fail(msg: &str) -> ! {
    eprintln!("错误: {}", msg);
    eprintln!("使用 -h 查看帮助。");
    exit(2);
}
