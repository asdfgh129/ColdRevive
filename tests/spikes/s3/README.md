# S3 spike：KAD 128-bit 与 Mainline 160-bit 双栈资源可控

> **状态（2026-11-11）：S3 已启动；`S3.status` 仍为 `not_run`（首轮结构性证据已产出，最终 pass 须结合 M1 真实双栈集成 + G3 兼容性矩阵由实验执行人判定）。**
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
