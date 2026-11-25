# G2 出口记录（M2 出口 · 2026-11-25 声明达标）

- 文档号：DOC-34
- 状态：**已签 —— G2 达标（带两项书面例外与两项欠账：G-2 / G-3；G-1、G-4、G-5 均已关闭）**
- 版本：v1.3（2026-11-25：v1.1 补采关闭 G-1；v1.2 实现并实测关闭 G-4「造种 Idle 优先级」；
  本版实现 macOS `PRIO_DARWIN_BG` 分支并关闭 G-5 的**代码缺口** —— mac 上仍无硬件实测，
  故 NFR-02 口径不变，见 §10）
- 声明人 / 决定权：项目所有者
- 关联：`docs/06-plan/ROADMAP.md` §3 门禁表 G2 行、DOC-31（复评简报）、DOC-32（S6 折算）、
  DOC-33（公平性例外 EX-1）、`tests/fixtures/g2-spike-verdicts.toml`

---

## 0. 声明本身（唯一口径）

> **G2 达标（所有者判定，2026-11-25T17:20+08:00），带两项书面例外与两项欠账（G-2 / G-3）。**
> G-1（NFR-02 空闲 CPU）、G-4（造种 Idle 优先级）已实测关闭；G-5（macOS）已关闭**代码缺口**
> ——darwin 分支已实现且经类型检查，但**本仓库没有 mac 硬件**，所以它不构成 macOS 实测证据，
> NFR-02 的口径继续只写「Linux/Windows 两分句已实测」（见 §10）。

- **判据来源**：`ROADMAP.md` §3 ——「NFR-01/02/03 达标；S6 按 eMule 式可达语义结案
  （真实设备 + 两台物理主机 + 每条路径 30 次真实 `WAKEUP_ACK` + HighID/LowID 两半各有证据）。
  **NAT 类别覆盖是描述性清单，不是 pass 前提**；声明须带已知限制，不得写「矩阵通过」」。
- **机器落点**：`tests/fixtures/g2-spike-verdicts.toml` → `g2_claim = true`（人为判定文件，authority=human）。
- **这个声明的强度**：G2 作为 M2 出口**可以解除**；但下面第 2 节列的东西它**不覆盖**，
  第 3 节列的四件事**不因它解除**。任何对外表述必须连例外一起给。

---

## 1. 逐项账（每项都写清「靠什么支撑」与「撑不撑得住」）

