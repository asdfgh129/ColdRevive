# S2 实网实施排期（最小 eMule KAD 客户端）

> 状态：**P0–P3 已完成，P4（实网小样本）待实验执行人显式执行**。
> 本文件是「排期 + 门禁」，不是实验结论。S2 的 `status` 仍为 `not_run`，唯一结论入口是 `spikes.json`。
> 判据来源：`spikes.json` 的 `S2.criteria`；合规红线：[OPS §6](../../../docs/07-operations/OPS.md)。

## 0. 为什么不直接「连上公共 KAD 发几个 Key」

S2 判据写的是「A 发布带前缀 Key，B 用 `FIND_VALUE` 命中 A」。协议核实后发现，这句话在
真实 eMule KAD 里**没有对应的原生操作**，硬发数据只会得到伪造的结论：

| 设计文档表述 | eMule KAD v2 实际 | 影响 |
| :--- | :--- | :--- |
| `FIND_VALUE` | 无此操作。最近似的是 `KADEMLIA2_SEARCH_KEY_REQ (0x33)` → `KADEMLIA2_SEARCH_RES (0x3B)` | ✅ 已在 `S2.criteria` 与 PROTO §2.1 改为 SEARCH_KEY 映射 |
| 128-bit / 160-bit Key | KAD ID 空间 **128-bit**；[PROTO §1](../../../docs/04-protocol/PROTO.md) 原定义 `Target_Key = SHA1(...)` 是 **160-bit** | ✅ PROTO §1/§2.0 已定义 `KAD_ID(x) = x[0..16]`；截断规则仍标「暂定待 ADR」 |
| `ColdRevive_V1:` 前缀 | 线上传输的是 128-bit **哈希 ID**，不是字符串；前缀只影响本地哈希输入 | ✅ `S2.criteria` 已改为「Key 空间不互相覆盖」，不再观测「前缀是否被接纳」 |
| KAD = 通用 KV | KAD 存的是「关键词 ID → (文件 ID, Tag 列表)」索引，`PUBLISH_KEY_REQ (0x43)` 的 Value 必须编码成 Tag | ✅ PROTO §2 已把 `Node_Descriptor` 定义为 Tag 列表 |
| 发布到 bootstrap 节点即可 | `Process2PublishKeyRequest` 有 `SEARCHTOLERANCE = 2^24` 距离校验，越界**静默丢弃、无响应** | 必须先做 `KADEMLIA2_REQ/RES (0x21/0x29)` 迭代查找，定位 tolerance zone 内节点；✅ v2.4.7 已在 `live` 实现（最多 3 跳，未收敛则跳过该样本、不发布） |
| 命名空间 Key 派生 | L1 预演取 SHA-256 前 16 字节，L2 探针取 SHA-1 前 16 字节，均非协议规定 | 两层 Key ID 不可直接比较；最终 160-bit → 128-bit 映射规则须由 ADR / PROTO 定稿 |
| 明文 UDP / UDP key | `ClientUDPSocket.cpp` 对首字节 `0xE4` 直接放行；`PacketTracking.cpp` 的 key 形参实际未启用；key 为 `(m_key, m_ip)` 且绑定源 IP | **明文不会被「因为没加密」而静默丢弃**；key 只用于反 IP 欺骗与「已验证联系人」标记，且不能从 `nodes.dat` 跨主机复制 |
| 响应帧形态 | >200 字节响应改 `0xE5` + zlib；`SendMyDetails` 对 kadVersion ≥ 6 才用 RC4 加密 `HELLO_RES` | 实测同一 LAN 节点对 `BOOTSTRAP_REQ` 交替回 `0xE4` 明文与 `0xE5` 压缩帧；实网流程跳过 HELLO，`BOOTSTRAP/PUBLISH/SEARCH` 的响应发送端均传 `cryptTargetID = NULL`；✅ v2.4.7 已实现 zlib 解码，两种帧统一走 `decode_wire()` |

协议依据固定为 aMule `master` @ `6a1105634d81242a581a0f36a7e00379b12a83f3`，
逐条出处见 [kad-mini/README.md](kad-mini/README.md)。**不引用二手资料的 opcode 表**：
核查中发现 DeepWiki 把 `PUBLISH_KEY_REQ` 记为 `0x35`，而源码 `0x35` 实为 `SEARCH_NOTES_REQ`，
真实值是 `0x43`。

