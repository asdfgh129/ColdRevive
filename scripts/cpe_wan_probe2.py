#!/usr/bin/env python3
"""对家宽 CPE 做只读探测：NAT-PMP/PCP 外网地址、管理页是否可读、端口开放情况。

不发送任何配置类请求（不建映射、不改设置）。NAT-PMP 只用 opcode 0（External IP）。
"""
import json
import socket
import struct
import sys
import time

GW = sys.argv[1] if len(sys.argv) > 1 else "192.168.1.254"
out = {"gateway": GW, "at": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "probes": {}}


def udp(payload, port, timeout=1.5, hexshow=True):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.settimeout(timeout)
    try:
        s.sendto(payload, (GW, port))
        data, addr = s.recvfrom(2048)
        return {"from": f"{addr[0]}:{addr[1]}", "len": len(data), "hex": data.hex()}
    except socket.timeout:
        return {"from": None, "error": "timeout"}
    except OSError as e:
        return {"from": None, "error": f"{type(e).__name__}: {e}"}
    finally:
        s.close()


# NAT-PMP: version 0, opcode 0 (public address request)
out["probes"]["natpmp_5351"] = udp(b"\x00\x00", 5351)

# PCP: version 2, opcode 0 (announce), 24-byte header + 0 map payload
pcp = struct.pack("!BBHI", 2, 0, 0, 0) + b"\x00" * 12
out["probes"]["pcp_5351"] = udp(pcp, 5351)


def tcp(port, timeout=1.2):
    s = socket.socket()
    s.settimeout(timeout)
    t0 = time.time()
    try:
        s.connect((GW, port))
        return {"open": True, "ms": round((time.time() - t0) * 1000, 1)}
    except Exception as e:  # noqa: BLE001
        return {"open": False, "err": type(e).__name__}
    finally:
        s.close()


for p in (80, 443, 8080, 8443, 1900, 5000, 7547):
    out["probes"][f"tcp_{p}"] = tcp(p)

# 管理页未认证首页（只读 GET，不提交任何表单）
if out["probes"]["tcp_80"].get("open"):
    try:
        import urllib.request

        req = urllib.request.Request(f"http://{GW}/", headers={"User-Agent": "cr-cpe-probe/1"})
        with urllib.request.urlopen(req, timeout=4) as r:
            body = r.read(6000).decode("utf-8", "replace")
        out["probes"]["http_root"] = {
            "status": r.status,
            "title": (body.split("<title>")[1].split("</title>")[0].strip() if "<title>" in body else None),
            "looks_like_login": any(k in body.lower() for k in ("password", "login", "登录", "密码")),
        }
    except Exception as e:  # noqa: BLE001
        out["probes"]["http_root"] = {"error": f"{type(e).__name__}: {e}"}

print(json.dumps(out, ensure_ascii=False, indent=2))
