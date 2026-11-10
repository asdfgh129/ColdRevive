#!/usr/bin/env python3
"""S2 P4 UDP egress precondition probe (portable, stdlib only).

This is an OPTIONAL, MANUALLY-RUN diagnostic. It is NOT part of P4 traffic, sends
no KAD packet to the public network, contacts no bootstrap node, and is never
invoked by `kad-mini`.

Why it exists
-------------
P4 executed on 2026-11-10 sent three protocol-correct BOOTSTRAP_REQ datagrams
to public KAD nodes and received no responses. The initial diagnosis was "public
high-port UDP is filtered here". A broader re-probe on 2026-11-10 refined that:

    UDP 53 (DNS)            OK  ~52 ms
    UDP 123 (NTP)           OK  ~185 ms   (time.cloudflare.com, pool.ntp.org)
    UDP 3478 / 19302 (STUN) TIMEOUT       (Cloudflare, ekiga, Google)
    fixed source ports (LAN eMule UDP port / 4672) on the same probes: still TIMEOUT
    LAN real eMule on its high UDP port: OK, 523 B / 10 ms
    Windows Firewall outbound block rules: 0

So, FOR STUN DATAGRAMS, the filter is at the network egress boundary and is
destination-port / application based (well-known low ports allowed, unknown
high ports dropped), NOT "UDP disabled", NOT source-port policy, NOT the local
host firewall.

IMPORTANT LIMIT (v2.4.11 / v2.4.12): STUN IS NOT KAD. STUN timeouts prove only
that STUN datagrams are filtered; an egress policy may still admit real KAD
traffic by application/DPI, by destination port, or by host IP + source port.
Use kad_egress_probe.py (a real 0xE4 0x01 BOOTSTRAP_REQ) for that question.
Also note eMule/aMule's "UDP state: open" is an INBOUND reachability test that
an inbound DNAT alone can satisfy -- it does not prove P4 can run. See
tests/spikes/s2/PLAN.md sections 8.6 and 9.

CROSS-HOST RESULT (2026-11-10): the LAN eMule host is on the same network and
HAS an inbound port mapping. Running this exact file there produced the same
BLOCKED result (low-port 4/4 OK, high-port 0/3 OK, exit 2). So there is no
per-host STUN egress exception and an inbound DNAT does not grant outbound
STUN high-port UDP. This does NOT prove "no host on this network can run P4".

    -> Move to an unrestricted network, re-run this check there (must exit 0),
       refresh nodes.dat (a 2-day-old contact list may be entirely dead), then
       re-run P4 with the G0-confirmed parameters.
    -> When diagnosing a DIFFERENT host/network, copy this single file there and
       repeat: 0 = high-port UDP works, 2 = STUN blocked, not an S2 finding.

Usage
-----
    python3 udp_egress_check.py
    python3 udp_egress_check.py --source-port <PORT>
    python3 udp_egress_check.py --lan-ip <PRIVATE_IP> --lan-port <UDP_PORT>
    python3 udp_egress_check.py --json
    echo $?     # 0 = high-port UDP round-trip OK, 2 = blocked, 1 = diag error

`--lan-ip/--lan-port` is an optional LAN-only control. It sends a single 2-byte
KADEMLIA2_BOOTSTRAP_REQ to a private eMule node to prove the local socket stack
and LAN path allow high-port UDP. It NEVER sends to a public address; the IP is
rejected unless it is RFC1918 / loopback. No LAN address is bundled here.

Exit codes: 0 = pass, 2 = blocked, 1 = diagnostic error.
"""

from __future__ import annotations

import argparse
import ipaddress
import json
import socket
import struct
import sys
import time

DNS_TARGETS = (("8.8.8.8", 53), ("1.1.1.1", 53))
NTP_TARGETS = (("time.cloudflare.com", 123), ("pool.ntp.org", 123))
STUN_TARGETS = (
    ("stun.cloudflare.com", 3478),
    ("stun.ekiga.net", 3478),
    ("stun.l.google.com", 19302),
)
TIMEOUT_S = 2.5

TX_ID = b"ColdReviveDiagXY"  # exactly 16 bytes
STUN_REQ = struct.pack(">HHI", 0x0001, 0x0000, 0x2112A442) + TX_ID
NTP_REQ = b"\x23" + b"\x00" * 47
DNS_REQ = (
    struct.pack(">HHHHHH", 0x1234, 0x0100, 1, 0, 0, 0)
    + b"\x07example\x03com\x00"
    + struct.pack(">HH", 1, 1)
)
KAD_BOOTSTRAP_REQ = bytes.fromhex("e401")

PAYLOADS = {
    "dns": DNS_REQ,
    "ntp": NTP_REQ,
    "stun": STUN_REQ,
    "kad": KAD_BOOTSTRAP_REQ,
}


