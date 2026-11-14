# 测试目录（tests/）

> 测试策略、矩阵与门禁见 [TEST 测试计划](../docs/05-quality/TEST.md)。Cargo Workspace 现有 80 个内联 Rust 单测；
> T0 真实链路 21/21、T1 `--mode real` 双物理主机 11/11（均 `evidence_valid=true`）→ **G1 于 2026-11-14 通过**；
> M2 门禁解除并已开工，新增 T2 本地接线验收 10/10（`evidence_valid=false`，非 S6/G2 证据）。
> 〔2026-11-13 历史口径：T1 真实验收 `BLOCKED`、G1 未通过、M2 `blocked_by_G1`，已被下方更正说明取代。〕

## 目录划分

| 目录 | 用途 | 对应测试层 |
| :--- | :--- | :--- |
| `unit/` | 各 crate 内联单测的补充与共享测试工具 | 单元测试 |
| `integration/` | 索引 → 造种 → 入库 → KAD 发布 → 唤醒 → 下载闭环 | 集成测试 |
| `e2e/` | 双节点 / 多节点真实网络场景、旧客户端兼容性矩阵 | 端到端与兼容性 |
| `e2e/t0/` | **T0 单机回环双节点验收**：用例清单、环境编排、观测适配器、CLI 契约、断言引擎、JSON 报告 | 端到端（G1 门禁证据） |
| `e2e/t1/` | **T1 双机 + 真实 NAT 验收**：跨主机启动契约、`t1_netprobe.py` 可达性探针；2026-11-14 双物理主机 `--mode real` **11/11 PASS** | 端到端（**G1 门禁证据，已取得**） |
| `e2e/t2/` | **M2/W3.3–W3.4 本地接线验收**：10 用例，同机双进程 + mock STUN + **真实 `coldrevive-relay` 服务端**（W3.4 起），验证 `cr-nat` 是否真正接入复活路径 | 接线预检（`evidence_valid=false`，**非 S6 / G2 证据**） |
| `spikes/` | **M0 Spike 实验包**：S1–S8 判据与状态、`m0_preflight.py` 就绪度检查、结论回填规则 | 选型验证（G0 门禁） |
| `fixtures/` | 固定种子文件、样本文件、golden 协议报文、`T0～T3` 环境配置 | 测试数据 |
| `fixtures/t0/` | **T0 确定性样本、PROTO v1 唤醒报文 golden** | 测试数据 |

## T0 验收骨架

入口说明：[tests/e2e/t0/README.md](e2e/t0/README.md)（判据来源、运行方式、报告格式、接线清单）。

```bash
# Rust 单测（std-only，离线可复现）
cargo test --workspace --offline

# 列出 21 条用例及判据出处
python3 tests/e2e/t0/t0_accept.py --list

# 框架自检：用合成观测跑通编排与全部断言（仅证明脚本正确）
python3 tests/e2e/t0/t0_accept.py --self-test

# 环境与确定性样本自检
python3 tests/e2e/t0/t0_harness.py --self-check

# 观测采集层自检（验证不合成观测值 + 环境级防呆，非 G1 验收）
python3 tests/e2e/t0/t0_cli_selftest.py

# 真实验收（需先 cargo build，并设三个二进制环境变量，见骨架 README §3）
python3 tests/e2e/t0/t0_accept.py --mode real
```

**当前状态：T0 真实链路 21/21 PASS、`evidence_valid=true`（2026-11-13；2026-11-14 M2 接线后回归仍 21/21）。**
`CliAdapter.wired = True`，但每次 real 运行都动态校验 `COLDREVIVE_DAEMON` / `COLDREVIVE_CLI` /
`T0_DOWNLOADER` 指向真实文件；缺任一项仍把用例标记为 `BLOCKED` 并以退出码 2 结束（CI 依赖此防呆）。
stub 自检报告的 `evidence_valid` 恒为 `false`，不得当作 G1 证据。
**边界更正（2026-11-14）**：原文「G1 还需 T1 与 S3 / S5 / S6 必测回填，M2 仍 `blocked_by_G1`」
是 2026-11-13 时点口径。T1 `--mode real` 双物理主机 11/11 PASS 后，T0 21/21 + T1 11/11
共同构成 **G1 通过**，M2 门禁已解除并开工；**S3 / S5 / S6 仍 `not_run`**（G0 条件冻结豁免，
分别属 M1/M2 必测回填，不是 G1 判据），G2 未达标。
见 [G0-REVIEW §11](../docs/06-plan/G0-REVIEW.md)、[ROADMAP §6](../docs/06-plan/ROADMAP.md)
与 `spikes.json` 的 `m1_progress` / `t1_progress` / `m2_progress`。

## T1 双机验收（2026-11-14 real 通过，G1 证据已取得）

