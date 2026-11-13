//! M1 标准 BT 数据面：最小种子端 + 本地 tracker。
//!
//! 这里**不实现私有协议**：对端是标准第三方 aria2（Motrix Next 捆绑 aria2 1.36.0）。
//! 私有唤醒只在控制面（UDP /cli）；种子与 Peer Wire 完全遵循 BT 规范。
//! M2 才会用 rqbit 替换这个最小服务端（W5.1），ADR-002 已定稿。

use crate::NodeState;
use cr_core::Config;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn hex_to_20(s: &str) -> Option<[u8; 20]> {
    if s.len() != 40 { return None; }
    let mut out = [0u8; 20];
    for i in 0..20 {
        out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn peer_id() -> [u8; 20] {
    let mut p = [0u8; 20];
    let s = b"-CR0100-000000000001";
    p.copy_from_slice(s);
    p
}

/// 本地 tracker 的 bencode 响应。按客户端 `compact` 参数返回紧凑或非紧凑 peer 列表。
pub fn tracker_response(query: &str, ip: &str, port: u16) -> Vec<u8> {
    let compact = query.split('&').any(|kv| kv == "compact=1")
        || !query.split('&').any(|kv| kv.starts_with("compact="));
    let mut peers = Vec::new();
    if compact {
        for part in ip.split('.') { peers.push(part.parse::<u8>().unwrap_or(127)); }
        while peers.len() < 4 { peers.push(127); }
        peers.extend_from_slice(&port.to_be_bytes());
    } else {
        peers.extend_from_slice(format!(
            "ld2:ip{}:{}4:porti{}eee", ip.len(), ip, port
        ).as_bytes());
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"d8:completei1e10:incompletei0e8:intervali1800e");
    if compact {
        out.extend_from_slice(format!("5:peers{}:", peers.len()).as_bytes());
        out.extend_from_slice(&peers);
    } else {
        out.extend_from_slice(b"5:peers");
        out.extend_from_slice(&peers);
    }
    out.push(b'e');
    out
}

pub fn spawn_seeder(cfg: Config, infohash_hex: String, data: Vec<u8>,
                    piece_size: usize, state: Arc<Mutex<NodeState>>) {
    let Some(ih) = hex_to_20(&infohash_hex) else {
        eprintln!("[{}] InfoHash 非法，BT 种子端未启动", cfg.node_id);
        return;
    };
    let port = cfg.me().map(|m| m.bt_port).unwrap_or(0);
    thread::spawn(move || {
        let listener = match TcpListener::bind(format!("{}:{}", cfg.bind, port)) {
            Ok(l) => l,
            Err(e) => { eprintln!("[{}] BT 端口 {} 绑定失败: {}", cfg.node_id, port, e); return; }
        };
        println!("[{}] 标准 BT 种子端监听 {}:{}", cfg.node_id, cfg.bind, port);
        for stream in listener.incoming() {
            if let Ok(s) = stream {
                let d = data.clone();
                let st = state.clone();
                thread::spawn(move || handle_peer(s, ih, d, piece_size, st));
            }
        }
    });
}

fn read_exact(stream: &mut TcpStream, n: usize) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; n];
    let mut got = 0;
    while got < n {
        match stream.read(&mut buf[got..]) {
            Ok(0) | Err(_) => return None,
            Ok(k) => got += k,
        }
    }
    Some(buf)
}

fn send_msg(stream: &mut TcpStream, id: u8, payload: &[u8]) -> bool {
    let len = (1 + payload.len()) as u32;
    let mut msg = len.to_be_bytes().to_vec();
    msg.push(id);
    msg.extend_from_slice(payload);
    stream.write_all(&msg).is_ok()
}

fn handle_peer(mut stream: TcpStream, ih: [u8; 20], data: Vec<u8>,
               piece_size: usize, state: Arc<Mutex<NodeState>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    // 1) 标准 BT 握手：0x13 + "BitTorrent protocol" + reserved(8) + infohash(20) + peerid(20)
    let hs = match read_exact(&mut stream, 68) { Some(v) => v, None => return };
    if hs[0] != 19 || &hs[1..20] != b"BitTorrent protocol" || hs[28..48] != ih { return; }
    let mut reply = Vec::new();
    reply.push(19u8);
    reply.extend_from_slice(b"BitTorrent protocol");
    reply.extend_from_slice(&[0u8; 8]);
    reply.extend_from_slice(&ih);
    reply.extend_from_slice(&peer_id());
    if stream.write_all(&reply).is_err() { return; }

    // 2) bitfield（全片可用）+ unchoke
    let count = data.len().div_ceil(piece_size);
    let mut bitfield = vec![0u8; count.div_ceil(8)];
    for i in 0..count { bitfield[i / 8] |= 0x80 >> (i % 8); }
    if !send_msg(&mut stream, 5, &bitfield) { return; }
    if !send_msg(&mut stream, 1, &[]) { return; }

    // 3) 只响应标准 request(id=6)，返回 piece(id=7)；不做任何私有扩展
    loop {
        let len_buf = match read_exact(&mut stream, 4) { Some(v) => v, None => return };
        let len = u32::from_be_bytes([len_buf[0], len_buf[1], len_buf[2], len_buf[3]]) as usize;
        if len == 0 { continue; } // keep-alive
        if len > 1 << 20 { return; }
        let body = match read_exact(&mut stream, len) { Some(v) => v, None => return };
        let id = body[0];
        if id == 6 && body.len() >= 13 {
            let index = u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;
            let begin = u32::from_be_bytes([body[5], body[6], body[7], body[8]]) as usize;
            let want = u32::from_be_bytes([body[9], body[10], body[11], body[12]]) as usize;
            // 真实状态门禁：只有被唤醒（HOT）后才供片，COLD 期间不上传
            let hot = state.lock().unwrap().bt_seeding;
            if !hot || want > 1 << 18 { return; }
            let offset = index * piece_size + begin;
            if offset >= data.len() { return; }
            let end = (offset + want).min(data.len());
            let mut payload = Vec::with_capacity(8 + end - offset);
            payload.extend_from_slice(&(index as u32).to_be_bytes());
            payload.extend_from_slice(&(begin as u32).to_be_bytes());
            payload.extend_from_slice(&data[offset..end]);
            if !send_msg(&mut stream, 7, &payload) { return; }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_tracker_response_is_valid_bencode_prefix() {
        let r = tracker_response("compact=1&info_hash=x", "127.0.0.1", 16802);
        let s = String::from_utf8_lossy(&r);
        assert!(s.starts_with("d8:completei1e"));
        assert!(s.contains("5:peers6:"));
        assert_eq!(r.last(), Some(&b'e'));
    }

    #[test]
    fn noncompact_tracker_response_contains_peer_dict() {
        let r = tracker_response("compact=0", "127.0.0.1", 16802);
        let s = String::from_utf8_lossy(&r);
        assert!(s.contains("ld2:ip9:127.0.0.14:porti16802eee"));
    }
}
