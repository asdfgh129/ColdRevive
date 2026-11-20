# W14 — FR-14 空闲自动休眠：HOT→COLD 且影子索引仍可检索

> 2026-11-20 · `make_report.py` **12/12** · 口径：`g2_claim=false`、`s6_status=not_run` 不变。

## 证明什么

SRS FR-14：空闲 N 分钟自动切出休眠、保留影子索引，**休眠后 KAD 仍可查到该节点**。
实现是 opt-in：`engine.idle_sleep_secs`（秒；也接受 `engine.idle_sleep_minutes`
或 `scheduler.idle_sleep_minutes`）。计时只由**真实成功的唤醒**刷新，
`/health` 轮询不算活动。默认关闭，老配置行为不变。

## 机制

1. `wake_seed` 成功（私有 UDP 或 FR-11 握手）才刷新 `last_activity`。
2. 后台线程每 `min(threshold, 5)` 秒检查；到阈值且调度器非空时，
   只经 `idle::sleep_hot_seeds` 批量暂停任务。
3. 引擎是事实来源：调度器有记录但引擎已停只清记账，不算一次休眠；
   pause 失败保留 HOT 与错误，不留「假装成功」的状态。
4. 成功后把对应 `ShadowIndex` 条目翻回 `COLD` 并 `save()`；
   条目和 `published_keys` 都不删除，所以本地 KAD 查询仍命中。

## 证据（tests/e2e/w14/reports/report.json）

- 64 KiB 样本以 paused 入库，`seed_tasks=1 / seed_active=0`，`idle.enabled=true`、阈值 2 s。
- 标准 BT 握手真实唤醒：`guard.woken` 0→1，取满 65536 B，md5 `a0099d2a6de6…` 一致；
  此刻仍 `idle.sleeps=0`。
- 等待 2 s：`seed_active` 1→0、`idle.sleeps=1`、`last_slept=[1]`。
- `state-A.json`：`state=COLD`、`transitions=["COLD","WAKING","HOT","COLD"]`、
  4 个 `published_keys` 全保留。
- 休眠后 `coldrevive-cli … find w14` 返回 `hit_node=A / count=1`。
- `/health.idle` 与守护进程日志对账：日志明确打印「空闲 2s：1 个任务切回 COLD」。

## 边界（照写）

- 只测本机回环、单样本、2 秒阈值（配置也支持分钟）；不是公网 KAD 发布证据。
- 休眠暂停的是**引擎做种任务**，不删文件、不删伴随种子、不撤 KAD Key。
- 自动休眠默认关闭；FR-11 guard 在本测试里只用来产生可验证的真实唤醒活动。
- 不宣称多任务长稳、不宣称 `s6_status` 或 G2 有任何变化。

## 复现

```bash
# 1) 造样本（幂等）
python -c "from pathlib import Path; r=Path('tests/e2e/w14/run'); (r/'golden').mkdir(parents=True,exist_ok=True); (r/'sample_idle.bin').write_bytes(bytes((i*11+5)%256 for i in range(65536))); (r/'golden/req.json').write_text('{\"type\":\"WAKEUP_REQ\"}'); (r/'golden/ack.json').write_text('{\"type\":\"WAKEUP_ACK\"}')"

# 2) 起持有者（18600 HTTP / 18601 UDP / 18602 BT-guard）
./target/release/coldrevived.exe --config tests/e2e/w14/holder.json --node-id A

# 3) 唤醒 + 取片 + 等休眠 + KAD 查询
IH=$(python tests/e2e/w14/wait_ready.py | grep '^INFOHASH=' | cut -d= -f2)
python tests/e2e/w14/probe_idle.py "$IH" "$(md5sum tests/e2e/w14/run/sample_idle.bin | cut -d' ' -f1)" | tee tests/e2e/w14/reports/probe_wake.json
curl -s http://127.0.0.1:18600/health > tests/e2e/w14/reports/health_after_wake.json
python tests/e2e/w14/wait_sleep.py
COLDREVIVE_T0_CONFIG=tests/e2e/w14/holder.json ./target/release/coldrevive-cli.exe --json --node A find w14 > tests/e2e/w14/reports/find_after_sleep.json

# 4) 判定
python tests/e2e/w14/make_report.py
```
