#!/usr/bin/env bash
# =============================================================================
# link_physical.sh — 把某一类 NAT 网关的 WAN 侧"搬"到物理 LAN 上
# -----------------------------------------------------------------------------
# 目的：让 S6 矩阵的**一端跑在另一台物理主机上**（跨主机档位 cross-host-lab），
#       而不是两端都待在实验机内的两个 netns 里。
#
# 为什么需要这个脚本（而不是直接在 A 上加路由）：
#   实验机的 WAN 侧原本是内部网桥 br-wan（10.0.0.0/24），另一台主机（A，Windows）
#   根本不知道 10.0.0.0/24 怎么走；要让它可达，通常得在 A 上加一条静态路由 ——
#   那需要管理员权限，且会改动对方主机的网络配置。
#   这里换成**相反的方向**：给网关的 WAN 口再挂一个**物理 LAN 上的地址**，
#   由实验机在物理网卡上做 proxy ARP 替它应答。
#   于是 A 看到的对端就在同一个二层网络里，A 端**零改动、零权限**。
#
# ⚠ 地址不能写死（2026-11-15 实测教训）：
#   原先按类固定用 .241~.245，但用户家庭 LAN 上 .242/.244/.245 已被**真实设备**占用，
#   于是 punch 全发给了别人的机器，报告里却只写「唤醒无应答」。现在地址由
#   `run_cross_host.py` 挑选空闲地址后经 `LINK_PHYS_ADDR` 传入，并在 up 之后
#   校验「该地址的 ARP 应答者 MAC == 本机 ens160 的 MAC」才算就绪。
#
# 数据路径（每个方向都只多一跳，不改写 IP）：
#   A(192.168.1.210) --ARP--> 实验机 ens160 --路由 192.168.1.24X/32--> br-wan
#       --> 网关 netns 的 WAN 口 (192.168.1.24X/32) --SNAT--> 内网 holder
#   反向：网关把包发给 192.168.1.24X 的邻居（静态写死为 br-wan 的 MAC），
#        br-wan 上的实验机按 /32 路由把它转出 ens160。
#
# 边界：
#   * 只改本实验机的 iptables / 路由 / sysctl，**不碰任何其他主机**。
#   * 每类环境同时只暴露一个物理地址；`down` 会把插入的 SNAT 规则删掉。
#   * 本脚本不跑唤醒、不写 spikes.json、不改任何 spike 状态。
#
# 用法：
#   sudo bash link_physical.sh up <class>        # class: full-cone|restricted-cone|
#                                                #        port-restricted-cone|symmetric|cgnat
#   sudo bash link_physical.sh status
#   sudo bash link_physical.sh down
#   sudo bash link_physical.sh stun-lan-start    # 在物理网卡上起第二个 STUN（给 A 用）
#   sudo bash link_physical.sh stun-lan-stop
# =============================================================================
set -euo pipefail

IPT="${IPT:-iptables-legacy}"
PHYS_IF="${PHYS_IF:-ens160}"
WAN_BR="${WAN_BR:-br-wan}"
LAN_CIDR="${LAN_CIDR:-192.168.1.0/24}"
STUN_PID_FILE="/tmp/coldrevive-stun-lan.pid"
STUN_LOG="/tmp/coldrevive-stun-lan.log"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# 每类环境的网关 netns 与其"物理身份"地址（.241 起，避开常见 DHCP 段）
gw_ns() {
  case "$1" in
    full-cone)            echo "gw-full-cone" ;;
    restricted-cone)      echo "gw-restricted-cone" ;;
    port-restricted-cone) echo "gw-port-restricted-cone" ;;
    symmetric)            echo "gw-symmetric" ;;
    cgnat)                echo "gw-cgnat-outer" ;;
    *) echo "" ;;
  esac
}
gw_phys() {
  # 显式覆盖优先：2026-11-15 实测发现固定地址池会**撞上真实设备**
  # （用户家里的 .242/.244/.245 已被其它主机占用），于是由驱动挑选空闲地址并经
  # LINK_PHYS_ADDR 传入；缺省仍用下面这张表，但**必须**配合身份校验（见 run_cross_host.py）。
  if [[ -n "${LINK_PHYS_ADDR:-}" ]]; then echo "$LINK_PHYS_ADDR"; return 0; fi
  case "$1" in
    full-cone)            echo "192.168.1.241" ;;
    restricted-cone)      echo "192.168.1.242" ;;
    port-restricted-cone) echo "192.168.1.243" ;;
    symmetric)            echo "192.168.1.244" ;;
    cgnat)                echo "192.168.1.245" ;;
    *) echo "" ;;
  esac
}

