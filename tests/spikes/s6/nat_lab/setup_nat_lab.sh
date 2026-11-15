#!/usr/bin/env bash
# =============================================================================
# setup_nat_lab.sh — S6 四类 NAT + CGNAT 实验室（单台 Linux 主机内真改写）
# -----------------------------------------------------------------------------
# 用途：在一台 Linux 主机内用 network namespace + iptables conntrack 真改写出
#       Full / Restricted / Port-restricted / Symmetric 四类 NAT，外加一层
#       CGNAT（双 NAT）。网关 netns 做的是**内核级 NAT 改写**（不是应用层代理），
#       因此从 LAN netns 内跑 nat_behavior.py 得到的分类报告是真实 NAT 行为证据。
#
# 关键设计（第一版曾漏掉，务必理解）：
#   * NAT 的可达性由两件事共同决定：**映射能否被解析回内网** + **过滤是否放行入向 NEW**。
#     只做 SNAT 不做 DNAT 时，任何 unsolicited 入向包都没有 conntrack 状态可查，
#     于是 full-cone 与 port-restricted 会**测出来完全一样**（都被丢弃）——
#     这是假阴性，不是 NAT 行为。所以三类的 cone 网关都补了
#     `-t nat PREROUTING -i <wan> -p udp -j DNAT --to-destination <lan_ip>`
#     （端口保持的 1:1 映射，等价于「cone NAT 在首次出向后建立的可解析映射」），
#     再由 FORWARD 规则决定哪些入向 NEW 放行：
#       full-cone          : 任意源放行
#       restricted-cone    : 仅放行「该内网 ip:port 曾发往过的源 IP」（任意端口）
#       port-restricted    : 只放行 ESTABLISHED,RELATED（不放行任何入向 NEW）
#       symmetric          : 反向映射随目的地址变化（SNAT --random-fully），且无 DNAT
#   * restricted-cone 用 ipset `hash:ip,port,ip` 记录 (内网ip, 内网port, 远端ip)，
#     这是 RFC 4787 REQ-4 的「按内网传输地址 + 远端 IP」语义，不是按整网段。
#   * ipset 只与 iptables-legacy 兼容；Ubuntu 默认 iptables 是 nf_tables 后端，
#     因此本脚本默认自动选择 iptables-legacy（可用 IPT=... 覆盖）。
#
# 边界（务必读完）：
#   * 本脚本只搭建网络环境 + 可选地起本地 STUN，不跑 ColdRevive 唤醒，不写 spikes.json。
#   * S6 的最终 pass/fail 由实验执行人依据 nat_matrix.py 产出的逐类报告手工回填；
#     脚本永不自动置 pass。
#   * 判据是**两个独立条款**：① 两端在不同物理主机；② NAT 是真实设备（real_nat_evidence）。
#     本实验室两条都不满足：端点同机不同 netns（条款①），NAT 由 iptables 合成（条款②）。
#     换主机只能满足①，永远满足不了② —— 详见 nat-matrix.md §13。
#
# ⚠ 为什么必须在实验室内起本地 STUN（而不是查公共 STUN）：
#   从 LAN netns 访问公网时，报文还会穿过本机自身出口 NAT。外层 NAT 会覆盖内层
#   的映射/过滤特征（外层若是 symmetric，内层 full-cone 也会被测成 symmetric）。
#   把 STUN 放在宿主 WAN 网桥上，路径里就只有被测的那一层 NAT。
#
# 用法：
#   sudo bash setup_nat_lab.sh up              # 建 4 类 NAT + CGNAT
#   sudo bash setup_nat_lab.sh status          # 检查 netns / 规则 / ipset
#   sudo bash setup_nat_lab.sh stun-start      # 在 10.0.0.1:3478 + 备用地址起本地 STUN
#   sudo bash setup_nat_lab.sh stun-stop
#   sudo bash setup_nat_lab.sh down            # 拆除全部
# =============================================================================
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LAB_EVIDENCE="$HERE/lab-evidence.json"
ATTEST_DIR="$HERE/attest"
STUN_LOG="/tmp/coldrevive-local-stun.log"
STUN_PID="/tmp/coldrevive-local-stun.pid"
STUN_STATS="$HERE/local-stun-stats.json"

