---
doc_id: ADR-011
title: eD2k 双向互操作：v1 优先兼容旧客户端
category: 架构决策
version: 1.0
status: 已接受
owner: 项目所有者 / 架构组
updated: 2026-11-11
---

# ADR-011：eD2k 双向互操作，v1 以兼容旧客户端为优先

| 项 | 值 |
| :--- | :--- |
| 状态 | **已接受**（2026-11-11 项目所有者决策） |
| 决策者 | 项目所有者 |
| 相关 | [ADR-004](ADR-004-ed2k-scope.md) · [ADR-006](ADR-006-metadata-distribution.md) · [ADR-009](ADR-009-sharing-model.md) · [ADR-001](ADR-001-kad-network.md) · [PRD C3 / P1–P3](../../01-product/PRD.md) · [PROTO §2](../../04-protocol/PROTO.md) · [RISK R-02 / R-10 / R-11 / R-14](../../05-quality/RISK.md) |

## 背景

[ADR-004](ADR-004-ed2k-scope.md) 已接受「v1 不内置完整 eD2K 协议栈，只在 M-02 计算 `ed2k_hash`
并预留接口；全栈与跨协议补源排 v2，进入 M5 前须独立评审投入产出比并新增 ADR」。
[PRD P2 / C3](../../01-product/PRD.md) 承诺的兼容是**链接级**：唤醒后产出标准 magnet / ed2k 链接，
旧客户端拿链接即可下载；旧客户端本身不需要理解 ColdRevive。

2026-11-11 项目所有者提出更高一级的目标：**双向索引级互通** —— 我们的客户端能检索 eMule 的既有资源
（不必从零构建资源圈），旧客户端也能在 Kad 里搜到我们的资源（融入生态、用户迁移零损失）。
S2 已于 2026-11-10 判 `pass`，公共 KAD 的第三方节点确实收下并交还我们的条目，技术前提已具备。

本 ADR 履行 ADR-004 要求的「独立评审 + 新增 ADR」，并**部分取代**其排期结论。

## 四项所有者决策（2026-11-11）

| # | 决策 | 落点 |
| :--- | :--- | :--- |
| D1 | **所有文件公开可见**，共享即公开，以此实现资源共享 | 共享目录内全部文件默认进入公共 eD2k 关键词字典；不设 per-file 公开开关 |
| D2 | **eD2k 不区分冷热**，与 eMule 一致：共享即可见、可下载 | 冷热是 BT 做种状态，不映射到 eD2k 源；不引入唤醒网关 |
| D3 | **现在实现 eD2k 传输栈**；自有栈等客户端形成规模、且确有优势时再评估 | 优先兼容旧客户端；本 ADR 定义 L1 / L2 |
| D4 | **法务不作为 L1/L2 的前置门禁**，文档与策略可随时修改 | 不阻塞开工；S8 / R-11 保持登记，M4 前补书面意见 |

### D3 的口径澄清

项目里**没有「自有传输栈」这个构件**。[PRD P1 零协议侵入](../../01-product/PRD.md) 规定数据面 100%
使用标准 BT / eD2K；`WAKEUP_REQ` 是**私有控制面 RPC**，不是传输协议。因此 D3 的准确表述是：
当前不立项任何私有数据面；若将来客户端规模形成、且能证明专有优化有收益，须另立 ADR、先证优势再开工。
本 ADR 不改动 P1 / P3。

### D1 / D4 的两条技术事实（不改变决策，只登记约束）

1. 条目发布到第三方节点后按 TTL 存活。**停用发布可以阻止新扩散，但无法即时召回已经缓存在别人内存里的条目**；
   D4 中「随时修改」能改的是客户端行为与文档，不是已经扩散出去的索引。
2. 从「索引/发现」变成「分发」是责任性质的改变，不是文档措辞的改变。按 D4 由所有者承担，
   [PRD 非目标](../../01-product/PRD.md) 中「规避版权保护、绕过地域限制、提供匿名化保证」三条**不变**。

## 对 L3（唤醒网关）的审查结论：**不必要，予以否决**

上一轮分析曾提出「旧客户端只会发 `FILEREQUEST`，不会发唤醒包，因此冷文件必须由常在线网关
代理唤醒并流回分块」。**这个前提是错的，现予撤回。** 依据本项目自己的设计：

