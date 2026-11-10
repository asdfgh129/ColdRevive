# 变更记录（Changelog）

本文件记录**文档体系与项目决策**的变更。格式参考 Keep a Changelog，版本与文档 front matter 中的 `version` 对应。

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
