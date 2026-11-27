//! cr-kad — W3.1 KAD 适配层（M1 本地双节点实现）。
//!
//! 边界（T0 红线）：本 crate 只跑 `127.0.0.1` 私有子网，**不向公共 KAD 发布任何测试 Key**。
//! 公共 KAD 的 `0xE4` wire 映射已在 S2 spike 验证（ADR-001 = public），M3 再接入生产网络。
//!
//! 双栈 ID 宽度（S3 的结构性证据）：KAD 用 128-bit KadID，Mainline DHT 用 160-bit ID。
//! 两者同进程共存，命名空间/宽度不同，禁止互相误投；T0 的 `inspect kad` 会同时暴露两者。

use cr_core::{Json, NAMESPACE};
use cr_hash::{hex, sha1_hex, sha256_hex};

/// ADR-011 eMule 兼容轨的字节层原语（无前缀 MD4 Key + 0x33/0x3B 编解码，
/// 以及 0x01/0x09/0x21/0x29 节点发现与 0xE5 zlib 解包）。
/// 本 crate 不发包；公网命中只由调用方的真实 UDP 观测登记。
pub mod kad_ed2k;
/// aMule 对 >200 B 的 KAD 应答发 `0xE5` zlib 帧；这里是零依赖解码器。
/// 从 S2 `kad-mini/src/inflate.rs` 原样提升，避免两个实现漂移。
pub mod kad_inflate;
/// aMule KAD UDP 混淆（RC4 try-0 / try-2 信封）。真机证实公网节点普遍开混淆，
/// 明文 `0xE4` 的 0x33/0x43 会被静默丢弃；从 S2 `kad-mini/src/wire.rs` 提升。
pub mod kad_obfuscate;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::Duration;

/// T0 harness 的 Target_Key 口径：`SHA1(namespace || file_hash)` 的十六进制。
/// 生产公共 KAD 使用 128-bit KadID（见 `kad_id_128`），两者在 M1 显式区分，不混用。
pub fn target_key(namespace: &str, file_hash: &str) -> String {
    sha1_hex(format!("{}{}", namespace, file_hash).as_bytes())
}
pub fn keyword_key(namespace: &str, keyword: &str) -> String {
    sha1_hex(format!("{}kw:{}", namespace, keyword).as_bytes())
}
/// eMule KAD 的 128-bit ID（SHA-256 前 16 字节）。
pub fn kad_id_128(namespace: &str, seed: &str) -> String {
    let h = sha256_hex(format!("{}{}", namespace, seed).as_bytes());
    h[..32].to_string()
}
/// Mainline DHT 的 160-bit ID（SHA-1 / 20 字节）。
pub fn dht_id_160(namespace: &str, seed: &str) -> String {
    sha1_hex(format!("{}{}", namespace, seed).as_bytes())
}

/// 生成要发布的 KAD key：目标 key + 每个（**扩展后**）关键词一个 key。
///
/// 同义词扩展只作用于「发出去的 key」，`FileEntry.keywords` 仍保留原始词，
/// 这样 `/cli inspect entries` 能分清「文件名里真有的词」和「我们补的词」。
pub fn publish_keys(namespace: &str, file_hash: &str, keywords: &[String]) -> Vec<String> {
    let expanded = expand_keywords(keywords, MAX_KEYWORD_KEYS);
    let mut keys = vec![target_key(namespace, file_hash)];
    for kw in &expanded {
        keys.push(keyword_key(namespace, kw));
    }
    keys
}

/// ADR-011 A 轨（eMule 兼容）关键词 key：`MD4(UTF-8(term))` 无前缀，32 位十六进制。
///
/// 与 B 轨是两套算法，**不共用** `publish_keys` 的结果；这里只算 key，不发包，
/// 也不碰公共 KAD。上限仍沿用 `MAX_KEYWORD_KEYS`（每文件 ≤ 8 个关键词）。
pub fn ed2k_keyword_keys(keywords: &[String]) -> Vec<String> {
    expand_keywords(keywords, MAX_KEYWORD_KEYS).iter()
        .map(|kw| hex(&kad_ed2k::ed2k_keyword_key(kw)))
        .collect()
}

