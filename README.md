# ColdRevive

> 把文件放进共享目录，它就永远能被搜到、被下到。

一个**不修改任何现有 P2P 协议**的混合下载客户端：以 eMule KAD 为去中心化黄页做发现，以轻量私有唤醒信令激活在线持有者临时做种，以标准 BitTorrent / eD2K 协议传输数据。对旧客户端完全透明。

| 项 | 值 |
| :--- | :--- |
| 当前阶段 | **M2 进行中 / W3.8 中继去单点化**：G1 已通过（2026-11-14），M2 门禁解除；W3.3 接线 + W3.4 生产中继服务端 + W3.5① 条件审计 + W3.5② NAT 实验室 + W3.6 双向打洞 + W3.6b 跨主机档位 + W3.6c 取证可信度 + W3.7 真实 NAT 档位（条款①②首次同时成立）+ **W3.8 多中继实例注册同步（不再单点）** 已落地；**W5.1 引擎落地**（`cr-engine` 真实实现，`/jsonrpc addUri` 真下载并逐片校验）＋**W5.1b 做种切换引擎**（第三方 aria2 从我们的引擎取片成功）＋**W5.1c LAN 跨主机复核**（221 上的 Transmission / qBittorrent 越过真实网卡取片成功）；**G2 仍未宣称达标**（类别覆盖仅 1/4 + 0 CGNAT） |
| 代码仓库 | **https://github.com/asdfgh129/ColdRevive**（公开） |
| 在线文档 | **https://asdfgh129.github.io/ColdRevive/**（GitHub Pages 自动部署） |
| 文档版本 | **v2.4.79（2026-11-18 M2 实现推进：`cr-dht` 从骨架到可运行的 Mainline DHT（38 单测，其中 9 条真实 UDP 回环）+ 新增 `coldrevive-dht` 自检/探测工具；**LRU 接引擎**（`cr-sched` 活跃池 + `wake_seed` 作为唯一 `activate` 入口，[ADR-017](docs/02-architecture/adr/ADR-017-active-seed-pool.md)）；**中继生产化**（按来源 IP 的并发/注册上限、`--deny` 黑名单、注册表 16 片分片）；**TCP 同时打开打洞原语**（判据 `poll`+`SO_ERROR`，回环两独立进程双侧建立+双向字节 PASS，[ADR-018](docs/02-architecture/adr/ADR-018-tcp-hole-punch.md)）；`cargo test --workspace --offline` 228 passed / 0 failed；**`g2_claim=false`、`S6.status=not_run` 不变**，且 BT-over-TCP 未接线、Unix 分支未编译验证、公网双侧 `BLOCKED` 三条限制照写）**；v2.4.78（2026-11-17 判定：S3=`pass` / S5=`fallback` / S6=`pass`（`closed_with_exceptions`，口径「按 eMule 语义结案，带已知限制」）；三项均依真实证据由所有者手工回填，记录见 `docs/06-plan/G2-SPIKE-VERDICT-2026-11-17.md` 与 `tests/fixtures/g2-spike-verdicts.toml`；判定不改机器常量 `s6_status=not_run` / `g2_claim=false` / `evidence_valid_for_pass=false`，**G2 仍未达标**（欠 M2 实现项））**；v2.4.77（2026-11-17 实测回填：S3 公网 Mainline DHT 测试 key 宣告/独立查询取回 + 跨进程 RSS 25.04 MiB；S5 真实内核 tc/netem 链路默认 ratio=0.02 前台劣化 0.00%（3/3）；S6 eMule 式两半真实证据齐备，aggregate `OBSERVED`/`complete=true`，按 eMule 语义结案带已知限制；三个 spike 的 `status` 仍 `not_run`、`g2_claim=false`）**；v2.4.76（ADR-016：S6 阶段性判据改为 eMule 式可达语义，NAT 类别覆盖不再是 pass 前提；`nat_matrix.py --self-test` 24/24、`cargo test --workspace --offline` 168 passed / 0 failed；`S6.status=not_run` / `g2_claim=false` 不变）**；v2.4.75（W5.1g：aMule 公网 KAD 索引面复核 **20/20**；W5.2：ADR-013「先查再打洞」**21/21 连续 3 轮**，并修掉订阅读超时误判断链导致中继声明在重连空窗被抹掉的真缺陷；`cargo test --workspace --offline` 166 passed / 0 failed；`g3_verdict=not_run` / `s6_status=not_run` 不变）；v2.4.74（W5.1f：**公网 µTP PASS，25/25 连续 3 轮**，本轮无代码改动 —— W5.1d 验的是公网 TCP、W5.1e 验的是 LAN µTP，拼不起来的点是**映射**：CPE 上两条 TCP 规则对 UDP 无效。运维补上第三条 `UDP 112.5.60.4:17802 → 192.168.1.210:17802` 后，外部主机 222（WAN `112.5.32.34`）用 qBittorrent 4.5.1（`Session\BTProtocol=2`，仅 uTP）经**只含公网 announce** 的种子变体取完 4 MiB。两侧证据：客户端自报 `ip=112.5.60.4, port=17802, connection=μTP`（1473–1577 条采样），**A 侧 `bt_port` 全程没有任何 TCP 连接**（公网、LAN 都为空）；UDP 映射另用与 µTP 无关的回显服务单独验证（A 侧源地址必须是 `112.5.32.34`，否则是 LAN 捷径）。落盘 SHA-1 `5118386a…f308a483` 一致、供片方 `served_bytes` 0 → 4194304、COLD 负对照 0 字节。同轮修掉三个 harness 缺陷（`POLL_END` 混进最后一块 JSON 把 100% 读成 97%、`0.99x` 中断条件在 99.2% 提前 break、复位不彻底导致续传 48 KiB）。观察项：公网 4 MiB 143–153 s，与 TCP 档（155–235 s）同量级，不折算吞吐（ADR-012）。163 passed / 0 failed；`evidence_scope=public-utp-single-egress`、`is_s6_evidence=false`）；v2.4.73（W5.1e：**µTP（BEP 29）已补上并通过，20/20 连续 4 轮** —— 先纠正一个错判：librqbit 9.1.0 本来就带 `librqbit-utp`，只是 `ListenerOptions.mode` 默认 `TcpOnly`，「默认关」被读成了「不支持」。现在 `EngineOptions.enable_utp` 默认开、监听时 TCP+uTP 同端口，`nat.utp=false` 可退回纯 TCP。验收要求**两侧同时成立**：Transmission 3.00（默认 `utp-enabled`）与 qBittorrent 4.5.1（`BTProtocol=2` 仅 uTP）都跑到 `100%`、客户端自报 `isUTP=true` / `connection=μTP`，且 **A 侧 `bt_port` 全程没有来自 221 的 TCP 连接**（逐秒采样 0 命中）；落盘 SHA-1 `8d5d0dea…309d72`、供片方计数 4 MiB + 4 MiB。公网 µTP 还缺一条 UDP 映射（未验）。163 passed / 0 failed；`evidence_scope=lan-utp-interop`、`is_s6_evidence=false`）；v2.4.72（W5.1d 收口：**公网入向 PASS，17/17 累计 8 轮** —— 运维在 CPE 建好两条 TCP 映射后，外部主机 222（WAN `112.5.32.34`）经公网完整跑通「取种 → announce → 唤醒 → 取片」：端口 `:17800`/`:17802` 均 OPEN、tracker 对公网请求方回 `112.5.60.4:17802`、COLD 负对照 0 字节、唤醒后客户端 `100%` 且落盘 SHA-1 一致、供片方计数覆盖全量，**A 侧连接对端只有 `112.5.32.34`（无 LAN 捷径）**。同时修掉 harness 三个「把量具坏了伪装成产品坏了」的缺陷（`MaxStartups` 载体失败被读成业务失败、`xxd` 过控制台污染二进制、Windows 上 ControlMaster 不可用）。观察项：公网单连接 4 MiB 155–235 s（LAN 秒级、形态稳定不停滞），按 ADR-012 不折算任何速率数字。161 passed / 0 failed；`evidence_scope=public-inbound-single-egress`、`is_s6_evidence=false`）；v2.4.71（W5.1d：**公网入向接线完成，实测 BLOCKED** —— 按所有者选择的 A 方向，把「外部客户端能进来」在代码与判据上补齐：配 `nat.bt_external`/`nat.http_external` 后 ① 做种监听落 `0.0.0.0:bt_port` ② 种子写 BEP 12 `announce-list`（LAN+公网，**InfoHash 不变**）③ tracker 按请求方 IP 分别回公网/私网 peer ④ 唤醒 ACK 的取种 URL 按来源切换 ⑤ `/nat/public` 暴露配置态且 `inbound_verified` 恒 false。验收 `tests/e2e/w51d/public_inbound.py` 要求**两侧证据同时成立**（客户端只用公网 announce；A 侧连接表对端必须是公网 IP）。**实测：`112.5.60.4:17800/17802` 从公网超时，该 CPE（`192.168.1.254`）UPnP/NAT-PMP/PCP 全不可用、本机无 IPv6 → `status=blocked`**，缺的是运维在 CPE 上建两条 TCP 映射。161 passed / 0 failed；W5.1c 复跑 18/18；`evidence_scope=public-inbound-single-egress`、`is_s6_evidence=false`，**不含任何可达率数字**）；v2.4.70（W5.1c：**LAN 跨主机复核** —— 把下载方搬出回环，221（Ubuntu，真实网卡）上的 **Transmission 3.00 与 qBittorrent 4.5.1** 依次从 A 的引擎取完 4 MiB，落盘 SHA-1 与样本一致，供片方计数器两阶段各 4194304 B。本轮抓到并处理两个真问题：① **µTP 互操作缺口**（Transmission 默认优先 µTP，对端只做 TCP 时抓包可见 `UDP 19501→18782` 却无 SYN，回落极慢；librqbit 不支持 µTP，已在测试与文档中显式登记）；② **`/scrape` 一直 404**（标准客户端加种子即 scrape，404 让它持续报错退避），已实现标准 scrape 响应。3 次复跑 **18/18**，157 passed / 0 failed；`evidence_scope=lan-cross-host`、`is_s6_evidence=false`，**不含任何可达率数字**）；v2.4.69（W5.1b：**做种也交给引擎，一条做种路径** —— `bt.rs` 的手写 Peer Wire 种子端删除，只留本地 tracker；A 启动即以 `EngineOptions.listen` 监听 `bt_port`、种子以 `paused` 加入（COLD 不供片），`WAKEUP_REQ` 才 `activate`（并修掉「重复唤醒把 hot 翻成 false」的非幂等缺陷）。验收 `tests/e2e/w51b/seed_to_thirdparty.py` 3 次复跑 9/9：**唤醒前第三方 aria2 8 s 拿到 0 字节**，唤醒后完整下完 1 MiB、SHA-256 一致、`served_bytes=1048576`。T0 real 21/21（T0-08 即第三方 aria2 从引擎取片），155 passed / 0 failed。**口径收窄**：W5.1 那条两端同库的测试不再算独立互证）；v2.4.68（W5.1：**取消零依赖，生产守护进程有了自己的数据面** —— [ADR-015](docs/02-architecture/adr/ADR-015-dependency-layering.md) 改为按层划分：协议/信令面 15 个 crate 继续 std-only，`cr-engine`/`coldrevived` 用 librqbit 9.1.0 + tokio。`cr-engine` 从骨架变为实现（`RqbitEngine`，Engine Trait 改为按任务形态，Tokio 运行时封在 crate 内），`coldrevived` 的 `/jsonrpc addUri` 真的下载、`tellStatus` 每次重新采样。验收 `tests/e2e/w51/engine_download.py` 4 次复跑 8/8：B 被唤醒后从 A 拉完 1 MiB（4 片/64 次 request），落盘 SHA-256 与样本一致，A 侧 `served_bytes=1048576` 两方对账。155 passed / 0 failed。**T0/T1 口径不变，S3/S5/S6 仍 `not_run`，不含任何可达率数字**）；v2.4.64（W3.15：**三台 LAN 主机真实第三方客户端互操作** —— Transmission 3.00 / qBittorrent 4.5.1 / aria2-next 各下一轮标准 `.torrent`，均 1 MiB 全量 + SHA-1 匹配 + seeder 上传 1048576 B；G3 附件证据，`not_run` 不变）；v2.4.63（W3.14b：生产 `cr-hash` 具备真实 eD2k hash —— 零依赖 MD4 + aMule **Red** 分块规则 + 标准 `ed2k://` 链接生成；此前只有 S2 探针有，旧客户端拿到虚假 `fileID` 会搜得到却下不了。148 passed / 0 failed）；v2.4.62（W3.13：**中继数据面字节预算**（[ADR-014](docs/02-architecture/adr/ADR-014-relay-data-budget.md)）—— `--data-rate` 默认 512 KiB/s、`--data-burst` 默认 2 MiB；≤4 KiB 唤醒信令绕过预算，超额整帧拒绝并计 `bandwidth_limited`，与 `dropped` 分开。部署侧带宽护栏，**不是可达率承诺**。按所有者决策重排路线：先标准协议互通（S1/S2 → G3 矩阵），再补种；出口类别 `--census` 移入 S6 证据阶段）；v2.4.61（W3.12：**借 eMule 的 LowID 自我声明**（[ADR-013](docs/02-architecture/adr/ADR-013-emule-lowid-declaration.md)）—— 每个节点自己探测、自己声明可达性类别（`direct` / `relay-only` / `unknown`），注册中继时随帧上报，别的节点可先查再决定路径，不必先试一次注定超时的直连。symmetric→relay-only、锥形→direct、**cgnat→unknown（不可按名字推断）**；声明带 TTL，过期降级 unknown；`unknown` 永远不许当 lowid 用（纪律在代码里）。协议帧纯追加（0x08–0x0B），部署顺序**先升中继再升客户端**。唤醒路径尚未接入「先查再打洞」，本轮只交付原语 + 诊断消费）；v2.4.60（W3.11：**R-04 改为「尽力」，取消可达率的数值承诺**（[ADR-012](docs/02-architecture/adr/ADR-012-r04-best-effort.md)）—— 删除 `R04_MIN_REACHABILITY=0.70`，改 `R04_STANCE="best-effort"` + `R04_NUMERIC_COMMITMENT=None`（保留字段名让机器读到「明确不给数」），加单测防阈值被好心加回；矩阵配置把 `r04_min_reachability` 换成四条定性判据（5 类各有真实设备证据 / 每类两条路径各 ≥30 次真实观测 / 直连失败时中继必须送达 / 结论手工回填）；`nat_matrix.py` 不再做阈值比较（自检 17/17）。**中继回退改为常开**，不再等一个永远不会到来的触发条件。理由不是「达不到」，而是：产品卖的是「多一条能叫醒机器的路」不是 SLA；阈值会污染测量（逼出挑环境/填格子）；分母（所有家用 NAT）无法抽样，5 个样本的百分比无统计意义却被当 SLA 引用。**一条强度都没放松**：条款①②与 `evidence_scope` 分层不变，S6 仍 `not_run`）；v2.4.59（W3.10：出口类别清单 —— `coldrevive-nat` 新增 `--census`/`--report`，把每次探测沉淀成 JSONL 样本（`nat_class` + 正交两维映射/过滤行为 + 依据观测条数 + 双层 NAT 判据 + 配置变体 + 防重 `sample_id`），`TraversalReport` 补 `filter_behavior` 与原始 `observations`，让每个类别结论都能回溯到具体报文。三条纪律写进代码：同一出口同 `variant` 出现两类 ⇒ 整条不进清单、`matrix_coverage=none`/`r04_judgeable=false`/`s6_status=not_run`/`g2_claim=false` 是**常量**、读回未知字段报错带行号。实测 A 机家宽出口 `112.5.60.4` = **port-restricted-cone**（eim + port-restricted）；结合 W3.7 对 `112.5.32.34` 的 APDM 结论，**现有两个出口本就是两类**，缺 `full-cone`/`restricted-cone`/`cgnat`）；v2.4.58（W3.9：中继公网生产化 —— `cr-nat::secure` 补 PSK 双向认证 + 逐记录加密 + 序号防重放（**不是 TLS**：无证书/前向保密，未经审计），心跳 `PING/PONG` 放记录层使订阅连接无需保活；再加连接数/速率/空闲/注册表/握手时限五道闸，且**绑非回环地址无 PSK 直接拒绝启动**。两台真实主机：`pair` PASS、`latency` 30/30（p50 0.88 ms）、跨实例加密 **60/60 帧**（avg 0.4 ms）、两端 `dropped=0`；反证三条（错误密钥 / 明文客户端 / 无密钥绑公网）全部如实失败。同轮纠正：`112.5.32.34:57100 → 222:7100` **并未打通**（同 CPE 的 54022 27 ms 可达、57100 按对端 IP 抓包 0 包），因此本轮**没有跨公网时延数字**）；v2.4.57（W3.8：中继去单点化 —— 两个真实实例（`221:7100` / `209:7100`）通过新增的 `PEER_HELLO`/`PEER_SYNC`/`FORWARD` 帧互认知并同步注册表快照，跨实例双向 **30 轮 60 帧全送达**（avg 0.3 ms）、两端 `dropped=0`；重启一台后持有者不重注册仍可达（快照自愈）；反证：不配 `--peer` 的隔离实例同一条用例立即 `dropped=1`；客户端 `nat.relay` 支持列表并顺序切换。同轮纠正上一轮的错误结论：`112.5.32.34:54022` 映射**一直是通的**，此前按端口抓包漏看是因为 NAT 同时改写了源端口）；v2.4.56（W3.7：真实 NAT 档位 —— 持有者 222（真实 NAT → 112.5.32.34）、请求者 A（真实 NAT → 112.5.60.4）、中继 221 仅信令；现场分类修正为 **symmetric**（APDM，手工报文实验确证），并修掉两个「编造分类」缺陷：单样本即判 cone、CHANGE-REQUEST 不校验回包源地址；30 次/路径 `direct` **0/30**（symmetric 的正确结论）、`relay` **30/30**，首次出现 `evidence_valid=true` 的单类报告）；v2.4.55（W3.6c：修掉 `nat probe` 恒报 restricted-cone 的分类缺陷（改用 CHANGE-REQUEST 实测过滤行为 + `filter_behavior_fresh`），并修掉三个 harness 缺陷 —— ipset 复位从未执行（CRLF）、物理地址撞上真实设备、STUN 进程被 ssh 带走导致静默降级；跨主机数字自此可复现）；v2.4.54 = W3.6b 跨主机档位（条款① `true` / 条款② `false`），v2.4.53 = S6 判据拆两条独立条款 + `--evidence-scope`，v2.4.52 = W3.6 双向打洞（`direct` 0/30 → 30/30），v2.4.51 = W3.5② NAT 实验室，v2.4.50 = W3.5① 条件审计 |
| M1 / G1 | T0 `--mode real` **21/21 PASS**（2026-11-14 复跑回归仍 21/21）；T1 `--mode real` 双机（A=192.168.1.210 / B=192.168.1.209）**11/11 PASS、`evidence_valid=true`**（2026-11-14）；两者共同构成 **G1 通过**证据 |
| M2 状态 | **进行中**：`cr-nat` 已接入 `coldrevived` 的跨 NAT UDP 唤醒（候选计划 + 单 socket 打洞 + 中继回退）与 tracker peer 地址协商；新增 `nat` CLI 子命令与 `/nat/advertise`。**W3.4**：`cr-nat::relay_server` 纯帧路由 + 独立 `coldrevive-relay` 二进制 + `coldrevived` 持有者订阅线程已落地，[T2](tests/e2e/t2/README.md) 用真实服务端（10/10、`delivered` 按增量归因、`evidence_valid=false`）。**W3.6 双向打洞**：`NAT_PUNCH` 携带请求侧映射候选、持有者主动回打、请求侧单 socket + STUN 自映射、中继作信令通道。**W3.8 去单点化**：`--peer` 让多实例互推注册表快照、目标不在本地时单跳 `FORWARD`，`nat.relay` 支持列表切换；两台真实主机实测跨实例 60/60 帧送达、`dropped=0`，但仍**无鉴权、无重传、快照全量**。**W3.9**：中继传输层升级为 PSK 认证 + 逐帧加密（`cr-nat::secure`），并补连接数/速率/空闲/握手时限，公网部署不再是无认证明文。**W3.10 出口类别清单**：`coldrevive-nat --census/--report` 把「还差哪些类别」变成可汇总的机器可读状态（清单 ≠ 覆盖，不含任何可达率数字）。**W3.12 LowID 自我声明**（ADR-013）：节点自报可达性类别（借 eMule「登录时报 LowID」，不借「LowID 即终点」——我们的中继转发的是几百字节信令，不是整个文件）。**S6.status 仍 `not_run`，不宣称 G2 达标**。**2026-11-18（v2.4.79）**：`cr-dht` 实现完成、LRU 接引擎、中继生产化收尾、TCP 打洞到「原语」层（BT 数据面未接线），`g2_claim=false` 不变 |
| 代码基线 | 根 `Cargo.toml` + 17 crate + **6 bin**（2026-11-18 增 `coldrevive-dht`）；依赖**按层**（ADR-015）：协议/信令面 15 个 crate std-only，`cr-engine`/`coldrevived` 用 librqbit 9.1.0 + tokio，版本集中在 `[workspace.dependencies]`、`Cargo.lock` 入库、验收一律 `--offline`；`cargo test --workspace --offline` **228 passed / 0 failed**（2026-11-18）；PROTOCOL_VERSION 仍为 1、T0 golden 未动 |
| S3 / S5 / S6 | **机器常量仍 `not_run`；2026-11-17 已由所有者依证据手工判定 S3=`pass` / S5=`fallback` / S6=`pass`（`closed_with_exceptions`，口径「按 eMule 语义结案，带已知限制」，见 `docs/06-plan/G2-SPIKE-VERDICT-2026-11-17.md`），两项口径不要混用**（G0 条件冻结的 M1/M2 必测回填项）。**W3.5① 条件审计判三者 `PARTIAL_RUNNABLE`**。S3：双机双栈集成 **17/17 PASS**（附件）。S5：真实 uTP 5 轮复跑，默认 0.2 档 **14.78% 不达标**。S6：**W3.5② 在单台 Ubuntu VM 的 `ip netns` 内建成四类 NAT + 双 NAT CGNAT 实验室**（真 conntrack/SNAT/DNAT 语义 + 本地 STUN），分类 **5/5 与建网声明一致**；**W3.6 双向打洞复测**：`direct` 受限锥/端口受限锥/CGNAT 由 **0/30 → 30/30**，symmetric 仍 **0/30**（正确结果，只能走中继），`relay` 六组均 30/30。**判据是两条独立条款**（[nat-matrix.md §13](tests/spikes/s6/nat-matrix.md)）：① 两端在不同物理主机 ② NAT 是真实设备 —— 加主机只满足①，永远满足不了②，且四类需 ≥4 个不同出口；现由 `--evidence-scope` 显式分层（`real-multi-nat` / `cross-host-lab` / `vm-namespace-lab`），实验室报告逐条给出 `evidence_clauses`。**W3.6b 跨主机档位**：把请求者挪到第二台物理主机 A（请求者本机），网关以物理 LAN 地址经 proxy ARP 暴露；30 次/路径下 `direct` full/restricted/port-restricted/cgnat **均 30/30**、symmetric **0/30**、`relay` 五类 **30/30**，与同机 netns 逐行一致；报告 `evidence_scope=cross-host-lab`、条款① `true`、条款② `false`。**W3.6c**：`nat probe` 改用 CHANGE-REQUEST 实测过滤行为（此前五类恒报 restricted-cone），并修掉 ipset 复位从未执行、物理地址被真实设备占用、STUN 进程被 ssh 带走三处 harness 缺陷 —— 每条数字现在都有独立反证（分类复测 5/5、ARP 身份 MAC、`reset_exit_code=0`、STUN 真实往返）。**W3.7 真实 NAT 档位**：持有者在 222（真实 NAT → 112.5.32.34）、请求者在 A（真实 NAT → 112.5.60.4）、中继仅信令；现场分类为 **symmetric**（APDM，手工报文实验确证：同一 socket 三个不同目的 IP 得到 2051/2055/2050），`direct` **0/30**（symmetric 的正确结论，抓包证明入向包被 NAT 丢弃）、`relay` **30/30**；报告 `evidence_scope=real-multi-nat`、条款① ② 均 `true`、`evidence_valid=true`（首次），但 `evidence_valid_for_pass=false`。**类别覆盖仍 1/4 + 0 CGNAT**（一个出口只对应一类）；仍缺 G3 矩阵、真实拥塞链路。**W3.8 中继去单点化不改变以上任何结论**：它只补中继可用性（多实例快照同步 + 客户端列表切换 + 重启自愈），不产生任何 NAT 类别样本 —— 见 [nat-matrix.md §16](tests/spikes/s6/nat-matrix.md)。同轮纠正一条环境结论：`112.5.32.34:54022` 映射**一直是通的**（此前按端口抓包漏看，因为 NAT 同时改写源端口）。**W3.9 又纠正一条**：`57100→7100` **实测并未打通**（同 CPE 的 54022 27 ms 可达、222 上按对端公网 IP 抓包 0 包，ufw/iptables/监听均已排除），故 W3.9 无跨公网时延数字。**W3.10 出口类别清单**：家宽出口 `112.5.60.4` 实测 **port-restricted-cone**、`112.5.32.34` 为 **symmetric**（W3.7 APDM），现有两出口 = **2/5 类**，缺 `full-cone` / `restricted-cone` / `cgnat`；清单不含可达性数字，不能折算成矩阵覆盖。**W3.11 口径变更**：R-04 取消数值承诺，S6 改为逐类查清「直连行不行、不行时中继是否送达」，判据四条定性要求（见 §19 / ADR-012）；「direct 0/30」不再是「没达标」，而必须逐类说明为什么 |
| 验收骨架 | [T0](tests/e2e/t0/README.md) 21/21 real · [T1](tests/e2e/t1/README.md) 11/11 real · [T2](tests/e2e/t2/README.md) 10/10 本地预检（真实中继服务端、无真实 NAT，非 S6 证据）；`--self-test` 恒 `evidence_valid=false` |
| W3.5 实验工具 | [条件探针](tests/spikes/probe_conditions.py) · [S3 双栈双机](tests/spikes/s3/m1_dualstack_integration.py) · [S6 NAT 行为分类](tests/spikes/s6/nat_behavior.py) · [S6 矩阵协议](tests/spikes/s6/nat_matrix.py) · [**S6 NAT 实验室**](tests/spikes/s6/nat_lab/README.md)（`setup_nat_lab.sh` / `local_stun.py` / `run_matrix_lab.py` / `link_physical.sh` / `run_cross_host.py` / **`run_real_nat.py`**（真实 NAT 档位） / **`relay_probe.py`**（中继帧协议探针，`pair` / `cross` / `reap` / `list`，支持 `--repeat` 连发与 `--restart` 重启对端做快照自愈验证））；全部只读/低速/私有环境，脚本不写 `spikes.json`、不自动置 pass |
| M0 / 门禁 | [G0 条件冻结](docs/06-plan/G0-REVIEW.md)：S1/S2/S4/S7/S8 `pass`，S3/S5/S6 `not_run`；机读进度 `tests/spikes/spikes.json` 的 `m1_progress` / `m2_progress` |
| 文档入口 | **[docs/index.html](docs/index.html)**（浏览器打开） / [docs/README.md](docs/README.md)（Markdown 导航） |
| 原始想法 | [docs/09-archive/chat.md](docs/09-archive/chat.md) |
| 历史基线 | [docs/09-archive/ColdRevive-项目开发文档-v2.0.md](docs/09-archive/ColdRevive-项目开发文档-v2.0.md)（已废止，勿作为依据） |
| 技术栈 | Rust + Tokio / SQLite / axum（详见 [技术选型](docs/02-architecture/TECH-STACK.md)） |

