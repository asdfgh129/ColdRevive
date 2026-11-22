#!/usr/bin/env python3
"""FR-25 upload-direction stall diagnostic (coldrevived serve -> real aMule).

Why this exists: the upload direction stalls part-way through a `>PARTSIZE`
file and aMule has to tell us why. It cannot:

  * `VerboseDebug=1` + `-o` produce nothing, because aMule compiles every
    `AddDebugLogLineN()` out of release builds (`#define AddDebugLogLineN(...)
    do {} while(false)` unless `__DEBUG__`). Verified against the 2.3.3 source
    and against the binary itself (`strings /usr/bin/amuled | grep "exceeds
    requested"` -> 0 hits). Shipping users run exactly that binary, so a debug
    rebuild would change the peer under test.

So this tool observes what aMule actually acts on, from three places that do
not require aMule internals:

  1. a transparent TCP proxy records every eD2k frame in both directions;
  2. the same proxy replays aMule's own block accounting over that trace
     (`CUpDownClient::ProcessBlockPacket`): a frame whose inclusive end equals
     the pending block's end completes it, a frame starting past the block end
     makes aMule *drop* the block, everything else is a partial write;
  3. aMule's own download list (`amulecmd Show DL`) plus `ss -tn` Recv-Q show
     whether the peer is still reading or has stopped consuming.

It renders no verdict on gate constants and never claims PASS. It writes one
JSON report of raw observations.

Usage:
  python3 tests/e2e/w25-daemon/diag_upload.py
Env:
  FR25_SAMPLE_BYTES     sample size (default PARTSIZE + 8192)
  FR25_STALL_SECS       no-progress window that counts as a stall (default 30)
  FR25_MAX_WAIT_SECS    hard cap on the watch loop (default 300)
"""
from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import re
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
RUN = HERE / "run"
REPORTS = HERE / "reports"
_spec = importlib.util.spec_from_file_location("h", HERE / "daemon_cross_chunk.py")
h = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(h)

PART_SIZE = 9_500 * 1024
BLOCK = 184_320
SAMPLE_BYTES = int(os.environ.get("FR25_SAMPLE_BYTES", str(PART_SIZE + 8192)))
STALL_SECS = int(os.environ.get("FR25_STALL_SECS", "30"))
MAX_WAIT = int(os.environ.get("FR25_MAX_WAIT_SECS", "300"))
OUR_LAN = h.OUR_LAN
PROXY_PORT = 4663
DAEMON_PORT = 4763

OPS = {
    0x01: "HELLO", 0x40: "COMPRESSEDPART", 0x46: "SENDINGPART",
    0x47: "REQUESTPARTS", 0x48: "FILEREQANSNOFIL", 0x49: "END_OF_DOWNLOAD",
    0x4C: "HELLOANSWER", 0x4F: "SETREQFILEID", 0x50: "FILESTATUS",
    0x51: "HASHSETREQUEST", 0x52: "HASHSETANSWER", 0x54: "STARTUPLOADREQ",
    0x55: "ACCEPTUPLOADREQ", 0x56: "CANCELTRANSFER", 0x57: "OUTOFPARTREQS",
    0x58: "REQUESTFILENAME", 0x59: "REQFILENAMEANSWER", 0x5C: "QUEUERANK",
    0x60: "QUEUERANKING", 0x62: "EMULEINFO", 0x63: "EMULEINFOANS",
    0xA2: "SENDINGPART_I64", 0xA3: "REQUESTPARTS_I64",
}

# ---- frame accounting: replay aMule's own bookkeeping over the trace ----------
LOCK = threading.Lock()
STATE = {
    "pending": {},        # (start, end_incl) -> {"bytes": n, "opened_at": t}
    "completed": 0,       # blocks whose inclusive end arrived
    "dropped_exceeds": 0, # frames aMule would reject as out of bounds
    "dropped_unknown": 0, # frames that start inside no pending block
    "duplicate_writes": 0,
    "requests": 0,
    "request_pairs": 0,
    "sends": 0,
    "bytes_sent": 0,
    "events": [],         # bounded tail of interesting events
}


def ev(kind: str, **kw):
    if len(STATE["events"]) < 4000:
        STATE["events"].append({"t": time.time(), "kind": kind, **kw})


def on_request(ranges):
    with LOCK:
        STATE["requests"] += 1
        for (s, e) in ranges:
            # aMule requests with an exclusive end and stores an inclusive one.
            incl = e - 1
            STATE["request_pairs"] += 1
            key = (s, incl)
            if key not in STATE["pending"]:
                STATE["pending"][key] = {"bytes": 0, "opened_at": time.time()}
            ev("request", start=s, end=s + (e - s), block=(s, incl))


