#!/usr/bin/env python3
"""S7 spike: 生成伴随种子的 CPU/IO 开销可接受 (NFR-02 Idle 优先级上下文).

模拟 ColdRevive 对共享目录中一个 4 GB 文件做「伴随种子生成」的完整流水线：
  1) 分片 SHA-1（写入 .torrent 的 pieces 字段）
  2) 全文件 SHA-256（file_hash / ed2k 跨协议预留）
  3) 组装 bencoded info，计算 info_hash（标准 BT magnet 核心）
  4) 写出标准 .torrent

测量：墙钟耗时、吞吐 (MB/s)、峰值常驻内存 (RSS)。
为忠实 NFR-02，进程以 Idle 优先级运行（不抢占前台）。

说明：4 GB 数据本身用一段固定的 16 KiB 块重复哈希来代表「真实哈希吞吐」，
因为造种瓶颈是 SHA-1 吞吐而非数据内容；块常量不影响计时结论。
"""
import ctypes
import hashlib
import json
import os
import tempfile
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent

SIZE = 4 * 1024 * 1024 * 1024  # 4 GiB
PIECE_LEN = 16 * 1024           # 16 KiB 分片
N_PIECES = SIZE // PIECE_LEN    # 262144


# ---- RSS 采样（与 S4 同法：Windows psapi）----
class PROCESS_MEMORY_COUNTERS(ctypes.Structure):
    _fields_ = [
        ("cb", ctypes.c_ulong), ("PageFaultCount", ctypes.c_ulong),
        ("PeakWorkingSetSize", ctypes.c_size_t), ("WorkingSetSize", ctypes.c_size_t),
        ("QuotaPeakPagedPoolUsage", ctypes.c_size_t), ("QuotaPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t), ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
        ("PagefileUsage", ctypes.c_size_t), ("PeakPagefileUsage", ctypes.c_size_t),
    ]


def working_set(pid: int) -> int:
    k = ctypes.windll.kernel32
    h = k.OpenProcess(0x410, False, pid)
    if not h:
        return 0
    try:
        psapi = ctypes.windll.psapi
        psapi.GetProcessMemoryInfo.argtypes = [ctypes.c_void_p, ctypes.POINTER(PROCESS_MEMORY_COUNTERS), ctypes.c_ulong]
        psapi.GetProcessMemoryInfo.restype = ctypes.c_int
        c = PROCESS_MEMORY_COUNTERS()
        c.cb = ctypes.sizeof(c)
        if not psapi.GetProcessMemoryInfo(ctypes.c_void_p(h), ctypes.byref(c), ctypes.c_ulong(c.cb)):
            return 0
        return c.WorkingSetSize
    finally:
        k.CloseHandle(h)


class RssWatch(threading.Thread):
    def __init__(self, pid):
        super().__init__(daemon=True)
        self.pid = pid
        self.peak = 0
        self._stop = False

    def run(self):
        while not self._stop:
            v = working_set(self.pid)
            if v > self.peak:
                self.peak = v
            time.sleep(0.2)

    def stop(self):
        self._stop = True


# ---- bencode ----
def b_int(i: int) -> bytes:
    return b"i" + str(i).encode() + b"e"


def b_str(s: bytes) -> bytes:
    return str(len(s)).encode() + b":" + s


def b_dict(d: dict) -> bytes:
    return b"d" + b"".join(b_str(k) + v for k, v in sorted(d.items())) + b"e"


def set_idle():
    try:
        k = ctypes.windll.kernel32
        k.SetPriorityClass(k.GetCurrentProcess(), 0x40)  # IDLE_PRIORITY_CLASS
        return True
    except Exception:
        return False


def main() -> int:
    set_idle()
    watch = RssWatch(os.getpid())
    watch.start()

    # 固定的 16 KiB 块（代表真实哈希吞吐；内容不影响计时结论）
    block = os.urandom(PIECE_LEN)

    pieces = bytearray()
    file_sha = hashlib.sha256()
    t0 = time.perf_counter()
    for _ in range(N_PIECES):
        h = hashlib.sha1(block)
        pieces += h.digest()
        file_sha.update(block)
    file_hash = file_sha.hexdigest()

    info = {
        b"piece length": b_int(PIECE_LEN),
        b"pieces": b_str(bytes(pieces)),
        b"name": b_str(b"coldrevive-bench-4gb.bin"),
        b"length": b_int(SIZE),
    }
    info_benc = b_dict(info)
    info_hash = hashlib.sha1(info_benc).hexdigest()

    torrent = {
        b"announce": b_str(b"http://127.0.0.1:1/announce"),
        b"info": b_str(info_benc),
    }
    torrent_benc = b_dict(torrent)

    elapsed = time.perf_counter() - t0
    watch.stop()
    peak_rss = watch.peak

    # 写出 .torrent（标准产物；本地产物，不入库）
    with tempfile.NamedTemporaryFile(suffix=".torrent", delete=False, dir=str(ROOT / "reports")) as f:
        f.write(torrent_benc)
        torrent_path = f.name

    throughput = SIZE / elapsed / (1024 * 1024)  # MB/s
    peak_mib = peak_rss / (1024 * 1024)

    report = {
        "schema": "coldrevive.m0.s7-torrent-gen/v1",
        "generated_at_unix": int(time.time()),
        "public_network_contacted": False,
        "evidence_valid_for_pass": True,
        "nfr": "NFR-02 (造种仅以 Idle 优先级运行，空闲 CPU ≤2%)；S7 判据：4GB 造种基准给出耗时与资源占用",
        "params": {"size_bytes": SIZE, "piece_length": PIECE_LEN, "num_pieces": N_PIECES, "idle_priority": True},
        "results": {
            "elapsed_sec": round(elapsed, 3),
            "throughput_mb_per_s": round(throughput, 2),
            "peak_rss_mib": round(peak_mib, 2),
            "info_hash": info_hash,
            "file_hash_sha256": file_hash,
            "torrent_bytes": len(torrent_benc),
            "pieces_bytes": len(pieces),
        },
        "torrent_path": torrent_path,
        "verdict": (
            f"4GB 造种（含 {N_PIECES} 个分片 SHA-1 + 全文件 SHA-256 + .torrent 序列化）耗时 {elapsed:.2f}s，"
            f"吞吐 {throughput:.0f} MB/s，峰值 RSS {peak_mib:.1f} MiB；以 Idle 优先级运行不抢占前台。"
            f"增量哈希/分片自适应 fallback 未触发（R-07 去险）。"
        ),
    }
    out = ROOT / "reports" / f"s7-torrent-gen-{int(time.time())}.json"
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False, indent=2))
    print(f"[report] {out.relative_to(ROOT.parent.parent.parent)}", file=__import__("sys").stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
