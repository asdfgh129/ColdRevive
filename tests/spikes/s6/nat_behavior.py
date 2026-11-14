#!/usr/bin/env python3
"""S6 真实 NAT 行为实测：RFC 5780 映射行为 + RFC 3489 CHANGE-REQUEST 过滤行为。

与 `stun_probe.py`（先导样本，只判「cone / 非 cone」）不同，本探针用**带对照的
真实 STUN 报文**把本机唯一一个真实 NAT 出口分类到具体子型：

  * 映射行为：同一本地 UDP socket 访问多个**不同目的 IP:port**，比较映射地址 →
    Endpoint-Independent (=cone) / Address(+Port)-Dependent (=symmetric)。
  * 过滤行为：找一个会返回 CHANGED-ADDRESS 且**确实响应 CHANGE-REQUEST** 的 STUN 服务器，
    依次以 flags=0（对照）/2（改端口）/4（改 IP）/6（都改）请求它从备用地址回包。
    对照必须成功、备用地址直连也必须成功，否则如实判 `undetermined`，不猜。
  * 附测：hairpin（NAT 回环）、映射保活/超时、UPnP IGD / NAT-PMP / PCP、CGNAT 启发式。

范围声明（红线）：
  * 本机**只有 1 类真实 NAT 出口**，无 CGNAT、无跨 NAT 双端、无第二/第三/第四类设备；
  * 因此本结果是**分类样本**，不是「四类 NAT 可达率矩阵」；
    `reachability_matrix_coverage = 0 / 4`，`evidence_valid_for_pass = false`，
    `s6_status = not_run`，脚本不写 spikes.json、不自动置 pass。
  * 全部为低速标准 STUN / SSDP / NAT-PMP 报文，不向公共 KAD / DHT 发布任何测试 Key。

用法:
  python3 tests/spikes/s6/nat_behavior.py [--timeout 2.0] [--tries 2]
           [--refresh-after 20] [--out <path>]
  python3 tests/spikes/s6/nat_behavior.py --self-test
"""
from __future__ import annotations

import argparse
import json
import os
import socket
import struct
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(HERE))
import stun_probe as sp  # noqa: E402 - 复用同一目录的 SSDP/NAT-PMP/CGNAT 实现

COOKIE = 0x2112A442
BINDING_REQUEST = 0x0001
BINDING_RESPONSE = 0x0101
CHANGE_REQUEST = 0x0003
CHANGED_ADDRESS = 0x0005
OTHER_ADDRESS = 0x802C

MAPPING_SERVERS = [
    ("stun.l.google.com", 19302),
    ("stun1.l.google.com", 19302),
    ("stun.cloudflare.com", 3478),
    ("stun.voipbuster.com", 3478),
    ("stun.telnyx.com", 3478),
]
FILTER_SERVERS = [
    ("stun.voipbuster.com", 3478),
]


def drain(sock: socket.socket, seconds: float = 0.05) -> None:
    """丢弃上一次试验的迟到响应，避免污染下一次的 TID 匹配。"""
    sock.settimeout(seconds)
    while True:
        try:
            sock.recvfrom(2048)
        except (socket.timeout, OSError):
            return


def binding(sock: socket.socket, dst: tuple[str, int], timeout: float,
            change_flags: int | None = None) -> dict:
    """发一个真实 Binding Request；返回响应来源、映射地址、CHANGED-ADDRESS。"""
    tid = os.urandom(12)
    attr = b"" if change_flags is None else struct.pack("!HHI", CHANGE_REQUEST, 4, change_flags)
    req = struct.pack("!HHI12s", BINDING_REQUEST, len(attr), COOKIE, tid) + attr
    row: dict = {"dst": f"{dst[0]}:{dst[1]}", "change_flags": change_flags, "ok": False}
    try:
        drain(sock)
        sock.settimeout(timeout)
        sock.sendto(req, dst)
        sock.settimeout(timeout)
        data, src = sock.recvfrom(2048)
        row["from"] = f"{src[0]}:{src[1]}"
        row["bytes"] = len(data)
        if (len(data) < 20 or struct.unpack_from("!H", data, 0)[0] != BINDING_RESPONSE
                or struct.unpack_from("!I", data, 4)[0] != COOKIE or data[8:20] != tid):
            row["error"] = "not a matching binding response"
            return row
        row["ok"] = True
        attrs = sp.parse_attrs(data)
        xor = attrs.get(0x0020, [])
        plain = attrs.get(0x0001, [])
        row["mapped_address"] = (sp.decode_mapped(xor[0], True) if xor else
                                 sp.decode_mapped(plain[0], False) if plain else None)
        changed = attrs.get(CHANGED_ADDRESS, []) + attrs.get(OTHER_ADDRESS, [])
        row["changed_address"] = sp.decode_mapped(changed[0], False) if changed else None
        row["attributes_hex"] = [f"0x{k:04x}" for k in attrs]
    except Exception as exc:  # noqa: BLE001
        row["error"] = f"{type(exc).__name__}: {exc}"
    return row


