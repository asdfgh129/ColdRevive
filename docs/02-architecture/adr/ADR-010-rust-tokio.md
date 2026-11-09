---
doc_id: ADR-010
title: 语言与运行时选用 Rust + Tokio
category: 架构决策
version: 1.0
status: 已接受
owner: 架构组 / 内核组
updated: 2026-11-09
---

# ADR-010：语言与运行时选用 Rust + Tokio

| 项 | 值 |
| :--- | :--- |
| 状态 | **已接受** |
| 决策者 | 架构组 / 内核组 |
| 相关 | [TECH-STACK §1](../TECH-STACK.md) · [ARCH §2](../ARCH.md) · [src/README.md](../../../src/README.md) |

## 背景

ColdRevive 需要同时处理：上千个 UDP/TCP 连接、DHT 路由表维护、大文件哈希计算、SQLite 索引、HTTP/WS 服务，并在 **1 万文件空闲常驻 ≤100 MB** 的硬约束下长期运行。讨论中调研的 Rust eD2K 生态（`ed2k-rs`、`rqbit`）与 FluxDown 的 Rust + Tokio 三进程架构，也提供了可复用的工程参照。

## 选项

| 选项 | 评价 |
| :--- | :--- |
| A. C++（沿用 aria2 / aMule 路线） | 可复用代码多，但内存安全与异步开发成本高，且需自建 HTTP/WS 层 |
| B. Go | 开发效率高、网络库成熟；但 GC 抖动与运行时常驻内存不利于 100 MB 目标 |
| C. **Rust + Tokio** | 零成本抽象、内存安全、异步 I/O 成熟、单二进制交付，**采纳** |
| D. Node.js / Python | 不适合长期常驻的哈希与 IO 密集负载，否决 |

## 决策

- 采用 **Rust + Tokio**，Cargo Workspace 按 [ARCH §4 模块清单](../ARCH.md) 的 `M-01～M-16` 拆分 crate。
- 依赖选择详见 [TECH-STACK 选型总表](../TECH-STACK.md)：SQLite(rusqlite)、notify、axum、tracing；BT 引擎由 [ADR-002](ADR-002-bt-engine.md) 决定。
- **M0 才创建 `Cargo.toml`**：技术路线冻结前不固化依赖树，避免 Spike 结论变化时返工。

## 后果

- 正面：内存与 CPU 可控、无 GC 停顿、跨平台单二进制、与现有 Rust P2P 生态可复用。
- 负面：团队需 Rust 能力；编译时间较长；部分 FFI 场景（如切 libtorrent）需额外审计。
- 约束：核心模块单测覆盖率 ≥70%（[SRS NFR-10](../../01-product/SRS.md)）；所有外部输入必须校验（NFR-08）。

## 相关文档

[ADR 索引](README.md) · [TECH-STACK 选型与 Spike 清单](../TECH-STACK.md) · [ROADMAP M0](../../06-plan/ROADMAP.md)
