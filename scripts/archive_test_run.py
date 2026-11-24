#!/usr/bin/env python3
"""Append a verifiable test-run record to tests/ARCHIVE/TEST-RUNS-<date>.md.

Every real test run (cargo test, e2e, probe, packet capture, remote-host check)
must be archived here before its result is cited in docs, changelog or gate
reasoning. Raw reports may stay gitignored, but this ledger is versioned.

Usage example:
  python3 scripts/archive_test_run.py --date 2026-11-24 --suite FR-24 \
    --title "public KAD publish->search loop" --result PASS \
    --command "coldrevive-ed2k search --public ..." \
    --evidence tests/e2e/w24-pub/reports/public-209-public-loop-verified-20261124.json \
    --criteria "real 0x4B ACK and parsed 0x3B answer" \
    --gate-impact "none: g2_claim=false, evidence_valid_for_pass=false"
"""

from __future__ import annotations

import argparse
import datetime as _dt
import hashlib
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
ARCHIVE_DIR = ROOT / "tests" / "ARCHIVE"
REDACT = re.compile(
    r"(?i)(--(?:password|passwd|secret|token|key-hex|stun-key)\s*"
    r"|[-_](?:password|passwd|secret|token)\s*[=:]\s*)(\S+)"
)


def redact(text: str) -> str:
    return REDACT.sub(lambda m: m.group(1) + "<redacted>", text)


def sha256_of(path: pathlib.Path) -> tuple[str, int]:
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
            size += len(chunk)
    return digest.hexdigest(), size


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--date", default=_dt.date.today().isoformat())
    parser.add_argument("--suite", required=True, help="suite / feature id, e.g. FR-24, P0")
    parser.add_argument("--title", required=True)
    parser.add_argument("--result", required=True,
                        choices=["PASS", "FAIL", "BLOCKED", "SKIPPED", "INFO"])
    parser.add_argument("--command", action="append", default=[],
                        help="command or operation; repeat for multi-step runs")
    parser.add_argument("--evidence", action="append", default=[],
                        help="evidence file path; repeat for multiple files")
    parser.add_argument("--criteria", default="")
    parser.add_argument("--gate-impact", default="none")
    parser.add_argument("--notes", default="")
    parser.add_argument("--missing-evidence", action="store_true",
                        help="explicitly record that a run had no durable evidence")
    args = parser.parse_args()

    try:
        _dt.date.fromisoformat(args.date)
    except ValueError:
        parser.error("--date must be YYYY-MM-DD")
    if not args.command:
        parser.error("at least one --command is required")
    if not args.evidence and not args.missing_evidence:
        parser.error("provide --evidence or explicitly pass --missing-evidence")

    evidence_lines: list[str] = []
    for raw in args.evidence:
        path = pathlib.Path(raw)
        if not path.is_absolute():
            path = ROOT / path
        if not path.is_file():
            if args.missing_evidence:
                evidence_lines.append(f"- MISSING: `{raw}`")
                continue
            parser.error(f"evidence file not found: {raw}")
        digest, size = sha256_of(path)
        try:
            rel = path.resolve().relative_to(ROOT).as_posix()
        except ValueError:
            rel = str(path.resolve())
        evidence_lines.append(
            f"- `{rel}` — {size} B, sha256 `{digest}`"
        )

    ARCHIVE_DIR.mkdir(parents=True, exist_ok=True)
    out = ARCHIVE_DIR / f"TEST-RUNS-{args.date}.md"
    header = (
        f"# Test run archive — {args.date}\n\n"
        "> Append-only. Every real test run must be recorded here before its result "
        "is cited. Raw reports may be gitignored; this ledger is versioned. "
        "Never store passwords, tokens or private keys.\n\n"
    )
    new_file = not out.exists()
    stamp = _dt.datetime.now().astimezone().isoformat(timespec="seconds")
    lines = [
        "" if not new_file else "",
        f"## {args.suite} — {args.title}",
        "",
        f"- recorded_at: `{stamp}`",
        f"- result: **{args.result}**",
        f"- criteria: {redact(args.criteria) or '—'}",
        f"- gate_impact: {redact(args.gate_impact) or 'none'}",
        "- command / operation:",
    ]
    lines.extend(f"  - `{redact(cmd)}`" for cmd in args.command)
    lines.append("- evidence:")
    lines.extend("  " + line for line in evidence_lines)
    if args.notes:
        lines.append(f"- notes: {redact(args.notes)}")
    lines.append("")

    with out.open("a", encoding="utf-8") as handle:
        if new_file:
            handle.write(header)
        handle.write("\n".join(lines))
    print(out.relative_to(ROOT).as_posix())
    return 0


if __name__ == "__main__":
    sys.exit(main())
