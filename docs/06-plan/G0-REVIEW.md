---
doc_id: DOC-22
title: G0 技术路线冻结评审记录
category: 计划跟踪
version: 1.2
status: 已接受
owner: 项目所有者
updated: 2026-11-13
---

# G0 技术路线冻结评审记录

> 编号 DOC-22 · 版本 1.2 · 状态 **已接受** · 责任人 项目所有者 · 更新 2026-11-13
> 机读状态见 [`tests/spikes/spikes.json`](../../tests/spikes/spikes.json) 的 `g0_review` 字段；
> 判据口径见 [spikes README §6](../../tests/spikes/README.md)，里程碑见 [ROADMAP §6](ROADMAP.md)。

## 1. 评审结论

**G0 判「条件通过」（conditional pass），技术路线自 2026-11-13 起条件冻结。**

项目所有者于 2026-11-13 显式作出两项治理决策：

| 决策项 | 结论 | 依据 |
| :--- | :--- | :--- |
| [ADR-001](../02-architecture/adr/ADR-001-kad-network.md) 默认模式签署 | **`public`**（选项 C 定稿：`bootstrap_mode` 默认 public，保留 `private` / `hybrid` 切换） | `S2.status = pass`：实网 `PUBLISH_RES load=1` → `SEARCH_RES` 命中，两进程 A/B 跨节点取回 |
| G0 冻结口径 | **条件冻结**：豁免 S3 / S5 / S6 在 G0 时点给出最终结论，M1/M2 必须实测回填 | 三项的量化判据按设计本就后置到 M1/M2，本轮已补真实量测/先导样本与可复现 harness |

条件冻结**不等于**八项 Spike 全部 `pass`：冻结时点 `spikes.json` 中 S3 / S5 / S6 状态仍为 `not_run`，不虚判、不回填。
（2026-11-17 三项已由所有者依证据判定：S3 `pass` / S5 `fallback` / S6 `pass`（`closed_with_exceptions`），
见 §2 表与[判定记录](G2-SPIKE-VERDICT-2026-11-17.md)。）
先例是 S8——2026-11-09 由项目所有者豁免登记 `pass`（无书面法务意见，R-11 保留，M4 前复核）。

## 2. 八项 Spike 结论矩阵（冻结时点）

| Spike | 状态 | 本轮证据 | 残余要求 |
| :--- | :--- | :--- | :--- |
| S1 BT 引擎能力 | `pass` | two-peer 4/4 verified；**Motrix Next(aria2) 第三方 seeder → rqbit 128/128 片 SHA-1 校验通过** | G3 兼容性矩阵收录 |
| S2 公共 KAD 命名空间 | `pass` | 实网 `PUBLISH_RES` / `SEARCH_RES` 命中 + 双进程 A/B 取回 | R-02 按运营速率的污染判定，M4 前复审 |
| S3 双栈资源可控 | **`pass`**（2026-11-17 判定） | 结构层 C1 verified；真实 UDP 双栈 I/O 0 串扰；M1 跨主机 17/17；**2026-11-17 公网 Mainline DHT 测试 key 宣告/取回闭合 + 跨进程 RSS 合计峰值 25.04 MiB** | 不含 `cr-dht` 实现（仍是骨架）；公开只能说「双栈资源可控」 |
| S4 影子索引 ≤100 MB | `pass` | 1 万文件 ≈7.79 MiB；5 万 ≈15.9 MiB；20 万 ≈53 MiB | M1 生产 profiling 复核 |
| S5 uTP/LEDBAT 退让 | **`fallback`**（2026-11-17 判定） | 令牌桶硬上限已实测；librqbit-utp 0.7.0 仅 CUBIC、无 LEDBAT；**2026-11-17 真实内核 tc/netem 链路：默认 ratio=0.02 前台劣化 0.00%（3/3），ratio=0.2 中位 20% 不达标** | NFR-03 由令牌桶 + 默认 0.02 满足，**不是**协议退让；默认值不得回调；R-07 维持 🟠 |
| S6 NAT 可达率 | **`pass`**（`closed_with_exceptions`，2026-11-17 判定） | **按 ADR-016 两半真实证据已齐**：HighID direct 3/30 + relay 30/30；LowID direct 0/30 + relay 30/30；aggregate `OBSERVED` / `complete=true` | 只能写「按 eMule 语义结案，带已知限制」；full-cone/restricted-cone/CGNAT 未测；不得写矩阵通过或 G2 达标 |
| S7 4 GB 造种开销 | `pass` | 4.14 s / 989 MB/s / 峰值 RSS 29.3 MiB / Idle 优先级 | M1 生产实现复核 |
| S8 法务评审 | `pass`（所有者豁免） | 无书面法律意见 | R-11 保留，M4 公测前必须复核 |

