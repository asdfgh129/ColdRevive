#!/usr/bin/env python3
"""家宽 kit：Mainline DHT 只读探测（KRPC ping / get_peers，绝不 announce）。

用 Python 标准库复刻 cr-dht CLI 的最小只读面，让没有 Rust 工具链的 macOS 家宽
也能对同一批目标取证。它回答的是：

  * 这台家宽主机能否与公网 DHT 节点完成 UDP KRPC 往返？
  * 对自有公网节点 82.157.68.159:6881 的既有 infohash 回查，是否仍能拿到 peer？
  * 公网引导节点是否返回 token（能 announce）还是 token_len=0 / 超时？

红线：
  * **不发送 announce_peer**，不发布任何 infohash；get_peers 是只读查询；
  * 单个目标最多 2 个请求（ping + 可选 get_peers），低速、有界超时；
  * 任何失败如实记 BLOCKED/timeout，不自动判 pass，不改 spikes.json；
  * `g2_claim=false`、`s6_status=not_run`、`evidence_valid_for_pass=false` 恒定。

用法:
  python3 probes/dht_readonly_probe.py --config config/dht-targets.json \
      --out results/dht.json
  python3 probes/dht_readonly_probe.py --self-test
"""

from __future__ import annotations

import argparse
import json
import os
import socket
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Optional, Tuple

# ---- 最小 bencode ---------------------------------------------------------


def bencode(value) -> bytes:
    if isinstance(value, int):
        return b"i" + str(value).encode("ascii") + b"e"
    if isinstance(value, str):
        value = value.encode("utf-8")
    if isinstance(value, bytes):
        return str(len(value)).encode("ascii") + b":" + value
    if isinstance(value, (list, tuple)):
        return b"l" + b"".join(bencode(item) for item in value) + b"e"
    if isinstance(value, dict):
        keys = sorted(value.keys(), key=lambda k: k if isinstance(k, bytes) else str(k).encode())
        return b"d" + b"".join(bencode(k) + bencode(value[k]) for k in keys) + b"e"
    raise TypeError("cannot bencode %s" % type(value).__name__)


def bdecode(data: bytes):
    value, end = _decode_at(data, 0)
    if end != len(data):
        raise ValueError("trailing bytes after bencoded value")
    return value


def _decode_at(data: bytes, pos: int):
    if pos >= len(data):
        raise ValueError("unexpected end of bencoded data")
    marker = data[pos:pos + 1]
    if marker == b"i":
        end = data.index(b"e", pos + 1)
        return int(data[pos + 1:end]), end + 1
    if marker == b"l":
        items: List = []
        pos += 1
        while data[pos:pos + 1] != b"e":
            item, pos = _decode_at(data, pos)
            items.append(item)
        return items, pos + 1
    if marker == b"d":
        result: Dict[bytes, object] = {}
        pos += 1
        while data[pos:pos + 1] != b"e":
            key, pos = _decode_at(data, pos)
            if not isinstance(key, bytes):
                raise ValueError("dictionary key must be a byte string")
            result[key], pos = _decode_at(data, pos)
        return result, pos + 1
    if marker.isdigit():
        colon = data.index(b":", pos)
        length = int(data[pos:colon])
        start = colon + 1
        return data[start:start + length], start + length
    raise ValueError("unknown bencode marker %r at %d" % (marker, pos))


def parse_nodes(blob: bytes) -> List[Dict]:
    nodes: List[Dict] = []
    for offset in range(0, len(blob) - 25, 26):
        chunk = blob[offset:offset + 26]
        nodes.append({
            "id": chunk[:20].hex(),
            "address": "%s:%d" % (socket.inet_ntoa(chunk[20:24]),
                                  int.from_bytes(chunk[24:26], "big")),
        })
    return nodes


def parse_peers(values: List[bytes]) -> List[str]:
    peers: List[str] = []
    for value in values or []:
        if len(value) == 6:
            peers.append("%s:%d" % (socket.inet_ntoa(value[:4]),
                                    int.from_bytes(value[4:6], "big")))
        else:
            peers.append("unparsed:%s" % value.hex())
    return peers


# ---- 网络探测 -------------------------------------------------------------

def resolve(host: str, port: int) -> Optional[Tuple[str, int]]:
    try:
        infos = socket.getaddrinfo(host, port, socket.AF_INET, socket.SOCK_DGRAM)
        if not infos:
            return None
        sockaddr = infos[0][4]
        return (str(sockaddr[0]), int(sockaddr[1]))
    except OSError:
        return None


def exchange(sock: socket.socket, addr: Tuple[str, int], query: Dict, timeout: float) -> Dict:
    tid = os.urandom(4)
    query = dict(query)
    query[b"t"] = tid
    query[b"y"] = b"q"
    row: Dict = {"ok": False}
    try:
        payload = bencode(query)
        started = time.time()
        sock.sendto(payload, addr)
        sock.settimeout(timeout)
        while True:
            data, source = sock.recvfrom(4096)
            if len(data) < 2:
                continue
            decoded = bdecode(data)
            if not isinstance(decoded, dict) or decoded.get(b"t") != tid:
                continue  # 不是本次事务的应答，丢弃但留在同一 socket 上继续等
            row["rtt_ms"] = round((time.time() - started) * 1000, 1)
            row["response_from"] = "%s:%d" % (source[0], source[1])
            row["y"] = decoded.get(b"y", b"?").decode("ascii", "replace")
            body = decoded.get(b"r") or decoded.get(b"e")
            if not isinstance(body, dict):
                row["error"] = "response has no r/e dictionary"
                return row
            row["ok"] = decoded.get(b"y") == b"r"
            row["remote_id"] = body[b"id"].hex() if isinstance(body.get(b"id"), bytes) else None
            if isinstance(body.get(b"token"), bytes):
                row["token_len"] = len(body[b"token"])
            else:
                row["token_len"] = 0
            if isinstance(body.get(b"nodes"), bytes):
                parsed = parse_nodes(body[b"nodes"])
                row["nodes"] = len(parsed)
                row["node_sample"] = parsed[:3]
            else:
                row["nodes"] = 0
            if isinstance(body.get(b"values"), list):
                peers = parse_peers(body[b"values"])
                row["peers"] = len(peers)
                row["peer_list"] = peers
            else:
                row["peers"] = 0
            return row
    except (OSError, ValueError, KeyError, TypeError) as exc:
        row["error"] = "%s: %s" % (type(exc).__name__, exc)
        return row


