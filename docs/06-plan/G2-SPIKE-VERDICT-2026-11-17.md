---
doc_id: DOC-23
title: S3 / S5 / S6 判据判定记录（G2 前置 Spike）
category: 计划
version: 1.0
status: 已定稿
owner: 项目所有者（判定） / 架构组（记录）
updated: 2026-11-17
---

# S3 / S5 / S6 判据判定记录（G2 前置 Spike）

> 编号 DOC-23 · 版本 1.0 · 状态 **已定稿** · 判定人 项目所有者 · 记录 架构组 · 更新 2026-11-17
> 本文件是**人的判定**，不是工具的推断。理由与 [g3-closeout.toml](../../tests/fixtures/g3-closeout.toml) 开头那段相同：
> 证据是证据，够不够是人的判断。机器字段（`census.s6_status`、报告里的 `g2_claim` /
> `evidence_valid_for_pass`）**不由本文件改写**，脚本也永远不会读本文件去置 pass。

## 1. 判定总表

| Spike | 判定 | 一句话理由 | 这份判定**不**代表 |
| :--- | :--- | :--- | :--- |
| S3 双栈资源可控 | **`pass`** | 串扰 0、资源增量实测在 NFR-05 内、公网 Mainline 测试 key 已闭环 | 不代表 `cr-dht` 已实现；不代表双栈 DHT 已在产品内跑通 |
| S5 uTP/LEDBAT 退让 | **`fallback`** | 主机制被否证（`librqbit-utp 0.7.0` 只有 CUBIC）；NFR-03 由令牌桶 + 默认 `ratio=0.02` 在真实内核链路达成 | 不代表 uTP 会退让；不代表 0.05 / 0.2 可用 |
| S6 家用 NAT 可达 | **`pass`**（`closed_with_exceptions`） | 按 ADR-016 的 eMule 式语义，HighID 与 LowID 两半真实证据齐 | 不代表 NAT 矩阵通过；不代表 G2 达标；不含任何可达率数字 |

三条判定都是**结论**，不是**乐观估计**。三条各自都附了「禁止表述」，写在
[`tests/fixtures/g2-spike-verdicts.toml`](../../tests/fixtures/g2-spike-verdicts.toml) 里，机器可读。

---

## 2. S3：`pass`（限定范围）

### 2.1 判据（`spikes.json` → `S3.criteria`，未改）

1. 双栈并行压测无 ID 空间串扰；
2. 内存与带宽增量在可接受范围。

### 2.2 实测

| 项 | 观测 | 证据 |
| :--- | :--- | :--- |
| C1 结构层 | `anomalies_total=0`、`leak_total=0`，10k/50k 两栈成员关系完全不相交，负查询全 miss | `tests/spikes/s3/reports/s3-stress-*.json` |
| C1 线级 | 跨命名空间注入畸形帧 **1437/1437 被拒**，`misrouted_frames=0`、`loss_pct=0.0` | `s3-dualstack-io-20261113T062159Z.json` |
| C1 跨主机 | A / B 两台物理主机真实 UDP，**17/17**，KAD 流 60/60 命中、DHT 流 0/60 命中 | `s3-m1-dualstack-w35.json` |
| C2 内存 | 双索引 10k = 2.26 MiB、50k = 11.08 MiB（结构上界）；**跨进程峰值合计 25.04 MiB**（12.38 + 12.66），Private 合计 7.96 MiB | 结构层报告 + `s3-mainline-public-20261117.json` 的 RSS 采样 |
| C2 带宽 | 双栈合计 3.276 Mbit/s ≈ 两栈之和（各 1.638），@800 pps/栈，无异常放大 | 同上线级报告 |
| 公网加入 | announce 进程路由表 76 → **82** 节点；独立 query 进程（不同 node id、不宣告）取回 `112.5.60.4:18081`，`expected_ip_hit=true` | `s3-mainline-announce-20261117-v2.json` / `s3-mainline-query-20261117-v2.json` |

补充事实：公网宣告用的 infohash 是确定性测试 key `f273a2971086f726e1ddb03fc8480c5121f846fa`，
**没有宣告任何产品资源**，这是所有者 2026-11-17 显式授权的范围。

### 2.3 判定

**`pass`。** 两条判据都有真实观测直接支撑：串扰为 0 且三层（结构 / 线级 / 跨主机）一致，
资源增量远低于 NFR-05 的 100 MB 且带宽呈加法关系。原先挡在 S3 前面的三项：
① G3 兼容矩阵 → 2026-11-16 已由所有者按 `closed_with_exceptions` 结案（见 g3-closeout.toml）；
② 公网 Mainline 加入 → 本轮闭环；③ 跨进程 RSS → 本轮取得 25.04 MiB。

