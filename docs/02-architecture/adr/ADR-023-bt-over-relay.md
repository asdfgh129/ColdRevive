---
doc_id: ADR-023
title: ADR-023 BT-over-relay：用 TCP 中继帧承载 BT 字节流（不自建可靠传输）
category: 架构决策
version: 1.0
status: 已定稿
owner: 架构组
updated: 2026-11-19
---

# ADR-023 BT-over-relay

## 状态

已采纳（2026-11-19）。实现：`cr-nat::relay_tunnel` + `cr-bridge` 的 tunnel 上游 +
`coldrevived` 的 opt-in 唤醒接线；**v1.1（2026-11-19 晚）补齐持有者侧 splice 与
OPEN/OPENED 握手**，见「v1.1 增补」。

## 背景：为什么以前说「中继没有 BT 数据面」

ADR-020 测到腾讯云 `119.29.150.254:7100` 的**入向 TCP** 被边缘注入 RST（0/30），
于是中继只被当作控制面；ADR-022 也因此把 `Route::Relay` 明写成「不登记 punched socket，
保持引擎原有出向」，并在 `/health` 常驻 `relay_data_plane="not_implemented"`。

这个结论只对该路径成立。2026-11-19 实测第二台云中继 `82.157.68.159:7100`：

| 发起端 | 结果 | p50 |
| :--- | :--- | :--- |
| 221（112.5.60.4，port-restricted cone） | 20/20，0 丢包 | 92.3 ms |
| 222（112.5.32.34，symmetric） | 20/20，0 丢包 | 81.4 ms |

TCP 帧本身可靠、有序，服务端已有 1 MiB 帧上限与 ADR-014 字节令牌桶。
所以**不需要自建 seq/ACK/重传**——那等于在 TCP 上重造 TCP。

## 决策

**数据面用 TCP 中继；UDP 继续做控制面/唤醒**（[ADR-020](ADR-020-relay-udp-transport.md)）。
两者不是替代关系：唤醒走 UDP（唯一在腾讯云路径上可用的承载），取片走可达的 TCP 中继。

### 线格式（不新增 `FrameKind`）

一条 `RELAY` 帧的 payload：

```text
magic(2) = b"CD" | channel(4, big-endian) | flags(1) | body
flags: 0x01 = DATA, 0x02 = CLOSE, 0x04 = OPEN, 0x08 = OPENED
```

- 用既有 `FrameKind::Relay` / `FrameKind::Delivered`，服务端无需改动。
- `channel` 让**一条中继连接**承载多条 BT peer 连接；每个 channel 独立关闭。
- body 用 16 KiB 分块（远小于 1 MiB 帧上限，也小于 ADR-014 的 4096 B 信令阈值，
  因此**会被字节令牌桶计费**——这是有意的：数据面必须占用预算，不能伪装成信令）。
- `CLOSE` 带空 body，双方对称关闭；超时也按 CLOSE 处理，避免半开 channel 泄漏。
- `OPEN`（0x04）body **必须**是 16 B 的发起方中继会话 id；`OPENED`（0x08）body 必须为空。
  长度不符直接拒绝——`OPEN` 少了 id 就做不了归属校验，多了就是解析歧义。

### 与 cr-bridge 的接法

`cr-bridge` 的 SOCKS5 前端不变，只把注册表的值从固定 `TcpStream` 扩成
`Upstream::{Direct(TcpStream), Tunnel(DuplexStream)}`：

- **未命中**仍回落真实直连（既有语义，不影响普通 BT）。
- **命中 tunnel** 时把 `DuplexStream` 交回引擎，`copy_bidirectional` 照常搬运字节。
- 引擎侧零改动：仍然是 `EngineOptions.proxy_url = socks5://127.0.0.1:<bridge>`。

### 为什么不让引擎直接连中继

librqbit 只有 `proxy_url` 一个出站钩子（[ADR-021](ADR-021-punched-tcp-bridge.md)），
没有自定义 connector。经本地 SOCKS5 是把外部 transport 接进引擎的唯一官方路径。
fork 引擎的代价此前已否决，不因换 transport 而重开。

