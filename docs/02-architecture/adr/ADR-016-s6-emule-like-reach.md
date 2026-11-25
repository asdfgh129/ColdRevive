---
doc_id: ADR-016
title: S6 阶段性判据改为 eMule 式可达语义，不再穷举 NAT 类别
category: 架构决策
version: 1.1
status: 已接受（2026-11-25 完成首次复审 R1）
owner: 项目所有者
updated: 2026-11-25
---

# ADR-016：S6 阶段性判据改为 eMule 式可达语义，不再穷举 NAT 类别

| 项 | 值 |
| :--- | :--- |
| 状态 | **已接受**（2026-11-17 项目所有者决定）；2026-11-25 完成首次复审 R1：维持本判据，触发条件改为「≥4 出口且 ≥3 类别」 |
| 决策者 | 项目所有者 / 架构组（记录） |
| 性质 | **阶段性（interim）判据**，带明确复审时点；不是「G2 达标」 |
| 关联 | [ADR-012](ADR-012-r04-best-effort.md) · [ADR-013](ADR-013-emule-lowid-declaration.md) · [nat-matrix.md §21](../../../tests/spikes/s6/nat-matrix.md) · [nat_matrix_config.json](../../../tests/spikes/s6/nat_matrix_config.json) · [ROADMAP](../../06-plan/ROADMAP.md) |
| 决策原文 | 「改判据，目前只要达到 eMule 类似的可达就可以了」（2026-11-17） |

## 背景

S6 原判据要求「4 类 NAT + 1 个 CGNAT 各有 ≥1 份 `evidence_valid=true` 的逐类报告」。
[nat-matrix.md §13](../../../tests/spikes/s6/nat-matrix.md) 已经论证：**一个网络出口在任一时刻
只对应一个真实 NAT 类别**，因此凑齐 5 类需要 ≥4 个不同真实出口；同机 netns、串行重测、
CPE 配置变体都不得计入独立覆盖（§18.1）。现有两个真实出口只覆盖
`port-restricted-cone`（`112.5.60.4`）与 `symmetric`（`112.5.32.34`），且前者只有分类、
没有逐类 30 次观测（§19.6）。

项目所有者 2026-11-17 决定：**当前阶段不再以穷举 NAT 类别为验收前提，达到 eMule 式可达语义即可**。

这个决定与此前两次决策是连贯的，不是临时让步：

- [ADR-012](ADR-012-r04-best-effort.md) 已取消可达率的数值承诺，理由是「产品提供的是多一条
  能叫醒机器的路，不是 SLA」；既然不再产出百分比，「按类别抽样出数字」就失去了意义。
- [ADR-013](ADR-013-emule-lowid-declaration.md) 已把 eMule 的核心机制落地：节点自报
  `direct` / `relay-only` / `unknown` + TTL，注册即声明，`relay-only` 才有资格跳过直连，
  `unknown` 不许当 LowID。W5.2 在真实 222 / A / 中继上跑了 **21/21 连续 3 轮**。
  eMule 面对同样的 NAT 世界几十年，从不测可达率，靠的就是「失败应当被预先知道」。

因此 S6 真正要回答的问题从「这几类各能打通多少」变成：
**在这个 NAT 世界里，每个节点是否都有一条可用的路，且走不通时能被预先知道并解释。**

## 选项

| 选项 | 做法 | 判断 |
| :--- | :--- | :--- |
| A | 维持 5 类矩阵不变，等真实出口环境齐备 | 判据不失真，但把 G2 卡在**环境采购**上，与当前主线（标准协议互通、补种）无关 |
| B | 按行为维度合并为 3 组（EIM / APDM / 双层 NAT） | 技术上更准确，但仍是「按类别抽样」，只是格子变少；仍需新增 CGNAT 出口 |
| **C** | **改为 eMule 式可达语义：类别覆盖降级为描述性清单** | **选定**。直接对齐产品承诺与 ADR-013 已落地的机制；不新增出口也能继续推进 |
| D | 不改判据，照 G3 先例「带例外结案」 | 诚实但只是记录缺口，没有回答「什么算够」 |

## 决策

**S6 阶段性验收改为「eMule 式可达语义」**。`required_classes` / `additional_environment`
降级为**描述性类别清单**（供 `--census` 归档与报告标注），不再是 pass 前提。

新的判据（`nat_matrix_config.json` → `s6_pass_criteria`，`spikes.json` → `S6.criteria`）：

1. 在**真实 NAT 设备**上取得 ≥1 份 `evidence_valid=true` 的 `real-multi-nat` 报告；
   条款①两端分处两台不同物理主机、条款② `nat_is_real_device` 可证伪，**两条都不变**；
2. 每份报告的 `direct` 与 `relay` 两条路径各 **≥30 次真实唤醒**，成功判据仍是真实
   `WAKEUP_ACK(status=OK)`；超时、缺字段、载体错误一律计入失败；
3. 至少一份报告显示 **`direct` 可达**（HighID 语义），至少一份显示 **`direct` 失败但 `relay`
   送达**（LowID 语义）；两条都失败的环境记为**已知限制**并写明，不得静默省略；
