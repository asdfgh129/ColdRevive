---
doc_id: DOC-29
title: P0/P1/P2 与 M1/M2 欠账复核（2026-11-21）
category: 计划
version: 1.3
status: 有效
owner: 架构组
updated: 2026-11-22
---

# P0/P1/P2 与 M1/M2 欠账复核（2026-11-21）

> 编号 DOC-29 · 版本 1.3 · 状态 **有效** · 更新 2026-11-22
> 基线：仓库 HEAD `5e3e17f`；本日复跑 `CARGO_INCREMENTAL=0 cargo test --workspace --offline`
> **384 passed / 0 failed / 3 ignored**（12:00 GMT+8 左右，停止本地守护进程后执行）。
> 口径不变：**`g2_claim=false`、`s6_status=not_run`、默认承载 `tcp`**。
> **2026-11-24 复跑更新**：`CARGO_INCREMENTAL=0 cargo test --workspace --offline`
> **499 passed / 0 failed / 3 ignored**，原始输出归档在
> `tests/ARCHIVE/evidence/cargo-test-workspace-20261124.log`。门禁口径不变。
> 本文复核 [DOC-28 状态盘点](STATUS-INVENTORY-2026-11-20.md)、
> [DOC-27 阶段小结](PHASE-SUMMARY-2026-11-20.md) 与 [ROADMAP](ROADMAP.md)，
> 并与 `tests/spikes/spikes.json`、`tests/fixtures/g2-spike-verdicts.toml`、
> 各 `tests/e2e/*/reports/` 证据逐项对照。**结论：门禁绿 ≠ 需求齐，欠账主要在代码/授权，不在“再测一次网络”。**

## 1. 一句话结论

| 范围 | 结论 |
| :--- | :--- |
| P0 | 17 行中 FR-03/08/11/14 为已实现（后两项 opt-in、仅回环 TCP），FR-24/25 升到“真实 aMule 对端”但仍非全量；其余 11 行部分实现或治理债，**无一行因本日复跑发生状态变化** |
| P1 | 6 行全部部分/未实现：QoS 未接数据面、daemon 不自动映射、DHT 未启用、`rpc-secret` 缺失、Web UI/WebSocket 未实现 |
| P2 | 3 行：CLI/REST 子集可用但无 OpenAPI；跨协议补源、信誉行仍骨架 |
| M1 | G1 已过（T0 21/21 + T1 11/11），但 W2 索引侧无 watcher/SQLite 迁移；FR-11/14 默认关闭、仅回环证据；S3/S5/S6 的人工判定未回填机器常量 |
| M2 | G2 **未达标**，是全部阶段的唯一出口欠账：公平性只有量具、Mainline 未加入、TCP 打洞未接 BT 数据面、可靠层无拥塞语义；FR-10 多样种、FR-24 公网 KAD、FR-25 上传/下载跨分片已在 LAN 或真公网目标上闭环（§9/§17.2/§18.3），但不构成 G2/S6 证据，门禁常量不变 |

## 2. P0 欠账明细（按优先级）

| 欠账 | 当前状态 | 证据 / 说明 | 家宽能否推进 |
| :--- | :--- | :--- | :--- |
| FR-01 目录监控 | **已接线（有边界）** | `watch.rs` + `cr_indexer::PollWatcher` 运行期增量（新增/修改/删除入库出库），`indexer.watch_secs` opt-in；后端是**轮询快照**，Windows `ReadDirectoryChangesW` 未接入（§9） | 否（代码） |
| FR-02 指纹多哈希联动 | **已接线** | 一次读盘产出 SHA-256 / SHA-1 / eD2k fileID，三者落索引（v2）；fileID 由独立纯 Python MD4 复算一致（§9） | 否（代码） |
| FR-04 持久化 | **已接线（JSON，非 SQLite）** | 原子落盘（tmp→fsync→`.bak`→rename）、`OpenStatus` 五态、v1→v2 迁移；重启 e2e 断言 `loaded_v2`（§9）。仍用 JSON 而非 SQLite，规模上限未验证 | 否（代码） |
| FR-05/06/07 KAD 发布/查询 | **部分（本地侧齐活，含同义词）** | 周期重发布改为**到期才发**（`kad.ttl_secs`，刷新窗口 `ttl/3`；成功前推截止、失败不前推），`sent=true` 要求真实 `KAD_PUBLISH_ACK`；文件名分词 + **确定性同义词扩展**（手写封闭表、按每词 Unicode 脚本选表）+ `detect_language` 展示标签 + `/cli kad-health` 去重健康度已接入；新增 `/cli publish` 把已入库 key 真发出去（§10/§11）。**TTL 是本机记账，不是协议 TTL**；公网检索仍 BLOCKED | 部分：公网出口样本可辅助归因，**不能解除 G0 scope** |
| FR-09 限流 | **已接线** | `ratelimit.rs` 令牌桶（按来源 IP，表上限 4096），超限 429 + `reason=rate_limited` + `retry_after_ms`；`/health`、`/nat/*` 不参与限流（§9） | 否（代码） |
| FR-10 LRU 逐出 | **已闭环（多种子任务）** | 多样本 + `engine.max_active_seeds=1`，真实 BT 握手唤醒两个 infohash → `evictions=1`、`scheduled<=1`、`seed_active<=cap`（§9） | 否（代码） |
| FR-11 握手拦截 | 已实现但有边界 | opt-in 默认 false，仅回环 TCP；µTP 不外透；W62 报告是 **11/11**（`tests/e2e/w62/reports/report.json`），不是 12/12 | 否（代码/默认策略） |
| FR-12 链接生成 | **已闭环（本地）** | `/cli ed2k` 优先用索引里的 fileID（`link_source=index`、`backfilled_from_disk=0`），v1 条目读盘后回填；链接形状与文件名/大小/哈希逐段校验（§9） | 否（代码） |
| FR-13 引擎完成回流 | **已接线** | 下载完成的产物写回影子索引（含 SHA-1/eD2k/infohash），无需人工 `index` | 否（代码） |
| FR-14 空闲休眠 | 已实现但有边界 | opt-in、`/health.idle` 可观测；只有空闲计时，无 CPU/内存/带宽触发；休眠后 `seed_active=0` 但 KAD key 保留（W14 12/12） | 否（代码/默认策略） |
| FR-15 资源阈值 | **五条阈值全部接线** | `max_hot_seeds` / `max_peers` / `max_upload_bytes_per_sec` / `max_rss_mb` / `max_seeding_cpu_millipct` 共用一套准入：超限**拒绝新唤醒**（`REJECT_RESOURCE_LIMIT` + `limit_name/limit/current/limit_unit`），UDP 唤醒与裸 BT 握手同一判据；`engine.max_active_seeds` 仍是「逐出后续服务」，语义分开。带宽与 CPU 是滞后指标；RSS/CPU 读不到的平台显式 `unavailable` 而非按 0 处理（§10/§11）。这里只说**接线齐**，不承诺公网性能或自动通过 | 否（代码） |
| FR-24 公共 KAD 检索 | **技术闭环已复现（2026-11-24 v2.4.111，见 §18.3）** | LAN 真 aMuleD 检索 12/12；真 eMule 上 `0x4B` ACK + `0x3B` 命中；222→公网 `112.5.60.4:41296` 同样 ACK + 命中并有 0/0 无 HELLO 对照；早期公网 5 节点 0 命中由当时的 firewalled/空索引目标解释 | 行政阻塞已解除；**不构成 G2/S6 证据**，门禁常量不变 |
| FR-25 标准 eD2k 传输 | 部分（跨分片双向 LAN 已闭环） | 生产栈对 aMule 2.3.3 双向 LAN 22/22；daemon 接线已补（§8），无 server 登录/source exchange/credit/AICH；`>PARTSIZE` 下载 10/10，上传用同构 `OP_COMPRESSEDPART(0x40)` 后与第二台 stock amuled 双向落地同一 md5（§17.2） | 部分：公网 server/peer 仍未测，不能外推 |

治理债：SRS 已于 2026-11-22 评审至 v2.1（口径确认）；原重复 FR-24 的信誉行已改为 FR-26，实现仍为骨架。

## 3. P1 / P2 欠账明细

| 优先级 | 欠账 | 证据 / 说明 |
| :--- | :--- | :--- |
| P1 | FR-16 QoS | `cr-qos` 令牌桶未接 BT 做种；S5=fallback 否证 LEDBAT，不得写“自动退让” |
| P1 | FR-17 NAT 打洞/回退 | 原语在 Windows+Linux 回环可用，daemon 不自动建映射；S6 机器常量仍 `not_run` |
| P1 | FR-18 Mainline DHT | 2026-11-24 已从 `dht.transmissionbt.com` 发现真实 token 节点 `37.48.95.218:3691`：222 受控 announce + 221 独立 get_peers 回查到 `112.5.32.34:17200`（`tests/e2e/dht-announce/`）；daemon 侧 DHT 仍默认关闭 |
| P1 | FR-19 / FR-21 | Web UI、WebSocket 未实现（`src/crates/cr-api/src/lib.rs` 注释明确 G2 前不做） |
| P1 | FR-20 | `/jsonrpc` 子集 + CORS 有；**无 `rpc-secret`** |
| P2 | FR-22 / FR-23 / FR-26（原重复 FR-24） | 无 OpenAPI；插件仅测试资产；跨协议补源与信誉/激励均骨架 |

## 4. M1 / M2 欠账明细

