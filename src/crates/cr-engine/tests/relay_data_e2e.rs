//! ADR-023：证明 BT 字节**确实经中继帧**送达真引擎，而不是回落直连。
//!
//! 与 ADR-021 的桥 e2e 同样用 librqbit 的 SOCKS5 官方钩子，但上游不是 punched socket，
//! 而是 `cr-nat::relay_tunnel` 提供的 duplex。中继是进程内真 `RelayServer`，
//! 两端各自真 register，帧走真 `RELAY`/`DELIVERED`。
//!
//! 范围：这是**交接与承载证据**，不是 NAT 可达证据；`is_s6_evidence=false`。

use std::io::{ErrorKind, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cr_bridge::Bridge;
use cr_engine::{EngineOptions, RqbitEngine, TorrentSpec};
use cr_nat::relay::{RelayClient, NODE_ID_LEN};
use cr_nat::relay_server::RelayServer;
use cr_nat::relay_tunnel::{RelayTunnel, RelayTunnelSet};
use librqbit::{create_torrent, spawn_utils::BlockingSpawner, CreateTorrentOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, copy_bidirectional};
use tokio::net::{TcpListener, TcpStream};

const SAMPLE_NAME: &str = "relay-data.bin";
const SAMPLE_LEN: usize = 1 << 20;
const PIECE_LEN: u32 = 64 * 1024;
const CHANNEL: u32 = 0x52514c31; // "RQL1"
const SIDE_A: [u8; NODE_ID_LEN] = [0xA1; NODE_ID_LEN];
const SIDE_B: [u8; NODE_ID_LEN] = [0xB2; NODE_ID_LEN];

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cr-relay-e2e-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sample_bytes() -> Vec<u8> {
    (0..SAMPLE_LEN).map(|i| (i % 241) as u8).collect()
}

fn make_torrent(sample_path: &std::path::Path) -> Vec<u8> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let spawner = BlockingSpawner::new(1);
        create_torrent(
            sample_path,
            CreateTorrentOptions {
                name: Some(SAMPLE_NAME),
                trackers: Vec::new(),
                piece_length: Some(PIECE_LEN),
            },
            &spawner,
        )
        .await
    })
    .unwrap()
    .as_bytes()
    .unwrap()
    .to_vec()
}

fn engine_opts(dir: &PathBuf, proxy: Option<String>) -> EngineOptions {
    let mut opts = EngineOptions::new(dir);
    opts.listen = None;
    opts.enable_dht = false;
    opts.enable_trackers = false;
    opts.enable_utp = false;
    opts.proxy_url = proxy;
    opts.init_timeout = Duration::from_secs(30);
    opts
}

fn torrent_spec<'a>(bytes: &'a [u8], dir: &PathBuf, peer: Option<SocketAddr>) -> TorrentSpec<'a> {
    TorrentSpec {
        name: SAMPLE_NAME,
        bytes,
        output_dir: Some(dir.clone()),
        initial_peers: peer.into_iter().collect(),
        paused: false,
        overwrite: true,
        disable_trackers: true,
    }
}

/// 把 std 阻塞的 `RelayTunnel` 和 tokio 的 `DuplexStream` 对接。
///
/// 这个 tokio 版本的 DuplexStream 没有公开的 try_read/try_write，所以在专用线程里
/// 起一个 current_thread runtime：duplex 侧用 1 ms 超时轮询，tunnel 侧用非阻塞
/// `try_read`。两边都不会饿死；不在共享 async runtime 上做阻塞读。
fn pump_duplex_tunnel(duplex: DuplexStream, tunnel: RelayTunnel) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async move {
        let mut duplex = duplex;
        let mut tunnel = tunnel;
        let mut buf = [0u8; 16 * 1024];
        loop {
            let mut progress = false;
            match tokio::time::timeout(Duration::from_millis(1), duplex.read(&mut buf)).await {
                Ok(Ok(0)) => break, // 对端关闭
                Ok(Ok(n)) => {
                    if tunnel.write_all(&buf[..n]).is_err() {
                        break;
                    }
                    progress = true;
                }
                Ok(Err(_)) => break,
                Err(_) => {} // 本轮 duplex 没数据
            }
            match tunnel.try_read(&mut buf) {
                Ok(n) if n > 0 => {
                    if tokio::time::timeout(Duration::from_secs(2), duplex.write_all(&buf[..n]))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    progress = true;
                }
                Ok(0) => break, // 对端 CLOSE → EOF
                Ok(_) => {}
                Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                Err(_) => break,
            }
            if !progress {
                tokio::task::yield_now().await;
            }
        }
    });
}

