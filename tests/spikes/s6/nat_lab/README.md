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

### 2026-11-15（W3.6）双向打洞复测

W3.5② 的 0/30 被定位为**单向打洞**造成：请求侧向持有者映射地址发包，持有者只回
`NAT_PUNCH_ACK` 不主动回打，受限锥及以上的网关在收到首个入向包时既无 conntrack 条目、
过滤集里也没有对方，直接丢弃。W3.6 让请求侧把自己的映射候选（`back`）随打洞帧经**中继**
送达持有者，持有者据此先回打一包，双方各自开洞后再直连。

复测时**修正了一个对照错误**：W3.5② 的 4/5 组把请求方放在 `lan-symmetric` 后面，
对称 NAT 的映射与目的地址绑定，`back` 永远报不准 —— 那是任何打洞方案都救不了的组合，
和「持有者侧过滤」混在一起测，等于把两个变量搅在一起。本轮请求方固定为**锥形 NAT**
（`lan-port-restricted-cone`，其中 port-restricted 持有者一组用 `lan-full-cone`，
避免同组同网段），隔离出持有者侧过滤这一个变量。

| 组 | 持有者 | 请求方 | `direct` W3.5② → W3.6 | `relay` |
| :--- | :--- | :--- | :--- | :--- |
| a | full-cone | port-restricted | 30/30 → **30/30** | 30/30 |
| b | restricted-cone | port-restricted | **0/30 → 30/30** | 30/30 |
| c | port-restricted-cone | full-cone | **0/30 → 30/30** | 30/30 |
| d | symmetric | port-restricted | 0/30 → **0/30** | 30/30 |
| e | cgnat（双 NAT） | port-restricted | **0/30 → 30/30** | 30/30 |
| f | restricted-cone | **symmetric** | — → 30/30 | 30/30 |

每组 30 次 × 2 路径，实际 CLI/UDP/HTTP 观测；六轮中继计数均为 `dropped=0`，
`reset_err=0`（每次试验前清空网关 ipset，避免第 1 次开洞后余下 29 次吃残留状态）。

结论：
1. 受限锥 / 端口受限锥 / 双 NAT CGNAT 的**直连从 0/30 变成 30/30**，证明缺的是
   「持有者先回打」这一步，而不是 NAT 不可穿透。
2. **symmetric 持有者仍 0/30，且这是正确结果**：该网关无 DNAT、映射随目的地址变化，
   广告出去的 STUN 映射端点根本不是入向可达端口，而它回打时的映射端口请求方又无从得知。
   这类环境只能走中继 —— 与 WebRTC 的实践一致。
3. f 组（对称**请求方** + 受限锥**持有者**）30/30 不是异常：`restricted-cone` 的过滤是
   **address-dependent**（ipset 键为 `(持有者ip, 持有者port, 远端ip)`，不含远端端口），
   所以持有者回打到错误端口也按 IP 放开了过滤。`port-restricted-cone` 则**完全不使用
   ipset，只靠 conntrack**（键为 ip+port），两者语义确实不同 —— 这也是分类探针能把它们
   分开的原因，不是仿真缺陷。

残留问题（未解决，如实记录）：W3.6 的直连**依赖中继做信令**。中继不可用时
（本轮实测过一次：上一轮 relay 未真正监听，`connect refused`），`back` 送不出去，
直连会整体退化回「只有全锥可通」。因此中继不是「兜底」，而是双向打洞的**前置依赖**。
这一点必须写进 S6 的剩余工作项。

## 跨主机档位（cross-host-lab，2026-11-15）

同机两个 netns 满足不了「两端在两台物理主机」这条判据。只要把**其中一个端点**挪出实验机
就成立，而且**不需要对端主机任何权限**：

```bash
# 在实验机（221）上：把该类网关的 WAN 侧挂到物理 LAN（proxy ARP 代答）
# 地址由驱动挑选后经 LINK_PHYS_ADDR 传入；手工跑时可省略（退回按类默认地址）
sudo bash link_physical.sh up restricted-cone
sudo bash link_physical.sh stun-lan-start     # 192.168.1.221:3478 + .239:3479

# 在**另一台物理主机**（A，192.168.1.210）上：驱动整套编排并跑矩阵
python tests/spikes/s6/nat_lab/run_cross_host.py --class restricted-cone --trials 30
```

为什么不用「在 A 上加一条到 10.0.0.0/24 的路由」：A 是非管理员账户改不了路由表；
也不用 macvlan：VMware vSwitch 默认拒绝混杂模式，实测 100% 丢包。因此改成让网关
**多挂一个物理 LAN 地址**，由 221 在 `ens160` 上 proxy ARP 代答。
数据路径每方向只多一跳、不改写 IP，NAT 语义不被额外一层污染。

