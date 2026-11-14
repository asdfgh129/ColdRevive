# T0 双节点验收骨架（tests/e2e/t0/）

> **状态：M1 已接线，T0 真实链路已跑通（21/21 PASS、`evidence_valid=true`）。** 本目录提供 G1 门禁所需的用例清单、判据、编排、观测采集与报告格式。
> **stub 自检的全部 PASS 只证明框架本身正确，不构成任何门禁证据。** 报告中的 `evidence_valid: false` 是刻意设计，禁止手工改成 `true`。
> 接线进度：CLI v1 契约与 20 条真实观测映射已落地（`t0_cli.py` / `t0_cli_selftest.py`），`CliAdapter.wired = True`（代码层）；`t0_accept.py` 会在 real 模式下启动 A/B 两个 `coldrevived` 实例并等待健康检查。
> **环境级防呆**：缺任一二进制环境变量时 `--mode real` 仍 `BLOCKED` / 退出码 2（CI 依赖此行为）。
> **边界**：T0 只覆盖 `127.0.0.1` 单机回环；G1 还需要 [T1 双机 + 真实 NAT 验收](../t1/README.md)（harness 已落地、`--mode real` 因无第二台物理主机 `BLOCKED`）与 S3/S5/S6 必测回填，见 [G0-REVIEW §9–§10](../../../docs/06-plan/G0-REVIEW.md)。

## 1. 定位

T0 = 单机回环、两个守护进程实例（A 持有者 / B 请求方）不同端口，最快反馈环境，见 [TEST §2](../../../docs/05-quality/TEST.md)。它验证 M1 的唯一核心假设：

```
A 放入自制样本 → F1 入库/伴随造种/COLD 影子索引 → KAD 专属命名空间发布
    → B 关键词发现命中 A → B 发 WAKEUP_REQ → A 校验并 WAKEUP_ACK(magnet)
        → B 校验 InfoHash → 第三方 BT 客户端标准下载 → 逐片校验 → B 落盘触发 F1 闭环
```

按 [ROADMAP §2 M1](../../../docs/06-plan/ROADMAP.md)，MVP 刻意做小：下载用第三方客户端、magnet 可硬编码，**不做** Web UI / aria2 RPC / WebSocket / 内置引擎 / 跨协议补源。

## 2. 目录

```
tests/e2e/t0/
├── README.md            本文件：判据、运行方式、接线清单
├── t0_config.json       环境参数（端口 / 超时 / 阈值 / 样本 / 启动契约）
├── t0_cases.json        用例元数据（只定义「验什么」，断言实现在 t0_accept.py）
├── t0_harness.py        配置载入、端口预检、确定性样本、进程启停与清理
├── t0_adapters.py       观测契约 + StubAdapter / CliAdapter（20 个观测入口）
├── t0_cli.py            CLI v1 契约实现：真实 JSON 取数、字段映射、真实 InfoHash 计算
├── t0_cli_selftest.py   观测层自检（证明不合成值；非 G1 验收，不写报告）
├── t0_accept.py         验收入口：用例驱动、断言引擎、JSON 报告、退出码
├── .run/                运行态（节点日志、PID、临时配置），不入库
└── reports/             验收报告 JSON，不入库
tests/fixtures/t0/
├── README.md            测试数据约定
├── golden/              WAKEUP_REQ / WAKEUP_ACK v1 契约 golden
└── sample/              运行时生成的确定性样本，不入库
```

编排与断言层仅依赖 Python 3.9+ 标准库，不需要 `pip install`；real 模式的真实观测由
`Cargo Workspace` 编译出的 `coldrevived` / `coldrevive-cli` 提供（本目录不直接构建 Rust 工程）。

## 3. 运行

```bash
# 1) 列出 21 条用例及判据出处
python3 tests/e2e/t0/t0_accept.py --list

# 2) 框架自检（默认模式）：用合成观测跑通编排与全部断言，验证脚本本身正确
python3 tests/e2e/t0/t0_accept.py --self-test

# 3) 真实验收：先编译 M1 二进制，再显式指定三个真实文件路径
cargo build --workspace --offline
COLDREVIVE_DAEMON=/path/to/coldrevived \
COLDREVIVE_CLI=/path/to/coldrevive-cli \
T0_DOWNLOADER=/path/to/aria2c \
python3 tests/e2e/t0/t0_accept.py --mode real

# 4) 观测层自检：验证 t0_cli 能取真实值、缺字段不合成、缺二进制抛异常
python3 tests/e2e/t0/t0_cli_selftest.py

# 5) M0 就绪度检查：列出当前不能开始 S1/S2/T0 的阻塞项
python3 tests/spikes/m0_preflight.py
```

