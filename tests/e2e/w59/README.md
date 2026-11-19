# W59：公网双端「打洞失败 → 中继接管」BT 数据面实测（v2.4.93）

> `g2_claim=false`、`s6_status=not_run`、`is_s6_evidence=false`。
> 本目录证明的是**两个真实 NAT 后的守护进程经公网 TCP 中继搬完 1 MiB 并逐字节一致**，
> **不是** NAT 可达性证据（理由见文末「边界」）。

## 为什么有这一轮

W58（v2.4.92）证明了持有者侧 splice 能工作，但那是**同处 `192.168.1.0/24` 的两台机器 + LAN 中继**。
本轮把两个条件都换掉：中继搬到公网（`82.157.68.159:7100`，PSK 认证），
一端换成真实的家庭宽带出口（本机 Windows，`112.5.60.4`），另一端是真 NAT（222，`112.5.32.34`）。

## 拓扑

| 角色 | 主机 | 内网 | 公网出口 | 进程 |
| :--- | :--- | :--- | :--- | :--- |
| 持有者 | 222（Ubuntu 23.04） | `192.168.34.222`（+`192.168.1.222` 管理面） | `112.5.32.34` | `coldrevived-w59 --node-id 222`，role=holder，`nat.punch_route=true` |
| 下载方 | A（本机 Windows 10/11） | `192.168.1.210` | `112.5.60.4` | `coldrevived.exe --node-id A`，role=member |
| 中继 | 82.157.68.159（腾讯云轻量） | 私网 `10.2.0.16` | — | `coldrevive-relay --listen 0.0.0.0:7100 --auth-key file:/root/relay.key` |
| STUN | 同上 | — | — | `coldrevive-nat --serve-stun 0.0.0.0:3478 --stun-alt 0.0.0.0:3479` |

两台客户端**都没有任何入向端口映射**：222 的 `/nat/public` 返回 `bt_external=null`、
`inbound_verified=false`；A 侧同理。

数据面路径（字节只能这么走）：

```text
librqbit → SOCKS5(127.0.0.1) → cr-bridge（命中 203.0.113.31:17902）
        → RelayTunnel（CD|channel|DATA）→ 公网中继 82.157.68.159:7100（PSK 加密记录层）
        → 持有者订阅连接 → TunnelMux → splice → 127.0.0.1:17902（222 的真引擎）
```

## 步骤

```bash
# 222：做种 + 订阅公网中继 + 数据面（pidfile 启停，别用 pkill -f）
ssh 222 /root/cr-w59/start-holder.sh

# A（本机 Windows）：下载方 + SOCKS5 桥
./target/release/coldrevived.exe --config tests/e2e/w59/member.json --node-id A

# A：驱动（advertise → peer → plan → wake → aria2.addUri → 轮询 → 两端 /health）
sh tests/e2e/w59/w59-run.sh e3bb049daa21d0a0cbafc0dffd550f94e7140759 \
                            0e5103fe52ba254c6a4ccfd4235c9710f68bb54817f9088c95ca42ed1ae41961
```

`report.json` 由脚本从**原始产物**生成并做 13 条断言（无手写数字、无补录）：
`python tests/e2e/w59/make_report.py` 式的逻辑见该文件的 `assertions` 字段。

## 结果（v2.4.93，一次干净运行）

| 观测 | 值 | 来源 |
| :--- | :--- | :--- |
| 唤醒路径 | `path=relay`、`ack_status=OK` | `reports/wake.json` |
| 直连候选 | `private-lan 192.168.34.222:17901` 从 A 不可达，**1204 ms** 无应答 | `reports/wake.json.attempts[0]` |
| 数据面握手 | `中继数据面已确认（OPEN/OPENED 双向握手完成）: channel=842154240` | `reports/wake.json.notes[2]` |
| 下载结果 | `status=complete`、`completedLength=1048576`、**<2 s** | `reports/tellstatus.json` |
| 内容一致 | 两端 md5 均为 `b0e93e825f86000e5341cf4aecb0559e` | `reports/md5_{holder,member}.txt` |
| 持有者 splice | `opens=1 accepted=1 data_frames=51 splices_done=1` | `reports/holder_health.json` |
| 持有者字节 | `→引擎 1,264 B`（BT 握手+请求）、`←引擎 1,049,585 B`（分片） | `reports/holder_log.txt` |
| 引擎自证 | `seed_active=1`、`served_bytes=1,048,576` | `reports/holder_health.json` |
| 下载方隧道 | `tunnels=1`、`tunnels_alive=1`、`served_registered=1` | `reports/member_health.json` |
| 桥字节 | `bytes_relayed=1,050,849 = 1,264 + 1,049,585` | `reports/member_health.json` |
| 中继单轮增量 | `data_bytes_admitted` **+1,050,048**、`delivered` +167、**`dropped` +0** | `relay_before.json` / `relay_after.json` |
| 限速 | `bandwidth_limited` +0、`rate_limited` +0、`rejected` +0、`bad_tag` +0 | 同上 |
| 两源同时在线的采样 | **60/60** 个采样点同时存在 `112.5.60.4` 与 `112.5.32.34`；其中 35 个点下载方有**两条**连接（订阅+数据面） | `reports/relay_conn_samples.txt` |

