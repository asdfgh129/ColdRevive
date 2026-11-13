//! cr-store — W2.4 影子索引（M1 最小实现）。
//!
//! 按 DATA §5，影子索引是 COLD 轻量 handle：不含 Bitfield、不含逐片哈希，
//! 因此 S4 的「1 万文件 ≈7.79 MiB」结论在此结构下仍成立。
//! M1 用 JSON 文件持久化；M2 再迁移到 SQLite（W1.2），接口保持不变。

use cr_core::Json;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

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
        }
    }
    pub fn transition(&mut self, to: FileState) {
        let s = to.as_str().to_string();
        if self.transitions.last() != Some(&s) { self.transitions.push(s); }
        self.state = to;
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
        })
    }
}

#[derive(Debug, Clone)]
pub struct ShadowIndex {
    path: PathBuf,
    pub entries: BTreeMap<String, FileEntry>,
}

impl ShadowIndex {
    pub fn open(path: &Path) -> ShadowIndex {
        let mut idx = ShadowIndex { path: path.to_path_buf(), entries: BTreeMap::new() };
        if let Ok(j) = Json::load(path) {
            if let Some(items) = j.get("entries").and_then(Json::as_arr) {
                for item in items {
                    if let Some(e) = FileEntry::from_json(item) {
                        idx.entries.insert(e.file_hash.clone(), e);
                    }
                }
            }
        }
        idx
    }
    pub fn save(&self) -> Result<(), String> {
        let j = Json::obj(vec![(
            "entries",
            Json::arr(self.entries.values().map(FileEntry::to_json).collect()),
        )]);
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(&self.path, j.dump()).map_err(|e| format!("{}: {}", self.path.display(), e))
    }
    pub fn insert(&mut self, e: FileEntry) { self.entries.insert(e.file_hash.clone(), e); }
    pub fn get(&self, hash: &str) -> Option<&FileEntry> { self.entries.get(hash) }
    pub fn get_mut(&mut self, hash: &str) -> Option<&mut FileEntry> { self.entries.get_mut(hash) }
    pub fn len(&self) -> usize { self.entries.len() }
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

    #[test]
    fn cold_handle_roundtrip_and_state_machine() {
        let p = std::env::temp_dir().join("cr-store-test.json");
        let _ = fs::remove_file(&p);
        let mut idx = ShadowIndex::open(&p);
        let mut e = FileEntry::new("abc", Path::new("sample.bin"), "sample.bin", 42);
        e.infohash = "deadbeef".into();
        e.published_keys = vec!["k1".into(), "k2".into()];
        e.kad_published = true;
        idx.insert(e);
        idx.save().unwrap();

        let mut back = ShadowIndex::open(&p);
        assert_eq!(back.len(), 1);
        assert_eq!(back.get("abc").unwrap().infohash, "deadbeef");
        assert_eq!(back.all_published_keys(), vec!["k1", "k2"]);
        back.get_mut("abc").unwrap().transition(FileState::Waking);
        back.get_mut("abc").unwrap().transition(FileState::Hot);
        assert_eq!(back.get("abc").unwrap().transitions,
                   vec!["COLD", "WAKING", "HOT"]);
        let _ = fs::remove_file(&p);
    }
}