### M1（G1 已过，不等于 M1 无债）

1. W2 索引侧只做一次性扫描：无目录 watcher、无 SQLite 迁移/重启 e2e。
2. FR-11/14 是 opt-in 且只对回环 TCP 有真实证据；默认配置下两个功能都关闭。
3. S3/S5/S6 的人工判定（`tests/fixtures/g2-spike-verdicts.toml`）未回填 `tests/spikes/spikes.json` 的机器常量；这是有意的分层，不是漏改。
4. S3/S5/S6 判定引用的证据文件存在于工作区，但位于 gitignored 的 `tests/spikes/{s3,s5,s6}/reports/`；只 checkout 提交无法复现，证据可移植性是独立欠账。

### M2（G2 未达标）

| 欠账 | 现状 | 下一步依赖 |
| :--- | :--- | :--- |
| 公平性 | v0/v1 量具只出原始计数（TCP 19.8–32.4%、UDP 23.1–26.8%、RTO 200→3200 ms）；可靠层无拥塞语义 | 跨公网丢包/时延梯度多组样本 + 阈值讨论；**在此之前不宣称公平性达标** |
| Mainline DHT | 已从 Transmission 引导节点发现 token 邻居并完成 222 写入 / 221 独立回查（2026-11-24）；daemon 仍默认关闭 | 证据级闭环，不作为 G2/S6 证据；如需产品内自动 DHT，仍要做 daemon 接线与资源治理 |
| TCP 打洞 | 原语通过，BT 数据面未接线，公网双侧 BLOCKED | 先解决公网双侧，再谈接线；symmetric 不可直连保留为正确结论 |
| 守护进程多样种 | **已由 P0 e2e 覆盖**：两个 infohash 在 daemon 内分别登记任务，cap=1 下真实 BT 握手唤醒第二个任务触发 `evictions>=1`、`scheduled<=1`、`seed_active<=cap` | 无需另建 daemon 多任务 e2e；`tests/e2e/p0/run_p0.py` phase A 即判据来源 |
| FR-24 公网 | **技术闭环已复现**（§18.3） | 不是 G2 证据；不因闭环改 `g2_claim` / `s6_status` |
| FR-25 跨分片/公网 | daemon 已接 `serve`/`download`；`>PARTSIZE` 下载 10/10、上传与第二台 stock amuled 双向落地同一 md5（§17.2，LAN） | 公网 server/peer 传输仍未测；不外推 LAN 结果为 G2 证据 |
| 外推边界 | W61 的 `rst-path-verified` 只指 `119.29.150.254` 这一条路径 | 不得写成“所有 RST 部署已验证” |
| 性能 | 跨公网只有单次 1 MiB 样本；4 MiB 公网 143–235 s 成因未定位 | 多组样本，按 ADR-012 不折算速率 |

## 5. 本次复核发现的文档陈旧点（已在此文纠正，不改动历史证据）

1. `docs/06-plan/PHASE-SUMMARY-2026-11-20.md` 与 `ROADMAP.md` 有 W62 **12/12** 写法；提交证据是 **11/11**。
2. `STATUS-INVENTORY-2026-11-20.md` 头部基线仍写 `e1bb4e6` / 336 passed；当前是 `5e3e17f` / 384 passed / 3 ignored。
3. `spikes.json` 的 `remaining_for_g2` 仍写 `112.5.32.34:57100 → 7100` “已实测可用”；同文件别处与 ROADMAP/README 都明确该映射**并未打通**。
4. `spikes.json` 仍把 `cr-engine/cr-dht/cr-guard/cr-ed2k/cr-sched` 列作骨架；这些已实现。`g2-spike-verdicts.toml` 中“cr-dht 仍是骨架”也是 11-17 的历史边界表述。
5. `ROADMAP.md` §6 标题写“更新至 2026-11-14”，正文实际已写到 11-20。
6. `spikes.json` 仍保留 ADR-012/016 已取消的“R-04 ≥70%”“需 ≥3 个不同真实出口”字样（有“已判定关闭”前缀，但容易误读）。

## 6. 家宽可参与的范围与交付物

**可推进（环境类证据）**：第二个真实 NAT 出口样本、公网 DHT 只读可达性、KAD 专用 UDP 出口、首个 macOS Intel 平台样本。
**不可推进（代码/授权类）**：第 2–4 节所列绝大多数欠账，尤其 G0 scope、SRS 评审、FR-25 跨分片闭环、拥塞语义、`g2_claim`/`s6_status` 常量（daemon 接线已于 §8 补上）。

交付 kit：`tests/e2e/homeprobe-macos/`（源）+ `dist/coldrevive-homeprobe-macos-intel-v1.4.zip`（分发包）。
只用 Python 标准库，兼容 macOS Ventura 13.x / Sequoia 15.x，x86_64 与 arm64 均可；
**不绑定本机 IP**（UDP 全部绑 `0.0.0.0` 临时端口，公网 IP 运行时观测），家宽动态 IP 换址后直接重跑即可；
配置中的固定地址都是远端目标（自有 STUN/DHT 与 KAD 联系人），不是本地绑定。
`run_all.sh` 依次跑离线自检、平台信息、STUN、NAT 行为、只读 DHT、KAD bootstrap 与汇总，
输出 `results/summary.json` 及各探针 JSON。脚本恒定写
`g2_claim=false`、`s6_status=not_run`、`evidence_valid_for_pass=false`、`auto_pass=false`；
缺失项记 `missing`，失败记真实错误，不补默认值。核对命令：

```bash
python3 scripts/package_homeprobe.py                    # 重建 zip 与 MANIFEST
python3 probes/dht_readonly_probe.py --self-test        # 离线编解码自检
python3 probes/nat_behavior.py --self-test              # 离线判据自检
bash run_all.sh                                         # macOS 上跑全量取证（避开 quarantine 执行位问题）
```

**首轮回传与修订（2026-11-21 晚）**：kit v1.2 在 macOS 15.5 / x86_64 / Python 3.14.5 上完成
STUN/NAT/DHT 取证：第二个真实家宽出口为对称型 NAT（映射与过滤均 address-and-port
dependent；UPnP 可用、NAT-PMP 不可用），自有 DHT 节点 `ping` 44.6 ms 且 `get_peers`
`token_len=8 / peers=1`，public transmissionbt 节点 `ping` 通。KAD `0/5 NO_REPLY`
使用的是 2026-11-20 W24-pub 的过期联系人，**不计 S2、也不构成出口被过滤的证据**。
kit v1.3 已从真 aMuleD 的 2026-11-21T12:29:40Z LAN BOOTSTRAP_RES 取 20 个联系人，
按 KAD host-order LE 反序解码取前 10 个；同清单在 Windows 主机正例对照 **8/10**
收到 `0x09 BOOTSTRAP_RES`（明文与 zlib 打包两种帧），排除清单失效/字节序错误。
原始回传含用户内网地址与主机名，未进入公开仓库；以上仅为脱敏后的描述性结论，
`g2_claim` / `s6_status` 机器常量不变。
**第二轮（v1.4）回传是正例**：同一家宽用 10 个新鲜目标、0.2 pps 实跑，
KAD **8/10** 收到 `0x09 BOOTSTRAP_RES`（`kad_plaintext`），`summary.json` 正常生成；
DHT 自有节点仍 `ping=OK / token_len=8 / peers=1`。这说明该对称 NAT / 疑似双层 NAT
出口具备 KAD 专用 UDP 出/回程能力，与 Windows 主机 8–9/10 的正例对照一致，
排除了首轮 `0/5` 的“联系人失效/字节序”混淆；但探针只发 `BOOTSTRAP_REQ`，
**仍不计 S2 / FR-24 公网检索**，也不改任何机器常量。分发包自此带版本后缀
（`...-v1.4.zip`，`MANIFEST.json` 含 `kit_version`），避免误用旧包。

## 7. 建议顺序（不因新增环境而重排）

1. **先补代码债的头部**：FR-25 公网 server/peer 传输证据（LAN 跨分片双向已闭环）；FR-11/14 的默认策略与 µTP 覆盖；可靠层拥塞语义。FR-10 守护进程多样种 + cap 逐出已由 P0 e2e 覆盖，不再列为头部欠账。
2. **授权/治理（已完成 2026-11-22）**：G0 `protocol_scope` 已书面解除；SRS v2.1 评审与编号去重已完成。R-02/R-11 仍按 M4 前复核。
3. **最后用家宽样本补环境面**：把新 NAT 分类、DHT/KAD 只读结果作为描述性证据归档；即使结果理想也不改 `g2_claim`、`s6_status`。
4. 公平性阈值、Mainline 邻居发现、TCP 打洞接数据面继续按 DOC-27 的下一步执行，不因为 kit 有输出就提前宣称达标。

## 8. FR-25 daemon 接线复核（2026-11-22，v2.4.99）

**结论：daemon 接线已补齐；`>PARTSIZE` 跨分片仍未闭环，不构成 FR-25 完成。**

- 代码：`coldrevived` 新增 `ed2k_tasks.rs`，`/cli` 接 `ed2k-link` /
  `ed2k-serve` / `ed2k-download` / `ed2k-status` / `ed2k-list`，`/jsonrpc`
  新增只读 `coldrevive.ed2kStatus`。任务结果只在生产 `cr_ed2k::stack`
  真正返回时写入，`download` 仍需本地参考文件导出真实 fileID。
