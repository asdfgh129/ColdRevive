#!/usr/bin/env python3
"""Local-loopback e2e for the P1 daemon debts (no full p0 e2e, no external probes).

Starts coldrevived with opt-in api.rpc_secret / api.web_ui / api.websocket enabled and
verifies the real HTTP surface, then stops it. All ports are bound on 127.0.0.1 only.

    python tests/e2e/p1/run_p1.py [--bin target/debug/coldrevived]

The report is written to a temporary workdir and its path printed at the end.
This script never writes g2_claim / s6_status / evidence_valid_for_pass / auto_pass.
"""

import argparse
import base64
import hashlib
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
WS_KEY = "dGhlIHNhbXBsZSBub25jZQ=="
WS_ACCEPT = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="


def free_port():
    """Pick a port that can actually be bound for both TCP and UDP.

    Windows reserves/excludes some ephemeral UDP ports; a TCP-only probe can
    return a port that later fails with WSAEACCES (10013) for the UDP socket.
    """
    for _ in range(50):
        tcp = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        udp = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        try:
            tcp.bind(("127.0.0.1", 0))
            port = tcp.getsockname()[1]
            udp.bind(("127.0.0.1", port))
            return port
        except OSError:
            continue
        finally:
            tcp.close()
            udp.close()
    raise RuntimeError("no free TCP+UDP port pair found")


def get_json(url):
    with urllib.request.urlopen(url, timeout=3) as r:
        return r.status, json.loads(r.read().decode("utf-8"))


def post_json(url, payload):
    data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(url, data=data, headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=3) as r:
            return r.status, r.read().decode("utf-8")
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode("utf-8")


def raw_http(port, request_bytes, read_bytes=None):
    s = socket.create_connection(("127.0.0.1", port), timeout=3)
    s.sendall(request_bytes)
    if read_bytes is None:
        chunks = []
        while True:
            b = s.recv(4096)
            if not b:
                break
            chunks.append(b)
        out = b"".join(chunks)
    else:
        out = b""
        while len(out) < read_bytes:
            b = s.recv(read_bytes - len(out))
            if not b:
                break
            out += b
    s.close()
    return out


