---
doc_id: ADR-013
title: 借鉴 eMule 的 LowID 自我声明，但不照搬「LowID 即终点」
category: 架构决策
version: 1.0
status: 已接受
owner: 项目所有者
updated: 2026-11-16
---

# ADR-013：借鉴 eMule 的 LowID 自我声明，但不照搬「LowID 即终点」

- 状态：Accepted（2026-11-16）
- 关联：[ADR-012](ADR-012-r04-best-effort.md)（R-04 取消数值承诺）、[ROADMAP W3.12](../../06-plan/ROADMAP.md)
- 实现位置：`cr-nat/src/reach.rs`、`cr-nat/src/relay.rs`（0x08–0x0B 帧）、`cr-nat/src/relay_server.rs`、`coldrevived/src/relay_sub.rs`

## 背景

项目所有者指出：eMule 网络同样存在这几类 NAT，而且**大多数节点就是 LowID**——
那为什么我们不直接借鉴 eMule 的做法？

这个质疑成立的部分必须先承认：在 ADR-012 取消数值承诺之后，此前「我们有数字所以要测」
的论证已经不完整了。eMule 面对同样的 NAT 世界，从不测量可达率，也运行了几十年。
**我们必须回答「借鉴之后还剩什么差别」，否则这个项目就是在重新发明一个更窄的 eMule。**

## eMule 怎么做（值得借的部分）

| eMule 的做法 | ColdRevive 的对应（本轮落地） |
| :--- | :--- |
| 客户端**自己知道**自己是 HighID / LowID | `ReachDeclaration`：由本机 NAT 实测结果推导，`reach.rs` |
| 登录服务器时**声明**，别的节点开连之前就知道结果 | `REGISTER_DECLARED (0x08)`：注册即声明，中继存储并跨实例同步 |
| 服务器记录每个源的 ID 等级 | 中继 `declared` 表 + `declared_direct/relay_only/unknown` gauge |
| 别的节点看到 LowID 就**不浪费一次注定失败的直连** | `lookup_reach (0x0A/0x0B)`：查到 `relay-only` 才有资格跳过直连尝试 |
| 服务器宕机换一台重登 | 多实例注册表快照 + 声明快照（`PEER_REACH 0x09`） |

核心思想只有一句：**失败应当被预先知道，而不是试出来。** 这正是我们此前指出的
「该走中继的偏去直连，白等超时、用户还不知道为什么」的解法。

## eMule 不能照搬的部分（剩下的差别）

1. **eMule 有「换一个源」，我们没有。**
   下载者要的是内容，哪个源给都行；LowID 的源连不上，换一个就是。
   冷启动唤醒要的是**那一台机器**，没有替补。所以 eMule 的 LowID 是终点，
   我们的 `relay-only` 必须是**另一条路**——中继在这里不是兜底，是产品的一部分。
2. **eMule 的 HighID ≠ 我们的 direct。**
   eMule 的 HighID 意味着「任何人现在就能主动连进来」（需要端口映射/公网可达）。
   我们的 `direct` 意味着「**双方配合打洞的前提下**能建立直连」：端口受限锥型 NAT
   对陌生来源不放行，但我方先发一个包、对端的回包就能进来（W3.6 实验室 30/30）。
   术语上借了 eMule 的形，语义必须写清楚，否则会把「可配合打洞」误读成「暴露在公网」。
3. **eMule 不中继数据，我们中继。**
   eD2k 服务器对 LowID 客户端几乎帮不上传输的忙（负载是整个文件，中继代价不可接受），
   所以 eMule 只能接受「这个源不可用」。我们中继的是**一次唤醒信令**（几百字节），
   代价低六个数量级——这正是我们能在 eMule 止步的地方继续往前走一步的原因，
   也是这个产品区别于「又一个 eMule」的存在理由。
4. **eMule 的分类等级是两个，我们的是三态。**
   HighID / LowID 之外，我们多了 `unknown`：没探测过、样本不足、CGNAT（行为取决于
   运营商侧那一层，不能按名字推断）。**unknown 不许当 lowid 用**——那会把「没测过」
   变成「连不上」，是本项目明令禁止的那类替换。`ReachClass::allows_skipping_direct()`
   只对 `relay-only` 返回 true，这条纪律写在代码里而不是文档里。

