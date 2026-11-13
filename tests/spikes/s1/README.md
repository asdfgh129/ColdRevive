# S1 最小 Demo：BT 引擎能力探测（`tests/spikes/s1/`）

> 独立 Cargo 工程，**不在项目根创建 `Cargo.toml`、不修改 `src/`**（M0/G0 门禁要求）。
> 状态：**S1 四条判据已全部有 `verified` 证据**（C1/C2 由 `probe`、C3/C4 由 `two-peer`），
> 已于 2026-11-11 由实验执行人依 two-peer 4/4 verified 证据手工回填 **`S1.status = pass`**（见 `spikes.json` 与 [ADR-002](../../../docs/02-architecture/adr/ADR-002-bt-engine.md)）。
> 补充项：第三方 BT 客户端互操作一轮（G3 兼容性矩阵素材）**已于 2026-11-11 以 Motrix Next（aria2 1.36.0）执行并通过**——rqbit `cross-lee` 从第三方 seeder 拉取 2 MiB，128/128 片 SHA-1 全部校验通过；S1 维持 `pass`，详见 §7 与 `spikes.json` 的 `cross_client_round`。

## 1. 目的

验证 [TECH-STACK §2](../../../docs/02-architecture/TECH-STACK.md) 的 S1 假设：rqbit / librqbit 是否支持
按需实例化、Pause-Resume、握手拦截与分片校验。判据来源为 `tests/spikes/spikes.json` 的 `S1.criteria`。

两个子命令分工：

| 子命令 | 作用 | 判据覆盖 | 报告 |
| :--- | :--- | :--- | :--- |
| `probe`（默认，2026-11-09 起） | 单 Session 回环预演：握手拦截 → 按需实例化 → Pause/Resume → 逐片 SHA-1 | C1/C2 `verified`；C3/C4 仅 `partial` | `reports/s1-probe-<unix-ts>.json` |
| `two-peer`（2026-11-11 新增） | 两个独立 Session 的真实 BT Peer Wire 分片传输 + 传输中途暂停/恢复 | C3/C4 `verified`；C1/C2 引用 probe 报告 | `reports/s1-two-peer-<unix-ts>.json` |
| `cross-seed`（2026-11-11 新增） | rqbit 作 **seeder**，写出真实 `.torrent` 到磁盘，监听 `bind` 地址，等待第三方/跨主机 leecher 连入拉取全部分片 | G3 互操作素材（不覆盖 S1 判据） | `reports/s1-cross-seed-<unix-ts>.json` |
| `cross-lee`（2026-11-11 新增） | rqbit 作 **leecher**，按 `--torrent` 打开第三方种子、直连 `--peer` 指定地址（第三方/跨主机 seeder），逐片校验 | G3 互操作素材（不覆盖 S1 判据） | `reports/s1-cross-lee-<unix-ts>.json` |

## 2. 运行

```bash
cd tests/spikes/s1
cargo run -- probe       # 历史单 Session 预演（不带参数时默认执行）
cargo run -- two-peer    # 双 peer 真实分片传输 + 中途暂停

# ---- 第三方 BT 客户端 / 跨主机一轮（G3 素材，S1 已 pass 不涉及回滚）----
# 场景 A：rqbit 作 seeder，第三方客户端（或另一台机器的 rqbit leecher）来拉
cargo run -- cross-seed --bind 127.0.0.1:0            # 本机第三方客户端
cargo run -- cross-seed --bind 0.0.0.0:0 --advertise <本机LAN_IP>   # 跨主机
#   打印 torrent_path / listen_addr / info_hash / sample_sha1；第三方客户端打开 .torrent 并 manual-peer 连到该地址即可

# 场景 B：第三方客户端作 seeder，rqbit 作 leecher
cargo run -- cross-lee --torrent /path/to/thirdparty.torrent --peer <第三方客户端IP:端口>
```

报告落在 `reports/`，运行数据在 `.run/`，两者均不纳入版本控制。
`two-peer` 每次会清掉 `.run/two-peer/`，避免 leecher 目录残留完整文件把「复制」误认成「传输」。

**网络边界**：全程 `127.0.0.1` 回环 TCP；DHT 置 `None`、tracker 与 LSD 全部关闭、
无公网报文。样本由固定种子 `20261109` 确定性生成（probe 256 KiB / two-peer 1 MiB），无版权内容。
`two-peer` 中 seeder 仅入站监听、leecher 仅主动出站，互为独立进程内 Session、独立输出目录。

## 3. `probe` 观测了什么（历史基线）

