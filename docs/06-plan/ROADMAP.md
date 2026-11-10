---
doc_id: DOC-14
title: 开发计划与里程碑（ROADMAP）
category: 计划
version: 2.4
status: 持续维护
owner: 项目组
updated: 2026-11-09
---

# 开发计划与里程碑（ROADMAP）

> 编号 DOC-14 · 版本 2.4 · 状态 **持续维护** · 责任人 项目组 · 更新 2026-11-09
> 本文只管「什么时候做、做完怎么验收」。需求见 [SRS](../01-product/SRS.md)，风险见 [RISK](../05-quality/RISK.md)。

## 1. WBS 任务树

```
ColdRevive
├─ W1 基础设施
│  ├─ W1.1 Cargo Workspace / CI / 日志 / 配置 / 指标
│  └─ W1.2 SQLite Schema 与迁移框架
├─ W2 索引与造种
│  ├─ W2.1 共享目录监控与去抖
│  ├─ W2.2 指纹计算（SHA-256 / MD4 / 分片 SHA-1）
│  ├─ W2.3 伴随种子生成与缓存
│  └─ W2.4 影子索引与关键词分词
├─ W3 发现与网络
│  ├─ W3.1 KAD 适配层 + 专属命名空间
│  ├─ W3.2 Mainline DHT 适配
│  ├─ W3.3 NAT 穿透 / HighID-LowID / 中继回退
│  └─ W3.4 发布续期与撤销
├─ W4 唤醒与调度
│  ├─ W4.1 私有唤醒协议与 Challenge-Response
│  ├─ W4.2 限流 / 黑名单 / 信誉
│  ├─ W4.3 LRU 冷热调度器
│  └─ W4.4 握手拦截器
├─ W5 数据面
│  ├─ W5.1 BT 引擎集成（下载/做种/暂停恢复）
│  ├─ W5.2 分片校验与完成闭环
│  └─ W5.3 跨协议补源（P2 / v2）
├─ W6 接入与生态
│  ├─ W6.1 内嵌 HTTP 静态 UI 服务
│  ├─ W6.2 aria2 兼容 JSON-RPC + 鉴权
│  ├─ W6.3 WebSocket 推送
│  ├─ W6.4 Web UI 页面
│  ├─ W6.5 CLI / REST / 浏览器插件 / MCP（P2）
│  └─ W6.6 三进程模型与托盘 Agent
├─ W7 治理与横切
│  ├─ W7.1 QoS / 阈值 / 降级
│  ├─ W7.2 安全加固与隐私
│  └─ W7.3 可观测性
└─ W8 质量与交付
   ├─ W8.1 测试框架与 DHT 仿真器
   ├─ W8.2 兼容性 / 性能 / Soak / 安全测试
   └─ W8.3 打包、文档、发布、社区冷启动
```

> WBS 与 [ARCH §4 模块清单](../02-architecture/ARCH.md)、[src crate 规划](../../src/README.md) 一一对应。

## 2. 里程碑

| 里程碑 | 目标 | 主要交付物 | 退出门禁 | 依赖 |
| :--- | :--- | :--- | :--- | :--- |
| **M0 Spike / 可行性验证** | 把风险最高的技术假设打穿 | S1–S8 验证报告、技术路线冻结、法务初步意见、ADR-001/002/005 定稿 | **G0**：无阻断项 | — |
| **M1 CLI 闭环（MVP）** | 证明「死种能被复活」 | W1、W2 基础、W3.1、W4.1、CLI；双节点 KAD 发布 → 唤醒 → 返回 magnet → 用第三方客户端下载 | **G1**：T0/T1 复活成功率 100% | M0 |
| **M2 引擎集成与资源治理** | 内置引擎、按需做种、阈值达标 | W5.1/5.2、W4.3/4.4、W3.3、W7.1、W6.3 | **G2**：NFR-01/02/03 达标，NAT 矩阵通过 | M1 |
| **M3 兼容与生态** | 旧客户端透明 + 多端接入 | W3.2、W6.1/6.2/6.4/6.6、兼容性矩阵回归 | **G3**：矩阵全绿、NFR-04 达标 | M2 |
| **M4 安全、冷启动与公测** | 抗污染、信誉、邀请制公测 | W4.2、W7.2/7.3、W8.2、安装包、社区运营材料 | **G4**：安全测试通过、公测 KPI、法务通过 | M3 |
| **M5 可选增强（v2）** | 扩展边界 | W5.3 跨协议补源、W6.5 MCP/REST、移动端评估、激励 ADR-005 定稿 | 独立评审 | M4 |

