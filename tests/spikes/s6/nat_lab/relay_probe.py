#!/usr/bin/env python3
"""中继帧协议探针（W3.7 中继去单点化的基础工具）。

它只做一件事：用**真实 TCP** 说 `cr-nat::relay` 的帧协议，验证中继到底把帧送到哪了。
不解析唤醒 JSON、不碰 UDP、不模拟业务 —— 和 `relay_server.rs` 的定位保持一致。

三种模式：

  pair   两个节点注册到**同一个**中继，双向投递必须成功。
         这是"该中继实例可用"的最小判据。
  cross  两个节点各注册到**不同**中继，双向跨中继投递。
         W3.8 之前实例注册表是私有的，这条必然失败（dropped 或超时）；W3.8 让两个实例
         配 --peer 互相同步注册表快照后，它必须转绿。`--repeat N` 可做连发压力观察。
  list   只探测一组中继端点是否可连（TCP 握手 + REGISTER/ACK 往返）。
  reap   先在第一台中继上挂一个长期在线的持有者，然后**重启第二台中继实例**
         （`--restart` 传入实际重启命令），等链路重建后再从第二台直投第一台。
         这条用例盯的是"注册表快照能自愈"：持有者全程没有重新注册，第二台必须
         通过重建的同侪链路重新学到它，否则帧就会被静默丢掉（dropped）。

帧格式（见 cr-nat/src/relay.rs）：
    1 字节 kind | 16 字节 node id | 4 字节大端负载长度 | 负载
    kind: 0x01 REGISTER / 0x02 RELAY / 0x03 DELIVERED / 0x04 ACK

W3.9 起中继支持 PSK 传输层（见 cr-nat/src/secure.rs）。给 `--key`（64 位十六进制或
`file:<路径>`）后，本探针会先说握手再发帧，格式必须与服务端逐字节一致：

    客户端 hello = "CRRL" | 版本(0x02) | nonce_c(32) | HMAC(psk,"CRRLY2 client-hello"+nonce_c)
    服务端 hello = 版本(0x02) | nonce_s(32) | HMAC(psk,"CRRLY2 server-hello"+nonce_c+nonce_s)
    master = HMAC(psk, "CRRLY2 master" + nonce_c + nonce_s)
    记录 = 类型(1) | 密文长度(4) | 密文 | tag(32)
    tag = HMAC(k_mac, seq(8) | 类型(1) | 长度(4) | 密文)，密钥流 = HMAC(k_enc, seq|块序号)

不给 `--key` 就是明文，只能连明文中继。

RELAY 的负载 = 16 字节目标 id + 真正的消息体。
DELIVERED 的 node 字段 = **发送方** id（不是目标 id）。

用法：
    python relay_probe.py --relay 192.168.1.209:7100 --mode pair
    python relay_probe.py --relay 192.168.1.221:7100 --relay 192.168.1.209:7100 --mode cross
    python relay_probe.py --relay 192.168.1.209:7100 --mode list
"""

from __future__ import annotations

import argparse
import hashlib
import hmac
import os
import socket
import struct
import subprocess
import sys
import time

NODE_ID_LEN = 16
HEADER_LEN = 1 + NODE_ID_LEN + 4

KIND_REGISTER = 0x01
KIND_RELAY = 0x02
KIND_DELIVERED = 0x03
KIND_ACK = 0x04

KIND_NAME = {
    KIND_REGISTER: "REGISTER",
    KIND_RELAY: "RELAY",
    KIND_DELIVERED: "DELIVERED",
    KIND_ACK: "ACK",
}


def encode(kind: int, node: bytes, payload: bytes = b"") -> bytes:
    if len(node) != NODE_ID_LEN:
        raise ValueError(f"node id 必须是 {NODE_ID_LEN} 字节，实际 {len(node)}")
    return bytes([kind]) + node + struct.pack(">I", len(payload)) + payload


def recv_exact(sock: socket.socket, n: int) -> bytes:
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise ConnectionError("对端关闭连接")
        buf += chunk
    return buf


