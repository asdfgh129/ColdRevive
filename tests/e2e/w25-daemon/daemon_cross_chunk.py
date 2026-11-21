#!/usr/bin/env python3
"""FR-25 daemon wiring + >PARTSIZE cross-chunk evidence against real aMule.

Drives `coldrevived` over its real HTTP /cli surface:
  ed2k-link -> ed2k-serve -> (real aMule downloads from us) -> ed2k-download
The >PARTSIZE sample forces OP_HASHSETREQUEST / OP_HASHSETANSWER and the
Red exact-boundary part-hash rule. Every check reads real files, real JSON
results, or real ssh/md5 output. The script never changes gate constants.

Usage:
  python3 tests/e2e/w25-daemon/daemon_cross_chunk.py
"""
from __future__ import annotations

import hashlib
import json
import os
import shlex
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
RUN = ROOT / "tests" / "e2e" / "w25-daemon" / "run"
REPORTS = ROOT / "tests" / "e2e" / "w25-daemon" / "reports"
PART_SIZE = 9_500 * 1024
SAMPLE_BYTES = int(os.environ.get("FR25_SAMPLE_BYTES", str(PART_SIZE + 8192)))
OUR_LAN = "192.168.1.210"          # A 的 LAN 地址，221 能经它连到我们的 serve 端口
AMULE_HOST = "192.168.1.221"
AMULE_ED2K = "192.168.1.221:4662"
AMULE_INCOMING = "/opt/g3-clients/amule-config/Incoming"
SERVE_PORT = 4663
KEY = os.path.join(os.environ.get("TEMP", "/tmp"), "crkeys", "id_ed25519")


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def free_port() -> int:
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


def free_udp_port() -> int:
    # Windows 的排除端口段会让「TCP 空闲端口」在 UDP 上绑定失败（10013）。
    # 这里直接以 UDP socket 选端口，和守护进程实际绑定方式一致。
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]
    finally:
        s.close()


def http_json(url: str, payload=None, timeout: float = 10.0):
    data = None if payload is None else json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        url, data=data,
        headers={"Content-Type": "application/json"} if data else {},
    )
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode("utf-8"))


def cli(url: str, command: str, *args: str):
    return http_json(url, {"command": command, "args": list(args)}, timeout=30.0)


def rpc(url: str, method: str, params):
    return http_json(url, {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}, timeout=30.0)


def wait_health(url: str, seconds: float = 20.0) -> bool:
    until = time.time() + seconds
    while time.time() < until:
        try:
            with urllib.request.urlopen(url, timeout=1.0) as r:
                if r.status == 200:
                    return True
        except Exception:
            time.sleep(0.2)
    return False


def ssh(*cmd: str, timeout: int = 60) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["ssh", "-i", KEY, "-o", "IdentitiesOnly=yes", "-o", "ConnectTimeout=8",
         "-o", "BatchMode=yes", "root@" + AMULE_HOST, *[shlex.quote(c) for c in cmd]],
        capture_output=True, text=True, timeout=timeout,
    )


def poll_task(url: str, task_id: str, seconds: float):
    until = time.time() + seconds
    last = None
    while time.time() < until:
        last = cli(url, "ed2k-status", "--task", task_id)
        if last.get("status") in ("complete", "error"):
            return last
        time.sleep(0.5)
    return last


