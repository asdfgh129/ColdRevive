# 变更记录（Changelog）

本文件记录**文档体系与项目决策**的变更。格式参考 Keep a Changelog，版本与文档 front matter 中的 `version` 对应。

## [2.4.1] - 2026-11-09 — 托管到 GitHub 并发布在线文档站

### 变更

- 建立 Git 仓库（默认分支 `main`），推送至 **https://github.com/asdfgh129/ColdRevive**（公开）。
- 新增 `.github/workflows/pages.yml`：安装 `markdown` → 运行 `docs` 构建器 → 校验 `index.html` 与源码同步 → 上传 `docs/` 为 Pages 产物 → 部署。仓库 Pages 设为 `build_type=workflow`，在线入口 **https://asdfgh129.github.io/ColdRevive/**。
- `.gitignore` 增加 `.workbuddy/`：本地智能体工作记忆不进入公开仓库。
- 根 `README.md` 增加代码仓库与在线文档入口；重跑 `scripts/build_docs.py` 同步 `docs/index.html` 内嵌内容。

### 说明

- 本版本只做托管与发布，**不改变任何技术设计或验收状态**：S1–S8 仍为 `not_run`、`CliAdapter.wired` 仍为 `False`、T0 stub 自检仍不构成 G1 门禁证据。
- 提交身份使用 GitHub noreply 邮箱，避免在公开提交历史中暴露个人邮箱。

## [2.4.0] - 2026-11-09 — 推进 T0 接线：CLI 契约、观测层与 M0 实验包

### 变更

- 新增 `tests/spikes/`（M0 实验包）：`spikes.json` 登记 S1–S8 判据、优先级、失败替代与状态（当前全部 `not_run`）；`README.md` 给出 S1/S2 实验步骤、合规红线与结论回填规则；`m0_preflight.py` 可复跑地检查实验前置条件，输出机器可读报告。
- **定稿 CLI v1 契约**（`proposed-v1`）：`t0_config.json` 的 `launch` 由占位改为 `coldrevive.t0.cli-contract/v1`，定义 `inspect / find / wake / scenario / metrics / download / golden / env` 八个命令与健康检查；契约说明见 [T0 骨架 §5.1](tests/e2e/t0/README.md)。
- 新增 `tests/e2e/t0/t0_cli.py`：20 条用例的真实取数映射 `OBSERVE_PLAN`，CLI 调用失败 / 非法 JSON / 字段缺失一律判「观测不可用」，**绝不合成观测值**；`torrent_infohash()` 按 BT 规范从伴随 `.torrent` 的 info 字典原始字节计算真实 InfoHash，替换骨架期的 `infohash_stub`。
- `CliAdapter` 接入观测层并显式绑定 20 个 `observe_t0_XX` 入口；`preflight()` 改为输出真实阻塞项。**`wired` 仍为 `False`**，`--mode real` 继续 `BLOCKED`。
- 新增 `tests/e2e/t0/t0_cli_selftest.py`（6 项观测层自检）与 `.github/workflows/t0.yml` 对应步骤；CI 保留「Real mode stays BLOCKED」断言。
- 文档同步：[TEST §2.1](docs/05-quality/TEST.md)（v2.2）、[ROADMAP §6](docs/06-plan/ROADMAP.md)（v2.2）、`docs/README.md`（v2.4）、`tests/README.md`、`scripts/README.md`、根 `README.md`；`build_docs.py` 的 `EMBED_EXTRA` 纳入 `tests/spikes/README.md`（7 → 8 个工程文件，内嵌项 40 → 41）。

### 验证

- `m0_preflight.py` 实测：阻塞 6 项 → 落地观测层后 5 项；`rustc` 与 T0 端口可用，`cargo`、第三方 BT 客户端、M1 二进制、S2 实网批准、`wired` 未就绪，S8 法务评审为外部提醒。
- `t0_cli_selftest.py`：6/6 通过（真实 InfoHash 计算、CLI 取数与字段映射、缺字段不合成、缺二进制抛异常、20 条映射齐全、`wired` 仍为 `False`）。
- `t0_accept.py --self-test` 仍 21/21 PASS 且 `evidence_valid: false`；`--mode real` 仍 1 PASS / 20 BLOCKED、退出码 2，`next_actions` 现已如实反映「观测层已实现、缺二进制与真实环境」。
- `build_docs.py --check` 通过；两次构建字节一致；索引页全量链接解析失效 0。

