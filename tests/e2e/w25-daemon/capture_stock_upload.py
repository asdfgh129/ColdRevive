#!/usr/bin/env python3
"""Capture stock-aMule uploader frames for the FR-25 reverse-A/B control arm.

Same sample and same primary aMule downloader as reverse_ab.py, but the source
is a transparent proxy in front of the second stock amuled. This gives a frame
trace directly comparable with diag_upload.py's trace of our daemon uploader.

Usage:
  python3 tests/e2e/w25-daemon/capture_stock_upload.py <sample-path> <ed2k-link> <name>
"""
from __future__ import annotations

import importlib.util
import json
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
RUN = HERE / "run"
REPORTS = HERE / "reports"
H = importlib.util.spec_from_file_location("h", HERE / "daemon_cross_chunk.py")
h = importlib.util.module_from_spec(H)
H.loader.exec_module(h)
D = importlib.util.spec_from_file_location("du", HERE / "diag_upload.py")
du = importlib.util.module_from_spec(D)
D.loader.exec_module(du)
R = importlib.util.spec_from_file_location("ra", HERE / "reverse_ab.py")
ra = importlib.util.module_from_spec(R)
R.loader.exec_module(ra)

PROXY_PORT = 4665
MAX_WAIT = int(__import__("os").environ.get("FR25_AB_MAX_WAIT_SECS", "120"))


def proxy(trace, stop: threading.Event) -> None:
    srv = socket.socket()
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("0.0.0.0", PROXY_PORT))
    srv.listen(8)
    srv.settimeout(0.5)
    trace.write(f"proxy listening 0.0.0.0:{PROXY_PORT} -> {h.AMULE_HOST}:{ra.SECOND_TCP}\n")
    trace.flush()
    while not stop.is_set():
        try:
            client, peer = srv.accept()
        except socket.timeout:
            continue
        except OSError:
            break
        trace.write(f"proxy accepted {peer}\n")
        trace.flush()
        try:
            upstream = socket.create_connection((h.AMULE_HOST, ra.SECOND_TCP), timeout=8)
        except OSError as exc:
            trace.write(f"proxy upstream failed {exc}\n")
            trace.flush()
            client.close()
            continue
        threading.Thread(target=du.pipe, args=(client, upstream, "C->S", trace.write),
                         daemon=True).start()
        threading.Thread(target=du.pipe, args=(upstream, client, "S->C", trace.write),
                         daemon=True).start()
    srv.close()


def main() -> int:
    if len(sys.argv) != 4:
        raise SystemExit("usage: capture_stock_upload.py <sample> <ed2k-link> <name>")
    sample, link, name = (Path(sys.argv[1]).resolve(), sys.argv[2], sys.argv[3])
    expected = ra.md5(sample.read_bytes())
    stamp = time.strftime("%Y%m%d%H%M%S")
    trace_path = RUN / f"stock-upload-frames-{stamp}.log"
    trace = trace_path.open("w", encoding="utf-8")
    proc = None
    stop = threading.Event()
    report: dict = {
        "schema": "coldrevive.fr25.stock-upload-capture/v1",
        "sample": {"name": name, "bytes": sample.stat().st_size, "md5": expected},
        "source": f"second stock amuled {h.AMULE_HOST}:{ra.SECOND_TCP} via proxy {PROXY_PORT}",
        "trace": str(trace_path),
        "gate_constants_untouched": True,
    }
    try:
        report["setup"] = ra.setup_second(sample, name)
        proc = ra.start_second(RUN / f"stock-capture-second-{stamp}.log")
        th = threading.Thread(target=proxy, args=(trace, stop), daemon=True)
        th.start()
        time.sleep(1)

        cancelled = du.clean_amule_downloads()
        with_sources = link + f"|sources,{h.OUR_LAN}:{PROXY_PORT}|/"
        add = h.ssh("amulecmd", "-p", "4712", "-P", ra.PASSWORD, "-c", "Add " + with_sources)
        report["cancelled_before"] = cancelled
        report["add_rc"] = add.returncode
        t0 = time.time()
        landed = ""
        while time.time() - t0 < MAX_WAIT:
            landed = ra.remote_md5(f"{ra.PRIMARY_INCOMING}/{name}")
            if landed == expected:
                break
            time.sleep(2)
        report["elapsed_sec"] = round(time.time() - t0, 1)
        report["landed_md5"] = landed
        report["md5_matches"] = landed == expected
        with du.LOCK:
            report["accounting"] = {
                "requests": du.STATE["requests"],
                "request_pairs": du.STATE["request_pairs"],
                "sends": du.STATE["sends"],
                "bytes_sent": du.STATE["bytes_sent"],
                "completed": du.STATE["completed"],
                "pending": len(du.STATE["pending"]),
                "dropped_exceeds": du.STATE["dropped_exceeds"],
                "dropped_unknown": du.STATE["dropped_unknown"],
            }
    finally:
        stop.set()
        report["cleanup"] = ra.stop_second(proc)
        trace.close()

    text = trace_path.read_text(encoding="utf-8", errors="replace")
    counts = {}
    for line in text.splitlines():
        if "op=" not in line:
            continue
        op = line.split("op=")[1].split(")")[0]
        counts[op] = counts.get(op, 0) + 1
    report["opcode_counts"] = counts
    out = REPORTS / f"stock-upload-capture-{stamp}.json"
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"report: {out}")
    print(f"md5_matches={report.get('md5_matches')} accounting={report.get('accounting')}")
    print(f"opcodes={counts}")
    return 0 if report.get("md5_matches") else 1


if __name__ == "__main__":
    raise SystemExit(main())
