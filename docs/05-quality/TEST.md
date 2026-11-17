---
doc_id: DOC-12
title: 测试计划（TEST）
category: 质量
version: 2.9
status: 待评审
owner: 测试组
updated: 2026-11-17
---

# 测试计划（TEST）

> 编号 DOC-12 · 版本 2.9 · 状态 待评审 · 责任人 测试组 · 更新 2026-11-17
> 本文是 [SRS KPI](../01-product/SRS.md) 与 [ROADMAP 质量门禁 G0～G4](../06-plan/ROADMAP.md) 的判定依据。测试代码落位于 [tests/](../../tests/README.md)。

> **【当前状态更正 · 2026-11-14 / v2.4.47】本文 §2.2、§6 之前的状态描述保留 2026-11-13 时点口径，不是现状。**
> **G1 已于 2026-11-14 通过**：T0 `--mode real` 21/21（含 2026-11-14 复跑）+ T1 `--mode real` 双物理主机 11/11，
> 均 `evidence_valid=true`；机读见 `tests/spikes/spikes.json` 的 `t1_progress` / `m1_progress.m2_gate`。
> **M2 门禁解除并已开工**（W3.3 `cr-nat` 接入 `coldrevived` 复活路径；W3.4 `cr-nat::relay_server` +
> `coldrevive-relay` 二进制 + 持有者订阅线程已落地）；[T2 本地接线验收](../../tests/e2e/t2/README.md)
> 10/10 PASS，中继改用真实服务端但仍**无真实 NAT**，`evidence_valid=false`、
> `s6_status=not_run`、`g2_claim=false`，**不构成 S6 / G2 证据**。
> **S3 / S5 / S6 状态：2026-11-17 已由所有者依证据判定**（S3 `pass` / S5 `fallback` /
> S6 `pass`（`closed_with_exceptions`），见 §2.11 与[判定记录](../06-plan/G2-SPIKE-VERDICT-2026-11-17.md)）；
> **G2 仍未达标、未宣称**（`g2_claim=false`，欠 M2 实现项）；`cargo test --workspace --offline` 168/168。
> 详见 [G0-REVIEW §11](../06-plan/G0-REVIEW.md) 与 [ROADMAP §6](../06-plan/ROADMAP.md)。

## 1. 测试分层

| 层级 | 范围 | 工具 / 方法 | 目录 |
| :--- | :--- | :--- | :--- |
| 单元 | 哈希、分词、Key 生成、Challenge 校验、LRU 淘汰、状态机 | `cargo test`，覆盖率 ≥ 70% | `tests/unit/` |
| 集成 | 索引 → 造种 → 入库 → KAD 发布 → 唤醒 → 下载闭环 | 双节点 Docker 网络 | `tests/integration/` |
| 协议一致性 | 唤醒报文、错误码、JSON-RPC 契约 | 契约测试 + golden files | `tests/integration/` |
| 网络仿真 | 1,000+ 虚拟 DHT 节点、丢包/延迟/抖动、节点抖动 | 本地 DHT 模拟器（`dht_simulator.py`） | `tests/e2e/` |
| 兼容性 | 见 §3 矩阵 | 真实客户端回归 | `tests/e2e/` |
| 性能 / Soak | 见 §4 | 72 h 长稳 + RSS/CPU 采样 | `tests/e2e/` |
| 安全 | 污染注入、Sybil、洪泛、重放、路径穿越 | 故障注入测试 | `tests/e2e/` |

## 2. 测试环境

| 编号 | 环境 | 用途 |
| :--- | :--- | :--- |
| **T0** | 单机回环，两个守护进程实例不同端口 | 验证全链路逻辑，最快反馈 |
| **T1** | 局域网双机，真实 NAT 内网 | 验证 UPnP 与 UDP 唤醒 |
| **T2** | 公网多节点（≥ 8 台不同网络环境） | 测可达率与复活成功率 |
| **T3** | 可控 DHT 仿真网络（1,000+ 虚拟节点） | 测发布/查询命中与污染抗性 |

> 环境编号在 [tests/fixtures/](../../tests/README.md) 配置中保持一致。`fixtures/` 中禁止存放任何真实受版权保护内容，一律使用自制小文件与合成数据。

> **编号消歧（2026-11-14）**：上表 `T2` 是**测试环境规划编号**（公网 ≥8 台不同网络节点，尚未落地）。
> 仓库中新增的 [`tests/e2e/t2/`](../../tests/e2e/t2/README.md) 是 **M2/W3.3–W3.4 的本地接线验收套件**
> （同一主机双守护进程 + mock STUN + **真实 `coldrevive-relay` 服务端**），二者**不是同一个东西**，
> 也不是上表 T2 环境的实现；真实中继不等于真实 NAT 环境。
> 本地套件报告固定 `evidence_valid=false` / `s6_status=not_run` / `g2_claim=false`；
> 将来公网多节点套件落地时应另取目录名（如 `e2e/t2-public/`）并区分用例 ID，
> 避免把「本地接线 PASS」误读为「公网可达率证据」。

### 2.1 T0 验收脚本骨架

[T0 双节点验收骨架](../../tests/e2e/t0/README.md) 已落位于 `tests/e2e/t0/`，是 G1 门禁的机器可读证据来源。**2026-11-13 起已接线并跑通真实链路**：

| 项 | 内容 |
| :--- | :--- |
| 用例 | `T0-00`～`T0-20`，覆盖 F1→F2→F3→F4 主链路、§5 故障注入、PROTO v1 golden 契约与最小暴露检查；判据出处逐条登记在 `t0_cases.json` |
| 阈值 | 只在 `t0_config.json` 定义一次：P95 < 3000 ms（NFR-04）、复活成功率 1.0（G1）、唤醒超时 1.5 s（PROTO §3.1） |
| 环境 | A / B 两实例回环不同端口；样本由固定种子生成，无版权内容 |
| 防呆 | `--self-test` 用合成观测验证脚本本身，报告的 `evidence_valid` 恒为 `false`；`CliAdapter.wired=True` 为代码层接线，但 real 运行前动态校验三个二进制环境变量，缺任一项一律 `BLOCKED` / 退出码 2，杜绝「没跑却显示绿」 |
| 观测层 | CLI v1 契约（`t0_config.json` 的 `launch`）与 20 条真实取数映射（`t0_cli.OBSERVE_PLAN`）已落地；伴随种子 InfoHash 按 BT 规范从 info 字典原始字节独立计算。`t0_cli_selftest.py` 验证「字段缺失不合成、缺二进制抛异常」 |
| 证据 | JSON 报告落在 `tests/e2e/t0/reports/`，按 §7 附入 G1 交付物；**2026-11-13 `--mode real` 实跑 21/21 PASS、`evidence_valid=true`**（数据面由第三方 aria2 真实完成 BT 下载）。接线清单见骨架 README §5 |

> T0 只在 `127.0.0.1` 运行，除 S2 专项外不接触公共 KAD。**T0 的 PASS 只覆盖单机回环**，T1（局域网双机）通过后才与 T0 共同构成 G1 证据，见 [ROADMAP §3](../06-plan/ROADMAP.md)。

### 2.2 T1 双机验收（2026-11-13 新增 harness，真实验收仍 BLOCKED）

[T1 双机 + 真实 NAT 验收](../../tests/e2e/t1/README.md) 落位于 `tests/e2e/t1/`：10 条用例 / 40 条断言，
覆盖双机身份、跨主机 KAD 发现、UDP 唤醒、P95 / 复活成功率、伴随种子与真实 InfoHash、
第三方 BT 数据面、真实 NAT 特征、PROTO v1 golden 与最小暴露。

| 运行 | 结果 | 证据有效性 |
| :--- | :--- | :--- |
| `--self-test` | 11/11 PASS（含注入坏观测判 FAIL 的断言引擎自检） | `false`（合成观测） |
| `--lan-precheck --address-mode aliases` | 10 PASS / 1 SKIP（T1-01 双机身份） | `false`（同一物理主机） |
| `--lan-precheck --address-mode nic` | 10 PASS / 1 SKIP（T1-01 双机身份） | `false`（同一物理主机） |
| `--mode real` | 10 BLOCKED / 退出码 2 | `false`（无第二台物理主机） |