### 不变

- **技术路线零改动**：不创建 `Cargo.toml`、不碰 `src/`；S1/S2/S8 实验**未执行**，不产出任何 Spike 结论，不接触公共 KAD。
- T0 仍不构成 G1 门禁证据；`Cargo.toml` 与 crate 骨架继续等 G0 通过后创建。

## [2.3.0] - 2026-11-09 — 搭建 T0 双节点验收脚本骨架

### 变更

- 新增 `tests/e2e/t0/`：T0 单机回环双节点验收骨架，含 `README.md`（判据/接线清单）、`t0_config.json`（环境、阈值、启动契约）、`t0_cases.json`（21 条用例元数据）、`t0_harness.py`（端口预检、确定性样本、进程启停与清理）、`t0_adapters.py`（观测契约与 Stub/Cli 适配器）、`t0_accept.py`（断言引擎、JSON 报告、退出码）。
- 新增 `tests/fixtures/t0/`：`WAKEUP_REQ` / `WAKEUP_ACK` v1 契约 golden、确定性样本生成约定；运行产物由 `.gitignore` 排除。
- 用例 `T0-00`～`T0-20` 覆盖 F1→F2→F3→F4 主链路、故障注入（8 类错误码）、协议 golden、最小暴露与端口隔离；判据出处逐条登记在 `t0_cases.json` 的 `source` 字段，阈值只在 `t0_config.json` 定义一次。
- **防呆设计**：stub 自检报告的 `evidence_valid` 恒为 `false`；`CliAdapter.wired = False` 时 `--mode real` 把所有真实用例标为 `BLOCKED` 并以退出码 2 结束，杜绝「没跑却显示绿」。`T0-00` 通过注入坏观测验证断言引擎能正确判 FAIL。
- 文档同步：[TEST §2.1](docs/05-quality/TEST.md) 新增 T0 骨架章节、[ROADMAP §6](docs/06-plan/ROADMAP.md) 修正状态滞后（v2.1 拆分 / M0 未开始 → v2.3 + T0 骨架已就位）、`tests/README.md` 修正「TEST §9.2」错误引用为 §2、根 `README.md` 增加自检命令、`docs/README.md` 增加「docs/ 外被引用文件须登记 `EMBED_EXTRA`」规则。
- `scripts/build_docs.py` 的 `EMBED_EXTRA` 纳入 `tests/e2e/t0/README.md`（7 个工程文件），保证自包含索引页里指向骨架的链接可就地打开；索引页版本标识同步 v2.3。
- 新增 `.github/workflows/t0.yml`：CI 运行 T0 框架自检与文档校验。

### 验证

- `py_compile` 三个脚本通过；`t0_harness.py --self-check` 通过（样本 SHA-256 复算一致、3 个 Keyword_Key ≤ 8、两份 golden 齐全）。
- `--self-test`：21/21 PASS，退出码 0，`evidence_valid: false`；把 P95 阈值临时改为 100 ms 后 `T0-10` 正确 FAIL 且退出码 1，证明阈值取自配置、断言生效。
- `--mode real`（未接线）：1 PASS / 20 BLOCKED，退出码 2，`next_actions` 列出 20 个待实现观测与 3 个未设环境变量。

### 不变

- **技术路线零改动**：不创建 `Cargo.toml`、不碰 `src/`、不提前固化 BT 引擎或 KAD 方案；M0 门禁与 G0～G4 判据保持原样。
- T0 骨架**不构成 G1 门禁证据**；S1/S2 Spike 与法务评审仍是 M0 的起跑线。

## [2.2.0] - 2026-11-09 — 修复索引页阅读链接失效

### 问题

在预览面板 / `srcdoc` 沙箱中只承载 `docs/index.html` 单个文件，卡片上的「阅读」链接指向同目录的 `01-product/PRD.html` 等文件，无法解析，打开后显示 *no data available*。