> **MVP 刻意做小**：M1 先用第三方 BT 客户端完成下载，硬编码 magnet，只验证「KAD 发现 → 唤醒 → 复活」这一唯一核心假设。这是讨论中建议的 `coldrevive-cli` 路径。

## 3. 质量门禁

| 门禁 | 位置 | 条件 |
| :--- | :--- | :--- |
| **G0** | M0 结束 | [S1–S8](../02-architecture/TECH-STACK.md) 全部有结论，关键技术路线冻结 |
| **G1** | M1 结束 | T0/T1 全链路闭环成功，受控环境复活成功率 100% |
| **G2** | M2 结束 | NFR-01/02/03 达标，NAT 矩阵通过率达标 |
| **G3** | M3 结束 | [兼容性矩阵](../05-quality/TEST.md) 全绿，NFR-04 达标 |
| **G4** | M4 结束 | 安全测试全通过，公测 KPI 达标，法务评审通过 |

门禁证据要求见 [TEST §7](../05-quality/TEST.md)。

## 4. 迭代节奏与评审点

- 每 2 周一个迭代，输出可运行版本 + 指标快照。
- 每里程碑末尾做架构复审，更新 [ADR](../02-architecture/adr/README.md) 与 [RISK](../05-quality/RISK.md)。
- 协议（[PROTO](../04-protocol/PROTO.md)）任何变更需 `PROTOCOL_VERSION` 递增并同步 DOC-09，禁止静默修改。
- 「待定」状态的 ADR 必须在本表有对应门禁，否则视为悬空决策。

## 5. 人力与技能要求

| 角色 | 关键能力 | 主要投入阶段 |
| :--- | :--- | :--- |
| P2P / 网络工程师 | Kademlia、BT Peer Wire、NAT 穿透、uTP | M0–M3（关键路径） |
| Rust 系统工程师 | Tokio、异步 I/O、内存与句柄治理 | 全程 |
| 存储 / 索引工程师 | SQLite、文件监控、哈希与分片 | M1–M2 |
| 前端工程师 | Web UI、WebSocket、AriaNg 兼容 | M3 |
| 安全 / 测试工程师 | 威胁建模、故障注入、压测 | M2–M4 |
| 法务 / 合规顾问 | P2P 工具合规边界 | M0、M4 |

## 6. 当前状态（2026-11-09）

