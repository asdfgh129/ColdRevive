---
doc_id: DOC-19
title: 参考资料与待核实项（REFERENCES）
category: 参考
version: 2.0
status: 有效
owner: 架构组
updated: 2026-11-09
---

# 参考资料与待核实项（REFERENCES）

> 编号 DOC-19 · 版本 2.0 · 状态 有效 · 责任人 架构组 · 更新 2026-11-09
> **重要**：讨论记录中引用的第三方事实均未经二次验证，M0 Spike 阶段逐项核实；核实结论须回填本表并同步 [RISK](../05-quality/RISK.md)。

## 1. 标准与规范

| 规范 | 用途 |
| :--- | :--- |
| BEP-0003 | BitTorrent 协议（Peer Wire、Handshake） |
| BEP-0005 | Mainline DHT（160-bit ID 空间） |
| BEP-0009 | magnet 元数据交换 |
| BEP-0029 | uTP / LEDBAT 低优先级拥塞控制 |
| BEP-0055 | Holepunch 扩展（**适用性待核实**） |
| eD2K / KAD 协议文档 | 关键词字典、文件索引字典、MD4 分块 |
| JSON-RPC 2.0 | `/jsonrpc` 契约基础 |
| aria2 RPC 接口手册 | 兼容方法子集与 `token:` 鉴权 |

## 2. 参考项目

| 项目 | 借鉴点 | 落点 |
| :--- | :--- | :--- |
| hMule | KAD 索引 + BT 传输混合架构、KAD 抗攻击论证；失败教训 | [TECH-STACK §3](../02-architecture/TECH-STACK.md)、[LLD §13](../03-design/LLD.md) |
| BitComet | 长效种子、以文件哈希索引来源、跨协议补源 | [PRD §1](../01-product/PRD.md) |
| BiglyBT | Swarm Merging 内容识别思路 | [TECH-STACK §3](../02-architecture/TECH-STACK.md) |
| FluxDown | Rust + Tokio 三进程、引擎/UI 分离、aria2 JSON-RPC + MCP | [ARCH §3](../02-architecture/ARCH.md) |
| naria2 | 伴生进程 + `--ui` 一体化启动 | [ADR-003](../02-architecture/adr/ADR-003-ui-delivery.md) |
| aria2 / AriaNg / Aria2 Explorer | JSON-RPC 契约、`rpc-secret`、插件捕获 | [API §2](../04-protocol/API.md) |
| Bitmagnet / magnetico | DHT 爬虫索引的可行性与局限 | [PRD §1](../01-product/PRD.md) |
| rqbit / ed2k-rs | 候选 Rust 依赖 | [TECH-STACK §1](../02-architecture/TECH-STACK.md) |

## 3. 待核实事实清单

> ⚠️ 以下均为 `chat.md` 讨论中的引用值，**不作为设计前提**。M0 逐项确认后更新状态与结论。

| # | 待核实项 | 影响 | 关联 Spike / 风险 |
| :--- | :--- | :--- | :--- |
| V-01 | BT 种子平均寿命「9–13 天」的原始研究出处与量级 | 问题定义的量化依据 | [PRD §1](../01-product/PRD.md) |
| V-02 | eMule KAD 128-bit 与 Mainline DHT 160-bit ID 空间的具体互操作限制 | 双栈设计是否成立 | S3 / R-03 |
| V-03 | `rqbit` 对 Pause/Resume、自定义握手处理、按需实例化的支持程度 | 引擎选型 | S1 / R-05 / [ADR-002](../02-architecture/adr/ADR-002-bt-engine.md) |
| V-04 | `ed2k-rs` 的维护状态与 AICH 支持边界 | v1 哈希实现 | S7 / [ADR-004](../02-architecture/adr/ADR-004-ed2k-scope.md) |
| V-05 | FluxDown 的性能数据与三进程架构细节 | 仅作架构参考，不作指标依据 | [ARCH §3](../02-architecture/ARCH.md) |
| V-06 | hMule 与 aMule 合入被拒的具体技术理由（公平性论述） | 生态共存策略 | [LLD §13](../03-design/LLD.md) / R-01 |
| V-07 | BitComet 长效种子 / 跨协议补源的实现边界，是否可借鉴（不假设可复用代码） | v2 排期 | [ADR-004](../02-architecture/adr/ADR-004-ed2k-scope.md) / R-10 |
| V-08 | BEP-55 对本项目 UDP 打洞场景的适用性 | NAT 策略 | S6 / R-04 |
| V-09 | 公共 KAD 对 `ColdRevive_V1:` 命名空间大量发布的容忍度 | 生态借力是否成立 | S2 / R-02 / Q1 |

## 4. 核实状态流转

```
待核实 → 已确认（记录来源与结论） / 已证伪（记录影响并更新设计） / 不适用（说明理由）
```

- 已证伪项必须评估对 [ADR](../02-architecture/adr/README.md) 与 [ROADMAP](../06-plan/ROADMAP.md) 的影响，必要时新增 ADR。
- 已确认项须在正文引用处去掉「待核实」措辞。
- 本表由架构组在 M0 结束（G0 门禁）时集中回填一次。

## 相关文档

- [DOC-04 TECH-STACK Spike 清单](../02-architecture/TECH-STACK.md)
- [DOC-13 RISK 风险登记册](../05-quality/RISK.md)
- [DOC-15 待决问题清单](../06-plan/OPEN-QUESTIONS.md)
- [DOC-18 讨论记录映射](DISCUSSION-MAP.md)
