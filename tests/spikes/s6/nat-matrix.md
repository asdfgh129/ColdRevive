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
