---
doc_id: DOC-04
title: 技术选型与 M0 Spike 清单
category: 架构
version: 2.0
status: 待评审
owner: 架构组 / 内核组
updated: 2026-11-09
---

# 技术选型与 M0 Spike 清单

> 编号 DOC-04 · 版本 2.0 · 状态 待评审 · 责任人 架构组 / 内核组 · 更新 2026-11-09
> 本文记录技术栈选择、备选方案、**必须由 M0 验证的假设**。选型变更须新增 [ADR](adr/README.md)。

## 1. 选型总表

| 领域 | 首选 | 备选 | 理由 / 风险 |
| :--- | :--- | :--- | :--- |
| 语言 / 运行时 | **Rust + Tokio** | Go | 高并发 UDP/TCP、底层哈希、内存安全；避免 GC 抖动影响 100 MB 目标 |
| 构建 | Cargo Workspace，按 M-01～M-16 分 crate | 单 crate | 模块边界清晰，便于并行开发与单测 |
| 本地存储 | **SQLite (WAL, rusqlite)** | Sled | 事务、迁移、5 万行轻松；运维可查 |
| 文件监控 | `notify` crate | 平台原生 API | 跨平台 inotify / FSEvents |
| BT 引擎 | `rqbit`（M0 验证） | `libtorrent` FFI | 纯 Rust 优先；若不支持 Pause/Resume 或握手拦截则切 FFI |
| eD2K 哈希 | `ed2k-rs` / `ed2k` | 自实现 MD4 分块 | v1 仅需哈希；AICH 可不做 |
| KAD / DHT | 基于 Kademlia 实现适配层（参考 `mainline` / `libp2p` 思路） | 复用 aMule KAD 代码 | **eMule KAD 与 Mainline DHT 不兼容，必须双栈**；M0 重点 Spike |
| HTTP / WS | `axum` + `tower` | `hyper` 直接 | 同端口静态资源 + RPC + WS，路由清晰 |
| 信令序列化 | JSON（v1）→ Protobuf（可演进） | MessagePack | v1 便于调试，报文字节数受控 |
| NAT | UPnP/NAT-PMP/PCP crate + 自研打洞 | 依赖引擎内置 | 需独立自检与 LowID 判定 |
| UI | **Tauri 或纯 Web UI** | GPUI | 复用 Web 生态、开发效率高；UI 进程可独立关闭 |
| 打包 | 单二进制 + 嵌入静态资源 | 容器镜像（NAS 场景） | 兼顾桌面与 Docker/NAS |

## 2. M0 必须验证的选型假设（Spike 清单）

> 这是 [ROADMAP G0 门禁](../06-plan/ROADMAP.md) 的唯一判定依据。每项须输出「通过 / 不通过 / 替代方案」结论并归档。

| # | 假设 | 验证方式 | 失败时的替代 |
| :--- | :--- | :--- | :--- |
| S1 | BT 引擎支持按需实例化、Pause/Resume、自定义握手拦截 | 最小 Demo：拦截 Handshake → 动态加载种子 | 改用 `libtorrent` FFI 或自研精简做种层 |
| S2 | 可接入公共 eMule KAD，且专属命名空间不与现有流量冲突 | 双节点发布/查询闭环 | 切换 `bootstrap_mode = private` 自建命名空间 |
| S3 | KAD 128-bit 与 Mainline 160-bit 双栈可并行且资源可控 | 双栈压测 | 收敛为单一 DHT + 外部映射服务 |
| S4 | 1 万文件影子索引常驻 ≤100 MB | 模拟索引压测 | 索引分库 / 冷数据惰性加载 |
| S5 | uTP/LEDBAT 后台退让效果满足 NFR-03 | 前台下载 + 后台做种并发压测 | 强化令牌桶与连接数限制 |
| S6 | 常见家用 NAT 下的可达率 | NAT 类型矩阵实测 | 提升中继回退优先级 |
| S7 | 生成伴随种子的 CPU/IO 开销可接受 | 4 GB 文件造种基准 | 分片大小自适应 + 增量哈希 |
| S8 | 法律与合规边界（P2P 工具、索引发布、默认配置） | 法务评审 | 调整默认发布策略与免责/举报机制 |