### 三道必须过的检查（2026-11-15 踩坑后加的）

它们对应三个**只让成功率变得不可信、却不会报错**的缺陷，详见
[nat-matrix.md §14.7](../nat-matrix.md)：

| 检查 | 为什么 | 不过会怎样 |
| :--- | :--- | :--- |
| **地址身份**：A 对该地址的 ARP 应答者 MAC == 221 `ens160` 的 MAC | 写死的 `.242/.244/.245` 被家庭 LAN 上**真实设备**占用；只 `ping` "有应答"就算通，应答的是邻居的机器 | punch 打到别人机器上，报告只写"唤醒无应答"，看起来像 NAT 不行 |
| **每轮 `reset_exit_code=0`** | Windows 上 `write_text()` 写出的 `reset.sh` 是 CRLF → 远端 bash 语法错误 → **ipset 复位从未执行** | 第一次开洞后余下试验全吃老本，成功率虚高 |
| **STUN 真实往返 + 收敛等待** | 物理口 STUN 若没 `setsid`，启动它的 ssh 一退出就被带走；链路刚 up 时 netns→STUN 也有一段不通 | 分类 `undetermined`、`nat_type=unknown`，矩阵静默退化成"只有中继"，成功率仍然漂亮 |

收敛等待实测：单层 NAT 约 10s，双层 CGNAT **28.8s**。

实测（30 次/路径，`delivered=120 / dropped=0`，上述三项全部通过）：分类 5/5 与建网声明一致，
`direct` 四类 30/30、symmetric 0/30，`relay` 全部 30/30 —— 与同机 netns 的 W3.6
逐行一致，说明 W3.6 的 30/30 不是「同内核」的假象。
报告 `evidence_scope=cross-host-lab`、`hosts=['DESKTOP-DJQSC95','ubuntu']`、
条款① `true`、条款② `false`、`evidence_valid=false`。
详见 [nat-matrix.md §14](../nat-matrix.md)。

## 真实 NAT 档位（real-multi-nat，2026-11-15）

前两档的 NAT 都是本实验室用 iptables 合成的。真档把**两个端点都放到真实设备后面**：

```bash
# 在 A（192.168.1.210）上执行；222 需已装同一把公钥、/root/bin 下有 release 二进制
python tests/spikes/s6/nat_lab/run_real_nat.py --smoke --capture   # 先验证接线
python tests/spikes/s6/nat_lab/run_real_nat.py --trials 30          # 正式矩阵
# 若该类不是 symmetric，用 --class 指定（必须与现场分类一致，否则中止）
```

拓扑与实测结果（30 次/路径）：

| 端点 | 本机地址 | 出口 | `direct` | `relay` |
| :--- | :--- | :--- | :--- | :--- |
| 持有者 `ubuntu23`(222) | 192.168.34.222 | 112.5.32.34（真实 NAT） | — | — |
| 请求者 `DESKTOP-DJQSC95`(A) | 192.168.1.210 | 112.5.60.4（真实 NAT） | **0/30** | **30/30** |

222 背后的设备实测为 **symmetric（Address/Port-Dependent Mapping）**：
同一 socket、本地端口固定，三个不同目的 IP 得到不同映射端口（2051/2055/2050）。
所以 `direct` **必定** 0/30 —— 这不是缺陷，是 symmetric 的定义性后果（广告的 STUN 端口
不是入向可达端口）。抓包证据：持有者出向回打 2 条到 `112.5.60.4:57238`，
而 A 打向 `112.5.32.34:2053` 的包**入向 0 条**。

**驱动把「真实设备」当可证伪条件，不是当声明**：

1. 映射端口 ≠ 内部端口且每次不同 → 无静态端口映射（真实 NAT 动态分配）；
2. 直连 attempt 的目标必须是**公网 egress**，命中私网地址即判 LAN 捷径并拒收；
3. 分类必须现场重测且与 `--class` 一致，否则中止（历史报告不作数 —— 同一台机器
   曾因单样本被判成 `port-restricted cone`）；
4. 两端出口必须不同、且都是公网地址，否则直接退出。

报告 `evidence_scope=real-multi-nat`、条款①② 均 `true`、`evidence_valid=true`（项目首次），
但 `evidence_valid_for_pass=false`、`S6` 仍 `not_run`、R-04 仍不判定。
**类别覆盖仍 1/4 + 0 CGNAT**：一个出口只对应一类，两个真实出口只能覆盖一类。

