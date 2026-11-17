---
doc_id: DOC-00
title: 文档体系说明与导航
category: 文档治理
version: 2.8
status: 有效
owner: 架构组
updated: 2026-11-13
---

# 文档体系说明与导航

> **Docs as Code**：`docs/**/*.md` 是唯一事实来源（Single Source of Truth），`.html` 为 `scripts/build_docs.py` 的构建产物，禁止直接编辑。
> 浏览器入口：[index.html](index.html)｜项目入口：[../README.md](../README.md)｜变更记录：[../CHANGELOG.md](../CHANGELOG.md)

## 1. 文档体系设计原则

| 原则 | 说明 |
| :--- | :--- |
| 单一职责 | 一份文档只回答一类问题；超过 300 行即考虑再拆 |
| 唯一事实来源 | 同一事实只在一处定义，其他文档用相对链接引用，不复制正文 |
| 可追踪 | 每份文档有 `doc_id`、版本、状态、责任人、更新日期（YAML front matter） |
| 决策留痕 | 每项技术抉择独立成 ADR，记录上下文、选项、结论与后果 |
| 归档不删除 | 被取代的版本移入 `09-archive/` 并标注废止，保留决策演化链 |
| 面向角色 | 索引页按角色提供阅读路径，避免新人从 1000 行大文件读起 |

## 2. 目录分层

| 目录 | 类别 | 回答的问题 |
| :--- | :--- | :--- |
| `01-product/` | 产品与需求 | **为什么做、做到什么程度** |
| `02-architecture/` | 架构与决策 | **系统如何划分、关键技术取舍** |
| `03-design/` | 详细设计 | **模块内部怎么实现、数据怎么存、流程怎么走** |
| `04-protocol/` | 协议与接口 | **对外契约是什么** |
| `05-quality/` | 质量保障 | **怎么验证、有什么风险** |
| `06-plan/` | 计划跟踪 | **什么时候做、还缺什么结论** |
| `07-operations/` | 发布运维 | **怎么交付、怎么运行、红线在哪** |
| `08-reference/` | 参考资料 | **术语、出处、待核实事实** |
| `09-archive/` | 归档基线 | **历史版本（已废止，不作为依据）** |

## 3. 文档编号与清单

| 编号 | 文档 | 路径 | 状态 |
| :--- | :--- | :--- | :--- |
| DOC-00 | 文档体系说明与导航 | [README.md](README.md) | 有效 |
| DOC-01 | 产品需求说明书 PRD | [01-product/PRD.md](01-product/PRD.md) | 待评审 |
| DOC-02 | 软件需求规格 SRS | [01-product/SRS.md](01-product/SRS.md) | 待评审 |
| DOC-03 | 总体架构设计 ARCH | [02-architecture/ARCH.md](02-architecture/ARCH.md) | 待评审 |
| DOC-04 | 技术选型与 Spike 清单 | [02-architecture/TECH-STACK.md](02-architecture/TECH-STACK.md) | 待评审 |
| DOC-05 | 架构决策记录集 ADR | [02-architecture/adr/README.md](02-architecture/adr/README.md) | 持续维护 |
| DOC-06 | 模块详细设计 LLD | [03-design/LLD.md](03-design/LLD.md) | 待评审 |
| DOC-07 | 端到端流程与状态机 | [03-design/FLOWS.md](03-design/FLOWS.md) | 待评审 |
| DOC-08 | 数据模型与迁移 | [03-design/DATA.md](03-design/DATA.md) | 待评审 |
| DOC-09 | 协议规范 PROTO | [04-protocol/PROTO.md](04-protocol/PROTO.md) | 待评审 |
| DOC-10 | 接口规范 API | [04-protocol/API.md](04-protocol/API.md) | 待评审 |
| DOC-11 | 配置规范 CONFIG | [04-protocol/CONFIG.md](04-protocol/CONFIG.md) | 待评审 |
| DOC-12 | 测试计划 TEST | [05-quality/TEST.md](05-quality/TEST.md) | 待评审 |
| DOC-13 | 风险登记册 RISK | [05-quality/RISK.md](05-quality/RISK.md) | 持续维护 |
| DOC-14 | 开发计划 ROADMAP | [06-plan/ROADMAP.md](06-plan/ROADMAP.md) | 持续维护 |
| DOC-15 | 待决问题清单 | [06-plan/OPEN-QUESTIONS.md](06-plan/OPEN-QUESTIONS.md) | 持续维护 |
| DOC-16 | 发布运维与合规 OPS | [07-operations/OPS.md](07-operations/OPS.md) | 待评审 |
| DOC-17 | 术语表 GLOSSARY | [08-reference/GLOSSARY.md](08-reference/GLOSSARY.md) | 有效 |
| DOC-18 | 讨论记录映射 | [08-reference/DISCUSSION-MAP.md](08-reference/DISCUSSION-MAP.md) | 有效 |
| DOC-19 | 参考资料与待核实项 | [08-reference/REFERENCES.md](08-reference/REFERENCES.md) | 有效 |
| DOC-21 | 法务评审材料包（S8） | [07-operations/LEGAL-REVIEW.md](07-operations/LEGAL-REVIEW.md) | 草稿 |
| DOC-22 | G0 技术路线冻结评审记录 | [06-plan/G0-REVIEW.md](06-plan/G0-REVIEW.md) | 已接受 |
| DOC-23 | S3 / S5 / S6 判据判定记录 | [06-plan/G2-SPIKE-VERDICT-2026-11-17.md](06-plan/G2-SPIKE-VERDICT-2026-11-17.md) | 已定稿 |

