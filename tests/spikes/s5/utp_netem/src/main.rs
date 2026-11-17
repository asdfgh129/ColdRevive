//! S5 真实内核链路量测 —— uTP 端点（双进程角色模式）。
//!
//! --role server : 在 netns s5srv 内绑 uTP socket，accept 并读取 seconds 秒，写字节数到 --stat
//! --role client : 在默认 ns 内连对端，按 cap_bps 限速写 seconds 秒，写字节数到 --stat
//!
//! 两个端点分处不同 netns，流量必经 v5a 出口的 tc htb+netem 内核瓶颈。
//! 前景 TCP 由 shell 用 iperf3 同步跑（同样跨 netns 穿 tc）。

use std::net::SocketAddr;
use std::time::Duration;

use librqbit_utp::UtpSocketUdp;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{sleep, Instant};

fn opt(args: &[String], name: &str, default: &str) -> String {
    args.iter().position(|a| a == name)
        .and_then(|i| args.get(i + 1)).cloned().unwrap_or_else(|| default.to_string())
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let role = opt(&args, "--role", "client");
    let ip = opt(&args, "--ip", "10.201.0.2");
    let port: u16 = opt(&args, "--port", "24000").parse().unwrap();
    let seconds: f64 = opt(&args, "--seconds", "4").parse().unwrap();
    let cap_mbps: f64 = opt(&args, "--cap-mbps", "3.2").parse().unwrap();
    let stat = opt(&args, "--stat", "/tmp/utp-stat.json");
    let addr: SocketAddr = format!("{ip}:{port}").parse().unwrap();

    let start = Instant::now();
    let mut bytes: u64 = 0;
    if role == "server" {
        let sock = match UtpSocketUdp::new_udp(addr).await {
            Ok(v) => v, Err(e) => { eprintln!("[uTP server] bind {addr}: {e}"); return; }
        };
        let mut stream = match tokio::time::timeout(Duration::from_secs(12), sock.accept()).await {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => { eprintln!("[uTP server] accept: {e}"); return; }
            Err(_) => { eprintln!("[uTP server] accept timeout"); return; }
        };
        eprintln!("[uTP server] accepted after {:?}", start.elapsed());
        let mut buf = vec![0u8; 16 * 1024];
        while start.elapsed().as_secs_f64() < seconds {
            // 超时只表示这一刻没数据（低速 uTP 正常），继续读到总时长结束。
            match tokio::time::timeout(Duration::from_millis(300), stream.read(&mut buf)).await {
                Ok(Ok(0)) | Ok(Err(_)) => break,
                Ok(Ok(n)) => bytes += n as u64,
                Err(_) => continue,
            }
        }
        eprintln!("[uTP server] read {bytes} bytes in {:?}", start.elapsed());
    } else {
        let sock = match UtpSocketUdp::new_udp("0.0.0.0:0".parse().unwrap()).await {
            Ok(v) => v, Err(e) => { eprintln!("[uTP client] bind: {e}"); return; }
        };
        let mut stream = match tokio::time::timeout(Duration::from_secs(12), sock.connect(addr)).await {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => { eprintln!("[uTP client] connect {addr}: {e}"); return; }
            Err(_) => { eprintln!("[uTP client] connect timeout"); return; }
        };
        eprintln!("[uTP client] connected after {:?}", start.elapsed());
        let chunk = 1400usize;
        let buf = vec![0x3Cu8; chunk];
        let cap_bps = cap_mbps * 1_000_000.0 / 8.0;
        let per_tick = cap_bps * 0.02; // 每 20ms 增量的字节预算
        let mut tick = Instant::now();
        let mut budget: f64 = 0.0;   // 累积预算：低速率时攒够一个 chunk 再发
        while start.elapsed().as_secs_f64() < seconds {
            budget += per_tick;
            while budget >= chunk as f64 {
                // 写失败（连接中途断开）时如实停写，但要走到末尾把已写字节落盘。
                if stream.write_all(&buf).await.is_err() {
                    eprintln!("[uTP client] write 中断，已写 {bytes} bytes");
                    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
                    let l = format!("{{\"role\":\"{role}\",\"ip\":\"{ip}\",\"port\":{port},\"bytes\":{bytes},\"seconds\":{seconds},\"cap_mbps\":{cap_mbps},\"elapsed_secs\":{:.3},\"unix\":{}}}\n",
                                    start.elapsed().as_secs_f64(), s);
                    let _ = std::fs::write(&stat, &l);
                    return;
                }
                bytes += chunk as u64;
                budget -= chunk as f64;
            }
            // 每轮实时落盘，避免进程被 kill 时统计丢失。
            if bytes > 0 {
                let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
                let l = format!("{{\"role\":\"{role}\",\"ip\":\"{ip}\",\"port\":{port},\"bytes\":{bytes},\"seconds\":{seconds},\"cap_mbps\":{cap_mbps},\"elapsed_secs\":{:.3},\"unix\":{}}}\n",
                                start.elapsed().as_secs_f64(), s);
                let _ = std::fs::write(&stat, &l);
            }
            tick += Duration::from_millis(20);
            let now = Instant::now();
            if tick > now { sleep(tick - now).await; } else { tick = now; }
        }
        eprintln!("[uTP client] wrote {bytes} bytes in {:?}", start.elapsed());
        let _ = stream.shutdown().await;
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let line = format!("{{\"role\":\"{role}\",\"ip\":\"{ip}\",\"port\":{port},\"bytes\":{bytes},\"seconds\":{seconds},\"cap_mbps\":{cap_mbps},\"elapsed_secs\":{:.3},\"unix\":{}}}\n",
                       start.elapsed().as_secs_f64(), secs);
    let _ = std::fs::write(&stat, &line);
    println!("{line}");
}
