# ColdRevive

> 把文件放进共享目录，它就永远能被搜到、被下到。

一个**不修改任何现有 P2P 协议**的混合下载客户端：以 eMule KAD 为去中心化黄页做发现，以轻量私有唤醒信令激活在线持有者临时做种，以标准 BitTorrent / eD2K 协议传输数据。对旧客户端完全透明。

| 项 | 值 |
| :--- | :--- |
| 当前阶段 | 设计冻结候选，待 M0 Spike 验证 |
| 代码仓库 | **https://github.com/asdfgh129/ColdRevive**（公开） |
| 在线文档 | **https://asdfgh129.github.io/ColdRevive/**（GitHub Pages 自动部署） |
| 文档版本 | v2.4（索引页自包含 + T0 验收骨架 + CLI 契约与 M0 实验包） |
| 验收骨架 | [T0 双节点 Demo](tests/e2e/t0/README.md) 已就位：CLI v1 契约与 20 条真实观测映射已落地，真实链路未接线，非 G1 证据 |
| M0 起跑线 | [Spike 实验包](tests/spikes/README.md)：S1–S7 `not_run`、S8 已豁免 `pass`；`cargo` 已就绪，[S1 Demo](tests/spikes/s1/README.md)、[S2 实验](tests/spikes/s2/README.md) 与[最小 eMule KAD 客户端](tests/spikes/s2/kad-mini/README.md)（真实 `0xE4` wire 回环 5/5、LAN 真实 eMule 互操作探测）已产出观测，均不构成 G0 结论；判据口径已按源码回写至 [PROTO §2.0](docs/04-protocol/PROTO.md)，[S2 实网排期](tests/spikes/s2/PLAN.md) P0–P3 完成；**v2.4.14 更正**：早期 P4 与所有探针的 0 响应源于我方把 KAD 联系人 IP 字节序读反（线上为点分十进制反序），包全发给了错误地址；修正后本机 10 目标宽扫 **9/10 收到真实 `BOOTSTRAP_RES`**，高位 UDP 一直可用，P4 两度重跑已真实连上公共 KAD（`0xE5` 压缩响应 + `0x21/0x29` 迭代），但 3/3 样本未收敛到 `SEARCHTOLERANCE` 邻域而跳过发布 —— 当前瓶颈是客户端收敛宽度，不需换网（见 [PLAN.md §11](tests/spikes/s2/PLAN.md)）；`S2.status` 仍 `not_run`；剩余阻塞见 `m0_preflight.py` |
| 文档入口 | **[docs/index.html](docs/index.html)**（浏览器打开） / [docs/README.md](docs/README.md)（Markdown 导航） |
| 原始想法 | [docs/09-archive/chat.md](docs/09-archive/chat.md) |
| 历史基线 | [docs/09-archive/ColdRevive-项目开发文档-v2.0.md](docs/09-archive/ColdRevive-项目开发文档-v2.0.md)（已废止，勿作为依据） |
| 技术栈 | Rust + Tokio / SQLite / axum（详见 [技术选型](docs/02-architecture/TECH-STACK.md)） |

## 快速导航

| 我想… | 去这里 |
| :--- | :--- |
| 了解项目要解决什么问题 | [PRD 产品需求说明书](docs/01-product/PRD.md) |
| 查看功能/非功能需求与验收指标 | [SRS 软件需求规格](docs/01-product/SRS.md) |
| 理解整体架构 | [ARCH 总体架构设计](docs/02-architecture/ARCH.md) |
| 了解模块怎么实现 | [LLD 模块详细设计](docs/03-design/LLD.md) |
| 查看协议与接口定义 | [PROTO 协议规范](docs/04-protocol/PROTO.md) · [API 接口规范](docs/04-protocol/API.md) |
| 看开发排期 | [ROADMAP 开发计划](docs/06-plan/ROADMAP.md) |
| 看风险与待决问题 | [RISK 风险登记册](docs/05-quality/RISK.md) · [OPEN-QUESTIONS](docs/06-plan/OPEN-QUESTIONS.md) |
| 查术语 / 讨论出处 | [GLOSSARY](docs/08-reference/GLOSSARY.md) · [DISCUSSION-MAP](docs/08-reference/DISCUSSION-MAP.md) |

## 目录结构