预检真实量测：唤醒 P95 **2.0 ms**（30 样本、0 超时）、复活 **30/30**、aria2 跨地址完成 BT 下载。
`evidence_valid=true` 仅当 `--mode real` + 全用例 PASS 无 BLOCKED/SKIP + `/health` 实测 A/B 的
`machine_id` 与 `host` 均不同 + 真实 NAT 报告 + 观测来源 `real`。

**环境阻塞（实测，非推断）**：`t1_netprobe.py` 的 25 组真实 UDP 探测显示本机同 IP `5/5` 可达、
跨 IP 仅 `6/20`（全部只在回环别名之间）；物理网卡与 Hyper-V 交换机双向 `WSAENETUNREACH (10051)`。
WSL / 第二台虚拟机被安全策略禁止，未做绕过。**设计缺口**：M1 CLI MVP 无 UPnP / NAT-PMP / 打洞 / 中继，
跨 NAT 双机须先补穿透栈（详见 [T1 README §5](../../tests/e2e/t1/README.md)）。

> **【2026-11-14 状态更正】以上 §2.2 表格、环境阻塞与设计缺口描述均为 2026-11-13 单机时点记录。**
> 用户部署第二台物理主机（B=192.168.1.209 / DESKTOP-CCE53RF，A=192.168.1.210 / DESKTOP-DJQSC95）后，
> `--mode real --refresh-nat` 已实跑 **11/11 PASS、`evidence_valid=true`**
> （报告 `tests/e2e/t1/reports/t1-20261114T084653Z-real-aliases.json`，现场 STUN 探针
> `t1-nat-20261114T084639Z.json`），与 T0 21/21 共同构成 **G1 通过**；「只有一台物理主机」的阻塞条件已消除。
> 「M1 无 NAT 穿透栈」的**接线层**缺口已由 M2/W3.3 的 `cr-nat` 接入关闭
> （[T2 本地接线验收](../../tests/e2e/t2/README.md) 10/10 PASS，`evidence_valid=false`）。
> **S6 判据现状**：2026-11-17 两半真实证据已齐，所有者判定 `pass`（`closed_with_exceptions`），
> 详见 §2.11；机器常量 `s6_status` 仍为 `not_run`（工具不替人下结论）。
> **2026-11-16 口径变更（[ADR-012](../02-architecture/adr/ADR-012-r04-best-effort.md)）**：
> S6 不再以「可达率 ≥70%」判定，改为四条定性要求（5 类各有真实设备证据 / 每类两条路径各 ≥30 次
> 真实观测 / 直连失败时中继必须送达 / 结论由执行人手工回填）。可达率降为描述性统计。
> **2026-11-17 第二次口径变更（[ADR-016](../02-architecture/adr/ADR-016-s6-emule-like-reach.md)）**：
> 上一条的「5 类各有真实设备证据」**不再是 pass 前提**——S6 阶段性改为 eMule 式可达语义：
> 真实 NAT 设备上 HighID（`direct` 可达）与 LowID（`direct` 失败但 `relay` 送达）两半
> 各有真实证据，节点自报声明与实测一致（ADR-013 / W5.2）；类别覆盖降级为描述性清单。
> **一条没松**：条款①两台不同物理主机 / 条款②真实设备 / 每路径 30 次真实 `WAKEUP_ACK` /
> 无数值承诺 / 不自动 pass / `S6.status=not_run` / `g2_claim=false`。
> 满足后 G2 只能写「按 eMule 语义结案，带已知限制」。`nat_matrix.py --self-test` **24/24**。
> 现有证据（2026-11-17 补跑后）：W3.7 symmetric 真实报告覆盖 LowID 一半
> （direct 0/30 + relay 30/30）；反向拓扑报告覆盖 HighID 一半
> （port-restricted-cone 持有 + symmetric 请求，direct **3/30** + relay **30/30**）。
> `--aggregate` 得 `status=OBSERVED`、`evidence_valid=true`、`reach_evidence.complete=true`；
> `full-cone`/`restricted-cone`/`cgnat` 仍登记为已知限制，类别清单（2/4）不是 pass 前提。
> 按 eMule 语义可结案，但不得写「NAT 矩阵通过」或「G2 达标」。
> 方法见 [nat-matrix.md §23](../../tests/spikes/s6/nat-matrix.md)。

### 2.3 W5.1 引擎下载验收（2026-11-16 新增）

生产守护进程第一次有了自己的数据面。脚本 `tests/e2e/w51/engine_download.py`，全本机回环、
DHT 关闭、tracker 用 A 的内置 `/announce`，**不产生任何公网报文**：

```
B --/cli wake--> A            A: COLD → HOT，引擎解除暂停开始供片（W5.1b 起）
B --/jsonrpc aria2.addUri--> cr-engine ← 按 tracker 返回的 peer 直连 A 拉分片
B 落盘文件 → SHA-256 与样本逐字节比对
```

通过条件（三条同时成立，缺一条即 fail）：

| # | 断言 | 来源 |
| :--- | :--- | :--- |
| ① | `status=complete` 且 `completedLength=1048576` | 引擎计数器（`/jsonrpc tellStatus`） |
| ② | 落盘文件 SHA-256 == 样本 SHA-256 | 真实文件 |
| ③ | A 侧 `served_bytes >= 1048576` | 供片方计数器（`/cli seed`，W5.1b 起来自引擎） |

**2026-11-16 实测（4 次复跑一致）**：8/8 断言 PASS，1 MiB / 4 片 / 64 次 request，
`served_bytes=1048576`，样本与落盘 SHA-256 均为 `37be9cb3…79e17`；报告落
`tests/e2e/w51/reports/engine-download-*.json`（本地证据，gitignore）。
**边界**：单机回环、无第三方客户端参与、不构成 G3 或 S1 的新证据，也不产生任何可达率数字；
它只回答一个问题——「生产守护进程能不能自己把种子下完并校验通过」，答案是能。
**2026-11-17 口径收窄（W5.1b）**：做种侧也换成同一套库后，本条两端同库，只能证明「协议自洽」，
**不再构成独立互证**；标准协议互通由 §2.4 与 T0-08 的第三方客户端承担。

### 2.4 W5.1b 引擎做种 → 第三方客户端（2026-11-17 新增）

做种从手写种子端换成 `cr-engine`（librqbit）后，**必须由一个不是我们写的客户端来验收**，
否则只是「自己和自己互通」。脚本 `tests/e2e/w51b/seed_to_thirdparty.py`，仍全本机回环、
DHT/PEX 关闭、tracker 用 A 的内置 `/announce`：

```
COLD: 第三方 aria2 → A 的 bt_port（端口在听，引擎停在 paused）→ 拿不到任何分片
唤醒 : B --/cli wake--> A → 引擎 activate（幂等）→ state=hot
HOT : 第三方 aria2 → 完整 1 MiB → SHA-256 与样本一致 → A 引擎 served_bytes=1048576
```

九条断言（缺一条即 fail）：

| # | 断言 | 来源 |
| :--- | :--- | :--- |
| ① | A 日志有「做种引擎就绪」且无旧种子端 | 守护进程日志（身份检查，防跑旧二进制） |
| ② | 伴随种子可取回 | `/torrent/<infohash>` 真实字节 |
| ③ | COLD 时 `state=cold`、`served_bytes=0` | `/cli seed`（引擎计数器） |
| ④ | COLD 时第三方客户端拿不完整 | aria2 8 s 内 `bytes_on_disk=0` 并超时 |
| ⑤ | 唤醒后 `state=hot` 且 `seeding_hot=true` | `/cli seed` |
| ⑥ | 第三方客户端下完且字节数正确 | aria2 退出码 0 + 落盘大小 |
| ⑦ | 落盘 SHA-256 == 样本 | 真实文件 |
| ⑧ | 供片方计数器覆盖全量 | `served_bytes >= 1048576` |
| ⑨ | 连发 3 次唤醒后仍 hot | 激活幂等回归（librqbit 对 live 任务 unpause 会 bail） |

**2026-11-17 实测（3 次复跑一致）**：9/9 PASS，样本与落盘 SHA-256 均为 `f8e3fa3a…670aa29`，
报告落 `tests/e2e/w51b/reports/seed-to-thirdparty-*.json`（本地证据，gitignore）。
同时 T0 `--mode real` 21/21，其中 **T0-08 就是第三方 aria2 从引擎做种处完成标准下载**。
**边界**：回环、无公网报文、不含任何可达率数字；LAN/公网上的旧客户端复核仍未做。

