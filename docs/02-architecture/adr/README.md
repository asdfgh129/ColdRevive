---
doc_id: DOC-05
title: 架构决策记录集（ADR 索引）
category: 架构决策
version: 2.2
status: 持续维护
owner: 架构组
updated: 2026-11-11
---

# 架构决策记录集（ADR 索引）

> 编号 DOC-05 · 版本 2.2 · 状态 持续维护 · 责任人 架构组 · 更新 2026-11-11
> 一份 ADR 只记录**一个**技术抉择：上下文、选项、结论、后果、复核时点。状态为「待定」的 ADR 由 M0 Spike 或里程碑门禁触发定稿。

## 1. 决策清单

| ADR | 议题 | 决策 | 状态 | 复核时点 |
| :--- | :--- | :--- | :--- | :--- |
| [ADR-001](ADR-001-kad-network.md) | 复用公共 eMule KAD 还是自建私有 DHT | 默认 `public` 借力引导，保留 `private` / `hybrid` 切换 | 🟡 待定 | M0 / S2 |
| [ADR-002](ADR-002-bt-engine.md) | BT 引擎选 `rqbit` 还是 `libtorrent` FFI | 采用 `rqbit`（S1 4/4 `verified` 后采纳），握手拦截自实现 | ✅ 已接受 | M0 / S1（2026-11-11 依 S1 4/4 `verified` 回填 `S1.status=pass` 定稿） |
| [ADR-003](ADR-003-ui-delivery.md) | UI 交付形态 | 守护进程内嵌 HTTP/WS + 可关闭 UI 进程 | ✅ 已接受 | — |
| [ADR-004](ADR-004-ed2k-scope.md) | 是否内置完整 eD2K 协议栈 | v1 即计算 MD4 哈希；**全栈由 ADR-011 提前至 v1**，哈希转译类补源仍 v2 | ✅ 已接受（部分取代） | 已于 2026-11-11 复核 |
| [ADR-005](ADR-005-incentive.md) | 是否引入贡献积分 / 全局信用 | 后置 P2，须先解决抗女巫与信用伪造 | 🟡 待定 | M4 |
| [ADR-006](ADR-006-metadata-distribution.md) | 元数据分发方式 | KAD 只存描述符；种子走 HTTP / magnet `xs` | ✅ 已接受 | — |
| [ADR-007](ADR-007-tracker.md) | 是否完全取代 Tracker | 否，Tracker 降级为可选加速层 | ✅ 已接受 | — |
| [ADR-008](ADR-008-websocket.md) | 状态推送机制 | WebSocket，禁止 UI 轮询 | ✅ 已接受 | — |
| [ADR-009](ADR-009-sharing-model.md) | 共享模型 | 常态共享目录（体验层）+ 按需唤醒（实现层） | ✅ 已接受 | — |
| [ADR-010](ADR-010-rust-tokio.md) | 语言与运行时 | Rust + Tokio | ✅ 已接受 | — |
| [ADR-011](ADR-011-ed2k-interop.md) | eD2k 双向互操作边界 | v1 优先兼容旧客户端：全部文件公开、实现标准 eD2k 栈、否决唤醒网关 | ✅ 已接受 | M1 / M4；L1 / L2 探针已于 2026-11-11 落地（见下注） |

**统计**：11 项决策，9 项已接受、2 项待定（ADR-001 待所有者签署 `public`/`private` 默认模式、ADR-005 待 M4）。

> **ADR-011 实施状态**：L1 / L2 已于 2026-11-11 在 `tests/spikes/s2/` 完成首轮实现与
> RFC1918 / 回环验证（L3 仍否决），详见 [PLAN §22–§23](../../../tests/spikes/s2/PLAN.md)、
> [PROTO §2.2](../../04-protocol/PROTO.md) 与 [CHANGELOG 2.4.31](../../../CHANGELOG.md)。
> 按 §4「**已接受的 ADR 不修改正文**」，实施进度与实测证据记录在上述文档与变更记录中，
> **不回写 ADR-011 正文**；ADR-011 的决策、范围与复核时点一字未动。

> **G0 待签署项（2026-11-11）**：[ADR-002](ADR-002-bt-engine.md) 已于 2026-11-11 依 S1 四条判据
> 全部 `verified` 证据回填 `S1.status = pass` 并定稿为「已接受」（选项 A：采用 `rqbit`，握手拦截
> 自实现）；[ADR-001](ADR-001-kad-network.md) 的定稿条件自 2026-11-10 `S2 = pass` 起已满足，
> 但该 ADR 明文要求「定稿与 `public`/`private` 的签署由项目所有者执行」，**状态列仍为「待定」**，
> 属治理待办而非技术缺口；[ADR-005](ADR-005-incentive.md) 复核时点为 M4，按 ADR 自身复核时点
> 以 M4 为准，M0 阶段不强制定稿（与 [ROADMAP M0 交付物](../../06-plan/ROADMAP.md) 的「ADR-005
> 定稿」表述存在口径差异，按 ADR 自身复核时点优先）。

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
