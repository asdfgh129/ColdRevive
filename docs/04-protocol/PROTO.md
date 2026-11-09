---
doc_id: DOC-09
title: 协议规范（PROTO）
category: 协议
version: 2.0
status: 待评审
owner: 架构组 / 内核组
updated: 2026-11-09
---

# 协议规范（PROTO）

> 编号 DOC-09 · 版本 2.0 · 状态 待评审 · 责任人 架构组 / 内核组 · 更新 2026-11-09
> **本协议是私有控制面契约**。数据面 100% 使用标准 BT / eD2K 协议，不受本文约束。
> 任何字段变更必须递增 `PROTOCOL_VERSION` 并新增 [ADR](../02-architecture/adr/README.md)，禁止静默修改。

## 1. 命名空间与常量

```
NAMESPACE_PREFIX = "ColdRevive_V1:"
Target_Key       = SHA1(NAMESPACE_PREFIX + file_hash)          # 文件精确查找
Keyword_Key      = SHA1(NAMESPACE_PREFIX + "kw:" + normalized_term)
Node_Descriptor  = { ip, wake_port, bt_port, proto_flags, infohash_prefix,
                     version, timestamp, signature }
PROTOCOL_VERSION = 1
```

> 版本前缀写进 Key，未来协议不兼容时可通过 `V2` 平滑隔离，不污染旧索引。
> `node_secret_seed` 为节点本地随机种子，仅用于 Challenge-Response 签名，不参与跨节点身份体系。

## 2. KAD 发布 / 查询规范

| 项 | 规定 |
| :--- | :--- |
| Key | 见 §1，**只使用专属命名空间** |
| Value | **仅轻量节点描述符**（IP、端口、协议标识、InfoHash 前缀、时间戳、签名），**不存放 `.torrent` 本体**（[ADR-006](../02-architecture/adr/ADR-006-metadata-distribution.md)） |
| 元数据获取 | 唤醒响应返回 `torrent_url`（本机 HTTP `/torrent/<infohash>`），或使用 magnet 的 `xs=` 参数 |
| 发布频率 | TTL 30 min 续期；全量启动时分批限速 20/s |
| 关键词上限 | 每文件 ≤ 8 个 Key，避免 KAD 存储与查询负担 |
| 撤销 | 文件移出共享目录时发布 `TOMBSTONE` 并等待 TTL 自然过期 |
| 结果处理 | 去重 → 黑名单过滤 → 信誉排序 → 上限 200 条 → 逐条尝试唤醒 |

### 2.1 查询与降级顺序

1. 关键词 → 多个 `Keyword_Key` → KAD `FIND_VALUE`
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
