#!/usr/bin/env python3
"""通过 UPnP IGD 读取本机所在 CPE 自报的 WAN 地址，用于判定是否存在运营商级双层 NAT。

判据（census.rs::double_nat 同口径）：
  STUN 观测到的映射公网 IP == CPE 自报 WAN IP  -> 单层 NAT
  两者不一致，且 CPE WAN 是 100.64/10 或私网   -> 中间还有一层（CGNAT 形态）

只读，不发任何修改类 SOAP。
"""
import json
import re
import socket
import sys
import time
import urllib.request

SSDP_ADDR = ("239.255.255.250", 1900)
SEARCH_TARGETS = [
    "urn:schemas-upnp-org:device:InternetGatewayDevice:1",
    "urn:schemas-upnp-org:service:WANIPConnection:1",
    "urn:schemas-upnp-org:service:WANPPPConnection:1",
]

MSEARCH = (
    "M-SEARCH * HTTP/1.1\r\n"
    "HOST: 239.255.255.250:1900\r\n"
    'MAN: "ssdp:discover"\r\n'
    "MX: 2\r\n"
    "ST: {st}\r\n"
    "\r\n"
)


def ssdp():
    found = {}
    for st in SEARCH_TARGETS:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_TTL, 2)
        s.settimeout(1.2)
        try:
            s.sendto(MSEARCH.format(st=st).encode(), SSDP_ADDR)
        except OSError as e:
            print("ssdp send failed:", e)
            s.close()
            continue
        t0 = time.time()
        while time.time() - t0 < 2.5:
            try:
                data, addr = s.recvfrom(65535)
            except socket.timeout:
                break
            text = data.decode("utf-8", "replace")
            loc = re.search(r"(?im)^LOCATION:\s*(\S+)", text)
            usn = re.search(r"(?im)^USN:\s*(\S+)", text)
            if loc:
                found[loc.group(1).strip()] = usn.group(1).strip() if usn else ""
        s.close()
    return found


def fetch(url, timeout=4):
    req = urllib.request.Request(url, headers={"User-Agent": "cr-cpe-probe/1"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read().decode("utf-8", "replace")


def services(xml, base):
    out = []
    for m in re.finditer(r"<service>(.*?)</service>", xml, re.S | re.I):
        blk = m.group(1)
        st = re.search(r"<serviceType>(.*?)</serviceType>", blk, re.S | re.I)
        cu = re.search(r"<controlURL>(.*?)</controlURL>", blk, re.S | re.I)
        if not (st and cu):
            continue
        stype = st.group(1).strip()
        if "WANIPConnection" not in stype and "WANPPPConnection" not in stype:
            continue
        ctl = cu.group(1).strip()
        if not ctl.lower().startswith("http"):
            ctl = base.rstrip("/") + "/" + ctl.lstrip("/")
        out.append((stype, ctl))
    return out


SOAP = """<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"
 s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
<s:Body><u:{action} xmlns:u="{stype}"/></s:Body></s:Envelope>"""


def soap(ctl, stype, action):
    body = SOAP.format(action=action, stype=stype).encode()
    req = urllib.request.Request(
        ctl,
        data=body,
        headers={
            "Content-Type": 'text/xml; charset="utf-8"',
            "SOAPAction": f'"{stype}#{action}"',
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=5) as r:
            return r.read().decode("utf-8", "replace")
    except Exception as e:  # noqa: BLE001 - 探测工具，错误要原样记录
        return f"__ERR__ {type(e).__name__}: {e}"


def tag(xml, name):
    m = re.search(rf"<{name}>(.*?)</{name}>", xml, re.S | re.I)
    return m.group(1).strip() if m else None


def is_private(ip):
    if not ip:
        return None
    try:
        parts = [int(x) for x in ip.split(".")]
    except ValueError:
        return None
    if len(parts) != 4:
        return None
    a, b = parts[0], parts[1]
    if a == 10:
        return "rfc1918-10/8"
    if a == 172 and 16 <= b <= 31:
        return "rfc1918-172.16/12"
    if a == 192 and b == 168:
        return "rfc1918-192.168/16"
    if a == 100 and 64 <= b <= 127:
        return "rfc6598-100.64/10 (CGNAT)"
    if a == 127:
        return "loopback"
    if a == 169 and b == 254:
        return "link-local"
    return None


def main():
    report = {"probe": "cpe-wan-via-upnp-igd", "at": time.strftime("%Y-%m-%dT%H:%M:%S%z")}
    locations = ssdp()
    report["ssdp_locations"] = list(locations.keys())
    if not locations:
        report["result"] = "UPNP_IGD_NOT_FOUND"
        print(json.dumps(report, ensure_ascii=False, indent=2))
        return 2

    devices = []
    for loc in locations:
        entry = {"location": loc}
        try:
            xml = fetch(loc)
        except Exception as e:  # noqa: BLE001
            entry["error"] = f"{type(e).__name__}: {e}"
            devices.append(entry)
            continue
        entry["friendly_name"] = tag(xml, "friendlyName")
        entry["model"] = tag(xml, "modelName")
        svcs = services(xml, loc)
        entry["wan_services"] = [s[0].split(":")[-2] for s in svcs]
        entry["wan_addr"] = {}
        for stype, ctl in svcs:
            short = stype.split(":")[-2]
            ext = soap(ctl, stype, "GetExternalIPAddress")
            ip = tag(ext, "NewExternalIPAddress")
            st = soap(ctl, stype, "GetStatusInfo")
            entry["wan_addr"][short] = {
                "external_ip": ip,
                "raw_err": None if ip else (ext[:300] if ext else "empty"),
                "connection_status": tag(st, "NewConnectionStatus"),
                "last_error": tag(st, "NewLastConnectionError"),
                "uptime_s": tag(st, "NewUptime"),
            }
        devices.append(entry)
    report["devices"] = devices

    ips = []
    for d in devices:
        for v in (d.get("wan_addr") or {}).values():
            if v.get("external_ip"):
                ips.append(v["external_ip"])
    report["cpe_reported_wan_ips"] = sorted(set(ips))
    report["private_scope"] = {ip: is_private(ip) for ip in sorted(set(ips))}
    if not ips:
        report["result"] = "IGD_PRESENT_BUT_WAN_IP_UNREADABLE"
    elif any(is_private(ip) for ip in ips):
        report["result"] = "BEHIND_CARRIER_NAT (CPE 自报 WAN 为私有/100.64)"
    else:
        report["result"] = "CPE_WAN_IS_PUBLIC (单层 NAT)"
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
