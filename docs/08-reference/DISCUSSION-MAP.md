---
doc_id: DOC-18
title: 讨论记录映射（DISCUSSION-MAP）
category: 参考
version: 2.0
status: 有效
owner: 架构组
updated: 2026-11-09
---

# 讨论记录映射（DISCUSSION-MAP）

> 编号 DOC-18 · 版本 2.0 · 状态 有效 · 责任人 架构组 · 更新 2026-11-09
> 原始讨论：[chat.md](../09-archive/chat.md)（已归档，约 940 行）。本表用于**从结论反查论据**，也用于核对某轮讨论是否已被完整吸收。

## 1. 讨论脉络

```
aria2 RPC / 静态 HTML 原理
  → 外部静态 HTML vs 内嵌 Web 服务
  → Web UI + RPC 共生、WebSocket vs 轮询
  → aria2 二次开发三方案与 HttpServer 源码
  → Rust eD2K 生态调研、FluxDown 架构
  → BT 搜索软件盘点、eMule 搜索为何更强
  → 死种问题与已有补丁方案
  → 混合客户端的四大难点（激励/索引/调度/兼容）
  → 【收敛】KAD 黄页 + 私有唤醒 RPC + 标准数据管道（v1.0 草案）
  → 共享目录与伴随种子生成
  → 阈值控制与冷热切入切出
  → KAD 广播种子 / 不依赖 Tracker
  → hMule 为何不流行（复盘）
  → 【v2.0】Consolidated Spec
  → 【v2.1】按工程规范拆分
```

## 2. 讨论轮次 → 文档落点

| 讨论主题 | 主要落点 | 补充落点 |
| :--- | :--- | :--- |
| aria2 RPC / 静态 HTML 原理 | [TECH-STACK §4](../02-architecture/TECH-STACK.md) | [ARCH §3](../02-architecture/ARCH.md) |
| 外部静态 HTML vs 内嵌 Web 服务 | [ADR-003](../02-architecture/adr/ADR-003-ui-delivery.md) | [LLD §12](../03-design/LLD.md) |
| Web UI + RPC 共生、WebSocket vs 轮询 | [ADR-008](../02-architecture/adr/ADR-008-websocket.md) | [API §3](../04-protocol/API.md) |
| aria2 二次开发三方案、HttpServer 源码 | [TECH-STACK §3 / §4](../02-architecture/TECH-STACK.md) | 参考项目 naria2 |
| Rust eD2K 生态 / FluxDown | [TECH-STACK §1 / §3](../02-architecture/TECH-STACK.md) | [ARCH §3 进程模型](../02-architecture/ARCH.md)、[ADR-010](../02-architecture/adr/ADR-010-rust-tokio.md) |
| BT 搜索软件 / eMule 搜索为何更强 | [PRD §1](../01-product/PRD.md) | [LLD §4](../03-design/LLD.md) |
| 死种问题与已有补丁方案 | [PRD §1](../01-product/PRD.md) | [TECH-STACK §3](../02-architecture/TECH-STACK.md) |
| 混合客户端难点（激励/索引/调度/兼容） | [PRD §1 / §4](../01-product/PRD.md) | [LLD §11 / §13](../03-design/LLD.md)、[ADR-005](../02-architecture/adr/ADR-005-incentive.md)、[RISK R-01](../05-quality/RISK.md) |
| KAD 黄页 + 私有唤醒架构（v1.0 草案） | [ARCH §1](../02-architecture/ARCH.md) | 全文基线，变更见 [CHANGELOG](../../CHANGELOG.md) |
| 共享目录 + 伴随种子生成 | [FLOWS F1](../03-design/FLOWS.md)、[LLD §2](../03-design/LLD.md) | [ADR-009](../02-architecture/adr/ADR-009-sharing-model.md) |
| 阈值控制与冷热切入切出 | [LLD §6 / §10](../03-design/LLD.md) | [CONFIG](../04-protocol/CONFIG.md)、[SRS NFR-01～03](../01-product/SRS.md) |
| KAD 广播种子 / 不依赖 Tracker | [ADR-007](../02-architecture/adr/ADR-007-tracker.md) | [ADR-006](../02-architecture/adr/ADR-006-metadata-distribution.md)、[PROTO §2](../04-protocol/PROTO.md) |
| hMule 为何不流行 | [LLD §13](../03-design/LLD.md)、[PRD C8](../01-product/PRD.md) | [RISK R-01](../05-quality/RISK.md) |

## 3. v1.0 → v2.0 的八处修订

`chat.md` 第 584–739 行内嵌的《ColdRevive 客户端开发设计文档 v1.0》写于后续讨论之前，v2.0 的主要修订：

| # | v1.0 | v2.0 | 落点 |
| :--- | :--- | :--- | :--- |
| 1 | 依赖私有 RPC 主动唤醒为主路径 | 常态共享目录（体验层）+ 按需唤醒（实现层）双层模型 | [ADR-009](../02-architecture/adr/ADR-009-sharing-model.md) |
| 2 | 未解决 BT 基于种子而非文件的硬壁垒 | 伴随种子生成 | [LLD §2](../03-design/LLD.md) |
| 3 | 活跃做种数量无约束 | 冷热分级 + LRU + 影子索引 + 握手拦截 | [LLD §3 / §6 / §7](../03-design/LLD.md) |
| 4 | 仅描述 KAD 发布/查询 | 分层 DHT；明确 128/160-bit ID 空间不兼容 | [ADR-007](../02-architecture/adr/ADR-007-tracker.md) |
| 5 | 安全仅列 3 条 | 完整威胁模型、Challenge-Response、限流、信誉 | [LLD §5 / §11](../03-design/LLD.md) |
| 6 | 未涉及冷启动 | hMule 复盘，列为头号非技术风险 | [RISK R-01](../05-quality/RISK.md) |
| 7 | UI 选型仅列出 Tauri/GPUI | 内嵌 HTTP/WS、单端口、aria2 JSON-RPC 兼容 | [ADR-003](../02-architecture/adr/ADR-003-ui-delivery.md) |
| 8 | 里程碑 3 阶段 | M0–M5、WBS、门禁、KPI、测试矩阵、风险登记册、ADR | [ROADMAP](../06-plan/ROADMAP.md) |

## 4. 使用约定

- 需要某个结论的**原始论据**时回查 `chat.md`；需要**现行定义**时以 DOC-01～DOC-19 为准。
- 归档内容与现行文档冲突时，一律以现行文档为准，并在 [OPEN-QUESTIONS](../06-plan/OPEN-QUESTIONS.md) 登记差异。
- `chat.md` 只读，禁止在其上修改后当作现行文档使用（[归档说明](../09-archive/README.md)）。

## 相关文档

- [chat.md（归档）](../09-archive/chat.md)
- [v2.0 合并文档（归档）](../09-archive/ColdRevive-项目开发文档-v2.0.md)
- [CHANGELOG](../../CHANGELOG.md)
- [DOC-19 参考资料与待核实项](REFERENCES.md)
