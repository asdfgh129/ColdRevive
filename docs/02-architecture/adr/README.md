---
doc_id: DOC-05
title: 架构决策记录集（ADR 索引）
category: 架构决策
version: 2.0
status: 持续维护
owner: 架构组
updated: 2026-11-09
---

# 架构决策记录集（ADR 索引）

> 编号 DOC-05 · 版本 2.0 · 状态 持续维护 · 责任人 架构组 · 更新 2026-11-09
> 一份 ADR 只记录**一个**技术抉择：上下文、选项、结论、后果、复核时点。状态为「待定」的 ADR 由 M0 Spike 或里程碑门禁触发定稿。

## 1. 决策清单

| ADR | 议题 | 决策 | 状态 | 复核时点 |
| :--- | :--- | :--- | :--- | :--- |
| [ADR-001](ADR-001-kad-network.md) | 复用公共 eMule KAD 还是自建私有 DHT | 默认 `public` 借力引导，保留 `private` / `hybrid` 切换 | 🟡 待定 | M0 / S2 |
| [ADR-002](ADR-002-bt-engine.md) | BT 引擎选 `rqbit` 还是 `libtorrent` FFI | 先 `rqbit`，S1 不过则切 FFI | 🟡 待定 | M0 / S1 |
| [ADR-003](ADR-003-ui-delivery.md) | UI 交付形态 | 守护进程内嵌 HTTP/WS + 可关闭 UI 进程 | ✅ 已接受 | — |
| [ADR-004](ADR-004-ed2k-scope.md) | 是否内置完整 eD2K 协议栈 | v1 仅计算 MD4 哈希；全栈与补源排 v2 | ✅ 已接受 | M5 前复核 |
| [ADR-005](ADR-005-incentive.md) | 是否引入贡献积分 / 全局信用 | 后置 P2，须先解决抗女巫与信用伪造 | 🟡 待定 | M4 |
| [ADR-006](ADR-006-metadata-distribution.md) | 元数据分发方式 | KAD 只存描述符；种子走 HTTP / magnet `xs` | ✅ 已接受 | — |
| [ADR-007](ADR-007-tracker.md) | 是否完全取代 Tracker | 否，Tracker 降级为可选加速层 | ✅ 已接受 | — |
| [ADR-008](ADR-008-websocket.md) | 状态推送机制 | WebSocket，禁止 UI 轮询 | ✅ 已接受 | — |
| [ADR-009](ADR-009-sharing-model.md) | 共享模型 | 常态共享目录（体验层）+ 按需唤醒（实现层） | ✅ 已接受 | — |
| [ADR-010](ADR-010-rust-tokio.md) | 语言与运行时 | Rust + Tokio | ✅ 已接受 | — |

**统计**：10 项决策，7 项已接受、3 项待 M0/M4 定稿。

## 2. 与 DR 编号的关系

[ARCH §5](../ARCH.md) 中的 `DR-01～DR-08` 是**架构决策摘要**（面向快速阅读），与 ADR 的对应关系如下；正文以 ADR 为准。

| DR | 对应 ADR |
| :--- | :--- |
| DR-01 双层共享模型 | ADR-009 |
| DR-02 保留唤醒 RPC | ADR-007（决策依据）+ [PRD C2](../../01-product/PRD.md) |
| DR-03 分层 DHT | ADR-007 + ADR-001 |
| DR-04 UI 交付形态 | ADR-003 |
| DR-05 状态推送 | ADR-008 |
| DR-06 进程模型 | ADR-003 |
| DR-07 元数据分发 | ADR-006 |
| DR-08 语言运行时 | ADR-010 |

## 3. ADR 模板

新增 ADR 时复制以下结构，文件名格式 `ADR-NNN-kebab-case.md`，并同步本表与 [docs/README.md](../../README.md)：

```markdown
---
doc_id: ADR-NNN
title: 一句话议题
category: 架构决策
version: 1.0
status: 提议 | 待定 | 已接受 | 已取代
owner: 决策角色
updated: YYYY-MM-DD
---

# ADR-NNN：<议题>

| 项 | 值 |
| :--- | :--- |
| 状态 | ... |
| 决策者 | ... |
| 相关 | ... |

## 背景
## 选项
## 决策
## 后果
## 复核时点（可选）
## 相关文档
```

## 4. 状态流转规则

```
提议 → 待定（需 Spike 或数据） → 已接受
                              ↘ 已取代（由新 ADR 明确引用并说明原因）
```

- **已接受的 ADR 不修改正文**：结论变更时新增 ADR 并在旧文标注「已取代」，保留决策演化链。
- 「待定」项必须在 [ROADMAP](../../06-plan/ROADMAP.md) 中有对应门禁，否则视为悬空决策。
- 协议相关决策（ADR-006/007）变更须同步递增 [PROTO](../../04-protocol/PROTO.md) 的 `PROTOCOL_VERSION`。

## 相关文档

- [DOC-03 ARCH 总体架构设计](../ARCH.md)
- [DOC-04 技术选型与 Spike 清单](../TECH-STACK.md)
- [DOC-14 ROADMAP 质量门禁](../../06-plan/ROADMAP.md)
- [DOC-15 待决问题清单](../../06-plan/OPEN-QUESTIONS.md)
