---
doc_id: DOC-25
title: ADR-019 公网观测点与单 IP 下的 NAT 分类边界
category: 架构决策
version: 1.0
status: 已定稿
owner: 架构组
updated: 2026-11-18
---

# ADR-019 公网观测点与单 IP 下的 NAT 分类边界

## 状态

已采纳（2026-11-18）。实现：`cr-nat::stun_server` + `cr-nat::stun::probe_port_filtering`；
CLI：`coldrevive-nat --serve-stun` / `--vantage`。

## 背景：现有分类结论不可证伪

M2 的 NAT 分类一直建立在「问公共 STUN」之上。2026-11-18 在前置机（家宽出口）实测基线：

```
VANTAGE=stun.l.google.com:19302 / stun.cloudflare.com:3478
mapped_address = 112.5.60.4:1239        (两台服务器映射一致 → 映射与目的地址无关)
filtering_behavior = port-restricted    → nat_type = port-restricted-cone
```

第二行是**编出来的**。`filter_behavior` 的判据是「带 CHANGE-REQUEST 的请求有没有回包」，
而 Google / Cloudflare / 绝大多数 3478 反射器**全部忽略 CHANGE-REQUEST**，永远从原地址回包。
于是「没有回包」有两种可能，客户端无法区分：

1. NAT 把这个方向的包丢了 —— 真有端口过滤（结论成立）；
2. 服务器压根没换源口 —— 服务器能力缺失（结论不成立）。

旧代码在模糊情形下返回 `PortRestrictedCone`，等于把「没测到」写成「测到了」。
仓库里甚至有一条测试 `filter_ignored_change_request_is_not_full_cone` 记录了这个行为——
它防住了「假 full-cone」，但没防住「假 port-restricted」。**两个方向都是编。**

结论：没有自己控制的回包源地址，过滤行为的任何结论都不成立。

## 决策

### 1. 自建 RFC 5780 观测点，不用公共 STUN 做过滤判定

`cr-nat::stun_server` 实现 BINDING 服务端，并支持 CHANGE-REQUEST：

| 请求 | 行为 | 为什么这样做 |
| :--- | :--- | :--- |
| 无 CHANGE | 从收到请求的套接字回包 | 正对照，客户端能确认「谁在回答」 |
| CHANGE-PORT | **从另一个套接字回包** | 源口真的变，才能区分「NAT 拦了」与「服务器没换」 |
| CHANGE-IP | 回 `420 + UNKNOWN-ATTRIBUTES(0x0003)` | 单公网 IP 做不到，必须**明说能力缺失**，不能沉默 |

CHANGE-IP 回 420 是这条 ADR 的关键：沉默会被客户端读成「NAT 拦了」，
而 420 让「服务器没这个能力」成为一条**可判定的事实**。

### 2. 三个 socket 的双对照，缺一即 `untested`

`probe_port_filtering` 的 socket 分工不可合并：

1. **对照 A**（主口直连）：证明观测点主口入向可达；
2. **对照 B**（备口直连，**另一个 socket**）：证明备口入向可达；
3. **被测 socket**：只与主口通信，再请求 CHANGE-PORT。

对照 B 必须换 socket。若复用被测 socket 先去戳备口，网关过滤集里就有备口地址，
「端口受限」会被测成「不限端口」——这是过滤测试最经典的假阳性，`stun.rs` 的注释里
已经因为同类原因栽过一次（`filter_behavior_fresh` 的存在理由）。

任对照失败、或回包源与备口不一致 → `FilterOutcome::Untested`，不给结论。

### 3. 分类结果分三层，不允许跨层升级

| 观测量 | `FilterBehavior` | `NatType` | 强度 |
| :--- | :--- | :--- | :--- |
| 双对照过 + change-port 无应答 | `port-restricted` | `port-restricted-cone` | 强结论 |
| 双对照过 + change-port 有应答且源=备口 | `port-open-address-untested` | `cone-undetermined` | 只测了端口轴 |
| 任对照失败 / 源被改写 | `unknown` | `unknown` | 无结论 |

新增 `NatType::ConeUndetermined` 的唯一理由：**它既不是 full-cone 也不是 restricted-cone**。
full-cone 要求「异 IP 也能进」，单 IP 观测点测不到；restricted-cone 要求「限地址」，
同样没测过。以前这种情形会被折成其中之一，两个方向都是猜。
打洞决策上它与 cone 同判（`is_cone_like()` → `ReachClass::Direct`），
因为打洞只依赖「映射可预测 + 不禁异端口」，与地址轴无关；但**类型名保留问号**。

## 后果

### 观测点自身也在 NAT 后（必须写在明处）

腾讯云轻量实例是 1:1 EIP：公网 `119.29.150.254` → 内网 `10.1.0.2`。
EIP 若改写源端口，客户端看到的 `src` 就不等于备口，`source_matches_alt` 会为 false，
结论自动降级为 `untested`。这条闸门是**故意**设的：观测仪器自身有偏差时，
宁可报「测不了」，也不能报一个被仪器污染的数。

### 单 IP 的能力上限

- 能测：入向是否允许**同 IP、异端口**（区分 APDF 与「非端口受限」）；
- 不能测：入向是否允许**异 IP**（无法区分 EIF 与 ADF）。

要补齐地址轴，需要第二块公网 IP 或第二台公网主机。**未补齐前，
任何 `full-cone` 结论都不得由本观测点产出**。

### 部署前提：公网入向端口

轻量服务器默认云防火墙只放 22 + ICMP。实测（2026-11-18）：

- 从家宽向 `3478 / 17800 / 17801 / 40001` 各发 3 个 UDP 包，共 12 个；
  服务器 `tcpdump -ni eth0` 过滤同一组端口 → **0 packets captured**；
- 同一时刻回环自测包正常收到（排除监听器问题）；
- TCP `80` / `443` 也超时（Caddy 在 `:80` 监听）→ 不是单端口问题，是整段默认拒绝；
- 机内 `ufw` inactive，`iptables` 只有腾讯云镜的 SSH 防爆破黑名单 → **不是本机防火墙**。

因此观测点可用性取决于控制台防火墙规则，这是运维前提，不是代码问题。

## 与门禁的关系

本 ADR **不改变** `g2_claim=false` 与 `s6_status=not_run`。
它做的是把「分类结论的依据」从「公共 STUN 的沉默」换成「自己控制的回包源」，
并明确标注单 IP 的能力边界。测出来的仍是**单点观测**，不是 S6 矩阵。