```
ColdRevive/
├── README.md                  本文件：项目入口与导航
├── CHANGELOG.md               文档与项目变更记录
├── .gitignore
├── docs/                      全部工程文档（Docs as Code）
│   ├── index.html             文档索引门户（自包含单文件，离线可读）
│   ├── README.md              文档体系、编号规则、阅读路径
│   ├── 01-product/            产品与需求   PRD / SRS
│   ├── 02-architecture/       架构与决策   ARCH / TECH-STACK / adr/
│   ├── 03-design/             详细设计     LLD / FLOWS / DATA
│   ├── 04-protocol/           协议与接口   PROTO / API / CONFIG
│   ├── 05-quality/            质量保障     TEST / RISK
│   ├── 06-plan/               计划跟踪     ROADMAP / OPEN-QUESTIONS
│   ├── 07-operations/         发布运维     OPS
│   ├── 08-reference/          参考资料     GLOSSARY / DISCUSSION-MAP / REFERENCES
│   └── 09-archive/            归档基线     chat.md / v2.0 合并文档
├── src/                       源码（M0 建立 Cargo Workspace，crates/ 按模块拆分）
├── tests/                     测试（unit / integration / e2e / fixtures / spikes）
│   ├── e2e/t0/                T0 双节点验收（21 条用例、CLI 契约、观测层、断言引擎、JSON 报告）
│   ├── fixtures/t0/           T0 确定性样本与 PROTO v1 golden 报文
│   └── spikes/                M0 Spike 实验包（判据、状态登记、就绪度检查）
├── scripts/                   工具脚本（build_docs.py 等）
├── config/                    配置样例（coldrevive.example.toml）
├── .github/workflows/         CI（文档构建与校验）
└── .workbuddy/memory/         工作区记忆（勿删）
```

> `src/` 目前只含目录说明，尚未建立可编译工程。按 [ROADMAP](docs/06-plan/ROADMAP.md) 的 M0 门禁，技术路线冻结后才创建 `Cargo.toml` 与 crate 骨架，避免提前固化未经 Spike 验证的选型。`tests/e2e/t0/` 是纯 Python 验收骨架，不依赖 Rust 工具链，也不改变这条门禁。

## 文档工作流

所有 `.md` 是**唯一事实来源**；`.html` 为构建产物，请勿直接编辑。

```bash
# 由 Markdown 生成各文档的 HTML 页面，并刷新 index.html 的内嵌数据
python3 scripts/build_docs.py

# 校验文档链接、front matter 与索引页数据块
python3 scripts/build_docs.py --check
```

浏览器直接打开 `docs/index.html` 即可离线浏览全套文档：全部正文与 Markdown 源码已内嵌在该页面内，点击「阅读」在页内阅读器打开，不依赖同目录的其他 HTML 文件，因此单文件分发或在预览沙箱中打开也能正常阅读。修改任一 `.md` 后重新运行构建脚本。

## T0 验收骨架自检

不依赖 Rust 工具链，仅用 Python 3.9+ 标准库：

```bash
python3 tests/e2e/t0/t0_accept.py --list        # 21 条用例与判据出处
python3 tests/e2e/t0/t0_accept.py --self-test   # 框架自检
python3 tests/e2e/t0/t0_harness.py --self-check # 环境与确定性样本自检
python3 tests/e2e/t0/t0_cli_selftest.py         # 观测层自检（证明不合成值）
python3 tests/spikes/m0_preflight.py             # M0 就绪度检查（退出码 2 = 有阻塞）
```

`--self-test` 的 PASS 只证明脚本编排与断言引擎正确，**不是 G1 门禁证据**（报告 `evidence_valid` 恒为 `false`）；`--mode real` 在 M1 二进制接线前一律 `BLOCKED`。完整说明见 [T0 骨架文档](tests/e2e/t0/README.md)。`m0_preflight.py` 只检查实验条件，不执行 Spike、不接触公共 KAD、不产出技术结论；见 [M0 实验包](tests/spikes/README.md)。

## 合规边界

本项目是通用 P2P 传输与索引协议的技术实现，不内置任何资源站点、索引库或默认搜索源，不提供绕过版权保护或匿名化的能力，用户对其分享与下载内容的合法性独立承担责任。详见 [OPS §合规与伦理边界](docs/07-operations/OPS.md)。