### 2.5 W5.1c LAN 跨主机复核（2026-11-17 新增）

回环测不出三类问题：**绑定地址写错**（只听 127.0.0.1）、**入向被拦**、**tracker 把回环地址
当 peer 通告出去**。所以下载方必须搬出本机。脚本 `tests/e2e/w51c/lan_thirdparty.py`：

```
A = 192.168.1.210（Windows，本机，引擎监听 0.0.0.0:18782）
B = 127.0.0.1（只负责发 WAKEUP_REQ）
下载方 = 192.168.1.221（Ubuntu，Transmission 3.00 与 qBittorrent 4.5.1，均真实网卡）
样本 4 MiB = 16 片；DHT/PEX/LSD 关闭；tracker 为 A 的 /announce
```

十八条断言（缺一条即 fail），关键几条：

| # | 断言 | 来源 |
| :--- | :--- | :--- |
| ① | **221 越过网卡**访问 A 的 `/health` 返回 200 | 在 221 上执行 curl |
| ② | 引擎实际监听 `0.0.0.0:<bt_port>` | 守护进程日志 |
| ③ | 种子 `announce` 指向 A 的 **LAN** 地址 | 221 上取回的 .torrent 字节 |
| ④ | tracker 对 221 通告的 peer 就是 A 的 LAN 端点 | 从 221 发标准 announce 后解析 compact peer |
| ⑤ | `/scrape` 返回 200 | 221 上 curl |
| ⑥ | **COLD 负对照**：唤醒前客户端 Done=0% 且 `served_bytes=0` | 客户端计数器 + 供片方计数器 |
| ⑦ | 唤醒后 `state=hot` | `/cli seed` |
| ⑧ | 客户端报告 100% 且落盘 SHA-1 与样本一致 | 客户端 RPC + `sha1sum` |
| ⑨ | 供片方 `served_bytes` 覆盖全量（qB 阶段按增量算） | `/cli seed` |

**2026-11-17 实测（3 次连续复跑，18/18 一致）**：样本/落盘 SHA-1 均为 `66c5db8d…330c2`；
Transmission 阶段 `served_bytes=4194304`，qBittorrent 阶段增量 `4194304`（累计 8388608）。
报告落 `tests/e2e/w51c/reports/`（本地证据，gitignore）。

**本节抓到的两个真问题（都已处理）**：

1. **µTP 首连问题**：Transmission 默认优先用 µTP（UDP），当时服务端只开了 TCP，它**先 U 后 T**、
   回落很慢，现场表现是「TCP 连上了但没有字节流动」。抓包证据：`221.19501 > 210.18782
   UDP length 30`，全程没有发往 18782 的 SYN；关掉 µTP 后立刻 ESTAB 并下载完成。
   本节当时记的「librqbit 不支持 µTP」**是错判**——库自带 `librqbit-utp`，只是
   `ListenerOptions` 默认 `TcpOnly`，我们没打开。**W5.1e 已修正并补验**（见 §2.7），
   本节的客户端仍显式降到 TCP，用于单独验 TCP 路径。
2. **`/scrape` 返回 404**：标准客户端加种子后会 scrape，404 会让它持续报
   「Scrape error: 404」并退避。已实现标准 scrape 响应（std-only、按 query 里的二进制 info_hash 回
   `complete=1 / incomplete=0`），单测 2 条覆盖（含二进制 hash 不能被当字符串处理）。

**边界**：LAN（同一二层网段）、无公网报文、**`evidence_scope=lan-cross-host`、`is_s6_evidence=false`**；
不含任何可达率数字，也不构成 G2/G3 新证据。公网入向见 §2.6、µTP 见 §2.7、公网 µTP 见 §2.8、aMule 公网 KAD 见 §2.9、唤醒可达性决策见 §2.10。

### 2.6 W5.1d 公网入向复核（2026-11-17 新增，当前 PASS）
验证「外部主机能否越过互联网从 A 的引擎取片」。脚本 `tests/e2e/w51d/public_inbound.py`：

```
A        = 192.168.1.210（公网 egress 112.5.60.4，做种监听须为 0.0.0.0:17802）
外部主机 = 192.168.1.222（WAN 112.5.32.34，Transmission 3.00；它同时挂在 LAN 上）
样本 4 MiB；nat.bt_external / nat.http_external 由运维在 CPE 上真实建好后才可填
```

判据分两段，**第一段过不了就直接 BLOCKED，不进入客户端阶段**：

| 阶段 | 判据 |
| :--- | :--- |
| 映射探测 | 外部主机 `connect <A 公网IP>:17800` 与 `:17802` 均成功 |
| 配置自洽 | `/nat/public` 报 `engine_listen=0.0.0.0:*`、announce 两条、`inbound_verified=false` |
| 发现面 | 外部主机经公网取回种子（200 + 真实字节）；种子带 `announce-list`（LAN + 公网） |
| tracker | 外部主机 announce 后，返回的 compact peer 是 **公网端点**（不是 192.168.x） |
| 路径 | **A 侧 `bt_port` 上已建立连接的对端是公网 IP**（用系统连接表读，不看引擎自报） |
| 数据 | COLD 负对照 0 字节；唤醒后 100%、落盘 SHA-1 一致、供片方计数覆盖全量 |

**「两侧证据同时成立」是这条的关键**：222 同时挂在 LAN 上，只看「下载成功」会被内网捷径骗过。
脚本因此把种子换成**只有公网 announce** 的变体（同一个 info 字典换壳，InfoHash 不变，脚本内复核），
再从 A 侧连接表确认对端不是 RFC1918。

**2026-11-17 首次实测：BLOCKED（已解除）**。外部主机对 `112.5.60.4:17800` 与 `112.5.60.4:17802` 均
`TimeoutError`（不是 RST——是网关直接丢弃，符合「无 DNAT 规则 + 有状态防火墙」）。
本机核实过的旁路手段全不可用：默认网关 `192.168.1.254` 的 **UPnP / NAT-PMP / PCP 全部无响应**
（首次探测误用了 `192.168.1.1`，按真实网关重测仍为 false），本机**无全局 IPv6**。
因此 `status=blocked`，报告里列出需要的两条规则：

```
TCP 112.5.60.4:17800 -> 192.168.1.210:17800     # tracker / .torrent
TCP 112.5.60.4:17802 -> 192.168.1.210:17802     # BT 数据面
```

**2026-11-17 11:05 起（运维在 CPE 建好上述两条映射后）：PASS，17/17，累计 8 轮（最近连续 2 轮）**。
外部主机（222，WAN `112.5.32.34`）完整跑通「公网取种 → announce → 唤醒 → 取片」：

| 观测 | 值 |
| :--- | :--- |
| 端口探测 | `112.5.60.4:17800` / `:17802` 均 `OPEN` |
| 公网 `/health` | `200`，`node=A` |
| 伴随种子 | `200`，567 B；`announce-list` 两条（LAN + 公网），InfoHash `f59221cd…495fc206` 不变 |
| tracker 对公网请求方 | `112.5.60.4:17802`（不是 192.168.x），响应 62 B |
| COLD 负对照 | 客户端 `Done=0%`、`state=cold`、`served_bytes=0` |
| 唤醒后 | `seeding_hot=true`；客户端 `100%`；落盘 SHA-1 `4f1b8617…d69` 与样本一致 |
| 供片方计数 | `served_bytes` ≈ 4.19 MB（≥ 4 MiB 全量） |
| **路径** | A 侧 `bt_port` 连接对端 = `['112.5.32.34']`，LAN 对端 `[]` —— 不是内网捷径 |

`--probe-only` 只验端口、不拉客户端，用于映射刚建好时快速确认。

**这条链路慢**：4 MiB 实测 155–235 s（含首包前约 8 s），同环境 LAN 档是秒级。形态是稳定而非停滞
（逐秒百分比单调推进），但**不据此给出任何速率/可达率结论**（ADR-012）；成因未定位，登记为观察项。

报告落 `tests/e2e/w51d/reports/`（本地证据，gitignore）；`evidence_scope=public-inbound-single-egress`、
`is_s6_evidence=false`，**不含任何可达率数字**，不构成 S6/G2 证据。