| # | 项 | 判据 | 2026-11-25 现状 | 证据 / 落点 | 支撑本声明？ |
|---|---|---|---|---|---|
| 1 | NFR-01 内存 | 1 万文件纯空闲 ≤ 100 MB（硬上限 200 MB） | 1 万 ≈ **7.79 MiB**、5 万 ≈ 15.9 MiB、20 万 ≈ 53 MiB | `tests/spikes/s4/`（S4 = pass） | ✅ 是 |
| 2 | NFR-02 CPU | 空闲态平均 ≤ 2%，造种仅 Idle 优先级 | **前半句**：600.63 s 空闲窗口内进程累计 CPU **0.18 s** → 均值 **0.03%**（单核；阈值 2%，余量 **66×**），`inst_p95=0.0%`、单点最大 1.33%。**后半句（2026-11-25T20:45 补实现并实测）**：引擎线程（`cr-engine-wrk`）**nice 19 + `SCHED_IDLE`**，3 次快照全部成立；唤醒关键路径（`cr-data-plane` ×2）与 std 线程全部 `nice 0`/`SCHED_OTHER` 未被降级。**负向对照**：`engine.idle_priority=false` 时同一批线程回到 `nice 0`/`SCHED_OTHER`，判定随之为 fail | `nfr02-idle-cpu-20261125.json`（+ `.raw.json`）、`nfr02-engine-idle-priority-20261125.json`（+ `.on.json`/`.off.json`）、`nfr02-macos-idle-priority-20261125.json` | ✅ 是（**Linux/Windows 两分句均有实测**；macOS 已实现但无硬件实测 → G-5 代码缺口已关，见 §10） |
| 3 | NFR-03 带宽 | 后台上行 ≤ 阈值；前台劣化 ≤ 5% | 令牌桶默认 `ratio=0.02` → 前台劣化 **4.03%**（qos_bench）/ **2.70%**（真 rqbit uTP 栈）；**LEDBAT 不存在**（该 crate 只实现 CUBIC） | `tests/spikes/s5/`（S5 = `fallback`） | ✅ 是（**凭令牌桶，不凭 LEDBAT**） |
| 4 | S6 NAT 可达性 | eMule 式可达语义结案 | `pass(closed_with_exceptions)`，2026-11-17 判；2026-11-25 按 DOC-32 **条件计入** | 夹具 `[[spike]] S6` + `ADR-016 §R3` | ✅ 是（**带四条例外**） |
| 5 | 公平性（G2 第 1 项） | Jain ≥ 0.95、份额 ∈ [0.45,0.55]、每 arm 10 轮全过 | **FAIL 78/80**（`cc_on` 第 7 轮 0.430/0.570、`cc_paced` 第 8 轮 0.433/0.567） | DOC-33 / EX-1 | ❌ **否** → 作为**例外 EX-1** 计入 |
| 6 | 实现项（M2 内涵） | cr-dht、LRU、中继、打洞 | 两项落地；中继欠运维项；打洞仅 3/30 且形态须区分 | 夹具 `g2_note_20261125` | 🟡 部分（见第 3 节） |

**读法**：第 1、2、3、4 项**完整支撑**本声明（第 2 项两条分句都已实测：
空闲 CPU 0.03%、引擎线程 `nice 19`+`SCHED_IDLE`，且带负向对照）；第 5 项不支撑 → 例外 **EX-1**；
第 6 项部分支撑。**这正是「带例外」三个字的实际含义**——不是四舍五入，而是把不支撑的项点名写在旁边。

---

## 2. 本声明覆盖什么，不覆盖什么

**覆盖**：M2 出口判据在「带例外」口径下成立，G2 可以解除，M3 可以开工。

**不覆盖**（写这些一律算越界）：

- ❌ 「公平性达标」——按原判据是 FAIL，例外不是判据达成；
- ❌ 「NAT 矩阵通过」/「4/4 类别覆盖」——实际 3/4，`restricted-cone` 按 R2 排除、`CGNAT` 未覆盖；
- ❌ 任何可达率 / 成功率百分比（`direct 3/30`、`relay 30/30` 都是计数，不是比率结论）；
- ❌ 「punched-direct 已打通」——只有 3/30，且 BT 公网那条属 `direct-via-static-dnat`；
- ❌ 「NFR-02 在三平台全部达标」——Linux/Windows 两分句均已实测；macOS 的
  `PRIO_DARWIN_BG` 分支虽已实现、判定逻辑有单测、分支经 `--features darwin-check` 类型检查，
  但**没有任何一台 mac 跑过**，读回谓词本身也没在真机上验证过。所以可写
  「NFR-02 在 Linux/Windows 上两分句均有实测」，**不可**写「三平台达标」或「macOS 已验证」；
- ❌ 「整个进程处于 Idle」——只有引擎线程降级，唤醒路径与 std 线程保持默认（这是设计要求）；
- ❌ 把单机 netns、单条公网路径、组件级 Spike 的结论外推成「跨公网/跨运营商普遍成立」。

---

## 3. 随声明生效、但**不因此解除**的四件事

1. **EX-1 的 C1–C6 全部保留**。尤其 C3 三条失效条件（端侧机制改动 / 链路形状或物理环境变化 /
   重新预注册判据）一旦发生，例外当场作废——但**不会**把已声明的 G2 倒回未达标，
   而是触发「必须重测并在下一次门禁复评时交代」，是否回退由所有者判。
