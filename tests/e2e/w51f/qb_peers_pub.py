#!/usr/bin/env python3
"""持续采样 qBittorrent 的 peer 列表，**带上 peer 的地址与传输层**。

与 w51e 的同名脚本的区别：那边只统计 `connection` 的分布（判「有没有走 µTP」），
这边必须同时记下 `ip` / `port` —— 公网测试里「走了 µTP」还不够，还得证明
**对端地址是公网 IP**，否则无法排除 LAN 捷径。

用法：
    qb_peers_pub.py <webui_port> <cookie_file> <infohash> --watch 420 --interval 0.1

输出：每行一个 JSON（单条采样），字段 peers 是 [{ip, port, conn, prog, dl, ul}]。
只用 ASCII 输出：`μTP` 经 Windows 控制台会变成 `?TP`。
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
        rows = []
        for key, p in peers.items():
            ip, _, prt = str(key).rpartition(":")
            conn = (p.get("connection") or "?").replace("\u03bc", "u").replace("\u00b5", "u")
            if not ip:
                ip = str(p.get("ip") or "")
            try:
                port_n = int(prt or p.get("port") or 0)
            except ValueError:
                port_n = 0
            rows.append({
                "ip": ip,
                "port": port_n,
                "conn": conn,
                "prog": round(float(p.get("progress") or 0.0), 4),
                "dl": int(p.get("dl_speed") or 0),
                "ul": int(p.get("up_speed") or 0),
            })
        return {"t": round(time.time(), 3), "count": len(rows), "peers": rows}

    def emit(obj: dict) -> None:
        print(json.dumps(obj, ensure_ascii=False).encode("ascii", "replace").decode("ascii"),
              flush=True)

    if watch <= 0:
        emit(sample())
        return 0

    deadline = time.time() + watch
    while time.time() < deadline:
        try:
            emit(sample())
        except Exception as e:  # WebUI 还没起来 / cookie 失效 / 任务已删
            emit({"t": round(time.time(), 3), "error": f"{type(e).__name__}: {e}"})
        time.sleep(interval)
    return 0


if __name__ == "__main__":
    sys.exit(main())