**非通过轮次的归因（写清楚，避免把量具故障记成产品失败）**：映射建成后共跑 13 轮，8 轮 17/17，
另外 5 轮各有 1–3 项失败，逐条都能定位到 harness：`xxd -p` 过 Windows 控制台污染二进制（2 轮，
表现为「tracker 没有 peer」）、222 的 sshd 掐连接（3 轮，分别表现为 `/health` 不 200、`sha1sum`
拿不到 hash、轮询无输出）。修复后最近两轮均 17/17。

**harness 自身的坑（已修，不是产品问题）**：① 222 的 sshd `MaxStartups` 会把「命令没跑成」
伪装成业务失败——现在载体错误单独识别并重试（`Peer.run_ok`）、空响应带 `http_code/size/rc`
三元组、`sha1sum` 先辨 `__NOFILE__` 再判失败、两次连接间隔 ≥1.5 s、两个端口一次连完；
② 二进制 bencode 不再经 `xxd` 过控制台，改「远端落盘 + base64」；
③ 本机 Windows 上 ControlMaster 不可用（`ssh -f` 起的常驻 master 会被回收，`-O check` 反复失败，
反而 0.4 s→21 s 抖动），改用单次连接 + 分类重试，并把逐秒轮询塞进对端一条 ssh 里跑完；
④ 复位用 `-t all -r`（按 id 删在重试时会删错），且复位后必须确认只剩 1 个任务。

### 2.7 W5.1e µTP（BEP 29）互操作复核（2026-11-17 新增，PASS）

验证「默认配置的标准客户端能否直接走 µTP 取片」——也就是 §2.5 里那条被登记成「缺口」的行为。
脚本 `tests/e2e/w51e/utp_thirdparty.py`：

```
A        = 192.168.1.210（nat.utp 默认开；引擎 TCP + uTP 同端口监听）
下载方   = 192.168.1.221：Transmission 3.00（utp-enabled=true，默认行为）
                          qBittorrent 4.5.1（Session\BTProtocol=2，仅 uTP）
样本 4 MiB；`--clients transmission|qbittorrent` 可单跑，`--keep` 保留现场
```

判据**必须两侧同时成立**，任何一侧单独都不算：

| # | 判据 | 观测手段 |
| :--- | :--- | :--- |
| ① | 引擎请求了 µTP **且** UDP 端口真的被占 | 启动日志 `utp_requested` / `utp_udp_port_occupied` |
| ② | **UDP 端口的属主就是这个守护进程** | `netstat -ano -p UDP` 的 PID 与进程 PID 比对（不看自报） |
| ③ | 客户端**自报** peer 走 µTP | Transmission `peers[].isUTP`；qB `torrentPeers.connection` |
| ④ | 传输全程 A 侧**没有**来自 221 的 TCP 连接 | `Get-NetTCPConnection` 按 bt_port + 对端 IP 逐秒采样 |
| ⑤ | COLD 负对照仍成立 | 唤醒前客户端 0%、`state=cold`、`served_bytes=0` |
| ⑥ | 落盘 SHA-1 一致 + 供片方计数覆盖全量 | 客户端 `sha1sum` / `/cli seed` |

**2026-11-17 实测：PASS，20/20，连续 4 轮**：

| 观测 | Transmission 3.00 | qBittorrent 4.5.1 |
| :--- | :--- | :--- |
| 客户端自报传输层 | `utp=1, tcp=0`（`isUTP=true`） | `connection=μTP`（50 条采样里恒为 uTP） |
| 下载 | `100%`，6 tick | `100%`，2 tick |
| 落盘 SHA-1 | `8d5d0dea…309d72`（与样本一致） | 同 |
| 供片方增量 | `4194304` | `4194304`（累计 `8388608`） |
| A 侧 TCP 连接 | 8 次采样、0 次命中 | 同 |

**边界**：LAN（同一二层网段）、无公网报文；`evidence_scope=lan-utp-interop`、`is_s6_evidence=false`；
不含任何速率/可达率数字（ADR-012）。**公网 µTP 已在 §2.8 补验**（本轮只覆盖 LAN）。

**这一轮纠的是一年前的判断，不是代码**：`librqbit` 9.x 自带 `librqbit-utp`，
`ListenerOptions.mode` 默认 `TcpOnly`。把「默认关」读成「不支持」，才会得出「要自己写 µTP」的结论。

**harness 的坑（已修）**：qB 的 peer 列表只在连接存在时才有内容，4 MiB 在本环境 1–2 秒就下完，
「一次一条 ssh」的采样（自己 ~0.4 s/次）会整段错过传输窗口，读出来是「没有 peer」——
第一次就是这么把「走了 µTP」误判成「没走」的。改成在 221 本地起一个 0.1 s 间隔的采样进程、
事后取文件，才拿到 50 条样本。同类问题还有：`Session\BTProtocol` 是位掩码
（1=TCP、2=uTP、3=两者），之前 w51c 里写的 `0` 并不等于「仅 TCP」。

### 2.8 W5.1f 公网 µTP（BEP 29）复核（2026-11-17 新增，PASS）

把 §2.6（公网 TCP）与 §2.7（LAN µTP）拼起来。**拼不起来的点是映射**：CPE 上的两条 TCP 规则
对 UDP 无效，µTP 的入向是另一条独立的 `UDP <wan>:17802 → 192.168.1.210:17802`。
没有它，默认配置的客户端会先等 µTP 超时再回落 TCP——公网侧看起来就是「连上了但迟迟没数据」。
脚本 `tests/e2e/w51f/public_utp.py`：

```
A        = 192.168.1.210（nat.bt_external/http_external + nat.utp=true，监听 0.0.0.0:17802 TCP+UDP）
下载方   = 192.168.1.222（WAN 112.5.32.34）：qBittorrent 4.5.1，Session\BTProtocol=2（仅 uTP）
样本 4 MiB；`--probe-only` 只验三条映射；`--keep` 保留现场
```

判据**必须两侧同时成立**，另加一条与 µTP 无关的「量具」前置：

| # | 判据 | 观测手段 |
| :--- | :--- | :--- |
| ⓪ | CPE 上确有 **UDP** 公网映射 | 用**回显服务**（与 µTP 无关）单独验证：外部发 3 包，包从 `<wan>:17802` 原路返回 |
| ⓪b | 该映射没有走 LAN 捷径 | A 侧回显进程记录的源地址必须是 `112.5.32.34`（若为 `192.168.1.222` 则作废） |
| ① | A 的引擎在 BT 端口上真的绑了 UDP | `netstat -ano -p UDP` 的 PID 与守护进程 PID 比对（不看自报） |
| ② | 客户端物理上不可能走 TCP | `Session\BTProtocol=2`（仅 uTP） |
| ③ | 客户端**自报** peer 是公网端点且走 µTP | qB `torrentPeers`：`ip=112.5.60.4, port=17802, connection=μTP`（0.1 s 采样） |
| ④ | A 侧 bt_port 全程没有 TCP 连接 | 逐 0.3 s 采样 `netstat`，按对端 IP 分类：公网与 LAN 都必须为空 |
| ⑤ | COLD 负对照仍成立 | 唤醒前客户端 0%、`state=cold`、`served_bytes=0` |
| ⑥ | 数据确实是 A 给的 | 落盘 SHA-1 一致 + 供片方 `served_bytes` 增量覆盖全量 |

**2026-11-17 实测：PASS，25/25，连续 3 轮**（`run-20261117T1551/1555/1558`）：

| 观测 | 结果 |
| :--- | :--- |
| UDP 映射量具 | 3/3 回显；A 侧源地址 `112.5.32.34`（公网，非 LAN） |
| tracker 决策 | 对公网请求方回 `112.5.60.4:17802` |
| 客户端进度 | `progress=1.000000`（3 轮分别 143 / 153 / 147 次轮询，1 s/次） |
| 客户端自报 | 1473–1577 条采样，peer 恒为 `112.5.60.4:17802` / `connection=uTP`；**TCP peer 0 条** |
| 落盘 SHA-1 | `5118386a…f308a483`（3 轮均与样本一致） |
| 供片方增量 | `4194304`（基线 0 → 全量） |
| A 侧 TCP | 公网 `[]`、LAN `[]`（无 TCP，也就没有 LAN 捷径） |
| COLD 负对照 | 客户端 0%、`served=0` |

