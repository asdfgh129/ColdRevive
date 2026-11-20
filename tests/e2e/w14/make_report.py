"""W14 / FR-14 判定：只读已有 health / 状态 / 日志，逐条断言。"""
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


def load(name):
    return json.loads((OUT / name).read_text(encoding="utf-8"))


sample = (RUN / "sample_idle.bin").read_bytes()
expect_md5 = hashlib.md5(sample).hexdigest()
initial = load("health_initial.json")
wake = load("health_after_wake.json")
slept = load("health_after_sleep.json")
found = load("find_after_sleep.json")
state = json.loads((RUN / "state-A.json").read_text(encoding="utf-8"))
probe = (OUT / "probe_wake.json").read_text(encoding="utf-8")
log = (RUN / "holder.log").read_text(encoding="utf-8", errors="replace")

check("idle_opt_in_enabled", initial["idle"]["enabled"] is True
      and initial["idle"]["threshold_secs"] == 2, initial["idle"])
check("starts_cold", initial["seed_active"] == 0 and initial["idle"]["sleeps"] == 0,
      f"seed_active={initial['seed_active']}")
check("real_handshake_woke_seed", wake["guard"]["woken"] > 0 and wake["seed_active"] >= 1,
      f"woken={wake['guard']['woken']} active={wake['seed_active']}")
check("piece_md5_matches", f"md5={expect_md5}" in probe and "FAIL" not in probe,
      f"md5={expect_md5}")
check("no_early_sleep", wake["idle"]["sleeps"] == 0, str(wake["idle"]))

idle = slept["idle"]
check("auto_slept_after_threshold", idle["sleeps"] == 1 and slept["seed_active"] == 0,
      f"sleeps={idle['sleeps']} active={slept['seed_active']}")
check("idle_clock_reached_threshold", idle["idle_secs"] >= 2,
      f"idle_secs={idle['idle_secs']}")
check("slept_task_is_reported", idle["last_slept"] and idle["last_slept"][0] >= 1,
      str(idle["last_slept"]))

entry = state["entries"][0]
check("shadow_index_kept_but_cold", entry["state"] == "COLD"
      and entry["transitions"][-2:] == ["HOT", "COLD"] and len(state["entries"]) == 1,
      f"state={entry['state']} transitions={entry['transitions']}")
check("published_keys_survive_sleep", len(entry["published_keys"]) >= 1
      and entry["kad_published"] is True,
      f"keys={len(entry['published_keys'])}")
check("kad_find_still_hits", found["result"]["count"] >= 1
      and found["result"]["hit_node"] == "A", found)
check("daemon_log_confirms_sleep", "FR-14 空闲 2s" in log and "影子索引 1 条保留" in log,
      "守护进程日志有真实休眠动作与索引条数")

passed = sum(1 for _, ok, _ in checks if ok)
report = {
    "schema": "coldrevive.w14.fr14/v1",
    "scope": "空闲 N 秒自动 HOT→COLD，影子索引与 KAD 发布 Key 保留；"
             "用真实 BT 握手唤醒 + 取片证明休眠前确有活动",
    "checks": [{"name": n, "ok": ok, "detail": d} for n, ok, d in checks],
    "passed": passed,
    "total": len(checks),
    "boundary": "本机回环、单样本、threshold=2s（分钟配置亦支持）；"
                "不宣称公网 KAD 发布、不宣称 daemon 多任务长稳；"
                "g2_claim=false、s6_status=not_run 不变",
}
(OUT / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
for n, ok, d in checks:
    print(f"[{'OK' if ok else 'XX'}] {n}: {d}")
print(f"passed={passed}/{len(checks)}")
raise SystemExit(0 if passed == len(checks) else 1)