/// FR-25 双轨发布帧的 `keys[]`：B 轨（已入库的 `published_keys`）+ A 轨（MD4 兼容轨）。
///
/// 刻意**不**改 `FileEntry.published_keys` / `inspect kad`：T0/T1 的 B 轨口径与
/// `keyword_count` 是冻结门禁夹具，A 轨只在真正发 `KAD_PUBLISH` 帧时并进去。
/// 收到帧的一方会把两轨 key 都记入 `remote_keys`，`KAD_FIND` 两轨都能命中。
/// ACK 仍只表示对端收下 key，不代表公网 KAD 可检索；双轨发布量翻倍按 R-02 复审。
pub fn dual_track_frame_keys(keywords: &[String], published_keys: &[String]) -> Vec<String> {
    let mut keys = published_keys.to_vec();
    keys.extend(ed2k_keyword_keys(keywords));
    keys.sort();
    keys.dedup();
    keys
}

/// 手写、封闭的同义词组。**刻意不用**通用词库或模型：那会让同一个文件名在
/// 不同机器/不同版本切出不同 key，而 KAD 的 key 一旦不可复算就等于发了个黑洞。
/// 每组第一个词是规范词；扩展时把同组其它词追加在原文之后，去重保序并封顶。
pub const LATIN_SYNONYM_GROUPS: &[&[&str]] = &[
    &["movie", "film", "cinema"],
    &["music", "song", "audio"],
    &["book", "novel", "ebook"],
    &["photo", "picture", "image"],
    &["video", "clip"],
    &["game", "gaming"],
    &["software", "program", "app"],
];

/// 中文同义词组。与拉丁组分开，由**每个词自己的字符脚本**选择，
/// 不靠整个文件名的语言标签——混名文件（中文标题 + 英文编码标记）很常见。
pub const CJK_SYNONYM_GROUPS: &[&[&str]] = &[
    &["电影", "影片", "片子"],
    &["音乐", "歌曲"],
    &["小说", "书籍"],
    &["图片", "照片"],
    &["视频", "录像"],
];

/// 单个 Unicode 字符的脚本标签。只认与检索有关的几种，其余统一 `unknown`。
pub fn char_script(ch: char) -> &'static str {
    let u = ch as u32;
    if matches!(u, 0x3040..=0x30ff) { return "ja" }   // 平假名/片假名
    if matches!(u, 0xac00..=0xd7af) { return "ko" }   // 韩文音节
    if matches!(u, 0x0400..=0x04ff) { return "ru" }   // 西里尔
    if matches!(u, 0x0370..=0x03ff) { return "el" }   // 希腊
    if matches!(u, 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff) { return "zh" }
    if ch.is_ascii_alphabetic() { return "en" }
    "unknown"
}

/// 一个词的语言：按出现的字母脚本计数，取最多的那个；
/// 平局按 ja > ko > ru > el > zh > en 的固定顺序，保证可复算。
pub fn token_language(tok: &str) -> &'static str {
    const PRIORITY: &[&str] = &["ja", "ko", "ru", "el", "zh", "en"];
    let mut best = "unknown";
    let mut best_n = 0usize;
    for lang in PRIORITY {
        let n = tok.chars().filter(|c| char_script(*c) == *lang).count();
        if n > best_n {
            best = lang;
            best_n = n;
        }
    }
    best
}

/// 整段文本的语言：**只用于展示与审计**，不参与 key 生成。
/// 出现多种脚本就报 `mixed`——这比挑一个「主要语言」更诚实。
pub fn detect_language(text: &str) -> &'static str {
    let mut found: Vec<&str> = Vec::new();
    for ch in text.chars() {
        let s = char_script(ch);
        if s != "unknown" && !found.iter().any(|x| *x == s) {
            found.push(s);
        }
    }
    match found.len() {
        0 => "unknown",
        1 => found[0],
        _ => "mixed",
    }
}

/// FR-07：在原有关键词之后追加**确定性**同义词。
///
/// 规则：原文全部保留且排在最前（运营者显式给的关键词永远优先）；
/// 每个原词按它自己的脚本选同义词组；同组成员按组内固定顺序追加；
/// 全程去重保序，最后硬截到 `limit`（与 key 数上限对齐）。
/// 不认识的词原样保留——「没扩展」比「扩展错」好。
pub fn expand_keywords(keywords: &[String], limit: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        if out.len() < limit && !out.iter().any(|x| x == s) {
            out.push(s.to_string());
        }
    };
    for kw in keywords.iter().take(limit) {
        push(kw);
    }
    for kw in keywords.iter().take(limit) {
        let low = kw.to_lowercase();
        let groups = match token_language(&low) {
            "ja" | "ko" | "zh" => CJK_SYNONYM_GROUPS,
            _ => LATIN_SYNONYM_GROUPS,
        };
        for group in groups {
            if group.iter().any(|x| *x == low) {
                for syn in *group {
                    push(syn);
                }
            }
        }
    }
    out
}

