# S2 实验目录（`tests/spikes/s2/`）

> **本目录不是 S2 结论。** 按 [tests/spikes/README.md §5](../README.md) 与 `spikes.json` 的
> `S2.criteria`，本地回环私有 KAD 子网只能作为预演，**不得据此把 S2 判为 `pass`**。
> 实网实施排期与门禁见 **[PLAN.md](PLAN.md)**。

## 1. 三层产出

| 层 | 路径 | 作用 |
| :--- | :--- | :--- |
| L1 逻辑预演 | [`s2_loopback.py`](s2_loopback.py) | JSON 消息的私有子网双节点，只验流程与命名空间概念，**不是真实协议** |
| L2 **真实 wire 闭环** | [`kad-mini/`](kad-mini/README.md) | 独立 Rust Cargo 工程，真实 `0xE4` Kademlia2 报文，BOOTSTRAP / HELLO / 迭代查找 / PUBLISH_KEY / SEARCH_KEY |
| L3 实网小样本 | `kad-mini live` | **未执行**，被 [PLAN.md §2](PLAN.md) 的门禁拒绝 |

```bash
python3 tests/spikes/s2/s2_loopback.py                    # L1
cd tests/spikes/s2/kad-mini && cargo test                 # L2 单测 7/7
cd tests/spikes/s2/kad-mini && cargo run -- selftest      # L2 常量/编解码断言 29/29
cd tests/spikes/s2/kad-mini && cargo run -- loopback      # L2 真实报文闭环
cd tests/spikes/s2/kad-mini && cargo run -- live --check  # L3 门禁检查（当前退出码 2）
```

报告落在 `reports/`（不纳入版本控制），固定标注
`public_kad_contacted: false`、`evidence_valid_for_pass: false`、`s2_status: "not_run"`。

## 2. L1 逻辑预演

只用 Python 标准库，在 `127.0.0.1` 起两个 UDP 节点：A 发布 5 个 `ColdRevive_V1:`
前缀 Key（Key ID 用 SHA-256 前 16 字节映射到 128-bit 空间），B 按 XOR 距离选最近节点发
`FIND_VALUE` 并命中；额外发布一个无前缀同名 Key 验证 ID 隔离。发布 5/5、命中 5/5。

**注意**：这层的 `FIND_VALUE` / `FIND_VALUE_REPLY` 是自定义的 JSON 消息，eMule KAD 里
并不存在这两个操作。真实映射见 L2。

## 3. L2 真实 wire 闭环（2026-11-09 已跑通）

[kad-mini](kad-mini/README.md) 零第三方依赖，报文字节逐条对齐 aMule 源码。三端点
（发布者 A / 索引节点 / 查询者 B）全程 `127.0.0.1`：

```
BOOTSTRAP_REQ/RES → HELLO_REQ/RES → KADEMLIA2_REQ/RES (迭代查找)
→ PUBLISH_KEY_REQ/RES → SEARCH_KEY_REQ/RES (B 命中 A 的描述符)
```

实测 5/5 发布被 ACK、5/5 查询命中且描述符 Tag 匹配、RTT p50 0.23 ms、
84 条收发报文的完整十六进制逐次记录、0 协议错误。命名空间隔离验证通过
（带前缀与无前缀 Key ID 不同，控制条目未泄漏到带前缀查询）。

**两层 ID 口径不同**：L1 用 SHA-256 前 16 字节、L2 用 SHA-1 前 16 字节派生 Key ID，
同一逻辑 Key 在两层得到不同 ID，结果不可直接比较；两者都是临时映射，
最终规则待 ADR / PROTO 决策（见 [PLAN.md §0](PLAN.md)）。

**关键发现**：5 个 Key 相对索引节点的 `xor_top32` 全部远超 `SEARCHTOLERANCE = 2^24`，
回环靠 aMule 的 LAN 豁免才被接受；在 WAN 上会被**静默丢弃、无任何响应**。
实网发布必须先多跳迭代查找到 ID 落在 Key 邻域的节点。详见 [PLAN.md §0](PLAN.md)。

## 4. 为什么还没有实网结论

`spikes.json` 的 `S2.public_kad_approval` 已登记项目所有者的人工批准，但这只是**允许做实网实验**，
不等于实验已完成。L2 已补齐了原先缺的第 1 项（协议实现），仍缺：

1. **G0 评审确认的样本量与限速阈值**：批准记录中的 `pending` 项，未确认前不得向公共网络发布测试 Key。
2. **用户提供的 bootstrap 节点清单**：[OPS §6](../../../docs/07-operations/OPS.md) 禁止内置默认
   bootstrap 资源列表，仓库不含任何公共节点 IP。
3. **逐次实网原始记录**：发布成功率、查询时延分布、被现有节点丢弃/拒绝的观测。

`kad-mini live --check` 当前明确列出 5 项阻塞并拒绝发送任何报文。三者齐备前
S2 状态保持 `not_run`。合规红线：人工批准、最小样本、限速、禁止批量发布测试 Key。