def binding_retry(sock: socket.socket, dst: tuple[str, int], timeout: float,
                  attempts: int, change_flags: int | None = None) -> dict:
    """有界重试：返回首个成功响应（保留全部尝试记录），全失败则返回最后一次。

    公共 STUN 备用地址偶发丢包；单次超时不足以断言「不可达」。
    结构保持扁平（ok/from/... 仍在顶层），便于 flag_validity 等直接消费。
    """
    rows = []
    for i in range(max(1, attempts)):
        r = binding(sock, dst, timeout, change_flags)
        rows.append(r)
        if r.get("ok"):
            break
        if i + 1 < max(1, attempts):
            time.sleep(0.12)  # 轻微退避，避免密集重试触发公共 STUN 限流
    best = next((r for r in rows if r.get("ok")), rows[-1])
    out = dict(best)
    out["attempts"] = rows
    out["attempt_count"] = len(rows)
    return out


def resolve(host: str, port: int) -> tuple[str, int] | None:
    try:
        return socket.getaddrinfo(host, port, socket.AF_INET, socket.SOCK_DGRAM)[0][4]
    except OSError:
        return None


def resolve_all(host: str, port: int) -> list[tuple[str, int]]:
    """返回全部 A 记录。公共 STUN 常做 DNS 轮询且个别节点已失效，
    只取第一条会因随机命中死节点而把可测环境误判成 undetermined。"""
    try:
        infos = socket.getaddrinfo(host, port, socket.AF_INET, socket.SOCK_DGRAM)
        return sorted({i[4] for i in infos})
    except OSError:
        return []


def mapping_behavior(timeout: float) -> dict:
    """同一 socket 访问多个不同目的 IP:port —— 否则映射端口不可比。"""
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(("", 0))
    local_port = sock.getsockname()[1]
    rows = []
    for host, port in MAPPING_SERVERS:
        addr = resolve(host, port)
        if addr is None:
            rows.append({"server": f"{host}:{port}", "ok": False, "error": "dns"})
            continue
        r = binding(sock, addr, timeout)
        r["server"] = f"{host}:{port}"
        r["resolved"] = f"{addr[0]}:{addr[1]}"
        rows.append(r)
    sock.close()
    mapped = [r["mapped_address"] for r in rows if r.get("ok") and r.get("mapped_address")]
    unique = sorted(set(mapped))
    distinct_dst_ips = sorted({r["resolved"].rsplit(":", 1)[0] for r in rows if r.get("resolved")})
    if len(unique) == 1 and len(mapped) >= 2 and len(distinct_dst_ips) >= 2:
        behavior = "endpoint-independent"
        classic = "cone 类（非对称型）"
        confidence = "high"
    elif len(unique) > 1:
        behavior = "address-or-port-dependent"
        classic = "symmetric（映射随目的变化）"
        confidence = "medium"
    else:
        behavior = "undetermined"
        classic = "undetermined"
        confidence = "none"
    return {
        "local_udp_port": local_port,
        "rows": rows,
        "distinct_destination_ips": distinct_dst_ips,
        "distinct_mapped_addresses": unique,
        "mapping_behavior": behavior,
        "classic_mapping_class": classic,
        "confidence": confidence,
        "method": "同一本地 UDP socket 对多个不同目的 IP:port 做 Binding，比较 XOR/MAPPED-ADDRESS",
    }


