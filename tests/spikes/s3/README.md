# S3 spike：KAD 128-bit 与 Mainline 160-bit 双栈资源可控

> **状态（2026-11-13）：S3 已补真实 UDP 双栈 I/O 量测；G0 条件冻结已豁免其 G0 时点结论；`S3.status` 仍为 `not_run`（首轮结构性证据已产出，最终 pass 须结合 M1 真实双栈集成 + G3 兼容性矩阵由实验执行人判定）。**
> 本目录是 S3 的**结构化探针**，验证本地双索引数据结构与隔离性 + 资源增量上界估算；
> **不做真实 KAD / Mainline DHT 网络加入**（那属于 M1 集成与 G3 兼容性矩阵）。

## 1. 假设与判据

**假设**：ColdRevive 必须同时维护两条索引空间 —— eMule KAD 的 **128-bit** key 与 Mainline DHT 的
**160-bit** infohash（[ADR-001](../../docs/02-architecture/adr/ADR-001-kad-network.md) / [TECH-STACK §1](../02-architecture/TECH-STACK.md) 已确认「eMule KAD 与 Mainline DHT 不兼容，必须双栈」）。
S3 要证明双栈**并行压测无 ID 空间串扰**，且**内存与带宽增量在可接受范围**。

| 判据 | 内容 |
| :--- | :--- |
| S3-C1 | 双栈并行压测无 ID 空间串扰（crosstalk / 泄漏） |
| S3-C2 | 内存与带宽增量在可接受范围（对照 SRS NFR-05 常驻 ≤100 MB） |

## 2. 实现

零依赖（仅 `serde_json` 出报告）Rust 二进制 `s3-dualstack`：

- 两个**独立**容器：`HashMap<[u8;16], Entry>`（KAD 栈）与 `HashMap<[u8;20], Entry>`（BT 栈）；
  key 长度不同，类型系统层面保证两栈永不串扰。
- `Entry { value: Vec<u8>, flags: u8 }`，`flags` 标记来源栈，作为串扰断言的运行时校验。
- 阶段 1：顺序**交错**插入 N 条/栈（KAD 与 BT 同索引号交替）。
- 阶段 2：单线程完整性（`flags` 正确）+ **负查询泄漏检查**（随机未在集合中的 key 两栈均须 miss）。
- 阶段 3：**并发读写压测** —— 4 读线程（随机查已存在 key + 负查询）+ 1 写线程（追加不相交新 key），
  断言并行下无跨栈、无泄漏。
- 阶段 4：资源增量**上界估算**（桶表 + entry 头 + value），以及发布/续期报文字节数代理。

## 3. 运行

```bash
cd tests/spikes/s3
cargo build
cargo run --                      # 默认 10_000 条/栈
cargo run -- --scale 50000        # 自定义规模
cargo run -- --scale 100000 --neg 200000
```

报告 JSON 落到 `reports/s3-stress-*.json`（已 gitignore，本地产物、不入库）。

## 4. 首轮证据（2026-11-11）

| 规模（条/栈） | S3-C1 串扰/泄漏 | 双栈索引估算 | 是否 ≤100 MB (NFR-05) | 双栈发布报文估算 |
| :--- | :--- | :--- | :--- | :--- |
| 10 000 | 0 / 0（verified） | ≈ 2.26 MiB | ✅ | ≈ 1 660 KiB |
| 50 000 | 0 / 0（verified） | ≈ 11.08 MiB | ✅ | ≈ 8 301 KiB |

- **S3-C1 verified**：连续两轮（10k / 50k）在顺序 + 并发读写下，KAD 16B key 与 BT 20B key 各自只命中本栈、
  负查询两栈全 miss，串扰/泄漏计数均为 0；KAD 栈与 BT 栈成员关系完全不相交。
- **S3-C2 verified（结构性上界）**：仅 10k/栈即 ≈2.26 MiB，线性外推 1M/栈 ≈ 0.28 GiB，
  仍低于传统 DHT 客户端常驻，且远低于 NFR-05 的 100 MB 目标（10k/50k 实测均达标）。

## 5. 证据边界（关键）

1. 本探针是**本地双 HashMap 数据结构与隔离性 + 资源增量上界估算**，**不是真实 KAD / Mainline DHT 网络加入**。
   真实网络的 churn / 续期 / 跨协议补源流量属于 M1 集成与 G3 兼容性矩阵，此处未测。
2. 内存为**结构体上界估算**（桶表指针 + entry 头 + value + ctrl），真实 RSS 须 M1 运行期 prof；
   带宽增量为**单条目序列化字节 × 条目数**的代理，不含索引刷新/republish 的频率因子。