# ---- 地址规划 ----------------------------------------------------------------
WAN_BR="br-wan"
WAN_GW_IP="10.0.0.1"        # 宿主在 WAN 网桥上的地址（本地 STUN 主地址）
WAN_ALT_IP="10.0.0.2"       # 宿主第二个地址（本地 STUN 备用 IP）
WAN_NET="10.0.0.0/24"
STUN_PORT=3478
STUN_ALT_PORT=3479

# 每类 NAT：id|lan_net|gw_lan_ip|gw_wan_ip
TYPES=(
  "full-cone|192.168.10.0/24|192.168.10.1|10.0.0.10"
  "restricted-cone|192.168.20.0/24|192.168.20.1|10.0.0.11"
  "port-restricted-cone|192.168.30.0/24|192.168.30.1|10.0.0.12"
  "symmetric|192.168.40.0/24|192.168.40.1|10.0.0.13"
)
# CGNAT 双 NAT：192.168.50.0/24 -> 内层 SNAT 到 100.64.0.1 -> 外层 SNAT 到 10.0.0.14
CGNAT_LAN="192.168.50.0/24"
CGNAT_LAN_IP="192.168.50.1"
CGNAT_HOST_IP="192.168.50.2"
CGNAT_INNER_NET="100.64.0.0/24"
CGNAT_INNER_IP="100.64.0.1"
CGNAT_INNER_HOST="100.64.0.2"
CGNAT_OUTER_WAN_IP="10.0.0.14"

# iptables 后端：ipset 只与 legacy 兼容
if [[ -z "${IPT:-}" ]]; then
  if command -v iptables-legacy >/dev/null 2>&1; then IPT=iptables-legacy; else IPT=iptables; fi
fi

require_root() {
  if [[ "$(id -u)" -ne 0 ]]; then
    echo "ERROR: 必须以 root 运行（sudo bash $0 $*）" >&2
    exit 1
  fi
}

need_cmd() {
  for c in "$@"; do
    command -v "$c" >/dev/null 2>&1 || {
      echo "ERROR: 缺少命令: $c" >&2; exit 1; }
  done
}

mkns() { ip netns add "$1" 2>/dev/null || true; }
nsx() { local ns="$1"; shift; ip netns exec "$ns" "$@"; }

# 在 netns 内执行 iptables
ipt() { local ns="$1"; shift; ip netns exec "$ns" "$IPT" "$@"; }

setup_wan() {
  ip link add "$WAN_BR" type bridge 2>/dev/null || true
  ip link set "$WAN_BR" up
  ip addr add "$WAN_GW_IP/24" dev "$WAN_BR" 2>/dev/null || true
  # 本地 STUN 的备用 IP：同网桥第二个地址，网关可直接 ARP 到
  ip addr add "$WAN_ALT_IP/24" dev "$WAN_BR" 2>/dev/null || true
  sysctl -w net.ipv4.ip_forward=1 >/dev/null
  sysctl -w net.bridge.bridge-nf-call-iptables=0 >/dev/null 2>&1 || true
}

# 网关内通用准备（转发开关、关 rp_filter、清表）
gw_prepare() {
  local ns="$1"
  nsx "$ns" sysctl -w net.ipv4.ip_forward=1 >/dev/null
  nsx "$ns" sysctl -w net.ipv4.conf.all.rp_filter=0 >/dev/null 2>&1 || true
  nsx "$ns" sysctl -w net.ipv4.conf.default.rp_filter=0 >/dev/null 2>&1 || true
  ipt "$ns" -t nat -F 2>/dev/null || true
  ipt "$ns" -F FORWARD 2>/dev/null || true
  ipt "$ns" -P FORWARD DROP
}