| 退出码 | 含义 |
| :--- | :--- |
| `0` | 本次执行全部用例通过（stub 自检通过也返回 0，但报告标记 `evidence_valid: false`） |
| `1` | 存在 `FAIL` 用例，或 mandatory 用例 `BLOCKED` |
| `2` | 环境未就绪 / real 模式未接线 / 配置或报告错误 |
| `3` | 报告写入失败 |

`--mode real` 会由 `setup_real_env()` 写双节点配置、按 `launch` 契约启动 A/B 守护进程并等待 `/health`；
`t0_accept.py` 结束时会 `stop_all()` 清理。三个环境变量缺失或指向的文件不存在时，一律
`BLOCKED` 并退出码 2，绝不静默降级为 stub。2026-11-13 实跑：**21/21 PASS、FAIL 0、
`evidence_valid=true`**（报告 `reports/t0-20261113T075155Z-real.json`），数据面由第三方
aria2 1.36.0 标准 BT 完成下载并逐片 SHA-1 校验。该结果仍只是 T0 证据，不等于 G1。

## 4. 用例与判据来源

用例编号 `T0-00`～`T0-20`，分五组：`framework`（断言引擎自检）、`chain`（主链路，G1 mandatory）、`negative`（故障注入）、`contract`（协议 golden）、`env`（最小暴露与端口隔离）。

| 组 | 判据来源 |
| :--- | :--- |
| 主链路 | [FLOWS §1/§3/§4/§7](../../../docs/03-design/FLOWS.md)、[ROADMAP M1/G1](../../../docs/06-plan/ROADMAP.md) |
| 协议与错误码 | [PROTO §3](../../../docs/04-protocol/PROTO.md)、[TEST §5](../../../docs/05-quality/TEST.md) |
| 时延与成功率 | [SRS NFR-04](../../../docs/01-product/SRS.md)、[TEST §4/§6](../../../docs/05-quality/TEST.md) |
| 配置与暴露面 | [CONFIG §3](../../../docs/04-protocol/CONFIG.md)、[config/coldrevive.example.toml](../../../config/coldrevive.example.toml) |

阈值**只在** `t0_config.json` 定义一次，取自上述文档：唤醒 P95 < 3000 ms、复活成功率 1.0、唤醒超时 1.5 s、采样数 30。禁止在脚本或用例里另写数字。

`t0_cases.json` 的 `source` 字段登记每条用例的文档出处；`ASSERTS` 表在 `t0_accept.py` 中按 `id` 对应。新增用例须同步两处，并先确认文档里有对应判据。

## 5. 接线清单（M0 冻结 / M1 开发时逐项完成）

当前 `CliAdapter.wired = True`（代码层已接线）；**环境级防呆**在 `observe()` 里动态校验三个二进制，
缺任一项抛 `NotWiredError`。状态标记：✅ 已落地 / ⏳ 待真实环境（G1 出口）/ ⛔ 必须等 G1+M1 出口条件。

| # | 动作 | 状态 | 说明 |
| :--- | :--- | :--- | :--- |
| 1 | **S2 先行**：确认公共 KAD 接纳 `ColdRevive_V1:` 命名空间 | ✅ | S2 已判 `pass`（实网 `PUBLISH_RES` / `SEARCH_RES` + 双进程 A/B 取回）。T0 仍只用本地双节点私有 KAD 子网，**不得向公共网络发布测试数据** |
| 2 | **S1 并行**：`rqbit` 按需实例化 / Pause-Resume / 握手拦截 | ✅ | S1 已判 `pass`（two-peer 4/4 verified + 第三方 aria2 128/128 片校验）；M1 数据面由第三方 aria2 承担，内置引擎属 M2 |
| 3 | 定稿 CLI 启动参数，替换 `launch` 占位契约 | ✅ | `coldrevive.t0.cli-contract/v1`（`t0_config.json` + §5.1），M1 二进制已按此实现 |
| 4 | 实现 `observe_t0_01`～`observe_t0_20` | ✅ | 20 条映射在 `t0_cli.OBSERVE_PLAN`；`t0_cli_selftest.py` 验证其不合成值 |
| 5 | 把 `infohash_stub` 换成伴随 `.torrent` 的真实 InfoHash | ✅ | `t0_cli.torrent_infohash()` 按 BT 规范对 info 字典原始字节做 SHA-1；M1 生成真实种子，T0-06 已用真值 |
| 6 | 接入第三方下载器 | ✅ | 本机 Motrix Next 捆绑 `motrixnext-aria2c.exe`（aria2 1.36.0），由 `T0_DOWNLOADER` 指定；G3 矩阵仍未锁定版本 |
| 7 | 指标来自真实 `trace_id` 埋点，不接受 stub 数组 | ✅ | `metrics wake-latency/revival` 由 B 节点发起真实 UDP 唤醒往返采样（30 次），不读合成数组 |
| 8 | 编译 M1 二进制并置 `wired = True`，同步改 CI | ✅ | 3 个二进制由 `cargo build --workspace --offline` 产出；CI 新增 workspace 单测并保留「无二进制时 real 必 BLOCKED」防呆 |
| 9 | 先跑 T0，再跑 T1（局域网双机、真实 NAT） | ✅ | T0 21/21 PASS（2026-11-14 M2 接线后回归仍 21/21）；[T1 harness](../t1/README.md) 于 2026-11-14 在双物理主机上 `--mode real --refresh-nat` **11/11 PASS、`evidence_valid=true`**。两者共同构成 **G1 通过**，`m2_gate` 解除。〔历史：2026-11-13 无第二台物理主机时 `BLOCKED`、M1 缺 NAT 穿透栈；后者接线层已由 M2/W3.3 关闭〕 |
| 10 | S3 / S5 / S6 必测回填 | ⛔ | 见 [G0-REVIEW §9](../../../docs/06-plan/G0-REVIEW.md)；三者 `status` 仍 `not_run`，不虚判 |

