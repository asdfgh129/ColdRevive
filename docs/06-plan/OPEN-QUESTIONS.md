---
doc_id: DOC-15
title: 待决问题清单（OPEN-QUESTIONS）
category: 计划
version: 2.1
status: 持续维护
owner: 架构组 / 项目组
updated: 2026-11-11
---

# 待决问题清单（OPEN-QUESTIONS）

> 编号 DOC-15 · 版本 2.1 · 状态 **持续维护** · 责任人 架构组 / 项目组 · 更新 2026-11-11
> 本表记录**尚未有结论**的问题。一经定稿，须产出/更新对应 [ADR](../02-architecture/adr/README.md) 或文档，并从本表移出（在 [CHANGELOG](../../CHANGELOG.md) 留痕）。

## 1. 开放问题

| # | 问题 | 影响 | 关联 | 建议决策时点 |
| :--- | :--- | :--- | :--- | :--- |
| **Q1** | 公共 KAD 网络对 `ColdRevive_V1:` **按运营速率**发布是否视为污染？<br>（接纳性已解决：2026-11-10 S2 `pass`，`PUBLISH_RES` + 跨进程取回） | 决定 [ADR-001](../02-architecture/adr/ADR-001-kad-network.md) 默认模式与「生态借力」是否成立 | S2 / R-02 | M4 前 |
| **Q2** | 中文分词方案与关键词 Key 数量的性价比拐点在哪？ | 搜索召回率与 KAD 负载的权衡（默认 8 个 Key 未经验证） | R-13 | M0 压测 |
| **Q3** | 伴随种子的分片大小策略（固定 vs 自适应）与跨客户端兼容取舍 | 造种开销、Swarm 合并可能性 | S7 | M1 |
| **Q4** | 私有唤醒 UDP 在对称 NAT / CGNAT 下的实际成功率 | 决定是否加大中继投入 | S6 / R-04 / [G0-REVIEW §5](G0-REVIEW.md) | M2 |
| **Q5** | 是否向公共 Mainline DHT 发布影子索引（可能放大「虚假在线」观感） | 兼容性与生态礼貌的边界 | [ADR-007](../02-architecture/adr/ADR-007-tracker.md) | M2 |
| **Q6** | 激励机制是否必要、如何抗女巫 | 长期节点留存 | [ADR-005](../02-architecture/adr/ADR-005-incentive.md) | M4 |
| **Q7** | 跨协议补源（MD4 9.28 MB ↔ BT 动态分片）**哈希转译**投入产出比<br>（范围已由 [ADR-011](../02-architecture/adr/ADR-011-ed2k-interop.md) 收窄；eD2k 传输栈已排 v1） | v2 是否排期 | [ADR-004](../02-architecture/adr/ADR-004-ed2k-scope.md) / R-10 | M5 前 |
| **Q8** | 各司法辖区对「索引发布」的合规边界（所有者 D4：不作为 L1/L2 前置门禁，M4 前补书面意见） | 默认发布策略与免责机制 | S8 / R-11 / [ADR-011](../02-architecture/adr/ADR-011-ed2k-interop.md) / [OPS §6](../07-operations/OPS.md) | M4 |
| **Q10** | `WAKEUP_REQ.capabilities.ed2k` 的语义：ADR-011 已把**标准 eD2k 公共数据面**排入 v1，而 [ADR-004](../02-architecture/adr/ADR-004-ed2k-scope.md) 规定该字段在 v1 固定 `false`、T0 golden 亦断言 `false`。二者字面冲突，须明确该字段只描述**私有唤醒面是否启用 eD2k**，还是改为反映全局能力 | 改 [PROTO §3](../04-protocol/PROTO.md) 字段须递增 `PROTOCOL_VERSION` 并重做 T0 golden；不改则须在 §3 写明语义边界 | [PROTO §2.2.4 / §3.2](../04-protocol/PROTO.md) · [ADR-011](../02-architecture/adr/ADR-011-ed2k-interop.md) · [ADR-004](../02-architecture/adr/ADR-004-ed2k-scope.md) / R-12 | M1 定稿 |

