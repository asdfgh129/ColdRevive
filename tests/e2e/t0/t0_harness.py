#!/usr/bin/env python3
"""T0 环境编排：配置载入、端口预检、确定性样本、进程启停与清理。

只依赖 Python 标准库。本模块不假设 Rust 二进制已存在；真实节点的启动参数
来自 t0_config.json 的 launch 占位契约，接线由 CliAdapter 完成。

单独运行可做环境自检：
    python3 tests/e2e/t0/t0_harness.py --self-check
"""

from __future__ import annotations

import argparse
import hashlib
import json
import random
import shutil
import socket
import subprocess
import sys
import time
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
DEFAULT_CONFIG = HERE / "t0_config.json"

REQUIRED_SECTIONS = (
    "suite", "bind", "nodes", "timeouts_sec", "thresholds",
    "fixture", "protocol", "paths", "bin", "launch",
)


def load_config(path: Path | None = None) -> dict[str, Any]:
    """载入并做最小结构校验，避免阈值散落在代码里。"""
    p = Path(path) if path else DEFAULT_CONFIG
    cfg = json.loads(p.read_text(encoding="utf-8"))
    missing = [k for k in REQUIRED_SECTIONS if k not in cfg]
    if missing:
        raise ValueError(f"{p} 缺少配置段: {', '.join(missing)}")
    if cfg["suite"] != "T0":
        raise ValueError(f"{p} 的 suite 应为 T0，实际为 {cfg['suite']!r}")
    if set(cfg["nodes"]) != {"A", "B"}:
        raise ValueError(f"{p} 的 nodes 必须恰好为 A / B 两个实例")
    return cfg


def load_cases(path: Path | None = None) -> dict[str, Any]:
    p = Path(path) if path else HERE / "t0_cases.json"
    data = json.loads(p.read_text(encoding="utf-8"))
    ids = [c["id"] for c in data["cases"]]
    if len(ids) != len(set(ids)):
        raise ValueError(f"{p} 存在重复用例 id")
    return data


@dataclass(frozen=True)
class Fixture:
    """确定性样本。infohash_stub 是合成值，不是真实 .torrent 的 InfoHash。"""

    path: Path
    size: int
    sha256: str
    infohash_stub: str
    target_key: str
    keyword_keys: tuple[str, ...]
    keywords: tuple[str, ...]

    def as_dict(self) -> dict[str, Any]:
        return {
            "file": self.path.relative_to(ROOT).as_posix(),
            "size_bytes": self.size,
            "sha256": self.sha256,
            "infohash_stub": self.infohash_stub,
            "target_key": self.target_key,
            "keyword_keys": list(self.keyword_keys),
            "infohash_is_real": False,
        }


