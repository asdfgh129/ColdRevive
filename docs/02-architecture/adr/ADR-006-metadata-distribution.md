---
doc_id: ADR-006
title: 种子元数据分发：KAD 只存描述符
category: 架构决策
version: 1.0
status: 已接受
owner: 架构组
updated: 2026-11-09
---

# ADR-006：KAD 只存节点描述符，种子元数据走 HTTP / magnet `xs`

| 项 | 值 |
| :--- | :--- |
| 状态 | **已接受** |
| 决策者 | 架构组 |
| 相关 | [PROTO §2](../../04-protocol/PROTO.md) · [ADR-001](ADR-001-kad-network.md) · [ARCH §5 DR-07](../ARCH.md) |

## 背景

讨论中曾设想「通过 KAD 广播种子信息，让节点不依赖 Tracker 也能拿到 `.torrent`」。但 DHT 的 Value 有容量限制，`.torrent` 文件本身（尤其分块哈希列表）可能达数十 KB 以上，直接存入 KAD 会造成存储膨胀、发布失败率上升，并加剧 [RISK R-02](../../05-quality/RISK.md) 的公共网络污染争议。

## 选项

| 选项 | 评价 |
| :--- | :--- |
| A. KAD Value 直接内嵌 `.torrent` | 存储膨胀、发布受限，否决 |
| B. KAD 只存 InfoHash，让下载方自行去别处找种子 | 回到「找不到元数据」的原问题，否决 |
| C. **KAD 存轻量节点描述符 + 唤醒响应返回元数据 URL** | 索引与元数据解耦，**采纳** |

## 决策

- **KAD Value**：仅 `{ ip, wake_port, bt_port, proto_flags, infohash_prefix, version, timestamp, signature }`，不含种子本体。
- **元数据获取**：`WAKEUP_ACK` 返回 `torrent_url`（本机 `http://<node>:16800/torrent/<infohash>`）或 magnet 的 `xs=` 参数；UDP 响应只回 URL 与摘要，大响应一律走 HTTP。
- **撤销**：文件移出共享目录时发布 `TOMBSTONE`，并等待 TTL 自然过期。

## 后果

- 正面：KAD 发布量可控（限速 20/s 有意义）；公共网络干扰小；元数据可由持有方实时生成，始终与文件一致。
- 负面：下载方必须能连到持有者的 HTTP 端口，NAT 不可达时依赖 [LLD §5.9](../../03-design/LLD.md) 的中继回退。
- 约束：`WAKEUP_ACK` 中的 `infohash` 必须与请求校验一致，否则按 `HASH_MISMATCH` 拉黑（[PROTO §3](../../04-protocol/PROTO.md)）。

## 相关文档

[ADR 索引](README.md) · [PROTO KAD 规范](../../04-protocol/PROTO.md) · [API `/torrent/<infohash>`](../../04-protocol/API.md)
