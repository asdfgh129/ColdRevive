#!/usr/bin/env python3
"""tcp_discovery CLI 定点自测（回环控制路径，不依赖公网）。

目的：在「常规打洞路径失效、需要判断是否适用映射端口发现」时，先用一个
可在开发机/实验机独立运行的 CLI 冒烟测试确认工具链本身可用。本脚本只验证
observer 回写、EIM 判定与输出字段，**不产生 NAT EIM 证据**，也不碰门禁常量。

用法：
    cargo build --offline -p coldrevive-nat
    python3 tests/e2e/w56/tcp_discovery_selftest.py \
        --bin target/debug/coldrevive-nat \
        --report-out tests/e2e/w56/reports/tcp-discovery-selftest.json
"""

from __future__ import annotations

import argparse
import json
import os
import socket
import subprocess
import sys
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]


def default_bin() -> Path:
    name = "coldrevive-nat.exe" if os.name == "nt" else "coldrevive-nat"
    return REPO_ROOT / "target" / "debug" / name


def free_port(host: str = "127.0.0.1") -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind((host, 0))
        return s.getsockname()[1]


def wait_listening(host: str, port: int, attempts: int = 40) -> bool:
    for _ in range(attempts):
        try:
            with socket.create_connection((host, port), timeout=0.2):
                return True
        except OSError:
            time.sleep(0.05)
    return False


class FakeObserver(threading.Thread):
    """回写固定假端点，用于验证「两个观测不一致 → 不合成端口」。"""

    def __init__(self, line: str):
        super().__init__(daemon=True)
        self.line = line
        self._sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._sock.bind(("127.0.0.1", 0))
        self._sock.listen(8)
        self._sock.settimeout(0.2)
        self.port = self._sock.getsockname()[1]
        self._stop = threading.Event()

    def run(self) -> None:
        while not self._stop.is_set():
            try:
                conn, _ = self._sock.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            with conn:
                conn.sendall(self.line.encode("utf-8"))

    def close(self) -> None:
        self._stop.set()
        self._sock.close()
        self.join(timeout=1.0)


def run_cli(bin_path: Path, *args: str, timeout: float = 15.0) -> dict:
    proc = subprocess.run(
        [str(bin_path), *args],
        capture_output=True,
        text=True,
        timeout=timeout,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"CLI exit={proc.returncode}: args={args!r} stderr={proc.stderr.strip()!r}"
        )
    lines = [ln for ln in proc.stdout.strip().splitlines() if ln.strip()]
    if not lines:
        raise RuntimeError(f"CLI 无输出: args={args!r}")
    try:
        return json.loads(lines[-1])
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"CLI 输出非 JSON: {lines[-1]!r}") from exc