4. 节点自报可达性与实测行为一致：只有 `relay-only` 可跳过直连，`unknown` 不得当作
   LowID（[ADR-013](ADR-013-emule-lowid-declaration.md) / W5.2 证据）；
5. NAT 类别覆盖**不再构成门禁条件**；`--census` 清单保留为描述性材料，未覆盖的类别写入
   已知限制，不得据此折算成矩阵覆盖或可达率；
6. 不承诺任何可达率数值（ADR-012）；结论由实验执行人依证据**手工回填**，脚本永不置 pass；
   `s6_status=not_run`、`g2_claim=false`、`evidence_valid_for_pass=false` 三个机器字段不变。

**声明口径**：本判据满足后，G2 只能写成「按 eMule 语义结案，带已知限制」，
**不得写成「NAT 矩阵通过」或「G2 达标」**——少测的类别是地图上标出来的空白，不是被划掉的格子。

### 与已接受 ADR 的关系

按 [ADR 索引 §4](README.md)「已接受的 ADR 不修改正文」：本 ADR **不改写 ADR-012 / ADR-013 正文**。

- ADR-012 的「取消一切数值承诺」结论**继续有效**，且本决策进一步消除按类别凑数字的动机；
- 本 ADR **取代** ADR-012「S6 现在的验收口径」第 1 条（5 类各有逐类报告）与
  `nat_matrix_config.json` 中 `required_classes` 作为 **pass 前提**的地位；其余三条
  （30 次观测、两路皆失败记限制、人工回填）原样保留。

## 后果

- **立刻解除的阻塞**：不再需要为了凑齐 4 类 NAT 而去采购 / 借用 ≥4 个真实出口；
  `--census` 从「能否宣称 G2」的关键路径移出，与 2026-11-16 的路线重排一致。
- **仍然欠的证据**（不因改判据而自动获得）：
  - `112.5.32.34`（symmetric / APDM）已有 W3.7 真实报告 `direct 0/30 + relay 30/30`，
    覆盖判据 3 的 LowID 一半；
  - `112.5.60.4`（EIM / port-restricted cone）**只有分类，没有逐类 30 次实测**，
    判据 3 的 HighID 一半仍缺；可用现有两台物理主机（A 作持有者、222 作请求者）补跑，
    **不需要新增出口**；
  - 声明与实测的一致性由 W5.2 的 21/21×3 承担，补跑时须同时记录声明查询结果。
- **一条都没放松**：真实设备 / 两台物理主机 / 30 次真实 `WAKEUP_ACK` / 不编数字 /
  不自动 pass / 机器状态不变。改的是「要证明什么」，不是「证据可以多糙」。
- **复审时点（必须）**：**M4 前**，或**当「≥4 个不同真实出口」且「≥3 个不同 NAT 类别」
  同时成立时**（两者先到为准），重新评审是否恢复完整类别矩阵。届时若仍维持本判据，
  须在本 ADR 追加复审记录并更新 `s6_stance_review_trigger`，不得默示延期。
  （原文字为「≥4 个不同真实出口」，2026-11-25 由所有者改为上面的双条件，原因见
  §复审记录 R1：单看出口数会被「同类主机」反复触发，结论每次都一样。）
  - **触发状态（2026-11-25，事实登记，非结论）**：出口数已达 4——
    `112.5.60.4`（port-restricted-cone）、`112.5.32.34`（symmetric）、
    `82.157.68.159` 与 `119.29.150.254`（两台云主机，云侧 1:1 映射）。
    按本条文字，复审条件**已成立**。
  - **类别更新（2026-11-25 15:38，事实登记）**：给广州那台 `119.29.150.254`
    放通入向 UDP 后，其类别由 port-restricted-cone 变为 **full-cone**
    （EIM + EIF 实测，含未放通主机的同探针对照）；北京那台未改，仍
    port-restricted-cone。类别覆盖因此为 **3/4**
    （full-cone ×1 / port-restricted-cone ×2 / symmetric ×1），
    仍缺 `restricted-cone` 与 `CGNAT`——这两个只能等真实环境，造不出来。
    注意该 full-cone 是「云侧 1:1 静态 NAT + 安全组放通」的组合行为，
    不是天然 full-cone 主机，不得据此声称矩阵通过。
    证据 `tests/ARCHIVE/evidence/s6-cloud-119-fullcone-20261125.json`。
    这两条只是登记事实；是否维持语义、是否改写触发条件，见 §复审记录 R1（已判定）。
- 对外表述统一为：**提供一条可行的路，失败时可解释、可感知**；不出现任何可达率、
  成功率或类别覆盖率承诺。

## 复审记录

### R1（2026-11-25）— 维持 eMule 式可达语义，触发条件改为「≥4 出口且 ≥3 类别」

**触发**：出口数达 4（`112.5.60.4`、`112.5.32.34`、`82.157.68.159`、`119.29.150.254`），
按原文字「≥4 个不同真实出口」条件成立；同日给广州那台放通入向 UDP 后类别由 2 类升到
3 类（`full-cone` ×1 / `port-restricted-cone` ×2 / `symmetric` ×1）。

