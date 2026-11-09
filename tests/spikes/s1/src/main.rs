//! ColdRevive M0 / S1 最小 Demo（本地回环，不接触公网）
//!
//! 观测四件事，绝不合成观测值：
//!   1. 拦截一次标准 68 字节 BitTorrent Handshake，提取 info_hash；
//!   2. 按 info_hash 动态加载伴随 .torrent，用 librqbit 以 paused=true 按需实例化；
//!   3. Session::unpause / pause 后读 is_paused 与 progress_bytes 位图；
//!   4. 对磁盘样本逐片 SHA-1，与伴随种子的 pieces 字段逐条比对。
//!
//! 红线：禁 DHT / tracker / LSD / 监听 / 外连；样本确定性生成、无版权内容；
//! 脚本绝不自动把 S1 置为 pass，结论由实验执行人依证据手工回填 spikes.json。

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use bytes::Bytes;
use librqbit::{AddTorrent, AddTorrentOptions, Session, SessionOptions};
use serde_json::json;
use sha1::{Digest, Sha1};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const PIECE_LEN: usize = 16 * 1024;
const SAMPLE_LEN: usize = 256 * 1024;
const PSTR: &[u8] = b"BitTorrent protocol";
const SAMPLE_NAME: &str = "coldrevive-s1-sample.bin";
const SEED: u64 = 20261109;

fn sha1_of(data: &[u8]) -> [u8; 20] {
    let mut h = Sha1::new();
    h.update(data);
    let digest = h.finalize();
    let mut out = [0u8; 20];
    out.copy_from_slice(&digest);
    out
}

fn bstr(data: &[u8]) -> Vec<u8> {
    let mut out = format!("{}:", data.len()).into_bytes();
    out.extend_from_slice(data);
    out
}

fn deterministic_sample(len: usize) -> Vec<u8> {
    let mut state = SEED;
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.push((state >> 24) as u8);
    }
    out
}

/// 生成单文件 .torrent（info 字典键按 bencode 字典序排列），返回种子字节、InfoHash、逐片 SHA-1。
fn build_torrent(data: &[u8]) -> (Vec<u8>, [u8; 20], Vec<[u8; 20]>) {
    let pieces: Vec<[u8; 20]> = data.chunks(PIECE_LEN).map(sha1_of).collect();
    let mut blob = Vec::with_capacity(pieces.len() * 20);
    for p in &pieces {
        blob.extend_from_slice(p);
    }

    let mut info = Vec::new();
    info.extend_from_slice(b"d");
    info.extend_from_slice(&bstr(b"length"));
    info.extend_from_slice(format!("i{}e", data.len()).as_bytes());
    info.extend_from_slice(&bstr(b"name"));
    info.extend_from_slice(&bstr(SAMPLE_NAME.as_bytes()));
    info.extend_from_slice(&bstr(b"piece length"));
    info.extend_from_slice(format!("i{}e", PIECE_LEN).as_bytes());
    info.extend_from_slice(&bstr(b"pieces"));
    info.extend_from_slice(&bstr(&blob));
    info.extend_from_slice(b"e");

    let info_hash = sha1_of(&info);

    let mut torrent = Vec::new();
    torrent.extend_from_slice(b"d");
    torrent.extend_from_slice(&bstr(b"announce"));
    torrent.extend_from_slice(&bstr(b"http://127.0.0.1:1/announce"));
    torrent.extend_from_slice(&bstr(b"info"));
    torrent.extend_from_slice(&info);
    torrent.extend_from_slice(b"e");
    (torrent, info_hash, pieces)
}

fn handshake_bytes(info_hash: &[u8; 20]) -> Vec<u8> {
    let mut v = Vec::with_capacity(68);
    v.push(19u8);
    v.extend_from_slice(PSTR);
    v.extend_from_slice(&[0u8; 8]);
    v.extend_from_slice(info_hash);
    v.extend_from_slice(b"-CR0001-20261109abcd");
    v
}

fn parse_handshake(buf: &[u8]) -> Result<([u8; 20], String), String> {
    if buf.len() != 68 {
        return Err(format!("handshake length {} != 68", buf.len()));
    }
    if buf[0] != 19 || &buf[1..20] != PSTR {
        return Err("not a standard BitTorrent handshake".into());
    }
    let mut ih = [0u8; 20];
    ih.copy_from_slice(&buf[28..48]);
    Ok((ih, String::from_utf8_lossy(&buf[48..68]).into_owned()))
}