2. **S6 的四条例外保留**：类别 3/4、HighID direct 3/30 属描述性、symmetric 0/30 是 NAT 行为、
   `punched-direct` 仅 3/30 且 BT 公网属 DNAT。关闭记录里这四条不许省略。
3. **中继运维欠账（G-3）**：限流/观测/告警未单列验收。功能链路在用（公网 relay
   `119.29.150.254:7101`，BT 经中继落地 md5 一致），欠的是运维面。
4. ~~**NFR-02 缺口（G-1）**：空闲 CPU ≤2% 没有独立采点。~~ → **2026-11-25T20:05 补采关闭**：
   600.63 s 空闲窗口（`role=holder`、`seed_active=0`、`served_bytes` 零增长、`dht.enabled=false`、
   无 `limits.*` 阈值）内进程累计 CPU **0.18 s** → 均值 **0.03%**（单核；阈值 2%，余量 66×），
   RSS 恒 17,320 kB、9 线程，20 次 `/health` 采样全部成功且 `seed_active` 恒 0。
   证据 `tests/ARCHIVE/evidence/nfr02-idle-cpu-20261125.json`，逐样本原件
   `nfr02-idle-cpu-20261125.raw.json`（sha256 `426b2a18…` 与清单同值）。
   采样在**按 HEAD 重建**的二进制上做（sha256 `74859068…`；构建树 86/86 `.rs` 与 git blob
   逐字节一致）——原部署二进制是旧树构建、8 个 `.rs` 落后于 HEAD，已重同步后重建。
   **这只关闭「无采点」，不关闭整条 NFR-02**：第二分句「造种任务仅以 Idle 优先级运行」
   在守护进程里**没有运行时实现**（全仓无 `setpriority` / `SetThreadPriority` / `SCHED_IDLE`，
   无 `libc`/`winapi` 依赖；systemd `Nice=0`、`CPUSchedulingPolicy=SCHED_OTHER`）。
   S7 基准里的「Idle 优先级」是**基准脚本自设**，属测试夹具属性，不是产品保证 → 缺口 **G-4**。
   ~~→ **2026-11-25T20:45 已实现并实测关闭**：新增 `cr_engine::prio`（Linux `nice 19` +
   `SCHED_IDLE`；Windows `THREAD_PRIORITY_IDLE(-15)`），在 `Builder::on_thread_start` 里
   **按线程**降级，`engine.idle_priority` 默认 `true`。221 实测：3 个引擎线程（`cr-engine-wrk`，
   含 2 个 worker + 1 个存活阻塞池线程）全部 `nice 19`/`SCHED_IDLE`，其余 7 个线程全部
   `nice 0`/`SCHED_OTHER`；关掉开关的负向对照让同一批线程回到 `nice 0`。同期 1 MiB 真实传输
   md5 一致（`6209e962…`）。证据 `nfr02-engine-idle-priority-20261125.json`。
   **macOS 未实现**（无机器实测，不写「支持」）→ 新缺口 **G-5**。~~
   > 上述删除线是 v1.2 的成文记录。**2026-11-25T21:5x 补**：macOS 分支已实现
   > （`PRIO_DARWIN_BG`，判定逻辑有单测、分支有类型检查），G-5 按「代码缺口」关闭；
   > 但 mac 实测仍缺，**口径不变**——详见 §10。历史记录不改写，故此处保留原文。
   另：**S4/S5 的 `reports/` 与 S6 一样被 `.gitignore` 排除**（仅 `.gitkeep` 入库），
   S6 已逐字节归档、S4/S5 未归档 → 缺口 **G-2**（证据可移植性）。

---

## 4. 为什么四个机器字段只改了一个

