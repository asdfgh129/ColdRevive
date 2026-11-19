"""W59：用**原始 BT 客户端**经 SOCKS5 桥验证中继数据面能搬完 1 MiB 并逐字节校验。

与 probe_tunnel.py 的区别：这个会真的发 request，收满 4 个分片，重组后算 md5，
用来区分「中继/隧道/持有者 splice 有问题」与「librqbit 那侧没消费隧道」。
用法: python probe_bt.py <infohash-hex> <期望md5> [piece_size] [total]
"""
import hashlib
import socket
import struct
import sys
import time
import urllib.request
import json

INFOHASH = sys.argv[1]
EXPECT_MD5 = sys.argv[2]
PIECE = int(sys.argv[3]) if len(sys.argv) > 3 else 262144
TOTAL = int(sys.argv[4]) if len(sys.argv) > 4 else 1048576
RENDEZVOUS = ("203.0.113.31", 17902)
BLOCK = 16384


def post(path, body):
    req = urllib.request.Request(
        "http://127.0.0.1:17800" + path,
        data=body.encode(),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=45) as r:
        return json.loads(r.read().decode())


def health():
    with urllib.request.urlopen("http://127.0.0.1:17800/health", timeout=5) as r:
        return json.loads(r.read().decode())


h = health()
proxy = h["data_plane"]["proxy"]
port = int(proxy.split("//")[1].split(":")[1])
print(f"proxy={proxy} tunnels={h['data_plane']['tunnels']}")

s = socket.create_connection(("127.0.0.1", port), timeout=10)
s.sendall(b"\x05\x01\x00")
assert s.recv(2) == b"\x05\x00", "桥没选无认证"
s.sendall(b"\x05\x01\x00\x01" + socket.inet_aton(RENDEZVOUS[0]) + struct.pack(">H", RENDEZVOUS[1]))
rep = s.recv(10)
assert len(rep) >= 2 and rep[1] == 0, f"CONNECT 被拒 {rep.hex()}"
print(f"socks5 CONNECT ok ({rep.hex()})")

ih = bytes.fromhex(INFOHASH)
peer_id = b"-CR0001-" + b"w59probe".ljust(12, b"0")[:12]
s.sendall(b"\x13BitTorrent protocol" + b"\x00" * 8 + ih + peer_id)

# 读握手，然后读到 unchoke（id=1）
buf = bytearray()
s.settimeout(20)
while len(buf) < 68:
    c = s.recv(4096)
    if not c:
        print(f"FAIL: 握手阶段 EOF，只收到 {len(buf)} B")
        sys.exit(2)
    buf.extend(c)
print(f"握手回包 peer_id={bytes(buf[48:68])}")
rest = buf[68:]
buf = bytearray(buf[:68])
unchoked = False
while not unchoked:
    while True:
        need = 4
        if len(rest) < need:
            break
        ln = struct.unpack(">I", rest[:4])[0]
        if len(rest) < 4 + ln:
            break
        mid = rest[4] if ln > 0 else None
        if mid == 1:
            unchoked = True
        if mid in (0, 1):
            print(f"  引擎消息 id={mid} len={ln}")
        rest = rest[4 + ln:]
        if unchoked:
            break
    if unchoked:
        break
    c = s.recv(4096)
    if not c:
        print(f"FAIL: 等 unchoke 时 EOF，收到 {len(rest)} B 剩余")
        sys.exit(3)
    rest.extend(c)
print("已 unchoke，开始请求分片")

# 发 request：每个 piece 按 16 KiB 切块
np = (TOTAL + PIECE - 1) // PIECE
sent = 0
for pi in range(np):
    plen = min(PIECE, TOTAL - pi * PIECE)
    for begin in range(0, plen, BLOCK):
        ln = min(BLOCK, plen - begin)
        s.sendall(struct.pack(">IBIII", 13, 6, pi, begin, ln))
        sent += 1
print(f"已发 {sent} 个 request")

# 收 piece 消息
pieces = {}
got = 0
t0 = time.time()
s.settimeout(25)
rest = bytearray(rest)
while got < TOTAL:
    while len(rest) >= 4:
        ln = struct.unpack(">I", rest[:4])[0]
        if len(rest) < 4 + ln:
            break
        if ln == 0:
            rest = rest[4:]
            continue
        mid = rest[4]
        if mid == 7:
            idx, begin = struct.unpack(">II", bytes(rest[5:13]))
            block = bytes(rest[13:4 + ln])
            pieces[(idx, begin)] = block
            got += len(block)
        rest = rest[4 + ln:]
    if got >= TOTAL:
        break
    try:
        c = s.recv(65536)
    except socket.timeout:
        print(f"FAIL: 读超时，已收 {got}/{TOTAL} B")
        break
    if not c:
        print(f"FAIL: EOF，已收 {got}/{TOTAL} B")
        break
    rest.extend(c)

elapsed = time.time() - t0
data = b"".join(pieces[k] for k in sorted(pieces))
print(f"收到 {got} B / {TOTAL} B，块数={len(pieces)}，耗时={elapsed:.2f}s")
md5 = hashlib.md5(data).hexdigest()
print(f"重组 md5={md5}")
print(f"期望 md5={EXPECT_MD5}")
print("结论: " + ("PASS 逐字节一致" if md5 == EXPECT_MD5 else "FAIL 内容不一致"))
try:
    s.close()
except Exception:
    pass
sys.exit(0 if md5 == EXPECT_MD5 else 4)
