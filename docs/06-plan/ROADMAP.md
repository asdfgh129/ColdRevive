---
doc_id: DOC-14
title: 开发计划与里程碑（ROADMAP）
category: 计划
version: 2.7
status: 持续维护
owner: 项目组
updated: 2026-11-11
---

# 开发计划与里程碑（ROADMAP）

> 编号 DOC-14 · 版本 2.6 · 状态 **持续维护** · 责任人 项目组 · 更新 2026-11-11
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

## 6. 当前状态（起始 2026-11-09，更新至 2026-11-11）

> 本节按时间追加观测，**较早条目保留当时口径**，最新状态以本节末尾条目与
> `spikes.json` 为准。截至 2026-11-11：`S1 / S2 / S4 / S7 / S8 = pass`（S4/S7 于 2026-11-11 经真实量测回填）；
> `S3 / S5 / S6 = not_run`（S3 首轮结构性证据已产出、S5/S6 分析就绪），**G0 未通过**。
> 其中「立即下一步 / 换网 / 重跑 P4」等条目属 2026-11-09～10 的历史排期，
> 已被 v2.4.14 / v2.4.26 的字节序与 Kad tag 编码修复推翻，**不再作为当前执行依据**，
> 保留仅为决策演化链；当前应执行的是本节末尾「G0 剩余缺口清单」。

