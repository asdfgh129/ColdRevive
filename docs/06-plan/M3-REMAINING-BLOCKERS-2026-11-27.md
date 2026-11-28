---
doc_id: DOC-36
title: M3 剩余工程项与卡点清单（2026-11-27）
category: 计划
version: 1.5
status: 持续维护
owner: 架构组
updated: 2026-11-28
---

# M3 剩余工程项与卡点清单（2026-11-27）

> 口径：本文件只记录**状态、缺什么、需要什么才能关闭**，不改变任何门禁字段。
> `g2_claim=true`、三个工具字段 `s6_status=not_run` / `evidence_valid_for_pass=false` /
> `auto_pass=false`、EX-1 公平性 FAIL 78/80、`runtime_verified=false`、
> `counts_toward_nfr02=false` 全部保持原值；S6 仍为「条件计入」。

## 1. 已关闭 / 已推进项（v2.4.157～v2.4.160）

| 项 | 落地 | 验证 | 边界 |
| :--- | :--- | :--- | :--- |
| FR-25 双轨发布 | `cr_kad::dual_track_frame_keys` 把 A 轨 `MD4(UTF-8(term))` 并入 daemon 发出的 `KAD_PUBLISH` 帧；B 轨冻结的 `published_keys` / `inspect kad` 口径不变 | `cr-kad` 34/34；P0 双进程 e2e **77/77**（`tests/ARCHIVE/evidence/p0-dual-track-20261127.json`） | ACK ≠ 公网可检索；发布量翻倍按 R-02 在 M4 前复审 |
| W6.6 agent 看护内核 | `coldrevive-agent` 从只打印一行改为 std-only：spawn 子进程、`try_wait` 巡检、异常退出按预算重启、只读 `/health` 就绪日志 | agent 5/5 单测 + `tests/e2e/w66/agent_supervisor.py` 3/3 | **不是**托盘 / 浏览器捕获 / UPnP 续租；不监管 relay；不碰公网 |
| NFR-04 公网唤醒 P95（追加关闭） | `tests/e2e/nfr04-public/run.py` 在 221（家庭出口 112.5.60.4）与 119（腾讯云 1:1 EIP）各跑真实 `coldrevived`，仅 relay 唤醒候选，每侧 30 条，超时 12000 ms | A→C P95 **281 ms**、C→A **230 ms**，各 30/30、0 超时；强制 relay RTT 241/187 ms；两端 `/proc/<pid>/exe` 均为修复后二进制 `8556cd8b…`。证据 `tests/ARCHIVE/evidence/nfr04-public-20261127/` | 是 `WAKEUP_REQ→ACK` 唤醒时延，不是 BT 可达性/G2/S6；ACK≠公网可检索；两份 0 ms 作废报告同目录留存 |
| 打洞入向握手唤醒（v2.4.159，推进 2.2） | `wake_for_inbound_handshake` 把「解析 infohash → FR-15 准入 → `wake_seed` → 等 HOT ≤3 s」抽成 guard 与打洞**共用**的裁决；holder 改用 `DataPlane::attach_punched_inbound_waking`（先读 68 字节 → 唤醒 → 补回前缀再 splice） | `cargo test -p coldrevived --bin coldrevived punched` **5/5**；公网实测 BT 数据**首次**经打洞 socket 真实流动（1 MiB 落地 512 KiB） | **不是** W6.7 通过：落地 md5 不符、中途被 RST，仍 FAIL；不产出可达率 |
| 打洞登记可重入（v2.4.160，**关闭 2.2**） | `cr_bridge` 登记槽化：`take()` 保留键当占位、已就绪未消费的连接仍拒绝覆盖；新增 `SlotState`(Armed/InUse/Awaiting/Unknown) 与 `end_session()`；`take_for_connect()` 在补装窗口（6 s）里等生产者补 socket。配套：`tcp_punch` 退避加抖动、打洞窗口可配（默认 4 s） | 单测 `cr-bridge` **15/15**、`coldrevived punched` **6/6**；公网 daemon 级 e2e **1 MiB 落地 md5 一致**，`socks5_rearmed=1` 证明占位被重新填装后仍能服务 | 一条可用路径，不报成功率；重填装须成对触发且先重发现映射；H 侧线上字节仅入向 |

