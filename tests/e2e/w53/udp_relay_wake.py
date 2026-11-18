#!/usr/bin/env python3
"""W3.16 / v2.4.86 验收：中继**只有 UDP 承载**时，唤醒仍然到达持有者。

背景（[ADR-020](../../../docs/02-architecture/adr/ADR-020-relay-udp-transport.md) §3.1 / §3.4）：
这台腾讯云轻量 `119.29.150.254` 的入向 TCP 除 `22` 外一律在数据阶段被边缘注入 RST，
中继的 TCP 承载根本不可用。v2.4.85 已证明 UDP 承载能跑，但**只有量具能跑**——
守护进程的订阅与唤醒还都走 TCP。本脚本验收把它们接到 UDP 上。

判据（全部来自真实进程 / 真实中继 / 真实报文，不回填）：

1. 持有者（`192.168.1.221`）用 `nat.relay_udp` 订阅公网中继 `119.29.150.254:7101`；
   盒子侧 `udp_stats_7101.json` 的 `sessions_active ≥ 1`（订阅真的到了 UDP 注册表）；
2. 强制中继路（`only_kind=relay`）：`ok=true`、`path=relay-udp`、`ack_status=OK`，
   且应答来自持有者守护进程（`ack` 里有 `WAKEUP_ACK` 字段）；
3. 生产默认路：把通告里的私网端点剥掉（模拟「对端在别的 NAT 后面」），
   `nat wake` 不带 `only_kind` 仍应落到 `relay-udp`，且 `attempts` 里能看到直连确实失败过；
4. 对照：同一中继只配 **TCP** 承载时唤醒必须失败——失败原因要能读出「连不上中继」，
   而不是「对端离线」。这条同时证明「UDP 不是可有可无的装饰」；
5. 每一步都记盒子侧 `data_in / deliver_out` 的增量，证明报文真的过了公网中继；
6. 不改门禁：`s6_status=not_run`、`is_s6_evidence=false`、`g2_claim=false`。

用法：
    python tests/e2e/w53/udp_relay_wake.py            # 全流程
    python tests/e2e/w53/udp_relay_wake.py --skip-build   # 复用 221 上已构建的二进制
"""

from __future__ import annotations

import hashlib

import argparse
import json
import os
import shutil
import socket
import subprocess
import sys
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
REPORTS = Path(__file__).resolve().parent / "reports"

KEY = os.environ.get("CR_KEY", str(Path(os.environ.get("TEMP", "/tmp")) / "crkeys" / "id_ed25519"))
PSK_FILE = Path(os.environ.get("TEMP", "/tmp")) / "crkeys" / "psk.txt"

VANTAGE = "119.29.150.254"          # 公网观测机（同时是中继）
RELAY_UDP = f"{VANTAGE}:7101"       # 专用 UDP 端口（云防火墙已放行，见 §3.4）
RELAY_TCP = f"{VANTAGE}:7101"       # 对照：同端口 TCP，实测不可用

HOLDER_SSH = "192.168.1.221"
HOLDER_BIND = "0.0.0.0"
HOLDER_SELF = "192.168.1.221"
HOLDER_WORK = "/tmp/w53"
HOLDER_HTTP = 19900
HOLDER_WAKE = 19901
HOLDER_BT = 19902

REQ_BIND = "192.168.1.210"
REQ_HTTP = 17800
REQ_WAKE = 17801
REQ_BT = 17802

FILE_HASH = "w53-udp-relay-file"
INFOHASH = "0123456789abcdef0123456789abcdef01234567"

CARRIER_ERRORS = ("Connection closed by", "Connection reset by peer",
                  "kex_exchange_identification", "MaxStartups",
                  "Broken pipe", "banner exchange")


class Ctx:
    def __init__(self) -> None:
        self.checks: list[dict] = []
        self.obs: dict = {}

    def check(self, label: str, ok: bool, actual: str = "") -> bool:
        self.checks.append({"label": label, "ok": bool(ok), "actual": actual})
        print(f"[{'PASS' if ok else 'FAIL'}] {label}" + (f"  -- {actual}" if actual else ""))
        return bool(ok)

    def record(self, key: str, value) -> None:
        self.obs[key] = value

    @property
    def failed(self) -> int:
        return sum(1 for c in self.checks if not c["ok"])


