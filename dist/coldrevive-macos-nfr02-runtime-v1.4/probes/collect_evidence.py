#!/usr/bin/env python3
"""汇总 macOS NFR-02 真机运行包的原始输出。

只做证据整理与逐项判定，不写门禁结论，不把本脚本的输出当成「NFR-02 达标」。
最终是否接受证据、是否改口径，由所有者在仓库侧决定。
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import pathlib
import sys
from typing import Optional


REQUIRED_UNIT_TESTS = (
    "prio::tests::idle_applies_to_the_calling_thread_only",
    "prio::tests::runtime_worker_task_runs_at_idle_priority",
    "prio::tests::darwin_verdict_reads_back_instead_of_trusting_return_code",
    "prio::tests::darwin_echo_of_minus_one_is_never_a_priority",
    "prio::tests::outcome_strings_are_stable",
)


def read_text(path: pathlib.Path) -> str:
    try:
        return path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""


def read_exit(path: pathlib.Path) -> Optional[int]:
    text = read_text(path).strip()
    if not text:
        return None
    try:
        return int(text)
    except ValueError:
        return None


def read_json(path: pathlib.Path) -> Optional[dict]:
    try:
        obj = json.loads(read_text(path))
    except (ValueError, TypeError):
        return None
    return obj if isinstance(obj, dict) else None


def sha256(path: pathlib.Path) -> Optional[str]:
    try:
        h = hashlib.sha256()
        with path.open("rb") as fh:
            for chunk in iter(lambda: fh.read(65536), b""):
                h.update(chunk)
        return h.hexdigest()
    except OSError:
        return None


def check(name: str, ok: Optional[bool], observed: object, expected: object) -> dict:
    if ok is None:
        status = "blocked"
    elif ok:
        status = "pass"
    else:
        status = "fail"
    return {
        "name": name,
        "status": status,
        "observed": observed,
        "expected": expected,
    }


def parse_preflight(path: pathlib.Path) -> dict:
    out: dict[str, str] = {}
    for line in read_text(path).splitlines():
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        out[key.strip()] = value.strip()
    return out


def parse_source_info(path: pathlib.Path) -> dict:
    return parse_preflight(path)


def unit_check(results: pathlib.Path) -> dict:
    code = read_exit(results / "01-cargo-test.exit")
    log = read_text(results / "01-cargo-test.log")
    missing = [name for name in REQUIRED_UNIT_TESTS if name not in log]
    ok_lines = [name for name in REQUIRED_UNIT_TESTS
                if f"test {name} ... ok" in log]
    observed = {
        "exit_code": code,
        "required_tests": list(REQUIRED_UNIT_TESTS),
        "seen_ok": ok_lines,
        "missing_from_log": missing,
        "test_summary_tail": log.strip().splitlines()[-3:],
    }
    ok = (code == 0 and not missing and len(ok_lines) == len(REQUIRED_UNIT_TESTS)) if code is not None else None
    return check("cr-engine unit tests on macOS", ok, observed,
                 "exit 0 and all required darwin/idle tests report ok")


def build_check(results: pathlib.Path, root: pathlib.Path) -> dict:
    code = read_exit(results / "02-build.exit")
    binary = root / "source" / "target" / "debug" / "coldrevived"
    observed = {
        "exit_code": code,
        "binary_path": str(binary),
        "binary_exists": binary.is_file(),
        "binary_sha256": sha256(binary),
    }
    ok = (code == 0 and binary.is_file()) if code is not None else None
    return check("coldrevived builds on macOS", ok, observed, "exit 0 and binary exists")


def daemon_check(results: pathlib.Path, tag: str, want_requested: bool) -> dict:
    snapshots = []
    for n in (1, 2, 3):
        snapshots.append(read_json(results / f"health-{tag}-{n}.json"))
    pid_text = read_text(results / f"daemon-{tag}.pid").strip()
    observed = {
        "pid": pid_text,
        "daemon_log_tail": read_text(results / f"daemon-{tag}.log").strip().splitlines()[-8:],
        "snapshots": [],
    }
    all_ok = True
    for idx, snap in enumerate(snapshots, 1):
        if snap is None:
            all_ok = False
            observed["snapshots"].append({"index": idx, "health": None})
            continue
        ip = snap.get("idle_priority")
        ip = ip if isinstance(ip, dict) else {}
        row = {
            "index": idx,
            "status": snap.get("status"),
            "node": snap.get("node"),
            "seed_tasks": snap.get("seed_tasks"),
            "idle_priority": ip,
        }
        ok = (
            snap.get("status") == "ok"
            and snap.get("node") == "A"
            and isinstance(snap.get("seed_tasks"), (int, float))
            and snap["seed_tasks"] >= 1
            and ip.get("requested") is want_requested
            and ip.get("applied", 0) >= (1 if want_requested else 0)
            and ip.get("failed") == 0
            and ip.get("unsupported") == 0
        )
        if want_requested:
            ok = ok and ip.get("applied", 0) >= 1
        else:
            ok = ok and ip.get("applied") == 0
        row["snapshot_ok"] = ok
        all_ok = all_ok and ok
        observed["snapshots"].append(row)
    label = "on" if want_requested else "off (negative control)"
    expected = {
        "status": "ok",
        "node": "A",
        "seed_tasks": ">= 1",
        "idle_priority.requested": want_requested,
        "idle_priority.applied": ">= 1" if want_requested else 0,
        "idle_priority.failed": 0,
        "idle_priority.unsupported": 0,
        "snapshots": "3/3 must hold",
    }
    return check(f"daemon idle_priority {label}", all_ok, observed, expected)


def guard_wake_check(results: pathlib.Path) -> dict:
    obj = read_json(results / "guard-wake.json")
    if obj is None:
        return check(
            "wake path functional (local BT handshake -> sample md5)",
            None,
            {"reason": "guard-wake.json not produced"},
            "handshake wakes seed and full sample md5 matches fixture",
        )
    checks = obj.get("checks")
    checks = checks if isinstance(checks, list) else []
    failed = [c.get("name") for c in checks
              if isinstance(c, dict) and c.get("status") != "pass"]
    observed = {
        "bt_target": obj.get("bt_target"),
        "bytes_received": obj.get("bytes_received"),
        "md5_received": obj.get("md5_received"),
        "expected_md5": obj.get("expected_md5"),
        "failed_checks": failed,
        "error": obj.get("error"),
    }
    ok = bool(checks) and not failed and obj.get("error") is None
    return check(
        "wake path functional (local BT handshake -> sample md5)",
        ok,
        observed,
        "all local guard-wake checks pass and md5 matches; functional only, not thread-priority evidence",
    )


def wake_path_check() -> dict:
    return {
        "name": "wake path not downgraded (guard / data-plane threads)",
        "status": "not_verified",
        "observed": {
            "reason": "Darwin getpriority(PRIO_DARWIN_THREAD, 0) only reads the calling thread; "
                      "this kit does not add an in-process per-thread reader for wake threads.",
        },
        "expected": "DOC-34 §10: confirm foreground class on wake threads, or state explicitly "
                    "that per-thread readback is unavailable. Do not substitute process-level data.",
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--results", required=True, type=pathlib.Path)
    ap.add_argument("--root", required=True, type=pathlib.Path)
    args = ap.parse_args()

    results = args.results.resolve()
    root = args.root.resolve()
    preflight = parse_preflight(results / "preflight.txt")
    source_info = parse_source_info(root / "SOURCE_INFO.txt")

    checks = [
        unit_check(results),
        build_check(results, root),
        daemon_check(results, "on", True),
        daemon_check(results, "off", False),
        guard_wake_check(results),
        wake_path_check(),
    ]

    evidence_files = []
    for p in sorted(results.iterdir()):
        if p.is_file():
            evidence_files.append({
                "file": p.name,
                "bytes": p.stat().st_size,
                "sha256": sha256(p),
            })

    summary = {
        "schema": "coldrevive.nfr02.macos-runtime/v1",
        "recorded_at_utc": dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat(),
        "clause": "SRS NFR-02 第二分句：造种任务仅以 Idle 优先级运行",
        "criteria_ref": "DOC-34 v1.5 §10 真机收尾清单",
        "source": source_info,
        "environment": preflight,
        "checks": checks,
        "evidence_files": evidence_files,
        "counts": {
            "pass": sum(c["status"] == "pass" for c in checks),
            "fail": sum(c["status"] == "fail" for c in checks),
            "not_verified": sum(c["status"] == "not_verified" for c in checks),
            "blocked": sum(c["status"] == "blocked" for c in checks),
        },
        "gate_claim": None,
        "gate_claim_note": "脚本只整理证据与逐项判定；g2_claim / NFR-02 口径由所有者决定，脚本不置位。",
        "wording_constraints": [
            "不可写「NFR-02 三平台达标」或「macOS 已验证」，除非所有者书面改口径。",
            "wake path 一项为 not_verified，不得用进程级读数或 Linux/Windows 结果外推。",
        ],
    }

    out = results / "summary.json"
    out.write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"summary: {out}")
    for c in checks:
        print(f"  [{c['status']:>12}] {c['name']}")
    print("  gate_claim: null (human decision required)")
    return 0 if summary["counts"]["fail"] == 0 and summary["counts"]["blocked"] == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
