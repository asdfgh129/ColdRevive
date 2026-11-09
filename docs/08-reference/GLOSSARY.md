---
doc_id: DOC-17
title: 术语表（GLOSSARY）
category: 参考
version: 2.0
status: 有效
owner: 架构组
updated: 2026-11-09
---

# 术语表（GLOSSARY）

> 编号 DOC-17 · 版本 2.0 · 状态 有效 · 责任人 架构组 · 更新 2026-11-09
> 术语在全部文档中保持统一；新增术语须先登记本表。

| 术语 | 英文 / 缩写 | 含义 |
| :--- | :--- | :--- |
| 死种 / 冷种 | Dead Torrent | 已无在线做种者、无法完成下载的 BT 种子 |
| 控制面 / 数据面 | Control / Data Plane | 前者负责发现与激活，后者负责实际文件传输 |
| 影子索引 | Shadow Index | 文件休眠后仍保留的索引记录与 DHT 通告，使节点「物理休眠、逻辑在线」 |
| 伴随种子 | Companion Torrent | 为共享目录中的普通文件自动生成的标准 `.torrent`，补齐 BT 所需的分片元数据 |
| 按需唤醒 | On-demand Wake-up | 收到私有信令或 BT 握手时才实例化做种上下文的机制 |
| 冷热分级 / LRU 活跃池 | Hot/Cold Tiering | 只让少量文件保持完整做种上下文，其余仅留索引 |
| 握手拦截 | Handshake Interception | 在 Reactor 层解析 BT Handshake 的 InfoHash 并触发被动唤醒 |
| KAD | Kademlia (eMule) | eMule 的 Kademlia DHT，含关键词字典与文件索引字典（128-bit ID） |
| Mainline DHT | BEP-0005 DHT | BT 的 DHT，按 InfoHash 查 Peer（160-bit ID） |
| Challenge-Response | — | 基于文件哈希与时间戳签名的一次性校验，防伪造唤醒与重放 |
| HighID / LowID | — | eMule 中「可被直接连接 / 需中继」的节点可达性状态 |
| LEDBAT / uTP | — | 低优先级拥塞控制，后台流量在前台拥塞时自动退让 |
| 跨协议补源 | Cross-protocol Sourcing | 从 eD2K 等其他网络取回缺失数据，经哈希转译后补齐 BT 分片 |
| Tit-for-Tat | — | BT 的短期互惠上传策略，与 eMule 的长期积分体系相对 |
| InfoHash | — | BT 种子的唯一标识（v1 为 SHA-1 of info dict） |
| magnet `xs` | eXact Source | magnet 链接中指向种子元数据地址的字段 |
| 命名空间前缀 | `ColdRevive_V1:` | 隔离公共 KAD 索引、避免污染与混淆的 Key 前缀，见 [PROTO §1](../04-protocol/PROTO.md) |
| 节点描述符 | Node Descriptor | KAD Value 中存放的轻量节点信息，不含种子本体 |
| 唤醒会话 TTL | `wake_session_ttl` | 单次唤醒的硬上限（默认 2 h），防止异常请求长期占用活跃池 |
| Spike | — | M0 阶段用于验证关键技术假设的最小实验，见 [TECH-STACK §2](../02-architecture/TECH-STACK.md) |
| ADR | Architecture Decision Record | 架构决策记录，一份一议题，见 [ADR 索引](../02-architecture/adr/README.md) |

## 编号速查

| 前缀 | 含义 | 所在文档 |
| :--- | :--- | :--- |
| `P1～P8` | 核心设计原则 | [PRD §4](../01-product/PRD.md) |
| `C1～C8` | 讨论沉淀的关键结论 | [PRD §5](../01-product/PRD.md) |
| `US-01～08` | 用户故事 | [PRD §6](../01-product/PRD.md) |
| `FR-xx` / `NFR-xx` | 功能 / 非功能需求 | [SRS](../01-product/SRS.md) |
| `M-01～16` | 模块编号 | [ARCH §4](../02-architecture/ARCH.md) |
| `F1～F6` | 端到端流程 | [FLOWS](../03-design/FLOWS.md) |
| `DR-01～08` | 架构决策摘要 | [ARCH §5](../02-architecture/ARCH.md) |
| `ADR-001～010` | 架构决策记录 | [ADR 索引](../02-architecture/adr/README.md) |
| `R-01～14` | 风险 | [RISK](../05-quality/RISK.md) |
| `S1～S8` | M0 Spike | [TECH-STACK §2](../02-architecture/TECH-STACK.md) |
| `T0～T3` | 测试环境 | [TEST §2](../05-quality/TEST.md) |
| `G0～G4` | 质量门禁 | [ROADMAP §3](../06-plan/ROADMAP.md) |
| `M0～M5` | 里程碑 | [ROADMAP §2](../06-plan/ROADMAP.md) |
| `W1～W8` | WBS 任务组 | [ROADMAP §1](../06-plan/ROADMAP.md) |
| `Q1～Q8` | 待决问题 | [OPEN-QUESTIONS](../06-plan/OPEN-QUESTIONS.md) |
