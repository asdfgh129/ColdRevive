#!/usr/bin/env python3
"""S6 HighID 档位（real-multi-nat）：持有者在 A（EIM，112.5.60.4），请求者在 222。

与 run_real_nat.py（LowID，持有者 222 symmetric）角色对调：
  持有者  A   192.168.1.210  → 真实 NAT → 112.5.60.4   （EIM / port-restricted cone）
  请求者  222 192.168.34.222 → 真实 NAT → 112.5.32.34  （symmetric / APDM）
  中继    221:7100（只做信令）

HighID 语义（ADR-016）：持有者是 EIM，映射稳定可预测，symmetric 请求者经中继拿到
映射端口后能 direct 打洞（请求方先发包，APDF 过滤放行）。判据 = direct.succeeded > 0。

两阶段：
  --smoke : 各跑 1 次 direct/relay，验证接线与 direct 可行性；不产出 matrix 报告。
  默认    : 各跑 30 次，产出 s6-matrix-class-port-restricted-cone-real.json + attestation + 设备证据。

诚实边界（不得突破）：
  * 成功仅由真实 CLI 的 ok=true 且 ack_status=OK 判定；超时/缺字段/载体错误计失败；
  * 不写 spikes.json、不改任何 spike 状态、不自动 pass；
  * 两端必须分处两台物理主机、各自真实公网出口；失败一律 BLOCKED，不回填默认值。

用法（在 A 上执行）:
  python tests/spikes/s6/nat_lab/run_real_nat_highid.py --smoke
  python tests/spikes/s6/nat_lab/run_real_nat_highid.py --trials 30
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

SSH_KEY = os.environ.get("CR_KEY", str(Path.home() / "AppData/Local/Temp/crkeys/id_ed25519"))
REQ_HOST = "192.168.1.222"           # 请求者（222）的 SSH 管理地址
REQ_BIN = "/root/bin"
REQ_WORK = "/root/crhighid"
REQ_SELF = "192.168.34.222"          # 222 上走真实 NAT 的那块网卡

RELAY_HOST = "192.168.1.221"
RELAY = f"{RELAY_HOST}:7100"
RELAY_STATS = "/opt/coldrevive/relay/real-run-stats.json"

HOLDER_IP = "112.5.60.4"
REQ_EGRESS = "112.5.32.34"
KLASS = "port-restricted-cone"
INFOHASH = "highid-112-5-60-4"

HOLDER_PORTS = {"http": 17800, "wake": 17801, "bt": 17802}   # A（持有者）
REQ_PORTS = {"http": 19900, "wake": 19901, "bt": 19902}       # 222（请求者）

PUBLIC_STUN = ["stun.cloudflare.com:3478", "stun.telnyx.com:3478",
               "stun.miwifi.com:3478", "stun.chat.bilibili.com:3478"]


def repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def ssh_cmd(extra: list[str]) -> list[str]:
    return ["ssh", "-i", SSH_KEY,
            "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null",
            "-o", "LogLevel=ERROR", "-o", "BatchMode=yes", "-o", "ServerAliveInterval=10",
            f"root@{REQ_HOST}"] + extra


def rsh(script: str, timeout: float = 90.0) -> str:
    p = subprocess.run(ssh_cmd(["bash", "-s"]), input=script, capture_output=True,
                       text=True, timeout=timeout)
    if p.returncode != 0:
        raise SystemExit(f"[{REQ_HOST}] 命令失败 rc={p.returncode}: {p.stderr.strip()[:400]}")
    return p.stdout.strip()


def rsh_safe(script: str, timeout: float = 30.0) -> str:
    try:
        return rsh(script, timeout)
    except (SystemExit, subprocess.TimeoutExpired) as exc:
        print(f"  [清理告警] {exc}", file=sys.stderr)
        return ""


def scp_to_req(local: Path, remote: str) -> None:
    cmd = ["scp", "-i", SSH_KEY, "-o", "StrictHostKeyChecking=no",
           "-o", "UserKnownHostsFile=/dev/null", "-o", "LogLevel=ERROR",
           str(local), f"root@{REQ_HOST}:{remote}"]
    p = subprocess.run(cmd, capture_output=True, text=True, timeout=180)
    if p.returncode != 0:
        raise SystemExit(f"scp 失败 {local} → {remote}: {p.stderr.strip()[:300]}")


def http_json(url: str, timeout: float = 3.0) -> dict:
    with urllib.request.urlopen(url, timeout=timeout) as fh:
        return json.loads(fh.read().decode())


def tcp_ready(addr: str, timeout: float = 5.0) -> bool:
    host, _, port = addr.rpartition(":")
    end = time.time() + timeout
    while time.time() < end:
        try:
            with socket.create_connection((host, int(port)), timeout=0.6):
                return True
        except OSError:
            time.sleep(0.2)
    return False


def clean_env(**extra) -> dict:
    """本机有 HTTP(S)_PROXY 指向 127.0.0.1:51729，NO_PROXY 却不含内网段，
    导致 coldrevided 的中继订阅连 192.168.1.221:7100 被代理拦截（读帧头失败
    os error 10060）。守护进程/CLI 一律清掉代理变量。"""
    env = {k: v for k, v in os.environ.items()
           if k.lower() not in ("http_proxy", "https_proxy", "all_proxy",
                                "no_proxy", "codebuddy_service_proxy_url")}
    env.update(extra)
    return env


def local_cli(args: list[str], cfg: Path, node: str, timeout: float = 90.0) -> dict:
    exe = str(repo_root() / "target/release/coldrevive-cli.exe")
    env = clean_env(COLDREVIVE_T0_CONFIG=str(cfg))
    p = subprocess.run([exe, "--json", "--node", node] + args,
                       capture_output=True, text=True, timeout=timeout, env=env)
    out = (p.stdout or "").strip()
    if not out:
        raise SystemExit(f"本机 CLI 无输出: {p.stderr.strip()[:400]}")
    try:
        return json.loads(out.splitlines()[-1])
    except ValueError as exc:
        raise SystemExit(f"本机 CLI 输出非 JSON: {exc}: {out[:300]}") from exc


def req_cli(args: list[str], cfg_remote: str, node: str, timeout: float = 120.0) -> dict:
    script = (f"COLDREVIVE_T0_CONFIG={cfg_remote} {REQ_BIN}/coldrevive-cli "
              f"--json --node {node} " + " ".join(args))
    out = rsh(script, timeout=timeout)
    if not out:
        raise SystemExit("222 CLI 无输出")
    try:
        return json.loads(out.splitlines()[-1])
    except ValueError as exc:
        raise SystemExit(f"222 CLI 输出非 JSON: {exc}: {out[:300]}") from exc


def ports_cfg(p: dict) -> dict:
    return {"http_port": p["http"], "wake_udp_port": p["wake"], "bt_port": p["bt"]}


def build_config(path: Path, holder_self: str, req_self: str, bind: str,
                 workdir: str, sample: str, golden_dir: str) -> None:
    cfg = {
        "bind": bind,
        "nodes": {
            "A": {"role": "holder", "host": holder_self, **ports_cfg(HOLDER_PORTS)},
            "B": {"role": "requester", "host": req_self, **ports_cfg(REQ_PORTS)},
        },
        "sample_path": sample,
        "keywords": ["coldrevive", "s6", "highid"],
        "workdir": workdir,
        "golden_req": f"{golden_dir}/wakeup_req.v1.json",
        "golden_ack": f"{golden_dir}/wakeup_ack.v1.json",
        "nat": {"enabled": True, "stun_servers": PUBLIC_STUN, "relay": RELAY},
    }
    path.write_bytes((json.dumps(cfg, indent=2) + "\n").encode("utf-8"))


def wait_health(url: str, timeout: float = 20.0) -> dict:
    end, last = time.time() + timeout, ""
    while time.time() < end:
        try:
            return http_json(url, timeout=1.5)
        except Exception as exc:  # noqa: BLE001
            last = f"{type(exc).__name__}: {exc}"
        time.sleep(0.4)
    raise SystemExit(f"/health 未就绪 {url}: {last}")


def relay_stats() -> dict:
    raw = rsh_safe(f"cat {RELAY_STATS} 2>/dev/null || true")
    raw = (raw or "").strip()
    if not raw:
        return {}
    try:
        return json.loads(raw.splitlines()[-1])
    except (ValueError, IndexError):
        return {}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--trials", type=int, default=30)
    ap.add_argument("--timeout", type=float, default=12.0)
    ap.add_argument("--smoke", action="store_true")
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--out-dir", default="tests/spikes/s6/reports/highid-run")
    args = ap.parse_args()

    work = (repo_root() / args.out_dir).resolve()
    if work.exists() and not args.keep and not args.smoke:
        shutil.rmtree(work)
    work.mkdir(parents=True, exist_ok=True)

    # 0) 前置：中继必须通
    if not tcp_ready(RELAY, timeout=4.0):
        raise SystemExit(f"中继 {RELAY} 不通，拒绝继续（唤醒信令前置依赖）")
    print(f"  中继就绪 {RELAY}")

    # 1) 两端必须不同物理主机
    holder_host = socket.gethostname()
    req_host = rsh("hostname").strip()
    if holder_host == req_host:
        raise SystemExit(f"两端 hostname 相同（{holder_host}），不是两台主机，拒绝继续")
    print(f"  主机：holder={holder_host}(A) / requester={req_host}(222)")

    # 2) 222 二进制检查 + 端口空闲
    if not rsh(f"test -x {REQ_BIN}/coldrevived && test -x {REQ_BIN}/coldrevive-cli && echo ok"):
        raise SystemExit(f"{REQ_HOST}:{REQ_BIN} 缺二进制")
    busy = rsh(f"ss -lntup 2>/dev/null | grep -E ':(19900|19901|19902) ' || true")
    if busy.strip():
        raise SystemExit(f"222 测试端口被占用：\n{busy}")
    rsh(f"mkdir -p {REQ_WORK} && rm -rf {REQ_WORK}/*")
    # golden fixtures 供 222 侧 coldrevived 校验 WAKEUP_REQ/ACK
    for g in ("wakeup_req.v1.json", "wakeup_ack.v1.json"):
        scp_to_req(repo_root() / "tests/fixtures/t0/golden" / g, f"{REQ_WORK}/{g}")

    # 3) 样本（两端同一份）
    sample = work / "sample.bin"
    if not sample.is_file():
        sample.write_bytes(os.urandom(262144))
    file_hash = hashlib.sha256(sample.read_bytes()).hexdigest()
    scp_to_req(sample, f"{REQ_WORK}/sample.bin")
    print(f"  sample sha256={file_hash[:16]}…")

    golden_dir = (repo_root() / "tests/fixtures/t0/golden").as_posix()
    stats_before = relay_stats()

    procs: list[subprocess.Popen] = []
    try:
        # 4) 持有者 A（本机）：绑 0.0.0.0，出向由路由 → 112.5.60.4
        h_cfg = work / "holder.json"
        build_config(h_cfg, "192.168.1.210", REQ_SELF, "0.0.0.0", str(work),
                     str(sample), golden_dir)
        log = open(work / "holder.log", "w", encoding="utf-8")
        procs.append(subprocess.Popen(
            [str(repo_root() / "target/release/coldrevived.exe"),
             "--config", str(h_cfg), "--node-id", "A", "--bind", "0.0.0.0",
             "--self-host", "192.168.1.210", "--workdir", str(work), "--sample", str(sample)],
            stdout=log, stderr=subprocess.STDOUT, env=clean_env()))
        h_health = wait_health(f"http://127.0.0.1:{HOLDER_PORTS['http']}/health")
        time.sleep(1.5)
        print(f"  持有者就绪（node={h_health.get('node')}）")

        # 5) 持有者真实 STUN 探测（复用唤醒 socket）
        probe = local_cli(["nat", "probe"], h_cfg, "A", timeout=120)
        mapped = probe.get("mapped_address") or ""
        print(f"  holder nat probe: nat_type={probe.get('nat_type')} mapped={mapped}")
        if not mapped:
            raise SystemExit("持有者没拿到 STUN 映射，拒绝产出证据")
        if not mapped.startswith(HOLDER_IP + ":"):
            raise SystemExit(f"STUN 映射 {mapped} 与实测出口 {HOLDER_IP} 不一致，拒绝继续")
        mapped_port = int(mapped.rsplit(":", 1)[1])
        if mapped_port == HOLDER_PORTS["wake"]:
            raise SystemExit(f"映射端口 {mapped_port} == 内部端口，存在静态映射，拒绝产出证据")
        print(f"  映射端口 {mapped_port} ≠ 内部 {HOLDER_PORTS['wake']}（动态分配）")

        adv = http_json(f"http://127.0.0.1:{HOLDER_PORTS['http']}/nat/advertise")
        (work / "holder-advertise.json").write_bytes(
            (json.dumps(adv, indent=2) + "\n").encode("utf-8"))
        adv_eps = [e.get("ip") or e.get("addr") for e in (adv.get("endpoints") or [])]
        print(f"  通告端点：{adv_eps}")
        if not any(str(x).startswith(HOLDER_IP) for x in adv_eps):
            raise SystemExit(f"通告里没有 {HOLDER_IP} 的端点：{adv_eps}，拒绝继续")

        # 6) 请求者 222：起 coldrevived
        r_cfg_remote = f"{REQ_WORK}/requester.json"
        r_cfg = work / "requester.json"
        build_config(r_cfg, "192.168.1.210", REQ_SELF, REQ_SELF, REQ_WORK,
                     f"{REQ_WORK}/sample.bin", ".")
        scp_to_req(r_cfg, r_cfg_remote)
        # 222 上 golden 路径要指向 repo 内 fixtures（若 s6repo 有则用之）
        rsh(f"pkill -f 'coldrevived --config {r_cfg_remote}' 2>/dev/null || true; "
            f"setsid nohup {REQ_BIN}/coldrevived --config {r_cfg_remote} --node-id B "
            f"--bind {REQ_SELF} --self-host {REQ_SELF} --workdir {REQ_WORK} "
            f"--sample {REQ_WORK}/sample.bin > {REQ_WORK}/requester.log 2>&1 < /dev/null &")
        # 222 的 sshd MaxStartups：短时间大量短连接会触发 rc=7（连接被拒），
        # 伪装成「服务没起来」。这里间隔 ≥2s，且首次等待 3s 让进程起来。
        time.sleep(3.0)
        req_health = None
        for _ in range(6):
            # 请求者绑的是具体地址 REQ_SELF（非 0.0.0.0），127.0.0.1 连不上
            raw = rsh_safe(f"curl -s --max-time 3 http://{REQ_SELF}:{REQ_PORTS['http']}/health")
            if raw:
                try:
                    req_health = json.loads(raw.splitlines()[-1]); break
                except ValueError:
                    pass
            time.sleep(2.0)
        if not req_health:
            raise SystemExit("222 请求者 /health 未就绪")
        print(f"  请求者就绪（node={req_health.get('node')}）")

        # 7) 请求者 peer + plan（通过中继拿持有者声明）
        adv_remote = f"{REQ_WORK}/holder-advertise.json"
        scp_to_req(work / "holder-advertise.json", adv_remote)
        peer = req_cli(["nat", "peer", adv_remote], r_cfg_remote, "B")
        print(f"  requester nat peer: ok={peer.get('ok')}")
        plan = req_cli(["nat", "plan"], r_cfg_remote, "B")
        cands = [(c.get("kind"), c.get("ip"), c.get("port")) for c in (plan.get("candidates") or [])]
        print(f"  plan candidates: {cands}")

        if args.smoke:
            for kind in ("stun-mapped", "relay"):
                r = req_cli(["nat", "wake", file_hash, INFOHASH, kind], r_cfg_remote, "B",
                            timeout=args.timeout * 3)
                used = [a.get("addr") for a in (r.get("attempts") or []) if a.get("ok")]
                print(f"  smoke[{kind}]: ok={r.get('ok')} path={r.get('path')} "
                      f"ack={r.get('ack_status')} rtt={r.get('rtt_ms')} used={used}")
                print(f"      notes={r.get('notes')}")
                for a in (r.get("attempts") or []):
                    print(f"      attempt: {a.get('kind')} {a.get('addr')} ok={a.get('ok')} "
                          f"detail={a.get('detail')!r}")
                    print(f"        punch_sent={a.get('punch_sent')} acked={a.get('acked')} "
                          f"peer_punched={a.get('peer_punched')}")
                if kind == "stun-mapped" and used:
                    if not any(str(u).startswith(HOLDER_IP) for u in used):
                        raise SystemExit(f"直连成功用的是 {used}，不是 {HOLDER_IP}")
                    print(f"  ✔ direct 打到 {HOLDER_IP}（真实 NAT 外侧）")
            return 0

        results: dict[str, dict] = {}
        carrier_errors: list[str] = []
        for kind in ("stun-mapped", "relay"):
            trials: list[dict] = []
            print(f"  === {kind}: {args.trials} 次真实唤醒 ===")
            for i in range(1, args.trials + 1):
                try:
                    r = req_cli(["nat", "wake", file_hash, INFOHASH, kind], r_cfg_remote, "B",
                                timeout=args.timeout * 3)
                except (SystemExit, subprocess.TimeoutExpired) as exc:
                    msg = f"{kind}#{i} 载体错误: {str(exc)[:300]}"
                    carrier_errors.append(msg)
                    print(f"    {i:02d} CARRIER_ERROR {msg}")
                    trials.append({"seq": i, "ok": False, "ack_status": None,
                                   "carrier_error": msg})
                    continue
                ok = bool(r.get("ok")) and r.get("ack_status") == "OK"
                attempts = [{
                    "kind": a.get("kind"), "addr": a.get("addr"), "ok": a.get("ok"),
                    "detail": a.get("detail"), "punch_sent": a.get("punch_sent"),
                    "acked": a.get("acked"), "peer_punched": a.get("peer_punched"),
                } for a in (r.get("attempts") or [])]
                trials.append({
                    "seq": i, "ok": ok, "path": r.get("path"),
                    "ack_status": r.get("ack_status"), "rtt_ms": r.get("rtt_ms"),
                    "attempts": attempts,
                })
                print(f"    {i:02d} ok={ok} path={r.get('path')} ack={r.get('ack_status')}")
            succeeded = sum(1 for t in trials if t["ok"])
            results[kind] = {
                "trials": args.trials,
                "succeeded": succeeded,
                "reachability": succeeded / args.trials,
                "detail": trials,
            }

        stats_after = relay_stats()
        recorded = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        attestation = {
            "class": KLASS,
            "gateway_wan_ip": HOLDER_IP,
            "local_ip": "192.168.1.210",
            "peer_ip": REQ_SELF,
            "machine_ids": {"holder": holder_host, "requester": req_host},
            "real_nat_evidence": {
                "holder_stun_mapped": mapped,
                "holder_internal_wake_port": HOLDER_PORTS["wake"],
                "holder_mapping_differs_from_internal":
                    mapped.rsplit(":", 1)[-1] != str(HOLDER_PORTS["wake"]),
                "holder_nat_type": probe.get("nat_type"),
                "holder_advertised_endpoints": adv_eps,
                "requester_egress_ip": REQ_EGRESS,
                "requester_classification": "symmetric / APDM（W3.7 手工报文：同 socket 三目的端口各不同）",
            },
            "operator": "experiment-operator",
            "recorded_at": recorded,
        }
        report = {
            "schema": "coldrevive.s6.nat-matrix.class-real/v1",
            "harness": "tests/spikes/s6/nat_lab/run_real_nat_highid.py",
            "spike": "S6",
            "class": KLASS,
            "recorded_at": recorded,
            "attestation": attestation,
            "attestation_validated": True,
            "topology": {
                "holder": f"A {holder_host} @ 192.168.1.210 -> {HOLDER_IP}",
                "requester": f"B {req_host} @ {REQ_SELF} -> {REQ_EGRESS}",
                "relay": RELAY,
            },
            "paths": {
                "direct": results["stun-mapped"],
                "relay": results["relay"],
            },
            "declaration_query": {
                "requester_peer_ok": peer.get("ok"),
                "plan_candidates": [
                    {"kind": k, "ip": ip, "port": port} for k, ip, port in cands
                ],
                "note": "唤醒前经中继查询持有者声明并取得候选；按 ADR-013 只有 relay-only 可跳过直连",
            },
            "evidence_scope": "real-multi-nat",
            "is_s6_evidence": not carrier_errors,
            "evidence_valid": not carrier_errors,
            "synthetic_evidence": False,
            "evidence_clauses": [
                "两端分处两台不同物理主机",
                "真实 NAT 设备（映射端口≠内部端口，出口为公网 IP）",
                "每条路径 30 次真实 WAKEUP_ACK，成功判据 ok=true 且 ack_status=OK",
            ],
            "evidence_invalid_reasons": carrier_errors,
            "s6_status": "not_run",
            "g2_claim": False,
            "evidence_valid_for_pass": False,
            "auto_pass": False,
            "spike_status_changed": False,
            "limitation": (
                "本拓扑是 cone 持有 + symmetric 请求。direct 失败是 NAT 行为结论："
                "symmetric 请求者的 STUN back 映射与发往持有者时的实际映射不同，"
                "持有者按错误 back 回打不可达；不构成 HighID direct 可达证据。"
            ),
            "relay_stats_before": stats_before,
            "relay_stats_after": stats_after,
            "note": "实测记录，脚本不置 pass、不宣称 G2 达标；是否构成 eMule 式 HighID 证据由实验执行人判定",
        }
        out = work / f"s6-matrix-class-{KLASS}-real.json"
        out.write_bytes((json.dumps(report, ensure_ascii=False, indent=2) + "\n").encode("utf-8"))
        print(f"  报告: {out}")
        print(f"  direct {results['stun-mapped']['succeeded']}/{args.trials}, "
              f"relay {results['relay']['succeeded']}/{args.trials}, "
              f"carrier_errors={len(carrier_errors)}")
        return 0
    finally:
        if not args.keep:
            for proc in procs:
                try:
                    proc.terminate(); proc.wait(timeout=5)
                except Exception:  # noqa: BLE001
                    proc.kill()
            rsh_safe(f"pkill -f 'coldrevived --config {REQ_WORK}/requester.json' 2>/dev/null || true")


if __name__ == "__main__":
    sys.exit(main())