## 快速导航

| 我想… | 去这里 |
| :--- | :--- |
| 了解项目要解决什么问题 | [PRD 产品需求说明书](docs/01-product/PRD.md) |
| 查看功能/非功能需求与验收指标 | [SRS 软件需求规格](docs/01-product/SRS.md) |
| 理解整体架构 | [ARCH 总体架构设计](docs/02-architecture/ARCH.md) |
| 了解模块怎么实现 | [LLD 模块详细设计](docs/03-design/LLD.md) |
| 查看协议与接口定义 | [PROTO 协议规范](docs/04-protocol/PROTO.md) · [API 接口规范](docs/04-protocol/API.md) |
| 看开发排期 | [ROADMAP 开发计划](docs/06-plan/ROADMAP.md) |
| 看风险与待决问题 | [RISK 风险登记册](docs/05-quality/RISK.md) · [OPEN-QUESTIONS](docs/06-plan/OPEN-QUESTIONS.md) |
| 查术语 / 讨论出处 | [GLOSSARY](docs/08-reference/GLOSSARY.md) · [DISCUSSION-MAP](docs/08-reference/DISCUSSION-MAP.md) |

## 目录结构

```
ColdRevive/
├── README.md                  本文件：项目入口与导航
├── CHANGELOG.md               文档与项目变更记录
├── .gitignore
├── docs/                      全部工程文档（Docs as Code）
│   ├── index.html             文档索引门户（自包含单文件，离线可读）
│   ├── README.md              文档体系、编号规则、阅读路径
│   ├── 01-product/            产品与需求   PRD / SRS
│   ├── 02-architecture/       架构与决策   ARCH / TECH-STACK / adr/
│   ├── 03-design/             详细设计     LLD / FLOWS / DATA
│   ├── 04-protocol/           协议与接口   PROTO / API / CONFIG
│   ├── 05-quality/            质量保障     TEST / RISK
│   ├── 06-plan/               计划跟踪     ROADMAP / OPEN-QUESTIONS
│   ├── 07-operations/         发布运维     OPS
│   ├── 08-reference/          参考资料     GLOSSARY / DISCUSSION-MAP / REFERENCES
│   └── 09-archive/            归档基线     chat.md / v2.0 合并文档
├── Cargo.toml / Cargo.lock    Workspace 根清单（依赖按层，见 ADR-015）
├── src/crates/                17 个 crate（M1 七个真实实现 + M2/M3/M4 骨架；cr-nat 已落地穿透栈，余 M2 crate 仍骨架）
├── src/bins/                  coldrevived / coldrevive-cli / coldrevive-agent / coldrevive-nat / coldrevive-relay（W3.4 生产中继服务端）/ coldrevive-dht（W3.2 自检与探测）
├── tests/                     测试（unit / integration / e2e / fixtures / spikes）
│   ├── e2e/t0/                T0 双节点验收（21 条用例、CLI 契约、观测层、断言引擎、JSON 报告）
│   ├── e2e/t1/                T1 双机 + 真实 NAT 验收（10 用例 / 40 断言、可达性探针、双机启动契约）
│   ├── fixtures/t0/           T0 确定性样本与 PROTO v1 golden 报文
│   └── spikes/                M0 Spike 实验包（判据、状态登记、就绪度检查；s5/utp 为真实 uTP 栈量测）
├── scripts/                   工具脚本（build_docs.py 等）
├── config/                    配置样例（coldrevive.example.toml）
├── .github/workflows/         CI（文档构建与校验）
└── .workbuddy/memory/         工作区记忆（勿删）
```

