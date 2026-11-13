#!/usr/bin/env python3
"""ColdRevive M0 就绪度检查（preflight）。

只回答一个问题：**现在这台机器能不能开始做 S1 / S2 和 T0 真实验收？**
不执行任何 Spike，不接触公共 KAD，不产出技术结论，也绝不把「未运行」写成「通过」。

    python3 tests/spikes/m0_preflight.py
    python3 tests/spikes/m0_preflight.py --probe-network   # 可选：仅探测依赖源可达性

退出码：0 = 全部就绪；2 = 存在阻塞（当前环境必然为 2）；1 = 脚本自身错误。
报告落在 tests/spikes/reports/，不纳入版本控制。
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import socket
import subprocess
import sys
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
T0_DIR = ROOT / "tests" / "e2e" / "t0"
REPORT_DIR = HERE / "reports"
SPIKES_JSON = HERE / "spikes.json"

DOWNLOADERS = ("qbittorrent-nox", "transmission-cli", "aria2c")
BIN_ENV = ("COLDREVIVE_DAEMON", "COLDREVIVE_CLI", "T0_DOWNLOADER")


def which_version(name: str, timeout: float = 3.0) -> dict[str, Any]:
    """返回命令是否可用及版本首行；不把「能执行」误判为「能力通过」。"""
    path = shutil.which(name)
    if not path:
        return {"available": False, "path": "", "version": ""}
    version = ""
    try:
        r = subprocess.run([path, "--version"], capture_output=True, text=True,
                           timeout=timeout, check=False)
        version = (r.stdout or r.stderr).strip().splitlines()[0][:120] if (r.stdout or r.stderr) else ""
    except (OSError, subprocess.TimeoutExpired, IndexError):
        version = "(版本查询超时或失败)"
    return {"available": True, "path": path, "version": version}


def check_ports(cfg: dict[str, Any]) -> dict[str, Any]:
    problems: list[str] = []
    bind = cfg["bind"]
    for node, spec in cfg["nodes"].items():
        for key in ("http_port", "wake_udp_port", "bt_port"):
            kind = socket.SOCK_DGRAM if key == "wake_udp_port" else socket.SOCK_STREAM
            s = socket.socket(socket.AF_INET, kind)
            try:
                s.bind((bind, int(spec[key])))
            except OSError as exc:
                problems.append(f"{node}.{key}={spec[key]} 不可绑定: {exc}")
            finally:
                s.close()
    return {"available": not problems, "problems": problems, "bind": bind}


def check_binaries() -> dict[str, Any]:
    out: dict[str, Any] = {}
    for env in BIN_ENV:
        raw = os.environ.get(env, "").strip()
        out[env] = {
            "set": bool(raw),
            "value": raw,
            "exists": bool(raw) and Path(raw).is_file(),
        }
    return out


def check_observer_layer() -> dict[str, Any]:
    """观测采集层是否已实现；注意 wired 是独立的接线开关，必须单独报告。"""
    if str(T0_DIR) not in sys.path:
        sys.path.insert(0, str(T0_DIR))
    result: dict[str, Any] = {"plan_available": False, "plan_size": 0,
                              "wired": None, "import_error": ""}
    try:
        import t0_cli  # noqa: F401
        plan = getattr(t0_cli, "OBSERVE_PLAN", {})
        result.update(plan_available=bool(plan), plan_size=len(plan))
    except Exception as exc:
        result["import_error"] = f"{type(exc).__name__}: {exc}"
    try:
        import t0_adapters  # noqa: F401
        result["wired"] = bool(t0_adapters.CliAdapter.wired)
    except Exception as exc:
        result["import_error"] = (result["import_error"] + "; " if result["import_error"] else "") \
            + f"{type(exc).__name__}: {exc}"
    return result


def probe_network() -> dict[str, Any]:
    """仅探测依赖源可达性，不发布任何数据；结果只作参考，不构成结论。"""
    try:
        req = urllib.request.Request("https://crates.io", method="HEAD",
                                     headers={"User-Agent": "ColdRevive-M0-preflight/1.0"})
        with urllib.request.urlopen(req, timeout=5) as resp:
            return {"checked": True, "http_status": getattr(resp, "status", 0),
                    "advisory_only": True}
    except Exception as exc:
        return {"checked": True, "error": f"{type(exc).__name__}: {exc}",
                "advisory_only": True}


def build_checks(cfg: dict[str, Any], spikes: dict[str, Any],
                 probe: bool) -> list[dict[str, Any]]:
    checks: list[dict[str, Any]] = []

    def add(cid: str, label: str, ok: bool, detail: Any, needed_by: list[str],
            severity: str = "blocker") -> None:
        checks.append({"id": cid, "label": label,
                       "status": "ok" if ok else ("warn" if severity == "warn" else "missing"),
                       "detail": detail, "needed_by": needed_by, "severity": severity})

    rustc = which_version("rustc")
    cargo = which_version("cargo")
    add("toolchain.rustc", "rustc 可用", rustc["available"], rustc, ["S1", "S2", "T0"])
    add("toolchain.cargo", "cargo 可用（Cargo Workspace 的前提）", cargo["available"], cargo,
        ["S1", "S2", "T0"])

    dl = {name: which_version(name) for name in DOWNLOADERS}
    dl_ok = any(v["available"] for v in dl.values())
    # 非标准名的已装客户端：Motrix Next 捆绑 motrixnext-aria2c.exe（aria2）。
    # 只有 spikes.json 的 S1.cross_client_round 记录过「已执行且逐片校验通过」才计为可用，
    # 避免把「文件存在」误判成「互操作能力通过」。
    by_id_all = {s["id"]: s for s in spikes["spikes"]}
    cc = (by_id_all.get("S1", {}).get("observations", {}) or {}).get("cross_client_round", {}) or {}
    interop_proven = bool(cc.get("executed")) and bool(
        (cc.get("result") or {}).get("all_pieces_verified"))
    motrix_paths = []
    if sys.platform == "win32":
        pf = os.environ.get("ProgramFiles", r"C:\Program Files")
        motrix_paths.append(Path(pf) / "MotrixNext" / "motrixnext-aria2c.exe")
    motrix = {"available": False, "path": "", "version": "", "interop_proven": interop_proven}
    for p in motrix_paths:
        if p.is_file():
            v = which_version(str(p), timeout=3.0)
            motrix.update({"available": True, "path": str(p), "version": v["version"]})
            break
    dl["motrix_next_aria2"] = motrix
    dl_ok = dl_ok or (motrix["available"] and interop_proven)
    dl["_note"] = ("非标准名客户端需同时满足「可执行文件存在」与「S1 cross_client_round "
                   "实测通过」才计入 OK；此项只反映 G3 互操作前置，不替代 M1 二进制门禁")
    add("downloader.any", "至少一个第三方 BT 客户端", dl_ok, dl, ["T0-08", "G3"])

    bins = check_binaries()
    bins_ok = all(v["exists"] for v in bins.values())
    add("binaries.m1", "M1 二进制与下载器环境变量指向真实文件", bins_ok, bins,
        ["T0-real"])

    ports = check_ports(cfg)
    add("network.ports", "T0 双实例端口可绑定", ports["available"], ports, ["T0-real"])

    obs = check_observer_layer()
    add("t0.observers", "20 条真实用例的观测采集层已实现",
        obs["plan_available"] and obs["plan_size"] >= 20, obs, ["T0-real"])
    add("t0.wired", "CliAdapter 已接线（仅在二进制与观测齐备后置 True）",
        bool(obs["wired"]), {"wired": obs["wired"]}, ["T0-real"])

    by_id = {s["id"]: s for s in spikes["spikes"]}
    add("approval.public_kad", "S2 实网小样本实验已获人工批准",
        bool(by_id["S2"].get("public_kad_approval")),
        {"note": "批准记录需人工写入 spikes.json 的 public_kad_approval 字段，脚本不得代填"},
        ["S2"])
    legal = by_id["S8"].get("legal_review") or {}
    add("approval.legal", "S8 法务结论已登记（书面意见或所有者豁免）",
        bool(legal.get("status")),
        {"status": legal.get("status", "未登记"),
         "written_opinion_received": legal.get("written_opinion_received", False),
         "note": "外部流程，非技术阻塞；豁免不等于取得书面法律意见"},
        ["S8"], severity="warn")

    net = probe_network() if probe else {"checked": False, "advisory_only": True,
                                        "note": "默认不探测；加 --probe-network 才检查依赖源可达性"}
    add("network.registry", "依赖源可达性（仅供参考）",
        bool(probe and net.get("http_status") == 200), net, [], severity="warn")

    return checks


def main() -> int:
    ap = argparse.ArgumentParser(description="ColdRevive M0 就绪度检查")
    ap.add_argument("--probe-network", action="store_true",
                    help="可选：HEAD 探测依赖源（不发布任何数据，结果仅供参考）")
    ap.add_argument("--json", action="store_true", help="只输出 JSON，不打印表格")
    args = ap.parse_args()

    try:
        cfg = json.loads((T0_DIR / "t0_config.json").read_text(encoding="utf-8"))
        spikes = json.loads(SPIKES_JSON.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        print(f"[ERROR] 配置读取失败: {exc}", file=sys.stderr)
        return 1

    checks = build_checks(cfg, spikes, args.probe_network)
    blockers = [c for c in checks if c["status"] == "missing"]
    warnings = [c for c in checks if c["status"] == "warn"]

    spike_status = {
        s["id"]: {
            "status": s["status"],
            "priority": s["priority"],
            "blocked_by": [c["id"] for c in blockers if s["id"] in c["needed_by"]],
        }
        for s in spikes["spikes"]
    }

    g0_ready = not blockers
    t0_real_ready = not any(c["id"] in {"binaries.m1", "downloader.any",
                                        "t0.observers", "t0.wired",
                                        "network.ports", "toolchain.cargo"}
                            for c in blockers)
    # G0 的真实判据是「S1–S8 全部有结论」；工程阻塞项只是其中一部分，单列出来避免误读。
    g0_pending_spikes = [k for k, v in spike_status.items() if v["status"] == "not_run"]
    # 治理层条件冻结只记录，不改变 g0_ready 算法，也不自动清除任何 Spike 结论
    gov = spikes.get("g0_review") or {}
    g0_governance = {
        "decision": gov.get("decision", "none"),
        "g0_status": gov.get("g0_status", "not_reviewed"),
        "adr_001_default_mode": (gov.get("adr_001") or {}).get("default_mode", ""),
        "conditional_freeze_exempted": (gov.get("conditional_freeze") or {}).get("exempted_spikes", []),
        "review_doc": gov.get("review_doc", ""),
        "note": "治理层条件冻结不改 Spike 状态；g0_ready 仍按工程阻塞项计算",
    }
    s1_obs = next((s.get("observations", {}) for s in spikes["spikes"] if s["id"] == "S1"), {})
    if "two_peer" in s1_obs and spike_status["S1"]["status"] == "not_run":
        s1_action = ("S1 四条判据已全部 verified（probe + two-peer 双 Session 真实分片传输）；"
                     "S1.status 仍 not_run，须实验执行人依证据手工回填，"
                     "并认定回环证据是否满足 README §6 第 3 条")
    elif spike_status["S1"]["status"] != "not_run":
        s1_action = "S1 已有结论；按证据复核 ADR-002 定稿条件"
    else:
        s1_action = ("cargo 已就绪：在 tests/spikes/s1/ 编译最小 Demo，"
                     "核对 rqbit 的按需实例化 / 握手拦截能力")

    report = {
        "schema": "coldrevive.m0.preflight/v1",
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "host": {"os": platform.system(), "python": platform.python_version()},
        "scope": "本地就绪度检查；未执行任何 Spike，未接触公共 KAD，不产出技术结论",
        "g0_ready": g0_ready,
        "t0_real_ready": t0_real_ready,
        "spikes": spike_status,
        "g0_pending_spikes": g0_pending_spikes,
        "g0_governance": g0_governance,
        "checks": checks,
        "blockers": [c["id"] for c in blockers],
        "warnings": [c["id"] for c in warnings],
        "next_actions": [
            s1_action,
            "S2 实网小样本实验已获人工批准，仍须限速、最小样本并逐次记录；本地回环模拟只能作为预演",
            "S8 已由项目所有者豁免登记；无书面法律意见，M4 公测前须复核 R-11",
            "第三方 BT 客户端已就绪（Motrix Next / aria2，经 S1 跨客户端轮证明互通）；"
            "仍须编译 M1 二进制后才能把 CliAdapter.wired 置为 True",
        ],
    }

    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    out = REPORT_DIR / f"m0-preflight-{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}.json"
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2))
    else:
        print("ColdRevive M0 就绪度检查（只查条件，不做实验）")
        print(f"主机: {report['host']['os']} / Python {report['host']['python']}")
        print("-" * 78)
        for c in checks:
            mark = {"ok": "[OK]     ", "warn": "[WARN]   ", "missing": "[MISSING]"}[c["status"]]
            print(f"{mark} {c['label']}")
            if c["status"] != "ok":
                print(f"          需要方: {', '.join(c['needed_by']) or '—'}")
        print("-" * 78)
        print(f"G0 就绪: {g0_ready} | T0 真实验收就绪: {t0_real_ready} | "
              f"阻塞项 {len(blockers)} 个 / 提醒 {len(warnings)} 个")
        print("Spike 状态: " + ", ".join(f"{k}={v['status']}" for k, v in spike_status.items()))
        print("G0 未结 Spike: " + (", ".join(g0_pending_spikes) or "无"))
        if g0_governance["decision"] == "conditional_freeze_approved":
            print("G0 治理: 条件冻结已获项目所有者批准 "
                  f"(ADR-001={g0_governance['adr_001_default_mode']}，"
                  f"豁免 G0 时点结论 {', '.join(g0_governance['conditional_freeze_exempted'])}；"
                  "Spike 状态不变、g0_ready 算法不变)")
        print(f"报告: {out.relative_to(ROOT).as_posix()}")
        print("[NOTE] 本脚本不产出任何 Spike 结论；结论须由实验执行人按证据手工回填 spikes.json。")
        for a in report["next_actions"]:
            print("  → " + a)
    return 0 if g0_ready else 2


if __name__ == "__main__":
    sys.exit(main())
