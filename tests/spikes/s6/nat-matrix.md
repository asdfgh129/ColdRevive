# S6 spike：常见家用 NAT 下的可达率

> **状态（2026-11-17）：`not_run`（G0 条件冻结豁免其 G0 时点结论）。**
> **判据已于 2026-11-17 由项目所有者改为 [ADR-016](../../../docs/02-architecture/adr/ADR-016-s6-emule-like-reach.md) 的
> 「eMule 式可达语义」：不再穷举 4 类 NAT + CGNAT，类别覆盖降级为描述性清单、不再是 pass 前提。**
> 本目录下 §13 / §18 / §19 的「凑齐 4 类 + 1 CGNAT」表述是**当时的判据与历史论证，保留不改写**；
> 现行判据、改动边界与复审时点以 **§23** 为准。`pass` 须由实验执行人依真实证据手工回填，
> 脚本永不自动置 pass，`s6_status` 仍 `not_run`、`g2_claim=false`。
> 下方 §1 的 2026-11-16 口径（ADR-012 取消数值承诺）**继续有效**。

## 1. 判据与风险

- **判据 S6（2026-11-16 起，[ADR-012](../../../docs/02-architecture/adr/ADR-012-r04-best-effort.md)）**：
  NAT 类型矩阵实测，逐类查清「直连行不行、不行时中继是否送达」。**不承诺任何可达率数值**——
  要的是地图，不是达标线。判据原文见 §19。
- **关联 FR-17**：UPnP / NAT-PMP 端口映射、UDP 打洞、LowID 回退；常见家用路由器 NAT 矩阵通过。
- **RISK R-04**：NAT 穿透失败率高，唤醒后连不上（中/高）；**触发条件已改为
  「某类环境中直连与中继回退同时失败」**（原「公网可达率 < 70%」已取消，见 ADR-012 / §19）。

## 2. ColdRevive 穿透栈（设计能力）

| 技术 | 作用 | 覆盖的 NAT 类型 |
| :--- | :--- | :--- |
| UPnP IGD | 主动在网关映射端口 | 支持 UPnP 的家用路由器（绝大多数） |
| NAT-PMP / PCP | UPnP 替代（Apple/现代网关） | 同上，互补 |
| UDP 打洞（STUN 获公网端点 + PEX/DHT 对端引见） | 无 UPnP 时的锥型 NAT 直连 | Full / Restricted / Port-restricted cone |
| 中继 / LowID 回退 | 对称 NAT 无法直连时的兜底 | Symmetric NAT（R-04 缓解项） |

## 3. NAT 类型矩阵（分析型估计）

基于典型家用/运营商 NAT 分布（行业常见经验值，非本项目实测）：

| NAT 类型 | 典型占比 | 主用技术 | 直连可达率（估计） | 加中继后可达率（估计） |
| :--- | :--- | :--- | :--- | :--- |
| Full-cone | ~5% | UPnP / STUN | ~100% | ~100% |
| Restricted-cone | ~10% | UPnP / STUN + 打洞 | ~100% | ~100% |
| Port-restricted-cone | ~45% | UPnP / STUN + 打洞 | ~98% | ~100% |
| Symmetric | ~40% | UPnP（若有）+ 中继/LowID | ~30–50% | ~90%+ |

> **2026-11-16 注**：这里原本写着「直连 ≈80–90%、含中继 ≈95%+，满足 R-04 ≥70% 阈值」。
> 这些数字已全部删除（ADR-012）：它们是分析型估计，却极容易被当成 SLA 引用；
> 而且分母（「所有家用 NAT」）无法抽样，任何百分比都不具统计意义。
> 现在只承诺机制：**对称/CGNAT 走中继，其余优先直连，两条路都有明确失败反馈**。

## 4. 关键不确定性（须 M2 实测）

- **CGNAT 比例**：运营商级 NAT 使对称型占比在部分区域显著上升，直接拉低直连率；
  这正是 OPEN-QUESTIONS **Q4**（私有唤醒 UDP 在对称 NAT / CGNAT 下的实际成功率）的核心。
- **打洞成功率**：依赖对端也支持打洞与 STUN 端点对称性，需真实双端实测。
- **中继成本**：中继/LowID 回退的可达率上限取决于中继节点可用性与带宽，需 M2 部署实测。

## 5. Fallback 链（常开，不再由阈值触发）

**2026-11-16 修订（ADR-012）**：原来这里是「若 M2 实测 <70% 则提升中继优先级」。阈值取消后，
中继回退改为**常开**——不等一个永远不会到来的触发条件：对称 NAT 与 CGNAT 直接走中继，
锥形 NAT 优先直连、失败即降级中继。极端场景（双重 CGNAT）接受「唤醒延迟升高 + 经中继」而非直连。

## 6. M2 可复现验证方案

1. 搭建/租用 4 类 NAT 环境（Full/Restricted/Port-restricted/Symmetric，含至少 1 个 CGNAT）；
2. 对每类执行「唤醒 → 直连建链」「唤醒 → 中继建链」两类路径，记录成功率；
3. 输出真实 NAT 矩阵与聚合可达率；若 <70%，触发 §5 fallback 并复测。

## 7. 结论（当前）

S6 的穿透栈设计覆盖全部 4 类 NAT，分析型可达率 ≥95%（含中继），满足 R-04 ≥70% 阈值；
但量化判据须 M2 真实多 NAT 实测。当前 `S6.status = not_run`，不回滚技术路线。

## 8. 真实先导实测（2026-11-13，矩阵首个真实样本）

`stun_probe.py`：RFC 5389 STUN Binding（同一本地 UDP 端口访问 6 台服务器，4 台响应、2 个不同目的 IP）、
SSDP M-SEARCH（UPnP IGD）、NAT-PMP 外部地址请求、二跳 traceroute。全部为标准单次低速探测，
**未向公共 KAD / DHT 发布任何测试 Key**。

| 观测项 | 实测值 |
| :--- | :--- |
| 本机 / 网关 | `192.168.1.210` / `192.168.1.254` |
| STUN 映射地址 | `112.5.60.4:36764`，跨 4 台服务器（2 个目的 IP）**保持不变** |
| NAT 类型（本样本） | **cone 类 / 非对称型**（confidence: medium；cone 子型未区分） |
| 公网 UDP 出口 | 可达，RTT ≈ 245 ms |
| **UPnP IGD** | **不可用**（SSDP 无响应） |
| **NAT-PMP / PCP** | **不可用**（网关 5351 超时） |
| 二跳 / CGNAT | `112.5.60.1`（公网）；未见 CGNAT 迹象 |

**风险含义**：穿透栈的第一层（UPnP / NAT-PMP）在本环境**全部不可用**，可达性只能依赖 UDP 打洞与
中继 / LowID 回退。因此 R-04 的可达率估计**不能按「UPnP 可用」假设**给出；M2 须在四类真实 NAT
（含 CGNAT）逐类实测，并重点验证打洞 + 中继回退链路。

**判据覆盖**：1 / 4 类 NAT 真实样本，`evidence_valid_for_pass=false`，`S6.status` 仍 `not_run`。
2026-11-13 项目所有者批准 [G0 条件冻结](../../../docs/06-plan/G0-REVIEW.md) 豁免 S6 在 G0 时点给出结论，
M2 出口必须回填；若实测 <70%，按 fallback 提升中继优先级并默认 LowID 回退，且重新提交冻结评审。

报告：`reports/s6-nat-20261113T061908Z.json`（本地产物、不入库）。

> 排错留痕：首版把 STUN 地址属性的 family 误判为 `0x02`（实际 IPv4 = `0x01`、IPv6 = `0x02`），
> 导致 4 个已收到的有效响应被标为解码失败；修正后才取得真实映射地址。

## 9. W3.5① 执行协议与单出口精细分类（2026-11-14）

本节交付的是**矩阵执行协议**，不是矩阵结论。`S6.status` 仍 `not_run`，`matrix_coverage=0/4`。

### 9.1 条件审计

`tests/spikes/probe_conditions.py` 先判定可测性：本机无管理员权限、无 `tc/netem`、WSL 被安全策略禁止、
无全局 IPv6、虚拟交换出口 `172.26.96.1` 出站不可达（不能当作第二类独立真实 NAT）、
仅 1 类真实 NAT 且未检出 CGNAT。S3 / S5 / S6 均判 `PARTIAL_RUNNABLE`。

### 9.2 单出口精细分类（`nat_behavior.py`）

在 §8 的「cone 类 / 子型未区分」基础上补上带对照的 RFC 3489 Test II/III：

| 项 | 实测 |
| :--- | :--- |
| 映射行为 | **Endpoint-Independent**（同一 UDP socket 对 4 个不同目的 IP 映射恒为 `112.5.60.4:14848`，high） |
| 过滤行为 | **Address-and-Port-Dependent**（flags 2/4/6 在测试 socket 全部无应答，medium） |
| 经典分类 | **port-restricted cone** |
| 关键对照 | 备用地址直连使用**独立 socket**；服务器能力正向对照 `server_capability_confirmed=true` |
| hairpin | 未收到（不据此断言不支持，可能被本机防火墙丢弃） |
| CGNAT | 未检出，公网第二跳 `112.5.60.1`；UPnP IGD / NAT-PMP 仍不可用 |

**方法论修复**：备用地址直连对照若与过滤测试共用 socket，会为该 socket 的 NAT 映射预先开好放行项，
使随后的 `CHANGE-REQUEST` 回包不再是 unsolicited，实测曾把 port-restricted cone 误判成 full cone。
另需遍历 STUN 的全部 A 记录（该服务 DNS 轮询含失效节点）并对偶发丢包做有界重试。
`coldrevive-nat` 的 `full-cone` 是未做过滤细分的保守标签，与此结论不矛盾。

报告：`reports/s6-nat-behavior-final.json`。

### 9.3 四类 NAT + CGNAT 矩阵执行协议（`nat_matrix.py` / `nat_matrix_config.json`）

- 四类：`full-cone` / `restricted-cone` / `port-restricted-cone` / `symmetric`，**另须至少 1 个 CGNAT 环境**。
- 每类在**两台不同物理主机**上各跑 `coldrevived`，由实验执行人签 attestation（真实网关 WAN IP、
  两端内网 IP、两台 `machine_id`、真实 STUN 分类报告路径、可核查的 NAT 设备证据）。
- 每条路径（`direct` 打洞 / `relay` 回退）≥30 次真实唤醒；**成功只认真实 CLI 输出的
  `ok=true` 且 `ack_status=OK`**（即真实 `WAKEUP_ACK`）；超时 / 缺字段 / 非 JSON 一律计失败。
- `--aggregate` 仅在 4 类 + CGNAT 证据齐备且 `evidence_valid=true` 时计算聚合可达率；
  **该数字只作描述性统计，不与任何阈值比较**（2026-11-16 起，ADR-012 / §19）；**脚本仍不判定达标**。
- `--mode real` 拒收自报类型：attestation 的 `class` 必须与 `--class` 一致，且必须附真实 STUN 分类报告文件。

本机结果：`--dry-run` → `status=BLOCKED` / 退出码 2；`--aggregate` → `matrix_complete=false` /
`aggregate_reachability=null`。报告 `reports/s6-matrix-dryrun-final.json` 与 `s6-matrix-aggregate-final.json`。
真实公网 STUN 单出口样本**不计入矩阵覆盖率**（它只证明本机一个出口的映射/过滤行为，不是跨 NAT 双端可达率）。

> 红线：所有报告恒 `s6_status=not_run` / `g2_claim=false` / `spike_status_changed=false` / `no_auto_pass=true`；
> 即便四类证据齐备，`pass` / `fail` 仍由实验执行人依 `spikes.json` 的 `evidence_required` 手工回填。

## 10. W3.5② 低负担环境：单 Linux VM 内真改写四类 NAT + CGNAT（2026-11-14）

用户确认多 NAT 物理环境难准备，且**公共 KAD 不能替代 S6**（见下「为何不能连公共 KAD」）。
改用**单台 Linux 虚拟机** + `nat_lab/setup_nat_lab.sh`：在 network namespace 内用
iptables conntrack **真改写**出 Full / Restricted / Port-restricted / Symmetric 四类 NAT，
外加一层 CGNAT（双 NAT）。网关做的是内核级 NAT，因此从 LAN netns 内跑 `nat_behavior.py`
得到的 STUN 分类报告就是**真实 NAT 行为**证据，可直接作为 `--mode real` 的 `classification_report`。

