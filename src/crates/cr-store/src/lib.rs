//! cr-store — W2.4 影子索引（M1 最小实现）+ FR-04 持久化加固。
//!
//! 按 DATA §5，影子索引是 COLD 轻量 handle：不含 Bitfield、不含逐片哈希，
//! 因此 S4 的「1 万文件 ≈7.79 MiB」结论在此结构下仍成立。
//!
//! **FR-04（2026-11-22）**：M1 的 JSON 落盘升级为「带 schema 的原子快照」：
//! - 文件头写 `schema`，v1（无该字段）读出后标记 `LoadedV1`，`save()` 即迁移到 v2；
//! - 写入走 `tmp → fsync → 备份旧文件 → rename`，任何时刻断电都只会丢最后一次
//!   未完成的写入，不会留下被截断的主文件；
//! - 主文件解析失败时自动回退到 `.bak`，并把状态暴露为 `RecoveredFromBackup`，
//!   由调用方（守护进程 `/health`、`/cli state`）如实呈现，不静默重建。
//!
//! 为什么不是 SQLite：ADR-015 明确 cr-store 属协议/信令面、保持 std-only；
//! 换引擎需要所有者先改 ADR。这里先把「不丢数据 + 可迁移 + 可恢复」做完，
//! 引擎替换留在 W1.2 的接口之后（`open/save` 语义不变）。

use cr_core::Json;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 当前落盘格式版本。v1 无 `schema` 字段（M1 早期）；v2 起带 schema 与 saved_at。
pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq)]
pub enum FileState { Cold, Waking, Hot }

impl FileState {
    pub fn as_str(&self) -> &'static str {
        match self { FileState::Cold => "COLD", FileState::Waking => "WAKING", FileState::Hot => "HOT" }
    }
    pub fn parse(s: &str) -> FileState {
        match s { "WAKING" => FileState::Waking, "HOT" => FileState::Hot, _ => FileState::Cold }
    }
}

/// 索引打开时的真实状态。**不允许把「恢复自备份」读成「正常加载」**：
/// 这条区分是 FR-04 的可观测性要求，否则索引被重建了也没人知道。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpenStatus {
    /// 文件不存在，从空索引开始
    Fresh,
    /// 读到 v1（无 schema），下次 save() 自动写 v2
    LoadedV1,
    /// 读到当前版本
    LoadedV2,
    /// 主文件损坏，已从 .bak 恢复
    RecoveredFromBackup,
    /// 主文件与备份都不可用，索引为空（**调用方必须告警**）
    Corrupt,
}

impl OpenStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            OpenStatus::Fresh => "fresh",
            OpenStatus::LoadedV1 => "loaded_v1",
            OpenStatus::LoadedV2 => "loaded_v2",
            OpenStatus::RecoveredFromBackup => "recovered_from_backup",
            OpenStatus::Corrupt => "corrupt",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub file_hash: String,
    pub path: PathBuf,
    pub name: String,
    pub size: usize,
    pub state: FileState,
    pub transitions: Vec<String>,
    pub torrent_path: PathBuf,
    pub infohash: String,
    pub piece_size: usize,
    pub keywords: Vec<String>,
    pub published_keys: Vec<String>,
    pub kad_published: bool,
    /// FR-02：ed2k fileID（MD4 变体）的小写十六进制。空串表示「尚未计算」，
    /// 绝不拿别的哈希顶替——ed2k link 一旦用错哈希就是另一个文件。
    pub ed2k_hash: String,
    /// FR-02：SHA-1 十六进制（BT v1 infohash 的原料，也是跨协议补源的索引键）
    pub sha1: String,
}

