# S6 NAT 实验室（单台 Linux 主机）

把「四类真实 NAT + CGNAT」的环境门槛压到最低：**一台 Linux 主机**，用
`setup_nat_lab.sh` 在 network namespace 里用 iptables conntrack **真改写**出四类
NAT 行为与一层 CGNAT。网关做的是内核级 NAT（不是应用层代理），因此从 LAN netns 内
跑 `nat_behavior.py` 得到的分类报告就是真实 NAT 行为证据。

2026-11-15 已在 Ubuntu 23.04 / kernel 6.2 的 VMware 虚拟机上**跑通全流程**：
五类环境全部分类成功，并完成逐类 30 次真实唤醒矩阵。

## 两个必须先理解的设计点

### 1. 必须在实验室内部起 STUN，不能查公共 STUN

从 LAN netns 访问公网时，报文还会穿过本机自己的出口 NAT。外层 NAT 会覆盖甚至掩盖
内层的映射/过滤特征（外层若是对称型，内层 full-cone 也会被测成对称型）。

`local_stun.py` 跑在宿主 WAN 网桥（`10.0.0.1:3478`、`10.0.0.2:3478`、`:3479`），
路径就变成「LAN netns → 被测网关（被测 NAT）→ 宿主 STUN」，中间只有被测那一层 NAT，
且完全离线、不受公共 STUN 限流影响。

### 2. cone 类网关必须有「可解析的入向映射」（DNAT）

NAT 可达性由两件事共同决定：**映射能否被解析回内网** + **过滤是否放行入向 NEW**。
只做 SNAT 不做 DNAT 时，任何 unsolicited 入向包都没有 conntrack 状态可查，
于是 **full-cone 与 port-restricted 会测出完全一样的结果**（都被丢弃）——这是假阴性。

所以 `setup_nat_lab.sh` 对三类 cone 网关补了端口保持的 1:1 DNAT
（`-t nat PREROUTING -i <wan> -p udp -j DNAT --to-destination <lan_ip>`），
再由 FORWARD 规则决定放行哪些入向 NEW：

| 类 | 映射 | 过滤 |
|---|---|---|
| full-cone | EIM，端口保持 + DNAT | 任意源入向 NEW 放行 |
| restricted-cone | 同上 | 仅放行「该内网 ip:port 曾发往过的远端 IP」（`ipset hash:ip,port,ip`，任意端口） |
| port-restricted-cone | 同上 | 只放行 ESTABLISHED,RELATED |
| symmetric | `SNAT --random-fully`，映射随目的变化 | 无 DNAT（本就不接受 unsolicited） |
| cgnat | 内层 SNAT 到 `100.64.0.0/24` + 外层 SNAT | 两层均只放行 ESTABLISHED |

`ipset` 只与 **iptables-legacy** 兼容（Ubuntu 默认 `iptables` 是 nf_tables 后端），
脚本默认自动选择 `iptables-legacy`，可用 `IPT=...` 覆盖。

## 拓扑（全部在单机内）

```
宿主 br-wan 10.0.0.1 / 10.0.0.2   ← 本地 STUN + coldrevive-relay 跑这里
 ├─ gw-full-cone (10.0.0.10)          ── lan-full-cone (192.168.10.2)
 ├─ gw-restricted-cone (10.0.0.11)    ── lan-restricted-cone (192.168.20.2)
 ├─ gw-port-restricted-cone (10.0.0.12) ── lan-port-restricted-cone (192.168.30.2)
 ├─ gw-symmetric (10.0.0.13)          ── lan-symmetric (192.168.40.2)
 └─ gw-cgnat-outer (10.0.0.14) ─ gw-cgnat (100.64.0.1) ─ lan-cgnat (192.168.50.2)
```

## 跑通流程

```bash
# 0) 前置：root + iptables-legacy + ipset + python3
sudo bash setup_nat_lab.sh up          # 建四类 NAT + CGNAT，产出 lab-evidence.json
sudo bash setup_nat_lab.sh stun-start  # 起本地 STUN（10.0.0.1/10.0.0.2）
sudo bash setup_nat_lab.sh status      # 检查 netns / 规则 / ipset / STUN

# 1) 逐类分类（每类在各自 LAN netns 内，指向本地 STUN）
for c in full-cone restricted-cone port-restricted-cone symmetric cgnat; do
  sudo ip netns exec lan-$c python3 ../nat_behavior.py \
    --mapping-servers 10.0.0.1:3478,10.0.0.2:3478 \
    --filter-servers  10.0.0.1:3478 \
    --label "$c-lab" --out /opt/coldrevive/reports/s6-nb-$c.json
done

# 2) 逐类真实矩阵（两个 netns 内各起 coldrevived + 真实中继，30 次真实唤醒）
python3 run_matrix_lab.py --class full-cone \
  --holder-ns lan-full-cone --requester-ns lan-symmetric --trials 30
# 其余四类同理，holder/requester 必须分处不同 NAT（同 netns 会走私网地址，绕过 NAT）
```

