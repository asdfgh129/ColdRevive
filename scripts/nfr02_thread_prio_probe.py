#!/usr/bin/env python3
"""NFR-02 第二分句取证：引擎线程是否真的以 Idle 优先级运行。

判据来自 SRS NFR-02：「造种任务仅以 Idle 优先级运行」。

这个探针只做一件事：**读回**。它不看 `/health` 的自述当成结论，而是直接读
`/proc/<pid>/task/<tid>/stat` 的第 18/19/39 字段（priority / nice / policy）与 `comm`，
逐个线程判定；`/health.idle_priority` 只作为对照（自述 vs 实况）。

角色划分（按线程名，名字是 cr-engine 自己定的，不是 tokio 默认名）：
- `cr-engine-worker`  → **引擎线程**（librqbit 的做种/校验/上传都在这上面）
- `cr-data-plane`     → 唤醒关键路径（UDP 数据面），必须保持默认优先级
- 进程名本身（无自定义名）→ std 线程（guard / HTTP / DHT 等），同样必须默认

用法：
    python3 nfr02_thread_prio_probe.py <pid> <out.json> [health_url] [快照数] [间隔秒]
"""

import json
import os
import subprocess
import sys
import time
import urllib.request

ENGINE_NAME = "cr-engine-wrk"
# Linux 的 TASK_COMM_LEN=16（含 NUL），comm 最多 15 字符，更长的名字会被**静默截断**。
# 曾经用 16 字符的名字 → /proc 里只剩 15 字符 → 按全名比对的探针报「0 个引擎线程」。
TASK_COMM_MAX = 15
WAKE_CRITICAL_PREFIXES = ("cr-data-plane",)
IDLE_NICE = 19
SCHED_OTHER, SCHED_BATCH, SCHED_IDLE = 0, 3, 5
POLICY_NAMES = {0: "SCHED_OTHER", 1: "SCHED_FIFO", 2: "SCHED_RR",
                3: "SCHED_BATCH", 5: "SCHED_IDLE", 6: "SCHED_DEADLINE"}


def read_stat(tid_dir):
    """读一个线程的 stat；comm 可能含空格，必须以最后一个 ')' 为界。"""
    try:
        raw = open(os.path.join(tid_dir, "stat"), "r", encoding="utf-8", errors="replace").read()
    except OSError:
        return None
    cut = raw.rfind(")")
    if cut < 0:
        return None
    f = raw[cut + 1:].split()
    if len(f) < 39:
        return None
    # ')' 之后第一个是 state（第 3 字段）⇒ 字段 n 的下标是 n-3
    return {"priority": int(f[15]), "nice": int(f[16]), "rt_priority": int(f[37]),
            "policy": int(f[38])}


def snapshot(pid):
    """一次快照：进程内每个线程的名字与优先级。"""
    out = []
    base = "/proc/%d/task" % pid
    try:
        tids = sorted(int(t) for t in os.listdir(base))
    except OSError as e:
        raise SystemExit("读不到 %s: %s" % (base, e))
    for tid in tids:
        d = os.path.join(base, str(tid))
        st = read_stat(d)
        if st is None:
            continue  # 线程刚退出
        try:
            comm = open(os.path.join(d, "comm"), encoding="utf-8", errors="replace").read().strip()
        except OSError:
            comm = ""
        out.append({"tid": tid, "comm": comm, **st,
                    "policy_name": POLICY_NAMES.get(st["policy"], "unknown")})
    return out


def classify(thread, pid):
    # 按截断后的可见名比对：comm 是内核截到 15 字符的，配置名更长时不能拿全名比。
    if thread["comm"] == ENGINE_NAME[:TASK_COMM_MAX]:
        return "engine"
    if thread["tid"] == pid:
        return "main"
    if any(thread["comm"].startswith(p) for p in WAKE_CRITICAL_PREFIXES):
        return "wake_critical"
    return "std"


