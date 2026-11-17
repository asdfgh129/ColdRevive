//! 出口普查（NAT egress census）—— 把「单机一次探测」沉淀成可归档、可复核、可汇总的样本。
//!
//! # 为什么需要它
//!
//! ADR-016（2026-11-17）后 S6 按 **eMule 式可达语义**验收，类别覆盖**不再是 pass 前提**：
//! 本模块的清单纯粹是描述性材料（「手上这些出口各是哪一类」），缺哪一类都不得据此判
//! S6 失败，也不得折算成矩阵覆盖。真实条件不齐时，正确的做法是把每次能做的观测按统一
//! 格式存下来，等条件齐了直接汇总，不必回头重测；本模块就是那个统一格式。
//!
//! # 门禁边界（必须逐条读）
//!
//! - 本模块产出**类别清单（class inventory）**，不产出**矩阵覆盖（matrix_coverage）**。
//!   清单回答「这个出口是哪一类」；覆盖回答「这一类能不能打通，30 次里成功几次」。
//!   前者不含任何可达性数字，**不能**折算成覆盖，也不能用于 R-04。
//! - 因此 `CensusSummary` 里 `matrix_coverage` 恒为 `"none"`、`r04_judgeable` 恒为 `false`、
//!   `s6_status` 恒为 `"not_run"`、`g2_claim` 恒为 `false`。这几个字段不是「暂时为 false」，
//!   而是本模块在原理上就不产生对应证据。
//! - 一个出口（`egress`）在**同一配置变体**下只能有一个类别。同一出口出现两个类别且没有
//!   `variant` 区分时，记为**冲突**，不计入清单，要求复测。这是把「不能用假设补测量」
//!   这条纪律写成代码，而不是写成文档。
//!
//! # 为什么变体要单独标
//!
//! 不少家用 CPE 提供「全锥 NAT / NAT 加速」开关，同一个真实设备可以配出两种行为。
//! 这是**同一台真实设备的不同配置**，证据价值真实但性质不同（真实用户不会去拨这个开关），
//! 所以 `variant` 非空的样本单独统计，永远不计入「独立出口覆盖」。

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use cr_core::Json;

use crate::{
    FilterBehavior, NatType, TraversalReport, S6_CLASS_COVERAGE_REQUIRED_FOR_PASS,
    S6_REACH_STANCE, S6_REQUIRED_MATRIX,
};

/// 普查记录格式版本。字段只增不改；改语义必须升版本。
pub const CENSUS_VERSION: u32 = 1;

/// 记录类型标识。存在的唯一目的：让读到这份文件的人不会把清单误读成覆盖。
pub const RECORD_KIND: &str = "class-inventory";

// ---------------------------------------------------------------------------
// 映射行为（EIM / APDM）—— 与过滤行为是两个正交的维度
// ---------------------------------------------------------------------------

/// 映射行为：同一本地端口对不同目的地址，映射端口是否保持一致。
///
/// 这个维度只能由「≥2 个目的地址的观测」得出；只有一个目的地址时必须 `Unknown`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingBehavior {
    /// Endpoint-Independent Mapping：映射与目的地址无关（锥形 NAT 的必要条件）。
    EndpointIndependent,
    /// Address-and-Port-Dependent Mapping：映射随目的变化（对称型）。
    AddressDependent,
    /// 观测不足以判定。
    Unknown,
}

impl MappingBehavior {
    pub fn label(&self) -> &'static str {
        match self {
            MappingBehavior::EndpointIndependent => "eim",
            MappingBehavior::AddressDependent => "apdm",
            MappingBehavior::Unknown => "unknown",
        }
    }

    /// 由分类结论反推映射行为。
    ///
    /// ⚠ 方向是「结论 → 维度」，不是「维度 → 结论」：`nat_type` 本身已由
    /// `classify_from_mappings` 从 ≥2 个目的地址的观测得出，这里只是把同一条观测
    /// 换个说法，不引入新假设。`Unknown` 一律映射为 `Unknown`。
    pub fn from_nat_type(t: NatType) -> Self {
        match t {
            NatType::Symmetric => MappingBehavior::AddressDependent,
            NatType::Unknown => MappingBehavior::Unknown,
            NatType::FullCone | NatType::RestrictedCone | NatType::PortRestrictedCone
            | NatType::Cgnat => MappingBehavior::EndpointIndependent,
        }
    }
}

// ---------------------------------------------------------------------------
// 时间（std-only，不引 chrono）
// ---------------------------------------------------------------------------