- 真实 aMule 2.3.3 暴露的缺口已修：`OP_SETREQFILEID(0x4F)` →
  `OP_FILESTATUS(0x50)`；大文件多 TCP 连接由每连接线程 + 共享 `UploadQueue`
  承载并在收尾 `shutdown` 排队连接；`wire::read_packet_pending` 保留半帧，
  消除 `invalid eD2k packet length 2655250573`；上传侧对 aMule 重叠的
  `REQUESTPARTS` 做区间去重和 10ms 轻节流，下载侧在无进展时重发
  `in_flight`。
- 第二轮（同日）又修三处，均带回归测试：
  1. **上传帧粒度**：对端请求 `184320 B` 整块，自己上传却按 `10240 B` 一帧。
     改为 `wire::UPLOAD_CHUNK` 同粒度（`upload_frames_follow_real_client_chunking`）。
  2. **`OP_REQUESTPARTS` 短帧容忍**：原解析固定读 3 组区间，只带 1–2 组的
     合法帧被当错误帧丢弃（`request_parts_accepts_fewer_than_three_pairs`）。
  3. **下载侧覆盖判定写反**：对端把 `184320 B` 请求拆成 10 帧 `10240 B` 应答，
     旧判据 `r.0 >= start && r.1 <= end` 认为“没覆盖”，`in_flight` 永不清空。
     改按字节覆盖判定后，下载方向闭环。
  另加 `server_satisfies_amule_style_sliding_window`，在本机复刻 aMule 的
  「3 个 pending 块 + 每完成一块补一个新块」滑窗，把这类上传侧回归钉在 CI 里。
- 证据边界：全量 `CARGO_INCREMENTAL=0 cargo test --workspace --offline`
  **392 passed / 0 failed / 3 ignored**。
- 真 aMule RFC1918 LAN 上，`>PARTSIZE`（`9,736,192 B`）：
  - **下载方向（aMule → coldrevived）PASS 10/10**：`bytes_received=9736192`、
    950 个 `OP_SENDINGPART`（每帧约 `10240 B`）、`hashset_verified=true`、
    `completed/hash_matches/content_matches=true`，10 项 checks 全通过。
  - **上传方向（coldrevived → aMule）仍 BLOCKED**：多轮推进到
    **19.2%–85.3%** 后转 `Waiting`；对端 `Recv-Q` 积压约 416 KB 却不再消费，
    未取得对端落地文件/md5。最近的复跑（`10 KiB` 分帧 + 短帧容忍后）停在
    `19.2%`（`Temp/001.part` 已分配 1,871,872 B），说明**不是帧粒度问题**，
    更可能是 aMule 对重叠请求之外的情形（如 pending 列表与已收块的账目）停止
    追加请求。帧级 trace 与报告留在 gitignored 的
    `tests/e2e/w25-daemon/{run,reports}/`。
- 因此本轮销掉“daemon 未接线”和“下载方向跨分片”两条；FR-25 仍开放：
  上传方向跨分片闭环、公网 server/peer、双轨发布、source exchange/credit/AICH。
  `g2_claim` / `s6_status` / `evidence_valid_for_pass` / `auto_pass` 常量不变。

## 9. P0 运行期欠账批量落地复核（2026-11-22，v2.4.100）

**结论：FR-01/02/04/09/10/12/13 的运行期行为已接线并取得本机回环 e2e 证据；
FR-05/06 只做到「周期重发布 + 真实 ACK」；FR-15 只落地 HOT 阈值一条。
FR-25 上传方向、FR-24 公网检索不受本轮影响，仍 BLOCKED。**

- 可复跑证据：`tests/e2e/p0/run_p0.py` → `tests/e2e/p0/reports/report.json`，
  **48/48** check（A 段功能、B 段重启+限流、C 段阈值拒绝）。该目录只入库脚本与 README，
  `run/`、`reports/` 在 `.gitignore` 内。
- 全量回归：`CARGO_INCREMENTAL=0 cargo test --workspace --offline`
  **406 passed / 0 failed / 3 ignored**（v2.4.99 为 392 passed）。
- 逐项证据（全部读运行期 JSON / 真实文件 / 真实 BT 握手 / 守护进程日志）：
  - **FR-01**：往共享目录丢文件 → 不重启、不手工 index，`/cli inspect entries` 多一条，
    `sha1`/`size` 与脚本独立复算一致，日志有 `FR-01 入库`；删除后条目消失并有
    `FR-01 出库`。后端是**轮询快照**（延迟 ≤ 周期），不是内核目录通知。
  - **FR-02**：同一条目同时有 40 位 SHA-1 与 32 位 eD2k fileID；fileID 用
    `tests/e2e/w24/verify_ed2k.py` 的纯 Python MD4 **独立复算**一致，
    并与 `ed2k://` 链接里的哈希段逐字相等。
  - **FR-04**：同一 `workdir` 重启 → `open_status=loaded_v2`、`schema_version=2`、
    两个样本条目与多哈希原样回来、已删文件不复活。
  - **FR-05/06**：`kad.republish_secs=3` → `/health.kad_publish.cycles` 增长，
    `last.sent=true` 且 `last.ack_t=KAD_PUBLISH_ACK`（对端是真实第二个进程）。
    **边界**：这只证明「发出帧并收到 ACK」，不证明公网 KAD 可检索。
  - **FR-09**：`api.rate_per_sec=4/burst=4` 连发 24 次 → 19 次 `429`，
    全部带 `reason=rate_limited`、`retry_after_ms>0`、`limit_per_sec=4`；
    同一时段 4 次 `/health` 全部 200；等待后恢复放行。
  - **FR-10**：两个样本 + `engine.max_active_seeds=1`，两次真实 BT 握手 →
    `seeding.evictions=1`、`scheduled=1`、`seed_active=1`，上限从未被突破。
  - **FR-12**：`/cli ed2k` 报 `link_source=index`、`backfilled_from_disk=0`。
  - **FR-13**：下载完成产物写回索引（本轮以入库入口收敛，未在回环下重复一次真实
    外部下载——该行为由 W25 的真实 aMule 取片覆盖）。
  - **FR-15**：`limits.max_hot_seeds=1` + `engine.max_active_seeds=2`：
    首个唤醒 `OK`；同一样本重复唤醒仍 `OK`（幂等）；第二个样本
    `REJECT_RESOURCE_LIMIT`（`limit_name=max_hot_seeds`、`limit=1`、`current=1`）；
    `evictions=0`（是拒绝，不是逐出）；裸 BT 握手同样被拒（`guard.missed` 增长、
    `hot` 不变）。
- 本轮 e2e **抓到的三个真实缺陷**（都已修，写进 CHANGELOG）：
  1. 共享目录里「只登记、无伴随种子」的条目会让下次重启 `fs::read("")` 失败退出；
  2. 同一文件相对/绝对路径入库时身份不一致，删除事件匹配不上，条目永久残留；
  3. `fresh` 索引在 `/cli inspect` 上显示 `schema_version=0`，像「格式未知」。
- **未销账（口径不许放宽）**：
  - FR-15 的内存/带宽/造种 CPU 阈值仍未接线 → 整体仍记「部分」；
  - FR-05/06/07 仍无 TTL 数值维护、关键词分词、去重健康度；
  - FR-24 公网 KAD 检索仍 BLOCKED（G0 `protocol_scope`，需所有者书面放宽）；
  - FR-25 上传方向 `>PARTSIZE` 仍 BLOCKED（见 §8）。
  - `g2_claim=false`、`s6_status=not_run`、`evidence_valid_for_pass=false`、
    `auto_pass=false` 全部不变。

## 10. FR-15 剩余阈值 / FR-05-07 TTL 与分词 / FR-25 上传侧定位（2026-11-22，v2.4.101）

**结论：FR-15 已有四条真实阈值（HOT 数 / 对端连接数 / 上行带宽 / 进程 RSS），
FR-05/07 的本地侧（TTL 维护、文件名分词、去重健康度）已接线；
FR-25 上传方向仍 BLOCKED，但停顿位置已定位到对端而不是我们。**

- 可复跑证据：`python tests/e2e/p0/run_p0.py` → `tests/e2e/p0/reports/report.json`
  **62/62**（v2.4.100 为 48/48；新增 D 段分词+去重、E 段 RSS 阈值）。
- 全量回归：**419 passed / 0 failed / 3 ignored**（v2.4.100 为 406）。
- **FR-15（四条阈值）**：`sysres.rs` 每秒采样一次真实读数（`EngineProgress.peers`
  之和、累计 `uploaded_bytes` 差分、平台 RSS），准入判定只读采样结果。
  - 语义分工：`limits.max_*` 是**阈值**（超限拒绝新唤醒，已供片的不动）；
    `engine.max_active_seeds` 是**容量**（满了逐出最久未用者继续服务）。
  - 已在 HOT 的条目重复唤醒不按新占用计（否则客户端重连一次就被自己的历史判成超限）。
  - 多条同时超限时顺序固定 hot → peers → bandwidth → rss，便于断言「是哪条拦的」。
  - **读不到 ≠ 0**：RSS 拿不到的平台 `/health.limits.max_rss_mb` 报
    `enabled=false` + `unavailable`，该条不参与判定。
  - 证据（E 段）：`limits.max_rss_mb=1` → `max_rss_mb.enabled=true`、
    `current=17 MiB`；唤醒被 `REJECT_RESOURCE_LIMIT` 拒，`limit_name=max_rss_mb`、
    `limit=1`、`current=17`、`limit_unit=MiB`，且 `hot` 与 `seed_active` 都保持 0。
  - **未销账**：造种 CPU 阈值仍未接线，故 FR-15 整体仍记「部分」。
