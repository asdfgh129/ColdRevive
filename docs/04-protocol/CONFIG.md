---
doc_id: DOC-11
title: 配置规范（CONFIG）
category: 协议
version: 2.3
status: 待评审
owner: 内核组 / 运维组
updated: 2026-11-20
---

# 配置规范（CONFIG）

> 编号 DOC-11 · 版本 2.3 · 状态 待评审 · 责任人 内核组 / 运维组 · 更新 2026-11-20
> 可执行样例：[config/coldrevive.example.toml](../../config/coldrevive.example.toml)。所有默认值遵循「最小暴露」原则。
> **v2.1**：`qos.background_upload_ratio` 默认由 `0.2` 回调为 `0.02`（S5 实测标定，见 §2 注记）。
> **v2.3**：补 FR-14 空闲自动休眠（`engine.idle_sleep_secs` / `scheduler.idle_sleep_minutes`，
> **opt-in、0=关闭、秒优先**）与 FR-11 `engine.guard_handshake`，并加 `/health.idle` 观测口径。
> `scheduler.idle_sleep_minutes` 默认值由 `10` 改为 `0`——休眠是 opt-in，默认行为不变。

## 1. 完整配置

```toml
[shared]
dirs = ["~/ColdRevive/Shared"]
min_file_size_mb = 1
exclude = ["*.part", "*.crdownload"]

[indexer]
piece_size = "auto"              # auto | 262144 | 524288 | 1048576
torrent_gen_concurrency = 2
max_keywords_per_file = 8

[network]
http_port = 16800
wake_udp_port = 16801
bt_port = 16802
enable_upnp = true
enable_ipv6 = true
bind = "127.0.0.1"              # 远程管理需显式改为 0.0.0.0

[kad]
namespace = "ColdRevive_V1"
bootstrap_mode = "public"       # public | private | hybrid
publish_ttl_sec = 1800
publish_rate = 20

[scheduler]
max_active_seeds = 50
idle_sleep_minutes = 0           # FR-14 opt-in，0 = 关闭；秒级项见 engine.idle_sleep_secs
wake_session_ttl_sec = 7200
memory_soft_limit_mb = 100
memory_hard_limit_mb = 200

[qos]
background_upload_ratio = 0.02
max_connections_total = 200
prefer_utp = true

[security]
rpc_secret = ""                 # 为空则仅允许回环
wake_rate_per_ip = 5
max_wake_concurrency = 8
cors_allowlist = []
```

## 2. 参数字典与约束