def on_send(start, end):
    """`end` is the exclusive end as it appears on the wire."""
    with LOCK:
        STATE["sends"] += 1
        STATE["bytes_sent"] += end - start
        incl = end - 1
        hit = None
        for (bs, be) in STATE["pending"]:
            if bs <= start <= be:
                hit = (bs, be)
                break
        if hit is None:
            STATE["dropped_unknown"] += 1
            ev("send_no_pending", start=start, end=end)
            return
        bs, be = hit
        if incl > be:
            # aMule: "Received Blockpacket exceeds requested boundaries" -> RemoveBlockFromList
            STATE["dropped_exceeds"] += 1
            STATE["pending"].pop(hit, None)
            ev("send_exceeds_block", start=start, end=end, block=hit)
            return
        STATE["pending"][hit]["bytes"] += end - start
        if incl == be:
            STATE["pending"].pop(hit, None)
            STATE["completed"] += 1
            ev("block_complete", block=hit)


def u32(raw, i):
    return int.from_bytes(raw[i:i + 4], "little")


def recvn(sock, n):
    out = b""
    while len(out) < n:
        chunk = sock.recv(n - len(out))
        if not chunk:
            return None
        out += chunk
    return out


def pipe(src, dst, direction, log):
    try:
        while True:
            hdr = recvn(src, 6)
            if not hdr:
                break
            length = u32(hdr, 1)
            if length < 1 or length > 16 * 1024 * 1024:
                log("%s BAD_LENGTH %d" % (direction, length))
                break
            payload = recvn(src, length - 1)
            if payload is None:
                break
            op = hdr[5]
            detail = ""
            if op == 0x47 and len(payload) >= 40:
                starts = [u32(payload, 16 + 4 * i) for i in range(3)]
                ends = [u32(payload, 28 + 4 * i) for i in range(3)]
                pairs = [(starts[i], ends[i]) for i in range(3) if ends[i] > starts[i]]
                detail = " ranges=%s" % pairs
                on_request(pairs)
            elif op == 0x47 and len(payload) != 40:
                detail = " NONCLASSIC_REQUESTPARTS payload=%d" % len(payload)
                ev("nonclassic_requestparts", payload_len=len(payload))
            elif op == 0x46 and len(payload) >= 24:
                s, e = u32(payload, 16), u32(payload, 20)
                detail = " range=%d..%d bytes=%d" % (s, e, e - s)
                on_send(s, e)
            elif op == 0x50:
                detail = " payload_len=%d" % len(payload)
            log("%s proto=%#x op=%s(%#02x) len=%d%s" % (
                direction, hdr[0], OPS.get(op, "?"), op, len(payload), detail))
            dst.sendall(hdr + payload)
    except Exception as exc:  # noqa: BLE001
        log("%s END %s: %s" % (direction, type(exc).__name__, exc))
    finally:
        try:
            dst.shutdown(socket.SHUT_WR)
        except Exception:
            pass


def proxy_loop(log, stop):
    srv = socket.socket()
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("0.0.0.0", PROXY_PORT))
    srv.listen(16)
    srv.settimeout(0.5)
    log("proxy listening 0.0.0.0:%d -> 127.0.0.1:%d" % (PROXY_PORT, DAEMON_PORT))
    while not stop.is_set():
        try:
            client, peer = srv.accept()
        except socket.timeout:
            continue
        except OSError:
            break
        log("proxy accepted %s" % (peer,))
        upstream = socket.create_connection(("127.0.0.1", DAEMON_PORT), timeout=8)
        threading.Thread(target=pipe, args=(client, upstream, "C->S", log), daemon=True).start()
        threading.Thread(target=pipe, args=(upstream, client, "S->C", log), daemon=True).start()
    srv.close()


def amule_progress() -> dict:
    """Parse `Show DL` for the active file's percent and state word."""
    out = h.ssh("amulecmd", "-p", "4712", "-P", "coldrevive", "-c", "Show DL").stdout or ""
    m = re.search(r"\[\s*([\d.]+)%\][^\n]*?-\s*([A-Za-z ]+?)\s*-\s*\d+\.part", out)
    if not m:
        return {"percent": None, "state": None, "raw": out[-400:]}
    return {"percent": float(m.group(1)), "state": m.group(2).strip(), "raw": out[-400:]}


