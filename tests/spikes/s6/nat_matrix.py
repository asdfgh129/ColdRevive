#!/usr/bin/env python3
"""W3.5① — S6 真实 NAT 唤醒证据 harness（ADR-016 后按 eMule 式可达语义汇总）。

**这是执行协议，不是结论。** 2026-11-17 起判据为 [ADR-016]：不再穷举 4 类 NAT + CGNAT，
改为证明「每个节点都有一条可用的路，且失败可预知」（HighID 直连 / LowID 只走中继）。
`required_classes` 降级为**描述性类别清单**，不再是 pass 前提。

不变的红线（tests/spikes/README.md §6 / spikes.json gate_discipline / ADR-012 / ADR-013）：
  * 条款①两端分处两台不同物理主机、条款② NAT 是真实设备——一条没松；
  * 每条路径 ≥30 次真实唤醒，成功只认真实 `WAKEUP_ACK(status=OK)`；
  * 脚本**永不**修改 spikes.json 的 status，也**永不**自动置 pass；
  * 缺失 / 失败 / 超时一律 `BLOCKED` 或计入失败，不回填「看起来通过」的默认值；
  * 不承诺任何可达率数字；`s6_status=not_run`、`g2_claim=false`、
    `evidence_valid_for_pass=false` 恒成立；
  * G2 只能写「按 eMule 语义结案，带已知限制」，不得写「NAT 矩阵通过」。

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
import tempfile
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
    # ADR-016：这里的类别只用于标注报告 / 描述性清单与 attestation 校验，
    # **不是** pass 前提。是否达标由 reach_evidence_state() 按可达语义判定。
    return [c["id"] for c in cfg["required_classes"]]


# ---------------------------------------------------------------------------
# 证据层级：把「两台物理主机」和「真实 NAT 设备」拆成两个**互相独立**的条款
#
# 混成一个 lab_mode 布尔值会掩盖一个关键事实：加主机只能满足第一条，永远满足不了
# 第二条（合成 NAT 不因换机器而变成真实设备）。分层后，报告能直接说出「缺哪一条」。
# ---------------------------------------------------------------------------

EVIDENCE_SCOPES = {
    "real-multi-nat": {
        "desc": "真实 NAT 设备 + 两端在不同物理主机（S6 判据要求的唯一合法层级）",
        "requires": ["two_distinct_machines", "nat_is_real_device"],
    },
    "cross-host-lab": {
        "desc": "合成 NAT（内核 conntrack 改写）+ 两端分处不同物理主机",
        "requires": ["two_distinct_machines"],
    },
    "vm-namespace-lab": {
        "desc": "合成 NAT + 两端在同一台主机的两个 netns",
        "requires": [],
    },
}


def resolve_scope(args: argparse.Namespace) -> str:
    """`--lab-mode` 是 `--evidence-scope vm-namespace-lab` 的旧别名（保留兼容）。"""
    scope = getattr(args, "evidence_scope", "") or ""
    legacy = bool(getattr(args, "lab_mode", False))
    if scope and legacy:
        raise ValueError("--evidence-scope 与 --lab-mode 不能同时使用（--lab-mode 等价于 vm-namespace-lab）")
    if legacy:
        return "vm-namespace-lab"
    return scope or "real-multi-nat"


def clause_state(att: dict, scope: str) -> dict:
    """逐条判定两个独立条款，而不是给一个笼统的 valid/invalid。"""
    mids = att.get("machine_ids") if isinstance(att.get("machine_ids"), list) else []
    # `machine_ids` 形如 `<ns>@<host>`：判「两台主机」必须比 **host 部分**。
    # 同机两个 netns 会给出两个不同字符串，但 host 相同 —— 那不是两台主机。
    hosts = sorted({m.rsplit("@", 1)[-1] for m in mids if isinstance(m, str) and m})
    distinct = len(hosts) >= 2
    real_nat = att.get("nat_is_real_device") is True
    return {
        "two_distinct_machines": {
            "satisfied": distinct,
            "hosts": hosts,
            "machine_ids": sorted({m for m in mids if isinstance(m, str) and m}),
            "note": ("判据要求每类环境在两台不同物理主机上各启动 coldrevived；"
                     "判定比 machine_ids 的 host 部分，同机多 netns 不算"),
        },
        "nat_is_real_device": {
            "satisfied": real_nat,
            "declared": att.get("nat_is_real_device"),
            "note": ("真实家用/运营商 NAT 设备的可核查证据；由 iptables/netns 合成的 NAT "
                     "无论换多少台主机都不是真实设备"),
        },
    }


def scope_invalid_reasons(att: dict, scope: str) -> list[str]:
    st = clause_state(att, scope)
    reasons = []
    for key in EVIDENCE_SCOPES[scope]["requires"]:
        if not st[key]["satisfied"]:
            reasons.append({
                "two_distinct_machines": "缺少两台不同物理主机（machine_ids 不足 2 个不同值）",
                "nat_is_real_device": "NAT 不是真实设备（attestation 未声明 nat_is_real_device=true）",
            }[key])
    if scope != "real-multi-nat":
        reasons.append(f"证据层级为 {scope}：{EVIDENCE_SCOPES[scope]['desc']}，不满足 S6 判据")
    return reasons


# ---------------------------------------------------------------------------
# Attestation 校验：只检查「真实环境证据是否齐备」，绝不据自报类型宣称分类成立
# ---------------------------------------------------------------------------

def validate_attestation(att: dict, cfg: dict, want_class: str) -> tuple[bool, list[str]]:
    errors: list[str] = []
    spec = next((c for c in cfg["required_classes"] if c["id"] == want_class), None)
    if spec is None:
        # CGNAT 等「附加环境」不在四类里，但同样是必测项；只认配置里显式声明过的 id
        add = cfg.get("additional_environment") or {}
        if add.get("id") == want_class:
            spec = add
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

def run_trials(template: str, trials: int, timeout: float, reset_cmd: str = "") -> dict:
    """逐次执行真实唤醒命令并统计成功率。

    `reset_cmd`：每次试验**前**执行的状态复位命令（可选）。实验室里受限锥网关用
    ipset 记录「已接触过的对端」，条目有 600s 超时；若不复位，第 1 次试验打开过滤项后
    余下 29 次会因条目残留而"轻易通过"，成功率就不再说明打洞本身有效。复位失败
    如实记进该行的 `reset_error`，不影响判定口径（失败仍按失败计）。
    """
    observations: list[dict] = []
    succeeded = 0
    for i in range(1, trials + 1):
        cmd = template.replace("{trial}", str(i))
        row: dict = {"trial": i, "command": cmd}
        if reset_cmd:
            try:
                rp = subprocess.run(reset_cmd, shell=True, capture_output=True,
                                    text=True, timeout=min(timeout, 10.0), check=False)
                row["reset_exit_code"] = rp.returncode
                if rp.returncode != 0:
                    row["reset_error"] = ((rp.stderr or rp.stdout or "").strip()[:200]
                                          or f"exit {rp.returncode}")
            except (subprocess.TimeoutExpired, OSError) as exc:
                row["reset_error"] = f"{type(exc).__name__}: {exc}"
        try:
            p = subprocess.run(cmd, shell=True, capture_output=True, text=True,
                               timeout=timeout, check=False)
            row["exit_code"] = p.returncode
            # 记 stderr：CLI 以非 0 退出且 stdout 为空时（`coldrevive-cli` 的错误走 stderr），
            # 只留「CLI 无输出」会让失败原因彻底不可追溯。
            err_text = (p.stderr or "").strip()
            if err_text:
                row["stderr"] = err_text[:300]
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

    try:
        scope = resolve_scope(args)
    except ValueError as exc:
        return ({"status": "BLOCKED", "reason": str(exc),
                 "evidence_valid": False, "s6_status": "not_run", "g2_claim": False},
                EXIT_BLOCKED)
    # 层级必须由 attestation 自报，禁止把弱层级静默上报成强层级：
    #   * 声明了非 real 层级 → attestation 必须带 lab_mode=true 或 evidence_scope 同名
    #   * 声明 real 层级但 attestation 自己写了 lab_mode=true → 明显矛盾，拒收
    declared = att.get("evidence_scope") or ("vm-namespace-lab" if att.get("lab_mode") else "")
    if scope == "real-multi-nat":
        if att.get("lab_mode") or (declared and declared != "real-multi-nat"):
            return ({"status": "BLOCKED",
                     "reason": (f"attestation 声明证据层级 {declared or 'lab_mode=true'}，"
                                "但未按其层级运行；不得据此产出真实证据"),
                     "evidence_valid": False, "s6_status": "not_run", "g2_claim": False},
                    EXIT_BLOCKED)
    else:
        if declared != scope:
            return ({"status": "BLOCKED",
                     "reason": (f"--evidence-scope {scope} 要求 attestation 显式声明 "
                                f"evidence_scope={scope}（或 lab_mode=true）；单侧声明即拒收，"
                                "避免把实验室证据当真实证据"),
                     "evidence_valid": False, "s6_status": "not_run", "g2_claim": False},
                    EXIT_BLOCKED)
    clauses = clause_state(att, scope)
    invalid_reasons = scope_invalid_reasons(att, scope)

    paths = {
        "direct": run_trials(args.direct_cmd, args.trials, args.timeout,
                             getattr(args, "inter_trial_reset", "")),
        "relay": run_trials(args.relay_cmd, args.trials, args.timeout,
                            getattr(args, "inter_trial_reset", "")),
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
        "evidence_valid": scope == "real-multi-nat" and not invalid_reasons,
        "evidence_scope": scope,
        "evidence_scope_desc": EVIDENCE_SCOPES[scope]["desc"],
        "evidence_clauses": clauses,
        "evidence_invalid_reasons": invalid_reasons,
        "lab_mode": scope != "real-multi-nat",
        "synthetic_evidence": False,
        "evidence_valid_for_pass": False,
        "status": "OBSERVED",
        "spike_status_changed": False,
        "s6_status": "not_run",
        "g2_claim": False,
        "note": ("实测记录：唤醒计数全部来自真实 WAKEUP_ACK。证据层级 "
                 f"{scope}（{EVIDENCE_SCOPES[scope]['desc']}）"
                 + ("；满足 S6 证据条款，但仍由实验执行人手工回填结论"
                    if scope == "real-multi-nat" and not invalid_reasons else
                    "；不满足条款：" + "；".join(invalid_reasons))
                 + "。脚本不置 pass、不宣称 G2 达标"),
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
        "s6_reach_stance": cfg.get("s6_reach_stance", "emule-like-reach"),
        "class_coverage_required_for_pass": cfg.get("class_coverage_required_for_pass", False),
        "rows": rows,
        "cgnat": {"status": "BLOCKED", "reason": "未检出 CGNAT 出口；ADR-016 后仅作描述性样本，不判 S6 失败",
                  "evidence_valid": False},
        "matrix_coverage": "0 / 4 类真实 NAT（含 CGNAT 环境 0/1）—— 描述性清单，非 pass 前提",
        "matrix_coverage_numeric": {"classes": 0, "required": 4, "cgnat": 0, "cgnat_required": 1,
                                    "required_for_pass": False},
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
        "decision": "结论由实验执行人依 evidence_required 手工回填 spikes.json；声明口径见 s6_claim_wording",
        "s6_claim_wording": cfg.get("s6_claim_wording", "按 eMule 语义结案，带已知限制"),
        "next_actions": [
            "用现有两台物理主机 + 真实 NAT 设备补跑 eMule 式可达证据：至少一份 direct 可达、一份 direct 失败但 relay 送达",
            "补跑时由实验执行人签 attestation（真实网关 WAN IP / 两端内网 IP / 两台 machine_id / 真实 STUN 分类报告），层级必须 real-multi-nat",
            "两条路径各 ≥30 次真实唤醒，同时记录唤醒前的可达性声明查询结果（ADR-013）",
            "--aggregate 按 HighID / LowID 两半汇总；类别覆盖只作描述性清单，未覆盖类别写入已知限制",
        ],
    }


# ---------------------------------------------------------------------------
# 聚合（ADR-016）：按 eMule 式可达语义判定证据齐备；类别只作描述性清单。
# 只汇总真实报告；即便齐备也不置 pass。
# ---------------------------------------------------------------------------

REAL_REPORT_SCHEMA_PREFIX = "coldrevive.s6.nat-matrix.class-real"


def collect_real_reports(folder: Path) -> list[dict]:
    """递归读取真实矩阵报告；非报告文件跳过，损坏文件不参与判定。"""
    out: list[dict] = []
    for p in sorted(folder.rglob("*real.json")):
        try:
            d = json.loads(p.read_text(encoding="utf-8"))
        except (ValueError, OSError):
            continue  # 损坏/不可读报告不得当作证据
        if not isinstance(d, dict) or not str(d.get("schema", "")).startswith(REAL_REPORT_SCHEMA_PREFIX):
            continue
        out.append({"file": p.name, "data": d})
    return out


def reach_evidence_state(reports: list[dict], cfg: dict) -> dict:
    """按 ADR-016 判定 HighID / LowID 两半是否各有真实证据（纯计算，不写结论）。

    字段缺失、层级不是 real-multi-nat、试验次数不足一律记为 rejected，
    绝不回填默认值 —— 与「缺失/失败一律 BLOCKED」的纪律一致。
    """
    min_trials = cfg["min_trials_per_path"]
    usable: list[dict] = []
    rejected: list[dict] = []
    for r in reports:
        d = r["data"]
        paths = d.get("paths") or {}
        reasons: list[str] = []
        if d.get("evidence_valid") is not True:
            reasons.append("evidence_valid!=true")
        if d.get("synthetic_evidence") is not False:
            reasons.append("synthetic_evidence!=false")
        if d.get("evidence_scope") != "real-multi-nat":
            reasons.append("evidence_scope!=real-multi-nat")
        for name in ("direct", "relay"):
            q = paths.get(name) or {}
            if not isinstance(q.get("trials"), int) or q["trials"] < min_trials:
                reasons.append(f"{name}.trials<{min_trials}")
            if not isinstance(q.get("succeeded"), int):
                reasons.append(f"{name}.succeeded 缺失")
        if reasons:
            rejected.append({"file": r["file"], "reasons": reasons})
            continue
        usable.append({
            "file": r["file"], "class": d.get("class"),
            "direct_succeeded": paths["direct"]["succeeded"],
            "relay_succeeded": paths["relay"]["succeeded"],
        })
    highid = [u["file"] for u in usable if u["direct_succeeded"] > 0]
    lowid = [u["file"] for u in usable
             if u["direct_succeeded"] == 0 and u["relay_succeeded"] > 0]
    both_failed = [u["file"] for u in usable
                   if u["direct_succeeded"] == 0 and u["relay_succeeded"] == 0]
    return {
        "criterion": "emule-like-reach (ADR-016)",
        "usable_reports": usable,
        "rejected_reports": rejected,
        "highid_reports": highid,
        "lowid_reports": lowid,
        "both_paths_failed": both_failed,
        "known_limitations": [f"{f}: direct 与 relay 两条都失败" for f in both_failed],
        "complete": bool(highid) and bool(lowid),
        "declaration_consistency_source": "ADR-013 / W5.2 21/21×3 真实证据；补跑时须同时记录声明查询结果",
    }


def aggregate(folder: Path, cfg: dict, condition_path: str | None = None) -> dict:
    reports = collect_real_reports(folder)
    reach = reach_evidence_state(reports, cfg)
    required = class_ids(cfg)
    found: dict[str, str | None] = {}
    for cid in required:
        found[cid] = next((r["file"] for r in reports
                           if r["data"].get("class") == cid
                           and r["data"].get("evidence_valid") is True), None)
    cgnat_files = [r["file"] for r in reports
                   if r["data"].get("class") == "cgnat"
                   and r["data"].get("evidence_valid") is True]
    missing = [c for c in required if found[c] is None]
    coverage = len([c for c in required if found[c]])
    # 描述性：类别清单是否凑齐，与 pass 无关（ADR-016）。
    matrix_complete = not missing and bool(cgnat_files)
    complete = reach["complete"]
    overall = None
    if complete:
        picked = set(reach["highid_reports"]) | set(reach["lowid_reports"])
        vals = [r["data"].get("overall_reachability") for r in reports if r["file"] in picked]
        if vals and all(isinstance(v, (int, float)) for v in vals):
            overall = sum(vals) / len(vals)
    return {
        "schema": "coldrevive.s6.nat-matrix.aggregate/v2",
        "harness": "tests/spikes/s6/nat_matrix.py",
        "recorded_at": now(),
        "mode": "aggregate",
        "source_dir": str(folder),
        "s6_reach_stance": cfg.get("s6_reach_stance", "emule-like-reach"),
        "class_coverage_required_for_pass": cfg.get("class_coverage_required_for_pass", False),
        "matrix_complete_is_pass_condition": False,
        "reach_evidence": reach,
        "rows": found,
        "missing_classes": missing,
        "missing_classes_note": "描述性清单，不构成门禁失败（ADR-016）",
        "cgnat_evidence": bool(cgnat_files),
        "matrix_coverage": (f"{coverage} / 4 类真实 NAT（含 CGNAT 环境 {int(bool(cgnat_files))}/1）"
                            " —— 描述性，非 pass 前提"),
        "matrix_complete": matrix_complete,
        "aggregate_reachability": overall,
        "r04_stance": cfg.get("r04_stance", "best-effort"),
        "r04_numeric_commitment": cfg.get("r04_numeric_commitment"),
        "reachability_note": ("aggregate_reachability 是**描述性统计**（须与样本量、观测环境一并读），"
                              "不是达标线。产品承诺的是「每个节点都有一条可用的路 + 失败可解释」，"
                              "不是百分比（ADR-012 / ADR-016）。"),
        "status": "OBSERVED" if complete else "BLOCKED",
        "evidence_valid": complete,
        "evidence_valid_for_pass": False,
        "s6_status": "not_run",
        "g2_claim": False,
        "spike_status_changed": False,
        "no_auto_pass": True,
        "s6_claim_wording": cfg.get("s6_claim_wording", "按 eMule 语义结案，带已知限制"),
        "decision": ("结论由实验执行人依 evidence_required 手工回填 spikes.json；"
                     "满足本判据后 G2 只能写「按 eMule 语义结案，带已知限制」"),
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
    # 自检必须与报告目录里当前的真实证据无关，否则补跑后会把「已有真实证据」误判成自检失败。
    with tempfile.TemporaryDirectory() as empty_dir:
        ag = aggregate(Path(empty_dir), cfg)
    checks.append(("aggregate_incomplete_is_blocked",
                   ag["status"] == "BLOCKED" and ag["matrix_complete"] is False
                   and ag["aggregate_reachability"] is None))
    checks.append(("aggregate_never_claims_g2",
                   ag["g2_claim"] is False and ag["s6_status"] == "not_run"
                   and ag["spike_status_changed"] is False))
    checks.append(("no_auto_pass_flag", cfg["evidence_policy"]["no_auto_pass"] is True))
    # ADR-012：R-04 是「尽力」，不是 SLA。这条断言的作用是防止阈值被好心加回来。
    checks.append(("r04_has_no_numeric_threshold",
                   "r04_min_reachability" not in cfg
                   and cfg.get("r04_numeric_commitment") is None
                   and ag.get("r04_numeric_commitment") is None
                   and ag.get("r04_stance") == "best-effort"))
    checks.append(("aggregate_still_never_evidence_for_pass",
                   ag["evidence_valid_for_pass"] is False))

    # ADR-016：eMule 式可达语义（纯逻辑，synthetic=true，绝不当作证据）
    checks.append(("s6_stance_is_emule_like_reach",
                   cfg.get("s6_reach_stance") == "emule-like-reach"
                   and cfg.get("class_coverage_required_for_pass") is False
                   and cfg["evidence_policy"].get("class_coverage_required_for_pass") is False))
    checks.append(("class_coverage_not_a_pass_condition",
                   ag["class_coverage_required_for_pass"] is False
                   and ag["matrix_complete_is_pass_condition"] is False
                   and cfg["additional_environment"]["required"] is False))

    def fake_report(name: str, direct_ok: int, relay_ok: int, **over: object) -> dict:
        d = {
            "schema": "coldrevive.s6.nat-matrix.class-real/v1",
            "class": "port-restricted-cone",
            "evidence_valid": True,
            "synthetic_evidence": False,
            "evidence_scope": "real-multi-nat",
            "paths": {
                "direct": {"trials": 30, "succeeded": direct_ok},
                "relay": {"trials": 30, "succeeded": relay_ok},
            },
            "overall_reachability": 1.0,
        }
        d.update(over)
        return {"file": name, "data": d}

    both = reach_evidence_state(
        [fake_report("highid.json", 30, 0), fake_report("lowid.json", 0, 30)], cfg)
    checks.append(("reach_evidence_needs_both_halves",
                   both["complete"] is True
                   and len(both["highid_reports"]) == 1 and len(both["lowid_reports"]) == 1))
    checks.append(("reach_evidence_lowid_only_incomplete",
                   reach_evidence_state([fake_report("lowid.json", 0, 30)], cfg)["complete"] is False))
    weak = reach_evidence_state(
        [fake_report("highid.json", 30, 0),
         fake_report("lowid.json", 0, 30, evidence_scope="cross-host-lab")], cfg)
    checks.append(("lab_scope_rejected_from_reach_evidence",
                   weak["complete"] is False and len(weak["rejected_reports"]) == 1))
    broken = fake_report("broken.json", 0, 30)
    broken["data"]["paths"]["direct"] = {"trials": 30}
    lacking = reach_evidence_state([fake_report("highid.json", 30, 0), broken], cfg)
    checks.append(("missing_succeeded_never_defaulted",
                   lacking["complete"] is False
                   and any("succeeded 缺失" in " ".join(r["reasons"])
                           for r in lacking["rejected_reports"])))

    # 证据层级：两个条款必须能被独立判定（这是本轮新增的判据语义）
    st_same_host = clause_state(
        {"machine_ids": ["lan-restricted@vm221", "lan-port@vm221"]}, "vm-namespace-lab")
    checks.append(("same_host_two_netns_is_not_two_hosts",
                   st_same_host["two_distinct_machines"]["satisfied"] is False
                   and st_same_host["two_distinct_machines"]["hosts"] == ["vm221"]))
    st_two = clause_state(
        {"machine_ids": ["holder@DESKTOP-DJQSC95", "requester@ubuntu-221"]}, "cross-host-lab")
    checks.append(("two_distinct_hosts_detected",
                   st_two["two_distinct_machines"]["satisfied"] is True
                   and st_two["nat_is_real_device"]["satisfied"] is False))
    checks.append(("synthetic_nat_never_counts_as_real_device",
                   clause_state({"machine_ids": ["a@h1", "b@h2"],
                                 "nat_is_real_device": False}, "real-multi-nat")
                   ["nat_is_real_device"]["satisfied"] is False))
    checks.append(("scope_invalid_reasons_name_both_clauses",
                   len(scope_invalid_reasons({"machine_ids": ["a@h1", "b@h1"]},
                                             "real-multi-nat")) >= 2))
    ns = argparse.Namespace(evidence_scope="", lab_mode=True)
    checks.append(("legacy_lab_mode_maps_to_vm_namespace",
                   resolve_scope(ns) == "vm-namespace-lab"))
    conflict_rejected = False
    try:
        resolve_scope(argparse.Namespace(evidence_scope="cross-host-lab", lab_mode=True))
    except ValueError:
        conflict_rejected = True
    checks.append(("conflicting_scope_flags_rejected", conflict_rejected))
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
    ap.add_argument("--lab-mode", action="store_true",
                    help="[兼容别名] 等价于 --evidence-scope vm-namespace-lab")
    ap.add_argument("--evidence-scope", default="",
                    choices=["", *EVIDENCE_SCOPES.keys()],
                    help=("证据层级：real-multi-nat（真实 NAT 设备 + 两台物理主机，唯一可判 pass 的层级）/"
                          "cross-host-lab（合成 NAT + 两端不同主机）/"
                          "vm-namespace-lab（合成 NAT + 同机两个 netns）。缺省 real-multi-nat"))
    ap.add_argument("--inter-trial-reset", default="",
                    help="每次试验前执行的状态复位命令（实验室用：清空受限锥网关的 ipset "
                         "接触记录，避免第 1 次打开过滤项后余下试验靠残留状态'通过'）")
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
    if report.get("evidence_scope"):
        print(f"  evidence_scope={report['evidence_scope']}"
              f"  hosts={report.get('evidence_clauses', {}).get('two_distinct_machines', {}).get('hosts')}")
        for r in report.get("evidence_invalid_reasons") or []:
            print(f"  - 不满足: {r}")
    if "matrix_coverage" in report:
        print(f"  matrix_coverage={report['matrix_coverage']}")
    print("report:", out)
    return code


if __name__ == "__main__":
    raise SystemExit(main())
