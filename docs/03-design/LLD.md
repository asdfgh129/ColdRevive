---
doc_id: DOC-06
title: 模块详细设计（LLD）
category: 设计
version: 2.0
status: 待评审
owner: 内核组
updated: 2026-11-09
---

# 模块详细设计（LLD）

> 编号 DOC-06 · 版本 2.0 · 状态 待评审 · 责任人 内核组 · 更新 2026-11-09
> 模块编号 `M-xx` 与 [ARCH §4](../02-architecture/ARCH.md)、[src crate 名](../../src/README.md) 一致。流程见 [FLOWS](FLOWS.md)，数据结构见 [DATA](DATA.md)。

## 1. M-01 共享目录监控器

- **输入**：用户显式配置的共享目录白名单（默认 `~/ColdRevive/Shared`）；**绝不扫描目录外文件**（[SRS NFR-09](../01-product/SRS.md)）。
- **事件**：`Create` / `Modify` / `Delete` / `Rename`；跨平台用 `notify` crate（inotify / FSEvents / ReadDirectoryChangesW）。
- **去抖**：新增文件在「写入完成」（连续 N 秒无 Modify 事件，默认 3 s）后才入队，避免对未完成的大文件造种。
- **变更处理**：修改/重命名 → 标记旧记录 `STALE` → 重算指纹 → 撤销旧 KAD 发布 → 发布新 Key。
- **排除规则**：隐藏文件、`.part`/`.crdownload` 临时文件、符号链接逃逸、小于阈值的文件（默认 1 MB）可配置跳过。
- **接口**：`IndexEvent { path, kind, size_hint }` 通过 mpsc 送入 M-02 队列。

## 2. M-02 / M-03 指纹与伴随种子生成

这是把「eMule 式文件共享」落到 BT 体系的**关键补丁**：BT 以种子为单位，只有文件本身无法下载——下载方不知道分片大小与每片 SHA-1。因此**每个共享文件都必须配一个伴随种子**（[ADR-009](../02-architecture/adr/ADR-009-sharing-model.md)）。

| 产物 | 算法 | 用途 |
| :--- | :--- | :--- |
| `file_hash` | SHA-256 | ColdRevive 主键、去重、唤醒请求标识 |
| `ed2k_hash` | MD4 根哈希（9.28 MB 分块） | 兼容 eMule / KAD 文件索引字典 |
| `bt_piece_hashes` | SHA-1 分片（分片大小按文件体积自适应，默认 512 KB～4 MB） | 生成 `.torrent`、逐片校验 |
| `infohash` | BT v1 InfoHash（v2 可选） | Mainline DHT 发布、magnet、握手匹配 |

- **调度**：独立低优先级队列，并发数受 `torrent_gen_concurrency`（默认 2）与 CPU Idle 检测约束；进度可中断可续算。
- **缓存**：`.torrent` 落盘到元数据目录，路径与 `infohash` 绑定；索引库只存引用，避免 DB 膨胀。
- **幂等**：同一 `file_hash + piece_size` 重复入库直接复用已有种子。
- **跨协议预留**：`ed2k_hash` 在 v1 即计算并入库，为 P2 的跨协议补源留出接口（[ADR-004](../02-architecture/adr/ADR-004-ed2k-scope.md)）。

## 3. M-04 影子索引库

- **定义**：文件休眠后，索引库仍保留其 `infohash / file_hash / 路径 / 大小 / 最后活跃时间 / 状态`，且 KAD 与 Mainline DHT 的通告**持续续期**。全世界依然认为本节点是在线做种者。
- **存储**：SQLite（WAL 模式），单表 5 万行无压力；索引字段建立唯一约束。
- **原则**：**索引与做种上下文生命周期解耦**。销毁 `TorrentContext` 不影响索引行。
- **Schema** 见 [DATA](DATA.md)。

## 4. M-05 / M-06 双 DHT 适配层

分层利用，不取代 Tracker（[ADR-007](../02-architecture/adr/ADR-007-tracker.md)）。

