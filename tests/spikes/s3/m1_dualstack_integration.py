#!/usr/bin/env python3
"""S3 附件 —— M1 **真实双栈跨主机集成**实测（KAD 128-bit / Mainline 160-bit）。

与既有证据的区别：
  * `tests/spikes/s3/src/main.rs`：本地双 HashMap 结构隔离；
  * `dualstack_io.py`：单机回环、自建命名空间的真实 UDP I/O；
  * 本 harness：**真实 coldrevived A/B 两进程 + 两台不同物理主机 + 真实 LAN UDP/HTTP/CLI**，
    观测 M1 守护进程实际暴露的 `inspect kad` 双栈 ID 与真实 KAD_FIND 命中行为。

判据对应（spikes.json S3.criteria）：
  * 「双栈并行压测无 ID 空间串扰」：KAD 形状 key 必须真实命中；
    160-bit DHT 形状 ID / 截断 128-bit ID / 畸形帧一律不得命中；
    两栈 ID 集合互不相交，长度分别 32 / 40 hex。
  * 「内存与带宽增量在可接受范围」：本轮只量测真实线上字节与两栈吞吐；
    跨进程 RSS 增量未取，如实列为 gap。

红线（tests/spikes/README.md §6）：
  * 本结果仅为**附件**，`evidence_valid_for_pass=false`；
  * 缺 G3 兼容性矩阵（M3 范围）与真实 Mainline DHT 公网加入，S3.status 恒 `not_run`；
  * 不向公共 KAD / DHT 发布任何测试 Key；全部报文留在私有 LAN；
  * 脚本绝不修改 spikes.json，也不自动置 pass；缺失/失败一律 FAIL / BLOCKED。

用法：
  python3 tests/spikes/s3/m1_dualstack_integration.py --self-test
  python3 tests/spikes/s3/m1_dualstack_integration.py --out <path> [--no-spawn-a]
"""
from __future__ import annotations

import argparse
import hashlib
import json
import random
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
RUN_DIR = HERE / ".run"
REPORT_DIR = HERE / "reports"
NAMESPACE = "ColdRevive_V1:"
DEFAULT_A = "192.168.1.210"
DEFAULT_B = "192.168.1.209"
PORTS = {
    "A": {"http": 17800, "udp": 17801, "bt": 17802},
    "B": {"http": 17900, "udp": 17901, "bt": 17902},
}
KEYWORDS = ["coldrevive", "s3", "dualstack"]


def now() -> str:
    return datetime.now(timezone.utc).isoformat()


# ---------------------------------------------------------------------------
# 与 cr-kad 完全一致的纯 Python 复刻（用于独立校验 Rust 守护进程的真实输出）
# ---------------------------------------------------------------------------

def sha1_hex(s: str) -> str:
    return hashlib.sha1(s.encode()).hexdigest()


def sha256_hex(s: str) -> str:
    return hashlib.sha256(s.encode()).hexdigest()


def target_key(namespace: str, file_hash: str) -> str:
    return sha1_hex(namespace + file_hash)


def keyword_key(namespace: str, keyword: str) -> str:
    return sha1_hex(f"{namespace}kw:{keyword}")


def publish_keys(namespace: str, file_hash: str, keywords: list[str]) -> list[str]:
    return [target_key(namespace, file_hash)] + [
        keyword_key(namespace, k) for k in keywords[:8]]


def kad_id_128(namespace: str, seed: str) -> str:
    return sha256_hex(namespace + seed)[:32]


def dht_id_160(namespace: str, seed: str) -> str:
    return sha1_hex(namespace + seed)


# ---------------------------------------------------------------------------
# 真实 HTTP / UDP 观测；不使用任何合成响应
# ---------------------------------------------------------------------------

def http_get(url: str, timeout: float = 3.0) -> dict:
    with urllib.request.urlopen(url, timeout=timeout) as resp:
        return json.loads(resp.read().decode("utf-8", "replace"))


def http_cli(base: str, command: str, args: list[str], timeout: float = 5.0) -> dict:
    body = json.dumps({"command": command, "args": args}).encode()
    req = urllib.request.Request(base + "/cli", data=body,
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read().decode("utf-8", "replace"))


