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

/// 真实 aMule 2.3.3 不会按请求的 184320 整块回帧：它把每个请求块拆成
/// 10240 字节的 SENDINGPART 碎片（W25-daemon 抓包：`len=10264 range=0..10240`）。
/// 只对着我们自己的 serve（整块回帧）做回环，覆盖判定的方向错误永远暴露不出来。
#[test]
fn downloader_accepts_peer_fragmented_sending_parts() {
    const FRAGMENT: u32 = 10_240;
    let file = deterministic_file(ED2K_PART_SIZE + 8192, 9);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let served = file.clone();
    let peer = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut link = stack::Link::new(stream);
        let _ = link.recv().expect("hello"); // client HELLO
        link.send_frame(&wire::hello_answer_frame(&hello(addr.port()))).unwrap();
        let hash = served.hash();
        let mut pending_recv = 0usize;
        loop {
            let p = match link.recv() {
                Ok(p) => p,
                Err(_) => return pending_recv,
            };
            match p.opcode {
                wire::op::REQUESTFILENAME => {
                    link.send_frame(&wire::file_name_answer(&hash, &served.name)).unwrap();
                }
                wire::op::HASHSETREQUEST => {
                    link.send_frame(&wire::hashset_answer(&hash, &served.part_hashes())).unwrap();
                }
                wire::op::STARTUPLOADREQ => {
                    link.send_frame(&wire::accept_upload_req()).unwrap();
                }
                wire::op::REQUESTPARTS => {
                    let (h, ranges) = wire::parse_request_parts(&p.payload).unwrap();
                    assert_eq!(h, hash);
                    for (s, e) in ranges {
                        let mut at = s;
                        while at < e {
                            let stop = (at + FRAGMENT).min(e);
                            link.send_frame(&wire::sending_part(
                                &hash, at, stop, &served.data[at as usize..stop as usize],
                            ))
                            .unwrap();
                            pending_recv += (stop - at) as usize;
                            at = stop;
                        }
                    }
                }
                wire::op::END_OF_DOWNLOAD => return pending_recv,
                _ => {}
            }
        }
    });

    let stream = std::net::TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let result = stack::download(stream, &file, &hello(0), Duration::from_secs(30));
    let sent = peer.join().unwrap();
    assert!(result.completed, "fragmented download 未完成: {:?}", result.error);
    assert!(result.hash_matches && result.content_matches, "内容不一致: {:?}", result.error);
    assert_eq!(sent, file.data.len(), "服务端只发出了 {} 字节", sent);
    assert!(
        result.blocks_received > file.blocks().len(),
        "没有发生分片：blocks_received={} 请求块数={}",
        result.blocks_received,
        file.blocks().len()
    );
}

/// aMule 的 DownloadClient 不是「一批请求等一批回帧」：它维持 3 个 pending 块，
/// 每完成一块就把「仍未完成的块 + 一个新块」重新发一次 REQUESTPARTS
/// （DownloadClient.cpp `SendBlockRequests`）。上传端若按区间永久去重，
/// 对端真正缺的那一块就永远送不到，会话停在中途。这里用同样的滑窗驱动
/// 我们的 serve，要求整文件送达。
#[test]
fn server_satisfies_amule_style_sliding_window() {
    let file = deterministic_file(ED2K_PART_SIZE + 8192, 11);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let served = file.clone();
    let queue = Arc::new(Mutex::new(UploadQueue::new(1)));
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stack::serve(stream, &[served], &hello(addr.port()), &queue, Duration::from_secs(60))
    });

    let stream = std::net::TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    let mut link = stack::Link::new(stream);
    let hash = file.hash();
    let mut queue_blocks = file.blocks();
    // aMule 先要末尾的小分片，再从头顺序取。
    let tail = queue_blocks.pop().unwrap();
    let mut window: Vec<(u32, u32)> = vec![tail];
    let mut written = vec![false; file.data.len()];
    let mut got = vec![0u8; file.data.len()];
    let mut covered = 0usize;

    link.send_frame(&wire::hello_frame(&hello(0))).unwrap();
    let mut accepted = false;
    let request = |link: &mut stack::Link, window: &[(u32, u32)]| {
        link.send_frame(&wire::request_parts(&hash, window)).unwrap();
    };

    while covered < file.data.len() {
        if !accepted {
            let p = link.recv().expect("handshake packet");
            match p.opcode {
                wire::op::HELLOANSWER => link.send_frame(&wire::file_request(&hash)).unwrap(),
                wire::op::REQFILENAMEANSWER => {
                    link.send_frame(&wire::hashset_request(&hash)).unwrap()
                }
                wire::op::HASHSETANSWER => link.send_frame(&wire::start_upload_req(&hash)).unwrap(),
                wire::op::ACCEPTUPLOADREQ => {
                    accepted = true;
                    while window.len() < 3 && !queue_blocks.is_empty() {
                        window.push(queue_blocks.remove(0));
                    }
                    request(&mut link, &window);
                }
                _ => {}
            }
            continue;
        }
        let p = match link.recv() {
            Ok(p) => p,
            Err(e) => panic!(
                "上传端在第 {} 字节处停止（覆盖 {}/{}，窗口 {:?}，队列剩余 {}）：{}",
                covered, covered, file.data.len(), window, queue_blocks.len(), e
            ),
        };
        if p.opcode == wire::op::END_OF_DOWNLOAD || p.opcode == wire::op::CANCELTRANSFER {
            break;
        }
        if p.opcode != wire::op::SENDINGPART {
            continue;
        }
        let (h, start, end, data) = wire::parse_sending_part(&p.payload).unwrap();
        assert_eq!(h, hash);
        for i in start as usize..end as usize {
            if !written[i] {
                written[i] = true;
                covered += 1;
            }
        }
        got[start as usize..end as usize].copy_from_slice(&data);

        // 只有整块覆盖后才像 aMule 那样补新块并重发请求。
        let mut done: Vec<(u32, u32)> = Vec::new();
        for r in &window {
            let full = (r.0 as usize..r.1 as usize).all(|i| written[i]);
            if full {
                done.push(*r);
            }
        }
        if !done.is_empty() {
            for r in &done {
                window.retain(|w| w != r);
            }
            let mut added = false;
            while window.len() < 3 && !queue_blocks.is_empty() {
                window.push(queue_blocks.remove(0));
                added = true;
            }
            if added || !window.is_empty() {
                request(&mut link, &window);
            }
        }
    }
    assert_eq!(got.as_slice(), file.data.as_slice(), "滑窗客户端没有收到完整内容");
    let up = server.join().unwrap();
    assert!(up.error.is_none(), "serve 报错: {:?}", up.error);
}

