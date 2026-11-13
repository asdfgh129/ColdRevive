#!/usr/bin/env python3
"""S6 先导实测：在**本机真实网络出口**上取 NAT 矩阵的第一个真实样本。

范围声明（重要）：
  * 只测 1 个 NAT 环境（本机当前出口），**不等于** S6 判据要求的「NAT 类型矩阵实测」，
    因此 `evidence_valid_for_pass=false`，脚本不写 spikes.json，S6 仍须由实验执行人判定。
  * 目的是把 nat-matrix.md 中纯分析型矩阵替换为「分析 + 真实先导样本」，并验证
    穿透栈第一层（STUN / UPnP IGD / NAT-PMP）在本环境是否真的可用。
  * 所有探测均为标准、单次、低速报文（STUN Binding / SSDP M-SEARCH / NAT-PMP address query），
    不向公共 KAD / DHT 发布任何测试 Key。

协议：
  * RFC 5389 STUN Binding Request，解析 XOR-MAPPED-ADDRESS / MAPPED-ADDRESS / OTHER-ADDRESS；
    用同一本地 UDP socket 访问多个不同 IP:port 的 STUN 服务器，比较映射后的 (ip, port)：
    相同 → 非对称 NAT（cone 类）；port 不同 → 对称 NAT。
  * SSDP M-SEARCH（239.255.255.250:1900，ST=InternetGatewayDevice:1）探测 UPnP IGD。
  * NAT-PMP 外部地址请求（网关:5351，opcode 0）探测 NAT-PMP/PCP 可用性。
  * CGNAT 判据：本机地址落在 100.64/10，或到公网第二跳仍是私有地址（tracert -h 2）。

用法: python3 stun_probe.py [--timeout 2.0] [--out <path>]
退出码 0 = 探测完成（含全部超时也视为完成）；1 = 脚本自身失败。
"""
from __future__ import annotations

import argparse
import ipaddress
import json
import os
import re
import socket
import struct
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

COOKIE = 0x2112A442
BINDING_REQUEST = 0x0001
BINDING_RESPONSE = 0x0101
STUN_SERVERS = [
    ("stun.l.google.com", 19302),
    ("stun1.l.google.com", 19302),
    ("stun2.l.google.com", 19302),
    ("stun.cloudflare.com", 3478),
    ("stun.ekiga.net", 3478),
    ("stun.stunprotocol.org", 3478),
]
PRIVATE_NETS = [ipaddress.ip_network(n) for n in
                ("10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "100.64.0.0/10",
                 "127.0.0.0/8", "169.254.0.0/16", "fc00::/7")]


def is_private(ip: str) -> bool:
    try:
        a = ipaddress.ip_address(ip)
        return any(a in n for n in PRIVATE_NETS)
    except ValueError:
        return False


def parse_attrs(data: bytes) -> dict:
    """解析 STUN 属性，返回 {type_int: [raw_value, ...]}。"""
    out: dict[int, list[bytes]] = {}
    if len(data) < 20:
        return out
    pos = 20
    while pos + 4 <= len(data):
        atype, alen = struct.unpack_from("!HH", data, pos)
        pos += 4
        if pos + alen > len(data):
            break
        out.setdefault(atype, []).append(data[pos:pos + alen])
        pos += alen + ((-alen) % 4)
    return out


def decode_mapped(value: bytes, xor: bool) -> str | None:
    # RFC 5389 §15.1: family 0x01 = IPv4, 0x02 = IPv6（首字节为保留位）
    if len(value) < 8 or value[1] != 0x01:  # IPv4 only
        return None
    port = struct.unpack_from("!H", value, 2)[0]
    raw = struct.unpack_from("!I", value, 4)[0]
    if xor:
        port ^= (COOKIE >> 16) & 0xFFFF
        raw ^= COOKIE
    return f"{socket.inet_ntoa(struct.pack('!I', raw))}:{port}"