def clean_amule_downloads() -> list:
    """Cancel every leftover `w25-*` download before starting a new one.

    aMule keeps **one source entry per IP:port** and assigns it to a single
    file; other files see it as A4AF (`+01`) and never connect. A stalled
    download from a previous run therefore blocks the next run even though the
    new link carries the same source. Cancelling is what releases the source,
    so this is a precondition of the measurement, not a convenience.
    """
    out = h.ssh("amulecmd", "-p", "4712", "-P", "coldrevive", "-c", "Show DL").stdout or ""
    hashes = re.findall(r"^ > ([0-9A-F]{32}) (w25-[^\s]+)$", out, re.MULTILINE)
    cancelled = []
    for fh, nm in hashes:
        r = h.ssh("amulecmd", "-p", "4712", "-P", "coldrevive", "-c", "Cancel " + fh)
        cancelled.append({"hash": fh, "name": nm, "rc": r.returncode,
                          "out": (r.stdout or "")[-160:]})
    if cancelled:
        time.sleep(3)
    return cancelled


def recv_q() -> str:
    # aMule's side of the connection: unread bytes queued in its kernel buffer.
    r = h.ssh("sh", "-c", "ss -tn | grep -E ':4663|:%d ' || true" % DAEMON_PORT)
    return (r.stdout or "").strip()