pub fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 把 Unix 毫秒格式化为 `YYYY-MM-DDTHH:MM:SS.mmmZ`（UTC）。
///
/// 自己算是因为仓库零第三方依赖。算法是 Howard Hinnant 的 `civil_from_days`
/// （公历 400 年周期），对 1970 前后都正确。
pub fn format_unix_ms_utc(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let millis = (ms % 1000) as u32;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        y, m, d, hh, mm, ss, millis
    )
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ---------------------------------------------------------------------------
// 双层 NAT 判定
// ---------------------------------------------------------------------------

/// 由「CPE 自报的 WAN 地址」与「STUN 观测到的映射地址」判是否双层 NAT。
///
/// 判据：两者**同时存在**且 IP 不同 → `Some(true)`。
///
/// 这条判据的价值在于它能区分「运营商级 CGNAT」与「只是本机在 CPE 后面」：
/// 只做单层 NAT 时，STUN 看到的映射对端就是 CPE 的 WAN 地址，两者应当一致；
/// 不一致说明中间还有一层我们看不见的 NAT（运营商 CGNAT 的典型形态）。
///
/// 任一值缺失返回 `None` —— 缺数据时不下结论，这是本仓库一贯的口径。
pub fn double_nat(mapped: Option<&str>, wan: Option<&str>) -> Option<bool> {
    let m = mapped.and_then(ip_of)?;
    let w = wan.and_then(ip_of)?;
    Some(m != w)
}

fn ip_of(addr: &str) -> Option<&str> {
    let a = addr.trim();
    if a.is_empty() {
        return None;
    }
    Some(match a.rsplit_once(':') {
        Some((ip, _)) => ip,
        None => a,
    })
}

// ---------------------------------------------------------------------------
// 一条样本
// ---------------------------------------------------------------------------

/// 样本的元信息（由命令行 `--tag k=v` 提供）。
///
/// `egress` 必填且受限字符集：这些字符串会进 JSON、进报告、进文件名，
/// 含控制字节会让**读的一方**永久失败而写的一方看起来一切正常
/// （W3.9 的 `--stats-file` 就栽在这上面）。
#[derive(Debug, Clone, Default)]
pub struct SampleMeta {
    pub host: String,
    pub egress: String,
    pub operator: String,
    pub device: Option<String>,
    pub variant: Option<String>,
    pub notes: Option<String>,
}

impl SampleMeta {
    /// 校验并补齐默认值。`egress` 缺失或含非法字符即报错，不静默改名。
    pub fn validate(&self) -> Result<SampleMeta, String> {
        let egress = self.egress.trim();
        if egress.is_empty() {
            return Err("缺少 --tag egress=<名字>（用来标识这是哪个出口，例如 home-cmcc）".into());
        }
        if let Some(bad) = egress
            .chars()
            .find(|c| !(c.is_ascii_alphanumeric() || "-_.:@".contains(*c)))
        {
            return Err(format!(
                "egress 含非法字符 {:?}：只允许字母数字与 - _ . : @（该值会进报告与文件名）",
                bad
            ));
        }
        let mut m = self.clone();
        m.egress = egress.to_string();
        if m.host.trim().is_empty() {
            m.host = default_host();
        }
        if m.operator.trim().is_empty() {
            m.operator = "unknown".into();
        }
        for (field, v) in [
            ("host", &mut m.host),
            ("operator", &mut m.operator),
        ] {
            if let Some(bad) = v.chars().find(|c| c.is_control()) {
                return Err(format!("{} 含控制字符 {:?}", field, bad));
            }
        }
        if let Some(bad) = m.variant.as_deref().and_then(|s| s.chars().find(|c| c.is_control())) {
            return Err(format!("variant 含控制字符 {:?}", bad));
        }
        Ok(m)
    }
}

/// 尽力取一个主机标识：优先显式 `--tag host=`，否则用环境变量，最后退回 `unknown`。
fn default_host() -> String {
    for k in ["COMPUTERNAME", "HOSTNAME"] {
        if let Ok(v) = std::env::var(k) {
            if !v.trim().is_empty() {
                return v.trim().to_string();
            }
        }
    }
    "unknown".to_string()
}

/// 一条归档样本。字段顺序即落盘顺序，读回时按白名单严格校验。
#[derive(Debug, Clone, PartialEq)]
pub struct CensusRecord {
    pub sample_id: String,
    pub measured_at: String,
    pub measured_unix_ms: u64,
    pub host: String,
    pub egress: String,
    pub operator: String,
    pub device: Option<String>,
    pub variant: Option<String>,
    pub nat_class: String,
    pub mapping_behavior: String,
    pub filtering_behavior: String,
    pub mapped_address: Option<String>,
    pub wan_address: Option<String>,
    pub double_nat: Option<bool>,
    pub cgnat_suspected: bool,
    pub observations: usize,
    pub distinct_mappings: usize,
    pub stun_servers: String,
    pub notes: Option<String>,
}

