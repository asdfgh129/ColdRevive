---
doc_id: ADR-003
title: UI 交付形态：守护进程内嵌 HTTP/WS
category: 架构决策
version: 1.0
status: 已接受
owner: 架构组
updated: 2026-11-09
---

# ADR-003：UI 交付形态采用守护进程内嵌 HTTP/WS

| 项 | 值 |
| :--- | :--- |
| 状态 | **已接受** |
| 决策者 | 架构组 |
| 相关 | [ADR-008](ADR-008-websocket.md) · [ARCH §3](../ARCH.md) · [TECH-STACK §4](../TECH-STACK.md) |

## 背景

早期讨论比较了三条为 aria2 增加内置 Web UI 的路线：外部静态 HTML 前端、伴生进程一体化容器、深度修改 aria2 C++ `HttpServer` 源码。三者分别对应「部署复杂」「进程分发复杂」「上游维护成本高」。

ColdRevive 是**从零自建的 Rust 守护进程**，不需要继承 aria2 的历史包袱，因此可以更直接地获得「深度嵌入」的收益。

## 选项

| 选项 | 评价 |
| :--- | :--- |
| A. 外部静态 HTML + 独立 Web 服务器 | 需用户自行部署，CORS/端口门槛高，否决 |
| B. 伴生进程托管 UI | 多一个进程需分发与保活，对自研守护进程是冗余，否决 |
| C. **守护进程内嵌 HTTP/WS 服务** | 单二进制、免 C++ 改造、UI 资源编译期嵌入、同端口无 CORS 问题，**采纳** |

## 决策

`coldrevived` 在同一端口（默认 16800/tcp）通过路径分流托管静态 UI、`/jsonrpc`、`/ws`、`/torrent/<infohash>`、`/api/v1/*`。UI 进程（Tauri 外壳或浏览器页面）与守护进程解耦，**可随时关闭且不影响后台下载与做种**。

进程模型：常驻 `coldrevived` + 轻量 `coldrevive-agent`（托盘/捕获/UPnP 续租/看护）+ 可关 UI，借鉴 FluxDown 的引擎与界面彻底分离。

## 后果

- 正面：单文件交付、无外部依赖、前后端仍解耦、NAS/Docker 场景可只用纯 Web UI。
- 负面：静态资源嵌入使二进制略增；守护进程需承担 HTTP 服务的攻击面（以默认回环 + `rpc-secret` + CORS 白名单缓解）。
- 约束：状态推送必须走 WebSocket，见 [ADR-008](ADR-008-websocket.md)。

## 相关文档

[ADR 索引](README.md) · [ARCH §3 进程视图](../ARCH.md) · [API §1](../../04-protocol/API.md) · [OPS §1](../../07-operations/OPS.md)