class Harness:
    """一次 T0 执行的生命周期。real 模式才做真实端口/进程操作。"""

    def __init__(self, cfg: dict[str, Any], mode: str = "stub", keep: bool = False):
        self.cfg = cfg
        self.mode = mode
        self.keep = keep
        self.workdir = ROOT / cfg["paths"]["workdir"]
        self.sample_dir = ROOT / cfg["paths"]["sample_dir"]
        self.report_dir = ROOT / cfg["paths"]["reports"]
        self.golden_dir = ROOT / cfg["paths"]["golden_dir"]
        self.procs: dict[str, subprocess.Popen] = {}
        self.logs: list[Any] = []
        self.fixture: Fixture | None = None

    # ---------- 准备 ----------

    def prepare(self) -> Fixture:
        # 明确限定可清理目录，避免误删项目内其他内容
        if self.workdir.name != ".run" or self.workdir.parent != HERE:
            raise RuntimeError(f"拒绝清理非预期工作目录: {self.workdir}")
        if self.workdir.exists():
            shutil.rmtree(self.workdir)
        self.workdir.mkdir(parents=True, exist_ok=True)
        self.sample_dir.mkdir(parents=True, exist_ok=True)
        self.report_dir.mkdir(parents=True, exist_ok=True)
        self.fixture = self._make_fixture()
        return self.fixture

    def _make_fixture(self) -> Fixture:
        f = self.cfg["fixture"]
        ns = self.cfg["protocol"]["namespace"]
        data = random.Random(int(f["seed"])).randbytes(int(f["size_bytes"]))
        path = self.sample_dir / f["filename"]
        path.write_bytes(data)
        file_hash = hashlib.sha256(data).hexdigest()
        keywords = tuple(str(k) for k in f["keywords"])
        return Fixture(
            path=path,
            size=len(data),
            sha256=file_hash,
            infohash_stub=hashlib.sha1(b"T0-STUB:" + data[:1024]).hexdigest(),
            target_key=hashlib.sha1((ns + file_hash).encode("utf-8")).hexdigest(),
            keyword_keys=tuple(
                hashlib.sha1((ns + "kw:" + k).encode("utf-8")).hexdigest()
                for k in keywords
            ),
            keywords=keywords,
        )

    def context(self) -> dict[str, Any]:
        """交给 adapter 与断言引擎的共享上下文。"""
        assert self.fixture is not None, "必须先调用 prepare()"
        return {
            "cfg": self.cfg,
            "mode": self.mode,
            "fixture": self.fixture,
            "bind": self.cfg["bind"],
            "golden": self.golden(),
            "workdir": self.workdir,
        }

    def golden(self) -> dict[str, dict[str, Any]]:
        out: dict[str, dict[str, Any]] = {}
        for name in ("wakeup_req.v1.json", "wakeup_ack.v1.json"):
            p = self.golden_dir / name
            out[name] = json.loads(p.read_text(encoding="utf-8")) if p.is_file() else {}
        return out

    # ---------- 端口与进程 ----------

    def ports(self) -> list[tuple[str, int, int]]:
        """返回 (标签, 端口, socket 类型)；wake 端口按 UDP 检查。"""
        out: list[tuple[str, int, int]] = []
        for node, spec in self.cfg["nodes"].items():
            for key in ("http_port", "wake_udp_port", "bt_port"):
                kind = socket.SOCK_DGRAM if key == "wake_udp_port" else socket.SOCK_STREAM
                out.append((f"{node}.{key}", int(spec[key]), kind))
        return out

    def check_ports(self) -> list[str]:
        problems: list[str] = []
        for label, port, kind in self.ports():
            s = socket.socket(socket.AF_INET, kind)
            try:
                s.bind((self.cfg["bind"], port))
            except OSError as exc:
                problems.append(f"{label}={port} 不可绑定: {exc}")
            finally:
                s.close()
        return problems

    def render_argv(self, template: list[str], **values: str) -> list[str]:
        return [part.format(**values) for part in template]

    def spawn(self, name: str, argv: list[str]) -> subprocess.Popen:
        log = (self.workdir / f"{name}.log").open("wb")
        self.logs.append(log)
        proc = subprocess.Popen(argv, stdout=log, stderr=subprocess.STDOUT, cwd=str(self.workdir))
        self.procs[name] = proc
        (self.workdir / f"{name}.pid").write_text(str(proc.pid), encoding="utf-8")
        return proc

    def wait_health(self, url: str, timeout: float) -> bool:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                with urllib.request.urlopen(url, timeout=0.5) as resp:
                    if getattr(resp, "status", 0) == 200:
                        return True
            except Exception:
                pass
            time.sleep(0.2)
        return False

    def stop_all(self) -> list[str]:
        """终止本 harness 启动的进程；返回未正常退出的标签。"""
        stuck: list[str] = []
        grace = float(self.cfg["timeouts_sec"]["teardown"])
        for name, proc in self.procs.items():
            if proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=grace)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait(timeout=grace)
                    stuck.append(name)
        self.procs.clear()
        for fh in self.logs:
            try:
                fh.close()
            except Exception:
                pass
        self.logs.clear()
        return stuck

    def cleanup(self) -> None:
        self.stop_all()
        if not self.keep and self.workdir.exists():
            shutil.rmtree(self.workdir)


def self_check(config_path: Path | None = None, cases_path: Path | None = None) -> int:
    """不依赖任何二进制的环境自检：配置、样本确定性、golden 文件。"""
    cfg = load_config(config_path)
    cases = load_cases(cases_path)
    h = Harness(cfg, mode="stub")
    f1 = h.prepare()
    again = h._make_fixture()
    ok = True
    print(f"配置段: {len(REQUIRED_SECTIONS)} 项齐全 | 用例: {len(cases['cases'])} 条")
    print(f"样本: {f1.size} 字节 | SHA-256 {f1.sha256}")
    print(f"确定复算一致: {f1.sha256 == again.sha256}")
    print(f"Target_Key: {f1.target_key}")
    print(f"Keyword_Key 数量: {len(f1.keyword_keys)} (PROTO 上限 8)")
    print(f"golden 文件: {sorted(k for k, v in h.golden().items() if v)}")
    ok &= f1.sha256 == again.sha256
    ok &= len(f1.keyword_keys) <= 8
    ok &= all(h.golden().values())
    print("端口预检（stub 模式仅供参考）:")
    for problem in h.check_ports():
        print("  " + problem)
    h.cleanup()
    print("[OK] harness 自检通过" if ok else "[FAIL] harness 自检未通过")
    return 0 if ok else 1


def main() -> int:
    ap = argparse.ArgumentParser(description="ColdRevive T0 harness 自检")
    ap.add_argument("--self-check", action="store_true", help="校验配置、样本确定性与 golden 文件")
    ap.add_argument("--config", type=Path, default=None)
    ap.add_argument("--cases", type=Path, default=None)
    args = ap.parse_args()
    if not args.self_check:
        ap.print_help()
        return 0
    try:
        return self_check(args.config, args.cases)
    except Exception as exc:
        print(f"[ERROR] {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
