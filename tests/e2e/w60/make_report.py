#!/usr/bin/env python3
"""W60：从原始产物生成「真打洞失败 → 中继接管」证据报告。

所有数字都来自 nat probe / wake / health / md5 的原始输出；断言失败即非零退出。
不写入任何手写数字，避免把结论写进证据。
"""
import json
import os
import re
import sys

R = os.path.join(os.path.dirname(os.path.abspath(__file__)), "reports")


def load(name):
    with open(os.path.join(R, name), encoding="utf-8") as f:
        return json.load(f)


def text(name):
    with open(os.path.join(R, name), encoding="utf-8") as f:
        return f.read()


def main():
    a = load("probe_A.json")
    b = load("probe_222.json")
    forced = load("wake_forced_stun.json")
    wake = load("wake.json")
    member = load("member_health.json")
    holder = load("holder_health.json")
    tell = load("tellstatus.json")["result"]
    md5_a = text("md5_member.txt").split()[0]
    md5_b = text("md5_holder.txt").split()[0]
    holder_log = text("holder_log.txt")

    a_maps = [r["mapped"] for r in a["stun_results"] if r["ok"]]
    b_maps = [r["mapped"] for r in b["stun_results"] if r["ok"]]
    attempt = forced["attempts"][0]
    detail = attempt.get("detail", "")
    hd = holder["holder_data"]
    dp = member["data_plane"]
    note_blob = " ".join(wake.get("notes", []))

    checks = {
        # 1) 真 STUN：两端都有映射，且类别由两次不同目的地址的观测得出
        "probe_A_has_real_mapping": a["public_udp_egress_ok"] and bool(a_maps),
        "probe_A_is_cone_eim": a["nat_type"] == "port-restricted-cone" and len(set(a_maps)) == 1,
        "probe_222_has_real_mapping": b["public_udp_egress_ok"] and bool(b_maps),
        "probe_222_is_symmetric": b["nat_type"] == "symmetric" and len(set(b_maps)) > 1,
        # 2) 强制直连候选：真的发包（双向打洞），真的失败
        "forced_punch_failed": not forced["ok"] and not attempt["ok"],
        "punch_packets_actually_sent": "punch_sent=2" in detail and "acked=false" in detail,
        "peer_did_not_reach_me": "peer_punched=false" in detail,
        "punch_used_real_timeout": "10060" in detail or "timed out" in detail.lower(),
        "peer_sent_punch_back": "punch-back" in holder_log and "真实发出 2 帧" in holder_log,
        # 3) 生产自动路径：中继接管（不是量具旁路）
        "auto_wake_path_relay": wake.get("path") == "relay" and wake.get("ack_status") == "OK",
        "auto_skipped_futile_punch": "relay-only" in note_blob and "跳过全部直连候选" in note_blob,
        "tunnel_two_way_handshake": "OPEN/OPENED" in note_blob,
        # 4) 数据面闭环
        "download_complete": tell.get("status") == "complete",
        "download_bytes_1048576": tell.get("completedLength") == "1048576",
        "md5_identical": md5_a == md5_b,
        "holder_splice_done": hd["splices_done"] == 1 and hd["accepted"] == 1,
        "byte_identity": hd["bytes_to_engine"] + hd["bytes_from_engine"] == dp["bytes_relayed"],
        "tunnel_pump_alive": dp["tunnels_alive"] == 1,
    }

    report = {
        "run": "w60",
        "title": "真 STUN 分类下「打洞失败 → 中继接管」完整链路（v2.4.93）",
        "date": "2026-11-19",
        "topology": {
            "member": {"node": "A", "lan": "192.168.1.210", "egress": "112.5.60.4",
                       "stun_mappings": a_maps, "nat_type": a["nat_type"]},
            "holder": {"node": "222", "lan": "192.168.34.222", "egress": "112.5.32.34",
                       "stun_mappings": b_maps, "nat_type": b["nat_type"]},
            "relay": "82.157.68.159:7100 (PSK, TCP carrier)",
            "stun": "82.157.68.159:3478/3479",
        },
        "observed": {
            "forced_direct_attempt": {
                "kind": attempt["kind"], "target": attempt["addr"], "ok": attempt["ok"],
                "detail": detail, "rtt_ms": attempt["rtt_ms"],
                "self_mapping": next((n for n in forced["notes"] if "本侧映射" in n), None),
                "peer_punch_back": "punch-back" in holder_log,
            },
            "automatic_path": {
                "path": wake.get("path"), "ack_status": wake.get("ack_status"),
                "reach_class": (wake.get("reach_lookup") or {}).get("class"),
                "skip_direct": (wake.get("reach_lookup") or {}).get("skip_direct"),
                "notes": wake.get("notes", []),
            },
            "download": {"status": tell.get("status"), "completedLength": tell.get("completedLength"),
                         "md5_member": md5_a, "md5_holder": md5_b},
            "holder_data": hd,
            "member_data_plane": dp,
            "holder_seed": {"seed_active": holder["seed_active"], "served_bytes": holder["served_bytes"]},
        },
        "assertions": checks,
        "all_pass": all(checks.values()),
        "boundaries": [
            "强制直连是验收量具的显式旁路（wake 第三参数 kind），生产自动路径按对端 relay-only 声明跳过打洞；",
            "  两者共同说明：直连候选确实不可用，且生产没有错误地依赖它。",
            "222 的 STUN 映射是「到 STUN 服务器」的端口（2051/2052）；symmetric NAT 到 A 的映射端口不可预知，",
            "  这正是打洞失败的机制，不是实现缺陷。",
            "控制面（advertise / 种子）经 192.168.1.222:17900 走 LAN；只有 BT 字节流进公网中继。",
            "222 另有 192.168.1.222 管理面别名，A 与 222 存在 L2 通路；本轮不是 NAT 可达性证据。",
            "s6_status=not_run、evidence_valid_for_pass=false、g2_claim=false 不变；单次 1 MiB、无吞吐/并发结论。",
        ],
    }
    with open(os.path.join(R, "report.json"), "w", encoding="utf-8") as f:
        json.dump(report, f, ensure_ascii=False, indent=2)

    for k, v in checks.items():
        print(("  OK   " if v else "  FAIL ") + k)
    print("all_pass =", report["all_pass"])
    return 0 if report["all_pass"] else 1


if __name__ == "__main__":
    sys.exit(main())