#[test]
fn relay_frames_carry_real_librqbit_piece() {
    let a_dir = tmp_dir("seed");
    let b_dir = tmp_dir("download");
    let b_neg_dir = tmp_dir("negative");
    let sample_path = a_dir.join(SAMPLE_NAME);
    let data = sample_bytes();
    std::fs::write(&sample_path, &data).unwrap();
    let torrent = make_torrent(&sample_path);

    // 做种方：真引擎、真监听。
    let mut seed_opts = engine_opts(&a_dir, None);
    seed_opts.listen = Some("127.0.0.1:0".parse().unwrap());
    let mut seeder = RqbitEngine::new(seed_opts).expect("做种引擎必须启动");
    let seeder_addr = seeder.listen_addr().expect("做种方必须真监听");
    let seed_task = seeder
        .add_torrent_with_timeout(&torrent_spec(&torrent, &a_dir, None), Duration::from_secs(30))
        .expect("做种任务必须加入");

    // 进程内真中继 + 两端真注册。
    let server = RelayServer::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    let handle = server.spawn();
    let addr = format!("127.0.0.1:{port}");
    let timeout = Duration::from_secs(2);
    let mut ca = RelayClient::new(addr.clone()).connect(timeout).unwrap();
    ca.register(&SIDE_A).unwrap();
    let mut cb = RelayClient::new(addr).connect(timeout).unwrap();
    cb.register(&SIDE_B).unwrap();
    let set_a = RelayTunnelSet::new(ca, SIDE_B).unwrap();
    let set_b = RelayTunnelSet::new(cb, SIDE_A).unwrap();
    let tunnel_a = set_a.open(CHANNEL).unwrap();
    let tunnel_b = set_b.open(CHANNEL).unwrap();

    let bridge_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();

    // 做种侧：tunnel → duplex → 引擎监听口，引擎看到的是普通入站连接。
    let (seed_duplex, seed_tunnel_side) = tokio::io::duplex(64 * 1024);
    bridge_rt.spawn(async move {
        if let Ok(mut to_engine) = TcpStream::connect(seeder_addr).await {
            let mut s = seed_tunnel_side;
            let _ = copy_bidirectional(&mut s, &mut to_engine).await;
        }
    });
    thread::Builder::new()
        .name("relay-e2e-seed-pump".into())
        .spawn(move || pump_duplex_tunnel(seed_duplex, tunnel_b))
        .unwrap();

    // SOCKS5 桥 + 下载侧 tunnel 登记。目标用不可直连的 TEST-NET-3：
    // 若回落真实直连，下载必然为 0。
    let bridge = Arc::new(Bridge::new());
    let listener = bridge_rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
    let bridge_addr = listener.local_addr().unwrap();
    let b = bridge.clone();
    bridge_rt.spawn(async move { let _ = b.serve_listener(listener).await; });

    let (client_duplex, bridge_tunnel_side) = tokio::io::duplex(64 * 1024);
    let advertised: SocketAddr = format!("203.0.113.31:{}", seeder_addr.port()).parse().unwrap();
    bridge.register_tunnel(advertised, bridge_tunnel_side).unwrap();
    thread::Builder::new()
        .name("relay-e2e-client-pump".into())
        .spawn(move || pump_duplex_tunnel(client_duplex, tunnel_a))
        .unwrap();

    let proxy = Some(format!("socks5://{bridge_addr}"));
    let mut downloader = RqbitEngine::new(engine_opts(&b_dir, proxy.clone())).unwrap();
    let task = downloader
        .add_torrent_with_timeout(
            &torrent_spec(&torrent, &b_dir, Some(advertised)),
            Duration::from_secs(30),
        )
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(90);
    let mut last = None;
    while Instant::now() < deadline {
        let p = downloader.sample(task).expect("任务必须仍在引擎里");
        last = Some(p.clone());
        if p.finished && p.downloaded_bytes >= SAMPLE_LEN as u64 {
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    let progress = last.expect("至少采样一次");
    println!(
        "RELAY_DATA_E2E served_registered={} downloaded={} finished={}",
        bridge.served_registered(),
        progress.downloaded_bytes,
        progress.finished
    );

    assert_eq!(bridge.served_registered(), 1, "必须命中中继 tunnel 登记项");
    assert!(progress.downloaded_bytes > 0, "真引擎必须经中继下行字节");
    assert!(progress.finished, "样本必须下载完成");
    assert!(seeder.sample(seed_task).unwrap().uploaded_bytes > 0, "做种方必须有上行");
    assert_eq!(
        std::fs::read(b_dir.join(SAMPLE_NAME)).unwrap(),
        data,
        "经中继落盘的内容必须逐字节一致"
    );

    // 负对照：同一桥、同一样本，但不登记 tunnel。假地址不可直连 ⇒ 0 B。
    let advertised_neg: SocketAddr = format!("203.0.113.32:{}", seeder_addr.port()).parse().unwrap();
    let mut negative = RqbitEngine::new(engine_opts(&b_neg_dir, proxy)).unwrap();
    let neg_task = negative
        .add_torrent_with_timeout(
            &torrent_spec(&torrent, &b_neg_dir, Some(advertised_neg)),
            Duration::from_secs(30),
        )
        .unwrap();
    thread::sleep(Duration::from_secs(3));
    let neg_progress = negative.sample(neg_task).unwrap();
    assert_eq!(
        neg_progress.downloaded_bytes, 0,
        "未登记 tunnel 时不得靠回落直连拿到字节"
    );
    assert_eq!(bridge.served_registered(), 1, "负对照不得计入桥命中");

    if let Ok(path) = std::env::var("CR_RELAY_E2E_REPORT") {
        let stats = handle.stats();
        let report = format!(concat!(
            "{{\n",
            "  \"schema\": \"coldrevive.relay-data-e2e/v1\",\n",
            "  \"served_registered\": {},\n",
            "  \"downloaded_bytes\": {},\n",
            "  \"finished\": {},\n",
            "  \"byte_equal\": true,\n",
            "  \"negative_downloaded_bytes\": {},\n",
            "  \"relay_delivered\": {},\n",
            "  \"relay_data_bytes_admitted\": {},\n",
            "  \"scope\": \"in-process real relay; real librqbit; not NAT evidence\",\n",
            "  \"relay_data_plane\": \"implemented-over-tcp-carrier\",\n",
            "  \"is_s6_evidence\": false,\n",
            "  \"g2_claim\": false,\n",
            "  \"s6_status\": \"not_run\"\n",
            "}}\n"),
            bridge.served_registered(),
            progress.downloaded_bytes,
            progress.finished,
            neg_progress.downloaded_bytes,
            stats.delivered,
            stats.data_bytes_admitted,
        );
        std::fs::write(&path, report).unwrap();
        println!("RELAY_E2E_REPORT={path}");
    }

    handle.stop();
}