| 字段 | 值 | 为什么 |
|---|---|---|
| `g2_claim` | **`false` → `true`** | 这是**人为判定**字段（`authority = "human"`），所有者声明达标即改；工具不得自行置位 |
| `s6_status` | `not_run`（不变） | census 的**工具**字段，含义是「工具没有替你下结论」，与本声明互不覆盖 |
| `evidence_valid_for_pass` | `false`（不变） | 同上：工具不判定证据「足以 pass」，那由人判 |
| `auto_pass` | `false`（不变） | 自动 pass 被永久禁止；它不是「G2 未达标」的意思 |

一句话：**前三个 `false` 表达的是「工具不替人下结论」，不是「人没判」。**
把 `s6_status` 也改成 pass 才是错的——那是伪造工具输出。

**一处容易被误读的地方**：`tests/fixtures/g2-fairness-thresholds.toml` 的 `[gate_snapshot]`
里仍有 `g2_claim = false`。那是 **2026-11-24 预注册判据时的快照**，而且该文件 sha256
（`0671a173…`）被 EX-1 钉住——**不去改它**：改它会让「判据先于样本」这条纪律破功，
也会让 EX-1 里记录的那个 sha 失效。要查当前值只看本单据与 `g2-spike-verdicts.toml`。

---

## 5. 历史记录不回写（避免下一个人各引一句）

以下文档里的 `g2_claim=false` 都是**成文时点的记录**，按惯例不改写：
`ADR-012` / `ADR-014` / `ADR-015` / `ADR-016`（R1–R3）/ `ADR-019` ~ `ADR-024`、
`G0-REVIEW.md`、`CHANGELOG.md` 历史条目、以及各 `tests/ARCHIVE/evidence/*.json`。
判定这些是历史记录还是现状，看文件头的时间戳，**当前值只看本单据与夹具**。

---

## 6. 仍然固定的红线（不因达标解除）

- 不编数字、不自动 pass、脚本不得写判定；
- 「矩阵通过 / 可达率 / 公平性达标 / NFR-02 达标」四类措辞继续禁用；
- 新增采集必须带 `git_commit` / `kernel_release` / `tc_qdisc_and_class` 与逐轮原始目录；
- 例外可以随条件失效，**不能**随「时间过去」失效或续期。

---

## 7. 签署

- **声明**：☑ G2 达标（带两项书面例外与两项欠账 G-2 / G-3；G-1、G-4、G-5 均已关闭）
- **例外**：☑ EX-1（公平性，DOC-33）　☑ S6 条件计入（DOC-32 / ADR-016 §R3）
- **欠账/缺口**：~~G-1（NFR-02 空闲 CPU）~~ **已实测关闭 0.03%**　☑ G-2（S4/S5 证据未归档）
  　☑ G-3（中继运维面）　~~G-4（造种 Idle 优先级无实现）~~ **已实现并实测关闭**
  　~~G-5（macOS 未实现 Idle 优先级降级）~~ **已实现并关闭代码缺口**（v1.3，§10；
  mac 硬件实测仍缺，故 NFR-02 口径不变）
- **机器改动**：☑ `g2_claim: false → true`　☑ 其余三个字段不动
- **签署人**：项目所有者　**日期**：2026-11-25

---

## 8. 附：NFR-02 空闲 CPU 采点方法（G-1 关闭依据）

- **时间 / 主机**：2026-11-25 19:55:21 – 20:05:20（+08:00），`221`（6.2.0-39-generic，2 核）。
- **被测对象**：`coldrevived-a`，pid 669250，`role=holder`、`dht.enabled=false`、`limits.*` 一条未配。
- **判据**：SRS NFR-02 前半句「空闲态平均 CPU ≤ 2%」。**阈值 2% 未改、未放松**。
- **采样方式**：外部读 `/proc/669250/stat` 的 `utime+stime`（`_SC_CLK_TCK=100`），1.5 s 一次共 400 点；
  均值由**首尾累计差 / 墙钟**算得，不依赖瞬时值，瞬时序列只用于 p95/极值描述。
