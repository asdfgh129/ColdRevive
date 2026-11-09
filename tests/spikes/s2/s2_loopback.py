#!/usr/bin/env python3
"""S2 本地回环预演：私有 KAD 子网双节点，A 发布带 ColdRevive_V1: 前缀的 Key，B 用 FIND_VALUE 命中 A。

判据来源：tests/spikes/spikes.json 的 S2.criteria；实验方案见 tests/spikes/README.md §5。

红线（必须遵守）：
  - 只在 127.0.0.1 回环收发 UDP，绝不连接公共 eMule KAD bootstrap 节点；
  - 不向公共网络发布任何测试 Key；
  - 本地回环结果只能作为预演，evidence_valid_for_pass 恒为 False，不得据此把 S2 判为 pass；
  - 实网小样本须另有人工批准、限速配置与逐次原始记录。
"""
from __future__ import annotations

import hashlib
import json
import socket
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

NAMESPACE = "ColdRevive_V1:"
HERE = Path(__file__).resolve().parent
REPORT_DIR = HERE / "reports"
SAMPLE_KEYS = [
    "coldrevive:t0:coldrevive-t0-sample.bin",
    "coldrevive:t0:wakeup_req.v1",
    "coldrevive:t0:wakeup_ack.v1",
    "coldrevive:adr:adr-001",
    "coldrevive:doc:roadmap",
]


def kad_id(key: str) -> str:
    """eMule KAD 为 128-bit ID 空间；此处用 SHA-256 前 16 字节作确定性映射。"""
    return hashlib.sha256(key.encode("utf-8")).digest()[:16].hex()


def xor_distance(a: str, b: str) -> int:
    return int(a, 16) ^ int(b, 16)


class Node:
    def __init__(self, name: str) -> None:
        self.name = name
        self.node_id = kad_id(f"node:{name}")
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.sock.bind(("127.0.0.1", 0))
        self.sock.settimeout(1.0)
        self.addr = self.sock.getsockname()
        self.store: dict[str, str] = {}
        self.peers: dict[str, tuple[str, int]] = {}

    def send(self, msg: dict, addr: tuple[str, int]) -> None:
        self.sock.sendto(json.dumps(msg, ensure_ascii=False).encode("utf-8"), addr)

    def recv(self, timeout: float = 1.0):
        self.sock.settimeout(timeout)
        try:
            data, addr = self.sock.recvfrom(65535)
            return json.loads(data.decode("utf-8")), addr
        except socket.timeout:
            return None, None
        except json.JSONDecodeError:
            return None, None

    def close(self) -> None:
        self.sock.close()

    def describe(self) -> dict:
        return {"node_id": self.node_id, "addr": f"{self.addr[0]}:{self.addr[1]}"}