随时可用 `python3 tests/spikes/m0_preflight.py` 复查上面哪些条件已齐备。该脚本**只查条件、不做实验、不产出 Spike 结论**。

### 5.1 CLI v1 契约（proposed-v1）

调用形式与命令集定义在 `t0_config.json` 的 `launch` 段，实现在 `t0_cli.py`：

```
coldrevive-cli --json --node <A|B> <command> [args...]   → stdout 单个 JSON 对象
GET http://<bind>:<http_port>/health                    → 200
```

| 命令 | 用途 | 被哪些用例使用 |
| :--- | :--- | :--- |
| `inspect file/index/kad/state/torrent <hash>` | 入库、影子索引、KAD 发布、状态跃迁、伴随种子路径 | T0-01/02/03/07/06 |
| `find <keyword>` | F2 关键词发现 | T0-03 |
| `wake probe/ack <hash> <infohash>` | 唤醒请求与 ACK 校验 | T0-04/05/06 |
| `scenario <name> <hash>` | 8 类故障注入场景 | T0-09、T0-12～18 |
| `metrics wake-latency/revival <n>` | 真实时延与复活成功率采样 | T0-10/11 |
| `download <magnet>` | 第三方客户端标准数据面下载结果 | T0-08 |
| `golden verify v1` · `env defaults` | 契约与最小暴露 | T0-19/20 |

**契约红线**：CLI 非 0 退出、非法 JSON、字段缺失都判「观测不可用」→ `BLOCKED` / 断言 FAIL，**绝不回填默认值**。少量字段（`fixture_sha256`、`bind_loopback`、`ports_distinct`、golden 校验、伴随种子 InfoHash）由本机真实文件与配置独立计算，避免产品自己证明自己。契约变更须同步三处：`launch`、`OBSERVE_PLAN`、本小节，并经 G0 评审。

## 6. 报告格式

每次执行在 `reports/t0-<UTC时间>-<mode>.json` 落盘，字段：

| 字段 | 说明 |
| :--- | :--- |
| `mode` / `adapter` | `stub` 或 `real`；决定结果是否可作证据 |
| `evidence_valid` | 仅 `real` + `WIRED=True` + mandatory 全 PASS 时为 `true` |
| `summary` | total / passed / failed / blocked / skipped / mandatory_failed |
| `cases[].assertions[]` | 每条断言的 label / actual / expect / ok，可追溯 |
| `fixture` | 样本 SHA-256 与 Key，用于复现 |
| `next_actions` | 未通过项对应的后续动作 |

按 [TEST §7](../../../docs/05-quality/TEST.md)，G1 交付物还需附指标快照与未通过项豁免说明；本脚本只产出机器可读证据，人工评审结论另附。

## 7. 红线

- 样本文件自制、确定性、无版权内容；`sample/` 与 `.run/` 均由 `.gitignore` 排除。
- T0 只在 `127.0.0.1` 运行，除 S2 专项外不接触公共 KAD，避免测试流量污染公共网络。
- 不伪造通过：stub 结果永远标记 `evidence_valid: false`；未接线的 real 模式永远 `BLOCKED`。
- 不把 [REFERENCES 中 V-01～V-09](../../../docs/08-reference/REFERENCES.md) 未经核实的第三方数据写进阈值。