### 变更

- `docs/index.html` 改为**自包含单文件门户**：新增页内阅读器（文档 / MD 源码双视图、跨文档跳转、Esc 关闭、独立页面可选出口）。
- 全部站内链接在运行时改为 `data-href` 并移除 `href`，从根上避免任何预览框架拦截锚点后的外部导航；外链与页内 `#` 锚点保持原样。
- `scripts/build_docs.py` 新增 `DOCS_PAYLOAD` 注入：把 33 份文档正文 + Markdown 源码，以及 `docs/` 外被引用的 6 个工程文件（`README.md`、`CHANGELOG.md`、`src/tests/scripts/README.md`、`config/coldrevive.example.toml`）共 39 项写入 `index.html`；索引页只替换标记之间的数据块，人工维护部分不被覆盖。
- 抽出共用的 `render_body()`，保证独立 HTML 页面与内嵌阅读器渲染结果一致。
- `--check` 增加索引页 `DOCS_PAYLOAD` 标记校验；同步更新 `README.md`、`docs/README.md`、`scripts/README.md` 的构建说明。

### 验证

- 构建：`--check` 通过；内嵌 39 份文档，生成 33 个独立 HTML 页面。
- 链接：用页面实际的 `normKey()` 校验 79 个索引/卡片链接 + 465 个正文链接，**544 个全部命中内嵌目标**。
- 脚本：`node --check` 通过；`armLinks()` 运行时行为（内部链接去除 `href`、中文路径解析）已抽样验证。

### 不变

- 技术内容、设计决策、指标与里程碑零改动；仅改变文档的分发与阅读方式。

## [2.1.0] - 2026-11-09 — 文档工程化重构

### 变更

- 将单文件 `ColdRevive-项目开发文档-v2.0.md`（1193 行）按软件工程规范拆分为 20+ 份独立文档，分入 8 个类别目录。
- 建立完整项目层级目录：`src/` `tests/` `scripts/` `config/` `.github/workflows/` `docs/`。
- 新增 `docs/index.html` 离线文档索引门户（分类导航、搜索、角色阅读路径、状态徽标）。
- 新增 `scripts/build_docs.py`：Markdown 单一事实来源 → 批量生成 HTML 页面，支持 `--check` 校验。
- 新增 `docs/02-architecture/adr/`：10 份独立架构决策记录（ADR），一份一议题。
- 新增 `config/coldrevive.example.toml` 可执行配置样例，与 CONFIG.md 规范对应。
- 原始 `chat.md` 与 v2.0 合并文档移入 `docs/09-archive/`，标记为已废止的历史基线。

### 不变

- 技术内容、设计决策、指标与里程碑相对 v2.0 **零改动**，仅做结构化拆分与归位。
- 所有已冻结决策（DR-01～DR-08、ADR-003/004/006/007/008/009/010）保持有效。

## [2.0.0] - 2026-11-09 — 设计收敛

基于 `chat.md` 全部讨论轮次，取代其中的 v1.0 草案。主要修订：

1. 确立「常态共享目录（体验层）+ 按需唤醒（实现层）」双层模型。
2. 补充伴随种子生成，解决 BT 基于种子而非文件的硬壁垒。
3. 增加冷热分级、LRU 活跃池、影子索引、握手拦截与资源阈值治理。
4. 分层 DHT：KAD 发现 / 私有 RPC 激活 / Mainline DHT 兼容 / Tracker 可选。
5. 扩充威胁模型、Challenge-Response、限流与信誉设计。
6. 纳入 hMule 失败复盘，将冷启动网络效应列为头号风险。
7. 吸收 aria2 预研结论：守护进程内嵌 HTTP/WS、WebSocket 替代轮询、兼容 aria2 JSON-RPC。
8. 细化 WBS、M0–M5 里程碑、质量门禁、测试矩阵与风险登记册。

## [1.0.0] - 讨论记录内嵌草案

`chat.md` 第 584–739 行的《ColdRevive 客户端开发设计文档 v1.0》，首次提出 KAD 黄页 + 私有唤醒 RPC + 标准数据管道架构。现仅作历史参考。