setup_type() {
  local id="$1" lan_net="$2" gw_lan_ip="$3" gw_wan_ip="$4" tag="$5"
  local gw_ns="gw-$id" lan_ns="lan-$id"
  # 接口名上限 15 字符，故用序号而非类名：lx-1 / lx-1_p / gw1 / hw1
  local lx="lx-$tag"
  local gwan="gw$tag" hwan="hw$tag"

  mkns "$gw_ns"; mkns "$lan_ns"
  ip link del "$lx" 2>/dev/null || true
  ip link add "$lx" type veth peer name "${lx}_p"
  ip link set "$lx" netns "$lan_ns"
  ip link set "${lx}_p" netns "$gw_ns"

  ip link del "$gwan" 2>/dev/null || true
  ip link add "$gwan" type veth peer name "$hwan"
  ip link set "$gwan" netns "$gw_ns"
  ip link set "$hwan" master "$WAN_BR"
  ip link set "$hwan" up

  local lan_ip="${gw_lan_ip%.*}.2"
  local gw_peer="${lx}_p"

  nsx "$lan_ns" sh -c "ip link set lo up; ip link set $lx up"
  nsx "$lan_ns" ip addr add "$lan_ip/24" dev "$lx"
  nsx "$lan_ns" ip route add default via "$gw_lan_ip"

  nsx "$gw_ns" sh -c "ip link set lo up; ip link set $gw_peer up; ip link set $gwan up"
  nsx "$gw_ns" ip addr add "$gw_lan_ip/24" dev "$gw_peer"
  nsx "$gw_ns" ip addr add "$gw_wan_ip/24" dev "$gwan"

  gw_prepare "$gw_ns"

  # 通用：已建立/相关放行 + 内网出向 NEW 放行
  ipt "$gw_ns" -A FORWARD -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT

  case "$id" in
    restricted-cone)
      need_cmd ipset
      # 必须在「出向 ACCEPT」之前记录，否则 ACCEPT 已终止链，集合永远为空
      nsx "$gw_ns" ipset create CONTACTED hash:ip,port,ip timeout 600 -exist
      ipt "$gw_ns" -A FORWARD -s "$lan_net" -p udp -j SET --add-set CONTACTED src,src,dst
      ;;
  esac

  ipt "$gw_ns" -A FORWARD -s "$lan_net" -m conntrack --ctstate NEW -j ACCEPT

  case "$id" in
    full-cone)
      ipt "$gw_ns" -t nat -A POSTROUTING -s "$lan_net" -o "$gwan" -j SNAT --to-source "$gw_wan_ip"
      ipt "$gw_ns" -t nat -A PREROUTING -i "$gwan" -p udp -j DNAT --to-destination "$lan_ip"
      ipt "$gw_ns" -A FORWARD -d "$lan_ip" -p udp -m conntrack --ctstate NEW -j ACCEPT
      ;;
    restricted-cone)
      ipt "$gw_ns" -t nat -A POSTROUTING -s "$lan_net" -o "$gwan" -j SNAT --to-source "$gw_wan_ip"
      ipt "$gw_ns" -t nat -A PREROUTING -i "$gwan" -p udp -j DNAT --to-destination "$lan_ip"
      # 入向 NEW 只放行「该内网 ip:port 曾发往过的远端 IP」（端口不限）-> restricted cone
      ipt "$gw_ns" -A FORWARD -d "$lan_ip" -p udp -m set --match-set CONTACTED dst,dst,src \
        -m conntrack --ctstate NEW -j ACCEPT
      ;;
    port-restricted-cone)
      ipt "$gw_ns" -t nat -A POSTROUTING -s "$lan_net" -o "$gwan" -j SNAT --to-source "$gw_wan_ip"
      ipt "$gw_ns" -t nat -A PREROUTING -i "$gwan" -p udp -j DNAT --to-destination "$lan_ip"
      # 不放行任何入向 NEW：只有 ESTABLISHED,RELATED（上面已加）
      ;;
    symmetric)
      # 反向映射随目的地址变化：每次新流随机源端口
      ipt "$gw_ns" -t nat -A POSTROUTING -s "$lan_net" -o "$gwan" \
        -j SNAT --to-source "$gw_wan_ip" --random-fully
      # 无 DNAT：symmetric NAT 本就不接受 unsolicited 入向
      ;;
  esac

  echo "  [$id] lan_ns=$lan_ns($lan_ip) gw=$gw_ns wan=$gw_wan_ip"
}

