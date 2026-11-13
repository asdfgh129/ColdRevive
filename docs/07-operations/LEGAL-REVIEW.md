---
doc_id: DOC-21
title: 法务评审材料包（S8）
category: 运维
version: 2.0
status: 草稿
owner: 项目组 / 法务顾问
updated: 2026-11-10
---

# 法务评审材料包（S8）

> 编号 DOC-21 · 版本 2.0 · 状态 **草稿** · 责任人 项目组 / 法务顾问 · 更新 2026-11-10
> 本文件是送审**材料包**，不是法律意见，也不替代 [OPS §6](OPS.md) 的红线正文。
> 对应 M0 Spike **S8**（[TECH-STACK §2](../02-architecture/TECH-STACK.md)）与风险 **R-11**（[RISK](../05-quality/RISK.md)）。

## 1. 为什么需要这份材料

- S8 于 2026-11-09 由项目所有者豁免，登记为 `pass`，但 `written_opinion_received = false`；
  `tests/spikes/spikes.json` 明确注明「不得据此声称已取得外部法律意见」。
- R-11 未消除；G4（M4 公测门禁）要求「法务评审通过」，[OPEN-QUESTIONS §3](../06-plan/OPEN-QUESTIONS.md) 要求 M4/G4 前复核。
- 因此需要一份可直接交给外部律师的**事实底稿 + 问题清单**，避免每次重新口头解释技术方案。

## 2. 一句话送审对象

ColdRevive 是通用 P2P 传输与索引协议的技术实现：用户在本机共享文件，系统为该文件计算哈希、
向公共 eMule KAD 发布一条**描述符**；其他节点按关键词检索到描述符后，经私有唤醒协议建立连接，
再用 BT 协议传输文件。软件不内置任何资源源、索引站或 bootstrap 列表。

## 3. 技术事实底稿（供法务判断，不含结论）

### 3.1 会向公网发布什么

发布路径：本地共享目录 → 计算 eD2K 哈希 / BT infohash → 关键词分词 → `PUBLISH_KEY_REQ` 写入公共 KAD
（[PROTO §2](../04-protocol/PROTO.md)）。载荷字段以 `tests/spikes/s2/kad-mini/src/main.rs::descriptor_tags` 为准：

| Tag | 内容 | 是否暴露具体内容 |
| :--- | :--- | :--- |
| `TAG_FILENAME` | 文件名（带 `ColdRevive_V1:` 命名空间前缀） | **是**，v1 发布明文文件名 |
| `TAG_FILESIZE` | 文件字节数 | 否（仅大小） |
| `TAG_SOURCEUPORT` | 源端 UDP 端口 | 否（端口） |
| `ColdRevive_V1:descriptor` | `v=1;node=…;wake_port=…;proto_flags=bt-only;infohash_prefix=none` | 否（协议元数据） |
| `ColdRevive_V1:namespace` | 命名空间常量 | 否 |

- 关键词本身以 128-bit 哈希作为 KAD Key 发布，线上不出现明文字符串。
- v1 的 `infohash_prefix=none`：不在 KAD 公开 infohash，取回还需经唤醒 + BT 元数据交换。
- 发布记录默认 `publish_ttl_sec = 1800` 到期即失效，不构成永久索引。

### 3.2 不做什么

红线正文见 [OPS §6](OPS.md)，此处只列送审要点：不内置资源站点 / 种子索引库 /
默认 bootstrap 列表 / 搜索源；不提供绕过版权保护、规避地域限制、隐藏身份或逃避监管的功能；
不做匿名化承诺；内容合法性由用户独立承担，首次启动须确认使用条款。

### 3.3 默认配置实体