def websocket_status(port):
    """RFC6455 handshake + one server text frame, stdlib only."""
    s = socket.create_connection(("127.0.0.1", port), timeout=3)
    s.sendall(
        f"GET /ws HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\n"
        f"Connection: Upgrade\r\nSec-WebSocket-Key: {WS_KEY}\r\n"
        f"Sec-WebSocket-Version: 13\r\n\r\n".encode("ascii")
    )
    head = b""
    while b"\r\n\r\n" not in head:
        c = s.recv(1)
        if not c:
            break
        head += c
    hdr = s.recv(2)
    if len(hdr) == 2 and hdr[0] == 0x81:
        n = hdr[1] & 0x7F
        payload = b""
        while len(payload) < n:
            payload += s.recv(n - len(payload))
    else:
        payload = b""
    s.close()
    expected = base64.b64encode(hashlib.sha1((WS_KEY + WS_GUID).encode()).digest()).decode()
    return {
        "head": head.decode("latin-1"),
        "accept_ok": expected in head.decode("latin-1"),
        "opcode": hdr[0] if hdr else None,
        "payload": payload.decode("utf-8", "replace"),
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", default=os.path.join(ROOT, "target", "debug", "coldrevived"))
    ap.add_argument("--report-out", default="",
                    help="optional stable path for the JSON report; default is the temp workdir")
    args = ap.parse_args()
    binary = os.path.abspath(args.bin)
    if not os.path.isfile(binary):
        subprocess.run(
            ["cargo", "build", "--offline", "-p", "coldrevived"],
            cwd=ROOT, env={**os.environ, "CARGO_INCREMENTAL": "0"}, check=True,
        )

    work = tempfile.mkdtemp(prefix="cr_p1_e2e_")
    meta = os.path.join(work, "meta")
    os.makedirs(meta, exist_ok=True)
    sample = os.path.join(work, "sample.bin")
    with open(sample, "wb") as f:
        f.write(bytes((i * 7 + 11) % 256 for i in range(4096)))
    for name in ("golden_req.json", "golden_ack.json"):
        with open(os.path.join(work, name), "w", encoding="utf-8") as f:
            json.dump({}, f)

    http_port, udp_port, bt_port = free_port(), free_port(), free_port()
    secret = "local-e2e-secret"
    cfg = {
        "bind": "127.0.0.1",
        "nodes": {"A": {"http_port": http_port, "wake_udp_port": udp_port, "bt_port": bt_port}},
        "sample_path": sample,
        "keywords": ["sample"],
        "namespace": "coldrevive-p1-e2e",
        "workdir": work,
        "golden_req": os.path.join(work, "golden_req.json"),
        "golden_ack": os.path.join(work, "golden_ack.json"),
        "api": {"rpc_secret": secret, "web_ui": True, "websocket": True},
        "qos": {"background_upload_ratio": 0.02},
        "nat": {"auto_map": False},
        "dht": {"enabled": False},
    }
    cfg_path = os.path.join(work, "config.json")
    with open(cfg_path, "w", encoding="utf-8") as f:
        json.dump(cfg, f)

    log_path = os.path.join(work, "daemon.log")
    log = open(log_path, "w", encoding="utf-8")
    proc = subprocess.Popen(
        [binary, "--config", cfg_path, "--node-id", "A", "--workdir", work, "--sample", sample],
        cwd=ROOT, stdout=log, stderr=subprocess.STDOUT,
    )
    base = f"http://127.0.0.1:{http_port}"
    checks = {}

    def check(name, fn):
        try:
            checks[name] = {"ok": True, "detail": fn()}
        except Exception as exc:  # noqa: BLE001 - report the real error
            checks[name] = {"ok": False, "detail": f"{type(exc).__name__}: {exc}"}

    try:
        for _ in range(100):
            if proc.poll() is not None:
                break
            try:
                get_json(base + "/health")
                break
            except Exception:
                time.sleep(0.1)

        def health_check():
            status, h = get_json(base + "/health")
            assert status == 200
            assert h["api"]["rpc_secret"]["enabled"] is True
            assert secret not in json.dumps(h), "secret leaked into /health"
            assert h["api"]["web_ui"]["enabled"] is True
            assert h["api"]["websocket"]["enabled"] is True
            assert h["nat"]["status"] == "disabled"
            assert h["dht"]["status"] == "disabled"
            assert h["qos"]["enabled"] is True and h["qos"]["cap_bps"] == 250000
            assert h["reputation"]["status"] == "not_implemented"
            return {"node": h["node"], "cap_bps": h["qos"]["cap_bps"]}

        check("health_exposes_flags_not_secret", health_check)

        def openapi_check():
            status, o = get_json(base + "/openapi.json")
            assert status == 200 and o["openapi"] == "3.0.3"
            for p in ("/health", "/cli", "/jsonrpc", "/announce", "/torrent/{key}"):
                assert p in o["paths"], p
            return {"paths": sorted(o["paths"].keys())}

        check("openapi_lists_endpoints", openapi_check)

        def webui_check():
            root = raw_http(http_port, b"GET / HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n")
            assert b"200 OK" in root and b"ColdRevive console" in root, root[:200]
            assert b"/ui/app.js" in root and b"/ui/style.css" in root, root[:300]
            health = raw_http(http_port, b"GET /health HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n")
            assert b'"status":"ok"' in health and b"<!doctype html>" not in health
            js = raw_http(http_port, b"GET /ui/app.js HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n")
            assert b"200 OK" in js and b"aria2.tellActive" in js, js[:200]
            css = raw_http(http_port, b"GET /ui/style.css HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n")
            assert b"200 OK" in css and b"text/css" in css, css[:200]
            v1 = raw_http(http_port, b"GET /api/v1/status HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n")
            assert b'"status":"ok"' in v1 and b"<!doctype html>" not in v1, v1[:200]
            return {"root_html": True, "assets": True, "api_v1": True, "health_not_shadowed": True}

        check("webui_assets_and_api_v1_do_not_shadow_health", webui_check)

        def rpc_check():
            _, denied = post_json(base + "/jsonrpc", {
                "jsonrpc": "2.0", "id": 1, "method": "aria2.addUri",
                "params": [["magnet:?fake"]],
            })
            assert "-32000" in denied and secret not in denied, denied
            _, allowed = post_json(base + "/jsonrpc", {
                "jsonrpc": "2.0", "id": 2, "method": "aria2.addUri",
                "params": [f"token:{secret}", ["magnet:?fake"]],
            })
            assert '"result"' in allowed and "-32000" not in allowed, allowed
            _, readonly = post_json(base + "/jsonrpc", {
                "jsonrpc": "2.0", "id": 3, "method": "aria2.getVersion", "params": [],
            })
            assert '"result"' in readonly, readonly
            for method in ("aria2.pauseAll", "aria2.resumeAll", "aria2.removeDownloadResult"):
                _, ctl = post_json(base + "/jsonrpc", {
                    "jsonrpc": "2.0", "id": 9, "method": method,
                    "params": [f"token:{secret}", []],
                })
                assert '"result":[]' in ctl and "-32601" not in ctl, (method, ctl)
            return {"mutating_denied": True, "mutating_allowed": True,
                    "readonly_free": True, "control_methods": True}

        check("rpc_secret_gates_mutating_calls", rpc_check)

        def exposure_check():
            bad = dict(cfg)
            bad["bind"] = "0.0.0.0"
            bad["api"] = {"rpc_secret": "", "web_ui": False, "websocket": False}
            bad["nodes"] = {"A": {
                "http_port": free_port(), "wake_udp_port": free_port(), "bt_port": free_port(),
            }}
            bad_path = os.path.join(work, "config-no-secret.json")
            with open(bad_path, "w", encoding="utf-8") as f:
                json.dump(bad, f)
            p = subprocess.run(
                [binary, "--config", bad_path, "--node-id", "A",
                 "--workdir", work, "--sample", sample],
                cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                text=True, timeout=5,
            )
            assert p.returncode == 2, f"应拒绝启动，实得 {p.returncode}: {p.stdout}"
            assert "api.rpc_secret" in p.stdout and "非回环" in p.stdout, p.stdout
            return {"exit": p.returncode, "refused_before_bind": True}

        check("non_loopback_without_secret_refused", exposure_check)

        def ws_check():
            w = websocket_status(http_port)
            assert "101 Switching Protocols" in w["head"], w["head"][:200]
            assert w["accept_ok"], w["head"][:200]
            assert w["opcode"] == 0x81, w["opcode"]
            frame = json.loads(w["payload"])
            assert "node" in frame and "seed_tasks" in frame, frame
            return {"opcode": w["opcode"], "frame_keys": sorted(frame.keys())}

        check("websocket_handshake_and_frame", ws_check)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=5)
        log.close()

    report = {
        "kind": "p1-local-loopback",
        "scope": "no full p0 e2e, no external network probes",
        "binary": binary,
        "checks": checks,
        "passed": sum(1 for v in checks.values() if v["ok"]),
        "total": len(checks),
        "gate_constants_untouched": True,
    }
    out = args.report_out or os.path.join(work, "report.json")
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    with open(out, "w", encoding="utf-8") as f:
        json.dump(report, f, indent=2, ensure_ascii=False)
    for name, value in checks.items():
        print(f"[{'PASS' if value['ok'] else 'FAIL'}] {name}: {value['detail']}")
    print(f"{report['passed']}/{report['total']} checks passed; report: {out}")
    return 0 if report["passed"] == report["total"] else 1


if __name__ == "__main__":
    sys.exit(main())
