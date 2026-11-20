---
doc_id: DOC-27
title: M2 阶段小结（2026-11-20，v2.4.97）
category: 计划
version: 1.1
status: 有效
owner: 架构组
updated: 2026-11-20
---

# M2 阶段小结（2026-11-20，v2.4.97）

> 本文是对当前状态的一次**横向收口**：已完成 / 待完成 / 下一步 / 可增强。
> 口径不变：**`g2_claim=false`、`s6_status=not_run`，默认承载仍是 `tcp`**。
> 只记已经量到的东西；没有证据的结论一律放在「待完成」，不写进成绩。
> 细节证据见 [M2-PROGRESS](M2-PROGRESS-2026-11-18.md) §14.7、[ROADMAP](ROADMAP.md)、
> [STATUS-INVENTORY](STATUS-INVENTORY-2026-11-20.md)、
> [ADR-024](../02-architecture/adr/ADR-024-udp-bt-data-plane.md)、
> `tests/e2e/w61/README.md`、`tests/e2e/dht-announce/README.md`。
>
> **v1.1**：补 v2.4.96/2.4.97 的三项 P0（FR-11/14/24/25）与各自**真实对端**证据；
> 其中 FR-14 从「未实现」升为 opt-in 已实现，FR-24/25 从「组件级」升到「真实客户端」，
> 但 **FR-24 公网检索、FR-25 daemon 接线与公网传输仍 BLOCKED**。

## 1. 已完成

| 模块 | 结论 | 证据 |
| :--- | :--- | :--- |
| G1 门禁 | T0 21/21、T1 11/11 全绿 | 测试套件与 T0/T1 golden |
| TCP 中继 BT 数据面（ADR-023 v1.2） | 双向 OPEN 握手、持有者 splice、同一订阅连接按 CD magic 分流；1 MiB 取片成功、两端 md5 一致、中继 Δdropped=0 | W59；`tests/e2e/w59/` |
| UDP 字节治理与订阅/唤醒（ADR-020） | 按帧类型计费、64 KiB/s 令牌桶；`nat.relay_udp` 可单独配置，20 s 保活；跨主机 840/840 零丢 | W53 23/23；`path=relay-udp`、`ack_status=OK` |
| UDP 承载 BT 数据面（ADR-024 v0.3） | std-only 有界可靠层：20 B 头、累积 ACK + 64 位 SACK、WINDOW=32、RTO 200 ms→5 s、3 dupACK 快重传；opt-in，默认仍 `tcp` | 环回双向注入 5%/2%/1% 下 1 MiB 字节一致；真 librqbit SOCKS5 取片 1 MiB |
| **W61：RST 主机上的双 NAT UDP 数据面** | 同一主机 `119.29.150.254` 同轮证明：7100/TCP 纯回显 12 轮中 11 轮被强制 RST；7101/UDP 承载 A（112.5.60.4，cone）↔ 222（112.5.32.34，symmetric）完成 1 MiB 取片 | `tests/e2e/w61/make_report.py` **12/12**；md5 `b0e93e825f86000e5341cf4aecb0559e`；`wake.path=relay-udp`、`ack_status=OK`、中途 `udp_tunnels_alive=1`；holder `opens=1/splices_done=1/←引擎 1,049,585 B`；member `bytes_relayed=1,050,872`；中继 Δ`data_in=4219/deliver_out=4214/data_bytes_in=2,623,138/bandwidth_limited=0/replay=0/mac_fail=0`；222 管理口抓包 50 包全是 17900 控制面、BT 17902 **0 包**（无 L2 近路） |
| 看板修正 | `/health.holder_data.carrier` 不再写死 `relay-sub-tcp`，按 opt-in 标记实际承载 | 单测 `holder_carrier_label_follows_opt_in_carrier` |
| DHT announce（自有公网节点） | 授权后从 222（源 IP 与宣告端口真实匹配）向自有节点宣告 `112.5.32.34:17200`，A 异机 `get_peers` 回查 `peers=1`、`peer_list=112.5.32.34:17200` | `tests/e2e/dht-announce/{announce.json,verify.json}` |
| **FR-11 握手拦截（opt-in）** | `engine.guard_handshake` 开启后引擎退到回环内部口、guard 占对外 `bt_port`，按 infohash 走唯一入口 `wake_seed`；两个不同样本只唤醒对应那个、未知 infohash 不唤醒 | W62 **12/12**；`tests/e2e/w62/make_report.py` |
| **FR-14 空闲自动休眠（opt-in）** | 唯一入口 `idle::sleep_hot_seeds`（与 `wake_seed` 对称）；`idle_sleep_secs`/`idle_sleep_minutes` 默认关闭、秒优先；`/health.idle` 可观测 | W14 **12/12**：`seed_active 1→0`、`sleeps=1`、状态序列 `COLD→WAKING→HOT→COLD`、4 个 `published_keys` 保留、`find` 仍 `hit_node=A` |
| **FR-24 公共轨 KAD 检索** | 生产 `coldrevive-ed2k search`：先 `0x01/0x09` bootstrap 再发 `0x33`，解析 `0x3B` 的 KAD tag 生成标准 `ed2k://` | W24-pub **12/12**（真实 aMuleD `192.168.1.221:4672`：`bootstrap_ms=273`、`rtt_ms=6`、Key 与发布侧逐字节一致）；公网 5 节点全超时 → **仍 BLOCKED** |
| **FR-25 与真实 aMule 2.3.3 双向互通** | 生产 `cr-ed2k::stack` 的 `download`/`serve`，两个方向都对真实客户端跑通 | W25-live **22/22**：下载 262144 B/25 块 hash+content 一致；上传 65536 B 与 262144 B 两样本**对端落地 md5**一致 |
| 回归与工程状态 | 全量 `CARGO_INCREMENTAL=0 cargo test --workspace --offline` **384 passed / 0 failed / 3 ignored**；工作区干净、本地与远端同 sha | 提交 `52ae792`（FR-14/24/25 真实对端）、`fc0377b`（FR-11/24/25 组件级）、`75dacda`（W61）、`78771f3`（DHT 证据） |