/// 真 aMule 的 `SendBlockRequests` 不是「问一批、等一批」：它维持 3 个 pending
/// 块，每完成一块就把「仍未完成的块 + 一个新块」重发一次（DownloadClient.cpp）。
/// 这里按 aMule 的**账目**复刻：只有收到某块的**末尾字节**才算完成（源码里
/// `nEndPos == cur_block->block->EndOffset` 才 RemoveBlockFromList），完成一块
/// 才补一块并重发请求。上传端若少送、重送错位或把 pending 区间当成永久去重，
/// 这个用例会停在半路——它比「收到全部字节」更接近真实对端判据。
#[test]
fn server_satisfies_amule_block_accounting() {
    let file = deterministic_file(ED2K_PART_SIZE + 8192, 21);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let served = file.clone();
    let queue = Arc::new(Mutex::new(UploadQueue::new(1)));
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stack::serve(stream, &[served], &hello(addr.port()), &queue, Duration::from_secs(60))
    });

    let stream = std::net::TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    let mut link = stack::Link::new(stream);
    let hash = file.hash();
    let size = file.data.len();

    link.send_frame(&wire::hello_frame(&hello(0))).unwrap();
    let mut accepted = false;
    while !accepted {
        let p = link.recv().expect("handshake");
        match p.opcode {
            wire::op::HELLOANSWER => link.send_frame(&wire::file_request(&hash)).unwrap(),
            wire::op::REQFILENAMEANSWER => link.send_frame(&wire::hashset_request(&hash)).unwrap(),
            wire::op::HASHSETANSWER => link.send_frame(&wire::start_upload_req(&hash)).unwrap(),
            wire::op::ACCEPTUPLOADREQ => accepted = true,
            _ => {}
        }
    }

    // aMule 先要末尾小块，再从 0 顺序取整块。
    let mut rest: Vec<(u32, u32)> = file.blocks();
    let tail = rest.pop().unwrap();
    let mut pending: Vec<(u32, u32)> = vec![tail];
    let mut complete = vec![false; size];
    let mut blocks_done = 0usize;
    let total_blocks = file.blocks().len();

    let fill = |pending: &mut Vec<(u32, u32)>, rest: &mut Vec<(u32, u32)>| {
        while pending.len() < 3 && !rest.is_empty() {
            pending.push(rest.remove(0));
        }
    };
    fill(&mut pending, &mut rest);
    link.send_frame(&wire::request_parts(&hash, &pending)).unwrap();

    let mut frames = 0usize;
    while blocks_done < total_blocks {
        let p = match link.recv() {
            Ok(p) => p,
            Err(e) => panic!(
                "上传端在第 {} 帧后停住（完成 {}/{} 块，pending={:?}）：{}",
                frames, blocks_done, total_blocks, pending, e
            ),
        };
        if p.opcode != wire::op::SENDINGPART {
            continue;
        }
        let (h, start, end, data) = wire::parse_sending_part(&p.payload).unwrap();
        assert_eq!(h, hash);
        assert_eq!(data.len(), (end - start) as usize, "帧长度与区间不符");
        frames += 1;

        // 与 aMule 一致：帧的起点必须落在某个 pending 块内，且不得越过块尾。
        let idx = pending.iter().position(|(s, e)| *s <= start && start < *e);
        let idx = match idx {
            Some(i) => i,
            None => continue, // aMule 收到窗口外的帧会静默丢弃
        };
        let (bs, be) = pending[idx];
        assert!(end <= be, "帧 {:?} 越过 pending 块尾 {:?}（aMule 会移除该块）", (start, end), (bs, be));
        for i in start as usize..end as usize {
            complete[i] = true;
        }
        if end == be {
            let done = pending.remove(idx);
            assert!(complete[done.0 as usize..done.1 as usize].iter().all(|v| *v));
            blocks_done += 1;
            fill(&mut pending, &mut rest);
            if !pending.is_empty() {
                link.send_frame(&wire::request_parts(&hash, &pending)).unwrap();
            }
        }
    }
    assert!(complete.iter().all(|v| *v), "有字节没送到");
    let up = server.join().unwrap();
    assert!(up.error.is_none(), "serve 报错: {:?}", up.error);
}
