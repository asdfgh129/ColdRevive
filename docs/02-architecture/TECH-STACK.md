---
doc_id: DOC-04
title: 技术选型与 M0 Spike 清单
category: 架构
version: 2.6
status: 待评审
owner: 架构组 / 内核组
updated: 2026-11-13
---

# 技术选型与 M0 Spike 清单

> 编号 DOC-04 · 版本 2.6 · 状态 待评审 · 责任人 架构组 / 内核组 · 更新 2026-11-13
> 本文记录技术栈选择、备选方案、**必须由 M0 验证的假设**。选型变更须新增 [ADR](adr/README.md)。

## 1. 选型总表

| 领域 | 首选 | 备选 | 理由 / 风险 |
| :--- | :--- | :--- | :--- |
| 语言 / 运行时 | **Rust + Tokio** | Go | 高并发 UDP/TCP、底层哈希、内存安全；避免 GC 抖动影响 100 MB 目标 |
| 构建 | Cargo Workspace，按 M-01～M-16 分 crate | 单 crate | 模块边界清晰，便于并行开发与单测 |
| 本地存储 | **SQLite (WAL, rusqlite)** | Sled | 事务、迁移、5 万行轻松；运维可查 |
| 文件监控 | `notify` crate | 平台原生 API | 跨平台 inotify / FSEvents |
| BT 引擎 | `rqbit`（M0 验证） | `libtorrent` FFI | 纯 Rust 优先；若不支持 Pause/Resume 或握手拦截则切 FFI |
| eD2K 哈希 / 传输栈 | **仓内可审计的自实现**（`tests/spikes/s2/{kad-mini,ed2k-mini}`，G0 后迁入 `cr-ed2k`） | `ed2k-rs` / `ed2k` **仅作哈希参考**，不作依赖 | V-04 已确认两个 crate 均为 hash-only、无传输栈且停更；`ed2k` 默认 Blue 与 eMule Red 分块边界不兼容（死条目）。[ADR-011](adr/ADR-011-ed2k-interop.md) 已把标准 eD2k 栈提前至 v1；AICH 可不做 |
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

> **当前状态（2026-11-10）**：**S2 = `pass`** —— 公共 KAD 的第三方节点接受并返回了我们的命名空间条目
> （`PUBLISH_RES load=1` → `SEARCH_RES` 命中，5 个 tag 原样返回）；随后以两个独立进程补做 A/B
> （`split-a` 发布 / `split-b` 不同 socket、KadID 种子与 bootstrap 入口独立取回），返回的 descriptor
> 携带 A 的源端口，确认跨节点可取回。证据：`tests/spikes/spikes.json` 的
> `observations.p4_execution_v2427` 与 `ab_two_process_2026_11_10`，过程见 [PLAN §20–§21](../../tests/spikes/s2/PLAN.md)。
> 该结论只覆盖「接纳性」；「按运营速率的污染判定」（R-02）仍需 M4 前按实测拒绝率复审。
> S8 由所有者豁免登记为 `pass`（无书面法务意见，R-11 保留）；**S1 四条判据已于 2026-11-11 全部 `verified`，`S1.status` 已回填为 `pass`，[ADR-002](adr/ADR-002-bt-engine.md) 据此定稿为「已接受」；S4 / S7 已于同日经真实量测回填 `pass`，S3 / S5 / S6 保持 `not_run`（见下）。G0 仍须 S3/S5/S6 结论与 ADR-001 所有者签署**。
>
> **S1 判据补齐（2026-11-11，不改变 `S1.status`）**：`tests/spikes/s1/` 新增
> `cargo run -- two-peer`，用两个独立 librqbit 9.1.0 Session 在 `127.0.0.1` 跑标准 BT Peer Wire：
> leecher 从空目录起，传输到 262144/1048576 时 `pause` 900 ms 位图不变，`unpause` 后跑完，
> 64/64 片 SHA-1 匹配、内容逐字节一致；连续 4 轮一致，C3/C4 由 `partial` 升为 `verified`。
> 加上历史 `probe` 报告的 C1/C2，**S1 判据 4/4 齐备**。报告
> `tests/spikes/s1/reports/s1-two-peer-*.json`，证据在 `spikes.json` 的 `S1.observations.two_peer`，
> 细节见 [S1 README](../../tests/spikes/s1/README.md) 与 [ADR-002 §S1 判据补齐](adr/ADR-002-bt-engine.md)。
> 边界：同主机双 Session、无第三方 BT 客户端 / 跨主机 / NAT / 公网，不构成 G3 兼容性或性能证据。
> `crate.rqbit` 阻塞项已解除（`S1.blocked_by` 置空）；[ADR-002](adr/ADR-002-bt-engine.md)
> 已于 2026-11-11 依 S1 4/4 `verified` 回填 `S1.status=pass` 并定稿为「已接受」（选项 A：采用 `rqbit`，握手拦截自实现）。
>
> **ADR-011 L1 / L2 已于 2026-11-11 落地（不改变上面的 Spike 状态）**：L1 用 `MD4` 无前缀关键词
> 在真实 aMule 上完成字典互通；L2 补齐 `0x44` / `0x34` 源字典，并在 `tests/spikes/s2/ed2k-mini`
> 实现最小标准 eD2k TCP 下载/上传栈 + 最小上传队列。回归：`kad-mini` 21/21 + 48/48 + 5/5，
> `ed2k-mini` 31/31 + 26/26 + 3/3 回环闭环，真实 eMule `HELLO` 握手成功。
> **全部为 RFC1918 / 127.0.0.1 证据、0 个公网报文，不判 pass、不回填 `spikes.json`**；
> 按 G0 冻结未创建生产 crate。详见 [PLAN §22–§23](../../tests/spikes/s2/PLAN.md) 与
> [PROTO §2.2](../04-protocol/PROTO.md)。