3. `public_network_contacted = false`、`evidence_valid_for_pass = false`：本探针不判 pass、不回填 `spikes.json`
   的 `S3.status`（保持 `not_run`）；最终 pass 由实验执行人在 M1 真实双栈集成 + G3 矩阵后判定。
4. 与 S1 / S2 一致：G0 冻结未解除前，本产物留在 `tests/spikes/`，不迁入 `src/crates/`。

## 6. 下一步（M1 之后）

- 接入真实 KAD 适配层（[W3.1](../docs/02-architecture/ARCH.md)）与 Mainline DHT 适配（W3.2），做双栈并发 join 压力；
- 量化真实 publish/refresh 频率下的带宽增量；
- 并入 G3 兼容性矩阵回归，由实验执行人据实回填 `S3.status`。

## 补充证据：真实 UDP 双栈 I/O（2026-11-13）

`dualstack_io.py` 补齐 C2 判据中「带宽增量」的**真实量测**（此前只有 `wire_announce_kib_at_10k` 估算）：

- 一个共享 demux 接收 socket + 两个独立命名空间发送 socket，真实 127.0.0.1 UDP 数据报并发收发；
  帧头 `magic(4) + 声明 ID 长度(1) + ID + seq`，KAD 16B / Mainline 20B；3 轮 × 2 s × 800 pps/栈。
- **串扰**：注入 1437 个跨命名空间畸形帧（KAD 声明 20B / DHT 声明 16B / 随机字节），
  **1437/1437 全部拒绝，0 误路由、0 错宽 ID 被接收、0 丢包**。
- **带宽**：KAD 1.638 + DHT 1.638 = **3.276 Mbit/s**（≈两栈之和，无异常放大），RSS 增量 **0.93 MiB**。
- 报告 `reports/s3-dualstack-io-20261113T062159Z.json`（本地产物、不入库）。

> 排错留痕：首版 demux 只校验最小长度，160 个「KAD magic + 20B ID」帧被误收进 KAD 命名空间；
> 改为**精确 ID 宽度校验**后归零。这说明双栈实现必须按 ID 宽度严格分流，不能只靠 magic 前缀。

**结论不变**：`S3.status = not_run`，本地证据按 README §6 第 3 条只是附件；最终 pass 须 M1 真实双栈集成 + G3 矩阵。
2026-11-13 项目所有者批准 [G0 条件冻结](../../../docs/06-plan/G0-REVIEW.md)，豁免 S3 在 G0 时点给出结论，
M1 出口必须回填，不达标则按 fallback（单栈 + 外部映射）降级并重新提交冻结评审。

## 补充证据：M1 真实双栈跨主机集成（2026-11-14 / W3.5①）

`m1_dualstack_integration.py` 把双栈判据从「本机数据结构 / 回环 UDP」推进到**两台不同物理主机上的真实 M1 守护进程**：

- 环境：A `192.168.1.210 / DESKTOP-DJQSC95`、B `192.168.1.209 / DESKTOP-CCE53RF`；
  A 由 harness 临时拉起并在结束时清理，B 常驻不改动；真实私有 LAN UDP / HTTP / CLI 观测；
  `nat` 段缺省（M1 原行为）；**未向公共 KAD / DHT 发布任何测试 Key**。
- 运行：`--self-test` 5/5（纯逻辑）；真实运行 **17/17 PASS**。
- 观测：`inspect kad` 返回 4 个 `published_keys`，`kad_ids_128` 均 32 hex、`dht_ids_160` 均 40 hex、
  两集合互不相交；独立 Python 复刻 `cr-kad` 算法与 Rust 守护进程真实输出**逐项一致**。
- 串扰：真实 KAD key 在 A 命中 `KAD_FIND_RES(hit_node=A, count=1)`；160-bit DHT 形状 ID、
  128-bit ID 当 key、截断 key、畸形帧一律不命中；跨主机 B 侧真实 UDP 往返亦返回真实 `KAD_FIND_RES`。
- 并行压测：两线程并发各 60 帧真实 UDP，KAD 流 60/60 命中、DHT 流 0/60 命中、`misrouted=0`、
  无丢包无错误；本机合计 13500 B / 24 ms，跨主机 B 侧 6720 B / 18 ms；压测后 `published_keys` 未被污染。

报告：`reports/s3-m1-dualstack-w35.json`（本地产物、不入库）。

**结论仍不变**：`evidence_valid=true` 但 `evidence_valid_for_pass=false`，按 README §6 第 3 条只是**附件**。
仍缺 G3 第三方客户端兼容性矩阵、真实 Mainline DHT 公网加入与跨进程 RSS 增量；`S3.status` 保持 `not_run`。