**边界**：单个出口（A 的家宽）；`evidence_scope=public-utp-single-egress`、`is_s6_evidence=false`；
不含任何可达率数字（ADR-012）。**观察项**：4 MiB 走公网 µTP 用 143–153 s，与 §2.6 的 TCP 档
（155–235 s）同量级——未见 µTP 额外恶化，成因仍未定位，不折算成吞吐结论。

**harness 三个坑（都已修，且都不是产品行为）**：
- 轮询最后一块尾部带着 `POLL_END`，直接 `json.loads` 会失败，**100% 那一格被静默丢掉**，
  读出来恒为 97%；
- 中断条件写成 `0\.99[0-9]*`，会在 **`0.992188`（16 片里还差两片）** 就 break——把「已完成」
  写成「没下完」，而同一轮的 SHA-1 其实已经一致；`int()` 截断也会把非 1 的收尾值读成 99%。
  现在只认 `progress=1`，百分比只用于显示；
- 复位只发一条 delete 就重加，qB 会重新校验残留文件并**断点续传 48 KiB**，
  于是供片增量恰好少 49152 B。现在必须先证明文件不存在（`test -e` → `GONE`）再重加。



### 2.9 W5.1g aMule 公网 KAD 索引面复核（2026-11-17 新增，PASS）

G3 的 `g3.amule_kad_index` 例外原先是「aMule 能经 eD2k TCP 送文件，但 KAD PUBLISH 静默」。
所有者补开 221 的公网入向 `UDP 4672` + `TCP 4662` 后，`tests/e2e/w51g/amule_wan_kad.py`
用真实 0x43/0x33 帧复核。**先量网络，再量 aMule 自己的判定状态**：

| # | 判据 | 观测 |
| :--- | :--- | :--- |
| ① | 公网 TCP 4662 可连，且 221 看到的源是 `112.5.32.34` | 从 222 连公网地址并保持连接；`ss` 对端地址核对（不是 LAN 捷径） |
| ② | 公网 UDP 4672 双向可达 | 221 上 tcpdump 过滤 `udp port 4672 and host 112.5.32.34`：入向 `112.5.32.34.x > 192.168.1.221.4672` 与 aMule 回包同时出现 |
| ③ | 公网侧 PUBLISH 被 ACK 且 SEARCH 取回本条 | `kad-mini wan-index-probe`；键贴着 `BOOTSTRAP_RES.self_id` 取（top32 距离 0），排除 `SEARCHTOLERANCE` 越界分支 |
| ④ | LAN 对照同样成立 | 同一 aMule 的 LAN 地址 `192.168.1.221:4672` 跑 `lan-index-probe` |
| ⑤ | 负对照 | 同一公网地址上未监听的 `4673` 跑同样帧，必须零回包 |
| ⑥ | 命名空间不串味 | `lan-ns-dual` 在**一个 socket** 内写两个 16B 键、逐键读回全部标签：自己的标签在、对方的标签不在 |

**2026-11-17 实测：PASS，20/20**（`run-20261117T091525Z`，公网 2 轮）：
`verdict=wan_confirmed_publish_then_retrieve`（2/2）、键距离 0、LAN `confirmed_publish_then_retrieve`、
`namespace_isolated`（两键写入 ACK、两次回查都有应答且 `other_label_present=false`）、
未监听端口 `reachable=false`、抓包两轮均见公网源入向与对应回包。

**关键观测，不是「加映射就好了」**：同一份 aMule 二进制、同一条路径，在**重启前**对 PUBLISH
一律静默（LAN 与公网都静默），`BOOTSTRAP_RES` 却正常回；重启后同样的帧立即被接受。
归因不是 NAT、不是 RC4 混淆、也不是键越界，而是进程内的「UDP 未被验证」状态未重判。
所以验收显式提供 `--restart-amule`（运维动作），并把「端口通」与「aMule 愿意入库」分成两条判据。

**量具坑（都已修）**：① tcpdump 按 `host:port` 冒号匹配，而实际输出是点分
`192.168.1.221.4672` → 明明抓到 46 行仍判 FAIL；② 抓包进程没用 `setsid nohup`，
ssh 退出会带走它，首轮 0 行；③ `-c 80` 被 aMule 自身的 KAD 流量填满，第二轮来不及抓我们的帧，
现改为按 `host 112.5.32.34` 过滤；④ 六个独立 socket 做 2 写 4 读会被 aMule 的 UDP 节流打断
（第 4 次查询稳定静默），且「按对方标签查本键」本就不是 KAD 语义——改为单 socket 双写、
每键一次查询后本地核对标签。

**边界**：单目标、经真实 NAT，但**不做公共 KAD 宣告、不扫网**；脚本输出
`g3_verdict=not_run`、`evidence_valid_for_pass=false`，结论由实验执行人回填；不含速率数字（ADR-012）。


### 2.10 W5.2 唤醒前查可达性（ADR-013 接线，2026-11-17 新增，PASS）

原语（`cr-nat::reach`、帧 0x08–0x0B、`--lookup`）早已交付，但 `wake_with_plan` 仍按静态顺序
先试直连、超时后才走中继。`tests/e2e/w52/reach_gated_wake.py` 用三台真实主机收口：

```
中继  192.168.1.221:7200（一次性实验 PSK；先升中继再升客户端）
持有者 222（ens160 192.168.34.222 / WAN 112.5.32.34，真实 symmetric），node_id=A
请求者 A（192.168.1.210 / WAN 112.5.60.4），node_id=B
```

判据必须两侧同时成立，且量具本身要有正对照：

| # | 判据 | 观测手段 |
| :--- | :--- | :--- |
| ① | 对端真是 symmetric | 222 上 `nat probe`：`nat_type=symmetric`，映射 `112.5.32.34:2062`（端口≠内部、来自公网出口） |
| ①b | 声明已传播且不污染抓包窗口 | `nat reach`（只查、不发包）轮询到 `declared=true` / `class=relay-only` 后才开抓包 |
| ② | 中继里确有声明 | `nat wake` 返回 `reach_lookup.declared=true`、`class=relay-only`、`skip_direct=true` |
| ③ | 决策真的改了候选 | `attempts` 只有 `kind=relay`，`path=relay`，拿到 `WAKEUP_ACK` |
| ④ | 零直连包 | 222 的 ens160 抓包 `udp and host 112.5.60.4`，主窗口 0 条 |
| ⑤ | 量具没有失灵 | 另起窗口 `only_kind=stun-mapped` 强制直连，同一抓包必须 >0 条且 `notes` 明说旁路 |

**2026-11-17 实测：PASS，21/21，连续 3 轮**（`run-20261117T100246Z/100339Z/100415Z`）。
`reach_lookup` RTT 13 / 22 / 16 ms、TTL 3600000 ms、`declared_at_ms` 为当轮真实探测时刻；
主窗口抓包 0 条，对照组抓到 2 条真实 UDP 报文（第 1 轮因请求侧自映射 STUN 抖动用了
3 次对照唤醒，后两轮各 1 次；每次结果都留在报告的 `control_attempts` 里）。
`only_kind` 旁路只在验收里出现，生产默认调用没有这个开关。

**这轮先抓到一个真产品缺陷，不是量具缺陷**：修复前 `nat reach` 明明查到 `relay-only`，
紧接着 `nat wake` 的查询却有 2/3 概率变回 `unknown`、把注定失败的直连又试一遍。
222 的 `holder.log` 显示订阅连接每 500 ms 读超时就 `订阅中断` → 1 s 后重连：
`recv_frame()` 把 `WouldBlock/TimedOut` 和真断链压成同一类字符串错误，订阅线程误判断链；
服务端在旧连接退出时 `forget_local` 删声明，于是中继在「1 秒空窗」里如实回答「不知道」。
修复两处：① `RelayConn::recv_frame_optional` 保留超时类型，订阅空闲超时时**不断链**、只在
声明真的变化时补发 `REGISTER_DECLARED`；② 服务端退出清理只在旧连接仍是当前注册表项时
才 `forget_local`，避免重连竞态抹掉新声明。修复后同一份 222 日志只有一次 `已订阅` +
一次 `可达性声明更新: relay-only`，无周期性重连。两条各有一条单测守着。

**边界**：`s6_status=not_run`、`is_s6_evidence=false`、`g3_verdict=not_run`，不写 G2/G3 结论，
不含速率/可达率数字（ADR-012）。本轮只证明「查到 relay-only 就不打洞」这一半；
「A 作为对端时查到 direct 并直连成功」仍由既有 W3.6/W5.1 系列证据覆盖，不在本脚本内重跑。

