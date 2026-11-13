---
doc_id: ADR-002
title: BT 引擎选 rqbit 还是 libtorrent FFI
category: 架构决策
version: 1.3
status: 已接受
owner: 内核组
updated: 2026-11-11
---

# ADR-002：BT 引擎选 `rqbit` 还是 `libtorrent` FFI

| 项 | 值 |
| :--- | :--- |
| 状态 | **已接受**（2026-11-11 依 S1 双 peer 4/4 `verified` 证据回填 `S1.status=pass` 并定稿；自实现握手拦截层实现 C1，选项 A 维持） |
| 决策者 | 内核组 / 架构组 |
| 相关 | [TECH-STACK S1](../TECH-STACK.md) · [RISK R-05](../../05-quality/RISK.md) · [ARCH §4 M-11](../ARCH.md) |

## 背景

ColdRevive 的冷热调度依赖三个能力：**按需实例化做种上下文、Pause/Resume、在握手阶段拦截并解析 InfoHash**。如果所选引擎不暴露这些接口，[LLD §5.6/5.7](../../03-design/LLD.md) 的核心机制（LRU 活跃池、被动唤醒）将无法实现，整个架构需要重做。

`rqbit` 是纯 Rust 实现，与项目技术栈一致；`libtorrent` 成熟稳定但需 FFI，带来跨语言内存安全与打包复杂度。

## S1 Demo 观测（2026-11-09）

[tests/spikes/s1/](../../../tests/spikes/s1/README.md) 用 librqbit 9.1.0 在 `127.0.0.1` 跑通了
68 字节 BT Handshake 拦截 → 按 InfoHash 动态加载伴随 `.torrent` → `paused=true` 实例化 →
`unpause / pause / unpause` → 16 片逐片 SHA-1 校验。C1/C2 `verified`，C3/C4 `partial`
（样本文件预先存在、未建立双 peer 真实分片传输）。

**关键发现**：rqbit 9.1.0 的 `SessionOptions` **没有握手回调字段**，
`session.rs::task_listener` 在 `read_handshake()` 后直接按 InfoHash 查已管理 torrent，
不暴露拦截点。也就是说「引擎是否暴露握手钩子」这个原始判据对 rqbit 恒为否，
但它**不等于**「ColdRevive 无法实现按需唤醒」——本次 Demo 的拦截层就是自实现的，C1 已 `verified`。

因此本 ADR 的选型门禁修正为：评估**自实现拦截层 / 本地代理层的可行性与成本**，
而不是等引擎提供回调。若 `rqbit` 的 Torrent 生命周期与 Pause/Resume 语义满足 LRU 活跃池需求，
选项 A 仍成立，握手拦截作为 `cr-engine` 外围组件落地；若生命周期不满足，
再按 `libtorrent` FFI 或精简做种层降级。S1 状态仍为 `not_run`，此处只登记观测、不出选型结论。

## S1 判据补齐（2026-11-11）

`tests/spikes/s1/` 新增 `cargo run -- two-peer`，把上面两处 `partial` 补齐。依据 librqbit 9.1.0
源码核实的 `SessionOptions.listen` / `ConnectionOptions` / `AddTorrentOptions.initial_peers` /
`AddTorrentOptions.ratelimits` / `ManagedTorrent::wait_until_completed`，建立**两个独立 Session**：
seeder 预置 1 MiB 完整样本、仅 `127.0.0.1` 入站 TCP 监听、上传限速 256 KiB/s；
leecher 空目录、`paused: true` 实例化、`initial_peers = [seeder_addr]`、仅主动出站；
DHT / tracker / LSD 全部禁用，无公网报文。

| 观测 | 结果（连续 4 轮一致） |
| :--- | :--- |
| leecher 起点 | `progress_bytes = 0`、`is_paused = true` |
| 传输中途 | 30 ms 时 `progress_bytes = 262144 / 1048576`（16 片，分片整数倍） |
| 中途 `pause` 后 | `is_paused = true`，停留 900 ms 位图仍为 262144，**不前进、不回退** |
| `unpause` 跑完 | `finished = true`、`progress_bytes = 1048576` |
| 双端计数器 | seeder `uploaded_bytes = 1048576`；leecher 恢复后 `fetched_bytes = 786432`（48 片） |
| 落盘校验 | 64/64 片 SHA-1 匹配、0 mismatch、内容逐字节等于确定性样本 |

