#!/bin/sh
# W59：公网双端「打洞失败 → 中继接管」数据面实测
# 持有者在 222（真 NAT 112.5.32.34），下载方在本机 A（真 NAT 112.5.60.4），
# 中继在公网 82.157.68.159:7100（PSK 认证）。
#
# 用法: w59-run.sh <infohash> <file_hash>
set -e
H=http://192.168.1.222:17900
M=http://127.0.0.1:17800
OUT=tests/e2e/w59/reports
IH=$1; FH=$2
mkdir -p "$OUT"
rm -f "$OUT"/*.json "$OUT"/gid.txt "$OUT"/md5*.txt

curl -s --max-time 8 "$H/nat/advertise" > "$OUT/advertise.json"
echo "1/6 advertise: $(wc -c < "$OUT/advertise.json") B"

curl -s --max-time 10 -X POST -d "{\"command\":\"nat\",\"args\":[\"peer\",\"$H/nat/advertise\"]}" "$M/cli" > "$OUT/peer.json"
echo "2/6 peer: $(cat "$OUT/peer.json")"

curl -s --max-time 10 -X POST -d '{"command":"nat","args":["plan"]}' "$M/cli" > "$OUT/plan.json"
echo "3/6 plan: $(cat "$OUT/plan.json")"

curl -s --max-time 40 -X POST -d "{\"command\":\"nat\",\"args\":[\"wake\",\"$FH\",\"$IH\"]}" "$M/cli" > "$OUT/wake.json"
echo "4/6 wake: $(cat "$OUT/wake.json")"

GID=$(curl -s --max-time 30 -X POST \
  -d "{\"jsonrpc\":\"2.0\",\"id\":\"1\",\"method\":\"aria2.addUri\",\"params\":[[\"$H/torrent/$IH\"]]}" \
  "$M/jsonrpc" | sed -n 's/.*"result":"\([^"]*\)".*/\1/p')
echo "$GID" > "$OUT/gid.txt"
echo "5/6 gid=$GID"
i=0
while [ $i -lt 45 ]; do
  sleep 2
  curl -s --max-time 10 -X POST -d "{\"jsonrpc\":\"2.0\",\"id\":\"2\",\"method\":\"aria2.tellStatus\",\"params\":[\"$GID\"]}" "$M/jsonrpc" > "$OUT/tellstatus.json"
  # 注意：不能用贪婪 sed 取 "status"——tellStatus 里 uris[].status 在后面，
  # 贪婪匹配会取到 URI 的 "used"，看着像任务状态。必须按 JSON 解析。
  PROG=$(python -c "import json;d=json.load(open(r'$OUT/tellstatus.json'))['result'];print(d.get('status'),d.get('completedLength'))")
  ST=${PROG%% *}
  COMP=${PROG##* }
  echo "   t=$((i*2))s status=$ST completed=$COMP"
  [ "$ST" = "complete" ] && break
  i=$((i+1))
done

curl -s --max-time 8 "$M/health" > "$OUT/member_health.json"
curl -s --max-time 8 "$H/health" > "$OUT/holder_health.json"
echo "6/6 health 已取"

find tests/e2e/w59/run-member -name "sample.bin" -exec md5sum {} + > "$OUT/md5_member.txt" 2>/dev/null || true
cat "$OUT/md5_member.txt"
echo "--- done"