> [G0 条件冻结](docs/06-plan/G0-REVIEW.md) 已于 2026-11-13 生效；**G1 于 2026-11-14 通过**
> （T0 21/21 + T1 real 11/11 均 `evidence_valid=true`），**M2 门禁解除并已开工**。
> 当前 M2/W3.3 已完成 cr-nat 接入（T2 本地预检 10/10，非 S6 证据）；
> **S3 / S5 / S6 仍 `not_run`，G2 未宣称达标**。
> **2026-11-17 口径变更（[ADR-016](docs/02-architecture/adr/ADR-016-s6-emule-like-reach.md)）**：
> S6 阶段性改为 **eMule 式可达语义**——不再要求穷举 4 类 NAT + CGNAT，类别覆盖降为描述性清单、
> 不再是 pass 前提；改为要求真实 NAT 设备上 HighID（`direct` 可达）与 LowID（`direct` 失败但
> `relay` 送达）两半各有真实证据，且节点自报声明与实测一致。**真实设备 / 两台物理主机 /
> 每路径 30 次真实 `WAKEUP_ACK` / 无数值承诺 / 不自动 pass 一条不松**；满足后 G2 只能写
> 「按 eMule 语义结案，带已知限制」。W3.7 的真实 symmetric 报告已覆盖 LowID 一半，
> `112.5.60.4` 的 HighID 一半待用现有两台物理主机补跑，**无需新增出口**。
> 方法见 [nat-matrix.md §23](tests/spikes/s6/nat-matrix.md)；复审时点 M4 前或 ≥4 个真实出口可用时。
> `tests/e2e/t0/` / `t1/` / `t2/` 的编排层是纯 Python（不依赖 Rust 工具链），真实观测由 `coldrevive-cli` 提供。

