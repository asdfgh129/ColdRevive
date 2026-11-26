#!/usr/bin/env python3
"""Relay ops watcher (G-3): turn real relay stats files into auditable alerts.

Reads the two JSON files written every 200 ms by coldrevive-relay
(--stats-file / --udp-stats-file), evaluates the PRE-REGISTERED thresholds in
tests/fixtures/g3-relay-ops-thresholds.toml, and appends JSON Lines alerts.

Design rules:
  * thresholds come from the criteria file only; no silent built-in fallback;
  * counters are cumulative: compare deltas per window, never raw values;
  * a decreasing counter means the relay restarted (counter_reset) -> no alert;
  * stale/missing stats file is itself an alert condition (stats_stale);
  * every alert carries criteria sha, condition, counter, window delta and time;
  * never writes to relay state: this process is read-only on the stats files.

Usage:
  python3 scripts/relay_ops_watch.py \
      --tcp-stats <path> --udp-stats <path> \
      --alert-file <path> --summary-file <path> \
      --criteria tests/fixtures/g3-relay-ops-thresholds.toml \
      --duration-secs 30
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import sys
import time
import tomllib

ALERT_SCHEMA = "coldrevive.g3.relay-alert/v1"
SUMMARY_SCHEMA = "coldrevive.g3.relay-watch-summary/v1"

# condition -> (stats file key, counter field)
CONDITIONS: dict[str, tuple[str, str]] = {
    "tcp_bandwidth_limited": ("tcp", "bandwidth_limited"),
    "tcp_ip_cap_rejected": ("tcp", "ip_cap_rejected"),
    "tcp_conn_cap_rejected": ("tcp", "conn_cap_rejected"),
    "tcp_rate_limited": ("tcp", "rate_limited"),
    "udp_bandwidth_limited": ("udp", "bandwidth_limited"),
    "udp_rate_limited": ("udp", "rate_limited"),
    "udp_per_session_limited": ("udp", "per_session_limited"),
    "udp_per_session_dropped": ("udp", "per_session_dropped"),
}


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat(timespec="milliseconds")


def sha256_file(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def load_criteria(path: str):
    with open(path, "rb") as f:
        data = tomllib.load(f)
    return data, sha256_file(path)


def read_stats(path: str):
    """Return (stats_dict, mtime) or (None, None) when unreadable."""
    try:
        mtime = os.path.getmtime(path)
        with open(path, "r", encoding="utf-8") as f:
            return json.load(f), mtime
    except (OSError, ValueError):
        return None, None


class Watcher:
    def __init__(self, args, criteria, criteria_sha):
        self.args = args
        self.criteria = criteria
        self.criteria_sha = criteria_sha
        self.window = criteria["window"]
        self.rules = criteria["alert_rules"]
        self.paths = {"tcp": args.tcp_stats, "udp": args.udp_stats}
        self.prev: dict[str, dict[str, int]] = {"tcp": {}, "udp": {}}
        self.window_deltas: dict[str, int] = {}
        self.active: set[tuple[str, str]] = set()
        self.resets = 0
        self.samples = 0
        self.emitted: list[dict] = []
        self.window_started = time.monotonic()

    def emit(self, state: str, condition: str, counter, stats_path, delta, threshold, observed):
        rec = {
            "schema": ALERT_SCHEMA,
            "recorded_at": utc_now(),
            "state": state,
            "condition": condition,
            "counter": counter,
            "stats_path": stats_path,
            "delta_window": delta,
            "threshold": threshold,
            "window_secs": self.window["alert_window_secs"],
            "criteria_sha256": self.criteria_sha,
            "observed": observed,
        }
        line = json.dumps(rec, ensure_ascii=False, separators=(",", ":"))
        with open(self.args.alert_file, "a", encoding="utf-8", newline="\n") as f:
            f.write(line + "\n")
        if not self.args.quiet:
            print(line, flush=True)
        self.emitted.append(rec)

    def sample(self):
        self.samples += 1
        now = time.time()
        for key, path in self.paths.items():
            stats, mtime = read_stats(path)
            stale = mtime is None or (now - mtime) > self.window["max_stale_secs"]
            stale_key = (f"{key}_stats_stale", path)
            if stale:
                if stale_key not in self.active:
                    self.emit("alert", stale_key[0], None, path, None,
                              self.window["max_stale_secs"], {"stats_mtime": mtime, "now": now})
                    self.active.add(stale_key)
                continue
            if stale_key in self.active:
                self.emit("resolved", stale_key[0], None, path, 0,
                          self.window["max_stale_secs"], {"stats_mtime": mtime})
                self.active.discard(stale_key)
            for cond, (file_key, counter) in CONDITIONS.items():
                if file_key != key or cond not in self.rules or counter not in stats:
                    continue
                value = int(stats[counter])
                previous = self.prev[key].get(counter)
                if previous is None:
                    self.prev[key][counter] = value
                    continue
                if value < previous:  # process restart: counters went to zero
                    self.resets += 1
                    self.prev[key][counter] = value
                    continue
                delta = value - previous
                self.prev[key][counter] = value
                self.window_deltas[cond] = self.window_deltas.get(cond, 0) + delta
        self.roll_window()

    def roll_window(self):
        elapsed = time.monotonic() - self.window_started
        if elapsed < self.window["alert_window_secs"]:
            return
        for cond, delta in self.window_deltas.items():
            threshold = self.rules[cond]
            key = (cond, self.paths[CONDITIONS[cond][0]])
            if delta > threshold and key not in self.active:
                self.emit("alert", cond, CONDITIONS[cond][1], key[1], delta, threshold,
                          {"window_secs": self.window["alert_window_secs"]})
                self.active.add(key)
            elif delta <= threshold and key in self.active:
                self.emit("resolved", cond, CONDITIONS[cond][1], key[1], delta, threshold,
                          {"window_secs": self.window["alert_window_secs"]})
                self.active.discard(key)
        self.window_deltas = {}
        self.window_started = time.monotonic()

    def run(self):
        started = time.monotonic()
        interval = self.window["sample_interval_ms"] / 1000.0
        while True:
            self.sample()
            if self.args.duration_secs and time.monotonic() - started >= self.args.duration_secs:
                break
            if self.args.stop_file and os.path.exists(self.args.stop_file):
                break
            time.sleep(interval)
        summary = {
            "schema": SUMMARY_SCHEMA,
            "started_at": utc_now(),
            "ended_at": utc_now(),
            "elapsed_sec": round(time.monotonic() - started, 3),
            "criteria": self.args.criteria,
            "criteria_sha256": self.criteria_sha,
            "samples": self.samples,
            "counter_resets_seen": self.resets,
            "alerts_emitted": sum(1 for a in self.emitted if a["state"] == "alert"),
            "resolved_emitted": sum(1 for a in self.emitted if a["state"] == "resolved"),
            "records": self.emitted,
        }
        with open(self.args.summary_file, "w", encoding="utf-8", newline="\n") as f:
            json.dump(summary, f, ensure_ascii=False, indent=2)
            f.write("\n")
        return 0


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--tcp-stats", required=True)
    p.add_argument("--udp-stats", required=True)
    p.add_argument("--alert-file", required=True)
    p.add_argument("--summary-file", required=True)
    p.add_argument("--criteria", required=True,
                   help="pre-registered threshold TOML (no built-in fallback)")
    p.add_argument("--duration-secs", type=float, default=0.0,
                   help="stop after N seconds; 0 = run until --stop-file appears")
    p.add_argument("--stop-file", default="",
                   help="when this path exists, finish the current sample and write summary")
    p.add_argument("--quiet", action="store_true")
    args = p.parse_args()
    criteria, criteria_sha = load_criteria(args.criteria)
    # Fail loudly if the criteria file does not define the minimum contract.
    for key in ("window", "alert_rules"):
        if key not in criteria:
            print(f"criteria missing [{key}]", file=sys.stderr)
            return 2
    return Watcher(args, criteria, criteria_sha).run()


if __name__ == "__main__":
    sys.exit(main())