impl CensusRecord {
    /// 由一次真实探测结果构造样本。`now_ms` 由调用方传入，便于测试固定时间。
    pub fn from_report(
        report: &TraversalReport,
        meta: &SampleMeta,
        now_ms: u64,
    ) -> Result<CensusRecord, String> {
        let meta = meta.validate()?;
        let wan = report
            .upnp_external_ip
            .clone()
            .or_else(|| report.natpmp_external_ip.clone());
        let distinct = distinct_mappings(&report.observations);
        let servers = distinct_servers(&report.observations);
        let sample_id = derive_sample_id(
            &meta.egress,
            now_ms,
            report.nat_type.label(),
            report.mapped_address.as_deref(),
        );
        Ok(CensusRecord {
            sample_id,
            measured_at: format_unix_ms_utc(now_ms),
            measured_unix_ms: now_ms,
            host: meta.host,
            egress: meta.egress,
            operator: meta.operator,
            device: meta.device,
            variant: meta.variant,
            nat_class: report.nat_type.label().to_string(),
            mapping_behavior: MappingBehavior::from_nat_type(report.nat_type)
                .label()
                .to_string(),
            filtering_behavior: filter_label(report.filter_behavior).to_string(),
            mapped_address: report.mapped_address.clone(),
            double_nat: double_nat(report.mapped_address.as_deref(), wan.as_deref()),
            wan_address: wan,
            cgnat_suspected: report.cgnat_suspected,
            observations: report.observations.len(),
            distinct_mappings: distinct,
            stun_servers: servers,
            notes: meta.notes,
        })
    }

    /// 一行 JSON（JSONL）。键顺序固定，便于 diff。
    pub fn to_json(&self) -> Json {
        Json::obj(vec![
            ("census_version", Json::n(CENSUS_VERSION as f64)),
            ("kind", Json::s(RECORD_KIND)),
            ("sample_id", Json::s(&self.sample_id)),
            ("measured_at", Json::s(&self.measured_at)),
            ("measured_unix_ms", Json::n(self.measured_unix_ms as f64)),
            ("host", Json::s(&self.host)),
            ("egress", Json::s(&self.egress)),
            ("operator", Json::s(&self.operator)),
            ("device", opt_str(&self.device)),
            ("variant", opt_str(&self.variant)),
            ("nat_class", Json::s(&self.nat_class)),
            ("mapping_behavior", Json::s(&self.mapping_behavior)),
            ("filtering_behavior", Json::s(&self.filtering_behavior)),
            ("mapped_address", opt_str(&self.mapped_address)),
            ("wan_address", opt_str(&self.wan_address)),
            ("double_nat", opt_bool(self.double_nat)),
            ("cgnat_suspected", Json::b(self.cgnat_suspected)),
            ("observations", Json::n(self.observations as f64)),
            ("distinct_mappings", Json::n(self.distinct_mappings as f64)),
            ("stun_servers", Json::s(&self.stun_servers)),
            ("notes", opt_str(&self.notes)),
        ])
    }

    pub fn to_line(&self) -> String {
        self.to_json().dump()
    }

    /// 严格读回：**未知键直接报错**。放宽会让字段名变了却照样“读得通”，
    /// 那是把格式错误静默成数据缺失。
    pub fn from_json(j: &Json) -> Result<CensusRecord, String> {
        let known = [
            "census_version", "kind", "sample_id", "measured_at", "measured_unix_ms",
            "host", "egress", "operator", "device", "variant", "nat_class",
            "mapping_behavior", "filtering_behavior", "mapped_address", "wan_address",
            "double_nat", "cgnat_suspected", "observations", "distinct_mappings",
            "stun_servers", "notes",
        ];
        let obj = j.as_obj().ok_or_else(|| "记录不是 JSON 对象".to_string())?;
        for (k, _) in obj {
            if !known.contains(&k.as_str()) {
                return Err(format!(
                    "未知字段 {:?}（格式版本不符，或手改过。已知字段：{}）",
                    k,
                    known.join(", ")
                ));
            }
        }
        let ver = j.get("census_version").and_then(|v| v.as_i64()).unwrap_or(-1);
        if ver != CENSUS_VERSION as i64 {
            return Err(format!(
                "census_version={} 与本文支持的 {} 不符",
                ver, CENSUS_VERSION
            ));
        }
        if let Some(k) = j.get("kind").and_then(|v| v.as_str()) {
            if k != RECORD_KIND {
                return Err(format!("kind={:?} 不是类别清单样本", k));
            }
        }
        Ok(CensusRecord {
            sample_id: req_str(j, "sample_id")?,
            measured_at: req_str(j, "measured_at")?,
            measured_unix_ms: req_num(j, "measured_unix_ms")? as u64,
            host: req_str(j, "host")?,
            egress: req_str(j, "egress")?,
            operator: req_str(j, "operator")?,
            device: j.get("device").and_then(|v| v.as_str()).map(str::to_string),
            variant: j.get("variant").and_then(|v| v.as_str()).map(str::to_string),
            nat_class: req_str(j, "nat_class")?,
            mapping_behavior: req_str(j, "mapping_behavior")?,
            filtering_behavior: req_str(j, "filtering_behavior")?,
            mapped_address: j.get("mapped_address").and_then(|v| v.as_str()).map(str::to_string),
            wan_address: j.get("wan_address").and_then(|v| v.as_str()).map(str::to_string),
            double_nat: j.get("double_nat").and_then(|v| v.as_bool()),
            cgnat_suspected: j
                .get("cgnat_suspected")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            observations: req_num(j, "observations")? as usize,
            distinct_mappings: req_num(j, "distinct_mappings")? as usize,
            stun_servers: j
                .get("stun_servers")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            notes: j.get("notes").and_then(|v| v.as_str()).map(str::to_string),
        })
    }
}

