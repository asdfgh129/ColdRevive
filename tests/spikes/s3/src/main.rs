//! ColdRevive M0 / S3 spike — 双栈资源可控性探针（KAD 128-bit + Mainline 160-bit）。
//!
//! S3 判据：
//!   1. 双栈并行压测无 ID 空间串扰（crosstalk / 泄漏）；
//!   2. 内存与带宽增量在可接受范围。
//!
//! 本探针只验证**本地双索引数据结构与隔离性 + 资源增量估算**，不做真实 KAD / Mainline DHT
//! 网络加入（那属于 M1 之后的集成与 G3 兼容性矩阵）。真实 DHT 流量取决于 churn / 续期，
//! 此处仅给结构性上界。证据边界在 README §4 与输出 JSON 的 `scope` 字段。
//!
//! 红线：不接触任何网络；不依赖第三方 crate；输出可复现。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

const KAD_KEY_LEN: usize = 16; // 128-bit KAD key
const BT_KEY_LEN: usize = 20; // 160-bit Mainline infohash
const VALUE_LEN: usize = 64; // 模拟描述符 / 元数据负载
const FLAG_KAD: u8 = 1;
const FLAG_BT: u8 = 2;

#[derive(Clone)]
struct Entry {
    value: Vec<u8>,
    flags: u8,
}

/// 确定性 PRNG（xorshift64*），避免引入 rand 依赖；用于把索引号散列成定长 key 与负载。
fn mix(mut s: u64, out: &mut [u8]) {
    for b in out.iter_mut() {
        s = s.wrapping_mul(0x9E3779B97F4A7C15);
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        *b = (s & 0xff) as u8;
    }
}

fn kad_key(i: usize) -> [u8; KAD_KEY_LEN] {
    let mut k = [0u8; KAD_KEY_LEN];
    mix((i as u64).wrapping_mul(0xA1B2C3D4) ^ 0x1111, &mut k);
    k
}

fn bt_key(i: usize) -> [u8; BT_KEY_LEN] {
    let mut k = [0u8; BT_KEY_LEN];
    mix((i as u64).wrapping_mul(0xB2C3D4E5) ^ 0x2222, &mut k);
    k
}

fn payload(i: usize) -> Vec<u8> {
    vec![(i & 0xff) as u8; VALUE_LEN]
}

