#!/usr/bin/env python3
"""S2 P4 KAD-specific UDP egress probe (portable, stdlib only).

Why this exists
---------------
`udp_egress_check.py` uses STUN (3478 / 19302) as a proxy for "public high-port
UDP works". That inference is too strong: an egress filter can allow some high
destination ports or recognise specific applications while dropping others.
On 2026-11-10 the LAN eMule client was observed with 432 KAD contacts and a
HighID on the same network whose hosts both fail the STUN check -- so a STUN
timeout does NOT prove that a KAD datagram to a live node is also dropped.

This probe sends ONLY a 2-byte `0xE4 0x01` KADEMLIA2_BOOTSTRAP_REQ to a small,
USER-SUPPLIED list of public KAD contacts, at the G0-confirmed rate, and
records every raw reply. It publishes no Key, sends no HELLO / REQ / SEARCH,
and cannot by itself produce S2 evidence. It answers exactly one question:

    from THIS host, does a real KAD packet to a live public node get a reply?

Targets are never bundled. Supply them via --targets <json> (a list of
{"ip": "...", "port": 4672}); e.g. contacts learned from a LAN eMule
BOOTSTRAP_RES. Any non-global IPv4 address is rejected (same rule as
`kad-mini live_gate`). Reports go to the gitignored reports/ directory.

Usage
-----
    python3 kad_egress_probe.py --targets <file> --attest-i-am-the-experiment-operator
    python3 kad_egress_probe.py --targets <file> --attest-i-am-the-experiment-operator --json
    echo $?   # 0 = at least one KAD reply, 2 = no reply, 1 = diagnostic error
"""

from __future__ import annotations

import argparse
import ipaddress
import json
import os
import socket
import sys
import time
import zlib
from datetime import datetime, timezone

BOOTSTRAP_REQ = bytes.fromhex("e401")  # 0xE4 KADEMLIA2_BOOTSTRAP_REQ, no payload
DEFAULT_SAMPLE = 3
MAX_SAMPLE = 10
DEFAULT_RATE = 0.2
MAX_RATE = 1.0
TIMEOUT_S = 3.0
HERE = os.path.dirname(os.path.abspath(__file__))
REPORT_DIR = os.path.join(HERE, "reports")


def is_global_ipv4(text: str) -> bool:
    try:
        a = ipaddress.IPv4Address(text)
    except (ipaddress.AddressValueError, ValueError):
        return False
    return bool(
        a.is_global
        and not a.is_private
        and not a.is_loopback
        and not a.is_link_local
        and not a.is_multicast
        and not a.is_reserved
        and not a.is_unspecified
    )


def load_targets(path: str, limit: int) -> list[dict]:
    with open(path, encoding="utf-8") as f:
        raw = json.load(f)
    if not isinstance(raw, list) or not raw:
        raise ValueError("targets file must be a non-empty JSON list")
    out = []
    for item in raw:
        if not isinstance(item, dict):
            raise ValueError("each target must be an object")
        ip = str(item.get("ip", "")).strip()
        port = item.get("port", item.get("udp"))
        port = int(port)
        if not is_global_ipv4(ip):
            raise ValueError(f"non-global IPv4 target rejected: {ip!r}")
        if not (1 <= port <= 65535):
            raise ValueError(f"port out of range: {port}")
        out.append({"ip": ip, "port": port})
        if len(out) >= limit:
            break
    return out


def classify(data: bytes) -> dict:
    rec = {"bytes": len(data), "header": data[:2].hex(), "reply_hex": data[:256].hex()}
    if not data:
        rec["class"] = "empty"
    elif data[0] == 0xE4:
        rec["class"] = "kad_plaintext"
        rec["opcode"] = "0x%02x" % data[1] if len(data) > 1 else "?"
    elif data[0] == 0xE5:
        rec["class"] = "kad_zlib_packed"
        rec["opcode"] = "0x%02x" % data[1] if len(data) > 1 else "?"
        try:
            payload = zlib.decompress(data[2:])
            rec["inflated_bytes"] = len(payload)
        except zlib.error as exc:
            rec["inflate_error"] = str(exc)
    else:
        rec["class"] = "non_kad_or_obfuscated"
    return rec