- 阶段：**设计冻结候选，M0 进行中（Spike 首批观测已产出，G0 未通过）**。
- 已完成：文档体系工程化（v2.4 自包含索引页，21 份 DOC + 10 份 ADR）、项目目录骨架、风险登记册、测试与计划基线。
- 已就位：**[T0 双节点验收骨架](../../tests/e2e/t0/README.md)**（21 条用例、断言引擎、双实例编排、JSON 报告）+ **CLI v1 契约与 20 条真实观测映射**（`t0_cli.py` / `t0_cli_selftest.py`）+ **[M0 Spike 实验包](../../tests/spikes/README.md)**（S1–S8 判据、`spikes.json` 状态登记、`m0_preflight.py` 就绪度检查）。
- 当前 `CliAdapter.wired = False`：`--mode real` 仍 `BLOCKED`（1 PASS / 20 BLOCKED，退出码 2），**不构成 G1 门禁证据**。接线清单见骨架 README §5，用例判据见 [TEST §2.1](../05-quality/TEST.md)。
- 工具链：`rustc` / `cargo` 1.99.1 已在本机安装（2026-11-09），`m0_preflight.py` 的工具链检查已转为 `[OK]`，S1 最小 Demo 具备执行条件。
- 决策：S8 法务评审由项目所有者于 2026-11-09 决定豁免，登记为 `pass`（**未取得法务书面意见**，R-11 风险保留，M4 公测前复核）；S2 实网实验的人工批准已写入 `spikes.json` 的 `public_kad_approval`。
- 已产出首批观测（2026-11-09）：[S1 最小 Demo](../../tests/spikes/s1/README.md)（握手拦截、按需实例化 `verified`；Pause-Resume、真实分片传输 `partial`）+ [S2 三层实验](../../tests/spikes/s2/README.md)（L1 逻辑预演发布 5/5、命中 5/5；L2 [最小 eMule KAD 客户端](../../tests/spikes/s2/kad-mini/README.md) 用真实 `0xE4` Kademlia2 报文完成发布 5/5 被 ACK、查询命中 5/5、84 条逐包原始记录、0 协议错误）。按判据**均不得判 `pass`**；证据路径登记在 `spikes.json` 的 `observations.real_wire`。S1 / S2 状态**当时**为 `not_run`（S2 已于 2026-11-10 判 `pass`，见下方对应条目）。
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
- **⚠️ v2.4.14 更正（上一条「环境过滤高位 UDP」与下方「换网」结论作废）**：根因是 KAD 联系人 IP 字节序读反（线上为点分十进制反序），所有 0/N 都发往错误地址。修正后本机 10 目标 KAD 探针 **9/10 收到 523 B `BOOTSTRAP_RES`**，P4 按 G0 参数两度重跑已 `public_kad_contacted=true`，收到 `0xE5` 压缩 `BOOTSTRAP_RES` 与 `0x21/0x29` 迭代应答，但每跳 2 候选 × 3 跳只学到 30–34 联系人，未命中 `SEARCHTOLERANCE` 邻域，3/3 样本跳过发布。**不再需要换网或改防火墙**；下一步是修 `live` 的收敛宽度（`ALPHA_QUERY=5`、`KADEMLIA_FIND_NODE=0x0B`、更多跳数）、改用实时 `BOOTSTRAP_RES` 目标，再按 G0 参数重跑。**v2.4.19 已按此修完**：plateau 改按路由表增长以恢复 `MAX_FIND_HOPS=10`，发布/搜索目标改为「`0x29` 已验证 + 真实 KadID 在责任区内」，不再盲发最近 N 个。**v2.4.21 已按 G0 参数（`sample_size=3` / `0.2 pps`）跑完第三轮**：验证覆盖修复生效（单样本 `verified` 8–12 → 27–30，`known` 86–127 → 138–165），`closest_top_chunk` 首次逼近 `SEARCHTOLERANCE`（2^24），但 `0x33`×11 / `0x43`×2 / 探针×9 **全部静默**；三轮合计 `0x21`→`0x29` 应答 86/99（87%）而索引操作 0/19（0%），Fisher p = 3.7e-14 —— 瓶颈是**索引操作本身的门禁**（疑似需先 `HELLO` 互认，或 `0x33`/`0x43` 载荷与 aMule 解析器不一致），**不是样本量、不是目标选择、也不是网络**。扩大样本已被证伪为有效手段。下一步见 [PLAN.md §14](../../tests/spikes/s2/PLAN.md)。
- **✅ S2 判 `pass`（2026-11-10，v2.4.26 / v2.4.27 / v2.4.28）**：§14 的「索引操作层门禁」归因被**证伪并撤回** —— 真因是
  Kad tag 帧头写成了 ed2k/met 的 `[type|0x80][name:1]`，而 aMule 的 Kad 路径走 `CFileDataIO::ReadTag`
  （名字恒为 `u16 长度 + 字节`，且不认 `STRn`/`BLOB`），第一个 tag 就抛异常，`PUBLISH_RES` 那一行永远执行不到。
  用局域网真实 aMule 做裁判二分（0 tag 回包、1 tag 静默）定位后修正 codec，第七轮公网首次得到
  **`PUBLISH_RES`（`load=1`，302 ms）** 与 **`SEARCH_RES` 命中**（265/265 字节零残余，5 个 tag 原样返回）；
  随后 `split-probe` 两进程 A/B：`split-a` 发布 → `split-b`（不同 socket/源端口、不同 KadID 种子、不同 bootstrap 入口）
  独立重走 tolerance zone 后取回，返回的 descriptor 携带 A 的源端口，**跨节点可取回成立**。
  由项目所有者指令「补一轮 B，成功后直接判 pass」登记 `S2.status = pass`。证据：
  `spikes.json` 的 `observations.p4_execution_v2427` / `ab_two_process_2026_11_10`，过程见 [PLAN §19–§21](../../tests/spikes/s2/PLAN.md)。
  **边界**：只判「接纳性」；R-02 的「按运营速率是否被当污染」仍需 M4 前按实测拒绝率复审，见 [RISK R-02](../05-quality/RISK.md)。
