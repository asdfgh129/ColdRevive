#!/usr/bin/env python3
"""W3.5① — S6 四类真实 NAT（含 CGNAT）可达率矩阵 harness。

**这是矩阵执行协议，不是矩阵结论。** 本机只有一个真实 cone 类 NAT 出口，
因此默认 `--dry-run` 会把四类 NAT + CGNAT 全部如实判为 `BLOCKED`：
`matrix_coverage = 0 / 4`、`evidence_valid=false`、`s6_status=not_run`、`g2_claim=false`。

红线（tests/spikes/README.md §6 / spikes.json gate_discipline）：
  * 脚本**永不**修改 spikes.json 的 status，也**永不**自动置 pass；
  * 缺失 / 失败 / 超时一律 `BLOCKED` 或计入失败，不回填「看起来通过」的默认值；
  * 达率只由真实 `WAKEUP_ACK` 计数得出；attestation 自报类型不能替代真实 STUN 分类报告；
  * 即使四类 + CGNAT 证据齐备，聚合结果仍标 `s6_status=not_run`，由实验执行人手工回填。

用法：
  python3 tests/spikes/s6/nat_matrix.py --self-test
  python3 tests/spikes/s6/nat_matrix.py --dry-run                 # 退出码 2 = 如实 BLOCKED
  python3 tests/spikes/s6/nat_matrix.py --mode real --class port-restricted-cone \
      --attest <attestation.json> --trials 30 \
      --direct-cmd 'coldrevive-cli ... nat wake ... direct' \
      --relay-cmd  'coldrevive-cli ... nat wake ... relay'
  python3 tests/spikes/s6/nat_matrix.py --aggregate <report-dir>

真实模式要求 `--attest` 由实验执行人签署（真实网关 WAN IP、两端内网 IP、两台
machine_id、真实 STUN 分类报告路径），并给出会真实执行 CLI 的命令模板。
模板中的 `{trial}` 会替换为本次试验序号；每行 stdout 必须是**一个 JSON 对象**，
且必须包含 `ok` / `ack_status` / `path` 字段，缺字段即计失败。
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
CONFIG = HERE / "nat_matrix_config.json"
REPORT_DIR = HERE / "reports"
EXIT_BLOCKED = 2


def now() -> str:
    return datetime.now(timezone.utc).isoformat()


def load_config() -> dict:
    return json.loads(CONFIG.read_text(encoding="utf-8"))


def class_ids(cfg: dict) -> list[str]:
    return [c["id"] for c in cfg["required_classes"]]


# ---------------------------------------------------------------------------
# Attestation 校验：只检查「真实环境证据是否齐备」，绝不据自报类型宣称分类成立
# ---------------------------------------------------------------------------

def validate_attestation(att: dict, cfg: dict, want_class: str) -> tuple[bool, list[str]]:
    errors: list[str] = []
    spec = next((c for c in cfg["required_classes"] if c["id"] == want_class), None)
    if spec is None:
        return False, [f"未知 NAT 类别 {want_class}"]
    for key in spec["required_attestation"]:
        if key not in att or att[key] in ("", None, [], {}):
            errors.append(f"attestation 缺字段 {key}")
    if att.get("class") and att["class"] != want_class:
        errors.append(f"attestation.class={att.get('class')} 与 --class {want_class} 不一致")
    mids = att.get("machine_ids")
    if isinstance(mids, list) and len(set(mids)) < 2:
        errors.append("machine_ids 必须包含两台不同物理主机")
    for ipkey in ("gateway_wan_ip", "local_ip", "peer_ip"):
        v = att.get(ipkey)
        if isinstance(v, str) and (v.startswith("127.") or v == "0.0.0.0"):
            errors.append(f"{ipkey} 不得为回环/通配地址: {v}")
    stun = att.get("classification_report")
    if not stun:
        errors.append("缺少真实 STUN 分类报告路径 classification_report")
    elif not Path(stun).is_file():
        errors.append(f"classification_report 文件不存在: {stun}")
    if att.get("real_nat_evidence") in (None, "", False):
        errors.append("缺少 real_nat_evidence（真实 NAT 设备的可核查证据）")
    return (not errors), errors


# ---------------------------------------------------------------------------
# 真实测量：只观测真实 CLI 输出，不合成任何应答
# ---------------------------------------------------------------------------

def run_trials(template: str, trials: int, timeout: float) -> dict:
    observations: list[dict] = []
    succeeded = 0
    for i in range(1, trials + 1):
        cmd = template.replace("{trial}", str(i))
        row: dict = {"trial": i, "command": cmd}
        try:
            p = subprocess.run(cmd, shell=True, capture_output=True, text=True,
                               timeout=timeout, check=False)
            row["exit_code"] = p.returncode
            out = (p.stdout or "").strip()
            if not out:
                row["error"] = "CLI 无输出"
            else:
                try:
                    data = json.loads(out.splitlines()[-1])
                except (ValueError, TypeError) as exc:
                    data = None
                    row["error"] = f"stdout 非 JSON: {exc}"
                if isinstance(data, dict):
                    row["observed"] = data
                    ok = data.get("ok") is True and data.get("ack_status") == "OK"
                    if "ok" not in data or "ack_status" not in data or "path" not in data:
                        row["error"] = "缺 ok / ack_status / path 字段，按失败计"
                    elif ok:
                        succeeded += 1
                        row["success"] = True
                    else:
                        row["success"] = False
                elif data is not None:
                    row["error"] = "stdout 不是 JSON 对象"
        except subprocess.TimeoutExpired:
            row["error"] = f"超时 {timeout}s"
        except OSError as exc:
            row["error"] = f"{type(exc).__name__}: {exc}"
        row.setdefault("success", False)
        observations.append(row)
    return {
        "trials": trials,
        "succeeded": succeeded,
        "reachability": (succeeded / trials) if trials else None,
        "observations": observations,
        "synthetic_evidence": False,
        "note": "成功仅由真实 CLI 输出的 ok=true 且 ack_status=OK 判定；超时/缺字段/非 JSON 一律失败",
    }


def real_mode(args: argparse.Namespace, cfg: dict) -> tuple[dict, int]:
    att_path = Path(args.attest) if args.attest else None
    if not att_path or not att_path.is_file():
        return ({"status": "BLOCKED", "reason": "缺少 --attest 或文件不存在",
                 "evidence_valid": False, "s6_status": "not_run", "g2_claim": False},
                EXIT_BLOCKED)
    att = json.loads(att_path.read_text(encoding="utf-8"))
    ok, errors = validate_attestation(att, cfg, args.klass)
    if not ok:
        return ({"status": "BLOCKED", "reason": "attestation 未通过真实环境校验",
                 "errors": errors, "evidence_valid": False,
                 "s6_status": "not_run", "g2_claim": False}, EXIT_BLOCKED)
    if not args.direct_cmd or not args.relay_cmd:
        return ({"status": "BLOCKED", "reason": "缺少 --direct-cmd / --relay-cmd 真实命令模板",
                 "evidence_valid": False, "s6_status": "not_run", "g2_claim": False},
                EXIT_BLOCKED)
    if args.trials < cfg["min_trials_per_path"]:
        return ({"status": "BLOCKED",
                 "reason": f"试验次数 {args.trials} < 配置下限 {cfg['min_trials_per_path']}",
                 "evidence_valid": False, "s6_status": "not_run", "g2_claim": False},
                EXIT_BLOCKED)

    paths = {
        "direct": run_trials(args.direct_cmd, args.trials, args.timeout),
        "relay": run_trials(args.relay_cmd, args.trials, args.timeout),
    }
    d, r = paths["direct"]["reachability"], paths["relay"]["reachability"]
    overall = None if d is None or r is None else 1.0 - (1.0 - d) * (1.0 - r)
    report = {
        "schema": "coldrevive.s6.nat-matrix.class-real/v1",
        "harness": "tests/spikes/s6/nat_matrix.py",
        "recorded_at": now(),
        "class": args.klass,
        "attestation": att,
        "attestation_validated": True,
        "paths": paths,
        "overall_reachability": overall,
        "evidence_valid": True,
        "synthetic_evidence": False,
        "evidence_valid_for_pass": False,
        "status": "OBSERVED",
        "spike_status_changed": False,
        "s6_status": "not_run",
        "g2_claim": False,
        "note": "单类真实实测记录；不得据此宣称 S6 达标。结论由实验执行人手工回填 spikes.json",
    }
    return report, 0


# ---------------------------------------------------------------------------
# 干跑：如实 BLOCKED
# ---------------------------------------------------------------------------

def blocked_class(cid: str, cfg: dict, condition_path: str | None) -> dict:
    return {
        "class": cid,
        "status": "BLOCKED",
        "reason": ("本机仅有 1 个真实 cone 类 NAT 出口；缺少该类真实 NAT 设备/"
                   "两台不同物理主机/跨 NAT 双端/签妥 attestation，无法执行真实唤醒矩阵"),
        "paths": {"direct": {"trials": 0, "reachability": None},
                  "relay": {"trials": 0, "reachability": None}},
        "evidence_valid": False,
    }


def dry_run(cfg: dict, condition_path: str | None) -> dict:
    rows = [blocked_class(cid, cfg, condition_path) for cid in class_ids(cfg)]
    cond = None
    if condition_path and Path(condition_path).is_file():
        cond = json.loads(Path(condition_path).read_text(encoding="utf-8"))
    observed = []
    if cond:
        stun = cond.get("real_probes", {}).get("stun", {})
        observed = stun.get("distinct_mapped_addresses", [])
    return {
        "schema": "coldrevive.s6.nat-matrix.dry-run/v1",
        "harness": "tests/spikes/s6/nat_matrix.py",
        "milestone": cfg["milestone"],
        "recorded_at": now(),
        "mode": "dry-run",
        "status": "BLOCKED",
        "required_classes": class_ids(cfg),
        "rows": rows,
        "cgnat": {"status": "BLOCKED", "reason": "未检出 CGNAT 出口，且缺少 CGNAT 真实环境与 attestation",
                  "evidence_valid": False},
        "matrix_coverage": "0 / 4 类真实 NAT（含 CGNAT 环境 0/1）",
        "matrix_coverage_numeric": {"classes": 0, "required": 4, "cgnat": 0, "cgnat_required": 1},
        "local_real_sample": {
            "available": bool(observed),
            "distinct_mapped_addresses": observed,
            "note": ("真实公网 STUN 往返只能证明本机单出口的映射/过滤行为，"
                     "不是跨 NAT 双端可达率样本，不计入矩阵覆盖率"),
            "evidence_valid_for_pass": False,
        },
        "evidence_valid": False,
        "evidence_valid_for_pass": False,
        "s6_status": "not_run",
        "g2_claim": False,
        "spike_status_changed": False,
        "no_auto_pass": True,
        "decision": "结论由实验执行人依 evidence_required 手工回填 spikes.json",
        "next_actions": [
            "在 4 类真实 NAT（含至少 1 个 CGNAT）环境各部署两台物理主机 + coldrevived + coldrevive-relay",
            "每类由实验执行人签 attestation 并附真实 STUN 分类报告",
            "用 --mode real 执行 ≥30 次真实唤醒/路径，产出逐类 real 报告",
            "--aggregate 汇总；只有 4 类 + CGNAT 齐备才计算聚合可达率",
        ],
    }


# ---------------------------------------------------------------------------
# 聚合：只汇总真实报告；即便齐备也不置 pass
# ---------------------------------------------------------------------------

def aggregate(folder: Path, cfg: dict, condition_path: str | None = None) -> dict:
    found: dict[str, dict] = {}
    required = class_ids(cfg)
    for cid in required:
        for p in sorted(folder.glob(f"*class-{cid}*real.json")):
            try:
                d = json.loads(p.read_text(encoding="utf-8"))
            except (ValueError, OSError):
                continue
            if d.get("evidence_valid") is True and d.get("synthetic_evidence", False) is False:
                found[cid] = {"report": p.name, "data": d}
                break
    cgnat_files = []
    for p in sorted(folder.glob("*cgnat*real.json")):
        try:
            d = json.loads(p.read_text(encoding="utf-8"))
        except (ValueError, OSError):
            continue  # 损坏/不可读报告不得当作证据
        if d.get("evidence_valid") is True and d.get("synthetic_evidence", False) is False:
            cgnat_files.append(p)
    missing = [c for c in required if c not in found]
    coverage = len(found)
    complete = not missing and bool(cgnat_files)
    overall = None
    if complete:
        vals = [found[c]["data"].get("overall_reachability") for c in required]
        if all(isinstance(v, (int, float)) for v in vals):
            overall = sum(vals) / len(vals)
        else:
            complete = False
    return {
        "schema": "coldrevive.s6.nat-matrix.aggregate/v1",
        "harness": "tests/spikes/s6/nat_matrix.py",
        "recorded_at": now(),
        "mode": "aggregate",
        "source_dir": str(folder),
        "rows": {c: (found[c]["report"] if c in found else None) for c in required},
        "missing_classes": missing,
        "cgnat_evidence": bool(cgnat_files),
        "matrix_coverage": f"{coverage} / 4 类真实 NAT（含 CGNAT 环境 {int(bool(cgnat_files))}/1）",
        "matrix_complete": complete,
        "aggregate_reachability": overall,
        "r04_min_reachability": cfg["r04_min_reachability"],
        "r04_threshold_note": ("仅在 matrix_complete=true 时可与阈值比较；"
                               "脚本仍不判定达标"),
        "status": "OBSERVED" if complete else "BLOCKED",
        "evidence_valid": complete,
        "evidence_valid_for_pass": False,
        "s6_status": "not_run",
        "g2_claim": False,
        "spike_status_changed": False,
        "no_auto_pass": True,
        "decision": "结论由实验执行人依 evidence_required 手工回填 spikes.json",
    }


# ---------------------------------------------------------------------------
# 自检：纯逻辑，明确标注 synthetic，绝不充当证据
# ---------------------------------------------------------------------------

def self_test(cfg: dict) -> int:
    checks: list[tuple[str, bool]] = []
    good = {
        "class": "port-restricted-cone", "gateway_wan_ip": "112.5.60.1",
        "local_ip": "192.168.1.210", "peer_ip": "10.0.0.9",
        "machine_ids": ["DESKTOP-DJQSC95", "DESKTOP-CCE53RF"],
        "real_nat_evidence": "router model / WAN capture",
        "operator": "unit-test", "recorded_at": now(),
        "classification_report": str(CONFIG),
    }
    ok, errs = validate_attestation(good, cfg, "port-restricted-cone")
    checks.append(("valid_attestation_accepted", ok and not errs))
    bad = {**good, "machine_ids": ["same", "same"]}
    ok2, errs2 = validate_attestation(bad, cfg, "port-restricted-cone")
    checks.append(("single_machine_attestation_rejected", not ok2 and any("machine_ids" in e for e in errs2)))
    ok3, _ = validate_attestation({**good, "class": "symmetric"}, cfg, "full-cone")
    checks.append(("class_mismatch_rejected", not ok3))
    ok4, _ = validate_attestation({**good, "classification_report": ""}, cfg, "port-restricted-cone")
    checks.append(("missing_classification_report_rejected", not ok4))
    dr = dry_run(cfg, None)
    checks.append(("dryrun_all_blocked",
                   all(r["status"] == "BLOCKED" for r in dr["rows"]) and dr["cgnat"]["status"] == "BLOCKED"))
    checks.append(("dryrun_no_false_evidence",
                   dr["evidence_valid"] is False and dr["g2_claim"] is False
                   and dr["s6_status"] == "not_run" and dr["spike_status_changed"] is False))
    checks.append(("dryrun_zero_coverage", dr["matrix_coverage_numeric"]["classes"] == 0))
    ag = aggregate(REPORT_DIR, cfg)
    checks.append(("aggregate_incomplete_is_blocked",
                   ag["status"] == "BLOCKED" and ag["matrix_complete"] is False
                   and ag["aggregate_reachability"] is None))
    checks.append(("aggregate_never_claims_g2",
                   ag["g2_claim"] is False and ag["s6_status"] == "not_run"
                   and ag["spike_status_changed"] is False))
    checks.append(("no_auto_pass_flag", cfg["evidence_policy"]["no_auto_pass"] is True))
    for name, passed in checks:
        print(f"  [{'PASS' if passed else 'FAIL'}] {name}")
    n = sum(1 for _, p in checks if p)
    print(f"nat_matrix self-test: {n}/{len(checks)} PASS (pure logic, synthetic=true, evidence_valid=false)")
    return 0 if n == len(checks) else 1


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--mode", choices=["real"], default="")
    ap.add_argument("--class", dest="klass", default="")
    ap.add_argument("--attest", default="")
    ap.add_argument("--trials", type=int, default=30)
    ap.add_argument("--timeout", type=float, default=20.0)
    ap.add_argument("--direct-cmd", default="")
    ap.add_argument("--relay-cmd", default="")
    ap.add_argument("--aggregate", default="")
    ap.add_argument("--condition-file", default="")
    ap.add_argument("--out", default="")
    args = ap.parse_args()
    cfg = load_config()
    REPORT_DIR.mkdir(parents=True, exist_ok=True)

    if args.self_test:
        return self_test(cfg)

    if args.aggregate:
        report = aggregate(Path(args.aggregate), cfg, args.condition_file or None)
        out = Path(args.out) if args.out else REPORT_DIR / (
            "s6-matrix-aggregate-" + datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + ".json")
        code = 0
    elif args.mode == "real":
        report, code = real_mode(args, cfg)
        out = Path(args.out) if args.out else REPORT_DIR / (
            f"s6-matrix-class-{args.klass or 'unknown'}-real.json")
    else:
        report = dry_run(cfg, args.condition_file or None)
        code = EXIT_BLOCKED
        out = Path(args.out) if args.out else REPORT_DIR / (
            "s6-matrix-dryrun-" + datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + ".json")

    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"  status={report['status']}  s6_status={report['s6_status']}  "
          f"g2_claim={report['g2_claim']}  evidence_valid={report.get('evidence_valid')}")
    if "matrix_coverage" in report:
        print(f"  matrix_coverage={report['matrix_coverage']}")
    print("report:", out)
    return code


if __name__ == "__main__":
    raise SystemExit(main())
