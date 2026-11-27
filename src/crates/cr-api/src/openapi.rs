//! FR-22：描述守护进程**现有** HTTP 面与 JSON-RPC 子集的 OpenAPI 文档。
//!
//! 这是描述符，不是新接口：每个 path 都对应 `coldrevived` 里真实存在的分支。
//! opt-in 的 `/`（Web UI）与 `/ws` 也列出来，但明确标记默认关闭。

use cr_core::Json;

fn op(summary: &str, tag: &str) -> Json {
    Json::obj(vec![
        ("summary", Json::s(summary)),
        ("tags", Json::arr(vec![Json::s(tag)])),
        ("responses", Json::obj(vec![
            ("200", Json::obj(vec![("description", Json::s("OK"))])),
        ])),
    ])
}

fn path(ops: Vec<(&'static str, Json)>) -> Json { Json::obj(ops) }

/// 构造完整文档。返回 `Json` 而不是裸字符串，便于调用方直接 `dump()` 或测试解析。
pub fn document_json() -> Json {
    let rpc_methods: Vec<Json> = vec![
        ("aria2.addUri", true, "登记/下载一个 .torrent（mutating）"),
        ("aria2.pause", true, "暂停一个已接入引擎的任务"),
        ("aria2.pauseAll", true, "暂停全部已接入引擎的任务"),
        ("aria2.resume", true, "恢复一个已暂停的任务"),
        ("aria2.resumeAll", true, "恢复全部已暂停的任务"),
        ("aria2.remove", true, "停止并移除任务（保留文件）"),
        ("aria2.forceRemove", true, "强制停止并移除任务（保留文件）"),
        ("aria2.removeDownloadResult", true, "停止任务并删除已下载文件"),
        ("aria2.tellStatus", false, "按 gid 查真实任务状态"),
        ("aria2.tellActive", false, "活动任务列表"),
        ("aria2.tellWaiting", false, "等待任务列表"),
        ("aria2.tellStopped", false, "结束任务列表"),
        ("aria2.getGlobalStat", false, "全局计数与速率"),
        ("aria2.getVersion", false, "版本与特性"),
        ("coldrevive.ed2kStatus", false, "eD2k 任务真实状态"),
    ].into_iter().map(|(name, mutating, summary)| Json::obj(vec![
        ("name", Json::s(name)),
        ("mutating", Json::b(mutating)),
        ("summary", Json::s(summary)),
    ])).collect();

    Json::obj(vec![
        ("openapi", Json::s("3.0.3")),
        ("info", Json::obj(vec![
            ("title", Json::s("ColdRevive daemon API")),
            ("version", Json::s("1.0.0")),
            ("description", Json::s(
                "现有 REST/JSON-RPC 面的描述符；opt-in 功能默认关闭，详见各端点 summary。")),
        ])),
        ("servers", Json::arr(vec![Json::obj(vec![
            ("url", Json::s("/")),
            ("description", Json::s("本进程监听地址（默认仅回环）")),
        ])])),
        ("paths", Json::obj(vec![
            ("/health", path(vec![
                ("get", op("节点真实运行状态；secret 值不会出现在响应中", "status")),
            ])),
            ("/api/v1/status", path(vec![
                ("get", op("/health 的稳定别名（同一实现，不复制状态）", "status")),
            ])),
            ("/openapi.json", path(vec![
                ("get", op("本描述符", "meta")),
            ])),
            ("/", path(vec![
                ("get", op("内嵌 Web UI 首页（api.web_ui 开启时；默认关闭）", "ui")),
            ])),
            ("/ui/app.js", path(vec![
                ("get", op("编译期嵌入的 Web UI 脚本（同源，无外部依赖）", "ui")),
            ])),
            ("/ui/style.css", path(vec![
                ("get", op("编译期嵌入的 Web UI 样式（同源，无外部依赖）", "ui")),
            ])),
            ("/ws", path(vec![
                ("get", op("RFC6455 状态推送（api.websocket 开启时；默认关闭）", "ui")),
            ])),
            ("/cli", path(vec![
                ("post", op("CLI 命令 dispatch（inspect/index/publish/nat/...）", "control")),
            ])),
            ("/inject", path(vec![
                ("post", op("故障注入：file-gone / restore（仅本机测试）", "control")),
            ])),
            ("/torrent/{key}", path(vec![
                ("get", op("取已入库样本的伴随 .torrent（infohash 或 file_hash）", "data")),
            ])),
            ("/announce", path(vec![
                ("get", op("本地 BT tracker announce（BEP 12 announce-list）", "data")),
            ])),
            ("/scrape", path(vec![
                ("get", op("本地 BT tracker scrape", "data")),
            ])),
            ("/nat/advertise", path(vec![
                ("get", op("本机真实 NAT 通告", "nat")),
            ])),
            ("/nat/public", path(vec![
                ("get", op("公网入向配置态（inbound_verified 恒 false）", "nat")),
            ])),
            ("/jsonrpc", path(vec![
                ("post", op("aria2 兼容 JSON-RPC 子集；mutating 方法可要求 token", "rpc")),
                ("options", op("CORS 预检", "rpc")),
            ])),
        ])),
        ("x-jsonrpc-methods", Json::arr(rpc_methods)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_is_valid_json_and_lists_key_endpoints() {
        let dump = document_json().dump();
        let parsed = Json::parse(&dump).expect("OpenAPI 必须是合法 JSON");
        assert_eq!(parsed.get("openapi").and_then(Json::as_str), Some("3.0.3"));
        let paths = parsed.get("paths").and_then(Json::as_obj).expect("paths");
        for key in ["/health", "/api/v1/status", "/cli", "/jsonrpc", "/announce",
                    "/torrent/{key}", "/openapi.json", "/ws", "/", "/ui/app.js",
                    "/ui/style.css"] {
            assert!(paths.iter().any(|(k, _)| k == key), "缺少端点 {key}");
        }
        let methods = parsed.get("x-jsonrpc-methods").and_then(Json::as_arr).expect("rpc methods");
        assert!(methods.iter().any(|m| m.get("name").and_then(Json::as_str)
                                    == Some("aria2.tellStatus")));
        assert!(methods.iter().any(|m| m.get("name").and_then(Json::as_str)
                                    == Some("aria2.addUri")
                                && m.get("mutating").and_then(Json::as_bool) == Some(true)));
    }
}