| 段 | 参数 | 默认 | 约束 / 说明 |
| :--- | :--- | :--- | :--- |
| shared | `dirs` | 空（示例值供参考） | **必须用户显式指定**；未配置时不发布任何索引 |
| shared | `min_file_size_mb` | 1 | 小于阈值的文件跳过 |
| shared | `exclude` | `*.part`, `*.crdownload` | 支持 glob；同时强制排除隐藏文件与符号链接逃逸 |
| indexer | `piece_size` | `auto` | 按文件体积自适应 512 KB～4 MB |
| indexer | `torrent_gen_concurrency` | 2 | Idle 优先级；机械硬盘建议 1 |
| indexer | `max_keywords_per_file` | 8 | 控制 KAD 发布量，见 [FLOWS §2](../03-design/FLOWS.md) |
| network | `bind` | `127.0.0.1` | 改为非回环时必须同时配置 `rpc_secret` |
| network | `http_port` / `wake_udp_port` / `bt_port` | 16800 / 16801 / 16802 | 与 [ARCH §3.2](../02-architecture/ARCH.md) 一致 |
| kad | `namespace` | `ColdRevive_V1` | 与 [PROTO §1](PROTO.md) 的 `NAMESPACE_PREFIX` 一致 |
| kad | `bootstrap_mode` | `public` | 由 [ADR-001](../02-architecture/adr/ADR-001-kad-network.md) 决定，M0 后可能改默认 |
| kad | `publish_ttl_sec` / `publish_rate` | 1800 / 20 | 续期周期与发布限速 |
| scheduler | `max_active_seeds` | 50 | LRU 活跃池硬上限 |
| scheduler | `idle_sleep_minutes` | 0（关闭） | **FR-14 opt-in** 空闲自动休眠阈值，单位分钟；`0` = 关闭。秒级优先项见 §2.1 `engine.idle_sleep_secs`，两者同时出现时**秒优先** |
| scheduler | `wake_session_ttl_sec` | 7200 | 唤醒会话硬顶 |
| scheduler | `memory_soft_limit_mb` / `memory_hard_limit_mb` | 100 / 200 | 越限触发降级链 |
| qos | `background_upload_ratio` | **0.02** | 后台做种上行占比。S5 本地模型量测：0.2 → 前台劣化 20.12%、0.05 → 6.19%、0.02 → 4.03%；**真实 librqbit-utp 0.7.0 栈量测**：0.2 → 14.82%、0.05 → 4.31%、0.02 → 2.70%（NFR-03 ≤5%）。默认值按实测回调，**不代表 S5 判 pass**，仍须 M1/G3 在 `tc/netem` / 跨主机真实拥塞链路实测回填。见 [G0-REVIEW §5](../06-plan/G0-REVIEW.md) |
| qos | `max_connections_total` | 200 | 全局连接数上限 |
| qos | `prefer_utp` | true | 优先用 uTP 传输。**2026-11-13 源码核实**：`librqbit-utp 0.7.0` 仅实现 CUBIC、无 LEDBAT（`// TODO: LEDBAT congestion control`），真实栈实测也未见退让；本项**不提供低优先级退让保证**，NFR-03 ≤5% 目前只由 `background_upload_ratio` 兜底，见 [S5 README §9](../../tests/spikes/s5/README.md) 与 [RISK R-07](../05-quality/RISK.md) |
| security | `rpc_secret` | 空 | 为空时强制仅回环监听 |
| security | `wake_rate_per_ip` / `max_wake_concurrency` | 5 / 8 | 唤醒限流 |
| security | `cors_allowlist` | 空 | 显式白名单，不做通配放行 |

### 2.1 运行时 JSON 段（M2 已接线，与上表并行）

`coldrevived --config` 实际读取的是 **JSON**（上表是 TOML 目标形态）。以下两段在 JSON 里，
缺省即关闭——`nat` / `engine` 段不出现时，T0/T1 行为逐字节不变。