impl FileEntry {
    pub fn new(file_hash: &str, path: &Path, name: &str, size: usize) -> FileEntry {
        FileEntry {
            file_hash: file_hash.to_string(),
            path: path.to_path_buf(),
            name: name.to_string(),
            size,
            state: FileState::Cold,
            transitions: vec!["COLD".to_string()],
            torrent_path: PathBuf::new(),
            infohash: String::new(),
            piece_size: 0,
            keywords: Vec::new(),
            published_keys: Vec::new(),
            kad_published: false,
            ed2k_hash: String::new(),
            sha1: String::new(),
        }
    }
    pub fn transition(&mut self, to: FileState) {
        let s = to.as_str().to_string();
        if self.transitions.last() != Some(&s) { self.transitions.push(s); }
        self.state = to;
    }
    /// FR-12：能否在不重新读盘的前提下给出 ed2k link 的三个要素。
    pub fn can_link_ed2k(&self) -> bool {
        self.ed2k_hash.len() == 32 && self.size > 0 && !self.name.is_empty()
    }
    fn to_json(&self) -> Json {
        Json::obj(vec![
            ("file_hash", Json::s(&self.file_hash)),
            ("path", Json::s(self.path.to_string_lossy())),
            ("name", Json::s(&self.name)),
            ("size", Json::n(self.size as f64)),
            ("state", Json::s(self.state.as_str())),
            ("transitions", Json::arr(self.transitions.iter().map(|s| Json::s(s)).collect())),
            ("torrent_path", Json::s(self.torrent_path.to_string_lossy())),
            ("infohash", Json::s(&self.infohash)),
            ("piece_size", Json::n(self.piece_size as f64)),
            ("keywords", Json::arr(self.keywords.iter().map(|s| Json::s(s)).collect())),
            ("published_keys", Json::arr(self.published_keys.iter().map(|s| Json::s(s)).collect())),
            ("kad_published", Json::b(self.kad_published)),
            ("ed2k_hash", Json::s(&self.ed2k_hash)),
            ("sha1", Json::s(&self.sha1)),
        ])
    }
    fn from_json(j: &Json) -> Option<FileEntry> {
        let s = |k: &str| j.get(k).and_then(Json::as_str).unwrap_or("").to_string();
        Some(FileEntry {
            file_hash: s("file_hash"),
            path: PathBuf::from(s("path")),
            name: s("name"),
            size: j.get("size").and_then(Json::as_i64).unwrap_or(0) as usize,
            state: FileState::parse(&s("state")),
            transitions: j.get("transitions").and_then(Json::as_arr).unwrap_or(&[])
                .iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            torrent_path: PathBuf::from(s("torrent_path")),
            infohash: s("infohash"),
            piece_size: j.get("piece_size").and_then(Json::as_i64).unwrap_or(0) as usize,
            keywords: j.get("keywords").and_then(Json::as_arr).unwrap_or(&[])
                .iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            published_keys: j.get("published_keys").and_then(Json::as_arr).unwrap_or(&[])
                .iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            kad_published: j.get("kad_published").and_then(Json::as_bool).unwrap_or(false),
            ed2k_hash: s("ed2k_hash"),
            sha1: s("sha1"),
        })
    }
}

#[derive(Debug, Clone)]
pub struct ShadowIndex {
    path: PathBuf,
    pub entries: BTreeMap<String, FileEntry>,
    /// 本次打开的真实来源（FR-04 可观测性）
    pub status: OpenStatus,
    /// 打开时文件里声明的 schema（0 表示 v1 / 无字段）
    pub loaded_schema: u32,
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

fn tmp_path(path: &Path) -> PathBuf {
    path.with_extension("json.tmp")
}

fn parse_bytes(bytes: &[u8]) -> Option<(BTreeMap<String, FileEntry>, u32)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let j = Json::parse(text).ok()?;
    let schema = j.get("schema").and_then(Json::as_i64).unwrap_or(0) as u32;
    let mut map = BTreeMap::new();
    if let Some(items) = j.get("entries").and_then(Json::as_arr) {
        for item in items {
            if let Some(e) = FileEntry::from_json(item) {
                map.insert(e.file_hash.clone(), e);
            }
        }
    }
    Some((map, schema))
}

fn load_path(path: &Path) -> Option<(BTreeMap<String, FileEntry>, u32)> {
    let bytes = fs::read(path).ok()?;
    parse_bytes(&bytes)
}

