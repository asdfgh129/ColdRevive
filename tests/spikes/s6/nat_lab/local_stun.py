#!/usr/bin/env python3
"""nat_lab 本地 STUN 服务端：RFC 5389 Binding + RFC 3489 CHANGE-REQUEST。

为什么需要它（方法论前提，不是便利工具）：

  S6 要测的是**被测网关那一层 NAT** 的行为。如果从 LAN netns 里直接去问公共 STUN，
  报文还会穿过这台 VM 自己的真实出口 NAT（192.168.1.254 → 运营商）。外层 NAT 会
  改写甚至覆盖内层的映射/过滤特征，测出来的就不再是实验室里那类 NAT ——
  例如外层若是 symmetric，内层 full-cone 也会被测量成 symmetric。

  把 STUN 服务端放在实验室宿主上（WAN 网桥 10.0.0.1 / 10.0.0.2），路径就变成
  `LAN netns → 被测网关（被测 NAT）→ 宿主 STUN`，中间只有被测的那一层 NAT，
  且完全离线、不依赖公共 STUN 的可用性与限流。

实现的协议能力：
  * Binding Request → Binding Response，回 `MAPPED-ADDRESS` /
    `XOR-MAPPED-ADDRESS`（即 NAT 改写后的源地址，这正是被测对象）、
    `SOURCE-ADDRESS` / `RESPONSE-ORIGIN`（本次应答源）、
    `CHANGED-ADDRESS` / `OTHER-ADDRESS`（备用地址）。
  * 解析 `CHANGE-REQUEST`(0x0003) 的 flags：0x02 改端口、0x04 改 IP、0x06 都改，
    并从请求要求的那个 (IP, port) 回包 —— 这正是判定过滤行为（full / restricted /
    port-restricted cone）所必需的能力。

范围声明：本服务端只负责产生 STUN 应答，不做任何分类判定，也不写 spikes.json。

用法:
  python3 local_stun.py --primary 10.0.0.1:3478 --alt-ip 10.0.0.2 --alt-port 3479
  python3 local_stun.py --self-test
"""
from __future__ import annotations

import argparse
import json
import os
import selectors
import socket
import struct
import sys
import time
from pathlib import Path

COOKIE = 0x2112A442
BINDING_REQUEST = 0x0001
BINDING_RESPONSE = 0x0101

ATTR_MAPPED_ADDRESS = 0x0001
ATTR_CHANGE_REQUEST = 0x0003
ATTR_SOURCE_ADDRESS = 0x0004
ATTR_CHANGED_ADDRESS = 0x0005
ATTR_XOR_MAPPED_ADDRESS = 0x0020
ATTR_RESPONSE_ORIGIN = 0x802B
ATTR_OTHER_ADDRESS = 0x802C

FLAG_CHANGE_PORT = 0x02
FLAG_CHANGE_IP = 0x04


def enc_addr(ip: str, port: int, xor: bool = False) -> bytes:
    """MAPPED-ADDRESS 编码：保留字节 + family + port + IPv4。"""
    p = port ^ (COOKIE >> 16) if xor else port
    a = struct.unpack("!I", socket.inet_aton(ip))[0]
    if xor:
        a ^= COOKIE
    return struct.pack("!BBHI", 0, 1, p, a)


def attr(kind: int, value: bytes) -> bytes:
    return struct.pack("!HH", kind, len(value)) + value


def parse_attrs(data: bytes) -> dict[int, bytes]:
    """从完整 STUN 报文（含 20 字节头）解析属性；同名属性保留第一个。"""
    out: dict[int, bytes] = {}
    off = 20
    while off + 4 <= len(data):
        kind, length = struct.unpack_from("!HH", data, off)
        off += 4
        if off + length > len(data):
            break
        out.setdefault(kind, data[off:off + length])
        off += length + ((4 - length % 4) % 4)
    return out