## 4. Front Matter 规范

每份文档开头必须包含（供构建脚本与 CI 校验）：

```yaml
---
doc_id: DOC-xx          # 唯一编号，见上表
title: 文档标题          # 中文全称
category: 产品 / 架构 / 设计 / 协议 / 质量 / 计划 / 运维 / 参考
version: 2.0            # 跟随文档体系版本
status: 草稿 | 待评审 | 有效 | 已废止 | 持续维护
owner: 责任角色          # 产品 / 架构组 / 内核组 / 测试组 / 运维组
updated: YYYY-MM-DD
---
```

`status` 语义：`草稿` 内容未稳定；`待评审` 等待 G0 门禁；`有效` 已冻结可执行；`持续维护` 活文档（风险、计划、ADR）；`已废止` 仅存于 `09-archive/`。

### 4.1 状态流转矩阵

> 本表是 `status` 字段的唯一流转依据。**没有「已审定」这个状态**——终态写 `有效`，
> 外部评审（法务 / 安全）通过是进入 `有效` 的前置条件之一，不单独构成状态值。

| 从 → 到 | 触发条件 | 必要证据 |
| :--- | :--- | :--- |
| 草稿 → 待评审 | 正文自洽、交叉引用无断链、责任人确认可送审 | `build_docs.py --check` 通过 |
| 待评审 → 有效 | **G0 通过**：S1–S8 全部有结论且技术路线冻结（[TECH-STACK §2](02-architecture/TECH-STACK.md)）；涉合规项须法务结论或登记豁免 | 测试报告、指标快照、未通过项与豁免说明（[TEST §7](05-quality/TEST.md)） |
| 待评审 → 有效（豁免路径） | 门禁项由项目所有者书面决定豁免 | 豁免记录（决策人 / 日期 / 理由 / 残余风险）+ 风险登记册同步更新 |
| 有效 → 已废止 | 被新版本取代 | 移入 `09-archive/`，索引页保留条目并加废止横幅 |
| 任意 → 持续维护 | 判定为活文档（风险、计划、ADR、待决问题） | 在索引页 `DOCS` 清单标注 |

单份文档改状态时必须**原子同步四处**，漏一处即自相矛盾：

1. 该文档 front matter 的 `status:` **和** 正文首段的「状态」文字（同一文件两处）；
2. 本文件 §3 清单表的对应行；
3. `index.html` 的 `DOCS` 数组 `s:` 字段（手工维护，非构建产物）；
4. `CHANGELOG.md` 记一条，并递增该文档 `version`、更新 `updated`。

