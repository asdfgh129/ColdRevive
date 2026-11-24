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
    let mut lookup: Option<(String, String)> = None;
    let mut census_path: Option<String> = None;
    let mut report_path: Option<String> = None;
    let mut tags: Vec<(String, String)> = Vec::new();
    let mut tcp_punch: Option<(String, Vec<String>)> = None;
    let mut echo = false;
    let mut echo_responder = false;
    let mut serve_stun: Option<String> = None;
    let mut stun_alt: Option<String> = None;
    let mut serve_secs: u64 = 0;
    let mut vantage: Option<String> = None;
    let mut vantage_alt: Option<u16> = None;
    let mut classify: Option<String> = None;
    let mut classify_alt: Option<u16> = None;
    let mut discover_tcp_port: Option<(String, Vec<String>)> = None;
    let mut serve_observer: Option<String> = None;
    let mut observer_secs: u64 = 20;

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
            "--lookup" => {
                let r = args.get(i + 1).cloned().unwrap_or_default();
                let n = args.get(i + 2).cloned().unwrap_or_default();
                if r.is_empty() || n.is_empty() {
                    fail("--lookup 需要 <中继地址> <节点id-hex（32位）> 两个参数");
                }
                lookup = Some((r, n));
                i += 2;
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
            "--tcp-punch" => {
                let local = args.get(i + 1).cloned().unwrap_or_default();
                let peers_raw = args.get(i + 2).cloned().unwrap_or_default();
                if local.is_empty() || peers_raw.is_empty() {
                    fail("--tcp-punch 需要 <本地 ip:port> <对端 ip:port[,ip:port...]>");
                }
                let peers: Vec<String> = peers_raw
                    .split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect();
                tcp_punch = Some((local, peers));
                i += 2;
            }
            "--echo" => echo = true,
            "--echo-responder" => echo_responder = true,
            "--serve-stun" => {
                serve_stun = args.get(i + 1).cloned();
                if serve_stun.is_none() {
                    fail("--serve-stun 需要 <ip:port>");
                }
                i += 1;
            }
            "--stun-alt" => {
                stun_alt = args.get(i + 1).cloned();
                if stun_alt.is_none() {
                    fail("--stun-alt 需要 <ip:port>");
                }
                i += 1;
            }
            "--serve-secs" => {
                let v = args.get(i + 1).cloned().unwrap_or_default();
                match v.parse::<u64>() {
                    Ok(n) => serve_secs = n,
                    Err(_) => fail(&format!("非法 --serve-secs 值: {}", v)),
                }
                i += 1;
            }
            "--vantage" => {
                vantage = args.get(i + 1).cloned();
                if vantage.is_none() {
                    fail("--vantage 需要 <ip:port>（对外部 STUN 观测点发起双对照探测）");
                }
                i += 1;
            }
            "--classify" => {
                classify = args.get(i + 1).cloned();
                if classify.is_none() {
                    fail("--classify 需要 <ip:port>（对同一观测点按映射+过滤两轴定性）");
                }
                i += 1;
            }
            "--classify-alt" => {
                let v = args.get(i + 1).cloned().unwrap_or_default();
                match v.parse::<u16>() {
                    Ok(n) => classify_alt = Some(n),
                    Err(_) => fail(&format!("非法 --classify-alt 端口: {}", v)),
                }
                i += 1;
            }
            "--vantage-alt" => {
                let v = args.get(i + 1).cloned().unwrap_or_default();
                match v.parse::<u16>() {
                    Ok(n) => vantage_alt = Some(n),
                    Err(_) => fail(&format!("非法 --vantage-alt 端口: {}", v)),
                }
                i += 1;
            }
            "--discover-tcp-port" => {
                let local = args.get(i + 1).cloned().unwrap_or_default();
                let observers_raw = args.get(i + 2).cloned().unwrap_or_default();
                if local.is_empty() || observers_raw.is_empty() {
                    fail("--discover-tcp-port 需要 <本地 ip:port> <observer ip:port[,ip:port...]>");
                }
                let observers: Vec<String> = observers_raw
                    .split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect();
                if observers.is_empty() {
                    fail("--discover-tcp-port 至少需要一个 observer");
                }
                discover_tcp_port = Some((local, observers));
                i += 2;
            }
            "--serve-observer" => {
                serve_observer = args.get(i + 1).cloned();
                if serve_observer.is_none() {
                    fail("--serve-observer 需要 <ip:port>");
                }
                i += 1;
            }
            "--observer-secs" => {
                let v = args.get(i + 1).cloned().unwrap_or_default();
                match v.parse::<u64>() {
                    Ok(n) => observer_secs = n,
                    Err(_) => fail(&format!("非法 --observer-secs 值: {}", v)),
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

    // ---- RFC 5780 观测点（服务端）----
    // 为什么要自建：公共 STUN 全部忽略 CHANGE-REQUEST，"没回包"因此无法区分
    // "NAT 拦了" 与 "服务器没换源口"。只有自己控制回包源地址，过滤行为才有证据。
    if let Some(bind_text) = serve_stun.as_deref() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let primary = match cr_nat::stun_server::parse_v4(bind_text) {
            Some(a) => a,
            None => fail(&format!("--serve-stun 需要 IPv4 的 ip:port，收到: {}", bind_text)),
        };
        let alt = match stun_alt.as_deref() {
            Some(t) => match cr_nat::stun_server::parse_v4(t) {
                Some(a) => Some(a),
                None => fail(&format!("--stun-alt 需要 IPv4 的 ip:port，收到: {}", t)),
            },
            None => Some(cr_nat::stun_server::adjacent(primary)),
        };
        let srv = match cr_nat::stun_server::StunServer::bind(primary, alt) {
            Ok(s) => Arc::new(s),
            Err(e) => fail(&format!("绑定失败: {}", e)),
        };
        let got_p = srv.primary_addr().map(|a| a.to_string()).unwrap_or_default();
        let got_a = srv.alt_addr().map(|a| a.to_string()).unwrap_or_default();
        println!("STUN_SERVER=bound primary={} alt={}", got_p, got_a);
        println!("STUN_NOTE=单公网 IP 只能测端口轴；CHANGE-IP 将回 420（能力缺失，不是 NAT 拦截）");
        use std::io::Write;
        let stop = Arc::new(AtomicBool::new(false));
        if serve_secs > 0 {
            let st = stop.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(serve_secs));
                st.store(true, Ordering::Relaxed);
            });
        }
        let log = Arc::new(std::sync::Mutex::new(std::io::stdout()));
        let quiet = as_json;
        srv.serve_until(stop.clone(), move |d, peer| {
            if quiet {
                return;
            }
            let w = format!("STUN_REQ peer={} decision={:?}\n", peer, d);
            if let Ok(mut out) = log.lock() {
                let _ = out.write_all(w.as_bytes());
                let _ = out.flush();
            }
        });
        let st = &srv.stats;
        let line = format!(
            "STUN_STATS requests={} answered_here={} answered_from_peer_socket={} change_ip_rejected={} malformed={}",
            st.requests.load(Ordering::Relaxed),
            st.answered_here.load(Ordering::Relaxed),
            st.answered_from_peer_socket.load(Ordering::Relaxed),
            st.change_ip_rejected.load(Ordering::Relaxed),
            st.malformed.load(Ordering::Relaxed)
        );
        println!("{}", line);
        return;
    }

    // ---- 两轴定性：映射轴 + 过滤轴，且拒绝把「对称映射的副作用」读成端口过滤 ----
    //
    // 存在的理由（实测踩到）：222 出口是 symmetric，过滤探测照样会回
    // `outcome=port-filtered` —— 因为 NAT 为目的端口 3479 另开了一个映射，
    // 回包落在客户端从未用过的映射上，于是「收不到」。那是映射的副作用，不是过滤策略。
    // 若把这条读成 port-restricted-cone，就会和同一台机器的 symmetric 结论互相矛盾。
    // 所以先测映射轴；映射已随目的变化时，过滤轴**不参与定性**。
    if let Some(v) = classify.as_deref() {
        let (host, port) = match parse_host_port(v) {
            Some(hp) => hp,
            None => fail(&format!("--classify 需要 host[:port]，收到: {}", v)),
        };
        let alt_port = classify_alt.unwrap_or(port.saturating_add(1));
        let timeout = Duration::from_millis(timeout_ms.max(800));
        let sock = match std::net::UdpSocket::bind("0.0.0.0:0") {
            Ok(s) => s,
            Err(e) => fail(&format!("本地 UDP socket 绑定失败: {}", e)),
        };
        let session = cr_nat::stun::StunSession::new(vec![
            (host.clone(), port),
            (host.clone(), alt_port),
        ]);
        let results = session.probe(&sock, timeout);
        let pairs: Vec<(String, String)> = results
            .iter()
            .filter(|r| r.ok)
            .filter_map(|r| r.mapped.clone().map(|m| (r.server.clone(), m)))
            .collect();
        let axis = cr_nat::mapping_axis(&pairs);
        // 映射轴已随目的变化时，**不跑**过滤探测：跑了也会得到「收不到」，
        // 那是映射副作用。判定规则在 cr_nat::classify_two_axis 里，有单测守着。
        let mut filter_axis = "not-applicable";
        let mut probe_json = String::from("null");
        let filter_obs = if axis == cr_nat::MappingAxis::EndpointIndependent {
            let probe = cr_nat::probe_port_filtering(&host, port, alt_port, timeout);
            filter_axis = probe.outcome.label();
            probe_json = format!(
                "{{\"control_primary\":{},\"control_alt\":{},\"change_port_answered\":{},\"change_port_source\":{},\"source_matches_alt\":{},\"change_ip_error\":{}}}",
                probe.control_primary,
                probe.control_alt,
                probe.change_port_answered,
                jopt(&probe.change_port_source),
                probe.source_matches_alt,
                probe.change_ip_error.map(|c| c.to_string()).unwrap_or_else(|| "null".into())
            );
            Some(cr_nat::classify_from_probe(&probe))
        } else {
            None
        };
        let (nat, fb, why) = cr_nat::classify_two_axis(&pairs, filter_obs);
        let reason = match why {
            cr_nat::classify::WHY_TWO_AXIS => match fb {
                cr_nat::FilterBehavior::PortRestrictedCone => "映射与目的无关 + 入向只放行已建立的映射 → 端口受限",
                cr_nat::FilterBehavior::PortOpenUntestedAddress => "映射与目的无关 + 入向不禁异端口；地址轴缺第二个公网 IP，未测",
                _ => "对照不足，过滤轴未测出结果",
            },
            cr_nat::classify::WHY_SYMMETRIC_FILTER_UNUSED => "映射随目的端口变化 → symmetric；过滤轴不参与定性（回包落到未用过的映射，属映射副作用）",
            cr_nat::classify::WHY_FILTER_MISSING => "只有映射轴样本，缺过滤轴观测",
            _ => "映射样本不足（<2 个目的端口有应答），不定性",
        };
        let pair_json: Vec<String> = pairs
            .iter()
            .map(|(s, m)| format!("{{\"server\": {}, \"mapped\": {}}}", jstr(s), jstr(m)))
            .collect();
        if as_json {
            println!(
                "{{\"mode\":\"classify\",\"vantage\":\"{}\",\"alt_port\":{},\"mapping_pairs\":[{}],\"mapping_axis\":\"{}\",\"filter_axis\":\"{}\",\"filter_probe\":{},\"filtering_behavior\":\"{}\",\"nat_type\":\"{}\",\"reason\":\"{}\",\"s6_status\":\"not_run\"}}",
                format!("{}:{}", host, port),
                alt_port,
                pair_json.join(", "),
                axis.label(),
                filter_axis,
                probe_json,
                fb.label(),
                nat.label(),
                reason
            );
        } else {
            println!("VANTAGE={}:{} alt_port={}", host, port, alt_port);
            for (sr, mm) in &pairs {
                println!("  MAPPING {} -> {}", sr, mm);
            }
            println!("AXIS mapping={}  filter={}", axis.label(), filter_axis);
            println!("NAT_TYPE={}  filtering={}", nat.label(), fb.label());
            println!("REASON={}", reason);
            if axis == cr_nat::MappingAxis::AddressOrPortDependent {
                println!("NOTE=过滤轴刻意未采用：对称映射下「change-port 收不到」是映射副作用，不是过滤策略");
            }
        }
        return;
    }

    // ---- 对外部观测点的双对照探测（客户端）----
    if let Some(v) = vantage.as_deref() {
        let (host, port) = match parse_host_port(v) {
            Some(hp) => hp,
            None => fail(&format!("--vantage 需要 host[:port]，收到: {}", v)),
        };
        let alt_port = vantage_alt.unwrap_or(port.saturating_add(1));
        let probe = cr_nat::probe_port_filtering(
            &host,
            port,
            alt_port,
            Duration::from_millis(timeout_ms.max(500)),
        );
        let fb = cr_nat::classify_from_probe(&probe);
        let nat = cr_nat::classify_from_filter(fb);
        if as_json {
            println!(
                "{{\"mode\":\"vantage-probe\",\"vantage\":\"{}\",\"alt_port\":{},\"control_primary\":{},\"control_alt\":{},\"change_port_answered\":{},\"change_port_source\":{},\"source_matches_alt\":{},\"change_ip_error\":{},\"outcome\":\"{}\",\"filtering_behavior\":\"{}\",\"nat_type\":\"{}\",\"s6_status\":\"not_run\"}}",
                probe.vantage,
                alt_port,
                probe.control_primary,
                probe.control_alt,
                probe.change_port_answered,
                jopt(&probe.change_port_source),
                probe.source_matches_alt,
                probe.change_ip_error.map(|c| c.to_string()).unwrap_or_else(|| "null".into()),
                probe.outcome.label(),
                fb.label(),
                nat.label()
            );
        } else {
            println!("VANTAGE={} alt_port={}", probe.vantage, alt_port);
            println!(
                "CONTROL primary={} alt={}   <- 两项必须都为 yes，否则下面的结论不成立",
                yn(probe.control_primary),
                yn(probe.control_alt)
            );
            println!(
                "CHANGE_PORT answered={} source={} matches_alt={}",
                yn(probe.change_port_answered),
                probe.change_port_source.as_deref().unwrap_or("-"),
                yn(probe.source_matches_alt)
            );
            println!(
                "CHANGE_IP error={}",
                probe
                    .change_ip_error
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "无应答".into())
            );
            println!("OUTCOME={}  filtering={}  nat_type={}", probe.outcome.label(), fb.label(), nat.label());
            if probe.outcome == cr_nat::FilterOutcome::PortNotFiltered {
                println!("NOTE=端口轴已实测（入向不禁异端口）；地址轴缺第二个公网 IP，未测，故不称 full-cone");
            }
        }
        return;
    }

    // TCP 映射端口发现（observer 端）：对每个入向连接回写它看到的源端点。
    // 这是 in-band 的「你被映射成了什么」信令源，本身不做打洞、不改判据。
    if let Some(bind_text) = serve_observer.as_deref() {
        let bind = match bind_text.parse::<std::net::SocketAddr>() {
            Ok(a) => a,
            Err(e) => fail(&format!("--serve-observer 地址 {} 非法: {}", bind_text, e)),
        };
        match cr_nat::tcp_discovery::serve_observer_for(bind, observer_secs) {
            Ok((stats, elapsed_ms)) => {
                let served = stats.served.load(std::sync::atomic::Ordering::Relaxed);
                let failed = stats.write_failed.load(std::sync::atomic::Ordering::Relaxed);
                if as_json {
                    println!(
                        "{{\"mode\":\"serve-observer\",\"bind\":{},\"served\":{},\"write_failed\":{},\"elapsed_ms\":{},\"s6_status\":\"not_run\"}}",
                        jstr(bind_text), served, failed, elapsed_ms
                    );
                } else {
                    println!(
                        "OBSERVER bind={} served={} write_failed={} elapsed_ms={}",
                        bind_text, served, failed, elapsed_ms
                    );
                }
            }
            Err(e) => fail(&format!("observer 监听失败: {}", e)),
        }
        return;
    }

    // TCP 映射端口发现（客户端）：从固定本地端口向 observer 发真 TCP 连接，
    // 读回映射端点作为信令字段；>=2 个 observer 一致才判 EIM。
    if let Some((local_text, observers_text)) = discover_tcp_port {
        let local = match local_text.parse::<std::net::SocketAddr>() {
            Ok(a) => a,
            Err(e) => fail(&format!("本地地址 {} 非法: {}", local_text, e)),
        };
        let mut observers = Vec::new();
        for p in &observers_text {
            match p.parse::<std::net::SocketAddr>() {
                Ok(a) => observers.push(a),
                Err(e) => fail(&format!("observer 地址 {} 非法: {}", p, e)),
            }
        }
        let report = cr_nat::tcp_discovery::discover_tcp_port(
            local,
            &observers,
            Duration::from_millis(timeout_ms.max(500)),
        );
        if as_json {
            println!("{}", report.to_json());
        } else {
            println!("DISCOVER local={} eim={} discovered_port={}",
                report.local,
                report.eim.map(|b| b.to_string()).unwrap_or_else(|| "unproven".into()),
                report.discovered_port.map(|p| p.to_string()).unwrap_or_else(|| "-".into()));
            for o in &report.observations {
                println!("  OBS {} -> {} {}", o.observer,
                    o.mapped_endpoint.as_deref().unwrap_or("-"),
                    o.error.as_deref().map(|e| format!("({e})")).unwrap_or_default());
            }
            println!("REASON={}", report.reason);
        }
        return;
    }

    // TCP 同时打开打洞：真实 SYN、真实握手，结果只报内核告诉我们的事实。
    if let Some((local_text, peers_text)) = tcp_punch {
        let local = match local_text.parse::<std::net::SocketAddr>() {
            Ok(a) => a,
            Err(e) => fail(&format!("本地地址 {} 非法: {}", local_text, e)),
        };
        let mut peers = Vec::new();
        for p in &peers_text {
            match p.parse::<std::net::SocketAddr>() {
                Ok(a) => peers.push(a),
                Err(e) => fail(&format!("对端地址 {} 非法: {}", p, e)),
            }
        }
        let report = cr_nat::tcp_punch::punch_tcp(
            local,
            &peers,
            Duration::from_millis(timeout_ms.max(500)),
        );
        let mut bytes_ok: Option<bool> = None;
        if let Some(mut p) = report.established {
            // 双向字节验证：只有真的能在洞里收发，「打通」才算被验证过。
            if echo || echo_responder {
                let _ = p.stream.set_read_timeout(Some(Duration::from_millis(timeout_ms.max(500))));
                bytes_ok = Some(if echo {
                    use std::io::{Read, Write};
                    let wrote = p.stream.write_all(b"PING").is_ok();
                    let mut buf = [0u8; 4];
                    wrote && p.stream.read_exact(&mut buf).is_ok() && &buf == b"PONG"
                } else {
                    use std::io::{Read, Write};
                    let mut buf = [0u8; 4];
                    p.stream.read_exact(&mut buf).is_ok()
                        && &buf == b"PING"
                        && p.stream.write_all(b"PONG").is_ok()
                });
            }
            if as_json {
                println!(
                    "{{\"mode\":\"tcp-punch\",\"ok\":{},\"local\":\"{}\",\"peer\":\"{}\",\"attempts\":{},\"elapsed_ms\":{},\"bytes_roundtrip\":{}}}",
                    bytes_ok.unwrap_or(true), p.local, p.peer, p.attempts,
                    p.elapsed.as_millis(),
                    bytes_ok.map(|b| b.to_string()).unwrap_or_else(|| "null".to_string())
                );
            } else {
                println!("TCP_PUNCH=established local={} peer={} attempts={} elapsed_ms={} bytes_roundtrip={}",
                    p.local, p.peer, p.attempts, p.elapsed.as_millis(),
                    bytes_ok.map(|b| b.to_string()).unwrap_or_else(|| "not-tested".to_string()));
            }
            if bytes_ok == Some(false) {
                exit(1);
            }
            return;
        }
        // 没打通：如实报，不合成结果。
        let errs = if report.errors.is_empty() {
            "no-error-reported(被静默丢弃)".to_string()
        } else {
            report.errors.join("; ")
        };
        if as_json {
            println!(
                "{{\"mode\":\"tcp-punch\",\"ok\":false,\"local\":\"{}\",\"peers\":\"{}\",\"attempts\":{},\"errors\":\"{}\"}}",
                local_text, peers_text.join(","), report.attempts, errs.replace('"', "'")
            );
        } else {
            eprintln!("TCP_PUNCH=failed attempts={} errors={}", report.attempts, errs);
        }
        exit(1);
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

    // 查询模式：问中继「某节点自报的可达性类别」。
    // 这不是探测——本机不发任何 STUN/打洞包，只是问一句已经存在的声明。
    if let Some((relay, hex_id)) = lookup {
        let target = parse_node_hex(&hex_id);
        let client = cr_nat::relay::RelayClient::new(relay.clone());
        let mut conn = match client.connect(Duration::from_millis(timeout_ms.max(1000))) {
            Ok(c) => c,
            Err(e) => fail(&format!("连接中继 {} 失败: {}", relay, e)),
        };
        let session = cr_nat::relay::ephemeral_session_id(&target);
        if let Err(e) = conn.register(&session) {
            fail(&format!("注册会话失败（旧版中继不支持？）: {}", e));
        }
        match conn.lookup_reach(&target, Duration::from_millis(timeout_ms.max(1000))) {
            Ok(Some(d)) => {
                if as_json {
                    println!("{}", d.to_json().dump());
                } else {
                    println!("REACH_CLASS={} nat_class={} declared_at_ms={} ttl_ms={}",
                        d.class.label(), d.nat_class.label(),
                        d.declared_at_ms, d.ttl_ms);
                }
            }
            Ok(None) => {
                if as_json {
                    println!("{{\"reach_class\":\"unknown\",\"declared\":false}}");
                } else {
                    println!("REACH_CLASS=unknown（中继没有该节点的声明；unknown ≠ 连不上）");
                }
            }
            Err(e) => fail(&format!("查询失败: {}", e)),
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
    println!("ColdRevive 出口普查汇总（描述性类别清单；S6=not_run，判据=eMule 式可达）");
    println!("---------------------------------------------------------------");
    println!("样本数              : {}（其中 unknown {}）", s.samples, s.unknown_class_samples);
    println!("出口数              : {}", s.egresses.len());
    println!(
        "清单类别 / 独立覆盖 : {} 类 / {} 类（描述性，非 pass 前提）",
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
    println!("  清单中未覆盖（描述性）: {}",
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
        "S6_STANCE={}  CLASS_COVERAGE_REQUIRED_FOR_PASS={}  MATRIX_COVERAGE={}  R04_JUDGEABLE={}  S6_STATUS={}  G2_CLAIM={}",
        cr_nat::S6_REACH_STANCE,
        cr_nat::S6_CLASS_COVERAGE_REQUIRED_FOR_PASS,
        s.matrix_coverage, s.r04_judgeable, s.s6_status, s.g2_claim
    );
    println!("注：类别清单不含任何可达性数字，不能折算成矩阵覆盖，也不能用于 R-04。");
    println!("    ADR-016 后类别覆盖不是 pass 前提；S6 按 eMule 式可达语义验收（HighID 直连 / LowID 只走中继）。");
}


/// 32 位十六进制 → 16 字节节点 id（coldrevive-cli 输出的 node_id 就是这个形态）。
fn parse_node_hex(s: &str) -> [u8; cr_nat::relay::NODE_ID_LEN] {
    let t = s.trim();
    if t.len() != 2 * cr_nat::relay::NODE_ID_LEN
        || !t.chars().all(|c| c.is_ascii_hexdigit())
    {
        fail(&format!(
            "节点 id 必须是 {} 位十六进制: {}",
            2 * cr_nat::relay::NODE_ID_LEN,
            s
        ));
    }
    let mut out = [0u8; cr_nat::relay::NODE_ID_LEN];
    for k in 0..cr_nat::relay::NODE_ID_LEN {
        out[k] = u8::from_str_radix(&t[2 * k..2 * k + 2], 16).unwrap_or(0);
    }
    out
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
    m.insert("reach_class", jstr(cr_nat::reach::ReachDeclaration::from_report(r, 0).class.label()));
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
    // 原始观测（服务器 → 映射地址）。放在 JSON 里是为了让分类结论可回溯到具体报文：
    // 只给一个 `distinct_mappings` 计数，读的人无法复核「到底是哪两台报了不同的口」。
    let pairs: Vec<String> = r
        .observations
        .iter()
        .map(|(s, mm)| format!("{{\"server\": {}, \"mapped\": {}}}", jstr(s), jstr(mm)))
        .collect();
    m.insert("observation_pairs", format!("[{}]", pairs.join(", ")));
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
    println!(
        "可达性类别（自报）  : {}",
        cr_nat::reach::ReachDeclaration::from_report(r, 0).class.label()
    );
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
  coldrevive-nat --serve-stun <ip:port> [--stun-alt <ip:port>] [--serve-secs <秒>]\n\
  coldrevive-nat --vantage <ip:port> [--vantage-alt <端口>] [--json]\n\
  coldrevive-nat --classify <ip:port> [--classify-alt <端口>] [--json]\n\
\n\
  # 4) 问中继「某节点自报的可达性类别」（不发探测包）\n\
  coldrevive-nat --lookup <中继地址> <节点id-hex（32位）> [--json]\n\
\n\
  # 5) TCP 同时打开打洞（两侧须几乎同时启动）\n\
  coldrevive-nat --tcp-punch <本地 ip:port> <对端 ip:port[,..]> \\\n\
                [--echo | --echo-responder] [--timeout <毫秒>] [--json]\n\
\n\
  # 5b) 动态 TCP 映射端口发现（STUN 语义，给对端的信令字段）\n\
  coldrevive-nat --serve-observer <ip:port> [--observer-secs <秒>] [--json]\n\
  coldrevive-nat --discover-tcp-port <本地 ip:port> <observer ip:port[,ip:port...]> \\\n\
                [--timeout <毫秒>] [--json]\n\
\n\
选项:\n\
  --json            以 JSON 输出（字段顺序稳定，供脚本解析）\n\
  --timeout <毫秒>  每次探测超时（默认 3000）\n\
  --stun <h[:p]>    追加 STUN 服务器（可多次，默认 3478 端口）\n\
  --gateway <h[:p]> 网关地址（启用 NAT-PMP / PCP 探测，默认端口 5351）\n\
  --upnp-location <url>  直接指定 UPnP IGD 控制地址（跳过 SSDP 发现）\n\
  --relay <addr>    中继地址（用于策略评估：对称 NAT 且无映射时回退）\n\
  --lookup <中继> <id>  查询某节点的可达性声明；查不到回 unknown（unknown ≠ 连不上）\n\
  --census <文件>   探测后追加一条样本到 JSONL（同 sample_id 自动去重）\n\
  --report <文件>   只汇总已有样本，不探测\n\
  --tcp-punch <l> <p...>  TCP 同时打开：绑定本地端口，向对端公网映射端口并发 SYN\n\
  --echo            打通后发 PING 并等 PONG（发起方）\n\
  --echo-responder  打通后等 PING 并回 PONG（应答方）\n\
  --serve-observer <ip:port>  端口发现 observer：回写看到的源端点\n\
  --observer-secs <秒>  observer 服务时长（默认 20）\n\
  --discover-tcp-port <l> <obs...>  从本地端口发现映射端点；>=2 observer 判 EIM\n\
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