**harness 坑（已修）**：`pkill -f '<含 --listen>'` 会匹配到自己的 ssh 命令行把 shell 杀掉
→ 改用按实际进程名（Linux 截断到 15 字符 `coldrevive-rela`）的 `pkill -x`；
中继绑具体地址后 `127.0.0.1:7200` 必然拒绝，检查要连 `192.168.1.221`；
221 的原 Cargo.lock 不含本机新加的 `bytes`，整仓覆盖会触发 221 缺 `pkg-config` 的
`openssl-sys` 编译失败 → 只覆盖协议面相关源文件并 `touch` 强制重编。


**M0 前置**：[tests/spikes/](../../tests/spikes/README.md) 是 G0 的 Spike 实验包（S1–S8 判据、状态登记、结论回填规则），`m0_preflight.py` 只检查实验条件是否齐备，不产出任何 Spike 结论。截至 2026-11-13：S1 / S2 / S4 / S7 为 `pass`、S8 由项目所有者豁免登记为 `pass`（无书面意见）、S3 / S5 / S6 为 `not_run`；G0 已由所有者批准条件冻结，详见 [G0-REVIEW](../06-plan/G0-REVIEW.md)；`cargo` 已可用、S2 实网实验已获人工批准，最小 eMule KAD 客户端的真实 `0xE4` wire 本地回环与 LAN 真实客户端互操作探测均已跑通（均不构成 S2 结论），G0 参数与 bootstrap 清单已确认、判据口径已回写，**S2 已于 2026-11-10 判 `pass`**（`PUBLISH_RES load=1` + `SEARCH_RES` 命中，并由两进程 A/B 独立取回，详见 [PLAN.md §20–§21](../../tests/spikes/s2/PLAN.md)）；早期 0 响应系 IP 字节序（v2.4.14）与 Kad tag 帧头（v2.4.26）两处编码缺陷，非网络过滤；v2.4.7 已补齐 P4 前置实现（`0xE5` 解包、tolerance 收敛、跳过 HELLO、bootstrap 公网校验）；**v2.4.38 起第三方 BT 客户端（Motrix Next / aria2 1.36.0）、M1 二进制与 `CliAdapter.wired=True` 均已就绪，T0 `--mode real` 已实跑 21/21 PASS**。S3 / S5 / S6 仍为 `not_run`（G0 条件冻结豁免），T1 真实验收仍 `BLOCKED`，故 G1 未通过。
〔**以上结句为 2026-11-13 时点口径；2026-11-14 已更正为 G1 通过**（T0 21/21 + T1 real 11/11，均
`evidence_valid=true`），见本文开头更正与 §2.2 补记。**S3 / S5 / S6 状态仍 `not_run`**，G2 未达标。〕

**ADR-011 L1 / L2 探针证据（2026-11-11，不构成 T0/T1 或 Spike 结论）**：[S2 实验包](../../tests/spikes/README.md)
在 `127.0.0.1` 与 RFC1918 上另产出两组回归 —— `kad-mini`（`cargo test` 21/21、`selftest` 48/48、
`loopback` 发布 ACK 5/5 + 命中 5/5、`l2-source-probe` 在真实 aMule 上完成 `0x43`+`0x44` 双轨发布与
`0x33`+`0x34` 双轨取回）与 `ed2k-mini`（`cargo test` 31/31、`selftest` 26/26、
3/3 真实 TCP 回环完成下载/上传分片闭环、真实 eMule `HELLO` 握手成功）。
报告固定 `public_network_contacted=false` / `evidence_valid_for_pass=false`，**0 个公网报文**。
缺口依旧：该 LAN 对端共享文件数为 0，能力「从旧客户端下载」只能靠自两端回环证明，须在 M1
换真有共享文件的对端补测；`OP_SETREQFILEID (0x4F)` 的源码/客户端差异登记为开放观测。
详见 [PLAN §23](../../tests/spikes/s2/PLAN.md)。

### 2.11 2026-11-17 补跑与判定：S3 公网 key / S5 真实 tc/netem / S6 HighID

按项目所有者显式授权（`S3 可以向公网发 key 测试…s6 的一半也进入实测，s5 也真实测一下`），
三个残余证据缺口本轮都补了真实观测；随后由所有者依证据下判定
（[G2-SPIKE-VERDICT-2026-11-17](../06-plan/G2-SPIKE-VERDICT-2026-11-17.md)）：

| 项 | 真实证据 | 判定 |
| :--- | :--- | :--- |
| S3 公网 Mainline DHT | 两个独立进程、不同 node id；announce 宣告测试 infohash `f273a297…` 与 18081；query 不宣告，从公网 DHT 取回 `112.5.60.4:18081` | S3 = **`pass`**（只发 S3 测试 key；不代表 `cr-dht` 已实现） |
| S3 跨进程 RSS | 两个并发真实进程逐秒采样；峰值 Working Set 12.38 + 12.66 = **25.04 MiB**，Private 7.96 MiB | 同上（低于 NFR-05 的 100 MB；是观测，不是承诺） |
| S5 真实拥塞链路 | 221 上跨 netns veth + 内核 `htb 16mbit + netem 20ms`，RTT 19.9/27.0/41.0 ms；单条 BBR iperf3 会话内比较区间 | S5 = **`fallback`**：默认 ratio=0.02 前台劣化 **0.00%（3/3）**；ratio=0.2 中位 20% 不达标；ratio=0.05 有 1 轮 19.99%（未解释） |
| S6 HighID 一半 | A（112.5.60.4，cone）持有 + 222（112.5.32.34，symmetric）请求；30 次/路径真实 `WAKEUP_ACK` | S6 = **`pass`**（`closed_with_exceptions`）：direct **3/30**、relay **30/30**；两半 `complete=true` |

证据文件：`tests/spikes/s3/mainline_probe/reports/`、`tests/spikes/s5/utp_netem/reports/`、
`tests/spikes/s6/reports/highid-run-30/` 与 `tests/spikes/s6/reports/s6-matrix-aggregate-20261117-highid.json`
（均按各自 `.gitignore` 留作本地证据，不入库）。判定**不改写**报告里的
`evidence_valid_for_pass=false` / `auto_pass=false`；`g2_claim` 仍为 `false`——
三项 Spike 只回答技术路线，G2 仍欠 M2 实现项（`cr-dht`、LRU 调度接引擎、中继生产化、BT/TCP 打洞）。

## 3. 兼容性矩阵（必须全绿）

| 对端 | 场景 | 预期 |
| :--- | :--- | :--- |
| qBittorrent | 拿 magnet 连接被唤醒的 A | 正常下载，无需 ColdRevive |
| Transmission | 导入伴随 `.torrent` | 正常解析与下载 |
| BitComet | 标准 handshake + 长效种子共存 | 互不干扰 |
| eMule / aMule（P2） | ed2k 链接与 KAD 共存 | 不污染彼此命名空间 |
| AriaNg | 通过 `/jsonrpc` 管理任务 | 任务列表与速度正常 |
| 浏览器插件 | 推送下载链接到 ColdRevive | 任务创建成功 |
| 关闭 UI 窗口 | 后台下载/做种 | 不中断 |

客户端版本需在 `tests/fixtures/compat-matrix.toml` 中锁定。

**2026-11-16 W3.14 基线核验**：上表已逐行落到
[compat-matrix.toml](../../tests/fixtures/compat-matrix.toml)，
配套只读核验器 [compat_matrix.py](../../tests/e2e/g3/compat_matrix.py)
（`--self-test` 3/3 通过：脚本任何情况下都不会把 `g3_status` 写成 `pass`）。
本机实测清点：**8 行中 7 行 `blocked_no_client`**（qBittorrent / Transmission / BitComet /
eMule-aMule / AriaNg / 浏览器插件 / 关闭 UI），只有 Motrix Next（aria2 1.36.0）一行
有 S1 `cross_client_round` 的 128/128 片校验素材（`runnable_with_evidence`）。
这只是**可跑性与素材清点**，不是 G3 通过：矩阵仍要求全部行有真实互操作报告，
`G3.status` 保持 `not_run`，由实验执行人在客户端齐备后手工回填。

