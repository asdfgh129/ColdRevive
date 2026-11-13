# 变更记录（Changelog）

本文件记录**文档体系与项目决策**的变更。格式参考 Keep a Changelog，版本与文档 front matter 中的 `version` 对应。

## [2.4.38] - 2026-11-13 — M1 开工：Cargo Workspace + 真实 CLI MVP + T0 real 21/21 PASS；M2 仍受 G1 阻塞

> 范围声明：依 [G0-REVIEW](docs/06-plan/G0-REVIEW.md) 的条件冻结**开工 M1**（工程接线与 MVP 实现），
> **不改变任何 Spike 的 `status`**（S3/S5/S6 仍 `not_run`），**不宣称 G1 通过、不启动 M2**。
> 门禁复核结论：**M1 可开工**（G0 条件冻结已签署、剩余 2 个 preflight 阻塞项恰为 M1 自身接线）；
> **M2 不可开工**（ROADMAP §2 依赖 M1/G1，T1 局域网双机与 S3/S5/S6 必测回填均未完成）。

### 工程落地（真实可编译，非零桩）

- 新增根 `Cargo.toml` + `Cargo.lock`：**17 个 crate + 3 个 bin**，`cargo test --workspace --offline` 通过
  **27 个单测 / 0 失败**，全部 std-only、零第三方依赖（不依赖 crates.io 可达性）。
- M1 真实实现：`cr-core`（最小 JSON / 配置 / trace_id）、`cr-hash`（SHA-1 / SHA-256 / HMAC-SHA256，含
  RFC 4231 向量）、`cr-indexer`、`cr-torrent`（真实 bencode、InfoHash、逐片 SHA-1 校验）、`cr-store`
  （COLD 轻量影子索引 + COLD/WAKING/HOT 幂等状态机）、`cr-kad`（专属命名空间 Key、KAD 128-bit /
  Mainline 160-bit 双栈 ID、本地 UDP 发现）、`cr-wake`（PROTO v1 Challenge-Response + 8 类错误码）。
- `coldrevived` / `coldrevive-cli` 按 [CLI v1 契约](tests/e2e/t0/README.md) 实现 20 条真实观测命令：
  HTTP `/health` `/cli` `/torrent` `/announce` `/inject`、UDP KAD 发现与唤醒、最小标准 BT 种子端 +
  本地 tracker。**数据面用第三方 aria2 1.36.0（Motrix Next）真实下载**，私有协议不进入 BT 分片。
- `cr-engine` / `cr-qos` / `cr-nat` / `cr-sched` / `cr-guard` / `cr-dht` / `cr-api` / `cr-ed2k` /
  `cr-reputation` / `cr-notify` / `coldrevive-agent` 为**骨架**（仅 trait / 常量 / 门禁标记），
  按 ROADMAP 归属 M2/M3/M4，不实现、不宣称达标。

### T0 真实验收（G1 的部分证据）

- `CliAdapter.wired = True`（代码层接线），并保留**环境级防呆**：缺任一二进制时 `--mode real` 仍
  `BLOCKED` / 退出码 2（CI 依赖该行为）。`t0_accept.py` 新增 `setup_real_env()`：写双节点配置、
  按 `launch` 契约启动 A/B、等待健康检查，由 `harness.stop_all()` 清理。
- 设 `COLDREVIVE_DAEMON` / `COLDREVIVE_CLI` / `T0_DOWNLOADER` 后执行 `--mode real`：
  **21/21 PASS、FAIL 0、BLOCKED 0、`evidence_valid=true`**，报告
  `tests/e2e/t0/reports/t0-20261113T075155Z-real.json`。真实链路含：伴随种子真实 InfoHash 复算、
  UDP WAKEUP_REQ/ACK、8 类故障注入、30 次唤醒时延采样、30 次复活成功率、aria2 逐片校验下载。
- 首轮实跑诚实暴露 3 个缺陷并修复：① 重复唤醒导致状态跃迁累积 → 改幂等激活；② nonce 重放
  判定「重复激活」误报 → 同上；③ `residual_pids` 把本次在用的 `.pid` 误判为残留 → 改为按进程
  存活状态判定（Windows `tasklist` / Linux `/proc`）。修复后复跑 21/21 全绿。

### M2 合法前置准备（不越过 G1）

- S5 实测标定落地：`config/coldrevive.example.toml` 与 [CONFIG](docs/04-protocol/CONFIG.md) 的
  `background_upload_ratio` 默认由 `0.2` 回调为 **`0.02`**（实测 0.2 → 20.12%、0.05 → 6.19%、
  0.02 → 4.03%；NFR-03 ≤5%）。**这只是配置回调，S5 仍 `not_run`**。
- `cr-qos` 登记实测表与令牌桶模型；`cr-nat` 登记 S6 四类 NAT + CGNAT 矩阵与 R-04 ≥70% 阈值；
  `cr-engine` 固定 `Engine` Trait 边界（ADR-002 选项 A）。
- `spikes.json` 新增 `m1_progress` 与 `g0_review.m1_engineering_wiring`，S3/S5/S6 各补
  `m1_2026_11_13` 证据条目（均 `evidence_valid_for_pass=false`），**状态保持 `not_run`**。

### 边界与剩余工作（G1 / M2 仍关闭）

`T0 evidence_valid=true` **只证明 T0 单机回环链路通过**，不等于 G1：还缺 ① **T1 局域网双机 +
真实 NAT 全链路**；② S3 真实双栈集成 + G3 矩阵；③ S5 真实拥塞链路 rqbit uTP ≤5% 实测；
④ S6（M2）四类 NAT 可达率实测。因此 **M2 保持 `blocked_by_G1`**，不得据本轮结果启动。
`m0_preflight.py` 的 `g0_ready` 算法按 G0-REVIEW §6 保持不变；工程阻塞项清零但豁免项未结案时
新增控制台提示，避免被误读为「八项 Spike 结论齐备」。

### 文档与 CI 同步

- [ROADMAP](docs/06-plan/ROADMAP.md) 2.8 → 2.9、[G0-REVIEW](docs/06-plan/G0-REVIEW.md) 1.0 → 1.1、
  [CONFIG](docs/04-protocol/CONFIG.md) 2.0 → 2.1、[README](README.md)、`src/README.md`、
  `tests/README.md`、`tests/e2e/t0/README.md` §3/§5；`t0_cli_selftest.py` 由「断言 wired=False」
  改为「代码层 wired=True + 环境级防呆仍 BLOCKED」。
- `.github/workflows/t0.yml`：新增 `cargo test --workspace --offline`；修正已过期的
  「S1–S7 必须 `not_run`」断言（改为人工回填白名单 + 证据字段校验）；real 模式防呆步骤改为
  「CI 无二进制时应 BLOCKED」。

## [2.4.37] - 2026-11-13 — G0 条件冻结生效 + S3/S5/S6 真实量测 + 工程门禁收敛

> 范围声明：本轮**不改任何 Spike 的 `status`**（S3/S5/S6 仍 `not_run`），不虚判 `pass`，
> **不创建生产 crate**（M1 工程接线仍未完成）。项目所有者于本轮显式作出两项治理决策：
> ① [ADR-001](docs/02-architecture/adr/ADR-001-kad-network.md) 签署默认模式 **`public`**（1.1 待定 → 1.2 已接受）；
> ② 批准 **G0 条件冻结（conditional pass）**——豁免 S3/S5/S6 在 G0 时点给出最终结论，M1/M2 必测回填。
> 评审记录见 [G0-REVIEW](docs/06-plan/G0-REVIEW.md)（新增 DOC-22），机读决策见 `spikes.json` 的 `g0_review`。

### 新增真实实验（三项，全部标记 `evidence_valid_for_pass=false`）

- **S3 `tests/spikes/s3/dualstack_io.py`**：真实 UDP 双栈并发 + 共享端口 demux（KAD 16B / Mainline 20B），
  注入 1437 个跨命名空间畸形帧 → **1437/1437 拒绝、0 误路由、0 丢包**；总带宽 **3.276 Mbit/s**
  ≈ 两栈之和，RSS 增量 **0.93 MiB**。C2 的「带宽增量」由估算升级为实测。
  排错留痕：首版 demux 仅校验最小长度，160 个错宽 ID 帧被误收；改为精确 ID 宽度校验后归零。
- **S5 `tests/spikes/s5/qos_bench.py`**：真实 TCP 双流 + 真实时钟令牌桶 + 64 Mbit/s 应用层瓶颈整形
  + LEDBAT 式延迟退让模型。令牌桶硬上限成立（后台 ≤20.25% 链路），但
  **默认 ratio=0.2 时前台劣化 20.12%**（退让模型 15.0%）、ratio=0.05 → 6.19%、
  **ratio=0.02 → 4.03%**。结论：**NFR-03 ≤5% 在默认配置下未达成，R-07 由 🟡 上调 🟠**，
  M1 默认值须按实测回调并启用 fallback 链。报告 `reports/s5-qos-final-ratio0{20,05,02}.json`。
- **S6 `tests/spikes/s6/stun_probe.py`**：RFC 5389 STUN + SSDP + NAT-PMP + 二跳 traceroute 的
  **首个真实 NAT 样本**：`192.168.1.210` → `112.5.60.4:36764`，跨 4 台 STUN（2 个目的 IP）映射不变（cone 类），
  公网 UDP 可达（RTT≈245 ms），**UPnP IGD 与 NAT-PMP 均不可用**，二跳公网、未见 CGNAT。
  R-04 不再按「UPnP 可用」假设估计；Q4 仍未结案，须 M2 多 NAT 实测。
  排错留痕：首版误把 STUN family `0x02` 当 IPv4（实际 IPv4=`0x01`），4 个有效响应被误判为解码失败。

### 工程门禁收敛

- `m0_preflight.py` 的「至少一个第三方 BT 客户端」改为**证据驱动**：除 PATH 标准名外，识别已安装的
  Motrix Next `motrixnext-aria2c.exe`，且**只有** `spikes.json` 的 `S1.cross_client_round` 记录
  `executed=true` 且 `all_pieces_verified=true` 时才计 OK（不把「文件存在」误判成「互通通过」）。
  阻塞项 **3 → 2**（剩余：M1 二进制、CliAdapter.wired，均属 M1 工程接线）。
- `m0_preflight.py` 报告新增 `g0_governance` 字段并在控制台打印治理状态；
  明确 **条件冻结不改变 `g0_ready` 算法、不自动清除任何 Spike 结论**。

### 文档同步

- 新增 [DOC-22 G0-REVIEW](docs/06-plan/G0-REVIEW.md)：八项结论矩阵、豁免边界与退回条款、
  ADR 状态、残余风险、工程门禁、M1/M2/M4 复核时点；`docs/README.md` 2.5 → 2.6 补 DOC-22 行。
- `ADR-001` 1.1 → 1.2（待定 → 已接受，所有者签署 `public`）；`adr/README.md` 统计 9 → 10 已接受、2 → 1 待定。
- `TECH-STACK` 2.3 → 2.4、`ROADMAP` 2.7 → 2.8（§6 状态行 + 本轮条目 + **G0 缺口清单第三次更新**）、
  `RISK`（R-04 观测收窄 / R-07 等级上调 + 冻结后风险复核节）、`OPEN-QUESTIONS` Q4 先导观测注记。
- `tests/spikes/README.md` 顶部状态行、§3 起跑顺序、§5 S3/S5/S6 段落、§6 第 4 条补唯一豁免说明；
  `s3/README.md` / `s5/README.md`（新增 §8 量测表 + 4 项 harness 排错留痕） / `s6/nat-matrix.md`（新增 §8 先导实测）。
- `spikes.json`：`status_note` 重写为当前口径，新增 `g0_review`，S3/S5/S6 的 `observations` 与 `evidence_required` 补本轮证据。

### 剩余工作（冻结后）

M1 出口：S3 真实双栈集成 + G3 矩阵、S5 真实拥塞链路 rqbit uTP 实测 ≤5%、按门禁创建 crate 骨架；
M2：S6 四类 NAT（含 CGNAT）可达率实测，R-04 阈值 ≥70%；M4 前：R-02 拒绝率复审、R-11 书面法务意见。
任一 M1/M2 实测不达标须按各 spike fallback 降级并重新提交冻结评审。

## [2.4.36] - 2026-11-11 — G0 推进：S1 第三方客户端互操作轮通过 + S4/S7 `pass`、S5/S6 分析就绪

> 范围声明：本轮依用户指令以本机 **Motrix Next** 作第三方 BT 客户端执行 S1 跨客户端一轮（通过，`S1.status` 维持
> `pass`），并推进 S4–S7：**S4 / S7 经真实量测回填 `pass`**，S5 / S6 产出分析并诚实保持 `not_run`（按治理规则
> 本地/分析证据不虚判 pass）。**仍不创建生产 crate**（G0 未通过：S3/S5/S6 待 M1/M2 真实环境验证、ADR-001 待所有者签署）。

### S1 第三方客户端互操作轮（G3 素材，`all_pieces_verified=true`）

- 采用 **Motrix Next** 捆绑 `motrixnext-aria2c.exe`（aria2 1.36.0，download-only 构建）作第三方 seeder，
  走反向路径：`tests/spikes/s1/third-party/make_payload.py` 生成确定性 `payload.bin/payload.torrent`
  （128 片 × 16 KiB，infohash `2b77301a18d84d45c1615b268d69641df709c214`），aria2 `--check-integrity=true`
  哈希校验后进入 Seeder 态监听 `127.0.0.1:16881`；`cargo run -- cross-lee --peer 127.0.0.1:16881` 拉取
  **128/128 片全部 SHA-1 校验通过**（2 MiB 全量）。
- 排错：首轮 0 字节系旧 `.aria2` 控制文件 + `--check-integrity=false` 使 aria2 处于 Leecher 态；另确认
  rqbit 作 seeder 时 tracker announce 恒报 `port=4240`（实际监听 ephemeral），tracker 发现路径不可用，故直连 `--peer`。
- `spikes.json` S1.observations.cross_client_round 更新：`client_installed=true / executed=true /
  operator_attest_pending=false`（attest 依据用户显式指令）；报告 `tests/spikes/s1/reports/s1-cross-lee-*.json`。

### S4（R-06，影子索引 ≤100 MB）→ `pass`

- `tests/spikes/s4/`：真实 Rust `HashMap<[u8;20], ShadowRow>` 模型（COLD 轻量 handle，不含 Bitfield / 逐片哈希，
  符合 DATA §5）+ Windows `GetProcessMemoryInfo` 工作集增量采样（baseline→10k→50k→200k）。
- 实测：1 万文件索引 ≈3.79 MiB、含 SQLite 缓存 **≈7.79 MiB**（≪ 100 MB NFR-01）；5 万 ≈15.9 MiB（兼满足
  NFR-05 ≥5 万容量）；20 万 ≈53 MiB 仍有余量。R-06 去险，fallback（索引分库 / 惰性加载）未触发。
- 证据 `tests/spikes/s4/reports/s4-shadow-index-*.json` + `measure_s4.py`。

### S7（R-07，4 GB 造种开销）→ `pass`

- `tests/spikes/s7/s7_bench.py`：流式哈希 4 GB（262144 片 SHA-1 + 全文件 SHA-256 + `.torrent` bencode/写出），
  **耗时 4.14 s / 吞吐 989 MB/s / 峰值 RSS 29.3 MiB**，Idle 优先级（NFR-02）。R-07 去险，
  fallback（分片自适应 + 增量哈希）未触发。证据 `tests/spikes/s7/reports/s7-torrent-gen-*.json`。

### S5 / S6 → `not_run`（分析就绪，待 M1/M2 实测回填）

- **S5**（`tests/spikes/s5/README.md`）：NFR-03 双层控制——令牌桶硬上限 20% 上行（可本地单元验证）+
  uTP/LEDBAT 退让（rqbit 9.1.0 支持 uTP，S1 已证真实分片传输）；「前台劣化 ≤5%」须真实拥塞链路，
  回环队列延迟恒 ~0 无法触发 LEDBAT 退让，M1/G3 实测回填。
- **S6**（`tests/spikes/s6/nat-matrix.md`）：穿透栈（UPnP + NAT-PMP + UDP 打洞 + 中继/LowID 回退）覆盖
  全 4 类 NAT，分析型含中继可达率 ≥95%（> R-04 的 70% 阈值）；量化判据须 M2 真实多 NAT 实测
  （OPEN-QUESTIONS Q4，CGNAT 比例为关键不确定项）。

### 文档同步

- `TECH-STACK` 2.2 → 2.3（§2 状态块：跨客户端轮结果 + S4–S7 推进）；`ROADMAP` 2.6 → 2.7（§6 顶部状态行 +
  追加本轮条目 + G0 缺口清单第二次更新）；`tests/README.md` 状态行更新；`tests/spikes/README.md` 顶部状态行与
  §5/§6 已于本轮内先行更新；`tests/spikes/s1/README.md` §7 由「准备状态」改为「执行结果」并补排错记录。
