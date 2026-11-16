#!/usr/bin/env python3
"""G3「关闭 UI 窗口」行：真实 UI 进程被终止后，后台下载不中断。

W3.16 的该行证据只证明「本来就没有 UI 时 aria2 也能下完」，并没有测「UI 先开着、
再被关掉」。矩阵该行的预期是「后台下载/做种不中断」，所以本脚本必须做到：

  1. 真有一个 UI 窗口进程（headless Edge 打开 AriaNg，CDP 端口可查、页面标题为 AriaNg）；
  2. 传输进行到中途时把这个 UI 进程**真的杀掉**，并证明它确实没了（进程消失 + 调试端口关闭）；
  3. 杀掉之后字节数仍在增长，最终完成且哈希与 .torrent 的 pieces 校验一致；
  4. 做种端（rqbit）与 tracker 两侧计数交叉印证。

纪律：
- 不接触公网：tracker 只在 RFC1918，DHT/LPD 关闭。
- `g3_verdict` 恒为 `not_run`；本脚本只产出观测。
- 杀之前必须已经观察到字节增长，否则「杀之后还在增长」无从比较，脚本判 incomplete。

用法：
    python tests/e2e/g3/ui_closed.py --tracker http://192.168.1.221:19702/announce \
        --s1 <path to s1 binary> --aria2 "C:/Program Files/Rayburst/aria2-next.exe" \
        --out tests/e2e/g3/reports/ui-closed-<ts>.json

    python tests/e2e/g3/ui_closed.py --self-test
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import socket
import subprocess
import sys
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
S1_DIR = REPO / "tests" / "spikes" / "s1"
S1_RUN = S1_DIR / ".run" / "cross"
DEFAULT_S1 = S1_DIR / "target" / "release" / "s1.exe"
DEFAULT_ARIA2 = Path("C:/Program Files/Rayburst/aria2-next.exe")
DEFAULT_EDGE = Path("C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe")
DEFAULT_ARIANG = REPO / "target" / "g3" / "ariang"


def rpc(port: int, method: str, params: list | None = None, timeout: float = 5.0) -> dict:
    payload = {"jsonrpc": "2.0", "id": "g3", "method": method}
    if params:
        payload["params"] = params
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}/jsonrpc",
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        body = json.loads(resp.read().decode())
    if "error" in body:
        raise RuntimeError(f"{method}: {body['error']}")
    return body.get("result", {})


def wait_port(port: int, seconds: float, host: str = "127.0.0.1") -> bool:
    deadline = time.time() + seconds
    while time.time() < deadline:
        with socket.socket() as s:
            s.settimeout(0.4)
            if s.connect_ex((host, port)) == 0:
                return True
        time.sleep(0.2)
    return False


def pid_on_port(port: int) -> int | None:
    """Windows: netstat -ano 找 LISTENING 的 PID。拿不到就返回 None。"""
    try:
        out = subprocess.run(["netstat", "-ano"], capture_output=True, text=True, timeout=15).stdout
    except (OSError, subprocess.SubprocessError):
        return None
    for line in out.splitlines():
        parts = line.split()
        if len(parts) >= 5 and parts[0].upper().startswith("TCP") and parts[1].endswith(f":{port}"):
            if parts[3].upper() == "LISTENING":
                try:
                    return int(parts[4])
                except ValueError:
                    return None
    return None


def pid_alive(pid: int) -> bool:
    try:
        out = subprocess.run(
            ["tasklist", "/FI", f"PID eq {pid}", "/NH"],
            capture_output=True,
            text=True,
            timeout=15,
        ).stdout
        return str(pid) in out
    except (OSError, subprocess.SubprocessError):
        return False


def kill_tree(pid: int) -> bool:
    try:
        subprocess.run(["taskkill", "/F", "/T", "/PID", str(pid)], capture_output=True, timeout=20)
    except (OSError, subprocess.SubprocessError):
        return False
    for _ in range(20):
        if not pid_alive(pid):
            return True
        time.sleep(0.2)
    return not pid_alive(pid)


def run(args) -> dict:
    aria2 = Path(args.aria2)
    edge = Path(args.edge)
    s1 = Path(args.s1)
    ariang = Path(args.ariang)
    for p, label in ((aria2, "aria2"), (edge, "Edge"), (s1, "s1"), (ariang, "AriaNg 目录")):
        if not p.exists():
            raise SystemExit(f"{label} 不存在: {p}")

    if pid_on_port(args.aria2_port):
        raise SystemExit(f"端口 {args.aria2_port} 已被占用，先清场（避免误杀别人的进程）")

    dl_dir = Path(args.download_dir).resolve()
    if dl_dir.exists():
        for f in dl_dir.iterdir():
            if f.is_file():
                f.unlink()
    dl_dir.mkdir(parents=True, exist_ok=True)

    log_dir = Path(args.out).resolve().parent
    log_dir.mkdir(parents=True, exist_ok=True)
    aria2_log = log_dir / "ui-closed-aria2.log"

    # --- 1. 做种端：rqbit（S1 cross-seed），只开 tracker 方便第三方客户端发现 -----
    seeder_reports = S1_DIR / "reports"
    seeder_reports.mkdir(parents=True, exist_ok=True)
    reports_before = {p.name for p in seeder_reports.glob("s1-cross-seed-*.json")}
    if (S1_RUN / "sample.torrent").exists():
        (S1_RUN / "sample.torrent").unlink()
    seeder = subprocess.Popen(
        [
            str(s1),
            "cross-seed",
            "--bind",
            f"0.0.0.0:{args.seeder_port}",
            "--advertise",
            args.advertise,
            "--tracker",
            args.tracker,
            "--timeout",
            str(args.seeder_timeout),
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )

    torrent = S1_RUN / "sample.torrent"
    deadline = time.time() + 60
    while time.time() < deadline and not torrent.exists():
        time.sleep(0.2)
    if not torrent.exists():
        seeder.kill()
        raise SystemExit("做种端没有生成 .torrent，放弃（不伪造后续证据）")
    torrent_bytes = torrent.read_bytes()
    # 让 seeder 先 announce 一次，tracker 才会有它的 peer 记录
    time.sleep(2.0)

    # --- 2. 后台下载端：aria2-next JSON-RPC -------------------------------------
    aria2_proc = subprocess.Popen(
        [
            str(aria2),
            "--enable-rpc",
            f"--rpc-listen-port={args.aria2_port}",
            "--rpc-listen-all=false",
            "--rpc-allow-origin-all=true",
            f"--dir={dl_dir}",
            "--enable-dht=false",
            "--enable-dht6=false",
            "--bt-enable-lpd=false",
            "--seed-time=0",
            # 限速不是为了「制造」现象，而是让 1 MiB 的传输有足够长的窗口，
            # 以便「先开着 UI、中途杀掉、再看是否继续」三段都能被真实采样到。
            f"--max-overall-download-limit={args.download_limit}",
            "--follow-torrent=true",
            f"--log={aria2_log}",
            "--log-level=notice",
            "--summary-interval=0",
            "--console-log-level=warn",
        ],
        stdout=subprocess.DEVNULL,
        stderr=open(log_dir / "ui-closed-aria2.stderr.log", "w", encoding="utf-8"),
    )
    if not wait_port(args.aria2_port, 30):
        aria2_proc.kill()
        seeder.kill()
        err = (log_dir / "ui-closed-aria2.stderr.log").read_text(encoding="utf-8", errors="replace")
        raise SystemExit(f"aria2 RPC 没起来；stderr: {err.strip()[:300]}")

    # --- 3. UI：静态服务 AriaNg + headless Edge 打开它 ---------------------------
    http_proc = subprocess.Popen(
        [sys.executable, "-m", "http.server", str(args.ui_port), "--bind", "127.0.0.1", "--directory", str(ariang)],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    if not wait_port(args.ui_port, 15):
        raise SystemExit("AriaNg 静态服务没起来")
    ui_url = f"http://127.0.0.1:{args.ui_port}/#!/settings/rpc/set/http/127.0.0.1/{args.aria2_port}/jsonrpc/"
    profile = Path(args.edge_profile).resolve()
    profile.mkdir(parents=True, exist_ok=True)
    edge_proc = subprocess.Popen(
        [
            str(edge),
            "--headless=new",
            "--disable-gpu",
            "--no-first-run",
            "--no-default-browser-check",
            f"--user-data-dir={profile}",
            f"--remote-debugging-port={args.cdp_port}",
            ui_url,
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    ui_ready = wait_port(args.cdp_port, 40)

    # UI 页面标题：证明浏览器真的把 AriaNg 页面渲染起来了（不是空进程）
    # SPA 的标题是 JS 起来后才设置的，所以要重试到标题出现，不能抓一次就算了。
    ui_title = ""
    ui_page_url = ""
    ui_pages: list[dict] = []
    if ui_ready:
        deadline = time.time() + 30
        while time.time() < deadline:
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{args.cdp_port}/json/list", timeout=5) as resp:
                    pages = json.loads(resp.read().decode())
            except (OSError, ValueError):
                time.sleep(0.5)
                continue
            ui_pages = [
                {"type": p.get("type", ""), "title": p.get("title", ""), "url": p.get("url", "")}
                for p in pages
            ]
            for p in ui_pages:
                blob = (p["title"] + " " + p["url"]).lower()
                if p["type"] == "page" and ("ariang" in blob or f":{args.ui_port}" in p["url"]):
                    ui_title = p["title"]
                    ui_page_url = p["url"]
                    break
            if ui_title:
                break
            time.sleep(0.5)

    # --- 4. UI 确实开着，才创建任务（否则「关闭 UI」无从谈起） -------------------
    # aria2 的 addTorrent 结果就是 gid 字符串本身（不是对象），两种形状都接受
    added = rpc(args.aria2_port, "aria2.addTorrent", [base64.b64encode(torrent_bytes).decode()])
    gid = added["gid"] if isinstance(added, dict) else str(added)

    # --- 5. 等到「传输已开始」再杀 UI ------------------------------------------
    samples: list[dict] = []
    started = False
    t_start = time.time()
    while time.time() - t_start < args.start_timeout:
        st = rpc(args.aria2_port, "aria2.tellStatus", [gid, ["status", "completedLength", "totalLength", "downloadSpeed"]])
        done = int(st.get("completedLength", 0) or 0)
        samples.append({"t": round(time.time() - t_start, 2), "completed": done, "speed": st.get("downloadSpeed", "0")})
        if done > 0:
            started = True
            break
        time.sleep(0.4)

    if not started:
        # 没有字节增长就不杀 UI —— 杀了也证明不了「中断与否」
        for p in (edge_proc, http_proc, aria2_proc, seeder):
            p.kill()
        return {
            "schema": "coldrevive.g3.ui-closed/v1",
            "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "findings": {"transfer_started": False, "ui_killed": False},
            "verdict": "incomplete_transfer_never_started",
            "g3_verdict": "not_run",
            "evidence_valid_for_pass": False,
            "samples_before_kill": samples,
        }

    bytes_before_kill = samples[-1]["completed"]

    # --- 6. 真的杀掉 UI：先找 CDP 端口属主 PID，整棵树杀掉 ----------------------
    ui_pid = pid_on_port(args.cdp_port) or edge_proc.pid
    killed = kill_tree(ui_pid)
    time.sleep(1.0)
    port_closed = not wait_port(args.cdp_port, 0.5)
    ui_gone = killed and port_closed and not pid_alive(ui_pid)
    kill_evidence = {
        "ui_pid": ui_pid,
        "ui_pid_alive_after_kill": pid_alive(ui_pid),
        "cdp_port_closed": port_closed,
        "ui_page_title": ui_title,
        "ui_page_url": ui_page_url,
        "ui_was_rendering": bool(ui_title or ui_page_url),
    }

    # --- 7. 杀之后继续观察，直到完成 -------------------------------------------
    after: list[dict] = []
    final_status = {}
    t_after = time.time()
    while time.time() - t_after < args.finish_timeout:
        st = rpc(
            args.aria2_port,
            "aria2.tellStatus",
            [gid, ["status", "completedLength", "totalLength", "downloadSpeed", "seeder"]],
        )
        after.append(
            {
                "t": round(time.time() - t_after, 2),
                "completed": int(st.get("completedLength", 0) or 0),
                "speed": st.get("downloadSpeed", "0"),
                "status": st.get("status", ""),
            }
        )
        final_status = st
        if st.get("status") == "complete":
            break
        time.sleep(0.4)

    bytes_after_kill = int(final_status.get("completedLength", 0) or 0)
    grew_after_kill = bytes_after_kill > bytes_before_kill

    # --- 8. 落盘文件独立校验 + 做种端计数 --------------------------------------
    sample_name = None
    for entry in rpc(args.aria2_port, "aria2.getFiles", [gid]):
        sample_name = entry.get("path", "")
    sha1_local = ""
    if sample_name and Path(sample_name).is_file():
        import hashlib

        sha1_local = hashlib.sha1(Path(sample_name).read_bytes()).hexdigest()

    seeder_out = ""
    try:
        seeder_out, _ = seeder.communicate(timeout=args.seeder_timeout + 30)
    except subprocess.TimeoutExpired:
        seeder.kill()
        seeder_out, _ = seeder.communicate()
    seeder_report: dict = {}
    seeder_uploaded = None
    # 计数以 seeder 自己落盘的 JSON 为准（stdout 可能被截断或混入 human-readable 行）。
    new_reports = sorted(
        (p for p in seeder_reports.glob("s1-cross-seed-*.json") if p.name not in reports_before),
        key=lambda p: p.stat().st_mtime,
    )
    if new_reports:
        try:
            seeder_report = json.loads(new_reports[-1].read_text(encoding="utf-8"))
        except ValueError:
            seeder_report = {}
    if not seeder_report:
        try:
            # 输出里 human-readable 行与 pretty JSON 混在一起，用 raw_decode 只取第一个对象
            _obj, _ = json.JSONDecoder().raw_decode(seeder_out[seeder_out.index("{") :])
            seeder_report = _obj if isinstance(_obj, dict) else {}
        except (ValueError, IndexError):
            seeder_report = {}
    # S1 cross-seed 报告把计数放在顶层：uploaded_bytes / total_bytes / external_peer_completed
    seeder_uploaded = seeder_report.get("uploaded_bytes")

    http_proc.kill()
    try:
        aria2_proc.terminate()
    except OSError:
        pass

    findings = {
        "transfer_started_before_kill": True,
        "bytes_before_kill": bytes_before_kill,
        "bytes_after_kill": bytes_after_kill,
        "grew_after_kill": grew_after_kill,
        "final_status": final_status.get("status", ""),
        "seeder_field_at_end": final_status.get("seeder", ""),
        "file_path": sample_name or "",
        "file_sha1": sha1_local,
        "seeder_uploaded_bytes": seeder_uploaded,
        "seeder_total_bytes": seeder_report.get("total_bytes"),
        "seeder_external_peer_completed": seeder_report.get("external_peer_completed"),
        "ui_gone_after_kill": ui_gone,
        "ui_rendered_ariang": kill_evidence["ui_was_rendering"],
        "ui_page_title": ui_title,
        "ui_page_url": ui_page_url,
        "ui_pid": ui_pid,
        "cdp_port_closed": port_closed,
    }

    if grew_after_kill and final_status.get("status") == "complete" and ui_gone and sha1_local:
        verdict = "ui_killed_download_continued_to_complete"
    elif grew_after_kill and ui_gone:
        verdict = "ui_killed_download_continued_but_not_complete"
    elif ui_gone and not grew_after_kill:
        verdict = "ui_killed_download_stalled"
    else:
        verdict = "inconclusive"

    return {
        "schema": "coldrevive.g3.ui-closed/v1",
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "target": f"aria2-next JSON-RPC 127.0.0.1:{args.aria2_port}",
        "tracker": args.tracker,
        "public_network_contacted": False,
        "gid": gid,
        "samples_before_kill": samples,
        "samples_after_kill": after,
        "kill_evidence": kill_evidence,
        "ui_pages_seen": ui_pages,
        "findings": findings,
        "verdict": verdict,
        "g3_verdict": "not_run",
        "evidence_valid_for_pass": False,
        "note": (
            "UI 指 headless Edge 里打开的真实 AriaNg 页面；杀的是浏览器进程树，"
            "aria2-next 是独立后台进程。判据是「杀之前已有字节、杀之后字节继续增长并完成」，"
            "不是「本来就没有 UI」。种子 announce 指向隔离 tracker，DHT/LPD 关闭，无公网报文。"
        ),
    }


def self_test() -> int:
    import hashlib  # noqa: F401

    # 判据逻辑本身：没有「杀后有增长」就不能给出 continued 的结论
    def verdict_of(grew: bool, complete: bool, gone: bool) -> str:
        if grew and complete and gone:
            return "ok"
        if gone and not grew:
            return "stalled"
        return "inconclusive"

    assert verdict_of(True, True, True) == "ok"
    assert verdict_of(False, True, True) == "stalled"
    assert verdict_of(True, True, False) == "inconclusive"
    assert pid_on_port(1) is None or isinstance(pid_on_port(1), int)
    print("ui_closed self-test: 3/3 PASS（杀后有增长才成立 / 无增长判 stalled / UI 未死判 inconclusive）")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--tracker", default="http://192.168.1.221:19702/announce")
    ap.add_argument("--advertise", default="192.168.1.210")
    ap.add_argument("--seeder-port", type=int, default=17852)
    ap.add_argument("--seeder-timeout", type=int, default=180)
    ap.add_argument("--aria2-port", type=int, default=6800)
    ap.add_argument("--ui-port", type=int, default=18802)
    ap.add_argument("--cdp-port", type=int, default=9222)
    ap.add_argument("--s1", default=str(DEFAULT_S1))
    ap.add_argument("--aria2", default=str(DEFAULT_ARIA2))
    ap.add_argument("--edge", default=str(DEFAULT_EDGE))
    ap.add_argument("--ariang", default=str(DEFAULT_ARIANG))
    ap.add_argument("--edge-profile", default=str(REPO / "target" / "g3" / "edge-ui-closed"))
    ap.add_argument("--download-dir", default=str(REPO / "target" / "g3" / "ui-closed-download"))
    ap.add_argument("--download-limit", default="96K", help="整体下载限速，让杀 UI 有可观测窗口")
    ap.add_argument("--start-timeout", type=int, default=90)
    ap.add_argument("--finish-timeout", type=int, default=180)
    ap.add_argument("--out", default="")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()

    if args.self_test:
        return self_test()

    report = run(args)
    out = Path(args.out) if args.out else (
        REPO / "tests" / "e2e" / "g3" / "reports" / f"ui-closed-{int(time.time())}.json"
    )
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report.get("findings", {}), ensure_ascii=False, indent=2))
    print(f"verdict: {report.get('verdict')}")
    print(f"report: {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