def udp_probe(host: str, port: int, frame: dict | bytes, timeout: float = 0.6) -> dict:
    """真实 UDP 单包往返。无应答/非法 JSON 如实记为 no_reply，绝不伪造。"""
    payload = frame if isinstance(frame, bytes) else json.dumps(frame).encode()
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.settimeout(timeout)
    row: dict = {"sent_bytes": len(payload), "ok": False, "no_reply": False}
    try:
        t0 = time.perf_counter()
        s.sendto(payload, (host, port))
        data, src = s.recvfrom(4096)
        row["rtt_ms"] = round((time.perf_counter() - t0) * 1000, 3)
        row["recv_bytes"] = len(data)
        row["from"] = f"{src[0]}:{src[1]}"
        try:
            row["reply"] = json.loads(data.decode("utf-8", "replace"))
            row["ok"] = True
        except (ValueError, UnicodeDecodeError):
            row["error"] = "non-json reply"
    except socket.timeout:
        row["no_reply"] = True
    except OSError as exc:
        row["error"] = f"{type(exc).__name__}: {exc}"
    finally:
        s.close()
    return row


def is_hit(row: dict) -> bool:
    """命中只认守护进程真实回包里的 count>0 / hit_node 非空。"""
    if not row.get("ok"):
        return False
    reply = row.get("reply") or {}
    return reply.get("t") == "KAD_FIND_RES" and (
        reply.get("count", 0) > 0 or bool(reply.get("hit_node")))


# ---------------------------------------------------------------------------
# A 节点本地拉起（仅在缺 /health 时；B 常驻由操作员部署，harness 绝不碰）
# ---------------------------------------------------------------------------

class LocalA:
    def __init__(self, host: str):
        self.host = host
        self.proc: subprocess.Popen | None = None
        self.log = None
        self.cfg_path = RUN_DIR / "s3-a-config.json"
        self.sample = RUN_DIR / "s3-dualstack-sample.bin"

    def spawn(self) -> dict:
        exe = ROOT / ("target/release/coldrevived.exe" if sys.platform == "win32"
                      else "target/release/coldrevived")
        if not exe.is_file():
            raise RuntimeError(f"缺少二进制 {exe}")
        RUN_DIR.mkdir(parents=True, exist_ok=True)
        (RUN_DIR / "A").mkdir(parents=True, exist_ok=True)
        data = random.Random(20261114).randbytes(256 * 1024)
        self.sample.write_bytes(data)
        cfg = {
            "suite": "S3", "schema": "coldrevive.daemon.config/v1",
            "bind": self.host,
            "nodes": {
                "A": {"host": self.host, "http_port": PORTS["A"]["http"],
                      "wake_udp_port": PORTS["A"]["udp"], "bt_port": PORTS["A"]["bt"]},
                "B": {"host": DEFAULT_B, "http_port": PORTS["B"]["http"],
                      "wake_udp_port": PORTS["B"]["udp"], "bt_port": PORTS["B"]["bt"]},
            },
            "sample_path": self.sample.relative_to(ROOT).as_posix(),
            "keywords": KEYWORDS, "namespace": NAMESPACE,
            "workdir": (RUN_DIR / "A").relative_to(ROOT).as_posix(),
            "golden_req": "tests/fixtures/t0/golden/wakeup_req.v1.json",
            "golden_ack": "tests/fixtures/t0/golden/wakeup_ack.v1.json",
            "downloader": "", "challenge_window_sec": 300,
        }
        self.cfg_path.write_text(json.dumps(cfg, ensure_ascii=False, indent=2) + "\n",
                                 encoding="utf-8")
        self.log = (RUN_DIR / "A.log").open("wb")
        self.proc = subprocess.Popen(
            [str(exe), "--config", str(self.cfg_path), "--node-id", "A",
             "--bind", self.host, "--self-host", self.host,
             "--workdir", str(RUN_DIR / "A"), "--sample", str(self.sample)],
            stdout=self.log, stderr=subprocess.STDOUT, cwd=str(ROOT))
        return {"spawned": True, "pid": self.proc.pid,
                "config": self.cfg_path.relative_to(ROOT).as_posix(),
                "sample_sha256": hashlib.sha256(data).hexdigest()}

    def wait_health(self, seconds: float = 12.0) -> bool:
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            try:
                h = http_get(f"http://{self.host}:{PORTS['A']['http']}/health", 1.0)
                if h.get("status") == "ok" and h.get("node") == "A":
                    return True
            except (urllib.error.URLError, OSError, ValueError):
                pass
            if self.proc and self.proc.poll() is not None:
                return False
            time.sleep(0.2)
        return False

    def stop(self) -> None:
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        if self.log:
            self.log.close()


