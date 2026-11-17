#!/usr/bin/env bash
# S3 跨进程 RSS 取样：两个真实 Mainline DHT probe 进程并发运行，逐秒采样 Windows working set / private bytes。
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
BIN=$HERE/target/release/coldrevive-s3-mainline-probe.exe
WORK=$HERE/reports
DURATION=${S3_RSS_SECONDS:-30}
KEY=${S3_RSS_KEY:-coldrevive:s3:rss:20261117}
mkdir -p "$WORK"

"$BIN" announce --attest-public-key-test --key-label "$KEY" \
  --seconds "$DURATION" --listen-port 18082 --announce-port 18082 \
  --out "$WORK/s3-rss-announce.json" >"$WORK/s3-rss-announce.log" 2>&1 &
A=$!
"$BIN" query --attest-public-key-test --key-label "$KEY" \
  --seconds "$DURATION" --expect-ip 112.5.60.4 \
  --out "$WORK/s3-rss-query.json" >"$WORK/s3-rss-query.log" 2>&1 &
Q=$!
sleep 1

AWIN=$(ps -p "$A" | awk 'NR==2{print $4}')
QWIN=$(ps -p "$Q" | awk 'NR==2{print $4}')
echo "winpids announce=$AWIN query=$QWIN"

: > "$WORK/s3-rss-samples.csv"
for i in $(seq 1 "$DURATION"); do
  line=$(powershell.exe -NoProfile -Command "\$a=Get-Process -Id $AWIN -ErrorAction SilentlyContinue; \$q=Get-Process -Id $QWIN -ErrorAction SilentlyContinue; if(\$a -and \$q){Write-Output (\$a.WorkingSet64.ToString()+','+\$a.PrivateMemorySize64.ToString()+','+\$q.WorkingSet64.ToString()+','+\$q.PrivateMemorySize64.ToString())} elseif(\$a){Write-Output (\$a.WorkingSet64.ToString()+','+\$a.PrivateMemorySize64.ToString()+',NA,NA')} elseif(\$q){Write-Output ('NA,NA,'+\$q.WorkingSet64.ToString()+','+\$q.PrivateMemorySize64.ToString())} else {Write-Output 'NA,NA,NA,NA'}" | tr -d '\r')
  printf "%s,%s\n" "$i" "$line" >> "$WORK/s3-rss-samples.csv"
  sleep 1
done

wait "$A" 2>/dev/null || true
wait "$Q" 2>/dev/null || true

python3 - "$WORK" "$DURATION" "$KEY" <<'PY'
import csv, json, os, statistics, sys
work, duration, key = sys.argv[1], int(sys.argv[2]), sys.argv[3]
ws_a, pv_a, ws_q, pv_q = [], [], [], []
with open(os.path.join(work, "s3-rss-samples.csv"), newline="") as f:
    for row in csv.reader(f):
        if len(row) != 5:
            continue
        for dst, raw in zip((ws_a, pv_a, ws_q, pv_q), row[1:]):
            if raw != "NA":
                try:
                    dst.append(int(raw))
                except ValueError:
                    pass
def stat(v):
    if not v:
        return None
    return {
        "samples": len(v),
        "min_bytes": min(v),
        "median_bytes": int(statistics.median(v)),
        "max_bytes": max(v),
        "max_mib": round(max(v) / 1024 / 1024, 2),
    }
out = {
    "spike": "S3",
    "check": "cross-process-rss",
    "key_label": key,
    "duration_sec": duration,
    "method": "two concurrent real Mainline DHT probe processes (announce + query), Windows Get-Process WorkingSet64/PrivateMemorySize64 sampled once per second",
    "announce_working_set": stat(ws_a),
    "announce_private_bytes": stat(pv_a),
    "query_working_set": stat(ws_q),
    "query_private_bytes": stat(pv_q),
    "combined_peak_working_set_mib": round((max(ws_a or [0]) + max(ws_q or [0])) / 1024 / 1024, 2),
    "combined_peak_private_mib": round((max(pv_a or [0]) + max(pv_q or [0])) / 1024 / 1024, 2),
    "within_srs_nfr05_100mb": (max(ws_a or [0]) + max(ws_q or [0])) <= 100 * 1024 * 1024,
    "evidence_valid": bool(ws_a and ws_q),
    "evidence_valid_for_pass": False,
    "spike_status_changed": False,
    "g2_claim": False,
    "auto_pass": False,
}
path = os.path.join(work, "s3-cross-process-rss-20261117.json")
open(path, "w", encoding="utf-8").write(json.dumps(out, ensure_ascii=False, indent=2) + "\n")
print(json.dumps(out, ensure_ascii=False, indent=2))
PY