改完运行 `python3 scripts/build_docs.py --check`，再全量构建使索引页内嵌内容刷新。

## 5. 交叉引用规则

1. 一律使用**相对路径**，禁止绝对本地路径与外部短链。
2. 引用其他文档中的具体章节时写成 `[ARCH §3.4 模块清单](02-architecture/ARCH.md)`，不复制被引用内容。
3. 引用 ADR 时写 `[ADR-002](02-architecture/adr/ADR-002-bt-engine.md)`。
4. 引用原始讨论时写 `[chat.md](09-archive/chat.md)`，并在 [DISCUSSION-MAP](08-reference/DISCUSSION-MAP.md) 登记轮次映射。
5. 新增/删除文档须同步：本表、`index.html` 的 `DOCS` 清单、`CHANGELOG.md`。
6. `docs/` 外的工程说明（如 `tests/e2e/t0/README.md`、`tests/spikes/README.md`）不占用 DOC 编号，但一旦被 docs 引用，必须登记进 `scripts/build_docs.py` 的 `EMBED_EXTRA`，否则页内阅读器无法就地打开该链接。

## 6. 按角色阅读路径

| 角色 | 顺序 |
| :--- | :--- |
| 架构师 / 新贡献者 | DOC-00 → 01 → 03 → 07 → 05 |
| 内核开发（Rust） | DOC-03 → 06 → 07 → 08 → 09 → 12 |
| 前端 / UI | DOC-03（进程视图）→ 06（M-14）→ 10 → 04 |
| 测试 | DOC-02（KPI）→ 07（异常表）→ 12 → [T0 验收骨架](../tests/e2e/t0/README.md) → [M0 实验包](../tests/spikes/README.md) |
| 运维 / 发布 | DOC-11 → 14 → 16 |
| 评审者 / 决策者 | DOC-01（关键结论）→ 03（ADR 摘要）→ 13 → 15 |

## 7. 版本与变更

- 文档体系版本记录在 `CHANGELOG.md`；单文档版本记录在各自 front matter。
- 技术内容变更 → 递增对应文档 `version`；仅结构调整 → 递增体系版本。
- **v2.4.78 补记（2026-11-17 / 判定）**：**S3 = `pass`、S5 = `fallback`、S6 = `pass`（`closed_with_exceptions`）**
  —— 三项 Spike 由所有者依真实证据手工判定，记录见
  [G2-SPIKE-VERDICT-2026-11-17](06-plan/G2-SPIKE-VERDICT-2026-11-17.md) 与 `tests/fixtures/g2-spike-verdicts.toml`。
  S3 限定在「资源可控 + 无串扰 + 公网 Mainline 测试 key 闭环」，不含 `cr-dht` 实现；
  S5 的 NFR-03 由令牌桶 + 默认 `ratio=0.02` 满足，uTP/LEDBAT 退让被否证；
  S6 只能写「按 eMule 语义结案，带已知限制」。
  **机器常量不变**（`s6_status=not_run` / `g2_claim=false` / `evidence_valid_for_pass=false`），
  **G2 仍未达标**（欠 M2 实现项）。新增 DOC-23。
- **v2.4.76 补记（2026-11-17 / ADR-016）**：**S6 阶段性判据改为 eMule 式可达语义**
  （[ADR-016](02-architecture/adr/ADR-016-s6-emule-like-reach.md)，项目所有者决定「目前只要达到
  eMule 类似的可达就可以了」）。不再穷举 4 类 NAT + CGNAT：`required_classes` 降级为描述性清单、
  不再是 pass 前提；改为要求真实 NAT 设备上 HighID（`direct` 可达）与 LowID（`direct` 失败但
  `relay` 送达）两半各有真实证据，且节点自报声明与实测一致（ADR-013 / W5.2）。
  **不变**：条款①两台不同物理主机 / 条款②真实设备 / 每路径 ≥30 次真实 `WAKEUP_ACK` /
  ADR-012 无数值承诺 / 脚本不自动 pass / `S6.status=not_run` / `g2_claim=false`。
  满足后 G2 只能写「按 eMule 语义结案，带已知限制」，不得写「NAT 矩阵通过」。
  落地：新 ADR-016 + ADR 索引 16 项；`nat_matrix_config.json` / `nat_matrix.py`（`--self-test` 24/24）、
  `cr-nat` 常量与 census 输出、`spikes.json` 的 `S6.criteria` 与
  `g0_review.s6_criteria_amendment_2026_11_17`；G0-REVIEW / ROADMAP / TEST / RISK / OPEN-QUESTIONS
  同步口径。`cargo test --workspace --offline` **168 passed / 0 failed**。
  方法见 [nat-matrix §23](../tests/spikes/s6/nat-matrix.md)；复审时点 M4 前或 ≥4 个真实出口可用时。
