# W25 — FR-25 组件级：生产 crate `cr-ed2k` 真 TCP 回环取片

> 2026-11-20 · 生产 crate 33 单测 + 回环 e2e 1/1 · 口径：daemon 未接线、公网 **BLOCKED**。

## 做了什么

把 S2 审计过的四个零依赖模块用 `#[path]` 纳入生产 crate `cr-ed2k`（单一事实来源，
不复制、不漂移）：`kad-mini/sha1.rs`、`kad-mini/wire.rs`、
`ed2k-mini/wire.rs`、`ed2k-mini/stack.rs`。ADR-011 L2 的「下载/上传共用一份栈」保持成立。

## 证据（tests/e2e/w25/loopback_transfer.log）

`src/crates/cr-ed2k/tests/loopback_transfer.rs` 在 127.0.0.1 真 TCP 上同时起
`stack::serve()` 与 `stack::download()`，三种 PARTSIZE 边界全部通过：

| 用例 | 大小 | 结果 |
| :--- | :--- | :--- |
| single_part_4KiB | 4096 B | hash_matches + content_matches，blocks=1 |
| exact_one_part_boundary | 9728000 B（恰好 1 PARTSIZE，Red 空块哈希） | 53 blocks，字节一致 |
| multi_part | 9736192 B（> PARTSIZE） | 53 blocks，字节一致 |

服务端 `bytes_sent` 与下载端 `data.len()` 逐字节对上；`completed=true`、`hash_matches=true`。

## 边界（照写，不许放宽）

- **没有 daemon 生产接线**：`cr-ed2k` 目前不是任何 bin 的依赖，只有 crate 级测试。
- **没有全量共享文件双轨发布**：只有单文件 KAD 索引面探针（S2 `l2-source-probe`）。
- **公网 eD2k 传输 BLOCKED**：没有可连的公网 eMule server，没有公网 peer TCP 证据；
  S2 spike 所有报告固定 `public_network_contacted=false`、`evidence_valid_for_pass=false`。
- 不做 LoginServer / source exchange / 压缩 / 混淆 / credit / AICH / >4 GiB I64 / NAT
  （ADR-011、ADR-004 明确排除）。
- 这是「栈可用」的组件级证据，**不是 FR-25 全项完成**；`g2_claim=false`、`s6_status=not_run` 不变。

## 复现

```bash
CARGO_INCREMENTAL=0 cargo test --offline -p cr-ed2k --test loopback_transfer -- --nocapture
```
