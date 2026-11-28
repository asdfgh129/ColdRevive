## [2.4.165] - 2026-11-28 — 决定 5 已签 A：M4 锁定 G4 核心

> 纯文档，不动代码、不动门禁、不动判据、不产生公网写入。
> `g2_claim=true`、`s6_status=not_run`、`evidence_valid_for_pass=false`、
> `auto_pass=false`、EX-1 v1 FAIL 78/80、`runtime_verified=false`、
> `counts_toward_nfr02=false` 全部原值。

- 所有者 2026-11-28 14:33 明示「决定 5 选 A，M4 只做 G4 核心」。[DOC-35](docs/06-plan/OWNER-DECISIONS-2026-11-28.md)
  v1.3 已回填签署块，状态恢复「已签」；决定 1–4 原文未动。
- **M4 范围锁定**：G4 三句（安全测试全通过 / 公测 KPI 达标 / 法务评审通过）+ W4.2、
  W7.2 / W7.3、W8.2 / W8.3、安装包、社区材料、邀请制公测；R-02 / R-11 仍是 G4 前置。
  托盘 / 浏览器下载捕获 / UPnP 续租保持「再评估」，**不纳入 M4 交付**；
  不得写「三项纳入 M4」「托盘可用」「浏览器捕获已交付」「UPnP 续租已验证」。
- [DOC-38](docs/06-plan/M4-SCOPE-2026-11-28.md) 升 v1.1 / 状态「有效」，DOC-36 §3、
  ROADMAP §6、README 索引同步已签口径。R-02 仍只在准备态：未做公共 KAD 发布、
  未做公网 DHT announce，运营速率与全量双轨发布仍待单独授权。

## [2.4.164] - 2026-11-28 — M4 范围展开与 R-02 复审启动包（决定 5 待签）

> 纯文档，不动代码、不动门禁、不动判据、不产生公网写入。
> `g2_claim=true`、`s6_status=not_run`、`evidence_valid_for_pass=false`、
> `auto_pass=false`、EX-1 v1 FAIL 78/80、`runtime_verified=false`、
> `counts_toward_nfr02=false` 全部原值。

- **新增 DOC-38** [`docs/06-plan/M4-SCOPE-2026-11-28.md`](docs/06-plan/M4-SCOPE-2026-11-28.md)：
  把 G4 三句展开成安全测试 / 公测 KPI / 书面法务意见的可执行范围（含 72 h Soak、安装包 ≤60 MB、
  R-02 与 R-11 作为 G4 前置）；把托盘 / 浏览器下载捕获 / UPnP 续租三项的当前状态、
  纳入 M4 的依赖与代价逐项写清；给出 R-02 三桶判据、≥10 个不同节点与预注册前置。
- **DOC-35 追加决定 5（待签）**：M4 是否把三项重新纳入，按五段式给出 A/B/C/D 利弊；
  建议 **A 只做 G4 核心**，理由与不决定的后果同页写明。决定 1–4 的签署内容一字未改。
- **过期台账同步**：DOC-36 §3 从「2.3 等拍板」改成四项已签及下一轮重点；
  ROADMAP §6 追加 2026-11-28 状态更新并改版号为 2.18；README 文档索引登记 DOC-38。
- **R-02 只做启动准备**：未定义「运营速率」前不采样、未获全量双轨公网发布授权前不写入，
  本轮没有公共 KAD 发布、没有公网 DHT announce，也没有新的 G2/S6/NFR-02 结论。

## [2.4.163] - 2026-11-28 — 四项决定闭环补强：闸门动作审计、健康快照即时同步、provenance harness

> **不改门禁、不改判据、不改已归档证据**：`g2_claim=true`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`、EX-1 的 v1 FAIL 78/80、
> `runtime_verified=false`、`counts_toward_nfr02=false` 全部保持原值。
> v2.4.162 的公平性采样证据与 `PROVENANCE-NOTE.md` 一字未动；本版只修下一轮的执行方式。

- **补 DOC-35 决定 3 的明文要求**：公共 DHT 闸门的 `kill` / `revive` / `revive_denied`
  现在都写进有界审计环（`action` 字段与 announce 决策分开，`info_hash` 用 40 个 0 占位），
  避免「关了闸门但审计只显示计数变化」。`/health.dht.public_announce.audit` 与
  `/cli dht announce-status` 读同一份快照。
- **CLI 开关即时生效于可观测面**：`commands::dht_gate_cmd` 与宣告轮次都调用
  `DhtStatus::sync_gate()`；`/health` 不再等下一轮才反映 killed / 计数 / 审计。
- **公平性 provenance 欠账改成工具层修复**：`run_g2_fairness.sh` 采样当场计算并记录
  `source_tree_sha256`；`analyze_g2_fairness.py` 原样带进 `summary.json`。tar 快照没有
  `.git` 时照实写 `git_commit=unknown`，**禁止事后用 blob 比对伪造 commit**。
- **文档**：DOC-35 回填每项的完成点 / 下一到期点 / 禁止措辞；`CONFIG.md` 补
  `nat.udp_cc_mode` 与公共 DHT 授权字段；`TEST.md` 加第 13 节。
- **测试**：`cr-dht` `public_gate` **7/7**、`coldrevived` `dht_daemon` **7/7**、
  `cr-api` `webui` **5/5**（各新增 1 例）；全量串行回归 **587 passed / 0 failed / 3 ignored**
  （`regression-full-20261128-v163.log`，比 v162 多 3 条）。

## [2.4.162] - 2026-11-28 — 四项决定落地：LEDBAT 判据 v2 采样通过；公共 DHT 闸门；Web UI 改推送

> **不改门禁、不改判据阈值**：`g2_claim=true`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`、EX-1 的 v1 FAIL 78/80、
> `runtime_verified=false`、`counts_toward_nfr02=false` 全部保持原值。
> 所有者 2026-11-28 09:18 口径：决定 1 选 A、决定 2 选 B、决定 3 选 B、决定 4 不动
> （[DOC-35](docs/06-plan/OWNER-DECISIONS-2026-11-28.md) 已签）。

- **决定 1-A 第一步：ADR-025（时延驱动拥塞控制）**。中继 UDP 流的 cwnd 从「丢包驱动」
  改为「排队时延驱动」：`base_delay` 取单调最小 RTT，每 RTT 按 `gain·(target−qd)/target`
  增减，`target=100 ms`、`gain=1.0`。**丢包只触发重传、不降窗**（EX-1 的失败机理就是
  AIMD 把非对称随机丢包当成拥塞）；连续 6 轮 RTO 才塌陷复位到 `cwnd_init`。
  明示三处偏离 RFC 6817（丢包不降窗 / 无慢启动 / 基准不老化）并各写了理由。
  `CcMode::Aimd` 仍是缺省，既有行为逐字节不变。
- **决定 1-A 第二步：判据 v2 先于代码登记**。`tests/fixtures/g2-fairness-thresholds-v2.toml`
  （`2026-11-28T09:20:38+08:00`，sha256 `4d5d931e…`）：阈值与 v1 **逐字一致**，
  只改被测机制与证据要求（补 `git_commit`/`kernel_release`/`tc_qdisc_and_class` 与逐轮原始目录）。
  旧判据 `0671a173…` 未删未改。先提交（`d5d93aa`）再写代码，这是 EX-1 C4 的顺序约束。
- **决定 1-A 第三步：实现与配置**。`CongestionConfig` 增 `mode`/`ledbat_target`/`ledbat_gain`；
  探针 `--cc-mode aimd|ledbat`；守护进程配置 `nat.udp_cc_mode`（拼错直接报错，不静默回落成
  aimd —— 否则「以为开了 LEDBAT 实际跑 AIMD」，量出来的数据会挂错名字）。
- **决定 1-A 第四步：采样（单机 netns，判定面 L2）**。221 真内核 veth+HTB+netem，
  5 arm × 2 shape × 10 轮 = 100 轮、**0 轮失败**：`ledbat` 10/10（0.496–0.504，Jain ≥ 0.9999）、
  `ledbat_paced` 10/10（0.490–0.510）、`ledbat_on_per_session_cap` 10/10、`cc_off` 10/10 对照，
  **参考臂 AIMD 8/10（0.429–0.571）**——与 v1 的 78/80 同源，说明变化来自机制而不是量具。
  总体 `criterion_met=true`（4/4 判定组合）。证据
  `tests/ARCHIVE/evidence/g2-fairness-v2-20261128/`（摘要 + 809 个逐轮原始文件打包 +
  PROVENANCE-NOTE）。**边界**：单机 netns，不是公网/跨主机；EX-1 的 v1 判定不改写；
  按 EX-1 C5 **不得**写「公平性达标/通过」。
- **决定 3-B：公共 DHT 宣告闸门（授权 ≠ 默认开）**。`cr-dht::public_gate`：缺省关闭；
  显式 `dht.announce_public=true` 后仍受每分钟（默认 4）与每 UTC 日（默认 200）双配额；
  `/cli dht announce-off` 立即生效，`announce-on` 在配置未授权时**必须失败**（避免
  「运行时开关替代授权」）；每次放行/拒绝留痕（有界环 16 条，`/health.dht.public_announce`）。
  只对公共地址过闸门，回环/私有/CGNAT/链路本地/组播不算公共——否则会误伤内网自建测试节点，
  也会把现有回环用例打挂。**没有新增任何触碰公网 DHT 的测试**。
- **决定 2-B：Web UI 成为唯一界面，并改回推送**。`cr-api::webui` 重写：状态走 `/ws`
  每秒推送（ADR-008 本来就禁止轮询，旧实现是 `setInterval(5000)`），任务列表只在
  「推送里 `seed_tasks` 变化」「用户操作」「用户点刷新」时拉取；断线显示「已断开」并退避重连，
  不拿旧数字充数；新增进度条/速率/错误原因分类。加了两条护栏单测：出现 `setInterval` 即失败、
  断线必须有可见文案。`ARCH`/`ROADMAP` 把托盘/浏览器捕获/UPnP 续租标为 **M4 再评估**。
- **测试**：`cr-nat` 24/24（新增 6 例 LEDBAT）、`cr-dht` public_gate 6/6、
  `cr-api` webui 4/4、`coldrevived` dht_daemon 6/6 与 p1_config 5/5；
  全量串行回归 **584 passed / 0 failed / 3 ignored**
  （`regression-full-20261128-v162.log`，比 v160 多 14 条）。
- **已知欠账**：`git_commit` 是事后 blob 比对推导（harness 在无 `.git` 的 tar 快照里只能写
  `unknown`）。下一轮采样必须改成干净 clone 或源码树哈希，已写进 PROVENANCE-NOTE 与 TEST.md。

## [2.4.161] - 2026-11-28 — 待所有者决定统一成 DOC-35（每条带利弊选项与不决定的后果）

> **纯文档，不动代码、不动门禁、不动判据**：`g2_claim=true`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`、EX-1 FAIL 78/80、
> `runtime_verified=false`、`counts_toward_nfr02=false` 全部保持原值。

- **新增 DOC-35 [`docs/06-plan/OWNER-DECISIONS-2026-11-28.md`]：四项待所有者决定集中一处**，
  每条固定五段：一句话问题 → 为什么卡在所有者 → **带利与弊的选项表** → 我的建议与理由 →
  不决定的后果。四项分别是：①EX-1 公平性（A 实现 LEDBAT / B 换宽判据 / C 书面延期 / D 删承诺）；
  ②W6.6 GUI 路线（A Tauri / B 纯 Web UI / C 轻量托盘 / D 移出）；③FR-25 是否授权公共 DHT
  announce；④macOS NFR-02 计入口径（补测 / 如实保留 / 不得伪造）。
- **分工写死**：`OPEN-QUESTIONS`（DOC-15）只放技术待定项，**不再混入授权类问题**；
  卡点清单（DOC-36）只描述「卡在哪」，不承担「怎么选」，§2 与 §3 已加指针指向 DOC-35。
- **补 DOC-36 的 front matter**（`doc_id` / `title` / `category` / `version` / `status` /
  `owner` / `updated`），`scripts/build_docs.py --check` 由 5 个问题降到 4 个。
  剩下 4 个都是**已签署单据**（DOC-32 / DOC-33 / DOC-34 / G3 运维验收）缺 front matter，
  属历史遗留；**未擅自改动签署文件**，待所有者确认后再补。
- 结论口径未变：四项都**不挡 M3 的 G3 门**（G3 已由所有者带 6 条例外结案），
  挡的是「产品能不能交出去用」；已关闭项（NFR-04 公网 P95、W6.7 打洞 BT、G-1～G-5）不回退。

## [2.4.160] - 2026-11-27 — 打洞登记可重入；W6.7 punched-TCP BT 关闭（1 MiB / md5 一致）

> **不改门禁、不改判据**：`g2_claim=true`（人为字段）、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`、EX-1 FAIL 78/80、S6 条件计入、
> `runtime_verified=false`、`counts_toward_nfr02=false` 全部保持原值。

- **`cr_bridge` 登记槽化（可重入登记）**：注册表从 `HashMap<SocketAddr, Upstream>` 改为
  `HashMap<SocketAddr, Slot>`。`take()` 只取走连接、**保留键当占位**，于是同一个目标地址
  可以重新填装；已就绪但未被消费的连接仍拒绝覆盖（不丢活连接，ADR-021 的坑不退让）。
  新增 `SlotState`（`Armed` / `InUse` / `Awaiting` / `Unknown`）与 `Bridge::end_session()`：
  会话结束才转 `Awaiting`，把「正在用」和「待补装」分开——混成一个数会让调用方在传输正常
  时去重打洞，而那时旧 socket 还占着本地端口，重绑只会 `EADDRNOTAVAIL`（当晚实测踩到）。
  `Bridge::take_for_connect()` 对**登记过**的目标在 6 s 补装窗口内等新 socket
  （`PUNCHED_REARM_WAIT`），而不是立刻去拨一个只在打洞存活期间有效的公网映射；未知目标
  仍然立刻直连。新增观测：`socks5_awaiting` / `socks5_in_use` / `socks5_rearmed` /
  `socks5_served_after_rearm` / `socks5_rearm_wait_ms`。
- **打洞重试去共振**：`tcp_punch` 的固定 60 ms 退避改为 `60 ms + 0–90 ms` 抖动。两侧跑同一份
  实现时固定退避会把相位锁死：一条 socket 收到对端 RST 后只活十几毫秒，相位一旦错开就永远
  错开（实测 8 s / 16 轮，两端每轮都 `Connection refused`，互相把对方的 SYN 拒掉）。
- **打洞窗口可配**：`nat punch-route` 增加第五个参数（300–15000 ms，默认 4000），替代写死的
  1200 ms；其余步骤（nonce 握手、中继裁决）仍用 1200 ms，互不影响。
- **实测（`tests/ARCHIVE/evidence/w67-punched-tcp-20261127-r3/`）**：H = `119.29.150.254`
  （轻量，`TCP:19720` 已放通）、M = `192.168.1.221`（家宽出口 `112.5.60.4`），全程无
  `REDIRECT`，holder 直接绑 `0.0.0.0:19720`。首次打洞 **1 次尝试**成功（两端
  `route=punched-direct`）；传输中途停滞触发 1 次「重发现映射 + 同时重打洞」，
  `socks5_rearmed=1` 后继续传完：**1 048 576 / 1 048 576 B 落地，
  `md5 6209e962f3b26a13c9ec1ae70fd78e10` 与源一致**；member 侧打洞端口线上 payload
  1 327 246 B / 1007 帧（最大帧 2824 B），两端 `tunnels=0`（不是中继）。
- **量具事故（如实记录，两份 PASS 已作废）**：脚本此前不清下载目录，librqbit 会直接校验通过
  上一轮留下的文件，报 `complete` 且 md5 相符却**一个字节都没传**（member 线上 payload 仅
  209 B / 268 B，`downloadSpeed` 13.9 MB/s）。现在每轮 `rm -rf downloads` 并断言为空，
  非空直接抛错。另修：M 侧 SSH 建连移到 barrier 之前（原来 M 比 H 晚 200–400 ms 发包，
  「同时」被量具做没了）；重打洞前重新发现映射（家宽映射会换源端口，holder 拨旧端口时
  27 轮静默、M 每轮 `Connection refused`）。
- **测试**：`cargo test -p cr-bridge` **15/15**（新增「消费后可重新填装并服务第二次会话」
  「已就绪连接不可被顶掉」「占位等待补装后可服务」「未知目标不等待」「InUse 与 Awaiting 分开」
  「关窗后不阻塞」）；`cargo test -p coldrevived --bin coldrevived punched` **6/6**
  （新增 daemon 级「同一目标第二条打洞 socket 重新填装并服务第二次会话」）。
  全量串行回归 **570 passed / 0 failed / 3 ignored**（`regression-full-20261127-v160.log`，比 v159 多 7 条新单测），本轮未命中既存 flaky。

## [2.4.159] - 2026-11-27 — 打洞入向握手唤醒（冷种子缺口）；W6.7 通道打通、落地 50%

> **不改门禁、不改判据**：`g2_claim=true`（人为字段）、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`、EX-1 FAIL 78/80、S6 条件计入、
> `runtime_verified=false`、`counts_toward_nfr02=false` 全部保持原值。
> 本条**不是** W6.7 通过：落地 md5 不符，仍 FAIL。

- **环境变更**：owner 在 `119.29.150.254` 放通 `TCP:19720`。顺带纠正一处误判——这台机器是
  **轻量应用服务器（Lighthouse，`lhins-cyayca13`，广州）**，规则在「实例 → 防火墙」，
  不在 VPC 安全组，所以 CVM 实例列表为空不是权限问题；`82.157.68.159` 同为轻量
  （北京 `lhins-dmotjt9m`）。放通后从 221 实测 `TCP/19720` 完整握手、payload 到达 `10.1.0.2`。
- **修掉一个真缺口：冷种子不响应打洞入向握手**。`nat punch-route` 的 holder 分支原来只做
  splice，而本机 guard 路径有「68 字节握手命中冷种子就唤醒」。现场是：握手指令**确实到达**
  holder 的打洞 socket（tcpdump `21:42:55.525 ... length 68`），但引擎 10 s 内一个字节都没回，
  member 发 FIN、holder 回 `R.`，报告表现为「打洞成功、route=punched-direct、取片 0 B」。
  修法：把裁决抽成 `wake_for_inbound_handshake`（解析 infohash → FR-15 准入 → `wake_seed`
  → 等引擎报 HOT ≤3 s），**guard 与打洞两条入向路径共用**；holder 改用
  `DataPlane::attach_punched_inbound_waking`（先读 68 字节 → 唤醒 → 把前缀补回再 splice，
  丢前缀是 ADR-021 记过的坑）。
- **量具补齐**：`punch-route` 失败时原来只回「TCP 打洞未建立」，与抓包对不上时查不动。
  现在同时回 `punch_local` / `punch_target` / `punch_attempts` / `punch_errors`。
- **实测（`tests/ARCHIVE/evidence/w67-punched-tcp-20261127-r2/`）**：脚本删掉全部 `REDIRECT`，
  holder 直接绑 `0.0.0.0:19720`，观测点即打洞目标端点、两次采样一致（`36883`）。
  两端 `route=punched-direct`、`tcp_punch_established=true`、`punched_socket_registered=true`、
  `direct` 计数 +1；holder `punched_alive=1`、member `served_registered=1`、`engineAttached=true`；
  BT 数据**首次**真实经打洞 socket 流动（holder 线上约 791 KB、member 桥搬运 728 644 B）。
  但只落地 **524 288 / 1 048 576 B**，`md5 6822f905…` ≠ `6209e962…`；
  21:59:02.5 member 侧发 `R`（接收缓冲仍有未读数据），此后停在 50% 直到超时。
- **剩余关闭条件**（二选一，都要新证据）：① 定位这次中途 RST（先分清是桥→引擎方向还是
  引擎→桥方向先断）；② 让打洞登记可重入——`cr_bridge` 用 `take()` 取走即删，一条 socket
  只服务一次 CONNECT，断了没有第二条，与 BT 会重连的实际行为不匹配。
- **测试**：`cargo test -p coldrevived --bin coldrevived punched` **5/5**（新增
  `punched_inbound_waking_restores_handshake_prefix`、`punched_inbound_waking_refusal_does_not_splice`）；
  全量串行回归 **563 passed / 0 failed / 3 ignored**（`regression-full-20261127-v159.log`，
  比 v158 多 2 条新单测）。`cr-ed2k` 的 `loopback_transfer` 两条（`amule_block_accounting` /
  `amule_style_sliding_window`）在机器被并发任务压住时会出现 Windows 回环 `os error 10060`，
  隔离单跑 4/4 通过——按既有 flaky 处理，不隐藏也不宣称已修。

## [2.4.158] - 2026-11-27 — NFR-04 公网 P95 双机实测关闭；W6.7 punched-TCP BT 落到 BLOCKED（环境）

> **不改门禁、不改判据**：`g2_claim=true`（人为字段）、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`、EX-1 FAIL 78/80、S6 条件计入、
> `runtime_verified=false`、`counts_toward_nfr02=false` 全部保持原值。
> 本条只新增两项真实环境取证，以及为取证暴露的两个量具缺陷与一个数据面缺口。

- **NFR-04 公网唤醒 P95 关闭（真实双网，非回环外推）**：新增
  `tests/e2e/nfr04-public/run.py`。A = `192.168.1.221`（家庭宽带出口 `112.5.60.4`），
  C = `119.29.150.254`（腾讯云 1:1 EIP），两侧各跑真实 `coldrevived`，对端通告**只给 relay
  候选**，先 `wake raw` 预热，再 `metrics wake-latency 30 12000`。结果：A→C P95 **281 ms**、
  C→A **230 ms**，各 30/30、0 超时；强制 relay 锚点 RTT 241 / 187 ms；两端
  `sha256sum /proc/<pid>/exe` 均等于阶段二进制 `8556cd8b…`。证据
  `tests/ARCHIVE/evidence/nfr04-public-20261127/`（`MANIFEST.json` + 两份作废报告）。
  **边界**：这是 `WAKEUP_REQ→ACK` 唤醒时延，不是 BT 可达性、不是 G2/S6、ACK ≠ 公网可检索。
- **量具缺陷 1（真实缺陷，已修）**：`nat_path::wake_with_plan` 的 relay 分支把 `rtt_ms`
  **硬编码为 0**，`metrics wake-latency` 因此恒采到 `0.0 ms`。这不是「公网零延迟」，
  而是量具假值。现 TCP / UDP 两条 relay 承载都用 `Instant` 实测往返。首份 0 ms 报告留档为
  `report-zero-rtt-invalid.json`，不当作通过。
- **量具缺陷 2（流程缺陷，已修）**：修复后仍读到 0 ms，根因是 systemd 仍跑旧 workdir 二进制
  （`cp` 报 `Text file busy`，脚本未拦截）。脚本改为「先停 unit → `install` 到 `.new` 再
  `mv` → 校验安装哈希 → 重启 → 校验 `/proc/<pid>/exe` 哈希」，运行哈希不符直接抛错；
  全零样本判 `INVALID`，与 PASS/FAIL 分开。第二份作废报告留档为
  `report-stale-binary-invalid.json`。
- **W6.7 数据面缺口（真实缺口，已修）**：`nat punch-route` 此前只对发起方（member）调用
  `attach_punched_socket`，**持有者侧的打洞 socket 建立后无处可去**，入向 BT 握手会落到
  无人监听的端口。新增 `DataPlane::attach_punched_inbound`（把已验身份的 socket
  `splice_to_listener` 拼到本机 `engine.listen`，并按存活标志登记），`punch-route` 改为按
  `role` 分流：holder → inbound splice，member → SOCKS5 桥注册；holder 缺 `engine.listen`
  时**拒绝猜 `bt_port`** 并回落 relay。新增 `is_holder_role` 把「member 绝不能拼到引擎口」
  钉成单测（显式 `role` 优先、未知 role 不算 holder）。
- **W6.7 daemon 级实测：BLOCKED（环境，不是代码）**：`tests/e2e/w67-punched-bt/run.py`，
  119 为 holder、221 为 member。两观测点一致得到 `112.5.60.4:23982`（`eim=true`）；
  但两侧 `punch-route` 均因 `TCP 打洞未建立（poll+SO_ERROR 判据）` 回落 relay。根因两级：
  ①**端口对称性**——同时打开要求两侧用同一对端口互发 SYN，holder 只能绑 `19720`、
  member 只能拨 `:22`，holder 出向经 1:1 EIP 保端口后对端看到 `:19720`，与 member 的
  remote `:22` 不是同一条连接，内核只能回 `R.`（`REDIRECT` 改写端口只会永久错位，不是写法问题）；
  ②**可达性**——1:1 EIP 本身是全端口、端口保持的 NAT（UDP `19720`/`19920` 的包实测到达
  `10.1.0.2`、源端口原样保留），缺的是**安全组 / 网络 ACL 的 TCP 入向规则**：UDP 段已放通、
  TCP 只放通 22，而 Tencent 安全组按协议分开、UDP 规则不覆盖 TCP；主机 `INPUT policy ACCEPT`，
  只有防 SSH 爆破黑名单，不是主机防火墙。抓包证据
  `tests/ARCHIVE/evidence/w67-sg-reachability-20261127/`。
  落地 0 B、md5 不匹配。报告中 `member_bridge_served` / `holder_splice_done` /
  `engineAttached` 属**relay 数据面**，不是 punched-direct。证据
  `tests/ARCHIVE/evidence/w67-punched-tcp-20261127/`。
  关闭条件：**只需加一条 TCP 入向规则**（如 `TCP:19720`），NAT 侧无需改动；`REDIRECT` /
  静态 DNAT / 中继均不得顶替。
  侧记：源地址限定 `-s <家宽 IP> --dport 22` 的 REDIRECT 会同时劫持控制通道（控制台与
  被测 member 共用出口 IP），本次一度失联、经 82 跳板机 `-D` 才恢复——下次须用独立出口做控制端。
- **验证**：`coldrevived` 定点单测（3 条 punched 数据面 + 1 条角色判定）通过；全量串行回归
  **561 passed / 0 failed / 3 ignored**（`tests/ARCHIVE/evidence/regression-full-20261127-v158.log`）。
  首次全量命中历史 flaky `cr-ed2k::server_satisfies_amule_style_sliding_window`
  （连接超时 / os error 10060），隔离单跑 1/1 通过后重跑全量归档；仍按 flaky 观察，
  不宣称已修。

## [2.4.157] - 2026-11-27 — M3 剩余项第二批：FR-25 双轨发布入帧、agent 看护内核与卡点清单

> **不改门禁、不改判据**：`g2_claim=true`、三个工具字段、EX-1 FAIL 78/80、S6 条件计入、
> `runtime_verified=false`、`counts_toward_nfr02=false` 全部不变。v2.4.156 中「agent 仍骨架 /
> FR-25 未接 KAD_PUBLISH / NFR-04 阈值未决」三条表述由本条 supersede（历史原文不改）。

- **FR-25 双轨发布（本地闭环）**：新增 `cr_kad::dual_track_frame_keys`，把 A 轨
  `MD4(UTF-8(term))`（无前缀、32 位十六进制）并入 daemon 真正发出的 `KAD_PUBLISH` 帧；
  B 轨 `published_keys` / `inspect kad` 口径保持冻结，避免动到 T0/T1 夹具。
  三个发帧点（周期重发布、`publish`、本机场景）统一走该函数。ACK 仍不是公网可检索证据；
  发布量翻倍按 R-02 在 M4 前复审。`cr-ed2k` 顶部旧表述同步更正。
- **W6.6 无 GUI 看护内核**：`coldrevive-agent` 从只打印一行改为 std-only：spawn 子进程、
  `try_wait` 巡检、仅异常退出按预算重启、只读 `/health` 就绪日志；不引 Tauri / tray-icon，
  不监管 relay，不碰公网。托盘 / 浏览器捕获 / UPnP 续租仍 BLOCKED，见新卡点清单。
- **NFR-04 口径更正**：SRS 写的是局域网 P95 < 3 s、公网 P95 < 8 s，T0/T1 的 3000 ms 只适用
  局域网，两者**不冲突**，不需要所有者裁决（此前记「未决」是把作用域混读）。量具已支持
  `timeout_ms`，公网采样须显式给 > 8000 ms 超时，否则超时样本会把 P95 截在 1.2 s。
- **新卡点清单**：`docs/06-plan/M3-REMAINING-BLOCKERS-2026-11-27.md` 逐项写清剩余项缺什么、
  需要什么资源与触发条件：公网 P95 采样、BT-over-punched-TCP 双端实测、LEDBAT 新 ADR +
  新判据 sha、W6.6 GUI 路线、FR-25 R-02 复审。`TEST.md §2.18` 同步更正。
- **验证**：`cr-kad` 34/34；`coldrevive-agent` 5/5 单测 +
  `tests/e2e/w66/agent_supervisor.py` 3/3（异常退出重启到预算耗尽、干净退出不重启、
  spawn 失败非静默通过）；P0 双进程 e2e **77/77**
  （`tests/ARCHIVE/evidence/p0-dual-track-20261127.json`，其中 `publish` 帧 keys 计数为 9
  （含 A 轨），`inspect kad` 仍为 B 轨 7 个 key）；全量串行回归
  **558 passed / 0 failed / 3 ignored**（`tests/ARCHIVE/evidence/regression-full-20261127-m3b.log`）。
  T1 `--lan-precheck` 曾尝试复验，因本机缺 `T0_DOWNLOADER`（aria2）二进制 10 条 BLOCKED，
  属环境缺失，不作为门禁回归结论，也不改 G1 既有证据。

## [2.4.156] - 2026-11-27 — M3 工程余项第一批：暴露面闸门、Web UI 资产、aria2 控制面与 NFR-04 量具

> **不改门禁、不改判据**：`g2_claim=true`、三个工具字段、EX-1 FAIL 78/80、S6 条件计入、
> `runtime_verified=false`、`counts_toward_nfr02=false` 全部不变。本条只补工程实现与量具能力。

- **W6.2 安全缺口（真实缺陷）**：此前 `cr_api::RPC_SECRET_REQUIRED_OUTSIDE_LOOPBACK=true` 只在
  `/health` 里报告，守护进程仍可按 `0.0.0.0`/LAN 裸启，`/jsonrpc` 的 mutating 调用等于无鉴权。
  新增 `cr_api::rpc::validate_exposure(bind, secret)`，在 `coldrevived` 绑定任何 socket 之前拒绝
  「非回环 + 无/空 secret」并以退出码 2 结束；回环绑定行为逐字节不变。
- **W6.1/W6.4 Web UI**：从单个只读状态页升级为编译期嵌入的 `GET /` + `GET /ui/style.css` +
  `GET /ui/app.js`。页面能列真实 `tellActive/tellWaiting/tellStopped` 任务、addUri、pause/resume/
  remove，状态仍只读 `/health`；无 CDN、无磁盘写入、不遮蔽既有端点。
  `/api/v1/status` 与 `/api/v1/openapi` 作为同实现别名（ADR-003）。
- **W6.2 aria2 控制面**：补 `aria2.pause` / `pauseAll` / `resume` / `resumeAll` / `remove` /
  `forceRemove` / `removeDownloadResult`，全部走真实 `cr-engine` 的暂停/恢复/移除，成功后才回写
  `RpcTask`；取 gid 时跳过 `token:<secret>` 参数，避免把凭据当任务号。所有方法仍按方法语义鉴权，
  secret 不会进任何响应。
- **NFR-04 量具**：`metrics wake-latency <n> [timeout_ms]` / `metrics revival <n> [timeout_ms]`
  增加可选超时参数，缺省 1200 ms 保持 T0/T1 逐字节不变，上限 60 s。公网 P95 仍需所有者先确认
  SRS 的 8 s 与 T0 夹具 3 s 的口径差异，未确认前不新增公网阈值夹具、不出任何 P95 结论。
- **口径修正（不重写历史，只标 supersession）**：`cr-ed2k/src/lib.rs` 的「daemon 未接线」与
  `tcp_punch.rs` 的「数据面未接线」已按 v2.4.112/116 现状更正；`config/coldrevive.example.toml`
  改成 `api.rpc_secret` 并标注 `engine.max_active_seeds`。多任务逐出与 daemon 接线本来就已落地，
  11-20 的状态清单属于历史快照，不在本条里改写。
- **验证**：`cargo check -p cr-api -p coldrevived` 通过；定向单测 cr-api **17/17**、coldrevived
  **84/84**；P1 本地回环 e2e **6/6**（含「非回环无 secret 拒绝启动」负向对照），报告
  `tests/ARCHIVE/evidence/p1-loopback-20261127-v2.json`。全量串行回归
  **552 passed / 0 failed / 3 ignored**（`tests/ARCHIVE/evidence/regression-full-20261127-m3a.log`）。
- **M3 仍未完成（明确留下，不冒充已闭环）**：`coldrevive-agent` 仍是骨架（需要托盘/进程监管/
  IPC 与打包，属 GUI 依赖）；FR-25 双轨发布未接 KAD_PUBLISH；BT-over-punched-TCP 缺公网双端
  daemon 级实测；NFR-04 公网 P95 缺双公网主机与阈值决定；可靠层无 LEDBAT（需新 ADR，公平性
  EX-1 仍是 FAIL，禁止调 β/重采样）。以上均不是「代码小修即可宣布完成」的项。

## [2.4.155] - 2026-11-27 — 文档同步：明确 NFR-02 范围与 macOS `not_verified` 含义（无代码/门禁变更）

> 仅更新测试文档、出口记录与机读夹具的表述；不改源码、判据、阈值或任何门禁字段。

- **TEST 3.2 新增 §2.17**：写清 NFR-02 是「空闲态平均 CPU ≤ 2%」+「造种任务仅以 Idle
  优先级运行」两个分句；本次 macOS 只覆盖第二分句，且只降引擎线程 `cr-engine-wrk`，
  唤醒链路线程保持默认优先级。
- **`not_verified` 定性**：唤醒线程逐线程优先级项不是 fail（未观察到降级），也不是 pass
  （未取得逐线程证据）。Darwin `getpriority(PRIO_DARWIN_THREAD,0)` 只能读调用线程，
  测试包没有进程内逐线程读取器；握手功能通过或进程级读数都不能替代。
- **DOC-34 v1.7 §15**：区分「2026-11-25 签署时无 mac 硬件」的历史表述与「2026-11-26 run2
  已有真机证据」的当前事实；§0 达标声明、`g2_claim`、三个工具字段均未改。
- **机读夹具**：`g2_claim_wording` / `g2_wording_constraints` / `platforms.macos` /
  `macos` 字段同步为 `set 0x1000 / readback 1` 与 run2 结果；`runtime_verified=false`、
  `counts_toward_nfr02=false` 保持，等待所有者决定是否计入。
- **归档账本**：追加一条 INFO 更正，说明 NFR-02-MAC-RUN2 的 `result: PASS` 应读作
  「套件执行完成」，计数是 5 pass / 1 not_verified，不是全项 PASS；原文按追加式规范保留。

## [2.4.154] - 2026-11-26 — macOS 真机复跑：套件 5 pass / 1 not_verified；修正 Darwin 读回判读

> **不改门禁、不改判据**：`g2_claim` 与三个工具字段不动，`gate_claim=null`，唤醒线程逐线程
> 优先级仍 `not_verified`。本次修的是实现与平台 API 语义不一致的 bug，**不写「NFR-02 三平台
> 达标」或「macOS 已验证」**——是否改口径由所有者书面决定。

- **Darwin 读回判读 bug（核心）**：`setpriority(PRIO_DARWIN_THREAD, 0, 0x1000)` 收的是标志位，
  而 `getpriority(PRIO_DARWIN_THREAD, 0)` 按 `man 2 setpriority` 与 XNU `kern_resource.c`
  返回**布尔值**：`1` = 后台、`0` = 前台。旧代码拿读回值和 `0x1000` 比，导致内核已切后台却
  记成 `applied=0/failed>0` 的**假失败**。现拆成 `PRIO_DARWIN_BG`（设置侧 0x1000）与
  `PRIO_DARWIN_BG_READBACK`（读回侧 1）；判读单测新增「读回 0x1000 必须失败」防止回退，
  真机两条 idle 用例与 `/health` 错误文案同步。设置侧参数**一点没放宽**。
- **附带修掉的三个缺陷**（不改变任何阈值）：`cr-nat/tcp_punch.rs` 把 `#[cfg(unix)]` 中
  Linux 专有的 `__errno_location`、`EINPROGRESS=115`、socket 常量与 `poll` nfds 类型拆成
  Linux/Darwin 两路（macOS 用 `__error`、`EINPROGRESS=36` 等），修复 daemon 链接失败；
  `run_all.sh` 启动 daemon 时加 `exec`，让 `$!` 是真正 PID，避免旧进程残留导致负向对照
  假失败；  `coldrevived/data_plane.rs` 在 `TcpStream::from_std` 前设非阻塞，修掉
  tokio 1.54.1 拒绝注册阻塞 socket 的既有缺陷（由全量回归暴露，不是 flaky）；
  `cr-nat/tcp_discovery.rs` 的测试夹具在主线程先 bind、再由 observer 线程 accept（带 5 s
  上限），并等监听就绪后再连接，修掉端口竞态导致的 ConnectionRefused 与无限等待。
- **验证**：测试包 run `20261126T104241Z`（Mac mini / macOS 26.5.2 / Darwin 25.5.0 x86_64）
  → **5 pass / 1 not_verified / 0 fail / 0 blocked**，`gate_claim=null`；`/health` 三次快照
  均为 `requested=true, applied=3, failed=0, unsupported=0`，负向对照 `applied=0`，握手唤醒
  262144 B md5 一致。`cargo test -p cr-engine --lib` 13/13、`cargo test -p cr-nat` 211/0/3
  （含真实回环 TCP 双向字节）；全量串行 `CARGO_INCREMENTAL=0 cargo test --workspace --offline
  -- --test-threads=1` → **549 passed / 0 failed / 3 ignored**
  （日志 `tests/ARCHIVE/evidence/regression-full-20261126-v154.log`）。
- **落点**：`src/crates/cr-engine/src/prio.rs`、`src/crates/cr-nat/src/tcp_punch.rs`、
  `src/bins/coldrevived/src/data_plane.rs`、`dist/coldrevive-macos-nfr02-runtime-v1.4/`
  （源码/README/SOURCE_INFO/MANIFEST 与 zip 同步）、`docs/06-plan/G2-EXIT-RECORD-2026-11-25.md`
  §14、`docs/README.md`、`tests/ARCHIVE/NOT-TESTED.md`、`tests/ARCHIVE/TEST-RUNS-2026-11-26.md`、
  `tests/fixtures/g2-spike-verdicts.toml`（只加事实补记，未改人为判定字段）。
- **证据**：`tests/ARCHIVE/evidence/nfr02-macos-run2-20261126/`（逐字节归档 + MANIFEST + sha256），
  run1 旧失败证据保留未动。

## [2.4.153] - 2026-11-26 — 首次 macOS 真机运行：**FAIL**，定位并修复两个缺陷（待重测）

> **不改变任何门禁判定**。`g2_claim=true`、EX-1 仍 FAIL、S6 仍「条件计入」；`runtime_verified`
> 仍为 `false`。本次只是把真机 FAIL 证据归档并修掉两处实现/打包缺陷。

- **真机结果**：用户 Mac mini（macOS 26.5.2 / Darwin 25.5.0 / x86_64 / rustc 1.100.0）跑
  `coldrevive-macos-nfr02-runtime-v1.2` → `summary.json` **0 pass / 4 fail / 1 blocked /
  1 not_verified**。原始 `results.zip` 与解包文件逐字节入库
  `tests/ARCHIVE/evidence/nfr02-macos-run1-20261126/`（MANIFEST + sha256 + `* -text`）。
- **FAIL 根因 1（代码常量错）**：`darwin_verdict::PRIO_DARWIN_BG` 被写成 `0x10000`，而 Apple XNU
  `<bsd/sys/resource.h>` 定义的是 **`0x1000`**（`0x1001` 才是废弃的 `PRIO_DARWIN_NONUI`）。
  真机日志：两条 idle 用例读回 `PRIO_DARWIN_THREAD = 0x0`、`applied 0 -> 0`。已改常量与钉值单测；
  这是**修正实现以符合 Darwin API**，不是降低判据——判据仍是「读回等于 `PRIO_DARWIN_BG`」。
- **FAIL 根因 2（运行包漏文件）**：`cr-ed2k/src/lib.rs` 用 `#[path]` 从 `tests/spikes/...` 纳入
  三个源文件，而运行包只收 `src/**`，`cargo build --bin coldrevived` 报 `couldn't find file
  sha1.rs`。v1.4 起把这三个文件按原相对路径一并收入。
- **仍缺**：这次没有 daemon 日志与 `/health` 快照，握手功能项 `blocked`；唤醒线程逐线程优先级项
  按设计固定 `not_verified`。修完后必须**重跑真机**，不能据本次证据写「macOS 已验证」。
- **回归**：全量串行 `CARGO_INCREMENTAL=0 cargo test --workspace --offline -- --test-threads=1`
  → **549 passed / 0 failed / 3 ignored**（`tests/ARCHIVE/evidence/regression-full-20261126-v153.log`）。
- **落点**：`src/crates/cr-engine/src/prio.rs`、本条 CHANGELOG、
  `tests/ARCHIVE/TEST-RUNS-2026-11-26.md`、`dist/coldrevive-macos-nfr02-runtime-v1.4.zip`。

## [2.4.152] - 2026-11-26 — G-3 关闭：中继运维面限流/观测/告警三项实测 PASS

> **先登记判据再采样**：DOC-35 + `tests/fixtures/g3-relay-ops-thresholds.toml`
> （sha `8e91943a6d46…`，登记后未改）。采样在 HEAD `f1373d4` 重建的二进制上完成，
> 独立回环 relay，**生产 `119.29.150.254:7101` 未触碰**。`g2_claim` 与三个工具字段未动，
> EX-1 仍 FAIL 78/80，mac 真机实测仍是未测项。

- **补实现缺口**（v2.4.151 提交 `f1373d4`）：此前**没有告警**；新增
  `scripts/relay_ops_watch.py`——只读两份 stats 文件、按预登记阈值算窗口增量、追加 JSON Lines
  告警，计数变小视为进程重启不告警，stats 过期本身也是告警。`coldrevive-relay` 的
  `--udp-stats-file` 补齐 5 个 per-session 计数（此前只在内存结构体里）。
- **采样编排**：`tests/e2e/g3/relay_ops_probe.py` 基线 → 违约 → 重启(reset) → 停进程(stale)，
  每段真实 stats 快照与告警文件入库。基线 TCP/UDP 各 50/50 回声、四类拒绝计数全 0、
  stats 滞后 0.019/0.020 s；违约 `bandwidth_limited` 0→10、告警 **0.433 s** 出现并自动 resolved；
  stale 2 条；reset 阶段 0 条规则告警且 `counter_resets_seen=1`。
- **三项判据**：限流 PASS / 观测 PASS / 告警 PASS。不是带宽丢包 SLO，不表示公网 relay 当前健康，
  不表示两个 NAT 互达（bench UDP 两端同机），也不使 S6 类别或 EX-1 例外变化。
- **落点**：DOC-35 v1.1（含结果）、DOC-34 v1.5 + §12、夹具 `g3_relay_ops` 与空 `g2_claim_open_gaps`、
  `docs/README.md`、`G2-REVIEW-BRIEF` 补记、`OPS.md` 运维用法。
- **证据**：`tests/ARCHIVE/evidence/g3-relay-ops-20261126/`（15 份原始文件 + MANIFEST + `* -text`，
  `summary.json` sha `cff30a3ceaabeafca69c23adf74f843f99cc81bf8b6ceebf12ee27fb58b8ffc4`）。
  实现提交前的一次预跑随 `f1373d4` 误入库，已在 HEAD 重建后重采覆盖；MANIFEST `supersedes` 留痕。
- **回归**：全量串行最终 **549/0/3**（`regression-full-20261126c.log`，锁字节）。当天前两次全量都撞上
  `cr-ed2k::server_satisfies_amule_style_sliding_window` 的连接超时，单跑 4/4、第三次全量通过，
  按计时型 flaky 记账（与本次改动无关，未碰 cr-ed2k），未隐藏也未声明已修。

## [2.4.150] - 2026-11-25 — G-2 关闭：S4/S5 报告逐字节归档（15 份 + sha256 清单；不改判定）

> **关的是「证据在 clone 后读不到」，不是「证据等级提高」。** S4=`pass`、S5=`fallback` 原判定，
> 各报告的 `evidence_valid_for_pass=false`（除 netem 的 `evidence_valid=true`）原值，`g2_claim=true`
> 与三个工具字段 `not_run/false/false` 全部未动。当时唯一开放欠账为 G-3（中继运维面，已于 v2.4.152 关闭）。

- 新增 `tests/ARCHIVE/evidence/g2-s4-s5-reports-20261125/`：S4 4 份（含 1 份 0 B 空产出，标注不用于判定）、
  S5 qos 7 份 / utp 3 份 / utp_netem 1 份，共 68,498 B。每份都是原件逐字节副本。
- `MANIFEST.json` 记录 `source_path` / `bytes` / `sha256` / `recorded_at` / 证据等级 / 引用位置；
  目录内 `.gitattributes` 为 `* -text`，`git show HEAD:<path> | sha256sum` 可与清单直接比对。
- 复核命令用 `sys.stdout.buffer.write` 输出 LF；Windows 上用 `print` 会让 sha256sum 把尾随 `\r`
  当文件名的一部分（已实测并记录在 DOC-34 §11 与 `tests/ARCHIVE/README.md §2.1`）。
- 夹具 `g2-spike-verdicts.toml`：新增 `s4_s5_evidence_archive`（G-2 closed），S5 的 `evidence`
  改指归档副本，`g2_claim_open_gaps` 只剩 G-3；`g2_claim_wording` 同步为「一项欠账」。
- 文档：DOC-34 v1.4 + §11、`G2-SPIKE-VERDICT` §7 归档索引、DOC-31 补记、`docs/README.md`。
- 归档账本：`tests/ARCHIVE/TEST-RUNS-2026-11-25.md` 追加一条；无测试失败，无门禁常量改动。

## [2.4.149] - 2026-11-25 — G-5 关闭**代码缺口**：macOS 走 `PRIO_DARWIN_BG`（实现 + 类型检查 + 判定单测；**仍无硬件实测**）

> **这一条关的是「macOS 没有实现」，不是「macOS 已达标」。** 本仓库没有 mac 机器，
> darwin 分支一次都没执行过。NFR-02 口径不变：只能说「Linux/Windows 两分句已实测」。
> `g2_claim` 仍 `true`，三个工具字段仍 `false`。

- **判据没动**：SRS NFR-02「造种任务仅以 Idle 优先级运行」原文未改、阈值未放松。
- **实现**（`src/crates/cr-engine/src/prio.rs`）：
  `setpriority(PRIO_DARWIN_THREAD=3, 0, PRIO_DARWIN_BG=0x10000)`；
  读回 `getpriority(PRIO_DARWIN_THREAD) == PRIO_DARWIN_BG` 才算成功。`PRIO_DARWIN_BG` 是
  **标志位不是 nice 值**，故 darwin 上 `ThreadPriority.nice` 恒 0（无意义），真实读数在
  `policy`（0 = 前台类 / `0x10000` = 背景类）；新增 `IdleOutcome::DarwinBackground`
  （`as_str()="darwin_bg"`，`is_idle()=true`），`/health.idle_priority` 照旧报计数。
- **`getpriority` 的 -1 陷阱**：darwin 上 -1 既是错误返回值、又绝不是合法状态，所以先清
  `errno`（darwin 的 errno 由 `__error()` 返回指针，不是全局变量）；读到 -1 一律折成
  `None`（读不出），**既不折算成 0，也不折算成成功**。
- **判定逻辑与 FFI 拆开**：`darwin_verdict::from_readback(set_rc, set_errno, read_back)` 与平台无关，
  在 Windows/Linux 上真跑 2 条用例——`(0,0,Some(BG))`→成功；`(0,0,Some(0))`→**失败**
  （只看返回码就会在这里误报达标）；`(0,0,None)`→无法判定；`(-1,1,Some(0))`→失败且带出 `errno`。
  不拆的话，darwin 分支连判定规则都没被测过——那只是「能编译」。
- **分支的类型检查**：新增 crate feature `darwin-check`，`cargo check -p cr-engine --features darwin-check`
  在 Linux/Windows 上把 darwin 分支一起编译（**只检查，不链接不执行**；生产构建默认关闭，
  Linux/Windows 仍走各自分支，`cargo build` 行为不变）。
- **为什么不用交叉 `--target`**（省得下一个人重试）：`rustup target add aarch64-apple-darwin`
  成功，但 `cargo check --target aarch64-apple-darwin -p cr-engine` 卡在 **`aws-lc-sys v0.47.0`**
  的构建脚本——它要用 `cc` 为目标平台编译 C 代码，Windows 上没有 darwin 交叉 C 工具链，
  报 `failed to find tool "cc"`。`aws-lc-sys` 是 librqbit → rustls 的传递依赖，绕不开。
- **测试**：`cargo test -p cr-engine --lib` → **13 passed / 0 failed**（原 11 + 2 条 darwin 判定用例）；
  全量 `CARGO_INCREMENTAL=0 cargo test --workspace --offline -- --test-threads=1` →
  **549 passed / 0 failed / 3 ignored**（原 547 + 2），日志 `tests/ARCHIVE/evidence/regression-full-20261125e.log`。
  首轮全量在 `cr-ed2k` 的 `server_satisfies_amule_style_sliding_window` 上出现过一次
  `os error 10060`（连接超时），单独重跑该用例 4/4 通过、重跑全量 549/0 通过——按**计时型 flaky**
  记录，不当作已修复也不隐藏。
- **Linux 侧未被改坏**：同一份树在 221 上 `cargo check -p cr-engine --offline` 通过
  （cfg 门控改动只影响分支选择，Linux 仍走 `nice 19`+`SCHED_IDLE`）。
- **221 清理**（所有者要求）：删的是本仓库自己的临时物——`/tmp/udl4-qbt-*`（151 个 e2e 测试目录）、
  `/tmp/crfull`(781M)/`/tmp/crmini`(144M)/`/tmp/rustup-init`/`/tmp/bcstrings.txt`、
  `/root/cr24-src-*.tgz`（6 个旧源码包）、`coldrevived.bak-nfr02`（被 `.bak-prio` 取代的旧备份）。
  **未动** `/opt/cr-punch-pub/221` 的二进制/配置/状态、`/root/cr24` 构建树、系统包与日志留存。
  根分区 24G：可用 **504M → 1.9G**（98% → 92%），`/tmp` 1.2G → 39M。
- 证据：`tests/ARCHIVE/evidence/nfr02-macos-idle-priority-20261125.json`
  （含 feature 检查、交叉检查失败原因、单测清单；`runtime_verified=false`、`counts_toward_nfr02=false`）。
- 文档：DOC-34 v1.3 新增 **§10**（macOS 取证与真机收尾清单）、§2 禁用措辞与 §7 签署同步更新；
  夹具 `g2_claim_open_gaps` 只余 G-2/G-3，新增 `g2_wording_constraints` 与
  `[nfr02_idle_priority].macos`（`runtime_verified=false`）；`CONFIG.md` 的 `engine.idle_priority` 行。

## [2.4.148] - 2026-11-25 — G-4 关闭：造种引擎线程实测以 Idle 优先级运行（含负向对照）；新开 G-5（macOS 未实现）

> **这一条只关闭 G-4（NFR-02 第二分句）。** 第一分句（空闲 CPU ≤2%）早在 v2.4.142 关闭。
> `g2_claim` 仍 `true`，三个工具字段仍 `false`。

- **判据没动**：SRS NFR-02「造种任务仅以 Idle 优先级运行」原文未改、阈值未放松。
- **实测（221 / `coldrevived-a` / 二进制 `ef34652b…` / commit `9ae510a`）**：
  3 个引擎线程（`cr-engine-wrk`：2 worker + 1 存活阻塞池线程）全部 **nice 19 + `SCHED_IDLE`**，
  3 次快照（间隔 3 s）每一次都成立；其余 7 个线程（主线程、2 个 `cr-data-plane`、4 个 std）
  全部 `nice 0`/`SCHED_OTHER`——**唤醒路径没有被降级**（NFR-04 不受影响）。
  `/health.idle_priority` = `requested=true, applied=3, failed=0, unsupported=0`。
- **负向对照**（关键）：`engine.idle_priority=false` 重启后，同一批线程名仍在但回到
  `nice 0`/`SCHED_OTHER`，判定变成 `fail`、`applied=0`。没有这一条，「探针只会印 pass」
  就无法排除——探针测的是事实还是自证，靠对照区分。
- **功能不敢假设**：引擎线程处于 Idle 时跑 LAN 用例（A 做种、B 拨入 68 B 握手唤醒），
  1 MiB 落地 **md5 一致**（`6209e962…`）、`guard.handshakes=1/woken=1`、`seed_after=hot`。
- **实现**：`cr_engine::prio`（Linux `nice 19`+`SCHED_IDLE`；Windows `THREAD_PRIORITY_IDLE(-15)`），
  `Builder::on_thread_start` 里**按线程**降级；`engine.idle_priority` 默认 `true`。
  `EngineOptions.idle_threads` 库层默认仍 `false`（不改既有证据的字节行为），由守护进程显式打开。
- **macOS 未实现** → 新缺口 **G-5**：`cr_engine::prio` 在 macOS 返回 `Unsupported`，
  `/health` 报 `unsupported`。darwin 有 `PRIO_DARWIN_BG`，但本仓库无 mac 机器实测，
  按规矩不写「支持」。因此可写「NFR-02 在 Linux/Windows 两分句均有实测」，
  **不可**写「三平台达标」。
- 证据：`tests/ARCHIVE/evidence/nfr02-engine-idle-priority-20261125.json` + `.on.json`/`.off.json`
  （两份原始探针输出，`-text` 锁字节）；探针 `scripts/nfr02_thread_prio_probe.py`。
- **全量回归**：`CARGO_INCREMENTAL=0 cargo test --workspace --offline -- --test-threads=1` → **547 passed / 0 failed / 3 ignored**（原 541 + 新增 6：`cr-engine::prio` 5 条、`cr-core` 配置 1 条），日志 `tests/ARCHIVE/evidence/regression-full-20261125d.log`。
- 文档：DOC-34 第 1 节第 2 行改为「两分句 ✅」、§2 禁用措辞、§3 关闭记录、§7 签署栏、
  新增 **§9 取证方法**；夹具 `[nfr02_idle_priority]`、`g2_claim_open_gaps` 用 G-5 换 G-4、
  `CONFIG.md` 的 `engine.idle_priority` 行补线程名与 15 字符限制。

## [2.4.145] - 2026-11-25 — 记录 tokio 1.54 的关键实现事实：worker 本身就是阻塞池线程

> 只是注释与一条测试，二进制不变。但它决定了「钩子能不能覆盖 worker」这个判断，
> 不写下来下次一定会有人（包括我）按公开文档想当然。

- **事实**：`Builder::on_thread_start` 在 tokio 1.54.1 里只有阻塞池
  `blocking::pool::Inner::run` 调用（`after_start`），`scheduler/multi_thread/worker.rs`
  里没有第二处。worker 之所以也能被降级，是因为 `Launch::launch` 用
  `runtime::spawn_blocking(move || run(worker))` 起线程——**worker 就是阻塞池线程**，
  于是共用同一个 `thread_name` 与同一个 `after_start`。这也解释了 221 上 9 个线程里
  只有 2 个 `tokio-rt-worker`：那就是引擎的 2 个 worker。
- 这是**版本相关实现细节**，不是公开契约。因此代码不靠「钩子一定被调用」来声称达标，
  靠读回；升级 tokio 后若实现变了，表现是 `applied` 变小或探针里出现 `nice=0` 的引擎线程
  ——看得见，不会静默变成「声称降了其实没降」。
- 新增测试 `runtime_worker_task_runs_at_idle_priority`：在**真正跑在 worker 上的 task 里**
  读自己的优先级，Linux 断言 `nice=19 || policy=5`，Windows 断言 `-15`。
  Windows 本地 4/4 通过。

## [2.4.144] - 2026-11-25 — 修 prio 读回路径：`/proc/self/stat` 是线程组组长，必须用 `/proc/thread-self/stat`

> 221 上第一次跑 Linux 分支时，降级其实**成功**了，却被判成失败（`读回 nice=0 policy=0`）。
> 原因是本模块读回时用了 `/proc/self`——它指向线程组组长，子线程里读到的是主线程的值。
> 只看代码看不出来：`setpriority` / `sched_setscheduler` 都返回 0，读回也「有值」。

- `read_back()` 改用 `/proc/thread-self/stat`（Linux 3.17+），老内核回落 `self` 并接受该限制。
- 这条正是「一律读回校验」的代价与价值：读回能抓住「调用返回 0 但没生效」，
  但读错文件会把「已生效」误判成「没生效」——两种情况都必须被看见，不能只看返回码。
- 无行为变更之外的影响：`/health.idle_priority.applied` 从此反映真实线程数，
  修复前在 Linux 上会恒为 0（假失败）。

## [2.4.143] - 2026-11-25 — 实现 NFR-02 第二分句：引擎工作线程降至 Idle（`engine.idle_priority`，默认开）

> **这是实现，不是达标声明。** 数值与线程读数见下一条（v2.4.144 才关闭 G-4）。
> 四个门禁机器字段一个没动。

- **为什么按线程降、不按进程降**：同进程里还有唤醒关键路径（cr-guard 读 68 B 握手、
  cr-data-plane 的 UDP、HTTP API）。整进程降到 Idle 会把 NFR-04 的唤醒 P95 一起拖下去
  ——那是拿一个门禁换另一个门禁。221 上实测线程分布为：主线程 + 6 个未命名 std 线程 +
  2 个 `cr-data-plane` + **2 个 `tokio-rt-worker`**，librqbit 的全部工作就在这 2 个上。
- **粒度边界的诚实说法**：librqbit 只有一个线程池，做种/分片校验/下载共用同一批 worker，
  因此降的是**整个引擎**，无法只降做种不降下载。这条写进 `EngineOptions::idle_threads`
  的文档、CONFIG.md 与 `/health`，不藏。
- **落点**：新增 `cr_engine::prio`（Linux `nice 19` + `SCHED_IDLE`；Windows
  `THREAD_PRIORITY_IDLE(-15)`；macOS 返回 `Unsupported`，不假装支持）；`RqbitEngine`
  在 `Builder::on_thread_start` 里降级——**必须卡在出生点**，事后按 tid 去 `/proc` 找
  会漏掉「线程已开始做种、还没被找到」的窗口；`EngineOptions.idle_threads` 默认 `false`
  （库层不动既有证据的字节行为），`coldrevived` 按 `engine.idle_priority`（**默认 `true`**）
  显式打开。`/health.idle_priority` 报 `requested/applied/failed/unsupported`，
  `applied` 是**读回校验通过**的线程数，不是「调用了几次」。
- **一律读回**：系统调用返回 0 不算数，判定只看读回值（Linux `/proc/self/stat` 的
  nice/policy，Windows `GetThreadPriority`）。
- 单测：`cr-engine` 3 条（含「子线程降级不污染调用线程」）、`cr-core` 1 条
  （默认开；字符串/数字不算布尔；与 `guard_handshake` 互不影响）。
- 配置键文档：`docs/04-protocol/CONFIG.md` 新增 `engine.idle_priority` 行。

## [2.4.142] - 2026-11-25 — G-1 补采关闭：NFR-02 空闲 CPU 实测 0.03%；新增 G-4（Idle 优先级无实现）

> **关闭的是「没有采点」，不是整条 NFR-02。** 第二分句「造种任务仅以 Idle 优先级运行」
> 在守护进程内没有运行时实现，转为缺口 **G-4**；因此仍不可写「NFR-02 达标」。
> 无源码改动（只新增探针与证据）；四个机器字段一个没动（`g2_claim` 仍是 `true`，
> 其余三个仍 `false`）。

- **采点**：221 / `coldrevived-a` / 600.63 s 空闲窗口（`role=holder`、`seed_active=0`、
  `served_bytes` 零增长、`dht.enabled=false`、无 `limits.*`）→ 进程累计 CPU **0.18 s**
  → 均值 **0.03%**（单核口径；阈值 2%，余量 66×），`inst_p95=0.0%`、单点最大 1.33%；
  RSS 恒 17,320 kB、9 线程；20 次 `/health` 全成功。
- **为什么不用 `/health` 自采样**：`sysres::spawn()` 仅在配置了任一 `limits.*` 时才起线程，
  本环境一条未配，故 `current=null` / `sample_at_secs=0`——**「没开采样」不等于「读不到 CPU」**；
  早期长跑的 `cpu_percent_sampled=false` 实为采集脚本未取资源块（原件写 `No CPU time series was captured`）。
- **构建溯源（顺带修掉一个真问题）**：221 的构建树有 8 个 `.rs` 落后于 HEAD，原部署二进制不是
  HEAD 构建。已按 HEAD 重同步（86/86 与 git blob 逐字节一致）后 native build，新二进制
  `sha256 74859068…`，旧件留档 `coldrevived.bak-nfr02`（`97dafe5a…`）。采样在新二进制上做。
- **新增**：`scripts/nfr02_idle_cpu_probe.py`（只读探针，`/proc/<pid>/stat` 差分）、
  `scripts/build_nfr02_evidence.py`（整理 + 填 provenance）；
  证据 `tests/ARCHIVE/evidence/nfr02-idle-cpu-20261125.json` 与逐样本
  `nfr02-idle-cpu-20261125.raw.json`（sha256 `426b2a18…`，`.gitattributes` 锁字节）。
- **落点**：DOC-34 升至 v1.1（第 1 节第 2 行、第 2 节禁用措辞、第 3 节第 4 条、第 7 节签署、
  新增第 8 节「采点方法」）；夹具 `g2_claim_open_gaps` 用 G-4 替换 G-1，新增
  `[nfr02_idle_sample]`（含数值、sha、`wording_ok` / `wording_forbidden`）；DOC-31 与 `docs/README` 同步。

## [2.4.141] - 2026-11-25 — 所有者声明 G2 达标：`g2_claim` 改 `true`，新增 DOC-34 出口记录

> **唯一变动的机器字段是 `g2_claim`（`false` → `true`，人为判定字段）。**
> `s6_status=not_run`、`evidence_valid_for_pass=false`、`auto_pass=false` 保持不变——
> 这三个是**工具**字段，含义是「工具不替人下结论」，不是「G2 没达标」。无源码改动。

- **新增 DOC-34** `docs/06-plan/G2-EXIT-RECORD-2026-11-25.md`：M2 出口记录。声明口径唯一：
  「**G2 达标（所有者判定，2026-11-25T17:20），带两项书面例外与三项欠账**」。
- **逐项账写明「撑不撑得住」**（这是本记录的重点，不是四舍五入）：
  NFR-01 内存 ✅（1 万 ≈7.79 MiB）；**NFR-02 CPU ❌ 无独立采点**（daemon 长跑
  `cpu_percent_sampled=false`）→ 缺口 **G-1**；NFR-03 带宽 ✅ 但**凭令牌桶不凭 LEDBAT**
  （ratio=0.02 前台劣化 4.03% / 真 rqbit uTP 2.70%，该 crate 无 LEDBAT）；
  S6 ✅ 带四条例外（类别 3/4 等）；**公平性 ❌ 按原判据 FAIL 78/80** → 例外 **EX-1**。
- **三项开放缺口**（不因达标消失）：G-1 NFR-02 无采点；G-2 S4/S5 的 `reports/` 与 S6 一样被
  `.gitignore` 排除、S6 已归档而 S4/S5 未归档；G-3 中继运维面（限流/观测/告警）未单列验收。
- **同步落点**：夹具 `g2_claim=true` + `g2_claim_decided_at/by/ref` + `g2_claim_wording` +
  `g2_claim_carries_exceptions` + `g2_claim_open_gaps` + `g2_claim_does_not_mean`；
  DOC-31 顶部横幅/§0/§6/§7 改为「已声明达标」并指向 DOC-34；DOC-32 §0 与 §2-6、
  DOC-33 §0 与 §4 各加时间点附录（保留签署时点的原文）；
  `TEST.md` 顶部加状态横幅、§2.11 证据注、§6 门禁表；`TECH-STACK.md` 现状段更新。
- **顺带修正一处真实冲突**：`TEST.md` §6 门禁表的 G2 行原写「NAT 矩阵通过率达标」，
  与 `ROADMAP` §3 / ADR-016（类别覆盖非 pass 前提）冲突。已对齐为 ROADMAP 措辞并标注修正日期。
- **历史不回写**：ADR-012/014/015/016(R1–R3)/019–024、`G0-REVIEW.md`、CHANGELOG 历史条目、
  各归档证据 JSON 里的 `g2_claim=false` 均为成文时点记录，一律不改。
- **`g2-fairness-thresholds.toml` 未动**：其 `[gate_snapshot] g2_claim=false` 是**预注册时点的快照**，
  且该文件 sha256 被 EX-1 钉住（`0671a173…`），改它等于事后改判据。

## [2.4.140] - 2026-11-25 — 公平性 FAIL 写成书面例外 EX-1（含六条后果）；HighID 措辞定为「HighID 可达，direct 3/30」

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。无源码改动。

- **新增 DOC-33** `docs/06-plan/G2-FAIRNESS-EXCEPTION-2026-11-25.md`：把「公平性按预注册判据
  整体 FAIL、所有者接受、本版不重采」写成**可引用的书面例外 EX-1**，并把后果写死为 **C1–C6**：
  C1 判定仍是 FAIL 且引用必须带 78/80 与两轮数值；C2 结论强度上限（只覆盖「本版不修 β/不改阈值/不重采」，
  不覆盖 NFR 达标）；C3 三条失效条件（端侧机制改动 / 链路形状或物理环境变化 / 重新预注册判据）；
  C4 下一版义务（先登记新判据再采样、补 `git_commit`/`kernel_release`/`tc_qdisc_and_class`
  与逐轮原始目录、不得用「重跑同一判据这次全过」替代）；C5 正负措辞清单；C6 残留风险。
- **性质写明**：例外 = 「这个 FAIL 怎么被接受、影响哪些表述」，**不是改判、不是带例外 PASS**。
  `closed_with_exceptions` 是 S6 的词，公平性这一项不适用。
- **判据文件未动**：`tests/fixtures/g2-fairness-thresholds.toml` sha256
  `0671a173…`（与预注册时逐字节相同），例外落在裁定夹具 `g2-spike-verdicts.toml` → `[fairness_exception]`，
  不写回判据文件（否则等于事后改规则）。
- **HighID 措辞改单选**：`g2_exit_fold_in_highid_wording` 由 `both` 改为 `highid_reachable`
  （所有者 2026-11-25 17:12 指定）。约束：**分母 `3/30` 必须同现**，不得简写为「HighID 可达」
  或「稳定可达」，不得据此算比例；归档报告 `limitation` 原文不改，仍作审计出处。
- **同步落点**：DOC-31 §0 摘要行、§1.4、§6-1、§7；DOC-32 §5 勾选与口径说明、落点表；
  `RISK.md` 新增 **R-16**（判定阈值：用户可感知的长期偏斜，或跨物理主机复现）。

## [2.4.139] - 2026-11-25 — DOC-32 签署并同步：S6 按「条件计入」关闭 G2 出口的 NAT 可达性项

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。无源码改动。

- **所有者签署 DOC-32（2026-11-25）**，三项勾选：L1 = ③ **条件计入**、L2 = **可对外说
  「公网可达性按 eMule 语义结案」，但须同时给出四条例外**、HighID 措辞 = **两者并列**。
  L1/L2/例外清单都是本单据标注的推荐项；HighID 一项单据里未标推荐，所有者回复「三个都勾了」
  未指定单选值，执行方取「两者并列」——它是三选中唯一不丢信息的一项（既保留预注册判据下的
  「HighID direct 3/30」，也保留报告 `limitation` 的自述），且改回单选只需动一个字段。
  口径说明已写进 DOC-32 第 5 节，供复核。
- **落点同步（逐条见 DOC-32 第 5 节末表）**：
  ① `tests/fixtures/g2-spike-verdicts.toml` → `[[spike]] S6` 增 `g2_exit_fold_in="conditional"`、
  `fold_in_exceptions`（四条）、`g2_exit_fold_in_highid_wording="both"`、签署时间/决定人/决定单引用；
  **`status` / `verdict_kind` / `claim_wording` 未动**，`known_limitations` 未动。
  ② `ADR-016` 新增 `### R3（2026-11-25）— 折算决定`，只引 DOC-32 编号与结论，不重述理由。
  ③ DOC-31 第 3 节「是否折算仍需书面决定」与第 6 节第 3 项「仍未决」改为已决并指向 DOC-32，
  第 7 节与第 0 节「一句话」同步；顺带更正第 3 节里「报告不可复现」的过期说法（已归档可复核）。
- **这个决定不是 G2 达标**：它只决定「NAT 可达性这一项由 S6 关闭、关闭记录带四条例外」。
  G2 总判仍压在公平性一项上（按预注册判据整体 FAIL，所有者记例外，例外≠PASS）。
  禁止措辞照旧：`G2 达标`、`NAT 矩阵通过`、任意可达率/成功率、`因此 S6 折算所以 G2 完成`。

## [2.4.138] - 2026-11-25 — 更正 punched-direct 表述：3/30 有真实命中；新增三种「直连」术语表

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。无源码改动。

- **先更正我自己的错误**：上一版 DOC-32 写「punched-direct 未取得正面结果」是**错的**。
  S6 highid 报告 direct 路径 30 次里有 **3 次真打通**（`stun-mapped 112.5.60.4:47488`、
  `ack_status=OK`、`punch_acked=true`），且 `47488` 是 NAT 为出站流量临时分配的映射端口
  （内部监听 `17801`，`holder_mapping_differs_from_internal=true`），**属打洞，不是 DNAT**。
  其余 27/30 `acked=false`，原因仍未定位。正确表述是「仅 3/30，不是零，也不是稳定」。
- **实测确认家宽直连**：221 → `112.5.32.34:54022` `OPEN`（跨 CPE、不同公网 IP，不经 hairpin）；
  广州观测点 `119.29.150.254` 同时打两个家宽的 `54022` 均 `OPEN`。所以「221 与 222 各经自己
  的公网 IP 互连」是成立的——但形态是 `direct-via-static-dnat`。
- **新增术语表**（ADR-016「术语：三种『直连』不得互相冒充」）：`punched-direct` /
  `direct-via-static-dnat` / `relay` 三者的成立条件、现有证据、能否计入打洞，外加三条判定规则：
  ①打洞端口必须 ≠ 人工 DNAT 的固定端口（`47488` vs `54022`/`17800-17802`）；
  ②单侧公网不能当双侧能力；③静态映射不改变该出口对出站临时映射的分配行为
  （`symmetric` 仍是 `symmetric`，开映射救不回来）。
- **一处措辞不一致登记待定**：highid 报告的 `limitation` 自述「不构成 HighID direct 可达证据」
  （27/30 失败源于 symmetric 请求者 back 映射不稳），而判定文件按预注册 `succeeded > 0`
  计为 HighID。判定未违反预注册条文，但两种说法并存会让下一个人各引一句；
  DOC-32 已加「HighID 措辞」勾选项交所有者选定。
- 证据 `tests/ARCHIVE/evidence/home-direct-via-dnat-20261125.json`；DOC-32 升 v1.1。

## [2.4.137] - 2026-11-25 — 新增待签决定单 DOC-32：S6 是否折算进 G2 出口

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。无源码改动。

- **`docs/06-plan/G2-S6-FOLD-IN-DECISION-2026-11-25.md`（DOC-32，v1.0，待签）**：
  把「折算」拆成 L1 记账（G2 出口清单由谁关闭）与 L2 对外表述两层，给出三个选项
  （① 直接计入 / ② 不计入 / ③ **条件计入**，推荐 ③）及各自后果；
  选项 ③ 要求关闭时连同**四条结构化例外**一起写入出口记录
  （类别 3/4、direct 3/30 描述性、symmetric 0/30 属 NAT 行为、**punched-direct 无正面结果**）。
- **单向性写明**：签完该单**不会**让 `g2_claim` 变 `true`；G2 达标是独立动作。
  该单也不替代「G2 是否达标」总判——公平性一项按预注册判据仍整体 FAIL（例外 ≠ PASS）。
- **附第 6 节**说明「不测 restricted-cone」与「是否折算」是两个对象：
  前者改 S6 已知限制写法（ADR-016 §R2），后者改 G2 出口清单归属；
  混起来就会得到「因为不测了所以 G2 完成了」，本仓库禁止这句。

## [2.4.136] - 2026-11-25 — `restricted-cone` 退出待测清单；S6 证据改为 clone 后可复核

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。无源码改动。

- **所有者决定（2026-11-25）**：不再采集 `restricted-cone` 真实样本。**这是范围决定，
  不是判定变更**——S6 早在 2026-11-17 已判 `pass(closed_with_exceptions)`，
  `class_coverage_required_for_pass` 自始为 `false`，所以本轮不涉及「能否 pass」，
  只涉及「已知限制怎么写」。记录见 `ADR-016` 新增 `### R2`。
- **明写后果**：类别覆盖实际上限 **3/4**；`restricted-cone` 从此只以「按决定排除」出现，
  不再写「待补测/等真实环境」。`CGNAT` 不随本决定退出，仍未覆盖。依据：现网真 ADF 罕见；
  云上唯一可造 ADF 的路径是主机 `nftables` 自建 IP 集合，属**人造类别**，不得登记。
- **禁止措辞**：`restricted-cone 已覆盖` / `4/4 矩阵完成` / `NAT 矩阵通过` / `因此 S6 才 pass`。
- **补一个审计窟窿**：`g2-spike-verdicts.toml` 里 S6 引用的三份报告位于
  `tests/spikes/s6/reports/`，而该目录按 `.gitignore`（`/reports/*`）不入库——判定引用的
  证据在 clone 后读不到。现归档逐字节副本到 `tests/ARCHIVE/evidence/s6-verdict-20261117/`
  并附 `MANIFEST.json`（sha256＋字节数＋关键字段），`evidence` 路径改为副本，内容未改。
  副本复核：`sha256sum -c`。局限写在清单里：两份 v1 报告无 `git_commit` / `kernel_release`，
  可作类别与拓扑证据，不能定位到具体提交。
- **顺带更正一条口径（同日实测）**：家宽出口 `112.5.60.4` **不是地址池型 CGNAT**——
  从广州观测点主动连未请求端口 `54022` 得 `TCP_OPEN`，说明该公网 IPv4 未被多用户复用。
  边界：不能反推「单层 NAT 已证明」（独立公网 IP 的 1:1 运营商 NAT 外部不可区分）。
  历史「疑似双层/CGNAT」出自 macOS 那台的跳数启发式，`census.double_nat` 至今无 `true` 样本。
  判据与探针见 `docs/07-operations/TCP-DISCOVERY-RUNBOOK.md` §8.5/§8.6、
  `scripts/cpe_wan_probe.py`、`scripts/cpe_wan_probe2.py`（均只读），
  证据 `tests/ARCHIVE/evidence/home-egress-cgnat-check-20261125.json`。
- **G2 未完成**（本轮不改变）：公平性按预注册判据仍整体 FAIL（所有者已记例外、暂缓，
  例外≠PASS）；S6 是否折算进 G2 出口仍未决；`g2_note` 的实现项需逐条重置状态后才能判。

## [2.4.135] - 2026-11-25 — ADR-016 复审 R1：维持 eMule 语义，触发条件改为「≥4 出口且 ≥3 类别」

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。本轮无源码改动，只改判据文本与记录。

- **所有者决定（2026-11-25，三选一取第 ① 项）**：维持 eMule 式可达语义，
  **不恢复完整类别矩阵**；复审触发条件由「M4 前或 ≥4 个不同真实出口可用」
  改为「**≥4 个不同真实出口** 且 **≥3 个不同 NAT 类别** 同时成立」（先到为准）。
- **理由**：原文字只看出口数量，「加一台同类主机」就会让它反复成立、每次复审结论都相同，
  等于把一次判定摊成无限次；双条件让触发对应「环境可能出现新类别」这件事。
- **判定未放松任何约束**：`class_coverage_required_for_pass=false`、
  `matrix_complete_is_pass_condition=false`、真实设备 / 两台物理主机 / 每路径 30 次真实
  `WAKEUP_ACK` / 不自动 pass 全部不变；`s6_status` 仍 `not_run`；
  `restricted-cone`、`CGNAT` 继续登记为已知限制。
- **记录与同步**：`ADR-016` 升 v1.1，新增 `## 复审记录 R1`（含触发、决定、理由、
  未放松项、同步落点表）；同步 `tests/fixtures/g2-spike-verdicts.toml`
  （`[[review_trigger]] s6.semantics` 的 `trigger` + 新增 `reviewed_at` /
  `review_outcome` / `trigger_history`）、`tests/spikes/s6/nat_matrix_config.json`
  （`s6_stance_review_trigger` + `_reviewed_at` / `_review_outcome` / `_review_record` /
  `_trigger_history`）、`docs/02-architecture/adr/README.md` 索引行、`RISK.md` R-04 段；
  `G0-REVIEW.md` 保持 2026-11-17 原文字（日期化记录，不改写历史）。
- **自检**：`python tests/spikes/s6/nat_matrix.py --self-test` → **24/24 PASS**，
  其中 `s6_stance_is_emule_like_reach`、`class_coverage_not_a_pass_condition` 保持绿。
- **仍未决**：S6 判定是否**折算进 G2 出口**——与触发条件是两件事，本轮未动。

## [2.4.134] - 2026-11-25 — 广州云出口放通后实测 full-cone，S6 类别覆盖 2/4 → 3/4

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。本轮无源码改动。

- **背景**：所有者只改了广州那台（`119.29.150.254`）的轻量防火墙入向 UDP；
  北京那台（`82.157.68.159`）在控制台已找不到（疑似回收，SSH 仍可用），故未改。
- **映射轴**：绑固定本地端口 `40100/40101`，两个观测端口（`3478/3479`）得到同一映射
  `119.29.150.254:40100`、`:40101` → EIM。
- **地址轴**（分类工具做不了，独立实测）：`coldrevive-nat --classify` 的地址轴依赖
  STUN 服务端 CHANGE-IP，单公网 EIP 回 `change_ip_error=420`，只能报
  `cone-undetermined`；改用从未接触过该机的公网 IP（`112.5.60.4:15456`）向映射端口
  发未请求数据报——**到达、投递给该 socket、并回包**，同时证明：
  该公网端口对不同目的地址有效（EIM）、过滤不限源（EIF）。EIM + EIF ⇒
  RFC 4787 **full-cone**。
- **对照**：北京那台用同一脚本、同一源，`5000/19920/40000` 映射都建出来了但入向
  零到达（全 `NO-REPLY`）→ 探针无误报，北京仍 `port-restricted-cone`。
- **放通范围**：`5000/10000/19920/20000/30000/40000/50000/60000/65000` 全部被应答，
  含 `bt_port` 用的 `19920`，故节点用固定端口也能拿到 full-cone 入向。
- **类别覆盖**：`full-cone` ×1（119）、`port-restricted-cone` ×2（`112.5.60.4`、82）、
  `symmetric` ×1（`112.5.32.34`）= **3/4**，仍缺 `restricted-cone`、`CGNAT`。
- **措辞红线**：这是「云侧 1:1 静态 NAT + 安全组放通」的组合行为，不写成「天然
  full-cone 主机」，不写成「NAT 矩阵通过」，不出现可达率。
- 证据 `tests/ARCHIVE/evidence/s6-cloud-119-fullcone-20261125.json`；
  旧分类证据新增 `superseded_by_partial` 保留原观测；DOC-30 升 v1.3 新增 §8.4
  （地址轴的替代测法与必做对照）；DOC-31 升 v2.8；审计 §23、ADR-016 触发状态同步。

## [2.4.133] - 2026-11-25 — 云出口本来就是端口保持的 1:1 静态 NAT；类别由安全组白名单决定

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。本轮无源码改动，只做网络定性。

- **问题**：所有者问「这两台云主机可以设成 1:1 NAT 吗」。答案是**不需要设，已经是**。
- **映射轴**：客户端绑定固定本地端口后读 STUN `XOR-MAPPED-ADDRESS`，`40000 → :40000`、
  `40001 → :40001`，且两个不同观测端口得到同一映射 → 端口保持 + EIM，
  即真正的 1:1 静态 NAT。
- **过滤轴是安全组白名单，不是状态过滤**（更正 §23 旧归因）：同机同时刻 A/B——
  放通端口 `3479` 接受从未接触过的源端口（`119.29.150.254:5577`）与完全不同的公网 IP
  （`112.5.60.4:14393`）打入并正常应答，按 RFC 4787 属 **full-cone**；
  未放通端口 `40001` 的入向包主机侧一个都没看到，属 port-restricted。
- **主机侧不是过滤源**：两台 `iptables -S INPUT` 只有 `YJ-FIREWALL-INPUT` 的 SSH 暴破黑名单。
- **含义**：云主机的 NAT 类别是**安全组放通范围的函数**。放通 UDP 端口区间即可得到
  第三个真实类别（full-cone），类别覆盖 3/4（缺 restricted-cone、CGNAT），无需改代码；
  因为映射端口==本地端口，只放通一个固定端口也能让节点具备 full-cone 入向能力。
- 证据 `tests/ARCHIVE/evidence/s6-cloud-egress-1to1-nat-20261125.json`；
  DOC-30 升 v1.2（新增 §8.3）；审计 §23 同步更正。

## [2.4.132] - 2026-11-25 — 公网直拨 BT 闭环；更正「CPE 映射失效」误判

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。以下是单条公网路径的组件级证据，
> 不是 NAT 矩阵结论，也不是 G2/S6 判定。

- **公网直拨 PASS**（`bt-wake-on-handshake-public-20261125.json`）：B 以
  `engine.listen=127.0.0.1:19920` + `wake_on_handshake=true` + `nat.bt_external=112.5.32.34:19920`
  启动，guard 绑 `0.0.0.0:19920`。A 从 `112.5.60.4` 拨入，B 抓到
  `112.5.60.4.40508 > 192.168.34.222.19920` 的 SYN 与 68 字节握手；冷种子 `cold → hot`
  （`guard.handshakes=1, woken=1`，**未调用 `nat wake`**），A `complete` 1048576 B，
  落地 md5 `6209e962f3b26a13c9ec1ae70fd78e10` 与种子一致。两个独立公网观测机
  （`82.157.68.159`、`119.29.150.254`）都能完成到 `112.5.32.34:19920` 的 TCP 握手。
- **更正 v2.4.131 的误判**：当时把观测机收到的 RST 读成「CPE 映射已撤」。实际是
  `0.0.0.0:19920` 无监听（引擎只绑回环、guard 未启用），RST 由主机内核发出；映射一直在。
  旧证据已加 `correction_20261125_public` 字段，`TEST-RUNS-2026-11-25.md` 追加 INFO 更正条目。
- **环境记录修正**：两台主机的公网 SSH 是 **`54022`**（`112.5.60.4:54022` → 221
  `ubuntu`，`112.5.32.34:54022` → 222 `ubuntu23`），不是 22；本机在 `192.168.1.0/24`
  内连 `112.5.60.4` 会因家宽不支持 hairpin 而超时，公网可达性须从外部观测机测。
  已写入 DOC-30 §8。
- 文档：DOC-31 升 v2.7（第 4 项改为已闭环）、审计新增 §22.9（含误判更正与判定顺序）、
  DOC-30 升 v1.1。回归仍为上一轮的 541/0/3，本轮未改引擎/协议代码。

## [2.4.131] - 2026-11-25 — 握手命中即唤醒（FR-14 语义由所有者定为「入向握手唤醒冷种子」）

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。以下是组件级证据，不是 G2/S6 判定。

- 所有者决定：冷种子遇到入向 BT 握手时**唤醒**，而不是「被通告为 peer 期间不暂停」。
  实现复用 FR-11 的 `cr-guard`（librqbit 无握手回调，ADR-002），新增配置别名
  `engine.wake_on_handshake`（与 `engine.guard_handshake` 同义，任一布尔 true 即开；
  字符串不生效，`cr-core` 单测钉住）。
- 修 `start_engine_seeder()` 的对外绑定：guard 原先沿用 `listen`，而部署里
  `engine.listen=127.0.0.1:<bt_port>` 是为把 LAN 端口让给打洞 socket，导致 guard 只绑回环，
  DNAT 来的握手永远进不了唤醒逻辑。现声明了 `nat.bt_external`/`nat.http_external` 时
  一律绑 `0.0.0.0:bt_port`（`guard_bind_addr()`，含两条单测），引擎仍在回环内部口。
- 修唤醒后的时序：`activate` 返回 Ok ≠ 引擎已可处理握手。立刻 splice 会让仍在暂停的
  torrent 回 FIN，与「没唤醒」无法区分。现在有界等待引擎自报 `HOT`（≤3 s），
  超时也照常 splice，由引擎按真实状态回答。
- 现场实测（`bt-wake-on-handshake-lan-20261125.json`，LAN 直拨 A→B:19920）：
  B 种子拨号前 `cold`、`seed_active=0`；握手到达后 `/health.guard`
  `handshakes=1, woken=1`、种子 `hot`（全程未调用 `nat wake`）；A 侧
  `status=complete`、`completedLength=1048576`，落地 md5
  `6209e962f3b26a13c9ec1ae70fd78e10` 与 B 种子文件一致。
  B 的监听是 guard `0.0.0.0:19920` + 引擎 `127.0.0.1:43693`，证明绑定修复生效。
- **公网 DNAT 变体本轮未复测**：测试时两个独立公网观测机对 `112.5.32.34:19920`、
  `112.5.60.4:19910/17800` 均收到 RST，B 侧零 SYN——CPE 映射已失效，属环境变化。
  证据中以 `public_path_probe` 记录；公网直拨需映射恢复后重跑，不得用 LAN 结果冒充。
- 已知取舍：guard 长期占住 `bt_port`，与同端口打洞 socket 互斥（逐节点选择，默认关闭）。

## [2.4.130] - 2026-11-25 — tracker 跨网段下发公网 BT 映射（下载端已能拨出；供片仍卡冷种子）

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。以下仍是组件级证据。

- 修 `cr_nat::endpoint::tracker_endpoint_for()`：私网请求方不再一律拿对端私网地址。
  新增「同 /24 才走局域网」判定——A 在 `192.168.1.0/24`、B 通告的私网地址是
  `192.168.34.222`，旧逻辑把不可路由的内网地址回给 A，A 的引擎永远 `connections=0`。
  现在跨网段且有真实公网映射（公网 IPv4 且 `internal_port == bt_port`）时下发该公网地址；
  同网段仍走局域网，无真实映射时仍退回私网地址（不伪造可达性）。
- `Relay` 会合地址不参与这条复用路径（语义是「拨本机桥」，不是端口映射）；
  对公网请求方的行为逐字节不变。
- 单测：`cross_subnet_private_requester_gets_public_mapping`、
  `cross_subnet_private_requester_without_mapping_stays_private`（`cargo test -p cr-nat` 18/18）。
- 现场验证（`tracker-public-endpoint-dial-20261125.json`）：
  A 向 B 的 `/announce` 得到 `112.5.32.34:19920`（旧值为 `192.168.34.222:19920`）；
  A 的引擎随后**真实拨出**，B 上 `tcpdump` 抓到
  `112.5.60.4.38695 > 192.168.34.222.19920: [S]` + `length 68` 真 BT 握手。
  结论 **PARTIAL**：控制面已通，数据面仍被 v2.4.129 记录的冷种子行为阻断
  （B 引擎不回首握手，`seed_active=0`）。A 落地文件 1048576 B 但 md5 为
  `b6d81b36…`（1 MiB 全零 = 引擎预分配，零字节到达），已明确区分，不算传输成功。
- 更正 v2.4.129 的表述：旧逻辑不是「返回空 peer 列表」，而是「返回对端私网地址」。

## [2.4.129] - 2026-11-25 — daemon 固化 systemd unit；直拨 B 公网映射：网络层 PASS、应用层 BLOCKED

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。

- 221/222 的 daemon 由 `systemd-run` 瞬态单元改为正式 unit
  （`coldrevived-a` / `coldrevived-b`，`Restart=on-failure`，`systemctl enable` 开机自启）；
  B 同时从 `/tmp` 迁到 `/opt/cr-punch-pub/222`，重启不再丢二进制与配置。
  证据 `daemon-systemd-units-20261125.json`（两机 health ok、同 md5、unit enabled+active）。
- 改测「不打洞、直接拨 B 的静态映射 `112.5.32.34:19920`」：
  A（`112.5.60.4`）与云观测机 `82.157.68.159` 都能与 `222:19920` 完成 TCP 三次握手，
  B 上 `tcpdump` 各记录 8 个包 → **网络层 PASS**。
- 应用层 **BLOCKED**：两个客户端发出 68 字节真实 BitTorrent 握手后，
  B 的引擎先 ACK 再立即 FIN，全程不回握手。根因是种子 `state=cold`
  （`idle::sleep_hot_seeds()` → `pause_seed()` 暂停了引擎内 torrent），
  被暂停的 torrent 不接入向 peer；中继路径能过正是因为它会先 `nat wake` 激活持有者。
- 加热路径当前不通：直接 `nat wake` 被拒（无 NAT 计划）；补 `nat peer` + `nat probe`
  后 `wake ok=true` 但 `path=relay`，种子仍 cold。三组对照均归档为 BLOCKED。
- 独立缺陷：`tracker_endpoint_for()` 对私网请求方返回空 peer 列表，
  下载侧 aria2 兼容接口长期 `connections=0`，引擎从未拨向 `112.5.32.34:19920`。
- 结论：剩余成本不在 CPE/NAT，而在两处应用层行为（入向握手唤醒冷种子 /
  有公网 `bt_external` 时下发公网 peer）。属行为变更，待所有者确认，本轮不改代码。

## [2.4.128] - 2026-11-25 — 守护进程自动 DHT 10 分钟有界长跑 PASS；S6 语义复核材料补齐

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。以下仍是组件级证据。

- 221 上以 `dht.enabled=true` + 两个 IPv4 bootstrap 运行 10 分钟，
  20 次 `/health.dht` 采样得 5 cycles / 1 attempt / 1 verified，
  0 no_token / 0 failed / 0 resource_skipped；RSS 20 次均 17,376 KB。
- 两个独立公网节点只读 `get_peers`：其中 `185.86.100.99:6881` 在 211 s 与 422 s
  回查到 `112.5.60.4:19910`；另一节点两次均 `peers=0`，如实保留。
  生成摘要中的子串计数已用 `summary_corrected` 更正为 2 次真实命中。
- 两次准备失败（配置未落盘、过早把 `no_bootstrap` 状态当失败）分别归档为 BLOCKED；
  成功运行也单独归档 `TEST-RUNS-2026-11-25.md`。
- S6 语义复核材料补入 DOC-31 / 审计 §23：`pass(closed_with_exceptions)`、
  机器字段 `not_run`、`s6.semantics` 触发条件未满足（仅 2 个真实出口）、
  原始报告不可由 checkout 复现，且禁止写成 NAT 矩阵/G2 通过或可达率。
- DOC-31 升 v2.3，审计新增 §24，docs 重建并入索引。

## [2.4.127] - 2026-11-25 — 当前 daemon 已部署；真 BT 走公网中继 PASS；打洞直连因 B 非 EIM 如实 BLOCKED

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。以下均为组件级证据，不宣称 G2/S6 通过。

- **代码修复并部署**：`punch-route` / `punch-route-answer` 的新中继连接先 `register`
  再交换裁决；`nat punch-route` 接受第四参数共享 nonce（32 位十六进制），
  支持两端真同时打开同一会话。定点单测通过；当前源码在 221 原生构建
  （md5 `1ba84f286fdab265e936fa65dfbc69e1`）并部署到 221/222，两端健康 OK。
- **端口布局**：本地引擎改为 `engine.listen=127.0.0.1:<bt_port>`，释放 LAN 的
  `19910/19920` 给绑定源端口 punch socket；`bt_external` 与通告口径不变。
- **中继闭环 PASS**：真 BT `.torrent` 在 B 侧 aria2 `status=complete`、
  `completedLength=1048576`，落地 md5 `6209e962f3b26a13c9ec1ae70fd78e10` 与 A 一致；
  B `bytes_relayed=1051320`、`tunnels_alive=1`，A `holder_data.bytes_from_engine=1049585`。
- **打洞直连 BLOCKED**：同会话双观测点实测 A 为 EIM（`34656`/`34656`），
  B 映射随目的变化（`2050` vs `2055`）。按 DOC-30 约定，B 不给 discovered_port、
  立即回退中继；没有拿单观测凑 PASS。
- 全量串行回归 **537 passed / 0 failed / 3 ignored**；本轮 PASS/BLOCKED/部署/回归
  全部归档 `tests/ARCHIVE/TEST-RUNS-2026-11-25.md`。
- DOC-31 升 v2.2，审计新增 §22.3–22.5 与 §23，DOC-30 runbook 修正 `/cli` 形状并补共享 nonce 与端口布局。
- S6 语义复核材料补入既有简报/审计：已判 `pass(closed_with_exceptions)`、机器字段仍 `not_run`；
  `s6.semantics` 触发条件是 M4 或 ≥4 个真实出口，当前仅 2 个，未触发；原始 S6 报告仍不可由 checkout 复现。

## [2.4.126] - 2026-11-25 — 公平性偏差所有者决定：记录在案、本版暂缓，下一版重调测试

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。本轮无代码行为变更。

- 所有者判定 L2 `cc_on` / `cc_paced` 各 1 轮的公平性偏差在可接受范围：
  **记录在案、本版跳过**，不调 β=0.5、不改阈值 sha `0671a173…`、不重抽样。
- 下一版再重新调整测试：必须先以新时间戳 + 新 sha 登记新预注册判据，再采集新样本；
  旧 80 轮证据保持原样，不删失败轮。
- DOC-31 升 v2.1、审计 §21 同步记录该决定；G2 出口与 S6 语义仍待所有者另行判定。

## [2.4.125] - 2026-11-25 — 报告口径更正：CPE DNAT 仍在、公网 TCP 打洞复测 PASS、简报改为通俗版

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。本轮无代码行为变更，不宣称 G2 解禁。

- **更正 v2.4.124 的错误判断**：当时据「本机连公网地址超时/被拒、两机无监听」写成
  「两条 CPE DNAT 映射已撤销」，这是误判。punch 端点是实验期临时进程，无监听不等于
  DNAT 消失；本机同出口超时是 hairpin 限制；`ConnectionRefused` 反而说明映射在转发。
- **真机复测**：用临时监听验证 222→`112.5.60.4:19910` 与 221→`112.5.32.34:19920`
  双向 PING/PONG PASS；82/119 双观测点 tcpdump 看到 221 映射端口 `112.5.60.4:31480`
  一致；发现后 3480 ms 内双侧 `--tcp-punch` 均 `ok=true`、`bytes_roundtrip=true`。
  一次慢 11 s 的失败尝试如实归档为 BLOCKED 对照。证据归档 `TEST-RUNS-2026-11-25.md`。
- **部署实况澄清**：221/222 的 `/tmp/coldrevive-nat`（11-24 11:49）已含 `tcp_discovery`，
  可供 DOC-30 探针使用；陈旧的是 daemon（221 11-20、222 11-18）与 221 源码树。
  仍缺当前 daemon 部署 + 真 BT 字节跨公网打洞，**DNAT 不需要重建**。
- **DOC-31 改为通俗版 v2.0**：先给四句话结论表，再逐项解释公平性在测什么、
  DHT/逐出过没过、CPE DNAT 误判如何更正；审计 §22.1–22.3 同步更正。

## [2.4.124] - 2026-11-25 — G2 欠账复评：DHT/逐出证据补齐、公平性归因、BT-over-punch 组件闭环


> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。本轮**不宣称 G2 解禁**，
> 最终判断权在所有者。简报：`docs/06-plan/G2-REVIEW-BRIEF-2026-11-25.md`。

- **真实 Mainline DHT 证据缺口闭合**：`coldrevive-dht --walk` 输出 `contacts`、
  `--get-peers` 输出 `node_list`；新增 `tests/e2e/w62/mainline_dht_announce_verify.py`
  证明真 token → announce ACK → ≥2 个独立公网节点用新 RPC 回查到同一 peer；
  新增 `--announce-seed` 走守护进程 FR-18 的 `announce_active_seed`，对真节点返回
  `announced_verified`（token_len=4、peer_verified=true）。引导节点仍 token_len=0。
- **守护进程多样种逐出 e2e 刷新**：当前 release 构建跑 `tests/e2e/p0/run_p0.py`
  77/77，FR-10 两个 infohash 真握手、cap=1、evictions=1、seed_active<=cap。
- **公平性定量归因（仍整体 FAIL）**：80 轮原始证据的 L2 cc_on+cc_paced 20 轮
  Pearson r(丢包差,份额偏移)=−0.706，两轮失败是丢包不对称最大者；决定**不改 β、
  不重抽样**（判据先于样本，调参即篡改）。审计 §21。
- **BT-over-punched-TCP**：公网复评当时记 BLOCKED（**「CPE DNAT 已撤」为误判，见 v2.4.125 更正**；
  两机无临时监听属实验结束常态，daemon 二进制陈旧属实）；
  代码级补 `bound_source_port_socket_carries_real_librqbit_piece`，
  用 DOC-30 的 `connect_bound_blocking` 固定源端口 + 解耦登记键，真引擎下载 1 MiB
  逐字节一致、served_registered=1。审计 §22。
- 全量串行 **536/0/3**（v123 535 +1）；全部运行归档 `TEST-RUNS-2026-11-25.md`。

## [2.4.123] - 2026-11-24 — JSON 解析 O(n²) 修复、FR-04 规模闭环、punch 桥键纠错

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。本轮无公网新证据，不外推 G2/S6。

- **`cr-core` JSON 解析 O(n²) 修复**（FR-04 规模根因）：`Parser::string()` 原实现对每个
  非 ASCII/普通字节都 `from_utf8(&b[i..])` 验证整段剩余缓冲，解析复杂度 O(n²)。改为
  ASCII 直推 + 按 UTF-8 lead byte 只切片单字符（≤4 B）验证；新增 2/3/4 字节
  UTF-8 往返单测。实测 10k 条目快照 open **110701 ms → 137 ms**（约 800×）。
- **FR-04 规模上限闭环**：新增 `cr-store` `json_snapshot_roundtrips_ten_thousand_entries...`，
  10k 条目 3.87 MB 快照 save≈213 ms / open≈137 ms，重启后条目/字段抽样一致，
  5s 回归护栏（非 SLA）。审计 §12 FR-04 行同步；**不引入 SQLite/新依赖**——
  SRS FR-04 只要求重启恢复，不指定存储引擎，JSON 原子快照已满足且规模可控。
- **`nat punch-route` 桥键纠错**（v2.4.122 引入的真 bug）：发现端口用作 SYN 目的地址，
  但打洞 socket 曾按发现端口登记进 SOCKS5 桥；引擎实际拨静态 `bt_external`，
  桥键不匹配会 miss 并回落直连，打洞白做。新增 `PunchTargets` 显式分离
  `syn`（发现端口）与 `bridge`（静态通告地址），单测断言二者不同时桥键仍为静态地址。
- **eD2k 未实现特性的决策固化**：核查 Kulbak/Bickson §6.2 与 aMule Wiki，登录帧格式
  公开但自述 "WILL have errors" 且 tag 布局互相冲突；AICH/credit 已被 ADR-004/011
  明确排除。PROTO §2.2.2 记录"为什么不猜"，三个 `UnsupportedFeature` 维持原状。

## [2.4.122] - 2026-11-24 — 字节口径标注、发现端口接线、公平性失败轮根因

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。本轮无公网新证据，不外推 G2/S6。

- **`/health` 字节口径**（PHASE-SUMMARY §4 可观测性项）：新增顶层
  `byte_accounting`，显式区分 `engine_reported_uploaded`（引擎自报）、
  `tunnel_written`（写入本地 SOCKS5/隧道）、`splice_written`（写入本地 socket）。
  `peer_received_bytes` 恒为 `null`、`peer_received_status=not_observable_from_this_process`
  ——本进程观测不到对端落地，绝不伪造数字；对端实际收到仍只由 e2e 读对端文件 md5
  证明。`data_plane.bytes_relayed_scope` 与 `holder_data.bytes_scope` 同步标注口径。
- **`nat punch-route` 接发现端口**（DOC-30 控制面接线）：第三个可选参数接受
  EIM `discovered_port`，**只替换端口、IP 仍取已登记 `bt_external`**，拒绝 0 /
  越界 / 非数字 / `ip:port` 字符串；成功结果带 `discovered_port_used` 便于审计。
  不给参数时行为不变。仍是组件级控制面，`s6_status=not_run`。
- **公平性失败轮根因**（不抽样、不改阈值）：从 221 原始 `summary.stdout.json`
  定位 L2 cc_on 第 7 轮 cwnd 19.19/22.01（丢包 7/5）、L2 cc_paced 第 8 轮
  cwnd 27.24/32.00 封顶（丢包 3/0）——份额 0.43/0.57 来自独立 per-ACK AIMD
  的 cwnd 漂移，不是丢包不对称，也不是中继整形（shaped/dropped/limited 全 0）。
  原始证据归档 `tests/ARCHIVE/evidence/g2-fairness-summary-20261124-v121.json`。
  阈值 sha `0671a173…` 未变，不重跑抽样「碰运气」。
- **FR-25 源字典编解码接线**：`coldrevive-ed2k source-codec --file <样本>`
  新增子命令，用真实 MD4 fileID 按 PROTO §2.2.1 编码 `0x44`/`0x34` 并回解，
  校验 aMule 接纳前置条件（TAG_SOURCETYPE + TCP/UDP 端口非零，端口 0 报错）。
  输出对 server login / credit / AICH 如实回 `Unsupported`——仓库无已验证线格式，
  不猜包。只证线格式，**不发包、无 0x4B ACK、不等于源字典已发布**。
- 全量串行 `CARGO_INCREMENTAL=0 cargo test --workspace --offline -- --test-threads=1`
  为 **532 passed / 0 failed / 3 ignored**（v121 530 + 2 新单测）。
  定点与全量结果均已归档 `tests/ARCHIVE/TEST-RUNS-2026-11-24.md`。

## [2.4.121] - 2026-11-24 — tcp_discovery 文档化与定点测试：EIM 判定四态固化

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。本轮不新增公网证据，不外推 G2/S6。

- 新增 [DOC-30 TCP 映射端口发现与 EIM 判定 Runbook](docs/07-operations/TCP-DISCOVERY-RUNBOOK.md)：
  固化 EIM 四态判定、`--serve-observer` / `--observer-secs` /
  `--discover-tcp-port` / `--timeout` / `--json` 参数与输出字段；把本办法写成
  「常规打洞失效时先判断是否适用」的应对流程，含适用前提、不适用信号、
  标准操作、回退与证据边界。TEST §11 / 审计 §20.5 同步引用。
- `cr_nat::tcp_discovery::judge_observations` 抽出纯判定函数：0 观测、全失败、
  单观测、`>=2` 一致 / 不一致各自结论可单测；**不一致时不再返回
  `discovered_port`**，避免把不可外推端口当信令。`parse_port` 拒绝端口 0。
- 定点测试：9 条 `cr-nat` 单测（原 4 条 + 5 条）+ 新增
  `tests/e2e/w56/tcp_discovery_selftest.py` 6/6 PASS（双观测一致 EIM、
  同端点、单观测 `eim=null`、不可达 `eim=false`、假 observer 不一致不出端口、
  reason 明确）。命令见 DOC-30 §6；全量串行仅作最终回归，不替代定点测试。
- 文档同步：DOC-00 索引与 `index.html` DOCS 清单新增 DOC-30。
- 全量串行回归 `CARGO_INCREMENTAL=0 cargo test --workspace --offline -- --test-threads=1`
  为 **530 passed / 0 failed / 3 ignored**（比 v2.4.120 多 5 条定点单测）。
  定点与全量结果均已归档 `tests/ARCHIVE/TEST-RUNS-2026-11-24.md`。

## [2.4.120] - 2026-11-24 — 动态 TCP 映射端口发现：发现端口作信令后 simultaneous-open 实测 PASS（组件级）

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。这是组件级证据，不是 G2 解锁。

- 新增 `cr_nat::tcp_discovery` 与 `tcp_punch::connect_bound_blocking`。
  `--serve-observer` 回写 `CR-MAPPED <ip>:<port>`；`--discover-tcp-port` 从固定本地
  端口向 observer 发真 TCP 连接读回映射端点，>=2 observer 一致才判 EIM，
  单 observer 只给端口、EIM 记 `null`。4 条单测。
- 双公网观测点 tcpdump 证据（`82.157.68.159` / `119.29.150.254`）证明 221 从
  `19910` 到不同目的都映射成 `112.5.60.4:20811` → EIM 成立。这是 §20.3
  `Connection refused` 的真正原因：该用发现的映射端口，不是配置里的 19910。
- `tests/e2e/w56/public_tcp_punch_discovered.py` 把发现端口作为信令字段交给 222，
  222 连 `112.5.60.4:<discovered>`，221 连 `112.5.32.34:19920`。首次报告判 **PASS**
  （两侧 `ok=true`、`bytes_roundtrip=true`），再连续 3 轮带 tcpdump 全部 PASS；
  抓包显示真实同时打开 SYN 交叉且源端口 == 发现端口。证据见审计 §20.5。
- 82 上用于发现的临时 REDIRECT 已撤销，中继 7100 复测可连。
- 全量 `cargo test --workspace --offline -- --test-threads=1` 为
  525 passed / 0 failed / 3 ignored。默认并行下 2 次既存计时型 flaky 失败
  已如实归档，隔离/串行通过。

## [2.4.119] - 2026-11-24 — 公网 TCP DNAT 验收：环回 PASS，simultaneous-open 仍 BLOCKED

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。本轮未改 Rust 代码。

- 所有者建立 `112.5.60.4:19910 → 192.168.1.221:19910` 与
  `112.5.32.34:19920 → 192.168.34.222:19920` 后，跨主机连接收到预期标记；
  工作机到 221 的超时是自身出口与 221 公网地址同址、网关不支持 hairpin，不是 DNAT 失败。
- 新增 `tests/e2e/w56/public_tcp_dnat_roundtrip.py`：两方向各监听/连接一次并做
  PING/PONG，报告 `public-tcp-dnat-roundtrip-20261124T102845Z.json` 判 **PASS**。
  这只证明 DNAT 承载真实 TCP 双向字节，明确不冒充 simultaneous-open。
- 原 `public_tcp_punch_probe.py` 重跑仍 BLOCKED。双侧抓包证明 221 绑定源端口
  `19910` 被上游 NAT 改写为 `18011`（同一 SYN seq），对端 4 元组不匹配而回 RST；
  需要上游保持源端口，或所有者书面改判据，才能改变该结论。审计 §20.3 已更新。

## [2.4.118] - 2026-11-24 — 公平性根因修复与同阈值重跑；TCP DNAT 前置复核

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。全量回归 **521 passed / 0 failed / 3 ignored**。

- **公平性根因**：§20.1 的 cap arms 失衡来自中继每会话令牌耗尽就丢帧回
  `BandwidthLimited`，发送端按超时退避，SRTT/RTO 涨到数百毫秒、cwnd 落底，
  两条同速率流被随机错配。可靠层现在把该 STATUS 作为背压信号做一次快重传
  （不算 ACK、不算丢包、不做 RTO 指数退避），并暴露 `udp_cc_backpressure`。
- **有界整形**：每会话预算不足的 DATA 进入每会话 64 帧 / 总 256 帧队列等令牌，
  按队首顺序放行；队列满才明确拒绝。计数拆为 `per_session_limited /
  per_session_shaped / per_session_dropped`，配置/审计文档同步更新。
- **同阈值重跑仍整体 FAIL**：预注册 fixture sha256 `0671a173…` 未改，221 上
  2 形状 × 4 arms × 10 轮全部完成：`g2-fairness-20261124T085447Z`。
  所有 L1 arms 与 L2 cc_off / cap 均 10/10；L2 cc_on、cc_paced 各 9/10
  （份额 0.430/0.570 与 0.433/0.567）。按 `all_rounds_must_pass` 仍是 FAIL，
  不再抽样碰运气，也不改阈值；门禁不自动解禁。
- **TCP DNAT 复核**：221 的 `112.5.60.4` 与 222 的 `112.5.32.34` 都是上游网关
  地址；221 本地 19910 监听 READY 但公网 TCP 连入超时，两侧 nat 表无 1991x 规则，
  现有 key 也进不了网关。审计 §20.3 给出两条 endpoint-independent DNAT 步骤与验收命令；
  规则建好前保持 BLOCKED。证据 `tests/ARCHIVE/evidence/tcp-dnat-reachability-20261124.json`。

## [2.4.117] - 2026-11-24 — G2 欠账推进第二轮：公平性阈值实测、FR-25 daemon 公网闭环

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。全量回归 **519 passed / 0 failed / 3 ignored**。

- **公平性证据（预注册判据，整体 FAIL）**：先写入 `tests/fixtures/g2-fairness-thresholds.toml`
  （sha256 `0671a173…`，Jain ≥ 0.95 且两流份额均在 [0.45,0.55]，每轮都必须达标，
  4 arms × 2 链路形状 × 10 轮）再采样；新增 `coldrevive-fairness-probe` 真
  `UdpRelayStream` 双流探针、`tests/e2e/w61/run_g2_fairness.sh`（veth+HTB+netem）
  与 `analyze_g2_fairness.py`。L1 的 cc_off/cc_on/cc_paced 与 L2 cc_off 全 10 轮达标；
  L2 cc_on、cc_paced 各 1 轮越界；两个每会话上限档多轮失败（含 2 次超时）。
  按预注册 `all_rounds_must_pass` 判 **FAIL**，阈值未事后修改，仍不写「公平性达标」。
  工具配置失败（1 B/s 桶、cap 未触发、CRLF）均已归档后修正重跑。
- **FR-25 daemon 公网闭环**：`ed2k-download` 新增 opt-in `--out`，只在
  `hash_matches/content_matches/completed` 全真时 `.part`→rename 落盘，结果带
  `out_path/landed_bytes`；真 daemon `/cli` 公网 A/B 两臂 PASS：
  221 daemon→222 daemon 跨分片 9,736,192 B、221 真 aMule→222 daemon 262,144 B，
  落地 md5 全部一致。测试期间暂停的 221 aMule 已用 `systemd-run` 恢复 active。
- **公网 TCP 打洞前置复核**：19910/19920 仍不可达且两端无本地 DNAT，保持 BLOCKED；
  需要所有者在 CPE 建 endpoint-independent TCP DNAT 后重测，symmetric 不可直连是正确结论。

## [2.4.116] - 2026-11-24 — G2 欠账推进：拥塞语义、DHT 治理、打洞 handoff、FR-25 公网

> 门禁常量全部不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。**没有自动解禁，也不把组件级证据
> 外推为 G2/S6 通过。**全量回归 **519 passed / 0 failed / 3 ignored**。

- **可靠层拥塞语义（ADR-024 阶段 3，opt-in）**：`StreamCore` 增加 AIMD（ACK 加性增长、
  每次丢失乘性减半且每 RTT 最多一次）、Jacobson–Karels RTT/RTO、可选 pacing；
  `UdpLimits` 增加每会话字节桶（0=关），与全局桶叠加防止单流独占。
  配置 `nat.udp_cc_*` 默认全关，中继 `--udp-per-session-rate/--udp-per-session-burst`，
  `/health.data_plane` 暴露 `udp_cc_cwnd_x100/srtt_ms/loss_events/rtt_samples/paced_new`。
  这只是**机制实现**：没有跨公网多流共享瓶颈与预注册公平阈值，仍不能说「公平性达标」。
- **DHT 产品化治理**：`dht_daemon` 引入 stop 句柄、有界 `find_node` 发现（先越过不发 token
  的引导节点）、每轮宣告预算与请求间隔、max_seeds/max_keys/RSS 软跳过；网络 I/O 移出
  `NodeState` 锁；`/health.dht` 增加 `last_cycle_status/cycles/requests/discovered/resource_skipped`
  且不再用历史 verified 永久覆盖本轮失败。`DhtNode` key 总数硬封顶并淘汰最旧 key。
- **打洞 handoff 真接线**：`nat punch-route` 以前验完 socket 就 drop；现在 `Route::Direct`
  时经 `DataPlane::attach_punched_socket` 登记进 SOCKS5 桥，`/health.data_plane` 分开报
  `punched_registered` / `punched_alive`。回环测试经真 SOCKS5 CONNECT 完成 hello/world 往返。
  公网双侧同时打开仍 **BLOCKED**：221/222 各 34 次 SYN 均被静默丢弃，19910/19920 无 TCP DNAT，
  222 又是对称 NAT；symmetric 不可直连按既定口径结案。
- **FR-25 公网组件级闭环**：222 → `112.5.60.4:4662` 从已共享 aMule 样本下载 262,144 B，
  `hash_matches/content_matches=true`；随后 221 用我们的 `serve --compressed-parts` 暂占映射端口，
  222 下载 9,736,192 B（>PARTSIZE），`hash_matches/content_matches/completed=true`。
  过程中发现我们自己的下载端只认 `0x46`、无法解析 `0x40`，新增 std-only zlib inflate
  （stored/fixed/dynamic + Adler-32 + 输出上限），修复后公网回环才通过。测试结束已恢复 aMule。
- 所有失败、BLOCKED 与日志覆盖更正均登记 `tests/ARCHIVE/TEST-RUNS-2026-11-24.md`；
  审计新增 §19，配置/测试文档同步。门禁仍需所有者书面复评，脚本不自动 pass。

## [2.4.115] - 2026-11-24 — FR-18 真实 Mainline DHT announce + 异机回查闭环

> 无源码改动；门禁常量不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。

- 从 `dht.transmissionbt.com:6881` 只读 `find_node` 发现公网邻居
  `37.48.95.218:3691`；该节点对真实 infohash 签发 `token_len=4`。
- 先确认所宣告端点 `112.5.32.34:17200` TCP 真可连接，再按 2026-11-20 的
  所有者授权从 222 执行受控 `announce_peer(17200)`；从**另一台主机 221**
  对同一节点独立 `get_peers` 回查到 `peers=1`、`peer_list=112.5.32.34:17200`。
- 证据 `tests/e2e/dht-announce/reports/mainline-announce-verify-20261124.json`
  与 `tests/ARCHIVE/evidence/mainline-*-20261124.jsonl`；测试已登记
  `tests/ARCHIVE/TEST-RUNS-2026-11-24.md`。这只闭合 FR-18 证据项，
  **不宣称 G2 达标，也不表示 daemon 已自动加入 DHT**。

## [2.4.114] - 2026-11-24 — FR-25 上传方向 LAN 闭环：同构 OP_COMPRESSEDPART

> 门禁常量不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。**公网仍未测，不宣称公网性能。**

- `cr-ed2k::wire` 新增 stock-aMule 同构的 `OP_COMPRESSEDPART(0x40)/PROT_EMULE(0xC5)`：
  `hash(16)+start(u32 LE)+compressed_len(u32 LE)+zlib 流`，每请求块一帧。
  字节布局由真 stock amuled 的抓包逐帧定死（`compressed-frames-20261124.hex`，
  54 帧、解压后与样本逐字节一致），并与 aMule `CreatePackedPackets` /
  `ProcessBlockPacket` 源码核对。
- 压缩用 std-only 的贪心 LZ77 + 固定 Huffman（`zlib_compact`）；不可压数据时回退
  stored DEFLATE。stored-only 版本虽然小块能落地，但整文件 A/B 不稳定（帧反而变大，
  与 stock「压不小就回退 0x46」的决策相反），已保留失败证据，不冒充通过。
- `ServeConfig.compressed_parts`（默认 false）与 daemon/CLI `--compressed-parts` 为
  opt-in。重复 `REQUESTPARTS` 现在立即重发该区间（stock 行为）；旧的「3 轮 + 2 s」
  抑制会让 aMule 反复请求同一批块并饿死。
- 同一 >PARTSIZE 样本、同一下载端，paced 200 kB/s 与 unpaced 两轮 A/B：
  A（我们 daemon + `--compressed-parts`）落地 md5 一致，B（第二 stock amuled）也一致；
  9,736,192 B 文件只走 408,507 B 压缩载荷。诊断脚本改用「对端落地 md5」判 PASS，
  不再把 `Show DL` 清空误报为停滞。全量回归 **504 passed / 0 failed / 3 ignored**。

## [2.4.113] - 2026-11-24 — FR-25 第二上传端反向 A/B：定位到帧形态差异

> 门禁常量不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。**FR-25 上传方向仍 BLOCKED**。

- 按用户明确确认执行「第二 stock aMule 上传端反向 A/B」：221 上临时启动第二台
  amuled（TCP 4664 / EC 4713，KAD/server 关闭），同一 >PARTSIZE 样本、同一下载端。
  A（我们 daemon，paced 200 kB/s）停在 97.6%、`STALLED_WITH_PENDING_BLOCKS`；
  B（第二 stock amuled）37.5 s 落地 md5 完全一致。
- 唯一新样本的帧级抓包显示：stock 上传端用 `OP_COMPRESSEDPART(0x40)` /
  protocol `0xC5`（约 1,070 B zlib 帧对应 184,320 B 块），下载端持续 3 块滑动窗口请求；
  我们发 `OP_SENDINGPART(0x46)` / `0xE3`、10,240 B 未压缩帧。
  这推翻早期「blocker 纯在对端消费、不是帧格式」的过宽推断。
- 更正 `cr-ed2k::stack` 旧注释；审计新增 §17.1；所有 A/B 与抓包测试已按
  `tests/ARCHIVE/` 规范归档。下一步：实现同构 `OP_COMPRESSEDPART` 编码并重跑 A/B。

## [2.4.112] - 2026-11-24 — 建立测试归档账本；更正 FR-10 / FR-24 陈旧口径

> 门禁常量不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。

- 新增 `tests/ARCHIVE/`：每次真实测试必须登记命令、判据、结果、证据路径 + sha256
  与门禁影响；`scripts/archive_test_run.py` 负责追加并脱敏，
  `scripts/build_test_archive_backfill.py` 从 gitignored 工作日志生成历史补档，
  `NOT-TESTED.md` 区分「测过但证据缺失」与「未测」。
- 已归档此前只在本机的 FR-25 报告/帧日志、w51g 干净 A/B 基线、P1/P0 重跑、
  全量 Rust 499/0/3 原始输出；家宽私有回传只留哈希清单，不入库。
  FR-15 被覆盖快照与缺失 tcpdump pcap 明确记为 MISSING，不补造。
- `tests/e2e/p1/run_p1.py` 加 `--report-out`，并修复 `free_port()`：同时验证 TCP/UDP
  绑定，避开 Windows 排除端口导致的 WSAEACCES 10013；P1 重跑 5/5 PASS。
- P0 重跑 77/77：FR-10 守护进程多样种 + cap 逐出已由 phase A 真实 BT 握手覆盖
  （wake1 `evictions=0` → wake2 `evictions=1`，`seed_active<=cap=1`），
  审计 §2/M2/§7 与 TEST §2.14 中「daemon 仍单任务 / 公网 BLOCKED」的历史口径已更正。
- 归档提交 `8d30d05` 已推送；本次文档更正另提交。

## [2.4.111] - 2026-11-24 — FR-24 在非 firewalled 的 209 eMule 上完成真实 publish→search 闭环

> 门禁常量不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。无源码改动。

- 用户提供 209（`192.168.1.209`）上非 firewalled 的 eMule：KAD UDP 41296、
  `KadUDPKey=1906084797`、KadID `dddb80f8…4760b3`（HELLO_RES 报版本 10），
  `key_index.dat` 204,629 B 非空，公网出口同 `112.5.60.4`。
- 221→209 局域网 try-2 HELLO 验证成功（`receiver_key_verified=true`），贴键
  `top_chunk=0`；随后 `--publish-file` 得到真实 `0x4B` ACK（`ack_count=1`），
  同一 `0x33` 取回 **1 条 `0x3B` SEARCH_RES**：file_id/md5、文件名、
  1024 B 大小与 ed2k link 全部匹配（`public-209-lan-loop-20261124.json`）。
- 222→`112.5.60.4:41296` 真公网路径：HELLO 因 222 对称 NAT 的可见端口不确定而超时，
  但 **publish ACK 仍为真、SEARCH_RES 仍为 1 条且 tag 可解析**
  （`public-209-public-loop-verified-20261124.json`）→ 再次证明索引操作没有
  「已验证接触」门槛；无 HELLO 的对照则 0 ACK/0 命中
  （`public-209-public-nohello-20261124.json`），差异来自信封/握手而非目标状态。
- 证据另含 `public-209-target-profile-20261124.json`（不含凭据）。FR-24 的技术闭环
  已在真 eMule 上复现；`evidence_valid_for_pass` 仍恒 false，不外推为门禁通过。

## [2.4.110] - 2026-11-23 — FR-24 防火墙状态恢复尝试：重启无效，入口路径正常

> 门禁常量不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。无源码改动；全量测试仍 499/0/3。

- 按用户确认执行下一步：备份 KAD 状态后 `systemctl restart cr-amuled3`，
  221 aMule 在 ~15 s 内重新声明 `Kad: Connected (firewalled)`，60 s 观测窗口内未变。
  `amulecmd` 没有强制 UDP 防火墙重测的命令（`set` 仅支持 BwLimit/IPFilter）。
- 两处独立公网入口验证：222 与公网中继 `82.157.68.159` 分别向
  `112.5.60.4:4672` 发 2 字节 `KAD2_BOOTSTRAP_REQ`，分别收到 521 B / 523 B
  `BOOTSTRAP_RES` → 入站 UDP 路径正常。
- 重启期间抓包 95 个 UDP 包：aMule 与大量公网 KAD 节点正常收发 51 B 请求及
  85/159/310 B 应答 → firewalled 是 aMule 自身 UDP 防火墙测试结果，不是 KAD 收发故障。
- 中继主机只有 UDP 7100 入站，任意端口无回包，不能当无 NAT 的公网探针主机。
- 证据：`tests/e2e/w24-pub/reports/firewall-remediation-attempt-20261123.json`。
  结论不变：闭环验证仍需非 firewalled 且本地索引含关键词的目标，别换信封/加发包。

## [2.4.109] - 2026-11-23 — FR-24 静默根因再修正：目标临时 UDP-firewalled，非接触接纳

> 门禁常量不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。
> 验证：`cargo test --workspace --offline` **499 passed / 0 failed / 3 ignored**。

- **上一版结论被推翻**：v2.4.108 把 `0x33`/`0x43` 静默归因为「aMule 索引接纳/
  节点状态」。对照 aMule 2.3.3 源码后确认此归因错误：
  - `Process2SearchKeyRequest`（0x33）**无责任区校验**，直接调
    `SendValidKeywordResult`；后者**只在该节点本地 `m_Keyword_map` 命中关键词时**
    才回 `0x3B`。我们用 `--key-hex` 贴的合成 key 在任何人索引里都不存在，静默是正确行为。
  - `Process2PublishKeyRequest`（0x43）在**接收方** `IsFirewalledUDP(true)` 时
    `return`（不给 publisher 假成功），且只有它的 `SEARCHTOLERANCE` 责任区门。
  - `InTrackListIsAllowedPacket(ip, opcode, /*bValidSenderkey*/)` **忽略** verify key，
    只做每 IP 每 opcode 的洪泛限速（0x33/0x43 各 3/min）；请求处理无「接触已验证」门。
- **干净 A/B（同一目标、同一公网路径、不同日期）**：
  - 2026-11-17 `tests/e2e/w51g/reports/run-20261117T085411Z.json` 对
    `112.5.60.4:4672` 有完整 7 包闭环：BOOTSTRAP → HELLO → PUBLISH_KEY_REQ →
    **PUBLISH_RES(19 B)** → SEARCH_KEY_REQ(20 B) → **SEARCH_RES(591 B)**，`publish_acked=true`。
  - 今天 `amulecmd -c status` 报 `Kad: Connected (firewalled)`，`key_index.dat` 仅 28 B（空）。
    同一 `kad-mini wan-index-probe` 重跑得到 `wan_publish_silent_in_zone` /
    `publish_acked=false`（`reports/public-firewalled-recheck-20261123.json`）。
  - 结论：**目标临时防火墙状态变化**单独解释全部静默，不是信封/NAT/责任区/接触接纳。
- **CLI 诊断旗标 `--search-closest`**：默认 0x33 仍只发 2^24 区内（旧口径）；该旗标在
  无在区节点时改取最近的已验证节点（≤3、仍 0.2 pps、`evidence_valid_for_pass=false`）。
  实测把真实词 `ubuntu` 的 try-0 0x33 发给 3 个公网节点，仍 0 命中
  （`reports/public-real-keyword-closest-20261123.json`）。
- **FR-24 口径**：真实 `SEARCH_RES` 闭环由 w51g 历史报告证明可达；今日公网 0 命中
  是**被测目标状态**而非协议实现缺陷。不要再换信封/加发包；要复现需先把 221 的 aMule
  恢复为非 firewalled，再跑同一 probe。

## [2.4.108] - 2026-11-23 — FR-24 跨公网 try-2 HELLO 验证；索引操作仍 BLOCKED

> 门禁常量不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。
> 验证：`cargo test --workspace --offline` **499 passed / 0 failed / 3 ignored**（+2 单测）。

- **CLI 新开关（全部显式 opt-in）**：`--key-hex <32HEX>` 固定责任区内索引键
  （W5.1g 贴键口径，报告 `key_source=hex_override`，不再冒称 MD4(term)）；
  `--stun <ip:port>` 在**同一条 KAD socket** 上取 `XOR-MAPPED-ADDRESS`；
  `--our-public-port` / `--bind-port` 显式给对端可见端口；
  `--index-envelope auto|try2|try0|plain` 只做信封诊断对照，不提高判定。
  复用 `cr-nat::stun`（std-only），新增 `coldrevive-ed2k` 2 条单测。
- **真机路径（222 → 221，两个不同公网出口）**：`tcpdump` 双侧证实 222 的
  UDP 从 `112.5.32.34` 进入 221 的 `112.5.60.4:4672`，`BOOTSTRAP_RES` 真实往返；
  旧明文索引探针 `wan_publish_silent_in_zone`（0 ACK / 0 命中）。
- **抓到两个真缺陷**：① HELLO 载荷的 `sourceUpPort` 填的是本地端口，NAT 后
  aMule 的 `HELLO_RES` 回向错误端口（实测本地 46057 → 外部 2078，却回 46057）；
  ② 222 是对称 NAT，STUN 对 STUN 服务器给的端口（2055/2056）不等于 KAD 目标
  看到的端口（2080）。固定本地口 + 对端可见口后，
  **try-2 HELLO 跨公网真成功**：`receiver_verify_key=141047448`、`rtt_ms=868`、
  `obfuscated_response=true`、`receiver_key_verified=true`
  （`reports/public-try2-hello-verified-20261123.json`）。
- **仍未闭环**：`0x33`/`0x43` 在 try-2、try-0+rvk、明文三种信封下均静默
  （`top_chunk=0` 已排除责任区越界），单 key 最小发布也拿不到 `0x4B` ACK，
  `answer_count=0`。剩余 blocker 位于 aMule 的索引接纳/节点状态，
  **不是 RC4 信封、NAT 端口或责任区**；别再靠换信封或加大发包重试。
  FR-24 公网真实 `SEARCH_RES` 保持 BLOCKED，不外推 LAN 结果。

## [2.4.107] - 2026-11-23 — FR-24 KAD v8 握手接线、混淆字节序修正与 FR-25 A/B

> 门禁常量不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。

- **FR-24 握手**：`coldrevive-ed2k search` 新增 `--hello` / `--hello-plain` /
  `--hello-try2` / `--peer-udp-key` / `--our-public-ip`，实现 `udp_verify_key`
  与 HELLO_RES_ACK 回包；`receiver_key_verified` 只在 try-2 真换回 `HELLO_RES`
  时置真，不因参数齐全就自证通过。
- **混淆层字节序**：源码核对发现 magic / verify-key 应为 LE 字节，且 try-0 的
  RC4 base 收发两端不对称（对端收包用 `StoreCryptValue` 逐字反序，发包用原始 ID）。
  旧实现自洽但真机静默；已拆成 `send_key_base` / `receive_key_base` 并补
  aMule 收包语义出站回归、真 aMule HELLO_RES 解密向量。
- **真机结果**：221 本地明文与 **try-2 混淆 HELLO 都成功**（`rtt_ms=31`、
  `receiver_verify_key=3224252923`、`obfuscated_response=true`），首次让真 aMule
  接受我们的混淆握手；try-0 在洪泛窗口外仍无应答。公网 `0x33` 真实命中仍 BLOCKED
  （公网节点缺 KadUDPKey / 可见 IP 输入）。
- **FR-25 A/B**：同链路唯一新样本两轮对照，unpaced 12.7% / Recv-Q 4 MB /
  390 个无主重传，paced（`FR25_PACE_BPS=200000`）36.6% / Recv-Q 133 KB /
  0 无主重传；两轮均 `STALLED_WITH_PENDING_BLOCKS`，对端落地 md5 闭环未取得。
- 验证：`cargo test --workspace --offline` **497 passed / 0 failed / 3 ignored**；
  P0 e2e **77/77**；P1 e2e **5/5**。审计 §16/§17 有完整证据边界。

## [2.4.106] - 2026-11-23 — FR-24 真机判别：根因定位为 KAD UDP 混淆

- 真机对照（221，出口 `112.5.60.4`）：同一台 firewalled 真 aMule KAD 搜 `ubuntu`
  命中 **361**，我们的明文工具同词收敛到在区节点却 `answer_count=0` → 公网 KAD
  可用，缺口在我们只发明文帧（此前「在区节点入站不可达」归因被证伪）。
- `tcpdump` 抓包证实 aMule `0x33/0x3B` 为 RC4 混淆帧（`CryptoKadUDPKey`），
  BOOTSTRAP_RES 是明文 `0xE5` zlib；STUN 判定 cone NAT，入向不是 blocker。
- 提升 S2 混淆原语为 `cr-kad::kad_obfuscate`（25/25 单测），`coldrevive-ed2k search`
  对已知 KadID 的 `0x33/0x43` 启用 try-0 封装并用本地 KadID 自解验证字节正确。
- **仍未闭环**：混淆帧对真 aMule（本地+公网）仍无应答；S2 历史 `--obfuscate`
  探针也从未成功（`wan_publish_silent_in_zone`），说明还缺 HELLO/verify-key 握手语义。
  FR-24 公网真实命中保持 BLOCKED，`evidence_valid_for_pass` 恒 false。
- 全量 `cargo test --workspace --offline` 486 passed / 0 failed / 3 ignored（门禁常量不变）。

# 变更记录（Changelog）

本文件记录**文档体系与项目决策**的变更。格式参考 Keep a Changelog，版本与文档 front matter 中的 `version` 对应。

## [2.4.105] - 2026-11-23 — FR-24 公网 0 命中的根因定位与生产工具修正

> 门禁常量仍不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。

- **根因**：此前 `search --public` 先收到 `BOOTSTRAP_RES`，再把 `0x33` 直接发给
  bootstrap 节点。S2 `PLAN.md` §6/§20 已证：aMule 的索引操作按
  `SEARCHTOLERANCE = 2^24` 校验「节点 KadID ↔ Key」距离，越界**静默丢弃**；
  它还会对 >200 B 的应答发 `0xE5` zlib 帧，旧工具只认 `0xE4`。
  所以「0 命中」不是 HELLO/RC4，也不是网关/换网问题。
- **修正**：`cr-kad::kad_ed2k` 提升 S2 的零依赖 zlib 解码器，新增联系人解码、
  `0x21/0x29` 节点发现、责任区判定和字节级 `0x43` 发布编码（对 aMule 已接受的
  真实请求帧逐字节回归）；`coldrevive-ed2k search --public` 先迭代收敛，
  只对「回显 target 且在责任区」的节点发 `0x33`，`0xE4/0xE5` 都解；
  `--publish-file` 支持 A 轨最小样本发布（一个唯一 key，最多 3 个在区节点，0.2 pps）。
  `evidence_valid_for_pass` 修正为**恒 false**，程序不再自证通过。
- **实测**：`public-lookup-20261123-ubuntu.json` — 36 次 `0x21`、33 次 `0x29`、
  33 个已验证节点、1 个在区（`top_chunk=8,875,008 < 2^24`）；该在区节点对
  `0x33` 仍静默，符合 S2 §20.5「能问路的在区节点也可能 UDP 入站不可达」。
  更宽的 16 跳/alpha=5 轮次（`public-roundtrip-20261123-wide.json`：80 发 `0x21` /
  72 收 `0x29` / 仍只 1 个在区节点）与新鲜联系人轮次
  （`public-roundtrip-20261123-fresh.json`：2 跳找到 2 个在区节点）都对
  `0x43`/`0x33` 静默，`ack_count=0`、`answer_count=0`。
  命中只认真实 `SEARCH_RES`，0 命中如实记 0；剩余边界更像是本机公网 UDP 入向不可达
  或对端索引策略，需要换具备真实入向能力的主机复核。
- **FR-25 定位补充**：`diag_upload.py` 从 7 份透明代理报告重放 aMule 块记账，
  `dropped_exceeds/dropped_unknown` 恒 0；对端 Recv-Q 停住但状态仍 `Downloading`。
  新增 `FR25_PACE_BPS` 仅用于诊断性限速对照（不进生产），用来区分
  「帧格式错」与「对端接收缓冲/消费速率问题」。
- 验证：全量 `cargo test --workspace --offline` **486 passed / 0 failed / 3 ignored**；
  P0 e2e **77/77**；P1 e2e **5/5**；`cr-kad` **22/22**；`coldrevive-ed2k` **3/3**。

## [2.4.104] - 2026-11-22 — P1/P2 接线与 FR-25 有界在途（全部 opt-in）

> 门禁常量仍不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。
> 验证：全量 `cargo test --workspace --offline` **480 passed / 0 failed / 3 ignored**；
> P0 e2e **77/77**；新增 P1 e2e **5/5**；`cr-ed2k` 单独 **53/53**。

- **P1 API/运行时（`coldrevived` / `cr-api`）**：
  - `api.rpc_secret`：写方法要 token/header，**`/health` 永不回显密钥**；
  - `api.web_ui` / `api.websocket`：自包含静态 UI + 真 RFC6455 状态帧，默认关闭、
    排在既有端点之后不遮蔽 `/health`；
  - `GET /openapi.json`：真实 REST 路径 + JSON-RPC 方法（含 `mutating` 标记）；
  - `qos.background_upload_ratio` 通过 librqbit 真实 `upload_bps` 落实硬上限，
    比率按 `reference_link_bps` 名义换算（`cr-qos` 只记账），**不宣称 LEDBAT**；
  - `nat.auto_map`：启动时真调 UPnP/NAT-PMP 并把成功/失败与外部地址写进 `/health.nat`，
    **路由器应答不是入向可达证据**；
  - `dht.enabled`：活跃种子真跑 `get_peers → announce_peer → 回查`，
    公共节点无 token 时如实记 `token_unavailable`，不宣称公网可发现；
  - 跨协议补源/信誉仍是 `not_implemented` 状态接口，不伪造来源或分数。
- **FR-25 上传侧**：`cr-ed2k::stack` 增加 `ServeConfig` / `serve_with_config`，
  以「请求回合」为进度信号给在途字节设硬上界（默认 3×`BLOCKSIZE`），
  半区间进度保留在 `deferred_bytes`；修 Windows `WouldBlock` 误判导致 idle 看门狗不跑；
  新增 `codecs.rs` 对 `0x44/0x34/0x4B/0x3B` 做真编解码，server login/credit/AICH
  明确 `Unsupported` 而不编造帧。`coldrevive-ed2k serve` 与
  `/cli ed2k-serve` 暴露 `--max-in-flight-bytes` / `--in-flight-unlimited` /
  `--request-gated-pacing`（后者是**未验证假说**），输出 `idle_resends` /
  `max_in_flight_bytes` / `deferred_bytes` 供真实 e2e 判定。
- **FR-24**：见 [2.4.103]；公网实测 `BOOTSTRAP_RES` 3/3、`SEARCH_RES` 0 命中，
  如实记 0，不外推 LAN 结果。
- **配置文档**：`docs/04-protocol/CONFIG.md` §2.2 列出全部新增 opt-in 字段与边界；
  SRS v2.1 完成评审与重复 FR-24 → FR-26 编号去重。

## [2.4.103] - 2026-11-22 — 项目所有者书面解除 G0 `protocol_scope`

> 决策条目，不含代码/门禁变更。用户在本会话明确指令「解除」，并按「全部剩余欠账」继续推进。
> 机读：`tests/spikes/spikes.json` → `g0_review.protocol_scope_relaxation_2026_11_22`。

- **放宽范围**：公共 KAD `0x33`/`0x43` 索引操作不再因 G0 scope 登记 BLOCKED；
  允许对真实公共节点做 `SEARCH_KEY_REQ`/`PUBLISH_KEY_REQ` 并读取响应，
  允许查询公共 eMule 关键词词典。v2.4.22 已证伪「必须先 HELLO」，所以本决定
  **不要求**实现 `HELLO`/UDP key/RC4，也不改帧格式。
- **保留约束**：0.2 pps 限速、最小样本、禁止批量发布测试 Key、真实 fileID、
  R-02/R-11 的 M4 复审、所有结论只来自真实观测且脚本不得自动 pass。
- **门禁常量不变**：`g2_claim=false`、`s6_status=not_run`、
  `evidence_valid_for_pass=false`、`auto_pass=false`。
  FR-24 公网检索仍须生产实现 + 真实公网证据重新登记，LAN 结果不得外推。
- 文档同步：`docs/06-plan/G0-REVIEW.md` §12、`OPEN-QUESTIONS.md` §3、
  `STAGE-DEBT-AUDIT-2026-11-21.md` §12。
- **FR-24 工具侧配套**：`coldrevive-ed2k search` 新增 `--rate-limit-pps`
  （`--public` 时缺省 0.2，0=不限速），bootstrap 与 0x33 发包前统一节流；
  输出新增 `rate_limit_pps` / `rate_limited`。daemon 的 `public_search` 文案由
  `BLOCKED` 改为 `AUTHORIZED`，仍明确本命令只做本地索引，不伪造公网命中。
  P0 e2e 两条断言同步改为「已授权且未宣称命中」。
- **首轮公网实测（rate=0.2 pps，3 个新鲜公共联系人）**：
  `ubuntu` / `mp3` 两轮均 `BOOTSTRAP_RES` 3/3，但 `SEARCH_RES` 0 命中；
  记录为 `tests/e2e/w24-pub/reports/public-search-20261122-{ubuntu,mp3}.json`。
  按证据口径这**不是** FR-24 公网检索通过，只证明公网 KAD UDP 出入程可达。

## [2.4.102] - 2026-11-22 — FR-15 五条阈值齐活；FR-07 同义词与语言识别保持可复算

> 补上此前唯一没接的造种 CPU 阈值，并给 KAD 关键词加**确定性**同义词扩展与
> Unicode 脚本语言识别。门禁常量仍不变：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。
> `cargo test --workspace --offline` **426 passed / 0 failed / 3 ignored**（v2.4.101 为 419）；
> P0 运行期 e2e **77/77**（v2.4.101 为 62）。

- **FR-15 第五条阈值：`limits.max_seeding_cpu_millipct`**。单位是千分百分点
  （1000 = 单核 1%），0/缺省 = 关闭；Linux 读 `/proc/self/stat` 的 `utime+stime`
  并用 `sysconf(_SC_CLK_TCK)` 换算，Windows 读 `GetProcessTimes`，其它平台如实报
  `unavailable`。与其余四条共用准入：**超限拒绝新唤醒**
  （`REJECT_RESOURCE_LIMIT` + `limit_name/limit/current/limit_unit`），
  拒绝顺序固定为 hot → peers → bandwidth → rss → cpu。
  - 这是**整个守护进程**的 CPU（含 HTTP/CLI），多线程求和可超过 100%；
    它是**滞后指标**，只能当拒绝理由，不承诺「做种永不超线」。
  - 读不到 CPU 时**不当作 0**，该条不参与判定；`/health` 同时给出原始
    milli-percent 与人类可读的 `current_percent`。
  - e2e F 段用 `1` 毫百分点（0.001%）触发：实测 `current=1600`（1.6%），
    真实唤醒被拒，`hot` 与 `seed_active` 都保持 0。
- **FR-07 同义词与语言识别（保持可复算）**：
  - `LATIN_SYNONYM_GROUPS` / `CJK_SYNONYM_GROUPS` 是**手写封闭表**，
    不用通用词库/模型/随机数；同名字在任何机器、任何版本都得到同一组词。
  - `expand_keywords` 按**每个词自己的 Unicode 脚本**选表：原文全部保留且排最前，
    同义词追加在后，去重保序，硬截 8 个。不认识的词原样保留——
    「没扩展」比「扩展错」好。
  - `detect_language` 只做展示/审计：多种脚本报 `mixed` 而不是硬挑一个「主要语言」。
    纯汉字（无假名）判 `zh`；这点在文档里写明，不假装能区分中文/日文汉字。
  - 扩展**只作用于发布出去的 KAD key**，`FileEntry.keywords` 仍保留原文，
    所以能分清「文件名里真有的词」和「我们补的词」。
  - `/cli index` 新增 `language` / `keywords_expanded` / `synonym_added`；
    `/cli kad-health` 新增 `languages` / `synonym_added`。
  - `.bin/.dat/.txt` 等通用扩展名加入 `NOISE_TOKENS`：它们不是内容词，
    还会让「中文标题 + .bin」被误判成 `mixed`。
- **新增 `/cli publish <路径>`**：此前 `/cli index` 只写 key 却从不标记已发布，
  `inspect kad` 看不到新入库条目。现在 publish 真发 `KAD_PUBLISH`，
  **只有收到 `KAD_PUBLISH_ACK` 才**置 `kad_published` 并前推 TTL。
  这个 ACK 只表示对端节点收下 key，不表示公网 KAD 可检索。
- **e2e 新增 F/G 两段**：F = CPU 真实读数与拒绝（含未配置阈值显式 `enabled=false`）；
  G = 拉丁/中文同义词、语言判定、重索引逐项一致、`cinema` 的 key 真出现在
  `inspect kad` 的已发布 key 里，且 `public_search` 仍标 BLOCKED。
  C 段另加断言：未配置的 CPU 阈值也报 `enabled=false` 而不是字段缺失。

## [2.4.101] - 2026-11-22 — FR-15 四条阈值齐活；FR-05/07 TTL+分词+去重；FR-25 上传侧定位到对端

> 三条并进：① 把 FR-15 剩下的 peers / 带宽 / RSS 阈值接上真实读数；② 把 FR-05/07 的
> TTL 维护、文件名分词、关键词去重健康度补齐；③ 用帧级代理把 FR-25 上传方向的停顿
> 定位到**对端停止读取**，并据此修了两处真实的守护进程缺陷。
> 门禁常量不变：`g2_claim=false`、`s6_status=not_run`、`evidence_valid_for_pass=false`、
> `auto_pass=false`。全量 `cargo test --workspace --offline` **419 passed / 0 failed / 3 ignored**
> （v2.4.100 为 406）；P0 运行期 e2e **62/62**（原 48）。

- **FR-15 补齐三条阈值**：新增 `src/bins/coldrevived/src/sysres.rs`。
  `limits.max_peers`（取 `EngineProgress.peers` 实测和）、
  `limits.max_upload_bytes_per_sec`（累计 `uploaded_bytes` 的 1s 差分）、
  `limits.max_rss_mb`（Linux `/proc/self/status` 的 `VmRSS`；Windows
  `GetProcessMemoryInfo`）。三条与 `max_hot_seeds` 共用一套准入：超限
  **拒绝新唤醒**，`REJECT_RESOURCE_LIMIT` 带 `limit_name/limit/current/limit_unit`。
  带宽是**滞后**指标，只能作为拒绝理由，不能承诺「不超过这条线」；
  读不到 RSS 的平台**不当作 0**，`/health.limits.max_rss_mb` 显式
  `enabled=false` + `unavailable`，宁可承认没生效。
- **FR-05 TTL 维护**：`FileEntry` 新增 `published_at` / `kad_expires_at` /
  `kad_refresh_failures`（追加可选字段，不递增 `schema_version`）。
  重发布从「每周期无脑发」改为**到期才发**：刷新窗口在 `ttl/3` 处打开
  （`kad.ttl_secs`，缺省 3600）。成功前推截止时间并清零失败计数，
  **失败不前推**——否则连续失败会把自己无限延期。这是本机记账，不是协议 TTL：
  对端只回 `KAD_PUBLISH_ACK`，不带过期时间。
- **FR-07 关键词分词**：`cr_kad::tokenize_keywords` 从文件名切词（确定性变换）：
  非字母数字处分词、ASCII 小写、去纯数字与 <3 字符 ASCII 词、去容器/编码噪声词
  （`NOISE_TOKENS`）、去重保序、上限 8。CJK 连续串整体保留（按字切会把标题切碎）。
  `/cli index` 在配置没给 `keywords` 时才分词，并回报 `keywords_source=tokenized|config`。
- **FR-05/07 去重健康度**：新增 `/cli kad-health`：`keys_total` / `keys_unique` /
  `duplicate_keys`（逐 key 列出被几条条目发出）、`keywords_total|unique`、
  `tokenized_entries`、`due` / `expired` / `expiring_soon`、`refresh_failures`。
  `keys_unique + duplicate_keys == keys_total` 可当自检。
- **FR-25 上传方向：定位到对端**。新增 `tests/e2e/w25-daemon/diag_upload.py`：
  透明 TCP 代理记录双向每一帧，并在旁路**重放 aMule 自己的块记账**
  （`CUpDownClient::ProcessBlockPacket` 的边界规则）。结论：
  - 所有 SENDINGPART 都落在 pending 块内，**0 帧越界、0 帧无主**；
  - aMule 在 6%–38% 之间**停止读取自己的 socket**（内核 Recv-Q 里留 ~493 KB 不动，
    客户端状态仍是 `Downloading`），约 100 s 后 `DOWNLOADTIMEOUT` 把它打回 `Waiting`；
  - 它停读时我们正好阻塞在 `send_frame` 的写里，**连 idle 重传的看门狗都轮不到执行**。
  - aMule 侧无法进一步取证：`AddDebugLogLineN` 在 release 构建里被 `#define` 成
    `do {} while(false)`，`VerboseDebug=1` 不会产生任何帧级日志
    （`strings /usr/bin/amuled | grep -c "exceeds requested"` = 0）。故不重建 debug 版，
    因为那会换掉被测客户端。
- **两处真实守护进程修复**（都由上面的量具暴露）：
  1. `serve` 的写操作加 10 s 上限：对端不读时不再把 serve 线程连同看门狗一起钉死，
     而是如实报 `write eD2k frame (peer stopped reading?)` 并放掉连接，让客户端重连；
  2. `serve` 加 idle 重传看门狗：对端静默 ≥3 s 就把「它最后请求过的那批区间」再发一次，
     总量封顶 2× 文件大小。实测把同一条链路的进度从 10.1% 推到 38.6%
     （`bytes_sent` 1.48 MB → 4.25 MB、完成块 9 → 24）。eD2k 没有块级 ACK，
     这是唯一能自愈的手段；仍**不足以**闭环，见下。
- **FR-25 上传仍 BLOCKED**：上表第 2 条是对端行为，我们无法从服务端纠正；
  重传只能缩短停顿、不能阻止对端再次停读。下载方向仍是 10/10 PASS，判据不变
  （只看对端落地 md5）。
- `tests/e2e/p0/` 新增两段：D 段（分词 + 去重健康度，`keywords: []` 配置）、
  E 段（`limits.max_rss_mb=1` 下真实唤醒被拒且不留 HOT 痕迹）。

## [2.4.100] - 2026-11-22 — P0 运行期欠账批量落地（FR-01/02/04/05/06/09/10/12/13/15）

> 本轮把「只有组件、没有运行期行为」的 P0 项接进守护进程，并用一条可复跑的本机回环
> e2e（`tests/e2e/p0/`，**48/48**）取证。门禁常量不变：`g2_claim=false`、
> `s6_status=not_run`、`evidence_valid_for_pass=false`、`auto_pass=false`。
> 全量 `CARGO_INCREMENTAL=0 cargo test --workspace --offline` **406 passed / 0 failed / 3 ignored**
> （v2.4.99 为 392 passed；新增 14 个单测 + 1 个 warning 修正）。

- **FR-01 目录增量**：新增 `src/bins/coldrevived/src/watch.rs`，按 `indexer.watch_secs`
  用 `cr_indexer::PollWatcher` 比对共享目录快照，新增/修改入库、删除出库。
  语义与内核通知一致，延迟 ≤ 周期；Windows `ReadDirectoryChangesW` **未接入**，
  不宣称已用内核通知。
- **FR-02 多哈希**：`cr_indexer::FileFacts` 一次读盘同时给 SHA-256 / SHA-1 / eD2k fileID，
  索引条目落盘三者（`schema_version=2`）；`cr_hash` 新增 `unhex16` 供链接侧校验。
- **FR-04 持久化**：`cr-store` 重写为**原子落盘**（tmp → fsync → 旧文件转 `.bak` → rename），
  新增 `OpenStatus`（`fresh` / `loaded_v1` / `loaded_v2` / `recovered_from_backup` / `corrupt`）
  与 v1→v2 就地迁移。恢复自备份**不得**被读成正常加载，corrupt 也不隐藏。
- **FR-05/06 周期重发布**：新增 `kad_republish.rs`，按 `kad.republish_secs` 重发
  `kad_published=true` 条目的已有 key，`/health.kad_publish` 报 `cycles` 与最近一次真实结果。
  `sent=true` 现在**要求对端回 `t=KAD_PUBLISH_ACK`**（此前收到任意一帧就算成功），
  并在 `ack_t` 里留证。不新增关键词、不改 TTL、不宣称公网可检索（FR-24 仍 BLOCKED）。
- **FR-09 按 IP 限流**：新增 `ratelimit.rs`（令牌桶，IP 表上限 4096），`api.rate_per_sec` /
  `api.rate_burst` 控制 `/cli`、`/jsonrpc`、`/inject`；被拒返回 **429** +
  `reason=rate_limited` + `retry_after_ms` + `limit_per_sec`，不伪装成 500/超时。
  `/health` 与 `/nat/*` 是探活/诊断面，**不参与限流**（否则「机器忙」会被读成「服务挂了」）。
- **FR-10 多任务做种可观测**：`/health` 增 `seeding{cap,scheduled,evictions}`，
  `seed_active` 与上限同屏，能区分「没触发」与「触发了但逐出没生效」。
- **FR-12 链接来源**：`/cli ed2k` 优先用索引里已存的 fileID（`link_source=index`），
  只有 v1 条目才读盘并把结果**回填**索引（`backfilled_from_disk` 计数）。
- **FR-13 完成回流**：下载完成的产物自动写入影子索引（含 SHA-1 / eD2k fileID / infohash），
  不再需要人工 `index`。
- **FR-15 资源阈值（部分）**：新增 `limits.max_hot_seeds`。与 `engine.max_active_seeds`
  的区别是**冲突时的行为**：容量满逐出最久未用者，阈值满**拒绝新唤醒**。
  拒绝返回 `REJECT_RESOURCE_LIMIT` 并带 `limit_name/limit/current`；
  **裸 BT 握手（被动唤醒）受同一条阈值约束**，否则绕过 `bt_port` 就能顶满。
  内存/带宽/造种 CPU 阈值**仍未接线**，FR-15 整体仍记为「部分」。
- **e2e（`tests/e2e/p0/`，48/48）**：A 段功能（FR-01/02/05/06/10/12），B 段重启+限流
  （FR-04/09），C 段阈值拒绝（FR-15）。eD2k fileID 由 `tests/e2e/w24/verify_ed2k.py`
  的纯 Python MD4 **独立复算**，不是把守护进程自己算的数读回来。
- **e2e 抓到的真实缺陷（已修）**：
  1. 往共享目录丢一个文件（只登记、无伴随种子）会让**下次重启起不来**
     （`fs::read("")` → os error 3）。现在跳过并打印，不静默、不拖垮节点。
  2. 同一文件相对/绝对路径入库时身份不一致，删除事件匹配不上，条目永久残留。
     现在入库 `canonicalize`、出库按「规范化父目录 + 文件名」比较。
  3. `/cli inspect index` 在新建节点上报 `schema_version=0`（像「格式未知」）；
     `fresh` 现在报当前 schema，`status` 仍保留「没有旧文件」这一事实。
- 新增 CLI：`/cli inspect entries`（逐条列出索引）、`/cli wake raw`（原样返回对端应答，
  用于分辨 `REJECT_*` 的具体码）、`/cli index|unindex`。

## [2.4.99] - 2026-11-22 — FR-25 接进守护进程；跨分片仍未闭环

> 本轮补上 FR-25 的 daemon 接线并修掉真实 aMule 暴露的协议/状态机缺口，
> 但 **`>PARTSIZE` 双向跨分片没有拿到完整 PASS 证据**，不升级门禁：
> `g2_claim=false`、`s6_status=not_run`、`evidence_valid_for_pass=false`、`auto_pass=false`。
> 全量 `CARGO_INCREMENTAL=0 cargo test --workspace --offline` **392 passed / 0 failed / 3 ignored**。

- **daemon 接线完成**：`coldrevived` 新增 `ed2k_tasks.rs`，`/cli` 增加
  `ed2k-link` / `ed2k-serve` / `ed2k-download` / `ed2k-status` / `ed2k-list`，
  `/jsonrpc` 增加只读 `coldrevive.ed2kStatus`。三条命令都调用生产
  `cr_ed2k::stack`；任务结果只在栈真正返回时写入，失败如实记 `error`。
  `ed2k-download` 仍需本地参考文件导出真实 fileID，未实现“无参考盲下载”。
- **真实 aMule 暴露并修复的缺口**：
  - `>= PARTSIZE` 的文件选择走 `OP_SETREQFILEID(0x4F)`，服务端此前只认
    `OP_REQUESTFILENAME`，对端停在握手后；现补 `OP_FILESTATUS(0x50)` 应答。
  - 大文件会同时开多条 TCP 连接；`serve_cmd` 改为每连接独立线程、共享
    `UploadQueue` 与聚合计数，收尾主动 `shutdown` 排队连接，避免拖到整体超时；
    新增 `serve_accepts_parallel_connections_and_reaps_queued_peers` 回归测试。
  - **读超时半帧错位**：`read_exact` 在超时前消费半帧却丢字节，真 aMule
    `>PARTSIZE` 下载报 `invalid eD2k packet length 2655250573`。新增
    `wire::read_packet_pending`，`Link` 保留 `pending` 缓冲，超时后接着读。
  - aMule 每收一块就把“仍 pending 的区间 + 一个新区间”重发，3 块会被放大成
    9 块；上传侧按 `(start,end)` 抑制重复并做 10ms 轻节流。下载侧在长时间
    无进展时重发 `in_flight` 区间，避免把部分应答误判为完成。
  - **上传帧粒度**：真 aMule 请求一整块 `184320 B`，自己上传时按 `10240 B`
    一帧发。上传侧改为同样粒度（`wire::UPLOAD_CHUNK`），并接受
    `OP_REQUESTPARTS` 只带 1–2 组区间（原先固定读 3 组，短帧被判错帧）。
    新增 `upload_frames_follow_real_client_chunking`、
    `request_parts_accepts_fewer_than_three_pairs` 与
    `server_satisfies_amule_style_sliding_window`（复刻 aMule 滑窗取片的回归测试）。
  - **下载侧覆盖判定写反**：真 aMule 把 `184320 B` 的请求拆成 10 帧 `10240 B`
    应答，旧的 `r.0 >= start && r.1 <= end` 认为“分片没有覆盖请求”，
    `in_flight` 永不清空，进度停在少数块。改为按字节覆盖判定后，
    `>PARTSIZE` 从真 aMule 下载 **10/10 PASS**（`bytes_received=9736192`、
    950 个 `OP_SENDINGPART` 分片、`hashset_verified=true`、
    `completed/hash_matches/content_matches=true`）。
- **跨分片证据半闭环**：真 aMule 2.3.3 / RFC1918 LAN 上，`>PARTSIZE`
  (`9,736,192 B`)：
  - **下载方向（aMule → coldrevived）PASS 10/10**，落地字节与 ed2k_hash 一致。
  - **上传方向（coldrevived → aMule）仍 BLOCKED**：多轮推进到
    **19.2%–85.3%** 后转 `Waiting`，对端 `Recv-Q` 积压约 416 KB 却不再消费，
    没有拿到对端落地文件与 md5。因此不宣称 FR-25 跨分片闭环。
  - 原始帧级 trace 与报告在 gitignored 的 `tests/e2e/w25-daemon/{run,reports}/`。
- 新增本地复核入口 `tests/e2e/w25-daemon/daemon_cross_chunk.py`（双向）与
  `daemon_download_cross_chunk.py`（仅下载方向）；运行产物不入库。

## [2.4.98] - 2026-11-21 — P0/P1/P2 与 M1/M2 欠账复核 + macOS 家宽取证 kit

> 本轮不改产品代码与门禁口径：`g2_claim=false`、`s6_status=not_run`、默认承载 `tcp`。
> 复跑 `CARGO_INCREMENTAL=0 cargo test --workspace --offline` **384 passed / 0 failed / 3 ignored**。

- 新增 [DOC-29 阶段欠账复核](docs/06-plan/STAGE-DEBT-AUDIT-2026-11-21.md)：逐项核对 P0/P1/P2、M1/M2，
  结论是**欠账主要在代码/授权**（FR-25 daemon 与跨分片、FR-10 守护进程多任务、G0 `protocol_scope`、
  SRS 评审与 FR-24 重号、公平性阈值/拥塞语义），换网络只能补环境类描述性证据。
- 纠正陈旧事实：W62 报告实际 **11/11**（PHASE-SUMMARY/ROADMAP 原写 12/12）；DOC-28 基线补注
  2026-11-21 复核值；ROADMAP §6 标题日期更新至 11-20。另在 DOC-29 登记 `spikes.json`
  里 `57100→7100`「可用」与骨架清单两处历史表述与现行证据不符，未改机器文件。
- 新增 `tests/e2e/homeprobe-macos/`：macOS 家宽取证 kit（仅 Python 标准库，Ventura/Sequoia、
  Intel/Apple Silicon 均可；**不绑定本机 IP，动态公网 IP 换址后直接重跑**）。
  `run_all.sh` 跑离线自检、平台信息、STUN/NAT 行为、只读 DHT（ping/get_peers，无 announce）、
  KAD `0xE401` bootstrap 与汇总；输出 `results/summary.json`，缺失记 `missing`，
  恒定 `g2_claim=false` / `s6_status=not_run` / `evidence_valid_for_pass=false` / `auto_pass=false`。
- 新增 `scripts/package_homeprobe.py` 与分发包 `dist/coldrevive-homeprobe-macos-intel-v1.4.zip`
  （11 文件 + `MANIFEST.json`，含逐文件 sha256，保留 .sh/.py 可执行位）。
  离线自检：DHT 4/4、NAT 判据 14/14；真机回环烟测：自有公网节点 `ping=OK`、`get_peers`
  `token_len=8 / peers=1 / peer_list=112.5.32.34:17200`，与既有 `verify.json` 一致。
- kit README 升到 v1.2：针对 macOS 下载 ZIP 的 `com.apple.quarantine` 导致
  `chmod +x` 后仍报 `operation not permitted`，把 `bash run_all.sh` 定为首选运行方式，
  并补充 `xattr -dr com.apple.quarantine .` 排障命令（探针始终由 `python3` 解释执行，
  不需要 probe 文件自身可执行）。
- **首轮家宽回传已判读**：macOS 15.5/x86_64、Python 3.14.5 实跑完成 STUN/NAT/DHT；
  STUN 给出第二个真实家宽出口（对称型 NAT，映射与过滤均 address-and-port dependent，
  UPnP=true / NAT-PMP=false），DHT 自有节点 `ping` 44.6 ms、`get_peers token_len=8 / peers=1`，
  transmissionbt `ping` 通；KAD 首轮 `0/5 NO_REPLY` 用的是 2026-11-20 过期联系人，**不计 S2**。
  原始回传含本机内网地址与主机名，未提交公开仓库，只留本地归档与脱敏结论。
- kit README 升 v1.3、目标清单重建：`config/kad-targets.json` 改用 2026-11-21T12:29:40Z
  真 aMuleD LAN BOOTSTRAP_RES 解出的 20 个联系人中前 10 个，`bootstrap_res_to_targets.py`
  按 KAD host-order LE 反序解码（`--selftest` OK）；同清单在 Windows 主机正例对照
  **8/10** 收到 `0x09 BOOTSTRAP_RES`（明文与 zlib 打包两种帧），排除清单/字节序问题。
  `run_all.sh` 的 KAD 采样升至 10，并把 `run_step` 改为 `local code=0` + `|| code=$?`
  （消除 `set -u` 下 `code?: unbound variable` 导致汇总步骤被跳过的隐患）。
- kit README 升 v1.4：用户重跑时误用了旧包（KAD 仍 5 目标、旧 shell 报错），且从
  `~/.Trash/` 运行后在别处找不到 `results/`。`run_all.sh` 现在启动与结束都打印
  **绝对路径** `results: ...`，所有探针 `--out` / `report.py --results` 也改用绝对路径，
  README 增加“如何识别旧包”与“不要放在废纸篓运行”的说明。
- **第二轮家宽回传（v1.4，正例）**：同一台 macOS 15.5 / x86_64 家宽（对称型 NAT，
  默认网关与第 2 跳均为私有地址，疑似双层/CGNAT）用新清单 **8/10** 收到真实
  `0x09 BOOTSTRAP_RES`（`kad_plaintext`，0.2 pps / 10 目标），`results/summary.json`
  正常产出；DHT 自有节点仍 `ping=OK / token_len=8 / peers=1`。这是家宽出口具备
  KAD 专用 UDP 出/回程能力的描述性正例，与 Windows 主机 8–9/10 正例对照一致，
  **但只发 BOOTSTRAP_REQ，仍不计 S2 / FR-24 公网检索**，机器常量不变。
- **分发包改名**：`scripts/package_homeprobe.py` 输出
  `dist/coldrevive-homeprobe-macos-intel-v1.4.zip`（内部目录同名），旧的无版本后缀
  同名文件从 `dist/` 移除；`MANIFEST.json` 新增 `kit_version` 字段，README/CHANGELOG/
  DOC-29/主 README 的引用同步更新，避免新旧副本混淆。

## [2.4.97] - 2026-11-20 — FR-14 空闲自动休眠 / FR-24 真实 aMuleD KAD 检索 / FR-25 与真实 aMule 双向互通

> 承接 v2.4.96「FR-24/25 只到本地/组件级」。本轮把三项 P0 都推到**真实对端**：
> FR-14 从「只有状态字段」变成 opt-in 自动休眠；FR-24 有了公共轨（对真实 aMuleD KAD
> bootstrap + `0x33`）；FR-25 与 aMule 2.3.3 下载/上传双向都跑通。
> **不改门禁**：`g2_claim=false`、`s6_status=not_run`、默认承载仍是 `tcp`。
> 全量 `CARGO_INCREMENTAL=0 cargo test --workspace --offline` **384 passed / 0 failed / 3 ignored**。

- **FR-14 空闲自动休眠（opt-in，W14 12/12）**：新增唯一休眠入口
  `coldrevived::idle::sleep_hot_seeds`（与 `sched::wake_seed` 对称），把到期的 HOT 做种
  任务 `pause` 并 `transition(FileState::Cold)`，同时 `index.save()` 让影子索引落盘。
  配置 `engine.idle_sleep_secs`（也接受顶层/`[scheduler]` 的 `idle_sleep_minutes`，
  **秒优先、0=关闭、默认关闭**）；`/health.idle` 暴露
  `enabled/threshold_secs/idle_secs/sleeps/last_slept`。
  关键坑：`seed_tasks` 的键是 **infohash**、索引按 **file_hash**——只按一种标识匹配会让
  「引擎已 pause，索引仍 HOT」，已加回归单测钉住。
  实测：真实握手唤醒 → 取 64 KiB md5 一致（`guard.woken=1`）→ 2 s 后
  `seed_active 1→0`、`idle.sleeps=1`、`last_slept=[1]`，索引 `COLD` 且
  `state: ["COLD","WAKING","HOT","COLD"]`、4 个 `published_keys` 保留、`find` 仍
  `hit_node=A`。**边界**：只有空闲计时触发，CPU/内存/带宽阈值仍未接；仅回环。
- **FR-24 公共轨（W24-pub 12/12）**：新增 `coldrevive-ed2k search` 子命令——
  先 `BOOTSTRAP_REQ(0x01)/RES(0x09)`（aMule 不 bootstrap 就不理 `0x33`），再发
  `SEARCH_KEY_REQ(0x33)`，解析 `SEARCH_RES(0x3B)` 的 KAD `ReadTag`
  （`[type:u8][name_len:u16][name][value]`，**拒绝 STR1..STR22/BLOB**，aMule 的
  `ReadTag` 不接受），生成标准 `ed2k://`；`evidence_valid_for_pass` 只在真解析出链接时为真。
  对真实 aMuleD `192.168.1.221:4672`：`target_matches_key=true`、`bootstrap_ms=273`、
  `rtt_ms=6`、`answer_count=1`、`file_id=28654c6a…`，Key
  `ad50ee0f06284f630cdf3950a06da42e` 与 S2 发布侧逐字节一致。
  **公网仍 BLOCKED**：A 主机直连 5 个公网 KAD 节点全部超时、`answer_count=0`，
  如实留证（`reports/public-search.json`）而非改写结论。
- **FR-25 与真实 aMule 2.3.3 双向互通（W25-live 22/22）**：新增 `download` /
  `serve` / `link` 三个子命令，均走生产 `cr-ed2k::stack`。
  - 下载（我们 ← aMule `192.168.1.221:4662`）：262144 B、`blocks_received=25`、
    `hash_matches=true`、`content_matches=true`、`completed=true`。
  - 上传（aMule ← 我们）：65536 B 单块 `bytes_sent=65536`；262144 B 多 range
    `blocks_sent=3`；两者对端落地 md5 与本机样本逐字节一致。
  - `link` 由**文件内容**导出 `ed2k_hash`（MD4 over 9.28 MB + Red 变体规则），
    不许从别处抄 hash。
  - 判据口径修正：aMule 会对已收区间**重复发 `REQUESTPARTS`**，重复发送如实计入
    `bytes_sent`（多块样本 339968 > 262144），故上传方向用「对端落地 md5」判正确性，
    只有单块样本才要求字节数精确相等。
  - **仍 BLOCKED**：daemon 未接线、公网 server/peer 未测、全量双轨发布未做；
    无 server 登录 / source exchange / credit / AICH；样本都 < `PARTSIZE(9,728,000)`，
    跨分片与 `OP_HASHSETREQUEST` 校验未覆盖（`hashset_verified=false`）；
    `queue_rank_sent=""` 表示队列立即授予，`OP_QUEUERANKING` 分支未被真实客户端触发过。
- 文档：新增 `tests/e2e/w14/`、`tests/e2e/w24-pub/`、`tests/e2e/w25-live/`（各带
  `make_report.py` + `README.md`）；DOC-28 状态盘点按新口径回填 FR-14/24/25 三行与「容易高估的点」。

## [2.4.96] - 2026-11-20 — 补 P0 未实现 FR-11/24/25（FR-11 闭环，FR-24/25 部分）

> 按阶段盘点（DOC-28）的建议先补 P0。**FR-11 完全闭环；FR-24/25 只到「本地/组件级」，
> 公网与 daemon 全量接线仍 BLOCKED**。不改门禁：`g2_claim=false`、`s6_status=not_run`。
> 全量 `CARGO_INCREMENTAL=0 cargo test --workspace --offline` **377 passed / 0 failed**。

- **FR-11 握手拦截（W62，11/11）**：`cr-guard` 从骨架扩为标准 68 B BT 握手解析 +
  `read_handshake`（5 s 超时，防 T0-07 裸 connect 挂线程）+ `splice_prefaced`
  （补回已读字节，否则引擎收到截断握手 → 唤醒成功但 0 数据面）。
  `engine.guard_handshake` opt-in，默认 false；开启后引擎退到回环内部口、guard 占对外
  bt_port，按 infohash 走唯一合法入口 `sched::wake_seed` 本地唤醒。
  `/health.guard` 暴露 handshakes/woken/missed。
  证据：两个不同 256 KiB 样本，裸握手只唤醒对应样本、取片 md5 逐字节一致；
  未知 infohash → `missed=1`、无唤醒。边界：仅本机回环、**仅 TCP**（µTP 不外透）。
- **FR-24 本地部分（W24，独立 MD4 复算 2/2）**：`cr-hash` 补无前缀 MD4 关键词哈希的
  RFC 1320 已知答案测试；`cr-kad::kad_ed2k` 纳入审计过的 A 轨 Key + 0x33/0x3B 编解码
  （离线原语，不对公网发包）；`/cli ed2k` 从真实文件内容算 `ed2k_hash` 输出标准
  `ed2k://` 链接。**公网 KAD 0x33 词典检索 BLOCKED**（G0 scope + 0/19 静默）。
- **FR-25 组件级（W25，33 单测 + 回环 e2e）**：S2 审计过的 sha1/kad wire/ed2k wire/stack
  经 `#[path]` 纳入生产 crate `cr-ed2k`（单一事实来源）。`tests/loopback_transfer.rs`
  真 TCP 跑 serve/download 两端，三种 PARTSIZE 边界（4KiB / ==9.28MB / 多分块）
  hash+content 全一致。**daemon 未接线、公网传输未测、全量双轨发布未做，仍 BLOCKED**。
- 文档：DOC-28 状态盘点按本轮三 FR 的新口径回填；新增 `tests/e2e/w24`、`w25`、`w62`。

## [2.4.95a] - 2026-11-20 — 公平性量具 v1：TCP/UDP 同预算对照 + 可靠层 RTO 时间序列

> 只加 `#[ignore]` 量具与证据，**不改协议、不改默认承载、不改门禁**。
> `g2_claim=false`、`s6_status=not_run` 不变；全量回归仍 336 passed / 0 failed。
> 量具不等于达标：可靠层仍无拥塞语义，跨公网公平性仍未测。

- **`carrier_fairness_tcp_vs_udp_probe`**（`cr-nat`，回环）：两种承载各起真中继，
  同为 64 KiB/s 全局字节桶 + 64 KiB 突发，各 4 条等权流，6 s。样本
  `tests/e2e/w61/fairness-control-v1.json`：TCP 份额
  `19.8% / 26.1% / 32.4% / 21.6%`（`data_bytes_admitted=456432`、
  `bandwidth_limited=9465`、`delivered=111`）；UDP 份额
  `23.1% / 26.8% / 25.9% / 24.3%`（`data_bytes_in=458640`、
  `bandwidth_limited=89081`、`deliver_out=441`）。TCP 帧 4096+16 B 才越过
  `SIGNAL_FRAME_MAX`，UDP 帧 1024+16 B，**只比份额不比绝对字节**。
  - 坑（已修）：首版用 8192 B 帧 + 8 KiB 突发 → 8208 B 单帧永远放不进桶，
    `data_bytes_admitted=0`；突发桶必须大于「单帧 + 16 B 目标 id」。
- **`reliable_window_rto_timeseries_probe`**（`cr-nat`，虚拟时钟）：真 `StreamCore`
  跑 64 KiB，25 ms/轮，5% 丢包 / 2% 乱序 / 1% 重复 + ACK 延迟 30 轮（750 ms，
  否则 RTO 永远停在 200 ms 量不到退避）。样本 `tests/e2e/w61/rto-timeseries-v1.json`：
  `delivered=65536` 逐字节一致、`inflight ≤ 32`、`rto 200→400→800→1600→3200 ms`、
  `retransmits=41`、`fast_retransmits=27`、`dup_dropped=39`、122 轮。
  **虚拟时钟协议面，不是网络实测；只证明退避与有界窗口确实实现，不证明拥塞安全**。
- 文档：`tests/e2e/w61/README.md`、DOC-27 阶段小结、ROADMAP 同步；
  两个探针都只打印 JSON、不自动判 pass。

## [2.4.95] - 2026-11-20 — W61：在 TCP 被 RST 的公网主机上跑通 UDP 数据面（双 NAT，1 MiB）

> `g2_claim=false`、`s6_status=not_run` 不变；默认承载仍是 `tcp`。
> 本节只把 v2.4.94 留下的「RST 部署复现」和「UDP 公网双端」两条证据补齐；
> 公平性只有转发层量具 v0，**可靠层拥塞语义仍未做**。

- **同一轮对照证据**（`tests/e2e/w61/`，`make_report.py` 12/12）：
  - TCP 对照：本机 A → `119.29.150.254:7100` 纯回显 12 轮，11 轮被
    `ConnectionResetError` 强制断开（历史 run1 是 0/12；其中一次偶发成功不推翻结论，
    ADR-020 §复盘已有同现象）；服务端读到 `Connection reset by peer`，
    抓包见伪造 RST 与无关 ack 号。**这不是协议问题，回显服务也会中招**。
  - NAT：同轮 `nat probe dedicated` 得到 A `port-restricted-cone`（`112.5.60.4`）、
    222 `symmetric`（`112.5.32.34`），两个不同出口。
  - UDP 数据面：`wake.path=relay-udp`、`ack_status=OK`、备注含
    `OPEN/OPENED 双向握手完成`；中途 `/health` 见
    `udp_tunnels_alive=1`、`holder_data.carrier=relay-sub-udp`；
    结束后 `opens=1 / splices_done=1 / ←引擎 1,049,585 B`、
    下载端 `bytes_relayed=1,050,872`，链路两端 md5 同为 `b0e93e82…0559e`。
  - 中继三方对账（Δ）：`data_in=4219`、`deliver_out=4214`、`status_out=4219`、
    `data_bytes_in=2,623,138`、`bandwidth_limited=0`、`replay=0`、`mac_fail=0`。
  - 222 管理口 `tcpdump -ni ens192` 捕获 50 包，**全部是 17900 控制面 HTTP，
    17902/BT 端口 0 包**——BT 字节没有走 A↔222 的 L2 近路。
- **公平性量具 v0（只量转发层，不宣称拥塞安全）**：新增 `#[ignore]` 量具
  `udp_relay_data_fairness_probe`（4 条 UDP DATA 流抢 64 KiB/s 全局令牌桶，3 s，
  只打印 JSON 不自动判 pass）。一次实测 `tests/e2e/w61/fairness-probe.json`：
  四条流份额 `25.5% / 21.4% / 21.4% / 31.6%`、`deliver_out=196`、
  `bandwidth_limited=44259`。**没有**可靠层拥塞控制，不能读成公平性达标。
- **可观测性修正**：`/health.holder_data.carrier` 此前写死 `relay-sub-tcp`，
  UDP 承载搬完 1 MiB 后看板仍指向 TCP 那条路。现按 `nat.data_plane_carrier`
  标 `relay-sub-udp`，单测 `holder_carrier_label_follows_opt_in_carrier` 钉住。
- **状态串**：`relay_data_plane_udp` 由 `udp-carrier-implemented-rst-unverified` 改为
  `udp-carrier-implemented-rst-path-verified`。`path` 仅限 W61 这台主机这条路径，
  不扩大成「所有 RST 部署」；`g2_claim`/`s6_status` 不动。
- **DHT（所有者已授权）**：只读 `walk` 拿到 112 contacts / 17 ping 存活；
  在自有公网节点 `82.157.68.159:6881` 上以 222 为源宣告已验证可达的
  `112.5.32.34:17200`，从 A 异机回查 `peers=1`、`peer_list=112.5.32.34:17200`
  （`tests/e2e/dht-announce/`）。三个真实 Mainline 引导节点 `token_len=0` 或超时，
  故只记「自有节点闭环」，**不称已加入 Mainline**。
- 全量回归 `CARGO_INCREMENTAL=0 cargo test --workspace --offline` **336 passed / 0 failed**
  （v2.4.94 基线 335 + 本版新增 1 条看板标签单测）。

## [2.4.94] - 2026-11-19 — ADR-024 落地：UDP 承载上的 BT 数据面（opt-in，默认仍 TCP）

> `g2_claim=false`、`s6_status=not_run` 不变；**默认承载仍是 `tcp`**，W58/W59/W60 的结论
> 全部不受影响。本版解决的是 ADR-020 那类「入向 TCP 被边缘注入 RST」部署的**覆盖面**
> 问题，不是性能问题——跨公网吞吐与公平性一律未测。

- **阶段 0：UDP 字节治理**（此前 UDP 只有每会话 pps 限额，数据面一旦上线就能绕过 ADR-014）：
  `UdpLimits` 增加 `data_byte_rate`（默认 512 KiB/s）/ `data_burst_bytes`（默认 2 MiB）。
  判据按 **UDP 帧类型**而不是帧长：只有 `UdpKind::Data` 进字节桶——沿用 TCP 那条
  「≤4096 B 帧绕过预算」会让所有 UDP DATA（≤1152 B）全部免费。
  超限回 `UdpStatus::BandwidthLimited`（码 5 + UTF-8 原因），既不静默丢，也不误报
  `target_absent`。新增统计 `data_bytes_in/out`、`bandwidth_limited`、
  `bandwidth_limited_bytes`；CLI `--udp-data-rate` / `--udp-data-burst`。
- **阶段 1：`cr_nat::relay_udp_stream`（std-only 有界可靠层）**：`"US"` 线段格式，
  20 B 头 = kind / 独立 app seq / 累积 ACK / 64 位 SACK；app seq 与 UDP 头 `seq` 分开，
  因为**重传必须换新的 UDP 头 seq**，否则被中继抗重放滑窗丢弃。
  接收侧有界乱序重组 + 重复丢弃 + 缺口驱动 ACK；发送侧 `WINDOW=32`、
  RTO 200 ms→5 s 指数退避、3 次重复 ACK 快重传、显式 CLOSE 与半关语义。
  对外只暴露 `Read + Write` 与 `split()` 两半，不引入 async 运行时。
  **中继 `STATUS` 只记数、永不滑窗**（`status_never_acts_as_ack` 钉死）。
- **阶段 2：opt-in 接入数据面**：新配置 `nat.data_plane_carrier`（`tcp` 默认 / `udp`；
  非法值回落 `tcp` 并告警）。发起方在 `derive_session_id(self_id, b"cr-dp-v1")` 的 UDP
  会话上做 OPEN/OPENED，**未收到 `0xED` 就失败且不登记隧道**（红线单测
  `udp_attach_without_peer_confirmation_fails_honestly`）；持有方在 `relay_udp` 订阅之外
  另起数据面服务，用全零 peer 构造流后从首个已验签 `DELIVER.from` 锁定对端，
  `recv_open` → 先拨本地引擎 → `send_opened` → `splice_stream_with`，复用 TCP 侧 `HolderStats`。
  `/health.data_plane` 新增 `carrier` / `udp_tunnels` / `udp_tunnels_alive`。
  **实施偏差（照写）**：UDP 数据面没有复用 `CD` magic + `TunnelMux` 分流——UDP DATA 本身
  就是独立会话，流内用 `"US"` 魔数区分；OPEN 仍复用 ADR-023 的 23 B `TunnelFrame::open` 编码。
- **实施期抓到的真 bug（已修，两个红线用例钉住）**：发起方最初把 DATA 发到持有方的**节点 id**，
  而持有方 UDP 数据面注册在 `derive_session_id(node_id, b"cr-dp-v1")` 上——节点 id 那条会话
  留给订阅/唤醒，OPEN 会被订阅线程当唤醒帧丢掉，现场只表现为「3 s 超时回落直连」，
  看起来像对端没上线；组件/引擎用例当时全绿，因为两端一致地「错」。修法是改发派生 id，
  并补正例 `udp_attach_registers_tunnel_when_holder_confirms`（**只有负例的握手测试等于没测**）。
  同时把 ADR-023 的归属红线搬到 UDP：`recv_open` 要求 OPEN 声明的发起方 id 等于中继观测来源、
  且来源不可为全零，冒名即拒（`recv_open_rejects_initiator_that_differs_from_observed_source`）。
- **阶段 3 证据（两条，均本机 `127.0.0.1`）**：
  ① `real_udp_relay_carries_1mib_under_loss_reorder_and_duplicate_injection`：
  真 `UdpRelayServer`，数据/ACK **双向**注入，1 MiB **5.99 s** 通过。
  DATA 方向 `seen=1220 / dropped=56 (4.6%) / reordered=23 (1.9%) / duplicated=11 (0.9%)`，
  ACK 方向 `seen=1175 / dropped=57 / reordered=20 / duplicated=14`；
  `retransmits=293`、接收侧 `dup_dropped=248`（重复段丢弃而非重复交付）、
  `max_inflight=32` / `max_incoming=31`（窗口有界）、交付字节精确 `1048576`、
  `status_ignored=1175/1175`。
  ② `udp_relay_frames_carry_real_librqbit_piece`：真 librqbit 经 SOCKS5 取片，
  `downloaded=1048576 finished=true`；持有者 `→引擎 1,264 B / ←引擎 1,049,586 B`；
  中继 `deliver_out=1940`、`data_bytes_in=1,120,714`、`bandwidth_limited=0`。
  与同文件里的 TCP 对照用例并列跑，同一把尺。
- **状态串（v2.4.94 当时）**：`punch_route.relay_data_plane_udp` 由 `not_implemented`
  改为 `udp-carrier-implemented-rst-unverified`。**当时 `rst-unverified` 不可省略**：
  还没有在 ADR-020 §3.1 那类机器上复现，不得据此宣称覆盖那类部署。
  v2.4.95 已用 W61 证据把后缀改为 `rst-path-verified`。
- **仍未完成（v2.4.94 当时）**：TCP 入向被 RST 的部署复现；跨公网吞吐/公平性
  （无拥塞控制，不宣称）；UDP 数据面的公网双端实测。前两项中的 RST 复现与双端
  实测由 v2.4.95 / W61 补齐，公平性仍缺。
- 全量回归：`cargo test --workspace --offline` **335 passed / 0 failed**。

## [2.4.93] - 2026-11-19 — 公网双端中继取片成功；修掉 v2.4.92 的数据面静默断流

> `g2_claim=false`、`s6_status=not_run` 不变。本版把 v2.4.92 的「同 LAN + LAN 中继」
> 换成**公网中继 + 两个不同公网出口的真实 NAT**，并因此发现 v2.4.92 的公网数据面是坏的。

- **致命 bug（已修）**：`RelayConn` 读帧用 `read_exact` 读帧头/帧体，轮询读超时被归成
  「本轮没有帧」后直接返回，但超时前**可能已消费半帧**，这些字节被丢弃；下一轮把帧体当帧头
  解析 → 「帧体超限」/解析失败 → 订阅或隧道线程 `return Err` → 连接关闭 → 中继把后续帧按
  「目标不存在」计入 `dropped`。加密路径（`SecureReader::recv`）同样如此，而公网中继**只走加密**。
  LAN 中继上记录总是整块到达，所以 W58 完全测不出来；公网 16 KiB 分块 + 512 KiB/s 令牌桶 +
  85 ms RTT 下，超时落在记录中间是常态。
  现场表现极具误导性：持有者 `←引擎 1,049,585 B`、`splices_done=1`（看着全对），
  下载方 `bytes_relayed=1441`、`tunnels=1`（也「有」隧道），下载 0 字节，中继 `dropped +224`。
- **修法**：新增 `cr_nat::secure::fill_pending` 可续读缓冲——用 `read`（而非 `read_exact`）
  累积，`WouldBlock/TimedOut` 只表示「本轮未凑齐」，缓冲原样保留，凑齐再解析；明文与加密共用。
  同时把静默死亡变可见：隧道泵出错打日志、`/health.data_plane` 新增 **`tunnels_alive`**
  （`tunnels` 只是登记条数，泵死了它照样是 1）。
- **回归测试**：`relay::frame_split_across_read_timeout_is_resumed_not_lost`（真 TCP，
  一帧故意切成 37 B + 余下、中间停 150 ms）、`secure::fill_pending_keeps_partial_bytes_across_timeouts`、
  `secure::fill_pending_reports_closed_link_as_error`。
- **W59 实测（公网，一次干净运行）**：持有者 222（真 NAT `112.5.32.34`）↔ 下载方 A
  （本机 Windows，家庭宽带 `112.5.60.4`），中继 `82.157.68.159:7100`（PSK）。
  `path=relay`、`ack_status=OK`；直连候选 `192.168.34.222:17901` 从 A 1204 ms 无应答 →
  中继接管；1 MiB 下载 `status=complete`、两端 md5 均 `b0e93e82…559e`、**<2 s**。
  持有者 `opens=1/accepted=1/splices_done=1`、`→引擎 1,264 B / ←引擎 1,049,585 B`、
  引擎自证 `served_bytes=1,048,576`；下载方 `bytes_relayed=1,050,849`（=1,264+1,049,585）、
  `tunnels_alive=1`；中继单轮 Δ`data_bytes_admitted`=**1,050,048**、Δ`delivered`=+167、
  **Δ`dropped`=0**、`bandwidth_limited`/`rate_limited` 均 0。采样 **60/60** 个点两个公网源
  同时在线上中继。证据 `tests/e2e/w59/`（13 条断言全过）。
- **边界（不变）**：222 在 `192.168.1.0/24` 上另有管理面别名，故**不是** NAT 可达性证据；
  `nat_type=unknown`、`s6_status=not_run`；控制面（advertise/种子）走 LAN，只有 BT 字节流进隧道；
  单次 1 MiB、未测吞吐与并发。
- **W60 证据补充（同版本，无代码改动）**：复查发现 W59 的 `nat_type=unknown` 是**报告初始值**——
  `w59-run.sh` 从未调用 `nat probe`，「STUN 未产出映射」并非实测。补跑后两端都拿到真实映射：
  A `112.5.60.4:38576/38576` = `port-restricted-cone`（EIM），222 `112.5.32.34:2053/2054`
  = `symmetric`。用验收量具旁路强制打洞：`punch_sent=2 acked=false peer_punched=false`、
  真实超时 1203 ms → **失败**（symmetric 对端到 A 的映射端口不可预知）；生产自动路径据对端
  `relay-only` 声明 `skip_direct=true` 选中继，`ack_status=OK`、OPEN/OPENED 完成，
  1 MiB `complete`、两端 md5 一致、`bytes_relayed=1,050,849`。证据 `tests/e2e/w60/`
  （18 条断言全过）。注意 W59 边界文字里的「STUN 未产出映射」应以此条为准。
- **仍未完成**：UDP 承载的数据面（无重传层）；方案选型见 **ADR-024 提案（未批准，无代码改动）**：
  但**同版本无代码改动**；`s6_status=not_run`、`g2_claim=false` 不因此放宽。
- 全量回归：`cargo test --workspace --offline` **313 passed / 0 failed**。

## [2.4.92] - 2026-11-19 — 持有者侧 splice + OPEN/OPENED 握手 + 双进程中继取片实测

> `g2_claim=false`、`s6_status=not_run` 不变。本版补上 ADR-023 缺的**另一半**：
> 接收方不再需要手工登记，两个真进程能自己把中继隧道接到本地引擎上。

- **`cr-nat::relay_tunnel` 重构**：拆出 `TunnelMux`（不拥有连接、由调用方驱动，出向帧
  逐帧带目的地——一条订阅连接服务多个对端）、`TunnelReader`/`TunnelWriter`（可 `split`，
  两半互不阻塞）、`splice_blocking_with`（进度回调 + 半关语义）、`derive_session_id`。
  新增 `FLAG_OPEN(0x04)` / `FLAG_OPENED(0x08)`：`OPEN` 的 body **必须**是 16 B 的发起方
  会话 id，`OPENED` 必须为空，长度不符一律拒绝。
- **双向握手（反假阳性）**：发起方 `send_open` 后必须 `wait_opened(3 s)`，超时即失败、
  不登记进 SOCKS5 桥；接受方**先拨通本地引擎再回 `OPENED`**。顺序反过来会让下载方把一条
  空隧道登记进桥，现场表现成「BT 连接凭空 EOF」，而日志里全是「隧道已登记」。
  `OPEN` 声明的 id 必须等于中继观测到的来源 id——中继不做归属校验，端上不做就等于
  谁都能往别人的 channel 灌字节。
- **持有者侧 `relay_sub` splice**：订阅连接上按 magic 分流（`CD` → 数据面，其余 → 唤醒，
  逐字节不变）；`OPEN` → 独立线程拨 `engine.listen` + 双向 splice。**不在订阅循环里同步
  拨号**，否则一条 BT 连接能把唤醒路径一起卡住；有活动 channel 时读超时从 500 ms 降到
  20 ms，并在等唤醒应答的空隙里排空出向队列（否则一次唤醒能把数据面拖慢 1.5 s）。
- **`/health.holder_data`**：`opens/accepted/rejected/duplicate_open/peer_mismatch/
  unknown_channel/data_frames/bytes_to_engine/bytes_from_engine/splices_done`，
  字节数**边传边计**（只在收尾时加，传输期间会一直显示 0）。
- **`nat.relay_rendezvous`（新，opt-in）**：中继会合地址（限 RFC 5737 文档网段，必须与
  `punch_route=true` 同配）。没有入向映射时持有者把它作为 `bt_external`（`kind=relay`）
  通告，对端引擎据此拨号、由本机桥改道到中继隧道。它**不是端口映射**：不进
  `is_explicit_port_mapping()`、不作为公网入向证据；填真实 IP/私网地址**启动即报错**。
  `tracker_endpoint_for` 对 `kind=relay` 单独放行（唯一消费者是装有本地桥的对端）。
- **引擎侧已知 peer**：`punch_route=true` 时把对端通告的 `bt_external` 作为
  `initial_peers` 交给 librqbit，不再依赖「持有者必须有可达 tracker」。
- **修复 `/health` 自锁（长期误记成数据面的锅）**：原实现在 `Json::obj(vec![...])` 的
  每个元素里各写一次 `state.lock()`，前一个元素的 guard 活到语句结束，**同一线程第二次
  `lock()` 把自己锁死** → `/health` 永远超时，而 `/nat/public`（不碰 state）正常。
  现在整段只取一次锁；回归用例 `health_answers_promptly_even_with_data_plane_enabled`
  用 3 s 读超时当判据。
- **双进程实测（W58）**：222（holder，`nat.punch_route`+会合地址）与 221（member）经中继
  `192.168.1.221:7100` 完成 1 MiB 取片：`path=relay`、`ack_status=OK`、
  `中继数据面已确认（OPEN/OPENED）`、下载 `complete` 且两端 md5 一致
  （`6172cae1…d5852`）；持有者 `opens=1/accepted=1/splices_done=1`，
  `→引擎 1,264 B / ←引擎 1,049,585 B`，引擎自证 `served_bytes=1,048,576`；
  中继 `dropped=0`、`bandwidth_limited=0`。证据 `tests/e2e/w58/`。
  **边界**：两台同处 `192.168.1.0/24`、中继在 LAN 上，故**不是** NAT 可达证据；
  221 → `82.157.68.159:7100` 的 `REGISTER` 拿不到 ACK（原因未定位）已如实记录，
  未拿它当证据。
- **仍未完成**：UDP 承载的数据面（无重传层）；「打洞失败 → 中继接管」的公网双端完整链路。
- 全量回归：`cargo test --workspace --offline` **310 passed / 0 failed**。

## [2.4.91] - 2026-11-19 — 跨双 NAT 中继实测 + BT-over-relay（ADR-023）

> `g2_claim=false`、`s6_status=not_run` 不变。

- **A：跨两个真实 NAT 的中继互达**：221（port-restricted cone / `112.5.60.4`）与
  222（真 symmetric / `112.5.32.34`，绑 `192.168.34.222`）均无入向映射，
  经 `119.29.150.254:7101` UDP 中继 **200/200** 往返、0 丢包、p50 42.6 ms；
  服务端抓包同时看到两个公网源并逐包转发。证据 `tests/e2e/w56/reports/cross_nat_relay.json`。
- **ADR-023 BT-over-relay**：`82.157.68.159:7100` TCP 中继双端各 20/20，
  说明 ADR-020 的 RST 结论只对原路径成立。`cr-nat::relay_tunnel` 用既有 RELAY/DELIVERED
  帧承载 16 KiB 分块字节流（channel + DATA/CLOSE），不新增帧类型、不重造 TCP。
- **`cr-bridge::Upstream::{Direct,Tunnel}`**：SOCKS5 前端不变，tunnel 命中走桥，
  未命中仍回落真实直连。
- **真引擎 e2e**：`served_registered=1`、1 MiB 逐字节一致、负对照 0 B、
  中继 `data_bytes_admitted=1,050,071`；证据 `tests/e2e/w57/reports/relay_data.json`。
- **守护进程 opt-in**：`nat.punch_route=true` 才建 SOCKS5 桥与 runtime；
  `wake_with_plan` 成功后登记中继隧道，失败如实回落；`/health.data_plane` 可观测。
- **仍未完成**：持有者侧 splice 未接 `relay_sub`；UDP 数据面未实现；
  公网双端「打洞失败→中继接管」未跑。
- 全量回归见本轮测试记录。文档：新增 ADR-023、ADR 索引、M2-PROGRESS §10、ROADMAP、CONFIG。

## [2.4.90] - 2026-11-18 — 打洞优先 / 中继兜底（两阶段裁决，反双向黑洞）

> `g2_claim=false`、`s6_status=not_run` 不变。本版把「能打洞就打洞、打不了就中继」
> 落成**两端一致的判据**；不是公网可达证据，也不代表中继已有 BT 数据面。

- **`cr-nat::punch_route`（状态机，std-only）**：两阶段裁决 `Local → Commit`。
  `Direct` 需「本端握手成功 ∧ 对端 Local ok ∧ 对端 Commit ok」三条件同时成立，
  任一端不成立则两端共同 `Relay`——杜绝「一端直连一端中继」的双向黑洞。
- **`cr-nat::punch_wire`（线格式 + 收发）**：裁决用既有 `RELAY`/`DELIVERED` 帧承载，
  不新增帧类型、不改 `cr-wake` 协议（字段集合与 golden 未动）。
  `DELIVERED` 的来源在 `frame.node`，`payload` 是原样裁决——**不得**在负载里再拼来源 id。
- **`cr-core`：`nat.punch_route`（缺省 `false`）**。关闭时 `proxy_url` 保持 `None`，
  既有部署与 T0/T1/G1 逐字节不变。
- **`coldrevived`：`PunchRouteState` 进 `/health`**（`attempts/direct/relay/failed`），
  常驻 `relay_data_plane="not_implemented"`；新增 opt-in CLI `nat punch-route` /
  `nat punch-route-answer`。
- **验证**：状态机 12 例；**真 `RelayServer` 双端 5 例**（双方 ok→两端 Direct、
  A ok/B 不 ok→两端 Relay、都不 ok→Relay、带前缀负载被拒）；真引擎 e2e 双路径
  （Direct `served_registered=1`、Relay `served_registered=0`，均 1 MiB 逐字节一致）。
  证据：`tests/e2e/w55/reports/{direct,relay}.json`。
- **仍不成立**：中继无 BT 数据面（`Route::Relay` ≠ 取片走中继）；`wake_with_plan`
  仍未创建/登记 punched socket；公网双侧打洞仍 `BLOCKED`。
- 全量回归 **288 passed / 0 failed**。文档同步：新增 ADR-022、ADR 索引、M2-PROGRESS §9。

## [2.4.89] - 2026-11-18 — cr-bridge 真引擎端到端（回环 handoff）

> `g2_claim=false`、`s6_status=not_run` 不变。本版补上 ADR-021 的验收证据，
> 但只是**回环上的 socket 交接能力**，不是 NAT 可达或公网 BT-over-punched-TCP 证据。

- **`EngineOptions.proxy_url`**：透传给 librqbit `ConnectionOptions.proxy_url`；
  `coldrevived` 显式保持 `None`（守护进程尚未接桥），避免悄悄改变现有行为。
- **`cr-bridge` 增加可观测计数**：`served_registered`（命中登记表的 CONNECT 次数）
  与 `bytes_relayed`（双向搬运字节数）；回落直连不计入命中，防止假通过。
- **真引擎 e2e**（`tests/bridge_librqbit_e2e.rs`）：两端独立 `RqbitEngine`；
  做种方真监听；`cr_nat::tcp_punch` 外部建真 TCP；以不可直连的
  `203.0.113.9:<port>` 登记 socket；下载方只配 SOCKS5 与该假地址。
  实测 **1,048,576 B**、落盘逐字节一致、做种方 `uploaded_bytes=1,048,576`、
  `served_registered=1`；负对照 `203.0.113.10:<port>` 不登记 → **0 B 下行**。
  证据：`tests/e2e/w54/reports/bridge_librqbit_e2e.json`。
- **仍不成立**：`wake_with_plan` 未接线；入向 `splice_to_listener` 仍只有单测；
  公网双侧打洞仍 `BLOCKED`；不得说「BT 已支持公网打洞直连」。
- 地址轴按所有者决定跳过：222 只有单 IP，`112.5.60.4` 是 A 侧 CPE 出口而非
  222 可绑定地址，无法用「两个默认网关」或不存在的第二 IP 伪造 EIF/ADF 证据。
- 全量回归 **270 passed / 0 failed**。文档同步：ADR-021 实施状态、ADR-018/索引、
  M2-PROGRESS §8、ROADMAP 第三轮条目。

## [2.4.88] - 2026-11-18 — 三个可观测性/正确性修复 + 222 跨公网取片与中继对照

> `g2_claim=false`、`s6_status=not_run` 不变。本版把「看起来成功」的三条路径改成如实报错或如实可观测，
> 并补上第一条真实 WAN 数据面证据（cone NAT ↔ 有 DNAT 的公网对端）。

- **角色不再焊死在 `node_id == "A"`**：`Config.role`（`holder`/`member`，缺省仍按 node_id 推断，
  非法值启动即失败）。实测 222 用 `--node-id 222` + `role=holder` 正常建种子；此前静默走 member
  分支，只表现为 `seed_tasks=0`，没有任何报错。
- **拒绝假完成**：librqbit 对同 infohash 二次 add 会返回已有句柄，即便产物已删除也报 100%。
  `RqbitEngine::task_by_infohash` + 磁盘产物核对后如实报错；实测重复 addUri 从 `complete` 变为
  `waiting` 且 note 明写「拒绝复用并返回假完成」。
- **`/health` 新增 `seed_active` / `served_bytes`**：冷态不再是「有任务但看不出没供片」。
  实测唤醒前 `seed_tasks=1, seed_active=0`，唤醒后 `seed_active=1`，取片后 `served_bytes=2785280`。
- **证据**：222 跨公网 BT 取片 2 MiB / 8 片、sha256 与源文件一致；唤醒经公网 UDP 17202；
  内网口 `ens192` 全程 0 包。TCP 中继对照：222 `7100` 30/30 全通（p50 6–9 ms、RST 0 个），
  腾讯云 `7100` 0/30 且伪造 RST TTL 随机 44–217（真实 116）→ 注入是单主机策略（更新 ADR-020 / M2-VANTAGE）。
- 全量回归 **269 passed / 0 failed**。
- 文档同步：`M2-PROGRESS` §8（第三轮当前状态）、`ROADMAP` 第三轮条目、
  ADR-017/018 实施状态更新（决策正文不变）、ADR 索引统计修正为 21 项/20 已接受。
  `g2_claim=false`、`s6_status=not_run` 不变。

## [2.4.87] - 2026-11-18 — P3 桥接层落地（ADR-021）+ P0 多样本做种

> `g2_claim=false`、`s6_status=not_run` 不变；**公网 TCP 打洞仍 BLOCKED**。
> 本版解决的是「打通的 socket 怎么交给引擎」和「多个样本怎么同时做种」，
> **不改变能不能打通、也不宣称 BT 已支持打洞直连**。

- **P3 `cr-bridge`（ADR-021）**：不 fork、不换引擎，用 librqbit 的两个既有入口接入：
  - **出向**：`ConnectionOptions.proxy_url` 指向本地 SOCKS5 服务；命中「已打通注册表」
    就交出那条 socket，未命中回落**真实直连**（普通 BT 行为不变）；
  - **入向**：`splice_to_listener` 把打通的连接接到引擎自己的监听口，引擎当普通入站处理。
  - 6 条真实 socket 测试：注册项字节往返（键用不可达的 `203.0.113.9:6881`，若实现真去 dial
    会超时而不是假通过）、未注册目标回落直连回显、非 CONNECT 拒绝码 `0x07`、
    非法 ATYP 拒绝码 `0x08`、重复登记拒绝覆盖、splice 双向送达。
  - **顺手修一个会把「明确拒绝」伪装成「网络断」的缺陷**：命令不支持时先回拒绝码再关连接，
    但请求的地址段还没读完，Windows 回 RST，客户端只看到 `ConnectionReset`。
    改为**先读完地址段再判命令**（测试 `non_connect_command_is_refused_with_code_7` 守着）。
  - **降级照写**：对端地址显示为 `127.0.0.1`（被引擎当本地 peer，不参与 PEX/通告、部分统计不计）；
    用户态多一次拷贝，未做大包压测前不写性能结论；一个 socket 只能被取走一次。
- **P0 多样本做种**：`Config.samples`（可选数组，缺省空 = 旧行为逐字节不变）→
  `NodeState.seed_tasks: infohash → TaskId`；启动时**每个样本都以 `paused` 加入引擎**；
  `WAKEUP_REQ` 按 `entry.infohash` 激活**对应的那一个**，映射非空却查不到时返回 `None`，
  **绝不拿别的样本顶上**（`pick_seed_task` 单测守着这条）。
  `/health` 新增 `seed_tasks`（实际登记数，不是「配置了几个」）。
- **实测（真实进程，非单测）**：配置 2 个 256 KiB 样本启动 A，日志
  `样本入库: 2 条记录` + 两条 `做种任务: infohash=… 任务 #1/#2 … state=cold` +
  `做种引擎就绪: … 任务 2 个`；`/health` 返回 `"seed_tasks":2`。
  **单样本路径未变**：只配 `sample_path` 时仍是一次 `add_torrent`。
- **未完成（不得写成完成）**：① BT-over-punched-TCP 的**端到端**仍未跑（桥接层只有单测，
  尚未接进 `wake_with_plan`，也没有真实样本过桥）；② 多样本下的**守护进程级逐出**
  仍需签名唤醒才能复跑（`cr-sched` 的真引擎逐出仍是组件级证据）；
  ③ 公网双侧打洞 BLOCKED；④ 第二台 NAT 仍缺（222 双网卡同广播域，禁用 ens192 会失联且造不出两个 NAT）。
- `cargo test --workspace --offline` **267 passed / 0 failed**（新增 7 条：桥接 6 + 选任务 1）。

## [2.4.86] - 2026-11-18 — 订阅与唤醒接到中继的 UDP 承载（W3.16）：这台 TCP 被掐的机器终于能当真正的中继用

> v2.4.85 只让**量具**会走 UDP；守护进程的订阅与唤醒仍连 TCP，等于「路修好了但没人走」。
> 本版把两侧接线完成。**`g2_claim=false`、`s6_status=not_run`、原生 TCP 侧 BLOCKED 均不变**。

- **配置** `nat.relay_udp`（逗号分隔可多端点）：**同一个中继的另一种承载**，不是第二个中继；
  与 `nat.relay` 可同时配，各自独立订阅、独立切换。只配它也能工作（中继入向 TCP 被掐时就是这种配法）。
  非法端点丢弃、不造占位；旧配置无此键 → 空列表，行为逐字节不变。
- **持有者侧** `relay-sub` 新增 UDP 订阅线程：`HELLO(node_id)` → `DELIVER` → 投给本机守护进程的
  唤醒端口 → 把守护进程的**真实应答**投回来源会话（订阅线程不合成任何业务应答）。
- **请求者侧** 新增 `holepunch::relay_wake_udp` / `relay_wake_udp_any`，与 TCP 版逐条对应
  （一次性会话 id、`DATA(target=peer)`、等 `DELIVER`）。两处差别照写：
  ① 承载不重传 → 调用层最多两连发（300 ms 间隔，命中即停），**不是**把 UDP 变可靠；
  ② 中继回 `STATUS(target_absent)` 时**立刻**失败并带原因，不让调用方把「对端没注册」等成「网络慢」。
  `wake_with_plan` 在 TCP 承载全失败后（或本就没配 TCP 中继）再试 UDP，路径标签记为 `relay-udp`。
- **保活是必需的**：中继按 `last_seen` 回收空闲会话（默认 60 s），订阅平时没有业务帧，不续期就
  会在 60 s 后被静默删掉——「进程还在、日志没报错、谁都叫不醒」。订阅线程每 20 s `send_ping()`，
  `PONG` 由收包循环当普通帧忽略。**不能用 `UdpRelayClient::ping()`**：它读到 `PONG` 为止，
  会把夹在中间的 `DELIVER` 吞掉。新增回归测试把中继空闲压到 1.5 s、保活 300 ms，跑 4 s 后
  断言 `sessions_expired=0` 且仍能唤醒成功——去掉保活这条测试立刻红。
- **实测（`tests/e2e/w53/udp_relay_wake.py`，23/23 全 PASS）**：持有者 `192.168.1.221`
  以 `nat.relay_udp=119.29.150.254:7101` 订阅公网中继，请求者在 A（出口 `112.5.60.4`）侧唤醒——
  强制中继路 `path=relay-udp`、`ack_status=OK`，中继增量 `datagrams=3 / data_in=2 / deliver_out=2 /
  mac_fail=0 / replay=0 / target_absent=0`；把通告里的候选端口指到一个没监听的口（**不改 IP、不造地址**）
  后，默认路径真的先试直连（`private-lan: false`）再落 `relay-udp`；**对照**：同一中继只配 TCP 承载时
  唤醒失败，失败原因指向「连不上中继」，且 UDP 侧计数无任何新增投递。盒子侧 `sessions_active≥1`
  与持有者日志 `relay-sub` 双向确认订阅真的进了 UDP 注册表。
- **降级照写**：UDP 线缆格式没有声明帧 → 走 UDP 订阅的节点在中继看来 `declared=false`（`lookup` 报
  unknown），只影响「先查再打洞」的优化，不影响送达；UDP 无重传、无拥塞控制不变；
  A/B 仍同出口 `112.5.60.4`，「两个不同 NAT」仍未测。
- **顺手修一处会误导排查的日志**：`样本入库: {} B` 实际印的是**索引条数**，读成「1 字节」会把人
  带到「样本没写完」的错误方向，改为 `{} 条记录`。
- `cargo test --workspace --offline` **260 passed / 0 failed**（新增 5 条：UDP 唤醒往返/快速失败、
  `relay_udp` 配置解析两例、订阅保活回归）。

## [2.4.85] - 2026-11-18 — 专用 `UDP 7101` 落地；纠正 v2.4.84 的 MTU 错误结论

> `g2_claim=false`、`s6_status=not_run` 不变；**原生 TCP 侧 BLOCKED 结论不变**。

- **专用端口生效，DHT 不再被借用**：所有者加 `UDP 7101` 规则后，同一抓包口径从
  「`0 packets captured`」（v2.4.84）变为 **26 个包有来有回**（首包 `112.5.60.4.5498 > 10.1.0.2.7101`）；
  `6881/UDP` 复归 DHT（全程未下线）。`/root/vantage.sh` 现在把 STUN + UDP 中继一起拉起，
  重启机器后不会只剩 STUN。
- **跨主机实测（`7101/UDP`，A=`210` / B=`221` / 中继 `119.29.150.254`）**：
  840 轮 **840/840、0 丢包**；p50 落在 **34–52 ms**（单次样本 20 轮又到 54 ms），
  同路径 ICMP `25 ms`。
- **不把单次 p50 当性能**：先看到「1100 B 比 200 B 快 15 ms」，于是同路径**交替载荷**跑四组
  （200/1100/200/1100 → `33.95 / 46.59 / 43.18 / 42.75 ms`），大小顺序被打乱 ⇒
  **载荷与 RTT 无关**；未证实推测（不作结论）：不同源端口 → 不同 NAT 映射 → 运营商按五元组散列选路。
- **三方对账逐字节精确**：A 侧 `rounds=840 / received=840 / lost=0`；
  B 侧 `delivered_in=840 / echoed_out=840 / bytes_in=366000`（`= 620×200 + 220×1100`）；
  中继 `data_in=deliver_out=status_out=1680`（`= 2×840`）、`bytes_in=867232`
  （`= 1240×280 + 440×1180 + 7×64 + 6×64`）、`mac_fail 0 / replay 0 / target_absent 0 / too_big 0`。
- **纠正 v2.4.84 的错误结论（MTU）**：原文「64 B 头 + 1200 B 负载 = 1264 B，留在 1280 的 IPv6 最小 MTU 之内」
  **漏算 IPv6 的 40 B IP 头 + 8 B UDP 头**（`1264+48=1312 > 1280`），该说法是假的。
  `UDP_MAX_PAYLOAD` 由 `1200` 改为 **`1168`**（`64+1168=1232`，`1232+48=1280` 恰好填满），
  新增单测 `max_datagram_fits_ipv6_min_mtu` 把「不分片」钉死（1264 那个数再想回来会被断言拦下）。
- `cargo test --workspace --offline` **255 passed / 0 failed**。
- 记录：`docs/06-plan/M2-VANTAGE-2026-11-18.md` §3.4（新增）/§4，[ADR-020](docs/02-architecture/adr/ADR-020-relay-udp-transport.md) 更正段。

## [2.4.84] - 2026-11-18 — 中继的 UDP 承载（W3.14）：在被掐 TCP 的边缘上第一次拿到原生公网路径

> 所有者选择「走 UDP」而不是再换一台机器（[ADR-020](docs/02-architecture/adr/ADR-020-relay-udp-transport.md)）。
> **`g2_claim=false`、`s6_status=not_run` 不变**；TCP 侧 BLOCKED 结论不变——本版是**绕开**，不是修复。

- **新模块 `cr-nat::relay_udp`**（9 条单测，全绿）：独立线缆格式 `CRU1`（64 B 头 + ≤1200 B 负载 = 1264 B，
  留在 IPv6 最小 MTU 内）；`HELLO/HELLO_ACK/DATA/DELIVER/PING/PONG/STATUS/BYE/REJECT` 独立编号；
  **逐报文 HMAC-SHA256**（MAC 校验失败静默丢弃并计 `mac_fail`）、**回程不大于来程**（不构成放大器）、
  64 位滑窗抗重放（接受乱序，只丢真重复）、会话空闲回收、单 IP 会话上限。
  拒绝**必须带原因**，`STATUS` 区分 `forwarded` / `target_absent` / `rate_limited` / `too_big` / `not_registered`。
- **部署入口**：`coldrevive-relay --udp-listen 0.0.0.0:6881 [--udp-stats-file ...]`
  （与 TCP 同时可用，共用同一把 PSK；未配 PSK 仍只允许绑回环）；
  `coldrevive-relaybench --udp`（全链往返 + 中继 `STATUS` 分类计数）、
  `--udp-echo / --udp-remote-echo`（两端分别跑在**两台机器**上，避免「同机两 socket」冒充两方）。
- **端口事实先测清楚**：`7101/UDP` 从 A 发 `HELLO`，服务器 `tcpdump -ni eth0 -U "udp port 7101"` 得到
  **`0 packets captured`**，而同一时刻 A→`3478/UDP` 的 STUN 正常（`public_udp_egress_ok=true`）
  → 云防火墙对 UDP 同样是**按端口放行**。本轮因此复用已放行的 `6881/UDP`（DHT 临时下线 ~15 分钟，测完放回）。
- **实测（全部 0 丢包）**：同机双 socket 50 轮 p50 `49.39 ms`、200 轮 p50 `52.93 ms`（p99 `54.55`、抖动 `0.021 ms`）、
  100 轮 1100 B p50 `49.63 ms`；**跨主机**（A=`210`、B=`221`）200 轮 **200/200、0 丢、p50 `42.68 ms` / p99 `43.62 ms`**。
  同路径 ICMP 25–26 ms（往返）→ 一次中继往返 = 4 段单程 ≈ 50 ms，与实测吻合（差额含两次用户态转发）。
- **三方对账无缺口**：A 侧 `received=200/lost=0/status_forwarded=200`；B 侧（独立进程）`delivered_in=200/echoed_out=200/bytes_in=40000`（=200×200 B）；
  中继增量 `datagrams +403`（2 HELLO + 400 DATA + 1 BYE）、`data_in +400`、`deliver_out +400`、`mac_fail 0`、`replay 0`、`target_absent 0`。
- **照写边界**：UDP 无重传（`forwarded` ≠ 对端应用收到）；未做拥塞控制（UDP 侧只有粗粒度 pps）；
  A/B 同出口 `112.5.60.4`，属「两台主机、一个 NAT」，**不是两个不同 NAT**（该场景仍未测）。
- `cargo test --workspace --offline` **254 passed / 0 failed**。
- 记录：`docs/06-plan/M2-VANTAGE-2026-11-18.md` §3.3 / §4 / §5，[ADR-020](docs/02-architecture/adr/ADR-020-relay-udp-transport.md)。

## [2.4.83] - 2026-11-18 — 8443 复测：把「7100 不可用」修正为「所有非 22 的入向 TCP 被注入 RST」；补一组标注清楚的隧道内 WAN 往返

> 所有者放行 TCP `8443` 后的复测。**无代码改动**，只改结论范围。
> **`g2_claim=false`、`s6_status=not_run` 不变。**

- **判据修正**：上一轮只验到 `7100` 被拆，因此只写「此端口不可用」。`8443` 表现完全一致
  → 结论升级为「此机器上除 `22` 外的入向 TCP 一律在数据阶段被拆」。
- **伪造 RST 的直接证据**：`tcpdump -vv` 显示真实客户端报文 TTL 恒为 `116`，
  而那条 `[R.]` 的 **TTL 是 215** 且 `ack` 与服务端 ISN 无关；它到达服务端仅比 SYN-ACK 晚 **669 µs**。
  TTL 只能单调递减 → 该包不可能是客户端发的，注入点在服务器侧边缘。
  这是本项目第一次用**单条报文属性**（而非多机对照）证伪「这是对端行为」。
- **排除项逐条实测**（不留「可能」）：换端口 / 换协议（纯 echo、HTTP、SSH banner、真 TLS ClientHello、64 B 随机串）
  / 客户端本机（唯一 AV 为 Defender、专用公用防火墙均关、WinHTTP 直连）/ 机内 netfilter·tc /
  **停掉云镜做对照**（`YDService`/`YDLive` 压制到 `pgrep -c -x` = 0/0 全程，8443 仍被拆）/
  「空闲也拆」（只建连不发数据存活 8 s）/ 云防火墙未生效（SYN 到达、主机 46 µs 回 SYN-ACK）/ 服务器自身（回环 EIP 直连正常）。
- **伪造者还在 TTL 随机化**：四次抓包的伪造 RST，TTL 分别是 **215 / 65 / 169**，而同一客户端真实报文永远是 **116**。
  随机 TTL 是刻意规避固定指纹的手法——这本身比「TTL 不一致」更说明问题。
- **操作备注**：压制云镜期间 `YJ-FIREWALL-INPUT` 的 234 条 SSH 黑名单被清空，恢复后已重新挂链并逐步重灌（实测 6 条并在增长）；
  本次只用 `kill -9`，未执行会清链的 `stopYDCore.sh`，但服务退出时链仍被清，属该组件的正常行为，需知悉。
- **补一组 WAN 数字，但明确不算原生指标**：经 `ssh -L` 把中继（只绑回环）暴露到本机，
  200 轮 **200/200、0 % 丢包、p50 44.27 ms / p99 47.33 ms**，同路径 ICMP 25 ms。
  这证明中继的帧路由与 PSK 握手在真实 WAN 上功能正确，**不**证明原生端口可用；端口判据仍为 BLOCKED。
- 记录：`docs/06-plan/M2-VANTAGE-2026-11-18.md` §3.1 / §3.2 / §5（新增复现命令）。

## [2.4.82] - 2026-11-18 — 公网观测点实测：两个出口定性完成；DHT 首次有「可达邻居」；中继 7100 判 BLOCKED

> 所有者放行端口后（UDP 3478-3479 / 17800-17801 / 6881、TCP 7100）的首轮真实测量。
> **`g2_claim=false`、`s6_status=not_run` 不变**；下面每条都带可复核的原始观测。

- **出口 A（家宽 `112.5.60.4`）= `port-restricted-cone`，这次是可证伪的结论。**
  证据链三件：① 服务器侧日志逐条留痕
  `peer=112.5.60.4:1928 AnsweredHere`（主口对照）、`:1929 AnsweredHere`（**备口对照，独立 socket**）、
  `:1930 AnsweredFromPeerSocket`（服务端确实从 3479 回包）、`:1931 ChangeIpRejected`（420 已送达客户端）；
  ② 客户端 `change_port_answered=false` —— 服务端发了、客户端没收到；
  ③ 对照 B 已证明「备口入向可达」，所以「没收到」只能是 NAT 丢弃。
  映射轴同 socket 对 3478/3479 得到同一 `112.5.60.4:1951` → EIM。
  **这条推翻的不是数字，是依据**：上一版拿 Google/Cloudflare 的沉默判出的 `port-restricted`
  是不可证伪的（它们忽略 CHANGE-REQUEST），这次才有正对照。
- **出口 222（`112.5.32.34`）= `symmetric`。** 同 socket 两个目的端口得到
  `2050` / `2051` 两个不同映射 → APDM；与 v2.4.56 的手工报文实验结论一致，
  区别是这次是**工具化、外部观测、可重跑**。
- **方法学修正（本轮真正的收获）：对称映射下「过滤轴」的观测是伪影，必须丢弃。**
  222 的过滤探测同样回 `outcome=port-filtered` —— 因为 NAT 为目的端口另开了映射，
  回包落在客户端从未用过的映射上。若据此定性就会得到 `port-restricted-cone`，
  和同一台机器的 `symmetric` 直接矛盾。新增纯函数 `classify_two_axis`
  （映射轴优先，映射已随目的变化则过滤轴不参与）+ **4 条单测**守住，包括
  `symmetric_mapping_makes_filter_axis_inadmissible`。CLI 新增 `--classify`（两轴一次出结果）。
- **公网 DHT：首次拥有可达邻居。** 在观测机起 `coldrevive-dht --serve 0.0.0.0:6881`
  （node id `04e9c1f9…5f64`）：A 与 222 **两个不同网络**分别 `--ping` 成功且 `remote_id` 一致；
  `--get-peers` 返回 8 字节 token + 2 nodes；`--walk` 4 次查询 / 1 次存活 / 3 个联系人。
  **边界**：announce 仍未授权（未执行），这解决的是「邻居不可达」，不是「公网 announce 已验证」。
- **公网中继：BLOCKED（不记任何 WAN RTT）。** 现象与证据：
  ① 221 与 222（两台 Linux）连 `7100` **2–3 ms 内** `EHOSTUNREACH`，而同一台 `22` 端口 20 ms 正常；
  ② A 能完成 TCP 握手，但服务端 SYN-ACK 后 **150 µs** 出现一个 `ack` 号与本次连接不符的 RST；
  ③ 把中继换成**纯 Python echo**，症状完全一样：服务端读到 `hello-from-A` 后自己报
  `Connection reset by peer` —— 双向 RST，与我们的协议无关；
  ④ 观测机上腾讯云镜（`YDService`/`YDLive`）已把我们的二进制与测试脚本标记
  `Malware:113 report`（脚本 `mod 9`）。**结论只到这里：7100 这条 TCP 路径被中间设备重置。**
  换端口是否可行未测，故不写「中继可用/不可用」，只写「此端口不可用」。
- `coldrevive-relaybench` 新增 `--auth-key`（公网中继强制 PSK，没有它只能在回环量，没意义）。
- `cargo test --workspace --offline` **245 passed / 0 failed**（较 v2.4.81 +4）。

## [2.4.81] - 2026-11-18 — 公网观测点接入；NAT 分类从「公共 STUN 的沉默」改为「自控回包源」

> 所有者提供一台腾讯云轻量实例（广州，`119.29.150.254`，2C2G/4Mbps，内网 `10.1.0.2`，1:1 EIP），
> 指令：「看能不能增加 NAT 分类？如果能用，接下来的测试就可以添加进这一台」。
> **`g2_claim=false`、`s6_status=not_run` 不变**。

- **先纠正一个既有结论：`port-restricted-cone` 是不可证伪的。**
  前置机出口实测基线 `mapped=112.5.60.4:1239`（两台服务器映射一致 → 映射与目的无关），
  但 `filtering_behavior=port-restricted` 的唯一样本是 Google / Cloudflare —— 这两家
  **忽略 CHANGE-REQUEST**，永远从原地址回包。于是「没有回包」既可能是 NAT 拦了，
  也可能是服务器压根没换源口。旧代码在模糊情形下返回 `PortRestrictedCone`，
  等于把「没测到」写成「测到了」。新实现下该结论降级为 `unknown`。
- **新增 `cr-nat::stun_server`（RFC 5389 Binding 服务端 + RFC 5780 单 IP 变体）**：
  CHANGE-PORT **真的从另一个套接字回包**（源口确实变），CHANGE-IP 回
  `420 + UNKNOWN-ATTRIBUTES(0x0003)`（单公网 IP 做不到就明说，不沉默——沉默会被读成 NAT 拦截）。
- **新增 `stun::probe_port_filtering` 三 socket 双对照**：主口对照、**备口对照（独立 socket）**、
  被测 socket（只与主口通信）。任一对照失败或回包源被改写 → `untested`，不给结论。
  对照 B 换 socket 是必须的：复用被测 socket 会先把备口地址写进网关过滤集，
  端口受限 NAT 会被测成「不限端口」。
- **分类分层，禁止跨层升级**：新增 `FilterBehavior::PortOpenUntestedAddress` 与
  `NatType::ConeUndetermined`。change-port 有应答只证明「入向不禁异端口」，
  **不能**推出 full-cone（异 IP 未测）或 restricted-cone（地址受限未测）。
  打洞决策上与 cone 同判 `Direct`，但类型名保留问号。见
  [ADR-019](docs/02-architecture/adr/ADR-019-public-vantage-nat-classification.md)。
- **CLI**：`coldrevive-nat --serve-stun <ip:port> [--stun-alt ...] [--serve-secs N]`（观测点）、
  `--vantage <ip:port> [--vantage-alt <端口>]`（对外部观测点做双对照探测，`--json` 可脚本化）。
- **实测（本机回环，端到端）**：`primary=127.0.0.1:13478 / alt=13479`，
  `control_primary=YES control_alt=YES`，`change_port source=13479 matches_alt=YES`，
  `change_ip=420` → `outcome=port-not-filtered`、`nat_type=cone-undetermined`。
- **轻量机云防火墙实测（BLOCKED 的原因）**：从家宽向 `3478/17800/17801/40001` 各发 3 个 UDP 包
  （共 12 个），服务器 `tcpdump -ni eth0` 过滤同组端口 → **0 packets captured**；
  同刻回环自测包正常收到（排除监听器问题）；TCP `80`/`443` 也超时（Caddy 在 `:80` 监听）
  → 整段默认拒绝；机内 `ufw` inactive、`iptables` 只有云镜 SSH 防爆破黑名单 → **不是本机防火墙**。
  **公网侧 NAT 分类实测：BLOCKED（待控制台防火墙放行）**，本文不记任何公网分类结论。
- `cargo test --workspace --offline` **241 passed / 0 failed**（新增 9 条：观测点 6 条 + 双对照探测 3 条）。

## [2.4.80] - 2026-11-18 — M2 第二轮：补 Unix 证据 / 真引擎逐出 / 中继实测 / 公网 DHT 只读探测

> 所有者指令：「按建议推进」（上一轮末尾的四条建议）。
> **`g2_claim=false`、`s6_status=not_run` 不变**。证据与范围见
> [M2-PROGRESS-2026-11-18](docs/06-plan/M2-PROGRESS-2026-11-18.md)。

- **TCP 打洞 Unix 分支补证（ADR-018 的遗留项关闭）**：在 221（Ubuntu，rustc 1.100.0）
  真实编译并运行 `cargo test -p cr-nat tcp_punch` → **4/4 通过**：回环双侧建立、
  `PING/PONG` 双向字节、空端口如实超时。Linux 观测 `attempts=7`（Windows 回环为 1），
  原因是同时打开要求 SYN 交叉、先到一侧收 RST 后重试；**差异在重试次数，不在能否建立**。
  边界：仍是回环；公网双侧 `BLOCKED`（A 侧 CPE 无 TCP 映射）不变，BT 数据面仍未接线。
- **真引擎逐出路径**：新增 `real_engine_eviction_pauses_the_previous_seed` —— 真 `RqbitEngine`
  + 两个真 `.torrent`（各 96 KiB）、`paused` 加入、容量 1，按 A→B→A 唤醒，
  断言 `Engine::state` 观测到 `Cold→Hot→Cold→Hot` 且 `evicted` 指向正确任务。
  **边界**：守护进程仍只持有一个做种任务，这是组件级证据，不是「生产多任务已验证」。
- **中继真实网络测量**：新增 `coldrevive-relaybench`（往返 + 并发外部观测）。
  中继 `221:7100`，A（Windows）200 轮 / 256 B：**0 丢包**，p50 0.821 ms、p99 1.117 ms、max 1.492 ms；
  222（Ubuntu）200 轮：0 丢包，p50 0.588 ms、p99 5.661 ms、max 8.008 ms。
  单 IP 并发上限外部观测与服务端计数逐条对上：A 96 条 → 关 32（`ip_cap_rejected +32`），
  222 80 条 → 关 16（`+16`），证明限额按来源 IP 而非全局。注册频率配额在密集突发下触发
  （50 成功 / 80 被拒，服务端 `ip_rate_limited=80`）。**范围**：LAN 真机 + 真中继进程，
  **不是公网 VPS**；公网中继时延/丢包仍未测。
- **修掉「拒绝不说原因」**：配额触顶时服务端直接关连接，客户端只看到
  `读帧头失败: failed to fill whole buffer`（会被误判成网络故障）。新增
  `FrameKind::Reject = 0x0c`（负载 = UTF-8 原因）：单 IP 注册频率 / 注册表已满 / 同连接改 ID
  三条路径在断开前发原因帧；客户端 `register` / `register_declared` 翻成
  `中继拒绝请求: <原因>`。旧客户端收到 `0x0c` 当 `Unknown` 处理，行为与以前一致。
  回归测试改为断言错误里必须出现「拒绝」与「频率」。
- **`cr-dht` 公网只读探测（首次拿到真实公网应答）**：
  - 先修可用性缺陷：BEP 5 引导节点全是**域名**，工具原来只收字面 IP（报
    `invalid socket address syntax`，像是用户写错）。新增 `parse_target`：先字面解析，
    失败再当域名解析并**只取 IPv4**（socket 是 v4），回显 `resolved` 便于核对。3 条单测。
  - `dht.transmissionbt.com:6881` **ping 4/4 成功**、`get_peers` 返回 8 条 compact 节点；
    其余 5 个常见引导节点（`router.bittorrent.com`、`router.utorrent.com`、`dht.aelitis.com`、
    `dht.libtorrent.org:25401`、`dht.bt.am:2710`）全部超时。
  - 新增 `--walk <seed[,seed...]>`（只读、查询数硬上限、不发 `announce_peer`）。
    实测学到 `unique_contacts=1` —— 用**独立 Python 实现**交叉验证确认：
    该引导节点每次返回「同一节点重复 8 次」，地址每轮不同且随后不可达
    （`remote_id` 恒定，确为该节点）。**不是我方解码缺陷**，是应答本身不可用。
  - **`announce_peer` 未执行，记 `BLOCKED`**：双重原因 ① 写入公网 DHT 需所有者明确授权；
    ② 当前拿不到可用邻居，宣告出去无人转发。
- **环境坑（会重复踩，已写入文档）**：① 221 上 `cargo test` 需 `libssl-dev`（`crypto-hash`
  经 librqbit 依赖链在 Linux 要 openssl 头文件，缺了在 `openssl-sys` build script 阶段失败）；
  ② **`tar` 同步保留旧 mtime，cargo 据此复用陈旧 rlib**，会得到假的编译结论
  （现象：两端文件 md5 相同却报 `cannot find function ...`）——同步后必须 `touch` 所有 `.rs` 与 `Cargo.toml`。
- **验证**：`cargo test --workspace --offline` **232 passed / 0 failed**，Windows 与 Linux（221）
  两端各自全绿且数字一致（含新增 4 条：DHT 目标解析 3 条 + 真引擎逐出 1 条）。
  中继启动行与 `--stats-file` 计数核对通过。
- **文档**：新增 [M2-PROGRESS-2026-11-18](docs/06-plan/M2-PROGRESS-2026-11-18.md)（含复现命令与
  「未完成」清单）；ADR-017 补「真实引擎逐出」节、ADR-018 回填 Unix 分支结果；
  README / TEST / ROADMAP 同步。

## [2.4.79] - 2026-11-18 — M2 实现项推进：cr-dht / LRU 接引擎 / 中继生产化 / TCP 打洞原语

> 所有者指令：「M2 还缺 cr-dht 等骨架 crate、LRU 接引擎、中继生产化、BT/TCP 打洞。继续推进 M2」。
> 本轮是**实现推进**，不是门禁放行：`g2_claim=false`、`S6.status=not_run` 不变，
> 且四项里有两项只到「原语/单测」层（见各条边界），不得读成已完成。

- **`cr-dht` 从骨架变成可运行的 Mainline DHT（BEP 5）**：`bencode`（严格规范形式 + 深度上限）、
  160 位 `NodeId` / XOR 距离 / 紧凑节点编码、Kademlia 路由表（160 桶 × K=8）、
  `ping` / `find_node` / `get_peers` / `announce_peer` 四种 KRPC 报文、按来源 IP 签发并校验 token。
  38 条单测，其中 9 条走**真实 UDP 回环**（应答 id 一致、紧凑节点取回、token→announce→取回闭环、
  伪造 token 被 203 拒、单 key peer 上限、超长包与垃圾包丢弃）。
  新增 `coldrevive-dht` 工具（`--self-test` / `--serve` / `--ping` / `--get-peers`），
  `--self-test` 输出 `evidence_scope=loopback` / `is_s6_evidence=false`。
  **边界**：这不等于 S3 的证据（S3 仍以 `tests/spikes/s3/mainline_probe/` 的公网闭环为准），
  本轮也没有向公网宣告。
- **LRU 调度接引擎（W4.3）**：`cr-sched` 实现真实活跃池（逻辑时钟、容量、逐出、`set_cap`），
  `coldrevived` 新增 `sched::wake_seed()` 作为**唯一**允许调 `Engine::activate` 的入口：
  申请名额 → 逐出则先暂停 → 激活 → 如实返回（`serving` / `evicted` / `errors`）。
  `AlreadyActive` 不重复激活（librqbit 对 live 任务 unpause 会 bail，W5.2 踩过）；
  激活失败释放名额并放回被逐出者。新增 `engine.max_active_seeds`（缺省 8，0 直接报错），
  `/health` 增加 `seeding_active` / `seeding_cap` / `seeding_evictions`。决策见 [ADR-017](docs/02-architecture/adr/ADR-017-active-seed-pool.md)。
  **边界**：守护进程当前只持有一个做种任务，逐出路径由 6 条 `FakeOps` 单测覆盖，
  真实运行中不会被触发——不得声称「多任务调度已在生产验证」。
- **中继生产化收尾（W3.9 后续）**：按来源 IP 的**并发连接上限**（默认 64）与
  **注册频率上限**（默认 240/分钟/ip）；`--deny <ip|cidr>` 黑名单（握手前断开，
  IPv6 与非法前缀**明确报错**而不是静默不匹配）；注册表按节点 ID 首字节分 16 片
  （查询/写入锁竞争降到 1/16，语义不变：同一 ID 恒落同一片、重复注册不占新名额）。
  新增计数 `blacklisted` / `ip_cap_rejected` / `ip_rate_limited` / `ip_tracked`，
  并入 `--stats-file`；`RELAY_LIMITS` / `RELAY_DENY` 启动行可核对生效值。
  7 条新测试（黑名单、单 IP 上限、注册配额、断开后计数回收、跨片投递、重复注册）。
  期间修掉一个自己写出来的坑：per-IP 计数必须用 `Drop` 守卫回收，否则一次异常永久占名额。
- **TCP 同时打开打洞（W3.9-⑤，原语层）**：`cr_nat::tcp_punch` 用最小 FFI 做
  `socket/bind(SO_REUSEADDR)/非阻塞 connect`，句柄交回 `TcpStream` 托管；
  **完成判据是 `poll(POLLOUT)` + `SO_ERROR=0`**，不是 `peer_addr()`——实测 Windows 上
  `getpeername` 在 SYN_SENT 阶段就会返回目标地址，对没有监听的端口也报「成功」（假阳性）。
  `coldrevive-nat --tcp-punch <本地> <对端[,..]> [--echo|--echo-responder]` 可做双机验证。
  回环**两独立进程**实测：`attempts=1`、`elapsed` 1–4 ms、`PING/PONG` 双向往返，
  单测连续 5 轮双侧均建立。决策与边界见 [ADR-018](docs/02-architecture/adr/ADR-018-tcp-hole-punch.md)。
  **边界**：① BT-over-punched-TCP **未接线**（librqbit 9.1.0 不接受外部 socket）；
  ② Unix 分支（`poll` + `__errno_location`）本机无法编译验证，须在 221 上复跑；
  ③ 公网 NAT 双侧实测 `BLOCKED`：A 侧 CPE 未为固定本地端口建 TCP 映射，
  UDP 学到的映射端口不能用于 TCP。三项都不得写成「BT/TCP 打洞已完成」。
- **顺带修掉的两个真实缺陷**：① TCP 打洞在 Windows 上首次调用 `socket()` 前未做
  `WSAStartup`（std 会惰性初始化，独立二进制不会 → 10093）；② 全部候选都建连失败时
  重试循环不退让，超时前空转两百多万轮。
- **验证**：`cargo test --workspace --offline` **228 passed / 0 failed**（本轮新增 63 条）；
  `coldrevive-dht --self-test` 回环 PASS；`coldrevive-nat --tcp-punch` 双进程回环 PASS；
  `coldrevive-relay` 启动行与计数文件核对通过。
- **文档**：ADR-017 / ADR-018、ADR 索引（18 项）、[CONFIG](docs/04-protocol/CONFIG.md)
  增 `engine.max_active_seeds`、[ROADMAP](docs/06-plan/ROADMAP.md) M2 进度条目。

## [2.4.78] - 2026-11-17 — S3 / S5 / S6 判据判定（所有者依证据手工回填）

> 所有者指令：「根据实验的结果，给 S3/s5/s6 下判据」。本轮是**判定**，不是新的测量：
> 不改任何实测报告，不改任何工具常量，也不宣称 G2 达标。
> 判定记录：[G2-SPIKE-VERDICT-2026-11-17](docs/06-plan/G2-SPIKE-VERDICT-2026-11-17.md)、
> 机读 `tests/fixtures/g2-spike-verdicts.toml`。

- **S3 = `pass`**（限定范围）：两条判据均有真实观测直接支撑 —— 串扰 0（结构层 anomalies/leak=0、
  线级 1437/1437 畸形帧被拒 + misrouted=0 + loss=0、跨主机 17/17）；资源增量 10k=2.26 MiB /
  50k=11.08 MiB、跨进程峰值合计 25.04 MiB、双栈带宽 3.276 Mbit/s 呈加法；公网 Mainline 测试 key
  由独立进程取回 `112.5.60.4:18081`。**边界**：`cr-dht` 仍是骨架，此判定只回答「双栈资源可控」，
  不得推出「双栈 DHT 已实现」；G3 仍按 `close_with_exceptions` 结案。
- **S5 = `fallback`**：Spike 主张的机制（uTP/LEDBAT 自动退让）为假（`librqbit-utp 0.7.0` 仅 CUBIC）；
  NFR-03 两半由 `spikes.json` 早已登记的 fallback「令牌桶 + 连接数限制」在真实内核链路达成 ——
  默认 `ratio=0.02` 前台劣化 0.00%（3/3 轮）。**风险形态改为「配置不变量被改回」**：
  默认值不得回调到 0.05/0.2；0.05 档 1/3 轮 19.99% 记为未解释观测。R-07 维持 🟠。
- **S6 = `pass`（`closed_with_exceptions`）**：ADR-016 判据 2/3 满足 —— HighID direct 3/30 +
  relay 30/30，LowID direct 0/30 + relay 30/30，`aggregate` 为 `OBSERVED` / `complete=true` /
  `rejected=[]`。结案口径固定为「按 eMule 语义结案，带已知限制」；`full-cone`/`restricted-cone`/
  `cgnat` 仍是已知限制；**机器字段 `census s6_status=not_run` / `g2_claim=false` /
  `evidence_valid_for_pass=false` 不变**（工具不替人下结论，办法同 G3 的
  `g3_status` vs `g3_status_declared`）。
- **G2 仍未达标**：`g2_claim=false`。三项 Spike 只回答技术路线是否成立；M2 还欠实现项 ——
  `cr-dht` 等骨架 crate 的引擎与资源治理、LRU 调度接引擎、中继生产化收尾、BT/TCP 打洞。
- **连带修改**：`spikes.json`（三个 `status` + `verdict_2026_11_17` 块 + `m2_progress`）、
  `probe_conditions.py`（状态改为只读转述 `spikes.json`，S6 判据文字改按 ADR-016，不再硬编码
  `not_run`）、`G0-REVIEW` §2/§11、`TEST.md` 头部与 §2.11、`RISK.md` R-03/R-04/R-07 补记、
  `ROADMAP` 新增判定条目、S3/S5/S6 三个 README、`README` 版本行。
- **纪律**：判定不改写 `evidence_valid_for_pass=false` / `auto_pass=false`；
  `probe_conditions.py --self-test` 5/5、`nat_matrix.py --self-test` 24/24、
  `cargo test --workspace --offline` 168/168、`build_docs.py --check` 通过。

## [2.4.77] - 2026-11-17 — 三个残余证据缺口的真实回填（S3 公网 key / S5 内核拥塞链路 / S6 HighID）

> 所有者显式授权：「S3 可以向公网发 key 测试，未完成的 3 项都进入测试完成，s6 的一半也进入实测，s5 也真实测一下」。
> 本轮是**证据回填**，不是门禁放行：S3/S5/S6 的 `status` 仍 `not_run`，`g2_claim=false`，
> `evidence_valid_for_pass=false`，脚本没有自动置 pass。

- **S3 公网 Mainline DHT 测试 key（已完成）**：新增独立工程 `tests/spikes/s3/mainline_probe/`
  （依赖已锁定的 `librqbit-dht 9.1.0`）。两个独立进程、不同 node id：announce 进程加入公网
  DHT（bootstrap 后路由表 82 节点）并宣告测试 infohash `f273a297…` 与端口 18081；
  query 进程**不宣告**，只按同一 infohash 查询，从公网 DHT 取回 `112.5.60.4:18081`。
  只发布 S3 测试 key，不发布产品资源 Key；报告
  `s3-mainline-public-20261117.json`（工作区产物，gitignore）。
- **S3 跨进程 RSS（已完成）**：`sample_rss.sh` 并发跑两个真实 probe 进程，逐秒采样 Windows
  `WorkingSet64` / `PrivateMemorySize64`。峰值 12.38 + 12.66 = **25.04 MiB**，
  Private 合计 7.96 MiB，低于 SRS NFR-05 的 100 MB；报告
  `s3-cross-process-rss-20261117.json`（工作区产物，gitignore）。
- **S3 G3 口径**：沿用 2026-11-16 所有者人工结论 `close_with_exceptions`
  （`tests/fixtures/g3-closeout.toml`），**不是全绿**。本轮公网 key 测试没有复审
  BitComet LT Seed / aMule KAD 索引两条例外。
- **S5 真实拥塞链路（已完成）**：新增 `tests/spikes/s5/utp_netem/`（跨 netns veth +
  内核 `htb 16mbit + netem delay 20ms 2ms`，RTT 19.9/27.0/41.0 ms）。首版独立会话基线漂移
  太大，改为**单条连续 iperf3 BBR 会话**内比较 baseline/concurrent/recovery 区间中位。
  结果：默认 `ratio=0.02` 后台实际占比 0.0199、前台劣化 **0.00%（3/3）**；
  `ratio=0.2` 中位 20% 不达标；`ratio=0.05` 中位 0.01% 但有 1 轮 19.99%，不写稳定达标。
  报告 `s5-utp-netem-20261117.json`（工作区产物，gitignore）。
- **S6 HighID 一半（已完成实测）**：补齐 `run_real_nat_highid.py` 的非 smoke 30 次分支。
  反向拓扑 A（112.5.60.4，cone）持有 + 222（112.5.32.34，symmetric）请求：
  direct **3/30 成功**（打到公网 47488、`ack_status=OK`）、relay **30/30 成功**，
  载体错误 0；持有者 STUN 映射端口 47488 ≠ 内部 17801，真实设备可证伪。
  与 W3.7 LowID 报告（direct 0/30 + relay 30/30）合并后，`--aggregate` 得
  `status=OBSERVED`、`evidence_valid=true`、`reach_evidence.complete=true`；
  类别覆盖 2/4（CGNAT 0/1）仍是描述性清单。
- **声明口径不变**：S6 只能写「按 eMule 语义结案，带已知限制」，不得写「NAT 矩阵通过 /
  G2 达标」。direct 3/30 是描述性观测，不是可达率数字承诺；
  `full-cone`/`restricted-cone`/`cgnat` 未测，登记为已知限制。
- **文档/机读回填**：`spikes.json` 的 S3/S5/S6 observations 增补本轮证据；
  `nat-matrix.md` §23.4、`TEST.md` §2.11、`G0-REVIEW`、`RISK` R-03/R-04/R-07、`README` 版本行同步。

## [2.4.76] - 2026-11-17 — ADR-016：S6 判据改为 eMule 式可达语义，类别覆盖不再是 pass 前提

> 项目所有者 2026-11-17 决定：「改判据，目前只要达到 eMule 类似的可达就可以了」。
> 这是**阶段性判据 + 明确复审时点**，不是 G2 达标声明，也不是把没测的格子填成通过。

- **决策**：新增 [ADR-016](docs/02-architecture/adr/ADR-016-s6-emule-like-reach.md)（已接受）。
  按 ADR 索引 §4「已接受的 ADR 不修改正文」，**不改写 ADR-012 / ADR-013 正文**；
  ADR-016 取代 ADR-012「S6 现在的验收口径」第 1 条（5 类各有逐类报告）与
  `required_classes` 作为 pass 前提的地位。ADR-012 取消一切数值承诺的结论继续有效。
- **背景**：§13 已论证「一个出口只对应一个真实 NAT 类别」，凑齐 4 类 + CGNAT 需 ≥4 个不同
  真实出口；§18.1 否证串行重测 / 配置变体 / 合成 NAT 可以凑格子。G2 被卡在环境采购而非工程能力。
  ADR-013 / W5.2 已把 eMule 的 LowID 自我声明落地（真实环境 21/21 连续 3 轮），
  因此「按类别抽样出数字」本就失去意义。
- **新判据**（`nat_matrix_config.json` → `s6_pass_criteria`）：真实 NAT 设备上
  HighID（`direct` 可达）与 LowID（`direct` 失败但 `relay` 送达）两半各有真实证据；
  节点自报声明与实测一致（只有 `relay-only` 可跳过直连，`unknown` 不得当 LowID）；
  类别覆盖降为描述性清单，未覆盖类别写入已知限制。
- **一条没松**：条款①两台不同物理主机、条款② `nat_is_real_device` 可证伪、
  每条路径 ≥30 次真实 `WAKEUP_ACK(status=OK)`、无可达率数字、脚本永不置 pass、
  `S6.status=not_run` / `g2_claim=false` / `evidence_valid_for_pass=false`。
  **声明口径**：满足后 G2 只能写「按 eMule 语义结案，带已知限制」，不得写「NAT 矩阵通过 / G2 达标」。
- **机器侧落地**：`nat_matrix_config.json` 升 `/v2`（`s6_reach_stance=emule-like-reach`、
  `class_coverage_required_for_pass=false`、重写 `s6_pass_criteria` 与 `measurement_protocol`）；
  `nat_matrix.py` 的 `--aggregate` 改按 `reach_evidence_state()` 汇总 HighID/LowID 两半，
  类别 `matrix_complete` 保留但 `matrix_complete_is_pass_condition=false`，
  `--self-test` **24/24 PASS**（新增 7 条：立场常量、覆盖非前提、两半判据、弱层级拒收、缺字段不回填等）；
  `cr-nat` 新增 `S6_REACH_STANCE` / `S6_CLASS_COVERAGE_REQUIRED_FOR_PASS` / `S6_CLAIM_WORDING`
  与 2 条单测；`census.rs` 汇总 JSON、`coldrevive-nat --report` 输出显式带上立场，
  「判据要求 / 独立覆盖」改为「清单类别 / 独立覆盖（描述性，非 pass 前提）」。
- **机读状态**：`spikes.json` 的 `S6.criteria` 改写、`g0_review.must_validate.S6` 追加口径变更、
  新增 `g0_review.s6_criteria_amendment_2026_11_17` 留痕；
  **8 项 Spike 状态一字未动**（S3/S5/S6 仍 `not_run`），`g2_claim=false`、`spike_status_changed=false`。
- **用现有环境复核**：对 `tests/spikes/s6/reports` 跑新 `--aggregate` →
  `status=BLOCKED`、`evidence_valid=false`、`highid_reports=[]`、
  `lowid_reports=['s6-matrix-class-symmetric-real.json']`（W3.7：direct 0/30 + relay 30/30）。
  即 **LowID 一半已有真实设备证据**；`112.5.60.4`（EIM / port-restricted cone）仍只有分类、
  缺逐类 30 次实测，**HighID 一半待用现有两台物理主机补跑，无需新增出口**。
- **文档同步**：ADR 索引（16 项 / 15 已接受 / 1 待定）、`nat-matrix.md` 新增 §23（并在抬头标注
  §13/§18/§19 为历史论证）、G0-REVIEW 第二次口径变更、ROADMAP v2.16（G2 两处行 + 路线说明）、
  TEST §2.2、OPEN-QUESTIONS Q4、RISK R-04、docs/README、本文件与 README 版本行。
- **回归**：`cargo test --workspace --offline` **168 passed / 0 failed**（原 166 + 2 条新单测）；
  `nat_matrix.py --self-test` 24/24；`scripts/build_docs.py --check` 通过。
- **复审时点（必须）**：**M4 前**，或当 ≥4 个不同真实出口可用时（先到为准），
  重新评审是否恢复完整类别矩阵；维持本判据须在 ADR-016 追加复审记录，不得默示延期。

## [2.4.75] - 2026-11-17 — M2/W5.1g + W5.2：aMule 公网 KAD 复核、「先查再打洞」收口

> W5.1g 把 G3 里「aMule KAD 索引面」的取证缺口关掉；W5.2 把 ADR-013 的可达性声明真正接进
> 唤醒决策。连跑 W5.2 时又抓到一个**真产品缺陷**（订阅抖动机掉声明），已一并修掉。

- **W5.1g（20/20，公网 2 轮）**：所有者开 221 的 `UDP 4672`/`TCP 4662` 入向后，
  `tests/e2e/w51g/amule_wan_kad.py` 先从 222 验公网源 `112.5.32.34` 的双向可达，
  再用 `kad-mini wan-index-probe` 从公网发真实 `0x43`/`0x33` 帧：贴 `BOOTSTRAP_RES.self_id`
  取键（排除 tolerance 越界），PUBLISH 被 ACK、SEARCH 取回本条；LAN 正对照、未监听端口
  负对照、单 socket 双键的 `lan-ns-dual` 命名空间隔离同时成立。关键发现：**加映射不够**，
  aMule 进程内的「UDP 未验证」状态要重启才重判，故脚本显式提供 `--restart-amule`。
  只关闭取证缺口，人工 G3 结案文件不动。
- **W5.2（21/21 连续 3 轮）**：`wake_with_plan` 在绑 UDP socket / STUN / `NAT_PUNCH` 之前先
  `lookup_reach_any`；**只有有效的 `relay-only`** 才跳过直连，`direct`/`unknown`/查询失败
  照常两条路都试。`nat wake` 新增 `reach_lookup`，另加「只查不改」的 `nat reach` 供开抓包窗口前
  确认声明已传播。`only_kind` 仍是验收量具专用旁路，生产默认没有。真实 222（symmetric）/
  A / 中继实测：只走 relay、拿到 `WAKEUP_ACK`、对 A 公网出口 0 条 UDP；强制 `stun-mapped`
  的对照窗口必须抓到真实包，证明量具没失灵。
- **修掉订阅抖动机掉声明（真缺陷，不是 harness）**：修复前 `nat reach` 查到 `relay-only` 后，
  `nat wake` 有 2/3 概率又查成 `unknown` 并把注定失败的直连重试一遍。222 日志证实持有者订阅
  每 500 ms 把读超时误判断链、1 s 后重连，旧连接退出时服务端无条件 `forget_local` 删了新声明。
  现在 `RelayConn::recv_frame_optional` 把 `Ok(None)` 留给超时、订阅超时保活；服务端退出只在
  旧连接仍是当前注册表项时清理。各加一条单测（明文超时后同连接仍可用、旧连接退出不抹新声明）。
- **回归**：`cargo test --workspace --offline` **166 passed / 0 failed**（原 163 + 3 条新单测）；
  文档：ADR-013 W5.2 补充、TEST §2.9/§2.10、ROADMAP W5.1g/W5.2。
  `g3_verdict=not_run`、`s6_status=not_run`、`evidence_valid_for_pass=false` 不变，
  不含速率/可达率数字（ADR-012）。

## [2.4.74] - 2026-11-17 — M2/W5.1f：公网 µTP PASS（25/25 连续 3 轮，无代码改动）

> W5.1d 证明了公网 **TCP** 入向、W5.1e 证明了 **LAN** µTP，两件事拼不起来：µTP 走 UDP，
> CPE 上那两条 TCP 映射对它无效。运维补了第三条规则
> `UDP 112.5.60.4:17802 → 192.168.1.210:17802` 之后，本轮把「外部客户端经公网、只走 µTP 取片」
> 跑通。**产品代码未改**——能力在 W5.1d/W5.1e 已经接好，缺的一直是那条映射。

- **先分量具、再测产品**：脚本 `tests/e2e/w51f/public_utp.py` 第 1 步用**与 µTP 无关的回显服务**
  验证 UDP 映射存在，并要求 A 侧看到的源地址是公网 `112.5.32.34`（若为 `192.168.1.222`
  就是 LAN 捷径，报告作废）。映射不存在时直接 BLOCKED 并打印缺的那条规则，不与产品失败混在一起。
- **两侧证据**：qBittorrent 4.5.1 `Session\BTProtocol=2`（仅 uTP，物理上不可能走 TCP）
  用**只含公网 announce** 的种子变体取 4 MiB（同 info 换壳，InfoHash 复核一致）；判据是
  客户端自报 `ip=112.5.60.4, port=17802, connection=μTP` **且** A 侧 `bt_port` 逐 0.3 s 采样
  **没有任何 TCP 连接**（公网与 LAN 都为空）。另加 COLD 负对照与供片方计数。
- **实测（3 轮连续 PASS，25/25）**：`progress=1.000000`（143 / 153 / 147 tick）、
  落盘 SHA-1 `5118386a…f308a483` 与样本一致、供片方 `served_bytes` 0 → 4194304、
  1473–1577 条客户端采样里 peer 恒为 `112.5.60.4:17802` + `uTP`、TCP peer 0 条、
  CPE 回显 3/3 且源为公网 IP。`evidence_scope=public-utp-single-egress`、`is_s6_evidence=false`。
- **观察项**：公网 4 MiB 用时 143–153 s，与 W5.1d 的 TCP 档（155–235 s）同量级，
  未见 µTP 额外恶化；成因仍未定位，按 ADR-012 不折算任何吞吐/速率结论。
- **修掉三个 harness 缺陷（都是量具，不是产品）**：① 轮询最后一块尾部带 `POLL_END`，
  直接 `json.loads` 失败 → 100% 那格被丢，恒读成 97%；② qB 收尾报 `0.9999…`，`int()` 截成 99%，
  且中断条件 `0\.99[0-9]*` 在 **0.992188（还差两片）** 就 break；现在只认 `progress=1`；
  ③ 复位只发 delete 就重加，qB 会重新校验残留文件并**续传 48 KiB**，供片增量恰好少 49152 B；
  现在必须先 `test -e` 证明文件不存在（`GONE`）再重加，基线也在加种前读。
- **回归**：本轮无源码改动，`cargo test --workspace --offline` 仍 **163 passed / 0 failed**；
  W5.1d/W5.1e 的判据与口径不变。文档：TEST §2.8、ROADMAP W5.1f、CONFIG 的 `bt_external`/`utp`
  两行补「CPE 需要 TCP 与 UDP 两条独立规则」。

## [2.4.73] - 2026-11-17 — M2/W5.1e：补上 µTP（BEP 29），并纠正「librqbit 不支持 µTP」的错判

> 上一轮把「默认配置的 Transmission 首连很慢」记成「引擎只做 TCP」的能力缺口。
> 查依赖才发现 `librqbit` 9.1.0 本来就带 `librqbit-utp`，只是 `ListenerOptions.mode`
> 默认 `TcpOnly`——**默认关 ≠ 不支持**。本轮把它打开，并用两个真实客户端证明走的是 UDP。

- **修正（先说结论再谈实现）**：`EngineOptions.enable_utp` 默认 `true`，监听时
  `ListenerMode::TcpAndUtp`，µTP 与 BT 端口同号（UDP）；`nat.utp=false` 可显式退回纯 TCP。
  ADR-015 增加「W5.1e 更正」段：把库的默认值当能力边界，与「拿假设当测量」同族。
- **静默降级要能被看见**：librqbit 的 UDP 绑定失败只打日志、TCP 照旧。引擎在会话建立后
  对同端口做一次 UDP 绑定探测，`/cli seed` 与启动日志报 `utp_requested` / `utp_udp_port_occupied`；
  `/nat/public` 报配置态。**真正的判据是系统连接表里该 UDP 端口的 PID**，不看自报。
- **验收 `tests/e2e/w51e/utp_thirdparty.py`（20/20，连续 4 轮）**：
  - Transmission 3.00（`utp-enabled=true`，默认行为）：`100%`，`peers[].isUTP=true`；
  - qBittorrent 4.5.1（`Session\BTProtocol=2` 仅 uTP）：`100%`，`connection=μTP`（50 条采样）；
  - 两侧同时成立：**A 侧 `bt_port` 全程没有来自 221 的 TCP 连接**（逐秒采样 0 命中）；
  - 落盘 SHA-1 `8d5d0dea…309d72` 一致；供片方 `4194304 + 4194304 = 8388608`；
  - COLD 负对照仍为 0 字节（µTP 打开没有让「不唤醒也能取片」漏出来）。
  `evidence_scope=lan-utp-interop`、`is_s6_evidence=false`，不含速率数字。
- **顺手修掉两个「量具」问题**：qB 的 peer 列表只在连接存在时才有内容，4 MiB 一秒级就下完，
  「一次一条 ssh」的采样会整段错过窗口（第一次就是这么把「走了 µTP」读成「没走」），
  改为远端 0.1 s 采样进程 + 事后取文件；`Session\BTProtocol` 是位掩码（1=TCP、2=uTP、3=两者），
  w51c 里原先写的 `0`（= 都不选）已改为 `1`。
- **回归**：`cargo test --workspace --offline` **163 passed / 0 failed**（新增 2 条：µTP 默认开时
  同端口 UDP 被占用；关掉后该端口必须空着）；W5.1c **18/18**、W5.1d **17/17**、
  W5.1b **9/9**、W5.1 **8/8**、T0 `--mode real` **21/21**（`evidence_valid=true`）。
- **仍未验**：公网 µTP（CPE 只有两条 TCP 映射，缺 `UDP 112.5.60.4:17802` 一条）；
  没有它外部客户端回落 TCP，不影响 W5.1d 的结论。

## [2.4.72] - 2026-11-17 — M2/W5.1d 收口：CPE 映射建成后公网入向 PASS（17/17 累计 8 轮，最近连续 2 轮）

> 上一条（2.4.71）测出「代码就绪、卡在路由器」。本轮运维在 CPE 建好两条 TCP 映射后重测，
> **外部主机真的越过互联网从 A 的引擎取完片**——这是 M2 里第一次由真实 WAN 路径证明入向成立。
> 同时修掉 harness 自身把「量具坏了」伪装成「产品坏了」的四类缺陷。

- **实测（222 @ WAN `112.5.32.34` → A @ `112.5.60.4`）**：
  - 端口：`:17800`（tracker/.torrent）与 `:17802`（BT 数据面）均 `OPEN`；
  - 发现面：公网 `/health` 200；伴随种子 200 / 567 B，`announce-list` 两条，InfoHash `f59221cd…` 不变；
  - tracker：对公网请求方回 `112.5.60.4:17802`（62 B），不回 RFC1918；
  - 负对照：COLD 状态下客户端 `Done=0%`、`state=cold`、`served_bytes=0`；
  - 闭环：唤醒后 `seeding_hot=true`、客户端 `100%`、落盘 SHA-1 `4f1b8617…` 与样本一致、
    供片方 `served_bytes` ≈ 4.19 MB；
  - **路径纪律**：A 侧 `bt_port` 连接对端只有 `['112.5.32.34']`，LAN 对端为空——
    222 同时挂在 LAN 上，这条才是「真走公网」的证据。
- **观察项（不给结论）**：公网单连接 4 MiB 155–235 s（含首包前 ~8 s），LAN 档秒级；
  逐秒百分比单调推进、不停滞。按 ADR-012 **不折算任何速率/可达率数字**，成因未定位。
- **harness 缺陷（修的是量具，不是让步于判据）**：
  1. 222 的 sshd `MaxStartups` 会把「命令没跑成」冒充成业务失败——曾把载体失败读成
     「tracker 没有 peer」「落盘缺失」。现在载体错误单独识别并重试、空响应带
     `http_code/size/rc` 三元组、`sha1sum` 缺 hash 时先分辨 `__NOFILE__` 再判失败；
  2. 二进制 bencode 不再经 `xxd -p` 过 Windows 控制台（会被替换字符污染导致假的「无 peer」），
     改为「远端落盘 + base64」；
  3. Windows 上 ControlMaster 不可用（`ssh -f` 的常驻 master 会被沙箱回收，`-O check` 反复失败、
     反而更慢），改用单次连接 + 分类重试、两次连接间隔 ≥1.5 s、两个端口一次连完，并把逐秒轮询
     塞进对端一条 ssh 里跑完；
  4. 复位用 `-t all -r`（按 id 删在重试时会删错），复位后必须确认只剩 1 个任务。
- **非通过轮次的归因**：映射建成后共 13 轮，8 轮 17/17；另 5 轮各挂 1–3 项，逐条都能定位到上面
  这些 harness 缺陷（2 轮 hex 污染、3 轮 sshd 掐连接），**无一是产品路径失败**。
- **回归**：`cargo test --workspace --offline` **161 passed / 0 failed**（本轮无 Rust 改动）；
  W5.1c LAN 跨主机复跑 **18/18**、T0 `--mode real` 复跑 **21/21**（`evidence_valid=true`）。
- **仍未定的取舍**：µTP——librqbit 只做 TCP，默认配置的 Transmission 首连会慢（W5.1c 已登记）。

## [2.4.71] - 2026-11-17 — M2/W5.1d：公网入向接线完成，实测卡在路由器映射（BLOCKED，不虚判）

> 所有者选 **A**（公网入向优先于 µTP 取舍）。本轮把「外部客户端能进来」这条能力在代码、判据、
> 失败诊断三处都补齐，然后**如实测出它现在不成立**：A 的 `:17800`/`:17802` 从公网不可达，
> 且该 CPE 不支持任何自动建映射的协议。

- **能力（声明式，不是承诺）**：配置 `nat.bt_external` / `nat.http_external` 后
  1. 做种监听落 `0.0.0.0:bt_port`——绑回环/私网时外部包永远进不来，而本机自测仍然全绿；
  2. 伴随种子写 BEP 12 `announce-list`（LAN + 公网两条），**InfoHash 逐字节不变**
     （同一份内容不能因为多一条通告就变成另一个种子）；
  3. tracker 对公网请求方回公网 peer、对私网回私网 peer（原有逻辑，本轮补测试覆盖到 HTTP 层）；
  4. 唤醒 ACK 里的取种 URL 按请求方来源切换——公网请求者拿到 RFC1918 地址就是
     「唤醒成功但取不到种子」，而持有者本地日志毫无异常；
  5. 新增 `/nat/public`（以及 `nat public`）暴露配置态，`inbound_verified` **恒 false**：
     本机无法自证入向，只有外部主机能。
- **验收 `tests/e2e/w51d/public_inbound.py`**：先由外部主机（192.168.1.222，WAN `112.5.32.34`）
  直连 `<A 公网IP>:17800/17802`，不通即 BLOCKED 并打印缺哪两条规则；连通后才跑闭环，且要求
  **两侧证据同时成立**——客户端种子只含公网 announce（同一 info 换壳），A 侧连接表的对端必须是
  公网 IP。222 同时挂在 LAN 上，只看「下载成功」会被内网捷径骗过。
- **实测结果：`status=blocked`**。从 222 对 `112.5.60.4:17800` / `:17802` 均 `TimeoutError`
  （网关丢弃，非 RST）。旁路手段逐一否证：默认网关 `192.168.1.254` 的 UPnP / NAT-PMP / PCP
  全部无响应；本机无全局 IPv6。（首次探测把网关当成 `192.168.1.1`，按真实网关重测仍为 false。）
  需要的规则：`TCP 112.5.60.4:17800 → 192.168.1.210:17800`、
  `TCP 112.5.60.4:17802 → 192.168.1.210:17802`。建好后重跑同一命令即可。
- **回归**：`cargo test --workspace --offline` **161 passed / 0 failed**（新增 5 条：
  announce-list 不换 InfoHash / 单 tier 与旧输出逐字节一致 / `public_status` 配置态 /
  ACK URL 按来源选端点）；W5.1c LAN 复核复跑 **18/18 PASS**（未配置公网端点时种子字节不变）。
- **口径**：`evidence_scope=public-inbound-single-egress`、`is_s6_evidence=false`、
  `contains_success_rate=false`；公网入向 **not_run**，不含任何可达率数字，不构成 S6/G2 证据。

## [2.4.70] - 2026-11-17 — M2/W5.1c：LAN 跨主机复核，两个第三方客户端越过真实网卡取片

> 前一版（W5.1b）的「第三方客户端能收片」只在 `127.0.0.1` 上成立。回环不经过网卡、
> 不经过操作系统的入向栈，所以**绑定地址写错、入向被拦、tracker 把回环地址当 peer 通告**
> 这三类问题在回环上全部看不见。本轮把下载方搬到 192.168.1.221。

- **拓扑**：A = 192.168.1.210（本机，引擎 `listen=0.0.0.0:18782`，HTTP 亦绑 `0.0.0.0`）；
  B = 127.0.0.1（只负责发 `WAKEUP_REQ`）；下载方 = 192.168.1.221（真实网卡）。
  `nodes.A.host` 写成 LAN 地址——它同时决定种子里的 `announce` 与 tracker 通告的 peer。
- **两条真问题（都不是为了让测试好过而改测试）**：
  1. **µTP 互操作缺口**：Transmission 默认优先 µTP（UDP），对端只做 TCP 时回落很慢。
     抓包证据：`192.168.1.221.19501 > 192.168.1.210.18782 UDP length 30`，同一时段
     **没有发往 18782 的 SYN**；资源侧 `peers=0`、`served_bytes=0`。关掉 µTP 后立刻 ESTAB
     并完整下载。librqbit 不支持 µTP，故客户端侧显式 `utp-enabled=false`，报告与文档都如实写明
     ——**这是已登记的互操作缺口**（默认配置的 Transmission 首连会明显变慢），不是我们修好了。
  2. **`/scrape` 一直 404**：标准客户端加种子后会 scrape，404 让它持续
     「Scrape error: 404」并退避，日志看起来像 tracker 坏了。已在 `bt.rs` 实现标准 scrape 响应
     （std-only，按 query 里的**二进制** info_hash 回 `complete=1 / incomplete=0`；
     percent-decode 必须还原成 20 字节，不能按字符串处理——与当初 tracker 那个坑同源）。
- **验收 `tests/e2e/w51c/lan_thirdparty.py`（3 次连续复跑 18/18）**：221 越过网卡取 `/health`
  与 `.torrent`；tracker 通告的 peer 是 `192.168.1.210:18782`；`/scrape` 200；
  **COLD 负对照**（唤醒前客户端 Done=0% 且 `served_bytes=0`）；唤醒后 `state=hot`；
  Transmission 100% + SHA-1 `66c5db8d…330c2` 与样本一致 + `served_bytes=4194304`；
  qBittorrent 同样 100% + SHA-1 一致 + 供片增量 `4194304`（累计 8388608，两方对账）。
- **回归**：`cargo test --workspace --offline` **157 passed / 0 failed**（新增 2 条 scrape 单测）；
  T0 `--mode real` **21/21 PASS**；W5.1 / W5.1b 均未回退。
- **口径**：`evidence_scope=lan-cross-host`、`is_s6_evidence=false`、`contains_success_rate=false`。
  LAN 同一二层网段、无公网报文，**不构成 G2/G3 新证据，不含任何可达率数字**；
  公网入向（NAT 打洞 / 端口映射）与 µTP 仍未测。

## [2.4.69] - 2026-11-17 — M2/W5.1b：做种也交给引擎，给旧客户端做种成立

> 所有者决策：**「按 A 进行」**（A = 把 M1 手写的 `bt.rs` 种子端换成引擎做种，只保留一条做种路径）。
> B 方案（`bt.rs` 管门禁、引擎管生产）被否，理由不是工作量，而是**两套做种实现必然行为漂移**：
> 同一份共享文件不该有两条供片路径。

- **`bt.rs` 只剩本地 tracker**：手写 Peer Wire 种子端（握手 / bitfield / unchoke / request→piece /
  上传计数）整段删除，`tracker_response` / `tracker_response_optional` 及其字节级断言原样保留
  （仍 std-only）。模块文档写明删除原因与替代者，避免下一个人以为是漏了。
- **A 启动即建引擎并做种**：`start_engine_seeder()` 用 `EngineOptions.listen = bind:bt_port` 建会话，
  把伴随种子以 `paused: true` 加入（COLD 语义：端口在听、不供片），
  `WAKEUP_REQ` 到达才 `activate` 解除暂停。三条约束都对应门禁：T0-07 的 `bt_port_listening` 是
  真实 TCP `connect`（所以不能懒创建）；未唤醒不供片由 w51b 的负对照量化；
  引擎侧 `enable_trackers=false` + `enable_dht=false`，持有者只等入站连接，不自己 announce 回环。
- **启动失败不降级**：引擎建不起来 / 端口绑不上 / 种子加不进引擎 / 总长为 0，一律打印原因并
  `exit 2`。静默降级成「能唤醒但没有数据面」在门禁里只会表现成下载超时，排查成本更高。
- **`/cli seed` 改口径**：`served_bytes` 现在来自引擎自报的 `uploaded_bytes`，
  新增 `state` / `peers` / `checked_bytes` / `total_bytes` / `finished` / `engine_task`；
  删掉 `piece_requests`（引擎没有这个计数器，留着只能编）。不做种的节点报 `state=none` + `note`，
  不拿 `0` 冒充「观测到 0 上传」。
- **修掉一个真 bug（激活幂等）**：librqbit 的 `unpause` 对已 live 的任务会
  `bail!("torrent is already live")`，而重复唤醒是正常流量（T0-10 连发 30 次采样、客户端重试）。
  原实现会让第二次唤醒返回 Err，调用方把 `bt_seeding` 翻成 false —— 门禁看到的是**假的**
  「唤醒后不供片」。`Engine::activate` 现在先看状态，Hot 直接 `Ok`，并加进 w51b 回归。

**验收（`tests/e2e/w51b/seed_to_thirdparty.py`，3 次复跑全绿，9/9 断言）**——全本机回环：

| # | 断言 | 实测 |
| :--- | :--- | :--- |
| ① | A 的做种由引擎承担（日志有「做种引擎就绪」，无旧种子端） | `engine_ready=True, legacy_seeder=False` |
| ② | COLD 对照：未唤醒时 `state=cold`、`served_bytes=0` | 引擎已核对 1048576/1048576 B 但未上传 |
| ③ | COLD 对照：第三方 aria2 拿不完整 | 8 s 内 `bytes_on_disk=0`，超时退出 |
| ④ | 唤醒后转 `hot` | `state=hot, seeding_hot=true` |
| ⑤ | 第三方 aria2 完整下载 | `exit=0`，1 MiB |
| ⑥ | 落盘 SHA-256 == 样本 | `f8e3fa3a…670aa29` 两侧一致 |
| ⑦ | 供片方计数器覆盖全量 | `served_bytes=1048576` |
| ⑧ | 连发 3 次唤醒后仍 hot | 激活幂等回归 |

- **回归**：`cargo test --workspace --offline` 155 passed / 0 failed；**T0 `--mode real` 21/21**
  （T0-07 `bt_port_listening` 真实连接；**T0-08 就是第三方 aria2 从引擎做种处下载**）；
  W5.1 `engine_download.py` 8/8（引擎↔引擎，降级为「协议自洽」证据，不再是独立互证）。
- **边界不变**：全部回环、DHT 关闭、无公网报文；S3/S5/S6 仍 `not_run`、`g2_claim=false`；
  不产生任何可达率数字（[ADR-012](docs/02-architecture/adr/ADR-012-r04-best-effort.md)）。

## [2.4.68] - 2026-11-16 — M2/W5.1：取消零依赖，生产守护进程有了自己的数据面

> 所有者决策：**「取消零依赖，按既定方案走」**。零依赖从来不是架构目标，是 M1 的临时脚手架
> （2026-11-13 / v2.4.38 建根 `Cargo.toml` 时，G1 未过、crates.io 可达性不明，为让工程接线不被
> 网络问题拖住而刻意 std-only，`Cargo.toml` 里留了「M2 引入 rqbit/tokio 须另立 ADR」的退出条件）。

- **新增 [ADR-015](docs/02-architecture/adr/ADR-015-dependency-layering.md)：按层划分依赖**。
  协议 / 信令面 15 个 crate（`cr-core` `cr-hash` `cr-torrent` `cr-kad` `cr-wake` `cr-store`
  `cr-indexer` `cr-nat` `cr-qos` `cr-sched` `cr-guard` `cr-reputation` `cr-dht` `cr-ed2k` `cr-api`）
  **继续 std-only**——它们的字节级行为已被 T0/T1/S1/S2/S6 证据固定，换实现等于让已有证据失效；
  `cr-engine` 与 `coldrevived` 允许 **librqbit 9.1.0 + tokio**。四条配套纪律：版本只写
  `[workspace.dependencies]`、`Cargo.lock` 入库、验收一律 `--offline`、业务 crate 不得直接
  `use librqbit`（只经 `Engine` Trait）。
- **`cr-engine` 从骨架变成实现**：`Engine` Trait 由 `&mut self` 的全局 pause/resume 改为**按任务**
  （`add_torrent` / `activate` / `pause` / `resume` / `state` / `progress` / `remove`），因为一个守护
  进程要同时持有下载中、做种中、冷态的多个种子，全局开关无法表达 LRU 活跃池。`RqbitEngine` 把
  **Tokio 运行时封在 crate 内部**，对外全是同步方法，`coldrevived` 的 std 线程模型一行没改。
  默认 `enable_dht=false`、`listen=None`（不监听入站），做种必须显式给地址。
- **`coldrevived` 的 `/jsonrpc` 从「只解析」升级为「真下载」**：`addUri` 惰性创建引擎并把
  `.torrent` 交给它；`tellStatus` / `tellActive` / `tellWaiting` / `tellStopped` 每次**重新采样**引擎
  计数器（不缓存），状态映射为 aria2 的 `active/paused/complete/error`；新增
  `coldrevive.engineAttached` / `coldrevive.engineTask` / `errorMessage` 字段。速率用**两次真实采样
  之差 / 真实时间差**算字节/秒，不猜 librqbit 的 `mbps` 语义。
- **顺手修掉两个「只认一种调用方」的坑**：`aria2.addUri` 原先只认 `params[1]`，而 AriaNg 与验收脚本
  发的是 `params:[[uri]]`（无 secret），任务会被判「需要至少一个 URI」；现两种位置都接受，并加单测
  `adduri_accepts_both_param_layouts`。同类问题此前在 `tellStatus` 上已踩过一次。
- **新增第二个计数器**：内置标准 BT 种子端记录 `seeder_served_bytes` / `seeder_piece_requests`
  （只在 `send_msg` 真正写出分片后自增），配 `/cli seed` 命令。下载方报的 `completedLength` 是
  单方面说法，两边对上才算证据。
- **验收 `tests/e2e/w51/engine_download.py`（4 次复跑全绿，8/8 断言）**：B 被 `/cli wake` 唤醒 A 后，
  经 `addUri` 从 A 的内置种子端拉完 **1 MiB（4 片 / 64 次 request）**；落盘 SHA-256 =
  `37be9cb3…79e17` 与样本逐字节一致；A 侧 `served_bytes=1048576`。报告落
  `tests/e2e/w51/reports/`（本地证据，gitignore）。**边界**：单机回环、无第三方客户端、不构成
  G3/S1 新证据，不含任何可达率数字。
- **T0/T1 不变量**：`engine` 段缺省时引擎根本不创建（首次 `addUri` 才惰性建立），G1 的 21+11 条
  口径不变；`S3`/`S5`/`S6` 仍 `not_run`，`g2_claim=false`。

**验证**：`cargo test --workspace --offline` **155 passed / 0 failed**（新增 cr-engine 4 条 +
`adduri_accepts_both_param_layouts`）；`cargo build -p cr-engine --offline` 从空依赖缓存 36 s 完成，
证明本地/CI 缓存齐备时 `--offline` 成立。

**没有做的（别读成做了）**：没有验证公网做种（`engine.listen` 缺省关闭）；没有把做种开关接到
LRU 调度（仍走 M1 的 `bt_seeding` 门闩）；没有向公网发过任何报文；没有宣称 G2 或任何可达率。

## [2.4.67] - 2026-11-16 — M2/W3.18：G3 按「带例外」结案（人工判定，非机器推断）

> 口径声明：**这是结案，不是全绿**。`g3_claim=false` 不变，`g3_status`（机器）仍 `not_run`；
> 新增 `g3_status_declared=closed_with_exceptions`（人工）。两者并存，是分工不是冲突。

- **新增人工判定记录 `tests/fixtures/g3-closeout.toml`**（所有者 2026-11-16 决定「先按现状结案，
  以后有机会再测」）。为什么单独一个文件：`compat-matrix.toml` 是「有哪些客户端、有哪些报告」，
  机器可核；结案是**人的判断**。混在一起，脚本就会被动地把人的结论当成自己的输出，以后没人分得清
  谁说的、凭什么。
- **`compat_matrix.py` 只转述、不推断**：新增 `--closeout`，输出 `closeout` 块并标
  `machine_inferred=false`；机器字段 `g3_status` / `evidence_valid_for_pass` / 每行 `g3_verdict`
  仍为硬常量。`--self-test` 从 3/3 扩到 **4/4**：新增「结案结论若存在必须是转述」「结案行必须是
  矩阵内的行（防止记录漂移被静默接受）」。取值白名单校验：`decision` / `gate_status_declared` /
  每行 `verdict` 写错值直接报错——写错值比写错结论更危险。
- **逐行判定 8 行：3 `accepted` / 5 `accepted_with_gap`**
  - `accepted`：Transmission、AriaNg、Motrix/aria2
  - `accepted_with_gap`：qBittorrent（喂 `.torrent` 非 magnet）、BitComet（LT seed 未触发）、
    eMule/aMule（KAD 索引面未验证、命名空间交叉污染未实测）、浏览器插件（不下载内容）、
    关闭 UI（UI 是浏览器、数据面是 aria2）
- **6 条例外，每条例明 `recheck_trigger`**：`g3.wake_path_not_exercised`（全矩阵共同缺口：
  接收端自始至终在线，唤醒→连接没参与）、`g3.bitcomet_lt_seed`、`g3.amule_kad_index`、
  `g3.browser_extension_no_download`、`g3.ui_closed_ui_is_browser`、`g3.namespace_pollution`。
- **3 条需要所有者开环境才能重测**（已登记 `needs_owner_action`，不催办）：
  ① 放行一次受控的公共 DHT 宣告（给 BitComet LT 计数）；② 221 的 aMule 公网入向
  **UDP 4672 + TCP 4662**（221 在 VMware 内，公网映射需落宿主再转 221，两跳都要通）；
  ③ 二者完成后重测命名空间污染。
- 文档：[TEST §3](../docs/05-quality/TEST.md) 增结案表与「机器/人工双口径」读数说明（2.7）；
  [ROADMAP](../docs/06-plan/ROADMAP.md) 增 W3.18 并把「下一步」改写为「卡点不是客户端，是我们
  自己没有数据面」（2.13）；`spikes.json` `g3_progress.status=closed_with_exceptions_by_owner`。

**没有做的（别读成做了）**：没有把任何一行写成 pass；没有为结案放宽任何 harness 断言；
没有向公网写入任何报文；`S3`/`S5`/`S6` 状态不变（`not_run`），`g2_claim=false`。

**验证**：`compat_matrix.py --self-test` **4/4 PASS**；基线重算 8 行有素材
（5 `runnable_with_evidence` + 3 `evidence_collected`、0 blocked）。

## [2.4.66] - 2026-11-16 — M2/W3.17：G3 三条边界补做成真观测（仍不判通过）

> 范围声明：`g3_status` 仍 `not_run`，`g3_claim=false`，`evidence_valid_for_pass=false`。
> 本轮把 W3.16 的四条「场景没做实」里的三条转成可复现观测，第四条明确保留为未验证。

- **新增 harness（均可复现、带 `--self-test`、硬编码 `g3_verdict=not_run`）**
  - `tests/e2e/g3/emule_coexist.py`：同一轮里 KAD 真实帧与真实 eD2k TCP 传输并发。
    实测重叠 30073 ms，eD2k 262144 B / 25 块 / hash `19f7adef…` 全对；aMule 自述
    `Kad: Connected (firewalled)` 传输前中后一致，传输窗口内 `Upload: 8.57 kB/s`。
  - `tests/e2e/g3/ui_closed.py`：headless Edge 打开真实 AriaNg（CDP 可查页面 URL），
    传至 376832 B 时杀浏览器进程树（PID 消失 + CDP 端口关闭），下载继续到 1048576
    complete，SHA-1 `f0f990bb…`，seeder `uploaded_bytes=1048576/1048576`。
  - `tests/e2e/g3/browser_push.py`：MV3 模板按运行地址渲染（仓库里不再留占位链接），
    推真实 `.torrent`，`coldrevived` 取回并解析；info_hash 与脚本独立 bencode 复算一致。
- **`coldrevived` `/jsonrpc` 真的解析种子**：`aria2.addUri` 对 `http://` 链接做最小
  HTTP/1.1 GET + `cr_torrent::parse`，`tellStatus` 回真实 `totalLength` / `bittorrent.infoHash` /
  `coldrevive.pieceCount` / `coldrevive.torrentParsed`。**没有下载引擎**：任务停在 `waiting`，
  不伪造进度（`downloads_content=false`）。3 个新单测覆盖「取回+解析」「https 明确拒绝」「非 200 报错」。
- **修复 `/jsonrpc` `aria2.tellStatus` 取错参数位**：早先读 `params[1]`，导致任务在队列里
  却回 `-32001 任务不存在`。现同时接受 `params[0]`/`params[1]`，由 harness 端到端复验。
- **已知不一致（本轮暴露，未擅自改语义）**：`config/coldrevive.example.toml` 是 TOML，
  而 `Config::load` 走 `Json::load`，直接喂给守护进程会报「非法 JSON 字符 '#'」；
  harness 改为按真实 schema 生成 JSON 配置。

**明确仍未验证（不得折算为通过）**
- BitComet 长效种子传输：`lt_seed_exercised=false`。模块在跑（计数 `0 added / 0 received`）、
  标准 handshake 不受影响；触发 LT 需把测试 infohash 宣告到 BitComet 公共 DHT，
  harness 纪律是 `public_network_contacted=false`，故不做。
- aMule KAD 索引面：`kad_publish_acked=false`。aMule 报 firewalled 时其
  `Process2PublishKeyRequest` 静默丢弃 PUBLISH；实测 BOOTSTRAP 有回包（UDP 双向通）、
  PUBLISH/SEARCH 无回包。只能证明「两条通道共存」，不能证明「索引已入库/回查」。
- 插件不下载内容；关闭 UI 行的 UI 是浏览器（AriaNg），不是 BitComet/qBittorrent 桌面窗口。

**验证**：`cargo test --workspace --offline` **151 passed / 0 failed**；
五个 harness `--self-test` 分别 3/3、3/3、3/3、3/3、4/4；`compat_matrix.py` 基线仍
8 行有素材、`g3_status=not_run`。

## [2.4.65] - 2026-11-16 — M2/W3.16：G3 剩余五行补充取证（8/8 有素材，仍未判通过）

> 范围声明：`g3_status` 仍 `not_run`，`g3_claim=false`，`evidence_valid_for_pass=false`；
> S3 / S5 / S6 状态不变；不改线格式，不宣称 G3 达标。

- **BitComet 2.23.1**：Xvfb + WebUI 上传标准 `.torrent`，1 MiB 完成、SHA-1
  `f0f990bb…` 匹配、tracker `completed downloaded=1048576 left=0`、seeder
  `uploaded_bytes=1048576`。Long-Term Seeds 仅观察到 `0 added / 0 received`，
  `lt_seed_exercised=false`，不宣称长效种子传输。
- **eMule/aMule 2.3.3**：`ed2k-mini download-from` 经标准 eD2k TCP 从真实 aMule 取回
  256 KiB，25 个 `OP_SENDINGPART`，独立重算 eD2k hash 与逐字节比较通过（`diff_bytes=0`）；
  同一 aMule 进程 `Kad: Connected (firewalled)`。未同轮同时发布/搜索，
  `namespace_crosstalk_tested=false`。
- **AriaNg 1.3.15 + aria2-next 3.1.0**：UI 显示 Connected，1 MiB 完成，tracker
  `completed`、seeder `uploaded_bytes=1048576`、SHA-1 匹配；首轮 19699 残留 peer
  `uploaded=0` 已废弃，改用隔离 tracker 19700 重跑。
- **浏览器插件**：新增 MV3 取证资产 `tests/e2e/g3/assets/browser-extension/`，Edge headless
  经 ColdRevive `/jsonrpc` 的 `aria2.addUri` 创建任务；未让示例链接真实下载。
- **ColdRevive `/jsonrpc` 最小面**：`coldrevived` 增加 aria2 兼容的 `addUri` /
  `tellStatus` / `tellActive` / `tellWaiting` / `tellStopped` / `getGlobalStat` /
  `getVersion`，只登记 URI 任务，不启动下载、不伪造进度；CORS 仅用于本地取证。
- **关闭 UI**：aria2-next 后台进程完成 1 MiB，采样 `524288 B @137458 B/s`，最终
  SHA-1 匹配，seeder 与 tracker 双侧计数一致；不是 GUI 客户端关窗行为。
- **核验器**：`compat_matrix.py` 增加 `evidence_collected` 状态（有报告但无独立可执行
  客户端），fixture 登记 BitComet / aMule 远端客户端；基线 **8 行有素材
  （5 `runnable_with_evidence` + 3 `evidence_collected`）、0 blocked**，`--self-test 3/3`。
- **spikes.json**：新增 `g3_progress`，逐行记录报告路径、`g3_verdict=not_run` 与明确边界；
  `spike_status_changed=false`。
- **eD2k spike 修复**：真实 aMule 的分片边界可与请求不同，`stack.rs` 改为按**实际收到的
  字节**校验完整性（不再只看 pending 记账）；`download-from` 独立重算 hash 并逐字节比较。
- **测试**：`cargo test --workspace --offline` 148 passed / 0 failed；G3 核验器 3/3、
  local tracker 3/3、eD2k selftest 26/26。

### 边界
8/8 行有报告不等于 G3 通过。BitComet LT seed 未做传输、aMule 未做同轮 KAD 双写、
浏览器插件只验证任务创建、关闭 UI 行不是 GUI 客户端；这些边界写在报告和 TEST §3 中，
由实验执行人在补齐后手工判定。

## [2.4.64] - 2026-11-16 — M2/W3.15：三台 LAN 主机真实第三方客户端互操作（G3 附件证据）

- **卡点解除**：所有者提供 221/222/209 三台可 SSH 主机，分别装有 Transmission 3.00、
  qBittorrent 4.5.1 (nox)、aria2-next（Motrix Next）；本机另有 Motrix。G3「本机无客户端」不再成立。
- **新增** `tests/e2e/g3/local_tracker.py`：标准 BEP23 compact peer list tracker，仅 RFC1918，
  以连接来源 IP 为准，`--self-test` 3/3。修复二进制 info_hash 用 `parse_qs` 走 str 时的
  `UnicodeEncodeError`（改为字节层 `unquote_to_bytes`）。
- **真实互操作**：A 上 S1 cross-seed 产标准 `.torrent`（1 MiB），三台机器原样客户端各下一轮，
  均 1 MiB 全量、SHA-1 `f0f990bb…` 匹配、seeder `uploaded_bytes=1048576`、tracker `completed`。
  qBittorrent 首轮因残留 peer（`peer_count=4`）被判不干净并废弃，清场重跑后仅 1 peer 方采信。
- **核验器**：`compat-matrix.toml` 增 `[[remote_clients]]`，`compat_matrix.py` 增
  `runnable_remote` 状态；基线 3 行 runnable / 5 行 blocked，`--self-test 3/3`，`g3_status` 恒 `not_run`。
- **纪律**：证据是附件（gitignored `tests/e2e/g3/reports/interop-bt-2026-11-16.json`），
  G3 未全绿（eMule GUI/BitComet/AriaNg/浏览器插件/关闭 UI 未取证），S3/S5/S6 `not_run`，
  `g2_claim=false`，`evidence_valid_for_pass=false`。

## [2.4.63] - 2026-11-16 — M2/W3.14b：生产 crate 具备真实 eD2k hash

> 范围声明：S3 / S5 / S6 仍 `not_run`，`g2_claim=false`；不改线格式与 T0 golden。

- `cr-hash` 从 S2 的 kad-mini 探针迁入零依赖 MD4、`kad_keyword_hash`、
  `ed2k_hash`（aMule **Red** 整块边界规则）与标准 `ed2k_link` 生成。
  此前生产 crate 没有 eD2k hash，只有 S2 探针里有一份——旧客户端找得到却下载不了
  虚假 `fileID`，所以这是「标准协议互通」必须的生产侧能力。
- 新增 3 条单测：RFC 1320 已知向量、aMule 1×PARTSIZE 的 Red 向量、
  ed2k 链接字段转义。全量 `cargo test --workspace --offline` **148 passed / 0 failed**。

## [2.4.62] - 2026-11-16 — M2/W3.13：中继数据面字节预算（ADR-014）+ 路线重排

> 范围声明：不改变任何 Spike 的 `status`；S3 / S5 / S6 仍 `not_run`，`g2_claim=false`。
> 不改线格式，不需要 `PROTOCOL_VERSION` 递增，T0 golden 不受影响。

### 变更
- `cr-nat::relay_server`：`Limits` 加 `data_byte_rate`（默认 512 KiB/s）与
  `data_burst_bytes`（默认 2 MiB）；≤4 KiB 的信令帧绕过预算，其余帧整帧放行/拒绝；
  同侪 `FORWARD` 在接收实例再次计费。
- `RelayStats` 加 `data_bytes_admitted` / `bandwidth_limited` / `bandwidth_limited_bytes`，
  与 `dropped` 分开；`coldrevive-relay` 加 `--data-rate` / `--data-burst`，
  `RELAY_LIMITS` 与 stats JSON 同步输出。
- 路线重排：**先做标准协议互通（S1/S2 → G3 矩阵），再谈补种**；
  出口类别 `--census` 移入 S6 证据阶段，当前不排期。
- `cargo test --workspace --offline`：145 passed / 0 failed（新增 2 条）。

### 边界
带宽阈值是**部署侧资源护栏**，不是可达率承诺，不进 S6 判据；与 ADR-012 一致。
默认 512 KiB/s 是初始值而非实测最优值，按部署机带宽调整。

## [2.4.61] - 2026-11-16 — M2/W3.12：借 eMule 的 LowID 自我声明（ADR-013）

> 范围声明：不改变任何 Spike 的 `status`；S3 / S5 / S6 仍 `not_run`，`g2_claim=false`。
> 协议帧**纯追加**（0x08–0x0B），原 0x01–0x07 逐字节不变，T0 golden 不受影响。

### 背景
所有者指出：eMule 网络同样存在这几类 NAT 且多为 LowID，为什么不直接借鉴？
回答：**借它「失败应当被预先知道，而不是试出来」的核心思想**（自我声明 + 服务器记录 +
查询先行）；不借它三个语义——LowID 即终点（我们能换一条路：中继）、没有中继
（它中继不了整个文件，我们中继的唤醒信令只有几百字节）、HighID = 谁都能连
（我们的 `direct` 是「配合打洞前提下可直连」，端口受限锥型不在此列）。
决策记录 [ADR-013]。

### 落地
| 位置 | 内容 |
| :--- | :--- |
| `cr-nat/src/reach.rs`（新） | `ReachClass`（direct / relay-only / unknown）+ `ReachDeclaration`（类别 + 依据 + 时刻 + TTL）。symmetric→relay-only、锥形→direct、**cgnat→unknown**（运营商侧那一层不可推断）；TTL 过期一律降级 unknown，时钟回拨也算过期；`allows_skipping_direct()` 只对 relay-only 为 true。10 条单测 |
| `cr-nat/src/relay.rs` | 新帧 `REGISTER_DECLARED(0x08)` / `PEER_REACH(0x09)` / `LOOKUP_REACH(0x0A)` / `REACH_RESULT(0x0B)`，客户端 `register_declared` / `lookup_reach` |
| `cr-nat/src/relay_server.rs` | `declared` 表（只存储转发不判断）+ 同侪声明快照（整份覆盖）+ gauge `declared_direct/relay_only/unknown`、`declared_updates`、`reach_lookups`；负向对照：伪造快照拒、id 不一致拒、断开即清理声明。6 条新单测 |
| `coldrevived/relay_sub.rs` | 注册即声明（没探测过如实报 unknown）；探测结论更新后经同一条订阅连接重声明（500ms 轮询）。坑：unknown 兜底的时刻戳必须盖章一次，否则每 500ms 重声明一次直到永远 |
| `coldrevived/nat_path.rs` | 探测结论交付声明槽位 |
| `coldrevive-relay` | stats JSON 新增五个声明字段 |
| `coldrevive-nat` | `--lookup <中继> <节点id>` 诊断查询；`--json` / 人类输出新增 `reach_class` |
| PRD / SRS | 撤掉「公开网络 ≥90%」数值承诺（ADR-012 口径覆盖产品层）；明确「工程指标可设验收线、外部环境成功率不设数值目标」的分界 |

### 边界（不宣称）
- 唤醒路径**尚未**接入「先查再打洞」——本轮只交付原语 + 诊断消费，
  不宣称节省了任何一次超时。
- **部署顺序：先升中继、再升客户端**（旧中继收到 0x08 会拒帧断连）。
- 声明快照与 PEER_SYNC 一样依赖 ACK 先行：`PeerReach` 抢在 ACK 前面发出会让拨号方
  「期待 ACK，收到 PeerReach」而永远连不上——已修并在代码注释里记录。

测试 **143 passed / 0 failed**（+17）。

## [2.4.60] - 2026-11-16 — M2/W3.11：R-04 改为「尽力」，取消可达率的数值承诺（ADR-012）

> 范围声明：**不改变任何 Spike 的 `status`**：S3 / S5 / S6 仍 `not_run`，`g2_claim` 未立、
> `spike_status_changed=false`；`evidence_valid_for_pass=false`。
> 这一轮改的是**验收口径**，不是实测结论 —— 撤掉一个「本来就不该许」的承诺。

### 决定
项目所有者：**「我们不能承诺具体数字，只能是尽力，我们是提供一个可能性，不是确保实现，也没必要。」**
故 `R-04 ≥70%` 取消，S6 从「百分比够不够」改为四条定性要求。决策记录 [ADR-012]。

### 为什么这不是「达不到所以放宽」
1. **产品卖的不是百分比**：提供的是「除在线唤醒之外，多一条能叫醒机器的路」；
   用户要的是「叫得醒 / 叫不醒时我知道为什么」，不是 SLA。
2. **数字会污染测量**：阈值一存在，「测不到 70%」就被读成「不合格」，只剩两条路——
   挑环境凑数字，或把没测到的格子填成「看起来通过」。后者正是本项目反复在打的那类缺陷。
3. **分母不可抽样**：「所有家用 NAT」无法抽样（运营商、光猫型号、月份策略都不同），
   5 个样本算出的百分比没有统计意义，却会被当 SLA 引用。

### 落地（可检查，不是表态）
| 位置 | 改动 |
| :--- | :--- |
| `cr-nat/src/lib.rs` | 删 `R04_MIN_REACHABILITY`；加 `R04_STANCE="best-effort"` 与 `R04_NUMERIC_COMMITMENT: Option<f64> = None`（保留字段名，让机器读到「明确不给数」而不是以为漏了）；单测 `r04_has_no_numeric_commitment` 防阈值被好心加回 |
| `nat_matrix_config.json` | `r04_min_reachability` → `r04_stance` / `r04_numeric_commitment: null` / `r04_requirement` / `s6_pass_criteria`（四条） |
| `nat_matrix.py` | 聚合报告不再做阈值比较，输出 `r04_stance` + `reachability_note`（可达率是**描述性统计**）；自检新增 `r04_has_no_numeric_threshold`，**17/17 PASS** |
| `RISK.md` R-04 | 触发条件由「公网可达率 <70%」改为「**某类环境中直连与中继回退同时失败**」 |
| `TECH-STACK` / `TEST` / `OPEN-QUESTIONS` / `G0-REVIEW`（追加不改行）/ `nat-matrix.md §19` | 同步为定性口径 |

### S6 现在的判据（替代 70%）
5 类环境各 ≥1 份 `evidence_valid=true` 报告；每类 `direct`/`relay` 各 ≥30 次真实观测（成功＝真实 `WAKEUP_ACK`）；
`direct` 失败时 `relay` 必须送达，两条都失败记为该类**不可用**并写入已知限制；结论手工回填。
**要的是「每一类会怎样」被查清，不是「总体够不够好」。** 少一类就是地图缺一块，标出来即可。

### 一条强度都没放松
条款①（两台不同物理主机）与②（真实 NAT 设备）一条没动，`--evidence-scope` 分层不变，
`S6` 仍 `not_run`、`g2_claim=false`。反向约束反而更强：以前「direct 0/30」可以含糊成「没到 70%」，
现在必须逐类说明原因（如 §15 的 symmetric：网关无 DNAT、映射随目的变，只能中继）。
**中继回退因此改为常开**，不再等一个永远不会到来的触发条件。

`cargo test --workspace --offline` **126 passed / 0 failed**。

## [2.4.59] - 2026-11-16 — M2/W3.10：出口类别清单（条件不齐时先沉淀样本）

> 范围声明：**不改变任何 Spike 的 `status`**：S3 / S5 / S6 仍 `not_run`，`g2_claim` 未立、
> `spike_status_changed=false`；`evidence_valid_for_pass=false`，R-04 仍不判定。
> 本轮也**没有**对矩阵做任何回填：清单不是覆盖。

### 先纠正一个读法：「一次验证一类」不等于「分五次做」
判据要的是「4 类 NAT + 1 个 CGNAT 各有 ≥1 个真实设备样本」，而一个出口在任一时刻
只表现为**一个**类别。所以把同一个出口测五天，得到的是同一个类别五条样本，
不会多出一格。真正有用的是**换出口**再测 —— 这才是「一次验证一类」的正确形态。

### 因此把「环境不够」变成「样本先存起来」
`coldrevive-nat` 新增 `--census <jsonl>` / `--report <jsonl>`，把每次探测沉淀成
**出口类别清单**（JSONL，单行、字段顺序固定、可去重、读回严格）：

- 记录 `nat_class` + **正交两维** `mapping_behavior`（eim/apdm）+ `filtering_behavior`；
- 记录 `observations` / `distinct_mappings`：让「结论来自哪几条报文」可复核；
- 记录 `double_nat`：CPE 自报 WAN 与 STUN 映射不一致 ⇒ 中间还有一层（运营商 CGNAT 形态）；
- `variant` 区分「同一设备不同配置」；`sample_id`（哈希前 16 位）防同一份输出贴两次算两个样本。

为此 `TraversalReport` 新增 `filter_behavior` 与原始 `observations`——此前它们只用于
当场算 `nat_type`，算完即丢，导致报告能说「是 port-restricted-cone」却说不出依据。

### 三条纪律写进代码，而不是写进文档
1. **一个出口一个类别**：同一出口同一 `variant` 出现两个类别 ⇒ `unresolved`，
   **整条不进清单**（哪个才对是未知的，任选一个都是编）；
2. **清单 ≠ 覆盖**：`matrix_coverage="none"`、`r04_judgeable=false`、`s6_status="not_run"`、
   `g2_claim=false` 是**常量不是计算结果**；
3. **读回严格**：未知字段直接报错带行号，不跳过。

### 实测（真实网络，A 机家宽出口）
`nat_class=port-restricted-cone`、`mapping_behavior=eim`（2 个目的地址映射一致）、
`filtering_behavior=port-restricted`、`mapped=112.5.60.4:3008`；
汇总输出 `5 类 / 独立覆盖 1 类`，缺 `full-cone / restricted-cone / symmetric / cgnat`。
结合 W3.7 对 `112.5.32.34` 的 APDM 结论，**现有两个出口本来就是两类**
（`port-restricted-cone` + `symmetric`），缺的是另外三类。

`cargo test --workspace --offline` **125 passed / 0 failed**（+11：`cr-nat::census`）。

## [2.4.58] - 2026-11-16 — M2/W3.9：中继公网生产化（PSK 认证 + 逐帧加密 + 三道闸）

> 范围声明：**不改变任何 Spike 的 `status`**：S3 / S5 / S6 仍 `not_run`，`g2_claim` 未立、
> `spike_status_changed=false`；`evidence_valid_for_pass=false`，R-04 仍不判定。
> 这是**中继可用性与传输安全工程**，没有产生任何 NAT 类别样本。

### 先让它只服务持钥方
W3.4 的中继是**无认证明文**：任何人可注册任意 16 字节 id 顶掉别人、可监听载荷、可重放。
`cr-nat::secure`（std-only，复用 `cr-hash`）补上 PSK 传输层：

- 握手：`CRRL|ver|nonce_c|HMAC(psk,"client-hello"|nonce_c)` → `ver|nonce_s|HMAC(psk,"server-hello"|nonce_c|nonce_s)`；
- 会话密钥：`HMAC(psk,"master"|nonce_c|nonce_s)` 分方向派生 enc/mac；
- 记录：`type(1)|len(4)|密文|tag(32)`，密钥流 = `HMAC(k_enc, seq|block)`，tag 覆盖 `seq|type|len|密文`；
- **序号严格递增，不重同步**：篡改与重放都表现为「固定序号上 tag 失败」→ 断链；
- **心跳放在记录层**（`PING`/`PONG`），`relay_sub` 的订阅连接不需要业务保活，也不会收到假 `DELIVERED`。

**明确不是 TLS**：无证书/域名校验、无前向保密、无密钥轮换，未经独立密码学审计。
它能挡无凭证注册、窃听、篡改、重放；挡不住持钥内部人。

### 三道闸与一条启动策略
`--max-conns` / `--rate`+`--burst`（令牌桶，按时间补充而非每秒清零）/ `--idle-timeout`
（先 `PING`、两轮无应答才断）/ `--max-nodes` / `--handshake-timeout`；计数分别可归因
（`conn_cap_rejected` / `rate_limited` / `idle_closed` / `secure_rejected` / `bad_tag`）。
**绑非回环地址且无 PSK → 拒绝启动**并打印三种处置方式；明文只允许回环或显式 `--allow-insecure`。
客户端侧 `nat.relay_key`（64 hex 或 `file:`）写错会打字告警，**不静默退回明文**。

### 实测（两台真实主机：relay-222 公网侧 / relay-209 内网侧）
| 用例 | 结果 |
| :--- | :--- |
| `pair` @ 222 | PASS，ACK 0.6 / 0.7 ms |
| `latency` 30 轮 | 30/30，min 0.30 / p50 0.88 / max 1.41 / avg 0.85 ms |
| 跨实例加密投递 30 轮（60 帧） | 60/60，avg 0.4 ms，两端 `dropped=0` |
| 两实例互认 | 各 `peers=1`，`key_fp` 一致 |
| 反证 | 错误 PSK / 明文客户端均被拒并计入 `secure_rejected`；无密钥绑 `0.0.0.0` 拒绝启动 |

`cargo test --workspace --offline` **114 passed / 0 failed**（+14：`cr-nat::secure` 9 条、
`relay_server` 5 条含限额与认证）。

### 顺带修掉一个只在「派生实例 id」下才现形的缺陷
`--stats-file` 里的 `instance` 取自实例 id，而**派生 id 的 8 字节哈希可能含裸控制字符** →
计数文件不是合法 JSON，读取方每次都解析失败。T2 的 `--mode local` 因此报「中继服务端未就绪」，
而服务端其实一直正常（`relay.log` 里有 `RELAY_LISTEN`，端口也在听）。现在拆成两个东西：
协议身份仍用原始 16 字节，**展示/落盘一律走 `printable_instance`**（非可打印字节换成 `.`），
并加了一条断言「标签里不得出现控制字符」的用例。这类问题在公网监控上尤其坑：
服务活着、指标全空。

### 纠正：57100 映射其实**没**打通（上一轮写错了）
上一轮把 `112.5.32.34:57100 → 192.168.34.222:7100` 记成「已实测可用、中继公网化不必另找 VPS」。
**这是错的。** 实测：`54022 → 22` 在 27 ms 内建连成功（同一 CPE、同一内网主机），`57100` 超时；
222 上按**对端公网 IP** 抓包 **0 包**（按端口抓同样 0 包，但这次先排除了「端口被改写」），
`ufw` inactive、`iptables -S INPUT` 为 ACCEPT、`0.0.0.0:7100` 正在监听 —— 包根本没到 222。
所以是这条映射自身没有投递，与中继实现无关。**本轮因此没有任何跨公网时延数字**：
LAN 的 0.85 ms 不外推到公网，`nat-matrix.md §17.6` 与 `spikes.json` 均按此记录。

## [2.4.57] - 2026-11-15 — M2/W3.8：中继去单点化（多实例注册同步 + 客户端列表切换）

> 范围声明：**不改变任何 Spike 的 `status`**：S3 / S5 / S6 仍 `not_run`，`g2_claim` 未立、
> `spike_status_changed=false`；`evidence_valid_for_pass=false`，R-04 仍不判定。
> 本轮是**中继可用性工程**，没有产生任何 NAT 类别样本 —— 不是 S6 证据。

### 缺口是怎么被证明的（先有反证，再动手）
W3.7 的双实例基线留下一条**会红的用例**：A 注册在 `221:7100`、B 注册在 `209:7100`，
B 发给 A 的帧到了 209 只能 `dropped=1`。两个实例都活着、网络全通、帧格式正确 ——
说明缺口是**状态**（注册表每实例私有），不是网络。

### 新增三个实例间帧（仅中继之间使用，客户端不发）
- `0x05 PEER_HELLO`：负载 = 呼叫方 16B 实例 id，对端回 `ACK(node=应答方实例 id)`；
  一条 TCP 全双工，两个方向都能发。
- `0x06 PEER_SYNC`：负载 = n×16B 本地注册节点 id，**整份覆盖**语义 —— 掉线节点自然消失，
  不留「幽灵归属」（否则后续帧会被算成 forwarded 然后消失，调用侧只看到超时，比报错更难查）。
- `0x07 FORWARD`：负载 = 原始来源 id + 目标 id + 消息；收到后**只做本地投递、不再转发**，
  因此最多一跳，不存在环路。

### 服务端（`cr-nat::relay_server` / `coldrevive-relay`）
- 新增 `--peer <ip:port>`（可重复）与 `--instance-id <label>`；`--stats-file` 增加
  `peers` / `peer_nodes` / `forwarded` / `forward_dropped` 四个真实计数。
- 注册时**事件驱动**立刻推快照，另有 500 ms 周期兜底；同侪链路建立时双向立即对齐。
- 两边都配 `--peer` 时按实例 id 去重：保留已有链路、给新连接回 ACK 后关闭；拨号侧记住对端
  实例 id，只在对方链路消失后才重拨（否则会形成拆建抖动）。

### 客户端（`nat.relay` 变列表）
- `cr-core`：`nat.relay` 接受逗号/分号分隔的**列表**，新增 `nat_relays`；单值与原行为等价，
  非法项（无端口 / 端口非数字）直接丢弃、不造占位端点。
- `holepunch::relay_wake_any`：顺序尝试，返回**真实成功的那一个**地址；全部失败时聚合每条的
  真实错误（区分「连不上」「未注册」「对端没同步」），不压成一句「中继失败」。
- `relay_sub`：订阅断开后从**下一个**中继开始重试，单个实例宕机自动切换。

### 实测（两台真实主机、真实 LAN）
| 用例 | 结果 |
| :--- | :--- |
| 跨实例双向 30 轮（60 帧） | **60/60 送达**，RTT min 0.3 / max 0.6 / avg 0.3 ms，两端 `dropped=0`、`forward_dropped=0` |
| 快照自愈（持有者不重注册，重启 relay-209） | **PASS**，重启后首帧 1.0 ms 送达 |
| 反证：同机再起一个**不配 `--peer`** 的隔离实例 | **FAIL**（3 s 无 DELIVERED），该实例 `dropped=1 / forwarded=0 / peers=0` |

反证这条是故意留的：同一条用例在缺快照同步时立刻失败，才说明通过来自协议而不是环境。

### 测试
- `cargo test --workspace --offline` **100 passed / 0 failed**（+4）：
  `peer_sync_enables_cross_instance_delivery`、`peer_death_purges_its_nodes_from_registry_view`、
  `relay_list_is_parsed_and_junk_is_dropped`、`all_relays_down_reports_every_endpoint`。
- 修掉一处**偶发断言**：跨实例用例在客户端先拿到帧、计数尚未自增时失败 —— 与 W3.7 修 relay
  偶发测试同一套做法（有界等待），连跑 5 次稳定。

### 顺带纠正一条错误结论（映射其实一直是通的）
上一轮记录「`112.5.32.34:54022` 从外部连不通、DNAT 命中但 `0 packets captured`」——
**这是错的**。用户从 Mac 外网实连登录成功；我在 222 上按端口抓包同样看不到，改按**对端公网 IP**
过滤后抓到完整握手：`112.5.60.4.28698 > 192.168.34.222.22 [S] → [S.] → [.] → [F.]`，
`5 packets captured`。真实原因是 NAT **同时改写了源端口**，只按内网侧原端口 54022 过滤必然为空；
早期那次大概率映射尚未生效。DNAT 与 hairpin 本身都正常。
副作用：`57100→7100` 可直接作为 W3.9 的**真实公网中继入口**，不必另找 VPS。

## [2.4.56] - 2026-11-15 — M2/W3.7：真实 NAT 档位（条款①②首次同时成立）

> 范围声明：**不改变任何 Spike 的 `status`**：S3 / S5 / S6 仍 `not_run`，`g2_claim` 未立、
> `spike_status_changed=false`；`evidence_valid_for_pass=false`，R-04 仍不判定。
> 本轮第一次出现 `evidence_valid=true` 的**单类**报告 —— 只表示取证过程合法，不等于该类达标。

### 新增档位：`real-multi-nat`（两端都在真实 NAT 后面）
- 新增 `tests/spikes/s6/nat_lab/run_real_nat.py`：持有者在 **222**（`192.168.34.222` 经真实 NAT →
  `112.5.32.34`），请求者在 **A**（`192.168.1.210` 经真实 NAT → `112.5.60.4`），中继在 221 只做信令。
  NAT 由用户的网络设备完成，我方未写任何 iptables 规则。
- 驱动把「真实设备」当**可证伪条件**验证，不是当声明：映射端口 ≠ 内部端口、测试端口无静态映射、
  直连 attempt 的目标必须是公网 egress（否则判为 LAN 捷径并拒收）。

### 修复的两个「编造分类」缺陷
- **`CLASSIFY-SINGLE-SAMPLE`**：`classify_from_mappings` 只要有 **1 个**映射样本就返回 `FullCone`
  （注释写着「至少是非对称」）。一个目的地址在原理上分不清 EIM 与 APDM —— 于是真实 symmetric
  设备被判成 cone。改为要求 **≥2 个不同目的地址**，否则 `Unknown`；且只有确认 `FullCone`（EIM）
  之后才用过滤测试细分，避免「用一个假设换另一个假设」。
- **`CHANGE-REQ-NO-SOURCE-CHECK`**：`stun::change_request_answered` 只校验 tid，**不校验回包源地址**。
  公共 STUN 若忽略 `CHANGE-REQUEST` 而从原地址回包，会被算成「change-IP 有应答」→ 假 `full-cone`。
  改为：正对照要求 `src == addr`，change 测试要求 `src != addr`（回包必须来自另一个套接字）。
  新增回归测试 `filter_ignored_change_request_is_not_full_cone`、
  `single_destination_sample_is_unknown_not_cone`。

### 修复的偶发测试
- `relay_server::tests::routes_frames_between_two_real_tcp_clients` 实测约 **1/3 偶发失败**：
  客户端收到帧时服务端计数线程尚未自增，断言读到旧值。改为有界等待（`wait_until`），连跑 5 次稳定。
  偶发失败的测试等于没有测试。

### 实测（30 次/路径）
- 持有者侧现场分类：`symmetric`（Address/Port-Dependent Mapping）。
  手工报文实验确证：同一 socket（本地端口 50114）、目的端口均 3478、三个不同目的 IP →
  映射端口 `2051 / 2055 / 2050` 各不相同。
- `direct` **0/30**、`relay` **30/30**；中继增量 `delivered=108 / dropped=0 / rejected=0`。
- 抓包给出完整因果链：持有者出向回打 2 条（`192.168.34.222.19901 > 112.5.60.4.57238`），
  而 A 打向 `112.5.32.34:2053` 的包**入向 0 条** —— 被真实 NAT 丢弃。symmetric 下双向打洞
  原理上不可能成功，`0/30` 是**正确结论**而非缺陷。
- 报告：`status=OBSERVED`、`evidence_scope=real-multi-nat`、`evidence_valid=true`、
  条款①（两台物理主机 `['DESKTOP-DJQSC95','ubuntu23']`）与条款②（`nat_is_real_device=true`）均成立。

### 文档
- `tests/spikes/s6/nat-matrix.md` §15：档位差别、真实设备判据、假阳性分类的推翻过程、
  两个分类缺陷与修法、抓包因果链、以及「这一档没解决什么」。
- `spikes.json` 新增 `S6.observations.w37_real_nat_2026_11_15` 与 `m2_progress.scope_done.w37_real_nat`；
  `remaining_for_g2` 第 2 条更新为「类别覆盖仍 1/4 + 0 CGNAT」。

## [2.4.55] - 2026-11-15 — M2/W3.6c：修 NAT 分类缺陷 + 把跨主机数字做到可复现

> 范围声明：**不改变任何 Spike 的 `status`**：S3 / S5 / S6 仍 `not_run`，`g2_claim` 未立、
> `spike_status_changed=false`；实验室证据 `evidence_valid=false`，R-04 仍不判定。

### 修复的产品缺陷（Rust）
- **`CLASSIFY-CONE-SUBTYPE-FABRICATED`**：`coldrevive-cli nat probe` 在五类环境里**恒报 `restricted-cone`**。
  根因是 `classify.rs::classify_cone_subtype` 比较 **NAT 映射地址**与 **STUN 服务器 OTHER-ADDRESS**
  —— 两者语义无关，只要服务器返回 OTHER-ADDRESS 就必然判 restricted。这个字段此前是恒定值，不是测量。
  - `cr-nat/src/stun.rs`：补齐 RFC 3489 Test II/III —— 发 `CHANGE-REQUEST(0x0003)`，
    按**是否收到应答**判定（change-IP 有应答→full cone；仅 change-port 有应答→restricted cone；
    均无→port-restricted cone；正向对照都收不到应答→`Unknown`，**不可达不等于受限**）。
  - `classify.rs`：删除 `classify_cone_subtype`，改为显式 `FilterBehavior` + `classify_from_filter`，
    逼调用方先拿到测量值再下结论。
  - **修复中抓到的第二个坑**：过滤测试若复用映射探测的 socket，网关过滤集里已有备用地址，
    change-IP 回包被放行 → 受限锥被误判成 full cone。故新增 `filter_behavior_fresh()`（每次新建 socket）。
  - 修复后五类实测：`full-cone / restricted-cone / port-restricted-cone / symmetric / port-restricted-cone(cgnat)`，
    与 `nat_behavior.py` **5/5** 一致（此前五类同一个值）。

### 修复的 harness 缺陷（`tests/spikes/s6/nat_lab/`）
- **`RESET-NEVER-RAN`**：`reset.sh` 在 Windows 上用 `write_text()` 落盘变成 CRLF，远端 bash 直接报语法错误
  （`syntax error near unexpected token`，指向 `do\r`），**ipset 复位从未执行** → 第一次开洞后余下试验全吃老本。
  改为 `write_bytes()`；每轮记录 `reset_exit_code`，跑完统计非 0 并告警（现五类全为 0）。
- **`ADDR-POOL-COLLISION`**：物理地址按类写死 `.241`~`.245`，但其中三个**已被家庭 LAN 上的真实设备占用**
  （A 的 ARP：`.242 → f4-74-88:f2:c6:ee`、`.244 → f4-74-88:f0:12:36`、`.245 → f4-74-88:f0:0d:26`，
  而 `.241/.243 → 00:0c:29:b6:ab:11` 才是 221）。punch 打到了别人的机器；原预检只 `ping`，
  "有应答"就算通。改为动态挑空闲地址，并在 `up` 之后**校验应答者身份**：A 对该地址的 ARP MAC
  必须等于 221 `ens160` 的 MAC，否则直接 `SystemExit`。
- **`LIFECYCLE-SILENT-DEGRADE`**：物理口 STUN 只用 `nohup` 起、没有 `setsid`，启动它的 ssh 一退出
  进程就被带走 → 分类 `undetermined`、`nat_type=unknown`，而矩阵照跑、候选只剩中继、
  成功率依然"漂亮"。改为 `setsid nohup`；驱动在 A 上**真发 STUN binding request** 验证双端点；
  分类前做 **netns→STUN 收敛等待**（实测单层约 10s，双层 CGNAT **28.8s**）；
  `undetermined` 或缺 `mapped_address` 一律中止，不产出矩阵。

### 最终实测（30 次/路径，`reset_exit_code` 全 0，地址身份已校验）
| 持有者（221 netns） | 分类复测 | `direct` | `relay` |
| :--- | :--- | :--- | :--- |
| full-cone | full cone | **30/30** | 30/30 |
| restricted-cone | restricted cone | **30/30** | 30/30 |
| port-restricted-cone | port-restricted cone | **30/30** | 30/30 |
| symmetric | symmetric | **0/30** | 30/30 |
| cgnat（双 NAT） | port-restricted cone | **30/30** | 30/30 |

数字与 2.4.54 一致，但 2.4.54 那批是在三个缺陷同时存在时拿到的（曾一次读成 30/30、
一次读成 0/30），**不可复现**。本轮价值在于：这些数字现在有独立反证（分类复测 5/5、
ARP 身份 MAC、`reset_exit_code=0`、STUN 真实往返）。

### 判定未变
`evidence_scope=cross-host-lab`、条款① `true`、条款② `false`、`evidence_valid=false`；
**S6 仍 `not_run`，R-04 ≥70% 仍不判定**。修的是产品缺陷与取证可信度，不是门禁结论。

## [2.4.54] - 2026-11-15 — M2/W3.6b：跨主机档位落地（条款①由 ❌ 变 ✅）

> 范围声明：**不改变任何 Spike 的 `status`**：S3 / S5 / S6 仍 `not_run`，`g2_claim` 未立、
> `spike_status_changed=false`；无 Rust 代码改动；实验室证据 `evidence_valid=false`。

### 背景
上一版把判据拆成两条独立条款后，仍有一句话容易被误读：「实验室是同机两个 netns」。
真正的问题不是「机器不够」，而是**一次测量里的两个端点都跑在 221 内部**。
把请求者挪到 A（192.168.1.210）就能让条款①成立，本轮做到了。

### 实现（`tests/spikes/s6/nat_lab/`）
- **`link_physical.sh`**：把某一类网关的 WAN 侧「挂」到物理 LAN 上
  （地址池 `192.168.1.241`~`.245`），由 221 在 `ens160` 上 **proxy ARP** 代答；
  网关侧补 `/24` 路由、把实验机/对端钉成静态邻居、插入指向物理地址的 SNAT
  （对称型保留 `--random-fully`）。`down` 精确删回，不留残留规则。
  - 为什么不用更直觉的方案：A 是**非管理员**账户（改不了路由表）；
    macvlan 在 VMware vSwitch 下 100% 丢包（拒绝混杂模式/伪传输），已实测排除。
- **`run_cross_host.py`**：在 A 上运行，ssh 编排 221 侧（链路 → 物理口 STUN →
  中继 → 持有者 → 通告），本机起请求者，再调 `nat_matrix.py --evidence-scope cross-host-lab`。
  `nat wake` 的直连/中继命令都在 A 上执行，**测量姿态与真实使用一致**。
  远端命令一律经 `ssh ... bash -s` 从 stdin 送入，避免 argv 被远端 shell 二次拆分。
- 分类与可达率必须出自**同一条链路**：每次 `up` 后先在物理链路上用 `nat_behavior.py`
  重测一次分类，报告一并落盘；分类报告取不回时直接失败，不用占位路径骗过 attestation。

### 实测（30 次/路径，`delivered=120 / dropped=0`）
分类 5/5 与建网声明一致（full/restricted high、port-restricted/cgnat medium、symmetric low）；
`direct`：full-cone / restricted-cone / port-restricted-cone / cgnat **均 30/30**，
symmetric **0/30**（无 DNAT、映射随目的变，只能中继）；`relay` 五类 **30/30**。
与 W3.6（同机 netns）逐行一致 —— 说明 30/30 不是「同内核」的假象。
报告：`evidence_scope=cross-host-lab`、`hosts=['DESKTOP-DJQSC95','ubuntu']`、
条款① `true`、条款② `false`、`evidence_valid=false`。**R-04 仍未判定**。

### 顺带发现的产品缺陷（已登记，另单修）
`coldrevive-cli nat probe` 在五类环境里**全报 `restricted-cone`**（含 full-cone）。
根因在 `cr-nat/src/classify.rs::classify_cone_subtype`：它比较 **NAT 映射地址**与
**STUN 服务器 OTHER-ADDRESS**，两者语义无关，只要服务器给 OTHER-ADDRESS 就恒判 restricted。
该字段此前一直是恒定值而非测量结果。已写入 `spikes.json` 的 `remaining_for_g2`，
明确「修复前不得用 `nat_type` 做策略决策」。

## [2.4.53] - 2026-11-15 — M2：S6 判据拆成两条独立条款 + 显式证据层级

> 范围声明：**不改变任何 Spike 的 `status`**：S3 / S5 / S6 仍 `not_run`，`g2_claim` 未立、
> `spike_status_changed=false`；无 Rust 代码改动（workspace 仍 17 crate + 5 bin）；
> `cargo test --workspace --offline` **88/88**。

### 背景
W3.5②/W3.6 的实验室证据被标 `evidence_valid=false`，容易被读成「环境不合格」，
进而被误解为「再加一台主机就能判 S6」。事实上判据是**两条互相独立**的条款：

| # | 条款 | 实验室现状 | 加主机能否解决 |
| :--- | :--- | :--- | :--- |
| 1 | 每类环境在**两台不同物理主机**上各启动 `coldrevived`（`measurement_protocol[0]`） | ❌ 同机两个 netns | ✅ |
| 2 | `real_nat_evidence`：**真实 NAT 设备**的可核查证据 | ❌ iptables/conntrack 合成 | ❌ 永远不能 |

再加一条更根本的限制：**一个网络出口只对应一个真实 NAT 类别**，四类 + CGNAT 需要 ≥4 个不同出口，
与主机数量正交。所以 S6 是**环境数量受限**，不是主机数量受限。

### 实现（`tests/spikes/s6/nat_matrix.py`）
- 布尔 `lab_mode` 拆成 `--evidence-scope`：`real-multi-nat`（唯一可判 pass 的层级）/
  `cross-host-lab`（合成 NAT + 两端不同主机）/ `vm-namespace-lab`（同机 netns）。
  `--lab-mode` 保留为后者的兼容别名，两者同时使用直接报错。
- 报告新增 `evidence_clauses`（逐条 `satisfied` + 观测到的 host 列表）与
  `evidence_invalid_reasons`（直接写明缺哪条）；`evidence_valid` 仅当
  `scope=real-multi-nat` 且条款全满足时为 `true`。
- 「两台主机」的判定改为比 `machine_ids` 的 **host 部分**（`<ns>@<host>`）：同机两个 netns
  会给出两个不同字符串但 host 相同，不算两台主机 —— 这正是旧布尔值能掩盖的地方。
- 层级必须**双向**自报：`real-multi-nat` 遇到 attestation 声明 `lab_mode=true`/弱层级即 `BLOCKED`；
  弱层级则要求 attestation 显式声明同名 `evidence_scope`，单侧声明即拒收。
- `run_matrix_lab.py` 相应支持 `--scope` / `--holder-host` / `--requester-host`，
  attestation 补 `evidence_scope` 与 `nat_is_real_device`（本驱动恒 `false`，永不给 `evidence_valid=true`）。
- 自检 `nat_matrix.py --self-test` 从 10 增至 **16/16**：新增「同机多 netns 不算两台主机」
  「两主机可识别」「合成 NAT 永不等于真实设备」「缺条款能逐条报出」「旧 `--lab-mode` 映射」
  「冲突参数被拒」。

### 复核
`restricted-cone` 持有者 + `port-restricted-cone` 请求者，30 次/路径：`direct` **30/30**、
`relay` **30/30**、中继 `delivered=120 / dropped=0`；报告 `evidence_scope=vm-namespace-lab`、
`hosts=['ubuntu']`、两条条款均 `false`。数值与 W3.6 完全一致 —— 层级改造只改「为什么不能判 pass」
的表述，未改测量口径。

### 文档
- [nat-matrix.md §13](tests/spikes/s6/nat-matrix.md)：双条款拆解、三层证据、三条闭合路线
  （A 真实设备级联 / B 跨主机实验室 / C 公网 ≥8 节点实测），推荐 C 为主线、A 为最小合规闭环。
- `nat_lab/README.md` 与 `setup_nat_lab.sh` 头注释同步更正（原文「以不同 machine_id 代替两台物理主机」
  表述不准，已改为双条款口径）。
- `spikes.json` 新增 `S6.observations.criteria_decomposition_2026_11_15` 并补 `remaining_for_g2`。

## [2.4.52] - 2026-11-15 — M2/W3.6：双向打洞 / rendezvous

> 范围声明：**不改变任何 Spike 的 `status`**：S3 / S5 / S6 仍 `not_run`，`g2_claim=false`、
> `spike_status_changed=false`；`PROTOCOL_VERSION=1`、T0 golden 与 CLI v1 契约未改；
> workspace 仍 17 crate + 5 bin、std-only 零第三方依赖；`cargo test --workspace --offline` **88/88**。

### 背景
W3.5② 实验室实测把 S6 的 `direct` 0/30 定位到了**具体一步**：`cr-nat::holepunch` 是单向的 ——
请求侧发 `NAT_PUNCH`，持有者只回 `NAT_PUNCH_ACK`、从不主动回打。受限锥及以上的网关在收到
**首个入向包**时既无 conntrack 条目、过滤集里也没有对方地址，直接丢弃，所以无论请求侧发多少次，
持有者都看不到，也就永远回不了 ACK。也就是说：不是 NAT 不可穿透，而是缺「持有者先回打」。

### 实现
- `NAT_PUNCH` 帧可选携带 `"back":"ip:port,..."`（请求侧自身映射候选）。解析只接受**可路由 IPv4**
  （环回/未指定/组播/广播/文档地址一律丢弃），去重并硬性限制 `MAX_BACK_TARGETS=4`、每目标 2 帧
  —— 该帧无需鉴权，不设上限等于把持有者的唤醒端口变成放大器。
- 持有者收到带 `back` 的帧后，**从唤醒监听 socket 主动回打**（`punch_back_to`），打开自身 NAT
  过滤项；回打帧**不再携带 `back`**，避免链式放大。加 `BackRate` 令牌桶限流。
- 请求侧一次唤醒**只绑定一个 socket**：同一 socket 上 STUN 取自身映射（`self_mapped`）→ 向直连
  候选「热身」一包 → 经**中继**把带 `back` 的帧送给持有者 → 同一 socket 上逐候选打洞 + 唤醒。
  单 socket 是硬要求：映射端口随本地端口走，换 socket 报出去的 `back` 必然是错地址
  （W3.3 每候选新绑 `:0`，这就是它无法双向的原因）。
- `WakeAttempt` 新增 `peer_punched`，与 `punch_acked` **分开记**：区分「对端已回打但我方收不到
  ACK」（过滤方向不对称）与「完全没有回打」。这是排查 direct 失败的关键证据，不能并成一个布尔。

### 实验室复测（30 次/路径，真实 CLI/UDP/HTTP）
先修正了一处**对照错误**：W3.5② 有 4/5 组的请求方在 `lan-symmetric` 后面，对称 NAT 的映射随
目的地址变化，`back` 永远报不准 —— 那是任何打洞方案都救不了的组合，与「持有者侧过滤」混在同一
组里测等于两个变量搅在一起。本轮请求方固定为锥形 NAT。

| 组 | 持有者 | 请求方 | `direct` W3.5② → W3.6 | `relay` |
| :--- | :--- | :--- | :--- | :--- |
| a | full-cone | port-restricted | 30/30 → **30/30** | 30/30 |
| b | restricted-cone | port-restricted | **0/30 → 30/30** | 30/30 |
| c | port-restricted-cone | full-cone | **0/30 → 30/30** | 30/30 |
| d | symmetric | port-restricted | 0/30 → **0/30** | 30/30 |
| e | cgnat（双 NAT） | port-restricted | **0/30 → 30/30** | 30/30 |
| f | restricted-cone | symmetric | — → 30/30 | 30/30 |

六轮 `dropped=0` / `rejected=0` / `reset_err=0`。**symmetric 持有者 0/30 是正确结果**：该网关无
DNAT、映射随目的地址变化，广告的 STUN 映射端点不是入向可达端口，其回打映射端口请求方又无从得知，
只能走中继（与 WebRTC 实践一致）。f 组 30/30 也不是异常：`restricted-cone` 的过滤是
address-dependent（ipset 键不含远端端口），回打到错误端口也按 IP 放开；`port-restricted-cone`
不使用 ipset、只靠 conntrack（ip+port），二者语义确实不同。

### 新增的依赖（写进 `remaining_for_g2`）
双向打洞目前**以中继作为信令通道**。中继不可用时 `back` 送不出去，直连会整体退化回「只有全锥可通」。
本轮实测到过一次（上一轮 relay 未真正监听，`connect refused`）：整轮 `relay` 0/30，`direct` 从
30/30 掉到 5/30。因此**中继不是兜底，而是双向打洞的前置依赖**。

### 测试方法与工具修正（都由实测暴露）
- **限流器会伪装成业务失败**：旧参数（容量 20 / 每秒 2）在连续试验里周期性打空，表现为 direct
  固定 `23/30` 且**失败序号呈严格周期 3**（12/15/18… 与 13/16/19…）。参数改为 64 / 16，并加两条
  回归用例：旧参数必须复现打空、新参数在 200 次连续唤醒下不得打空。
- `nat_matrix.py` 新增 `--inter-trial-reset`：每次试验前清空网关 ipset（`CONTACTED` 有 600s 超时，
  不复位则第 1 次开洞后余下 29 次吃老本，成功率虚高）；复位失败记入 `reset_error` 不静默忽略。
- `nat_matrix.py` 记录 CLI `stderr`：CLI 非 0 退出且 stdout 为空时，只留「CLI 无输出」会让原因不可追溯。
- `run_matrix_lab.py`：中继「就绪」改用**真实 TCP 连接**校验（`wait_tcp`，失败重启一次，仍失败则
  明确退出）；修正 `pkill` 模式（`^coldrevive-relay` 匹配不到绝对路径，会泄漏上一轮进程并污染下一轮）；
  跑末打印 relay 计数；冒烟输出补充 notes/attempts。
- T2-07 的中继计数改为**按增量归因**（`delivered-基线 >= 2`）：W3.6 起 `nat wake` 会经中继发一次
  回打信令（额外 2 帧送达），累计值无法再区分「中继唤醒本身成功」与「信令流量」。

### 校验
- `cargo test --workspace --offline` **88 passed / 0 failed**（cr-nat 新增 8 个用例：back 帧编解码、
  非法/不可路由地址拒绝、候选数上限与去重、限流放行与洪泛拒绝、旧参数打空回归、持有者回打真实
  回环、`back` 端到端送达）。
- [T2](tests/e2e/t2/README.md) `--mode local` **10/10 PASS**（真实中继服务端，`evidence_valid=false`）。
- 实验室六份报告：`tests/spikes/s6/reports/w36-20261115/`（gitignored 本地证据）。
- 机读：`spikes.json` 的 `S6.observations.w36_bidirectional_punch_2026_11_15` 与
  `m2_progress.scope_done.w36_bidirectional_punch`；证据边界仍 `lab_mode=true` /
  `evidence_scope=vm-namespace-lab` / `evidence_valid=false`。

## [2.4.51] - 2026-11-15 — M2/W3.5②：单台 Ubuntu VM 内建成四类 NAT + CGNAT 实验室并实测

> 范围声明：本轮**不改变任何 Spike 的 `status`**：S3 / S5 / S6 仍 `not_run`，`g2_claim=false`、
> `spike_status_changed=false`；`PROTOCOL_VERSION=1`、T0 golden 与 CLI v1 契约未改；workspace 仍
> 17 crate + 5 bin、std-only 零第三方依赖。实验室证据一律 `evidence_valid=false`、
> `evidence_scope=vm-namespace-lab`（同一台物理主机内的两个 netns，不满足「两台物理主机」条款）。

### 背景
- W3.5① 审计结论：S6 需「四类真实 NAT（含 CGNAT）可达率矩阵」，而现场只有 1 类真实 NAT 出口、
  无 CGNAT、且本机无管理员权限与 `tc/netem`。用户提供了一台可 SSH 操作的 Ubuntu Server
  （192.168.1.221）用于搭建实验环境。
- 目标不是「用模拟数据凑一个 pass」，而是**用内核级真实 NAT 语义**把四类 NAT 与双 NAT CGNAT
  在一台主机内复现，让分类探针测到的行为是网关真实行为（conntrack / SNAT / DNAT / ipset 匹配），
  从而把 `nat_matrix.py` 的完整执行路径真正跑通一遍，暴露接线与信令缺陷。

### 新增：`tests/spikes/s6/nat_lab/`
- `setup_nat_lab.sh`：`ip netns` + veth + bridge 建 5 套拓扑（full-cone / restricted-cone /
  port-restricted-cone / symmetric / cgnat 双 NAT）；子命令 `up` / `status` / `stun-start` /
  `stun-stop` / `down`。NAT 由 `iptables-legacy` 在网关 netns 内做**真改写**：
  SNAT 映射、`--random-fully` 对称型、`ipset hash:ip,port,ip` 做 RFC 4787 受限锥跟踪、
  全锥/受限锥网关另加**保留端口 1:1 DNAT**。统一 `iptables-legacy` 是因为目标机默认 `nf_tables`
  而 `ipset` 只在 legacy 下工作。
- `local_stun.py`：本地 RFC 5389 Binding + RFC 3489 `CHANGE-REQUEST`（flags 2/4/6）/
  `CHANGED-ADDRESS` / `OTHER-ADDRESS` STUN 服务器。**必需**：若用公网 STUN，VM 自身出口 NAT
  会叠在待测 NAT 之上，测到的是两级复合行为。`--self-test` 8/8 PASS。
- `run_matrix_lab.py`：在 netns 内拉起真实 `coldrevive-relay` + 两个真实 `coldrevived`
  （holder 在 A / requester 在 B），取 `/nat/advertise`、用真实 `sha256(file)` 做 `file_hash`，
  再驱动 `nat_matrix.py --mode real --lab-mode` 跑 30 次 × 2 路径 × 5 环境。
- `README.md`：拓扑图、运行流程、2026-11-15 结果、证据边界、8 条排障记录。

### 方法论修正（两处关键，否则测量无效）
1. **锥形网关必须做保留端口 DNAT**：只做 SNAT 时，外部主动入向包在网关上无 conntrack 条目，
   全锥与端口受限会得到相同的「不可达」，分类产生假阴性。加 1:1 DNAT 后两类才可区分。
2. **STUN 必须从「收到请求的那个地址」回包**：原先恒从主地址回应，导致发往备用地址的请求其回包
   元组与出向 conntrack 不匹配，被端口受限/对称网关当 unsolicited 丢弃 —— 实测会把「可达」
   误判成「不可达」，5 类里 3 类降级为 `undetermined`。已修复并加回归自检
   `answers_from_receiving_alt_ip`。

### 实测结果（真实 CLI / UDP / HTTP / STUN 观测，30 次/路径）
- 分类（`nat_behavior.py`，指向本地 STUN）：full cone `high`、restricted cone `high`、
  port-restricted cone `medium`、symmetric `low`、cgnat→port-restricted cone `medium`。
  **5/5 环境分类与建网时声明的类型一致**，这是实验室保真度的反证。
- 可达率（`nat_matrix.py --mode real --lab-mode`）：`direct` 仅 full-cone holder **30/30**，
  其余四类（restricted-cone / port-restricted-cone / symmetric / cgnat）**0/30**；
  `relay` **5/5 环境均 30/30**。
- 结论（工程性，非「NAT 不可穿透」）：现行 `NAT_PUNCH` 是**单向**的 —— 请求侧向持有者候选地址
  发探测包，持有者不回打，因此只有全锥 holder（入向不受过滤）能通。受限锥及以上需要
  **双向打洞 / 会合（rendezvous）**：双方各自先向对方出向发一包，打开自身过滤项后再互发载荷。
  中继回退覆盖全部 5 类环境，验证 W3.4 的中继路径在 NAT 侧的兜底是有效的。

### 门禁纪律
- `nat_matrix.py` 新增 `--lab-mode`：强制 `evidence_valid=false`、`evidence_scope=vm-namespace-lab`；
  且**双向校验** attestation 的 `lab_mode` —— 传了 `--lab-mode` 而 attestation 未自报 → `BLOCKED`；
  attestation 自报 `lab_mode=true` 却未传 `--lab-mode` → 同样 `BLOCKED`。禁止把「同机两个 netns」
  静默上报成「两台物理主机」。
- `nat_matrix.py` 支持 `additional_environment`（cgnat）作为必测附加环境，不再因不在
  `required_classes` 四类里而报「未知 NAT 类别」。
- `spikes.json` 只新增 `observations` / `scope_done`，未改任何 `status`；`matrix_coverage` 仍
  记 `lab 5/5（非真实）`，**不折算成 S6 覆盖率**。

### 修复
- `stun_probe.py::default_gateway()` 原先只实现 Windows `route print`，在 Linux 返回空串导致
  CGNAT 启发式失效；补 `ip route show default` 回退。
- `nat_behavior.py` 增加 `--mapping-servers` / `--filter-servers` / `--label`，报告新增
  `environment_label` / `stun_endpoints`；`host.os` 改用 `platform.system()`。

## [2.4.50] - 2026-11-14 — M2/W3.5①：S3/S5/S6 条件审计 + 符合条件的真实实验 + S6 矩阵执行协议

> 范围声明：G1 已通过、M2 进行中。本轮**不改变任何 Spike 的 `status`**：S3 / S5 / S6 仍 `not_run`，
> `g2_claim=false`、`spike_status_changed=false`、`matrix_coverage=0/4`；`PROTOCOL_VERSION=1`、
> T0 golden 与 CLI v1 契约未改；workspace 仍 17 crate + 5 bin、std-only 零第三方依赖。
> 所有新增证据一律标 `evidence_valid_for_pass=false`，只作附件；结论仍由实验执行人手工回填。

### 背景
- 用户指令：「继续按下一步计划推进，s3/s5/s6 如果符合测试条件的可以进行实验测试」。
- 先做条件审计再决定可测范围，避免把不可测环境虚判成通过或把本地模拟当 pass 证据。

### 条件审计（新增 `tests/spikes/probe_conditions.py`）
- 只读真实探针：4 台公网 STUN、真实 `coldrevive-nat --json`、B 机 `/health` + `/cli`、跨主机 HTTP RTT、
  IPv6 全局地址/路由、虚拟交换出口、 shaping 工具与管理权限。`--self-test` 5/5 PASS。
- S3 / S5 / S6 均判 **`PARTIAL_RUNNABLE`**：可跑附件实验，不可判 pass。
- 实测约束：无管理员权限；`tc/netem/ipfw/dummynet/wondershaper` 均不存在；WSL 被安全策略禁止且未绕过；
  无全局 IPv6（`WinError 10051`）；`vEthernet (Default Switch) 172.26.96.1` 出站不可达，不能当作第二类独立真实 NAT；
  仅 1 类真实 NAT 出口、未检出 CGNAT；跨主机 100 Mbps 未整形 LAN（HTTP RTT 中位 4.32 ms / p95 31.74 ms）。
- 修正取证口径：条件探针每次 STUN 新建 socket，4 个不同映射端口**不得**据此推断对称型 NAT；分类须由
  `nat_behavior.py` 的单 socket 对照完成。IPv6 / 虚拟出口由真实 socket 探测，不再硬编码。

### S3：真实 M1 双栈跨主机集成（新增 `tests/spikes/s3/m1_dualstack_integration.py`）
- 两台不同物理主机上的真实 `coldrevived` 进程；A 由 harness 临时拉起并在结束时清理，B 常驻未改动；
  私有 LAN 真实 UDP/HTTP/CLI 观测，`nat` 段缺省（M1 原行为），未向公共 KAD/DHT 发包。
- **17/17 PASS**：`inspect kad` 双栈 ID 32/40 hex 且互不相交；独立 Python 复刻 `cr-kad` 算法与 Rust
  守护进程真实输出逐项一致；真实 KAD key 命中 `hit_node=A/count=1`；160-bit DHT 形状 ID、128-bit ID
  当 key、截断 key、畸形帧一律不命中；两线程并发 120 帧 `misrouted=0`、无丢包；跨主机 B 侧并行亦无串扰；
  压测后 `published_keys` 未被污染。`--self-test` 5/5 PASS（纯逻辑）。
- 缺口仍在：G3 兼容性矩阵、真实 Mainline DHT 公网加入、跨进程 RSS 增量。

### S5：真实 uTP 栈 5 轮复跑
- `tests/spikes/s5/utp/reports/s5-utp-real-20261114.json`：默认 `0.2` 档前台劣化 **14.78%**（不达标），
  `0.05` 档 4.58%（单轮 5.11% 越线）、`0.02` 档 3.23%（全轮达标）；`librqbit-utp 0.7.0` 仍仅 CUBIC、无 LEDBAT。
- 结论与 2026-11-13 一致，但瓶颈仍为本机用户态令牌桶整形，不是 `tc/netem` 或实验室拥塞链路，**不能支撑 pass**。

### S6：真实 NAT 行为精细分类 + 四类矩阵执行协议
- 新增 `tests/spikes/s6/nat_behavior.py`（`--self-test` 12/12 PASS）：RFC 5389 同 socket 多目的映射 +
  RFC 3489 Test II/III `CHANGE-REQUEST` flags 2/4/6 + **独立对照 socket** + 服务器能力正向对照 +
  遍历全部 A 记录 + 有界重试抗偶发丢包。实测唯一出口为
  `Endpoint-Independent Mapping + Address-and-Port-Dependent Filtering` / **port-restricted cone**（medium）。
- **关键方法论修复**：备用地址直连对照若与过滤测试共用 socket，会自行打开 NAT 放行项，实测曾把
  port-restricted cone 误判成 full cone；现强制两个真实不同本地端口。另修复 STUN 帧长度断言、
  `parse_attrs` 切片误用、Python 关键字语法错误、公共 STUN 死节点 DNS 轮询导致的假 `undetermined`。
- 新增 `tests/spikes/s6/nat_matrix.py` + `nat_matrix_config.json`（`--self-test` 10/10 PASS）：四类真实 NAT
  + CGNAT、每类每条路径 ≥30 次真实唤醒、成功只认真实 `WAKEUP_ACK`、必须由实验执行人签 attestation、
  超时/缺字段/非 JSON 一律计失败。`--dry-run` 退出码 2 / `status=BLOCKED`；`--aggregate`
  `matrix_complete=false`、`aggregate_reachability=null`。**即便证据齐备脚本也不置 pass。**
- `coldrevive-nat` 的 `full-cone` 是未做过滤细分的保守标签，与本次带对照的 `port-restricted cone` 不矛盾。
- 分类样本**不是可达率**；`matrix_coverage` 仍 `0/4`（含 CGNAT 0/1），R-04 ≥70% 阈值未判定，G2 未达标。

### 验证（全部真实观测）
- `cargo test --workspace --offline` **80 passed / 0 failed**；`cargo build --release --workspace --offline` OK。
- **T0 `--mode real` 21/21 PASS、`evidence_valid=true`**（需设置三个真实二进制环境变量）；
  T1 `--self-test` 11/11；T2 `--self-test` 3/3 + `--mode local` **10/10 PASS**（`evidence_valid=false`）；
  `scripts/build_docs.py --check` 36 份 Markdown 校验通过。
- 机读口径见 `tests/spikes/spikes.json` 的 `m2_progress.scope_done.w35_*`、
  `g0_review.w35_experiments_2026_11_14` 与 S3/S5/S6 的 `observations.w35_*`。

## [2.4.49] - 2026-11-14 — M2/W3.4：生产中继服务端 + 持有者订阅，T2 换用真实中继

> 范围声明：G1 已通过、M2 进行中。本轮**不改变任何 Spike 的 `status`**：S3 / S5 / S6 仍 `not_run`，
> `g2_claim=false`、`spike_status_changed=false`；`PROTOCOL_VERSION=1`、T0 golden 与 CLI v1 契约未改。
> 新增二进制 `coldrevive-relay`，workspace = 17 crate + **5 bin**，仍 std-only 零第三方依赖。

### 背景
- `m2_progress.remaining_for_g2` 记录：「生产中继服务端 + 持有者侧中继订阅线程（当前 `cr-nat::relay` 只有客户端侧）」。
- W3.3 的 T2-07 靠本地 `MockRelay` **在服务端里直接发 UDP 到 A 的唤醒端口**才跑通，服务端掺了业务逻辑，不可部署。

### 实现
- **`cr-nat/src/relay_server.rs`（新）**：与业务无关的 TCP 帧路由器。`REGISTER` 校验负载回显并回 `ACK`；
  `RELAY` 按负载前 16 字节目标 id 查注册表投递 `DELIVERED`（`node` = **真实发送方**注册 id）；
  目标未注册 / 写入失败**只计 `dropped`，绝不伪造应答**；1 MiB 单帧上限；读写分离使 reader 阻塞不阻碍投递。
  `RelayStats` 提供 `connections / registered / delivered / dropped / rejected` 真实计数。
- **`coldrevive-relay`（新 bin）**：独立部署入口 `--listen / --stats-file / --max-frame`；
  端口 `0` 时首行回显 `RELAY_LISTEN=`，stats 每 200 ms 落机读 JSON。
- **`coldrevived/src/relay_sub.rs`（新）**：`nat.enabled && nat.relay` 时启动持有者订阅；把 `DELIVERED`
  负载经**真实 UDP** 送进本守护进程唤醒端口，取回**真实 `WAKEUP_ACK`** 后 `RELAY` 回投来源 id。
  UDP 无应答时跳过、不合成。`nat` 段缺省时完全不启动（T0/T1 行为不变）。
- **`relay.rs` / `holepunch.rs`**：新增 `ephemeral_session_id()`，请求侧用一次性注册 id，避免与自身持有者
  订阅连接抢占服务端 `HashMap<node_id, conn>` 表项；新增 `RelayConn::set_read_timeout` / `write_raw`。
- **`tests/e2e/t2/`**：`--mode local` 改用真实 `coldrevive-relay` 子进程替换 mock 桥，端口从 stats 回读；
  T2-07 断言改为读取真实服务端计数（`delivered >= 2 && dropped == 0`）+ A 的真实 ACK。
  `--self-test` 仍用纯 wire mock 验帧编解码、不启二进制。报告新增 `environment_detail.relay_stats`。

### 验证（全部真实观测）
- `cargo test --workspace --offline` **80 passed / 0 failed**（W3.3 基线 74 + 新增 6：`relay_server`
  4 项真实 TCP 路由 / 未注册丢弃 / 畸形注册拒绝 / 帧长上限，`relay_sub` 2 项）。
- **T2 `--mode local` 10/10 PASS**；T2-07 实测 `connections=3 / registered=3 / delivered=2 /
  dropped=0 / rejected=0`、`ack_status=OK`。报告
  `tests/e2e/t2/reports/t2-20261114T121322Z-local.json`（`evidence_valid=false` /
  `s6_status=not_run` / `g2_claim=false`）。`--self-test` 3/3。
- **T0 `--mode real` 回归 21/21 PASS、`evidence_valid=true`**（T0 配置无 `nat` 段，订阅线程不启动）；
  T1 `--self-test` 11/11；`scripts/build_docs.py --check` PASS。

### 口径同步
- `spikes.json`：`m2_progress.scope_done.w34_relay`、validation 全量回填、
  `S6.observations.w34_relay_2026_11_14`（`evidence_valid_for_pass=false`、`matrix_coverage=0/4`、
  `s6_status_after=not_run`）、`t1_progress.design_gaps[0]` 补 W3.4 补记；`remaining_for_g2` 中
  「生产中继服务端」改为「中继公网生产化收尾（TLS / 鉴权 / 多实例 / 限流 / 时延）」。
  **S1–S8 的 `status` 一个未改。**
- ROADMAP §6 把「下一步计划 W3.4」更新为已落地；T2 README 增 §8 真实帧流图；
  README / TEST / tests README / spikes README 同步 5 bin、80 单测、真实中继口径。

### 剩余（G2 出口，未结案）
S6 四类真实 NAT（含 CGNAT）可达率矩阵、S3 双栈 + G3 矩阵、S5 真实拥塞链路 ≤5%、
M2 其余骨架 crate、BT/TCP 打洞、中继公网生产化（TLS / 身份鉴权 / 多实例注册同步 / 限流 /
跨公网时延）。**真实中继服务端不是真实 NAT 环境，`S6.status` 仍 `not_run`，G2 未达标、未宣称。**

## [2.4.48] - 2026-11-14 — 门禁口径全量收尾 + T2 编号消歧 + 文档站重建

> 范围声明：纯文档 / 构建口径收尾，**不含新的实验结论**。
> S3 / S5 / S6 仍 `not_run`，G2 未达标、未宣称；`spikes.json` 中任何 Spike 的 `status` 一律未改。

- 补齐 2.4.47 未覆盖的历史口径：`docs/05-quality/TEST.md`、`tests/README.md`、
  `tests/spikes/README.md`、`tests/e2e/t0/README.md`、`tests/e2e/t1/README.md`、
  `docs/02-architecture/TECH-STACK.md`、`docs/README.md`、`CHANGELOG.md` 旧版本条目。
  一律「保留 2026-11-13 原文 + 追加 2026-11-14 更正 / 历史标记」，不静默改写决策演化链。
- `tests/e2e/t1/README.md` 新增 §2.1 记录 G1 证据（11/11、`evidence_valid=true`、报告与 STUN 探针路径），
  §5 改为「接线层已关闭 / 跨 NAT 与 S6 层仍开放」；`tests/e2e/t0/README.md` §5 第 9 项 ⛔ → ✅。
- **T2 编号消歧**：`tests/e2e/t2/` 是 M2/W3.3 本地接线套件，与 [TEST §2](docs/05-quality/TEST.md)
  规划编号「T2 = 公网 ≥8 台不同网络节点」不是同一环境；已在 TEST / tests README / T1·T2 README 写明，
  防止把「本地接线 PASS」误读为「公网可达率证据」。
- `scripts/build_docs.py` 的 `EMBED_EXTRA` 增加 `tests/e2e/t2/README.md`；重生成
  `docs/index.html` 的 `DOCS_PAYLOAD`（共 47 份文档），保证离线文档站可就地打开新增链接。
- `.gitignore` 忽略 Git Bash 下 `2>nul` 落盘的 Windows 保留名空壳文件（只忽略不删除：
  系统回收站 API 不支持该设备名）。
- 工作记忆 `.workbuddy/memory/MEMORY.md` / `2026-11-14.md`：M2 状态改为「进行中」、
  单测基线 55 → 74、`cr-nat` 标记为已接入、登记 T2 套件与编号消歧。

### 收尾验证（均为真实命令输出）
- `cargo test --workspace --offline` **74 passed / 0 failed**；`T2 --self-test` 3/3、
  `--mode local` **10/10 PASS**（`evidence_valid=false` / `s6_status=not_run`）；
  `T0 --mode real --no-cleanup` **21/21 PASS、`evidence_valid=true`**；
  `T1 --self-test` 11/11（2.4.47 已跑）；`scripts/build_docs.py --check` PASS（36 份 Markdown、
  front matter、相对链接与索引页数据块）。
- 上下文感知的陈旧门禁口径审计：**0 条未标注的历史语句**；`spikes.json` / `t2_config.json` JSON 校验通过。

### 下一步计划（M2/W3.4）
从 `m2_progress.remaining_for_g2` 中选可在**本机闭环**的一项：**生产中继服务端 +
持有者侧中继订阅线程**。当前 `cr-nat::relay` 只有客户端侧帧协议，T2 依赖本地 mock 桥；
W3.4 补服务端（注册 / 路由 / ACK / 连接管理）与守护进程订阅线程，并把 T2 的 mock 桥换成真实服务端回归。
**不改动 S3/S5/S6 状态、不宣称 G2 达标**；S6 四类真实 NAT 矩阵、S3、S5 仍须真实多 NAT / 跨主机环境。

## [2.4.47] - 2026-11-14 — M2/W3.3 开工：cr-nat 接入复活路径 + T2 本地接线验收 + 门禁口径同步

> 范围声明：G1 已于上一版本通过、M2 门禁解除；本轮才**真正启动 M2 接线层**。
> **不改变任何 Spike 的 `status`**：S3 / S5 / S6 仍 `not_run`，**不宣称 G2 达标**；
> `PROTOCOL_VERSION=1`、T0 golden 字段、CLI v1 既有命令返回值均未改。

### 背景
- `spikes.json → t1_progress.design_gaps[0]` 记录的缺口：「cr-nat 库已落地，但 M1 CLI 复活路径
  （跨 NAT 的 UDP 唤醒 / 私网 tracker peer 地址协商）尚未接入该库，接入属 M2 范围」。
- 2026-11-14 的 2.4.46 只更新了 `spikes.json`，`README.md` / `ROADMAP.md` / `G0-REVIEW.md`
  / `docs/README.md` 曾写着「G1 未通过、M2 blocked_by_G1」，与机读结论矛盾。本轮一并同步。

### 实现（std-only，零第三方依赖）
- **`cr-core`**：`Config` 新增可选 `nat` 段（`enabled` / `stun_servers` / `gateway` /
  `upnp_location` / `relay` / `bt_external`）。缺省即全关，T0/T1 行为逐字节不变；新增向后兼容单测。
- **`cr-nat/src/endpoint.rs`（新）**：`EndpointKind` / `Endpoint` / `PeerAdvertisement` /
  `EndpointPlan`；`build_plan()` 纯计算生成有序候选（私网 → 显式端口映射 → STUN 预测 → 中继，
  对称 NAT/CGNAT 下剔除预测映射并中继优先）；`tracker_endpoint_for()` 保证**公网请求方绝不收到
  RFC1918 私网 peer**（无真实公网 BT 映射时返回空 peer 列表）。另修 `Traversal::run()` 中
  `CGNAT` 启发式误把 LAN 私有网关当证据、以及 `rsplit(':')` 取到端口而非 IP 的两处缺陷。
- **`cr-nat/src/holepunch.rs`（新）**：`NAT_PUNCH` / `NAT_PUNCH_ACK` 控制面帧；
  `wake_via_endpoint()` **打洞与唤醒复用同一个 UDP socket**（cone NAT 正确性的关键）；
  `relay_wake()` 经 `cr_nat::relay` 帧协议取回真实 `DELIVERED`。失败一律 Err，不合成应答。
- **`cr-nat/src/stun.rs`**：新增 `probe_one_with` / `StunSession::probe_with`（可插拔接收），
  使守护进程能用**唤醒监听 socket** 做 STUN，学到的外部端口与实际唤醒端口一致。
- **`cr-nat/src/lib.rs`**：抽出 `choose_strategy()` 供回填 STUN 结果后重算策略。
- **`coldrevived`**：新增 `nat_path.rs`（`NatContext` / probe / advertise / peer / plan / wake /
  tracker / status）、`UdpHub`（STUN 响应 demux 给 probe）、`NAT_PUNCH` 回包；
  `commands.rs` 的 `send_wake()` 在 nat 打开且取得对端通告时改走候选计划（覆盖 `wake` / `find` /
  `metrics` / 全部 scenario），否则保持原单播；`/announce` 按请求方 IP 分流；
  新增 `nat` CLI 子命令与 `GET /nat/advertise`。
- **`bt.rs`**：新增 `tracker_response_optional()` 支持空 peer bencode 列表；原 `tracker_response()`
  保留并在 nat 关闭时仍被调用（字节级不变）。

### 验证（全部真实观测）
- `cargo test --workspace --offline`：**74 passed / 0 failed**（基线 55 + 本轮 19）。
- **T2 本地接线验收 `10/10 PASS`**：`tests/e2e/t2/t2_nat_path.py --mode local`，
  mock STUN / mock 中继桥 + 真实 UDP/TCP/HTTP/CLI 观测；报告
  `tests/e2e/t2/reports/t2-*-local.json`（gitignore），`evidence_valid=false`、`s6_status=not_run`。
  `--self-test` 3/3（mock wire 自检）。
- **T0 `--mode real` 回归 21/21 PASS、`evidence_valid=true`**；T1 `--self-test` 11/11。
- 顺带修复 T0 观测客户端对 `download` 的 10 s 超时：daemon 内部上限 120 s，新版
  aria2-next 3.1.0 稍慢即被误判 `BLOCKED`；现按条用例放宽到 190 s（`t0_cli.py`），断言不变。

### 文档与机读口径
- `README.md` / `docs/README.md` / `docs/06-plan/ROADMAP.md` / `G0-REVIEW.md`（新增 §11）/
  `OPEN-QUESTIONS.md`：把 G1/M2 状态从「未通过 / blocked」同步为「已通过 / 已开工」，
  历史条目按时间追加口径保留，未篡改。
- `tests/spikes/spikes.json`：修正 `m1_progress.note` 与 `m2_gate` 的自相矛盾；
  新增 `m2_progress`；更新 `t1_progress.design_gaps[0]` 为「接线层已闭合、S6 矩阵仍待实测」；
  `S6.observations` 增补 `m2_wiring_2026_11_14`（`evidence_valid_for_pass=false`，**status 不变**）。
- `.gitignore` 增加 T2 的 `.run/` 与 `reports/`；新增 `tests/e2e/t2/README.md`。
- 收尾补齐剩余历史口径：`docs/05-quality/TEST.md`（开头 / §2 / §2.2 / §6 前更正）、
  `docs/02-architecture/TECH-STACK.md`、`tests/README.md`、`tests/spikes/README.md`、
  `tests/e2e/t0/README.md`、`tests/e2e/t1/README.md`、`.workbuddy/memory/MEMORY.md`，
  一律「保留 2026-11-13 原文 + 追加 2026-11-14 更正」，不静默改写历史结论。
- **T2 编号消歧**：`tests/e2e/t2/` 是 M2/W3.3 本地接线套件，与 `TEST §2` 规划编号
  「T2 = 公网 ≥8 台多节点」不是同一环境；已在 TEST / tests README / T1·T2 README 写明，
  避免把本地接线 PASS 误读为公网可达率证据。
- `scripts/build_docs.py` 的 `EMBED_EXTRA` 增加 `tests/e2e/t2/README.md`，并重生成
  `docs/index.html` 的 `DOCS_PAYLOAD`，保证离线文档站可就地打开新增链接。

### 剩余（G2 出口，未结案）
S6 四类真实 NAT（含 CGNAT）可达率矩阵、S3 真实双栈 + G3 矩阵、S5 真实拥塞链路 ≤5% 实测、
M2 其余骨架 crate 引擎/资源治理、生产中继服务端与持有者侧订阅、BT/TCP 打洞。

## [2.4.46] - 2026-11-14 — T1 --mode real 双机真实 NAT 全链路 11/11 PASS；G1 通过、M2 门禁解除

> 范围声明：仅 T1 真实验收结果与 `spikes.json` 回填。**不改变任何 Spike 的 `status`**，
> S3 / S5 / S6 仍 `not_run`；S6 四类 NAT 矩阵实测属 M2 范围。

- 用户部署第二台物理主机 B（192.168.1.209 / DESKTOP-CCE53RF），A 为 192.168.1.210 /
  DESKTOP-DJQSC95；B 侧预启动 `coldrevived.exe`，下载器为 Rayburst `aria2-next.exe` 3.1.0。
- `python3 tests/e2e/t1/t1_accept.py --mode real --refresh-nat` → **11/11 PASS、
  `evidence_valid=true`**：双机 machine_id/host 互异、跨主机 KAD 发布发现、UDP WAKEUP v1
  挑战应答、P95 2.0 ms（30 样本）、复活 30/30、伴随种子 InfoHash 校验、第三方 BT 下载闭环
  （落盘 SHA-256 + 逐片 SHA-1，数据面无私有协议）、现场 STUN 探针
  （mapped=112.5.60.4:8749 / 3 台应答 / 公网 UDP 出站可用）、golden 报文契约、最小暴露无残留。
- 报告：`tests/e2e/t1/reports/t1-20261114T084653Z-real-aliases.json`；
  STUN 探针：`tests/e2e/t1/reports/t1-nat-20261114T084639Z.json`（均 gitignore）。
- 门禁：**G1 = T0（21/21）+ T1（11/11）通过**；`m1_progress.m2_gate` 由 `blocked_by_G1`
  改为 `G1 satisfied — unblocked`。**S6.status 保持 `not_run`**：T1-08 只是单 NAT 样本，
  不是四类 NAT 矩阵实测。机读同步 `t1_progress` / `g0_review.t1_and_s5` / `S6.observations`。

## [2.4.45] - 2026-11-14 — 下载器随 Motrix Next→Rayburst 升级更正为 `aria2-next.exe` 3.1.0

> 范围声明：仅文档与配置路径更正（app 升级导致），**不改变任何 Spike 的 `status`**、**不宣称 G1 通过**。

### 背景
- 用户为两端一致，将 Motrix Next 升级为更名后的 **Rayburst**（安装目录 `C:\Program Files\Rayburst\`）。
- 捆绑的 aria2 CLI 由 `motrixnext-aria2c.exe` 更名为 **`aria2-next.exe`**，版本由 aria2 1.36.0 变为 **aria2-next 3.1.0**；图形界面由 `motrix-next.exe` 变为 `rayburst.exe`。

### 验证
- 实测 `C:\Program Files\Rayburst\aria2-next.exe --version` → `aria2-next version 3.1.0`，且接受 ColdRevive `download()` 传入的全部参数
  （`--bt-tracker` / `--enable-dht=false` / `--listen-port` / `--seed-time=0` / `--file-allocation=none` 等）无报错（exit 0）。

### 改动
- `tests/e2e/t1/README.md` §4 / §4.1：下载器统一改为 **Rayburst 捆绑 `aria2-next.exe`**（`C:\Program Files\Rayburst\aria2-next.exe`），并明确**不要用 `rayburst.exe` 图形界面**。
- `README.md` T0 env 示例 `T0_DOWNLOADER` → `C:/Program Files/Rayburst/aria2-next.exe`。
- `docs/02-architecture/TECH-STACK.md`：S1 历史记录保留「实测时 1.36.0」原貌，补充更名/升级说明（不篡改历史证据）。
- 重新生成 `t1-config.json`（`--prepare-only`），其 `downloader` 字段改为 `C:/Program Files/Rayburst/aria2-next.exe`；并同步到 A 侧分发目录 `C:\ColdRevive-dist`。

## [2.4.44] - 2026-11-14 — T1 README 澄清 B 机无需源码，只需 tests/ + exe

> 范围声明：仅文档澄清，**不改变任何 Spike 的 `status`**、**不宣称 G1 通过**。

### 改动
- `tests/e2e/t1/README.md` §4.1 步骤②澄清：B 机**不需要整个工程源码**。`coldrevived` 是预编译、std-only、自包含的二进制，运行时不编译，也不需要 Rust 源码 / Cargo.toml / `target` 构建目录 / Python harness / 其它二进制。
- 依据：`main.rs` 中 `bootstrap_holder()`（读取 `sample_path` 并做种）**仅当 `node_id=="A"` 时执行**；`else` 分支（node B）只起 UDP/HTTP 监听、不读样本、不做种。故样本文件 `sample_path` 仅 A 需要，B 可不拷。
- B 实际只需：`coldrevived.exe` + `t1-config.json` + `tests/fixtures/t0/golden/*`（相对仓库根路径，由 CWD 解析）+ aria2。给出「只拷 `tests/` 子树 + exe」的最小拷贝命令，整仓库拷贝仅作为备选。

## [2.4.43] - 2026-11-14 — T1 下载器明确为 Motrix Next 捆绑 aria2（非 GUI）

> 范围声明：仅文档更正，**不改变任何 Spike 的 `status`**、**不宣称 G1 通过**。

### 改动
- `tests/e2e/t1/README.md` §4 与 §4.1 澄清：ColdRevive 的 `download()` 以子进程调用下载器并传入 aria2 专属参数（`--bt-tracker` / `--listen-port` / `--enable-dht=false` …），且等其下载完成后退出。**下载器必须用 Motrix Next 捆绑的 aria2 CLI `motrixnext-aria2c.exe`，不能用 `motrix-next.exe` 图形界面**（后者不认参数、不会退出）。
- 实测 `C:\Program Files\MotrixNext\motrixnext-aria2c.exe --version` 输出 `aria2 version 1.36.0` 且接受上述全部参数；与 `bt.rs` 注释「Motrix Next 捆绑 aria2 1.36.0」一致。
- Windows CMD 步骤的 `T0_DOWNLOADER` 与 `--downloader` 均改为该路径（带空格需加引号），并注明 B 机同理、无需额外安装 aria2、无需启动 Motrix 图形界面。

## [2.4.42] - 2026-11-14 — T1 README 新增 Windows CMD 操作小节

> 范围声明：仅文档补充，**不改变任何 Spike 的 `status`**、**不宣称 G1 通过**。
> 目的：把 §4 的 bash 步骤完整翻译为 Windows 命令提示符（CMD）写法，便于用户在两台物理机实测时直接照抄。

### 改动
- `tests/e2e/t1/README.md` 新增 **§4.1 Windows CMD 操作（A/B 机具体指令）**，包含：前置防火墙 `netsh` 规则、① A 编译+`--prepare-only` 生成配置、② 三选一文件传输（U 盘 robocopy / 局域网 `\\B\C$` / OpenSSH `scp`）、③ B 从仓库根启动守护进程、④ A 正式跑 `--mode real`、收尾与本地提交。
- 强调 Windows 差异：`set`（仅本窗口有效）、`%CD%`、`^` 续行、`py`/`python`、B 端 `VCRUNTIME140.dll` 缺失处理；因 config 已相对化，B 端**零路径编辑**。

## [2.4.41] - 2026-11-14 — T1 harness 配置可移植化（B 主机零路径编辑）

> 范围声明：仅改 `tests/e2e/t1/t1_accept.py` 与文档，**不改变任何 Spike 的 `status`**、**不宣称 G1 通过**。
> 目的：让第二台物理主机 B 无需安装编译环境——只需拷二进制 + 仓库源码/夹具，从仓库根目录启动即可。

### 改动
- `_write_config` 输出的 `workdir` / `sample_path` / `golden_req` / `golden_ack` 改为**相对仓库根**路径（`os.path.relpath(..., ROOT)`，统一为正斜杠）。
- `spawn()` 与 `cli()` 的子进程 `cwd` 设为仓库根 `ROOT`，使相对路径在 A/B 两侧一致解析。
- 新增 `--prepare-only`（须配合 `--mode real`）：仅生成可移植 `t1-config.json` 后退出，**不触发远端未预启动守卫**，便于先生成配置再拷到 B。
- 校验：已用 `--prepare-only --mode real` 生成配置并确认四路径均为相对路径；`cargo build --release --workspace --offline` 通过。

### 对二机实测的影响
- B 主机只需 `coldrevived.exe` + `aria2c` + 仓库源码/夹具（`tests/fixtures/t0/golden/` 等），从仓库根 `cd` 后启动守护进程，**无需任何路径编辑**。
- 完整步骤见 [T1 README §4](tests/e2e/t1/README.md)。

## [2.4.40] - 2026-11-13 — cr-nat NAT 穿透栈落地（M2 合法前置）；G1 / M2 / S6 门禁不变

> 范围声明：依用户指令（准备第二台物理主机并先实现 NAT 穿透）落地 `cr-nat` 穿透栈，**作为 M2 的合法前置**，
> 与 G0 条件冻结同口径（豁免 G0 时点结论，但 **S6.status 保持 `not_run`**）。
> **不改变任何 Spike 的 `status`**、**不宣称 G1 通过、不启动 M2 二进制 / 唤醒接入**。
> 结论：`cr-nat` 补齐了 T1 design_gaps[0] 的「M1 无穿透手段」库层缺口；但 M1 CLI 复活路径尚未接入该库（属 M2）。

### cr-nat 穿透栈实现（`src/crates/cr-nat/`，std-only 零第三方依赖）

- `stun.rs` — RFC 5389 STUN Binding：XOR-MAPPED-ADDRESS 解析、TID 校验、同 socket 多服务器探测。
- `natpmp.rs` — RFC 6886 NAT-PMP 外部地址 / 端口映射（显式端口优先、缺省 5351）。
- `pcp.rs` — RFC 6887 PCP MAP（UDP/TCP），含 nonce 回显与 ext_ip 解析。
- `upnp.rs` — UPnP IGD：SSDP 发现 + SOAP `get_external_ip` / `add_port_mapping`，相对 `controlURL` 解析。
- `relay.rs` — 中继 / LowID 回退帧协议（register / send_relay / recv_frame）。
- `classify.rs` — 由 STUN 映射集合分类 NAT 类型（FullCone / RestrictedCone / PortRestrictedCone / Symmetric / Cgnat / Unknown）+ cone 子型细分 + CGNAT 启发式。
- `lib.rs` — `Traversal::run()` 编排 STUN→UPnP→NAT-PMP→PCP→CGNAT 并选策略；常量 `S6_REQUIRED_MATRIX` / `R04_MIN_REACHABILITY=0.70` 齐备。
- **24 个单测全绿**（含 STUN / NAT-PMP / PCP / UPnP / 中继的本地 UDP mock 往返 + 分类 + 策略选择 + 矩阵口径）；
  `cargo test --workspace --offline` 现 **55 / 0 失败**（基线 31 + 本轮 24）。

### 新增诊断 bin：`coldrevive-nat`（`src/bins/coldrevive-nat/`）

- 仅依赖 `cr-nat`，不触碰 T0 契约与 M1 复活流程；`--json` 稳定字段输出
  `nat_type / public_udp_egress_ok / upnp|natpmp|pcp_available / mapped_address / cgnat_suspected / recommended_strategy / needs_relay / s6_status=not_run`。
- 端到端校验：对本机运行真实 STUN（stun.l.google.com / stun.cloudflare.com）已往返成功，
  `mapped_address=112.5.60.4:xxxxx`、`public_udp_egress_ok=true`；本环境 UPnP / NAT-PMP / PCP 均不可用，`recommended_strategy=udp-predictive`。
  分类保守为 full-cone（未做 OTHER-ADDRESS 二次细分与多端口对称判定，属 S6 矩阵实测范围）。

### 门禁结论

`cr-nat` 仅补齐穿透**手段**；S6 判据「四类 NAT（含 CGNAT）实测可达率」仍须 **M2 在多个真实 NAT 环境实测**，
`spikes.json` 中 `S6.status` 保持 `not_run`，不据此宣称 G2 / S6 达标。`spikes.json` 结论仍由实验执行人据 M2 实测证据手工回填。
机读口径见 `spikes.json` 的 `S6.observations.cr_nat_implementation_2026_11_13`、`m1_progress.workspace`、`t1_progress.design_gaps`。

同步：[README.md](README.md)（v2.4.40）· [S6 实验包](tests/spikes/s6/) · [spikes.json](tests/spikes/spikes.json) · `.gitignore`。

## [2.4.39] - 2026-11-13 — T1 双机 harness + M1 跨主机寻址 + S5 真实 uTP 栈量测；G1 / M2 门禁不变

> 范围声明：本轮继续推进 [G0-REVIEW](docs/06-plan/G0-REVIEW.md) 条件冻结下的 M1 必测回填，
> **不改变任何 Spike 的 `status`**（S3/S5/S6 仍 `not_run`），**不宣称 G1 通过、不启动 M2**。
> 结论：**T1 真实验收在本环境物理阻塞**（只有一台主机，WSL/虚拟机被安全策略禁止，未做绕过），
> 因此仅交付可复现 harness 与单机预检；`--mode real` 明确 `BLOCKED`。

### M1 跨主机支持（为 T1 铺路，T0 口径不变）

- `cr-core` 的 `NodeSpec` 新增可选 `host`（缺省回落 `bind`），新增 `Config::host / me_host / peer_host / url`；
  所有**对端** URL / UDP 地址 / tracker / torrent URL 按节点 host 构造，本地监听仍用 `--bind`。
- `coldrevived` 新增 `--self-host` / `--workdir` / `--sample` / `--downloader`，两端可共用一份配置；
  `coldrevive-cli` 按节点 host 连接；`/health` 输出真实 `host` / `machine_id` / `pid` 供双机取证。
- 新增单测 `per_node_host_defaults_to_bind_and_can_override`；`cargo test --workspace --offline` 全绿
  （31 个单测；此前 v2.4.38 记为 27 系统计口径遗漏，见下方更正）；**T0 `--mode real` 回归 21/21 PASS、`evidence_valid=true` 不变**。

### T1 双机真实验收 harness（`tests/e2e/t1/`）

- `t1_config.json`（双机启动契约）、`t1_cases.json`（10 用例 / 40 断言）、`t1_accept.py`（编排、
  断言、报告）、`t1_netprobe.py`（真实 UDP 可达性探针）、[README](tests/e2e/t1/README.md)。
- 证据闸门：`evidence_valid=true` 仅当 `--mode real` + 全用例 PASS 无 BLOCKED/SKIP + `/health` 实测
  A/B 的 `machine_id` 与 `host` 均不同 + 真实 NAT 报告 + 观测来源 `real`。`--self-test` /
  `--lan-precheck` 恒为 `false`，且 `--mode real` 不假装能远程拉起 B 进程。
- 实跑：`--self-test` 11/11 PASS；`--lan-precheck`（`aliases` 与 `nic` 两种地址模式）各
  **10 PASS / 1 SKIP**（T1-01 双机身份），唤醒 P95 **2.0 ms**（30 样本、0 超时）、复活 **30/30**、
  第三方 aria2 跨地址完成标准 BT 下载；`--mode real` 缺远端前置条件时 **10 BLOCKED / 退出码 2**。
- `t1_netprobe.py` 真实量测：同 IP `5/5` 可达，跨 IP 仅 `6/20`（全部是回环别名之间）；
  物理网卡 `192.168.1.210` 与 Hyper-V `172.26.96.1` 双向 `WSAENETUNREACH (10051)`。
  首次两网卡预检的 UDP 唤醒确实报 `10051` 并如实 FAIL/BLOCKED，已保留现场报告。
- **设计缺口登记**：M1 CLI MVP 无 UPnP / NAT-PMP / PCP / STUN 打洞 / 中继，跨 NAT 双机的唤醒与
  私网 tracker peer 地址将不可达；S6 先导样本显示本机 UPnP 与 NAT-PMP 均不可用。G1 前须补穿透栈
  或明确 T1 为「同一 NAT 内两机」。

### S5 真实 librqbit-utp 协议栈量测（`tests/spikes/s5/utp/`）

- 用缓存中的 **librqbit-utp 0.7.0 真实 BEP 29 栈**（独立 Cargo 工程，`cargo build --offline --release`
  可复现）跑共享真实时钟瓶颈：前台 TCP 与后台 uTP 争用 16 Mbit/s 链路，排队由真实 `sleep` 产生。
- **源码核实否证了原假设**：该版本 `src/lib.rs` 写着 `// TODO: LEDBAT congestion control`，
  `CongestionControllerKind` 只有 `Cubic`，`src/congestion/` 只有 `cubic.rs`。
  即 `prefer_utp=true` 只切换传输层，**不提供 LEDBAT 低优先级退让**；S5 README §2 已更正、
  R-07 风险口径上调。
- 3 s × 3 轮中位前台劣化：`ratio=0.20 → 14.82%`（❌）、`0.05 → 4.31%`（✅）、
  `0.02 → 2.70%`（✅）；三档劣化与后台实际链路占比近似 1:1，**未观察到退让回收**。
  支持 M1 已回调的默认 `0.02`，但这是令牌桶降级生效，**不是 uTP 达标**。
- 报告固定 `evidence_valid_for_pass=false`（瓶颈仍是本机用户态整形，非 `tc/netem` / 跨主机链路），
  `S5.status` 保持 `not_run`。

> **计数更正**：v2.4.38 记为「27 个单测」是统计口径遗漏（只数了部分 crate）。按 `cargo test --workspace --offline` 汇总，全部 crate / bin 的 `#[test]` 基线实为 **30**；本轮新增 `per_node_host_defaults_to_bind_and_can_override` 后为 **31 / 0 失败**。历史条目按 ROADMAP §6 惯例保留当时口径，现行状态一律以 31 为准。

### 门禁结论

`T0 21/21 PASS` + `T1 单机预检 10 PASS` **都不等于 G1**：T1 `--mode real` 未执行、S3 真实双栈 +
G3 矩阵、S5 真实拥塞链路、S6 多 NAT 实测均未完成。**M2 保持 `blocked_by_G1`（当时口径）**，本轮未新增任何
M2 实现。机读口径见 `spikes.json` 的 `t1_progress`、`m1_progress.g1_remaining` 与
`S5.observations.utp_real_2026_11_13`。

同步：[ROADMAP](docs/06-plan/ROADMAP.md) 2.10 · [G0-REVIEW](docs/06-plan/G0-REVIEW.md) 1.2 ·
[TEST](docs/05-quality/TEST.md) 2.4 · [RISK](docs/05-quality/RISK.md) 2.3 ·
[TECH-STACK](docs/02-architecture/TECH-STACK.md) 2.6 · [CONFIG](docs/04-protocol/CONFIG.md) 2.2 ·
[T1 README](tests/e2e/t1/README.md) · [S5 README](tests/spikes/s5/README.md) · `.gitignore`。

## [2.4.38] - 2026-11-13 — M1 开工：Cargo Workspace + 真实 CLI MVP + T0 real 21/21 PASS；M2 仍受 G1 阻塞

> 范围声明：依 [G0-REVIEW](docs/06-plan/G0-REVIEW.md) 的条件冻结**开工 M1**（工程接线与 MVP 实现），
> **不改变任何 Spike 的 `status`**（S3/S5/S6 仍 `not_run`），**不宣称 G1 通过、不启动 M2**。
> 门禁复核结论：**M1 可开工**（G0 条件冻结已签署、剩余 2 个 preflight 阻塞项恰为 M1 自身接线）；
> **M2 不可开工**（ROADMAP §2 依赖 M1/G1，T1 局域网双机与 S3/S5/S6 必测回填均未完成）。

### 工程落地（真实可编译，非零桩）

- 新增根 `Cargo.toml` + `Cargo.lock`：**17 个 crate + 3 个 bin**，`cargo test --workspace --offline` 通过
  **27 个单测 / 0 失败**，全部 std-only、零第三方依赖（不依赖 crates.io 可达性）。
- M1 真实实现：`cr-core`（最小 JSON / 配置 / trace_id）、`cr-hash`（SHA-1 / SHA-256 / HMAC-SHA256，含
  RFC 4231 向量）、`cr-indexer`、`cr-torrent`（真实 bencode、InfoHash、逐片 SHA-1 校验）、`cr-store`
  （COLD 轻量影子索引 + COLD/WAKING/HOT 幂等状态机）、`cr-kad`（专属命名空间 Key、KAD 128-bit /
  Mainline 160-bit 双栈 ID、本地 UDP 发现）、`cr-wake`（PROTO v1 Challenge-Response + 8 类错误码）。
- `coldrevived` / `coldrevive-cli` 按 [CLI v1 契约](tests/e2e/t0/README.md) 实现 20 条真实观测命令：
  HTTP `/health` `/cli` `/torrent` `/announce` `/inject`、UDP KAD 发现与唤醒、最小标准 BT 种子端 +
  本地 tracker。**数据面用第三方 aria2 1.36.0（Motrix Next）真实下载**，私有协议不进入 BT 分片。
- `cr-engine` / `cr-qos` / `cr-nat` / `cr-sched` / `cr-guard` / `cr-dht` / `cr-api` / `cr-ed2k` /
  `cr-reputation` / `cr-notify` / `coldrevive-agent` 为**骨架**（仅 trait / 常量 / 门禁标记），
  按 ROADMAP 归属 M2/M3/M4，不实现、不宣称达标。

### T0 真实验收（G1 的部分证据）

- `CliAdapter.wired = True`（代码层接线），并保留**环境级防呆**：缺任一二进制时 `--mode real` 仍
  `BLOCKED` / 退出码 2（CI 依赖该行为）。`t0_accept.py` 新增 `setup_real_env()`：写双节点配置、
  按 `launch` 契约启动 A/B、等待健康检查，由 `harness.stop_all()` 清理。
- 设 `COLDREVIVE_DAEMON` / `COLDREVIVE_CLI` / `T0_DOWNLOADER` 后执行 `--mode real`：
  **21/21 PASS、FAIL 0、BLOCKED 0、`evidence_valid=true`**，报告
  `tests/e2e/t0/reports/t0-20261113T075155Z-real.json`。真实链路含：伴随种子真实 InfoHash 复算、
  UDP WAKEUP_REQ/ACK、8 类故障注入、30 次唤醒时延采样、30 次复活成功率、aria2 逐片校验下载。
- 首轮实跑诚实暴露 3 个缺陷并修复：① 重复唤醒导致状态跃迁累积 → 改幂等激活；② nonce 重放
  判定「重复激活」误报 → 同上；③ `residual_pids` 把本次在用的 `.pid` 误判为残留 → 改为按进程
  存活状态判定（Windows `tasklist` / Linux `/proc`）。修复后复跑 21/21 全绿。

### M2 合法前置准备（不越过 G1）

- S5 实测标定落地：`config/coldrevive.example.toml` 与 [CONFIG](docs/04-protocol/CONFIG.md) 的
  `background_upload_ratio` 默认由 `0.2` 回调为 **`0.02`**（实测 0.2 → 20.12%、0.05 → 6.19%、
  0.02 → 4.03%；NFR-03 ≤5%）。**这只是配置回调，S5 仍 `not_run`**。
- `cr-qos` 登记实测表与令牌桶模型；`cr-nat` 登记 S6 四类 NAT + CGNAT 矩阵与 R-04 ≥70% 阈值；
  `cr-engine` 固定 `Engine` Trait 边界（ADR-002 选项 A）。
- `spikes.json` 新增 `m1_progress` 与 `g0_review.m1_engineering_wiring`，S3/S5/S6 各补
  `m1_2026_11_13` 证据条目（均 `evidence_valid_for_pass=false`），**状态保持 `not_run`**。

### 边界与剩余工作（G1 / M2 仍关闭）

`T0 evidence_valid=true` **只证明 T0 单机回环链路通过**，不等于 G1：还缺 ① **T1 局域网双机 +
真实 NAT 全链路**；② S3 真实双栈集成 + G3 矩阵；③ S5 真实拥塞链路 rqbit uTP ≤5% 实测；
④ S6（M2）四类 NAT 可达率实测。因此 **M2 保持 `blocked_by_G1`（当时口径）**，不得据本轮结果启动。
`m0_preflight.py` 的 `g0_ready` 算法按 G0-REVIEW §6 保持不变；工程阻塞项清零但豁免项未结案时
新增控制台提示，避免被误读为「八项 Spike 结论齐备」。

### 文档与 CI 同步

- [ROADMAP](docs/06-plan/ROADMAP.md) 2.8 → 2.9、[G0-REVIEW](docs/06-plan/G0-REVIEW.md) 1.0 → 1.1、
  [CONFIG](docs/04-protocol/CONFIG.md) 2.0 → 2.1、[README](README.md)、`src/README.md`、
  `tests/README.md`、`tests/e2e/t0/README.md` §3/§5；`t0_cli_selftest.py` 由「断言 wired=False」
  改为「代码层 wired=True + 环境级防呆仍 BLOCKED」。
- `.github/workflows/t0.yml`：新增 `cargo test --workspace --offline`；修正已过期的
  「S1–S7 必须 `not_run`」断言（改为人工回填白名单 + 证据字段校验）；real 模式防呆步骤改为
  「CI 无二进制时应 BLOCKED」。

## [2.4.37] - 2026-11-13 — G0 条件冻结生效 + S3/S5/S6 真实量测 + 工程门禁收敛

> 范围声明：本轮**不改任何 Spike 的 `status`**（S3/S5/S6 仍 `not_run`），不虚判 `pass`，
> **不创建生产 crate**（M1 工程接线仍未完成）。项目所有者于本轮显式作出两项治理决策：
> ① [ADR-001](docs/02-architecture/adr/ADR-001-kad-network.md) 签署默认模式 **`public`**（1.1 待定 → 1.2 已接受）；
> ② 批准 **G0 条件冻结（conditional pass）**——豁免 S3/S5/S6 在 G0 时点给出最终结论，M1/M2 必测回填。
> 评审记录见 [G0-REVIEW](docs/06-plan/G0-REVIEW.md)（新增 DOC-22），机读决策见 `spikes.json` 的 `g0_review`。

### 新增真实实验（三项，全部标记 `evidence_valid_for_pass=false`）

- **S3 `tests/spikes/s3/dualstack_io.py`**：真实 UDP 双栈并发 + 共享端口 demux（KAD 16B / Mainline 20B），
  注入 1437 个跨命名空间畸形帧 → **1437/1437 拒绝、0 误路由、0 丢包**；总带宽 **3.276 Mbit/s**
  ≈ 两栈之和，RSS 增量 **0.93 MiB**。C2 的「带宽增量」由估算升级为实测。
  排错留痕：首版 demux 仅校验最小长度，160 个错宽 ID 帧被误收；改为精确 ID 宽度校验后归零。
- **S5 `tests/spikes/s5/qos_bench.py`**：真实 TCP 双流 + 真实时钟令牌桶 + 64 Mbit/s 应用层瓶颈整形
  + LEDBAT 式延迟退让模型。令牌桶硬上限成立（后台 ≤20.25% 链路），但
  **默认 ratio=0.2 时前台劣化 20.12%**（退让模型 15.0%）、ratio=0.05 → 6.19%、
  **ratio=0.02 → 4.03%**。结论：**NFR-03 ≤5% 在默认配置下未达成，R-07 由 🟡 上调 🟠**，
  M1 默认值须按实测回调并启用 fallback 链。报告 `reports/s5-qos-final-ratio0{20,05,02}.json`。
- **S6 `tests/spikes/s6/stun_probe.py`**：RFC 5389 STUN + SSDP + NAT-PMP + 二跳 traceroute 的
  **首个真实 NAT 样本**：`192.168.1.210` → `112.5.60.4:36764`，跨 4 台 STUN（2 个目的 IP）映射不变（cone 类），
  公网 UDP 可达（RTT≈245 ms），**UPnP IGD 与 NAT-PMP 均不可用**，二跳公网、未见 CGNAT。
  R-04 不再按「UPnP 可用」假设估计；Q4 仍未结案，须 M2 多 NAT 实测。
  排错留痕：首版误把 STUN family `0x02` 当 IPv4（实际 IPv4=`0x01`），4 个有效响应被误判为解码失败。

### 工程门禁收敛

- `m0_preflight.py` 的「至少一个第三方 BT 客户端」改为**证据驱动**：除 PATH 标准名外，识别已安装的
  Motrix Next `motrixnext-aria2c.exe`，且**只有** `spikes.json` 的 `S1.cross_client_round` 记录
  `executed=true` 且 `all_pieces_verified=true` 时才计 OK（不把「文件存在」误判成「互通通过」）。
  阻塞项 **3 → 2**（剩余：M1 二进制、CliAdapter.wired，均属 M1 工程接线）。
- `m0_preflight.py` 报告新增 `g0_governance` 字段并在控制台打印治理状态；
  明确 **条件冻结不改变 `g0_ready` 算法、不自动清除任何 Spike 结论**。

### 文档同步

- 新增 [DOC-22 G0-REVIEW](docs/06-plan/G0-REVIEW.md)：八项结论矩阵、豁免边界与退回条款、
  ADR 状态、残余风险、工程门禁、M1/M2/M4 复核时点；`docs/README.md` 2.5 → 2.6 补 DOC-22 行。
- `ADR-001` 1.1 → 1.2（待定 → 已接受，所有者签署 `public`）；`adr/README.md` 统计 9 → 10 已接受、2 → 1 待定。
- `TECH-STACK` 2.3 → 2.4、`ROADMAP` 2.7 → 2.8（§6 状态行 + 本轮条目 + **G0 缺口清单第三次更新**）、
  `RISK`（R-04 观测收窄 / R-07 等级上调 + 冻结后风险复核节）、`OPEN-QUESTIONS` Q4 先导观测注记。
- `tests/spikes/README.md` 顶部状态行、§3 起跑顺序、§5 S3/S5/S6 段落、§6 第 4 条补唯一豁免说明；
  `s3/README.md` / `s5/README.md`（新增 §8 量测表 + 4 项 harness 排错留痕） / `s6/nat-matrix.md`（新增 §8 先导实测）。
- `spikes.json`：`status_note` 重写为当前口径，新增 `g0_review`，S3/S5/S6 的 `observations` 与 `evidence_required` 补本轮证据。

### 剩余工作（冻结后）

M1 出口：S3 真实双栈集成 + G3 矩阵、S5 真实拥塞链路 rqbit uTP 实测 ≤5%、按门禁创建 crate 骨架；
M2：S6 四类 NAT（含 CGNAT）可达率实测，R-04 阈值 ≥70%；M4 前：R-02 拒绝率复审、R-11 书面法务意见。
任一 M1/M2 实测不达标须按各 spike fallback 降级并重新提交冻结评审。

## [2.4.36] - 2026-11-11 — G0 推进：S1 第三方客户端互操作轮通过 + S4/S7 `pass`、S5/S6 分析就绪

> 范围声明：本轮依用户指令以本机 **Motrix Next** 作第三方 BT 客户端执行 S1 跨客户端一轮（通过，`S1.status` 维持
> `pass`），并推进 S4–S7：**S4 / S7 经真实量测回填 `pass`**，S5 / S6 产出分析并诚实保持 `not_run`（按治理规则
> 本地/分析证据不虚判 pass）。**仍不创建生产 crate**（G0 未通过：S3/S5/S6 待 M1/M2 真实环境验证、ADR-001 待所有者签署）。

### S1 第三方客户端互操作轮（G3 素材，`all_pieces_verified=true`）

- 采用 **Motrix Next** 捆绑 `motrixnext-aria2c.exe`（aria2 1.36.0，download-only 构建）作第三方 seeder，
  走反向路径：`tests/spikes/s1/third-party/make_payload.py` 生成确定性 `payload.bin/payload.torrent`
  （128 片 × 16 KiB，infohash `2b77301a18d84d45c1615b268d69641df709c214`），aria2 `--check-integrity=true`
  哈希校验后进入 Seeder 态监听 `127.0.0.1:16881`；`cargo run -- cross-lee --peer 127.0.0.1:16881` 拉取
  **128/128 片全部 SHA-1 校验通过**（2 MiB 全量）。
- 排错：首轮 0 字节系旧 `.aria2` 控制文件 + `--check-integrity=false` 使 aria2 处于 Leecher 态；另确认
  rqbit 作 seeder 时 tracker announce 恒报 `port=4240`（实际监听 ephemeral），tracker 发现路径不可用，故直连 `--peer`。
- `spikes.json` S1.observations.cross_client_round 更新：`client_installed=true / executed=true /
  operator_attest_pending=false`（attest 依据用户显式指令）；报告 `tests/spikes/s1/reports/s1-cross-lee-*.json`。

### S4（R-06，影子索引 ≤100 MB）→ `pass`

- `tests/spikes/s4/`：真实 Rust `HashMap<[u8;20], ShadowRow>` 模型（COLD 轻量 handle，不含 Bitfield / 逐片哈希，
  符合 DATA §5）+ Windows `GetProcessMemoryInfo` 工作集增量采样（baseline→10k→50k→200k）。
- 实测：1 万文件索引 ≈3.79 MiB、含 SQLite 缓存 **≈7.79 MiB**（≪ 100 MB NFR-01）；5 万 ≈15.9 MiB（兼满足
  NFR-05 ≥5 万容量）；20 万 ≈53 MiB 仍有余量。R-06 去险，fallback（索引分库 / 惰性加载）未触发。
- 证据 `tests/spikes/s4/reports/s4-shadow-index-*.json` + `measure_s4.py`。

### S7（R-07，4 GB 造种开销）→ `pass`

- `tests/spikes/s7/s7_bench.py`：流式哈希 4 GB（262144 片 SHA-1 + 全文件 SHA-256 + `.torrent` bencode/写出），
  **耗时 4.14 s / 吞吐 989 MB/s / 峰值 RSS 29.3 MiB**，Idle 优先级（NFR-02）。R-07 去险，
  fallback（分片自适应 + 增量哈希）未触发。证据 `tests/spikes/s7/reports/s7-torrent-gen-*.json`。

### S5 / S6 → `not_run`（分析就绪，待 M1/M2 实测回填）

- **S5**（`tests/spikes/s5/README.md`）：NFR-03 双层控制——令牌桶硬上限 20% 上行（可本地单元验证）+
  uTP/LEDBAT 退让（rqbit 9.1.0 支持 uTP，S1 已证真实分片传输）；「前台劣化 ≤5%」须真实拥塞链路，
  回环队列延迟恒 ~0 无法触发 LEDBAT 退让，M1/G3 实测回填。
- **S6**（`tests/spikes/s6/nat-matrix.md`）：穿透栈（UPnP + NAT-PMP + UDP 打洞 + 中继/LowID 回退）覆盖
  全 4 类 NAT，分析型含中继可达率 ≥95%（> R-04 的 70% 阈值）；量化判据须 M2 真实多 NAT 实测
  （OPEN-QUESTIONS Q4，CGNAT 比例为关键不确定项）。

### 文档同步

- `TECH-STACK` 2.2 → 2.3（§2 状态块：跨客户端轮结果 + S4–S7 推进）；`ROADMAP` 2.6 → 2.7（§6 顶部状态行 +
  追加本轮条目 + G0 缺口清单第二次更新）；`tests/README.md` 状态行更新；`tests/spikes/README.md` 顶部状态行与
  §5/§6 已于本轮内先行更新；`tests/spikes/s1/README.md` §7 由「准备状态」改为「执行结果」并补排错记录。
- `m0_preflight.py` 现报 `G0 就绪: False`、`g0_pending_spikes = S3,S5,S6`；其余 3 个 MISSING 为 T0/G1 工程
  接线项（M1 二进制、CliAdapter、第三方下载器 PATH 标准名）。

## [2.4.35] - 2026-11-11 — G0 收尾校验 + 第三方 BT 客户端一轮预备

> 范围声明：本轮**不判任何新 pass、不改 `S3.status`**（仍为 `not_run`）、**不创建生产 crate**（G0 未通过）。
> 完成 S3 首轮证据的落盘与全量文档校验；并为「补第三方 BT 客户端 / 跨主机一轮」准备可立即执行的 harness。

- **S3 证据落盘**：在 `tests/spikes/s3/` 补充 50k/栈压测报告（`reports/s3-stress-50k-*.json`），与既有 10k 报告共同支撑 `spikes.json` 的 `dual_index_mib_at_10k=2.26 / at_50k=11.08`；两份报告均 `C1 verified`（串扰/泄漏 0/0）、`C2 verified`（≤100 MB NFR-05）、`evidence_valid_for_pass=false`。
- **文档校验通过**：`scripts/build_docs.py --check` 校验 35 份 Markdown（front matter / 相对链接 / 索引数据块均有效）；全量重建生成 35 个 HTML 页面并刷新 `docs/index.html`（43 份文档内嵌）。
- **第三方 BT 客户端一轮预备**（对应 S1 用户指令「补第三方 BT 客户端 / 跨主机再判」）：
  - `tests/spikes/s1/src/main.rs` 新增 `cross-seed`（rqbit 作 seeder，写出真实 `.torrent` 到磁盘、监听 `--bind`、等待外部 leecher 拉全部分片）与 `cross-lee`（rqbit 作 leecher，按 `--torrent` + `--peer` 直连第三方/跨主机 seeder、逐片校验）两个子命令；已编译通过并本机 `cross-seed` 自测（seed + 写种子 + 打印 info_hash/sample_sha1 + 超时干净退出）。
  - `spikes.json` S1.observations 新增 `cross_client_round`：`harness_ready=true / client_installed=false / executed=false / operator_attest_pending=true`。
  - `tests/spikes/s1/README.md` §1/§2 扩展子命令表与运行示例，新增 §7 记录上午安装客户端后的执行路径与治理规则（**不回滚 S1=pass**，需 operator attest 方可并入结论）。

## [2.4.34] - 2026-11-11 — G0 推进：**启动 S3 双栈资源可控 spike（首轮结构性证据）**

> 范围声明：本轮**启动 S3**（不判 pass），在 `tests/spikes/s3/` 新增零依赖双栈探针，给出首轮
> 结构性证据；**不做真实 KAD / Mainline DHT 网络加入**，`S3.status` 保持 `not_run`。仍不创建生产 crate
> （G0 未通过）。

- `tests/spikes/s3/`：零依赖 Rust 二进制 `s3-dualstack`，两个独立容器 `HashMap<[u8;16],_>`（KAD）+ `HashMap<[u8;20],_>`（BT），顺序 + 并发读写压测。
- 首轮证据：**S3-C1 verified**（串扰/泄漏 = 0，10k 与 50k/栈两轮）；**S3-C2 verified 结构性上界**（双栈索引 10k/栈 ≈2.26 MiB、50k/栈 ≈11.08 MiB，均 ≤ NFR-05 100 MB）。
- `spikes.json`：S3 新增 `observations`（规模、C1/C2 结果、scope、边界），`status` 仍为 `not_run`。
- `TECH-STACK` §2、`ROADMAP` §6（S3 已启动 + 「未开始」收窄为 S4–S7）、`tests/spikes/README.md` §5 补充 S3 说明。

## [2.4.33] - 2026-11-11 — G0 推进：**回填 S1=pass 并定稿 ADR-002（选项 A：采用 rqbit）**

> 范围声明：本轮**依 `two-peer` 4/4 `verified` 证据回填 `S1.status=pass`**（实验执行人手工回填，
> 非脚本自动），并据 `conclusion_rule` 将 [ADR-002](adr/ADR-002-bt-engine.md) 由「待定」定稿为
> 「已接受」（选项 A：采用 `rqbit`，握手拦截自实现于 `cr-engine` 外围）。**仍不创建生产 crate**
> （G0 未通过，S3–S7 未提交结论、ADR-001 待所有者签署）。S1 结论的边界同 v2.4.32：
> `127.0.0.1` 回环、0 公网报文、同主机双 Session；第三方 BT 客户端 / 跨主机一轮按用户指令补做（见下）。

- `tests/spikes/spikes.json`：`S1.status` 由 `not_run` → `pass`；`status_note` 同步；`S1.blocked_by` 已在前序置空。
- `ADR-002` 1.2 → 1.3：状态「待定」→「已接受」，决策表与 §决策 / §复核时点更新；明确「选项 A 已采纳、握手拦截自实现」。
- `ADR 索引`：ADR-002 行 🟡待定 → ✅已接受；统计 8→9 已接受；G0 待签署项重写（ADR-002 已定稿，ADR-001 仍待所有者签署 public/private）。
- `TECH-STACK` §2 状态块：S1 判据 4/4 `verified` 且已回填 `pass`，ADR-002 已定稿。
- `ROADMAP` §6：当前状态行更新（S1=pass）；S1 边界与治理段更新；G0 剩余缺口清单 ① 标记完成、③ 收窄为 ADR-001。
- `tests/spikes/README.md`：顶部状态行、§4 S1 判据、§6 第 3 条口径更新为「已回填」。
- `m0_preflight.py` 动态逻辑：`g0_pending_spikes` 自动剔除 S1（现为 `S3,S4,S5,S6,S7`）；`s1_action` 走「已有结论」分支。

## [2.4.32] - 2026-11-11 — 推进 G0：**S1 双 peer 真实分片传输与中途暂停打穿，四条判据齐备**

> 范围声明：本轮**未改 `spikes.json` 的 `S1.status`**（仍 `not_run`）、**未判任何 `pass`**、
> **未把 ADR-001/002 改为已接受**、**未创建生产 crate**（G0 未通过）。
> 全部证据为 `127.0.0.1` 回环，0 个公网报文。`G0` 仍未通过。

### S1 补齐：`tests/spikes/s1` 新增 `cargo run -- two-peer`

按 librqbit 9.1.0 源码核实的 `SessionOptions.listen` / `ConnectionOptions` /
`AddTorrentOptions.initial_peers` / `AddTorrentOptions.ratelimits` /
`ManagedTorrent::wait_until_completed`，建立**两个独立 Session**：

- seeder：1 MiB 确定性样本预置、`ListenerMode::TcpOnly` 绑 `127.0.0.1:0`、上传限速 256 KiB/s；
- leecher：**空目录**、`paused: true` 实例化、`initial_peers = [seeder_addr]`、仅主动出站；
- 两端 DHT / tracker / LSD 全禁，各自独立输出目录；每次运行先清 `.run/two-peer/`，
  避免残留文件把「复制」误认成「传输」。

| 连续 4 轮一致观测 | 值 |
| :--- | :--- |
| leecher 起点 | `progress_bytes = 0` / `is_paused = true` |
| 传输中途（30 ms） | `262144 / 1048576`（16 片，分片整数倍） |
| `pause` + 停留 900 ms | `is_paused = true`，位图仍 262144，不前进不回退 |
| `unpause` → `wait_until_completed` | `finished = true` / `1048576` |
| seeder `uploaded_bytes` | 1048576 |
| leecher 恢复后 `fetched_bytes` | 786432（48 片；`pause` 重建 live state，计数器从恢复点重算） |
| 落盘校验 | 64/64 片 SHA-1 匹配、0 mismatch、逐字节等于确定性样本 |

`probe` 子命令回归无变化（C1/C2 `verified`、C3/C4 保留历史 `partial`）。
**合起来 S1 四条判据 4/4 `verified`**：C1/C2 = `s1-probe-*.json`，C3/C4 = `s1-two-peer-*.json`。
报告 `s1_status` 恒为 `not_run`，`conclusion_rule` 不变。

修掉一处实现错误：seeder 最初把 `ConnectionOptions.enable_tcp` 设为 `false`，
rqbit 构造 stream connector 时报 `no way to connect to peers`；改为 `true`
（seeder 无 peer 来源，启用 TCP 只满足构造约束、不会发起外连）。

### 治理边界：本轮刻意留下的两个动作

1. **不回填 `S1.status`**。`spikes.json` 的 `conclusion_rule` 要求实验执行人按证据手工回填，
   S2 亦是先由项目所有者明确指令「成功后直接判 pass」才回填；本轮无此指令。
   另 `tests/spikes/README.md §6` 第 3 条规定「本地模拟结果不能单独支撑 `pass`」——
   `two-peer` 是真实协议栈的标准 Peer Wire（非逻辑预演），但两端同主机回环，
   **是否满足该条须由实验执行人认定**，或要求补第三方 BT 客户端 / 跨主机一轮。
2. **不定稿 ADR**。[ADR-001](docs/02-architecture/adr/ADR-001-kad-network.md) 明文要求
   「定稿与 `public`/`private` 的签署由项目所有者执行」；[ADR-002](docs/02-architecture/adr/ADR-002-bt-engine.md)
   的「S1 通过 → 采用 `rqbit`」决策树已具备条件，但依赖上面的 `S1.status` 回填与架构组签署。
   按 [ADR 索引 §4](docs/02-architecture/adr/README.md)，**已接受的 ADR 不修改正文**，
   故待定阶段只补正文、不改状态。

顺带记录一处口径冲突：[ROADMAP M0 交付物](docs/06-plan/ROADMAP.md) 写「ADR-005 定稿」，
而 ADR-005 自身复核时点为 M4；按 ADR 自身时点以 M4 为准，M0 不强制定稿。

### 文档同步

| 文件 | 变更 |
| :--- | :--- |
| `tests/spikes/s1/README.md` | 重写为两份报告的分工、two-peer 观测表、证据边界与回填清单 |
| `tests/spikes/spikes.json` | `S1.observations.two_peer` 证据路径、`criteria_coverage` 四项更新、`blocked_by` 清空、`status_note` 开头矛盾口径修正并追加本轮记录 |
| `tests/spikes/README.md` | 首行状态修正、§4 补完成记录、§6 新增回环证据口径待认定 |
| `ADR-002` 1.1 → 1.2 | 新增「S1 判据补齐」章节、更新决策与复核时点，**状态仍「待定」** |
| `DOC-05` ADR 索引 2.1 → 2.2 | ADR-002 行备注、新增「G0 待签署项」注（含 ADR-005 口径差异） |
| `DOC-04` TECH-STACK 2.1 → 2.2 | §2 新增 S1 判据补齐段、`crate.rqbit` 阻塞解除 |
| `DOC-14` ROADMAP 2.5 → 2.6 | §6 标题改为「起始 11-09 / 更新至 11-11」、补历史演化链说明、新增 S1 与「G0 剩余缺口清单」、修正「未开始」与历史排期中的过期表述 |
| `tests/spikes/m0_preflight.py` | 新增 `g0_pending_spikes` 字段与终端「G0 未结 Spike」行，把 G0 的真实判据（S1–S8 全部有结论）与 3 个工程阻塞项分开呈现；S1 的下一步提示改为动态读取 `S1.observations.two_peer`。`g0_ready` 判定逻辑未改 |

**G0 剩余缺口**：① S1 状态回填（治理动作，技术判据已齐）；② S3–S7 五项实验未开工；
③ ADR-001/002 定稿与所有者签署；④ 技术路线冻结评审。
`m0_preflight.py` 报的 3 个阻塞项偏 T0/G1 工程接线，装齐它们不能替代 G0 判据。

## [2.4.31] - 2026-11-11 — ADR-011 L2 落地：**源字典 + 最小 eD2k TCP 栈，索引面/传输面双闭环（LAN/回环）**

> 范围声明：本轮**没有公网报文**、**没有改 `spikes.json`**、**没有判任何 pass**、
> **没有创建生产 crate**（G0 未通过）。所有证据为 RFC1918 / `127.0.0.1`，
> 报告固定 `public_network_contacted=false` / `evidence_valid_for_pass=false`。

### Q9 / V-04 结案：两个候选 crate 都不是可复用的传输栈

- crates.io `ed2k-rs` **不存在该 crate**（API 返回 `crate ed2k-rs does not exist`）；
  GitHub `runfalk/ed2k-rs` 为 alpha、仅哈希、无 AICH、无传输栈，最后 push `2023-08-28T16:28:05Z`。
- crates.io `ed2k 1.0.1` 同样仅哈希，最后更新 `2023-05-19T22:55:00.744168Z`；
  提供 Red / Blue 变体，**默认别名 `Ed2k` = Blue**。
- 结论：v1 保留仓内可审计自实现，不引入停更依赖（R-12）；最小栈「自研 vs 复用」已答 = 自研。
  **Q9 从 [OPEN-QUESTIONS](docs/06-plan/OPEN-QUESTIONS.md) 待决表移出**，V-04 标记「✅ 已确认」并补 §3.1 核实结论。
  credit 缺失代价（R-15）仍留 M1，未随 Q9 结案。

### 源码核实连带修掉一个会导致「死条目」的哈希缺陷

L1 的 63 B 探针从未跨过 `PARTSIZE` 边界，掩盖了精确整数倍大小时的 Blue / Red 分歧。
aMule `6a110563` 采用 **Red**（`KnownFile.cpp::SetFileSize` 样本、
`ThreadTasks.cpp::CreateNextPartHash` 的 `g_emptyMD4Hash`、`KnownFile.cpp::CreateHashFromHashlist`）：
`size % 9_728_000 == 0` 时追加 `MD4("") = 31D6CFE0…`。
`kad-mini/src/wire.rs::ed2k_hash` 已修正、新增 `ed2k_part_hashes` 与上游已知答案向量回归，
并反转了 L1「精确整数倍不补空分块」的旧断言。若沿用 Blue，A 轨会生成「搜得到、永远下不动」的 `fileID`。

### 代码（全部在 `tests/spikes/s2/`，未触碰 `src/crates/`）

| 位置 | 内容 |
| :--- | :--- |
| `kad-mini/src/wire.rs` | `0x44` / `0x34` 载荷编解码、`source_tags`、源 tag 常量、Red 边界修正、MD4 中一组遗留重复 `g` 计算清理（消除编译告警，行为不变） |
| `kad-mini/src/main.rs` | `l2-source-probe` 子命令：RFC1918 硬校验 → BOOTSTRAP → `0x43`+`0x44` 双轨发布 → `0x33`+`0x34` 双轨取回 |
| `ed2k-mini/`（新 spike crate，零依赖） | `wire.rs` 标准 eD2k TCP 帧/HELLO/文件请求/hashset/队列/分片；`stack.rs` 下载与上传共用一份状态机 + 单槽最小上传队列；`main.rs` `selftest` / `loopback` / `lan-hello`；`README.md` |

源码核实要点（详见 PROTO §2.2）：`0x44` 载荷**没有** `u16` 条目数、tag 数 1 字节、
距离校验用 `fileID`、缺 `TAG_SOURCETYPE (0xFF)` 会静默丢弃且不回 `PUBLISH_RES`、
源 IP/UDP 端口取自 UDP 报头；eD2k TCP 帧为 `<protocol u8><len u32 LE><opcode u8><payload>`，
`OP_REQUESTPARTS` 是**先 3 个 start 再 3 个 end**（不是交错对），HELLO tag 与 KAD `ReadTag` 同布局。

### 首轮证据（2026-11-11 复跑一致）

- `kad-mini l2-source-probe`（`172.16.29.209:41296`，真实 aMule）：两笔 `PUBLISH_RES load=1`
  （源发布 65 B / 5 tags、key 回显匹配）、`0x33` 与 `0x34` 各 1 条答案、
  `our_entry_returned=true` / `our_source_returned=true`，
  verdict = `confirmed_dual_track_keyword_and_source_publish_then_retrieve`。
- `ed2k-mini loopback`：3/3 用例完整闭环（4 096 B / 恰好 1×PARTSIZE 的 Red 空分块 / 多分块），
  每例 53 个分片内 `hash_matches=true`、`content_matches=true`、`completed=true`。
- `ed2k-mini lan-hello`（`172.16.29.209:31969`，真实 eMule）：`OP_HELLO` 104 B →
  `OP_HELLOANSWER`，nick `https://www.emule-project.org`；`OP_ASKSHAREDFILES` →
  `OP_ASKSHAREDFILESANSWER`，**共享文件数 0**。
- 回归：`kad-mini` `cargo test` **21/21**、`selftest` **48/48**、`loopback` **5/5 发布 + 5/5 命中 + 0 协议错误**；
  `ed2k-mini` `cargo test` **31/31**、`selftest` **26/26**；`build_docs.py --check` 通过。
- 报告：`tests/spikes/s2/reports/kad-l2-source-probe-20261111023613.json`、
  `ed2k-mini-loopback-20261111023642.json`、`ed2k-mini-lan-hello-20261111023647.json`（本地证据，由 gitignore 排除）。

### 证据边界与开放观测（必须跟结论写在一起）

- **握手是真客户端给的，完整分片传输是我们自己两端给的**，二者不可互相替代。这台 LAN eMule
  共享文件数为 0，能力 ②「从旧客户端下载」无法对它实测，M1 须换真有共享文件的对端补测并实测 R-15。
- `OP_SETREQFILEID (0x4F)` 对不可能存在的 fileID：aMule `ClientTCPSocket.cpp:425` 规定应回
  `OP_FILEREQANSNOFIL (0x48)`，真实 LAN eMule 在 4 s（另做 12 s 独立复核）内静默。
  **原样登记为 `open_observations`，未改写成通过或失败**；`0x58` 对未知 fileID 静默则是源码设计行为。
- 源条目由同一进程发回并取回，未验证第三方旧客户端 UI 搜索；关键词 Key 一致性仍是源码核实。
- 公网 `0x43` / `0x44` 发布须执行人**重新显式 attest**，本轮未做、未自动加该参数。

### 文档同步与版本

| 文档 | 变更 |
| :--- | :--- |
| [PROTO](docs/04-protocol/PROTO.md) | **2.4 → 2.5**：新增 §2.2（`0x44`/`0x34` 载荷、eD2k TCP 报文序列、`ed2k_hash` Red 更正、§2.2.4 口径冲突），§4 增「L2 映射（v2.5）」行。**`PROTOCOL_VERSION = 1`、§3 私有唤醒字段与 T0 golden 一字未动** |
| [OPEN-QUESTIONS](docs/06-plan/OPEN-QUESTIONS.md) | **2.0 → 2.1**：Q9 结案移出并留注；新增 **Q10**（`WAKEUP_REQ.capabilities.ed2k` 语义冲突，M1 定稿） |
| [REFERENCES](docs/08-reference/REFERENCES.md) | **2.0 → 2.1**：V-04 标「✅ 已确认」、新增 §3.1 核实结论与来源；§4 补两条流转规则 |
| [TECH-STACK](docs/02-architecture/TECH-STACK.md) | **2.0 → 2.1**：eD2K 行改为「仓内可审计自实现 / 两 crate 仅作哈希参考」；S2 状态段补 L1/L2 探针证据 |
| [RISK](docs/05-quality/RISK.md) | **2.0 → 2.1**：R-02 / R-12 / R-15 按 V-04 与本轮证据更新，明确「单条唯一关键词、0 公网报文」不构成 R-02 拒绝率证据 |
| [TEST](docs/05-quality/TEST.md) | **2.2 → 2.3**：M0 前置段补 L2 两组回归与其证据边界 |
| [ROADMAP](docs/06-plan/ROADMAP.md) | **2.4 → 2.5**：§6 补 L2 落地条目；把 2026-11-09 快照里的 `not_run` 标为「当时值」；生产 crate 仍受 G0 冻结 |
| [DOC-00](docs/README.md) | **2.4 → 2.5**：§7 把「协议任何变更必须递增 `PROTOCOL_VERSION`」收窄为私有契约字段变更，并写明 PROTO §4 的「映射修订例外」，消除两处口径冲突 |
| [ADR 索引](docs/02-architecture/adr/README.md) | **2.0 → 2.1**：ADR-011 行补实施状态指针，并声明按 §4「已接受的 ADR 不修改正文」**不回写 ADR-011 正文** |
| [PLAN](tests/spikes/s2/PLAN.md) | 新增 **§23**（Q9/V-04、Red 修正、`0x44` 索引面、eD2k 传输面、回归、证据边界、M1/M4 下一步）；§22.3 / §22.7 更正；修正文件头与 P4 表格的过期 `not_run` 表述及 §22.7 的断链（`../../docs` → `../../../docs`） |
| `kad-mini/README.md` / `ed2k-mini/README.md` / `tests/spikes/README.md` | 新增 §10（源字典 + Red 更正）、L2 spike 说明、L1/L2 探针边界声明 |
| `.gitignore` / `tests/spikes/s2/.gitignore` | `/target/` → `**/target/`，避免把新 spike 的 Cargo 构建产物入库；把 `*.out.json` / `*.err.txt` / `live-*.json` 等本地运行捕获排除（正式报告仍在 `reports/`） |

**ADR-011 本体未修改**：按 [ADR 索引 §4](docs/02-architecture/adr/README.md)，已接受的 ADR 不修改正文，
实施进度与证据记在 PLAN §23 / 本变更记录 / 各专题文档中；D1–D4、L3 否决与复核时点全部保持原样。

## [2.4.30] - 2026-11-11 — ADR-011 L1 落地：**eMule 关键词字典互通，在真实 aMule 上闭环**

### 源码核实（会改变实现的一条）

`Kademlia.cpp::KadGetKeywordHash` 用的是 `CryptoPP::Weak::MD4(unicode2UTF8(term))` →
`CUInt128::SetValueBE`，**不是 SHA-1**；`UInt128.cpp::ToByteArray` 是其逆，线上 16 字节即 MD4 摘要原值。
函数内不做小写化 / trim / Unicode 归一化。兼容轨与私有轨是两套哈希，不得复用同一函数。
`PUBLISH_SOURCE_REQ = 0x44` 已在 `wire.rs::op` 的源码核实表，但 PROTO §2.0 尚未描述载荷，L2 前须补齐。

### `kad-mini` 新增（探针内，未触碰生产 crate）

- `wire.rs`：`md4`（RFC 1320 纯 Rust，零依赖）、`kad_keyword_hash`、`ed2k_hash`
  （`ED2K_PART_SIZE = 9_500 * 1024 = 9_728_000 B`，单/多分块、~~精确整数倍不补空分块~~）。
  ⚠️ **划掉的边界规则已于 2.4.31 更正为 aMule Red（追加 `MD4("")`），此处保留当时原文**。
- `main.rs`：`ed2k-dict-probe` —— RFC1918 硬校验、BOOTSTRAP 双向可达、
  `0x43` 发布**无前缀**关键词条目（`fileID = 真实 ed2k_hash`）、`0x33` 取回、产出标准 `ed2k://` 链接。
  固定 `public_kad_contacted=false` / `evidence_valid_for_pass=false`，不判 pass。
- `selftest` 新增 5 项（MD4 向量、关键词哈希、eD2k 单分块），`cargo test` 新增 3 项。

### 局域网实测（`tests/spikes/s2/reports/kad-ed2k-dict-probe-20261111012841.json`）

`172.16.29.209:41296`：`PUBLISH_RES load=1`（7.0 ms，Key 回显匹配）、`SEARCH_RES` 1 条答案
（11.7 ms）、`our_entry_returned=true`、返回 `\x01` 文件名 / `\x02` 大小 / `3`（aMule 附加），
verdict = `confirmed_unnamespaced_keyword_publish_then_retrieve`。**0 个公网报文**。

证据边界：本轮证明「aMule 按我们发过去的 target Key 存下并回服务」；
「aMule 自己按关键词算出的 Key 与我们一致」是源码核实，不是运行时验证。
副作用：该 aMule 若同时连公共 KAD，可能为持有的条目做 TTL 续期并重发一次；探针只用唯一无意义关键词。

### 回归与文档

`cargo test` 18/18、`selftest` 42/42、`loopback` 5/5 发布 + 5/5 命中、0 协议错误；
`PROTOCOL_VERSION = 1`、T0 golden 未动。`PLAN §22`、`kad-mini/README §9`。
L2 尚未开工（**当时状态，已于 2.4.31 完成**）：先做 Q9（`ed2k-rs` 复用边界），再补 `0x44` 载荷与最小传输栈；
任何公网发布需执行人重新显式 attest，本轮未做、未自动加该参数。

## [2.4.29] - 2026-11-11 — 所有者决策 D1–D4：**ADR-011 批准 eD2k 双向互操作 L1/L2，否决 L3**

### 审查：撤回上一轮的「唤醒网关」结论

上一轮把「BT 未活跃做种」误当成「文件位于远端休眠节点」，据此推出旧客户端下载冷文件必须经常在线网关。
项目所有者指出 **eD2k 不区分冷热、共享即可见可下载**，核对本项目自己的设计后确认该前提不成立：
[ADR-009](docs/02-architecture/adr/ADR-009-sharing-model.md) 的 `COLD` = 仅一条索引行、无做种上下文；
[LLD §6](docs/03-design/LLD.md) 明确切入时「从磁盘加载元数据」；[ADR-006](docs/02-architecture/adr/ADR-006-metadata-distribution.md)
的元数据 URL 是**本机**；[PRD US-06](docs/01-product/PRD.md) 的守护进程常驻。
文件始终在本机磁盘、节点常在线，eD2k 源只需一个常在线监听读盘发片，与 [LLD §7](docs/03-design/LLD.md)
的 M-10 握手拦截器同构。**L3 是伪需求，予以否决**；节点真下线时源消失，与 eMule 源 TTL 一致，不额外引入机制。

### 四项决策

| # | 决策 |
| :--- | :--- |
| D1 | 所有文件公开可见，共享即公开，不设 per-file 公开开关 |
| D2 | 冷热是 BT 做种状态，不映射到 eD2k 源 |
| D3 | 现在实现标准 eD2k 传输栈；项目里没有「自有传输栈」（P1 零协议侵入，`WAKEUP_REQ` 是控制面 RPC），将来若要加私有优化须另立 ADR 并先证优势 |
| D4 | 法务不作为 L1/L2 前置门禁、可随时修改；S8 / R-11 保持登记，M4 前补书面意见 |

D1/D4 附两条技术事实：已发布到第三方节点的条目按 TTL 存活、**无法即时召回**；PRD 的「不规避版权保护 /
不绕过地域限制 / 不提供匿名化保证」三条非目标不变。

### 源码核实：eMule 关键词 Key 是 MD4，不是 SHA-1

`Kademlia.cpp::KadGetKeywordHash` = `CryptoPP::Weak::MD4(unicode2UTF8(term))` → `CUInt128::SetValueBE`；
`UInt128.cpp::ToByteArray` 是其逆，故线上 16 字节即 MD4 摘要原值。函数内不做小写化 / trim / Unicode 归一化。
与 PROTO §1 的 `SHA1(NAMESPACE_PREFIX + "kw:" + term)` 是两套算法，**双轨不得复用同一哈希函数**。

### 文档同步

- 新增 [ADR-011](docs/02-architecture/adr/ADR-011-ed2k-interop.md)（已接受）：四层能力、双轨发布表、L1/L2 范围、L3 否决理由、D1–D4、风险与复核时点。
- ADR 索引加第 11 项；ADR-004 追加复核注（「v1 不内置 eD2k 栈」的排期部分被取代，哈希转译仍 v2）。
- `PROTO` **2.3 → 2.4**：§1 增 `Emule_Keyword_Key` / `Emule_File_Key`、§2 改双轨 Key、§2.1 两轨合并查询、§4 记映射修订例外。`PROTOCOL_VERSION` 仍 1、T0 golden 未动。
- `PRD §3` 增「索引级双向互通」目标与「共享即公开」注；`SRS` 增 FR-24（关键词检索 + `ed2k://` 链接）/ FR-25（标准 eD2k 下载上传 + 双轨发布）。
- `ARCH` L5 / M-05 / M-12 改为双轨与 eD2k 栈（P0）；`RISK` 更新 R-02 / R-10 / R-14、新增 R-15（零积分排队靠后）；`OPEN-QUESTIONS` 收窄 Q7、D4 注记 Q8、新增 Q9（`ed2k-rs` 复用边界）。

## [2.4.28] - 2026-11-10 — 两进程 A/B 补测通过，**S2 判 `pass`**

### 新增工具（复用同一 `live_gate`，红线不减）

`kad-mini split-probe --stage publish|search`：把 `live` 里同进程的发布与查询拆成两个进程 ——
A（`split-a`）只发布并把「确认地址 + 条目 source id」写进 handoff，B（`split-b`）在**另一个进程**里
用不同源端口、不同本地 KadID 种子、不同 bootstrap 入口重走一遍 tolerance zone，再取回。
未加 attest 时实测被门禁拒绝且 0 报文。

### A 侧（14 分 30 秒 / 324 条报文）

`split-a` = `e465a455…` / 源端口 61195。3 个样本里 1 个收敛到在区（`closest=13,899,520`），
`0x43` → `PUBLISH_RES`（305 ms），handoff 记下 `82.63.191.18:4672` 与 source id `a89b9f21…`；
另 2 个 16 跳后无在区目标，按红线跳过。

### B 侧（5 分 15 秒 / 113 条报文）

`split-b` = `0dab3f81…` / 源端口 **49876** / bootstrap `79.116.53.69:54709`（A 用的是另一台）。
B 自建路由表（`known=194` / `verified=47`）后对同一地址发 `0x33` → `SEARCH_RES`（312.7 ms，2 条答案），
其中一条 source id 与 A 的完全相同，且 descriptor 里写着 `node=split-a;wake_port=61195`。

**判别项**：返回条目的端口是 A 的 61195，B 自己是 49876 —— 条目由 A 创建，B 从第三方索引取回。

### 判定

按项目所有者指令「补一轮 B，成功后直接判 pass」，`S2.status` 登记为 `pass`：

- `tests/spikes/spikes.json`：`S2.status = pass`；新增 `observations.p4_execution_v2427` 与
  `observations.ab_two_process_2026_11_10`（A/B 两侧报告路径、KadID、端口、RTT、判别项）。
- 下游同步：`TECH-STACK §2` 状态注、`ROADMAP` 新增结论条目、`TEST §2`、`RISK R-02`（接纳性成立、污染判据未做）、
  `OPEN-QUESTIONS Q1`（收窄为「按运营速率是否被当污染」，时点改 M4 前）、`ADR-001`（具备定稿条件，状态仍待所有者签署）、
  `tests/spikes/README.md`。
- **判决边界**：只判接纳性（能存、能取）。R-02 的另一半「运营速率下是否被当污染」仍缺实测拒绝率，
  不随本次 pass 关闭。

## [2.4.27] - 2026-11-10 — S2 第七轮：公共 KAD **首次接受并返回**我们的命名空间条目

### 首轮实证（代码零改动，仅重跑）

`live --sample-size 3 --rate-limit 0.2`，G0 参数未动，执行人 attest，门禁 0 blocker。
10 分 53 秒 / 236 条原始记录（`kad-mini/live-run-v2427.json`）。

- `wakeup_req.v1`：`0x21` 收敛到 `closest = 13,899,520 < 2^24`，在区 2 个目标；
  `0x43` → **`PUBLISH_RES`（`load=1`，302 ms）**；`0x33` → `SEARCH_RES`，1 条答案，**命中**。
- `coldrevive-t0-sample.bin`：16 跳后无在区目标，按红线跳过发布。
- `wakeup_ack.v1`：bootstrap `81.33.83.203:4672` 直接超时，0 跳。
- 报文：`0x21` 发 124 / `0x29` 收 101（应答率 81.5%）、`0x43`×2、`0x33`×2、`0x4b`×1、`0x3b`×1。

### 命中已解到底（不是本地回显）

`0xE5` zlib 帧 230 B → 解压 265 B，**265/265 字节零残余**：`target` 与发布的 Key 逐字节相同，
`entry[0].source_id` 与本轮发布条目相同，5 个 tag 全部原样返回，含两个 24 字符自定义名
（`ColdRevive_V1:descriptor`、`ColdRevive_V1:namespace`）。

### 因果证据：同一对端前后对照

`82.63.191.18:4672` 在 v2.4.23 / v2.4.24 对同一 key 的 `0x43` 两次静默；本轮同一台机器、同一 key、
同一份清单，回了 `PUBLISH_RES`。对端、Key、路径都没变，只有 tag 帧头变了（2.4.26）。

### 结论边界（写进 PLAN §20.6–20.7）

- 判据 1/2/4/5/6 满足；判据 3 只有 n=1（1 接受、1 UDP 超时、0 明示拒绝），
  拒绝率 95% 上界 79%，**不足以支撑 R-02「拒绝率 > 20%」的判定**。
- A（发布）与 B（查询）是同一进程、同一 KadID、同一源地址；存储与取回都经第三方节点，
  但"另一台主机能否取回"未测。
- 程序仍固定 `evidence_valid_for_pass=false`（永不自证），`S2.status` 仍 `not_run`，待执行人回填。

## [2.4.26] - 2026-11-10 — 选项 D 定位根因：**Kad tag 帧头格式写错**，五轮 0 回应由此而来

### 定位过程（局域网真实 aMule 当裁判）

- 新增 `lan-index-probe`（`PLAN §18.6` 选项 D）：RFC1918 硬校验 → `BOOTSTRAP` 证明双向可达 →
  `0x43` 发布一个从未存在过的命名空间 Key → `0x33` 取回。`public_kad_contacted=false`，不判 pass。
- 第一次执行：`172.16.29.209:41296` 回 `BOOTSTRAP_RES`（20 联系人 / 15.6 ms），但 `0x43` 仍 `10060`。
  距离 3,428,064,205 虽超 `2^24`，LAN 豁免本应放行 ⇒ 不是公网、不是 firewalled、不是距离。
- 二分（每个变体独立 key）：**0 个 tag 回 `PUBLISH_RES`（load=0），1 个 tag 就静默**；
  单 tag 六种取值（1 字符/43 字符文件名、UINT32、UINT16、长名短值、长名 14 字节）全部静默
  ⇒ 与类型、名字、值都无关，是**帧头**。

### 源码依据

- `Process2PublishKeyRequest` 读 tag 用的是 `bio.ReadTag()` → `CFileDataIO::ReadTag`：
  `type = ReadUInt8(); name = ReadString(false);` —— 名字恒为 `u16 长度 + 字节`，
  且 switch 里**没有** `0x80` 短名分支、**没有** `TAGTYPE_STR1..STR22`、**没有** `TAGTYPE_BLOB`，
  命中外一律 `throw "Invalid Kad tag type"`。
- `FileTags.h`：`#define TAG_FILENAME wxT("\x01")` —— 数值名字在线上就是长度 1 的字符串。
- 我们写的是 ed2k/met 的 `[type|0x80][name:1]`（`CTag::CTag` / `WriteNewEd2kTag` 的格式），
  `0x82` 直接落 `default`；`namespace` 的 14 字节值又被编成 `STR14 (0x1E)`，同样落 `default`。
  异常在 `catch(...) { delete entry; throw; }` 里原样抛出，`PUBLISH_RES` 那一行永远执行不到。

### 修正（v2.4.26）

- `Tag::encode` → `[type][name_len:u16][name][value]`，字符串一律 `TAGTYPE_STRING`，不再自动选 `STRn`；
  `Tag::decode` 同步并显式拒绝 `STRn` / `BLOB`。新增两个单测（字节固定向量 + 拒绝 `0x1E`）。
- `PROTO` 升 **v2.3**：§2.0 静默路径「两条」改「三条」，新增 tag 帧头一行；
  `kad-mini/README.md` §3 把 tag 编码拆成 Kad 路径与 ed2k/met 路径两行。
- 回归：`cargo test` **15/15**、`selftest` **37/37**、`loopback` **5/5 发布 + 5/5 命中**、0 协议错误。

### 实测闭环

同一台 aMule，修好后：`verdict=confirmed_publish_then_retrieve`，`publish_acked=true` `load=1`（5.2 ms），
key 回显匹配；`0x33` 得到 1 条答案（8.5 ms），target 回显匹配、**我们发布的 descriptor 被原样返回**、
`namespace` tag 匹配。即真实 aMule 会收下并服务我们的命名空间条目。

### 更正（撤回）

- **撤回 2.4.24「tag 编码逐字节合法」**：当时对照的是 `CTag::CTag(CFileDataIO&, bool)`（`.met` 路径），
  而 Kad 走 `CFileDataIO::ReadTag`。用 met 的规则解 ed2k 格式当然零残余，但 aMule 不走那个解析器。
- **撤回 2.4.25 §18.5 的 `p^5` 概率论证**：它建立在「报文合法」的前提上，前提已推翻；
  五轮静默的原因是帧头 bug，不是对端 firewalled，也不是公共 KAD 拒绝该命名空间。

### 下一步

重跑公网 P4（G0 参数不变，需执行人重新 attest），预期首次出现 `PUBLISH_RES`；
判据仍为 RISK R-02 三桶计数，`S2.status` 与 `spikes.json` 结论字段仍只由人回填。

## [2.4.25] - 2026-11-10 — S2 第六轮：跳数翻倍（选项 A）被证伪，爬行在 2^23.7–2^25 处饱和

### 执行

- 唯一改动：`MAX_FIND_HOPS` 16 → 32。G0 参数（`sample_size=3` / `0.2 pps` / 明文 `0xE4` /
  跳过 HELLO）一字未改，执行人显式 attest，门禁 `passed=true`。
- 耗时 **31 分 35 秒**、698 条报文（`0x21` 发 375 / `0x29` 收 315，应答率 **84%**）。

### 结果：预期被直接证伪

- 在区目标 **0 / 1 / 0**（§17 用 16 跳拿到 4 个），全轮只有 1 次 PUBLISH，仍 `10060` 超时。
- **同 key 对照**：`fac258d1…` 在 16 跳与 32 跳下 closest 均为 **13,899,520**，
  在区目标仍是同一台 `82.63.191.18:4672`。多给的 16 跳没有拉近 1 个 bit。
- `frontier_widened_hops` 9/13/11（16 跳）→ 19/25/18（32 跳）：绝对数翻倍、比例不降，
  新增跳数全部落在带外盲探上。
- 归因：在区概率 `2^24/2^32 = 2^-8 ≈ 0.39%`，`known` 230–245 的期望在区数 0.90–0.96，
  与实测完全自洽——路由表相对该 key 近似随机，最后 1–2 bit 因末端邻居枯竭而到不了。

### 处置与下一步

- `MAX_FIND_HOPS` **回退为 16**（A 无收益、单轮成本 1.77 倍），常量注释保留本次结论；
  三道过滤（`verified ∧ global IPv4 ∧ top_chunk ≤ 2^24`）未放宽。
- 5 次发布（4 个不同主机）全静默；按 §17.4，非 firewalled 且不超距的节点必然回
  `PUBLISH_RES`，故「对端自判 UDP firewalled」仍是唯一自洽解释（p=0.6/0.7 时全静默概率 7.8%/16.8%）。
  **该混淆不能靠加目标消除**。
- `PLAN.md` 新增 §18，下一步改推选项 D：把 `lan-probe` 扩到 `0x43`/`0x33`，
  用局域网真实 aMule 闭环索引路径（只读证据，`public_kad_contacted=false`，不判 pass）。
- `S2.status` 仍 `not_run`，`spikes.json` 结论字段未回填。

## [2.4.24] - 2026-11-10 — S2 第五轮：前沿护栏生效，首获 4 个在区目标，发布仍 0 回应

### 执行

- G0 参数（`sample_size=3` / `0.2 pps` / 明文 `0xE4` / 跳过 HELLO），执行人显式 attest，
  门禁 `passed=true`。耗时 17 分 53 秒，395 条报文。
- `known` 177–214、`verified` 49–65、`queried` 58–75；`closest_top_chunk` 13.9M–26.6M，
  首次稳定贴在 `SEARCHTOLERANCE`（16,777,216）边界。
- **首次同轮拿到 4 个不同在区目标**并发出 4 次 PUBLISH（`119.112.72.25:39763`、
  `82.63.191.18:4672`、`93.38.125.19:37642`、`85.51.5.149:4673`），**全部 `10060` 超时**；
  4 次 SEARCH 同样超时，0 命中。

### 排除项（离线，未发包）

- **tag 编码逐字节合法**：237 字节 `0x43` 按 `CTag(const CFileDataIO&, bool)` 完整解码，
  5 个 tag 精确消耗 235 字节，零残余、零非法类型；type 在前 + `0x80` 表示 1 字节 name ID
  的顺序与 aMule 一致；tag 计数 u8 与 `bio.ReadUInt8()` 一致。
- **距离判据 chunk 语义正确**：`UInt128.h` 注明 `Get32BitChunk(0)` 取最高 32 位
  （`m_data.u32_data[3 - val]`），与本地 `xor >> 96` 一致；此前怀疑的「按低 32 位判区」不成立。
- **`IsFirewalledUDP` 默认 false**：`m_firewalledUDP = false` 是静态初值，只有节点自己跑过
  UDP 防火墙测试且失败（或超时 6 分钟）才置 true。未测试过的 NAT 后节点会正常处理并回应 PUBLISH，
  因此「公网大多 firewalled 故拒收」的解释强度显著下调。
- **发布响应无条件**：除 firewalled / 超距两条 early return，即使 `AddKeyword` 返回 false 仍发送
  `PUBLISH_RES`（`file(16)+load(u8)`）；第三条静默路径（tag 解析抛异常）已被上条排除。

### 结论与下一步

`0/4` 证明协议实现全链路正确，但不足以判定公共 KAD 是否接纳该命名空间——4 次发布虽为 4 个主机，
仅覆盖 2 个 key，独立试验单位不足。`frontier_widened_hops` 9/13/11 表明 2^24 附近的已知路由表本身稀疏。
`PLAN.md` 新增 §17，列出三个选项（A：跳数 16→32；B：`ALPHA_QUERY` 5→8 + 跳数 24；
C：`sample_size` 3→10，需所有者改 confirmed 值），建议先做 A（纯代码改动）。
`S2.status` 仍 `not_run`，`spikes.json` 结论字段未回填。

## [2.4.23] - 2026-11-10 — S2 第四轮：定位爬行震荡根因，补 Kademlia 前沿护栏

### 定位

- 第四轮（v2.4.22，G0 参数）跑出 448 条报文：`known` 198–231、`verified` 62–70（覆盖修复有效），
  但 `zone_verified` 仍 0/1/0，`closest_top_chunk` 反而更差（38M / 13.9M / 53M）。
- 用该轮原始报文做四项离线复现（未发包）：`0x29` 载荷为 `target(16)+count(1)+25n`（count=11，
  与 aMule 一致，此前「前缀异常」是本人偏移读错）；`check==KadID` 把关使 200 次应答成为 200 次
  真实 KadID 证明；四个 32 位分块中只有 chunk0 有富集（1.14% vs 基线 0.39% = 2.9 倍），端序正确；
  **逐跳还原显示询问距离在震荡**（43M→54M→272M→743M→38M→266M→1342M）。

### 修正

- 根因：v2.4.22 把候选窗口改为「全表最近的未问节点」且**无距离上限**，最近几个联系人 2–3 跳问完
  后只能去够 700M–1400M 的远端节点，形成震荡；报文越多震荡越大，`known` 翻倍而距离变差。
- 新增 `FRONTIER_FACTOR = 8` 前沿护栏：只用「距离 ≤ 8 × 当前最近距离」的未问节点作候选，下限取
  `SEARCHTOLERANCE`；前沿耗尽才放宽一档到全表，并计入新字段 `frontier_widened_hops`。
- 报文预算不变（`ALPHA_QUERY=5`、`MAX_FIND_HOPS=16`），与 v2.4.22 只差选点一个变量。
- `PLAN.md` 新增 §16，并在 §15.5 标注「窗口 30 导致结构性截断」的判断已推翻。

### 回归

`cargo test` 13/13、`selftest` 37/37、`loopback` 3/3 发布确认 + 3/3 命中 + 0 协议错误。
`S2.status` 仍 `not_run`；本轮无 `PUBLISH_RES`。

## [2.4.22] - 2026-11-10 — H2 源码逐字段比对：编码无误、H1 被证伪，且「搜索静默是设计要求」

### 方法

纯离线核对 aMule `master` 的 `CSearch::StorePacket`、`Process2SearchKeyRequest`、
`Process2PublishKeyRequest`、`Defines.h`、`CIndexed::SendValidKeywordResult`。未发送任何报文。

### 结论一：载荷无偏差，H2 不成立

`SEARCH_KEY_REQ` = `target(16) + u16(0)`；`PUBLISH_KEY_REQ` = `file(16) + count(u16) + [sourceID(16) + tagCount(u8) + tags]`；
`SEARCHTOLERANCE = 16777216`、`K = 10`、`ALPHA_QUERY = 5`、距离比较取最高 32 位——**全部与我们一致**。
此前怀疑的「常量误读」也不成立。

### 结论二：H1 被证伪，不需要放宽 `protocol_scope`

两个处理函数都不查路由表、不校验发送方是否为已知 contact。发布的前置条件只有两条：
对端自身 `IsFirewalledUDP(true)`，以及 `top_chunk(对端KadID, Key) > SEARCHTOLERANCE`。
**「必须先 HELLO 互认」不成立**，`HELLO` 路径无须实现。

### 结论三：更正 §14.4 ——搜索静默是设计要求

`CIndexed::SendValidKeywordResult` 把整个响应构造放在 `if (find(keyID) != end())` 内，
miss 时只执行末尾的 `Clean()`，**一个字节都不回**。我们搜的是无人在线的私有命名空间 Key，
所以 `0x33` 的 0/11 与 9 个探针的 0/9 是必然结果。

因此 v2.4.21 的「请求类别级门禁」结论**撤销**：`p = 3.7e-14` 的检验不成立，
因为它比较的两类操作里，search 这一侧的信道本身就是关闭的。

### 代码

- 候选窗口 `ALPHA_QUERY * 6`（=30）→ **整张表**。这是 v2.4.21 的结构性上限：
  第 7 跳退出、`queried = 33`，而 `known` 里还有 138–165 个从未问过的联系人被窗口挡在外面，
  `closest_top_chunk` 因此卡在 ~2^23——正好差一跳跨过 2^24。
- 删除 `index_op_probe`：它测不出任何东西，只会再次误导判读。

### 文档

- `PLAN.md` 新增 §15（逐字段比对表、H1 证伪、三条静默分支、更正与下一步），并在 §14.4 顶部加更正指引。
- `PROTO.md` 升 2.2，§2.0 增三行源码核实结论：「查询必有响应」不成立、「发布必有响应」有两条静默丢弃、
  索引操作不依赖 `HELLO` 互认。这三条直接影响产品侧的超时与冗余策略。

`S2.status` 仍为 `not_run`。

## [2.4.21] - 2026-11-10 — S2 P4 第三轮：验证覆盖修复成功，索引操作 0/19 静默，样本量被证伪为自变量

### 决策：不扩大样本，先修覆盖

「加大样本能否提高成功率」用前两轮数据即可回答：每样本 `verified` 仅 9.2（`known` 却有 86–127），
`P(样本含 ≥1 在区目标) = 0.5`，每样本发布 0.5 次。`sample_size` 3→10 只是把期望发布次数从 1.5 提到 5，
而每次仍有一半概率发不出包。另有硬约束：`live_gate` 要求 CLI 值等于 `confirmed.sample_size`，
`--sample-size=10` 实测被拦（`与 G0 确认值 3 不一致`），改值属所有者确认参数的变更。

### 代码

- **plateau 不再是退出条件**：原先连续两跳零增长即 break，16 跳预算只用 3–5 跳，
  110–130 个联系人里只验证了 ~10 个。现在只在「跳数用尽」或「候选耗尽」时退出。
- **D1 距离诊断**：`closest_top_chunk` / `closest_verified_top_chunk` / `queried_contacts` / `stale_hops`。
- **D2 索引操作探针**：`index_op_probe_*`，向 ≤3 个「已验证据、但不在区」的节点各发一个 `SEARCH_KEY_REQ`（只发查询，不发发布）。
- **应答来源校验**：`exchange` 只接受来自被问地址的报文（最多轮询 4 次）。
  原实现接受任意来源，迟到的 `0x29` 会被解成「expected 0x43, got 0x29」，迟到的 `0x43` 会被记到错误目标头上。

### 执行与结果（`sample_size=3` / `0.2 pps` / 明文 `0xE4` / 跳过 HELLO）

| seq | hops | known | verified | zone_verified | closest_top_chunk | PUBLISH | SEARCH | 探针 |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| 0 | 7 | 165 | 29 | 1 | 9,826,055（在区） | 1 发 0 回 | 0 回 | 3 发 0 回 |
| 1 | 7 | 138 | 27 | 1 | 13,899,520（在区） | 1 发 0 回 | 0 回 | 3 发 0 回 |
| 2 | 7 | 162 | 30 | 0 | 31,927,299（越界） | 跳过 | — | 3 发 0 回 |

覆盖修复生效：单样本 `verified` 8–12 → 27–30（约 3 倍），`known` 86–127 → 138–165。
结果仍为 0：`0x33`×11、`0x43`×2、探针×9，全部静默。

### 关键判读

三轮合计：`0x21`→`0x29` **86/99 = 87%** 应答；索引操作（`0x33`/`0x43`）**0/19 = 0%**。
Fisher 精确检验双侧 **p = 3.7e-14**。同一批活节点、同一源端口、同一加密信封，
回答节点查找而拒绝索引操作 → 这是**请求类别级门禁**，与目标选择、网络、报文可解密性均无关。
**继续加大样本只会重复同一个 0/N。**

### 待决

H1（索引操作要求先 `HELLO` 互认）与 H2（`0x33`/`0x43` 载荷与 aMule 解析器不一致，
回环自洽无法证伪）均需触碰 `HELLO` 路径，而 G0 确认的 `protocol_scope` 明写「跳过 HELLO」，
需项目所有者先书面放宽。`S2.status` 保持 `not_run`。

## [2.4.20] - 2026-11-10 — S2 P4 两轮实网执行：目标筛选生效，但在区节点策略性不应答

### 执行

两轮均按 G0 参数（`sample_size=3` / `0.2 pps`）、执行人显式 attest，明文 `0xE4`，bootstrap 10 节点。

| 轮次 | seq | known | verified | zone_verified | hops | PUBLISH |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| v2.4.19 | 0/1/2 | 103 / 88 / 114 | 8 / 8 / 9 | 0 / 1 / 1 | 3 / 3 / 2 | 0 确认（2 次 `10060`） |
| v2.4.20 | 0/1/2 | 113 / 86 / 127 | 11 / 7 / 12 | 0 / 1 / 0 | 5 / 3 / 4 | 0 确认（1 次 `10060`） |

`SEARCH_KEY_REQ` 与 PUBLISH 同结果：全部超时，0 命中。

### 代码（v2.4.20 相对 v2.4.19）

- `MAX_FIND_HOPS` 10 → 16；新增 `MIN_ZONE_TARGETS = 3`：单目标时一个策略性丢弃就等于
  零信息样本（v2.4.19 两次都撞上这一点）。
- 每跳候选改为「**未查询过的**最近 `ALPHA_QUERY` 个」：原先取原始最近 N 个会被 `seen` 跳过，
  整跳预算可能全花在已问过的节点上并误判为 plateau。
- plateau 改为**连续两跳零增长**才停。

### 观测与判读

- 目标筛选生效：不再盲发最近 10 个，`known` 由 30–34 提升到 86–127，`verified` 7–12。
- 在区命中仍为 0–1，与「按随机联系人」的期望一致（127/256 ≈ 0.5），说明漏斗尚未真正贴近 key。
- 同一个在区节点 `82.63.191.18:4672` 两轮均「答 `0x21`、不答 `0x43` 与 `0x33`」——
  非死节点，属**策略性不应答**索引操作。
- 因此 0/N **不属于索引层拒绝，也不属于报文问题**，而是目标侧服务策略；详见
  [PLAN.md §13](tests/spikes/s2/PLAN.md)。

### 状态不变

`S2.status = not_run`；两次 PUBLISH 均未获得 `PUBLISH_RES`；0 命中；`PROTOCOL_VERSION = 1`；
T0 golden 未动；`spikes.json` 结论字段未回填（只由人填写）。下一步 D1（距离诊断）/
D2（陌生节点搜索对照）见 PLAN §13.3。

## [2.4.19] - 2026-11-10 — S2 发布目标筛选：只发给「已验证在责任区内的应答者」

### 依据

v2.4.17 实网记录：seq0 / seq1 **已收敛**但 publish / search **0/10 全超时**，同版本回环 3/3 + 3/3 证明
wire 正确。结论：0/10 不是 codec 问题，而是目标选择问题 —— 「取 known 里 XOR 最近的 10 个」必然混入
① KadID 陈旧（eMule 换 ID 后仍应答，aMule 以 `check` 是否等于自身 ID 判定，不等即静默丢弃）与
② UDP firewalled（`Process2PublishKeyRequest` 直接丢弃）两类不接收发布的节点。

### 改动

- `live` 新增 `verified: HashSet<SocketAddr>`：任何成功的 `0x21 → 0x29` 即证明该地址真实 KadID
  等于其广告 ID（`check` 必须等于接收方自身 KadID，否则 aMule 静默丢弃）。
- 发布 / 搜索目标改为 `verified ∩ 全局 IPv4 ∩ top_chunk(id,key) ≤ SEARCHTOLERANCE`，上限
  `PUBLISH_FANOUT`；删除「最近 N 个」兜底。找不到时**跳过样本并写明原因**，不盲发。
- 循环条件改为「直到出现 verified 在区节点或路由表停止增长」，配合 2.4.18 的 plateau 修正。
- 报告新增 `verified_contacts`、`zone_verified_contacts`、`publish_target_policy=verified_in_zone`。
- [PLAN.md §12](tests/spikes/s2/PLAN.md) 记录依据、改动、判据口径与残余风险。

### 回归

`cargo test` 13/13、`selftest` 37/37、`loopback` 3/3 发布确认 + 3/3 命中 + 0 协议错误；
`live --check` 唯一 blocker 仍为执行人确认。

### 状态不变

`S2.status = not_run`；本轮**未发送任何报文、未接触公共 KAD**；`PROTOCOL_VERSION = 1`；T0 golden 未动。

## [2.4.18] - 2026-11-10 — 法务送审材料包 + 状态流转矩阵；修 S2 收敛自截断与测试编译错误

### 文档

- 新增 [DOC-21 法务评审材料包](docs/07-operations/LEGAL-REVIEW.md)（`草稿`）：汇编
  [TECH-STACK §2 S8](docs/02-architecture/TECH-STACK.md)、[OPS §6](docs/07-operations/OPS.md)、
  [RISK R-11](docs/05-quality/RISK.md)、[Q8](docs/06-plan/OPEN-QUESTIONS.md)；固化 KAD **实发**描述符字段
  （明文文件名 / 文件大小 / 源端口 / descriptor / namespace）、七项默认配置实体、L1–L6 六个待法务确认问题、
  结论登记区与 G0/G4 对应关系。
- [docs/README.md](docs/README.md) 新增 **§4.1 状态流转矩阵**：明确「已审定」不是合法状态值、终态为
  `有效`；列出四条流转边与必要条件；规定改状态须**原子同步四处**（front matter + 正文状态行、
  本文件 §3 表、`index.html` 的 `DOCS`、`CHANGELOG`）。
- `docs/index.html` 的 `DOCS` 清单同步增补 DOC-21。

### M0 / S2 代码

- **修测试编译失败**：`deobfuscate` 在 2.4.17 改为返回 `(inner_frame, peer_verify_key)`，两处 `assert_eq!`
  未同步，`cargo test` 直接 `E0308` 编译错误 —— **13 个用例一条都跑不起来**，2.4.17 声称的
  「`cargo test` 通过」无法复现。已改为比较 `back.0`。
- **修收敛自截断缺陷**：`live` 的 plateau 判据原本建在 `in_zone` 上。未收敛时 `in_zone` 每跳恒为 0，
  `hops >= 2` 立刻 break —— 2.4.17 设的 `MAX_FIND_HOPS = 10` 实际**永远只走 2 跳**，这正是两次 P4
  重跑停在 30–34 联系人的原因。改为以路由表 `known` 是否增长为 plateau 判据，循环条件改为
  `in_zone.is_empty()`，`SEARCHTOLERANCE` 漏斗才真正可走。
- 回归：`cargo test` **13/13**、`selftest` **37/37**、`loopback` **3/3 发布确认 + 3/3 命中 + 0 协议错误**。
- `live --check` 现只剩一个 blocker：`--attest-i-am-the-experiment-operator`；
  `g0_confirmed sample_size=3 / rate_limit_pps=0.2`、bootstrap 10 节点公网 IPv4 校验通过。

### 状态不变

`S2.status = not_run`；本轮**未发送任何报文、未接触公共 KAD**；`PROTOCOL_VERSION = 1`；T0 golden 未动；
S8 的所有者豁免未变为书面法务意见。

## [2.4.17] - 2026-11-10 — 多目标扇出 + 混淆信封字节级对齐（补记）

> 本条为补记：提交 `53c87f7` 当时未写入本文件。

- 收敛参数：`MAX_FIND_HOPS = 10`、`ALPHA_QUERY = 5`、`KADEMLIA_FIND_NODE = 0x0B`、`PUBLISH_FANOUT = 10`。
- publish / search 改为向最近的 K 个已知节点多目标扇出，避免单点陈旧 KadID 导致的静默丢弃。
- 新增 `obfuscate` / `obfuscate_verify` / `deobfuscate` 混淆信封，字节级对齐 aMule（回环 3/3 发布确认、
  3/3 搜索命中、0 协议错误）。
- 遗留：`deobfuscate` 返回值改为元组后测试未同步，`cargo test` 编译失败，已于 2.4.18 修复。

## [2.4.14] - 2026-11-10 — 根因归位：联系人 IP 字节序缺陷，高位 UDP 从未被阻断

> **本条撤回 2.4.9–2.4.13 的主要环境结论。** 它们保留为审计轨迹，不再作为决策依据。

### 触发证据

项目所有者提供 LAN eMule 主机**运行时**三张截图：当日 11:12 新加的下载以 MB/s 传输、
展开来源含多行 `Kad` 与 `eD2K Se...`；Kad 页签 `当前搜索 (1)` 关键字 `windows`、
**数量 93 / 已发送包 7|2 / 回应总数 600**；`当前搜索 (2)` 两条 `节点查找` 回应总数 2 与 1。
按 [eMule 官方 Kad 窗口](https://www.emule-project.com/home/perl/help.cgi?l=1&topic_id=1266&rm=show_topic)
与[搜索详情图](https://www.emule-project.com/home/perl/help.cgi?l=1&rm=show_topic&topic_id=1552)，
这些是远端节点经 UDP 送回的应答 —— 与「整网过滤高位 UDP」不相容。

### 根因：把联系人变成 UDP 目的地时读反了 IP 字节序

| 源码 | 事实 |
| :--- | :--- |
| `src/NetworkFunctions.h` | `KadIPToString(ip)` 以 `ip>>24` 为首字节 → KAD 内部 IP 为 **host order** |
| `src/KadDlg.cpp` | 注释「StringIPtoUint32 returns anti-host and **kad expects host order**」 |
| `src/SafeFile.cpp` | `WriteUInt32` / `ReadUInt32` 在 x86 上是**小端** |
| `KademliaUDPListener.cpp` | `SendPacket` / `OnPacketReceived` 用 `wxUINT32_SWAP_ALWAYS` 双向转换 |

故 `a.b.c.d` 的 host-order 整数以小端写出后，线上 4 字节是 **`d.c.b.a`**。真实
`nodes.dat`（200 条）反序解码后 **200/200 全局可路由**、首字节集中在宽带 ISP 段；
原样解码则有 19 条 RFC1918 与 225/236/246/253 等组播/保留首字节。自 P4 首次执行起，
所有 0/N 都是把 `BOOTSTRAP_REQ` 发给了不存在的地址。

另核实 `Process2BootstrapRequest` 不要求 payload / KAD ID / HELLO / UDP key，
我们 2 字节 `0xE4 0x01` 报文**本身合法**，不是报文格式问题。

### 修复与复验

- `nodes_dat_to_bootstrap.py`、`bootstrap_res_to_targets.py` 新增 `decode_kad_ip()`
  反转 4 字节并加 `--selftest`；`kad-mini/src/wire.rs` 的 `Contact::encode/decode`
  统一「内存点分顺序 / 线上反序」，新增 `contact_ip_wire_order_is_reversed` 单测。
- 重新生成三份 gitignored 目标清单；`cargo test` **8/8**、两个脚本 selftest 通过。
- 修正后本机 `kad_egress_probe.py` 10 目标 / 0.2 pps：**9/10 收到 523 B `BOOTSTRAP_RES`**、
  `RESULT: REACHABLE` 退出码 0（1 条 `WSAECONNRESET`）。**高位 UDP 一直是通的**；
  STUN 3478/19302 超时只是 STUN 专属过滤，不能当 KAD 代理指标。
- P4 按 G0 参数（`sample_size=3` / `rate_limit_pps=0.2`）**两度重跑**：
  `public_kad_contacted=true`，收到 517 B `0xE5` zlib 压缩 `BOOTSTRAP_RES`、
  20 联系人及 `0x21/0x29` 迭代应答，3 hops / 30–34 联系人；两次均
  `tolerance_reached=false` → 3/3 样本按 criteria #2 跳过，**未发布、未搜索**。

### 撤回与保留

- **撤回**：「出口按目标端口/应用白名单过滤高位 UDP」「整网无主机可跑 P4」
  「本机已确证不可达」「换网重跑 P4」；RISK R-02 中历史超时由「环境前置条件失败」
  改判为**测错对象**，整体作废、不进入任何桶。
- **保留**：STUN ≠ KAD；KAD UDP「打开」是入站可达性测试、入站 DNAT 即可满足；
  eD2k 下载/服务器搜索走 TCP，与 KAD UDP 是不同判据。

### 新的关键环节

瓶颈转移到客户端的 **`SEARCHTOLERANCE` 路由收敛**。`live` 每跳只取 2 个候选、最多 3 跳，
累计 30–34 联系人；aMule `SEARCHTOLERANCE = 2^24` 比较 XOR 距离**最高 32 位**
（`UInt128.h` 的 `Get32BitChunk(0)` 为最高位），随机命中约 1/256。
`Constants.h` 定义 `KADEMLIA_FIND_VALUE=0x02` / `STORE=0x04` / `FIND_NODE=0x0B`、
`ALPHA_QUERY=5`；当前 `0x21` 首字节用了非标准的 5。修复方向：每跳 `ALPHA_QUERY`
个最近候选 + `FIND_NODE=0x0B` + 提高跳数，再用实时 `BOOTSTRAP_RES` 目标重跑 P4。
`nodes.dat` 已过期（3 条中 2 条无响应），不再作为首选清单。

### 登记范围

`PLAN.md` 新增 §11（含撤回表）并改顶部状态；`spikes.json` 新增
`p4_environment_diagnosis.contact_ip_byte_order_defect_2026_11_10`，改写
`conclusion` / `required_next_step`，并把 `p4_execution.result` 改为
`inconclusive_targeting_defect`；RISK R-02、OPEN-QUESTIONS、ROADMAP、根 README、
各 README、两个诊断脚本文档字符串与提示语、`docs/index.html` 重建。公共文档不含
LAN IP / 端口 / 公网 bootstrap IP。

**状态不变**：`S2.status = not_run`、`PROTOCOL_VERSION = 1`、T0 golden 未动、
两次 P4 重跑 `evidence_valid_for_pass=false`、**未发布任何测试 Key**、G0 仍 False。

## [2.4.13] - 2026-11-10 — 跨主机 KAD 探针 0/3 + eD2k/KAD 判据区分：工程决策改为换网重跑

### 新增证据

- 项目所有者在 LAN eMule 机器（同网段、开了入站映射）执行 §8.6.3 的
  [`kad_egress_probe.py`](tests/spikes/s2/kad_egress_probe.py)：3 个目标全部
  `TimeoutError: timed out`，`RESULT: NO_REPLY (0/3)`、退出码 2。主机身份经确认；
  本机 Desktop 无该脚本 / 目标文件 / 报告目录，报告路径在该机 `Desktop\reports\`。
- 所有者补充「该机 eMule 下载和搜索都很正常」，并提出可现场开下载验证。
- 新增 [`bootstrap_res_to_targets.py`](tests/spikes/s2/bootstrap_res_to_targets.py)：
  只读解析 LAN `BOOTSTRAP_RES`（`0xE4`/`0xE5` + `0x09`，20 × 25 B 联系人），
  过滤公网 IPv4、去重、产出新鲜目标；**不发任何包**。03:12 UTC 重跑 `lan-probe`
  得 523 B / 20 contacts / RTT 12 ms，解出 15 个公网 IPv4，取前 10 个
  （5 × `4672` + 5 × 其他高位端口），与旧 3 目标**重叠 0/10**；本机宽扫 **0/10**。

### 两处方法学修正

1. **两次 0/3 不是独立复现**：本机（三种源端口）与该机共用同一份 3 个目标，
   换主机 / 换源端口但没换目标 —— 若这 3 个联系人已离线，两台主机都会 0/3。
   跨主机结果压低了「按主机例外」，没有压低「死联系人」；新鲜 10 目标宽扫把
   本机侧这一混淆大幅削弱。
2. **「下载/搜索正常」不构成 KAD 出站证据**：按 eMule 官方
   [Network Guide](https://www.emule-project.com/home/perl/help.cgi?l=1&rm=show_topic&topic_id=232)
   与 [Search → Method](https://emule-project.com/home/perl/help.cgi?l=1&rm=show_topic&topic_id=133)，
   下载本身「与两个网络的选择无关」、走 TCP；`Server` / `Global (Server)` 搜索逐个查询
   eD2k 服务器（TCP）；只有 `Kademlia` 方法要求 UDP 通（官方原话：热门关键词都无结果时
   「系统的 UDP 端口很可能被防火墙或路由器阻断」）。HighID 也只证明 TCP 回连 + 入站 DNAT。
   因此一条完整健康的 eD2k 路径可以**完全不产生出站高位 UDP**，与 0/3、STUN 0/3 不冲突。
   → 验证应改为「搜索 Method 明确选 `Kademlia` 搜热门词」，**不必开下载**。
   另注意入站 KAD 请求触发的 UDP 回包属已建立流，计数增长不是充分证据。

### 结论

**工程决策与科学归因解耦**：本网没有任何独立客户端进程被证明能完成一次公网 KAD UDP
往返（本机 0/3 三种源端口 + 0/10 新鲜目标；该机 0/3）。无论剩余假设
① 按应用放行 / ③ 另一出口或代理 / ④ 出站统一过滤 + DNAT 产生「打开」哪个为真，
ColdRevive 自己的客户端在本网都跑不了 P4 —— 差别只在「换网」还是「为本项目进程单独
申请出站例外」。**P4 应换网重跑，不再等待归因收敛**；Kademlia 方法搜索、该机新鲜宽扫、
统计计数只用于收窄归因，不阻塞。

修订：[PLAN.md §10](tests/spikes/s2/PLAN.md)（新增）、顶部状态、§8.6.2/§8.6.3、§9.3；
`spikes.json` 新增 `cross_host_kad_probe_2026_11_10` 并修订 `conclusion` /
`cross_host_result` / `decisive_checks` / `required_next_step`；RISK R-02、
OPEN-QUESTIONS、ROADMAP、根 README 与各 README、两个诊断脚本的说明与提示语、
`docs/index.html` 重建。公共文档不含 LAN IP / 端口 / 公网 bootstrap IP。

状态不变：`S2.status=not_run`、`PROTOCOL_VERSION=1`、T0 golden 未动、三桶整体作废、
未发布任何测试 Key；本节全部观测 `evidence_valid_for_pass=false`。

## [2.4.12] - 2026-11-10 — `Kad 网络` 面板证据：否证 firewalled 假设，但「打开」是入站测试

项目所有者补上 2.4.11 要求的第 ① 项：LAN eMule 主机 `Kad 网络` 面板完整截图，
`状态: 打开`、`UDP 状态: 打开`、`IP:端口: <公网 IPv4>:<端口被裁切>`、`ID: <数字 ID>`，
**无 `防火墙` / `Buddy` 行**（公共文档已脱敏）。eMule 官方帮助把 KAD 状态区分为「打开」与
「通过防火墙」，aMule 官方手册把 `Connection State` / `UDP Connection State` 定义为
`OK` / `Firewalled` 且两者均 OK 时不显示 Buddy 行。

### 否证与更正

- §8.6.2 假设 ②「该机 KAD 已 firewalled / 有 Buddy、432 条为陈旧路由表」**被否证**；
  v2.4.10「更可能是 LowID / firewalled」的推测撤回；[PLAN.md §8.2](tests/spikes/s2/PLAN.md)
  对应表述同步改写。
- v2.4.10 的「整网无主机可跑 P4」仍维持 2.4.11 的降级结论，**不因面板而回升**。

### 源码级复核：为什么「打开」不等于能跑 P4

按 §0 同一 aMule 源码族逐处核实（详见 [PLAN.md §9](tests/spikes/s2/PLAN.md)）：

| 位置 | 事实 |
| :--- | :--- |
| `ClientList.cpp::DoRequestFirewallCheckUDP` | 只用联系人 **TCP 端口**建连（`KS_QUEUED_FWCHECK_UDP` → `TryToConnect`），**不发** UDP 到其 KAD 端口 |
| `OP_KAD_FWCHECKUDP_REQ` 处理 | 远端把一个**无载荷入站 UDP 包** `KADEMLIA_FWCHECKUDP_RES` 发到请求方 `GetIP():GetKadPort()` |
| `KademliaUDPListener.cpp::Process2FirewallUDP` | 端口匹配即 `SetUDPFWCheckResult(true, false, ...)` |
| `UDPFirewallTester.cpp` | `m_firewalledUDP` **初值即 `false`**；全失败才置 `true`；6 分钟超时降级还要求 `CKademlia::IsFirewalled()` |

→ KAD UDP「打开」测的是**入站可达性**，由入站 DNAT 即可满足，协调走 TCP，**无需任何出站
高位 UDP**。因此「该机 STUN 高位 UDP 全超时」与「KAD UDP 打开」并不矛盾，不必引入按应用 /
DPI 放行；「打开」也可能是未核实的乐观默认值或历史状态残留。432 条联系人是路由表而非流量
计数，面板公网 IP 亦可由 eD2K HighID 提供，均不证明发生过公网 KAD UDP 往返。

### 假设表与决定性核实

[PLAN.md §9.2](tests/spikes/s2/PLAN.md) 更新为：① KAD 真活（仍开放）、② firewalled/Buddy
（**否证**）、③ 另一条出口 / 代理（仍开放）、④ 新增：出口统一过滤出站高位 UDP + 入站 DNAT
产生「打开」（无法排除，与全部观测一致）。

仍需项目所有者：① 在 LAN eMule 机器运行
[`kad_egress_probe.py`](tests/spikes/s2/kad_egress_probe.py)（退出码 `0` = 充分证据；`2` 在存在
按应用放行时**不是充分否证**）；② 查看 `统计信息` 中 KAD 收发包计数是否增长。计数增长 +
探针 `2` 表示按应用 / DPI 放行；计数不增长 + 探针 `2` 才支持本网无可用出口。

### 变更与状态

`PLAN.md` 新增 §9（面板转录、源码表、假设表、核实步骤），修订顶部状态 / §8.2 / §8.6.2；
`spikes.json` 新增 `observations.p4_environment_diagnosis.kad_panel_evidence_2026_11_10`，
修订 `status_note` / `conclusion` / `cross_host_result.implications` /
`kad_specific_egress_2026_11_10` 的假设表与决定性核实 / `required_next_step`；RISK R-02、
OPEN-QUESTIONS、ROADMAP、各 README 同步；`docs/index.html` 重建。

**状态不变**：`S2.status = not_run`、`PROTOCOL_VERSION = 1`、T0 golden files 未动、
三桶计数整体作废、未发布任何测试 Key；本面板为环境诊断证据，`evidence_valid_for_pass = false`。

## [2.4.11] - 2026-11-10 — 新证据：STUN ≠ KAD，归因收窄并新增 KAD 专用探针

项目所有者提供 LAN eMule 主机截图：eD2K 服务器连接为 **HighID**，Kad 页签 **节点 (432)**、
联系人版本 8/9/10。同时本机 `lan-probe` 早先已从该客户端 `BOOTSTRAP_RES` 解出 20 个联系人
（17 个公网 IPv4，多条使用标准 `4672`），与 2026-11-08 `nodes.dat` 中 P4 用的 3 个联系人
完全不重叠。这使 v2.4.10 的绝对措辞不再成立：**STUN 超时只能说明 STUN 被过滤，不能推出
KAD 报文也被过滤**；出口策略可能按应用 / DPI、按目标端口或按「主机 IP + 源端口」放行。

### 概念澄清

HighID 是 eD2K 服务器分配的客户端 ID，证明 DNAT 让服务器能回连 TCP；KAD 有独立的
「状态 / 防火墙 / Buddy」，截图里 `Kad 网络` 面板被裁掉。Kad 页签 432 条是**路由表联系人**
（`nodes.dat` 载入 + 入站请求学习），不等于此刻存在双向 UDP 流量；`0(8)` 这类值是联系人
类型与 KAD 协议版本，不是 HighID。

### 新增诊断与本机实测

新增 [`kad_egress_probe.py`](tests/spikes/s2/kad_egress_probe.py)：只发 2 字节 `0xE4 0x01`
`BOOTSTRAP_REQ`，不发布 Key、不发 HELLO / REQ / SEARCH；目标由用户提供且硬校验为公网 IPv4、
不内置；G0 参数 3 条 / 0.2 包每秒、逐次留原始报文；报告写入 gitignored `reports/`。

| 本机源端口 | KAD 响应 |
| :--- | :--- |
| 系统临时端口 | **0/3** |
| 固定为 LAN eMule 的 KAD 源端口 | **0/3** |
| 固定标准 eMule UDP 端口 `4672` | **0/3** |

→ **本机确实无法与公网 KAD 节点完成一次往返**，不是「只有 STUN 不通」、不是「P4 只挑到死
联系人」、不是源端口策略；§7 对本机的环境阻断结论成立。

### 归因收窄与待核实

[PLAN.md §8.6](tests/spikes/s2/PLAN.md) 记录三种未排除的可能：该机 KAD 确实存活（出口按应用 /
按 IP+源端口放行，STUN 是假阴性）、KAD 已 firewalled 且 432 条为陈旧路由表、eMule 走另一条
出口 / 代理。`spikes.json` 新增 `kad_specific_egress_2026_11_10`，并把
`cross_host_result.conclusion` 与 `required_next_step` 改为非绝对表述；OPEN-QUESTIONS /
ROADMAP / 各 README / RISK R-02 同步；`docs/index.html` 重建。

**v2.4.10 的「整网无主机可跑 P4」降级为「本机已确证不可跑；LAN eMule 主机待核实」**。
决定性核实：① 提供 `Kad 网络` 面板完整截图（状态 / 防火墙 / Buddy / 节点数）与 KAD 收发包
计数是否增长；② 把 `kad_egress_probe.py` 与本地 `bootstrap.live.local.json` 拷到该机运行，
退出码 0 / 2。

**状态不变**：`S2.status = not_run`、`PROTOCOL_VERSION = 1`、T0 golden files 未动、
三桶计数整体作废、未发布任何测试 Key。

## [2.4.10] - 2026-11-10 — 跨主机 STUN 对照：本机与 LAN 主机同被阻断

项目所有者把同一份 [udp_egress_check.py](tests/spikes/s2/udp_egress_check.py) 拷到 LAN 那台
eMule 机器（与本机同网段、同一网络环境，**开了入站映射**）上运行，结果：

- 低位对照 **4/4 OK**：DNS/53 ×2、NTP/123 ×2；
- 高位探测 **0/3 OK**：STUN/3478 ×2、STUN/19302 ×1 全部 `TimeoutError`；
- `RESULT: BLOCKED`，退出码 **2**，与本机完全一致。

**结论（按 STUN 口径）**：v2.4.9 留下的「出口例外是否按主机」开放问题被否证 —— 入站 DNAT
不产生 STUN 出站例外，两台主机对 STUN 高位端口同样不通。
> v2.4.11 更正：此处「高位 UDP / 白名单」结论只对 STUN 成立，不能外推到 KAD 报文；
> 「本网无主机可跑 P4」已降级为「本机已确证不可跑；LAN eMule 主机待核实」，见 2.4.11。

### 变更

- [PLAN.md §8.4](tests/spikes/s2/PLAN.md) 由「待执行」改为「已执行 + 结论」，§7 / §8 标题与
  §8.2 条件推论同步收敛；新增输出原文与判定表。
- `spikes.json` 新增 `observations.p4_environment_diagnosis.cross_host_result`，并更新
  `status_note` / `conclusion` / `open_question` / `cross_host_test` / `required_next_step`。
- [RISK R-02](docs/05-quality/RISK.md) 补记「逐主机判定已于本网两台主机实测、均为阻断」。
- 诊断脚本文档字符串与提示语改为「已闭环」措辞，保留可搬运复用的跨主机用法。
- S2 / kad-mini / tests-spikes README、ROADMAP、OPEN-QUESTIONS 去掉「待验证」表述；
  `docs/index.html` 重建。

**唯一有效路径**：换到高位 UDP 未被过滤的网络 → 诊断退出码 0 → 刷新 `nodes.dat` →
按 G0 参数（`sample_size=3` / `rate_limit_pps=0.2`）重跑 P4，再由实验执行人手工回填 `S2.status`。

**状态不变**：`S2.status = not_run`、`PROTOCOL_VERSION = 1`、T0 golden files 未动、
三桶计数整体作废、未发布任何测试 Key、公共 KAD 未获得任何协议证据。

## [2.4.9] - 2026-11-10 — 环境归因复核：按端口白名单 + 跨主机待验证

项目所有者指出 LAN 内那台 eMule 与本机同一网络、没有额外设置，唯一差异是 eMule 开了
入站映射，据此质疑 v2.4.8「网络统一过滤公网高位 UDP」的归因。补做更宽的对照实验后
**修正归因并升级诊断脚本**；协议设计基线、`S2.status` 与三桶作废结论均不变。

### 复核观测

| 探测 | 结果 | 排除项 |
| :--- | :--- | :--- |
| UDP/53 DNS、UDP/123 NTP（Cloudflare / pool.ntp.org） | OK，42～190 ms | 「UDP 全封」「整张网只走 HTTP 代理」不成立 |
| UDP/3478 / 19302 STUN（Cloudflare、ekiga、Google） | 全超时 | 非 Google 单一来源混淆；高位未知端口不通 |
| 同批 STUN 固定源端口（含 LAN eMule 端口与 4672） | 仍全超时 | 按源端口放行 |
| Windows 防火墙出站阻断规则 | 0 条，默认允许 | 本机防火墙 |
| LAN eMule 高位 UDP，同一帧 `e401` | OK 523 B / 9 ms | 本机 socket 栈与 LAN 路径 |

**修正后的归因**：公网出口按目标端口/应用白名单放行低位知名端口、丢弃高位未知端口，
不是 UDP 被整体过滤。撤回 §7「整张网只走 HTTP 代理出网」的绝对措辞。

### 三点更正

1. **LAN 探测不经过公网出口**，只证明报文格式与 LAN 路径正确，**不能**作为公网 UDP 可达性证据 —— 这是原归因的逻辑漏洞。
2. **入站映射不是请求/响应的必要条件**：aMule `Process2BootstrapRequest` 把响应发回请求的 `ip, port`，状态化 NAT 自动放行；`ProcessPacket` 唯一源端口检查是拒绝来自 53 的未加密包。映射决定的是「能否被外部主动联系」，且很可能是同一条按 IP 出口策略的**表征**而非机制。
3. **`nodes.dat`（2026-11-08）新鲜度是独立次要风险**：3 个随机联系人在健康网络上也可能全部离线，原先判为「非主因」证据不足。

### 新增 / 变更

- [udp_egress_check.py](tests/spikes/s2/udp_egress_check.py) 升级为可搬运的单文件诊断：
  新增 NTP/123 低位对照、非 Google STUN 目标、`--source-port`、`--lan-ip/--lan-port`
  （RFC1918/loopback 硬校验、绝不向公网发 KAD 包）、`--json`；退出码 `0`/`2`/`1` 语义不变。
  本机复验：低位 **4/4 OK**、高位 **0/3 OK**、LAN 对照 OK、退出码 **2**。
- [PLAN.md §8](tests/spikes/s2/PLAN.md) 记录复核与**决定性跨主机实验**：把该脚本拷到 LAN
  eMule 机器运行；退出码 0 → 出口例外按主机，可从该机重跑 P4 或为本机申请同策略；
  退出码 2 → 整网无主机可跑，须换网并刷新 `nodes.dat`。
- `spikes.json` 新增 `observations.p4_environment_diagnosis.refinement_2026_11_10` 并更新
  `conclusion` / `precondition_check`；[RISK R-02](docs/05-quality/RISK.md) 补充「前置判定逐主机、
  LAN 探测不构成公网证据」；S2 / kad-mini / tests-spikes README、ROADMAP、OPEN-QUESTIONS 同步；
  `docs/index.html` 重建。

**状态不变**：`S2.status = not_run`、`PROTOCOL_VERSION = 1`、T0 golden files 未动、
三桶计数整体作废、未发布任何测试 Key、公共 KAD 未获得任何协议证据。

## [2.4.8] - 2026-11-10 — P4 首次执行：环境阻断，结论不可用

项目所有者显式下达「执行 P4」。`live --check` 复验门禁 `passed=true` / 0 blocker 后，
按 G0 确认参数执行 `kad-mini live`（不带 `--check`）。**只登记观测与前置检查，不改协议设计基线**：
`PROTOCOL_VERSION` 仍为 1、T0 golden files 不变、`S2.status` 仍为 `not_run`。

### 原始观测

- 2026-11-10T00:26:52Z → 00:27:05Z（13 s）；`gate.passed=true`。
- 发出 **3** 条报文，均为 2 字节 `0xE4 0x01` `KADEMLIA2_BOOTSTRAP_REQ`，间隔严格 5.000 s
  （0.2 包/秒）；**收到 0 条响应**（WSAETIMEDOUT 10060，3 s 读窗口内无数据报）。
- HELLO / 迭代 / PUBLISH / SEARCH 均为 **0**；`known_contacts=0`、`tolerance_reached=0/3`，
  3 个样本全部按 `S2.criteria #2` 记 `skipped`，**未发布任何测试 Key**。
- 报告 `tests/spikes/s2/reports/kad-live-20261110002705.json`（gitignored，逐次完整原始报文）；
  `evidence_valid_for_pass=false`。

### 归因（对照实验）

| 对照 | 结果 |
| :--- | :--- |
| DNS/UDP-53 → 8.8.8.8 / 1.1.1.1 | 成功，61 B，≈53～58 ms |
| STUN/UDP-19302、UDP-3478（与 KAD 无关） | 全部 3 s 超时 |
| 直连 TCP → 1.1.1.1:443（绕过本地 HTTP 代理） | 6 s 超时 |
| LAN 真实 eMule，同一份 `e401` | `0xE4 0x09` 明文 523 B / **20 contacts** |
| aMule 源码 | `ProcessPacket` 直接分发 `KADEMLIA2_BOOTSTRAP_REQ`，请求无 payload 要求 |

结论：`inconclusive_environment_blocked` —— **执行环境过滤公网高位 UDP（出站或回程）**，
不是报文格式、不是节点拒绝、不是命名空间被拒。按 [RISK R-02](docs/05-quality/RISK.md) 口径，
这三次超时属环境前置条件失败，**三桶计数整体作废**，不得判定 S2。
`nodes.dat`（2026-11-08）联系人新鲜度为次要可能因素，STUN 对照已排除其为主因。

### 新增与文档

- 新增可选手动诊断 [udp_egress_check.py](tests/spikes/s2/udp_egress_check.py)：只做 DNS/53 与
  STUN 往返对照，不发任何 KAD 报文、不内置 bootstrap；退出码 `0`=可跑 P4、`2`=被过滤。
  本环境实测 **2**。重跑 P4 前须先通过。
- [PLAN.md §7](tests/spikes/s2/PLAN.md) 记录完整执行与归因；`spikes.json` 新增
  `observations.p4_execution` / `p4_environment_diagnosis` 并更新 `real_network`；
  [RISK R-02](docs/05-quality/RISK.md) 补充「前置条件失败不入三桶」。
- 同步 [S2 README](tests/spikes/s2/README.md)、[kad-mini README](tests/spikes/s2/kad-mini/README.md)、
  [tests/spikes/README.md](tests/spikes/README.md)、[ROADMAP](docs/06-plan/ROADMAP.md)、
  [TEST](docs/05-quality/TEST.md)、[OPEN-QUESTIONS](docs/06-plan/OPEN-QUESTIONS.md)、根 README，
  `docs/index.html` 重建。

## [2.4.7] - 2026-11-09 — P4 前置一致性修正（补齐实现与判据的错配）

推送 [v2.4.6](tests/spikes/s2/PLAN.md) 后复核 `kad-mini live` 实现，发现四处会让 P4 产出假结论的
错配。**只改 S2 spike 实现与其文档，不动协议设计基线**：`PROTOCOL_VERSION` 仍为 1、
T0 golden files 不变、`S2.status` 仍为 `not_run`、**公共 KAD 未被接触**。

### 修复

| 错配 | 修复 |
| :--- | :--- |
| `live` 直接向 bootstrap 节点发布，违反 `S2.criteria` 第 2 条 | 发布前用 `KADEMLIA2_REQ/RES (0x21/0x29)` 最多 3 跳迭代收敛到 `SEARCHTOLERANCE = 2^24` 邻域；未收敛则记录 `skipped` 并跳过该样本，不制造必然被静默丢弃的无效发布 |
| `live` 仍发 `HELLO_REQ`，与 `confirmed.protocol_scope`「跳过 HELLO」矛盾 | 移除 HELLO 交换；逐次记录 `hello=skipped_per_p4_protocol_scope` |
| `0xE5` 压缩响应被当作「协议层失败」，污染 RISK R-02 三桶计数 | 新增零依赖 `tests/spikes/s2/kad-mini/src/inflate.rs`（zlib/DEFLATE，覆盖 stored / fixed / dynamic 块型、校验 Adler-32）；`decode_wire()` 统一处理 `0xE4` / `0xE5` |
| `live_gate` 只数 `"ip"` 字段不校验地址；`live` 用 127.0.0.1 绑定 socket | 门禁实际解析清单并拒绝非公网 IPv4 / `lan-interop-probe-only`；`live` 改用 `bind_any()` |

### 复验结果

- `cargo test` **7/7**；`kad-mini selftest` **37/37**（新增 8 项：三种 DEFLATE 块型、Adler-32 拒绝、
  非 KAD 拒绝、明文回归、公网/私有 IPv4 门禁）；`loopback` **5/5 发布 + 5/5 命中 + 84 条原始记录 + 0 协议错误**。
- 真实 LAN eMule `BOOTSTRAP_RES` 解出 521 B payload / **20 个联系人**；此前捕获的
  真实 519 B `0xE5` 帧经 `inflate.rs` 解出同样结果，与 Python `zlib.decompress` 一致。
- `live --check`：公网清单 `gate.passed=true`；LAN 清单命中两条 blocker；无标记的 RFC1918 清单仍被
  公网 IPv4 校验拦截。三条路径均 `public_kad_contacted=false`，**未发送任何报文**。
- 修复了 v2.4.6 正常 `git push` 因本地代理 CONNECT 502 失败的问题（重试即成功）；Git Data API
  兜底曾因 tree SHA 不一致在更新远端 ref **之前**中止，未产生任何远端副作用。
  远端 `main` 的 tree 与本地 `e25b0f0` 完全一致（`eaad7a5b…`），三个 workflow 已复验。

### 文档

[PLAN.md §0/§1/§2/§3/§5/§6](tests/spikes/s2/PLAN.md)、[kad-mini/README.md](tests/spikes/s2/kad-mini/README.md)、
[S2 README](tests/spikes/s2/README.md)、[tests/spikes/README.md](tests/spikes/README.md)、
[ROADMAP](docs/06-plan/ROADMAP.md)、[TEST](docs/05-quality/TEST.md)、
[OPEN-QUESTIONS](docs/06-plan/OPEN-QUESTIONS.md)、`spikes.json` 同步登记；`docs/index.html` 已重建。

## [2.4.6] - 2026-11-09 — 判据口径回写 + G0 参数确认 + LAN 互操作探测

### 变更：把判据口径改对（A）

按 aMule 源码核实结果回写设计基线，**只改 KAD 集成映射与观测语义，不动私有控制面 v1 字段**，
`PROTOCOL_VERSION` 保持 1、T0 golden files 不变（PROTO 文档版本 2.0 → 2.1）。

- `spikes.json`：`S2.criteria` 从「`FIND_VALUE` 命中带前缀 Key」改为
  `PUBLISH_KEY_REQ (0x43)` / `SEARCH_KEY_REQ (0x33)` / `SEARCH_RES (0x3B)`；
  新增「发布前须 `KADEMLIA2_REQ/RES` 多跳迭代收敛到 `SEARCHTOLERANCE` 邻域」、
  「协议层无响应与索引层拒绝必须分开计数」、「前缀线上不可见，隔离口径为 Key 空间不互相覆盖」、
  「实网参数须与 G0 确认值一致」。同步修正 `S2.assumption` 并登记 `S1.adr_002_impact`。
- [PROTO §1/§2.0/§2/§2.1/§4](docs/04-protocol/PROTO.md)：新增 §2.0「KAD 集成映射基线」，
  定义 `KAD_ID(x) = SHA1(x)[0..16]`（截断规则仍标「暂定待 ADR」）、Tag 值模型、
  `0xE4` / `0xE5` / RC4 帧形态、UDP key 的真实作用；§4 增加「映射修订例外」条款。
- [ADR-001](docs/02-architecture/adr/ADR-001-kad-network.md)：新增「协议核实结论」5 条；
  20/s 发布限速标注须按 tolerance zone 复核；复核时点明确为 **P4 实网小样本之后**（1.0 → 1.1）。
- [ADR-002](docs/02-architecture/adr/ADR-002-bt-engine.md)：登记 rqbit 9.1.0 无握手回调的
  S1 观测，选型门禁改为「评估自实现拦截层 / 代理层」，不再以「引擎是否暴露回调」判 S1（1.0 → 1.1）。
- [RISK R-02](docs/05-quality/RISK.md)：预警线从笼统的「发布失败率 > 20%」改为
  **「索引层拒绝率 > 20%（须先排除协议层不可解析 / 静默丢弃 / 未收敛）」**，并给出三桶计数表。
- `PLAN.md` §0/§1/§4 由「需回写」改为「已回写」，P3 标记完成。

### 变更：G0 参数确认与门禁收紧（B）

- 项目所有者确认采用建议区间的最保守值：
  **`sample_size = 3`**、**`rate_limit_pps = 0.2`**（每 5 秒 1 个包），
  写入 `S2.public_kad_approval.confirmed` 并**清除 `pending`**。
- `kad-mini live_gate` 增加一致性校验：CLI 的 `--sample-size` / `--rate-limit`
  必须与 `confirmed` 逐项相等，否则拒绝；防止批准记录沦为装饰。
- 新增 `kad-mini lan-probe`：只发 `BOOTSTRAP_REQ` / `HELLO_REQ`、**不发任何 PUBLISH**，
  硬编码拒绝非 RFC1918 地址，报告固定 `public_kad_contacted=false` / `publish_sent=false`。
  `rec()` 现在能正确标注 `0xE5` 帧的 opcode；`exchange()` 对 `0xE5` / 非 KAD 帧给出明确错误。
- 新增 [`nodes_dat_to_bootstrap.py`](tests/spikes/s2/nodes_dat_to_bootstrap.py)：
  只读解析 aMule/eMule `nodes.dat`（fileVersion 1–3），输出 gitignored 的本地 bootstrap JSON；
  刻意**不导出 `CKadUDPKey`**（绑定原机器公网 IP，跨主机不可用）。

### 观测结论：节点文件与 LAN 真实客户端

| 项 | 结论 |
| :--- | :--- |
| `nodes.dat` | `fileVersion=2`，**200 个 KAD contacts 全部 verified**，版本 8/9/10 为主；可作公共 bootstrap 来源 |
| `server.met` | magic `0xE0`、17 个 **eD2K 服务器**条目，不是 KAD 路由表，**对 bootstrap 无用** |
| LAN 真实 eMule（RFC1918 私有地址，具体 IP/端口只存于本地 bootstrap 文件） | KAD 在线。`BOOTSTRAP_REQ` 一次回 `0xE4 0x09` 明文 523 B（RTT ≈ 13 ms），另一次回 `0xE5 0x09` zlib 压缩 519 B；`HELLO_REQ` v7/v8 均超时 |

**明文 vs sender/receiver UDP key 的真实区别**：`ClientUDPSocket.cpp::DecryptReceivedClient`
对首字节 `0xE4` 直接放行，`PacketTracking.cpp::InTrackListIsAllowedPacket` 的 key 形参
（`bool /*bValidSenderkey*/`）实际未启用 —— **明文不会被「因未加密」而静默丢弃**。
key 是 `(m_key, m_ip)`、绑定源 IP 的反欺骗 / 联系人验证标记，补它并不能解决压缩帧与 RC4；
而 `BOOTSTRAP_RES` / `KADEMLIA2_RES` / `PUBLISH_RES` / `SEARCH_RES` 发送端都传
`cryptTargetID = NULL`（明文），只有 `HELLO_RES` 对 kadVersion ≥ 6 才 RC4。
→ **决策：保持明文、P4 跳过 HELLO、不实现 key / RC4**；若 P4 中 `0xE5` 桶占比过高再追加 zlib 解码。

### 不变 / 红线

- **P4 实网小样本未执行**：`live --check` 现在报门禁通过但永不发包；须显式去掉 `--check`
  并给出操作者声明才会发布。`S2.status` 仍 `not_run`，公共 KAD 未被接触。
- LAN 节点只作互操作预演，不是公共 KAD 证据；若它连着公共 KAD，向其发布测试 Key 可能外泄，故禁止。
- 仓库不含任何节点 IP：两个 `*.local.json` 均在 `reports/` 下被 `.gitignore` 排除。
- 项目根仍无 `Cargo.toml`、`src/` 未动；S1–S7 仍 `not_run`，S8 仍为所有者豁免（R-11 保留）。

## [2.4.5] - 2026-11-09 — 修复 T0 skeleton CI 断言与 S8 豁免的冲突

### 变更

- `.github/workflows/t0.yml` 的「M0 preflight must report blockers honestly」步骤：原断言
  `all(s['status'] == 'not_run')` 与 2026-11-09 登记的 S8 项目所有者豁免（`pass`）冲突，
  导致 `eccec2e` / `c7b065f` 两个提交的 T0 skeleton 工作流失败
  （`AssertionError: Spike 结论不得由脚本自动置为 pass`）。Docs / Pages 同期为 success。
- 改为**更严格的分级校验**：S1–S7 必须保持 `not_run`（脚本不得自动置 `pass`）；
  唯一允许的非 `not_run` 是 S8，且须能从 `spikes.json` 追溯到
  `resolution=waived_by_owner`、`legal_review.status=waived_by_owner`、
  `written_opinion_received=false`、`residual_risk` 非空 —— 既不放宽其余 Spike 的结论入口，
  也禁止把豁免包装成「已取得法务书面意见」。
- 其余门禁不变：`g0_ready=false`、`t0_real_ready=false`、preflight 退出码 2、
  stub 报告 `evidence_valid=false`、`--mode real` 仍 `BLOCKED`。

### 验证

- 本地以同一断言脚本跑 `m0_preflight.py --json`：通过，
  blockers = `downloader.any` / `binaries.m1` / `t0.wired`。
- 本修复提交后需确认 T0 skeleton 工作流转绿。

### 不变

- S8 仍为所有者豁免（无书面法务意见，R-11 保留）；S2 仍 `not_run`；未接触公共 KAD；
  项目根仍无 `Cargo.toml`、`src/` 未动。

## [2.4.4] - 2026-11-09 — S2 实网排期与最小 eMule KAD 客户端（真实 wire 回环）

### 新增

- `tests/spikes/s2/PLAN.md`：S2 实网**实施排期与门禁**。P0 协议核实 / P1 wire codec /
  P2 本地真实报文闭环**已完成**，P3 实网门禁参数 / P4 实网小样本**未执行**。协议依据固定为
  aMule `master` @ `6a1105634d81242a581a0f36a7e00379b12a83f3`，不采用二手 opcode 表。
- `tests/spikes/s2/kad-mini/`：**零第三方依赖**的独立 Rust Cargo 工程（项目根仍无 `Cargo.toml`、
  `src/` 未动）。实现真实 `0xE4` Kademlia2 帧编解码、128-bit ID / XOR 距离 / `SEARCHTOLERANCE`、
  Tag 与 Contact 编解码、命名空间 Key 派生、树内 SHA-1（含 FIPS 已知答案向量）、三端点本地索引节点、
  令牌桶限速、逐包十六进制原始记录与 `live` 模式门禁。
- `tests/spikes/s2/README.md` 改述为三层结构：L1 逻辑预演 / L2 真实 wire 闭环 / L3 实网小样本。
- `spikes.json` 为 S2 回填 `observations.real_wire`、`protocol_findings` 与报告路径；
  **`status` 仍为 `not_run`**，`public_kad_approval.pending` 仍保留。

### 验证（全部 `127.0.0.1` 回环）

- `cargo test` 7/7；`kad-mini selftest` 29/29（协议常量、帧轮编、SHA-1 已知答案向量）。
- `kad-mini loopback`：发布 5/5 被 ACK、查询命中 5/5 且描述符 Tag 匹配、RTT p50 0.2318 ms、
  命名空间隔离通过、**84 条收发报文完整十六进制逐次记录**、0 协议错误。报告固定写
  `public_kad_contacted=false` 与 `evidence_valid_for_pass=false`。
- `kad-mini live --check`：退出码 2 拒绝，列出 5 项阻塞（`pending` 未清除、缺 `--bootstrap`、
  缺 `--sample-size`、缺 `--rate-limit`、缺操作者声明），**未发送任何非回环报文**，
  未向公共 KAD 发布任何测试 Key。

### 观测结论（发现，本次不擅改设计基线）

- eMule KAD **无 `FIND_VALUE`**：判据应映射为 `SEARCH_KEY_REQ (0x33) / SEARCH_RES (0x3B)`，
  发布为 `PUBLISH_KEY_REQ (0x43)`。核查发现 DeepWiki 把 `PUBLISH_KEY_REQ` 误记为 `0x35`
  （源码中 `0x35` 是 `SEARCH_NOTES_REQ`），本工程常量全部取自 aMule 源码。
- KAD ID 空间为 **128-bit**，[PROTO §1](docs/04-protocol/PROTO.md) 的 `Target_Key = SHA1(...)`
  是 **160-bit**；探针暂取 SHA-1 前 16 字节，截断规则待 ADR 决策。L1 预演用的是
  SHA-256 前 16 字节，两层 Key ID **不可直接比较**，均为临时映射。
- `ColdRevive_V1:` 前缀只作用于本地哈希输入，线上仅传输 128-bit ID，公共节点看不到字符串前缀；
  S2-C3「前缀是否被接纳」的观测口径需重定义。
- KAD 不是通用 KV，Value 只能编码为 `(fileID, Tag 列表)`；`Node_Descriptor` 已映射为命名空间 Tag。
- `Process2PublishKeyRequest` 有 `SEARCHTOLERANCE = 2^24` 距离校验：实测 5 个 Key 的
  `xor_top32` 全部远超阈值，回环靠 LAN 豁免通过；WAN 下会被**静默丢弃且无响应**，
  实网必须先多跳迭代查找收敛到 Key 邻域。
- 明文 UDP 未实现 aMule ≥ v6 的 sender/receiver UDP key 与 RC4 混淆，真实节点可能静默丢弃 ——
  这是 S2-C2 待观测项，也是 P3 是否需追加 P1 工作的决策点。

### 不变

- S1–S7 仍 `not_run`，S8 仍为所有者豁免 `pass`（无书面法务意见，R-11 保留）。
- 未接触公共 KAD；仓库不含任何默认 bootstrap 节点；`CliAdapter.wired` 仍 `False`，
  `--mode real` 仍 `BLOCKED`，T0 不构成 G1 证据；不创建根 `Cargo.toml`、不动 `src/`。
- 上述错配需回写 ADR-001、PROTO §1/§2、`S2.criteria` 与 RISK R-02，本次只登记发现。
- `2.4.3` 及之前的改动已作为 `eccec2e` 推送至 GitHub。

## [2.4.3] - 2026-11-09 — M0 首批实验产出：S1 最小 Demo 与 S2 回环预演

### 新增

- `tests/spikes/s1/`：**独立 Cargo 工程**（项目根仍无 `Cargo.toml`、`src/` 未动），依赖 librqbit 9.1.0。
  本地回环串起 ColdRevive 的核心链路：拦截标准 68 字节 BT Handshake → 按 InfoHash 动态加载伴随种子
  → `AddTorrent::TorrentFileBytes` + `paused=true` 按需实例化 → `unpause / pause / unpause`
  → 16 片逐片 SHA-1 校验。机器可读报告落在 `reports/s1-probe-*.json`。
- `tests/spikes/s2/s2_loopback.py`：私有 KAD 子网双节点预演，A 发布 5 个 `ColdRevive_V1:` 前缀 Key，
  B `FIND_VALUE` 命中 5/5，RTT p50 0.33 ms，命名空间 ID 隔离通过。报告固定标注
  `public_kad_contacted=false` 与 `evidence_valid_for_pass=false`。
- `spikes.json` 为 S1 / S2 登记 `observations` 与报告路径，**`status` 仍为 `not_run`**。

### 观测结论

- **S1-C1 / C2 verified**：握手拦截与按需实例化成立。但源码探测发现 rqbit 9.1.0 的
  `SessionOptions` 无握手回调字段，`session.rs::task_listener` 内部 `read_handshake()` 后直接按
  InfoHash 查已管理 torrent，不暴露拦截点 → 握手拦截器须自实现或加代理层，**影响 ADR-002 选型论证**。
- **S1-C3 / C4 partial**：`is_paused` 正常切换、`progress_bytes` 保持 262144/262144，逐片 SHA-1
  16/16 匹配；但样本文件预先存在（位图始终完整），且未建立双 peer 真实分片传输。
- **S2 仅为预演**：按 `S2.criteria` 第 4 条不得判 `pass`。实网结论还缺真实 eMule KAD 协议实现、
  G0 评审确认的最小样本量与限速阈值、以及实网逐次原始记录。

### 不变

- 脚本禁止自动置 `pass`，S1 / S2 状态仍为 `not_run`；`CliAdapter.wired` 仍 `False`，
  `--mode real` 仍 `BLOCKED`，T0 不构成 G1 门禁证据。
- 未向公共 KAD 发布任何测试 Key；两个 Demo 全程 `127.0.0.1` 回环。

## [2.4.2] - 2026-11-09 — M0 状态对齐：工具链就绪、S8 豁免、S2 批准与 Windows 兼容

### 变更

- 本机安装 Rust stable 1.99.1（`rustc` / `cargo` / `rustup`），MSVC 链接实测可用；`m0_preflight.py` 的 `toolchain.*` 检查转为 `[OK]`。
- 修正因工具链落地而过期的「本机缺 `cargo`」表述：`ROADMAP §6`、`TEST §2.1`、`tests/e2e/t0/README.md §5`、`tests/spikes/README.md`、`spikes.json` 的 `status_note`；同步根 `README.md`、`tests/README.md`、`OPEN-QUESTIONS §3` 的实验状态。
- **S8 法务评审**：项目所有者明确指示略过、视为已评审；在 `spikes.json` 登记 `status: pass` 与 `legal_review.status: waived_by_owner`，并如实标注 `written_opinion_received: false`。**未取得法务书面意见，R-11 风险保留，M4/G4 前须复核**；`m0_preflight.py` 的对应检查文案改为「书面意见或所有者豁免」。
- **S2 实网实验**：项目所有者批准，人工回填 `spikes.json` 的 `public_kad_approval`（最小样本、限速、禁止批量发布测试 Key、逐次记录；样本量与限速阈值待 G0 评审确认）。`status` 仍为 `not_run`：本地回环预演不得判 `pass`，实网结论须基于真实小样本数据。
- 修复 `t0_cli_selftest.py` 在 Windows 的 `WinError 193`：自检生成的 `fake-cli.py` 依赖 shebang 直接执行，Windows 不支持；改为在自检内用 `sys.executable` 显式启动，**不改产品 CLI v1 契约**。
- 本机补装 Python `markdown` 依赖以支持 `scripts/build_docs.py`，重跑构建同步 `docs/index.html` 内嵌数据。

### 验证

- `m0_preflight.py`：阻塞 3 项 / 提醒 1 项，`G0 ready=False`；`cargo`、S2 批准、S8 登记均 `[OK]`。
- `t0_cli_selftest.py`：6/6 通过（含 Windows 修复）；`t0_accept.py --self-test`：21/21 PASS 且 `evidence_valid=false`；`build_docs.py --check` 通过。

### 不变

- **技术路线零改动**：不创建根 `Cargo.toml`、不碰 `src/`；S1–S7 仍 `not_run`；`CliAdapter.wired` 仍 `False`；`--mode real` 仍 `BLOCKED`；T0 不构成 G1 门禁证据。

## [2.4.1] - 2026-11-09 — 托管到 GitHub 并发布在线文档站

### 变更

- 建立 Git 仓库（默认分支 `main`），推送至 **https://github.com/asdfgh129/ColdRevive**（公开）。
- 新增 `.github/workflows/pages.yml`：安装 `markdown` → 运行 `docs` 构建器 → 校验 `index.html` 与源码同步 → 上传 `docs/` 为 Pages 产物 → 部署。仓库 Pages 设为 `build_type=workflow`，在线入口 **https://asdfgh129.github.io/ColdRevive/**。
- `.gitignore` 增加 `.workbuddy/`：本地智能体工作记忆不进入公开仓库。
- 根 `README.md` 增加代码仓库与在线文档入口；重跑 `scripts/build_docs.py` 同步 `docs/index.html` 内嵌内容。

### 说明

- 本版本只做托管与发布，**不改变任何技术设计或验收状态**：S1–S8 仍为 `not_run`、`CliAdapter.wired` 仍为 `False`、T0 stub 自检仍不构成 G1 门禁证据。
- 提交身份使用 GitHub noreply 邮箱，避免在公开提交历史中暴露个人邮箱。

## [2.4.0] - 2026-11-09 — 推进 T0 接线：CLI 契约、观测层与 M0 实验包

### 变更

- 新增 `tests/spikes/`（M0 实验包）：`spikes.json` 登记 S1–S8 判据、优先级、失败替代与状态（当前全部 `not_run`）；`README.md` 给出 S1/S2 实验步骤、合规红线与结论回填规则；`m0_preflight.py` 可复跑地检查实验前置条件，输出机器可读报告。
- **定稿 CLI v1 契约**（`proposed-v1`）：`t0_config.json` 的 `launch` 由占位改为 `coldrevive.t0.cli-contract/v1`，定义 `inspect / find / wake / scenario / metrics / download / golden / env` 八个命令与健康检查；契约说明见 [T0 骨架 §5.1](tests/e2e/t0/README.md)。
- 新增 `tests/e2e/t0/t0_cli.py`：20 条用例的真实取数映射 `OBSERVE_PLAN`，CLI 调用失败 / 非法 JSON / 字段缺失一律判「观测不可用」，**绝不合成观测值**；`torrent_infohash()` 按 BT 规范从伴随 `.torrent` 的 info 字典原始字节计算真实 InfoHash，替换骨架期的 `infohash_stub`。
- `CliAdapter` 接入观测层并显式绑定 20 个 `observe_t0_XX` 入口；`preflight()` 改为输出真实阻塞项。**`wired` 仍为 `False`**，`--mode real` 继续 `BLOCKED`。
- 新增 `tests/e2e/t0/t0_cli_selftest.py`（6 项观测层自检）与 `.github/workflows/t0.yml` 对应步骤；CI 保留「Real mode stays BLOCKED」断言。
- 文档同步：[TEST §2.1](docs/05-quality/TEST.md)（v2.2）、[ROADMAP §6](docs/06-plan/ROADMAP.md)（v2.2）、`docs/README.md`（v2.4）、`tests/README.md`、`scripts/README.md`、根 `README.md`；`build_docs.py` 的 `EMBED_EXTRA` 纳入 `tests/spikes/README.md`（7 → 8 个工程文件，内嵌项 40 → 41）。

### 验证

- `m0_preflight.py` 实测：阻塞 6 项 → 落地观测层后 5 项；`rustc` 与 T0 端口可用，`cargo`、第三方 BT 客户端、M1 二进制、S2 实网批准、`wired` 未就绪，S8 法务评审为外部提醒。
- `t0_cli_selftest.py`：6/6 通过（真实 InfoHash 计算、CLI 取数与字段映射、缺字段不合成、缺二进制抛异常、20 条映射齐全、`wired` 仍为 `False`）。
- `t0_accept.py --self-test` 仍 21/21 PASS 且 `evidence_valid: false`；`--mode real` 仍 1 PASS / 20 BLOCKED、退出码 2，`next_actions` 现已如实反映「观测层已实现、缺二进制与真实环境」。
- `build_docs.py --check` 通过；两次构建字节一致；索引页全量链接解析失效 0。

### 不变

- **技术路线零改动**：不创建 `Cargo.toml`、不碰 `src/`；S1/S2/S8 实验**未执行**，不产出任何 Spike 结论，不接触公共 KAD。
- T0 仍不构成 G1 门禁证据；`Cargo.toml` 与 crate 骨架继续等 G0 通过后创建。

## [2.3.0] - 2026-11-09 — 搭建 T0 双节点验收脚本骨架

### 变更

- 新增 `tests/e2e/t0/`：T0 单机回环双节点验收骨架，含 `README.md`（判据/接线清单）、`t0_config.json`（环境、阈值、启动契约）、`t0_cases.json`（21 条用例元数据）、`t0_harness.py`（端口预检、确定性样本、进程启停与清理）、`t0_adapters.py`（观测契约与 Stub/Cli 适配器）、`t0_accept.py`（断言引擎、JSON 报告、退出码）。
- 新增 `tests/fixtures/t0/`：`WAKEUP_REQ` / `WAKEUP_ACK` v1 契约 golden、确定性样本生成约定；运行产物由 `.gitignore` 排除。
- 用例 `T0-00`～`T0-20` 覆盖 F1→F2→F3→F4 主链路、故障注入（8 类错误码）、协议 golden、最小暴露与端口隔离；判据出处逐条登记在 `t0_cases.json` 的 `source` 字段，阈值只在 `t0_config.json` 定义一次。
- **防呆设计**：stub 自检报告的 `evidence_valid` 恒为 `false`；`CliAdapter.wired = False` 时 `--mode real` 把所有真实用例标为 `BLOCKED` 并以退出码 2 结束，杜绝「没跑却显示绿」。`T0-00` 通过注入坏观测验证断言引擎能正确判 FAIL。
- 文档同步：[TEST §2.1](docs/05-quality/TEST.md) 新增 T0 骨架章节、[ROADMAP §6](docs/06-plan/ROADMAP.md) 修正状态滞后（v2.1 拆分 / M0 未开始 → v2.3 + T0 骨架已就位）、`tests/README.md` 修正「TEST §9.2」错误引用为 §2、根 `README.md` 增加自检命令、`docs/README.md` 增加「docs/ 外被引用文件须登记 `EMBED_EXTRA`」规则。
- `scripts/build_docs.py` 的 `EMBED_EXTRA` 纳入 `tests/e2e/t0/README.md`（7 个工程文件），保证自包含索引页里指向骨架的链接可就地打开；索引页版本标识同步 v2.3。
- 新增 `.github/workflows/t0.yml`：CI 运行 T0 框架自检与文档校验。

### 验证

- `py_compile` 三个脚本通过；`t0_harness.py --self-check` 通过（样本 SHA-256 复算一致、3 个 Keyword_Key ≤ 8、两份 golden 齐全）。
- `--self-test`：21/21 PASS，退出码 0，`evidence_valid: false`；把 P95 阈值临时改为 100 ms 后 `T0-10` 正确 FAIL 且退出码 1，证明阈值取自配置、断言生效。
- `--mode real`（未接线）：1 PASS / 20 BLOCKED，退出码 2，`next_actions` 列出 20 个待实现观测与 3 个未设环境变量。

### 不变

- **技术路线零改动**：不创建 `Cargo.toml`、不碰 `src/`、不提前固化 BT 引擎或 KAD 方案；M0 门禁与 G0～G4 判据保持原样。
- T0 骨架**不构成 G1 门禁证据**；S1/S2 Spike 与法务评审仍是 M0 的起跑线。

## [2.2.0] - 2026-11-09 — 修复索引页阅读链接失效

### 问题

在预览面板 / `srcdoc` 沙箱中只承载 `docs/index.html` 单个文件，卡片上的「阅读」链接指向同目录的 `01-product/PRD.html` 等文件，无法解析，打开后显示 *no data available*。

### 变更

- `docs/index.html` 改为**自包含单文件门户**：新增页内阅读器（文档 / MD 源码双视图、跨文档跳转、Esc 关闭、独立页面可选出口）。
- 全部站内链接在运行时改为 `data-href` 并移除 `href`，从根上避免任何预览框架拦截锚点后的外部导航；外链与页内 `#` 锚点保持原样。
- `scripts/build_docs.py` 新增 `DOCS_PAYLOAD` 注入：把 33 份文档正文 + Markdown 源码，以及 `docs/` 外被引用的 6 个工程文件（`README.md`、`CHANGELOG.md`、`src/tests/scripts/README.md`、`config/coldrevive.example.toml`）共 39 项写入 `index.html`；索引页只替换标记之间的数据块，人工维护部分不被覆盖。
- 抽出共用的 `render_body()`，保证独立 HTML 页面与内嵌阅读器渲染结果一致。
- `--check` 增加索引页 `DOCS_PAYLOAD` 标记校验；同步更新 `README.md`、`docs/README.md`、`scripts/README.md` 的构建说明。

### 验证

- 构建：`--check` 通过；内嵌 39 份文档，生成 33 个独立 HTML 页面。
- 链接：用页面实际的 `normKey()` 校验 79 个索引/卡片链接 + 465 个正文链接，**544 个全部命中内嵌目标**。
- 脚本：`node --check` 通过；`armLinks()` 运行时行为（内部链接去除 `href`、中文路径解析）已抽样验证。

### 不变

- 技术内容、设计决策、指标与里程碑零改动；仅改变文档的分发与阅读方式。

## [2.1.0] - 2026-11-09 — 文档工程化重构

### 变更

- 将单文件 `ColdRevive-项目开发文档-v2.0.md`（1193 行）按软件工程规范拆分为 20+ 份独立文档，分入 8 个类别目录。
- 建立完整项目层级目录：`src/` `tests/` `scripts/` `config/` `.github/workflows/` `docs/`。
- 新增 `docs/index.html` 离线文档索引门户（分类导航、搜索、角色阅读路径、状态徽标）。
- 新增 `scripts/build_docs.py`：Markdown 单一事实来源 → 批量生成 HTML 页面，支持 `--check` 校验。
- 新增 `docs/02-architecture/adr/`：10 份独立架构决策记录（ADR），一份一议题。
- 新增 `config/coldrevive.example.toml` 可执行配置样例，与 CONFIG.md 规范对应。
- 原始 `chat.md` 与 v2.0 合并文档移入 `docs/09-archive/`，标记为已废止的历史基线。

### 不变

- 技术内容、设计决策、指标与里程碑相对 v2.0 **零改动**，仅做结构化拆分与归位。
- 所有已冻结决策（DR-01～DR-08、ADR-003/004/006/007/008/009/010）保持有效。

## [2.0.0] - 2026-11-09 — 设计收敛

基于 `chat.md` 全部讨论轮次，取代其中的 v1.0 草案。主要修订：

1. 确立「常态共享目录（体验层）+ 按需唤醒（实现层）」双层模型。
2. 补充伴随种子生成，解决 BT 基于种子而非文件的硬壁垒。
3. 增加冷热分级、LRU 活跃池、影子索引、握手拦截与资源阈值治理。
4. 分层 DHT：KAD 发现 / 私有 RPC 激活 / Mainline DHT 兼容 / Tracker 可选。
5. 扩充威胁模型、Challenge-Response、限流与信誉设计。
6. 纳入 hMule 失败复盘，将冷启动网络效应列为头号风险。
7. 吸收 aria2 预研结论：守护进程内嵌 HTTP/WS、WebSocket 替代轮询、兼容 aria2 JSON-RPC。
8. 细化 WBS、M0–M5 里程碑、质量门禁、测试矩阵与风险登记册。

## [1.0.0] - 讨论记录内嵌草案

`chat.md` 第 584–739 行的《ColdRevive 客户端开发设计文档 v1.0》，首次提出 KAD 黄页 + 私有唤醒 RPC + 标准数据管道架构。现仅作历史参考。
