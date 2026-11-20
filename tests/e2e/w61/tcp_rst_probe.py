#!/usr/bin/env python3
"""W61：从 A 侧对 119.29.150.254:7100 做多轮纯 TCP 回显探测。

判据必须是多轮（ADR-020 复盘：曾有一次 13 B 回显成功，随后同载荷全败）。
脚本只输出原始 JSON，不自动判 pass；由 make_report.py 按证据口径判定。
"""
import json
import socket
import sys


def probe(host, port, rounds, payload):
    results = []
    for i in range(rounds):
        rec = {"round": i + 1, "ok": False, "error": None, "echo_bytes": 0}
        sock = socket.socket()
        sock.settimeout(3.0)
        try:
            sock.connect((host, port))
            sock.sendall(payload)
            got = sock.recv(len(payload))
            rec["echo_bytes"] = len(got)
            rec["ok"] = got == payload
        except Exception as exc:  # noqa: BLE001 - 原始异常就是证据
            rec["error"] = f"{type(exc).__name__}: {exc}"
        finally:
            sock.close()
        results.append(rec)
    return {
        "host": host,
        "port": port,
        "rounds": rounds,
        "payload_len": len(payload),
        "passed": sum(1 for r in results if r["ok"]),
        "results": results,
    }


def main():
    host = sys.argv[1] if len(sys.argv) > 1 else "119.29.150.254"
    port = int(sys.argv[2]) if len(sys.argv) > 2 else 7100
    rounds = int(sys.argv[3]) if len(sys.argv) > 3 else 12
    payload = (b"W61-rst-control-" * 4)[:64]
    print(json.dumps(probe(host, port, rounds, payload), ensure_ascii=False))


if __name__ == "__main__":
    main()
