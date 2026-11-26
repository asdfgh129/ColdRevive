# G-3 中继运维面验收判据（预登记）

- 文档号：DOC-35
- 版本：v1.0（2026-11-26T08:29:00+08:00，**预登记；采样结果待补**）
- 状态：判据已固定，尚未采样；本文件不签发 G-3 通过
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

## 5. 结果

待采样后以追加章节写入；判据章节不改写。