## 后果与必须照写的降级

1. **中继必须可达才可启用数据面**。TLS/边缘策略可能只放行 UDP（如腾讯云那台），
   此时如实回落，不得声明「数据走中继」。启动时探测 TCP 中继可达性。
2. **多一次用户态拷贝**（duplex + SOCKS5），未做大包压测前不写吞吐结论。
3. **全局带宽受 ADR-014 约束**（默认 512 KiB/s 令牌桶、2 MiB 突发）。
   `bandwidth_limited` 增长是如实降级，不是 bug。
4. **UDP 承载不用于数据**：1168 B 上限、无重传，若要用必须先做可靠层，属独立工作。
5. **默认关闭**：`nat.punch_route=false` 时引擎 `proxy_url` 仍为 `None`，
   既有部署与 T0/T1/G1 逐字节不变。

## 验收口径

- 单测：帧编码/解码、channel 分流、多 channel 并存、CLOSE 对称关闭、超长 body 拒绝。
- 真中继集成：起 in-process `RelayServer`，两端各自 `RelayTunnelSet`，
  在 channel 上跑 1 MiB 往返并逐字节比对；另一 channel 不得串字节。
- 真引擎 e2e：下载方只配 SOCKS5 + 不可直连的假 peer 地址，
  登记键由 **tunnel** 提供；必须证明 `served_registered=1` 且下行字节 > 0，
  负对照不登记时 0 B。`is_s6_evidence=false`、`g2_claim=false`、`s6_status=not_run`。
- 公网 real-host 证据：`82.157.68.159:7100` 双端 TCP relaybench 各 20/20。

## v1.1 增补：双向握手与持有者侧 splice（v2.4.92）

v1.0 只证明了「承载」，没证明「两个真进程能自己接上」。缺口有两处，v1.1 补的是这两处。

### 1. OPEN / OPENED：登记前必须拿到对端的**确认**

「帧发出去了」不等于「对端接上了」。中继是纯帧路由，目标不在线时它只计 `dropped`，
不会替谁回一个「收到了」。所以：

- 发起方 `TunnelMux::open(channel, peer)` 后立刻 `send_open(本端会话 id)`，
  并 `wait_opened(3 s)`；**超时就失败**，不登记、回落直连，`notes` 里写明原因。
- 接受方收到 `OPEN` 才「就地开出」channel，拨本机引擎监听口，**拨通了才回 `OPENED`**。
  顺序不能反：先回 `OPENED` 再拨号，等于对下载方谎称「已经接上」，它那边会把一条空
  隧道登记进 SOCKS5 桥，最终表现成 BT 连接凭空 EOF。
- `OPEN` 声明 id 必须等于中继观测到的来源 id。中继不做归属校验，端上不做就等于
  谁都能往别人的 channel 里灌字节。

### 2. 持有者侧：订阅连接上分流 → 拨本地引擎 → 双向 splice

`relay_sub` 的订阅连接同时承担唤醒转发与数据面（**共用一条连接**，因为中继注册表按
node id 唯一，开第二条会把订阅表项顶掉）。分流只认 magic：

- payload 以 `CD` 开头 → 数据面；其余 → 唤醒路径，行为逐字节不变。
- 数据面 OPEN → 独立线程拨 `engine.listen` 并 `splice_blocking`，
  **不在订阅循环里同步做**：一条 BT 连接不能把唤醒路径一起卡住。
- 有活动 channel 时把订阅读超时从 500 ms 降到 20 ms，并在等唤醒应答的空隙里排空出向
  队列——否则一次唤醒就能把数据面拖慢 1.5 s。
- 字节数**边传边计**（`splice_blocking_with` 的进度回调）。只在收尾时一次性加，
  传输期间 `/health` 会一直显示 0，看起来像没通。

### 3. 会话 id 派生：数据面不能复用订阅的 id

