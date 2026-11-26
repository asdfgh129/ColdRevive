//! FR-25 congestion evidence (cr-ed2k only; no network, no real aMule).
//!
//! A peer that asks once and then stops consuming must not be able to make
//! `serve` blast bytes into its socket or spin forever. These tests use a real
//! loopback TCP socket but never contact a server or a real client.

use cr_ed2k::kad;
use cr_ed2k::stack::{self, ServeConfig, SharedFile, UploadQueue};
use cr_ed2k::wire::{self, Hash16};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

fn hello(port: u16) -> wire::Hello {
    let mut h = [0u8; 16];
    h.copy_from_slice(&kad::md4(b"coldrevive-congestion"));
    wire::default_hello(h, port, 4672, 4672, "coldrevive-congestion")
}

fn file() -> SharedFile {
    let size = wire::BLOCKSIZE as usize * 3 + 17;
    let data: Vec<u8> = (0..size).map(|i| ((i * 17 + 3) % 256) as u8).collect();
    SharedFile::new("stall.bin", data)
}

/// Drive the standard handshake up to `OP_ACCEPTUPLOADREQ`.
fn accept_upload(link: &mut stack::Link, hash: &Hash16) {
    link.send_frame(&wire::hello_frame(&hello(0))).unwrap();
    loop {
        let p = link.recv().expect("handshake frame");
        match p.opcode {
            wire::op::HELLOANSWER => link.send_frame(&wire::file_request(hash)).unwrap(),
            wire::op::REQFILENAMEANSWER => link.send_frame(&wire::hashset_request(hash)).unwrap(),
            wire::op::HASHSETANSWER => link.send_frame(&wire::start_upload_req(hash)).unwrap(),
            wire::op::ACCEPTUPLOADREQ => return,
            _ => {}
        }
    }
}

fn request_three_blocks(link: &mut stack::Link, hash: &Hash16) {
    let b = wire::BLOCKSIZE;
    link.send_frame(&wire::request_parts(hash, &[(0, b), (b, 2 * b), (2 * b, 3 * b)]))
        .unwrap();
}

/// A peer that requests once and then stops reading. The window must bound
/// what we put on the wire, the remainder must stay queued, and `serve` must
/// finish at its deadline instead of spinning.
#[test]
fn stalled_reader_bounds_outstanding_bytes_and_terminates() {
    const WINDOW: usize = 32 * 1024; // < one 184320 block on purpose
    let served = file();
    let hash = served.hash();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let config = ServeConfig {
        max_in_flight_bytes: Some(WINDOW),
        ..ServeConfig::default()
    };
    let (tx, rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let queue = Arc::new(Mutex::new(UploadQueue::new(1)));
        let result = stack::serve_with_config(
            stream,
            &[served],
            &hello(addr.port()),
            &queue,
            Duration::from_secs(2), // shorter than IDLE_RESEND (3 s)
            config,
        );
        tx.send(result).unwrap();
    });

    let stream = TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut link = stack::Link::new(stream);
    accept_upload(&mut link, &hash);
    request_three_blocks(&mut link, &hash);
    // Intentionally never call `link.recv()` again: the peer stopped reading.
    // Keep the socket open so `serve` sees a live-but-silent peer.

    let started = Instant::now();
    let up = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("serve did not terminate for a stalled peer");
    server.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(10), "sender spun instead of finishing");

    assert!(up.max_in_flight_bytes <= WINDOW, "window peak {} > {WINDOW}", up.max_in_flight_bytes);
    assert!(up.bytes_sent <= WINDOW, "blasted {} bytes at a peer that stopped reading", up.bytes_sent);
    assert!(up.deferred_bytes > 0, "unconsumed bytes must stay queued, not be pushed into a full buffer");
    assert!(up.error.is_some(), "expected an honest deadline error, got {up:?}");
    let _keep_socket_open = &link;
}

