# S2 本地回环预演（`tests/spikes/s2/`）

> **本目录不是 S2 结论。** 按 [tests/spikes/README.md §5](../README.md) 与 `spikes.json` 的
> `S2.criteria`，本地回环私有 KAD 子网只能作为预演，**不得据此把 S2 判为 `pass`**。

## 1. 运行

```bash
python3 tests/spikes/s2/s2_loopback.py
```

报告落在 `reports/s2-loopback-<timestamp>.json`（不纳入版本控制）。只用 Python 标准库，
在 `127.0.0.1` 上起两个 UDP 节点，不接触公共 eMule KAD。

## 2. 预演内容

| 步骤 | 做法 |
| :--- | :--- |
| A 发布 | 生成 5 个 `ColdRevive_V1:` 前缀 Key，Key ID 用 SHA-256 前 16 字节映射到 128-bit KAD ID 空间，写入 A 的存储 |
| B 查询 | B 按 XOR 距离选出最近节点（双节点即 A），发 `FIND_VALUE`，A 回 `FIND_VALUE_REPLY` |
| 观测 | 逐次记录发布结果、查询命中、RTT（min / p50 / max）、`msg_id` |
| 命名空间隔离 | 额外发布一个无前缀的同名 Key，验证 ID 不同、带前缀 Key 的值未被覆盖 |

报告里 `public_kad_contacted: false`、`evidence_valid_for_pass: false`、`s2_status: "not_run"` 为固定值。

## 3. 为什么还没有实网结论

`spikes.json` 的 `S2.public_kad_approval` 已登记项目所有者的人工批准，但这只是**允许做实网实验**，
不等于实验已完成。要产出 S2 实网结论，还缺三样：

1. **真实的 eMule KAD 协议实现**：需实现 Kademlia2 UDP 报文、节点 bootstrap、握手/加密与
   `ColdRevive_V1:` 前缀 Key 的发布/查询语义；本项目尚未实现（G0 前不会创建 `src/`）。
2. **G0 评审确认的样本量与限速阈值**：批准记录中的 `pending` 项，未确认前不得向公共网络发布测试 Key。
3. **逐次原始记录**：发布成功率、查询时延分布、被现有节点丢弃/拒绝的观测，须来自实网小样本。

在三者齐备前，S2 状态保持 `not_run`。合规红线见 [OPS §6](../../../docs/07-operations/OPS.md)：
人工批准、最小样本、限速、禁止批量发布测试 Key。