fn req_str(j: &Json, k: &str) -> Result<String, String> {
    j.get(k)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("缺少字符串字段 {:?}", k))
}

fn req_num(j: &Json, k: &str) -> Result<f64, String> {
    j.get(k)
        .and_then(|v| v.as_i64())
        .map(|v| v as f64)
        .ok_or_else(|| format!("缺少数字字段 {:?}", k))
}

fn opt_str(v: &Option<String>) -> Json {
    match v {
        Some(s) => Json::s(s.clone()),
        None => Json::Null,
    }
}

fn opt_bool(v: Option<bool>) -> Json {
    match v {
        Some(b) => Json::b(b),
        None => Json::Null,
    }
}

fn filter_label(f: FilterBehavior) -> &'static str {
    f.label()
}

fn distinct_mappings(obs: &[(String, String)]) -> usize {
    let mut s: Vec<&str> = obs.iter().map(|(_, m)| m.as_str()).collect();
    s.sort_unstable();
    s.dedup();
    s.len()
}

fn distinct_servers(obs: &[(String, String)]) -> String {
    let mut s: Vec<&str> = obs.iter().map(|(srv, _)| srv.as_str()).collect();
    s.sort_unstable();
    s.dedup();
    s.join(",")
}

/// 样本 id：对「出口 + 时间 + 类别 + 映射」求 SHA-256 取前 16 位十六进制。
///
/// 用途是**去重**：同一份输出被贴两次不该算两个样本，否则 30 次复测会虚增 n。
fn derive_sample_id(egress: &str, now_ms: u64, nat_class: &str, mapped: Option<&str>) -> String {
    let mut s = String::new();
    s.push_str(egress);
    s.push('|');
    s.push_str(&now_ms.to_string());
    s.push('|');
    s.push_str(nat_class);
    s.push('|');
    s.push_str(mapped.unwrap_or(""));
    cr_hash::sha256_hex(s.as_bytes())[..16].to_string()
}

// ---------------------------------------------------------------------------
// 归档读写
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendOutcome {
    Appended,
    Duplicate,
}

/// 追加一条样本。同 `sample_id` 已存在时不再写（返回 `Duplicate`）。
pub fn append(path: &Path, rec: &CensusRecord) -> Result<AppendOutcome, String> {
    if path.exists() {
        let existing = load(path)?;
        if existing.iter().any(|r| r.sample_id == rec.sample_id) {
            return Ok(AppendOutcome::Duplicate);
        }
    }
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() && !dir.exists() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("创建目录 {} 失败: {}", dir.display(), e))?;
        }
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("打开 {} 失败: {}", path.display(), e))?;
    writeln!(f, "{}", rec.to_line()).map_err(|e| format!("写入 {} 失败: {}", path.display(), e))?;
    f.flush().ok();
    Ok(AppendOutcome::Appended)
}

/// 读回全部样本。空行跳过；任何一行解析失败都带行号报错，**不跳过**。
pub fn load(path: &Path) -> Result<Vec<CensusRecord>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("读取 {} 失败: {}", path.display(), e))?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let j = Json::parse(t).map_err(|e| format!("{}:{}: {}", path.display(), i + 1, e))?;
        let r = CensusRecord::from_json(&j)
            .map_err(|e| format!("{}:{}: {}", path.display(), i + 1, e))?;
        out.push(r);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 单个出口的画像。