- 脚本：`tests/spikes/s6/nat_lab/setup_nat_lab.sh`（`up` / `status` / `down`）。
  拓扑：主机 `br-wan 10.0.0.1`（relay 跑这里）+ 4 个单 NAT 网关 netns + 1 个双 NAT（CGNAT）网关 netns。
- 工作流：`setup_nat_lab.sh up` → 每类在 LAN netns 内跑 `nat_behavior.py` 出分类报告 →
  填 `attest/attest-<class>.template.json` → 两 netns 各起 `coldrevived` →
  `nat_matrix.py --mode real` 每类 ≥30 次真实唤醒 → `--aggregate`。详见 `nat_lab/README.md`。
- 诚实署名：虚拟机内仿真的 NAT 用 `iptables` 规则集作为 `real_nat_evidence`，并在 attestation 里
  如实说明是 VM 实验室而非独立路由器硬件；`machine_ids` 用两个不同 netns 端点标识。
- 生成物 `lab-evidence.json` / `attest/` 含本地 IP，已被 `nat_lab/.gitignore` 排除，不入库。

### 为何不能连公共 KAD 来测 S6

1. **协议错配**：S6 测的是 **ColdRevive 自己的唤醒链路**跨 NAT 可达率；公共 KAD（eMule/aMule）
   上**没有 ColdRevive 节点**，`cr-ed2k` 仍是骨架、`cr-kad` 明确只跑 `127.0.0.1` 私有子网。连上去测的是
   eMule 连通性，根本没跑 ColdRevive 的唤醒路径。
2. **判据错配**：S6 要求**按 NAT 类型拆分**的四类 + CGNAT 矩阵；公共 KAD 对端**不暴露自身 NAT 类型**，
   最多拿到一个混在一起的总可达率，`matrix_coverage` 仍是 `0/4`。
3. **合规红线**：`docs/06-plan/G0-REVIEW.md` 明确「本冻结**不授权**向公共 KAD 批量发布测试 Key」；
   S2（命名空间接纳，已 `pass`）是特例，与 S6 不是同一件事。


## 11. W3.5② 实验室实测结果（2026-11-15，单台 Ubuntu VM）

环境：Ubuntu 23.04 / kernel 6.2 / VMware 虚拟机（192.168.1.221，root）。
四类 NAT + CGNAT 由 `nat_lab/setup_nat_lab.sh` 在 netns 内用 iptables-legacy
conntrack 真改写；STUN 由 `nat_lab/local_stun.py` 跑在宿主 `br-wan`（10.0.0.1/10.0.0.2）。

### 11.1 逐类分类（`nat_behavior.py` 实跑）

| 环境 | 分类结论 | 置信度 | 关键对照 |
|---|---|---|---|
| full-cone | EIM + Endpoint-Independent Filtering | high | flags 2/4/6 全应答 |
| restricted-cone | EIM + Address-Dependent Filtering | high | flags=2 应答、flags=4/6 无应答 |
| port-restricted-cone | EIM + Address-and-Port-Dependent Filtering | medium | flags 2/4/6 全无应答，能力对照成立 |
| symmetric | Address/Port-Dependent Mapping | low | 同 socket 多目的映射端口互异 |
| cgnat | 双 NAT 之后的 port-restricted cone | medium | 内层 100.64/24 + 外层 SNAT |

### 11.2 逐类可达率（`nat_matrix.py --mode real --lab-mode --trials 30`）

| 持有者 NAT | direct（打洞） | relay（中继） |
|---|---|---|
| full-cone | **30/30** | 30/30 |
| restricted-cone | 0/30 | 30/30 |
| port-restricted-cone | 0/30 | 30/30 |
| symmetric | 0/30 | 30/30 |
| cgnat | 0/30 | 30/30 |

成功判据严格为真实 `ok=true` 且 `ack_status=OK`；direct 失败样本为真实 1216 ms
无应答（`Resource temporarily unavailable`），未回填任何默认值。

### 11.3 两条结论

1. **工程缺口（可修）**：当前只由请求方向对端映射地址发 `NAT_PUNCH`，因此只能进入
   full-cone 持有者。restricted / port-restricted 的过滤按「该内网 ip:port 曾发往的远端 IP」
   放行，必须**双向打洞 / rendezvous**（双方同时向对端映射发包）才能开各自过滤。
   这是 `remaining_for_g2` 的新增项，不是 NAT 不可穿透。
2. **仿真保真度的反证**：结果按 NAT 类型分化（full-cone 全通、其余全断），说明四类
   行为确实被改写成不同语义；若仿真把四类做成一样，不会出现这种分化。

### 11.4 证据边界

- 同机两个 netns，**不满足「两台不同物理主机」条款** → `--lab-mode` 强制
  `evidence_valid=false`、`evidence_scope=vm-namespace-lab`，并要求 attestation
  显式 `lab_mode=true`。
- `S6.status` 仍 `not_run`，`matrix_coverage` 不计入真实四类覆盖率，
  R-04 ≥70% 判定**未作出**，`g2_claim=false`。
- 报告存于 `tests/spikes/s6/reports/lab-20261115/`（gitignored 本地证据）。

## 12. W3.6 双向打洞复测（2026-11-15，同一实验室）

### 动机：把 0/30 归因到具体一步
W3.5② 的结果是 `direct` 只有全锥 30/30、其余四类 0/30。归因检查指向
`cr-nat::holepunch` 的**单向**语义：请求侧发 `NAT_PUNCH`，持有者只回 `NAT_PUNCH_ACK`，
从不主动回打。受限锥及以上的网关在收到**首个入向包**时既无 conntrack 条目、过滤集里也没有
对方地址，直接丢弃 —— 所以无论请求侧发多少次，持有者都看不到，也就永远回不了 ACK。

### 改动（W3.6）
1. `NAT_PUNCH` 帧可选携带 `"back":"ip:port,..."`（请求侧自身映射候选），
   解析侧只接受可路由 IPv4，去重并硬性限制 `MAX_BACK_TARGETS=4`（防放大）。
2. 持有者收到带 `back` 的帧后，**从唤醒监听 socket 主动回打**（`punch_back_to`），
   打开自身 NAT 过滤项；回打帧不再携带 `back`，避免链式放大；并由令牌桶限流。
3. 请求侧一次唤醒**只绑定一个 socket**：先在该 socket 上 STUN 取自身映射（`self_mapped`），
   再向直连候选热身一包，再经**中继**把带 `back` 的帧送给持有者，最后在同一 socket 上
   逐候选打洞 + 唤醒。单 socket 是硬要求 —— 映射端口随本地端口走，换 socket 报出去的
   `back` 必然是错地址。
4. `WakeAttempt` 新增 `peer_punched`：与 `punch_acked` 分开记，用于区分
   「对端已回打但我方收不到 ACK」（过滤方向不对称）与「完全没有回打」。

### 复测对照
修正了 W3.5② 的一处对照错误：当时 4/5 组的请求方在 `lan-symmetric` 后面，对称 NAT 的
映射随目的地址变化，`back` 永远报不准，是任何打洞方案都救不了的组合。本轮请求方固定为锥形 NAT。

| 组 | 持有者 | 请求方 | `direct` W3.5② → W3.6 | `relay` |
| :--- | :--- | :--- | :--- | :--- |
| a | full-cone | port-restricted | 30/30 → **30/30** | 30/30 |
| b | restricted-cone | port-restricted | **0/30 → 30/30** | 30/30 |
| c | port-restricted-cone | full-cone | **0/30 → 30/30** | 30/30 |
| d | symmetric | port-restricted | 0/30 → **0/30** | 30/30 |
| e | cgnat（双 NAT） | port-restricted | **0/30 → 30/30** | 30/30 |
| f | restricted-cone | symmetric | — → 30/30 | 30/30 |

30 次/路径，真实 CLI/UDP/HTTP 观测；六轮 `dropped=0`、`reset_err=0`。

### 判定
- 受限锥 / 端口受限锥 / 双 NAT CGNAT 直接由 0/30 变为 30/30：**缺的是「持有者先回打」这一步**，
  不是「NAT 不可穿透」。这同时说明 W3.5② 的 0/30 是工程缺口而非环境必然。
- **symmetric 持有者 0/30 是正确结果**：该网关无 DNAT、映射随目的变化，广告的 STUN 映射端点
  不是入向可达端口，其回打映射端口请求方又无从得知。只能走中继（与 WebRTC 实践一致）。
- f 组 30/30 不是异常：`restricted-cone` 的过滤是 address-dependent（ipset 键不含远端端口），
  回打到错误端口也按 IP 放开；`port-restricted-cone` 不用 ipset、只靠 conntrack（ip+port）。
  两者语义确实不同，这也是分类探针能把二者分开的依据。

### 新增的依赖（必须记录）
双向打洞目前**以中继作为信令通道**。中继不可用时 `back` 送不出去，直连会整体退化回
「只有全锥可通」。本轮实测到过一次这种情况（上一轮 relay 未真正监听，`connect refused`），
整轮 `relay` 0/30 且 `direct` 从 30/30 掉到 5/30 —— 因此中继不是「兜底」，而是双向打洞的
**前置依赖**，已加入 `remaining_for_g2`。

### 证据边界
- 仍为同机两个 netns，`--lab-mode` 强制 `evidence_valid=false`、
  `evidence_scope=vm-namespace-lab`；`S6.status` 仍 `not_run`，R-04 **未判定**，`g2_claim=false`。
- 六份报告存于 `tests/spikes/s6/reports/w36-20261115/`（gitignored 本地证据）。

## 13. 判据是两个独立条款，不是一条（2026-11-15 澄清）

W3.5②/W3.6 的实验室证据被标 `evidence_valid=false`，容易被读成「环境不合格」。
真正的理由要拆成两条，它们**互相独立**，而且第二条不是「换机器」能解决的：

| # | 条款 | 出处 | 实验室现状 | 再加一台主机能否解决 |
| :--- | :--- | :--- | :--- | :--- |
| 1 | 每类环境在**两台不同物理主机**上各启动 `coldrevived` | `nat_matrix_config.json` → `measurement_protocol[0]` | ❌ 同机两个 netns（`machine_ids` 的 host 部分相同） | ✅ 能 |
| 2 | `real_nat_evidence`：**真实 NAT 设备**的可核查证据 | 同上 → `required_attestation` | ❌ iptables/conntrack 合成的 NAT | ❌ **不能**，合成 NAT 不因换主机而变成真实设备 |

由此还有一条更根本的限制：**一个家庭/办公网络的出口只对应一个真实 NAT 类别**。
判据要求 4 类（+1 个 CGNAT），那就必须来自 **4 个以上不同出口**——这与「有几台主机」正交。
换句话说 S6 是**环境数量受限**，不是主机数量受限；加主机只会让条款 1 成立，条款 2 和「类别多样性」都不会动。

### 13.1 证据层级（`nat_matrix.py --evidence-scope`）

`--lab-mode` 原先是一个布尔值，把两条条款混在一起，报告说不出「缺哪一条」。现已改为显式层级：

| `--evidence-scope` | 含义 | `evidence_valid` | 用途 |
| :--- | :--- | :--- | :--- |
| `real-multi-nat`（缺省） | 真实 NAT 设备 + 两端不同物理主机 | 条款全满足时可 `true` | S6 唯一合规口径 |
| `cross-host-lab` | 合成 NAT + 两端分处不同主机 | 恒 `false` | 排除「同内核」保真度质疑 |
| `vm-namespace-lab` | 合成 NAT + 同机两个 netns（`--lab-mode` 为兼容别名） | 恒 `false` | 机制验证（W3.5②/W3.6 现状） |

报告新增 `evidence_clauses`（逐条 `satisfied` + 观测到的 host 列表）与 `evidence_invalid_reasons`
（直接写明缺哪条）。「两台主机」的判定比的是 `machine_ids` 的 **host 部分**，
`<ns>@<host>` 里 ns 不同但 host 相同不算两台主机——这正是同机多 netns 不能冒充的原因。

