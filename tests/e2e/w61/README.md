# W61 — 在 TCP 被 RST 的公网主机上跑通 UDP 数据面（双 NAT，1 MiB）

> 日期：2026-11-20 · 版本：v2.4.95（基于 v2.4.94 二进制 + holder 看板标签修正）
>
> `g2_claim=false`、`s6_status=not_run` 不变。本轮**不**宣称 NAT 可达性、
> **不**宣称拥塞/公平性，只补 ADR-024 阶段 3 的第三条证据。

## 为什么这一轮才算闭环

v2.4.94 的环回证据只能说「UDP 承载本身能跑」；ADR-024 明确要求：
**只有在确有 TCP 入向被 RST 的部署上复现成功，才宣称覆盖 ADR-020 场景**。
W61 在同一台 `119.29.150.254` 上同时量两件事：

1. 这台机器的非 22 入向 TCP 在数据阶段仍被注入伪造 RST（纯回显服务也会中招）；
2. 同一台机器的 `7101/UDP` 中继承载了 A ↔ 222 的 1 MiB BT 取片，字节一致。

## 拓扑与口径

| 角色 | 地址 | NAT / 说明 |
| :--- | :--- | :--- |
| A（下载端） | `192.168.1.210`，出口 `112.5.60.4` | STUN 实测 `port-restricted-cone` |
| 222（持有者） | `192.168.34.222`，出口 `112.5.32.34` | STUN 实测 `symmetric`；另有 `192.168.1.222` 管理口（仅控制面） |
| 中继/STUN | `119.29.150.254` | TCP `7100` 非 22 入向被 RST；UDP `7101` 可用 |
| 控制面 | `http://192.168.1.222:17900` | 走 LAN；L2 抓包证明 BT 字节没走近路 |

两端配置见 `holder.json` / `member.json`：`nat.data_plane_carrier="udp"`（opt-in），
`nat.relay_udp="119.29.150.254:7101"`，**没有**配 TCP `relay`。
默认 `tcp` 行为不受影响。

## 结果（`reports/report.json`，12/12）

| 检查 | 结果 |
| :--- | :--- |
| TCP 对照 | 12 轮里 11 轮 `ConnectionResetError`；首次 run1 是 0/12。一轮偶发成功**不**推翻结论 |
| 双 NAT 分类 | A `port-restricted-cone` / 222 `symmetric`（同轮 `nat probe`） |
| 唤醒 | `path=relay-udp`、`ack_status=OK`、备注 `OPEN/OPENED 双向握手完成` |
| 中途看板 | `data_plane.carrier=relay-udp`、`udp_tunnels_alive=1`、`holder_data.carrier=relay-sub-udp` |
| 取片 | `complete`、`1048576 B`、两端 md5 `b0e93e825f86000e5341cf4aecb0559e` |
| 持有者 | `opens=1`、`splices_done=1`、`→引擎 1287 B / ←引擎 1049585 B` |
| 下载端 | `bytes_relayed=1050872` |
| 中继 Δ | `data_in=4219`、`deliver_out=4214`、`status_out=4219`、`data_bytes_in=2623138`、`bandwidth_limited=0`、`replay=0`、`mac_fail=0` |
| L2 | 222 `ens192` 捕获 50 包，全部是 17900 控制面；BT 端口 17902 **0 包** |

`run1/` 保留第一次（标签修正前）完整原始产物；顶层 `reports/` 是最终一组。

## 公平性量具 v0（只量转发层）

`cr-nat` 里新增 `#[ignore]` 量具 `udp_relay_data_fairness_probe`：
4 条 UDP DATA 流争抢同一个 64 KiB/s 全局令牌桶，跑 3 s，只打印 JSON、不自动判 pass。
复现：`cargo test --offline -p cr-nat udp_relay_data_fairness_probe -- --ignored --nocapture`。
一次样本见 `fairness-probe.json`：份额 `25.5% / 21.4% / 21.4% / 31.6%`，
`deliver_out=196`、`bandwidth_limited=44259`。

**这不是拥塞控制结论**：量的是中继 DATA 转发公平性，可靠层本身仍没有
拥塞/退避语义，不能据此宣称「跨公网公平性达标」。

## 本轮抓到并修掉的看板 bug

`/health.holder_data.carrier` 原先把计数器写死标成 `relay-sub-tcp`。UDP 承载实际搬完
1 MiB 后，看板仍指向 TCP 那条路——排障会去查错的那条链路。
现按 `nat.data_plane_carrier` 标 `relay-sub-udp`，单测
`holder_carrier_label_follows_opt_in_carrier` 钉住。

## 边界（不得放宽）

- 控制面走 LAN，只有 BT 字节走公网 UDP 中继；A 与 222 有 L2 通路，
  所以这**不是** NAT 可达性证据，也不是「打洞成功」。
- 单次 1 MiB、约 32 s；无并发/吞吐/长稳/公平性结论（可靠层窗口固定 `WINDOW=32`）。
- 状态串 `relay_data_plane_udp="udp-carrier-implemented-rst-path-verified"`：
  `path` 只指这台主机这条路径，不外推成所有 RST 部署。
- `g2_claim=false`、`s6_status=not_run`。

## 复现

```bash
# 1) 部署 v2.4.95 的 coldrevived 到 A/222，v2.4.94+ 的 relay 到 119.29.150.254（PSK 同一把）
# 2) 在 RST 主机起纯 TCP 回显并抓包，A 侧多轮探测（会看到 11/12 左右被重置）
ssh root@119.29.150.254 'python3 /root/w61/echo_7100.py 7100 &'
python tests/e2e/w61/tcp_rst_probe.py 119.29.150.254 7100 12

# 3) 稳态：/root/vantage.sh（3478/3479 + 7101），两端分别启动冷态守护进程
/usr/local/bin/coldrevived-w61 --config /root/cr-w61/holder.json --node-id 222
./target/release/coldrevived.exe --config tests/e2e/w61/member.json --node-id A

# 4) 控制链 + 取片（infohash/file hash 见 reports/wake.json 与 holder state）
curl -s http://192.168.1.222:17900/nat/advertise
curl -s -X POST -d '{"command":"nat","args":["peer","http://192.168.1.222:17900/nat/advertise"]}' http://127.0.0.1:17800/cli
curl -s -X POST -d '{"command":"nat","args":["plan"]}' http://127.0.0.1:17800/cli
curl -s -X POST -d '{"command":"nat","args":["wake","<file_hash>","<infohash>"]}' http://127.0.0.1:17800/cli
curl -s -X POST -d '{"jsonrpc":"2.0","id":"1","method":"aria2.addUri","params":[["http://192.168.1.222:17900/torrent/<infohash>"]]}' http://127.0.0.1:17800/jsonrpc

# 5) 只读原始产物判定（失败即非零退出）
python tests/e2e/w61/make_report.py
```