def stun_query(sock: socket.socket, host: str, port: int, timeout: float) -> dict:
    tid = os.urandom(12)
    req = struct.pack("!HHI12s", BINDING_REQUEST, 0, COOKIE, tid)
    row = {"server": f"{host}:{port}", "resolved": "", "ok": False}
    try:
        infos = socket.getaddrinfo(host, port, socket.AF_INET, socket.SOCK_DGRAM)
        addr = infos[0][4]
        row["resolved"] = f"{addr[0]}:{addr[1]}"
    except OSError as e:
        row["error"] = f"dns: {e}"
        return row
    t0 = time.perf_counter()
    try:
        sock.sendto(req, addr)
        sock.settimeout(timeout)
        data, _ = sock.recvfrom(2048)
    except socket.timeout:
        row["error"] = "timeout"
        row["rtt_ms"] = round((time.perf_counter() - t0) * 1000, 1)
        return row
    except OSError as e:
        row["error"] = f"socket: {e}"
        return row
    row["rtt_ms"] = round((time.perf_counter() - t0) * 1000, 1)
    if len(data) < 20:
        row["error"] = "short response"
        return row
    mtype, mlen, magic = struct.unpack_from("!HHI", data, 0)
    if mtype != BINDING_RESPONSE or magic != COOKIE or data[8:20] != tid:
        row["error"] = "not matching binding response"
        return row
    attrs = parse_attrs(data)
    xor_vals = attrs.get(0x0020, [])
    plain_vals = attrs.get(0x0001, [])
    mapped = decode_mapped(xor_vals[0], True) if xor_vals else (
        decode_mapped(plain_vals[0], False) if plain_vals else None)
    row["ok"] = mapped is not None
    if mapped is None:
        row["error"] = "unsupported address family or malformed XOR-MAPPED-ADDRESS"
    row["mapped_address"] = mapped
    row["attributes_hex"] = [f"0x{k:04x}" for k in attrs]
    others = attrs.get(0x802C, []) + attrs.get(0x0005, [])
    row["other_address"] = decode_mapped(others[0], False) if others else None
    row["response_bytes"] = len(data)
    return row


def local_ip() -> str:
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        s.connect(("8.8.8.8", 9))  # UDP connect 不发包，只选路
        return s.getsockname()[0]
    except OSError:
        return ""
    finally:
        s.close()


def default_gateway() -> str:
    try:
        r = subprocess.run(["route", "print", "0.0.0.0"], capture_output=True,
                           text=True, timeout=6, check=False)
        for line in (r.stdout or "").splitlines():
            m = re.match(r"\s*0\.0\.0\.0\s+0\.0\.0\.0\s+(\d+\.\d+\.\d+\.\d+)\s", line)
            if m:
                return m.group(1)
    except (OSError, subprocess.TimeoutExpired):
        pass
    return ""


def upnp_discover(timeout: float) -> dict:
    msg = (b"M-SEARCH * HTTP/1.1\r\n"
           b"HOST: 239.255.255.250:1900\r\n"
           b'MAN: "ssdp:discover"\r\n'
           b"MX: 2\r\n"
           b"ST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\r\n")
    out: dict = {"available": False, "responses": [], "error": ""}
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        s.settimeout(timeout)
        s.bind(("", 0))
        s.sendto(msg, ("239.255.255.250", 1900))
        end = time.perf_counter() + timeout
        while time.perf_counter() < end:
            try:
                data, peer = s.recvfrom(4096)
            except socket.timeout:
                break
            text = data.decode("utf-8", "replace")
            loc = re.search(r"LOCATION:\s*(\S+)", text, re.I)
            srv = re.search(r"SERVER:\s*(.+)", text, re.I)
            out["responses"].append({
                "from": f"{peer[0]}:{peer[1]}",
                "location": loc.group(1) if loc else "",
                "server": srv.group(1).strip() if srv else "",
                "igd_match": "InternetGatewayDevice" in text,
            })
        s.close()
        out["available"] = any(r["igd_match"] or r["location"] for r in out["responses"])
    except OSError as e:
        out["error"] = str(e)
    return out


def natpmp_query(gateway: str, timeout: float) -> dict:
    out: dict = {"gateway": gateway, "available": False, "error": ""}
    if not gateway:
        out["error"] = "no default gateway"
        return out
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.settimeout(timeout)
        s.sendto(struct.pack("!BB", 0, 0), (gateway, 5351))
        data, _ = s.recvfrom(64)
        s.close()
        if len(data) >= 12:
            ver, op, result, epoch, ext = struct.unpack_from("!BBHII", data, 0)
            out.update({
                "version": ver, "opcode": op, "result_code": result,
                "epoch_sec": epoch, "external_ip": socket.inet_ntoa(struct.pack("!I", ext)),
                "available": ver == 0 and op == 128 and result == 0,
                "response_bytes": len(data),
            })
        else:
            out["error"] = "short response"
    except socket.timeout:
        out["error"] = "timeout (NAT-PMP/PCP 未启用或被过滤)"
    except OSError as e:
        out["error"] = f"socket: {e}"
    return out


