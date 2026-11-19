# W58：双进程「持有者 splice + 中继数据面」实测（v2.4.92）

> `g2_claim=false`、`s6_status=not_run`、`is_s6_evidence=false`。
> 本目录证明的是**两个真实守护进程经中继交换 BT 字节**，**不是** NAT 可达性证据。

## 拓扑

| 角色 | 主机 | 地址 | 进程 |
| :--- | :--- | :--- | :--- |
| 持有者 | 222（Ubuntu 23.04，真 NAT `112.5.32.34`） | `192.168.1.222` / `192.168.34.222` | `coldrevived-w58 --node-id 222`（role=holder，`nat.punch_route=true`） |
| 下载方 | 221（Ubuntu 23.04，port-restricted cone `112.5.60.4`） | `192.168.1.221` | `coldrevived-w58 --node-id 221`（role=member） |
| 中继 | 221 | `192.168.1.221:7100` | `coldrevive-relay --allow-insecure --stats-file /tmp/relay2-stats.json` |

两台都没有任何入向映射。下载方拨的 peer 地址是**中继会合地址** `203.0.113.31:17902`
（RFC 5737 文档网段，不可路由），所以字节**只能**经中继：

```text
librqbit → SOCKS5(127.0.0.1) → cr-bridge（命中 203.0.113.31:17902）
        → RelayTunnel（CD|channel|DATA）→ 中继 192.168.1.221:7100
        → 持有者订阅连接 → TunnelMux → splice → 127.0.0.1:17902（真引擎）
```

## 步骤（`w58-run.sh`，在 221 上执行）

```bash
/root/cr-w58/start-holder.sh                       # 222：做种 + 订阅中继 + 数据面
/root/cr-w58/start-member.sh                       # 221：下载方 + SOCKS5 桥
ssh 222 'curl -s http://127.0.0.1:17900/health'     # 200，holder_data.enabled=true
/root/cr-w58/w58-run.sh 0f67435d…54b56 cdc00479…4fb56
```

脚本依次做四件事，每步输出都落在 `reports/`（无手写、无补录）：

1. `GET /nat/advertise` → `advertise.json`（`bt_external.kind=relay`，`ip=203.0.113.31`）
2. `POST /cli {"command":"nat","args":["peer", …]}` → `peer.json`
3. `POST /cli {"command":"nat","args":["wake", file_hash, infohash]}` → `wake.json`
4. `POST /jsonrpc aria2.addUri`（取种 URL 走 221→222 的 LAN HTTP）→ `tellstatus.json`
   并读取两端 `/health` 与中继统计。

## 结果

| 观测 | 值 | 来源 |
| :--- | :--- | :--- |
| 唤醒路径 | `path=relay`，`ack_status=OK`，`rtt_ms=0`（LAN 中继） | `wake.json` |
| 直连候选 | `private-lan 192.168.34.222:17901` 无应答，1 216 ms 后回落中继 | `wake.json.attempts[0]` |
| 数据面握手 | `中继数据面已确认（OPEN/OPENED 双向握手完成）channel=842150450` | `wake.json.notes[2]` |
| 隧道登记 | `tunnels=1`、`served_registered=1`、`bytes_relayed=1,050,849` | `member_health.json` |
| 持有者 splice | `opens=1`、`accepted=1`、`splices_done=1` | `holder_health.json` |
| 持有者字节 | `→引擎 1,264 B`（BT 握手+请求）、`←引擎 1,049,585 B`（分片） | `holder_log.txt` |
| 引擎自证 | `seed_active=1`、`served_bytes=1,048,576`（**引擎自己的计数器**） | `holder_health.json` |
| 下载结果 | `status=complete`，`completedLength=1,048,576` | `tellstatus.json` |
| 内容一致 | 两端 md5 均为 `6172cae176d636d28590b1a9d57d5852` | `md5_*.txt` |
| 中继计费 | `data_bytes_admitted=1,050,048`（首轮结束时）、`bandwidth_limited=0`、`dropped=0` | `relay_stats.json`（累计两轮 `2,099,990`） |

`1,050,849 = 1,264 + 1,049,585`（桥按**流字节**计），`1,050,048` 是**被计费的帧负载**
（ADR-014 只对超过 4096 B 信令阈值的帧收费），**两个数本来就不该相等**，不要拿差值当丢包。

## 这组证据能说什么 / 不能说什么

**能说**：

- 持有者侧的 `relay_sub` 会在**同一条订阅连接**上识别数据面帧、拨本地引擎、回 `OPENED`、
  做双向 splice——两端都是真进程，**没有任何手工登记**。
- 握手是双向的：下载方要等 `OPENED` 才把隧道登记进 SOCKS5 桥（`wake.json.notes` 那句
  「已确认」就是它的观测）；没有这一步，日志里会出现「隧道已登记」但 BT 永远连不上。
- 1 MiB 分片真的穿过中继，且引擎与中继两侧的计数器互相印证。

**不能说**：

- **不是 NAT 可达证据**：两台主机同处 `192.168.1.0/24`，`nat_type=unknown`，
  `s6_status=not_run`。会合地址不可路由只保证「这次没有回落直连」，不证明打洞结论。
- **不是公网中继证据**：本轮用的是 221 上的 LAN 中继。原因如实记录：
  从 222 到 `82.157.68.159:7100` relaybench 3/3（p50 84 ms），但从 221 到同一端点
  能建 TCP、`REGISTER` 却拿不到 ACK（`A 注册失败: 空闲超时`，`--insecure` 同样），
  原因未定位，故不拿它当证据。
- **不是吞吐结论**：单次 1 MiB、未做压测、未测并发，且中继默认有 512 KiB/s 令牌桶。

## 复现

```bash
# 本机（crate 级 + 生产 serve 的 splice）：
cargo test --offline -p cr-nat relay_tunnel
cargo test --offline -p coldrevived relay_sub
cargo test --offline -p coldrevived health_answers
# 双进程：
scp 本目录 holder.json member.json 到两台主机 /root/cr-w58/
/root/cr-w58/start-holder.sh && /root/cr-w58/start-member.sh
/root/cr-w58/w58-run.sh <infohash> <file_hash>
```
