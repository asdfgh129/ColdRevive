---
doc_id: DOC-28
title: 阶段状态盘点：各优先级 P / 各里程碑 M 的完成与待办（2026-11-20）
category: 计划
version: 1.0
status: 有效
owner: 架构组
updated: 2026-11-20
---

# 阶段状态盘点：各优先级 P / 各里程碑 M 的完成与待办（2026-11-20）

> 编号 DOC-28 · 版本 1.0 · 状态 **有效** · 责任人 架构组 · 更新 2026-11-20
> 口径基准：仓库 commit `e1bb4e6`（公平性量具 v1），全量回归 336 passed / 0 failed。
> 本文是横向盘点，不改任何门禁口径：**`g2_claim=false`、`s6_status=not_run`，默认承载仍 `tcp`**。
> 命名约定：**P = 需求优先级 P0/P1/P2（[SRS](../01-product/SRS.md)）**；
> **M = 里程碑 M0–M5（[ROADMAP](ROADMAP.md)）**；另有架构模块 M-01～M-16（[ARCH](../02-architecture/ARCH.md)）。
> 详细证据见 [DOC-27 阶段小结](PHASE-SUMMARY-2026-11-20.md)、[M2-PROGRESS](M2-PROGRESS-2026-11-18.md)、
> [ADR-024](../02-architecture/adr/ADR-024-udp-bt-data-plane.md)、`tests/e2e/w61/`、`tests/e2e/dht-announce/`。

## 1. 按里程碑 M0–M5

| 里程碑 | 已完成 | 待完成 / 未达标 |
| :--- | :--- | :--- |
| **M0 Spike** | G0 条件冻结；S1/S2/S4/S7/S8=pass，S3/S6=pass（限定口径），S5=fallback；ADR-001/002/005 定稿 | R-02 按运营速率的拒绝率复审、R-11 书面法务意见（均压到 M4 前） |
| **M1 CLI 闭环** | **G1 已过**：T0 21/21 + T1 双机 11/11，唤醒→magnet→第三方下载闭环；FR-11 握手拦截已 opt-in 落地（W62 11/11） | W2 索引侧仅部分实现（无目录 watcher、无 SQLite 迁移）；FR-14 休眠仍未实现；FR-11 默认关闭、仅回环 TCP |
| **M2 引擎集成与资源治理** | W5.1～5.2 引擎下载/做种/公网 TCP/µTP；W3.3～W3.17 NAT/中继/DHT/生产化；W59/W60/W61 公网取片；公平性量具 v1；DHT 自有节点 announce 闭环 | **G2 未达标**：`g2_claim=false`、`s6_status=not_run`；公平性量具≠达标、真实 Mainline 未加入、TCP 打洞未接 BT 数据面、守护进程多任务逐出仅组件级、可靠层无拥塞语义 |
| **M3 兼容与生态** | G3 由所有者**带 6 条例外人工结案**（机器口径仍 `not_run`）；8 行矩阵均有素材；W5.1g aMule 公网 KAD 20/20 | Web UI、WebSocket、`rpc-secret` 鉴权、REST/OpenAPI、三进程托盘（`coldrevive-agent` 仍是骨架）；NFR-04 公网时延未测 |
| **M4 安全/冷启动/公测** | 未开始 | 安全测试、72 h Soak、安装包 ≤60 MB、R-02/R-11 收口、公测 KPI、G4 |
| **M5 可选增强** | 未开始 | 跨协议补源、MCP/REST、移动端评估、激励 ADR-005 |

## 2. 按需求优先级 P0 / P1 / P2（SRS 共 26 行，FR-24 编号重复一次）

| 优先级 | 已实现 | 部分实现 | 未实现 |
| :--- | :--- | :--- | :--- |
| **P0**（17 行） | FR-03 伴随种子、FR-08 私有唤醒；FR-11 握手拦截（opt-in，W62 11/11，仅回环 TCP） | FR-01 目录监控、FR-02 指纹、FR-04 持久化、FR-05/06/07 KAD 发布查询、FR-09 限流、FR-10 LRU、FR-12 链接生成、FR-13 引擎、FR-14 休眠、FR-15 阈值；FR-24 本地索引→`ed2k://` 已落地（公网词典检索 BLOCKED）；FR-25 栈已生产化但 daemon 未接线（组件级） | — FR-14 休眠仍未实现；FR-24 公网检索、FR-25 公网传输与全量双轨发布仍 BLOCKED |
| **P1**（6 行） | — | FR-16 QoS、FR-17 NAT 打洞/回退、FR-18 Mainline DHT、FR-20 aria2 JSON-RPC | **FR-19 Web UI、FR-21 WebSocket** |
| **P2**（3 行） | — | FR-22 CLI/REST/插件 | FR-23 跨协议补源、FR-24（重复编号）信誉激励 |

