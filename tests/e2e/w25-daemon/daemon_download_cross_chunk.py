#!/usr/bin/env python3
"""FR-25 >PARTSIZE cross-chunk DOWNLOAD from real aMule into coldrevived.

The upload direction is traced separately in reports/daemon-cross-chunk-*.json.
This harness only asserts the download direction: real aMule 2.3.3 shares a
> PARTSIZE file, the daemon fetches it over production cr_ed2k::stack, and the
result must show hashset exchange plus byte-for-byte content match.
"""
from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("h", HERE / "daemon_cross_chunk.py")
h = importlib.util.module_from_spec(spec)
spec.loader.exec_module(h)

INCOMING = h.AMULE_INCOMING


def scp_sample(local: Path, remote_name: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["scp", "-i", h.KEY, "-o", "IdentitiesOnly=yes", "-o", "ConnectTimeout=8",
         str(local), "root@%s:%s/%s" % (h.AMULE_HOST, INCOMING, remote_name)],
        capture_output=True, text=True, timeout=120,
    )


def main() -> int:
    h.RUN.mkdir(parents=True, exist_ok=True)
    h.REPORTS.mkdir(parents=True, exist_ok=True)
    stamp = time.strftime("%Y%m%d%H%M%S")
    name = "w25-down-%s.bin" % stamp
    sample = h.RUN / name
    with open(sample, "wb") as f:
        seed = int(stamp) % 251
        f.write(bytes(((i * 91 + seed + 7) % 256) for i in range(h.SAMPLE_BYTES)))
    local_md5 = h.hashlib.md5(sample.read_bytes()).hexdigest()

    report = {
        "schema": "coldrevive.fr25.daemon-download-cross-chunk/v1",
        "generated_at_unix": int(time.time()),
        "scope": "RFC1918 LAN, real aMule 2.3.3 uploader, daemon /cli + /jsonrpc",
        "sample": {"name": name, "bytes": h.SAMPLE_BYTES, "sha256": h.sha256(sample),
                   "md5": local_md5, "part_size": h.PART_SIZE},
        "checks": [],
    }
    procs = []
    logf = None
    try:
        copy = scp_sample(sample, name)
        report["scp"] = {"rc": copy.returncode, "stderr": copy.stderr[-400:]}
        report["checks"].append({"name": "sample_shared_by_real_amule", "ok": copy.returncode == 0})
        if copy.returncode != 0:
            report.update(status="BLOCKED", reason="scp 到 aMule Incoming 失败")
            return 2
        reload = h.ssh("amulecmd", "-p", "4712", "-P", "coldrevive", "-c", "Reload Shared")
        time.sleep(2)
        report["reload_shared"] = {"rc": reload.returncode, "stdout": reload.stdout[-300:]}

        daemon = os.environ.get("COLDREVIVE_DAEMON") or str(
            h.ROOT / ("target/release/coldrevived.exe" if os.name == "nt" else "target/release/coldrevived"))
        cfg = {
            "bind": "127.0.0.1",
            "nodes": {"B": {"http_port": h.free_port(), "wake_udp_port": h.free_udp_port(), "bt_port": h.free_port()}},
            "sample_path": str(sample), "keywords": ["w25-daemon"], "namespace": "ColdRevive_V1:",
            "workdir": str(h.RUN), "golden_req": str(h.RUN / "req.json"),
            "golden_ack": str(h.RUN / "ack.json"), "downloader": "", "challenge_window_sec": 300,
        }
        (h.RUN / "req.json").write_text("{}", encoding="utf-8")
        (h.RUN / "ack.json").write_text("{}", encoding="utf-8")
        cfg_path = h.RUN / ("config-down-%s.json" % stamp)
        cfg_path.write_text(json.dumps(cfg, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        logf = (h.RUN / ("daemon-down-%s.log" % stamp)).open("wb")
        procs.append(subprocess.Popen(
            [daemon, "--config", str(cfg_path), "--node-id", "B",
             "--bind", "127.0.0.1", "--workdir", str(h.RUN)],
            stdout=logf, stderr=subprocess.STDOUT, cwd=str(h.RUN)))
        url = "http://127.0.0.1:%d" % cfg["nodes"]["B"]["http_port"]
        cli_url, rpc_url = url + "/cli", url + "/jsonrpc"
        if not h.wait_health(url + "/health"):
            report.update(status="BLOCKED", reason="daemon 健康检查未通过")
            return 2
        report["checks"].append({"name": "daemon_health", "ok": True})

        started = h.cli(cli_url, "ed2k-download", "--file", str(sample), "--name", name,
                        "--host", h.AMULE_ED2K, "--timeout-ms", "300000")
        did = started.get("id", "")
        report["download_started"] = started
        report["checks"].append({"name": "download_task_registered", "ok": bool(did)})
        if not did:
            report.update(status="FAIL", reason="ed2k-download 未登记任务: %r" % started)
            return 1
        result = h.poll_task(cli_url, did, 330.0)
        report["download_result"] = result
        dr = (result or {}).get("result") or {}
        report["checks"].extend([
            {"name": "hashset_verified", "ok": dr.get("hashset_verified") is True},
            {"name": "download_completed", "ok": dr.get("completed") is True},
            {"name": "hash_matches", "ok": dr.get("hash_matches") is True},
            {"name": "content_matches", "ok": dr.get("content_matches") is True},
            {"name": "bytes_received_covers_sample", "ok": dr.get("bytes_received", 0) >= h.SAMPLE_BYTES},
        ])
        mon = h.rpc(rpc_url, "coldrevive.ed2kStatus", [did]).get("result") or {}
        report["checks"].append({
            "name": "jsonrpc_exposes_same_task",
            "ok": mon.get("id") == did and mon.get("status") == (result or {}).get("status"),
        })
        remote = h.ssh("stat", "-c", "%s", "%s/%s" % (INCOMING, name))
        report["remote_size_ok"] = remote.returncode == 0 and remote.stdout.strip() == str(h.SAMPLE_BYTES)
        report["checks"].append({"name": "real_amule_file_size", "ok": report["remote_size_ok"]})
        report["status"] = "PASS" if all(c["ok"] for c in report["checks"]) else "FAIL"
        return 0 if report["status"] == "PASS" else 1
    except Exception as exc:
        report.update(status="FAIL", reason="%s: %s" % (type(exc).__name__, exc))
        return 1
    finally:
        out = h.REPORTS / ("daemon-download-cross-chunk-%s.json" % stamp)
        out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print("report: %s" % out)
        print("status: %s (%d/%d checks)" % (
            report.get("status"), sum(1 for c in report["checks"] if c["ok"]), len(report["checks"])))
        for p in procs:
            p.terminate()
            try:
                p.wait(timeout=8)
            except subprocess.TimeoutExpired:
                p.kill()
        if logf:
            logf.close()


if __name__ == "__main__":
    sys.exit(main())