def read_frame(sock: socket.socket) -> tuple[int, bytes, bytes]:
    head = recv_exact(sock, HEADER_LEN)
    kind = head[0]
    node = head[1 : 1 + NODE_ID_LEN]
    (length,) = struct.unpack(">I", head[1 + NODE_ID_LEN : HEADER_LEN])
    payload = recv_exact(sock, length) if length else b""
    return kind, node, payload


def parse_endpoint(text: str) -> tuple[str, int]:
    host, _, port = text.rpartition(":")
    if not host or not port:
        raise argparse.ArgumentTypeError(f"端点必须是 host:port，收到 {text!r}")
    return host, int(port)


def _hmac(key: bytes, msg: bytes) -> bytes:
    return hmac.new(key, msg, hashlib.sha256).digest()


def parse_psk(text: str) -> bytes:
    body = text.strip()
    if body.startswith("file:"):
        body = open(body[5:].strip(), "r", encoding="utf-8").read().strip()
    elif body.startswith("hex:"):
        body = body[4:].strip()
    if len(body) != 64 or any(c not in "0123456789abcdefABCDEF" for c in body):
        raise ValueError("PSK 必须是 64 位十六进制（或 file:<路径>）")
    return bytes.fromhex(body)


class SecureTransport:
    """与 `cr-nat/src/secure.rs` 逐字节一致的客户端侧握手 + 记录层。"""

    RT_DATA, RT_PING, RT_PONG = 0x00, 0x01, 0x02

    def __init__(self, sock: socket.socket, key: bytes):
        self.sock = sock
        self.key = key
        self.out_seq = 0
        self.in_seq = 0
        self.handshake()

    def handshake(self) -> None:
        nc = os.urandom(32)
        tag = _hmac(self.key, b"CRRLY2 client-hello" + nc)
        self.sock.sendall(b"CRRL" + bytes([2]) + nc + tag)
        buf = recv_exact(self.sock, 65)
        if buf[0] != 2:
            raise AssertionError(f"服务端握手版本不支持: {buf[0]}")
        ns, server_tag = buf[1:33], buf[33:65]
        want = _hmac(self.key, b"CRRLY2 server-hello" + nc + ns)
        if not hmac.compare_digest(want, server_tag):
            raise AssertionError("服务端凭证无效（PSK 不匹配或中间人）")
        master = _hmac(self.key, b"CRRLY2 master" + nc + ns)

        def dk(direction: bytes, purpose: bytes) -> bytes:
            return _hmac(master, b"CRRLY2 " + direction + b" " + purpose)

        self.out_enc, self.out_mac = dk(b"c2s", b"enc"), dk(b"c2s", b"mac")
        self.in_enc, self.in_mac = dk(b"s2c", b"enc"), dk(b"s2c", b"mac")

    @staticmethod
    def _keystream(key: bytes, seq: int, n: int) -> bytes:
        out = b""
        block = 0
        while len(out) < n:
            out += _hmac(key, struct.pack(">Q", seq) + struct.pack(">I", block))
            block += 1
        return out[:n]

    def send_record(self, rtype: int, plain: bytes) -> None:
        ks = self._keystream(self.out_enc, self.out_seq, len(plain))
        ct = bytes(a ^ b for a, b in zip(plain, ks))
        tag = _hmac(self.out_mac,
                    struct.pack(">Q", self.out_seq) + bytes([rtype])
                    + struct.pack(">I", len(ct)) + ct)
        self.sock.sendall(bytes([rtype]) + struct.pack(">I", len(ct)) + ct + tag)
        self.out_seq += 1

    def recv_record(self) -> tuple[int, bytes]:
        head = recv_exact(self.sock, 5)
        rtype = head[0]
        (length,) = struct.unpack(">I", head[1:5])
        ct = recv_exact(self.sock, length) if length else b""
        tag = recv_exact(self.sock, 32)
        want = _hmac(self.in_mac,
                     struct.pack(">Q", self.in_seq) + bytes([rtype])
                     + struct.pack(">I", length) + ct)
        if not hmac.compare_digest(want, tag):
            raise AssertionError(f"记录 tag 校验失败（seq={self.in_seq} type={rtype}）")
        seq = self.in_seq
        self.in_seq += 1
        ks = self._keystream(self.in_enc, seq, len(ct))
        return rtype, bytes(a ^ b for a, b in zip(ct, ks))

    def recv_frame(self) -> tuple[int, bytes, bytes]:
        while True:
            rtype, plain = self.recv_record()
            if rtype == self.RT_PING:
                self.send_record(self.RT_PONG, b"")   # 服务端空闲探活，必须回
                continue
            if rtype == self.RT_PONG:
                continue
            if rtype != self.RT_DATA:
                raise AssertionError(f"非法记录类型 {rtype}")
            if len(plain) < HEADER_LEN:
                raise AssertionError("记录里的帧不完整")
            kind = plain[0]
            node = plain[1 : 1 + NODE_ID_LEN]
            (length,) = struct.unpack(">I", plain[1 + NODE_ID_LEN : HEADER_LEN])
            return kind, node, plain[HEADER_LEN : HEADER_LEN + length]