> **S3 已启动（2026-11-11，v2.4.34）**：`tests/spikes/s3/` 新增零依赖双栈探针，本地双 HashMap
> （KAD 16B + BT 20B）在顺序 + 并发读写下**串扰/泄漏 = 0**（S3-C1 verified），双栈索引估算
> 10k/栈 ≈2.26 MiB、50k/栈 ≈11.08 MiB，均远低于 NFR-05 的 100 MB（S3-C2 verified 结构性上界）；
> 但属本地数据结构证据，不做真实 DHT 网络加入，`S3.status` 保持 `not_run`，最终 pass 须 M1 真实双栈集成 + G3 矩阵。

> **S1 第三方客户端互操作轮已执行（2026-11-11，G3 素材，不改变 `S1.status`）**：按用户指令以本机
> **Motrix Next**（后更名为 **Rayburst**，捆绑 CLI 由 `motrixnext-aria2c.exe` 改为 `aria2-next.exe`；本轮 S1 实测时为 aria2 1.36.0，现升级为 aria2-next 3.1.0）作第三方 seeder：
> 其对确定性 `payload.bin/payload.torrent`（128 片 × 16 KiB，infohash `2b77301a…`）做种并监听
> `127.0.0.1:16881`，`cargo run -- cross-lee --peer 127.0.0.1:16881` 由 rqbit 直连拉取，
> **128/128 片全部 SHA-1 校验通过**（`all_pieces_verified=true`，2 MiB 全量）。
> 排错记录：首轮 aria2 因旧 `.aria2` 控制文件 + `--check-integrity=false` 处于 Leecher 态（0 字节），
> 修复后以 `--check-integrity=true` 进入 Seeder 态。证据 `tests/spikes/s1/reports/s1-cross-lee-*.json`，
> 登记 `spikes.json` 的 `S1.observations.cross_client_round`（operator attest 已依用户指令满足）。
> **S1 维持 `pass`**，本结果仅作 G3 兼容性矩阵素材。
>
> **S4–S7 推进（2026-11-11）**：
> - **S4 `pass`**：`tests/spikes/s4/` 以真实 Rust `HashMap<[u8;20], ShadowRow>` 模型（COLD 轻量 handle，
>   不含 Bitfield / 逐片哈希，符合 DATA §5）+ Windows 工作集采样：1 万文件 ≈7.79 MiB（含 SQLite 缓存）、
>   5 万 ≈15.9 MiB（同时满足 NFR-05 ≥5 万容量）、20 万 ≈53 MiB，均 ≪ 100 MB，R-06 去险，
>   fallback（索引分库 / 惰性加载）未触发。
> - **S7 `pass`**：`tests/spikes/s7/s7_bench.py` 流式哈希 4 GB（262144 片 SHA-1 + 全文件 SHA-256 +
>   `.torrent` 序列化）耗时 **4.14 s / 989 MB/s**、峰值 RSS **29.3 MiB**、Idle 优先级，R-07 去险，
>   fallback（分片自适应 + 增量哈希）未触发。
> - **S5 `not_run`（分析就绪 + 真实栈量测）**：NFR-03 双层控制——令牌桶硬上限 20% 上行 +
>   uTP 低优先级退让；但**2026-11-13 源码核实 `librqbit-utp 0.7.0` 只实现 CUBIC、无 LEDBAT**
>   （`// TODO: LEDBAT congestion control`），`prefer_utp` 不再构成退让手段。
>   真实 uTP 协议栈量测（16 Mbit/s 共享瓶颈、3 轮中位）：ratio 0.20 → 14.82%、0.05 → 4.31%、
>   0.02 → 2.70% 前台劣化，且与后台链路占比近似 1:1（无退让）。「前台劣化 ≤5%」仍须
>   `tc/netem` / 跨主机真实拥塞链路实测，M1/G3 回填。见 `tests/spikes/s5/README.md` §9。
> - **S6 `not_run`（分析就绪）**：穿透栈（UPnP + NAT-PMP + UDP 打洞 + 中继/LowID 回退）覆盖全 4 类 NAT，
>   分析型含中继可达率 ≥95%（> R-04 的 70% 阈值）；量化判据须 M2 真实多 NAT 实测（OPEN-QUESTIONS Q4）。
>   见 `tests/spikes/s6/nat-matrix.md`。