impl ShadowIndex {
    pub fn open(path: &Path) -> ShadowIndex {
        let mut idx = ShadowIndex {
            path: path.to_path_buf(),
            entries: BTreeMap::new(),
            status: OpenStatus::Fresh,
            // `fresh` 表示「没有旧文件」，但内存里的索引**就在当前 schema 上**：
            // 下一次 `save()` 写出来的必然是 v2。报 0 会让 `/cli inspect` 在
            // 一个新建节点上显示 `schema_version=0`，看起来像「格式未知」。
            loaded_schema: SCHEMA_VERSION,
        };
        if !path.exists() {
            return idx;
        }
        if let Some((entries, schema)) = load_path(path) {
            idx.entries = entries;
            idx.loaded_schema = schema;
            idx.status = if schema >= SCHEMA_VERSION { OpenStatus::LoadedV2 } else { OpenStatus::LoadedV1 };
            return idx;
        }
        // 主文件存在但读不出来：回退备份，并把「恢复」与「正常加载」分开记。
        let bak = backup_path(path);
        if let Some((entries, schema)) = load_path(&bak) {
            idx.entries = entries;
            idx.loaded_schema = schema;
            idx.status = OpenStatus::RecoveredFromBackup;
            return idx;
        }
        idx.status = OpenStatus::Corrupt;
        idx
    }