| 层 | 网络 | 作用 | 内容 |
| :--- | :--- | :--- | :--- |
| 发现层 | **eMule KAD**（128-bit ID） | 关键词/文件哈希 → 在线持有者 IP | `ColdRevive_V1:` 专属命名空间索引 |
| 激活层 | **私有唤醒 RPC** | 触发对方从冷存储加载并临时做种 | UDP `WAKEUP_*` |
| 兼容层 | **Mainline DHT**（160-bit ID, BEP-0005） | 让标准 BT 客户端能找到被激活的节点 | 按 InfoHash 发布 Peer |
| 加速层 | Tracker（**完全可选**） | 提升热门资源发现速度 | 标准 announce |

- **ID 空间映射**：KAD 与 Mainline DHT 不能直接互相写入。适配层维护两张独立路由表与发布队列，以 `file_hash` / `infohash` 作为关联键，**不做跨网络 Key 转换**。
- **命名空间隔离**：`Target_Key = SHA1("ColdRevive_V1:" + File_Hash)`；关键词同理加前缀。避免污染公共 KAD，也避免与普通 eMule 结果混淆。
- **发布策略**：启动时全量发布（分批限速），之后按 TTL（默认 30 min）增量续期；仅发布 **HOT + COLD 全部共享文件**，即影子索引。
- **查询策略**：多 Key 并查、超时分级、结果上限（默认 200）、按信誉与在线概率排序。
- **不信任原则**：DHT 返回一律视为不可信候选，必须通过唤醒响应 + 元数据哈希校验才采信。

## 5. M-08 唤醒信令服务

- **传输**：UDP 单包请求/响应（轻量、无连接、便于打洞）；大响应（`.torrent` 本体）改走 HTTP 接口，UDP 只回 URL 与摘要。
- **报文**：Protobuf 或紧凑 JSON（`cr-wake` 内部定版，见 [PROTO](../04-protocol/PROTO.md)），带 `version` 字段便于协议演进。
- **鉴权（Challenge-Response）**：`challenge = HMAC(node_secret_seed, file_hash || utc_date || nonce)`；接收方校验时间窗（±5 min）与 nonce 去重，防止重放与「任意节点通知我做种」被滥用为 DDoS / 资源探测。
- **限流**：按 IP 令牌桶（默认 5 次/分钟）、全局并发唤醒上限（默认 8）、单文件并发上限（默认 3）。
- **隐私**：响应只暴露用户显式共享目录下的文件；不在白名单内的 `file_hash` 一律返回 `REJECT_NOT_FOUND`（避免成为文件存在性探测接口）。
- **超时**：请求方 1.5 s 超时重试一次，随后切换候选节点；避免陷入 TCP 长超时。

## 6. M-09 冷热分级与 LRU 活跃做种池

**资源池化 + 影子索引**是「1 万文件 / 100 MB」目标的核心，也是 [ADR-009](../02-architecture/adr/ADR-009-sharing-model.md) 的实现层。

| 状态 | 内存占用 | 网络连接 | 是否可被搜到 | 是否可下载 |
| :--- | :--- | :--- | :--- | :--- |
| HOT | 完整上下文（连接、缓存、Bitfield） | 保持 | 是 | 立即 |
| SERVING | 完整上下文 + 传输缓冲 | 活跃 | 是 | 立即 |
| COLD | **≈ 仅一条索引行** | 无 | **是（影子索引）** | 唤醒后（毫秒～秒级） |

- **硬上限**：`max_active_seeds = 50`（默认）。池满触发 LRU 淘汰最久未活跃者。
- **切入**：由 `WAKEUP_REQ`（新客户端）或 `Handshake InfoHash`（任意客户端）触发，从磁盘加载元数据、实例化 `TorrentContext`、注册到 M-10。
- **切出**：`idle_sleep_minutes = 10` 无连接/无数据，或 LRU 淘汰，或内存/带宽越限强制切出。
- **会话硬顶**：`wake_session_ttl = 2 h`，到期强制回冷，防止异常长连接。
- **预热**：刚下载完成 / 最近 1 小时被请求过的文件保留在 HOT，减少抖动。
- **抖动抑制**：切入/切出加滞回（hysteresis），避免频繁唤醒-休眠震荡。

## 7. M-10 接入层握手拦截器