## 2. 仍开放的卡点

> **需要所有者拍板的四项，统一见 [OWNER-DECISIONS-2026-11-28](OWNER-DECISIONS-2026-11-28.md)**：
> 每条都有一句话问题、为什么卡在所有者、带利弊的选项、我的建议、不决定的后果。
> 本文件只描述「卡在哪」，不承担「怎么选」——避免两处口径不一致。

### 2.1 NFR-04 公网 P95 —— 已关闭（2026-11-27）

**先纠正一处此前的误判**：SRS 第 70 行写的是「唤醒 P95 < 3 s（**局域网**），公网 P95 < 8 s」，
T1 夹具的 `wake_p95_ms=3000` 只适用局域网，**与公网 8 s 不冲突**，不需要所有者裁决阈值。
此前记的「SRS 8 s 与 T0 夹具 3 s 口径未决」是把两个作用域混读，现予更正。

量具已就绪：`coldrevived` 的 `metrics wake-latency <n> [timeout_ms]` 可选超时
（缺省 1200 ms 不变，上限 60 s），公网采样应显式给 `>8000` 的超时，否则超时样本会把
P95 截在 1.2 s 而失去意义。

**已实测**：`tests/e2e/nfr04-public/run.py` 用 221（家庭宽带出口 `112.5.60.4`）与
119（腾讯云 1:1 EIP `119.29.150.254`）两台不同公网网络的真实 `coldrevived`，
只给对端通告 relay 候选，先 `wake raw` 预热，再每侧 30 条
`metrics wake-latency 30 12000`。A→C P95 **281 ms**、C→A **230 ms**，
各 30/30、0 超时，强制 relay 锚点 RTT 241/187 ms；两端运行二进制 SHA-256
与阶段二进制一致（`8556cd8b…`）。归档 `tests/ARCHIVE/evidence/nfr04-public-20261127/`
（含 `MANIFEST.json` 与两份作废报告）。

**作废报告必须同读**：早先 `0 ms` 不是「公网零延迟」，先后是 relay 分支硬编码 RTT、
以及 systemd 仍运行旧 workdir 二进制（`cp` 报 `Text file busy` 未被脚本拦截）。
两版均已归档为 `report-zero-rtt-invalid.json` / `report-stale-binary-invalid.json`；
修复含真实 `Instant` 计时、先停旧进程再换二进制、校验安装/运行哈希、零样本判 INVALID。

**禁止**：把本机回环 / 同一局域网的结果写成公网 P95；把超时样本丢弃后只算成功样本；
把该 P95 写成 G2/S6/NAT 可达证据。

### 2.2 BT-over-punched-TCP daemon 级实测 —— 已关闭（2026-11-27 22:56，单条可用路径）

**2026-11-27 21:34 环境变更**：所有者按提示在 `119.29.150.254` 上放通了 `TCP:19720`。
这台机器是**轻量应用服务器（Lighthouse，`lhins-cyayca13`，广州）**，不是 CVM，所以规则在
「实例 → 防火墙」里，不在 VPC 安全组里——之前「CVM 实例列表为空」就是这个原因，不是权限
问题。`82.157.68.159` 同样是轻量（北京 `lhins-dmotjt9m`）。放通后实测：从 221 发起
`TCP/19720` 得到完整握手，payload `PUNCH-PROBE-OK-19720` 到达 `10.1.0.2`。

