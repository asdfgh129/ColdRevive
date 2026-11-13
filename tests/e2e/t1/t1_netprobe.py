#!/usr/bin/env python3
"""T1 环境可达性探针：实测本机各本地 IP 之间能否互发 UDP。

用途（不是验收结论）：
  为「为什么单机预检不能用两块网卡伪装成双机」提供机读证据。
  每个组合都真实创建 UDP socket、bind 源 IP、sendto 目的 IP、recvfrom 收包；
  失败时如实记录 WSA 错误码，绝不用推断值填充。

用法: python3 tests/e2e/t1/t1_netprobe.py [--ips 192.168.1.210,172.26.96.1,...]
退出码恒为 0（这是量测，不是门禁）；报告落在 tests/e2e/t1/reports/。
"""
from __future__ import annotations

import argparse
import json
import socket
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
DEFAULT_IPS = ["192.168.1.210", "172.26.96.1", "127.0.0.1", "127.0.0.2", "127.0.0.3"]


def probe(src: str, dst: str) -> dict:
    srv = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        srv.bind((dst, 0))
    except OSError as exc:
        srv.close()
        return {"src": src, "dst": dst, "reachable": False,
                "stage": "bind_server", "error": str(exc), "errno": exc.errno}
    port = srv.getsockname()[1]
    received: list = []

    def recv() -> None:
        srv.settimeout(1.0)
        try:
            data, addr = srv.recvfrom(64)
            received.append((data, addr))
        except OSError as exc:
            received.append((b"", str(exc)))

    t = threading.Thread(target=recv, daemon=True)
    t.start()
    cli = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    result = {"src": src, "dst": dst, "dst_port": port}
    try:
        cli.bind((src, 0))
        cli.settimeout(1.0)
        result["src_port"] = cli.getsockname()[1]
        cli.sendto(b"CRT1-PING", (dst, port))
    except OSError as exc:
        result.update(reachable=False, stage="send", error=str(exc), errno=exc.errno)
    else:
        t.join(timeout=1.3)
        ok = bool(received) and received[0][0] == b"CRT1-PING"
        result.update(reachable=ok, stage="recv" if ok else "timeout",
                      error="" if ok else "no echo datagram received")
    finally:
        cli.close()
        srv.close()
    return result


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ips", default=",".join(DEFAULT_IPS))
    ap.add_argument("--out", type=Path, default=None)
    args = ap.parse_args()
    ips = [x.strip() for x in args.ips.split(",") if x.strip()]
    rows = [probe(a, b) for a in ips for b in ips]
    cross = [r for r in rows if r["src"] != r["dst"]]
    report = {
        "suite": "T1", "harness": "tests/e2e/t1/t1_netprobe.py",
        "recorded_at": datetime.now(timezone.utc).isoformat(),
        "method": "真实 UDP socket：bind(src) → sendto(dst) → recvfrom，逐组合实测",
        "ips": ips,
        "results": rows,
        "summary": {
            "pairs_total": len(rows),
            "same_ip_pairs_ok": sum(1 for r in rows if r["src"] == r["dst"] and r["reachable"]),
            "cross_ip_pairs_total": len(cross),
            "cross_ip_pairs_ok": sum(1 for r in cross if r["reachable"]),
        },
        "interpretation": "跨 IP 组合若全部不可达，说明本机无法用两块网卡/别名伪装成两台主机，"
                          "T1 必须换用两台真实物理机器；此结论只描述环境，不构成任何门禁判据。",
        "evidence_valid_for_pass": False,
    }
    out = args.out or HERE / "reports" / (
        "t1-netprobe-" + datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + ".json")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    s = report["summary"]
    print(f"跨 IP 可达 {s['cross_ip_pairs_ok']}/{s['cross_ip_pairs_total']}；"
          f"同 IP 可达 {s['same_ip_pairs_ok']}/{len(ips)}")
    for r in cross:
        print(f"  {r['src']:<16} -> {r['dst']:<16} "
              f"{'OK' if r['reachable'] else r.get('error', '?')}")
    print("report:", out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