> **Q9 已结案并移出本表（2026-11-11，v2.4.31）**：`ed2k-rs` / `ed2k` 可复用边界（V-04）已核实 ——
> crates.io 无 `ed2k-rs` crate；GitHub `runfalk/ed2k-rs`（最后 push 2023-08-28）与 crates.io
> `ed2k 1.0.1`（最后更新 2023-05-19）**都只做哈希，不提供传输栈**，且后者默认 Blue 变体与
> eMule 的 Red 分块边界规则不兼容。结论：v1 保留仓内可审计自实现，不引入停更依赖；
> 最小栈「自研 vs 复用」已有答案（自研）。详见 [REFERENCES V-04](../08-reference/REFERENCES.md)、
> [PLAN §23.1](../../tests/spikes/s2/PLAN.md) 与 [CHANGELOG 2.4.31](../../CHANGELOG.md)。
> 原 Q9 中「credit 缺失的实际代价」未随本轮结案，仍由 **R-15** 在 M1 实测。

## 2. 与待定 ADR 的对应

| 待定 ADR | 依赖问题 | 定稿门禁 |
| :--- | :--- | :--- |
| ADR-001 KAD 网络 | Q1 | M0 / S2 |
| ADR-002 BT 引擎 | （S1，非本表问题） | M0 / S1 |
| ADR-005 激励体系 | Q6 | M4 |

> 若发现 [归档基线](../09-archive/README.md) 与现行文档冲突，一律以现行文档为准，并在本表登记差异。

## 3. 立即下一步（M0 Spike 清单）

> **⚠️ v2.4.14 更正：本节下方 v2.4.9–v2.4.13 的环境归因与「换网重跑」结论已被否证并撤回。**
> 根因是 KAD 联系人 IP 字节序读反（线上为点分十进制反序），所有 0/N 都发往错误地址；
> 修正后本机 10 目标探针 **9/10 收到真实 `BOOTSTRAP_RES`**，P4 两次重跑已连上公共 KAD。
> **高位 UDP 一直是通的，不需要换网、不需要开防火墙。** 当前下一步是修 `kad-mini live`
> 的 `SEARCHTOLERANCE` 收敛逻辑（每跳 `ALPHA_QUERY=5` 个候选、`KADEMLIA_FIND_NODE=0x0B`、
> 提高跳数），改用实时 `BOOTSTRAP_RES` 目标清单，再按 G0 参数重跑 P4。
> `S2.status` 已于 2026-11-10 判 `pass`（接纳性）；本条只保留归因记录，风险侧见 [RISK R-02](../05-quality/RISK.md)。详见 [PLAN.md §19](../../tests/spikes/s2/PLAN.md)。

