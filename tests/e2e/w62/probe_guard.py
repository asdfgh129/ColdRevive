"""W62 / FR-11：真 socket 证明「旧客户端直接连入 bt_port → 握手 infohash → 唤醒对应样本」。

不调用私有唤醒 UDP，只发标准 68 字节 BT 握手；随后发 interested/request，
收满分片算 md5，证明 guard 补回了已读的握手字节（不是只唤醒却断了数据面）。

用法: python probe_guard.py <infohash-hex> <expect-md5> [piece_size] [total]
"""
import hashlib
import json
import socket
import struct
import sys
import time
import urllib.request

HTTP = "http://127.0.0.1:18800"
BT = ("127.0.0.1", 18802)
INFOHASH = sys.argv[1]
EXPECT_MD5 = sys.argv[2]
PIECE = int(sys.argv[3]) if len(sys.argv) > 3 else 262144
TOTAL = int(sys.argv[4]) if len(sys.argv) > 4 else 262144
BLOCK = 16384


def health():
    with urllib.request.urlopen(HTTP + "/health", timeout=5) as r:
        return json.loads(r.read().decode())


before = health()
print("before guard=", before["guard"], "seed_active=", before["seed_active"])
assert before["guard"]["enabled"], "guard 未启用（配置没生效）"

s = socket.create_connection(BT, timeout=10)
ih = bytes.fromhex(INFOHASH)
peer_id = b"-CR0002-" + b"w62guard".ljust(12, b"0")[:12]
s.sendall(b"\x13BitTorrent protocol" + b"\x00" * 8 + ih + peer_id)

# 等握手回包：若 guard 没补回 68 B，引擎看到的握手被截断 → 这里 EOF。
buf = bytearray()
s.settimeout(20)
while len(buf) < 68:
    c = s.recv(4096)
    if not c:
        print("FAIL 握手阶段 EOF，guard 很可能丢了已读的 68 B")
        sys.exit(2)
    buf.extend(c)
print("握手回包 peer_id=", bytes(buf[48:68]))

after_wake = health()
print("after  guard=", after_wake["guard"], "seed_active=", after_wake["seed_active"])
assert after_wake["guard"]["handshakes"] > before["guard"]["handshakes"], "guard 没记到本次握手"
assert after_wake["guard"]["woken"] > before["guard"]["woken"], "握手没有唤醒本地任务"
assert after_wake["seed_active"] >= 1, "/health 看不到活跃做种任务"

# interested → unchoke
s.sendall(struct.pack(">IB", 1, 2))
rest = bytearray(buf[68:])
unchoked = False
s.settimeout(20)
while not unchoked:
    while len(rest) >= 4:
        ln = struct.unpack(">I", rest[:4])[0]
        if len(rest) < 4 + ln:
            break
        mid = rest[4] if ln > 0 else None
        if mid == 1:
            unchoked = True
        rest = rest[4 + ln:]
        if unchoked:
            break
    if unchoked:
        break
    c = s.recv(4096)
    if not c:
        print("FAIL 等 unchoke 时 EOF")
        sys.exit(3)
    rest.extend(c)
print("已 unchoke")

# request 全部分片
np = (TOTAL + PIECE - 1) // PIECE
for pi in range(np):
    plen = min(PIECE, TOTAL - pi * PIECE)
    for begin in range(0, plen, BLOCK):
        ln = min(BLOCK, plen - begin)
        s.sendall(struct.pack(">IBIII", 13, 6, pi, begin, ln))

pieces = {}
got = 0
s.settimeout(30)
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
        print(f"FAIL 读超时 {got}/{TOTAL}")
        break
    if not c:
        print(f"FAIL EOF {got}/{TOTAL}")
        break
    rest.extend(c)

data = b"".join(pieces[k] for k in sorted(pieces))
md5 = hashlib.md5(data).hexdigest()
print(f"收到 {got}/{TOTAL} B 块数={len(pieces)} md5={md5} 期望={EXPECT_MD5}")
s.close()
sys.exit(0 if got == TOTAL and md5 == EXPECT_MD5 else 4)
