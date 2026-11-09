# S1 最小 Demo：BT 引擎能力探测（`tests/spikes/s1/`）

> 独立 Cargo 工程，**不在项目根创建 `Cargo.toml`、不修改 `src/`**（M0/G0 门禁要求）。
> 状态：`not_run` 的部分观测已产出，但 **S1 整体结论仍待实验执行人回填**。

## 1. 目的

验证 [TECH-STACK §2](../../../docs/02-architecture/TECH-STACK.md) 的 S1 假设：rqbit / librqbit 是否支持
按需实例化、Pause-Resume、握手拦截与分片校验。判据来源为 `tests/spikes/spikes.json` 的 `S1.criteria`。

## 2. 运行

```bash
cd tests/spikes/s1
cargo run --release
```

报告落在 `reports/s1-probe-<unix-ts>.json`，运行数据在 `.run/`，两者均不纳入版本控制。

**网络边界**：全程 `127.0.0.1` 回环。`SessionOptions` 中 DHT 置 `None`、tracker 与 LSD 关闭、
`listen`/`connect` 置 `None`，不接触公共网络。样本由固定种子 `20261109` 确定性生成，无版权内容。

## 3. 观测了什么

| 步骤 | 做法 | 对应判据 |
| :--- | :--- | :--- |
| 伴随种子 | 按 bencode 生成单文件 `.torrent`，`piece length=16 KiB`，逐片 SHA-1 写入 `pieces`；InfoHash = SHA-1(info 字典原始字节) | S1-C4 的数据基础 |
| 握手拦截 | 本地 TCP 回环，请求方发送标准 68 字节 BT Handshake；持有者 `read_exact` 后解析 `pstrlen / protocol / reserved / info_hash / peer_id` | S1-C1 |
| 按需实例化 | 用握手里的 `info_hash` 查「InfoHash → 伴随种子」索引，读出种子字节，交给 `AddTorrent::TorrentFileBytes` 并以 `AddTorrentOptions { paused: true }` 实例化 | S1-C2 |
| Pause / Resume | `Session::unpause` → `pause` → `unpause`，每步读 `is_paused()` 与 `stats().progress_bytes` | S1-C3 |
| 分片校验 | 重新读磁盘样本，按 16 KiB 分块计算 SHA-1，与伴随种子 `pieces` 逐条比对 | S1-C4 的校验半项 |

## 4. 已知边界（不得据此判 `pass`）

- **rqbit 9.1.0 未暴露公开握手拦截钩子**：`SessionOptions` 无回调字段，`session.rs::task_listener`
  内部 `read_handshake()` 后直接按 `info_hash` 查已管理 torrent，不给调用方拦截点。
  ColdRevive 的「握手拦截器」需自实现（本 Demo 即自实现）或在 rqbit 之外加代理层，
  这会影响 [ADR-002](../../../docs/02-architecture/adr/ADR-002-bt-engine.md) 的选型论证。
- **S1-C3 只算 partial**：样本文件预先存在，位图自始至终完整；没有覆盖「传输中途暂停再恢复」。
- **S1-C4 只算 partial**：逐片 SHA-1 校验通过，但未建立双 peer 的真实分片传输。
- 脚本 `overall` 恒为 `partial`、`s1_status` 恒为 `not_run`，**绝不自动置 `pass`**。

## 5. 结论回填

按 [tests/spikes/README.md §6](../README.md) 的规则，由实验执行人依据报告中的证据路径手工修改
`spikes.json` 的 `S1.status`。若要判 `pass`，需补齐：真实双 peer 分片传输记录、传输中途暂停/恢复的
位图对比、以及握手拦截方案（自实现或代理层）的架构结论与 ADR-002 更新。