### 13.2 闭合 S6 的三条路线

| 路线 | 需要什么 | 产出 | 代价 |
| :--- | :--- | :--- | :--- |
| **A. 真实设备级联** | 2–4 台真实家用路由器（含 1 个 CGNAT 出口），每类环境两台主机 | 条款 1 + 条款 2 均成立，可逐类判 R-04 | 需硬件；且真实设备**类别不可选**，凑齐 4 类往往要 4 个不同出口 |
| **B. 跨主机实验室** | 第二台 Linux 主机（能跑 `ip netns`）| 条款 1 成立、条款 2 仍不成立；保真度提升一档 | 一台 Linux 机器；仍不能判 R-04 |
| **C. 公网多节点实测** | ≥8 个真实网络上的节点 + 公网中继 + G0 授权测试 Key | **直接给出 R-04 所需的可达率**，类别分布来自真实节点总体 | 部署与运营成本；但这是唯一能产出「≥70% 是否成立」这个数字的路线 |

推荐：**C 为主线**（R-04 问的是真实家用 NAT 总体的可达率，矩阵本身只是手段），
**A 作为条款合规的最小闭环**（哪怕只覆盖 1–2 类，也能把「真实设备」这条从 ❌ 变成部分 ✅）。
B 只在有第二台 Linux 主机时顺手做，用于消除「同内核」质疑，不指望它给出结论。

### 13.3 本轮复核（2026-11-15，新字段生效后）

`restricted-cone` 持有者 + `port-restricted-cone` 请求者，30 次/路径：
`direct` **30/30**、`relay` **30/30**、中继 `delivered=120 / dropped=0`；
报告 `evidence_scope=vm-namespace-lab`、`hosts=['ubuntu']`、条款 1/2 均 `false`。
数值与 W3.6 一致，说明层级改造没有改变测量口径，只把「为什么不能判 pass」写清楚了。

## 14. 跨主机档位：把请求者挪到第二台物理主机（2026-11-15，v2.4.54）

### 14.1 回应的问题
「实验室是同机两个 netns，不满足两台物理主机」这句话容易被读成「环境不合格」。
需要把一件事说准：**实验机 221 本身就是一台主机，它让四类 NAT 得以建起来**；
不满足条款①的不是「机器不够」，而是**一次测量里的两个端点都在 221 内部**。
把其中一个端点挪到 A（192.168.1.210，另一台物理机）就成立 —— 本轮做到了。

### 14.2 为什么不是"在 A 上加一条路由"
最直觉的做法是让 A 能路由到实验机的内部 WAN 网段 10.0.0.0/24。实测两条路都不通：

| 方案 | 实测结果 |
| :--- | :--- |
| `route add 10.0.0.0/24 via 192.168.1.221` | A 是**非管理员** Windows 账户，改不了路由表 |
| macvlan 把网关挂到物理 LAN | VMware vSwitch 默认 **拒绝混杂模式/伪传输**，`ping` 100% 丢包、ARP 无应答 |

改用**反方向**：给网关的 WAN 口再挂一个**物理 LAN 上的地址**（.241~.245），
由 221 在 `ens160` 上做 **proxy ARP** 替它应答。数据路径每个方向只多一跳、**不改写 IP**：

```
A(192.168.1.210) --ARP 由 221 代答--> 221 ens160 --/32 路由--> br-wan
    --> 网关 netns 的 WAN 口(192.168.1.24X) --SNAT--> 内网 holder(192.168.X.2)
反向：网关把包发给"邻居"（静态写死为 br-wan 的 MAC），221 按 /32 转出 ens160
```

好处：**对 A 零改动、零权限**，且 SNAT 目标地址就是 A 看到的对端地址，
NAT 语义没有被额外一层改写污染。

### 14.3 新增的两个工具

| 文件 | 作用 |
| :--- | :--- |
| `nat_lab/link_physical.sh` | `up <class>` / `down` / `status` / `stun-lan-start` / `stun-lan-stop`：暴露网关到物理 LAN（含 SNAT 改写与静态邻居，对称型保留 `--random-fully`） |
| `nat_lab/run_cross_host.py` | 在 A 上运行：ssh 编排 221 侧（链路 → STUN → 中继 → 持有者 → 通告），本机起请求者，再调 `nat_matrix.py --evidence-scope cross-host-lab` |

`hold` 一侧的事都在 221 上，`nat wake` 的直连/中继命令都在 A 上执行 ——
**测量姿态与真实使用一致**（不是"在实验机里模拟一个外部主机"）。

### 14.4 结果（30 次/路径，`delivered=120 / dropped=0`）

先看**分类**：同一条物理链路上用 `nat_behavior.py` 重测，5/5 与建网声明一致 ——
这也是链路保真度的反证（代理路径若改坏了 NAT 语义，分类会先露馅）：

| 建网类型 | 物理链路实测分类 | 置信度 |
| :--- | :--- | :--- |
| full-cone | Endpoint-Independent Mapping + Endpoint-Independent Filtering | high |
| restricted-cone | EIM + Address-Dependent Filtering | high |
| port-restricted-cone | EIM + Address-and-Port-Dependent Filtering | medium |
| symmetric | Address/Port-Dependent Mapping | low |
| cgnat（双 NAT） | EIM + Address-and-Port-Dependent Filtering | medium |

再看**可达率**：

| 持有者（221 的 netns） | 请求者 | `direct` | `relay` |
| :--- | :--- | :--- | :--- |
| full-cone | A @192.168.1.210 | **30/30** | 30/30 |
| restricted-cone | A @192.168.1.210 | **30/30** | 30/30 |
| port-restricted-cone | A @192.168.1.210 | **30/30** | 30/30 |
| symmetric | A @192.168.1.210 | **0/30** | 30/30 |
| cgnat（双 NAT） | A @192.168.1.210 | **30/30** | 30/30 |

与 W3.6（同机 netns）逐行一致 —— 说明 W3.6 的 30/30 不是"同内核"造成的假象。

> ⚠ **这张表是修完三个 harness 缺陷之后才成立的**（见 §14.7）。数字和第一版一模一样，
> 但第一版的数字是在「ipset 从没真正 flush 过、三个地址指向别人的机器、STUN 收敛没等」
> 的条件下拿到的 —— 一次读成 30/30、下一次读成 0/30，都属于不可复现的读数。
> §14.7 记录了三处修复，本表以修复后的那次全量重跑为准（每类 `reset_exit_code` 全为 0）。

### 14.5 判定：条款①成立，条款②仍不成立

报告里现在写得很直白：

```
evidence_scope = cross-host-lab
hosts = ['DESKTOP-DJQSC95', 'ubuntu']   ← 两台不同主机
clause1 two_distinct_machines = true
clause2 nat_is_real_device    = false   ← 合成 NAT，换多少台主机都不变
evidence_valid = false
```

所以：**S6 仍未判定，R-04 ≥70% 仍不能给结论**。
本轮拿到的是"排除同内核质疑"这一档证据，不是过关。
要真正闭合条款②，仍然只有两条路：接入真实 NAT 设备（路线 A），
或在 ≥8 个真实网络上实测（路线 C，也是唯一能给出 R-04 那个数字的路线）。

### 14.6 顺带暴露的产品缺陷（`CLASSIFY-CONE-SUBTYPE-FABRICATED`，v2.4.55 已修复）

**症状**：`coldrevive-cli nat probe` 在五类环境里**全部**报 `restricted-cone`，包括 full-cone。

**根因**：`cr-nat/src/classify.rs::classify_cone_subtype` 比较的是
**NAT 映射地址**与 **STUN 服务器的 OTHER-ADDRESS**，两者语义无关；
只要服务器给了 OTHER-ADDRESS（正常情况都会给），IP 必然不同，就必然判成 `restricted-cone`。
也就是说这个字段此前一直是**恒定值**，不是测量结果 —— 它一直在骗人，而且骗得"合理"。

**修法**：在 `cr-nat/src/stun.rs` 补齐 RFC 3489 Test II/III ——
发 `CHANGE-REQUEST(0x0003)` 并看**是否收到应答**（关键是"有没有回包"，不是"回了什么"）：

| 观测 | 结论 |
| :--- | :--- |
| 无 `CHANGE-REQUEST` 的正向对照都收不到应答 | `Unknown`（服务器不可达 ≠ 端口受限锥，不能默认结论） |
| change-IP（`0x04`）有应答 | Full cone |
| 仅 change-port（`0x02`）有应答 | Restricted cone |
| 两者都无应答 | Port-restricted cone |

`classify.rs` 里那个"聪明"的 `classify_cone_subtype` 被**删掉**了，
换成显式的 `FilterBehavior` 观测类型 + `classify_from_filter`——
逼着调用方先拿到测量值再下结论，而不是从地址差异里"推断"。

**修复过程中实验室抓到的第二个坑（假阳性）**：
第一版把过滤测试跑在**映射探测用过的那个 socket** 上，结果 restricted / port-restricted / cgnat
全被误判成 `full cone`。原因不是代码写错，而是**环境被自己污染**：
映射探测同时问了主备两台 STUN，网关的过滤集里已经有了备用地址，
于是 change-IP 的回包被正常放行 —— 受限锥看起来像全锥。
**过滤测试必须用一个"只跟主服务器说过话"的 socket**，`filter_behavior_fresh()` 为此存在。

**修复后实测**（同一条物理链路，五类逐一对齐 `nat_behavior.py`）：

```
full-cone            → full-cone
restricted-cone      → restricted-cone
port-restricted-cone → port-restricted-cone
symmetric            → symmetric
cgnat                → port-restricted-cone
```

与改动前的恒定 `restricted-cone` 相比，现在是**五类五个值**，且与权威探针 5/5 一致。

**门禁影响**：无。分类修对了不等于 NAT 变真实了，
`evidence_valid` 仍为 `false`、S6 仍 `not_run`、R-04 仍不判定。
修的是**产品缺陷**，不是门禁结论。

### 14.7 三个 harness 缺陷：为什么第一版数字不可复现（2026-11-15）

§14.4 那张表看起来"一次就跑出来了"，实际上中间有三处**看起来有数字、其实没测**的坑。
三个坑的共同形态都是：脚本照常输出、报告照常生成、成功率照常漂亮，但底层没测。

#### 14.7.1 `RESET-NEVER-RAN` —— ipset 复位从未执行

`reset.sh` 由 Python 在 Windows 上 `write_text()` 落盘，`\n` 被翻成 `\r\n`；
远端 bash 读到 `do\r` 直接语法错误：