def probe_one(target: dict, timeout: float, src_port: int = 0) -> dict:
    rec = {"ip": target["ip"], "port": target["port"], "source_port": src_port,
           "sent_hex": BOOTSTRAP_REQ.hex(), "ok": False,
           "sent_at_utc": datetime.now(timezone.utc).isoformat()}
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.settimeout(timeout)
    try:
        if src_port:
            sock.bind(("0.0.0.0", src_port))
        t0 = time.time()
        sock.sendto(BOOTSTRAP_REQ, (target["ip"], target["port"]))
        data, peer = sock.recvfrom(2048)
        rec["rtt_ms"] = round((time.time() - t0) * 1000, 1)
        rec["peer"] = "%s:%d" % peer
        rec.update(classify(data))
        rec["ok"] = rec["class"] in ("kad_plaintext", "kad_zlib_packed")
    except OSError as exc:
        rec["class"] = "timeout_or_error"
        rec["detail"] = type(exc).__name__ + ": " + str(exc)
    finally:
        sock.close()
    return rec


def main() -> int:
    ap = argparse.ArgumentParser(description="S2 P4 KAD-specific UDP egress probe")
    ap.add_argument("--targets", required=True, help="JSON list of {ip, port}; never bundled")
    ap.add_argument("--sample-size", type=int, default=DEFAULT_SAMPLE)
    ap.add_argument("--rate-limit", type=float, default=DEFAULT_RATE)
    ap.add_argument("--timeout", type=float, default=TIMEOUT_S)
    ap.add_argument("--source-port", type=int, default=0,
                    help="bind a fixed local UDP source port (tests per-source-port policy)")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--attest-i-am-the-experiment-operator", action="store_true",
                    help="required: this sends real KAD datagrams to public nodes")
    args = ap.parse_args()

    if not args.attest_i_am_the_experiment_operator:
        print("error: --attest-i-am-the-experiment-operator is required", file=sys.stderr)
        return 1
    if not (1 <= args.sample_size <= MAX_SAMPLE):
        print(f"error: --sample-size must be 1..{MAX_SAMPLE}", file=sys.stderr)
        return 1
    if not (0 < args.rate_limit <= MAX_RATE):
        print(f"error: --rate-limit must be >0 and <= {MAX_RATE}", file=sys.stderr)
        return 1
    try:
        targets = load_targets(args.targets, args.sample_size)
    except (OSError, ValueError, TypeError, json.JSONDecodeError) as exc:
        print("error: cannot load targets: %s" % exc, file=sys.stderr)
        return 1

    spacing = 1.0 / args.rate_limit
    records = []
    for i, t in enumerate(targets):
        if i:
            time.sleep(spacing)
        records.append(probe_one(t, args.timeout, args.source_port))

    got = sum(1 for r in records if r["ok"])
    summary = {
        "schema": "coldrevive.m0.s2-kad-egress-probe/v1",
        "generated_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "mode": "kad-specific-egress-diagnostic",
        "public_kad_contacted": True,
        "evidence_valid_for_pass": False,
        "publish_sent": False,
        "search_sent": False,
        "hello_sent": False,
        "sample_size": len(records),
        "rate_limit_pps": args.rate_limit,
        "source_port": args.source_port,
        "replies": got,
        "records": records,
        "note": "BOOTSTRAP_REQ only. A reply proves UDP egress/reply path for KAD on this host; "
                "no reply cannot separate egress filtering from dead contacts. NOT an S2 result.",
    }
    os.makedirs(REPORT_DIR, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%d%H%M%S")
    report = os.path.join(REPORT_DIR, f"kad-egress-probe-{stamp}.json")
    with open(report, "w", encoding="utf-8") as f:
        json.dump(summary, f, ensure_ascii=False, indent=2)

    if args.json:
        print(json.dumps(summary, ensure_ascii=False, indent=2))
    else:
        print("S2 P4 KAD-specific UDP egress probe (BOOTSTRAP_REQ only, no publish/search)")
        for r in records:
            endpoint = "%s:%d" % (r["ip"], r["port"])
            if r["ok"]:
                print("  %-26s OK     %s %s, %d B, %.0f ms" % (
                    endpoint, r["class"], r.get("opcode", ""), r["bytes"], r["rtt_ms"]))
            else:
                print("  %-26s FAIL   %s %s" % (
                    endpoint, r["class"], r.get("detail", "")))
        print("\nRESULT: %s (%d/%d KAD replies)." % ("REACHABLE" if got else "NO_REPLY",
                                                     got, len(records)))
        print("  report: %s" % report)
        if got:
            print("  A real KAD packet reached a public node and came back from this host.")
            print("  STUN-based egress checks were a false negative -> re-evaluate P4.")
        else:
            print("  Cannot separate egress filtering from dead contacts; do NOT count as S2 evidence.")
    return 0 if got else 2


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:  # noqa: BLE001 - diagnostic must always report clearly
        print("diagnostic error: %s: %s" % (type(exc).__name__, exc), file=sys.stderr)
        sys.exit(1)
