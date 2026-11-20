//! FR-25 生产 crate 级回环 e2e：用 `cr_ed2k::stack` 的两端跑真实 TCP 取片。
//!
//! 这是把 S2 审计过的栈纳入生产 crate 后的**生产路径**证据（不是跑 spike 二进制）。
//! 只连 127.0.0.1；不碰公网、不碰 server、不判 G2 pass。
use cr_ed2k::kad::{self, ED2K_PART_SIZE};
use cr_ed2k::stack::{self, SharedFile, UploadQueue};
use cr_ed2k::wire;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn hello(port: u16) -> wire::Hello {
    let mut h = [0u8; 16];
    h.copy_from_slice(&kad::md4(b"coldrevive-fr25-production"));
    wire::default_hello(h, port, 4672, 4672, "coldrevive-fr25")
}

fn deterministic_file(size: usize, seed: u8) -> SharedFile {
    let data: Vec<u8> = (0..size).map(|i| ((i as u32 * 31 + seed as u32) % 256) as u8).collect();
    SharedFile::new(format!("fr25-{}.bin", seed), data)
}

fn run_case(file: SharedFile, label: &str) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let files = vec![file.clone()];
    let queue = Arc::new(Mutex::new(UploadQueue::new(1)));
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stack::serve(stream, &files, &hello(addr.port()), &queue, Duration::from_secs(30))
    });

    let stream = std::net::TcpStream::connect(addr).unwrap();
    let _ = stream.set_read_timeout(Some(Duration::from_secs(12)));
    let result = stack::download(stream, &file, &hello(0), Duration::from_secs(30));
    let up = server.join().unwrap();

    assert!(result.completed, "{} download 未完成: {:?}", label, result.error);
    assert!(result.hash_matches, "{} eD2k hash 不符", label);
    assert!(result.content_matches, "{} 内容逐字节不符", label);
    assert_eq!(result.data, file.data.as_slice(), "{} 字节不一致", label);
    assert_eq!(up.bytes_sent as usize, file.data.len(), "{} 服务端上行字节不符", label);
    println!(
        "{} ok: size={} blocks={} hash={} queue_rank={:?}",
        label, result.data.len(), result.blocks_received,
        cr_ed2k::sha1::hex(&file.hash()), result.queue_rank_seen,
    );
}

#[test]
fn production_ed2k_stack_loopback_single_and_multipart() {
    // 三个形状覆盖 L2 hash 修正的全部边界：< PARTSIZE、== PARTSIZE（Red 空块）、> PARTSIZE。
    run_case(deterministic_file(4096, 1), "single_part_4KiB");
    run_case(deterministic_file(ED2K_PART_SIZE, 2), "exact_one_part_boundary");
    run_case(deterministic_file(ED2K_PART_SIZE + 8192, 3), "multi_part");
}
