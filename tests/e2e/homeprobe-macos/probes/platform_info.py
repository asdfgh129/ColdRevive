#!/usr/bin/env python3
"""家宽 kit：记录运行主机与出口的客观信息（只读，不宣称任何门禁结论）。

macOS Ventura / Intel 目标。只用 Python 标准库：
  * platform / socket：OS、CPU、Python、本机地址（UDP connect 选路，不发包）；
  * 可选 HTTP 查询公网 IP（两个端点，失败如实记录，不编造）；
  * macOS 用 `route -n get default` 读默认网关。

输出 JSON；`evidence_valid_for_pass=false`、`g2_claim=false`、`s6_status=not_run` 为常量。
"""

from __future__ import annotations

import argparse
import json
import platform
import socket
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Optional

PUBLIC_IP_ENDPOINTS = [
    "https://api.ipify.org?format=json",
    "https://ipinfo.io/json",
]


def local_ip() -> str:
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        sock.connect(("8.8.8.8", 53))
        return sock.getsockname()[0]
    except OSError:
        return ""
    finally:
        sock.close()


def default_gateway() -> str:
    import re
    import subprocess

    if platform.system() != "Darwin":
        return ""
    try:
        result = subprocess.run(["route", "-n", "get", "default"],
                                capture_output=True, text=True, timeout=6, check=False)
        match = re.search(r"^\s*gateway:\s*(\d+\.\d+\.\d+\.\d+)\s*$",
                          result.stdout or "", re.M)
        return match.group(1) if match else ""
    except (OSError, subprocess.TimeoutExpired):
        return ""


def public_ip(timeout: float) -> Dict:
    import urllib.request

    attempts: List[Dict] = []
    for url in PUBLIC_IP_ENDPOINTS:
        row: Dict = {"endpoint": url, "ok": False}
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "coldrevive-homeprobe/1.0"})
            with urllib.request.urlopen(req, timeout=timeout) as response:
                payload = json.loads(response.read().decode("utf-8", "replace"))
            ip = str(payload.get("ip") or payload.get("query") or "").strip()
            row["ok"] = bool(ip)
            row["ip"] = ip or None
        except Exception as exc:  # noqa: BLE001 - HTTP 是尽力而为，失败必须留痕
            row["error"] = "%s: %s" % (type(exc).__name__, exc)
        attempts.append(row)
        if row["ok"]:
            break
    chosen = next((a["ip"] for a in attempts if a["ok"]), None)
    return {"observed_public_ip": chosen, "attempts": attempts,
            "note": "HTTP 查询失败不影响 STUN 映射观测；不得用本机网卡地址冒充公网 IP"}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--timeout", type=float, default=3.0)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    report = {
        "schema": "coldrevive.homeprobe.platform/v1",
        "recorded_at": datetime.now(timezone.utc).isoformat(),
        "host": {
            "os": platform.system(),
            "os_version": platform.mac_ver()[0] if platform.system() == "Darwin" else "",
            "architecture": platform.machine(),
            "processor": platform.processor(),
            "python": sys.version.split()[0],
            "hostname": socket.gethostname(),
            "local_ipv4": local_ip(),
            "default_gateway": default_gateway(),
        },
        "egress": public_ip(args.timeout),
        "scope": "home-broadband-single-egress",
        "evidence_valid_for_pass": False,
        "g2_claim": False,
        "s6_status": "not_run",
    }
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print("platform: %s %s / %s / python %s" % (
        report["host"]["os"], report["host"]["os_version"] or "-",
        report["host"]["architecture"], report["host"]["python"]))
    print("  local=%s public=%s" % (
        report["host"]["local_ipv4"] or "n/a",
        report["egress"]["observed_public_ip"] or "n/a (see attempts)"))
    print("  report: %s" % out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
