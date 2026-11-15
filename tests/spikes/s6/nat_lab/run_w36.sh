#!/bin/bash
# W3.6 双向打洞实验室复测（对照 W3.5② 的 direct 0/30）
#
# 关键点：W3.5② 的 4/5 组里 requester 都在 lan-symmetric 后面，对称 NAT 的映射与
# 目的地址绑定，back 永远报不准 —— 那是双向打洞也救不了的组合。本轮把 requester
# 固定为锥形 NAT，隔离出「持有者侧过滤」这一个变量。
set -u
cd /opt/coldrevive/repo || exit 1
OUT=/opt/coldrevive/reports/w36
rm -rf "$OUT"
mkdir -p "$OUT"
pkill -f "coldrevive-relay --listen" 2>/dev/null
sleep 1

run() {
  local cls=$1 hns=$2 rns=$3 tag=$4
  echo "=== $tag | class=$cls | holder=$hns | requester=$rns"
  python3 tests/spikes/s6/nat_lab/run_matrix_lab.py \
    --class "$cls" --holder-ns "$hns" --requester-ns "$rns" \
    --trials 30 --timeout 10 2>&1 | tail -5
  if cp "/opt/coldrevive/reports/matrix/s6-matrix-class-$cls-lab.json" "$OUT/$tag.json" 2>/dev/null; then
    echo "--- saved $OUT/$tag.json"
  else
    echo "--- MISSING report for $tag"
  fi
  sleep 2   # 让上一轮的中继监听端口彻底释放，避免同端口重启竞态
}

run full-cone            lan-full-cone            lan-port-restricted-cone w36-a-full-cone
run restricted-cone      lan-restricted-cone      lan-port-restricted-cone w36-b-restricted-cone
run port-restricted-cone lan-port-restricted-cone lan-full-cone            w36-c-port-restricted-cone
run symmetric            lan-symmetric            lan-port-restricted-cone w36-d-symmetric
run cgnat                lan-cgnat                lan-port-restricted-cone w36-e-cgnat
run restricted-cone      lan-restricted-cone      lan-symmetric            w36-f-control-symreq

echo "ALL DONE"
