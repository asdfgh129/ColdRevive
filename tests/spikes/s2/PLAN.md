# S2 实网实施排期（最小 eMule KAD 客户端）

> 状态：**P0–P2 已完成，P3/P4 未执行**。
> 本文件是「排期 + 门禁」，不是实验结论。S2 的 `status` 仍为 `not_run`，唯一结论入口是 `spikes.json`。
> 判据来源：`spikes.json` 的 `S2.criteria`；合规红线：[OPS §6](../../../docs/07-operations/OPS.md)。

## 0. 为什么不直接「连上公共 KAD 发几个 Key」

S2 判据写的是「A 发布带前缀 Key，B 用 `FIND_VALUE` 命中 A」。协议核实后发现，这句话在
真实 eMule KAD 里**没有对应的原生操作**，硬发数据只会得到伪造的结论：

| 设计文档表述 | eMule KAD v2 实际 | 影响 |
| :--- | :--- | :--- |
| `FIND_VALUE` | 无此操作。最近似的是 `KADEMLIA2_SEARCH_KEY_REQ (0x33)` → `KADEMLIA2_SEARCH_RES (0x3B)` | S2-C1 的报文映射需明确定义 |
| 128-bit / 160-bit Key | KAD ID 空间 **128-bit**；[PROTO §1](../../../docs/04-protocol/PROTO.md) 定义 `Target_Key = SHA1(...)` 是 **160-bit** | 必须定义截断/重映射规则，PROTO 需修订 |
| `ColdRevive_V1:` 前缀 | 线上传输的是 128-bit **哈希 ID**，不是字符串；前缀只影响本地哈希输入 | 公共节点不会「看到前缀」并拒绝，S2-C3 的观测口径需重定义 |
| KAD = 通用 KV | KAD 存的是「关键词 ID → (文件 ID, Tag 列表)」索引，`PUBLISH_KEY_REQ (0x43)` 的 Value 必须编码成 Tag | `Node_Descriptor` 需映射为 Tag 列表，PROTO §2 需修订 |
| 发布到 bootstrap 节点即可 | `Process2PublishKeyRequest` 有 `SEARCHTOLERANCE = 2^24` 距离校验，越界**静默丢弃、无响应** | 必须先做 `KADEMLIA2_REQ/RES (0x21/0x29)` 迭代查找，定位 tolerance zone 内节点 |
| 命名空间 Key 派生 | L1 预演取 SHA-256 前 16 字节，L2 探针取 SHA-1 前 16 字节，均非协议规定 | 两层 Key ID 不可直接比较；最终 160-bit → 128-bit 映射规则须由 ADR / PROTO 定稿 |
| 明文 UDP | aMule ≥ v6 使用 sender/receiver UDP key 校验与可选 RC4 混淆 | 明文最小客户端可能被真实节点静默丢弃 —— 这正是 S2-C2 要观测的 |

协议依据固定为 aMule `master` @ `6a1105634d81242a581a0f36a7e00379b12a83f3`，
逐条出处见 [kad-mini/README.md](kad-mini/README.md)。**不引用二手资料的 opcode 表**：
核查中发现 DeepWiki 把 `PUBLISH_KEY_REQ` 记为 `0x35`，而源码 `0x35` 实为 `SEARCH_NOTES_REQ`，
真实值是 `0x43`。

## 1. 阶段排期

| 阶段 | 交付物 | 退出条件 | 状态 |
| :--- | :--- | :--- | :--- |
| **P0** 协议核实 | opcode / 帧头 / Contact / Tag / 距离校验 / 版本号的源码级依据 | 每个常量能指向 aMule 源码行 | ✅ 2026-11-09 |
| **P1** Wire codec | `kad-mini/src/wire.rs`：Kademlia2 帧编解码、128-bit ID、XOR 距离、命名空间 Key 派生、Tag 编解码 | `kad-mini selftest` 全绿，含轮编与已知 SHA-1 向量 | ✅ 2026-11-09 |
| **P2** 本地真实报文闭环 | `kad-mini loopback`：BOOTSTRAP → HELLO → 迭代查找 → PUBLISH_KEY/RES → SEARCH_KEY/RES，全程 `0xE4` 真实报文 | 报告 `public_kad_contacted=false`、命中率与逐次原始记录完整 | ✅ 2026-11-09 |
| **P3** 实网门禁与参数 | 用户提供的 bootstrap 节点清单、G0 确认的最小样本量与限速阈值、`pending` 项清除 | `spikes.json` 的 `public_kad_approval` 不再含 `pending`；`kad-mini live --check` 通过 | ⏳ **待项目所有者提供** |
| **P4** 实网小样本执行 | 逐次发布/查询原始记录、成功率、RTT 分布、丢弃/拒绝观测 | 由实验执行人依据证据人工回填 `S2.status` | ⏳ 未执行 |

## 2. 红线（不可通过参数绕过）

1. **不内置默认 bootstrap 节点列表**（[OPS §6](../../../docs/07-operations/OPS.md)）：`live` 模式必须由用户通过 `--bootstrap <file>` 显式提供，仓库不含任何公共节点 IP/域名。
2. **禁止批量发布测试 Key**：`--sample-size` 硬上限 10；每条约 1 个 `PUBLISH_KEY_REQ` + 1 个 `SEARCH_KEY_REQ`。
3. **限速**：`--rate-limit` 必填且 ≤ 1.0 包/秒（比 ADR-001 的 20/s 更保守），令牌桶强制。
4. **人工批准**：`spikes.json` 的 `public_kad_approval.approved == true` 且 `pending` 字段已清除，否则 `live` 直接拒绝。
5. **双开关**：必须同时给 `--live` 与 `--attest-i-am-the-experiment-operator`。
6. **逐次原始记录**：每个收发报文记录时间戳、方向、opcode、完整十六进制、来源/目的地址，落盘后才发下一个包。
7. **不自动置 `pass`**：程序只产出观测；结论由实验执行人手工回填 `spikes.json`。
8. **本地回环不得判 `pass`**：报告固定写 `public_kad_contacted=false` 与 `evidence_valid_for_pass=false`。

## 3. P3 需要项目所有者提供/确认的清单

- [ ] **G0 确认的最小样本量**（建议 3～5 条 Key，`--sample-size N`）
- [ ] **G0 确认的限速阈值**（建议 0.2～1.0 包/秒，`--rate-limit R`）
- [ ] **bootstrap 节点清单**：`[{"ip":"x.x.x.x","port":4672,"kad_version":8}, ...]`，由用户自行取得（如自有 eMule 客户端 `nodes.dat`），仓库不内置
- [ ] 确认是否接受「明文 UDP」被真实节点静默丢弃这一结果，或需要先实现 sender/receiver UDP key（P1 追加项）
- [ ] 清除 `spikes.json` 中 `S2.public_kad_approval.pending`，并把确认后的阈值写回该字段

## 4. 与既有文档的关系

- P0–P2 的产出登记在 `spikes.json` 的 `S2.observations.real_wire`，**不改 `S2.status`**。
- 上述协议错配需回写 [ADR-001](../../../docs/02-architecture/adr/ADR-001-kad-network.md)、
  [PROTO §1/§2](../../../docs/04-protocol/PROTO.md) 与 `S2.criteria` 的报文映射；本排期只记录发现，不擅自改设计基线。
- 项目根仍**无** `Cargo.toml`，`src/` 未动；`kad-mini` 是 `tests/spikes/s2/` 下的独立 Cargo 工程，遵循 G0 门禁。
