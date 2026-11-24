//! G2 公平性量具。只输出原始 JSON，不判 G2 pass，不写门禁文件。
//!
//! `relay` 模式在净网内的真中继上跑固定窗口；`flow` 模式同时跑一对
//! 真 `UdpRelayStream`（发送方 + 独立 ACK 接收方），逐时刻记录应用字节。
//! 两条 flow 进程共享同一个 veth/HTB 瓶颈，才构成竞争流样本。

use cr_nat::relay::default_psk;
use cr_nat::relay_udp::{UdpLimits, UdpRelayClient, UdpRelayServer};
use cr_nat::relay_udp_stream::{CongestionConfig, UdpRelayStream};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const NODE_ID_LEN: usize = 16;
const HEX: &[u8; 16] = b"0123456789abcdef";

fn opt(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn req(args: &[String], key: &str) -> Result<String, String> {
    opt(args, key).ok_or_else(|| format!("缺少参数 {key}"))
}

fn num(args: &[String], key: &str) -> Result<u64, String> {
    let s = req(args, key)?;
    s.parse::<u64>()
        .map_err(|e| format!("{key} 不是整数: {e}"))
}

fn boolean(args: &[String], key: &str, default: bool) -> Result<bool, String> {
    match opt(args, key) {
        None => Ok(default),
        Some(v) => match v.as_str() {
            "1" | "true" | "TRUE" | "on" => Ok(true),
            "0" | "false" | "FALSE" | "off" => Ok(false),
            _ => Err(format!("{key} 必须是 true/false，收到 {v}")),
        },
    }
}

fn parse_id(s: &str) -> Result<[u8; NODE_ID_LEN], String> {
    if s.len() != NODE_ID_LEN * 2 {
        return Err(format!("节点 id 必须是 {} 个十六进制字符", NODE_ID_LEN * 2));
    }
    let mut out = [0u8; NODE_ID_LEN];
    let bytes = s.as_bytes();
    for i in 0..NODE_ID_LEN {
        let hi = HEX
            .iter()
            .position(|&c| c == bytes[i * 2].to_ascii_lowercase())
            .ok_or_else(|| format!("非法十六进制 id: {s}"))? as u8;
        let lo = HEX
            .iter()
            .position(|&c| c == bytes[i * 2 + 1].to_ascii_lowercase())
            .ok_or_else(|| format!("非法十六进制 id: {s}"))? as u8;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

fn jstr(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn cc_json(cc: CongestionConfig) -> String {
    format!(
        "{{\"enabled\":{},\"cwnd_init\":{},\"cwnd_min\":{},\"cwnd_max\":{},\"beta\":{},\"pacing\":{}}}",
        cc.enabled, cc.cwnd_init, cc.cwnd_min, cc.cwnd_max, cc.beta, cc.pacing
    )
}

fn run_relay(args: &[String]) -> Result<(), String> {
    let bind = req(args, "--bind")?;
    let seconds = num(args, "--seconds")?;
    let out = req(args, "--out")?;
    let mut limits = UdpLimits::default();
    limits.rate_pps = 100_000; // 排除 pps 瓶颈，只让字节预算/内核 HTB 起作用
    limits.data_byte_rate = num(args, "--data-rate")?;
    limits.data_burst_bytes = num(args, "--data-burst")?;
    limits.per_session_data_byte_rate = num(args, "--per-session-rate")?;
    limits.per_session_burst_bytes = num(args, "--per-session-burst")?;

    let server = UdpRelayServer::bind(&bind)?
        .with_psk(default_psk())
        .with_limits(limits);
    let handle = server.spawn();
    println!("RELAY_READY {}", handle.local_addr());

    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(seconds) {
        thread::sleep(Duration::from_millis(100));
    }
    let s = handle.stats();
    handle.stop();

    let body = format!(
        concat!(
            "{{\"kind\":\"relay\",\"bind\":{},\"seconds\":{},\"limits\":{{\"rate_pps\":{},",
            "\"data_byte_rate\":{},\"data_burst_bytes\":{},",
            "\"per_session_data_byte_rate\":{},\"per_session_burst_bytes\":{}}},",
            "\"stats\":{{\"datagrams\":{},\"bytes_in\":{},\"bytes_out\":{},",
            "\"data_in\":{},\"deliver_out\":{},\"status_out\":{},",
            "\"data_bytes_in\":{},\"data_bytes_out\":{},",
            "\"bandwidth_limited\":{},\"bandwidth_limited_bytes\":{},",
            "\"per_session_limited\":{},\"per_session_limited_bytes\":{},",
            "\"rate_limited\":{},\"replay\":{},\"mac_fail\":{},",
            "\"sessions_active\":{},\"sessions_expired\":{}}}}}"
        ),
        jstr(&bind),
        seconds,
        limits.rate_pps,
        limits.data_byte_rate,
        limits.data_burst_bytes,
        limits.per_session_data_byte_rate,
        limits.per_session_burst_bytes,
        s.datagrams,
        s.bytes_in,
        s.bytes_out,
        s.data_in,
        s.deliver_out,
        s.status_out,
        s.data_bytes_in,
        s.data_bytes_out,
        s.bandwidth_limited,
        s.bandwidth_limited_bytes,
        s.per_session_limited,
        s.per_session_limited_bytes,
        s.rate_limited,
        s.replay,
        s.mac_fail,
        s.sessions_active,
        s.sessions_expired,
    );
    fs::write(&out, body).map_err(|e| format!("写 {out} 失败: {e}"))?;
    Ok(())
}

fn run_flow(args: &[String]) -> Result<(), String> {
    let server = req(args, "--server")?;
    let recv_id = parse_id(&req(args, "--recv-id")?)?;
    let send_id = parse_id(&req(args, "--send-id")?)?;
    let purpose = req(args, "--purpose")?;
    let warmup_s = num(args, "--warmup")?;
    let sample_s = num(args, "--sample")?;
    let chunk = num(args, "--chunk")?.max(1).min(1024) as usize;
    let out = req(args, "--out")?;
    let cwnd_init = match opt(args, "--cwnd-init") {
        None => 4u64,
        Some(v) => v.parse::<u64>().map_err(|e| format!("--cwnd-init 不是整数: {e}"))?,
    };
    let cc = CongestionConfig {
        enabled: boolean(args, "--cc", false)?,
        cwnd_init: cwnd_init.min(32).max(1) as u32,
        cwnd_min: 1,
        cwnd_max: 32,
        beta: 0.5,
        pacing: boolean(args, "--pacing", false)?,
    };

    let (ready_tx, ready_rx) = mpsc::sync_channel::<()>(2);
    let psk = default_psk();
    let server_recv = server.clone();
    let cc_recv = cc;
    let total_s = warmup_s + sample_s + 3;
    let ready_recv = ready_tx.clone();

    let receiver = thread::spawn(move || -> Result<String, String> {
        let mut client = UdpRelayClient::connect(&server_recv, Duration::from_millis(500))?
            .with_psk(psk);
        client.hello(recv_id, Duration::from_secs(3))?;
        ready_recv.send(()).map_err(|_| "接收方就绪通知失败".to_string())?;
        let stream = UdpRelayStream::new_with_cc(client, send_id, cc_recv)?;
        let (mut read, mut write) = stream.split();
        // 不能在这里 close_write()：对端流泵一旦先收到 CLOSE 且当时 in-flight 为空，
        // 会按 half-close 语义直接收尾，发送方随后写入就会 BrokenPipe。
        // 接收方全程不发应用数据，只在采样结束后关闭写半边。
        let started = Instant::now();
        let mut samples: Vec<(u64, u64)> = Vec::new();
        let mut buf = vec![0u8; 16 * 1024];
        while started.elapsed() < Duration::from_secs(total_s) {
            match read.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => samples.push((started.elapsed().as_millis() as u64, n as u64)),
                Err(e) => return Err(format!("接收方读失败: {e}")),
            }
        }
        write.close_write();
        let stats = read.cc_stats();
        let total: u64 = samples.iter().map(|(_, n)| *n).sum();
        let sample_pairs = samples
            .iter()
            .map(|(t, n)| format!("[{t},{n}]"))
            .collect::<Vec<_>>()
            .join(",");
        Ok(format!(
            concat!(
                "{{\"bytes_read\":{},\"bytes_read_counter\":{},\"retransmits\":{},",
                "\"cc\":{{\"enabled\":{},\"cwnd_x100\":{},\"srtt_ms\":{},",
                "\"rtt_samples\":{},\"loss_events\":{},\"paced_new\":{}}},",
                "\"samples\":[{}]}}"
            ),
            total,
            read.bytes_read(),
            read.retransmits(),
            stats.enabled,
            stats.cwnd_x100,
            stats.srtt_ms,
            stats.rtt_samples,
            stats.loss_events,
            stats.paced_new_segments,
            sample_pairs,
        ))
    });

    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| format!("接收方未就绪: {e}"))?;

    let server_send = server.clone();
    let cc_send = cc;
    let ready_send = ready_tx;
    let sender = thread::spawn(move || -> Result<String, String> {
        let mut client = UdpRelayClient::connect(&server_send, Duration::from_millis(500))?
            .with_psk(psk);
        client.hello(send_id, Duration::from_secs(3))?;
        ready_send.send(()).map_err(|_| "发送方就绪通知失败".to_string())?;
        let stream = UdpRelayStream::new_with_cc(client, recv_id, cc_send)?;
        let (read, mut write) = stream.split();
        drop(read); // 发送方不向应用暴露反向数据；ACK 仍在流泵内处理
        let payload = vec![0x5au8; chunk];
        let started = Instant::now();
        let deadline = Duration::from_secs(warmup_s + sample_s);
        while started.elapsed() < deadline {
            write
                .write_all(&payload)
                .map_err(|e| format!("发送方写失败: {e}"))?;
        }
        let bytes = write.bytes_written();
        let retransmits = write.retransmits();
        let stats = write.cc_stats();
        write.close_write();
        thread::sleep(Duration::from_secs(2)); // 让 ACK/CLOSE 排空
        Ok(format!(
            concat!(
                "{{\"bytes_written\":{},\"retransmits\":{},",
                "\"cc\":{{\"enabled\":{},\"cwnd_x100\":{},\"srtt_ms\":{},",
                "\"rtt_samples\":{},\"loss_events\":{},\"paced_new\":{}}}}}"
            ),
            bytes,
            retransmits,
            stats.enabled,
            stats.cwnd_x100,
            stats.srtt_ms,
            stats.rtt_samples,
            stats.loss_events,
            stats.paced_new_segments,
        ))
    });

    let receiver_json = receiver
        .join()
        .map_err(|_| "接收线程 panic".to_string())??;
    let sender_json = sender
        .join()
        .map_err(|_| "发送线程 panic".to_string())??;

    let report = format!(
        concat!(
            "{{\"kind\":\"flow\",\"purpose\":{},\"server\":{},",
            "\"recv_id\":{},\"send_id\":{},\"warmup_s\":{},\"sample_s\":{},",
            "\"chunk_bytes\":{},\"cc_config\":{},",
            "\"receiver\":{},\"sender\":{}}}"
        ),
        jstr(&purpose),
        jstr(&server),
        jstr(&hex(&recv_id)),
        jstr(&hex(&send_id)),
        warmup_s,
        sample_s,
        chunk,
        cc_json(cc),
        receiver_json,
        sender_json,
    );
    fs::write(&out, report).map_err(|e| format!("写 {out} 失败: {e}"))?;
    Ok(())
}

fn main() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mode = args.first().ok_or_else(|| "用法: relay|flow ...".to_string())?;
    match mode.as_str() {
        "relay" => run_relay(&args[1..]),
        "flow" => run_flow(&args[1..]),
        other => Err(format!("未知模式 {other}")),
    }
}