| 参数 | 默认值 | 合规含义 |
| :--- | :--- | :--- |
| `[network] bind` | `127.0.0.1` | 默认不对外提供 RPC / UI，远程管理须显式改 `0.0.0.0` 并设密钥 |
| `[shared] dirs` | `["~/ColdRevive/Shared"]` | 仅指向本机目录；目录为空则不发布，用户须显式放入文件 |
| `[kad] bootstrap_mode` | `public` | 接入公共 KAD，**本项是评审核心** |
| `[kad] publish_ttl_sec` | `1800` | 发布 30 分钟过期，非永久索引 |
| `[kad] publish_rate` | `20` | 限速，降低被视作污染的风险 |
| `[security] rpc_secret` | `""` | 为空时强制仅允许回环监听 |
| `[security] cors_allowlist` | `[]` | 不默认放行浏览器跨域 |

> ⚠️ 口径待统一：[OPS §6](OPS.md) 写「共享目录为空」，而
> `config/coldrevive.example.toml` 给了默认路径 `~/ColdRevive/Shared`。送审前须统一为
> 「默认路径存在但为空时不产生任何发布」或直接改为空列表，避免向法务陈述与代码不一致。

## 4. 待法务确认的六个问题

| # | 问题 | 现状 | 若答案为「不可」的替代方案 |
| :--- | :--- | :--- | :--- |
| **L1** | 以「通用 P2P 工具」定位是否足以隔离帮助侵权责任？免责声明与用户确认流程的最低要求是什么？ | 首次启动展示使用条款 | 强制勾选 + 条款版本化留痕 |
| **L2** | 向公共 KAD 发布**明文文件名**是否构成「提供索引服务」？ | v1 明文文件名 | 只发布哈希与关键词，文件名改在握手后交换 |
| **L3** | 关键词以哈希形式发布，是否足以降低「索引」属性？ | 128-bit Key，线上无明文 | 已是当前最小化方案，可作抗辩点 |
| **L4** | 默认 `bootstrap_mode = "public"` 是否应改为 `private`（自建命名空间）？ | `public` | 改默认 `private`，`public` 需显式开启 |
| **L5** | 举报 / 下架机制的最低要求？（本地黑名单是否足够；是否需受理窗口与处理时限） | 本地黑名单 + 便捷举报 | 增加受理邮箱 / 表单 + 承诺处理时限 |
| **L6** | 不同司法辖区（EU / US / CN）差异是否需要按地区裁剪默认发布策略？ | 单一默认配置 | 按地区分发不同默认配置（[Q8](../06-plan/OPEN-QUESTIONS.md)） |

## 5. 结论登记（由法务填写）

| 项 | 内容 |
| :--- | :--- |
| 意见编号 / 日期 | *待填* |
| 出具人 / 机构 | *待填* |
| 结论 | *通过 / 有条件通过 / 不通过* |
| 条件与整改项 | *待填* |
| 覆盖范围（辖区 / 版本） | *待填* |
| 归档位置 | *待填（PDF 入库路径）* |

登记完成后须同步：`spikes.json` 的 `S8.legal_review.written_opinion_received` 改 `true`、
`resolution` 改为 `reviewed`、`evidence` 追加意见文件路径；并在
[RISK R-11](../05-quality/RISK.md) 更新处置记录与等级。

## 6. 与门禁的对应

| 时点 | 要求 | 当前 |
| :--- | :--- | :--- |
| M0 结束（G0） | 法务**初步意见**（可含条件） | 所有者豁免，无书面意见 |
| M4 结束（G4） | 法务评审**通过** | 未开始，R-11 保留 |

## 7. 变更记录

- 2.0（2026-11-10）：首版。汇编 [TECH-STACK §2](../02-architecture/TECH-STACK.md) S8、
  [OPS §6](OPS.md)、[RISK R-11](../05-quality/RISK.md)、
  [Q8](../06-plan/OPEN-QUESTIONS.md)；补 `descriptor_tags` 实发字段与默认配置实体，
  并标出「共享目录为空」口径不一致。

## 相关文档

- [DOC-04 TECH-STACK（S8 定义）](../02-architecture/TECH-STACK.md)
- [DOC-16 OPS（合规红线）](OPS.md)
- [DOC-13 RISK（R-11）](../05-quality/RISK.md)
- [DOC-15 OPEN-QUESTIONS（Q8）](../06-plan/OPEN-QUESTIONS.md)
- [DOC-00 文档体系说明](../README.md)
