# 公网 DHT announce：只读探测有进展，写操作仍 BLOCKED

> 2026-11-20 补测。**没有执行 `announce_peer`**：把信息写进公网 DHT 是外部可见行为，
> 需要所有者明确授权（M2-PROGRESS §8）。本文只更新「只读拓扑是否可用」这一半事实。

## 本轮只读结果

| 命令 | 结果 |
| :--- | :--- |
| `--get-peers 82.157.68.159:6881 <infohash>` | `ok=true`、`token_len=8`、`nodes=8`、`peers=0` |
| `--walk 82.157.68.159:6881,dht.transmissionbt.com:6881 --rounds 3 --max-queries 32` | `queried=32`、`ping_alive=17`、`find_node_answered=17`、`unique_contacts=112`、`frontier_left=82`、`announced=none` |

原始产物：`get-peers.json`、`walk.json`。

## 与旧结论的关系

旧记录（M2-PROGRESS §8/§9）写的是「引导节点返回同一不可达节点 8 次 + 无可用邻居」。
本轮 `walk` 拿到 112 个不同 contact、17 个 ping 存活，**说明只读拓扑探测不再完全不可用**；
但 `get_peers` 仍 `peers=0`，且我们**没有**验证这些 contact 能否承载 announce token 回执。

## 仍 BLOCKED 的原因

1. **授权**：`announce_peer` 会把我们的 infohash/port 写到公网 DHT，属于外部可见写操作；
   本项目此前明确要求所有者授权后才能执行。本轮没有得到该授权。
2. **证据**：即使授权，也需要先证明「拿到 token 的节点真的接受 announce 并可被回查」，
   而不是只看到 `unique_contacts` 变多；`peers=0` 不能当成可用邻居。

复现（只读，安全）：

```bash
./target/release/coldrevive-dht.exe --get-peers 82.157.68.159:6881 <infohash-hex>
timeout 120 ./target/release/coldrevive-dht.exe \
  --walk 82.157.68.159:6881,dht.transmissionbt.com:6881 --rounds 3 --max-queries 32
```
