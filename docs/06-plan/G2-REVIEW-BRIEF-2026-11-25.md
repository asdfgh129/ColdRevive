---
doc_id: DOC-31
title: G2 解禁复评简报（2026-11-25，通俗版 v2.0）
category: 计划
version: 2.3
status: 第1项暂缓 · 第2项10分钟daemon PASS · 第4项已部署/中继PASS/打洞BLOCKED · G2仍待所有者判定
owner: 项目所有者（判定） / 架构组（记录）
updated: 2026-11-25
---

# G2 解禁复评简报（2026-11-25，v2.4.128）

> 编号 DOC-31 · 版本 2.3 · 状态 **第1项暂缓；第2项10分钟 daemon PASS；第4项已部署，中继 PASS、打洞 BLOCKED；G2 仍待判定** · 更新 2026-11-25
>
> **门禁机器常量不变**：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。
> 这份简报不是解禁决定，也不授权任何人把机器字段改成 pass。G2 是 M2 出口，
> 必须由项目所有者基于真实证据书面判定。

## 0. 先看这里：四句话版

| # | 你问的问题 | 通俗答案 | 严格结论 | 还差什么 |
|---|---|---|---|---|
| 1 | 公平性到底测什么？为什么全是数字？ | 让两条数据流抢同一条较慢的网络，看是不是各拿约一半；**慢链路 + 开拥塞控制时 20 轮里 18 轮达标，2 轮差一点** | 按原预注册判据整体 **FAIL**；**所有者已决定：偏差在可接受范围，记录在案、暂缓处理** | 下一版重新调整测试（先登记新判据，再采新样本）；本版不动 β、不改阈值 |
| 2 | DHT 到底过没过？ | 单次真节点 announce 已由 ≥2 独立节点回查到；守护进程自动跑 10 分钟后也拿到 1 次 verified announce，两个独立节点中 1 个回查到我们 | 单次 CLI 与 10 分钟 daemon 自动运行均组件级 **PASS** | 更长时间 / 更多出口的数据仍未采集；不等于 G2/S3/S6 |
| 3 | 多样种逐出是什么问题？ | 「只允许同时做种 1 个文件，来第 2 个怎么办」；结果是第 2 个能启动，旧的被踢掉，始终只跑 1 个，**没发现问题** | P0 e2e **77/77 PASS** | 仅限本机回环；不代表公网 NAT |
| 4 | 两条 CPE DNAT 真的没了吗？ | **你是对的，映射还在。** 当前 daemon 已部署；真 BT 文件已跨公网经**中继**落地且 md5 一致。但真打洞这条没过：A 是 EIM，B 的映射随目的变化，按约定必须回退 | DNAT/探针 PASS；当前 daemon 部署 PASS；中继 BT **PASS**；punched-direct **BLOCKED** | 若要打洞直连，需要 B 侧具备可判定的 EIM 或运营侧给出稳定映射；否则维持中继 |

**一句话**：第 2 项单次 CLI 与 10 分钟 daemon 自动运行均过了组件级测试；
第 3 项规则测试过了、S6 语义复核条件仍未触发；第 4 项环境更正后当前 daemon 已部署，
真实 BT 文件经公网中继落地一致，但**打洞直连因 B 非 EIM 而 BLOCKED**；
第 1 项按原判据仍没过，但所有者已决定偏差可接受、暂缓到下一版重调测试。
G2 本身仍未被解禁。

---

## 1. 第 1 点：公平性在测什么，为什么 78/80 还算 FAIL？

### 1.1 实验设置（用人话讲）

在 221 这台机器上用两个网络命名空间造出两条虚拟线路，让**两条数据流同时通过同一个中继**
抢带宽。两种线路形状：

- **L1**：16 Mbit/s、20 ms 延迟、2 ms 抖动 —— 相当于比较宽的通道；
- **L2**：8 Mbit/s、50 ms 延迟、5 ms 抖动 —— 相当于更窄、更卡、抖动更大的通道。

四种模式（arm）：

- `cc_off`：关掉端侧拥塞控制，作为「量具是否偏袒某一条流」的对照；
- `cc_on`：打开 AIMD + RTT 超时重传；
- `cc_paced`：在 `cc_on` 基础上再加发送整形；
- `cc_on_per_session_cap`：打开中继侧每会话字节桶，用来区分「中继限流」与「端侧退让」。

