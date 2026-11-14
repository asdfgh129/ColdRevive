# T2 — M2 跨 NAT 复活路径接线验收（本地预检）

> **性质**：本地接线验收，**不是 S6 / G2 证据**。
> `evidence_valid=false`、`s6_status=not_run`、`g2_claim=false`。
> 四类真实 NAT（含 CGNAT）可达率矩阵仍须 M2 在多个真实 NAT 环境逐类实测后由实验执行人手工回填。
> **W3.4 起中继回退改用真实 `coldrevive-relay` 二进制**（纯帧路由）+ `coldrevived` 持有者订阅线程；
> `--self-test` 仍用纯 wire mock 验证帧编解码。**中继是真实组件，但本机没有真实 NAT**，
> 因此本套件依然不构成 S6 证据。
> **编号消歧**：本目录的 `T2` 是 M2/W3.3 接线套件代号；[TEST §2](../../../docs/05-quality/TEST.md)
> 的规划编号「T2 = 公网 ≥8 台不同网络节点」是另一回事、且尚未落地，二者不得混用。

## 1. 目的

G1 于 2026-11-14 通过后 M2 门禁解除。本套件验证 `cr-nat` 是否真正接入
`coldrevived` 的 M1 复活路径，关闭 `spikes.json → t1_progress.design_gaps[0]`
的**接线层**缺口：

| 能力 | 实现位置 | 本套件验证 |
| :--- | :--- | :--- |
| STUN 探测复用唤醒监听 socket | `coldrevived/src/nat_path.rs::probe()` + `main.rs::UdpHub` | T2-01 |
| 端点通告 / 候选计划 | `cr-nat/src/endpoint.rs` | T2-02 / 03 / 04 |
| 单 socket UDP 打洞 + 唤醒 | `cr-nat/src/holepunch.rs` | T2-05 |
| 中继（LowID）回退 | `cr-nat::relay_server` + `coldrevive-relay` 二进制 + `coldrevived/src/relay_sub.rs` | T2-07 |
| tracker 私网/公网 peer 地址协商 | `bt.rs::tracker_response_optional` + `/announce` | T2-08 |
| M1 既有命令不回归 | `wake` / `find` 经 `active_plan` 走候选计划 | T2-06 |
| 失败如实上报 | 无候选时不合成应答 | T2-09 |

## 2. 运行

```bash
cargo build --release --workspace --offline
python3 tests/e2e/t2/t2_nat_path.py --self-test      # mock wire 自检（不需二进制）
python3 tests/e2e/t2/t2_nat_path.py --mode local     # 完整 10 条用例
```

环境变量 `COLDREVIVE_DAEMON` / `COLDREVIVE_CLI` / `COLDREVIVE_RELAY` 可覆盖二进制路径，
缺省使用 `target/release/`。`--mode local` 会拉起真实的 `coldrevive-relay` 子进程
（`--listen 127.0.0.1:0 --stats-file .run/relay-stats.json`），端口由 stats 文件回读，
A/B 守护进程在 `nat.relay` 存在时自动启动持有者订阅线程。
报告写到 `tests/e2e/t2/reports/`（gitignore）。
`--keep` 保留 `.run/`（运行时配置、样本、守护进程与中继日志、stats）便于排查。

## 3. 环境口径

| 项 | 值 |
| :--- | :--- |
| 双机 | ❌ 同一台主机，A/B 两个守护进程（端口 18800-18802 / 18900-18902） |
| 监听 | `0.0.0.0`（使回环别名 127.0.0.2 / 127.0.0.3 可达） |
| STUN | 本机 mock RFC 5389 服务器；按源端口把 A/B 唤醒端口映射到 127.0.0.2 / 127.0.0.3 |
| 中继 | ✅ **真实 `coldrevive-relay` 二进制**：只做 TCP 帧路由（REGISTER→ACK / RELAY→DELIVERED），不解析唤醒 JSON、不碰 UDP；`--stats-file` 输出真实 `connections/registered/delivered/dropped/rejected` |
| 持有者订阅 | ✅ `coldrevived/src/relay_sub.rs`：注册稳定 node id，把 `DELIVERED` 负载经**真实 UDP** 送进本守护进程唤醒端口，再把**真实 WAKEUP_ACK** 用 `RELAY` 回投给来源会话 id |
| NAT | ❌ 无真实 NAT；mock STUN 只模拟「外部地址 ≠ 私网地址」这一点 |

所有观测来自真实 CLI / HTTP / UDP / TCP / 文件与真实中继进程；mock 只提供 STUN 地址映射，
**不合成任何业务应答**。WAKEUP_ACK 始终由真实守护进程的 `handle_udp_frame` 产生，
经订阅线程的真实 UDP 往返取回；中继服务端 `dropped > 0` 或 `delivered < 2` 即判 FAIL。

## 4. 用例（10 条）

| ID | 判据 |
| :--- | :--- |
| T2-00 | A/B 守护进程真实存活（`/health` 200） |
| T2-01 | `nat probe listener` 的 `stun_socket=wake-listener`，映射端口 = 实际唤醒端口；`s6_status=not_run` |
| T2-02 | A 端点通告只含真实证据，无伪造公网 BT 映射（`bt_external=null`） |
| T2-03 | B 经真实文件取回 A 的端点通告 |
| T2-04 | 候选顺序 = 私网 → STUN 映射 → 中继兜底 |
| T2-05 | 强制走 `stun-mapped`：单 socket 打洞（真实 PUNCH_ACK）+ 真实 `WAKEUP_ACK` |
| T2-06 | M1 `wake ack` / `wake probe` / `find` 在 nat 打开时仍满足 PROTO v1 契约 |
| T2-07 | 强制走 `relay`：真实中继服务端 `delivered=2 / dropped=0` + 持有者订阅的真实 UDP 往返 + A 的真实 `WAKEUP_ACK` |
| T2-08 | 私网请求方收到私网 peer；公网请求方在没有公网 BT 映射时收到**空 peer 列表**（不泄漏 RFC1918） |
| T2-09 | 无匹配候选时如实报错，不合成成功应答 |