**2026-11-16 W3.15 第三方客户端实网互操作（附件证据，非 G3 结论）**：三台 LAN 主机
（221/222/209）的发行版原样客户端各完成一轮真实下载——我方 S1 seeder 在 A 生成标准单文件
`.torrent`，经 221 上 `local_tracker.py`（BEP23 compact，仅 RFC1918）做 peer 发现，
无 DHT/LSD/补丁：

| 行 | 客户端（远端实测版本） | 主机 | 结果 |
| :-- | :-- | :-- | :-- |
| Transmission | transmission-daemon 3.00 | 192.168.1.221 | 1 MiB 全量，SHA-1 匹配，seeder `uploaded_bytes=1048576` |
| qBittorrent | qBittorrent v4.5.1 (nox) | 192.168.1.222 | 1 MiB 全量，SHA-1 匹配，隔离干净（tracker 仅返回 seeder） |
| Motrix/aria2 | aria2-next | 192.168.1.209 | tracker `completed` + seeder `uploaded_bytes=1048576` |

证据：`tests/e2e/g3/reports/interop-bt-2026-11-16.json`（gitignored 本地取证），
Seeder 上传计数来自 `s1-cross-seed-17948240{73}/4418/4506.json`。
首轮 qBittorrent `peer_count_returned=4` 时 seeder `uploaded=0`，判为不干净并废弃重跑，
未混入结论。**仍有 5 行未覆盖**（BitComet / eMule-aMule GUI / AriaNg / 浏览器插件 / 关闭 UI），
G3 保持 `not_run`；证据只证明「我方标准 .torrent 可被零改造第三方客户端经标准 tracker 下载」。

**2026-11-16 W3.16 五行补充取证（附件证据，G3 仍未通过）**：在上轮三台 LAN 主机上继续驱动
未改造客户端，补齐 BitComet、aMule、AriaNg、浏览器插件、关闭 UI 五行。所有报告仍固定
`g3_verdict=not_run` / `evidence_valid_for_pass=false`；本轮只把「有没有真实素材」推进到
**8/8 行均有报告**，不把有报告等同于矩阵通过：

| 行 | 客户端 / 方式 | 本轮真实观测 | 证据 |
| :-- | :-- | :-- | :-- |
| BitComet | BitComet 2.23.1 Linux AppImage；Xvfb + WebUI `/panel/task_add_bt_result` 上传标准 `.torrent` | 1 MiB 全量，SHA-1 `f0f990bb…` 匹配，tracker `completed downloaded=1048576 left=0`，seeder `uploaded_bytes=1048576`；`Long-Term Seeds: 0 added, 0 received` | `bitcomet-20261116.json` |
| eMule / aMule | aMuleD 2.3.3，标准 eD2k TCP `4662`；ColdRevive `ed2k-mini download-from` | 256 KiB 全量，25 个 `OP_SENDINGPART`，独立重算 eD2k hash `19f7adef…` 与逐字节比较均通过（`diff_bytes=0`）；同一进程 `Kad: Connected (firewalled)` | `emule-amule-20261116.json` |
| AriaNg | AriaNg 1.3.15 + aria2-next 3.1.0 `/jsonrpc` | UI 显示 Connected；1 MiB 任务完成，tracker `completed`，seeder `uploaded_bytes=1048576`，SHA-1 匹配 | `ariang-20261116.json` |
| 浏览器插件 | MV3 取证插件（Edge headless）向 ColdRevive `/jsonrpc` 发 `aria2.addUri` | 任务由插件进程创建，`aria2.tellActive` 返回同一 GID 与完整 URI；未让示例链接真实下载 | `browser-extension-20261116.json` |
| 关闭 UI 窗口 | aria2-next 后台进程，无 AriaNg/WebUI 前端 | 任务由 `/jsonrpc` 创建后后台完成 1 MiB；中途采样 `524288 B @137458 B/s`，最终 SHA-1 匹配，seeder 与 tracker 双侧计数一致 | `ui-closed-20261116.json` |

> 报告文件说明：W3.16 的 `emule-amule-20261116.json` / `browser-extension-20261116.json` /
> `ui-closed-20261116.json` 三份已被 W3.17 的实测报告取代（`emule-coexist-*.json`、
> `browser-extension-<ts>.json`、`ui-closed-<ts>.json`）。报告目录是 gitignored 本地证据，
> 这些旧文件未入库、也未回填成新结论；历史结论以本表与 [CHANGELOG](../../CHANGELOG.md) 2.4.65 为准。

**边界（必须跟结论一起读）**：① BitComet 的 Long-Term Seed 模块虽启用，但本轮没有 LT seed 对端，`lt_seed_exercised=false`，只证明标准 handshake/tracker 路径可用，不宣称「长效种子共存传输」；
② aMule 的 KAD 在本轮保持 `Connected (firewalled)`，但未在同一轮同时发起 KAD 发布/搜索，
`namespace_crosstalk_tested=false`；命名空间隔离由 16B `Hash16` 类型与既有 S2 KAD 证据支持，
不把「结构上分离」写成「已完成同时双写压力测试」；③ 浏览器插件只验证任务创建，不验证内容下载；
④ 关闭 UI 行使用 aria2 后台进程，不是 BitComet/qBittorrent 的 UI 关闭行为。
核验器基线随之变为 **8 行有素材（5 `runnable_with_evidence` + 3 `evidence_collected`）、0 `blocked_no_client`**，
`g3_status` 仍为 `not_run`，`evidence_valid_for_pass=false`。完整报告在
`tests/e2e/g3/reports/`（gitignored 本地证据），由实验执行人在剩余边界补齐后手工判定。

**2026-11-16 W3.17 三条边界补做实（附件证据，G3 判据仍未回填）**：上一轮四条边界里三条可测的，
本轮用可复现 harness 做实；第四条（BitComet 长效种子）受环境限制，明确保留为未验证。

| 行 | 上一轮的弱在哪 | 本轮怎么补 | 结果 |
| :-- | :-- | :-- | :-- |
| eMule / aMule | KAD 只是「连着」，没有与 eD2k 传输同轮活动 | `emule_coexist.py`：同一轮里先起 KAD 轮（BOOTSTRAP/PUBLISH/SEARCH 真实帧），1 s 后并发真实 eD2k TCP 传输；重叠 30073 ms；期间用 EC 读 aMule 自述状态与上传速率 | 两条通道同轮共存：eD2k 262144 B / 25 块 / hash `19f7adef…` 全对；aMule `Kad: Connected (firewalled)` 传输前中后一致；传输窗口内 `Upload: 8.57 kB/s` |
| 关闭 UI 窗口 | 上一轮是「本来就没有 UI」，不是「UI 被关掉」 | `ui_closed.py`：headless Edge 打开真实 AriaNg（CDP 可查页面 URL），任务开跑并已有 376832 B 后**杀浏览器进程树**，验证 PID 消失 + 调试端口关闭 | 杀后字节继续涨到 1048576 并 complete，SHA-1 `f0f990bb…`，seeder `uploaded_bytes=1048576/1048576`、`external_peer_completed=true`，tracker `completed left=0` |
| 浏览器插件 | 推的是 `example.com` 占位链接，只能证明「登记」 | `browser_push.py`：MV3 模板按运行地址渲染（仓库里不再放占位链接），推一条真实 `.torrent`；`coldrevived` 的 `/jsonrpc` 真的取回并解析 | 任务 `gid=1a1e47831710000`，`coldrevive.torrentParsed=true`，info_hash `dbeabf89…` 与本脚本独立 bencode 复算一致，64 片 / 1048576 B |

**仍然是「未通过」的部分（不许折算成 pass）**：

- **BitComet 长效种子（LT Seed）**：`lt_seed_exercised=false`。该 build 的任务视图能读到
  `Long-Term Seeds: 0 added, 0 received in total`，即模块在跑、标准 handshake 不受影响；
  但要让它非零，必须有一个经 BitComet 自家分布式网络发现的第二个 LT 对端——那等于把测试
  infohash 宣告到公共 DHT。G3 harness 的纪律是 `public_network_contacted=false`，不做公网写入，
  因此本轮到此为止，该行的「长效种子共存传输」仍未验证。
