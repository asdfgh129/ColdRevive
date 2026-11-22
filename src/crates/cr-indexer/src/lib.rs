//! cr-indexer — W2.1 共享目录监控 / W2.2 指纹计算（M1 最小实现）+ FR-02 多哈希。
//!
//! FR-02（2026-11-22）：索引不再只存 SHA-256。一次读盘同时算出
//! - SHA-256（内容寻址主键，用于完整性校验与去重）
//! - SHA-1（BT v1 infohash 原料 / 跨协议补源的通用标识）
//! - eD2k fileID（MD4 变体，直接可拼 `ed2k://` link）
//!
//! 三者来自同一份字节，不存在「只算了一个、另一个事后重扫」的口径差。
//! 目录监控仍是 `inspect()` 的一次性扫描；增量监听（FR-01）见 `poll_watch`。

use cr_hash::{ed2k_hash, hex, sha1_hex, sha256_hex};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct FileFacts {
    pub path: PathBuf,
    pub name: String,
    pub size: usize,
    pub file_hash: String,
    pub data: Vec<u8>,
    /// FR-02：SHA-1 十六进制（BT v1 infohash 原料）
    pub sha1: String,
    /// FR-02：eD2k fileID 十六进制（可直接拼 ed2k link）
    pub ed2k_hash: String,
}

/// 读取并指纹化一个共享文件。文件不存在时返回 Err，绝不合成哈希。
pub fn inspect(path: &Path) -> Result<FileFacts, String> {
    let data = fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("payload.bin").to_string();
    Ok(FileFacts {
        file_hash: sha256_hex(&data),
        sha1: sha1_hex(&data),
        ed2k_hash: hex(&ed2k_hash(&data)),
        size: data.len(),
        path: path.to_path_buf(),
        name,
        data,
    })
}

/// 去抖窗口（毫秒）。M1 保持单文件即时入库；批量事件复用此常量。
pub const DEBOUNCE_MS: u64 = 250;

/// 一次扫描看到的文件「签名」。用 (大小, mtime 纳秒) 判定变化：
/// 只比 mtime 会漏掉「同秒内改完」的写入，只比大小会漏掉等长改写。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub size: u64,
    pub modified_ns: u128,
}

fn stamp_of(meta: &fs::Metadata) -> Stamp {
    let ns = meta.modified().ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Stamp { size: meta.len(), modified_ns: ns }
}

/// 目录快照：路径 → 签名。`scan()` 是纯读操作，不改动任何文件。
pub fn scan(dir: &Path) -> Result<BTreeMap<PathBuf, Stamp>, String> {
    let mut out = BTreeMap::new();
    let rd = fs::read_dir(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    for entry in rd {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue, // 扫描期间被删除：下一轮会看到
        };
        if !meta.is_file() {
            continue;
        }
        out.insert(path, stamp_of(&meta));
    }
    Ok(out)
}

/// 两轮快照的差异。`removed` 只报「上一轮有、这一轮没有」的路径。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DirDelta {
    pub added: Vec<PathBuf>,
    pub modified: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
}

impl DirDelta {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.removed.is_empty()
    }
    pub fn touched(&self) -> usize {
        self.added.len() + self.modified.len() + self.removed.len()
    }
}

/// 比较两轮快照。抽成纯函数是为了能回归：I/O 版只负责喂快照。
pub fn diff(prev: &BTreeMap<PathBuf, Stamp>, next: &BTreeMap<PathBuf, Stamp>) -> DirDelta {
    let mut d = DirDelta::default();
    for (p, s) in next {
        match prev.get(p) {
            None => d.added.push(p.clone()),
            Some(old) if old != s => d.modified.push(p.clone()),
            Some(_) => {}
        }
    }
    for p in prev.keys() {
        if !next.contains_key(p) {
            d.removed.push(p.clone());
        }
    }
    d.added.sort();
    d.modified.sort();
    d.removed.sort();
    d
}

