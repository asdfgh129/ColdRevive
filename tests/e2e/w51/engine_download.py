#!/usr/bin/env python3
"""W5.1 验收：coldrevived 经 cr-engine（librqbit）真实下载并逐片校验。

链路（全在本机回环，不碰公网）：
    B（冷节点）--/cli wake--> A   ← A 由 COLD 转 HOT，内置标准 BT 种子端开始供片
    B --/jsonrpc aria2.addUri--> cr-engine ← 引擎按 tracker 返回的 peer 直连 A 拉分片
    B 落盘文件 → SHA-256 与样本逐字节比对

判「通过」必须三方同时对上（缺一条就判失败，不做「看起来差不多」）：
    ① 引擎计数器：`completedLength == 样本字节数` 且 `status == complete`
    ② 落盘内容：下载文件 SHA-256 == 样本 SHA-256
    ③ 供片方对账：A 的内置种子端 `served_bytes >= 样本字节数`（第二个计数器）

用法：
    python tests/e2e/w51/engine_download.py            # 需要先 cargo build -p coldrevived
    python tests/e2e/w51/engine_download.py --keep     # 保留临时目录便于翻证据

环境变量 `COLDREVIVE_DAEMON` 可指定二进制路径，缺省用 target/debug/coldrevived(.exe)。
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
import urllib.error
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
SAMPLE_BYTES = 1 << 20  # 1 MiB；PIECE_SIZE=262144 → 4 片


# ---------------------------------------------------------------------------
# 基础设施
# ---------------------------------------------------------------------------

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
    """确定性样本：可复现，不依赖 random 实现。"""
    out = bytearray()
    seed = b"coldrevive-w51-sample"
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


# ---------------------------------------------------------------------------
# 主流程
# ---------------------------------------------------------------------------

def run(keep: bool) -> dict:
    daemon = os.environ.get("COLDREVIVE_DAEMON", "").strip()
    if not daemon:
        cand = REPO / "target" / "debug" / ("coldrevived.exe" if os.name == "nt" else "coldrevived")
        daemon = str(cand)
    if not Path(daemon).is_file():
        return {"status": "blocked", "reason": f"二进制不存在: {daemon}"}

    work = Path(tempfile.mkdtemp(prefix="cr-w51-"))
    sample = work / "w51-sample.bin"
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
        "keywords": ["w51"],
        "namespace": "ColdRevive_V1:",
        "workdir": str(work),
        "golden_req": str(work / "wakeup_req.v1.json"),
        "golden_ack": str(work / "wakeup_ack.v1.json"),
        "downloader": "",
        "challenge_window_sec": 300,
    }
    cfg_path = work / "w51-nodes.json"
    cfg_path.write_text(json.dumps(cfg, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    procs: list[subprocess.Popen] = []
    log_files = []
    report: dict = {
        "schema": "coldrevive.w51.engine-download/v1",
        "generated_at_unix": int(time.time()),
        "engine": "librqbit 9.1.0 (cr-engine)",
        "scope": "127.0.0.1 回环；DHT 关闭；tracker 为 A 的内置 /announce",
        "sample": {"bytes": SAMPLE_BYTES, "sha256": sample_sha},
        "nodes": nodes,
        "checks": [],
    }
    try:
        for node in ("A", "B"):
            log_path = work / f"daemon-{node}.log"
            lf = open(log_path, "wb")
            log_files.append(lf)
            procs.append(subprocess.Popen(
                [daemon, "--config", str(cfg_path), "--node-id", node,
                 "--bind", "127.0.0.1", "--workdir", str(work)],
                stdout=lf, stderr=subprocess.STDOUT, cwd=str(work),
            ))
        for node in ("A", "B"):
            url = f"http://127.0.0.1:{nodes[node]['http_port']}/health"
            if not wait_health(url, 20.0):
                report["status"] = "fail"
                report["reason"] = f"{node} 健康检查未通过"
                return report

        # A 启动时 bootstrap_holder 会建 COLD 影子索引并生成伴随种子。
        state = json.loads((work / "state-A.json").read_text(encoding="utf-8"))
        entries = state.get("entries") or []
        picked = None
        for entry in entries:
            if entry.get("infohash"):
                picked = (entry.get("file_hash", ""), entry["infohash"], entry.get("name") or "")
                break
        if not picked:
            report["status"] = "fail"
            report["reason"] = "state-A.json 里没有带 infohash 的条目"
            return report
        file_hash, infohash, name = picked
        report["torrent"] = {"file_hash": file_hash, "info_hash": infohash, "name": name}

        cli_b = f"http://127.0.0.1:{nodes['B']['http_port']}/cli"
        cli_a = f"http://127.0.0.1:{nodes['A']['http_port']}/cli"

        # A 此刻应是 COLD：种子端不上传。先记下来，之后作为对照。
        cold = http_json(cli_a, {"command": "seed", "args": []})
        report["a_before_wake"] = cold.get("seed")

        t0 = time.time()
        ack = http_json(cli_b, {"command": "wake", "args": ["ack", file_hash, infohash]})
        wake_ms = int((time.time() - t0) * 1000)
        ack_obj = (ack or {}).get("ack") or {}
        # `wake ack` 子命令返回的是字段校验摘要（不含原始 torrent_url），
        # 这里按 A 的真实地址重建同一个 URL（= WAKEUP_ACK 里那个 /torrent/<infohash>）。
        torrent_url = ack_obj.get("torrent_url", "") or (
            f"http://127.0.0.1:{nodes['A']['http_port']}/torrent/{infohash}"
            if ack_obj.get("has_torrent_url") else ""
        )
        report["wake"] = {"elapsed_ms": wake_ms, "ack": ack_obj, "torrent_url": torrent_url}
        report["checks"].append({
            "label": "唤醒返回 torrent_url 且可取回",
            "ok": bool(torrent_url) and bool(ack_obj.get("torrent_fetch_ok")),
            "actual": f"{torrent_url or '<空>'} (fetch_ok={ack_obj.get('torrent_fetch_ok')})",
        })
        if not torrent_url:
            report["status"] = "fail"
            return report

        after = http_json(cli_a, {"command": "seed", "args": []})
        report["a_after_wake"] = after.get("seed")
        report["checks"].append({
            "label": "A 被唤醒后进入供片状态",
            "ok": bool((after.get("seed") or {}).get("seeding_hot")),
            "actual": json.dumps(report["a_after_wake"], ensure_ascii=False),
        })

        # 关键一步：把 .torrent 交给 B 的引擎（和浏览器插件走同一个入口）。
        rpc = f"http://127.0.0.1:{nodes['B']['http_port']}/jsonrpc"
        gid = http_json(rpc, {"jsonrpc": "2.0", "id": 1, "method": "aria2.addUri",
                              "params": [[torrent_url]]})
        gid = gid.get("result") or ""
        report["gid"] = gid
        if not gid:
            report["status"] = "fail"
            report["reason"] = f"addUri 未返回 gid: {gid!r}"
            return report

        deadline = time.time() + 60
        last = {}
        samples = []
        while time.time() < deadline:
            st = http_json(rpc, {"jsonrpc": "2.0", "id": 2, "method": "aria2.tellStatus",
                                 "params": [gid]})
            last = st.get("result") or {}
            samples.append({
                "t_ms": int((time.time() - t0) * 1000),
                "status": last.get("status"),
                "completed": last.get("completedLength"),
                "speed": last.get("downloadSpeed"),
                "peers": last.get("connections"),
            })
            if last.get("status") in ("complete", "error"):
                break
            time.sleep(0.2)

        report["task_final"] = last
        report["progress_samples"] = samples[-12:]
        attached = bool(last.get("coldrevive.engineAttached"))
        completed = int(last.get("completedLength") or 0)
        report["checks"].append({
            "label": "任务接了引擎（不是只登记 URI）",
            "ok": attached, "actual": f"engineAttached={attached}, note={last.get('coldrevive.note','')}",
        })
        report["checks"].append({
            "label": "引擎计数器报完成",
            "ok": last.get("status") == "complete" and completed == SAMPLE_BYTES,
            "actual": f"status={last.get('status')}, completedLength={completed}",
        })
        report["checks"].append({
            "label": "info_hash 与 A 的种子一致",
            "ok": (last.get("bittorrent") or {}).get("infoHash") == infohash,
            "actual": json.dumps(last.get("bittorrent") or {}, ensure_ascii=False),
        })

        # ② 落盘内容校验
        out = work / "downloads" / name
        exists = out.is_file()
        got_sha = ""
        got_len = 0
        if exists:
            blob = out.read_bytes()
            got_len = len(blob)
            got_sha = hashlib.sha256(blob).hexdigest()
        report["output"] = {"path": str(out), "bytes": got_len, "sha256": got_sha}
        report["checks"].append({
            "label": "落盘文件存在且字节数正确",
            "ok": exists and got_len == SAMPLE_BYTES,
            "actual": f"{out} exists={exists} bytes={got_len}",
        })
        report["checks"].append({
            "label": "落盘内容与样本逐字节一致",
            "ok": got_sha == sample_sha,
            "actual": f"{got_sha or '<未读到>'} vs {sample_sha}",
        })

        # ③ 供片方对账
        seed_end = http_json(cli_a, {"command": "seed", "args": []})
        seed_obj = seed_end.get("seed") or {}
        report["a_after_download"] = seed_obj
        report["checks"].append({
            "label": "A 实际发出的分片字节覆盖全量（第二计数器）",
            "ok": int(seed_obj.get("served_bytes") or 0) >= SAMPLE_BYTES,
            "actual": json.dumps(seed_obj, ensure_ascii=False),
        })

        report["all_passed"] = all(c["ok"] for c in report["checks"])
        report["status"] = "pass" if report["all_passed"] else "fail"
        return report
    finally:
        for p in procs:
            kill_tree(p)
        for lf in log_files:
            lf.close()
        if keep:
            report["workdir"] = str(work)
        else:
            shutil.rmtree(work, ignore_errors=True)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--keep", action="store_true", help="保留临时目录")
    ap.add_argument("--json", action="store_true", help="只打印 JSON")
    args = ap.parse_args()
    rep = run(args.keep)
    out_dir = REPO / "tests" / "e2e" / "w51" / "reports"
    out_dir.mkdir(parents=True, exist_ok=True)
    out = out_dir / f"engine-download-{time.strftime('%Y%m%dT%H%M%SZ', time.gmtime())}.json"
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
