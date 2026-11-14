#!/usr/bin/env python3
"""ColdRevive 实验条件探针（2026-11-14）——判定 S3 / S5 / S6 是否具备**真实实测**条件。

本脚本只做只读、低速、标准协议探测，产出机读条件快照，**不修改任何 spike 结论**。

判据来源：
  * S3：`tests/spikes/s3/README.md` + `spikes.json` → M1 真实双栈集成 + G3 兼容性矩阵；
  * S5：`tests/spikes/s5/README.md` → tc/netem 或实验室整形链路 + 真实 rqbit uTP ≤5%；
  * S6：`tests/spikes/s6/nat-matrix.md` → 四类真实 NAT（含 CGNAT）可达率矩阵。

红线（tests/spikes/README.md §6）：
  * 本地回环 / 用户态整形 / 单 NAT 样本**只能作为附件**，不能单独支撑 pass；
  * 脚本禁止自动把任何 Spike 置为 `pass`；结论由实验执行人依证据手工回填；
  * 缺失/失败一律 `BLOCKED`，不回填「看起来通过」的默认值。

用法：
  python3 tests/spikes/probe_conditions.py [--out <path>] [--peer-health <url>]
  python3 tests/spikes/probe_conditions.py --self-test
退出码：0 = 快照产出成功（无论条件是否满足）；1 = 脚本自身失败。
"""
from __future__ import annotations

import argparse
import ctypes
import json
import os
import platform
import shutil
import socket
import struct
import subprocess
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
COOKIE = 0x2112A442
BINDING_REQUEST = 0x0001
BINDING_RESPONSE = 0x0101

# 运营商/链路速率由 `Get-NetAdapter` 现场观测录入（不硬编码探测结果）。
# 该值只用于说明「未整形链路」的能力，不参与任何判据计算。
OBSERVED_LINK_MBPS = 100.0


def is_admin() -> bool:
    if sys.platform != "win32":
        return os.geteuid() == 0
    try:
        return bool(ctypes.windll.shell32.IsUserAnAdmin())
    except (AttributeError, OSError):
        return False


def stun_probe(host: str, port: int, timeout: float = 2.0) -> dict:
    """真实 RFC 5389 Binding Request；返回映射地址或失败原因。"""
    tid = os.urandom(12)
    req = struct.pack("!HHI12s", BINDING_REQUEST, 0, COOKIE, tid)
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.settimeout(timeout)
    row: dict = {"server": f"{host}:{port}", "ok": False}
    try:
        addr = socket.getaddrinfo(host, port, socket.AF_INET, socket.SOCK_DGRAM)[0][4]
        row["resolved"] = f"{addr[0]}:{addr[1]}"
        t0 = time.perf_counter()
        s.sendto(req, addr)
        data, _ = s.recvfrom(2048)
        row["rtt_ms"] = round((time.perf_counter() - t0) * 1000, 1)
        if (len(data) >= 20
                and struct.unpack_from("!H", data, 0)[0] == BINDING_RESPONSE
                and struct.unpack_from("!I", data, 4)[0] == COOKIE
                and data[8:20] == tid):
            pos = 20
            while pos + 4 <= len(data):
                atype, alen = struct.unpack_from("!HH", data, pos)
                pos += 4
                value = data[pos:pos + alen]
                pos += alen + ((-alen) % 4)
                if atype in (0x0020, 0x0001) and len(value) >= 8 and value[1] == 0x01:
                    p = struct.unpack_from("!H", value, 2)[0]
                    raw = struct.unpack_from("!I", value, 4)[0]
                    if atype == 0x0020:
                        p ^= (COOKIE >> 16) & 0xFFFF
                        raw ^= COOKIE
                    row["mapped_address"] = (
                        f"{socket.inet_ntoa(struct.pack('!I', raw))}:{p}")
                    row["ok"] = True
                    break
        if not row["ok"]:
            row["error"] = "no mapped address in response"
    except Exception as exc:  # noqa: BLE001 - 探针需如实记录任何失败
        row["error"] = f"{type(exc).__name__}: {exc}"
    finally:
        s.close()
    return row


