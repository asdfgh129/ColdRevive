#!/usr/bin/env python3
"""R-02 公网运营速率实验执行包装器（DOC-35 决定 6 + 7=C）。

职责（顺序固定，任一步失败即 BLOCKED，绝不自动 pass）：
  1. 字节级 sha256 校验预注册判据文件与 spikes.json 登记值一致；
  2. 校验 bootstrap 清单新鲜度（<= 判据 bootstrap_max_age_hours）且全为公网 IPv4；
  3. 运行 `kad-mini r02-live --check` 门禁，passed 才继续；
  4. （仅当 --execute）调用不带 --check 的真实公网双轨发布，落盘报告；
  5. 归档：写 MANIFEST.json（含各文件 sha256），不判 pass、不回填 spikes.json。

默认只做 1-3（dry-run）。真实对外写入必须显式 --execute 且 --attest。
"""
import argparse
import datetime as dt
import hashlib
import ipaddress
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
CRITERIA = os.path.join(ROOT, "tests", "fixtures", "r02-operational-rate.toml")
SPIKES = os.path.join(HERE, "..", "spikes.json")
EVID = os.path.join(ROOT, "tests", "ARCHIVE", "evidence")
MINI = os.path.join(HERE, "kad-mini", "target", "debug", "kad-mini.exe")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def global_ipv4(s):
    try:
        a = ipaddress.IPv4Address(s)
    except ValueError:
        return False
    return (a.is_global and not a.is_private and not a.is_loopback
            and not a.is_link_local and not a.is_multicast and not a.is_reserved
            and not a.is_unspecified)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--bootstrap", required=True)
    ap.add_argument("--execute", action="store_true", help="really send public KAD packets")
    ap.add_argument("--attest", action="store_true", help="operator attestation for live run")
    ap.add_argument("--out", help="evidence dir name (default: auto)")
    args = ap.parse_args()

    result = {"schema": "coldrevive.r02.run-wrapper/v1",
              "generated_at": dt.datetime.now(dt.timezone.utc).isoformat(),
              "execute_requested": args.execute, "steps": [], "verdict": "BLOCKED"}

    def step(name, ok, detail):
        result["steps"].append({"step": name, "ok": bool(ok), "detail": detail})
        return ok

    with open(SPIKES, encoding="utf-8") as f:
        spikes = json.load(f)
    auth = spikes.get("r02_operational_rate_review", {})

    # 1. criteria sha256 byte-exact
    if not os.path.exists(CRITERIA):
        step("criteria_file_exists", False, CRITERIA); return dump(result)
    actual = sha256(CRITERIA)
    registered = auth.get("criteria_sha256", "")
    step("criteria_sha256", actual == registered, {"actual": actual, "registered": registered})
    if actual != registered:
        return dump(result)

    # 2. bootstrap freshness + public-only
    with open(CRITERIA, encoding="utf-8") as f:
        criteria_text = f.read()
    import re
    m = re.search(r"bootstrap_max_age_hours\s*=\s*(\d+)", criteria_text)
    max_age_h = int(m.group(1)) if m else 48
    age_h = (dt.datetime.now().timestamp() - os.path.getmtime(args.bootstrap)) / 3600.0
    step("bootstrap_freshness", age_h <= max_age_h, {"age_hours": round(age_h, 2), "max_hours": max_age_h})
    with open(args.bootstrap, encoding="utf-8") as f:
        nodes = json.load(f)
    bad = [n for n in nodes if not global_ipv4(str(n.get("ip", ""))) or int(n.get("port", 0)) <= 0]
    step("bootstrap_public_only", not bad and len(nodes) >= 10,
         {"nodes": len(nodes), "bad": bad[:5]})
    if age_h > max_age_h or bad or len(nodes) < 10:
        return dump(result)

    if not os.path.exists(MINI):
        step("binary_exists", False, MINI); return dump(result)

    base = [MINI, "r02-live", "--bootstrap", args.bootstrap,
            "--criteria-sha256", actual, "--attest-i-am-the-experiment-operator"]

    # 3. gate check
    chk = subprocess.run(base + ["--check"], capture_output=True, text=True, cwd=HERE)
    try:
        rep = json.loads([l for l in chk.stdout.splitlines() if l.startswith("{")][-1])
        gate_ok = rep.get("gate", {}).get("passed", False)
    except Exception as exc:
        gate_ok = False
        rep = {"parse_error": str(exc), "stdout": chk.stdout[:500], "stderr": chk.stderr[:500]}
    step("gate_check", gate_ok, rep)
    if not gate_ok:
        return dump(result)

    if not args.execute:
        result["verdict"] = "DRY_RUN_GATE_PASSED"
        return dump(result)

    if not args.attest:
        step("execute_attestation", False, "需要 --attest 才执行不可逆公网发布")
        return dump(result)

    # 4. real run
    out_name = args.out or ("r02-operational-rate-" + dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ"))
    outdir = os.path.join(EVID, out_name)
    os.makedirs(outdir, exist_ok=True)
    live = subprocess.run(base, capture_output=True, text=True, cwd=HERE)
    report_path = os.path.join(outdir, "r02-live.json")
    with open(report_path, "w", encoding="utf-8", newline="\n") as f:
        f.write(live.stdout[live.stdout.find("{"):] if "{" in live.stdout else live.stdout)
    with open(os.path.join(outdir, "r02-live.stderr.txt"), "w", encoding="utf-8", newline="\n") as f:
        f.write(live.stderr)
    step("live_run_capture", os.path.getsize(report_path) > 0,
         {"exit": live.returncode, "report": report_path})

    # 5. manifest: sha of every artifact, NO pass verdict
    manifest = {"schema": "coldrevive.r02.evidence-manifest/v1",
                "created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
                "criteria_sha256": actual,
                "bootstrap_sha256": sha256(args.bootstrap),
                "binary_sha256": sha256(MINI),
                "git_commit": git_commit(),
                "files": {}}
    for fn in sorted(os.listdir(outdir)):
        fp = os.path.join(outdir, fn)
        if os.path.isfile(fp):
            manifest["files"][fn] = sha256(fp)
    with open(os.path.join(outdir, "MANIFEST.json"), "w", encoding="utf-8", newline="\n") as f:
        json.dump(manifest, f, ensure_ascii=False, indent=2)
    result["evidence_dir"] = outdir
    result["verdict"] = "EXECUTED_NEEDS_HUMAN_VERDICT"
    return dump(result)


def git_commit():
    try:
        return subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    except Exception:
        return "unknown"


def dump(r):
    print(json.dumps(r, ensure_ascii=False, indent=2))
    return 0 if r["verdict"] in ("DRY_RUN_GATE_PASSED", "EXECUTED_NEEDS_HUMAN_VERDICT") else 2


if __name__ == "__main__":
    sys.exit(main())