- **为什么不用 `/health` 自采样**：`sysres::spawn()` 只在配置了任一 `limits.*` 阈值时才起线程
  （`needed()` 为 false 直接返回）。本环境一条未配，故 `/health` 的 `current=null`、`sample_at_secs=0`。
  **这是「没开采样」，不是「读不到 CPU」**——早期长跑的 `cpu_percent_sampled=false` 同源于
  采集脚本没取 `/health` 资源块（原件写的是 `No CPU time series was captured`）。
- **空闲侧证**：20 次 `/health` 中 `seed_active` 恒 0、`served_bytes` 零增长、`hot=0`；
  `seed_tasks=1` 且状态 `COLD`（冷种子未唤醒）。
- **结果**：0.18 s / 600.63 s = **0.03%**（单核口径；占整机 0.015%）。`inst_p95=0.0%`，
  单点最大 1.33%（1.5 s 级抖动，非持续占用）。RSS 恒 17,320 kB，9 线程。
- **二进制来源**：原部署二进制由**落后 HEAD 的构建树**编出（8 个 `.rs` 不一致），本次先按 HEAD
  重同步（86/86 `.rs` 与 git blob 逐字节一致）再 native build，`sha256 74859068…`；
  旧二进制备份为 `coldrevived.bak-nfr02`（`97dafe5a…`）。
- **复现**：
  ```
  scp scripts/nfr02_idle_cpu_probe.py root@192.168.1.221:/root/
  systemd-run --collect --unit=nfr02 --property=Type=oneshot \
    /usr/bin/python3 /root/nfr02_idle_cpu_probe.py <pid> 600 /var/tmp/nfr02.json \
    http://127.0.0.1:17800/health
  python scripts/build_nfr02_evidence.py <raw.json> \
    tests/ARCHIVE/evidence/nfr02-idle-cpu-20261125.json
  ```
- **不覆盖**：本采样只回答「空闲时 CPU 是多少」，不回答「造种时会不会抢占前台」——
  后者靠第二分句的 Idle 优先级（见 §9），两者是不同的问法，别互相顶替。

---

## 9. 附：NFR-02 第二分句（造种 Idle 优先级）的实现与取证（G-4 关闭依据）

- **判据**：SRS NFR-02 后半句「造种任务仅以 Idle 优先级运行」。**判据原文未改、未放松**。
- **粒度（必须先说清）**：降的是**引擎线程**，不是进程。同进程里 cr-guard 的 68 B 握手、
  cr-data-plane 的 UDP 数据面、HTTP API、DHT/KAD 都跑在别的线程上，整进程降级会把
  NFR-04 的唤醒 P95 一起拖下去——那是拿一个门禁换另一个门禁。
- **落到哪一层**：librqbit 只有一个线程池，做种/分片校验/上传/下载共用同一批 worker，
  因此无法再细到「只降做种、不降下载」；成员角色的下载同样以 Idle 运行。这条边界写进
  `EngineOptions::idle_threads` 文档、`docs/04-protocol/CONFIG.md` 与 `/health`，不藏。
- **平台**：Linux `setpriority(19)` + `sched_setscheduler(SCHED_IDLE)`；
  Windows `SetThreadPriority(THREAD_PRIORITY_IDLE = -15)`；
  macOS `setpriority(PRIO_DARWIN_THREAD, 0, PRIO_DARWIN_BG)`（读回
  `getpriority(PRIO_DARWIN_THREAD) == PRIO_DARWIN_BG`）。**macOS 只做到「实现 + 类型检查」**，
  本仓库无 mac 硬件，实测仍缺——这条的取证细节与不许说的话单列在 §10，别把它读成已达标的第三平台。
- **为什么卡在出生点**：`Builder::on_thread_start` 里降级。事后按 tid 去 `/proc` 找线程，
  会漏掉「线程已开始做种、还没被找到」的窗口，且依赖线程名这类非契约信息。
  > tokio 1.54.1 的实现细节：`on_thread_start` 的回调**只由阻塞池**调用（`after_start`），
  > 多线程 worker 之所以也被覆盖，是因为 `Launch::launch` 用 `spawn_blocking` 起 worker
  > ——**worker 本身就是阻塞池线程**。这是版本相关的，不是公开契约，所以判定一律看读回。
