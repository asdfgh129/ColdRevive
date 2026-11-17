#!/usr/bin/env python3
"""W5.1b 验收：引擎做种，**第三方标准客户端**下载（不再是自己人对自己人）。

为什么要单独一条：W5.1 的 `engine_download.py` 是「A 手写种子端 ↔ B librqbit」，
两套实现彼此独立，所以能互相印证。W5.1b 按 ADR-015 把手写种子端删掉、做种也交给
librqbit 之后，两端同库——那条测试就只能证明「库自己能互通」，不足以证明**标准协议互通**。
因此这里把下载方换成第三方 aria2，测的是「我们的引擎做种，外面的客户端能不能收」。

判「通过」四条缺一不可：
    ① COLD 对照：**唤醒前** aria2 拿不到完整数据（未唤醒就不供片，端口在听但暂停）
    ② 唤醒后 A 的引擎状态转 hot
    ③ 第三方 aria2 完整下载 1 MiB，落盘 SHA-256 与样本逐字节一致
    ④ 供片方对账：A 引擎自报 `served_bytes >= 样本字节数`

另有一条身份检查：A 的日志必须出现「做种引擎就绪」且**不**出现旧的手写种子端
——防止「改了代码但跑的是旧二进制」这类自欺。

用法：
    python tests/e2e/w51b/seed_to_thirdparty.py
    python tests/e2e/w51b/seed_to_thirdparty.py --keep

环境变量：
    COLDREVIVE_DAEMON  守护进程路径（缺省 target/debug/coldrevived(.exe)）
    COLDREVIVE_ARIA2   第三方 aria2c 路径（缺省 Windows 上 Rayburst 捆绑版，其次 PATH 里的 aria2c）
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
import tempfile
import time
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
SAMPLE_BYTES = 1 << 20  # 1 MiB
COLD_PROBE_SECONDS = 8   # 负对照：给 aria2 8 秒，未唤醒应当下不完


def free_port(kind: str = "tcp") -> int:
    if kind == "udp":
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.bind(("127.0.0.1", 0))
        p = s.getsockname()[1]
        s.close()
        return p
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


def deterministic_sample(n: int) -> bytes:
    out = bytearray()
    seed = b"coldrevive-w51b-sample"
    while len(out) < n:
        seed = hashlib.sha256(seed).digest()
        out.extend(seed * 8)
    return bytes(out[:n])


def http_json(url: str, payload: dict, timeout: float = 30.0) -> dict:
    body = json.dumps(payload).encode()
    req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode())


def http_bytes(url: str, timeout: float = 15.0) -> bytes:
    with urllib.request.urlopen(url, timeout=timeout) as r:
        return r.read()


def wait_health(url: str, timeout: float) -> bool:
    end = time.time() + timeout
    while time.time() < end:
        try:
            with urllib.request.urlopen(url, timeout=1.0) as r:
                if r.status == 200:
                    return True
        except Exception:
            pass
        time.sleep(0.1)
    return False


def kill_tree(proc: subprocess.Popen) -> None:
    if proc.poll() is not None:
        return
    if os.name == "nt":
        subprocess.run(["taskkill", "/F", "/T", "/PID", str(proc.pid)],
                       capture_output=True, check=False)
    else:
        proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()


def find_aria2() -> str:
    env = os.environ.get("COLDREVIVE_ARIA2", "").strip()
    if env:
        return env
    if os.name == "nt":
        cand = Path("C:/Program Files/Rayburst/aria2-next.exe")
        if cand.is_file():
            return str(cand)
    return shutil.which("aria2c") or ""


def run_aria2(aria2: str, torrent: Path, out_dir: Path, tracker: str,
              listen_port: int, timeout: float) -> dict:
    """跑一次 aria2c，返回它自己的退出码/日志/落盘文件大小。超时即杀掉并如实记录。"""
    out_dir.mkdir(parents=True, exist_ok=True)
    cmd = [
        aria2,
        f"--bt-tracker={tracker}",
        "--enable-dht=false", "--enable-dht6=false",
        "--enable-peer-exchange=false",
        "--bt-max-peers=8", "--bt-tracker-timeout=8", "--bt-tracker-connect-timeout=4",
        "--seed-time=0", "--check-integrity=true", "--allow-overwrite=true",
        "--file-allocation=none", "--summary-interval=0",
        "--console-log-level=notice",
        f"--dir={out_dir}",
        f"--listen-port={listen_port}",
        str(torrent),
    ]
    t0 = time.time()
    timed_out = False
    # 输出落文件而不是 PIPE：超时被杀时管道会关闭，再从 PIPE 读会 ValueError，
    # 而且被杀的进程还能留下可翻的日志。
    log_path = out_dir.parent / f"aria2-{out_dir.name}.log"
    with open(log_path, "wb") as lf:
        proc = subprocess.Popen(cmd, stdout=lf, stderr=subprocess.STDOUT)
        try:
            proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            kill_tree(proc)
    log = log_path.read_text(encoding="utf-8", errors="replace") if log_path.is_file() else ""
    files = [p for p in out_dir.iterdir() if p.is_file() and not p.name.endswith(".aria2")]
    total = sum(p.stat().st_size for p in files)
    return {
        "argv": cmd[1:],
        "exit_code": proc.returncode,
        "timed_out": timed_out,
        "elapsed_ms": int((time.time() - t0) * 1000),
        "bytes_on_disk": total,
        "log_tail": log[-1500:],
    }


def run(keep: bool) -> dict:
    daemon = os.environ.get("COLDREVIVE_DAEMON", "").strip()
    if not daemon:
        daemon = str(REPO / "target" / "debug" / ("coldrevived.exe" if os.name == "nt" else "coldrevived"))
    aria2 = find_aria2()
    if not Path(daemon).is_file():
        return {"status": "blocked", "reason": f"守护进程不存在: {daemon}"}
    if not aria2 or not Path(aria2).is_file():
        return {"status": "blocked", "reason": "找不到第三方 aria2c（设置 COLDREVIVE_ARIA2）"}

    work = Path(tempfile.mkdtemp(prefix="cr-w51b-"))
    sample = work / "w51b-sample.bin"
    data = deterministic_sample(SAMPLE_BYTES)
    sample.write_bytes(data)
    sample_sha = hashlib.sha256(data).hexdigest()

    nodes = {
        "A": {"http_port": free_port(), "wake_udp_port": free_port("udp"), "bt_port": free_port()},
        "B": {"http_port": free_port(), "wake_udp_port": free_port("udp"), "bt_port": free_port()},
    }
    cfg = {
        "bind": "127.0.0.1",
        "nodes": nodes,
        "sample_path": str(sample),
        "keywords": ["w51b"],
        "namespace": "ColdRevive_V1:",
        "workdir": str(work),
        "golden_req": str(work / "wakeup_req.v1.json"),
        "golden_ack": str(work / "wakeup_ack.v1.json"),
        "downloader": aria2,
        "challenge_window_sec": 300,
    }
    cfg_path = work / "w51b-nodes.json"
    cfg_path.write_text(json.dumps(cfg, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    report: dict = {
        "schema": "coldrevive.w51b.seed-to-thirdparty/v1",
        "generated_at_unix": int(time.time()),
        "seeder": "cr-engine / librqbit（A）",
        "downloader": f"third-party aria2c: {aria2}",
        "scope": "127.0.0.1 回环；DHT/PEX 关闭；tracker 为 A 的内置 /announce",
        "sample": {"bytes": SAMPLE_BYTES, "sha256": sample_sha},
        "nodes": nodes,
        "checks": [],
    }
    procs: list[subprocess.Popen] = []
    logs: list = []
    try:
        for node in ("A", "B"):
            lf = open(work / f"daemon-{node}.log", "wb")
            logs.append(lf)
            procs.append(subprocess.Popen(
                [daemon, "--config", str(cfg_path), "--node-id", node,
                 "--bind", "127.0.0.1", "--workdir", str(work)],
                stdout=lf, stderr=subprocess.STDOUT, cwd=str(work),
            ))
        for node in ("A", "B"):
            if not wait_health(f"http://127.0.0.1:{nodes[node]['http_port']}/health", 25.0):
                report["status"] = "fail"
                report["reason"] = f"{node} 健康检查未通过"
                return report

        daemon_a_log = (work / "daemon-A.log").read_text(encoding="utf-8", errors="replace")
        engine_seeder = "做种引擎就绪" in daemon_a_log
        legacy = "标准 BT 种子端监听" in daemon_a_log
        report["a_log_excerpt"] = [ln for ln in daemon_a_log.splitlines()
                                   if "做种引擎" in ln or "种子端" in ln]
        report["checks"].append({
            "label": "A 的做种由引擎承担（旧手写种子端已不在）",
            "ok": engine_seeder and not legacy,
            "actual": f"engine_ready={engine_seeder}, legacy_seeder={legacy}",
        })

        state = json.loads((work / "state-A.json").read_text(encoding="utf-8"))
        entry = next((e for e in (state.get("entries") or []) if e.get("infohash")), None)
        if not entry:
            report["status"] = "fail"
            report["reason"] = "state-A.json 里没有带 infohash 的条目"
            return report
        file_hash, infohash, name = entry["file_hash"], entry["infohash"], entry.get("name", "")
        report["torrent"] = {"file_hash": file_hash, "info_hash": infohash, "name": name}

        cli_a = f"http://127.0.0.1:{nodes['A']['http_port']}/cli"
        cli_b = f"http://127.0.0.1:{nodes['B']['http_port']}/cli"
        tracker = f"http://127.0.0.1:{nodes['A']['http_port']}/announce"

        # .torrent 走生产链路取回（和第三方客户端拿到的是同一份字节）
        raw = http_bytes(f"http://127.0.0.1:{nodes['A']['http_port']}/torrent/{infohash}")
        torrent_file = work / "w51b.torrent"
        torrent_file.write_bytes(raw)
        report["checks"].append({
            "label": "伴随种子可取回",
            "ok": len(raw) > 0,
            "actual": f"{len(raw)} B",
        })

        # ① 负对照：未唤醒时端口在听，但引擎处于暂停，aria2 应当拿不到完整数据
        cold_seed = (http_json(cli_a, {"command": "seed", "args": []}) or {}).get("seed") or {}
        report["a_cold"] = cold_seed
        cold_try = run_aria2(aria2, torrent_file, work / "cold-try", tracker,
                             free_port(), COLD_PROBE_SECONDS)
        report["cold_probe"] = cold_try
        report["checks"].append({
            "label": "COLD 对照：未唤醒时引擎处于非 hot 且未上传",
            "ok": (cold_seed.get("seeding_hot") is False
                   and cold_seed.get("state") != "hot"
                   and int(cold_seed.get("served_bytes") or 0) == 0),
            "actual": json.dumps(cold_seed, ensure_ascii=False),
        })
        report["checks"].append({
            "label": "COLD 对照：第三方客户端拿不完整（未唤醒不供片）",
            "ok": cold_try["bytes_on_disk"] < SAMPLE_BYTES,
            "actual": f"bytes_on_disk={cold_try['bytes_on_disk']} / {SAMPLE_BYTES}，"
                      f"timed_out={cold_try['timed_out']}，exit={cold_try['exit_code']}",
        })

        # ② 唤醒
        t0 = time.time()
        ack = http_json(cli_b, {"command": "wake", "args": ["ack", file_hash, infohash]})
        wake_ms = int((time.time() - t0) * 1000)
        ack_obj = (ack or {}).get("ack") or {}
        report["wake"] = {"elapsed_ms": wake_ms, "ack": ack_obj}
        after = (http_json(cli_a, {"command": "seed", "args": []}) or {}).get("seed") or {}
        report["a_after_wake"] = after
        report["checks"].append({
            "label": "唤醒后 A 的引擎转入 hot",
            "ok": after.get("state") == "hot" and bool(after.get("seeding_hot")),
            "actual": json.dumps(after, ensure_ascii=False),
        })

        # ③ 第三方客户端完整下载
        dl = run_aria2(aria2, torrent_file, work / "downloads", tracker, free_port(), 150.0)
        report["download"] = dl
        out = work / "downloads" / name
        exists = out.is_file()
        got = out.read_bytes() if exists else b""
        got_sha = hashlib.sha256(got).hexdigest() if exists else ""
        report["output"] = {"path": str(out), "bytes": len(got), "sha256": got_sha}
        report["checks"].append({
            "label": "第三方客户端下完且字节数正确",
            "ok": dl["exit_code"] == 0 and exists and len(got) == SAMPLE_BYTES,
            "actual": f"exit={dl['exit_code']}, bytes={len(got)}, timed_out={dl['timed_out']}",
        })
        report["checks"].append({
            "label": "落盘内容与样本逐字节一致",
            "ok": got_sha == sample_sha,
            "actual": f"{got_sha or '<未读到>'} vs {sample_sha}",
        })

        # ④ 供片方对账
        end_seed = (http_json(cli_a, {"command": "seed", "args": []}) or {}).get("seed") or {}
        report["a_after_download"] = end_seed
        report["checks"].append({
            "label": "A 引擎自报上传覆盖全量（供片方计数器）",
            "ok": int(end_seed.get("served_bytes") or 0) >= SAMPLE_BYTES,
            "actual": json.dumps(end_seed, ensure_ascii=False),
        })

        # ⑤ 幂等回归：重复唤醒不得把「已就绪」翻掉（librqbit 对 live 任务 unpause 会报错）
        for _ in range(3):
            http_json(cli_b, {"command": "wake", "args": ["ack", file_hash, infohash]})
        repeat = (http_json(cli_a, {"command": "seed", "args": []}) or {}).get("seed") or {}
        report["a_after_repeat_wake"] = repeat
        report["checks"].append({
            "label": "连发 3 次唤醒后仍处于 hot（激活幂等）",
            "ok": repeat.get("state") == "hot" and bool(repeat.get("seeding_hot")),
            "actual": json.dumps(repeat, ensure_ascii=False),
        })

        report["all_passed"] = all(c["ok"] for c in report["checks"])
        report["status"] = "pass" if report["all_passed"] else "fail"
        return report
    finally:
        for p in procs:
            kill_tree(p)
        for lf in logs:
            lf.close()
        if keep:
            report["workdir"] = str(work)
        else:
            shutil.rmtree(work, ignore_errors=True)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()
    rep = run(args.keep)
    out_dir = REPO / "tests" / "e2e" / "w51b" / "reports"
    out_dir.mkdir(parents=True, exist_ok=True)
    out = out_dir / f"seed-to-thirdparty-{time.strftime('%Y%m%dT%H%M%SZ', time.gmtime())}.json"
    out.write_text(json.dumps(rep, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    if args.json:
        print(json.dumps(rep, ensure_ascii=False, indent=2))
    else:
        print(f"状态: {rep.get('status')}  报告: {out}")
        for c in rep.get("checks", []):
            print(f"  [{'PASS' if c['ok'] else 'FAIL'}] {c['label']} — {c['actual']}")
        if rep.get("reason"):
            print(f"  原因: {rep['reason']}")
    return 0 if rep.get("status") == "pass" else 1


if __name__ == "__main__":
    sys.exit(main())