setup_cgnat() {
  local lan_ns="lan-cgnat" inner="gw-cgnat" outer="gw-cgnat-outer"
  local lx="lx-cgnat" gin="gin-cgnat" hin="hin-cgnat" gwan="gwan-cgnat" hwan="hwan-cgnat"

  mkns "$lan_ns"; mkns "$inner"; mkns "$outer"

  ip link del "$lx" 2>/dev/null || true
  ip link add "$lx" type veth peer name "${lx}_p"
  ip link set "$lx" netns "$lan_ns"
  ip link set "${lx}_p" netns "$inner"

  ip link del "$gin" 2>/dev/null || true
  ip link add "$gin" type veth peer name "$hin"
  ip link set "$gin" netns "$inner"
  ip link set "$hin" netns "$outer"

  ip link del "$gwan" 2>/dev/null || true
  ip link add "$gwan" type veth peer name "$hwan"
  ip link set "$gwan" netns "$outer"
  ip link set "$hwan" master "$WAN_BR"
  ip link set "$hwan" up

  nsx "$lan_ns" sh -c "ip link set lo up; ip link set $lx up"
  nsx "$lan_ns" ip addr add "$CGNAT_HOST_IP/24" dev "$lx"
  nsx "$lan_ns" ip route add default via "$CGNAT_LAN_IP"

  nsx "$inner" sh -c "ip link set lo up; ip link set ${lx}_p up; ip link set $gin up"
  nsx "$inner" ip addr add "$CGNAT_LAN_IP/24" dev "${lx}_p"
  nsx "$inner" ip addr add "$CGNAT_INNER_IP/24" dev "$gin"
  # 内层网关的默认路由指向运营商外层设备，否则到公网报 Destination Net Unreachable
  nsx "$inner" ip route add default via "$CGNAT_INNER_HOST"

  nsx "$outer" sh -c "ip link set lo up; ip link set $hin up; ip link set $gwan up"
  nsx "$outer" ip addr add "$CGNAT_INNER_HOST/24" dev "$hin"
  nsx "$outer" ip addr add "$CGNAT_OUTER_WAN_IP/24" dev "$gwan"

  # 内层 NAT（运营商侧第一跳）
  gw_prepare "$inner"
  ipt "$inner" -A FORWARD -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT
  ipt "$inner" -A FORWARD -s "$CGNAT_LAN" -m conntrack --ctstate NEW -j ACCEPT
  ipt "$inner" -t nat -A POSTROUTING -s "$CGNAT_LAN" -o "$gin" \
    -j SNAT --to-source "$CGNAT_INNER_IP"
  ipt "$inner" -t nat -A PREROUTING -i "$gin" -p udp -j DNAT --to-destination "$CGNAT_HOST_IP"

  # 外层 NAT（运营商 CGNAT 设备）
  gw_prepare "$outer"
  ipt "$outer" -A FORWARD -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT
  ipt "$outer" -A FORWARD -s "$CGNAT_INNER_NET" -m conntrack --ctstate NEW -j ACCEPT
  ipt "$outer" -t nat -A POSTROUTING -s "$CGNAT_INNER_NET" -o "$gwan" \
    -j SNAT --to-source "$CGNAT_OUTER_WAN_IP"
  ipt "$outer" -t nat -A PREROUTING -i "$gwan" -p udp -j DNAT --to-destination "$CGNAT_INNER_IP"

  echo "  [cgnat] lan_ns=$lan_ns($CGNAT_HOST_IP) -> inner=$CGNAT_INNER_IP -> outer wan=$CGNAT_OUTER_WAN_IP"
}