```
/opt/coldrevive/cross/cgnat/reset.sh: line 1: syntax error near unexpected token `$'do\r''
```

于是每轮试验前**网关的 `CONTACTED` 集合根本没被清空**。第一次开洞之后，
余下的试验全都在"吃老本"：过滤集里早有对端，压根不需要打洞也能通。
**修法**：`reset_local.write_bytes(RESET_SH.encode("utf-8"))`；
每一轮都记录 `reset_exit_code`，跑完统计非 0 的条数并告警（现在五类全为 `0`）。

#### 14.7.2 `ADDR-POOL-COLLISION` —— punch 发给了别人的机器

物理地址按类写死 `.241`~`.245`，但用户的家庭 LAN 上**这些地址已经被真实设备占着**。
A 的 ARP 表是铁证：

| 地址 | A 看到的 MAC | 是谁 |
| :--- | :--- | :--- |
| `192.168.1.241` | `00-0c-29-b6-ab-11` | 221（VMware OUI，proxy ARP 生效） |
| `192.168.1.242` | `f4-74-88-f2-c6-ee` | **别人** |
| `192.168.1.243` | `00-0c-29-b6-ab-11` | 221 |
| `192.168.1.244` | `f4-74-88-f0-12-36` | **别人** |
| `192.168.1.245` | `f4-74-88-f0-0d-26` | **别人** |

失败的正好是被占的那三类。更糟的是**预检是假的**：原来只 `ping` 一下，
"有应答"就算通 —— 应答的是邻居家的设备，不是实验机。
在 `gw-restricted-cone` 的 WAN 口抓包，只看到出向包、**一个入向包都没有**：

```
09:24:13.003270 IP 192.168.1.242.19901 > 192.168.1.210.62566: UDP, length 71
09:24:13.003371 IP 192.168.1.242.19901 > 192.168.1.210.62566: UDP, length 71
```

**修法**：地址不再写死。驱动先取 `ens160` 的 MAC，从 `.236`~`.253` 里挑一个
ARP 表里没有"别人的 MAC"的地址，`up` 之后再**校验应答者身份**：

```
A → 192.168.1.236 的应答者 MAC = 00:0c:29:b6:ab:11（= ubuntu ens160）
```

不相等就 `SystemExit`，一个字的证据都不产出。可达性→身份的这一步替换，
是本次最重要的修正：**"ping 得通"从来不是"通到了谁"**。

#### 14.7.3 `LIFECYCLE-SILENT-DEGRADE` —— 环境塌了，报告还"成功"

物理口 STUN 用 `nohup` 起、没 `setsid`，于是**启动它的那条 ssh 一退出，进程就被带走**。
后果不是报错，而是静默降级：

- 分类变成 `undetermined`（`STUN 不可达，如实不判`）—— 这是对的；
- `nat probe` 变成 `nat_type=unknown / mapped=None` —— 也是对的；
- 但矩阵照跑，候选只剩 `private-lan` + `relay`，**成功率依然漂亮**。

也就是说，测的已经不是"NAT 穿透"了，报告却看不出来。**修法**三层：

1. `link_physical.sh` 改用 `setsid nohup`（与中继/持有者一致）。
2. 驱动在 A 上**真发一个 STUN binding request** 确认双端点都活着（不看 `ss`）。
3. 再加一步**收敛等待**：链路刚 up 时 netns→STUN 有一段时间不通（单层 NAT 约 10s，
   双层 CGNAT 实测 **28.8s** 才收敛）。分类前必须等到真通，否则如实报"没测"并中止。
4. 分类为 `undetermined`、或 `nat_probe` 拿不到 `mapped_address` → 直接失败。

#### 14.7.4 小结

| 缺陷 | 误读成 | 真因 | 现在的处置 |
| :--- | :--- | :--- | :--- |
| `RESET-NEVER-RAN` | 30/30（吃老本） | CRLF 让复位脚本语法错误 | 按字节写 + `reset_exit_code` 全量校验 |
| `ADDR-POOL-COLLISION` | "唤醒无应答"（像 NAT 不行） | 地址被别人占，punch 打错机器 | 动态选空闲地址 + ARP 身份校验 |
| `LIFECYCLE-SILENT-DEGRADE` | "只有中继也可用" | STUN 进程被 ssh 带走 | `setsid` + 真探针 + 收敛等待 + 硬失败 |

三者都不改变门禁结论（条款②仍不成立），但它们说明一件事：
**"成功率"这个数字本身不会提示自己是不是真的测过**。
所以每一条都要有独立的反证 —— 分类复测、身份 MAC、`reset_exit_code`、STUN 往返 ——
缺一条，那个数字就不该写进报告。


## 15. 真实 NAT 档位：两端各自在真实 NAT 后面（2026-11-15，v2.4.56）

### 15.1 这一档与前两档的差别

前两档里 NAT 都是**我们自己用 iptables/conntrack 合成的**：语义可控、可复现，
但它是"我们相信 NAT 应该这样"，不是"真实设备确实这样"。本档把两个端点都放到
**真实设备后面**，NAT 由用户的网络设备完成，我们一行规则都没写。

| 档位 | 持有者 | 请求者 | NAT 由谁实现 | `evidence_scope` |
| :--- | :--- | :--- | :--- | :--- |
| 同机 netns | 221 内 netns | 221 内另一 netns | 我们（iptables） | `vm-namespace-lab` |
| 跨主机 | 221 内 netns | A @192.168.1.210 | 我们（iptables） | `cross-host-lab` |
| **本档** | **222 @192.168.34.222** | **A @192.168.1.210** | **真实设备** | **`real-multi-nat`** |

拓扑（全部实测）：

| 端点 | 本机地址 | 出口公网 IP | 说明 |
| :--- | :--- | :--- | :--- |
| 持有者 `ubuntu23`（222） | 192.168.34.222（ens160） | **112.5.32.34** | 经 192.168.34.1 出去；另一块 ens192 只有内网、无网关 |
| 请求者 `DESKTOP-DJQSC95`（A） | 192.168.1.210 | **112.5.60.4** | 家庭宽带出口 |
| 中继 | — | 192.168.1.221:7100 | LAN 内，**只做信令**，不承载数据面 |

两个出口 IP 不同、都 ≠ 本机地址，且都通过了真实公网 STUN 的映射观测 —— 这才叫
"映射由我方代码之外的设备完成"。

**怎么证明"不是我们自己造的全锥"**（这条最容易作弊，所以单列）：

1. STUN 观测到的映射端口（2053~2056）**≠ 内部监听端口 19901**，且每次不同 → 动态分配，
   不存在为该端口配置的静态映射；
2. 用户在 192.168.34.1 上只做了 `54022→22`、`57100→7100` 两条映射，与测试端口无关；
3. 数据面直连 attempt 的目标地址是 `112.5.32.34:2053`（公网侧），不是 `192.168.34.222`
   —— 没有走 LAN 捷径。

> **2026-11-15 23:2x 更正：上面这条早期结论是错的，映射其实是通的。**
>
> 用户从自己的 Mac（外网侧）实连 `112.5.32.34:54022` **登录成功**；随后我在 222 上
> `tcpdump -i ens160 -n 'tcp and (port 22 or port 54022)'`，并从 A（出口 112.5.60.4）发起连接，
> 抓到完整握手 + 关闭：
>
> ```
> 15:21:00.836268 IP 112.5.60.4.28698 > 192.168.34.222.22: Flags [S]
> 15:21:00.836334 IP 192.168.34.222.22 > 112.5.60.4.28698: Flags [S.]
> 15:21:00.838548 IP 112.5.60.4.28698 > 192.168.34.222.22: Flags [.]
> 15:21:00.838549 IP 112.5.60.4.28698 > 192.168.34.222.22: Flags [F.]
> 15:21:00.839945 IP 192.168.34.222.22 > 112.5.60.4.28698: Flags [.]
> 5 packets captured
> ```
>
> 即该设备的 **DNAT 与 NAT 环回（hairpin）都正常**：LAN 侧源地址被 SNAT 改写成出口
> `112.5.60.4` 后回注内网，端口也被改写成 28698，所以只按 `port 54022` 抓包会漏——
> 这也解释了当初为什么会得出"0 packets captured"。
>
> **教训**：`tcpdump` 过滤端口时，NAT 会同时改写**源端口**，只盯内网侧原端口会误判成"不通"。
> 抓包要按**对端公网 IP** 过滤，而不是按端口。这条和 W3.6c 的"按对端公网 IP 抓"是同一个坑，
> 我在中继侧记住了、在映射侧又踩了一次。
>
> 这条不影响 §15 的任何数字（数据面走的是 `112.5.32.34:2053` 公网侧，与 `:54022` 无关），
> 但它带来两个可用资源：① 222 有一条**可从公网直入的管理面**；② `57100→7100` 这条公网端口
> 在 W3.9（中继公网生产化）里可以直接当作**真实公网中继入口**，不必另找 VPS。

### 15.2 222 到底是什么 NAT：一次"假阳性的分类"被现场推翻

同一台机器、同一天，两次分类给出不同结论：

| 时间 | 结论 | 可信度 |
| :--- | :--- | :--- |
| 22:4x 首次分类 | `port-restricted cone`（EIM + APDF） | ❌ **假阳性** |
| 23:0x 重跑 | `symmetric`（Address/Port-Dependent Mapping） | ✅ 与手工实验一致 |

手工受控实验（不经过任何分类脚本，直接看报文）：

```
同一个 socket，本地端口固定在 50114，目的端口都是 3478，只换目的 IP：
  stun.cloudflare.com  162.159.207.0   -> 112.5.32.34:2051
  stun.voipbuster.com   77.72.169.210  -> 112.5.32.34:2055
  stun.telnyx.com      192.76.120.66   -> 112.5.32.34:2050