| 段 | 参数 | 默认 | 约束 / 说明 |
| :--- | :--- | :--- | :--- |
| nat | `enabled` | false | 关闭时唤醒走 M1 单播、tracker 一律通告 `nodes.<id>.host` |
| nat | `stun_servers` | 空 | `"host:port"` 列表；空时用内置列表 |
| nat | `gateway` / `upnp_location` | 空 | 映射协议探测目标；留空则用默认发现 |
| nat | `relay` / `relay_key` | 空 | 中继地址（逗号分隔可多端点）/ PSK（64 位 hex 或 `file:<路径>`） |
| nat | `relay_udp` | 空 | **同一个中继的 UDP 承载**端点（逗号分隔可多端点，如 `119.29.150.254:7101`）。中继所在机器的入向 TCP 被掐时（[ADR-020](../02-architecture/adr/ADR-020-relay-udp-transport.md) §3.1），只填这一项即可：订阅与唤醒都走 UDP。与 `relay` 可同时配，两者独立工作、各自按列表切换。走 UDP 订阅的节点在中继看来 `declared=false`（该承载没有声明帧），只影响「先查再打洞」的优化，不影响送达 |
| nat | `bt_external` | 空 | 运维在 CPE 上**真实建好**的 TCP 映射 `公网IP:端口` → 本机 `bt_port`。只有配置了它，tracker 才会向公网请求方通告 peer；未配置即返回空 peer 列表（绝不发 RFC1918）。私网请求方同 /24 时回本机私网地址，**跨网段时改回这条公网映射**（`internal_port` 必须等于 `bt_port`，v2.4.130），否则对方拿到的是自己网段里不存在的地址。**µTP 开着时还需要一条同号的 UDP 规则**——TCP 映射对 UDP 无效，缺它外部客户端会先等 µTP 超时再回落 |
| nat | `http_external` | 空 | 同上，对应 HTTP/tracker 端口。配置后伴随种子写 BEP 12 `announce-list`（LAN + 公网两条，InfoHash 不变），唤醒 ACK 里给公网请求者的取种 URL 也换成这条 |
| nat | `utp` | `true` | BT 监听端口上是否同时开 µTP（BEP 29，UDP 同端口）。关掉只留 TCP；标准客户端默认先试 µTP，关了它们要先等回落。不影响任何帧格式。**公网入向另需 CPE 上的 UDP 映射**（与 `bt_external` 那条 TCP 规则并列）；是否真的绑上以 `netstat -ano -p UDP` 的 PID 为准 |
| nat | `punch_route` | `false` | 启用「打洞优先 / 中继兜底」控制面（[ADR-022](../02-architecture/adr/ADR-022-punch-first-relay-fallback.md)）与 opt-in 的 TCP 中继 BT 数据面（[ADR-023](../02-architecture/adr/ADR-023-bt-over-relay.md)）。关闭时 `proxy_url` 保持 `None`、引擎行为不变；开启后建本地 SOCKS5 桥，唤醒成功即向对端发 `OPEN`、**收到 `OPENED` 才登记隧道**，失败如实回落直连。持有者侧同时在订阅连接上分流数据面帧、拨本地引擎并双向 splice，计数见 `/health.holder_data`（`opens/accepted/bytes_to_engine/bytes_from_engine`）；下载方侧 `/health.data_plane` 暴露 `proxy/served_registered/bytes_relayed/tunnels/tunnels_alive`（**`tunnels` 只是登记条数，泵死了它照样是 1；要看 `tunnels_alive`**）。CLI：`nat punch-route` / `nat punch-route-answer`。`punch_route.relay_data_plane` 写 `tcp-carrier-implemented`，UDP 承载另写 `relay_data_plane_udp="udp-carrier-implemented-rst-path-verified"`（[ADR-024](../02-architecture/adr/ADR-024-udp-bt-data-plane.md)：W61 已在 `119.29.150.254` 那条入向 TCP 被注入 RST 的路径上复现双 NAT 1 MiB；`path` 只指该路径，不外推所有 RST 部署）。持有方 `/health.holder_data.carrier` 随 opt-in 承载标 `relay-sub-tcp` / `relay-sub-udp`，不再写死 TCP |
| nat | `data_plane_carrier` | `tcp` | 中继 BT 数据面走哪种承载：`tcp`（默认，[ADR-023](../02-architecture/adr/ADR-023-bt-over-relay.md)）或 `udp`（[ADR-024](../02-architecture/adr/ADR-024-udp-bt-data-plane.md)，需同时配 `relay_udp`）。非法值回落 `tcp` 并告警。选 `udp` 时发起方在 UDP 中继会话上做 OPEN/OPENED（**未收到 `OPENED` 不登记隧道**），持有方在 `relay_udp` 订阅之外另起数据面服务；`/health.data_plane` 增加 `carrier`、`udp_tunnels`、`udp_tunnels_alive`。默认 `tcp` 保证 W58/W59/W60 的结论不变 |
| nat | `udp_cc_enabled` / `udp_cc_pacing` | `false` / `false` | ADR-024 阶段 3 的 UDP 可靠层拥塞控制，默认全关。开启后发送窗口从固定 32 段改为 AIMD（ACK 加性增长、丢失乘性减半且每 RTT 最多一次），RTO 由真实 RTT 推导；`pacing=true` 把新段摊到整个 RTT，重传仍立即发。非法整数在配置层报错，不静默夹到「以为开了」。`/health.data_plane.udp_cc_*` 是机制快照，**不是跨公网公平性结论** |
| nat | `udp_cc_cwnd_init` / `udp_cc_beta_x100` | `4` / `50` | 初始拥塞窗口（1..32）与乘性减小系数（10..90，除以 100）。只在 `udp_cc_enabled=true` 时生效；越界直接报错 |
| nat | `udp_per_session_rate` / `udp_per_session_burst` | `0` / `0`（关） | 这是**中继进程**的启动参数（`coldrevive-relay --udp-per-session-rate/--udp-per-session-burst`），不是 daemon 配置。每会话字节桶与全局 `data_byte_rate` 叠加；v2.4.118 起超会话预算的 DATA 进入**有界整形队列**（每会话 64 帧、总 256 帧）等令牌，不再立刻拒收——立即拒收会让发送端按 RTO 退避，RTT 反馈环造成长期不公平。队列满才返回带原因的 `BandwidthLimited` STATUS。计数分开：`per_session_limited`（触发整形/拒绝）、`per_session_shaped`（入队）、`per_session_dropped`（队列满拒绝）。默认 0 = 不建桶、老行为逐字节不变 |
| nat | `relay_rendezvous` | 空 | **中继会合地址**（IPv4，只允许 RFC 5737 网段 `203.0.113.0/24`、`198.51.100.0/24`、`192.0.2.0/24`），必须与 `punch_route=true` 同时配置。没有入向映射时，持有者把它作为 `bt_external`（`kind=relay`）通告出去，对端引擎据此拨号、再由本机 SOCKS5 桥改道到中继隧道。它**不是端口映射**：不进 `is_explicit_port_mapping()`，也不作为公网入向证据；填真实 IP 或私网地址会在启动时直接报错（避免被 tracker/可达性声明读成「真的有公网入向」） |
| engine | `dir` | `<workdir>/downloads` | 引擎下载目录 |
| engine | `listen` | 空 | `ip:port`。留空且声明了任一 `*_external` 时，做种监听落 `0.0.0.0:bt_port`；否则绑 `bind` |
| engine | `max_active_seeds` | 空（= `cr-sched::MAX_ACTIVE_SEEDS`，当前 8） | 做种活跃池上限（[ADR-017](../02-architecture/adr/ADR-017-active-seed-pool.md)）。超过上限时逐出**最久未用**的任务并暂停它；容量 0 在配置层直接报错，不静默夹到 1。`/health` 的 `seeding_active` / `seeding_cap` / `seeding_evictions` 是这三个事实的观测口径 |
| engine | `idle_sleep_secs` | 空（= 关闭） | **FR-14 空闲自动休眠**：HOT 做种任务在「最后一次真实唤醒」之后空闲满这么多秒，就由唯一入口 `idle::sleep_hot_seeds` 暂停并切回 `COLD`（与 `sched::wake_seed` 对称，不再有第二个休眠入口）。`0` 或留空 = 关闭（默认）。也可写在 `engine.idle_sleep_minutes` 或顶层 `scheduler.idle_sleep_minutes`（按分钟，内部折算成秒），**秒项与小项同时出现时秒优先**（秒项为 `0` 也按秒项走 = 关闭）。**非法值不是静默关闭而是启动报错**（`engine.idle_sleep_secs 非法（需要非负整数秒）`）——避免「以为会休眠其实一直在供片」。休眠**不清索引**：影子索引与 `published_keys` 保留，所以 KAD 仍查得到 ——「查得到」不等于「还在做种」，要看 `/health.idle` 的 `sleeps` 与 `seeding_active` |
| engine | `guard_handshake` / `wake_on_handshake` | `false` | **FR-11 握手唤醒**（opt-in，两个键同义，任一为布尔 `true` 即启用；写字符串不生效）。开启后引擎退到回环内部口、`cr-guard` 占对外 `bt_port`，裸 BT 握手按 infohash 走 `sched::wake_seed` 唤醒，等引擎报 `HOT` 后再把已读的 68 字节补回并 splice；`/health.guard` 暴露 `handshakes/woken/missed`。**对外绑定**：只要配了 `nat.bt_external` 或 `nat.http_external`，代理一律绑 `0.0.0.0:bt_port`，不跟随 `engine.listen` 的 `127.0.0.1`（否则 DNAT 入向到不了代理）。**与打洞互斥**：代理长期占住 `bt_port`，同端口打洞 socket 无法再绑，需要打洞的节点别开。仅 TCP，µTP 不经 guard |
| engine | `idle_priority` | `true` | **NFR-02 第二分句**：引擎工作线程（librqbit 的 worker，线程名 `cr-engine-wrk`，造种/分片校验/下载都在上面）降到 Idle 优先级——Linux `nice 19` + `SCHED_IDLE`，Windows `THREAD_PRIORITY_IDLE`(-15)，macOS `PRIO_DARWIN_BG`（`/health` 报 `darwin_bg`；**darwin 分支已实现但本仓库无 mac 硬件实测**，别读成「macOS 已验证」）。线程名必须 ≤15 字符（Linux `comm` 会被静默截断到 15）。**按线程降，不按进程降**：cr-guard 的 68 B 握手、cr-data-plane 的 UDP、HTTP API 在别的线程上，唤醒 P95（NFR-04）不受影响。写成字符串或数字不生效（按默认 `true`）；要全速下载可显式 `false`。`/health.idle_priority` 报 `requested/applied/failed/unsupported`，其中 `applied` 是**读回校验通过**的线程数（darwin 上读回 `getpriority(PRIO_DARWIN_THREAD)`，`-1` 视为读不出、不算成功） |

