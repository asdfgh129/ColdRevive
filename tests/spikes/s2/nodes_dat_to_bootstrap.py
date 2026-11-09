#!/usr/bin/env python3
"""Convert an eMule/aMule ``nodes.dat`` into a kad-mini bootstrap JSON file.

Read-only on the source file. The output path MUST be given explicitly and MUST
NOT be committed: OPS §6 forbids bundling default bootstrap node lists in the
repository. Put it under ``tests/spikes/s2/reports/`` (gitignored).

Layout is taken from aMule ``src/kademlia/routing/RoutingZone.cpp::ReadFile``:

    uint32 legacyCount   (0 => modern header)
    uint32 fileVersion   (1..3)
    uint32 numContacts
    contact := ID[16] IP[4] UDP[2] TCP[2] ver[1]
               [ KadUDPKey[8] verified[1] ]        # only when fileVersion >= 2

``server.met`` is the eD2K *server* list (magic 0xE0), not a KAD routing table.
It cannot be used to bootstrap KAD and is deliberately not parsed here.

Usage::

    python3 tests/spikes/s2/nodes_dat_to_bootstrap.py \
        --nodes-dat /path/to/nodes.dat \
        --out tests/spikes/s2/reports/bootstrap.public.local.json \
        --limit 3 --public-only

    python3 tests/spikes/s2/nodes_dat_to_bootstrap.py \
        --nodes-dat /path/to/nodes.dat \
        --out tests/spikes/s2/reports/bootstrap.lan.local.json \
        --lan-ip <PRIVATE_IP> --lan-udp <UDP_PORT> --lan-only
"""

from __future__ import annotations

import argparse
import ipaddress
import json
import struct
import sys
from pathlib import Path

MAGIC_SERVER_MET = 0xE0


def parse_contacts(raw: bytes) -> tuple[int, list[dict]]:
    if len(raw) < 12:
        raise ValueError("nodes.dat too short for a modern header")
    legacy_count, file_version, count = struct.unpack_from("<III", raw, 0)
    if legacy_count != 0:
        raise ValueError(
            "legacy (version 0) nodes.dat is unsupported: it cannot record the "
            "KAD version of contacts; aMule refuses to read it too"
        )
    if not 1 <= file_version <= 3:
        raise ValueError(f"unsupported nodes.dat fileVersion={file_version}")
    stride = 25 if file_version == 1 else 34
    body = len(raw) - 12
    if count * stride > body:
        raise ValueError(
            f"truncated file: {count} contacts x {stride}B > {body}B body"
        )

    contacts = []
    for i in range(count):
        off = 12 + i * stride
        rec = raw[off : off + stride]
        # aMule stores the IP as four network-order octets; reading them back
        # as a dotted quad gives the address without any byte swapping.
        ip = ".".join(str(b) for b in rec[16:20])
        udp, tcp = struct.unpack_from("<HH", rec, 20)
        version = rec[24]
        verified = rec[33] if stride == 34 else 0
        contacts.append(
            {
                "ip": ip,
                "port": udp,
                "tcp_port": tcp,
                "kad_version": version,
                "contact_verified": bool(verified),
                "kad_id": rec[0:16].hex(),
                # KadUDPKey is (m_key, m_ip): GetKeyValue(myIP) only returns the
                # key when myIP == m_ip. It is bound to the machine that wrote
                # nodes.dat and is therefore intentionally NOT exported.
            }
        )
    return file_version, contacts


def is_public(addr: str) -> bool:
    try:
        ip = ipaddress.ip_address(addr)
        return ip.is_global
    except ValueError:
        return False


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--nodes-dat", required=True, help="path to nodes.dat (read-only)")
    ap.add_argument("--out", required=True, help="output bootstrap JSON path (must be gitignored)")
    ap.add_argument("--limit", type=int, default=3, help="max contacts to emit (default 3)")
    ap.add_argument("--public-only", action="store_true", help="drop RFC1918 / non-global addresses")
    ap.add_argument("--min-kad-version", type=int, default=6, help="skip older KAD versions (default 6)")
    ap.add_argument("--lan-ip", help="explicit LAN eMule node to emit instead of nodes.dat contacts")
    ap.add_argument("--lan-udp", type=int, help="UDP/KAD port of the LAN eMule node")
    ap.add_argument("--lan-only", action="store_true", help="emit only the LAN node")
    ap.add_argument("--server-met", help="optional server.met path; only reported, never used for KAD")
    args = ap.parse_args()

    src = Path(args.nodes_dat)
    raw = src.read_bytes()
    file_version, contacts = parse_contacts(raw)

    if args.server_met:
        met = Path(args.server_met).read_bytes()
        servers = struct.unpack_from("<I", met, 1)[0] if len(met) >= 5 else 0
        print(
            f"[note] server.met magic=0x{met[0]:02x} servers={servers}: "
            "eD2K server list, NOT KAD contacts; ignored for bootstrap.",
            file=sys.stderr,
        )

    print(
        f"[info] nodes.dat fileVersion={file_version} contacts={len(contacts)} "
        f"verified={sum(1 for c in contacts if c['contact_verified'])}",
        file=sys.stderr,
    )
    versions: dict[int, int] = {}
    for c in contacts:
        versions[c["kad_version"]] = versions.get(c["kad_version"], 0) + 1
    print(f"[info] KAD version distribution: {versions}", file=sys.stderr)
    print(
        "[info] KadUDPKey is IP-bound to the original machine and is NOT exported.",
        file=sys.stderr,
    )

    if args.lan_only:
        if not args.lan_ip or not args.lan_udp:
            ap.error("--lan-only requires --lan-ip and --lan-udp")
        if not ipaddress.ip_address(args.lan_ip).is_private:
            ap.error("--lan-ip must be an RFC1918 private address")
        out_nodes = [
            {
                "ip": args.lan_ip,
                "port": args.lan_udp,
                "kad_version": 8,
                "scope": "lan-interop-probe-only",
                "note": "真实 eMule 客户端；仅用于 BOOTSTRAP/HELLO 互操作探测，禁止用于 live 发布",
            }
        ]
    else:
        selected = [
            c
            for c in contacts
            if c["port"] > 0
            and c["kad_version"] >= args.min_kad_version
            and (not args.public_only or is_public(c["ip"]))
        ]
        out_nodes = [
            {"ip": c["ip"], "port": c["port"], "kad_version": c["kad_version"]}
            for c in selected[: args.limit]
        ]

    if not out_nodes:
        print("[error] no usable bootstrap contacts", file=sys.stderr)
        return 1

    dest = Path(args.out)
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text(json.dumps(out_nodes, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"[ok] wrote {len(out_nodes)} node(s) to {dest}", file=sys.stderr)
    print("[warn] never commit this file: OPS §6 forbids bundled bootstrap lists", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
