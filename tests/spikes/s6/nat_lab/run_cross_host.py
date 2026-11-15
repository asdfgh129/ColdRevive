#!/usr/bin/env python3
"""S6 矩阵的**跨主机**驱动：持有者在实验机 221 的 netns 内，请求者在本机（A）。

与 run_matrix_lab.py 的唯一区别是"另一端在哪"：
  * run_matrix_lab.py：两端都是 221 内的 netns → `vm-namespace-lab`
  * 本脚本：持有者在 221 的 netns 内，请求者是**另一台物理主机**上的 coldrevived
            → `cross-host-lab`（判据条款一成立，条款二仍不成立）

链路怎么做出来的（不需要对端主机任何权限、任何配置改动）：
  `link_physical.sh up <class>` 给该类网关的 WAN 口挂一个物理 LAN 地址
  （192.168.1.241~.245），由 221 在 ens160 上做 proxy ARP 替它应答。
  于是请求者 A 看到的对端就在同一二层网络里。

所有远端命令都通过 `ssh ... bash -s` 把脚本从 **stdin** 送入，
避免 ssh 把 argv 拼成字符串后被远端 shell 二次拆分。

诚实边界（不得突破）：
  * NAT 仍由 iptables/conntrack 合成 → `nat_is_real_device=false`，
    `evidence_valid` 恒为 false，**R-04 仍不可判定**。
  * 本脚本不写 spikes.json，不改变任何 spike 状态。
  * 结束后 `link_physical.sh down` 并停掉本次拉起的进程（除非 --keep）。

用法（在 Windows / A 上执行）:
  python tests/spikes/s6/nat_lab/run_cross_host.py --class restricted-cone --trials 30
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
HOST_SSH = os.environ.get("CR_HOST", "192.168.1.221")
SSH_KEY = os.environ.get("CR_KEY", str(Path.home() / "AppData/Local/Temp/crkeys/id_ed25519"))
REMOTE_REPO = "/opt/coldrevive/repo"
REMOTE_WORK = "/opt/coldrevive/cross"

REQ_IP = "192.168.1.210"          # 请求者所在物理主机（本机）在 LAN 上的地址
LAN_STUN = ["192.168.1.221:3478", "192.168.1.239:3478"]
RELAY = "192.168.1.221:7100"

HOLDER_PORTS = {"http": 19900, "wake": 19901, "bt": 19902}
REQ_PORTS = {"http": 17800, "wake": 17801, "bt": 17802}

NETNS = {
    "full-cone": ("lan-full-cone", "192.168.10.2", "192.168.1.241"),
    "restricted-cone": ("lan-restricted-cone", "192.168.20.2", "192.168.1.242"),
    "port-restricted-cone": ("lan-port-restricted-cone", "192.168.30.2", "192.168.1.243"),
    "symmetric": ("lan-symmetric", "192.168.40.2", "192.168.1.244"),
    "cgnat": ("lan-cgnat", "192.168.50.2", "192.168.1.245"),
}
INFOHASH = "cross-host-hash"

# 物理地址**不能写死**：2026-11-15 实测发现用户的家庭 LAN 上 .242/.244/.245 已被真实设备
# 占用，于是三个类的 punch 全发给了别人的机器，报告里却只写「唤醒无应答」。
# 现在的策略：首选类默认地址，被占就从池子里往后找，并在 up 之后**校验应答者身份**。
ADDR_POOL = [f"192.168.1.{n}" for n in range(236, 254)]

# 每次试验前清空网关的 ipset「接触记录」，否则第一次开洞后余下的试验全吃老本。
RESET_SH = """for ns in $(ip netns list | awk '{print $1}' | grep '^gw-'); do
  ip netns exec "$ns" sh -c 'for s in $(ipset list -n 2>/dev/null); do ipset flush $s; done' 2>/dev/null
