#!/usr/bin/env bash
# S5 真实内核拥塞链路测量（221, root）：tc htb+netem 跨 netns veth。
# 前景 TCP = iperf3（跨 netns 穿 tc）；后台 = 真实 librqbit-utp（双进程，分处两 netns）。
# 每轮只开一条连续 iperf3 会话，比较同一会话内的 baseline / concurrent / recovery 区间。
set -u
source ~/.cargo/env 2>/dev/null || true
REPO=${CR_REPO:-/opt/coldrevive/relaybuild}
HARNESS=$REPO/tests/spikes/s5/utp_netem
BIN=$HARNESS/target/release/coldrevive-s5-utp-netem
WORK=/opt/coldrevive/s5netem
LINK=16
DELAY=20ms
CONGESTION=${S5_CONGESTION:-bbr}
BASE=${S5_BASE:-5}
CONC=${S5_CONC:-5}
REC=${S5_REC:-5}
TOTAL=$((BASE + CONC + REC))
ROUNDS=${S5_ROUNDS:-3}
RATIOS=${S5_RATIOS:-0.2,0.05,0.02}

cleanup() {
  ip netns del s5srv 2>/dev/null || true
  ip link del v5a 2>/dev/null || true
  pkill -f '[i]perf3 -s' 2>/dev/null || true
}
trap cleanup EXIT

echo "=== 0) cleanup ==="; cleanup; sleep 0.5
echo "=== 1) veth + netns ==="
ip link add v5a type veth peer name v5b || exit 1
ip netns add s5srv || exit 1
ip link set v5b netns s5srv
ip addr add 10.201.0.1/24 dev v5a
ip netns exec s5srv ip addr add 10.201.0.2/24 dev v5b
ip link set v5a up
ip netns exec s5srv ip link set v5b up
ip netns exec s5srv ip link set lo up
sleep 1
echo "=== 2) tc htb + netem on v5a egress ==="
tc qdisc add dev v5a root handle 1: htb default 10 || exit 1
tc class add dev v5a parent 1: classid 1:10 htb rate ${LINK}mbit ceil ${LINK}mbit burst 64k || exit 1
tc qdisc add dev v5a parent 1:10 handle 10: netem delay $DELAY 2ms limit 1000 || exit 1
tc -s qdisc show dev v5a | head -4
echo "=== 3) netem 验证（跨 netns ping，应 ~40ms RTT）==="
ping -c 3 10.201.0.2 | tail -3
echo "=== 4) 编译 harness ==="
cd "$HARNESS" || exit 1
cargo build --offline --release 2>&1 | tail -4 || exit 1
mkdir -p "$WORK/reports"

# iperf3 server in netns
start_iperf() {
  ip netns exec s5srv iperf3 -s -p 5201 >"$WORK/iperf.log" 2>&1 &
  IPERF_PID=$!
  sleep 0.6
}
stop_iperf() { kill $IPERF_PID 2>/dev/null || true; wait $IPERF_PID 2>/dev/null || true; }

echo "=== 5) 测量：单会话 iperf3 baseline/concurrent/recovery ==="
: > "$WORK/results.tsv"
start_iperf
for ratio in $(echo "$RATIOS" | tr ',' ' '); do
  capmbps=$(python3 -c "print(f'{$LINK*$ratio:.4f}')")
  for r in $(seq 1 $ROUNDS); do
    port=$((26000 + RANDOM % 1000))
    tag=$(echo "${ratio}_${r}" | tr '.' '_')

    # uTP server 先起；生命周期覆盖整个 uTP client 窗口。
    ip netns exec s5srv "$BIN" --role server --ip 10.201.0.2 --port $port \
      --seconds $((BASE + CONC + 3)) --stat "$WORK/utp_s_${tag}.json" \
      >"$WORK/utp_s_${tag}.log" 2>&1 &
    US=$!
    sleep 0.3

    # 一条连续 iperf3 会话，覆盖 baseline(BASE s) + concurrent(CONC s) + recovery(REC s)。
    iperf3 -c 10.201.0.2 -p 5201 -t "$TOTAL" -i 1 -J --congestion "$CONGESTION" \
      >"$WORK/fg_${tag}.json" 2>"$WORK/fg_${tag}.err" &
    IP=$!

    # 大约 0.3 s 后 iperf3 才真正开始；这里按 iperf3 起点近似对齐。
    sleep "$BASE"
    "$BIN" --role client --ip 10.201.0.2 --port $port \
      --seconds "$CONC" --cap-mbps "$capmbps" --stat "$WORK/utp_c_${tag}.json" \
      >"$WORK/utp_c_${tag}.log" 2>&1 &
    UC=$!

    wait $IP 2>/dev/null || true
    wait $UC 2>/dev/null || true
    wait $US 2>/dev/null || true

    python3 - "$WORK" "$tag" "$BASE" "$CONC" "$TOTAL" "$ratio" "$r" "$capmbps" "$LINK" >>"$WORK/results.tsv" <<'PY'
import json, os, statistics, sys
work, tag, base_s, conc_s, total_s, ratio, rnd, cap, link = sys.argv[1:]
base_s, conc_s, total_s = int(base_s), int(conc_s), int(total_s)
fg = os.path.join(work, f"fg_{tag}.json")
uc = os.path.join(work, f"utp_c_{tag}.json")
try:
    d = json.load(open(fg))
    iv = []
    for x in d.get("intervals", []):
        s = x.get("sum", {})
        iv.append((float(s.get("start", 0.0)), float(s.get("bits_per_second", 0.0))))