#[derive(Debug, Clone)]
pub struct EgressProfile {
    pub egress: String,
    pub operator: String,
    pub device: Option<String>,
    pub samples: usize,
    /// 该出口（不含变体样本）出现的类别集合。>1 即冲突。
    pub classes: Vec<String>,
    /// 该出口声明过的配置变体。
    pub variants: Vec<String>,
    /// 冲突说明（空则无冲突）。
    pub conflicts: Vec<String>,
    /// 「不标变体的样本」是否存在类别冲突。为真时该出口**整条不进清单**：
    /// 冲突意味着我们不知道哪个类别才对，任选一个都是编。
    pub unresolved: bool,
}

/// 汇总结果。注意 `matrix_coverage` / `r04_judgeable` / `s6_status` 是**常量**，
/// 不是计算结果——普查在原理上不产出这些证据。
#[derive(Debug, Clone)]
pub struct CensusSummary {
    pub samples: usize,
    pub unknown_class_samples: usize,
    pub egresses: Vec<EgressProfile>,
    /// 类别 → 独立出口列表（`variant` 为空的样本才算）。
    pub class_inventory: BTreeMap<String, Vec<String>>,
    /// 只能靠配置变体复现的类别（有样本，但都带 `variant`）。
    pub variant_only_classes: Vec<String>,
    /// 清单中**尚无独立出口**覆盖的类别（描述性；ADR-016 后不构成门禁失败）。
    pub missing_classes: Vec<String>,
    pub conflicts: Vec<String>,
    pub warnings: Vec<String>,
    pub matrix_coverage: &'static str,
    pub r04_judgeable: bool,
    pub s6_status: &'static str,
    pub g2_claim: bool,
}

impl CensusSummary {
    /// 独立出口覆盖的类别数（描述性清单长度，ADR-016 后不是达分数）。
    pub fn independent_classes(&self) -> usize {
        self.class_inventory.len()
    }

    pub fn to_json(&self) -> Json {
        let egresses = self
            .egresses
            .iter()
            .map(|e| {
                Json::obj(vec![
                    ("egress", Json::s(&e.egress)),
                    ("operator", Json::s(&e.operator)),
                    ("device", opt_str(&e.device)),
                    ("samples", Json::n(e.samples as f64)),
                    ("classes", Json::arr(e.classes.iter().map(|s| Json::s(s.as_str())).collect())),
                    ("variants", Json::arr(e.variants.iter().map(|s| Json::s(s.as_str())).collect())),
                    ("conflicts", Json::arr(e.conflicts.iter().map(|s| Json::s(s.as_str())).collect())),
                    ("unresolved", Json::b(e.unresolved)),
                ])
            })
            .collect();
        let inv: Vec<(String, Json)> = self
            .class_inventory
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    Json::arr(v.iter().map(|s| Json::s(s.as_str())).collect()),
                )
            })
            .collect();
        Json::obj(vec![
            ("census_version", Json::n(CENSUS_VERSION as f64)),
            ("kind", Json::s(RECORD_KIND)),
            ("samples", Json::n(self.samples as f64)),
            ("unknown_class_samples", Json::n(self.unknown_class_samples as f64)),
            ("egresses", Json::arr(egresses)),
            ("class_inventory", Json::Obj(inv)),
            ("independent_classes", Json::n(self.independent_classes() as f64)),
            ("class_inventory_size", Json::n(S6_REQUIRED_MATRIX.len() as f64)),
            // 兼容旧报告读取方：字段名保留，但 ADR-016 后数值只是清单长度，
            // 是否构成 pass 前提一律看 class_coverage_required_for_pass。
            ("required_classes", Json::n(S6_REQUIRED_MATRIX.len() as f64)),
            ("s6_reach_stance", Json::s(S6_REACH_STANCE)),
            ("class_coverage_required_for_pass", Json::b(S6_CLASS_COVERAGE_REQUIRED_FOR_PASS)),
            (
                "variant_only_classes",
                Json::arr(self.variant_only_classes.iter().map(|s| Json::s(s.as_str())).collect()),
            ),
            (
                "missing_classes",
                Json::arr(self.missing_classes.iter().map(|s| Json::s(s.as_str())).collect()),
            ),
            (
                "conflicts",
                Json::arr(self.conflicts.iter().map(|s| Json::s(s.as_str())).collect()),
            ),
            (
                "warnings",
                Json::arr(self.warnings.iter().map(|s| Json::s(s.as_str())).collect()),
            ),
            ("matrix_coverage", Json::s(self.matrix_coverage)),
            ("r04_judgeable", Json::b(self.r04_judgeable)),
            ("s6_status", Json::s(self.s6_status)),
            ("g2_claim", Json::b(self.g2_claim)),
        ])
    }
}

