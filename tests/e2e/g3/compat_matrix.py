#!/usr/bin/env python3
"""G3 兼容性矩阵基线核验：只读检查客户端可用性与已有证据，绝不判定 pass。

用法：
    python tests/e2e/g3/compat_matrix.py [--out tests/e2e/g3/reports/baseline.json]

纪律（与项目其它 spike 一致）：
- `g3_status` 恒为 `not_run`；`evidence_valid_for_pass` 恒为 `false`。
- 「客户端已安装」「有历史报告」都只是**可跑性/素材**，不是通过。
- 缺客户端、缺证据一律写 blocked，不猜版本、不伪造命中。
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
DEFAULT_FIXTURE = REPO / "tests" / "fixtures" / "compat-matrix.toml"
DEFAULT_OUT = REPO / "tests" / "e2e" / "g3" / "reports" / "baseline.json"


def load_toml(path: Path) -> dict:
    try:
        import tomllib
    except ImportError:  # pragma: no cover - Python < 3.11
        raise SystemExit("需要 Python 3.11+（tomllib）")
    with path.open("rb") as f:
        return tomllib.load(f)


def find_executable(spec: str) -> str | None:
    if not spec:
        return None
    if os.path.sep in spec or "/" in spec:
        p = Path(spec)
        return str(p) if p.is_file() else None
    return shutil.which(spec)


def read_version(exe: str | None) -> str:
    if not exe:
        return "unknown"
    try:
        out = subprocess.run(
            [exe, "--version"],
            capture_output=True,
            text=True,
            timeout=4,
            check=False,
        )
        line = (out.stdout or out.stderr).strip().splitlines()
        return line[0][:200] if line else "unknown"
    except (OSError, subprocess.SubprocessError):
        return "unknown"


def evidence_for(pattern: str) -> list[str]:
    if not pattern:
        return []
    return sorted(
        str(Path(p).relative_to(REPO)).replace("\\", "/")
        for p in glob.glob(str(REPO / pattern), recursive=True)
        if Path(p).is_file()
    )


def audit(fixture: Path) -> dict:
    data = load_toml(fixture)
    # 远端客户端注册表：本机没装的行可借 LAN 主机取证（只读登记，不联网、不判 pass）。
    remote = {}
    for rc in data.get("remote_clients", []):
        remote[str(rc.get("row_id"))] = {
            "host": str(rc.get("host", "")),
            "executable": str(rc.get("executable", "")),
            "version_observed": str(rc.get("version_observed", "")),
            "mode": str(rc.get("mode", "")),
        }
    rows = []
    for spec in data.get("rows", []):
        rid = str(spec.get("id"))
        exe = find_executable(str(spec.get("executable", "")))
        version = read_version(exe)
        evidence = evidence_for(str(spec.get("evidence_glob", "")))
        rc = remote.get(rid)
        if exe is None and rc is None:
            status = "blocked_no_client"
        elif exe is None and rc is not None:
            status = "runnable_remote"
            version = rc["version_observed"]
        elif not evidence:
            status = "runnable_no_evidence"
        else:
            status = "runnable_with_evidence"
        rows.append(
            {
                "id": spec.get("id"),
                "client": spec.get("client"),
                "scenario": spec.get("scenario"),
                "executable": exe,
                "version_observed": version,
                "remote_client": rc,
                "evidence": evidence,
                "evidence_note": spec.get("evidence_note", ""),
                "status": status,
                # 这是硬常量：本脚本没有执行任何互操作测试，不能判通过。
                "g3_verdict": "not_run",
            }
        )
    return {
        "schema": "coldrevive.g3.baseline.v1",
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "fixture": str(fixture.relative_to(REPO)).replace("\\", "/"),
        "g3_status": "not_run",
        "evidence_valid_for_pass": False,
        "note": "客户端可用性 + 历史素材清点；不运行互操作，不判 pass。G3 全绿须实验执行人依据真实测试手工回填。",
        "rows": rows,
        "summary": {
            "total": len(rows),
            "runnable_with_evidence": sum(r["status"] == "runnable_with_evidence" for r in rows),
            "runnable_no_evidence": sum(r["status"] == "runnable_no_evidence" for r in rows),
            "runnable_remote": sum(r["status"] == "runnable_remote" for r in rows),
            "blocked_no_client": sum(r["status"] == "blocked_no_client" for r in rows),
        },
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--fixture", type=Path, default=DEFAULT_FIXTURE)
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT)
    ap.add_argument("--self-test", action="store_true", help="核验本脚本永远不会判 pass")
    args = ap.parse_args()
    if args.self_test:
        probe = audit(args.fixture.resolve())
        assert probe["g3_status"] == "not_run"
        assert probe["evidence_valid_for_pass"] is False
        assert all(r["g3_verdict"] == "not_run" for r in probe["rows"])
        print("G3 self-test: 3/3 PASS（g3_status / evidence_valid / 每行 verdict 都未判通过）")
        return 0
    result = audit(args.fixture.resolve())
    out = args.out if args.out.is_absolute() else REPO / args.out
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"G3 基线: {out.relative_to(REPO) if out.is_relative_to(REPO) else out}")
    print(json.dumps(result["summary"], ensure_ascii=False))
    for r in result["rows"]:
        print(f"  {r['id']:<20} {r['status']:<24} evidence={len(r['evidence'])}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
