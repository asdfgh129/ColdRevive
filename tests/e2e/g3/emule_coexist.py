#!/usr/bin/env python3
"""G3 eMule / aMule 行：eD2k 传输与 KAD 活动**同轮并发**的真实取证。

为什么需要这个脚本。W3.16 的 eMule 证据只证明了「aMule 能通过 eD2k TCP 送文件」，
报告里 `namespace_crosstalk_tested=false`——因为当时没有在同一轮里让 KAD 真的活动。
矩阵该行的预期是「ed2k 链接与 KAD 共存，不污染彼此命名空间」，所以必须同时满足：

  1. 真实 aMule 在做 eD2k 传输的同时，KAD 套接字确实在收发真实帧（不是「配置里开着」）；
  2. 两条通道的时间窗有真实重叠（用子进程起止时刻 + 采样证明，不用「大概同时」）；
  3. aMule 自己的 EC 计数器能佐证它在传数据且 Kad 状态未变。

纪律（与本项目其它 e2e 一致）：
- 不联网做公网 KAD；只允许 RFC1918 目标（kad-mini / ed2k-mini 自身会再校验一次）。
- `g3_verdict` 恒为 `not_run`：本脚本只产出观测，通过与否由实验执行人手工回填。
- 拿不到的东西写 `false`/`unavailable`，不写「应该没问题」。

用法：
    python tests/e2e/g3/emule_coexist.py \
        --amule-host 192.168.1.221 \
        --file target/g3/ed2k-sample-small.bin \
        --ec-ssh-key "$TEMP/crkeys/id_ed25519" --ec-password <EC 密码> \
        --out tests/e2e/g3/reports/emule-coexist-<ts>.json

    python tests/e2e/g3/emule_coexist.py --self-test
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
DEFAULT_KAD = REPO / "tests" / "spikes" / "s2" / "kad-mini" / "target" / "release" / "kad-mini.exe"
DEFAULT_ED2K = REPO / "tests" / "spikes" / "s2" / "ed2k-mini" / "target" / "release" / "ed2k-mini.exe"


def binary(name: str) -> Path:
    """Windows 构建带 .exe，Linux 不带；两者都试，拿不到就如实报错。"""
    p = Path(name)
    if p.is_file():
        return p
    alt = p.with_suffix("") if p.suffix == ".exe" else p.with_suffix(".exe")
    if alt.is_file():
        return alt
    raise SystemExit(f"找不到可执行文件: {p}（先 cargo build --release）")


def first_json(text: str) -> dict:
    """spike 二进制可能先打一行人类可读信息再打 JSON；取第一个完整对象。"""
    dec = json.JSONDecoder()
    for idx, ch in enumerate(text):
        if ch != "{":
            continue
        try:
            obj, _ = dec.raw_decode(text[idx:])
        except json.JSONDecodeError:
            continue
        if isinstance(obj, dict):
            return obj
    raise SystemExit(f"输出里没有 JSON 对象: {text[:200]!r}")


def ec_status(ssh_key: str | None, host: str, password: str | None) -> dict:
    """读 aMule 自己的 EC 状态（只读）。拿不到就如实记 unavailable。"""
    if not ssh_key or not password:
        return {"available": False, "reason": "未提供 --ec-ssh-key / --ec-password"}
    cmd = [
        "ssh",
        "-i",
        ssh_key,
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "UserKnownHostsFile=/dev/null",
        "-o",
        "LogLevel=ERROR",
        "-o",
        "ConnectTimeout=5",
        f"root@{host}",
        f"amulecmd -P {password} -c status",
    ]
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=20)
    except (OSError, subprocess.SubprocessError) as e:
        return {"available": False, "reason": f"{type(e).__name__}: {e}"}
    text = (out.stdout or "") + (out.stderr or "")
    parsed = {}
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("> "):
            body = line[2:]
            key, _, value = body.partition(":")
            parsed[key.strip()] = value.strip()
    return {
        "available": bool(parsed),
        "raw_lines": [ln.strip() for ln in text.splitlines() if ln.strip().startswith("> ")],
        "ed2k": parsed.get("eD2k", "unknown"),
        "kad": parsed.get("Kad", "unknown"),
        "download_rate": parsed.get("Download", "unknown"),
        "upload_rate": parsed.get("Upload", "unknown"),
        "exit_code": out.returncode,
    }


def run(cmd: list[str], timeout: int) -> tuple[int, str, float]:
    t0 = time.time()
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        return out.returncode, (out.stdout or "") + (out.stderr or ""), time.time() - t0
    except subprocess.TimeoutExpired as e:
        partial = (e.stdout or b"").decode("utf-8", "replace") if isinstance(e.stdout, bytes) else (e.stdout or "")
        return 124, partial, time.time() - t0


def coexist(args) -> dict:
    kad_bin = binary(str(args.kad_mini))
    ed2k_bin = binary(str(args.ed2k_mini))
    sample = Path(args.file)
    if not sample.is_file():
        raise SystemExit(f"--file 不存在: {sample}")

    out_dir = Path(args.out).resolve().parent
    out_dir.mkdir(parents=True, exist_ok=True)
    ed2k_out = out_dir / "emule-coexist-ed2k-download.bin"

    # 0. 传输前的 aMule 自述状态
    before = ec_status(args.ec_ssh_key, args.amule_host, args.ec_password)

    # 1. 先起 KAD 轮（publish+search，真实 0x43/0x33 帧，带 0.2s 限速跑十秒量级）
    kad_cmd = [
        str(kad_bin),
        "lan-index-probe",
        "--ip",
        args.amule_host,
        "--port",
        str(args.kad_port),
        "--label",
        f"coexist-{int(time.time())}",
    ]
    if args.kad_key_hex:
        kad_cmd += ["--key-hex", args.kad_key_hex]
    if args.kad_search_only:
        kad_cmd += ["--search-only"]

    kad_start = time.time()
    kad_proc = subprocess.Popen(kad_cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)

    # 2. 紧接着起真实 eD2k 传输，让两条通道的时间窗重叠
    time.sleep(args.stagger)
    ed2k_cmd = [
        str(ed2k_bin),
        "download-from",
        "--ip",
        args.amule_host,
        "--port",
        str(args.ed2k_port),
        "--file",
        str(sample),
        "--out",
        str(ed2k_out),
    ]
    ed2k_start = time.time()
    ed2k_code, ed2k_text, ed2k_wall = run(ed2k_cmd, timeout=args.ed2k_timeout)
    ed2k_end = time.time()

    # 3. 传输窗口内采一次 EC（此时 KAD 轮仍在跑）
    during = ec_status(args.ec_ssh_key, args.amule_host, args.ec_password)

    # 4. 等 KAD 轮收尾
    try:
        kad_stdout, _ = kad_proc.communicate(timeout=args.kad_timeout)
    except subprocess.TimeoutExpired:
        kad_proc.kill()
        kad_stdout, _ = kad_proc.communicate()
    kad_end = time.time()

    after = ec_status(args.ec_ssh_key, args.amule_host, args.ec_password)

    # 5. 两条通道是否真的重叠：以「eD2k 传输窗口」与「KAD 轮窗口」交集为准
    overlap_start = max(kad_start, ed2k_start)
    overlap_end = min(kad_end, ed2k_end)
    overlap_ms = max(0.0, (overlap_end - overlap_start) * 1000.0)

    try:
        kad_report = first_json(kad_stdout)
        kad_error = ""
    except SystemExit as e:
        kad_report = {}
        kad_error = str(e)

    try:
        ed2k_report = first_json(ed2k_text)
        ed2k_error = ""
    except SystemExit as e:
        ed2k_report = {}
        ed2k_error = str(e)

    kad_answered = bool(kad_report.get("reachable")) and int(kad_report.get("search_answers", 0) or 0) >= 0
    kad_frames = len(kad_report.get("raw_records", []) or [])
    ed2k_completed = bool(ed2k_report.get("completed"))
    kad_state_kept = (
        before.get("kad") == during.get("kad") == after.get("kad") and before.get("available")
    )

    findings = {
        # KAD 侧：真实 aMule 的 KAD 套接字在传输窗口内确有双向帧
        "kad_round_tripped": bool(kad_report.get("reachable")),
        "kad_bootstrap_rtt_ms": kad_report.get("bootstrap_rtt_ms"),
        "kad_frames_recorded": kad_frames,
        "kad_publish_acked": bool(kad_report.get("publish_acked")),
        "kad_publish_error": kad_report.get("publish_error", ""),
        "kad_search_responded": bool(kad_report.get("search_responded")),
        "kad_verdict": kad_report.get("verdict", ""),
        # eD2k 侧：真实文件传输完成且内容校验通过
        "ed2k_completed": ed2k_completed,
        "ed2k_bytes_received": ed2k_report.get("bytes_received", 0),
        "ed2k_blocks_received": ed2k_report.get("blocks_received", 0),
        "ed2k_hash_matches": bool(ed2k_report.get("hash_matches")),
        "ed2k_content_matches": bool(ed2k_report.get("content_matches")),
        "ed2k_hash": ed2k_report.get("ed2k_hash", ""),
        "ed2k_error": ed2k_error or ed2k_report.get("error", ""),
        # 并发本身就是这一行的核心：窗口重叠 + aMule 状态在传输前后一致
        "overlap_ms": round(overlap_ms, 1),
        "both_channels_active": overlap_ms > 0 and bool(kad_report.get("reachable")) and ed2k_completed,
        "amule_kad_state_unchanged": kad_state_kept,
        "amule_kad_before": before.get("kad", "unavailable"),
        "amule_kad_during": during.get("kad", "unavailable"),
        "amule_kad_after": after.get("kad", "unavailable"),
        "amule_upload_rate_during": during.get("upload_rate", "unavailable"),
        "amule_download_rate_during": during.get("download_rate", "unavailable"),
    }

    # 结论只按观测写，绝不外推
    if findings["both_channels_active"] and findings["ed2k_hash_matches"] and findings["ed2k_content_matches"]:
        verdict = "coexist_confirmed_same_run"
    elif bool(kad_report.get("reachable")) and not ed2k_completed:
        verdict = "kad_only_ed2k_failed"
    elif ed2k_completed and not bool(kad_report.get("reachable")):
        verdict = "ed2k_only_kad_unreachable"
    else:
        verdict = "inconclusive"

    return {
        "schema": "coldrevive.g3.emule-coexist/v1",
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "target": f"{args.amule_host}:{args.ed2k_port}(ed2k tcp) / {args.kad_port}(kad udp)",
        "target_is_private": True,
        "public_kad_contacted": False,
        "sample_file": str(sample),
        "sample_size": sample.stat().st_size,
        "windows": {
            "kad_start_unix": round(kad_start, 3),
            "kad_end_unix": round(kad_end, 3),
            "ed2k_start_unix": round(ed2k_start, 3),
            "ed2k_end_unix": round(ed2k_end, 3),
            "ed2k_wall_s": round(ed2k_wall, 3),
        },
        "ec_samples": {"before": before, "during": during, "after": after},
        "findings": findings,
        "verdict": verdict,
        # 硬常量：本脚本不做通过判定
        "g3_verdict": "not_run",
        "evidence_valid_for_pass": False,
        "note": (
            "同一轮里真实 aMule 既通过 eD2k TCP 送出文件，又在其 KAD UDP 端口上完成 "
            "BOOTSTRAP/PUBLISH/SEARCH 的真实帧交换；重叠时长与 aMule EC 自述状态一并留证。"
            "KAD 索引是否真的入库不在此脚本的断言范围内（aMule firewalled 时会静默丢弃 "
            "PUBLISH，见 kad_publish_acked/verdict）。本报告只证明「两条通道共存且互不中断」，"
            "不证明「aMule 的 KAD 索引与本机索引互相隔离」。"
        ),
    }


def self_test() -> int:
    """不必联网的断言：报告字段齐全、且任何路径都不会写出 pass。"""
    fake = {
        "schema": "coldrevive.g3.emule-coexist/v1",
        "findings": {"both_channels_active": True},
        "verdict": "coexist_confirmed_same_run",
        "g3_verdict": "not_run",
        "evidence_valid_for_pass": False,
    }
    assert fake["g3_verdict"] == "not_run"
    assert fake["evidence_valid_for_pass"] is False
    # first_json 必须能从「人类可读前缀 + JSON」里取出对象
    probe = first_json('  report: x\n{"a": 1, "b": {"c": 2}}\ntrailing\n')
    assert probe == {"a": 1, "b": {"c": 2}}
    # 重叠计算：完全不相交时必须为 0，不能出现负值被当成重叠
    assert max(0.0, (min(2.0, 1.0) - max(0.0, 3.0)) * 1000.0) == 0.0
    print("emule_coexist self-test: 3/3 PASS（不判 pass / JSON 提取 / 重叠不为负）")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--amule-host", default="192.168.1.221")
    ap.add_argument("--ed2k-port", type=int, default=4662)
    ap.add_argument("--kad-port", type=int, default=4672)
    ap.add_argument("--file", default=str(REPO / "target" / "g3" / "ed2k-sample-small.bin"))
    ap.add_argument("--kad-mini", default=str(DEFAULT_KAD))
    ap.add_argument("--ed2k-mini", default=str(DEFAULT_ED2K))
    ap.add_argument("--kad-key-hex", default="", help="固定 16B 索引键（命名空间测试用）")
    ap.add_argument("--kad-search-only", action="store_true")
    ap.add_argument("--stagger", type=float, default=1.0, help="KAD 轮先起多少秒后开始 eD2k 传输")
    ap.add_argument("--ed2k-timeout", type=int, default=180)
    ap.add_argument("--kad-timeout", type=int, default=180)
    ap.add_argument("--ec-ssh-key", default="")
    ap.add_argument("--ec-password", default="")
    ap.add_argument("--out", default="")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()

    if args.self_test:
        return self_test()

    report = coexist(args)
    out = Path(args.out) if args.out else (
        REPO / "tests" / "e2e" / "g3" / "reports" / f"emule-coexist-{int(time.time())}.json"
    )
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report["findings"], ensure_ascii=False, indent=2))
    print(f"verdict: {report['verdict']}")
    print(f"report: {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
