# M0 Spike 实验包（tests/spikes/）

> **状态（2026-11-13）：S1 / S2 / S4 / S7 / S8 为 `pass`；S3 / S5 / S6 `not_run`**（S8 由项目所有者豁免登记，无书面法务意见；S4/S7 经真实量测回填）。
> **G0 已于 2026-11-13 由项目所有者批准「条件冻结」（conditional pass）**：[ADR-001](../../docs/02-architecture/adr/ADR-001-kad-network.md)
> 签署 `public` 默认模式；S3/S5/S6 豁免 G0 时点结论（状态仍 `not_run`，不虚判），M1/M2 必测回填。
> 评审记录 [G0-REVIEW](../../docs/06-plan/G0-REVIEW.md)，机读决策 `spikes.json` 的 `g0_review`。
> 本目录是「实验的准备与就绪度检查 + 证据产出」，**不是结论本身**；结论唯一入口是 `spikes.json`。
> S1 的四条判据已于 2026-11-11 全部 `verified`（见下方 §4），但状态仍未回填。
> 本机已具备 `rustc` / `cargo` 1.99.1，S1 最小 Demo 已跑通；S2 实网实验的人工批准已登记在 `spikes.json` 的 `public_kad_approval`，[实施排期](s2/PLAN.md) P0–P3 与[最小 eMule KAD 客户端](s2/kad-mini/README.md)（真实 wire 回环、LAN 互操作、`0xE5` 解包、tolerance 收敛）已完成；**v2.4.14 更正**：早期 P4 / 探针的 0 响应不是环境阻断，而是 KAD 联系人 4 字节 IP 被按点分顺序读取（线上是反序），所有包发往错误地址；修正后本机 10 目标宽扫 **9/10 收到真实 `BOOTSTRAP_RES`**，高位 UDP 一直可用，P4 两度重跑已连上公共 KAD，但 3/3 样本未收敛到 `SEARCHTOLERANCE` 邻域而跳过发布。`udp_egress_check.py` 的 STUN 结果只说明 STUN 被过滤，**不是 KAD 代理指标**；不需要换网或开防火墙。**2026-11-10 结论**：根因是 Kad tag 帧头被写成 ed2k/met 的 `[type|0x80][name:1]`（aMule 的 Kad 路径用 `CFileDataIO::ReadTag`，名字恒为 `u16 长度 + 字节`），修正后公网 `PUBLISH_RES` 与 `SEARCH_RES` 命中均出现，两进程 A/B 补测通过，**S2 已判 `pass`**，详见 [PLAN.md §19–§21](s2/PLAN.md)。仍缺第三方 BT 客户端与 M1 二进制，`m0_preflight.py` 会把剩余阻塞项逐条列出。

> **2026-11-11 追加（ADR-011 L1 / L2）**：`s2/` 新增 [ed2k-mini](s2/ed2k-mini/README.md) 最小标准 eD2k
> 传输栈，并在 `kad-mini` 新增 `l2-source-probe`（`0x44` / `0x34` 源字典）。二者是**探针 / 参考实现**，
> 按 G0 冻结不落在 `src/crates/`；所有证据只在 RFC1918 / `127.0.0.1` 产生，报告固定
> `public_network_contacted=false` / `evidence_valid_for_pass=false`，**0 个公网报文、不判 pass、
> 不回填 `spikes.json`**。详见 [PLAN §22–§23](s2/PLAN.md)。

## 1. 定位与红线

判据的唯一来源是 [TECH-STACK §2 M0 Spike 清单](../../docs/02-architecture/TECH-STACK.md)，机读状态在 `spikes.json`。本目录只回答两件事：

1. **现在能不能开始做实验**（`m0_preflight.py`）—— 纯本地检查，不接触公共 KAD。
2. **实验怎么做、结论怎么记**（本文件 + `spikes.json`）—— 步骤、判据、证据清单。

三条红线：

- 脚本**禁止**自动把任何 Spike 置为 `pass`。结论只能由实验执行人依据证据手工回填 `spikes.json`，并附证据路径。
- **本地回环私有 KAD 子网只能作为 S2 预演**，不得据此判定 S2 通过；S2 的 `pass` 必须来自实网小样本，且需人工批准、最小样本、限速，见 [OPS §6 合规边界](../../docs/07-operations/OPS.md)。
- 不把 [REFERENCES 中 V-01～V-09](../../docs/08-reference/REFERENCES.md) 未经核实的第三方数据写进判据阈值。

## 2. 运行