## 文档工作流

所有 `.md` 是**唯一事实来源**；`.html` 为构建产物，请勿直接编辑。

```bash
# 由 Markdown 生成各文档的 HTML 页面，并刷新 index.html 的内嵌数据
python3 scripts/build_docs.py

# 校验文档链接、front matter 与索引页数据块
python3 scripts/build_docs.py --check
```

浏览器直接打开 `docs/index.html` 即可离线浏览全套文档：全部正文与 Markdown 源码已内嵌在该页面内，点击「阅读」在页内阅读器打开，不依赖同目录的其他 HTML 文件，因此单文件分发或在预览沙箱中打开也能正常阅读。修改任一 `.md` 后重新运行构建脚本。

## T0 验收：自检与真实链路

自检不依赖 Rust 工具链，仅用 Python 3.9+ 标准库：

```bash
python3 tests/e2e/t0/t0_accept.py --list        # 21 条用例与判据出处
python3 tests/e2e/t0/t0_accept.py --self-test   # 框架自检
python3 tests/e2e/t0/t0_harness.py --self-check # 环境与确定性样本自检
python3 tests/e2e/t0/t0_cli_selftest.py         # 观测层自检（不合成值 + 环境级防呆）
python3 tests/e2e/t1/t1_accept.py --list        # T1 的 10 条用例 / 40 条断言
python3 tests/e2e/t1/t1_accept.py --self-test   # T1 编排与断言引擎自检（evidence_valid=false）
python3 tests/e2e/t1/t1_netprobe.py             # 本机跨 IP UDP 可达性真实探测
python3 tests/e2e/t2/t2_nat_path.py --self-test # T2 mock STUN / 中继 wire 自检
python3 tests/e2e/t2/t2_nat_path.py --mode local # M2 跨 NAT 接线本地预检（evidence_valid=false）
python3 tests/spikes/m0_preflight.py             # 就绪度检查（退出码 2 = 有阻塞）
cargo test --workspace --offline                # 168 个 Rust 单测（协议面 std-only；引擎用 librqbit）
```