`derive_session_id(node_id, b"cr-dp-v1") = sha256(id || tag)[..16]`。同一个 id 在中继
注册表里只有一个表项，数据面复用订阅 id 会让两条连接互相顶掉，现场表现是
「订阅随机失效」或「数据面莫名不通」。

### 4. 中继会合地址（`nat.relay_rendezvous`）

librqbit 只肯拨「看起来像公网 peer」的地址，而两台 NAT 后的机器谁都没有入向映射。
`nat.relay_rendezvous` 就是这个**会合键**：持有者把它作为 `bt_external` 通告出去
（`EndpointKind::Relay`），下载方的引擎照常拨号，本地 SOCKS5 桥把这次拨号改道到中继隧道。

- 只允许 RFC 5737 文档网段（`203.0.113/24`、`198.51.100/24`、`192.0.2/24`），
  写真实 IP 直接**启动失败**——否则 tracker 与可达性声明会把它读成「真的有公网入向」。
- `EndpointKind::Relay` 不在 `is_explicit_port_mapping()` 里，因此不会进入
  `s6`/公网入向的证据链；`tracker_endpoint_for` 对它是唯一一处「文档网段放行」，
  因为它的唯一消费者就是装有本地桥的对端。
- 必须与 `nat.punch_route=true` 同时配置，否则地址不可达，通告出去就是假 peer。

## 实施状态

### v2.4.91（承载与交接）

- `cr-nat::relay_tunnel`（4 例，含真 `RelayServer` 上 1 MiB 逐字节往返 +
  channel 不串流 + CLOSE EOF）；`cr-bridge::Upstream::{Direct,Tunnel}`（7 例，
  SOCKS5 前端不变）；真引擎 e2e 1 例（`served_registered=1`、1 MiB 逐字节一致，
  未登记负对照 0 B，中继 `data_bytes_admitted=1,050,071`）——
  证据 `tests/e2e/w57/reports/relay_data.json`。
- **A 的独立证据**：两个真实不同 NAT（221 = port-restricted cone / `112.5.60.4`；
  222 = symmetric / `112.5.32.34`）经 UDP 中继 200/200 往返、0 丢包，
  中继抓包同时看到两个公网源，报告 `tests/e2e/w56/reports/cross_nat_relay.json`。

### v2.4.92（双向握手 + 持有者 splice）

- `cr-nat::relay_tunnel`：`TunnelMux`（不拥有连接、由调用方驱动，出向逐帧带目的地）、
  `TunnelReader`/`TunnelWriter`（可 `split`，两半互不阻塞）、`splice_blocking_with`
  （进度回调 + 半关语义）、`derive_session_id`、`OPEN`/`OPENED` 形状校验。
- `relay_sub`：订阅连接上按 magic 分流；OPEN → 拨本地引擎 → splice，全在独立线程；
  `/health.holder_data` 暴露 opens/accepted/rejected/bytes_to_engine/bytes_from_engine。
- `data_plane`：派生会话 id 注册 + `send_open` + `wait_opened` 后才登记进桥。
- 引擎侧：`nat.punch_route=true` 时把对端通告的 `bt_external` 作为 `initial_peers`
  交给 librqbit，不再依赖「持有者必须有可达 tracker」。
- **仍未完成（不得冒充）**：
  1. UDP 承载的数据面仍未实现（1168 B 上限、无重传，需可靠层，属独立工作）。
  2. 「打洞失败 → 中继数据面接管」的公网双端完整链路（含真实 NAT 类型分类）
     尚未作为一次运行记录进证据；当前只有 crate 级、真引擎 e2e 与双进程中继证据。

## 复现

```bash
cargo test --offline -p cr-nat relay_tunnel      # 含 OPEN+splice 真中继用例
cargo test --offline -p cr-bridge
cargo test --offline -p coldrevived relay_sub     # 含生产 serve 的 splice 用例
CR_RELAY_E2E_REPORT="$PWD/tests/e2e/w57/reports/relay_data.json" \
  cargo test --offline -p cr-engine --test relay_data_e2e -- --nocapture
```

双进程公网证据见 `tests/e2e/w58/README.md`（运行脚本与原始输出在该目录）。
