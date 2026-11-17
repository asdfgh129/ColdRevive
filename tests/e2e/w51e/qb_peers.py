#!/usr/bin/env python3
"""打印 qBittorrent 当前 peer 的 `connection` 字段统计（"μTP" / "TCP" / ...）。

两种用法：
    一次性：  qb_peers.py <webui_port> <cookie> <infohash>
    持续采样：qb_peers.py <webui_port> <cookie> <infohash> --watch 180 --interval 0.1

为什么需要「持续采样」：4 MiB 在本环境 1–2 秒就下完了，而每次采样都要起一条 ssh
（几百毫秒）。一次一条的做法会整段错过传输窗口，读出来是「没有 peer」，很容易被
当成「没走 µTP」。放进远端一个进程里高频采样、事后把文件取回来，采到的才是真形态。

输出只用 ASCII：`μTP` 过 Windows 控制台会被替换成 `?TP`。
"""
from __future__ import annotations

import http.cookiejar
import json
import sys
import time
import urllib.request


def main() -> int:
    port, cookie, info_hash = sys.argv[1], sys.argv[2], sys.argv[3]
    watch = 0.0
    interval = 0.1
    if "--watch" in sys.argv:
        watch = float(sys.argv[sys.argv.index("--watch") + 1])
    if "--interval" in sys.argv:
        interval = float(sys.argv[sys.argv.index("--interval") + 1])

    jar = http.cookiejar.MozillaCookieJar(cookie)
    jar.load(ignore_discard=True, ignore_expires=True)
    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
    url = f"http://127.0.0.1:{port}/api/v2/sync/torrentPeers?hash={info_hash}&rid=0"

    def sample() -> dict:
        raw = opener.open(url, timeout=5).read()
        body = json.loads(raw.decode("utf-8")) or {}
        peers = body.get("peers") or {}
        counts: dict[str, int] = {}
        for p in peers.values():
            kind = (p.get("connection") or "?").replace("μ", "u").replace("µ", "u")
            counts[kind] = counts.get(kind, 0) + 1
        first = next(iter(peers.values()), {}) if peers else {}
        return {"peers": len(peers), "by_connection": counts,
                "sample": {k: first.get(k) for k in ("connection", "client", "progress")}}

    def emit(obj: dict) -> None:
        print(json.dumps(obj, ensure_ascii=False).encode("ascii", "replace").decode("ascii"),
              flush=True)

    if watch <= 0:
        emit(sample())
        return 0

    deadline = time.time() + watch
    while time.time() < deadline:
        try:
            out = sample()
            out["t"] = round(time.time(), 3)
            emit(out)
        except Exception as e:  # WebUI 还没起来 / cookie 还没写
            emit({"error": f"{type(e).__name__}: {e}", "t": round(time.time(), 3)})
        time.sleep(interval)
    return 0


if __name__ == "__main__":
    sys.exit(main())