```bash
# 就绪度检查（默认完全本地，不联网）
python3 tests/spikes/m0_preflight.py

# 可选：仅 HEAD 探测依赖源可达性，不发布任何数据，结果只作参考
python3 tests/spikes/m0_preflight.py --probe-network

# 机器可读输出
python3 tests/spikes/m0_preflight.py --json
```

| 退出码 | 含义 |
| :--- | :--- |
| `0` | 全部前置条件就绪 |
| `2` | 存在阻塞项（当前环境必然为 2） |
| `1` | 脚本自身错误 |

报告落在 `tests/spikes/reports/`，已由 `.gitignore` 排除。报告中的 `g0_ready` / `t0_real_ready` 只表示**条件是否齐备**，不表示实验通过。

## 3. 起跑顺序

按 [OPEN-QUESTIONS §3](../../docs/06-plan/OPEN-QUESTIONS.md) 与 [ROADMAP §6](../../docs/06-plan/ROADMAP.md)，不是 S1→S8 顺序刷：

| 顺序 | Spike | 能否决什么 | 并行关系 |
| :--- | :--- | :--- | :--- |
| 第一 | **S2** | 产品定位（公共 KAD 若不接纳命名空间，ADR-001 需切 `private`） | 与 S1 并行 |
| 已决 | **S8** | 项目所有者 2026-11-09 豁免，登记 `pass`（无书面意见，R-11 保留，M4 前复核） | 无需再等 |
| 第二 | **S1** | 架构（BT 引擎能力）；失败只抬成本，`Engine` Trait 已留退路 | 与 S2 并行 |
| 后置 | S3 / S5 / S6 | 参数与降级策略，不推翻设计；S4/S7 已 `pass`；2026-11-13 已补真实量测/先导样本并获所有者条件冻结豁免，M1/M2 必测回填 | — |

## 4. S1 实验方案：BT 引擎能力

**假设**：`rqbit` 支持按需实例化、Pause/Resume、自定义握手拦截、分片校验。

| 项 | 内容 |
| :--- | :--- |
| 前置 | `cargo` 可用；能拉取 `rqbit`；最小 Demo 放在 `tests/spikes/s1/`，**不建 `src/`、不建 `Cargo.toml` 于项目根** |
| 步骤 | ① 起两个本地 peer ② B 发起标准 BT Handshake ③ A 在握手里拦截并动态加载伴随 `.torrent` ④ 实例化 TorrentContext ⑤ Pause / Resume ⑥ 传输至少一个分片并逐片 SHA-1 校验 |
| **已完成（2026-11-11）** | `cargo run -- probe` 覆盖 ②③④ 与单 Session 的 ⑤⑥；`cargo run -- two-peer` 用两个独立 librqbit Session 覆盖真实 BT Peer Wire 传输与**传输中途暂停/恢复**：1 MiB / 64 片，中途 262144 B 时 `pause` 900 ms 位图不变，`unpause` 跑完，seeder 上传 1048576 B，落盘 64/64 片 SHA-1 匹配、0 mismatch；连续 4 轮一致。`S1-C1/C2` 由 probe `verified`，`S1-C3/C4` 由 two-peer `verified`，**4/4 判据齐备**。报告 `s1/reports/s1-two-peer-*.json`（本地产物、不入库，路径登记在 `spikes.json`），细节见 [S1 README](s1/README.md) |
| 判据 | 见 `spikes.json` 的 `S1.criteria` 四条，**已于 2026-11-11 全部 `verified` 且 `S1.status` 由实验执行人手工回填为 `pass`**；第三方 BT 客户端 / 跨主机一轮按用户指令补做以增强互操作证据 |
| 记录 | 发布/查询逐次原始日志、Demo 源码版本、分片校验输出 |
| 失败替代 | 切 `libtorrent` FFI 或自研精简做种层；`cr-engine` 的 `Engine` Trait 不变（[TECH-STACK §5](../../docs/02-architecture/TECH-STACK.md)） |
| 影响 | 决定 [ADR-002](../../docs/02-architecture/adr/ADR-002-bt-engine.md) 是否维持 `rqbit`，以及 T0-07 的观测方式 |

## 5. S2 实验方案：公共 KAD 命名空间

**假设**：可接入公共 eMule KAD，且 `ColdRevive_V1:` 前缀不与现有流量冲突。

