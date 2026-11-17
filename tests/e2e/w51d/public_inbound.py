#!/usr/bin/env python3
"""W5.1d 验收：公网入向 —— 外部主机越过互联网从 A 的引擎取片。

和 W5.1c（LAN 跨主机）的区别只有一条，但决定性质不同：**路径必须经过公网**。
222 同时挂在 192.168.1.0/24 上，所以「能下载」本身不构成证据——它完全可能走 LAN。
本脚本因此同时要求两条相反方向的证据：
    ① 客户端侧：种子里只有 **公网** announce，peer 地址由 tracker 按公网请求方返回；
    ② 供片侧：A 的 bt_port 上那条已建立连接的对端地址是 **公网 IP**（不是 192.168.x）。
两条都成立才算走公网；只满足其一即判失败并写明是哪一侧露了 LAN。

前置条件是运维侧在 CPE 上真实建好两条 TCP 端口映射（A 的路由器）：
    <公网IP>:<bt_port>   → 192.168.1.210:<bt_port>
    <公网IP>:<http_port> → 192.168.1.210:<http_port>
本机自己无法验证映射是否存在，只能**从外部探测**——所以第 0 步就是
让 222 去 connect 这两个端口，连不上直接 BLOCKED，并打印需要的规则。
绝不因为「本机绑定成功」就认为入向可用。

边界（写进报告）：
    - `evidence_scope=public-inbound-single-egress`、`is_s6_evidence=false`；
    - 只有一个出口（A 的家宽），不含任何可达率数字，不构成 S6/G2 证据；
    - µTP 仍不支持：客户端显式关掉（librqbit 只做 TCP）。

用法：
    python tests/e2e/w51d/public_inbound.py
    python tests/e2e/w51d/public_inbound.py --peer 192.168.1.222 --public-ip 112.5.60.4
    python tests/e2e/w51d/public_inbound.py --probe-only     # 只测端口映射，不起客户端
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import re
import socket
import subprocess
import sys
import threading
import time
import urllib.parse
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
SAMPLE_BYTES = 4 << 20
PIECE_SIZE = 262144
COLD_PROBE_SECONDS = 10
DOWNLOAD_TIMEOUT = 240.0

A_LAN = "192.168.1.210"
PUBLIC_HTTP = 17800
PUBLIC_BT = 17802
B_HTTP = 17900
B_WAKE = 17901
B_BT = 17902

TX_RPC_PORT = 19093
TX_PEER_PORT = 19503
REMOTE_ROOT = "/tmp/cr-pub-w51d"


# ---------------------------------------------------------------------------
# 基础工具
# ---------------------------------------------------------------------------
def run(cmd: list[str], timeout: float = 60.0) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, capture_output=True, timeout=timeout)


def shquote(s: str) -> str:
    return "'" + s.replace("'", "'\\''") + "'"


class Peer:
    """对端（外部主机）。sshd 偶发 MaxStartups 拒绝，故每次连接都重试。"""

    def __init__(self, host: str, key: str, tries: int = 4) -> None:
        self.host, self.key, self.tries = host, key, tries

    def __call__(self, cmd: str, timeout: float = 60.0) -> tuple[int, str]:
        base = ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10",
                "-o", "StrictHostKeyChecking=no", "-o", "ServerAliveInterval=5",
                "-i", self.key, f"root@{self.host}"]
        last = (1, "")
        for i in range(self.tries):
            try:
                p = run(base + [cmd], timeout=timeout)
                out = (p.stdout + p.stderr).decode("utf-8", "replace")
                # MaxStartups 是 sshd 的瞬时拒绝，不是命令失败
                if "Exceeded MaxStartups" in out or "kex_exchange_identification" in out:
                    last = (1, out)
                    time.sleep(3 + i * 2)
                    continue
                return p.returncode, out
            except subprocess.TimeoutExpired:
                last = (1, "ssh 超时")
                time.sleep(2)
        return last


def deterministic_sample(n: int) -> bytes:
    out = bytearray()
    seed = b"coldrevive-w51d-public-sample"
    while len(out) < n:
        seed = hashlib.sha256(seed).digest()
        out.extend(seed * 8)
    return bytes(out[:n])


def http_json(url: str, payload: dict, timeout: float = 40.0) -> dict:
    body = json.dumps(payload).encode()
    req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode())


def http_get(url: str, timeout: float = 10.0) -> tuple[int, bytes]:
    try:
        with urllib.request.urlopen(url, timeout=timeout) as r:
            return r.status, r.read()
    except Exception as e:
        code = getattr(e, "code", 0)
        return code, b""


def tcp_probe(host: str, port: int, timeout: float = 6.0) -> tuple[bool, str]:
    s = socket.socket()
    s.settimeout(timeout)
    try:
        s.connect((host, port))
        return True, "connected"
    except Exception as e:
        return False, f"{type(e).__name__}: {e}"
    finally:
        s.close()


# ---- 最小 bencode 处理（只为把 info 字典**原样**搬到另一个 announce 下） ----
def skip_bencode(b: bytes, i: int) -> int:
    c = b[i:i + 1]
    if c == b"i":
        j = b.index(b"e", i)
        return j + 1
    if c == b"l" or c == b"d":
        i += 1
        while b[i:i + 1] != b"e":
            i = skip_bencode(b, i)
        return i + 1
    j = b.index(b":", i)
    n = int(b[i:j])
    return j + 1 + n


def info_slice(raw: bytes) -> bytes:
    """取出 `4:info` 后那个字典的**原始字节**（连同 d..e）。"""
    pos = raw.find(b"4:info")
    if pos < 0:
        raise ValueError("种子里没有 4:info")
    start = pos + len(b"4:info")
    return raw[start:skip_bencode(raw, start)]


def bstr(s: bytes) -> bytes:
    return str(len(s)).encode() + b":" + s


def torrent_with_announce(raw: bytes, announce: str) -> bytes:
    """同 info、单一 announce 的变体。InfoHash 因此**必然**不变（下方会复核）。"""
    info = info_slice(raw)
    return b"d" + bstr(b"announce") + bstr(announce.encode()) + bstr(b"info") + info + b"e"


def infohash_of(raw: bytes) -> str:
    return hashlib.sha1(info_slice(raw)).hexdigest()


def sha1_hex(b: bytes) -> str:
    return hashlib.sha1(b).hexdigest()


def parse_tx_percent(listing: str) -> int:
    for line in listing.splitlines():
        parts = line.split()
        if not parts or not parts[0].isdigit() or len(parts) < 2:
            continue
        m = re.match(r"([\d.]+)%", parts[1])
        if m:
            return int(float(m.group(1)))
    return -1


def established_peers(local_port: int) -> list[str]:
    """A 侧 bt_port 上已建立连接的远端地址。读系统连接表，不靠引擎自报。

    用 netstat 而不是 Get-NetTCPConnection：后者每次要拉起 PowerShell（~1s），
    而 4 MiB 的公网下载只持续几秒，采样太慢会直接错过那条连接，
    把「走的是公网」误判成「没连上」。
    """
    try:
        p = subprocess.run(["netstat", "-ano"], capture_output=True, timeout=15)
        out = p.stdout.decode("utf-8", "replace")
    except Exception:
        return []
    found = []
    for ln in out.splitlines():
        parts = ln.split()
        if len(parts) < 4 or parts[3].upper() != "ESTABLISHED":
            continue
        local, remote = parts[1], parts[2]
        if not local.endswith(f":{local_port}"):
            continue
        found.append(remote.rsplit(":", 1)[0])
    return found


def is_private(ip: str) -> bool:
    return (ip.startswith("10.") or ip.startswith("127.") or ip.startswith("192.168.")
            or ip.startswith("169.254.") or re.match(r"^172\.(1[6-9]|2\d|3[01])\.", ip) is not None)


def wan_ip_of_self() -> str:
    """A 的 WAN 地址：用真实 STUN 探测（不读配置、不猜）。"""
    exe = REPO / "target" / "debug" / ("coldrevive-nat.exe" if os.name == "nt" else "coldrevive-nat")
    if exe.is_file():
        p = subprocess.run([str(exe), "--json", "--timeout", "1500"], capture_output=True, timeout=90)
        try:
            j = json.loads(p.stdout.decode("utf-8", "replace"))
            m = j.get("mapped_address") or ""
            if ":" in m:
                return m.rsplit(":", 1)[0]
        except Exception:
            pass
    return ""


# ---------------------------------------------------------------------------
# 主流程
# ---------------------------------------------------------------------------
def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--peer", default=os.environ.get("CR_PEER", "192.168.1.222"))
    ap.add_argument("--public-ip", default="", help="A 的 WAN 地址；缺省用 STUN 实测")
    ap.add_argument("--probe-only", action="store_true", help="只验证端口映射，不跑客户端")
    args = ap.parse_args()

    key = os.environ.get("CR_SSH_KEY") or str(
        Path(os.environ.get("TEMP", "/tmp")) / "crkeys" / "id_ed25519")
    daemon = os.environ.get("COLDREVIVE_DAEMON") or str(
        REPO / "target" / "debug" / ("coldrevived.exe" if os.name == "nt" else "coldrevived"))
    if not Path(daemon).is_file():
        print(f"[BLOCKED] 找不到守护进程 {daemon}")
        return 2

    work = Path(REPO / "tests" / "e2e" / "w51d" / "reports" / f"run-{time.strftime('%Y%m%dT%H%M%SZ')}")
    work.mkdir(parents=True, exist_ok=True)
    report: dict = {
        "schema": "coldrevive.w51d.public-inbound/v1",
        "generated_at_unix": int(time.time()),
        "evidence_scope": "public-inbound-single-egress",
        "is_s6_evidence": False,
        "contains_success_rate": False,
        "seeder": f"cr-engine / librqbit（A @ {A_LAN}）",
        "external_host": args.peer,
        "ports": {"http": PUBLIC_HTTP, "bt": PUBLIC_BT},
        "checks": [],
        "phases": {},
    }

    def check(label: str, ok: bool, actual: str) -> bool:
        report["checks"].append({"label": label, "ok": bool(ok), "actual": actual})
        print(f"  [{'PASS' if ok else 'FAIL'}] {label} — {actual}")
        return ok

    def finish(status: str, code: int) -> int:
        report["status"] = status
        (work / "report.json").write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(f"\n报告: {work / 'report.json'}")
        return code

    peer = Peer(args.peer, key)
    procs: list[subprocess.Popen] = []
    logs: list = []
    try:
        # ---------------- 0. 预检 ----------------
        print("== 0. 预检 ==")
        rc, out = peer("echo ok; command -v transmission-daemon transmission-remote curl sha1sum")
        if rc != 0 or "ok" not in out:
            print(f"[BLOCKED] 无法登录外部主机 {args.peer}: {out[:300]}")
            report["blocked_reason"] = "external host unreachable via ssh"
            return finish("blocked", 2)
        tools = {n: (n in out) for n in ("transmission-daemon", "transmission-remote")}
        report["peer_tools"] = tools
        peer_wan = ""
        rc, out = peer("curl -s --max-time 8 http://ifconfig.me")
        if rc == 0:
            m = re.search(r"\b(\d{1,3}(?:\.\d{1,3}){3})\b", out)
            peer_wan = m.group(1) if m else ""
        report["external_host_wan_ip"] = peer_wan

        wan = args.public_ip or wan_ip_of_self()
        if not wan:
            print("[BLOCKED] 取不到 A 的 WAN 地址（STUN 无响应）")
            report["blocked_reason"] = "no wan ip"
            return finish("blocked", 2)
        report["a_wan_ip"] = wan
        print(f"  A 的 WAN 地址（实测）= {wan}；外部主机 = {args.peer}（WAN {peer_wan or '未知'}）")

        # ---------------- 1. 起 A（公网档）+ B（信令） ----------------
        print("== 1. 启动 A（公网映射档）与 B ==")
        sample_name = "coldrevive-w51d-public-sample.bin"
        sample = work / sample_name
        data = deterministic_sample(SAMPLE_BYTES)
        sample.write_bytes(data)
        sample_sha1 = sha1_hex(data)
        report["sample"] = {"name": sample_name, "bytes": SAMPLE_BYTES, "sha1": sample_sha1}

        cfg = {
            "bind": "0.0.0.0",
            "nodes": {
                "A": {"host": A_LAN, "http_port": PUBLIC_HTTP,
                      "wake_udp_port": PUBLIC_HTTP + 1, "bt_port": PUBLIC_BT},
                "B": {"host": "127.0.0.1", "http_port": B_HTTP,
                      "wake_udp_port": B_WAKE, "bt_port": B_BT},
            },
            "sample_path": str(sample),
            "keywords": ["w51d-public"],
            "namespace": "ColdRevive_V1:",
            "workdir": str(work),
            "golden_req": str(work / "wakeup_req.v1.json"),
            "golden_ack": str(work / "wakeup_ack.v1.json"),
            "downloader": "",
            "challenge_window_sec": 300,
            "nat": {
                "enabled": True,
                "bt_external": f"{wan}:{PUBLIC_BT}",
                "http_external": f"{wan}:{PUBLIC_HTTP}",
            },
        }
        cfg_path = work / "w51d-nodes.json"
        cfg_path.write_text(json.dumps(cfg, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

        for node in ("A", "B"):
            lf = open(work / f"daemon-{node}.log", "wb")
            logs.append(lf)
            procs.append(subprocess.Popen(
                [daemon, "--config", str(cfg_path), "--node-id", node,
                 "--bind", "0.0.0.0", "--workdir", str(work)],
                stdout=lf, stderr=subprocess.STDOUT, cwd=str(work)))

        ok = False
        for _ in range(60):
            code, _b = http_get(f"http://127.0.0.1:{PUBLIC_HTTP}/health", timeout=2)
            if code == 200:
                ok = True
                break
            time.sleep(0.5)
        if not check("A 本机 /health", ok, f"http://127.0.0.1:{PUBLIC_HTTP}/health"):
            return finish("fail", 1)

        pub = {}
        code, body = http_get(f"http://127.0.0.1:{PUBLIC_HTTP}/nat/public", timeout=5)
        if code == 200:
            pub = json.loads(body.decode())
        report["nat_public"] = pub
        check("A 报告的引擎监听是 0.0.0.0（外部包才进得来）",
              str(pub.get("engine_listen", "")).startswith("0.0.0.0:"),
              str(pub.get("engine_listen")))
        check("种子 announce 同时含 LAN 与公网两条",
              len(pub.get("announce") or []) == 2
              and any(wan in u for u in (pub.get("announce") or [])),
              json.dumps(pub.get("announce"), ensure_ascii=False))
        check("A 不声称入向已验证（本机无法自证）",
              pub.get("inbound_verified") is False,
              f"inbound_verified={pub.get('inbound_verified')}")

        # ---------------- 2. 外部端口映射探测（唯一的 BLOCKED 判据） ----------------
        print("== 2. 从外部主机探测公网端口 ==")
        probe = {}
        for name, port in (("http", PUBLIC_HTTP), ("bt", PUBLIC_BT)):
            rc, out = peer(
                "python3 - <<'PYEOF'\n"
                "import socket\n"
                f"s=socket.socket(); s.settimeout(6)\n"
                "try:\n"
                f"    s.connect(('{wan}',{port})); print('OPEN')\n"
                "except Exception as e:\n"
                "    print('CLOSED', type(e).__name__, e)\n"
                "finally:\n"
                "    s.close()\n"
                "PYEOF")
            probe[name] = out.strip()
            print(f"  {wan}:{port} → {out.strip()}")
        report["phases"]["external_port_probe"] = probe
        http_open = "OPEN" in probe.get("http", "")
        bt_open = "OPEN" in probe.get("bt", "")
        check("外部主机能连上公网 HTTP 端口", http_open, probe.get("http", "")[:80])
        check("外部主机能连上公网 BT 端口", bt_open, probe.get("bt", "")[:80])

        if not (http_open and bt_open):
            report["blocked_reason"] = "no port mapping on CPE"
            report["required_cpe_rules"] = [
                f"TCP {wan}:{PUBLIC_HTTP} -> {A_LAN}:{PUBLIC_HTTP}",
                f"TCP {wan}:{PUBLIC_BT} -> {A_LAN}:{PUBLIC_BT}",
            ]
            print("\n[BLOCKED] A 的路由器上没有这两条映射，公网入向不可能成立。")
            print("          需要在 CPE（光猫/路由器）里添加：")
            for r in report["required_cpe_rules"]:
                print(f"            {r}")
            print("          本机已经确认：UPnP / NAT-PMP / PCP 都不可用，无法自动建映射。")
            return finish("blocked", 2)

        if args.probe_only:
            print("\n--probe-only：端口映射已通，跳过客户端复核。")
            return finish("pass", 0)

        if not (tools["transmission-daemon"] and tools["transmission-remote"]):
            print("[BLOCKED] 外部主机缺少 transmission-daemon / transmission-remote")
            report["blocked_reason"] = "peer missing transmission"
            return finish("blocked", 2)

        # ---------------- 3. 外部取种子 + tracker 决策 ----------------
        print("== 3. 外部主机经公网取种子并 announce ==")
        rc, hout = peer(f"curl -sS -m 8 -w ' %{{http_code}}' http://{wan}:{PUBLIC_HTTP}/health")
        report["phases"]["public_health"] = hout.strip()
        check("外部主机经公网访问 A 的 /health",
              hout.strip().endswith("200") and '"ok"' in hout,
              hout.strip()[:120])

        state = json.loads((work / "state-A.json").read_text(encoding="utf-8"))
        entry = next((e for e in (state.get("entries") or []) if e.get("infohash")), None)
        if not entry:
            print("[FAIL] state-A.json 里没有带 infohash 的条目")
            return finish("fail", 1)
        infohash, file_hash = entry["infohash"], entry.get("file_hash") or ""
        report["infohash"] = infohash

        rc, out = peer(f"curl -sS -m 15 -o {REMOTE_ROOT}.torrent -w '%{{http_code}} %{{size_download}}' "
                       f"http://{wan}:{PUBLIC_HTTP}/torrent/{infohash}; "
                       f"mkdir -p {REMOTE_ROOT}")
        parts = out.split()
        report["phases"]["public_torrent_fetch"] = out.strip()
        check("外部主机经公网取回伴随种子（真实字节）",
              bool(parts) and parts[0] == "200" and len(parts) > 1 and int(parts[1]) > 0,
              out.strip())

        raw = b""
        rc, b64out = peer(f"base64 -w0 {REMOTE_ROOT}.torrent")
        try:
            raw = base64.b64decode(re.sub(r"[^A-Za-z0-9+/=]", "", b64out))
        except Exception:
            raw = b""
        pub_announce = ""
        m = re.search(rb"8:announce(\d+):", raw[:400])
        if m:
            n = int(m.group(1))
            pub_announce = raw[m.end():m.end() + n].decode("latin-1", "replace")
        lan_announce = f"http://{A_LAN}:{PUBLIC_HTTP}/announce"
        pub_url = f"http://{wan}:{PUBLIC_HTTP}/announce"
        text = raw.decode("latin-1", "replace")
        report["torrent_announces"] = {"primary": pub_announce,
                                       "has_lan": lan_announce in text,
                                       "has_public": pub_url in text}
        check("种子带 BEP 12 announce-list（LAN + 公网）",
              lan_announce in text and pub_url in text,
              json.dumps(report["torrent_announces"], ensure_ascii=False))

        # tracker 对公网请求方必须回公网端点
        ih_param = urllib.parse.quote_from_bytes(bytes.fromhex(infohash))
        q = (f"info_hash={ih_param}"
             f"&peer_id={urllib.parse.quote_from_bytes(b'cr-w51d-probe-000000000')}"
             f"&port={TX_PEER_PORT}&uploaded=0&downloaded=0&left={SAMPLE_BYTES}"
             f"&compact=1&numwant=20")
        rc, hexout = peer(f"curl -sS -m 8 'http://{wan}:{PUBLIC_HTTP}/announce?{q}' "
                          f"| xxd -p | tr -d '\\n'")
        raw_hex = re.sub(r"[^0-9a-f]", "", hexout)
        peer_ip, peer_port = "", 0
        marker = "353a7065657273363a"     # "5:peers6:"
        idx = raw_hex.find(marker)
        if idx >= 0:
            blob = bytes.fromhex(raw_hex[idx + len(marker): idx + len(marker) + 12])
            peer_ip = ".".join(str(b) for b in blob[:4])
            peer_port = int.from_bytes(blob[4:], "big")
        report["tracker"] = {"peer_ip": peer_ip, "peer_port": peer_port, "raw_prefix": raw_hex[:120]}
        check("tracker 对公网请求方通告公网 peer（不是 LAN 地址）",
              peer_ip == wan and peer_port == PUBLIC_BT,
              f"{peer_ip}:{peer_port}（期望 {wan}:{PUBLIC_BT}）")

        # ---------------- 4. 客户端只需公网 announce 的变体 ----------------
        # 外部主机同时挂在 LAN 上，若种子保留 LAN 那条 announce，它可能直接走内网，
        # 「下载成功」就变成了 LAN 捷径。这里把同一个 info 字典搬到只有公网 announce
        # 的壳里（InfoHash 因此不变），把路径钉死在公网上。
        pub_only = torrent_with_announce(raw, pub_url)
        check("公网版种子 InfoHash 与 A 的服务内容一致",
              infohash_of(pub_only) == infohash, f"{infohash_of(pub_only)} vs {infohash}")
        (work / "peer-public-only.torrent").write_bytes(pub_only)
        b64 = base64.b64encode(pub_only).decode()
        peer(f"mkdir -p {REMOTE_ROOT}/tx/dl && base64 -d > {REMOTE_ROOT}/peer.torrent <<'B64'\n{b64}\nB64")

        tx_conf, tx_dl = f"{REMOTE_ROOT}/tx", f"{REMOTE_ROOT}/tx/dl"
        rc, out = peer(f"pkill -f '[t]ransmission-daemon -g {REMOTE_ROOT}' ; rm -rf {REMOTE_ROOT}/tx ; "
                       f"mkdir -p {tx_conf} {tx_dl}")
        peer("cat > " + f"{tx_conf}/settings.json <<'TXCFG'\n"
             + json.dumps({
                 "utp-enabled": False, "dht-enabled": False, "pex-enabled": False,
                 "lpd-enabled": False, "port-forwarding-enabled": False, "encryption": 0,
                 "rpc-enabled": True, "rpc-port": TX_RPC_PORT,
                 "rpc-bind-address": "127.0.0.1", "rpc-whitelist-enabled": False,
                 "rpc-authentication-required": False,
                 "peer-port": TX_PEER_PORT, "peer-port-random-on-start": False,
                 "download-dir": tx_dl, "incomplete-dir-enabled": False,
                 "blocklist-enabled": False, "alt-speed-enabled": False,
             }, indent=2) + "\nTXCFG\n")
        peer(f"setsid nohup transmission-daemon -g {tx_conf} >{REMOTE_ROOT}/tx.log 2>&1 & "
             f"sleep 3; transmission-remote 127.0.0.1:{TX_RPC_PORT} -l")
        rc, out = peer(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} --add {REMOTE_ROOT}/peer.torrent")
        report["phases"]["tx_add"] = out.strip()
        time.sleep(2)
        rc, listing = peer(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} -l")
        report["phases"]["tx_cold_list"] = listing.strip()

        time.sleep(COLD_PROBE_SECONDS)
        rc, listing = peer(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} -l")
        cold_pct = parse_tx_percent(listing)
        cold_seed = (http_json(f"http://127.0.0.1:{PUBLIC_HTTP}/cli",
                               {"command": "seed", "args": []}) or {}).get("seed") or {}
        report["phases"]["cold"] = {"client_percent": cold_pct, "seed": cold_seed}
        check("COLD 负对照：唤醒前公网客户端拿不到数据",
              cold_pct == 0 and cold_seed.get("state") == "cold"
              and int(cold_seed.get("served_bytes") or 0) == 0,
              f"client_done={cold_pct}%, state={cold_seed.get('state')}, "
              f"served={cold_seed.get('served_bytes')}")

        # ---------------- 5. 唤醒并下载 ----------------
        print("== 5. 唤醒 A，等公网客户端取完 ==")
        t0 = time.time()
        ack = http_json(f"http://127.0.0.1:{B_HTTP}/cli",
                        {"command": "wake", "args": ["ack", file_hash, infohash]})
        report["phases"]["wake"] = {"elapsed_ms": int((time.time() - t0) * 1000), "ack": ack}
        hot = (http_json(f"http://127.0.0.1:{PUBLIC_HTTP}/cli",
                         {"command": "seed", "args": []}) or {}).get("seed") or {}
        check("唤醒后 A 的引擎转入 hot",
              hot.get("state") == "hot" and bool(hot.get("seeding_hot")),
              json.dumps(hot, ensure_ascii=False))

        peer(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} -t 1 -r 2>/dev/null; sleep 1; "
             f"rm -rf {tx_dl}/* ; transmission-remote 127.0.0.1:{TX_RPC_PORT} --add {REMOTE_ROOT}/peer.torrent")
        time.sleep(1.5)

        seen_peers: set[str] = set()
        stop = threading.Event()

        def sampler() -> None:
            # 连接表采样必须比下载快：4 MiB 走公网可能只连几秒。
            while not stop.is_set():
                seen_peers.update(established_peers(PUBLIC_BT))
                time.sleep(0.3)

        th = threading.Thread(target=sampler, daemon=True)
        th.start()
        deadline = time.time() + DOWNLOAD_TIMEOUT
        pct, last = 0, ""
        try:
            while time.time() < deadline:
                rc, last = peer(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} -l")
                pct = parse_tx_percent(last)
                if pct >= 100:
                    break
                time.sleep(1.0)
        finally:
            stop.set()
            th.join(timeout=5)
        seen_peers.update(established_peers(PUBLIC_BT))
        report["phases"]["final_percent"] = pct
        report["phases"]["final_list"] = last.strip()
        report["a_side_peers"] = sorted(seen_peers)
        check("公网客户端报告下载完成 100%", pct >= 100, f"done={pct}%")

        rc, out = peer(f"sha1sum {tx_dl}/{sample_name} 2>/dev/null")
        report["phases"]["peer_sha1"] = out.strip()
        m = re.search(r"([0-9a-f]{40})", out)
        got = m.group(1) if m else ""
        check("外部主机落盘 SHA-1 与样本一致", got == sample_sha1, f"{got or '<缺失>'} vs {sample_sha1}")

        end_seed = (http_json(f"http://127.0.0.1:{PUBLIC_HTTP}/cli",
                              {"command": "seed", "args": []}) or {}).get("seed") or {}
        report["phases"]["end_seed"] = end_seed
        check("供片方计数器覆盖全量",
              int(end_seed.get("served_bytes") or 0) >= SAMPLE_BYTES,
              json.dumps(end_seed, ensure_ascii=False))

        # 反向证据：A 侧的连接对端必须是公网地址
        public_seen = [ip for ip in seen_peers if not is_private(ip)]
        lan_seen = [ip for ip in seen_peers if is_private(ip)]
        report["path_evidence"] = {"public": public_seen, "lan": lan_seen}
        check("A 侧 bt_port 的连接对端是公网地址（证明没走 LAN 捷径）",
              bool(public_seen) and not lan_seen,
              f"public={public_seen}, lan={lan_seen}")
        return finish("pass" if all(c["ok"] for c in report["checks"]) else "fail",
                      0 if all(c["ok"] for c in report["checks"]) else 1)
    finally:
        peer(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} --exit 2>/dev/null; "
             f"pkill -f '[t]ransmission-daemon -g {REMOTE_ROOT}' 2>/dev/null; true")
        for p in procs:
            p.terminate()
        for lf in logs:
            lf.close()
        if not args.keep:
            for p in procs:
                try:
                    p.wait(timeout=10)
                except Exception:
                    p.kill()


if __name__ == "__main__":
    sys.exit(main())
