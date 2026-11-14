# T2 — M2 跨 NAT 复活路径接线验收（本地预检）

> **性质**：本地接线验收，**不是 S6 / G2 证据**。
> `evidence_valid=false`、`s6_status=not_run`、`g2_claim=false`。
> 四类真实 NAT（含 CGNAT）可达率矩阵仍须 M2 在多个真实 NAT 环境逐类实测后由实验执行人手工回填。
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
| 中继（LowID）回退 | `cr-nat/src/relay.rs` + `holepunch::relay_wake` | T2-07 |
| tracker 私网/公网 peer 地址协商 | `bt.rs::tracker_response_optional` + `/announce` | T2-08 |
| M1 既有命令不回归 | `wake` / `find` 经 `active_plan` 走候选计划 | T2-06 |
| 失败如实上报 | 无候选时不合成应答 | T2-09 |

## 2. 运行

```bash
cargo build --release --workspace --offline
python3 tests/e2e/t2/t2_nat_path.py --self-test      # mock wire 自检（不需二进制）
python3 tests/e2e/t2/t2_nat_path.py --mode local     # 完整 10 条用例
```

环境变量 `COLDREVIVE_DAEMON` / `COLDREVIVE_CLI` 可覆盖二进制路径，
缺省使用 `target/release/`。报告写到 `tests/e2e/t2/reports/`（gitignore）。
`--keep` 保留 `.run/`（运行时配置、样本、守护进程日志）便于排查。

## 3. 环境口径

| 项 | 值 |
| :--- | :--- |
| 双机 | ❌ 同一台主机，A/B 两个守护进程（端口 18800-18802 / 18900-18902） |
| 监听 | `0.0.0.0`（使回环别名 127.0.0.2 / 127.0.0.3 可达） |
| STUN | 本机 mock RFC 5389 服务器；按源端口把 A/B 唤醒端口映射到 127.0.0.2 / 127.0.0.3 |
| 中继 | 本机 mock TCP 帧中继桥，把 RELAY 负载经**真实 UDP** 转发到 A 的真实唤醒端口，再把 A 的**真实 ACK** 封装回 DELIVERED |
| NAT | ❌ 无真实 NAT；mock 只模拟「外部地址 ≠ 私网地址」这一点 |

所有观测来自真实 CLI / HTTP / UDP / TCP / 文件；mock 只提供地址映射与转发，
**不合成任何业务应答**。WAKEUP_ACK 始终由真实守护进程产生。

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
| T2-07 | 强制走 `relay`：真实 TCP 帧 → 真实 UDP 转发 → A 的真实 ACK |
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
  "relay": "127.0.0.1:<mock-relay-port>",
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

## 7. 已知边界（M2 后续）

1. **S6**：四类 NAT（Full/Restricted/Port-restricted/Symmetric）+ CGNAT 的真实可达率矩阵未测；
   R-04 ≥70% 阈值未判定。T1-08 与 S6 pilot 都是同一出口的单样本。
2. **BT/TCP 穿透**：tracker 只使用 `internal_port == bt_port` 的真实显式端口映射；
   STUN 学到的 UDP 映射不用于 BT/TCP，TCP 打洞未实现。
3. **中继服务端**：`cr-nat::relay` 只有客户端侧帧协议，生产中继服务与持有者侧订阅线程尚未实现；
   T2 用的是本地 mock 桥。
4. **对称 NAT 预测**：`build_plan` 在 Symmetric / Cgnat 下剔除 STUN 预测映射并要求中继优先，
   但该策略尚未在真实对称 NAT 上验证。