### 2.4 边界（必须与判定一起引用）

- **`cr-dht` 仍是骨架**（无 KRPC / BEP5 实现）。S3 回答的是「双栈并存时资源可不可控」，
  不是「DHT 能不能用」；后者是 M2 实现项，见 §5。
- KAD 128-bit 侧的公网行为由 **S2**（公共 KAD 命名空间，`pass`）承担，本轮没有重复做 KAD 公网宣告。
- G3 是**带例外**结案，不是全绿；S3 借用的是这个人工结论，不重新解释它。
- 不得由 S3 的 `pass` 推出任何内存或带宽承诺数字；25.04 MiB / 11.08 MiB 是观测，不是 SLA。

---

## 3. S5：`fallback`

### 3.1 判据与实际目标

Spike 标题是「uTP / LEDBAT 后台退让满足 NFR-03」；NFR-03 的两半是
**后台上行 ≤ 配置阈值（默认总上行 20%）** 与 **前台吞吐劣化 ≤ 5%**。

### 3.2 实测（真实内核链路）

环境：221 上跨 netns veth，内核 `htb 16mbit` + `netem delay 20ms 2ms`，
对照 TCP 为 iperf3 BBR，后台为真实 `librqbit-utp 0.7.0` 双进程；
每轮一条连续 iperf3 会话，同一会话内比较 baseline 5 s / concurrent 10 s / recovery 5 s。

| `background_upload_ratio` | 后台实际占比 | 前台劣化（3 轮） | 判定 |
| :--- | :--- | :--- | :--- |
| **0.02（现行默认）** | 0.0199 | **0.00 / -0.01 / 0.00 %** | 达标 |
| 0.05 | 0.0496 | **19.99** / 0.00 / 0.01 % | 不稳定，不达标 |
| 0.20 | 0.1252（未打满 3.2 Mbit 配额） | -0.01 / 20.00 / 20.00 % | 明确不达标 |

`librqbit-utp 0.7.0` 源码只有 `Cubic`，留着 `// TODO: LEDBAT congestion control`——
**「uTP 自动退让」这个机制不存在**，此前 14.82% / 4.31% / 2.70% 的用户态结果也与此一致：
劣化与后台占比近似 1:1，没有回收。

### 3.3 判定

**`fallback`。** 不是 `pass`：Spike 标题主张的机制（uTP/LEDBAT 退让）为假，写成 `pass`
等于把「令牌桶限流」冒充成「协议退让」。也不是 `fail`：NFR-03 的两半分都在**默认配置**下
于真实内核整形链路上成立，走的是 `spikes.json` 早就登记过的 fallback
「强化令牌桶与连接数限制」。

R-07 的等级**维持 🟠**：判据达成依赖一个配置不变量（默认 `0.02`），而不是一个协议能力。

### 3.4 边界

- `0.05` 档 1/3 轮出现 19.99%，与后台实际只占 0.049（0.79 Mbit/s）不相称，
  **至今没有解释**（怀疑 iperf3 区间跨相边界，未证实）。不得据此说 0.05 可用，也不得当成 5% 证据。
- 只在 16 Mbit / 20 ms 一种链路形状上测过；不同带宽 / RTT 下阈值可能不同。
- 证据全部来自 221 单机的两个 netns，**不是跨物理主机**；跨主机链路仍未测。
- 不得回收默认值到 `0.05` / `0.2` 再声称满足 NFR-03；改默认值须重跑本 harness 并留报告。

---

## 4. S6：`pass`（按 ADR-016 的 eMule 式语义，带已知限制）

### 4.1 判据

[ADR-016](../02-architecture/adr/ADR-016-s6-emule-like-reach.md) §决策 的 6 条，核心是第 2、3 条：
两路径各 ≥30 次真实 `WAKEUP_ACK`，且 HighID 与 LowID 两半各有真实证据。

### 4.2 实测

| 拓扑 | 出口类别 | direct | relay | 说明 |
| :--- | :--- | :--- | :--- | :--- |
| A 持有 / 222 请求 | `port-restricted-cone`（112.5.60.4） | **3/30**（命中 `112.5.60.4:47488`，`ack_status=OK`） | **30/30** | HighID 语义 |
| 反向（W3.7） | `symmetric`（112.5.32.34） | 0/30 | **30/30** | LowID 语义 |