- `m0_preflight.py` 现报 `G0 就绪: False`、`g0_pending_spikes = S3,S5,S6`；其余 3 个 MISSING 为 T0/G1 工程
  接线项（M1 二进制、CliAdapter、第三方下载器 PATH 标准名）。

## [2.4.35] - 2026-11-11 — G0 收尾校验 + 第三方 BT 客户端一轮预备

> 范围声明：本轮**不判任何新 pass、不改 `S3.status`**（仍为 `not_run`）、**不创建生产 crate**（G0 未通过）。
> 完成 S3 首轮证据的落盘与全量文档校验；并为「补第三方 BT 客户端 / 跨主机一轮」准备可立即执行的 harness。

- **S3 证据落盘**：在 `tests/spikes/s3/` 补充 50k/栈压测报告（`reports/s3-stress-50k-*.json`），与既有 10k 报告共同支撑 `spikes.json` 的 `dual_index_mib_at_10k=2.26 / at_50k=11.08`；两份报告均 `C1 verified`（串扰/泄漏 0/0）、`C2 verified`（≤100 MB NFR-05）、`evidence_valid_for_pass=false`。
- **文档校验通过**：`scripts/build_docs.py --check` 校验 35 份 Markdown（front matter / 相对链接 / 索引数据块均有效）；全量重建生成 35 个 HTML 页面并刷新 `docs/index.html`（43 份文档内嵌）。
- **第三方 BT 客户端一轮预备**（对应 S1 用户指令「补第三方 BT 客户端 / 跨主机再判」）：
  - `tests/spikes/s1/src/main.rs` 新增 `cross-seed`（rqbit 作 seeder，写出真实 `.torrent` 到磁盘、监听 `--bind`、等待外部 leecher 拉全部分片）与 `cross-lee`（rqbit 作 leecher，按 `--torrent` + `--peer` 直连第三方/跨主机 seeder、逐片校验）两个子命令；已编译通过并本机 `cross-seed` 自测（seed + 写种子 + 打印 info_hash/sample_sha1 + 超时干净退出）。
  - `spikes.json` S1.observations 新增 `cross_client_round`：`harness_ready=true / client_installed=false / executed=false / operator_attest_pending=true`。
  - `tests/spikes/s1/README.md` §1/§2 扩展子命令表与运行示例，新增 §7 记录上午安装客户端后的执行路径与治理规则（**不回滚 S1=pass**，需 operator attest 方可并入结论）。

## [2.4.34] - 2026-11-11 — G0 推进：**启动 S3 双栈资源可控 spike（首轮结构性证据）**

> 范围声明：本轮**启动 S3**（不判 pass），在 `tests/spikes/s3/` 新增零依赖双栈探针，给出首轮
> 结构性证据；**不做真实 KAD / Mainline DHT 网络加入**，`S3.status` 保持 `not_run`。仍不创建生产 crate
> （G0 未通过）。

- `tests/spikes/s3/`：零依赖 Rust 二进制 `s3-dualstack`，两个独立容器 `HashMap<[u8;16],_>`（KAD）+ `HashMap<[u8;20],_>`（BT），顺序 + 并发读写压测。
- 首轮证据：**S3-C1 verified**（串扰/泄漏 = 0，10k 与 50k/栈两轮）；**S3-C2 verified 结构性上界**（双栈索引 10k/栈 ≈2.26 MiB、50k/栈 ≈11.08 MiB，均 ≤ NFR-05 100 MB）。
- `spikes.json`：S3 新增 `observations`（规模、C1/C2 结果、scope、边界），`status` 仍为 `not_run`。
- `TECH-STACK` §2、`ROADMAP` §6（S3 已启动 + 「未开始」收窄为 S4–S7）、`tests/spikes/README.md` §5 补充 S3 说明。

## [2.4.33] - 2026-11-11 — G0 推进：**回填 S1=pass 并定稿 ADR-002（选项 A：采用 rqbit）**

> 范围声明：本轮**依 `two-peer` 4/4 `verified` 证据回填 `S1.status=pass`**（实验执行人手工回填，
> 非脚本自动），并据 `conclusion_rule` 将 [ADR-002](adr/ADR-002-bt-engine.md) 由「待定」定稿为
> 「已接受」（选项 A：采用 `rqbit`，握手拦截自实现于 `cr-engine` 外围）。**仍不创建生产 crate**
> （G0 未通过，S3–S7 未提交结论、ADR-001 待所有者签署）。S1 结论的边界同 v2.4.32：
> `127.0.0.1` 回环、0 公网报文、同主机双 Session；第三方 BT 客户端 / 跨主机一轮按用户指令补做（见下）。

- `tests/spikes/spikes.json`：`S1.status` 由 `not_run` → `pass`；`status_note` 同步；`S1.blocked_by` 已在前序置空。
- `ADR-002` 1.2 → 1.3：状态「待定」→「已接受」，决策表与 §决策 / §复核时点更新；明确「选项 A 已采纳、握手拦截自实现」。
- `ADR 索引`：ADR-002 行 🟡待定 → ✅已接受；统计 8→9 已接受；G0 待签署项重写（ADR-002 已定稿，ADR-001 仍待所有者签署 public/private）。
- `TECH-STACK` §2 状态块：S1 判据 4/4 `verified` 且已回填 `pass`，ADR-002 已定稿。
- `ROADMAP` §6：当前状态行更新（S1=pass）；S1 边界与治理段更新；G0 剩余缺口清单 ① 标记完成、③ 收窄为 ADR-001。
- `tests/spikes/README.md`：顶部状态行、§4 S1 判据、§6 第 3 条口径更新为「已回填」。
- `m0_preflight.py` 动态逻辑：`g0_pending_spikes` 自动剔除 S1（现为 `S3,S4,S5,S6,S7`）；`s1_action` 走「已有结论」分支。

## [2.4.32] - 2026-11-11 — 推进 G0：**S1 双 peer 真实分片传输与中途暂停打穿，四条判据齐备**

> 范围声明：本轮**未改 `spikes.json` 的 `S1.status`**（仍 `not_run`）、**未判任何 `pass`**、
> **未把 ADR-001/002 改为已接受**、**未创建生产 crate**（G0 未通过）。
> 全部证据为 `127.0.0.1` 回环，0 个公网报文。`G0` 仍未通过。

### S1 补齐：`tests/spikes/s1` 新增 `cargo run -- two-peer`

按 librqbit 9.1.0 源码核实的 `SessionOptions.listen` / `ConnectionOptions` /
`AddTorrentOptions.initial_peers` / `AddTorrentOptions.ratelimits` /
`ManagedTorrent::wait_until_completed`，建立**两个独立 Session**：

- seeder：1 MiB 确定性样本预置、`ListenerMode::TcpOnly` 绑 `127.0.0.1:0`、上传限速 256 KiB/s；
- leecher：**空目录**、`paused: true` 实例化、`initial_peers = [seeder_addr]`、仅主动出站；
- 两端 DHT / tracker / LSD 全禁，各自独立输出目录；每次运行先清 `.run/two-peer/`，
  避免残留文件把「复制」误认成「传输」。

| 连续 4 轮一致观测 | 值 |
| :--- | :--- |
| leecher 起点 | `progress_bytes = 0` / `is_paused = true` |
| 传输中途（30 ms） | `262144 / 1048576`（16 片，分片整数倍） |
| `pause` + 停留 900 ms | `is_paused = true`，位图仍 262144，不前进不回退 |
| `unpause` → `wait_until_completed` | `finished = true` / `1048576` |
| seeder `uploaded_bytes` | 1048576 |
| leecher 恢复后 `fetched_bytes` | 786432（48 片；`pause` 重建 live state，计数器从恢复点重算） |
| 落盘校验 | 64/64 片 SHA-1 匹配、0 mismatch、逐字节等于确定性样本 |

`probe` 子命令回归无变化（C1/C2 `verified`、C3/C4 保留历史 `partial`）。
**合起来 S1 四条判据 4/4 `verified`**：C1/C2 = `s1-probe-*.json`，C3/C4 = `s1-two-peer-*.json`。
报告 `s1_status` 恒为 `not_run`，`conclusion_rule` 不变。

修掉一处实现错误：seeder 最初把 `ConnectionOptions.enable_tcp` 设为 `false`，
rqbit 构造 stream connector 时报 `no way to connect to peers`；改为 `true`
（seeder 无 peer 来源，启用 TCP 只满足构造约束、不会发起外连）。

### 治理边界：本轮刻意留下的两个动作

1. **不回填 `S1.status`**。`spikes.json` 的 `conclusion_rule` 要求实验执行人按证据手工回填，
   S2 亦是先由项目所有者明确指令「成功后直接判 pass」才回填；本轮无此指令。
   另 `tests/spikes/README.md §6` 第 3 条规定「本地模拟结果不能单独支撑 `pass`」——
   `two-peer` 是真实协议栈的标准 Peer Wire（非逻辑预演），但两端同主机回环，
   **是否满足该条须由实验执行人认定**，或要求补第三方 BT 客户端 / 跨主机一轮。
2. **不定稿 ADR**。[ADR-001](docs/02-architecture/adr/ADR-001-kad-network.md) 明文要求
   「定稿与 `public`/`private` 的签署由项目所有者执行」；[ADR-002](docs/02-architecture/adr/ADR-002-bt-engine.md)
   的「S1 通过 → 采用 `rqbit`」决策树已具备条件，但依赖上面的 `S1.status` 回填与架构组签署。
   按 [ADR 索引 §4](docs/02-architecture/adr/README.md)，**已接受的 ADR 不修改正文**，
   故待定阶段只补正文、不改状态。

顺带记录一处口径冲突：[ROADMAP M0 交付物](docs/06-plan/ROADMAP.md) 写「ADR-005 定稿」，
而 ADR-005 自身复核时点为 M4；按 ADR 自身时点以 M4 为准，M0 不强制定稿。

### 文档同步

| 文件 | 变更 |
| :--- | :--- |
| `tests/spikes/s1/README.md` | 重写为两份报告的分工、two-peer 观测表、证据边界与回填清单 |
| `tests/spikes/spikes.json` | `S1.observations.two_peer` 证据路径、`criteria_coverage` 四项更新、`blocked_by` 清空、`status_note` 开头矛盾口径修正并追加本轮记录 |
| `tests/spikes/README.md` | 首行状态修正、§4 补完成记录、§6 新增回环证据口径待认定 |
| `ADR-002` 1.1 → 1.2 | 新增「S1 判据补齐」章节、更新决策与复核时点，**状态仍「待定」** |
| `DOC-05` ADR 索引 2.1 → 2.2 | ADR-002 行备注、新增「G0 待签署项」注（含 ADR-005 口径差异） |
| `DOC-04` TECH-STACK 2.1 → 2.2 | §2 新增 S1 判据补齐段、`crate.rqbit` 阻塞解除 |
| `DOC-14` ROADMAP 2.5 → 2.6 | §6 标题改为「起始 11-09 / 更新至 11-11」、补历史演化链说明、新增 S1 与「G0 剩余缺口清单」、修正「未开始」与历史排期中的过期表述 |
| `tests/spikes/m0_preflight.py` | 新增 `g0_pending_spikes` 字段与终端「G0 未结 Spike」行，把 G0 的真实判据（S1–S8 全部有结论）与 3 个工程阻塞项分开呈现；S1 的下一步提示改为动态读取 `S1.observations.two_peer`。`g0_ready` 判定逻辑未改 |

**G0 剩余缺口**：① S1 状态回填（治理动作，技术判据已齐）；② S3–S7 五项实验未开工；
③ ADR-001/002 定稿与所有者签署；④ 技术路线冻结评审。
`m0_preflight.py` 报的 3 个阻塞项偏 T0/G1 工程接线，装齐它们不能替代 G0 判据。

## [2.4.31] - 2026-11-11 — ADR-011 L2 落地：**源字典 + 最小 eD2k TCP 栈，索引面/传输面双闭环（LAN/回环）**

> 范围声明：本轮**没有公网报文**、**没有改 `spikes.json`**、**没有判任何 pass**、
> **没有创建生产 crate**（G0 未通过）。所有证据为 RFC1918 / `127.0.0.1`，
> 报告固定 `public_network_contacted=false` / `evidence_valid_for_pass=false`。

### Q9 / V-04 结案：两个候选 crate 都不是可复用的传输栈

- crates.io `ed2k-rs` **不存在该 crate**（API 返回 `crate ed2k-rs does not exist`）；
  GitHub `runfalk/ed2k-rs` 为 alpha、仅哈希、无 AICH、无传输栈，最后 push `2023-08-28T16:28:05Z`。
- crates.io `ed2k 1.0.1` 同样仅哈希，最后更新 `2023-05-19T22:55:00.744168Z`；
  提供 Red / Blue 变体，**默认别名 `Ed2k` = Blue**。
- 结论：v1 保留仓内可审计自实现，不引入停更依赖（R-12）；最小栈「自研 vs 复用」已答 = 自研。
  **Q9 从 [OPEN-QUESTIONS](docs/06-plan/OPEN-QUESTIONS.md) 待决表移出**，V-04 标记「✅ 已确认」并补 §3.1 核实结论。
  credit 缺失代价（R-15）仍留 M1，未随 Q9 结案。

### 源码核实连带修掉一个会导致「死条目」的哈希缺陷

L1 的 63 B 探针从未跨过 `PARTSIZE` 边界，掩盖了精确整数倍大小时的 Blue / Red 分歧。
aMule `6a110563` 采用 **Red**（`KnownFile.cpp::SetFileSize` 样本、
`ThreadTasks.cpp::CreateNextPartHash` 的 `g_emptyMD4Hash`、`KnownFile.cpp::CreateHashFromHashlist`）：
`size % 9_728_000 == 0` 时追加 `MD4("") = 31D6CFE0…`。
`kad-mini/src/wire.rs::ed2k_hash` 已修正、新增 `ed2k_part_hashes` 与上游已知答案向量回归，
并反转了 L1「精确整数倍不补空分块」的旧断言。若沿用 Blue，A 轨会生成「搜得到、永远下不动」的 `fileID`。

### 代码（全部在 `tests/spikes/s2/`，未触碰 `src/crates/`）

| 位置 | 内容 |
| :--- | :--- |
| `kad-mini/src/wire.rs` | `0x44` / `0x34` 载荷编解码、`source_tags`、源 tag 常量、Red 边界修正、MD4 中一组遗留重复 `g` 计算清理（消除编译告警，行为不变） |
| `kad-mini/src/main.rs` | `l2-source-probe` 子命令：RFC1918 硬校验 → BOOTSTRAP → `0x43`+`0x44` 双轨发布 → `0x33`+`0x34` 双轨取回 |
| `ed2k-mini/`（新 spike crate，零依赖） | `wire.rs` 标准 eD2k TCP 帧/HELLO/文件请求/hashset/队列/分片；`stack.rs` 下载与上传共用一份状态机 + 单槽最小上传队列；`main.rs` `selftest` / `loopback` / `lan-hello`；`README.md` |

源码核实要点（详见 PROTO §2.2）：`0x44` 载荷**没有** `u16` 条目数、tag 数 1 字节、
距离校验用 `fileID`、缺 `TAG_SOURCETYPE (0xFF)` 会静默丢弃且不回 `PUBLISH_RES`、
源 IP/UDP 端口取自 UDP 报头；eD2k TCP 帧为 `<protocol u8><len u32 LE><opcode u8><payload>`，
`OP_REQUESTPARTS` 是**先 3 个 start 再 3 个 end**（不是交错对），HELLO tag 与 KAD `ReadTag` 同布局。

### 首轮证据（2026-11-11 复跑一致）

- `kad-mini l2-source-probe`（`172.16.29.209:41296`，真实 aMule）：两笔 `PUBLISH_RES load=1`
  （源发布 65 B / 5 tags、key 回显匹配）、`0x33` 与 `0x34` 各 1 条答案、
  `our_entry_returned=true` / `our_source_returned=true`，
  verdict = `confirmed_dual_track_keyword_and_source_publish_then_retrieve`。
- `ed2k-mini loopback`：3/3 用例完整闭环（4 096 B / 恰好 1×PARTSIZE 的 Red 空分块 / 多分块），
  每例 53 个分片内 `hash_matches=true`、`content_matches=true`、`completed=true`。
- `ed2k-mini lan-hello`（`172.16.29.209:31969`，真实 eMule）：`OP_HELLO` 104 B →
  `OP_HELLOANSWER`，nick `https://www.emule-project.org`；`OP_ASKSHAREDFILES` →
  `OP_ASKSHAREDFILESANSWER`，**共享文件数 0**。
- 回归：`kad-mini` `cargo test` **21/21**、`selftest` **48/48**、`loopback` **5/5 发布 + 5/5 命中 + 0 协议错误**；
  `ed2k-mini` `cargo test` **31/31**、`selftest` **26/26**；`build_docs.py --check` 通过。
- 报告：`tests/spikes/s2/reports/kad-l2-source-probe-20261111023613.json`、
  `ed2k-mini-loopback-20261111023642.json`、`ed2k-mini-lan-hello-20261111023647.json`（本地证据，由 gitignore 排除）。

