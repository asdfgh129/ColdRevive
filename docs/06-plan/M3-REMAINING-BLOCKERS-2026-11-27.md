# M3 剩余工程项与卡点清单（2026-11-27）

> 口径：本文件只记录**状态、缺什么、需要什么才能关闭**，不改变任何门禁字段。
> `g2_claim=true`、三个工具字段 `s6_status=not_run` / `evidence_valid_for_pass=false` /
> `auto_pass=false`、EX-1 公平性 FAIL 78/80、`runtime_verified=false`、
> `counts_toward_nfr02=false` 全部保持原值；S6 仍为「条件计入」。

## 1. 已关闭 / 已推进项（v2.4.157～v2.4.159）

| 项 | 落地 | 验证 | 边界 |
| :--- | :--- | :--- | :--- |
| FR-25 双轨发布 | `cr_kad::dual_track_frame_keys` 把 A 轨 `MD4(UTF-8(term))` 并入 daemon 发出的 `KAD_PUBLISH` 帧；B 轨冻结的 `published_keys` / `inspect kad` 口径不变 | `cr-kad` 34/34；P0 双进程 e2e **77/77**（`tests/ARCHIVE/evidence/p0-dual-track-20261127.json`） | ACK ≠ 公网可检索；发布量翻倍按 R-02 在 M4 前复审 |
| W6.6 agent 看护内核 | `coldrevive-agent` 从只打印一行改为 std-only：spawn 子进程、`try_wait` 巡检、异常退出按预算重启、只读 `/health` 就绪日志 | agent 5/5 单测 + `tests/e2e/w66/agent_supervisor.py` 3/3 | **不是**托盘 / 浏览器捕获 / UPnP 续租；不监管 relay；不碰公网 |
| NFR-04 公网唤醒 P95（追加关闭） | `tests/e2e/nfr04-public/run.py` 在 221（家庭出口 112.5.60.4）与 119（腾讯云 1:1 EIP）各跑真实 `coldrevived`，仅 relay 唤醒候选，每侧 30 条，超时 12000 ms | A→C P95 **281 ms**、C→A **230 ms**，各 30/30、0 超时；强制 relay RTT 241/187 ms；两端 `/proc/<pid>/exe` 均为修复后二进制 `8556cd8b…`。证据 `tests/ARCHIVE/evidence/nfr04-public-20261127/` | 是 `WAKEUP_REQ→ACK` 唤醒时延，不是 BT 可达性/G2/S6；ACK≠公网可检索；两份 0 ms 作废报告同目录留存 |
| 打洞入向握手唤醒（v2.4.159，推进 2.2） | `wake_for_inbound_handshake` 把「解析 infohash → FR-15 准入 → `wake_seed` → 等 HOT ≤3 s」抽成 guard 与打洞**共用**的裁决；holder 改用 `DataPlane::attach_punched_inbound_waking`（先读 68 字节 → 唤醒 → 补回前缀再 splice） | `cargo test -p coldrevived --bin coldrevived punched` **5/5**；公网实测 BT 数据**首次**经打洞 socket 真实流动（1 MiB 落地 512 KiB） | **不是** W6.7 通过：落地 md5 不符、中途被 RST，仍 FAIL；不产出可达率 |

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

### 2.2 BT-over-punched-TCP daemon 级实测 —— 传输通道已打通，仍未通过落地判据

**2026-11-27 21:34 环境变更**：所有者按提示在 `119.29.150.254` 上放通了 `TCP:19720`。
需要注意的是这台机器是**轻量应用服务器（Lighthouse，`lhins-cyayca13`，广州）**，不是 CVM，
所以规则在「实例 → 防火墙」里，不在 VPC 安全组里——之前「CVM 实例列表为空」就是这个原因，
不是权限问题。`82.157.68.159` 同样是轻量（北京 `lhins-dmotjt9m`）。放通后实测：
从 221 发起 `TCP/19720` 得到完整握手，payload `PUNCH-PROBE-OK-19720` 到达 `10.1.0.2`。

