//! S4 spike: 1万文件影子索引常驻 ≤100 MB (SRS NFR-01 / NFR-05).
//!
//! 模型严格遵循 DATA.md §5「COLD 状态的主要常驻开销 = 一行索引 + 无上下文」，
//! **禁止**缓存 Bitfield 或分片哈希数组（分片哈希留在磁盘 `.torrent`，DB 仅存 `torrent_path` 引用）。
//! 因此每个内存条目是一个轻量 handle，而非完整做种上下文。
//!
//! 内存测量交由外部 Python 采样器通过 Windows `GetProcessMemoryInfo` 读取本进程工作集
//! （Working Set），在 MARK 标记处采样，得到索引的真实常驻内存。本二进制只负责构建索引并打标记。

use std::collections::HashMap;
use std::thread::sleep;
use std::time::{Duration, Instant};

/// 影子索引中 COLD 态的一条记录（内存 handle）。
/// 字段对齐 DATA.md / LLD §5.3 的 `files` 行：infohash / file_hash / 路径 / 大小 / 状态 / 最后活跃。
/// `root_hash` 是分片 Merkle 根（32B），**不是**全量分片哈希（那对 4GB 文件会膨胀到 ~5MB/文件）。
#[derive(Clone)]
struct ShadowRow {
    rowid: u32,        // SQLite 行引用
    state: u8,         // 0 = COLD（仅影子索引在线，无做种上下文）
    size: u64,
    mtime: i64,
    piece_length: u32,
    num_pieces: u32,
    root_hash: [u8; 32], // 分片 Merkle 根（紧凑），非逐片哈希数组
    flags: u8,
    last_published: i64, // KAD / DHT 发布 TTL 续期时间戳
    torrent_path: String, // 相对共享根的路径；.torrent 落盘，仅存引用
}

fn make_row(i: usize) -> ([u8; 20], ShadowRow) {
    let mut key = [0u8; 20];
    let mut x = (i as u64).wrapping_mul(0x9E3779B9).wrapping_add(1);
    for b in key.iter_mut() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *b = (x & 0xFF) as u8;
    }
    let row = ShadowRow {
        rowid: i as u32,
        state: 0,
        size: 4u64 * 1024 * 1024 * 1024,
        mtime: 1_700_000_000 + (i as i64),
        piece_length: 16 * 1024,
        num_pieces: (4u64 * 1024 * 1024 * 1024 / (16 * 1024)) as u32,
        root_hash: {
            let mut h = [0u8; 32];
            let mut y = (i as u64).wrapping_mul(0x85EBCA6B);
            for b in h.iter_mut() {
                y ^= y >> 15;
                y = y.wrapping_mul(0x2C1B3C6D);
                *b = (y & 0xFF) as u8;
            }
            h
        },
        flags: 0b0000_0001,
        last_published: 1_700_000_000,
        torrent_path: format!("shard/{:05}/{:08}.torrent", i % 256, i),
    };
    (key, row)
}

fn build_into(map: &mut HashMap<[u8; 20], ShadowRow>, n: usize) {
    let start = map.len();
    for i in start..start + n {
        let (k, r) = make_row(i);
        map.insert(k, r);
    }
}

fn main() {
    // 让外部采样器能拿到 PID
    eprintln!("PID={}", std::process::id());

    let mut held: Vec<HashMap<[u8; 20], ShadowRow>> = Vec::new();
    let mut current = HashMap::new();

    // 基线：尚未构建任何索引
    eprintln!("MARK baseline");
    sleep(Duration::from_millis(800));

    // 阶段 1：1 万文件（NFR-01 直接目标）
    let t = Instant::now();
    build_into(&mut current, 10_000);
    let ms_10k = t.elapsed().as_millis();
    held.push(std::mem::replace(&mut current, HashMap::new()));
    eprintln!("MARK 10k");
    sleep(Duration::from_millis(800));

    // 阶段 2：追加到 5 万（NFR-05 容量 ≥50k）
    let t = Instant::now();
    build_into(&mut current, 40_000);
    let ms_50k = t.elapsed().as_millis();
    held.push(std::mem::replace(&mut current, HashMap::new()));
    eprintln!("MARK 50k");
    sleep(Duration::from_millis(800));

    // 阶段 3：追加到 20 万（扩张性上限观察）
    let t = Instant::now();
    build_into(&mut current, 150_000);
    let ms_200k = t.elapsed().as_millis();
    held.push(current);
    eprintln!("MARK 200k");
    sleep(Duration::from_millis(800));

    let per = std::mem::size_of::<ShadowRow>() as f64;
    println!(
        "{{\"note\":\"RSS 由外部采样器在 MARK 处读取\",\"theoretical_sizeof_shadowrow_bytes\":{:.0},\
\"insert_ms\":{{\"10k\":{},\"50k\":{},\"200k\":{}}}}}",
        per, ms_10k, ms_50k, ms_200k
    );
    // 防止优化
    eprintln!("TOTAL_ROWS={}", held.iter().map(|m| m.len()).sum::<usize>());
}