def probe(kind: str, host: str, port: int, timeout: float, src_port: int) -> dict:
    """One UDP round-trip. Returns a small record; never raises for network errors."""
    rec = {"kind": kind, "host": host, "port": port, "source_port": src_port, "ok": False}
    try:
        ip = socket.gethostbyname(host)
    except OSError as exc:
        rec["detail"] = "dns_resolve_failed: " + type(exc).__name__
        return rec
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.settimeout(timeout)
    try:
        if src_port:
            sock.bind(("0.0.0.0", src_port))
        t0 = time.time()
        sock.sendto(PAYLOADS[kind], (ip, port))
        data, _ = sock.recvfrom(2048)
        rec["rtt_ms"] = round((time.time() - t0) * 1000, 1)
        rec["bytes"] = len(data)
        if kind == "stun":
            rec["ok"] = len(data) >= 20 and data[8:24] == TX_ID
            rec["detail"] = "txid_match=" + str(rec["ok"])
        elif kind == "kad":
            rec["ok"] = len(data) >= 2
            rec["detail"] = "header=" + data[:2].hex()
        else:
            rec["ok"] = len(data) > 0
            rec["detail"] = "reply"
    except OSError as exc:
        rec["detail"] = type(exc).__name__
    finally:
        sock.close()
    return rec


def is_private_target(host: str) -> bool:
    try:
        addr = ipaddress.ip_address(socket.gethostbyname(host))
    except (OSError, ValueError):
        return False
    return addr.is_private or addr.is_loopback


def main() -> int:
    ap = argparse.ArgumentParser(description="S2 P4 UDP egress precondition probe")
    ap.add_argument("--source-port", type=int, default=0,
                    help="bind a fixed local UDP source port (tests source-port policy)")
    ap.add_argument("--lan-ip", default=None,
                    help="optional RFC1918 eMule IP for a LAN-only high-port control")
    ap.add_argument("--lan-port", type=int, default=0,
                    help="UDP port of that LAN eMule node")
    ap.add_argument("--timeout", type=float, default=TIMEOUT_S)
    ap.add_argument("--json", action="store_true", help="emit a JSON summary on stdout")
    args = ap.parse_args()

    if bool(args.lan_ip) != bool(args.lan_port):
        print("error: --lan-ip and --lan-port must be given together", file=sys.stderr)
        return 1
    if args.lan_ip and not is_private_target(args.lan_ip):
        print("error: --lan-ip must be an RFC1918 / loopback address", file=sys.stderr)
        return 1

    controls = [probe("dns", h, p, args.timeout, args.source_port) for h, p in DNS_TARGETS]
    controls += [probe("ntp", h, p, args.timeout, args.source_port) for h, p in NTP_TARGETS]
    high = [probe("stun", h, p, args.timeout, args.source_port) for h, p in STUN_TARGETS]
    lan = ([probe("kad", args.lan_ip, args.lan_port, args.timeout, 0)]
           if args.lan_ip else [])

    ctl_ok = sum(r["ok"] for r in controls)
    high_ok = sum(r["ok"] for r in high)
    passed = high_ok > 0

    if args.json:
        print(json.dumps({
            "controls": controls, "high_port": high, "lan_control": lan,
            "control_ok": ctl_ok, "high_port_ok": high_ok, "passed": passed,
        }, ensure_ascii=False, indent=2))
        return 0 if passed else 2

    print("S2 P4 UDP egress precondition check (diagnostic only, no public KAD traffic)")
    if args.source_port:
        print("  fixed source port: %d" % args.source_port)
    for group, label in ((controls, "LOW-PORT CONTROL"),
                         (high, "HIGH-PORT PROBE"),
                         (lan, "LAN CONTROL")):
        if not group:
            continue
        print("  -- " + label)
        for r in group:
            print("     %-34s %-5s %s" % (
                "%s:%d" % (r["host"], r["port"]),
                "OK" if r["ok"] else "FAIL",
                ("%s B, %.0f ms, %s" % (r["bytes"], r["rtt_ms"], r["detail"]))
                if r["ok"] else r["detail"],
            ))

    if passed:
        print("\nRESULT: PASS (%d/%d high-port UDP round-trips OK)." % (high_ok, len(high)))
        print("        P4 may be run from this network.")
        return 0

    print("\nRESULT: BLOCKED (low-port control %d/%d OK, high-port %d/%d OK)." %
          (ctl_ok, len(controls), high_ok, len(high)))
    if ctl_ok:
        print("        UDP itself works; the egress boundary filters unknown high")
        print("        destination ports (or their replies). Local firewall / source")
        print("        port are NOT the cause, and an inbound DNAT will not fix egress.")
    else:
        print("        Even low-port UDP failed: no usable UDP egress from this host.")
    if lan and lan[0]["ok"]:
        print("        LAN high-port control OK -> local host + LAN path are fine;")
        print("        the filter sits at the internet egress boundary.")
    print("        STUN is blocked here; do NOT treat this alone as proof that")
    print("        P4 is impossible. A KAD timeout is an environment failure,")
    print("        NOT an S2 finding -- but confirm with kad_egress_probe.py.")
    print("        Cross-host check: repeat on any other candidate host/network;")
    print("        egress policy can be per-host (0 = high-port UDP works there).")
    return 2


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:  # noqa: BLE001 - diagnostic must always report clearly
        print("diagnostic error: %s: %s" % (type(exc).__name__, exc), file=sys.stderr)
        sys.exit(1)
