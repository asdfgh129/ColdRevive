#!/usr/bin/env python3
"""W5.1f 验收：公网 µTP —— 外部主机**只经 UDP、只经公网地址**从 A 的引擎取片。

为什么要单独一条：
    W5.1d 证明了公网 **TCP** 入向成立（CPE 两条 TCP 映射）；
    W5.1e 证明了 µTP（BEP 29）在 LAN 上成立；
    但两者拼不起来——µTP 走 UDP，需要**另一条 UDP 映射**，而 TCP 映射对 UDP 无效。
    默认配置的客户端首选 µTP，没有这条 UDP 映射时它们会先等一段再回落 TCP，
    公网侧表现为「连得上但迟迟不来数据」。

判「通过」六条，每条都要两侧证据（单侧不算）：
    ① **量具先过关**：CPE 上确有 `UDP <wan>:<bt> -> 192.168.1.210:<bt>` 的映射——
       用一个与 µTP 无关的回显服务单独验证，并从 A 侧确认探测包的源是**公网 IP**
       （不是 192.168.1.222，否则是 LAN 捷径）；
    ② A 的引擎在 BT 端口上真的绑了 UDP——系统连接表里属主必须是守护进程本身，
       不接受引擎自报（绑失败只打日志、TCP 照旧，是静默降级）；
    ③ 客户端 `Session\\BTProtocol=2`（仅 uTP），物理上不可能走 TCP；
    ④ 客户端**自报** peer 的地址是 `<wan>:<bt>` 且 `connection=μTP`；
    ⑤ A 侧 bt_port 上**没有**来自公网 IP 的 TCP 连接（按对端 IP 采样）；
    ⑥ 落盘 SHA-1 与样本一致，供片方 `served_bytes` 增量覆盖全量；COLD 负对照仍为 0。

为什么 UDP 这条要这么绕：Windows 的连接表只有 UDP **端点**、没有对端，
所以「A 侧看到谁发来 UDP」拿不到。因此公网性由三件事交叉锁定：
tracker 只可能回公网端点（按请求方 IP 决策）→ 客户端自报 peer 地址是公网 IP →
回显对照证明该公网端点的入向确实打到 A 的这个 UDP 端口。

边界（写进报告）：
    - `evidence_scope=public-utp-single-egress`、`is_s6_evidence=false`；
    - 只有一个出口，不含任何可达率/速率数字（ADR-012）；
    - 传输耗时只作为观察项记录，不折算成吞吐结论。

前置条件（运维侧，CPE 上）：
    TCP <wan>:17800 -> 192.168.1.210:17800
    TCP <wan>:17802 -> 192.168.1.210:17802
    UDP <wan>:17802 -> 192.168.1.210:17802   ← 本轮新增，缺它直接 BLOCKED

用法：
    python tests/e2e/w51f/public_utp.py
    python tests/e2e/w51f/public_utp.py --keep --public-ip 112.5.60.4
    python tests/e2e/w51f/public_utp.py --probe-only     # 只验三条映射
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
HERE = Path(__file__).resolve().parent
SAMPLE_BYTES = 4 << 20
COLD_PROBE_SECONDS = 10
DOWNLOAD_TIMEOUT = 420.0        # 公网 µTP：用户态拥塞控制 + 单向延迟，给足余量

A_LAN = "192.168.1.210"
PUBLIC_HTTP = 17800
PUBLIC_WAKE = 17801
PUBLIC_BT = 17802
B_HTTP = 17900
B_WAKE = 17901
B_BT = 17902

REMOTE_ROOT = "/tmp/cr-pub-w51f"


# ---------------------------------------------------------------------------
# 基础工具
# ---------------------------------------------------------------------------
def run(cmd: list[str], timeout: float = 60.0) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, capture_output=True, timeout=timeout)


def free_port_local(port: int, kind: int = socket.SOCK_STREAM) -> bool:
    s = socket.socket(socket.AF_INET, kind)
    try:
        s.bind(("0.0.0.0", port))
        return True
    except OSError:
        return False
    finally:
        s.close()


class Peer:
    """外部主机（222）的命令通道。

    sshd 的 `MaxStartups` 会把「命令没跑成」伪装成业务结论（假的无 peer / 假的不存在文件），
    所以载体错误单独计数、重试，并保证两次连接之间 ≥1.5 s。判 BLOCKED 之前必须确认命令
    真的在对面执行过。
    """

    CARRIER_ERRORS = ("Exceeded MaxStartups", "kex_exchange_identification",
                      "ssh: connect to host", "Connection reset by peer",
                      "Connection closed by", "Connection to ",
                      "Broken pipe", "mux_client", "Timeout, server")

    def __init__(self, host: str, key: str, tries: int = 5) -> None:
        self.host, self.key, self.tries = host, key, tries
        self.carrier_failures = 0
        self._last_connect = 0.0
        self.min_gap = 1.5

    def _base(self) -> list[str]:
        return ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10",
                "-o", "StrictHostKeyChecking=no", "-o", "ServerAliveInterval=5",
                "-i", self.key, f"root@{self.host}"]

    def __call__(self, cmd: str, timeout: float = 60.0) -> tuple[int, str]:
        last = (1, "")
        for i in range(self.tries):
            gap = self.min_gap - (time.time() - self._last_connect)
            if gap > 0:
                time.sleep(gap)
            self._last_connect = time.time()
            try:
                p = run(self._base() + [cmd], timeout=timeout)
            except subprocess.TimeoutExpired:
                last = (1, "ssh 超时")
                time.sleep(2)
                continue
            out = (p.stdout + p.stderr).decode("utf-8", "replace")
            if p.returncode != 0 and any(e in out for e in self.CARRIER_ERRORS):
                self.carrier_failures += 1
                last = (1, out)
                time.sleep(3 + i * 3)
                continue
            return p.returncode, out
        return last

    def wait_ready(self, budget: float = 150.0) -> bool:
        deadline = time.time() + budget
        while time.time() < deadline:
            rc, out = self("echo __ready__")
            if rc == 0 and "__ready__" in out:
                return True
            time.sleep(5)
        return False

    def run_ok(self, cmd: str, ok, timeout: float = 60.0,
               tries: int = 4, gap: float = 3.0) -> tuple[bool, str, list[str]]:
        outs: list[str] = []
        for i in range(tries):
            rc, out = self(cmd, timeout=timeout)
            outs.append(out.strip()[:200])
            if ok(out):
                return True, out, outs
            if i < tries - 1:
                time.sleep(gap)
        return False, "", outs

    def close(self) -> None:
        return


def deterministic_sample(n: int) -> bytes:
    out = bytearray()
    seed = b"coldrevive-w51f-public-utp-sample"
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
        return getattr(e, "code", 0), b""


# ---- 最小 bencode（把 info 字典原样搬到只有公网 announce 的壳里） ----
def skip_bencode(b: bytes, i: int) -> int:
    c = b[i:i + 1]
    if c == b"i":
        return b.index(b"e", i) + 1
    if c in (b"l", b"d"):
        i += 1
        while b[i:i + 1] != b"e":
            i = skip_bencode(b, i)
        return i + 1
    j = b.index(b":", i)
    return j + 1 + int(b[i:j])


def info_slice(raw: bytes) -> bytes:
    pos = raw.find(b"4:info")
    if pos < 0:
        raise ValueError("种子里没有 4:info")
    start = pos + len(b"4:info")
    return raw[start:skip_bencode(raw, start)]


def bstr(s: bytes) -> bytes:
    return str(len(s)).encode() + b":" + s


def torrent_with_announce(raw: bytes, announce: str) -> bytes:
    info = info_slice(raw)
    return b"d" + bstr(b"announce") + bstr(announce.encode()) + bstr(b"info") + info + b"e"


def infohash_of(raw: bytes) -> str:
    return hashlib.sha1(info_slice(raw)).hexdigest()


def sha1_hex(b: bytes) -> str:
    return hashlib.sha1(b).hexdigest()


def established_peers(local_port: int) -> list[str]:
    """A 侧 bt_port 上已建立 TCP 连接的远端 IP。读系统连接表，不靠引擎自报。"""
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
        if not parts[1].endswith(f":{local_port}"):
            continue
        found.append(parts[2].rsplit(":", 1)[0])
    return found


def udp_owners(port: int) -> list[int]:
    p = run(["netstat", "-ano", "-p", "UDP"], timeout=20)
    pids = []
    for line in p.stdout.decode("utf-8", "replace").splitlines():
        parts = line.split()
        if len(parts) < 4 or parts[0].upper() != "UDP":
            continue
        if parts[1].endswith(f":{port}") and parts[-1].isdigit():
            pids.append(int(parts[-1]))
    return sorted(set(pids))


def is_private(ip: str) -> bool:
    return (ip.startswith("10.") or ip.startswith("127.") or ip.startswith("192.168.")
            or ip.startswith("169.254.") or re.match(r"^172\.(1[6-9]|2\d|3[01])\.", ip) is not None)


def wan_ip_of_self() -> str:
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
    ap.add_argument("--probe-only", action="store_true", help="只验证三条端口映射")
    ap.add_argument("--timeout", type=float, default=DOWNLOAD_TIMEOUT)
    args = ap.parse_args()

    key = os.environ.get("CR_SSH_KEY") or str(
        Path(os.environ.get("TEMP", "/tmp")) / "crkeys" / "id_ed25519")
    daemon = os.environ.get("COLDREVIVE_DAEMON") or str(
        REPO / "target" / "debug" / ("coldrevived.exe" if os.name == "nt" else "coldrevived"))
    if not Path(daemon).is_file():
        print(f"[BLOCKED] 找不到守护进程 {daemon}")
        return 2

    work = Path(REPO / "tests" / "e2e" / "w51f" / "reports" / f"run-{time.strftime('%Y%m%dT%H%M%SZ')}")
    work.mkdir(parents=True, exist_ok=True)
    report: dict = {
        "schema": "coldrevive.w51f.public-utp/v1",
        "generated_at_unix": int(time.time()),
        "evidence_scope": "public-utp-single-egress",
        "is_s6_evidence": False,
        "contains_success_rate": False,
        "seeder": f"cr-engine / librqbit + librqbit-utp（A @ {A_LAN}）",
        "external_host": args.peer,
        "ports": {"http": PUBLIC_HTTP, "bt": PUBLIC_BT, "wake_udp": PUBLIC_WAKE},
        "checks": [],
        "phases": {},
        "notes": [
            "UDP 在 Windows 连接表里只有端点、没有对端，因此公网性由 tracker 决策、"
            "客户端自报 peer 地址、回显对照三方交叉锁定。",
            "传输耗时只作观察项，不折算吞吐（ADR-012）。",
        ],
    }

    def check(label: str, ok: bool, actual: str) -> bool:
        report["checks"].append({"label": label, "ok": bool(ok), "actual": actual})
        print(f"  [{'PASS' if ok else 'FAIL'}] {label} — {actual}")
        return ok

    def finish(status: str, code: int) -> int:
        report["status"] = status
        blob = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
        (work / "report.json").write_text(blob, encoding="utf-8")
        # 再留一份在 reports/ 下：--keep 关闭时工作目录会被删掉，证据不能跟着没。
        (HERE / "reports" / f"{work.name}.json").write_text(blob, encoding="utf-8")
        print(f"\n报告: {HERE / 'reports' / f'{work.name}.json'}")
        return code

    peer = Peer(args.peer, key)
    procs: list[subprocess.Popen] = []
    logs: list = []
    echo_proc: subprocess.Popen | None = None
    echo_log: Path | None = None
    try:
        # ---------------- 0. 预检 ----------------
        print("== 0. 预检 ==")
        pre_ok, pre_out = False, ""
        deadline = time.time() + 180
        while time.time() < deadline:
            rc, pre_out = peer("echo __ok__; command -v qbittorrent-nox python3 curl sha1sum")
            if rc == 0 and "__ok__" in pre_out:
                pre_ok = True
                break
            peer.wait_ready(budget=30)
        if not pre_ok:
            print(f"[BLOCKED] 无法登录外部主机 {args.peer}（载体失败 {peer.carrier_failures} 次）："
                  f"{pre_out[:160]}")
            report["blocked_reason"] = "external host unreachable via ssh"
            return finish("blocked", 2)
        tools = {n: (n in pre_out) for n in ("qbittorrent-nox", "python3")}
        report["peer_tools"] = tools

        # 观测脚本按 base64 投送：引号/编码经 ssh 层层转义容易出错。
        helper_b64 = base64.b64encode((HERE / "qb_peers_pub.py").read_bytes()).decode()
        oks, sout, _ = peer.run_ok(
            f"mkdir -p {REMOTE_ROOT} && echo {helper_b64} | base64 -d > {REMOTE_ROOT}/qb_peers_pub.py "
            f"&& python3 -c 'import ast,sys; ast.parse(open(sys.argv[1]).read())' "
            f"{REMOTE_ROOT}/qb_peers_pub.py && echo shipped",
            lambda s: "shipped" in s, tries=4)
        if not oks:
            print(f"[BLOCKED] 投送 peer 观测脚本失败：{sout[:160]}")
            report["blocked_reason"] = "ship peer sampler failed"
            return finish("blocked", 2)
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

        if not tools["python3"]:
            print("[BLOCKED] 外部主机缺 python3，无法读客户端自报的传输层")
            report["blocked_reason"] = "peer missing python3"
            return finish("blocked", 2)

        # 本机端口必须干净：残留守护进程会让我们测到上一轮的东西。
        busy = [f"{p}/{k}" for p, k in ((PUBLIC_HTTP, "tcp"), (PUBLIC_BT, "tcp"),
                                        (PUBLIC_BT, "udp"), (PUBLIC_WAKE, "udp"))
                if not free_port_local(p, socket.SOCK_DGRAM if k == "udp" else socket.SOCK_STREAM)]
        if busy:
            print(f"[BLOCKED] A 上这些端口已被占用（可能有残留守护进程）：{busy}")
            report["blocked_reason"] = f"local ports busy: {busy}"
            return finish("blocked", 2)

        # ---------------- 1. 量具：UDP 映射回显对照 ----------------
        # 先证明「这个公网 UDP 端点的入向确实打到 A 的这个端口」，再谈 µTP 能不能跑。
        print("== 1. 用与 µTP 无关的回显服务验证 UDP 映射 ==")
        echo_log = work / "udp-echo.log"
        echo_proc = subprocess.Popen(
            [sys.executable, str(HERE / "udp_echo_control.py"), str(PUBLIC_BT), "90"],
            stdout=open(echo_log, "wb"), stderr=subprocess.STDOUT)
        listening = False
        for _ in range(40):
            if echo_log.is_file() and "LISTEN" in echo_log.read_text(encoding="utf-8", errors="replace"):
                listening = True
                break
            time.sleep(0.25)
        if not listening:
            print("[BLOCKED] 本机回显服务没起来，量具无效")
            report["blocked_reason"] = "udp echo control did not start"
            return finish("blocked", 2)

        probe_cmd = (
            "python3 - <<'PY'\n"
            "import socket\n"
            "s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); s.settimeout(5)\n"
            "ok=0\n"
            "for i in range(3):\n"
            f"    s.sendto(('probe%d'%i).encode(), ('{wan}',{PUBLIC_BT}))\n"
            "    try:\n"
            "        d,a=s.recvfrom(2048); ok+=1\n"
            "        print('REPLY',a[0],a[1],d.decode('latin-1'),flush=True)\n"
            "    except socket.timeout:\n"
            "        print('TIMEOUT',i,flush=True)\n"
            "print('REPLIES',ok,flush=True)\n"
            "PY")
        okp, pout, ptries = peer.run_ok(probe_cmd, lambda s: "REPLIES" in s, tries=3, gap=2)
        replies = re.findall(r"^REPLY (\S+) (\d+) ECHO:(\S+)$", pout, re.M)
        report["phases"]["udp_mapping_probe"] = {
            "cmd_ok": okp, "replies": replies, "raw": pout.strip()[:400]}
        time.sleep(1.0)
        echo_text = echo_log.read_text(encoding="utf-8", errors="replace") if echo_log.exists() else ""
        seen_src = re.findall(r"^GOT (\d{1,3}(?:\.\d{1,3}){3}):(\d+) ", echo_text, re.M)
        report["phases"]["udp_mapping_observed_source"] = seen_src

        echo_proc.terminate()
        try:
            echo_proc.wait(timeout=5)
        except Exception:
            echo_proc.kill()
        echo_proc = None
        for _ in range(40):
            if free_port_local(PUBLIC_BT, socket.SOCK_DGRAM):
                break
            time.sleep(0.25)
        else:
            print(f"[BLOCKED] {PUBLIC_BT}/udp 回显进程退出后端口未释放")
            report["blocked_reason"] = "udp port not released after control"
            return finish("blocked", 2)

        measured = len(replies) >= 3 and all(r[0] == wan and int(r[1]) == PUBLIC_BT for r in replies)
        if not measured:
            report["blocked_reason"] = "no udp port mapping on cpe"
            report["required_cpe_rules"] = [
                f"UDP {wan}:{PUBLIC_BT} -> {A_LAN}:{PUBLIC_BT}",
            ]
            print("\n[BLOCKED] 公网 UDP 端口没有映射到 A，µTP 的公网入向不可能成立。")
            print("          需要在 CPE 里添加（与 TCP 那条是**两条独立规则**）：")
            print(f"            UDP {wan}:{PUBLIC_BT} -> {A_LAN}:{PUBLIC_BT}")
            print(f"          探测输出：{pout.strip()[:300]!r}")
            return finish("blocked", 2)
        check("CPE 上存在 UDP 公网映射（回显包原路返回）", True,
              f"{len(replies)}/3 从 {wan}:{PUBLIC_BT} 回到外部主机")
        # 「源是公网 IP」这一条把 LAN 捷径排除掉：若走内网，A 侧看到的源会是 192.168.1.222。
        src_ips = sorted({s[0] for s in seen_src})
        check("A 侧看到的探测源是公网 IP（不是 LAN 地址）",
              bool(peer_wan) and src_ips == [peer_wan],
              f"observed={src_ips}, expected=[{peer_wan}]")

        if args.probe_only:
            print("\n--probe-only：UDP 映射已通，跳过客户端复核。")
            return finish("pass", 0)

        if not tools["qbittorrent-nox"]:
            print("[BLOCKED] 外部主机缺 qbittorrent-nox（只有它能强制 uTP-only）")
            report["blocked_reason"] = "peer missing qbittorrent-nox"
            return finish("blocked", 2)

        # ---------------- 2. 起 A（公网档，µTP 默认开）+ B ----------------
        print("== 2. 启动 A（公网映射档）与 B ==")
        sample_name = "coldrevive-w51f-public-utp-sample.bin"
        sample = work / sample_name
        data = deterministic_sample(SAMPLE_BYTES)
        sample.write_bytes(data)
        sample_sha1 = sha1_hex(data)
        report["sample"] = {"name": sample_name, "bytes": SAMPLE_BYTES, "sha1": sample_sha1}

        cfg = {
            "bind": "0.0.0.0",
            "nodes": {
                "A": {"host": A_LAN, "http_port": PUBLIC_HTTP,
                      "wake_udp_port": PUBLIC_WAKE, "bt_port": PUBLIC_BT},
                "B": {"host": "127.0.0.1", "http_port": B_HTTP,
                      "wake_udp_port": B_WAKE, "bt_port": B_BT},
            },
            "sample_path": str(sample),
            "keywords": ["w51f-public-utp"],
            "namespace": "ColdRevive_V1:",
            "workdir": str(work),
            "golden_req": str(work / "wakeup_req.v1.json"),
            "golden_ack": str(work / "wakeup_ack.v1.json"),
            "downloader": "",
            "challenge_window_sec": 300,
            # 本轮的验收对象就是公网 µTP：显式写 true，不靠「默认恰好是 true」。
            "nat": {
                "enabled": True,
                "bt_external": f"{wan}:{PUBLIC_BT}",
                "http_external": f"{wan}:{PUBLIC_HTTP}",
                "utp": True,
            },
        }
        cfg_path = work / "w51f-nodes.json"
        cfg_path.write_text(json.dumps(cfg, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

        for node in ("A", "B"):
            lf = open(work / f"daemon-{node}.log", "wb")
            logs.append(lf)
            procs.append(subprocess.Popen(
                [daemon, "--config", str(cfg_path), "--node-id", node,
                 "--bind", "0.0.0.0", "--workdir", str(work)],
                stdout=lf, stderr=subprocess.STDOUT, cwd=str(work)))
        p_a = procs[0]
        daemon_pid = p_a.pid
        report["daemon_pid"] = daemon_pid

        ok = False
        for _ in range(60):
            code, _b = http_get(f"http://127.0.0.1:{PUBLIC_HTTP}/health", timeout=2)
            if code == 200:
                ok = True
                break
            time.sleep(0.5)
        if not check("A 本机 /health", ok, f"127.0.0.1:{PUBLIC_HTTP}/health"):
            return finish("fail", 1)

        pub = {}
        code, body = http_get(f"http://127.0.0.1:{PUBLIC_HTTP}/nat/public", timeout=5)
        if code == 200:
            pub = json.loads(body.decode())
        report["nat_public"] = pub
        check("A 报告的引擎监听是 0.0.0.0（外部包才进得来）",
              str(pub.get("engine_listen", "")).startswith("0.0.0.0:"),
              str(pub.get("engine_listen")))
        check("配置态里 µTP 是开着的", pub.get("utp_requested") is True,
              f"utp_requested={pub.get('utp_requested')} note={str(pub.get('utp_note'))[:80]}")
        check("种子 announce 同时含 LAN 与公网两条",
              len(pub.get("announce") or []) == 2
              and any(wan in u for u in (pub.get("announce") or [])),
              json.dumps(pub.get("announce"), ensure_ascii=False))
        check("A 不声称入向已验证（本机无法自证）",
              pub.get("inbound_verified") is False,
              f"inbound_verified={pub.get('inbound_verified')}")

        a_log = (work / "daemon-A.log").read_text(encoding="utf-8", errors="replace")
        report["a_log_excerpt"] = [ln for ln in a_log.splitlines()
                                   if "做种引擎" in ln or "utp" in ln]
        ready_line = next((ln for ln in a_log.splitlines() if "做种引擎就绪" in ln), "")
        check("做种由引擎承担且自报 µTP/UDP 端口状态",
              "做种引擎就绪" in a_log and "utp_requested=true" in ready_line,
              ready_line[:200] or "<未出现『做种引擎就绪』>")

        # 真正的判据：系统连接表里这个 UDP 端口的属主是不是守护进程。
        owners = udp_owners(PUBLIC_BT)
        report["udp_owners"] = {"port": PUBLIC_BT, "pids": owners, "daemon_pid": daemon_pid}
        check(f"A 的系统连接表里 {PUBLIC_BT}/udp 属主就是本进程",
              daemon_pid in owners, f"owners={owners}, daemon={daemon_pid}")

        # ---------------- 3. 外部取种子 + tracker 决策 ----------------
        print("== 3. 外部主机经公网取种子并 announce ==")
        okh, hout, htry = peer.run_ok(
            f"curl -sS -m 8 -w ' %{{http_code}}' http://{wan}:{PUBLIC_HTTP}/health",
            lambda s: s.strip().endswith("200") and '"ok"' in s, tries=4)
        report["phases"]["public_health"] = hout.strip() or htry[-1]
        check("外部主机经公网访问 A 的 /health", okh,
              (hout.strip() or f"载体失败 x{len(htry)}")[:120])

        state = json.loads((work / "state-A.json").read_text(encoding="utf-8"))
        entry = next((e for e in (state.get("entries") or []) if e.get("infohash")), None)
        if not entry:
            print("[FAIL] state-A.json 里没有带 infohash 的条目")
            return finish("fail", 1)
        infohash, file_hash = entry["infohash"], entry.get("file_hash") or ""
        report["infohash"] = infohash

        okt, out, ttry = peer.run_ok(
            f"mkdir -p {REMOTE_ROOT}; curl -sS -m 15 -o {REMOTE_ROOT}/a.torrent "
            f"-w '%{{http_code}} %{{size_download}}' "
            f"http://{wan}:{PUBLIC_HTTP}/torrent/{infohash}",
            lambda s: len(s.split()) >= 2 and s.split()[0] == "200" and int(s.split()[1]) > 0,
            tries=4)
        report["phases"]["public_torrent_fetch"] = out.strip() or ttry[-1]
        check("外部主机经公网取回伴随种子（真实字节）", okt,
              (out.strip() or f"载体失败 x{len(ttry)}")[:120])

        raw = b""
        rc, b64out = peer(f"base64 -w0 {REMOTE_ROOT}/a.torrent")
        try:
            raw = base64.b64decode(re.sub(r"[^A-Za-z0-9+/=]", "", b64out))
        except Exception:
            raw = b""
        lan_announce = f"http://{A_LAN}:{PUBLIC_HTTP}/announce"
        pub_url = f"http://{wan}:{PUBLIC_HTTP}/announce"
        text = raw.decode("latin-1", "replace")
        report["torrent_announces"] = {"has_lan": lan_announce in text,
                                       "has_public": pub_url in text, "bytes": len(raw)}
        check("种子带 BEP 12 announce-list（LAN + 公网）",
              lan_announce in text and pub_url in text,
              json.dumps(report["torrent_announces"], ensure_ascii=False))

        # tracker 对公网请求方必须回公网端点。二进制响应走落盘 + base64（不经控制台）。
        ih_param = urllib.parse.quote_from_bytes(bytes.fromhex(infohash))
        q = (f"info_hash={ih_param}"
             f"&peer_id={urllib.parse.quote_from_bytes(b'cr-w51f-probe-000000000')}"
             f"&port=19631&uploaded=0&downloaded=0&left={SAMPLE_BYTES}"
             f"&compact=1&numwant=20")
        attempts = []
        peer_ip, peer_port = "", 0
        for _attempt in range(3):
            rc, out = peer(
                f"curl -sS -m 10 -o /tmp/w51f-ann.bin "
                f"-w 'HTTP=%{{http_code}} SIZE=%{{size_download}}' "
                f"'{pub_url}?{q}'; echo; echo \"rc=$?\"; "
                f"base64 -w0 /tmp/w51f-ann.bin 2>/dev/null; echo")
            m_hdr = re.search(r"HTTP=(\d+) SIZE=(\d+)", out)
            b64 = out.strip().splitlines()[-1] if out.strip() else ""
            try:
                body = base64.b64decode(re.sub(r"[^A-Za-z0-9+/=]", "", b64))
            except Exception:
                body = b""
            rec = {"http": int(m_hdr.group(1)) if m_hdr else None,
                   "size": int(m_hdr.group(2)) if m_hdr else None,
                   "bytes": len(body),
                   "text": body[:160].decode("latin-1", "replace")}
            attempts.append(rec)
            marker = b"5:peers6:"
            idx = body.find(marker)
            if idx >= 0 and len(body[idx + len(marker): idx + len(marker) + 6]) == 6:
                blob = body[idx + len(marker): idx + len(marker) + 6]
                peer_ip = ".".join(str(b) for b in blob[:4])
                peer_port = int.from_bytes(blob[4:], "big")
                break
            time.sleep(2)
        report["tracker"] = {"peer_ip": peer_ip, "peer_port": peer_port, "attempts": attempts}
        check("tracker 对公网请求方通告公网 peer（不是 LAN 地址）",
              peer_ip == wan and peer_port == PUBLIC_BT,
              f"{peer_ip}:{peer_port}（期望 {wan}:{PUBLIC_BT}）"
              f" 末次={json.dumps(attempts[-1], ensure_ascii=False)}")

        # 客户端只用公网 announce 的变体：否则 222 还挂在 LAN 上，可能走内网。
        pub_only = torrent_with_announce(raw, pub_url)
        check("公网版种子 InfoHash 与 A 的服务内容一致",
              infohash_of(pub_only) == infohash, f"{infohash_of(pub_only)} vs {infohash}")
        (work / "peer-public-only.torrent").write_bytes(pub_only)
        b64 = base64.b64encode(pub_only).decode()
        okd, dout, _ = peer.run_ok(
            f"base64 -d > {REMOTE_ROOT}/peer.torrent <<'B64'\n{b64}\nB64\nls -l {REMOTE_ROOT}/peer.torrent",
            lambda s: "peer.torrent" in s, tries=4)
        if not okd:
            print(f"[BLOCKED] 投送公网版种子失败：{dout[:160]}")
            report["blocked_reason"] = "ship public-only torrent failed"
            return finish("blocked", 2)

        # ---------------- 4. qBittorrent（仅 uTP） ----------------
        print("== 4. qBittorrent 4.5.1（BTProtocol=2，仅 uTP） ==")
        qb_cfg = f"{REMOTE_ROOT}/qb/cfg"
        qb_data = f"{REMOTE_ROOT}/qb/data"
        qb_dl = f"{REMOTE_ROOT}/qb/dl"
        qb_webui = 18180
        qb_peer_port = 19631
        ssh_prep = peer(f"pkill -f '[q]bittorrent-nox.*{qb_webui}' 2>/dev/null; "
                        f"rm -rf {REMOTE_ROOT}/qb; mkdir -p {qb_cfg}/qBittorrent {qb_data} {qb_dl}")
        report["phases"]["qb_prep"] = ssh_prep[1].strip()[:200]
        salt = hashlib.sha256(b"coldrevive-w51f-qb-salt").digest()[:16]
        key_pw = hashlib.pbkdf2_hmac("sha512", b"adminadmin", salt, 100000)
        pw = base64.b64encode(salt).decode() + ":" + base64.b64encode(key_pw).decode()
        conf = ("[BitTorrent]\n"
                # 2 = 仅 uTP（1=TCP, 2=uTP, 3=both）。这一行是核心：客户端物理上不可能走 TCP。
                "Session\\BTProtocol=2\n"
                "Session\\DHTEnabled=false\n"
                "Session\\PeXEnabled=false\n"
                "Session\\LSDEnabled=false\n"
                f"Session\\Port={qb_peer_port}\n"
                "\n[Preferences]\n"
                "WebUI\\Username=admin\n"
                f"WebUI\\Password_PBKDF2=\"@ByteArray({pw})\"\n"
                f"WebUI\\Port={qb_webui}\n"
                "WebUI\\LocalHostAuth=false\n"
                "WebUI\\CSRFProtection=false\n")
        okcfg, cout, _ = peer.run_ok(
            f"cat > {qb_cfg}/qBittorrent/qBittorrent.conf <<'QBCFG'\n{conf}QBCFG\nwc -l < {qb_cfg}/qBittorrent/qBittorrent.conf",
            lambda s: bool(re.search(r"^\s*\d+\s*$", s, re.M)), tries=3)
        if not okcfg:
            print(f"[BLOCKED] 写 qB 配置失败：{cout[:160]}")
            report["blocked_reason"] = "write qb config failed"
            return finish("blocked", 2)

        okq, qout, _ = peer.run_ok(
            f"XDG_CONFIG_HOME={qb_cfg} XDG_DATA_HOME={qb_data} HOME={REMOTE_ROOT}/qb "
            f"setsid nohup qbittorrent-nox --webui-port={qb_webui} "
            f">{REMOTE_ROOT}/qb/qb.log 2>&1 & sleep 4; echo started",
            lambda s: "started" in s, tries=3)
        if not okq:
            print(f"[BLOCKED] qBittorrent 启动命令没跑成：{qout[:160]}")
            report["blocked_reason"] = "qb start failed"
            return finish("blocked", 2)

        webui = f"http://127.0.0.1:{qb_webui}"
        cookiejar = f"{REMOTE_ROOT}/qb/cookies.txt"
        ready = False
        for _ in range(60):
            rc, out = peer(f"curl -sS -m 2 -o /dev/null -w '%{{http_code}}' {webui}/api/v2/app/version")
            if rc == 0 and out.strip() in ("200", "401", "403"):
                ready = True
                break
            time.sleep(1.0)
        rc, out = peer(f"curl -sS -m 5 -c {cookiejar} -d 'username=admin&password=adminadmin' "
                       f"{webui}/api/v2/auth/login")
        logged_in = out.strip() == "Ok."
        check("qBittorrent WebUI 可登录", ready and logged_in,
              f"ready={ready}, login={out.strip()[:40]}")
        if not logged_in:
            print("[BLOCKED] 无法登录 222 上的 qBittorrent WebUI")
            report["blocked_reason"] = "qb webui login failed"
            return finish("blocked", 2)

        # 只下发网络开关；传输层策略留在 ini 里（避免 API 的枚举顺序把它改回去）。
        peer(f"curl -sS -m 5 -b {cookiejar} --data-urlencode "
             f"'json={{\"dht\":false,\"pex\":false,\"lsd\":false,"
             f"\"listen_port\":{qb_peer_port}}}' {webui}/api/v2/app/setPreferences")

        def qb_get(path: str, tries: int = 3):
            for _i in range(tries):
                rc, out = peer(f"curl -sS -m 8 -b {cookiejar} {webui}{path}")
                s = out.strip()
                if s.startswith("[") or s.startswith("{"):
                    try:
                        return json.loads(s)
                    except Exception:
                        pass
                time.sleep(2)
            return None

        def qb_add() -> str:
            rc, out = peer(f"curl -sS -m 20 -b {cookiejar} "
                           f"-F 'torrents=@{REMOTE_ROOT}/peer.torrent;filename=t.torrent' "
                           f"-F 'savepath={qb_dl}' {webui}/api/v2/torrents/add")
            return out.strip()

        def qb_delete() -> str:
            rc, out = peer(f"curl -sS -m 10 -b {cookiejar} -X POST "
                           f"--data 'hashes={infohash}&deleteFiles=true' "
                           f"{webui}/api/v2/torrents/delete")
            return out.strip()

        # ---- COLD 负对照：唤醒前一个字节都不能有 ----
        okadd, addout, addtry = peer.run_ok(
            f"curl -sS -m 20 -b {cookiejar} "
            f"-F 'torrents=@{REMOTE_ROOT}/peer.torrent;filename=t.torrent' "
            f"-F 'savepath={qb_dl}' {webui}/api/v2/torrents/add",
            lambda s: s.strip().lower().startswith("ok"), tries=4)
        report["phases"]["qb_add_cold"] = addout.strip() or addtry[-1]
        if not okadd:
            print(f"[BLOCKED] 对端客户端加种失败：{addtry[-1][:160]}")
            report["blocked_reason"] = "qb add failed"
            return finish("blocked", 2)

        time.sleep(COLD_PROBE_SECONDS)
        info_cold = qb_get("/api/v2/torrents/info") or []
        cold_pct = int(float((info_cold[0].get("progress") if info_cold else 0) or 0) * 100)
        cold_seed = (http_json(f"http://127.0.0.1:{PUBLIC_HTTP}/cli",
                               {"command": "seed", "args": []}) or {}).get("seed") or {}
        report["phases"]["cold"] = {"client_percent": cold_pct, "seed": cold_seed,
                                    "info": info_cold[:1]}
        check("COLD 负对照：唤醒前公网客户端拿不到数据",
              cold_pct == 0 and cold_seed.get("state") == "cold"
              and not cold_seed.get("seeding_hot")
              and int(cold_seed.get("served_bytes") or 0) == 0,
              f"client_done={cold_pct}%, state={cold_seed.get('state')}, "
              f"served={cold_seed.get('served_bytes')}")

        # ---------------- 5. 唤醒 + 取片 ----------------
        print("== 5. 唤醒 A，等公网客户端经 µTP 取完 ==")
        t0 = time.time()
        ack = http_json(f"http://127.0.0.1:{B_HTTP}/cli",
                        {"command": "wake", "args": ["ack", file_hash, infohash]})
        elapsed_ms = int((time.time() - t0) * 1000)
        hot = (http_json(f"http://127.0.0.1:{PUBLIC_HTTP}/cli",
                         {"command": "seed", "args": []}) or {}).get("seed") or {}
        report["phases"]["wake"] = {"elapsed_ms": elapsed_ms, "ack": ack}
        check("唤醒后 A 的引擎转入 hot",
              hot.get("state") == "hot" and bool(hot.get("seeding_hot")),
              json.dumps(hot, ensure_ascii=False))

        # 复位：冷相位里客户端可能已建立连接（引擎暂停，无数据）。清干净再测正相。
        #
        # 必须**证明文件真的被删掉**：只发一条 delete 就重加，qB 可能重新校验残留文件并从
        # 断点续传（48 KiB），于是「供片增量」比整片少一截——这不是产品少传了，是我们把
        # 上一段的数据算进了这一段。上一轮就栽在这里（delta 恰好少 49152 B）。
        qb_delete()
        gone, gone_out = False, ""
        for _ in range(24):
            rc, gone_out = peer(f"rm -rf {qb_dl}/*; "
                                f"test -e {qb_dl}/{sample_name} && echo PRESENT || echo GONE")
            if "GONE" in gone_out:
                gone = True
                break
            time.sleep(0.5)
        check("复位后旧文件确实不存在（否则会续传，污染供片计数）", gone,
              "GONE" if gone else gone_out.strip()[:120])
        if not gone:
            report["blocked_reason"] = "client file not removed before reset"
            return finish("blocked", 2)
        time.sleep(1.0)
        # 基线必须在**加种之前**读：加完种客户端立刻开始下，晚一秒读就把这一秒的字节
        # 算进基线，delta 会比整片少一点，看着像「供片方少传了」。
        served_baseline = int(((http_json(f"http://127.0.0.1:{PUBLIC_HTTP}/cli",
                                          {"command": "seed", "args": []}) or {})
                               .get("seed") or {}).get("served_bytes") or 0)
        report["phases"]["served_baseline"] = served_baseline
        okre, reout, retry = peer.run_ok(
            f"curl -sS -m 20 -b {cookiejar} "
            f"-F 'torrents=@{REMOTE_ROOT}/peer.torrent;filename=t.torrent' "
            f"-F 'savepath={qb_dl}' {webui}/api/v2/torrents/add",
            lambda s: s.strip().lower().startswith("ok"), tries=4)
        report["phases"]["qb_readd"] = reout.strip() or retry[-1]
        if not okre:
            print(f"[BLOCKED] 复位后加种失败：{retry[-1][:160]}")
            report["blocked_reason"] = "qb re-add failed"
            return finish("blocked", 2)
        info_after = qb_get("/api/v2/torrents/info") or []
        report["phases"]["qb_after_readd"] = info_after
        if len(info_after) != 1:
            print(f"[BLOCKED] 复位后任务数不为 1：{len(info_after)}")
            report["blocked_reason"] = "qb state not reset"
            return finish("blocked", 2)

        peers_file = f"{REMOTE_ROOT}/qb/peers.jsonl"
        peer(f"setsid nohup python3 {REMOTE_ROOT}/qb_peers_pub.py {qb_webui} {cookiejar} "
             f"{infohash} --watch {int(args.timeout) + 90} --interval 0.1 "
             f"> {peers_file} 2>&1 & sleep 1; echo sampler_started")

        seen_tcp: set[str] = set()
        stop = threading.Event()

        def tcp_sampler() -> None:
            # 4 MiB 也可能只连几十秒；0.3 s 一条，用 netstat 而不是 PowerShell（后者要 1 s）。
            while not stop.is_set():
                seen_tcp.update(established_peers(PUBLIC_BT))
                time.sleep(0.3)

        th = threading.Thread(target=tcp_sampler, daemon=True)
        th.start()

        remote_poll = ("for i in $(seq 1 " + str(int(args.timeout)) + "); do "
                       "o=$(curl -sS -m 5 -b " + cookiejar + " " + webui +
                       "/api/v2/torrents/info 2>/dev/null); "
                       "echo TICK; echo \"$o\"; "
                       # 只认 1，不认 0.99x：本文件 16 片，0.992188 = 还差两片。
                       # 上一版写成 `0\\.99[0-9]*`，在 99.2% 就 break，于是「已完成」被写成「没下完」。
                       "echo \"$o\" | grep -qE '\"progress\": *1(\\.0+)?[,}]' && break; "
                       "sleep 1; done; echo POLL_END")
        poll_out, poll_tries = "", []
        try:
            for _attempt in range(3):
                peer.wait_ready(budget=60)
                rc, out = peer(remote_poll, timeout=args.timeout + 90)
                poll_tries.append(f"tick={'TICK' in out} end={'POLL_END' in out} len={len(out)}")
                if "TICK" in out:
                    poll_out = out
                    break
                time.sleep(3)
        finally:
            stop.set()
            th.join(timeout=5)
            peer(f"pkill -f '[q]b_peers_pub.py {qb_webui}' 2>/dev/null; true")
        seen_tcp.update(established_peers(PUBLIC_BT))

        blocks = re.split(r"^TICK$", poll_out, flags=re.M)
        ratios: list[float] = []
        for b in blocks[1:]:
            # 最后一块尾部带着 POLL_END：不剥掉就 json.loads 失败，100% 那一格会被静默丢掉，
            # 读出来永远是「97%」——上一轮就是这么误判的。
            s = b.replace("POLL_END", "").strip()
            if not s:
                continue
            try:
                info = json.loads(s)
            except Exception:
                continue
            if isinstance(info, list) and info:
                ratios.append(float(info[0].get("progress") or 0.0))
        max_ratio = max(ratios) if ratios else -1.0
        # 百分比只用来显示；判定看原始 ratio：qB 收尾时会是 0.9999…，用 int() 截断会读成 99%，
        # 于是「文件完整、SHA-1 一致」的现场被写成「没下完」——这是显示口径，不是产品行为。
        pct = int(round(max_ratio * 100)) if ratios else -1
        report["phases"]["poll_end_seen"] = "POLL_END" in poll_out
        report["phases"]["poll_attempts"] = poll_tries
        report["phases"]["progress_percent"] = [int(round(x * 100)) for x in ratios]
        report["phases"]["final_progress_ratio"] = max_ratio
        report["phases"]["seconds_to_complete"] = len(ratios)
        report["phases"]["final_percent"] = pct
        check("公网客户端报告下载完成（progress ≥ 0.999）", max_ratio >= 0.999,
              f"progress={max_ratio:.6f}, ticks={len(ratios)}" if ratios
              else f"没拿到轮询数据（载体失败），attempts={poll_tries}")

        got, sha_diag = "", []
        for _ in range(4):
            rc, out = peer(f"sha1sum {qb_dl}/{sample_name} 2>/dev/null || echo __NOFILE__")
            m = re.search(r"([0-9a-f]{40})", out)
            if m:
                got = m.group(1)
                break
            sha_diag.append(out.strip()[:120])
            if "__NOFILE__" in out:
                break
            time.sleep(3)
        report["phases"]["peer_sha1"] = got or sha_diag
        check("外部主机落盘 SHA-1 与样本一致", got == sample_sha1,
              f"{got or ('<缺失> ' + json.dumps(sha_diag, ensure_ascii=False)[:140])} vs {sample_sha1}")

        end_seed = (http_json(f"http://127.0.0.1:{PUBLIC_HTTP}/cli",
                              {"command": "seed", "args": []}) or {}).get("seed") or {}
        report["phases"]["end_seed"] = end_seed
        delta = int(end_seed.get("served_bytes") or 0) - served_baseline
        check("供片方计数器增量覆盖全量（数据确实是 A 给的）", delta >= SAMPLE_BYTES,
              f"delta={delta}, total={end_seed.get('served_bytes')}")

        # ---- 客户端自报：peer 地址 + 传输层 ----
        rc, out = peer(f"cat {peers_file} 2>/dev/null")
        samples = []
        for line in out.splitlines():
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                samples.append(json.loads(line))
            except Exception:
                continue
        report["phases"]["qb_peer_sample_count"] = len(samples)
        utp_pub = [p for s in samples for p in (s.get("peers") or [])
                   if str(p.get("conn", "")).lower() == "utp"
                   and p.get("ip") == wan and int(p.get("port") or 0) == PUBLIC_BT]
        tcp_any = [p for s in samples for p in (s.get("peers") or [])
                   if "tcp" in str(p.get("conn", "")).lower()]
        report["phases"]["qb_utp_public_peer"] = utp_pub[:3]
        report["phases"]["qb_tcp_peer"] = tcp_any[:3]
        report["a_side_tcp_peers"] = sorted(seen_tcp)

        check("客户端自报 peer 就是公网端点且走 µTP",
              bool(utp_pub),
              f"hits={len(utp_pub)}, first={json.dumps(utp_pub[0], ensure_ascii=False) if utp_pub else '<无>'}"
              f"（样本 {len(samples)}）")
        check("客户端全程没有用 TCP（BTProtocol=2 生效）",
              len(samples) > 0 and not tcp_any,
              f"samples={len(samples)}, tcp_peers={len(tcp_any)}")
        public_tcp = [ip for ip in seen_tcp if not is_private(ip)]
        lan_tcp = [ip for ip in seen_tcp if is_private(ip)]
        report["path_evidence"] = {"public_tcp": public_tcp, "lan_tcp": lan_tcp}
        check("A 侧 bt_port 上没有来自公网的 TCP 连接（数据只走 UDP）",
              not public_tcp, f"public_tcp={public_tcp}")
        check("A 侧 bt_port 上没有来自 LAN 的 TCP 连接（没有走内网捷径）",
              not lan_tcp, f"lan_tcp={lan_tcp}")

        peer(f"curl -sS -m 10 -b {cookiejar} -X POST --data 'hashes={infohash}&deleteFiles=true' "
             f"{webui}/api/v2/torrents/delete 2>/dev/null; "
             f"pkill -f '[q]bittorrent-nox.*{qb_webui}' 2>/dev/null; true")

        all_ok = all(c["ok"] for c in report["checks"])
        passed = sum(1 for c in report["checks"] if c["ok"])
        report["summary"] = f"{passed}/{len(report['checks'])} PASS"
        return finish("pass" if all_ok else "fail", 0 if all_ok else 1)

    finally:
        if echo_proc is not None:
            try:
                echo_proc.terminate()
                echo_proc.wait(timeout=5)
            except Exception:
                try:
                    echo_proc.kill()
                except Exception:
                    pass
        try:
            peer(f"curl -sS -m 8 -b {REMOTE_ROOT}/qb/cookies.txt -X POST "
                 f"--data 'hashes=all' http://127.0.0.1:18180/api/v2/torrents/delete 2>/dev/null; "
                 f"pkill -f '[q]bittorrent-nox.*18180' 2>/dev/null; "
                 f"pkill -f '[q]b_peers_pub.py' 2>/dev/null; true", timeout=30)
        except Exception:
            pass
        peer.close()
        for p in procs:
            try:
                p.terminate()
            except Exception:
                pass
        for lf in logs:
            try:
                lf.close()
            except Exception:
                pass
        for p in procs:
            try:
                p.wait(timeout=10)
            except Exception:
                try:
                    p.kill()
                except Exception:
                    pass
        if not args.keep:
            import shutil
            shutil.rmtree(work, ignore_errors=True)
            try:
                peer(f"rm -rf {REMOTE_ROOT}", timeout=30)
            except Exception:
                pass
        else:
            print(f"工作目录保留: {work}")


if __name__ == "__main__":
    sys.exit(main())
