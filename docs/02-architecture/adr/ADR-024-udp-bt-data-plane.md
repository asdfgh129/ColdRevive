---
doc_id: ADR-024
title: ADR-024 中继 UDP 承载上的 BT 数据面
category: 架构决策
version: 0.3
status: 已接受（2026-11-19 所有者「按建议执行」；阶段 0–3 均完成，RST 路径证据限于 119.29.150.254）
owner: 架构组
updated: 2026-11-20
---

# ADR-024 中继 UDP 承载上的 BT 数据面

## 状态

**已接受，按建议执行。** 2026-11-19 所有者先选「先出设计再实现」，随后确认「按建议执行」，
即采用方案 B（std-only 有界可靠层）并按本文闸门分阶段实施。
v0.3 记录实施结果：阶段 0（UDP 字节治理）、阶段 1（`cr_nat::relay_udp_stream`）、
阶段 2（opt-in 数据面接线）与阶段 3 三条证据均已完成；状态串为
`relay_data_plane_udp="udp-carrier-implemented-rst-path-verified"`。

**第三条证据的严格边界**：W61 是在 ADR-020 §3.1 那台 `119.29.150.254` 上，
同一轮同时证明「非 22 入向 TCP 仍被注入 RST」与「UDP 7101 承载 1 MiB 双 NAT 取片成功」。
状态串里的 `path` **只指这台主机这条路径**，不外推成所有 RST 部署；默认承载仍是 TCP。
跨公网公平性/拥塞量具仍未做，不能把「跑通」读成「拥塞安全」。

## 背景：为什么还要考虑 UDP 数据面

W58（LAN 中继）、W59（公网中继）、W60（真 STUN 分类 + 强制打洞失败）已经证明：
**只要中继的 TCP 承载可用，BT 数据面就能闭环**，1 MiB `complete`、两端 md5 一致、
中继 `Δdropped=0`。所以本 ADR 的目标**不是「UDP 更快」**，也不是补救现有公网链路。

动机来自 ADR-020 记录的另一类部署：腾讯云轻量 `119.29.150.254` 的**入向 TCP 在数据阶段被
边缘注入伪造 RST**（同一端口 `7100`、非 22，0/30 通过；同期入向 UDP 全程正常）。
在那类机器上，中继的 TCP 承载根本不可用，唤醒能靠 UDP，但 BT 字节流无路可走。
因此 UDP 数据面是**部署覆盖面**问题，不是性能问题。

## 硬约束（不可协商）

| 约束 | 来源 | 后果 |
| :--- | :--- | :--- |
| `cr-nat` 继续 **std-only** | ADR-015 依赖分层表 | 不能在 `cr-nat` 直接引入 `librqbit-utp`/tokio；要引入必须另立 ADR |
| UDP 负载上限 **1232 B**（头 64 + 数据 1168） | ADR-020；`UDP_MAX_PAYLOAD` 单测钉住 IPv6 最小 MTU | 隧道 16 KiB body 必须分片成约 15 个 datagram |
| DATA 负载前 16 B 是目标 id | `relay_udp::send_data` | 每个隧道分片的用户字节 ≤ 1152，扣掉新增帧头后更少 |
| `STATUS` 只是「中继转发了」，**不是端到端 ACK** | `relay_udp.rs` 模块头；ADR-020 | 任何把 `STATUS` 当送达凭据的实现都必须否决 |
| 中继 UDP 头 `seq` 参与 64 位抗重放滑窗 | `Session::accept_seq` | **重传必须换新的 UDP 头 seq**；可靠层 seq 必须是独立字段 |
| 中继 UDP 只有 PPS 限额（2000/s/session），**没有字节级令牌桶** | ADR-014 只覆盖 TCP 帧 | 数据面启用前必须补字节治理，否则可绕过既有带宽治理 |
| 订阅会话 60 s 空闲回收，keepalive 20 s | ADR-020 / `relay_sub::serve_udp` | 大文件传输中不得静默停心跳 |
| 默认承载仍为 TCP；UDP 只能 opt-in | ADR-023 | 不得因本 ADR 改变 W58/W59/W60 的任何既有结论 |

## 目标与非目标

**目标**

1. 在中继 TCP 入向被掐、UDP 可用的部署上，让 BT 字节流也能走中继。
2. 对上层（`cr-bridge`、librqbit、持有者 splice）**零改动**：复用 `TunnelMux`、
   `TunnelFrame`、`splice_blocking_with`、`DataPlane` 的 SOCKS5 桥。
3. 失败要可见、可计数；不允许「登记了隧道但一个字节没搬」的假绿（W59 教训）。

**非目标（本 ADR 明确不做）**