> 判定口径：「已实现」= 有生产代码且有测试/e2e 证据；「部分实现」= 有代码但未接线、
> 仅组件级/单测/回环或仅 opt-in；「未实现」= 只有骨架或完全缺失。
> 逐条证据指针见 §5「需求逐条核对」与源码注释。

## 3. 按架构模块 M-01～M-16

- **完整实现**：M-02 `cr-hash`、M-03 `cr-torrent`、M-07 `cr-nat`、M-08 `cr-wake`
- **部分实现**：M-01 `cr-indexer`（一次性扫描，无 watcher）、M-04 `cr-store`（JSON，无 SQLite 迁移）、
  M-05 `cr-kad`（生产仅 CLI publish/find，无 TTL 续期）、M-06 `cr-dht`（协议栈+工具，daemon 未启用）、
  M-09 `cr-sched`（已接线，守护进程仍单任务）、M-11 `cr-engine`（无完成回流闭环）、
  M-13 `cr-qos`（令牌桶未接数据面）、M-14 `cr-api`（HTTP/JSON-RPC 子集）
- **仅骨架**：M-10 `cr-guard`、M-12 `cr-ed2k`、M-15 `cr-reputation`、M-16 `cr-ui`
  —— 源码注释明确写「G2 前不实现」，不计入成绩。

## 4. 进度判断（必要提示）

1. **门禁绿 ≠ 需求齐**：G1 已过、G0 条件冻结、G3 人工结案，但 SRS 26 行需求中
   **3 行完全实现、14 行部分、0 行「完全未实现」**（FR-11 升级为已实现；
   FR-24/25 从「未实现」升级为部分：本地链接输出 + 生产 eD2k 栈，公网/daemon 全量仍 BLOCKED）。
   里程碑看起来顺，功能面仍只铺了一层——「部分」不等于「完成」。
2. **M2 是唯一重心**：所有工程产出集中在 M2，G2 是唯一出口门禁且未过；M3/M4/M5 基本是骨架。
3. **证据强度必须分层读**：T0/T1/W59/W61 是真机真链路；G2-SPIKE 与 G3 closeout 是**人工判定**；
   骨架 crate 只是占位。三者不能混算为「完成度」。
4. **容易高估的点**：FR-14 没有 idle 自动休眠；FR-15 只接了 seed cap；
   FR-20 没有 `rpc-secret`；W61 只覆盖 `119.29.150.254` 一条路径；公平性量具只出原始计数。
   FR-11 虽已落地但**默认关闭**且只回环 TCP；FR-24 的 `ed2k://` 是本地索引算的，
   不是公网词典检索结果；FR-25 的栈在生产 crate 里跑通，但 daemon 未接线、公网未测。
5. **未定位的体验风险**：W5.1d/f 公网 4 MiB 耗时 143–235 s（LAN 秒级），形态稳定、成因未明；
   按 ADR-012 不折算速率，但这是真实体验问题。
6. **本轮已补 P0 FR-11/24/25（2026-11-20）**：FR-11 opt-in 握手拦截（W62 11/11）；
   FR-24 本地 `ed2k://` 输出 + 无前缀 MD4 Key（W24 独立 MD4 复算 2/2）；
   FR-25 eD2k 栈纳入生产 crate cr-ed2k（33 单测 + W25 三种 PARTSIZE 边界真 TCP 取片）。
   **仍开放**：FR-24 公网 KAD 0x33 检索（需 G0 `protocol_scope` 放宽）、FR-25 daemon 接线与
   公网传输、FR-14 休眠；这些不因本轮而算完成。
7. **治理债**：SRS 仍 v2.0「待评审」（2026-11-09 起未动）、FR-24 编号重复、
   ROADMAP version/date 未跟上 11-20 更新。

## 5. 需求逐条核对（FR-01～FR-25）

