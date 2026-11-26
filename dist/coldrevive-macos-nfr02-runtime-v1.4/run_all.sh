#!/bin/bash
# ColdRevive NFR-02 macOS 真机运行包（v1.4，源码 HEAD e698d4b）
# 只在 127.0.0.1 上起本地守护进程，不连公网 KAD/中继，不向任何外部节点发包。
# 本脚本只落原始证据与逐项判定，不会写 g2_claim，也不会自动宣布 NFR-02 达标。
# v1.4: 不再信任 PYTHON 环境变量（某些 shell 里被设成 "?" 导致工具检查误报）；
#       工具检查改为显式函数 + ASCII 报错，避免中文标点与变量相邻。
set -o pipefail
cd "$(dirname "$0")" || exit 1

PY=python3
ROOT="$(pwd)"
HEALTH="http://127.0.0.1:19600/health"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "ERROR: this kit is for macOS only; got $(uname -s)" >&2
  exit 2
fi
check_tool() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf 'ERROR: required tool not found: [%s]\n' "$1" >&2
    printf 'Install Rust via rustup and Xcode Command Line Tools: xcode-select --install\n' >&2
    exit 2
  fi
}
check_tool "$PY"
check_tool cargo
check_tool rustc
check_tool curl
if ! "$PY" -c 'import sys; raise SystemExit(0 if sys.version_info >= (3, 8) else 1)'; then
  echo "ERROR: 需要 Python 3.8+" >&2
  exit 2
fi
if ! xcode-select -p >/dev/null 2>&1; then
  echo "WARN: xcode-select 未配置，cargo 构建可能在 C 依赖处失败：xcode-select --install" >&2
fi

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
RESULTS="$ROOT/results/$RUN_ID"
mkdir -p "$RESULTS" "$ROOT/run/on" "$ROOT/run/off"

echo "ColdRevive NFR-02 macOS runtime kit"
echo "  run_id:  $RUN_ID"
echo "  kit:     $ROOT"
echo "  results: $RESULTS"
echo "  python:  $($PY --version 2>&1)"

{
  echo "run_id=$RUN_ID"
  echo "utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "local=$(date +%Y-%m-%dT%H:%M:%S%z)"
  echo "hostname=$(hostname)"
  echo "uname=$(uname -a)"
  echo "arch=$(uname -m)"
  echo "sw_vers=$(sw_vers 2>/dev/null | tr '\n' ' ')"
  echo "cargo=$(cargo --version 2>&1)"
  echo "rustc=$(rustc --version 2>&1)"
  echo "curl=$(curl --version 2>/dev/null | head -n 1)"
  echo "xcode_select=$(xcode-select -p 2>&1)"
  echo "source_commit=$(grep '^git_commit=' SOURCE_INFO.txt | cut -d= -f2)"
} > "$RESULTS/preflight.txt"

"$PY" - "$RESULTS/platform.json" "$RESULTS/preflight.txt" <<'PY'
import json, sys
out = {}
for line in open(sys.argv[2], encoding="utf-8", errors="replace"):
    if "=" in line:
        k, v = line.rstrip("\n").split("=", 1)
        out[k] = v
with open(sys.argv[1], "w", encoding="utf-8") as fh:
    json.dump(out, fh, ensure_ascii=False, indent=2)
    fh.write("\n")
PY

if curl -fsS --max-time 1 "$HEALTH" >/dev/null 2>&1; then
  echo "ERROR: 127.0.0.1:19600 已有服务在应答；先停掉旧 coldrevived 再跑，避免测到错误进程" >&2
  exit 2
fi

echo
echo "=== 1/6 cargo test -p cr-engine --lib ==="
code=0
( cd "$ROOT/source" && CARGO_INCREMENTAL=0 cargo test -p cr-engine --lib --locked -- --test-threads=1 ) \
  > "$RESULTS/01-cargo-test.log" 2>&1 || code=$?
echo "$code" > "$RESULTS/01-cargo-test.exit"
echo "exit=$code  log=$RESULTS/01-cargo-test.log"

echo
echo "=== 2/6 cargo build --bin coldrevived ==="
code=0
( cd "$ROOT/source" && CARGO_INCREMENTAL=0 cargo build --bin coldrevived --locked ) \
  > "$RESULTS/02-build.log" 2>&1 || code=$?
