---
doc_id: ADR-015
title: 取消「全局零依赖」，改为按层划分的依赖策略
category: 架构决策
version: 1.0
status: 已接受
owner: 项目所有者
updated: 2026-11-16
---

# ADR-015：取消「全局零依赖」，改为按层划分的依赖策略

| 项 | 值 |
| :--- | :--- |
| 状态 | **已接受**（2026-11-16 项目所有者：「取消零依赖，按既定方案走」） |
| 决策者 | 项目所有者 / 架构组（记录） |
| 相关 | [ADR-002 选 rqbit](ADR-002-bt-engine.md) · [ADR-010 Rust + Tokio](ADR-010-rust-tokio.md) · [TECH-STACK §5](../TECH-STACK.md) · [ROADMAP W5.1](../../06-plan/ROADMAP.md) |

## 背景

「零第三方依赖」不是架构目标，是 **M1 的临时脚手架**。2026-11-13（v2.4.38）建根
`Cargo.toml` 时，G1 还没过、crates.io 可达性不明，为了让「工程接线本身」不被网络问题拖住，
当时把全部 crate 写成 std-only，并在 `Cargo.toml` 里留了退出条件：

> M2 引入 rqbit / tokio / axum / rusqlite 时须另立 ADR 并复核 G1 门禁。

现在的状态：

1. **G1 已过**（T0 21/21 + T1 11/11，`evidence_valid=true`），退出条件的前半段已满足。
2. **主线卡住的原因就是没有数据面**：`coldrevived` 的 `/jsonrpc addUri` 只把 `.torrent`
   取回来解析，任务永远停在 `waiting`；补种（做种给旧客户端）无从谈起。
3. **ADR-002 早已选定 librqbit**，`tests/spikes/s1/` 用 librqbit 9.1.0 跑通双 peer 真实分片传输，
   `Cargo.lock` 依赖树在本地缓存里是完整的（`--offline` 可构建）。
4. **G3 的教训反过来支持引入成熟引擎**：互操作难在协议细节（bencode 边界、扩展握手、
   tracker 语义），自己重写一遍 wire 协议，等于把 G3 已经踩过的坑再踩一遍，而且没有第三方
   客户端对照就无法证明「兼容」。

继续坚持全局零依赖，代价是重写 BT Peer Wire + tracker + 存储层；收益只是「构建不需要网络」——
而这一点已经可以用 `--offline` + 入库的 `Cargo.lock` 达成。所以零依赖作为**全局规则**应当取消。

## 决策

**按层划分，而不是一刀切。**

| 层 | crate | 依赖策略 | 理由 |
| :--- | :--- | :--- | :--- |
| 协议 / 信令面 | `cr-core` `cr-hash` `cr-torrent` `cr-kad` `cr-wake` `cr-store` `cr-indexer` `cr-nat` `cr-qos` `cr-sched` `cr-guard` `cr-reputation` `cr-dht` `cr-ed2k` `cr-api` | **继续 std-only** | 它们的价值是「每条结论都能回溯到一条实测报文」。这些 crate 的字节级行为已被 T0/T1/S1/S2/S6 证据固定（golden 帧、ed2k Red 分块边界、STUN 回包源地址校验），换实现等于让已有证据失效。 |
| 引擎 / 守护进程 | `cr-engine` `coldrevived` | **可用 librqbit + tokio** | 这两处要的是「真的把分片拉下来」，属于成熟协议栈的既有能力，自研只会更差且更慢。 |

配套纪律（四条，缺一条本决策就不成立）：

1. **版本只写在 `[workspace.dependencies]`**，各 crate 用 `.workspace = true` 引用，禁止散落。
2. **`Cargo.lock` 入库**，验收与 CI 一律 `cargo test --workspace --offline`；网络可达性变化
   不得影响构建结果。
3. **新增第三方依赖须登记**：本 ADR 的表格是白名单，新增 crate 要追加一行并写明用途；
   协议面任何 crate 想加依赖，必须另立 ADR。
4. **业务 crate 不直接 `use librqbit`**：只经 `cr-engine` 的 `Engine` Trait（ADR-002 原有约束）。

## 备选方案与否决理由

- **保持全局零依赖 + 自研精简 BT 层**：选项 C。要覆盖 BEP-3 分片协议、BEP-10 扩展握手、
  tracker(HTTP)、断点续传与逐片校验，才勉强能与 Transmission/qBittorrent 对话；工作量最大、
  兼容性风险最高，且无法用「社区客户端能下」证明——因为那正是我们缺的东西。
- **只给 `coldrevived` 加依赖，`cr-engine` 保持骨架**：`/jsonrpc` 会绕过 `Engine` Trait 直接
  调具体库，等于把 ADR-002 的隔离边界拆了，之后换引擎要改守护进程。
- **引入 libtorrent FFI**：ADR-002 已否决（跨语言内存安全 + 打包复杂度），本次不重开。

## 后果与边界

