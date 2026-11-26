//! cr-notify — 跨平台通知（可选骨架）。

pub const OPTIONAL: bool = true;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn module_is_optional() { assert!(OPTIONAL); }
}
