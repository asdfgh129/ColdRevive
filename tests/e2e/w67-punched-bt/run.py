#!/usr/bin/env python3
"""W6.7 daemon-level BT over punched TCP (public, cross-network).

Topology (2026-11-27, revision 2 — a real public TCP port is available):
- H = 119.29.150.254, Lighthouse instance, public 1:1 EIP, role=holder,
      engine.listen=127.0.0.1:19730. The owner opened TCP/19720 in the Lighthouse
      firewall, so the punch socket binds 0.0.0.0:19720 and the peer dials
      119.29.150.254:19720 directly. No REDIRECT anywhere: an earlier revision tried
      to rewrite inbound 22 -> 19720 and could never satisfy TCP simultaneous open
      because the two sides then used different port pairs.
- M = 192.168.1.221, home NAT (EIM), role=member.
      Punch socket binds 192.168.1.221:19710; its mapped public port is discovered
      at the same endpoint the punch will use, then consumed immediately.
- Observer = one coldrevive-nat observer on 119.29.150.254:19720, i.e. exactly the
      endpoint the punch targets, so no cross-destination port extrapolation happens.
      The mapping is sampled twice and both samples must agree before punching.
- Relay 82.157.68.159:7100 is used only for ADR-022 two-phase verdict control,
      never as the BT data plane.

PASS needs all of: mapping observed twice and identical at the punch endpoint, both
sides poll+SO_ERROR punch established, both sides route=punched-direct, real SOCKS5
bridge registration, real BT handshake/piece transfer, 1 MiB landed with md5, and
holder splice counters > 0.
No success rate. No S6/G2 claim. No relay data-plane substitution.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import pathlib
import re
import secrets
import subprocess
import sys
import threading
import time

import paramiko

ROOT = pathlib.Path(__file__).resolve().parents[2]
KEY = pathlib.Path(os.environ.get("TEMP", "/tmp")) / "crkeys" / "id_ed25519"
BIN_SRC = "/opt/cr-head-20261127/target/release/coldrevived"
NAT_SRC = "/opt/cr-head-20261127/target/release/coldrevive-nat"
FILE_HASH = "49a667d142127224ef1b5343f64e6c66f7707894549d1c871609643db66f1019"
INFOHASH = "8b02ea8fdc9fd44275a829668aace7290b8b8df1"
EXPECTED_MD5 = "6209e962f3b26a13c9ec1ae70fd78e10"
SAMPLE_BYTES = 1048576
HOME_EGRESS = "112.5.60.4"
RELAY = "82.157.68.159:7100"
OBS82, OBS119 = "82.157.68.159", "119.29.150.254"
# 打洞窗口（毫秒）。覆盖默认值，可在命令行用 --punch-timeout-ms 调整。
PUNCH_MS = 4000

HOSTS = {
    # bt_public is now the real, firewall-opened public port on H. It is both the
    # bind port of the holder's punch socket and the port the member dials.
    "H": {"ssh": OBS119, "node": "H", "http": 19711, "wake": 19712,
          "bt_public": 19720, "workdir": "/root/cr-w67/holder"},
    "M": {"ssh": "192.168.1.221", "node": "M", "http": 19721, "wake": 19722,
          "bt_public": 19910, "workdir": "/root/cr-w67/member"},
}
UNITS = {
    "H": "cr-w67-h", "M": "cr-w67-m", "obsH": "cr-w67-obs-h",
    "http": "cr-w67-http", "capH": "cr-w67-cap-h", "capM": "cr-w67-cap-m",
}


def client(host: str) -> paramiko.SSHClient:
    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    c.connect(host, username="root", key_filename=str(KEY), timeout=15)
    return c


def run(c: paramiko.SSHClient, cmd: str, timeout: int = 90) -> dict:
    _, so, se = c.exec_command(cmd, timeout=timeout)
    out, err = so.read().decode("utf-8", "replace"), se.read().decode("utf-8", "replace")
    rc = so.channel.recv_exit_status()
    return {"cmd": cmd, "rc": rc, "stdout": out, "stderr": err}


def put(c: paramiko.SSHClient, path: str, body: str | bytes, mode: int = 0o600) -> None:
    sftp = c.open_sftp()
    with sftp.file(path, "wb") as f:
        f.write(body.encode() if isinstance(body, str) else body)
    sftp.chmod(path, mode)
    sftp.close()


def get(c: paramiko.SSHClient, path: str) -> bytes:
    sftp = c.open_sftp()
    with sftp.file(path, "rb") as f:
        data = f.read()
    sftp.close()
    return data


def cli(c: paramiko.SSHClient, port: int, command: str, args: list[str],
        timeout: int = 90) -> dict:
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


def rpc(c: paramiko.SSHClient, port: int, secret: str, method: str,
        params: list, timeout: int = 30) -> dict:
    payload = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method,
                          "params": ["token:" + secret] + params}, ensure_ascii=False)
    cmd = ("curl -sS --max-time %d -H 'Content-Type: application/json' -X POST "
           "-d %s http://127.0.0.1:%d/jsonrpc" %
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


def parse_json_line(raw: str):
    for line in raw.splitlines():
        line = line.strip()
        if line.startswith("{"):
            try:
                return json.loads(line)
            except json.JSONDecodeError:
                return None
    return None


def stop_unit(c: paramiko.SSHClient, unit: str) -> dict:
    return run(c, f"systemctl stop {unit} 2>/dev/null || true")


def start_unit(c: paramiko.SSHClient, unit: str, workdir: str, command: str) -> dict:
    stopped = stop_unit(c, unit)
    started = run(c, f"systemd-run --collect --unit={unit} "
                     f"--property=WorkingDirectory={workdir} {command}", 60)
    time.sleep(2.0)
    return {"unit": unit, "stopped": stopped, "start": started}


def daemon_config(node: str, peer: str, secret: str) -> str:
    h, p = HOSTS[node], HOSTS[peer]
    holder = node == "H"
    nat_bt_external = (f"{OBS119}:{h['bt_public']}" if holder
                       else f"{HOME_EGRESS}:{h['bt_public']}")
    engine = {"dir": h["workdir"] + "/downloads", "guard_handshake": False}
    if holder:
        engine["listen"] = "127.0.0.1:19730"
    return json.dumps({
        "role": "holder" if holder else "member",
        "bind": "0.0.0.0",
        "nodes": {
            "H": {"host": OBS119, "http_port": HOSTS["H"]["http"],
                  "wake_udp_port": HOSTS["H"]["wake"], "bt_port": HOSTS["H"]["bt_public"]},
            "M": {"host": HOSTS["M"]["ssh"], "http_port": HOSTS["M"]["http"],
                  "wake_udp_port": HOSTS["M"]["wake"], "bt_port": HOSTS["M"]["bt_public"]},
        },
        "sample_path": h["workdir"] + ("/sample.bin" if holder else "/placeholder.bin"),
        "keywords": ["w67", "punched-direct"],
        "namespace": "ColdRevive_V1:",
        "challenge_window_sec": 300,
        "workdir": h["workdir"],
        "golden_req": h["workdir"] + "/req.json",
        "golden_ack": h["workdir"] + "/ack.json",
        "downloader": h["workdir"] + "/downloader",
        "engine": engine,
        "nat": {
            "enabled": True,
            "stun_servers": [OBS82 + ":3478", OBS119 + ":3478"],
            "relay": RELAY,
            "relay_key": "file:/root/relay.key",
            "punch_route": True,
            "bt_external": nat_bt_external,
        },
        "api": {"rpc_secret": secret, "web_ui": False},
    }, ensure_ascii=False, indent=2) + "\n"


def advertisement(node: str) -> str:
    h = HOSTS[node]
    if node == "H":
        bt = {"kind": "configured-mapping", "ip": OBS119, "port": h["bt_public"],
              "internal_port": h["bt_public"], "evidence": "operator-public-1to1-eip"}
    else:
        bt = {"kind": "configured-mapping", "ip": HOME_EGRESS, "port": h["bt_public"],
              "internal_port": h["bt_public"], "evidence": "operator-configured-bt-mapping"}
    return json.dumps({
        "node_id": node,
        "private_ip": h["ssh"],
        "wake_port": h["wake"],
        "bt_port": h["bt_public"],
        "endpoints": [{"kind": "private-lan", "ip": h["ssh"], "port": h["wake"],
                       "internal_port": h["wake"], "evidence": "configured-private-host"}],
        "bt_external": bt,
        "relay": RELAY,
        "nat_type": "unknown",
        "public_udp_egress_ok": False,
        "probed_at": int(time.time()),
        "s6_status": "not_run",
        "evidence_valid_for_pass": False,
    }, ensure_ascii=False, indent=2) + "\n"


def install_binary(c: paramiko.SSHClient, data: bytes, path: str, expected_sha: str) -> dict:
    put(c, path + ".new", data, 0o755)
    r = run(c, f"chmod 755 {path}.new && mv -f {path}.new {path} && sha256sum {path}")
    installed = r["stdout"].strip().split()[0] if r["rc"] == 0 else None
    if installed != expected_sha:
        raise RuntimeError(f"binary install failed {path}: {r} expected={expected_sha}")
    return r


def discover(c: paramiko.SSHClient, local: str, observer: str) -> dict:
    """Ask the observer what public port `local` is mapped to."""
    r = run(c, f"/root/cr-w67/coldrevive-nat --discover-tcp-port {local} {observer} "
               f"--timeout 3000 --json", 40)
    r["parsed"] = parse_json_line(r["stdout"])
    return r


def punch(c: paramiko.SSHClient, node: str, peer: str, bind_hint: str,
          discovered_port: int, nonce: str, punch_ms: int = 4000) -> dict:
    port = HOSTS[node]["http"]
    args = ["punch-route", peer, bind_hint, str(discovered_port), nonce, str(punch_ms)]
    r = cli(c, port, "nat", args, timeout=90)
    return {"node": node, "peer": peer, "bind_hint": bind_hint,
            "discovered_port_argument": discovered_port, "punch_ms": punch_ms, "args": args,
            "result": r.get("json"), "raw": r}


def capture(c: paramiko.SSHClient, unit: str, log: str, bpf: str,
            seconds: int = 240) -> dict:
    # 窗口必须覆盖「打洞 → 握手 → 整个 BT 传输」。25 s 会在数据面开始前就结束，
    # 于是报告里只有 SYN、没有负载，最后只能靠计数器自证——那是弱证据。
    script = f"timeout {seconds} tcpdump -nn -i any -l '{bpf}' > {log} 2>&1"
    cmd = f"/bin/bash -lc \"{script}\""
    return start_unit(c, unit, "/root/cr-w67", cmd)


def main() -> int:
    global PUNCH_MS
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--report-out", required=True)
    ap.add_argument("--punch-timeout-ms", type=int, default=4000)
    args = ap.parse_args()
    PUNCH_MS = max(300, min(15000, args.punch_timeout_ms))

    report: dict = {
        "schema": "coldrevive.w67.punched-tcp-bt/v1",
        "generated_at": dt.datetime.now().astimezone().isoformat(timespec="seconds"),
        "scope": "daemon-level punched-direct TCP carrying real BT payload; component evidence, not G2/S6",
        "topology": {
            "H": "119.29.150.254 public EIP, holder, engine.listen=127.0.0.1:19730, punch bind 0.0.0.0:19720",
            "M": "192.168.1.221 home EIM NAT, member, punch bind 192.168.1.221:19710",
            "observer": OBS119 + ":19720",
            "observer_note": ("observer sits on the exact endpoint the punch targets; "
                              "the mapping is sampled twice at that endpoint and must agree, "
                              "so no cross-destination port extrapolation is performed"),
            "public_port_provisioning": "TCP/19720 opened in the Lighthouse firewall by the owner",
            "redirect_used": False,
            "relay_control_only": RELAY,
        },
        "file_hash": FILE_HASH, "infohash": INFOHASH, "expected_md5": EXPECTED_MD5,
        "gate_snapshot": {
            "g2_claim": True, "s6_status": "not_run",
            "evidence_valid_for_pass": False, "auto_pass": False,
        },
        "claim_scope": {
            "g2_claim_from_this_evidence": False,
            "is_s6_evidence": False,
            "is_relay_data_plane_evidence": False,
            "success_rate_reported": False,
        },
        "steps": {}, "checks": [],
    }

    cH, cM = client(HOSTS["H"]["ssh"]), client(HOSTS["M"]["ssh"])
    for c in (cH, cM):
        run(c, "mkdir -p /root/cr-w67 /root/cr-w67/holder /root/cr-w67/member")
    try:
        # 0) binaries and sample.
        bin_data = get(cM, BIN_SRC)
        nat_data = get(cM, NAT_SRC)
        bin_sha, nat_sha = (hashlib.sha256(x).hexdigest() for x in (bin_data, nat_data))
        report["steps"]["binaries"] = {
            "coldrevived_sha256": bin_sha, "coldrevive_nat_sha256": nat_sha,
        }
        install_binary(cM, bin_data, "/root/cr-w67/member/coldrevived", bin_sha)
        install_binary(cH, bin_data, "/root/cr-w67/holder/coldrevived", bin_sha)
        install_binary(cM, nat_data, "/root/cr-w67/coldrevive-nat", nat_sha)
        install_binary(cH, nat_data, "/root/cr-w67/coldrevive-nat", nat_sha)

        sample = get(cM, "/opt/g3-clients/amule-config/Incoming/ed2k-sample.bin")
        if len(sample) != SAMPLE_BYTES:
            raise RuntimeError(f"sample size {len(sample)} != {SAMPLE_BYTES}")
        put(cH, "/root/cr-w67/holder/sample.bin", sample, 0o644)
        put(cM, "/root/cr-w67/member/placeholder.bin", b"x", 0o644)
        report["steps"]["sample"] = {
            "bytes": len(sample), "sha256": hashlib.sha256(sample).hexdigest(),
        }

        secret = secrets.token_hex(32)
        for c, node in ((cH, "H"), (cM, "M")):
            wd = HOSTS[node]["workdir"]
            # 下载目录必须清空。上一次跑剩下的 sample.bin 会让 librqbit 直接校验通过、
            # 报 complete 且 md5 相符，但**一个字节都没传**——那是假的通过。
            # 这条是硬约束（engine.dir 必须为空），每次都要显式确认。
            clean = run(c, f"rm -rf {wd}/downloads && mkdir -p {wd}/downloads && "
                           f"find {wd}/downloads -type f | wc -l")
            report["steps"].setdefault("download_dir_clean", {})[node] = clean["stdout"].strip()
            run(c, f"mkdir -p {wd}/downloader {wd}/meta /root/cr-w67")
            put(c, wd + "/config.json", daemon_config(node, "M" if node == "H" else "H", secret))
            put(c, wd + "/peer-holder.json", advertisement("H"))
            put(c, wd + "/peer-member.json", advertisement("M"))
            put(c, wd + "/req.json", "{}\n")
            put(c, wd + "/ack.json", "{}\n")
        for node, val in report["steps"]["download_dir_clean"].items():
            if val != "0":
                raise RuntimeError(f"{node} 下载目录未清空（残留 {val} 个文件）")

        # 1) 观测点就是打洞目标端点本身。不做任何 iptables / REDIRECT：
        #    TCP/19720 是 H 上真实放通的端口，改写规则在这里也永远不会奏效，
        #    因为两侧必须用同一对端口。观测在 `observe_mapping()` 里成对完成。

        # 2) daemons, peer ads, capture — all before the time-critical discovery.
        startH = start_unit(cH, UNITS["H"], HOSTS["H"]["workdir"],
                            f"{HOSTS['H']['workdir']}/coldrevived --config {HOSTS['H']['workdir']}/config.json "
                            f"--node-id H --bind 0.0.0.0")
        startM = start_unit(cM, UNITS["M"], HOSTS["M"]["workdir"],
                            f"{HOSTS['M']['workdir']}/coldrevived --config {HOSTS['M']['workdir']}/config.json "
                            f"--node-id M --bind 0.0.0.0")
        hltH, hltM = health(cH, HOSTS["H"]["http"]), health(cM, HOSTS["M"]["http"])
        report["steps"]["daemon_start"] = {"H": startH, "M": startM,
                                            "health_H": hltH, "health_M": hltM}
        if hltH.get("status") != "ok" or hltM.get("status") != "ok":
            raise RuntimeError("daemon health not ok")

        peerH = cli(cH, HOSTS["H"]["http"], "nat", ["peer", HOSTS["H"]["workdir"] + "/peer-member.json"])
        peerM = cli(cM, HOSTS["M"]["http"], "nat", ["peer", HOSTS["M"]["workdir"] + "/peer-holder.json"])
        report["steps"]["peer_load"] = {"H": peerH.get("json"), "M": peerM.get("json")}
        if not (peerH.get("json") or {}).get("ok") or not (peerM.get("json") or {}).get("ok"):
            raise RuntimeError("nat peer load failed")

        capH = capture(cH, UNITS["capH"], "/root/cr-w67/tcpdump-h.log",
                       "host 112.5.60.4 or tcp port 19720")
        capM = capture(cM, UNITS["capM"], "/root/cr-w67/tcpdump-m.log",
                       "host 119.29.150.254 or tcp port 19710")
        report["steps"]["capture_start"] = {"H": capH, "M": capM}

        # 3) BT metadata prep. Everything that can be slow happens BEFORE the punch,
        #    because the previous revision let `wake ack` block for its full 90 s CLI
        #    timeout and the downloader engine was only created 94 s after the punch —
        #    by then librqbit had already closed the holder's idle splice and the
        #    member's 68-byte BT handshake went into a dead socket.
        #    `wake ack` is dropped here: it is a metadata handshake that demonstrably
        #    timed out in the previous run, while the torrent is fetched directly from
        #    the holder's HTTP endpoint and its infohash is checked against the seed
        #    index, so the data-plane test does not depend on it.
        #    The runtime infohash comes from the holder's real seed index; the fixed
        #    INFOHASH is historical and changes with keywords/namespace.
        state_raw = get(cH, HOSTS["H"]["workdir"] + "/state-H.json")
        state = json.loads(state_raw.decode("utf-8"))
        entry = next((e for e in state.get("entries", [])
                      if e.get("file_hash") == FILE_HASH), None)
        runtime_infohash = (entry or {}).get("infohash", "")
        report["runtime_infohash"] = runtime_infohash
        if not runtime_infohash:
            raise RuntimeError("holder seed index has no runtime infohash")
        torrent_path = "/root/cr-w67/holder/sample.torrent"
        tf = run(cH, f"curl -sS --max-time 10 -o {torrent_path} "
                     f"http://127.0.0.1:{HOSTS['H']['http']}/torrent/{runtime_infohash} && "
                     f"wc -c {torrent_path}")
        report["steps"]["torrent_fetch_on_holder"] = tf
        torrent = get(cH, torrent_path)
        if tf["rc"] != 0 or len(torrent) < 100 or b"4:info" not in torrent:
            raise RuntimeError(f"holder torrent invalid: rc={tf['rc']} bytes={len(torrent)}")
        put(cM, "/root/cr-w67/member/sample.torrent", torrent, 0o644)
        report["steps"]["torrent_http"] = {
            "start": start_unit(cM, UNITS["http"], "/root/cr-w67/member",
                                "python3 -m http.server 19880 --bind 127.0.0.1"),
            "bytes": len(torrent)}
        time.sleep(1.0)

        # 4) 观测映射：同一端点采两次，必须一致；随后停掉 observer，
        #    让打洞 socket 能绑同一个端口。
        #
        #    为什么做成函数：一次 BT 会话结束后，家宽 NAT 的映射可能已经换了源端口。
        #    重打洞前必须**重新发现**，否则 holder 会一直对着一个没人等的旧 4-tuple
        #    发 SYN（2026-11-27 现场：H 27 轮静默、M 每轮 Connection refused）。
        def observe_mapping(tag: str, n_samples: int = 2) -> tuple[int, dict]:
            start_unit(cH, UNITS["obsH"], "/root/cr-w67",
                       f"/root/cr-w67/coldrevive-nat --serve-observer "
                       f"0.0.0.0:{HOSTS['H']['bt_public']} --observer-secs 180 --json")
            ready = run(cH, f"ss -lntp | grep -c ':{HOSTS['H']['bt_public']}'")
            if ready["stdout"].strip() in ("", "0"):
                raise RuntimeError(f"[{tag}] observer 未在 {HOSTS['H']['bt_public']} 监听")
            samples = [discover(cM, "192.168.1.221:19710",
                                f"{OBS119}:{HOSTS['H']['bt_public']}") for _ in range(n_samples)]
            ports = [int(s["parsed"]["discovered_port"]) for s in samples
                     if s["parsed"] and s["parsed"].get("discovered_port")]
            stop_unit(cH, UNITS["obsH"])
            time.sleep(0.5)
            stable = len(ports) == n_samples and len(set(ports)) == 1
            detail = {
                "tag": tag,
                "samples": [{"rc": s["rc"], "parsed": s["parsed"]} for s in samples],
                "ports": ports, "stable": stable,
                "eim_flags": [s["parsed"].get("eim") for s in samples if s["parsed"]],
            }
            return (ports[0] if stable else 0), detail

        discovered_port, disc0 = observe_mapping("initial")
        mapping_stable = disc0["stable"]
        report["steps"]["discovery"] = disc0
        if not discovered_port:
            raise RuntimeError(f"发现端口为空或不稳定，无法打洞: {disc0}")
        if not mapping_stable:
            raise RuntimeError(f"同一端点两次采样不一致，拒绝外推: {disc0['ports']}")

        # 5) simultaneous punch with shared nonce.
        #
        # 一次 attempt 内，两端各发一次 `punch-route`：TCP 同时打开必须两侧一起发
        # SYN，所以「同时」由脚本用 barrier 保证。M 侧的 SSH 建连放在 barrier
        # **之前**——之前是在线程里现连，M 比 H 晚 200～400 ms 才发包，两边的
        # SYN 窗口根本对不上，每一轮都互相 RST（8 s / 16 轮全是 Connection refused）。
        #
        # 允许有限次重试：打洞与经中继的两轮裁决都可能真实失败，失败就再试，
        # 但每一次都如实记进 attempts，不挑好看的写。
        def punch_pair(tag: str, port: int, max_attempts: int = 3) -> tuple[dict, list]:
            attempts: list = []
            for i in range(max_attempts):
                n = secrets.token_hex(16)
                gate = threading.Barrier(2)
                out: dict = {}

                def do_m() -> None:
                    cm = client(HOSTS["M"]["ssh"])
                    try:
                        gate.wait()
                        out["M"] = punch(cm, "M", "H", "192.168.1.221:19710",
                                         HOSTS["H"]["bt_public"], n, PUNCH_MS)
                    finally:
                        cm.close()

                t = threading.Thread(target=do_m, daemon=True)
                t.start()
                gate.wait()
                try:
                    out["H"] = punch(cH, "H", "M", "0.0.0.0:19720",
                                     port, n, PUNCH_MS)
                finally:
                    t.join(timeout=120)
                h_res = (out.get("H") or {}).get("result") or {}
                m_res = (out.get("M") or {}).get("result") or {}
                attempts.append({"tag": tag, "attempt": i + 1, "nonce": n,
                                 "H": h_res, "M": m_res})
                if (h_res.get("route") == "punched-direct"
                        and m_res.get("route") == "punched-direct"):
                    break
                time.sleep(0.5)
            last = attempts[-1] if attempts else {"H": {}, "M": {}}
            pair = {"H": {"result": last["H"]}, "M": {"result": last["M"]}}
            return pair, attempts

        result_h, punch_attempts = punch_pair("initial", discovered_port)
        report["steps"]["punch"] = result_h
        report["steps"]["punch_attempts"] = punch_attempts
        time.sleep(1.0)

        tcpdump_h0 = run(cH, "cat /root/cr-w67/tcpdump-h.log 2>/dev/null || true")
        tcpdump_m0 = run(cM, "cat /root/cr-w67/tcpdump-m.log 2>/dev/null || true")
        report["steps"]["tcpdump_after_punch"] = {"H": tcpdump_h0["stdout"][-4000:],
                                                  "M": tcpdump_m0["stdout"][-4000:]}

        # 6) real BT payload over the punched socket. The downloader engine is created
        #    here, seconds after the socket was registered, so its first BT handshake
        #    finds a live splice instead of an idle timeout.
        hltH_p, hltM_p = health(cH, HOSTS["H"]["http"]), health(cM, HOSTS["M"]["http"])
        report["steps"]["health_after_punch"] = {"H": hltH_p, "M": hltM_p}
        report["steps"]["holder_log_after_punch"] = run(
            cH, "journalctl -u cr-w67-h --no-pager -n 20 2>/dev/null | tail -6")["stdout"]
        add = rpc(cM, HOSTS["M"]["http"], secret, "aria2.addUri",
                  [["http://127.0.0.1:19880/sample.torrent"]])
        gid = ((add.get("json") or {}).get("result") or "")
        report["steps"]["add_uri"] = add
        if not gid:
            raise RuntimeError("aria2.addUri no gid")

        def repunch(tag: str) -> dict:
            """重新发现映射并两端同时重打洞。

            可重入登记只解决「桥还认不认这个键」；TCP 同时打开必须两侧一起发 SYN，
            所以补装必须成对触发——这由脚本（同时持有两端控制通道）负责，
            不是单侧能自愈的事。映射也必须重新发现：旧端口可能已经不属于这条连接了。
            """
            port, detail = observe_mapping(tag)
            if not port:
                return {"tag": tag, "error": "映射重发现失败或不稳定",
                        "discovery": detail}
            pair, attempts = punch_pair(tag, port, max_attempts=2)
            return {"tag": tag,
                    "H": (pair.get("H") or {}).get("result"),
                    "M": (pair.get("M") or {}).get("result"),
                    "attempts": len(attempts),
                    "discovered_port": port,
                    "discovery": detail}

        # 下载期间盯住 member 的 `socks5_awaiting`。它只在「会话已结束、桥里没有
        # socket」时才 >0（`socks5_in_use` 覆盖正常传输中的状态），所以不会在
        # 传输健康时误触发补装。再加一道「进度停滞」条件：socket 还活着的时候
        # 重绑本地端口只会拿到 EADDRNOTAVAIL。
        def download_with_repunch(seconds: int, max_repunch: int) -> tuple[dict, list]:
            deadline = time.time() + seconds
            last, samples, log = {}, [], []
            stated: dict = {}
            next_health = 0.0
            progress_bytes = -1
            progress_at = time.time()
            while time.time() < deadline:
                r = rpc(cM, HOSTS["M"]["http"], secret, "aria2.tellStatus", [gid])
                last = (r.get("json") or {}).get("result") or {}
                done = int(last.get("completedLength") or 0)
                if done != progress_bytes:
                    progress_bytes = done
                    progress_at = time.time()
                stalled = (time.time() - progress_at) >= 6.0
                row = {"status": last.get("status"),
                       "completed": last.get("completedLength"),
                       "connections": last.get("connections"),
                       "stalled": stalled}
                if time.time() >= next_health:
                    stated = health(cM, HOSTS["M"]["http"]).get("data_plane") or {}
                    row["awaiting"] = stated.get("socks5_awaiting")
                    row["in_use"] = stated.get("socks5_in_use")
                    row["rearmed"] = stated.get("socks5_rearmed")
                    row["served_after_rearm"] = stated.get("socks5_served_after_rearm")
                    next_health = time.time() + 2.0
                samples.append(row)
                if last.get("status") in ("complete", "error"):
                    break
                awaiting = int(stated.get("socks5_awaiting") or 0)
                if awaiting > 0 and stalled and len(log) < max_repunch:
                    entry = repunch(f"repunch-{len(log) + 1}")
                    entry["at_completed_bytes"] = last.get("completedLength")
                    entry["trigger_awaiting"] = awaiting
                    entry["trigger_stalled_s"] = round(time.time() - progress_at, 1)
                    log.append(entry)
                    stated = health(cM, HOSTS["M"]["http"]).get("data_plane") or {}
                    next_health = time.time() + 2.0
                    progress_at = time.time()
                    time.sleep(1.0)
                time.sleep(0.4)
            return {"final": last, "samples_tail": samples[-16:]}, log

        dl, repunch_log = download_with_repunch(seconds=150, max_repunch=4)
        report["steps"]["download"] = dl
        report["steps"]["repunch"] = repunch_log

        find = run(cM, f"find {HOSTS['M']['workdir']}/downloads -type f -printf '%p %s\\n'")
        files = []
        for line in find["stdout"].splitlines():
            parts = line.strip().split()
            if len(parts) == 2:
                files.append({"path": parts[0], "bytes": int(parts[1])})
        for f in files:
            f["md5"] = run(cM, f"md5sum {f['path']}")["stdout"].strip().split()[0]
        report["steps"]["landed_files"] = files
        hltH2, hltM2 = health(cH, HOSTS["H"]["http"]), health(cM, HOSTS["M"]["http"])
        report["steps"]["health_after"] = {"H": hltH2, "M": hltM2}

        # 完整抓包：覆盖打洞 + 握手 + 整个传输，作为「字节真的走了打洞 socket」
        # 的线路级证据，而不是只看计数器。
        tcpdump_h = run(cH, "cat /root/cr-w67/tcpdump-h.log 2>/dev/null || true")
        tcpdump_m = run(cM, "cat /root/cr-w67/tcpdump-m.log 2>/dev/null || true")
        report["steps"]["tcpdump"] = {"H": tcpdump_h["stdout"][-200000:],
                                       "M": tcpdump_m["stdout"][-200000:]}

        def wire_summary(text: str, port: int) -> dict:
            """统计某端口上的帧数与负载字节。只看 length 字段，不做推断。

            注意两个方向的行格式不同：入向是 `src.port > host.port:`，
            出向是 `host.port > dst.port:`——只匹配 `.{port}:` 会把出向整个漏掉。
            """
            frames, payload, biggest = 0, 0, 0
            out_frames, out_payload = 0, 0
            for line in text.splitlines():
                if f".{port}:" not in line and f".{port} >" not in line:
                    continue
                m = re.search(r"length (\d+)\s*$", line.strip())
                if not m:
                    continue
                n = int(m.group(1))
                frames += 1
                payload += n
                biggest = max(biggest, n)
                if f"Out IP " in line and f".{port} >" in line:
                    out_frames += 1
                    out_payload += n
            return {"port": port, "frames": frames, "payload_bytes": payload,
                    "out_frames": out_frames, "out_payload_bytes": out_payload,
                    "largest_frame": biggest}

        report["wire"] = {"H_punch_port": wire_summary(tcpdump_h["stdout"], 19720),
                          "M_punch_port": wire_summary(tcpdump_m["stdout"], 19710)}

        punch_h = (result_h["H"].get("result") or {})
        punch_m = (result_h["M"].get("result") or {})
        dpM = (hltM2.get("data_plane") or {})
        dpM_p = (hltM_p.get("data_plane") or {})
        dpH_p = (hltH_p.get("data_plane") or {})
        prM = (hltM2.get("punch_route") or {})
        prH = (hltH2.get("punch_route") or {})
        final = dl.get("final") or {}
        landed = [f for f in files if f["bytes"] == SAMPLE_BYTES and f["md5"] == EXPECTED_MD5]
        checks = {
            "mapping_observed_at_punch_endpoint": bool(discovered_port),
            "mapping_stable_two_samples": mapping_stable,
            "H_punch_established": punch_h.get("tcp_punch_established") is True
                                   and punch_h.get("route") == "punched-direct",
            "M_punch_established": punch_m.get("tcp_punch_established") is True
                                   and punch_m.get("route") == "punched-direct",
            "M_punched_socket_registered": punch_m.get("punched_socket_registered") is True,
            "H_punched_socket_registered": punch_h.get("punched_socket_registered") is True,
            "member_bridge_served": int(dpM.get("served_registered") or 0) > 0,
            "holder_socket_alive_after_punch": int(dpH_p.get("punched_alive") or 0) > 0,
            "member_socket_alive_after_punch": int(dpM_p.get("punched_alive") or 0) > 0,
            "direct_counters": int(prH.get("direct") or 0) > 0 and int(prM.get("direct") or 0) > 0,
            "bt_engine_attached": final.get("coldrevive.engineAttached") is True,
            "bt_download_complete": final.get("status") == "complete"
                                    and int(final.get("completedLength") or 0) == SAMPLE_BYTES,
            "bt_infohash_match": ((final.get("bittorrent") or {}).get("infoHash")
                                  == runtime_infohash),
            "bt_md5_match": bool(landed),
        }
        # 可重入登记单独成块，**不并入 PASS 判据**：一次干净的单会话传输本来就不该
        # 需要补装，把它写成必要条件等于为了让检查表全绿去逼出补装（那是造数据）。
        # 这里只如实记录「有没有补装、补装后有没有真的服务到连接」。
        report["reentrancy"] = {
            "socks5_awaiting": dpM.get("socks5_awaiting"),
            "socks5_in_use": dpM.get("socks5_in_use"),
            "socks5_rearmed": dpM.get("socks5_rearmed"),
            "socks5_served_after_rearm": dpM.get("socks5_served_after_rearm"),
            "socks5_rearm_wait_ms": dpM.get("socks5_rearm_wait_ms"),
            "repunch_attempts": len(repunch_log),
            "repunch_routes": [{"tag": e["tag"],
                                "H_route": (e.get("H") or {}).get("route"),
                                "M_route": (e.get("M") or {}).get("route"),
                                "H_reason": (e.get("H") or {}).get("reason")}
                               for e in repunch_log],
        }
        tcpdump_text = (report["steps"].get("tcpdump", {}).get("H", "")
                        + report["steps"].get("tcpdump", {}).get("M", ""))
        punch_blocked = (
            not checks["H_punch_established"] or not checks["M_punch_established"]
        ) and ("Flags [R.]" in tcpdump_text or "Connection refused" in tcpdump_text
               or "连接被拒" in tcpdump_text)
        checks["public_simultaneous_open_reachable"] = not punch_blocked
        report["checks"] = [{"name": k, "ok": bool(v)} for k, v in checks.items()]
        if all(checks.values()):
            report["verdict"] = "PASS"
            report["reason"] = ("one usable punched-direct TCP path carried a real 1 MiB BT "
                                "transfer with md5 match in this environment")
        elif not checks["mapping_observed_at_punch_endpoint"]:
            report["verdict"] = "BLOCKED"
            report["reason"] = (f"公网映射端口未能观测到：192.168.1.221:19710 在 "
                                f"{OBS119}:{HOSTS['H']['bt_public']} 上看不到发现端口")
        elif not checks["mapping_stable_two_samples"]:
            report["verdict"] = "BLOCKED"
            report["reason"] = f"同一端点两次采样不一致，拒绝外推: {ports}"
        elif punch_blocked:
            report["verdict"] = "BLOCKED"
            report["reason"] = (
                "TCP simultaneous open did not complete even though the public port is "
                "directly bindable (RST/refused in tcpdump); see tcpdump for which side "
                "answered R.")
        else:
            report["verdict"] = "FAIL"
            report["reason"] = "; ".join(k for k, v in checks.items() if not v)
    except Exception as e:  # noqa: BLE001 - keep raw evidence for every failure
        disc = report.get("steps", {}).get("discovery", {}) or {}
        report["verdict"] = "BLOCKED" if disc.get("stable") is not True else "FAIL"
        report["error"] = f"{type(e).__name__}: {e}"
    finally:
        cleanup: dict = {}

        def safe(label: str, fn) -> None:
            try:
                cleanup[label] = fn()
            except Exception as e:  # cleanup must never hide the real verdict
                cleanup[label] = {"error": f"{type(e).__name__}: {e}"}

        for node, c in (("H", cH), ("M", cM)):
            safe(UNITS[node], lambda c=c, node=node: stop_unit(c, UNITS[node]))
        for unit, c in ((UNITS["obsH"], cH), (UNITS["http"], cM),
                        (UNITS["capH"], cH), (UNITS["capM"], cM)):
            safe(unit, lambda c=c, unit=unit: stop_unit(c, unit))
        report["steps"]["cleanup"] = cleanup
        for c in (cH, cM):
            c.close()

    out = pathlib.Path(args.report_out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"report": str(out), "verdict": report.get("verdict"),
                      "reason": report.get("reason") or report.get("error"),
                      "checks": report.get("checks")}, ensure_ascii=False))
    return 0 if report.get("verdict") == "PASS" else 2


if __name__ == "__main__":
    raise SystemExit(main())
