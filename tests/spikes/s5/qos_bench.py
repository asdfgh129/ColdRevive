#!/usr/bin/env python3
"""S5 harness: NFR-03 本地可验证半边（令牌桶硬上限 + 前台吞吐劣化量测）。

范围声明（重要）：
  * 这是**真实 TCP 传输 + 真实时钟 + 应用层瓶颈整形**的量测，不是 127.0.0.1 回环裸奔，
    也不是 librqbit uTP / LEDBAT 协议栈本身。
  * 目的是补齐 s5/README §4 的两个 TODO：① 令牌桶硬上限（≤20% 上行）的确定性验证；
    ② 并发场景下前台吞吐劣化的可复现数字（作为 M1/G3 真实拥塞链路实测的 harness 骨架）。
  * 按 tests/spikes/README §6 第 3 条，本地/模拟证据**不能单独支撑 S5.status=pass**；
    本脚本不写 spikes.json，报告只作为 S5 observations 的附件。

机理：
  shared link bucket 模拟一条容量 LINK_BPS 的瓶颈上行；前台流无自有上限（可占满链路），
  后台流另受 bg bucket（默认 20% 链路）硬限。三种相位：
    baseline   : 仅前台，测得 T0
    bucket_only: 前台 + 后台（仅令牌桶），预期前台 ≈80% 链路（令牌桶并不会主动退让）
    with_yield : 前台 + 后台（令牌桶 + LEDBAT 式延迟退让控制器），
                 后台观测到等待瓶颈令牌的排队延迟超过 target 时按 AIMD 减小窗口，
                 前台得以恢复接近 T0。
  退让控制器是**LEDBAT 机理的可执行模型**，不是 uTP 实现；M1/G3 须换成 rqbit uTP 实测。

用法: python3 qos_bench.py [--seconds 3] [--rounds 3] [--link-mbps 64] [--out <path>]
退出码 0 = 量测完成（不表示判据通过）；1 = harness 自身失败。
"""
from __future__ import annotations

import argparse
import json
import socket
import statistics
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

MAGIC = b"CRS5"
CHUNK = 16 * 1024


class TokenBucket:
    """真实时钟令牌桶：rate 字节/秒，burst 字节。acquire() 返回等待秒数（排队延迟代理）。"""

    def __init__(self, rate: float, burst: float | None = None):
        self.rate = float(rate)
        # burst 必须 ≥ 单次申请量，否则 acquire() 会永远凑不够令牌并空转
        min_burst = 4 * CHUNK
        self.burst = float(max(burst if burst is not None else rate * 0.05, min_burst))
        self.tokens = self.burst
        self.last = time.perf_counter()
        self.lock = threading.Lock()

    def acquire(self, n: int) -> float:
        waited = 0.0
        while True:
            with self.lock:
                now = time.perf_counter()
                self.tokens = min(self.burst, self.tokens + (now - self.last) * self.rate)
                self.last = now
                if self.tokens >= n:
                    self.tokens -= n
                    return waited
                deficit = n - self.tokens
                sleep_s = min(deficit / self.rate, 0.05)
            t0 = time.perf_counter()
            time.sleep(sleep_s)
            waited += time.perf_counter() - t0


class YieldController:
    """LEDBAT 式延迟退让（机理模型）：queue_delay > target 时乘性减，否则加性增。"""

    def __init__(self, target_ms: float = 8.0, min_gain: float = 0.02):
        self.target = target_ms / 1000.0
        self.min_gain = min_gain
        self.gain = 1.0
        self.max_delay_seen = 0.0

    def observe(self, queue_delay: float) -> None:
        self.max_delay_seen = max(self.max_delay_seen, queue_delay)
        if queue_delay > self.target:
            self.gain = max(self.min_gain, self.gain * 0.5)
        else:
            self.gain = min(1.0, self.gain + 0.05)


def _pump(sock: socket.socket, counter: list[int], stop: threading.Event) -> None:
    sock.settimeout(0.2)
    while not stop.is_set():
        try:
            data = sock.recv(64 * 1024)
        except socket.timeout:
            continue
        except OSError:
            break
        if not data:
            break
        counter[0] += len(data)


