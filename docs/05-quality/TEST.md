---
doc_id: DOC-12
title: 测试计划（TEST）
category: 质量
version: 2.3
status: 待评审
owner: 测试组
updated: 2026-11-11
---

# 测试计划（TEST）

> 编号 DOC-12 · 版本 2.3 · 状态 待评审 · 责任人 测试组 · 更新 2026-11-11
> 本文是 [SRS KPI](../01-product/SRS.md) 与 [ROADMAP 质量门禁 G0～G4](../06-plan/ROADMAP.md) 的判定依据。测试代码落位于 [tests/](../../tests/README.md)。

## 1. 测试分层

| 层级 | 范围 | 工具 / 方法 | 目录 |
| :--- | :--- | :--- | :--- |
| 单元 | 哈希、分词、Key 生成、Challenge 校验、LRU 淘汰、状态机 | `cargo test`，覆盖率 ≥ 70% | `tests/unit/` |
| 集成 | 索引 → 造种 → 入库 → KAD 发布 → 唤醒 → 下载闭环 | 双节点 Docker 网络 | `tests/integration/` |
| 协议一致性 | 唤醒报文、错误码、JSON-RPC 契约 | 契约测试 + golden files | `tests/integration/` |
| 网络仿真 | 1,000+ 虚拟 DHT 节点、丢包/延迟/抖动、节点抖动 | 本地 DHT 模拟器（`dht_simulator.py`） | `tests/e2e/` |
| 兼容性 | 见 §3 矩阵 | 真实客户端回归 | `tests/e2e/` |
| 性能 / Soak | 见 §4 | 72 h 长稳 + RSS/CPU 采样 | `tests/e2e/` |
| 安全 | 污染注入、Sybil、洪泛、重放、路径穿越 | 故障注入测试 | `tests/e2e/` |

## 2. 测试环境

| 编号 | 环境 | 用途 |
| :--- | :--- | :--- |
| **T0** | 单机回环，两个守护进程实例不同端口 | 验证全链路逻辑，最快反馈 |
| **T1** | 局域网双机，真实 NAT 内网 | 验证 UPnP 与 UDP 唤醒 |
| **T2** | 公网多节点（≥ 8 台不同网络环境） | 测可达率与复活成功率 |
| **T3** | 可控 DHT 仿真网络（1,000+ 虚拟节点） | 测发布/查询命中与污染抗性 |

> 环境编号在 [tests/fixtures/](../../tests/README.md) 配置中保持一致。`fixtures/` 中禁止存放任何真实受版权保护内容，一律使用自制小文件与合成数据。

### 2.1 T0 验收脚本骨架

[T0 双节点验收骨架](../../tests/e2e/t0/README.md) 已落位于 `tests/e2e/t0/`，是 G1 门禁的机器可读证据来源。当前为**骨架 / 未接线**状态：

| 项 | 内容 |
| :--- | :--- |
| 用例 | `T0-00`～`T0-20`，覆盖 F1→F2→F3→F4 主链路、§5 故障注入、PROTO v1 golden 契约与最小暴露检查；判据出处逐条登记在 `t0_cases.json` |
| 阈值 | 只在 `t0_config.json` 定义一次：P95 < 3000 ms（NFR-04）、复活成功率 1.0（G1）、唤醒超时 1.5 s（PROTO §3.1） |
| 环境 | A / B 两实例回环不同端口；样本由固定种子生成，无版权内容 |
| 防呆 | `--self-test` 用合成观测验证脚本本身，报告的 `evidence_valid` 恒为 `false`；`--mode real` 在 M1 二进制接线前一律 `BLOCKED` 并以退出码 2 结束，杜绝「没跑却显示绿」 |
| 观测层 | CLI v1 契约（`t0_config.json` 的 `launch`）与 20 条真实取数映射（`t0_cli.OBSERVE_PLAN`）已落地；伴随种子 InfoHash 按 BT 规范从 info 字典原始字节独立计算。`t0_cli_selftest.py` 验证「字段缺失不合成、缺二进制抛异常」 |
| 证据 | JSON 报告落在 `tests/e2e/t0/reports/`，按 §7 附入 G1 交付物；接线清单见骨架 README §5 |

> T0 只在 `127.0.0.1` 运行，除 S2 专项外不接触公共 KAD。T1（局域网双机）通过后才与 T0 共同构成 G1 证据，见 [ROADMAP §3](../06-plan/ROADMAP.md)。

**M0 前置**：[tests/spikes/](../../tests/spikes/README.md) 是 G0 的 Spike 实验包（S1–S8 判据、状态登记、结论回填规则），`m0_preflight.py` 只检查实验条件是否齐备，不产出任何 Spike 结论。当前 S1–S7 为 `not_run`，S8 由项目所有者豁免登记为 `pass`（无书面意见）；`cargo` 已可用、S2 实网实验已获人工批准，最小 eMule KAD 客户端的真实 `0xE4` wire 本地回环与 LAN 真实客户端互操作探测均已跑通（均不构成 S2 结论），G0 参数与 bootstrap 清单已确认、判据口径已回写，**S2 已于 2026-11-10 判 `pass`**（`PUBLISH_RES load=1` + `SEARCH_RES` 命中，并由两进程 A/B 独立取回，详见 [PLAN.md §20–§21](../../tests/spikes/s2/PLAN.md)）；早期 0 响应系 IP 字节序（v2.4.14）与 Kad tag 帧头（v2.4.26）两处编码缺陷，非网络过滤；v2.4.7 已补齐 P4 前置实现（`0xE5` 解包、tolerance 收敛、跳过 HELLO、bootstrap 公网校验）；仍缺第三方 BT 客户端与 M1 二进制、`CliAdapter.wired` 为 `False`，因此 T0 真实链路仍不可执行。