1. 落地 [TECH-STACK §2](../02-architecture/TECH-STACK.md) 的 **S1–S7** 七项验证，每项输出「通过 / 不通过 / 替代方案」结论（S8 已于 2026-11-09 由项目所有者豁免登记为 `pass`，无书面法务意见）。其中 S2 的 [PLAN.md](../../tests/spikes/s2/PLAN.md) P0–P3 已完成（协议核实、真实 wire 回环、LAN 互操作、G0 确认 `sample_size=3` / `rate_limit_pps=0.2`、用户 bootstrap 清单），P4 已于 2026-11-10 数度执行；早期 0 响应系 IP 字节序缺陷（v2.4.14 更正），修正后已真实连上公共 KAD，但 3/3 样本因未收敛到 `SEARCHTOLERANCE` 邻域而跳过发布，**收敛逻辑与发布目标选择均已修（v2.4.18 / v2.4.19：plateau 改按路由表增长以恢复 `MAX_FIND_HOPS=10`；发布目标限定为「`0x29` 已验证且真实 KadID 落在责任区内」的节点），且在 G0 参数下完成了第三轮有效执行（v2.4.21：`verified` 8–12 → 27–30，`closest_top_chunk` 逼近 2^24）**。第三轮暴露的是**索引操作层门禁**：`0x21`→`0x29` 应答 86/99，而 `0x33`/`0x43`/探针 **0/19 全静默**（Fisher p = 3.7e-14），扩大样本只会重复同一结果。下一步须在 H1（先 `HELLO` 互认）/ H2（`0x33`/`0x43` 载荷与 aMule 解析器不一致）之间取证，而两者都要触碰 G0 确认的 `protocol_scope`（现为「跳过 HELLO」），**需项目所有者先书面放宽 scope**；KAD 报文映射与 128/160-bit ID 错配已于 v2.4.6 回写 ADR-001 / PROTO §2.0 / `S2.criteria` / RISK R-02；v2.4.7 又补齐了 `live` 与判据的一致性（`0xE5` 解包、tolerance 收敛、跳过 HELLO、公网 bootstrap 校验），详见 [PLAN.md §6/§7](../../tests/spikes/s2/PLAN.md)。重跑前置检查：[udp_egress_check.py](../../tests/spikes/s2/udp_egress_check.py)（v2.4.9 升级，退出码 0）。
跨主机 STUN 对照（v2.4.10）：LAN 那台 eMule 与本机同一网络、仅多一条入站映射；同一份脚本
在它上面也 `BLOCKED` / 退出码 2（低位 4/4 OK、高位 0/3 OK），**按主机的 STUN 出站例外被否证**。
**v2.4.11 重新开放**：该机截图显示 eD2K HighID 与 Kad 页签 432 个联系人，而 STUN 不等于 KAD；
本机已用 KAD 专用探针确证不可达，但该机 KAD 是否真正存活、出口是否按应用 / 按 IP+源端口放行
仍未判定。须补 `Kad 网络` 面板状态，并在该机运行
[`kad_egress_probe.py`](../../tests/spikes/s2/kad_egress_probe.py)。详见
[PLAN.md §8.6](../../tests/spikes/s2/PLAN.md)。
**v2.4.12 已答一半**：`Kad 网络` 面板 `状态` / `UDP 状态` 均为「打开」、无 Buddy 行 →
firewalled / 陈旧路由表假设被否证；但 aMule 源码核实表明该「打开」是**入站可达性**测试
（远端向本机公网 IP:KAD 端口发无载荷 UDP 包，由入站 DNAT 满足，协调走 TCP），
`m_firewalledUDP` 初值还是 `false`，故不推出该机具备公网 KAD 出站能力，也不与 STUN 超时
矛盾。剩余决定性核实：在该机运行 [`kad_egress_probe.py`](../../tests/spikes/s2/kad_egress_probe.py)
（退出码 `0` = 充分证据；`2` 在按应用放行时不是充分否证）+ 查看 `统计信息` KAD 收发包计数是否
增长。详见 [PLAN.md §9](../../tests/spikes/s2/PLAN.md)。
**v2.4.13 探针已在该机执行**：0/3 / 退出码 2（见 [PLAN.md §10](../../tests/spikes/s2/PLAN.md)）；
但两次 0/3 共用同一批目标，不是独立复现。新增
[`bootstrap_res_to_targets.py`](../../tests/spikes/s2/bootstrap_res_to_targets.py) 取新鲜
`BOOTSTRAP_RES` 的 10 个公网联系人（重叠 0/10），本机宽扫 0/10。所有者补充「该机 eMule
下载/搜索正常」，但按 eMule 官方帮助，下载走 TCP 且与网络选择无关、默认搜索可为 eD2k
服务器 TCP 搜索，只有明确选 `Kademlia` 方法出结果才要求 UDP 通 —— 因此这不构成 KAD
出站证据，**不要开下载验证**，改做 `Kademlia` 方法热门词搜索。工程结论与归因解耦：
本网无独立客户端可跑 P4，**换网重跑**；剩余核实只用于收窄归因，不阻塞。
2. 建立 Cargo Workspace 骨架与 CI（对应 W1.1），冻结 [ARCH](../02-architecture/ARCH.md) 与 [PROTO](../04-protocol/PROTO.md)——**须等 G0 通过**。
3. 确认 [OPS §6](../07-operations/OPS.md) 合规红线；R-11 因 S8 豁免未消除，M4/G4 前复核法务结论。
4. 输出 M1 的双节点 Demo 验收脚本（T0/T1），作为 G1 门禁依据。
5. 回填 [RISK](../05-quality/RISK.md) 中各风险的概率/影响与等级。