`/health.idle` 是 FR-14 的观测口径：`enabled` / `threshold_secs` / `idle_secs` / `sleeps` /
`last_slept`。**`enabled=false` 时其余字段不承诺含义**（阈值未启用，`idle_secs` 仍随心跳增长）。
休眠是「省资源」，不是「退出网络」：任务回到 `COLD` 后只要再来一次真实唤醒就重新 `HOT`，
状态序列可查（`COLD → WAKING → HOT → COLD`）。

**这两项只是声明，不是证据**：守护进程无法自证映射存在。入向是否真的可用，
只能由外部主机实测（[`tests/e2e/w51d/public_inbound.py`](../../tests/e2e/w51d/public_inbound.py)）；
`/nat/public` 返回的 `inbound_verified` 恒为 `false`，`s6_status` 恒为 `not_run`。

### 2.2 P1 运行时 JSON 段（2026-11-22 接线，全部 opt-in）

以下字段与 §2.1 共用同一份 JSON 配置；不写时行为与旧版本逐字节一致。

| 段 | 参数 | 默认 | 约束 / 说明 |
| :--- | :--- | :--- | :--- |
| api | `rpc_secret` | 空 | 配置后 `/jsonrpc` 的**写方法**要求 `token:<secret>`（params）或 `Authorization` / `X-RPC-Secret` 头；读方法仍免费。`/health.api.rpc_secret` 只给 `enabled`，**永不回显密钥值** |
| api | `web_ui` / `websocket` | `false` | 开启后 `GET /` 提供自包含静态 UI（无 CDN），`GET /ws` 提供 RFC6455 状态推送。两者都排在既有端点之后、只匹配精确路径，不遮蔽 `/health` / `/cli` 等 |
| qos | `background_upload_ratio` | `0.02` | 通过 librqbit 的真实 `upload_bps` 硬上限落实；`cr-qos` 令牌桶负责记账与可观测。**比率按 `reference_link_bps` 折算，不是实测链路速率**，也不宣称 LEDBAT |
| qos | `reference_link_bps` | `100000000` | 比率→字节的名义参考带宽；只用于换算，不代表本机实测带宽 |
| nat | `auto_map` | `false` | 启动时调用真实 UPnP IGD / NAT-PMP 原语建映射；`/health.nat` 逐项记录成功/失败与外部地址。**路由器应答不等于入向可达证据** |
| dht | `enabled` / `bootstrap` | `false` / 空 | 开启后先做有界 `find_node` 发现，再对活跃种子执行真实 `get_peers → announce_peer → 回查`；`/health.dht` 报 verified/no-token/failed 与真实错误。公共节点不给 token 时如实记 `token_unavailable`，不宣称公网可发现 |
| dht | `timeout_ms` / `refresh_secs` / `min_interval_ms` | `2000` / `600` / `50` | 单请求超时、轮次间隔、请求间最小间隔。`min_interval_ms=0` 表示不刻意限速；前两项 0 或非整数回落缺省 |
| dht | `max_bootstraps` / `max_seeds` / `max_keys` | `8` / `50` / `512` | 每轮引导节点与种子数上限；`max_keys` 是本地 DHT 节点保存 peer 表的 infohash 总数上限，超出淘汰最旧 key。0 回落缺省，不解释成「无限」 |
| dht | `max_announces_per_cycle` / `rss_soft_skip_mb` | `16` / `0` | 每轮宣告尝试硬上限（每次内部仍走真实 token 门）与 RSS 软跳过阈值（MB，0=关）。读不到 RSS 时不参与判定，**绝不当 0** |

