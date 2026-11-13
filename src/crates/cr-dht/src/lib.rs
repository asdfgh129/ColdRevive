//! cr-dht — W3.2 Mainline DHT 适配（M3 骨架）。
//! 与 `cr-kad` 的 128-bit eMule KadID 并存时须保持 160-bit ID 命名空间隔离（S3）。

pub const MILESTONE: &str = "M3/W3.2";
pub const DHT_ID_BITS: usize = 160;
pub const KAD_ID_BITS: usize = 128;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dual_stack_id_widths_differ() { assert_ne!(DHT_ID_BITS, KAD_ID_BITS); }
}