- **v2.7（2026-11-13，M1 开工补记）**：DOC-04 → 2.5、DOC-11 → 2.1（S5 实测标定默认值）、
  DOC-13 → 2.2、DOC-14 → 2.9、DOC-22 → 1.1；本轮未新增 DOC 编号，未改 `PROTOCOL_VERSION` 与 T0 golden。
  T0 real 21/21 PASS 只覆盖单机回环，G1 仍需 T1 与 S3/S5/S6 必测回填，M2 保持 `blocked_by_G1`（**历史口径，下条为更正**）。
- **门禁现状更正（2026-11-14 / v2.4.47）**：上一条是 M1 开工时点的历史记录，不是当前状态。
  **G1 已于 2026-11-14 通过**（T0 21/21 + T1 `--mode real` 双机 11/11，均 `evidence_valid=true`），
  **M2 门禁解除并已开工**（cr-nat 接入复活路径、T2 本地预检 10/10）。S3/S5/S6 仍 `not_run`，G2 未达标。
- **v2.4.50 补记（2026-11-14 / W3.5①）**：新增 [条件探针](../tests/spikes/probe_conditions.py)、
  [S3 双机双栈集成](../tests/spikes/s3/m1_dualstack_integration.py)（17/17 PASS）、
  [S6 NAT 行为分类](../tests/spikes/s6/nat_behavior.py) 与
  [四类 NAT 矩阵执行协议](../tests/spikes/s6/nat_matrix.py)（干跑 `BLOCKED`）；
  S5 真实 uTP 栈 5 轮复跑。全部为 `evidence_valid_for_pass=false` 的附件证据，
  **S3 / S5 / S6 仍 `not_run`、`matrix_coverage=0/4`、G2 未达标**。
  本轮改动落在 DOC-15（ROADMAP）、G0-REVIEW、OPEN-QUESTIONS 与 spikes 文档，
  未改 `PROTOCOL_VERSION` 与 T0 golden。
- **v2.4.57 补记（2026-11-15 / W3.8）**：中继去单点化 —— `cr-nat::relay_server` 新增实例间
  `PEER_HELLO` / `PEER_SYNC` / `FORWARD` 三个帧（快照整份覆盖、转发单跳不成环），
  `coldrevive-relay` 新增 `--peer` / `--instance-id`，`nat.relay` 接受列表并由
  `relay_wake_any` / `relay_sub` 顺序切换。两台真实主机实测跨实例双向 60/60 帧、
  `dropped=0`，重启一台后快照自愈；反证为「不配 `--peer` 的隔离实例同一条用例立即 `dropped=1`」。
  本轮改动落在 [nat-matrix.md §16](../tests/spikes/s6/nat-matrix.md)、
  [nat_lab README](../tests/spikes/s6/nat_lab/README.md)、DOC-15（ROADMAP）与 spikes.json；
  未改 `PROTOCOL_VERSION` 与 T0 golden（新帧只在中继实例之间使用，客户端不发）。
  **这是中继可用性工程，不产生 NAT 类别样本**：S3/S5/S6 仍 `not_run`、G2 未达标、R-04 不判定。