**决定（所有者，2026-11-25）**：**维持 eMule 式可达语义，不恢复完整类别矩阵**；
复审触发条件改为「**≥4 个不同真实出口** 且 **≥3 个不同 NAT 类别** 同时成立」（先到为准）。
选项原文见 `docs/06-plan/G2-REVIEW-BRIEF-2026-11-25.md` 的三选一，本次取第 ① 项。

**理由**：原文字只看出口数量，而「加一台同类主机」就会让它反复成立、每次复审结论都相同，
等于把一次判定摊成无限次。改成双条件后，触发对应的是「环境可能出现新类别」这件事，
而不是「又买了一台机器」。

**本条判定不放松任何东西**：

- `class_coverage_required_for_pass` 仍 `false`，`matrix_complete_is_pass_condition` 仍 `false`；
- 真实设备 / 两台物理主机 / 每路径 30 次真实 `WAKEUP_ACK` / 不编数字 / 不自动 pass 一条未改；
- `s6_status` 仍 `not_run`，`g2_claim` / `evidence_valid_for_pass` / `auto_pass` 仍 `false`；
- 仍缺 `restricted-cone` 与 `CGNAT`，未覆盖类别继续登记为已知限制，不因本次复审变成已覆盖。

**本次复审没有被用来“延期”**：它是一次有结论的判定（维持），不是把问题推到 M4；
下一次复审只在**类别集合发生变化**（新增或丢失类别）、或 M4 前必须复评时发生。

**同步落点**（防文档与机器字段不一致）：

| 位置 | 值 |
|---|---|
| `tests/fixtures/g2-spike-verdicts.toml` → `[[review_trigger]] s6.semantics` | 已改为双条件 |
| `tests/spikes/s6/nat_matrix_config.json` → `s6_stance_review_trigger` | 已改为双条件 + R1 记录 |
| `docs/02-architecture/adr/README.md` 索引行的「复审时点」 | 已同步 |
| `docs/05-quality/RISK.md` R-04 段的「复审时点」 | 已同步 |
| `docs/06-plan/G0-REVIEW.md` | **不改**：那是 2026-11-17 的日期化记录，保留当时文字 |

### R2（2026-11-25）— `restricted-cone` 退出待测清单（所有者决定，非判据变更）

**性质**：这是**范围决定**，不是判定变更。S6 于 2026-11-17 已判 `pass(closed_with_exceptions)`，
`class_coverage_required_for_pass=false` 从 ADR-016 生效当天起就为 `false`，
因此**本轮决定的不是「能不能 pass」，而是「已知限制怎么写」**。

**决定（所有者，2026-11-25）**：不再等待/采集 `restricted-cone` 真实样本。

- 依据：现网真正的 ADF 已罕见（家用 CPE 基本是 full-cone 或 port-restricted）；
  在云主机上唯一可造 ADF 的办法是主机 `nftables` 自建「只存远端 IP」的动态集合，
  那是**人造类别**，与 `CLASSIFY-CONE-SUBTYPE-FABRICATED` 同类，不得登记为真实类别。
  详见 `docs/07-operations/TCP-DISCOVERY-RUNBOOK.md` §8.5。
- 后果（必须明写，不许含糊）：**类别覆盖的实际上限是 3/4**；`restricted-cone` 从今往后
  只会以「按决定排除」的身份出现，不再写「待补测」「等真实环境」。
  `CGNAT` 不随本决定退出——它仍未覆盖，仍登记为已知限制。

**本决定不改变的东西**（逐条，便于审计）：

- S6 的 `status` / `verdict_kind` / `claim_wording` 一个字未改；
- `class_coverage_required_for_pass` 仍 `false`、`matrix_complete_is_pass_condition` 仍 `false`；
- `s6_status=not_run` / `g2_claim=false` / `evidence_valid_for_pass=false` / `auto_pass=false` 四个机器常量不动；
- 不新增任何类别覆盖，不把「不测」写成「已覆盖」，不把「不测」写成「判据达成」；
- 禁止措辞：`restricted-cone 已覆盖`、`4/4 矩阵完成`、`NAT 矩阵通过`、`因此 S6 才 pass`。

**与 R1 的关系**：R1 判定「维持 eMule 式可达语义、不恢复完整类别矩阵」，R2 是其后果的
执行细则（把「等真实环境」改成「明确排除」）。R1 的双条件触发不变；若将来真的出现
ADF 出口，属于「类别集合发生变化」，按 R1 触发复审，本决定随即可被推翻。

## 相关文档

- [ADR-012 R-04 改为尽力](ADR-012-r04-best-effort.md) · [ADR-013 eMule LowID 自我声明](ADR-013-emule-lowid-declaration.md)
- [nat-matrix.md §19 / §21](../../../tests/spikes/s6/nat-matrix.md) · [nat_matrix_config.json](../../../tests/spikes/s6/nat_matrix_config.json)
- [spikes.json](../../../tests/spikes/spikes.json) · [G0-REVIEW](../../06-plan/G0-REVIEW.md) · [ROADMAP](../../06-plan/ROADMAP.md) · [TEST](../../05-quality/TEST.md)
