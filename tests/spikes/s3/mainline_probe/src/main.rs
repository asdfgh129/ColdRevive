//! S3 public Mainline DHT probe.
//!
//! Uses the real librqbit-dht 9.1.0 implementation already locked by the workspace.
//! announce role: join public DHT and announce a deterministic test infohash.
//! query role:    join public DHT and get_peers for the same infohash WITHOUT announcing.
//!
//! No private bootstrap nodes are used. No product key is published: the infohash is derived
//! from an explicitly labelled S3 test string.

use std::collections::BTreeSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use librqbit_dht::{DhtBuilder, DhtConfig, Id20};
use tokio::time::{sleep, timeout, Instant};

fn opt(args: &[String], name: &str, default: &str) -> String {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| default.to_string())
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

/// Deterministic 20-byte test infohash from a label. This is NOT a product resource hash.
fn test_infohash(label: &str) -> Id20 {
    let mut out = [0u8; 20];
    let mut h: u64 = 0xcbf29ce4_84222325;
    for (i, b) in label.as_bytes().iter().enumerate() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x00000100_000001b3);
        for j in 0..8 {
            let idx = (i * 7 + j) % 20;
            out[idx] ^= ((h >> (j * 8)) & 0xff) as u8;
        }
    }
    // Second pass so short labels still diffuse across all 20 bytes.
    let mut h2: u64 = 0x9e3779b9_7f4a7c15;
    for b in out {
        h2 ^= b as u64;
        h2 = h2.wrapping_mul(0x00000100_000001b3);
    }
    for j in 0..8 {
        out[(j * 3 + 1) % 20] ^= ((h2 >> (j * 8)) & 0xff) as u8;
    }
    Id20::new(out)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let role = args.get(1).cloned().unwrap_or_else(|| "announce".to_string());
    if role != "announce" && role != "query" {
        return Err(format!("usage: mainline-probe announce|query [options]").into());
    }
    if has_flag(&args, "--attest-public-key-test") == false {
        return Err("refusing to contact public DHT without --attest-public-key-test".into());
    }

    let label = opt(&args, "--key-label", "coldrevive:s3:public-mainline:20261117");
    let seconds: u64 = opt(&args, "--seconds", "45").parse()?;
    let expect_ip = opt(&args, "--expect-ip", "");
    let out = opt(&args, "--out", "");

    let info_hash = test_infohash(&label);
    let bootstrap: Vec<String> = if let Some(i) = args.iter().position(|a| a == "--bootstrap") {
        args.get(i + 1)
            .ok_or("--bootstrap requires comma-separated addrs")?
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect()
    } else {
        vec![
            "router.bittorrent.com:6881".to_string(),
            "router.utorrent.com:6881".to_string(),
            "dht.transmissionbt.com:6881".to_string(),
            "dht.libtorrent.org:25401".to_string(),
            "router.bitcomet.com:6881".to_string(),
            "dht.aelitis.com:6881".to_string(),
        ]
    };
    let listen_port: u16 = if role == "announce" {
        opt(&args, "--listen-port", "18081").parse()?
    } else {
        0
    };
    let announce_port: Option<u16> = if role == "announce" {
        Some(opt(&args, "--announce-port", &listen_port.to_string()).parse()?)
    } else {
        None
    };

    let config = DhtConfig {
        bootstrap_addrs: Some(bootstrap.clone()),
        listen_addr: Some(([0, 0, 0, 0], listen_port).into()),
        ..Default::default()
    };
    let dht = DhtBuilder::with_config(config).await?;

    // Wait until the routing table has real public-node responses.
    let boot_start = Instant::now();
    let mut boot_secs = 0u64;
    while boot_secs < 60 {
        let st = dht.stats();
        if st.routing_table_size >= 8 || st.routing_table_size_v6 >= 4 {
            break;
        }
        sleep(Duration::from_secs(1)).await;
        boot_secs = boot_start.elapsed().as_secs();
    }
    let boot_stats = dht.stats();

    let mut peers = BTreeSet::new();
    let mut stream = dht.get_peers(info_hash, announce_port);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        let remain = match deadline.checked_duration_since(Instant::now()) {
            Some(d) if d > Duration::from_millis(0) => d,
            _ => break,
        };
        match timeout(remain, stream.next()).await {
            Ok(Some(addr)) => {
                peers.insert(addr.to_string());
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    drop(stream);

    let final_stats = dht.stats();
    let peer_vec: Vec<String> = peers.into_iter().collect();
    let expected_hit = if expect_ip.is_empty() {
        None
    } else {
        Some(peer_vec.iter().any(|p| p.contains(&expect_ip)))
    };
    let public_contacted =
        boot_stats.routing_table_size > 0 || boot_stats.routing_table_size_v6 > 0;

    let report = serde_json::json!({
        "spike": "S3",
        "check": "mainline-dht-public-key",
        "role": role,
        "recorded_at_unix": unix_now(),
        "key_label": label,
        "info_hash": info_hash.as_string(),
        "dht_node_id": final_stats.id.as_string(),
        "listen_addr": dht.listen_addr().to_string(),
        "announce_port": announce_port,
        "bootstrap_addrs": bootstrap,
        "bootstrap_wait_secs": boot_secs,
        "bootstrap_routing_table_size": boot_stats.routing_table_size,
        "bootstrap_routing_table_size_v6": boot_stats.routing_table_size_v6,
        "final_routing_table_size": final_stats.routing_table_size,
        "final_routing_table_size_v6": final_stats.routing_table_size_v6,
        "outstanding_requests": final_stats.outstanding_requests,
        "peer_count": peer_vec.len(),
        "peers": peer_vec,
        "expect_ip": if expect_ip.is_empty() { None } else { Some(&expect_ip) },
        "expected_ip_hit": expected_hit,
        "public_dht_contacted": public_contacted,
        "evidence_valid": public_contacted,
        "evidence_valid_for_pass": false,
        "spike_status_changed": false,
        "g2_claim": false,
        "auto_pass": false,
        "scope": "public Mainline DHT via librqbit-dht 9.1.0; deterministic S3 test infohash only; no product key"
    });
    let text = serde_json::to_string_pretty(&report)?;
    if !out.is_empty() {
        std::fs::write(&out, format!("{text}\n"))?;
    }
    println!("{text}");
    Ok(())
}