**本轮（revision 2）做了什么**：`tests/e2e/w67-punched-bt/run.py` 删掉全部 `REDIRECT`，
改为 holder 直接绑 `0.0.0.0:19720`、member 拨 `119.29.150.254:19720`；观测点就设在这同一个
端点上，两次采样必须一致（`36584→…→36883`，两次相同），不做跨目的地外推。
两个 BT 阶段的问题也在本轮定位并修掉：

1. **冷种子不响应入向握手**（真代码缺口，v2.4.159 修复）。`nat punch-route` 的 holder 分支
   只做 splice，而 guard 路径有「握手命中冷种子就唤醒」。结果是 68 字节 BT 握手**到达了**
   holder 的打洞 socket（tcpdump `21:42:55.525 ... length 68`），引擎 10 秒内一个字都不回，
   member 发 FIN、holder 回 `R.`。修法是把这个裁决抽成 `wake_for_inbound_handshake`
   （解析 infohash → FR-15 准入 → `wake_seed` → 等 HOT ≤3 s），guard 与打洞两条入向路径
   共用；holder 改用 `DataPlane::attach_punched_inbound_waking`（先读 68 字节 → 唤醒 →
   补回前缀再 splice）。5 条 punched 单测通过，含「前缀必须补回」与「拒绝时不得 splice」。
2. **量具不足**：打洞失败时只写「未建立」查不动。现在把 `punch_local` / `punch_target` /
   `punch_attempts` / `punch_errors` 一并回出来。

**本轮实测结果（`tests/ARCHIVE/evidence/w67-punched-tcp-20261127-r2/`）**：

- 映射 `112.5.60.4:36883` 两次采样一致；两端 `punch-route` 均 `route=punched-direct`，
  `tcp_punch_established=true`，`punched_socket_registered=true`，`direct` 计数 +1；
- holder `punched_alive=1`、member `served_registered=1`，`engineAttached=true`，
  infohash 命中；**真实 BT 数据经打洞 socket 流过**：holder 线上发出约 791 KB，
  member 桥搬运 728 644 B；
- 但只落地 **524 288 / 1 048 576 B**（4 片中的 2 片），`md5 6822f905…` ≠ `6209e962…`；
- 21:59:02.5 member 侧发 `R`（其接收缓冲仍有未读数据，属「close with pending data」形态），
  holder 之后每 20 ms 重传都被 `R` 拒；此后 member 停在 50% 直到超时。

**因此 W6.7 仍未通过**。原来那条 BLOCKED 理由（没有可直绑的公网 TCP 端口）**已经消除**，
剩下的是另一件事：

- 打洞连接在传输中途被对端 RST，原因未定位（不是端口可达性，也不是唤醒——数据真的流过）；
- `cr_bridge` 的登记是**一次性**的（`take()` 取走即删），一条打洞 socket 只能服务一次
  CONNECT；连接断掉之后，member 的 librqbit 再拨同一目标只能回落直连，必然失败。
  也就是说传输层「断了就没有第二条」，与 BT 会重连的实际行为不匹配。

关闭条件（二选一，都需要新证据）：

1. 定位并修掉中途 RST（先从 member 侧看是谁先关的：桥→引擎方向还是引擎→桥方向）；
2. 或让打洞登记**可重入**：socket 死亡后允许重新 punch 并覆盖同一桥键（`register` 目前
   对重复键是拒绝覆盖），并在 e2e 里如实记录「本次共用了 N 条 punched 会话」。

**不得**因为「有 512 KiB 流过去」就写成 punched-direct BT 已通过，也不得把这段字节数
写成可达率。按 ADR-012 不报成功率/可达率。本机、回环、静态 DNAT 都不能顶替这项。

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

1. 2.1 已关闭。2.2 的公网端口条件已由所有者放通并复测：打洞已直连、BT 数据已真实流过，
   但中途被 RST、只落地 50%，未过 md5 —— 下一步先定 2.2 里那两条关闭条件之一
   （定位中途 RST，或让打洞登记可重入），不要重跑同一套指望结果变化。
2. 2.3 属于独立课题，先 ADR 后判据，不与 2.2 混跑。
3. 2.4 需要所有者选 GUI 路线；2.5 等 R-02 复审窗口。
