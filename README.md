# ColdRevive

> 把文件放进共享目录，它就永远能被搜到、被下到。

一个**不修改任何现有 P2P 协议**的混合下载客户端：以 eMule KAD 为去中心化黄页做发现，以轻量私有唤醒信令激活在线持有者临时做种，以标准 BitTorrent / eD2K 协议传输数据。对旧客户端完全透明。

| 项 | 值 |
| :--- | :--- |
| 当前阶段 | **M1 进行中**（CLI MVP 已跑通 T0 真实链路）；**G1 未通过、M2 受门禁阻塞** |
| 代码仓库 | **https://github.com/asdfgh129/ColdRevive**（公开） |
| 在线文档 | **https://asdfgh129.github.io/ColdRevive/**（GitHub Pages 自动部署） |
| 文档版本 | v2.4.38（M1 工程接线 + T0 real 验收 + G1/M2 门禁边界） |
| M1 状态 | 根 `Cargo.toml` + 17 crate + 3 bin（std-only，`cargo test --workspace --offline` 27/27）；`coldrevived` / `coldrevive-cli` 已按 CLI v1 契约实现；**T0 `--mode real` 21/21 PASS、`evidence_valid=true`**（报告 `tests/e2e/t0/reports/t0-20261113T075155Z-real.json`），数据面由第三方 aria2 真实完成 BT 下载与逐片 SHA-1 校验 |
| M2 状态 | **未开工（`blocked_by_G1`）**：G1 还缺 T1 局域网双机 + 真实 NAT，以及 S3 真实双栈 / S5 真实拥塞链路 uTP 实测；本轮仅建 M2 crate 骨架、按 S5 实测把 `background_upload_ratio` 默认回调为 `0.02`、登记 S6 四类 NAT 矩阵，均不宣称 G2 达标 |
| 验收骨架 | [T0 双节点验收](tests/e2e/t0/README.md)：CLI v1 契约 + 20 条真实观测映射已接线，**real 模式 21/21 PASS、`evidence_valid=true`**；`--self-test` 的 PASS 仍非 G1 证据 |
| M0 / 门禁 | [G0 条件冻结](docs/06-plan/G0-REVIEW.md) 已生效：S1/S2/S4/S7/S8 `pass`，S3/S5/S6 保持 `not_run`（M1/M2 必测回填）；[Spike 实验包](tests/spikes/README.md) · [S2 实网排期](tests/spikes/s2/PLAN.md) · 机读进度 `spikes.json` 的 `m1_progress` |
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
├── Cargo.toml / Cargo.lock    Workspace 根清单（M1 建立，std-only 零第三方依赖）
├── src/crates/                17 个 crate（M1 七个真实实现 + M2/M3/M4 骨架）
├── src/bins/                  coldrevived / coldrevive-cli / coldrevive-agent
├── tests/                     测试（unit / integration / e2e / fixtures / spikes）
│   ├── e2e/t0/                T0 双节点验收（21 条用例、CLI 契约、观测层、断言引擎、JSON 报告）
│   ├── fixtures/t0/           T0 确定性样本与 PROTO v1 golden 报文
│   └── spikes/                M0 Spike 实验包（判据、状态登记、就绪度检查）
├── scripts/                   工具脚本（build_docs.py 等）
├── config/                    配置样例（coldrevive.example.toml）
├── .github/workflows/         CI（文档构建与校验）
└── .workbuddy/memory/         工作区记忆（勿删）
```

> [G0 条件冻结](docs/06-plan/G0-REVIEW.md) 已于 2026-11-13 生效，M1 据此建立可编译的 Cargo Workspace 与生产二进制；
> `CliAdapter` 代码层已接线并保留环境级防呆。**T0 真实链路已通过，但 G1 仍缺 T1 与 S3/S5/S6 必测回填，M2 未开工。**
> `tests/e2e/t0/` 的编排层仍是纯 Python（不依赖 Rust 工具链），真实观测由 `coldrevive-cli` 提供。

## 文档工作流

所有 `.md` 是**唯一事实来源**；`.html` 为构建产物，请勿直接编辑。

```bash
# 由 Markdown 生成各文档的 HTML 页面，并刷新 index.html 的内嵌数据
python3 scripts/build_docs.py

# 校验文档链接、front matter 与索引页数据块
python3 scripts/build_docs.py --check
```

浏览器直接打开 `docs/index.html` 即可离线浏览全套文档：全部正文与 Markdown 源码已内嵌在该页面内，点击「阅读」在页内阅读器打开，不依赖同目录的其他 HTML 文件，因此单文件分发或在预览沙箱中打开也能正常阅读。修改任一 `.md` 后重新运行构建脚本。

## T0 验收：自检与真实链路

自检不依赖 Rust 工具链，仅用 Python 3.9+ 标准库：

```bash
python3 tests/e2e/t0/t0_accept.py --list        # 21 条用例与判据出处
python3 tests/e2e/t0/t0_accept.py --self-test   # 框架自检
python3 tests/e2e/t0/t0_harness.py --self-check # 环境与确定性样本自检
python3 tests/e2e/t0/t0_cli_selftest.py         # 观测层自检（不合成值 + 环境级防呆）
python3 tests/spikes/m0_preflight.py             # 就绪度检查（退出码 2 = 有阻塞）
cargo test --workspace --offline                # 27 个 Rust 单测（零第三方依赖）
```

真实链路（M1 二进制 + 第三方 BT 下载器，Windows 示例）：

```bash
cargo build --workspace --offline
export COLDREVIVE_DAEMON="C:/path/to/ColdRevive/target/debug/coldrevived.exe"
export COLDREVIVE_CLI="C:/path/to/ColdRevive/target/debug/coldrevive-cli.exe"
export T0_DOWNLOADER="C:/Program Files/MotrixNext/motrixnext-aria2c.exe"
python3 tests/e2e/t0/t0_accept.py --mode real
```

2026-11-13 实跑结果：**21/21 PASS、`evidence_valid=true`**（含第三方 aria2 标准 BT 下载与逐片 SHA-1 校验）。
未设置三个环境变量时 `--mode real` 仍 `BLOCKED` / 退出码 2（CI 依赖此防呆）。

`--self-test` 的 PASS 只证明脚本编排与断言引擎正确，**不是 G1 门禁证据**（报告 `evidence_valid` 恒为 `false`）；
`--mode real` 的 T0 结果也**只覆盖 T0 单机回环**，G1 还需 T1 局域网双机与 S3/S5/S6 必测回填。
完整说明见 [T0 骨架文档](tests/e2e/t0/README.md) 与 [G0-REVIEW §9](docs/06-plan/G0-REVIEW.md)。
`m0_preflight.py` 只检查实验条件，不执行 Spike、不接触公共 KAD、不产出技术结论；见 [M0 实验包](tests/spikes/README.md)。

## 合规边界

本项目是通用 P2P 传输与索引协议的技术实现，不内置任何资源站点、索引库或默认搜索源，不提供绕过版权保护或匿名化的能力，用户对其分享与下载内容的合法性独立承担责任。详见 [OPS §合规与伦理边界](docs/07-operations/OPS.md)。