emit_evidence() {
  mkdir -p "$ATTEST_DIR"
  {
    echo "{"
    echo "  \"schema\": \"coldrevive.s6.nat-lab/v1\","
    echo "  \"recorded_at\": \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\","
    echo "  \"host\": \"$(hostname)\","
    echo "  \"iptables_backend\": \"$IPT\","
    echo "  \"stun_endpoints\": {"
    echo "    \"primary\": \"$WAN_GW_IP:$STUN_PORT\","
    echo "    \"alt_ip\": \"$WAN_ALT_IP:$STUN_PORT\","
    echo "    \"alt_port\": \"$WAN_GW_IP:$STUN_ALT_PORT\","
    echo "    \"alt_ip_alt_port\": \"$WAN_ALT_IP:$STUN_ALT_PORT\""
    echo "  },"
    echo "  \"note\": \"VM 内 iptables conntrack 真改写 NAT；非独立路由器硬件。分类以 nat_behavior.py 实跑为准。\","
    echo "  \"emulation_model\": {"
    echo "    \"cone_dnat\": \"cone 类网关对入向 UDP 做端口保持的 DNAT（映射可解析），过滤由 FORWARD 规则决定\","
    echo "    \"restricted_tracking\": \"ipset hash:ip,port,ip 记录 (内网ip,内网port,远端ip)\","
    echo "    \"symmetric\": \"SNAT --random-fully + 无 DNAT\","
    echo "    \"cgnat\": \"双 NAT：内层 SNAT 到 100.64.0.0/24 + 外层 SNAT 到 $CGNAT_OUTER_WAN_IP，两层均仅 ESTABLISHED\""
    echo "  },"
    echo "  \"classes\": ["
    for t in "${TYPES[@]}"; do
      IFS='|' read -r id lan_net gw_lan_ip gw_wan_ip <<< "$t"
      echo "    {\"id\": \"$id\", \"lan_namespace\": \"lan-$id\", \"lan_ip\": \"${gw_lan_ip%.*}.2\","
      echo "     \"gateway_namespace\": \"gw-$id\", \"gateway_wan_ip\": \"$gw_wan_ip\","
      echo "     \"classification_report\": \"在 lan-$id 内跑 nat_behavior.py 后填写绝对路径\"},"
    done
    echo "    {\"id\": \"cgnat\", \"lan_namespace\": \"lan-cgnat\", \"lan_ip\": \"$CGNAT_HOST_IP\","
    echo "     \"gateway_namespace\": \"gw-cgnat + gw-cgnat-outer\", \"gateway_wan_ip\": \"$CGNAT_OUTER_WAN_IP\","
    echo "     \"classification_report\": \"在 lan-cgnat 内跑 nat_behavior.py 后填写绝对路径\"}"
    echo "  ]"
    echo "}"
  } > "$LAB_EVIDENCE"
  echo "  证据清单: $LAB_EVIDENCE"

  for t in "${TYPES[@]}"; do
    IFS='|' read -r id lan_net gw_lan_ip gw_wan_ip <<< "$t"
    cat > "$ATTEST_DIR/attest-$id.template.json" <<EOF
{
  "class": "$id",
  "gateway_wan_ip": "$gw_wan_ip",
  "local_ip": "${gw_lan_ip%.*}.2",
  "peer_ip": "<对端 LAN 端点 IP，例如 192.168.30.2>",
  "machine_ids": ["lan-$id", "<对端 netns 标识>"],
  "real_nat_evidence": "$LAB_EVIDENCE",
  "classification_report": "<在 lan-$id 内跑 nat_behavior.py 产出的报告绝对路径>",
  "operator": "<执行人>",
  "recorded_at": "<ISO8601>"
}
EOF
  done
  cat > "$ATTEST_DIR/attest-cgnat.template.json" <<EOF
{
  "class": "cgnat",
  "gateway_wan_ip": "$CGNAT_OUTER_WAN_IP",
  "local_ip": "$CGNAT_HOST_IP",
  "peer_ip": "<对端 LAN 端点 IP>",
  "machine_ids": ["lan-cgnat", "<对端 netns 标识>"],
  "real_nat_evidence": "$LAB_EVIDENCE",
  "classification_report": "<在 lan-cgnat 内跑 nat_behavior.py 产出的报告绝对路径>",
  "operator": "<执行人>",
  "recorded_at": "<ISO8601>"
}
EOF
  echo "  attestation 模板: $ATTEST_DIR/attest-*.template.json"
}

stun_start() {
  require_root
  need_cmd python3
  if [[ -f "$STUN_PID" ]] && kill -0 "$(cat "$STUN_PID")" 2>/dev/null; then
    echo "本地 STUN 已在运行 (pid $(cat "$STUN_PID"))"
    return 0
  fi
  setsid python3 "$HERE/local_stun.py" \
    --primary "$WAN_GW_IP:$STUN_PORT" --alt-ip "$WAN_ALT_IP" --alt-port "$STUN_ALT_PORT" \
    --stats-file "$STUN_STATS" >"$STUN_LOG" 2>&1 &
  echo $! > "$STUN_PID"
  sleep 1
  if grep -q LOCAL_STUN_READY "$STUN_LOG" 2>/dev/null; then
    echo "本地 STUN 就绪: $(grep LOCAL_STUN_READY "$STUN_LOG")"
  else
    echo "ERROR: 本地 STUN 启动失败，见 $STUN_LOG" >&2
    tail -5 "$STUN_LOG" >&2 || true
    exit 1
  fi
}