/// 文件名里常见的「容器/编码/来源」噪声词。它们在任何检索里都命中一片、
/// 却不指向任何具体内容，发上去只会稀释关键词质量。
pub const NOISE_TOKENS: &[&str] = &[
    "mkv", "mp4", "avi", "wmv", "flv", "mov", "mpg", "mpeg", "iso", "rar", "zip", "7z",
    "x264", "x265", "h264", "h265", "hevc", "avc", "aac", "ac3", "dts", "flac", "mp3",
    "1080p", "720p", "480p", "2160p", "4k", "webrip", "webdl", "bluray", "bdrip", "hdtv",
    "remux", "repack", "proper", "internal", "hdr", "sdr", "yify", "rarbg",
    // 通用二进制/文本扩展名：它们不是内容词，混进关键词还会把整段文本的语言
    // 判成 mixed（中文标题 + .bin 就是这么被误判的）。
    "bin", "dat", "exe", "dll", "so", "dylib", "txt", "log", "csv", "json",
    "tar", "gz", "bz2", "xz",
];

/// FR-07：把文件名切成 KAD 关键词。
///
/// 规则刻意保守，且**只做确定性变换**（同一个名字永远切出同一组词，便于复算）：
/// - 只在「非字母数字」处切分；CJK 连续串会成为一个词——中文没有空格，
///   按「每字一词」切会让一个四字标题变成四条互不相干的 key；
/// - ASCII 转小写；纯数字丢掉（`2024`、`01` 这类不带内容信息）；
/// - ASCII 词短于 3 个字符丢掉，CJK 词短于 2 个字符丢掉；
/// - 命中 [`NOISE_TOKENS`] 的丢掉；
/// - 去重保持首次出现顺序，最多 `limit` 个（与 [`publish_keys`] 的 8 个上限对齐）。
///
/// 本函数**只切词**：同义词扩展在 [`expand_keywords`] 里单独做，语言识别在
/// [`detect_language`] 里单独做。两者都是封闭表 + Unicode 脚本判定的确定性变换，
/// 不引入词库版本/模型版本这种「同一输入不同机器不同结果」的来源。
pub fn tokenize_keywords(name: &str, limit: usize) -> Vec<String> {
    fn keep(tok: &str) -> bool {
        if tok.is_empty() {
            return false;
        }
        if tok.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        let ascii_only = tok.is_ascii();
        if ascii_only && tok.len() < 3 {
            return false;
        }
        if !ascii_only && tok.chars().count() < 2 {
            return false;
        }
        if ascii_only && NOISE_TOKENS.contains(&tok) {
            return false;
        }
        true
    }

    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let push = |cur: &mut String, out: &mut Vec<String>| {
        if cur.is_empty() {
            return;
        }
        let low = cur.to_lowercase();
        cur.clear();
        if keep(&low) && !out.iter().any(|x| x == &low) && out.len() < limit {
            out.push(low);
        }
    };
    for ch in name.chars() {
        if ch.is_alphanumeric() {
            cur.push(ch);
        } else {
            push(&mut cur, &mut out);
        }
    }
    push(&mut cur, &mut out);
    out
}

#[cfg(test)]
mod tokenizer_tests {
    use super::*;

    #[test]
    fn tokenizer_is_deterministic_and_lowercases() {
        let a = tokenize_keywords("Cold.Revive_Sample-2024.MKV", 8);
        let b = tokenize_keywords("Cold.Revive_Sample-2024.MKV", 8);
        assert_eq!(a, b, "同一名字必须切出同一组词");
        assert_eq!(a, vec!["cold", "revive", "sample"]);
    }