**ADR-011 L1 / L2 探针证据（2026-11-11，不构成 T0/T1 或 Spike 结论）**：[S2 实验包](../../tests/spikes/README.md)
在 `127.0.0.1` 与 RFC1918 上另产出两组回归 —— `kad-mini`（`cargo test` 21/21、`selftest` 48/48、
`loopback` 发布 ACK 5/5 + 命中 5/5、`l2-source-probe` 在真实 aMule 上完成 `0x43`+`0x44` 双轨发布与
`0x33`+`0x34` 双轨取回）与 `ed2k-mini`（`cargo test` 31/31、`selftest` 26/26、
3/3 真实 TCP 回环完成下载/上传分片闭环、真实 eMule `HELLO` 握手成功）。
报告固定 `public_network_contacted=false` / `evidence_valid_for_pass=false`，**0 个公网报文**。
缺口依旧：该 LAN 对端共享文件数为 0，能力「从旧客户端下载」只能靠自两端回环证明，须在 M1
换真有共享文件的对端补测；`OP_SETREQFILEID (0x4F)` 的源码/客户端差异登记为开放观测。
详见 [PLAN §23](../../tests/spikes/s2/PLAN.md)。

## 3. 兼容性矩阵（必须全绿）

| 对端 | 场景 | 预期 |
| :--- | :--- | :--- |
| qBittorrent | 拿 magnet 连接被唤醒的 A | 正常下载，无需 ColdRevive |
| Transmission | 导入伴随 `.torrent` | 正常解析与下载 |
| BitComet | 标准 handshake + 长效种子共存 | 互不干扰 |
| eMule / aMule（P2） | ed2k 链接与 KAD 共存 | 不污染彼此命名空间 |
| AriaNg | 通过 `/jsonrpc` 管理任务 | 任务列表与速度正常 |
| 浏览器插件 | 推送下载链接到 ColdRevive | 任务创建成功 |
| 关闭 UI 窗口 | 后台下载/做种 | 不中断 |

客户端版本需在 `tests/fixtures/compat-matrix.toml` 中锁定。

## 4. 性能压测场景与基线

| 场景 | 负载 | 通过标准 |
| :--- | :--- | :--- |
| 海量索引 | 10,000 文件（模拟元数据） | 空闲 RSS ≤ 100 MB，启动索引加载 < 10 s |
| 唤醒时延 | 并发 50 次唤醒 | P95 < 3 s（局域网），无失败 |
| 后台 QoS | 前台满速下载 + 50 个后台做种 | 前台劣化 ≤ 5%，上行 ≤ 阈值 |
| 造种 | 4 GB 文件 × 10 | 不阻塞 UI，CPU 优先级符合配置 |
| Soak | 72 h 连续运行 + 周期性唤醒/休眠 | 无内存增长趋势、无句柄泄漏 |
| 洪泛 | 1,000 req/s 伪造唤醒 | 服务存活，合法请求成功率 ≥ 95% |

## 5. 故障注入清单

需求来源：[FLOWS §8 异常与降级路径](../03-design/FLOWS.md)。

| 注入 | 期望 |
| :--- | :--- |
| 唤醒后节点离线 | 1.5 s 超时后切换候选，不阻塞 UI |
| 索引命中但文件被删 | 返回 `REJECT_FILE_GONE` 且撤销 KAD 发布 |
| 活跃池打满 | `REJECT_BUSY` 或 LRU 淘汰，无崩溃 |
| 内存/带宽越限 | 降级链逐层生效，RSS 不越硬限 |
| Symmetric NAT / CGNAT | LowID 中继回退，给出用户提示 |
| KAD 返回空 | 按 Mainline DHT → Tracker → 本地索引降级 |
| 伪造种子 / 分片投毒 | 校验失败丢弃 + 拉黑 + 信誉扣分 |
| 索引污染 / 重放攻击 | 限流与 nonce 去重生效，合法请求不受影响 |
| 造种队列积压 | 降并发、延迟发布，不阻塞前台 |

## 6. 各阶段质量门禁

| 门禁 | 位置 | 条件 |
| :--- | :--- | :--- |
| **G0** | M0 结束 | S1–S8 全部有结论，关键技术路线冻结（[TECH-STACK](../02-architecture/TECH-STACK.md)） |
| **G1** | M1 结束 | T0/T1 全链路闭环成功，受控环境复活成功率 100% |
| **G2** | M2 结束 | NFR-01/02/03 达标，NAT 矩阵通过率达标 |
| **G3** | M3 结束 | 兼容性矩阵全绿，NFR-04 达标 |
| **G4** | M4 结束 | 安全测试全通过，公测 KPI 达标，法务评审通过 |

## 7. 交付物

- 每次门禁输出：测试报告、指标快照（RSS/CPU/上行/唤醒时延 P95）、兼容性矩阵结果、未通过项与豁免说明。
- 性能与 Soak 用例必须输出原始采样数据，作为门禁证据归档。

## 相关文档

- [DOC-02 SRS 验收指标](../01-product/SRS.md)
- [DOC-07 FLOWS 异常降级表](../03-design/FLOWS.md)
- [DOC-13 RISK 风险登记册](RISK.md)
- [DOC-14 ROADMAP 里程碑与门禁](../06-plan/ROADMAP.md)