### 证据边界与开放观测（必须跟结论写在一起）

- **握手是真客户端给的，完整分片传输是我们自己两端给的**，二者不可互相替代。这台 LAN eMule
  共享文件数为 0，能力 ②「从旧客户端下载」无法对它实测，M1 须换真有共享文件的对端补测并实测 R-15。
- `OP_SETREQFILEID (0x4F)` 对不可能存在的 fileID：aMule `ClientTCPSocket.cpp:425` 规定应回
  `OP_FILEREQANSNOFIL (0x48)`，真实 LAN eMule 在 4 s（另做 12 s 独立复核）内静默。
  **原样登记为 `open_observations`，未改写成通过或失败**；`0x58` 对未知 fileID 静默则是源码设计行为。
- 源条目由同一进程发回并取回，未验证第三方旧客户端 UI 搜索；关键词 Key 一致性仍是源码核实。
- 公网 `0x43` / `0x44` 发布须执行人**重新显式 attest**，本轮未做、未自动加该参数。

### 文档同步与版本

| 文档 | 变更 |
| :--- | :--- |
| [PROTO](docs/04-protocol/PROTO.md) | **2.4 → 2.5**：新增 §2.2（`0x44`/`0x34` 载荷、eD2k TCP 报文序列、`ed2k_hash` Red 更正、§2.2.4 口径冲突），§4 增「L2 映射（v2.5）」行。**`PROTOCOL_VERSION = 1`、§3 私有唤醒字段与 T0 golden 一字未动** |
| [OPEN-QUESTIONS](docs/06-plan/OPEN-QUESTIONS.md) | **2.0 → 2.1**：Q9 结案移出并留注；新增 **Q10**（`WAKEUP_REQ.capabilities.ed2k` 语义冲突，M1 定稿） |
| [REFERENCES](docs/08-reference/REFERENCES.md) | **2.0 → 2.1**：V-04 标「✅ 已确认」、新增 §3.1 核实结论与来源；§4 补两条流转规则 |
| [TECH-STACK](docs/02-architecture/TECH-STACK.md) | **2.0 → 2.1**：eD2K 行改为「仓内可审计自实现 / 两 crate 仅作哈希参考」；S2 状态段补 L1/L2 探针证据 |
| [RISK](docs/05-quality/RISK.md) | **2.0 → 2.1**：R-02 / R-12 / R-15 按 V-04 与本轮证据更新，明确「单条唯一关键词、0 公网报文」不构成 R-02 拒绝率证据 |
| [TEST](docs/05-quality/TEST.md) | **2.2 → 2.3**：M0 前置段补 L2 两组回归与其证据边界 |
| [ROADMAP](docs/06-plan/ROADMAP.md) | **2.4 → 2.5**：§6 补 L2 落地条目；把 2026-11-09 快照里的 `not_run` 标为「当时值」；生产 crate 仍受 G0 冻结 |
| [DOC-00](docs/README.md) | **2.4 → 2.5**：§7 把「协议任何变更必须递增 `PROTOCOL_VERSION`」收窄为私有契约字段变更，并写明 PROTO §4 的「映射修订例外」，消除两处口径冲突 |
| [ADR 索引](docs/02-architecture/adr/README.md) | **2.0 → 2.1**：ADR-011 行补实施状态指针，并声明按 §4「已接受的 ADR 不修改正文」**不回写 ADR-011 正文** |
| [PLAN](tests/spikes/s2/PLAN.md) | 新增 **§23**（Q9/V-04、Red 修正、`0x44` 索引面、eD2k 传输面、回归、证据边界、M1/M4 下一步）；§22.3 / §22.7 更正；修正文件头与 P4 表格的过期 `not_run` 表述及 §22.7 的断链（`../../docs` → `../../../docs`） |
| `kad-mini/README.md` / `ed2k-mini/README.md` / `tests/spikes/README.md` | 新增 §10（源字典 + Red 更正）、L2 spike 说明、L1/L2 探针边界声明 |
| `.gitignore` / `tests/spikes/s2/.gitignore` | `/target/` → `**/target/`，避免把新 spike 的 Cargo 构建产物入库；把 `*.out.json` / `*.err.txt` / `live-*.json` 等本地运行捕获排除（正式报告仍在 `reports/`） |

**ADR-011 本体未修改**：按 [ADR 索引 §4](docs/02-architecture/adr/README.md)，已接受的 ADR 不修改正文，
实施进度与证据记在 PLAN §23 / 本变更记录 / 各专题文档中；D1–D4、L3 否决与复核时点全部保持原样。

## [2.4.30] - 2026-11-11 — ADR-011 L1 落地：**eMule 关键词字典互通，在真实 aMule 上闭环**

### 源码核实（会改变实现的一条）

`Kademlia.cpp::KadGetKeywordHash` 用的是 `CryptoPP::Weak::MD4(unicode2UTF8(term))` →
`CUInt128::SetValueBE`，**不是 SHA-1**；`UInt128.cpp::ToByteArray` 是其逆，线上 16 字节即 MD4 摘要原值。
函数内不做小写化 / trim / Unicode 归一化。兼容轨与私有轨是两套哈希，不得复用同一函数。
`PUBLISH_SOURCE_REQ = 0x44` 已在 `wire.rs::op` 的源码核实表，但 PROTO §2.0 尚未描述载荷，L2 前须补齐。

### `kad-mini` 新增（探针内，未触碰生产 crate）

- `wire.rs`：`md4`（RFC 1320 纯 Rust，零依赖）、`kad_keyword_hash`、`ed2k_hash`
  （`ED2K_PART_SIZE = 9_500 * 1024 = 9_728_000 B`，单/多分块、~~精确整数倍不补空分块~~）。
  ⚠️ **划掉的边界规则已于 2.4.31 更正为 aMule Red（追加 `MD4("")`），此处保留当时原文**。
- `main.rs`：`ed2k-dict-probe` —— RFC1918 硬校验、BOOTSTRAP 双向可达、
  `0x43` 发布**无前缀**关键词条目（`fileID = 真实 ed2k_hash`）、`0x33` 取回、产出标准 `ed2k://` 链接。
  固定 `public_kad_contacted=false` / `evidence_valid_for_pass=false`，不判 pass。
- `selftest` 新增 5 项（MD4 向量、关键词哈希、eD2k 单分块），`cargo test` 新增 3 项。

### 局域网实测（`tests/spikes/s2/reports/kad-ed2k-dict-probe-20261111012841.json`）

`172.16.29.209:41296`：`PUBLISH_RES load=1`（7.0 ms，Key 回显匹配）、`SEARCH_RES` 1 条答案
（11.7 ms）、`our_entry_returned=true`、返回 `\x01` 文件名 / `\x02` 大小 / `3`（aMule 附加），
verdict = `confirmed_unnamespaced_keyword_publish_then_retrieve`。**0 个公网报文**。

证据边界：本轮证明「aMule 按我们发过去的 target Key 存下并回服务」；
「aMule 自己按关键词算出的 Key 与我们一致」是源码核实，不是运行时验证。
副作用：该 aMule 若同时连公共 KAD，可能为持有的条目做 TTL 续期并重发一次；探针只用唯一无意义关键词。

### 回归与文档

`cargo test` 18/18、`selftest` 42/42、`loopback` 5/5 发布 + 5/5 命中、0 协议错误；
`PROTOCOL_VERSION = 1`、T0 golden 未动。`PLAN §22`、`kad-mini/README §9`。
L2 尚未开工（**当时状态，已于 2.4.31 完成**）：先做 Q9（`ed2k-rs` 复用边界），再补 `0x44` 载荷与最小传输栈；
任何公网发布需执行人重新显式 attest，本轮未做、未自动加该参数。

## [2.4.29] - 2026-11-11 — 所有者决策 D1–D4：**ADR-011 批准 eD2k 双向互操作 L1/L2，否决 L3**

### 审查：撤回上一轮的「唤醒网关」结论

上一轮把「BT 未活跃做种」误当成「文件位于远端休眠节点」，据此推出旧客户端下载冷文件必须经常在线网关。
项目所有者指出 **eD2k 不区分冷热、共享即可见可下载**，核对本项目自己的设计后确认该前提不成立：
[ADR-009](docs/02-architecture/adr/ADR-009-sharing-model.md) 的 `COLD` = 仅一条索引行、无做种上下文；
[LLD §6](docs/03-design/LLD.md) 明确切入时「从磁盘加载元数据」；[ADR-006](docs/02-architecture/adr/ADR-006-metadata-distribution.md)
的元数据 URL 是**本机**；[PRD US-06](docs/01-product/PRD.md) 的守护进程常驻。
文件始终在本机磁盘、节点常在线，eD2k 源只需一个常在线监听读盘发片，与 [LLD §7](docs/03-design/LLD.md)
的 M-10 握手拦截器同构。**L3 是伪需求，予以否决**；节点真下线时源消失，与 eMule 源 TTL 一致，不额外引入机制。

### 四项决策

| # | 决策 |
| :--- | :--- |
| D1 | 所有文件公开可见，共享即公开，不设 per-file 公开开关 |
| D2 | 冷热是 BT 做种状态，不映射到 eD2k 源 |
| D3 | 现在实现标准 eD2k 传输栈；项目里没有「自有传输栈」（P1 零协议侵入，`WAKEUP_REQ` 是控制面 RPC），将来若要加私有优化须另立 ADR 并先证优势 |
| D4 | 法务不作为 L1/L2 前置门禁、可随时修改；S8 / R-11 保持登记，M4 前补书面意见 |

D1/D4 附两条技术事实：已发布到第三方节点的条目按 TTL 存活、**无法即时召回**；PRD 的「不规避版权保护 /
不绕过地域限制 / 不提供匿名化保证」三条非目标不变。

### 源码核实：eMule 关键词 Key 是 MD4，不是 SHA-1

`Kademlia.cpp::KadGetKeywordHash` = `CryptoPP::Weak::MD4(unicode2UTF8(term))` → `CUInt128::SetValueBE`；
`UInt128.cpp::ToByteArray` 是其逆，故线上 16 字节即 MD4 摘要原值。函数内不做小写化 / trim / Unicode 归一化。
与 PROTO §1 的 `SHA1(NAMESPACE_PREFIX + "kw:" + term)` 是两套算法，**双轨不得复用同一哈希函数**。

### 文档同步

- 新增 [ADR-011](docs/02-architecture/adr/ADR-011-ed2k-interop.md)（已接受）：四层能力、双轨发布表、L1/L2 范围、L3 否决理由、D1–D4、风险与复核时点。
- ADR 索引加第 11 项；ADR-004 追加复核注（「v1 不内置 eD2k 栈」的排期部分被取代，哈希转译仍 v2）。
- `PROTO` **2.3 → 2.4**：§1 增 `Emule_Keyword_Key` / `Emule_File_Key`、§2 改双轨 Key、§2.1 两轨合并查询、§4 记映射修订例外。`PROTOCOL_VERSION` 仍 1、T0 golden 未动。
- `PRD §3` 增「索引级双向互通」目标与「共享即公开」注；`SRS` 增 FR-24（关键词检索 + `ed2k://` 链接）/ FR-25（标准 eD2k 下载上传 + 双轨发布）。
- `ARCH` L5 / M-05 / M-12 改为双轨与 eD2k 栈（P0）；`RISK` 更新 R-02 / R-10 / R-14、新增 R-15（零积分排队靠后）；`OPEN-QUESTIONS` 收窄 Q7、D4 注记 Q8、新增 Q9（`ed2k-rs` 复用边界）。

## [2.4.28] - 2026-11-10 — 两进程 A/B 补测通过，**S2 判 `pass`**

### 新增工具（复用同一 `live_gate`，红线不减）

`kad-mini split-probe --stage publish|search`：把 `live` 里同进程的发布与查询拆成两个进程 ——
A（`split-a`）只发布并把「确认地址 + 条目 source id」写进 handoff，B（`split-b`）在**另一个进程**里
用不同源端口、不同本地 KadID 种子、不同 bootstrap 入口重走一遍 tolerance zone，再取回。
未加 attest 时实测被门禁拒绝且 0 报文。

### A 侧（14 分 30 秒 / 324 条报文）

`split-a` = `e465a455…` / 源端口 61195。3 个样本里 1 个收敛到在区（`closest=13,899,520`），
`0x43` → `PUBLISH_RES`（305 ms），handoff 记下 `82.63.191.18:4672` 与 source id `a89b9f21…`；
另 2 个 16 跳后无在区目标，按红线跳过。

### B 侧（5 分 15 秒 / 113 条报文）

`split-b` = `0dab3f81…` / 源端口 **49876** / bootstrap `79.116.53.69:54709`（A 用的是另一台）。
B 自建路由表（`known=194` / `verified=47`）后对同一地址发 `0x33` → `SEARCH_RES`（312.7 ms，2 条答案），
其中一条 source id 与 A 的完全相同，且 descriptor 里写着 `node=split-a;wake_port=61195`。

**判别项**：返回条目的端口是 A 的 61195，B 自己是 49876 —— 条目由 A 创建，B 从第三方索引取回。

### 判定

按项目所有者指令「补一轮 B，成功后直接判 pass」，`S2.status` 登记为 `pass`：

- `tests/spikes/spikes.json`：`S2.status = pass`；新增 `observations.p4_execution_v2427` 与
  `observations.ab_two_process_2026_11_10`（A/B 两侧报告路径、KadID、端口、RTT、判别项）。
- 下游同步：`TECH-STACK §2` 状态注、`ROADMAP` 新增结论条目、`TEST §2`、`RISK R-02`（接纳性成立、污染判据未做）、
  `OPEN-QUESTIONS Q1`（收窄为「按运营速率是否被当污染」，时点改 M4 前）、`ADR-001`（具备定稿条件，状态仍待所有者签署）、
  `tests/spikes/README.md`。
- **判决边界**：只判接纳性（能存、能取）。R-02 的另一半「运营速率下是否被当污染」仍缺实测拒绝率，
  不随本次 pass 关闭。

## [2.4.27] - 2026-11-10 — S2 第七轮：公共 KAD **首次接受并返回**我们的命名空间条目

### 首轮实证（代码零改动，仅重跑）

`live --sample-size 3 --rate-limit 0.2`，G0 参数未动，执行人 attest，门禁 0 blocker。
10 分 53 秒 / 236 条原始记录（`kad-mini/live-run-v2427.json`）。

- `wakeup_req.v1`：`0x21` 收敛到 `closest = 13,899,520 < 2^24`，在区 2 个目标；
  `0x43` → **`PUBLISH_RES`（`load=1`，302 ms）**；`0x33` → `SEARCH_RES`，1 条答案，**命中**。
- `coldrevive-t0-sample.bin`：16 跳后无在区目标，按红线跳过发布。
- `wakeup_ack.v1`：bootstrap `81.33.83.203:4672` 直接超时，0 跳。
- 报文：`0x21` 发 124 / `0x29` 收 101（应答率 81.5%）、`0x43`×2、`0x33`×2、`0x4b`×1、`0x3b`×1。

### 命中已解到底（不是本地回显）

`0xE5` zlib 帧 230 B → 解压 265 B，**265/265 字节零残余**：`target` 与发布的 Key 逐字节相同，
`entry[0].source_id` 与本轮发布条目相同，5 个 tag 全部原样返回，含两个 24 字符自定义名
（`ColdRevive_V1:descriptor`、`ColdRevive_V1:namespace`）。

### 因果证据：同一对端前后对照

`82.63.191.18:4672` 在 v2.4.23 / v2.4.24 对同一 key 的 `0x43` 两次静默；本轮同一台机器、同一 key、
同一份清单，回了 `PUBLISH_RES`。对端、Key、路径都没变，只有 tag 帧头变了（2.4.26）。

### 结论边界（写进 PLAN §20.6–20.7）

- 判据 1/2/4/5/6 满足；判据 3 只有 n=1（1 接受、1 UDP 超时、0 明示拒绝），
  拒绝率 95% 上界 79%，**不足以支撑 R-02「拒绝率 > 20%」的判定**。
- A（发布）与 B（查询）是同一进程、同一 KadID、同一源地址；存储与取回都经第三方节点，
  但"另一台主机能否取回"未测。
- 程序仍固定 `evidence_valid_for_pass=false`（永不自证），`S2.status` 仍 `not_run`，待执行人回填。

## [2.4.26] - 2026-11-10 — 选项 D 定位根因：**Kad tag 帧头格式写错**，五轮 0 回应由此而来

### 定位过程（局域网真实 aMule 当裁判）

- 新增 `lan-index-probe`（`PLAN §18.6` 选项 D）：RFC1918 硬校验 → `BOOTSTRAP` 证明双向可达 →
  `0x43` 发布一个从未存在过的命名空间 Key → `0x33` 取回。`public_kad_contacted=false`，不判 pass。
