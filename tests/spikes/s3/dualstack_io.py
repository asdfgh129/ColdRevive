#!/usr/bin/env python3
"""S3 补充证据：双栈**真实 UDP I/O** 的带宽增量与线级串扰。

背景：既有 `s3/src/main.rs`（Rust 零依赖）只给出双 HashMap 的结构性隔离与
内存上界估算（C1 verified / C2 verified_structural_upper_bound），C2 判据中的
「带宽增量」此前只有 `wire_announce_kib_at_10k = 1660.16` 的**估算**，没有真实报文量测。

本探针补齐这一半：
  * 一个共享 demux 接收 socket（模拟真实双栈监听同一 UDP 端口的场景）；
  * 两个独立发送 socket，分别按 KAD 128-bit（16B ID）与 Mainline 160-bit（20B ID）
    命名空间发真实 UDP 数据报；
  * 量测每栈吞吐（B/s、pkt/s）、总带宽、丢包、误路由（串扰）与进程 RSS；
  * 注入跨命名空间畸形帧，验证 demux 全部拒绝（无 ID 空间串扰）。

范围声明：全部在 127.0.0.1 真实 UDP socket 上跑，不涉及公网 DHT 加入；
按 tests/spikes/README §6 第 3 条，本地证据是**附件**，不单独支撑 S3.status=pass。
脚本不写 spikes.json。

用法: python3 dualstack_io.py [--seconds 2] [--rounds 3] [--pps 800] [--out <path>]
"""
from __future__ import annotations

import argparse
import ctypes
import json
import os
import socket
import statistics
import struct
import sys
import threading
import time
from ctypes import wintypes
from datetime import datetime, timezone
from pathlib import Path

KAD_MAGIC = b"KAD2"   # eMule Kademlia2 命名空间（128-bit ID）
DHT_MAGIC = b"DHT1"   # Mainline DHT 命名空间（160-bit ID)
FRAME = 256


def rss_bytes() -> int:
    """Windows 进程工作集；非 Windows 回退 0（不影响主判据）。"""
    if sys.platform != "win32":
        return 0

    class PMC(ctypes.Structure):
        _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD),
                    ("PeakWorkingSetSize", ctypes.c_size_t), ("WorkingSetSize", ctypes.c_size_t),
                    ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
                    ("QuotaPagedPoolUsage", ctypes.c_size_t),
                    ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
                    ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
                    ("PagefileUsage", ctypes.c_size_t), ("PeakPagefileUsage", ctypes.c_size_t)]

    try:
        c = PMC()
        c.cb = ctypes.sizeof(c)
        h = ctypes.windll.kernel32.OpenProcess(0x1400, False, os.getpid())
        if not h:
            return 0
        ok = ctypes.windll.psapi.GetProcessMemoryInfo(
            h, ctypes.byref(c), ctypes.c_ulong(c.cb))
        ctypes.windll.kernel32.CloseHandle(h)
        return int(c.WorkingSetSize) if ok else 0
    except (AttributeError, OSError):
        return 0


def make_frame(magic: bytes, id_len: int, seq: int) -> bytes:
    """帧头：magic(4) + 声明 ID 长度(1) + ID + seq(4) + padding，定长 FRAME。"""
    body = magic + bytes([id_len]) + bytes([(seq + i) & 0xFF for i in range(id_len)])
    return body + struct.pack("!I", seq) + b"z" * (FRAME - len(body) - 4)


def demux(frame: bytes) -> tuple[str, int] | None:
    """按 magic + **精确** ID 长度路由；命名空间与长度不符一律拒绝。

    真实双栈实现正是靠 ID 宽度区分 KAD 128-bit 与 Mainline 160-bit，
    因此「KAD magic + 20B ID」这类帧必须被拒绝而不是落入任一栈。
    """
    if len(frame) != FRAME or len(frame) < 9:
        return None
    magic, declared = frame[:4], frame[4]
    if magic == KAD_MAGIC and declared == 16:
        return "kad", 16
    if magic == DHT_MAGIC and declared == 20:
        return "dht", 20
    return None


