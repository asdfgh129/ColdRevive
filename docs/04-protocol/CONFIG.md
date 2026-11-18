---
doc_id: DOC-11
title: 配置规范（CONFIG）
category: 协议
version: 2.2
status: 待评审
owner: 内核组 / 运维组
updated: 2026-11-13
---

# 配置规范（CONFIG）

> 编号 DOC-11 · 版本 2.2 · 状态 待评审 · 责任人 内核组 / 运维组 · 更新 2026-11-13
> 可执行样例：[config/coldrevive.example.toml](../../config/coldrevive.example.toml)。所有默认值遵循「最小暴露」原则。
> **v2.1**：`qos.background_upload_ratio` 默认由 `0.2` 回调为 `0.02`（S5 实测标定，见 §2 注记）。

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
idle_sleep_minutes = 10
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
| scheduler | `idle_sleep_minutes` | 10 | 无活动自动休眠 |
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
| nat | `relay_udp` | 空 | **同一个中继的 UDP 承载**端点（逗号分隔可多端点，如 `119.29.150.254:7101`）。中继所在机器的入向 TCP 被掐时（[ADR-020](../../02-architecture/adr/ADR-020-relay-udp-transport.md) §3.1），只填这一项即可：订阅与唤醒都走 UDP。与 `relay` 可同时配，两者独立工作、各自按列表切换。走 UDP 订阅的节点在中继看来 `declared=false`（该承载没有声明帧），只影响「先查再打洞」的优化，不影响送达 |
| nat | `bt_external` | 空 | 运维在 CPE 上**真实建好**的 TCP 映射 `公网IP:端口` → 本机 `bt_port`。只有配置了它，tracker 才会向公网请求方通告 peer；未配置即返回空 peer 列表（绝不发 RFC1918）。**µTP 开着时还需要一条同号的 UDP 规则**——TCP 映射对 UDP 无效，缺它外部客户端会先等 µTP 超时再回落 |
| nat | `http_external` | 空 | 同上，对应 HTTP/tracker 端口。配置后伴随种子写 BEP 12 `announce-list`（LAN + 公网两条，InfoHash 不变），唤醒 ACK 里给公网请求者的取种 URL 也换成这条 |
| nat | `utp` | `true` | BT 监听端口上是否同时开 µTP（BEP 29，UDP 同端口）。关掉只留 TCP；标准客户端默认先试 µTP，关了它们要先等回落。不影响任何帧格式。**公网入向另需 CPE 上的 UDP 映射**（与 `bt_external` 那条 TCP 规则并列）；是否真的绑上以 `netstat -ano -p UDP` 的 PID 为准 |
| engine | `dir` | `<workdir>/downloads` | 引擎下载目录 |
| engine | `listen` | 空 | `ip:port`。留空且声明了任一 `*_external` 时，做种监听落 `0.0.0.0:bt_port`；否则绑 `bind` |
| engine | `max_active_seeds` | 空（= `cr-sched::MAX_ACTIVE_SEEDS`，当前 8） | 做种活跃池上限（[ADR-017](../02-architecture/adr/ADR-017-active-seed-pool.md)）。超过上限时逐出**最久未用**的任务并暂停它；容量 0 在配置层直接报错，不静默夹到 1。`/health` 的 `seeding_active` / `seeding_cap` / `seeding_evictions` 是这三个事实的观测口径 |

**这两项只是声明，不是证据**：守护进程无法自证映射存在。入向是否真的可用，
只能由外部主机实测（[`tests/e2e/w51d/public_inbound.py`](../../tests/e2e/w51d/public_inbound.py)）；
`/nat/public` 返回的 `inbound_verified` 恒为 `false`，`s6_status` 恒为 `not_run`。

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
