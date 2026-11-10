#!/usr/bin/env python3
"""S2 P4 precondition probe: can this host complete a public high-port UDP round-trip?

This is an OPTIONAL, MANUALLY-RUN diagnostic. It is NOT part of P4 traffic, sends
no KAD packets, contacts no bootstrap node, and is never invoked by `kad-mini`.

Why it exists: P4 executed on 2026-11-10 sent three protocol-correct
BOOTSTRAP_REQ datagrams to public KAD nodes and received no responses. Control
experiments showed DNS/UDP-53 round-trips worked (~50 ms) while STUN on UDP
19302 / 3478 timed out, and a LAN eMule answered the identical frame. The cause
was the execution environment filtering public high-port UDP, not the KAD wire
format and not namespace rejection.

Run this BEFORE re-running P4 from any new network. If high-port UDP fails, do
not burn the approved small sample: move to an unrestricted network first.

    python3 tests/spikes/s2/udp_egress_check.py
    echo $?     # 0 = high-port UDP round-trip OK, 2 = blocked

Exit codes: 0 = pass, 2 = blocked, 1 = diagnostic error.
"""

from __future__ import annotations

import socket
import struct
import sys
import time

# Public STUN servers are used only as UDP echo endpoints. They are NOT KAD
# bootstrap nodes; no KAD frame is ever sent by this script.
STUN_TARGETS = (
    ("stun.l.google.com", 19302),
    ("stun1.l.google.com", 19302),
)
DNS_TARGETS = (("8.8.8.8", 53), ("1.1.1.1", 53))
TIMEOUT_S = 3.0


def dns_round_trip(host: str, port: int) -> tuple[bool, str]:
    query = (
        struct.pack(">HHHHHH", 0x1234, 0x0100, 1, 0, 0, 0)
        + b"\x07example\x03com\x00"
        + struct.pack(">HH", 1, 1)
    )
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.settimeout(TIMEOUT_S)
        try:
            t0 = time.time()
            sock.sendto(query, (host, port))
            data, _ = sock.recvfrom(512)
            return True, f"{len(data)} B, {round((time.time() - t0) * 1000, 1)} ms"
        except OSError as exc:
            return False, type(exc).__name__


def stun_round_trip(host: str, port: int) -> tuple[bool, str]:
    tx_id = b"ColdReviveP4Egress!"
    message = struct.pack(">HHI", 0x0001, 0x0000, 0x2112A442) + tx_id
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.settimeout(TIMEOUT_S)
        try:
            t0 = time.time()
            sock.sendto(message, (host, port))
            data, _ = sock.recvfrom(512)
            ok = len(data) >= 20 and data[8:24] == tx_id
            return ok, (
                f"{len(data)} B, {round((time.time() - t0) * 1000, 1)} ms, "
                f"txid_match={ok}"
            )
        except OSError as exc:
            return False, type(exc).__name__


def main() -> int:
    print("S2 P4 UDP egress precondition check (diagnostic only, no KAD traffic)")
    dns_ok = 0
    for host, port in DNS_TARGETS:
        ok, detail = dns_round_trip(host, port)
        dns_ok += int(ok)
        print(f"  DNS   {host}:{port:<6} {'OK  ' if ok else 'FAIL'} {detail}")

    stun_ok = 0
    for host, port in STUN_TARGETS:
        ok, detail = stun_round_trip(host, port)
        stun_ok += int(ok)
        print(f"  STUN  {host}:{port:<6} {'OK  ' if ok else 'FAIL'} {detail}")

    if stun_ok:
        print(f"\nRESULT: PASS ({stun_ok}/{len(STUN_TARGETS)} high-port UDP round-trips OK).")
        print("        P4 may be run from this network.")
        return 0

    print(f"\nRESULT: BLOCKED (DNS {dns_ok}/{len(DNS_TARGETS)} OK, "
          f"high-port UDP {stun_ok}/{len(STUN_TARGETS)} OK).")
    print("        Public high-port UDP egress or its replies are filtered here.")
    print("        Do NOT run P4 from this network; use an unrestricted network.")
    print("        A KAD timeout here is an environment failure, NOT an S2 finding.")
    return 2


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:  # noqa: BLE001 - diagnostic must always report clearly
        print(f"diagnostic error: {type(exc).__name__}: {exc}", file=sys.stderr)
        sys.exit(1)