done
true
"""


def repo_root() -> Path:
    # __file__ = <repo>/tests/spikes/s6/nat_lab/run_cross_host.py
    return Path(__file__).resolve().parents[4]


# ---- ssh 封装（脚本走 stdin，规避远端 shell 的二次拆分） ----------------------
def ssh_run(script: str, timeout: float = 60.0) -> subprocess.CompletedProcess:
    cmd = ["ssh", "-i", SSH_KEY,
           "-o", "StrictHostKeyChecking=no",
           "-o", "UserKnownHostsFile=/dev/null",
           "-o", "LogLevel=ERROR",
           "-o", "ServerAliveInterval=10",
           f"root@{HOST_SSH}", "bash", "-s"]
    return subprocess.run(cmd, input=script, capture_output=True, text=True, timeout=timeout)


def sh(script: str, timeout: float = 60.0) -> str:
    p = ssh_run(script, timeout)
    if p.returncode != 0:
        raise SystemExit(f"[221] 命令失败 rc={p.returncode}: {p.stderr.strip()[:400]}")
    return p.stdout.strip()


def sh_safe(script: str, timeout: float = 30.0) -> str:
    try:
        return sh(script, timeout)
    except (SystemExit, subprocess.TimeoutExpired) as exc:
        print(f"  [清理告警] {exc}", file=sys.stderr)
        return ""


def scp(local: Path, remote: str) -> None:
    cmd = ["scp", "-i", SSH_KEY,
           "-o", "StrictHostKeyChecking=no",
           "-o", "UserKnownHostsFile=/dev/null",
           "-o", "LogLevel=ERROR",
           str(local), f"root@{HOST_SSH}:{remote}"]
    p = subprocess.run(cmd, capture_output=True, text=True, timeout=180)
    if p.returncode != 0:
        raise SystemExit(f"scp 失败 {local} → {remote}: {p.stderr.strip()[:300]}")


# ---- 就绪检查（一律真实观测，不看进程/文件是否存在） -------------------------
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


# ---- 物理地址：挑选 + 身份校验 -----------------------------------------------
# 「ping 得通」不等于「通到实验机」。2026-11-15 的失败就是栽在这里：.242/.244/.245
# 被家里的真实设备占着，ping 有应答 → 预检通过 → punch 全打到别人机器上 → 报告里
# 只剩一句「唤醒无应答」。所以判定必须比**应答者的 MAC**，不能用可达性。
def norm_mac(s: str) -> str:
    parts = s.strip().lower().replace("-", ":").split(":")
    if len(parts) != 6:
        return ""
    try:
        return ":".join(f"{int(p, 16):02x}" for p in parts)
    except ValueError:
        return ""


def arp_lookup(ip: str) -> str:
    """读本机 ARP 表里 ip 的 MAC；没有条目返回空串。只读，不需要管理员。"""
    try:
        p = subprocess.run(["arp", "-a", ip], capture_output=True, text=True, timeout=10)
    except (OSError, subprocess.SubprocessError):
        return ""
    for line in (p.stdout or "").splitlines():
        f = line.split()
        if len(f) >= 2 and f[0] == ip:
            return norm_mac(f[1])
    return ""


def local_ping(ip: str, wait_ms: int = 700) -> bool:
    p = subprocess.run(["ping", "-n", "1", "-w", str(wait_ms), ip],
                       capture_output=True, text=True)
    return "TTL=" in (p.stdout or "")


def phys_iface_mac() -> str:
    return norm_mac(sh("cat /sys/class/net/ens160/address"))


def pick_free_addr(preferred: str, our_mac: str) -> str:
    """挑一个当前没有被**其它设备**占用的地址。

    ARP 表里出现非本机 MAC 的条目 = 被占用，直接跳过（宁可跳过也不冒险）。
    空条目 / 条目就是本机 MAC = 可用。
    """
    order = [preferred] + [a for a in ADDR_POOL if a != preferred]
    for cand in order:
        if cand in (REQ_IP, HOST_SSH) or cand.endswith(".1"):
            continue
        local_ping(cand)                      # 触发一次 ARP 解析
        mac = arp_lookup(cand)
        if mac and mac != our_mac:
            print(f"  [{cand}] 已被其它设备占用（{mac}），跳过")
            continue
        return cand
    raise SystemExit("地址池里找不到空闲地址，拒绝继续（避免把 punch 打到别人机器上）")


def assert_phys_identity(ip: str, our_mac: str) -> None:
    local_ping(ip)
    mac = arp_lookup(ip)
    if not mac:
        raise SystemExit(f"{ip} 没有 ARP 条目：proxy ARP 未生效，拒绝继续")
    if mac != our_mac:
        raise SystemExit(
            f"{ip} 的应答者不是实验机：期望 {our_mac}，实际 {mac}。"
            f"该地址被其它设备占用，punch 会打到错误的机器上 —— 拒绝产出证据")


def stun_probe(addr: str, timeout: float = 1.0) -> bool:
    """真发一个 STUN binding request，收到回包才算「STUN 活着」。

    不能用 `ss` 看有没有监听：进程可能在启动它的 ssh 会话结束时被带走，
    那时 `ss` 早就查不到了，而分类会静默退化成「只有中继候选」。
    """
    host, _, port = addr.rpartition(":")
    tid = os.urandom(12)
    req = b"\x00\x01\x00\x00\x21\x12\xa4\x42" + tid
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
            s.settimeout(timeout)
            s.sendto(req, (host, int(port)))
            data, _ = s.recvfrom(2048)
    except OSError:
        return False
    return len(data) >= 20 and data[0:2] in (b"\x01\x01", b"\x01\x11") and data[8:20] == tid


def assert_stun_ready(timeout: float = 8.0) -> None:
    end = time.time() + timeout
    while time.time() < end:
        if all(stun_probe(a, 0.6) for a in LAN_STUN):
            return
        time.sleep(0.4)
    got = {a: stun_probe(a, 0.6) for a in LAN_STUN}
    raise SystemExit(f"物理口 STUN 不可达：{got}。分类与 stun-mapped 端点都会失真，拒绝继续")


WARM_PY = r"""
import os, socket, sys, time
deadline = time.time() + %(deadline)f
tid = os.urandom(12)
req = b"\x00\x01\x00\x00\x21\x12\xa4\x42" + tid
t0 = time.time()
n = 0
while time.time() < deadline:
    n += 1
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.settimeout(1.0)
    try:
        s.sendto(req, ("%(host)s", %(port)d))
        d, _ = s.recvfrom(2048)
        if len(d) >= 20 and d[8:20] == tid:
            print("WARM_OK %%d %%.1f" %% (n, time.time() - t0))
            sys.exit(0)
    except OSError:
        pass
    finally:
        s.close()
    time.sleep(0.25)
