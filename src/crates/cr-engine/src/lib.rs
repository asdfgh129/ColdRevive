//! cr-engine — W5.1 BT 引擎抽象（M2 骨架）。
//!
//! ADR-002 已定稿采用 rqbit（选项 A），但真实引擎集成属 M2/W5.1，
//! 须在 G1（T0/T1 复活成功率 100%）通过后开工；本文件只固定抽象边界。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineState { Stopped, Cold, Waking, Hot }

/// 业务 crate 只能依赖本 Trait，不得直接依赖具体 BT 库。
pub trait Engine {
    fn activate(&mut self, infohash: &str) -> Result<(), String>;
    fn pause(&mut self) -> Result<(), String>;
    fn resume(&mut self) -> Result<(), String>;
    fn state(&self) -> EngineState;
}

pub const MILESTONE: &str = "M2/W5.1";
pub const GATE: &str = "G1";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gate_is_recorded() { assert_eq!(GATE, "G1"); }
}
