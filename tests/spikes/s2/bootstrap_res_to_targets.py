#!/usr/bin/env python3
"""Turn a real BOOTSTRAP_RES into a FRESH, public-only KAD targets list.

Read-only helper. It sends NO packets and contacts NO node. It exists to remove
the "dead contacts" confound from `kad_egress_probe.py`:

    A 0/N result cannot separate egress filtering from N dead contacts. If two
    different hosts probe the SAME stale 3 contacts, the two runs are NOT
    independent replicates -- they share the same confound. The remedy is to
    ask a live eMule client for the contacts it holds RIGHT NOW (a LAN-only
    BOOTSTRAP_REQ, which is what `kad-mini lan-probe` does), then sweep a wider
    sample of those fresh contacts.

Wire layout (aMule Kademlia v2, KADEMLIA2_BOOTSTRAP_RES = 0x09):

    frame   = [0xE4 | 0xE5][0x09][payload | zlib(payload)]
    payload = <selfID 16><selfTCP u16 LE><kadVersion u8><count u16 LE>
              count * <id 16><ip 4><udpPort u16 LE><tcpPort u16 LE><version u8>

0xE5 is zlib-compressed (aMule PackPacket above 200 B). IP octets are in
network order; ports are little-endian. Only globally-routable IPv4 contacts
with a usable UDP port are emitted -- the same rule as `kad-mini live_gate`
and `kad_egress_probe.py`. Reports/targets are gitignored; never commit them.

Usage
-----
    python3 bootstrap_res_to_targets.py                       # newest LAN report
    python3 bootstrap_res_to_targets.py --report <lan-probe.json> \
        --out bootstrap.fresh.local.json --max 10 \
        --compare bootstrap.live.local.json

Then, on the host whose egress is being tested:

    python3 kad_egress_probe.py --targets bootstrap.fresh.local.json \
        --sample-size 10 --rate-limit 0.2 \
        --attest-i-am-the-experiment-operator

The wider sweep is a DIAGNOSTIC, not P4: BOOTSTRAP_REQ only, no publish / no
HELLO / no SEARCH. sample_size=10 and rate_limit=0.2 stay within the
owner-confirmed hard caps (10 / 1.0 pps) recorded in spikes.json.
"""

from __future__ import annotations

import argparse
import glob
import ipaddress
import json
import os
import struct
import sys
import zlib

BOOTSTRAP_RES = 0x09
CONTACT_LEN = 25
HERE = os.path.dirname(os.path.abspath(__file__))
REPORT_DIR = os.path.join(HERE, "reports")
DEFAULT_OUT = os.path.join(REPORT_DIR, "bootstrap.fresh.local.json")


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


def decode_frame(hex_text: str) -> bytes:
    raw = bytes.fromhex(hex_text)
    if len(raw) < 3:
        raise ValueError("frame too short")
    proto, opcode = raw[0], raw[1]
    if opcode != BOOTSTRAP_RES:
        raise ValueError("opcode 0x%02x is not BOOTSTRAP_RES" % opcode)
    if proto == 0xE4:
        return raw[2:]
    if proto == 0xE5:
        return zlib.decompress(raw[2:])
    raise ValueError("protocol byte 0x%02x is neither 0xE4 nor 0xE5" % proto)


def parse_contacts(payload: bytes) -> list[dict]:
    if len(payload) < 21:
        raise ValueError("payload shorter than BOOTSTRAP_RES header")
    count = struct.unpack_from("<H", payload, 19)[0]
    off = 21
    contacts = []
    for _ in range(count):
        if off + CONTACT_LEN > len(payload):
            raise ValueError("truncated contact list at offset %d" % off)
        ip = ".".join(str(x) for x in payload[off + 16:off + 20])
        udp, tcp, version = struct.unpack_from("<HHB", payload, off + 20)
        contacts.append({"ip": ip, "port": udp, "tcp": tcp, "kad_version": version})
        off += CONTACT_LEN
    return contacts


def newest_lan_report() -> str:
    files = sorted(glob.glob(os.path.join(REPORT_DIR, "kad-lan-probe-*.json")))
    if not files:
        raise FileNotFoundError("no kad-lan-probe-*.json in %s" % REPORT_DIR)
    return files[-1]


def extract(report_path: str) -> list[dict]:
    with open(report_path, encoding="utf-8") as f:
        report = json.load(f)
    errors = []
    for attempt in report.get("attempts", []):
        hex_text = attempt.get("response_hex", "")
        if not hex_text:
            continue
        try:
            return parse_contacts(decode_frame(hex_text))
        except (ValueError, zlib.error) as exc:
            errors.append("%s: %s" % (attempt.get("probe", "?"), exc))
    raise ValueError("no decodable BOOTSTRAP_RES in report (%s)" % "; ".join(errors))


def main() -> int:
    ap = argparse.ArgumentParser(description="Extract fresh public KAD contacts from a LAN BOOTSTRAP_RES")
    ap.add_argument("--report", help="kad-mini lan-probe report JSON (default: newest in reports/)")
    ap.add_argument("--out", default=DEFAULT_OUT, help="targets JSON to write")
    ap.add_argument("--max", type=int, default=10, help="max contacts to emit (1..10)")
    ap.add_argument("--compare", help="optional older targets JSON, to report overlap")
    args = ap.parse_args()

    if not (1 <= args.max <= 10):
        print("error: --max must be 1..10", file=sys.stderr)
        return 1
    try:
        report = args.report or newest_lan_report()
        contacts = extract(report)
    except (OSError, ValueError, json.JSONDecodeError, zlib.error) as exc:
        print("error: %s: %s" % (type(exc).__name__, exc), file=sys.stderr)
        return 1

    seen, fresh = set(), []
    for c in contacts:
        key = (c["ip"], c["port"])
        if key in seen or not is_global_ipv4(c["ip"]) or not (1 <= c["port"] <= 65535):
            continue
        seen.add(key)
        fresh.append({"ip": c["ip"], "port": c["port"]})
        if len(fresh) >= args.max:
            break
    if not fresh:
        print("error: no globally-routable IPv4 contacts in BOOTSTRAP_RES", file=sys.stderr)
        return 1

    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(fresh, f, ensure_ascii=False, indent=2)

    public_all = [c for c in contacts if is_global_ipv4(c["ip"]) and c["port"] > 0]
    standard = sum(1 for c in fresh if c["port"] == 4672)
    print("report : %s" % report)
    print("decoded: %d contacts, %d globally-routable IPv4" % (len(contacts), len(public_all)))
    print("emitted: %d fresh targets (%d on standard 4672, %d on other high ports)"
          % (len(fresh), standard, len(fresh) - standard))
    print("out    : %s" % os.path.abspath(args.out))
    if args.compare:
        try:
            with open(args.compare, encoding="utf-8") as f:
                old = json.load(f)
            old_keys = {(str(x["ip"]), int(x.get("port", x.get("udp")))) for x in old}
            overlap = [c for c in fresh if (c["ip"], c["port"]) in old_keys]
            print("compare: %d/%d fresh targets also appear in %s"
                  % (len(overlap), len(fresh), os.path.basename(args.compare)))
        except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError) as exc:
            print("compare: skipped (%s)" % exc)
    print("No packets were sent. Feed this file to kad_egress_probe.py --targets.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:  # noqa: BLE001 - diagnostic must always report clearly
        print("error: %s: %s" % (type(exc).__name__, exc), file=sys.stderr)
        sys.exit(1)