> **2026-11-16 口径变更（不改写本表结论）**：上表「S6 NAT 可达率」的待验项原为
> 「M2 四类真实 NAT（含 CGNAT）实测可达率」。项目所有者决定取消可达率的数值承诺
> （[ADR-012](../02-architecture/adr/ADR-012-r04-best-effort.md)），待验项改为
> 「逐类查清直连/中继的实际行为」。`S6.status` 仍 `not_run`，本表的时点结论与豁免范围不变。

> **2026-11-17 第二次口径变更（不改写本表结论、不改写 11-16 那条）**：项目所有者决定
> [ADR-016](../02-architecture/adr/ADR-016-s6-emule-like-reach.md) —— S6 阶段性验收改为
> **eMule 式可达语义**：不再穷举「4 类 NAT + CGNAT 各有逐类报告」，类别覆盖降级为描述性清单、
> 不再是 pass 前提；改为要求真实设备上至少一份 `direct` 可达（HighID）与至少一份
> `direct` 失败但 `relay` 送达（LowID）的真实证据，且节点自报声明与实测一致（ADR-013）。
> **§3 的边界一条不松**：条款①两台不同物理主机、条款②真实 NAT 设备、每条路径 ≥30 次真实
> `WAKEUP_ACK`、无数值承诺、脚本不自动 pass、结论人工回填；`S3/S5/S6.status` 仍全部
> `not_run`（该句写于判据变更时点；**2026-11-17 判定后为 `pass` / `fallback` / `pass`**，
> 见 §2 与[判定记录](G2-SPIKE-VERDICT-2026-11-17.md)，`g2_claim` 仍为 `false`），退回条款继续适用。
> 满足新判据后 G2 只能写
> 「按 eMule 语义结案，带已知限制」，不得写「NAT 矩阵通过 / G2 达标」。
> **复审时点**：M4 前，或当 ≥4 个不同真实出口可用时（先到为准）。
> 机读留痕：`spikes.json` → `g0_review.s6_criteria_amendment_2026_11_17` 与现行 `S6.criteria`；
> 方法见 [nat-matrix.md §23](../../tests/spikes/s6/nat-matrix.md)。

## 3. 条件冻结的边界与退回条款

**豁免的只是时点要求，不是判据本身。**

1. S3 / S5 / S6 的 `status` 保持 `not_run`；`spikes.json` 中「八项结论齐备」的机读口径不得被脚本或文档改写。
   **2026-11-17 例外（所有者判定）**：三项已依证据手工回填为 `pass` / `fallback` / `pass`
   （记录 `tests/fixtures/g2-spike-verdicts.toml`）。本条的「不得由**脚本或文档**改写」仍然有效——
   改写者是所有者本人，且 `g2_claim` 仍为 `false`。
2. 三项的 M1/M2 必测判据、证据路径与验收方式登记在 `spikes.json` 的 `g0_review.conditional_freeze.must_validate`，
   并由 [ROADMAP §6](ROADMAP.md) 的 G0 缺口清单逐项跟踪。
3. **退回条款**：任一 M1/M2 实测不达标时，按各 Spike 的 fallback 链降级——
   S3 收敛单栈 + 外部映射、S5 降低 `background_upload_ratio` / 连接数 / 暂停造种队列、S6 提升中继回退优先级并默认 LowID——
   并**重新提交技术路线冻结评审**。M1 之后不得继续以豁免状态代替结论。
4. 本冻结**不授权**向公共 KAD 批量发布测试 Key、不授权放宽 S2 实网实验的限速与最小样本约束、
   不替代 S8 的书面法务意见。

## 4. ADR 状态

