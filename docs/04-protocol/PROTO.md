---
doc_id: DOC-09
title: 协议规范（PROTO）
category: 协议
version: 2.1
status: 待评审
owner: 架构组 / 内核组
updated: 2026-11-09
---

# 协议规范（PROTO）

> 编号 DOC-09 · 版本 2.1 · 状态 待评审 · 责任人 架构组 / 内核组 · 更新 2026-11-09
> **本协议是私有控制面契约**。数据面 100% 使用标准 BT / eD2K 协议，不受本文约束。
> 任何字段变更必须递增 `PROTOCOL_VERSION` 并新增 [ADR](../02-architecture/adr/README.md)，禁止静默修改。
> **v2.1 只修正 §1/§2 的 eMule KAD 集成映射（外部协议映射）**，§3 私有唤醒报文的字段一字未改，
> 故 `PROTOCOL_VERSION` 保持 1、T0 golden files 不变；依据见 [§2.0](#20-kad-集成映射基线2026-11-09-源码核实)。

## 1. 命名空间与常量

```
NAMESPACE_PREFIX = "ColdRevive_V1:"     # 只作用于本地哈希输入，线上不可见
KAD_ID_LEN       = 16                   # 128 bit，eMule KAD ID 空间（不是 SHA-1 的 160 bit）
KAD_ID(x)        = x[0..16]             # 暂定截断规则，待 ADR 定稿，见 §2.0
Target_Key       = KAD_ID(SHA1(NAMESPACE_PREFIX + file_hash))
Keyword_Key      = KAD_ID(SHA1(NAMESPACE_PREFIX + "kw:" + normalized_term))
Node_Descriptor  = { ip, wake_port, bt_port, proto_flags, infohash_prefix,
                     version, timestamp, signature }   # 发布时编码为 KAD Tag 列表
PROTOCOL_VERSION = 1                    # 私有控制面版本，本次不变
```

> 版本前缀写进 Key，未来协议不兼容时可通过 `V2` 平滑隔离，不污染旧索引。
> **前缀不是线上字符串**：公共节点只看到 128-bit ID，「命名空间隔离」由哈希输入实现，
> 不能假设节点会识别或拒绝 `ColdRevive_V1:` 字面量（§2.0）。
> `node_secret_seed` 为节点本地随机种子，仅用于 Challenge-Response 签名，不参与跨节点身份体系。

## 2. KAD 发布 / 查询规范

### 2.0 KAD 集成映射基线（2026-11-09 源码核实）

固定依据：aMule `master` @ `6a1105634d81242a581a0f36a7e00379b12a83f3`。逐条出处与探针实现见
[tests/spikes/s2/kad-mini/README.md](../../tests/spikes/s2/kad-mini/README.md) 与
[S2 排期](../../tests/spikes/s2/PLAN.md)。**不引用二手资料的 opcode 表**（核查中发现
DeepWiki 把 `PUBLISH_KEY_REQ` 误记为 `0x35`，源码中 `0x35` 实为 `SEARCH_NOTES_REQ`）。

| 设计文档旧表述 | 真实 eMule Kademlia v2 | 本规范采用 |
| :--- | :--- | :--- |
| `FIND_VALUE` | **无此操作**。关键词查询是 `KADEMLIA2_SEARCH_KEY_REQ (0x33)` → `KADEMLIA2_SEARCH_RES (0x3B)` | 全文以 `SEARCH_KEY` 取代 `FIND_VALUE` |
| `Target_Key = SHA1(...)` | KAD ID 空间 **128-bit**，帧头 `0xE4`，Kademlia2 版本 `0x08` | `SHA1` 前 16 字节；截断规则待 ADR 定稿 |
| KAD = 通用 KV | `PUBLISH_KEY_REQ (0x43)` 存的是 `(fileID/sourceID, Tag 列表)` | `Node_Descriptor` 编码为命名空间 Tag |
| 前缀可线上识别 | 线上只有 128-bit ID | 隔离 = 不同哈希空间不互相覆盖 |
| 发布到 bootstrap 即可 | `Process2PublishKeyRequest` 有 `SEARCHTOLERANCE = 2^24`，越界**静默丢弃** | 发布前必须 `KADEMLIA2_REQ/RES (0x21/0x29)` 多跳迭代收敛 |
| 响应都是明文 `0xE4` | >200 字节的响应改用 `0xE5` + zlib 压缩帧；`HELLO_RES` 对 kadVersion ≥ 6 可能 RC4 混淆 | 客户端须能分类 `0xE4` / `0xE5` / 非 KAD 帧，并分开计数 |

`sender/receiver UDP key` 的作用是**反 IP 欺骗与标记联系人已验证**，不是「明文包一律丢弃」的加密门禁：
`ClientUDPSocket.cpp` 对首字节 `0xE4` 直接放行，`PacketTracking.cpp` 的 key 形参实际未启用。
该 key 形如 `(m_key, m_ip)` 且 `GetKeyValue(myIP)` 仅在 `myIP == m_ip` 时返回，**绑定原机器公网 IP，
不可从 `nodes.dat` 复制到别的主机使用**。因此 v1 不实现它，也不实现 RC4；实网小样本跳过 `HELLO` 路径
（`BOOTSTRAP_RES` / `KADEMLIA2_RES` / `PUBLISH_RES` / `SEARCH_RES` 的发送端均传 `cryptTargetID = NULL`）。

| 项 | 规定 |
| :--- | :--- |
| Key | 见 §1，**只使用专属命名空间** |
| Value | **仅轻量节点描述符**（IP、端口、协议标识、InfoHash 前缀、时间戳、签名），编码为 KAD Tag 列表；**不存放 `.torrent` 本体**（[ADR-006](../02-architecture/adr/ADR-006-metadata-distribution.md)） |
| 发布流程 | `BOOTSTRAP_REQ/RES` → `KADEMLIA2_REQ/RES` 迭代收敛 → `PUBLISH_KEY_REQ/RES`；查询为 `SEARCH_KEY_REQ/RES` |
| 元数据获取 | 唤醒响应返回 `torrent_url`（本机 HTTP `/torrent/<infohash>`），或使用 magnet 的 `xs=` 参数 |
| 发布频率 | TTL 30 min 续期；产品侧全量启动分批限速 20/s（[ADR-001](../02-architecture/adr/ADR-001-kad-network.md)）。**M0 实网实验另按 G0 确认的 0.2 包/秒执行**，二者不可混淆 |
| 关键词上限 | 每文件 ≤ 8 个 Key，避免 KAD 存储与查询负担 |
| 撤销 | 文件移出共享目录时发布 `TOMBSTONE` 并等待 TTL 自然过期 |
| 结果处理 | 去重 → 黑名单过滤 → 信誉排序 → 上限 200 条 → 逐条尝试唤醒 |

### 2.1 查询与降级顺序

1. 关键词 → 多个 `Keyword_Key` → KAD `SEARCH_KEY_REQ` 迭代查找
2. 精确 `Target_Key` 查询（已知 `file_hash` 时）
3. 无结果 → Mainline DHT 按 InfoHash 精确查
4. 仍无结果 → 可选 Tracker announce
5. 最后回退本地索引（自建节点自查）

> KAD 关键词为**精确哈希匹配**，不支持通配符；分词与模糊重排策略见 [FLOWS §2](../03-design/FLOWS.md)。

## 3. 私有唤醒协议（PROTOCOL_VERSION = 1）

### 3.1 传输

- UDP 单包请求/响应（轻量、无连接、便于打洞）。
- 大响应（`.torrent` 本体）一律改走 HTTP 接口；UDP 只回 URL 与摘要。
- 请求方 1.5 s 超时重试一次，随后切换候选节点，避免 TCP 长超时。

### 3.2 `WAKEUP_REQ`

```json
{
  "v": 1,
  "type": "WAKEUP_REQ",
  "trace_id": "uuid",
  "file_hash": "sha256...",
  "infohash": "optional-bt-infohash",
  "nonce": "random-16b",
  "ts": 1762650000,
  "challenge": "hmac(...)",
  "client_version": "coldrevive/0.1.0",
  "capabilities": { "bt": true, "utp": true, "ed2k": false }
}
```

- `challenge = HMAC(node_secret_seed, file_hash || utc_date || nonce)`
- 接收方校验时间窗 ±5 min、nonce 去重、文件在共享白名单内。
- `capabilities.ed2k` 在 v1 固定为 `false`（[ADR-004](../02-architecture/adr/ADR-004-ed2k-scope.md)）。

### 3.3 `WAKEUP_ACK`

```json
{
  "v": 1,
  "type": "WAKEUP_ACK",
  "trace_id": "uuid",
  "status": "OK",
  "magnet": "magnet:?xt=urn:btih:...&xs=http%3A%2F%2F...",
  "torrent_url": "http://<node>:16800/torrent/<infohash>",
  "file_size": 123456789,
  "piece_size": 1048576,
  "ttl": 7200,
  "bt_port": 16802
}
```

请求方必须校验 `magnet` 中的 InfoHash 与请求一致，不一致按 `HASH_MISMATCH` 处理并拉黑节点。

### 3.4 错误码

| 码 | 含义 | 请求方动作 |
| :--- | :--- | :--- |
| `REJECT_NOT_FOUND` | 文件不在共享白名单 | 换节点，不拉黑 |
| `REJECT_FILE_GONE` | 索引命中但文件已删除/改名 | 换节点；接收方须撤销该 Key 的 KAD 发布 |
| `REJECT_BAD_CHALLENGE` | 签名/时间窗/nonce 校验失败 | 重试一次后放弃 |
| `REJECT_RATE_LIMITED` | 触发限流 | 按 `retry_after` 退避 |
| `REJECT_BUSY` | 活跃池/并发已满 | 退避后重试或换节点 |
| `REJECT_RESOURCE` | 内存/带宽越限 | 换节点 |
| `REJECT_VERSION` | 协议版本不兼容 | 换节点 |
| `HASH_MISMATCH` | 请求方侧元数据校验失败 | **拉黑 + 信誉扣分** |

错误码落库为 [DATA `wake_log.result`](../03-design/DATA.md)，异常路径见 [FLOWS §8](../03-design/FLOWS.md)。

## 4. 版本与兼容

| 规则 | 说明 |
| :--- | :--- |
| 版本字段 | 所有报文带 `v`；不兼容版本直接 `REJECT_VERSION` |
| 命名空间隔离 | 协议不兼容时 Key 前缀递增为 `V2`，新旧并存一个 TTL 周期 |
| 演进顺序 | 新增 ADR → 递增 `PROTOCOL_VERSION` → 更新本文 → 更新契约测试 golden files |
| 映射修订例外 | 只改 §1/§2 的**外部 KAD 集成映射**、不动 §3 `WAKEUP_REQ` / `WAKEUP_ACK` 字段时，递增本文 `version`（2.0 → 2.1）即可，`PROTOCOL_VERSION` 与 T0 golden 保持不变；须在本文记录源码依据 |
| 禁止事项 | 禁止静默改字段、禁止复用旧错误码语义、禁止在 Value 中塞种子本体 |

## 5. 安全约束

- 所有外部输入（DHT 返回值、唤醒报文、种子元数据）必须校验后才采信。
- 限流默认：单 IP 5 次/分钟、全局并发 8、单文件并发 3。
- `REJECT_NOT_FOUND` 用于非白名单文件，避免成为文件存在性探测接口。
- 详见 [LLD §5 / §11](../03-design/LLD.md) 与 [SRS NFR-08](../01-product/SRS.md)。

## 相关文档

- [DOC-07 FLOWS 端到端流程](../03-design/FLOWS.md)
- [DOC-08 DATA 数据模型](../03-design/DATA.md)
- [DOC-10 API 接口规范](API.md)
- [DOC-11 CONFIG 配置规范](CONFIG.md)
- [DOC-12 TEST 契约测试](../05-quality/TEST.md)
