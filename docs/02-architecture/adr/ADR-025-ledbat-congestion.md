---
doc_id: ADR-025
title: ADR-025 中继 UDP 流改用时延驱动拥塞控制（LEDBAT 语义），AIMD 保留为可选项
category: 架构决策
version: 1.0
status: 已定稿
owner: 架构组
updated: 2026-11-28
---

# ADR-025 中继 UDP 流改用时延驱动拥塞控制

## 状态

已采纳（2026-11-28，所有者决定 1 选 A）。实现：`cr_nat::relay_udp_stream::CcMode`。
**本 ADR 只改变「cwnd 由什么信号驱动」，不改变可靠性层、不改变判据阈值、不改变任何门禁字段。**

## 背景：EX-1 的 FAIL 不是量具问题，是 AIMD 的信号选错了

EX-1（`G2-FAIRNESS-EXCEPTION-2026-11-25.md`）的事实：

- 判据预先登记：两条流份额各在 `[0.45, 0.55]`、Jain ≥ `0.95`、**每轮都要达标**；
- L1（16 Mbit/s，20 ms）四 arm 全过；L2（8 Mbit/s，50 ms）`cc_off` / `cap` 各 10/10，
  但 `cc_on` 与 `cc_paced` 各 **9/10**，失败轮 `0.430/0.570`、`0.433/0.567`；
- 合计 **78/80 → FAIL**；
- 归因：`r(丢包差, 份额偏移) = −0.706`，失败轮恰是丢包最不对称的轮；中继
  `shaping/dropped/limited` 全为 0；`cc_off` 全部落在 0.499–0.502。

结论很清楚：**在对称链路上量具无偏，在丢包不对称时 AIMD 会把「丢得多」误当成「我该让」**。
AIMD 的乘性减半（`beta = 0.5`）只认丢包，不区分「拥塞丢包」与「随机/非对称丢包」，
于是两条同速率竞争流按丢包比例分账，而不是按可用带宽分账。这不是调 `β` 能修的：
调大 `β` 让退让变慢、收敛变差；调小则更容易被随机丢包打崩。

## 可选项

| 方案 | 机制 | 判定 |
| :--- | :--- | :--- |
| A. 继续 AIMD，调 `β` 或加丢包过滤 | 仍以丢包为主信号 | **否决**：C4 第 5 条禁止「调 β 解释已有数据」；且根因未变 |
| B. 每流静态配额 / 中继侧加权公平 | 在瓶颈处分配，不靠端侧反馈 | **否决（本轮）**：等于把公平性变成配置项，跨主机无法自证；且 S6 已把中继定位为退化路径 |
| C. **时延驱动（LEDBAT 语义）** | cwnd 由**排队时延**的升降驱动，丢包只触发重传 | **采纳** |
| D. 不做，接受 EX-1 延期 | 0 代码 | 所有者未选；记录在案 |

C 的直接理由：排队时延是**共享瓶颈的公共信号**。两条流挤同一个 HTB 桶时，谁多占带宽，
谁看到的排队时延就高，与「谁被随机丢了几个包」无关。把 cwnd 的增长/收缩接到这个信号上，
分账偏移的驱动项就从「丢包差」变成「占用差」——这正是公平性要的东西。

## 决策：新增 `CcMode::Ledbat`，AIMD 原样保留

`CongestionConfig` 增加三个字段（默认值使既有行为逐字节不变）：

```rust
pub enum CcMode { Aimd, Ledbat }   // 默认 Aimd
pub mode: CcMode,
pub ledbat_target: Duration,       // 默认 100 ms（RFC 6817 TARGET）
pub ledbat_gain: f64,              // 默认 1.0（RFC 6817 GAIN）
```

### 语义（每个 RTT 最多更新一次 cwnd）

1. **基准时延** `base_delay` = 观测到的最小 RTT（只降不升，单调）。
2. **排队时延** `qd = 本次 RTT 样本 − base_delay`。
3. 距上次更新已满一个 `srtt` 时：
   - `qd < target`：`cwnd += gain × (target − qd) / target`（单位：段）；
   - `qd ≥ target`：`cwnd −= gain × (qd − target) / target`；
   - 结果夹到 `[cwnd_min, cwnd_max]`。
4. `cwnd_max` 仍为 `WINDOW`（32 段），**不放大**——窗口上限是协议面约束，不是拥塞策略。

