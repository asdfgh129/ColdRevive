# P0 运行期 e2e（FR-01/02/04/05/06/07/09/10/12/13/15）

一条命令跑完，产出 `reports/report.json`（逐条 check + 边界声明）。

```bash
cargo build --release -p coldrevived
python tests/e2e/p0/run_p0.py
```

## 它证明了什么

本机回环、**两个真实进程**（A=holder，B=member），只经 HTTP `/cli`、`/health`、
真实 BT 握手与真实文件读写取证。没有任何断言读取被测代码写死的常量。

| 分阶段 | 检查项 | 证据来源 |
| --- | --- | --- |
| A | FR-01 目录增量入库 | 往共享目录丢文件 → 不重启、不手工 index，`/cli inspect entries` 多一条，`sha1`/`size` 与独立复算一致，日志有 `FR-01 入库` |
| A | FR-01 出库 | 删文件 → 条目消失，日志有 `FR-01 出库` |
| A | FR-02 多哈希 | 同一条目同时有 40 位 SHA-1 与 32 位 eD2k fileID，且 fileID 由 `tests/e2e/w24/verify_ed2k.py` 的**纯 Python MD4**独立复算一致 |
| A | FR-12 链接来源 | `/cli ed2k` 报 `link_source=index`、`backfilled_from_disk=0`，链接形状与文件名/大小/哈希逐段对齐 |
| A | FR-05/06 周期重发布 | `kad.republish_secs=3` → `/health.kad_publish.cycles` 增长且 `last.sent=true`、`last.ack_t=KAD_PUBLISH_ACK`（对端是真实 B 进程） |
| A | FR-10 多任务做种 | 两个样本 + `engine.max_active_seeds=1`，两次真实 BT 握手 → `seeding.evictions>=1`、`scheduled<=1`、`seed_active<=cap` |
| B | FR-04 重启存活 | 同一 `workdir` 重启 → `open_status=loaded_v2`、`schema_version=2`、条目与多哈希原样回来、已删文件不复活 |
| B | FR-09 按 IP 限流 | `api.rate_per_sec=4/burst=4`，连发 24 次 → 部分 `429` + `reason=rate_limited` + `retry_after_ms>0` + `limit_per_sec=4`；同一时段 `/health` 全部 200；等待后恢复放行 |
| C | FR-15 阈值拒绝 | `limits.max_hot_seeds=1` + `engine.max_active_seeds=2`：首个唤醒 `OK`；**同一样本重复唤醒仍 OK**（幂等）；第二个样本回 `REJECT_RESOURCE_LIMIT` 且带 `limit_name/limit/current`；`evictions=0`（不是逐出）；裸 BT 握手同样被拒（`guard.missed` 增长，`hot` 不变） |

## 边界（不许被这份报告顺手放宽）

- **FR-05/06 的 `sent=true` 只表示「发出 KAD_PUBLISH 并收到对端 ACK」**，不等于公网
  KAD 可检索。FR-24 的 G0 `protocol_scope` 已于 2026-11-22 由项目所有者书面解除，
  但本机回环 ACK 仍不是公网命中证据；公网检索请另跑
  `coldrevive-ed2k search --public --rate-limit-pps 0.2` 并按真实响应登记。
- **FR-01 后端是轮询快照**（`cr_indexer::PollWatcher`），语义与内核通知一致但延迟 ≤ 周期；
  Windows `ReadDirectoryChangesW` 尚未接入，不得宣称已用内核通知。