- 第一次执行：`172.16.29.209:41296` 回 `BOOTSTRAP_RES`（20 联系人 / 15.6 ms），但 `0x43` 仍 `10060`。
  距离 3,428,064,205 虽超 `2^24`，LAN 豁免本应放行 ⇒ 不是公网、不是 firewalled、不是距离。
- 二分（每个变体独立 key）：**0 个 tag 回 `PUBLISH_RES`（load=0），1 个 tag 就静默**；
  单 tag 六种取值（1 字符/43 字符文件名、UINT32、UINT16、长名短值、长名 14 字节）全部静默
  ⇒ 与类型、名字、值都无关，是**帧头**。

### 源码依据

- `Process2PublishKeyRequest` 读 tag 用的是 `bio.ReadTag()` → `CFileDataIO::ReadTag`：
  `type = ReadUInt8(); name = ReadString(false);` —— 名字恒为 `u16 长度 + 字节`，
  且 switch 里**没有** `0x80` 短名分支、**没有** `TAGTYPE_STR1..STR22`、**没有** `TAGTYPE_BLOB`，
  命中外一律 `throw "Invalid Kad tag type"`。
- `FileTags.h`：`#define TAG_FILENAME wxT("\x01")` —— 数值名字在线上就是长度 1 的字符串。
- 我们写的是 ed2k/met 的 `[type|0x80][name:1]`（`CTag::CTag` / `WriteNewEd2kTag` 的格式），
  `0x82` 直接落 `default`；`namespace` 的 14 字节值又被编成 `STR14 (0x1E)`，同样落 `default`。
  异常在 `catch(...) { delete entry; throw; }` 里原样抛出，`PUBLISH_RES` 那一行永远执行不到。

### 修正（v2.4.26）

- `Tag::encode` → `[type][name_len:u16][name][value]`，字符串一律 `TAGTYPE_STRING`，不再自动选 `STRn`；
  `Tag::decode` 同步并显式拒绝 `STRn` / `BLOB`。新增两个单测（字节固定向量 + 拒绝 `0x1E`）。
- `PROTO` 升 **v2.3**：§2.0 静默路径「两条」改「三条」，新增 tag 帧头一行；
  `kad-mini/README.md` §3 把 tag 编码拆成 Kad 路径与 ed2k/met 路径两行。
- 回归：`cargo test` **15/15**、`selftest` **37/37**、`loopback` **5/5 发布 + 5/5 命中**、0 协议错误。

### 实测闭环

同一台 aMule，修好后：`verdict=confirmed_publish_then_retrieve`，`publish_acked=true` `load=1`（5.2 ms），
key 回显匹配；`0x33` 得到 1 条答案（8.5 ms），target 回显匹配、**我们发布的 descriptor 被原样返回**、
`namespace` tag 匹配。即真实 aMule 会收下并服务我们的命名空间条目。

### 更正（撤回）

- **撤回 2.4.24「tag 编码逐字节合法」**：当时对照的是 `CTag::CTag(CFileDataIO&, bool)`（`.met` 路径），
  而 Kad 走 `CFileDataIO::ReadTag`。用 met 的规则解 ed2k 格式当然零残余，但 aMule 不走那个解析器。
- **撤回 2.4.25 §18.5 的 `p^5` 概率论证**：它建立在「报文合法」的前提上，前提已推翻；
  五轮静默的原因是帧头 bug，不是对端 firewalled，也不是公共 KAD 拒绝该命名空间。

### 下一步

重跑公网 P4（G0 参数不变，需执行人重新 attest），预期首次出现 `PUBLISH_RES`；
判据仍为 RISK R-02 三桶计数，`S2.status` 与 `spikes.json` 结论字段仍只由人回填。

## [2.4.25] - 2026-11-10 — S2 第六轮：跳数翻倍（选项 A）被证伪，爬行在 2^23.7–2^25 处饱和

### 执行

- 唯一改动：`MAX_FIND_HOPS` 16 → 32。G0 参数（`sample_size=3` / `0.2 pps` / 明文 `0xE4` /
  跳过 HELLO）一字未改，执行人显式 attest，门禁 `passed=true`。
- 耗时 **31 分 35 秒**、698 条报文（`0x21` 发 375 / `0x29` 收 315，应答率 **84%**）。

### 结果：预期被直接证伪

- 在区目标 **0 / 1 / 0**（§17 用 16 跳拿到 4 个），全轮只有 1 次 PUBLISH，仍 `10060` 超时。
- **同 key 对照**：`fac258d1…` 在 16 跳与 32 跳下 closest 均为 **13,899,520**，
  在区目标仍是同一台 `82.63.191.18:4672`。多给的 16 跳没有拉近 1 个 bit。
- `frontier_widened_hops` 9/13/11（16 跳）→ 19/25/18（32 跳）：绝对数翻倍、比例不降，
  新增跳数全部落在带外盲探上。
- 归因：在区概率 `2^24/2^32 = 2^-8 ≈ 0.39%`，`known` 230–245 的期望在区数 0.90–0.96，
  与实测完全自洽——路由表相对该 key 近似随机，最后 1–2 bit 因末端邻居枯竭而到不了。

### 处置与下一步

- `MAX_FIND_HOPS` **回退为 16**（A 无收益、单轮成本 1.77 倍），常量注释保留本次结论；
  三道过滤（`verified ∧ global IPv4 ∧ top_chunk ≤ 2^24`）未放宽。
- 5 次发布（4 个不同主机）全静默；按 §17.4，非 firewalled 且不超距的节点必然回
  `PUBLISH_RES`，故「对端自判 UDP firewalled」仍是唯一自洽解释（p=0.6/0.7 时全静默概率 7.8%/16.8%）。
  **该混淆不能靠加目标消除**。
- `PLAN.md` 新增 §18，下一步改推选项 D：把 `lan-probe` 扩到 `0x43`/`0x33`，
  用局域网真实 aMule 闭环索引路径（只读证据，`public_kad_contacted=false`，不判 pass）。
- `S2.status` 仍 `not_run`，`spikes.json` 结论字段未回填。

## [2.4.24] - 2026-11-10 — S2 第五轮：前沿护栏生效，首获 4 个在区目标，发布仍 0 回应

### 执行

- G0 参数（`sample_size=3` / `0.2 pps` / 明文 `0xE4` / 跳过 HELLO），执行人显式 attest，
  门禁 `passed=true`。耗时 17 分 53 秒，395 条报文。
- `known` 177–214、`verified` 49–65、`queried` 58–75；`closest_top_chunk` 13.9M–26.6M，
  首次稳定贴在 `SEARCHTOLERANCE`（16,777,216）边界。
- **首次同轮拿到 4 个不同在区目标**并发出 4 次 PUBLISH（`119.112.72.25:39763`、
  `82.63.191.18:4672`、`93.38.125.19:37642`、`85.51.5.149:4673`），**全部 `10060` 超时**；
  4 次 SEARCH 同样超时，0 命中。

### 排除项（离线，未发包）

- **tag 编码逐字节合法**：237 字节 `0x43` 按 `CTag(const CFileDataIO&, bool)` 完整解码，
  5 个 tag 精确消耗 235 字节，零残余、零非法类型；type 在前 + `0x80` 表示 1 字节 name ID
  的顺序与 aMule 一致；tag 计数 u8 与 `bio.ReadUInt8()` 一致。
- **距离判据 chunk 语义正确**：`UInt128.h` 注明 `Get32BitChunk(0)` 取最高 32 位
  （`m_data.u32_data[3 - val]`），与本地 `xor >> 96` 一致；此前怀疑的「按低 32 位判区」不成立。
- **`IsFirewalledUDP` 默认 false**：`m_firewalledUDP = false` 是静态初值，只有节点自己跑过
  UDP 防火墙测试且失败（或超时 6 分钟）才置 true。未测试过的 NAT 后节点会正常处理并回应 PUBLISH，
  因此「公网大多 firewalled 故拒收」的解释强度显著下调。
- **发布响应无条件**：除 firewalled / 超距两条 early return，即使 `AddKeyword` 返回 false 仍发送
  `PUBLISH_RES`（`file(16)+load(u8)`）；第三条静默路径（tag 解析抛异常）已被上条排除。

### 结论与下一步

`0/4` 证明协议实现全链路正确，但不足以判定公共 KAD 是否接纳该命名空间——4 次发布虽为 4 个主机，
仅覆盖 2 个 key，独立试验单位不足。`frontier_widened_hops` 9/13/11 表明 2^24 附近的已知路由表本身稀疏。
`PLAN.md` 新增 §17，列出三个选项（A：跳数 16→32；B：`ALPHA_QUERY` 5→8 + 跳数 24；
C：`sample_size` 3→10，需所有者改 confirmed 值），建议先做 A（纯代码改动）。
`S2.status` 仍 `not_run`，`spikes.json` 结论字段未回填。

## [2.4.23] - 2026-11-10 — S2 第四轮：定位爬行震荡根因，补 Kademlia 前沿护栏

### 定位

- 第四轮（v2.4.22，G0 参数）跑出 448 条报文：`known` 198–231、`verified` 62–70（覆盖修复有效），
  但 `zone_verified` 仍 0/1/0，`closest_top_chunk` 反而更差（38M / 13.9M / 53M）。
- 用该轮原始报文做四项离线复现（未发包）：`0x29` 载荷为 `target(16)+count(1)+25n`（count=11，
  与 aMule 一致，此前「前缀异常」是本人偏移读错）；`check==KadID` 把关使 200 次应答成为 200 次
  真实 KadID 证明；四个 32 位分块中只有 chunk0 有富集（1.14% vs 基线 0.39% = 2.9 倍），端序正确；
  **逐跳还原显示询问距离在震荡**（43M→54M→272M→743M→38M→266M→1342M）。

### 修正

- 根因：v2.4.22 把候选窗口改为「全表最近的未问节点」且**无距离上限**，最近几个联系人 2–3 跳问完
  后只能去够 700M–1400M 的远端节点，形成震荡；报文越多震荡越大，`known` 翻倍而距离变差。
- 新增 `FRONTIER_FACTOR = 8` 前沿护栏：只用「距离 ≤ 8 × 当前最近距离」的未问节点作候选，下限取
  `SEARCHTOLERANCE`；前沿耗尽才放宽一档到全表，并计入新字段 `frontier_widened_hops`。
- 报文预算不变（`ALPHA_QUERY=5`、`MAX_FIND_HOPS=16`），与 v2.4.22 只差选点一个变量。
- `PLAN.md` 新增 §16，并在 §15.5 标注「窗口 30 导致结构性截断」的判断已推翻。

### 回归

`cargo test` 13/13、`selftest` 37/37、`loopback` 3/3 发布确认 + 3/3 命中 + 0 协议错误。
`S2.status` 仍 `not_run`；本轮无 `PUBLISH_RES`。

## [2.4.22] - 2026-11-10 — H2 源码逐字段比对：编码无误、H1 被证伪，且「搜索静默是设计要求」

### 方法

纯离线核对 aMule `master` 的 `CSearch::StorePacket`、`Process2SearchKeyRequest`、
`Process2PublishKeyRequest`、`Defines.h`、`CIndexed::SendValidKeywordResult`。未发送任何报文。

### 结论一：载荷无偏差，H2 不成立

`SEARCH_KEY_REQ` = `target(16) + u16(0)`；`PUBLISH_KEY_REQ` = `file(16) + count(u16) + [sourceID(16) + tagCount(u8) + tags]`；
`SEARCHTOLERANCE = 16777216`、`K = 10`、`ALPHA_QUERY = 5`、距离比较取最高 32 位——**全部与我们一致**。
此前怀疑的「常量误读」也不成立。

### 结论二：H1 被证伪，不需要放宽 `protocol_scope`

两个处理函数都不查路由表、不校验发送方是否为已知 contact。发布的前置条件只有两条：
对端自身 `IsFirewalledUDP(true)`，以及 `top_chunk(对端KadID, Key) > SEARCHTOLERANCE`。
**「必须先 HELLO 互认」不成立**，`HELLO` 路径无须实现。

### 结论三：更正 §14.4 ——搜索静默是设计要求

`CIndexed::SendValidKeywordResult` 把整个响应构造放在 `if (find(keyID) != end())` 内，
miss 时只执行末尾的 `Clean()`，**一个字节都不回**。我们搜的是无人在线的私有命名空间 Key，
所以 `0x33` 的 0/11 与 9 个探针的 0/9 是必然结果。

因此 v2.4.21 的「请求类别级门禁」结论**撤销**：`p = 3.7e-14` 的检验不成立，
因为它比较的两类操作里，search 这一侧的信道本身就是关闭的。

### 代码

- 候选窗口 `ALPHA_QUERY * 6`（=30）→ **整张表**。这是 v2.4.21 的结构性上限：
  第 7 跳退出、`queried = 33`，而 `known` 里还有 138–165 个从未问过的联系人被窗口挡在外面，
  `closest_top_chunk` 因此卡在 ~2^23——正好差一跳跨过 2^24。
- 删除 `index_op_probe`：它测不出任何东西，只会再次误导判读。

### 文档

- `PLAN.md` 新增 §15（逐字段比对表、H1 证伪、三条静默分支、更正与下一步），并在 §14.4 顶部加更正指引。
- `PROTO.md` 升 2.2，§2.0 增三行源码核实结论：「查询必有响应」不成立、「发布必有响应」有两条静默丢弃、
  索引操作不依赖 `HELLO` 互认。这三条直接影响产品侧的超时与冗余策略。

`S2.status` 仍为 `not_run`。

## [2.4.21] - 2026-11-10 — S2 P4 第三轮：验证覆盖修复成功，索引操作 0/19 静默，样本量被证伪为自变量

### 决策：不扩大样本，先修覆盖

「加大样本能否提高成功率」用前两轮数据即可回答：每样本 `verified` 仅 9.2（`known` 却有 86–127），
`P(样本含 ≥1 在区目标) = 0.5`，每样本发布 0.5 次。`sample_size` 3→10 只是把期望发布次数从 1.5 提到 5，
而每次仍有一半概率发不出包。另有硬约束：`live_gate` 要求 CLI 值等于 `confirmed.sample_size`，
`--sample-size=10` 实测被拦（`与 G0 确认值 3 不一致`），改值属所有者确认参数的变更。

### 代码

- **plateau 不再是退出条件**：原先连续两跳零增长即 break，16 跳预算只用 3–5 跳，
  110–130 个联系人里只验证了 ~10 个。现在只在「跳数用尽」或「候选耗尽」时退出。
- **D1 距离诊断**：`closest_top_chunk` / `closest_verified_top_chunk` / `queried_contacts` / `stale_hops`。
- **D2 索引操作探针**：`index_op_probe_*`，向 ≤3 个「已验证据、但不在区」的节点各发一个 `SEARCH_KEY_REQ`（只发查询，不发发布）。
- **应答来源校验**：`exchange` 只接受来自被问地址的报文（最多轮询 4 次）。
  原实现接受任意来源，迟到的 `0x29` 会被解成「expected 0x43, got 0x29」，迟到的 `0x43` 会被记到错误目标头上。

### 执行与结果（`sample_size=3` / `0.2 pps` / 明文 `0xE4` / 跳过 HELLO）

| seq | hops | known | verified | zone_verified | closest_top_chunk | PUBLISH | SEARCH | 探针 |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| 0 | 7 | 165 | 29 | 1 | 9,826,055（在区） | 1 发 0 回 | 0 回 | 3 发 0 回 |
| 1 | 7 | 138 | 27 | 1 | 13,899,520（在区） | 1 发 0 回 | 0 回 | 3 发 0 回 |
| 2 | 7 | 162 | 30 | 0 | 31,927,299（越界） | 跳过 | — | 3 发 0 回 |

覆盖修复生效：单样本 `verified` 8–12 → 27–30（约 3 倍），`known` 86–127 → 138–165。
结果仍为 0：`0x33`×11、`0x43`×2、探针×9，全部静默。

### 关键判读

三轮合计：`0x21`→`0x29` **86/99 = 87%** 应答；索引操作（`0x33`/`0x43`）**0/19 = 0%**。
Fisher 精确检验双侧 **p = 3.7e-14**。同一批活节点、同一源端口、同一加密信封，
回答节点查找而拒绝索引操作 → 这是**请求类别级门禁**，与目标选择、网络、报文可解密性均无关。
**继续加大样本只会重复同一个 0/N。**

### 待决

H1（索引操作要求先 `HELLO` 互认）与 H2（`0x33`/`0x43` 载荷与 aMule 解析器不一致，
回环自洽无法证伪）均需触碰 `HELLO` 路径，而 G0 确认的 `protocol_scope` 明写「跳过 HELLO」，
需项目所有者先书面放宽。`S2.status` 保持 `not_run`。

## [2.4.20] - 2026-11-10 — S2 P4 两轮实网执行：目标筛选生效，但在区节点策略性不应答

### 执行

两轮均按 G0 参数（`sample_size=3` / `0.2 pps`）、执行人显式 attest，明文 `0xE4`，bootstrap 10 节点。

