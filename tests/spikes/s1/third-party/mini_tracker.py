#!/usr/bin/env python3
"""Minimal single-file HTTP BitTorrent tracker (stdlib only).

Used by the S1 third-party-client interop round: rqbit's `cross-seed` seeder
announces to this tracker, and a foreign download-only client (Motrix Next's
bundled aria2c) queries it to discover rqbit's peer address. No DHT / LSD is
used; the tracker is the only discovery path, which is exactly what we want to
prove (a real client can fetch data from rqbit over real BT Peer Wire).

Run:  python3 mini_tracker.py [port]   (default 16999)
"""
import json
import socket
import struct
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse

PEERS = {}  # info_hash(bytes) -> list[(ip_str, port)]
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 16999


def compact_peers(info_hash: bytes, exclude_ip: str, exclude_port: int) -> bytes:
    out = b""
    for ip, port in PEERS.get(info_hash, []):
        if ip == exclude_ip and port == exclude_port:
            continue
        try:
            out += socket.inet_aton(ip) + struct.pack(">H", port)
        except OSError:
            continue
    return out


class Handler(BaseHTTPRequestHandler):
    def _handle(self):
        parsed = urlparse(self.path)
        if parsed.path != "/announce":
            self.send_response(404)
            self.end_headers()
            return
        sys.stderr.write(
            f"RAW src={self.client_address} path={self.path}\n"
        )
        sys.stderr.flush()
        q = parsed.query
        params = {}
        for pair in q.split("&"):
            if not pair:
                continue
            k, _, v = pair.partition("=")
            params[k] = v

        # info_hash / peer_id arrive percent-encoded as raw bytes
        info_hash = bytearray()
        i = 0
        raw = params.get("info_hash", "")
        while i < len(raw):
            if raw[i] == "%" and i + 2 < len(raw):
                info_hash.append(int(raw[i + 1 : i + 3], 16))
                i += 3
            else:
                info_hash.append(ord(raw[i]))
                i += 1
        info_hash = bytes(info_hash)

        peer_id = params.get("peer_id", "")
        pid = bytearray()
        i = 0
        while i < len(peer_id):
            if peer_id[i] == "%" and i + 2 < len(peer_id):
                pid.append(int(peer_id[i + 1 : i + 3], 16))
                i += 3
            else:
                pid.append(ord(peer_id[i]))
                i += 1
        pid = bytes(pid)

        try:
            port = int(params.get("port", "0"))
        except ValueError:
            port = 0
        event = params.get("event", "")
        ip = self.client_address[0]

        sys.stderr.write(
            f"ANNOUNCE ih={info_hash.hex()[:12]} peer={ip}:{port} event={event}\n"
        )
        sys.stderr.flush()

        if info_hash not in PEERS:
            PEERS[info_hash] = []

        if event == "stopped":
            PEERS[info_hash] = [
                (p, pr) for (p, pr) in PEERS[info_hash] if not (p == ip and pr == port)
            ]
        else:
            lst = PEERS[info_hash]
            lst[:] = [(p, pr) for (p, pr) in lst if not (p == ip and pr == port)]
            if pid and port:
                lst.append((ip, port))

        peers_blob = compact_peers(info_hash, ip, port)
        seeders = sum(1 for _ in PEERS[info_hash])
        sys.stderr.write(
            f"  -> known_peers={len(PEERS[info_hash])} returned_blob={len(peers_blob)}\n"
        )
        sys.stderr.flush()
        resp = (
            b"d8:completei"
            + str(seeders).encode()
            + b"e10:incompletei0e8:intervali1800e5:peers"
            + str(len(peers_blob)).encode()
            + b":"
            + peers_blob
            + b"e"
        )
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(resp)))
        self.end_headers()
        self.wfile.write(resp)

    def do_GET(self):
        try:
            self._handle()
        except Exception as e:  # noqa: BLE001
            body = str(e).encode()
            self.send_response(500)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    def log_message(self, *args):  # silence default logging
        pass


if __name__ == "__main__":
    srv = ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    print(json.dumps({"event": "tracker_up", "addr": f"127.0.0.1:{PORT}"}))
    sys.stdout.flush()
    srv.serve_forever()