| 出处 | 事实 |
| :--- | :--- |
| [ADR-009](ADR-009-sharing-model.md) | 双层模型：体验层常态共享，**实现层按需唤醒** —— 休眠的是 BT 做种上下文，不是文件、不是节点 |
| [LLD §6](../../03-design/LLD.md) | `COLD` = 「≈ 仅一条索引行，无做种上下文」；切入时「**从磁盘加载元数据**」 → 文件始终在本机磁盘 |
| [ADR-006](ADR-006-metadata-distribution.md) | `WAKEUP_ACK` 返回的是 `http://<node>:16800/torrent/...`，**本机**就有数据与元数据 |
| [PRD US-06](../../01-product/PRD.md) | 关闭 UI 后守护进程常驻，共享不中断 |
| [LLD §7](../../03-design/LLD.md) | M-10 是「始终在线、无状态、开销极低」的监听器，旧 BT 客户端直接连入即可触发切入 |

即：发布描述符的节点**就是持有文件的节点，且常在线**。eD2k 源只需要一个常在线监听，
收到 `FILEREQUEST` 后**读本地磁盘、按 MD4 9.28 MB 分块发片**即可，与 M-10 握手拦截器完全同构。
eMule 自身也不区分冷热：文件在共享目录、客户端在线，就是可用源。

上一轮把「BT 未活跃做种」误当成「文件位于远端休眠节点」，凭空引入了「唤醒必须由第三方代发」的前提，
进而推出网关。该前提不成立，L3 因此是伪需求。

**残留情形**：节点真的关机 / 下线时，源会消失。这与 eMule 的源 TTL 行为一致，是网络常识而非缺陷，
不引入额外机制。影子索引（[PRD P5](../../01-product/PRD.md)）保证的是「逻辑在线可被搜到」，
不承诺「物理随时可下」——这一点对 eMule 与对我们自己的客户端同样成立，须在 `cr-kad`
的健康度字段里如实体现，避免虚假在线观感（见 [OPEN-QUESTIONS Q5](../../06-plan/OPEN-QUESTIONS.md)）。

## 四层能力（成本差一个数量级，不可混谈）

| # | 能力 | 协议动作 | 现有基础 | 增量 |
| :--- | :--- | :--- | :--- | :--- |
| ① | 我们检索 eMule 关键词字典（只看） | `SEARCH_KEY_REQ (0x33)`，Key 用无前缀关键词哈希 | `0x33`/`0x3B` 已实现、实网验证过 | 小 |
| ② | 我们从 eMule 下载 | eD2k TCP 客户端 | 无 | **大** |
| ③ | 旧客户端搜到我们 | `PUBLISH_KEY_REQ (0x43)` 发无前缀关键词条目 + `PUBLISH_SOURCE_REQ` 发源地址 | tag 帧头已修正；`ed2k_hash` 在 v1 入库（ADR-004） | 中 |
| ④ | 旧客户端下到我们 | eD2k TCP 服务端 | 无 | **大**（与 ② 同一份栈） |

**关键判断**：② 和 ④ 是同一份 eD2k 传输栈的两端，成本不是两倍、是一份。所以真正的决策是
「做不做 eD2k 栈」这一个二元问题；一旦实现，四个方向同时打通，只做一半则四个方向都不成立。
`PUBLISH_SOURCE_REQ` 的 opcode `0x44` 已在 `kad-mini/src/wire.rs::op` 的源码核实表里
（出处 `src/include/protocol/kad2/Client2Client/UDP.h`），但 [PROTO §2.0](../../04-protocol/PROTO.md)
**未描述其载荷格式**；L2 实现前须按 `KademliaUDPListener.cpp::Process2PublishSourceRequest`
与 `SendPublishSourcePacket` 补齐载荷，不得凭记忆写死。

## 源码核实：eMule 关键词 Key 是 MD4，不是 SHA-1

aMule `master` @ `6a110563`（与 PROTO §2.0 同一基线），`src/kademlia/kademlia/Kademlia.cpp`：

```cpp
void KadGetKeywordHash(const wxString& rstrKeyword, Kademlia::CUInt128* pKadID)
{
    uint8_t Output[16];
    CryptoPP::Weak::MD4 md4_hasher;
    char* ansi_buffer = strdup(unicode2UTF8(rstrKeyword));
    md4_hasher.CalculateDigest(Output, (const unsigned char*)ansi_buffer, strlen(ansi_buffer));
    free(ansi_buffer);
    pKadID->SetValueBE(Output);
}
```

`src/kademlia/utils/UInt128.cpp` 中 `SetValueBE` 与 `ToByteArray` 互为逆运算，
所以**线上 16 字节就是 MD4 摘要原值**。函数内**不做小写化、不 trim、不做 Unicode 归一化**，
只做 UTF-8 转换；归一化发生在调用方（搜索词构造），实现时须逐项核对，否则召回率会静默下降。

