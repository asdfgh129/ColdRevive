---
doc_id: DOC-16
title: 发布、运维与合规（OPS）
category: 运维
version: 2.0
status: 待评审
owner: 运维组 / 项目组
updated: 2026-11-09
---

# 发布、运维与合规（OPS）

> 编号 DOC-16 · 版本 2.0 · 状态 待评审 · 责任人 运维组 / 项目组 · 更新 2026-11-09

## 1. 打包与分发

| 场景 | 交付形态 | 说明 |
| :--- | :--- | :--- |
| 桌面 | 单守护进程二进制 + 嵌入静态 UI；可选 Tauri 外壳 | 目标安装包 ≤ 60 MB（[SRS NFR-12](../01-product/SRS.md)） |
| NAS / 服务器 | Docker 镜像 + 纯 Web UI（无需桌面进程） | 默认回环绑定，公网需反向代理 TLS |
| 平台 | Windows / macOS / Linux | 架构 x86_64 + aarch64 |
| 升级 | 守护进程自更新或包管理器 | 升级前自动备份 SQLite 索引库 |

进程模型（`coldrevived` / `coldrevive-agent` / 可关 UI）见 [ARCH §3](../02-architecture/ARCH.md)。

## 2. 默认配置与调优建议

- 默认仅监听回环、**默认共享目录为空**（用户显式指定后才开始发布）——安全优先。
- 机械硬盘用户建议 `torrent_gen_concurrency = 1`；SSD 可提到 4。
- 上行带宽小的用户把 `background_upload_ratio` 降到 10%，并开启 `prefer_utp`。
- 大规模共享（> 1 万文件）建议调大 `max_active_seeds` 前先做内存压测。

完整参数与校验规则见 [CONFIG](../04-protocol/CONFIG.md)。

## 3. 可观测性

### 3.1 日志

- 结构化日志：`tracing` + JSON；每个唤醒链路贯穿 `trace_id`，可跨「B → KAD → A → BT」全链路排查。
- 日志脱敏：`path` 只记相对路径，绝不记录共享目录外的绝对路径（[SRS NFR-09](../01-product/SRS.md)）。
- `wake_log` 滚动保留（默认 7 天），见 [DATA §3](../03-design/DATA.md)。

### 3.2 核心指标

指标端点 `/api/v1/metrics`，供 Prometheus 抓取。

| 指标 | 用途 |
| :--- | :--- |
| `active_seeds` / `cold_files` | LRU 活跃池与影子索引规模 |
| `kad_publish_total` / `kad_publish_fail` | 索引发布健康度（R-02 预警） |
| `wake_latency_p95` / `wake_success_rate` | 核心 KPI |
| `hash_mismatch_total` | 投毒与污染预警（R-08） |
| `rss_bytes` | 内存越限预警（R-06） |
| `background_upload_bps` | QoS 合规 |
| `torrent_gen_queue_len` | 造种积压 |
| `nat_type` | 连通性分布 |
| `handshake_wakeup_total` | 被动唤醒触发次数 |

### 3.3 告警条件

- RSS 越软限持续 5 min；
- 唤醒失败率突增；
- KAD 发布连续失败（> 20%）；
- 造种队列积压 > 阈值；
- 出现任何共享目录越界发布（R-14，立即阻断发布）。

### 3.4 内置自检

提供「网络自检」页面：`nat_type`、KAD 连通性、发布成功率、最近唤醒结果，降低用户排障门槛。

## 4. 升级与数据迁移

- `schema_version` 顺序迁移，向下兼容至少 2 个版本；失败则拒绝启动并保留原库。
- 索引库始终可从共享目录全量重建；重建过程低优先级、可中断。
- 协议 `version` 不兼容时，新旧命名空间并存一个 TTL 周期后下线旧版（[PROTO §4](../04-protocol/PROTO.md)）。
- 详见 [DATA §4](../03-design/DATA.md)。

## 5. 社区与冷启动运营

冷启动是头号风险 [R-01](../05-quality/RISK.md)，运营动作与产品设计同等重要：

1. 先开源 `coldrevive-cli`，用「死种复活」的**可复现 Demo** 吸引 PT/BT 社区极客。
2. 提供私有团体部署指南（小范围熟人网络是讨论中确认最易成功的场景）。
3. 公开可验证的复活成功率仪表盘，用数据而非口号建立信任。
4. 不与上游客户端争夺协议合入，以「完全兼容」定位共存，避免 hMule 式生态冲突。
5. 渐进式开放：M1 局域网 / 私有团体 → M3 邀请制公测 → M4 公开网络，每阶段记录复活成功率与节点留存。

设计层面的破局机制见 [LLD §13](../03-design/LLD.md)。

## 6. 合规与伦理边界

> **本节是项目红线，不可通过配置或插件绕过。**

- 本软件是**通用 P2P 传输与索引协议的技术实现**，性质与 eMule、qBittorrent、aria2 等开源下载器一致；其本身不提供、不索引、不推荐任何具体内容。
- **不内置**任何资源站点、种子索引库、默认 bootstrap 资源列表或搜索源；用户必须自行显式指定共享目录与 KAD 引导方式。
- **不提供**绕过版权保护措施、规避地域限制、隐藏用户身份或逃避监管的功能；不做匿名化承诺。
- 用户对其分享与下载内容的合法性**独立承担全部责任**；首次启动须展示并确认使用条款。
- 提供便捷的资源**举报与本地黑名单**机制，并保留按法规要求下架/禁用索引的能力。
- 默认配置遵循「最小暴露」：仅回环监听、共享目录为空、RPC 需密钥、CORS 白名单为空。
- 发布前完成法务评审（M0 Spike **S8**）；不同司法辖区可按需裁剪默认发布策略，见 [OPEN-QUESTIONS Q8](../06-plan/OPEN-QUESTIONS.md)。

## 相关文档

- [DOC-03 ARCH 进程视图](../02-architecture/ARCH.md)
- [DOC-08 DATA 迁移策略](../03-design/DATA.md)
- [DOC-10 API 鉴权与指标端点](../04-protocol/API.md)
- [DOC-11 CONFIG 配置规范](../04-protocol/CONFIG.md)
- [DOC-13 RISK R-01 / R-11](../05-quality/RISK.md)
- [DOC-14 ROADMAP M4 门禁](../06-plan/ROADMAP.md)
