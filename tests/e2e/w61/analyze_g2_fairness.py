#!/usr/bin/env python3
"""按预注册判据计算 G2 公平性；只输出结果，不改门禁、不让 CI 自动 pass。"""
from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

try:
    import tomllib  # Python 3.11+
except ImportError:  # pragma: no cover
    tomllib = None


def load_fixture(path: pathlib.Path) -> dict:
    if tomllib is None:
        # 不猜测阈值：无法解析就直接拒绝，防止用默认值替代预注册值。
        raise SystemExit("需要 Python 3.11+ tomllib 读取预注册判据")
    with path.open("rb") as f:
        return tomllib.load(f)


def samples_in_window(flow: dict, lo_ms: int, hi_ms: int) -> int:
    total = 0
    for pair in flow.get("receiver", {}).get("samples", []):
        if len(pair) != 2:
            continue
        t, n = pair
        if lo_ms <= t <= hi_ms:
            total += int(n)
    return total


def jain(values: list[int]) -> float:
    total = sum(values)
    if total <= 0:
        return 0.0
    return (total * total) / (len(values) * sum(v * v for v in values))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", required=True, help="run_g2_fairness.sh 的 OUT 目录")
    ap.add_argument("--fixture", default="tests/fixtures/g2-fairness-thresholds.toml")
    ap.add_argument("--out", help="可选：写汇总 JSON")
    args = ap.parse_args()

    base = pathlib.Path(args.dir)
    fixture = load_fixture(pathlib.Path(args.fixture))
    c = fixture["criteria"]
    lo = int(c["warmup_s"]) * 1000
    hi = (int(c["warmup_s"]) + int(c["sample_s"])) * 1000
    jmin = float(c["jain_min"])
    smin = float(c["share_min"])
    smax = float(c["share_max"])
    need_rounds = int(c["rounds_per_arm"])

    pattern = re.compile(r"^(L\d+)-([A-Za-z0-9_]+)-r(\d+)$")
    groups: dict[tuple[str, str], list[dict]] = {}
    for path in sorted(base.glob("flow1-*.json")):
        tag = path.name[len("flow1-") : -len(".json")]
        m = pattern.match(tag)
        if not m:
            continue
        shape, arm, rnd = m.group(1), m.group(2), int(m.group(3))
        f1 = json.loads(path.read_text(encoding="utf-8"))
        f2p = base / f"flow2-{tag}.json"
        rp = base / f"relay-{tag}.json"
        if not f2p.exists() or not rp.exists():
            row = {"round": rnd, "valid": False, "reason": "missing_flow2_or_relay", "pass": False}
        else:
            f2 = json.loads(f2p.read_text(encoding="utf-8"))
            relay = json.loads(rp.read_text(encoding="utf-8"))
            x = [
                samples_in_window(f1, lo, hi),
                samples_in_window(f2, lo, hi),
            ]
            total = sum(x)
            shares = [v / total if total else 0.0 for v in x]
            j = jain(x)
            ok = total > 0 and j >= jmin and all(smin <= s <= smax for s in shares)
            row = {
                "round": rnd,
                "valid": total > 0,
                "received_bytes": x,
                "shares": shares,
                "jain": j,
                "pass": ok,
                "cc_flow1": f1.get("sender", {}).get("cc"),
                "cc_flow2": f2.get("sender", {}).get("cc"),
                "retransmits": [
                    f1.get("sender", {}).get("retransmits"),
                    f2.get("sender", {}).get("retransmits"),
                ],
                "relay_stats": relay.get("stats"),
            }
        groups.setdefault((shape, arm), []).append(row)

    summary = []
    for (shape, arm), rows in sorted(groups.items()):
        rows.sort(key=lambda r: r["round"])
        valid = [r for r in rows if r.get("valid")]
        passed = sum(1 for r in valid if r.get("pass"))
        criterion_met = len(valid) == need_rounds and passed == need_rounds
        summary.append(
            {
                "shape": shape,
                "arm": arm,
                "rounds_seen": len(rows),
                "rounds_valid": len(valid),
                "rounds_pass": passed,
                "criterion_met": criterion_met,
                "criterion": {
                    "jain_min": jmin,
                    "share_min": smin,
                    "share_max": smax,
                    "rounds_required": need_rounds,
                    "all_rounds_must_pass": True,
                },
                "rounds": rows,
            }
        )

    result = {
        "tool": "analyze_g2_fairness.py",
        "fixture_version": fixture.get("version"),
        "fixture_recorded_at": fixture.get("recorded_at"),
        "scope": fixture.get("scope"),
        "summary": summary,
        "gate_snapshot": fixture.get("gate_snapshot"),
        "verdict_wording": "component-level fairness measurement only; not G2 unlock",
    }
    text = json.dumps(result, ensure_ascii=False, indent=2)
    if args.out:
        pathlib.Path(args.out).write_text(text + "\n", encoding="utf-8")
    print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