    #[test]
    fn tokenizer_drops_noise_digits_and_short_tokens() {
        let t = tokenize_keywords("Movie.1080p.x265.AAC-GROUP.2024.mkv", 8);
        assert!(!t.iter().any(|x| x == "mkv"), "容器名不该成为关键词: {t:?}");
        assert!(!t.iter().any(|x| x == "2024"), "纯数字不该成为关键词: {t:?}");
        assert!(!t.iter().any(|x| x == "hd"), "两字母 ASCII 词要丢掉: {t:?}");
        assert!(t.contains(&"movie".to_string()));
        assert!(t.contains(&"group".to_string()));
    }

    #[test]
    fn tokenizer_keeps_cjk_runs_whole() {
        let t = tokenize_keywords("冷启动复活-测试样本.bin", 8);
        assert!(t.contains(&"冷启动复活".to_string()), "CJK 连续串应整体保留: {t:?}");
        assert!(t.contains(&"测试样本".to_string()), "CJK 连续串应整体保留: {t:?}");
    }

    #[test]
    fn tokenizer_dedupes_and_honours_limit() {
        let t = tokenize_keywords("alpha beta alpha gamma delta epsilon zeta eta theta", 4);
        assert_eq!(t.len(), 4, "必须尊重上限");
        assert_eq!(t.iter().filter(|x| *x == "alpha").count(), 1, "必须去重");
        assert_eq!(t[0], "alpha", "去重应保留首次出现顺序");
    }

    #[test]
    fn tokenizer_handles_degenerate_input() {
        assert!(tokenize_keywords("", 8).is_empty());
        assert!(tokenize_keywords("---___...", 8).is_empty());
        assert!(tokenize_keywords("ab cd", 8).is_empty(), "全是短词时应为空");
    }

    #[test]
    fn synonym_expansion_is_deterministic_and_keeps_originals_first() {
        let a = expand_keywords(&["Movie".into(), "collection".into()], 8);
        let b = expand_keywords(&["Movie".into(), "collection".into()], 8);
        assert_eq!(a, b);
        assert_eq!(a[0], "Movie", "原始关键词不能被同义词顶掉");
        assert!(a.contains(&"film".to_string()) && a.contains(&"cinema".to_string()));
        // 同组两个原词同时出现时也不能重复追加。
        let c = expand_keywords(&["movie".into(), "film".into()], 8);
        assert_eq!(c.iter().filter(|x| *x == "cinema").count(), 1);
    }

    #[test]
    fn cjk_synonyms_use_cjk_table_not_latin_table() {
        let out = expand_keywords(&["电影".into()], 8);
        assert!(out.contains(&"影片".to_string()) && out.contains(&"片子".to_string()));
        assert!(!out.iter().any(|x| x == "movie"));
        assert_eq!(token_language("电影"), "zh");
        assert_eq!(token_language("カタカナ"), "ja");
    }

    #[test]
    fn expansion_honours_limit_and_leaves_unknown_words_alone() {
        let many: Vec<String> = (0..20).map(|i| format!("word{i}")).collect();
        let out = expand_keywords(&many, 8);
        assert_eq!(out.len(), 8);
        assert_eq!(out, many[..8]);
        assert_eq!(expand_keywords(&["zzzzunknown".into()], 8), vec!["zzzzunknown"]);
    }

    #[test]
    fn language_detection_is_explicit_about_mixed_scripts() {
        assert_eq!(detect_language("cold revive sample"), "en");
        assert_eq!(detect_language("冷启动复活测试样本"), "zh");
        assert_eq!(detect_language("Привет"), "ru");
        assert_eq!(detect_language("冷启动 sample"), "mixed");
        assert_eq!(detect_language("2024---"), "unknown");
    }

    #[test]
    fn published_keys_include_expansion_but_stay_capped() {
        let kws = vec!["movie".to_string(), "book".to_string()];
        let keys = publish_keys("ns:", "h", &kws);
        // B 轨：1 个 target + 若干关键词 key；不超过上限即可（不该为了凑满而灌水）。
        assert!(keys.len() <= 1 + MAX_KEYWORD_KEYS && keys.len() > 3);
        assert!(keys.contains(&keyword_key("ns:", "cinema")));
        assert!(keys.contains(&keyword_key("ns:", "novel")));
        // A 轨只进发布帧，不污染冻结的 published_keys 口径。
        assert!(!keys.contains(&hex(&kad_ed2k::ed2k_keyword_key("cinema"))));
        let frame = dual_track_frame_keys(&kws, &keys);
        assert!(frame.contains(&keyword_key("ns:", "cinema")));
        assert!(frame.contains(&hex(&kad_ed2k::ed2k_keyword_key("cinema"))));
        assert!(frame.contains(&hex(&kad_ed2k::ed2k_keyword_key("novel"))));
    }
}

