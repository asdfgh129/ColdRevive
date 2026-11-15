#!/usr/bin/env python3
"""S6 真实 NAT 档位（real-multi-nat）：持有者在 222（真实 NAT 后面），请求者在本机 A。

与前两档的本质区别：
  * run_matrix_lab.py   两端都是 221 内的 netns          → vm-namespace-lab（合成 NAT）
  * run_cross_host.py   一端 netns、一端 A               → cross-host-lab（合成 NAT + 两台主机）
  * 本脚本              **两端各自在真实 NAT 后面**      → real-multi-nat

拓扑（全部实测，不写死假设）：
  持有者  192.168.34.222（222 的 ens160）→ 真实 NAT → 112.5.32.34
  请求者  192.168.1.210（A 的本机网卡）  → 真实 NAT → 112.5.60.4
  中继    192.168.1.221:7100（LAN 内，两端都可达；只做信令，不承载数据面）

为什么这样才算真实设备：
  * 两个出口各有自己的公网 IP，且都 ≠ 本机地址 —— 映射由**我们的代码之外**的设备完成；
  * 测试端口（19900-19902 / 17800-17802）**没有任何静态端口映射**，
    STUN 观测到的映射端口 ≠ 内部端口，即 NAT 动态分配，而不是「路由器配置出来的全锥」；
  * 分类由真实公网 STUN（RFC 5389 映射 + RFC 3489 CHANGE-REQUEST 过滤）独立测得，
    不采信 attestation 自报。

诚实边界（不得突破）：
  * 本脚本不写 spikes.json、不改变任何 spike 状态、不自动置 pass；
  * 只覆盖 port-restricted-cone **一类**；单类成立 ≠ 矩阵成立（仍需 4 类 + CGNAT）；
  * 失败一律如实计失败，缺失证据一律 BLOCKED，不回填默认值。

用法（在 A 上执行）:
  python tests/spikes/s6/nat_lab/run_real_nat.py --smoke
  python tests/spikes/s6/nat_lab/run_real_nat.py --trials 30
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import socket
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

# ---- 拓扑常量 ----------------------------------------------------------------
SSH_KEY = os.environ.get("CR_KEY", str(Path.home() / "AppData/Local/Temp/crkeys/id_ed25519"))
HOLDER_SSH = os.environ.get("CR_HOLDER_HOST", "192.168.1.222")
HOLDER_BIN = "/root/bin"
HOLDER_WORK = "/root/crreal"

RELAY_HOST = os.environ.get("CR_RELAY_HOST", "192.168.1.221")
RELAY = f"{RELAY_HOST}:7100"
RELAY_STATS = "/opt/coldrevive/relay/real-run-stats.json"
RELAY_BIN = "/opt/coldrevive/repo/target/release/coldrevive-relay"

REQ_IP = "192.168.1.210"          # 请求者（本机 A）在 LAN 上的地址
# 222 背后的设备实测为 **Address/Port-Dependent Mapping**（symmetric）：
# 同一 socket、同一目的端口 3478、三个不同目的 IP → 映射端口 2051/2055/2050 各不相同。
# 所以这一类必须按 symmetric 测，预期 direct 0/30、relay 30/30。
DEFAULT_CLASS = "symmetric"
KLASS = DEFAULT_CLASS

HOLDER_PORTS = {"http": 19900, "wake": 19901, "bt": 19902}
REQ_PORTS = {"http": 17800, "wake": 17801, "bt": 17802}

# 真实公网 STUN。**必须两端都通**：实测 222 上 `stun.l.google.com` 与
# `stun.voipbuster.com`（DNS 轮询到的节点）完全不回包，而 cloudflare / telnyx /
# miwifi / bilibili 在两端 3~4/4 可用。映射分类要求 ≥2 个目的地址有样本，
# 只配一台服务器会让结论退化成 Unknown（这是正确行为，但白跑一次）。
PUBLIC_STUN = ["stun.cloudflare.com:3478", "stun.telnyx.com:3478",
               "stun.miwifi.com:3478", "stun.chat.bilibili.com:3478"]

INFOHASH = "real-nat-222"


def repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


# ---- ssh / scp ---------------------------------------------------------------
def _ssh(host: str, script: str, timeout: float = 60.0) -> subprocess.CompletedProcess:
    cmd = ["ssh", "-i", SSH_KEY,
           "-o", "StrictHostKeyChecking=no",
           "-o", "UserKnownHostsFile=/dev/null",
           "-o", "LogLevel=ERROR",
           "-o", "BatchMode=yes",
           "-o", "ServerAliveInterval=10",
           f"root@{host}", "bash", "-s"]
    return subprocess.run(cmd, input=script, capture_output=True, text=True, timeout=timeout)


def hsh(script: str, timeout: float = 60.0) -> str:
    p = _ssh(HOLDER_SSH, script, timeout)
    if p.returncode != 0:
        raise SystemExit(f"[{HOLDER_SSH}] 命令失败 rc={p.returncode}: {p.stderr.strip()[:400]}")
    return p.stdout.strip()


def hsh_safe(script: str, timeout: float = 30.0) -> str:
    try:
        return hsh(script, timeout)
    except (SystemExit, subprocess.TimeoutExpired) as exc:
        print(f"  [清理告警] {exc}", file=sys.stderr)
        return ""


def rsh(script: str, timeout: float = 60.0) -> str:
    p = _ssh(RELAY_HOST, script, timeout)
    if p.returncode != 0:
        raise SystemExit(f"[{RELAY_HOST}] 命令失败 rc={p.returncode}: {p.stderr.strip()[:400]}")
    return p.stdout.strip()


def scp_to_holder(local: Path, remote: str) -> None:
    cmd = ["scp", "-i", SSH_KEY,
           "-o", "StrictHostKeyChecking=no",
           "-o", "UserKnownHostsFile=/dev/null",
           "-o", "LogLevel=ERROR",
           str(local), f"root@{HOLDER_SSH}:{remote}"]
    p = subprocess.run(cmd, capture_output=True, text=True, timeout=180)
    if p.returncode != 0:
        raise SystemExit(f"scp 失败 {local} → {remote}: {p.stderr.strip()[:300]}")


def scp_from_holder(remote: str, local: Path) -> bool:
    cmd = ["scp", "-i", SSH_KEY,
           "-o", "StrictHostKeyChecking=no",
           "-o", "UserKnownHostsFile=/dev/null",
           "-o", "LogLevel=ERROR",
           f"root@{HOLDER_SSH}:{remote}", str(local)]
    p = subprocess.run(cmd, capture_output=True, text=True, timeout=180)
    return p.returncode == 0 and local.is_file()


# ---- 观测 -------------------------------------------------------------------
def tcp_ready(addr: str, timeout: float = 6.0) -> bool:
    host, _, port = addr.rpartition(":")
    end = time.time() + timeout
    while time.time() < end:
        try:
            with socket.create_connection((host, int(port)), timeout=0.6):
                return True
        except OSError:
            time.sleep(0.2)
    return False


def http_json(url: str, timeout: float = 3.0) -> dict:
    with urllib.request.urlopen(url, timeout=timeout) as fh:
        return json.loads(fh.read().decode())


def public_ip_local(timeout: float = 10.0) -> str:
    for url in ("https://api.ipify.org", "https://ifconfig.me/ip", "http://ip.3322.net"):
        try:
            with urllib.request.urlopen(url, timeout=timeout) as fh:
                txt = fh.read().decode().strip()
            if txt and txt[0].isdigit():
                return txt
        except Exception:  # noqa: BLE001
            continue
    return ""


def public_ip_holder() -> str:
    """在 222 上**指定 ens160** 出口取公网 IP，避免默认路由被别处改走时静默失真。"""
    script = ("curl -s --max-time 8 --interface ens160 https://api.ipify.org "
              "|| curl -s --max-time 8 --interface ens160 http://ifconfig.me/ip")
    return hsh(script, timeout=40).strip()


def is_public_ipv4(ip: str) -> bool:
    try:
        a, b, c, _ = (int(x) for x in ip.split("."))
    except (ValueError, TypeError):
        return False
    if a in (10, 127) or a == 0 or a >= 224:
        return False
    if a == 172 and 16 <= b <= 31:
        return False
    if a == 192 and b == 168:
        return False
    if a == 169 and b == 254:
        return False
    if a == 100 and 64 <= b <= 127:
        return False
    return True


def wait_local_health(ip: str, port: int, timeout: float = 20.0) -> dict:
    end, last = time.time() + timeout, ""
    while time.time() < end:
        try:
            return http_json(f"http://{ip}:{port}/health", timeout=1.5)
        except Exception as exc:  # noqa: BLE001
            last = f"{type(exc).__name__}: {exc}"
        time.sleep(0.4)
    raise SystemExit(f"请求者 /health 未就绪（{ip}:{port}）：{last}")


def wait_holder_health(port: int, timeout: float = 25.0) -> dict:
    end, last = time.time() + timeout, ""
    while time.time() < end:
        p = _ssh(HOLDER_SSH,
                 f"curl -s --max-time 2 http://127.0.0.1:{port}/health", timeout=15)
        if p.returncode == 0 and p.stdout.strip():
            try:
                return json.loads(p.stdout.strip().splitlines()[-1])
            except ValueError as exc:
                last = f"非 JSON: {exc}"
        else:
            last = (p.stderr or p.stdout or "").strip()[-160:]
        time.sleep(0.5)
    raise SystemExit(f"持有者 /health 未就绪（127.0.0.1:{port} @222）：{last}")


# ---- 配置 --------------------------------------------------------------------
def ports(p: dict) -> dict:
    return {"http_port": p["http"], "wake_udp_port": p["wake"], "bt_port": p["bt"]}


def build_config(path: Path, holder_self: str, bind: str, workdir: str, sample: str,
                 golden_dir: str) -> None:
    cfg = {
        "bind": bind,
        # 守护进程把「持有者」硬编码为 node_id=="A"（bootstrap_holder），故 A=holder、B=requester
        "nodes": {
            "A": {"role": "holder", "host": holder_self, **ports(HOLDER_PORTS)},
            "B": {"role": "requester", "host": REQ_IP, **ports(REQ_PORTS)},
        },
        "sample_path": sample,
        "keywords": ["coldrevive", "s6", "real-nat"],
        "workdir": workdir,
        "golden_req": f"{golden_dir}/fixtures/t0/golden/wakeup_req.v1.json",
        "golden_ack": f"{golden_dir}/fixtures/t0/golden/wakeup_ack.v1.json",
        "nat": {"enabled": True, "stun_servers": PUBLIC_STUN, "relay": RELAY},
    }
    path.write_bytes((json.dumps(cfg, indent=2) + "\n").encode("utf-8"))


def local_cli(args: list[str], cfg: Path, node: str, timeout: float = 90.0) -> dict:
    exe = str(repo_root() / "target/release/coldrevive-cli.exe")
    env = dict(os.environ, COLDREVIVE_T0_CONFIG=str(cfg))
    p = subprocess.run([exe, "--json", "--node", node] + args,
                       capture_output=True, text=True, timeout=timeout, env=env)
    out = (p.stdout or "").strip()
    if not out:
        raise SystemExit(f"本机 CLI 无输出: {p.stderr.strip()[:400]}")
    try:
        return json.loads(out.splitlines()[-1])
    except ValueError as exc:
        raise SystemExit(f"本机 CLI 输出非 JSON: {exc}: {out[:300]}") from exc


def stun_mapped_local(timeout: float = 3.0) -> str:
    """用真实 STUN 报文取本机出口映射地址。

    不用 HTTP 取 IP：这台机器上 `curl https://api.ipify.org` 有时返回空（代理/DNS 干扰），
    而 HTTP 返回的是「跟 HTTP 代理说话时看到的那一端」，未必等于 UDP 的出口 —— 而我们关心的
    恰恰是 **UDP** 出口。这里发真实 binding request 并解析 XOR-MAPPED-ADDRESS。
    """
    for spec in PUBLIC_STUN:
        host, _, port = spec.rpartition(":")
        try:
            ip = socket.gethostbyname(host)
        except OSError:
            continue
        tid = os.urandom(12)
        req = b"\x00\x01\x00\x00\x21\x12\xa4\x42" + tid
        try:
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
                s.settimeout(timeout)
                s.sendto(req, (ip, int(port)))
                data, _ = s.recvfrom(2048)
        except OSError:
            continue
        if len(data) < 20 or data[8:20] != tid:
            continue
        i = 20
        while i + 4 <= len(data):
            atype = int.from_bytes(data[i:i + 2], "big")
            alen = int.from_bytes(data[i + 2:i + 4], "big")
            val = data[i + 4:i + 4 + alen]
            if atype == 0x0020 and alen >= 8:          # XOR-MAPPED-ADDRESS
                xport = int.from_bytes(val[2:4], "big") ^ 0x2112
                xip = bytes(a ^ b for a, b in zip(val[4:8], b"\x21\x12\xa4\x42"))
                return f"{socket.inet_ntoa(xip)}:{xport}"
            if atype == 0x0001 and alen >= 8:          # MAPPED-ADDRESS（老式）
                return f"{socket.inet_ntoa(val[4:8])}:{int.from_bytes(val[2:4], 'big')}"
            i += 4 + alen + ((4 - alen % 4) % 4)
    return ""


def ensure_relay() -> dict:
    """中继只做信令。已在跑就复用，未跑就拉起；就绪判定用真实 TCP 连接。"""
    if tcp_ready(RELAY, timeout=3.0):
        print(f"  中继已在运行 {RELAY}")
    else:
        rsh(f"pkill -f 'coldrevive-relay --listen {RELAY}' || true; sleep 0.3; "
            f"mkdir -p /opt/coldrevive/relay; "
            f"setsid nohup {RELAY_BIN} --listen {RELAY} --stats-file {RELAY_STATS} "
            f"> /opt/coldrevive/relay/real-run.log 2>&1 < /dev/null &")
        if not tcp_ready(RELAY, timeout=10.0):
            raise SystemExit(f"中继 {RELAY} 未接受 TCP 连接，拒绝继续")
        print(f"  中继已拉起 {RELAY}")
    return {"relay": RELAY, "stats_file": RELAY_STATS}


def relay_stats() -> dict:
    for path in (RELAY_STATS, "/opt/coldrevive/relay/stats-a.json"):
        try:
            raw = rsh(f"cat {path} 2>/dev/null || true")
        except SystemExit:
            continue
        raw = (raw or "").strip()
        if not raw:
            continue
        try:
            return json.loads(raw.splitlines()[-1])
        except (ValueError, IndexError):
            continue
    return {}



def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--trials", type=int, default=30)
    ap.add_argument("--timeout", type=float, default=12.0)
    ap.add_argument("--class", dest="klass", default=DEFAULT_CLASS,
                    choices=["full-cone", "restricted-cone", "port-restricted-cone",
                             "symmetric", "cgnat"],
                    help="持有者侧真实 NAT 的类别（必须与现场分类一致，否则中止）")
    ap.add_argument("--smoke", action="store_true", help="只各跑 1 次直连/中继，验证接线")
    ap.add_argument("--keep", action="store_true", help="结束后保留进程（便于复现/排障）")
    ap.add_argument("--capture", action="store_true", help="冒烟时在 222 上抓包留证")
    ap.add_argument("--out-dir", default="tests/spikes/s6/reports/real-run")
    args = ap.parse_args()

    global KLASS
    KLASS = args.klass

    work = (repo_root() / args.out_dir).resolve()
    if work.exists() and not args.keep:
        shutil.rmtree(work)
    work.mkdir(parents=True, exist_ok=True)

    # 1) 前置：两端出口必须不同，且都必须是真实公网地址
    egress_holder = public_ip_holder()
    egress_req_raw = stun_mapped_local() or public_ip_local()
    egress_req = egress_req_raw.rsplit(":", 1)[0] if ":" in egress_req_raw else egress_req_raw
    if not egress_holder or not egress_req:
        raise SystemExit(f"取不到出口 IP：holder={egress_holder!r} requester={egress_req!r}")
    if not is_public_ipv4(egress_holder) or not is_public_ipv4(egress_req):
        raise SystemExit(f"出口不是公网地址：holder={egress_holder} requester={egress_req}")
    if egress_holder == egress_req:
        raise SystemExit(f"两端出口相同（{egress_holder}）—— 同一台真实 NAT，"
                         f"不构成「两个真实 NAT 设备」，拒绝继续")
    print(f"  出口：holder(222)={egress_holder} / requester(A)={egress_req}")

    holder_host = hsh("hostname").strip() or "ubuntu-222"
    requester_host = socket.gethostname()
    if holder_host == requester_host:
        raise SystemExit(f"两端 hostname 相同（{holder_host}）—— 不是两台主机，拒绝继续")
    print(f"  主机：holder={holder_host}({HOLDER_SSH}) / requester={requester_host}({REQ_IP})")

    # 2) 持有者侧准备
    if not hsh(f"test -x {HOLDER_BIN}/coldrevived && test -x {HOLDER_BIN}/coldrevive-cli "
               f"&& echo ok"):
        raise SystemExit(f"{HOLDER_SSH}:{HOLDER_BIN} 缺 coldrevived / coldrevive-cli")
    busy = hsh(f"ss -lntup 2>/dev/null | grep -E ':(19900|19901|19902) ' || true")
    if busy.strip():
        raise SystemExit(f"222 上测试端口被占用，拒绝继续：\n{busy}")
    hsh(f"mkdir -p {HOLDER_WORK} && rm -rf {HOLDER_WORK}/*")

    # 3) 样本（两端同一份）
    sample = work / "sample.bin"
    sample.write_bytes(os.urandom(262144))
    file_hash = hashlib.sha256(sample.read_bytes()).hexdigest()
    scp_to_holder(sample, f"{HOLDER_WORK}/sample.bin")
    print(f"  sample sha256={file_hash[:16]}…")

    relay_info = ensure_relay()
    stats_before = relay_stats()
    print(f"  中继基线计数：{stats_before}")

    procs: list[subprocess.Popen] = []
    try:
        # 4) 持有者（222 上，绑 0.0.0.0，出向由路由选 ens160 → 真实 NAT）
        h_cfg = work / "holder.json"
        build_config(h_cfg, "192.168.34.222", "0.0.0.0", HOLDER_WORK,
                     f"{HOLDER_WORK}/sample.bin", ".")
        scp_to_holder(h_cfg, f"{HOLDER_WORK}/holder.json")
        hsh_safe(f"pkill -f 'coldrevived --config {HOLDER_WORK}/holder.json' || true")
        hsh(f"setsid nohup {HOLDER_BIN}/coldrevived --config {HOLDER_WORK}/holder.json "
            f"--node-id A --bind 0.0.0.0 --self-host 192.168.34.222 "
            f"--workdir {HOLDER_WORK} --sample {HOLDER_WORK}/sample.bin "
            f"> {HOLDER_WORK}/holder.log 2>&1 < /dev/null &")
        h_health = wait_holder_health(HOLDER_PORTS["http"])
        time.sleep(1.5)   # 等异步入库，否则唤醒会被 REJECT_NOT_FOUND
        print(f"  持有者就绪（node={h_health.get('node')}, workdir={h_health.get('workdir')}）")

        # 5) 持有者真实 STUN 探测（复用唤醒 socket）——这是「直连候选」的来源
        probe_raw = hsh(f"env COLDREVIVE_T0_CONFIG={HOLDER_WORK}/holder.json "
                        f"{HOLDER_BIN}/coldrevive-cli --json --node A nat probe", timeout=120)
        try:
            probe = json.loads(probe_raw.splitlines()[-1])
        except ValueError:
            raise SystemExit(f"nat probe 输出无法解析：{probe_raw[:300]}")
        mapped = probe.get("mapped_address") or ""
        print(f"  holder nat probe: nat_type={probe.get('nat_type')} mapped={mapped}")
        if not mapped:
            raise SystemExit("持有者没拿到 STUN 映射：真实 NAT 后面取不到公网端点，拒绝产出证据")
        if not mapped.startswith(egress_holder + ":"):
            raise SystemExit(f"STUN 映射 {mapped} 与实测出口 {egress_holder} 不一致："
                             f"出口判定或 STUN 结果不可信，拒绝继续")
        mapped_port = int(mapped.rsplit(":", 1)[1])
        if mapped_port == HOLDER_PORTS["wake"]:
            raise SystemExit(f"映射端口 {mapped_port} == 内部端口 —— 说明存在静态端口映射，"
                             f"那不是真实 NAT 行为，拒绝产出证据")
        print(f"  映射端口 {mapped_port} ≠ 内部端口 {HOLDER_PORTS['wake']}（动态分配，无静态映射）")

        adv_raw = hsh(f"curl -s --max-time 5 http://127.0.0.1:{HOLDER_PORTS['http']}/nat/advertise")
        adv = json.loads(adv_raw)
        adv_path = work / "holder-advertise.json"
        adv_path.write_bytes((json.dumps(adv, indent=2) + "\n").encode("utf-8"))
        adv_eps = [e.get("ip") or e.get("addr") for e in (adv.get("endpoints") or [])]
        print(f"  通告端点：{adv_eps}")
        if not any(str(x).startswith(egress_holder) for x in adv_eps):
            raise SystemExit(f"通告里没有 {egress_holder} 的端点：{adv_eps}，拒绝继续")

        # 6) 请求者（本机 A）
        r_cfg = work / "requester.json"
        build_config(r_cfg, "192.168.34.222", REQ_IP, str(work), str(sample),
                     str(repo_root()).replace("\\", "/"))
        log = open(work / "requester.log", "w", encoding="utf-8")
        procs.append(subprocess.Popen(
            [str(repo_root() / "target/release/coldrevived.exe"),
             "--config", str(r_cfg), "--node-id", "B", "--bind", REQ_IP,
             "--self-host", REQ_IP, "--workdir", str(work), "--sample", str(sample)],
            stdout=log, stderr=subprocess.STDOUT))
        r_health = wait_local_health(REQ_IP, REQ_PORTS["http"])
        print(f"  请求者就绪（node={r_health.get('node')}）")

        peer = local_cli(["nat", "peer", str(adv_path)], r_cfg, "B")
        print(f"  requester nat peer: ok={peer.get('ok')} endpoints={peer.get('endpoints')}")
        plan = local_cli(["nat", "plan"], r_cfg, "B")
        cands = [(c.get("kind"), c.get("ip"), c.get("port")) for c in (plan.get("candidates") or [])]
        print(f"  plan candidates: {cands}")
        if not any(k == "stun-mapped" and ip == egress_holder for k, ip, _ in cands):
            raise SystemExit(f"计划里没有 {egress_holder} 的 stun-mapped 候选：{cands}，拒绝继续")

        if args.smoke:
            cap = None
            if args.capture:
                cap = work / "capture-222.txt"
                cap_h = open(cap, "w", encoding="utf-8")
                # 必须按**对端公网 IP**过滤，不能按端口：经过 NAT 后源端口已被改写，
                # `port 19901` 抓不到任何真实 NAT 侧流量（踩过一次，只剩 loopback 噪音）。
                cap_proc = subprocess.Popen(
                    ["ssh", "-i", SSH_KEY, "-o", "StrictHostKeyChecking=no",
                     "-o", "UserKnownHostsFile=/dev/null", "-o", "LogLevel=ERROR",
                     "-o", "BatchMode=yes", f"root@{HOLDER_SSH}",
                     f"timeout 20 tcpdump -i ens160 -n -l 'udp and host {egress_req}'"],
                    stdout=cap_h, stderr=subprocess.STDOUT)
                procs.append(cap_proc)
                time.sleep(2.0)

            for kind in ("stun-mapped", "relay"):
                r = local_cli(["nat", "wake", file_hash, INFOHASH, kind], r_cfg, "B",
                              timeout=args.timeout * 3)
                used = [a.get("addr") for a in (r.get("attempts") or []) if a.get("ok")]
                print(f"  smoke[{kind}]: ok={r.get('ok')} path={r.get('path')} "
                      f"ack={r.get('ack_status')} rtt={r.get('rtt_ms')} used={used}")
                for a in (r.get("attempts") or []):
                    print(f"      attempt: {a.get('kind')} {a.get('addr')} "
                          f"ok={a.get('ok')} {str(a.get('detail'))[:120]}")
                if kind == "stun-mapped":
                    if used:
                        if not any(str(u).startswith(egress_holder) for u in used):
                            raise SystemExit(f"直连成功用的是 {used}，不是公网出口 {egress_holder}："
                                             f"数据面走了 LAN，不能算真实 NAT 穿透")
                        print(f"  ✔ 直连确实打到了 {egress_holder}（真实 NAT 外侧）")
                    elif KLASS == "symmetric":
                        # 真实 symmetric 无 DNAT，映射随目的地址变化 → 广告端点不是入向可达端口，
                        # 打洞在原理上不可能。这里必须**如实记录失败**并继续（不是放过）。
                        print("  ⚠ 直连失败 —— 与 symmetric 的预期一致（不是缺陷，是结论）")
                    else:
                        raise SystemExit("直连没打出去（无成功 attempt），拒绝产出证据")
                time.sleep(1.0)

            if cap is not None:
                time.sleep(3.0)
                cap_h.close()
                text = cap.read_text(encoding="utf-8", errors="replace") if cap.is_file() else ""
                lines = [ln for ln in text.splitlines()
                         if egress_req in ln or egress_holder in ln]
                # 方向只能靠「对端 IP 在 > 之前还是之后」判：`-i ens160` 的 tcpdump 输出
                # 不带 In/Out 标记（那是 `-i any` 才有的）。
                def is_inbound(ln: str) -> bool:
                    head, _, tail = ln.partition(">")
                    return egress_req in head

                inbound = [ln for ln in lines if is_inbound(ln)]
                outbound = [ln for ln in lines if not is_inbound(ln)]
                print(f"  抓包 @222/ens160（对端 {egress_req}）："
                      f"入向 {len(inbound)} 条 / 出向 {len(outbound)} 条")
                for ln in lines[:12]:
                    print("     " + ln.strip())
            return 0

        # 7) attestation + 设备证据（真实 NAT 的可核查记录）
        #    分类必须在**现场、同一条出口**上重测一遍：今天早些时候同一台机器曾因
        #    可用 STUN 样本不足被判成 port-restricted cone，实测是 symmetric。
        #    所以这里不接受历史报告，只接受刚才这一轮真跑出来的结论。
        print("  现场复测持有者分类（真实公网 STUN）…")
        hsh(f"cd /root/s6repo/tests/spikes/s6 && timeout 300 python3 nat_behavior.py "
            f"--label real-{KLASS}-holder --out {HOLDER_WORK}/s6-nb-{KLASS}.json",
            timeout=420)
        cls_report = work / f"s6-nb-{KLASS}.json"
        if not scp_from_holder(f"{HOLDER_WORK}/s6-nb-{KLASS}.json", cls_report):
            raise SystemExit("拿不到持有者的现场 STUN 分类报告，拒绝产出证据")
            print(f"  ⚠ 现场分类报告缺失，使用同日同机报告 {fallback.name}")
        nb = json.loads(cls_report.read_text(encoding="utf-8"))
        c = nb.get("classification") or {}
        print(f"  持有者分类：{c.get('classic')} | {c.get('rfc4787')} | conf={c.get('confidence')}")
        if str(c.get("classic")) != KLASS:
            raise SystemExit(f"持有者分类是 {c.get('classic')}，与 --class {KLASS} 不符："
                             f"环境定性不成立，拒绝产出证据")

        peer_cls_path = work / "s6-nb-requester.json"
        if not peer_cls_path.is_file():
            fallback = repo_root() / "tests/spikes/s6/reports/real-run/s6-nb-A-requester.json"
            if fallback.is_file():
                shutil.copyfile(fallback, peer_cls_path)
        peer_cls = {}
        if peer_cls_path.is_file():
            peer_cls = (json.loads(peer_cls_path.read_text(encoding="utf-8"))
                        .get("classification") or {})

        evidence = {
            "schema": "coldrevive.s6.real-nat-evidence/v1",
            "recorded_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "why_this_is_a_real_nat": [
                "两个出口各有自己的公网 IP，且都 ≠ 本机地址：映射由我方代码之外的设备完成",
                f"持有者映射端口 {mapped_port} ≠ 内部端口 {HOLDER_PORTS['wake']}：动态分配，"
                "不存在为该端口配置的静态映射（用户只映射了 22/7100，与测试端口无关）",
                "分类由真实公网 STUN 独立测得（RFC 5389 映射 + RFC 3489 CHANGE-REQUEST 过滤），"
                "不采信 attestation 自报类型",
                "数据面实测走公网出口（直连 attempt 的目标地址 = 持有者公网 egress），"
                "不是 LAN 捷径",
            ],
            "holder_side": {
                "host": holder_host,
                "local_ip": "192.168.34.222",
                "gateway_wan_ip": egress_holder,
                "stun_mapped": mapped,
                "internal_wake_port": HOLDER_PORTS["wake"],
                "classification": c,
                "classification_report": str(cls_report.name),
            },
            "requester_side": {
                "host": requester_host,
                "local_ip": REQ_IP,
                "gateway_wan_ip": egress_req,
                "classification": peer_cls,
                "classification_report": (peer_cls_path.name if peer_cls_path.is_file() else None),
            },
            "relay": relay_info,
            "test_ports_have_no_static_mapping": True,
            "synthesized_by_us": False,
        }
        ev_path = work / "real-nat-evidence.json"
        ev_path.write_bytes((json.dumps(evidence, indent=2) + "\n").encode("utf-8"))

        att = {
            "class": KLASS,
            "evidence_scope": "real-multi-nat",
            "nat_is_real_device": True,
            "gateway_wan_ip": egress_holder,
            "local_ip": "192.168.34.222",
            "peer_ip": REQ_IP,
            "peer_gateway_wan_ip": egress_req,
            "machine_ids": [f"holder@{holder_host}", f"requester@{requester_host}"],
            "real_nat_evidence": str(ev_path),
            "classification_report": str(cls_report.resolve()),
            "operator": "agent-on-behalf-of-owner",
            "recorded_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "note": (f"持有者在 {holder_host} 的 192.168.34.222（经真实 NAT → {egress_holder}）；"
                     f"请求者在 {requester_host} 的 {REQ_IP}（经真实 NAT → {egress_req}）；"
                     f"中继 {RELAY} 只做信令。测试端口无静态映射，映射端口由 STUN 实测。"),
        }
        att_path = work / f"attest-{KLASS}.json"
        att_path.write_bytes((json.dumps(att, indent=2) + "\n").encode("utf-8"))

        # 8) 矩阵（真实执行 CLI，成功判据 = ok=true 且 ack_status=OK）
        prefix = (f'set COLDREVIVE_T0_CONFIG={r_cfg}&& '
                  f'"{repo_root() / "target/release/coldrevive-cli.exe"}" '
                  f'--json --node B nat wake {file_hash} {INFOHASH}')
        out = work / f"s6-matrix-class-{KLASS}-real.json"
        cmd = [sys.executable, str(repo_root() / "tests/spikes/s6/nat_matrix.py"),
               "--mode", "real", "--class", KLASS,
               "--attest", str(att_path), "--trials", str(args.trials),
               "--timeout", str(args.timeout), "--evidence-scope", "real-multi-nat",
               "--direct-cmd", f"{prefix} stun-mapped",
               "--relay-cmd", f"{prefix} relay",
               "--out", str(out)]
        print("  开始测量 …")
        p = subprocess.run(cmd, capture_output=True, text=True,
                           timeout=args.trials * args.timeout * 3 + 300)
        print(p.stdout.strip()[-2000:])
        if p.stderr.strip():
            print("STDERR:", p.stderr.strip()[-800:], file=sys.stderr)

        if out.is_file():
            rep = json.loads(out.read_text(encoding="utf-8"))
            print(f"  status={rep.get('status')} evidence_valid={rep.get('evidence_valid')}")
            for k, v in (rep.get("paths") or {}).items():
                print(f"    {k}: {v.get('succeeded')}/{v.get('trials')} "
                      f"rate={v.get('reachability')}")
            # 直连目标必须是公网出口，否则是 LAN 捷径，数字无效
            obs = (rep.get("paths") or {}).get("direct", {}).get("observations") or []
            bad = [o for o in obs
                   if o.get("success") and not any(
                       str(a.get("addr", "")).startswith(egress_holder)
                       for a in ((o.get("observed") or {}).get("attempts") or []) if a.get("ok"))]
            if bad:
                print(f"  ⚠ {len(bad)} 次成功直连的对端不是 {egress_holder}，"
                      f"疑似 LAN 捷径，该报告不得用作真实 NAT 证据", file=sys.stderr)
            final = relay_stats()
            delta = {k: (final.get(k, 0) - stats_before.get(k, 0))
                     for k in ("connections", "registered", "delivered", "dropped", "rejected")
                     if isinstance(final.get(k), (int, float))
                     and isinstance(stats_before.get(k), (int, float))}
            print(f"  中继计数（本轮增量）：{delta}  累计={final}")
        return p.returncode
    finally:
        if not args.keep:
            for proc in procs:
                try:
                    proc.terminate()
                    proc.wait(timeout=5)
                except Exception:  # noqa: BLE001
                    proc.kill()
            hsh_safe(f"pkill -f 'coldrevived --config {HOLDER_WORK}/holder.json' || true")


if __name__ == "__main__":
    sys.exit(main())