`GET /openapi.json` 始终可用，列出真实 REST 路径与 JSON-RPC 方法（含 `mutating` 标记）。
跨协议补源与信誉/激励仍是 `not_implemented` 状态接口，不返回伪造来源或分数。

## 3. 校验规则

启动时校验，失败则拒绝启动并给出明确错误：

1. `bind != 127.0.0.1` 且 `rpc_secret == ""` → 拒绝。
2. `memory_hard_limit_mb <= memory_soft_limit_mb` → 拒绝。
3. `dirs` 中存在不存在/不可读/符号链接逃逸的路径 → 警告并跳过该目录。
4. 端口冲突或 `bootstrap_mode` 取值非法 → 拒绝。
5. `namespace` 与 `PROTOCOL_VERSION` 不匹配 → 警告（允许升级过渡期）。

## 4. 热更新与优先级

- **配置来源优先级**：CLI 参数 > 环境变量 > `coldrevive.toml` > 内置默认值。
- 支持运行时热更新的字段：`qos.*`、`scheduler.*`（除内存硬限）、`security.wake_rate_per_ip`、`kad.publish_rate`。
- 需重启生效的字段：`network.*`、`kad.namespace`、`shared.dirs`（目录变更走 F1 增量处理，可不重启）。
- 运行时值写回 [DATA `config` 表](../03-design/DATA.md)，重启时以文件为准。