于是：空链路（`qd ≈ 0`）每 RTT 涨 1 段；排队满 `target` 时停止增长；超过 `target` 时按超出
比例回退。稳定态是「把瓶颈队列填到 `target` 附近」，而不是「把队列填到丢包」。

### 与 RFC 6817 的**明示偏离**（必须写清，否则就是拿标准当挡箭牌）

| 项 | RFC 6817 | 本实现 | 为什么 |
| :--- | :--- | :--- | :--- |
| 丢包时的 cwnd | 超时后 `cwnd = 2·MSS`（重启） | **不降窗**，只重传；仅在「连续超时」时复位 | 非对称随机丢包不是拥塞信号；降窗正是 EX-1 的失败机理 |
| 更新节奏 | 每 RTT | 每 RTT（同） | — |
| 慢启动 | 有（初始指数增长） | **无**，从 `cwnd_init` 起线性起步 | 窗口上限只有 32 段，指数阶段只会放大起步期的抢占不公平 |
| 延迟基准 | 固定窗口内最小 | 单调最小（不老化） | 路由不变时单调最小更稳；老化会把「基准漂移」误判成「排队」 |

**塌陷保护（必须存在，否则上面的偏离会变成不负责任）**：同一段连续 RTO 重传达到
`LEDBAT_RTO_COLLAPSE = 6` 次且 base 未推进，则把 `cwnd` 复位到 `cwnd_init`，并记
`collapse_resets`。理由：完全不降窗在真实拥塞崩溃时会把队列越堆越深；连续超时是
「时延信号拿不到、但链路确实不通」的兜底证据。

### 不改的东西

- 可靠性层（seq/ack/SACK/重传/重组）一行不改；
- 中继令牌桶、`BandwidthLimited` 背压语义不改（背压仍不降窗，见 `note_relay_backpressure`）；
- pacing 仍是独立开关，可与 LEDBAT 组合；
- 判据阈值不改（见下方「与判据的关系」）。

## 与判据的关系（顺序纪律）

所有者决定 1 选 A，因此本轮**重新预注册**判据：
`tests/fixtures/g2-fairness-thresholds-v2.toml`（新时间戳 + 新 sha256），
在写代码与采样**之前**提交。

- 登记时间：`2026-11-28T09:20:38+08:00`；**sha256 `4d5d931e4fa90f244a7dad307a00496026596286bba89fb5485e291791def615`**
  （文件 3 896 B，`eol=lf` 锁定，不随 `core.autocrlf` 变化）。**代码与采样都在这条之后**。

- **阈值一个不动**：份额 `[0.45, 0.55]`、Jain ≥ `0.95`、`all_rounds_must_pass`。
  变的只有被测机制与新增的 provenance 要求。
- 旧判据 `g2-fairness-thresholds.toml`（sha `0671a173…`）**保留不删不改**，它是 v1 的历史。
- 新判据不得用于「重跑同一判据这次全过」——那是 C4 第 4 条禁止的写法。

## 后果

- 正面：分账不再由丢包差驱动；慢链路（8 Mbit/s / 50 ms）的 43/57 偏移应有机制性改善。
- 负面 / 风险：
  - 时延驱动对**缓冲膨胀（bufferbloat）**敏感：`target = 100 ms` 在深队列链路上会主动堆到
    100 ms，端到端时延变大。这是 LEDBAT 的固有取舍，不是 bug。
  - 与 Cubic/Reno 竞争时会**礼让过头**（LEDBAT 天然低优先级）。本项目的竞争场景是
    「本项目自己的两条中继流」，不与外部 TCP 抢，因此可接受；一旦用于公网共享瓶颈须重评。
  - 单调最小基准在**路径变化**（换路由/换出口）后会把新基准当排队，导致短暂过度回退。
    缓解：`collapse` 与 `base_delay` 在 `srtt` 连续 30 个样本上升超过 2× 时重置（见实现）。
- 不产生任何 G2/NFR 结论：本 ADR 只改机制，判定仍由预注册判据跑出来，且必须带 provenance。

## 相关

[ADR-024](ADR-024-udp-bt-data-plane.md) · [ADR-014](ADR-014-relay-data-budget.md) ·
EX-1 [G2-FAIRNESS-EXCEPTION](../../06-plan/G2-FAIRNESS-EXCEPTION-2026-11-25.md) ·
判据 v2 `tests/fixtures/g2-fairness-thresholds-v2.toml` · `src/crates/cr-nat/src/relay_udp_stream.rs`