> **一句话判断**：技术路径在讨论中已被论证为自洽且可行，剩下的不是「能不能做」，而是 **S1/S2 两个引擎级假设** 与 **R-01 冷启动** 这三件事。M0 的全部意义就是把这三件事从「乐观预期」变成「实测数字」。

## 4. 维护规则

- 每个问题必须有「建议决策时点」；到期未决须在里程碑复审中说明原因。
- 定稿时写明结论、依据数据、影响的文档编号，然后移出本表。
- 新增问题须关联至少一个风险（R-xx）或 ADR，避免悬空。

## 相关文档

- [DOC-05 ADR 索引](../02-architecture/adr/README.md)
- [DOC-13 RISK 风险登记册](../05-quality/RISK.md)
- [DOC-14 ROADMAP 里程碑](../06-plan/ROADMAP.md)

> **2026-11-13 先导观测（Q4 未结案）**：`tests/spikes/s6/stun_probe.py` 在本机真实出口取得
> 首个 NAT 样本——单层 cone 类 NAT、公网 WAN IP、公网 UDP 可达、二跳为公网地址（未见 CGNAT），
> 但 **UPnP IGD 与 NAT-PMP 均不可用**。这只覆盖 1/4 类 NAT，不能回答对称 NAT / CGNAT 的成功率；
> Q4 仍须 M2 多环境实测，且中继投入决策须考虑「穿透栈第一层不可用」这一实测前提。
>
> **2026-11-14 补充（Q4 仍未结案）**：G1 已通过、M2 已开工；`cr-nat` 已接入复活路径
> （候选计划 / 单 socket 打洞 / 中继回退 / tracker 私网-公网地址分流），
> [T2 本地接线验收](../../tests/e2e/t2/README.md) 10/10 PASS。但 T2 用的是本机回环 +
> mock STUN + 中继（W3.3 为 mock 桥，**W3.4 起为真实 `coldrevive-relay` 二进制**），
> `evidence_valid=false`，**不是新的 NAT 样本**：真实中继组件不等于真实 NAT 环境。
> 2026-11-14 的 T1-08 现场 STUN 探针仍是同一出口单样本（cone 类、UPnP/NAT-PMP 不可用）。
> S6 四类 NAT（含 CGNAT）实测与 R-04 ≥70% 阈值判定仍待用户提供多 NAT 环境后执行。
>
> **2026-11-14 W3.5① 补记（Q4 仍未结案）**：[条件探针](../../tests/spikes/probe_conditions.py)
> 确认本机仅 1 类真实 NAT 出口、无 CGNAT、无全局 IPv6、虚拟交换出口不可达、无内核整形能力。
> [精细分类探针](../../tests/spikes/s6/nat_behavior.py) 用带对照的 RFC 3489 Test II/III 把该出口细分为
> **Endpoint-Independent Mapping + Address-and-Port-Dependent Filtering / port-restricted cone**（medium）；
> 关键修复是备用地址直连对照必须使用**独立 socket**，否则会自开 NAT 放行项并产生 full cone 假阳性。
> `coldrevive-nat` 的保守 `full-cone` 标签与该结论不矛盾。**这是分类样本，不是可达率。**
> [四类 NAT 矩阵执行协议](../../tests/spikes/s6/nat_matrix.py) 已交付，本机 `--dry-run` /
> `--aggregate` 均 `BLOCKED`、`matrix_coverage=0/4`；Q4 的对称 NAT / CGNAT 成功率
> 仍须用户提供真实多 NAT 环境并签 attestation 逐类实测后，由实验执行人手工回填。