**结论**：S1-C3（传输中途暂停/恢复不丢位图）与 S1-C4（真实双 peer 分片传输 + 逐片校验）
由 `partial` 升为 `verified`；连同 [§S1 Demo 观测](#s1-demo-观测2026-11-09) 的 C1/C2，
**S1 四条判据已全部有 `verified` 证据**，本报告与上文「S1 Demo 观测」的 `s1-probe-*.json` 需合起来读。
报告：`tests/spikes/s1/reports/s1-two-peer-*.json`，证据登记在 `spikes.json` 的
`S1.observations.two_peer`；细节与边界见 [S1 README §4–§6](../../../tests/spikes/s1/README.md)。

**证据边界**：同一主机两个 librqbit Session，无第三方 BT 客户端、无跨主机、无 NAT、无公网；
限速回环不构成性能证据。C1 的握手拦截仍是自实现层，**不是引擎暴露的回调**，选型论证不变。

**治理状态**：`S1.status` 已于 2026-11-11 由实验执行人依 `two-peer` 4/4 `verified` 证据手工回填为
`pass`。本 ADR 据此依 [ADR 索引 §4](README.md)「已接受的 ADR 不修改正文」原则定稿为「已接受」，
结论为**选项 A：采用 `rqbit`**；握手拦截由自实现层（`cr-engine` 外围）落地，不要求引擎暴露回调。
后续每里程碑复审 `rqbit` 上游维护状态（见 [RISK R-12](../../05-quality/RISK.md)）。

## 选项

| 选项 | 优点 | 缺点 |
| :--- | :--- | :--- |
| A. `rqbit` | 纯 Rust、无 FFI、与 Tokio 生态一致、易于定制 | 维护状态与高级能力需核实，可能缺 Pause/Resume 或握手钩子 |
| B. `libtorrent` FFI | 功能完备、久经考验、NAT/DHT 能力齐全 | FFI 边界复杂、二进制体积上升、定制握手拦截仍可能需补丁 |
| C. 自研精简做种层 | 完全可控、只实现所需子集 | 工作量最大，协议兼容性风险高 |

## 决策

**选项 A（已采纳）**：以 **S1 Spike 为硬门禁**已通过 —— 最小 Demo 验证了「拦截 Handshake → 动态加载种子 → 正常完成标准下载」全链路，其中握手拦截由自实现层实现（不要求引擎暴露回调）。

- S1 通过 → 采用 `rqbit`，按 [ARCH §4](../ARCH.md) 落地 M-11，拦截层放在 `cr-engine` 外围。
- S1 不通过 → 切换 `libtorrent` FFI；仍不通过则评估选项 C 的精简做种层。
- **2026-11-11 已完成**：双 peer 真实分片传输与传输中途暂停恢复（S1-C3/C4 由 `partial` 升为
  `verified`，见「S1 判据补齐」）；`S1.status` 已由实验执行人依证据回填为 `pass`，本 ADR 据此
  定稿为「已接受」，选项 A 采纳。后续复审见 [RISK R-12](../../05-quality/RISK.md)。

**无论选择哪个**，业务 crate 一律只依赖 `cr-engine` 的 `Engine` Trait，禁止直接引用具体库（见 [TECH-STACK §5](../TECH-STACK.md)）。

## 后果

- 正面：选型失败不会推翻上层设计，切换成本被限制在 `cr-engine` 内部。
- 负面：Trait 抽象会牺牲部分引擎特有优化；需为 Pause/Resume 语义写契约测试。
- 风险：若 `rqbit` 停更，按 [RISK R-12](../../05-quality/RISK.md) 处理。

## 复核时点

本 ADR 已于 2026-11-11 依 S1 4/4 `verified` 证据定稿为「已接受」；此后每里程碑复审 `rqbit`
上游维护状态（[RISK R-12](../../05-quality/RISK.md)）。若 `rqbit` 停更或缺失关键能力，按
[RISK R-12] 处理并重新评估选项 B / C。

## 相关文档

[ADR 索引](README.md) · [LLD §5.8](../../03-design/LLD.md) · [TEST 兼容性矩阵](../../05-quality/TEST.md)
