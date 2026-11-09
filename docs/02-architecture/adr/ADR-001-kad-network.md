---
doc_id: ADR-001
title: 复用公共 eMule KAD 还是自建私有 DHT
category: 架构决策
version: 1.0
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

同时需确认：公共 KAD 网络是否接受 `ColdRevive_V1:` 前缀的大量发布，是否会被现有实现视为污染而丢弃或屏蔽。这直接决定 [PRD C8](../../01-product/PRD.md) 的「生态借力」策略是否成立。

## 选项

| 选项 | 优点 | 缺点 |
| :--- | :--- | :--- |
| A. 完全复用公共 KAD | 现成引导节点、现成路由表、零冷启动成本 | 命名空间冲突风险、发布量受生态容忍度约束、依赖外部网络健康度 |
| B. 自建私有 DHT | 完全可控、无污染争议、可定制 Key 语义 | 节点数为零时毫无价值，重演 hMule 冷启动失败 |
| C. **混合（推荐）** | 默认借公共网络引导，保留私有切换能力 | 需维护两套发布/查询路径，实现复杂度上升 |

## 决策

**暂定选项 C**：配置 `bootstrap_mode` 默认 `public`，支持 `private` / `hybrid` 切换；所有 Key 强制带 `ColdRevive_V1:` 命名空间前缀，并对发布做限速（默认 20/s）。

若 S2 验证发现公共网络不接受专属命名空间，或 [RISK R-02](../../05-quality/RISK.md) 触发（发布失败率 > 20%），则默认切换为 `private`，并以「单机 / 私有小团体价值」作为冷启动破局点。

## 后果

- 正面：第一天即可接入现有网络，避免从零造网；命名空间隔离降低对 eMule 生态的干扰。
- 负面：M-05 需抽象 `KadBackend` Trait，屏蔽 public/private 差异；发布策略需持续观测。
- 约束：不得向公共 KAD 写入种子文件本体，见 [ADR-006](ADR-006-metadata-distribution.md)。

## 复核时点

M0 Spike S2 结束后立即定稿；公测阶段（M4）按实际发布失败率复审。

## 相关文档

[ADR 索引](README.md) · [ARCH §1](../ARCH.md) · [PROTO §2](../../04-protocol/PROTO.md) · [OPEN-QUESTIONS Q1](../../06-plan/OPEN-QUESTIONS.md)
