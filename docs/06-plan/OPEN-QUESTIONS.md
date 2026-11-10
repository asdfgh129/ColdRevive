---
doc_id: DOC-15
title: 待决问题清单（OPEN-QUESTIONS）
category: 计划
version: 2.0
status: 持续维护
owner: 架构组 / 项目组
updated: 2026-11-09
---

# 待决问题清单（OPEN-QUESTIONS）

> 编号 DOC-15 · 版本 2.0 · 状态 **持续维护** · 责任人 架构组 / 项目组 · 更新 2026-11-09
> 本表记录**尚未有结论**的问题。一经定稿，须产出/更新对应 [ADR](../02-architecture/adr/README.md) 或文档，并从本表移出（在 [CHANGELOG](../../CHANGELOG.md) 留痕）。

## 1. 开放问题

| # | 问题 | 影响 | 关联 | 建议决策时点 |
| :--- | :--- | :--- | :--- | :--- |
| **Q1** | 公共 KAD 网络是否接受 `ColdRevive_V1:` 大量发布？是否会被视为污染？ | 决定 [ADR-001](../02-architecture/adr/ADR-001-kad-network.md) 默认模式与「生态借力」是否成立 | S2 / R-02 | M0 |
| **Q2** | 中文分词方案与关键词 Key 数量的性价比拐点在哪？ | 搜索召回率与 KAD 负载的权衡（默认 8 个 Key 未经验证） | R-13 | M0 压测 |
| **Q3** | 伴随种子的分片大小策略（固定 vs 自适应）与跨客户端兼容取舍 | 造种开销、Swarm 合并可能性 | S7 | M1 |
| **Q4** | 私有唤醒 UDP 在对称 NAT / CGNAT 下的实际成功率 | 决定是否加大中继投入 | S6 / R-04 | M2 |
| **Q5** | 是否向公共 Mainline DHT 发布影子索引（可能放大「虚假在线」观感） | 兼容性与生态礼貌的边界 | [ADR-007](../02-architecture/adr/ADR-007-tracker.md) | M2 |
| **Q6** | 激励机制是否必要、如何抗女巫 | 长期节点留存 | [ADR-005](../02-architecture/adr/ADR-005-incentive.md) | M4 |
| **Q7** | 跨协议补源（MD4 9.28 MB ↔ BT 动态分片）投入产出比 | v2 是否排期 | [ADR-004](../02-architecture/adr/ADR-004-ed2k-scope.md) / R-10 | M5 前 |
| **Q8** | 各司法辖区对「索引发布」的合规边界 | 默认发布策略与免责机制 | S8 / R-11 / [OPS §6](../07-operations/OPS.md) | M0 / M4 |

## 2. 与待定 ADR 的对应

| 待定 ADR | 依赖问题 | 定稿门禁 |
| :--- | :--- | :--- |
| ADR-001 KAD 网络 | Q1 | M0 / S2 |
| ADR-002 BT 引擎 | （S1，非本表问题） | M0 / S1 |
| ADR-005 激励体系 | Q6 | M4 |

> 若发现 [归档基线](../09-archive/README.md) 与现行文档冲突，一律以现行文档为准，并在本表登记差异。

## 3. 立即下一步（M0 Spike 清单）

1. 落地 [TECH-STACK §2](../02-architecture/TECH-STACK.md) 的 **S1–S7** 七项验证，每项输出「通过 / 不通过 / 替代方案」结论（S8 已于 2026-11-09 由项目所有者豁免登记为 `pass`，无书面法务意见）。其中 S2 的 [PLAN.md](../../tests/spikes/s2/PLAN.md) P0–P3 已完成（协议核实、真实 wire 回环、LAN 互操作、G0 确认 `sample_size=3` / `rate_limit_pps=0.2`、用户 bootstrap 清单），P4 已于 2026-11-10 显式执行但被环境公网高位 UDP 过滤阻断（0 响应、0 发布），**只差在无过滤网络上重跑一次有效的 P4**；KAD 报文映射与 128/160-bit ID 错配已于 v2.4.6 回写 ADR-001 / PROTO §2.0 / `S2.criteria` / RISK R-02；v2.4.7 又补齐了 `live` 与判据的一致性（`0xE5` 解包、tolerance 收敛、跳过 HELLO、公网 bootstrap 校验），详见 [PLAN.md §6/§7](../../tests/spikes/s2/PLAN.md)。重跑前置检查：[udp_egress_check.py](../../tests/spikes/s2/udp_egress_check.py)（v2.4.9 升级，退出码 0）。
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