echo "$code" > "$RESULTS/02-build.exit"
echo "exit=$code  log=$RESULTS/02-build.log"
BIN="$ROOT/source/target/debug/coldrevived"
if [ "$code" -ne 0 ] || [ ! -x "$BIN" ]; then
  echo "ERROR: 构建失败，后续 daemon 阶段跳过；请把 $RESULTS 回传" >&2
  "$PY" "$ROOT/probes/collect_evidence.py" --results "$RESULTS" --root "$ROOT" || true
  exit 0
fi

start_and_sample() {
  cfg="$1"
  tag="$2"
  echo
  echo "=== daemon $tag: $cfg ==="
  # 必须 exec：否则 $! 是子 shell 的 PID，kill 后真正的 daemon 可能残留，
  # 下一阶段会连到旧进程并撞 UDP 端口，导致负向对照假失败。
  ( cd "$ROOT" && exec "$BIN" --config "$cfg" --node-id A ) \
    > "$RESULTS/daemon-$tag.log" 2>&1 &
  pid=$!
  echo "$pid" > "$RESULTS/daemon-$tag.pid"

  ready=0
  i=0
  while [ "$i" -lt 60 ]; do
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "daemon $tag 提前退出，见 daemon-$tag.log" >&2
      break
    fi
    if curl -fsS --max-time 2 "$HEALTH" -o "$RESULTS/health-$tag-probe.json" >/dev/null 2>&1; then
      ready=1
      break
    fi
    i=$((i + 1))
    sleep 0.5
  done
  echo "$ready" > "$RESULTS/daemon-$tag.ready"
  if [ "$ready" -ne 1 ]; then
    return 1
  fi

  n=1
  while [ "$n" -le 3 ]; do
    curl -fsS --max-time 3 "$HEALTH" -o "$RESULTS/health-$tag-$n.json" >/dev/null 2>&1 \
      || : > "$RESULTS/health-$tag-$n.json"
    n=$((n + 1))
    sleep 1
  done

  if [ "$tag" = "on" ]; then
    ih="$(grep -o 'infohash=[0-9a-f]\{40\}' "$RESULTS/daemon-on.log" | head -n 1 | cut -d= -f2)"
    md5="$("$PY" -c 'import hashlib,sys; print(hashlib.md5(open(sys.argv[1],"rb").read()).hexdigest())' "$ROOT/fixtures/sample.bin")"
    {
      echo "infohash=$ih"
      echo "expected_md5=$md5"
    } > "$RESULTS/guard-info.txt"
    if [ -n "$ih" ]; then
      echo
      echo "=== 4/6 本地握手唤醒功能复核（不证明线程优先级）==="
      code=0
      "$PY" "$ROOT/probes/guard_wake_probe.py" \
        --http "http://127.0.0.1:19600" \
        --bt-host 127.0.0.1 --bt-port 19602 \
        --infohash "$ih" --expected-md5 "$md5" \
        --piece-size 262144 --total 262144 \
        --out "$RESULTS/guard-wake.json" > "$RESULTS/guard-wake.stdout" 2>&1 || code=$?
      echo "$code" > "$RESULTS/guard-wake.exit"
      cat "$RESULTS/guard-wake.stdout"
    else
      echo "WARN: daemon-on.log 未找到 infohash，跳过握手唤醒功能复核" >&2
    fi
  fi

  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  i=0
  while [ "$i" -lt 20 ]; do
    if ! curl -fsS --max-time 1 "$HEALTH" >/dev/null 2>&1; then
      break
    fi
    i=$((i + 1))
    sleep 0.2
  done
  return 0
}

echo
echo "=== 3/6 default-on daemon（三次 /health 快照）==="
start_and_sample "$ROOT/config/daemon-on.json" on || echo "WARN: default-on 阶段未就绪"

echo
echo "=== 5/6 negative-control daemon（idle_priority=false）==="
start_and_sample "$ROOT/config/daemon-off.json" off || echo "WARN: negative-control 阶段未就绪"

echo
echo "=== 6/6 证据汇总 ==="
"$PY" "$ROOT/probes/collect_evidence.py" --results "$RESULTS" --root "$ROOT" | tee "$RESULTS/03-collect.stdout"
collect_code=${PIPESTATUS[0]}
echo "$collect_code" > "$RESULTS/03-collect.exit"

echo
echo "完成。把整个目录压缩回传：$RESULTS"
echo "注意：脚本不写 g2_claim，也不改 NFR-02 口径；wake 路径逐线程项标记为 not_verified。"
exit 0
