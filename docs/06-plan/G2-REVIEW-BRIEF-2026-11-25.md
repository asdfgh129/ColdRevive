---
doc_id: DOC-31
title: G2 解禁复评简报（2026-11-25）
category: 计划
version: 1.0
status: 待所有者判定
owner: 项目所有者（判定） / 架构组（记录）
updated: 2026-11-25
---

# G2 解禁复评简报（2026-11-25，v2.4.124）

> 编号 DOC-31 · 版本 1.0 · 状态 **待所有者判定** · 更新 2026-11-25
>
> **门禁机器常量不变**：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。
> 本简报**不是**解禁决定，也不授权任何人把机器字段改成 pass。G2 是 M2 出口，
> 必须由项目所有者基于真实证据书面判定（G0-REVIEW / G2-SPIKE-VERDICT 的既定口径）。

## 1. 本轮做了什么

针对上一轮遗留的 G2 相关任务逐条推进；每条都给出「清掉了什么 / 仍缺什么 /
证据路径」。所有测试运行按规矩归档到 `tests/ARCHIVE/TEST-RUNS-2026-11-25.md`。

## 2. 逐条状态

### 2.1 公平性：归因完成，但**未达标，也不调参**

- 预注册判据文件 `tests/fixtures/g2-fairness-thresholds.toml` 的 sha256 仍是
  `0671a173bd4cb013eebba02ed44b8d53ead006844bef5a47b37c4ebe7109e594`（未改、未重录）。
- 对 80 轮原始报告定量分析：L2 cc_on+cc_paced 共 20 轮，
  **Pearson r(丢包差, 份额偏移) = −0.706**；两轮失败恰好是丢包不对称最大的两轮
  （+2 → −0.070、+3 → −0.067），中继 shaping/dropped/limited 全 0，
  `cc_off` 控制臂 20 轮全部 0.499–0.502（量具无方向偏置）。
- **决定不把 β 从 0.5 改到 0.7、不重抽样**。探针 `main.rs:189` 硬编码 β=0.5，
  判据先于样本登记；按结果改参数再重跑等于调参，违反约定。
- 证据：`tests/ARCHIVE/evidence/g2-fairness-attribution-20261125.json`、
  审计 §21。**所有 80 轮仍是整体 FAIL**，不宣称达标。

### 2.2 真实 Mainline DHT：原「token_len=0 / 引导节点超时」证据缺口**已补**

- 引导节点仍不给 token（`dht.transmissionbt.com` token_len=0、另两个超时），
  但真实路由节点会签发：新增 `coldrevive-dht --walk` 输出 `contacts`、
  `--get-peers` 输出 `node_list`（只读，不改既有结构）。
- 新增可复现 harness `tests/e2e/w62/mainline_dht_announce_verify.py`：
  真 token → announce ACK → 用**新的** get_peers RPC 在 ≥2 个不同公网节点回查到
  `112.5.60.4:17800`。全部判据 met。
- 更关键：新增 `--announce-seed` 直接调用守护进程 FR-18 用的
  `cr_dht::announce_active_seed`，对真节点 `114.224.34.208:17700` 返回
  `announced_verified`（token_len=4、peer_verified=true）。
- 证据：`mainline-dht-announce-verify-20261125.json`、
  `mainline-dht-daemon-path-20261125.log`。
- **边界**：这是组件级/单轮真实网络证据；守护进程**自动长期运行**的 DHT 可达率
  与资源数据仍未采集，不等于 S3/S6/G2。

### 2.3 守护进程多样种逐出：e2e 证据**已刷新**

- 当前 release 构建跑 `tests/e2e/p0/run_p0.py`：**77/77 PASS**。
- FR-10 关键项：两个不同 infohash 的真实 BT 握手、cap=1、
  `evictions=1`、`seed_active<=cap`、`second_infohash_wakes_its_own_task`、
  `wake_count_never_exceeds_cap`。
- 证据：`tests/ARCHIVE/evidence/p0-daemon-eviction-20261125.json`。
- 边界：本机回环双进程，不是公网 NAT 证据。

### 2.4 公网 BT-over-punched-TCP：**BLOCKED（外部条件撤销）**，代码级组合已补

- 真机复评：CPE TCP DNAT `112.5.60.4:19910` 本机连接超时、
  `112.5.32.34:19920` ConnectionRefused；221/222 上 19910/19920 均无监听；
  221 部署源码没有 `tcp_discovery`，221/222 二进制分别是 11-24 08:12 / 11-18 09:51，
  早于桥键修复。**需要所有者先重建 DNAT 并部署当前构建**，才能按 DOC-30 做真机验证。
- 代码级缺口已补：新增
  `bound_source_port_socket_carries_real_librqbit_piece`（用 DOC-30 的
  `connect_bound_blocking` 固定源端口原语）：登记键与 SYN 目的解耦
  （v2.4.123 `PunchTargets` 不变量），真 librqbit 下载 1,048,576 B、
  `finished=true`、逐字节一致、`served_registered=1`。
- 证据：`bt-over-punch-public-blocked-20261125.json`、
  `bound-port-bridge-e2e-20261125.json`、审计 §22。
- 边界：回环组件级组合，不是 NAT 可达性证据。

## 3. 回归

`CARGO_INCREMENTAL=0 cargo test --workspace --offline -- --test-threads=1`：
**536 passed / 0 failed / 3 ignored**（v2.4.123 为 535，+1 绑定源端口 e2e），
0 panic。证据 `tests/ARCHIVE/evidence/workspace-test-20261124-v124.log`。

## 4. 仍需所有者决定的事项

1. **公平性整体 FAIL 如何处置**：维持原判据（不达标）；或书面批准一套**新的**
   预注册判据（新时间戳 + 新 sha）后再采集新样本。本轮不代为选择。
2. **恢复公网真机环境**：重建两条 CPE TCP DNAT、把当前构建部署到 221/222、
   提供两个独立公网观测点，之后才能跑真机 BT-over-punched-TCP。
3. **S6 语义复审**：G0-REVIEW 记载 S6 已于 2026-11-17 判 `pass(closed_with_exceptions)`；
   机器字段按设计仍为 `not_run`。是否把人工判定折算进 G2，需所有者书面决定。
4. **G2 出口判断**：即便上述证据齐备，`g2_claim` 也只能由所有者改；
   工具与文档一律不自置 pass。

## 5. 一句话结论

本轮把「Mainline DHT 可达性」和「守护进程多样种逐出」两项证据缺口补成真机/真引擎
证据，给「BT-over-punched-TCP」补了绑定源端口的组件级闭环并如实登记公网 BLOCKED；
公平性完成定量归因但按预注册判据仍整体 FAIL、不调参。G2 解禁的最终判断权在所有者。
