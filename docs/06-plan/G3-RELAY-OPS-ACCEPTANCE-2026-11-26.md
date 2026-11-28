---
doc_id: DOC-35
title: G-3 中继运维面验收判据（预登记）
category: 计划
version: 1.1
status: 有效
owner: 项目所有者
updated: 2026-11-26
---

# G-3 中继运维面验收判据（预登记）

- 文档号：DOC-35
- 版本：v1.1（2026-11-26T08:32+08:00：预登记判据未改，追加 §5 实测结果）
- 状态：**三项分句均实测通过；G-3 运维面关闭**（不改变 `g2_claim` 与三个工具字段）
- 关联：DOC-34 §3 第 3 项（G-3 欠账原文）、ADR-014（数据预算）、`docs/07-operations/OPS.md`

## 1. 为什么先写这份

DOC-34 只写了「限流/观测/告警未单列验收」，**没有**可执行的阈值、采样方法与证据要求。
按项目纪律（降判据 ≠ 降证据；预注册判据先固定再采样），先把判据与 sha 落盘，再跑真实进程。

- 判据文件：`tests/fixtures/g3-relay-ops-thresholds.toml`
- 登记时间：`2026-11-26T08:29:00+08:00`
- **判据 sha256**：`8e91943a6d46cca21e3b1a5fa1f03f2bf709a1c9db1542c9a590145452f11ed6`
- 采样期间不得改该文件；改了就必须换新时间戳 + 新 sha + 新证据，不能调阈值解释旧数据。

## 2. 三项分句与判据

| 分句 | 要证明什么 | 通过判据 |
| :--- | :--- | :--- |
| ① 限流 | 真实 relay 进程按配置拒绝超额流量，且基线不误伤 | 基线组 `bandwidth_limited / ip_cap_rejected / conn_cap_rejected / rate_limited` 全为 0；违约组（`data_rate=1`）至少一项拒绝计数增加，且真实 stats 文件可见 |
| ② 观测 | 两个 200 ms stats 文件覆盖 TCP/UDP 路由与全部拒绝/整形计数 | 基线 50 轮 TCP+UDP 至少 45 轮回声；两份 stats 文件 mtime 滞后 ≤2 s；UDP stats 含 5 个 per-session 字段（`per_session_limited/limited_bytes/shaped/shaped_bytes/dropped`） |
| ③ 告警 | 有独立进程把 stats 变成可审计告警，并能区分真实违约与健康态 | 违约后 ≤3 s 出现结构化告警（含 condition/counter/delta/时间）；基线组告警数为 0（负向对照）；停 relay 后报 `stats_stale`；重启使计数归零后不误报 |

三项**全部**满足才算 G-3 运维面验收通过；任何一项 BLOCKED 就如实写成未关。

## 3. 采样方法（真实进程，非纯单测）

1. 用 HEAD 构建的 `coldrevive-relay` / `coldrevive-relaybench`，记录二进制 sha256 与 git commit。
2. 回环独立实例：`--listen 127.0.0.1:0` + `--udp-listen 127.0.0.1:0`，端口由内核分配并从首行回显读取；
   **不连生产中继** `119.29.150.254:7101`，也不给 `--peer`。
3. 基线组用判据文件里的 `baseline_relay` 参数，违约组用 `breach_relay` 参数；
   两组都开 `--stats-file` 与 `--udp-stats-file`（各 200 ms 覆盖写一份）。
4. `relay_ops_watch` 以判据文件为唯一阈值来源，读两份 stats 文件算窗口增量，
   追加写 JSON Lines 告警文件；计数比上次小视为进程重启（`counter_reset`），该窗不告警。
5. 场景：基线 → 违约 → 停进程（stale）→ 重启（reset）。原始 stats 快照与告警文件一起归档。

## 4. 明确不覆盖

- **不改变** `g2_claim` 与三个工具字段；G2 达标声明不因 G-3 采样变化。
- 不表示公网中继当前健康——本判据只在回环独立实例上跑；公网路径另有 W59/W61/BT 证据。
- 不表示「任意链路上的中继带宽/丢包 SLO」；ADR-014 的 512 KiB/s 是部署侧护栏初始值，不是 SLO。
- 不表示仓库内有 relay systemd unit（实际没有，只有 `coldrevived-a/b`）。
- 不表示 S6 NAT 类别覆盖或公平性达标（EX-1 仍 FAIL 78/80）。

## 5. 结果（2026-11-26T08:32+08:00 采样，判据章节未改）

**总判：三项分句全部 PASS，G-3 运维面关闭。** 采样在 HEAD `f1373d4`（实现提交）重建的二进制上完成，
不是把旧构建的计数拿来充数。

| 分句 | 结果 | 实测要点 |
| :--- | :--- | :--- |
| ① 限流 | **PASS** | 基线组 TCP `bandwidth_limited/ip_cap_rejected/conn_cap_rejected/rate_limited` 与 UDP 对应四项**全为 0**；违约组（`data_rate=1`）真实 stats 中 `bandwidth_limited` **0 → 10** |
| ② 观测 | **PASS** | TCP/UDP 各 50/50 轮回声（门槛 45）；两份 stats mtime 滞后 **0.019 s / 0.020 s**（≤2 s）；UDP stats 五个 per-session 字段全部可见（此前只存在于内存结构体、未落盘，本次补齐） |
| ③ 告警 | **PASS** | 基线组 **0** 条告警；违约启动后 **0.433 s** 出现 `tcp_bandwidth_limited` 告警（门槛 ≤3 s），并在计数停止增长后自动 `resolved`；停止 relay 后 2 条 `*_stats_stale`；重启使计数归零后 **0** 条规则告警、`counter_resets_seen=1` |

- **告警实现**：`scripts/relay_ops_watch.py` 只读两份 stats 文件，按预登记阈值算窗口增量，
  追加 JSON Lines 到告警文件；计数变小一律当进程重启，不告警。产出记录带
  `condition / counter / delta_window / threshold / window_secs / recorded_at / criteria_sha256`。
- **采样编排**：`tests/e2e/g3/relay_ops_probe.py` 依次跑基线 → 违约 → 重启(reset) → 停进程(stale)，
  原始 stats 快照、告警文件、watch 摘要全部入库。
- **证据**：`tests/ARCHIVE/evidence/g3-relay-ops-20261126/`
  （`summary.json` sha256 `cff30a3ceaabeafca69c23adf74f843f99cc81bf8b6ceebf12ee27fb58b8ffc4`；
  `MANIFEST.json` sha256 `5202e619fe23f39668259762a1e529a5b517f798ee782c1eb89ca9c35b63eae4`，
  目录内 `* -text` 锁字节；15 份原始文件 sha 见 MANIFEST，`sha256sum -c` 全过）。
- **首次采样被取代**：实现提交前曾做一次预跑（随 `f1373d4` 一并提交），其 `git_commit` 字段指向旧提交；
  本次在 HEAD 重建二进制后重采并覆盖同名文件。MANIFEST 的 `supersedes` 字段保留此事实。
- **复核边界不变**：仍是回环独立实例，不碰生产 `119.29.150.254:7101`；bench 的 UDP 两端在同机，
  不能冒充两个 NAT；这些数字不是带宽/丢包 SLO，也不使 EX-1 或 S6 例外失效。