def flag_validity(alternates: dict) -> dict[int, bool]:
    """每个 CHANGE-REQUEST 标志只依赖它对应的那个备用地址直连对照。"""
    return {
        2: alternates.get("same_ip_alt_port", {}).get("ok") is True,
        4: alternates.get("alt_ip_same_port", {}).get("ok") is True,
        6: alternates.get("alt_ip_alt_port", {}).get("ok") is True,
    }


def filtering_behavior(timeout: float, tries: int, control_tries: int = 4) -> dict:
    """RFC 3489 Test II/III 过滤行为。

    两处方法论修正（缺一不可）：
    1. 每个 CHANGE-REQUEST 标志只依赖**它对应的那个**备用地址直连对照；
       flags=6 的备用地址不可达，并不使 flags=2（同 IP 改端口）失效。
    2. **备用地址直连对照必须换一个本地 socket/端口**。若与过滤测试共用 socket，
       直连探针自己就会为 77.72.169.210:3479 等备用源开好 NAT 放行项，
       随后的 CHANGE-REQUEST 回包便不再是「unsolicited」，会把 port-restricted
       cone 误判成 full cone（实测已复现该假阳性）。
    3. 公共 STUN 的 DNS 轮询含失效节点：遍历全部 A 记录逐个尝试，
       只在取得**可判定结果**时停止，否则如实记录每个候选的失败原因。
    仍如实记录：哪个对照失败、该标志是否可解释、以及残余歧义。
    """
    def add_caveat(text: str) -> None:
        out["caveat"] = (out["caveat"] + "；" if out["caveat"] else "") + text

    out: dict = {
        "method": ("RFC 3489 Test II/III（CHANGE-REQUEST flags 2/4/6）"
                   "+ flags=0 对照 + 每个标志对应的备用地址直连对照"
                   "（独立 socket）+ 对照 socket 上的服务器能力正向对照"),
        "server": None, "primary": None, "changed_address": None,
        "control_valid": False, "alternates_reachable": False,
        "alternate_validity": {}, "interpreted_flags": [],
        "control_socket_isolated": False, "server_capability_confirmed": False,
        "server_honors_change_request": False,
        "results": {}, "filtering_behavior": "undetermined",
        "confidence": "none", "reason": "", "caveat": "",
        "candidates_tried": [],
    }
    candidates = [(host, addr) for host, port in FILTER_SERVERS
                  for addr in resolve_all(host, port)]
    last_reason = ""
    for ci, (host, addr) in enumerate(candidates):
        if ci:
            time.sleep(0.4)  # 候选间退避，避免密集请求触发公共 STUN 限流
        out["caveat"] = ""  # 每个候选独立判定，不继承上一次的残余歧义
        attempt = {"server": f"{host}:{addr[1]}", "resolved": f"{addr[0]}:{addr[1]}"}
        out["candidates_tried"].append(attempt)
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        sock.bind(("", 0))
        # 对照探针专用 socket：绝不能与过滤测试共用本地端口
        ctrl_sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        ctrl_sock.bind(("", 0))
        out["test_local_port"] = sock.getsockname()[1]
        out["control_local_port"] = ctrl_sock.getsockname()[1]
        out["control_socket_isolated"] = (
            out["control_local_port"] != out["test_local_port"])
        base = binding_retry(sock, addr, timeout, 2)
        changed = base.get("changed_address")
        out["server"] = f"{host}:{addr[1]}"
        out["primary"] = f"{addr[0]}:{addr[1]}"
        out["changed_address"] = changed
        if not base.get("ok") or not changed:
            last_reason = "服务器未返回可用 CHANGED-ADDRESS，无法做过滤行为对照"
            attempt["outcome"] = "no_changed_address"
            attempt["error"] = base.get("error")
            sock.close()
            ctrl_sock.close()
            continue
        if not out["control_socket_isolated"]:
            last_reason = "对照 socket 与测试 socket 端口相同，过滤测试会自污染，不可判定"
            attempt["outcome"] = "socket_not_isolated"
            sock.close()
            ctrl_sock.close()
            continue

        # 对照 1：flags=0 必须成功（服务器接受带 CHANGE-REQUEST 属性的请求）
        # flags=0 只验「服务器接受该属性」，无需与备用地址同等强度的重试
        control = [binding_retry(sock, addr, timeout, min(control_tries, 2), 0)
                   for _ in range(tries)]
        out["results"]["control_flags0"] = control
        out["control_valid"] = any(r.get("ok") for r in control)
        if not out["control_valid"]:
            last_reason = "flags=0 对照无应答，过滤行为不可判定"
            attempt["outcome"] = "control_failed"
            sock.close()
            ctrl_sock.close()
            continue

        # 对照 2：逐个备用地址直连，必须使用独立 socket，
        # 否则这些直连报文会给测试 socket 的 NAT 映射预先开好放行项。
        cip, cport = changed.rsplit(":", 1)
        alternates = {
            "same_ip_alt_port": binding_retry(ctrl_sock, (addr[0], int(cport)),
                                             timeout, control_tries),
            "alt_ip_same_port": binding_retry(ctrl_sock, (cip, addr[1]),
                                             timeout, control_tries),
            "alt_ip_alt_port": binding_retry(ctrl_sock, (cip, int(cport)),
                                            timeout, control_tries),
        }
        out["results"]["alternate_direct_socket"] = out["control_local_port"]
        out["results"]["alternate_direct"] = alternates
        # 服务器能力对照：在「已对备用源开好放行项」的对照 socket 上发 flags=6，
        # 若收到来自备用地址的应答，即正向证明服务器确实解析非零 CHANGE-REQUEST。
        cap = binding_retry(ctrl_sock, addr, timeout, control_tries, 6)
        out["results"]["server_capability_flags6_control_socket"] = cap
        out["server_capability_confirmed"] = (
            cap.get("ok") is True and cap.get("from") != out["primary"])
        out["alternates_reachable"] = all(v.get("ok") for v in alternates.values())
        validity = flag_validity(alternates)
        out["alternate_validity"] = {str(k): v for k, v in validity.items()}
        out["interpreted_flags"] = [f for f, v in validity.items() if v]
        if not out["interpreted_flags"]:
            last_reason = "所有备用地址直连均失败，无法区分 NAT 过滤与服务器端限制"
            attempt["outcome"] = "no_valid_alternate"
            sock.close()
            ctrl_sock.close()
            continue

        # 三个标志都真实发出；只有对照成立的标志参与结论
        trials: dict[str, list[dict]] = {}
        for label, flags in (("change_port_flags2", 2), ("change_ip_flags4", 4),
                             ("change_both_flags6", 6)):
            # 「无应答」本身是测量结果，2 次有界重试已足以排除偶发丢包，
            # 且可显著减少对公共 STUN 的请求量。
            trials[label] = [binding_retry(sock, addr, timeout,
                                           min(control_tries, 2), flags)
                             for _ in range(tries)]
        out["results"]["change_requests"] = trials
        got = {2: any(r.get("ok") for r in trials["change_port_flags2"]),
               4: any(r.get("ok") for r in trials["change_ip_flags4"]),
               6: any(r.get("ok") for r in trials["change_both_flags6"])}
        out["change_request_results"] = {str(k): v for k, v in got.items()}
        out["server_honors_change_request"] = any(
            r.get("ok") and r.get("from") != out["primary"]
            for rows in trials.values() for r in rows)

        if validity[6] and got[6]:
            out["filtering_behavior"] = "endpoint-independent"
            out["confidence"] = "high"
            out["reason"] = "备用 IP+备用端口的 unsolicited 回包可到达（full cone）"
        elif validity[2] and got[2]:
            out["filtering_behavior"] = "address-dependent"
            out["confidence"] = "medium" if not validity[6] else "high"
            out["reason"] = "同 IP 改端口的回包可到达（restricted cone）；flags=2 有效"
            if not validity[6]:
                add_caveat("flags=6 对照不成立，无法完全排除 endpoint-independent filtering")
        elif validity[2] and not got[2]:
            out["filtering_behavior"] = "address-and-port-dependent"
            out["confidence"] = "medium"
            out["reason"] = ("flags=2 对照成立但无应答：同 IP 改端口的 unsolicited 回包被过滤，"
                             "结合 endpoint-independent mapping 指向 port-restricted cone")
            if not validity[6]:
                add_caveat("flags=6 对照不成立，无法独立确认 full cone 已被排除")
            if out["server_capability_confirmed"]:
                add_caveat("对照 socket 上 flags=6 已收到备用源应答，"
                           "正向证明服务器解析非零 CHANGE-REQUEST，"
                           "故测试 socket 无应答可归因于 NAT 过滤")
            else:
                add_caveat("服务器能力对照未取得备用源应答，"
                           "无法完全排除服务器忽略 CHANGE-REQUEST 的可能，置信度降为 low")
                out["confidence"] = "low"
        elif (validity[6] and not got[6] and not validity[2]
              and validity[4] and not got[4]
              and out["server_capability_confirmed"]):
            # flags=2 关键对照失效时，仍可诚实给出粗粒度结论：
            # 备用 IP+端口与备用 IP 同端口的 unsolicited 回包均被过滤，
            # 且已正向证明服务器确实解析非零 CHANGE-REQUEST。
            out["filtering_behavior"] = "address-dependent-or-stricter"
            out["confidence"] = "low"
            out["reason"] = ("endpoint-independent filtering 已被排除；"
                             "但 flags=2 对照不成立，无法区分 address-dependent 与 "
                             "address-and-port-dependent（cone 子型未定）")
            add_caveat("flags=2 备用地址直连对照失败，cone 子型不判；"
                       "本结论不得冒充 port-restricted cone 的精确分类")
        else:
            last_reason = "flags=2 对照不成立且 flags=6 未给出肯定结果，如实不判"
            attempt["outcome"] = "inconclusive"
            sock.close()
            ctrl_sock.close()
            continue
        attempt["outcome"] = "conclusive"
        sock.close()
        ctrl_sock.close()
        return out
    out["reason"] = (f"已尝试 {len(candidates)} 个真实 STUN 候选地址均未取得可判定结果；"
                     f"末次原因：{last_reason or '无可用 RFC 3489 双地址 STUN 服务器'}")
    return out


