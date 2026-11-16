#!/usr/bin/env python3
"""G3 兼容性矩阵基线核验：只读检查客户端可用性与已有证据，绝不判定 pass。

用法：
    python tests/e2e/g3/compat_matrix.py [--out tests/e2e/g3/reports/baseline.json]
                                         [--closeout tests/fixtures/g3-closeout.toml]

纪律（与项目其它 spike 一致）：
- `g3_status` 恒为 `not_run`；`evidence_valid_for_pass` 恒为 `false`。
  这两个字段描述的是**本脚本**：它没有跑过任何互操作，无权主张通过。
- 「客户端已安装」「有历史报告」都只是**可跑性/素材**，不是通过。
- 缺客户端、缺证据一律写 blocked，不猜版本、不伪造命中。
- 有报告但没有独立可执行客户端的行（AriaNg / 浏览器插件 / 关闭 UI）记
  `evidence_collected`，同样不表示通过。

与人工结案的关系（2026-11-16 起）：
- 人的判定写在 `tests/fixtures/g3-closeout.toml`，本脚本**只转述**，放进 `closeout` 块并
  标记 `machine_inferred=false`；它不会改变自己的机器字段，也不会去猜缺失的记录。
- 于是输出里同时存在两个口径：`g3_status`（机器：not_run）与
  `g3_status_declared`（人工：closed_with_exceptions）。两者不冲突，是分工。
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
DEFAULT_CLOSEOUT = REPO / "tests" / "fixtures" / "g3-closeout.toml"
DEFAULT_OUT = REPO / "tests" / "e2e" / "g3" / "reports" / "baseline.json"

# 人工结案记录允许的取值。写错值比写错结论更危险（会被当成有效声明），所以这里白名单校验。
ALLOWED_DECISIONS = {"close_with_exceptions", "close_clean", "reopened"}
ALLOWED_GATE_STATUS = {"closed_with_exceptions", "closed", "reopened", "not_run"}
ALLOWED_ROW_VERDICTS = {"accepted", "accepted_with_gap"}


def load_toml(path: Path) -> dict:
    try:
        import tomllib
    except ImportError:  # pragma: no cover - Python < 3.11
        raise SystemExit("需要 Python 3.11+（tomllib）")
    with path.open("rb") as f:
        return tomllib.load(f)


def load_closeout(path: Path | None) -> dict | None:
    """读人工结案记录。文件不在 = 没有结案声明，绝不替它编一个。"""
    if path is None or not path.is_file():
        return None
    raw = load_toml(path)
    decision = str(raw.get("decision", ""))
    gate = str(raw.get("gate_status_declared", ""))
    if decision not in ALLOWED_DECISIONS:
        raise SystemExit(f"g3-closeout: decision={decision!r} 不在 {sorted(ALLOWED_DECISIONS)} 内")
    if gate not in ALLOWED_GATE_STATUS:
        raise SystemExit(f"g3-closeout: gate_status_declared={gate!r} 不在 {sorted(ALLOWED_GATE_STATUS)} 内")
    rows = []
    for rv in raw.get("row_verdict", []):
        v = str(rv.get("verdict", ""))
        if v not in ALLOWED_ROW_VERDICTS:
            raise SystemExit(f"g3-closeout: row {rv.get('id')!r} verdict={v!r} 不在 {sorted(ALLOWED_ROW_VERDICTS)} 内")
        rows.append(
            {
                "id": str(rv.get("id", "")),
                "verdict": v,
                "observed": str(rv.get("observed", "")),
                "gap": str(rv.get("gap", "")),
            }
        )
    exceptions = [
        {
            "id": str(e.get("id", "")),
            "scope": str(e.get("scope", "")),
            "what": str(e.get("what", "")),
            "why": str(e.get("why", "")),
            "recheck_trigger": str(e.get("recheck_trigger", "")),
            "needs_owner_action": bool(e.get("needs_owner_action", False)),
            "owner_action": str(e.get("owner_action", "")),
        }
        for e in raw.get("exception", [])
    ]
    return {
        "present": True,
        "source": str(path.relative_to(REPO)).replace("\\", "/"),
        "authority": str(raw.get("authority", "human")),
        "decided_by": str(raw.get("decided_by", "")),
        "decided_at": str(raw.get("decided_at", "")),
        "decision": decision,
        "gate_status_declared": gate,
        "commitment": str(raw.get("commitment", "")),
        "rationale": str(raw.get("rationale", "")).strip(),
        # 这份结论是人给的，不是本脚本推出来的 —— 单列出来，免得以后被读成机器结论。
        "machine_inferred": False,
        "row_verdicts": rows,
        "exceptions": exceptions,
        "needs_owner_action": [e["id"] for e in exceptions if e["needs_owner_action"]],
    }


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


def audit(fixture: Path, closeout_path: Path | None = None) -> dict:
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
        if evidence and exe is not None:
            status = "runnable_with_evidence"
        elif evidence and rc is not None:
            status = "runnable_with_evidence"
            version = rc["version_observed"]
        elif evidence:
            # AriaNg / browser-extension / UI-closed 这类行没有独立可执行客户端，
            # 但已有真实互操作报告。只记「有证据」，不表示 G3 通过。
            status = "evidence_collected"
        elif exe is None and rc is None:
            status = "blocked_no_client"
        elif exe is None and rc is not None:
            status = "runnable_remote"
            version = rc["version_observed"]
        else:
            status = "runnable_no_evidence"
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
    closeout = load_closeout(closeout_path)
    return {
        "schema": "coldrevive.g3.baseline.v1",
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "fixture": str(fixture.relative_to(REPO)).replace("\\", "/"),
        # 机器口径：本脚本没跑互操作，永远不判通过。
        "g3_status": "not_run",
        "evidence_valid_for_pass": False,
        # 人工口径：由 g3-closeout.toml 声明，本脚本只转述。
        "g3_status_declared": closeout["gate_status_declared"] if closeout else None,
        "g3_status_declared_source": closeout["source"] if closeout else None,
        "closeout": closeout,
        "note": "客户端可用性 + 历史素材清点；不运行互操作，不判 pass。"
        "g3_status 是机器口径（本脚本永远 not_run），g3_status_declared 是人工结案口径，两者分开读。",
        "rows": rows,
        "summary": {
            "total": len(rows),
            "runnable_with_evidence": sum(r["status"] == "runnable_with_evidence" for r in rows),
            "runnable_no_evidence": sum(r["status"] == "runnable_no_evidence" for r in rows),
            "runnable_remote": sum(r["status"] == "runnable_remote" for r in rows),
            "evidence_collected": sum(r["status"] == "evidence_collected" for r in rows),
            "blocked_no_client": sum(r["status"] == "blocked_no_client" for r in rows),
        },
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--fixture", type=Path, default=DEFAULT_FIXTURE)
    ap.add_argument("--closeout", type=Path, default=DEFAULT_CLOSEOUT)
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT)
    ap.add_argument("--self-test", action="store_true", help="核验本脚本永远不会自己判 pass")
    args = ap.parse_args()
    if args.self_test:
        # 断言一/二/三：无论人工结案怎么写，机器字段都不许变。
        probe = audit(args.fixture.resolve(), args.closeout.resolve() if args.closeout.is_file() else None)
        assert probe["g3_status"] == "not_run", probe["g3_status"]
        assert probe["evidence_valid_for_pass"] is False
        assert all(r["g3_verdict"] == "not_run" for r in probe["rows"])
        # 断言四：结案结论若存在，必须是「转述的人工记录」，不是机器推断。
        if probe["closeout"] is not None:
            assert probe["closeout"]["machine_inferred"] is False
            assert probe["g3_status_declared"] == probe["closeout"]["gate_status_declared"]
            assert probe["g3_status_declared_source"] == probe["closeout"]["source"]
            stated = {r["id"] for r in probe["closeout"]["row_verdicts"]}
            under_test = {str(r["id"]) for r in probe["rows"]}
            # 结案记录里出现了矩阵没有的行 = 记录已漂移，宁可报错也不静默通过。
            assert stated <= under_test, f"结案记录引用了矩阵外的行: {sorted(stated - under_test)}"
        print(
            "G3 self-test: 4/4 PASS（机器口径恒 not_run；人工结案只转述、标记 machine_inferred=false；"
            "结案行必须是矩阵内的行）"
        )
        return 0
    result = audit(
        args.fixture.resolve(),
        args.closeout.resolve() if args.closeout and args.closeout.is_file() else None,
    )
    out = args.out if args.out.is_absolute() else REPO / args.out
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"G3 基线: {out.relative_to(REPO) if out.is_relative_to(REPO) else out}")
    co = result["closeout"]
    if co:
        gaps = sum(1 for r in co["row_verdicts"] if r["verdict"] == "accepted_with_gap")
        print(
            f"人工结案口径: {co['gate_status_declared']}（{co['decided_by']} @ {co['decided_at']}）"
            f"｜{len(co['row_verdicts'])} 行判定（{gaps} 行带缺口）｜{len(co['exceptions'])} 条例外"
        )
        if co["needs_owner_action"]:
            print(f"  需要所有者开环境才能重测: {', '.join(co['needs_owner_action'])}")
    else:
        print("人工结案口径: 无记录（未结案）")
    print("机器口径: g3_status=not_run, evidence_valid_for_pass=false（核验器不跑互操作，永不判 pass）")
    print(json.dumps(result["summary"], ensure_ascii=False))
    for r in result["rows"]:
        print(f"  {r['id']:<20} {r['status']:<24} evidence={len(r['evidence'])}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
