---
doc_id: ADR-001
title: 复用公共 eMule KAD 还是自建私有 DHT
category: 架构决策
version: 1.1
status: 待定
owner: 架构组
updated: 2026-11-09
---

# ADR-001：复用公共 eMule KAD 还是自建私有 DHT

| 项 | 值 |
| :--- | :--- |
| 状态 | **待定**（M0 / S2 后定稿） |
| 决策者 | 架构组 |
| 相关 | [ADR-007](ADR-007-tracker.md) · [TECH-STACK S2](../TECH-STACK.md) · [RISK R-02](../../05-quality/RISK.md) |

## 背景

ColdRevive 需要一个支持关键词与文件哈希检索的去中心化索引。eMule KAD 已具备「关键词字典 + 文件索引字典」两张表和成熟的公共节点网络，但从零自建 DHT 也没有现成用户。核心矛盾是**冷启动**与**对公共网络的影响**之间的取舍。

同时需确认：公共 KAD 网络是否接受专属命名空间的大量发布，是否会被现有实现视为污染而丢弃或屏蔽。这直接决定 [PRD C8](../../01-product/PRD.md) 的「生态借力」策略是否成立。

## 协议核实结论（2026-11-09，S2 P0）

按 aMule `master` @ `6a1105634d81242a581a0f36a7e00379b12a83f3` 源码核实，
[PROTO §2.0](../../04-protocol/PROTO.md) 已据此重写。以下结论**修正了本 ADR 原始措辞**：

1. **前缀在线上不可见**。`ColdRevive_V1:` 只作用于本地哈希输入，公共节点收到的是 128-bit ID。
   「公共 KAD 是否接受这个前缀」不是有效命题；应问「专属哈希空间是否与既有 KAD Key 互相覆盖 / 是否被索引策略丢弃」。
   `S2.criteria` 与 [RISK R-02](../../05-quality/RISK.md) 已改用该口径。
2. **KAD 不是通用 KV**。发布的是 `(fileID, Tag 列表)`，不是任意 Value；`Node_Descriptor` 需映射为 Tag。
3. **发布不是发到 bootstrap 就生效**。`SEARCHTOLERANCE = 2^24` 的距离校验会**静默丢弃**越界发布，
   必须先多跳迭代查找到 Key 邻域节点。因此「默认 20/s 全量启动分批发布」在生产侧需要重新评估，
   且与 M0 实网实验的 0.2 包/秒是两件事。
4. **报文形态不稳**。真实节点对同一 `BOOTSTRAP_REQ` 既可能回明文 `0xE4`，也可能回 `0xE5` + zlib 压缩帧；
   `HELLO_RES` 对 kadVersion ≥ 6 可能 RC4 混淆。`sender/receiver UDP key` 只用于反 IP 欺骗与
   「联系人已验证」标记，且绑定源 IP、不可跨主机复制，**不是明文包被丢弃的原因**。
5. **LAN 真实客户端 ≠ 公共 KAD 证据**。局域网内真实 eMule 可用于互操作探测，但它是私有子网单机，
   按 `S2.criteria` 第 5 条不得据此判 `pass`；若该客户端同时连着公共 KAD，向它发布测试 Key 还可能外泄。

> 这些是**协议事实**，不是 S2 结论。

> **2026-11-10 补充：S2 已判 `pass`，本 ADR 具备定稿条件。** 公共 KAD 的第三方节点接受了
> `PUBLISH_KEY_REQ`（`load=1`）并在 `SEARCH_RES` 中返回我们 5 个 tag 的原始条目；随后两进程 A/B
> （`split-a` 发布 / `split-b` 独立取回）确认跨节点可取回。早期五轮 0 响应不是网络或命名空间问题，
> 而是 Kad tag 帧头编码错误（详见 [PLAN §19–§21](../../../tests/spikes/s2/PLAN.md)）。
> 因此「生态借力」的**技术前提成立**；但「按运营速率是否被当污染」（R-02 的另一半）仍须 M4 前复审，
> 选项 4（`hybrid` 切换）保留不动。**定稿与 public/private 的签署仍由项目所有者执行，本文件不改状态。**

## 选项

| 选项 | 优点 | 缺点 |
| :--- | :--- | :--- |
| A. 完全复用公共 KAD | 现成引导节点、现成路由表、零冷启动成本 | 命名空间冲突风险、发布量受生态容忍度约束、依赖外部网络健康度 |
| B. 自建私有 DHT | 完全可控、无污染争议、可定制 Key 语义 | 节点数为零时毫无价值，重演 hMule 冷启动失败 |
| C. **混合（推荐）** | 默认借公共网络引导，保留私有切换能力 | 需维护两套发布/查询路径，实现复杂度上升 |

## 决策

**暂定选项 C**：配置 `bootstrap_mode` 默认 `public`，支持 `private` / `hybrid` 切换；所有 Key 强制使用
`ColdRevive_V1:` 命名空间哈希输入（详见 [PROTO §1](../../04-protocol/PROTO.md)），并对发布做限速（默认 20/s，
须按 `SEARCHTOLERANCE` 与迭代查找成本复核）。

若 S2 验证发现公共网络不接纳专属 Key 空间，或 [RISK R-02](../../05-quality/RISK.md) 触发
（**索引层拒绝率** > 20%，须先排除协议层不可解析 / 静默丢弃），则默认切换为 `private`，
并以「单机 / 私有小团体价值」作为冷启动破局点。

## 后果

- 正面：第一天即可接入现有网络，避免从零造网；命名空间隔离降低对 eMule 生态的干扰。
- 负面：M-05 需抽象 `KadBackend` Trait，屏蔽 public/private 差异；发布策略需持续观测。
- 约束：不得向公共 KAD 写入种子文件本体，见 [ADR-006](ADR-006-metadata-distribution.md)。

## 复核时点

M0 Spike S2 **实网小样本（P4）**结束后立即定稿；仅完成 P0–P3（协议核实、wire 回环、LAN 互操作、门禁参数）不构成定稿依据。**2026-11-10 S2 已 `pass`（实网 `PUBLISH_RES` + 命中 + 两进程 A/B 取回），定稿条件已满足，待所有者签署 `public` 默认模式。** 公测阶段（M4）按实际索引层拒绝率复审。

## 相关文档

[ADR 索引](README.md) · [ARCH §1](../ARCH.md) · [PROTO §2](../../04-protocol/PROTO.md) · [OPEN-QUESTIONS Q1](../../06-plan/OPEN-QUESTIONS.md)