def probe_target(sock: socket.socket, target: Dict, timeout: float) -> Dict:
    host = str(target.get("host", ""))
    port = int(target.get("port", 6881))
    row: Dict = {
        "label": target.get("label") or host,
        "target": "%s:%d" % (host, port),
        "resolved": None,
        "ping": None,
        "get_peers": None,
    }
    addr = resolve(host, port)
    if addr is None:
        row["error"] = "DNS resolution failed"
        return row
    row["resolved"] = "%s:%d" % addr

    node_id = os.urandom(20)
    ping = exchange(sock, addr, {b"q": b"ping", b"a": {b"id": node_id}}, timeout)
    row["ping"] = ping

    ih_hex = str(target.get("infohash") or "").strip()
    if ih_hex:
        try:
            infohash = bytes.fromhex(ih_hex)
            if len(infohash) != 20:
                raise ValueError("infohash must be 20 bytes")
        except ValueError as exc:
            row["get_peers"] = {"ok": False, "error": "invalid infohash: %s" % exc}
            return row
        row["get_peers"] = exchange(
            sock, addr,
            {b"q": b"get_peers", b"a": {b"id": node_id, b"info_hash": infohash}},
            timeout,
        )
    return row


def self_test() -> int:
    checks: List[Tuple[str, bool]] = []
    value = {b"t": b"\x01\x02", b"y": b"r", b"r": {b"id": b"i" * 20, b"token": b"tok"}}
    checks.append(("bencode_roundtrip", bdecode(bencode(value)) == value))
    node = b"n" * 20 + socket.inet_pton(socket.AF_INET, "112.5.32.34") + (17200).to_bytes(2, "big")
    parsed = parse_nodes(node)
    checks.append(("compact_node_parse", parsed == [{"id": "6e" * 20, "address": "112.5.32.34:17200"}]))
    peer = socket.inet_pton(socket.AF_INET, "112.5.60.4") + (12345).to_bytes(2, "big")
    checks.append(("compact_peer_parse", parse_peers([peer]) == ["112.5.60.4:12345"]))
    # 伪事务：应答 TID 不匹配必须继续等，不能把别人的包记成自己的结果
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(("127.0.0.1", 0))
    other = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    wrong = bencode({b"t": b"wrong", b"y": b"r", b"r": {b"id": b"x" * 20}})
    other.sendto(wrong, sock.getsockname())
    time.sleep(0.05)
    result = exchange(sock, sock.getsockname(),
                      {b"q": b"ping", b"a": {b"id": b"z" * 20}}, 0.2)
    checks.append(("mismatched_tid_is_not_counted", result.get("ok") is not True))
    other.close()
    sock.close()
    for name, ok in checks:
        print("  [%s] %s" % ("PASS" if ok else "FAIL", name))
    total = sum(1 for _, ok in checks if ok)
    print("dht_readonly self-test: %d/%d PASS (no network, evidence_valid_for_pass=false)" %
          (total, len(checks)))
    return 0 if total == len(checks) else 1


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--config", default="")
    ap.add_argument("--target", action="append", default=[],
                    help="直接指定 host:port[:infohash]，可重复；给 --config 时忽略")
    ap.add_argument("--timeout", type=float, default=2.5)
    ap.add_argument("--out", default="")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return self_test()

    targets: List[Dict] = []
    if args.config:
        payload = json.loads(Path(args.config).read_text(encoding="utf-8"))
        targets = list(payload.get("targets") or [])
    for spec in args.target:
        parts = spec.split(":")
        if len(parts) not in (2, 3):
            raise SystemExit("--target 格式 host:port[:infohash]，收到: %s" % spec)
        item: Dict = {"host": parts[0], "port": int(parts[1])}
        if len(parts) == 3:
            item["infohash"] = parts[2]
        targets.append(item)
    if not targets:
        raise SystemExit("没有目标：请给 --config 或 --target")

    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(("0.0.0.0", 0))
    results = [probe_target(sock, target, args.timeout) for target in targets]
    sock.close()

    report = {
        "schema": "coldrevive.homeprobe.dht-readonly/v1",
        "recorded_at": datetime.now(timezone.utc).isoformat(),
        "harness": "tests/e2e/homeprobe-macos/probes/dht_readonly_probe.py",
        "scope": "read-only-public-dht",
        "announce_sent": False,
        "results": results,
        "g2_claim": False,
        "s6_status": "not_run",
        "evidence_valid_for_pass": False,
        "note": "KRPC ping/get_peers 只证明这台主机与该节点有 UDP 往返；"
                "token_len=0/超时不得写成 Mainline 已加入，authorization 变更需另行审批",
    }
    out = Path(args.out) if args.out else Path("results/dht.json")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    for row in results:
        ping = row.get("ping") or {}
        gp = row.get("get_peers") or {}
        print("  DHT %-28s ping=%s token_len=%s peers=%s" % (
            row["label"], "OK" if ping.get("ok") else (ping.get("error") or "NO"),
            gp.get("token_len", "-"), gp.get("peers", "-")))
    print("  report: %s" % out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