`1,050,849` 是**流字节**，`1,050,048` 是**被计费的帧负载**（ADR-014 只对 >4096 B 的帧计费），
两者本来就不该相等，**不要拿差值当丢包**。

## 这一轮真正修掉的东西：v2.4.92 的公网数据面是坏的

第一次按 W58 的方式跑，**数据一个字节都没到**，而且看板全是「正常」：

| 观测 | 坏的表现 | 说明 |
| :--- | :--- | :--- |
| 持有者 | `←引擎 1,049,585 B`、`splices_done=1` | 引擎确实把整个文件写进了隧道 |
| 下载方 | `bytes_relayed=1441`、`tunnels=1` | 桥只搬了 1441 B |
| 中继 | `dropped` **+98 / +224** | 目标节点「不在注册表里」——连接已经死了 |
| 下载 | `completedLength=0`，文件是 1 MiB 全零（aria2 预分配） | — |

`reports/timeline.jsonl` 是那次失败的时间线（每 500 ms 采两端 `/health`）：
第 9 秒持有者已经写完 1 MiB，第 19 秒下载方才把 `bytes_relayed` 从 0 变成 1441 —— 一个连接只活了这么久。

**根因**（`cr-nat`）：`RelayConn` 读帧用 `read_exact` 读帧头/帧体，读超时被归类成「本轮没有帧」
后直接返回，但 `read_exact` 超时前**可能已经消费掉半帧**，这些字节被丢弃。下一轮把帧体当帧头解析
→「帧体超限」/解析失败 → 订阅/隧道线程 `return Err` → 连接关闭 → 中继把后续所有帧按
「目标不存在」计 `dropped`。加密路径（`SecureReader::recv`）同样如此，而公网中继**只走加密路径**。

为什么 LAN 测不出来：W58 的中继在同一个交换机上，记录总是整块到达，20 ms 轮询超时永远落在帧边界上。
公网 16 KiB 分块 + 512 KiB/s 令牌桶 + 85 ms RTT，超时落在记录中间是常态。

**修法（v2.4.93）**：新增可续读缓冲 `cr_nat::secure::fill_pending`——用 `read`（而非 `read_exact`）
累积字节，`WouldBlock/TimedOut` 只返回「本轮未凑齐」，缓冲原样保留，凑齐才解析。明文与加密两条路径共用。
另外把静默死亡变成可见：隧道泵出错打日志，`/health.data_plane` 新增 `tunnels_alive`
（`tunnels` 只是登记条数，泵死了它照样是 1）。

回归测试：
- `relay::tests::frame_split_across_read_timeout_is_resumed_not_lost`（真 TCP，故意把一帧切成 37 B + 余下，中间停 150 ms）
- `secure::tests::fill_pending_keeps_partial_bytes_across_timeouts`
- `secure::tests::fill_pending_reports_closed_link_as_error`

诊断脚本（保留在目录里，w59 的结论靠它们定位）：
- `probe_tunnel.py`：绕开 librqbit，直接 SOCKS5 + BT 握手，确认隧道通不通
- `probe_bt.py`：真的发 request、收满 4 个分片、重组算 md5 —— 用来把「中继坏了」和「客户端没消费」分开

## 这组证据能说什么 / 不能说什么

**能说**：

1. 两个**公网出口不同、都没有入向映射**的主机，能经**公网 TCP 中继**完成一次真实 BT 取片，逐字节一致。
2. 字节确实走的是中继：持有者只经隧道对外供片（`holder_data.bytes_from_engine`），
   中继侧单轮 `data_bytes_admitted` 增量与载荷吻合，且**两源 IP 同时在线**有采样佐证。
3. 直连在本轮不可用（本地候选 1204 ms 无应答），系统**没有回落直连**，而是中继接管并成功。
4. v2.4.92 在该场景下是坏的，坏在哪、怎么坏的、修完怎么验证的，都有可复现的中间证据。

**不能说**：

- **不是 NAT 可达性证据**：222 在 `192.168.1.0/24` 上还有一个管理面别名 `192.168.1.222`，
  两台机器之间存在 L2 通路。「跨 NAT 不可直连」靠的是**通告地址**（`192.168.34.222`、`203.0.113.31`）
  从 A 不可达，而不是物理隔离。`nat_type=unknown`、`s6_status=not_run`、`g2_claim=false`。
- **不证明打洞**：本轮 STUN 没产出映射（`nat_type=unknown`、`public_udp_egress_ok=false`），
  所以没有对「对端公网映射地址」发起过打洞；只能说「本轮直连不可用 → 中继接管」。
- **控制面走 LAN**：advertise 与种子文件经 `192.168.1.222:17900` 取，只有 BT 字节流被强制进隧道。
- **不是吞吐结论**：单次 1 MiB、无并发、无长稳；中继默认 512 KiB/s 数据令牌桶。