| 轮次 | seq | known | verified | zone_verified | hops | PUBLISH |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| v2.4.19 | 0/1/2 | 103 / 88 / 114 | 8 / 8 / 9 | 0 / 1 / 1 | 3 / 3 / 2 | 0 确认（2 次 `10060`） |
| v2.4.20 | 0/1/2 | 113 / 86 / 127 | 11 / 7 / 12 | 0 / 1 / 0 | 5 / 3 / 4 | 0 确认（1 次 `10060`） |

`SEARCH_KEY_REQ` 与 PUBLISH 同结果：全部超时，0 命中。

### 代码（v2.4.20 相对 v2.4.19）

- `MAX_FIND_HOPS` 10 → 16；新增 `MIN_ZONE_TARGETS = 3`：单目标时一个策略性丢弃就等于
  零信息样本（v2.4.19 两次都撞上这一点）。
- 每跳候选改为「**未查询过的**最近 `ALPHA_QUERY` 个」：原先取原始最近 N 个会被 `seen` 跳过，
  整跳预算可能全花在已问过的节点上并误判为 plateau。
- plateau 改为**连续两跳零增长**才停。

### 观测与判读

- 目标筛选生效：不再盲发最近 10 个，`known` 由 30–34 提升到 86–127，`verified` 7–12。
- 在区命中仍为 0–1，与「按随机联系人」的期望一致（127/256 ≈ 0.5），说明漏斗尚未真正贴近 key。
- 同一个在区节点 `82.63.191.18:4672` 两轮均「答 `0x21`、不答 `0x43` 与 `0x33`」——
  非死节点，属**策略性不应答**索引操作。
- 因此 0/N **不属于索引层拒绝，也不属于报文问题**，而是目标侧服务策略；详见
  [PLAN.md §13](tests/spikes/s2/PLAN.md)。

### 状态不变

`S2.status = not_run`；两次 PUBLISH 均未获得 `PUBLISH_RES`；0 命中；`PROTOCOL_VERSION = 1`；
T0 golden 未动；`spikes.json` 结论字段未回填（只由人填写）。下一步 D1（距离诊断）/
D2（陌生节点搜索对照）见 PLAN §13.3。

## [2.4.19] - 2026-11-10 — S2 发布目标筛选：只发给「已验证在责任区内的应答者」

### 依据

v2.4.17 实网记录：seq0 / seq1 **已收敛**但 publish / search **0/10 全超时**，同版本回环 3/3 + 3/3 证明
wire 正确。结论：0/10 不是 codec 问题，而是目标选择问题 —— 「取 known 里 XOR 最近的 10 个」必然混入
① KadID 陈旧（eMule 换 ID 后仍应答，aMule 以 `check` 是否等于自身 ID 判定，不等即静默丢弃）与
② UDP firewalled（`Process2PublishKeyRequest` 直接丢弃）两类不接收发布的节点。

### 改动

- `live` 新增 `verified: HashSet<SocketAddr>`：任何成功的 `0x21 → 0x29` 即证明该地址真实 KadID
  等于其广告 ID（`check` 必须等于接收方自身 KadID，否则 aMule 静默丢弃）。
- 发布 / 搜索目标改为 `verified ∩ 全局 IPv4 ∩ top_chunk(id,key) ≤ SEARCHTOLERANCE`，上限
  `PUBLISH_FANOUT`；删除「最近 N 个」兜底。找不到时**跳过样本并写明原因**，不盲发。
- 循环条件改为「直到出现 verified 在区节点或路由表停止增长」，配合 2.4.18 的 plateau 修正。
- 报告新增 `verified_contacts`、`zone_verified_contacts`、`publish_target_policy=verified_in_zone`。
- [PLAN.md §12](tests/spikes/s2/PLAN.md) 记录依据、改动、判据口径与残余风险。

### 回归

`cargo test` 13/13、`selftest` 37/37、`loopback` 3/3 发布确认 + 3/3 命中 + 0 协议错误；
`live --check` 唯一 blocker 仍为执行人确认。

### 状态不变

`S2.status = not_run`；本轮**未发送任何报文、未接触公共 KAD**；`PROTOCOL_VERSION = 1`；T0 golden 未动。

## [2.4.18] - 2026-11-10 — 法务送审材料包 + 状态流转矩阵；修 S2 收敛自截断与测试编译错误

### 文档

- 新增 [DOC-21 法务评审材料包](docs/07-operations/LEGAL-REVIEW.md)（`草稿`）：汇编
  [TECH-STACK §2 S8](docs/02-architecture/TECH-STACK.md)、[OPS §6](docs/07-operations/OPS.md)、
  [RISK R-11](docs/05-quality/RISK.md)、[Q8](docs/06-plan/OPEN-QUESTIONS.md)；固化 KAD **实发**描述符字段
  （明文文件名 / 文件大小 / 源端口 / descriptor / namespace）、七项默认配置实体、L1–L6 六个待法务确认问题、
  结论登记区与 G0/G4 对应关系。
- [docs/README.md](docs/README.md) 新增 **§4.1 状态流转矩阵**：明确「已审定」不是合法状态值、终态为
  `有效`；列出四条流转边与必要条件；规定改状态须**原子同步四处**（front matter + 正文状态行、
  本文件 §3 表、`index.html` 的 `DOCS`、`CHANGELOG`）。
- `docs/index.html` 的 `DOCS` 清单同步增补 DOC-21。

### M0 / S2 代码

- **修测试编译失败**：`deobfuscate` 在 2.4.17 改为返回 `(inner_frame, peer_verify_key)`，两处 `assert_eq!`
  未同步，`cargo test` 直接 `E0308` 编译错误 —— **13 个用例一条都跑不起来**，2.4.17 声称的
  「`cargo test` 通过」无法复现。已改为比较 `back.0`。
- **修收敛自截断缺陷**：`live` 的 plateau 判据原本建在 `in_zone` 上。未收敛时 `in_zone` 每跳恒为 0，
  `hops >= 2` 立刻 break —— 2.4.17 设的 `MAX_FIND_HOPS = 10` 实际**永远只走 2 跳**，这正是两次 P4
  重跑停在 30–34 联系人的原因。改为以路由表 `known` 是否增长为 plateau 判据，循环条件改为
  `in_zone.is_empty()`，`SEARCHTOLERANCE` 漏斗才真正可走。
- 回归：`cargo test` **13/13**、`selftest` **37/37**、`loopback` **3/3 发布确认 + 3/3 命中 + 0 协议错误**。
- `live --check` 现只剩一个 blocker：`--attest-i-am-the-experiment-operator`；
  `g0_confirmed sample_size=3 / rate_limit_pps=0.2`、bootstrap 10 节点公网 IPv4 校验通过。

### 状态不变

`S2.status = not_run`；本轮**未发送任何报文、未接触公共 KAD**；`PROTOCOL_VERSION = 1`；T0 golden 未动；
S8 的所有者豁免未变为书面法务意见。

## [2.4.17] - 2026-11-10 — 多目标扇出 + 混淆信封字节级对齐（补记）

> 本条为补记：提交 `53c87f7` 当时未写入本文件。

- 收敛参数：`MAX_FIND_HOPS = 10`、`ALPHA_QUERY = 5`、`KADEMLIA_FIND_NODE = 0x0B`、`PUBLISH_FANOUT = 10`。
- publish / search 改为向最近的 K 个已知节点多目标扇出，避免单点陈旧 KadID 导致的静默丢弃。
- 新增 `obfuscate` / `obfuscate_verify` / `deobfuscate` 混淆信封，字节级对齐 aMule（回环 3/3 发布确认、
  3/3 搜索命中、0 协议错误）。
- 遗留：`deobfuscate` 返回值改为元组后测试未同步，`cargo test` 编译失败，已于 2.4.18 修复。

## [2.4.14] - 2026-11-10 — 根因归位：联系人 IP 字节序缺陷，高位 UDP 从未被阻断

> **本条撤回 2.4.9–2.4.13 的主要环境结论。** 它们保留为审计轨迹，不再作为决策依据。

### 触发证据