# 进程级：命令行给的 PSK（None = 明文）。放在模块级是为了不改动各模式已有的函数签名。
KEY: bytes | None = None


class Peer:
    """一条到中继的 TCP 连接 + 它的节点 id。"""

    def __init__(self, endpoint: tuple[str, int], node_id: bytes, timeout: float):
        self.endpoint = endpoint
        self.node_id = node_id
        self.label = node_id.rstrip(b"\x00").decode(errors="replace")
        self.sock = socket.create_connection(endpoint, timeout=timeout)
        self.sock.settimeout(timeout)
        self.secure = SecureTransport(self.sock, KEY) if KEY else None
        self.send_frame(KIND_REGISTER, node_id, node_id)

    def send_frame(self, kind: int, node: bytes, payload: bytes = b"") -> None:
        raw = encode(kind, node, payload)
        if self.secure:
            self.secure.send_record(self.secure.RT_DATA, raw)
        else:
            self.sock.sendall(raw)

    def read_frame(self) -> tuple[int, bytes, bytes]:
        if self.secure:
            return self.secure.recv_frame()
        return read_frame(self.sock)

    def expect_ack(self) -> float:
        t0 = time.perf_counter()
        kind, _, _ = self.read_frame()
        rtt_ms = (time.perf_counter() - t0) * 1000
        if kind != KIND_ACK:
            raise AssertionError(f"{self.label}: 期待 ACK，收到 {KIND_NAME.get(kind, hex(kind))}")
        return rtt_ms

    def send_relay(self, target: bytes, message: bytes) -> None:
        self.send_frame(KIND_RELAY, self.node_id, target + message)

    def expect_delivered(self, timeout: float) -> tuple[bytes, bytes, float]:
        self.sock.settimeout(timeout)
        t0 = time.perf_counter()
        kind, node, payload = self.read_frame()
        rtt_ms = (time.perf_counter() - t0) * 1000
        if kind != KIND_DELIVERED:
            raise AssertionError(
                f"{self.label}: 期待 DELIVERED，收到 {KIND_NAME.get(kind, hex(kind))}"
            )
        return node, payload, rtt_ms

    def close(self) -> None:
        try:
            self.sock.close()
        except OSError:
            pass


def id_for(label: str) -> bytes:
    raw = label.encode()
    if len(raw) > NODE_ID_LEN:
        raise ValueError(f"label 太长: {label}")
    return raw.ljust(NODE_ID_LEN, b"\x00")


def mode_list(endpoints: list[tuple[str, int]], timeout: float) -> int:
    print("中继端点可连性（真实 TCP 握手 + REGISTER/ACK 往返）")
    rc = 0
    for idx, ep in enumerate(endpoints):
        label = f"list{idx}"
        try:
            peer = Peer(ep, id_for(label), timeout)
            rtt = peer.expect_ack()
            peer.close()
            print(f"  [OK]   {ep[0]}:{ep[1]:<5} ACK 往返 {rtt:.1f} ms")
        except Exception as exc:  # noqa: BLE001 - 探针要如实报告任何失败形态
            rc = 1
            print(f"  [FAIL] {ep[0]}:{ep[1]:<5} {type(exc).__name__}: {exc}")
    return rc


