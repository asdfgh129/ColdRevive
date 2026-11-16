#!/usr/bin/env python3
"""G3「浏览器插件」行：真插件 + 真 .torrent 链接 + ColdRevive 真解析。

W3.16 的该行只推了一条 `https://example.com/...` 占位链接，所以只能证明「任务被登记」。
矩阵该行的预期是「推送下载链接到 ColdRevive，任务创建成功」，本脚本把它做实：

  1. 用 MV3 模板渲染出的插件（service worker 里带真实链接）在 headless Edge 里加载；
  2. 推的是一条**真实存在的 .torrent**（HTTP 静态服务，内容与磁盘文件逐字节一致）；
  3. ColdRevive 真的把种子取回来并解析，`tellStatus` 回的是真实 info_hash / 长度 / 片数；
  4. info_hash 由本脚本用独立 bencode 解析重算，与 ColdRevive 回包交叉比对。

边界要说清楚：ColdRevive 目前没有 BT 下载引擎（W5.1 用 rqbit 替换最小服务端），
任务停在 `waiting`，本脚本**不**声称内容已下载。它证明的是「推送 → 取回 → 解析」这条链路。

用法：
    python tests/e2e/g3/browser_push.py \
        --torrent tests/spikes/s1/.run/cross/sample.torrent \
        --out tests/e2e/g3/reports/browser-extension-<ts>.json

    python tests/e2e/g3/browser_push.py --self-test
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
ASSETS = REPO / "tests" / "e2e" / "g3" / "assets" / "browser-extension"
EXAMPLE_TOML = REPO / "config" / "coldrevive.example.toml"
DEFAULT_DAEMON = REPO / "target" / "release" / "coldrevived.exe"
DEFAULT_EDGE = Path("C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe")


# --------------------------------------------------------------- bencode ----
def _bdecode(raw: bytes, i: int = 0) -> tuple[object, int]:
    """极小 bencode 解码器：只为独立复算 info_hash，不复用被测代码。"""
    c = raw[i : i + 1]
    if c == b"i":
        end = raw.index(b"e", i)
        return int(raw[i + 1 : end]), end + 1
    if c == b"l":
        i += 1
        out = []
        while raw[i : i + 1] != b"e":
            v, i = _bdecode(raw, i)
            out.append(v)
        return out, i + 1
    if c == b"d":
        i += 1
        out = {}
        while raw[i : i + 1] != b"e":
            k, i = _bdecode(raw, i)
            v, i = _bdecode(raw, i)
            out[k] = v
        return out, i + 1
    colon = raw.index(b":", i)
    n = int(raw[i:colon])
    start = colon + 1
    return raw[start : start + n], start + n


def info_span(raw: bytes) -> tuple[int, int]:
    """返回 info 值在原始字节里的区间——info_hash 必须对**原始字节**做 SHA-1。"""
    i = raw.index(b"4:info") + len(b"4:info")
    _, end = _bdecode(raw, i)
    return i, end


def torrent_infohash(raw: bytes) -> str:
    a, b = info_span(raw)
    return hashlib.sha1(raw[a:b]).hexdigest()


# ----------------------------------------------------------------- utils ----
def wait_port(port: int, seconds: float, host: str = "127.0.0.1") -> bool:
    deadline = time.time() + seconds
    while time.time() < deadline:
        with socket.socket() as s:
            s.settimeout(0.4)
            if s.connect_ex((host, port)) == 0:
                return True
        time.sleep(0.2)
    return False


def rpc(port: int, method: str, params: list | None = None, timeout: float = 8.0) -> dict:
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
    return body.get("result")


def render_extension(dest: Path, rpc_url: str, link: str) -> None:
    rpc_origin = rpc_url.rsplit("/jsonrpc", 1)[0]
    dest.mkdir(parents=True, exist_ok=True)
    for src, name in (("manifest.template.json", "manifest.json"), ("sw.template.js", "sw.js")):
        text = (ASSETS / src).read_text(encoding="utf-8")
        text = text.replace("__RPC_URL__", rpc_url).replace("__RPC_ORIGIN__", rpc_origin).replace("__LINK__", link)
        (dest / name).write_text(text, encoding="utf-8", newline="\n")


def write_config(dest: Path, work: Path, http_port: int, wake_port: int, bt_port: int) -> None:
    """写一份最小可用的守护进程配置。

    注意：`Config::load` 走的是 `Json::load`，**只认 JSON**；仓库里的
    `config/coldrevive.example.toml` 是给人看的样例，直接喂给守护进程会报
    「非法 JSON 字符 '#'」。这里按真实 schema 生成 JSON，避免拿错格式。
    """
    cfg = {
        "bind": "127.0.0.1",
        "nodes": {"A": {"http_port": http_port, "wake_udp_port": wake_port, "bt_port": bt_port}},
        "sample_path": str(REPO / "tests" / "fixtures" / "t0" / "sample" / "coldrevive-t0-sample.bin"),
        "keywords": ["coldrevive", "g3"],
        "namespace": "ColdRevive_V1:",
        "workdir": str(work / "daemon-run"),
        "golden_req": str(REPO / "tests" / "fixtures" / "t0" / "golden" / "wakeup_req.v1.json"),
        "golden_ack": str(REPO / "tests" / "fixtures" / "t0" / "golden" / "wakeup_ack.v1.json"),
        "downloader": str(Path("C:/Program Files/Rayburst/aria2-next.exe")),
        "challenge_window_sec": 300,
    }
    dest.write_text(json.dumps(cfg, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def daemon_health(port: int) -> bool:
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=3) as r:
            return r.status == 200
    except (urllib.error.URLError, OSError):
        return False


def run(args) -> dict:
    torrent = Path(args.torrent)
    if not torrent.is_file():
        raise SystemExit(f"--torrent 不存在: {torrent}")
    raw = torrent.read_bytes()
    expected_hash = torrent_infohash(raw)
    # info.length 才是内容长度（单文件种子）；用来核对前端看到的 totalLength。
    _meta, _ = _bdecode(raw)
    expected_payload = int(_meta[b"info"][b"length"]) if isinstance(_meta, dict) else 0

    daemon_bin = Path(args.daemon)
    edge = Path(args.edge)
    for p, label in ((daemon_bin, "coldrevived"), (edge, "Edge")):
        if not p.is_file():
            raise SystemExit(f"{label} 不存在: {p}")

    work = Path(args.workdir).resolve()
    work.mkdir(parents=True, exist_ok=True)
    # 端口上已经有东西就直接停：上一轮残留的守护进程会让「任务没被创建」看起来像插件没工作，
    # 实际是查错了进程（本轮踩过）。
    if wait_port(args.daemon_port, 0.6):
        raise SystemExit(f"端口 {args.daemon_port} 已被占用，先清场再跑（可能是上一轮残留的 coldrevived）")
    cfg = work / f"rpc-daemon-{args.daemon_port}.json"
    write_config(cfg, work, args.daemon_port, args.daemon_port + 1, args.daemon_port + 2)
    shared = work / "shared"
    shared.mkdir(parents=True, exist_ok=True)

    daemon_log = open(work / "coldrevived.log", "w", encoding="utf-8")
    daemon = subprocess.Popen(
        [str(daemon_bin), "--config", str(cfg), "--node-id", "A"],
        cwd=str(REPO),
        stdout=daemon_log,
        stderr=subprocess.STDOUT,
    )
    if not wait_port(args.daemon_port, 25) or not daemon_health(args.daemon_port):
        daemon.kill()
        raise SystemExit(f"coldrevived 没起来；日志: {work / 'coldrevived.log'}")

    # 真实 .torrent 用静态服务暴露；插件推的就是这个地址
    http_dir = work / "torrent-http"
    http_dir.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(torrent, http_dir / "sample.torrent")
    static = subprocess.Popen(
        [sys.executable, "-m", "http.server", str(args.static_port), "--bind", "127.0.0.1", "--directory", str(http_dir)],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    if not wait_port(args.static_port, 15):
        raise SystemExit("静态服务没起来")
    link = f"http://127.0.0.1:{args.static_port}/sample.torrent"

    ext_dir = work / f"ext-{args.daemon_port}"
    if ext_dir.exists():
        shutil.rmtree(ext_dir)
    render_extension(ext_dir, f"http://127.0.0.1:{args.daemon_port}/jsonrpc", link)

    # 每次用全新 profile：复用旧 profile 时 Edge 会保留上一轮的插件注册状态，
    # 表面上看是「插件没跑」，实际是它压根没重新加载（本轮踩过）。
    profile = work / f"edge-profile-{args.daemon_port}-{int(time.time())}"
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
            f"--load-extension={ext_dir}",
            # 必须给一个真实 http 页面：about:blank 下 MV3 的 service worker
            # 在本机 Edge 上不会启动（本轮实测），插件就永远不推送。
            f"http://127.0.0.1:{args.static_port}/",
        ],
        stdout=subprocess.DEVNULL,
        stderr=open(work / "edge-stderr.log", "w", encoding="utf-8"),
    )

    # 给 Edge 把插件装起来的时间；顺手记下当时的 CDP 目标（插件 SW 在不在）
    time.sleep(8)
    early_targets: list[str] = []
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{args.cdp_port}/json/list", timeout=4) as r:
            early_targets = [t.get("url", "") for t in json.loads(r.read().decode())]
    except (OSError, ValueError) as e:
        early_targets = [f"cdp-unavailable: {e}"]

    # 等插件把任务推过来：任务会停在 waiting（没有下载引擎），所以三个队列都要查，
    # 只查 tellActive 会永远是空——本轮踩过这个坑。
    task = None
    last_error = ""
    deadline = time.time() + args.push_timeout
    while time.time() < deadline:
        gid = ""
        for method in ("aria2.tellActive", "aria2.tellWaiting", "aria2.tellStopped"):
            try:
                items = rpc(args.daemon_port, method) or []
            except Exception as e:  # noqa: BLE001 - 任何解析/网络异常都记下来，不猜
                last_error = f"{method}: {e}"
                items = []
            for it in items:
                if isinstance(it, dict) and it.get("gid"):
                    gid = it["gid"]
                    break
            if gid:
                break
        if gid:
            try:
                task = rpc(args.daemon_port, "aria2.tellStatus", [gid])
            except Exception as e:  # noqa: BLE001
                last_error = f"tellStatus({gid}): {e}"
                task = None
            if task:
                break
        time.sleep(0.5)

    edge_running = wait_port(args.cdp_port, 5)
    live_targets: list[str] = []
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{args.cdp_port}/json/list", timeout=4) as r:
            live_targets = [t.get("url", "") for t in json.loads(r.read().decode())]
    except (OSError, ValueError) as e:
        live_targets = [f"cdp-unavailable: {e}"]
    try:
        edge_proc.kill()
    except OSError:
        pass
    static.kill()
    daemon.terminate()
    try:
        daemon.wait(timeout=5)
    except subprocess.TimeoutExpired:
        daemon.kill()
    daemon_log.close()

    if task is None:
        # 失败时把现场留下：没有这些就只能猜「插件没跑」还是「RPC 没通」。
        diag_targets: list[str] = []
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{args.cdp_port}/json/list", timeout=4) as r:
                diag_targets = [t.get("url", "") for t in json.loads(r.read().decode())]
        except (OSError, ValueError) as e:
            diag_targets = [f"cdp-unavailable: {e}"]
        return {
            "schema": "coldrevive.g3.browser-push/v1",
            "diagnostics": {
                "live_cdp_targets_before_kill": live_targets,
                "early_cdp_targets": early_targets,
                "cdp_targets": diag_targets,
                "daemon_log_tail": (work / "coldrevived.log").read_text(encoding="utf-8", errors="replace")[-800:],
                "edge_stderr_tail": (work / "edge-stderr.log").read_text(encoding="utf-8", errors="replace")[-800:],
                "last_poll_error": last_error,
                "extension_dir": str(ext_dir),
                "render_check": {
                    "sw_link_line": next(
                        (ln for ln in (ext_dir / "sw.js").read_text(encoding="utf-8").splitlines() if ln.startswith("const LINK")),
                        "",
                    )
                },
            },
            "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "link": link,
            "expected_infohash": expected_hash,
            "findings": {"task_created": False},
            "verdict": "no_task_pushed",
            "g3_verdict": "not_run",
            "evidence_valid_for_pass": False,
        }

    got_hash = (task.get("bittorrent") or {}).get("infoHash", "")
    total = int(task.get("totalLength", "0") or 0)
    findings = {
        "task_created": True,
        "gid": task.get("gid", ""),
        "status": task.get("status", ""),
        "pushed_link": link,
        "task_uri": ((task.get("files") or [{}])[0].get("uris") or [{}])[0].get("uri", ""),
        "torrent_parsed": bool(task.get("coldrevive.torrentParsed")),
        "info_hash_from_coldrevive": got_hash,
        "info_hash_independent": expected_hash,
        "info_hash_matches": got_hash == expected_hash,
        "total_length": total,
        # 别把 .torrent 文件本身的字节数当成内容长度：两者含义不同，写清楚。
        "torrent_file_bytes": len(raw),
        "expected_payload_length": expected_payload,
        "payload_length_matches": total == expected_payload,
        "piece_count": int(task.get("coldrevive.pieceCount", "0") or 0),

        "task_name": ((task.get("files") or [{}])[0].get("path", "")),
        "coldrevive_note": task.get("coldrevive.note", ""),
        "edge_was_running": edge_running,
        "downloads_content": False,  # 明确边界：没有下载引擎，不声称下载
    }
    if findings["info_hash_matches"] and findings["torrent_parsed"]:
        verdict = "push_then_parse_confirmed"
    elif findings["task_created"]:
        verdict = "task_registered_but_not_parsed"
    else:
        verdict = "inconclusive"

    return {
        "schema": "coldrevive.g3.browser-push/v1",
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "target": f"coldrevived /jsonrpc 127.0.0.1:{args.daemon_port}",
        "extension_dir": str(ext_dir),
        "public_network_contacted": False,
        "findings": findings,
        "verdict": verdict,
        "g3_verdict": "not_run",
        "evidence_valid_for_pass": False,
        "note": (
            "插件是 MV3 模板渲染产物，service worker 里带的是本次运行的真实 .torrent 地址；"
            "任务由浏览器进程里的插件创建，ColdRevive 取回并解析后回真实 info_hash，"
            "info_hash 由本脚本独立 bencode 复算比对。**未下载内容**：ColdRevive 尚无 BT 下载引擎。"
        ),
    }


def self_test() -> int:
    # 独立 bencode 解析必须对「info 原始字节」求哈希，不能重新编码
    raw = b"d4:infod6:lengthi3e4:name1:a12:piece lengthi2e6:pieces20:00000000000000000000ee4:name1:x12:piece lengthi2ee"
    # 构造一个合法的最小种子再验证：用 cr-torrent 的布局手写
    info = b"d6:lengthi3e4:name1:a12:piece lengthi2e6:pieces20:" + b"0" * 20 + b"e"
    raw = b"d8:announce9:http://x/4:info" + info + b"e"
    expect = hashlib.sha1(info).hexdigest()
    assert torrent_infohash(raw) == expect, "info_hash 必须对 info 原始字节求 SHA-1"
    assert _bdecode(b"i42e")[0] == 42
    assert _bdecode(b"4:spam")[0] == b"spam"
    # 渲染必须把所有占位符替换干净，不留 __TOKEN__
    import tempfile

    with tempfile.TemporaryDirectory() as d:
        dest = Path(d)
        render_extension(dest, "http://127.0.0.1:18800/jsonrpc", "http://127.0.0.1:18805/sample.torrent")
        body = (dest / "sw.js").read_text(encoding="utf-8")
        man = json.loads((dest / "manifest.json").read_text(encoding="utf-8"))
        assert "18805/sample.torrent" in body
        link_line = next(ln for ln in body.splitlines() if ln.startswith("const LINK"))
        assert "example.com" not in link_line, f"推的链接不能是占位地址: {link_line}"
        assert man["host_permissions"] == ["http://127.0.0.1:18800/*"], man["host_permissions"]
    print("browser_push self-test: 4/4 PASS（独立 info_hash / bencode 解码 / 模板渲染 / 主机权限）")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--torrent", default=str(REPO / "tests" / "spikes" / "s1" / ".run" / "cross" / "sample.torrent"))
    ap.add_argument("--daemon", default=str(DEFAULT_DAEMON))
    ap.add_argument("--edge", default=str(DEFAULT_EDGE))
    ap.add_argument("--daemon-port", type=int, default=18800)
    ap.add_argument("--static-port", type=int, default=18805)
    ap.add_argument("--cdp-port", type=int, default=9224)
    ap.add_argument("--push-timeout", type=int, default=60)
    ap.add_argument("--workdir", default=str(REPO / "target" / "g3" / "browser-push"))
    ap.add_argument("--out", default="")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()

    if args.self_test:
        return self_test()

    report = run(args)
    out = Path(args.out) if args.out else (
        REPO / "tests" / "e2e" / "g3" / "reports" / f"browser-extension-{int(time.time())}.json"
    )
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report.get("findings", {}), ensure_ascii=False, indent=2))
    print(f"verdict: {report.get('verdict')}")
    print(f"report: {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
