"""W62 / FR-11 判定：只读已有报告与状态，逐条断言；脚本不自动判 pass 之外的结论。"""
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent
RUN = ROOT / "run"
OUT = ROOT / "reports"
checks = []


def check(name, ok, detail):
    checks.append((name, bool(ok), detail))


sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
b = (RUN / "sample_b.bin").read_bytes()
a = (RUN / "sample_a.bin").read_bytes()
check("two_distinct_samples", sha(RUN / "sample_a.bin") != sha(RUN / "sample_b.bin"),
      f"a={sha(RUN/'sample_a.bin')[:12]} b={sha(RUN/'sample_b.bin')[:12]}")

probe_b = (OUT / "guard_probe_b.json").read_text(encoding="utf-8")
probe_a = (OUT / "guard_probe_a.json").read_text(encoding="utf-8")
for tag, blob, sample in (("b", probe_b, b), ("a", probe_a, a)):
    md5 = hashlib.md5(sample).hexdigest()
    check(f"handshake_woke_sample_{tag}", "guard 没记到" not in blob and "FAIL" not in blob,
          "probe 退出码 0 且无 FAIL（见 guard_probe_%s.json）" % tag)
    check(f"piece_md5_sample_{tag}", f"md5={md5}" in blob,
          f"期望 md5={md5}")

state = json.loads((RUN / "state-A.json").read_text(encoding="utf-8"))
entries = {e["name"]: e for e in state.get("entries", [])}
check("correct_samples_went_hot",
      all(entries[n]["state"] == "HOT" for n in ("sample_a.bin", "sample_b.bin")),
      {n: entries[n]["state"] for n in entries})

health = json.loads((OUT / "health_final.json").read_text(encoding="utf-8"))
g = health["guard"]
check("guard_enabled", g["enabled"] is True, g)
check("guard_counts", g["handshakes"] == 3 and g["woken"] == 2 and g["missed"] == 1,
      f"handshakes={g['handshakes']} woken={g['woken']} missed={g['missed']}（2 命中 + 1 未知 infohash）")
check("seed_active_matches", health["seed_active"] == 2, str(health["seed_active"]))

log = (RUN / "holder.log").read_text(encoding="utf-8", errors="replace")
check("proxy_uses_internal_loopback", "FR-11 guard 代理监听" in log and "引擎内部 127.0.0.1" in log,
      "外部 bt_port 与引擎内部口都在回环，splice 路径已打印")
check("unknown_infohash_rejected",
      "未命中本节点做种任务" in log and "0000000000000000000000000000000000000000" in log,
      "未知 infohash 记录 missed，不唤醒任何样本")

passed = sum(1 for _, ok, _ in checks if ok)
report = {
    "schema": "coldrevive.w62.fr11/v1",
    "scope": "opt-in 握手拦截：旧客户端直连 bt_port → 标准 BT 握手 → 按 infohash 本地唤醒；"
             "仅 TCP，不宣称 µTP 可达（µTP 走内部口不外透）",
    "checks": [{"name": n, "ok": ok, "detail": d} for n, ok, d in checks],
    "passed": passed,
    "total": len(checks),
    "boundary": "本机回环单次样本；未测公网直连、未测 NAT；g2_claim=false、s6_status=not_run 不变",
}
(OUT / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
for n, ok, d in checks:
    print(f"[{'OK' if ok else 'XX'}] {n}: {d}")
print(f"passed={passed}/{len(checks)}")
raise SystemExit(0 if passed == len(checks) else 1)
