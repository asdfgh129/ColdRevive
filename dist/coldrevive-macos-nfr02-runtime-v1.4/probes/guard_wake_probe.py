#!/usr/bin/env python3
"""本地握手唤醒功能复核：同机连 bt_port，发 68 B BT 握手，取整片验 md5。

这是**功能**复核，不证明 guard/data-plane 线程的优先级；Darwin 上逐线程优先级
只能由线程自己读回，本探针不做进程级读数冒充。
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import socket
import struct
import sys
import urllib.request
from typing import Optional

BLOCK = 16384


def get_json(url: str, timeout: float = 5.0) -> dict:
    with urllib.request.urlopen(url, timeout=timeout) as resp:
        return json.loads(resp.read().decode("utf-8"))


def parse_args() -> argparse.Namespace:
    ap = argparse.ArgumentParser()
    ap.add_argument("--http", default="http://127.0.0.1:19600")
    ap.add_argument("--bt-host", default="127.0.0.1")
    ap.add_argument("--bt-port", type=int, default=19602)
    ap.add_argument("--infohash", required=True)
    ap.add_argument("--expected-md5", required=True)
    ap.add_argument("--piece-size", type=int, default=262144)
    ap.add_argument("--total", type=int, default=262144)
    ap.add_argument("--out", required=True)
    return ap.parse_args()


def main() -> int:
    args = parse_args()
    out_path = args.out
    result: dict = {
        "schema": "coldrevive.nfr02.guard-wake-functional/v1",
        "recorded_at_utc": dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat(),
        "note": "功能复核：握手唤醒 + 分片 md5；不证明唤醒线程优先级。",
        "bt_target": f"{args.bt_host}:{args.bt_port}",
        "infohash": args.infohash,
        "expected_md5": args.expected_md5,
        "checks": [],
    }

    def add(name: str, ok: bool, observed: object) -> None:
        result["checks"].append({
            "name": name,
            "status": "pass" if ok else "fail",
            "observed": observed,
        })

    try:
        before = get_json(args.http + "/health")
    except Exception as exc:
        result["error"] = f"before /health unavailable: {exc}"
        _write(out_path, result)
        print(result["error"], file=sys.stderr)
        return 2

    gb = before.get("guard") if isinstance(before, dict) else None
    gb = gb if isinstance(gb, dict) else {}
    add("guard enabled before probe", gb.get("enabled") is True, gb)

    ih = bytes.fromhex(args.infohash)
    peer_id = b"-CR0002-" + b"macnfr02".ljust(12, b"0")[:12]
    sock: Optional[socket.socket] = None
    try:
        sock = socket.create_connection((args.bt_host, args.bt_port), timeout=10)
        sock.sendall(b"\x13BitTorrent protocol" + b"\x00" * 8 + ih + peer_id)
        buf = bytearray()
        sock.settimeout(20)
        while len(buf) < 68:
            chunk = sock.recv(4096)
            if not chunk:
                raise RuntimeError("握手阶段 EOF（guard 可能丢了已读的 68 B）")
            buf.extend(chunk)
        add("handshake echoed 68 bytes", len(buf) >= 68, {"bytes": len(buf)})

        after_wake = get_json(args.http + "/health")
        gw = after_wake.get("guard") if isinstance(after_wake, dict) else None
        gw = gw if isinstance(gw, dict) else {}
        add("guard recorded handshake",
            gw.get("handshakes", 0) > gb.get("handshakes", 0),
            {"before": gb.get("handshakes"), "after": gw.get("handshakes")})
        add("guard woke seed task",
            gw.get("woken", 0) > gb.get("woken", 0),
            {"before": gb.get("woken"), "after": gw.get("woken")})
        add("seed_active >= 1 after wake",
            after_wake.get("seed_active", 0) >= 1,
            {"seed_active": after_wake.get("seed_active")})

        sock.sendall(struct.pack(">IB", 1, 2))
        rest = bytearray(buf[68:])
        unchoked = False
        sock.settimeout(20)
        while not unchoked:
            while len(rest) >= 4:
                ln = struct.unpack(">I", rest[:4])[0]
                if len(rest) < 4 + ln:
                    break
                mid = rest[4] if ln > 0 else None
                rest = rest[4 + ln:]
                if mid == 1:
                    unchoked = True
                    break
            if unchoked:
                break
            chunk = sock.recv(4096)
            if not chunk:
                raise RuntimeError("等 unchoke 时 EOF")
            rest.extend(chunk)
        add("unchoke received", unchoked, {"unchoked": unchoked})

        np = (args.total + args.piece_size - 1) // args.piece_size
        for pi in range(np):
            plen = min(args.piece_size, args.total - pi * args.piece_size)
            for begin in range(0, plen, BLOCK):
                ln = min(BLOCK, plen - begin)
                sock.sendall(struct.pack(">IBIII", 13, 6, pi, begin, ln))

        pieces: dict = {}
        got = 0
        sock.settimeout(30)
        while got < args.total:
            while len(rest) >= 4:
                ln = struct.unpack(">I", rest[:4])[0]
                if len(rest) < 4 + ln:
                    break
                if ln == 0:
                    rest = rest[4:]
                    continue
                mid = rest[4]
                if mid == 7:
                    idx, begin = struct.unpack(">II", bytes(rest[5:13]))
                    pieces[(idx, begin)] = bytes(rest[13:4 + ln])
                    got += ln - 9
                rest = rest[4 + ln:]
            if got >= args.total:
                break
            try:
                chunk = sock.recv(65536)
            except socket.timeout:
                break
            if not chunk:
                break
            rest.extend(chunk)

        data = b"".join(pieces[k] for k in sorted(pieces))
        md5 = hashlib.md5(data).hexdigest() if got == args.total else None
        result["bytes_received"] = got
        result["md5_received"] = md5
        add("full sample received", got == args.total, {"bytes": got, "expected": args.total})
        add("md5 matches fixture", md5 == args.expected_md5,
            {"received": md5, "expected": args.expected_md5})
    except Exception as exc:
        result["error"] = f"{type(exc).__name__}: {exc}"
    finally:
        if sock is not None:
            try:
                sock.close()
            except OSError:
                pass

    result["counts"] = {
        "pass": sum(c["status"] == "pass" for c in result["checks"]),
        "fail": sum(c["status"] == "fail" for c in result["checks"]),
    }
    _write(out_path, result)
    for c in result["checks"]:
        print(f"  [{c['status']:>4}] {c['name']}")
    print(f"  md5={result.get('md5_received')} expected={args.expected_md5}")
    return 0 if result["counts"]["fail"] == 0 and "error" not in result else 1


def _write(path: str, obj: dict) -> None:
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(obj, fh, ensure_ascii=False, indent=2)
        fh.write("\n")


if __name__ == "__main__":
    sys.exit(main())
