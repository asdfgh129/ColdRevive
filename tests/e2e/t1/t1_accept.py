#!/usr/bin/env python3
"""T1 双机 + 真实 NAT 全链路验收 harness（G1 证据来源之一）。

红线（与 T0 一致，且更严）：
  * 观测一律来自真实 CLI / HTTP / 文件 / STUN 探针；二进制缺失、字段缺失、命令失败
    一律判 BLOCKED 或 FAIL，绝不回填「看起来像通过」的默认值。
  * `evidence_valid=true` 只能由 --mode real 且**同时满足**下列条件产生：
      ① 全部 mandatory 用例 PASS 且无 BLOCKED/SKIP；
      ② /health 实测 A/B 的 machine_id 与 host 均不同（真两台机器）；
      ③ 真实 NAT 探针报告存在且非合成（public_udp_egress_ok）；
      ④ 观测来源为 real，非 stub。
    --self-test / --lan-precheck 的 evidence_valid 恒为 false。
  * 本 harness 不假装能远程拉起进程：real 模式下 B 由操作员在远端主机预启动，
    harness 只做健康检查与远端 CLI 取数；缺少远端前置条件时整体 BLOCKED、退出码 2。

用法：
  python3 tests/e2e/t1/t1_accept.py --list
  python3 tests/e2e/t1/t1_accept.py --self-test
  python3 tests/e2e/t1/t1_accept.py --lan-precheck
  T1_HOST_A=... T1_HOST_B=... T1_REMOTE_PRESTARTED=1 \
  COLDREVIVE_DAEMON=... COLDREVIVE_CLI=... T0_DOWNLOADER=... \
  python3 tests/e2e/t1/t1_accept.py --mode real
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import random
import shutil
import subprocess
import sys
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
DEFAULT_CONFIG = HERE / "t1_config.json"
DEFAULT_CASES = HERE / "t1_cases.json"

MISSING = "<missing>"


class BlockedError(RuntimeError):
    """真实环境前置条件不满足；用例标 BLOCKED，不计入 PASS。"""


class UnavailableError(RuntimeError):
    """真实观测不可用（进程/命令/字段缺失）；不得降级为合成值。"""


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def bdecode(raw: bytes, i: int = 0) -> tuple[Any, int]:
    c = raw[i:i + 1]
    if c == b"i":
        j = raw.index(b"e", i)
        return int(raw[i + 1:j]), j + 1
    if c == b"l":
        out, i = [], i + 1
        while raw[i:i + 1] != b"e":
            v, i = bdecode(raw, i)
            out.append(v)
        return out, i + 1
    if c == b"d":
        out, i = {}, i + 1
        while raw[i:i + 1] != b"e":
            k, i = bdecode(raw, i)
            v, i = bdecode(raw, i)
            out[k] = v
        return out, i + 1
    if c.isdigit():
        j = raw.index(b":", i)
        n = int(raw[i:j])
        return raw[j + 1:j + 1 + n], j + 1 + n
    raise ValueError(f"非法 bencode 类型: {c!r}")


def torrent_infohash(path: Path) -> str:
    """按 BT 规范对 info 字典原始字节做 SHA-1，不采信文件名里的产品自报值。"""
    raw = path.read_bytes()
    top, _ = bdecode(raw, 0)
    if not isinstance(top, dict) or b"info" not in top:
        raise UnavailableError(f"{path} 不是合法 .torrent（缺 info 字典）")
    marker = b"4:info"
    pos = raw.find(marker)
    if pos < 0:
        raise UnavailableError(f"{path} 中未找到 info 字段")
    start = pos + len(marker)
    _, end = bdecode(raw, start)
    return hashlib.sha1(raw[start:end]).hexdigest()


def resolve_expect(expect: Any, cfg: dict[str, Any]) -> Any:
    if isinstance(expect, str) and expect.startswith("@"):
        node: Any = cfg
        for part in expect[1:].split("."):
            node = node[part]
        return node
    return expect


def evaluate(op: str, actual: Any, expect: Any) -> bool:
    if actual is MISSING:
        return False
    try:
        if op == "eq":
            return actual == expect
        if op == "ne":
            return actual != expect
        if op == "gte":
            return actual >= expect
        if op == "gt":
            return actual > expect
        if op == "lt":
            return actual < expect
        if op == "lte":
            return actual <= expect
        if op == "nonempty":
            return bool(actual)
        if op == "true":
            return actual is True
    except (TypeError, ValueError):
        return False
    raise ValueError(f"未知断言操作符 {op}")


def run_assertions(case: dict[str, Any], values: dict[str, Any],
                   cfg: dict[str, Any]) -> list[dict[str, Any]]:
    out = []
    for a in case["assertions"]:
        actual = values.get(a["key"], MISSING)
        expect = resolve_expect(a["expect"], cfg)
        out.append({
            "label": a["label"], "key": a["key"], "op": a["op"],
            "expect": expect, "actual": actual,
            "ok": evaluate(a["op"], actual, expect),
        })
    return out


def percentile(values: list[float], q: float) -> float:
    if not values:
        return math.inf
    s = sorted(values)
    idx = max(0, math.ceil(q * len(s)) - 1)
    return s[min(idx, len(s) - 1)]


# ---------------------------------------------------------------------------
# Stub：仅用于 --self-test 验证编排与断言引擎，永不作为证据
# ---------------------------------------------------------------------------

def ideal_values(cfg: dict[str, Any]) -> dict[str, Any]:
    n = int(cfg["thresholds"]["wake_sample_count"])
    m = int(cfg["thresholds"]["revival_sample_count"])
    return {
        "a_health": True, "b_health": True, "distinct_host": True,
        "distinct_machine": True, "distinct_node": True,
        "hit_node": "A", "result_count": 1, "namespace_ok": True,
        "published_keys_count": 4,
        "req_valid": True, "challenge_ok": True, "nonce_unique": True,
        "time_window_ok": True, "wake_status": "OK", "ack_fields_ok": True,
        "p95_ms": 180.0, "sample_count": n, "timeout_samples": 0,
        "success_rate": 1.0, "success": m, "total": m,
        "torrent_fetch_ok": True, "torrent_parse_ok": True,
        "infohash_match": True, "infohash_len": 40,
        "download_ok": True, "sha256_match": True, "piece_sha1_ok": True,
        "private_protocol_in_data_plane": False, "downloader_third_party": True,
        "stun_responded": 4, "mapped_address": "203.0.113.1:36764",
        "public_udp_egress": True, "nat_report_real": True,
        "golden_req_ok": True, "golden_ack_ok": True, "protocol_version": 1,
        "shared_dirs_empty": True, "rpc_secret_required": True, "residual_pids": 0,
    }


def framework_check(cases: list[dict[str, Any]], cfg: dict[str, Any]) -> list[dict[str, Any]]:
    """注入坏观测，确认断言引擎确实会判 FAIL（防「永远绿」）。"""
    good = ideal_values(cfg)
    bad = {**good, "wake_status": "REJECT", "p95_ms": 99999.0,
           "success_rate": 0.5, "distinct_machine": False}
    good_ok = all(a["ok"] for a in run_assertions(cases[2], good, cfg))
    bad_ok = all(a["ok"] for a in run_assertions(cases[2], bad, cfg))
    lat_good = all(a["ok"] for a in run_assertions(cases[3], good, cfg))
    lat_bad = all(a["ok"] for a in run_assertions(cases[3], bad, cfg))
    return [
        {"label": "正常观测被判 PASS", "ok": good_ok and lat_good, "actual": good_ok and lat_good},
        {"label": "坏观测被判 FAIL", "ok": not bad_ok and not lat_bad,
         "actual": not bad_ok and not lat_bad},
    ]


# ---------------------------------------------------------------------------
# 真实 runner
# ---------------------------------------------------------------------------

class T1Runner:
    def __init__(self, cfg: dict[str, Any], cases: dict[str, Any], mode: str,
                 refresh_nat: bool, keep_logs: bool, address_mode: str = "aliases"):
        self.cfg = cfg
        self.cases = cases
        self.mode = mode
        self.address_mode = address_mode
        self.refresh_nat = refresh_nat
        self.keep_logs = keep_logs
        self.workdir = ROOT / cfg["paths"]["workdir"]
        self.sample_dir = ROOT / cfg["paths"]["sample_dir"]
        self.report_dir = ROOT / cfg["paths"]["reports"]
        self.golden_dir = ROOT / cfg["paths"]["golden_dir"]
        self.sample = self.sample_dir / cfg["fixture"]["filename"]
        self.procs: dict[str, subprocess.Popen] = {}
        self.logs: list[Any] = []
        self.config_path = self.workdir / "t1-config.json"
        self.hosts: dict[str, str] = {}
        self.binds: dict[str, str] = {}
        self._file_hash = ""
        self._infohash = ""

    # ---------- 环境准备 ----------

    def binaries(self) -> dict[str, str]:
        return {k: os.environ.get(v, "").strip() for k, v in self.cfg["bin"].items()}

    def require_binaries(self) -> None:
        missing = [k for k, v in self.binaries().items() if not v or not Path(v).is_file()]
        if missing:
            raise BlockedError("真实二进制未就绪：" + ", ".join(missing)
                               + "（须为实际文件，不接受占位）")

    def prepare(self) -> None:
        if self.workdir.name != ".run" or self.workdir.parent != HERE:
            raise RuntimeError(f"拒绝清理非预期工作目录: {self.workdir}")
        if self.workdir.exists():
            shutil.rmtree(self.workdir)
        self.workdir.mkdir(parents=True, exist_ok=True)
        self.sample_dir.mkdir(parents=True, exist_ok=True)
        self.report_dir.mkdir(parents=True, exist_ok=True)
        f = self.cfg["fixture"]
        data = random.Random(int(f["seed"])).randbytes(int(f["size_bytes"]))
        self.sample.write_bytes(data)
        self._file_hash = hashlib.sha256(data).hexdigest()

        if self.mode == "real":
            self.hosts["A"] = os.environ.get("T1_HOST_A", "").strip()
            self.hosts["B"] = os.environ.get("T1_HOST_B", "").strip()
            self.binds["A"] = self.hosts["A"]
        else:
            spec = self.cfg["lan_precheck"]["modes"][self.address_mode]
            self.hosts = dict(spec["host"])
            self.binds = dict(spec["bind"])

        self._write_config()

    def _write_config(self) -> None:
        nodes = {}
        for node, spec in self.cfg["nodes"].items():
            nodes[node] = {
                "host": self.hosts.get(node, spec["host"]),
                "http_port": spec["http_port"],
                "wake_udp_port": spec["wake_udp_port"],
                "bt_port": spec["bt_port"],
            }
        def rel(p: Any) -> str:
            # 相对仓库根目录：跨机拷贝后只需从仓库根启动守护进程即可解析，无需改路径
            return os.path.relpath(str(p), str(ROOT)).replace("\\", "/")
        content = {
            "suite": "T1",
            "schema": "coldrevive.daemon.config/v1",
            "bind": self.binds.get("A", "127.0.0.1"),
            "nodes": nodes,
            "sample_path": rel(self.sample),
            "keywords": self.cfg["fixture"]["keywords"],
            "namespace": self.cfg["protocol"]["namespace"],
            "workdir": rel(self.workdir),
            "golden_req": rel(self.golden_dir / "wakeup_req.v1.json"),
            "golden_ack": rel(self.golden_dir / "wakeup_ack.v1.json"),
            "downloader": self.binaries().get("downloader_env", ""),
            "challenge_window_sec": self.cfg["protocol"]["challenge_window_sec"],
        }
        self.config_path.write_text(
            json.dumps(content, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    # ---------- 进程 ----------

    def spawn(self, node: str) -> None:
        b = self.binaries()
        values = {
            "daemon": b["daemon_env"], "config": str(self.config_path),
            "node": node, "bind": self.binds[node], "host": self.hosts[node],
            "workdir": str(self.workdir / node), "sample": str(self.sample),
            "downloader": b["downloader_env"],
        }
        (self.workdir / node).mkdir(parents=True, exist_ok=True)
        argv = [p.format(**values) for p in self.cfg["launch"]["daemon_argv"]]
        log = (self.workdir / f"{node}.log").open("wb")
        self.logs.append(log)
        proc = subprocess.Popen(argv, stdout=log, stderr=subprocess.STDOUT,
                                cwd=str(ROOT))
        self.procs[node] = proc

    def health(self, node: str) -> dict[str, Any] | None:
        spec = self.cfg["nodes"][node]
        url = f"http://{self.hosts[node]}:{spec['http_port']}/health"
        try:
            with urllib.request.urlopen(url, timeout=float(self.cfg["timeouts_sec"]["health"])) as r:
                if r.status != 200:
                    return None
                body = json.loads(r.read().decode("utf-8"))
                return body if isinstance(body, dict) else None
        except Exception:
            return None

    def wait_health(self, node: str) -> bool:
        deadline = time.monotonic() + float(self.cfg["timeouts_sec"]["startup"])
        while time.monotonic() < deadline:
            if self.health(node) is not None:
                return True
            time.sleep(0.2)
        return False

    def start_processes(self) -> None:
        self.require_binaries()
        if self.mode == "real":
            # B 远端预启动：只接受显式确认，绝不静默补一个本地进程冒充远端
            if not self.hosts.get("A") or not self.hosts.get("B"):
                raise BlockedError("real 模式须设置 T1_HOST_A / T1_HOST_B")
            if os.environ.get("T1_REMOTE_PRESTARTED", "").strip() != "1":
                raise BlockedError("real 模式须设置 T1_REMOTE_PRESTARTED=1 确认 B 已在远端主机启动")
            if self.hosts["A"] == self.hosts["B"]:
                raise BlockedError("A/B 地址相同，不构成双机环境")
            self.spawn("A")
            if not self.wait_health("A"):
                raise BlockedError("A /health 未就绪")
            if not self.wait_health("B"):
                raise BlockedError("B /health 未就绪（远端守护进程须由操作员预启动）")
        else:
            for node in ("A", "B"):
                self.spawn(node)
            for node in ("A", "B"):
                if not self.wait_health(node):
                    raise BlockedError(f"{node} /health 未就绪（绑定 {self.binds[node]} 失败？"
                                      f" 见 {self.workdir / (node + '.log')}）")

    def cleanup(self) -> None:
        for proc in self.procs.values():
            if proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
        for log in self.logs:
            try:
                log.close()
            except OSError:
                pass
        if not self.keep_logs and self.workdir.exists():
            # 仅清理本 harness 自建的 .run 目录
            try:
                shutil.rmtree(self.workdir)
            except OSError:
                pass

    # ---------- 真实观测 ----------

    def cli(self, node: str, command: str, *args: str, timeout: float = 190.0) -> dict[str, Any]:
        exe = self.binaries()["cli_env"]
        if not exe or not Path(exe).is_file():
            raise UnavailableError("coldrevive-cli 二进制不可用")
        env = {**os.environ, "COLDREVIVE_T0_CONFIG": str(self.config_path)}
        argv = [exe, "--json", "--node", node, command, *args]
        try:
            p = subprocess.run(argv, capture_output=True, text=True, env=env,
                                timeout=timeout, cwd=str(ROOT))
        except subprocess.TimeoutExpired as exc:
            raise UnavailableError(f"CLI 超时: {command}") from exc
        if p.returncode != 0:
            raise UnavailableError(f"CLI 退出 {p.returncode}: {command}: {p.stderr.strip()[:200]}")
        try:
            out = json.loads(p.stdout.strip())
        except json.JSONDecodeError as exc:
            raise UnavailableError(f"CLI 输出非 JSON: {command}: {p.stdout[:120]!r}") from exc
        if not isinstance(out, dict):
            raise UnavailableError(f"CLI 输出非对象: {command}")
        if "error" in out:
            raise UnavailableError(f"CLI 返回错误: {command}: {out['error']}")
        return out

    def infohash(self) -> str:
        if self._infohash:
            return self._infohash
        meta = self.workdir / "A" / "meta"
        files = sorted(meta.glob("*.torrent")) if meta.is_dir() else []
        if not files:
            files = sorted((self.workdir / "meta").glob("*.torrent"))
        if not files:
            raise UnavailableError("A 未生成伴随 .torrent")
        self._infohash = torrent_infohash(files[0])
        return self._infohash

    def magnet(self) -> str:
        return "magnet:?xt=urn:btih:" + self.infohash()

    def nat_report(self) -> dict[str, Any]:
        if self.refresh_nat:
            out = self.report_dir / ("t1-nat-" + datetime.now(timezone.utc)
                                     .strftime("%Y%m%dT%H%M%SZ") + ".json")
            probe = ROOT / "tests/spikes/s6/stun_probe.py"
            if not probe.is_file():
                raise UnavailableError("STUN 探针不存在")
            p = subprocess.run([sys.executable, str(probe), "--out", str(out),
                                "--timeout", str(self.cfg["timeouts_sec"]["stun"])],
                               capture_output=True, text=True, timeout=90)
            if p.returncode != 0 or not out.is_file():
                raise UnavailableError(f"STUN 探针失败: {p.stderr[-200:]}")
            return load_json(out)
        path = ROOT / self.cfg["paths"]["nat_report_fallback"]
        if not path.is_file():
            raise UnavailableError(f"真实 NAT 报告不存在: {path}（可用 --refresh-nat 现场采集）")
        return load_json(path)

    def observe(self, case_id: str) -> dict[str, Any]:
        c = self.cfg
        n = int(c["thresholds"]["wake_sample_count"])
        m = int(c["thresholds"]["revival_sample_count"])
        kw = c["fixture"]["keywords"][0]

        if case_id == "T1-01":
            ha, hb = self.health("A"), self.health("B")
            if ha is None or hb is None:
                raise UnavailableError("A/B /health 不可达")
            return {
                "a_health": ha.get("status") == "ok",
                "b_health": hb.get("status") == "ok",
                "distinct_host": ha.get("host") != hb.get("host"),
                "distinct_machine": ha.get("machine_id") != hb.get("machine_id"),
                "distinct_node": ha.get("node") != hb.get("node"),
            }
        if case_id == "T1-02":
            r = self.cli("B", "find", kw)
            k = self.cli("A", "inspect", "kad")
            return {
                "hit_node": r.get("result", {}).get("hit_node"),
                "result_count": r.get("result", {}).get("count"),
                "namespace_ok": k.get("kad", {}).get("namespace_ok"),
                "published_keys_count": len(k.get("kad", {}).get("published_keys", [])),
            }
        if case_id == "T1-03":
            probe = self.cli("B", "wake", "probe", self._file_hash, self.infohash())
            ack = self.cli("B", "wake", "ack", self._file_hash, self.infohash())
            return {
                "req_valid": probe.get("req", {}).get("valid"),
                "challenge_ok": probe.get("req", {}).get("challenge_ok"),
                "nonce_unique": probe.get("req", {}).get("nonce_unique"),
                "time_window_ok": probe.get("req", {}).get("time_window_ok"),
                "wake_status": ack.get("ack", {}).get("status"),
                "ack_fields_ok": ack.get("ack", {}).get("fields_ok"),
            }
        if case_id == "T1-04":
            r = self.cli("B", "metrics", "wake-latency", str(n))
            samples = [float(x) for x in r.get("samples_ms", []) if isinstance(x, (int, float))]
            return {
                "p95_ms": round(percentile(samples, 0.95), 2),
                "sample_count": len(samples),
                "timeout_samples": sum(1 for x in samples if x >= 99999),
            }
        if case_id == "T1-05":
            r = self.cli("B", "metrics", "revival", str(m))
            success = float(r.get("success", -1))
            total = float(r.get("total", -1))
            return {"success": success, "total": total,
                    "success_rate": (success / total) if total > 0 else 0.0}
        if case_id == "T1-06":
            r = self.cli("B", "wake", "ack", self._file_hash, self.infohash())
            a = r.get("ack", {})
            ih = self.infohash()
            return {
                "torrent_fetch_ok": a.get("torrent_fetch_ok"),
                "torrent_parse_ok": a.get("torrent_parse_ok"),
                "infohash_match": a.get("magnet_infohash") == ih,
                "infohash_len": len(ih),
            }
        if case_id == "T1-07":
            r = self.cli("B", "download", self.magnet(), timeout=190.0)
            d = r.get("download", {})
            dl = self.binaries()["downloader_env"]
            third_party = bool(dl) and Path(dl).is_file() \
                and not Path(dl).name.lower().startswith("coldrevive")
            return {
                "download_ok": d.get("complete"),
                "sha256_match": d.get("sha256") == self._file_hash,
                "piece_sha1_ok": d.get("piece_sha1_ok"),
                "private_protocol_in_data_plane": d.get("private_protocol_seen"),
                "downloader_third_party": third_party,
            }
        if case_id == "T1-08":
            rep = self.nat_report()
            cls = rep.get("classification", {})
            results = rep.get("stun_results", [])
            mapped = cls.get("mapped_addresses") or []
            real = (rep.get("harness", "").endswith("stun_probe.py")
                    and bool(rep.get("stun_results")) and "classification" in rep)
            return {
                "stun_responded": sum(1 for r in results if r.get("ok")),
                "mapped_address": mapped[0] if mapped else "",
                "public_udp_egress": cls.get("public_udp_egress_ok"),
                "nat_report_real": real,
            }
        if case_id == "T1-09":
            g = self.cli("A", "golden", "verify", "v1")
            req = load_json(self.golden_dir / "wakeup_req.v1.json")
            return {
                "golden_req_ok": g.get("req_ok"),
                "golden_ack_ok": g.get("ack_ok"),
                "protocol_version": req.get("v"),
            }
        if case_id == "T1-10":
            e = self.cli("A", "env", "defaults")
            d = e.get("defaults", {})
            pids = e.get("env", {}).get("residual_pids")
            return {
                "shared_dirs_empty": d.get("shared_dirs_empty"),
                "rpc_secret_required": d.get("rpc_secret_required"),
                "residual_pids": len(pids) if isinstance(pids, list) else MISSING,
            }
        raise UnavailableError(f"未定义观测实现: {case_id}")


def build_report(runner: T1Runner, mode: str, results: list[dict[str, Any]],
                 env_facts: dict[str, Any]) -> dict[str, Any]:
    summary = {
        "total": len(results),
        "passed": sum(1 for c in results if c["status"] == "PASS"),
        "failed": sum(1 for c in results if c["status"] == "FAIL"),
        "blocked": sum(1 for c in results if c["status"] == "BLOCKED"),
        "skipped": sum(1 for c in results if c["status"] == "SKIP"),
        "mandatory_failed": sum(1 for c in results
                                if c["mandatory"] and c["status"] != "PASS"),
    }
    all_green = (summary["failed"] == 0 and summary["blocked"] == 0
                 and summary["skipped"] == 0 and summary["mandatory_failed"] == 0)
    two_hosts = bool(env_facts.get("two_distinct_hosts"))
    real_nat = bool(env_facts.get("real_nat"))
    evidence_valid = (mode == "real" and all_green and two_hosts
                      and real_nat and env_facts.get("observation_source") == "real")
    if evidence_valid:
        reason = "真实双机 + 真实 NAT 全链路通过；可与 T0 共同构成 G1 证据"
    elif mode == "self-test":
        reason = "合成观测自检，仅证明编排与断言引擎正确，不可作为 G1 证据"
    elif mode == "lan-precheck":
        reason = (f"单机预检（{env_facts.get('address_mode', '?')} 地址模式）：A/B 同一物理主机"
                  "（machine_id 相同），不满足 T1 双机 + 真实 NAT 环境，不可作为 G1 证据")
    else:
        bits = []
        if not all_green:
            bits.append("存在 FAIL/BLOCKED/SKIP")
        if not two_hosts:
            bits.append("未证明两台不同物理主机")
        if not real_nat:
            bits.append("真实 NAT 特征报告缺失")
        bits.append("本次结果不计入 G1")
        reason = "；".join(bits)
    return {
        "suite": "T1", "schema": "coldrevive.t1.report/v1",
        "recorded_at": datetime.now(timezone.utc).isoformat(),
        "mode": mode,
        "gate": "G1",
        "evidence_valid": evidence_valid,
        "reason": reason,
        "environment": env_facts,
        "config": {"hosts": runner.hosts, "binds": runner.binds,
                   "config_file": str(runner.config_path)},
        "summary": summary,
        "cases": results,
        "next_actions": ([] if evidence_valid else
                         ["在两台物理主机上分别启动 coldrevived（B 远端预启动后置 T1_REMOTE_PRESTARTED=1）",
                          "设置 T1_HOST_A / T1_HOST_B 与三个二进制环境变量，以 --mode real 重跑",
                          "确保 --mode real 下 /health 的 machine_id 与 host 均不同，并采集真实 NAT 报告",
                          "T1 + T0 共同通过后，才可按 ROADMAP §3 判定 G1 并解除 M2 门禁"]),
    }


def main() -> int:
    ap = argparse.ArgumentParser(description="T1 双机 + 真实 NAT 验收")
    ap.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    ap.add_argument("--cases", type=Path, default=DEFAULT_CASES)
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("--lan-precheck", action="store_true")
    ap.add_argument("--mode", choices=["real"], default=None)
    ap.add_argument("--refresh-nat", action="store_true",
                    help="现场运行 STUN 探针采集真实 NAT 报告（默认复用 S6 先导报告）")
    ap.add_argument("--prepare-only", action="store_true",
                    help="仅生成可移植的 t1-config.json（路径相对仓库根，供远端 B 主机直接使用）；须配合 --mode real")
    ap.add_argument("--address-mode", choices=["aliases", "nic"],
                    default=None, help="单机预检地址模式（默认取 t1_config.json）")
    ap.add_argument("--keep-logs", action="store_true")
    ap.add_argument("--report", type=Path, default=None)
    args = ap.parse_args()

    cfg = load_json(args.config)
    case_data = load_json(args.cases)
    cases = case_data["cases"]
    if args.list:
        for c in cases:
            print(f"{c['id']}  {c['title']}  ({c['group']}, {len(c['assertions'])} 条断言)")
        return 0

    address_mode = args.address_mode or cfg["lan_precheck"]["default_address_mode"]
    if args.prepare_only:
        if args.mode != "real":
            print("--prepare-only 必须配合 --mode real（且不再指定 --self-test/--lan-precheck）",
                  file=sys.stderr)
            return 2
        runner = T1Runner(cfg, case_data, "real", args.refresh_nat, args.keep_logs, address_mode)
        runner.prepare()
        print(f"[PREPARE] 已生成可移植配置: {runner.config_path}")
        print("          将该文件拷到 B 主机，从仓库根目录启动 coldrevived 即可（路径已相对化，无需编辑）")
        return 0
    if sum(bool(x) for x in (args.self_test, args.lan_precheck, args.mode)) != 1:
        print("必须且只能指定 --self-test / --lan-precheck / --mode real 之一", file=sys.stderr)
        return 2
    mode = "self-test" if args.self_test else ("lan-precheck" if args.lan_precheck else "real")
    runner = T1Runner(cfg, case_data, mode, args.refresh_nat, args.keep_logs, address_mode)
    results: list[dict[str, Any]] = []
    env_facts: dict[str, Any] = {
        "definition": cfg["environment"]["definition"],
        "two_distinct_hosts": False,
        "real_nat": False,
        "address_mode": address_mode,
        "observation_source": "stub" if mode == "self-test" else "real",
        "scope": ("synthetic" if mode == "self-test" else
                  ("single-host/" + address_mode) if mode == "lan-precheck"
                  else "two-hosts-real-nat"),
    }
    blocked_reason = ""

    try:
        runner.prepare()
        if mode != "self-test":
            runner.start_processes()
            ha, hb = runner.health("A"), runner.health("B")
            env_facts["health"] = {"A": ha, "B": hb}
            env_facts["two_distinct_hosts"] = bool(
                ha and hb and ha.get("machine_id") != hb.get("machine_id")
                and ha.get("host") != hb.get("host"))
            try:
                rep = runner.nat_report()
                env_facts["real_nat"] = bool(
                    rep.get("classification", {}).get("public_udp_egress_ok"))
                env_facts["nat_report"] = (
                    "fresh STUN probe (--refresh-nat)" if runner.refresh_nat
                    else cfg["paths"]["nat_report_fallback"])
            except UnavailableError as exc:
                env_facts["nat_error"] = str(exc)
    except (BlockedError, UnavailableError, OSError, ValueError) as exc:
        blocked_reason = f"{type(exc).__name__}: {exc}"
        print(f"[BLOCKED] {blocked_reason}", file=sys.stderr)

    try:
        for case in cases:
            cid = case["id"]
            entry: dict[str, Any] = {
                "id": cid, "title": case["title"], "group": case["group"],
                "flow": case["flow"], "source": case["source"],
                "mandatory": case["mandatory"], "status": "SKIP",
                "assertions": [], "note": "",
            }
            if blocked_reason:
                entry.update(status="BLOCKED", note=blocked_reason,
                             observation_source="none")
            elif mode == "lan-precheck" and case.get("requires_two_hosts"):
                entry.update(status="SKIP",
                             note="单机预检：本机只有一台物理主机，T1 双机身份用例不可执行",
                             observation_source="none")
            elif mode == "self-test":
                values = ideal_values(cfg)
                assertions = run_assertions(case, values, cfg)
                entry.update(
                    status="PASS" if all(a["ok"] for a in assertions) else "FAIL",
                    assertions=assertions, observation_source="stub",
                    note="合成观测，仅验证编排与断言引擎")
            else:
                try:
                    values = runner.observe(cid)
                    assertions = run_assertions(case, values, cfg)
                    entry.update(
                        status="PASS" if all(a["ok"] for a in assertions) else "FAIL",
                        assertions=assertions, observation_source="real",
                        note="真实观测：" + ", ".join(f"{k}={v}" for k, v in values.items()))
                except (UnavailableError, OSError, ValueError, KeyError) as exc:
                    entry.update(status="BLOCKED", observation_source="none",
                                 note=f"真实观测不可用: {type(exc).__name__}: {exc}")
            results.append(entry)
            mark = entry["status"]
            tail = f" — {entry['note']}" if mark in ("BLOCKED", "SKIP") else ""
            print(f"[{mark:<7}] {cid} {case['title']}{tail}")

        # 断言引擎自检：注入坏观测必须判 FAIL
        fw = framework_check(cases, cfg)
        results.append({
            "id": "T1-00", "title": "断言引擎自检（注入坏观测必须判 FAIL）",
            "group": "framework", "flow": "—", "source": "本 harness 自检",
            "mandatory": True,
            "status": "PASS" if all(a["ok"] for a in fw) else "FAIL",
            "assertions": fw, "note": "框架自检，不计入 G1 用例数",
            "observation_source": "stub",
        })
        print(f"[{results[-1]['status']:<7}] T1-00 断言引擎自检")

        report = build_report(runner, mode, results, env_facts)
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        out = args.report or (runner.report_dir /
                              f"t1-{stamp}-{mode}-{address_mode}.json")
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n",
                       encoding="utf-8")
        s = report["summary"]
        print("\n" + "=" * 78)
        print(f"T1 {mode.upper()} | 共 {s['total']} 条：PASS {s['passed']} / "
              f"FAIL {s['failed']} / BLOCKED {s['blocked']} / SKIP {s['skipped']}")
        print(f"门禁证据有效: {report['evidence_valid']} — {report['reason']}")
        print(f"报告: {out.relative_to(ROOT) if out.is_relative_to(ROOT) else out}")
        for action in report["next_actions"]:
            print("  → " + action)
        print("=" * 78)
        # 有 SKIP 说明环境不齐（如单机预检缺第二台物理机），按「证据不完整」处理，
        # 不与应用层 FAIL 混为一谈。
        if blocked_reason or s["blocked"] or s["skipped"]:
            return 2
        if s["failed"] or s["mandatory_failed"]:
            return 1
        return 0
    finally:
        runner.cleanup()


if __name__ == "__main__":
    sys.exit(main())
