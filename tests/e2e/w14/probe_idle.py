"""W14 / FR-14：用真实 BT 握手唤醒持有者，再取满一片，证明 sleep 前确有 HOT 活动。

用法: python probe_idle.py <infohash-hex> <expect-md5>
"""
import hashlib
import json
import socket
import struct
import sys
import urllib.request

HTTP = "http://127.0.0.1:18600/health"
BT = ("127.0.0.1", 18602)
INFOHASH = bytes.fromhex(sys.argv[1])
EXPECT_MD5 = sys.argv[2]
PIECE = 65536
BLOCK = 16384


def health():
    with urllib.request.urlopen(HTTP, timeout=5) as r:
        return json.loads(r.read().decode())


before = health()
print("before:", json.dumps(before, ensure_ascii=False))
assert before["idle"]["enabled"] is True, "FR-14 未启用"
assert before["seed_active"] == 0, "启动后必须先是 COLD"

s = socket.create_connection(BT, timeout=10)
peer_id = b"-CR0014-" + b"w14idle".ljust(12, b"0")[:12]
s.sendall(b"\x13BitTorrent protocol" + b"\x00" * 8 + INFOHASH + peer_id)
buf = bytearray()
s.settimeout(20)
while len(buf) < 68:
    c = s.recv(4096)
    if not c:
        print("FAIL 握手 EOF")
        sys.exit(2)
    buf.extend(c)

after_wake = health()
print("after_wake:", json.dumps(after_wake, ensure_ascii=False))
assert after_wake["guard"]["woken"] > before["guard"]["woken"]
assert after_wake["seed_active"] >= 1
assert after_wake["idle"]["sleeps"] == before["idle"]["sleeps"], "休眠过早"

s.sendall(struct.pack(">IB", 1, 2))
rest = bytearray(buf[68:])
unchoked = False
s.settimeout(20)
while not unchoked:
    while len(rest) >= 4:
        ln = struct.unpack(">I", rest[:4])[0]
        if len(rest) < 4 + ln:
            break
        if rest[4] == 1:
            unchoked = True
        rest = rest[4 + ln:]
        if unchoked:
            break
    if unchoked:
        break
    c = s.recv(4096)
    if not c:
        print("FAIL 等 unchoke EOF")
        sys.exit(3)
    rest.extend(c)

for begin in range(0, PIECE, BLOCK):
    ln = min(BLOCK, PIECE - begin)
    s.sendall(struct.pack(">IBIII", 13, 6, 0, begin, ln))
blocks = {}
got = 0
s.settimeout(30)
while got < PIECE:
    while len(rest) >= 4:
        ln = struct.unpack(">I", rest[:4])[0]
        if len(rest) < 4 + ln:
            break
        if ln > 0 and rest[4] == 7:
            _, begin = struct.unpack(">II", bytes(rest[5:13]))
            blocks[begin] = bytes(rest[13:4 + ln])
            got += len(blocks[begin])
        rest = rest[4 + ln:]
    if got >= PIECE:
        break
    c = s.recv(65536)
    if not c:
        break
    rest.extend(c)

md5 = hashlib.md5(b"".join(blocks[k] for k in sorted(blocks))).hexdigest()
s.close()
print(f"piece {got}/{PIECE} md5={md5} expect={EXPECT_MD5}")
sys.exit(0 if got == PIECE and md5 == EXPECT_MD5 else 4)