# 网关 WAN 口名字按类不同（hw1..hw4 / hwan-cgnat），靠"哪个口带 10.0.0.x 地址"识别，
# 避免把接口命名规则硬编码进两个脚本各写一份。
wan_if() {
  local ns="$1"
  ip netns exec "$ns" ip -o -4 addr show 2>/dev/null \
    | awk '$4 ~ /^10\.0\.0\./ {print $2; exit}'
}

# 额外需要"钉死邻居"的地址：实验机自身的 STUN 地址 + 对端主机。
# 这些地址在 br-wan 上都不是"同网段邻居"，靠 ARP 不可靠，直接写死到 br-wan 的 MAC。
NEIGH_EXTRA="${NEIGH_EXTRA:-192.168.1.221 192.168.1.239 192.168.1.210}"

cmd_up() {
  local cls="${1:-}"
  local ns phys wan mac src extra spec
  ns="$(gw_ns "$cls")"
  [[ -n "$ns" ]] || { echo "未知类别: $cls" >&2; exit 2; }
  phys="$(gw_phys "$cls")"
  # 注意：`ip netns list` 每行带 " (id: N)" 后缀，不能直接整行比对
  ip netns list | awk '{print $1}' | grep -qx "$ns" \
    || { echo "netns $ns 不存在，先 bash setup_nat_lab.sh up" >&2; exit 2; }
  wan="$(wan_if "$ns")"
  [[ -n "$wan" ]] || { echo "在 $ns 里找不到 WAN 口（期待 10.0.0.x 地址）" >&2; exit 2; }

  # 1) 宿主侧：把物理地址指向 br-wan，并允许 proxy ARP 替网关应答
  ip route replace "$phys/32" dev "$WAN_BR"
  sysctl -qw "net.ipv4.conf.$PHYS_IF.proxy_arp=1"
  sysctl -qw "net.ipv4.conf.$WAN_BR.proxy_arp=1"
  sysctl -qw net.ipv4.conf.all.proxy_arp=1

  # 2) 网关侧：挂物理地址 + 到物理 LAN 的路由
  ip netns exec "$ns" ip addr replace "$phys/32" dev "$wan"
  ip netns exec "$ns" ip route replace "$LAN_CIDR" dev "$wan"

  # 3) 网关侧：把实验机 + 对端主机钉成静态邻居（ARP 在 br-wan 上解析不到它们）
  mac="$(cat "/sys/class/net/$WAN_BR/address")"
  local a
  for a in $NEIGH_EXTRA "$phys"; do
    ip netns exec "$ns" ip neigh replace "$a" lladdr "$mac" dev "$wan" nud permanent
  done

  # 4) SNAT 改写到物理地址。用 -I 1 而不是改原规则：原规则保持原样，down 时精确删除。
  #    对称型必须带上 --random-fully，否则会把对称 NAT 悄悄变成锥形。
  spec="$(ip netns exec "$ns" "$IPT" -t nat -S POSTROUTING | grep -- "-o $wan" | grep -- "--to-source" | head -1)"
  [[ -n "$spec" ]] || { echo "在 $ns 里找不到 POSTROUTING SNAT 规则（-o $wan）" >&2; exit 2; }
  src="$(sed -n 's/.*-s \([^ ]*\).*/\1/p' <<<"$spec")"
  extra=""
  grep -q -- "--random-fully" <<<"$spec" && extra="--random-fully"
  if ! ip netns exec "$ns" "$IPT" -t nat -S POSTROUTING | grep -q -- "--to-source $phys"; then
    # shellcheck disable=SC2086
    ip netns exec "$ns" "$IPT" -t nat -I POSTROUTING 1 -s "$src" -o "$wan" \
      -j SNAT --to-source "$phys" $extra
  fi

  printf '%s %s %s %s %s\n' "$cls" "$ns" "$wan" "$phys" "$src" > /tmp/coldrevive-link-physical.state
  echo "已暴露 $cls → $phys（netns=$ns wan=$wan src=$src ${extra:-}）"
  # 机器可读的身份行：对端主机据此校验「这个地址确实是实验机在应答」，
  # 而不是碰巧有别的设备占着同一地址（2026-11-15 真实踩过这个坑）。
  echo "PHYS=$phys MAC=$(cat "/sys/class/net/$PHYS_IF/address")"
}

