//! coldrevive-nat — NAT 穿透诊断工具（M2 合法前置）。
//!
//! 离线可编译（仅依赖 cr-nat，std-only），不触碰 T0 契约与 M1 复活流程。
//! 它只是把 `cr_nat::Traversal::run()` 的结果格式化输出，用于：
//!   1) 在第二台物理主机接入前，先验证本机所处 NAT 类型与可达手段；
//!   2) 为后续跨 NAT 实测（S6）提供策略选择依据。
//!
//! 另有 `--census` / `--report`：把每次探测沉淀成**出口类别清单**。
//! 清单回答「这个出口是哪一类」，**不回答**「这一类能不能打通」——
//! 后者是矩阵覆盖（30 次里的成功率），只有 `tests/spikes/s6` 的矩阵跑得出来。
//!
//! **门禁声明**：本工具仅做探测与策略建议，不宣称 S6/G2 达标。S6.status 仍为 `not_run`。

use std::collections::BTreeMap;
use std::path::Path;
use std::process::exit;
use std::time::Duration;

use cr_nat::census::{self, AppendOutcome, CensusRecord, SampleMeta};
use cr_nat::{needs_relay, Traversal};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut as_json = false;
    let mut timeout_ms: u64 = 3000;
    let mut stun: Vec<(String, u16)> = Vec::new();
    let mut gateway: Option<String> = None;
    let mut upnp_location: Option<String> = None;
    let mut relay_addr: Option<String> = None;
    let mut census_path: Option<String> = None;
    let mut report_path: Option<String> = None;
    let mut tags: Vec<(String, String)> = Vec::new();

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
            "--census" => {
                census_path = args.get(i + 1).cloned();
                if census_path.is_none() {
                    fail("--census 需要一个文件路径");
                }
                i += 1;
            }
            "--report" => {
                report_path = args.get(i + 1).cloned();
                if report_path.is_none() {
                    fail("--report 需要一个文件路径");
                }
                i += 1;
            }
            "--tag" => {
                let v = args.get(i + 1).cloned().unwrap_or_default();
                match v.split_once('=') {
                    Some((k, val)) => tags.push((k.trim().to_string(), val.trim().to_string())),
                    None => fail(&format!("非法 --tag 值（需 k=v）: {}", v)),
                }
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

    // 汇总模式：只读文件，不探测。避免「想看一眼清单却顺手多打了几枪」。
    if let Some(p) = report_path {
        let records = match census::load(Path::new(&p)) {
            Ok(r) => r,
            Err(e) => fail(&e),
        };
        let s = census::summarize(&records);
        if as_json {
            println!("{}", s.to_json().dump());
        } else {
            print_summary(&s);
        }
        return;
    }

    let mut t = Traversal::new();
    if !stun.is_empty() {
        t.stun_servers = stun;
    }
    t.gateway = gateway;
    t.upnp_location = upnp_location;
    t.relay_addr = relay_addr;

    let report = t.run(Duration::from_millis(timeout_ms));

    if let Some(path) = census_path {
        let meta = meta_from_tags(&tags);
        let rec = match CensusRecord::from_report(&report, &meta, census::now_unix_ms()) {
            Ok(r) => r,
            Err(e) => fail(&e),
        };
        match census::append(Path::new(&path), &rec) {
            Ok(AppendOutcome::Appended) => {
                println!("CENSUS_APPENDED={} egress={} class={}",
                    rec.sample_id, rec.egress, rec.nat_class)
            }
            Ok(AppendOutcome::Duplicate) => {
                println!("CENSUS_DUPLICATE={} （同一份样本已存在，未重复计入）", rec.sample_id)
            }
            Err(e) => fail(&e),
        }
        if !as_json {
            println!();
        }
    }

    if as_json {
        println!("{}", to_json(&report));
    } else {
        print_human(&report);
    }
}

fn meta_from_tags(tags: &[(String, String)]) -> SampleMeta {
    let mut m = SampleMeta::default();
    for (k, v) in tags {
        match k.as_str() {
            "egress" => m.egress = v.clone(),
            "operator" => m.operator = v.clone(),
            "device" => m.device = Some(v.clone()),
            "variant" => m.variant = Some(v.clone()),
            "host" => m.host = v.clone(),
            "notes" => m.notes = Some(v.clone()),
            other => fail(&format!(
                "未知 --tag 键: {}（可用: egress, operator, device, variant, host, notes）",
                other
            )),
        }
    }
    m
}