```

本地端口没变、映射端口变了 → **Address/Port-Dependent Mapping**，即 symmetric。
EIM 的定义恰恰是"映射与目的无关"，这里明显不成立。

**为什么会误判成 cone**：当时只有一台 STUN 有应答（其余节点超时），
`classify_from_mappings` 拿到**一个样本**就返回 `FullCone`（注释写着"至少是非对称"），
再叠加过滤测试的假阳性，就变成了 `full-cone` / `port-restricted cone`。
**一个样本不能判对称性** —— 这是原理问题，不是调参问题。

### 15.3 因此修掉两个"编造分类"的缺陷（v2.4.56）

| 缺陷 | 症状 | 根因 | 修法 |
| :--- | :--- | :--- | :--- |
| `CLASSIFY-SINGLE-SAMPLE` | 只有 1 台 STUN 有应答时判成 cone | `classify_from_mappings` 单样本即返回 `FullCone` | 要求 **≥2 个不同目的地址**，否则 `Unknown`；且只有确认 EIM 后才用过滤测试细分 |
| `CHANGE-REQ-NO-SOURCE-CHECK` | 忽略 CHANGE-REQUEST 的服务器回包也被算"change-IP 有应答" | `change_request_answered` 只校验 tid，**不校验回包源地址** | 正对照要求 `src == addr`；change 测试要求 `src != addr`（回包必须来自另一个套接字） |

修复后同一台 222 的 `coldrevive-cli nat probe` 输出 `nat_type=symmetric`，
与 `nat_behavior.py` 的现场分类一致（此前同一分钟内一个说 `full-cone`、一个说 `symmetric`）。

新增回归测试：`filter_ignored_change_request_is_not_full_cone`（服务器永远从主地址回包
→ 必须判 `PortRestrictedCone`，不得判 full cone）、
`single_destination_sample_is_unknown_not_cone`。

顺带修掉一个**偶发失败**的中继测试（`routes_frames_between_two_real_tcp_clients`
实测约 1/3 失败）：客户端收到帧时服务端计数线程还没自增，断言读到旧值。
改成有界等待。偶发失败的测试等于没有测试。

### 15.4 结果（30 次/路径，真实设备）

| 持有者（222，真实 NAT → 112.5.32.34） | 请求者 | `direct` | `relay` |
| :--- | :--- | :--- | :--- |
| symmetric（APDM，现场复测确认） | A @192.168.1.210（真实 NAT → 112.5.60.4） | **0/30** | **30/30** |

中继计数器（本轮增量）：`delivered=108`、`dropped=0`、`rejected=0`。

报告：`status=OBSERVED`、`evidence_scope=real-multi-nat`、
`evidence_valid=true`、条款①`true`（hosts=`['DESKTOP-DJQSC95','ubuntu23']`）、
条款②`true`（`nat_is_real_device=true`，附可核查证据文件）。
**这是项目第一次拿到 `evidence_valid=true` 的单类报告。**

### 15.5 为什么 direct 必须是 0/30（这不是缺陷，是定义）

在 222 的 `ens160` 上按**对端公网 IP**抓包（不能按端口抓 —— 过 NAT 后源端口已改写）：

```
出向 2 条：192.168.34.222.19901 > 112.5.60.4.57238  UDP length 71
入向 0 条：A 打向 112.5.32.34:2053 的包，一条都没到 222
```

因果链完整：

1. 请求者经中继把 `NAT_PUNCH` 送到持有者（**信令成功**）；
2. 持有者从唤醒 socket 回打到 A 的真实映射 `112.5.60.4:57238`（**出向成功**，抓到了）；
3. 但 A 打向 `112.5.32.34:2053` 的包被**222 的 NAT 丢弃** —— 因为 APDM 下
   2053 这个映射是"对 STUN 服务器"建立的，只接受那个源的入向；
4. A 收不到 `WAKEUP_ACK`，如实记为失败。

所以 symmetric 下双向打洞在**原理上**不可能成功（除非做端口预测，我们没有做，也不打算做）。
这一档的价值不是"证明打洞有效"，而是**用真实设备确认了 W3.6 的结论方向**：
此前"中继是前置依赖"只有 iptables 合成的 symmetric 支撑，现在有了商用设备的现场证据。

### 15.6 这一档解决了什么、没解决什么

**解决了**：条款②第一次有真实、可核查、可复现的证据（两个真实出口 + 两台物理主机 + 30 次/路径）。

**没解决**（不得含糊）：

* 类别覆盖仍是 **1/4 + 0 CGNAT**。这几台机器只有两个真实出口，而一个出口只对应一类；
  full cone 要特定路由器配置或公网直连，restricted cone 要有状态防火墙，CGNAT 要运营商级，
  在家里凑不齐。
* 本档这一类的 `direct` 是 **0%**，所以它**不能**用来证明"打洞在某类真实 NAT 上有效"。
  要那种证据，需要至少一个真实的 **EIM + 有限过滤** 出口（例如手机热点或另一条宽带
  恰好是 port-restricted cone）。
* `S6.status` 仍 `not_run`、`evidence_valid_for_pass` 仍 `false`、R-04 仍不判定。
  `evidence_valid=true` 只表示"这份证据的取证过程合法"，**不等于该类达标**。

> 口径提醒：报告里的 `overall_reachability=1.0` 是"任意路径可用率"（`1-(1-d)(1-r)`）。
> 中继恒定可用时它永远接近 1，**不能当直连可达率读**。直连必须看 `paths.direct.reachability`。

## 16. 中继去单点化：缺的是状态，不是网络（2026-11-15，v2.4.57）

### 16.1 先有一条会红的用例

W3.7 建双实例基线时留了两个中继：`221:7100` 与 `209:7100`。两个实例都活着、网络全通、
帧格式合法，唯一的现象是——A 注册在 221、B 注册在 209 时，B 发给 A 的帧到了 209 **只有
`dropped=1`**。

这条 `dropped` 就是缺口本身：**每个实例的注册表是私有的**。它把两件事区分得很干净：
不是"中继连不通"，也不是"帧编码错"，而是"对端不知道 A 存在"。所以去单点化要补的是**状态
同步**，不是重连、不是重试。

> 这条用例被写进了 `relay_probe.py --mode cross`，并且在 W3.8 之前**一直对着红**。
> 保留一条"当前必然失败"的用例，比在文档里写一句"待办：去单点化"更难自欺。

### 16.2 补了三个帧（只在中继之间，客户端不发）

| 帧 | 值 | 负载 | 语义 |
| :--- | :--- | :--- | :--- |
| `PEER_HELLO` | `0x05` | 呼叫方 16B 实例 id | 对端回 `ACK(node=应答方实例 id)`，一条 TCP 全双工，两向都能发 |
| `PEER_SYNC` | `0x06` | n × 16B 本地节点 id | **整份覆盖**：掉线节点自然消失 |
| `FORWARD` | `0x07` | 16B 来源 + 16B 目标 + 消息 | 收到后**只做本地投递，不再转发** → 最多一跳，无环 |

两个设计选择值得写下来，因为都是踩过的坑：

1. **覆盖语义而不是增量。** 增量同步在"实例崩溃"时无法区分"它没变化"和"它没了"，
   于是会留下**幽灵归属**：帧被算成 `forwarded` 然后消失，调用侧只看到超时——
   这比直接报错难查得多。所以 `FORWARD` 到达一个已经不持有目标的实例时，记
   `forward_dropped`，并且每隔 500 ms 用整份快照覆盖一次。
2. **转发只一跳。** 如果收到 `FORWARD` 还继续转发，多实例之间就会成环（A→B→A），
   而且丢包路径不可解释。限制成一跳后，最坏情况是"目标不在直接同侪上"→ dropped，
   行为可预测。

### 16.3 实测（两台真实主机、真实 LAN）

| 用例 | 结果 | 说明 |
| :--- | :--- | :--- |
| 跨实例双向 30 轮（60 帧） | **60/60 送达** | RTT min 0.3 / max 0.6 / avg 0.3 ms；来源 id 与负载逐字节保持 |
| 两端计数 | `relay-221` delivered 31 / forwarded 30 / dropped 0；`relay-209` delivered 30 / forwarded 31 / dropped 0 | 互补关系与双向帧数一致（B→A 31 帧含上一轮） |
| 快照自愈 | **PASS**，重启后首帧 1.0 ms 送达 | 持有者全程**没有重新注册**；`taskkill` + `schtasks /run` 重启 relay-209 后由新链路重推快照 |

**反证（这条最重要）**：在同一台机器上再起一个 `7101` 实例，**不配 `--peer`**，对同一节点跑
同一条 `cross` 用例 —— 立刻 `FAIL`，该实例 `dropped=1 / forwarded=0 / peers=0`。
同一条用例在缺快照同步时立即失败，才能说明上面的 PASS 来自协议而不是环境偶然。

### 16.4 客户端侧：列表而不是单点

`nat.relay` 现在接受逗号/分号分隔的列表（单值与旧行为等价，非法项直接丢弃）。

- `holepunch::relay_wake_any` 顺序尝试，返回**真实成功的那一个**地址；全部失败时聚合
  每一条的真实错误（区分"连不上""未注册""对端没同步"），不压成一句"中继失败"。
- `relay_sub` 断开后从**下一个**中继开始重试。
- 顺序而不是并发：多中继在这里是"可切换"，不是任播；并发抢连会让"到底走了哪条路"
  变得不可解释。

### 16.5 这一节解决了什么、没解决什么

解决：

- 单个实例宕机不再等于"注册在它上面的节点全部失联"；
- 同侪重启后快照自愈，持有者不必重连；
- 客户端配置列表即可切换，不必改代码。

没解决：

- **不是共识，也不是任播**。快照最终一致，转发是**一次性尽力投递**：链路断在半路只计
  `forward_dropped`，不重传、不做多副本。
- **没有鉴权**。`PEER_HELLO` 无凭证，多实例互信目前靠网络隔离；公网部署前必须补
  TLS / 身份（W3.9）。`REGISTER` 至今也只校验负载回显。
- **没有限流与空闲超时**；注册表是全量快照，实例数上千需要改成增量/分片。
- **与 S6 判定无关**。这是中继可用性工程，没有产生任何 NAT 类别样本：`S6.status` 仍
  `not_run`、`g2_claim=false`、R-04 不判定。

### 16.6 顺带纠正：映射其实一直是通的（抓包口径错）

上一轮（§15.1）记过一句"`54022` 从外面连不通、`0 packets captured`"。**这是错的。**

用户从自己的 Mac（外网侧）实连 `112.5.32.34:54022` **登录成功**。复测：

```
# 222 上
tcpdump -i ens160 -n -c 5 'tcp and (port 22 or port 54022)'
# A（出口 112.5.60.4）发起连接
15:21:00.836268 IP 112.5.60.4.28698 > 192.168.34.222.22: Flags [S]
15:21:00.836334 IP 192.168.34.222.22 > 112.5.60.4.28698: Flags [S.]
15:21:00.838548 IP 112.5.60.4.28698 > 192.168.34.222.22: Flags [.]
15:21:00.838549 IP 112.5.60.4.28698 > 192.168.34.222.22: Flags [F.]
15:21:00.839945 IP 192.168.34.222.22 > 112.5.60.4.28698: Flags [.]
5 packets captured
```

DNAT **与 hairpin 都正常**：源地址被 SNAT 改写成出口 `112.5.60.4`、**源端口也被改写**
（54022 → 28698）再回注内网。所以只按内网侧原端口过滤必然为空——这正是当初误判的根因。
W3.6c 我为"按对端公网 IP 抓包"写过一条教训，这里又在映射侧踩了同一个坑：**NAT 会改端口，
过滤要按对端 IP**。

这条不影响 §15 的任何数字（数据面走 `112.5.32.34:2053` 公网侧，与 `:54022` 无关），
但它带来一个可直接用上的资源：`57100→192.168.34.222:7100` 是一条**真实公网端口**，
W3.9 的中继公网生产化不必另找 VPS。

## 17. 中继公网生产化（W3.9，2026-11-16，v2.4.58）

### 17.1 为什么这一步必须做

W3.6 起直连依赖中继做信令（单点化在 W3.8 解决了「实例挂了」，但没解决「谁都能连」）。
把这样一个中继直接摆到公网，等于给所有人一个免费的消息转发器：任何人可以注册任意 16 字节
id，把别人的 id 顶掉，广播任意帧；链路是明文，载荷里是唤醒请求与应答。所以「公网化」不是
加个端口映射，而是**先让它在被恶意对待时仍然只服务于持钥方**。

### 17.2 传输层：PSK 双向认证 + 逐记录加密（不是 TLS）

std-only 零第三方依赖是仓库硬约束，引 TLS 就要引 crate 与证书体系。这里用预共享密钥：

```text
客户端 hello  "CRRL" | ver=2 | nonce_c(32) | HMAC(psk,"CRRLY2 client-hello"|nonce_c)
服务端 hello  ver=2 | nonce_s(32) | HMAC(psk,"CRRLY2 server-hello"|nonce_c|nonce_s)
master        HMAC(psk,"CRRLY2 master"|nonce_c|nonce_s) → 分方向派生 enc/mac 键
记录          type(1) | len(4) | 密文 | tag(32)
密钥流        HMAC(k_enc, seq_be(8) | block_be(4)) 串接，逐块 XOR
tag           HMAC(k_mac, seq_be(8) | type | len | 密文)
```

三个设计选择，都是为了让失败**可解释**而不只是「连不上」：

1. **序号严格递增，不做重同步。** tag 校验失败就把连接报废。篡改与重放都会落在「某个固定
   序号上 tag 不对」，不需要单独判别——也**不该**给攻击者一个「发错就回退重来」的接口。
   代价是：链路中间丢一段（TCP 不会）或实现不一致时，只能重连。
2. **握手用固定长度、固定魔数。** 明文误连会在 4 字节魔数处立刻失败（或读不满 69 字节超时），
   不会挂在「等一个永远不来的分隔符」上。
3. **心跳放在记录层（`PING`/`PONG`），不放帧层。** 收发两侧的读循环自己消费控制记录，
   业务帧的语义完全不变——`relay_sub` 的订阅连接不需要写一行保活代码，也不会收到假的
   `DELIVERED`。

**这不是 TLS**：没有证书与域名校验、没有前向保密（PSK 泄露即历史流量可解）、没有密钥轮换，
HMAC-SHA256 计数器密钥流也未经独立密码学审计。它能挡的是：无凭证注册、被动窃听、记录篡改、
重放；挡不住持有 PSK 的内部人。

### 17.3 公网三道闸

| 闸 | 参数 | 超限行为 |
| :--- | :--- | :--- |
| 连接数 | `--max-conns 512` | 直接关，计 `conn_cap_rejected` |
| 每连接速率 | `--rate 200/s`、`--burst 400`（令牌桶） | 断开，计 `rate_limited`；按时间补充而非每秒清零，避免窗口边界放过 2 倍突发 |
| 空闲 | `--idle-timeout 300` | 先发 `PING`，连续两轮无应答才断，计 `idle_closed` |
| 注册表 | `--max-nodes 8192` | 拒绝新注册，计 `rejected` |
| 握手 | `--handshake-timeout 10` | 读不满 hello 就断（slowloris），计 `secure_rejected` |

另外一条**启动策略**（在二进制里，不在库里）：绑非回环地址且没配 PSK → 拒绝启动，并打印
三种处置方式。`--allow-insecure` 才放行明文。这条比任何文档都管用：部署时不可能「忘了开」。

### 17.4 实测（两台真实主机，relay-222 公网侧 / relay-209 内网侧）

| 用例 | 结果 |
| :--- | :--- |
| `pair` @ 222（PSK） | **PASS**，ACK 0.6 / 0.7 ms，B→A 0.6 ms、A→B 0.4 ms |
| `latency` 30 轮 @ 222 | **PASS**，30/30，min 0.30 / p50 0.88 / max 1.41 / avg 0.85 ms |
| 跨实例加密投递 30 轮（60 帧） | **PASS**，60/60，min 0.4 / max 0.7 / avg 0.4 ms |
| 两端计数 | 222：delivered 62、forwarded 30、dropped 0、secure_handshakes 6、secure_rejected 5；209：delivered 30、forwarded 30、dropped 0、secure_handshakes 2 |
| 两实例互认 | 各 `peers=1`，`key_fp=037d79762181690e` 一致（指纹是人工核对用的，不是密钥） |

`secure_rejected=5` 不是噪声：它正是下一节两条反证与几次明文探测留下的计数——拒绝被记下来，
才说明拒绝真的发生了。

### 17.5 三条反证（比 PASS 更重要）

| 反证 | 观测 |
| :--- | :--- |
| 错误 PSK 连 222 | 服务端在 hello 处关闭；探针 `ConnectionError`；`secure_rejected += 1` |
| 明文客户端连 222 | 读不满 hello 超时断开；探针 `TimeoutError`；`secure_rejected += 1` |
| 无 PSK 绑 `0.0.0.0:7199` | **拒绝启动**并打印处置方式 |

单元层面另有 8 条（`cr-nat::secure`）：握手往返、线上不可读明文、错误密钥、篡改密文、重放
记录、明文误连、PING→PONG、密钥指纹稳定性；中继层面 4 条：明文被拒、双端认证投递、超速断连、
连接数上限。

### 17.6 公网入口没通 —— 但**不是**中继的问题

目标入口：`112.5.32.34:57100 → 192.168.34.222:7100`。实测不通，证据链：

```
A → 112.5.32.34:54022   建连成功 27 ms      # 同一台 CPE、同一台内网主机，只是端口不同
A → 112.5.32.34:57100   6 s 超时
221 → 112.5.32.34:57100 超时                # 换一条出口（112.5.60.4）也一样
222: tcpdump -i ens160 'tcp port 7100 or tcp port 57100'  → 0 packets
222: tcpdump -i any 'host 112.5.60.4'（A 正在连）           → 0 packets   # 排除「目的端口被改写」
222: ufw inactive；iptables -S INPUT → -P INPUT ACCEPT；ss → 0.0.0.0:7100 LISTEN
```

也就是说：包根本没到 222。**中继监听、防火墙、PSK 全部无关**，是这条映射自身没有投递。
`54022` 通而 `57100` 不通，唯一差别就是两条规则的内容——按 §16.6 的教训，这次没有再按端口
抓包，而是**按对端公网 IP** 抓，结论才站得住。

修好之后可以直接复跑，不需要改代码：

```bash
python tests/spikes/s6/nat_lab/relay_probe.py --relay 112.5.32.34:57100 --mode latency \
  --repeat 30 --key file:/path/to/psk.txt
