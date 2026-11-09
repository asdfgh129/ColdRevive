---
doc_id: ADR-008
title: 状态推送一律 WebSocket，禁止 UI 轮询
category: 架构决策
version: 1.0
status: 已接受
owner: 架构组 / 前端组
updated: 2026-11-09
---

# ADR-008：状态推送一律 WebSocket，禁止 UI 高频轮询

| 项 | 值 |
| :--- | :--- |
| 状态 | **已接受** |
| 决策者 | 架构组 / 前端组 |
| 相关 | [ADR-003](ADR-003-ui-delivery.md) · [ARCH §3.1](../ARCH.md) · [SRS FR-21](../../01-product/SRS.md) |

## 背景

aria2 预研中对比了 HTTP 轮询与 WebSocket 的事件通知机制，并出现「HTTP 轮询经隧道转发导致 CPU 约 90%」的实际案例，切换 WebSocket 后显著下降。结论是：**静态 UI 放在哪里几乎不影响性能，真正决定 CPU 的是 RPC 通信协议**。

ColdRevive 的 UI 需要持续展示任务进度、速度、活跃池、内存/带宽与节点事件，若采用 1 秒级轮询，守护进程将长期承担无意义的请求解析开销，直接威胁 [SRS NFR-02](../../01-product/SRS.md) 的「空闲 CPU ≤ 2%」。

## 选项

| 选项 | 评价 |
| :--- | :--- |
| A. UI 定时轮询 `tellActive` | 空闲时仍持续消耗 CPU，否决 |
| B. 长轮询 | 连接管理复杂、仍有周期性开销，否决 |
| C. **WebSocket 服务端推送** | 空闲时请求数趋近 0，**采纳** |

## 决策

- 守护进程暴露 `/ws`，状态变化由服务端主动推送；UI 建立单条长连接并按事件增量更新。
- **禁止**在 UI 中实现任何周期性状态轮询；`aria2.tellStatus` 等 JSON-RPC 方法保留给外部生态做按需单次查询，不作为 UI 数据源。
- 连接断开时按指数退避重连，重连成功后先拉一次全量快照再消费增量，避免状态空洞。

## 后果

- 正面：空闲态 RPC 请求数趋近 0，满足 NFR-02；与 aria2 JSON-RPC 生态互不干扰。
- 负面：需处理连接生命周期、背压与重连后的状态一致性；前端状态管理略复杂。
- 约束：WebSocket 与 `/jsonrpc` 共用同一鉴权边界（[API §4](../../04-protocol/API.md)）。

## 相关文档

[ADR 索引](README.md) · [ARCH §3.1 单端口设计](../ARCH.md) · [API 接口规范](../../04-protocol/API.md) · [TECH-STACK §4](../TECH-STACK.md)