这与 [PROTO §1](../../04-protocol/PROTO.md) 的 `Keyword_Key = SHA1(NAMESPACE_PREFIX + "kw:" + term)`
是**两套不同算法**，双轨发布不得复用同一个哈希函数。

## 双轨发布（D1 + D2 的直接后果）

| 轨 | Key | 条目内容 | 面向 | 载荷 |
| :--- | :--- | :--- | :--- | :--- |
| A · 兼容轨 | `MD4(UTF-8(term))`，**无前缀** | `fileID = ed2k_hash`，tag 为 `FILENAME / FILESIZE / FILETYPE` 等 eMule 可解析字段 | eMule / aMule 及一切旧客户端 | 每文件 ≤ 8 关键词（沿用 PROTO 上限） |
| B · 私有轨 | `SHA1("ColdRevive_V1:kw:" + term)[0..16]` | `Node_Descriptor`（wake_port / bt_port / infohash_prefix / signature） | ColdRevive 自有客户端 | 同量级 |

自有客户端**同时查两轨**：A 轨给出 `ed2k://|file|<name>|<size>|<hash>|/` 与可用源，
B 轨给出 BT 高速通道与唤醒能力。发布量因此翻倍，且 A 轨真正进入对方的关键词条目池，
[RISK R-02](../../05-quality/RISK.md) 的判据从「专属命名空间是否被接纳」扩展为
「按运营速率的双轨发布是否被当污染」，M4 前必须按实测拒绝率复审。

## 范围

| 档 | 内容 | 状态 |
| :--- | :--- | :--- |
| **L1** | ① + 链接交接：`MD4` 关键词哈希、无前缀 `0x33` 检索、解析 `0x3B`、产出标准 `ed2k://` 链接；先在 `kad-mini` 用局域网友好节点验证字典互通 | **本 ADR 批准开工** |
| **L2** | ②④ 最小 eD2k 传输栈（下载与上传共用一份）+ ③ 对全部共享文件做双轨发布 | **本 ADR 批准开工**，M1 里程碑内落地 |
| L3 | 常在线网关代理冷文件 | **否决**（见上） |

两档都不改变 [P1 零协议侵入](../../01-product/PRD.md)：只实现标准 eD2K 报文，不改其格式。

## 后果

- 正面：直接兑现「复用既有资源圈」与「迁移零损失」，是 [RISK R-01](../../05-quality/RISK.md)
  冷启动风险最有力的缓解；旧客户端无需任何改造即可搜到、下到我们的文件。
- 正面：L3 被否决后，不需要网关带宽预算、不需要内容过境的法务判断、不引入新的常在线角色。
- 负面：**发布量翻倍** + 全部文件公开，A 轨条目与 eMule 真实文件争用同一批 Key 的存储配额，
  R-02 的生态压力实质上升；必须保留发布限速与 `private` 切换（[ADR-001](ADR-001-kad-network.md)）。
- 负面：eMule 的积分 / 排队 / 上传信用体系仍属 [PRD 非目标](../../01-product/PRD.md)。
  最小上传队列属于传输栈、要做；但**不实现 credit 体系**意味着我们下载别人时从零信用起步、
  排队靠后，冷启动期下载体验会明显低于原生 eMule 客户端。此项须在 M1 实测后复核是否补做。
- 约束：`fileID` 必须是**真实内容导出**的 `ed2k_hash`（MD4 over 9.28 MB 分块哈希），
  不可伪造；否则旧客户端搜到的是永远下不动的死条目。M-02 已有该字段，复用即可。
- 约束：A 轨 tag 必须严格按 Kad 的 `ReadTag` 格式（[PROTO §2.0 末条](../../04-protocol/PROTO.md)），
  任何 wire 变更先过 `lan-index-probe` 再上公网（PLAN §19.7 的方法论教训）。

## 复核时点

- **M1 末**：L2 实测 credit 缺失对下载排队的影响，决定是否补最小信用实现。
- **M4 前**：双轨发布按运营速率的实测拒绝率（R-02）；S8 / R-11 补书面法务意见（D4）。
- [OPEN-QUESTIONS Q7](../../06-plan/OPEN-QUESTIONS.md) 保留，但范围收窄为「MD4 9.28 MB ↔ BT
  动态分片的**哈希转译**」；eD2k 传输栈本身已由本 ADR 排入 v1，不再是 Q7 的待决项。

## 相关文档

[ADR 索引](README.md) · [PROTO §1/§2](../../04-protocol/PROTO.md) · [ARCH M-05 / M-12](../ARCH.md) ·
[SRS FR-12 / FR-24 / FR-25](../../01-product/SRS.md) · [PLAN §22](../../../tests/spikes/s2/PLAN.md)
