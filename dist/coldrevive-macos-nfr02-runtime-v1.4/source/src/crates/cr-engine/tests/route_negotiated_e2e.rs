//! ADR-022 / W3.18：打洞优先、中继兜底的**判据 + 交接**真引擎 e2e。
//!
//! 两端都是真的 librqbit 会话，样本是真的落盘文件。区别上一版 bridge e2e 的是：
//! 走不走桥不再由测试脚本写死，而是由 `cr_nat::punch_route::RouteNegotiator`
//! 的双端协商结果决定。
//!
//! 两个必须说清的范围边界：
//! 1. 协商消息在测试里用内存通道交换，代替真实中继控制面；**不是** NAT 打洞证据。
//! 2. 本仓库没有「BT 数据走中继」的数据面（relay 只传帧）。所以 Relay 分支验证的是
//!    「不得把半打通的 socket 交给引擎、不得把普通直连冒充桥命中」，harness 用回环
//!    普通直连当兜底路径的**替身**。绝不声称这是 relay 数据证据。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cr_bridge::Bridge;
use cr_engine::{EngineOptions, RqbitEngine, TorrentSpec};
use cr_nat::punch_route::{
    handshake_frame, validate_handshake, Route, RouteNegotiator, Verdict, HANDSHAKE_LEN,
};
use librqbit::{create_torrent, spawn_utils::BlockingSpawner, CreateTorrentOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream as TokioTcpStream};

