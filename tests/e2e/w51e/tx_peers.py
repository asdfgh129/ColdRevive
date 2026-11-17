#!/usr/bin/env python3
"""打印 Transmission 当前各 peer 的传输层（uTP / TCP）。

Transmission 3.00 的 RPC `torrent-get` 会在 `peers[]` 里回 `isUTP`——这是**客户端自己**的
判断，比在供片方猜「这条连接可能是 UDP」可靠。输出刻意只用 ASCII：它要经 ssh 回来，
`μTP` 这类字符过 Windows 控制台会被替换。
"""
from __future__ import annotations

import json
import sys
import urllib.error
import urllib.request


def main() -> int:
    port = sys.argv[1] if len(sys.argv) > 1 else "19501"
    url = f"http://127.0.0.1:{port}/transmission/rpc"

    def call(payload: dict) -> dict:
        data = json.dumps(payload).encode()
        req = urllib.request.Request(url, data=data,
                                     headers={"Content-Type": "application/json"})
        try:
            return json.load(urllib.request.urlopen(req, timeout=8))
        except urllib.error.HTTPError as e:
            if e.code != 409:
                raise
            sid = e.headers.get("X-Transmission-Session-Id", "")
            req.add_header("X-Transmission-Session-Id", sid)
            return json.load(urllib.request.urlopen(req, timeout=8))

    r = call({"method": "torrent-get",
              "arguments": {"fields": ["id", "percentDone", "peers"]}})
    torrents = (r.get("arguments") or {}).get("torrents") or []
    peers = [p for t in torrents for p in (t.get("peers") or [])]
    out = {
        "torrents": len(torrents),
        "peers": len(peers),
        "utp": sum(1 for p in peers if p.get("isUTP") is True),
        "tcp": sum(1 for p in peers if p.get("isUTP") is not True),
        "addresses": [p.get("address") for p in peers][:4],
    }
    print(json.dumps(out))
    return 0


if __name__ == "__main__":
    sys.exit(main())
