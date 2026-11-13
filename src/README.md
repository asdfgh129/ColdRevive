# 源码目录（src/）

> **当前状态**：目录骨架阶段。[G0 技术路线条件冻结](../docs/06-plan/G0-REVIEW.md) 已于 2026-11-13 由项目所有者批准，
> 按 [ROADMAP M0](../docs/06-plan/ROADMAP.md) 门禁**已允许**创建 `Cargo.toml` 与 crate 实现；
> 但 M1 二进制与 `CliAdapter.wired` 两项工程接线仍未完成（`m0_preflight.py` 报 2 个阻塞项），本轮未创建生产 crate。
> S3 / S5 / S6 仍为 `not_run`（豁免 G0 时点结论，M1/M2 必测回填），详见 G0-REVIEW §3 与 §6。

## 规划结构

Rust Cargo Workspace，按 [ARCH §3.4 模块清单](../docs/02-architecture/ARCH.md) 与 [ROADMAP WBS](../docs/06-plan/ROADMAP.md) 一一对应：

```
Cargo.toml                  workspace 根清单（M0 创建）
src/
├── crates/
│   ├── cr-core/            公共类型、错误码、配置、trace_id
│   ├── cr-indexer/         M-01 共享目录监控
│   ├── cr-hash/            M-02 指纹计算
│   ├── cr-torrent/         M-03 伴随种子生成
│   ├── cr-store/           M-04 影子索引库（SQLite）
│   ├── cr-kad/             M-05 KAD 适配
│   ├── cr-dht/             M-06 Mainline DHT 适配
│   ├── cr-nat/             M-07 连通性
│   ├── cr-wake/            M-08 唤醒信令
│   ├── cr-sched/           M-09 冷热调度
│   ├── cr-guard/           M-10 握手拦截
│   ├── cr-engine/          M-11 BT 引擎抽象（Engine Trait，屏蔽 rqbit / libtorrent 差异）
│   ├── cr-ed2k/            M-12 跨协议补源（P2 / v2）
│   ├── cr-qos/             M-13 资源治理
│   ├── cr-api/             M-14 接入层
│   ├── cr-reputation/      M-15 信誉（P2）
│   └── cr-notify/          跨平台通知（可选）
├── bins/
│   ├── coldrevived/        常驻守护进程（引擎 + 索引 + DHT + HTTP/WS/RPC）
│   ├── coldrevive-agent/   托盘 / 浏览器捕获 / UPnP 续租 / 看护
│   └── coldrevive-cli/     CLI（M1 MVP 入口）
└── ui/                     Web UI 静态资源（编译期嵌入二进制）
```

## 约定

- **引擎抽象先行**：`cr-engine` 定义 `Engine` Trait（Pause/Resume、按需实例化、握手回调）。这是 [ADR-002](../docs/02-architecture/adr/ADR-002-bt-engine.md) 与 M0 Spike S1 的退路保障，不允许业务 crate 直接依赖具体 BT 库。
- **模块边界即 crate 边界**：`M-xx` 编号在文档、crate 名、日志 `target` 中保持一致。
- **协议独立**：私有唤醒报文定义放 `cr-wake`，版本字段 `PROTOCOL_VERSION` 单一来源；任何变更须递增版本并同步 [PROTO](../docs/04-protocol/PROTO.md)。
- **默认最小暴露**：回环监听、空共享目录、RPC 需密钥，见 [CONFIG](../docs/04-protocol/CONFIG.md)。