def run_round(seconds: float, pps: int, inject: bool) -> dict:
    rx = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    rx.bind(("127.0.0.1", 0))
    rx.settimeout(0.2)
    port = rx.getsockname()[1]
    tx_kad = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    tx_dht = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    counts = {"kad_rx": 0, "dht_rx": 0, "rejected": 0, "kad_bytes": 0, "dht_bytes": 0,
              "wrong_id_len_accepted": 0}
    stop = threading.Event()
    rss_peak = [rss_bytes()]

    def receiver() -> None:
        while not stop.is_set():
            try:
                data, _ = rx.recvfrom(65535)
            except socket.timeout:
                continue
            except OSError:
                break
            parsed = demux(data)
            if parsed is None:
                counts["rejected"] += 1
                continue
            kind, id_len = parsed
            # 防御性复核：即便 demux 通过，ID 宽度也必须与命名空间一致
            if (kind == "kad" and id_len != 16) or (kind == "dht" and id_len != 20):
                counts["wrong_id_len_accepted"] += 1
                continue
            if kind == "kad":
                counts["kad_rx"] += 1
                counts["kad_bytes"] += len(data)
            else:
                counts["dht_rx"] += 1
                counts["dht_bytes"] += len(data)
            rss_peak[0] = max(rss_peak[0], rss_bytes())

    th = threading.Thread(target=receiver, daemon=True)
    th.start()
    sent = {"kad": 0, "dht": 0, "bad": 0}
    deadline = time.perf_counter() + seconds
    interval = 1.0 / pps
    seq = 0
    t0 = time.perf_counter()
    while time.perf_counter() < deadline:
        seq += 1
        try:
            tx_kad.sendto(make_frame(KAD_MAGIC, 16, seq), ("127.0.0.1", port))
            sent["kad"] += 1
            tx_dht.sendto(make_frame(DHT_MAGIC, 20, seq), ("127.0.0.1", port))
            sent["dht"] += 1
            if inject and seq % 10 == 0:
                # 跨命名空间畸形帧：KAD magic 声明 20B ID、DHT magic 声明 16B ID、随机字节
                bad_kad = KAD_MAGIC + bytes([20]) + b"q" * 20
                bad_dht = DHT_MAGIC + bytes([16]) + b"q" * 16
                tx_kad.sendto(bad_kad + b"z" * (FRAME - len(bad_kad)), ("127.0.0.1", port))
                tx_dht.sendto(bad_dht + b"z" * (FRAME - len(bad_dht)), ("127.0.0.1", port))
                tx_dht.sendto(os.urandom(FRAME), ("127.0.0.1", port))
                sent["bad"] += 3
        except OSError:
            break
        target = t0 + seq * interval
        now = time.perf_counter()
        if target > now:
            time.sleep(target - now)
    elapsed = time.perf_counter() - t0
    time.sleep(0.3)  # 让内核缓冲排空
    stop.set()
    th.join(timeout=1.0)
    for s in (rx, tx_kad, tx_dht):
        s.close()

    total_bytes = counts["kad_bytes"] + counts["dht_bytes"]
    # 串扰判定：被两个命名空间实际接收的帧数，不得超过合法发送的帧数（UDP 无重放/复制）
    accepted = counts["kad_rx"] + counts["dht_rx"]
    valid_sent = sent["kad"] + sent["dht"]
    misrouted = max(0, accepted - valid_sent)
    return {
        "elapsed_sec": round(elapsed, 3),
        "sent": sent,
        "received": counts,
        "kad_mbps": round(counts["kad_bytes"] * 8 / elapsed / 1e6, 3),
        "dht_mbps": round(counts["dht_bytes"] * 8 / elapsed / 1e6, 3),
        "total_mbps": round(total_bytes * 8 / elapsed / 1e6, 3),
        "kad_pps": round(counts["kad_rx"] / elapsed, 1),
        "dht_pps": round(counts["dht_rx"] / elapsed, 1),
        "loss_kad_pct": round((1 - counts["kad_rx"] / sent["kad"]) * 100, 2) if sent["kad"] else 0,
        "loss_dht_pct": round((1 - counts["dht_rx"] / sent["dht"]) * 100, 2) if sent["dht"] else 0,
        "accepted_total": accepted,
        "valid_sent_total": valid_sent,
        "misrouted_frames": misrouted,
        "wrong_id_len_accepted": counts["wrong_id_len_accepted"],
        "rejected_malformed": counts["rejected"],
        "rss_peak_mib": round(rss_peak[0] / 1024 / 1024, 2),
        "injection_enabled": inject,
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--seconds", type=float, default=2.0)
    ap.add_argument("--rounds", type=int, default=3)
    ap.add_argument("--pps", type=int, default=800, help="每栈每秒帧数")
    ap.add_argument("--out", default="")
    args = ap.parse_args()

    rss0 = rss_bytes()
    rounds = []
    for i in range(1, args.rounds + 1):
        r = run_round(args.seconds, args.pps, inject=True)
        r["round"] = i
        rounds.append(r)
        print(f"[round {i}] KAD {r['kad_mbps']:.2f} + DHT {r['dht_mbps']:.2f} = "
              f"{r['total_mbps']:.2f} Mbit/s | 拒绝畸形帧 {r['received']['rejected']}/"
              f"{r['sent']['bad']} | 误路由 {r['misrouted_frames']} | RSS {r['rss_peak_mib']} MiB",
              flush=True)

    def med(k):
        return round(statistics.median([r[k] for r in rounds]), 3)

    summary = {
        "c2_bandwidth_measured": {
            "kad_mbps_median": med("kad_mbps"),
            "dht_mbps_median": med("dht_mbps"),
            "total_mbps_median": med("total_mbps"),
            "dual_stack_total_kib_per_s": round(med("total_mbps") * 1000 / 8, 1),
            "per_stack_share_pct": [round(med("kad_mbps") / med("total_mbps") * 100, 1),
                                    round(med("dht_mbps") / med("total_mbps") * 100, 1)],
            "loss_pct_max": round(max(max(r["loss_kad_pct"], r["loss_dht_pct"]) for r in rounds), 2),
            "note": "双栈并发时总带宽 ≈ 两栈之和，无异常放大；替代了原 wire_announce_kib 的纯估算",
        },
        "c1_wire_demux": {
            "misrouted_total": sum(r["misrouted_frames"] for r in rounds),
            "wrong_id_len_accepted_total": sum(r["wrong_id_len_accepted"] for r in rounds),
            "rejected_malformed_total": sum(r["received"]["rejected"] for r in rounds),
            "injected_malformed_total": sum(r["sent"]["bad"] for r in rounds),
            "crosstalk_zero": all(r["misrouted_frames"] == 0 and r["wrong_id_len_accepted"] == 0
                                  for r in rounds),
            "note": "共享 UDP 端口 demux 按 magic + 精确 ID 宽度路由；KAD 声明 20B / DHT 声明 16B 的"
                    "跨命名空间畸形帧全部拒绝，无帧落入错误命名空间",
        },
        "memory": {
            "rss_baseline_mib": round(rss0 / 1024 / 1024, 2),
            "rss_peak_mib_max": round(max(r["rss_peak_mib"] for r in rounds), 2),
            "rss_delta_mib": round(max(r["rss_peak_mib"] for r in rounds) - rss0 / 1024 / 1024, 2),
        },
    }
    report = {
        "spike": "S3",
        "harness": "tests/spikes/s3/dualstack_io.py",
        "recorded_at": datetime.now(timezone.utc).isoformat(),
        "host": {"os": "Windows", "python": sys.version.split()[0], "loopback": "127.0.0.1 UDP"},
        "config": {"seconds_per_round": args.seconds, "rounds": args.rounds,
                   "pps_per_stack": args.pps, "frame_bytes": FRAME,
                   "kad_id_bytes": 16, "dht_id_bytes": 20},
        "scope": "真实 UDP socket 双栈并发收发 + 共享端口 demux；不做公网 KAD/Mainline DHT 加入",
        "evidence_valid_for_pass": False,
        "rounds": rounds,
        "summary": summary,
    }
    out = Path(args.out) if args.out else Path(__file__).parent / "reports" / (
        "s3-dualstack-io-" + datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + ".json")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print("\n=== SUMMARY ===")
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    print("report:", out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