def main() -> int:
    RUN.mkdir(parents=True, exist_ok=True)
    REPORTS.mkdir(parents=True, exist_ok=True)
    stamp = time.strftime("%Y%m%d%H%M%S")
    name = "w25-cross-%s.bin" % stamp
    sample = RUN / name
    # 每轮换 seed：aMule 按 fileID 去重，若两轮内容相同会被当成「已在列表」，
    # 这不是网络结论。stamp 只影响载荷，不改变 >PARTSIZE / 跨分片语义。
    # 只取末两位会每 100 分钟重复一次 fileID，aMule 按 fileID 去重会把新名
    # 当成同一个文件；用完整时间戳取模，保证连续多轮内容/ID 都不同。
    seed_offset = int(stamp) % 251
    with open(sample, "wb") as f:
        # 确定性但非重复载荷，避免压缩/稀疏文件把 hashset 测试变成假阳性
        f.write(bytes(((i * 91 + seed_offset + 7) % 256) for i in range(SAMPLE_BYTES)))
    sample_sha = sha256(sample)

    report = {
        "schema": "coldrevive.fr25.daemon-cross-chunk/v1",
        "generated_at_unix": int(time.time()),
        "scope": "RFC1918 LAN, real aMule 2.3.3, daemon /cli + /jsonrpc",
        "sample": {"name": name, "bytes": SAMPLE_BYTES, "sha256": sample_sha,
                   "part_size": PART_SIZE, "above_part_size": SAMPLE_BYTES > PART_SIZE},
        "checks": [],
    }
    procs = []
    logf = None
    try:
        daemon = os.environ.get("COLDREVIVE_DAEMON") or str(
            ROOT / ("target/release/coldrevived.exe" if os.name == "nt" else "target/release/coldrevived")
        )
        nodes = {"B": {"http_port": free_port(), "wake_udp_port": free_udp_port(), "bt_port": free_port()}}
        cfg = {
            "bind": "127.0.0.1", "nodes": nodes, "sample_path": str(sample),
            "keywords": ["w25-daemon"], "namespace": "ColdRevive_V1:",
            "workdir": str(RUN), "golden_req": str(RUN / "req.json"),
            "golden_ack": str(RUN / "ack.json"), "downloader": "",
            "challenge_window_sec": 300,
        }
        (RUN / "req.json").write_text("{}", encoding="utf-8")
        (RUN / "ack.json").write_text("{}", encoding="utf-8")
        cfg_path = RUN / ("config-%s.json" % stamp)
        cfg_path.write_text(json.dumps(cfg, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

        logf = open(RUN / ("daemon-%s.log" % stamp), "wb")
        procs.append(subprocess.Popen(
            [daemon, "--config", str(cfg_path), "--node-id", "B",
             "--bind", "127.0.0.1", "--workdir", str(RUN)],
            stdout=logf, stderr=subprocess.STDOUT, cwd=str(RUN),
        ))
        url = "http://127.0.0.1:%d" % nodes["B"]["http_port"]
        cli_url, rpc_url = url + "/cli", url + "/jsonrpc"
        if not wait_health(url + "/health"):
            report["status"] = "BLOCKED"
            report["reason"] = "daemon 健康检查未通过（见 run/daemon-*.log）"
            return 2
        report["checks"].append({"name": "daemon_health", "ok": True})

        # 1) link：fileID 必须由真实文件内容导出
        linked = cli(cli_url, "ed2k-link", "--file", str(sample), "--name", name)
        report["link"] = linked
        report["checks"].append({
            "name": "link_real_hash_and_hashset_required",
            "ok": bool(linked.get("file_id")) and linked.get("size") == SAMPLE_BYTES
                  and linked.get("hashset_required") is (SAMPLE_BYTES >= PART_SIZE),
        })
        link = linked.get("ed2k_link", "")
        if not link:
            report["status"] = "FAIL"
            report["reason"] = "ed2k-link 未产出链接: %r" % linked
            return 1

        # 2) serve：daemon 后台任务，真实 aMule 从我们这里取 >PARTSIZE 文件
        started = cli(cli_url, "ed2k-serve", "--file", str(sample), "--name", name,
                      "--port", str(SERVE_PORT), "--timeout-ms", "900000")
        serve_id = started.get("id", "")
        report["serve_started"] = started
        report["checks"].append({"name": "serve_task_registered", "ok": bool(serve_id)})
        if not serve_id:
            report["status"] = "FAIL"
            report["reason"] = "ed2k-serve 未登记任务: %r" % started
            return 1

        with_sources = link + "|sources,%s:%d|/" % (OUR_LAN, SERVE_PORT)
        add = ssh("amulecmd", "-p", "4712", "-P", "coldrevive", "-c", "Add " + with_sources)
        report["amule_add"] = {"rc": add.returncode, "stdout": add.stdout[-1000:], "stderr": add.stderr[-1000:]}
        # aMule 的 Add 在已存在记录时会失败；名字带时间戳，正常应 rc=0。
        report["checks"].append({"name": "amule_add_link", "ok": add.returncode == 0})

        # aMule 偶发不立刻尝试 link 里的 source（尤其是刚处理完/取消过同类任务）。
        # 15s 还没开始收字节就重下同一条 link，最多 2 次；只重试真实 Add 命令，
        # 不把重试本身写成成功证据。
        add_retries = []
        for attempt in range(2):
            time.sleep(15)
            probe = cli(cli_url, "ed2k-status", "--task", serve_id)
            if probe.get("status") != "running":
                break
            if (probe.get("result") or {}).get("bytes_sent", 0) > 0:
                break
            retry = ssh("amulecmd", "-p", "4712", "-P", "coldrevive", "-c", "Add " + with_sources)
            add_retries.append({"attempt": attempt + 1, "rc": retry.returncode,
                                "stdout": retry.stdout[-500:]})
        report["amule_add_retries"] = add_retries

        served = poll_task(cli_url, serve_id, 900.0)
        report["serve_result"] = served
        sr = (served or {}).get("result") or {}
        report["checks"].append({
            "name": "serve_bytes_sent_covers_sample",
            "ok": served is not None and served.get("status") == "complete"
                  and sr.get("bytes_sent", 0) >= SAMPLE_BYTES,
        })

        remote = ssh("stat", "-c", "%s", "%s/%s" % (AMULE_INCOMING, name))
        report["remote_size_ok"] = remote.returncode == 0 and remote.stdout.strip() == str(SAMPLE_BYTES)
        report["checks"].append({"name": "remote_file_size", "ok": report["remote_size_ok"]})
        rmd5 = ssh("md5sum", "%s/%s" % (AMULE_INCOMING, name), timeout=90)
        report["remote_md5"] = rmd5.stdout.strip()
        report["checks"].append({
            "name": "remote_md5_matches_local",
            "ok": rmd5.returncode == 0 and rmd5.stdout.split()[0] == hashlib.md5(sample.read_bytes()).hexdigest(),
        })

        # 3) download：从真 aMule 取回，必须走 hashset 且逐字节一致
        dstarted = cli(cli_url, "ed2k-download", "--file", str(sample), "--name", name,
                       "--host", AMULE_ED2K, "--timeout-ms", "900000")
        download_id = dstarted.get("id", "")
        report["download_started"] = dstarted
        report["checks"].append({"name": "download_task_registered", "ok": bool(download_id)})
        if not download_id:
            report["status"] = "FAIL"
            report["reason"] = "ed2k-download 未登记任务: %r" % dstarted
            return 1
        downloaded = poll_task(cli_url, download_id, 900.0)
        report["download_result"] = downloaded
        dr = (downloaded or {}).get("result") or {}
        report["checks"].append({
            "name": "hashset_verified",
            "ok": dr.get("hashset_verified") is (SAMPLE_BYTES >= PART_SIZE),
        })
        report["checks"].append({"name": "download_completed", "ok": dr.get("completed") is True})
        report["checks"].append({"name": "hash_and_content_match", "ok":
            dr.get("hash_matches") is True and dr.get("content_matches") is True})

        # 4) /jsonrpc 只读监控看到同一条真实任务
        mon = rpc(rpc_url, "coldrevive.ed2kStatus", [download_id])
        mresult = mon.get("result") or {}
        report["checks"].append({
            "name": "jsonrpc_status_exposes_same_task",
            "ok": mresult.get("id") == download_id and mresult.get("status") == (downloaded or {}).get("status"),
        })
        report["status"] = "PASS" if all(c["ok"] for c in report["checks"]) else "FAIL"
        return 0 if report["status"] == "PASS" else 1
    except Exception as exc:
        report["status"] = "FAIL"
        report["reason"] = "%s: %s" % (type(exc).__name__, exc)
        return 1
    finally:
        out = REPORTS / ("daemon-cross-chunk-%s.json" % stamp)
        out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print("report: %s" % out)
        print("status: %s (%d/%d checks)" % (
            report.get("status"), sum(1 for c in report["checks"] if c["ok"]), len(report["checks"])))
        for p in procs:
            p.terminate()
            try:
                p.wait(timeout=8)
            except subprocess.TimeoutExpired:
                p.kill()
        if logf:
            logf.close()


if __name__ == "__main__":
    sys.exit(main())
