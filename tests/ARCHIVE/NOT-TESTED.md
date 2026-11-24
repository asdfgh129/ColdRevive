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
| FR-25 第二个 aMule 上传端反向 A/B | **未启动** | 需用户明确确认；当前上传方向仍 BLOCKED |
| FR-10 守护进程多样种 + cap 逐出 e2e | 仅组件级 `LruScheduler` 有证据（`/health` 可观测），daemon 多任务 e2e 未跑 | 见 `TEST-RUNS-2026-11-24.md` 的后续条目 |
| 公平性阈值 / 拥塞语义 | 只有量具原始计数，无阈值判定 | 多组公网丢包/时延梯度样本 + 阈值讨论 |
| Mainline DHT 公共引导邻居发现 | 自有节点闭环；公共节点无可用 token | 找到确实签发 token 的邻居 |
| TCP 打洞接 BT 数据面 | 原语通过，公网双侧 BLOCKED | 先解决公网双侧；symmetric 不可直连是正确结论 |
| G2 / S6 自动通过 | 机器常量固定 `g2_claim=false`、`s6_status=not_run`、`evidence_valid_for_pass=false`、`auto_pass=false` | 只能由项目所有者书面判定 |

## C. 已补齐的可移植证据

- P1 本地回环 e2e：2026-11-24 重跑 **5/5 PASS**，报告 `evidence/p1-loopback-20261124.json`；
  首次运行因 Windows UDP 端口排除导致 WSAEACCES 10013，原始 daemon.log 保留在
  `evidence/p1-first-attempt-daemon.log`，已修复端口探测逻辑。
- FR-25 daemon 接线、跨分片与上传诊断 JSON：`tests/e2e/w25-daemon/reports/*.json`
  与 `run/diag-upload-frames-*.log` 已纳入版本控制；大体积 `.bin` 样本仍留本机。
- FR-24 209 eMule 闭环：四份 2026-11-24 JSON 报告原本已入库，账本登记其哈希。
- 全量 Rust 单测：2026-11-24 原始输出 `evidence/cargo-test-workspace-20261124.log`。