- **正面**：`coldrevived` 第一次有了真实数据面。2026-11-16 实测：B 节点被唤醒后经
  `aria2.addUri` 把种子交给 `cr-engine`，从 A 的内置种子端拉完 1 MiB（4 片、64 次 request），
  落盘文件 SHA-256 与样本一致，A 侧 `served_bytes = 1048576`（第二个计数器对上）。
  报告 `tests/e2e/w51/reports/engine-download-*.json`，`cargo test --workspace --offline` 全绿。
- **W5.1b 补充（2026-11-17）**：做种侧也切到引擎，`bt.rs` 里那套手写 Peer Wire 种子端
  （握手 / bitfield / request-piece）已删除，只保留本地 tracker 的 bencode 响应（仍 std-only）。
  理由是「两套做种实现必然行为漂移」——同一份共享文件不应该有两条供片路径。代价是 W5.1
  那条「手写服务端 ↔ librqbit 客户端」的独立互证失效（同库两端只能证明能互通，不能证明标准互通），
  因此新增 `tests/e2e/w51b/seed_to_thirdparty.py`：**我们的引擎做种，第三方 aria2 下载**，
  并带 COLD 负对照（唤醒前拿不到数据）。T0-08 同样是第三方 aria2 从引擎取片。
- **负面**：构建时间与二进制体积上升（librqbit 带 rustls/reqwest 依赖树）；`--offline` 的前提是
  本地/CI 缓存完整，换机器首次构建仍需一次联网拉取。
- **不变量**：`engine` 段缺省时引擎**根本不创建**（首次 `addUri` 才惰性建立，且默认不监听入站），
  所以 T0/T1 的复活路径字节级不变——G1 门禁复核仍是那 21+11 条，不因本决策放松。
- **不承诺**：本决策不改变 S6/G2 的任何状态（`s6_status=not_run`、`g2_claim=false`），
  也不产生任何可达率数字（[ADR-012](ADR-012-r04-best-effort.md) 仍然有效）。

## W5.1c 补充（2026-11-17）：LAN 复核暴露的引擎能力边界

把下载方搬出回环（192.168.1.221 的 Transmission 3.00 / qBittorrent 4.5.1）后，暴露一条
**引擎行为差异**：Transmission 默认优先 µTP，于是首连落在「TCP 已建立但无字节流动」的状态
（抓包：`221.19501 > 210.18782 UDP`，无 SYN）。处理方式是把客户端降到 TCP 并在文档/报告中写明，
**不修改验收标准去迁就实现**。

## W5.1e 更正（2026-11-17）：这不是能力边界，是默认值

上一条当时写的是「librqbit 只实现 TCP，不做 µTP」——**这个归因是错的**。
librqbit 9.x 的依赖里就有 `librqbit-utp`（BEP 29 的独立实现），`SessionOptions.listen` 走
`ListenerOptions`，而 `ListenerMode` 的默认值是 `TcpOnly`。**默认关 ≠ 不支持。**

教训与「拿假设当测量」同族：看到「客户端发的 UDP 包没人应答」就推断「库不支持 UDP」，
跳过了读一眼依赖清单和默认值。代价是把一个开关写成了「要自己实现 µTP」的结论。

现在的形态：`EngineOptions.enable_utp` 默认 `true` → `ListenerMode::TcpAndUtp`（UDP 与 BT 端口同号），
`nat.utp=false` 显式退回纯 TCP。分层策略不变：这个开关只存在于 `cr-engine`，
业务 crate 仍不 `use librqbit`；uTP 的失败形态是「只打日志、TCP 照旧」的静默降级，
因此引擎在会话建立后做一次同端口 UDP 绑定探测，并由验收脚本用**系统连接表里的 PID**
判定「到底谁占着那个 UDP 端口」（见 [TEST §2.7](../../05-quality/TEST.md)）。

## W5.1f 补充（2026-11-17）：能力与「能不能进来」是两件事

公网 µTP 通过之后补一条分层经验：`enable_utp` 打开只解决「引擎会不会说 µTP」，
不解决「外部 UDP 包能不能到达它」。后者取决于 CPE 上的端口映射，而 **TCP 映射对 UDP 无效**——
同一端口号要两条独立规则。因此验收脚本把「映射是否存在」拆成一步**与 µTP 无关**的回显对照，
并核对 A 侧看到的源地址是公网 IP（否则是 LAN 捷径）；映射缺失时判 BLOCKED，
不写成产品失败。这与上面「默认值不是能力边界」是同一族纪律：**先确认量具，再谈被测物**。
见 [TEST §2.8](../../05-quality/TEST.md)。

## 复核时点

- 补种上线前：复核二进制体积与常驻内存是否仍满足 NFR-05（1 万文件空闲 ≤100 MB）——
  librqbit 的会话与分片缓存计入 RSS，需实测而非估算。
- 每次升级 librqbit 主版本：按 [RISK R-12](../../05-quality/RISK.md) 复审上游维护状态，
  并重跑 `tests/e2e/w51/engine_download.py`。

## 相关文档

[ADR 索引](README.md) · [ADR-002](ADR-002-bt-engine.md) · [TECH-STACK §5](../TECH-STACK.md) ·
[TEST 兼容性矩阵](../../05-quality/TEST.md) · [src/README.md](../../../src/README.md)
