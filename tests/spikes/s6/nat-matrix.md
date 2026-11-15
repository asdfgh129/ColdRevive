# S6 spike：常见家用 NAT 下的可达率

> **状态（2026-11-13）：`not_run`（已取得 1 个真实先导样本；G0 条件冻结豁免其 G0 时点结论）。** 判据要求「NAT 类型矩阵**实测**给出可达率」，
> 但「实测」需要多类真实家用 NAT / CGNAT 环境，无法在单机回环上复现。
> 本目录给出**分析型 NAT 矩阵**（基于典型家用 NAT 分布与 ColdRevive 穿透栈能力），
> 并明确 M2 真实多 NAT 实测的验证方案（见 OPEN-QUESTIONS Q4）。`pass` 须 M2 实测后由实验执行人回填。

## 1. 判据与风险

- **判据 S6**：NAT 类型矩阵实测给出可达率。
- **关联 FR-17**：UPnP / NAT-PMP 端口映射、UDP 打洞、LowID 回退；常见家用路由器 NAT 矩阵通过。
- **RISK R-04**：NAT 穿透失败率高，唤醒后连不上（中/高）；触发条件：公网可达率 < 70%。

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

- **直连聚合可达率（估计）**：≈ 80–90%
- **含中继/LowID 回退聚合可达率（估计）**：≈ 95%+

> 该估计**满足 R-04 阈值（公网可达率 ≥70%）**，且对称 NAT 占比越高，中继回退的权重越大。
> 但这是**分析型**数字，不是 S6 判据要求的「实测」。

## 4. 关键不确定性（须 M2 实测）

- **CGNAT 比例**：运营商级 NAT 使对称型占比在部分区域显著上升，直接拉低直连率；
  这正是 OPEN-QUESTIONS **Q4**（私有唤醒 UDP 在对称 NAT / CGNAT 下的实际成功率）的核心。
- **打洞成功率**：依赖对端也支持打洞与 STUN 端点对称性，需真实双端实测。
- **中继成本**：中继/LowID 回退的可达率上限取决于中继节点可用性与带宽，需 M2 部署实测。

## 5. Fallback 链（若 M2 实测 <70%）

按 RISK R-04 缓解预案与 OPEN-QUESTIONS Q4 决策：**提升中继回退优先级** ——
增大中继预算、默认开启 LowID 回退、对对称 NAT 节点标记并优先经中继唤醒，
确保整体可达率拉回 ≥70% 阈值；极端场景（双重 CGNAT）接受「唤醒延迟升高 + 经中继」而非直连。

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
- `--aggregate` 仅在 4 类 + CGNAT 证据齐备且 `evidence_valid=true` 时计算聚合可达率，与
  `R04_MIN_REACHABILITY=0.70` 比较；**脚本仍不判定达标**。
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

> 附带结论：用户截图确认该映射的 `Pool: Test` 内网地址就是 `192.168.34.222`，
> 所以 `54022` 从外面连不通**不是填错地址**，而是这类设备把 DNAT 与放行策略分开
> （DNAT 命中 13 次、包仍未转发到 222，`ens160` 上 `0 packets captured`）。
> 这条路对本档**不是必需**：管理走 LAN，信令走 LAN，数据面走真实 NAT。

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
