# ColdRevive

> 把文件放进共享目录，它就永远能被搜到、被下到。

一个**不修改任何现有 P2P 协议**的混合下载客户端：以 eMule KAD 为去中心化黄页做发现，以轻量私有唤醒信令激活在线持有者临时做种，以标准 BitTorrent / eD2K 协议传输数据。对旧客户端完全透明。

| 项 | 值 |
| :--- | :--- |
| 当前阶段 | **M2 进行中 / W3.3 连通性**：G1 已通过（2026-11-14），M2 门禁解除并已开工；**G2 未宣称达标** |
| 代码仓库 | **https://github.com/asdfgh129/ColdRevive**（公开） |
| 在线文档 | **https://asdfgh129.github.io/ColdRevive/**（GitHub Pages 自动部署） |
| 文档版本 | v2.4.48（门禁口径全量收尾 + T2 编号消歧 + 文档站重建）；上一版 v2.4.47 = M2/W3.3 cr-nat 接入复活路径 + T2 本地接线验收 |
| M1 / G1 | T0 `--mode real` **21/21 PASS**（2026-11-14 复跑回归仍 21/21）；T1 `--mode real` 双机（A=192.168.1.210 / B=192.168.1.209）**11/11 PASS、`evidence_valid=true`**（2026-11-14）；两者共同构成 **G1 通过**证据 |
| M2 状态 | **已开工**：`cr-nat` 已接入 `coldrevived` 的跨 NAT UDP 唤醒（候选计划 + 单 socket 打洞 + 中继回退）与 tracker peer 地址协商；新增 `nat` CLI 子命令与 `/nat/advertise`。[T2 本地接线验收](tests/e2e/t2/README.md) `10/10 PASS`、`evidence_valid=false`。**S6.status 仍 `not_run`，不宣称 G2 达标**（四类 NAT 含 CGNAT 实测须真实多 NAT 环境） |
| 代码基线 | 根 `Cargo.toml` + 17 crate + 4 bin，std-only 零第三方依赖；`cargo test --workspace --offline` **74 passed / 0 failed**；PROTOCOL_VERSION 仍为 1、T0 golden 未动 |
| S3 / S5 / S6 | 均保持 `not_run`（G0 条件冻结的 M1/M2 必测回填项）：S3 待真实双栈集成 + G3 矩阵；S5 待真实拥塞链路 ≤5% 实测；S6 待 M2 四类 NAT 可达率实测 |
| 验收骨架 | [T0](tests/e2e/t0/README.md) 21/21 real · [T1](tests/e2e/t1/README.md) 11/11 real · [T2](tests/e2e/t2/README.md) 10/10 本地预检（非 S6 证据）；`--self-test` 恒 `evidence_valid=false` |
| M0 / 门禁 | [G0 条件冻结](docs/06-plan/G0-REVIEW.md)：S1/S2/S4/S7/S8 `pass`，S3/S5/S6 `not_run`；机读进度 `tests/spikes/spikes.json` 的 `m1_progress` / `m2_progress` |
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
├── src/crates/                17 个 crate（M1 七个真实实现 + M2/M3/M4 骨架；cr-nat 已落地穿透栈，余 M2 crate 仍骨架）
├── src/bins/                  coldrevived / coldrevive-cli / coldrevive-agent / coldrevive-nat（NAT 穿透诊断，M2 合法前置）
├── tests/                     测试（unit / integration / e2e / fixtures / spikes）
│   ├── e2e/t0/                T0 双节点验收（21 条用例、CLI 契约、观测层、断言引擎、JSON 报告）
│   ├── e2e/t1/                T1 双机 + 真实 NAT 验收（10 用例 / 40 断言、可达性探针、双机启动契约）
│   ├── fixtures/t0/           T0 确定性样本与 PROTO v1 golden 报文
│   └── spikes/                M0 Spike 实验包（判据、状态登记、就绪度检查；s5/utp 为真实 uTP 栈量测）
├── scripts/                   工具脚本（build_docs.py 等）
├── config/                    配置样例（coldrevive.example.toml）
├── .github/workflows/         CI（文档构建与校验）
└── .workbuddy/memory/         工作区记忆（勿删）
```

> [G0 条件冻结](docs/06-plan/G0-REVIEW.md) 已于 2026-11-13 生效；**G1 于 2026-11-14 通过**
> （T0 21/21 + T1 real 11/11 均 `evidence_valid=true`），**M2 门禁解除并已开工**。
> 当前 M2/W3.3 已完成 cr-nat 接入（T2 本地预检 10/10，非 S6 证据）；
> **S3 / S5 / S6 仍 `not_run`，G2 未宣称达标**，四类 NAT 矩阵须真实多 NAT 环境实测。
> `tests/e2e/t0/` / `t1/` / `t2/` 的编排层是纯 Python（不依赖 Rust 工具链），真实观测由 `coldrevive-cli` 提供。

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
python3 tests/e2e/t1/t1_accept.py --list        # T1 的 10 条用例 / 40 条断言
python3 tests/e2e/t1/t1_accept.py --self-test   # T1 编排与断言引擎自检（evidence_valid=false）
python3 tests/e2e/t1/t1_netprobe.py             # 本机跨 IP UDP 可达性真实探测
python3 tests/e2e/t2/t2_nat_path.py --self-test # T2 mock STUN / 中继 wire 自检
python3 tests/e2e/t2/t2_nat_path.py --mode local # M2 跨 NAT 接线本地预检（evidence_valid=false）
python3 tests/spikes/m0_preflight.py             # 就绪度检查（退出码 2 = 有阻塞）
cargo test --workspace --offline                # 74 个 Rust 单测（零第三方依赖）
```