- **FR-09 只覆盖按来源 IP 的令牌桶**，不含全局/分布式配额，也不覆盖数据面。
- **FR-15 五条阈值全部接线**：`limits.max_hot_seeds` / `max_peers` /
  `max_upload_bytes_per_sec` / `max_rss_mb` / `max_seeding_cpu_millipct`。
  C 段测 HOT 数，E 段测 RSS（`max_rss_mb=1` 必然触发），F 段测 CPU
  （`max_seeding_cpu_millipct=1` = 0.001%，任何真实采样都会越过）；
  并断言**没配的阈值显式 `enabled=false`** 而不是字段缺失。
  这里只说「接线齐」，不宣称公网性能或自动通过。
  阈值的语义是「拒绝新唤醒」，与 `engine.max_active_seeds` 的「逐出后续服务」是两件事，
  两者可同时配置（C 段就是 `cap=2` + `limit=1`）。
- **RSS/CPU 读不到的平台**（非 Linux/Windows）该条不参与判定，`/health` 报
  `enabled=false` + `unavailable`；E/F 段只在能读到的平台断言，
  读不到时会失败而不是静默通过。
- **带宽与 CPU 阈值是滞后指标**：带宽来自累计上传字节的 1 s 差分，CPU 来自进程
  CPU 时间的 1 s 差分（进程级、多线程求和可超过 100%，含 HTTP/CLI）；
  只能当拒绝理由，不能读成「保证不超线」。
- **FR-07 同义词是手写封闭表**，不用词库/模型，同一文件名永远同一组词；
  扩展只进发布 key，不改 `FileEntry.keywords`。`detect_language` 只做展示，
  多脚本报 `mixed`，不假装能区分中日汉字。
- **`/cli publish` 的 ACK 只表示对端节点收下 key**，不表示公网 KAD 可检索。
- **FR-10 的「多任务」指多种子任务 + 容量约束**；不宣称跨主机长期做种稳定性。
- 样本均 < `PARTSIZE`：跨分片（`>PARTSIZE`）由 `tests/e2e/w25*` 覆盖。
- 门禁常量不变：`g2_claim=false`、`s6_status=not_run`。

## 本脚本抓到过的真实缺陷（回归价值）

1. **重启被目录监听条目拖死**：`/cli index`/监听登记的文件没有伴随种子，
   做种启动时 `fs::read("")` 直接 error 3 → 整个节点起不来。现在跳过并打印。
2. **路径身份不一致**：同一文件用相对路径入库、用绝对路径删除时匹配不上，
   条目永远留在索引里。现在入库 `canonicalize`、出库按「规范化父目录 + 文件名」比较。
3. **`sent` 可以假 true**：重发布只看到「收到一帧应答」就把 `sent` 置真；
   现在要求 `t=KAD_PUBLISH_ACK`，`/health` 里带 `ack_t`。
4. **`.bin` 被当成内容关键词**：中文标题 + `.bin` 被判成 `mixed`，
   同义词表也选不对；通用扩展名现列入 `NOISE_TOKENS`。
5. **`index` 写了 key 却永不标记已发布**：`inspect kad` 看不到新入库条目；
   新增 `/cli publish`，只有真 ACK 才置 `kad_published`。

## 分段一览

| 段 | 覆盖 |
|---|---|
| A | FR-01 目录监听、FR-02 多哈希、FR-04 重启、FR-05/06 周期重发布、FR-10 逐出、FR-12/13 链接与回流 |
| B | FR-04 重启取证 + FR-09 按 IP 限流（含 `/health` 不受限） |
| C | FR-15 HOT 阈值：拒绝而非逐出，裸 BT 握手同判据 |
| D | FR-07 文件名分词（配置无 `keywords` 时）+ `/cli kad-health` 去重健康度 |
| E | FR-15 `max_rss_mb` 真实生效：读到 RSS → 超限拒绝 → 不留 HOT 痕迹 |
| F | FR-15 `max_seeding_cpu_millipct` 真实生效：读到进程 CPU → 拒绝 → 不留 HOT 痕迹；日志带阈值名 |
| G | FR-07 确定性同义词/语言识别 + `/cli publish` 真 ACK 后 key 才出现在 `inspect kad` |

`run/`、`reports/` 全部在 `.gitignore` 内：这里只留脚本与说明，运行产物不进公开仓库。