| ADR | 冻结时点状态 | 说明 |
| :--- | :--- | :--- |
| [ADR-001](../02-architecture/adr/ADR-001-kad-network.md) | **已接受**（1.2，2026-11-13 所有者签署 `public`） | 选项 C 定稿；R-02 的 M4 复审条款保留 |
| [ADR-002](../02-architecture/adr/ADR-002-bt-engine.md) | 已接受（1.3） | 选项 A：采用 rqbit，握手拦截自实现于 `cr-engine` 外围 |
| [ADR-005](../02-architecture/adr/ADR-005-incentive.md) | 待定 | 待 M4，与本冻结无关 |
| [ADR-011](../02-architecture/adr/ADR-011-ed2k-interop.md) | 已接受 | eD2k 互操作边界，L1/L2 探针层已落地 |

## 5. 本轮新增的真实量测与风险含义

| 项 | 实测结果 | 风险含义 |
| :--- | :--- | :--- |
| S3 双栈 UDP I/O | 1437/1437 跨命名空间畸形帧被拒绝、**0 误路由 / 0 丢包**，总带宽 3.28 Mbit/s ≈ 两栈之和，RSS Δ0.93 MiB | R-03 的「带宽增量」从估算升级为实测；仍非真实 DHT 加入 |
| S5 QoS | 后台上行被硬卡在 ≤20.25% 链路（令牌桶有效）；**但默认 20% 配置前台劣化 20.12%**，退让模型 15.0%；ratio=0.05 → 6.19%；**ratio=0.02 → 4.03%** | **R-07 未完全去险**：M1 默认值不得沿用 0.2 即声称满足 NFR-03，须按实测回调并启用 fallback 链 |
| S6 先导实测 | 本机 `192.168.1.210` → 映射 `112.5.60.4:36764`，跨 4 台 STUN（2 个目的 IP）映射不变 = cone 类；公网 UDP 可达（RTT≈245 ms）；**UPnP IGD 与 NAT-PMP 均不可用**；二跳 `112.5.60.1` 为公网，未发现 CGNAT | R-04 不能按「UPnP 可用」假设估计；穿透栈实际依赖 UDP 打洞与中继/LowID 回退，M2 必须按真实多 NAT 复核 |
| 工程门禁 | 第三方 BT 客户端检查由 `MISSING` 修正为 `OK`（Motrix Next / aria2，S1 互通证据驱动）；preflight 阻塞项 **3 → 2** | 剩余 2 项均为 M1 工程接线（M1 二进制、CliAdapter），不属 G0 判据 |

## 6. 工程门禁（不因冻结而消失）

`m0_preflight.py` 在 2026-11-13 仍报 `G0 就绪: False`、`T0 真实验收就绪: False`，阻塞项 2 个：

1. `binaries.m1`：M1 二进制与下载器环境变量未指向真实文件；
2. `t0.wired`：`CliAdapter.wired` 必须为 `False`，直到 M1 二进制与观测齐备。

**在 G0 条件冻结生效后，方可按 [src/README.md](../../src/README.md) 的门禁创建 `Cargo.toml` 与 crate 骨架**；
但 `--mode real` 在上述两项接线完成前必须继续报 `BLOCKED`，由 CI（`.github/workflows/t0.yml`）强制校验。
`g0_pending_spikes = S3,S5,S6` 是治理豁免项，不是脚本可自动清除的阻塞项。

## 7. 复核时点

| 时点 | 必须完成 |
| :--- | :--- |
| M1 出口 | S3 真实双栈集成 + G3 矩阵；S5 真实拥塞链路 uTP 实测；`Cargo.toml` / crate 骨架按门禁落地；重跑 preflight |
| M2 | S6 四类 NAT（含 CGNAT）可达率实测；按结果决定 R-04 是否触发中继加码 |
| M4 公测前 | S2 / R-02 按运营速率的索引层拒绝率复审；S8 / R-11 补书面法务意见 |

## 8. 证据路径

- 机读决策：[`tests/spikes/spikes.json`](../../tests/spikes/spikes.json) → `g0_review`、`status_note`、S3/S5/S6 `observations`、**`m1_progress` / `g0_review.m1_engineering_wiring`**
- 实验框架：[spikes README](../../tests/spikes/README.md) · [S3 README](../../tests/spikes/s3/README.md) ·
  [S5 README](../../tests/spikes/s5/README.md) · [S6 矩阵](../../tests/spikes/s6/nat-matrix.md)
- Harness：`tests/spikes/s3/dualstack_io.py` · `tests/spikes/s5/qos_bench.py` · `tests/spikes/s6/stun_probe.py`
- 就绪度报告：`tests/spikes/reports/m0-preflight-*.json`（本地产物，不入库）