## 证据边界（不得突破）

- 判据是**两个互相独立的条款**：
  1. **两端在不同物理主机** —— 同机两个 netns 不满足；但用 `link_physical.sh` +
     `run_cross_host.py` 把请求者挪到另一台主机后**可以满足**（`cross-host-lab`，已实测）；
  2. **NAT 是真实设备**（`real_nat_evidence`）—— 这里是 iptables/conntrack 合成的 NAT，
     **换多少台主机都不会变成真实设备**。
  而且四类 NAT 需要四个不同出口，与主机数量正交。详见 [nat-matrix.md §13](../nat-matrix.md)。
- `nat_matrix.py` 因此改为显式证据层级 `--evidence-scope`
  （`real-multi-nat` / `cross-host-lab` / `vm-namespace-lab`，`--lab-mode` 是后者的兼容别名），
  报告里 `evidence_clauses` 逐条写 `satisfied`、`evidence_invalid_reasons` 直接写明缺哪条，
  避免再用一个布尔值把两条混在一起。
- 分类 5/5 已实测，但**可达率证据仍属实验室**，R-04 ≥70% 判定尚未作出。
- 脚本**永不**改 `spikes.json.status`，报告恒 `s6_status=not_run` / `g2_claim=false`。
  `pass` / `fail` 由实验执行人在真实物理主机 + 真实 NAT 设备上复测后手工回填。

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
9. **中继「就绪」不能只看 stats 文件**：上一轮 relay 刚被 terminate 时，同端口重启可能
   绑定失败而立即退出，但 stats 文件已经写出。此时整轮 `relay` 路径全 `Connection refused`，
   而 `direct` 又会因回打信令失败整体退化 —— 一份看起来像「打洞失败」的假数据。已加
   **真实 TCP 连接校验**（`wait_tcp`，失败重启一次，仍失败则明确退出）、以及跑末打印 relay 计数。
10. **进程清理的 `pkill` 模式必须能匹配绝对路径**：`pkill -f "^coldrevive-relay"` 匹配不到
   `/opt/coldrevive/repo/target/release/coldrevive-relay`，上一轮的 relay 会泄漏并污染下一轮。
   改为 `pkill -f "coldrevive-relay --listen"`，并在每组之间 `sleep 2` 让端口释放。
11. **限流器会伪装成业务失败**：持有者侧回打限流（容量 20、每秒补 2）在连续试验里周期性打空，
   表现为 direct 成功率固定掉到 23/30，且**失败序号呈严格周期 3**（12/15/18… 或 13/16/19…）。
   识别方法就是看失败分布是否周期性 —— 周期性失败几乎一定是本地状态机而不是网络。
   参数已调整为容量 64 / 每秒 16，并补了两条回归用例（旧参数必须复现打空、新参数不得打空）。
12. **ipset 条目不复位会让成功率虚高**：受限锥网关的 `CONTACTED` 有 600s 超时，不复位则第 1 次
   开洞后余下 29 次吃老本。`nat_matrix.py --inter-trial-reset` 在每次试验前清空，
   复位失败会记进该行 `reset_error`，不静默忽略。

13. **抓包不能按端口过滤**：过 NAT 后源端口已被改写，`tcpdump -i any 'udp and port 19901'`
    在真实 NAT 侧只能抓到 loopback 噪音（第一次就这么白抓了一轮）。要按**对端公网 IP**抓：
    `tcpdump -i ens160 -n 'udp and host 112.5.60.4'`。另外 `-i ens160` 的输出**没有 In/Out 标记**
    （那是 `-i any` 才有的），判方向只能看对端 IP 在 `>` 之前还是之后。
14. **"收到一个 tid 匹配的包"不等于"服务器换了源回包"**：公共 STUN 常忽略 `CHANGE-REQUEST`，
    从原地址回包。早期实现只校验 tid，于是真实 symmetric 设备被判成 `full-cone`。
    现在正对照要求 `src == addr`、change 测试要求 `src != addr`。
15. **单样本不能判对称性**：只有一台 STUN 有应答时，`classify_from_mappings` 曾直接返回
    `FullCone`（"至少是非对称"是假设）。现在要求 ≥2 个不同目的地址，否则 `Unknown` ——
    宁可说不出结论，也不编一个。公共 STUN 可用性因网络而异（本例 222 上 google/voipbuster
    全超时，cloudflare/telnyx/miwifi/bilibili 稳定），所以 `PUBLIC_STUN` 要按两端实测挑选。
