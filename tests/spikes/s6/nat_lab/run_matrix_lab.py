#!/usr/bin/env python3
"""S6 矩阵的 VM 实验室驱动：在同机两个 netns 端点间跑真实唤醒矩阵。

它做的事（全部为真实进程 / 真实 UDP / 真实 WAKEUP_ACK，无任何合成应答）：

  1. 在宿主 10.0.0.1:7100 起真实 `coldrevive-relay`；
  2. 在 holder netns 起 `coldrevived`（node B），在 requester netns 起另一个
     `coldrevived`（node A），两者分处不同 NAT 网关之后；
  3. holder 执行 `nat probe`（复用唤醒 socket 做 STUN，建立真实映射）；
  4. 取回 holder 的 `/nat/advertise`，交给 requester `nat peer`；
  5. 调 nat_matrix.py --mode real --lab-mode，逐条路径跑 >=30 次真实唤醒；
  6. 收尾杀进程。

诚实边界：
  * 这是**单机两个 netns**，不满足 S6 判据的「两台不同物理主机」条款，因此
    一律以 `--lab-mode` 运行，报告里 evidence_valid=false、evidence_scope=vm-namespace-lab。
  * 本脚本不写 spikes.json，也不改变任何 spike 状态。

用法:
  python3 run_matrix_lab.py --class full-cone --holder-ns lan-full-cone \
      --requester-ns lan-symmetric --trials 30 --repo /opt/coldrevive/repo
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

HOLDER_PORTS = {"http": 19900, "wake": 19901, "bt": 19902}
REQ_PORTS = {"http": 19800, "wake": 19801, "bt": 19802}
RELAY = "10.0.0.1:7100"
STUN = ["10.0.0.1:3478", "10.0.0.2:3478"]
INFOHASH = "lab-info-hash"
NETNS_IP = {
    "lan-full-cone": "192.168.10.2",
    "lan-restricted-cone": "192.168.20.2",
    "lan-port-restricted-cone": "192.168.30.2",
    "lan-symmetric": "192.168.40.2",
    "lan-cgnat": "192.168.50.2",
}
# 各类 NAT 网关的 WAN 侧地址（attestation 用）
GATEWAY_WAN = {
    "full-cone": "10.0.0.10",
    "restricted-cone": "10.0.0.11",
    "port-restricted-cone": "10.0.0.12",
    "symmetric": "10.0.0.13",
    "cgnat": "10.0.0.14",
}


def ns_exec(ns: str, argv: list[str], env: dict | None = None,
            timeout: float = 30.0) -> subprocess.CompletedProcess:
    cmd = ["ip", "netns", "exec", ns]
    if env:
        cmd += ["env"] + [f"{k}={v}" for k, v in env.items()]
    cmd += argv
    return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)


def wait_health(ns: str, ip: str, port: int, timeout: float = 12.0) -> dict:
    """健康检查必须在**该 netns 内**发起：宿主没有到对端 LAN 子网的路由。"""
    end = time.time() + timeout
    last = ""
    py = ("import json,sys,urllib.request;"
          "print(urllib.request.urlopen(sys.argv[1],timeout=1).read().decode())")
    while time.time() < end:
        p = ns_exec(ns, [sys.executable, "-c", py, f"http://{ip}:{port}/health"], timeout=6)
        if p.returncode == 0 and p.stdout.strip():
            try:
                return json.loads(p.stdout.strip().splitlines()[-1])
            except ValueError as exc:
                last = f"非 JSON: {exc}"
        else:
            last = (p.stderr or "").strip()[-160:]
        time.sleep(0.3)
    raise SystemExit(f"[{ns}] /health 未就绪（{ip}:{port}）：{last}")


def cli(ns: str, cfg: str, node: str, args: list[str], cli_bin: str,
        timeout: float = 30.0) -> dict:
    p = ns_exec(ns, [cli_bin, "--json", "--node", node] + args,
                env={"COLDREVIVE_T0_CONFIG": cfg}, timeout=timeout)
    out = (p.stdout or "").strip()
    if not out:
        raise SystemExit(f"[{ns}] CLI 无输出: {p.stderr.strip()[:400]}")
    try:
        return json.loads(out.splitlines()[-1])
    except ValueError as exc:
        raise SystemExit(f"[{ns}] CLI 输出非 JSON: {exc}: {out[:300]}") from exc


def write_config(path: Path, node_hosts: dict[str, str], bind: str,
                 workdir: Path, sample: Path) -> None:
    cfg = {
        "bind": bind,
        "nodes": {
            # 守护进程把「持有者」硬编码为 node_id=="A"（bootstrap_holder），故 A=holder、B=requester
            "A": {"role": "holder", "host": node_hosts["A"], **_ports(HOLDER_PORTS)},
            "B": {"role": "requester", "host": node_hosts["B"], **_ports(REQ_PORTS)},
        },
        "sample_path": str(sample),
        "keywords": ["coldrevive", "s6", "lab"],
        "workdir": str(workdir),
        "golden_req": str(Path(__file__).resolve().parents[3] / "fixtures/t0/golden/wakeup_req.v1.json"),
        "golden_ack": str(Path(__file__).resolve().parents[3] / "fixtures/t0/golden/wakeup_ack.v1.json"),
        "nat": {"enabled": True, "stun_servers": STUN, "relay": RELAY},
    }
    path.write_text(json.dumps(cfg, indent=2) + "\n", encoding="utf-8")


def _ports(p: dict) -> dict:
    return {"http_port": p["http"], "wake_udp_port": p["wake"], "bt_port": p["bt"]}


def fetch_advertise(ns: str, ip: str, port: int) -> dict:
    p = ns_exec(ns, [sys.executable, "-c",
                     "import json,urllib.request,sys;"
                     "print(urllib.request.urlopen(sys.argv[1],timeout=5).read().decode())",
                     f"http://{ip}:{port}/nat/advertise"], timeout=20)
    if p.returncode != 0:
        raise SystemExit(f"[{ns}] 取通告失败: {p.stderr.strip()[:300]}")
    return json.loads(p.stdout)


def start(cmd: list[str], log: Path) -> subprocess.Popen:
    fh = open(log, "w", encoding="utf-8")
    return subprocess.Popen(cmd, stdout=fh, stderr=subprocess.STDOUT)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--class", dest="klass", required=True)
    ap.add_argument("--holder-ns", required=True)
    ap.add_argument("--requester-ns", required=True)
    ap.add_argument("--trials", type=int, default=30)
    ap.add_argument("--timeout", type=float, default=25.0)
    ap.add_argument("--repo", default="/opt/coldrevive/repo")
    ap.add_argument("--work", default="/opt/coldrevive/matrix")
    ap.add_argument("--reports", default="/opt/coldrevive/reports/matrix")
    ap.add_argument("--smoke", action="store_true", help="只跑 1 次直连+1 次中继验证接线")
    args = ap.parse_args()

    repo = Path(args.repo)
    work = Path(args.work) / args.klass
    reports = Path(args.reports)
    reports.mkdir(parents=True, exist_ok=True)
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True)

    daemon = str(repo / "target/release/coldrevived")
    cli_bin = str(repo / "target/release/coldrevive-cli")
    relay_bin = str(repo / "target/release/coldrevive-relay")

    sample = work / "sample.bin"
    sample.write_bytes(os.urandom(262144))
    # 唤醒请求必须携带持有者真实入库的 file_hash（cr-indexer: sha256(file)），
    # 否则持有者按设计返回 REJECT_NOT_FOUND —— 那会被计为失败，不是「可达」。
    file_hash = hashlib.sha256(sample.read_bytes()).hexdigest()
    print(f"  sample file_hash={file_hash[:16]}…")

    h_ip = NETNS_IP[args.holder_ns]
    r_ip = NETNS_IP[args.requester_ns]
    if h_ip == r_ip:
        raise SystemExit("holder 与 requester 必须在不同 NAT 之后（同 netns 会走私有地址，绕过 NAT）")

    h_cfg, r_cfg = work / "holder.json", work / "requester.json"
    write_config(h_cfg, {"A": h_ip, "B": r_ip}, h_ip, work, sample)
    write_config(r_cfg, {"A": h_ip, "B": r_ip}, r_ip, work, sample)

    procs: list[subprocess.Popen] = []
    relays = subprocess.run(["pkill", "-f", "^coldrevive-relay"], capture_output=True)
    del relays
    try:
        # 1) 真实中继服务端（宿主）
        relay_stats = work / "relay-stats.json"
        procs.append(start([relay_bin, "--listen", RELAY,
                            "--stats-file", str(relay_stats)], work / "relay.log"))
        time.sleep(0.8)

        # 2) 两个净空内的守护进程
        procs.append(start(["ip", "netns", "exec", args.holder_ns, daemon,
                            "--config", str(h_cfg), "--node-id", "A",
                            "--bind", h_ip, "--self-host", h_ip,
                            "--workdir", str(work), "--sample", str(sample)],
                           work / "holder.log"))
        procs.append(start(["ip", "netns", "exec", args.requester_ns, daemon,
                            "--config", str(r_cfg), "--node-id", "B",
                            "--bind", r_ip, "--self-host", r_ip,
                            "--workdir", str(work), "--sample", str(sample)],
                           work / "requester.log"))
        h_health = wait_health(args.holder_ns, h_ip, HOLDER_PORTS["http"])
        time.sleep(1.5)  # 等持有者完成样本入库（异步），否则唤醒会被 REJECT_NOT_FOUND
        r_health = wait_health(args.requester_ns, r_ip, REQ_PORTS["http"])
        print(f"  holder health: {h_health.get('node')} {h_health.get('machine_id', '')[:12]}")
        print(f"  requester health: {r_health.get('node')} {r_health.get('machine_id', '')[:12]}")

        # 3) holder 真实 STUN 探测（建立映射）
        probe = cli(args.holder_ns, str(h_cfg), "A", ["nat", "probe"], cli_bin, timeout=40)
        print(f"  holder nat probe: nat_type={probe.get('nat_type')} "
              f"mapped={(probe.get('mapped_address') or probe.get('external', ''))}")

        # 4) 通告交接
        adv = fetch_advertise(args.holder_ns, h_ip, HOLDER_PORTS["http"])
        adv_path = work / "holder-advertise.json"
        adv_path.write_text(json.dumps(adv, indent=2) + "\n", encoding="utf-8")
        peer = cli(args.requester_ns, str(r_cfg), "B", ["nat", "peer", str(adv_path)], cli_bin)
        print(f"  requester nat peer: endpoints={peer.get('endpoints')} ok={peer.get('ok')}")

        plan = cli(args.requester_ns, str(r_cfg), "B", ["nat", "plan"], cli_bin)
        kinds = [c.get("kind") for c in (plan.get("candidates") or [])]
        print(f"  plan candidates: {kinds}")

        # 5) attestation（如实标注 lab_mode）
        att = {
            "class": args.klass,
            "lab_mode": True,
            "gateway_wan_ip": GATEWAY_WAN.get(args.klass, h_ip),
            "local_ip": h_ip,
            "peer_ip": r_ip,
            "machine_ids": [f"{args.holder_ns}@{socket.gethostname()}", f"{args.requester_ns}@{socket.gethostname()}"],
            "real_nat_evidence": str(repo / "tests/spikes/s6/nat_lab/lab-evidence.json"),
            "classification_report": f"/opt/coldrevive/reports/s6-nb-{args.klass}.json",
            "operator": "agent-on-behalf-of-owner",
            "recorded_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "note": ("VM 内 iptables conntrack 真改写 NAT；两端为同机两个 netns，"
                     "不满足「两台不同物理主机」条款，故以 lab 模式上报"),
        }
        att_path = work / f"attest-{args.klass}.json"
        att_path.write_text(json.dumps(att, indent=2) + "\n", encoding="utf-8")

        cli_prefix = f"ip netns exec {args.requester_ns} env COLDREVIVE_T0_CONFIG={r_cfg} " \
                     f"{cli_bin} --json --node B nat wake {file_hash} {INFOHASH}"
        out = reports / f"s6-matrix-class-{args.klass}-lab.json"

        # 5b) 冒烟：先各跑 1 次直连/中继，确认接线通了再跑整轮
        if args.smoke:
            for kind in ("stun-mapped", "relay"):
                r = cli(args.requester_ns, str(r_cfg), "B",
                        ["nat", "wake", file_hash, INFOHASH, kind], cli_bin, timeout=args.timeout)
                print(f"  smoke[{kind}]: ok={r.get('ok')} path={r.get('path')} "
                      f"ack_status={r.get('ack_status')} rtt={r.get('rtt_ms')} err={r.get('error')}")
            return 0

        # 6) 真实矩阵
        matrix = Path(repo / "tests/spikes/s6/nat_matrix.py")
        cmd = [sys.executable, str(matrix), "--mode", "real", "--class", args.klass,
               "--attest", str(att_path), "--trials", str(args.trials),
               "--timeout", str(args.timeout), "--lab-mode",
               "--direct-cmd", f"{cli_prefix} stun-mapped",
               "--relay-cmd", f"{cli_prefix} relay",
               "--out", str(out)]
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=args.trials * args.timeout * 2 + 120)
        print(p.stdout.strip()[-1200:])
        if p.stderr.strip():
            print("STDERR:", p.stderr.strip()[-500:], file=sys.stderr)
        return p.returncode
    finally:
        for proc in reversed(procs):
            try:
                proc.terminate()
                proc.wait(timeout=5)
            except Exception:  # noqa: BLE001
                proc.kill()
        subprocess.run(["pkill", "-f", "^coldrevive-relay"], capture_output=True)


if __name__ == "__main__":
    sys.exit(main())