- 不做 QUIC，不做通用可靠传输库，不宣称跨公网吞吐/公平性结论。
- 不做 LEDBAT 退让（NFR-03 的这一半仍由 S5 量具负责；见 ADR-015 §uTP 更正）。
- 不改变 NAT 分类、`g2_claim`、`s6_status` 的任何判据。
- 不把 UDP 数据面作为默认承载。

## 候选方案

| 方案 | 做法 | 优点 | 致命问题 |
| :--- | :--- | :--- | :--- |
| **A. 维持现状** | 只有 TCP 数据面；TCP 被掐的部署不接受 | 零风险；现有证据全部有效 | ADR-020 那类机器永远无法取片 |
| **B. 自研有界可靠层（std-only）** | 在 `cr-nat` 新增 `relay_udp_stream`：独立 app seq、累积 ACK + 有界 SACK、RTO/退避、发送窗口、乱序重组；把 `TunnelFrame` 放进 DATA 负载 | 不破坏 ADR-015；可复用 `TunnelMux`；每字节行为仍可回溯到报文，符合协议面 crate 的证据文化 | 没有经过审计的拥塞控制；写成了「更差的 QUIC」风险真实存在（ADR-020 §反对理由） |
| **C. 复用 `librqbit_utp`** | 用真实 uTP（BEP 29）做可靠层，经虚拟 UDP socket 适配到中继 DATA/Deliver | 成熟的重传/窗口/拥塞控制；仓库已有 `tests/spikes/s5/utp` 实证栈可用 | ① 需新依赖，协议面 crate 要另立 ADR（ADR-015）；② uTP 假定真实 UDP socket 与对端地址，中继是 session 转发，虚拟适配层不小；③ 该实现是 **Cubic，未实现 LEDBAT**（spike 报告实测），与 QoS 退让目标不一致 |
| **D. QUIC** | 完整 QUIC 栈 | 最完整的可靠性 + 拥塞控制 | 依赖与安全边界远超本项目；ADR-020 已明确否决「自己造一个更差的 QUIC」 |

## 建议（已批准并执行）

**采用 B，但分阶段、带闸门，且明确定位为「中继内受控链路的可靠层」，不是通用传输协议。**

理由：

1. C 的两个前提（依赖例外 + uTP→中继 session 的虚拟 socket 适配）都需要新 ADR，
   且 uTP 在该版本是 Cubic，不能顺带解决 LEDBAT；把「拥塞控制」和「接入 UDP 数据面」
   绑在一起会让本轮范围失控。
2. B 能守住 ADR-015 的 std-only 边界，且协议面每条结论仍可回溯到真实报文——
   这正是本项目门禁的红线。
3. B 的风险（无审计拥塞控制）用**闸门**而不是靠口号控制：先补字节治理，再在
   **有界窗口 + RTO 退避 + 严格 opt-in** 下验证；跨公网公平性没有证据就永远不宣称。

**否决条件（任一成立即停，不进入下一阶段）**：

- 字节级预算无法与 ADR-014 对齐（UDP 能绕过令牌桶）；
- 丢包注入下出现数据损坏、重复投递或无限重传；
- 与 TCP 流共享瓶颈时测出明显饿死（需先有量具，不靠估计）；
- 任何实现把 `STATUS` 当作端到端送达。

## 分阶段实施（已执行；v0.2 回填结果）

### 阶段 0：先补治理（无数据面）✅

- 中继 UDP 统计新增 `data_bytes_in/out`、`bandwidth_limited`、`bandwidth_limited_bytes`，
  按 ADR-014 同一口径（512 KiB/s、2 MiB 突发）计费；UDP 侧按**UDP 帧类型**判据，
  只有 `UdpKind::Data` 进字节桶，HELLO/PING/STATUS 等信令绕过。
  超限以 `UdpStatus::BandwidthLimited`（码 5，附 UTF-8 原因）返回 STATUS，
  不静默丢、也不误报 `target_absent`。
- CLI 新增 `--udp-data-rate` / `--udp-data-burst`；唤醒/订阅的转发行为与既有计数不变。
- 验收：`relay_udp::tests` 14/14（含 4 条预算用例）；W3.16 订阅用例仍过。

### 阶段 1：`cr_nat::relay_udp_stream`（纯协议面，std-only）✅

- 独立于 UDP 头 `seq` 的 **app seq**；UDP 头 seq 每个 datagram 递增以免被抗重放丢弃。
- 接收：有界乱序重组窗口（`WINDOW=32`）、重复丢弃、缺口驱动 ACK（累积 + 64 位 SACK）。
- 发送：有界窗口、RTO 指数退避（200 ms→5 s）、3 次重复 ACK 快重传、显式 CLOSE。
- 对外暴露 `Read + Write` 与 `split()` 两半；不引入 async 运行时。
  ACK/CLOSE 也装在 `UdpKind::Data` 里，**同样计入**阶段 0 字节预算（保守，不绕过 ADR-014）。
