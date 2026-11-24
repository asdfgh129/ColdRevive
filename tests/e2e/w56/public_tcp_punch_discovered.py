#!/usr/bin/env python3
"""公网 TCP 打洞：用动态发现的映射端口作为信令字段。

与 public_tcp_punch_probe.py 的区别只有一个关键点：222 侧的目标端口不再固定写
`112.5.60.4:19910`，而是先用 `--discover-tcp-port` 从 221 的 19910 向公网 observer
发现真实映射端口 P，再让 222 连 `112.5.60.4:P`。EIM 由独立的双观测点 tcpdump 证据
（tests/ARCHIVE/evidence/tcp-eim-two-observers-tcpdump-20261124.json）证明；本脚本
不做 EIM 宣称，也不改门禁常量。

判据仍与原探针一致：两侧 `ok=true` 且 `bytes_roundtrip=true`。任何失败如实记
BLOCKED/FAIL，不写「打洞成功率」。
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import pathlib
import threading

import paramiko

HOSTS = {
    "221": {
        "host": "192.168.1.221",
        "bin": "/tmp/coldrevive-nat",
        "bind": "0.0.0.0:19910",
        "egress": "112.5.60.4",
    },
    "222": {
        "host": "192.168.1.222",
        "bin": "/tmp/coldrevive-nat",
        "bind": "0.0.0.0:19920",
        "egress": "112.5.32.34",
    },
}
OBSERVERS = "82.157.68.159:7100"  # 82 上 REDIRECT -> 19110 的 in-band observer
LOCAL_BIN = "/tmp/coldrevive-nat"


def key_path() -> pathlib.Path:
    if os.environ.get("CR_SSH_KEY"):
        return pathlib.Path(os.environ["CR_SSH_KEY"])
    return pathlib.Path(os.environ.get("TEMP", "/tmp")) / "crkeys" / "id_ed25519"


def ssh(label: str, cmd: str, timeout: int, host: str | None = None):
    """返回一个记录 dict，出错也不吞。host 省略时按 label 查 HOSTS。"""
    out: dict = {"node": label, "command": cmd}
    target = host or HOSTS[label]["host"]
    try:
        c = paramiko.SSHClient()
        c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
        c.connect(target, username="root", key_filename=str(key_path()), timeout=10)
        _, so, se = c.exec_command(cmd, timeout=timeout)
        out["stdout"] = so.read().decode("utf-8", "replace")
        out["stderr"] = se.read().decode("utf-8", "replace")
        c.close()
    except Exception as e:  # noqa: BLE001 - 远程不可达也是证据
        out["error"] = f"{type(e).__name__}: {e}"
    return out


def parse_json_line(raw: str):
    for line in raw.splitlines():
        line = line.strip()
        if line.startswith("{"):
            try:
                return json.loads(line)
            except json.JSONDecodeError:
                return None
    return None


def discover(timeout_ms: int) -> dict:
    cmd = (
        f"{LOCAL_BIN} --discover-tcp-port {HOSTS['221']['bind']} {OBSERVERS} "
        f"--timeout {timeout_ms} --json 2>&1"
    )
    rec = ssh("221-discover", cmd, (timeout_ms // 1000) + 10, host=HOSTS["221"]["host"])
    rec["parsed"] = parse_json_line(rec.get("stdout", ""))
    return rec


def punch_one(label: str, peer: str, target_port: int, timeout_ms: int) -> dict:
    me = HOSTS[label]
    target = f"{HOSTS[peer]['egress']}:{target_port}"
    role = "--echo" if label == "221" else "--echo-responder"
    cmd = (
        f"{me['bin']} --tcp-punch {me['bind']} {target} {role} "
        f"--timeout {timeout_ms} --json --tag egress={me['egress']} --tag peer={peer} 2>&1"
    )
    rec = ssh(label, cmd, (timeout_ms // 1000) + 15)
    rec["parsed"] = parse_json_line(rec.get("stdout", ""))
    return rec


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--timeout-ms", type=int, default=5000)
    ap.add_argument("--observed-port", type=int, default=0,
                    help="若已知发现端口，直接指定，跳过 discover 阶段")
    ap.add_argument("--report-out", required=True)
    args = ap.parse_args()

    disc = None
    if args.observed_port > 0:
        discovered_port = args.observed_port
    else:
        disc = discover(args.timeout_ms)
        parsed = disc.get("parsed") or {}
        discovered_port = parsed.get("discovered_port") or 0

    report: dict = {
        "recorded_at": dt.datetime.now().astimezone().isoformat(timespec="seconds"),
        "scope": "real dual-side TCP simultaneous open using dynamically discovered mapped port as signaling; no G2/S6 claim",
        "discovery": disc,
        "signaling": {
            "source": ("provided --observed-port" if args.observed_port > 0 else "discover-tcp-port from 221:19910"),
            "observer_endpoints": OBSERVERS,
            "discovered_port": discovered_port,
            "peer_target": f"112.5.60.4:{discovered_port}" if discovered_port else None,
        },
        "eim_evidence": "tests/ARCHIVE/evidence/tcp-eim-two-observers-tcpdump-20261124.json",
        "results": {},
        "g2_claim": False,
        "s6_status": "not_run",
        "evidence_valid_for_pass": False,
        "auto_pass": False,
    }

    if not discovered_port:
        report["verdict"] = "BLOCKED"
        report["reason"] = "未能发现映射端口，无法构造信令；不猜测端口"
        out = pathlib.Path(args.report_out)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({"verdict": "BLOCKED", "reason": report["reason"], "report": str(out)},
                         ensure_ascii=False))
        return 0

    results: dict[str, dict] = {}
    # 221 目标端口：对端 222 的公网 DNAT 端口 19920（保持不变）。
    # 222 目标端口：发现端口 P（信令字段），而不是固定 19910。
    threads = [
        threading.Thread(target=lambda k, p, tp: results.__setitem__(
            k, punch_one(k, p, tp, args.timeout_ms)),
            args=("221", "222", 19920)),
        threading.Thread(target=lambda k, p, tp: results.__setitem__(
            k, punch_one(k, p, tp, args.timeout_ms)),
            args=("222", "221", discovered_port)),
    ]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    report["results"] = results

    parsed = {k: (v.get("parsed") or {}) for k, v in results.items()}
    both_ok = all(
        bool(parsed.get(k, {}).get("ok")) and bool(parsed.get(k, {}).get("bytes_roundtrip"))
        for k in ("221", "222")
    )
    report["verdict"] = "PASS" if both_ok else "BLOCKED"
    report["reason"] = ("两侧用发现端口完成 TCP 同时打开并双向回环成功" if both_ok else
        "; ".join(
            f"{k}: " + str(
                results[k].get("error")
                or results[k].get("stderr", "").strip()
                or parsed[k].get("errors")
                or parsed[k].get("reason")
                or ("ok=false" if not parsed[k].get("ok") else "no roundtrip")
            )
            for k in ("221", "222")
        ))

    out = pathlib.Path(args.report_out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"verdict": report["verdict"], "reason": report["reason"],
                      "discovered_port": discovered_port, "report": str(out)},
                     ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
