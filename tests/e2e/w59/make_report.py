"""从 W59 的**原始产物**生成 report.json，并对关键结论做断言。

原则：报告里的数字全部读自 `reports/*.json`、`relay_before.json`、`relay_after.json`，
不手写、不补录。任何一条断言失败就直接非零退出，避免「跑挂了但报告很好看」。

用法（在仓库根目录）:
    python tests/e2e/w59/make_report.py
"""
import json
import sys

R = "tests/e2e/w59/reports/"


def load(p):
    with open(p, encoding="utf-8") as f:
        return json.load(f)


def main():
    wake = load(R + "wake.json")
    holder = load(R + "holder_health.json")
    member = load(R + "member_health.json")
    tell = load(R + "tellstatus.json")["result"]
    relay_b = load("tests/e2e/w59/relay_before.json")
    relay_a = load("tests/e2e/w59/relay_after.json")
    conn = load(R + "relay_conn_summary.json")
    md5_h = open(R + "md5_holder.txt", encoding="utf-8").read().split()[0]
    md5_m = open(R + "md5_member.txt", encoding="utf-8").read().split()[0]

    hd = holder["holder_data"]
    dp = member["data_plane"]

    checks = {
        "download_complete": tell.get("status") == "complete",
        "completed_bytes_1048576": tell.get("completedLength") == "1048576",
        "md5_identical": md5_h == md5_m,
        "wake_path_is_relay": wake.get("path") == "relay",
        "wake_ack_ok": wake.get("ack_status") == "OK",
        "tunnel_confirmed_by_peer": any("OPENED" in n for n in wake.get("notes", [])),
        "holder_splice_done": hd["splices_done"] == 1 and hd["accepted"] == 1,
        "byte_identity_1264_plus_1049585": hd["bytes_to_engine"] + hd["bytes_from_engine"]
        == dp["bytes_relayed"],
        "relay_dropped_zero": (relay_a["dropped"] - relay_b["dropped"]) == 0,
        "relay_admitted_matches_payload": (
            relay_a["data_bytes_admitted"] - relay_b["data_bytes_admitted"]
        )
        == 1050048,
        "no_bandwidth_limited": (
            relay_a["bandwidth_limited"] - relay_b["bandwidth_limited"]
        )
        == 0,
        "tunnel_pump_alive": dp["tunnels_alive"] == 1,
        "both_public_ips_on_relay": conn["both_ips_present"] == conn["samples"],
    }

    report = {
        "run": "w59",
        "title": "公网双端「打洞失败 → 中继接管」BT 数据面（v2.4.93 修复后）",
        "version": "v2.4.93",
        "date": "2026-11-19",
        "topology": {
            "holder": {
                "host": "222",
                "lan": "192.168.34.222",
                "public_egress": "112.5.32.34",
                "role": "holder",
                "bind_bt": "0.0.0.0:17902",
            },
            "member": {
                "host": "A",
                "lan": "192.168.1.210",
                "public_egress": "112.5.60.4",
                "role": "member",
                "bind_bt": "0.0.0.0:17802",
            },
            "relay": {
                "addr": "82.157.68.159:7100",
                "auth": "psk",
                "key_fp": "037d79762181690e",
                "carrier": "tcp",
            },
            "stun": "82.157.68.159:3478/3479",
        },
        "observed": {
            "wake": {
                "path": wake.get("path"),
                "ack_status": wake.get("ack_status"),
                "direct_candidate": wake["attempts"][0]["addr"],
                "direct_ok": wake["attempts"][0]["ok"],
                "direct_rtt_ms": wake["attempts"][0]["rtt_ms"],
                "relay_hop": "真实 DELIVERED 帧",
                "tunnel": "OPEN/OPENED 双向握手完成",
            },
            "download": {
                "status": tell.get("status"),
                "completed_length": tell.get("completedLength"),
                "total_length": tell.get("totalLength"),
                "connections": tell.get("connections"),
                "md5_holder": md5_h,
                "md5_member": md5_m,
            },
            "holder_data": hd,
            "holder_seed": {
                "seed_active": holder["seed_active"],
                "served_bytes": holder["served_bytes"],
            },
            "member_data_plane": dp,
            "relay_delta": {
                k: relay_a[k] - relay_b[k]
                for k in [
                    "connections",
                    "registered",
                    "delivered",
                    "dropped",
                    "data_bytes_admitted",
                    "bandwidth_limited",
                    "rate_limited",
                    "rejected",
                    "bad_tag",
                ]
            },
            "relay_conn_sampling": conn,
        },
        "assertions": checks,
        "all_pass": all(checks.values()),
        "boundaries": [
            "**不是 NAT 可达性证据**：两台主机在 192.168.1.0/24 上还有一条管理面直连"
            "（222 有 192.168.1.222 别名），所以「跨 NAT 不可直连」靠的是**通告地址不可达**"
            "（192.168.34.222 / 203.0.113.31 都从 A 不可达），不是物理隔离；"
            "nat_type=unknown、s6_status=not_run、g2_claim=false。",
            "**控制面走 LAN**：advertise 与种子文件经 192.168.1.222:17900 取；"
            "只有 BT 字节流被强制走公网中继。",
            "**单次 1 MiB**：未做吞吐、并发、长稳测试；中继默认 512 KiB/s 数据令牌桶。",
            "**打洞失败是观测结论而非设置**：直连候选 192.168.34.222:17901 从 A 不可达"
            "（1204 ms 无应答）；本轮 STUN 未产出映射（nat_type=unknown），"
            "故只能说「本轮直连不可用 → 中继接管」。",
        ],
        "bug_found": "v2.4.92 的 RELAY 读帧用 read_exact，轮询读超时落在记录中间会丢掉已读字节"
        "并导致流错位；表现为持有者写完 1 MiB、下载方只收到 1441 B、中继 dropped 暴涨。",
        "bug_fixed": "v2.4.93 改为可续读缓冲（cr_nat::secure::fill_pending），"
        "读超时只表示「本轮未凑齐」，缓冲保留，凑齐才解析；明文与加密两条路径共用。",
    }

    with open(R + "report.json", "w", encoding="utf-8") as f:
        json.dump(report, f, ensure_ascii=False, indent=2)

    for k, v in checks.items():
        print(("  OK  " if v else "  FAIL") + " " + k)
    print("all_pass =", report["all_pass"])
    return 0 if report["all_pass"] else 1


if __name__ == "__main__":
    sys.exit(main())