def mode_pair(endpoint: tuple[str, int], timeout: float) -> int:
    print(f"同中继双向投递 @ {endpoint[0]}:{endpoint[1]}")
    try:
        a = Peer(endpoint, id_for("probe-A"), timeout)
        b = Peer(endpoint, id_for("probe-B"), timeout)
    except Exception as exc:  # noqa: BLE001 - 连不上/握手被拒都要如实报，不能抛栈
        print(f"  [FAIL] 建立连接失败: {type(exc).__name__}: {exc}")
        print("  结论: FAIL")
        return 1
    ok = True
    try:
        ack_a = a.expect_ack()
        ack_b = b.expect_ack()
        print(f"  注册 ACK: A={ack_a:.1f} ms  B={ack_b:.1f} ms")

        # B → A
        b.send_relay(a.node_id, b"WAKEUP_REQ")
        node, payload, rtt = a.expect_delivered(timeout)
        if node != b.node_id:
            print(f"  [FAIL] DELIVERED 的 node 不是真实发送方（收到 {node!r}）")
            ok = False
        elif payload != b"WAKEUP_REQ":
            print(f"  [FAIL] 负载被改写: {payload!r}")
            ok = False
        else:
            print(f"  B→A OK  {rtt:.1f} ms  负载 {payload!r}")

        # A → B
        a.send_relay(b.node_id, b"WAKEUP_ACK")
        node, payload, rtt = b.expect_delivered(timeout)
        if node != a.node_id or payload != b"WAKEUP_ACK":
            print(f"  [FAIL] A→B node={node!r} payload={payload!r}")
            ok = False
        else:
            print(f"  A→B OK  {rtt:.1f} ms  负载 {payload!r}")
    except Exception as exc:  # noqa: BLE001 - 明文连加密服务端会卡到超时，必须如实报
        ok = False
        print(f"  [FAIL] {type(exc).__name__}: {exc}")
    finally:
        a.close()
        b.close()
    print("  结论:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def mode_cross(endpoints: list[tuple[str, int]], timeout: float, repeat: int = 1) -> int:
    """跨中继双向投递。

    返回码语义（与 mode_pair 一致：0 = 通过）：
      双向都成功  → 0（说明注册表同步生效）
      任一方向失败 → 1（未配 --peer / 快照未同步 / 转发丢失）

    判据比"收到了"更严：
      · DELIVERED 的 node 必须等于**真实发送方** id（不能是被转发的实例 id）；
      · 负载必须逐字节等于发出的消息（不能被改写或截断）；
      · 双向都要成立——单向能通只说明转发链路存在，说明不了回程。
    """
    if len(endpoints) < 2:
        print("cross 模式需要至少两个 --relay")
        return 2
    (h1, p1), (h2, p2) = endpoints[0], endpoints[1]
    print(f"跨中继双向投递：A@{h1}:{p1}  B@{h2}:{p2}  repeat={repeat}")
    a = Peer(endpoints[0], id_for("cross-A"), timeout)
    b = Peer(endpoints[1], id_for("cross-B"), timeout)
    ok = True
    rtts: list[float] = []
    try:
        print(f"  注册 ACK: A={a.expect_ack():.1f} ms  B={b.expect_ack():.1f} ms")
        for i in range(repeat):
            tag = f"#{i + 1}"
            # B(在 h2) → A(在 h1)：必须由 h2 转发
            msg = f"WAKEUP_REQ-{i}".encode()
            b.send_relay(a.node_id, msg)
            try:
                node, payload, rtt = a.expect_delivered(min(timeout, 3.0))
            except (socket.timeout, TimeoutError, AssertionError, ConnectionError) as exc:
                ok = False
                print(f"  [FAIL] {tag} B→A 未收到 DELIVERED: {type(exc).__name__}: {exc}")
                break
            if node != b.node_id:
                ok = False
                print(f"  [FAIL] {tag} B→A 来源被改写成 {node!r}（应为 {b.node_id!r}）")
                break
            if payload != msg:
                ok = False
                print(f"  [FAIL] {tag} B→A 负载被改写: {payload!r}")
                break
            rtts.append(rtt)

            # A(在 h1) → B(在 h2)：回程必须由 h1 转发
            ack = f"WAKEUP_ACK-{i}".encode()
            a.send_relay(b.node_id, ack)
            try:
                node, payload, rtt = b.expect_delivered(min(timeout, 3.0))
            except (socket.timeout, TimeoutError, AssertionError, ConnectionError) as exc:
                ok = False
                print(f"  [FAIL] {tag} A→B 未收到 DELIVERED: {type(exc).__name__}: {exc}")
                break
            if node != a.node_id or payload != ack:
                ok = False
                print(f"  [FAIL] {tag} A→B node={node!r} payload={payload!r}")
                break
            rtts.append(rtt)
    finally:
        a.close()
        b.close()
    if ok and rtts:
        print(f"  双向 {len(rtts)} 帧全部送达：min={min(rtts):.1f} max={max(rtts):.1f} "
              f"avg={sum(rtts) / len(rtts):.1f} ms")
    print("  结论:", "PASS（跨实例注册表已同步）" if ok else "FAIL（未同步或转发丢失）")
    return 0 if ok else 1


def mode_reap(endpoints: list[tuple[str, int]], timeout: float,
              restart_cmd: str, wait: float) -> int:
    """持有者在线不动，重启另一台中继，验证注册表快照的自愈能力。

    失败形态值得单独点名：链路断了但快照没重建时，帧会被算成 forwarded 然后消失，
    调用侧只能看到超时——比直接报错更难查。所以这里要求**真实 DELIVERED 且来源正确**。
    """
    if len(endpoints) < 2:
        print("reap 模式需要两个 --relay")
        return 2
    if not restart_cmd:
        print("reap 模式必须给 --restart（真正重启第二台实例的命令）")
        return 2
    (h1, p1), (h2, p2) = endpoints[0], endpoints[1]
    print(f"快照自愈：持有者 A 常驻 @{h1}:{p1}；重启 B @{h2}:{p2}")
    holder = Peer(endpoints[0], id_for("reap-hold"), timeout)
    ok = True
    try:
        print(f"  A 注册 ACK {holder.expect_ack():.1f} ms（此后不再重注册）")
        print(f"  执行重启: {restart_cmd}")
        rc = subprocess.run(restart_cmd, shell=True, capture_output=True, text=True)
        if rc.returncode != 0:
            ok = False
            print(f"  [FAIL] 重启命令返回 {rc.returncode}: {(rc.stderr or rc.stdout).strip()[:300]}")
        if ok:
            time.sleep(wait)
            # 新客户端注册到**刚重启的**第二台，向从未在它上面注册过的 A 发送。
            sender = Peer(endpoints[1], id_for("reap-send"), timeout)
            try:
                print(f"  B 注册 ACK {sender.expect_ack():.1f} ms")
                sender.send_relay(holder.node_id, b"WAKEUP_REQ")
                node, payload, rtt = holder.expect_delivered(min(timeout, 5.0))
                if node != sender.node_id or payload != b"WAKEUP_REQ":
                    ok = False
                    print(f"  [FAIL] 来源/负载不符: node={node!r} payload={payload!r}")
                else:
                    print(f"  重启后跨实例投递成功 {rtt:.1f} ms（快照已自愈）")
            finally:
                sender.close()
    finally:
        holder.close()
    print("  结论:", "PASS（同侪链路重建后快照自愈）" if ok else "FAIL（重启后帧丢失）")
    return 0 if ok else 1


def mode_latency(endpoint: tuple[str, int], timeout: float, repeat: int) -> int:
    """同一条连接上的往返时延：注册一次，然后发 N 次、收 N 次。

    这条用例是给「中继放到公网」用的：LAN 里 0.3 ms 说明不了跨公网可用性，
    只有把公网入口（例如 `112.5.32.34:57100`）填进来，数字才有意义。
    统计给 min / p50 / max / avg，样本不足或任何一次丢失都判 FAIL。
    """
    print(f"中继往返时延 @ {endpoint[0]}:{endpoint[1]}  repeat={repeat}  "
          f"auth={'psk' if KEY else 'none'}")
    try:
        a = Peer(endpoint, id_for("lat-A"), timeout)
        b = Peer(endpoint, id_for("lat-B"), timeout)
    except Exception as exc:  # noqa: BLE001
        print(f"  [FAIL] 建立连接失败: {type(exc).__name__}: {exc}")
        print("  结论: FAIL")
        return 1
    ok = True
    rtts: list[float] = []
    try:
        ack_a = a.expect_ack()
        ack_b = b.expect_ack()
        print(f"  握手/注册 ACK: A={ack_a:.1f} ms  B={ack_b:.1f} ms")
        for i in range(repeat):
            msg = f"PING-{i}".encode()
            b.send_relay(a.node_id, msg)
            try:
                node, payload, rtt = a.expect_delivered(max(1.0, min(timeout, 5.0)))
            except Exception as exc:  # noqa: BLE001
                ok = False
                print(f"  [FAIL] #{i + 1} 未收到 DELIVERED: {type(exc).__name__}: {exc}")
                break
            if node != b.node_id or payload != msg:
                ok = False
                print(f"  [FAIL] #{i + 1} 来源/负载不符: node={node!r} payload={payload!r}")
                break
            rtts.append(rtt)
            time.sleep(0.05)
    except Exception as exc:  # noqa: BLE001
        ok = False
        print(f"  [FAIL] {type(exc).__name__}: {exc}")
    finally:
        a.close()
        b.close()
    if rtts:
        ordered = sorted(rtts)
        p50 = ordered[len(ordered) // 2]
        print(f"  样本 {len(rtts)}/{repeat}  min {min(rtts):.2f}  p50 {p50:.2f}  "
              f"max {max(rtts):.2f}  avg {sum(rtts) / len(rtts):.2f} ms")
    print("  结论:", "PASS" if ok and len(rtts) == repeat else "FAIL")
    return 0 if ok and len(rtts) == repeat else 1


def main() -> int:
    ap = argparse.ArgumentParser(description="中继帧协议探针")
    ap.add_argument("--relay", action="append", required=True, metavar="HOST:PORT",
                    help="中继端点，可重复")
    ap.add_argument("--mode", choices=("pair", "cross", "list", "reap", "latency"),
                    default="pair")
    ap.add_argument("--timeout", type=float, default=5.0)
    ap.add_argument("--repeat", type=int, default=1, help="cross 模式双向各发多少帧")
    ap.add_argument("--restart", default="", help="reap 模式：重启第二台中继的 shell 命令")
    ap.add_argument("--reap-wait", type=float, default=6.0,
                    help="reap 模式：重启后等待链路重建/快照重推的秒数")
    ap.add_argument("--key", default="", metavar="64HEX|file:PATH",
                    help="中继 PSK；不给就是明文（只能连明文中继）")
    ap.add_argument("--key-file", default="", help="等价于 --key file:<路径>")
    args = ap.parse_args()

    global KEY
    spec = args.key or (f"file:{args.key_file}" if args.key_file else "")
    if spec:
        try:
            KEY = parse_psk(spec)
        except (OSError, ValueError) as exc:
            print(f"--key 非法: {exc}", file=sys.stderr)
            return 2
        print(f"传输层: PSK（指纹 {hashlib.sha256(KEY).hexdigest()[:16]}）")
    else:
        print("传输层: 明文（未给 --key）")

    endpoints = [parse_endpoint(r) for r in args.relay]
    if args.mode == "list":
        return mode_list(endpoints, args.timeout)
    if args.mode == "cross":
        return mode_cross(endpoints, args.timeout, max(1, args.repeat))
    if args.mode == "reap":
        return mode_reap(endpoints, args.timeout, args.restart, args.reap_wait)
    if args.mode == "latency":
        if len(endpoints) != 1:
            print("latency 模式只接受一个 --relay", file=sys.stderr)
            return 2
        return mode_latency(endpoints[0], args.timeout, max(1, args.repeat))
    if len(endpoints) != 1:
        print("pair 模式只接受一个 --relay", file=sys.stderr)
        return 2
    return mode_pair(endpoints[0], args.timeout)


if __name__ == "__main__":
    sys.exit(main())
