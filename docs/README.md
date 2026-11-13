---
doc_id: DOC-00
title: 文档体系说明与导航
category: 文档治理
version: 2.7
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
- **v2.7（2026-11-13，M1 开工补记）**：DOC-04 → 2.5、DOC-11 → 2.1（S5 实测标定默认值）、
  DOC-13 → 2.2、DOC-14 → 2.9、DOC-22 → 1.1；本轮未新增 DOC 编号，未改 `PROTOCOL_VERSION` 与 T0 golden。
  T0 real 21/21 PASS 只覆盖单机回环，G1 仍需 T1 与 S3/S5/S6 必测回填，M2 保持 `blocked_by_G1`。
- 协议（DOC-09）**私有契约字段**（§3 `WAKEUP_REQ` / `WAKEUP_ACK`）变更必须递增 `PROTOCOL_VERSION`、新增 ADR 并更新 T0 golden，禁止静默修改。
- **映射修订例外**：只改 §1 / §2 的**外部 KAD / eD2k 集成映射**（如 v2.3 的 Kad tag 帧头、v2.4 的双轨 Key、v2.5 的 `0x44` / `0x34` 载荷与 eD2k TCP 序列）且不动 §3 字段时，按 [PROTO §4](04-protocol/PROTO.md) 只递增该文档的 `version` 并记录源码依据，`PROTOCOL_VERSION` 与 T0 golden 保持不变。本条与 PROTO §4 是同一规则的两处表述。

## 8. 构建与校验

```bash
python3 scripts/build_docs.py            # 生成配套 HTML
python3 scripts/build_docs.py --check    # 校验 front matter 与相对链接（CI 使用）
```

依赖：Python 3.9+、`markdown>=3.4`。构建产物 `docs/**/*.html` 由 `.gitignore` 排除，唯一例外是 `docs/index.html`。

`docs/index.html` 是**自包含单文件门户**：构建器把全部文档的渲染正文与 Markdown 源码（含 `docs/` 外被引用的 `README.md`、`CHANGELOG.md`、`src/tests/scripts/README.md`、`tests/e2e/t0/README.md`、`tests/spikes/README.md`、`config/coldrevive.example.toml`，共 8 项）写入页内的 `DOCS_PAYLOAD` 数据块，点击「阅读 / MD」在页内阅读器中就地渲染，不跳转到同目录文件。原因是预览面板 / `srcdoc` 沙箱通常只承载 `index.html` 一个文件，指向同级 `.html` 的相对链接会解析失败并提示 *no data available*。

因此：**修改任一 `.md` 后必须重跑构建**，否则索引页内嵌内容不会更新；手工编辑 `index.html` 时不要改动 `DOCS_PAYLOAD_START` / `DOCS_PAYLOAD_END` 两个 HTML 注释标记之间的内容。`--check` 会校验这两个标记是否存在。