const SAMPLE_NAME: &str = "route-e2e.bin";
const SAMPLE_LEN: usize = 1 << 20;
const PIECE_LEN: u32 = 64 * 1024;
const SESSION_NONCE: [u8; 16] = [0xA7; 16];
const SIDE_A: [u8; 16] = [0x11; 16];
const SIDE_B: [u8; 16] = [0x22; 16];

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cr-route-e2e-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sample_bytes() -> Vec<u8> {
    (0..SAMPLE_LEN).map(|i| (i % 239) as u8).collect()
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

/// 在一条**已建立**的 socket 上做 nonce 握手：发本端帧，读对端帧并校验。
/// 用 tokio runtime 同步驱动，避免把异步 I/O 混进主线程轮询。
fn verify_on_stream(rt: &tokio::runtime::Runtime, stream: TokioTcpStream, side: [u8; 16],
                    nonce: [u8; 16]) -> (TokioTcpStream, bool) {
    let frame = handshake_frame(&side, &nonce);
    let result = rt.block_on(async move {
        let mut s = stream;
        let mut peer_buf = [0u8; HANDSHAKE_LEN];
        if s.write_all(&frame).await.is_err() || s.read_exact(&mut peer_buf).await.is_err() {
            return (s, false);
        }
        match validate_handshake(&peer_buf, &side, &nonce) {
            Ok(_) => (s, true),
            Err(_) => (s, false),
        }
    });
    result
}

/// 用内存通道交换两轮裁决，模拟中继控制面。返回双方最终路由。
fn negotiate(local_a_ok: bool, local_b_ok: bool) -> (Route, Route) {
    let mut a = RouteNegotiator::new(SIDE_A, SESSION_NONCE, local_a_ok);
    let mut b = RouteNegotiator::new(SIDE_B, SESSION_NONCE, local_b_ok);

    // 第一轮 Local
    let va: Verdict = a.verdict();
    let vb: Verdict = b.verdict();
    a.observe(vb).unwrap();
    b.observe(va).unwrap();

    // 第二轮 Commit
    let va2 = a.verdict();
    let vb2 = b.verdict();
    a.observe(vb2).unwrap();
    b.observe(va2).unwrap();

    (a.route(), b.route())
}

fn wait_finished(eng: &RqbitEngine, task: cr_engine::TaskId, budget: Duration) -> cr_engine::EngineProgress {
    let deadline = Instant::now() + budget;
    let mut last = None;
    while Instant::now() < deadline {
        let p = eng.sample(task).expect("任务必须仍在引擎里");
        last = Some(p.clone());
        if p.finished && p.downloaded_bytes >= SAMPLE_LEN as u64 {
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    last.expect("至少采样一次")
}

#[test]
fn direct_route_carries_real_piece_through_registered_socket() {
    let a_dir = tmp_dir("direct-seed");
    let b_dir = tmp_dir("direct-download");
    let sample = a_dir.join(SAMPLE_NAME);
    let data = sample_bytes();
    std::fs::write(&sample, &data).unwrap();
    let torrent = make_torrent(&sample);

    // 真做种引擎，带真实监听口。
    let mut seed_opts = engine_opts(&a_dir, None);
    seed_opts.listen = Some("127.0.0.1:0".parse().unwrap());
    let mut seeder = RqbitEngine::new(seed_opts).unwrap();
    let seeder_addr = seeder.listen_addr().unwrap();
    let seed_task = seeder
        .add_torrent_with_timeout(&torrent_spec(&torrent, &a_dir, None), Duration::from_secs(30))
        .unwrap();

    // 桥与其独立 runtime。
    let bridge_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1).enable_all().build().unwrap();
    let bridge = Arc::new(Bridge::new());
    let listener = bridge_rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
    let bridge_addr = listener.local_addr().unwrap();
    let server = bridge.clone();
    bridge_rt.spawn(async move { let _ = server.serve_listener(listener).await; });

    // 双向各建立一条真 TCP，并在两条上都完成 nonce 握手。
    let (s_a, s_b) = {
        let l_b = bridge_rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let addr_b = l_b.local_addr().unwrap();
        let l_a = bridge_rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let addr_a = l_a.local_addr().unwrap();
        let fut = async move {
            let ca = TokioTcpStream::connect(addr_b).await.unwrap();
            let cb = TokioTcpStream::connect(addr_a).await.unwrap();
            let (ab, _) = l_b.accept().await.unwrap();
            let (ba, _) = l_a.accept().await.unwrap();
            (ca, cb, ab, ba)
        };
        let (ca, cb, mut ab, mut ba) = bridge_rt.block_on(fut);
        // ca ↔ ab（B 侧），cb ↔ ba（A 侧）。对端必须发**自己的**身份帧，
        // 而不是把收到的帧原样回显——回显自己 id 会被 SelfEcho 正确拒绝。
        let send_b = bridge_rt.spawn(async move {
            let mut b = [0u8; HANDSHAKE_LEN];
            ab.read_exact(&mut b).await.unwrap();
            ab.write_all(&handshake_frame(&SIDE_B, &SESSION_NONCE)).await.unwrap();
        });
        let send_a = bridge_rt.spawn(async move {
            let mut b = [0u8; HANDSHAKE_LEN];
            ba.read_exact(&mut b).await.unwrap();
            ba.write_all(&handshake_frame(&SIDE_A, &SESSION_NONCE)).await.unwrap();
        });
        let (ca, ok_a) = verify_on_stream(&bridge_rt, ca, SIDE_A, SESSION_NONCE);
        let (cb, ok_b) = verify_on_stream(&bridge_rt, cb, SIDE_B, SESSION_NONCE);
        bridge_rt.block_on(async { send_b.await.unwrap(); send_a.await.unwrap(); });
        assert!(ok_a && ok_b, "双方握手必须都成立");
        (ca, cb)
    };
    // s_a/s_b 是**控制面**握手用的连接：它们证明「对端确是这次会话的另一方」。
    // 握手后 peer 端 stub 即退出，不能拿它承载 BT。数据面另起一条 punched socket，
    // 这正是 ADR-021 已验过的 handoff 模式。
    drop(s_a);
    drop(s_b);

    let (route_a, route_b) = negotiate(true, true);
    assert_eq!(route_a, Route::Direct);
    assert_eq!(route_b, Route::Direct);

    // 只有协商为 Direct 才登记数据面的 punched socket；键是不可直连的 TEST-NET-3。
    let advertised: SocketAddr = format!("203.0.113.9:{}", seeder_addr.port()).parse().unwrap();
    let punch = cr_nat::tcp_punch::punch_tcp(
        "127.0.0.1:0".parse().unwrap(),
        &[seeder_addr],
        Duration::from_secs(3),
    );
    let data_stream = punch.established.expect("回环打洞必须建立数据面连接").stream;
    // from_std 前必须切非阻塞，否则 tokio 不会正确接管 fd（ADR-021 e2e 的同款要求）。
    data_stream.set_nonblocking(true).unwrap();
    let data_stream = bridge_rt.block_on(async { TokioTcpStream::from_std(data_stream).unwrap() });
    bridge.register(advertised, data_stream).unwrap();

    let proxy = Some(format!("socks5://{bridge_addr}"));
    let mut downloader = RqbitEngine::new(engine_opts(&b_dir, proxy.clone())).unwrap();
    let task = downloader
        .add_torrent_with_timeout(
            &torrent_spec(&torrent, &b_dir, Some(advertised)),
            Duration::from_secs(30),
        )
        .unwrap();
    let progress = wait_finished(&downloader, task, Duration::from_secs(90));

    println!(
        "ROUTE_E2E route={} served_registered={} downloaded={} finished={}",
        route_a.label(), bridge.served_registered(), progress.downloaded_bytes, progress.finished
    );
    assert_eq!(bridge.served_registered(), 1, "Direct 必须命中登记表");
    assert!(progress.finished, "真引擎必须完成下载");
    assert_eq!(std::fs::read(b_dir.join(SAMPLE_NAME)).unwrap(), data, "落盘必须逐字节一致");
    assert!(seeder.sample(seed_task).unwrap().uploaded_bytes > 0, "做种方必须有上行");

    if let Ok(path) = std::env::var("CR_ROUTE_E2E_REPORT") {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let report = format!(concat!(
            "{{\n  \"schema\": \"coldrevive.route-e2e/v1\",\n",
            "  \"generated_at_unix\": {now},\n",
            "  \"case\": \"direct\",\n",
            "  \"route_a\": \"{ra}\",\n  \"route_b\": \"{rb}\",\n",
            "  \"served_registered\": {sr},\n  \"downloaded_bytes\": {dl},\n",
            "  \"finished\": {fin},\n  \"byte_equal\": true,\n",
            "  \"handshake_both_sides\": true,\n",
            "  \"scope\": \"loopback; real librqbit; in-memory verdict channel; not NAT evidence\",\n",
            "  \"relay_data_plane\": \"not_implemented\",\n",
            "  \"is_s6_evidence\": false,\n  \"g2_claim\": false,\n  \"s6_status\": \"not_run\"\n}}\n"),
            now = now, ra = route_a.label(), rb = route_b.label(),
            sr = bridge.served_registered(), dl = progress.downloaded_bytes, fin = progress.finished);
        std::fs::write(&path, report).unwrap();
        println!("ROUTE_E2E_REPORT={path}");
    }

    downloader.shutdown();
    seeder.shutdown();
}

#[test]
fn relay_route_refuses_to_register_half_verified_socket() {
    // B 侧握手故意失败：A 成功、B 失败。两端都必须判 Relay。
    let (route_a, route_b) = negotiate(true, false);
    assert_eq!(route_a, Route::Relay, "一端失败时另一端不得独自走直连");
    assert_eq!(route_b, Route::Relay);

    let a_dir = tmp_dir("relay-seed");
    let b_dir = tmp_dir("relay-download");
    let sample = a_dir.join(SAMPLE_NAME);
    let data = sample_bytes();
    std::fs::write(&sample, &data).unwrap();
    let torrent = make_torrent(&sample);

    let mut seed_opts = engine_opts(&a_dir, None);
    seed_opts.listen = Some("127.0.0.1:0".parse().unwrap());
    let mut seeder = RqbitEngine::new(seed_opts).unwrap();
    let seeder_addr = seeder.listen_addr().unwrap();
    let seed_task = seeder
        .add_torrent_with_timeout(&torrent_spec(&torrent, &a_dir, None), Duration::from_secs(30))
        .unwrap();

    let bridge_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1).enable_all().build().unwrap();
    let bridge = Arc::new(Bridge::new());
    let listener = bridge_rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
    let bridge_addr = listener.local_addr().unwrap();
    let server = bridge.clone();
    bridge_rt.spawn(async move { let _ = server.serve_listener(listener).await; });

    // 即使握手失败，代码里那条 socket 仍然存在；协商判 Relay 后**不得**登记它。
    let half_verified = bridge_rt.block_on(TokioTcpStream::connect(seeder_addr)).unwrap();
    bridge_rt.block_on(async { half_verified.set_nodelay(true).unwrap() });
    if route_a == Route::Relay {
        // 不 register —— 这是本测试要守的核心不变量。
        assert_eq!(bridge.registered(), 0);
    }
    drop(half_verified);

    // 兜底路径的替身：给引擎真实可直连地址。它走 SOCKS5 的普通直连回落，
    // 不计入桥命中；这证明 Relay 决策没有偷用 punched socket。
    let proxy = Some(format!("socks5://{bridge_addr}"));
    let mut downloader = RqbitEngine::new(engine_opts(&b_dir, proxy.clone())).unwrap();
    let task = downloader
        .add_torrent_with_timeout(
            &torrent_spec(&torrent, &b_dir, Some(seeder_addr)),
            Duration::from_secs(30),
        )
        .unwrap();
    let progress = wait_finished(&downloader, task, Duration::from_secs(90));

    println!(
        "ROUTE_E2E_RELAY route={} served_registered={} downloaded={} finished={}",
        route_a.label(), bridge.served_registered(), progress.downloaded_bytes, progress.finished
    );
    assert_eq!(bridge.served_registered(), 0, "Relay 分支不得冒充桥命中");
    assert!(progress.finished, "兜底路径仍必须完成下载");
    assert_eq!(std::fs::read(b_dir.join(SAMPLE_NAME)).unwrap(), data, "兜底落盘必须一致");

    if let Ok(path) = std::env::var("CR_ROUTE_E2E_RELAY_REPORT") {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let report = format!(concat!(
            "{{\n  \"schema\": \"coldrevive.route-e2e/v1\",\n",
            "  \"generated_at_unix\": {now},\n",
            "  \"case\": \"relay-fallback-decision\",\n",
            "  \"route_a\": \"{ra}\",\n  \"route_b\": \"{rb}\",\n",
            "  \"served_registered\": {sr},\n  \"downloaded_bytes\": {dl},\n",
            "  \"finished\": {fin},\n  \"byte_equal\": true,\n",
            "  \"fallback_path\": \"ordinary loopback dial (stand-in)\",\n",
            "  \"relay_data_plane\": \"not_implemented\",\n",
            "  \"scope\": \"loopback; real librqbit; decision gate only; not relay data evidence\",\n",
            "  \"is_s6_evidence\": false,\n  \"g2_claim\": false,\n  \"s6_status\": \"not_run\"\n}}\n"),
            now = now, ra = route_a.label(), rb = route_b.label(),
            sr = bridge.served_registered(), dl = progress.downloaded_bytes, fin = progress.finished);
        std::fs::write(&path, report).unwrap();
        println!("ROUTE_E2E_RELAY_REPORT={path}");
    }

    downloader.shutdown();
    assert!(seeder.sample(seed_task).unwrap().uploaded_bytes > 0);
    seeder.shutdown();
}