### 1.2 判据（事先写死，采样本之前已登记）

`tests/fixtures/g2-fairness-thresholds.toml`，sha256 `0671a173…` 未改：

- 预热 3 秒，正式采样 10 秒；
- 每种模式每轮各跑 10 轮；
- 每条流实际收到的字节占比必须在 **45%–55%**；
- Jain 公平指数 ≥ **0.95**；
- **每一轮都要达标**，平均达标或中位数达标都不算（防止「9 轮很好、1 轮很糟」被平均掉）。

### 1.3 结果（翻译成人话）

| 线路 | 模式 | 结果 | 说明 |
|---|---|---|---|
| L1 | 四种模式 | 10/10 全过 | 宽通道下没问题 |
| L2 | `cc_off` | 10/10 全过 | 说明量具本身没有偏袒某条流 |
| L2 | `cc_on_per_session_cap` | 10/10 全过 | 说明中继限流不是失败原因 |
| L2 | `cc_on` | **9/10** | 第 7 轮 0.430 / 0.570 |
| L2 | `cc_paced` | **9/10** | 第 8 轮 0.433 / 0.567 |

所以总共 80 轮里 78 轮达标，但判据是「每种模式 10 轮全达标」，
L2 的 `cc_on` 和 `cc_paced` 各差 1 轮，**整体只能写 FAIL**。

`r = −0.706` 不是「70% 通过率」，而是相关系数：
「两条流丢包数差得越多，字节份额离 50% 越远」这个倾向比较强。
两轮失败恰好是丢包不对称最大的轮次；中继 shaping/dropped/limited 全为 0，
`cc_off` 的 20 轮都落在 0.499–0.502，说明不是量具偏心。

### 1.4 为什么不改参数重跑

AIMD 的 β（丢包后窗口减半系数）写死在探针 `main.rs:189`，当前是 0.5。
把它改成 0.7 再重跑，等于**看到结果后改规则**，违反「判据先于样本」。
所以原样保留 80 轮，既不删失败轮，也不靠多跑几次碰运气。

**所有者决定（2026-11-25 09:28）**：这两轮偏差在可接受范围内，先记录在案、
本版跳过，不调 β、不改阈值、不改判据；**下一版**再重新调整测试——
必须先以新时间戳 + 新 sha 登记新判据，再采集新样本。
机器常量与 G2 判定不受此决定影响。

---

## 2. 第 2 点：真实 Mainline DHT 到底过没过？

**答：这项组件测试过了，但不是 G2 过了。**

原来的障碍是引导节点不给 token（`token_len=0`）或超时。这次换了做法：

1. 用 `coldrevive-dht --walk` 从真实公网节点找到活节点；
2. 向真实路由节点拿 **真 token**（不是伪造）；
3. 发 announce，并确认收到 ACK；
4. 用**新的** get_peers RPC，在 **2 个不同公网节点**上查到 `112.5.60.4:17800`；
5. 新增 `--announce-seed` 直接调用守护进程 FR-18 用的 `announce_active_seed`，
   对真节点返回 `announced_verified`（`token_len=4`、`peer_verified=true`）。

判据 `walk_answered` / `token_issued` / `announce_accepted` /
`fresh_rpc_recheck` / `multi_node_placement` 全部 met。

**补充：守护进程自动运行（2026-11-25）**：在 221 上以 `dht.enabled=true`、
IPv4 公网节点作 bootstrap 跑 10 分钟，20 次 `/health.dht` 采样显示
5 个 cycle、1 次 attempt、1 次 verified、0 no_token / 0 failed / 0 resource_skipped；
两个独立公网节点的 `get_peers` 在 211 s 与 422 s 两次回查到 `112.5.60.4:19910`
（另一节点两次仍为 0 peer，如实保留）。进程 RSS 20 次采样均为 17,376 KB；
CPU 未采样，`/health.limits` 的 RSS/CPU current 为 null（未配置），不写成 0。
原始 `summary.external_recheck_hits=6` 是子串计数，已用 `summary_corrected` 更正为
2 次真实 peer-list 命中。

**边界**：这是单次 CLI + 10 分钟有界 daemon 窗口的组件级真实网络证据。
更长时间、多出口、CPU 时间序列仍未采集，不能写 S3/S6/G2 通过。

---