- **FR-05（TTL）**：`kad.ttl_secs`（缺省 3600）定义本机刷新周期，刷新窗口在 `ttl/3`。
  成功才前推 `kad_expires_at`，**失败只累加 `kad_refresh_failures`**——否则连续失败
  会把自己无限延期。空转周期报 `due=0`，不会伪造「发了 N 个 key」。
  **边界**：这是本机记账，不是协议 TTL；对端只回 `KAD_PUBLISH_ACK`，不带过期时间。
- **FR-07（分词）**：`cr_kad::tokenize_keywords` 是**确定性**变换（同一名字永远同一组词，
  可复算）：非字母数字处切分、ASCII 小写、去纯数字与 <3 字符 ASCII 词、
  去容器/编码噪声词、去重保序、上限 8。CJK 连续串整体保留。
  `/cli index` 只在配置没给 `keywords` 时分词，并回报 `keywords_source`。
- **FR-05/07（去重健康度）**：`/cli kad-health` 给出
  `keys_total / keys_unique / duplicate_keys`（逐 key 列出被几条条目发出）、
  `keywords_total|unique`、`tokenized_entries`、`due / expired / expiring_soon`、
  `refresh_failures`。D 段实测：两条共享 `cold`/`revive` 的条目 →
  `keys_total=10`、`keys_unique=8`、`duplicate_keys=2`。
- **FR-25 上传方向（仍 BLOCKED，但已定位）**：新增
  `tests/e2e/w25-daemon/diag_upload.py`（透明 TCP 代理 + 旁路重放 aMule 自己的块记账）。
  一次典型停顿的读数：aMule 停在 38.6%，`requests=7`（21 对区间）、`sends=145`、
  完成 24 块、`dropped_exceeds_boundaries=0`、`dropped_no_pending_block=0`；
  aMule 侧 `ss` 显示内核 Recv-Q 留 ~493 KB 不动、状态仍是 `Downloading`，
  约 100 s 后 `DOWNLOADTIMEOUT` 才把它打回 `Waiting`。
  - **我们的帧没有一处违反它的边界规则**（0 越界、0 无主），所以问题不在帧格式。
  - aMule 停止读取后我们阻塞在 `send_frame` 的写里，看门狗轮不到执行 → 就此冻结。
  - 对端无法进一步取证：`AddDebugLogLineN` 在 release 构建里是空宏，
    `VerboseDebug=1` 产不出帧级日志（二进制里 `exceeds requested` 字符串计数为 0）。
    不重建 debug 版——那会换掉被测客户端。
  - 据此修了两处真实缺陷（见 CHANGELOG）：写操作 10 s 上限（不再钉死线程）、
    idle 重传看门狗（对端静默 ≥3 s 重发它最后请求的区间，封顶 2× 文件大小）。
    实测同一链路 10.1% → 38.6%（1.48 MB → 4.25 MB、完成块 9 → 24），
    **能缩短停顿，不能阻止对端再次停读**，所以上传方向仍是 BLOCKED。
- 口径未变：`g2_claim=false`、`s6_status=not_run`、`evidence_valid_for_pass=false`、
  `auto_pass=false`；FR-24 公网检索仍等 G0 `protocol_scope` 书面放宽。

## 11. FR-15 第五条阈值 / FR-07 可复算同义词与语言识别（2026-11-22，v2.4.102）

**结论：FR-15 五条阈值全部接线；FR-07 的同义词与语言识别在不牺牲可复算性的前提下补齐。
门禁常量与公网口径不变。**

- 证据：`python tests/e2e/p0/run_p0.py` → `tests/e2e/p0/reports/report.json`
  **77/77**（v2.4.101 为 62/62；新增 F 段 CPU、G 段同义词/语言/publish）。
  全量回归 **426 passed / 0 failed / 3 ignored**（原 419）。
- **FR-15 CPU 阈值**：`limits.max_seeding_cpu_millipct`，单位千分百分点
  （1000 = 单核 1%），0/缺省关闭。Linux `/proc/self/stat` 的 `utime+stime`
  除以 `sysconf(_SC_CLK_TCK)`；Windows `GetProcessTimes`；其它平台 `None`。
  - **读不到 ≠ 0**：`/health.limits.max_seeding_cpu_millipct` 报
    `enabled=false` + `unavailable`，该条不参与准入。已知值在短暂读失败时保留。
  - 进程级、多线程求和可超过 100%，包含 HTTP/CLI 的 CPU；因此只当
    **拒绝新唤醒的滞后理由**，不承诺「做种永不超线」。
  - 拒绝顺序固定：hot → peers → bandwidth → rss → cpu。ACK 带
    `limit_name=max_seeding_cpu_millipct`、`limit`、`current`（均为 milli-percent）、
    `limit_unit=milli-percent`。
  - e2e F 段：`limit=1`（0.001%）、实测 `current=1600`（1.6%）→
    `REJECT_RESOURCE_LIMIT`，`hot=0`、`seed_active=0`；不配的阈值仍显式
    `enabled=false`，不是字段缺失。
- **FR-07 确定性同义词**：
  - `LATIN_SYNONYM_GROUPS` / `CJK_SYNONYM_GROUPS` 是封闭手写表；不用词库/模型/随机数，
    所以同一名字永远得到同一组词，可直接复算。
  - `expand_keywords` 用 `token_language` 按**每个词自己的脚本**选表，
    避免「中文标题 + 英文标记」被整体判成一种语言后选错表。
    原文全保留且排最前，同义词按组内固定顺序追加，去重保序，硬截 8。
  - 扩展只进 `published_keys`；`FileEntry.keywords` 原样保留，区分
    「文件名真有的词」和「系统补的词」。
  - `detect_language` 仅展示/审计：多脚本报 `mixed`；纯汉字（无假名）判 `zh`，
    不假装能区分中日汉字。
  - `.bin/.dat/.txt` 等扩展名加入 `NOISE_TOKENS`，否则「中文标题 + .bin」
    会把语言判定污染成 `mixed`。
- **`/cli publish`**：补上此前「index 写了 key 却永远不标记已发布」的缺口。
  真发 `KAD_PUBLISH`，**只有 `KAD_PUBLISH_ACK` 才**置 `kad_published`
  并调用 `mark_published` 前推 TTL；失败只累加刷新失败计数。
  ACK 只表示对端节点收下 key，**不表示公网 KAD 可检索**。
- e2e G 段实测：英文条目 `keywords=[movie, film, collection]` →
  `keywords_expanded=[..., cinema]`、`language=en`；中文条目
  `keywords=[冷启动复活, 电影, 测试样本]` → 扩展出 `影片/片子`、`language=zh`；
  重索引逐项一致；`cinema` 的 SHA-1 key 出现在 `inspect kad` 已发布列表里；
  `public_search` 仍为 BLOCKED（行政口径已于 2026-11-22 解除，见 §12）。
- 口径未变：`g2_claim=false`、`s6_status=not_run`、`evidence_valid_for_pass=false`、
  `auto_pass=false`；FR-24 公网检索仍等 G0 `protocol_scope` 书面放宽（已于 2026-11-22 解除，见 §12）。

## 12. G0 scope 解除与剩余欠账总账（2026-11-22，项目所有者决定）

项目所有者在本会话明确指令「解除」，并按「全部剩余欠账」继续推进。
机读决定：`tests/spikes/spikes.json` → `g0_review.protocol_scope_relaxation_2026_11_22`；
说明：[G0-REVIEW §12](G0-REVIEW.md)、[OPEN-QUESTIONS §3](OPEN-QUESTIONS.md)。

- **FR-24 行政阻塞销账**：公共 KAD `0x33`/`0x43` 索引操作不再因 G0 scope 记 BLOCKED。
  这不等于 FR-24 完成——生产实现、真实公网节点证据、R-02/R-11 仍开放。
- **本轮工作清单**（按 DOC-29 §7 顺序展开）：FR-24 公网检索；FR-25 上传跨分片 /
  server / source exchange / credit / AICH / 双轨发布 / 公网证据；P1 的 QoS 接数据面、
  NAT 自动映射、Mainline DHT 启用、`rpc-secret`、Web UI/WebSocket；P2 的 OpenAPI、
  跨协议补源、信誉行；治理项的 SRS 评审状态与 FR-24 编号去重、证据可移植性、
  公平性阈值与拥塞语义。
- **不因本决定变化**：`g2_claim=false`、`s6_status=not_run`、
  `evidence_valid_for_pass=false`、`auto_pass=false`；限速 0.2 pps、最小样本、
  禁止批量发布测试 Key；脚本不得自动 pass；缺证据即 BLOCKED/FAIL。
- 上方 §2–§11 的历史口径按决策演化链保留，不与本条冲突时以本条为准。

## 13. P1/P2 接线与 FR-25 有界在途落地（2026-11-22，v2.4.104）

**结论：§3 的 P1 五项与 §4 中可代码化的 P2/OpenAPI 项已接线（全部 opt-in、带测试）；
FR-25 上传侧获得有界在途与真实编解码，但真 aMule 闭环仍未取得；门禁机器常量不变。**

