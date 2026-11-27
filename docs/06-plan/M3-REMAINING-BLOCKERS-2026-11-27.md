# M3 剩余工程项与卡点清单（2026-11-27）

> 口径：本文件只记录**状态、缺什么、需要什么才能关闭**，不改变任何门禁字段。
> `g2_claim=true`、三个工具字段 `s6_status=not_run` / `evidence_valid_for_pass=false` /
> `auto_pass=false`、EX-1 公平性 FAIL 78/80、`runtime_verified=false`、
> `counts_toward_nfr02=false` 全部保持原值；S6 仍为「条件计入」。

## 1. 本轮（v2.4.157）关闭的两项 + 追加关闭的 NFR-04

| 项 | 落地 | 验证 | 边界 |
| :--- | :--- | :--- | :--- |
| FR-25 双轨发布 | `cr_kad::dual_track_frame_keys` 把 A 轨 `MD4(UTF-8(term))` 并入 daemon 发出的 `KAD_PUBLISH` 帧；B 轨冻结的 `published_keys` / `inspect kad` 口径不变 | `cr-kad` 34/34；P0 双进程 e2e **77/77**（`tests/ARCHIVE/evidence/p0-dual-track-20261127.json`） | ACK ≠ 公网可检索；发布量翻倍按 R-02 在 M4 前复审 |
| W6.6 agent 看护内核 | `coldrevive-agent` 从只打印一行改为 std-only：spawn 子进程、`try_wait` 巡检、异常退出按预算重启、只读 `/health` 就绪日志 | agent 5/5 单测 + `tests/e2e/w66/agent_supervisor.py` 3/3 | **不是**托盘 / 浏览器捕获 / UPnP 续租；不监管 relay；不碰公网 |
| NFR-04 公网唤醒 P95（追加关闭） | `tests/e2e/nfr04-public/run.py` 在 221（家庭出口 112.5.60.4）与 119（腾讯云 1:1 EIP）各跑真实 `coldrevived`，仅 relay 唤醒候选，每侧 30 条，超时 12000 ms | A→C P95 **281 ms**、C→A **230 ms**，各 30/30、0 超时；强制 relay RTT 241/187 ms；两端 `/proc/<pid>/exe` 均为修复后二进制 `8556cd8b…`。证据 `tests/ARCHIVE/evidence/nfr04-public-20261127/` | 是 `WAKEUP_REQ→ACK` 唤醒时延，不是 BT 可达性/G2/S6；ACK≠公网可检索；两份 0 ms 作废报告同目录留存 |

## 2. 仍开放的卡点

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

### 2.2 BT-over-punched-TCP daemon 级实测 —— 仍 BLOCKED（缺可直绑的公网 TCP 端口）

2026-11-27 用 119（公网 1:1 EIP，holder）与 221（家庭 EIM NAT，member）重跑
`tests/e2e/w67-punched-bt/run.py`。代码 handoff 已补齐：holder 侧新增
`DataPlane::attach_punched_inbound`，`nat punch-route` 按 `role` 分别走
holder splice / member SOCKS5 注册，并有角色与安全回退单测；调用方
`/proc` 哈希、peer 通告、真实 BT 任务与 md5 全部取证。

本次结果 **BLOCKED**（`tests/ARCHIVE/evidence/w67-punched-tcp-20261127/`）：

- 两观测点一致：221 从 `192.168.1.221:19710` 映射为 `112.5.60.4:23982`，`eim=true`；
- 两侧 `punch-route` 均因 `TCP 打洞未建立（poll+SO_ERROR 判据）` 回落 relay；
- 根因是环境而非代码，两级：
  1. **端口对称性**（决定性）：TCP 同时打开要求两侧用**同一对端口**互发 SYN——A 从
     `pa` 发往 `pb`，B 必须从 `pb` 发往 `pa`。本环境 holder 只能绑 `19720`、member 只能拨
     `119.29.150.254:22`，holder 出向经 1:1 EIP 保端口后对端看到的是 `:19720`，
     与 member socket 的 remote `:22` **不是同一条连接**，内核只能回 `R.`。
     这不是 `REDIRECT` 写法问题——把入向 `22` 改写成 `19720` 只会让 member 拨的端口
     和 holder 的公开源端口永久错位；
  2. **可达性**：119 / 82 的安全组只放通 TCP/22（实测其余端口 3 s 超时=过滤，
     不是 refused），而 `22` 由 `sshd` 占着（`ss -lntp` 见 `0.0.0.0:22` sshd pid 1049），
     punch socket 无法绑到「对端唯一能拨到」的那个端口。