- 一个**始终在线、无状态、开销极低**的 Reactor，仅解析 BT Handshake 前 68 字节提取 InfoHash。
- 命中影子索引 → 触发 M-09 切入 → 补全握手 → 移交标准做种上下文。
- 未命中 → 按未知种子处理（拒绝或转发给普通引擎），不分配资源。
- 该组件是「旧客户端零改造兼容」的技术支点，也是 [PRD P2](../01-product/PRD.md) 的实现处。

## 8. M-11 / M-12 数据面引擎

- **BT 引擎**：标准 Peer Wire、magnet、DHT、PEX、uTP；**必须支持 Pause/Resume 与按需实例化**（M0 Spike S1 重点验证项，见 [ADR-002](../02-architecture/adr/ADR-002-bt-engine.md)）。
- **分片校验**：逐片 SHA-1 校验，损坏片丢弃并重取；完成后全文件复核。
- **跨协议补源（P2，暂缓）**：eD2K 用 MD4 / 固定 9.28 MB 分块，BT 用 SHA-1 / 动态分片，**边界完全不对齐**。需要「哈希转译层」：先取 eD2K 分块 → 本地重算 BT 分片 SHA-1 → 校验通过才拼接写入。该层计算与实现成本远高于协议封装，故明确排到 v2（[ADR-004](../02-architecture/adr/ADR-004-ed2k-scope.md)）。
- **旧客户端兼容保证**：对外只暴露标准端口、标准握手、标准种子；私有协议从不出现在数据面。

## 9. M-07 NAT 穿透与连通性

| 手段 | 优先级 | 说明 |
| :--- | :--- | :--- |
| UPnP / NAT-PMP / PCP | 1 | 启动时自动映射唤醒端口与 BT 端口，定时续租 |
| KAD / UDP 打洞 | 2 | 利用 DHT 节点协助双方打洞；参考 BEP-55 思路（M0 核实） |
| HighID / LowID 判定 | 3 | 启动时自检可达性，向 KAD 声明能力 |
| 中继回退 | 4 | 双向均不可达时，经在线 HighID 节点转发信令；数据面限速并明确提示 |
| IPv6 直连 | 5 | 若可用优先使用，规避 NAT |

> 服务的可用性上界由「在线节点比例 × NAT 可达率」决定，需在 M2 建立 NAT 矩阵测试并给出实测成功率。

## 10. M-13 资源治理与 QoS

所有默认值可配置（[CONFIG](../04-protocol/CONFIG.md)），运行时热更新优先。

| 参数 | 默认值 | 作用 |
| :--- | :--- | :--- |
| `max_active_seeds` | 50 | LRU 活跃做种池硬上限 |
| `memory_soft_limit_mb` / `memory_hard_limit_mb` | 100 / 200 | 越限触发强制切出与拒绝新唤醒 |
| `background_upload_ratio` | 20% of 上行 | 令牌桶后台做种限速 |
| `max_connections_total` | 200 | 全局连接数上限 |
| `max_wake_concurrency` | 8 | 并发唤醒处理上限 |
| `wake_rate_per_ip` | 5 / min | 单 IP 唤醒限流 |
| `idle_sleep_minutes` | 10 | 无活动自动休眠 |
| `wake_session_ttl` | 2 h | 唤醒会话硬顶 |
| `torrent_gen_concurrency` | 2 | 造种并发（Idle 优先级） |
| `max_keywords_per_file` | 8 | 控制 KAD 发布量 |
| `kad_publish_ttl` | 30 min | 索引续期周期 |
| `kad_publish_rate` | 20 / s | 发布限速，避免冲击公共网络 |

- **QoS**：后台做种走 uTP / LEDBAT 低优先级拥塞控制，前台有流量时自动退让；令牌桶保证上行不超阈。
- **降级链**：接近软限 → 降低后台限速 → 停止新唤醒 → 强制切出最冷项 → 暂停造种队列。

## 11. 安全、信誉与隐私