def hairpin_test(timeout: float) -> dict:
    """NAT 回环：从另一个本地 socket 向本机公网映射地址发包，看能否收到。"""
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(("", 0))
    addr = resolve("stun.l.google.com", 19302)
    r = binding(sock, addr, timeout) if addr else {"ok": False}
    mapped = r.get("mapped_address")
    out: dict = {"mapped_address": mapped, "performed": bool(mapped),
                 "received": False, "error": None}
    if mapped:
        ip, port = mapped.rsplit(":", 1)
        sender = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        sender.bind(("", 0))
        try:
            sender.sendto(b"COLDREVIVE-HAIRPIN-PROBE", (ip, int(port)))
            sock.settimeout(1.0)
            data, _ = sock.recvfrom(64)
            out["received"] = data.startswith(b"COLDREVIVE-HAIRPIN-PROBE")
            out["received_bytes"] = len(data)
        except Exception as exc:  # noqa: BLE001
            out["error"] = f"{type(exc).__name__}: {exc}"
        finally:
            sender.close()
    sock.close()
    out["hairpin_supported"] = bool(out["received"])
    out["note"] = ("未收到不等于确定不支持：也可能被本机防火墙丢弃；"
                   "仅作真实观测，不用于分类结论")
    return out


def mapping_refresh(sock: socket.socket, baseline: str | None,
                    wait_sec: int, timeout: float) -> dict:
    if wait_sec <= 0 or not baseline:
        return {"performed": False, "reason": "--refresh-after 未启用或无基线映射"}
    import time
    time.sleep(wait_sec)
    addr = resolve("stun.l.google.com", 19302)
    r = binding(sock, addr, timeout) if addr else {"ok": False}
    current = r.get("mapped_address")
    return {"performed": True, "waited_sec": wait_sec, "baseline": baseline,
            "after_wait": current, "mapping_preserved": current == baseline,
            "note": "真实空闲后同 socket 复测；只描述本次样本，不推断通用超时值"}


