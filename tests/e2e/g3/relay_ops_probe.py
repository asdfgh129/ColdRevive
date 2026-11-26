#!/usr/bin/env python3
"""G-3 relay ops probe: real relay processes, real stats files, real alerts.

Runs four phases against an isolated loopback coldrevive-relay instance:
  baseline -> breach (tight byte budgets) -> reset (counter reset) -> stale (relay stopped)
The relay_ops_watch process reads --stats-file / --udp-stats-file and appends JSONL
alerts. All raw files are written into --out-dir for byte-exact archiving.

The thresholds are read from the pre-registered criteria file; this script never
invents or overrides a threshold.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
import tomllib

TCP_REJECT_COUNTERS = [
    "bandwidth_limited", "ip_cap_rejected", "conn_cap_rejected", "rate_limited"
]
UDP_REJECT_COUNTERS = [
    "bandwidth_limited", "rate_limited", "per_session_limited", "per_session_dropped"
]


def now_iso() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat(timespec="milliseconds")


def sha256(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def jload(path: str):
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


def jdump(path: str, obj) -> None:
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        json.dump(obj, f, ensure_ascii=False, indent=2)
        f.write("\n")


def parse_json_line(text: str):
    for line in reversed(text.splitlines()):
        line = line.strip()
        if line.startswith("{"):
            try:
                return json.loads(line)
            except ValueError:
                continue
    return None


def stop_proc(proc) -> None:
    if proc is None or proc.poll() is not None:
        return
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=5)


class Probe:
    def __init__(self, args, criteria, criteria_sha):
        self.args = args
        self.criteria = criteria
        self.criteria_sha = criteria_sha
        self.out = os.path.abspath(args.out_dir)
        os.makedirs(self.out, exist_ok=True)
        self.tcp_stats = os.path.join(self.out, "tcp-stats.json")
        self.udp_stats = os.path.join(self.out, "udp-stats.json")
        self.alert_file = os.path.join(self.out, "alerts.jsonl")
        self.summary_file = os.path.join(self.out, "watch-summary.json")
        self.stop_file = os.path.join(self.out, "watch.stop")
        for p in (self.alert_file, self.summary_file, self.stop_file):
            if os.path.exists(p):
                os.unlink(p)
        self.proc = None
        self.watcher = None
        self.proc_args: list[str] = []
        self.boundaries: dict[str, str] = {}
        self.notes: list[str] = []

    def start_relay(self, mode: str, fresh: bool = False):
        cfg = self.criteria["baseline_relay"] if mode == "baseline" else self.criteria["breach_relay"]
        if fresh:
            for p in (self.tcp_stats, self.udp_stats):
                if os.path.exists(p):
                    os.unlink(p)
        cmd = [
            self.args.relay_bin, "--listen", "127.0.0.1:0",
            "--stats-file", self.tcp_stats,
            "--udp-listen", "127.0.0.1:0",
            "--udp-stats-file", self.udp_stats,
            "--data-rate", str(cfg["data_rate"]),
            "--data-burst", str(cfg["data_burst"]),
            "--udp-data-rate", str(cfg["udp_data_rate"]),
            "--udp-data-burst", str(cfg["udp_data_burst"]),
            "--max-conns-per-ip", str(cfg["max_conns_per_ip"]),
        ]
        if cfg.get("udp_per_session_rate"):
            cmd += [
                "--udp-per-session-rate", str(cfg["udp_per_session_rate"]),
                "--udp-per-session-burst", str(cfg["udp_per_session_burst"]),
            ]
        with open(os.path.join(self.out, f"{mode}-relay.stdout.log"), "w", encoding="utf-8") as log:
            self.proc = subprocess.Popen(cmd, stdout=log, stderr=subprocess.STDOUT)
        self.proc_args = cmd
        return cmd

    def wait_ready(self, prev_listen):
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            try:
                tcp = jload(self.tcp_stats)
                udp = jload(self.udp_stats)
                listen = tcp.get("listen")
                ulisten = udp.get("udp_listen")
                if listen and ulisten and listen != prev_listen:
                    return listen, ulisten, tcp, udp
            except (OSError, ValueError):
                pass
            time.sleep(0.1)
        raise RuntimeError("relay stats files did not become ready within 15 s")

    def start_watcher(self):
        cmd = [
            sys.executable, self.args.watch_script,
            "--tcp-stats", self.tcp_stats,
            "--udp-stats", self.udp_stats,
            "--alert-file", self.alert_file,
            "--summary-file", self.summary_file,
            "--criteria", self.args.criteria,
            "--stop-file", self.stop_file,
        ]
        with open(os.path.join(self.out, "watch.stdout.log"), "w", encoding="utf-8") as log:
            self.watcher = subprocess.Popen(cmd, stdout=log, stderr=subprocess.STDOUT)
        return cmd

    def bench(self, relay: str, udp: bool, rounds: int, payload: int, timeout_ms: int, tag: str):
        cmd = [
            self.args.bench_bin, "--relay", relay,
            "--rounds", str(rounds), "--payload", str(payload),
            "--timeout-ms", str(timeout_ms), "--json",
        ]
        if udp:
            cmd.append("--udp")
        try:
            r = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
            parsed = parse_json_line(r.stdout)
            return {
                "tag": tag, "args": cmd, "returncode": r.returncode,
                "json": parsed, "stdout_tail": r.stdout[-1500:], "stderr_tail": r.stderr[-800:],
            }
        except subprocess.TimeoutExpired as e:
            return {"tag": tag, "args": cmd, "timeout": True, "stdout_tail": str(e.stdout)[-1500:]}

    def snapshot(self, tag: str):
        tcp = jload(self.tcp_stats)
        udp = jload(self.udp_stats)
        meta = {}
        for name, path in (("tcp", self.tcp_stats), ("udp", self.udp_stats)):
            meta[name + "_mtime_age_sec"] = round(time.time() - os.path.getmtime(path), 3)
        shutil.copyfile(self.tcp_stats, os.path.join(self.out, f"{tag}-tcp-stats-final.json"))
        shutil.copyfile(self.udp_stats, os.path.join(self.out, f"{tag}-udp-stats-final.json"))
        return {"tcp": tcp, "udp": udp, **meta}

    def alerts(self):
        if not os.path.exists(self.alert_file):
            return []
        with open(self.alert_file, "r", encoding="utf-8") as f:
            return [json.loads(line) for line in f if line.strip()]

    def run(self):
        acc = self.criteria["acceptance"]
        rounds = acc["bench_rounds"]
        min_echo = acc["min_bench_echoes"]
        phases = {}
        self.boundaries["watch_start"] = now_iso()

        # --- baseline -------------------------------------------------------
        base_cmd = self.start_relay("baseline", fresh=True)
        base_listen, base_udp_listen, _, _ = self.wait_ready(None)
        watch_cmd = self.start_watcher()
        time.sleep(0.5)
        b_tcp = self.bench(base_listen, False, rounds, 64, 1000, "baseline-tcp")
        b_udp = self.bench(base_udp_listen, True, rounds, 64, 1000, "baseline-udp")
        time.sleep(3)
        base_snap = self.snapshot("baseline")
        self.boundaries["baseline_end"] = now_iso()
        phases["baseline"] = {
            "relay_args": base_cmd, "tcp_listen": base_listen, "udp_listen": base_udp_listen,
            "bench_tcp": b_tcp, "bench_udp": b_udp, "stats": base_snap,
        }

        # --- breach ---------------------------------------------------------
        stop_proc(self.proc)
        breach_cmd = self.start_relay("breach")
        breach_listen, breach_udp_listen, _, _ = self.wait_ready(base_listen)
        self.boundaries["breach_start"] = now_iso()
        x_tcp = self.bench(breach_listen, False, 10, 60000, 200, "breach-tcp")
        x_udp = self.bench(breach_udp_listen, True, 10, 5000, 200, "breach-udp")
        triggered = []
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            tcp = jload(self.tcp_stats)
            udp = jload(self.udp_stats)
            triggered = (
                [(file, c, base_snap["tcp"].get(c, 0), tcp.get(c, 0))
                 for file, counters, snap, stats in (
                     ("tcp", TCP_REJECT_COUNTERS, base_snap["tcp"], tcp),
                     ("udp", UDP_REJECT_COUNTERS, base_snap["udp"], udp))
                 for c in counters if stats.get(c, 0) > snap.get(c, 0)]
            )
            if triggered:
                break
            time.sleep(0.1)
        self.boundaries["breach_trigger_observed"] = now_iso()
        time.sleep(3)
        breach_snap = self.snapshot("breach")
        self.boundaries["breach_end"] = now_iso()
        phases["breach"] = {
            "relay_args": breach_cmd, "tcp_listen": breach_listen, "udp_listen": breach_udp_listen,
            "bench_tcp": x_tcp, "bench_udp": x_udp, "stats": breach_snap,
            "counters_increased": triggered,
        }

        # --- reset (relay restarted, counters back to zero) -----------------
        stop_proc(self.proc)
        reset_cmd = self.start_relay("breach")
        _, _, _, _ = self.wait_ready(breach_listen)
        self.boundaries["reset_start"] = now_iso()
        time.sleep(3)
        reset_snap = self.snapshot("reset")
        self.boundaries["reset_end"] = now_iso()
        phases["reset"] = {"relay_args": reset_cmd, "stats": reset_snap}

        # --- stale (relay stopped, stats files stop being refreshed) --------
        stop_proc(self.proc)
        self.proc = None
        self.boundaries["stale_start"] = now_iso()
        time.sleep(4)
        self.boundaries["stale_end"] = now_iso()
        phases["stale"] = {"relay_stopped_at": self.boundaries["stale_start"]}

        # --- collect --------------------------------------------------------
        with open(self.stop_file, "w", encoding="utf-8") as f:
            f.write("stop\n")
        if self.watcher is not None:
            try:
                self.watcher.wait(timeout=15)
            except subprocess.TimeoutExpired:
                stop_proc(self.watcher)
                self.notes.append("watcher did not exit within 15 s after stop-file; terminated")
        all_alerts = self.alerts()
        watch_summary = jload(self.summary_file) if os.path.exists(self.summary_file) else None

        def alerts_between(start_key, end_key):
            lo = self.boundaries[start_key]
            hi = self.boundaries[end_key]
            return [a for a in all_alerts if a["state"] == "alert" and lo <= a["recorded_at"] <= hi]

        baseline_alerts = alerts_between("watch_start", "baseline_end")
        breach_alerts = alerts_between("breach_start", "breach_end")
        reset_alerts = [a for a in alerts_between("reset_start", "reset_end")
                        if "stale" not in a["condition"]]
        stale_alerts = [a for a in all_alerts
                        if a["state"] == "alert" and a["condition"].endswith("stats_stale")
                        and a["recorded_at"] >= self.boundaries["stale_start"]]

        def echoes(bench):
            j = bench.get("json") or {}
            return int(j.get("echoed", 0) or 0)

        required_fields = acc["required_udp_stats_fields"]
        observability_fields_ok = all(f in base_snap["udp"] for f in required_fields)
        observability_traffic_ok = (
            echoes(b_tcp) >= min_echo and echoes(b_udp) >= min_echo
        )
        observability_fresh_ok = (
            base_snap["tcp_mtime_age_sec"] <= self.criteria["window"]["max_stale_secs"]
            and base_snap["udp_mtime_age_sec"] <= self.criteria["window"]["max_stale_secs"]
        )
        baseline_rejects_zero = all(
            base_snap["tcp"].get(c, 0) == 0 for c in TCP_REJECT_COUNTERS
        ) and all(base_snap["udp"].get(c, 0) == 0 for c in UDP_REJECT_COUNTERS)

        # Measure from the moment the tight-budget relay started: that is the
        # earliest instant a breach could exist, so this is a conservative bound.
        # Polling the counter after bench returns would be too late (the watcher
        # can legitimately fire while the bench is still running).
        latency_sec = None
        if breach_alerts:
            t = dt.datetime.fromisoformat(self.boundaries["breach_start"])
            first = min(dt.datetime.fromisoformat(a["recorded_at"]) for a in breach_alerts)
            latency_sec = round((first - t).total_seconds(), 3)

        verdict = {
            "rate_limit": "PASS" if baseline_rejects_zero and bool(triggered) else "FAIL",
            "observability": "PASS" if (observability_fields_ok and observability_traffic_ok
                                        and observability_fresh_ok) else "FAIL",
            "alerting": "PASS" if (
                not baseline_alerts
                and bool(breach_alerts)
                and bool(stale_alerts)
                and not reset_alerts
                and latency_sec is not None
                and latency_sec <= acc["alert_max_latency_secs"]
            ) else "FAIL",
        }
        evidence = {
            "schema": "coldrevive.g3.relay-ops-evidence/v1",
            "recorded_at": now_iso(),
            "criteria": {
                "path": self.args.criteria,
                "sha256": self.criteria_sha,
                "registered_at": self.criteria["registered_at"],
            },
            "binaries": {
                "relay": {"path": self.args.relay_bin, "sha256": sha256(self.args.relay_bin)},
                "bench": {"path": self.args.bench_bin, "sha256": sha256(self.args.bench_bin)},
                "watch": {"path": self.args.watch_script, "sha256": sha256(self.args.watch_script)},
                "python": sys.version.split()[0],
            },
            "git_commit": subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True,
                                         text=True, cwd=self.args.repo).stdout.strip(),
            "watch_args": watch_cmd,
            "boundaries": self.boundaries,
            "phases": phases,
            "alerts": {
                "all": all_alerts,
                "baseline": baseline_alerts,
                "breach": breach_alerts,
                "reset_rule_alerts": reset_alerts,
                "stale": stale_alerts,
                "breach_latency_sec": latency_sec,
                "watch_summary": watch_summary,
            },
            "checks": {
                "baseline_reject_counters_all_zero": baseline_rejects_zero,
                "breach_counters_increased": triggered,
                "required_udp_fields_present": observability_fields_ok,
                "required_udp_fields": required_fields,
                "bench_echoes_tcp": echoes(b_tcp),
                "bench_echoes_udp": echoes(b_udp),
                "min_bench_echoes": min_echo,
                "baseline_stats_mtime_age_sec": [base_snap["tcp_mtime_age_sec"],
                                                 base_snap["udp_mtime_age_sec"]],
                "counter_resets_seen": (watch_summary or {}).get("counter_resets_seen"),
            },
            "clauses": verdict,
            "verdict": "PASS" if all(v == "PASS" for v in verdict.values()) else "FAIL",
            "notes": self.notes + [
                "Isolated loopback relay only; production relay 119.29.150.254:7101 untouched.",
                "bench --udp uses two sockets on the same host; not evidence of two distinct NATs.",
                "Thresholds come from the pre-registered criteria file; no runtime override.",
            ],
        }
        jdump(os.path.join(self.out, "summary.json"), evidence)
        print(json.dumps({"verdict": evidence["verdict"], "clauses": verdict,
                          "alerts": len(all_alerts), "breach_latency_sec": latency_sec},
                         ensure_ascii=False), flush=True)
        return 0 if evidence["verdict"] == "PASS" else 1

    def cleanup(self):
        with open(self.stop_file, "a", encoding="utf-8") as f:
            f.write("cleanup\n")
        stop_proc(self.watcher)
        stop_proc(self.proc)


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--relay-bin", required=True)
    p.add_argument("--bench-bin", required=True)
    p.add_argument("--watch-script", default="scripts/relay_ops_watch.py")
    p.add_argument("--criteria", default="tests/fixtures/g3-relay-ops-thresholds.toml")
    p.add_argument("--out-dir", required=True)
    p.add_argument("--repo", default=".")
    args = p.parse_args()
    with open(args.criteria, "rb") as f:
        criteria = tomllib.load(f)
    probe = Probe(args, criteria, sha256(args.criteria))
    try:
        return probe.run()
    finally:
        probe.cleanup()


if __name__ == "__main__":
    sys.exit(main())