- **v2.4.64 补记（2026-11-16 / W3.15）**：**三台 LAN 主机真实第三方客户端互操作**。所有者提供
  221/222/209（SSH），分别装 Transmission 3.00 / qBittorrent 4.5.1 (nox) / aria2-next，G3
  「本机无第三方客户端」卡点解除。新增 [local_tracker.py](../tests/e2e/g3/local_tracker.py)
  （标准 BEP23 compact，仅 RFC1918，`--self-test` 3/3；修过二进制 info_hash 字节解析 bug）。
  A 上 S1 cross-seed 产标准 `.torrent`（1 MiB），三台机器原样客户端各下一轮：均 1 MiB 全量、
  SHA-1 `f0f990bb…` 匹配、seeder `uploaded_bytes=1048576`、tracker `completed`；qBittorrent
  首轮残留 peer 被判不干净废弃重跑。`compat-matrix.toml` 增 `[[remote_clients]]`，核验器增
  `runnable_remote`，基线 3 行 runnable / 5 行 blocked，`g3_status` 恒 `not_run`。证据是 gitignored
  附件 `tests/e2e/g3/reports/interop-bt-2026-11-16.json`。**G3 未全绿**（eMule GUI / BitComet /
  AriaNg / 浏览器插件 / 关闭 UI 未取证），S3/S5/S6 `not_run`、`g2_claim=false` 不变。
- **v2.4.63 补记（2026-11-16 / W3.14b）**：生产 `cr-hash` 补齐真实 eD2k hash —— 从 S2 kad-mini
  探针迁入零依赖 MD4、`kad_keyword_hash`、`ed2k_hash`（aMule 的 **Red** 整块边界规则：
  文件刚好 N 块时追加空块 MD4）与标准 `ed2k://|file|...|/` 链接生成（文件名做最小百分号编码）。
  这一步是标准协议互通的生产侧前提：只有探针里有 hash 不够，旧客户端拿到错的 `fileID`
  会「搜得到却永远下载不了」。新增 3 条单测（RFC 1320 向量 / aMule 1×PARTSIZE 向量 /
  链接转义），`cargo test --workspace --offline` **148 passed / 0 failed**。
- **v2.4.62 补记（2026-11-16 / W3.13）**：**中继数据面字节预算**（[ADR-014](../docs/02-architecture/adr/ADR-014-relay-data-budget.md)）。
  所有者要求「数据中转要设阈值，别把中继带宽爆了」。新增全局令牌桶 `--data-rate`（默认
  512 KiB/s）与 `--data-burst`（默认 2 MiB）：**≤4 KiB 的唤醒信令绕过预算**，其余帧整帧
  放行或整帧拒绝；超限计 `bandwidth_limited` / `bandwidth_limited_bytes`，与 `dropped`
  分开，便于区分「中继忙」与「节点离线」。这是部署侧带宽护栏，**不是可达率承诺**
  （ADR-012 口径）。同轮按所有者决策重排路线：**先做标准协议互通（S1/S2 → G3 矩阵），
  再谈补种**；出口类别 `--census` 移到 S6 证据阶段，当前不排期。145 passed / 0 failed。
