---
doc_id: ADR-002
title: BT 引擎选 rqbit 还是 libtorrent FFI
category: 架构决策
version: 1.0
status: 待定
owner: 内核组
updated: 2026-11-09
---

# ADR-002：BT 引擎选 `rqbit` 还是 `libtorrent` FFI

| 项 | 值 |
| :--- | :--- |
| 状态 | **待定**（M0 / S1 后定稿） |
| 决策者 | 内核组 / 架构组 |
| 相关 | [TECH-STACK S1](../TECH-STACK.md) · [RISK R-05](../../05-quality/RISK.md) · [ARCH §4 M-11](../ARCH.md) |

## 背景

ColdRevive 的冷热调度依赖三个能力：**按需实例化做种上下文、Pause/Resume、在握手阶段拦截并解析 InfoHash**。如果所选引擎不暴露这些接口，[LLD §5.6/5.7](../../03-design/LLD.md) 的核心机制（LRU 活跃池、被动唤醒）将无法实现，整个架构需要重做。

`rqbit` 是纯 Rust 实现，与项目技术栈一致；`libtorrent` 成熟稳定但需 FFI，带来跨语言内存安全与打包复杂度。

## 选项

| 选项 | 优点 | 缺点 |
| :--- | :--- | :--- |
| A. `rqbit` | 纯 Rust、无 FFI、与 Tokio 生态一致、易于定制 | 维护状态与高级能力需核实，可能缺 Pause/Resume 或握手钩子 |
| B. `libtorrent` FFI | 功能完备、久经考验、NAT/DHT 能力齐全 | FFI 边界复杂、二进制体积上升、定制握手拦截仍可能需补丁 |
| C. 自研精简做种层 | 完全可控、只实现所需子集 | 工作量最大，协议兼容性风险高 |

## 决策

**暂定选项 A**，但以 **S1 Spike 为硬门禁**：先写最小 Demo，验证「拦截 Handshake → 动态加载种子 → 正常完成标准下载」全链路。

- S1 通过 → 采用 `rqbit`，按 [ARCH §4](../ARCH.md) 落地 M-11。
- S1 不通过 → 切换 `libtorrent` FFI；仍不通过则评估选项 C 的精简做种层。

**无论选择哪个**，业务 crate 一律只依赖 `cr-engine` 的 `Engine` Trait，禁止直接引用具体库（见 [TECH-STACK §5](../TECH-STACK.md)）。

## 后果

- 正面：选型失败不会推翻上层设计，切换成本被限制在 `cr-engine` 内部。
- 负面：Trait 抽象会牺牲部分引擎特有优化；需为 Pause/Resume 语义写契约测试。
- 风险：若 `rqbit` 停更，按 [RISK R-12](../../05-quality/RISK.md) 处理。

## 复核时点

M0 Spike S1 结论出具后立即定稿；此后每里程碑复审上游维护状态。

## 相关文档

[ADR 索引](README.md) · [LLD §5.8](../../03-design/LLD.md) · [TEST 兼容性矩阵](../../05-quality/TEST.md)
