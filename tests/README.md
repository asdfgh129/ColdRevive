# 测试目录（tests/）

> 测试策略、矩阵与门禁见 [TEST 测试计划](../docs/05-quality/TEST.md)。当前为目录骨架 + T0 验收脚本骨架，尚无 Rust 测试代码。

## 目录划分

| 目录 | 用途 | 对应测试层 |
| :--- | :--- | :--- |
| `unit/` | 各 crate 内联单测的补充与共享测试工具 | 单元测试 |
| `integration/` | 索引 → 造种 → 入库 → KAD 发布 → 唤醒 → 下载闭环 | 集成测试 |
| `e2e/` | 双节点 / 多节点真实网络场景、旧客户端兼容性矩阵 | 端到端与兼容性 |
| `e2e/t0/` | **T0 单机回环双节点验收**：用例清单、环境编排、观测适配器、CLI 契约、断言引擎、JSON 报告 | 端到端（G1 门禁证据） |
| `spikes/` | **M0 Spike 实验包**：S1–S8 判据与状态、`m0_preflight.py` 就绪度检查、结论回填规则 | 选型验证（G0 门禁） |
| `fixtures/` | 固定种子文件、样本文件、golden 协议报文、`T0～T3` 环境配置 | 测试数据 |
| `fixtures/t0/` | **T0 确定性样本、PROTO v1 唤醒报文 golden** | 测试数据 |

## T0 验收骨架

入口说明：[tests/e2e/t0/README.md](e2e/t0/README.md)（判据来源、运行方式、报告格式、接线清单）。

```bash
# 列出 21 条用例及判据出处
python3 tests/e2e/t0/t0_accept.py --list

# 框架自检：用合成观测跑通编排与全部断言（仅证明脚本正确）
python3 tests/e2e/t0/t0_accept.py --self-test

# 环境与确定性样本自检
python3 tests/e2e/t0/t0_harness.py --self-check

# 观测采集层自检（验证不合成观测值，非 G1 验收）
python3 tests/e2e/t0/t0_cli_selftest.py
```

**当前状态：观测层已实现，真实链路仍未接线。** `CliAdapter.wired = False`，`--mode real` 一律把真实用例标记为 `BLOCKED` 并以退出码 2 结束；stub 自检报告的 `evidence_valid` 恒为 `false`。这是刻意防呆，避免把「没跑」显示成「绿」。CLI v1 契约与 20 条真实取数映射已落地（见骨架 README §5.1），但 M1 二进制、第三方 BT 客户端与真实伴随种子尚缺。M0 技术路线冻结后按骨架 §5 完成剩余接线，再以 T0 + T1 共同构成 G1 证据。

## M0 Spike 实验包

入口说明：[tests/spikes/README.md](spikes/README.md)。`spikes.json` 登记 S1–S8 的判据与状态（截至 2026-11-11：S1 / S2 / S4 / S7 `pass`，S8 由项目所有者豁免登记 `pass` 无书面法务意见，S3 / S5 / S6 `not_run`），`m0_preflight.py` 只检查实验条件是否齐备：

```bash
python3 tests/spikes/m0_preflight.py            # 退出码 2 = 存在阻塞项（当前必然如此）
python3 tests/spikes/m0_preflight.py --json    # 机器可读输出
```

**该脚本不执行任何 Spike、不接触公共 KAD、不产出技术结论。** 结论只能由实验执行人依据证据手工回填 `spikes.json`；本地回环模拟不得判定 S2 通过。

## 约定

- 测试环境编号与 [TEST §2](../docs/05-quality/TEST.md) 一致：`T0` 单机回环、`T1` 局域网双机、`T2` 公网多节点、`T3` DHT 仿真。
- 用例新增须先在 docs 中确认判据出处，并在 `t0_cases.json` 的 `source` 字段登记；**阈值只在 `t0_config.json` 定义一次**，禁止在断言代码里另写数字。
- `fixtures/` 中**禁止**存放任何真实受版权保护内容；一律使用自制小文件与合成数据。样本由固定种子生成，`sample/` 不入库。
- 兼容性矩阵客户端（qBittorrent / Transmission / BitComet / eMule / AriaNg）版本需在 `fixtures/compat-matrix.toml` 中锁定。
- 性能与 Soak 用例需输出 RSS / CPU / 上行采样，作为 G2、G4 门禁证据。
- 运行产物（`e2e/t0/.run/`、`e2e/t0/reports/`、`spikes/reports/`、`fixtures/t0/sample/`）已由 `.gitignore` 排除。