python tests/spikes/s6/nat_lab/relay_probe.py --relay 112.5.32.34:57100 \
  --relay 192.168.1.209:7100 --mode cross --repeat 30 --key file:/path/to/psk.txt
```

因此本轮**没有任何跨公网时延数字**：LAN 里的 0.85 ms 说明不了公网可用性，宁可不写。

> **2026-11-16 复测（用户要求）**：`57100` 仍不通（A 连 `112.5.32.34:57100` 5–6 s 超时，重复两次；
> 同期 `54022` = 42 ms、16 ms 可用）。按对端公网 IP 抓包（`tcpdump -i ens160 'host 112.5.60.4'`）
> 抓到 `112.5.60.4 → 192.168.34.222:22` 与 `:7100` 的 SYN/握手，**57100 对应的包 0 条**，
> 再次确认是映射本身没有投递。新发现：公网 `112.5.32.34:7100` **可用**，
> `relay_probe.py --mode pair --key <PSK>` 实测注册 ACK A=0.1 ms / B=2.3 ms、
> 双向投递 `WAKEUP_REQ`/`WAKEUP_ACK` PASS（密钥指纹 `037d79762181690e`），
> 抓包见 `112.5.60.4 → 192.168.34.222:7100`。
> 即 CPE 当前实际生效的是 `7100→7100` 一类规则，`57100→7100` 仍无投递；
> 具体规则内容需在 CPE 管理页核对（本机无管理权限，不猜）。
> 这条只改变「跨公网中继入口已存在」的事实，**不改变** §17.6 的 57100 结论，也不改变任何 S6 判据。

### 17.7 解决了什么、没解决什么

解决：

- 中继可以被恶意对待而只服务持钥方：无凭证注册、窃听、篡改、重放都被挡在门外；
- 刷连接 / 刷帧 / 半开连接不会把进程拖死（三道闸 + 注册表上限）；
- 订阅连接不需要业务保活，空闲探测在记录层完成；
- 配置写错（`nat.relay_key` 非法）会打字告警，**不会**静默退回明文。

没解决：

- 不是 TLS（见 §17.2 的边界）；没有密钥轮换与多租户隔离——所有持钥方共享一个注册表命名空间；
- 速率与连接数是尽力而为，不抗真正的 DDoS；没有按 IP 的黑名单/限速；
- 跨公网时延、丢包、抖动**尚未测量**（入口未通，见 §17.6）；
- 仍然不产生任何 NAT 类别样本：`S6.status` 仍 `not_run`、`g2_claim=false`、R-04 不判定。

## 18. 出口类别清单：条件不齐时先把样本沉淀下来（W3.10，2026-11-16，v2.4.59）

### 18.1 起因：一个出口只有一个类别，这是原理不是配置

上一轮之后用户提了一个很实在的问题：**能不能用现有两个公网 IP 一次验证一类，不追求同时凑齐？**

「一次验证一类」本来就是这套矩阵的工作方式——`nat_matrix.py` 每次只跑一个类别。
但把它当成「那就分五次做」是**误读**：判据要的不是「同时存在」，而是

> 4 类 NAT + 1 个 CGNAT，**各有** ≥1 个来自真实 NAT 设备的样本。

一个出口（一条宽带、一个手机热点后面）在任一时刻只表现为**一个**真实 NAT 类别。所以：

- **串行做**：第 1 天量 A 出口 = `port-restricted-cone`，第 2 天再量 A 出口 —— 还是
  `port-restricted-cone`。**时间不改变类别**，串行不产生新的一格。
- **串行做（真正有用的一半）**：换**出口**再量。同一套工具、同一个流程，只是把 `--tag egress=`
  换个名字。这才是「一次验证一类」的正确形态：**不是分五次做，是分类五次测不同出口**。
- **配置变体**：部分 CPE 带「全锥 NAT / NAT 加速」开关，同一台真实设备可以配出两种行为。
  这是真实设备的真实行为，可以测，但**必须标注 `variant`**，且**不计入独立出口覆盖**——
  真实用户不会去拨这个开关，把它算成「我们覆盖了这一类」就是拿配置冒充总体。

### 18.2 因此新增：`coldrevive-nat --census / --report`

条件不齐时的正确做法不是伪造覆盖，而是**把每次能做的观测按统一格式存下来**，
等条件齐了直接汇总，不必回头重测。工具：

```bash
# 探测并归档为一条样本（egress 必填，其余 tag 可选）
coldrevive-nat --census tests/spikes/s6/census/egress.jsonl \
               --tag egress=home-a --tag operator=cmcc \
               --tag device=HG6145F --tag variant= --tag notes="..."

# 只看汇总，不探测（避免「想看一眼却顺手多打了几枪」）
coldrevive-nat --report tests/spikes/s6/census/egress.jsonl [--json]
```

记录是单行 JSON（JSONL，字段顺序固定），关键字段：

| 字段 | 含义 | 为什么必须有 |
| :--- | :--- | :--- |
| `nat_class` | 类别 | 清单主键 |
| `mapping_behavior` | `eim` / `apdm` / `unknown` | 与过滤行为正交，缺一个都说不清是哪一类 |
| `filtering_behavior` | `unfiltered` / `address-restricted` / `port-restricted` / `unknown` | 同上 |
| `observations` / `distinct_mappings` | 得出结论的观测条数 | 让「结论来自哪几条报文」可复核，不是凭印象 |
| `double_nat` | CPE 自报 WAN 与 STUN 映射是否一致 | 不一致 ⇒ 中间还有一层（运营商 CGNAT 的典型形态） |
| `variant` | 配置变体标签 | 区分「同一设备不同配置」与「不同出口」 |
| `sample_id` | 出口+时间+类别+映射的哈希前 16 位 | 去重：同一份输出贴两次不该算两个样本 |

`filter_behavior` 与原始 `observations` 这一轮才进 `TraversalReport`——此前它们只用于
当场算 `nat_type`，算完就丢，导致报告能说「是 port-restricted-cone」却说不出依据。
现在「说不出依据就返回 Unknown」这条纪律有了数据结构支撑。

### 18.3 三个反滥用设计（都是纪律写成代码）

1. **一个出口一个类别**。同一 `egress` 在**同一 `variant`** 下出现两个类别 ⇒ 记为
   `unresolved` 冲突，**该出口整条不进清单**。两个类别里哪个才对是未知的，任选一个都是编。
2. **清单 ≠ 覆盖**。`CensusSummary` 里 `matrix_coverage` 恒为 `"none"`、
   `r04_judgeable` 恒为 `false`、`s6_status` 恒为 `"not_run"`、`g2_claim` 恒为 `false`。
   这四个是**常量不是计算结果**——普查在原理上不产生可达性数字，因此不能折算成
   `matrix_coverage`，也不能用于 R-04。看清单的人必须看到这四个字段才能下判断。
3. **读回严格**。`--report` 遇到未知字段直接报错并带行号，不跳过。放宽会让「字段名改了
   却照样读得通」变成静默的数据缺失。

### 18.4 本轮实测（真实网络，一次样本）

A 机（`192.168.1.210`，家宽出口 `112.5.60.4`）：

```json
{"census_version":1,"kind":"class-inventory","sample_id":"5560ced40a259875",
 "measured_at":"2026-11-16T01:24:11.333Z","host":"A","egress":"home-a","operator":"cmcc",
 "device":null,"variant":null,"nat_class":"port-restricted-cone","mapping_behavior":"eim",
 "filtering_behavior":"port-restricted","mapped_address":"112.5.60.4:3008",
 "wan_address":null,"double_nat":null,"cgnat_suspected":false,"observations":2,
 "distinct_mappings":1,"stun_servers":"stun.cloudflare.com:3478,stun.l.google.com:19302"}
```

解读：2 个目的地址映射一致（`eim`，`distinct_mappings=1`）⇒ 锥形；过滤实测为
`port-restricted` ⇒ **`port-restricted-cone`**。与 W3.6/W3.7 在同一出口上的历史结论一致。

汇总输出（同一份文件）：

```
样本数              : 1（其中 unknown 0）
出口数              : 1
判据要求 / 独立覆盖 : 5 类 / 1 类
  port-restricted-cone  : home-a
  缺（无独立出口）    : full-cone, restricted-cone, symmetric, cgnat
警告:
  - 只有 1 个出口样本：任何「跨出口」结论都不成立...
  - 全部样本来自同一运营商（cmcc）：类别分布对「真实家用 NAT 总体」没有代表性。