## 5. 调优建议

| 场景 | 建议 |
| :--- | :--- |
| 机械硬盘 | `torrent_gen_concurrency = 1`，避免造种抢占 I/O |
| SSD / 高性能主机 | 可提到 4 |
| 上行带宽小 | `background_upload_ratio = 0.02`，保持 `prefer_utp = true`（但不要把它当作退让手段，见上表）；不得回调到 0.05 / 0.1 / 0.2（本地模型 6.19% / 20.12%、真实 uTP 栈 14.82%） |
| > 1 万文件共享 | 调大 `max_active_seeds` 前先做 [TEST 内存压测](../05-quality/TEST.md) |
| 私有小团体 | `bootstrap_mode = "private"`，见 [ADR-001](../02-architecture/adr/ADR-001-kad-network.md) |
| NAS / Docker | `bind = "0.0.0.0"` + 强 `rpc_secret` + 反向代理 TLS，见 [OPS §1](../07-operations/OPS.md) |

## 相关文档

- [config/coldrevive.example.toml](../../config/coldrevive.example.toml)
- [DOC-03 ARCH 端口视图](../02-architecture/ARCH.md)
- [DOC-06 LLD §10 阈值总表](../03-design/LLD.md)
- [DOC-09 PROTO 命名空间](PROTO.md)
- [DOC-16 OPS 发布运维](../07-operations/OPS.md)