- 验收：单元与真 socket 测试覆盖 丢包 / 乱序 / 重复 / 超时重传 / 有界内存；
  中继 `STATUS` 只记数，`status_never_acts_as_ack` 钉死不得滑窗。

### 阶段 2：接入数据面（opt-in）✅

- 新增配置 `nat.data_plane_carrier = "tcp" | "udp"`（默认 `tcp`，非法值回落 `tcp`）；
  只有显式选 `udp` 才注册 UDP 数据面会话，W58/W59/W60 默认行为不变。
- 发起方：`UdpRelayClient::connect/hello` + `derive_session_id(self_id, b"cr-dp-v1")`
  （与 TCP 相同的派生逻辑，避免订阅会话被顶掉）；OPEN/OPENED 语义不变——
  **未收到 `0xED` 不登记 SOCKS5 隧道**，红线单测 `udp_attach_without_peer_confirmation_fails_honestly`。
- **寻址必须是派生 id**：发起方把 DATA 发到 `derive_session_id(peer_id, b"cr-dp-v1")`，
  不是对端节点 id。UDP 侧节点 id 那条会话由订阅/唤醒占用，发错目标会被订阅线程当唤醒帧
  丢掉，只表现为「3 s 超时回落直连」——实施期真踩到了（见 M2-PROGRESS §14.4 第 6 条），
  正例单测 `udp_attach_registers_tunnel_when_holder_confirms` 钉住。
- 持有方：`relay_sub` 在 UDP 订阅之外另起数据面服务；`UdpRelayStream` 用全零 peer 构造，
  从首个已验签的 `DELIVER.from` 锁定发起方；`recv_open` → 先拨本地引擎 → `send_opened` →
  `splice_stream_with`，复用 TCP 侧 `HolderStats`。
- **归属校验（ADR-023 同一条红线）**：`recv_open` 要求 OPEN 声明的发起方 id **等于**中继
  观测来源，且来源不可为全零；不符即拒绝、不拨引擎。单测
  `recv_open_rejects_initiator_that_differs_from_observed_source`。
- **实施偏差**：UDP 数据面没有复用 `CD` magic / `TunnelMux` 分流——UDP DATA 已是独立会话，
  流内用 `"US"` 线段魔数区分；OPEN 仍复用 ADR-023 的 `TunnelFrame::open` 23 B 编码。
- 分片：每条线段 = 20 B 头（app seq / ACK / SACK）+ ≤1132 B 用户字节，
  装在 UDP DATA 的 16 B 目标 id 之后，合计不超过 1168 B 负载上限。
- 看板：`/health.data_plane` 新增 `carrier`、`udp_tunnels`、`udp_tunnels_alive`；
  沿用 `tunnels_alive` 语义（登记数不等于活着）。
- 阶段 3 证据出现后，`relay_data_plane_udp` 才从 `not_implemented` 改为
  `udp-carrier-implemented-rst-path-verified`。

### 阶段 3：证据（没有证据就不改状态字符串）

1. ✅ 环回 + 真实 `UdpRelayServer`：1 MiB 字节级一致；注入 5% 丢包 / 2% 乱序 / 1% 重复；
   断言无损坏、无重复投递、窗口有界（实测数字见下节）。
2. ✅ 与 TCP 数据面同一把尺：真引擎 librqbit 经 SOCKS5 取片，`1048576 B`、`finished=true`，
   两端一致；同一测试文件里 TCP 与 UDP 两条用例并列跑。
3. ✅ **W61（2026-11-20）**：在 ADR-020 §3.1 那台 `119.29.150.254` 上，同轮做两件事：
   ① A → `7100/TCP` 纯回显 12 轮，11 轮被强制断开（另一轮偶发成功，ADR-020 已记录同现象）；
   ② A（`112.5.60.4`, cone）与 222（`112.5.32.34`, symmetric）经 `7101/UDP` 中继完成
   1 MiB 取片，md5 两端一致、`wake.path=relay-udp`、`holder.udp_tunnels_alive=1`、
   `splices_done=1`、中继 `Δdata_bytes_in=2,623,138`、`bandwidth_limited=0`。
   因此后缀从 `rst-unverified` 改为 `rst-path-verified`；`path` 只指该主机该路径。

## 与现有 ADR 的关系

- **ADR-020**：本 ADR 不改其任何约束；恰恰是其「UDP 可用、TCP 被掐」场景的下游需求。
  ADR-020 反对的是**无边界地自造 QUIC 替代品**；本 ADR 用阶段闸门和明确非目标回应这一条。