- 验证：全量 `cargo test --workspace --offline` **480 passed / 0 failed / 3 ignored**；
  P0 e2e **77/77**；新增 `tests/e2e/p1/run_p1.py` **5/5**；`cr-ed2k` **53/53**。
- **P1 已接线**：`rpc-secret`（写方法鉴权、`/health` 不回显）、Web UI / WebSocket
  （opt-in、不遮蔽既有端点）、OpenAPI、QoS 真实 `upload_bps` 硬上限、NAT `auto_map`
  （真调 UPnP/NAT-PMP）、Mainline DHT daemon enable（真 announce 回查，无 token 如实 BLOCKED）。
  配置字段见 [CONFIG §2.2](../04-protocol/CONFIG.md)。
- **P2 状态**：OpenAPI 已落地；跨协议补源与 FR-26（原重复 FR-24）信誉仍是
  `not_implemented` 状态接口，不伪造来源/分数。
- **FR-25 上传侧**：`ServeConfig` 以请求回合为信号给在途字节设上界，
  `deferred_bytes` 保留半区间进度；Windows `WouldBlock` 分类修复让 idle 看门狗真跑；
  `codecs.rs` 真编解 `0x44/0x34/0x4B/0x3B`，server login/credit/AICH 明确 Unsupported。
  CLI/daemon 暴露 `--max-in-flight-bytes` / `--request-gated-pacing`（未验证假说）。
  **真 aMule 完整文件 md5 闭环仍未取得**，上传方向继续保持 BLOCKED。
- **治理**：SRS v2.1 已评审、重复 FR-24 → FR-26；G0 scope 已书面解除（§12）。
  R-02 / R-11 仍按 M4 前复核；证据可移植性（gitignored reports）与公平性阈值/
  可靠层拥塞语义仍开放，不以本轮数字宣称 G2 达标。

## 14. FR-24 公网 0 命中根因与生产工具修正（2026-11-23，v2.4.105）

**结论：此前「BOOTSTRAP_RES 成功但 SEARCH_RES 0 命中」的机制原因已定位并修入生产工具；
剩余 0 命中来自另一类边界（在区节点拒绝/无法接收索引操作），仍按真实证据登记，不宣称 FR-24 公网命中通过。**

- **根因**：直接向 bootstrap 节点发 `0x33`。aMule 的索引操作有
  `SEARCHTOLERANCE = 2^24` 责任区校验（S2 PLAN §6/§20），越界静默丢弃；
  >200 B 的应答可能是 `0xE5` zlib 帧，旧工具只解 `0xE4`。
  这与 HELLO/RC4、换网、防火墙行政限制都无关——G0 scope 已于 §12 书面解除。
- **代码**：`cr-kad` 提升零依赖 zlib 解码器与联系人/`0x21/0x29`/责任区原语，
  并加字节级 `0x43` 发布编码回归（对 aMule 已接受帧逐字节一致）；
  `coldrevive-ed2k search --public` 先迭代收敛，只对已验证在区节点发检索；
  `--publish-file` 做一个唯一 key 的最小样本发布（最多 3 节点、0.2 pps）。
  `evidence_valid_for_pass` 改为恒 false。
- **证据边界**：`public-lookup-20261123-ubuntu.json` 36 发 `0x21` / 33 收 `0x29` /
  33 已验证 / 1 在区（`top_chunk=8,875,008`）；该节点 `0x33` 仍超时，符合
  S2 §20.5「能问路但 UDP 入站不可达/拒绝索引操作」那一类边界。
  更宽收敛（`wide`：80 发 `0x21` / 72 收 `0x29` / 仍 1 个在区节点）与新鲜联系人
  （`fresh`：2 跳找到 2 个在区节点）轮次对 `0x43`/`0x33` 都静默，
  `ack_count=0`、`answer_count=0`。未取得真实 `SEARCH_RES` 前 FR-24 公网命中保持
  BLOCKED；下一判别实验应在有真实公网 UDP 入向的主机（如 221）重跑同一工具。
- **FR-25**：7 份透明代理报告重放 aMule 块记账，`dropped_exceeds/dropped_unknown`
  恒 0，对端 Recv-Q 停住而状态仍 `Downloading`。新增诊断性 `FR25_PACE_BPS`
  做限速对照，用来区分帧格式错误与对端接收缓冲/消费速率问题；
  生产 `serve` 语义未因此改变，真 aMule md5 闭环仍 BLOCKED。
- 验证：全量 `cargo test --workspace --offline` **486 passed / 0 failed / 3 ignored**；
  P0 e2e **77/77**；P1 e2e **5/5**。门禁机器常量不变。

## 15. 真机判别实验：FR-24 根因再定位（2026-11-23，221 真机）

**结论修正**：此前 §14 把公网 0 命中归因为「在区节点 UDP 入站不可达/拒绝索引」，
本轮真机对照**证伪**了这一归因——公网 KAD 本身可用，缺口在我们只会发明文帧。

- **主机/网络**：221 `192.168.1.221`（出口 `112.5.60.4`）；STUN 判定
  `cone_or_no_nat`（多个服务器一致映射到 `112.5.60.4:10193`），
  `UPnP IGD=False / NAT-PMP=False`。本机 A 出口同为 `112.5.60.4`（同一 NAT）。
  amuled `Kad: Connected (firewalled)`，UDP 4672 监听正常。
- **决定性对照**（同一台 221、同一 NAT）：
  - 真 aMule `amulecmd search kad ubuntu` → **361 个结果**（eD2k Not connected，纯 KAD）。
  - 真 aMule `search kad fr24-after-restart`（我们自造 key）→ 0（靶子本就不在公网索引）。
  - 我们的明文工具 `search --public --term ubuntu` → 收敛 60 发 `0x21`/55 收 `0x29`/
    55 已验证/1 在区（`tolerance_zone_reached=true`），但 `0x33` 静默、`answer_count=0`。
  - 我们的明文工具直连本地 aMule（`--bootstrap 127.0.0.1:4672 --no-lookup`）→ 0。
- **抓包根因**：`tcpdump` 证实 aMule 的 `0x33/0x3B` 全是 **RC4 混淆帧**
  （`CryptoKadUDPKey=2738379211`），payload 首字节随机、无明文 `0xE4` 头；
  BOOTSTRAP_RES 是 `0xE5` zlib，仍为明文可达。所以「能 BOOTSTRAP/问路、索引操作静默」
  的原因是**我们只发明文，混淆节点直接丢弃**。
- **混淆实现与验证**：把 S2 `wire.rs` 的 `obfuscate/deobfuscate`（RC4 + MD5
  `StoreCryptValue(targetKadID)` + senderVerifyKey）提升为 `cr-kad::kad_obfuscate`
  （25/25 单测），并在 `coldrevive-ed2k search` 对已知 KadID 的 `0x33/0x43` 启用 try-0
  封装。用 `BOOTSTRAP_RES` 解出的本地 KadID 作 key，**自解信封 magic 与 inner 帧逐字节正确**
  （`e4 33 <target16> 0000`）。
- **仍未闭环**：混淆帧发到真 aMule（本地 + 公网在区节点）**仍无应答**。
  查证 S2 历史：`kad-wan-index-probe-20261117084125.json` 的 `--obfuscate` 也是
  `search_responded=false / publish_acked=false / verdict=wan_publish_silent_in_zone`——
  即 S2 的混淆实现**从未被真 aMule 接受过**，是自洽但未验证的假说。
  aMule 收到我们的 try-0 帧后不回任何包（无 HELLO 请求），说明还缺
  `HELLO`/receiver-verify-key 握手或 marker/verify 字段的精确语义。
- **门禁口径不变**：`evidence_valid_for_pass` 恒 false；FR-24 公网真实命中仍 BLOCKED，
  不外推 LAN/真 aMule 结果。下一步见 §16。

## 16. HELLO/verify-key 接线与混淆字节序再定位（2026-11-23，v2.4.107 工作树）

**结论**：HELLO/HELLO_RES_ACK 与 receiverVerifyKey 派生已接进 CLI；源码核对又抓到两个
真缺陷（magic/verify-key 字节序、try-0 双向 RC4 base 不对称），已修并有 aMule 收包语义
回归。**真 aMule 的 try-2 HELLO 已在 LAN 成功**（`receiver_key_verified=true`），
但 try-0 仍不回包，且公网索引命中未取得，FR-24 公网命中继续 BLOCKED。

- **新增协议语义**（源码 `EncryptedDatagramSocket.cpp` / `UInt128.cpp` / `ClientUDPSocket.cpp`）：
  - 收包 `DecryptReceivedClient`：try-0 base = 对端 `GetKadID().StoreCryptValue()`，
    x86 上等于线上 KadID **每 4 字节字反序**；try-2 base = `PokeUInt32(verifyKey)`（LE）。
  - 发包 `EncryptSendClient`：try-0 base = `md4cpy(clientHashOrKadID)`，即**原始线上 ID**。
    两边不对称；此前用同一个「原始 ID」函数做双向，自洽回环通过、真机静默。
  - magic 与 receiver/senderVerifyKey 在混淆头中都是 **LE 字节**（x86 的
    `ENDIAN_SWAP_32` / `ENDIAN_SWAP_I_32` 恒等）；旧代码写 BE，只有自解能通过。
  - `udp_verify_key(kadKey, ip)` 按 `CPrefs::GetUDPVerifyKey` 实现并有已知答案单测。
