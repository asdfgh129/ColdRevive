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

    # 判定面：只判 `[arms].required` 里、且在 `[link_shapes].judging_shapes` 内的组合。
    # `[arms].reference`（v2 的 cc_on）记录并归档，但**不参与判定**——用它判会把
    # v1 已 FAIL 的 AIMD 结论混进 v2 的结论里。
    arms_cfg = fixture.get("arms", {})
    required = list(arms_cfg.get("required", []))
    reference = list(arms_cfg.get("reference", []))
    judging_shapes = list(fixture.get("link_shapes", {}).get("judging_shapes", []))
    shapes_seen = sorted({s["shape"] for s in summary})
    judge_shapes = judging_shapes or shapes_seen
    judged = [
        s
        for s in summary
        if (not required or s["arm"] in required) and s["shape"] in judge_shapes
    ]
    expected = len(required) * len(judge_shapes) if required else len(judged)
    missing = [
        f"{shape}/{arm}"
        for shape in judge_shapes
        for arm in required
        if not any(s["shape"] == shape and s["arm"] == arm for s in summary)
    ]
    failed = [
        f"{s['shape']}/{s['arm']}"
        for s in judged
        if not s["criterion_met"]
    ]
    overall_met = bool(required) and not failed and not missing and len(judged) == expected
    overall = {
        "required_arms": required,
        "reference_arms_not_judged": reference,
        "judging_shapes": judge_shapes,
        "judged_combinations": len(judged),
        "expected_combinations": expected,
        "missing_combinations": missing,
        "failed_combinations": failed,
        "criterion_met": overall_met,
        "wording": (
            "该判据在单机 netns + 真内核 HTB/netem 下被满足"
            if overall_met
            else "该判据未被满足（不得写成「基本达标」或「带例外 PASS」）"
        ),
        "does_not_imply": [
            "G2 解禁或 NFR 达标",
            "跨公网 / 跨物理主机公平性",
            "与外部 Cubic/Reno 竞争的表现",
            "任何成功率或可达率（ADR-012）",
        ],
    }

    result = {
        "tool": "analyze_g2_fairness.py",
        "fixture_version": fixture.get("version"),
        "fixture_recorded_at": fixture.get("recorded_at"),
        "mechanism_under_test": fixture.get("mechanism_under_test"),
        "scope": fixture.get("scope"),
        "summary": summary,
        "overall": overall,
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