```bash
python3 tests/e2e/t1/t1_accept.py --list
python3 tests/e2e/t1/t1_accept.py --self-test                                  # 11/11 PASS，evidence_valid=false
python3 tests/e2e/t1/t1_netprobe.py                                             # 本机跨 IP UDP 可达性真实探测
python3 tests/e2e/t1/t1_accept.py --lan-precheck --address-mode aliases         # 需三个二进制环境变量
python3 tests/e2e/t1/t1_accept.py --lan-precheck --address-mode nic
python3 tests/e2e/t1/t1_accept.py --mode real --refresh-nat                     # 需两台物理主机 + T1_REMOTE_PRESTARTED=1
```

单机预检各 **10 PASS / 1 SKIP**、`evidence_valid=false`；`--mode real` 的 `evidence_valid=true`
必须由双机 `machine_id` / `host` 互异 + 真实 NAT 报告 + 全用例 PASS 共同决定。详见 [T1 README](e2e/t1/README.md)。
**2026-11-14 实跑结果：双物理主机 11/11 PASS、`evidence_valid=true`**（A=192.168.1.210、B=192.168.1.209），
叠加 T0 21/21 → **G1 通过**。2026-11-13 因无第二台物理主机而 `BLOCKED` 的结论已失效。

## T2 本地接线验收（M2/W3.3，非 S6 / G2 证据）

```bash
cargo build --release --workspace --offline
python3 tests/e2e/t2/t2_nat_path.py --self-test      # mock wire 自检，3/3 PASS，不需二进制
python3 tests/e2e/t2/t2_nat_path.py --mode local     # 完整 10 条用例，10/10 PASS
# 可选：COLDREVIVE_RELAY 覆盖真实中继服务端二进制路径
```

[T2 README](e2e/t2/README.md)：同一主机 A/B 双守护进程（18800-18802 / 18900-18902）+ mock STUN
+ **真实 `coldrevive-relay` 二进制**（纯帧路由 `--stats-file` 落真实计数）+ `coldrevived` 持有者订阅线程。
全部业务应答由**真实**守护进程经真实 UDP/TCP/HTTP/CLI 产生；验证端点通告、候选计划、
单 socket 打洞、中继回退（T2-07 实测 `delivered=2 / dropped=0`）、tracker 私网/公网 peer 协商、
M1 命令无回归、无候选时如实失败。`--self-test` 仍用纯 wire mock，只验帧编解码。
报告固定 **`evidence_valid=false` / `s6_status=not_run` / `g2_claim=false`**，
**只证明接线，不证明四类真实 NAT 可达率**（真实中继服务端 ≠ 真实 NAT 环境）。

## M0 Spike 实验包

入口说明：[tests/spikes/README.md](spikes/README.md)。`spikes.json` 登记 S1–S8 的判据与状态（S1 / S2 / S4 / S7 `pass`，S8 由项目所有者豁免登记 `pass` 无书面法务意见，**S3 / S5 / S6 仍 `not_run`**）；M1 开工后的机读进度在 `m1_progress`，M2 进度在 `m2_progress`。**当前口径：G1 已通过、M2 门禁解除并已开工**（原文「G1 未通过、M2 仍 `blocked_by_G1`」为 2026-11-13 历史记录）。`m0_preflight.py` 只检查实验条件是否齐备：

```bash
python3 tests/spikes/m0_preflight.py            # 退出码 2 = 存在工程阻塞项（默认未设二进制环境变量时）
python3 tests/spikes/m0_preflight.py --json    # 机器可读输出
```

**该脚本不执行任何 Spike、不接触公共 KAD、不产出技术结论。** 结论只能由实验执行人依据证据手工回填 `spikes.json`；本地回环模拟不得判定 S2 通过。

## 约定

- 测试环境编号与 [TEST §2](../docs/05-quality/TEST.md) 一致：`T0` 单机回环、`T1` 局域网双机、`T2` 公网多节点、`T3` DHT 仿真。
  **编号消歧**：`e2e/t2/` 目录是 M2/W3.3 的**本地接线验收套件**，与规划编号「T2 = 公网 ≥8 节点」不是同一环境；前者报告恒 `evidence_valid=false`，后者尚未落地。
- 用例新增须先在 docs 中确认判据出处，并在 `t0_cases.json` 的 `source` 字段登记；**阈值只在 `t0_config.json` 定义一次**，禁止在断言代码里另写数字。
- `fixtures/` 中**禁止**存放任何真实受版权保护内容；一律使用自制小文件与合成数据。样本由固定种子生成，`sample/` 不入库。
- 兼容性矩阵客户端（qBittorrent / Transmission / BitComet / eMule / AriaNg）版本需在 `fixtures/compat-matrix.toml` 中锁定。
- 性能与 Soak 用例需输出 RSS / CPU / 上行采样，作为 G2、G4 门禁证据。
- 运行产物（`e2e/t0/.run/`、`e2e/t0/reports/`、`e2e/t1/.run/`、`e2e/t1/reports/`、
  `e2e/t2/.run/`、`e2e/t2/reports/`、
  `spikes/reports/`、`spikes/s5/utp/reports/`、`fixtures/t0/sample/`）已由 `.gitignore` 排除。