fn stats_snapshot(h: &librqbit::ManagedTorrent) -> serde_json::Value {
    let s = h.stats();
    json!({
        "is_paused": h.is_paused(),
        "total_bytes": s.total_bytes,
        "progress_bytes": s.progress_bytes,
        "finished": s.finished,
        "file_progress": s.file_progress,
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let run_dir = root.join(".run");
    let out_dir = run_dir.join("out");
    let report_dir = root.join("reports");
    fs::create_dir_all(&out_dir)?;
    fs::create_dir_all(&report_dir)?;

    // --- 1. 确定性样本与伴随种子 -------------------------------------------------
    let sample = deterministic_sample(SAMPLE_LEN);
    fs::write(out_dir.join(SAMPLE_NAME), &sample)?;
    let (torrent_bytes, info_hash, expected_pieces) = build_torrent(&sample);
    let torrent_path = run_dir.join("sample.torrent");
    fs::write(&torrent_path, &torrent_bytes)?;

    // 模拟 ColdRevive 的「InfoHash -> 伴随种子」索引
    let mut index: HashMap<[u8; 20], PathBuf> = HashMap::new();
    index.insert(info_hash, torrent_path.clone());

    // --- 2. 标准 BT Handshake 拦截（127.0.0.1 回环）-----------------------------
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let peer = tokio::spawn(async move {
        let mut s = TcpStream::connect(addr).await?;
        s.write_all(&handshake_bytes(&info_hash)).await?;
        s.flush().await?;
        let mut ack = [0u8; 1];
        let _ = s.read(&mut ack).await;
        Ok::<(), std::io::Error>(())
    });

    let (mut stream, peer_addr) = listener.accept().await?;
    let mut buf = [0u8; 68];
    stream.read_exact(&mut buf).await?;
    let (observed_ih, peer_id) = parse_handshake(&buf)
        .map_err(|e| anyhow::anyhow!("handshake parse failed: {e}"))?;
    stream.write_all(b"\x01").await?;
    peer.await??;
    drop(listener);

    let intercept_ok = observed_ih == info_hash;

    // --- 3. 按 info_hash 动态加载伴随种子，并按需实例化 ---------------------------
    let loaded_path: Option<&Path> = index.get(&observed_ih).map(|p| p.as_path());
    let loaded_torrent: Option<Vec<u8>> = match loaded_path {
        Some(p) => Some(fs::read(p)?),
        None => None,
    };
    let on_demand_ok = loaded_torrent.is_some();

    let opts = SessionOptions {
        dht: None,
        disable_trackers: true,
        disable_local_service_discovery: true,
        listen: None,
        connect: None,
        ipv4_only: true,
        ..Default::default()
    };
    let session = Session::new_with_opts(out_dir.clone(), opts).await?;

    let add_opts = AddTorrentOptions {
        paused: true,
        // 样本文件已预置在输出目录，需允许覆盖以便校验既有分片位图。
        overwrite: true,
        disable_trackers: true,
        ..Default::default()
    };
    let handle = session
        .add_torrent(
            AddTorrent::TorrentFileBytes(Bytes::from(
                loaded_torrent.clone().expect("torrent must be loaded by info_hash"),
            )),
            Some(add_opts),
        )
        .await?
        .into_handle()
        .expect("add_torrent must return a ManagedTorrent handle");
    handle.wait_until_initialized().await?;

    // --- 4. Pause / Resume 状态与分片位图 ----------------------------------------
    let s_initial = stats_snapshot(&handle);
    session.unpause(&handle).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let s_unpaused = stats_snapshot(&handle);

    session.pause(&handle).await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let s_paused_again = stats_snapshot(&handle);

    session.unpause(&handle).await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let s_resumed = stats_snapshot(&handle);

    // --- 5. 逐片 SHA-1 校验 ------------------------------------------------------
    let on_disk = fs::read(out_dir.join(SAMPLE_NAME))?;
    let computed: Vec<[u8; 20]> = on_disk.chunks(PIECE_LEN).map(sha1_of).collect();
    let mismatches = computed
        .iter()
        .zip(expected_pieces.iter())
        .filter(|(a, b)| a != b)
        .count();
    let piece_check_ok =
        computed.len() == expected_pieces.len() && mismatches == 0 && on_disk == sample;

    session.stop().await;

    // --- 6. 报告（只记录观测，不产出 Spike 结论）---------------------------------
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let bitmap_kept = s_initial["progress_bytes"] == s_resumed["progress_bytes"]
        && s_unpaused["progress_bytes"] == s_resumed["progress_bytes"]
        && s_paused_again["is_paused"] == true
        && s_unpaused["is_paused"] == false;

    let report = json!({
        "schema": "coldrevive.m0.s1-probe/v1",
        "generated_at_unix": ts,
        "scope": "127.0.0.1 回环；DHT/tracker/LSD/监听/外连全部禁用；不接触公共网络",
        "engine": { "crate": "librqbit", "version": "9.1.0", "toolchain": "rustc 1.99.1" },
        "sample": {
            "bytes": SAMPLE_LEN,
            "piece_length": PIECE_LEN,
            "piece_count": expected_pieces.len(),
            "seed": SEED,
            "info_hash": hex::encode(info_hash),
            "sample_sha1": hex::encode(sha1_of(&sample)),
        },
        "observations": {
            "handshake": {
                "peer_addr": peer_addr.to_string(),
                "bytes_received": 68,
                "pstrlen": buf[0],
                "protocol": String::from_utf8_lossy(&buf[1..20]).into_owned(),
                "info_hash": hex::encode(observed_ih),
                "peer_id": peer_id,
                "matched_expected": intercept_ok,
            },
            "on_demand_load": {
                "torrent_path": loaded_path.map(|p| p.to_string_lossy().into_owned()),
                "torrent_bytes": loaded_torrent.as_ref().map(|b| b.len()),
                "instantiated_paused": s_initial["is_paused"],
                "resolved_by_info_hash": on_demand_ok,
            },
            "pause_resume": {
                "initial": s_initial,
                "after_unpause": s_unpaused,
                "after_pause_again": s_paused_again,
                "after_resume": s_resumed,
                "bitmap_kept": bitmap_kept,
            },
            "piece_sha1": {
                "pieces_computed": computed.len(),
                "pieces_expected": expected_pieces.len(),
                "mismatches": mismatches,
                "all_match": piece_check_ok,
            },
        },
        "api_probe": {
            "rqbit_public_handshake_hook": false,
            "note": "SessionOptions 无握手回调字段；session.rs::task_listener 内部 read_handshake 后按 info_hash 查已有 torrent，不向调用方暴露拦截点。ColdRevive 的握手拦截器须自实现（本 Demo 即自实现），或在 rqbit 之外加代理层。",
        },
        "criteria": [
            { "id": "S1-C1", "text": "最小 Demo 能拦截一次标准 BT Handshake",
              "status": if intercept_ok { "verified" } else { "not_verified" },
              "note": "由自实现拦截层完成；rqbit 9.1.0 未暴露公开握手钩子，见 api_probe" },
            { "id": "S1-C2", "text": "拦截后能动态加载伴随 .torrent 并按需实例化 TorrentContext",
              "status": if on_demand_ok && s_initial["is_paused"] == true { "verified" } else { "not_verified" },
              "note": "按 info_hash 查索引加载种子字节，AddTorrent::TorrentFileBytes + paused=true 实例化" },
            { "id": "S1-C3", "text": "Pause / Resume 后状态可恢复且不丢分片位图",
              "status": if bitmap_kept { "partial" } else { "not_verified" },
              "note": "已观测 is_paused 切换与 progress_bytes 保持；但样本文件预先存在、位图始终完整，未覆盖『传输中途暂停』场景" },
            { "id": "S1-C4", "text": "完成一次真实分片传输并通过逐片 SHA-1 校验",
              "status": if piece_check_ok { "partial" } else { "not_verified" },
              "note": "逐片 SHA-1 校验通过；未建立双 peer 真实分片传输，该半项未验证" },
        ],
        "overall": "partial",
        "s1_status": "not_run",
        "conclusion_rule": "脚本禁止自动置 pass；结论须由实验执行人按证据手工回填 spikes.json",
    });

    let path = report_dir.join(format!("s1-probe-{ts}.json"));
    fs::write(&path, serde_json::to_string_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    eprintln!("[S1] report: {}", path.display());
    Ok(())
}
