---
doc_id: DOC-29
title: P0/P1/P2 与 M1/M2 欠账复核（2026-11-21）
category: 计划
version: 1.0
status: 有效
owner: 架构组
updated: 2026-11-21
---

# P0/P1/P2 与 M1/M2 欠账复核（2026-11-21）

> 编号 DOC-29 · 版本 1.0 · 状态 **有效** · 更新 2026-11-21
> 基线：仓库 HEAD `5e3e17f`；本日复跑 `CARGO_INCREMENTAL=0 cargo test --workspace --offline`
> **384 passed / 0 failed / 3 ignored**（12:00 GMT+8 左右，停止本地守护进程后执行）。
> 口径不变：**`g2_claim=false`、`s6_status=not_run`、默认承载 `tcp`**。
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
| M2 | G2 **未达标**，是全部阶段的唯一出口欠账：公平性只有量具、Mainline 未加入、TCP 打洞未接 BT 数据面、守护进程仍单任务、可靠层无拥塞语义、FR-24 公网 KAD 与 FR-25 跨分片仍 BLOCKED（FR-25 daemon 接线已于 2026-11-22 补上，见 §8） |

## 2. P0 欠账明细（按优先级）

| 欠账 | 当前状态 | 证据 / 说明 | 家宽能否推进 |
| :--- | :--- | :--- | :--- |
| FR-01 目录监控 | 部分 | `cr-indexer::inspect()` 一次性扫描；无 FSEvents/ReadDirectoryChangesW | 否（代码） |
| FR-02 指纹多哈希联动 | 部分 | `cr-hash` 齐备，索引只接 SHA-256/BT SHA-1 | 否（代码） |
| FR-04 持久化 | 部分 | JSON roundtrip；无 SQLite 迁移、无重启 e2e | 否（代码） |
| FR-05/06/07 KAD 发布/查询 | 部分 | CLI publish/find；daemon 无周期发布/TTL，关键词未分词，无去重健康度 | 部分：公网出口样本可辅助归因，**不能解除 G0 scope** |
| FR-09 限流 | 部分 | Challenge + nonce 去重；生产路径无按 IP 限流 | 否（代码） |
| FR-10 LRU 逐出 | 部分 | 组件级真引擎逐出已证；守护进程仍单任务 | 否（代码） |
| FR-11 握手拦截 | 已实现但有边界 | opt-in 默认 false，仅回环 TCP；µTP 不外透；W62 报告是 **11/11**（`tests/e2e/w62/reports/report.json`），不是 12/12 | 否（代码/默认策略） |
| FR-12 链接生成 | 部分 | magnet + `/torrent/` 已接线；`ed2k_link` 原语未被生产调用 | 否（代码） |
| FR-13 引擎完成回流 | 部分 | 下载/暂停/恢复可用，完成产物不回流重建索引 | 否（代码） |
| FR-14 空闲休眠 | 已实现但有边界 | opt-in、`/health.idle` 可观测；只有空闲计时，无 CPU/内存/带宽触发；休眠后 `seed_active=0` 但 KAD key 保留（W14 12/12） | 否（代码/默认策略） |
| FR-15 资源阈值 | 部分 | seed cap + 中继令牌桶；阈值自动拒绝唤醒未接线 | 否（代码） |
| FR-24 公共 KAD 检索 | 部分 | LAN 真实 aMuleD `0x33` 检索 12/12；公网 5 节点 `answer_count=0`（`tests/e2e/w24-pub/reports/public-search.json`） | **部分**：新出口可重测 0/5 是否出口相关；但根因登记为 G0 `protocol_scope`，需所有者书面放宽 |
| FR-25 标准 eD2k 传输 | 部分 | 生产栈对 aMule 2.3.3 双向 LAN 22/22；daemon 接线已于 2026-11-22 补上（§8），无 server 登录/source exchange/credit/AICH；`>PARTSIZE` **下载方向已闭环（10/10）**，上传方向仍未闭环（19.2%–85.3% 后静置） | 部分：公网 eD2k 传输可补证据，但上传跨分片闭环是代码债 |

治理债：SRS 仍 v2.0「待评审」；FR-24 在 SRS 第 43/45 行重复编号（信誉行未实现）。

## 3. P1 / P2 欠账明细

| 优先级 | 欠账 | 证据 / 说明 |
| :--- | :--- | :--- |
| P1 | FR-16 QoS | `cr-qos` 令牌桶未接 BT 做种；S5=fallback 否证 LEDBAT，不得写“自动退让” |
| P1 | FR-17 NAT 打洞/回退 | 原语在 Windows+Linux 回环可用，daemon 不自动建映射；S6 机器常量仍 `not_run` |
| P1 | FR-18 Mainline DHT | daemon 关闭 DHT；自有公网节点 announce 闭环，但 transmissionbt `token_len=0`、两个 router 超时（`tests/e2e/dht-announce/`） |
| P1 | FR-19 / FR-21 | Web UI、WebSocket 未实现（`src/crates/cr-api/src/lib.rs` 注释明确 G2 前不做） |
| P1 | FR-20 | `/jsonrpc` 子集 + CORS 有；**无 `rpc-secret`** |
| P2 | FR-22 / FR-23 / 重复 FR-24 | 无 OpenAPI；插件仅测试资产；跨协议补源与信誉/激励均骨架 |

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
| Mainline DHT | 自有节点闭环；公共引导节点无可用 token | 找到确实签发 token 的邻居，独立节点 get_peers 回查；否则维持 BLOCKED |
| TCP 打洞 | 原语通过，BT 数据面未接线，公网双侧 BLOCKED | 先解决公网双侧，再谈接线；symmetric 不可直连保留为正确结论 |
| 守护进程多样种 | 组件级 `Cold→Hot→Cold→Hot` 已证；daemon 仍单任务 | 多 infohash + cap 触发 e2e |
| FR-24 公网 | 5 节点全超时 | G0 `protocol_scope` 书面放宽 |
| FR-25 跨分片/公网 | daemon 已接 `serve`/`download`；`>PARTSIZE` 下载方向 10/10 闭环，上传方向推进到 19.2%–85.3% 后静置 | 定位 aMule 上传侧停止追加请求的判据 → 双轨发布 → 公网证据 |
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

1. **先补代码债的头部**：FR-25 的 >`PARTSIZE` 跨分片闭环（daemon 接线已完成）；FR-10 守护进程多样种；FR-11/14 的默认策略与 µTP 覆盖。
2. **再补授权/治理**：G0 `protocol_scope` 书面决定是否允许公共 KAD `0x33`；SRS v2.0 评审与 FR-24 编号去重；R-02/R-11（M4 前）。
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