/// 从样本集合汇总类别清单。纯函数，不读盘。
pub fn summarize(records: &[CensusRecord]) -> CensusSummary {
    let mut by_egress: BTreeMap<String, Vec<&CensusRecord>> = BTreeMap::new();
    for r in records {
        by_egress.entry(r.egress.clone()).or_default().push(r);
    }

    let mut egresses = Vec::new();
    for (egress, rs) in &by_egress {
        let mut classes: Vec<String> = Vec::new();
        let mut variants: Vec<String> = Vec::new();
        // 按变体分组，逐组检查「一个出口一个类别」不变量。
        let mut per_variant: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for r in rs {
            if let Some(v) = &r.variant {
                if !variants.contains(v) {
                    variants.push(v.clone());
                }
            }
            if r.nat_class == NatType::Unknown.label() {
                continue; // unknown 不参与清单，也不制造冲突
            }
            per_variant
                .entry(r.variant.clone().unwrap_or_default())
                .or_default()
                .push(r.nat_class.clone());
            if !classes.contains(&r.nat_class) {
                classes.push(r.nat_class.clone());
            }
        }
        let mut conflicts = Vec::new();
        let mut unresolved = false;
        for (variant, cs) in &per_variant {
            let mut uniq = cs.clone();
            uniq.sort_unstable();
            uniq.dedup();
            if uniq.len() > 1 {
                let label = if variant.is_empty() {
                    "同一出口（未标 variant）".to_string()
                } else {
                    format!("同一出口 variant={}", variant)
                };
                if variant.is_empty() {
                    unresolved = true;
                }
                conflicts.push(format!(
                    "{}出现 {} 个类别（{}）：一个出口只能有一个类别，需复测或补 variant 标注",
                    label,
                    uniq.len(),
                    uniq.join(" / ")
                ));
            }
        }
        let mut classes_sorted = classes;
        classes_sorted.sort();
        egresses.push(EgressProfile {
            egress: egress.clone(),
            operator: rs.first().map(|r| r.operator.clone()).unwrap_or_default(),
            device: rs.iter().find_map(|r| r.device.clone()),
            samples: rs.len(),
            classes: classes_sorted,
            variants,
            conflicts: conflicts.clone(),
            unresolved,
        });
    }

    // 独立出口清单：只看 variant 为空的样本。
    // 有冲突的出口**整条剔除**——两个类别里哪个才对是未知的，任选一个都是编。
    let conflicted: Vec<&str> = egresses
        .iter()
        .filter(|e| e.unresolved)
        .map(|e| e.egress.as_str())
        .collect();
    let mut class_inventory: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut variant_classes: Vec<String> = Vec::new();
    for r in records {
        if r.nat_class == NatType::Unknown.label() {
            continue;
        }
        if conflicted.contains(&r.egress.as_str()) {
            continue;
        }
        match &r.variant {
            None => {
                let e = class_inventory.entry(r.nat_class.clone()).or_default();
                if !e.contains(&r.egress) {
                    e.push(r.egress.clone());
                }
            }
            Some(_) => {
                if !variant_classes.contains(&r.nat_class) {
                    variant_classes.push(r.nat_class.clone());
                }
            }
        }
    }
    for v in class_inventory.values_mut() {
        v.sort();
    }
    let variant_only_classes: Vec<String> = {
        let mut v: Vec<String> = variant_classes
            .into_iter()
            .filter(|c| !class_inventory.contains_key(c))
            .collect();
        v.sort();
        v
    };

    let missing_classes: Vec<String> = S6_REQUIRED_MATRIX
        .iter()
        .map(|c| c.label().to_string())
        .filter(|c| !class_inventory.contains_key(c))
        .collect();

    let unknown_class_samples = records
        .iter()
        .filter(|r| r.nat_class == NatType::Unknown.label())
        .count();

    let all_conflicts: Vec<String> = egresses
        .iter()
        .flat_map(|e| e.conflicts.iter().cloned())
        .collect();

    let mut warnings = Vec::new();
    if egresses.len() == 1 {
        warnings.push(
            "只有 1 个出口样本：任何「跨出口」结论都不成立，且无法区分「该类很少见」与「我没测到」"
                .to_string(),
        );
    }
    let ops: Vec<&str> = {
        let mut v: Vec<&str> = egresses.iter().map(|e| e.operator.as_str()).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    if ops.len() == 1 && !ops[0].is_empty() && ops[0] != "unknown" {
        warnings.push(format!(
            "全部样本来自同一运营商（{}）：类别分布对「真实家用 NAT 总体」没有代表性。\
             样本本身仍可核查，但不能用来宣称总体可达率。",
            ops[0]
        ));
    }
    if unknown_class_samples > 0 {
        warnings.push(format!(
            "{} 条样本分类为 unknown（STUN 不可达或目的地址不足 2 个），不计入清单",
            unknown_class_samples
        ));
    }
    if !all_conflicts.is_empty() {
        warnings.push("存在同出口多类别冲突，冲突项已从清单剔除".to_string());
    }
    if !variant_only_classes.is_empty() {
        warnings.push(format!(
            "{} 只能靠配置变体复现：可核查，但真实用户不会拨这个开关，不计入独立出口覆盖",
            variant_only_classes.join(", ")
        ));
    }

    CensusSummary {
        samples: records.len(),
        unknown_class_samples,
        egresses,
        class_inventory,
        variant_only_classes,
        missing_classes,
        conflicts: all_conflicts,
        warnings,
        // 以下四个字段是常量：普查不产出矩阵覆盖，也不产出可达率。
        matrix_coverage: "none",
        r04_judgeable: false,
        s6_status: "not_run",
        g2_claim: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Strategy;

    fn report(nat: NatType, mapped: Option<&str>, obs: Vec<(&str, &str)>) -> TraversalReport {
        TraversalReport {
            nat_type: nat,
            public_udp_egress_ok: mapped.is_some(),
            upnp_available: false,
            upnp_external_ip: None,
            natpmp_available: false,
            natpmp_external_ip: None,
            pcp_available: false,
            cgnat_suspected: false,
            mapped_address: mapped.map(str::to_string),
            recommended_strategy: Strategy::Relay,
            filter_behavior: FilterBehavior::Unknown,
            observations: obs
                .into_iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
        }
    }

    fn meta(egress: &str) -> SampleMeta {
        SampleMeta {
            host: "host-a".into(),
            egress: egress.into(),
            operator: "cmcc".into(),
            device: None,
            variant: None,
            notes: None,
        }
    }

    #[test]
    fn utc_formatter_matches_known_epochs() {
        assert_eq!(format_unix_ms_utc(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(format_unix_ms_utc(1_700_000_000_000), "2023-11-14T22:13:20.000Z");
        assert_eq!(format_unix_ms_utc(2_000_000_000_123), "2033-05-18T03:33:20.123Z");
    }

    #[test]
    fn egress_id_rejects_control_bytes_and_empty() {
        let mut m = meta("home");
        m.egress = "".into();
        assert!(m.validate().is_err());
        m.egress = "home\ncmcc".into();
        assert!(m.validate().is_err(), "控制字符必须拒绝，否则报告会读不出来");
        m.egress = "relay-222".into();
        assert!(m.validate().is_ok());
    }

    #[test]
    fn record_roundtrips_through_jsonl() {
        let r = CensusRecord::from_report(
            &report(NatType::Symmetric, Some("112.5.32.34:2051"), vec![
                ("stun.a:3478", "112.5.32.34:2051"),
                ("stun.b:3478", "112.5.32.34:2055"),
            ]),
            &meta("home-cmcc"),
            1_700_000_000_000,
        )
        .unwrap();
        let line = r.to_line();
        assert!(!line.contains('\n'));
        let back = CensusRecord::from_json(&Json::parse(&line).unwrap()).unwrap();
        assert_eq!(r, back);
        assert_eq!(back.mapping_behavior, "apdm");
        assert_eq!(back.distinct_mappings, 2);
        assert_eq!(back.observations, 2);
        assert_eq!(back.stun_servers, "stun.a:3478,stun.b:3478");
    }

    #[test]
    fn unknown_field_is_rejected_not_ignored() {
        let mut v = Json::parse(
            &CensusRecord::from_report(
                &report(NatType::FullCone, Some("1.2.3.4:5000"), vec![]),
                &meta("home"),
                1_700_000_000_000,
            )
            .unwrap()
            .to_line(),
        )
        .unwrap();
        if let Json::Obj(p) = &mut v {
            p.push(("surprise".into(), Json::n(1.0)));
        }
        let e = CensusRecord::from_json(&v).unwrap_err();
        assert!(e.contains("未知字段"), "{}", e);
    }

    #[test]
    fn double_nat_detects_extra_layer() {
        // CPE 自报 WAN 与 STUN 映射一致 → 单层
        assert_eq!(double_nat(Some("112.5.32.34:2051"), Some("112.5.32.34")), Some(false));
        // CPE 自报 100.64/10，STUN 看到公网 → 中间还有一层（运营商 CGNAT）
        assert_eq!(double_nat(Some("112.5.32.34:2051"), Some("100.64.7.9")), Some(true));
        // 缺数据 → 不下结论
        assert_eq!(double_nat(Some("112.5.32.34:2051"), None), None);
        assert_eq!(double_nat(None, Some("112.5.32.34")), None);
    }

    #[test]
    fn one_egress_two_classes_is_a_conflict_not_coverage() {
        let a = CensusRecord::from_report(
            &report(NatType::Symmetric, Some("1.1.1.1:1000"), vec![]),
            &meta("home"),
            1_700_000_000_000,
        )
        .unwrap();
        let mut b = a.clone();
        b.sample_id = "x".into();
        b.measured_unix_ms += 1000;
        b.nat_class = NatType::FullCone.label().into();
        let s = summarize(&[a, b]);
        assert_eq!(s.egresses.len(), 1);
        assert_eq!(s.egresses[0].classes.len(), 2);
        assert!(!s.egresses[0].conflicts.is_empty());
        assert!(!s.conflicts.is_empty());
        // 冲突样本不进入清单：一个出口不能同时占两格
        assert!(s.class_inventory.is_empty(), "{:?}", s.class_inventory);
    }

    #[test]
    fn variant_classes_never_count_as_independent_coverage() {
        let mut m = meta("home");
        m.variant = Some("fullcone-toggle-on".into());
        let rec = CensusRecord::from_report(
            &report(NatType::FullCone, Some("1.1.1.1:1000"), vec![]),
            &m,
            1_700_000_000_000,
        )
        .unwrap();
        let s = summarize(&[rec]);
        assert!(s.class_inventory.is_empty());
        assert_eq!(s.variant_only_classes, vec!["full-cone".to_string()]);
        assert!(s.missing_classes.contains(&"full-cone".to_string()));
    }

    #[test]
    fn summary_never_claims_matrix_or_r04() {
        let rec = CensusRecord::from_report(
            &report(NatType::RestrictedCone, Some("1.1.1.1:1000"), vec![]),
            &meta("home"),
            1_700_000_000_000,
        )
        .unwrap();
        let s = summarize(&[rec]);
        assert_eq!(s.matrix_coverage, "none");
        assert!(!s.r04_judgeable);
        assert_eq!(s.s6_status, "not_run");
        assert!(!s.g2_claim);
        assert_eq!(s.independent_classes(), 1);
        assert_eq!(s.missing_classes.len(), S6_REQUIRED_MATRIX.len() - 1);
    }

    #[test]
    fn summary_states_that_class_coverage_is_not_a_pass_condition() {
        // ADR-016：清单必须自带「我不要求类别覆盖」的机器可读声明，
        // 免得看报告的人把 class_inventory_size 当成达标线。
        let s = summarize(&[]);
        let j = s.to_json();
        assert_eq!(j.get("s6_reach_stance").and_then(Json::as_str), Some(S6_REACH_STANCE));
        assert_eq!(j.get("class_coverage_required_for_pass").and_then(Json::as_bool), Some(false));
        assert_eq!(j.get("class_inventory_size").and_then(Json::as_i64), Some(5));
        assert_eq!(s.matrix_coverage, "none");
        assert_eq!(s.s6_status, "not_run");
        assert!(!s.g2_claim);
    }

    #[test]
    fn unknown_samples_are_excluded_from_inventory() {
        let rec = CensusRecord::from_report(
            &report(NatType::Unknown, None, vec![]),
            &meta("home"),
            1_700_000_000_000,
        )
        .unwrap();
        let s = summarize(&[rec]);
        assert_eq!(s.unknown_class_samples, 1);
        assert!(s.class_inventory.is_empty());
        assert!(s.egresses[0].conflicts.is_empty(), "unknown 不应制造冲突");
    }

    #[test]
    fn duplicate_sample_id_is_not_counted_twice() {
        let dir = std::env::temp_dir().join(format!("cr-census-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("census.jsonl");
        let _ = std::fs::remove_file(&path);
        let rec = CensusRecord::from_report(
            &report(NatType::Symmetric, Some("1.1.1.1:1000"), vec![]),
            &meta("home"),
            1_700_000_000_000,
        )
        .unwrap();
        assert_eq!(append(&path, &rec).unwrap(), AppendOutcome::Appended);
        assert_eq!(append(&path, &rec).unwrap(), AppendOutcome::Duplicate);
        assert_eq!(load(&path).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_line_fails_with_line_number() {
        let dir = std::env::temp_dir().join(format!("cr-census-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("census.jsonl");
        std::fs::write(&path, "{\"not\":\"a census record\"}\n").unwrap();
        let e = load(&path).unwrap_err();
        assert!(e.contains(":1:"), "错误必须带行号: {}", e);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