**风险映射**：S1 → R-05、S2 → R-02、S3 → R-03、S4/S5/S7 → R-06/R-07、S6 → R-04、S8 → R-11，详见 [RISK](../05-quality/RISK.md)。

## 3. 参考项目与可复用资产

| 项目 | 借鉴点 | 警惕点 |
| :--- | :--- | :--- |
| **hMule** | KAD 索引 + BT 传输的混合架构、KAD 抗攻击论证 | 只做发现不做激活；冷启动失败；被上游因公平性拒绝合入 |
| **BitComet 长效种子** | 以文件哈希（而非种子）索引来源；跨协议补源已工程化 | 依赖中心化服务器，非开放标准 |
| **BiglyBT Swarm Merging** | 内容相同的不同 Swarm 合并思路 | 需内容识别能力 |
| **FluxDown** | 引擎/界面彻底分离的三进程模型、关窗不断下载、统一引擎多端复用、aria2 JSON-RPC + MCP 生态 | 其性能数据仅作量级参考，不作为设计前提 |
| **naria2** | 伴生进程 + `--ui` 一体化启动模式 | ColdRevive 自研守护进程，不需要伴生分发 |
| **aria2 / AriaNg / Aria2 Explorer** | JSON-RPC 契约、`rpc-secret`、WebSocket 优于轮询、插件捕获下载 | 不自研私有前端协议，直接兼容既有生态 |
| **Bitmagnet / magnetico** | DHT 爬虫索引的可行性与局限 | 属「外挂索引」，与本协议内发现定位不同 |

## 4. aria2 预研结论在 ColdRevive 中的映射

早期讨论围绕 aria2 的 RPC、内置 Web UI 与二次开发方案展开，其结论直接塑造了 [ARCH §3 进程视图](ARCH.md)。

| 预研结论 | 本项目的落地 |
| :--- | :--- |
| aria2 RPC 由高度定制的 `HttpServer` 实现，非通用 Web 服务器 | 自建守护进程，用通用 HTTP 框架同时承载静态 UI 与 RPC，无历史包袱 |
| 深度嵌入需改 C++ 源码、维护成本高 | 从零用 Rust 写，天然内嵌，无需改任何上游 |
| 伴生进程是工程性价比最高的路径 | 不需要：守护进程自身即服务；仅保留「UI 进程可关」的解耦 |
| 静态 UI 开销可忽略，瓶颈在 RPC 协议 | 强制 WebSocket 推送，禁止轮询 |
| CORS / rpc-secret / 端口暴露是主要配置门槛 | 默认回环 + 白名单 + secret，开箱即用且安全 |
| 一体化与外部静态方案性能几乎无差别 | 选一体化单二进制，兼顾交付简洁与性能 |

## 5. 引擎抽象约束

无论 ADR-002 最终选择哪个 BT 引擎，业务 crate **不得直接依赖**具体库：

- `cr-engine` 定义 `Engine` Trait：Pause/Resume、按需实例化、握手回调、分片校验。
- M-09 调度器与 M-10 拦截器只面向 Trait 编程。
- 这是 S1 失败时可切换 `libtorrent` FFI 而不推翻上层设计的前提。

## 相关文档

- [DOC-03 ARCH 总体架构设计](ARCH.md)
- [DOC-05 ADR 决策记录集](adr/README.md) — 尤其 ADR-001 / ADR-002 / ADR-003
- [DOC-14 ROADMAP M0](../06-plan/ROADMAP.md)
- [DOC-19 参考资料与待核实项](../08-reference/REFERENCES.md)