| 项 | 内容 |
| :--- | :--- |
| 前置 | `cargo` 可用；**实网实验人工批准记录**（写入 `spikes.json` 的 `S2.public_kad_approval`）；限速配置 |
| 预演 | L1 逻辑预演 [`s2_loopback.py`](s2/s2_loopback.py) + L2 真实 wire 回环 [`kad-mini/`](s2/kad-mini/README.md)（真实 `0xE4` Kademlia2 报文）均已在 `127.0.0.1` 跑通发布/查询闭环，此步只验协议实现，**不产生 S2 结论** |
| 实网 | **S2 = `pass`（2026-11-10）**：`live` 第七轮首次得到 `PUBLISH_RES`（`load=1`）与 `SEARCH_RES` 命中；随后 `split-probe` 两进程 A/B 由 `split-b` 独立取回 `split-a` 的条目（返回 descriptor 携带 A 的源端口）。早期 0 响应曾误判为环境过滤，实为（v2.4.14）IP 字节序与（v2.4.26）Kad tag 帧头两处编码缺陷；`S2.status` 已由项目所有者在证据基础上判 `pass`，证据见 `spikes.json` 的 `observations.p4_execution_v2427` / `ab_two_process_2026_11_10`。`live --check` 报门禁通过但**永不发包**；重跑须先过 `udp_egress_check.py`（退出码 0），再显式去掉 `--check` 并加操作者声明。按最小样本逐条记录发布成功率与查询时延分布，观测按协议层 / 索引层 / 成功三桶分类（环境前置失败整批作废、不入桶），禁止批量发布测试 Key |
| 判据 | 见 `spikes.json` 的 `S2.criteria`；量化阈值由 G0 评审确认后写入，当前不硬编码 |
| 失败替代 | `bootstrap_mode = private` 自建命名空间，并重估 [ADR-001](../../docs/02-architecture/adr/ADR-001-kad-network.md) 与冷启动论证 |
| 影响 | 唯一能否决**产品定位**的 Spike；T0-03 只能在私有子网验证，不得向公共网络发布测试数据 |

### S3（已启动，2026-11-11）：双栈资源可控

**假设**：必须同时维护 KAD 128-bit 与 Mainline 160-bit 两条索引空间（[ADR-001](../../docs/02-architecture/adr/ADR-001-kad-network.md) / [TECH-STACK §1](../02-architecture/TECH-STACK.md) 已确认「必须双栈」）。判据 S3-C1（双栈并行压测无串扰）与 S3-C2（内存/带宽增量可接受，对照 NFR-05 ≤100 MB）。

`tests/spikes/s3/` 已实现零依赖双栈探针：两个独立容器 `HashMap<[u8;16],_>`（KAD）+ `HashMap<[u8;20],_>`（BT），顺序 + 并发读写压测下**串扰/泄漏 = 0**（S3-C1 verified），双栈索引估算 10k/栈 ≈2.26 MiB、50k/栈 ≈11.08 MiB（S3-C2 内存上界）。**2026-11-13 补充 `dualstack_io.py` 真实 UDP 双栈 I/O 量测**：共享端口 demux 按 magic + 精确 ID 宽度路由，1437/1437 跨命名空间畸形帧被拒、0 误路由 / 0 丢包，总带宽 3.28 Mbit/s ≈ 两栈之和、RSS Δ0.93 MiB，C2 带宽由估算升级为实测。属本地证据，**不做真实 DHT 网络加入**；`S3.status` 保持 `not_run`，已获 G0 条件冻结豁免，最终 pass 须 M1 真实双栈集成 + G3 矩阵。详见 [S3 README](s3/README.md)。

### S4–S7（2026-11-11 推进）

**S4（R-06，1 万文件影子索引常驻 ≤100 MB）：`pass`。** 真实 Rust `HashMap<[u8;20], ShadowRow>` 模型（COLD 轻量 handle，不含 Bitfield / 逐片哈希，符合 DATA.md §5），经 Windows 工作集采样实测：1 万文件索引常驻 **3.79 MiB + SQLite 缓存 ≈ 7.8 MiB**，5 万 ≈ 15.9 MiB，20 万 ≈ 53 MiB，均远低于 NFR-01 的 100 MB（硬上限 200 MB）。R-06 去险，fallback（索引分库 / 惰性加载）未触发。证据见 `s4/reports/` 与 `s4/measure_s4.py`。

**S7（R-07，4 GB 造种 CPU/IO 开销）：`pass`。** 流式哈希基准：4 GB 文件分 262144 片 SHA-1 + 全文件 SHA-256 + `.torrent` 序列化，**耗时 4.14 s、吞吐 989 MB/s、峰值 RSS 29.3 MiB**，以 Idle 优先级运行（NFR-02）。R-07 去险，fallback（分片自适应 + 增量哈希）未触发。证据见 `s7/reports/` 与 `s7/s7_bench.py`。