真实链路（M1 二进制 + 第三方 BT 下载器，Windows 示例）：

```bash
cargo build --workspace --offline
export COLDREVIVE_DAEMON="C:/path/to/ColdRevive/target/debug/coldrevived.exe"
export COLDREVIVE_CLI="C:/path/to/ColdRevive/target/debug/coldrevive-cli.exe"
export T0_DOWNLOADER="C:/Program Files/Rayburst/aria2-next.exe"
python3 tests/e2e/t0/t0_accept.py --mode real
```

2026-11-14 复跑回归：**21/21 PASS、`evidence_valid=true`**（含第三方 aria2-next 3.1.0 标准 BT 下载与逐片 SHA-1 校验）。
未设置三个环境变量时 `--mode real` 仍 `BLOCKED` / 退出码 2（CI 依赖此防呆）。
v2.4.47 修正了 T0 观测客户端对 `download` 的 10 s 超时：daemon 内部上限为 120 s，现按条用例放宽到 190 s，避免新版下载器稍慢即被误判 `BLOCKED`。

T1 单机预检（同样需要三个二进制环境变量）：

```bash
python3 tests/e2e/t1/t1_accept.py --lan-precheck --address-mode aliases  # 两个回环别名，验证按节点 host 寻址
python3 tests/e2e/t1/t1_accept.py --lan-precheck --address-mode nic      # 真实物理网卡（非回环）
```