## 1. 阶段排期

| 阶段 | 交付物 | 退出条件 | 状态 |
| :--- | :--- | :--- | :--- |
| **P0** 协议核实 | opcode / 帧头 / Contact / Tag / 距离校验 / 版本号的源码级依据 | 每个常量能指向 aMule 源码行 | ✅ 2026-11-09 |
| **P1** Wire codec | `kad-mini/src/wire.rs` + `src/inflate.rs`（std-only zlib/DEFLATE）：Kademlia2 帧编解码、128-bit ID、XOR 距离、命名空间 Key 派生、Tag 编解码、`0xE5` 解包 | `kad-mini selftest` **37/37** 全绿，含三种 DEFLATE 块型与真实 `0xE5` 帧验证 | ✅ 2026-11-09（v2.4.7 补解码） |
| **P2** 本地真实报文闭环 | `kad-mini loopback`：BOOTSTRAP → HELLO → 迭代查找 → PUBLISH_KEY/RES → SEARCH_KEY/RES，全程 `0xE4` 真实报文 | 报告 `public_kad_contacted=false`、命中率与逐次原始记录完整 | ✅ 2026-11-09 |
| **P3** 实网门禁与参数 | 用户提供的 bootstrap 节点清单、G0 确认的最小样本量与限速阈值、`pending` 项清除 | `spikes.json` 的 `public_kad_approval` 不再含 `pending`；`kad-mini live --check` 通过 | ✅ 2026-11-09（见 §3） |
| **P4** 实网小样本执行 | 逐次发布/查询原始记录、成功率、RTT 分布、丢弃/拒绝观测 | 由实验执行人依据证据人工回填 `S2.status` | ⏳ **未执行**；门禁已解锁且 v2.4.7 已补齐 P4 前置实现，须显式运行 `live`（不带 `--check`）才会发包 |

## 2. 红线（不可通过参数绕过）

1. **不内置默认 bootstrap 节点列表**（[OPS §6](../../../docs/07-operations/OPS.md)）：`live` 模式必须由用户通过 `--bootstrap <file>` 显式提供，仓库不含任何公共节点 IP/域名。
2. **禁止批量发布测试 Key**：`--sample-size` 硬上限 10；每条约 1 个 `PUBLISH_KEY_REQ` + 1 个 `SEARCH_KEY_REQ`。
3. **限速**：`--rate-limit` 必填且 ≤ 1.0 包/秒（比 ADR-001 的 20/s 更保守），令牌桶强制。
4. **人工批准 + 参数一致**：`spikes.json` 的 `public_kad_approval.approved == true`、`pending` 已清除、
   `confirmed.sample_size` / `confirmed.rate_limit_pps` 存在；`live_gate` 会**逐项比对 CLI 参数与 G0 确认值**，
   不一致直接拒绝（防止批准记录沦为装饰）。
5. **双开关**：必须同时给 `--live` 与 `--attest-i-am-the-experiment-operator`。
6. **逐次原始记录**：每个收发报文记录时间戳、方向、opcode、完整十六进制、来源/目的地址，落盘后才发下一个包。
7. **不自动置 `pass`**：程序只产出观测；结论由实验执行人手工回填 `spikes.json`。
8. **本地回环不得判 `pass`**：报告固定写 `public_kad_contacted=false` 与 `evidence_valid_for_pass=false`。
9. **bootstrap 必须是公网可路由 IPv4**（v2.4.7）：`live_gate` 实际解析清单并拒绝 RFC1918 / loopback /
   link-local / CGNAT / 组播 / 文档保留段，也拒绝带 `lan-interop-probe-only` 标记的文件；
   防止把 LAN 节点当成实网目标发布。
10. **未收敛不发布**（v2.4.7）：`live` 先以 `KADEMLIA2_REQ/RES` 迭代定位 `SEARCHTOLERANCE`
    邻域节点；找不到就跳过该样本并记录 `skipped`，不制造必然被静默丢弃的无效发布。
11. **跳过 HELLO、绑定通配地址**（v2.4.7）：与 `confirmed.protocol_scope` 一致，`live` 不发
    `HELLO_REQ`；socket 改用 `bind_any()`，否则 127.0.0.1 绑定的 socket 根本无法向公网发包。

