---
doc_id: ADR-007
title: 分层利用 DHT，不完全取代 Tracker
category: 架构决策
version: 1.0
status: 已接受
owner: 架构组
updated: 2026-11-09
---

# ADR-007：分层利用 DHT，Tracker 降级为可选加速层

| 项 | 值 |
| :--- | :--- |
| 状态 | **已接受** |
| 决策者 | 架构组 |
| 相关 | [LLD §5.4](../../03-design/LLD.md) · [PRD C6](../../01-product/PRD.md) · [ADR-001](ADR-001-kad-network.md) |

## 背景

讨论后期提出「能否用 KAD 广播种子信息，彻底不依赖 Tracker」。全面评估后发现问题不在「能不能」，而在「不同 DHT 的职责本就不同」：

- **eMule KAD**（128-bit ID）：有关键词字典与文件索引字典，适合做**发现**。
- **Mainline DHT**（BEP-0005，160-bit ID）：按精确 InfoHash 查 Peer，适合做**兼容**。
- 两者 **ID 空间不兼容**，不能直接互相写入或互相取代。
- DHT 无论哪种都只是「雷达」，无法**激活**一个已休眠的冷节点——这正是 hMule 只做发现而失败的原因（[PRD C2](../../01-product/PRD.md)）。

## 选项

| 选项 | 评价 |
| :--- | :--- |
| A. 用 KAD 完全取代 Mainline DHT 与 Tracker | 旧客户端全部失联，违反向下兼容，否决 |
| B. 只用 Mainline DHT + 中心化索引 | 关键词检索仍靠外挂，回到原问题，否决 |
| C. **四层分工** | 各司其职且互不破坏，**采纳** |

## 决策

| 层 | 网络 / 机制 | 作用 | 内容 |
| :--- | :--- | :--- | :--- |
| 发现层 | **eMule KAD** | 关键词/文件哈希 → 在线持有者 IP | `ColdRevive_V1:` 命名空间索引 |
| 激活层 | **私有唤醒 RPC** | 触发对方从冷存储加载并临时做种 | UDP `WAKEUP_*` |
| 兼容层 | **Mainline DHT** | 让标准 BT 客户端能找到被激活的节点 | 按 InfoHash 发布 Peer |
| 加速层 | **Tracker（完全可选）** | 提升热门资源发现速度 | 标准 announce |

- KAD 与 Mainline DHT 维护**两张独立路由表与发布队列**，以 `file_hash` / `infohash` 关联，**不做跨网络 Key 转换**。
- Tracker 为可选项：热门资源可用它加速，冷门资源完全不依赖它；KAD 查询无结果时按「Mainline DHT 精确查 → 可选 Tracker → 本地索引」降级。

## 后果

- 正面：旧客户端零改造兼容；冷门资源不依赖任何中心化设施；职责清晰便于分别压测。
- 负面：双栈 DHT 带来实现与资源开销，是 [RISK R-03](../../05-quality/RISK.md) 与 M0 Spike S3 的重点验证对象。
- 开放问题：是否向公共 Mainline DHT 发布影子索引（可能放大「虚假在线」观感），见 [OPEN-QUESTIONS Q5](../../06-plan/OPEN-QUESTIONS.md)。

## 相关文档

[ADR 索引](README.md) · [LLD §5.4 双 DHT 适配层](../../03-design/LLD.md) · [FLOWS F2](../../03-design/FLOWS.md)
