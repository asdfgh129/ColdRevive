#!/usr/bin/env python3
"""W6.7 daemon-level BT over punched TCP (public, cross-network).

Topology (2026-11-27):
- H = 119.29.150.254, public 1:1 EIP, role=holder, engine.listen=127.0.0.1:19730.
      Punch socket binds 0.0.0.0:19720; public TCP/22 is temporarily REDIRECTed
      from the home egress to 19720 for the punch window only.
- M = 192.168.1.221, home NAT (EIM proven by two observers), role=member.
      Punch socket binds 192.168.1.221:19710; discovered public port is signalled
      to H and used immediately as the SYN target.
- Observers = two ports on 82.157.68.159 (22 and 7100), temporarily REDIRECTed
      from the home egress to local coldrevive-nat observers during discovery.
      Cross-IP EIM for 221 is additionally supported by the archived two-IP
      tcpdump evidence; this run re-proves port-independence immediately.
- Relay 82.157.68.159:7100 is used only for ADR-022 two-phase verdict control,
      never as the BT data plane.

PASS needs all of: two-observer EIM, both sides poll+SO_ERROR punch established,
both sides route=punched-direct, real SOCKS5 bridge registration, real BT
handshake/piece transfer, 1 MiB landed with md5, and holder splice counters > 0.
No success rate. No S6/G2 claim. No relay data-plane substitution.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import pathlib
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

HOSTS = {
    "H": {"ssh": OBS119, "node": "H", "http": 19711, "wake": 19712,
          "bt_public": 22, "workdir": "/root/cr-w67/holder"},
    "M": {"ssh": "192.168.1.221", "node": "M", "http": 19721, "wake": 19722,
          "bt_public": 19910, "workdir": "/root/cr-w67/member"},
}
UNITS = {
    "H": "cr-w67-h", "M": "cr-w67-m",
    "obs82a": "cr-w67-obs82a", "obs82b": "cr-w67-obs82b",
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


def iptables_redirect(c: paramiko.SSHClient, action: str, dport: int, to_port: int) -> dict:
    rule = (f"-t nat -{action} PREROUTING -p tcp -s {HOME_EGRESS} --dport {dport} "
            f"-m state --state NEW -j REDIRECT --to-ports {to_port}")
    return run(c, "iptables " + rule)


def punch(c: paramiko.SSHClient, node: str, peer: str, bind_hint: str,
          discovered_port: int, nonce: str) -> dict:
    port = HOSTS[node]["http"]
    args = ["punch-route", peer, bind_hint, str(discovered_port), nonce]
    r = cli(c, port, "nat", args, timeout=90)
    return {"node": node, "peer": peer, "bind_hint": bind_hint,
            "discovered_port_argument": discovered_port, "args": args,
            "result": r.get("json"), "raw": r}


def capture(c: paramiko.SSHClient, unit: str, log: str, bpf: str) -> dict:
    script = f"timeout 25 tcpdump -nn -i any -l '{bpf}' > {log} 2>&1"
    cmd = f"/bin/bash -lc \"{script}\""
    return start_unit(c, unit, "/root/cr-w67", cmd)


def wait_download(c: paramiko.SSHClient, secret: str, gid: str, seconds: int = 90) -> dict:
    deadline = time.time() + seconds
    last, samples = {}, []
    while time.time() < deadline:
        r = rpc(c, HOSTS["M"]["http"], secret, "aria2.tellStatus", [gid])
        last = (r.get("json") or {}).get("result") or {}
        samples.append({"status": last.get("status"),
                        "completed": last.get("completedLength"),
                        "connections": last.get("connections")})
        if last.get("status") in ("complete", "error"):
            break
        time.sleep(0.4)
    return {"final": last, "samples_tail": samples[-12:]}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--report-out", required=True)
    ap.add_argument("--punch-timeout-ms", type=int, default=8000)
    args = ap.parse_args()

    report: dict = {
        "schema": "coldrevive.w67.punched-tcp-bt/v1",
        "generated_at": dt.datetime.now().astimezone().isoformat(timespec="seconds"),
        "scope": "daemon-level punched-direct TCP carrying real BT payload; component evidence, not G2/S6",
        "topology": {
            "H": "119.29.150.254 public EIP, holder, engine.listen=127.0.0.1:19730, punch bind 19720",
            "M": "192.168.1.221 home EIM NAT, member, punch bind 192.168.1.221:19710",
            "observers": [OBS82 + ":22", OBS82 + ":7100"],
            "observer_note": "two observer ports on one public host; cross-IP EIM corroborated by archived tcpdump evidence",
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

    cH, cM, c82 = client(HOSTS["H"]["ssh"]), client(HOSTS["M"]["ssh"]), client(OBS82)
    for c in (cH, cM, c82):
        run(c, "mkdir -p /root/cr-w67 /root/cr-w67/holder /root/cr-w67/member")
    obs_rules_added = {"82": False, "119": False}
    punch_rule_added = False
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
        install_binary(c82, nat_data, "/root/cr-w67/coldrevive-nat", nat_sha)

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
            run(c, f"mkdir -p {wd}/downloader {wd}/meta {wd}/downloads /root/cr-w67")
            put(c, wd + "/config.json", daemon_config(node, "M" if node == "H" else "H", secret))
            put(c, wd + "/peer-holder.json", advertisement("H"))
            put(c, wd + "/peer-member.json", advertisement("M"))
            put(c, wd + "/req.json", "{}\n")
            put(c, wd + "/ack.json", "{}\n")

        # 1) two observer ports on 82 + public-22-to-punch-socket on 119.
        #    119:22 is NEVER an observer here, so its REDIRECT stays stable while
        #    the NAT mapping is discovered and consumed. This removes the earlier
        #    failure where the H-side mapping changed during rule swap.
        obs_start = {}
        for unit, port in ((UNITS["obs82a"], 19110), (UNITS["obs82b"], 19111)):
            obs_start[unit] = start_unit(
                c82, unit, "/root/cr-w67",
                f"/root/cr-w67/coldrevive-nat --serve-observer 0.0.0.0:{port} "
                f"--observer-secs 180 --json")
        report["steps"]["observers"] = obs_start
        add22 = iptables_redirect(c82, "A", 22, 19110)
        add7100 = iptables_redirect(c82, "A", 7100, 19111)
        pr = iptables_redirect(cH, "A", 22, 19720)
        punch_rule_added = pr["rc"] == 0
        obs_rules_added = {"22": add22["rc"] == 0, "7100": add7100["rc"] == 0}
        report["steps"]["redirect_add"] = {"82_22": add22, "82_7100": add7100,
                                            "119_22_to_19720": pr}
        if not punch_rule_added or not all(obs_rules_added.values()):
            raise RuntimeError("observer/punch REDIRECT 添加失败")

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
                       "host 112.5.60.4 or tcp port 22 or tcp port 19720")
        capM = capture(cM, UNITS["capM"], "/root/cr-w67/tcpdump-m.log",
                       "host 119.29.150.254 or tcp port 22 or tcp port 19710")
        report["steps"]["capture_start"] = {"H": capH, "M": capM}

        # 3) discover and consume the mapping immediately; no daemon restart or
        #    peer load between discovery and punch.
        disc = run(cM, f"/root/cr-w67/coldrevive-nat --discover-tcp-port "
                       f"192.168.1.221:19710 {OBS82}:22,{OBS82}:7100 "
                       f"--timeout 3000 --json", 40)
        discovery = parse_json_line(disc["stdout"])
        report["steps"]["discovery"] = {"raw": disc, "parsed": discovery}
        discovered_port = int(discovery["discovered_port"]) if discovery and discovery.get("discovered_port") else 0
        if not discovery or discovery.get("eim") is not True or not discovered_port:
            raise RuntimeError(f"two-observer EIM unavailable: {discovery}")
        # Restore relay 7100 before the two-phase verdict. 119:22 is untouched.
        del7100 = iptables_redirect(c82, "D", 7100, 19111)
        del22 = iptables_redirect(c82, "D", 22, 19110)
        report["steps"]["observer_redirect_del"] = {"82_22": del22, "82_7100": del7100}
        obs_rules_added = {"22": False, "7100": False}
        stop_unit(c82, UNITS["obs82a"])
        stop_unit(c82, UNITS["obs82b"])

        # 4) simultaneous punch with shared nonce.
        nonce = secrets.token_hex(16)
        barrier = threading.Barrier(2)
        result_h: dict = {}

        def do_m() -> None:
            cm = client(HOSTS["M"]["ssh"])
            try:
                barrier.wait()
                result_h["M"] = punch(cm, "M", "H",
                                      "192.168.1.221:19710", 22, nonce)
            finally:
                cm.close()

        t = threading.Thread(target=do_m, daemon=True)
        t.start()
        barrier.wait()
        try:
            result_h["H"] = punch(cH, "H", "M", "0.0.0.0:19720", discovered_port, nonce)
        finally:
            t.join(timeout=100)
        report["steps"]["punch"] = result_h
        time.sleep(1.0)

        tcpdump_h = run(cH, "cat /root/cr-w67/tcpdump-h.log 2>/dev/null || true")
        tcpdump_m = run(cM, "cat /root/cr-w67/tcpdump-m.log 2>/dev/null || true")
        report["steps"]["tcpdump"] = {"H": tcpdump_h["stdout"][-6000:],
                                       "M": tcpdump_m["stdout"][-6000:]}

        # 5) real BT payload over the punched socket.
        #    The runtime infohash is derived from the holder's real seed index:
        #    fixed INFOHASH is historical and changes with keywords/namespace.
        state_raw = get(cH, HOSTS["H"]["workdir"] + "/state-H.json")
        state = json.loads(state_raw.decode("utf-8"))
        entry = next((e for e in state.get("entries", [])
                      if e.get("file_hash") == FILE_HASH), None)
        runtime_infohash = (entry or {}).get("infohash", "")
        report["runtime_infohash"] = runtime_infohash
        if not runtime_infohash:
            raise RuntimeError("holder seed index has no runtime infohash")
        # `wake ack` here only confirms the metadata handshake; the data plane
        # remains the already-registered punched socket, not a relay tunnel.
        ack = cli(cM, HOSTS["M"]["http"], "wake", ["ack", FILE_HASH, runtime_infohash])
        report["steps"]["wake_ack_for_torrent_metadata"] = ack.get("json")
        torrent_path = "/root/cr-w67/holder/sample.torrent"
        tf = run(cH, f"curl -sS --max-time 10 -o {torrent_path} "
                     f"http://127.0.0.1:{HOSTS['H']['http']}/torrent/{runtime_infohash} && "
                     f"wc -c {torrent_path}")
        report["steps"]["torrent_fetch_on_holder"] = tf
        torrent = get(cH, torrent_path)
        if tf["rc"] != 0 or len(torrent) < 100 or b"4:info" not in torrent:
            raise RuntimeError(f"holder torrent invalid: rc={tf['rc']} bytes={len(torrent)}")
        put(cM, "/root/cr-w67/member/sample.torrent", torrent, 0o644)
        http_start = start_unit(cM, UNITS["http"], "/root/cr-w67/member",
                                "python3 -m http.server 19880 --bind 127.0.0.1")
        report["steps"]["torrent_http"] = {"start": http_start, "bytes": len(torrent)}
        time.sleep(1.0)
        add = rpc(cM, HOSTS["M"]["http"], secret, "aria2.addUri",
                  [["http://127.0.0.1:19880/sample.torrent"]])
        gid = ((add.get("json") or {}).get("result") or "")
        report["steps"]["add_uri"] = add
        if not gid:
            raise RuntimeError("aria2.addUri no gid")
        dl = wait_download(cM, secret, gid)
        report["steps"]["download"] = dl

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

        punch_h = (result_h["H"].get("result") or {})
        punch_m = (result_h["M"].get("result") or {})
        dpM = (hltM2.get("data_plane") or {})
        hdH = (hltH2.get("holder_data") or {})
        prM = (hltM2.get("punch_route") or {})
        prH = (hltH2.get("punch_route") or {})
        final = dl.get("final") or {}
        landed = [f for f in files if f["bytes"] == SAMPLE_BYTES and f["md5"] == EXPECTED_MD5]
        checks = {
            "two_observer_eim": discovery.get("eim") is True,
            "H_punch_established": punch_h.get("tcp_punch_established") is True
                                   and punch_h.get("route") == "punched-direct",
            "M_punch_established": punch_m.get("tcp_punch_established") is True
                                   and punch_m.get("route") == "punched-direct",
            "M_punched_socket_registered": punch_m.get("punched_socket_registered") is True,
            "H_punched_socket_registered": punch_h.get("punched_socket_registered") is True,
            "member_bridge_served": int(dpM.get("served_registered") or 0) > 0,
            "member_punched_alive": int(dpM.get("punched_alive") or 0) > 0,
            "holder_splice_done": int(hdH.get("splices_done") or 0) > 0,
            "direct_counters": int(prH.get("direct") or 0) > 0 and int(prM.get("direct") or 0) > 0,
            "bt_engine_attached": final.get("coldrevive.engineAttached") is True,
            "bt_download_complete": final.get("status") == "complete"
                                    and int(final.get("completedLength") or 0) == SAMPLE_BYTES,
            "bt_infohash_match": ((final.get("bittorrent") or {}).get("infoHash")
                                  == runtime_infohash),
            "bt_md5_match": bool(landed),
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
        elif checks["two_observer_eim"] is False:
            report["verdict"] = "BLOCKED"
            report["reason"] = "two-observer EIM port unavailable; no port extrapolated"
        elif punch_blocked:
            report["verdict"] = "BLOCKED"
            report["reason"] = (
                "public side only exposes TCP/22; source-scoped REDIRECT 22->19720 "
                "cannot complete TCP simultaneous open (RST/refused observed). "
                "Need a free public TCP port directly bound by the punch socket.")
        else:
            report["verdict"] = "FAIL"
            report["reason"] = "; ".join(k for k, v in checks.items() if not v)
    except Exception as e:  # noqa: BLE001 - keep raw evidence for every failure
        report["verdict"] = "BLOCKED" if report.get("steps", {}).get("discovery", {}).get("parsed", {}).get("eim") is not True else "FAIL"
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
        for unit, c in ((UNITS["obs82a"], c82), (UNITS["obs82b"], c82),
                        (UNITS["http"], cM), (UNITS["capH"], cH), (UNITS["capM"], cM)):
            safe(unit, lambda c=c, unit=unit: stop_unit(c, unit))
        if punch_rule_added:
            safe("punch_redirect_del", lambda: iptables_redirect(cH, "D", 22, 19720))
        if obs_rules_added.get("22"):
            safe("observer_redirect_del_22", lambda: iptables_redirect(c82, "D", 22, 19110))
        if obs_rules_added.get("7100"):
            safe("observer_redirect_del_7100", lambda: iptables_redirect(c82, "D", 7100, 19111))
        report["steps"]["cleanup"] = cleanup
        for c in (cH, cM, c82):
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