| 步骤 | 做法 | 对应判据 |
| :--- | :--- | :--- |
| 伴随种子 | 按 bencode 生成单文件 `.torrent`，`piece length=16 KiB`，逐片 SHA-1 写入 `pieces`；InfoHash = SHA-1(info 字典原始字节) | S1-C4 的数据基础 |
| 握手拦截 | 本地 TCP 回环，请求方发送标准 68 字节 BT Handshake；持有者 `read_exact` 后解析 `pstrlen / protocol / reserved / info_hash / peer_id` | S1-C1 |
| 按需实例化 | 用握手里的 `info_hash` 查「InfoHash → 伴随种子」索引，读出种子字节，交给 `AddTorrent::TorrentFileBytes` 并以 `AddTorrentOptions { paused: true }` 实例化 | S1-C2 |
| Pause / Resume | `Session::unpause` → `pause` → `unpause`，每步读 `is_paused()` 与 `stats().progress_bytes` | S1-C3（仅状态切换，非传输中途） |
| 分片校验 | 重新读磁盘样本，按 16 KiB 分块计算 SHA-1，与伴随种子 `pieces` 逐条比对 | S1-C4 的校验半项 |

## 4. `two-peer` 补齐了什么（2026-11-11）

依据 librqbit 9.1.0 源码核实的 API：`SessionOptions.listen` / `ConnectionOptions` /
`AddTorrentOptions.initial_peers` / `AddTorrentOptions.ratelimits` / `ManagedTorrent::wait_until_completed`。

| 步骤 | 做法 | 证据字段 |
| :--- | :--- | :--- |
| 建 seeder | seeder 目录预置完整样本，`ListenerMode::TcpOnly` 绑 `127.0.0.1:0`，上传限速 256 KiB/s 留出可观测窗口 | `setup.seeder_listen_addr`、`setup.seeder_upload_bps` |
| 建 leecher | **空目录**，`paused: true` 实例化，`initial_peers = [seeder_addr]`；DHT/tracker/LSD 全禁 | `setup.leecher_started_empty`、`observations.start` |
| 捕获传输中途 | `unpause` 后轮询 `progress_bytes`，命中 `0 < progress < total` 即记录（实测 262144 B = 16 片，30 ms） | `observations.in_flight` |
| 中途暂停 | 立即 `Session::pause`，停留 900 ms 再读一次，确认 `is_paused=true` 且位图不前进 | `observations.at_pause` / `after_pause_wait_900ms` |
| 恢复并跑完 | `Session::unpause` → `wait_until_completed`（90 s 上限），`finished=true`、`progress=total` | `observations.final`、`completion` |
| 双端计数器 | seeder `uploaded_bytes`、leecher `fetched_bytes` / `downloaded_and_checked_pieces` 均取自 rqbit 自身统计 | `observations.seeder_live` / `leecher_live` |
| 逐片校验 | 读 leecher 落盘文件，64 片逐片 SHA-1 与种子 `pieces` 比对，并与确定性样本逐字节相等 | `observations.piece_sha1` |

**2026-11-11 连续 4 轮一致结果**：`in_flight = 262144/1048576`，pause 前后位图均为 262144，
恢复后 `finished=true` / 1048576，seeder 上传 1048576 B、leecher 抓取 786432 B（48 片，
`pause` 会重建 live state 故计数器从恢复点重算），64/64 片匹配、0 mismatch、内容逐字节一致。
`probe` 子命令回归无变化（C1/C2 `verified`、C3/C4 仍为历史 `partial`）。

## 5. 证据边界

- **rqbit 9.1.0 未暴露公开握手拦截钩子**：`SessionOptions` 无回调字段，`session.rs::task_listener`
  内部 `read_handshake()` 后直接按 `info_hash` 查已管理 torrent，不给调用方拦截点。
  C1 由**自实现的拦截层**完成，不是引擎能力；ColdRevive 的握手拦截器须自实现或在 rqbit 外加代理层。
  这一点已写入 [ADR-002](../../../docs/02-architecture/adr/ADR-002-bt-engine.md) 的选型门禁修正。
- `two-peer` 只证明**同一台机器上两个独立 librqbit Session 的标准 Peer Wire 传输**；
  没有第三方 BT 客户端、没有跨主机、没有公网、没有 NAT 穿透。第三方客户端互操作属于 G3 兼容性矩阵。
- 传输在 `127.0.0.1` 且上传被限速，不能作为性能/NFR 证据。
- 报告 `public_network_contacted=false`；`s1_status` 恒为 `not_run`，**脚本绝不自动置 `pass`**。
- `probe` 报告里 C3/C4 仍标 `partial` 是**历史事实**，不是当前缺口；两者由 `two-peer` 报告补齐，
  读判据覆盖必须**两份报告合起来看**。