def classify(mapping: dict, filtering: dict) -> dict:
    mb, fb = mapping["mapping_behavior"], filtering["filtering_behavior"]
    # 结论置信度取「映射行为」与「过滤行为」中较低者；对照不全时不得高报
    rank = {"none": 0, "low": 1, "medium": 2, "high": 3}
    combined = min(rank.get(mapping.get("confidence", "none"), 0),
                   rank.get(filtering.get("confidence", "none"), 0))
    conf = {0: "none", 1: "low", 2: "medium", 3: "high"}[combined]
    caveat = filtering.get("caveat", "")
    if mb == "endpoint-independent" and fb == "address-and-port-dependent":
        return {"rfc4787": "Endpoint-Independent Mapping + Address-and-Port-Dependent Filtering",
                "classic": "port-restricted cone", "confidence": conf,
                "caveat": caveat,
                "note": "与 cr-nat 的保守 full-cone 标签相比，本次带对照的过滤测试给出更精细结论"}
    if mb == "endpoint-independent" and fb == "address-dependent":
        return {"rfc4787": "Endpoint-Independent Mapping + Address-Dependent Filtering",
                "classic": "restricted cone", "confidence": conf, "caveat": caveat, "note": ""}
    if mb == "endpoint-independent" and fb == "endpoint-independent":
        return {"rfc4787": "Endpoint-Independent Mapping + Endpoint-Independent Filtering",
                "classic": "full cone", "confidence": conf, "caveat": caveat, "note": ""}
    if fb == "address-dependent-or-stricter":
        return {"rfc4787": "Endpoint-Independent Mapping + Filtering ≠ Endpoint-Independent (子型未定)",
                "classic": "cone 类（地址依赖或更严，子型未定）",
                "confidence": conf, "caveat": caveat,
                "note": "flags=2 对照失效，如实只给粗粒度结论，不猜具体子型"}
    if mb == "address-or-port-dependent":
        return {"rfc4787": "Address/Port-Dependent Mapping", "classic": "symmetric",
                "confidence": conf, "caveat": caveat,
                "note": "过滤行为不足以细分时按映射行为归类"}
    return {"rfc4787": "undetermined", "classic": "undetermined", "confidence": "none",
            "caveat": caveat, "note": "对照不全或 STUN 不可达，如实不判"}