- 阶段：**设计冻结候选，M0 进行中（Spike 首批观测已产出，G0 未通过）**。
- 已完成：文档体系工程化（v2.4 自包含索引页，21 份 DOC + 10 份 ADR）、项目目录骨架、风险登记册、测试与计划基线。
- 已就位：**[T0 双节点验收骨架](../../tests/e2e/t0/README.md)**（21 条用例、断言引擎、双实例编排、JSON 报告）+ **CLI v1 契约与 20 条真实观测映射**（`t0_cli.py` / `t0_cli_selftest.py`）+ **[M0 Spike 实验包](../../tests/spikes/README.md)**（S1–S8 判据、`spikes.json` 状态登记、`m0_preflight.py` 就绪度检查）。
- 当前 `CliAdapter.wired = False`：`--mode real` 仍 `BLOCKED`（1 PASS / 20 BLOCKED，退出码 2），**不构成 G1 门禁证据**。接线清单见骨架 README §5，用例判据见 [TEST §2.1](../05-quality/TEST.md)。
- 工具链：`rustc` / `cargo` 1.99.1 已在本机安装（2026-11-09），`m0_preflight.py` 的工具链检查已转为 `[OK]`，S1 最小 Demo 具备执行条件。
- 决策：S8 法务评审由项目所有者于 2026-11-09 决定豁免，登记为 `pass`（**未取得法务书面意见**，R-11 风险保留，M4 公测前复核）；S2 实网实验的人工批准已写入 `spikes.json` 的 `public_kad_approval`。
- 已产出首批观测（2026-11-09）：[S1 最小 Demo](../../tests/spikes/s1/README.md)（握手拦截、按需实例化 `verified`；Pause-Resume、真实分片传输 `partial`）+ [S2 三层实验](../../tests/spikes/s2/README.md)（L1 逻辑预演发布 5/5、命中 5/5；L2 [最小 eMule KAD 客户端](../../tests/spikes/s2/kad-mini/README.md) 用真实 `0xE4` Kademlia2 报文完成发布 5/5 被 ACK、查询命中 5/5、84 条逐包原始记录、0 协议错误）。按判据**均不得判 `pass`**；证据路径登记在 `spikes.json` 的 `observations.real_wire`。S1 / S2 状态仍为 `not_run`。
- **S2 实网排期**：[PLAN.md](../../tests/spikes/s2/PLAN.md) 的 **P0–P3 已完成**（协议核实 / wire codec / 本地真实报文闭环 / G0 参数与 bootstrap 清单），**P4 实网小样本已于 2026-11-10 执行但被环境阻断、结论不可用**（见下）。`S2.public_kad_approval.pending` 已清除并写入 `confirmed`（`sample_size=3`、`rate_limit_pps=0.2`）；用户提供 `nodes.dat`（200 个 KAD contacts）转成本地 bootstrap，仓库不含节点 IP；LAN 真实 eMule 的互操作探测已跑通（`BOOTSTRAP_REQ` 得明文/压缩响应）。`live --check` 带公网清单现报门禁通过但**永不发包**，须显式去掉 `--check` 才执行 P4。
- **判据口径已回写（v2.4.6）**：按 aMule 源码修正 `S2.criteria`、`S2.assumption`、[PROTO §1/§2.0/§2.1](../04-protocol/PROTO.md)、[ADR-001](../02-architecture/adr/ADR-001-kad-network.md)、[ADR-002](../02-architecture/adr/ADR-002-bt-engine.md)、[RISK R-02](../05-quality/RISK.md)：eMule KAD 无 `FIND_VALUE`（映射为 `SEARCH_KEY_REQ/RES`）、ID 空间 128-bit、前缀线上不可见、`Node_Descriptor` 须编码为 Tag、`SEARCHTOLERANCE` 越界静默丢弃；「发布失败率」拆成协议层/索引层/成功三桶。只改 KAD 集成映射，`PROTOCOL_VERSION` 保持 1、T0 golden 不变。
- **P4 前置一致性修正（v2.4.7）**：复核发现 `live` 实现与 v2.4.6 判据存在四处错配，已全部修复——
  发布前改为 `KADEMLIA2_REQ/RES` 最多 3 跳迭代收敛到 `SEARCHTOLERANCE` 邻域（未收敛则不发布）、
  移除与「跳过 HELLO」冲突的 `HELLO_REQ`、新增零依赖 `inflate.rs` 解码 `0xE5` zlib 压缩帧、
  `live_gate` 实际解析 bootstrap 并拒绝非公网 IPv4 / `lan-interop-probe-only`、socket 改用 `bind_any()`。
  复验 `cargo test` 7/7、`selftest` 37/37、`loopback` 5/5 发布 + 5/5 命中 + 0 协议错误，
  真实 519 B `0xE5` 帧解出 521 B payload / 20 个联系人。详见 [PLAN.md §6](../../tests/spikes/s2/PLAN.md)。
  **P4 仍未产出结论**，`S2.status` 保持 `not_run`。