def cgnat_heuristic(local: str, mapped: str, gateway: str) -> dict:
    out = {"suspected": False, "reasons": []}
    if local and ipaddress.ip_address(local) in ipaddress.ip_network("100.64.0.0/10"):
        out["suspected"] = True
        out["reasons"].append("本机地址落在 CGNAT 保留段 100.64/10")
    if gateway and is_private(gateway):
        out["reasons"].append(f"默认网关 {gateway} 为私有地址（多层 NAT）")
    # 二跳 traceroute（尽力而为，不因失败影响结论）
    try:
        r = subprocess.run(["tracert", "-h", "2", "-w", "400", "-d", "8.8.8.8"],
                           capture_output=True, text=True, timeout=10, check=False)
        hops = re.findall(r"^\s*(\d+)\s+.*?(\d+\.\d+\.\d+\.\d+)", r.stdout or "", re.M)
        second = next((ip for h, ip in hops if h == "2"), "")
        out["second_hop"] = second or None
        if second and is_private(second):
            out["suspected"] = True
            out["reasons"].append(f"到公网第 2 跳 {second} 仍为私有地址（典型 CGNAT/运营商级 NAT）")
    except (OSError, subprocess.TimeoutExpired):
        out["second_hop"] = None
    out["note"] = ("仅凭单出口样本无法精确区分家庭 NAT 与 CGNAT；Q4 的对称性比例仍须 M2 多环境实测"
                   if not out["suspected"] else "疑似 CGNAT，R-04 应对称型直连率按保守值估计")
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--timeout", type=float, default=2.0)
    ap.add_argument("--out", default="")
    args = ap.parse_args()

    lip = local_ip()
    gw = default_gateway()
    print(f"local_ip={lip or 'n/a'}  gateway={gw or 'n/a'}", flush=True)

    # 关键：所有 Test I 复用**同一个本地socket/端口**，否则映射端口不可比
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(("", 0))
    local_port = sock.getsockname()[1]
    results = []
    for host, port in STUN_SERVERS:
        row = stun_query(sock, host, port, args.timeout)
        results.append(row)
        print(f"  STUN {row['server']:<32} {'OK ' + str(row.get('mapped_address')) if row['ok'] else row.get('error','?')}",
              flush=True)
    # 同 IP 不同端口再打一次（对称性判据的加强测试）
    first_ok = next((r for r in results if r["ok"]), None)
    alt = None
    if first_ok:
        host = first_ok["server"].rsplit(":", 1)[0]
        alt = stun_query(sock, host, 3479, min(args.timeout, 1.5))
        results.append(alt)
        print(f"  STUN {alt['server']:<32} {'OK ' + str(alt.get('mapped_address')) if alt['ok'] else alt.get('error','?')}",
              flush=True)
    sock.close()

    mapped = [r["mapped_address"] for r in results if r.get("ok")]
    unique = sorted(set(mapped))
    if not mapped:
        nat_type, confidence = "undetermined_stun_unreachable", "none"
    elif len(unique) == 1:
        nat_type, confidence = "cone_or_no_nat (映射地址不随目的变化)", "medium"
    else:
        ips = {m.rsplit(":", 1)[0] for m in unique}
        ports = {m.rsplit(":", 1)[1] for m in unique}
        nat_type = "symmetric_nat" if len(ports) > 1 else "cone_with_multiple_exits"
        confidence = "medium" if len(ips) == 1 else "low"

    upnp = upnp_discover(min(3.0, args.timeout + 1))
    npmp = natpmp_query(gw, min(2.0, args.timeout))
    cgnat = cgnat_heuristic(lip, unique[0] if unique else "", gw)
    print(f"  UPnP IGD: {upnp['available']} | NAT-PMP: {npmp['available']} ({npmp.get('error') or npmp.get('external_ip','')})",
          flush=True)
    print(f"  NAT 判定: {nat_type} | mapped={unique} | CGNAT suspected={cgnat['suspected']}", flush=True)

    report = {
        "spike": "S6",
        "harness": "tests/spikes/s6/stun_probe.py",
        "recorded_at": datetime.now(timezone.utc).isoformat(),
        "host": {"os": "Windows", "python": sys.version.split()[0],
                 "local_ip": lip, "local_udp_port": local_port, "default_gateway": gw},
        "config": {"timeout_sec": args.timeout, "stun_servers": [f"{h}:{p}" for h, p in STUN_SERVERS],
                   "protocol": "RFC 5389 STUN Binding / SSDP M-SEARCH / NAT-PMP opcode 0"},
        "stun_results": results,
        "classification": {
            "mapped_addresses": unique,
            "nat_type_observed": nat_type,
            "confidence": confidence,
            "public_udp_egress_ok": bool(mapped),
            "note": "同一本地端口对多个不同目的 IP:port 的映射比较；cone 子型（restricted / port-restricted）"
                    "需要 OTHER-ADDRESS 配合二次测试，本环境未强制区分",
        },
        "traversal_stack_probe": {
            "upnp_igd_discovery": upnp,
            "nat_pmp": npmp,
        },
        "cgnat": cgnat,
        "sample": {"count": 1, "environment": "本机当前网络出口",
                   "matrix_coverage": "1 / 4 类 NAT（其余为分析型估计）"},
        "evidence_valid_for_pass": False,
        "limitation": "单出口单样本，不满足 S6 判据「NAT 类型矩阵实测给出可达率」；"
                      "须 M2 在 Full/Restricted/Port-restricted/Symmetric 四类真实环境（含 CGNAT）逐一实测",
    }
    out = Path(args.out) if args.out else Path(__file__).parent / "reports" / (
        "s6-nat-" + datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + ".json")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print("report:", out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