## 9. M1 开工后的复核补记（2026-11-13，v1.1）

**条件冻结的边界未被突破**：本轮只解除 §6 的两项**工程接线**阻塞，不改八项 Spike 结论矩阵，
不把 §3 的豁免改成结论，也不改 `m0_preflight.py` 的 `g0_ready` 算法。

| 项 | 冻结时点（§6） | M1 开工后 |
| :--- | :--- | :--- |
| `binaries.m1` | MISSING | 已解除：`cargo build` 产出 3 个二进制；real 运行前须显式设置三个环境变量 |
| `t0.wired` | MISSING（`False`） | 代码层已接线（`True`）；**环境级防呆保留**，缺二进制时 real 仍 `BLOCKED` / 退出码 2 |
| `g0_pending_spikes` | S3 / S5 / S6 | **不变**，仍 `not_run`（各补 `m1_2026_11_13` 证据条目，`evidence_valid_for_pass=false`） |
| T0 real | 不可执行 | **21/21 PASS、`evidence_valid=true`**（仅 T0 单机回环，报告见 ROADMAP §6） |

> **【当前状态更正 · 2026-11-14】以下 §9–§10 是 2026-11-13 评审时点的历史口径，不是现状。**
> G1 已于 2026-11-14 通过，M2 已解除门禁并开工；现状见本文 **§11**。历史条目按决策演化链保留。

**G1 与 M2 状态（2026-11-13 评审时点）**：按 [ROADMAP §2](ROADMAP.md)，G1 要求 T0 **/ T1** 全链路闭环，M2 依赖 M1。
T1（局域网双机 + 真实 NAT）未执行，S3 真实双栈集成 + G3 矩阵、S5 真实拥塞链路 uTP 实测
均未完成，S6 四类 NAT 实测属 M2 必测。因此：

- **G1 未通过**，`T0 evidence_valid=true` 不得外推为 G1 证据；
- **M2 保持 `blocked_by_G1`**，本轮只做了门禁允许的 crate 骨架、S5 配置默认值回调
  （`0.2 → 0.02`，[CONFIG](../04-protocol/CONFIG.md) 2.1）与 S6 矩阵登记；
- 任一 M1/M2 必测不达标仍按 §3 第 3 条退回条款降级并重新提交冻结评审。

机读口径见 `spikes.json` 的 `m1_progress`（含 `g1_remaining` 与 `m2_gate`）。

## 10. T1 与 S5 必测回填补记（2026-11-13，v1.2）

**§3 的必测回填仍未结案，条件冻结的边界未被突破**：S3 / S5 / S6 的 `status` 全部保持
`not_run`，本轮只补工程与量测证据，`evidence_valid_for_pass` 一律为 `false`。

| 必测项 | 本轮进展 | 是否结案 |
| :--- | :--- | :--- |
| T1 双机 + 真实 NAT 全链路 | `tests/e2e/t1/` harness 落地（10 用例 / 40 断言）、M1 补齐按节点 host 寻址与 `/health` 机器身份；单机预检两模式各 10 PASS / 1 SKIP，唤醒 P95 2.0 ms、复活 30/30 | ❌ `--mode real` 因只有一台物理主机 `BLOCKED`，`evidence_valid=false` |
| S3 真实双栈 + G3 矩阵 | 未新增；M1 仍只有本地私有子网 UDP KAD | ❌ |
| S5 真实拥塞链路 uTP ≤5% | 真实 `librqbit-utp 0.7.0` 协议栈量测完成；`ratio=0.02` 前台劣化 2.70% | ❌ 瓶颈为本机用户态整形，非 `tc/netem` / 跨主机链路 |
| S6 四类 NAT 可达率 | 未新增（M2 必测） | ❌ |

两条影响后续决策的新事实：

1. **T1 不是「找两台机器就能跑」**：M1 CLI MVP 没有 UPnP IGD / NAT-PMP / PCP / STUN 打洞 / 中继，
   且 S6 先导样本显示本机 UPnP 与 NAT-PMP 均不可用。跨 NAT 双机时 UDP 唤醒与 tracker 返回的私网
   peer 地址都不可达。G1 前须补穿透栈，或把 T1 边界明确限定为「同一 NAT 内两机」并重新评审。
