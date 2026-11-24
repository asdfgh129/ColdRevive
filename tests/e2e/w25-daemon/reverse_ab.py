#!/usr/bin/env python3
"""FR-25 reverse A/B: our daemon uploader vs a second stock aMule uploader.

Same >PARTSIZE sample, same stock aMule downloader (221, EC 4712), two uploaders:

  A  daemon 192.168.1.210:4663 -> aMule 221      (run diag_upload.py, paced)
  B  second stock amuled 221:4664 -> same aMule 221 (control)

B uses a separate amuled instance with KAD/server connect disabled. It is
started only for this experiment and stopped in `finally`. The only verdict
is the real landed md5 on the downloader; no gate constant is changed.

Usage:
  python3 tests/e2e/w25-daemon/reverse_ab.py
Env:
  FR25_AB_MAX_WAIT_SECS   B arm watch window (default 180)
  FR25_PACE_BPS           A arm pacing (default 200000)
"""
from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import subprocess
import sys
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

SECOND_DIR = "/opt/g3-clients/amule-config-fr25-second"
SECOND_TCP = 4664
SECOND_EC = 4713
PRIMARY_INCOMING = "/opt/g3-clients/amule-config/Incoming"
MAX_WAIT = int(os.environ.get("FR25_AB_MAX_WAIT_SECS", "180"))
PACE = int(os.environ.get("FR25_PACE_BPS", "200000"))
PART_SIZE = 9_500 * 1024
SAMPLE_BYTES = PART_SIZE + 8192
PASSWORD = "coldrevive"  # aMule EC password used by the existing test harness only


def stamp() -> str:
    return time.strftime("%Y%m%d%H%M%S")


def md5(data: bytes) -> str:
    return hashlib.md5(data).hexdigest()


def remote_md5(path: str) -> str:
    r = h.ssh("md5sum", path)
    out = (r.stdout or "").strip().split()
    return out[0] if out and r.returncode == 0 else ""


def setup_second(sample: Path, name: str) -> dict:
    script = (
        f"set -e; D={SECOND_DIR}; mkdir -p \"$D/Incoming\" \"$D/Temp\"; "
        "cp -a /opt/g3-clients/amule-config/amule.conf \"$D/amule.conf\"; "
        "sed -i '0,/^Port=.*/s//Port=4664/' \"$D/amule.conf\"; "
        "sed -i -e 's/^UDPPort=.*/UDPPort=4674/' -e 's/^UDPEnable=.*/UDPEnable=0/' "
        f"-e 's/^ConnectToKad=.*/ConnectToKad=0/' -e 's/^ConnectToED2K=.*/ConnectToED2K=0/' "
        f"-e 's/^Autoconnect=.*/Autoconnect=0/' -e 's/^ECPort=.*/ECPort={SECOND_EC}/' "
        f"-e 's|^TempDir=.*|TempDir=$D/Temp|' -e 's|^IncomingDir=.*|IncomingDir=$D/Incoming|' "
        f"-e 's|^OSDirectory=.*|OSDirectory=$D|' \"$D/amule.conf\"; "
        "pkill -f '[a]muled -c /opt/g3-clients/amule-config-fr25-second' 2>/dev/null || true; "
        f"cp -f \"$D/Incoming/{name}\" \"$D/Incoming/{name}.old\" 2>/dev/null || true"
    )
    r = h.ssh("sh", "-c", script, timeout=60)
    if r.returncode != 0:
        raise RuntimeError(f"second amuled setup failed: {(r.stderr or '')[-300:]}")
    scp = subprocess.run(
        ["scp", "-i", h.KEY, "-o", "IdentitiesOnly=yes", "-o", "BatchMode=yes",
         str(sample), f"root@{h.AMULE_HOST}:{SECOND_DIR}/Incoming/{name}"],
        capture_output=True, text=True, timeout=60,
    )
    if scp.returncode != 0:
        raise RuntimeError(f"sample copy failed: {(scp.stderr or '')[-300:]}")
    return {"config_dir": SECOND_DIR, "tcp_port": SECOND_TCP, "ec_port": SECOND_EC,
            "incoming": f"{SECOND_DIR}/Incoming/{name}"}


def start_second(log_path: Path) -> subprocess.Popen:
    log = log_path.open("wb")
    proc = subprocess.Popen(
        ["ssh", "-i", h.KEY, "-o", "IdentitiesOnly=yes", "-o", "BatchMode=yes",
         f"root@{h.AMULE_HOST}", "amuled", "-c", SECOND_DIR, "-o"],
        stdout=log, stderr=subprocess.STDOUT,
    )
    proc._cr_log = log  # type: ignore[attr-defined]
    for _ in range(60):
        ready = h.ssh("sh", "-c", f"ss -ltn | grep -q ':{SECOND_EC} ' && echo ready || true")
        if "ready" in (ready.stdout or ""):
            return proc
        time.sleep(1)
    raise RuntimeError("second amuled EC port never became ready; see " + str(log_path))


def stop_second(proc: subprocess.Popen | None) -> str:
    h.ssh("sh", "-c",
          "pkill -f '[a]muled -c /opt/g3-clients/amule-config-fr25-second' 2>/dev/null || true")
    if proc is not None:
        try:
            proc.terminate()
            proc.wait(timeout=5)
        except Exception:  # noqa: BLE001
            proc.kill()
        try:
            proc._cr_log.close()  # type: ignore[attr-defined]
        except Exception:  # noqa: BLE001
            pass
    return "stopped"


