---
doc_id: ADR-024
title: ADR-024 中继 UDP 承载上的 BT 数据面（提案，未批准）
category: 架构决策
version: 0.1
status: 提案（待所有者确认；本 ADR 不含实现）
owner: 架构组
updated: 2026-11-19
---

# ADR-024 中继 UDP 承载上的 BT 数据面

## 状态

**提案，未接受。** 2026-11-19 所有者选择「先出设计再实现」。
本文件不修改任何代码；`nat.relay_data_plane_udp` 仍为 `not_implemented`，
TCP 承载仍是唯一数据面实现。批准后才能进入本文 §7 的分阶段实施。

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
| 默认承载仍为 TCP；UDP 只能 opt-in | ADR-023 | 不得因本提案改变 W58/W59/W60 的任何既有结论 |

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

## 建议（仍需批准）

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

## 分阶段实施（批准后才开始）

### 阶段 0：先补治理（无数据面）

- 中继 UDP 统计增加 `data_bytes_in/out`，按 ADR-014 同一口径计入带宽账本；
  超限要有明确的 `Reject` 原因，不能静默丢。
- 现有 UDP 唤醒/订阅行为与统计不变；`relay_data_plane_udp` 保持 `not_implemented`。
- 验收：单测证明超限被拒且原因可观测；W3.16 的 23/23 订阅用例仍过。

### 阶段 1：`cr_nat::relay_udp_stream`（纯协议面，std-only）

- 独立于 UDP 头 `seq` 的 **app seq**；UDP 头 seq 每个 datagram 递增以免被抗重放丢弃。
- 接收：有界乱序重组窗口、重复丢弃、缺口驱动 ACK（累积 + 有界 SACK 位图）。
- 发送：有界窗口、RTO 指数退避、3 次重复 ACK 快重传、显式关闭。
- 对外只暴露 `Read + Write`（或可直接喂 `TunnelMux` 的 `dispatch`/出队接口），
  不引入 async 运行时。
- 验收：单元与真 socket 测试覆盖 丢包 / 乱序 / 重复 / 超时重传 / 有界内存；
  **不得**把中继 `STATUS` 计入「对端已收」。

### 阶段 2：接入数据面（opt-in）

- 新增配置 `nat.data_plane_carrier = "tcp" | "udp"`（默认 `tcp`）；
  只有显式选 `udp` 才注册 UDP 数据面会话。
- 发起方：`UdpRelayClient::connect/hello` + `derive_session_id(self_id, b"cr-dp-v1")`
  （与 TCP 相同的派生逻辑，避免订阅会话被顶掉）；OPEN/OPENED 握手语义不变。
- 持有方：`relay_sub::serve_udp` 增加与 TCP 同样的 `CD` magic 分流 + `TunnelMux` +
  独立线程拨本地引擎；复用 `splice_blocking_with` 与 `HolderStats`。
- 分片：每个 `TunnelFrame` 编码后切成 ≤1152 B 的 UDP DATA；每片带 channel/app seq。
- 看板：新增 `carrier` 与 `udp_*` 计数；沿用 `tunnels_alive` 语义（登记数不等于活着）。
- `punch_route::relay_data_plane_udp` 只有在阶段 3 的真实证据出现后才改为 `implemented`。

### 阶段 3：证据（没有证据就不改状态字符串）

1. 环回 + 真实 `UdpRelayServer`：1 MiB 字节级一致；注入 5% 丢包 / 2% 乱序 / 1% 重复；
   断言无损坏、无重复投递、窗口有界。
2. 与 TCP 数据面同一把尺：真引擎 librqbit 经 SOCKS5 取片，md5 两端一致。
3. 只有在**确有 TCP 入向被 RST 的部署**上复现成功，才宣称解决 ADR-020 场景；
   否则只写「同 LAN/普通公网 UDP 中继可用」，不冒充覆盖那类机器。

## 与现有 ADR 的关系

- **ADR-020**：本提案不改其任何约束；恰恰是其「UDP 可用、TCP 被掐」场景的下游需求。
  ADR-020 反对的是**无边界地自造 QUIC 替代品**；本提案用阶段闸门和明确非目标回应这一条。
- **ADR-023**：隧道帧、OPEN/OPENED、持有者 splice、session id 派生全部复用；
  仅承载层可替换。
- **ADR-014**：阶段 0 是把 TCP 的字节预算口径补齐到 UDP，而不是新增一套治理。
- **ADR-015**：阶段 1 保持 `cr-nat` std-only；若要走方案 C，必须另立依赖 ADR。

## 复现/核验

本 ADR 为提案，无实现可复现。现有事实可核：

```bash
# UDP 承载目前只做唤醒，无数据面（源码模块头与状态字符串）
grep -n "not_implemented" src/bins/coldrevived/src/{punch_route.rs,nat_path.rs}
# UDP DATAGRAM 上限与 MTU 断言
cargo test --offline -p cr-nat max_datagram_fits_ipv6_min_mtu
# 既有 UDP 订阅证据（23/23）
# 见 tests/e2e/w53/README.md
```
