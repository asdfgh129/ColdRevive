# W60 — 真 STUN 分类下的「打洞失败 → 中继接管」完整链路

> 日期：2026-11-19 · 版本：v2.4.93（无需改代码，本轮是**补跑 W59 漏掉的 `nat probe`**）
>
> `g2_claim=false`、`s6_status=not_run`、`evidence_valid_for_pass=false` 不变。

## 与 W59 的区别

W59 证明了公网中继数据面可用，但它的 `nat_type=unknown` 是**报告初始值**——
`w59-run.sh` 从来没调用过 `nat probe`，所以「STUN 未产出映射」不是实测结论。
W60 补上这一步，并把直连候选**真的打了一次**：

1. 两端对各跑一次 `nat probe dedicated 3000`（STUN `82.157.68.159:3478/3479`）；
2. 把对端通告喂进 `nat peer / nat plan`；
3. 用**验收量具旁路** `nat wake <file_hash> <infohash> stun-mapped` 强制打洞一次；
4. 用生产默认路径 `nat wake <file_hash> <infohash>` 观察自动接管；
5. 跑 1 MiB BT 取片并校验 md5。

## 实测拓扑与结论

| 端 | 出口 | STUN 映射（两次目的地址） | 分类 |
| :--- | :--- | :--- | :--- |
| A（本机 Windows） | `112.5.60.4` | `38576` / `38576`（一致） | `port-restricted-cone` |
| 222（真 NAT） | `112.5.32.34` | `2053` / `2054`（不同） | `symmetric` |

**强制打洞（量具旁路）**：A 用本侧映射 `112.5.60.4:38579`（打洞另绑一个 socket，
故端口与 probe 的 `38576` 不同）向对端
`112.5.32.34:2053` 发了 2 个 punch 包，并经中继让 222 回打了 2 帧；
`punch_sent=2 acked=false peer_punched=false`，真实超时 1203 ms（os error 10060），
**失败**。这是机制性失败：symmetric NAT 到 A 的映射端口不可预知，
STUN 观测到的 `2053` 只是「到 STUN 服务器」的端口。

**生产自动路径**：中继查询报告 `peer=relay-only`（222 探测后如实自报），
`skip_direct=true`，直接选 `relay`；`ack_status=OK`，OPEN/OPENED 握手完成。

**取片**：`complete`、1 048 576 B、两端 md5 `b0e93e82…559e`；
持有者 `→引擎 1 264 B / ←引擎 1 049 585 B`、`splices_done=1`；
下载方 `bytes_relayed=1 050 849`（=两者之和）、`tunnels_alive=1`。

## 边界（不得放宽）

- 强制直连是**验收量具的显式旁路**（`wake` 第三参数），生产自动路径按对端
  `relay-only` 声明**跳过**打洞；两者合起来说明「直连确实不可用，生产没有依赖它」。
- 控制面（advertise / 种子）经 `192.168.1.222:17900` 走 LAN，只有 BT 字节流进公网中继。
- 222 另有 `192.168.1.222` 管理面别名，A 与 222 存在 L2 通路 → **不是 NAT 可达性证据**。
- 单次 1 MiB、无并发/吞吐/长稳结论；`s6_status=not_run`、`g2_claim=false`。

## 复现

```bash
# 前置：W59 的两个守护进程（配置与二进制相同），均订阅 82.157.68.159:7100
curl -s -X POST -d '{"command":"nat","args":["probe","dedicated","3000"]}' http://127.0.0.1:17800/cli
curl -s -X POST -d '{"command":"nat","args":["probe","dedicated","3000"]}' http://192.168.1.222:17900/cli
sh tests/e2e/w60/w60-run.sh <infohash> <file_hash>
python tests/e2e/w60/make_report.py     # 18 条断言，失败即非零退出
```

`reports/` 下保留原始 probe / wake / health / md5 / 两端日志；
`report.json` 由脚本从这些原始产物生成。