cmd_down() {
  local cls ns wan phys
  [[ -f /tmp/coldrevive-link-physical.state ]] || { echo "无活动链路"; return 0; }
  # 空状态文件（上一次 up 中途失败留下）会让 read 返回非零，set -e 会直接退出。
  read -r cls ns wan phys _ < /tmp/coldrevive-link-physical.state || { echo "状态文件为空，按已拆除处理"; rm -f /tmp/coldrevive-link-physical.state; return 0; }
  # 精确删除插入的 SNAT（按 --to-source 定位，避免误删原规则）
  while ip netns exec "$ns" "$IPT" -t nat -S POSTROUTING 2>/dev/null | grep -q -- "--to-source $phys"; do
    local spec extra=""
    spec="$(ip netns exec "$ns" "$IPT" -t nat -S POSTROUTING | grep -- "--to-source $phys" | head -1)"
    grep -q -- "--random-fully" <<<"$spec" && extra="--random-fully"
    # shellcheck disable=SC2046
    ip netns exec "$ns" "$IPT" -t nat -D POSTROUTING $(sed -n 's/^-A POSTROUTING //p' <<<"$spec") 2>/dev/null || break
  done
  ip netns exec "$ns" ip route del "$LAN_CIDR" dev "$wan" 2>/dev/null || true
  ip netns exec "$ns" ip addr del "$phys/32" dev "$wan" 2>/dev/null || true
  ip route del "$phys/32" dev "$WAN_BR" 2>/dev/null || true
  rm -f /tmp/coldrevive-link-physical.state
  echo "已撤销 $cls 的物理暴露"
}

cmd_status() {
  echo "== 宿主 =="
  ip -4 -br addr show "$PHYS_IF" | sed 's/^/  /'
  echo "  proxy_arp($PHYS_IF)=$(sysctl -n net.ipv4.conf.$PHYS_IF.proxy_arp)"
  echo "  proxy_arp($WAN_BR)=$(sysctl -n net.ipv4.conf.$WAN_BR.proxy_arp)"
  for ip in 192.168.1.241 192.168.1.242 192.168.1.243 192.168.1.244 192.168.1.245; do
    if ip route get "$ip" 2>/dev/null | grep -q "dev $WAN_BR"; then
      echo "  路由 $ip → $WAN_BR"
    fi
  done
  echo "== 状态文件 =="
  [[ -f /tmp/coldrevive-link-physical.state ]] && sed 's/^/  /' /tmp/coldrevive-link-physical.state \
    || echo "  （无）"
  echo "== 各类网关 WAN 口 =="
  for c in full-cone restricted-cone port-restricted-cone symmetric cgnat; do
    ns="$(gw_ns "$c")"
    ip netns list | awk '{print $1}' | grep -qx "$ns" || continue
    printf '  %-22s ns=%-24s wan=%-12s' "$c" "$ns" "$(wan_if "$ns")"
    ip netns exec "$ns" ip -o -4 addr show 2>/dev/null \
      | awk '$4 ~ /^192\.168\.1\./ {printf " phys=%s", $4} END {print ""}'
  done
  echo "== STUN(物理口) =="
  ss -lunp 2>/dev/null | grep -E "192\.168\.1\.(221|239):3478" | sed 's/^/  /' || echo "  （未启动）"
}

cmd_stun_lan_start() {
  if [[ -f "$STUN_PID_FILE" ]] && kill -0 "$(cat "$STUN_PID_FILE")" 2>/dev/null; then
    echo "STUN(物理口) 已在跑 pid=$(cat "$STUN_PID_FILE")"; return 0
  fi
  # 备用 IP 供 RFC 3489 CHANGE-IP 用；绑不存在的地址会直接失败，故先确认已配置。
  if ! ip -4 -br addr show "$PHYS_IF" | grep -q "192.168.1.239"; then
    ip addr add 192.168.1.239/32 dev "$PHYS_IF" 2>/dev/null || true
  fi
  # 必须 setsid：只加 nohup 时，ssh 会话结束会把整个进程组带走，
  # 于是 STUN 在「启动的那条 ssh 返回之后」就死了 —— 下一次分类会拿不到映射，
  # 结果被静默降级成 "只有中继候选"。中继/持有者一直用 setsid，这里对齐。
  setsid nohup python3 "$HERE/local_stun.py" --primary 192.168.1.221:3478 \
    --alt-ip 192.168.1.239 --alt-port 3479 > "$STUN_LOG" 2>&1 < /dev/null &
  echo $! > "$STUN_PID_FILE"
  sleep 0.8
  ss -lun 2>/dev/null | grep -q "192.168.1.221:3478" || { echo "STUN 启动失败，见 $STUN_LOG" >&2; exit 2; }
  echo "STUN(物理口) 已启动 192.168.1.221:3478 / 192.168.1.239:3479 pid=$(cat "$STUN_PID_FILE")"
}

cmd_stun_lan_stop() {
  if [[ -f "$STUN_PID_FILE" ]]; then
    kill "$(cat "$STUN_PID_FILE")" 2>/dev/null || true
    rm -f "$STUN_PID_FILE"
    echo "STUN(物理口) 已停止"
  else
    echo "STUN(物理口) 未在运行"
  fi
}

case "${1:-}" in
  up)             cmd_up "${2:-}" ;;
  down)           cmd_down ;;
  status)         cmd_status ;;
  stun-lan-start) cmd_stun_lan_start ;;
  stun-lan-stop)  cmd_stun_lan_stop ;;
  *) sed -n '2,40p' "$0"; exit 2 ;;
esac
