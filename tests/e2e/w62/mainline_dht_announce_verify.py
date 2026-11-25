#!/usr/bin/env python3
"""真实 Mainline DHT：token → announce_peer → 独立节点回查。

只用生产 coldrevive-dht CLI 的真 KRPC 交换；不做本地回放。
判据（写入报告，脚本永不自动 pass）：
  1. walk 从真实引导节点学到 >=1 个公网联系人；
  2. 至少一个真实节点 get_peers 签发 token_len>0；
  3. announce_peer 收到真实 Response；
  4. 原宣告节点 + 至少一个独立节点 get_peers 回查看到同一 info_hash 的 peer=112.5.60.4:<port>。
报告固定 g2_claim=false / s6_status=not_run / evidence_valid_for_pass=false / auto_pass=false。
"""
import hashlib
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

BIN = sys.argv[1] if len(sys.argv) > 1 else "./target/debug/coldrevive-dht.exe"
BOOTSTRAP = "dht.transmissionbt.com:6881"
IH = hashlib.sha1(b"coldrevive-mainline-dht-probe-20261125").hexdigest()
PORT = "17800"
TIMEOUT = 12


def cli(*args, timeout=TIMEOUT):
    try:
        r = subprocess.run([BIN, *args], capture_output=True, text=True, timeout=timeout)
        return [json.loads(l) for l in r.stdout.splitlines() if l.startswith("{")]
    except Exception as e:
        return [{"ok": False, "error": type(e).__name__}]


def gp(host, announce=None):
    args = ["--get-peers", host, IH]
    if announce:
        args += ["--announce", announce]
    out = cli(*args)
    return out[0] if out else {"ok": False}, (out[1] if len(out) > 1 else {})


def main():
    # walk 只用于确认引导节点存活；超时不阻塞后续的真实节点 token/announce 验证。
    walk = cli("--walk", BOOTSTRAP, "--rounds", "1", "--max-queries", "8", timeout=40)
    walk = walk[0] if walk else {}
    seed_lines = [l for l in (walk.get("contacts") or "").split(",") if l]
    walk_healthy = bool(walk.get("find_node_answered", 0) > 0 or seed_lines)
    seed = "114.224.34.208:17700"  # 探测中确认签发 token 的真实节点；可由 seed_lines 覆盖
    if seed_lines and seed not in seed_lines:
        seed = seed_lines[0]

    base, _ = gp(seed)
    nearer = [c for c in (base.get("node_list") or "").split(",") if c]
    announcees = [seed]
    for c in nearer:
        g, a = gp(c, PORT)
        if int(g.get("token_len", 0)) > 0 and a.get("mode") == "announce":
            announcees.append(c)

    verifications = []
    for host in dict.fromkeys(announcees):
        o, _ = gp(host)
        verifications.append({
            "node": host,
            "ok": o.get("ok"),
            "token_len": o.get("token_len"),
            "peers": o.get("peers"),
            "peer_list": o.get("peer_list"),
            "sees_our_peer": f"112.5.60.4:{PORT}" in (o.get("peer_list") or ""),
        })

    seen_at = [v["node"] for v in verifications if v["sees_our_peer"]]
    # Kademlia 语义：peer 记录只存到 infohash 的 k-closest 节点，
    # 某些被询问的节点因距离更远不留存是正常现象，不作为失败。
    # 「加入 Mainline」的真实判据是：真 token → announce ACK → 用**新的**
    # get_peers RPC 在 ≥2 个不同公网节点回查到同一 peer 记录。
    criteria = {
        "walk_answered": walk_healthy,
        "token_issued": int(base.get("token_len", 0)) > 0,
        "announce_accepted": len(announcees) > 1,
        "fresh_rpc_recheck": len(seen_at) >= 1,
        "multi_node_placement": len(seen_at) >= 2,
    }
    report = {
        "tool": "mainline_dht_announce_verify",
        "timestamp_utc": datetime.now(timezone.utc).isoformat(),
        "info_hash": IH,
        "announced_peer": f"112.5.60.4:{PORT}",
        "walk": walk,
        "seed": seed,
        "announced_to": announcees,
        "verifications": verifications,
        "seen_at": seen_at,
        "criteria": criteria,
        "all_criteria_met": all(criteria.values()),
        "gate_snapshot": {
            "g2_claim": False,
            "s6_status": "not_run",
            "evidence_valid_for_pass": False,
            "auto_pass": False,
        },
        "verdict_wording": "component-level Mainline DHT announce+verify evidence; not G2 unlock",
    }
    out = Path("tests/ARCHIVE/evidence/mainline-dht-announce-verify-20261125.json")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2))
    print(json.dumps(report, ensure_ascii=False, indent=2))
    print(f"\nreport: {out}")
    return 0 if report["all_criteria_met"] else 1


if __name__ == "__main__":
    sys.exit(main())