## 3. P3 项目所有者确认清单（2026-11-09 全部完成）

- [x] **G0 确认的最小样本量**：`sample_size = 3`（建议区间 3～5 取最保守值）
- [x] **G0 确认的限速阈值**：`rate_limit_pps = 0.2`（建议区间 0.2～1.0 取最保守值，即每 5 秒 1 个包）
- [x] **bootstrap 节点清单**：项目所有者提供 `nodes.dat` / `server.met`，由
      [`nodes_dat_to_bootstrap.py`](nodes_dat_to_bootstrap.py) 转成本地 JSON；仓库不内置任何节点 IP（见 §5）
- [x] **明文 / UDP key 决策**：**保持明文，跳过 HELLO，不实现 sender/receiver UDP key 与 RC4**。
      源码核实与 LAN 实测表明明文 `0xE4` 不会被「因未加密」而丢弃；真正需要处理的是
      `0xE5` 压缩帧与 RC4 加密的 `HELLO_RES`，而发布/查询链路的响应均为明文（见 §0、§5）
- [x] 清除 `S2.public_kad_approval.pending`，写入 `confirmed` 与上述阈值；`live_gate` 已加参数一致性校验

> P3 完成**不等于** P4 已执行。`kad-mini live --check` 只报告门禁状态、不发包；
> 实网发布必须由实验执行人显式去掉 `--check` 并给出 `--attest-i-am-the-experiment-operator`。
> 在此之前 `S2.status` 保持 `not_run`，公共 KAD 未被接触。

v2.4.7 实测门禁：公网 bootstrap 清单 + 完整参数 → `gate.passed=true`（不发包）；
LAN 清单 → 同时命中「`lan-interop-probe-only` 标记」与「非公网地址」两条 blocker；
无标记的 RFC1918 清单 → 仍被公网 IPv4 校验拦截。三条路径均 `public_kad_contacted=false`。

## 4. 与既有文档的关系

- P0–P2 的产出登记在 `spikes.json` 的 `S2.observations.real_wire`，**不改 `S2.status`**。
- **v2.4.6 已按项目所有者指示回写设计基线**：`S2.criteria` / `S2.assumption`、
  [PROTO §1/§2.0/§2.1/§4](../../../docs/04-protocol/PROTO.md)、
  [ADR-001](../../../docs/02-architecture/adr/ADR-001-kad-network.md)、
  [ADR-002](../../../docs/02-architecture/adr/ADR-002-bt-engine.md)（S1 握手回调发现）、
  [RISK R-02](../../../docs/05-quality/RISK.md)（三桶观测口径）。
  只改 KAD 集成映射与观测语义，**不动 §3 私有控制面字段**，`PROTOCOL_VERSION` 保持 1、T0 golden 不变。
- 项目根仍**无** `Cargo.toml`，`src/` 未动；`kad-mini` 是 `tests/spikes/s2/` 下的独立 Cargo 工程，遵循 G0 门禁。

## 5. 用户提供的节点文件与 LAN 互操作探测（2026-11-09）

| 文件 | 结论 |
| :--- | :--- |
| `nodes.dat` | aMule `fileVersion=2`，**200 个 KAD contacts，全部 verified**，KAD 版本分布 8/9/10（另有 1 个 v4）；可作公共 KAD bootstrap 来源。每条含 `(m_key, m_ip)` 形式的 `CKadUDPKey`，**绑定原机器公网 IP，跨主机不可用**，转换脚本刻意不导出 |
| `server.met` | magic `0xE0`、17 个 **eD2K 服务器**条目，不是 KAD 路由表，对 KAD bootstrap **无用**，仅登记不解析 |
| LAN eMule（RFC1918 私有地址，具体 IP/端口只存于本地 bootstrap 文件） | 用户提供的 TCP / UDP 端口；KAD 实测在线 |

[`nodes_dat_to_bootstrap.py`](nodes_dat_to_bootstrap.py) 只读解析并输出两个**被 `.gitignore` 排除**的本地文件：

```
tests/spikes/s2/reports/bootstrap.public.local.json   # 3 个公共节点，供 P4 用
tests/spikes/s2/reports/bootstrap.lan.local.json     # 仅 LAN 节点，供互操作探测，禁止用于 live 发布
```