- **ADR-023**：隧道帧、OPEN/OPENED、持有者 splice、session id 派生全部复用；
  仅承载层可替换。
- **ADR-014**：阶段 0 是把 TCP 的字节预算口径补齐到 UDP，而不是新增一套治理。
- **ADR-015**：阶段 1 保持 `cr-nat` std-only；若要走方案 C，必须另立依赖 ADR。

## 复现/核验

```bash
# 阶段 0：UDP 字节预算（超限带原因，不静默丢）
cargo test --offline -p cr-nat relay_udp::tests      # 14/14

# 阶段 1：纯协议面（线格式 / 乱序 / 重复 / 超时重传 / 快重传 / STATUS 不当 ACK）
cargo test --offline -p cr-nat relay_udp_stream::    # 14/14

# 阶段 2：未收到 OPENED 必须如实失败；收到则登记（正例同时钉住派生 id 寻址）
cargo test --offline -p coldrevived data_plane::tests::udp_attach

# 阶段 3-1：真 UdpRelayServer + 双向注入（加 --nocapture 看 [stage3] 行）
cargo test --offline -p cr-nat real_udp_relay_carries_1mib -- --nocapture

# 阶段 3-2：真引擎 librqbit 经 SOCKS5 取片（同文件另有一条 TCP 对照）
cargo test --offline -p cr-engine --test relay_data_e2e -- --nocapture

# 阶段 3-3：RST 部署上的公网双端（详见 tests/e2e/w61/README.md）
python tests/e2e/w61/make_report.py        # 12/12，失败即非零退出

# UDP 承载上限与 MTU
cargo test --offline -p cr-nat max_datagram_fits_ipv6_min_mtu
```

**阶段 3-1 实测（本机，一次干净运行，5.99 s）**：1 MiB，发送窗口上限 `WINDOW=32`。
DATA 方向注入 `seen=1220 / dropped=56 (4.6%) / reordered=23 (1.9%) / duplicated=11 (0.9%)`，
ACK 方向注入 `seen=1175 / dropped=57 / reordered=20 / duplicated=14`；结果
`retransmits=293`、接收侧 `dup_dropped=248`（重复段被丢弃而非重复交付）、
`max_inflight=32` / `max_incoming=31`（窗口有界）、交付字节精确 `1048576`（重复投递不增账），
`status_ignored=1175/1175`（中继 STATUS 全部只记数、不滑窗）。

**阶段 3-2 实测（本机，真 librqbit + 真 UDP 中继 + SOCKS5 桥）**：
`downloaded=1048576 finished=true`，持有者 `→引擎 1264 B / ←引擎 1,049,586 B`，
中继 `deliver_out=1940`、`data_bytes_in=1,120,714`、`bandwidth_limited=0`。

**阶段 3-3 实测（W61，`tests/e2e/w61/reports/report.json`）**：12/12 断言通过。
TCP 对照 11/12 重置；A/222 的 STUN 分类分别是 cone / symmetric；
`wake.path=relay-udp`；中途 `udp_tunnels_alive=1`；结束后两端 md5 一致、
持有者 `splices_done=1 / ←引擎 1,049,585 B`、下载端 `bytes_relayed=1,050,872`；
中继 Δ`data_in=4219 / deliver_out=4214 / status_out=4219 / data_bytes_in=2,623,138 /
bandwidth_limited=0 / replay=0 / mac_fail=0`；222 管理口 50 包全是 17900 控制面，
BT 端口 0 包。

**证据边界（照写，不许放宽）**

1. 证据 3-1/3-2 在 `127.0.0.1`；证据 3-3 是**公网 UDP 中继 + 两个不同 NAT 出口**，
   但只有单次 1 MiB、约 32 s 的样本。**跨公网吞吐与 RTT 敏感性仍未测**；
   v2.4.95 新增的 `udp_relay_data_fairness_probe` 只是回环转发层量具
   （4 流抢 64 KiB/s 令牌桶，份额 21.4–31.6%），**不是**可靠层拥塞控制结论。
2. `rst-path-verified` 只覆盖 `119.29.150.254` 这条被注入 RST 的路径，
   不外推成「所有 RST 部署都能用」，也不代表 ADR-020 的其他约束被改掉。
3. W61 的控制面经 `192.168.1.222:17900`（LAN），只有 BT 字节进公网 UDP 中继；
   L2 抓包证明没有 BT 近路，但这仍**不是**「两个 NAT 直接可达」的证据。
4. 默认承载仍是 `tcp`；W58/W59/W60 的全部结论不受影响。
5. `g2_claim=false`、`s6_status=not_run` 不变；UDP 数据面不是 NAT 可达性证据。
