"""W59 诊断：绕过 librqbit/aria2，直接用 SOCKS5 + BT 握手走隧道。

目的：判定「1 MiB 是在桥/隧道/pump 这一层丢的」还是「在 librqbit 那侧丢的」。
- 读出 /health 里的 proxy 地址（桥的 SOCKS5 端口）
- SOCKS5 CONNECT 到会合地址 203.0.113.31:17902
- 发 BT 握手 + interested，然后一直读，统计字节并保存原始负载
用法: python probe_tunnel.py <infohash-hex> [timeout_s]
"""
import socket
import struct
import sys
import time
import urllib.request
import json

INFOHASH = sys.argv[1]
TIMEOUT = float(sys.argv[2]) if len(sys.argv) > 2 else 25.0
RENDEZVOUS = ("203.0.113.31", 17902)


def health():
    with urllib.request.urlopen("http://127.0.0.1:17800/health", timeout=5) as r:
        return json.loads(r.read().decode())


h = health()
proxy = h["data_plane"]["proxy"]
host, port = proxy.split("//")[1].split(":")
port = int(port)
print(f"proxy={proxy} tunnels={h['data_plane']['tunnels']}")

s = socket.create_connection(("127.0.0.1", port), timeout=10)
# SOCKS5：无认证
s.sendall(b"\x05\x01\x00")
greet = s.recv(2)
print(f"socks5 greet={greet.hex()}")
if greet != b"\x05\x00":
    print("FAIL: 桥没选无认证")
    sys.exit(1)

# CONNECT，地址用 IPv4
req = b"\x05\x01\x00\x01" + socket.inet_aton(RENDEZVOUS[0]) + struct.pack(">H", RENDEZVOUS[1])
s.sendall(req)
rep = s.recv(10)
print(f"socks5 reply={rep.hex()}")
if len(rep) < 2 or rep[1] != 0:
    print(f"FAIL: CONNECT 被拒 rep={rep.hex()}")
    sys.exit(1)

# BT 握手
ih = bytes.fromhex(INFOHASH)
peer_id = b"-CR0001-" + b"w59probe".ljust(12, b"0")[:12]
hs = b"\x13BitTorrent protocol" + b"\x00" * 8 + ih + peer_id
assert len(hs) == 68, len(hs)
s.sendall(hs)

s.settimeout(TIMEOUT)
buf = bytearray()


def pump(deadline):
    while time.time() < deadline:
        try:
            chunk = s.recv(65536)
        except socket.timeout:
            print("读超时（超时未再收到数据）")
            return False
        except OSError as e:
            print(f"读出错: {e}")
            return False
        if not chunk:
            print("对端 EOF")
            return False
        buf.extend(chunk)
        if len(buf) >= 68:
            return True
    return False


if not pump(time.time() + TIMEOUT):
    print(f"FAIL: 握手阶段没读到 68 字节，只有 {len(buf)} B")
    sys.exit(2)

print(f"握手回包 {buf[:68].hex()}")
print(f"对端 peer_id={buf[48:68]}")
# interested
s.sendall(b"\x00\x00\x00\x01\x02")
print("已发 interested，继续读…")

start = time.time()
end = start + TIMEOUT
while time.time() < end:
    try:
        chunk = s.recv(65536)
    except socket.timeout:
        print("读超时，停止")
        break
    except OSError as e:
        print(f"读出错: {e}")
        break
    if not chunk:
        print("对端 EOF")
        break
    buf.extend(chunk)
    if len(buf) >= 1048576:
        pass

elapsed = time.time() - start
print(f"总收字节={len(buf)}（含 68 B 握手）耗时={elapsed:.2f}s")
open("tests/e2e/w59/reports/probe_payload.bin", "wb").write(bytes(buf))
print("已写 tests/e2e/w59/reports/probe_payload.bin")