def ssh(host: str, cmd: str, timeout: int = 120) -> tuple[int, str]:
    p = subprocess.run(
        ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10",
         "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null",
         "-o", "LogLevel=ERROR", "-i", KEY, f"root@{host}", cmd],
        capture_output=True, text=True, timeout=timeout)
    return p.returncode, (p.stdout or "") + (p.stderr or "")


def ssh_ok(host: str, cmd: str, want, tries: int = 2) -> tuple[bool, str]:
    out = ""
    for _ in range(tries):
        rc, out = ssh(host, cmd)
        if rc == 0 and want(rc, out):
            return True, out
        time.sleep(0.5)
    return False, out


def free_port(kind: str = "tcp") -> int:
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM if kind == "tcp" else socket.SOCK_DGRAM)
    try:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]
    finally:
        s.close()


def build_config(path: Path, work: Path, sample: Path, golden: Path,
                 nat: dict) -> None:
    cfg = {
        "bind": REQ_BIND,
        "nodes": {
            "A": {"role": "holder", "host": HOLDER_SELF,
                  "http_port": HOLDER_HTTP, "wake_udp_port": HOLDER_WAKE,
                  "bt_port": HOLDER_BT},
            "B": {"role": "requester", "host": REQ_BIND,
                  "http_port": REQ_HTTP, "wake_udp_port": REQ_WAKE,
                  "bt_port": REQ_BT},
        },
        "sample_path": str(sample),
        "keywords": ["coldrevive", "w53", "udp-relay"],
        "workdir": str(work),
        "golden_req": str(golden / "wakeup_req.v1.json"),
        "golden_ack": str(golden / "wakeup_ack.v1.json"),
        "nat": nat,
    }
    path.write_text(json.dumps(cfg, indent=2) + "\n", encoding="utf-8")


def holder_config_json(nat: dict) -> str:
    cfg = {
        "bind": HOLDER_BIND,
        "nodes": {
            "A": {"role": "holder", "host": HOLDER_SELF,
                  "http_port": HOLDER_HTTP, "wake_udp_port": HOLDER_WAKE,
                  "bt_port": HOLDER_BT},
            "B": {"role": "requester", "host": REQ_BIND,
                  "http_port": REQ_HTTP, "wake_udp_port": REQ_WAKE,
                  "bt_port": REQ_BT},
        },
        "sample_path": f"{HOLDER_WORK}/sample.bin",
        "keywords": ["coldrevive", "w53", "udp-relay"],
        "workdir": HOLDER_WORK,
        "golden_req": f"{HOLDER_WORK}/golden/wakeup_req.v1.json",
        "golden_ack": f"{HOLDER_WORK}/golden/wakeup_ack.v1.json",
        "nat": nat,
    }
    return json.dumps(cfg, indent=2) + "\n"


def http_json(host: str, port: int, path: str, timeout: float = 5.0):
    with urllib.request.urlopen(f"http://{host}:{port}{path}", timeout=timeout) as r:
        return json.loads(r.read().decode("utf-8"))


def wait_health(host: str, port: int, tries: int = 40) -> bool:
    for _ in range(tries):
        try:
            with urllib.request.urlopen(f"http://{host}:{port}/health", timeout=2) as r:
                if r.status == 200:
                    return True
        except Exception:
            pass
        time.sleep(0.5)
    return False


def wait_health_remote(port: int, tries: int = 40) -> bool:
    for _ in range(tries):
        ok, _ = ssh_ok(HOLDER_SSH, f"curl -s -o /dev/null -w '%{{http_code}}' -m 2 "
                                   f"http://127.0.0.1:{port}/health", lambda c, o: "200" in o,
                       tries=1)
        if ok:
            return True
        time.sleep(0.5)
    return False


def cli(cfg: Path, node: str, args: list[str], timeout: int = 90) -> dict:
    exe = REPO / "target" / "debug" / "coldrevive-cli.exe"
    env = dict(os.environ, COLDREVIVE_T0_CONFIG=str(cfg))
    p = subprocess.run([str(exe), "--json", "--node", node] + args,
                       capture_output=True, text=True, timeout=timeout, env=env)
    out = (p.stdout or "").strip()
    if not out:
        raise RuntimeError(f"CLI 无输出: {(p.stderr or '')[:300]}")
    return json.loads(out.splitlines()[-1])


