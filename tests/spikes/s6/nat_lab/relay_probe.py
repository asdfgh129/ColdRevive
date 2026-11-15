#!/usr/bin/env python3
"""中继帧协议探针（W3.7 中继去单点化的基础工具）。

它只做一件事：用**真实 TCP** 说 `cr-nat::relay` 的帧协议，验证中继到底把帧送到哪了。
不解析唤醒 JSON、不碰 UDP、不模拟业务 —— 和 `relay_server.rs` 的定位保持一致。

三种模式：

  pair   两个节点注册到**同一个**中继，双向投递必须成功。
         这是"该中继实例可用"的最小判据。
  cross  两个节点各注册到**不同**中继，跨中继投递。
         当前实现**没有注册表同步**，所以这里应当失败（dropped 或超时）。
         把它保留成一条会红的用例，是为了让"去单点化还没做"这件事有可执行的证据，
         而不是停留在文档里的一句话。
  list   只探测一组中继端点是否可连（TCP 握手 + REGISTER/ACK 往返）。

帧格式（见 cr-nat/src/relay.rs）：
    1 字节 kind | 16 字节 node id | 4 字节大端负载长度 | 负载
    kind: 0x01 REGISTER / 0x02 RELAY / 0x03 DELIVERED / 0x04 ACK

RELAY 的负载 = 16 字节目标 id + 真正的消息体。
DELIVERED 的 node 字段 = **发送方** id（不是目标 id）。

用法：
    python relay_probe.py --relay 192.168.1.209:7100 --mode pair
    python relay_probe.py --relay 192.168.1.221:7100 --relay 192.168.1.209:7100 --mode cross
    python relay_probe.py --relay 192.168.1.209:7100 --mode list
"""

from __future__ import annotations

import argparse
import socket
import struct
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


class Peer:
    """一条到中继的 TCP 连接 + 它的节点 id。"""

    def __init__(self, endpoint: tuple[str, int], node_id: bytes, timeout: float):
        self.endpoint = endpoint
        self.node_id = node_id
        self.label = node_id.rstrip(b"\x00").decode(errors="replace")
        self.sock = socket.create_connection(endpoint, timeout=timeout)
        self.sock.settimeout(timeout)
        self.sock.sendall(encode(KIND_REGISTER, node_id, node_id))

    def expect_ack(self) -> float:
        t0 = time.perf_counter()
        kind, _, _ = read_frame(self.sock)
        rtt_ms = (time.perf_counter() - t0) * 1000
        if kind != KIND_ACK:
            raise AssertionError(f"{self.label}: 期待 ACK，收到 {KIND_NAME.get(kind, hex(kind))}")
        return rtt_ms

    def send_relay(self, target: bytes, message: bytes) -> None:
        self.sock.sendall(encode(KIND_RELAY, self.node_id, target + message))

    def expect_delivered(self, timeout: float) -> tuple[bytes, bytes, float]:
        self.sock.settimeout(timeout)
        t0 = time.perf_counter()
        kind, node, payload = read_frame(self.sock)
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
    a = Peer(endpoint, id_for("probe-A"), timeout)
    b = Peer(endpoint, id_for("probe-B"), timeout)
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
    finally:
        a.close()
        b.close()
    print("  结论:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def mode_cross(endpoints: list[tuple[str, int]], timeout: float) -> int:
    """跨中继投递。当前实现没有注册表同步，预期失败。

    返回码语义（与 mode_pair 一致：0 = 通过）：
      投递成功  → 0（说明注册表同步已经生效，这条用例该转绿了）
      投递失败  → 1（当前预期结果；在测试脚本里按"预期失败"消费）
    """
    if len(endpoints) < 2:
        print("cross 模式需要至少两个 --relay")
        return 2
    (h1, p1), (h2, p2) = endpoints[0], endpoints[1]
    print(f"跨中继投递：A@{h1}:{p1}  B@{h2}:{p2}")
    a = Peer(endpoints[0], id_for("cross-A"), timeout)
    b = Peer(endpoints[1], id_for("cross-B"), timeout)
    delivered = False
    try:
        print(f"  注册 ACK: A={a.expect_ack():.1f} ms  B={b.expect_ack():.1f} ms")
        b.send_relay(a.node_id, b"WAKEUP_REQ")
        try:
            node, payload, rtt = a.expect_delivered(min(timeout, 3.0))
            delivered = True
            print(f"  跨中继投递成功 {rtt:.1f} ms  来源={node!r} 负载={payload!r}")
        except (socket.timeout, TimeoutError):
            print(f"  A 在 {min(timeout, 3.0):.1f}s 内没有收到 DELIVERED（注册表未同步）")
        except (AssertionError, ConnectionError) as exc:
            print(f"  未收到 DELIVERED: {exc}")
    finally:
        a.close()
        b.close()
    print("  结论:", "已同步（投递成功）" if delivered else "未同步（当前实现的预期结果）")
    return 0 if delivered else 1


def main() -> int:
    ap = argparse.ArgumentParser(description="中继帧协议探针")
    ap.add_argument("--relay", action="append", required=True, metavar="HOST:PORT",
                    help="中继端点，可重复")
    ap.add_argument("--mode", choices=("pair", "cross", "list"), default="pair")
    ap.add_argument("--timeout", type=float, default=5.0)
    args = ap.parse_args()

    endpoints = [parse_endpoint(r) for r in args.relay]
    if args.mode == "list":
        return mode_list(endpoints, args.timeout)
    if args.mode == "cross":
        return mode_cross(endpoints, args.timeout)
    if len(endpoints) != 1:
        print("pair 模式只接受一个 --relay", file=sys.stderr)
        return 2
    return mode_pair(endpoints[0], args.timeout)


if __name__ == "__main__":
    sys.exit(main())