**S5（R-07，uTP/LEDBAT 后台退让满足 NFR-03）：`not_run`（本地量测就绪，已获 G0 条件冻结豁免）。** `s5/qos_bench.py` 真实 TCP 双流 + 令牌桶 + 瓶颈整形量测：后台上行硬卡 ≤20.25% 链路（NFR-03 第一半成立），**但默认 20% 配置前台劣化 20.12%**、LEDBAT 式退让模型 15.0%，ratio=0.05 → 6.19%，**ratio=0.02 → 4.03%（进入 ≤5% 目标）**。结论：**默认配置不满足 NFR-03 ≤5%，M1 默认值须按实测回调，R-07 未完全去险**。该数字来自本地模型而非真实 uTP，不能支撑 `pass`；M1/G3 真实拥塞链路实测方案见 `s5/README.md`。

**S6（R-04，常见家用 NAT 可达率）：`not_run`（1 个真实先导样本，已获 G0 条件冻结豁免）。** 穿透栈覆盖四类 NAT，分析型含中继可达率 ≥95%（> R-04 70% 阈值）。`s6/stun_probe.py` 于 2026-11-13 取得首个真实样本：`192.168.1.210` → `112.5.60.4:36764`，跨 4 台 STUN 映射不变（cone 类）、公网 UDP 可达；**UPnP IGD 与 NAT-PMP 在本环境均不可用**，二跳公网、未见 CGNAT → R-04 不能按「UPnP 可用」假设估计。单样本不满足矩阵判据，须 M2 真实多 NAT 实测（OPEN-QUESTIONS Q4）。矩阵见 `s6/nat-matrix.md`。

## 6. 结论回填规则

`spikes.json` 每项的 `status` 取值：`not_run` / `pass` / `fail` / `fallback`。

1. 只有实验执行人能改 `status`，且必须同时补 `evidence_required` 中列出的证据路径。
2. `fail` 与 `fallback` 必须写明触发的判据编号与替代方案，并同步 [RISK](../../docs/05-quality/RISK.md) 对应风险项状态。
3. `pass` 需要至少一次可复现的完整运行记录；本地模拟结果只能作为附件，不能单独支撑 `pass`。
4. 八项结论齐备后形成 G0 报告，才能按 [src/README.md](../../src/README.md) 的门禁创建 `Cargo.toml` 与 crate 骨架。
   > **2026-11-13 所有者条件冻结（对上述第 4 条的唯一豁免）**：项目所有者批准豁免 S3/S5/S6 在 **G0 时点**给出最终结论（状态仍 `not_run`、不虚判），G0 判「条件通过」并可进入 M1；M1/M2 必测判据、退回条款与复核时点见 [G0-REVIEW §3](../../docs/06-plan/G0-REVIEW.md)。豁免先例为 S8。脚本不得据此自动把任何 spike 置 `pass`。

> **S1 的口径认定（2026-11-11）**：第 3 条禁止「本地模拟结果」单独支撑 `pass`。
> `two-peer` 跑的是**真实 librqbit 协议栈 + 标准 BT Peer Wire TCP**（非逻辑预演），且双端字节计数与逐片
> 校验齐备；实验执行人据此于 2026-11-11 回填 `S1.status = pass`，ADR-002 定稿为「已接受」。**为增强互操作证据，
> 用户另要求补第三方 BT 客户端 / 跨主机一轮**（见 [§4 补充](#4-s1-实验方案bt-引擎能力) 与 `s1/README.md`），
> 该轮属 G3 兼容性矩阵前置口径，不回退已判 `pass`。

## 7. 与 T0 的关系

[T0 双节点验收骨架](../e2e/t0/README.md) 属于 **G1**（M1 核心链路），本目录属于 **G0**（选型假设）。顺序是 G0 先冻结技术路线，再按 T0 README §5 接线：S2 先行 → S1 并行 → CLI 契约定稿 → 观测实现 → 真实 InfoHash / 埋点 → `wired=True`。

**在 M1 二进制与观测接线完成之前，`CliAdapter.wired` 必须保持 `False`，`--mode real` 必须继续报 `BLOCKED`。**（G0 条件冻结已生效，但它不替代 T0/G1 的工程接线） 这条由 `.github/workflows/t0.yml` 在 CI 中强制校验。