**revision 2（v2.4.159）修掉的真缺口**：`nat punch-route` 的 holder 分支只 splice，而 guard
路径有「68 字节握手命中冷种子就唤醒」。冷种子不应答入向握手，现场是握手到达（tcpdump
`length 68`）、引擎 10 s 不回、对端 FIN、holder 回 `R.`。裁决现抽成
`wake_for_inbound_handshake`（infohash → FR-15 准入 → `wake_seed` → 等 HOT ≤3 s），guard 与
打洞两条入向路径共用；holder 改用 `attach_punched_inbound_waking`（读 68 字节 → 唤醒 →
补回前缀再 splice）。该轮数据**首次**真实经打洞 socket 流动，但只落地 512 KiB、md5 不符。

**revision 3（v2.4.160）做了什么** —— 关闭的就是 rev2 剩下的第二条条件「让登记可重入」：

1. **`cr_bridge` 登记槽化**：`HashMap<addr, Upstream>` 改成 `HashMap<addr, Slot>`。
   `take()` 只取走连接、**保留键**当占位，于是同一个目标可以重新填装；已就绪但未被消费的
   连接仍然拒绝覆盖（不丢掉活连接）。新增 `SlotState`（`Armed` / `InUse` / `Awaiting` /
   `Unknown`）与 `end_session()`：会话结束才转 `Awaiting`，避免把「正在用」误报成「待补装」。
2. **CONNECT 会在补装窗口里等**：`take_for_connect()` 对登记过的目标最多等 6 s
   （`PUNCHED_REARM_WAIT`）等生产者补一条新 socket，而不是立刻去拨一个只在打洞存活期间
   有效的公网映射。未知目标仍然立刻直连——那不是桥的责任。
3. **打洞重试去共振**：`tcp_punch` 的固定 60 ms 退避改成 `60 ms + 0..90 ms` 抖动。两侧跑
   同一份实现时固定退避会锁死相位：一条 socket 收到对端 RST 后只活十几毫秒，相位一旦错开
   就永远错开（rev2 后第一次复跑：8 s / 16 轮，两端每轮都是 `Connection refused`）。
4. **打洞窗口可配**：`punch-route` 第五个参数（300–15000 ms，默认 4000），替代写死的 1200 ms。
5. **量具修正**（都在 harness 里，属测量问题不是产品问题）：
   - M 侧 SSH 建连移到 barrier **之前**。原来在线程里现连，M 比 H 晚 200–400 ms 才发包，
     两侧 SYN 窗口根本对不上——这是「同时打开」被量具做没了。
   - **重打洞前重新发现映射**。一次会话结束后家宽 NAT 的映射可能换源端口，继续用旧端口会让
     holder 对着没人等的 4-tuple 发 SYN（现场：H 27 轮静默、M 每轮 `Connection refused`）。
   - **下载目录每轮清空并断言为空**。这条最要命：上一轮留下的 `sample.bin` 会让 librqbit
     直接校验通过、报 `complete` 且 md5 相符，但一个字节都没传（当晚两份 PASS 因此作废，
     数字见 MANIFEST 的 `voided_runs`）。

**revision 3 实测结果（`tests/ARCHIVE/evidence/w67-punched-tcp-20261127-r3/`）**：

- 下载目录两端实测为空（`0` 个文件）；映射两次采样一致；
- 首次打洞 **1 次尝试**成功，两端 `route=punched-direct`、`tcp_punch_established=true`、
  `punched_socket_registered=true`、`direct` 计数 +1；
- 传输中途停滞 → harness 触发 1 次「重发现 + 同时重打洞」，再次两端 `punched-direct`；
  `socks5_rearmed=1`，**桥把占位重新填装并继续服务同一条 BT 会话**；
- 最终 **1 048 576 / 1 048 576 B** 落地，`md5 6209e962f3b26a13c9ec1ae70fd78e10` 与源文件一致，
  aria2 `status=complete`、`connections=0`；
- member 侧打洞端口线上 payload **1 327 246 B / 1007 帧**（最大帧 2824 B），是真实 BT 数据，
  不是中继：两端 `tunnels=0`、`holder punched_alive=1`、member `served_registered=1`。

**边界（必须一起写）**：

