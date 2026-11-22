# P0 运行期 e2e（FR-01/02/04/05/06/09/10/12/13）

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
  KAD 可检索。FR-24 公网检索仍 BLOCKED（G0 `protocol_scope` 未放宽）。
- **FR-01 后端是轮询快照**（`cr_indexer::PollWatcher`），语义与内核通知一致但延迟 ≤ 周期；
  Windows `ReadDirectoryChangesW` 尚未接入，不得宣称已用内核通知。
- **FR-09 只覆盖按来源 IP 的令牌桶**，不含全局/分布式配额，也不覆盖数据面。
- **FR-15 目前只落地「HOT 条目数」这一条阈值**（`limits.max_hot_seeds`）：内存/带宽/造种
  CPU 的阈值仍**未接线**，不得因为本文档有一行 FR-15 就宣称 FR-15 全部完成。
  阈值的语义是「拒绝新唤醒」，与 `engine.max_active_seeds` 的「逐出后续服务」是两件事，
  两者可同时配置（本脚本 C 段就是 `cap=2` + `limit=1`）。
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

`run/`、`reports/` 全部在 `.gitignore` 内：这里只留脚本与说明，运行产物不进公开仓库。
