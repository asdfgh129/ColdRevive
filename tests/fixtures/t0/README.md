# T0 测试数据（tests/fixtures/t0/）

> 本目录只存放**自制、确定性、可重复生成**的测试数据。按 [TEST §2](../../../docs/05-quality/TEST.md) 的约定，`fixtures/` 中禁止存放任何真实受版权保护内容。

## 目录

| 路径 | 用途 | 是否入库 |
| :--- | :--- | :--- |
| `golden/wakeup_req.v1.json` | `WAKEUP_REQ` 契约 golden，字段对应 PROTO §3.2 | 是 |
| `golden/wakeup_ack.v1.json` | `WAKEUP_ACK` 契约 golden，字段对应 PROTO §3.3 | 是 |
| `sample/` | 运行时由 `t0_harness.py` 生成的确定性样本文件 | 否，已由 `.gitignore` 排除 |

## 样本生成规则

- 文件：`coldrevive-t0-sample.bin`，262,144 字节，`random.Random(20261109).randbytes()` 生成。
- 内容不含任何第三方素材；固定种子保证任何机器、任何时间生成的 SHA-256 完全一致。
- 参数位于 `tests/e2e/t0/t0_config.json` 的 `fixture` 段，**不要在代码里另写一份**。
- `Key` 计算遵循 [PROTO §1](../../../docs/04-protocol/PROTO.md)：`Target_Key = SHA1("ColdRevive_V1:" + file_hash)`，`Keyword_Key = SHA1("ColdRevive_V1:" + "kw:" + normalized_term)`。
- 骨架里的 `infohash_stub` 是**合成值**，不是真实 `.torrent` 的 InfoHash；M1 接线后必须换成伴随种子的真实 InfoHash。

## golden 报文约定

- 占位值统一用 `*_PLACEHOLDER_*`，契约测试只比对**字段名、类型、嵌套结构与枚举取值**，不比对具体值。
- `capabilities.ed2k` 在 v1 固定为 `false`（ADR-004），改 `true` 即视为契约破坏。
- `PROTOCOL_VERSION` 递增时，golden 文件名同步为 `wakeup_req.v2.json` 等，v1 文件保留用于兼容性回归，见 [PROTO §4](../../../docs/04-protocol/PROTO.md)。
