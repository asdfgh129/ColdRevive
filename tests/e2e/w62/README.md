# W62 — FR-11 握手拦截：旧客户端直连 bt_port 触发被动唤醒（opt-in）

> 2026-11-20 · `make_report.py` **11/11** · 口径：`g2_claim=false`、`s6_status=not_run` 不变。

## 证明什么

SRS FR-11：旧客户端**不做私有 UDP 唤醒**、直接连入 bt_port 发标准 BT 握手，
持有者按握手里的 infohash 唤醒对应样本。librqbit 没有握手钩子（ADR-002），
所以实现是 `cr-guard` 前置代理，**opt-in（`engine.guard_handshake=true`）**。

## 机制

1. 引擎监听退到内核分配的回环内部口；对外 `bt_port` 由 guard 代理占用。
2. 每条入站连接：`read_handshake`（68 B，5 s 超时）→ 解析 infohash
   → `sched::wake_seed`（唯一合法 activate 入口）→ `splice_prefaced`
   把已读 68 B 补回、双向搬运给引擎。
3. 未知/非标准握手不唤醒；裸 connect（T0-07 探测）超时退出，不留线程。

**关键坑**：读走握手后必须补回。直接复用 `cr_bridge::splice_to_listener`
会让引擎收到截断握手 → 唤醒看着成功、数据面 0 字节（ADR-021 同类）。

## 证据（tests/e2e/w62/reports/report.json）

- 两个内容不同的 256 KiB 样本 A/B，均以 paused 入库（`seed_tasks=2`、`seed_active=0`）。
- 向 **sample_b** 的 infohash 发裸握手：`guard.woken` 0→1，**只 B 转 HOT**、A 仍 COLD；
  BT `interested`/`unchoke`/`request` 取满 262144 B，md5 `c775e178…3ad8` 逐字节一致。
- 负例：未知 infohash 握手 → `guard.missed=1`、`woken` 不变、两样本状态不变、引擎回 0 B。
- 向 **sample_a** 握手：`woken` 1→2、`seed_active=2`，取满 262144 B md5 `a5fd8558…2bd8` 一致。
- `/health.guard = {enabled:true, handshakes:3, woken:2, missed:1}` 与三次握手对账。

## 边界（照写）

- 只有**本机回环**一次样本；没有公网直连、没有 NAT、没有第三方客户端实测。
- **只宣称 TCP 握手拦截**：µTP 走内部回环口，对外不可达，不得读成「µTP 可达」。
- 默认关闭；W5.1d/f 的 µTP 证据与 T0/T1 行为不受影响（默认路径字节级不变）。
- 这不是 NAT 可达性证据，不改 `g2_claim` / `s6_status`。

## 复现

```bash
# 1) 造样本（幂等）
python -c "from pathlib import Path; r=Path('tests/e2e/w62/run'); r.mkdir(parents=True,exist_ok=True); (r/'sample_a.bin').write_bytes(bytes((i*7+3)%256 for i in range(262144))); (r/'sample_b.bin').write_bytes(bytes((i*13+11)%256 for i in range(262144)))"

# 2) 起持有者（18800 HTTP / 18801 UDP / 18802 BT-guard）
./target/release/coldrevived.exe --config tests/e2e/w62/holder.json --node-id A

# 3) 从 state-A.json 读 infohash，分别取两个样本的 md5，跑探针
python tests/e2e/w62/probe_guard.py <infohash-b> $(md5sum tests/e2e/w62/run/sample_b.bin | cut -d' ' -f1)
python tests/e2e/w62/probe_guard.py <infohash-a> $(md5sum tests/e2e/w62/run/sample_a.bin | cut -d' ' -f1)

# 4) 判定
curl -s http://127.0.0.1:18800/health > tests/e2e/w62/reports/health_final.json
python tests/e2e/w62/make_report.py
```