def check_case(name: str, expect: dict, actual: dict) -> dict:
    failures = []
    for key, wanted in expect.items():
        got = actual.get(key)
        if got != wanted:
            failures.append(f"{key}: expected {wanted!r}, got {got!r}")
    return {
        "name": name,
        "pass": not failures,
        "expect": expect,
        "actual": actual,
        "failures": failures,
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--bin", type=Path, default=default_bin(), help="coldrevive-nat 可执行文件路径")
    ap.add_argument("--host", default="127.0.0.1", help="observer 绑定地址与客户端本地地址")
    ap.add_argument("--timeout-ms", type=int, default=2000, help="--discover-tcp-port 超时")
    ap.add_argument("--observer-secs", type=int, default=12, help="真 observer 存活秒数")
    ap.add_argument("--report-out", type=Path, help="可选：写出 JSON 报告")
    args = ap.parse_args()

    bin_path = args.bin.resolve()
    if not bin_path.is_file():
        print(f"[FAIL] 未找到 {bin_path}；先运行 cargo build --offline -p coldrevive-nat", file=sys.stderr)
        return 2

    cases = []
    procs = []
    fakes = []
    try:
        p1, p2 = free_port(args.host), free_port(args.host)
        while p2 == p1:
            p2 = free_port(args.host)
        for port in (p1, p2):
            proc = subprocess.Popen(
                [
                    str(bin_path),
                    "--serve-observer",
                    f"{args.host}:{port}",
                    "--observer-secs",
                    str(args.observer_secs),
                    "--json",
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            procs.append(proc)
        if not all(wait_listening(args.host, p) for p in (p1, p2)):
            raise RuntimeError("observer 未在限时内监听；检查端口/防火墙或 --host")

        local_port = free_port(args.host)
        two = run_cli(
            bin_path,
            "--discover-tcp-port",
            f"{args.host}:{local_port}",
            f"{args.host}:{p1},{args.host}:{p2}",
            "--timeout",
            str(args.timeout_ms),
            "--json",
        )
        cases.append(
            check_case(
                "two_observers_consistent_eim",
                {
                    "eim": True,
                    "discovered_port": local_port,
                    "g2_claim": False,
                    "s6_status": "not_run",
                    "auto_pass": False,
                },
                two,
            )
        )
        endpoints = [o.get("mapped_endpoint") for o in two.get("observations", [])]
        cases.append(
            check_case(
                "two_observers_same_mapped_endpoint",
                {"endpoints": [f"{args.host}:{local_port}"] * 2},
                {"endpoints": endpoints},
            )
        )

        one_local = free_port(args.host)
        one = run_cli(
            bin_path,
            "--discover-tcp-port",
            f"{args.host}:{one_local}",
            f"{args.host}:{p1}",
            "--timeout",
            str(args.timeout_ms),
            "--json",
        )
        cases.append(
            check_case(
                "single_observer_is_unproven",
                {"eim": None, "discovered_port": one_local},
                one,
            )
        )

        dead_port = free_port(args.host)
        dead = run_cli(
            bin_path,
            "--discover-tcp-port",
            f"{args.host}:{free_port(args.host)}",
            f"{args.host}:{dead_port}",
            "--timeout",
            str(max(args.timeout_ms, 500)),
            "--json",
        )
        cases.append(
            check_case(
                "unreachable_observer_is_blocked",
                {"eim": False, "discovered_port": None},
                dead,
            )
        )

        # 两个假 observer 故意回写不同端点：验证不一致时绝不输出 discovered_port。
        fake_a = FakeObserver("CR-MAPPED 198.51.100.1:1111\n")
        fake_b = FakeObserver("CR-MAPPED 198.51.100.2:2222\n")
        fakes.extend([fake_a, fake_b])
        fake_a.start()
        fake_b.start()
        bad_local = free_port(args.host)
        inconsistent = run_cli(
            bin_path,
            "--discover-tcp-port",
            f"{args.host}:{bad_local}",
            f"{args.host}:{fake_a.port},{args.host}:{fake_b.port}",
            "--timeout",
            str(args.timeout_ms),
            "--json",
        )
        cases.append(
            check_case(
                "inconsistent_observers_have_no_port",
                {"eim": False, "discovered_port": None},
                inconsistent,
            )
        )
        cases.append(
            check_case(
                "inconsistent_reason_is_explicit",
                {"hit": True},
                {"hit": "不一致" in inconsistent.get("reason", "")},
            )
        )
    except Exception as exc:  # noqa: BLE001 - 定点脚本要把任何异常落进报告
        cases.append(
            {
                "name": "harness",
                "pass": False,
                "expect": "no exception",
                "actual": {"error": str(exc)},
                "failures": [str(exc)],
            }
        )
    finally:
        for fake in fakes:
            fake.close()
        for proc in procs:
            if proc.poll() is None:
                proc.terminate()
            try:
                proc.wait(timeout=2.0)
            except subprocess.TimeoutExpired:  # pragma: no cover - 极端情况下强杀
                proc.kill()
                proc.wait(timeout=2.0)

    passed = all(c["pass"] for c in cases)
    report = {
        "recorded_at": datetime.now(timezone.utc).isoformat(),
        "scope": "Loopback CLI smoke for tcp_discovery control path; NOT NAT EIM evidence.",
        "bin": str(bin_path),
        "cases": cases,
        "status": "PASS" if passed else "FAIL",
        "g2_claim": False,
        "s6_status": "not_run",
        "evidence_valid_for_pass": False,
        "auto_pass": False,
    }
    if args.report_out:
        args.report_out.parent.mkdir(parents=True, exist_ok=True)
        args.report_out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    for case in cases:
        print(f"[{ 'PASS' if case['pass'] else 'FAIL' }] {case['name']}")
        for failure in case["failures"]:
            print(f"       {failure}")
    print(f"RESULT={report['status']} ({sum(c['pass'] for c in cases)}/{len(cases)})")
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