> **G0 条件冻结与 2026-11-13 补充证据**：项目所有者已签署 [ADR-001](adr/ADR-001-kad-network.md)
> 默认模式 `public` 并批准 **G0 条件冻结**（S3/S5/S6 豁免 G0 时点结论，状态仍 `not_run`，M1/M2 必测回填），
> 详见 [G0-REVIEW](../06-plan/G0-REVIEW.md)。本轮新增三项真实量测：
> - **S3**：真实 UDP 双栈 I/O（`tests/spikes/s3/dualstack_io.py`），1437/1437 跨命名空间畸形帧拒绝、
>   0 误路由 / 0 丢包、总带宽 3.28 Mbit/s ≈ 两栈之和、RSS Δ0.93 MiB；C2 带宽由估算升级为实测。
> - **S5**：`tests/spikes/s5/qos_bench.py` 证实令牌桶硬上限成立（后台 ≤20.25% 链路），
>   但**默认 20% 配置前台劣化 20.12%**、退让模型 15.0%，ratio=0.02 才降到 4.03%——
>   **NFR-03 ≤5% 在默认配置下未达成，M1 默认值须按实测回调，R-07 未完全去险**。
> - **S6**：`tests/spikes/s6/stun_probe.py` 真实先导样本 `112.5.60.4:36764`（cone 类、公网 UDP 可达），
>   **UPnP IGD / NAT-PMP 在本环境均不可用**，R-04 不能按「UPnP 可用」假设估计。
> 工程门禁：第三方 BT 客户端检查已按 S1 互通证据修正为 `OK`，preflight 阻塞项 3 → 2（均为 M1 接线）。

> **M1 开工补记（2026-11-13，v2.5）**：依条件冻结建立根 `Cargo.toml` + 17 crate + 3 bin，
> **M1 实现刻意保持 std-only 零第三方依赖**（`cargo test --workspace --offline` 27 单测全绿），
> 目的是让 G1 前的工程接线不受 crates.io 可达性影响。§1 选型表中的 Tokio / rusqlite / notify /
> rqbit / axum 均**尚未引入生产代码**，仍按各自里程碑落地：M2 引入 `rqbit`（ADR-002 选项 A）、
> `rusqlite`、Tokio/axum 时须另立 ADR 并复核 G1。`coldrevived` 当前的最小 HTTP / UDP / BT 种子端
> 是 M1 验证用实现，不是最终架构。T0 `--mode real` 已 21/21 PASS，但 T1 与 S3/S5/S6 必测回填
> 未完成，**G1 未通过、M2 仍 `blocked_by_G1`**，见 [G0-REVIEW §9](../06-plan/G0-REVIEW.md)。
> 〔以上为 2026-11-13 时点记录；**当前状态更正（2026-11-14）**：G1 已通过、M2 门禁解除并已开工。
> 本轮 M2/W3.3 的 cr-nat 接入**仍坚持 std-only 零第三方依赖**（`cargo test --workspace --offline`
> 74 单测全绿），选型表中的 rqbit / rusqlite / Tokio / axum **依然未引入生产代码**，
> 引入时仍须另立 ADR 并复核门禁。T2 本地预检 10/10 只证明接线，S3/S5/S6 仍 `not_run`。
> 详见 [G0-REVIEW §11](../06-plan/G0-REVIEW.md) 与 [ROADMAP §6](../06-plan/ROADMAP.md)。〕

> **T1 / S5 补记（2026-11-13，v2.6）**：M1 增加按节点 `host` 的跨主机寻址（`NodeSpec.host`
> 缺省回落 `bind`）、`--self-host` / `--workdir` / `--sample` / `--downloader` 启动参数与
> `/health` 机器身份，`tests/e2e/t1/` 建立 10 用例 / 40 断言的双机验收 harness。
> 单机预检两模式各 10 PASS / 1 SKIP、`evidence_valid=false`；`--mode real` 因**只有一台物理主机**
> 明确 `BLOCKED`。环境实测：本机跨 IP UDP 仅 `6/20` 可达（全部是回环别名之间），
> 物理网卡 ↔ Hyper-V 双向 `WSAENETUNREACH`。**新的技术缺口**：M1 无 UPnP / NAT-PMP / 打洞 / 中继，
> 跨 NAT 双机的唤醒与私网 peer 地址不可达，G1 前须补穿透栈；S5 的 LEDBAT 假设已否证（见上）。
> 因此 G1 与 M2 结论不变，详见 [G0-REVIEW §10](../06-plan/G0-REVIEW.md) 与
> [T1 README](../../tests/e2e/t1/README.md)。

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