- **aMule 的 KAD 索引面**：`kad_publish_acked=false`。aMule 自述 `Connected (firewalled)`，
  其 `Process2PublishKeyRequest` 对 firewalled 状态**静默丢弃** PUBLISH（源码级已知路径），
  本机实测也复现：BOOTSTRAP 900 ms 有回包（UDP 双向通）、PUBLISH 无回包、SEARCH 无回包。
  所以本轮只能说「两条通道共存且互不中断」，不能说「aMule 的 KAD 索引已验证入库/回查」。
- **插件仍不下载内容**：`coldrevived` 还没有 BT 下载引擎（W5.1 才用 rqbit 替换最小服务端），
  任务停在 `waiting`，报告里 `downloads_content=false`。
- **关闭 UI 行的 UI 是浏览器（AriaNg），不是 BitComet/qBittorrent 的桌面窗口**。

顺带修掉两个真缺陷：`/jsonrpc` 的 `aria2.tellStatus` 早先把 gid 取成 `params[1]`，
任务在队列里却回「任务不存在」（本轮被 harness 抓到，已修 + 端到端复验）；
`config/coldrevive.example.toml` 是 TOML，而 `Config::load` 走 `Json::load`，直接喂给守护进程
会报「非法 JSON 字符 '#'」——本轮 harness 改为按真实 schema 生成 JSON 配置。

**2026-11-16 W3.18 G3 结案（人工判定，带 6 条例外）**：所有者决定「先按现状结案，不再为补齐残余边界
追加环境改造；以后有机会再测」。结案记录落在
[g3-closeout.toml](../../tests/fixtures/g3-closeout.toml)，核验器只**转述**它
（`closeout.machine_inferred=false`），不据此改写自己的机器字段。因此 G3 现在有两个并行口径，
读数时必须分清：

| 口径 | 字段 | 值 | 含义 |
| :--- | :--- | :--- | :--- |
| 机器 | `g3_status` / `evidence_valid_for_pass` | `not_run` / `false` | 核验器没跑过互操作，无权主张通过；永久常量 |
| 人工 | `g3_status_declared` | `closed_with_exceptions` | 项目所有者 2026-11-16 结案：**带例外通过**，不是全绿 |

逐行判定（`accepted` = 场景已有真实观测支撑；`accepted_with_gap` = 主路径有观测、存在写明未验证处）：

| 行 | 判定 | 支撑 | 缺口 |
| :-- | :-- | :-- | :-- |
| qBittorrent | `accepted_with_gap` | 1 MiB 全量 + SHA-1 + 双侧计数 | 喂的是 `.torrent` 非 magnet；唤醒环节未参与 |
| Transmission | `accepted` | 种子文件导入 + 1 MiB 全量 + SHA-1 | — |
| BitComet | `accepted_with_gap` | 1 MiB 全量、tracker `completed`、seeder 计数 | LT Seed `0 added / 0 received` |
| eMule / aMule | `accepted_with_gap` | eD2k 262144 B / 25 块 / hash 全对；KAD 同轮并发 30 s | KAD 索引入库未验证；命名空间交叉污染未实测 |
| AriaNg | `accepted` | UI Connected + 1 MiB 完成 + 双侧计数 | — |
| 浏览器插件 | `accepted_with_gap` | 推真实 `.torrent`，ColdRevive 取回并解析出真 info_hash | 只到解析，不下载内容 |
| 关闭 UI 窗口 | `accepted_with_gap` | 杀浏览器进程树后继续到完成，SHA-1 + 双侧计数 | UI 是浏览器 AriaNg；数据面是 aria2 |
| Motrix/aria2（附加行） | `accepted` | 128/128 片校验 + 本轮 1 MiB | — |

**结案时明确不装作已解决的 6 条**（`[[exception]]`，`recheck_trigger` 写明何时可重测）：

1. `g3.wake_path_not_exercised` —— 全矩阵共同缺口：每行接收端都是「一直醒着的 A」，唤醒→连接这一环
   没参与。唤醒链路已在 G1 T0/T1 单独取证，不在此重复。
2. `g3.bitcomet_lt_seed` —— LT 计数非零需要把测试 infohash 宣告到 BitComet 公共 DHT，
   harness 纪律为 `public_network_contacted=false`，**需所有者放行**才做。
3. `g3.amule_kad_index` —— aMule 在 `firewalled` 下静默丢弃 PUBLISH，**需所有者开公网入向**：
   UDP 4672（KAD）+ TCP 4662（eD2k）；221 在 VMware 内，公网映射要落宿主再转 221，两跳都得通。
4. `g3.browser_extension_no_download` —— 依赖 W5.1 引擎接入。
5. `g3.ui_closed_ui_is_browser` —— 依赖 W5.1 + ColdRevive 自带 UI（W6.1/W6.4）。
6. `g3.namespace_pollution` —— 16B `Hash16` 与 20B infohash 的类型分离是既有 S2 结论，
   不是同轮污染实验；等 aMule 脱离 firewalled、KAD 索引可写后重测。

> 结案不含任何可达率、成功率数字，也不构成 SLA —— 与 [ADR-012](../02-architecture/adr/ADR-012-r04-best-effort.md)
> 的「best-effort、只提供一种可能性」口径一致。要重开 G3，把 `decision` 改成 `reopened`
> 并补齐对应 `recheck_trigger` 即可，矩阵与 harness 都可直接复用。

## 4. 性能压测场景与基线

| 场景 | 负载 | 通过标准 |
| :--- | :--- | :--- |
| 海量索引 | 10,000 文件（模拟元数据） | 空闲 RSS ≤ 100 MB，启动索引加载 < 10 s |
| 唤醒时延 | 并发 50 次唤醒 | P95 < 3 s（局域网），无失败 |
| 后台 QoS | 前台满速下载 + 50 个后台做种 | 前台劣化 ≤ 5%，上行 ≤ 阈值 |
| 造种 | 4 GB 文件 × 10 | 不阻塞 UI，CPU 优先级符合配置 |
| Soak | 72 h 连续运行 + 周期性唤醒/休眠 | 无内存增长趋势、无句柄泄漏 |
| 洪泛 | 1,000 req/s 伪造唤醒 | 服务存活，合法请求成功率 ≥ 95% |

## 5. 故障注入清单

需求来源：[FLOWS §8 异常与降级路径](../03-design/FLOWS.md)。

| 注入 | 期望 |
| :--- | :--- |
| 唤醒后节点离线 | 1.5 s 超时后切换候选，不阻塞 UI |
| 索引命中但文件被删 | 返回 `REJECT_FILE_GONE` 且撤销 KAD 发布 |
| 活跃池打满 | `REJECT_BUSY` 或 LRU 淘汰，无崩溃 |
| 内存/带宽越限 | 降级链逐层生效，RSS 不越硬限 |
| Symmetric NAT / CGNAT | LowID 中继回退，给出用户提示 |
| KAD 返回空 | 按 Mainline DHT → Tracker → 本地索引降级 |
| 伪造种子 / 分片投毒 | 校验失败丢弃 + 拉黑 + 信誉扣分 |
| 索引污染 / 重放攻击 | 限流与 nonce 去重生效，合法请求不受影响 |
| 造种队列积压 | 降并发、延迟发布，不阻塞前台 |

## 6. 各阶段质量门禁

| 门禁 | 位置 | 条件 |
| :--- | :--- | :--- |
| **G0** | M0 结束 | S1–S8 全部有结论，关键技术路线冻结（[TECH-STACK](../02-architecture/TECH-STACK.md)） |
| **G1** | M1 结束 | T0/T1 全链路闭环成功，受控环境复活成功率 100% |
| **G2** | M2 结束 | NFR-01/02/03 达标，NAT 矩阵通过率达标 |
| **G3** | M3 结束 | 兼容性矩阵全绿，NFR-04 达标 |
| **G4** | M4 结束 | 安全测试全通过，公测 KPI 达标，法务评审通过 |

## 7. 交付物

- 每次门禁输出：测试报告、指标快照（RSS/CPU/上行/唤醒时延 P95）、兼容性矩阵结果、未通过项与豁免说明。
- 性能与 Soak 用例必须输出原始采样数据，作为门禁证据归档。

## 相关文档

- [DOC-02 SRS 验收指标](../01-product/SRS.md)
- [DOC-07 FLOWS 异常降级表](../03-design/FLOWS.md)
- [DOC-13 RISK 风险登记册](RISK.md)
- [DOC-14 ROADMAP 里程碑与门禁](../06-plan/ROADMAP.md)