`--aggregate` 结果：`status=OBSERVED`、`evidence_valid=true`、`rejected_reports=[]`、
`reach_evidence.complete=true`（`s6-matrix-aggregate-20261117-highid.json`）。
载体错误 0；持有者映射端口 47488 ≠ 内部 17801，满足 `nat_is_real_device` 可证伪条件；
两端分处 A / 222 两台不同物理主机。

### 4.3 判定

**`pass`，结案口径 `closed_with_exceptions`。** 对外只能写
「**按 eMule 语义结案，带已知限制**」。

DIRECT 3/30 是**描述性观测**：它证明「这条路存在且能走通」，不证明「30 次里有 10% 能走通」；
ADR-012 已取消一切可达率承诺，这条数字不得进入任何对外材料。

### 4.4 已知限制（结案时必须一起写）

- `full-cone` / `restricted-cone` / `cgnat` 三类**未测**，是地图上的空白，不是被划掉的格子；
- symmetric 请求者打不通 direct 是 NAT 行为（STUN 映射与发往持有者的映射端口不同），**不是缺陷，不许修**；
- 声明与实测的一致性由 W5.2 的 21/21×3 承担，本轮报告同时记录了唤醒前的声明查询结果；
- 不得写「NAT 矩阵通过」「G2 达标」「可达率 xx%」。

### 4.5 机器字段不变

`census` 的 `s6_status=not_run`、报告里的 `g2_claim=false` / `evidence_valid_for_pass=false`
**保持原值**。含义是「工具不替人下结论」，与本文件的人工判定互不覆盖——与 G3 的
`g3_status` vs `g3_status_declared` 是同一套办法。

---

## 5. 这些判定改变了什么，没改变什么

**改变了**：`spikes.json` 中 S3 / S5 / S6 的 `status`（`not_run` → `pass` / `fallback` / `pass`），
以及沿用了两周的「三项必测全部未回填」这一阻塞描述。

**没有改变**：

- **G2 仍未达标，`g2_claim=false`。** G2 是 M2 出口，除三项 Spike 外还欠实现项：
  `cr-dht` 等骨架 crate 的内置引擎与资源治理、**LRU 冷热调度器接引擎**（现仍走 M1 的
  `bt_seeding` 门闩）、中继生产化收尾（公网部署 / 鉴权 / 限流）、BT/TCP 打洞。
  Spike 判定解决的是「技术路线是否成立」，不是「M2 是否做完」。
- 报告与工具的 `evidence_valid_for_pass=false`、`auto_pass=false`、`spike_status_changed=false` 均为实测时点快照，**不回填**。
- 所有「不设数值承诺」的结论（ADR-012）、「先升中继再升客户端」的部署顺序（ADR-013）、
  R-04 / R-07 / R-03 的风险触发条件。

## 6. 复审时点

| 项 | 触发条件 |
| :--- | :--- |
| S6 语义（ADR-016） | M4 前，或 ≥4 个不同真实出口可用时（先到为准） |
| S5 fallback | 换上带 LEDBAT 的 uTP 实现、或换链路形状（带宽 / RTT）时重跑 |
| S3 | `cr-dht` 真实实现接入后，用同一 harness 复测双栈资源增量 |
| R-02 / R-11 | M4 前（不因本判定变化） |

## 7. 证据索引

- S3：[`s3-mainline-public-20261117.json`](../../tests/spikes/s3/mainline_probe/reports/s3-mainline-public-20261117.json)、`tests/spikes/s3/reports/`
- S5：[`s5-utp-netem-20261117.json`](../../tests/spikes/s5/utp_netem/reports/s5-utp-netem-20261117.json)、`tests/spikes/s5/utp_netem/run_netem.sh`
- S6：[`s6-matrix-aggregate-20261117-highid.json`](../../tests/spikes/s6/reports/s6-matrix-aggregate-20261117-highid.json)、`tests/spikes/s6/reports/highid-run-30/`
- 机器可读判定：[`tests/fixtures/g2-spike-verdicts.toml`](../../tests/fixtures/g2-spike-verdicts.toml)
- 口径来源：[ADR-016](../02-architecture/adr/ADR-016-s6-emule-like-reach.md)、[ADR-012](../02-architecture/adr/ADR-012-r04-best-effort.md)、[ADR-013](../02-architecture/adr/ADR-013-emule-lowid-declaration.md)