MATRIX_COVERAGE=none  R04_JUDGEABLE=false  S6_STATUS=not_run  G2_CLAIM=false
```

`double_nat=null` 是因为该 CPE 不提供 UPnP/NAT-PMP 外部地址（探测报 `否`），缺数据不下结论。

### 18.5 现有两个出口各自是哪一类（这是 S6 的第一个真答案）

| 出口 | 公网 IP | 类别 | 证据 |
| :--- | :--- | :--- | :--- |
| 家宽（A 所在） | `112.5.60.4` | `port-restricted-cone` | 本轮 §18.4 实测（eim + port-restricted） |
| 222 所在 | `112.5.32.34` | `symmetric`（APDM） | W3.7 手工报文：同一 socket、同本地端口、三个目的 IP → 三个不同映射端口（§15） |

即**现有两个出口本来就是两类**。这对「还要几个 IP」是关键信息：缺的是
`full-cone` / `restricted-cone` / `cgnat` 三类，而不是「再多几个 IP」——
同一运营商再加一条线，大概率还是 `port-restricted-cone`。

### 18.6 没解决什么

- 清单里**没有一个可达性数字**：`port-restricted-cone` 这一类「能不能打通、30 次通几次」
  仍要 `nat_matrix.py` 在四类环境里跑，本文档给不出。
- 类别判定依赖 STUN 可达：只有一个目的地址应答时一律 `unknown`（宁可说不出，也不编）。
- 手机热点/VPS 这些出口**尚未测**：工具就绪，等环境。

## 19. R-04 改为「尽力」：取消可达率的数值承诺（2026-11-16，v2.4.60，ADR-012）

### 19.1 决定

项目所有者否定了「R-04 ≥70%」这个口径：

> 我们不能承诺具体数字，只能是尽力，我们是提供一个可能性，不是确保实现，也没必要。

即：`R04_MIN_REACHABILITY = 0.70` 删除，`nat_matrix_config.json` 的
`r04_min_reachability: 0.7` 删除，S6 的验收从「百分比够不够」改为**四条定性要求**。
决策记录见 [ADR-012](../../../docs/02-architecture/adr/ADR-012-r04-best-effort.md)。

### 19.2 为什么这不是「放松标准」，而是「撤掉一个不该许的承诺」

| 理由 | 说明 |
| :--- | :--- |
| 产品卖的不是百分比 | 提供的是「多一条能叫醒机器的路」，用户要的是「叫得醒 / 叫不醒时我知道为什么」，不是 SLA |
| 数字会污染测量 | 阈值存在 ⇒「测不到 70%」被读成「不合格」⇒ 只能靠**挑环境**或**填格子**过关。后者正是本项目反复在打的那类缺陷 |
| 分母不可抽样 | 「所有家用 NAT」无法抽样：运营商、光猫型号、月份策略都不同。5 个样本算出的百分比没有统计意义，却会被当 SLA 引用 |

第二条尤其要紧：本项目已经因为「拿假设当测量」错了四次（§18.1 的三连坑 + §17.6 的假阳性）。
一个会奖励「凑数字」的阈值，和这条纪律是直接冲突的。

### 19.3 落地（都是可检查的，不是表态）

| 位置 | 改动 |
| :--- | :--- |
| `cr-nat/src/lib.rs` | 删 `R04_MIN_REACHABILITY`；加 `R04_STANCE="best-effort"` 与 `R04_NUMERIC_COMMITMENT: Option<f64> = None`（**保留字段名，让机器读到「我们明确不给数」而不是以为漏了**）；单测 `r04_has_no_numeric_commitment` 防阈值被好心加回 |
| `nat_matrix_config.json` | 换成 `r04_stance` / `r04_numeric_commitment: null` / `r04_requirement`（文字承诺）/ `s6_pass_criteria`（四条） |
| `nat_matrix.py` | 聚合报告不再做阈值比较，输出 `r04_stance` + `reachability_note`；自检新增 `r04_has_no_numeric_threshold`（17/17 PASS） |
| `RISK.md` R-04 | 触发条件由「公网可达率 <70%」改为「**某类环境中直连与中继回退同时失败**」 |

### 19.4 S6 现在的判据（替代 70%）

1. 5 类环境（4 类 NAT + CGNAT）各有 ≥1 份 `evidence_valid=true` 的逐类报告；
2. 每类 `direct` 与 `relay` 两条路径各 ≥30 次真实观测，成功判据是收到真实 `WAKEUP_ACK`；
3. `direct` 失败的环境里 `relay` 必须送达；两条都失败的记为该类**不可用**，作为已知限制写进报告
   （「这一类只能中继」是合格结论；「没测」不是）；
4. 结论由执行人依证据手工回填，脚本永不置 pass。

**要的是「每一类会怎样」被查清，不是「总体够不够好」。** 少一类就是地图缺一块，标出来即可。

### 19.5 一条强度都没放松

取消的是「达标数字」，不是「必须实测」：

- `S6.status` 仍 `not_run`、`g2_claim=false`、`spike_status_changed=false`；
- 复核条款①（两台不同物理主机）与②（真实 NAT 设备）**一条没动**，`evidence_scope` 分层不变；
- 反向约束更强了一条：以前「direct 0/30」要靠「没到 70%」来解释，现在必须**逐类说明为什么**
  （如 §15 的 symmetric：网关无 DNAT、映射随目的变，只能中继）。

### 19.6 已观察到的可达率怎么记

`aggregate_reachability` 字段保留，但**只能与样本量、观测环境一起读**，报告里跟着一句
`reachability_note`。已观测到的（同机/跨主机实验室、真实 NAT 单类）：

| 类别 | direct | relay | 环境 |
| :--- | :--- | :--- | :--- |
| 完全锥 / 受限锥 / 端口受限锥 | 30/30 | 30/30 | 同机 netns + 跨主机实验室 |
| 双 NAT CGNAT（合成） | 30/30 | 30/30 | 同上 |
| symmetric（真实设备 222） | 0/30 | 30/30 | W3.7 真实 NAT |
| 端口受限锥（真实家宽 A） | 未测 | 未测 | W3.10 仅分类 |

**这张表不是「可达率」，是「每类已知的行为」** —— 这正是取消阈值后想要的形态。

## 20. 借 eMule 的 LowID：把「可达性类别」变成节点自报的声明（W3.12，2026-11-16，v2.4.61）

### 20.1 起因：一个必须正面回答的质疑

R-04 取消数值承诺（§19）之后，所有者问：**eMule 网络同样存在这几类 NAT，而且更多是
LowID，为什么不直接借鉴 eMule 的做法？**

回答分两半。借的那一半是它的核心思想——**失败应当被预先知道，而不是试出来**：
每个客户端自己知道自己是 HighID 还是 LowID，登录时声明，别的节点开连之前就知道结果。
不借的那一半是它的三个语义：LowID 即终点（eMule 可以换一个源，我们不能——要叫醒的就是
那一台机器）、没有中继（eD2k 服务器中继不了整个文件，我们中继的是几百字节的唤醒信令，
代价低六个数量级）、HighID = 谁都能连（我们的 `direct` 是「双方配合打洞的前提下可直连」，
端口受限锥型不在此列，W3.6 已逐类实测）。

完整决策记录见 [ADR-013](../../../docs/02-architecture/adr/ADR-013-emule-lowid-declaration.md)。

### 20.2 落地：`ReachDeclaration` + 四类纯追加帧

| 帧类型 | 编码 | 方向 | 内容 |
| :--- | :--- | :--- | :--- |
| `REGISTER_DECLARED` | 0x08 | 客户端 → 中继 | 16B node id + 1B 类别 + 8B 声明时刻 |
| `PEER_REACH` | 0x09 | 实例 ↔ 实例 | n × 25B 声明记录，整份覆盖（与 `PEER_SYNC` 同语义） |
| `LOOKUP_REACH` | 0x0A | 客户端 → 中继 | 16B 目标 id |
| `REACH_RESULT` | 0x0B | 中继 → 客户端 | 16B 目标 id + 1B 类别 + 8B 时刻（查不到时后两段为 0） |

类别推导（`cr-nat/src/reach.rs`，10 条单测）：

| NAT 类型 | `ReachClass` | 依据 |
| :--- | :--- | :--- |
| `symmetric` | `relay-only` | 映射随目的变，对端无从得知该打哪个端口（W3.7 抓包） |
| 三种锥形 | `direct` | 映射与目的无关，配合打洞可直连（W3.6 实验室 30/30） |
| `cgnat` | **`unknown`** | 运营商侧那一层行为未知，不能按名字推断 |
| `unknown` | `unknown` | 样本不足，说不出来就不说 |

纪律条目（都写进代码，不是文档表态）：

- `allows_skipping_direct()` 只对 `relay-only` 为 true——**unknown 永远不许当 lowid 用**；
- 声明带 `ttl`（默认 1h），过期一律降级 `unknown`，时钟回拨也算过期；
- 声明帧的负载 id 必须与帧头一致（防止替别人声明）；客户端发 `PEER_REACH` 直接拒
  （否则可以伪造任意节点的类别）——两条负向对照各有专门单测。

### 20.3 中继侧：存储与转发，不判断

中继新增 `declared` 表与四个计数：`declared_updates` / `declared_direct` /
`declared_relay_only` / `declared_unknown` / `reach_lookups`。gauge 名字里的
`declared_` 前缀是刻意的——它们是**客户端自报的声明**，不是本实例观测的事实，
读成「有 N 台机器可直连」就是误读。

### 20.4 持有者侧：注册即声明，探测更新即重声明

`relay_sub` 注册改用 `register_declared`：没探测过就如实声明 `unknown`（中继记为
「未声明」而不是替我们猜）；`nat probe` 一旦得出结论，声明槽位更新，订阅线程在 500ms
轮询里通过**同一条连接**重声明，不必断开重连。

实现中踩到一个值得记录的坑：`unknown` 兜底声明的时刻戳如果每次现取，订阅循环会把
「没有更新」误判成「有更新」，每 500ms 重声明一次直到永远。修复是把兜底**盖章一次**
并缓存，重声明只在声明真的变化时发生。

### 20.5 部署顺序

四类帧是纯追加，旧版中继收到只会计数拒帧。因此**先升中继、再升客户端**：
反过来新客户端注册阶段就会被旧中继断开。现网 222 / 209 按此顺序滚动。

### 20.6 边界（本轮不宣称的）

- `coldrevive-nat --lookup` 只是**诊断消费**（查一句已存在的声明，不发探测包）；
- **唤醒路径尚未接入「先查再打洞」**——那会改变信令流，须单独一轮实现并实测；
  在那之前，本节的全部机制只是「能力就绪」，不宣称节省了任何一次超时；
- 声明的正确性仍然取决于探测本身：`unknown` 的来源（没测 / 样本不足 / CGNAT）
  在声明里只有一个类别，区分不开——需要区分时看 `nat probe` 的完整报告。

## 21. 中继带宽护栏与路线重排（W3.13，2026-11-16，v2.4.62）

### 21.1 为什么先做这个

所有者提出数据中继必须设阈值。中继同时是**信令前置依赖**和**数据兜底通道**：
W3.9 只有每连接帧速率闸（200 帧/s、突发 400、单帧最大 1 MiB），
按上限算足以占满一台小 VPS 的上行，并连累所有唤醒信令。
所以数据中继上线**之前**先有字节预算，而不是出事后再补。

### 21.2 实际实现

`Limits` 新增 `data_byte_rate`（默认 512 KiB/s）与 `data_burst_bytes`（默认 2 MiB），
CLI 为 `--data-rate` / `--data-burst`。全局令牌桶、整帧申请：

- 负载 ≤ 4096 B 的帧视为**唤醒信令**，绕过字节预算，只受每连接帧速率约束；
- 超过阈值的帧按负载字节数计费，不足则**整帧拒绝**，不半途截流；
- 同侪 `FORWARD` 在接收实例再计一次费——每个实例花自己的带宽；
- 新增 `data_bytes_admitted` / `bandwidth_limited` / `bandwidth_limited_bytes`，
  与 `dropped`（目标不存在）分开，`RELAY_LIMITS` 启动行与 stats JSON 均可见。

验证是本地的：`cargo test --workspace --offline` **145 passed / 0 failed**
（新增 2 条：预算耗尽时信令仍送达、数据帧整帧拒绝与按时间补充）；
真实进程启动回显 `data_rate:1234/s,data_burst:4096`，stats JSON 三个新字段解析通过。

**这条护栏不产生任何可达率数字**，不进 S6 判据；它是部署侧资源上限，
与 ADR-012「工程指标可设线、外部环境成功率不设数值目标」一致。

### 21.3 出口类别清单为什么从当前路径拿掉

`--census` 只为回答「能否宣称 G2」提供逐类证据：S6 要求每类在两台不同物理主机上、
由真实 NAT 设备产生证据。既然本轮已明确**不宣称 G2**，补手机热点 / 云 VPS 类别
就没有当前交付价值，移入 S6 证据阶段；等真的要判定 G2 时再排期。
按所有者决策，当前顺序改为**标准协议互通（S1/S2 → G3 矩阵）→ 补种唤醒路径**。
清单工具与其纪律（同出口同 variant 多类 ⇒ unresolved、常量口径、未知字段报错）
保持不变，只是不再占本轮工时。

## 22. 第三方客户端互操作：G3 卡点解除但矩阵未全绿（W3.15，2026-11-16，v2.4.64）

本节与 NAT 分类判据无关，只记录「**我方标准协议产物能否被旧客户端零改造使用**」这一
标准协议互通前提；不产生任何可达率数字，不进 S6 判据。

### 22.1 卡点与解除

W3.14 的 G3 基线核验结论是「8 行中 7 行 `blocked_no_client`」——本机只有 Motrix。
所有者提供三台可 SSH 的 LAN 主机，装有不同的发行版原样客户端：

| 主机 | 客户端 | 驱动方式 |
| :-- | :-- | :-- |
| 192.168.1.221 | Transmission 3.00 (bb6b5a062e) | `transmission-remote` RPC |
| 192.168.1.222 | qBittorrent v4.5.1 (nox) | 种子文件作位置参数，无 GUI |
| 192.168.1.209 | aria2-next（Motrix Next 捆绑） | `--bt-tracker` 指向标准 tracker |

### 22.2 做法与证据

A（192.168.1.210）上跑 S1 `coldrevive-s1-probe cross-seed`，生成标准单文件 `.torrent`
（info 字典字节的 SHA-1 即 InfoHash；1 MiB、64 片、`info_hash=dbeabf89…`），DHT/LSD 关闭，
只经 221 上新增的 `tests/e2e/g3/local_tracker.py`（标准 BEP23 compact peer list，仅 RFC1918，
以连接来源 IP 为准）做 peer 发现。三台机器**不打任何补丁**，只拿到 `.torrent` 与 tracker URL。

三轮结果（每轮独立清 tracker peer 状态）：

| 行 | 客户端 | 结果 | 我方 seeder 侧 |
| :-- | :-- | :-- | :-- |
| Transmission | 221 | 1 MiB 全量，`sha1sum=f0f990bb…` | `uploaded_bytes=1048576` |
| qBittorrent | 222 | 1 MiB 全量，SHA-1 一致，tracker 仅返回 1 个 peer | `uploaded_bytes=1048576` |
| aria2-next | 209 | tracker `event=completed downloaded=1048576` | `uploaded_bytes=1048576` |

「通了」的判据不是客户端自报，而是**两方独立计数器交叉印证**：远端 `sha1sum` / tracker
`completed` 字节数 + 我方 rqbit `stats().uploaded_bytes >= total`。qBittorrent 首轮
`peer_count_returned=4`（残留 Transmission / 旧 qBittorrent peer）时 seeder `uploaded=0`，
说明下载来自别的 peer 而非我方——该轮**判为不干净证据并废弃**，清场重跑确认 tracker 只返回
seeder 一个 peer 后才采信。这正是「拿假设当测量」那类坑的又一次现场复现。

`local_tracker.py` 本身也踩过一个真 bug：`parse_qs` 把原始二进制 `info_hash` 当 str 解码，
对 `>=128` 的字节抛 `UnicodeEncodeError`；改为字节层 `unquote_to_bytes` 后，rqbit 的
`%XX` 全字节编码可被正确还原。`--self-test 3/3` 覆盖 peer 列表、`stopped` 清理与二进制解析。

### 22.3 纪律边界

证据落在 gitignored 附件 `tests/e2e/g3/reports/interop-bt-2026-11-16.json`，
`g3_status` / 每行 `g3_verdict` 恒 `not_run`。核验器登记远端客户端后基线为 3 行 runnable /
5 行 blocked，但**不自动升级判据**：G3 仍差 eMule（GUI，未无头驱动）/ BitComet / AriaNg /
浏览器插件 / 关闭 UI 五行，S3/S5/S6 保持 `not_run`，`g2_claim=false`。

这三行能证明的只有一件事：**我方标准 `.torrent` + 标准 BT tracker 可被零改造第三方客户端
下载**。它不证明 eMule 互通（那需要 ed2k 链接与 KAD 共存取证），也不证明 NAT 可达性。

## 23. 判据改为 eMule 式可达语义（2026-11-17，v2.4.76，ADR-016）

### 23.1 所有者决定

> 「改判据，目前只要达到 eMule 类似的可达就可以了」（2026-11-17）

决策记录：[ADR-016](../../../docs/02-architecture/adr/ADR-016-s6-emule-like-reach.md)。
性质是**阶段性（interim）判据，带复审时点**，不是「G2 达标」。

此前 §13 论证过一条硬限制：**一个出口在任一时刻只对应一个真实 NAT 类别**，
凑齐 4 类 + CGNAT 需要 ≥4 个不同真实出口；§18.1 又否证了「串行重测 / 配置变体 /
合成 NAT 可以凑格子」。于是 G2 被卡在**环境采购**上，而不是工程能力上。
ADR-012 已取消可达率数值承诺、ADR-013/W5.2 已把 eMule 的 LowID 自我声明落地
（真实环境 21/21 连续 3 轮），「按类别抽样出数字」这条路本就失去了意义。

### 23.2 现行判据（替代 §19.4 第 1 条）

| # | 判据 | 是否变化 |
| :-- | :-- | :-- |
| 1 | 真实 NAT 设备上 ≥1 份 `evidence_valid=true` 的 `real-multi-nat` 报告；条款①两台不同物理主机、条款②真实设备 | **不变** |
| 2 | 每条路径 `direct` / `relay` 各 ≥30 次真实唤醒，只认真实 `WAKEUP_ACK(status=OK)` | **不变** |
| 3 | 至少一份报告 `direct` 可达（HighID），至少一份 `direct` 失败但 `relay` 送达（LowID）；两条都失败记为已知限制 | **改写** |
| 4 | 节点自报可达性与实测一致：只有 `relay-only` 可跳过直连，`unknown` 不得当 LowID（ADR-013 / W5.2） | **新增** |
| 5 | NAT 类别覆盖**不再是 pass 前提**；`--census` 清单为描述性材料，未覆盖类别写入已知限制 | **改写** |
| 6 | 不承诺数值、人工回填、脚本永不置 pass；`s6_status=not_run` / `g2_claim=false` / `evidence_valid_for_pass=false` | **不变** |

**声明口径**：满足后 G2 只能写「**按 eMule 语义结案，带已知限制**」，
不得写「NAT 矩阵通过」或「G2 达标」。少测的类别是地图上标出来的空白，不是被划掉的格子。

### 23.3 落地位置（都是可检查的）

| 位置 | 改动 |
| :-- | :-- |
| [ADR-016](../../../docs/02-architecture/adr/ADR-016-s6-emule-like-reach.md) | 决策正文、取代范围、复审时点；按 ADR 索引 §4 **不改写 ADR-012 / ADR-013 正文** |
| `nat_matrix_config.json` | `s6_reach_stance=emule-like-reach`、`class_coverage_required_for_pass=false`、重写 `s6_pass_criteria`、`required_classes` 标 `role=descriptive-inventory`、`additional_environment.required=false` |
| `nat_matrix.py` | `--aggregate` 改按 HighID / LowID 两半汇总（`reach_evidence_state()`）；类别清单仍输出但 `matrix_complete_is_pass_condition=false`；`--self-test` **24/24** |
| `cr-nat/src/lib.rs` | `S6_REACH_STANCE` / `S6_CLASS_COVERAGE_REQUIRED_FOR_PASS=false` / `S6_CLAIM_WORDING`，配套单测 |
| `cr-nat/src/census.rs` + `coldrevive-nat` | 汇总 JSON 与 CLI 输出显式带上立场与「不是 pass 前提」，不再把清单长度写成判据要求 |
| `spikes.json` | `S6.criteria` 改写、`g0_review.s6_criteria_amendment_2026_11_17` 留痕；**`S6.status` 仍 `not_run`** |

`cargo test --workspace --offline` **168 passed / 0 failed**（原 166 + 2 条新单测）。

### 23.4 用现有环境复核：两半都已补齐（2026-11-17）

对 `tests/spikes/s6/reports` 跑 `--aggregate`（新判据）：

```
status=OBSERVED   evidence_valid=true   s6_status=not_run   g2_claim=false
reach_evidence.usable_reports = 2
  highid_reports = ['s6-matrix-class-port-restricted-cone-real.json']
  lowid_reports  = ['s6-matrix-class-symmetric-real.json']
  complete = true