- **一律读回**：系统调用返回 0 不算数。Linux 读 `/proc/thread-self/stat`（**不是**
  `/proc/self/stat`——后者是线程组组长，子线程里读到的是主线程的值，第一版就因此把
  「已生效」误判成「没生效」）；Windows 读 `GetThreadPriority`。
- **线程名**：`cr-engine-wrk`（**≤ 15 字符**，Linux `TASK_COMM_LEN=16` 含 NUL，更长的名字
  会被静默截断；曾经用 16 字符名，`/proc` 里只剩 15 字符，按全名比对的探针报「0 个引擎线程」
  ——看着像没降级，其实是名字对不上）。
- **实测（221，`coldrevived-a`，二进制 `sha256 ef34652b…`，commit `9ae510a`）**：

  | 角色 | 线程数 | nice | policy |
  |---|---|---|---|
  | engine（`cr-engine-wrk`） | 3（2 worker + 1 存活阻塞池线程） | **19** | **SCHED_IDLE** |
  | wake_critical（`cr-data-plane`） | 2 | 0 | SCHED_OTHER |
  | main / std | 5 | 0 | SCHED_OTHER |

  3 次快照（间隔 3 s）**每一次**都成立；`/health.idle_priority` = `requested=true, applied=3,
  failed=0, unsupported=0`。
- **负向对照**（这条比正向更能说明探针在测事实）：`engine.idle_priority=false` 重启后，
  同一批线程名仍在，但 `nice=0`/`SCHED_OTHER`，判定变成 `fail`、`applied=0`。
  没有这条，「探针只会印 pass」就无法排除。
- **功能不敢假设，直接测**：引擎线程处于 Idle 的状态下跑 LAN 用例（A 做种、B 拨入握手唤醒），
  1 MiB 落地 **md5 一致**（`6209e962…`）、`guard.handshakes=1/woken=1`、`seed_after=hot`。
  证据 `tests/ARCHIVE/evidence/bt-wake-on-handshake-lan-idleprio-20261125.json`。
- **复现**：
  ```
  scp scripts/nfr02_thread_prio_probe.py root@192.168.1.221:/root/
  python3 /root/nfr02_thread_prio_probe.py <pid> /var/tmp/nfr02-thread-prio.json \
      http://127.0.0.1:17800/health 3 3
  ```
- **不覆盖**：「造种时前台吞吐劣化 ≤5%」属 NFR-03（S5，凭令牌桶）；本项只回答
  「造种任务在哪个优先级上跑」。

---

## 10. 附：macOS `PRIO_DARWIN_BG` 分支（G-5 关闭依据 / **不构成 macOS 实测**）

- **关的是什么**：G-5 原文是「macOS **未实现** Idle 降级（`prio` 返回 `Unsupported`）」。
  这是**代码缺口**。本版把 darwin 分支写出来、写清读回判据、并让判定逻辑有单测，
  代码缺口即关闭。
- **没关的是什么**：**macOS 上仍然一次都没跑过**。本仓库没有 mac 机器；没有真机就没有
  `getpriority(PRIO_DARWIN_THREAD)` 的实测读回值，也就没有「darwin 上确实降下来了」的证据。
  所以：
  - ✅ 可写：macOS 分支已实现，判定逻辑有单测（`darwin_verdict_*`），分支经类型检查；
  - ❌ 不可写：「macOS 已验证」「三平台达标」「NFR-02 已在 macOS 实现」——
    最后一句里的「实现」容易被读成「生效」，与事实不符，**一律不写**。