## 6. 结论回填

按 [tests/spikes/README.md §6](../README.md) 的规则，由实验执行人依据证据路径手工修改
`spikes.json` 的 `S1.status`。判 `pass` 所需的三项材料现已齐备：

1. 真实双 peer 分片传输记录 —— `reports/s1-two-peer-*.json`（C3/C4 `verified`）；
2. 传输中途暂停/恢复的位图对比 —— 同报告 `observations.at_pause` / `after_pause_wait_900ms` / `final`；
3. 握手拦截方案的架构结论 —— 自实现拦截层，见 [ADR-002 §S1 判据补齐](../../../docs/02-architecture/adr/ADR-002-bt-engine.md)。

回填 `S1.status = pass` 后，[ADR-002](../../../docs/02-architecture/adr/ADR-002-bt-engine.md) 的「S1 通过 → 采用 `rqbit`」决策树即可定稿；
[ADR-001](../../../docs/02-architecture/adr/ADR-001-kad-network.md) 另有明文要求「定稿与
`public`/`private` 的签署由项目所有者执行」，须单独签署。

## 7. 第三方 BT 客户端 / 跨主机一轮（G3 素材，不回滚 S1）

`two-peer` 只证明同一主机上两个独立 librqbit Session 的标准 Peer Wire 传输，缺第三方客户端与跨主机维度。
为补齐 G3 兼容性矩阵，新增 `cross-seed` / `cross-lee` 两个子命令，由项目所有者于上午安装第三方 BT 客户端后执行。

**执行结果（2026-11-11）**：按用户指令采用本机已装的 **Motrix Next**（捆绑 `motrixnext-aria2c.exe` =
aria2 1.36.0，download-only 构建，可对完整文件做种）作第三方 seeder，走**反向路径（上述第 3 条）**：
`tests/spikes/s1/third-party/make_payload.py` 生成确定性 `payload.bin/payload.torrent`（128 片 × 16 KiB，
infohash `2b77301a18d84d45c1615b268d69641df709c214`），aria2 `--check-integrity=true --allow-overwrite=true
--listen-port=16881` 哈希校验后进入 **Seeder 态**；`cargo run -- cross-lee --torrent third-party/payload.torrent
--peer 127.0.0.1:16881` 完成拉取，**`all_pieces_verified=true` / 128/128 片 SHA-1 校验通过 / fetched 2 MiB**。
报告 `reports/s1-cross-lee-*.json`；`spikes.json` 的 `S1.observations.cross_client_round` 已更新
（`client_installed=true / executed=true / operator_attest_pending=false`，attest 依据用户显式指令）。

**排错记录**：首轮 0 字节——旧 `payload.bin.aria2` 控制文件记录不全 + `--check-integrity=false` 使 aria2
处于 Leecher 态；删除控制文件、重生成 payload 后以 `--check-integrity=true` 重启即进入 Seeder 态。
另注：rqbit 作 seeder 时 tracker announce 恒报 `port=4240`（实际监听 ephemeral 端口），tracker 发现路径不可用，
故采用直连 `--peer`。

**典型执行路径（任选其一或两个都做）**：

1. **本机第三方客户端**（127.0.0.1，最省事）：
   - 安装任意第三方 BT 客户端（aria2c / transmission-cli / qBittorrent-nox 等）；
   - `cargo run -- cross-seed --bind 127.0.0.1:0`，按打印的 `torrent_path` 在第三方客户端中打开，
     用 manual peer / `--peer` 连到 `listen_addr`；
   - 第三方客户端拉完整文件后，`cross-seed` 打印 `external_peer_completed=true` 并退出，报告入 `reports/s1-cross-seed-*.json`。
2. **跨主机**：在一台机器跑 `cross-seed --bind 0.0.0.0:0 --advertise <LAN_IP>`，另一台（或同一台另一进程）的
   第三方客户端连 `<LAN_IP>:<listen_port>`；注意 Windows 防火墙需放行该入站 TCP 端口。
3. **反向（第三方客户端作 seeder）**：第三方客户端按某 `.torrent` 做种并监听 peer 端口，
   `cargo run -- cross-lee --torrent <该torrent> --peer <IP:端口>`；rqbit 按 `pieces` 逐片校验，
   `all_pieces_verified=true` 即代表对方数据完整正确。

**治理规则**：本一轮只补充互操作证据，**不修改 `S1.status`**（已为 `pass`）；任何并入 S1 结论的动作
（若发现异常需重新评估）都必须由实验执行人**显式 attest** 后手工回填 `spikes.json`，脚本不自动判。