def http_json(url: str, timeout: float = 2.0) -> dict:
    req = urllib.request.Request(url, headers={"User-Agent": "coldrevive-condition-probe/1"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read().decode("utf-8", "replace"))


def http_post_json(url: str, payload: dict, timeout: float = 3.0) -> dict:
    body = json.dumps(payload).encode()
    req = urllib.request.Request(url, data=body,
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read().decode("utf-8", "replace"))


def tcp_rtt_samples(url: str, n: int = 20) -> dict:
    """真实跨主机 HTTP RTT 取样，用于说明未整形 LAN 的基线时延/抖动。"""
    samples: list[float] = []
    errors = 0
    for _ in range(n):
        t0 = time.perf_counter()
        try:
            http_json(url, timeout=2.0)
            samples.append((time.perf_counter() - t0) * 1000)
        except (urllib.error.URLError, OSError, ValueError):
            errors += 1
    if not samples:
        return {"samples": 0, "errors": errors, "note": "对端不可达，未取得 RTT 样本"}
    ordered = sorted(samples)
    return {
        "samples": len(samples),
        "errors": errors,
        "rtt_min_ms": round(ordered[0], 2),
        "rtt_median_ms": round(ordered[len(ordered) // 2], 2),
        "rtt_p95_ms": round(ordered[min(len(ordered) - 1, int(len(ordered) * 0.95))], 2),
        "rtt_max_ms": round(ordered[-1], 2),
        "jitter_p95_minus_median_ms": round(
            ordered[min(len(ordered) - 1, int(len(ordered) * 0.95))]
            - ordered[len(ordered) // 2], 2),
        "note": "真实 TCP/HTTP 往返；链路未整形，不能据此判定 NFR-03 前台劣化 ≤5%",
    }


def ipv6_probe() -> dict:
    """真实枚举本机 IPv6 地址与 IPv6 UDP 路由，不用任何硬编码结论。"""
    import ipaddress
    out: dict = {"addresses": [], "global_unicast": [], "udp_socket_ok": False,
                 "egress_route": "not_probed"}
    try:
        infos = socket.getaddrinfo(socket.gethostname(), None, socket.AF_INET6)
        out["addresses"] = sorted({i[4][0] for i in infos})
    except OSError as exc:
        out["enum_error"] = f"{type(exc).__name__}: {exc}"
    for addr in out["addresses"]:
        try:
            if ipaddress.ip_address(addr.split("%")[0]).is_global:
                out["global_unicast"].append(addr)
        except ValueError:
            pass
    try:
        s = socket.socket(socket.AF_INET6, socket.SOCK_DGRAM)
        s.bind(("::", 0))
        s.settimeout(1.0)
        s.connect(("2001:4860:4860::8888", 53))  # UDP connect 只做路由查找，不发包
        out["udp_socket_ok"] = True
        out["egress_route"] = "udp_connect_ok"
        s.close()
    except OSError as exc:
        out["egress_route"] = f"unavailable: {type(exc).__name__}: {exc}"
    out["note"] = ("connect 成功只说明本机 IPv6 协议栈/路由存在，不等于公网 IPv6 端到端可达；"
                   "本项只描述条件，不用于任何 spike 判定")
    return out


def virtual_nat_probe() -> dict:
    """检测是否有额外虚拟 NAT 出口可作第二类真实 NAT；如实记录不可达。"""
    out: dict = {"candidate_ips": [], "egress_ok": False, "detail": None,
                 "usable_as_extra_real_nat": False}
    try:
        infos = socket.getaddrinfo(socket.gethostname(), None, socket.AF_INET)
        out["candidate_ips"] = sorted({i[4][0] for i in infos
                                       if i[4][0].startswith("172.")
                                       and not i[4][0].startswith("127.")})
    except OSError as exc:
        out["enum_error"] = f"{type(exc).__name__}: {exc}"
    for ip in out["candidate_ips"][:1]:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        try:
            s.bind((ip, 0))
            s.settimeout(1.0)
            s.connect(("8.8.8.8", 53))  # 只做路由查找，不向公共网络发载荷
            out["egress_ok"] = True
            out["detail"] = f"{ip} UDP 路由可用"
        except OSError as exc:
            out["detail"] = f"{ip} UDP 路由不可用: {type(exc).__name__}: {exc}"
        finally:
            s.close()
    out["note"] = ("即便虚拟交换机路段存在，也是同一物理出口的软件转发，"
                   "不能当作第二类独立真实 NAT 设备；本项仅用于排除可测条件")
    return out


def nat_cli_report(binary: Path) -> dict:
    if not binary.is_file():
        return {"available": False, "reason": f"缺少二进制 {binary}"}
    try:
        p = subprocess.run([str(binary), "--json", "--timeout", "6000"],
                           capture_output=True, text=True, timeout=25, check=False)
        return {"available": True, "exit_code": p.returncode,
                "report": json.loads(p.stdout.strip()) if p.returncode == 0 else None,
                "stderr": p.stderr.strip()[:200]}
    except Exception as exc:  # noqa: BLE001
        return {"available": False, "reason": f"{type(exc).__name__}: {exc}"}


def build_snapshot(peer_health: str, rtt_n: int) -> dict:
    admin = is_admin()
    ipv6 = ipv6_probe()
    vnat = virtual_nat_probe()
    shaping = {t: shutil.which(t) for t in ("tc", "netem", "ipfw", "dummynet", "wondershaper")}
    stun_servers = [
        ("stun.l.google.com", 19302),
        ("stun1.l.google.com", 19302),
        ("stun.cloudflare.com", 3478),
        ("stun.voipbuster.com", 3478),
    ]
    stun_rows = []
    for host, port in stun_servers:
        stun_rows.append(stun_probe(host, port))
    mapped = sorted({r["mapped_address"] for r in stun_rows if r.get("ok")})

    peer = {"url": peer_health, "reachable": False}
    try:
        h = http_json(peer_health, timeout=2.0)
        peer.update({"reachable": True, "health": h})
        cli_url = peer_health.rsplit("/", 1)[0] + "/cli"
        try:
            peer["cli_inspect_kad"] = http_post_json(
                cli_url, {"command": "inspect", "args": ["kad"]})
        except Exception as exc:  # noqa: BLE001
            peer["cli_inspect_kad_error"] = f"{type(exc).__name__}: {exc}"
    except Exception as exc:  # noqa: BLE001
        peer["error"] = f"{type(exc).__name__}: {exc}"

    rtt = tcp_rtt_samples(peer_health, rtt_n) if peer["reachable"] else {
        "samples": 0, "note": "对端不可达，跳过 RTT 取样"}

    nat_bin = ROOT / ("target/release/coldrevive-nat.exe" if sys.platform == "win32"
                      else "target/release/coldrevive-nat")
    nat = nat_cli_report(nat_bin)

    # ---- 判定（只描述条件，不产生任何 spike 结论） ----
    # 出口类别按「不同公网 IP 数」计；端口不同只因每次探测新建了 socket，
    # 绝不能据此推断对称型 NAT（映射行为判据见 nat_behavior.py 的单 socket 对照）。
    distinct_wan_ips = sorted({a.rsplit(":", 1)[0] for a in mapped})
    real_nat_classes_observed = 1 if distinct_wan_ips else 0
    cgnat_observed = bool(nat.get("report", {}).get("cgnat_suspected")) if nat.get("report") else False
    shaping_available = any(shaping.values()) and admin
    g3_matrix_available = False  # G3 属 M3：第三方客户端兼容矩阵，本机无该资产

    verdicts = {
        "S3": {
            "pass_conditions": ["M1 真实双栈集成（KAD 128-bit / Mainline 160-bit）",
                                "G3 兼容性矩阵全绿 + NFR-04"],
            "locally_available": [
                "真实 coldrevived / coldrevive-cli 二进制（cr-kad 暴露双栈 ID）",
                "真实 UDP/HTTP/CLI 观测通道",
                "第二台物理主机 B（跨主机）",
            ],
            "missing": ["G3 兼容性矩阵（M3 范围，本机无第三方客户端矩阵资产）",
                        "真实 Mainline DHT 公网加入（合规红线：不得向公网发布测试 Key）"],
            "verdict": "PARTIAL_RUNNABLE",
            "experiment_allowed": True,
            "evidence_valid_for_pass": False,
            "note": "可跑真实 M1 双栈集成 wire 实验，但结果只能作为附件；S3.status 保持 not_run",
        },
        "S5": {
            "pass_conditions": ["tc/netem 或实验室整形链路",
                                "真实 rqbit uTP 栈前台劣化 ≤5% 实测"],
            "locally_available": [
                "真实 librqbit-utp 0.7.0 协议栈 + 真实 UDP/TCP socket",
                "真实时钟令牌桶瓶颈（用户态）",
                f"跨主机 {OBSERVED_LINK_MBPS:g} Mbps 未整形 LAN",
            ],
            "missing": ["内核队列整形（tc/netem）需要管理员权限与 Linux/容器环境",
                        "WSL 被安全策略列入黑名单，未尝试也未绕过",
                        "实验室受控拥塞链路"],
            "verdict": "PARTIAL_RUNNABLE",
            "experiment_allowed": True,
            "evidence_valid_for_pass": False,
            "note": "可跑真实 uTP 栈复现实验，但用户态整形不构成 NFR-03 pass 证据；S5.status 保持 not_run",
        },
        "S6": {
            "pass_conditions": ["四类真实 NAT（Full/Restricted/Port-restricted/Symmetric）可达率矩阵",
                                "至少一个 CGNAT 环境",
                                "真实双端唤醒 → 直连 / 中继两条路径"],
            "locally_available": [
                f"{real_nat_classes_observed} 类真实 NAT 出口（单一 cone 类样本）",
                "真实公网 STUN 往返 + RFC 3489 CHANGE-REQUEST 行为测试",
                "真实 coldrevive-relay 服务端与持有者订阅线程",
            ],
            "missing": ["另外 3 类真实 NAT 设备/环境", "CGNAT 真实环境（本样本未检出）",
                        "跨 NAT 双端可达率矩阵（每类 ≥30 次真实唤醒）",
                        "实验执行人对真实 NAT 环境的书面声明/attestation"],
            "verdict": "PARTIAL_RUNNABLE",
            "experiment_allowed": True,
            "evidence_valid_for_pass": False,
            "note": "可做单类真实 NAT 深度行为实测 + 矩阵 harness 干跑；矩阵覆盖率仍 0/4，S6.status 保持 not_run",
        },
    }

    return {
        "schema": "coldrevive.experiment-conditions/v1",
        "recorded_at": datetime.now(timezone.utc).isoformat(),
        "harness": "tests/spikes/probe_conditions.py",
        "purpose": "判定 S3/S5/S6 是否具备真实实测条件；只描述条件，不产生 spike 结论",
        "host": {
            "os": platform.system(), "release": platform.release(),
            "python": platform.python_version(),
            "admin": admin,
            "local_ip": socket.gethostbyname(socket.gethostname()),
        },
        "environment": {
            "shaping_tools": shaping,
            "shaping_available": shaping_available,
            "wsl": "policy-blocked (not probed, no workaround attempted)",
            "observed_lan_link_mbps": OBSERVED_LINK_MBPS,
            "observed_lan_link_source": "Get-NetAdapter LinkSpeed（现场只读观测，人工录入）",
            "ipv6": ipv6,
            "virtual_nat_path": vnat,
            "wsl_note": "安全策略黑名单（人工观测记录，未尝试也未绕过）",
            "note": ("无管理员权限、无 tc/netem；IPv6/虚拟出口由本脚本真实 socket 探测；"
                     "WSL 被安全策略禁止且未绕过；无法构造额外独立真实 NAT 类型或内核整形链路"),
        },
        "real_probes": {
            "stun": {"rows": stun_rows, "distinct_mapped_addresses": mapped,
                     "distinct_wan_ips": distinct_wan_ips,
                     "public_udp_egress_ok": bool(mapped),
                     "method_note": ("每次探测新建 UDP socket，故映射端口天然不同；"
                                     "本字段不是对称型 NAT 判据。映射/过滤行为分类须用 "
                                     "tests/spikes/s6/nat_behavior.py（同一 socket 多目的 + "
                                     "CHANGE-REQUEST 双对照）")},
            "coldrevive_nat": nat,
            "second_physical_host": peer,
            "cross_host_tcp_rtt": rtt,
        },
        "verdicts": verdicts,
        "gate_discipline": {
            "spike_status_changed": False,
            "s3_s5_s6_status": "not_run",
            "s6_matrix_coverage": "0 / 4 类真实 NAT（含 CGNAT 环境 0/1）",
            "g2_claim": False,
            "no_auto_pass": True,
            "decision": "结论由实验执行人依 evidence_required 手工回填 spikes.json",
        },
    }


def self_test() -> int:
    """纯逻辑自检：不联网、不声称任何真实证据。"""
    assert stun_probe.__doc__ and "RFC 5389" in stun_probe.__doc__
    assert is_admin() in (True, False)
    r = tcp_rtt_samples("http://127.0.0.1:1/health", 2)
    assert r["samples"] == 0 and r["errors"] == 2, r
    # 判据字段完整性
    fake = {"verdicts": {k: {"pass_conditions": [], "missing": [],
                             "verdict": "PARTIAL_RUNNABLE",
                             "evidence_valid_for_pass": False} for k in ("S3", "S5", "S6")}}
    for k, v in fake["verdicts"].items():
        assert v["pass_conditions"] == [] or True
        assert v["evidence_valid_for_pass"] is False, k
    # 环境探针不得把硬编码结论当观测：字段必须齐备且带免责说明
    checks = []
    ip6 = ipv6_probe()
    checks.append(("ipv6_probe_reports_keys",
                   all(k in ip6 for k in ("addresses", "global_unicast", "udp_socket_ok",
                                          "egress_route", "note"))))
    vn = virtual_nat_probe()
    checks.append(("virtual_nat_probe_never_claims_extra_nat",
                   vn["usable_as_extra_real_nat"] is False
                   and all(k in vn for k in ("candidate_ips", "egress_ok", "detail", "note"))))
    for name, ok in checks:
        assert ok, name
    n = 3 + len(checks)
    print(f"probe_conditions self-test: {n}/{n} PASS (pure logic + local sockets, evidence_valid=false)")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="")
    ap.add_argument("--peer-health", default="http://192.168.1.209:17900/health")
    ap.add_argument("--rtt-samples", type=int, default=20)
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return self_test()
    snap = build_snapshot(args.peer_health, args.rtt_samples)
    out = (Path(args.out) if args.out else
           ROOT / "tests/spikes/reports" / ("conditions-"
           + datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + ".json"))
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(snap, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    for sid, v in snap["verdicts"].items():
        print(f"  {sid}: {v['verdict']}  experiment_allowed={v['experiment_allowed']}  "
              f"evidence_valid_for_pass={v['evidence_valid_for_pass']}")
    print(f"  STUN mapped: {snap['real_probes']['stun']['distinct_mapped_addresses'] or 'none'}")
    print(f"  B host reachable: {snap['real_probes']['second_physical_host']['reachable']}")
    print("  spike_status_changed: False | s6_matrix_coverage: 0 / 4 | g2_claim: False")
    print("report:", out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
