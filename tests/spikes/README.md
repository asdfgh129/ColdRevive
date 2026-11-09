# M0 Spike 实验包（tests/spikes/）

> **状态：S1–S7 `not_run`；S8 已由项目所有者豁免登记为 `pass`（无书面意见）。** 本目录是「实验的准备与就绪度检查」，不是实验结果。
> 本机已具备 `rustc` / `cargo` 1.99.1，S1 最小 Demo 已跑通；S2 实网实验的人工批准已登记在 `spikes.json` 的 `public_kad_approval`，[实施排期](s2/PLAN.md) P0–P2 与[最小 eMule KAD 客户端](s2/kad-mini/README.md)（真实 wire 回环）已完成，实网 P3/P4 仍被 G0 参数与 bootstrap 门禁阻断。仍缺第三方 BT 客户端与 M1 二进制，`m0_preflight.py` 会把剩余阻塞项逐条列出。

## 1. 定位与红线

判据的唯一来源是 [TECH-STACK §2 M0 Spike 清单](../../docs/02-architecture/TECH-STACK.md)，机读状态在 `spikes.json`。本目录只回答两件事：

1. **现在能不能开始做实验**（`m0_preflight.py`）—— 纯本地检查，不接触公共 KAD。
2. **实验怎么做、结论怎么记**（本文件 + `spikes.json`）—— 步骤、判据、证据清单。

三条红线：

- 脚本**禁止**自动把任何 Spike 置为 `pass`。结论只能由实验执行人依据证据手工回填 `spikes.json`，并附证据路径。
- **本地回环私有 KAD 子网只能作为 S2 预演**，不得据此判定 S2 通过；S2 的 `pass` 必须来自实网小样本，且需人工批准、最小样本、限速，见 [OPS §6 合规边界](../../docs/07-operations/OPS.md)。
- 不把 [REFERENCES 中 V-01～V-09](../../docs/08-reference/REFERENCES.md) 未经核实的第三方数据写进判据阈值。

## 2. 运行

```bash
# 就绪度检查（默认完全本地，不联网）
python3 tests/spikes/m0_preflight.py

# 可选：仅 HEAD 探测依赖源可达性，不发布任何数据，结果只作参考
python3 tests/spikes/m0_preflight.py --probe-network

# 机器可读输出
python3 tests/spikes/m0_preflight.py --json
```

| 退出码 | 含义 |
| :--- | :--- |
| `0` | 全部前置条件就绪 |
| `2` | 存在阻塞项（当前环境必然为 2） |
| `1` | 脚本自身错误 |

报告落在 `tests/spikes/reports/`，已由 `.gitignore` 排除。报告中的 `g0_ready` / `t0_real_ready` 只表示**条件是否齐备**，不表示实验通过。

## 3. 起跑顺序

按 [OPEN-QUESTIONS §3](../../docs/06-plan/OPEN-QUESTIONS.md) 与 [ROADMAP §6](../../docs/06-plan/ROADMAP.md)，不是 S1→S8 顺序刷：

| 顺序 | Spike | 能否决什么 | 并行关系 |
| :--- | :--- | :--- | :--- |
| 第一 | **S2** | 产品定位（公共 KAD 若不接纳命名空间，ADR-001 需切 `private`） | 与 S1 并行 |
| 已决 | **S8** | 项目所有者 2026-11-09 豁免，登记 `pass`（无书面意见，R-11 保留，M4 前复核） | 无需再等 |
| 第二 | **S1** | 架构（BT 引擎能力）；失败只抬成本，`Engine` Trait 已留退路 | 与 S2 并行 |
| 后置 | S3–S7 | 参数与降级策略，不推翻设计，G0 前补齐即可 | — |

## 4. S1 实验方案：BT 引擎能力

**假设**：`rqbit` 支持按需实例化、Pause/Resume、自定义握手拦截、分片校验。

