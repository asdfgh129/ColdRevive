#!/usr/bin/env python3
"""W6.6 无 GUI 看护内核本地 e2e：真实子进程、真实退出码、真实重启预算。

不连公网、不需要第二台机器；用 Python 自身当被监管的子进程，避免依赖任何 GUI。
失败一律写进报告并返回非 0，不把「脚本跑完」当成 PASS。
"""
from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys


def run_agent(bin_path: str, py: str, code: str, *, child: str | None = None,
              child_args: list[str] | None = None, max_restarts: int = 2,
              timeout: float = 12.0) -> dict:
    cmd = [bin_path]
    if child is not None:
        cmd += ["--child", child]
    else:
        cmd += ["--child", py, "--child-arg", "-c", "--child-arg", code]
    cmd += [
        "--health-url", "http://127.0.0.1:1/health",  # 故意不可达：不参与重启判定
        "--poll-ms", "50", "--backoff-ms", "20",
        "--max-restarts", str(max_restarts),
    ]
    proc = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    return {
        "argv": cmd,
        "returncode": proc.returncode,
        "stdout": proc.stdout,
        "stderr": proc.stderr,
    }


def check(name: str, ok: bool, observed: object) -> dict:
    return {"name": name, "status": "pass" if ok else "fail", "observed": observed}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True, help="coldrevive-agent 可执行文件路径")
    ap.add_argument("--python", default=sys.executable, help="用作被监管子进程的 Python")
    ap.add_argument("--report-out", required=True, help="报告 JSON 落盘路径")
    args = ap.parse_args()

    normal = run_agent(args.bin, args.python,
                       "import sys; sys.exit(7)")  # 立即异常退出，快而确定
    clean = run_agent(args.bin, args.python, "pass")
    spawn_err = run_agent(args.bin, args.python, "",
                          child=str(pathlib.Path(args.bin).parent / "no-such-child-binary"))

    checks = [
        check("abnormal_exit_exhausts_budget",
              normal["returncode"] == 3
              and normal["stdout"].count("child spawned") == 3
              and normal["stdout"].count("child exited code=7 clean=false") == 3
              and "重启预算耗尽" in normal["stderr"],
              {"returncode": normal["returncode"],
               "spawns": normal["stdout"].count("child spawned"),
               "exits": normal["stdout"].count("child exited code=7 clean=false"),
               "stderr_tail": normal["stderr"][-160:]}),
        check("clean_exit_is_not_restarted",
              clean["returncode"] == 0
              and clean["stdout"].count("child spawned") == 1
              and "干净退出，不重启" in clean["stdout"],
              {"returncode": clean["returncode"],
               "spawns": clean["stdout"].count("child spawned")}),
        check("spawn_failure_is_error_not_silent_pass",
              spawn_err["returncode"] == 2 and "启动子进程失败" in spawn_err["stderr"],
              {"returncode": spawn_err["returncode"],
               "stderr_tail": spawn_err["stderr"][-160:]}),
    ]

    out = pathlib.Path(args.report_out)
    out.parent.mkdir(parents=True, exist_ok=True)
    report = {
        "suite": "W6.6 agent supervisor (headless, no GUI, no external host)",
        "checks": checks,
        "counts": {
            "pass": sum(c["status"] == "pass" for c in checks),
            "fail": sum(c["status"] == "fail" for c in checks),
        },
        "runs": {"abnormal": normal, "clean": clean, "spawn_error": spawn_err},
        "scope_note": "只证明 std-only 看护内核的重启语义；托盘/浏览器捕获/UPnP 续租仍 BLOCKED。",
    }
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report["counts"], ensure_ascii=False))
    return 0 if report["counts"]["fail"] == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