pub fn find_frame(key: &str) -> Json {
    Json::obj(vec![("t", Json::s("KAD_FIND")), ("key", Json::s(key))])
}
pub fn find_result(hit_node: &str, count: usize) -> Json {
    Json::obj(vec![
        ("t", Json::s("KAD_FIND_RES")),
        ("hit_node", Json::s(hit_node)),
        ("count", Json::n(count as f64)),
    ])
}

/// 单包 UDP 请求/响应。超时返回 None，由调用方决定超时/换候选，**绝不合成响应**。
pub fn udp_roundtrip(bind: &str, peer: SocketAddr, frame: &Json,
                     timeout_ms: u64) -> Result<(Json, u64), String> {
    let socket = UdpSocket::bind(format!("{}:0", bind))
        .map_err(|e| format!("UDP bind 失败: {}", e))?;
    socket.set_read_timeout(Some(Duration::from_millis(timeout_ms)))
        .map_err(|e| e.to_string())?;
    let payload = frame.dump();
    let start = std::time::Instant::now();
    socket.send_to(payload.as_bytes(), peer)
        .map_err(|e| format!("UDP send 失败: {}", e))?;
    let mut buf = [0u8; 65535];
    let (n, _) = socket.recv_from(&mut buf)
        .map_err(|e| format!("UDP 超时或失败: {}", e))?;
    let elapsed = start.elapsed().as_millis() as u64;
    let text = String::from_utf8_lossy(&buf[..n]);
    let reply = Json::parse(&text).map_err(|e| format!("非法响应 JSON: {}", e))?;
    Ok((reply, elapsed))
}

pub fn udp_reply(socket: &UdpSocket, target: SocketAddr, frame: &Json) {
    let _ = socket.send_to(frame.dump().as_bytes(), target);
}

pub fn socket_addr(ip: &str, port: u16) -> Result<SocketAddr, String> {
    let parsed: IpAddr = ip.parse().map_err(|e| format!("非法 IP {}: {}", ip, e))?;
    Ok(SocketAddr::new(parsed, port))
}

pub const DEFAULT_NAMESPACE: &str = NAMESPACE;
pub const MAX_KEYWORD_KEYS: usize = 8;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_match_t0_fixture_derivation() {
        // 与 tests/e2e/t0/t0_harness.py 的 hashlib.sha1(ns+file_hash) 一致
        let k = target_key("ColdRevive_V1:", "abc");
        assert_eq!(k.len(), 40);
        assert_eq!(k, sha1_hex(b"ColdRevive_V1:abc"));
        let kw = keyword_key("ColdRevive_V1:", "coldrevive");
        assert_eq!(kw, sha1_hex(b"ColdRevive_V1:kw:coldrevive"));
    }

    #[test]
    fn dual_stack_ids_have_distinct_widths() {
        assert_eq!(kad_id_128("ColdRevive_V1:", "x").len(), 32); // 128 bit
        assert_eq!(dht_id_160("ColdRevive_V1:", "x").len(), 40); // 160 bit
        assert_ne!(kad_id_128("ColdRevive_V1:", "x"), dht_id_160("ColdRevive_V1:", "x"));
    }

    #[test]
    fn keyword_cap_is_enforced() {
        let kws: Vec<String> = (0..20).map(|i| format!("k{}", i)).collect();
        assert_eq!(publish_keys("ColdRevive_V1:", "h", &kws).len(), 1 + MAX_KEYWORD_KEYS);
        assert_eq!(ed2k_keyword_keys(&kws).len(), MAX_KEYWORD_KEYS);
        assert_eq!(dual_track_frame_keys(&kws, &publish_keys("ColdRevive_V1:", "h", &kws)).len(),
                   1 + 2 * MAX_KEYWORD_KEYS);
    }

    #[test]
    fn a_track_key_is_unprefixed_md4_and_distinct_from_b_track() {
        let term = "zzz-no-synonym-term".to_string();
        let a = ed2k_keyword_keys(&[term.clone()]);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0], hex(&cr_hash::md4(term.as_bytes())));
        assert_eq!(a[0].len(), 32);
        assert_ne!(a[0], keyword_key("ColdRevive_V1:", &term));
    }
}
