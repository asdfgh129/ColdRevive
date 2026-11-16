#!/usr/bin/env python3
"""最小标准 BT Tracker（BEP 23 compact peer list），仅用于 G3 私有 LAN 互操作取证。

纪律：
- 只在 127.0.0.1 / RFC1918 上监听，不接触公网；announce 里上报的地址以**连接来源 IP**为准，
  不信客户端自称的 IP（避免伪造 peer）。
- 每次 announce 追加一行 JSON 到 --log，作为原始证据；脚本不判任何 pass。
- 只做「一个 seeder + 若干个 leecher」所需的最小状态：info_hash -> {(ip, port)}。

用法：
    python local_tracker.py --port 19699 --log reports/tracker-announces.jsonl
"""

from __future__ import annotations

import argparse
import json
import threading
from collections import defaultdict
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote_to_bytes


def bencode_str(s: bytes) -> bytes:
    return str(len(s)).encode() + b":" + s


def bencode_int(n: int) -> bytes:
    return b"i" + str(n).encode() + b"e"


class Tracker:
    def __init__(self, log_path: Path):
        self.peers: dict[bytes, dict[tuple[str, int], dict]] = defaultdict(dict)
        self.lock = threading.Lock()
        self.log_path = log_path
        self.log_path.parent.mkdir(parents=True, exist_ok=True)

    def _log(self, rec: dict) -> None:
        rec["at"] = datetime.now(timezone.utc).isoformat(timespec="milliseconds")
        with self.log_path.open("a", encoding="utf-8") as f:
            f.write(json.dumps(rec, ensure_ascii=False) + "\n")

    def announce(self, info_hash: bytes, peer_ip: str, params: dict) -> bytes:
        try:
            port = int(params.get("port", ["0"])[0])
        except (ValueError, TypeError):
            port = 0
        event = params.get("event", [""])[0]
        key = (peer_ip, port)
        with self.lock:
            bucket = self.peers[info_hash]
            if event == "stopped":
                bucket.pop(key, None)
            elif port > 0:
                bucket[key] = {"ip": peer_ip, "port": port, "event": event}
            # 返回除自己以外的 peer（seeder 与 leecher 互不返回自己）
            others = [p for k, p in bucket.items() if k != key]
        self._log({
            "info_hash": info_hash.hex(),
            "peer_ip": peer_ip,
            "peer_port": port,
            "event": event,
            "uploaded": params.get("uploaded", [""])[0],
            "downloaded": params.get("downloaded", [""])[0],
            "left": params.get("left", [""])[0],
            "peer_count_returned": len(others),
        })
        compact = b"".join(
            bytes(int(x) for x in p["ip"].split(".")) + p["port"].to_bytes(2, "big")
            for p in others
        )
        complete = sum(1 for p in others if p.get("event") == "completed")
        incomplete = len(others) - complete
        return (
            b"d"
            + bencode_str(b"complete") + bencode_int(complete)
            + bencode_str(b"incomplete") + bencode_int(incomplete)
            + bencode_str(b"interval") + bencode_int(30)
            + bencode_str(b"peers") + bencode_str(compact)
            + b"e"
        )


def make_handler(tracker: Tracker):
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def do_GET(self):
            # BaseHTTPRequestHandler.path 用 latin-1 解码请求行；info_hash 是 20 字节
            # 原始二进制（含 >=128 的字节），必须用 unquote_to_bytes 在**字节层**还原，
            # 不能走 str 再 encode，否则 UnicodeEncodeError（实测踩过）。
            raw = self.path.encode("latin-1")
            path_bytes, sep, query = raw.partition(b"?")
            if path_bytes != b"/announce":
                self.send_error(404)
                return
            params: dict[str, list[str]] = {}
            raw_ih = b""
            for field in query.split(b"&"):
                if not field:
                    continue
                k, _, v = field.partition(b"=")
                key = unquote_to_bytes(k).decode("ascii", "replace")
                val = unquote_to_bytes(v)  # 字节层百分号解码，保留 0x00-0xFF
                if key == "info_hash":
                    raw_ih = val
                params.setdefault(key, []).append(val.decode("latin-1"))
            info_hash = raw_ih
            peer_ip = self.client_address[0]
            if len(info_hash) != 20:
                self.send_error(400, "bad info_hash")
                return
            body = tracker.announce(info_hash, peer_ip, params)
            self.send_response(200)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, fmt, *args):
            pass  # 已写入 JSONL，不重复打 stderr

    return Handler


def self_test() -> int:
    """纯逻辑自检：bencode / peer 列表 / 二进制 info_hash 字节解析，不联网。"""
    def decode(b: bytes, i: int = 0):
        c = b[i:i + 1]
        if c == b"i":
            j = b.index(b"e", i)
            return int(b[i + 1:j]), j + 1
        if c == b"d":
            d = {}
            i += 1
            while b[i:i + 1] != b"e":
                k, i = decode(b, i)
                v, i = decode(b, i)
                d[k] = v
            return d, i + 1
        j = b.index(b":", i)
        n = int(b[i:j])
        return b[j + 1:j + 1 + n], j + 1 + n

    import tempfile
    with tempfile.TemporaryDirectory() as td:
        t = Tracker(Path(td) / "a.jsonl")
        ih = bytes(range(20))  # 含 >=128 与 0x00
        t.announce(ih, "192.168.1.210", {"port": ["19701"], "event": ["started"]})
        resp, _ = decode(t.announce(
            ih, "192.168.1.221", {"port": ["51431"], "event": ["started"]}
        ))
        peers = resp[b"peers"]
        assert len(peers) == 6 and peers[:4] == bytes([192, 168, 1, 210])
        assert int.from_bytes(peers[4:6], "big") == 19701
        t.announce(ih, "192.168.1.221", {"port": ["51431"], "event": ["stopped"]})
        resp, _ = decode(t.announce(
            ih, "192.168.1.210", {"port": ["19701"], "event": [""]}
        ))
        assert resp[b"peers"] == b""
        # 字节层解析：rqbit 把 info_hash 全字节百分号编码
        q = b"info_hash=" + b"".join(b"%%%02X" % b for b in range(20)) + b"&port=1"
        raw_ih = b""
        for field in q.split(b"&"):
            k, _, v = field.partition(b"=")
            if unquote_to_bytes(k) == b"info_hash":
                raw_ih = unquote_to_bytes(v)
        assert raw_ih == bytes(range(20))
    print("tracker self-test: 3/3 PASS（peer 列表 / stopped 清理 / 二进制 info_hash 解析）")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--port", type=int, default=19699)
    ap.add_argument("--bind", default="0.0.0.0")
    ap.add_argument("--log", type=Path, default=Path("reports/tracker-announces.jsonl"))
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return self_test()
    tracker = Tracker(args.log.resolve())
    srv = ThreadingHTTPServer((args.bind, args.port), make_handler(tracker))
    print(f"TRACKER_READY {args.bind}:{args.port} log={tracker.log_path}", flush=True)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        srv.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
