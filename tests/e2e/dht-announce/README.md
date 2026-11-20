# 公网 DHT announce：自有公网节点已闭环，Mainline 仍 BLOCKED

> 2026-11-20 补测。所有者已明确授权执行受控 `announce_peer` 并回查。
> 宣告的 infohash `6d207826…c50fc99a8a7482e37` 与端口 `17200` 对应 222 上
> **已验证真实可达**的 BT 端点 `112.5.32.34:17200`（W3b），不是伪造 peer。

## 只读基线

| 命令 | 结果 |
| :--- | :--- |
| `--get-peers 82.157.68.159:6881 <infohash>` | `ok=true`、`token_len=8`、`nodes=8`、`peers=0` |
| `--walk 82.157.68.159:6881,dht.transmissionbt.com:6881 --rounds 3 --max-queries 32` | `queried=32`、`ping_alive=17`、`find_node_answered=17`、`unique_contacts=112`、`frontier_left=82`、`announced=none` |

原始产物：`get-peers.json`、`walk.json`。

## 已闭环：自有公网 DHT 节点上的 announce + 独立回查

从 **222**（源 IP 即 `112.5.32.34`，与所宣告端口真实匹配）执行：

```bash
/usr/local/bin/coldrevive-dht --get-peers 82.157.68.159:6881 \
  6d20782655acfb074d7e518c50fc99a8a7482e37 --announce 17200
```

原始产物 `announce.json`：`token_len=8`，
先返回 `mode=get-peers`，再返回 `{"ok":true,"mode":"announce","port":17200}`。

从 **A**（不同公网出口 `112.5.60.4`）独立回查同一节点：

```bash
./target/release/coldrevive-dht.exe --get-peers 82.157.68.159:6881 \
  6d20782655acfb074d7e518c50fc99a8a7482e37
```

原始产物 `verify.json`：`peers=1`、`peer_list="112.5.32.34:17200"`。
这是两个不同来源的观测：写入端拿 token，回查端不是写入进程。

## 仍 BLOCKED：真实 Mainline 引导节点

| 引导节点 | get_peers 结果 |
| :--- | :--- |
| `dht.transmissionbt.com:6881` | `token_len=0`、`nodes=8`、`peers=0` → 不能 announce |
| `router.bittorrent.com:6881` | `DHT 查询超时 3000 ms` |
| `router.utorrent.com:6881` | `DHT 查询超时 3000 ms` |
| `82.157.68.159:6881`（自有） | `token_len=8`，announce 成功 |

因此**只**能说「自有公网 DHT 节点上的 announce/回查闭环」，
**不能**说「已加入真实 Mainline 网络」或「公网 DHT 拓扑可用」。
`g2_claim=false`、`s6_status=not_run` 不变。

## 复现

```bash
# 只读基线
./target/release/coldrevive-dht.exe --get-peers 82.157.68.159:6881 <infohash-hex>
timeout 120 ./target/release/coldrevive-dht.exe \
  --walk 82.157.68.159:6881,dht.transmissionbt.com:6881 --rounds 3 --max-queries 32

# 授权后写入 + 异机回查（见上）
```

