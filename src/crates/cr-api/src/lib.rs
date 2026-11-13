//! cr-api — W6.1/6.2/6.3 接入层（M3 骨架）。
//! M1 的 `coldrevived` 内置最小 HTTP（/health、/cli、/torrent、/announce），
//! 不引入 axum / WebSocket / aria2 JSON-RPC；这些属 M3，G2 通过前不实现。

pub const MILESTONE: &str = "M3/W6.x";
pub const DEFAULT_BIND: &str = "127.0.0.1";
pub const RPC_SECRET_REQUIRED_OUTSIDE_LOOPBACK: bool = true;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_exposure_is_loopback_only() { assert_eq!(DEFAULT_BIND, "127.0.0.1"); }
}