def relay_stats() -> dict:
    ok, out = ssh_ok(VANTAGE, "cat /root/udp_stats_7101.json",
                     lambda c, o: "data_in" in o)
    if not ok:
        raise RuntimeError(f"读不到中继计数: {out[:200]}")
    return json.loads(out)


def relay_delta(before: dict, after: dict) -> dict:
    keys = ("datagrams", "data_in", "deliver_out", "status_out", "mac_fail",
            "replay", "target_absent", "reject_out")
    return {k: after.get(k, 0) - before.get(k, 0) for k in keys}


def closed_port_endpoints(adv: dict, port: int = 19999) -> dict:
    """把通告里的候选端口全改成一个**没有监听**的口，用来模拟「直连打不进来」。

    刻意**不改 IP、不新增地址**：地址仍是持有者真实通告的那一个，只是端口上没人应答。
    这样默认路径会真的去试直连并失败，从而落到中继——而不是靠删掉候选来「制造」结论。
    """
    out = json.loads(json.dumps(adv))
    for ep in out.get("endpoints", []):
        ep["port"] = port
        ep["internal_port"] = port
    return out


def strip_private_endpoints(adv: dict) -> tuple[dict, list[str]]:
    """剥掉通告里的私网端点，模拟「对端在另一个 NAT 后面」。

    只删私网/RFC1918 与回环端点；公网端点原样保留。**不伪造任何地址**——
    这是把「同 LAN 的捷径」去掉，让默认路径真的去试直连并失败，从而落到中继。
    """
    out = dict(adv)
    kept, dropped = [], []
    for ep in adv.get("endpoints", []):
        ip = str(ep.get("ip", ""))
        if ip.startswith(("10.", "192.168.", "172.16.", "172.17.", "172.18.",
                          "172.19.", "172.2", "172.30.", "172.31.", "127.")):
            dropped.append(ip)
        else:
            kept.append(ep)
    out["endpoints"] = kept
    return out, dropped


def wait_port_free(port: int, tries: int = 20) -> bool:
    """等远端端口真的空出来。

    上一轮残留的守护进程会占着端口，新进程起来只会 "Address already in use"，
    而 `/health` 却是**旧进程**在应答——不查端口就会拿旧进程的证据冒充本轮结论。
    """
    for _ in range(tries):
        ok, out = ssh_ok(HOLDER_SSH, f"ss -lnu | grep -c ':{port}' || true",
                         lambda c, o: (o.strip().splitlines() or ["1"])[-1].strip() == "0",
                         tries=1)
        if ok:
            return True
        time.sleep(0.5)
    return False