print("WARM_FAIL %%d" %% n)
sys.exit(2)
"""


def warm_stun_from_netns(ns: str, timeout: float = 45.0) -> float:
    """等 netns → 物理口 STUN 真的通。

    实测（2026-11-15）：链路 up 之后有一段时间不通 —— 头 5 次请求超时、约 10s 后
    10/10 且每次 <1ms。这是链路/ARP 收敛，不是丢包。若不等它收敛就测分类，
    分类会如实报 `undetermined`，于是「这一类是什么」直接缺失。
    等不到就失败，不用默认值糊过去。
    """
    host, _, port = LAN_STUN[0].rpartition(":")
    script = ("ip netns exec %s python3 - <<'PY'\n%s\nPY"
              % (ns, WARM_PY % {"deadline": timeout, "host": host, "port": int(port)}))
    p = ssh_run(script, timeout=timeout + 30)
    out = (p.stdout or "").strip().splitlines()
    if not out or not out[-1].startswith("WARM_OK"):
        raise SystemExit(f"{ns} → {LAN_STUN[0]} 在 {timeout:.0f}s 内始终不通："
                         f"{(p.stdout or p.stderr or '').strip()[-200:]}")
    n, elapsed = out[-1].split()[1:3]
    return float(elapsed)


def wait_remote_health(ns: str, ip: str, port: int, timeout: float = 15.0) -> dict:
    """持有者健康检查必须在**该 netns 内**发起（宿主没有到对端 LAN 子网的路由）。"""
    end, last = time.time() + timeout, ""
    script = (f"ip netns exec {ns} python3 -c \"import json,sys,urllib.request;"
              f"print(urllib.request.urlopen(sys.argv[1],timeout=1).read().decode())\" "
              f"http://{ip}:{port}/health")
    while time.time() < end:
        p = ssh_run(script, timeout=15)
        if p.returncode == 0 and p.stdout.strip():
            try:
                return json.loads(p.stdout.strip().splitlines()[-1])
            except ValueError as exc:
                last = f"非 JSON: {exc}"
        else:
            last = (p.stderr or p.stdout or "").strip()[-160:]
        time.sleep(0.4)
    raise SystemExit(f"持有者 /health 未就绪（{ip}:{port}）：{last}")


def wait_local_health(ip: str, port: int, timeout: float = 15.0) -> dict:
    end, last = time.time() + timeout, ""
    while time.time() < end:
        try:
            return http_json(f"http://{ip}:{port}/health", timeout=1.5)
        except Exception as exc:  # noqa: BLE001
            last = f"{type(exc).__name__}: {exc}"
        time.sleep(0.4)
    raise SystemExit(f"请求者 /health 未就绪（{ip}:{port}）：{last}")


# ---- 配置 --------------------------------------------------------------------
def ports(p: dict) -> dict:
    return {"http_port": p["http"], "wake_udp_port": p["wake"], "bt_port": p["bt"]}


def build_config(path: Path, holder_host: str, req_host: str, bind: str,
                 workdir: str, sample: str, golden_dir: str) -> None:
    cfg = {
        "bind": bind,
        # 守护进程把「持有者」硬编码为 node_id=="A"（bootstrap_holder），故 A=holder、B=requester
        "nodes": {
            "A": {"role": "holder", "host": holder_host, **ports(HOLDER_PORTS)},
            "B": {"role": "requester", "host": req_host, **ports(REQ_PORTS)},
        },
        "sample_path": sample,
        "keywords": ["coldrevive", "s6", "cross-host"],
        "workdir": workdir,
        "golden_req": f"{golden_dir}/fixtures/t0/golden/wakeup_req.v1.json",
        "golden_ack": f"{golden_dir}/fixtures/t0/golden/wakeup_ack.v1.json",
        "nat": {"enabled": True, "stun_servers": LAN_STUN, "relay": RELAY},
    }
    path.write_text(json.dumps(cfg, indent=2) + "\n", encoding="utf-8")


def local_cli(args: list[str], cfg: Path, node: str, timeout: float = 60.0) -> dict:
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


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--class", dest="klass", required=True, choices=sorted(NETNS))
    ap.add_argument("--trials", type=int, default=30)
    ap.add_argument("--timeout", type=float, default=12.0)
    ap.add_argument("--keep", action="store_true", help="结束后不拆链路、不停进程")
    ap.add_argument("--smoke", action="store_true", help="只各跑 1 次直连/中继验证接线")
    ap.add_argument("--out-dir", default="tests/spikes/s6/reports/cross-host")
    args = ap.parse_args()

    ns, h_ip, phys_default = NETNS[args.klass]
    work = (repo_root() / args.out_dir / args.klass).resolve()
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True)
    remote_work = f"{REMOTE_WORK}/{args.klass}"

    holder_host = sh("hostname").strip() or "ubuntu-221"
    requester_host = socket.gethostname()
    print(f"  两端主机：holder={holder_host}(221) / requester={requester_host}({REQ_IP})")

    # 物理地址必须是「实验机在应答」的那一个。先挑空闲，up 之后再验身份。
    our_mac = phys_iface_mac()
    phys = pick_free_addr(phys_default, our_mac)
    print(f"  物理地址：{phys}（实验机 {holder_host} 的 ens160 = {our_mac}）")

    if not remote_work.startswith(REMOTE_WORK + "/"):
        raise SystemExit(f"拒绝清理可疑远端目录: {remote_work}")
    sh(f"mkdir -p {remote_work} && rm -rf {remote_work}/*")
    reset_local = work / "reset.sh"
    # 必须按字节写：Windows 的 write_text 会把 \n 翻成 \r\n，远端 bash 会报
    # "syntax error near unexpected token `do\r'"，复位静默失效 → 成功率虚高。
    reset_local.write_bytes(RESET_SH.encode("utf-8"))
    scp(reset_local, f"{remote_work}/reset.sh")
    sh(f"chmod +x {remote_work}/reset.sh")

    # 1) 样本（两端同一份：持有者按 sha256(file) 入库，请求者用同一 hash 唤醒）
    sample = work / "sample.bin"
    sample.write_bytes(os.urandom(262144))
    file_hash = hashlib.sha256(sample.read_bytes()).hexdigest()
    scp(sample, f"{remote_work}/sample.bin")
    print(f"  sample sha256={file_hash[:16]}…")

    procs: list[subprocess.Popen] = []
    try:
        # 2) 链路：暴露该类网关到物理 LAN，并起一个给 A 用的 STUN
        sh_safe(f"cd {REMOTE_REPO}/tests/spikes/s6/nat_lab && ./link_physical.sh down || true")
        up_out = sh(f"cd {REMOTE_REPO}/tests/spikes/s6/nat_lab && "
                    f"LINK_PHYS_ADDR={phys} ./link_physical.sh up {args.klass}")
        # 回读实验机自报的地址/MAC，跟驱动的认知对齐（避免两边各说各话）
        reported = dict(kv.split("=", 1) for kv in up_out.split()
                        if kv.startswith(("PHYS=", "MAC=")))
        if reported.get("PHYS") != phys:
            raise SystemExit(f"实验机暴露的地址是 {reported.get('PHYS')}，与驱动挑选的 {phys} 不一致")
        sh(f"cd {REMOTE_REPO}/tests/spikes/s6/nat_lab && ./link_physical.sh stun-lan-start")
        # 身份校验，不是可达性校验：ping 通只证明「有人应答」，不证明「应答的是实验机」。
        assert_phys_identity(phys, our_mac)
        print(f"  物理链路就绪：A → {phys} 的应答者 MAC = {our_mac}（= {holder_host} ens160）")
        assert_stun_ready()
        print(f"  物理口 STUN 就绪：{' / '.join(LAN_STUN)}")
        warm = warm_stun_from_netns(ns)
        print(f"  {ns} → STUN 收敛完成（{warm:.1f}s）")

        # 2b) 分类也在这条**同一条物理链路**上重测一次：可达率与分类必须出自同一环境，
        #     否则"这一类是什么"和"这一类能不能通"来自两个不同布置，证据不自洽。
        #     同时它也是链路保真度的反证：若代理路径改坏了 NAT 语义，分类会先露馅。
        sh(f"ip netns exec {ns} python3 {REMOTE_REPO}/tests/spikes/s6/nat_behavior.py "
           f"--mapping-servers {','.join(LAN_STUN)} --filter-servers {','.join(LAN_STUN)} "
           f"--label cross-host-{args.klass} --out {remote_work}/s6-nb-{args.klass}.json",
           timeout=240)
        nb_local = work / f"s6-nb-{args.klass}.json"
        subprocess.run(["scp", "-i", SSH_KEY,
                        "-o", "StrictHostKeyChecking=no",
                        "-o", "UserKnownHostsFile=/dev/null",
                        "-o", "LogLevel=ERROR",
                        f"root@{HOST_SSH}:{remote_work}/s6-nb-{args.klass}.json", str(nb_local)],
                       capture_output=True, text=True, timeout=120, check=False)
        cls_report = nb_local
        if cls_report.is_file():
            nb = json.loads(cls_report.read_text(encoding="utf-8"))
            c = nb.get("classification") or {}
            print(f"  链路分类复测: {c.get('classic')} "
                  f"| {c.get('rfc4787')} | conf={c.get('confidence')}")
            # 「不判」是合法结论，但不能拿来当这一类的分类用：那说明测试本身没跑成，
            # 于是「这一类是什么」和下面的可达率就不是同一个环境了。
            if str(c.get("classic")) in ("undetermined", "None", ""):
                raise SystemExit(
                    f"链路分类复测为 undetermined（{c.get('note')}）——"
                    f"测试环境没跑起来，拒绝产出矩阵")
        else:
            # 分类报告缺失不得用占位值糊过去：直接失败，宁可不出证据。
            raise SystemExit("分类复测报告未取回，拒绝继续（避免用占位路径骗过 attestation）")

        # 3) 中继（绑在物理地址：两端都从这里取信令）
        relay_log = f"{remote_work}/relay.log"
        relay_stats = f"{remote_work}/relay-stats.json"
        sh_safe(f"pkill -f 'coldrevive-relay --listen {RELAY}'")
        sh("sleep 0.3")
        sh(f"nohup setsid {REMOTE_REPO}/target/release/coldrevive-relay "
           f"--listen {RELAY} --stats-file {relay_stats} > {relay_log} 2>&1 < /dev/null &")
        if not tcp_ready(RELAY, timeout=8.0):
            raise SystemExit(f"中继 {RELAY} 未接受 TCP 连接，拒绝继续（避免产出假数据）")
        print(f"  中继就绪 {RELAY}")

        # 4) 持有者（221 的 netns 内）
        h_cfg = work / "holder.json"
        build_config(h_cfg, h_ip, REQ_IP, h_ip, remote_work,
                     f"{remote_work}/sample.bin", REMOTE_REPO)
        scp(h_cfg, f"{remote_work}/holder.json")
        sh(f"nohup setsid ip netns exec {ns} {REMOTE_REPO}/target/release/coldrevived "
           f"--config {remote_work}/holder.json --node-id A --bind {h_ip} --self-host {h_ip} "
           f"--workdir {remote_work} --sample {remote_work}/sample.bin "
           f"> {remote_work}/holder.log 2>&1 < /dev/null &")
        h_health = wait_remote_health(ns, h_ip, HOLDER_PORTS["http"])
        time.sleep(1.5)  # 等异步入库（否则唤醒会被 REJECT_NOT_FOUND）
        print(f"  持有者 {ns}/{h_ip} 就绪（node={h_health.get('node')}）")

        # 5) 持有者真实 STUN 探测 + 通告
        probe_raw = sh(f"ip netns exec {ns} env COLDREVIVE_T0_CONFIG={remote_work}/holder.json "
                       f"{REMOTE_REPO}/target/release/coldrevive-cli --json --node A nat probe",
                       timeout=60)
        try:
            probe = json.loads(probe_raw.splitlines()[-1])
        except ValueError:
            probe = {}
        print(f"  holder nat probe: nat_type={probe.get('nat_type')} "
              f"mapped={probe.get('mapped_address') or probe.get('external')}")
        # 没有 stun-mapped 端点时矩阵会「安静地」只剩中继，成功率看起来仍然正常，
        # 但测的已经不是「NAT 穿透」了。这种情况必须炸掉，不能产出证据。
        if not (probe.get("mapped_address") or probe.get("external")):
            raise SystemExit(f"持有者没有拿到 STUN 映射（nat_type={probe.get('nat_type')}）："
                             f"STUN 不可达，直连候选不存在，拒绝产出矩阵")

        adv_raw = sh(f"ip netns exec {ns} python3 -c \"import urllib.request;"
                     f"print(urllib.request.urlopen('http://{h_ip}:{HOLDER_PORTS['http']}"
                     f"/nat/advertise',timeout=5).read().decode())\"")
        adv = json.loads(adv_raw)
        adv_path = work / "holder-advertise.json"
        adv_path.write_text(json.dumps(adv, indent=2) + "\n", encoding="utf-8")
        print(f"  通告端点: {[e.get('addr') or e for e in (adv.get('endpoints') or [])]}")

        # 6) 请求者（本机 A）
        r_cfg = work / "requester.json"
        build_config(r_cfg, h_ip, REQ_IP, REQ_IP, str(work), str(sample),
                     str(repo_root()).replace("\\", "/"))
        log = open(work / "requester.log", "w", encoding="utf-8")
        procs.append(subprocess.Popen(
            [str(repo_root() / "target/release/coldrevived.exe"),
             "--config", str(r_cfg), "--node-id", "B", "--bind", REQ_IP,
             "--self-host", REQ_IP, "--workdir", str(work), "--sample", str(sample)],
            stdout=log, stderr=subprocess.STDOUT))
        r_health = wait_local_health(REQ_IP, REQ_PORTS["http"])
        print(f"  请求者 {REQ_IP} 就绪（node={r_health.get('node')}）")

        peer = local_cli(["nat", "peer", str(adv_path)], r_cfg, "B")
        print(f"  requester nat peer: ok={peer.get('ok')} endpoints={peer.get('endpoints')}")
        plan = local_cli(["nat", "plan"], r_cfg, "B")
        print(f"  plan candidates: {[c.get('kind') for c in (plan.get('candidates') or [])]}")

        if args.smoke:
            for kind in ("stun-mapped", "relay"):
                r = local_cli(["nat", "wake", file_hash, INFOHASH, kind], r_cfg, "B",
                              timeout=args.timeout * 3)
                print(f"  smoke[{kind}]: ok={r.get('ok')} path={r.get('path')} "
                      f"ack={r.get('ack_status')} rtt={r.get('rtt_ms')} err={r.get('error')}")
                for note in (r.get("notes") or []):
                    print(f"      note: {note}")
                for a in (r.get("attempts") or []):
                    print(f"      attempt: {a.get('kind')} {a.get('addr')} "
                          f"ok={a.get('ok')} {a.get('detail')}")
            return 0

        # 7) attestation：如实标注 cross-host-lab（条款一成立、条款二不成立）
        att = {
            "class": args.klass,
            "evidence_scope": "cross-host-lab",
            "lab_mode": True,
            "nat_is_real_device": False,
            "gateway_wan_ip": phys,
            "gateway_wan_mac_verified": our_mac,
            "local_ip": h_ip,
            "peer_ip": REQ_IP,
            "machine_ids": [f"{ns}@{holder_host}", f"requester@{requester_host}"],
            "real_nat_evidence": f"{REMOTE_REPO}/tests/spikes/s6/nat_lab/lab-evidence.json",
            "classification_report": str(cls_report.resolve()),
            "operator": "agent-on-behalf-of-owner",
            "recorded_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "note": (f"持有者在 {holder_host} 的 netns 内，请求者在另一台物理主机 "
                     f"{requester_host}({REQ_IP}) 的 Windows 上；网关 WAN 以物理 LAN 地址 "
                     f"{phys} 暴露（221 做 proxy ARP），且该地址的 ARP 应答者 MAC 已校验为 "
                     f"{holder_host} 的 ens160。NAT 仍为 iptables/conntrack 合成 → "
                     "条款二不成立，evidence_valid=false"),
        }
        att_path = work / f"attest-{args.klass}.json"
        att_path.write_text(json.dumps(att, indent=2) + "\n", encoding="utf-8")

        # 8) 真实矩阵（在本机跑：直连/中继命令都在本机执行，与真实使用姿态一致）
        prefix = (f'set COLDREVIVE_T0_CONFIG={r_cfg}&& '
                  f'"{repo_root() / "target/release/coldrevive-cli.exe"}" '
                  f'--json --node B nat wake {file_hash} {INFOHASH}')
        reset = (f'ssh -i "{SSH_KEY}" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null '
                 f'-o LogLevel=ERROR root@{HOST_SSH} "bash {remote_work}/reset.sh"')
        out = work / f"s6-matrix-class-{args.klass}-cross-host.json"
        cmd = [sys.executable, str(repo_root() / "tests/spikes/s6/nat_matrix.py"),
               "--mode", "real", "--class", args.klass,
               "--attest", str(att_path), "--trials", str(args.trials),
               "--timeout", str(args.timeout), "--evidence-scope", "cross-host-lab",
               "--inter-trial-reset", reset,
               "--direct-cmd", f"{prefix} stun-mapped",
               "--relay-cmd", f"{prefix} relay",
               "--out", str(out)]
        p = subprocess.run(cmd, capture_output=True, text=True,
                           timeout=args.trials * args.timeout * 3 + 180)
        print(p.stdout.strip()[-1500:])
        if p.stderr.strip():
            print("STDERR:", p.stderr.strip()[-600:], file=sys.stderr)
        bad_reset = [r for r in (json.loads(out.read_text(encoding="utf-8")).get("paths", {})
                                 .get("direct", {}).get("observations") or [])
                     if r.get("reset_exit_code") not in (0, None)]
        if bad_reset:
            print(f"  ⚠ {len(bad_reset)} 次试验的 ipset 复位失败（reset_exit_code≠0），"
                  f"本轮成功率不可信", file=sys.stderr)
        print("  relay stats: " + sh_safe(f"cat {relay_stats}"))
        return p.returncode
    finally:
        for proc in procs:
            try:
                proc.terminate()
                proc.wait(timeout=5)
            except Exception:  # noqa: BLE001
                proc.kill()
        if not args.keep:
            sh_safe(f"pkill -f 'coldrevived --config {remote_work}/holder.json'")
            sh_safe(f"pkill -f 'coldrevive-relay --listen {RELAY}'")
            sh_safe(f"cd {REMOTE_REPO}/tests/spikes/s6/nat_lab && ./link_physical.sh down || true")
            sh_safe(f"cd {REMOTE_REPO}/tests/spikes/s6/nat_lab && ./link_physical.sh stun-lan-stop || true")


if __name__ == "__main__":
    sys.exit(main())