- **代码**：`cr-kad::kad_obfuscate` 拆出 `send_key_base` / `receive_key_base`，
  修正 magic/verify-key 字节序；`coldrevive-ed2k` 新增 `--hello` / `--hello-try2` /
  `--hello-plain` / `--peer-udp-key` / `--our-public-ip`，HELLO_RES 请求 ACK 时回
  `HELLO_RES_ACK`（用回包 senderVerifyKey 做 try-2 信封）。
- **真机证据**：221 本地明文 `--hello-plain` **成功**（`rtt_ms=31`、
  `obfuscated_response=true`、解出 peer_id/version/ACK 标志），说明 HELLO 载荷正确、
  aMule 可回混淆帧且我们的解包正确。抓包中 try-0 帧为
  `4034120c…`（49 B），按 aMule 收包语义离线解密得到 magic `0x395F2EC1`、
  `rvk=3224252923`、`svk=OUR_UDP_KEY`、内帧 `e4 11 <ourid> …`，
  **字节级合法但 aMule 未回 HELLO_RES**（洪泛窗口外重试仍如此）；说明真实节点
  对 try-0 base 的精确语义仍与我们建模不同，不能靠自洽解密代替对端验收。
- **突破**：`--hello --hello-try2 --peer-udp-key 2738379211 --our-public-ip 192.168.1.210`
  对同一真 aMule **成功**（`local-hello-try2-verified-20261123.json`）：
  `hello_envelope=try2`、`receiver_verify_key=3224252923`、`rtt_ms=31`、
  `obfuscated_response=true`、`peer_id=27d3ccfa…`、`receiver_key_verified=true`。
  这是首次真 aMule 接受我们的**混淆**握手；`receiver_key_verified` 只在 try-2
  真换回 HELLO_RES 时置真，参数齐全但 try-0 失败时保持 false。
- `cr-kad` 相关单测 **9/9**（含真 aMule HELLO_RES 解密向量、aMule 收包语义的出站帧校验）；
  `cr-kad + coldrevive-ed2k` **34 passed**。证据文件：
  `tests/e2e/w24-pub/reports/local-hello-plain-cli-20261123.json`、
  `local-hello-try0-*.json`、`local-hello-try2-verified-20261123.json`。
  **公网边界不变**：try-2 需要预先知道对端 KadUDPKey 与我们被看到的公网 IP；
  目前只有 LAN 靶机具备这两个输入，公网节点仍无真实 `SEARCH_RES`。

## 17. FR-25 原版 aMule A/B 上传对照（2026-11-23）

**结论**：按「同链路、同客户端、唯一新样本、先 Cancel 旧项」做了 unpaced / paced 两轮；
两轮都停在 `STALLED_WITH_PENDING_BLOCKS`，对端收下字节但不再消费。
`FR25_PACE_BPS=200000` 把完成块从 11 提高到 21（12.7% → 36.6%），但没有闭环。

- A（无 pacing）：`upload-diag-20261123100705.json`，`amule 12.7%`、
  `bytes_sent=5,844,992`、`blocks_completed=11`、`blocks_pending=6`、
  `dropped_exceeds=0`、`dropped_no_pending=390`、`Recv-Q=4,070,974 B`。
  390 个 `send_no_pending` 来自 idle 重传看门狗在块已完成后继续重发最后请求区间；
  Recv-Q 累积到 4 MB 说明字节进了内核缓冲但对端应用未消费，**不是帧越界**。
- B（`FR25_PACE_BPS=200000`）：`upload-diag-20261123100746.json`，`amule 36.6%`、
  `bytes_sent=3,766,272`、`blocks_completed=21`、`blocks_pending=6`、
  `dropped_exceeds=0`、`dropped_no_pending=0`、`Recv-Q=133,336 B`。
  限速消除了无主重传、进度更快，但仍在 36.6% 停住。
- 判据保持：只有对端落地 md5 一致才算 FR-25 上传闭环；两轮都没有。
  生产 `ServeConfig` 的有界在途/限速是 opt-in 诊断，不宣称公网性能。

### 17.1 第二上传端反向 A/B（2026-11-24，用户明确确认后执行）

**结论：上行停滞不是 aMule 下载端「普遍不消费」，而是我们的上传帧形态与真 aMule
不一致。** 同一样本、同一下载端（221 `amulecmd:4712`）下，第二台 stock amuled
（221:4664，KAD/server 关闭）37.5 s 落地 md5 一致；我们的守护进程仍停在 97.6%。

- A：我们的 daemon → 透明代理 → aMule，paced 200 kB/s，`upload-diag-20261124084819.json`：
  `verdict=STALLED_WITH_PENDING_BLOCKS`，97.6%，`bytes_sent=9,910,272`
  （文件 9,736,192 B），968 blocks，`idle_resends=0`、`deferred_bytes=0`，
  代理侧 14 个 pending 块不再收到新的 `REQUESTPARTS`，最终 `write ... 10060`。
- B：同一 ed2k link + `sources,192.168.1.221:4664` → 同一 aMule 下载端，
  `reverse-ab-20261124084814.json`：37.5 s，`landed_md5` 与样本一致，
  第二 amuled 的 TCP 4664 有真实 ESTAB。
- 帧级对照（唯一新样本，避免 aMule 复用已完成文件）：
  `stock-upload-capture-20261124085257.json` +
  `run/stock-upload-frames-20261124085257.log`。Stock 上传端发
  **`OP_COMPRESSEDPART(0x40)` / protocol `0xC5`**，每帧约 1,070 B zlib 数据对应
  184,320 B 块；下载端持续以 3 块滑动窗口发 `REQUESTPARTS` 直到完成。
  我们的栈发 `OP_SENDINGPART(0x46)` / protocol `0xE3`、10,240 B 未压缩帧。
- 因此 §17 早期「blocker 在对端消费逻辑、不是帧格式」的推断**过宽**：
  对端能消费 stock 帧；差异落在我们的帧 opcode/压缩格式/粒度。
  **FR-25 上传方向当时仍 BLOCKED**；§17.2 记录同构帧实现后的闭环结果。
  门禁常量与 `evidence_valid_for_pass=false` 不变。

### 17.2 同构 OP_COMPRESSEDPART 后的上传闭环（2026-11-24，v2.4.114）

**结论：FR-25 上传方向在 RFC1918 LAN 上闭环——判据是对端落地 md5 一致。**
同一样本、同一下载端、两轮 A/B（paced 200 kB/s 与 unpaced）都 PASS：

- A：`coldrevived ed2k-serve --compressed-parts` → 透明代理 → 221 aMule。
  `reverse-ab-20261124101956.json`（unpaced，落地 `3a22401f…64b7`）与
  `reverse-ab-20261124102018.json`（paced，落地 `10016b4b…1e5c`）中
  `arm_a.md5_matches=true`；`upload-diag-20261124101959.json` /
  `upload-diag-20261124102021.json` 给出 `verdict=PASS`。
- B：第二台 stock amuled 在两轮都落地同一 md5，链路与 §17.1 相同。
- 帧格式来自真 stock 抓包 `run/compressed-frames-20261124.hex`：54 帧、
  `hash(16)+start(u32 LE)+compressed_len(u32 LE)+完整 zlib 流`、`PROT_EMULE 0xC5`，
  解压后与样本逐字节一致。aMule 源码 `CreatePackedPackets` 仅在压缩确实变小时
  使用 0x40，否则回退 0x46；`unzip` 支持流式 inflate 与 `Z_STREAM_END`。
- 中间失败路径已如实归档：stored DEFLATE（帧比原块大）整文件 A/B 在
  89.1% / 26.6% / 15.2% 停住；「3 轮 + 2 s」重传抑制会导致同一批块永久饿死。
  修复为「请求即重发」，并以 `congestion.rs` 的重复请求测试锁定。
- **边界**：证据仅在 RFC1918 LAN + 真 aMule 2.3.3 release 构建；`--compressed-parts`
  为 opt-in；公网 server/peer 与 G2/S6 门禁均未因此改变。

## 18. FR-24 跨公网 try-2 HELLO 验证与索引静默再定位（2026-11-23，v2.4.108）

**结论**：把两个真实工程缺陷修掉之后，try-2 混淆 HELLO 首次在**两个不同公网出口
之间**（222 `112.5.32.34` → 221 `112.5.60.4:4672`）拿到真 `HELLO_RES`
（`receiver_key_verified=true`）；但 `0x33`/`0x43` 索引操作仍 0 ACK、0 命中，
FR-24 公网检索继续 BLOCKED。`evidence_valid_for_pass` 恒 false。

- **缺陷 A（HELLO 端口）**：`kad_hello` 把本地 socket 端口写进 `TAG_SOURCEUPORT`；
  222 的 CPE 把本地 `46057` 映射为外部 `2078`，aMule 却按载荷回 `112.5.32.34:46057`
  ——该端口没有 NAT 映射，应答永远到不了本机。修法：`--stun` 在**同一条 KAD socket**
  上取映射 IP/port，或显式 `--our-public-port` + `--bind-port`。
- **缺陷 B（对称 NAT）**：STUN 服务器给的是 `112.5.32.34:2055/2056`，KAD 目标看到的
  却是 `2080`（222 的映射随目的变化）。所以 STUN 端口不能替代「对端可见端口」；
  实验上用固定本地口 `40100` + 对端抓包观测的 `2081` 才闭环。