def arm_b(link: str, name: str, expected_md5: str) -> dict:
    cancelled = du.clean_amule_downloads()
    with_sources = link + f"|sources,{h.AMULE_HOST}:{SECOND_TCP}|/"
    add = h.ssh("amulecmd", "-p", "4712", "-P", PASSWORD, "-c", "Add " + with_sources)
    timeline: list[dict] = []
    t0 = time.time()
    landed = ""
    while time.time() - t0 < MAX_WAIT:
        prog = du.amule_progress()
        timeline.append({"t": round(time.time() - t0, 1),
                         "percent": prog.get("percent"), "state": prog.get("state")})
        landed = remote_md5(f"{PRIMARY_INCOMING}/{name}")
        if landed == expected_md5:
            break
        time.sleep(2)
    ul = h.ssh("amulecmd", "-p", str(SECOND_EC), "-P", PASSWORD, "-c", "Show UL")
    conns = h.ssh("sh", "-c", f"ss -tn | grep ':{SECOND_TCP} ' || true")
    return {
        "uploader": f"{h.AMULE_HOST}:{SECOND_TCP} (second stock amuled)",
        "downloader": f"{h.AMULE_HOST}:4712 (same primary amulecmd)",
        "link": with_sources,
        "cancelled_before": cancelled,
        "add_rc": add.returncode,
        "add_stdout": (add.stdout or "")[-300:],
        "timeline": timeline,
        "elapsed_sec": round(time.time() - t0, 1),
        "landed_md5": landed,
        "md5_matches": landed == expected_md5,
        "second_show_ul": (ul.stdout or "")[-500:],
        "second_connections": (conns.stdout or "").strip()[-500:],
    }


def main() -> int:
    RUN.mkdir(parents=True, exist_ok=True)
    REPORTS.mkdir(parents=True, exist_ok=True)
    sid = stamp()
    name = f"w25-reverse-{sid}.bin"
    sample = RUN / name
    seed = int(sid) % 251
    sample.write_bytes(bytes(((i * 91 + seed + 7) % 256) for i in range(SAMPLE_BYTES)))
    expect = md5(sample.read_bytes())

    report: dict = {
        "schema": "coldrevive.fr25.reverse-ab/v1",
        "generated_at_unix": int(time.time()),
        "scope": "RFC1918 LAN; same sample and same downloader, two uploaders",
        "gate_constants_untouched": True,
        "sample": {"name": name, "bytes": SAMPLE_BYTES, "md5": expect},
        "criteria": {
            "A": "our daemon uploader stalls/lands no md5 under the same paced run",
            "B": "stock aMule uploader lands the exact same file on the same downloader",
            "conclusion": "only a real landed md5 is evidence; no PASS/FAIL promotion",
        },
    }
    proc = None
    try:
        report["second_uploader_setup"] = setup_second(sample, name)
        proc = start_second(RUN / f"reverse-ab-second-{sid}.log")
        report["second_uploader_ready"] = True

        env = os.environ.copy()
        env.update({"FR25_SAMPLE_PATH": str(sample), "FR25_SAMPLE_NAME": name,
                    "FR25_PACE_BPS": str(PACE), "FR25_MAX_WAIT_SECS": "180",
                    "FR25_STALL_SECS": "25"})
        before = {p for p in REPORTS.glob("upload-diag-*.json")}
        a_stdout = RUN / f"reverse-ab-A-{sid}.stdout.log"
        with a_stdout.open("w", encoding="utf-8") as out:
            a = subprocess.run([sys.executable, str(HERE / "diag_upload.py")],
                               env=env, stdout=out, stderr=subprocess.STDOUT,
                               cwd=str(HERE.parent.parent.parent), timeout=300)
        fresh = [p for p in REPORTS.glob("upload-diag-*.json") if p not in before]
        a_report_path = max(fresh, key=lambda p: p.stat().st_mtime) if fresh else None
        a_data = json.loads(a_report_path.read_text(encoding="utf-8")) if a_report_path else {}
        report["arm_a"] = {
            "uploader": "coldrevived serve via transparent proxy 192.168.1.210:4663",
            "exit_code": a.returncode,
            "stdout_log": str(a_stdout),
            "report": str(a_report_path) if a_report_path else "",
            "pace_bps": a_data.get("pace_bps"),
            "amule_percent": (a_data.get("timeline") or [{}])[-1].get("amule_percent")
                             if a_data.get("timeline") else None,
            "bytes_sent": (a_data.get("timeline") or [{}])[-1].get("serve_bytes_sent")
                          if a_data.get("timeline") else None,
            "reason": a_data.get("reason", ""),
            "verdict": a_data.get("verdict", ""),
            "landed_md5": a_data.get("landed_md5", ""),
            "md5_matches": a_data.get("md5_matches", False),
        }
        link = (a_data.get("link") or {}).get("ed2k_link", "")
        if not link:
            report["arm_b"] = {"error": "A arm produced no ed2k link; B arm not run"}
            return 2
        report["arm_b"] = arm_b(link, name, expect)
    finally:
        report["second_uploader_cleanup"] = stop_second(proc)

    out = REPORTS / f"reverse-ab-{sid}.json"
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"report: {out}")
    print(f"A: exit={report['arm_a'].get('exit_code')} percent={report['arm_a'].get('amule_percent')}")
    b = report.get("arm_b", {})
    print(f"B: elapsed={b.get('elapsed_sec')} md5_matches={b.get('md5_matches')}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