stun_stop() {
  if [[ -f "$STUN_PID" ]]; then
    kill "$(cat "$STUN_PID")" 2>/dev/null || true
    rm -f "$STUN_PID"
    echo "本地 STUN 已停止"
  else
    echo "本地 STUN 未在运行"
  fi
}

status_all() {
  echo "== iptables 后端: $IPT =="
  echo "== netns =="
  ip netns list | sed 's/^/  /'
  echo "== 桥 $WAN_BR =="
  ip -o addr show "$WAN_BR" 2>/dev/null | awk '{print "  "$2, $4}' || echo "  (未创建)"
  for t in "${TYPES[@]}"; do
    IFS='|' read -r id _ _ _ <<< "$t"
    echo "== $id =="
    nsx "lan-$id" ip -o -4 addr show 2>/dev/null | awk '{print "  lan:", $2, $4}' || echo "  lan-$id 缺失"
    ipt "gw-$id" -t nat -S POSTROUTING 2>/dev/null | grep SNAT | sed 's/^/  /' || true
    ipt "gw-$id" -S FORWARD 2>/dev/null | grep -v '^-P' | sed 's/^/  /' || true
    if [[ "$id" == "restricted-cone" ]]; then
      nsx "gw-$id" ipset list CONTACTED 2>/dev/null | sed -n '/Members/,$p' | head -5 | sed 's/^/  set: /' || true
    fi
  done
  echo "== cgnat =="
  nsx lan-cgnat ip -o -4 addr show 2>/dev/null | awk '{print "  lan:", $2, $4}' || echo "  lan-cgnat 缺失"
  ipt gw-cgnat -t nat -S POSTROUTING 2>/dev/null | grep SNAT | sed 's/^/  inner: /' || true
  ipt gw-cgnat-outer -t nat -S POSTROUTING 2>/dev/null | grep SNAT | sed 's/^/  outer: /' || true
  echo "== 本地 STUN =="
  if [[ -f "$STUN_PID" ]] && kill -0 "$(cat "$STUN_PID")" 2>/dev/null; then
    echo "  运行中 pid=$(cat "$STUN_PID")  stats=$STUN_STATS"
  else
    echo "  未运行（bash $0 stun-start）"
  fi
}

teardown_all() {
  stun_stop >/dev/null 2>&1 || true
  for t in "${TYPES[@]}"; do
    IFS='|' read -r id _ _ _ <<< "$t"
    ip netns del "gw-$id" 2>/dev/null || true
    ip netns del "lan-$id" 2>/dev/null || true
  done
  ip netns del gw-cgnat 2>/dev/null || true
  ip netns del gw-cgnat-outer 2>/dev/null || true
  ip netns del lan-cgnat 2>/dev/null || true
  ip link set "$WAN_BR" down 2>/dev/null || true
  ip link del "$WAN_BR" 2>/dev/null || true
  echo "已拆除全部 NAT 实验室 netns 与网桥。"
}

case "${1:-up}" in
  up)
    require_root
    need_cmd ip "$IPT"
    setup_wan
    n=0
    for t in "${TYPES[@]}"; do
      n=$((n + 1))
      IFS='|' read -r id lan_net gw_lan_ip gw_wan_ip <<< "$t"
      setup_type "$id" "$lan_net" "$gw_lan_ip" "$gw_wan_ip" "$n"
    done
    setup_cgnat
    emit_evidence
    echo ""
    echo "环境就绪。下一步："
    echo "  1) bash $0 stun-start"
    echo "  2) 每类跑分类（在对应 LAN netns 内）："
    echo "     ip netns exec lan-full-cone python3 $HERE/../nat_behavior.py \\"
    echo "       --mapping-servers $WAN_GW_IP:$STUN_PORT,$WAN_ALT_IP:$STUN_PORT \\"
    echo "       --filter-servers  $WAN_GW_IP:$STUN_PORT --label full-cone-lab \\"
    echo "       --out /opt/coldrevive/reports/s6-nb-full-cone.json"
    echo "  3) 两个 netns 各起 coldrevived，用 nat_matrix.py --mode real 跑 >=30 次真实唤醒"
    echo "  4) nat_matrix.py --aggregate <reports-dir>"
    ;;
  status) status_all ;;
  stun-start) stun_start ;;
  stun-stop) stun_stop ;;
  down) require_root; teardown_all ;;
  *) echo "用法: $0 {up|status|stun-start|stun-stop|down}"; exit 2 ;;
esac
