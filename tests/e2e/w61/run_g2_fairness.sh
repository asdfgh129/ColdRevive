#!/usr/bin/env bash
# G2 公平性量具（221 root）：真内核 veth + HTB + netem 瓶颈；两条真 UdpRelayStream 竞争。
# 只产生原始证据，不改 spikes.json / g2-spike-verdicts.toml / 任何门禁常量。
set -uo pipefail

REPO=${CR_REPO:-/opt/cr-current}
BIN=${G2F_BIN:-$REPO/target/release/coldrevive-fairness-probe}
OUT=${G2F_OUT:-$REPO/tests/e2e/w61/reports/g2-fairness-$(date -u +%Y%m%dT%H%M%SZ)}
ROUNDS=${G2F_ROUNDS:-10}
WARMUP=${G2F_WARMUP:-3}
SAMPLE=${G2F_SAMPLE:-10}
CHUNK=${G2F_CHUNK:-1024}
# 中继桶必须显式给大值：0 会被ByteBucket夹成 1 B/s，反而成为瓶颈。
# 这里让唯一瓶颈是 veth 上的 HTB。
DATA_RATE=${G2F_DATA_RATE:-1073741824}
DATA_BURST=${G2F_DATA_BURST:-1048576}
# 每会话桶的突发不能是 1 字节：首包就会被拒，发送方等不到 ACK 会永久阻塞。
PER_BURST=${G2F_PER_BURST:-65536}
# 可选只跑指定 arm（冒烟用）；正式判据必须跑全部四个 arm。
G2F_ARMS=${G2F_ARMS:-cc_off cc_on cc_paced cc_on_per_session_cap}

NS=g2fairsrv
VETH_A=g2fa
VETH_B=g2fb
IPA=10.207.0.1
IPB=10.207.0.2
PORT=7101

cleanup() {
  ip netns del "$NS" 2>/dev/null || true
  ip link del "$VETH_A" 2>/dev/null || true
}
trap cleanup EXIT

mkdir -p "$OUT"
if [[ ! -x "$BIN" ]]; then
  echo "缺少探针二进制 $BIN" >&2
  exit 2
fi

echo "=== 0) cleanup ==="
cleanup
sleep 0.3

echo "=== 1) veth + netns ==="
ip link add "$VETH_A" type veth peer name "$VETH_B" || exit 2
ip netns add "$NS" || exit 2
ip link set "$VETH_B" netns "$NS" || exit 2
ip addr add "$IPA/24" dev "$VETH_A" || exit 2
ip netns exec "$NS" ip addr add "$IPB/24" dev "$VETH_B" || exit 2
ip link set "$VETH_A" up || exit 2
ip netns exec "$NS" ip link set "$VETH_B" up || exit 2
ip netns exec "$NS" ip link set lo up || exit 2
sleep 0.5
ping -c 2 -W 1 "$IPB" >"$OUT/link-ping.log" 2>&1 || { echo "veth 不通" >&2; exit 2; }

setup_link() {
  local rate=$1 delay=$2 jitter=$3
  tc qdisc del dev "$VETH_A" root 2>/dev/null || true
  tc qdisc add dev "$VETH_A" root handle 1: htb default 10 || return 2
  tc class add dev "$VETH_A" parent 1: classid 1:10 htb rate "${rate}mbit" ceil "${rate}mbit" burst 64k || return 2
  tc qdisc add dev "$VETH_A" parent 1:10 handle 10: netem delay "$delay" "$jitter" limit 1000 || return 2
  tc qdisc show dev "$VETH_A" >"$OUT/tc-qdisc-L${rate}M-${delay}ms.txt"
  tc class show dev "$VETH_A" >"$OUT/tc-class-L${rate}M-${delay}ms.txt"
}

run_arm() {
  local shape=$1 arm=$2 cc=$3 pacing=$4 per_rate=$5
  case " $G2F_ARMS " in
    *" $arm "*) ;;
    *) return 0 ;;
  esac
  for ((r = 1; r <= ROUNDS; r++)); do
    local tag="${shape}-${arm}-r${r}"
    local relay_out="$OUT/relay-${tag}.json"
    local f1="$OUT/flow1-${tag}.json"
    local f2="$OUT/flow2-${tag}.json"

    tc -s qdisc show dev "$VETH_A" >"$OUT/tc-before-${tag}.txt"
    ip netns exec "$NS" "$BIN" relay \
      --bind "$IPB:$PORT" --seconds "$((WARMUP + SAMPLE + 8))" \
      --data-rate "$DATA_RATE" --data-burst "$DATA_BURST" \
      --per-session-rate "$per_rate" --per-session-burst "$PER_BURST" \
      --out "$relay_out" >"$OUT/relay-${tag}.log" 2>&1 &
    local relay_pid=$!
    for ((i = 0; i < 100; i++)); do
      grep -q RELAY_READY "$OUT/relay-${tag}.log" 2>/dev/null && break
      sleep 0.1
    done
    if ! grep -q RELAY_READY "$OUT/relay-${tag}.log" 2>/dev/null; then
      echo "ROUND_FAILED $tag relay_not_ready" >&2
      kill "$relay_pid" 2>/dev/null || true
      continue
    fi

    local common=(--server "$IPB:$PORT" --warmup "$WARMUP" --sample "$SAMPLE" --chunk "$CHUNK" --cc "$cc" --pacing "$pacing")
    local budget=$((WARMUP + SAMPLE + 20))
    timeout "$budget" "$BIN" flow "${common[@]}" --purpose flow1 \
      --recv-id 01000000000000000000000000000001 \
      --send-id 02000000000000000000000000000002 \
      --out "$f1" >"$OUT/flow1-${tag}.log" 2>&1 &
    local p1=$!
    timeout "$budget" "$BIN" flow "${common[@]}" --purpose flow2 \
      --recv-id 03000000000000000000000000000003 \
      --send-id 04000000000000000000000000000004 \
      --out "$f2" >"$OUT/flow2-${tag}.log" 2>&1 &
    local p2=$!
    wait "$p1" || echo "ROUND_FAILED $tag flow1" >&2
    wait "$p2" || echo "ROUND_FAILED $tag flow2" >&2
    wait "$relay_pid" || echo "ROUND_FAILED $tag relay" >&2
    tc -s qdisc show dev "$VETH_A" >"$OUT/tc-after-${tag}.txt"
    echo "ROUND_DONE $tag"
  done
}

echo "=== 2) shapes and arms ==="
while read -r shape rate delay jitter; do
  [[ -z "$shape" ]] && continue
  setup_link "$rate" "$delay" "$jitter" || exit 2
  echo "SHAPE $shape rate=${rate}mbit delay=${delay}ms jitter=${jitter}ms"
  # cap 设为链路速率的 1/8，低于 CC 流的自然速率，确保令牌桶真的触发；
  # 它是解释隔离组，不替代端侧拥塞控制，也不参与主判据的“机制达标”宣称。
  cap=$((rate * 1000000 / 8 / 8))
  run_arm "$shape" cc_off false false 0
  run_arm "$shape" cc_on true false 0
  run_arm "$shape" cc_paced true true 0
  run_arm "$shape" cc_on_per_session_cap true false "$cap"
done <<'SHAPES'
L1 16 20ms 2ms
L2 8 50ms 5ms
SHAPES

echo "=== 3) done ==="
echo "OUT=$OUT"
echo "G2_GATE_SNAPSHOT g2_claim=false s6_status=not_run evidence_valid_for_pass=false auto_pass=false"
