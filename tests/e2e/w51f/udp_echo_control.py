#!/usr/bin/env python3
"""UW5.1f 的「量具」：在 A 的 UDP 端口上做一次回显，用来单独验证 CPE 的 UDP 映射。

它**不参与** µTP 的结论，只回答一个问题：`<公网IP>:<bt_port>/udp` 的入向映射到底有没有。
把这个和「µTP 能不能传」分开测，是因为两者失败的形态一模一样（客户端一个字节都拿不到），
混在一起就只能猜。

输出（每行）：
    LISTEN <port>
    GOT <src_ip>:<src_port> <payload repr>   ← src_ip 是 NAT 之后的公网地址
    DONE
"""
from __future__ import annotations

import socket
import sys
import time


def main() -> int:
    port = int(sys.argv[1])
    duration = float(sys.argv[2]) if len(sys.argv) > 2 else 60.0
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("0.0.0.0", port))
    s.settimeout(0.5)
    deadline = time.time() + duration
    print(f"LISTEN {port}", flush=True)
    while time.time() < deadline:
        try:
            data, addr = s.recvfrom(2048)
        except socket.timeout:
            continue
        except OSError as e:
            print(f"RECV_ERR {e}", flush=True)
            break
        print(f"GOT {addr[0]}:{addr[1]} {data!r}", flush=True)
        try:
            s.sendto(b"ECHO:" + data, addr)
        except OSError as e:
            print(f"SEND_ERR {e}", flush=True)
    print("DONE", flush=True)
    s.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