- **真机证据**：`public-path-plain-20261123.json`（BOOTSTRAP_RES 300 ms、
  双侧抓包源为公网 IP）、`public-try2-hello-verified-20261123.json`
  （HELLO try-2 `receiver_verify_key=141047448`、`rtt_ms=868`、
  `obfuscated_response=true`、`receiver_key_verified=true`）。
- **索引仍静默**：同一条已验证 try-2 链路上，`--index-envelope` 三种信封
  （try2 / try0+rvk / plain）都拿不到 `0x4B` ACK 或 `0x3B` 应答；
  `--key-hex` 贴键使 `top_chunk=0`，已排除 `SEARCHTOLERANCE=2^24` 越界分支。
  证据：`public-try2-index-silent-20261123.json`、
  `public-index-try0-variant-20261123.json`、
  `public-index-plain-variant-20261123.json`。
  **剩余 blocker 在 aMule 的索引接纳/节点状态**（例如 UDP-verified 联系人门槛），
  不是信封、NAT 端口或责任区；不要再靠换信封/加发包频率尝试。
- **代码**：`src/bins/coldrevive-ed2k/src/main.rs` 新增 `parse_key_hex`、
  `stun_on_kad_socket`、`--key-hex` / `--stun` / `--bind-port` /
  `--our-public-port` / `--index-envelope`；复用 `cr-nat::stun`；报告新增
  `key_source` / `local_udp_port` / `advertised_udp_port` / `stun_mapped` /
  `index_envelope`。全量 `cargo test --workspace --offline` **499 passed / 0 failed / 3 ignored**。

### 18.1 更正（2026-11-23，v2.4.109）：§18 的「索引接纳/节点状态」归因错误

**上面 §18 末尾「剩余 blocker 在 aMule 的索引接纳/节点状态」是错误推断，已被源码与干净 A/B 推翻。**
对照 aMule 2.3.3 `KademliaUDPListener.cpp` / `Indexed.cpp` / `PacketTracking.cpp`：

- `Process2SearchKeyRequest`（0x33）**没有**责任区校验，也不要求接触已验证；
  它直接调 `SendValidKeywordResult`，而后者**只在接收节点本地 `m_Keyword_map`
  命中该 key 时**才发 `KADEMLIA2_SEARCH_RES`（0x3B），否则静默。
  §18 用的 `--key-hex 27d3...da17` 是贴着对端 KadID 造的合成 key，**不在任何节点索引里**，
  所以 0x33 静默是协议正确行为，不能当「被接纳门挡住」的证据。
- `Process2PublishKeyRequest`（0x43）只在两处静默 `return`：
  ① **接收节点自己** `IsFirewalledUDP(true)`（防火墙节点不索引、也不给假 ACK）；
  ② 目标 key 超出接收节点 `SEARCHTOLERANCE=2^24` 且源不是 LAN IP。
  只有 publish 有责任区门，search 没有——v2.4.105 把 0x33 也限在区内是过度推广。
- `InTrackListIsAllowedPacket(ip, opcode, /*bValidSenderkey*/)` 的 `bValidSenderkey`
  **被注释掉、完全不参与**；只是每 IP 每 opcode 的洪泛限速（0x33/0x43 各 3/min，
  超过 15/min 封 IP）。0.2 pps 远低于此，不构成阻断。

**干净 A/B（同目标 `112.5.60.4:4672`、同公网路径、不同日期/节点状态）**：

| 证据 | 日期 | aMule 状态 | 结果 |
|---|---|---|---|
| `tests/e2e/w51g/reports/run-20261117T085411Z.json` | 2026-11-17 | 未 firewalled | 7 包闭环：PUBLISH_RES(19 B) + SEARCH_RES(591 B)，`publish_acked=true` |
| `reports/public-firewalled-recheck-20261123.json` | 2026-11-23 | `Kad: Connected (firewalled)`、`key_index.dat` 28 B 空 | `wan_publish_silent_in_zone`、`publish_acked=false` |

同一 `kad-mini wan-index-probe`、同一贴键（`top_chunk=0` 已排除责任区），
**唯一变量是目标的 UDP 防火墙状态** → 静默由目标状态单独解释。

**修正后的 FR-24 口径**：

- 真实公网 PUBLISH_RES + SEARCH_RES 闭环**已被 w51g 历史报告证明可达**；
  不是「从未成功」。今日 0 命中是**被测目标临时 firewalled + 空索引**，不是实现缺陷。
- 不要再换信封、加发包或改责任区；要复现闭环，先把 221 的 aMule 恢复到非 firewalled
  （重开 UDP 入向 / 确认 UPnP），再跑同一 probe。
- CLI 新增诊断旗标 `--search-closest`：在无可信在区节点时改取最近已验证节点（≤3、
  0.2 pps、`evidence_valid_for_pass=false`），用来测真实关键词；本次对 `ubuntu`
  仍 0 命中（`reports/public-real-keyword-closest-20261123.json`），与目标 firewalled 一致。
- G1/G2 门禁常量、`evidence_valid_for_pass=false`、s6_status 均不变；不外推为公网检索通过。

### §18.2 防火墙状态恢复尝试（2026-11-23，用户确认后执行）

按 v2.4.109 的下一步建议，备份 KAD 状态后重启 221 的 aMule（`systemctl restart cr-amuled3`）
以触发新的 UDP 防火墙测试：~15 s 内再次声明 `Kad: Connected (firewalled)`，
60 s 窗口内未变化；`amulecmd` 的 `set` 只支持 BwLimit/IPFilter，没有强制重测命令。
证据 `reports/firewall-remediation-attempt-20261123.json`。

同时排除了「入站 UDP 被墙」这一解释：222 与独立公网中继 `82.157.68.159` 分别向
`112.5.60.4:4672` 发 2 字节 `KAD2_BOOTSTRAP_REQ`，都收到 521/523 B `BOOTSTRAP_RES`；
重启期间 40 s 抓包 95 个 UDP 包，aMule 与大量公网节点正常收发 KAD 帧
（51 B 请求、85/159/310 B 应答）。所以 firewalled 来自 aMule 自身的 UDP 防火墙测试，
不是入口路径或 KAD 收发失效。中继主机只开放 UDP 7100，无法作为无 NAT 的公网探针主机。

FR-24 口径不变：历史 w51g 报告已证明真实公网闭环可达；要今日复现，需要一个
**非 firewalled 且本地索引含目标关键词**的节点，或让 221 的防火墙测试拿到一次成功回包
（外部端口发现/CGNAT 映射需另行排查）。不要再换信封、加发包或改责任区。

### §18.3 非 firewalled 目标上的闭环复现（2026-11-24，v2.4.111）

用户提供了 209（`192.168.1.209`）上的真 eMule（非 firewalled）：KAD UDP 41296、
`KadUDPKey=1906084797`、KadID `dddb80f83a3baf426ec305266b4760b3`（HELLO_RES 报版本 10），
`key_index.dat` 204,629 B 非空，公网出口同为 `112.5.60.4`。用该 KadID 生成在区贴键
`dddb80f83a3baf426ec305266b4760b4`（`top_chunk=0`）：

1. **221→209 LAN**：try-2 HELLO `receiver_key_verified=true`；`--publish-file` 得
   真实 `0x4B` ACK（`ack_count=1`）；同一 `0x33` 得 **1 条 `0x3B` SEARCH_RES**，
   file_id `5ae257c47e9be1243ee32aabe408fb6b`、文件名、1024 B 与 ed2k link 全部匹配。
   证据 `reports/public-209-lan-loop-20261124.json`。
2. **222→112.5.60.4:41296 真公网路径**：HELLO 因 222 对称 NAT 可见端口不确定而超时，
   但 publish ACK 仍为真、SEARCH_RES 仍为 1 条且 tag 可解析
   （`reports/public-209-public-loop-verified-20261124.json`）。无 HELLO 的对照为
   0 ACK / 0 命中（`reports/public-209-public-nohello-20261124.json`）。

结论：**FR-24 的 publish→search 技术闭环在真 eMule 上已复现**；此前 221 的静默确由
目标临时 firewalled + 空索引解释。门禁常量与 `evidence_valid_for_pass=false` 不变，
不把诊断探针外推为 G2 通过。目标配置见 `reports/public-209-target-profile-20261124.json`
（不含凭据）。

## 19. G2 欠账推进复核（2026-11-24，v2.4.116）

**门禁口径不变**：`g2_claim=false`、`s6_status=not_run`、`evidence_valid_for_pass=false`、
`auto_pass=false`。本节只记录实现与组件级证据，不宣称任何门禁解禁。

| 欠账 | 本轮结果 | 仍缺什么 |
|---|---|---|
| 可靠层拥塞语义 / 公平性 | `StreamCore` 实现 AIMD、RTT 派生 RTO 与 opt-in pacing；中继增加每会话字节桶；4+2 条确定性单测通过并归档 | **共享真实瓶颈 + 竞争流 + 预注册 Jain/份额阈值 + 多轮原始样本**；当前只有机制与虚拟时钟/回环量具，不能写「公平性达标」 |
| DHT 产品化治理 | daemon 可 stop；有界 `find_node` 越过不发 token 的引导节点；每轮预算/间隔/key 上限/RSS 软跳过；`/health.dht` 非黏性；I/O 移出状态锁 | 自动 DHT 公网可达率、NAT 矩阵、资源长期运行数据；FR-18 证据仍不构成 S3/S6/G2 |
| TCP 打洞接 BT 数据面 | 验完的 punched socket 已登记进 SOCKS5 桥，真 hello/world 往返；`/health.data_plane.punched_registered/punched_alive` 分开计数 | 公网双侧仍 BLOCKED：221/222 各 34 次 SYN 静默丢弃，19910/19920 无真实 TCP DNAT，222 对称 NAT。需要所有者提供 CPE TCP DNAT 后重测，不能改判据 |
| FR-25 公网传输 | 222→221 公网下载 262,144 B 落地一致；221→222 跨分片 9,736,192 B（>PARTSIZE）落地一致。补上下载端 `0x40` zlib inflate（`cr-ed2k::inflate`）后才闭合 | 生产 daemon 路径的公网 A/B（本轮是 CLI 组件级）、非 `sources` 的自动发现仍 `not_implemented`；不作 G2 证据 |