2. **S5 的次级退让假设被源码否证**：`librqbit-utp 0.7.0` 只有 CUBIC 拥塞控制器，
   `src/lib.rs` 留有 `// TODO: LEDBAT congestion control`；真实栈实测前台劣化与后台链路占比近似
   1:1，无退让回收。按 §3 第 3 条，这属于「必测不达标」的口径变化：NFR-03 ≤5% 目前**只能依赖
   `background_upload_ratio=0.02` 的令牌桶降级**，不得记为「uTP/LEDBAT 达标」；R-07 风险口径已上调。
   若坚持保留 uTP 退让路线，须换实现或自行实现 LEDBAT 并重新提交冻结评审。

**G1 / M2 结论（2026-11-13 时点）**：G1 未通过，M2 保持 `blocked_by_G1`。机读见 `spikes.json` 的
`t1_progress`、`m1_progress.g1_remaining`、`g0_review.t1_and_s5_2026_11_13`。
**该结论已于 2026-11-14 被 T1 `--mode real` 11/11 的证据取代，见 §11。**

## 11. G1 通过 / M2 开工（2026-11-14，v1.3）

| 门禁 | 结论 | 证据 |
| :--- | :--- | :--- |
| G1 | ✅ **通过** | T0（2026-11-13 / 2026-11-14 复跑均 21/21、`evidence_valid=true`）+ T1 `--mode real` 双机 11/11、`evidence_valid=true`（2026-11-14，A=192.168.1.210 / B=192.168.1.209） |
| M2 | 🔓 **门禁解除、已开工** | `spikes.json` 的 `m1_progress.m2_gate` 由 `blocked_by_G1` 改为 unblocked；W3.3 cr-nat 接入复活路径已落地 |
| G2 | ❌ **未达标、未宣称** | NFR-01/02/03 与 NAT 矩阵尚未在 M2 收口；S3 / S5 / S6 仍 `not_run` |

**M2/W3.3 接线范围（本轮）**：`cr-core` 增加可选 `nat` 配置段（缺省时 T0/T1 行为不变）；
`cr-nat` 新增 `endpoint.rs`（候选计划 / tracker peer 决策，纯计算）与 `holepunch.rs`
（打洞 + 唤醒复用同一 UDP socket、中继帧回退）；`coldrevived` 新增 `nat_path.rs` 与 `nat` CLI、
`/nat/advertise`、`/announce` 私网/公网分流；`coldrevived/src/bt.rs` 支持空 peer 列表应答。
**PROTOCOL_VERSION=1、T0 golden 字段、CLI v1 既有命令返回值全部未变。**

**证据**：[T2 本地接线验收](../../tests/e2e/t2/README.md) `10/10 PASS`（W3.3 时为 mock STUN / mock 中继桥 +
真实 UDP/TCP/HTTP/CLI 观测，`evidence_valid=false`）；T0 `--mode real` 回归 21/21；
T1 `--self-test` 11/11；`cargo test --workspace --offline` 74/74（W3.3 时点）。

> **【W3.4 补记 · 2026-11-14 / v2.4.49】**：上述数字是 W3.3 时点。**M2/W3.4 已落地**
> `cr-nat::relay_server` 纯帧路由 + 独立 `coldrevive-relay` 二进制 +
> `coldrevived/src/relay_sub.rs` 持有者订阅；T2 的 mock 中继桥已换成**真实中继服务端**
> （`--mode local` 仍 10/10，T2-07 实测 `delivered=2 / dropped=0`），
> `cargo test --workspace --offline` **80/80**，T0 `--mode real` 回归仍 21/21、
> T1 `--self-test` 11/11。**真实中继服务端 ≠ 真实 NAT 环境**，新增
> `S6.observations.w34_relay_2026_11_14` 的 `evidence_valid_for_pass=false`、
> `matrix_coverage=0/4`、`s6_status_after=not_run`；**S3 / S5 / S6 状态仍全部 `not_run`**，
> 下方「条件冻结边界仍未被突破」的结论原样有效。

