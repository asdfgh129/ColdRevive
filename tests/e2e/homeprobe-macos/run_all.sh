#!/bin/bash
# ColdRevive 家宽取证 kit（macOS Ventura / Intel x86_64）
# 运行前请读 README.md：这会向公开 STUN / DHT / KAD 节点发送低速只读探测包。
# 脚本只落 JSON 证据，不会自动判 pass，也不会写回仓库的 spikes.json。
set -uo pipefail
cd "$(dirname "$0")" || exit 1

PY="${PYTHON:-python3}"
if ! command -v "$PY" >/dev/null 2>&1; then
  echo "ERROR: 找不到 python3。macOS 请先安装 Xcode Command Line Tools：xcode-select --install" >&2
  exit 1
fi
if ! "$PY" -c 'import sys; raise SystemExit(0 if sys.version_info >= (3, 8) else 1)'; then
  echo "ERROR: 需要 Python 3.8+（Ventura 自带 Python 3 通常满足）" >&2
  exit 1
fi

mkdir -p results
LABEL="home-$(hostname)"

run_step() {
  name="$1"
  shift
  echo
  echo "=== $name ==="
  "$PY" "$@"
  code=$?
  echo "$code" > "results/${name}.exit"
  if [ "$code" -gt 1 ]; then
    echo "WARN: $name 退出码 $code（资料缺失照记，不补默认值）" >&2
  fi
  return 0
}

echo "ColdRevive home probe @ $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "  kit: $(pwd)"
echo "  python: $($PY --version 2>&1)"

# 0) 纯离线自检：先确认编解码与判据没有被 macOS/Python 版本破坏
run_step selftests probes/dht_readonly_probe.py --self-test
run_step nat_selftest probes/nat_behavior.py --self-test

# 1) 主机与出口
run_step platform probes/platform_info.py --out results/platform.json

# 2) NAT 基础样本（STUN / UPnP / NAT-PMP / CGNAT 启发式）
run_step stun probes/stun_probe.py \
  --timeout 2.0 \
  --stun 119.29.150.254:3478 \
  --stun stun.l.google.com:19302 \
  --stun stun.cloudflare.com:3478 \
  --out results/stun.json

# 3) NAT 行为细分（映射 + 带对照的过滤，充分性不足时如实 undetermined）
run_step nat_behavior probes/nat_behavior.py \
  --label "$LABEL" --timeout 2.0 --tries 2 \
  --mapping-servers 119.29.150.254:3478,stun.l.google.com:19302,stun.cloudflare.com:3478 \
  --filter-servers 119.29.150.254:3478 \
  --out results/nat_behavior.json

# 4) 只读 DHT（ping + get_peers，绝不 announce）
run_step dht probes/dht_readonly_probe.py \
  --config config/dht-targets.json --timeout 2.5 --out results/dht.json

# 5) KAD 专用 UDP 出口（只发 0xE401 BOOTSTRAP_REQ，频率 0.2 pps）
run_step kad_egress probes/kad_egress_probe.py \
  --targets config/kad-targets.json --sample-size 5 --rate-limit 0.2 --timeout 3.0 \
  --attest-i-am-the-experiment-operator --out results/kad_egress.json

# 6) 汇总
run_step report probes/report.py --results results

echo
echo "完成：把整个 results/ 目录（含 *.json 与 *.exit）压缩回传。"
echo "注意：g2_claim=false / s6_status=not_run / evidence_valid_for_pass=false 不会被本脚本改变。"