| 项 | 内容 |
| :--- | :--- |
| 前置 | `cargo` 可用；能拉取 `rqbit`；最小 Demo 放在 `tests/spikes/s1/`，**不建 `src/`、不建 `Cargo.toml` 于项目根** |
| 步骤 | ① 起两个本地 peer ② B 发起标准 BT Handshake ③ A 在握手里拦截并动态加载伴随 `.torrent` ④ 实例化 TorrentContext ⑤ Pause / Resume ⑥ 传输至少一个分片并逐片 SHA-1 校验 |
| 判据 | 见 `spikes.json` 的 `S1.criteria` 四条，全部满足才判 `pass` |
| 记录 | 发布/查询逐次原始日志、Demo 源码版本、分片校验输出 |
| 失败替代 | 切 `libtorrent` FFI 或自研精简做种层；`cr-engine` 的 `Engine` Trait 不变（[TECH-STACK §5](../../docs/02-architecture/TECH-STACK.md)） |
| 影响 | 决定 [ADR-002](../../docs/02-architecture/adr/ADR-002-bt-engine.md) 是否维持 `rqbit`，以及 T0-07 的观测方式 |

## 5. S2 实验方案：公共 KAD 命名空间

**假设**：可接入公共 eMule KAD，且 `ColdRevive_V1:` 前缀不与现有流量冲突。

| 项 | 内容 |
| :--- | :--- |
| 前置 | `cargo` 可用；**实网实验人工批准记录**（写入 `spikes.json` 的 `S2.public_kad_approval`）；限速配置 |
| 预演 | L1 逻辑预演 [`s2_loopback.py`](s2/s2_loopback.py) + L2 真实 wire 回环 [`kad-mini/`](s2/kad-mini/README.md)（真实 `0xE4` Kademlia2 报文）均已在 `127.0.0.1` 跑通发布/查询闭环，此步只验协议实现，**不产生 S2 结论** |
| 实网 | **未执行**，门禁见 [PLAN.md §2/§3](s2/PLAN.md)：须先由 G0 确认最小样本量与限速阈值、由用户提供 bootstrap 清单；`kad-mini live --check` 当前拒绝。执行时按最小样本逐条记录发布成功率与查询时延分布、观察是否被现有节点丢弃 / 视为污染，禁止批量发布测试 Key |
| 判据 | 见 `spikes.json` 的 `S2.criteria`；量化阈值由 G0 评审确认后写入，当前不硬编码 |
| 失败替代 | `bootstrap_mode = private` 自建命名空间，并重估 [ADR-001](../../docs/02-architecture/adr/ADR-001-kad-network.md) 与冷启动论证 |
| 影响 | 唯一能否决**产品定位**的 Spike；T0-03 只能在私有子网验证，不得向公共网络发布测试数据 |

## 6. 结论回填规则

`spikes.json` 每项的 `status` 取值：`not_run` / `pass` / `fail` / `fallback`。

1. 只有实验执行人能改 `status`，且必须同时补 `evidence_required` 中列出的证据路径。
2. `fail` 与 `fallback` 必须写明触发的判据编号与替代方案，并同步 [RISK](../../docs/05-quality/RISK.md) 对应风险项状态。
3. `pass` 需要至少一次可复现的完整运行记录；本地模拟结果只能作为附件，不能单独支撑 `pass`。
4. 八项结论齐备后形成 G0 报告，才能按 [src/README.md](../../src/README.md) 的门禁创建 `Cargo.toml` 与 crate 骨架。

## 7. 与 T0 的关系

[T0 双节点验收骨架](../e2e/t0/README.md) 属于 **G1**（M1 核心链路），本目录属于 **G0**（选型假设）。顺序是 G0 先冻结技术路线，再按 T0 README §5 接线：S2 先行 → S1 并行 → CLI 契约定稿 → 观测实现 → 真实 InfoHash / 埋点 → `wired=True`。

**在 G0 未通过、M1 二进制不存在之前，`CliAdapter.wired` 必须保持 `False`，`--mode real` 必须继续报 `BLOCKED`。** 这条由 `.github/workflows/t0.yml` 在 CI 中强制校验。