except Exception:
    iv = []
try:
    written = int(json.load(open(uc)).get("bytes", 0))
except Exception:
    written = 0
# 跳过第一段：TCP 慢启动不属于稳态 baseline。concurrent 也跳过第一秒以避开启动瞬间。
base = [b for s, b in iv if 1 <= s < base_s]
mid  = [b for s, b in iv if base_s + 1 <= s < base_s + conc_s]
rec  = [b for s, b in iv if base_s + conc_s <= s < total_s]
valid = len(base) >= 2 and len(mid) >= 2 and written > 0
mb = statistics.median(base) if base else 0.0
mm = statistics.median(mid) if mid else 0.0
deg = ((mb - mm) / mb * 100.0) if mb > 0 else 0.0
utp_mbps = written * 8.0 / (conc_s * 1_000_000.0)
actual_ratio = utp_mbps / float(link)
row = {
    "ratio": float(ratio), "round": int(rnd), "cap_mbps": float(cap),
    "base_median": int(mb), "conc_median": int(mm),
    "rec_median": int(statistics.median(rec)) if rec else 0,
    "deg_pct": round(deg, 2), "utp_written": written,
    "utp_mbps": round(utp_mbps, 4), "actual_ratio": round(actual_ratio, 4),
    "n_base": len(base), "n_conc": len(mid), "n_rec": len(rec), "valid": valid,
}
print("\t".join(str(row[k]) for k in
      ("ratio", "round", "cap_mbps", "base_median", "conc_median",
       "rec_median", "deg_pct", "utp_written", "utp_mbps", "actual_ratio",
       "n_base", "n_conc", "n_rec", "valid")))
PY
    tail -1 "$WORK/results.tsv"
  done
done
stop_iperf

echo "=== 6) 汇总 ==="
python3 - "$WORK" "$LINK" "$DELAY" "$CONGESTION" "$BASE" "$CONC" "$REC" "$ROUNDS" "$TOTAL" <<'PY'
import json, os, statistics, sys, datetime
work, link, delay, congestion, base_s, conc_s, rec_s, rounds, total_s = sys.argv[1:]
cols = ("ratio", "round", "cap_mbps", "base_median", "conc_median",
        "rec_median", "deg_pct", "utp_written", "utp_mbps", "actual_ratio",
        "n_base", "n_conc", "n_rec", "valid")
rows = []
for line in open(os.path.join(work, "results.tsv")):
    p = line.rstrip("\n").split("\t")
    if len(p) != len(cols):
        continue
    d = dict(zip(cols, p))
    d["ratio"] = float(d["ratio"]); d["round"] = int(d["round"])
    d["cap_mbps"] = float(d["cap_mbps"])
    d["utp_mbps"] = float(d["utp_mbps"]); d["actual_ratio"] = float(d["actual_ratio"])
    for k in ("base_median", "conc_median", "rec_median", "utp_written",
              "n_base", "n_conc", "n_rec"):
        d[k] = int(d[k])
    d["deg_pct"] = float(d["deg_pct"]); d["valid"] = d["valid"] == "True"
    rows.append(d)
per = []
for ratio in sorted(set(x["ratio"] for x in rows)):
    good = [x for x in rows if x["ratio"] == ratio and x["valid"]]
    ds = [x["deg_pct"] for x in good]
    med = round(statistics.median(ds), 2) if ds else None
    per.append({
        "bg_ratio": ratio, "valid_rounds": len(good),
        "actual_ratio_median": round(statistics.median(
            [x["actual_ratio"] for x in good]), 4) if good else None,
        "deg_median": med, "deg_all": ds,
        "meets_5pct": bool(ds and med <= 5.0),
    })
rep = {
    "spike": "S5",
    "harness": "tests/spikes/s5/utp_netem (真实内核 tc htb+netem, 跨 netns veth)",
    "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "method": "每轮一条连续 iperf3 TCP(BBR) 会话(-i 1)，同一会话内比较 baseline/concurrent/recovery 区间；后台真实 librqbit-utp 0.7.0 双进程跨 netns",
    "environment": {
        "link_mbps": int(link), "netem_delay": delay, "tcp_congestion": congestion,
        "baseline_seconds": int(base_s), "concurrent_seconds": int(conc_s),
        "recovery_seconds": int(rec_s), "total_seconds": int(total_s),
        "rounds": int(rounds), "tc_verified": True, "cross_netns": True,
        "note": "ping 跨 netns RTT 见 stdout（应 ~40ms 证明 netem 生效）",
    },
    "source_finding": {
        "crate": "librqbit-utp 0.7.0", "ledbat": False,
        "controllers": ["Cubic"],
        "note": "uTP 仅切传输层无 LEDBAT 退让；≤5% 依赖 qos ratio 降级",
    },
    "evidence_valid": bool(rows and all(x["valid"] for x in rows)),
    "samples": rows,
    "per_ratio": per,
    "scope": "真实内核整形链路 + 真实 uTP 栈；非用户态令牌桶",
    "spike_status_changed": False,
    "g2_claim": False,
    "auto_pass": False,
}
out = os.path.join(work, "reports", "s5-utp-netem.json")
os.makedirs(os.path.dirname(out), exist_ok=True)
open(out, "w").write(json.dumps(rep, ensure_ascii=False, indent=2) + "\n")
print("report:", out)
for p in per:
    print("ratio", p["bg_ratio"], "valid", p["valid_rounds"],
          "actual_ratio", p["actual_ratio_median"],
          "median deg", p["deg_median"], "% <=5%:", p["meets_5pct"])
PY