- **ADR-011 L1 / L2 已在探针层落地（2026-11-11，v2.4.31）**：L1 用 `MD4` 无前缀关键词在真实 aMule 上打通字典互通；L2 补齐 `0x44` / `0x34` 源字典与最小标准 eD2k TCP 下载/上传栈 + 最小上传队列（`tests/spikes/s2/ed2k-mini`）。回归：`kad-mini` 21/21 + 48/48 + 5/5、`ed2k-mini` 31/31 + 26/26 + 3/3 回环闭环、真实 eMule `HELLO` 握手成功；`PROTO.md` 升 v2.5 补 §2.2，`PROTOCOL_VERSION = 1` 与 T0 golden 未动。**证据全部是 RFC1918 / 127.0.0.1、0 个公网报文，不判 pass、不回填 `spikes.json`，G0 仍未通过**。缺口：该 LAN 对端共享文件数为 0，「从旧客户端下载」只有自两端回环证据，M1 须换真有共享文件的对端补测并实测 credit 排队代价（R-15）；Q9 / V-04 已结案，新增 [Q10](OPEN-QUESTIONS.md)（`capabilities.ed2k` 语义）待 M1 定稿。详见 [PLAN §22–§23](../../tests/spikes/s2/PLAN.md)。
- **S1 判据已齐备（2026-11-11，v2.4.32）**：`tests/spikes/s1/` 新增 `cargo run -- two-peer`，两个独立 librqbit 9.1.0 Session 在 `127.0.0.1` 完成标准 BT Peer Wire 分片传输（1 MiB / 64 片，seeder 上传限速 256 KiB/s）。leecher 从 `progress=0` 起，在 262144/1048576 中途 `pause` 900 ms 位图不前进、不回退，`unpause` 后 `finished=true` / 1048576；seeder `uploaded_bytes=1048576`、leecher 恢复后 `fetched_bytes=786432`（48 片），落盘 64/64 片 SHA-1 匹配、0 mismatch、逐字节等于确定性样本。**连续 4 轮结果一致**，C3/C4 由 `partial` 升为 `verified`，连同 `probe` 的 C1/C2，**S1 四条判据 4/4 齐备**；`probe` 子命令回归无变化。证据 `reports/s1-two-peer-*.json`，登记于 `spikes.json` 的 `S1.observations.two_peer`；见 [S1 README](../../tests/spikes/s1/README.md) 与 [ADR-002 §S1 判据补齐](../02-architecture/adr/ADR-002-bt-engine.md)。
  **边界与治理**：全部为 `127.0.0.1` 回环、0 个公网报文；同主机双 Session，未覆盖第三方 BT 客户端 / 跨主机 / NAT（属 G3）。`crate.rqbit` 阻塞项已解除（`S1.blocked_by` 置空）。`S1.status` 已于 2026-11-11 由实验执行人依 `two-peer` 4/4 `verified` 证据手工回填为 `pass`，[ADR-002](../02-architecture/adr/ADR-002-bt-engine.md) 据此定稿为「已接受」（选项 A：采用 `rqbit`）；[ADR-001](../02-architecture/adr/ADR-001-kad-network.md) 定稿条件已满足但仍待项目所有者签署 `public`/`private` 默认模式（治理动作，非技术缺口）。