class LocalStun:
    def __init__(self, primary: tuple[str, int], alt_ip: str, alt_port: int) -> None:
        self.primary = primary
        self.alt_ip = alt_ip
        self.alt_port = alt_port
        self.sel = selectors.DefaultSelector()
        self.socks: dict[tuple[str, int], socket.socket] = {}
        self.handled = 0
        self.changed_ip = 0
        self.changed_port = 0
        self.stopped = False
        for ip, port in self.endpoints():
            s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            s.bind((ip, port))
            s.setblocking(False)
            self.socks[(ip, port)] = s
            self.sel.register(s, selectors.EVENT_READ, (ip, port))

    def endpoints(self) -> list[tuple[str, int]]:
        ip0, p0 = self.primary
        return [(ip0, p0), (self.alt_ip, p0), (ip0, self.alt_port), (self.alt_ip, self.alt_port)]

    def serve_forever(self, stats_path: str | None = None, verbose: bool = False) -> None:
        last_stats = 0.0
        while not self.stopped:
            try:
                events = self.sel.select(timeout=0.5)
            except (OSError, ValueError):     # selector/socket 已被关闭（自检收尾）
                return
            for key, _ in events:
                ip, port = key.data
                sock = self.socks[(ip, port)]
                try:
                    data, src = sock.recvfrom(2048)
                except OSError:
                    continue
                self.handle(sock, (ip, port), data, src, verbose)
            if stats_path and time.time() - last_stats > 1.0:
                last_stats = time.time()
                self.write_stats(stats_path)

    def handle(self, sock: socket.socket, local: tuple[str, int], data: bytes,
               src: tuple[str, int], verbose: bool) -> None:
        if len(data) < 20:
            return
        mtype, _mlen, cookie = struct.unpack_from("!HHI", data, 0)
        if mtype != BINDING_REQUEST or cookie != COOKIE:
            return
        tid = data[8:20]
        attrs = parse_attrs(data)
        flags = 0
        if ATTR_CHANGE_REQUEST in attrs and len(attrs[ATTR_CHANGE_REQUEST]) >= 4:
            flags = struct.unpack_from("!I", attrs[ATTR_CHANGE_REQUEST], 0)[0]
        # 默认从「收到请求的那个地址」回包。这一点至关重要：NAT 的 conntrack 只
        # 认与出向流目的元组一致的入向包；若对 10.0.0.2:3478 的请求从
        # 10.0.0.1:3478 回应，回包元组不匹配，会被对称/端口受限网关当作
        # unsolicited 新入向包丢弃 —— 实测会把「可达」误判成「不可达」。
        # CHANGE-REQUEST 的语义是「换到与本地址相对的另一侧」。
        base_ip, base_port = local
        other_ip = self.alt_ip if base_ip == self.primary[0] else self.primary[0]
        other_port = self.alt_port if base_port == self.primary[1] else self.primary[1]
        want_ip = other_ip if flags & FLAG_CHANGE_IP else base_ip
        want_port = other_port if flags & FLAG_CHANGE_PORT else base_port
        origin = (want_ip, want_port)
        out_sock = self.socks.get(origin)
        if out_sock is None:                      # 请求的备用地址不存在：如实不回
            return
        if flags & FLAG_CHANGE_IP:
            self.changed_ip += 1
        if flags & FLAG_CHANGE_PORT:
            self.changed_port += 1
        self.handled += 1

        # 关键：MAPPED-ADDRESS 用「收到请求时看到的源地址」，即被 NAT 改写后的地址
        body = b"".join([
            attr(ATTR_MAPPED_ADDRESS, enc_addr(src[0], src[1], False)),
            attr(ATTR_XOR_MAPPED_ADDRESS, enc_addr(src[0], src[1], True)),
            attr(ATTR_SOURCE_ADDRESS, enc_addr(origin[0], origin[1], False)),
            attr(ATTR_CHANGED_ADDRESS, enc_addr(self.alt_ip, self.alt_port, False)),
            attr(ATTR_RESPONSE_ORIGIN, enc_addr(origin[0], origin[1], False)),
            attr(ATTR_OTHER_ADDRESS, enc_addr(self.alt_ip, self.alt_port, False)),
        ])
        resp = struct.pack("!HHI12s", BINDING_RESPONSE, len(body), COOKIE, tid) + body
        try:
            out_sock.sendto(resp, src)
        except OSError:
            return
        if verbose:
            print(f"[stun] {src[0]}:{src[1]} -> req_dst={local[0]}:{local[1]} flags={flags} "
                  f"reply_from={origin[0]}:{origin[1]} mapped={src[0]}:{src[1]}", flush=True)

    def write_stats(self, path: str) -> None:
        tmp = path + ".tmp"
        Path(tmp).write_text(json.dumps({
            "handled": self.handled,
            "answered_from_alt_ip": self.changed_ip,
            "answered_from_alt_port": self.changed_port,
            "endpoints": [f"{ip}:{p}" for ip, p in self.endpoints()],
        }) + "\n", encoding="utf-8")
        os.replace(tmp, path)