历史单机预检为各 **10 PASS / 1 SKIP**、`evidence_valid=false`；
**2026-11-14 已用两台真实物理主机跑通 `--mode real`：11/11 PASS、`evidence_valid=true`，与 T0 共同构成 G1 通过证据**。
后续复跑仍需两台物理主机与 B 端预启动守护进程，命令见 [T1 README §4](tests/e2e/t1/README.md)。

`--self-test` 的 PASS 只证明脚本编排与断言引擎正确，**不是 G1 门禁证据**（`evidence_valid` 恒为 `false`）；
T2 的 10/10 PASS 同理，只证明 M2 接线，不是 S6 四类 NAT 实测。
完整说明见 [T0 骨架文档](tests/e2e/t0/README.md) · [T2 预检文档](tests/e2e/t2/README.md) · [G0-REVIEW §9](docs/06-plan/G0-REVIEW.md)。
`m0_preflight.py` 只检查实验条件，不执行 Spike、不接触公共 KAD、不产出技术结论；见 [M0 实验包](tests/spikes/README.md)。

## 合规边界

本项目是通用 P2P 传输与索引协议的技术实现，不内置任何资源站点、索引库或默认搜索源，不提供绕过版权保护或匿名化的能力，用户对其分享与下载内容的合法性独立承担责任。详见 [OPS §合规与伦理边界](docs/07-operations/OPS.md)。
