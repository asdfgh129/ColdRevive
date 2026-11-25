#!/usr/bin/env python3
"""把 221 上的 NFR-02 采样结果整理成归档证据（本脚本在本机跑，只读远端文件）。

用法：python scripts/build_nfr02_evidence.py <raw.json> <out.json>
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
import time


def sha256_file(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def git(*args: str) -> str:
    return subprocess.run(["git", *args], capture_output=True, text=True).stdout.strip()


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    raw_path, out_path = sys.argv[1], sys.argv[2]
    raw = json.load(open(raw_path, encoding="utf-8"))

    cpu = raw["cpu"]
    mean = cpu["mean_pct_one_core"]
    threshold = 2.0
    verdict = "PASS" if (mean is not None and mean <= threshold) else "FAIL"
    if mean is None:
        verdict = "BLOCKED"

    # 采样点太多，归档时按 5 抽 1 保留趋势，均值/极值不受影响（由总量差与全样本算得）。
    samples = raw.get("samples") or []
    kept = samples[::5]

    commit = git("rev-parse", "HEAD")
    evidence = {
        "schema": "coldrevive.nfr02.idle-cpu-evidence/v1",
        "recorded_at": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        "scope": (
            "NFR-02 第一分句（空闲态平均 CPU ≤ 2%）的独立采点；"
            "不覆盖第二分句（造种任务仅以 Idle 优先级运行），该分句无运行时实现，见 open_issues"
        ),
        "criterion": {
            "text": "SRS NFR-02：空闲态平均 CPU ≤ 2%；造种任务仅以 Idle 优先级运行（本次只测前半句）",
            "threshold_pct_one_core": threshold,
            "window_sec": raw["duration_sec_actual"],
        },
        "verdict": {
            "idle_cpu_clause": verdict,
            "mean_pct_one_core": mean,
            "headroom_x": (round(threshold / mean, 2) if mean else None),
            "note": "判据是均值 ≤2%；p95/max 作分布描述，不作判据",
        },
        "measurement": raw,
        "provenance": {
            "git_commit": commit,
            "git_commit_subject": git("log", "-1", "--pretty=%s", commit),
            "git_commit_date": git("log", "-1", "--pretty=%cI", commit),
            "host": "221 (ubuntu, 192.168.1.221 / 出口 112.5.60.4)",
            "kernel_release": raw.get("kernel") or os.environ.get("NFR02_KERNEL", ""),
            "nproc": raw["nproc"],
            "clock_ticks_per_sec": raw["clock_ticks_per_sec"],
            "binary_sha256": os.environ.get("NFR02_BIN_SHA", ""),
            "previous_binary_sha256": os.environ.get("NFR02_PREV_BIN_SHA", ""),
            "build": {
                "note": "在 221 上从 HEAD 源码 native build；同步前构建树的 8 个 .rs 落后于 HEAD，已重同步",
                "src_tree_matches_head": "86/86 .rs 文件逐字节一致（sha256 与 git blob 比对）",
                "cargo": os.environ.get("NFR02_CARGO", ""),
            },
            "scheduler_priority": {
                "systemd_Nice": os.environ.get("NFR02_NICE", ""),
                "systemd_CPUSchedulingPolicy": os.environ.get("NFR02_CPUPOL", ""),
                "systemd_IOSchedulingClass": os.environ.get("NFR02_IOCLASS", ""),
                "note": "SCHED_OTHER / nice 0 / best-effort：第二分句未在运行时实现",
            },
            "tc_qdisc_and_class": "未采集（本项与网络整形无关；NFR-02 为进程 CPU 指标）",
        },
        "idle_basis": {
            "role": "holder",
            "dht_enabled": False,
            "seed_tasks": 1,
            "seed_state": "COLD",
            "seed_active": 0,
            "hot": 0,
            "served_bytes_delta": (raw.get("idle_evidence") or {}).get("served_bytes_delta"),
            "limits_configured": False,
            "note": "无对端、无上传、无 DHT；limits.* 一条未配（因此 daemon 自身采样线程不起，见下）",
        },
        "why_external_sampling": (
            "sysres::spawn() 仅在配置了任一 limits.* 阈值时启动采样线程（needed() 为 false 直接返回），"
            "本环境未配任何阈值，故 /health 的 limits.max_seeding_cpu_millipct.current=null、"
            "sample_at_secs=0。这是「没开采样」而非「读不到 CPU」——早期长跑的 "
            "cpu_percent_sampled=false 同源于采集脚本未取 /health 资源块，不是平台能力缺失。"
        ),
        "open_issues": [
            {
                "id": "NFR-02-b",
                "text": "「造种任务仅以 Idle 优先级运行」在守护进程内无运行时实现："
                        "仓库无 setpriority / SetThreadPriority / SCHED_IDLE 调用，无 libc/winapi 依赖；"
                        "S7 基准的 Idle 优先级由基准脚本自设，属测试夹具属性，不是产品保证",
                "evidence": "grep -rn 'setpriority|SetThreadPriority|SCHED_IDLE' src/ 为空；systemd Nice=0",
            }
        ],
        "do_not_claim": [
            "NFR-02 达标（第二分句未实现）",
            "任何可达率/成功率",
            "本条样本可替代公平性或 S6 的判定",
        ],
        "gate_snapshot_at_capture": {
            "g2_claim": True,
            "g2_claim_note": "人为字段，所有者 2026-11-25T17:20 声明达标（DOC-34）；本条样本不自置任何 pass",
            "s6_status": "not_run",
            "evidence_valid_for_pass": False,
            "auto_pass": False,
        },
        "raw_file": {"name": os.path.basename(raw_path), "sha256": sha256_file(raw_path)},
        "samples_note": f"归档保留 {len(kept)}/{len(samples)} 条（5 抽 1）；均值由首尾累计 CPU 差与墙钟算得",
        "samples_downsampled": kept,
    }
    with open(out_path, "w", encoding="utf-8") as fh:
        json.dump(evidence, fh, ensure_ascii=False, indent=1)
    print(json.dumps({
        "verdict": verdict,
        "mean_pct_one_core": mean,
        "p95": cpu["inst_p95"],
        "max": cpu["inst_max"],
        "total_cpu_s": cpu["total_cpu_seconds"],
        "wall_s": cpu["wall_seconds"],
        "rss_kb": [raw["process"]["rss_kb_min"], raw["process"]["rss_kb_max"]],
        "host_idle_pct": raw["host_context"]["idle_pct_including_iowait"],
    }, ensure_ascii=False, indent=1))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
