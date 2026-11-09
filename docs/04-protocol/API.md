---
doc_id: DOC-10
title: 接口规范（API）
category: 协议
version: 2.0
status: 待评审
owner: 内核组 / 前端组
updated: 2026-11-09
---

# 接口规范（API）

> 编号 DOC-10 · 版本 2.0 · 状态 待评审 · 责任人 内核组 / 前端组 · 更新 2026-11-09
> 单端口设计见 [ARCH §3.1](../02-architecture/ARCH.md)；私有唤醒报文见 [PROTO](PROTO.md)。

## 1. HTTP 路径总表

守护进程默认监听 `127.0.0.1:16800`（[CONFIG](CONFIG.md)）。

| 路径 | 方法 | 用途 | 鉴权 |
| :--- | :--- | :--- | :--- |
| `/`、`/assets/*` | GET | 内置静态 Web UI（编译期嵌入） | 回环免密；远程需 secret |
| `/jsonrpc` | POST | aria2 兼容 JSON-RPC | `token:<rpc-secret>` |
| `/ws` | GET (Upgrade) | WebSocket 状态推送 | 同 `/jsonrpc` |
| `/torrent/<infohash>` | GET | 拉取伴随 `.torrent` 元数据 | 需校验 InfoHash 格式 |
| `/api/v1/search` | GET | KAD 关键词搜索（扩展接口） | secret |
| `/api/v1/shared` | GET / PUT | 共享目录与索引管理 | secret |
| `/api/v1/config` | GET / PATCH | 配置热更新 | secret |
| `/api/v1/status` | GET | 活跃池、内存、带宽、NAT 类型 | secret |
| `/api/v1/metrics` | GET | Prometheus 指标 | 建议仅回环 |

## 2. aria2 兼容 JSON-RPC

实现 aria2 常用方法子集，参数首位附 `token:<rpc-secret>`（与 aria2 契约一致）。

| 方法 | 映射 |
| :--- | :--- |
| `aria2.addUri` | 新建下载任务（含 magnet / ed2k / http） |
| `aria2.addTorrent` | 导入 `.torrent` |
| `aria2.tellStatus` / `tellActive` / `tellWaiting` | 任务状态查询 |
| `aria2.pause` / `unpause` / `remove` | 任务控制 |
| `aria2.getGlobalStat` | 全局速度、活跃池、内存、上行占用 |
| `coldrevive.search`（扩展） | KAD 关键词搜索 |
| `coldrevive.wake`（扩展） | 手动唤醒指定 InfoHash |
| `coldrevive.shared.*`（扩展） | 共享目录与索引管理 |

**兼容性目标**：AriaNg、Aria2 Explorer 等现成前端与浏览器插件可直接接入，零成本获得多端远程管理与下载捕获能力（[PRD C7](../01-product/PRD.md)）。

## 3. WebSocket 状态推送

- 事件类型：`task.*`（新增/进度/完成/错误）、`seed.*`（切入/切出/活跃池变化）、`node.*`（唤醒结果、节点在线）、`stat.*`（速度、内存、上行周期快照）。
- **UI 禁止周期性轮询**（[ADR-008](../02-architecture/adr/ADR-008-websocket.md)）；JSON-RPC 的 `tellStatus` 仅供外部生态按需单次查询。
- 断线按指数退避重连；重连后先拉一次全量快照再消费增量，避免状态空洞。
- 需处理背压：慢消费者超限则丢弃可合并的 `stat.*` 增量，不阻塞引擎线程。

## 4. 鉴权与暴露边界

| 规则 | 说明 |
| :--- | :--- |
| 默认绑定 | `127.0.0.1`，**默认不对局域网/公网开放** |
| `rpc_secret` | 为空时强制仅回环；配置后允许显式放行 |
| CORS | 默认空白名单；`--rpc-allow-origin-all` 等价能力默认关闭，仅对用户显式配置的源放行 |
| 传输 | 公网部署建议由用户自行加 TLS 反向代理；守护进程不内置证书管理 |
| 最小暴露 | 共享目录默认为空，未显式指定前不发布任何索引（[OPS §6](../07-operations/OPS.md)） |

## 5. CLI / REST / MCP（P2）

| 接口 | 场景 |
| :--- | :--- |
| `coldrevive-cli` | M1 MVP 入口：`search` / `wake` / `add` / `status` / `shared list` |
| REST `/api/v1/*` | 自动化脚本、NAS Web UI |
| MCP（可选） | AI 智能体管理下载与共享目录，参考 FluxDown 生态思路 |
| 浏览器插件 | 捕获下载链接推送到 `/jsonrpc`，复用 aria2 插件协议 |

## 6. 错误与可观测性

- HTTP 错误统一 JSON：`{ "error": { "code": "...", "message": "...", "trace_id": "..." } }`。
- 每个唤醒链路贯穿 `trace_id`，可跨「B → KAD → A → BT」端到端追踪（[SRS NFR-11](../01-product/SRS.md)）。
- 核心指标见 [OPS §3](../07-operations/OPS.md)。

## 相关文档

- [DOC-03 ARCH 进程与端口视图](../02-architecture/ARCH.md)
- [DOC-06 LLD §12 接入层](../03-design/LLD.md)
- [DOC-09 PROTO 协议规范](PROTO.md)
- [DOC-11 CONFIG 配置规范](CONFIG.md)
- [DOC-12 TEST 兼容性矩阵](../05-quality/TEST.md)
