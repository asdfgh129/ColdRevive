---
doc_id: ADR-022
title: ADR-022 打洞优先 / 中继兜底：双端两阶段裁决，不允许「一端直连一端中继」
category: 架构决策
version: 1.0
status: 已定稿
owner: 架构组
updated: 2026-11-18
---

# ADR-022 打洞优先 / 中继兜底

## 状态

已采纳（2026-11-18）。实现：`cr-nat` 的 `punch_route`（纯状态机）+
`punch_wire`（线格式与收发）+ 守护进程 `punch_route`（计数与健康输出）。
**默认关闭**，`nat.punch_route=false` 时对既有部署零影响。

## 背景：两条路都是真的，缺的是「怎么选」

M2 到这一轮，三块能力都已独立验通：

| 能力 | 证据 | 缺口 |
| :--- | :--- | :--- |
| TCP 打洞原语（`cr_nat::tcp_punch`） | ADR-018 双平台回环 | 公网双侧仍 `BLOCKED` |
| 打通 socket 喂给引擎（`cr-bridge`） | ADR-021 真引擎 e2e | 守护进程未登记 punched socket |
| 中继（UDP 承载） | ADR-020 跨主机 840/840 | **只承载控制帧，没有 BT 数据面** |

缺的不是能力，是**选择规则**。而选择规则一旦写错，后果不是「慢一点」，是**双向黑洞**：
A 判定直连成功、把帧发到 punched socket；B 判定失败、留在中继上等。双方都不报错，
数据一个字节也过不去——这类故障在日志里最像「网络抖动」，排查成本极高。

## 决策

**一条链路、自动回落，但落点必须两端共识。**具体是两阶段裁决（two-phase commit）：

```text
阶段 1（Local）：双方各自报「本端握手是否成功」
阶段 2（Commit）：双方各自报「我是否接受直连」
最终路由 = 本端 ok ∧ 对端 Local ok ∧ 对端 Commit ok → Direct，否则 Relay
```

三个不可松动的实现约束：

1. **「发包成功」不等于「打通」。** TCP 的 `peer_addr()` 在 `SYN_SENT` 就返回目标地址（假阳性），
   判据只能是 `poll(POLLOUT)+SO_ERROR`（ADR-018）。同理，socket 建起来也不算数，
   必须**对端带着 `node_id + nonce` 回过来**——`validate_handshake` 会同时拒绝 nonce 不符
   （残留会话/重放）和回显本端 id（自己证明自己）。
2. **裁决走中继，不新增帧类型。** `RELAY` 帧的负载就是一条 `Verdict::encode()`
   （`side(16) + nonce(16) + stage(1) + ok(1)` = 34 B）；服务端转发时把来源放进
   `DELIVERED.frame.node`，`payload` 是原样裁决。**负载里不能再拼来源 id**——
   会和 `frame.node` 重复，解出来 `side/nonce/stage` 全错位，而错位后 `observe()` 以
   `NonceMismatch` 静默回落，表现得像「对端不支持」，这是本 ADR 实施期踩到的真坑
   （`payload_with_node_prefix_is_rejected` 是它的回归测试）。
3. **不只一端说了算。** 一端成功一端失败必须共同回落 `Relay`。这条由状态机强制：
   `route()` 只有「对端 Commit 已到 ∧ 本端倾向直连 ∧ 对端接受」才给 `Direct`。

## 明确不做：中继数据面

`Route::Relay` 的语义是**「不登记 punched socket，保持引擎原有出向行为」**，
**不是**「取片走中继」——中继目前只有 `REGISTER`/`RELAY`/`DELIVERED` 这类消息帧，
没有 BT 流隧道。健康输出里常驻 `relay_data_plane: "not_implemented"`，
就是为了防止有人把 `route=relay-fallback` 读成「中继已承载 BT 流量」。

补数据面是独立工作（ADR-014 的带宽预算要先过），不能用本 ADR 冒充。

## 后果

1. **symmetric 一侧每轮都会失败一次**，浪费一个 RTT 和一条映射表项，然后回落中继。
   对 symmetric 而言这是**正确结论**，不是 bug（ADR-019 的映射轴判定）。
2. **中继不能因为「今天打洞成功」就关掉**：它同时是信令通道（交换裁决）和兜底路径。
3. **opt-in**：`nat.punch_route=true` 才启用 CLI 路径与计数；默认 `false` 时
   `proxy_url` 保持 `None`，T0/T1/G1 与既有 M2 部署逐字节不变。
4. 唤醒协议（`cr-wake`）字段集合与 golden 文件**未改动**——裁决完全走中继消息，
   不需要协议版本升级。

## 验收口径与证据（v2.4.90，2026-11-18）

单元/集成（`cargo test -p cr-nat`）：

- `both_verified_converge_to_direct` / `one_sided_success_falls_back_to_relay` /
  `commit_before_local_is_out_of_order` / `nonce_mismatch_rejected` / `self_echo_rejected`
- `punch_wire` 四条**起真 `RelayServer` 的双端用例**：两端都 ok → 双方 `Direct`；
  A ok / B 不 ok → 双方 `Relay`；都不 ok → `Relay`（不是错误）；带来源前缀的负载被拒。

真引擎 e2e（`cr-engine/tests/route_negotiated_e2e.rs`）：

| 路径 | 断言 | 实测 |
| :--- | :--- | :--- |
| Direct | 协商 `Direct` 后登记 punched socket，取片走它 | `served_registered=1`、1 MiB 逐字节一致 |
| Relay | 协商 `Relay` 不登记，引擎回落直连 | `served_registered=0`、1 MiB 逐字节一致 |

机器可读：`tests/e2e/w55/reports/direct.json`、`tests/e2e/w55/reports/relay.json`
（均 `is_s6_evidence=false`、`g2_claim=false`、`s6_status=not_run`）。

**这些证据证明的是「决策与交接闸门」，不是公网可达性。** 公网双侧打洞仍
`BLOCKED`（ADR-018）；本 ADR 不改变 `g2_claim=false`。

## 复现

```bash
cargo test --offline -p cr-nat punch_route
cargo test --offline -p cr-nat punch_wire
CR_ROUTE_E2E_REPORT="$PWD/tests/e2e/w55/reports/direct.json" \
CR_ROUTE_E2E_RELAY_REPORT="$PWD/tests/e2e/w55/reports/relay.json" \
  cargo test --offline -p cr-engine --test route_negotiated_e2e -- --nocapture
```