# ---------------------------------------------------------------- self-test
def self_test() -> int:
    checks = []
    v = enc_addr("10.0.0.7", 3478, False)
    checks.append(("addr_encoding_roundtrip",
                   struct.unpack("!BBHI", v) == (0, 1, 3478, 0x0A000007)))
    x = enc_addr("10.0.0.7", 3478, True)
    back_port = struct.unpack_from("!H", x, 2)[0] ^ (COOKIE >> 16)
    back_ip = struct.unpack_from("!I", x, 4)[0] ^ COOKIE
    checks.append(("xor_encoding_matches_rfc5389",
                   back_port == 3478 and socket.inet_ntoa(struct.pack("!I", back_ip)) == "10.0.0.7"))

    # CHANGE-REQUEST 解析：flags 必须原样取出
    req_body = attr(ATTR_CHANGE_REQUEST, struct.pack("!I", 6))
    req = struct.pack("!HHI12s", BINDING_REQUEST, len(req_body), COOKIE, b"A" * 12) + req_body
    pa = parse_attrs(req)
    checks.append(("change_request_parsed",
                   struct.unpack_from("!I", pa[ATTR_CHANGE_REQUEST], 0)[0] == 6))

    # 端到端：本机回环上真起服务端，验证 flags=0 从主地址回、flags=6 从备用地址回
    srv = LocalStun(("127.0.0.1", 0), "127.0.0.2", 0)
    p0 = srv.socks[("127.0.0.1", 0)].getsockname()[1]
    srv2 = LocalStun(("127.0.0.1", p0), "127.0.0.2", p0 + 1)
    for s in srv.socks.values():
        s.close()
    import threading
    t = threading.Thread(target=srv2.serve_forever, args=(None, False), daemon=True)
    t.start()
    cli = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    cli.bind(("127.0.0.1", 0))
    cli.settimeout(1.5)

    def ask(flags: int | None, dst=None):
        dst = dst or ("127.0.0.1", p0)
        body = b"" if flags is None else attr(ATTR_CHANGE_REQUEST, struct.pack("!I", flags))
        tid = os.urandom(12)
        cli.sendto(struct.pack("!HHI12s", BINDING_REQUEST, len(body), COOKIE, tid) + body, dst)
        d, s = cli.recvfrom(2048)
        return d, s

    d0, s0 = ask(None)
    a0 = parse_attrs(d0)
    checks.append(("answers_from_primary",
                   s0 == ("127.0.0.1", p0) and ATTR_MAPPED_ADDRESS in a0))
    d6, s6 = ask(6)
    checks.append(("answers_from_alternate_on_flags6", s6 == ("127.0.0.2", p0 + 1)))
    d2, s2 = ask(2)
    checks.append(("answers_from_alt_port_on_flags2", s2 == ("127.0.0.1", p0 + 1)))
    # 回归：发往备用 IP 的请求必须从备用 IP 回（否则 NAT 侧元组不匹配会丢弃回包）
    _, s_alt = ask(None, ("127.0.0.2", p0))
    checks.append(("answers_from_receiving_alt_ip", s_alt == ("127.0.0.2", p0)))
    # mapped address 必须是「客户端看到的源地址」，即客户端自己
    mp = struct.unpack("!BBHI", a0[ATTR_MAPPED_ADDRESS])
    checks.append(("mapped_address_is_observed_source",
                   socket.inet_ntoa(struct.pack("!I", mp[3])) == "127.0.0.1"
                   and mp[2] == cli.getsockname()[1]))
    cli.close()
    for s in srv2.socks.values():
        s.close()
    for name, ok in checks:
        print(f"  [{'PASS' if ok else 'FAIL'}] {name}")
    n = sum(1 for _, ok in checks if ok)
    print(f"local_stun self-test: {n}/{len(checks)} PASS (real loopback sockets)")
    return 0 if n == len(checks) else 1


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--primary", default="10.0.0.1:3478", help="主地址 ip:port")
    ap.add_argument("--alt-ip", default="10.0.0.2", help="备用 IP（须是宿主上的真实第二地址）")
    ap.add_argument("--alt-port", type=int, default=3479, help="备用端口")
    ap.add_argument("--stats-file", default="", help="每秒写一次计数（证据用）")
    ap.add_argument("--verbose", action="store_true")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return self_test()
    ip, port = args.primary.rsplit(":", 1)
    srv = LocalStun((ip, int(port)), args.alt_ip, args.alt_port)
    print("LOCAL_STUN_READY " + " ".join(f"{i}:{p}" for i, p in srv.endpoints()), flush=True)
    srv.serve_forever(args.stats_file or None, args.verbose)
    return 0


if __name__ == "__main__":
    sys.exit(main())
