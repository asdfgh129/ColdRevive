#!/usr/bin/env python3
"""NFR-02 空闲 CPU 采样探针（只读，不改守护进程状态）。

判据（SRS NFR-02 第一分句）：空闲态平均 CPU ≤ 2%。

为什么不用守护进程自带的 `/health` 采样：
`sysres::spawn()` 只在配置了任一 `limits.*` 阈值时才起采样线程（`needed()`），
本环境的配置一条都没配，所以 `/health` 里 `current=null`、`sample_at_secs=0`
——那是「没开采样」，不是「读不到 CPU」。这里从 `/proc/<pid>/stat` 外部读，
既不动配置，也不改变被测进程行为。

口径：
- `mean_pct_one_core` = 进程 CPU 时间增量 / 墙上时间 × 100（单核 = 100%，与 top 一致）。
- `mean_pct_of_host` = 上值 / nproc（整机容量口径），同时给出，避免口径歧义。
- 瞬时百分比按相邻样本差分；间隔 < 0.2 s 的样本丢弃（`CpuClock` 同款约束）。
- 读不到就记 null，绝不写 0。

用法：nfr02_idle_cpu_probe.py <pid> <duration_sec> <out.json> [health_url]
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time

CLK_TCK = os.sysconf("SC_CLK_TCK")


def read_proc_cpu_s(pid: int) -> float | None:
    """进程累计 CPU 秒数（utime+stime）。读不到返回 None。"""
    try:
        with open(f"/proc/{pid}/stat", "r", encoding="utf-8", errors="replace") as fh:
            text = fh.read()
        # comm 可能含空格与括号，必须以最后一个 ')' 为界。
        rest = text[text.rfind(")") + 1:].split()
        # ')' 之后第一个是 state（第 3 字段），utime（第 14）在下标 11。
        return (int(rest[11]) + int(rest[12])) / CLK_TCK
    except (OSError, IndexError, ValueError):
        return None


def read_proc_mem(pid: int) -> dict:
    out: dict = {"rss_kb": None, "threads": None}
    try:
        with open(f"/proc/{pid}/status", "r", encoding="utf-8", errors="replace") as fh:
            for line in fh:
                if line.startswith("VmRSS:"):
                    out["rss_kb"] = int(line.split()[1])
                elif line.startswith("Threads:"):
                    out["threads"] = int(line.split()[1])
    except OSError:
        pass
    return out


def read_sys_cpu() -> tuple[int, int] | None:
    """整机累计 (total, idle+iowait) 的 jiffies，用于机况对照。"""
    try:
        with open("/proc/stat", "r", encoding="utf-8") as fh:
            parts = fh.readline().split()[1:]
        vals = [int(x) for x in parts]
        total = sum(vals)
        idle = vals[3] + (vals[4] if len(vals) > 4 else 0)
        return total, idle
    except (OSError, ValueError, IndexError):
        return None


def curl_health(url: str) -> dict:
    try:
        res = subprocess.run(
            ["curl", "-fsS", "--max-time", "3", url],
            capture_output=True, text=True, timeout=6,
        )
        if res.returncode != 0:
            return {"error": (res.stderr or "").strip()[:200] or f"curl rc={res.returncode}"}
        return json.loads(res.stdout)
    except Exception as exc:  # noqa: BLE001 - 探针不能因健康检查失败而中断
        return {"error": f"{type(exc).__name__}: {exc}"}


def established(port: int) -> int | None:
    """到 bt_port 的 ESTABLISHED 连接数，证明「确实没有对端」。"""
    try:
        res = subprocess.run(
            ["ss", "-Htn", "state", "established", f"( sport = :{port} )"],
            capture_output=True, text=True, timeout=6,
        )
        if res.returncode != 0:
            return None
        return sum(1 for ln in res.stdout.splitlines() if ln.strip())
    except Exception:  # noqa: BLE001
        return None


def pct(values: list[float], q: float) -> float | None:
    if not values:
        return None
    xs = sorted(values)
    idx = min(len(xs) - 1, max(0, int(round(q * (len(xs) - 1)))))
    return xs[idx]


def main() -> int:
    if len(sys.argv) < 4:
        print(__doc__)
        return 2
    pid = int(sys.argv[1])
    duration = int(sys.argv[2])
    out_path = sys.argv[3]
    health_url = sys.argv[4] if len(sys.argv) > 4 else "http://127.0.0.1:17800/health"

    started_at = time.strftime("%Y-%m-%dT%H:%M:%S%z", time.localtime())
    nproc = os.cpu_count() or 1
    load0 = os.getloadavg()
    health0 = curl_health(health_url)
    cpu0 = read_proc_cpu_s(pid)
    sys0 = read_sys_cpu()
    if cpu0 is None:
        print(json.dumps({"error": f"cannot read /proc/{pid}/stat"}, ensure_ascii=False))
        return 1

    samples: list[dict] = []
    health_series: list[dict] = []
    t_start = time.monotonic()
    prev_t, prev_cpu = t_start, cpu0
    next_health = 0.0

    while True:
        now = time.monotonic()
        elapsed = now - t_start
        if elapsed >= duration:
            break
        cpu = read_proc_cpu_s(pid)
        mem = read_proc_mem(pid)
        inst = None
        if cpu is not None:
            dt = now - prev_t
            if dt >= 0.2:
                inst = round((cpu - prev_cpu) / dt * 100.0, 4)
                prev_t, prev_cpu = now, cpu
        samples.append({
            "elapsed_sec": round(elapsed, 3),
            "cpu_s": None if cpu is None else round(cpu, 4),
            "inst_pct_one_core": inst,
            **mem,
        })
        if elapsed >= next_health:
            h = curl_health(health_url)
            health_series.append({
                "elapsed_sec": round(elapsed, 1),
                "status": h.get("status"),
                "seed_tasks": h.get("seed_tasks"),
                "seed_active": h.get("seed_active"),
                "hot": (h.get("limits") or {}).get("hot"),
                "served_bytes": h.get("served_bytes"),
                "punch_attempts": (h.get("punch_route") or {}).get("attempts"),
                "error": h.get("error"),
            })
            next_health = elapsed + 30
        time.sleep(1.5)

    elapsed_total = time.monotonic() - t_start
    cpu1 = read_proc_cpu_s(pid)
    sys1 = read_sys_cpu()
    health1 = curl_health(health_url)

    # 先把原始样本落盘，再算汇总：万一汇总出异常，600 秒的数据不至于全丢。
    with open(out_path + ".partial", "w", encoding="utf-8") as fh:
        json.dump({"pid": pid, "started_at": started_at,
                   "duration_sec_actual": round(elapsed_total, 2),
                   "cpu_start_s": cpu0, "cpu_end_s": cpu1,
                   "samples": samples, "health_series": health_series},
                  fh, ensure_ascii=False)

    def d(a, b):
        return None if (a is None or b is None) else b - a

    cpu_delta = d(cpu0, cpu1)
    mean_one_core = None if cpu_delta is None else round(cpu_delta / elapsed_total * 100.0, 4)
    insts = [s["inst_pct_one_core"] for s in samples if s["inst_pct_one_core"] is not None]

    host_idle_pct = None
    if sys0 and sys1:
        dtot = sys1[0] - sys0[0]
        didle = sys1[1] - sys0[1]
        host_idle_pct = round(didle / dtot * 100.0, 2) if dtot else None

    rss = [s["rss_kb"] for s in samples if s["rss_kb"]]
    threads = [s["threads"] for s in samples if s["threads"]]
    served = [h["served_bytes"] for h in health_series if isinstance(h.get("served_bytes"), int)]
    active = [h["seed_active"] for h in health_series if isinstance(h.get("seed_active"), int)]

    report = {
        "schema": "coldrevive.nfr02.idle-cpu-sample/v1",
        "criterion": "SRS NFR-02 第一分句：空闲态平均 CPU ≤ 2%",
        "sampling_method": (
            "external /proc/<pid>/stat utime+stime diff over wall clock; daemon-side "
            "sysres sampler not started because no limits.* threshold is configured"
        ),
        "pid": pid,
        "started_at": started_at,
        "duration_sec_planned": duration,
        "duration_sec_actual": round(elapsed_total, 2),
        "kernel": os.uname().release,
        "clock_ticks_per_sec": CLK_TCK,
        "nproc": nproc,
        "loadavg": {"start": [round(x, 3) for x in load0],
                    "end": [round(x, 3) for x in os.getloadavg()]},
        "cpu": {
            "mean_pct_one_core": mean_one_core,
            "mean_pct_of_host": None if mean_one_core is None else round(mean_one_core / nproc, 4),
            "total_cpu_seconds": None if cpu_delta is None else round(cpu_delta, 4),
            "wall_seconds": round(elapsed_total, 2),
            "inst_samples": len(insts),
            "inst_mean": None if not insts else round(sum(insts) / len(insts), 4),
            "inst_p50": pct(insts, 0.50),
            "inst_p95": pct(insts, 0.95),
            "inst_max": None if not insts else round(max(insts), 4),
        },
        "host_context": {
            "idle_pct_including_iowait": host_idle_pct,
            "note": "探针自身也占少量 CPU，本项只作机况对照，不作判据",
        },
        "process": {
            "rss_kb_min": min(rss) if rss else None,
            "rss_kb_max": max(rss) if rss else None,
            "threads_min": min(threads) if threads else None,
            "threads_max": max(threads) if threads else None,
        },
        "idle_evidence": {
            "health_start": {k: health0.get(k) for k in ("status", "seed_tasks", "seed_active", "served_bytes")},
            "health_end": {k: health1.get(k) for k in ("status", "seed_tasks", "seed_active", "served_bytes")},
            "seed_active_max": max(active) if active else None,
            "served_bytes_delta": (max(served) - min(served)) if served else None,
            "health_samples": len(health_series),
            "health_errors": sum(1 for h in health_series if h.get("error")),
        },
        "samples": samples,
        "health_series": health_series,
    }
    with open(out_path, "w", encoding="utf-8") as fh:
        json.dump(report, fh, ensure_ascii=False, indent=1)
    print(out_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