def start_holder(daemon: str, cfg_remote: str, ctx: Ctx) -> None:
    ssh(HOLDER_SSH, f"pkill -x coldrevived 2>/dev/null; fuser -k -n udp {HOLDER_WAKE} "
                    f"2>/dev/null; sleep 0.5")
    ctx.check(f"221 上唤醒端口 {HOLDER_WAKE} 已空出（无残留进程）",
              wait_port_free(HOLDER_WAKE))
    # 用子 shell `( ... & )` 包住后台进程：只写 `&` 时 ssh 会一直等着
    # 那个继承了通道的孙进程，命令看起来「跑起来了」但调用方阻塞到超时。
    rc, out = ssh(HOLDER_SSH,
                  f"cd {HOLDER_WORK} && (setsid nohup {daemon} --config {cfg_remote} "
                  f"--node-id A --bind {HOLDER_BIND} --self-host {HOLDER_SELF} "
                  f"--workdir {HOLDER_WORK} --sample {HOLDER_WORK}/sample.bin "
                  f"> {HOLDER_WORK}/holder.log 2>&1 < /dev/null &) ; sleep 1; echo started",
                  timeout=25)
    ctx.check("持有者进程已拉起（221）", "started" in out, out.strip()[:120])
    ctx.check("持有者 /health 200", wait_health_remote(HOLDER_HTTP))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--skip-build", action="store_true",
                    help="不再往 221 传二进制（假定已部署）")
    ap.add_argument("--timeout", type=int, default=90)
    args = ap.parse_args()

    started = datetime.now(timezone.utc)
    work = REPO / ".run" / "w53"
    shutil.rmtree(work, ignore_errors=True)
    (work / "golden").mkdir(parents=True, exist_ok=True)
    ctx = Ctx()

    golden_src = REPO / "tests" / "fixtures" / "t0" / "golden"
    for name in ("wakeup_req.v1.json", "wakeup_ack.v1.json"):
        shutil.copy2(golden_src / name, work / "golden" / name)
    sample = work / "sample.bin"
    sample.write_bytes(os.urandom(4096))

    if not PSK_FILE.is_file():
        print(f"缺少 PSK 文件: {PSK_FILE}")
        return 2
    psk = PSK_FILE.read_text(encoding="utf-8").strip()

    daemon_local = REPO / "target" / "debug" / "coldrevived.exe"
    cli_local = REPO / "target" / "debug" / "coldrevive-cli.exe"
    if not (daemon_local.is_file() and cli_local.is_file()):
        print("本机二进制缺失，先 cargo build --offline -p coldrevived -p coldrevive-cli")
        return 2

    # ---------------- 部署 ----------------
    ok, out = ssh_ok(VANTAGE, "cat /root/udp_stats_7101.json", lambda c, o: "data_in" in o)
    ctx.check("公网中继在跑（udp 7101，读得到计数）", ok, out.strip()[:120])
    if not ok:
        return 2

    if not args.skip_build:
        rc, out = ssh(HOLDER_SSH,
                      "ls -l /tmp/crfull/target/release/coldrevived "
                      "/tmp/crfull/target/release/coldrevive-cli 2>&1 | tail -2")
        ctx.check("221 上已有 Linux 版守护进程", rc == 0 and "coldrevived" in out,
                  out.strip()[:200])

    remote_daemon = "/tmp/crfull/target/release/coldrevived"
    ssh(HOLDER_SSH, f"rm -rf {HOLDER_WORK}; mkdir -p {HOLDER_WORK}/golden")
    # 种子与 golden 必须真的在 221 上（配置里是绝对路径，缺文件守护进程会拒启）
    scp = ["scp", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10",
           "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null",
           "-o", "LogLevel=ERROR", "-i", KEY]
    subprocess.run(scp + [str(sample)] + [f"root@{HOLDER_SSH}:{HOLDER_WORK}/sample.bin"],
                   check=False)
    for name in ("wakeup_req.v1.json", "wakeup_ack.v1.json"):
        subprocess.run(scp + [str(work / "golden" / name)]
                       + [f"root@{HOLDER_SSH}:{HOLDER_WORK}/golden/{name}"], check=False)

    # 唤醒请求里的 file_hash 必须是持有者索引里真实存在的那一个：
    # 索引键 = 样本文件的 SHA-256（cr-indexer），所以在样本落盘后自己算。
    file_hash = hashlib.sha256(sample.read_bytes()).hexdigest()
    ctx.record("file_hash", file_hash)
    rc, out = ssh(HOLDER_SSH, f"sha256sum {HOLDER_WORK}/sample.bin | cut -d' ' -f1")
    ctx.check("221 上的样本与请求侧同一份（sha256 一致）", out.strip() == file_hash,
              out.strip()[:80])

    holder_nat = {"enabled": True, "relay_udp": RELAY_UDP, "relay_key": psk}
    holder_cfg_text = holder_config_json(holder_nat)
    rc, out = ssh(HOLDER_SSH, f"cat > {HOLDER_WORK}/holder.json <<'EOF'\n{holder_cfg_text}EOF\n"
                              f"grep -E 'relay_udp|workdir' {HOLDER_WORK}/holder.json")
    ctx.check("持有者配置已落盘（221，nat.relay_udp 在文件里）",
              '"relay_udp"' in out and HOLDER_WORK in out, out.strip()[:200])

    before = relay_stats()
    start_holder(remote_daemon, f"{HOLDER_WORK}/holder.json", ctx)

    # ---------------- 判据 1：订阅真的进了 UDP 注册表 ----------------
    sub_ok = False
    for _ in range(20):
        st = relay_stats()
        if st.get("sessions_active", 0) >= 1:
            sub_ok = True
            break
        time.sleep(0.5)
    st = relay_stats()
    ctx.check("中继 UDP 注册表里有订阅会话（sessions_active≥1）", sub_ok,
              f"sessions_active={st.get('sessions_active')} hello={st.get('hello')}")
    ok, log = ssh_ok(HOLDER_SSH, f"grep -c 'relay-sub' {HOLDER_WORK}/holder.log",
                     lambda c, o: o.strip().splitlines()[-1].isdigit()
                     and int(o.strip().splitlines()[-1]) > 0)
    ctx.check("持有者日志有 relay-sub 订阅记录", ok, log.strip().splitlines()[-1][:60] if log else "")
    ctx.record("holder_log_subscribe", log.strip()[-300:])

    ok, adv_raw = ssh_ok(HOLDER_SSH, f"curl -s -m 5 http://127.0.0.1:{HOLDER_HTTP}/nat/advertise",
                         lambda c, o: '"node_id"' in o)
    ctx.check("取到持有者通告", ok, adv_raw.strip()[:160])
    if not ok:
        return finish(ctx, args, started, 2)
    adv = json.loads(adv_raw)
    ctx.record("advertise", {"node_id": adv.get("node_id"),
                             "endpoints": [e.get("ip") for e in adv.get("endpoints", [])]})

    adv_full = work / "adv-full.json"
    adv_full.write_text(json.dumps(adv), encoding="utf-8")
    adv_closed = closed_port_endpoints(adv)
    ctx.record("direct_candidates_pointed_at_closed_port", 19999)
    adv_closed_path = work / "adv-closed-port.json"
    adv_closed_path.write_text(json.dumps(adv_closed), encoding="utf-8")

    # ---------------- 请求者（本机 A）----------------
    procs: list[subprocess.Popen] = []

    def start_requester(tag: str, nat: dict) -> Path:
        cfg = work / f"req-{tag}.json"
        build_config(cfg, work, sample, work / "golden", nat)
        log = open(work / f"req-{tag}.log", "w", encoding="utf-8")
        p = subprocess.Popen([str(daemon_local), "--config", str(cfg), "--node-id", "B",
                              "--bind", REQ_BIND, "--self-host", REQ_BIND,
                              "--workdir", str(work), "--sample", str(sample)],
                             stdout=log, stderr=subprocess.STDOUT)
        procs.append(p)
        return cfg

    req_nat = {"enabled": True, "relay_udp": RELAY_UDP, "relay_key": psk}
    req_cfg = start_requester("udp", req_nat)
    ctx.check("请求者 /health 200（本机 A）", wait_health(REQ_BIND, REQ_HTTP))

    peer = cli(req_cfg, "B", ["nat", "peer", str(adv_full)])
    ctx.check("通告已装入请求者", bool(peer.get("ok")), f"endpoints={peer.get('endpoints')}")

    # ---------------- 判据 2：强制中继路 ----------------
    before = relay_stats()
    wake = cli(req_cfg, "B", ["nat", "wake", file_hash, INFOHASH, "relay"], timeout=args.timeout)
    delta = relay_delta(before, relay_stats())
    ctx.record("wake_forced_relay", {"wake": wake, "relay_delta": delta})
    ctx.check("强制中继路：ok=true", bool(wake.get("ok")), str(wake.get("error"))[:160])
    ctx.check("强制中继路：path=relay-udp", wake.get("path") == "relay-udp",
              f"path={wake.get('path')} strategy={wake.get('strategy')}")
    ctx.check("强制中继路：持有者回了 WAKEUP_ACK", wake.get("ack_status") == "OK",
              f"ack_status={wake.get('ack_status')}")
    ctx.check("报文真的过了公网中继（data_in≥2 且 deliver_out≥2）",
              delta["data_in"] >= 2 and delta["deliver_out"] >= 2, str(delta))
    ctx.check("中继无 MAC 失败 / 无重放 / 无 target_absent",
              delta["mac_fail"] == 0 and delta["replay"] == 0 and delta["target_absent"] == 0,
              str(delta))

    # ---------------- 判据 3：生产默认路（剥掉私网端点）----------------
    peer2 = cli(req_cfg, "B", ["nat", "peer", str(adv_closed_path)])
    ctx.check("已装入「候选端口无监听」的通告（模拟直连不可达）", bool(peer2.get("ok")),
              f"endpoints={peer2.get('endpoints')}")
    before = relay_stats()
    wake2 = cli(req_cfg, "B", ["nat", "wake", file_hash, INFOHASH], timeout=args.timeout)
    delta2 = relay_delta(before, relay_stats())
    attempts = wake2.get("attempts") or []
    kinds = [(a.get("kind"), a.get("ok")) for a in attempts]
    ctx.record("wake_default_path", {"wake": wake2, "relay_delta": delta2,
                                     "attempt_kinds": kinds})
    ctx.check("默认路：最终仍成功且落到 relay-udp",
              bool(wake2.get("ok")) and wake2.get("path") == "relay-udp",
              f"ok={wake2.get('ok')} path={wake2.get('path')} err={str(wake2.get('error'))[:100]}")
    ctx.check("默认路：直连候选确实试过且失败（不是直接跳到中继）",
              any(k in ("private-lan", "stun-mapped", "mapping") and ok is False
                  for k, ok in kinds), str(kinds))

    # ---------------- 判据 4：对照，只有 TCP 承载 ---------------
    for p in procs:
        p.terminate()
    procs.clear()
    time.sleep(1.0)
    tcp_nat = {"enabled": True, "relay": RELAY_TCP, "relay_key": psk}
    tcp_cfg = start_requester("tcp", tcp_nat)
    ctx.check("对照请求者 /health 200", wait_health(REQ_BIND, REQ_HTTP))
    cli(tcp_cfg, "B", ["nat", "peer", str(adv_full)])
    before = relay_stats()
    wake3 = cli(tcp_cfg, "B", ["nat", "wake", file_hash, INFOHASH, "relay"], timeout=args.timeout)
    delta3 = relay_delta(before, relay_stats())
    ctx.record("control_tcp_only", {"wake": wake3, "relay_delta": delta3})
    ctx.check("对照：只配 TCP 承载时唤醒必须失败", not wake3.get("ok"),
              f"ok={wake3.get('ok')} path={wake3.get('path')}")
    err_text = json.dumps(wake3.get("attempts") or [], ensure_ascii=False) + str(wake3.get("error"))
    ctx.check("对照：失败原因指向「连不上中继」而非「对端离线」",
              any(k in err_text for k in ("timeout", "超时", "Connection", "connect", "拒绝", "不可达")),
              err_text[:180])
    ctx.check("对照：UDP 侧计数没有新增投递（证明失败不是伪装成功）",
              delta3["deliver_out"] == 0, str(delta3))

    for p in procs:
        p.terminate()
    ssh(HOLDER_SSH, "pkill -x coldrevived 2>/dev/null; sleep 0.3")
    return finish(ctx, args, started, 0)


def finish(ctx: Ctx, args, started: datetime, code: int) -> int:
    REPORTS.mkdir(parents=True, exist_ok=True)
    status = "pass" if ctx.failed == 0 else "fail"
    report = {
        "spike": "W3.16",
        "title": "中继只有 UDP 承载时的订阅与唤醒",
        "status": status,
        "is_s6_evidence": False,
        "s6_status": "not_run",
        "g2_claim": False,
        "started_at": started.isoformat(),
        "finished_at": datetime.now(timezone.utc).isoformat(),
        "relay_udp": RELAY_UDP,
        "holder": HOLDER_SELF,
        "requester": REQ_BIND,
        "checks": ctx.checks,
        "observations": ctx.obs,
        "failed": ctx.failed,
    }
    out = REPORTS / f"udp_relay_wake_{datetime.now().strftime('%Y%m%d-%H%M%S')}.json"
    out.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    print(f"\n=== {status.upper()} ({len(ctx.checks) - ctx.failed}/{len(ctx.checks)}) -> {out}")
    return code if status == "pass" else 1


if __name__ == "__main__":
    sys.exit(main())