真实链路（M1 二进制 + 第三方 BT 下载器，Windows 示例）：

```bash
cargo build --workspace --offline
export COLDREVIVE_DAEMON="C:/path/to/ColdRevive/target/debug/coldrevived.exe"
export COLDREVIVE_CLI="C:/path/to/ColdRevive/target/debug/coldrevive-cli.exe"
export T0_DOWNLOADER="C:/Program Files/Rayburst/aria2-next.exe"
python3 tests/e2e/t0/t0_accept.py --mode real
```

2026-11-14 复跑回归：**21/21 PASS、`evidence_valid=true`**（含第三方 aria2-next 3.1.0 标准 BT 下载与逐片 SHA-1 校验）。
未设置三个环境变量时 `--mode real` 仍 `BLOCKED` / 退出码 2（CI 依赖此防呆）。
v2.4.47 修正了 T0 观测客户端对 `download` 的 10 s 超时：daemon 内部上限为 120 s，现按条用例放宽到 190 s，避免新版下载器稍慢即被误判 `BLOCKED`。

T1 单机预检（同样需要三个二进制环境变量）：

```bash
python3 tests/e2e/t1/t1_accept.py --lan-precheck --address-mode aliases  # 两个回环别名，验证按节点 host 寻址
python3 tests/e2e/t1/t1_accept.py --lan-precheck --address-mode nic      # 真实物理网卡（非回环）
```

历史单机预检为各 **10 PASS / 1 SKIP**、`evidence_valid=false`；
**2026-11-14 已用两台真实物理主机跑通 `--mode real`：11/11 PASS、`evidence_valid=true`，与 T0 共同构成 G1 通过证据**。
后续复跑仍需两台物理主机与 B 端预启动守护进程，命令见 [T1 README §4](tests/e2e/t1/README.md)。

`--self-test` 的 PASS 只证明脚本编排与断言引擎正确，**不是 G1 门禁证据**（`evidence_valid` 恒为 `false`）；
T2 的 10/10 PASS 同理，只证明 M2 接线，不是 S6 四类 NAT 实测。
完整说明见 [T0 骨架文档](tests/e2e/t0/README.md) · [T2 预检文档](tests/e2e/t2/README.md) · [G0-REVIEW §9](docs/06-plan/G0-REVIEW.md)。
`m0_preflight.py` 只检查实验条件，不执行 Spike、不接触公共 KAD、不产出技术结论；见 [M0 实验包](tests/spikes/README.md)。

## 合规边界

本项目是通用 P2P 传输与索引协议的技术实现，不内置任何资源站点、索引库或默认搜索源，不提供绕过版权保护或匿名化的能力，用户对其分享与下载内容的合法性独立承担责任。详见 [OPS §合规与伦理边界](docs/07-operations/OPS.md)。
