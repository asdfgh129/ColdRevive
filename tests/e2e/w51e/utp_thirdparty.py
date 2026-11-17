#!/usr/bin/env python3
"""W5.1e 验收：µTP（BEP 29）互操作 —— 真实客户端**只经 UDP** 从 A 的引擎取片。

为什么要单独一条：W5.1b / W5.1c 的客户端都被显式关掉了 µTP（`utp-enabled=false`、
`Session\\BTProtocol=0`）。那不是产品需求，而是当时的一个误判——以为 librqbit 只做 TCP。
实际上 librqbit 9.x 自带 `librqbit-utp`，只是 `ListenerOptions` 默认 `TcpOnly`，
我们没打开。代价是：**默认配置的 Transmission 会先试 µTP 并干等一段才回落**，
现场形态就是「TCP 已 ESTABLISHED 但零字节」。

判「通过」五条缺一不可（每条都要两侧证据，单侧不算）：
    ① A 的引擎确实在 BT 端口上绑了 **UDP** —— 系统连接表里的 PID 必须是守护进程自己，
       不接受引擎自报（librqbit 绑 UDP 失败只打日志、TCP 照旧，属于静默降级）；
    ② 客户端**自报**它跟 A 之间的 peer 走的是 µTP（Transmission `peers[].isUTP`、
       qBittorrent `torrentPeers.connection`）——这是客户端自己的判断，不是我们的推断；
    ③ 传输全程 A 侧**没有一条来自 221 的 TCP 连接**（按 bt_port + 对端 IP 采样）；
    ④ 落盘 SHA-1 与样本逐字节一致，供片方 `served_bytes` 覆盖全量；
    ⑤ COLD 负对照仍然成立：唤醒前 0 字节（µTP 打开不得让「不唤醒也能取片」漏出来）。

边界（写进报告）：
    - `evidence_scope=lan-utp-interop`、`is_s6_evidence=false`；
    - 不含任何速率/可达率数字（ADR-012）；LAN 同二层网段，无公网报文。

用法：
    python tests/e2e/w51e/utp_thirdparty.py
    python tests/e2e/w51e/utp_thirdparty.py --keep
    python tests/e2e/w51e/utp_thirdparty.py --clients transmission

环境变量：
    COLDREVIVE_DAEMON   守护进程路径（缺省 target/debug/coldrevived(.exe)）
    CR_SSH_KEY          SSH 私钥（缺省 %TEMP%/crkeys/id_ed25519）
    CR_PEER             对端地址（缺省 192.168.1.221）
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
import time
import urllib.parse
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
SAMPLE_BYTES = 4 << 20          # 4 MiB = 16 片（PIECE_SIZE=262144）
COLD_PROBE_SECONDS = 10
DOWNLOAD_TIMEOUT = 240.0        # µTP 走用户态拥塞控制，给足余量

A_HOST = "192.168.1.210"
PEER = os.environ.get("CR_PEER", "192.168.1.221")
SSH_KEY = os.environ.get("CR_SSH_KEY") or str(
    Path(os.environ.get("TEMP", "/tmp")) / "crkeys" / "id_ed25519")
HERE = Path(__file__).resolve().parent

TX_RPC_PORT = 19181
QB_WEBUI_PORT = 18180
REMOTE_ROOT = "/tmp/cr-utp-w51e"


# ---------------------------------------------------------------------------
# 基础工具
# ---------------------------------------------------------------------------
def run(cmd: list[str], timeout: float = 60.0) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, capture_output=True, timeout=timeout)


def ssh(cmd: str, timeout: float = 60.0) -> tuple[int, str]:
    base = ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8",
            "-o", "StrictHostKeyChecking=no", "-i", SSH_KEY, f"root@{PEER}"]
    p = run(base + [cmd], timeout=timeout)
    return p.returncode, (p.stdout + p.stderr).decode("utf-8", "replace")


def ssh_bg(cmd: str) -> None:
    """后台拉起长期进程：必须 setsid，否则 ssh 断开时被带走（失败形态是静默降级）。"""
    ssh(f"setsid nohup bash -c {shquote(cmd)} >/dev/null 2>&1 &")


def shquote(s: str) -> str:
    return "'" + s.replace("'", "'\\''") + "'"


def free_port_local(port: int, kind: int = socket.SOCK_STREAM) -> bool:
    s = socket.socket(socket.AF_INET, kind)
    try:
        s.bind(("0.0.0.0", port))
        return True
    except OSError:
        return False
    finally:
        s.close()


def pick_port(start: int, kind: int = socket.SOCK_STREAM, span: int = 40) -> int:
    for p in range(start, start + span):
        if free_port_local(p, kind):
            return p
    raise RuntimeError(f"{start} 起的 {span} 个端口都被占用")


def remote_free_port(start: int, kind: str = "tcp", span: int = 40) -> int:
    flag = "-tln" if kind == "tcp" else "-uln"
    rc, out = ssh(f"ss {flag} 2>/dev/null | awk '{{print $4}}' | sed 's/.*://' | sort -u")
    used = {int(x) for x in re.findall(r"^\d+$", out, re.M)}
    for p in range(start, start + span):
        if p not in used:
            return p
    raise RuntimeError(f"221 上 {start} 起的 {span} 个端口都被占用")


def deterministic_sample(n: int) -> bytes:
    out = bytearray()
    seed = b"coldrevive-w51e-utp-sample"
    while len(out) < n:
        seed = hashlib.sha256(seed).digest()
        out.extend(seed * 8)
    return bytes(out[:n])


def http_json(url: str, payload: dict, timeout: float = 40.0) -> dict:
    body = json.dumps(payload).encode()
    req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode())


def wait_http(url: str, timeout: float, via_peer: bool = True) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if via_peer:
            rc, out = ssh(f"curl -sS -m 3 -o /dev/null -w '%{{http_code}}' {url} 2>/dev/null")
            if rc == 0 and out.strip() == "200":
                return True
        else:
            try:
                with urllib.request.urlopen(url, timeout=3) as r:
                    if r.status == 200:
                        return True
            except Exception:
                pass
        time.sleep(0.5)
    return False


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


# ---------------------------------------------------------------------------
# A 侧观测（都是系统层，不读我们的自报）
# ---------------------------------------------------------------------------
def udp_owners(port: int) -> list[int]:
    """`netstat -ano -p UDP` 里绑定该端口的 PID。

    行格式：`UDP    0.0.0.0:17802          *:*                 1234`。
    只认第一列是 UDP 且本地地址以 `:<port>` 结尾的行——按端口号做子串匹配会把
    178022 之类误判成 17802（同族坑：拿「看起来像」当「就是」）。
    """
    p = run(["netstat", "-ano", "-p", "UDP"], timeout=20)
    pids = []
    for line in p.stdout.decode("utf-8", "replace").splitlines():
        parts = line.split()
        if len(parts) < 4 or parts[0].upper() != "UDP":
            continue
        if parts[1].endswith(f":{port}") and parts[-1].isdigit():
            pids.append(int(parts[-1]))
    return sorted(set(pids))


def tcp_peers_from(bt_port: int, peer_ip: str) -> list[str]:
    """A 侧 bt_port 上已建立连接、且对端是 peer_ip 的地址列表。空 = 没走 TCP。"""
    ps = (f"$c = Get-NetTCPConnection -LocalPort {bt_port} -State Established "
          f"-ErrorAction SilentlyContinue | Where-Object {{ $_.RemoteAddress -eq '{peer_ip}' }}; "
          f"if ($c) {{ $c | ForEach-Object {{ \"$($_.RemoteAddress):$($_.RemotePort)\" }} }}")
    try:
        p = run(["powershell", "-NoProfile", "-Command", ps], timeout=25)
        out = p.stdout.decode("utf-8", "replace")
    except Exception:
        return []
    return [ln.strip() for ln in out.splitlines() if ln.strip()]


def ship_helper(name: str) -> None:
    """把 peer 观测脚本按 base64 送到 221（避免引号/编码在 ssh 层层转义里出错）。"""
    raw = (HERE / name).read_bytes()
    b64 = base64.b64encode(raw).decode()
    rc, out = ssh(f"echo {b64} | base64 -d > {REMOTE_ROOT}/{name} && echo shipped")
    if "shipped" not in out:
        raise RuntimeError(f"投送 {name} 失败: {out[:200]}")


def tx_peer_stats() -> dict:
    rc, out = ssh(f"python3 {REMOTE_ROOT}/tx_peers.py {TX_RPC_PORT} 2>&1")
    for line in reversed(out.strip().splitlines()):
        try:
            return json.loads(line)
        except Exception:
            continue
    return {"error": out.strip()[-200:]}


def qb_peer_stats(cookie: str, info_hash: str) -> dict:
    rc, out = ssh(f"python3 {REMOTE_ROOT}/qb_peers.py {QB_WEBUI_PORT} {cookie} {info_hash} 2>&1")
    for line in reversed(out.strip().splitlines()):
        try:
            return json.loads(line)
        except Exception:
            continue
    return {"error": out.strip()[-200:]}


# ---------------------------------------------------------------------------
# 主流程
# ---------------------------------------------------------------------------
def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--keep", action="store_true", help="保留工作目录便于排查")
    ap.add_argument("--clients", default="transmission,qbittorrent",
                    help="参与复核的第三方客户端，逗号分隔")
    ap.add_argument("--a-host", default=A_HOST, help="A 的 LAN 地址")
    args = ap.parse_args()
    clients = [c.strip() for c in args.clients.split(",") if c.strip()]

    daemon = os.environ.get("COLDREVIVE_DAEMON") or str(
        REPO / "target" / "debug" / ("coldrevived.exe" if os.name == "nt" else "coldrevived"))
    if not Path(daemon).is_file():
        print(f"[BLOCKED] 找不到守护进程 {daemon}")
        return 2

    work = Path(REPO / "tests" / "e2e" / "w51e" / "reports" / f"run-{time.strftime('%Y%m%dT%H%M%SZ')}")
    work.mkdir(parents=True, exist_ok=True)
    report: dict = {
        "schema": "coldrevive.w51e.utp-thirdparty/v1",
        "generated_at_unix": int(time.time()),
        "evidence_scope": "lan-utp-interop",
        "is_s6_evidence": False,
        "contains_success_rate": False,
        "seeder": f"cr-engine / librqbit + librqbit-utp（A @ {args.a_host}）",
        "downloader_host": PEER,
        "clients": clients,
        "sample": {"bytes": SAMPLE_BYTES},
        "checks": [],
        "phases": {},
    }

    def check(label: str, ok: bool, actual: str) -> bool:
        report["checks"].append({"label": label, "ok": bool(ok), "actual": actual})
        print(f"  [{'PASS' if ok else 'FAIL'}] {label} — {actual}")
        return ok

    procs: list[subprocess.Popen] = []
    logs: list = []
    daemon_pid = 0
    try:
        # ---------------- 0. 预检 ----------------
        print("== 0. 预检 ==")
        rc, out = ssh("echo ok; command -v transmission-daemon transmission-remote "
                      "qbittorrent-nox curl sha1sum python3")
        if rc != 0 or "ok" not in out:
            print(f"[BLOCKED] 无法登录 {PEER}: {out[:300]}")
            report["status"] = "blocked"
            return 2
        have = {n: (n in out) for n in ("transmission-daemon", "transmission-remote",
                                        "qbittorrent-nox", "python3")}
        report["peer_tools"] = have
        if not have["python3"]:
            print("[BLOCKED] 221 缺 python3，无法读取客户端自报的传输层")
            report["status"] = "blocked"
            return 2
        if "transmission" in clients and not (have["transmission-daemon"] and have["transmission-remote"]):
            print("[BLOCKED] 221 缺少 transmission-daemon / transmission-remote")
            report["status"] = "blocked"
            return 2
        if "qbittorrent" in clients and not have["qbittorrent-nox"]:
            print("[BLOCKED] 221 缺少 qbittorrent-nox")
            report["status"] = "blocked"
            return 2

        ssh(f"pkill -f '[t]ransmission-daemon -g {REMOTE_ROOT}' ; "
            f"pkill -f '[q]bittorrent-nox.*{QB_WEBUI_PORT}' ; "
            f"rm -rf {REMOTE_ROOT} ; mkdir -p {REMOTE_ROOT}")
        rc, out = ssh(f"mkdir -p {REMOTE_ROOT} && echo ready")
        if "ready" not in out:
            print(f"[BLOCKED] 221 无法准备目录: {out[:200]}")
            report["status"] = "blocked"
            return 2
        ship_helper("tx_peers.py")
        ship_helper("qb_peers.py")

        http_port = pick_port(18980)
        wake_port = pick_port(18981, socket.SOCK_DGRAM)
        bt_port = pick_port(18982)
        b_http = pick_port(http_port + 6)
        report["ports"] = {"http": http_port, "wake_udp": wake_port, "bt": bt_port}

        # ---------------- 1. 起 A（µTP 默认开） ----------------
        print("== 1. 启动 A（LAN 绑定，µTP 默认开） ==")
        sample_name = "coldrevive-w51e-utp-sample.bin"
        sample = work / sample_name
        data = deterministic_sample(SAMPLE_BYTES)
        sample.write_bytes(data)
        sample_sha1 = sha1_hex(data)
        report["sample"]["sha1"] = sample_sha1
        report["sample"]["name"] = sample_name

        cfg = {
            "bind": "0.0.0.0",
            "nodes": {"A": {"host": args.a_host, "http_port": http_port,
                            "wake_udp_port": wake_port, "bt_port": bt_port},
                      "B": {"host": "127.0.0.1", "http_port": b_http,
                            "wake_udp_port": pick_port(wake_port + 6, socket.SOCK_DGRAM),
                            "bt_port": pick_port(bt_port + 6)}},
            "sample_path": str(sample),
            "keywords": ["w51e-utp"],
            "namespace": "ColdRevive_V1:",
            "workdir": str(work),
            "golden_req": str(work / "wakeup_req.v1.json"),
            "golden_ack": str(work / "wakeup_ack.v1.json"),
            "downloader": "",
            "challenge_window_sec": 300,
            # 显式写出来：本轮的验收对象就是 µTP，不能靠「默认值恰好是 true」蒙过去。
            "nat": {"utp": True},
        }
        cfg_path = work / "w51e-nodes.json"
        cfg_path.write_text(json.dumps(cfg, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

        # B 是「用户侧」：唤醒请求经它发向 A。不带 B 就得在 A 上自唤醒，
        # 那条路径绕过了 WAKEUP_REQ/ACK，等于少验一环。
        for node in ("A", "B"):
            lf = open(work / f"daemon-{node}.log", "wb")
            logs.append(lf)
            proc = subprocess.Popen([daemon, "--config", str(cfg_path), "--node-id", node,
                                     "--bind", "0.0.0.0", "--workdir", str(work)],
                                    stdout=lf, stderr=subprocess.STDOUT, cwd=str(work))
            procs.append(proc)
            if node == "A":
                p_a = proc
        daemon_pid = p_a.pid
        report["daemon_pid"] = daemon_pid

        a_health = f"http://{args.a_host}:{http_port}/health"
        if not check("221 越过网卡访问 A 的 /health", wait_http(a_health, 30.0), a_health):
            report["status"] = "fail"
            return 1
        if not check("B 本地 /health", wait_http(f"http://127.0.0.1:{b_http}/health", 20.0,
                                                 via_peer=False), f"127.0.0.1:{b_http}"):
            report["status"] = "fail"
            return 1

        a_log = (work / "daemon-A.log").read_text(encoding="utf-8", errors="replace")
        report["a_log_excerpt"] = [ln for ln in a_log.splitlines()
                                   if "做种引擎" in ln or "utp" in ln]
        ready_line = next((ln for ln in a_log.splitlines() if "做种引擎就绪" in ln), "")
        check("A 的做种由引擎承担（非旧手写种子端）",
              "做种引擎就绪" in a_log and "标准 BT 种子端监听" not in a_log,
              ready_line[:200] or "<未出现『做种引擎就绪』>")

        # 引擎自报只是配置意图；真正的判据是系统连接表里这个 UDP 端口的属主。
        self_reported = "utp_requested=true" in ready_line and "utp_udp_port_occupied=true" in ready_line
        report["engine_utp_self_report"] = {
            "line": ready_line, "ok": self_reported}
        check("引擎自报已请求 µTP 且探测到 UDP 端口被占", self_reported,
              ready_line.split("utp_requested=")[-1] if "utp_requested=" in ready_line else "<缺字段>")

        owners = udp_owners(bt_port)
        report["udp_owners"] = {"port": bt_port, "pids": owners, "daemon_pid": daemon_pid}
        check("A 的系统连接表里 UDP/BT 端口的属主就是本进程（真的绑上了）",
              daemon_pid in owners, f"owners={owners}, daemon={daemon_pid}")

        # ---------------- 2. 取种子 ----------------
        print("== 2. 从 221 取伴随种子 ==")
        state = json.loads((work / "state-A.json").read_text(encoding="utf-8"))
        entry = next((e for e in (state.get("entries") or []) if e.get("infohash")), None)
        if not entry:
            print("[FAIL] state-A.json 里没有带 infohash 的条目")
            report["status"] = "fail"
            return 1
        infohash, file_hash = entry["infohash"], entry.get("file_hash") or ""
        report["infohash"] = infohash

        remote_torrent = f"{REMOTE_ROOT}/{infohash}.torrent"
        rc, out = ssh(f"curl -sS -m 15 -o {remote_torrent} -w '%{{http_code}} %{{size_download}}' "
                      f"http://{args.a_host}:{http_port}/torrent/{infohash}")
        parts = out.split()
        code = parts[0] if parts else ""
        size = int(parts[1]) if len(parts) > 1 and parts[1].isdigit() else 0
        check("221 取回伴随种子（真实字节）", rc == 0 and code == "200" and size > 0,
              f"http={code} bytes={size}")

        # ---------------- 3. 共同：唤醒与采样 ----------------
        def wake() -> dict:
            ack = http_json(f"http://127.0.0.1:{b_http}/cli",
                            {"command": "wake", "args": ["ack", file_hash, infohash]})
            hot = (http_json(f"http://127.0.0.1:{http_port}/cli",
                             {"command": "seed", "args": []}) or {}).get("seed") or {}
            return {"ack": ack, "seed": hot}

        def seed_state() -> dict:
            return (http_json(f"http://127.0.0.1:{http_port}/cli",
                              {"command": "seed", "args": []}) or {}).get("seed") or {}

        hot = {"done": False}

        def ensure_hot() -> bool:
            """唤醒只做一次。两条客户端路径都要它，所以抽出来而不是塞在某一段里。"""
            if hot["done"]:
                return True
            w = wake()
            ok = w["seed"].get("state") == "hot" and bool(w["seed"].get("seeding_hot"))
            report["phases"]["wake"] = w
            check("唤醒后 A 的引擎转入 hot", ok, json.dumps(w["seed"], ensure_ascii=False))
            hot["done"] = bool(ok)
            return ok

        tcp_samples: list[list[str]] = []

        def watch_until_done(percent_fn, timeout: float, interval: float = 1.0) -> dict:
            """轮询客户端进度，**同时**采样 A 侧有没有来自 221 的 TCP 连接。

            采样与进度放在同一循环里：只看「下载成功后没有 TCP 连接」会漏掉
            「先走 TCP 传完、连接随即消失」这种形态。
            `interval` 对 qB 调小：它的 peer 列表只在传输中存在，4 MiB 一两秒就完了。
            """
            deadline = time.time() + timeout
            pct, ticks = -1, 0
            while time.time() < deadline:
                pct = percent_fn()
                tcp_samples.append(tcp_peers_from(bt_port, PEER))
                ticks += 1
                if pct >= 100:
                    break
                time.sleep(interval)
            return {"percent": pct, "ticks": ticks}

        served_baseline = 0
        tx_utp_seen = 0
        qb_utp_seen = 0

        # ---------------- 4. Transmission 阶段 ----------------
        if "transmission" in clients:
            print("== 3. Transmission 3.00（µTP 开启，默认行为） ==")
            tx_peer = remote_free_port(19521)
            tx_conf = f"{REMOTE_ROOT}/tx"
            tx_dl = f"{tx_conf}/dl"
            ssh(f"mkdir -p {tx_conf} {tx_dl}")
            ssh("cat > " + f"{tx_conf}/settings.json <<'TXCFG'\n"
                + json.dumps({
                    # 这里**不关** µTP：要审的就是 Transmission 的默认首选传输层。
                    "utp-enabled": True,
                    "dht-enabled": False,
                    "pex-enabled": False,
                    "lpd-enabled": False,
                    "port-forwarding-enabled": False,
                    "encryption": 0,
                    "rpc-port": TX_RPC_PORT,
                    "peer-port": tx_peer,
                    "download-dir": tx_dl,
                }, indent=2) + "\nTXCFG")
            report["client_settings"] = {"utp_enabled": True,
                                         "note": "审的就是默认首选 µTP，不再人为关掉"}
            ssh_bg(f"transmission-daemon -g {tx_conf} -f >{tx_conf}/daemon.log 2>&1")
            out = ""
            for _ in range(40):
                rc, out = ssh(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} --list")
                if "Sum:" in out:
                    break
                time.sleep(0.5)
            check("Transmission RPC 就绪", "Sum:" in out, f"port={TX_RPC_PORT}")

            ssh(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} --add {remote_torrent}")
            time.sleep(2.0)

            # 负对照：未唤醒时不得有任何字节。µTP 打开也不能把「不唤醒也能取片」放出来。
            time.sleep(COLD_PROBE_SECONDS)
            rc, listing = ssh(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} -l")
            cold_pct = parse_tx_percent(listing)
            cold_seed = seed_state()
            report["phases"]["transmission_cold"] = {
                "client_percent": cold_pct, "seed": cold_seed,
                "peers": tx_peer_stats()}
            check("COLD 负对照：唤醒前第三方客户端拿不到数据",
                  cold_pct == 0 and cold_seed.get("state") == "cold"
                  and not cold_seed.get("seeding_hot")
                  and int(cold_seed.get("served_bytes") or 0) == 0,
                  f"client_done={cold_pct}%, state={cold_seed.get('state')}, "
                  f"served={cold_seed.get('served_bytes')}")

            ensure_hot()
            served_baseline = int(seed_state().get("served_bytes") or 0)

            # 冷相位里客户端连过 A（端口在听但引擎暂停），删掉重加让正相从干净状态开始。
            ssh(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} -t all -r 2>/dev/null; sleep 1; "
                f"rm -rf {tx_dl}/* ; "
                f"transmission-remote 127.0.0.1:{TX_RPC_PORT} --add {remote_torrent}")
            time.sleep(1.5)

            def tx_pct() -> int:
                nonlocal tx_utp_seen
                rc, listing = ssh(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} -l")
                p = parse_tx_percent(listing)
                if p >= 0:
                    st = tx_peer_stats()
                    report["phases"].setdefault("tx_peer_samples", []).append(st)
                    tx_utp_seen = max(tx_utp_seen, int(st.get("utp") or 0))
                return p

            res = watch_until_done(tx_pct, DOWNLOAD_TIMEOUT)
            report["phases"]["transmission_done"] = res
            check("Transmission 报告下载完成 100%", res["percent"] >= 100,
                  f"done={res['percent']}%, ticks={res['ticks']}")

            rc, out = ssh(f"sha1sum {tx_dl}/{sample_name} 2>/dev/null")
            m = re.search(r"([0-9a-f]{40})", out)
            got = m.group(1) if m else ""
            report["phases"]["transmission_sha1"] = out.strip()
            check("Transmission 落盘 SHA-1 与样本一致", got == sample_sha1,
                  f"{got or '<缺失>'} vs {sample_sha1}")

            end_seed = seed_state()
            report["phases"]["transmission_seed"] = end_seed
            delta = int(end_seed.get("served_bytes") or 0) - served_baseline
            check("Transmission 阶段供片方增量覆盖全量", delta >= SAMPLE_BYTES,
                  f"delta={delta}, total={end_seed.get('served_bytes')}")
            served_baseline = int(end_seed.get("served_bytes") or 0)

            check("Transmission 自报 peer 走 µTP（isUTP=true）", tx_utp_seen >= 1,
                  f"max_utp_peers={tx_utp_seen}（样本数 {len(report['phases'].get('tx_peer_samples', []))}）")

            ssh(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} --exit 2>/dev/null; sleep 0.5")

        # ---------------- 5. qBittorrent 阶段 ----------------
        if "qbittorrent" in clients:
            print("== 4. qBittorrent 4.5.1（强制 uTP-only） ==")
            qb_cfg = f"{REMOTE_ROOT}/qb/cfg"
            qb_data = f"{REMOTE_ROOT}/qb/data"
            qb_dl = f"{REMOTE_ROOT}/qb/dl"
            qb_peer_port = remote_free_port(19621)
            ssh(f"mkdir -p {qb_cfg}/qBittorrent {qb_data} {qb_dl}")
            salt = hashlib.sha256(b"coldrevive-w51e-qb-salt").digest()[:16]
            key = hashlib.pbkdf2_hmac("sha512", b"adminadmin", salt, 100000)
            pw = base64.b64encode(salt).decode() + ":" + base64.b64encode(key).decode()
            conf = (f"[BitTorrent]\n"
                    # 2 = 仅 uTP（1=TCP, 2=uTP, 3=both）。这是本轮的关键设置：
                    # 客户端物理上不可能走 TCP，于是「下载成功」必然来自 UDP。
                    f"Session\\BTProtocol=2\n"
                    f"Session\\DHTEnabled=false\n"
                    f"Session\\PeXEnabled=false\n"
                    f"Session\\LSDEnabled=false\n"
                    f"Session\\Port={qb_peer_port}\n"
                    f"\n[Preferences]\n"
                    f"WebUI\\Username=admin\n"
                    f"WebUI\\Password_PBKDF2=\"@ByteArray({pw})\"\n"
                    f"WebUI\\Port={QB_WEBUI_PORT}\n"
                    f"WebUI\\LocalHostAuth=false\n"
                    f"WebUI\\CSRFProtection=false\n")
            ssh("cat > " + f"{qb_cfg}/qBittorrent/qBittorrent.conf <<'QBCFG'\n" + conf + "QBCFG")
            ssh_bg(f"XDG_CONFIG_HOME={qb_cfg} XDG_DATA_HOME={qb_data} HOME={REMOTE_ROOT}/qb "
                   f"qbittorrent-nox --webui-port={QB_WEBUI_PORT} "
                   f">{REMOTE_ROOT}/qb/qb.log 2>&1")

            webui = f"http://127.0.0.1:{QB_WEBUI_PORT}"
            ready = False
            for _ in range(60):
                rc, out = ssh(f"curl -sS -m 2 -o /dev/null -w '%{{http_code}}' {webui}/api/v2/app/version")
                if rc == 0 and out.strip() in ("200", "401", "403"):
                    ready = True
                    break
                time.sleep(0.5)
            cookiejar = f"{REMOTE_ROOT}/qb/cookies.txt"
            rc, out = ssh(f"curl -sS -m 5 -c {cookiejar} -d 'username=admin&password=adminadmin' "
                          f"{webui}/api/v2/auth/login")
            logged_in = out.strip() == "Ok."
            check("qBittorrent WebUI 可登录", ready and logged_in,
                  f"ready={ready}, login={out.strip()[:40]}")

            if logged_in:
                # 只改网络相关的开关；传输层策略靠上面写进 ini 的 BTProtocol，
                # 这里再下发一次同名设置，避免 qB 首启把 ini 里的值回写掉。
                ssh(f"curl -sS -m 5 -b {cookiejar} --data-urlencode "
                    f"'json={{\"dht\":false,\"pex\":false,\"lsd\":false,"
                    f"\"listen_port\":{qb_peer_port}}}' "
                    f"{webui}/api/v2/app/setPreferences")

                def qb_add() -> str:
                    rc, out = ssh(f"curl -sS -m 15 -b {cookiejar} "
                                  f"-F 'torrents=@{remote_torrent};filename=t.torrent' "
                                  f"-F 'savepath={qb_dl}' {webui}/api/v2/torrents/add")
                    return out.strip()

                def qb_reset() -> None:
                    ssh(f"curl -sS -m 10 -b {cookiejar} -X POST "
                        f"--data 'hashes={infohash}&deleteFiles=true' "
                        f"{webui}/api/v2/torrents/delete ; sleep 1; rm -rf {qb_dl}/*")

                # peer 列表只在**连接存在时**才有内容：qB 拿完片立刻断开做种方，
                # 一秒钟一条的「一次一个 ssh」采样会整段错过 1–2 秒的传输窗口，
                # 读出来就是「没有 peer」，很容易被误判成「没走 µTP」。
                # 所以把高频采样放到 221 本地的一个进程里（0.1 s 一条），事后取文件。
                peers_file = f"{REMOTE_ROOT}/qb/peers.jsonl"
                ssh_bg(f"python3 {REMOTE_ROOT}/qb_peers.py {QB_WEBUI_PORT} {cookiejar} {infohash} "
                       f"--watch {int(DOWNLOAD_TIMEOUT) + 60} --interval 0.1 > {peers_file} 2>&1")

                if not hot["done"]:
                    # qB 单独跑时没有前面的 Transmission 阶段，负对照得自己做。
                    check("qBittorrent 接受种子（冷相位）", qb_add().lower().startswith("ok"), "cold-add")
                    time.sleep(COLD_PROBE_SECONDS)
                    cold_seed = seed_state()
                    report["phases"]["qb_cold"] = {"seed": cold_seed,
                                                   "peers": qb_peer_stats(cookiejar, infohash)}
                    check("COLD 负对照：唤醒前第三方客户端拿不到数据",
                          cold_seed.get("state") == "cold"
                          and not cold_seed.get("seeding_hot")
                          and int(cold_seed.get("served_bytes") or 0) == 0,
                          f"state={cold_seed.get('state')}, served={cold_seed.get('served_bytes')}")
                    ensure_hot()
                    qb_reset()
                served_baseline = int(seed_state().get("served_bytes") or 0)
                check("qBittorrent 接受种子", qb_add().lower().startswith("ok"), "hot-add")

                def qb_pct() -> int:
                    rc, out = ssh(f"curl -sS -m 5 -b {cookiejar} {webui}/api/v2/torrents/info")
                    try:
                        info = json.loads(out)
                    except Exception:
                        return -1
                    if not info:
                        return -1
                    return int(float(info[0].get("progress") or 0) * 100)

                try:
                    res = watch_until_done(qb_pct, DOWNLOAD_TIMEOUT, interval=0.5)
                finally:
                    ssh(f"pkill -f '[q]b_peers.py {QB_WEBUI_PORT}' 2>/dev/null; true")
                rc, out = ssh(f"cat {peers_file} 2>/dev/null")
                qb_samples = []
                for line in out.splitlines():
                    line = line.strip()
                    if not line.startswith("{"):
                        continue
                    try:
                        qb_samples.append(json.loads(line))
                    except Exception:
                        continue
                report["phases"]["qb_peer_samples"] = qb_samples
                qb_utp_seen = max([int((s.get("by_connection") or {}).get("uTP") or 0)
                                   for s in qb_samples] or [0])
                report["phases"]["qb_done"] = res
                check("qBittorrent 报告下载完成 100%", res["percent"] >= 100,
                      f"progress={res['percent']}%, ticks={res['ticks']}")

                rc, out = ssh(f"sha1sum {qb_dl}/{sample_name} 2>/dev/null")
                m = re.search(r"([0-9a-f]{40})", out)
                qb_sha = m.group(1) if m else ""
                report["phases"]["qb_sha1"] = out.strip()
                check("qBittorrent 落盘 SHA-1 与样本一致", qb_sha == sample_sha1,
                      f"{qb_sha or '<缺失>'} vs {sample_sha1}")

                end_seed = seed_state()
                report["phases"]["qb_seed"] = end_seed
                delta = int(end_seed.get("served_bytes") or 0) - served_baseline
                check("qBittorrent 阶段供片方增量覆盖全量", delta >= SAMPLE_BYTES,
                      f"delta={delta}, total={end_seed.get('served_bytes')}")

                check("qBittorrent 自报 peer 走 µTP（connection=μTP）", qb_utp_seen >= 1,
                      f"max_utp_peers={qb_utp_seen}（样本数 {len(report['phases'].get('qb_peer_samples', []))}）")

            ssh(f"pkill -f '[q]bittorrent-nox.*{QB_WEBUI_PORT}' ; sleep 0.5")

        # ---------------- 6. 路径纪律 ----------------
        tcp_hits = [s for s in tcp_samples if s]
        report["phases"]["tcp_samples"] = {
            "samples": len(tcp_samples), "with_peer_tcp": len(tcp_hits), "detail": tcp_hits[:5]}
        check("传输全程 A 侧没有来自 221 的 TCP 连接（数据只走 UDP）",
              len(tcp_samples) > 0 and not tcp_hits,
              f"samples={len(tcp_samples)}, hits={len(tcp_hits)} {tcp_hits[:3]}")

        report["status"] = "pass" if all(c["ok"] for c in report["checks"]) else "fail"
        passed = sum(1 for c in report["checks"] if c["ok"])
        report["summary"] = f"{passed}/{len(report['checks'])} PASS"
        return 0 if report["status"] == "pass" else 1

    except Exception as e:  # noqa: BLE001
        report["status"] = "error"
        report["reason"] = f"{type(e).__name__}: {e}"
        import traceback
        traceback.print_exc()
        return 3
    finally:
        for p in procs:
            try:
                p.terminate()
                p.wait(timeout=10)
            except Exception:
                try:
                    p.kill()
                except Exception:
                    pass
        for lf in logs:
            try:
                lf.close()
            except Exception:
                pass
        ssh(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} --exit 2>/dev/null; "
            f"pkill -f '[t]ransmission-daemon -g {REMOTE_ROOT}' 2>/dev/null; "
            f"pkill -f '[q]bittorrent-nox.*{QB_WEBUI_PORT}' 2>/dev/null; true", timeout=25)
        out = REPO / "tests" / "e2e" / "w51e" / "reports" / f"utp-{time.strftime('%Y%m%dT%H%M%SZ')}.json"
        out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(f"\n报告: {out}")
        if report.get("summary"):
            print(f"结果: {report['status'].upper()} {report['summary']}")
        if not args.keep:
            import shutil
            shutil.rmtree(work, ignore_errors=True)
            ssh(f"rm -rf {REMOTE_ROOT}", timeout=30)
        else:
            print(f"工作目录保留: {work}")


if __name__ == "__main__":
    sys.exit(main())