| ID | 结论 | 证据指针 / 口径 |
| :--- | :--- | :--- |
| FR-01 | 部分 | `cr-indexer::inspect()` 一次性 read+hash；无 inotify/ReadDirectoryChangesW watcher |
| FR-02 | 部分 | `cr-hash` sha256/sha1/md4/ed2k_hash 齐备并有已知答案测试；索引只接 SHA-256/BT SHA-1 |
| FR-03 | 已实现 | `cr-torrent::generate_with_announce_list`；W51c 第三方 qB/Transmission/aria2 实可取片 |
| FR-04 | 部分 | `cr-store::ShadowIndex::open/save` JSON roundtrip；无 SQLite 迁移、无重启 e2e |
| FR-05 | 部分 | `cr-kad` Key 派生 + CLI `publish`；daemon 无周期发布/TTL 续期 |
| FR-06 | 部分 | 关键词 Key 由配置 `keywords` 派生，非分词；本地 `find` 只回 hit_node/count |
| FR-07 | 部分 | `cr-kad::find_frame/find_result`；无去重与健康度排序；eMule KAD 无 `FIND_VALUE` |
| FR-08 | 已实现 | `cr-wake` WAKEUP_REQ/ACK/REJECT 接线；T0 21/21 + T1 11/11 |
| FR-09 | 部分 | Challenge + nonce 去重已接线；黑名单仅靠 HASH_MISMATCH 注入，生产路径无按 IP 限流 |
| FR-10 | 部分 | `sched::wake_seed` 为唯一 activate 入口，真引擎逐出测试；守护进程级逐出未做 |
| FR-11 | 已实现（opt-in） | `cr-guard` 握手解析 + `splice_prefaced` 补回已读字节；`engine.guard_handshake` 默认 false；W62 11/11：裸握手只唤醒对应样本、md5 一致、未知 infohash 不唤醒。仅回环 TCP，µTP 不外透 |
| FR-12 | 部分 | magnet + `/torrent/` 已接线；`ed2k_link` 原语存在但生产未调用 |
| FR-13 | 部分 | librqbit 下载/暂停/恢复可用；完成产物不回流重建索引（无闭环） |
| FR-14 | 部分 | ShadowIndex + COLD/HOT 状态存在；无 idle 定时器、无自动 HOT→COLD |
| FR-15 | 部分 | seed pool cap 与中继令牌桶有；内存/CPU/带宽阈值自动拒绝唤醒未接线 |
| FR-16 | 部分 | uTP 默认开（W5.1e）；`cr-qos` 令牌桶未接 BT 做种；LEDBAT 被 S5 `fallback` 否证 |
| FR-17 | 部分 | UPnP/NAT-PMP/PCP/holepunch/relay/reach 有实现与单测；daemon 不自动建映射 |
| FR-18 | 部分 | std-only DHT 栈 + CLI；daemon 关闭 DHT；announce 只对自有公网节点闭环 |
| FR-19 | 未实现 | `cr-api` 注释明确「G2 通过前不实现」；无内嵌 Web UI 资产 |
| FR-20 | 部分 | `/jsonrpc` 子集 + CORS，AriaNg/MV3 测过；**无 `rpc-secret`** |
| FR-21 | 未实现 | `cr-api` 注释明确未实现；无 WebSocket server/upgrade/push |
| FR-22 | 部分 | CLI 可用；HTTP 仅少量 ad-hoc 端点、无 OpenAPI；浏览器插件只是测试资产 |
| FR-23 | 未实现 | `cr-ed2k` 骨架；可用栈仅在 `tests/spikes/s2/ed2k-mini` |
| FR-24 | 部分实现 | `cr-hash` MD4 已知答案测试；`cr-kad::kad_ed2k` 无前缀 A 轨 Key + 0x33/0x3B 字节编解码；`/cli ed2k` 从真实文件算 `ed2k_hash` 输出标准链接（W24 独立 MD4 复算 2/2）。**公网 KAD 0x33 词典检索 BLOCKED**（需 G0 scope 放宽） |
| FR-25 | 部分实现 | S2 审计过的 eD2k 栈（sha1/kad wire/ed2k wire/stack）经 `#[path]` 纳入生产 crate `cr-ed2k`，33 单测通过；W25 真 TCP 回环取片三边界（4KiB / ==PARTSIZE / 多分块）全部 hash+content 一致。**daemon 未接线、公网 server/peer 未测、全量双轨发布未做，仍 BLOCKED** |

> SRS 第 45 行重复占用 FR-24（节点信誉/贡献积分），映射到 `cr-reputation` 骨架，仍未实现。

> FR-11/24/25 本轮升级的证据见 `tests/e2e/w62/`、`tests/e2e/w24/`、`tests/e2e/w25/`。

## 6. 复现 / 核对命令

```bash
# 全量回归（先停占用 17800 的本地守护进程）
CARGO_INCREMENTAL=0 cargo test --workspace --offline

# 机读门禁与人工判定分开看
cat tests/spikes/spikes.json
cat tests/fixtures/g2-spike-verdicts.toml
python tests/e2e/w61/make_report.py
```