## 2. 待完成 / 明确未达标

1. **G2 门禁未达标**：`g2_claim=false`、`s6_status=not_run`。S3=pass、S5=fallback、
   S6=`closed_with_exceptions`（ADR-016 eMule 式语义），但机器常量未改，不得折算成通过。
2. **公平性量具已到 v1（量具 ≠ 达标）**：v0 转发层 4 流份额
   25.5% / 21.4% / 21.4% / 31.6%（`fairness-probe.json`）；v1 补齐两项——
   ① 同预算 64 KiB/s 的 TCP/UDP 4 流对照（`fairness-control-v1.json`：
   TCP 19.8–32.4%、UDP 23.1–26.8%）；② 真 `StreamCore` + 虚拟时钟 +
   5%/2%/1% 注入 + 750 ms ACK 延迟的窗口/RTO/重传时间序列
   （`rto-timeseries-v1.json`：`inflight ≤ 32`、`rto 200→400→800→1600→3200 ms`、
   `retransmits=41`、字节一致）。两者都只打印 JSON、不自动判 pass。
   **可靠层仍无拥塞语义，跨公网公平性仍未测，不得读成公平性达标**。
3. **真实 Mainline DHT 未加入**：`dht.transmissionbt.com` 返回 `token_len=0`，
   `router.bittorrent.com` 与 `router.utorrent.com` 查询超时；只证明自有公网节点闭环。
4. **TCP 打洞**：原语在 Windows + Linux 双端回环通过，但 BT 数据面未接线、公网双侧 BLOCKED。
5. **守护进程仍单做种任务**：真引擎逐出（cap=1，A→B→A，`Cold→Hot→Cold→Hot`）只在组件级验证。
6. **外推边界**：W61 的 `rst-path-verified` 只指 `119.29.150.254` 这台主机这条路径，
   不外推所有 RST 部署；symmetric 不可直连是正确结论，不当成失败。
7. 跨公网吞吐 / RTT 敏感性只有单次 1 MiB 样本，不足以做性能结论。
8. **FR-14 只有空闲计时**：没有 CPU / 内存 / 带宽触发；休眠后 `seed_active=0` 但影子索引与
   `published_keys` 保留——**「KAD 还查得到」不等于还在做种**，别把它读成「休眠仍供片」。
9. **FR-24 只有 LAN 公共轨**：能对 `192.168.1.221` 的 aMuleD 检索，公网 KAD 5 个节点全超时
   （`answer_count=0`）；且 `0x33` 必须**先 bootstrap** 才有应答，客户端顺序写错就是 0 结果。
10. **FR-25 只是局域网内组件级**：`cr-ed2k` **daemon 未接线**，公网 eD2k server/peer 未测，
    无 server 登录 / source exchange / credit / AICH；样本都 < `PARTSIZE(9,728,000)`，
    跨分片与 `OP_HASHSETREQUEST` 校验未覆盖；`OP_QUEUERANKING` 分支未被真实客户端触发过。
    上传判据必须是**对端落地 md5**（aMule 重复 `REQUESTPARTS` 会让 `bytes_sent` 大于文件大小）。

