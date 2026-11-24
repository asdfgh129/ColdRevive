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

## 2026-11-20 历史基线：引导节点本身仍不签发 token

| 引导节点 | get_peers 结果 |
| :--- | :--- |
| `dht.transmissionbt.com:6881` | `token_len=0`、`nodes=8`、`peers=0` → 不能 announce |
| `router.bittorrent.com:6881` | `DHT 查询超时 3000 ms` |
| `router.utorrent.com:6881` | `DHT 查询超时 3000 ms` |
| `82.157.68.159:6881`（自有） | `token_len=8`，announce 成功 |

## 2026-11-24 更新：真实 Mainline 节点 announce + 异机回查已闭环

从 `dht.transmissionbt.com:6881` 做只读 `find_node`，222 发现邻居
`37.48.95.218:3691`；先 `get_peers` 得到真实 `token_len=4`，确认
`112.5.32.34:17200` TCP 可连接后，按 2026-11-20 的所有者授权执行受控
`announce_peer(17200)`。随后由**另一台主机 221**对同一节点/infohash 独立
`get_peers`，返回 `peers=1`、`peer_list=112.5.32.34:17200`。

证据：`reports/mainline-announce-verify-20261124.json` 与
`tests/ARCHIVE/evidence/mainline-{discovery-a,discovery-222,token-222-before,announce-222,verify-221}-20261124.jsonl`。

边界：这只闭合 FR-18 的「真实 Mainline 节点可加入并回查」证据；不是
G2 通过，不证明 DHT 拓扑整体可用，也不改 `g2_claim=false` /
`s6_status=not_run` / `evidence_valid_for_pass=false` / `auto_pass=false`。
引导节点 `dht.transmissionbt.com` 自身仍 `token_len=0`，这一历史事实不变。

## 复现

```bash
# 只读基线
./target/release/coldrevive-dht.exe --get-peers 82.157.68.159:6881 <infohash-hex>
timeout 120 ./target/release/coldrevive-dht.exe \
  --walk 82.157.68.159:6881,dht.transmissionbt.com:6881 --rounds 3 --max-queries 32

# 授权后写入 + 异机回查（见上）
```

