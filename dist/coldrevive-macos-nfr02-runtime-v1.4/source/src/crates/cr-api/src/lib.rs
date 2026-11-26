//! cr-api — W6.1/6.2/6.3 接入层。
//!
//! M1 的 `coldrevived` 内置最小 HTTP（/health、/cli、/torrent、/announce）。
//! 本 crate 提供守护进程直接复用的 std-only 助手：JSON-RPC token 判定、内嵌 Web UI
//! 静态资产、RFC6455 握手/帧编解码、OpenAPI 描述符。不引入 axum 或任何 websocket crate。
//!
//! `cr-qos` / `cr-dht` / `cr-reputation` 在此**再导出**，让守护进程只依赖一个接入层
//! 门面即可调用这些能力，无需改动它自己的 Cargo.toml。

pub mod openapi;
pub mod rpc;
pub mod webui;
pub mod ws;

pub use cr_dht as dht;
pub use cr_qos as qos;
pub use cr_reputation as reputation;

pub const MILESTONE: &str = "M3/W6.x";
pub const DEFAULT_BIND: &str = "127.0.0.1";
pub const RPC_SECRET_REQUIRED_OUTSIDE_LOOPBACK: bool = true;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_exposure_is_loopback_only() { assert_eq!(DEFAULT_BIND, "127.0.0.1"); }
}