def is_idle(t):
    return t["nice"] == IDLE_NICE or t["policy"] == SCHED_IDLE


def curl_json(url):
    try:
        with urllib.request.urlopen(url, timeout=5) as r:
            return json.load(r)
    except Exception as e:  # 连不上就如实记 None + 原因，不编一个空对象
        return {"_error": "%s: %s" % (type(e).__name__, e)}


def main():
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    pid = int(sys.argv[1])
    out_path = sys.argv[2]
    health_url = sys.argv[3] if len(sys.argv) > 3 else "http://127.0.0.1:17800/health"
    rounds = int(sys.argv[4]) if len(sys.argv) > 4 else 3
    interval = float(sys.argv[5]) if len(sys.argv) > 5 else 3.0

    snaps = []
    for i in range(rounds):
        snaps.append({"at": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "threads": snapshot(pid)})
        if i + 1 < rounds:
            time.sleep(interval)

    health = curl_json(health_url)

    # 稳定性：所有快照里，引擎线程**始终** idle、非引擎线程**始终**默认优先级。
    engine_ok_all, wake_ok_all = True, True
    union = {}
    for s in snaps:
        for t in s["threads"]:
            t["role"] = classify(t, pid)
            union.setdefault(t["tid"], t)
            if t["role"] == "engine" and not is_idle(t):
                engine_ok_all = False
            if t["role"] != "engine" and (t["nice"] != 0 or t["policy"] != SCHED_OTHER):
                wake_ok_all = False

    engine = [t for t in union.values() if t["role"] == "engine"]
    others = [t for t in union.values() if t["role"] != "engine"]

    proc = subprocess.run(["uname", "-r"], capture_output=True, text=True)
    result = {
        "probe": "nfr02_thread_prio_probe.py",
        "pid": pid,
        "kernel_release": proc.stdout.strip(),
        "nproc": os.cpu_count(),
        "rounds": rounds,
        "interval_sec": interval,
        "engine_thread_name": ENGINE_NAME,
        "snapshots": snaps,
        "threads_union": sorted(union.values(), key=lambda t: t["tid"]),
        "summary": {
            "engine_threads": len(engine),
            "engine_nice": sorted({t["nice"] for t in engine}),
            "engine_policy": sorted({t["policy_name"] for t in engine}),
            "engine_all_idle_in_every_snapshot": engine_ok_all,
            "non_engine_threads": len(others),
            "non_engine_all_default_in_every_snapshot": wake_ok_all,
        },
        "health_idle_priority": (health.get("idle_priority")
                                 if isinstance(health, dict) else None),
        "health_pid": (health.get("pid") if isinstance(health, dict) else None),
        "verdict": {
            # 判据：引擎线程存在且全部 idle；唤醒关键路径全部未被降级。
            # 「没找到引擎线程」与「找到了但没降下来」必须分开写——今天正是前者被
            # 误读成后者（线程名被截断）。引擎没起时进程里本就没有这些线程。
            "nfr02_second_clause": (
                "pass" if (engine and engine_ok_all)
                else ("blocked_no_engine_threads" if not engine else "fail")),
            "wake_path_undisturbed": "pass" if wake_ok_all else "fail",
            "notes": [] if engine else [
                "进程内没有名为 %r 的线程：引擎尚未启动，或线程名与约定不符"
                % ENGINE_NAME[:TASK_COMM_MAX]],
        },
        "wording": {
            "ok": "引擎线程（cr-engine-worker）以 Idle 优先级运行（nice 19 + SCHED_IDLE）",
            "forbidden": ["整个进程处于 Idle", "所有线程 Idle", "唤醒路径也降级了"],
        },
    }
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(result, f, ensure_ascii=False, indent=1)
        f.write("\n")
    print(json.dumps(result["summary"], ensure_ascii=False))
    print(json.dumps(result["verdict"], ensure_ascii=False))


if __name__ == "__main__":
    main()
