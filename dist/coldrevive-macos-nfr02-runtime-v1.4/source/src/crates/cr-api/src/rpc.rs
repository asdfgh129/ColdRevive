//! JSON-RPC 访问控制助手（FR-20）。
//!
//! 这里只做**纯判定**：守护进程把「是否配置了 secret、请求方法、params、可选的
//! HTTP 头里的 token」交进来，本模块回答「这条 mutating 调用是否放行」。
//! 不读取环境变量、不读配置文件、不输出 secret 本身——secret 永远留在调用方，
//! 因此 `/health` 不可能因为本模块的返回值而泄露它。

use cr_core::Json;

/// 会改变守护进程状态的 JSON-RPC 方法（aria2 兼容子集 + 本项目扩展）。
///
/// 判定依据是**方法语义**，不是字符串包含关系：`tell*` / `get*` / `coldrevive.ed2kStatus`
/// 都是只读，不需要 token；`addUri` / `remove` / `pause` 等会写状态，必须校验。
pub const MUTATING_METHODS: &[&str] = &[
    "addUri", "addTorrent", "addMetalink", "remove", "forceRemove",
    "pause", "pauseAll", "forcePause", "forcePauseAll", "resume", "resumeAll",
    "removeDownloadResult", "retryDownload", "purgeDownloadResult",
    "changeOption", "changeGlobalOption", "changeBtOption", "shutdown",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    /// 配置了 secret，但 mutating 调用没有给出正确的 token。
    Unauthorized,
}

/// 方法是否需要 token。
pub fn is_mutating_method(method: &str) -> bool {
    let bare = method.strip_prefix("aria2.").unwrap_or(method);
    MUTATING_METHODS.contains(&bare)
}

/// 从 `params` 的前两个位置取 aria2 风格的 `token:<secret>`。
///
/// 兼容两种历史调用习惯：`["token:abc", [uri]]` 与 `[[uri], "token:abc"]`。
/// 不扫描更靠后的参数，避免把普通 URI 误当成凭据。
pub fn extract_token(params: &[Json]) -> Option<&str> {
    params.iter().take(2).filter_map(Json::as_str)
        .find_map(|s| s.strip_prefix("token:"))
}

/// 从 HTTP 头取值里取 token：支持 `Bearer x`、`token:x` 与裸 token。
pub fn token_from_header(value: Option<&str>) -> Option<&str> {
    let v = value?.trim();
    if let Some(rest) = v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")) {
        return Some(rest.trim());
    }
    if let Some(rest) = v.strip_prefix("token:") {
        return Some(rest.trim());
    }
    Some(v)
}

/// 校验一条 JSON-RPC 调用。
///
/// - 没有配置 secret（或配置为空串）→ 一律放行，保证既有 T0/T1 行为逐字节不变；
/// - 只读方法 → 放行，方便 UI 在未填 secret 时先展示状态；
/// - mutating 方法 → params 或 HTTP 头里必须有一个与配置完全相等的 token。
pub fn authorize(
    configured: Option<&str>,
    params: &[Json],
    header_token: Option<&str>,
    method: &str,
) -> Result<(), AuthError> {
    let Some(secret) = configured.filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    if !is_mutating_method(method) {
        return Ok(());
    }
    let supplied = extract_token(params).into_iter()
        .chain(token_from_header(header_token));
    if supplied.into_iter().any(|t| t == secret) {
        Ok(())
    } else {
        Err(AuthError::Unauthorized)
    }
}

/// `/health` 里**只能**出现这个：secret 是否启用，绝不出现 secret 本身。
///
/// 函数签名只收 `bool`，从类型上就不可能把 secret 带进输出。
pub fn health_api_json(
    rpc_secret_configured: bool,
    web_ui_enabled: bool,
    ws_enabled: bool,
) -> Json {
    Json::obj(vec![
        ("rpc_secret", Json::obj(vec![
            ("enabled", Json::b(rpc_secret_configured)),
            ("note", Json::s("token 值不会出现在 /health 或任何响应中")),
        ])),
        ("web_ui", Json::obj(vec![("enabled", Json::b(web_ui_enabled))])),
        ("websocket", Json::obj(vec![("enabled", Json::b(ws_enabled))])),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_secret_keeps_legacy_behavior() {
        assert!(authorize(None, &[], None, "aria2.addUri").is_ok());
        assert!(authorize(Some(""), &[], None, "aria2.addUri").is_ok());
    }

    #[test]
    fn mutating_call_requires_matching_token() {
        let secret = "s3cr3t";
        assert_eq!(
            authorize(Some(secret), &[], None, "aria2.addUri"),
            Err(AuthError::Unauthorized)
        );
        let good = vec![Json::s("token:s3cr3t"), Json::arr(vec![Json::s("magnet:?")])];
        assert!(authorize(Some(secret), &good, None, "aria2.addUri").is_ok());
        let bad = vec![Json::s("token:wrong"), Json::arr(vec![Json::s("magnet:?")])];
        assert_eq!(
            authorize(Some(secret), &bad, None, "aria2.addUri"),
            Err(AuthError::Unauthorized)
        );
    }

    #[test]
    fn header_token_works_and_read_methods_are_free() {
        let p: Vec<Json> = vec![Json::arr(vec![Json::s("magnet:?")])];
        assert!(authorize(Some("k"), &p, Some("Bearer k"), "aria2.addUri").is_ok());
        assert!(authorize(Some("k"), &p, Some("token:k"), "aria2.addUri").is_ok());
        assert!(authorize(Some("k"), &[], None, "aria2.tellStatus").is_ok());
        assert!(authorize(Some("k"), &[], None, "coldrevive.ed2kStatus").is_ok());
    }

    #[test]
    fn classification_covers_mutating_and_readonly() {
        assert!(is_mutating_method("aria2.addUri"));
        assert!(is_mutating_method("aria2.remove"));
        assert!(!is_mutating_method("aria2.tellStatus"));
        assert!(!is_mutating_method("aria2.getGlobalStat"));
        assert!(!is_mutating_method("system.listMethods"));
    }

    #[test]
    fn health_exposes_only_enabled_flag() {
        let j = health_api_json(true, false, true);
        let dump = j.dump();
        assert!(dump.contains("\"enabled\":true"));
        assert!(!dump.to_ascii_lowercase().contains("s3cr3t"));
        assert_eq!(
            j.get("rpc_secret").and_then(|x| x.get("enabled")).and_then(Json::as_bool),
            Some(true)
        );
    }
}