/// Local test of the OPT-IN aMule stop-read hypothesis:
/// `request_gated_pacing` sends at most one block per request and suppresses
/// unsolicited idle-resend bytes. The peer here keeps reading but never asks
/// for more, so any extra bytes would be visible. No claim is made about a
/// real aMule until the external e2e is run.
#[test]
fn opt_in_request_gated_pacing_sends_one_block_and_no_unsolicited_resend() {
    let served = file();
    let hash = served.hash();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let config = ServeConfig {
        max_in_flight_bytes: None,
        request_gated_pacing: true,
        ..ServeConfig::default()
    };
    let (tx, rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let queue = Arc::new(Mutex::new(UploadQueue::new(1)));
        let result = stack::serve_with_config(
            stream,
            &[served],
            &hello(addr.port()),
            &queue,
            Duration::from_secs(4), // long enough that the default watchdog would fire at 3 s
            config,
        );
        tx.send(result).unwrap();
    });

    let stream = TcpStream::connect(addr).unwrap();
    // This peer DOES drain the socket, so only the missing requests -- not a
    // full kernel buffer -- are what stops the transfer.
    stream.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    let mut link = stack::Link::new(stream);
    accept_upload(&mut link, &hash);
    request_three_blocks(&mut link, &hash);

    let mut got = 0usize;
    let mut quiet = 0;
    while quiet < 5 {
        match link.recv() {
            Ok(p) if p.opcode == wire::op::SENDINGPART => {
                quiet = 0;
                let (h, _, _, data) = wire::parse_sending_part(&p.payload).unwrap();
                assert_eq!(h, hash);
                got += data.len();
            }
            Ok(_) => {}
            Err(_) => quiet += 1,
        }
    }
    let up = rx.recv_timeout(Duration::from_secs(10)).expect("serve did not terminate");
    server.join().unwrap();

    assert_eq!(got, wire::BLOCKSIZE as usize, "opt-in pacing must emit exactly one requested block");
    assert_eq!(up.bytes_sent, wire::BLOCKSIZE as usize);
    assert_eq!(up.idle_resends, 0, "opt-in pacing must not push unsolicited resends");
    assert_eq!(up.deferred_bytes, 2 * wire::BLOCKSIZE as usize);
    assert!(up.error.is_some());
}

#[test]
fn compressed_parts_emit_stock_amule_frame_layout() {
    let served = file();
    let hash = served.hash();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let config = ServeConfig {
        compressed_parts: true,
        ..ServeConfig::default()
    };
    let (tx, rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let queue = Arc::new(Mutex::new(UploadQueue::new(1)));
        let result = stack::serve_with_config(
            stream,
            &[served],
            &hello(addr.port()),
            &queue,
            Duration::from_secs(4),
            config,
        );
        tx.send(result).unwrap();
    });
    let stream = TcpStream::connect(addr).unwrap();
    let mut link = stack::Link::new(stream);
    accept_upload(&mut link, &hash);
    request_three_blocks(&mut link, &hash);

    let p = link.recv().expect("compressed frame");
    assert_eq!(p.protocol, wire::PROT_EMULE);
    assert_eq!(p.opcode, wire::op::COMPRESSEDPART);
    assert_eq!(&p.payload[..16], &hash);
    assert_eq!(u32::from_le_bytes(p.payload[16..20].try_into().unwrap()), 0);
    let zlen = u32::from_le_bytes(p.payload[20..24].try_into().unwrap()) as usize;
    assert_eq!(zlen, p.payload.len() - 24);
    assert_eq!(&p.payload[24..26], &[0x78, 0x01]);
    drop(link);
    let up = rx.recv_timeout(Duration::from_secs(10)).expect("serve did not terminate");
    server.join().unwrap();
    assert!(up.bytes_sent >= wire::BLOCKSIZE as usize);
}

#[test]
fn repeated_request_resends_the_same_block() {
    let served = file();
    let hash = served.hash();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let config = ServeConfig {
        max_in_flight_bytes: Some(wire::UPLOAD_CHUNK as usize),
        ..ServeConfig::default()
    };
    let (tx, rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let queue = Arc::new(Mutex::new(UploadQueue::new(1)));
        let result = stack::serve_with_config(
            stream, &[served], &hello(addr.port()), &queue,
            Duration::from_secs(4), config,
        );
        tx.send(result).unwrap();
    });
    let stream = TcpStream::connect(addr).unwrap();
    let mut link = stack::Link::new(stream);
    accept_upload(&mut link, &hash);
    let req = wire::request_parts(&hash, &[(0, wire::BLOCKSIZE)]);
    for _ in 0..2 {
        link.send_frame(&req).unwrap();
        let p = link.recv().expect("resend after re-request");
        assert_eq!(p.opcode, wire::op::SENDINGPART);
        let (h, start, end, _) = wire::parse_sending_part(&p.payload).unwrap();
        assert_eq!(h, hash);
        assert_eq!((start, end), (0, wire::UPLOAD_CHUNK));
    }
    drop(link);
    let _ = rx.recv_timeout(Duration::from_secs(10)).expect("serve did not terminate");
    server.join().unwrap();
}