- **v2.4.61 补记（2026-11-16 / W3.12）**：**借 eMule 的 LowID 自我声明**\n  （[ADR-013](../docs/02-architecture/adr/ADR-013-emule-lowid-declaration.md)）。\n  所有者质疑「eMule 同样存在这几类 NAT 且多为 LowID，为什么不直接借鉴」——在 R-04 取消数值承诺后，\n  这是对「为什么还要自研穿透栈」的正面质询，必须回答而不能绕开。\n  回答分两半：**借**「失败应当被预先知道，而不是试出来」——节点自己探测、自己声明可达性类别\n  （`direct`/`relay-only`/`unknown`），注册中继即上报（0x08），跨实例同步（0x09），可查询（0x0A/0x0B）；\n  **不借**三个语义：LowID 即终点（我们能换一条路——中继转发的是几百字节唤醒信令，不是整个文件，\n  代价低六个数量级，这是本产品区别于「又一个 eMule」的存在理由）、HighID=谁都能连\n  （我们的 `direct` 是「配合打洞前提下可直连」）、让服务器替客户端判断。\n  纪律写进代码：`unknown` 永远不许当 lowid 用、cgnat 不自动分类、TTL 过期降级、时钟回拨算过期、\n  伪造快照拒 / id 不一致拒 / 断开即清理。**部署顺序先升中继再升客户端**。\n  边界：唤醒路径尚未接入「先查再打洞」，本轮只交付原语 + `coldrevive-nat --lookup` 诊断消费。\n  同轮撤掉 PRD「公开网络 ≥90%」数值承诺（ADR-012 口径覆盖产品层），并在 SRS 明确\n  「工程指标可设验收线、外部环境成功率不设数值目标」的分界。方法见 [nat-matrix §20](../tests/spikes/s6/nat-matrix.md)。\n- **v2.4.60 补记（2026-11-16 / W3.11）**：**R-04 改为「尽力」，取消公网可达率的数值承诺**
  （[ADR-012](../docs/02-architecture/adr/ADR-012-r04-best-effort.md)）。项目所有者明确「不能承诺具体数字，
  只能是尽力，我们是提供一个可能性，不是确保实现」。这不是「达不到所以放宽」，而是三条更根本的理由：
  ① 产品卖的是「多一条能叫醒机器的路」，不是可达率 SLA；② 阈值会污染测量——「测不到 70%」被读成「不合格」，
  只剩「挑环境凑数字」或「把没测到的格子填成通过」两条路；③ 分母（所有家用 NAT）不可抽样，
  5 个样本的百分比没有统计意义却会被当 SLA 引用。
  落地：删 `R04_MIN_REACHABILITY`，改 `R04_STANCE` + `R04_NUMERIC_COMMITMENT=None`（保留字段名，
  让机器读到「明确不给数」），加单测防阈值被加回；矩阵配置换成 `s6_pass_criteria` 四条定性要求；
  `nat_matrix.py` 不再做阈值比较（自检 17/17）；RISK R-04 触发条件改为「某类环境中直连与中继同时失败」。
  **中继回退改为常开**。一条强度都没放松：条款①②与 `evidence_scope` 分层不变，S6 仍 `not_run`。
  方法见 [nat-matrix.md §19](../tests/spikes/s6/nat-matrix.md)。
- **v2.4.59 补记（2026-11-16 / W3.10）**：出口类别清单 —— 条件是「一个出口只对应一个真实 NAT 类别」，所以「用现有两个 IP 一次验证一类」的正确读法不是「把同一个出口分五次测」，而是**换出口再测**。为了让「环境不够」不变成「结论靠编」，新增 `coldrevive-nat --census <jsonl>` / `--report <jsonl>`：把每次探测沉淀成单行 JSON 样本（`nat_class` + 正交两维 `mapping_behavior`/`filtering_behavior` + `observations`/`distinct_mappings` 依据 + `double_nat` 双层 NAT 判据 + `variant` 配置变体 + 防重 `sample_id`）；`TraversalReport` 相应补 `filter_behavior` 与原始 `observations`（此前算完即丢，导致「能说类别、说不出依据」）。**三条纪律写进代码**：① 同一出口同一 `variant` 出现两个类别 ⇒ `unresolved`，整条不进清单；② `matrix_coverage="none"` / `r04_judgeable=false` / `s6_status="not_run"` / `g2_claim=false` 是常量，因为清单在原理上不含可达性数字；③ 读回遇未知字段直接报错并带行号。实测 A 机家宽出口 `112.5.60.4` = `port-restricted-cone`（eim + port-restricted）；结合 W3.7 对 `112.5.32.34` 的 APDM 结论，**现有两个出口本就是两类**，缺 `full-cone`/`restricted-cone`/`cgnat`。改动落在 [nat-matrix.md §18](../tests/spikes/s6/nat-matrix.md)、DOC-14（ROADMAP）与 spikes.json；未改 `PROTOCOL_VERSION` 与 T0 golden（只加探测输出字段与新的 CLI 入口，业务帧不变）。
- **v2.4.58 补记（2026-11-16 / W3.9）**：中继公网生产化 —— 新增 `cr-nat::secure`（std-only，
  复用 `cr-hash`）：PSK 双向挑战应答 + HMAC-SHA256 计数器密钥流 + 逐记录 HMAC，序号严格递增
  （篡改与重放都表现为固定序号上 tag 失败 → 断链，不重同步）；`PING`/`PONG` 放在记录层，
  订阅连接不需要业务保活。**明确不是 TLS**（无证书/域名校验、无前向保密、无密钥轮换，未经审计）。
  `coldrevive-relay` 新增 `--auth-key` / `--auth-key-file` / `--allow-insecure` / `--rate` /
  `--burst` / `--idle-timeout` / `--max-conns` / `--max-nodes` / `--handshake-timeout`，
  并新增启动策略：**绑非回环地址且无 PSK 直接拒绝启动**。客户端侧新增 `nat.relay_key`。
  两台真实主机实测：`pair` PASS、`latency` 30/30（p50 0.88 ms）、跨实例加密 60/60 帧、
  两端 `dropped=0`；三条反证（错误密钥 / 明文客户端 / 无密钥绑公网）全部如实失败。
  同轮**纠正**：`112.5.32.34:57100 → 192.168.34.222:7100` 并未打通（同 CPE 的 `54022`
  27 ms 可达、`57100` 按对端公网 IP 抓包 0 包），因此**本轮没有跨公网时延数字**。
  改动落在 [nat-matrix.md §17](../tests/spikes/s6/nat-matrix.md)、
  [nat_lab README](../tests/spikes/s6/nat_lab/README.md)、DOC-14（ROADMAP）与 spikes.json；
  未改 `PROTOCOL_VERSION` 与 T0 golden（记录层在中继链路内部，业务帧语义不变）。
  **这是中继可用性与传输安全工程，不产生 NAT 类别样本**：S3/S5/S6 仍 `not_run`、G2 未达标、R-04 不判定。