- **S3 已启动（2026-11-11，v2.4.34）**：`tests/spikes/s3/` 新增零依赖双栈探针，本地双 HashMap（KAD 16B + BT 20B）在顺序 + 并发读写下**串扰/泄漏 = 0**（S3-C1 verified），双栈索引估算 10k/栈 ≈2.26 MiB、50k/栈 ≈11.08 MiB，均远低于 NFR-05 的 100 MB（S3-C2 verified 结构性上界）。属本地数据结构证据，不做真实 DHT 网络加入；`S3.status` 保持 `not_run`，最终 pass 须 M1 真实双栈集成 + G3 兼容性矩阵。详见 [S3 README](../../tests/spikes/s3/README.md)。
- 未开始：`Cargo.toml` 与 crate 骨架（按 M0 门禁刻意推迟，见 [src/README.md](../../src/README.md)；L2 参考实现按同一门禁暂放 `tests/spikes/`，G0 通过后迁入 `cr-ed2k`）、S4–S7 全部实验（1 万文件影子索引、uTP 退让、NAT 矩阵、4 GB 造种基准）。S3 已启动（[双栈结构化探针](../../tests/spikes/s3/README.md)，S3-C1/C2 结构性 verified，`status` 仍 `not_run`）。仍缺第三方 BT 客户端与 M1 二进制，`CliAdapter.wired` 保持 `False`；实时阻塞项以 `m0_preflight.py` 输出为准。
- **G0 剩余缺口清单（2026-11-11）**：① S1 状态回填（**已于 2026-11-11 完成**：`S1.status=pass`，[ADR-002](../02-architecture/adr/ADR-002-bt-engine.md) 定稿）；② S3–S7 五项实验尚未开工；③ [ADR-001](../02-architecture/adr/ADR-001-kad-network.md) 定稿与所有者签署（[ADR-002](../02-architecture/adr/ADR-002-bt-engine.md) 已于 2026-11-11 定稿）；④ 技术路线冻结评审。`m0_preflight.py` 当前报 `G0 就绪: False`、3 个阻塞项（第三方 BT 客户端、M1 二进制、CliAdapter 接线），这三项偏向 T0/G1 工程接线，**装齐它们并不能替代上述 G0 判据**。
- **S1 第三方客户端互操作轮已执行并通过（2026-11-11）**：按用户指令以本机 **Motrix Next**（捆绑 aria2 1.36.0）作第三方 seeder，rqbit `cross-lee` 直连 `127.0.0.1:16881` 拉取确定性 payload（128 片 × 16 KiB），**128/128 片 SHA-1 全部校验通过**（`all_pieces_verified=true`）。`S1.status` 维持 `pass`，结果仅作 G3 兼容性矩阵素材，登记 `spikes.json` 的 `cross_client_round`（operator attest 依用户指令满足）。
- **S4–S7 已推进（2026-11-11）**：**S4 = `pass`**（`tests/spikes/s4/` 真实 Rust 影子索引模型 + Windows 工作集实测：1 万文件 ≈7.79 MiB ≪ 100 MB，5 万 ≈15.9 MiB 兼满足 NFR-05，20 万 ≈53 MiB 仍有余量；R-06 去险）；**S7 = `pass`**（4 GB 造种基准 4.14 s / 989 MB/s、峰值 RSS 29.3 MiB、Idle 优先级；R-07 去险）；**S5 / S6 = `not_run`（分析就绪）**——S5 的 NFR-03 双层控制（令牌桶 20% 上限 + uTP/LEDBAT）中「前台劣化 ≤5%」须真实拥塞链路实测（M1/G3），S6 的 NAT 矩阵为分析型估计（含中继可达率 ≥95%），量化判据须 M2 真实多 NAT 实测；均按治理规则不虚判 `pass`。
- **G0 剩余缺口清单（2026-11-11 第二次更新）**：① ~~S1 状态回填~~ ✅；② ~~S4 / S7~~ ✅；剩余：S3 / S5 / S6 结论（S3 待 M1 真实双栈集成 + G3 矩阵；S5 待 M1/G3 真实拥塞实测；S6 待 M2 多 NAT 实测）；③ [ADR-001](../02-architecture/adr/ADR-001-kad-network.md) 定稿与所有者签署；④ 技术路线冻结评审。`m0_preflight.py` 报 `G0 就绪: False`：`g0_pending_spikes = S3,S5,S6`，其余 3 个 MISSING 为 T0/G1 工程接线（M1 二进制、CliAdapter、第三方下载器 PATH 标准名——Motrix Next 已装机并在 S1 证明互通，仅名称未在 PATH）。**装齐工程项不能替代上述 G0 判据**。
- 立即下一步见 [OPEN-QUESTIONS §3](OPEN-QUESTIONS.md)：v2.4.10 跨主机 STUN 对照在本机与 LAN eMule 机器上同得 `BLOCKED` / 退出码 2；v2.4.11 新增 KAD 专用探针确证本机不可达；v2.4.12 该机 `Kad 网络` 面板否证 firewalled 假设，但源码复核显示「打开」是入站 DNAT 即可满足的入站测试；v2.4.13 该机 KAD 专用探针已执行得 0/3 / 退出码 2，本机另用新鲜 10 目标宽扫 0/10，并按 eMule 官方帮助澄清「下载/搜索正常」不等于 KAD 出站可用（详见 [PLAN.md §10](../../tests/spikes/s2/PLAN.md)）。**工程决策已与归因解耦：本网无独立客户端被证明可跑 P4，直接换到高位 UDP 未被过滤的网络** → 先跑诊断得 0 并刷新 `nodes.dat`，再由实验执行人显式**重跑 S2 P4 实网小样本**（最小 3 条 Key、0.2 包/秒、逐次原始记录、三桶分类），再由人工回填 `S2.status`。剩余 `Kademlia` 方法搜索 / 该机新鲜宽扫 / 统计计数只用于收窄归因，不阻塞换网；~~S1 补齐双 peer 传输~~（**已于 2026-11-11 完成，见上**），S3–S7 在 G0 前补齐；G0 通过后编译 M1 二进制，再按骨架 §5 完成第 6～9 项接线。

## 相关文档

- [DOC-04 TECH-STACK Spike 清单](../02-architecture/TECH-STACK.md)
- [DOC-12 TEST 测试计划](../05-quality/TEST.md)
- [DOC-15 待决问题清单](OPEN-QUESTIONS.md)
- [CHANGELOG](../../CHANGELOG.md)