- 这是**该类环境下有一条可用路径**，按 ADR-012 **不报成功率/可达率**；
- 重填装**必须成对触发且先重发现映射**：单侧无法自愈，holder 也不能继续拨旧端口；
- 一条打洞 socket 仍然只服务一次 CONNECT，这是 TCP 的物理约束，不是缺陷；可重入解决的是
  「键还能不能再被填装」，配套的补装动作由控制面负责；
- 本报告里 H 侧线上字节数只统计了入向（过滤器 bug，事后已修），应读 M 侧接收数；
- 不是 G2/S6/NFR-02 证据；门禁字段一个未动。

### 2.3 LEDBAT / 公平性 EX-1 —— 所有者已选 A（2026-11-28），四项已全部落地

EX-1 仍为 **FAIL**（80 轮中 78/80，cc_on/paced 两轮 0.430/0.570、0.433/0.567）。
2026-11-28 09:18 所有者对 DOC-35 决定 1 选 **A**：实现 LEDBAT + 新 ADR + 新判据 + 重采。
执行顺序按 EX-1 C4 固定，不可倒置：

1. ✅ **ADR-025**（`ADR-025-ledbat-congestion.md`）：cwnd 由排队时延驱动，丢包只重传不降窗，
   连续 RTO 6 次塌陷复位；AIMD 原样保留为 `CcMode::Aimd`。
2. ✅ **新判据预注册**（`tests/fixtures/g2-fairness-thresholds-v2.toml`，
   `recorded_at=2026-11-28T09:20:38+08:00`，sha256 `4d5d931e…`）：阈值与 v1 **逐字一致**，
   只改被测机制与证据要求（补 provenance 三项 + 逐轮原始目录必入库）。
   旧判据 sha `0671a173…` 未删未改。
3. ✅ **实现**：`CcMode::Ledbat` + `ledbat_target`/`ledbat_gain`；`--cc-mode` 探针开关；
   `nat.udp_cc_mode`（`aimd|ledbat`，非法值报错）；`cr-nat` 24/24（新增 6 例）。
4. ✅ **重采（2026-11-28 02:04 UTC 完成）**：`tests/e2e/w61/run_g2_fairness.sh` 加三个
   ledbat arm + provenance + 判据原文落盘；221 真内核 veth/HTB/netem，5 arm × 2 shape ×
   10 轮 = 100 轮，**0 轮失败**。L2（8 Mbit/s / 50 ms / jitter 5 ms）判定面：
   `ledbat` 10/10（份额 0.496–0.504，Jain ≥ 0.9999）、`ledbat_paced` 10/10（0.490–0.510，
   Jain ≥ 0.9996）、`ledbat_on_per_session_cap` 10/10（0.499–0.501）、`cc_off` 10/10；
   **参考臂 AIMD（cc_on）8/10**（0.429–0.571），与 v1 的 78/80 同源。
   总体 `criterion_met=true`（4/4 判定组合）。
   证据 `tests/ARCHIVE/evidence/g2-fairness-v2-20261128/`（summary.json + 809 个逐轮原始文件
   打包 + PROVENANCE-NOTE）。

   **边界（写在一起，不许拆开引用）**：
   - 这是**单机 netns + 真内核 HTB/netem** 的结果，不是公网、不是跨主机；
   - AIMD 的 8/10 是**参考臂**，不参与 v2 判定，但也不能拿来当「v2 全过」的装饰；
   - **EX-1 的 v1 判定仍是 FAIL 78/80，未改写**；本轮履行的是 C4 的「新判据 + 新样本 +
     provenance」义务，是否调整 G2 记账口径属 G2 复评，不在本轮动；
   - 措辞红线：**不得**写「公平性达标／通过」（EX-1 C5）；
   - 欠账一条：`git_commit` 是事后 blob 比对推导（harness 在无 `.git` 目录里写了 unknown），
     下一轮采样必须换成「干净 clone 或源码树哈希」，见 PROVENANCE-NOTE。

