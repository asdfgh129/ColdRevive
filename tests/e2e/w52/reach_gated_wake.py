#!/usr/bin/env python3
"""W5.2 / ADR-013 验收：唤醒前先查可达性，symmetric 对端零直连包。

判据（全部来自真实 CLI / HTTP / tcpdump，不回填）：
1. 持有者 222（symmetric）真实 `nat probe`，中继里得到 `relay-only` 声明；
2. 请求者 A（port-restricted）`nat wake` **不带** only_kind：
   `reach_lookup.class=relay-only`、`skip_direct=true`、`attempts` 只有 relay；
3. 222 的 ens160 抓包里，主测量窗口对 `112.5.60.4` 的 UDP 报文为 0；
4. 量具对照：另起一个窗口用 `only_kind=stun-mapped` 强制直连，必须抓到 >0 包，
   否则「主窗口 0 包」只说明抓包没工作，不算证据；
5. 结果只写观测；`s6_status=not_run`、`is_s6_evidence=false`，不改 spikes.json。

中继用一次性实验 PSK（64 hex，写死在脚本里但只用于 192.168.1.221:7200 的实验实例），
避免 relay 因「非回环无密钥」拒绝启动；`coldrevive-nat --lookup` 不支持 PSK，
所以声明类别从 `nat wake` 的 `reach_lookup` 与 `nat reach` 读，不去查明文 CLI。
`nat reach` 只查不发包，用来在开抓包窗口前等声明传播完；强制直连对照最多试 3 次，
因为请求侧自映射 STUN 偶发失败会让那一轮根本不发直接包（量具没起步，不是抓不到）。
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
import tarfile
import tempfile
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
REPORTS = Path(__file__).resolve().parent / "reports"

KEY = os.environ.get("CR_KEY", str(Path(os.environ.get("TEMP", "/tmp")) / "crkeys" / "id_ed25519"))
RELAY_HOST = "192.168.1.221"
HOLDER_SSH = "192.168.1.222"
HOLDER_SELF = "192.168.34.222"       # 222 的 ens160，不是 SSH 管理地址 192.168.1.222
REQ_IP = "192.168.1.210"
EGRESS_REQ = "112.5.60.4"
EGRESS_HOLDER = "112.5.32.34"
RELAY_PORT = 7200
RELAY = f"{RELAY_HOST}:{RELAY_PORT}"
PSK = "5a17c9e4b83d2f60a1c47e59d0b3862f7e41a9c3508bd6e2f47c1a90b3d5e678"
STUN = ["stun.cloudflare.com:3478", "stun.telnyx.com:3478",
        "stun.miwifi.com:3478", "stun.chat.bilibili.com:3478"]
HOLDER_PORTS = {"http": 19900, "wake": 19901, "bt": 19902}
REQ_PORTS = {"http": 17800, "wake": 17801, "bt": 17802}
HOLDER_WORK = "/root/crreach"
HOLDER_BIN = "/root/bin"
INFOHASH = "reach-gate-222"

CARRIER_ERRORS = ("Connection closed by", "Connection reset by peer",
                  "kex_exchange_identification", "MaxStartups", "Connection refused",
                  "Broken pipe", "banner exchange")


class Ctx:
    def __init__(self) -> None:
        self.checks: list[dict] = []
        self.obs: dict = {}

    def check(self, label: str, ok: bool, actual: str = "") -> bool:
        self.checks.append({"label": label, "ok": bool(ok), "actual": actual})
        print(f"[{'PASS' if ok else 'FAIL'}] {label}" + (f"  -- {actual}" if actual else ""))
        return bool(ok)


def ssh_base(host: str) -> list[str]:
    return ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8",
            "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null",
            "-o", "LogLevel=ERROR", "-i", KEY, f"root@{host}"]


def ssh(host: str, cmd: str, timeout: int = 120) -> tuple[int, str]:
    try:
        p = subprocess.run(ssh_base(host) + [cmd], capture_output=True, text=True,
                           timeout=timeout)
        return p.returncode, (p.stdout or "") + (p.stderr or "")
    except subprocess.TimeoutExpired as e:
        out = e.stdout or ""
        if isinstance(out, bytes):
            out = out.decode("utf-8", "replace")
        return 124, out


def ssh_ok(host: str, cmd: str, pred, tries: int = 3, timeout: int = 120) -> tuple[bool, str]:
    last = ""
    for _ in range(tries):
        code, out = ssh(host, cmd, timeout)
        last = out
        if any(c in out for c in CARRIER_ERRORS):
            time.sleep(1.5)
            continue
        return pred(code, out), out
    return False, last


def scp(src: str, dst: str, timeout: int = 180) -> bool:
    p = subprocess.run(
        ["scp", "-q", "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=no",
         "-o", "UserKnownHostsFile=/dev/null", "-o", "LogLevel=ERROR",
         "-i", KEY, src, dst], capture_output=True, text=True, timeout=timeout)
    return p.returncode == 0


def local_cli(cfg: Path, node: str, args: list[str], timeout: int = 90) -> dict:
    exe = REPO / "target" / "debug" / "coldrevive-cli.exe"
    env = dict(os.environ, COLDREVIVE_T0_CONFIG=str(cfg))
    p = subprocess.run([str(exe), "--json", "--node", node] + args,
                       capture_output=True, text=True, timeout=timeout, env=env)
    out = (p.stdout or "").strip()
    if not out:
        raise RuntimeError(f"本机 CLI 无输出: {(p.stderr or '')[:300]}")
    return json.loads(out.splitlines()[-1])


def build_config(path: Path, holder_self: str, bind: str, node: str,
                 ports: dict, work: str, sample: str, golden_dir: str) -> None:
    other = "B" if node == "A" else "A"
    nodes = {
        node: {"role": "holder" if node == "A" else "requester",
               "host": holder_self if node == "A" else REQ_IP,
               "http_port": ports["http"], "wake_udp_port": ports["wake"],
               "bt_port": ports["bt"]},
        other: {"role": "holder" if other == "A" else "requester",
                "host": holder_self if other == "A" else REQ_IP,
                "http_port": HOLDER_PORTS["http"] if other == "A" else REQ_PORTS["http"],
                "wake_udp_port": HOLDER_PORTS["wake"] if other == "A" else REQ_PORTS["wake"],
                "bt_port": HOLDER_PORTS["bt"] if other == "A" else REQ_PORTS["bt"]},
    }
    cfg = {
        "bind": bind,
        "nodes": nodes,
        "sample_path": sample,
        "keywords": ["coldrevive", "w52", "reach-gate"],
        "workdir": work,
        "golden_req": f"{golden_dir}/wakeup_req.v1.json",
        "golden_ack": f"{golden_dir}/wakeup_ack.v1.json",
        "nat": {"enabled": True, "stun_servers": STUN, "relay": RELAY,
                "relay_key": PSK},
    }
    path.write_text(json.dumps(cfg, indent=2) + "\n", encoding="utf-8")


def wait_health(remote: bool, host: str, port: int, tries: int = 30) -> bool:
    for _ in range(tries):
        if remote:
            ok, out = ssh_ok(host, f"curl -s -o /dev/null -w '%{{http_code}}' -m 2 "
                                   f"http://127.0.0.1:{port}/health",
                             lambda c, o: "200" in o, tries=1)
        else:
            try:
                with urllib.request.urlopen(f"http://{host or '127.0.0.1'}:{port}/health", timeout=2) as r:
                    ok, out = r.status == 200, str(r.status)
            except Exception as e:
                ok, out = False, str(e)
        if ok:
            return True
        time.sleep(0.5)
    return False


def deploy(args, ctx: Ctx, work: Path) -> tuple[Path | None, Path | None]:
    if args.skip_build:
        local_daemon = REPO / "target" / "debug" / "coldrevived.exe"
        local_cli_bin = REPO / "target" / "debug" / "coldrevive-cli.exe"
        ok = local_daemon.is_file() and local_cli_bin.is_file()
        ctx.check("本机二进制已存在（--skip-build）", ok,
                  f"{local_daemon.name} / {local_cli_bin.name}")
        return (local_daemon, local_cli_bin) if ok else (None, None)

    p = subprocess.run(["cargo", "build", "--offline",
                        "-p", "coldrevived", "-p", "coldrevive-cli"],
                       cwd=REPO, capture_output=True, text=True, timeout=300)
    ctx.check("本机构建 daemon / CLI", p.returncode == 0 and "Finished" in p.stderr,
              p.stderr.strip().splitlines()[-1] if p.stderr.strip() else "")

    # 221 的原仓库有全套离线依赖缓存和旧 Cargo.lock；直接上传当前 Cargo.lock 会引入
    # 本机新加的 bytes/openssl-sys 解析，221 缺 pkg-config 会编译失败。这里只覆盖
    # 协议面 cr-nat 与 daemon 里唤醒相关的三个文件，沿用 221 原锁文件离线构建。
    tar = work / "w52-code.tgz"
    with tarfile.open(tar, "w:gz") as tf:
        tf.add(REPO / "src" / "crates" / "cr-nat" / "src",
               arcname="crates/cr-nat/src")
        tf.add(REPO / "src" / "crates" / "cr-nat" / "Cargo.toml",
               arcname="crates/cr-nat/Cargo.toml")
        tf.add(REPO / "src" / "crates" / "cr-core" / "src" / "lib.rs",
               arcname="crates/cr-core/src/lib.rs")
        for name in ("nat_path.rs", "relay_sub.rs"):
            tf.add(REPO / "src" / "bins" / "coldrevived" / "src" / name,
                   arcname=f"bins/coldrevived/src/{name}")
        tf.add(REPO / "tests" / "fixtures", arcname="fixtures")
    ok = scp(str(tar), f"root@{RELAY_HOST}:/tmp/w52-code.tgz")
    if not ctx.check("上传协议面源码到 221", ok):
        return None, None
    build = ("rm -rf /opt/w52-repo && cp -a /opt/coldrevive/repo /opt/w52-repo && "
             "tar xzf /tmp/w52-code.tgz -C /opt/w52-repo/src && "
             "touch /opt/w52-repo/src/crates/cr-nat/src/*.rs "
             "/opt/w52-repo/src/crates/cr-core/src/lib.rs "
             "/opt/w52-repo/src/bins/coldrevived/src/nat_path.rs "
             "/opt/w52-repo/src/bins/coldrevived/src/relay_sub.rs && "
             "cd /opt/w52-repo && /root/.cargo/bin/cargo build --offline "
             "-p coldrevived -p coldrevive-cli -p coldrevive-relay 2>&1 | tail -4")
    ok, out = ssh_ok(RELAY_HOST, build, lambda c, o: c == 0 and "error" not in o.lower(),
                     timeout=300)
    if not ctx.check("221 离线构建 daemon / CLI / relay", ok, out.strip()[-160:]):
        return None, None

    for name in ("coldrevived", "coldrevive-cli"):
        local_bin = work / name
        if not scp(f"root@{RELAY_HOST}:/opt/w52-repo/target/debug/{name}", str(local_bin)):
            ctx.check(f"取回 {name}", False)
            return None, None
        ok = scp(str(local_bin), f"root@{HOLDER_SSH}:{HOLDER_BIN}/{name}")
        if not ctx.check(f"部署 {name} 到 222", ok):
            return None, None
    ok, out = ssh_ok(HOLDER_SSH, f"chmod +x {HOLDER_BIN}/coldrevived {HOLDER_BIN}/coldrevive-cli "
                                 f"&& {HOLDER_BIN}/coldrevived --version 2>&1 | head -1",
                     lambda c, o: c == 0)
    ctx.check("222 上二进制可执行", ok, out.strip()[:80])
    return (REPO / "target" / "debug" / "coldrevived.exe",
            REPO / "target" / "debug" / "coldrevive-cli.exe")


def capture_start(which: str) -> None:
    cap = f"/tmp/w52-cap-{which}.txt"
    cmd = (f"rm -f {cap}; setsid nohup timeout 25 tcpdump -i ens160 -n -l "
           f"'udp and host {EGRESS_REQ}' > {cap} 2>&1 < /dev/null & "
           f"sleep 1; for i in $(seq 1 15); do grep -q 'listening on' {cap} "
           f"&& break; sleep 0.2; done; echo started")
    ssh(HOLDER_SSH, cmd, timeout=20)


def capture_read(ctx: Ctx, key: str, which: str) -> int:
    ok, out = ssh_ok(HOLDER_SSH, f"cat /tmp/w52-cap-{which}.txt",
                     lambda c, o: True, tries=2)
    lines = [ln for ln in out.splitlines() if EGRESS_REQ in ln and "IP " in ln]
    ctx.obs[key] = lines[:20]
    return len(lines)


def cleanup(args, procs: list[subprocess.Popen]) -> None:
    for p in procs:
        if p.poll() is None:
            p.terminate()
            try:
                p.wait(timeout=5)
            except subprocess.TimeoutExpired:
                p.kill()
    if not args.keep:
        ssh(RELAY_HOST, "pkill -x coldrevive-rela || true", 20)
        ssh(HOLDER_SSH, "pkill -x coldrevived || true", 20)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--keep", action="store_true", help="保留进程和抓包文本，便于排障")
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--timeout", type=int, default=30)
    args = ap.parse_args()

    ctx = Ctx()
    started = datetime.now(timezone.utc)
    work = Path(tempfile.mkdtemp(prefix="w52-"))
    procs: list[subprocess.Popen] = []
    status = "fail"
    try:
        ok_a, _ = ssh_ok(RELAY_HOST, "echo __ok__; command -v tcpdump",
                         lambda c, o: "__ok__" in o and "tcpdump" in o)
        ok_h, _ = ssh_ok(HOLDER_SSH, "echo __ok__; command -v tcpdump; command -v curl",
                         lambda c, o: "__ok__" in o and "tcpdump" in o and "curl" in o)
        ctx.check("载体：221 / 222 可 ssh 且量具齐全", ok_a and ok_h)
        if not (ok_a and ok_h):
            status = "blocked"
            return finish(ctx, args, started, status, work, 2)

        busy = []
        for p in REQ_PORTS.values():
            s = socket.socket()
            try:
                s.bind(("0.0.0.0", p))
            except OSError:
                busy.append(p)
            finally:
                s.close()
        ctx.check("本机 17800/17801/17802 未被占用", not busy, f"busy={busy}")
        if busy:
            status = "blocked"
            return finish(ctx, args, started, status, work, 2)

        local_daemon, _ = deploy(args, ctx, work)
        if local_daemon is None:
            status = "blocked"
            return finish(ctx, args, started, status, work, 2)

        # 一次性实验中继（带 PSK），只服务本机/222。
        relay_cmd = (f"pkill -x coldrevive-rela || true; sleep 0.3; "
                     f"mkdir -p /opt/w52-relay; setsid nohup "
                     f"/opt/w52-repo/target/debug/coldrevive-relay --listen {RELAY} "
                     f"--auth-key {PSK} --stats-file /opt/w52-relay/stats.json "
                     f"> /opt/w52-relay/run.log 2>&1 < /dev/null & sleep 1; echo started")
        ssh(RELAY_HOST, relay_cmd, timeout=30)
        ok, out = ssh_ok(RELAY_HOST,
                         f"python3 -c \"import socket;s=socket.create_connection(('{RELAY_HOST}',{RELAY_PORT}),2);print('OK')\"",
                         lambda c, o: "OK" in o)
        ctx.check("实验中继真实接受 TCP", ok, out.strip()[:80])
        if not ok:
            status = "blocked"
            return finish(ctx, args, started, status, work, 2)

        sample = work / "sample.bin"
        sample.write_bytes(bytes((i * 37 + 11) % 256 for i in range(65536)))
        file_hash = hashlib.sha256(sample.read_bytes()).hexdigest()
        golden = REPO / "tests" / "fixtures" / "t0" / "golden"
        ssh_ok(HOLDER_SSH, f"mkdir -p {HOLDER_WORK} && chmod +x {HOLDER_BIN}/coldrevived",
               lambda c, o: c == 0, timeout=20)
        ssh(HOLDER_SSH, f"mkdir -p {HOLDER_WORK}", 20)
        for name in ("wakeup_req.v1.json", "wakeup_ack.v1.json"):
            scp(str(golden / name), f"root@{HOLDER_SSH}:{HOLDER_WORK}/{name}")
        scp(str(sample), f"root@{HOLDER_SSH}:{HOLDER_WORK}/sample.bin")

        h_cfg = work / "holder.json"
        r_cfg = work / "requester.json"
        build_config(h_cfg, HOLDER_SELF, "0.0.0.0", "A", HOLDER_PORTS,
                     HOLDER_WORK, f"{HOLDER_WORK}/sample.bin", HOLDER_WORK)
        build_config(r_cfg, HOLDER_SELF, REQ_IP, "B", REQ_PORTS,
                     str(work).replace("\\", "/"), str(sample).replace("\\", "/"),
                     str(golden).replace("\\", "/"))
        scp(str(h_cfg), f"root@{HOLDER_SSH}:{HOLDER_WORK}/holder.json")

        # 222 持有者
        ssh(HOLDER_SSH, "pkill -x coldrevived || true; sleep 1", 20)
        start_h = (f"setsid nohup {HOLDER_BIN}/coldrevived --config {HOLDER_WORK}/holder.json "
                   f"--node-id A --bind 0.0.0.0 --self-host {HOLDER_SELF} "
                   f"--workdir {HOLDER_WORK} --sample {HOLDER_WORK}/sample.bin "
                   f"> {HOLDER_WORK}/holder.log 2>&1 < /dev/null & sleep 1; echo started")
        ssh(HOLDER_SSH, start_h, timeout=30)
        ctx.check("222 持有者 /health 200", wait_health(True, HOLDER_SSH, 19900))

        h_probe = ssh(HOLDER_SSH,
                      f"env COLDREVIVE_T0_CONFIG={HOLDER_WORK}/holder.json "
                      f"{HOLDER_BIN}/coldrevive-cli --json --node A nat probe listener 2500",
                      timeout=90)[1]
        try:
            probe = json.loads(h_probe.strip().splitlines()[-1])
        except Exception:
            probe = {}
        ctx.obs["holder_probe"] = probe
        nat = probe.get("nat_type")
        mapped = probe.get("mapped_address") or ""
        ctx.check("持有者真实 NAT 分类为 symmetric", nat == "symmetric", f"nat_type={nat}")
        ctx.check("持有者映射来自公网出口 112.5.32.34", mapped.startswith(EGRESS_HOLDER),
                  f"mapped_address={mapped}")
        if nat != "symmetric":
            status = "blocked"
            return finish(ctx, args, started, status, work, 2)

        time.sleep(1.5)  # relay_sub 500 ms 重声明周期
        ok, adv_raw = ssh_ok(HOLDER_SSH, f"curl -s -m 5 http://127.0.0.1:19900/nat/advertise",
                             lambda c, o: '"node_id"' in o)
        if not ok:
            ctx.check("取到持有者通告", False, adv_raw.strip()[:100])
            status = "blocked"
            return finish(ctx, args, started, status, work, 2)
        adv_path = work / "holder-advertise.json"
        adv_path.write_text(adv_raw, encoding="utf-8")
        ctx.obs["advertise_endpoints"] = [e.get("ip") for e in
                                          json.loads(adv_raw).get("endpoints", [])]

        # 本机 A 请求者
        log = open(work / "requester.log", "w", encoding="utf-8")
        procs.append(subprocess.Popen(
            [str(local_daemon), "--config", str(r_cfg), "--node-id", "B",
             "--bind", REQ_IP, "--self-host", REQ_IP,
             "--workdir", str(work), "--sample", str(sample)],
            stdout=log, stderr=subprocess.STDOUT))
        ctx.check("本机请求者 /health 200", wait_health(False, REQ_IP, 17800))
        peer = local_cli(r_cfg, "B", ["nat", "peer", str(adv_path)])
        plan = local_cli(r_cfg, "B", ["nat", "plan"])
        cands = [(c.get("kind"), c.get("ip")) for c in (plan.get("candidates") or [])]
        ctx.obs["plan_candidates"] = cands
        ctx.check("通告已装入请求者", bool(peer.get("ok")), f"endpoints={peer.get('endpoints')}")
        ctx.check("计划含 relay 与 stun-mapped 候选",
                  any(k == "relay" for k, _ in cands)
                  and any(k == "stun-mapped" and ip == EGRESS_HOLDER for k, ip in cands),
                  str(cands))
        if not (peer.get("ok") and any(k == "relay" for k, _ in cands)):
            status = "blocked"
            return finish(ctx, args, started, status, work, 2)

        # 声明传播是异步的（订阅线程 500 ms 轮询）。先在**不发任何唤醒包**的
        # `nat reach` 上等到 relay-only，再开抓包窗口，避免把「还没传播到」误判成
        # 「查询失败 → 直连兜底」并污染主窗口。
        reach_ready = {}
        for _ in range(12):
            reach_ready = local_cli(r_cfg, "B", ["nat", "reach"])
            if reach_ready.get("declared") and reach_ready.get("class") == "relay-only":
                break
            time.sleep(0.6)
        ctx.obs["reach_ready"] = reach_ready
        ctx.check("中继声明已就绪（nat reach，不发包）",
                  bool(reach_ready.get("declared"))
                  and reach_ready.get("class") == "relay-only",
                  f"class={reach_ready.get('class')} declared={reach_ready.get('declared')}")

        # 主测量：不带 only_kind，走新的「先查再打洞」决策。
        capture_start("main")
        wake = local_cli(r_cfg, "B", ["nat", "wake", file_hash, INFOHASH],
                         timeout=args.timeout)
        main_lines = capture_read(ctx, "main_capture_lines", "main")
        ctx.obs["main_wake"] = wake
        reach = wake.get("reach_lookup") or {}
        attempts = wake.get("attempts") or []
        ctx.check("reach_lookup 有值且中继声明存在",
                  bool(reach) and bool(reach.get("declared")),
                  f"reach_lookup={reach}")
        ctx.check("对端被判 relay-only 且跳过直连",
                  reach.get("class") == "relay-only" and bool(reach.get("skip_direct")),
                  f"class={reach.get('class')} skip_direct={reach.get('skip_direct')}")
        ctx.check("wake 成功且只走 relay",
                  bool(wake.get("ok")) and wake.get("path") == "relay"
                  and all(a.get("kind") == "relay" for a in attempts),
                  f"ok={wake.get('ok')} path={wake.get('path')} "
                  f"kinds={[a.get('kind') for a in attempts]}")
        ctx.check("主窗口抓包：发往 A 公网出口的 UDP 直接包为 0",
                  main_lines == 0, f"packets={main_lines}")
        if wake.get("ack_status") != "OK":
            ctx.check("wake 拿到 WAKEUP_ACK", False, f"ack_status={wake.get('ack_status')}")

        # 量具对照：强制直连必须能被同一个 tcpdump 抓到，否则上面的 0 不可信。
        # 请求侧自映射 STUN 偶尔拿不到（notes 会写「退化为单向打洞」），此时一包都不会发；
        # 那是量具没起步，不是「抓包抓不到」。重试并保留每次结果。
        control = {}
        control_lines = 0
        control_attempts: list[dict] = []
        for attempt in range(1, 4):
            which = f"control-{attempt}"
            capture_start(which)
            control = local_cli(r_cfg, "B",
                                ["nat", "wake", file_hash, INFOHASH, "stun-mapped"],
                                timeout=args.timeout)
            lines = capture_read(ctx, f"control_capture_{attempt}", which)
            control_attempts.append({
                "attempt": attempt,
                "packets": lines,
                "notes": control.get("notes") or [],
                "attempts": control.get("attempts") or [],
            })
            if lines > 0:
                control_lines = lines
                break
            time.sleep(5)
        ctx.obs["control_wake"] = control
        ctx.obs["control_attempts"] = control_attempts
        ctx.obs["control_note"] = [n for n in (control.get("notes") or [])
                                   if "强制候选路径" in n]
        ctx.check("量具对照：强制直连时抓到真实 UDP 包",
                  control_lines > 0,
                  f"packets={control_lines} tries={len(control_attempts)} "
                  f"control_ok={control.get('ok')} bypass={ctx.obs['control_note']}")

        status = "pass" if all(c["ok"] for c in ctx.checks) else "fail"
        return finish(ctx, args, started, status, work, 0 if status == "pass" else 1)
    finally:
        cleanup(args, procs)


def finish(ctx: Ctx, args, started, status: str, work: Path, code: int) -> int:
    REPORTS.mkdir(parents=True, exist_ok=True)
    out = REPORTS / f"run-{started.strftime('%Y%m%dT%H%M%SZ')}.json"
    passed = sum(1 for c in ctx.checks if c["ok"])
    report = {
        "schema": "coldrevive.w52.reach-gated-wake/v1",
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "status": status,
        "summary": f"{passed}/{len(ctx.checks)}",
        "checks": ctx.checks,
        "observations": ctx.obs,
        "g3_verdict": "not_run",
        "s6_status": "not_run",
        "evidence_valid_for_pass": False,
        "is_s6_evidence": False,
        "note": ("单目标、真实 NAT：中继声明驱动的唤醒路径与强制直连量具对照。"
                 "不写 G2/G3/S6 结论，不含速率数字。"),
    }
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"\n结果: {passed}/{len(ctx.checks)}  status={status}")
    print(f"报告: {out}")
    if not args.keep:
        shutil.rmtree(work, ignore_errors=True)
    return code


if __name__ == "__main__":
    sys.exit(main())