证据均按 `tests/ARCHIVE/TEST-RUNS-2026-11-24.md` 登记，包括失败与 BLOCKED：
`public-tcp-punch-20261124.json`（双侧 34 次 SYN）、
`fr25-public-download-shared-20261124.log`、`fr25-public-upload-{server,client}-final-20261124.log`、
`workspace-test-20261124-v24116-final.log`（519 passed / 0 failed / 3 ignored）。
所有脚本仍禁止自动 pass；下一次 G2 复评必须是所有者基于真实证据的书面决定。

## 20. G2 欠账推进第二轮复核（2026-11-24，v2.4.117）

**门禁口径仍不变**：`g2_claim=false`、`s6_status=not_run`、`evidence_valid_for_pass=false`、
`auto_pass=false`。本节记录证据与反例，不构成解禁。

### 20.1 公平性：按预注册判据实测，整体 FAIL

先在 `tests/fixtures/g2-fairness-thresholds.toml` 写入判据（时间戳
`2026-11-24T14:55:21+08:00`，sha256 `0671a173…`）并单独提交（`25e4f64`），
之后才采集任何样本。判据：n=2、3 s 预热 + 10 s 稳态、每 arm 10 轮、
Jain ≥ 0.95、两条流稳态份额均在 [0.45, 0.55]，且 `all_rounds_must_pass`。

实验用 221 的 veth + HTB + netem（L1 16 Mbit/20 ms、L2 8 Mbit/50 ms），
真 `coldrevive-fairness-probe` 跑两条独立 `UdpRelayStream`（各含独立 ACK 接收端），
4 arms：cc_off / cc_on / cc_paced / cc_on_per_session_cap（cap=链路速率 1/8，
确保令牌桶真触发）。原始 645 个文件、7.8 MiB 全量保存在
`tests/e2e/w61/reports/g2-fairness-20261124T073344Z/`。

| shape | arm | 达标轮 | 结果 |
| :--- | :--- | :--- | :--- |
| L1 | cc_off / cc_on / cc_paced | 10/10、10/10、10/10 | PASS |
| L1 | cc_on_per_session_cap | 2/8 有效（另 2 轮超时） | FAIL |
| L2 | cc_off | 10/10 | PASS |
| L2 | cc_on / cc_paced | 9/10、9/10 | FAIL |
| L2 | cc_on_per_session_cap | 4/10 | FAIL |

按预注册聚合规则**整体 FAIL**。不能挑 L1 达标或「多数轮达标」改称公平性通过；
也不能把每会话令牌桶限流冒充协议退让。中途发现的量具缺陷（接收方过早 CLOSE、
`data-rate=0` 被夹成 1 B/s、cap 高于自然速率、CRLF、分析脚本 f-string）均已归档后修正，
阈值始终未改。机制实现仍只有 `nat.udp_cc_*` 与 per-session cap，不能写「公平性达标」。

### 20.2 FR-25 daemon 公网闭环

`ed2k_tasks::download_cmd` 新增 opt-in `--out`：只有 `hash_matches`、`content_matches`、
`completed` 全真才写 `.part` 后 rename，结果 JSON 带 `out_path/landed_bytes`。
真 daemon `/cli` 公网 A/B（报告 `public-daemon-ab-20261124162437.json`）：

- A：221 daemon `ed2k-serve --compressed-parts` 占映射 TCP 4662 → 222 daemon
  `ed2k-download --out`：`bytes_sent=bytes_received=landed_bytes=9,736,192`，落地 md5 一致。
- B：恢复 221 真 aMule → 222 daemon 从公网 `112.5.60.4:4662` 下载 262,144 B：
  `hash_matches/content_matches/completed=true`，落地 md5 一致。
- 测试后 221 aMule 为 `active`。这是生产 daemon 路径的组件级公网证据，不等于 G2/S6。

### 20.3 公网 TCP DNAT 已建：DNAT 环回 PASS，simultaneous-open 仍 BLOCKED

所有者已在上游网关建立两条固定映射：TCP `112.5.60.4:19910` → `192.168.1.221:19910`、
TCP `112.5.32.34:19920` → `192.168.34.222:19920`（**不能**填管理网卡 `192.168.1.222`）。
本机出口与 221 公网站点同为 `112.5.60.4`，本机直连 `112.5.60.4:19910` 因网关不支持
hairpin 而超时；这不是映射失败。证据 `tcp-dnat-hairpin-crosshost-20261124.json`。

跨主机验收（真正独立出口）：222→`112.5.60.4:19910` 与 221→`112.5.32.34:19920`
都收到预期标记，221 抓包确认 SYN 到达 `192.168.1.221:19910`。随后新增
`tests/e2e/w56/public_tcp_dnat_roundtrip.py`：一端监听、一端经公网连接，两方向各做
PING/PONG，报告 `tcp-dnat-bidirectional-roundtrip-20261124.json`：两个方向 `ok=true`、
`PAYLOAD PONG` 与 `DATA PING` 同时成立，判 **PASS**。这证明 DNAT 承载真实 TCP 双向字节，
**不是** simultaneous-open 证据。

原判据重跑仍是 BLOCKED：`public_tcp_punch_probe.py` 两侧 `ok=false`，
错误为 `Connection refused (os error 111)`；报告
`tcp-punch-after-dnat-20261124T102535Z.json` 与
`tcp-punch-after-dnat-tcpdump-run-20261124T102900Z.json`。抓包
`tcp-punch-source-port-rewrite-20261124.txt` 给出确定根因：221 打洞 socket 绑定源端口
`19910`，同一 SYN（seq `1461449036`）到达 222 时源端口被上游 NAT 改写为 `18011`，
222 的 SYN_SENT socket 期望远端 `112.5.60.4:19910`，4 元组对不上而回 RST。
这是 NAT 不保留源端口的限制，不是链路不可达，也不是我们的握手判据错误。

要使 §20.3 原判据（`ok=true` 且 `bytes_roundtrip=true`）成立，需要上游对这两条流
保持源端口（1:1 / no-PAT 静态映射），或由所有者书面修改判据以接受 DNAT 环回证据；
我不单方改判据。门禁常量不变：`g2_claim=false`、`s6_status=not_run`、
`evidence_valid_for_pass=false`、`auto_pass=false`。

### 20.4 公平性根因修复与同阈值重跑：仍整体 FAIL

§20.1 的 cap arms 失败不是「令牌桶天然不公平」，而是实现选择造成的 RTO 反馈环：
每会话预算耗尽时中继直接 `BandwidthLimited` 拒收并丢帧，发送端只能等
ACK 缺口超时；`loss_events` 与 SRTT 一起涨到数百毫秒，cwnd 落到下限，
两条同速率流被随机退避错配成 0.4/0.6 长期失衡，L1 还有 2 轮整轮超时。
先用 STATUS 码做快重传（不算 ACK、不算丢包）仍不够；最终改为
**有界整形**：超会话预算的 DATA 进入每会话 64 帧、总 256 帧的队列等令牌，
只有队首按令牌放行才转发，队列满才带原因拒绝。计数拆为
`per_session_limited / per_session_shaped / per_session_dropped`，
`/health.data_plane.udp_cc_backpressure` 与探针 JSON 同步暴露背压次数。
冒烟：L1 0.501/0.499、L2 0.500/0.500，`dropped=0`、`backpressure=0`。

用**同一份** `tests/fixtures/g2-fairness-thresholds.toml`（sha256 `0671a173…`
未变）在 221 完整重跑 80 轮，原始报告
`tests/e2e/w61/reports/g2-fairness-20261124T085447Z/`：

| shape | arm | 达标轮 | 结果 |
| :--- | :--- | :--- | :--- |
| L1 | cc_off / cc_on / cc_paced / cc_on_per_session_cap | 10/10 各 | PASS |
| L2 | cc_off / cc_on_per_session_cap | 10/10 各 | PASS |
| L2 | cc_on / cc_paced | 9/10 各 | FAIL |

L2 cc_on 第 7 轮份额 0.430/0.570（Jain 0.981），L2 cc_paced 第 8 轮
0.433/0.567（Jain 0.983）；其余 78 轮全部达标。按预注册
`all_rounds_must_pass` 仍判 **整体 FAIL**。不再通过再次抽样「碰运气」改成通过，
也不改阈值或窗口；脚本继续写 `component-level fairness measurement only; not G2 unlock`。

全量 `cargo test --workspace --offline` 为 521 passed / 0 failed / 3 ignored（新增 2 条单测）。
所有成功、失败、BLOCKED 与日志更正均写入 `tests/ARCHIVE/TEST-RUNS-2026-11-24.md`。
下一次 G2 复评仍必须由所有者基于真实证据书面决定。