- 协议（DOC-09）**私有契约字段**（§3 `WAKEUP_REQ` / `WAKEUP_ACK`）变更必须递增 `PROTOCOL_VERSION`、新增 ADR 并更新 T0 golden，禁止静默修改。
- **映射修订例外**：只改 §1 / §2 的**外部 KAD / eD2k 集成映射**（如 v2.3 的 Kad tag 帧头、v2.4 的双轨 Key、v2.5 的 `0x44` / `0x34` 载荷与 eD2k TCP 序列）且不动 §3 字段时，按 [PROTO §4](04-protocol/PROTO.md) 只递增该文档的 `version` 并记录源码依据，`PROTOCOL_VERSION` 与 T0 golden 保持不变。本条与 PROTO §4 是同一规则的两处表述。

## 8. 构建与校验

```bash
python3 scripts/build_docs.py            # 生成配套 HTML
python3 scripts/build_docs.py --check    # 校验 front matter 与相对链接（CI 使用）
```

依赖：Python 3.9+、`markdown>=3.4`。构建产物 `docs/**/*.html` 由 `.gitignore` 排除，唯一例外是 `docs/index.html`。

`docs/index.html` 是**自包含单文件门户**：构建器把全部文档的渲染正文与 Markdown 源码（含 `docs/` 外被引用的 `README.md`、`CHANGELOG.md`、`src/tests/scripts/README.md`、`tests/e2e/t0/README.md`、
`tests/e2e/t1/README.md`、`tests/spikes/README.md`、`tests/spikes/s5/README.md`、
`config/coldrevive.example.toml`，共 10 项）写入页内的 `DOCS_PAYLOAD` 数据块，点击「阅读 / MD」在页内阅读器中就地渲染，不跳转到同目录文件。原因是预览面板 / `srcdoc` 沙箱通常只承载 `index.html` 一个文件，指向同级 `.html` 的相对链接会解析失败并提示 *no data available*。

因此：**修改任一 `.md` 后必须重跑构建**，否则索引页内嵌内容不会更新；手工编辑 `index.html` 时不要改动 `DOCS_PAYLOAD_START` / `DOCS_PAYLOAD_END` 两个 HTML 注释标记之间的内容。`--check` 会校验这两个标记是否存在。
