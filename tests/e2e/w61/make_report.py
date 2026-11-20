#!/usr/bin/env python3
"""W61 证据判定：只读原始产物，逐条断言，失败即非零退出。

判据刻意保守：
- TCP 对照允许多轮里**一次**偶发成功（ADR-020 §复盘已记录这种现象），但必须
  绝大多数轮次被强制断开；不能用单次成功推翻 RST 结论，也不能假装 0/12。
- UDP 数据面必须有 wake 的 `relay-udp` 路径、OPEN/OPENED 备注、md5 两端一致、
  中继三方对账，且 L2 管理口没有 BT 字节（只有控制面 HTTP）。
"""
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent
OUT = ROOT / "reports"


def load(name):
    with (OUT / name).open(encoding="utf-8") as fh:
        return json.load(fh)


def main():
    checks = []

    def check(name, ok, detail):
        checks.append({"name": name, "ok": bool(ok), "detail": detail})

    rst = load("tcp_rst_probe.json")
    resets = sum(1 for r in rst["results"] if "ConnectionReset" in (r["error"] or ""))
    check("tcp_control_multi_round", rst["rounds"] >= 12 and resets >= 10,
          f"passed={rst['passed']}/{rst['rounds']}, resets={resets}")

    probe_a = load("probe_A.json")
    probe_b = load("probe_222.json")
    check("two_nat_classification",
          probe_a.get("nat_type") == "port-restricted-cone"
          and probe_b.get("nat_type") == "symmetric",
          f"A={probe_a.get('nat_type')}, 222={probe_b.get('nat_type')}")

    wake = load("wake.json")
    notes = " ".join(wake.get("notes") or [])
    check("wake_via_udp_relay",
          wake.get("path") == "relay-udp" and wake.get("ack_status") == "OK"
          and "OPEN/OPENED" in notes and "UDP" in notes,
          f"path={wake.get('path')} ack={wake.get('ack_status')}")

    status = load("tellstatus.json")["result"]
    # aria2 的 completedLength 是字符串，不能拿整数比较（会假 FAIL）。
    completed = int(status.get("completedLength", 0))
    check("piece_download_complete",
          status.get("status") == "complete" and completed == 1048576,
          f"status={status.get('status')} bytes={completed}")

    md5_member = (OUT / "md5_member.txt").read_text(encoding="utf-8")
    md5_holder = (OUT / "md5_holder.txt").read_text(encoding="utf-8")
    check("md5_identical", md5_member.split()[0] == md5_holder.split()[0],
          f"member={md5_member.split()[0]} holder={md5_holder.split()[0]}")

    mid_m = load("member_health_mid.json")["data_plane"]
    mid_h = load("holder_health_mid.json")["holder_data"]
    end_m = load("member_health.json")["data_plane"]
    end_h = load("holder_health.json")["holder_data"]
    check("member_udp_tunnel_alive_during_transfer",
          mid_m.get("carrier") == "relay-udp"
          and mid_m.get("udp_tunnels_alive", 0) >= 1,
          f"mid carrier={mid_m.get('carrier')} alive={mid_m.get('udp_tunnels_alive')}")
    check("holder_carrier_label_is_udp",
          mid_h.get("carrier") == "relay-sub-udp"
          and end_h.get("carrier") == "relay-sub-udp",
          f"mid={mid_h.get('carrier')} end={end_h.get('carrier')}")
    check("holder_spliced_bt_bytes",
          end_h.get("opens", 0) >= 1 and end_h.get("splices_done", 0) >= 1
          and end_h.get("bytes_from_engine", 0) >= 1048576,
          f"opens={end_h.get('opens')} splices={end_h.get('splices_done')} "
          f"from_engine={end_h.get('bytes_from_engine')}")
    check("member_counted_relayed_bytes",
          end_m.get("bytes_relayed", 0) >= 1048576,
          f"bytes_relayed={end_m.get('bytes_relayed')}")

    before = load("relay_before.json")
    after = load("relay_after.json")
    delta = {k: after.get(k, 0) - before.get(k, 0)
             for k in ("data_in", "deliver_out", "status_out", "data_bytes_in",
                       "bandwidth_limited", "bandwidth_limited_bytes", "replay",
                       "mac_fail", "too_big", "bad_format", "not_registered")}
    check("relay_three_way_accounting",
          delta["data_in"] > 0 and delta["deliver_out"] > 0
          and delta["status_out"] == delta["data_in"]
          and delta["data_bytes_in"] >= 1048576,
          json.dumps(delta, ensure_ascii=False))
    check("relay_no_silent_drop",
          delta["bandwidth_limited"] == 0 and delta["replay"] == 0
          and delta["mac_fail"] == 0 and delta["too_big"] == 0
          and delta["bad_format"] == 0,
          json.dumps(delta, ensure_ascii=False))

    l2 = (OUT / "l2_capture.txt").read_text(encoding="utf-8", errors="replace")
    # tcpdump 用 `host.port` 写法（192.168.1.222.17900），不是 `:port`。
    bt_lines = [line for line in l2.splitlines()
                if "IP " in line and re.search(r"\.17902\b", line)]
    non_control = [line for line in l2.splitlines()
                   if "IP " in line and not re.search(r"\.17900\b", line)]
    check("no_l2_bt_shortcut", not bt_lines and not non_control,
          f"bt_lines={len(bt_lines)} non_control_lines={len(non_control)}")

    report = {
        "result": "PASS" if all(c["ok"] for c in checks) else "FAIL",
        "scope": "two-home-NAT (A cone / 222 symmetric), public UDP relay "
                 "119.29.150.254:7101, TCP control path on same host still RSTed",
        "checks": checks,
        "relay_delta": delta,
        "rst": {"passed": rst["passed"], "rounds": rst["rounds"], "resets": resets},
        "boundaries": [
            "control plane via LAN 192.168.1.222:17900; L2 capture shows no BT bytes",
            "single 1 MiB run; no congestion/fairness conclusion",
            "g2_claim=false, s6_status=not_run",
        ],
    }
    (OUT / "report.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    for c in checks:
        print(("PASS" if c["ok"] else "FAIL"), c["name"], "-", c["detail"])
    print("TOTAL", report["result"], f"{sum(1 for c in checks if c['ok'])}/{len(checks)}")
    return 0 if report["result"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