`run_matrix_lab.py` 负责：起 `coldrevive-relay` → 两个 netns 各起 `coldrevived`
→ holder `nat probe`（真实 STUN 建立映射）→ 取 `/nat/advertise` 交给 requester
`nat peer` → 调 `nat_matrix.py --mode real --lab-mode` 跑 30×2 次真实唤醒 → 收尾。

## 2026-11-15 实测结果

分类（`nat_behavior.py`，带独立对照 socket 与服务器能力正向对照）：

| 环境 | 分类 | 置信度 |
|---|---|---|
| full-cone | full cone（EIM + EIF） | high |
| restricted-cone | restricted cone（EIM + AD） | high |
| port-restricted-cone | port-restricted cone（EIM + APDF） | medium |
| symmetric | symmetric（Address/Port-Dependent Mapping） | low |
| cgnat | port-restricted cone（双 NAT 之后） | medium |

可达率（`nat_matrix.py --lab-mode --trials 30`，成功只认真实 `ack_status=OK`）：

| 持有者所在 NAT | direct（打洞直连） | relay（中继回退） |
|---|---|---|
| full-cone | **30/30** | 30/30 |
| restricted-cone | 0/30 | 30/30 |
| port-restricted-cone | 0/30 | 30/30 |
| symmetric | 0/30 | 30/30 |
| cgnat | 0/30 | 30/30 |

**结论（重要）**：当前只由请求方向对端映射地址发 `NAT_PUNCH`，因此只能进入
full-cone 持有者。restricted / port-restricted 的过滤按「该内网 ip:port 曾发往的远端」
放行，只有持有者也先向请求方发包才能开洞 → 需要**双向打洞 / rendezvous**。
这是工程缺口，不是 NAT 不可穿透。中继路径五类全部 30/30，说明回退覆盖有效。

这个「按 NAT 类型分化」的结果本身就是仿真保真度的证据：若四类被仿真成一样，
结果不会呈现 full-cone 全通、其余全断的形态。

## 证据边界（不得突破）

- 实验室是**同机两个 netns**，不满足 S6 判据的「两台不同物理主机」条款。因此
  `nat_matrix.py` 新增 `--lab-mode`：强制 `evidence_valid=false`、
  `evidence_scope=vm-namespace-lab`，并要求 attestation 显式声明 `lab_mode=true`，
  防止 lab 数据被误当真实证据。
- 分类 5/5 已实测，但**可达率证据仍属实验室**，R-04 ≥70% 判定尚未作出。
- 脚本**永不**改 `spikes.json.status`，报告恒 `s6_status=not_run` / `g2_claim=false`。
  `pass` / `fail` 由实验执行人在真实物理主机上复测后手工回填。

## 清理

```bash
sudo bash setup_nat_lab.sh down
```

## 排错留痕（2026-11-15 实际踩到的坑）

1. **本地 STUN 总是从主地址回包** → 对 `10.0.0.2:3478` 的请求从 `10.0.0.1:3478`
   回应，回包元组与 conntrack 期望不符，被对称/端口受限网关当 unsolicited 丢弃，
   导致备用地址「假不可达」。已改为从**收到请求的那个地址**回包，并补回归自检。
2. **只做 SNAT 不做 DNAT** → full-cone 与 port-restricted 结果相同（见上文设计点 2）。
3. **veth 接口名 15 字符上限**：`lx-restricted-cone_p` 建不出来，改用序号 `lx-1`。
4. **CGNAT 内层网关缺默认路由** → 到公网报 `Destination Net Unreachable`。
5. **持有者角色在代码里硬编码为 `node_id == "A"`**（`bootstrap_holder`），
   驱动曾把持有点放到 B，导致 `nat wake` 连到对端 IP 失败。
6. **健康检查/通告必须在目标 netns 内发起**：宿主没有到对端 LAN 子网的路由。
7. **唤醒必须携带真实 `sha256(file)` 作为 file_hash**，否则持有者按设计返回
   `REJECT_NOT_FOUND`（会被如实计为失败，不是「可达」）。
8. `pkill -f local_stun.py` 会匹配到远程 shell 自身命令行并把自己杀掉，
   用 `^python3 local_stun` 锚定。
