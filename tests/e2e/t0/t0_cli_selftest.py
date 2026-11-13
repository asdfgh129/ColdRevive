#!/usr/bin/env python3
"""t0_cli.py 观测层自检：证明真实采集逻辑不是空壳，且绝不合成观测值。

这不是 G1 验收，也不写报告、不改 wired。它用临时目录里的一个假 CLI 验证四件事：

1. 真实 .torrent 的 info 字典 SHA-1 计算正确（替换 infohash_stub 的前提）；
2. 按 CLI v1 契约能取回真实 JSON 并映射成观测字段；
3. 字段缺失时留空（由断言引擎判 FAIL），绝不回填「看起来像通过」的默认值；
4. 二进制缺失时抛 CliUnavailableError，绝不降级为 stub。

    python3 tests/e2e/t0/t0_cli_selftest.py
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import t0_cli  # noqa: E402
from t0_harness import Harness, load_config  # noqa: E402

FAKE_CLI = '''#!/usr/bin/env python3
import sys, json, os
a = sys.argv[1:]
i = a.index("--node")
cmd, args = a[i + 2], a[i + 3:]
if cmd == "inspect" and args[0] == "torrent":
    print(json.dumps({"file": {"torrent_path": os.environ["T0_FAKE_TORRENT"]}}))
elif cmd == "wake" and args[0] == "probe":
    print(json.dumps({"req": {"valid": True, "challenge_ok": True,
                              "nonce_unique": True, "time_window_ok": True}}))
else:
    print(json.dumps({}))
'''


def run() -> int:
    tmp = Path(tempfile.mkdtemp(prefix="t0-cli-selftest-"))
    checks: list[tuple[str, bool, str]] = []
    harness: Harness | None = None
    old_cli = os.environ.get("COLDREVIVE_CLI")
    try:
        # 1. 真实 bencode 伴随种子 → InfoHash
        info = b"d4:name4:test6:lengthi5ee"
        torrent = tmp / "sample.torrent"
        torrent.write_bytes(b"d4:info" + info + b"e")
        expected = hashlib.sha1(info).hexdigest()
        got = t0_cli.torrent_infohash(torrent)
        checks.append(("伴随种子 InfoHash 计算", got == expected,
                       f"{got} == {expected}"))

        # 2/3. 临时 CLI 按契约返回 JSON
        cli = tmp / "fake-cli.py"
        cli.write_text(FAKE_CLI, encoding="utf-8")
        cli.chmod(0o755)
        os.environ["COLDREVIVE_CLI"] = str(cli)
        os.environ["T0_FAKE_TORRENT"] = str(torrent)

        cfg = load_config()
        if os.name == "nt":
            # Windows 不能直接执行带 shebang 的 .py 脚本（WinError 193），
            # 自检改用当前解释器显式启动假 CLI；只影响本自检，不改产品 CLI v1 契约。
            cfg["launch"]["cli_argv"] = [sys.executable] + list(cfg["launch"]["cli_argv"])
        harness = Harness(cfg)
        harness.prepare()
        ctx = harness.context()
        ctx["harness"] = harness

        obs = t0_cli.collect("T0-04", ctx)
        keys = ("wake_req_valid", "challenge_ok", "nonce_unique", "time_window_ok")
        ok2 = obs.source == "cli" and all(obs.values.get(k) is True for k in keys)
        checks.append(("CLI 调用与字段映射", ok2, f"values={obs.values}"))

        empty = t0_cli.collect("T0-05", ctx)
        checks.append(("字段缺失不合成", not empty.values, f"values={empty.values}"))

        # 4. 二进制缺失 → 异常
        del os.environ["COLDREVIVE_CLI"]
        try:
            t0_cli.collect("T0-04", ctx)
            checks.append(("二进制缺失必须抛异常", False, "未抛异常"))
        except t0_cli.CliUnavailableError as exc:
            checks.append(("二进制缺失必须抛异常", True, str(exc)[:80]))

        # 5. 20 条映射齐全；M1 代码层已接线 (wired=True)，但环境级防呆必须仍生效：
        #    本机缺任一真实二进制时，real 观测仍须抛 NotWiredError（CI 保持 BLOCKED）。
        import t0_adapters
        checks.append(("观测映射 20 条齐全", len(t0_cli.OBSERVE_PLAN) == 20,
                       f"size={len(t0_cli.OBSERVE_PLAN)}"))
        checks.append(("M1 代码层已接线", t0_adapters.CliAdapter.wired is True,
                       f"wired={t0_adapters.CliAdapter.wired}"))
        saved = {env: os.environ.pop(env, None) for env in cfg["bin"].values()}
        try:
            adapter = t0_adapters.CliAdapter()
            try:
                adapter.observe("T0-04", ctx)
                checks.append(("环境级防呆：二进制缺失仍 BLOCKED", False, "未抛 NotWiredError"))
            except t0_adapters.NotWiredError as exc:
                checks.append(("环境级防呆：二进制缺失仍 BLOCKED", True, str(exc)[:80]))
        finally:
            for env, value in saved.items():
                if value is not None:
                    os.environ[env] = value
    finally:
        if harness is not None:
            harness.cleanup()
        if old_cli is None:
            os.environ.pop("COLDREVIVE_CLI", None)
        else:
            os.environ["COLDREVIVE_CLI"] = old_cli
        os.environ.pop("T0_FAKE_TORRENT", None)
        shutil.rmtree(tmp, ignore_errors=True)

    print("t0_cli 观测层自检（不是 G1 验收，不写报告、不改 wired）")
    print("-" * 72)
    for label, ok, detail in checks:
        print(f"[{'OK' if ok else 'FAIL'}] {label} — {detail}")
    failed = [label for label, ok, _ in checks if not ok]
    print("-" * 72)
    print(f"共 {len(checks)} 项，失败 {len(failed)} 项")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(run())