> **【W3.5① 补记 · 2026-11-14 / v2.4.50】**：按「先审计测试条件、只跑满足条件的实验」推进。
> [条件探针](../../tests/spikes/probe_conditions.py) 判定 S3 / S5 / S6 均为 `PARTIAL_RUNNABLE`
> （可跑附件实验、不可判 pass）：无管理员权限、无 `tc/netem`、WSL 被安全策略禁止、无全局 IPv6、
> 虚拟交换出口不可达、仅 1 类真实 NAT 且无 CGNAT。
> 本轮新增真实附件证据：S3 [双机双栈集成](../../tests/spikes/s3/m1_dualstack_integration.py) **17/17 PASS**；
> S5 真实 `librqbit-utp` 5 轮复跑（默认 `0.2` 档 14.78% 不达标、`0.02` 档 3.23%）；
> S6 [单出口精细分类](../../tests/spikes/s6/nat_behavior.py) 为 port-restricted cone（medium）
> 及 [四类 NAT 矩阵执行协议](../../tests/spikes/s6/nat_matrix.py) 干跑 `BLOCKED`。
> **全部 `evidence_valid_for_pass=false`；S3 / S5 / S6 仍 `not_run`、`matrix_coverage=0/4`、
> `g2_claim=false`、`spike_status_changed=false`。** 分类样本不等于可达率，
> 条件冻结的必测回填仍未结案，退回条款仍适用。机读见 `spikes.json` 的
> `g0_review.w35_experiments_2026_11_14` 与 `m2_progress.scope_done.w35_*`。

**§3 条件冻结的三项必测已由 2026-11-17 判定关闭**：S3 = `pass`、S5 = `fallback`、S6 = `pass`（`closed_with_exceptions`，
口径「按 eMule 语义结案，带已知限制」）。判定见[判定记录](G2-SPIKE-VERDICT-2026-11-17.md)与
`tests/fixtures/g2-spike-verdicts.toml`。判定**不改写**任何实测报告的
`evidence_valid_for_pass=false` / `auto_pass=false`，也不改机器常量
（`census.s6_status=not_run`、`g2_claim=false`）——那些字段的含义是「工具不替人下结论」。

依据：S6 的 eMule 式两半证据已齐（`s6-matrix-aggregate-20261117-highid.json`：`status=OBSERVED`、
`reach_evidence.complete=true`；HighID direct 3/30 + relay 30/30，LowID direct 0/30 + relay 30/30）；
S3 已补公网 Mainline DHT 测试 key 宣告/取回与跨进程 RSS（25.04 MiB 合计峰值）；
S5 已补真实内核 `tc htb + netem` 跨 netns 链路，默认 ratio=0.02 前台劣化 0.00%（3/3 轮）。

**剩余（G2 出口）**：① M2 其余骨架 crate 的引擎与资源治理（含 `cr-dht` 真实实现，S3 判定不含它）；
② LRU 冷热调度器接引擎（现仍走 M1 `bt_seeding` 门闩）；③ 中继生产化收尾（公网部署 / 鉴权 / 限流 / 跨公网时延）；
④ BT/TCP 打洞；⑤ M4 前 R-02 拒绝率复审与 R-11 书面法务意见。
**三项 Spike 判定只回答「技术路线是否成立」，不构成 G2 达标**；`g2_claim` 仍为 `false`。

## 12. G0 `protocol_scope` 书面放宽（2026-11-22，项目所有者决定）

项目所有者在本会话中明确指令「解除」，据此把 G0 `protocol_scope` 对**公共 KAD `0x33`/`0x43` 索引操作**的 BLOCKED 登记解除。机读留痕见
[`spikes.json`](../../tests/spikes/spikes.json) → `g0_review.protocol_scope_relaxation_2026_11_22`。

- **放宽范围**：允许对真实公共节点执行 `SEARCH_KEY_REQ` / `PUBLISH_KEY_REQ` 并读取响应，
  允许查询公共 eMule 关键词词典。v2.4.22 的源码核对已证伪「必须先 HELLO 互认」（H1），
  所以本决定**不要求**实现 `HELLO` / sender/receiver UDP key / RC4，也不是要改帧格式。
- **不随放宽消失的约束**：沿用 0.2 包/秒限速与最小样本口径；禁止批量发布测试 Key；
  发布仍以真实内容和真实 fileID 为前提；R-02 / R-11 仍是 M4 前未结风险；
  运行期结论只来自真实 UDP / 文件 / CLI 观测，脚本不得自动 pass。
- **门禁不变**：`g2_claim=false`、`s6_status=not_run`、`evidence_valid_for_pass=false`、`auto_pass=false`。
  FR-24 公网检索的实现状态与证据仍须按真实公网节点重测后登记，**LAN 结果不得外推**。
- 历史字段 `S2.g0.confirmed.protocol_scope` 原文不改；本条只覆盖其后的行政 BLOCKED 口径。
