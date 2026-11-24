#!/usr/bin/env python3
"""Public dual-side TCP simultaneous-open probe (221 <-> 222).

判据只有 `ok=true` 且 `bytes_roundtrip=true` 且两侧都报告同一个已建立连接。
任何超时/拒绝都如实记为 BLOCKED，不写「打洞成功率」，不改门禁常量。
SSH 私钥从 `CR_SSH_KEY` 或系统临时目录读取，凭据不写入报告。
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import pathlib
import threading

import paramiko

ROOT = pathlib.Path(__file__).resolve().parents[2]
HOSTS = {
    "221": {
        "host": "192.168.1.221",
        "bin": "/opt/coldrevive/repo/target/release/coldrevive-nat",
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


def key_path() -> pathlib.Path:
    if os.environ.get("CR_SSH_KEY"):
        return pathlib.Path(os.environ["CR_SSH_KEY"])
    return pathlib.Path(os.environ.get("TEMP", "/tmp")) / "crkeys" / "id_ed25519"


def run_one(label: str, peer: str, timeout_ms: int) -> dict:
    me = HOSTS[label]
    target = f"{HOSTS[peer]['egress']}:{HOSTS[peer]['bind'].rsplit(':', 1)[1]}"
    role = "--echo" if label == "221" else "--echo-responder"
    cmd = (
        f"{me['bin']} --tcp-punch {me['bind']} {target} {role} "
        f"--timeout {timeout_ms} --json --tag egress={me['egress']} "
        f"--tag peer={peer}"
    )
    out: dict = {"node": label, "command": cmd}
    key = str(key_path())
    try:
        client = paramiko.SSHClient()
        client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
        client.connect(me["host"], username="root", key_filename=key, timeout=10)
        stdin, stdout, stderr = client.exec_command(cmd, timeout=(timeout_ms // 1000) + 15)
        raw = stdout.read().decode("utf-8", "replace")
        err = stderr.read().decode("utf-8", "replace")
        client.close()
        out["stdout"] = raw
        out["stderr"] = err
        # CLI 可能在 JSON 前打印日志：取第一行以 '{' 开头的内容。
        for line in raw.splitlines():
            line = line.strip()
            if line.startswith("{"):
                try:
                    out["parsed"] = json.loads(line)
                except json.JSONDecodeError:
                    out["parse_error"] = "JSON line invalid"
                break
        else:
            out["parse_error"] = "no JSON line in stdout"
    except Exception as e:  # 远程不可达也是证据，不能吞掉
        out["error"] = f"{type(e).__name__}: {e}"
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--timeout-ms", type=int, default=5000)
    ap.add_argument("--report-out", required=True)
    args = ap.parse_args()

    results: dict[str, dict] = {}
    threads = [
        threading.Thread(target=lambda k=k, p=p: results.__setitem__(k, run_one(k, p, args.timeout_ms)))
        for k, p in (("221", "222"), ("222", "221"))
    ]
    for t in threads:
        t.start()
    for t in threads:
        t.join()

    parsed = {k: v.get("parsed", {}) for k, v in results.items()}
    both_ok = all(
        bool(parsed.get(k, {}).get("ok")) and bool(parsed.get(k, {}).get("bytes_roundtrip"))
        for k in ("221", "222")
    )
    reason = "两侧 TCP 同时打开并双向回环成功" if both_ok else (
        "; ".join(
            f"{k}: " + str(
                results[k].get("error")
                or results[k].get("parse_error")
                or parsed[k].get("errors")
                or parsed[k].get("reason")
                or ("ok=false" if not parsed[k].get("ok") else "no roundtrip")
            )
            for k in ("221", "222")
        )
    )
    report = {
        "recorded_at": dt.datetime.now().astimezone().isoformat(timespec="seconds"),
        "scope": "real public dual-side TCP simultaneous open; no NAT matrix or G2 claim",
        "hosts": HOSTS,
        "results": results,
        "verdict": "PASS" if both_ok else "BLOCKED",
        "reason": reason,
        "g2_claim": False,
        "s6_status": "not_run",
        "evidence_valid_for_pass": False,
        "auto_pass": False,
    }
    out = pathlib.Path(args.report_out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"verdict": report["verdict"], "reason": reason, "report": str(out)},
                     ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