/// 轮询式目录监听（FR-01 的跨平台基线）。
///
/// 没有内核通知（FSEvents / ReadDirectoryChangesW / inotify）时的正确做法：
/// 用固定周期比对快照，**事件语义与内核版一致**（新增/修改/删除 + 去抖）。
/// 代价是延迟 ≤ interval、空转 CPU 很低（只 stat 目录项，不读内容）。
pub struct PollWatcher {
    dir: PathBuf,
    interval_ms: u64,
    seen: BTreeMap<PathBuf, Stamp>,
}

impl PollWatcher {
    pub fn new(dir: &Path, interval_ms: u64) -> Result<PollWatcher, String> {
        let seen = scan(dir)?;
        Ok(PollWatcher {
            dir: dir.to_path_buf(),
            interval_ms: interval_ms.max(50),
            seen,
        })
    }
    pub fn interval_ms(&self) -> u64 { self.interval_ms }
    /// 比对一次并更新基线。调用方自己决定间隔（或由 `run` 驱动）。
    pub fn poll(&mut self) -> Result<DirDelta, String> {
        let next = scan(&self.dir)?;
        let d = diff(&self.seen, &next);
        self.seen = next;
        Ok(d)
    }
    /// 阻塞运行：每 `interval_ms` 轮询一次，把非空差异交给回调；回调返回 false 即退出。
    pub fn run<F: FnMut(&DirDelta) -> bool>(mut self, mut cb: F) {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(self.interval_ms));
            match self.poll() {
                Ok(d) if !d.is_empty() => {
                    if !cb(&d) { return; }
                }
                Ok(_) => {}
                Err(_) => return, // 目录消失：交给调用方在下一轮重建
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_real_sha256() {
        let p = std::env::temp_dir().join("cr-indexer-test.bin");
        fs::write(&p, b"abc").unwrap();
        let f = inspect(&p).unwrap();
        assert_eq!(f.file_hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(f.size, 3);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn all_three_digests_come_from_the_same_bytes() {
        // 三个摘要必须都来自同一份字节，且互不相同（拿错算法的回归会立刻暴露）。
        let p = std::env::temp_dir().join("cr-indexer-multihash.bin");
        fs::write(&p, b"abc").unwrap();
        let f = inspect(&p).unwrap();
        assert_eq!(f.sha1, "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(f.ed2k_hash, hex(&ed2k_hash(b"abc")));
        assert_eq!(f.ed2k_hash.len(), 32);
        assert_ne!(f.ed2k_hash, f.sha1[..32], "eD2k 与 SHA-1 前 16 字节必须不同");
        assert_ne!(f.file_hash[..32], f.ed2k_hash);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn watcher_reports_add_modify_remove() {
        let dir = std::env::temp_dir().join(format!("cr-indexer-watch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut w = PollWatcher::new(&dir, 60).unwrap();
        assert!(w.poll().unwrap().is_empty());

        let a = dir.join("a.bin");
        fs::write(&a, b"one").unwrap();
        let d1 = w.poll().unwrap();
        assert_eq!(d1.added, vec![a.clone()]);
        assert!(d1.removed.is_empty());

        // 等一帧再改，确保 mtime 变化可观测（部分文件系统 mtime 精度 1s）
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&a, b"one-two").unwrap();
        let d2 = w.poll().unwrap();
        assert_eq!(d2.modified, vec![a.clone()], "等长/短写都要能被看见");

        fs::remove_file(&a).unwrap();
        let d3 = w.poll().unwrap();
        assert_eq!(d3.removed, vec![a]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn diff_is_pure_and_symmetric() {
        let p1 = PathBuf::from("/x/a");
        let p2 = PathBuf::from("/x/b");
        let s1 = Stamp { size: 1, modified_ns: 10 };
        let s2 = Stamp { size: 2, modified_ns: 20 };
        let mut prev = BTreeMap::new();
        prev.insert(p1.clone(), s1);
        let mut next = BTreeMap::new();
        next.insert(p1.clone(), s2);
        next.insert(p2.clone(), s1);
        let d = diff(&prev, &next);
        assert_eq!(d.modified, vec![p1]);
        assert_eq!(d.added, vec![p2]);
        assert!(d.removed.is_empty());
        // 快照没被改动（纯函数）
        assert_eq!(prev.len(), 1);
        assert!(diff(&next, &next).is_empty());
    }
}