## 3. 下一步计划（按依赖顺序）

1. **公平性判定阈值与对照扩展**（v1 量具已完成）：补跨公网背景流、丢包/时延梯度
   的多组样本，再讨论是否有可自动判定的阈值。此前一律只出原始计数，
   **不宣称公平性达标**（任务 #36 的量具部分已闭环，判定部分仍开）。
2. **真实 Mainline DHT**：继续找确实签发 token 的引导节点；announce 后用**独立节点**
   `get_peers` 回查；拿不到证据就维持 BLOCKED，不宣称加入 Mainline。
3. **守护进程多样种任务**：把组件级逐出扩成真引擎 e2e（多 infohash + cap 触发 + 按
   infohash 唤醒的顺序验证）。
4. **TCP 打洞接 BT 数据面**：先解决公网双侧的 BLOCKED，再谈接线；symmetric 场景
   保留不可直连结论，不强求。
5. **G2/S6 复评**：只在 1–4 都有证据后进行；提前不改 `g2_claim` 与 `s6_status`。
6. **FR-25 接进守护进程**：把 `cr-ed2k` 的 `serve`/`download` 接到 `coldrevived` 生产路径
   （现在只是 CLI 组件），再接「双轨发布」——这是 FR-25 从「部分」升到「已实现」的唯一路径。
7. **FR-25 跨分片样本**：用 > `PARTSIZE(9,728,000)` 的样本跑一次双向，补上
   `OP_HASHSETREQUEST` / hashset 校验与跨分片拼接这两条未覆盖路径。
8. **FR-24 公网 KAD**：需 G0 `protocol_scope` 放宽；放开前维持 BLOCKED，不拿 LAN 结果外推。

## 4. 可增强功能

- **拥塞控制**：UDP 可靠层加 AIMD 之类退避——但必须先有量具，否则只是猜。
- **可观测性**：`/health` 补「对端实际收到字节」而非「写入隧道字节」，避免再次出现
  `data_plane.tunnels` 与 `tunnels_alive` 那类看板误读；`bytes_from_engine` 也应标注口径。
- **路径选择状态机**：补多路径并发探测与切换滞后，避免 RST 单轮偶发成功导致抖动。
- **工程自动化**：把跨主机部署 / 抓包 / 三方对账 / 判定收拢成可复跑的 e2e 命令，
  减少手工 ssh 步骤与人为漏采。
- **公平性判定**：v1 量具已稳定（转发层对照 + 可靠层时间序列），下一步是跨公网
  梯度样本与阈值讨论；此前一律只出原始计数，不自动判 pass。

## 5. 复现命令

```bash
# 全量回归（先停占用 17800 的本地守护进程，否则 e2e 会挂住）
CARGO_INCREMENTAL=0 cargo test --workspace --offline

# W61 证据判定（需按 tests/e2e/w61/README.md 重新采集 reports/）
python tests/e2e/w61/make_report.py

# FR-14 空闲休眠 / FR-24 检索 / FR-25 双向互通（判据会 ssh 去读对端落地 md5；
# 读不到记 BLOCKED，不记 pass。复现细节见各目录 README）
python tests/e2e/w14/make_report.py
python tests/e2e/w24-pub/make_report.py
python tests/e2e/w25-live/make_report.py

# FR-24 检索：必须先 bootstrap，客户端顺序写错就是 0 结果
./target/release/coldrevive-ed2k.exe search --term <关键词> --bootstrap 192.168.1.221:4672

# FR-25 供片端（只服务一个连接，结束即退出）+ 由内容导出真实 ed2k 链接
./target/release/coldrevive-ed2k.exe link  --file <样本>
./target/release/coldrevive-ed2k.exe serve --file <样本> --port 4663 --timeout-ms 180000

# 公平性量具 v0/v1（只打印 JSON，不判 pass）
CARGO_INCREMENTAL=0 cargo test --offline -p cr-nat udp_relay_data_fairness_probe -- --ignored --nocapture
CARGO_INCREMENTAL=0 cargo test --offline -p cr-nat carrier_fairness_tcp_vs_udp_probe -- --ignored --nocapture
CARGO_INCREMENTAL=0 cargo test --offline -p cr-nat reliable_window_rto_timeseries_probe -- --ignored --nocapture

# DHT 只读回查（写操作需另行授权）
./target/release/coldrevive-dht.exe --get-peers 82.157.68.159:6881 <infohash-hex>
```