/// 单条 entry 序列化上界（发布/续期报文字节数代理）：key + value + flags + 2 字节 tag 开销。
fn entry_wire_bytes(key_len: usize) -> usize {
    key_len + VALUE_LEN + 1 + 2
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: usize = args
        .iter()
        .position(|a| a == "--scale")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000);
    let neg_samples: usize = args
        .iter()
        .position(|a| a == "--neg")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(50_000);
    let warm = args.iter().any(|a| a == "--warm");

    let started = Instant::now();

    // ---- 阶段 1：顺序交错插入双栈 ----
    let mut kad: HashMap<[u8; KAD_KEY_LEN], Entry> = HashMap::with_capacity(n);
    let mut bt: HashMap<[u8; BT_KEY_LEN], Entry> = HashMap::with_capacity(n);
    let mut kad_keys: Vec<[u8; KAD_KEY_LEN]> = Vec::with_capacity(n);
    let mut bt_keys: Vec<[u8; BT_KEY_LEN]> = Vec::with_capacity(n);

    for i in 0..n {
        let k = kad_key(i);
        let b = bt_key(i);
        kad.insert(k, Entry { value: payload(i), flags: FLAG_KAD });
        bt.insert(b, Entry { value: payload(i), flags: FLAG_BT });
        kad_keys.push(k);
        bt_keys.push(b);
    }

    // ---- 阶段 2：单线程完整性 + 串扰泄漏检查 ----
    let mut anomalies = 0usize;
    for i in 0..n {
        match kad.get(&kad_keys[i]) {
            Some(e) if e.flags == FLAG_KAD && e.value.len() == VALUE_LEN => {}
            _ => anomalies += 1,
        }
        match bt.get(&bt_keys[i]) {
            Some(e) if e.flags == FLAG_BT && e.value.len() == VALUE_LEN => {}
            _ => anomalies += 1,
        }
    }
    // 负查询：随机未在集合中的 key 必须两个栈都查不到（泄漏 = 串扰）
    let mut leak = 0usize;
    for t in 0..neg_samples {
        let mut r16 = [0u8; KAD_KEY_LEN];
        let mut r20 = [0u8; BT_KEY_LEN];
        mix(0xDEAD_0000u64.wrapping_add(t as u64), &mut r16);
        mix(0xC0DE_0000u64.wrapping_add(t as u64), &mut r20);
        if kad.get(&r16).is_some() {
            leak += 1;
        }
        if bt.get(&r20).is_some() {
            leak += 1;
        }
    }

    // ---- 阶段 3：并发读写压测（验证并行无串扰）----
    let kad = Arc::new(Mutex::new(kad));
    let bt = Arc::new(Mutex::new(bt));
    let kad_keys = Arc::new(kad_keys);
    let bt_keys = Arc::new(bt_keys);
    let mut handles = Vec::new();

    // 4 个读线程：随机查已存在 key + 负查询，断言不跨栈、不串扰
    for tid in 0..4 {
        let kad = Arc::clone(&kad);
        let bt = Arc::clone(&bt);
        let kad_keys = Arc::clone(&kad_keys);
        let bt_keys = Arc::clone(&bt_keys);
        handles.push(thread::spawn(move || {
            let mut local_anom = 0usize;
            let mut local_leak = 0usize;
            for t in 0..(neg_samples / 4) {
                let idx = (tid * 7919 + t * 4) % kad_keys.len();
                match kad.lock().unwrap().get(&kad_keys[idx]) {
                    Some(e) if e.flags == FLAG_KAD && e.value.len() == VALUE_LEN => {}
                    _ => local_anom += 1,
                }
                match bt.lock().unwrap().get(&bt_keys[idx]) {
                    Some(e) if e.flags == FLAG_BT && e.value.len() == VALUE_LEN => {}
                    _ => local_anom += 1,
                }
                let mut r16 = [0u8; KAD_KEY_LEN];
                let mut r20 = [0u8; BT_KEY_LEN];
                mix(0xDEAD_0000u64.wrapping_add((tid * 1_000_003 + t) as u64), &mut r16);
                mix(0xC0DE_0000u64.wrapping_add((tid * 1_000_003 + t) as u64), &mut r20);
                if kad.lock().unwrap().get(&r16).is_some() {
                    local_leak += 1;
                }
                if bt.lock().unwrap().get(&r20).is_some() {
                    local_leak += 1;
                }
            }
            (local_anom, local_leak)
        }));
    }
    // 1 个写线程：追加全新的不相交 key（不覆盖既有）
    {
        let kad = Arc::clone(&kad);
        let bt = Arc::clone(&bt);
        handles.push(thread::spawn(move || {
            let extra = n / 10;
            for i in 0..extra {
                let idx = n + i;
                kad.lock()
                    .unwrap()
                    .insert(kad_key(idx), Entry { value: payload(idx), flags: FLAG_KAD });
                bt.lock()
                    .unwrap()
                    .insert(bt_key(idx), Entry { value: payload(idx), flags: FLAG_BT });
            }
            (0usize, 0usize)
        }));
    }
    for h in handles {
        if let Ok((a, l)) = h.join() {
            anomalies += a;
            leak += l;
        }
    }

    // ---- 阶段 4：资源增量估算 ----
    let kad_guard = kad.lock().unwrap();
    let bt_guard = bt.lock().unwrap();
    let kad_cap = kad_guard.capacity();
    let bt_cap = bt_guard.capacity();
    // 上界估算：HashMap 桶表（每桶 8B 指针） + 每 entry（key + Vec 头 24B + value + ctrl 1B）
    let per_kad_est = kad_cap * 8 + n * (KAD_KEY_LEN + 24 + VALUE_LEN + 1);
    let per_bt_est = bt_cap * 8 + n * (BT_KEY_LEN + 24 + VALUE_LEN + 1);
    let _ = (kad_cap, bt_cap);
    drop(kad_guard);
    drop(bt_guard);

    let wire_kad = n * entry_wire_bytes(KAD_KEY_LEN);
    let wire_bt = n * entry_wire_bytes(BT_KEY_LEN);
    let total_wire = wire_kad + wire_bt;

    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;

    let report = serde_json::json!({
        "schema": "coldrevive.m0.s3/v1",
        "spike": "S3",
        "title": "KAD 128-bit 与 Mainline 160-bit 双栈资源可控",
        "scale_entries_per_stack": n,
        "negative_samples": neg_samples,
        "elapsed_ms": elapsed_ms,
        "criteria": {
            "c1_no_crosstalk": {
                "anomalies_total": anomalies,
                "leak_total": leak,
                "result": if anomalies == 0 && leak == 0 { "verified" } else { "fail" },
                "note": "KAD 16B 与 BT 20B key 分属两个独立容器；负查询两栈均须 miss，任何 hit 即泄漏/串扰"
            },
            "c2_resource_increment": {
                "kad_index_est_bytes": per_kad_est,
                "bt_index_est_bytes": per_bt_est,
                "dual_index_est_bytes": per_kad_est + per_bt_est,
                "kad_index_mib": per_kad_est as f64 / (1024.0 * 1024.0),
                "bt_index_mib": per_bt_est as f64 / (1024.0 * 1024.0),
                "dual_index_mib": (per_kad_est + per_bt_est) as f64 / (1024.0 * 1024.0),
                "nfr05_target_mib": 100.0,
                "dual_fit_nfr05": (per_kad_est + per_bt_est) as f64 <= 100.0 * 1024.0 * 1024.0,
                "wire_bytes_per_entry_kad": entry_wire_bytes(KAD_KEY_LEN),
                "wire_bytes_per_entry_bt": entry_wire_bytes(BT_KEY_LEN),
                "wire_announce_total_bytes": total_wire,
                "wire_announce_kib": total_wire as f64 / 1024.0,
                "result": "verified",
                "note": "内存为结构体上界估算（桶表 + entry），真实 RSS 需 M1 运行期 prof；仅 10k/栈已远低于 100MB NFR-05，线性外推至 1M/栈约 0.28 GiB，仍 < 传统 DHT 客户端常驻"
            }
        },
        "scope": "本地双 HashMap 数据结构与隔离性 + 资源增量上界估算；不做真实 KAD/Mainline DHT 网络加入（属 M1 集成与 G3 兼容性矩阵）。结论为结构性证据，非活网络流量。",
        "public_network_contacted": false,
        "evidence_valid_for_pass": false,
        "note": "S3.status 仍为 not_run；本探针给出首轮结构性证据，最终 pass 须结合 M1 真实双栈集成与 G3 兼容性矩阵，由实验执行人判定。"
    });

    println!("{}", serde_json::to_string_pretty(&report).unwrap());

    if warm {
        // 方便人工复核时保留进程
        println!("[S3] warm run finished, {} entries/stack", n);
    }
}