- **实现**（`src/crates/cr-engine/src/prio.rs`）：
  `setpriority(PRIO_DARWIN_THREAD=3, 0, PRIO_DARWIN_BG=0x10000)`；
  读回 `getpriority(PRIO_DARWIN_THREAD, 0)`，**等于 `PRIO_DARWIN_BG` 才算成功**。
  `PRIO_DARWIN_BG` 是标志位不是 nice 值，所以 `ThreadPriority.nice` 在 darwin 恒 0（无意义），
  真实读数放 `policy`（0 = 前台类 / `0x10000` = 背景类）；`IdleOutcome` 新增
  `DarwinBackground`，`as_str()` 为 `"darwin_bg"`，`is_idle()` 为 `true`。
  **它不是 Linux `SCHED_IDLE` 的同义词**：两者机制不同（darwin 的 QoS 背景类 vs Linux 的调度策略），
  语义相近但不等价；本项判据只要求「仅以 Idle 优先级运行」，这一档是 darwin 上能拿到的最接近的东西。
- **`getpriority` 的 -1 陷阱**：darwin 上 -1 既是错误返回值、又不可能是一个合法状态，
  所以先清 `errno`（darwin 的 `errno` 由 `__error()` 返回指针，不是全局变量），
  读到 -1 一律折成 `None`（读不出），**既不折算成 0，也不折算成成功**。
- **为什么把判定拆出来**：FFI 那几行在非 mac 上只能类型检查；但「什么读回算成功」是纯逻辑。
  `darwin_verdict::from_readback(set_rc, set_errno, read_back)` 因此与平台无关，
  四个用例在 Windows/Linux 上真跑：
  ① `(0,0,Some(BG))` → `DarwinBackground`；② `(0,0,Some(0))` → 失败（**只看返回码会在这里误报达标**）；
  ③ `(0,0,None)` → 失败且文案说明「无法判定」；④ `(-1,1,Some(0))` → 失败并带出 `errno 1`。
- **分支怎么做的类型检查**：`cargo check -p cr-engine --features darwin-check`。
  这个 feature 只把 `prio::imp` 切到 darwin 分支，不链接、不执行；生产构建默认关闭
  （`cargo build` 行为不变，Linux/Windows 仍走各自分支）。
- **为什么不用 `--target aarch64-apple-darwin` 直接交叉检查**（这是个真实记录，省得下一个人重试）：
  已 `rustup target add aarch64-apple-darwin` 成功，但 `cargo check --target …` 在
  **`aws-lc-sys v0.47.0`** 的构建脚本处失败——它要通过 `cc` 为目标平台编译 C 代码，
  Windows 上没有 darwin 交叉 C 工具链，报 `failed to find tool "cc"`。
  `aws-lc-sys` 是 librqbit → rustls 的传递依赖，绕不开。所以采用 feature 门控的等价检查。
- **判定逻辑的证据（可跑、已跑）**：`cargo test -p cr-engine --lib` → 13 passed / 0 failed
  （原 11 + `darwin_verdict_*` 2 条；`outcome_strings_are_stable` 增补 `darwin_bg` 断言）。
  归档 `tests/ARCHIVE/evidence/nfr02-macos-idle-priority-20261125.json`（含 feature 检查输出、
  `runtime_verified=false`、`counts_toward_nfr02=false`）。
- **真机收尾清单**（有 mac 时照做，做完才允许改口径）：
  1. `cargo test -p cr-engine --lib` —— 应看到 `idle_applies_to_the_calling_thread_only` 的
     macOS 分支断言（`policy == PRIO_DARWIN_BG`）通过；
  2. 起 daemon，`/health.idle_priority` 应 `requested=true, applied≥1, failed=0, unsupported=0`；
  3. 负向对照：`engine.idle_priority=false` 重启后 `applied=0`，且读回 `policy=0`；
  4. 顺手确认唤醒路径未被降级（guard/data-plane 线程仍是前台类）——若是 mac 上无法逐线程读，
     就如实写「无法区分」，不要用进程级读数顶替。
- **状态**：`runtime_verified = false`。这三条（实测、负向对照、唤醒路径）一条都不许从
  Linux/Windows 的结果外推。