def _fake_response(req: bytes) -> bytes:
    """按请求 TID 构造一个最小合法 Binding Response（纯字节，不联网）。"""
    tid = req[8:20]
    value = struct.pack("!BBHI", 0, 1, 1234 ^ ((COOKIE >> 16) & 0xFFFF),
                        0x7F000001 ^ COOKIE)
    attr = struct.pack("!HH", 0x0020, 8) + value
    return struct.pack("!HHI12s", BINDING_RESPONSE, len(attr), COOKIE, tid) + attr


class _FakeSock:
    """只在 recvfrom 上模拟丢包/成功；drain 阶段一律超时。"""

    def __init__(self, outcomes: list):
        self.outcomes = list(outcomes)
        self.sent: list[bytes] = []
        self.drain_mode = False

    def settimeout(self, seconds: float) -> None:
        self.drain_mode = seconds == 0.05

    def sendto(self, data: bytes, _dst) -> None:
        self.sent.append(data)

    def recvfrom(self, _n: int):
        if self.drain_mode:
            raise socket.timeout()
        item = self.outcomes.pop(0) if self.outcomes else socket.timeout()
        if isinstance(item, BaseException):
            raise item
        return (_fake_response(self.sent[-1]) if item == "ok" else item,
                ("77.72.169.211", 3478))