def run_phase(phase: str, seconds: float, link_bps: float, bg_ratio: float,
              enable_yield: bool) -> dict:
    """两个独立发送线程竞争同一个瓶颈令牌桶；前台无自有上限，后台受 20% 硬限。"""
    fg_a, fg_b = socket.socketpair()
    bg_a, bg_b = socket.socketpair()
    stop = threading.Event()
    fg_recv, bg_recv = [0], [0]
    ths = [
        threading.Thread(target=_pump, args=(fg_b, fg_recv, stop), daemon=True),
        threading.Thread(target=_pump, args=(bg_b, bg_recv, stop), daemon=True),
    ]
    for t in ths:
        t.start()

    link = TokenBucket(link_bps, burst=link_bps * 0.02)
    bg_bucket = TokenBucket(link_bps * bg_ratio, burst=link_bps * bg_ratio * 0.02)
    ctrl = YieldController()
    payload = MAGIC + b"x" * (CHUNK - len(MAGIC))
    deadline = time.perf_counter() + seconds
    t_start = time.perf_counter()
    # link_wait 由两个线程共同写入，作为瓶颈队列延迟的可观测量（LEDBAT 的 OWD 代理）
    shared = {"link_wait": 0.0}
    shared_lock = threading.Lock()
    bg_delay_samples: list[float] = []
    last_delay = 0.0

    def publish_wait(w: float) -> None:
        with shared_lock:
            # 指数平滑，避免单次抖动主导
            shared["link_wait"] = 0.8 * shared["link_wait"] + 0.2 * w

    def current_wait() -> float:
        with shared_lock:
            return shared["link_wait"]

    def fg_sender() -> None:
        while time.perf_counter() < deadline:
            w = link.acquire(CHUNK)
            publish_wait(w)
            try:
                fg_a.sendall(payload)
            except OSError:
                return

    def bg_sender() -> None:
        nonlocal last_delay
        while time.perf_counter() < deadline:
            bg_bucket.acquire(CHUNK)  # 硬上限：无论是否拥塞都不得超过 ratio
            if enable_yield:
                # LEDBAT 依据**上一个自己报文**的单向延迟调整拥塞窗口；
                # 决策必须在申请瓶颈令牌之前，否则被让出的令牌会被白白消耗掉。
                ctrl.observe(last_delay)
                if ctrl.gain < 1.0 and (time.perf_counter_ns() % 1000) / 1000.0 > ctrl.gain:
                    continue
            w = link.acquire(CHUNK)  # 本后台报文在瓶颈链路上的排队延迟（OWD 代理）
            last_delay = w
            bg_delay_samples.append(w)
            publish_wait(w)
            try:
                bg_a.sendall(payload)
            except OSError:
                return

    senders = [threading.Thread(target=fg_sender, daemon=True)]
    if phase != "baseline":
        senders.append(threading.Thread(target=bg_sender, daemon=True))
    for t in senders:
        t.start()
    for t in senders:
        t.join(timeout=seconds + 5)
    elapsed = time.perf_counter() - t_start
    # 给接收线程一点时间把内核缓冲读完
    time.sleep(0.25)
    stop.set()
    for t in ths:
        t.join(timeout=1.0)
    for s in (fg_a, fg_b, bg_a, bg_b):
        try:
            s.close()
        except OSError:
            pass

    fg_bytes, bg_bytes = fg_recv[0], bg_recv[0]
    total = fg_bytes + bg_bytes
    return {
        "phase": phase,
        "elapsed_sec": round(elapsed, 3),
        "foreground_bytes": fg_bytes,
        "background_bytes": bg_bytes,
        "foreground_mbps": round(fg_bytes * 8 / elapsed / 1e6, 2),
        "background_mbps": round(bg_bytes * 8 / elapsed / 1e6, 2),
        "background_share_of_total": round(bg_bytes / total, 4) if total else 0.0,
        "background_share_of_link": round(bg_bytes / elapsed / link_bps, 4),
        "bg_queue_delay_max_ms": round(max(bg_delay_samples) * 1000, 2) if bg_delay_samples else 0.0,
        "link_wait_final_ms": round(current_wait() * 1000, 2),
        "yield_gain_final": round(ctrl.gain, 4) if enable_yield else None,
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--seconds", type=float, default=3.0)
    ap.add_argument("--rounds", type=int, default=3)
    ap.add_argument("--link-mbps", type=float, default=64.0, help="模拟瓶颈链路容量(Mbit/s)")
    ap.add_argument("--bg-ratio", type=float, default=0.2, help="后台上行硬上限占比")
    ap.add_argument("--out", default="")
    args = ap.parse_args()

    link_bps = args.link_mbps * 1e6 / 8
    rounds = []
    for r in range(1, args.rounds + 1):
        row = {"round": r}
        for phase in ("baseline", "bucket_only", "with_yield"):
            row[phase] = run_phase(phase, args.seconds, link_bps, args.bg_ratio,
                                   enable_yield=(phase == "with_yield"))
        t0 = row["baseline"]["foreground_mbps"]
        t1 = row["bucket_only"]["foreground_mbps"]
        t2 = row["with_yield"]["foreground_mbps"]
        row["degradation_bucket_only_pct"] = round((t0 - t1) / t0 * 100, 2) if t0 else None
        row["degradation_with_yield_pct"] = round((t0 - t2) / t0 * 100, 2) if t0 else None
        rounds.append(row)
        print(f"[round {r}] T0={t0:.1f} T1(bucket)={t1:.1f} T2(+yield)={t2:.1f} Mbit/s | "
              f"劣化 {row['degradation_bucket_only_pct']}% → {row['degradation_with_yield_pct']}% | "
              f"后台占比 {row['with_yield']['background_share_of_link']*100:.1f}% 链路", flush=True)

    def med(key, phase=None):
        vals = [r[phase][key] if phase else r[key] for r in rounds]
        return round(statistics.median(vals), 3)

    summary = {
        "token_bucket_hard_cap": {
            "configured_ratio": args.bg_ratio,
            "max_observed_background_share_of_link": max(
                r[ph]["background_share_of_link"] for r in rounds
                for ph in ("bucket_only", "with_yield")),
            "within_20pct": all(
                r[ph]["background_share_of_link"] <= args.bg_ratio + 0.02
                for r in rounds for ph in ("bucket_only", "with_yield")),
            "note": "后台上行被令牌桶硬卡在配置占比内（含 2% 突发容差），与前台是否拥塞无关",
        },
        "foreground_degradation": {
            "baseline_mbps_median": med("foreground_mbps", "baseline"),
            "bucket_only_degradation_pct_median": med("degradation_bucket_only_pct"),
            "with_yield_degradation_pct_median": med("degradation_with_yield_pct"),
            "target_pct": 5.0,
            "note": "with_yield 为本地瓶颈整形 + LEDBAT 式延迟退让模型的量测，"
                    "不是真实 uTP 协议栈；按治理规则不作为 S5 pass 证据",
        },
    }

    report = {
        "spike": "S5",
        "harness": "tests/spikes/s5/qos_bench.py",
        "recorded_at": datetime.now(timezone.utc).isoformat(),
        "host": {"os": "Windows", "python": __import__("sys").version.split()[0]},
        "config": {
            "phase_seconds": args.seconds, "rounds": args.rounds,
            "link_mbps": args.link_mbps, "bg_ratio": args.bg_ratio,
            "chunk_bytes": CHUNK, "yield_target_ms": 8.0,
        },
        "scope": "真实 TCP socketpair + 真实时钟令牌桶 + 应用层瓶颈整形；LEDBAT 退让为机理模型，非 librqbit uTP",
        "evidence_valid_for_pass": False,
        "rounds": rounds,
        "summary": summary,
    }
    out = Path(args.out) if args.out else Path(__file__).parent / "reports" / (
        "s5-qos-" + datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + ".json")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print("\n=== SUMMARY ===")
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    print("report:", out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
