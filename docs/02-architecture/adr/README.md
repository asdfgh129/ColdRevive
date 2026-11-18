---
doc_id: DOC-05
title: 架构决策记录集（ADR 索引）
category: 架构决策
version: 2.4
status: 持续维护
owner: 架构组
updated: 2026-11-17
---

# 架构决策记录集（ADR 索引）

> 编号 DOC-05 · 版本 2.4 · 状态 持续维护 · 责任人 架构组 · 更新 2026-11-17
> 一份 ADR 只记录**一个**技术抉择：上下文、选项、结论、后果、复核时点。状态为「待定」的 ADR 由 M0 Spike 或里程碑门禁触发定稿。

## 1. 决策清单

| ADR | 议题 | 决策 | 状态 | 复核时点 |
| :--- | :--- | :--- | :--- | :--- |
| [ADR-001](ADR-001-kad-network.md) | 复用公共 eMule KAD 还是自建私有 DHT | 默认 `public` 借力引导，保留 `private` / `hybrid` 切换 | ✅ 已接受（2026-11-13 所有者签署） | M0 / S2 |
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
| [ADR-012](ADR-012-r04-best-effort.md) | R-04 可达率是否设数值目标 | 改为「尽力」，取消一切百分比承诺 | ✅ 已接受（2026-11-16 所有者决策） | 每次对外描述可达性时 |
| [ADR-013](ADR-013-emule-lowid-declaration.md) | 节点如何告知自身可达性 | 借 eMule LowID：注册时自我声明 direct/relay-only/unknown + TTL | ✅ 已接受（2026-11-16） | M2 / S6 |
| [ADR-014](ADR-014-relay-data-budget.md) | 中继数据面是否限流 | 全局字节令牌桶 + 信令（≤4 KiB）优先 | ✅ 已接受（2026-11-16） | 部署中继带宽时 |
| [ADR-015](ADR-015-dependency-layering.md) | 是否继续全局零第三方依赖 | 取消全局零依赖：协议层保持 std-only，`cr-engine`/`coldrevived` 可用 librqbit + tokio | ✅ 已接受（2026-11-16 所有者决策） | 升级 librqbit / 补种上线前复核体积与内存 |
| [ADR-016](ADR-016-s6-emule-like-reach.md) | S6 是否仍须穷举 4 类 NAT + CGNAT | **阶段性改为 eMule 式可达语义**：类别覆盖降为描述性清单，不再是 pass 前提；真实设备 / 两台主机 / 30 次观测 / 不自动 pass 一条不松 | ✅ 已接受（2026-11-17 所有者决策） | **M4 前**，或当 ≥4 个不同真实出口可用时（先到为准） |

| [ADR-017](ADR-017-active-seed-pool.md) | 谁能同时供片 | 由 `cr-sched` 唯一决策；`Engine::activate` 只能经 `wake_seed`，满载逐出最久未用 | ✅ 已接受（2026-11-18） | 接入多任务做种时复核 |
| [ADR-018](ADR-018-tcp-hole-punch.md) | TCP 打洞怎么做、算不算完成 | 最小 FFI + `poll`/`SO_ERROR` 判据；**原语完成、BT 数据面未接线** | ✅ 已接受（2026-11-18） | 引擎支持外部 socket 或换接入方式时 |
| [ADR-019](ADR-019-public-vantage-nat-classification.md) | NAT 分类能不能信、单公网 IP 能测到哪 | 自建 RFC 5780 观测点（CHANGE-PORT 真换源口、CHANGE-IP 回 420）+ 三 socket 双对照；新增 `cone-undetermined`，**不得升级成 full-cone** | ✅ 已接受（2026-11-18） | 拿到第二块公网 IP 时补地址轴 |

| [ADR-020](ADR-020-relay-udp-transport.md) | 入向 TCP 被边缘掐断后，中继走哪条路 | 换承载层：独立 UDP 线缆格式（`CRU1`，64 B 头 + 1168 B 负载，逐报文 HMAC，回程不大于来程，seq 滑窗抗重放）；**不做重传**，`forwarded` ≠ 对端收到 | ✅ 已接受（2026-11-18；v2.4.86 补齐订阅/唤醒接线与保活） | 引入拥塞控制或与 TCP 注册表合并时 |

**统计**：19 项决策，18 项已接受、1 项待定（ADR-005 待 M4）；ADR-001 已于 2026-11-13 由项目所有者签署 `public` 默认模式并定稿。

> **ADR-011 实施状态**：L1 / L2 已于 2026-11-11 在 `tests/spikes/s2/` 完成首轮实现与
> RFC1918 / 回环验证（L3 仍否决），详见 [PLAN §22–§23](../../../tests/spikes/s2/PLAN.md)、
> [PROTO §2.2](../../04-protocol/PROTO.md) 与 [CHANGELOG 2.4.31](../../../CHANGELOG.md)。
> 按 §4「**已接受的 ADR 不修改正文**」，实施进度与实测证据记录在上述文档与变更记录中，
> **不回写 ADR-011 正文**；ADR-011 的决策、范围与复核时点一字未动。

> **G0 签署已完成（2026-11-13）**：[ADR-001](ADR-001-kad-network.md) 由项目所有者签署
> `public` 默认模式并定稿为「已接受」；同次评审批准 G0 条件冻结（S3/S5/S6 豁免 G0 时点结论，
> 状态仍 `not_run`，M1/M2 必测）。机读决策见 `tests/spikes/spikes.json` 的 `g0_review`，
> 详见 [G0-REVIEW](../../06-plan/G0-REVIEW.md)。下方 2026-11-11 条目保留为决策演化记录。
>
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