`kad-mini lan-probe --ip <RFC1918_IP> --port <UDP>` 只发 `BOOTSTRAP_REQ` / `HELLO_REQ`、
**不发任何 PUBLISH**，硬编码拒绝非私有地址，报告固定 `public_kad_contacted=false`、
`evidence_valid_for_pass=false`、`publish_sent=false`。实测结果：

| 探测 | 观测 | 归因 |
| :--- | :--- | :--- |
| `BOOTSTRAP_REQ (0x01)` | 一次得到 `0xE4 0x09` 明文 **523 B**（RTT ≈ 13 ms）；另一次得到 `0xE5 0x09` **519 B** zlib 压缩帧 | 节点可达、KAD 在跑；响应形态不稳定，探针已能分类但尚无 zlib 解码 |
| `HELLO_REQ (0x11)`，声明 v8 与 v7 | 均无响应（3～5 s 超时） | 对应 `SendMyDetails` 对 kadVersion ≥ 6 的 RC4 路径与「未验证联系人」处理；不据此判定节点故障 |

**这就是不补 sender/receiver UDP key 的实测依据**：明文请求能拿到明文响应，
问题集中在「压缩帧解析」和「HELLO 路径」两处，而 P4 的发布/查询链路可以绕开 HELLO。
v2.4.7 已追加 std-only 的 `src/inflate.rs`（zlib/DEFLATE，覆盖 stored / fixed / dynamic
三种块型），并用此前捕获的**真实 519 B `0xE5` 帧**验证：解出 521 B `BOOTSTRAP_RES` payload、
20 个联系人，与 Python `zlib.decompress` 一致。自此 `0xE5` 不再落入「协议层不可解析」桶。
RC4 + key 协商仍刻意不实现（工作量远大于 zlib，且 P4 链路用不到）。

**LAN 节点不是 S2 的实网证据**：它是私有子网上的单机，按 `S2.criteria` 第 5 条只作预演；
且若它同时连着公共 KAD，向其发布测试 Key 可能被转发到公共网络，因此明文规定禁止在 LAN 上发布。

## 6. v2.4.7 P4 前置一致性修正（2026-11-09）

v2.4.6 把 `S2.criteria` 改对了，但复核 `kad-mini live` 实现时发现**四处会让 P4 产出假结论**的错配，
已全部修复并复验：

| 错配 | 后果 | 修复 |
| :--- | :--- | :--- |
| `live` 直接向 bootstrap 节点 `PUBLISH_KEY_REQ` | 违反 `criteria` 第 2 条，越界发布被静默丢弃，会把「必然的协议行为」误记为「索引层拒绝」 | 发布前用 `KADEMLIA2_REQ/RES` 最多 3 跳迭代收敛到 `SEARCHTOLERANCE` 邻域；未收敛则跳过该样本并记录 `skipped` |
| `live` 仍发 `HELLO_REQ` | 与 `confirmed.protocol_scope`「跳过 HELLO」矛盾；v7/v8 的 RC4 `HELLO_RES` 必然超时 | 移除 HELLO 交换，逐次记录 `hello=skipped_per_p4_protocol_scope` |
| `0xE5` 压缩帧被当作「协议层失败」 | LAN 实测约半数 `BOOTSTRAP_RES` 是压缩帧，会把正常响应计入失败桶 | 新增 std-only `inflate.rs`，`decode_wire()` 统一处理 `0xE4`/`0xE5`；selftest 37/37，真实帧解码验证通过 |
| `live_gate` 只数 `"ip"` 字段，不校验地址 | `bootstrap.lan.local.json` 能通过门禁；`Client::bind` 还把 socket 绑在 127.0.0.1，根本发不出去 | 门禁实际解析并拒绝非公网 IPv4 / `lan-interop-probe-only`；`live` 改用 `bind_any()` |

复验结果：`cargo test` 7/7、`selftest` 37/37、`loopback` 5/5 发布 + 5/5 命中 + 84 条原始记录 + 0 协议错误、
真实 LAN `BOOTSTRAP_RES` 解出 20 个联系人、公网清单 `live --check` 放行 / LAN 清单拒绝（均不发包）。
**P4 仍未执行**，`S2.status` 仍为 `not_run`，公共 KAD 未被接触。