## 决定

1. **借**：自我声明 + 服务器记录 + 查询先行。四类新帧全部**纯追加**（0x08–0x0B），
   旧版中继收到只会计数拒帧，不会误解析。
2. **不借**：「LowID 即终点」「没有中继」「HighID 谁都能连」这三个语义。
3. **声明必须带有效期**（默认 1 小时）：类别是光猫的当前配置行为，不是机器的固有属性。
   过期即降级 `unknown`；时钟回拨也算过期——不能让一份旧声明因系统时间倒退而永久有效。
4. **CGNAT 不自动分类**：实验室合成的双层 CGNAT 直连 30/30，真实运营商 CGNAT 常常
   是对称型——两边都真实存在，所以「cgnat」这个名字本身不携带可达性信息，只能给 `unknown`。

## 部署顺序约束（运维必须知道）

**先升级中继，再升级客户端。** 反过来会让新客户端在注册阶段就被旧中继断开
（表现为「中继未回 ACK（可能是不支持声明帧的旧版中继）」）。
现网两实例（222 / 209）按此顺序滚动升级。

## 后果

- 正面：唤醒方第一次有了「提前知道」的能力；未探测的节点不再假装有结论。
- 代价：中继多一张表、四类新帧、一套 gauge；订阅线程多了 500ms 轮询。
- 边界：本轮只交付**原语 + 诊断消费**（`coldrevive-nat --lookup`）。唤醒路径**尚未**接入
  「先查再打洞」——那会改变信令流，须单独一轮做并实测。不宣称它已经生效。

## W5.2 补充（2026-11-17）：唤醒路径已消费声明

`wake_with_plan` 在构造任何直连候选、UDP socket、STUN `back` 或 `NAT_PUNCH` 之前，
先经 `lookup_reach_any` 向配置的中继列表查询对端声明：

- **只有**有效（未过 TTL）的 `relay-only` 才跳过全部直连候选，直接走中继；
- `direct` / `unknown` / 查不到 / 查询失败一律保留直连候选——`unknown` 不是低 ID 的证据；
- `ReachDeclaration::effective_at` 是唯一口径，过期声明降级 `unknown`；
- `nat wake <hash> <infohash> [kind]` 的第三个参数仍是**验收量具**显式旁路：
  强制 `private-lan` / `stun-mapped` 等单条路径时跳过该决策并在 `notes` 里明说。
  生产默认调用（不带第三参数）没有这个旁路。

返回值新增 `reach_lookup`（中继、类别、声明时刻、TTL、`skip_direct`、错误、RTT），
可由 CLI 直接核对；另有**只查不改**的 `nat reach`，供运维/验收在开抓包窗口前确认声明已经传播，
它不发包、不唤醒、不改状态。两者都不是新的 S6 证据，`s6_status=not_run` 不变。

**声明的在线前提（W5.2 实测补）**：`LOOKUP_REACH` 只反映中继当前存了什么，订阅连接的存活性
属于另一个层面。持有者订阅把本地 500 ms 读超时误判断链会每轮重连，而退出的旧连接无条件
`forget_local`，就会在重连间隙把新声明抹掉——查询如实回答「不知道」，但 wake 会退化成
先试直连。现在 `recv_frame_optional` 区分超时与断链（超时保活），服务端也只在旧连接仍是
当前注册表项时才清声明。这是**让声明可用的前置条件**，不是新的分类判据：`unknown` 依旧
不等于「连不上」。

`tests/e2e/w52/reach_gated_wake.py` 用真实 222（symmetric）/ A（port-restricted）/ 中继实测：
`class=relay-only`、`skip_direct=true`、候选只有 `relay`，222 侧抓包对 A 公网出口 **0 包**；
另起窗口用 `only_kind=stun-mapped` 强制直连必须抓到真实 UDP 包，证明「0 包」不是量具失灵。
详见 [TEST §2.10](../../05-quality/TEST.md)。