def run() -> int:
    a = Node("A")
    b = Node("B")
    publishes: list[dict] = []
    queries: list[dict] = []

    try:
        # B 通过 HELLO 获知 A 的路由（双节点私有子网）
        b.peers[a.node_id] = a.addr
        a.peers[b.node_id] = b.addr

        # --- A 发布：带专属命名空间前缀的 Key -----------------------------
        for raw in SAMPLE_KEYS:
            key = NAMESPACE + raw
            value = "urn:btih:" + hashlib.sha1(key.encode()).hexdigest()
            t0 = time.perf_counter()
            a.store[key] = value
            elapsed_ms = (time.perf_counter() - t0) * 1000
            publishes.append({
                "key": key,
                "key_id": kad_id(key),
                "value": value,
                "has_namespace_prefix": key.startswith(NAMESPACE),
                "stored": key in a.store,
                "elapsed_ms": round(elapsed_ms, 4),
            })

        # --- B 用 FIND_VALUE 向最近节点（A）查询 ---------------------------
        for pub in publishes:
            key = pub["key"]
            target_id = kad_id(key)
            nearest = min(b.peers, key=lambda nid: xor_distance(nid, target_id))
            req = {"type": "FIND_VALUE", "key": key, "key_id": target_id,
                   "from": b.node_id, "msg_id": hashlib.sha256(
                       (key + str(time.perf_counter_ns())).encode()).hexdigest()[:16]}

            t0 = time.perf_counter()
            b.send(req, b.peers[nearest])
            msg, addr = a.recv(timeout=1.0)
            if msg is None:
                queries.append({"key": key, "hit": False, "error": "no request received"})
                continue
            reply = {
                "type": "FIND_VALUE_REPLY",
                "key": key,
                "value": a.store.get(key),
                "responder": a.node_id,
                "key_id": target_id,
            }
            a.send(reply, addr)
            resp, _ = b.recv(timeout=1.0)
            rtt_ms = (time.perf_counter() - t0) * 1000
            hit = bool(resp and resp.get("value") == pub["value"]
                       and resp.get("responder") == a.node_id)
            queries.append({
                "key": key,
                "target_id": target_id,
                "nearest_node": nearest,
                "xor_distance": str(xor_distance(nearest, target_id)),
                "request_type": msg.get("type"),
                "hit": hit,
                "value_matches": bool(resp and resp.get("value") == pub["value"]),
                "rtt_ms": round(rtt_ms, 4),
            })

        # --- 命名空间隔离：无前缀同名 Key 不得污染带前缀 Key ----------------
        plain_key = SAMPLE_KEYS[0]
        a.store[plain_key] = "urn:btih:" + hashlib.sha1(plain_key.encode()).hexdigest()
        ns_key = NAMESPACE + plain_key
        isolation = {
            "plain_key": plain_key,
            "plain_key_id": kad_id(plain_key),
            "namespaced_key": ns_key,
            "namespaced_key_id": kad_id(ns_key),
            "ids_differ": kad_id(plain_key) != kad_id(ns_key),
            "namespaced_value_unchanged": a.store.get(ns_key) == publishes[0]["value"],
            "all_published_keys_have_prefix": all(p["has_namespace_prefix"] for p in publishes),
        }

        hits = [q for q in queries if q.get("hit")]
        rtts = sorted(q["rtt_ms"] for q in queries if "rtt_ms" in q)
        summary = {
            "publishes": len(publishes),
            "publish_success": sum(1 for p in publishes if p["stored"] and p["has_namespace_prefix"]),
            "publish_success_rate": round(
                sum(1 for p in publishes if p["stored"] and p["has_namespace_prefix"]) / len(publishes), 4),
            "queries": len(queries),
            "find_hits": len(hits),
            "find_hit_rate": round(len(hits) / len(queries), 4) if queries else 0.0,
            "rtt_ms_min": rtts[0] if rtts else None,
            "rtt_ms_max": rtts[-1] if rtts else None,
            "rtt_ms_p50": rtts[len(rtts) // 2] if rtts else None,
        }
    finally:
        a.close()
        b.close()

    report = {
        "schema": "coldrevive.m0.s2-loopback/v1",
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "scope": "本地回环私有 KAD 子网双节点预演（A 发布 / B FIND_VALUE 命中 A）",
        "network": "127.0.0.1 UDP loopback",
        "public_kad_contacted": False,
        "evidence_valid_for_pass": False,
        "namespace": NAMESPACE,
        "nodes": {"A": a.describe(), "B": b.describe()},
        "publishes": publishes,
        "queries": queries,
        "isolation": isolation,
        "summary": summary,
        "criteria": [
            {"id": "S2-C1", "text": "双节点闭环：A 发布带前缀 Key，B 用 FIND_VALUE 命中 A",
             "status": "preview_only", "note": "回环内已跑通，但按判据不得据此判 pass（须实网小样本）"},
            {"id": "S2-C2", "text": "记录发布成功率、查询时延分布、被现有节点丢弃或拒绝的观测",
             "status": "partial", "note": "本地成功率与 RTT 已记录；公共节点丢弃/拒绝观测在回环内不存在"},
            {"id": "S2-C3", "text": "专属命名空间前缀不覆盖 / 不干扰现有 KAD Key 空间",
             "status": "preview_only", "note": "仅验证前缀 Key 与无前缀 Key 的 ID 不同，未接触真实 Key 空间"},
            {"id": "S2-C4", "text": "结论须基于实网小样本；本地回环模拟只能作为预演，不得判为 pass",
             "status": "not_verified", "note": "未执行实网实验"},
        ],
        "s2_status": "not_run",
        "note": ("本脚本只产出预演观测，绝不自动置 pass。实网实验须使用 spikes.json 的 "
                 "public_kad_approval 批准记录，确认最小样本量与限速阈值后，由实验执行人执行并逐次记录。"),
    }

    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    out = REPORT_DIR / f"s2-loopback-{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}.json"
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    print(f"S2 本地回环预演（不构成 S2 结论，public_kad_contacted=False）")
    print("-" * 72)
    print(f"节点 A: {a.describe()}  /  节点 B: {b.describe()}")
    print(f"发布 {summary['publish_success']}/{summary['publishes']} 条 "
          f"(成功率 {summary['publish_success_rate']})")
    print(f"查询命中 {summary['find_hits']}/{summary['queries']} 条 "
          f"(命中率 {summary['find_hit_rate']})")
    print(f"RTT ms: min={summary['rtt_ms_min']} p50={summary['rtt_ms_p50']} max={summary['rtt_ms_max']}")
    print(f"命名空间隔离: ids_differ={isolation['ids_differ']}, "
          f"前缀齐全={isolation['all_published_keys_have_prefix']}, "
          f"带前缀值未变={isolation['namespaced_value_unchanged']}")
    print(f"报告: {out}")
    print("[NOTE] 本地回环只能作为预演；实网小样本结论须人工批准 + 限速 + 逐次原始记录。")
    return 0 if summary["find_hit_rate"] == 1.0 and isolation["ids_differ"] else 2


if __name__ == "__main__":
    sys.exit(run())
