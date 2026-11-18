---
doc_id: ADR-021
title: ADR-021 把已打通的 TCP socket 喂给 BT 引擎（SOCKS5 钩子 + 回环 splice，不改 librqbit）
category: 架构决策
version: 1.0
status: 已定稿
owner: 架构组
updated: 2026-11-18
---

# ADR-021 把已打通的 TCP socket 喂给 BT 引擎

## 状态

已采纳（2026-11-18）。实现：`cr-bridge`。**不 fork、不换引擎**。

## 背景：打洞原语有了，数据面进不去

`cr_nat::tcp_punch` 已经能建立一条真实的 TCP 连接（双平台回环验通，见 ADR-018），
但 BT 数据面用不上它：librqbit 9.1.0 的 `SessionOptions` 没有自定义 connector，
入站只认**自己绑定的监听口**，出站只认**自己发起的 `TcpStream::connect`**。
一条外部建立的 socket 无法塞进引擎，于是「双方都在 NAT 后、中继又不可用」时只能干等。

这是**能力缺口**，不是**门禁缺口**：S6 的中继侧 30/30 全通，BT-over-punched-TCP
属于「中继不可用时的降级路径」。因此本 ADR 的第一原则是**不为此付出长期维护成本**。

## 可选项

| 方案 | 代码量 | 长期成本 | 判定 |
|------|--------|----------|------|
| A. 本地 SOCKS5 钩子 + 回环 splice | ~400–600 行 | 无分叉，跟上游升级 | **采纳** |
| B. fork librqbit 加 connector/accept 钩子 | 库内 ~100–200 行 | 每次升级 rebase | 备选（A 失败再走） |
| C. 换 libtorrent（C++ 绑定） | 重写 `cr-engine` 实现 | Windows 构建链、LGPL、µTP/DHT/限速行为回归 | 否决 |
| D. 不做 | 0 | 放弃该降级路径 | 保底（门禁不需要） |

C 被否决的理由不是「工作量」本身，而是**行为税**：`cr-engine` 只有 534 行、`Engine` trait
8 个方法、`rqbit::` 类型零泄漏——接口换起来很便宜；但要重新对齐 µTP、fastresume、
盘上布局、限速语义，并引入 C++ 构建链。用一个非门禁项去换这些风险，不划算。

## 决策：A

librqbit 的 `ConnectionOptions.proxy_url` 接受 `socks5://host:port`（见
`librqbit-9.1.0/src/stream_connect.rs:38`）。这正好是一个**官方钩子**，够用了：

- **出向**：进程内起一个本地 SOCKS5 服务。引擎把每条出站连接交给它；
  命中「已打通注册表」的目标 → 直接拿那条 socket 冒充；
  未命中 → 回落为真实直连（行为与不用代理一致，不影响普通 BT）。
- **入向**：对端打通的 socket 在 `127.0.0.1:<engine_listen>` 上再连一次引擎的监听口，
  两端 `copy_bidirectional`。引擎看到的就是一条**普通入站连接**。

两条路都不需要改一行 librqbit。

## 后果与必须照写的降级

1. **对端地址会变成 `127.0.0.1`**。librqbit 会把这类 peer 当本地 peer
   （`torrent_state/live/mod.rs:972` 用 `is_loopback()/is_private()` 判定是否对外通告），
   因此：不参与 PEX/通告、部分统计不计入。**可以接受，但不能宣称「与直连完全等价」。**
2. **多一次用户态拷贝**（出向 SOCKS5 中继、入向回环 splice）。吞吐上限受拷贝影响，
   未做大包压测前不得写性能结论。
3. **注册表是「一次性」的**：一个已打通 socket 只能被取走一次。重复 CONNECT 会回落直连，
   不会复用同一条连接（BT 一条连接只服务一个 peer 会话）。
4. **不改变 NAT 结论**：本 ADR 只解决「socket 怎么交给引擎」，不解决「能不能打通」。
   公网双侧打洞仍受 ADR-018 的 BLOCKED 约束。
5. 本 crate 使用 tokio（属引擎层，符合 ADR-015 的分层；协议/信令面仍 std-only）。

## 验收口径

- 单测必须覆盖：注册 socket 的字节往返、未注册目标回落直连、非 CONNECT 命令拒绝、
  非法 ATYP 拒绝、注册项一次性。
- 端到端（后续）：一条 punched socket 上跑完一个真实样本的一片，且引擎侧
  `progress().downloaded_bytes > 0`。**在此之前，不得对外说「BT 已支持打洞直连」。**

## 复现

```bash
cargo test -p cr-bridge --offline
```
