#!/bin/sh
# 采样时间线：两端 /health + 中继统计。用于定位「字节去哪了」。
# 用法: poll.sh <输出文件> <采样次数>
OUT=$1
N=$2
i=0
: > "$OUT"
while [ $i -lt "$N" ]; do
  TS=$(date +%s%3N 2>/dev/null || date +%s)
  M=$(curl -s --max-time 3 http://127.0.0.1:17800/health | tr -d '\n')
  H=$(curl -s --max-time 3 http://192.168.1.222:17900/health | tr -d '\n')
  echo "{\"ts\":$TS,\"member\":$M,\"holder\":$H}" >> "$OUT"
  i=$((i+1))
  sleep 0.5
done
echo "poll done: $i samples"