- **P4 首次执行（v2.4.8，2026-11-10）**：项目所有者显式下令执行。门禁 0 blocker，
  3 条 `0xE4 0x01` `BOOTSTRAP_REQ` 以严格 5 s 间隔发往 3 个公网 KAD 节点，**0 响应**；
  因学不到联系人无法收敛到 `SEARCHTOLERANCE`，`live` 按 `criteria #2` 正确跳过全部 3 个样本，
  **未发布任何测试 Key**。对照实验：DNS/UDP-53 正常（≈55 ms）、STUN/19302 与 /3478 全超时、
  直连 TCP 超时、LAN 真实 eMule 对同一帧正常回 523 B / 20 contacts、aMule 源码确认
  `BOOTSTRAP_REQ` 无需 payload —— 归因是**执行环境过滤公网高位 UDP**，
  不是报文格式、不是节点拒绝、不是命名空间被拒。判 `inconclusive_environment_blocked`，
  三桶计数整体作废。新增可选前置诊断 [udp_egress_check.py](../../tests/spikes/s2/udp_egress_check.py)
  （退出码 0 才可跑 P4）。详见 [PLAN.md §7](../../tests/spikes/s2/PLAN.md)。
- **⚠️ v2.4.14 更正（上一条「环境过滤高位 UDP」与下方「换网」结论作废）**：根因是 KAD 联系人 IP 字节序读反（线上为点分十进制反序），所有 0/N 都发往错误地址。修正后本机 10 目标 KAD 探针 **9/10 收到 523 B `BOOTSTRAP_RES`**，P4 按 G0 参数两度重跑已 `public_kad_contacted=true`，收到 `0xE5` 压缩 `BOOTSTRAP_RES` 与 `0x21/0x29` 迭代应答，但每跳 2 候选 × 3 跳只学到 30–34 联系人，未命中 `SEARCHTOLERANCE` 邻域，3/3 样本跳过发布。**不再需要换网或改防火墙**；下一步是修 `live` 的收敛宽度（`ALPHA_QUERY=5`、`KADEMLIA_FIND_NODE=0x0B`、更多跳数）、改用实时 `BOOTSTRAP_RES` 目标，再按 G0 参数重跑。详见 [PLAN.md §11](../../tests/spikes/s2/PLAN.md)。
- 未开始：`Cargo.toml` 与 crate 骨架（按 M0 门禁刻意推迟，见 [src/README.md](../../src/README.md)）、S1 的双 peer 真实分片传输、S3–S7 全部实验、S2 的 P4 **有效**实网小样本（首次执行被环境阻断）。仍缺第三方 BT 客户端与 M1 二进制，`CliAdapter.wired` 保持 `False`；实时阻塞项以 `m0_preflight.py` 输出为准。
- 立即下一步见 [OPEN-QUESTIONS §3](OPEN-QUESTIONS.md)：v2.4.10 跨主机 STUN 对照在本机与 LAN eMule 机器上同得 `BLOCKED` / 退出码 2；v2.4.11 新增 KAD 专用探针确证本机不可达；v2.4.12 该机 `Kad 网络` 面板否证 firewalled 假设，但源码复核显示「打开」是入站 DNAT 即可满足的入站测试；v2.4.13 该机 KAD 专用探针已执行得 0/3 / 退出码 2，本机另用新鲜 10 目标宽扫 0/10，并按 eMule 官方帮助澄清「下载/搜索正常」不等于 KAD 出站可用（详见 [PLAN.md §10](../../tests/spikes/s2/PLAN.md)）。**工程决策已与归因解耦：本网无独立客户端被证明可跑 P4，直接换到高位 UDP 未被过滤的网络** → 先跑诊断得 0 并刷新 `nodes.dat`，再由实验执行人显式**重跑 S2 P4 实网小样本**（最小 3 条 Key、0.2 包/秒、逐次原始记录、三桶分类），再由人工回填 `S2.status`。剩余 `Kademlia` 方法搜索 / 该机新鲜宽扫 / 统计计数只用于收窄归因，不阻塞换网；S1 补齐双 peer 传输，S3–S7 在 G0 前补齐；G0 通过后编译 M1 二进制，再按骨架 §5 完成第 6～9 项接线。

## 相关文档

- [DOC-04 TECH-STACK Spike 清单](../02-architecture/TECH-STACK.md)
- [DOC-12 TEST 测试计划](../05-quality/TEST.md)
- [DOC-15 待决问题清单](OPEN-QUESTIONS.md)
- [CHANGELOG](../../CHANGELOG.md)
