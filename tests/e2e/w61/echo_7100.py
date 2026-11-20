#!/usr/bin/env python3
"""W61 对照用的纯 TCP 回显服务。

只用于证明 119.29.150.254 的非 22 入向 TCP 在数据阶段仍被注入 RST。
故意不引入项目代码：ADR-020 §3.1 已证明这是路径问题，不是帧协议问题。
"""
import socket
import sys
import threading


def handle(conn, addr):
    try:
        conn.settimeout(5)
        while True:
            data = conn.recv(4096)
            if not data:
                break
            conn.sendall(data)
    except Exception as exc:  # noqa: BLE001 - 回显服务需要记录原始错误
        print(f"session {addr}: {type(exc).__name__}: {exc}", flush=True)
    finally:
        conn.close()


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 7100
    srv = socket.socket()
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("0.0.0.0", port))
    srv.listen(16)
    print(f"ECHO_READY port={port}", flush=True)
    while True:
        conn, addr = srv.accept()
        threading.Thread(target=handle, args=(conn, addr), daemon=True).start()


if __name__ == "__main__":
    main()
