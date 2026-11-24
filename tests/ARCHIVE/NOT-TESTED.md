# 未验证 / 证据缺失清单（2026-11-24 审计）

> 本文防止两类误读：①「日志里写了结论」当成有原始证据；
> ②「测过一次」当成需求闭环。归档只描述事实，不改门禁常量。

## A. 测过但原始证据已缺失（不可追溯，禁止升级为门禁证据）

| 项 | 实际情况 | 处理 |
| :--- | :--- | :--- |
| P0 FR-15 资源阈值拒绝快照（2026-11-22 首次运行） | 后续运行把同名 `health_a_*.json` 覆盖；真实观测结论只留在工作日志 | 归档为 `log-only`；如要引用结论必须重跑并落新证据 |
| FR-25 上传侧 tcpdump 原始 pcap | 当时的 `tcpdump` 只输出到临时目录，未进仓库且现已找不到 | 保留帧级日志 `w25-daemon/run/diag-upload-frames-*.log`；pcap 记为 MISSING |
| 家宽 kit 原始回传 `results*.zip` | 含用户内网地址/主机名，被 `.gitignore` 排除，只在本机 `$TEMP/cr-homeprobe-results` | 只登记路径与哈希；公开文档仅引用脱敏结论 |
| 11-24 209 主机 SSH 可用性检查 | 用户名/密码由用户提供，仅用于连接；凭据不得入库 | 归档时只保留目标主机、系统、eMule 配置与结论；见 `public-209-target-profile-20261124.json` |

## B. 明确尚未测试 / 尚未闭环

| 项 | 状态 | 前置条件 |
| :--- | :--- | :--- |
| FR-25 第二个 aMule 上传端反向 A/B | **已执行并归档**：0x46 版 97.6% 停滞；同构 `OP_COMPRESSEDPART(0x40)` 后两轮 A/B 我们 daemon 与 stock 上传端都落地同一 md5（LAN PASS） | 见 `TEST-RUNS-2026-11-24.md` 与审计 §17.2；公网 CLI 组件级闭环已于 v2.4.116 补齐，daemon 路径仍未测 |
| FR-10 守护进程多样种 + cap 逐出 e2e | **已于 2026-11-24 重跑并归档**：P0 77/77，wake1 `evictions=0` → wake2 `evictions=1`，`seed_active<=cap=1` | 见 `TEST-RUNS-2026-11-24.md` 与 `evidence/p0-health-after-wake*-20261124.json` |
| 公平性阈值 / 拥塞语义 | **机制已实现并测过**（AIMD/RTT/pacing + 每会话桶，单测 PASS）；仍无阈值判定，不能说公平性达标 | 共享真实瓶颈 + 竞争流 + 预注册 Jain/份额阈值 + 多轮原始样本 |
| Mainline DHT 公共引导邻居发现 | **已闭环（2026-11-24）**：从 `dht.transmissionbt.com` 发现 `37.48.95.218:3691`（`token_len=4`），222 受控 announce + 221 独立 get_peers 回查到 `112.5.32.34:17200`；v2.4.116 daemon 已补治理与有界 find_node | 见 `tests/e2e/dht-announce/reports/mainline-announce-verify-20261124.json`；daemon 自动加入仍缺公网可达率/NAT 矩阵证据 |
| TCP 打洞接 BT 数据面 | 原语通过，handoff 已接线并回环验证；公网双侧 **BLOCKED**（2026-11-24 各 34 次 SYN 静默丢弃，19910/19920 无 TCP DNAT） | 需要所有者在 CPE 上真实建 endpoint-independent TCP DNAT；symmetric 不可直连是正确结论 |
| G2 / S6 自动通过 | 机器常量固定 `g2_claim=false`、`s6_status=not_run`、`evidence_valid_for_pass=false`、`auto_pass=false` | 只能由项目所有者书面判定 |

## C. 已补齐的可移植证据

- P1 本地回环 e2e：2026-11-24 重跑 **5/5 PASS**，报告 `evidence/p1-loopback-20261124.json`；
  首次运行因 Windows UDP 端口排除导致 WSAEACCES 10013，原始 daemon.log 保留在
  `evidence/p1-first-attempt-daemon.log`，已修复端口探测逻辑。
- FR-25 daemon 接线、跨分片与上传诊断 JSON：`tests/e2e/w25-daemon/reports/*.json`
  与 `run/diag-upload-frames-*.log` 已纳入版本控制；大体积 `.bin` 样本仍留本机。
- FR-24 209 eMule 闭环：四份 2026-11-24 JSON 报告原本已入库，账本登记其哈希。
- 全量 Rust 单测：2026-11-24 原始输出 `evidence/cargo-test-workspace-20261124.log`；
  v2.4.116 最终 `evidence/workspace-test-20261124-v24116-final.log`（519 passed / 0 failed / 3 ignored）。
- FR-25 公网 CLI 双向：`evidence/fr25-public-download-shared-20261124.log`、
  `evidence/fr25-public-upload-{server,client}-final-20261124.log`；失败尝试同样在账本中。
- 公网 TCP 打洞：`tests/e2e/w56/reports/public-tcp-punch-20261124.json` 与
  `evidence/public-tcp-endpoint-reach-20261124.json`，判据为两侧 `ok && bytes_roundtrip`，
  本次如实 BLOCKED。
- FR-25 第二上传端反向 A/B：两臂报告、A 臂帧日志与 stock 上传端帧抓包均已入库；
  `stock-upload-capture-20261124085132.json` 那轮因复用文件无传输，明确不作为协议证据。
