//! ADR-021 / W3.17：cr-bridge 真引擎端到端。
//!
//! 这不是 cr-bridge 的字节回环单测：两端都是真的 librqbit 会话，样本是真的落盘文件，
//! 下载方只拿到一个**不可直连的假 peer 地址**；只有 SOCKS5 桥命中登记表、把外部
//! 建立的 socket 交给引擎，才能真正取到片。未登记的对照同一地址拿不到任何字节。
//!
//! 注意范围：这是回环上的「handoff 能力」验收，不是 NAT 打洞证据，也不是 S6 门禁证据。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cr_bridge::Bridge;
use cr_engine::{EngineOptions, RqbitEngine, TorrentSpec};
use librqbit::{create_torrent, spawn_utils::BlockingSpawner, CreateTorrentOptions};
use tokio::net::{TcpListener, TcpStream as TokioTcpStream};

const SAMPLE_NAME: &str = "bridge-e2e.bin";
const SAMPLE_LEN: usize = 1 << 20; // 1 MiB，64 KiB 一片 → 16 片
const PIECE_LEN: u32 = 64 * 1024;

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "cr-bridge-e2e-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sample_bytes() -> Vec<u8> {
    (0..SAMPLE_LEN).map(|i| (i % 251) as u8).collect()
}

fn seeder_spec<'a>(bytes: &'a [u8], dir: &PathBuf) -> TorrentSpec<'a> {
    TorrentSpec {
        name: SAMPLE_NAME,
        bytes,
        output_dir: Some(dir.clone()),
        initial_peers: Vec::new(),
        paused: false,
        overwrite: true,
        disable_trackers: true,
    }
}

fn downloader_spec<'a>(
    bytes: &'a [u8],
    dir: &PathBuf,
    peer: SocketAddr,
) -> TorrentSpec<'a> {
    TorrentSpec {
        name: SAMPLE_NAME,
        bytes,
        output_dir: Some(dir.clone()),
        initial_peers: vec![peer],
        paused: false,
        overwrite: true,
        disable_trackers: true,
    }
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

fn make_torrent(sample_path: &std::path::Path) -> Vec<u8> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = rt.block_on(async {
        // BlockingSpawner 内部要抓当前 reactor，必须在 runtime context 里造。
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
    });
    result.unwrap().as_bytes().unwrap().to_vec()
}

