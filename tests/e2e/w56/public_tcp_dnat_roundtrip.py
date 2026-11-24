#!/usr/bin/env python3
"""Public TCP DNAT bidirectional roundtrip verification (221 <-> 222).

This is deliberately NOT a simultaneous-open claim.  It verifies that the two
owner-provided DNAT mappings carry a real TCP connection and real bytes in both
directions: one host listens, the other connects through the public endpoint,
then the roles are swapped.  The simultaneous-open probe remains a separate
experiment with its own verdict.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import pathlib
import shlex

import paramiko

HOSTS = {
    "221": {"host": "192.168.1.221", "bind_port": 19910, "egress": "112.5.60.4"},
    "222": {"host": "192.168.1.222", "bind_port": 19920, "egress": "112.5.32.34"},
}


def as_text(value) -> str:
    if isinstance(value, bytes):
        return value.decode("utf-8", "replace")
    return str(value or "")


def key_path() -> pathlib.Path:
    if os.environ.get("CR_SSH_KEY"):
        return pathlib.Path(os.environ["CR_SSH_KEY"])
    return pathlib.Path(os.environ.get("TEMP", "/tmp")) / "crkeys" / "id_ed25519"


def client(host: str, key: str) -> paramiko.SSHClient:
    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    c.connect(host, username="root", key_filename=key, timeout=10)
    return c


def listener_code(port: int) -> str:
    return f'''import socket
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("0.0.0.0", {port}))
s.listen(1)
s.settimeout(12)
print("READY", flush=True)
c, a = s.accept()
data = c.recv(4)
c.sendall(b"PONG" if data == b"PING" else b"FAIL")
print("ACCEPT", a, "DATA", data.decode("ascii", "replace"), flush=True)
c.close()
s.close()
'''


def connector_code(ip: str, port: int) -> str:
    return f'''import socket
s = socket.create_connection(("{ip}", {port}), 6)
s.settimeout(5)
s.sendall(b"PING")
data = s.recv(4)
print("PAYLOAD", data.decode("ascii", "replace"), "PEER", str(s.getpeername()), flush=True)
s.close()
'''


def run_phase(listen_label: str, connect_label: str, key: str, timeout: int) -> dict:
    listen_cfg = HOSTS[listen_label]
    connect_cfg = HOSTS[connect_label]
    target = f"{listen_cfg['egress']}:{listen_cfg['bind_port']}"
    out: dict = {
        "listen_node": listen_label,
        "connect_node": connect_label,
        "public_target": target,
    }
    lc = client(listen_cfg["host"], key)
    cc = client(connect_cfg["host"], key)
    try:
        lcmd = f"timeout {timeout} python3 -u -c " + shlex.quote(listener_code(listen_cfg["bind_port"]))
        _, lso, lse = lc.exec_command(lcmd, timeout=timeout + 5)
        ready = as_text(lso.readline()).strip()
        out["listener_ready"] = ready == "READY"
        out["listener_cmd"] = lcmd
        if not out["listener_ready"]:
            out["listener_stdout"] = ready
            return out

        ccmd = f"timeout {timeout} python3 -u -c " + shlex.quote(
            connector_code(listen_cfg["egress"], listen_cfg["bind_port"])
        )
        _, cso, cse = cc.exec_command(ccmd, timeout=timeout + 5)
        out["connector_cmd"] = ccmd
        out["connector_stdout"] = as_text(cso.read()).strip()
        out["connector_stderr"] = as_text(cse.read()).strip()
        out["listener_rest"] = as_text(lso.read()).strip()
        out["listener_stderr"] = as_text(lse.read()).strip()
        out["ok"] = (
            "PAYLOAD PONG" in out["connector_stdout"]
            and "DATA PING" in out["listener_rest"]
        )
    finally:
        lc.close()
        cc.close()
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--report-out", required=True)
    ap.add_argument("--timeout", type=int, default=15)
    args = ap.parse_args()

    key = str(key_path())
    phases = {
        "222_to_221": run_phase("221", "222", key, args.timeout),
        "221_to_222": run_phase("222", "221", key, args.timeout),
    }
    verdict = "PASS" if all(p.get("ok") for p in phases.values()) else "BLOCKED"
    report = {
        "recorded_at": dt.datetime.now().astimezone().isoformat(timespec="seconds"),
        "scope": "real TCP through owner-provided public DNAT; bidirectional byte roundtrip; NOT simultaneous open",
        "hosts": HOSTS,
        "phases": phases,
        "verdict": verdict,
        "simultaneous_open_claim": False,
        "note": (
            "Pure RFC793 simultaneous open was measured separately and remains BLOCKED: "
            "upstream NAT rewrites the bound source port (observed 19910 -> 18011), so the "
            "returned SYN does not match the local 4-tuple.  This report only validates DNAT reachability."
        ),
        "g2_claim": False,
        "s6_status": "not_run",
        "evidence_valid_for_pass": False,
        "auto_pass": False,
    }
    out = pathlib.Path(args.report_out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"verdict": verdict, "report": str(out)}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