项目所有者提供 LAN eMule 主机**运行时**三张截图：当日 11:12 新加的下载以 MB/s 传输、
展开来源含多行 `Kad` 与 `eD2K Se...`；Kad 页签 `当前搜索 (1)` 关键字 `windows`、
**数量 93 / 已发送包 7|2 / 回应总数 600**；`当前搜索 (2)` 两条 `节点查找` 回应总数 2 与 1。
按 [eMule 官方 Kad 窗口](https://www.emule-project.com/home/perl/help.cgi?l=1&topic_id=1266&rm=show_topic)
与[搜索详情图](https://www.emule-project.com/home/perl/help.cgi?l=1&rm=show_topic&topic_id=1552)，
这些是远端节点经 UDP 送回的应答 —— 与「整网过滤高位 UDP」不相容。

### 根因：把联系人变成 UDP 目的地时读反了 IP 字节序

| 源码 | 事实 |
| :--- | :--- |
| `src/NetworkFunctions.h` | `KadIPToString(ip)` 以 `ip>>24` 为首字节 → KAD 内部 IP 为 **host order** |
| `src/KadDlg.cpp` | 注释「StringIPtoUint32 returns anti-host and **kad expects host order**」 |
| `src/SafeFile.cpp` | `WriteUInt32` / `ReadUInt32` 在 x86 上是**小端** |
| `KademliaUDPListener.cpp` | `SendPacket` / `OnPacketReceived` 用 `wxUINT32_SWAP_ALWAYS` 双向转换 |

故 `a.b.c.d` 的 host-order 整数以小端写出后，线上 4 字节是 **`d.c.b.a`**。真实
`nodes.dat`（200 条）反序解码后 **200/200 全局可路由**、首字节集中在宽带 ISP 段；
原样解码则有 19 条 RFC1918 与 225/236/246/253 等组播/保留首字节。自 P4 首次执行起，
所有 0/N 都是把 `BOOTSTRAP_REQ` 发给了不存在的地址。

另核实 `Process2BootstrapRequest` 不要求 payload / KAD ID / HELLO / UDP key，
我们 2 字节 `0xE4 0x01` 报文**本身合法**，不是报文格式问题。

### 修复与复验

- `nodes_dat_to_bootstrap.py`、`bootstrap_res_to_targets.py` 新增 `decode_kad_ip()`
  反转 4 字节并加 `--selftest`；`kad-mini/src/wire.rs` 的 `Contact::encode/decode`
  统一「内存点分顺序 / 线上反序」，新增 `contact_ip_wire_order_is_reversed` 单测。
- 重新生成三份 gitignored 目标清单；`cargo test` **8/8**、两个脚本 selftest 通过。
- 修正后本机 `kad_egress_probe.py` 10 目标 / 0.2 pps：**9/10 收到 523 B `BOOTSTRAP_RES`**、
  `RESULT: REACHABLE` 退出码 0（1 条 `WSAECONNRESET`）。**高位 UDP 一直是通的**；
  STUN 3478/19302 超时只是 STUN 专属过滤，不能当 KAD 代理指标。
- P4 按 G0 参数（`sample_size=3` / `rate_limit_pps=0.2`）**两度重跑**：
  `public_kad_contacted=true`，收到 517 B `0xE5` zlib 压缩 `BOOTSTRAP_RES`、
  20 联系人及 `0x21/0x29` 迭代应答，3 hops / 30–34 联系人；两次均
  `tolerance_reached=false` → 3/3 样本按 criteria #2 跳过，**未发布、未搜索**。

### 撤回与保留

- **撤回**：「出口按目标端口/应用白名单过滤高位 UDP」「整网无主机可跑 P4」
  「本机已确证不可达」「换网重跑 P4」；RISK R-02 中历史超时由「环境前置条件失败」
  改判为**测错对象**，整体作废、不进入任何桶。
- **保留**：STUN ≠ KAD；KAD UDP「打开」是入站可达性测试、入站 DNAT 即可满足；
  eD2k 下载/服务器搜索走 TCP，与 KAD UDP 是不同判据。

### 新的关键环节

瓶颈转移到客户端的 **`SEARCHTOLERANCE` 路由收敛**。`live` 每跳只取 2 个候选、最多 3 跳，
累计 30–34 联系人；aMule `SEARCHTOLERANCE = 2^24` 比较 XOR 距离**最高 32 位**
（`UInt128.h` 的 `Get32BitChunk(0)` 为最高位），随机命中约 1/256。
`Constants.h` 定义 `KADEMLIA_FIND_VALUE=0x02` / `STORE=0x04` / `FIND_NODE=0x0B`、
`ALPHA_QUERY=5`；当前 `0x21` 首字节用了非标准的 5。修复方向：每跳 `ALPHA_QUERY`
个最近候选 + `FIND_NODE=0x0B` + 提高跳数，再用实时 `BOOTSTRAP_RES` 目标重跑 P4。
`nodes.dat` 已过期（3 条中 2 条无响应），不再作为首选清单。

### 登记范围

`PLAN.md` 新增 §11（含撤回表）并改顶部状态；`spikes.json` 新增
`p4_environment_diagnosis.contact_ip_byte_order_defect_2026_11_10`，改写
`conclusion` / `required_next_step`，并把 `p4_execution.result` 改为
`inconclusive_targeting_defect`；RISK R-02、OPEN-QUESTIONS、ROADMAP、根 README、
各 README、两个诊断脚本文档字符串与提示语、`docs/index.html` 重建。公共文档不含
LAN IP / 端口 / 公网 bootstrap IP。

**状态不变**：`S2.status = not_run`、`PROTOCOL_VERSION = 1`、T0 golden 未动、
两次 P4 重跑 `evidence_valid_for_pass=false`、**未发布任何测试 Key**、G0 仍 False。

## [2.4.13] - 2026-11-10 — 跨主机 KAD 探针 0/3 + eD2k/KAD 判据区分：工程决策改为换网重跑

### 新增证据

- 项目所有者在 LAN eMule 机器（同网段、开了入站映射）执行 §8.6.3 的
  [`kad_egress_probe.py`](tests/spikes/s2/kad_egress_probe.py)：3 个目标全部
  `TimeoutError: timed out`，`RESULT: NO_REPLY (0/3)`、退出码 2。主机身份经确认；
  本机 Desktop 无该脚本 / 目标文件 / 报告目录，报告路径在该机 `Desktop\reports\`。
- 所有者补充「该机 eMule 下载和搜索都很正常」，并提出可现场开下载验证。
- 新增 [`bootstrap_res_to_targets.py`](tests/spikes/s2/bootstrap_res_to_targets.py)：
  只读解析 LAN `BOOTSTRAP_RES`（`0xE4`/`0xE5` + `0x09`，20 × 25 B 联系人），
  过滤公网 IPv4、去重、产出新鲜目标；**不发任何包**。03:12 UTC 重跑 `lan-probe`
  得 523 B / 20 contacts / RTT 12 ms，解出 15 个公网 IPv4，取前 10 个
  （5 × `4672` + 5 × 其他高位端口），与旧 3 目标**重叠 0/10**；本机宽扫 **0/10**。

### 两处方法学修正

1. **两次 0/3 不是独立复现**：本机（三种源端口）与该机共用同一份 3 个目标，
   换主机 / 换源端口但没换目标 —— 若这 3 个联系人已离线，两台主机都会 0/3。
   跨主机结果压低了「按主机例外」，没有压低「死联系人」；新鲜 10 目标宽扫把
   本机侧这一混淆大幅削弱。
2. **「下载/搜索正常」不构成 KAD 出站证据**：按 eMule 官方
   [Network Guide](https://www.emule-project.com/home/perl/help.cgi?l=1&rm=show_topic&topic_id=232)
   与 [Search → Method](https://emule-project.com/home/perl/help.cgi?l=1&rm=show_topic&topic_id=133)，
   下载本身「与两个网络的选择无关」、走 TCP；`Server` / `Global (Server)` 搜索逐个查询
   eD2k 服务器（TCP）；只有 `Kademlia` 方法要求 UDP 通（官方原话：热门关键词都无结果时
   「系统的 UDP 端口很可能被防火墙或路由器阻断」）。HighID 也只证明 TCP 回连 + 入站 DNAT。
   因此一条完整健康的 eD2k 路径可以**完全不产生出站高位 UDP**，与 0/3、STUN 0/3 不冲突。
   → 验证应改为「搜索 Method 明确选 `Kademlia` 搜热门词」，**不必开下载**。
   另注意入站 KAD 请求触发的 UDP 回包属已建立流，计数增长不是充分证据。

### 结论

**工程决策与科学归因解耦**：本网没有任何独立客户端进程被证明能完成一次公网 KAD UDP
往返（本机 0/3 三种源端口 + 0/10 新鲜目标；该机 0/3）。无论剩余假设
① 按应用放行 / ③ 另一出口或代理 / ④ 出站统一过滤 + DNAT 产生「打开」哪个为真，
ColdRevive 自己的客户端在本网都跑不了 P4 —— 差别只在「换网」还是「为本项目进程单独
申请出站例外」。**P4 应换网重跑，不再等待归因收敛**；Kademlia 方法搜索、该机新鲜宽扫、
统计计数只用于收窄归因，不阻塞。

修订：[PLAN.md §10](tests/spikes/s2/PLAN.md)（新增）、顶部状态、§8.6.2/§8.6.3、§9.3；
`spikes.json` 新增 `cross_host_kad_probe_2026_11_10` 并修订 `conclusion` /
`cross_host_result` / `decisive_checks` / `required_next_step`；RISK R-02、
OPEN-QUESTIONS、ROADMAP、根 README 与各 README、两个诊断脚本的说明与提示语、
`docs/index.html` 重建。公共文档不含 LAN IP / 端口 / 公网 bootstrap IP。

状态不变：`S2.status=not_run`、`PROTOCOL_VERSION=1`、T0 golden 未动、三桶整体作废、
未发布任何测试 Key；本节全部观测 `evidence_valid_for_pass=false`。

## [2.4.12] - 2026-11-10 — `Kad 网络` 面板证据：否证 firewalled 假设，但「打开」是入站测试

项目所有者补上 2.4.11 要求的第 ① 项：LAN eMule 主机 `Kad 网络` 面板完整截图，
`状态: 打开`、`UDP 状态: 打开`、`IP:端口: <公网 IPv4>:<端口被裁切>`、`ID: <数字 ID>`，
**无 `防火墙` / `Buddy` 行**（公共文档已脱敏）。eMule 官方帮助把 KAD 状态区分为「打开」与
「通过防火墙」，aMule 官方手册把 `Connection State` / `UDP Connection State` 定义为
`OK` / `Firewalled` 且两者均 OK 时不显示 Buddy 行。

### 否证与更正

- §8.6.2 假设 ②「该机 KAD 已 firewalled / 有 Buddy、432 条为陈旧路由表」**被否证**；
  v2.4.10「更可能是 LowID / firewalled」的推测撤回；[PLAN.md §8.2](tests/spikes/s2/PLAN.md)
  对应表述同步改写。
- v2.4.10 的「整网无主机可跑 P4」仍维持 2.4.11 的降级结论，**不因面板而回升**。

### 源码级复核：为什么「打开」不等于能跑 P4

按 §0 同一 aMule 源码族逐处核实（详见 [PLAN.md §9](tests/spikes/s2/PLAN.md)）：

| 位置 | 事实 |
| :--- | :--- |
| `ClientList.cpp::DoRequestFirewallCheckUDP` | 只用联系人 **TCP 端口**建连（`KS_QUEUED_FWCHECK_UDP` → `TryToConnect`），**不发** UDP 到其 KAD 端口 |
| `OP_KAD_FWCHECKUDP_REQ` 处理 | 远端把一个**无载荷入站 UDP 包** `KADEMLIA_FWCHECKUDP_RES` 发到请求方 `GetIP():GetKadPort()` |
| `KademliaUDPListener.cpp::Process2FirewallUDP` | 端口匹配即 `SetUDPFWCheckResult(true, false, ...)` |
| `UDPFirewallTester.cpp` | `m_firewalledUDP` **初值即 `false`**；全失败才置 `true`；6 分钟超时降级还要求 `CKademlia::IsFirewalled()` |

→ KAD UDP「打开」测的是**入站可达性**，由入站 DNAT 即可满足，协调走 TCP，**无需任何出站
高位 UDP**。因此「该机 STUN 高位 UDP 全超时」与「KAD UDP 打开」并不矛盾，不必引入按应用 /
DPI 放行；「打开」也可能是未核实的乐观默认值或历史状态残留。432 条联系人是路由表而非流量
计数，面板公网 IP 亦可由 eD2K HighID 提供，均不证明发生过公网 KAD UDP 往返。

### 假设表与决定性核实

[PLAN.md §9.2](tests/spikes/s2/PLAN.md) 更新为：① KAD 真活（仍开放）、② firewalled/Buddy
（**否证**）、③ 另一条出口 / 代理（仍开放）、④ 新增：出口统一过滤出站高位 UDP + 入站 DNAT
产生「打开」（无法排除，与全部观测一致）。

仍需项目所有者：① 在 LAN eMule 机器运行
[`kad_egress_probe.py`](tests/spikes/s2/kad_egress_probe.py)（退出码 `0` = 充分证据；`2` 在存在
按应用放行时**不是充分否证**）；② 查看 `统计信息` 中 KAD 收发包计数是否增长。计数增长 +
探针 `2` 表示按应用 / DPI 放行；计数不增长 + 探针 `2` 才支持本网无可用出口。

### 变更与状态

`PLAN.md` 新增 §9（面板转录、源码表、假设表、核实步骤），修订顶部状态 / §8.2 / §8.6.2；
`spikes.json` 新增 `observations.p4_environment_diagnosis.kad_panel_evidence_2026_11_10`，
修订 `status_note` / `conclusion` / `cross_host_result.implications` /
`kad_specific_egress_2026_11_10` 的假设表与决定性核实 / `required_next_step`；RISK R-02、
OPEN-QUESTIONS、ROADMAP、各 README 同步；`docs/index.html` 重建。

**状态不变**：`S2.status = not_run`、`PROTOCOL_VERSION = 1`、T0 golden files 未动、
三桶计数整体作废、未发布任何测试 Key；本面板为环境诊断证据，`evidence_valid_for_pass = false`。

## [2.4.11] - 2026-11-10 — 新证据：STUN ≠ KAD，归因收窄并新增 KAD 专用探针

项目所有者提供 LAN eMule 主机截图：eD2K 服务器连接为 **HighID**，Kad 页签 **节点 (432)**、
联系人版本 8/9/10。同时本机 `lan-probe` 早先已从该客户端 `BOOTSTRAP_RES` 解出 20 个联系人
（17 个公网 IPv4，多条使用标准 `4672`），与 2026-11-08 `nodes.dat` 中 P4 用的 3 个联系人
完全不重叠。这使 v2.4.10 的绝对措辞不再成立：**STUN 超时只能说明 STUN 被过滤，不能推出
KAD 报文也被过滤**；出口策略可能按应用 / DPI、按目标端口或按「主机 IP + 源端口」放行。

### 概念澄清

HighID 是 eD2K 服务器分配的客户端 ID，证明 DNAT 让服务器能回连 TCP；KAD 有独立的
「状态 / 防火墙 / Buddy」，截图里 `Kad 网络` 面板被裁掉。Kad 页签 432 条是**路由表联系人**
（`nodes.dat` 载入 + 入站请求学习），不等于此刻存在双向 UDP 流量；`0(8)` 这类值是联系人
类型与 KAD 协议版本，不是 HighID。

### 新增诊断与本机实测

新增 [`kad_egress_probe.py`](tests/spikes/s2/kad_egress_probe.py)：只发 2 字节 `0xE4 0x01`
`BOOTSTRAP_REQ`，不发布 Key、不发 HELLO / REQ / SEARCH；目标由用户提供且硬校验为公网 IPv4、
不内置；G0 参数 3 条 / 0.2 包每秒、逐次留原始报文；报告写入 gitignored `reports/`。

| 本机源端口 | KAD 响应 |
| :--- | :--- |
| 系统临时端口 | **0/3** |
| 固定为 LAN eMule 的 KAD 源端口 | **0/3** |
| 固定标准 eMule UDP 端口 `4672` | **0/3** |

→ **本机确实无法与公网 KAD 节点完成一次往返**，不是「只有 STUN 不通」、不是「P4 只挑到死
联系人」、不是源端口策略；§7 对本机的环境阻断结论成立。

### 归因收窄与待核实

[PLAN.md §8.6](tests/spikes/s2/PLAN.md) 记录三种未排除的可能：该机 KAD 确实存活（出口按应用 /
按 IP+源端口放行，STUN 是假阴性）、KAD 已 firewalled 且 432 条为陈旧路由表、eMule 走另一条
出口 / 代理。`spikes.json` 新增 `kad_specific_egress_2026_11_10`，并把
`cross_host_result.conclusion` 与 `required_next_step` 改为非绝对表述；OPEN-QUESTIONS /
ROADMAP / 各 README / RISK R-02 同步；`docs/index.html` 重建。

**v2.4.10 的「整网无主机可跑 P4」降级为「本机已确证不可跑；LAN eMule 主机待核实」**。
决定性核实：① 提供 `Kad 网络` 面板完整截图（状态 / 防火墙 / Buddy / 节点数）与 KAD 收发包
计数是否增长；② 把 `kad_egress_probe.py` 与本地 `bootstrap.live.local.json` 拷到该机运行，
退出码 0 / 2。

**状态不变**：`S2.status = not_run`、`PROTOCOL_VERSION = 1`、T0 golden files 未动、
三桶计数整体作废、未发布任何测试 Key。

## [2.4.10] - 2026-11-10 — 跨主机 STUN 对照：本机与 LAN 主机同被阻断

项目所有者把同一份 [udp_egress_check.py](tests/spikes/s2/udp_egress_check.py) 拷到 LAN 那台
eMule 机器（与本机同网段、同一网络环境，**开了入站映射**）上运行，结果：

- 低位对照 **4/4 OK**：DNS/53 ×2、NTP/123 ×2；
- 高位探测 **0/3 OK**：STUN/3478 ×2、STUN/19302 ×1 全部 `TimeoutError`；
- `RESULT: BLOCKED`，退出码 **2**，与本机完全一致。

**结论（按 STUN 口径）**：v2.4.9 留下的「出口例外是否按主机」开放问题被否证 —— 入站 DNAT
不产生 STUN 出站例外，两台主机对 STUN 高位端口同样不通。
> v2.4.11 更正：此处「高位 UDP / 白名单」结论只对 STUN 成立，不能外推到 KAD 报文；
> 「本网无主机可跑 P4」已降级为「本机已确证不可跑；LAN eMule 主机待核实」，见 2.4.11。

### 变更

- [PLAN.md §8.4](tests/spikes/s2/PLAN.md) 由「待执行」改为「已执行 + 结论」，§7 / §8 标题与
  §8.2 条件推论同步收敛；新增输出原文与判定表。
- `spikes.json` 新增 `observations.p4_environment_diagnosis.cross_host_result`，并更新
  `status_note` / `conclusion` / `open_question` / `cross_host_test` / `required_next_step`。
- [RISK R-02](docs/05-quality/RISK.md) 补记「逐主机判定已于本网两台主机实测、均为阻断」。
- 诊断脚本文档字符串与提示语改为「已闭环」措辞，保留可搬运复用的跨主机用法。
- S2 / kad-mini / tests-spikes README、ROADMAP、OPEN-QUESTIONS 去掉「待验证」表述；
  `docs/index.html` 重建。

**唯一有效路径**：换到高位 UDP 未被过滤的网络 → 诊断退出码 0 → 刷新 `nodes.dat` →
按 G0 参数（`sample_size=3` / `rate_limit_pps=0.2`）重跑 P4，再由实验执行人手工回填 `S2.status`。

**状态不变**：`S2.status = not_run`、`PROTOCOL_VERSION = 1`、T0 golden files 未动、
三桶计数整体作废、未发布任何测试 Key、公共 KAD 未获得任何协议证据。

## [2.4.9] - 2026-11-10 — 环境归因复核：按端口白名单 + 跨主机待验证

项目所有者指出 LAN 内那台 eMule 与本机同一网络、没有额外设置，唯一差异是 eMule 开了
入站映射，据此质疑 v2.4.8「网络统一过滤公网高位 UDP」的归因。补做更宽的对照实验后
**修正归因并升级诊断脚本**；协议设计基线、`S2.status` 与三桶作废结论均不变。

### 复核观测

| 探测 | 结果 | 排除项 |
| :--- | :--- | :--- |
| UDP/53 DNS、UDP/123 NTP（Cloudflare / pool.ntp.org） | OK，42～190 ms | 「UDP 全封」「整张网只走 HTTP 代理」不成立 |
| UDP/3478 / 19302 STUN（Cloudflare、ekiga、Google） | 全超时 | 非 Google 单一来源混淆；高位未知端口不通 |
| 同批 STUN 固定源端口（含 LAN eMule 端口与 4672） | 仍全超时 | 按源端口放行 |
| Windows 防火墙出站阻断规则 | 0 条，默认允许 | 本机防火墙 |
| LAN eMule 高位 UDP，同一帧 `e401` | OK 523 B / 9 ms | 本机 socket 栈与 LAN 路径 |

**修正后的归因**：公网出口按目标端口/应用白名单放行低位知名端口、丢弃高位未知端口，
不是 UDP 被整体过滤。撤回 §7「整张网只走 HTTP 代理出网」的绝对措辞。

### 三点更正

1. **LAN 探测不经过公网出口**，只证明报文格式与 LAN 路径正确，**不能**作为公网 UDP 可达性证据 —— 这是原归因的逻辑漏洞。
2. **入站映射不是请求/响应的必要条件**：aMule `Process2BootstrapRequest` 把响应发回请求的 `ip, port`，状态化 NAT 自动放行；`ProcessPacket` 唯一源端口检查是拒绝来自 53 的未加密包。映射决定的是「能否被外部主动联系」，且很可能是同一条按 IP 出口策略的**表征**而非机制。
3. **`nodes.dat`（2026-11-08）新鲜度是独立次要风险**：3 个随机联系人在健康网络上也可能全部离线，原先判为「非主因」证据不足。

### 新增 / 变更

- [udp_egress_check.py](tests/spikes/s2/udp_egress_check.py) 升级为可搬运的单文件诊断：
  新增 NTP/123 低位对照、非 Google STUN 目标、`--source-port`、`--lan-ip/--lan-port`
  （RFC1918/loopback 硬校验、绝不向公网发 KAD 包）、`--json`；退出码 `0`/`2`/`1` 语义不变。
  本机复验：低位 **4/4 OK**、高位 **0/3 OK**、LAN 对照 OK、退出码 **2**。
- [PLAN.md §8](tests/spikes/s2/PLAN.md) 记录复核与**决定性跨主机实验**：把该脚本拷到 LAN
  eMule 机器运行；退出码 0 → 出口例外按主机，可从该机重跑 P4 或为本机申请同策略；
  退出码 2 → 整网无主机可跑，须换网并刷新 `nodes.dat`。
- `spikes.json` 新增 `observations.p4_environment_diagnosis.refinement_2026_11_10` 并更新
  `conclusion` / `precondition_check`；[RISK R-02](docs/05-quality/RISK.md) 补充「前置判定逐主机、
  LAN 探测不构成公网证据」；S2 / kad-mini / tests-spikes README、ROADMAP、OPEN-QUESTIONS 同步；
  `docs/index.html` 重建。

**状态不变**：`S2.status = not_run`、`PROTOCOL_VERSION = 1`、T0 golden files 未动、
三桶计数整体作废、未发布任何测试 Key、公共 KAD 未获得任何协议证据。

## [2.4.8] - 2026-11-10 — P4 首次执行：环境阻断，结论不可用

项目所有者显式下达「执行 P4」。`live --check` 复验门禁 `passed=true` / 0 blocker 后，
按 G0 确认参数执行 `kad-mini live`（不带 `--check`）。**只登记观测与前置检查，不改协议设计基线**：
`PROTOCOL_VERSION` 仍为 1、T0 golden files 不变、`S2.status` 仍为 `not_run`。

### 原始观测

- 2026-11-10T00:26:52Z → 00:27:05Z（13 s）；`gate.passed=true`。
- 发出 **3** 条报文，均为 2 字节 `0xE4 0x01` `KADEMLIA2_BOOTSTRAP_REQ`，间隔严格 5.000 s
  （0.2 包/秒）；**收到 0 条响应**（WSAETIMEDOUT 10060，3 s 读窗口内无数据报）。
- HELLO / 迭代 / PUBLISH / SEARCH 均为 **0**；`known_contacts=0`、`tolerance_reached=0/3`，
  3 个样本全部按 `S2.criteria #2` 记 `skipped`，**未发布任何测试 Key**。
- 报告 `tests/spikes/s2/reports/kad-live-20261110002705.json`（gitignored，逐次完整原始报文）；
  `evidence_valid_for_pass=false`。

### 归因（对照实验）

| 对照 | 结果 |
| :--- | :--- |
| DNS/UDP-53 → 8.8.8.8 / 1.1.1.1 | 成功，61 B，≈53～58 ms |
| STUN/UDP-19302、UDP-3478（与 KAD 无关） | 全部 3 s 超时 |
| 直连 TCP → 1.1.1.1:443（绕过本地 HTTP 代理） | 6 s 超时 |
| LAN 真实 eMule，同一份 `e401` | `0xE4 0x09` 明文 523 B / **20 contacts** |
| aMule 源码 | `ProcessPacket` 直接分发 `KADEMLIA2_BOOTSTRAP_REQ`，请求无 payload 要求 |

结论：`inconclusive_environment_blocked` —— **执行环境过滤公网高位 UDP（出站或回程）**，
不是报文格式、不是节点拒绝、不是命名空间被拒。按 [RISK R-02](docs/05-quality/RISK.md) 口径，
这三次超时属环境前置条件失败，**三桶计数整体作废**，不得判定 S2。
`nodes.dat`（2026-11-08）联系人新鲜度为次要可能因素，STUN 对照已排除其为主因。

### 新增与文档

- 新增可选手动诊断 [udp_egress_check.py](tests/spikes/s2/udp_egress_check.py)：只做 DNS/53 与
  STUN 往返对照，不发任何 KAD 报文、不内置 bootstrap；退出码 `0`=可跑 P4、`2`=被过滤。
  本环境实测 **2**。重跑 P4 前须先通过。
- [PLAN.md §7](tests/spikes/s2/PLAN.md) 记录完整执行与归因；`spikes.json` 新增
  `observations.p4_execution` / `p4_environment_diagnosis` 并更新 `real_network`；
  [RISK R-02](docs/05-quality/RISK.md) 补充「前置条件失败不入三桶」。
- 同步 [S2 README](tests/spikes/s2/README.md)、[kad-mini README](tests/spikes/s2/kad-mini/README.md)、
  [tests/spikes/README.md](tests/spikes/README.md)、[ROADMAP](docs/06-plan/ROADMAP.md)、
  [TEST](docs/05-quality/TEST.md)、[OPEN-QUESTIONS](docs/06-plan/OPEN-QUESTIONS.md)、根 README，
  `docs/index.html` 重建。

## [2.4.7] - 2026-11-09 — P4 前置一致性修正（补齐实现与判据的错配）

推送 [v2.4.6](tests/spikes/s2/PLAN.md) 后复核 `kad-mini live` 实现，发现四处会让 P4 产出假结论的
错配。**只改 S2 spike 实现与其文档，不动协议设计基线**：`PROTOCOL_VERSION` 仍为 1、
T0 golden files 不变、`S2.status` 仍为 `not_run`、**公共 KAD 未被接触**。

### 修复

| 错配 | 修复 |
| :--- | :--- |
| `live` 直接向 bootstrap 节点发布，违反 `S2.criteria` 第 2 条 | 发布前用 `KADEMLIA2_REQ/RES (0x21/0x29)` 最多 3 跳迭代收敛到 `SEARCHTOLERANCE = 2^24` 邻域；未收敛则记录 `skipped` 并跳过该样本，不制造必然被静默丢弃的无效发布 |
| `live` 仍发 `HELLO_REQ`，与 `confirmed.protocol_scope`「跳过 HELLO」矛盾 | 移除 HELLO 交换；逐次记录 `hello=skipped_per_p4_protocol_scope` |
| `0xE5` 压缩响应被当作「协议层失败」，污染 RISK R-02 三桶计数 | 新增零依赖 `tests/spikes/s2/kad-mini/src/inflate.rs`（zlib/DEFLATE，覆盖 stored / fixed / dynamic 块型、校验 Adler-32）；`decode_wire()` 统一处理 `0xE4` / `0xE5` |
| `live_gate` 只数 `"ip"` 字段不校验地址；`live` 用 127.0.0.1 绑定 socket | 门禁实际解析清单并拒绝非公网 IPv4 / `lan-interop-probe-only`；`live` 改用 `bind_any()` |

### 复验结果

- `cargo test` **7/7**；`kad-mini selftest` **37/37**（新增 8 项：三种 DEFLATE 块型、Adler-32 拒绝、
  非 KAD 拒绝、明文回归、公网/私有 IPv4 门禁）；`loopback` **5/5 发布 + 5/5 命中 + 84 条原始记录 + 0 协议错误**。
- 真实 LAN eMule `BOOTSTRAP_RES` 解出 521 B payload / **20 个联系人**；此前捕获的
  真实 519 B `0xE5` 帧经 `inflate.rs` 解出同样结果，与 Python `zlib.decompress` 一致。
- `live --check`：公网清单 `gate.passed=true`；LAN 清单命中两条 blocker；无标记的 RFC1918 清单仍被
  公网 IPv4 校验拦截。三条路径均 `public_kad_contacted=false`，**未发送任何报文**。
- 修复了 v2.4.6 正常 `git push` 因本地代理 CONNECT 502 失败的问题（重试即成功）；Git Data API
  兜底曾因 tree SHA 不一致在更新远端 ref **之前**中止，未产生任何远端副作用。
  远端 `main` 的 tree 与本地 `e25b0f0` 完全一致（`eaad7a5b…`），三个 workflow 已复验。

### 文档

[PLAN.md §0/§1/§2/§3/§5/§6](tests/spikes/s2/PLAN.md)、[kad-mini/README.md](tests/spikes/s2/kad-mini/README.md)、
[S2 README](tests/spikes/s2/README.md)、[tests/spikes/README.md](tests/spikes/README.md)、
[ROADMAP](docs/06-plan/ROADMAP.md)、[TEST](docs/05-quality/TEST.md)、
[OPEN-QUESTIONS](docs/06-plan/OPEN-QUESTIONS.md)、`spikes.json` 同步登记；`docs/index.html` 已重建。

## [2.4.6] - 2026-11-09 — 判据口径回写 + G0 参数确认 + LAN 互操作探测

### 变更：把判据口径改对（A）

按 aMule 源码核实结果回写设计基线，**只改 KAD 集成映射与观测语义，不动私有控制面 v1 字段**，
`PROTOCOL_VERSION` 保持 1、T0 golden files 不变（PROTO 文档版本 2.0 → 2.1）。

- `spikes.json`：`S2.criteria` 从「`FIND_VALUE` 命中带前缀 Key」改为
  `PUBLISH_KEY_REQ (0x43)` / `SEARCH_KEY_REQ (0x33)` / `SEARCH_RES (0x3B)`；
  新增「发布前须 `KADEMLIA2_REQ/RES` 多跳迭代收敛到 `SEARCHTOLERANCE` 邻域」、
  「协议层无响应与索引层拒绝必须分开计数」、「前缀线上不可见，隔离口径为 Key 空间不互相覆盖」、
  「实网参数须与 G0 确认值一致」。同步修正 `S2.assumption` 并登记 `S1.adr_002_impact`。
- [PROTO §1/§2.0/§2/§2.1/§4](docs/04-protocol/PROTO.md)：新增 §2.0「KAD 集成映射基线」，
  定义 `KAD_ID(x) = SHA1(x)[0..16]`（截断规则仍标「暂定待 ADR」）、Tag 值模型、
  `0xE4` / `0xE5` / RC4 帧形态、UDP key 的真实作用；§4 增加「映射修订例外」条款。
- [ADR-001](docs/02-architecture/adr/ADR-001-kad-network.md)：新增「协议核实结论」5 条；
  20/s 发布限速标注须按 tolerance zone 复核；复核时点明确为 **P4 实网小样本之后**（1.0 → 1.1）。
- [ADR-002](docs/02-architecture/adr/ADR-002-bt-engine.md)：登记 rqbit 9.1.0 无握手回调的
  S1 观测，选型门禁改为「评估自实现拦截层 / 代理层」，不再以「引擎是否暴露回调」判 S1（1.0 → 1.1）。
- [RISK R-02](docs/05-quality/RISK.md)：预警线从笼统的「发布失败率 > 20%」改为
  **「索引层拒绝率 > 20%（须先排除协议层不可解析 / 静默丢弃 / 未收敛）」**，并给出三桶计数表。
- `PLAN.md` §0/§1/§4 由「需回写」改为「已回写」，P3 标记完成。

### 变更：G0 参数确认与门禁收紧（B）

- 项目所有者确认采用建议区间的最保守值：
  **`sample_size = 3`**、**`rate_limit_pps = 0.2`**（每 5 秒 1 个包），
  写入 `S2.public_kad_approval.confirmed` 并**清除 `pending`**。
- `kad-mini live_gate` 增加一致性校验：CLI 的 `--sample-size` / `--rate-limit`
  必须与 `confirmed` 逐项相等，否则拒绝；防止批准记录沦为装饰。
- 新增 `kad-mini lan-probe`：只发 `BOOTSTRAP_REQ` / `HELLO_REQ`、**不发任何 PUBLISH**，
  硬编码拒绝非 RFC1918 地址，报告固定 `public_kad_contacted=false` / `publish_sent=false`。
  `rec()` 现在能正确标注 `0xE5` 帧的 opcode；`exchange()` 对 `0xE5` / 非 KAD 帧给出明确错误。
- 新增 [`nodes_dat_to_bootstrap.py`](tests/spikes/s2/nodes_dat_to_bootstrap.py)：
  只读解析 aMule/eMule `nodes.dat`（fileVersion 1–3），输出 gitignored 的本地 bootstrap JSON；
  刻意**不导出 `CKadUDPKey`**（绑定原机器公网 IP，跨主机不可用）。

### 观测结论：节点文件与 LAN 真实客户端

| 项 | 结论 |
| :--- | :--- |
| `nodes.dat` | `fileVersion=2`，**200 个 KAD contacts 全部 verified**，版本 8/9/10 为主；可作公共 bootstrap 来源 |
| `server.met` | magic `0xE0`、17 个 **eD2K 服务器**条目，不是 KAD 路由表，**对 bootstrap 无用** |
| LAN 真实 eMule（RFC1918 私有地址，具体 IP/端口只存于本地 bootstrap 文件） | KAD 在线。`BOOTSTRAP_REQ` 一次回 `0xE4 0x09` 明文 523 B（RTT ≈ 13 ms），另一次回 `0xE5 0x09` zlib 压缩 519 B；`HELLO_REQ` v7/v8 均超时 |

**明文 vs sender/receiver UDP key 的真实区别**：`ClientUDPSocket.cpp::DecryptReceivedClient`
对首字节 `0xE4` 直接放行，`PacketTracking.cpp::InTrackListIsAllowedPacket` 的 key 形参
（`bool /*bValidSenderkey*/`）实际未启用 —— **明文不会被「因未加密」而静默丢弃**。
key 是 `(m_key, m_ip)`、绑定源 IP 的反欺骗 / 联系人验证标记，补它并不能解决压缩帧与 RC4；
而 `BOOTSTRAP_RES` / `KADEMLIA2_RES` / `PUBLISH_RES` / `SEARCH_RES` 发送端都传
`cryptTargetID = NULL`（明文），只有 `HELLO_RES` 对 kadVersion ≥ 6 才 RC4。
→ **决策：保持明文、P4 跳过 HELLO、不实现 key / RC4**；若 P4 中 `0xE5` 桶占比过高再追加 zlib 解码。

### 不变 / 红线

- **P4 实网小样本未执行**：`live --check` 现在报门禁通过但永不发包；须显式去掉 `--check`
  并给出操作者声明才会发布。`S2.status` 仍 `not_run`，公共 KAD 未被接触。
- LAN 节点只作互操作预演，不是公共 KAD 证据；若它连着公共 KAD，向其发布测试 Key 可能外泄，故禁止。
- 仓库不含任何节点 IP：两个 `*.local.json` 均在 `reports/` 下被 `.gitignore` 排除。
- 项目根仍无 `Cargo.toml`、`src/` 未动；S1–S7 仍 `not_run`，S8 仍为所有者豁免（R-11 保留）。

## [2.4.5] - 2026-11-09 — 修复 T0 skeleton CI 断言与 S8 豁免的冲突

### 变更

- `.github/workflows/t0.yml` 的「M0 preflight must report blockers honestly」步骤：原断言
  `all(s['status'] == 'not_run')` 与 2026-11-09 登记的 S8 项目所有者豁免（`pass`）冲突，
  导致 `eccec2e` / `c7b065f` 两个提交的 T0 skeleton 工作流失败
  （`AssertionError: Spike 结论不得由脚本自动置为 pass`）。Docs / Pages 同期为 success。
- 改为**更严格的分级校验**：S1–S7 必须保持 `not_run`（脚本不得自动置 `pass`）；
  唯一允许的非 `not_run` 是 S8，且须能从 `spikes.json` 追溯到
  `resolution=waived_by_owner`、`legal_review.status=waived_by_owner`、
  `written_opinion_received=false`、`residual_risk` 非空 —— 既不放宽其余 Spike 的结论入口，
  也禁止把豁免包装成「已取得法务书面意见」。
- 其余门禁不变：`g0_ready=false`、`t0_real_ready=false`、preflight 退出码 2、
  stub 报告 `evidence_valid=false`、`--mode real` 仍 `BLOCKED`。

### 验证

- 本地以同一断言脚本跑 `m0_preflight.py --json`：通过，
  blockers = `downloader.any` / `binaries.m1` / `t0.wired`。
- 本修复提交后需确认 T0 skeleton 工作流转绿。

### 不变

- S8 仍为所有者豁免（无书面法务意见，R-11 保留）；S2 仍 `not_run`；未接触公共 KAD；
  项目根仍无 `Cargo.toml`、`src/` 未动。

## [2.4.4] - 2026-11-09 — S2 实网排期与最小 eMule KAD 客户端（真实 wire 回环）

### 新增

- `tests/spikes/s2/PLAN.md`：S2 实网**实施排期与门禁**。P0 协议核实 / P1 wire codec /
  P2 本地真实报文闭环**已完成**，P3 实网门禁参数 / P4 实网小样本**未执行**。协议依据固定为
  aMule `master` @ `6a1105634d81242a581a0f36a7e00379b12a83f3`，不采用二手 opcode 表。
- `tests/spikes/s2/kad-mini/`：**零第三方依赖**的独立 Rust Cargo 工程（项目根仍无 `Cargo.toml`、
  `src/` 未动）。实现真实 `0xE4` Kademlia2 帧编解码、128-bit ID / XOR 距离 / `SEARCHTOLERANCE`、
  Tag 与 Contact 编解码、命名空间 Key 派生、树内 SHA-1（含 FIPS 已知答案向量）、三端点本地索引节点、
  令牌桶限速、逐包十六进制原始记录与 `live` 模式门禁。
- `tests/spikes/s2/README.md` 改述为三层结构：L1 逻辑预演 / L2 真实 wire 闭环 / L3 实网小样本。
- `spikes.json` 为 S2 回填 `observations.real_wire`、`protocol_findings` 与报告路径；
  **`status` 仍为 `not_run`**，`public_kad_approval.pending` 仍保留。

### 验证（全部 `127.0.0.1` 回环）

- `cargo test` 7/7；`kad-mini selftest` 29/29（协议常量、帧轮编、SHA-1 已知答案向量）。
- `kad-mini loopback`：发布 5/5 被 ACK、查询命中 5/5 且描述符 Tag 匹配、RTT p50 0.2318 ms、
  命名空间隔离通过、**84 条收发报文完整十六进制逐次记录**、0 协议错误。报告固定写
  `public_kad_contacted=false` 与 `evidence_valid_for_pass=false`。
- `kad-mini live --check`：退出码 2 拒绝，列出 5 项阻塞（`pending` 未清除、缺 `--bootstrap`、
  缺 `--sample-size`、缺 `--rate-limit`、缺操作者声明），**未发送任何非回环报文**，
  未向公共 KAD 发布任何测试 Key。

### 观测结论（发现，本次不擅改设计基线）

- eMule KAD **无 `FIND_VALUE`**：判据应映射为 `SEARCH_KEY_REQ (0x33) / SEARCH_RES (0x3B)`，
  发布为 `PUBLISH_KEY_REQ (0x43)`。核查发现 DeepWiki 把 `PUBLISH_KEY_REQ` 误记为 `0x35`
  （源码中 `0x35` 是 `SEARCH_NOTES_REQ`），本工程常量全部取自 aMule 源码。
- KAD ID 空间为 **128-bit**，[PROTO §1](docs/04-protocol/PROTO.md) 的 `Target_Key = SHA1(...)`
  是 **160-bit**；探针暂取 SHA-1 前 16 字节，截断规则待 ADR 决策。L1 预演用的是
  SHA-256 前 16 字节，两层 Key ID **不可直接比较**，均为临时映射。
- `ColdRevive_V1:` 前缀只作用于本地哈希输入，线上仅传输 128-bit ID，公共节点看不到字符串前缀；
  S2-C3「前缀是否被接纳」的观测口径需重定义。
- KAD 不是通用 KV，Value 只能编码为 `(fileID, Tag 列表)`；`Node_Descriptor` 已映射为命名空间 Tag。
- `Process2PublishKeyRequest` 有 `SEARCHTOLERANCE = 2^24` 距离校验：实测 5 个 Key 的
  `xor_top32` 全部远超阈值，回环靠 LAN 豁免通过；WAN 下会被**静默丢弃且无响应**，
  实网必须先多跳迭代查找收敛到 Key 邻域。
- 明文 UDP 未实现 aMule ≥ v6 的 sender/receiver UDP key 与 RC4 混淆，真实节点可能静默丢弃 ——
  这是 S2-C2 待观测项，也是 P3 是否需追加 P1 工作的决策点。

### 不变

- S1–S7 仍 `not_run`，S8 仍为所有者豁免 `pass`（无书面法务意见，R-11 保留）。
- 未接触公共 KAD；仓库不含任何默认 bootstrap 节点；`CliAdapter.wired` 仍 `False`，
  `--mode real` 仍 `BLOCKED`，T0 不构成 G1 证据；不创建根 `Cargo.toml`、不动 `src/`。
- 上述错配需回写 ADR-001、PROTO §1/§2、`S2.criteria` 与 RISK R-02，本次只登记发现。
- `2.4.3` 及之前的改动已作为 `eccec2e` 推送至 GitHub。

## [2.4.3] - 2026-11-09 — M0 首批实验产出：S1 最小 Demo 与 S2 回环预演

### 新增

- `tests/spikes/s1/`：**独立 Cargo 工程**（项目根仍无 `Cargo.toml`、`src/` 未动），依赖 librqbit 9.1.0。
  本地回环串起 ColdRevive 的核心链路：拦截标准 68 字节 BT Handshake → 按 InfoHash 动态加载伴随种子
  → `AddTorrent::TorrentFileBytes` + `paused=true` 按需实例化 → `unpause / pause / unpause`
  → 16 片逐片 SHA-1 校验。机器可读报告落在 `reports/s1-probe-*.json`。
- `tests/spikes/s2/s2_loopback.py`：私有 KAD 子网双节点预演，A 发布 5 个 `ColdRevive_V1:` 前缀 Key，
  B `FIND_VALUE` 命中 5/5，RTT p50 0.33 ms，命名空间 ID 隔离通过。报告固定标注
  `public_kad_contacted=false` 与 `evidence_valid_for_pass=false`。
- `spikes.json` 为 S1 / S2 登记 `observations` 与报告路径，**`status` 仍为 `not_run`**。

### 观测结论

- **S1-C1 / C2 verified**：握手拦截与按需实例化成立。但源码探测发现 rqbit 9.1.0 的
  `SessionOptions` 无握手回调字段，`session.rs::task_listener` 内部 `read_handshake()` 后直接按
  InfoHash 查已管理 torrent，不暴露拦截点 → 握手拦截器须自实现或加代理层，**影响 ADR-002 选型论证**。
- **S1-C3 / C4 partial**：`is_paused` 正常切换、`progress_bytes` 保持 262144/262144，逐片 SHA-1
  16/16 匹配；但样本文件预先存在（位图始终完整），且未建立双 peer 真实分片传输。
- **S2 仅为预演**：按 `S2.criteria` 第 4 条不得判 `pass`。实网结论还缺真实 eMule KAD 协议实现、
  G0 评审确认的最小样本量与限速阈值、以及实网逐次原始记录。

### 不变

- 脚本禁止自动置 `pass`，S1 / S2 状态仍为 `not_run`；`CliAdapter.wired` 仍 `False`，
  `--mode real` 仍 `BLOCKED`，T0 不构成 G1 门禁证据。
- 未向公共 KAD 发布任何测试 Key；两个 Demo 全程 `127.0.0.1` 回环。

## [2.4.2] - 2026-11-09 — M0 状态对齐：工具链就绪、S8 豁免、S2 批准与 Windows 兼容

### 变更

- 本机安装 Rust stable 1.99.1（`rustc` / `cargo` / `rustup`），MSVC 链接实测可用；`m0_preflight.py` 的 `toolchain.*` 检查转为 `[OK]`。
- 修正因工具链落地而过期的「本机缺 `cargo`」表述：`ROADMAP §6`、`TEST §2.1`、`tests/e2e/t0/README.md §5`、`tests/spikes/README.md`、`spikes.json` 的 `status_note`；同步根 `README.md`、`tests/README.md`、`OPEN-QUESTIONS §3` 的实验状态。
- **S8 法务评审**：项目所有者明确指示略过、视为已评审；在 `spikes.json` 登记 `status: pass` 与 `legal_review.status: waived_by_owner`，并如实标注 `written_opinion_received: false`。**未取得法务书面意见，R-11 风险保留，M4/G4 前须复核**；`m0_preflight.py` 的对应检查文案改为「书面意见或所有者豁免」。
- **S2 实网实验**：项目所有者批准，人工回填 `spikes.json` 的 `public_kad_approval`（最小样本、限速、禁止批量发布测试 Key、逐次记录；样本量与限速阈值待 G0 评审确认）。`status` 仍为 `not_run`：本地回环预演不得判 `pass`，实网结论须基于真实小样本数据。
- 修复 `t0_cli_selftest.py` 在 Windows 的 `WinError 193`：自检生成的 `fake-cli.py` 依赖 shebang 直接执行，Windows 不支持；改为在自检内用 `sys.executable` 显式启动，**不改产品 CLI v1 契约**。
- 本机补装 Python `markdown` 依赖以支持 `scripts/build_docs.py`，重跑构建同步 `docs/index.html` 内嵌数据。

### 验证

- `m0_preflight.py`：阻塞 3 项 / 提醒 1 项，`G0 ready=False`；`cargo`、S2 批准、S8 登记均 `[OK]`。
- `t0_cli_selftest.py`：6/6 通过（含 Windows 修复）；`t0_accept.py --self-test`：21/21 PASS 且 `evidence_valid=false`；`build_docs.py --check` 通过。

### 不变

- **技术路线零改动**：不创建根 `Cargo.toml`、不碰 `src/`；S1–S7 仍 `not_run`；`CliAdapter.wired` 仍 `False`；`--mode real` 仍 `BLOCKED`；T0 不构成 G1 门禁证据。

## [2.4.1] - 2026-11-09 — 托管到 GitHub 并发布在线文档站

### 变更

- 建立 Git 仓库（默认分支 `main`），推送至 **https://github.com/asdfgh129/ColdRevive**（公开）。
- 新增 `.github/workflows/pages.yml`：安装 `markdown` → 运行 `docs` 构建器 → 校验 `index.html` 与源码同步 → 上传 `docs/` 为 Pages 产物 → 部署。仓库 Pages 设为 `build_type=workflow`，在线入口 **https://asdfgh129.github.io/ColdRevive/**。
- `.gitignore` 增加 `.workbuddy/`：本地智能体工作记忆不进入公开仓库。
- 根 `README.md` 增加代码仓库与在线文档入口；重跑 `scripts/build_docs.py` 同步 `docs/index.html` 内嵌内容。

### 说明

- 本版本只做托管与发布，**不改变任何技术设计或验收状态**：S1–S8 仍为 `not_run`、`CliAdapter.wired` 仍为 `False`、T0 stub 自检仍不构成 G1 门禁证据。
- 提交身份使用 GitHub noreply 邮箱，避免在公开提交历史中暴露个人邮箱。

## [2.4.0] - 2026-11-09 — 推进 T0 接线：CLI 契约、观测层与 M0 实验包

### 变更

- 新增 `tests/spikes/`（M0 实验包）：`spikes.json` 登记 S1–S8 判据、优先级、失败替代与状态（当前全部 `not_run`）；`README.md` 给出 S1/S2 实验步骤、合规红线与结论回填规则；`m0_preflight.py` 可复跑地检查实验前置条件，输出机器可读报告。
- **定稿 CLI v1 契约**（`proposed-v1`）：`t0_config.json` 的 `launch` 由占位改为 `coldrevive.t0.cli-contract/v1`，定义 `inspect / find / wake / scenario / metrics / download / golden / env` 八个命令与健康检查；契约说明见 [T0 骨架 §5.1](tests/e2e/t0/README.md)。
- 新增 `tests/e2e/t0/t0_cli.py`：20 条用例的真实取数映射 `OBSERVE_PLAN`，CLI 调用失败 / 非法 JSON / 字段缺失一律判「观测不可用」，**绝不合成观测值**；`torrent_infohash()` 按 BT 规范从伴随 `.torrent` 的 info 字典原始字节计算真实 InfoHash，替换骨架期的 `infohash_stub`。
- `CliAdapter` 接入观测层并显式绑定 20 个 `observe_t0_XX` 入口；`preflight()` 改为输出真实阻塞项。**`wired` 仍为 `False`**，`--mode real` 继续 `BLOCKED`。
- 新增 `tests/e2e/t0/t0_cli_selftest.py`（6 项观测层自检）与 `.github/workflows/t0.yml` 对应步骤；CI 保留「Real mode stays BLOCKED」断言。
- 文档同步：[TEST §2.1](docs/05-quality/TEST.md)（v2.2）、[ROADMAP §6](docs/06-plan/ROADMAP.md)（v2.2）、`docs/README.md`（v2.4）、`tests/README.md`、`scripts/README.md`、根 `README.md`；`build_docs.py` 的 `EMBED_EXTRA` 纳入 `tests/spikes/README.md`（7 → 8 个工程文件，内嵌项 40 → 41）。

### 验证

- `m0_preflight.py` 实测：阻塞 6 项 → 落地观测层后 5 项；`rustc` 与 T0 端口可用，`cargo`、第三方 BT 客户端、M1 二进制、S2 实网批准、`wired` 未就绪，S8 法务评审为外部提醒。
- `t0_cli_selftest.py`：6/6 通过（真实 InfoHash 计算、CLI 取数与字段映射、缺字段不合成、缺二进制抛异常、20 条映射齐全、`wired` 仍为 `False`）。
- `t0_accept.py --self-test` 仍 21/21 PASS 且 `evidence_valid: false`；`--mode real` 仍 1 PASS / 20 BLOCKED、退出码 2，`next_actions` 现已如实反映「观测层已实现、缺二进制与真实环境」。
- `build_docs.py --check` 通过；两次构建字节一致；索引页全量链接解析失效 0。

### 不变

- **技术路线零改动**：不创建 `Cargo.toml`、不碰 `src/`；S1/S2/S8 实验**未执行**，不产出任何 Spike 结论，不接触公共 KAD。
- T0 仍不构成 G1 门禁证据；`Cargo.toml` 与 crate 骨架继续等 G0 通过后创建。

## [2.3.0] - 2026-11-09 — 搭建 T0 双节点验收脚本骨架

### 变更

- 新增 `tests/e2e/t0/`：T0 单机回环双节点验收骨架，含 `README.md`（判据/接线清单）、`t0_config.json`（环境、阈值、启动契约）、`t0_cases.json`（21 条用例元数据）、`t0_harness.py`（端口预检、确定性样本、进程启停与清理）、`t0_adapters.py`（观测契约与 Stub/Cli 适配器）、`t0_accept.py`（断言引擎、JSON 报告、退出码）。
- 新增 `tests/fixtures/t0/`：`WAKEUP_REQ` / `WAKEUP_ACK` v1 契约 golden、确定性样本生成约定；运行产物由 `.gitignore` 排除。
- 用例 `T0-00`～`T0-20` 覆盖 F1→F2→F3→F4 主链路、故障注入（8 类错误码）、协议 golden、最小暴露与端口隔离；判据出处逐条登记在 `t0_cases.json` 的 `source` 字段，阈值只在 `t0_config.json` 定义一次。
- **防呆设计**：stub 自检报告的 `evidence_valid` 恒为 `false`；`CliAdapter.wired = False` 时 `--mode real` 把所有真实用例标为 `BLOCKED` 并以退出码 2 结束，杜绝「没跑却显示绿」。`T0-00` 通过注入坏观测验证断言引擎能正确判 FAIL。
- 文档同步：[TEST §2.1](docs/05-quality/TEST.md) 新增 T0 骨架章节、[ROADMAP §6](docs/06-plan/ROADMAP.md) 修正状态滞后（v2.1 拆分 / M0 未开始 → v2.3 + T0 骨架已就位）、`tests/README.md` 修正「TEST §9.2」错误引用为 §2、根 `README.md` 增加自检命令、`docs/README.md` 增加「docs/ 外被引用文件须登记 `EMBED_EXTRA`」规则。
- `scripts/build_docs.py` 的 `EMBED_EXTRA` 纳入 `tests/e2e/t0/README.md`（7 个工程文件），保证自包含索引页里指向骨架的链接可就地打开；索引页版本标识同步 v2.3。
- 新增 `.github/workflows/t0.yml`：CI 运行 T0 框架自检与文档校验。

### 验证

- `py_compile` 三个脚本通过；`t0_harness.py --self-check` 通过（样本 SHA-256 复算一致、3 个 Keyword_Key ≤ 8、两份 golden 齐全）。
- `--self-test`：21/21 PASS，退出码 0，`evidence_valid: false`；把 P95 阈值临时改为 100 ms 后 `T0-10` 正确 FAIL 且退出码 1，证明阈值取自配置、断言生效。
- `--mode real`（未接线）：1 PASS / 20 BLOCKED，退出码 2，`next_actions` 列出 20 个待实现观测与 3 个未设环境变量。

### 不变

- **技术路线零改动**：不创建 `Cargo.toml`、不碰 `src/`、不提前固化 BT 引擎或 KAD 方案；M0 门禁与 G0～G4 判据保持原样。
- T0 骨架**不构成 G1 门禁证据**；S1/S2 Spike 与法务评审仍是 M0 的起跑线。

## [2.2.0] - 2026-11-09 — 修复索引页阅读链接失效

### 问题

在预览面板 / `srcdoc` 沙箱中只承载 `docs/index.html` 单个文件，卡片上的「阅读」链接指向同目录的 `01-product/PRD.html` 等文件，无法解析，打开后显示 *no data available*。

### 变更

- `docs/index.html` 改为**自包含单文件门户**：新增页内阅读器（文档 / MD 源码双视图、跨文档跳转、Esc 关闭、独立页面可选出口）。
- 全部站内链接在运行时改为 `data-href` 并移除 `href`，从根上避免任何预览框架拦截锚点后的外部导航；外链与页内 `#` 锚点保持原样。
- `scripts/build_docs.py` 新增 `DOCS_PAYLOAD` 注入：把 33 份文档正文 + Markdown 源码，以及 `docs/` 外被引用的 6 个工程文件（`README.md`、`CHANGELOG.md`、`src/tests/scripts/README.md`、`config/coldrevive.example.toml`）共 39 项写入 `index.html`；索引页只替换标记之间的数据块，人工维护部分不被覆盖。
- 抽出共用的 `render_body()`，保证独立 HTML 页面与内嵌阅读器渲染结果一致。
- `--check` 增加索引页 `DOCS_PAYLOAD` 标记校验；同步更新 `README.md`、`docs/README.md`、`scripts/README.md` 的构建说明。

### 验证

- 构建：`--check` 通过；内嵌 39 份文档，生成 33 个独立 HTML 页面。
- 链接：用页面实际的 `normKey()` 校验 79 个索引/卡片链接 + 465 个正文链接，**544 个全部命中内嵌目标**。
- 脚本：`node --check` 通过；`armLinks()` 运行时行为（内部链接去除 `href`、中文路径解析）已抽样验证。

### 不变

- 技术内容、设计决策、指标与里程碑零改动；仅改变文档的分发与阅读方式。

## [2.1.0] - 2026-11-09 — 文档工程化重构

### 变更

- 将单文件 `ColdRevive-项目开发文档-v2.0.md`（1193 行）按软件工程规范拆分为 20+ 份独立文档，分入 8 个类别目录。
- 建立完整项目层级目录：`src/` `tests/` `scripts/` `config/` `.github/workflows/` `docs/`。
- 新增 `docs/index.html` 离线文档索引门户（分类导航、搜索、角色阅读路径、状态徽标）。
- 新增 `scripts/build_docs.py`：Markdown 单一事实来源 → 批量生成 HTML 页面，支持 `--check` 校验。
- 新增 `docs/02-architecture/adr/`：10 份独立架构决策记录（ADR），一份一议题。
- 新增 `config/coldrevive.example.toml` 可执行配置样例，与 CONFIG.md 规范对应。
- 原始 `chat.md` 与 v2.0 合并文档移入 `docs/09-archive/`，标记为已废止的历史基线。

### 不变

- 技术内容、设计决策、指标与里程碑相对 v2.0 **零改动**，仅做结构化拆分与归位。
- 所有已冻结决策（DR-01～DR-08、ADR-003/004/006/007/008/009/010）保持有效。

## [2.0.0] - 2026-11-09 — 设计收敛

基于 `chat.md` 全部讨论轮次，取代其中的 v1.0 草案。主要修订：

1. 确立「常态共享目录（体验层）+ 按需唤醒（实现层）」双层模型。
2. 补充伴随种子生成，解决 BT 基于种子而非文件的硬壁垒。
3. 增加冷热分级、LRU 活跃池、影子索引、握手拦截与资源阈值治理。
4. 分层 DHT：KAD 发现 / 私有 RPC 激活 / Mainline DHT 兼容 / Tracker 可选。
5. 扩充威胁模型、Challenge-Response、限流与信誉设计。
6. 纳入 hMule 失败复盘，将冷启动网络效应列为头号风险。
7. 吸收 aria2 预研结论：守护进程内嵌 HTTP/WS、WebSocket 替代轮询、兼容 aria2 JSON-RPC。
8. 细化 WBS、M0–M5 里程碑、质量门禁、测试矩阵与风险登记册。

## [1.0.0] - 讨论记录内嵌草案

`chat.md` 第 584–739 行的《ColdRevive 客户端开发设计文档 v1.0》，首次提出 KAD 黄页 + 私有唤醒 RPC + 标准数据管道架构。现仅作历史参考。
