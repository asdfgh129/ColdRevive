---
doc_id: ADR-018
title: TCP 同时打开打洞的判据与边界（原语完成；桥接层见 ADR-021，端到端未验）
category: 架构决策
version: 1.0
status: 已接受
owner: 内核组
updated: 2026-11-18
---

# ADR-018：TCP 同时打开打洞——判据、实现代价与引擎接入边界

| 项 | 值 |
| :--- | :--- |
| 状态 | **已接受**（2026-11-18，W3.9-⑤ 实现） |
| 决策者 | 内核组 |
| 性质 | 实现层决策 + 一条明确的**未完成边界** |
| 关联 | [ADR-015](ADR-015-dependency-layering.md) · [ROADMAP](../../06-plan/ROADMAP.md) · `crates/cr-nat/src/tcp_punch.rs` |

## 背景

M2 剩余项里有「BT/TCP 打洞」。真实 BT 客户端之间相当比例的流量走 TCP；
两侧都在受限锥/对称 NAT 之后时，TCP 入向被过滤，唯一可行的手段是
**同时打开**（RFC 793）：双方从各自的公网映射端口向对方发 SYN，
两个方向上的 SYN 同时到达，各自网关的过滤项同时被打开，握手得以完成。

这里有两个必须先说清的事实：

1. **必须固定本地端口**。不固定的话内核挑临时端口，两侧都不知道对方这次用的外部端口。
   std 的 `TcpStream` 不能 bind-before-connect；`TcpSocket`（1.63 起提供）本工具链的 std
   里没有（2026-11-18 实测 `std::net::TcpSocket` 编译不过）。
2. **「连上了」的判据很脆**。Windows 上 `getpeername` 在 SYN_SENT 阶段就可能返回目标地址，
   拿它判断会把「连接被拒」读成「已连接」——实测确实对没有监听的端口报过成功。

## 决策

1. **用最小 FFI 自建 socket**：`socket/bind/setsockopt(SO_REUSEADDR)/connect`，
   完成绑定后把句柄交给 `std::net::TcpStream` 托管（`FromRawSocket` / `FromRawFd`），
   之后不再手动 close，避免双重释放。协议面仍保持 std-only（不引入 libc/socket2 依赖）。
2. **完成判据 = `poll(POLLOUT|WSAPoll)` + `getsockopt(SO_ERROR)`**：
   可写且 `SO_ERROR==0` 才算握手完成；`SO_ERROR!=0` 即本轮失败并按系统错误原文记录。
   任何情况下都**不用 `peer_addr()` 代替**（见上）。
3. **重试是真实 SYN**，间隔 150 ms，直到调用方给的超时。不合成任何结果；
   失败时 `errors` 可能为空，此时如实标为「被静默丢弃」而不是「不可达原因未知但成功」。
4. **两侧都必须建立**才算通过。回环实测（2026-11-18，本机 Windows）：
   两个独立进程各自绑定 19911/19912，`attempts=1`、`elapsed` 1–4 ms，
   `PING/PONG` 双向往返成功，连续 5 轮双侧均建立。只建立一侧属于需要解释的退化，
   不允许把断言放宽成「至少一侧」。
5. **公开声明边界**：本模块交付的是**传输原语**。把它接进 BT 数据面还要求引擎接受
   「外部已建连的 socket」，而 librqbit 9.1.0 没有这个入口。因此
   **BT-over-punched-TCP 仍是缺口**，ROADMAP 里按「原语完成、接线未完成」记录。

## 后果

- 正面：TCP 打洞从「分析上可行」变成「有真实双侧证据的原语」，并带走了一个
  容易复发的假阳性判据（`peer_addr`）。
- 负面/限制：
  - ~~Unix 分支未编译验证~~ → **2026-11-18 第二轮已补**：在 221（Ubuntu，rustc 1.100.0）上
    `cargo test -p cr-nat tcp_punch` 真实编译并 4/4 通过（回环双侧建立、双向字节 PASS、
    空端口如实超时）。记录 `attempts=7`（Windows 回环是 1）：同时打开要求双方 SYN 交叉，
    先到一侧收到 RST 属正常重试，**差异在重试次数、不在能否建立**。
    仍需注意：该证据是**回环**；Linux 上公网双侧未测。
  - 公网 NAT 上的双侧实测仍 `BLOCKED`：A 侧 CPE 没有为该固定本地端口建 TCP 映射，
    而 UDP 侧学到的映射端口不能用于 TCP（W5.1f 同一条结论）。
  - 本 ADR 自身不提供「打洞后交给引擎」的路径，因此不能对外说「BT 已能走 TCP 打洞」。

> **实施状态更新（v2.4.87/88，2026-11-18 晚；本 ADR 决策正文不变）**：
> 引擎接入路径已由 [ADR-021](ADR-021-punched-tcp-bridge.md) / `cr-bridge` 提供
> （SOCKS5 出向钩子 + 回环 splice 入向，不 fork、不换引擎，6/6 单测）。
> **但尚未接进 `wake_with_plan`，也没有真实样本过桥，端到端仍未验证**；
> 因此「BT 已支持打洞直连」仍然是不成立的声明。公网双侧打洞仍 `BLOCKED`。
- 风险：FFI 是 `unsafe`，且 socket 句柄所有权一旦搞错就是句柄泄漏或双关。
  缓解：句柄只在 `open_pending` 内创建，失败路径显式 `close`，成功路径立即 `into_stream`，
  不存在「同一句柄两处关闭」的分支。
