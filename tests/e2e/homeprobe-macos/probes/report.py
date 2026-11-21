#!/usr/bin/env python3
"""家宽 kit：把各探针的 JSON 汇成一份摘要（只汇总，不判定 pass）。

缺少的探针记为 missing，不补默认值、不合成结果。摘要恒定写：
  g2_claim=false / s6_status=not_run / evidence_valid_for_pass=false / auto_pass=false。
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List

FILES = {
    "platform": "platform.json",
    "stun": "stun.json",
    "nat_behavior": "nat_behavior.json",
    "dht": "dht.json",
    "kad_egress": "kad_egress.json",
}

LIMITATIONS = [
    "单个家宽出口 = 一个 NAT 分类样本，不是 NAT 矩阵，也不是可达率",
    "STUN / KAD / DHT 只读探测不修改 g2_claim 或 s6_status 机器常量",
    "KAD 探针只发 BOOTSTRAP_REQ，不发 SEARCH/PUBLISH；FR-24 公网检索仍 BLOCKED",
    "DHT 探针不发 announce_peer；S3 pass 不能外推为真实 Mainline 已加入",
    "本 kit 不含 eD2k 传输、BT 数据面或公平性量具，无法关闭 FR-25 / TCP 打洞 / 拥塞语义欠账",
]


def load(results_dir: Path, name: str):
    path = results_dir / FILES[name]
    if not path.exists():
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        return {"error": "%s: %s" % (type(exc).__name__, exc), "path": str(path)}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--results", default="results")
    args = ap.parse_args()
    results_dir = Path(args.results)
    raw = {name: load(results_dir, name) for name in FILES}

    summary: Dict = {
        "schema": "coldrevive.homeprobe.summary/v1",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "results_dir": str(results_dir),
        "missing": [name for name, value in raw.items() if value is None],
        "probe_files": FILES,
        "observed": {},
        "g2_claim": False,
        "s6_status": "not_run",
        "evidence_valid_for_pass": False,
        "auto_pass": False,
        "limitations": LIMITATIONS,
    }

    platform_report = raw["platform"] or {}
    summary["observed"]["platform"] = {
        "os": (platform_report.get("host") or {}).get("os"),
        "os_version": (platform_report.get("host") or {}).get("os_version"),
        "architecture": (platform_report.get("host") or {}).get("architecture"),
        "public_ip": (platform_report.get("egress") or {}).get("observed_public_ip"),
    }
    stun = raw["stun"] or {}
    summary["observed"]["stun"] = {
        "nat_type": (stun.get("classification") or {}).get("nat_type_observed"),
        "mapped_addresses": (stun.get("classification") or {}).get("mapped_addresses"),
        "upnp": ((stun.get("traversal_stack_probe") or {}).get("upnp_igd_discovery") or {}).get("available"),
        "nat_pmp": ((stun.get("traversal_stack_probe") or {}).get("nat_pmp") or {}).get("available"),
    }
    nat = raw["nat_behavior"] or {}
    summary["observed"]["nat_behavior"] = {
        "classic": (nat.get("classification") or {}).get("classic"),
        "confidence": (nat.get("classification") or {}).get("confidence"),
        "mapping": (nat.get("mapping_behavior") or {}).get("mapping_behavior"),
        "filtering": (nat.get("filtering_behavior") or {}).get("filtering_behavior"),
        "environment_label": nat.get("environment_label"),
    }
    dht = raw["dht"] or {}
    summary["observed"]["dht"] = [
        {
            "label": row.get("label"),
            "ping_ok": (row.get("ping") or {}).get("ok", False),
            "token_len": (row.get("get_peers") or {}).get("token_len"),
            "peers": (row.get("get_peers") or {}).get("peers"),
        }
        for row in (dht.get("results") or [])
    ]
    kad = raw["kad_egress"] or {}
    summary["observed"]["kad_egress"] = {
        "replies": kad.get("replies"),
        "sample_size": kad.get("sample_size"),
        "public_kad_contacted": kad.get("public_kad_contacted", False),
    }

    out = results_dir / "summary.json"
    results_dir.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    print("ColdRevive home-broadband probe summary")
    print("  platform : %s %s / %s / public_ip=%s" % (
        summary["observed"]["platform"]["os"] or "-",
        summary["observed"]["platform"]["os_version"] or "-",
        summary["observed"]["platform"]["architecture"] or "-",
        summary["observed"]["platform"]["public_ip"] or "n/a"))
    print("  STUN     : %s mapped=%s" % (
        summary["observed"]["stun"]["nat_type"] or "n/a",
        summary["observed"]["stun"]["mapped_addresses"] or "n/a"))
    print("  behavior : %s (confidence=%s)" % (
        summary["observed"]["nat_behavior"]["classic"] or "n/a",
        summary["observed"]["nat_behavior"]["confidence"] or "n/a"))
    print("  DHT      : %s" % (
        ", ".join("%s=%s" % (r["label"], "OK" if r["ping_ok"] else "NO")
                  for r in summary["observed"]["dht"]) or "n/a"))
    print("  KAD      : %s/%s replies" % (
        summary["observed"]["kad_egress"]["replies"],
        summary["observed"]["kad_egress"]["sample_size"]))
    print("  missing  : %s" % (", ".join(summary["missing"]) or "none"))
    print("  gates    : g2_claim=false / s6_status=not_run / auto_pass=false")
    print("  summary  : %s" % out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
