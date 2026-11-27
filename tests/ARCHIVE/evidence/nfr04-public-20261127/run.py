#!/usr/bin/env python3
"""NFR-04 公网唤醒 P95：两台不同公网网络的真实 coldrevived 双向采样。

拓扑（2026-11-27）：
- 221：家庭 NAT，真实 coldrevived 节点 `P95A`，HTTP 18800 / UDP 18801。
- 119：云端 1:1 EIP，真实 coldrevived 节点 `P95C`，HTTP 18900 / UDP 18901。
- 82：公网 relay 82.157.68.159:7100（PSK），只做唤醒候选，不承载 BT 数据面。

判据严格：
1. 双方 `nat peer` 只接收**仅 relay 候选**的通告，避免私网候选 10 s 超时污染公网 P95。
2. 先 `wake raw <file_hash> <infohash>` 预热，让 `metrics` 有真实 last_file_hash。
3. 每侧各 30 条 `metrics wake-latency 30 12000`；实现层把实际等待夹到 10 s，
   报告同时记录请求超时与实现上限。
4. 超时样本 `99999` 原样保留；P95 按 SRS 公网阈值 8000 ms 判定，局域网 3000 ms
   只作为历史口径列出，不混用。
5. 不写 G2/S6/NFR-02 结论；不把 ACK 写成公网可检索。
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import math
import os
import pathlib
import secrets
import sys
import time

import paramiko

ROOT = pathlib.Path(__file__).resolve().parents[2]
KEY = pathlib.Path(os.environ.get("TEMP", "/tmp")) / "crkeys" / "id_ed25519"
HOSTS = {
    "A": {"ssh": "192.168.1.221", "node": "P95A", "http": 18800, "wake": 18801,
          "bt": 18802, "workdir": "/root/cr-nfr04/A", "sample": "/root/cr-nfr04/sample.bin"},
    "C": {"ssh": "119.29.150.254", "node": "P95C", "http": 18900, "wake": 18901,
          "bt": 18902, "workdir": "/root/cr-nfr04/C", "sample": "/root/cr-nfr04/sample.bin"},
}
RELAY = "82.157.68.159:7100"
FILE_HASH = "49a667d142127224ef1b5343f64e6c66f7707894549d1c871609643db66f1019"
INFOHASH = "8b02ea8fdc9fd44275a829668aace7290b8b8df1"


def client(host: str) -> paramiko.SSHClient:
    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    c.connect(host, username="root", key_filename=str(KEY), timeout=15)
    return c


def run(c: paramiko.SSHClient, cmd: str, timeout: int = 60) -> dict:
    _, so, se = c.exec_command(cmd, timeout=timeout)
    out, err = so.read().decode("utf-8", "replace"), se.read().decode("utf-8", "replace")
    rc = so.channel.recv_exit_status()
    return {"cmd": cmd, "rc": rc, "stdout": out, "stderr": err}


def put(c: paramiko.SSHClient, path: str, body: str, mode: int = 0o600) -> None:
    sftp = c.open_sftp()
    with sftp.file(path, "w") as f:
        f.write(body)
    sftp.chmod(path, mode)
    sftp.close()


def cli(c: paramiko.SSHClient, port: int, command: str, args: list[str],
        timeout: int = 120) -> dict:
    payload = json.dumps({"command": command, "args": args}, ensure_ascii=False)
    cmd = ("curl -sS --max-time %d -H 'Content-Type: application/json' -X POST "
           "-d %s http://127.0.0.1:%d/cli" %
           (timeout, "'" + payload.replace("'", "'\\''") + "'", port))
    r = run(c, cmd, timeout + 10)
    try:
        r["json"] = json.loads(r["stdout"])
    except json.JSONDecodeError:
        r["json"] = None
    return r


def health(c: paramiko.SSHClient, port: int) -> dict:
    r = run(c, f"curl -sS --max-time 8 http://127.0.0.1:{port}/health", 20)
    try:
        return json.loads(r["stdout"])
    except json.JSONDecodeError:
        return {}


def percentile(values: list[float], q: float) -> float:
    s = sorted(values)
    if not s:
        return math.inf
    return s[min(max(0, math.ceil(q * len(s)) - 1), len(s) - 1)]


def unit_name(node: str) -> str:
    return f"cr-p95-{HOSTS[node]['node'].lower()}"


def stop_daemon(c: paramiko.SSHClient, node: str) -> dict:
    return run(c, f"systemctl stop {unit_name(node)} 2>/dev/null || true")


def daemon_config(node: str, peer: str, secret: str) -> str:
    h = HOSTS[node]
    p = HOSTS[peer]
    return json.dumps({
        "role": "holder",
        "bind": "0.0.0.0",
        "nodes": {
            h["node"]: {"host": h["ssh"], "http_port": h["http"],
                        "wake_udp_port": h["wake"], "bt_port": h["bt"]},
            p["node"]: {"host": p["ssh"], "http_port": p["http"],
                        "wake_udp_port": p["wake"], "bt_port": p["bt"]},
        },
        "sample_path": h["sample"],
        "keywords": ["nfr04", "public"],
        "namespace": "ColdRevive_V1:",
        "challenge_window_sec": 300,
        "workdir": h["workdir"],
        "golden_req": f"{h['workdir']}/req.json",
        "golden_ack": f"{h['workdir']}/ack.json",
        "downloader": f"{h['workdir']}/downloader",
        "nat": {
            "enabled": True,
            "stun_servers": ["119.29.150.254:3478", "82.157.68.159:3478"],
            "relay": RELAY,
            "relay_key": "file:/root/relay.key",
            "punch_route": False,
        },
        "api": {"rpc_secret": secret, "web_ui": False},
    }, ensure_ascii=False, indent=2) + "\n"


def relay_only_advertisement(node: str, peer: str) -> str:
    p = HOSTS[peer]
    return json.dumps({
        "node_id": p["node"],
        "private_ip": p["ssh"],
        "wake_port": p["wake"],
        "bt_port": p["bt"],
        "endpoints": [{
            "kind": "relay", "ip": RELAY.split(":")[0], "port": int(RELAY.split(":")[1]),
            "internal_port": p["wake"], "evidence": "harness-relay-only-wake-path",
        }],
        "bt_external": None,
        "relay": RELAY,
        "nat_type": "unknown",
        "public_udp_egress_ok": False,
        "probed_at": int(time.time()),
        "s6_status": "not_run",
        "evidence_valid_for_pass": False,
    }, ensure_ascii=False, indent=2) + "\n"


def prepare(c: paramiko.SSHClient, node: str, peer: str, secret: str,
            binary: bytes, expected_sha: str) -> dict:
    h = HOSTS[node]
    wd = h["workdir"]
    # 必须先停旧进程再换 binary。此前 prepare() 在进程仍持有可执行文件时直接 cp，
    # 得到 "Text file busy"，却继续采样，因此报出假的 0 ms P95。
    stopped = stop_daemon(c, node)
    cmds = [
        stopped,
        run(c, f"mkdir -p {wd}/downloader {wd}/meta /root/cr-nfr04"),
        run(c, "ls -l /root/relay.key"),
    ]
    if node == "A":
        cmds.append(run(c, "cp /opt/g3-clients/amule-config/Incoming/ed2k-sample.bin "
                           "/root/cr-nfr04/sample.bin"))
    sftp = c.open_sftp()
    with sftp.file("/root/cr-nfr04/coldrevived", "wb") as f:
        f.write(binary)
    sftp.chmod("/root/cr-nfr04/coldrevived", 0o755)
    sftp.close()
    copy = run(c, f"install -m 0755 /root/cr-nfr04/coldrevived {wd}/coldrevived.new && "
                  f"mv -f {wd}/coldrevived.new {wd}/coldrevived")
    cmds.append(copy)
    verify = run(c, f"sha256sum {wd}/coldrevived")
    cmds.append(verify)
    installed = verify["stdout"].strip().split()[0] if verify["rc"] == 0 else None
    if copy["rc"] != 0 or installed != expected_sha:
        raise RuntimeError(
            f"{node} binary install failed: copy_rc={copy['rc']} "
            f"installed={installed} expected={expected_sha} verify={verify}")
    put(c, f"{wd}/config.json", daemon_config(node, peer, secret))
    put(c, f"{wd}/peer.json", relay_only_advertisement(node, peer))
    put(c, f"{wd}/req.json", "{}\n")
    put(c, f"{wd}/ack.json", "{}\n")
    return {
        "setup": cmds,
        "installed_binary_sha256": installed,
        "expected_binary_sha256": expected_sha,
        "config_sha256": hashlib.sha256(
            daemon_config(node, peer, secret).encode()).hexdigest(),
    }


def start(c: paramiko.SSHClient, node: str, expected_sha: str) -> dict:
    h = HOSTS[node]
    unit = unit_name(node)
    stop = stop_daemon(c, node)
    start_cmd = (
        f"systemd-run --collect --unit={unit} --property=WorkingDirectory={h['workdir']} "
        f"{h['workdir']}/coldrevived --config {h['workdir']}/config.json "
        f"--node-id {h['node']} --bind 0.0.0.0"
    )
    started = run(c, start_cmd)
    time.sleep(2.5)
    hlt = health(c, h["http"])
    pid = hlt.get("pid")
    exe = run(c, f"readlink -f /proc/{pid}/exe && sha256sum /proc/{pid}/exe", 20) if pid else {
        "cmd": None, "rc": 1, "stdout": "", "stderr": "health has no pid",
    }
    running_sha = exe["stdout"].strip().splitlines()[-1].split()[0] if exe["rc"] == 0 else None
    if started["rc"] != 0 or hlt.get("status") != "ok" or running_sha != expected_sha:
        raise RuntimeError(
            f"{node} daemon not running expected binary: start_rc={started['rc']} "
            f"health={hlt} exe={exe} expected={expected_sha}")
    return {
        "unit": unit, "stop": stop, "start": started, "health": hlt,
        "running_exe": exe, "running_binary_sha256": running_sha,
    }


def sample_side(c: paramiko.SSHClient, node: str, count: int, timeout_ms: int) -> dict:
    h = HOSTS[node]
    plan = cli(c, h["http"], "nat", ["plan"])
    peer = cli(c, h["http"], "nat", ["peer", f"{h['workdir']}/peer.json"])
    warm = cli(c, h["http"], "wake", ["raw", FILE_HASH, INFOHASH])
    forced = cli(c, h["http"], "nat", ["wake", FILE_HASH, INFOHASH, "relay"])
    ms = cli(c, h["http"], "metrics", ["wake-latency", str(count), str(timeout_ms)])
    values = ms.get("json", {}).get("samples_ms", []) if isinstance(ms.get("json"), dict) else []
    values = [float(v) for v in values if isinstance(v, (int, float))]
    forced_json = forced.get("json") if isinstance(forced.get("json"), dict) else {}
    forced_rtt = forced_json.get("rtt_ms")
    invalid_reasons: list[str] = []
    if len(values) != count:
        invalid_reasons.append(f"sample_count={len(values)},expected={count}")
    timeouts = sum(1 for v in values if v >= 99999)
    if timeouts:
        invalid_reasons.append(f"timeouts={timeouts}")
    # 0 ms 曾由旧二进制的硬编码 RTT 产生。强制定向 relay 的锚点样本必须是真实正数，
    # 且本轮 metrics 至少有一个正数，否则这不是「低延迟」，而是量具失效。
    if forced_json.get("ok") is not True or forced_json.get("path") != "relay":
        invalid_reasons.append("forced_relay_wake did not take relay path")
    if not isinstance(forced_rtt, (int, float)) or float(forced_rtt) <= 0:
        invalid_reasons.append(f"forced_relay_rtt_ms={forced_rtt!r}")
    if not any(v > 0 for v in values):
        invalid_reasons.append("all metrics samples are zero")
    return {
        "node": h["node"], "host": h["ssh"], "http_port": h["http"],
        "plan": plan.get("json"), "peer": peer.get("json"),
        "warm_wake": warm.get("json"), "forced_relay_wake": forced_json,
        "samples_ms": values, "sample_count": len(values),
        "timeouts": timeouts,
        "p95_ms": round(percentile(values, 0.95), 2) if values else None,
        "threshold_public_ms": 8000,
        "threshold_lan_ms_for_reference_only": 3000,
        "requested_timeout_ms": timeout_ms,
        "implementation_cap_ms": 10000,
        "measurement_valid": not invalid_reasons,
        "invalid_reasons": invalid_reasons,
        "metrics_raw": ms,
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--report-out", required=True)
    ap.add_argument("--count", type=int, default=30)
    ap.add_argument("--timeout-ms", type=int, default=12000)
    ap.add_argument("--binary", default="/opt/cr-head-20261127/target/release/coldrevived",
                    help="221 上已构建的最新 coldrevived 路径")
    args = ap.parse_args()

    ca, cc = client(HOSTS["A"]["ssh"]), client(HOSTS["C"]["ssh"])
    b = run(ca, f"sha256sum {args.binary} && base64 -w0 {args.binary}", 60)
    import base64
    binary = base64.b64decode(b["stdout"].strip().splitlines()[-1])
    sha = hashlib.sha256(binary).hexdigest()

    secret = secrets.token_hex(32)
    prep_a = prepare(ca, "A", "C", secret, binary, sha)
    prep_c = prepare(cc, "C", "A", secret, binary, sha)
    # 两端用同一份 1 MiB 样本，file_hash/infohash 才对得上。
    sftp_a, sftp_c = ca.open_sftp(), cc.open_sftp()
    with sftp_a.file(HOSTS["A"]["sample"], "rb") as f:
        sample = f.read()
    with sftp_c.file(HOSTS["C"]["sample"], "wb") as f:
        f.write(sample)
    sftp_a.close(); sftp_c.close()

    started = {"A": start(ca, "A", sha), "C": start(cc, "C", sha)}
    a = sample_side(ca, "A", args.count, args.timeout_ms)
    c = sample_side(cc, "C", args.count, args.timeout_ms)

    def verdict(side: dict) -> str:
        if not side["measurement_valid"]:
            return "INVALID"
        if side["sample_count"] < args.count or side["timeouts"]:
            return "FAIL"
        return "PASS" if side["p95_ms"] is not None and side["p95_ms"] < 8000 else "FAIL"

    verdict_a, verdict_c = verdict(a), verdict(c)
    report = {
        "schema": "coldrevive.nfr04.public-p95/v2",
        "generated_at": dt.datetime.now().astimezone().isoformat(timespec="seconds"),
        "scope": "真实双机跨公网 WAKEUP_REQ→ACK 时延；仅 relay 唤醒候选；不是 G2/S6/NAT 可达证据",
        "binary_sha256": sha,
        "file_hash": FILE_HASH,
        "infohash": INFOHASH,
        "sample_sha256": hashlib.sha256(sample).hexdigest(),
        "networks": {
            "A": "家庭宽带出口 112.5.60.4（221）",
            "C": "腾讯云 1:1 EIP 119.29.150.254",
            "relay": RELAY,
        },
        "preparation": {"A": prep_a, "C": prep_c},
        "started": started,
        "directions": {"A_to_C": a, "C_to_A": c},
        "verdicts": {"A_to_C": verdict_a, "C_to_A": verdict_c},
        "acceptance": {
            "nfr04_public_p95": "PASS" if verdict_a == verdict_c == "PASS" else "NOT_PASS",
            "rule": "两侧都必须是真实非零样本且 P95 < 8000 ms；INVALID 不得当 FAIL 之外的任何通过",
        },
        "gate_snapshot": {
            "g2_claim": True, "s6_status": "not_run",
            "evidence_valid_for_pass": False, "auto_pass": False,
        },
        "is_s6_evidence": False,
    }
    out = pathlib.Path(args.report_out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    ca.close(); cc.close()
    print(json.dumps({
        "report": str(out), "sha256": sha,
        "verdicts": report["verdicts"],
        "p95_ms": {"A_to_C": a["p95_ms"], "C_to_A": c["p95_ms"]},
        "counts": {"A_to_C": a["sample_count"], "C_to_A": c["sample_count"]},
    }, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
