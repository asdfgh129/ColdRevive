//! cr-indexer — W2.1 共享目录监控 / W2.2 指纹计算（M1 最小实现）。
//!
//! M1 只做「把自制样本放进共享目录 → 立刻产生可发布的文件事实」这一条链路；
//! 完整 inotify/ReadDirectoryChangesW 去抖与增量哈希属 M2，本 crate 保留接口位置。

use cr_hash::sha256_hex;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct FileFacts {
    pub path: PathBuf,
    pub name: String,
    pub size: usize,
    pub file_hash: String,
    pub data: Vec<u8>,
}

/// 读取并指纹化一个共享文件。文件不存在时返回 Err，绝不合成哈希。
pub fn inspect(path: &Path) -> Result<FileFacts, String> {
    let data = fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("payload.bin").to_string();
    Ok(FileFacts {
        file_hash: sha256_hex(&data),
        size: data.len(),
        path: path.to_path_buf(),
        name,
        data,
    })
}

/// 去抖窗口（毫秒）。M1 保持单文件即时入库；M2 引入批量事件时复用此常量。
pub const DEBOUNCE_MS: u64 = 250;

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
}