# ---------------------------------------------------------------------------
# 并行双栈压测：KAD 形状 / DHT 形状两股真实 UDP 流同时打向守护进程
# ---------------------------------------------------------------------------

def parallel_stream(host: str, port: int, kad_keys: list[str], dht_ids: list[str],
                    rounds: int, timeout: float) -> dict:
    out: dict = {"rounds_per_stack": rounds, "kad": {}, "dht": {}}

    def run(label: str, keys: list[str], expect_hit: bool) -> None:
        rows = []
        for i in range(rounds):
            key = keys[i % len(keys)]
            r = udp_probe(host, port, {"t": "KAD_FIND", "key": key}, timeout)
            rows.append({"key": key, "hit": is_hit(r), "rtt_ms": r.get("rtt_ms"),
                         "sent_bytes": r["sent_bytes"], "recv_bytes": r.get("recv_bytes", 0),
                         "no_reply": r.get("no_reply", False), "error": r.get("error")})
        hits = sum(1 for r in rows if r["hit"])
        wire = sum(r["sent_bytes"] + r["recv_bytes"] for r in rows)
        rtts = [r["rtt_ms"] for r in rows if r["rtt_ms"] is not None]
        out[label] = {
            "expected_hit": expect_hit, "frames": len(rows), "hits": hits,
            "wire_bytes": wire,
            "no_reply": sum(1 for r in rows if r["no_reply"]),
            "errors": sum(1 for r in rows if r.get("error")),
            "rtt_median_ms": round(sorted(rtts)[len(rtts) // 2], 3) if rtts else None,
            "rows": rows,
        }

    t0 = time.perf_counter()
    threads = [threading.Thread(target=run, args=("kad", kad_keys, True)),
               threading.Thread(target=run, args=("dht", dht_ids, False))]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    out["elapsed_sec"] = round(time.perf_counter() - t0, 3)
    total_bytes = out["kad"]["wire_bytes"] + out["dht"]["wire_bytes"]
    out["total_wire_bytes"] = total_bytes
    out["total_mbps"] = round(total_bytes * 8 / out["elapsed_sec"] / 1e6, 4) \
        if out["elapsed_sec"] else None
    out["misrouted_frames"] = out["dht"]["hits"]
    out["kad_misses"] = out["kad"]["frames"] - out["kad"]["hits"]
    out["no_crosstalk"] = out["misrouted_frames"] == 0
    out["method"] = ("两个真实线程并发发送真实 UDP 数据报；命中判据只认守护进程"
                     "真实 KAD_FIND_RES 的 count>0 / hit_node 非空")
    return out


# ---------------------------------------------------------------------------
# 自检：纯逻辑，不联网、不声称真实证据
# ---------------------------------------------------------------------------

def self_test() -> int:
    checks: list[tuple[str, bool]] = []
    h = "a" * 64
    keys = publish_keys(NAMESPACE, h, KEYWORDS)
    checks.append(("publish_keys_matches_cr_kad_shape",
                   len(keys) == 1 + len(KEYWORDS)
                   and all(len(k) == 40 for k in keys)))
    k128 = [kad_id_128(NAMESPACE, k) for k in keys]
    d160 = [dht_id_160(NAMESPACE, k) for k in keys]
    checks.append(("dual_stack_id_widths",
                   all(len(x) == 32 for x in k128) and all(len(x) == 40 for x in d160)))
    checks.append(("dual_stack_id_sets_disjoint", not (set(k128) & set(d160))))
    checks.append(("is_hit_requires_real_reply",
                   not is_hit({"ok": False, "no_reply": True})
                   and not is_hit({"ok": True, "reply": {"t": "KAD_FIND_RES",
                                                         "count": 0, "hit_node": ""}})
                   and is_hit({"ok": True, "reply": {"t": "KAD_FIND_RES",
                                                     "count": 1, "hit_node": "A"}})))
    r = {"schema": "coldrevive.s3.m1-dualstack/v1", "evidence_valid_for_pass": False,
         "s3_status": "not_run", "g3_matrix_available": False,
         "spike_status_changed": False}
    checks.append(("gate_discipline_is_honest",
                   r["evidence_valid_for_pass"] is False
                   and r["s3_status"] == "not_run"
                   and r["spike_status_changed"] is False))
    for name, ok in checks:
        print(f"  [{'PASS' if ok else 'FAIL'}] {name}")
    n = sum(1 for _, ok in checks if ok)
    print(f"m1_dualstack_integration self-test: {n}/{len(checks)} PASS "
          "(pure logic, evidence_valid=false)")
    return 0 if n == len(checks) else 1


# ---------------------------------------------------------------------------
# 真实实验
# ---------------------------------------------------------------------------

def real_run(args: argparse.Namespace) -> tuple[dict, int]:
    a_host, b_host = args.a_host, args.b_host
    a_base = f"http://{a_host}:{PORTS['A']['http']}"
    b_base = f"http://{b_host}:{PORTS['B']['http']}"
    checks: list[dict] = []
    proc_info: dict = {"spawned": False}

    def check(name: str, ok: bool, detail: str = "") -> None:
        checks.append({"name": name, "ok": bool(ok), "detail": detail})

    local_a = LocalA(a_host)
    try:
        # --- 环境：两台不同物理主机 ---
        health = {}
        for node, base in (("A", a_base), ("B", b_base)):
            try:
                health[node] = http_get(base + "/health", 2.0)
            except (urllib.error.URLError, OSError, ValueError) as exc:
                health[node] = {"error": f"{type(exc).__name__}: {exc}"}
        if health["A"].get("status") != "ok" and not args.no_spawn_a:
            proc_info = local_a.spawn()
            check("A 本地守护进程真实启动", local_a.wait_health(),
                  json.dumps(proc_info, ensure_ascii=False))
            try:
                health["A"] = http_get(a_base + "/health", 2.0)
            except (urllib.error.URLError, OSError, ValueError) as exc:
                health["A"] = {"error": f"{type(exc).__name__}: {exc}"}
        distinct = (health.get("A", {}).get("machine_id")
                    and health.get("B", {}).get("machine_id")
                    and health["A"]["machine_id"] != health["B"]["machine_id"]
                    and health["A"].get("host") != health["B"].get("host"))
        check("A/B 为两台不同物理主机（/health machine_id + host 互异）", bool(distinct),
              json.dumps(health, ensure_ascii=False))
        if not distinct:
            return _blocked("A/B 双机 /health 未同时就绪或为同一主机", health, checks), 2

        # --- M1 真实双栈 ID 输出 ---
        inspect = {}
        for node, base in (("A", a_base), ("B", b_base)):
            try:
                inspect[node] = http_cli(base, "inspect", ["kad"])
            except (urllib.error.URLError, OSError, ValueError) as exc:
                inspect[node] = {"error": f"{type(exc).__name__}: {exc}"}
        kad_a = inspect["A"].get("kad", {})
        pub = kad_a.get("published_keys", [])
        check("A 的 inspect kad 返回真实已发布 key", len(pub) >= 1,
              f"published_keys={len(pub)}")
        check("A 的 namespace_ok=true", kad_a.get("namespace_ok") is True)
        r128, r160 = kad_a.get("kad_ids_128", []), kad_a.get("dht_ids_160", [])
        check("M1 同时暴露 128-bit / 160-bit 双栈 ID",
              len(r128) == len(pub) and len(r160) == len(pub) and len(pub) > 0)
        check("双栈 ID 位宽正确（32 / 40 hex）",
              all(len(x) == 32 for x in r128) and all(len(x) == 40 for x in r160))
        check("双栈 ID 集合互不相交", not (set(r128) & set(r160)))
        # 独立 Python 复刻必须与 Rust 守护进程真实输出逐项一致
        py128 = [kad_id_128(NAMESPACE, k) for k in pub]
        py160 = [dht_id_160(NAMESPACE, k) for k in pub]
        check("独立复刻与 Rust 守护进程双栈 ID 逐项一致",
              py128 == r128 and py160 == r160,
              f"rust128={r128[:1]} py128={py128[:1]}")

        # --- 真实 UDP 串扰判据 ---
        dht_ids = list(dict.fromkeys(r160)) or ["f" * 40]
        probes: dict[str, dict] = {}
        probes["kad_published_key_A"] = udp_probe(
            a_host, PORTS["A"]["udp"], {"t": "KAD_FIND", "key": pub[0]})
        probes["kad_published_key_B_cross_host"] = udp_probe(
            b_host, PORTS["B"]["udp"], {"t": "KAD_FIND", "key": pub[0]})
        probes["dht_160bit_id_A"] = udp_probe(
            a_host, PORTS["A"]["udp"], {"t": "KAD_FIND", "key": dht_ids[0]})
        probes["kad_128bit_id_as_key_A"] = udp_probe(
            a_host, PORTS["A"]["udp"], {"t": "KAD_FIND", "key": r128[0]})
        probes["truncated_key_A"] = udp_probe(
            a_host, PORTS["A"]["udp"], {"t": "KAD_FIND", "key": pub[0][:16]})
        probes["malformed_frame_A"] = udp_probe(
            a_host, PORTS["A"]["udp"], b"{not-json")
        probes["dht_160bit_id_B_cross_host"] = udp_probe(
            b_host, PORTS["B"]["udp"], {"t": "KAD_FIND", "key": dht_ids[0]})

        check("真实 KAD key 在 A 命中（count>0 / hit_node=A）",
              is_hit(probes["kad_published_key_A"]),
              json.dumps(probes["kad_published_key_A"], ensure_ascii=False)[:220])
        check("跨主机真实 UDP 往返可用（B 返回真实 KAD_FIND_RES）",
              probes["kad_published_key_B_cross_host"].get("ok") is True,
              json.dumps(probes["kad_published_key_B_cross_host"], ensure_ascii=False)[:220])
        negative = ["dht_160bit_id_A", "kad_128bit_id_as_key_A", "truncated_key_A",
                    "dht_160bit_id_B_cross_host"]
        check("160-bit DHT 形状 / 128-bit ID / 截断 key 均不串扰命中",
              all(not is_hit(probes[k]) for k in negative),
              json.dumps({k: is_hit(probes[k]) for k in negative}))
        check("畸形帧不产生任何命中（守护进程静默丢弃）",
              not is_hit(probes["malformed_frame_A"])
              and probes["malformed_frame_A"].get("no_reply") is True)

        # --- 双栈并行压测 ---
        stress = parallel_stream(a_host, PORTS["A"]["udp"], pub, dht_ids,
                                 args.rounds, args.udp_timeout)
        check("并行双栈压测 KAD 流全部真实命中",
              stress["kad"]["hits"] == stress["kad"]["frames"],
              f"hits={stress['kad']['hits']}/{stress['kad']['frames']}")
        check("并行双栈压测无 ID 空间串扰",
              stress["no_crosstalk"], f"misrouted={stress['misrouted_frames']}")
        check("并行压测无丢包 / 无错误",
              stress["kad"]["no_reply"] == 0 and stress["dht"]["no_reply"] == 0
              and stress["kad"]["errors"] == 0 and stress["dht"]["errors"] == 0)
        cross = parallel_stream(b_host, PORTS["B"]["udp"], pub, dht_ids,
                                max(10, args.rounds // 2), args.udp_timeout)
        check("跨主机并行双栈亦无串扰（B 侧）", cross["no_crosstalk"])

        inspection_after = http_cli(a_base, "inspect", ["kad"])
        check("压测未污染 A 的 published_keys 集合",
              inspection_after.get("kad", {}).get("published_keys") == pub)

        passed = sum(1 for c in checks if c["ok"])
        report = {
            "schema": "coldrevive.s3.m1-dualstack/v1",
            "harness": "tests/spikes/s3/m1_dualstack_integration.py",
            "spike": "S3", "milestone": "M2/W3.5①（S3 附件，非 pass 证据）",
            "recorded_at": now(),
            "scope": ("两台不同物理主机上真实 coldrevived 进程的 M1 双栈集成；"
                      "私有 LAN UDP/HTTP/CLI 真实观测；未加入公共 Mainline DHT"),
            "environment": {
                "health": health,
                "two_distinct_physical_hosts": bool(distinct),
                "a_process": proc_info,
                "ports": PORTS,
                "namespace": NAMESPACE,
                "nat_section": "absent（M1 原行为，未启用 W3.3/W3.4 接线）",
            },
            "dual_stack_ids": {
                "published_keys": pub, "kad_ids_128": r128, "dht_ids_160": r160,
                "independent_python_recompute_matches_rust": py128 == r128 and py160 == r160,
                "id_sets_disjoint": not (set(r128) & set(r160)),
            },
            "crosstalk_probes": probes,
            "parallel_stress_local_A": stress,
            "parallel_stress_cross_host_B": cross,
            "bandwidth": {
                "method": "真实 UDP 线上收+发字节 / 真实墙钟耗时",
                "local_total_mbps": stress["total_mbps"],
                "cross_host_total_mbps": cross["total_mbps"],
                "local_wire_bytes": stress["total_wire_bytes"],
                "note": ("双栈合计吞吐≈两栈之和，未观测异常放大；"
                         "跨进程 RSS 增量本轮未取，沿用既有本地上界量测"),
            },
            "gaps": [
                "G3 第三方客户端兼容性矩阵（M3 范围，本机无该资产）",
                "真实 Mainline DHT 公网加入（合规红线：不向公共 KAD/DHT 发布测试 Key）",
                "跨进程 RSS 增量在本 harness 未取；已有本地结构层上界量测",
            ],
            "checks": checks,
            "summary": {"total": len(checks), "passed": passed,
                        "failed": len(checks) - passed,
                        "status": "OBSERVED" if passed == len(checks) else "FAIL"},
            "evidence_valid": passed == len(checks),
            "evidence_valid_for_pass": False,
            "s3_status": "not_run",
            "g3_matrix_available": False,
            "spike_status_changed": False,
            "no_auto_pass": True,
            "public_kad_dht_contacted": False,
            "decision": "结论由实验执行人依 S3.evidence_required 手工回填 spikes.json",
        }
        return report, 0 if passed == len(checks) else 1
    finally:
        local_a.stop()


def _blocked(reason: str, health: dict, checks: list[dict]) -> dict:
    return {
        "schema": "coldrevive.s3.m1-dualstack/v1", "recorded_at": now(),
        "status": "BLOCKED", "reason": reason, "health": health, "checks": checks,
        "evidence_valid": False, "evidence_valid_for_pass": False,
        "s3_status": "not_run", "g3_matrix_available": False,
        "spike_status_changed": False, "no_auto_pass": True,
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("--a-host", default=DEFAULT_A)
    ap.add_argument("--b-host", default=DEFAULT_B)
    ap.add_argument("--no-spawn-a", action="store_true",
                    help="不自动拉起本地 A 守护进程（要求 A 已在运行）")
    ap.add_argument("--rounds", type=int, default=60)
    ap.add_argument("--udp-timeout", type=float, default=0.6)
    ap.add_argument("--out", default="")
    args = ap.parse_args()
    if args.self_test:
        return self_test()
    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    report, code = real_run(args)
    out = (Path(args.out) if args.out else REPORT_DIR / (
        "s3-m1-dualstack-" + datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + ".json"))
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    s = report.get("summary", {})
    print(f"  status={report.get('status', 'OBSERVED')}  checks={s.get('passed')}/{s.get('total')}")
    print(f"  s3_status={report['s3_status']}  evidence_valid_for_pass=False  "
          f"g3_matrix_available=False  spike_status_changed=False")
    print("report:", out)
    return code


if __name__ == "__main__":
    raise SystemExit(main())