#[test]
fn registered_socket_carries_real_librqbit_piece() {
    let a_dir = tmp_dir("seed");
    let b_dir = tmp_dir("download");
    let b_neg_dir = tmp_dir("negative");
    let sample_path = a_dir.join(SAMPLE_NAME);
    let data = sample_bytes();
    std::fs::write(&sample_path, &data).unwrap();
    let torrent = make_torrent(&sample_path);

    // 做种方：真引擎、真监听、禁用 DHT/tracker/uTP。
    let mut seed_opts = engine_opts(&a_dir, None);
    seed_opts.listen = Some("127.0.0.1:0".parse().unwrap());
    let mut seeder = RqbitEngine::new(seed_opts).expect("带监听的做种引擎必须启动");
    let seeder_addr = seeder.listen_addr().expect("做种方必须真的监听");
    let seed_task = seeder
        .add_torrent_with_timeout(&seeder_spec(&torrent, &a_dir), Duration::from_secs(30))
        .expect("做种任务必须加入");

    // SOCKS5 桥：独立 runtime，生命周期覆盖两个下载会话。
    // 必须是 multi_thread：bridge 的 accept 循环要在测试主线程做同步引擎轮询时继续跑。
    let bridge_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let bridge = Arc::new(Bridge::new());
    let listener = bridge_rt
        .block_on(TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let bridge_addr = listener.local_addr().unwrap();
    let server = bridge.clone();
    bridge_rt.spawn(async move {
        let _ = server.serve_listener(listener).await;
    });

    // 外部建立的一条真 TCP 连接（走 cr-nat 的打洞原语），引擎自己并不知道它。
    let punch = cr_nat::tcp_punch::punch_tcp(
        "127.0.0.1:0".parse().unwrap(),
        &[seeder_addr],
        Duration::from_secs(3),
    );
    let punched = punch.established.expect("回环打洞必须建立真连接");
    punched.stream.set_nonblocking(true).unwrap();
    let registered = bridge_rt
        .block_on(async { TokioTcpStream::from_std(punched.stream).unwrap() });

    // 关键：交给引擎的地址是**不可直连**的 TEST-NET-3。
    // 若桥没命中而回落直连，下载必然为 0，不能靠「自己做种自己连」蒙混过关。
    let advertised: SocketAddr = format!("203.0.113.9:{}", seeder_addr.port()).parse().unwrap();
    bridge.register(advertised, registered).unwrap();

    let proxy = Some(format!("socks5://{bridge_addr}"));
    let mut downloader = RqbitEngine::new(engine_opts(&b_dir, proxy.clone()))
        .expect("下载引擎必须启动");
    let task = downloader
        .add_torrent_with_timeout(
            &downloader_spec(&torrent, &b_dir, advertised),
            Duration::from_secs(30),
        )
        .expect("下载任务必须加入");

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
        "BRIDGE_E2E registered_target={advertised} downloaded={} total={} finished={} served_registered={}",
        progress.downloaded_bytes,
        progress.total_bytes,
        progress.finished,
        bridge.served_registered()
    );

    assert_eq!(bridge.served_registered(), 1, "必须命中登记表，不能回落直连");
    assert!(progress.downloaded_bytes > 0, "真引擎必须经桥下行字节");
    assert!(progress.finished, "样本必须下载完成");
    assert!(
        seeder.sample(seed_task).unwrap().uploaded_bytes > 0,
        "做种方必须观测到上行"
    );
    let downloaded = std::fs::read(b_dir.join(SAMPLE_NAME)).expect("落盘文件必须存在");
    assert_eq!(downloaded, data, "落盘内容必须与样本逐字节一致");

    // 负对照：同一桥、同一做种方，但不登记。假地址不可直连 ⇒ 不得有一字节下行。
    let advertised_neg: SocketAddr =
        format!("203.0.113.10:{}", seeder_addr.port()).parse().unwrap();
    let mut negative = RqbitEngine::new(engine_opts(&b_neg_dir, proxy.clone()))
        .expect("负对照引擎必须启动");
    let neg_task = negative
        .add_torrent_with_timeout(
            &downloader_spec(&torrent, &b_neg_dir, advertised_neg),
            Duration::from_secs(30),
        )
        .expect("负对照任务必须加入");
    let neg_deadline = Instant::now() + Duration::from_secs(8);
    let mut neg_downloaded = 0;
    while Instant::now() < neg_deadline {
        neg_downloaded = negative.sample(neg_task).unwrap().downloaded_bytes;
        if neg_downloaded > 0 {
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    println!(
        "BRIDGE_E2E_NEGATIVE target={advertised_neg} downloaded={neg_downloaded} served_registered={}",
        bridge.served_registered()
    );
    assert_eq!(neg_downloaded, 0, "未登记且不可直连时不得取到任何字节");
    assert_eq!(bridge.served_registered(), 1, "负对照不得计入桥命中");

    if let Ok(path) = std::env::var("CR_BRIDGE_E2E_REPORT") {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let report = format!(
            concat!(
                "{{\n",
                "  \"schema\": \"coldrevive.cr-bridge-e2e/v1\",\n",
                "  \"generated_at_unix\": {now},\n",
                "  \"scope\": \"127.0.0.1 loopback; real librqbit; no NAT evidence\",\n",
                "  \"proxy_url\": \"{proxy_url}\",\n",
                "  \"registered_target\": \"{advertised}\",\n",
                "  \"downloaded_bytes\": {downloaded_bytes},\n",
                "  \"total_bytes\": {total_bytes},\n",
                "  \"finished\": {finished},\n",
                "  \"byte_equal\": true,\n",
                "  \"uploaded_bytes\": {uploaded_bytes},\n",
                "  \"served_registered\": {served_registered},\n",
                "  \"negative_target\": \"{advertised_neg}\",\n",
                "  \"negative_downloaded_bytes\": {neg_downloaded},\n",
                "  \"is_s6_evidence\": false,\n",
                "  \"g2_claim\": false,\n",
                "  \"s6_status\": \"not_run\"\n",
                "}}\n"
            ),
            now = now,
            proxy_url = proxy.as_deref().unwrap_or(""),
            advertised = advertised,
            advertised_neg = advertised_neg,
            downloaded_bytes = progress.downloaded_bytes,
            total_bytes = progress.total_bytes,
            finished = progress.finished,
            neg_downloaded = neg_downloaded,
            uploaded_bytes = seeder.sample(seed_task).unwrap().uploaded_bytes,
            served_registered = bridge.served_registered(),
        );
        std::fs::write(&path, report).unwrap();
        println!("BRIDGE_E2E_REPORT={path}");
    }

    negative.shutdown();
    downloader.shutdown();
    seeder.shutdown();
}

/// DOC-30 发现端口路径用的是 `connect_bound_blocking`（从固定源端口发起真连接），
/// 而不是 w54 的 `punch_tcp`（临时源端口）。这里钉住：绑定源端口建成的真 socket
/// 同样能登记进 SOCKS5 桥、被真 librqbit 命中并下完整片，且登记键可与 SYN 目的
/// 地址解耦（v2.4.123 的 PunchTargets 不变量）。回环范围，不是 NAT/G2 证据。
#[test]
fn bound_source_port_socket_carries_real_librqbit_piece() {
    let a_dir = tmp_dir("bound-seed");
    let b_dir = tmp_dir("bound-download");
    let sample_path = a_dir.join(SAMPLE_NAME);
    let data = sample_bytes();
    std::fs::write(&sample_path, &data).unwrap();
    let torrent = make_torrent(&sample_path);

    let mut seed_opts = engine_opts(&a_dir, None);
    seed_opts.listen = Some("127.0.0.1:0".parse().unwrap());
    let mut seeder = RqbitEngine::new(seed_opts).expect("做种引擎必须启动");
    let seeder_addr = seeder.listen_addr().expect("做种方必须真监听");
    let _seed_task = seeder
        .add_torrent_with_timeout(&seeder_spec(&torrent, &a_dir), Duration::from_secs(30))
        .expect("做种任务加入");

    // 先占一个临时端口拿到确定值，再释放给绑定源端口连接使用。
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let bound_port = probe.local_addr().unwrap().port();
    drop(probe);
    let bound_local: SocketAddr = format!("127.0.0.1:{bound_port}").parse().unwrap();

    let socket = cr_nat::tcp_punch::connect_bound_blocking(
        bound_local,
        seeder_addr,
        Duration::from_secs(3),
    )
    .expect("绑定源端口的真 TCP 连接必须建立");

    let bridge_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let bridge = Arc::new(Bridge::new());
    let listener = bridge_rt
        .block_on(TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let bridge_addr = listener.local_addr().unwrap();
    let server = bridge.clone();
    bridge_rt.spawn(async move {
        let _ = server.serve_listener(listener).await;
    });

    // 登记键用不可直连的 TEST-NET-3（与 SYN 目的 seeder_addr 解耦），
    // 引擎只按该键经桥取片；若桥 miss 回落直连则下载必然为 0。
    let advertised: SocketAddr = format!("203.0.113.11:{}", seeder_addr.port()).parse().unwrap();
    socket.set_nonblocking(true).unwrap();
    let registered = bridge_rt
        .block_on(async { TokioTcpStream::from_std(socket).unwrap() });
    bridge.register(advertised, registered).unwrap();

    let proxy = Some(format!("socks5://{bridge_addr}"));
    let mut downloader = RqbitEngine::new(engine_opts(&b_dir, proxy.clone()))
        .expect("下载引擎必须启动");
    let task = downloader
        .add_torrent_with_timeout(
            &downloader_spec(&torrent, &b_dir, advertised),
            Duration::from_secs(30),
        )
        .expect("下载任务加入");

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
        "BOUND-PORT BRIDGE_E2E bound_port={bound_port} registered_target={advertised} downloaded={} finished={} served_registered={}",
        progress.downloaded_bytes, progress.finished, bridge.served_registered()
    );

    assert_eq!(bridge.served_registered(), 1, "必须命中登记表，不能回落直连");
    assert!(progress.downloaded_bytes > 0, "绑定源端口的 socket 必须经桥下行字节");
    assert!(progress.finished, "样本必须下载完成");
    let downloaded = std::fs::read(b_dir.join(SAMPLE_NAME)).expect("落盘文件必须存在");
    assert_eq!(downloaded, data, "落盘内容必须与样本逐字节一致");

    if let Ok(path) = std::env::var("CR_BOUND_PORT_E2E_REPORT") {
        let report = format!(
            concat!(
                "{{\n",
                "  \"schema\": \"coldrevive.bound-port-bridge-e2e/v1\",\n",
                "  \"scope\": \"127.0.0.1 loopback; real librqbit; bound source port; no NAT evidence\",\n",
                "  \"bound_source_port\": {bound_port},\n",
                "  \"registered_target\": \"{advertised}\",\n",
                "  \"downloaded_bytes\": {downloaded_bytes},\n",
                "  \"finished\": {finished},\n",
                "  \"byte_equal\": true,\n",
                "  \"served_registered\": {served_registered},\n",
                "  \"is_s6_evidence\": false,\n",
                "  \"g2_claim\": false,\n",
                "  \"s6_status\": \"not_run\"\n",
                "}}\n"
            ),
            bound_port = bound_port,
            advertised = advertised,
            downloaded_bytes = progress.downloaded_bytes,
            finished = progress.finished,
            served_registered = bridge.served_registered(),
        );
        std::fs::write(&path, report).unwrap();
        println!("CR_BOUND_PORT_E2E_REPORT={path}");
    }

    downloader.shutdown();
    seeder.shutdown();
}