def main() -> int:
    RUN.mkdir(parents=True, exist_ok=True)
    REPORTS.mkdir(parents=True, exist_ok=True)
    stamp = time.strftime("%Y%m%d%H%M%S")
    name = "w25-diag-%s.bin" % stamp
    sample = RUN / name
    seed = int(stamp) % 251
    with open(sample, "wb") as f:
        f.write(bytes(((i * 91 + seed + 7) % 256) for i in range(SAMPLE_BYTES)))

    trace_path = RUN / ("diag-upload-frames-%s.log" % stamp)
    trace = trace_path.open("w", encoding="utf-8")

    def log(msg: str) -> None:
        trace.write("%s %s\n" % (time.strftime("%H:%M:%S", time.gmtime()), msg))
        trace.flush()

    report: dict = {
        "schema": "coldrevive.fr25.upload-diag/v2",
        "generated_at_unix": int(time.time()),
        "scope": "RFC1918 LAN, real aMule 2.3.3 release build, transparent TCP proxy",
        "sample": {"name": name, "bytes": SAMPLE_BYTES,
                   "md5": hashlib.md5(sample.read_bytes()).hexdigest()},
        "amule_logging": {
            "note": "aMule release builds compile AddDebugLogLineN() out; "
                    "VerboseDebug=1 cannot produce frame-level diagnostics",
            "verified": "strings /usr/bin/amuled | grep -c 'exceeds requested' == 0",
        },
        "timeline": [],
    }
    stop = threading.Event()
    proc = None
    logf = None
    try:
        cfg = {
            "bind": "127.0.0.1",
            "nodes": {"B": {"http_port": h.free_port(),
                            "wake_udp_port": h.free_udp_port(),
                            "bt_port": h.free_port()}},
            "sample_path": str(sample), "keywords": ["w25-diag"],
            "namespace": "ColdRevive_V1:", "workdir": str(RUN),
            "golden_req": str(RUN / "req.json"), "golden_ack": str(RUN / "ack.json"),
            "downloader": "", "challenge_window_sec": 300,
        }
        (RUN / "req.json").write_text("{}", encoding="utf-8")
        (RUN / "ack.json").write_text("{}", encoding="utf-8")
        cfg_path = RUN / ("diag-config-%s.json" % stamp)
        cfg_path.write_text(json.dumps(cfg, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

        daemon = os.environ.get("COLDREVIVE_DAEMON") or str(
            h.ROOT / ("target/release/coldrevived.exe" if os.name == "nt" else "target/release/coldrevived"))
        logf = open(RUN / ("diag-daemon-%s.log" % stamp), "wb")
        proc = subprocess.Popen([daemon, "--config", str(cfg_path), "--node-id", "B",
                                 "--bind", "127.0.0.1", "--workdir", str(RUN)],
                                stdout=logf, stderr=subprocess.STDOUT, cwd=str(RUN))
        proxy = threading.Thread(target=proxy_loop, args=(log, stop), daemon=True)
        proxy.start()

        url = "http://127.0.0.1:%d" % cfg["nodes"]["B"]["http_port"]
        cli_url = url + "/cli"
        if not h.wait_health(url + "/health"):
            report["verdict"] = "BLOCKED"
            report["reason"] = "daemon health check failed"
            return 2

        linked = h.cli(cli_url, "ed2k-link", "--file", str(sample), "--name", name)
        started = h.cli(cli_url, "ed2k-serve", "--file", str(sample), "--name", name,
                        "--port", str(DAEMON_PORT), "--timeout-ms", "300000")
        sid = started.get("id", "")
        link = linked.get("ed2k_link", "")
        report["link"] = linked
        report["serve_id"] = sid
        if not sid or not link:
            report["verdict"] = "BLOCKED"
            report["reason"] = "serve task or link missing"
            return 2

        with_sources = link + "|sources,%s:%d|/" % (OUR_LAN, PROXY_PORT)
        report["amule_cancelled_before"] = clean_amule_downloads()
        add = h.ssh("amulecmd", "-p", "4712", "-P", "coldrevive", "-c", "Add " + with_sources)
        report["amule_add"] = {"rc": add.returncode, "stdout": add.stdout[-400:]}

        t0 = time.time()
        last_pct = -1.0
        last_change = time.time()
        stalled = False
        while time.time() - t0 < MAX_WAIT:
            prog = amule_progress()
            st = h.cli(cli_url, "ed2k-status", "--task", sid)
            res = st.get("result") or {}
            with LOCK:
                snap = {
                    "requests": STATE["requests"], "request_pairs": STATE["request_pairs"],
                    "sends": STATE["sends"], "bytes_sent": STATE["bytes_sent"],
                    "completed": STATE["completed"], "pending": len(STATE["pending"]),
                    "dropped_exceeds": STATE["dropped_exceeds"],
                    "dropped_unknown": STATE["dropped_unknown"],
                }
            report["timeline"].append({
                "t": round(time.time() - t0, 1),
                "amule_percent": prog.get("percent"),
                "amule_state": prog.get("state"),
                "task_status": st.get("status"),
                "serve_bytes_sent": res.get("bytes_sent"),
                **snap,
            })
            pct = prog.get("percent")
            if pct is not None and pct != last_pct:
                last_pct, last_change = pct, time.time()
            if pct is not None and pct <= 0.0 and time.time() - t0 < 20:
                pass  # give aMule time to pick the source up
            elif time.time() - last_change > STALL_SECS:
                stalled = True
                break
            time.sleep(3.0)

        report["stalled"] = stalled
        report["amule_percent_at_stop"] = last_pct
        with LOCK:
            report["accounting"] = {
                "requests": STATE["requests"],
                "request_pairs": STATE["request_pairs"],
                "sends": STATE["sends"],
                "bytes_sent": STATE["bytes_sent"],
                "blocks_completed": STATE["completed"],
                "blocks_pending_at_stop": len(STATE["pending"]),
                "dropped_exceeds_boundaries": STATE["dropped_exceeds"],
                "dropped_no_pending_block": STATE["dropped_unknown"],
                "pending_list": [
                    {"start": s, "end_incl": e, "bytes": v["bytes"]}
                    for (s, e), v in sorted(STATE["pending"].items())
                ],
            }
            report["events_tail"] = [
                {k: v for k, v in e.items() if k != "t"} for e in STATE["events"][-80:]
            ]
        report["recv_q_at_stop"] = recv_q()
        report["amule_show_dl"] = (h.ssh("amulecmd", "-p", "4712", "-P", "coldrevive",
                                         "-c", "Show DL").stdout or "")[-1500:]
        report["serve_final"] = h.cli(cli_url, "ed2k-status", "--task", sid)

        acc = report["accounting"]
        if acc["blocks_pending_at_stop"] and acc["bytes_sent"]:
            verdict = "STALLED_WITH_PENDING_BLOCKS"
        elif acc["bytes_sent"] and not acc["blocks_pending_at_stop"]:
            verdict = "ALL_BLOCKS_COMPLETED_BUT_PEER_STOPPED"
        elif not acc["bytes_sent"]:
            verdict = "NO_DATA_SENT"
        else:
            verdict = "UNCLASSIFIED"
        report["verdict"] = verdict
        return 0
    except Exception as exc:  # noqa: BLE001
        report["verdict"] = "ERROR"
        report["reason"] = "%s: %s" % (type(exc).__name__, exc)
        return 1
    finally:
        stop.set()
        out = REPORTS / ("upload-diag-%s.json" % stamp)
        out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        acc = report.get("accounting") or {}
        print("report: %s" % out)
        print("trace:  %s" % trace_path)
        print("verdict: %s | amule %s%% | sent=%s completed=%s pending=%s "
              "dropped_exceeds=%s dropped_unknown=%s" % (
                  report.get("verdict"), report.get("amule_percent_at_stop"),
                  acc.get("bytes_sent"), acc.get("blocks_completed"),
                  acc.get("blocks_pending_at_stop"), acc.get("dropped_exceeds_boundaries"),
                  acc.get("dropped_no_pending_block")))
        trace.close()
        if proc:
            proc.terminate()
            try:
                proc.wait(timeout=8)
            except subprocess.TimeoutExpired:
                proc.kill()
        if logf:
            logf.close()


if __name__ == "__main__":
    sys.exit(main())