- tcpdump 实录：H→M 的 SYN 进入 `112.5.60.4:23982`，M→H 的 SYN 打在 `119.29.150.254:22` 后被 `R.`；
- 侧记（操作风险）：源地址限定 `-s 112.5.60.4 --dport 22` 的 REDIRECT 会**同时劫持
  本机控制通道**，因为控制台与被测 member 共用一个家宽出口 IP，无法用源 IP 区分；
  本次一度失联，靠 82 跳板机 `-D` 撤销规则才恢复。下次这类实验必须用独立出口做控制端。
- 报告中 `member_bridge_served` / `holder_splice_done` / `engineAttached` / infohash 命中
  是**回落 relay 数据面**的计数，不是 punched-direct，不可写成通过；
- 文件落地为 0 B、`md5` 不一致，未通过 BT 落地判据。

关闭条件：需要一台**安全组放通、且该 TCP 端口未被 sshd/其他服务占用**的公网主机，
让 punch socket 直接绑到「对端会拨的那个端口」上；或用持有者书面认可的等效真实公网
TCP 路径。不得用 `REDIRECT` 冒充直绑（端口对称性无法靠改写绕过），不得用静态
DNAT／中继顶替 punched-direct。

按 ADR-012 **不报成功率/可达率**，只写「该类环境有一条可用路径」与失败原因。
本机、回环、静态 DNAT（`punched-direct` 历史 3/30）都不能顶替这项。

### 2.3 LEDBAT / 公平性 EX-1 —— 缺新判据，不是缺重跑

EX-1 仍为 **FAIL**（80 轮中 78/80，cc_on/paced 两轮 0.430/0.570、0.433/0.567）。
按既有书面例外，重测只有三种正当触发：端侧机制真改 / 链路形状或物理环境变化 / 所有者
重新预注册判据。LEDBAT 目前**没有实现**，所以「实现 LEDBAT 后重测」必须：

1. 先写新 ADR 说明拥塞语义改变；
2. 再登记新判据文件与新 sha（新时间戳）；
3. 才采样，并补齐 `git_commit` / `kernel_release` / `tc_qdisc_and_class` 与逐轮原始目录。

不得调 β、不改现有阈值、不复用 sha `0671a173…` 重跑后宣称 PASS。

### 2.4 W6.6 托盘 / 浏览器捕获 / UPnP 续租 —— 缺 GUI 依赖决策

本轮只交付无 GUI 的看护内核。剩余部分需要：

- 明确托盘实现路线（ADR-003 提过 Tauri 外壳或浏览器页面）并引入新依赖；
- 浏览器下载捕获的合法接口与权限边界；
- UPnP 续租的可用环境（S6 已实测本机 IGD / NAT-PMP 不可用，不能假装能测）；
- 打包口径与 systemd/launchd 集成。

在这些决定之前，不得把 W6.6 写成「三进程模型已完成」。

### 2.5 FR-25 公网侧 —— 缺 R-02 复审与公网证据

双轨 key 已进本地发布帧，但：

- 按运营速率的双轨发布量翻倍，R-02 要求 M4 前用实测拒绝率复审；
- 公网 server/peer 的 FR-25 传输证据仍是历史 BLOCKED 项；
- `announce_peer` 到公共 DHT 仍需所有者明确授权，不能因为本轮本地接线而视作已授权。

## 3. 下一轮建议顺序

1. 2.1 已关闭；2.2 需在第二台公网机上提供真实可直绑 TCP 端口后重跑，不再用 22 端口 REDIRECT。
2. 2.3 属于独立课题，先 ADR 后判据，不与 2.2 混跑。
3. 2.4 需要所有者选 GUI 路线；2.5 等 R-02 复审窗口。