## 3. 第 3 点：守护进程多样种逐出，是出了什么问题吗？

**答：不是出了问题，是这个规则以前缺一份用当前构建跑出来的证据，现已补齐。**

规则：配置 `max_active_seeds = 1` 时，索引里可以有很多个文件，但**同时处于热做种状态的只能有 1 个**。
要验的是：

- 第一个文件被唤醒时，确实开始做种（`seed_active=1`）；
- 第二个不同 infohash 的文件被唤醒时，它会唤醒**自己的**做种任务；
- 旧任务被逐出（`evictions=1`），但活跃数始终 ≤ 1；
- 守护进程重启后索引与状态仍正确。

当前 release 构建跑 `tests/e2e/p0/run_p0.py`：**77/77 PASS**，其中 FR-10 关键项
两个不同 infohash 的真 BT 握手、cap=1、`evictions=1`、
`second_infohash_wakes_its_own_task`、`wake_count_never_exceeds_cap` 全部通过。

**边界**：这是本机回环双进程测试，不是公网 NAT 证据。它证明的是「逐出规则正确」，
不是「公网可达」。

**S6 语义复核材料（本次补入）**：S6 在 2026-11-17 已由 G0 判 `pass`，
但 `verdict_kind=closed_with_exceptions`，不是无例外通过。机器字段
`s6_status=not_run` 是**有意分层**，不是漏改。复核触发条件是「M4 或 ≥4 个不同真实出口」，
当前只实测到 `112.5.60.4` 与 `112.5.32.34` 两个出口，**触发条件未满足**。
且 S6 判定引用的部分原始报告位于 gitignored 的 `tests/spikes/s6/reports/`，
单独 checkout 不能复现，属于独立的可移植性欠账。
复核时只允许表述为「按 eMule 语义结案、带已知限制」；
不得写成「NAT 矩阵通过」「G2 达标」或任何可达率百分比。
是否把这份已结案判定折算进 G2，仍需所有者书面决定；本简报不代为改判。

---

## 4. 第 4 点：CPE DNAT 与公网打洞——用户质疑正确，已更正

### 4.1 我上一版错在哪里

我据「本机连 `112.5.60.4:19910` 超时、`112.5.32.34:19920` refused、
两机 `19910/19920` 无监听」写成「两条 CPE DNAT 映射都没了」。这是**误判**：

- punch 端点本来就是**实验期间临时起的进程**，实验结束没有监听是正常的；
- 本机跟 `112.5.60.4` 同出口，连公网地址超时是已记录的上游 hairpin 限制；
- `ConnectionRefused` 通常恰恰说明映射存在、内网主机回了 RST。

### 4.2 2026-11-25 复测结果

| 检查 | 结果 |
|---|---|
| 222 → `112.5.60.4:19910`，221 侧临时监听 | `PAYLOAD PONG`，监听侧收到 `DATA PING` |
| 221 → `112.5.32.34:19920`，222 侧临时监听 | `PAYLOAD PONG`，监听侧收到 `DATA PING` |
| 221 从 `19910` 连两个独立公网观测点的 22 端口 | 82/119 两台 tcpdump 都看到 `112.5.60.4:31480`，EIM 一致 |
| 立即用 `31480` 做双侧 TCP 同时打开 | 发现→打洞 3480 ms；两侧 `ok=true`、`bytes_roundtrip=true` |
| 一次慢 11 秒的失败尝试 | BLOCKED，如实归档，不删；说明发现端口必须即时使用 |

证据：

- `tests/ARCHIVE/evidence/tcp-dnat-recheck-20261125.json`
- `tests/ARCHIVE/evidence/tcp-eim-and-punch-recheck-20261125-fast.json`
- `tests/ARCHIVE/evidence/tcp-punch-discovered-recheck-20261125-fast.json`
- 慢速失败对照：`tcp-eim-and-punch-recheck-20261125.json`、
  `tcp-punch-discovered-recheck-20261125.json`

### 4.3 当前 daemon 已部署（不再是旧构建）

- 在 221 上用当前源码原生构建，md5 `1ba84f286fdab265e936fa65dfbc69e1`，
  部署到 221/222，两端 `/health` OK、`nat.punch_route.enabled=true`；
- 这次还带上两个真实修复：新中继连接先 `register`；`nat punch-route`
  支持第四参数共享 nonce，使两端能真同时打开同一会话；