## 5. 配置契约

`t2_config.json` 是套件元数据；运行时由 harness 生成 `.run/t2-runtime.json` 供
`coldrevived` 与 `coldrevive-cli` 读取。`cr-core::Config` 的 `nat` 段为**可选**：

```json
"nat": {
  "enabled": true,
  "stun_servers": ["127.0.0.1:<mock-port>"],
  "gateway": "<可选，启用 NAT-PMP/PCP 探测>",
  "upnp_location": "<可选>",
  "relay": "127.0.0.1:<coldrevive-relay 实际端口>",
  "bt_external": "<可选，运维侧真实创建的公网 BT/TCP 映射 IP:端口>"
}
```

`nat` 段缺省时 `coldrevived` 的 T0/T1 行为逐字节不变（已由 T0 real 21/21 回归证明）。
`bt_external` 未配置且请求方为公网 IP 时，`/announce` 返回空 peer 列表；
该配置项的**显式公网映射路径**只做 Rust 单元级验证（`cr-nat::endpoint` /
`coldrevived::nat_path` 单测），本套件不伪造公网映射来凑 PASS。

## 6. CLI 契约（新增，不影响 v1）

```
coldrevive-cli --json --node <A|B> nat probe [listener|dedicated] [超时毫秒]
coldrevive-cli --json --node <A|B> nat advertise
coldrevive-cli --json --node <A|B> nat peer <url|文件>
coldrevive-cli --json --node <A|B> nat plan
coldrevive-cli --json --node <A|B> nat wake <file_hash> <infohash> [候选 kind]
coldrevive-cli --json --node <A|B> nat tracker <请求方 IP>
coldrevive-cli --json --node <A|B> nat status
```

`NAT_PUNCH` / `NAT_PUNCH_ACK` 是新增控制面帧，**不属于 PROTO v1 golden**；
`PROTOCOL_VERSION` 保持 1，T0 报文契约未改。

独立部署入口（W3.4 新增，不属于 CLI v1）：

```
coldrevive-relay --listen <ip:port> [--stats-file <path>] [--max-frame <bytes>]
```

- `--listen` 端口为 `0` 时由内核分配，首行 `RELAY_LISTEN=<地址>` 回显；
- `--stats-file` 每 200 ms 落一份机读计数，供验收 harness 读真实投递数；
- `--max-frame` 限制单帧负载（默认 1 MiB），超限直接断开，防内存耗尽。

## 7. 已知边界（M2 后续）

1. **S6**：四类 NAT（Full/Restricted/Port-restricted/Symmetric）+ CGNAT 的真实可达率矩阵未测；
   R-04 ≥70% 阈值未判定。T1-08 与 S6 pilot 都是同一出口的单样本。
2. **BT/TCP 穿透**：tracker 只使用 `internal_port == bt_port` 的真实显式端口映射；
   STUN 学到的 UDP 映射不用于 BT/TCP，TCP 打洞未实现。
3. ~~**中继服务端**~~ → **帧路由核心 + 持有者订阅已落地（W3.4）**。仍缺生产化：
   公网部署、TLS / 身份鉴权（当前 REGISTER 只校验负载回显，无凭证）、多实例注册同步、
   连接限流与空闲超时、以及跨公网中继时延量测。本套件全部在本机回环完成。
4. **对称 NAT 预测**：`build_plan` 在 Symmetric / Cgnat 下剔除 STUN 预测映射并要求中继优先，
   但该策略尚未在真实对称 NAT 上验证。

## 8. W3.4 中继链路（真实帧流）

```
B: holepunch::relay_wake()
   ├─ REGISTER(一次性会话 id S) ──────────────► coldrevive-relay → ACK
   └─ RELAY(target=A, WAKEUP_REQ) ────────────► 查注册表 A → DELIVERED(node=S, WAKEUP_REQ)
                                                       │
A: relay_sub 订阅线程 ◄────────────────────────────────┘
   ├─ 真实 UDP sendto(127.0.0.1:A.wake_port, WAKEUP_REQ)
   ├─ 真实 UDP recvfrom ← A 守护进程 handle_udp_frame 产生 WAKEUP_ACK
   └─ RELAY(target=S, WAKEUP_ACK) ────────────► 查注册表 S → DELIVERED(node=A, WAKEUP_ACK)
                                                       │
B: relay_wake() 取回真实 WAKEUP_ACK ◄─────────────────┘
```

请求侧用**一次性会话 id** 注册（`relay::ephemeral_session_id`），避免与自身持有者订阅连接
抢占服务端 `HashMap<node_id, conn>` 的唯一表项；服务端把该 id 作为 `DELIVERED.node` 交给
持有者，持有者据此把应答准确回投到本次请求。目标未注册时服务端只计 `dropped`、
**不伪造任何应答帧**，请求侧按自身超时如实失败（见 `cr-nat::relay_server` 单测）。