    /// 原子落盘：tmp 写完并 fsync 后才替换主文件，旧主文件先转成 .bak。
    /// 任一步失败都不会让主文件变成半截。
    pub fn save(&self) -> Result<(), String> {
        let saved_at = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let j = Json::obj(vec![
            ("schema", Json::n(SCHEMA_VERSION as f64)),
            ("saved_at_unix", Json::n(saved_at as f64)),
            ("entries", Json::arr(self.entries.values().map(FileEntry::to_json).collect())),
        ]);
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {}", parent.display(), e))?;
        }
        let tmp = tmp_path(&self.path);
        {
            let mut f = fs::File::create(&tmp)
                .map_err(|e| format!("{}: {}", tmp.display(), e))?;
            f.write_all(j.dump().as_bytes())
                .map_err(|e| format!("{}: {}", tmp.display(), e))?;
            // fsync：不 sync 的话断电后 rename 可能指向空文件（ext4 delayed alloc 等）
            f.sync_all().map_err(|e| format!("{}: {}", tmp.display(), e))?;
        }
        if self.path.exists() {
            let bak = backup_path(&self.path);
            let _ = fs::remove_file(&bak);
            fs::rename(&self.path, &bak).map_err(|e| format!("{}: {}", bak.display(), e))?;
        }
        fs::rename(&tmp, &self.path).map_err(|e| format!("{}: {}", self.path.display(), e))?;
        Ok(())
    }

    /// 显式迁移：把 v1 读入的内存索引按当前 schema 重写一遍。
    /// `save()` 本身也会写 v2，这个入口是给运维/测试一个可断言的步骤。
    pub fn migrate(&mut self) -> Result<(), String> {
        if self.loaded_schema >= SCHEMA_VERSION {
            return Ok(());
        }
        self.save()?;
        self.loaded_schema = SCHEMA_VERSION;
        self.status = OpenStatus::LoadedV2;
        Ok(())
    }

    pub fn insert(&mut self, e: FileEntry) { self.entries.insert(e.file_hash.clone(), e); }
    pub fn get(&self, hash: &str) -> Option<&FileEntry> { self.entries.get(hash) }
    pub fn get_mut(&mut self, hash: &str) -> Option<&mut FileEntry> { self.entries.get_mut(hash) }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
    pub fn all_published_keys(&self) -> Vec<String> {
        let mut out = Vec::new();
        for e in self.entries.values() {
            if e.kad_published { out.extend(e.published_keys.iter().cloned()); }
        }
        out.sort(); out.dedup(); out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("cr-store-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn cold_handle_roundtrip_and_state_machine() {
        let dir = temp_dir("roundtrip");
        let p = dir.join("state.json");
        let mut idx = ShadowIndex::open(&p);
        assert_eq!(idx.status, OpenStatus::Fresh);
        let mut e = FileEntry::new("abc", Path::new("sample.bin"), "sample.bin", 42);
        e.infohash = "deadbeef".into();
        e.published_keys = vec!["k1".into(), "k2".into()];
        e.kad_published = true;
        idx.insert(e);
        idx.save().unwrap();

        let mut back = ShadowIndex::open(&p);
        assert_eq!(back.status, OpenStatus::LoadedV2);
        assert_eq!(back.len(), 1);
        assert_eq!(back.get("abc").unwrap().infohash, "deadbeef");
        assert_eq!(back.all_published_keys(), vec!["k1", "k2"]);
        back.get_mut("abc").unwrap().transition(FileState::Waking);
        back.get_mut("abc").unwrap().transition(FileState::Hot);
        assert_eq!(back.get("abc").unwrap().transitions.as_slice(),
                   ["COLD", "WAKING", "HOT"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn v1_index_is_migrated_in_place() {
        let dir = temp_dir("migrate");
        let p = dir.join("state.json");
        // M1 早期格式：只有 entries，没有 schema。
        fs::write(&p, r#"{"entries":[{"file_hash":"h1","path":"/tmp/a.bin","name":"a.bin","size":7,"state":"HOT","transitions":["COLD","HOT"],"torrent_path":"/tmp/a.torrent","infohash":"ih1","piece_size":16384,"keywords":["k"],"published_keys":["pk"],"kad_published":true}]}"#).unwrap();

        let mut idx = ShadowIndex::open(&p);
        assert_eq!(idx.status, OpenStatus::LoadedV1);
        assert_eq!(idx.loaded_schema, 0);
        assert_eq!(idx.len(), 1);
        assert_eq!(idx.get("h1").unwrap().state, FileState::Hot);

        idx.migrate().unwrap();
        assert_eq!(idx.status, OpenStatus::LoadedV2);
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.contains("\"schema\""), "迁移后必须写 schema 字段");
        let again = ShadowIndex::open(&p);
        assert_eq!(again.status, OpenStatus::LoadedV2);
        assert_eq!(again.len(), 1);
        assert_eq!(again.get("h1").unwrap().published_keys, vec!["pk"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_main_file_recovers_from_backup() {
        let dir = temp_dir("recover");
        let p = dir.join("state.json");
        let mut idx = ShadowIndex::open(&p);
        idx.insert(FileEntry::new("h1", Path::new("a.bin"), "a.bin", 3));
        idx.save().unwrap();          // 第一次写：无 .bak
        idx.save().unwrap();          // 第二次写：产生 .bak
        assert!(backup_path(&p).exists(), "第二次落盘后应有备份");

        fs::write(&p, "{not json").unwrap(); // 模拟断电/截断
        let back = ShadowIndex::open(&p);
        assert_eq!(back.status, OpenStatus::RecoveredFromBackup);
        assert_eq!(back.len(), 1);
        assert_eq!(back.get("h1").unwrap().size, 3);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn both_files_broken_is_reported_not_hidden() {
        let dir = temp_dir("corrupt");
        let p = dir.join("state.json");
        fs::write(&p, "garbage").unwrap();
        fs::write(backup_path(&p), "also garbage").unwrap();
        let idx = ShadowIndex::open(&p);
        assert_eq!(idx.status, OpenStatus::Corrupt);
        assert_eq!(idx.len(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_save_leaves_no_temp_file() {
        let dir = temp_dir("atomic");
        let p = dir.join("state.json");
        let mut idx = ShadowIndex::open(&p);
        idx.insert(FileEntry::new("h1", Path::new("a.bin"), "a.bin", 1));
        idx.save().unwrap();
        assert!(!tmp_path(&p).exists(), "落盘后不应残留 .tmp");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ed2k_and_sha1_survive_restart() {
        let dir = temp_dir("multi-hash");
        let p = dir.join("state.json");
        let mut idx = ShadowIndex::open(&p);
        let mut e = FileEntry::new("h", Path::new("a.bin"), "a.bin", 9);
        e.ed2k_hash = "00112233445566778899aabbccddeeff".into();
        e.sha1 = "a9993e364706816aba3e25717850c26c9cd0d89d".into();
        assert!(e.can_link_ed2k());
        idx.insert(e);
        idx.save().unwrap();

        let back = ShadowIndex::open(&p);
        let e = back.get("h").unwrap();
        assert_eq!(e.ed2k_hash, "00112233445566778899aabbccddeeff");
        assert_eq!(e.sha1, "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert!(e.can_link_ed2k());
        let _ = fs::remove_dir_all(&dir);
    }
}
