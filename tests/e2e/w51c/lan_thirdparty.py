#!/usr/bin/env python3
"""W5.1c 验收：LAN 真实网络复核 —— 第三方客户端越过真实网卡，从 A 的引擎取片。

为什么要单独一条：W5.1（引擎下载）与 W5.1b（引擎做种 → 本机 aria2）都跑在
**127.0.0.1 回环**上。回环不经过网卡、不经过操作系统入向栈，所以「端口在听但
外部连不上」「防火墙/绑定地址写错」「tracker 把 127.0.0.1 当 peer 通告出去」
这三类问题在回环上**全部看不见**。本脚本把下载方搬到 192.168.1.221，走真实网卡。

判「通过」六条缺一不可：
    ① 221 → A 的 HTTP（/health、/torrent/<infohash>）真实可达且取到字节
    ② 伴随种子的 announce 指向 A 的 **LAN** 地址（不是 127.0.0.1）——
       否则 221 会去 announce 自己的回环，永远等不到 peer
    ③ COLD 负对照：唤醒前第三方客户端拿不到数据（端口在听、引擎暂停）
    ④ 唤醒后 A 状态转 hot
    ⑤ 第三方客户端完整下载，落盘 SHA-1 与样本逐字节一致
    ⑥ 供片方对账：A 引擎 served_bytes 增量 >= 样本字节数

另有一条身份检查：A 的日志必须有「做种引擎就绪」，防止跑旧二进制自欺。

边界（写进报告，不允许含糊）：
    - 这是 **LAN 跨主机**复核，`evidence_scope=lan-cross-host`、`is_s6_evidence=false`；
    - 不含任何可达率数字，也不构成 G2/G3 的新证据；
    - 公网入向（NAT 打洞 / 端口映射）仍未测。

用法：
    python tests/e2e/w51c/lan_thirdparty.py
    python tests/e2e/w51c/lan_thirdparty.py --keep
    python tests/e2e/w51c/lan_thirdparty.py --clients transmission

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
COLD_PROBE_SECONDS = 10         # 负对照：未唤醒时给客户端 10 秒
DOWNLOAD_TIMEOUT = 180.0

A_HOST = "192.168.1.210"
PEER = os.environ.get("CR_PEER", "192.168.1.221")
SSH_KEY = os.environ.get("CR_SSH_KEY") or str(
    Path(os.environ.get("TEMP", "/tmp")) / "crkeys" / "id_ed25519")

# 221 侧的专用端口/目录（不复用系统上的 transmission / qbittorrent 配置）
TX_RPC_PORT = 19091
TX_PEER_PORT = 19501
QB_WEBUI_PORT = 18080
QB_PEER_PORT = 19500
REMOTE_ROOT = "/tmp/cr-lan-w51c"


# ---------------------------------------------------------------------------
# 基础工具
# ---------------------------------------------------------------------------
def run(cmd: list[str], timeout: float = 60.0, cwd: str | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, capture_output=True, timeout=timeout, cwd=cwd)


def ssh(cmd: str, timeout: float = 60.0) -> tuple[int, str]:
    """在 221 上跑一条命令，合并 stdout/stderr 返回。"""
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
    seed = b"coldrevive-w51c-lan-sample"
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
    """健康检查。via_peer=True 时**从 221 发起**，这样返回值本身就是 LAN 可达的证据。"""
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
    """从 `transmission-remote -l` 的表格里取第一行的 Done 百分比。

    行格式：ID  Done  Have  ETA  Up  Down  Ratio  Status  Name
    只认数字行（表头以 ID 开头，跳过）。
    """
    for line in listing.splitlines():
        parts = line.split()
        if not parts or not parts[0].isdigit() or len(parts) < 2:
            continue
        m = re.match(r"([\d.]+)%", parts[1])
        if m:
            return int(float(m.group(1)))
    return -1


# ---------------------------------------------------------------------------
# 主流程
# ---------------------------------------------------------------------------
def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--keep", action="store_true", help="保留工作目录便于排查")
    ap.add_argument("--clients", default="transmission,qbittorrent",
                    help="参与复核的第三方客户端，逗号分隔")
    ap.add_argument("--a-host", default=A_HOST, help="A 的 LAN 地址（announce 与 peer 通告用它）")
    args = ap.parse_args()
    clients = [c.strip() for c in args.clients.split(",") if c.strip()]

    daemon = os.environ.get("COLDREVIVE_DAEMON") or str(
        REPO / "target" / "debug" / ("coldrevived.exe" if os.name == "nt" else "coldrevived"))
    if not Path(daemon).is_file():
        print(f"[BLOCKED] 找不到守护进程 {daemon}")
        return 2

    work = Path(REPO / "tests" / "e2e" / "w51c" / "reports" / f"run-{time.strftime('%Y%m%dT%H%M%SZ')}")
    work.mkdir(parents=True, exist_ok=True)
    report: dict = {
        "schema": "coldrevive.w51c.lan-thirdparty/v1",
        "generated_at_unix": int(time.time()),
        "evidence_scope": "lan-cross-host",
        "is_s6_evidence": False,
        "contains_success_rate": False,
        "seeder": f"cr-engine / librqbit（A @ {args.a_host}）",
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
    try:
        # ---------------- 0. 预检 ----------------
        print("== 0. 预检 ==")
        rc, out = ssh("echo ok; command -v transmission-daemon transmission-remote qbittorrent-nox curl sha1sum")
        if rc != 0 or "ok" not in out:
            print(f"[BLOCKED] 无法登录 {PEER}: {out[:300]}")
            report["status"] = "blocked"
            return 2
        have = {n: (n in out) for n in ("transmission-daemon", "transmission-remote", "qbittorrent-nox")}
        report["peer_tools"] = have
        if "transmission" in clients and not (have["transmission-daemon"] and have["transmission-remote"]):
            print("[BLOCKED] 221 缺少 transmission-daemon / transmission-remote")
            return 2
        if "qbittorrent" in clients and not have["qbittorrent-nox"]:
            print("[BLOCKED] 221 缺少 qbittorrent-nox")
            return 2

        # 清场：只杀我们自己的实例，用方括号模式避免 pkill 命中自己的 ssh 命令
        ssh(f"pkill -f '[t]ransmission-daemon -g {REMOTE_ROOT}' ; "
            f"pkill -f '[q]bittorrent-nox.*{QB_WEBUI_PORT}' ; "
            f"rm -rf {REMOTE_ROOT} ; mkdir -p {REMOTE_ROOT}")
        rc, out = ssh(f"mkdir -p {REMOTE_ROOT} && echo ready")
        if "ready" not in out:
            print(f"[BLOCKED] 221 无法准备目录: {out[:200]}")
            return 2

        http_port = pick_port(18780)
        wake_port = pick_port(18781, socket.SOCK_DGRAM)
        bt_port = pick_port(18782)
        b_http = pick_port(http_port + 4)
        b_wake = pick_port(wake_port + 4, socket.SOCK_DGRAM)
        b_bt = pick_port(bt_port + 4)
        report["ports"] = {"http": http_port, "wake_udp": wake_port, "bt": bt_port}

        # ---------------- 1. 起 A / B ----------------
        print("== 1. 启动 A（LAN 绑定）与 B ==")
        sample_name = "coldrevive-w51c-lan-sample.bin"
        sample = work / sample_name
        data = deterministic_sample(SAMPLE_BYTES)
        sample.write_bytes(data)
        sample_sha1 = sha1_hex(data)
        report["sample"]["sha1"] = sample_sha1
        report["sample"]["name"] = sample_name

        nodes = {
            # A 的 host 必须是 LAN 地址：它同时决定 ① 种子里的 announce ② tracker 通告的 peer
            "A": {"host": args.a_host, "http_port": http_port,
                  "wake_udp_port": wake_port, "bt_port": bt_port},
            "B": {"host": "127.0.0.1", "http_port": b_http,
                  "wake_udp_port": b_wake, "bt_port": b_bt},
        }
        cfg = {
            "bind": "0.0.0.0",
            "nodes": nodes,
            "sample_path": str(sample),
            "keywords": ["w51c-lan"],
            "namespace": "ColdRevive_V1:",
            "workdir": str(work),
            "golden_req": str(work / "wakeup_req.v1.json"),
            "golden_ack": str(work / "wakeup_ack.v1.json"),
            "downloader": "",
            "challenge_window_sec": 300,
        }
        cfg_path = work / "w51c-nodes.json"
        cfg_path.write_text(json.dumps(cfg, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

        # 两个进程都绑 0.0.0.0：B 的唤醒 socket 若绑 127.0.0.1，发往 LAN 地址的包
        # 会带环回源地址，A 的 ACK 就回不到它（回环 ≠ 网卡）。
        for node in ("A", "B"):
            lf = open(work / f"daemon-{node}.log", "wb")
            logs.append(lf)
            procs.append(subprocess.Popen(
                [daemon, "--config", str(cfg_path), "--node-id", node,
                 "--bind", "0.0.0.0", "--workdir", str(work)],
                stdout=lf, stderr=subprocess.STDOUT, cwd=str(work)))

        a_health = f"http://{args.a_host}:{http_port}/health"
        b_health = f"http://127.0.0.1:{b_http}/health"
        ok_a = wait_http(a_health, 30.0, via_peer=True)
        ok_b = wait_http(b_health, 30.0, via_peer=False)
        if not check("221 越过网卡访问 A 的 /health", ok_a, a_health):
            report["status"] = "fail"
            return 1
        if not check("B 本地 /health", ok_b, b_health):
            report["status"] = "fail"
            return 1

        a_log = (work / "daemon-A.log").read_text(encoding="utf-8", errors="replace")
        engine_ready = "做种引擎就绪" in a_log
        legacy = "标准 BT 种子端监听" in a_log
        report["a_log_excerpt"] = [ln for ln in a_log.splitlines() if "做种引擎" in ln or "种子端" in ln]
        check("A 的做种由引擎承担（非旧手写种子端）", engine_ready and not legacy,
              f"engine_ready={engine_ready}, legacy={legacy}")

        listen_line = next((ln for ln in a_log.splitlines() if "做种引擎就绪" in ln), "")
        m = re.search(r"实际 Some\(([\d\.]+):(\d+)\)", listen_line)
        actual_listen = f"{m.group(1)}:{m.group(2)}" if m else ""
        report["engine_listen"] = actual_listen
        check("引擎实际监听在 LAN 可达地址（非仅回环）", actual_listen.endswith(f":{bt_port}"),
              actual_listen or listen_line)

        # ---------------- 2. 取种子，核对 announce ----------------
        print("== 2. 从 221 取伴随种子并核对 announce ==")
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

        rc, out = ssh(f"cat {remote_torrent}")
        remote_bytes = out.encode("latin-1", "replace")
        announce = ""
        mm = re.search(r"8:announce(\d+):", out[:400])
        if mm:
            n = int(mm.group(1))
            start = mm.end()
            announce = out[start:start + n]
        report["torrent_announce"] = announce
        check("种子 announce 指向 A 的 LAN 地址（不是回环）",
              announce.startswith(f"http://{args.a_host}:{http_port}/"),
              announce or "<未解析到 announce>")

        # ---------------- 3. Transmission 阶段 ----------------
        if "transmission" in clients:
            print("== 3. Transmission 3.00 阶段 ==")
            tx_peer = remote_free_port(TX_PEER_PORT)
            tx_conf = f"{REMOTE_ROOT}/tx"
            tx_dl = f"{tx_conf}/dl"
            ssh(f"mkdir -p {tx_conf} {tx_dl}")

            # 先单独验一次 tracker：从 221 发标准 announce，看回来的 peer 是不是 A 的 LAN 端点。
            # 这一条把「客户端不通」拆成两半：tracker 通告错 vs 连接/握手不通。
            ih_param = urllib.parse.quote_from_bytes(bytes.fromhex(infohash))
            q = (f"info_hash={ih_param}"
                 f"&peer_id={urllib.parse.quote_from_bytes(b'cr-w51c-probe-000000000')}"
                 f"&port={tx_peer}&uploaded=0&downloaded=0&left={SAMPLE_BYTES}"
                 f"&compact=1&numwant=20")
            rc, hexout = ssh(f"curl -sS -m 8 'http://{args.a_host}:{http_port}/announce?{q}' | xxd -p | tr -d '\\n'")
            raw_hex = re.sub(r"[^0-9a-f]", "", hexout)
            peer_ip, peer_port = "", 0
            marker = "353a7065657273363a"   # "5:peers6:"
            idx = raw_hex.find(marker)
            if idx >= 0:
                blob = bytes.fromhex(raw_hex[idx + len(marker): idx + len(marker) + 12])
                peer_ip = ".".join(str(b) for b in blob[:4])
                peer_port = int.from_bytes(blob[4:], "big")
            report["tracker_announce"] = {"peer_ip": peer_ip, "peer_port": peer_port,
                                           "raw_prefix": raw_hex[:120]}
            check("tracker 对 221 通告的 peer 是 A 的 LAN 端点",
                  peer_ip == args.a_host and peer_port == bt_port,
                  f"{peer_ip}:{peer_port}（期望 {args.a_host}:{bt_port}）")

            # 标准客户端加种子后会先 scrape。以前这里 404，Transmission 日志会一直报
            # 「Scrape error: 404」并退避，看起来像 tracker 坏了。
            rc, code_out = ssh(f"curl -sS -m 8 -o /dev/null -w '%{{http_code}}' "
                               f"'http://{args.a_host}:{http_port}/scrape?info_hash={ih_param}'")
            check("tracker /scrape 返回 200（不再是 404）", code_out.strip() == "200",
                  f"http={code_out.strip() or '<无响应>'}")

            # 本轮显式把客户端降到 TCP，**只验 TCP 路径**——不是为了让测试好过。
            # 当时的理由是抓包看到 Transmission 先试 µTP（UDP 19501→18782）而没发 SYN，
            # 干等很久才回落，现场像「连上了但没数据」；而当时的引擎确实只开了 TCP
            # （librqbit 的 `ListenerOptions` 默认 `TcpOnly`）。
            # **2026-11-17 W5.1e 更正**：不是「librqbit 不支持 µTP」，是我们没打开开关。
            # µTP 路径见 tests/e2e/w51e/utp_thirdparty.py；本脚本保留 TCP-only 姿态。
            ssh("cat > " + f"{tx_conf}/settings.json <<'TXCFG'\n"
                + json.dumps({
                    "utp-enabled": False,
                    "dht-enabled": False,
                    "pex-enabled": False,
                    "lpd-enabled": False,
                    "port-forwarding-enabled": False,
                    "encryption": 0,          # 0 = tolerated：不强制 MSE（引擎不做加密握手）
                    "rpc-port": TX_RPC_PORT,
                    "peer-port": tx_peer,
                    "download-dir": tx_dl,
                }, indent=2) + "\nTXCFG")
            report["client_settings"] = {"utp_enabled": False,
                                         "reason": "本脚本只验 TCP 路径；µTP 由 W5.1e 单独验"}
            ssh_bg(f"transmission-daemon -g {tx_conf} -f >{tx_conf}/daemon.log 2>&1")
            rc, out = "", ""
            for _ in range(40):
                rc, out = ssh(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} --list")
                if "Sum:" in out:
                    break
                time.sleep(0.5)
            if "Sum:" not in out:
                check("Transmission RPC 就绪", False, out[-300:])
            else:
                check("Transmission RPC 就绪", True, f"port={TX_RPC_PORT}")

            ssh(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} --add {remote_torrent}")
            time.sleep(2.0)
            rc, out = ssh(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} --list")
            report["phases"]["transmission_cold_list"] = out.strip()

            # 负对照：未唤醒时应当拿不到数据（端口在听、引擎暂停）。
            # 不读目录大小——Transmission 会在 add 时建稀疏文件，目录看起来是满额；
            # 读客户端自己的 Done 百分比 + 供片方的 served_bytes，两个计数器都要是 0。
            time.sleep(COLD_PROBE_SECONDS)
            rc, listing = ssh(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} -l")
            cold_pct = parse_tx_percent(listing)
            cold_seed = (http_json(f"http://127.0.0.1:{http_port}/cli",
                                   {"command": "seed", "args": []}) or {}).get("seed") or {}
            report["phases"]["transmission_cold"] = {
                "client_percent": cold_pct, "client_list": listing.strip(), "seed": cold_seed}
            check("COLD 负对照：唤醒前第三方客户端拿不到数据",
                  cold_pct == 0 and cold_seed.get("state") == "cold"
                  and not cold_seed.get("seeding_hot")
                  and int(cold_seed.get("served_bytes") or 0) == 0,
                  f"client_done={cold_pct}%, state={cold_seed.get('state')}, "
                  f"served={cold_seed.get('served_bytes')}")

            # 唤醒
            t0 = time.time()
            ack = http_json(f"http://127.0.0.1:{b_http}/cli",
                            {"command": "wake", "args": ["ack", file_hash, infohash]})
            report["phases"]["wake"] = {"elapsed_ms": int((time.time() - t0) * 1000), "ack": ack}
            hot = (http_json(f"http://127.0.0.1:{http_port}/cli",
                             {"command": "seed", "args": []}) or {}).get("seed") or {}
            report["phases"]["after_wake"] = hot
            check("唤醒后 A 的引擎转入 hot", hot.get("state") == "hot" and bool(hot.get("seeding_hot")),
                  json.dumps(hot, ensure_ascii=False))

            # 冷相里客户端连过 A（端口在听但引擎暂停），连接被拒后它会进入退避，
            # 再等它也只会看到旧的失败记录。删掉重加，让正相从干净的连接状态开始——
            # 负对照证明的是「供片方不供」，不是「客户端不会重试」。
            ssh(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} -t 1 -r 2>/dev/null; sleep 1")
            ssh(f"rm -rf {tx_dl}/* && transmission-remote 127.0.0.1:{TX_RPC_PORT} --add {remote_torrent}")
            time.sleep(1.5)

            # 等下载完成
            deadline = time.time() + DOWNLOAD_TIMEOUT
            last, pct = "", 0
            while time.time() < deadline:
                rc, out = ssh(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} -l")
                last = out.strip()
                pct = parse_tx_percent(last)
                if pct >= 100:
                    break
                time.sleep(1.0)
            report["phases"]["transmission_final_list"] = last
            report["phases"]["transmission_final_percent"] = pct
            check("Transmission 报告下载完成 100%", pct >= 100, f"done={pct}%")
            rc, out = ssh(f"ls -l {tx_dl}; sha1sum {tx_dl}/{sample_name} 2>/dev/null")
            report["phases"]["transmission_files"] = out.strip()
            got_sha1 = ""
            m = re.search(r"([0-9a-f]{40})", out)
            if m:
                got_sha1 = m.group(1)
            check("Transmission 落盘 SHA-1 与样本一致", got_sha1 == sample_sha1,
                  f"{got_sha1 or '<缺失>'} vs {sample_sha1}")

            end_seed = (http_json(f"http://127.0.0.1:{http_port}/cli",
                                  {"command": "seed", "args": []}) or {}).get("seed") or {}
            report["phases"]["transmission_seed"] = end_seed
            check("Transmission 阶段供片方计数器覆盖全量",
                  int(end_seed.get("served_bytes") or 0) >= SAMPLE_BYTES,
                  json.dumps(end_seed, ensure_ascii=False))
            tx_served = int(end_seed.get("served_bytes") or 0)

            ssh(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} --exit 2>/dev/null; sleep 0.5")
        else:
            tx_served = 0

        # ---------------- 4. qBittorrent 阶段 ----------------
        if "qbittorrent" in clients:
            print("== 4. qBittorrent 4.5.1 阶段 ==")
            qb_cfg = f"{REMOTE_ROOT}/qb/cfg"
            qb_data = f"{REMOTE_ROOT}/qb/data"
            qb_dl = f"{REMOTE_ROOT}/qb/dl"
            ssh(f"mkdir -p {qb_cfg}/qBittorrent {qb_data} {qb_dl}")
            # 4.5.1 首次启动会生成随机临时密码；直接写入已知口令的 PBKDF2，避免解析日志
            salt = hashlib.sha256(b"coldrevive-w51c-qb-salt").digest()[:16]
            key = hashlib.pbkdf2_hmac("sha512", b"adminadmin", salt, 100000)
            pw = base64.b64encode(salt).decode() + ":" + base64.b64encode(key).decode()
            conf = (f"[BitTorrent]\n"
                    f"Session\\BTProtocol=1\n"          # 1 = 仅 TCP（2=仅 uTP, 3=两者）
                    f"Session\\DHTEnabled=false\n"
                    f"Session\\PeXEnabled=false\n"
                    f"Session\\LSDEnabled=false\n"
                    f"Session\\Port={QB_PEER_PORT}\n"
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
            check("qBittorrent WebUI 可登录", ready and logged_in, f"ready={ready}, login={out.strip()[:40]}")
            if logged_in:
                ssh(f"curl -sS -m 5 -b {cookiejar} --data-urlencode "
                    f"'json={{\"dht\":false,\"pex\":false,\"lsd\":false,\"listen_port\":{QB_PEER_PORT}}}' "
                    f"{webui}/api/v2/app/setPreferences")
                rc, out = ssh(f"curl -sS -m 15 -b {cookiejar} -F 'torrents=@{remote_torrent};filename=t.torrent' "
                              f"-F 'savepath={qb_dl}' {webui}/api/v2/torrents/add")
                added = out.strip().lower().startswith("ok")
                check("qBittorrent 接受种子", added, out.strip()[:60])

                deadline = time.time() + DOWNLOAD_TIMEOUT
                info = []
                while time.time() < deadline:
                    rc, out = ssh(f"curl -sS -m 5 -b {cookiejar} {webui}/api/v2/torrents/info")
                    try:
                        info = json.loads(out)
                    except Exception:
                        info = []
                    if info and float(info[0].get("progress") or 0) >= 0.999:
                        break
                    time.sleep(1.0)
                report["phases"]["qb_torrent_info"] = info[:1]
                rc, out = ssh(f"sha1sum {qb_dl}/{sample_name} 2>/dev/null; ls -l {qb_dl}")
                report["phases"]["qb_files"] = out.strip()
                m = re.search(r"([0-9a-f]{40})", out)
                qb_sha1 = m.group(1) if m else ""
                check("qBittorrent 落盘 SHA-1 与样本一致", qb_sha1 == sample_sha1,
                      f"{qb_sha1 or '<缺失>'} vs {sample_sha1}")

                end_seed = (http_json(f"http://127.0.0.1:{http_port}/cli",
                                      {"command": "seed", "args": []}) or {}).get("seed") or {}
                report["phases"]["qb_seed"] = end_seed
                delta = int(end_seed.get("served_bytes") or 0) - tx_served
                check("qBittorrent 阶段供片方计数器增量覆盖全量", delta >= SAMPLE_BYTES,
                      f"delta={delta}, total={end_seed.get('served_bytes')}")

            ssh(f"pkill -f '[q]bittorrent-nox.*{QB_WEBUI_PORT}' ; sleep 0.5")

        # ---------------- 5. 收尾 ----------------
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
        if "transmission" in clients:
            ssh(f"transmission-remote 127.0.0.1:{TX_RPC_PORT} --exit 2>/dev/null; "
                f"pkill -f '[t]ransmission-daemon -g {REMOTE_ROOT}' 2>/dev/null; true", timeout=20)
        ssh(f"pkill -f '[q]bittorrent-nox.*{QB_WEBUI_PORT}' 2>/dev/null; true", timeout=20)
        out = REPO / "tests" / "e2e" / "w51c" / "reports" / f"lan-thirdparty-{time.strftime('%Y%m%dT%H%M%SZ')}.json"
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