matrix_coverage = 2 / 4 类真实 NAT（含 CGNAT 环境 0/1） —— 描述性，非 pass 前提
```

补跑拓扑：A（`192.168.1.210` → `112.5.60.4`，EIM / port-restricted cone）作持有者，
222（`192.168.34.222` → `112.5.32.34`，symmetric / APDM）作请求者，中继 221:7100 只做信令。

- 先跑 `--smoke` 6 次：relay 路径成功、direct 0/6；tcpdump 只见请求者出站包，没有持有者回打包。
  当时记为「symmetric 请求者 back 映射错配」的诊断（`reports/highid-run/s6-highid-diagnosis.json`），
  **不是最终 30 次判据证据**。
- 再把 harness 的非 smoke 分支补齐，跑满每路径 30 次真实 `WAKEUP_ACK`
  （`reports/highid-run-30/s6-matrix-class-port-restricted-cone-real.json`）：
  - direct（`stun-mapped`）：**3/30 成功**，成功样本 `path=stun-mapped`、`ack_status=OK`，
    打到持有者公网地址 `112.5.60.4:47488`；
  - relay：**30/30 成功**，`path=relay`、`ack_status=OK`；
  - 载体错误 0；持有者 STUN 映射端口 47488 ≠ 内部 17801，满足真实设备可证伪条款；
  - 唤醒前 `nat peer` / `nat plan` 已取到持有者声明候选（`private-lan` / `stun-mapped` / `relay`），
    与 ADR-013「只有 relay-only 可跳过直连」一致。

因此按 ADR-016 的行为判据，**HighID 一半与 LowID 一半都有真实 `real-multi-nat` 证据**。
但 direct 只在 3/30 次成功，说明该组合仍高度依赖 NAT 映射时序；这不是数值承诺，
也不把 3/30 写成「稳定可达」。未覆盖的 `full-cone` / `restricted-cone` / `cgnat`
仍是已知限制，`S6.status` / `g2_claim` / `evidence_valid_for_pass` 三个常量不因补跑而改变。

### 23.5 判定：`pass` / `closed_with_exceptions`（2026-11-17）

所有者依上节证据判定 **`S6.status = pass`**，结案口径固定为
「**按 eMule 语义结案，带已知限制**」，不得写成「NAT 矩阵通过」或「G2 达标」。

| 项 | 值 |
| :--- | :--- |
| 判据 | ADR-016 §决策 1–6（两路各 ≥30 次真实 `WAKEUP_ACK`、HighID + LowID 两半、声明与实测一致） |
| 证据 | `reports/s6-matrix-aggregate-20261117-highid.json`（`OBSERVED` / `complete=true` / `rejected=[]`） |
| 机器常量 | `census s6_status=not_run`、`g2_claim=false`、`evidence_valid_for_pass=false` **不变** |
| 已知限制 | `full-cone` / `restricted-cone` / `cgnat` 未测；direct 3/30 只是描述性观测 |
| 记录 | [G2-SPIKE-VERDICT-2026-11-17](../../../docs/06-plan/G2-SPIKE-VERDICT-2026-11-17.md)、`tests/fixtures/g2-spike-verdicts.toml` |

为什么机器常量仍写 `not_run`：那是**工具**字段，含义是「核验器没有替人下结论」；
人工结论单独记在判定记录里。这与 G3 的 `g3_status` vs `g3_status_declared` 是同一套办法。

### 23.6 没有因为改判据而发生的事

- 真实设备 / 两台物理主机 / 30 次真实 `WAKEUP_ACK` / 不编数字 / 不自动 pass，一条没松；
- 未覆盖的 `full-cone` / `restricted-cone` / `cgnat` 仍是**已知限制**，不是「测过了」；
- `--census` 依旧 `matrix_coverage=none`、`r04_judgeable=false`（§18.3 常量），
  清单不折算成覆盖、不用于 R-04；
- §13.2 的三条路线与 §19 的历史论证保留原文，但**不再作为当前推进依据**。

### 23.7 复审时点

**M4 前**，或**当 ≥4 个不同真实出口可用时**（先到为准）重新评审是否恢复完整类别矩阵。
若届时维持本判据，须在 ADR-016 追加复审记录并更新 `s6_stance_review_trigger`，
不得默示延期。