| 威胁 | 缓解 |
| :--- | :--- |
| 恶意唤醒洪泛（DDoS / 资源探测） | Challenge-Response、nonce 去重、按 IP 限流、全局并发上限、LRU 上限 |
| 索引污染 / 虚假文件 | 专属命名空间、结果去重、元数据与分片双重哈希校验、失败即拉黑 |
| 女巫攻击（伪造大量节点） | 结果交叉验证、信誉分、发布频率限制（P2） |
| 投毒 / 篡改分片 | 逐片 SHA-1 + 全文件终验 |
| 文件存在性探测 | 非白名单文件统一返回 `REJECT_NOT_FOUND` |
| RPC 未授权访问 | `rpc-secret` 强制、CORS 白名单、默认仅本机监听 |
| 路径/隐私泄露 | 日志脱敏、共享目录白名单、不外露绝对路径 |
| 协议演进攻击 | 报文带 `version`，不兼容版本直接拒绝 |

**明确接受的风险**（讨论中已决定「不管了」）：开放 KAD 网络固有的索引污染不完全消除，只做客户端侧过滤，不引入中心化治理。

## 12. M-14 / M-16 接入层与生态

| 能力 | 设计 |
| :--- | :--- |
| 内置 Web UI | 静态资源编译期嵌入二进制，同端口 `/` 提供；无需外部 Nginx / 静态文件服务器 |
| 状态更新 | **WebSocket 推送**，禁止 UI 高频轮询（[ADR-008](../02-architecture/adr/ADR-008-websocket.md)） |
| JSON-RPC | 实现 aria2 常用方法子集，带 `token:` 前缀鉴权，见 [API](../04-protocol/API.md) |
| 生态复用 | AriaNg、Aria2 Explorer 等现成前端与浏览器插件可直接接入，**零成本获得多端远程管理与下载捕获能力** |
| 远程管理 | 默认仅监听回环；用户显式开启局域网/公网并配置 secret 与 CORS 白名单 |
| CLI / REST / MCP | P2，覆盖自动化与 AI 智能体管理场景 |
| UI 功能 | 搜索框、结果列表与健康度、任务管理、共享目录管理、活跃池/内存/带宽监控、设置与日志 |

> 讨论中曾比较「外部静态 HTML 前端 / 一体化容器 / 深度改 aria2 C++ `HttpServer`」三条路线。ColdRevive 自建 Rust 守护进程，因此**天然获得「深度嵌入」的收益（单二进制、无外部依赖、无 CORS 问题）而无需承担 C++ 源码维护成本**；同时通过「UI 进程可独立关闭」保留前后端解耦的灵活性（[ADR-003](../02-architecture/adr/ADR-003-ui-delivery.md)）。

## 13. 激励与冷启动策略

hMule 的失败验证了三件事：**没人用 → 找不到资源 → 更没人用**；上游生态因「跨协议公平性」拒绝合入；学术原型缺乏持续工程投入。ColdRevive 的应对：

1. **先提供不依赖网络效应的单机价值**：即使全网只有一个节点，它也是一台「去中心化个人归档/自建网盘」——共享目录自动造种、可被自己其他设备检索、可作为私有小团体的资源库。这打破了冷启动死循环。
2. **借生态而非造生态**：复用公共 KAD 引导节点、兼容 Mainline DHT、兼容 aria2/AriaNg 插件生态，让 ColdRevive 在第一天就能「混进」现有网络。
3. **下载即贡献**：下载完成的文件自动进入共享目录并发布索引（[FLOWS F4](FLOWS.md)），用户规模与资源总量正相关。
4. **渐进式开放**：M1 局域网/私有团体 → M3 邀请制公测 → M4 公开网络；每阶段记录复活成功率与节点留存。
5. **不强行做协议推广**：不与 aMule/qBittorrent 上游争夺合入，避免 hMule 式的生态冲突；以「对旧客户端完全透明」换取共存。
6. **激励后置**：贡献积分/全局优先级列为 P2，且必须先解决抗女巫与信用伪造，否则不上线（[ADR-005](../02-architecture/adr/ADR-005-incentive.md)）。

## 相关文档

- [DOC-03 ARCH 模块清单](../02-architecture/ARCH.md)
- [DOC-07 FLOWS 端到端流程](FLOWS.md)
- [DOC-08 DATA 数据模型](DATA.md)
- [DOC-09 PROTO 协议规范](../04-protocol/PROTO.md)
- [DOC-11 CONFIG 配置规范](../04-protocol/CONFIG.md)