不得调 β、不改现有阈值、不复用 sha `0671a173…` 重跑后宣称 PASS。

### 2.4 W6.6 —— 所有者已选 B（2026-11-28）：纯 Web UI，托盘三件移出

原「缺 GUI 依赖决策」已决：**不引入 Tauri、不做托盘、不做浏览器下载捕获、不做 UPnP 续租**。
W6.6 现口径 = `coldrevive-agent` 看护内核 + 浏览器页面（纯 Web UI）。

- 界面：`GET /` + `/ui/app.js` + `/ui/style.css`，状态走 `/ws` 每秒推送（ADR-008），
  **不轮询**（旧实现 `setInterval(5000)` 已去掉，并加了单测禁止回退）。
- 断线必须显示「已断开」并退避重连，不拿旧数字充数。
- 托盘 / 捕获 / UPnP 三项在 `ROADMAP` 标为 **M4 再评估**；不再作为 M3 的未完成项挂账。
- 仍**不得**写「三进程模型已完成」——本轮的第三件是「可关的浏览器页面」，不是托盘进程。

### 2.5 FR-25 公网侧 —— 所有者已授权（决定 3 选 B），机制已建、默认关闭

- `announce_peer` 到公共 DHT 现在受闸门约束：`dht.announce_public` **默认 false**；
  打开后仍受 `public_max_per_min`（默认 4）与 `public_daily_cap`（默认 200/UTC 日）双重配额；
  运行时可 `coldrevived /cli dht announce-off` 立即关闭，`announce-on` 在配置未授权时**必须失败**。
- 只对**公共地址**过闸门；回环/私有/CGNAT/链路本地不算公共目标（自建测试节点不受影响）。
- 审计留痕：每次放行与拒绝都记时间 + infohash + 原因（有界环形缓冲，`/health.dht.public_announce`）。
- **本轮不产生任何公共 DHT 可达性证据**；R-02 的「双轨发布量翻倍需在 M4 前用实测拒绝率复审」
  仍未做，仍未解除。授权 ≠ 已发布，≠ 已验证。

## 3. 下一轮建议顺序（2026-11-28 更新）

1. 2.1（NFR-04 公网 P95）与 2.2（BT-over-punched-TCP）均已关闭，证据分别在
   `tests/ARCHIVE/evidence/nfr04-public-20261127/` 与
   `tests/ARCHIVE/evidence/w67-punched-tcp-20261127-r3/`。
   2.2 的关闭口径是「该类环境下有一条可用路径」；若后续要在更多 NAT 形态上复现，
   必须重新预注册判据，不能把本轮结果外推成成功率。
2. 2.3 已在 2026-11-28 09:18 由所有者签 **A**（DOC-35 决定 1），ADR-025 → 新判据预注册
   → LEDBAT 实现 → 采样四步全部完成；EX-1 v1 判定仍是 **FAIL 78/80**，不得写「公平性达标」。
   G2 记账口径是否调整属 G2 复评，不由本单自动生效；下一轮采样必须补 `git_commit` 真实值，
   v2.4.163 已修 harness，**旧证据不回写**。
3. 2.4 已签 **B**（纯 Web UI），托盘 / 浏览器捕获 / UPnP 续租从 M3 移出，M4 再评估；
   2.5 已签 **B**（授权机制但默认关），闸门已交付，`R-02` 拒绝率复审仍未做；
   macOS 决定 4 **不动**，维持 5 pass / 1 not_verified 且不计入 NFR-02。
4. 五项决定都不挡已关闭的门。下一轮工程重点是：R-02 复审准备、G2 记账口径复评；
   M4 范围已按 [DOC-35](OWNER-DECISIONS-2026-11-28.md) 决定 5 = A 锁定为 G4 核心，
   托盘 / 浏览器捕获 / UPnP 续租不纳入交付。详见
   [DOC-38 M4 范围与 R-02 复审启动](M4-SCOPE-2026-11-28.md)。