- 为避免 daemon 自己占住 `19910/19920`，本地引擎改为
  `engine.listen=127.0.0.1:<bt_port>`，`bt_external` 与通告口径不变。
  这样 LAN 上的 `19910/19920` 留给绑定源端口的 punch socket；
- 全量串行回归 **537 passed / 0 failed / 3 ignored**。
- 证据：`tests/ARCHIVE/evidence/daemon-loopback-linux-deploy-20261125.json`、
  `regression-full-20261125.log`。

### 4.4 真 BT 字节：中继过了，打洞没过

**中继兜底 PASS**：B 通过 daemon 的 SOCKS5 桥下载真 `.torrent`，
aria2 `status=complete`、`completedLength=1048576`，落地 md5
`6209e962f3b26a13c9ec1ae70fd78e10`，与 A 侧样本一致。计数也吻合：
B `bytes_relayed=1051320`、`tunnels_alive=1`；A `holder_data.bytes_from_engine=1049585`、
`splices_done=1`。证据 `bt-over-public-relay-20261125.json`。
这是**真实公网中继数据面**，不是打洞直连，也不等于 G2/S6。

**打洞直连 BLOCKED**：两端同时做双观测点 EIM 探测，结果是：

| 节点 | 观测点 82 | 观测点 119 | 判定 |
|---|---|---|---|
| A / 221（源 `19910`） | `112.5.60.4:34656` | `112.5.60.4:34656` | EIM 一致 |
| B / 222（源 `19920`） | `112.5.32.34:2050` | `112.5.32.34:2055` | 不一致，非 EIM |

按事先约定的方法，B 侧映射随目的变化就**不得给出 discovered_port**，
必须立即回退中继；本次如实判 BLOCKED，没有拿 `2050`/`2055` 凑 PASS。
证据 `bt-over-punched-tcp-public-20261125-v2.json`。

结论：现在缺的不是 DNAT，也不是部署，而是 **B 侧可判定的 EIM/稳定映射**。
在运营侧未给出该条件前，punched-direct 不能写成通过。

---

## 5. 回归与归档

- 全量串行 `CARGO_INCREMENTAL=0 cargo test --workspace --offline -- --test-threads=1`：
  **537 passed / 0 failed / 3 ignored**（含本轮 `nat_path.rs` 修复后的最终回归）。
- 所有本轮运行（含 BLOCKED 与失败对照）已归档 `tests/ARCHIVE/TEST-RUNS-2026-11-25.md`。
  DHT 自动运行的一次配置写入失败、一次过早中止、一次成功均按原始结果分别归档。
- 审计更正见 `STAGE-DEBT-AUDIT-2026-11-21.md` §22.1–22.5、§23–24。

## 6. 需要所有者决定/提供的四件事

1. **公平性（已决定，暂缓）**：所有者判定偏差可接受，记录在案、本版跳过；
   本版不调 β、不改阈值、不改判据。下一版再重新调整测试：先登记新判据
   （新时间戳 + 新 sha），再采集新样本。
2. **BT 公网闭环**：当前 daemon 已部署；真 BT 文件已走**公网中继**并落地 md5 一致。
   **打洞直连仍 BLOCKED**：B 的两个公网观测端口不一致（`2050` vs `2055`），
   按约定不得外推。若仍要求 punched-direct，需要所有者/运营侧提供 B 的可判定 EIM
   或稳定公网映射；DNAT 不需要重建。
3. **S6 语义**：G0-REVIEW 记载 S6 已于 2026-11-17 判 `pass(closed_with_exceptions)`；
   机器字段按设计仍 `not_run`。是否折算进 G2 需书面决定。
4. **G2 出口**：即便证据齐备，`g2_claim` 也只能由所有者改；工具与文档一律不自置 pass。

## 7. 一句话结论

第 2 项单次与 10 分钟 daemon DHT 组件测试过了；第 3 项规则测试过了、S6 语义复核未触发；
第 4 项「NAT 被撤」已更正，当前 daemon 已部署，真 BT 文件经公网中继落地一致，
但打洞直连因 B 非 EIM 而 BLOCKED；第 1 项按原判据仍整体 FAIL，
但所有者已决定偏差可接受、本版暂缓，下一版再登记新判据重测。
G2 解禁的最终判断权在所有者。