fn print_summary(s: &cr_nat::CensusSummary) {
    println!("ColdRevive 出口普查汇总（类别清单，非矩阵覆盖；S6=not_run）");
    println!("---------------------------------------------------------------");
    println!("样本数              : {}（其中 unknown {}）", s.samples, s.unknown_class_samples);
    println!("出口数              : {}", s.egresses.len());
    println!(
        "判据要求 / 独立覆盖 : {} 类 / {} 类",
        cr_nat::S6_REQUIRED_MATRIX.len(),
        s.independent_classes()
    );
    println!();
    for (class, egresses) in &s.class_inventory {
        println!("  {:<22}: {}", class, egresses.join(", "));
    }
    if !s.variant_only_classes.is_empty() {
        println!("  仅配置变体可复现    : {}", s.variant_only_classes.join(", "));
    }
    println!("  缺（无独立出口）    : {}",
        if s.missing_classes.is_empty() { "-".to_string() } else { s.missing_classes.join(", ") });
    println!();
    println!("出口明细:");
    for e in &s.egresses {
        println!(
            "  {:<18} operator={:<8} samples={:<3} classes={}{}",
            e.egress,
            e.operator,
            e.samples,
            if e.classes.is_empty() { "-".to_string() } else { e.classes.join("/") },
            match &e.device { Some(d) => format!(" device={}", d), None => String::new() }
        );
        for c in &e.conflicts {
            println!("    ! {}", c);
        }
    }
    if !s.warnings.is_empty() {
        println!();
        println!("警告:");
        for w in &s.warnings {
            println!("  - {}", w);
        }
    }
    println!("---------------------------------------------------------------");
    println!(
        "MATRIX_COVERAGE={}  R04_JUDGEABLE={}  S6_STATUS={}  G2_CLAIM={}",
        s.matrix_coverage, s.r04_judgeable, s.s6_status, s.g2_claim
    );
    println!("注：类别清单不含任何可达性数字，不能折算成矩阵覆盖，也不能用于 R-04。");
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
    m.insert("filtering_behavior", jstr(r.filter_behavior.label()));
    m.insert("observations", r.observations.len().to_string());
    m.insert(
        "distinct_mappings",
        {
            let mut v: Vec<&str> = r.observations.iter().map(|(_, m)| m.as_str()).collect();
            v.sort_unstable();
            v.dedup();
            v.len()
        }
        .to_string(),
    );
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
    println!(
        "过滤行为（实测）    : {}（依据 {} 条观测）",
        r.filter_behavior.label(),
        r.observations.len()
    );
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
  # 1) 单次探测\n\
  coldrevive-nat [--json] [--timeout <毫秒>] [--stun <host[:port]>...] \\\n\
                [--gateway <host[:port]>] [--upnp-location <url>] [--relay <addr>]\n\
\n\
  # 2) 探测并归档为一条出口样本\n\
  coldrevive-nat --census <census.jsonl> --tag egress=<名字> \\\n\
                [--tag operator=<运营商>] [--tag device=<设备>] \\\n\
                [--tag variant=<配置变体>] [--tag host=<主机>] [--tag notes=<备注>]\n\
\n\
  # 3) 只看已有样本的汇总（不探测）\n\
  coldrevive-nat --report <census.jsonl> [--json]\n\
\n\
选项:\n\
  --json            以 JSON 输出（字段顺序稳定，供脚本解析）\n\
  --timeout <毫秒>  每次探测超时（默认 3000）\n\
  --stun <h[:p]>    追加 STUN 服务器（可多次，默认 3478 端口）\n\
  --gateway <h[:p]> 网关地址（启用 NAT-PMP / PCP 探测，默认端口 5351）\n\
  --upnp-location <url>  直接指定 UPnP IGD 控制地址（跳过 SSDP 发现）\n\
  --relay <addr>    中继地址（用于策略评估：对称 NAT 且无映射时回退）\n\
  --census <文件>   探测后追加一条样本到 JSONL（同 sample_id 自动去重）\n\
  --report <文件>   只汇总已有样本，不探测\n\
  --tag k=v         样本元信息，可多次；egress 必填\n\
  -h, --help        显示本帮助\n\
\n\
关于 --census：产出的是**出口类别清单**，回答「这个出口是哪一类」。\n\
它不含任何可达性数字，因此不能折算成 S6 矩阵覆盖，也不能用于 R-04；\n\
矩阵覆盖只能由 tests/spikes/s6 的矩阵跑出来。S6.status 始终为 not_run。"
    );
}

fn fail(msg: &str) -> ! {
    eprintln!("错误: {}", msg);
    eprintln!("使用 -h 查看帮助。");
    exit(2);
}