def self_test() -> int:
    """纯协议编解码自检，不联网、不声称真实证据。"""
    checks = []
    s1 = _FakeSock([socket.timeout(), "ok", "unused"])
    r1 = binding_retry(s1, ("77.72.169.211", 3478), 2.0, 3)
    checks.append(("retry_recovers_from_transient_loss",
                   r1.get("ok") is True and r1["attempt_count"] == 2))
    s2 = _FakeSock([socket.timeout(), socket.timeout(), socket.timeout()])
    r2 = binding_retry(s2, ("77.72.169.211", 3478), 2.0, 3)
    checks.append(("retry_bounded_and_honest_failure",
                   r2.get("ok") is False and r2["attempt_count"] == 3))
    attr = struct.pack("!HHI", CHANGE_REQUEST, 4, 2)
    req = struct.pack("!HHI12s", BINDING_REQUEST, len(attr), COOKIE, b"0" * 12) + attr
    checks.append(("change_request_frame_layout",
                   struct.unpack_from("!H", req, 2)[0] == 8 and req[20:22] == b"\x00\x03"))
    # parse_attrs 从 STUN 头之后的第 20 字节开始解析，须传完整报文而非切片
    parsed = sp.parse_attrs(req)
    checks.append(("attrs_parser_sees_change_request", CHANGE_REQUEST in parsed))
    mapped = sp.decode_mapped(struct.pack("!BBHI", 0, 1, 1234, 0x7F000001), False)
    checks.append(("mapped_address_decoder", mapped == "127.0.0.1:1234"))
    c = classify({"mapping_behavior": "endpoint-independent", "confidence": "high"},
                 {"filtering_behavior": "address-and-port-dependent", "confidence": "high"})
    checks.append(("classification_is_port_restricted_cone",
                   c["classic"] == "port-restricted cone" and c["confidence"] == "high"))
    u = classify({"mapping_behavior": "undetermined", "confidence": "none"},
                 {"filtering_behavior": "undetermined", "confidence": "none"})
    checks.append(("no_guess_when_controls_missing", u["confidence"] == "none"))
    weak = classify({"mapping_behavior": "endpoint-independent", "confidence": "high"},
                    {"filtering_behavior": "address-and-port-dependent", "confidence": "low",
                     "caveat": "flags=6 对照不成立"})
    checks.append(("confidence_downgraded_with_weak_controls",
                   weak["confidence"] == "low" and "flags=6" in weak["caveat"]))
    # per-flag 对照：备用 IP+端口不可达时，flags=2 仍须可解释
    v = flag_validity({"same_ip_alt_port": {"ok": True},
                       "alt_ip_same_port": {"ok": True},
                       "alt_ip_alt_port": {"ok": False, "error": "timeout"}})
    checks.append(("per_flag_controls_are_independent",
                   v[2] is True and v[4] is True and v[6] is False))
    # 对照 socket 与测试 socket 必须是两个真实不同的本地端口
    a = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    b = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    a.bind(("", 0)); b.bind(("", 0))
    checks.append(("control_socket_isolated_from_test_socket",
                   a.getsockname()[1] != b.getsockname()[1]))
    a.close(); b.close()
    # 数值字面量不需 DNS；验证多 A 记录展开为去重列表
    checks.append(("resolve_all_returns_deduped_list",
                   resolve_all("127.0.0.1", 3478) == [("127.0.0.1", 3478)]))
    coarse = classify({"mapping_behavior": "endpoint-independent", "confidence": "high"},
                      {"filtering_behavior": "address-dependent-or-stricter",
                       "confidence": "low", "caveat": "flags=2 对照失败"})
    checks.append(("weak_controls_do_not_claim_exact_cone_subtype",
                   "port-restricted" not in coarse["classic"]
                   and "cone" in coarse["classic"] and coarse["confidence"] == "low"))
    for name, ok in checks:
        print(f"  [{'PASS' if ok else 'FAIL'}] {name}")
    n = sum(1 for _, ok in checks if ok)
    print(f"nat_behavior self-test: {n}/{len(checks)} PASS (pure wire logic, evidence_valid=false)")
    return 0 if n == len(checks) else 1


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--timeout", type=float, default=2.0)
    ap.add_argument("--tries", type=int, default=2)
    ap.add_argument("--control-tries", type=int, default=4,
                    help="每个对照/CHANGE-REQUEST 报文的有界重试次数（抗公共 STUN 偶发丢包）")
    ap.add_argument("--refresh-after", type=int, default=0)
    ap.add_argument("--out", default="")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return self_test()

    mapping = mapping_behavior(args.timeout)
    filtering = filtering_behavior(args.timeout, args.tries, args.control_tries)
    hairpin = hairpin_test(args.timeout)
    klass = classify(mapping, filtering)
    upnp = sp.upnp_discover(3.0)
    gw = sp.default_gateway()
    npmp = sp.natpmp_query(gw, 2.0)
    cgnat = sp.cgnat_heuristic(sp.local_ip(),
                               (mapping["distinct_mapped_addresses"] or [""])[0], gw)

    # 映射保活复测复用映射行为探针的 socket 端口（新建 socket 会换端口，故这里独立标注）
    refresh_sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    refresh_sock.bind(("", mapping["local_udp_port"]))
    baseline_addr = resolve("stun.l.google.com", 19302)
    baseline = binding(refresh_sock, baseline_addr, args.timeout) if baseline_addr else {"ok": False}
    refresh = mapping_refresh(refresh_sock, baseline.get("mapped_address"),
                              args.refresh_after, args.timeout)
    refresh_sock.close()

    report = {
        "schema": "coldrevive.s6.nat-behavior/v1",
        "spike": "S6",
        "harness": "tests/spikes/s6/nat_behavior.py",
        "recorded_at": datetime.now(timezone.utc).isoformat(),
        "host": {"os": "Windows", "python": sys.version.split()[0],
                 "local_ip": sp.local_ip(), "default_gateway": gw},
        "config": {"timeout_sec": args.timeout, "tries_per_change_request": args.tries,
                   "control_tries": args.control_tries,
                   "refresh_after_sec": args.refresh_after,
                   "protocol": "RFC 5389 STUN Binding + RFC 3489 CHANGE-REQUEST / CHANGED-ADDRESS"},
        "mapping_behavior": mapping,
        "filtering_behavior": filtering,
        "classification": klass,
        "hairpin": hairpin,
        "mapping_refresh": refresh,
        "traversal_stack_probe": {"upnp_igd": upnp, "nat_pmp": npmp},
        "cgnat": cgnat,
        "public_udp_egress_ok": bool(mapping["distinct_mapped_addresses"]),
        "scope": {
            "real_nat_classes_observed": 1 if mapping["distinct_mapped_addresses"] else 0,
            "classification_sample_coverage": "1 / 4 类真实 NAT（仅分类，非可达率）",
            "reachability_matrix_coverage": "0 / 4 类真实 NAT（含 CGNAT 环境 0/1）",
            "cgnat_observed": bool(cgnat.get("suspected")),
            "two_distinct_hosts_behind_different_nat": False,
            "note": "单出口、单 NAT 类别；无跨 NAT 双端唤醒样本，不能给出可达率",
        },
        "evidence_valid_for_pass": False,
        "s6_status": "not_run",
        "g2_claim": False,
        "spike_status_changed": False,
        "no_auto_pass": True,
        "compliance": "低速标准 STUN/SSDP/NAT-PMP 报文；未向公共 KAD/DHT 发布任何测试 Key",
        "limitation": ("结论只覆盖本机当前 cone 类出口；矩阵须 M2 在 Full/Restricted/"
                       "Port-restricted/Symmetric 四类真实环境（含 CGNAT）逐类实测可达率，"
                       "由实验执行人手工回填 spikes.json"),
    }
    out = (Path(args.out) if args.out else HERE / "reports" / (
        "s6-nat-behavior-" + datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + ".json"))
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"  mapping={klass['rfc4787']}")
    print(f"  classic={klass['classic']}  confidence={klass['confidence']}")
    print(f"  filtering={filtering['filtering_behavior']}  confidence={filtering['confidence']}  "
          f"control_valid={filtering['control_valid']}  valid_flags={filtering['interpreted_flags']}")
    print(f"  control_socket_isolated={filtering['control_socket_isolated']}  "
          f"server_capability_confirmed={filtering['server_capability_confirmed']}  "
          f"server_honors_change_request={filtering['server_honors_change_request']}")
    if filtering.get("caveat"):
        print(f"  caveat: {filtering['caveat']}")
    print(f"  hairpin={hairpin['hairpin_supported']}  cgnat={cgnat.get('suspected')}")
    print(f"  reachability_matrix_coverage=0 / 4  s6_status=not_run  g2_claim=false")
    print("report:", out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
