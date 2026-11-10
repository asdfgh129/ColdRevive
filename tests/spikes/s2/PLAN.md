# S2 实网实施排期（最小 eMule KAD 客户端）

> 状态：**P0–P3 已完成；P4 已于 2026-11-10 按 G0 参数两度重跑，已真实连上公共 KAD**（`0xE5` 压缩 `BOOTSTRAP_RES` + `KADEMLIA2_REQ/RES` 迭代），但 3/3 样本因未收敛到 `SEARCHTOLERANCE` 邻域而跳过发布，`S2.status` 仍 `not_run`。
> **v2.4.14 重大更正**：§7–§10 的「出口过滤高位 UDP / 换网重跑」结论**整体作废**。根因是我方把 KAD 联系人 4 字节 IP 按点分十进制顺序读取，而线上是**反序**；所有历史 0/N 结果都是把包发给了不存在的地址。修正后本机 10 目标探针 **9/10 收到真实 `BOOTSTRAP_RES`**，高位 UDP 一直是通的。详见 §11。
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
| **P4** 实网小样本执行 | 逐次发布/查询原始记录、成功率、RTT 分布、丢弃/拒绝观测 | 由实验执行人依据证据人工回填 `S2.status` | ⚠️ **2026-11-10 已执行但 inconclusive**：3 × `BOOTSTRAP_REQ` 发出、0 响应、0 发布；环境过滤公网高位 UDP，`S2.criteria` 未被实际执行，须在无过滤网络重跑（§7） |

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

> P3 完成**不等于** P4 已产出结论（P4 已于 2026-11-10 执行但被环境阻断，见 §7）。`kad-mini live --check` 只报告门禁状态、不发包；
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

## 7. P4 首次执行记录（2026-11-10）——环境阻断，结论不可用

项目所有者于 2026-11-10 显式下达「执行 P4」。`live --check` 先复验门禁：
`gate.passed=true`、0 blocker、G0 参数逐项一致、`public_kad_contacted=false`（不发包）。
随后执行：

```text
kad-mini live --bootstrap <PUBLIC_LIST> --sample-size 3 --rate-limit 0.2 \
              --attest-i-am-the-experiment-operator
```

### 原始观测

| 项 | 结果 |
| :--- | :--- |
| 门禁 | passed，0 blockers；`approved=true`、`pending` 已清、`confirmed` 与 CLI 一致 |
| 耗时 | 2026-11-10T00:26:52Z → 00:27:05Z（13 s） |
| 发出报文 | **3** 条，均为 2 字节 `e401` `KADEMLIA2_BOOTSTRAP_REQ`，间隔严格 5.000 s（0.2 包/秒） |
| 收到响应 | **0**（三条全部 WSAETIMEDOUT 10060 = 3 s 读窗口内无数据报） |
| HELLO / 迭代 / PUBLISH / SEARCH | **0 / 0 / 0 / 0** |
| `tolerance_reached` | 0/3；`known_contacts=0`，3 个样本全部按 `S2.criteria #2` 记 `skipped` 并跳过发布 |
| 报告 | `tests/spikes/s2/reports/kad-live-20261110002705.json`（gitignored，含完整逐次原始报文） |
| `evidence_valid_for_pass` | `false`；`s2_status` 仍为 `not_run` |

### 归因对照实验（关键：区分环境与协议）

| 对照 | 观测 | 说明 |
| :--- | :--- | :--- |
| DNS/UDP-53 → 8.8.8.8 / 1.1.1.1 | 成功，61 B，RTT ≈ 53～58 ms | 主机具备 UDP 出站与回程能力 |
| STUN/UDP-19302 → `stun.l.google.com` / `stun1.l.google.com` | 3 s 超时 | 与 KAD 无关的公网高位 UDP 往返失败 |
| STUN/UDP-3478 → 8.8.8.8 | 3 s 超时 | 同上 |
| 直连 TCP → 1.1.1.1:443（绕过代理） | 6 s 超时 | 直连 TCP 被挡；但 v2.4.9 复核发现 UDP/123 NTP 可直接出网，故「整张网只走 HTTP 代理」的措辞过强，已修正 |
| **LAN 真实 eMule 对照** | 同一份 `e401` → `0xE4 0x09` 明文 **523 B / 20 contacts** | **报文格式与探针实现正确** |
| aMule 源码复核 | `ProcessPacket` 对 `KADEMLIA2_BOOTSTRAP_REQ` 直接分发；请求无需 payload；仅 `InTrackListIsAllowedPacket` 每 IP 限流与 `port == 53` 特例 | 排除「报文不合法被丢弃」 |

结论：**本机所在的公网出口按目标端口/应用做了白名单，高位未知端口被丢弃**（v2.4.9 细化，见 §8），
不是 KAD 报文格式问题、不是节点拒绝、不是命名空间被拒。按 RISK R-02 口径，这三次超时属于
「环境前置条件失败 / 协议层无响应」，**不计入索引层拒绝**，也不得据此判定 S2。

⚠️ **v2.4.9 更正两点**：① LAN 探测是纯 LAN 流量、未经过公网出口，**不能**作为公网 UDP 可达性
证据；② `nodes.dat`（2026-11-08）联系人新鲜度是**独立的**次要风险，原先把「次要」判得过于确定
——3 个从两天前清单里随机挑的联系人，即使在健康网络上也可能全部离线。

### 门禁表现得很好

P4 没有产生任何测试 Key 发布：因为 `BOOTSTRAP_RES` 全超时导致学不到联系人，
`live` 拒绝在 `SEARCHTOLERANCE` 邻域之外发布，3 个样本全部 `skipped`。
这正是 v2.4.7 修复的目的——避免把「必然被静默丢弃」误记成「索引层拒绝」。

### 重跑前置条件（新增）

```bash
python3 tests/spikes/s2/udp_egress_check.py   # 可选手动诊断，不发任何 KAD 报文
echo $?   # 0 = 公网高位 UDP 往返正常，可跑 P4；2 = 被过滤，禁止在此网络跑
```

该脚本只做低位端口与高位端口 UDP 往返对照，不内置也不接触任何 KAD bootstrap 节点，
不参与 `kad-mini live`。v2.4.9 起支持 `--source-port`、`--lan-ip/--lan-port`（仅 RFC1918）、
`--json`。**在退出码为 0 的主机上，按同样的 G0 参数重跑 P4**，再由实验执行人依据逐次原始
记录手工回填 `S2.status`。在此之前 `S2.status` 保持 `not_run`。

**跨主机 STUN 对照（v2.4.9 提出，2026-11-10 已执行）**：同一份脚本在 LAN 那台 eMule 机器上
同样得到 `BLOCKED`（低位 4/4 OK、高位 0/3 OK、退出码 2），按主机的 STUN 出站例外假设被否证。
**v2.4.11 收窄**：STUN 不等于 KAD；本机已由 KAD 专用探针确证不可达，LAN eMule 主机仍待
§8.6 核实，暂不下整网绝对结论。

## 8. 环境归因复核：按端口白名单 + 跨主机已闭环（2026-11-10，v2.4.9 / v2.4.10）

项目所有者指出：**局域网内那台 eMule 与本机是同一个网络环境、没有额外设置，唯一差异是
eMule 开了入站映射**。这对 §7「网络统一过滤公网高位 UDP」的归因提出了合理质疑，遂补做
更宽的对照实验。本节只修正归因与诊断脚本，不改协议设计基线、不动 `S2.status`。

### 8.1 复核观测（本机为 RFC1918 私网地址，网关为同网段私网地址）

| 探测 | 结果 | 排除什么 |
| :--- | :--- | :--- |
| UDP/53 DNS → 8.8.8.8、1.1.1.1 | OK，42～58 ms | UDP 出站、回程与状态化 NAT 正常 |
| UDP/123 NTP → time.cloudflare.com、pool.ntp.org | OK，23～190 ms | **「UDP 全被封」「整张网只走 HTTP 代理」均不成立** |
| UDP/3478 STUN → Cloudflare、ekiga；UDP/19302 → Google | 全部超时 | 非 Google 单一来源问题；高位未知端口不通 |
| 同一批 STUN 改用与 LAN eMule 相同的固定源端口、以及 4672 重发 | 仍全部超时 | 不是按源端口放行 |
| Windows 防火墙 | 出站阻断规则 0 条，默认出站允许 | 不是本机防火墙 |
| LAN eMule 的高位 UDP 端口，同一帧 `e401` | OK 523 B / 9 ms | 本机 socket 栈与 LAN 路径无问题 |

同一家 Cloudflare：`123` 通、`3478` 不通。**修正后的归因是「公网出口按目标端口/应用白名单
放行低位知名端口、丢弃高位未知端口」**，而不是「UDP 被整体过滤」。§7 中「整张网只走 HTTP
代理出网」的措辞过强，此处撤回。

### 8.2 入站映射到底解决什么（源码核实）

`KademliaUDPListener.cpp::Process2BootstrapRequest` 用请求的 `ip, port` 发回
`SendPacket(..., KADEMLIA2_BOOTSTRAP_RES, ip, port, senderKey, NULL)` —— 响应回到**请求的
源 IP:源端口**，状态化 NAT 自动放行。`ProcessPacket` 里唯一的源端口检查是
`if (port == 53 && senderKey.IsEmpty()) return;`（防 DNS 协议混淆攻击），与映射无关。

所以：

- **请求/响应模式（P4 探针）不需要入站映射**，开 DNAT 也不能让出站高位包出去。
- 入站映射决定的是**能否作为可被其他节点主动联系的路由节点**（eMule 的 firewalled 状态），
  这是 eMule 维持公共 KAD 路由表所必需的，但不是 bootstrap 响应能否回来的原因。
- 该假设已由 §8.4 的跨主机实验否证：那台 eMule 的高位 UDP（STUN 口径）同样被挡，入站映射
  **没有**带来 STUN 出站例外；给本机申请入站映射也不起作用。
- **v2.4.12 更正**：本条原先推测「该机若连着公共 KAD，更可能是 firewalled / LowID」，已被
  §9 的完整 `Kad 网络` 面板否证（状态与 UDP 状态均为「打开」、无 Buddy 行）。但「打开」
  是**入站可达性**结论，仍不能推出该机具备公网 KAD 出站能力，见 §9.1。

### 8.3 LAN 对照的逻辑边界（原归因的漏洞）

`lan-probe` 的目的地是 RFC1918 地址，流量**不经过公网出口**。它证明 `0xE4 0x01` 报文格式与
探针实现正确、本机与 LAN 路径允许高位 UDP，但**既不能证明也不能否证**公网高位 UDP 可达。
§7 把它列为「环境对照」时没有说清这条边界，v2.4.9 在此明确。

### 8.4 跨主机 STUN 对照（2026-11-10 已执行；结论范围见 §8.6 收窄）

项目所有者把同一份单文件、零依赖 [`udp_egress_check.py`](udp_egress_check.py) 拷到
LAN 那台 eMule 机器（与本机同网段、同一网络环境，且**开了入站映射**）上运行，输出：

```text
-- LOW-PORT CONTROL     4/4 OK   8.8.8.8:53 / 1.1.1.1:53 /
                                 time.cloudflare.com:123 / pool.ntp.org:123
-- HIGH-PORT PROBE      0/3 OK   stun.cloudflare.com:3478 / stun.ekiga.net:3478 /
                                 stun.l.google.com:19302 全部 TimeoutError
RESULT: BLOCKED (exit 2)
```

| 判定 | 结果 | 结论 |
| :--- | :--- | :--- |
| 出口例外按主机生效（STUN 口径） | **否证** | 开了入站映射的那台机器 STUN 高位 UDP 同样被挡，映射不产生 STUN 出站例外 |
| 本网存在可跑 P4 的主机 | **v2.4.11 起不下绝对结论** | 本机已由 §8.6 的 KAD 专用探针确证不可达；LAN eMule 主机仍有 KAD 存活迹象待核实 |

由此 §8.2 的条件推论收敛：入站 DNAT 只决定「能否被外部主动联系」，与出站白名单无关。
但本节**只证明 STUN 报文**在两台主机上都出不去；STUN 不等于 KAD（见 §8.6）。给本机追加
更多入站映射不解决出站过滤。

**重跑前须完成 §8.6 的两项决定性核实**；若确认本网无可用出口，唯一有效路径仍是换到高位
UDP 未被过滤的网络 → 诊断退出码 0 → **刷新 `nodes.dat`** → 按 G0 参数
（`sample_size=3` / `rate_limit_pps=0.2`）重跑 P4。另外，即便换到健康网络，从两天前的
清单里随机挑 3 个联系人也可能全部离线，有效重跑前须先刷新联系人清单或在 `live` 中增加
备用 bootstrap 与重试。

### 8.5 脚本升级

`udp_egress_check.py` v2.4.9：新增 NTP/123 低位对照、非 Google STUN 目标（去掉单一来源
混淆）、`--source-port`（测源端口策略）、`--lan-ip/--lan-port`（可选 LAN 高位对照，硬校验
RFC1918/loopback，绝不向公网发 KAD 包）、`--json`。退出码语义不变：`0` 可跑 P4、`2` 阻断、
`1` 诊断错误。本机复验：低位对照 **4/4 OK**、高位 **0/3 OK**、LAN 对照 OK，退出码 **2**；
同一脚本在 LAN eMule 机器上复验同样退出码 **2**（v2.4.10，见 §8.4），脚本提示语已改为
通用跨主机用法，可继续搬运到新网络使用。

**状态不变**：`S2.status = not_run`、`PROTOCOL_VERSION = 1`、T0 golden files 未动、
RISK R-02 三桶整体作废仍成立、未发布任何测试 Key。

### 8.6 新证据与归因收窄：STUN ≠ KAD（2026-11-10，v2.4.11）

项目所有者提供 LAN eMule 主机两张截图：eD2K 服务器 eMule Sunrise（44,225 用户）上本客户端
为 **HighID**；Kad 页签 **节点 (432)**，联系人类型 / 版本为 `0(8)`、`0(9)`、`0(10)`、部分
`2(10)`。同时本机 `lan-probe` 早先从该客户端 `BOOTSTRAP_RES` 解出 **20 个联系人、17 个公网
IPv4**（多条使用标准 eMule UDP 端口 `4672`），与 `nodes.dat`（2026-11-08）里 P4 用的 3 个
联系人**完全不重叠**。

**这使 v2.4.10 的绝对措辞不再成立**：`udp_egress_check.py` 用 STUN 作代理，STUN 超时只能
证明「STUN 目标端口 / 该类报文被过滤」，推不出「所有高位 UDP，尤其 KAD 报文」都被过滤。
出口策略可能是按应用 / DPI、按目标端口（例如放行 `4672`）、或按「主机 IP + 源端口」放行的。
`nodes.dat` 陈旧也不再是 P4 零响应的唯一竞争解释 —— 现在有了明显更新的联系人清单。

概念澄清：**HighID 是 eD2K 服务器分配的客户端 ID**，证明 DNAT 让服务器能回连本客户端 TCP
端口；KAD 有独立的「状态 / 防火墙 / Buddy」，两图里 `Kad 网络` 面板被裁掉、尚未看到。Kad
页签的 432 条是**路由表联系人**（`nodes.dat` 载入 + 入站请求学习），不等于此刻存在双向 UDP
流量；`0(8)` 这类值是联系人类型与 KAD 协议版本，不是 HighID。

#### 8.6.1 本机 KAD 专用探针实测（新增 [`kad_egress_probe.py`](kad_egress_probe.py)）

只发 2 字节 `0xE4 0x01` `BOOTSTRAP_REQ`，不发布 Key、不发 HELLO / REQ / SEARCH；目标来自
用户提供的 LAN 客户端 `BOOTSTRAP_RES`（3 条，含标准 `4672` 与非常规高位端口），G0 参数
3 条 / 0.2 包每秒，逐次留原始报文：

| 源端口 | KAD 响应 |
| :--- | :--- |
| 系统临时端口 | **0/3** |
| 固定为 LAN eMule 的 KAD 源端口 | **0/3** |
| 固定标准 eMule UDP 端口 `4672` | **0/3** |

→ **本机确实无法与公网 KAD 节点完成一次往返**，不是「只有 STUN 不通」、不是「P4 只是挑到
死联系人」、也不是源端口策略问题。§7 对本机的环境阻断结论成立。

#### 8.6.2 仍未判定的是 LAN eMule 主机

**v2.4.13 更新**：第 ① 项（面板截图）见 §9；第 ② 项（在该机跑 KAD 专用探针）**已执行，
结果 0/3 / 退出码 2，见 §10.1**；`统计信息` 计数与「Kademlia 方法搜索」仍未核实，但已
不阻塞换网重跑的工程决策（见 §10.4）。

原有三种可能：

1. KAD 真的活着 → 出口按应用 / 按「该机 IP + 源端口」放行，STUN 与临时端口探针都是假阴性；
2. KAD 已 firewalled、有 Buddy，432 条为陈旧路由表，靠 DNAT 接收入站请求 + 状态化 NAT
   回包维持 → v2.4.10 的整网结论成立（**已被 §9 面板否证**）；
3. eMule 走了另一条出口 / 代理。

决定性核实：① 面板截图（已到，见 §9）与 `统计信息` 里 KAD 收发包计数是否增长（仍缺）；
② 把 [`kad_egress_probe.py`](kad_egress_probe.py) 与本地 `bootstrap.live.local.json` 拷到该机
运行，看退出码 `0` / `2`（命令见 §8.6.3）。`0` → 可从该机重跑 P4；`2` → 还须结合计数增长与否
再判断，**不得**直接推出整网结论。

#### 8.6.3 跨主机 KAD 专用探测命令

```bat
python kad_egress_probe.py --targets bootstrap.live.local.json --sample-size 3 --rate-limit 0.2 --attest-i-am-the-experiment-operator
echo %ERRORLEVEL%
```

`0` = 至少一个 `BOOTSTRAP_RES`（KAD 往返通）；`2` = 零响应；`1` = 诊断错误。脚本硬校验目标
必须为公网 IPv4、目标清单不内置、必须显式 attest；报告写入 gitignored `reports/`；
`public_kad_contacted=true` 但 `evidence_valid_for_pass=false`，**不是 S2 结论**。
若该机 KAD 确实存活而临时端口探针仍失败，可临时停掉 eMule 后加 `--source-port <该机 KAD 端口>`
复测，以区分「按 IP + 源端口」策略。

**v2.4.13 已执行**：项目所有者在 LAN eMule 机器上运行上面这条命令，得到 `NO_REPLY (0/3)`
/ 退出码 2，逐字输出与判定见 §10.1。为消除两次 0/3 共用的「死联系人」混淆，另新增
[`bootstrap_res_to_targets.py`](bootstrap_res_to_targets.py) 生成新鲜目标；宽扫命令：

```bat
python kad_egress_probe.py --targets bootstrap.fresh.local.json --sample-size 10 --rate-limit 0.2 --attest-i-am-the-experiment-operator
echo %ERRORLEVEL%
```

`--sample-size 10` 是**诊断**而非 P4，仍只发 `0xE4 0x01`，在所有者的硬上限
（`sample_size_max=10` / `rate_limit_pps_max=1.0`）之内，`evidence_valid_for_pass=false`。

**状态不变**：`S2.status = not_run`、`PROTOCOL_VERSION = 1`、三桶整体作废、未发布任何测试 Key。
v2.4.10 的「整网无主机可跑 P4」降级为「本机已确证不可跑；该机独立进程 0/3，工程上按换网处理」。

## 9. LAN eMule 主机 `Kad 网络` 面板证据与源码级复核（2026-11-10，v2.4.12）

项目所有者补上 §8.6.2 要求的第 ① 项：`Kad 网络` 面板完整截图（右缘仍裁掉了 `IP:端口` 的
端口部分）。逐字转录（公网 IP 与 ID 已脱敏）：

```text
Kad 网络
状态:        打开
UDP 状态:    打开
IP:端口:     112.5.x.x:<端口被裁切>
ID:          <数字 ID，已脱敏>
```

面板中**没有** `防火墙` / `Buddy` 行。eMule 官方帮助把 KAD 状态区分为「打开」与
「通过防火墙 / 被防火墙阻挡」；[aMule 官方手册 Networks / Kad Info](https://amule-org.github.io/docs/manual/interfaces/gui/networks)
把 `Connection State` 与 `UDP Connection State` 定义为 `OK` 或 `Firewalled`，并明确「两者均 OK
时不显示 Firewalled/Buddy 行」。结合 eD2K HighID 与 Kad 页签 432 个联系人，可判定该客户端
**自报 KAD 与 KAD UDP 均非 firewalled**。

→ §8.6.2 的假设 ②（KAD 已 firewalled、靠 Buddy 与陈旧路由表维持）**被否证**；v2.4.10 里
「该机更可能是 LowID / firewalled」的推测同样撤回。

### 9.1 但「打开」不等于「能跑 P4」：KAD UDP 防火墙检测是入站测试

按 §0 同一源码族（aMule master）逐处核实，结论见表：

| 源码位置 | 事实 |
| :--- | :--- |
| `src/ClientList.cpp::DoRequestFirewallCheckUDP` | 只用 `contact.GetTCPPort()` 构造客户端并置 `KS_QUEUED_FWCHECK_UDP`；状态机 `TryToConnect(true)` 走 **TCP**，**不发** UDP 到联系人的 KAD 端口 |
| `OP_KAD_FWCHECKUDP_REQ` 处理 | 远端回一个**无载荷 UDP 包** `KADEMLIA_FWCHECKUDP_RES`，目的 = 请求方 `client->GetIP()` : `client->GetKadPort()`（未经请求的入站包） |
| `src/kademlia/net/KademliaUDPListener.cpp::Process2FirewallUDP` | 收到且 `incomingPort` 等于本机内 / 外 KAD 端口 → `SetUDPFWCheckResult(true, false, ...)` |
| `src/kademlia/kademlia/UDPFirewallTester.cpp` | `m_firewalledUDP` **初值即 `false`**；成功置 `false` + `m_isFWVerifiedUDP=true`；只有全部测试失败才置 `true`；6 分钟超时降级还要求 `CKademlia::IsFirewalled()` 为真 |
| `KademliaUDPListener.cpp::ProcessFirewalledResponse` / `Process2Pong` | 外部 IP 来自 `KADEMLIA_FIREWALLED_RES`（4 B），外部端口来自 `KADEMLIA2_PONG`（2 B）；eD2K HighID 也能提供公网 IP |

由此：

1. **该检测测的是「入站可达性」**：远端从公网向本机公网 IP:KAD 端口发一个未经请求的 UDP
   包，收到即「打开」。这正是**入站映射 / DNAT 提供的能力**，协调与触发走 TCP，**不需要本机
   向公网高位 UDP 端口发出任何报文**。
2. 所以「该机 STUN 高位 UDP 全超时」与「该机 KAD UDP 状态打开」**并不矛盾**，二者可同时成立：
   出站高位 UDP 被过滤、入站 DNAT 通行。§8.4 的观测无需用「按应用 / DPI 放行」解释。
3. `m_firewalledUDP` 初值为 `false`，且 HighID（TCP 非 firewalled）下超时降级路径可能不触发；
   若 UDP 检测从未收到结果，「打开」也可能是**未经核实的乐观默认值或上次连接残留**。
4. 432 条是路由表联系人（`nodes.dat` 载入 + 入站请求学习），不是流量计数；面板显示公网
   `IP:端口` 也可由 eD2K HighID 提供，均不能单独证明发生过公网 KAD UDP 往返。

### 9.2 假设表更新

| §8.6.2 假设 | v2.4.12 判定 |
| :--- | :--- |
| ① KAD 真活 → 出口按应用 / 按「该机 IP + 源端口」放行 | **仍开放**，但不再是唯一解释；须由该机 KAD 专用探针 + 统计计数判定 |
| ② KAD 已 firewalled / 有 Buddy、432 条为陈旧路由表 | **否证**：面板两行均「打开」、无 Buddy 行 |
| ③ eMule 走另一条出口 / 代理 | **仍开放** |
| ④ 新增：出口统一过滤出站高位 UDP，而「打开」由入站 DNAT + 乐观默认状态产生 | **无法排除**，且与目前全部观测一致 |

### 9.3 决定性核实（不变，且必要性更强）

1. ~~在 LAN eMule 机器运行 §8.6.3 命令~~ → **v2.4.13 已执行，结果 `2` / 0/3，见 §10.1**。
   该机的独立进程没有 blanket 出站例外；但按下方判据边界，`2` 不是充分否证。
2. 同机查看 `统计信息` 中 **KAD 收发包计数是否随时间增长**（本次最关键的补充证据）：
   - 计数增长 + 探针 `0` → ① 成立，出口对 KAD 有选择性放行；
   - 计数增长 + 探针 `2` → 出口按应用 / 进程 / DPI 放行，Python 探针是假阴性，须查差异
     （含停掉 eMule 后 `--source-port <该机 KAD 端口>` 复测）；
   - 计数不增长 + 探针 `2` → ④ / ③ 更可能，本网无可用 P4 出口，换网并刷新 `nodes.dat`。
3. 判据边界：探针 `0` 是**充分证据**；探针 `2` 在存在按应用放行时**不是充分否证**。
   不得再用一次 `2` 推出「整网无主机可跑 P4」。

**状态不变**：`S2.status = not_run`、`PROTOCOL_VERSION = 1`、T0 golden files 未动、
RISK R-02 三桶整体作废、未发布任何测试 Key。本节的面板截图属**环境诊断证据**，
`evidence_valid_for_pass = false`，不进入 S2 判据。

## 10. 跨主机 KAD 探针已执行 + eD2k / KAD 判据区分（2026-11-10，v2.4.13）

### 10.1 §9.3 决定性核实第 ② 项：已执行

项目所有者在 LAN eMule 机器（同网段、开了入站映射）运行 §8.6.3 命令。主机身份核验：
本机 `C:\Users\bemin\Desktop` 下**没有**该脚本、目标文件或 `reports\` 目录，而输出中的
报告路径是 `Desktop\reports\kad-egress-probe-20261110024452.json`（脚本按自身所在目录写
`reports/`）；与上一轮跨主机 STUN 对照的机器一致。输出逐字转录（公网 IPv4 已脱敏）：

```text
S2 P4 KAD-specific UDP egress probe (BOOTSTRAP_REQ only, no publish/search)
<公网 IPv4>:4672         FAIL   timeout_or_error TimeoutError: timed out
<公网 IPv4>:4672         FAIL   timeout_or_error TimeoutError: timed out
<公网 IPv4>:<高位端口>    FAIL   timeout_or_error TimeoutError: timed out
RESULT: NO_REPLY (0/3 KAD replies).
Cannot separate egress filtering from dead contacts; do NOT count as S2 evidence.
```

退出码 **2**。§9.3 第 1 项因此闭环：**该机上的独立 Python 进程同样无法完成一次公网
KAD UDP 往返**。假设表中「该机存在 blanket 出站例外」被否证；但按 §9.3 判据边界，
`2` 在存在按应用 / 进程放行时**不是充分否证**。

### 10.2 两次 0/3 不是独立复现：共享「死联系人」混淆

本机（§8.6.1，三种源端口）与该机（§10.1）用的是**同一份 3 个目标**
（`bootstrap.live.local.json`，由 00:29 UTC 一次 LAN `BOOTSTRAP_RES` 挑出）。换了主机、
换了源端口，唯独**目标没换** —— 若这 3 个联系人已全部离线，两台主机都会 0/3，与出口
策略无关。所以跨主机结果压低了「按主机例外」的可能，**没有**压低「死联系人」的可能。

新增 [`bootstrap_res_to_targets.py`](bootstrap_res_to_targets.py)：只读解析 LAN
`BOOTSTRAP_RES`（`0xE4` / `0xE5` 帧 + `0x09`，20 × 25 B 联系人，IP 网络序 / 端口小端），
过滤公网 IPv4、去重、产出新目标清单；**不发任何包**。2026-11-10 03:12 UTC 重跑
`lan-probe` 得 523 B / 20 contacts、RTT 12 ms（该 eMule 此刻确实在线），解出 15 个公网
IPv4，取前 10 个（5 个标准 `4672` + 5 个其他高位端口），与旧 3 目标**重叠 0/10**。

本机用这 10 个新鲜目标宽扫（`--sample-size 10 --rate-limit 0.2`，只发 `e401`，在硬上限内）：
**0/10 无响应**（9 个超时 + 1 个 `WSAENETRESET`）。10 个来自活跃客户端当前路由表、含
5 个标准 `4672` 的目标全部无回包，「全是死联系人」在本机侧已不再是简洁解释；该机侧的
新鲜宽扫仍待执行（§10.5）。

### 10.3 新证据「eMule 下载 / 搜索正常」不构成 KAD 出站证据

所有者补充：「那台机器通过 eMule 下载和搜索都很正常，目前没在下载，但之前下载很正常」。
按 eMule 官方帮助逐条核对（[Network Guide](https://www.emule-project.com/home/perl/help.cgi?l=1&rm=show_topic&topic_id=232)、
[Search → Method](https://emule-project.com/home/perl/help.cgi?l=1&rm=show_topic&topic_id=133)）：

| 官方表述 | 推论 |
| :--- | :--- |
| 「eMule 实际的下载**与两个网络的选择无关**……网络拓扑只与文件搜索以及寻找来源有关；找到来源后客户端连接它」 | 下载本身是 **TCP** 到来源，与 KAD UDP 无关 |
| 搜索 `Method` 分 `Server` / `Global (Server)` / `Kademlia`；前者「逐个询问服务器」，后者「搜索整个 Kademlia 网络，**不使用 eD2k 服务器**」 | 「搜索正常」默认可能完全是 eD2k 服务器搜索（**TCP**） |
| Kademlia 方法条目：「若连热门关键词都**无结果**，系统的 **UDP 端口很可能被防火墙或路由器阻断**」 | 只有 `Kademlia` 方法**出结果**才要求 UDP 通 |
| HighID 由 eD2k 服务器检查「其他客户端能否自由连入」后分配 | TCP + 入站 DNAT，不需要出站高位 UDP |

所以**一条完整、健康的 eD2k 路径可以完全不产生任何出站高位 UDP**：搜索 / 查源走 TCP
到服务器，HighID 让远端主动连入，下载走 TCP。它与 §10.1 的 0/3、§8.4 的 STUN 0/3
**并不冲突**，反而是假设 ④（出站过滤 + 入站 DNAT + 乐观「打开」）的预期表现。
反过来，只有「明确选择 `Kademlia` 方法且热门关键词有结果」才证伪 ④。

→ **判据修正**：不必「开个下载」验证（下载不区分网络）；应改为在搜索框 `Method`
明确选 `Kademlia`，搜常见关键词看是否有结果。另注意：入站 KAD 请求触发的 UDP **回包**
属于已建立流，状态化防火墙通常放行，即便本机不能主动发起出站 KAD，计数也可能增长 ——
故「计数增长」不是充分证据，**客户端主动发起的 Kademlia 搜索**才是。

### 10.4 假设表与工程结论

| 假设 | v2.4.13 判定 |
| :--- | :--- |
| ① KAD 真活、出口按应用 / 进程放行 | **仍开放**；0/3 与 0/10 都来自独立进程，若仅 `eMule.exe` 被放行则均为假阴性；须由 `Kademlia` 方法搜索判定 |
| ② firewalled / Buddy | **已否证**（§9） |
| ③ eMule 走另一条出口 / 代理 | **仍开放** |
| ④ 出站统一过滤 + 入站 DNAT 产生「打开」 | 与全部观测一致，§10.3 补上机制解释；仍非严格证明 |
| ⑤ 新增：共享的死联系人 | 本机侧由 §10.2 的 0/10 新鲜宽扫**大幅削弱**；该机侧仍待宽扫 |

**工程结论（与科学归因解耦）**：截至目前，本网**没有任何独立客户端进程**被证明能完成
一次公网 KAD UDP 往返 —— 本机 0/3（三种源端口）+ 0/10（新鲜目标），该机 0/3。
无论 ①③④ 哪个为真，ColdRevive 自己的客户端在本网都跑不了 P4：差别只在于「换网」
还是「为本项目的进程单独申请一条出站例外」。因此 **P4 应换到高位 UDP 未被过滤的网络
重跑，不再等待归因收敛**；本节全部观测都是环境诊断，`evidence_valid_for_pass=false`，
不构成 S2 结论。

### 10.5 剩余核实（可选，用于收窄归因，不阻塞换网）

1. 该机：搜索 `Method` 明确选 `Kademlia`，搜热门关键词 → 有结果 = ① / ③；无结果 = ④。
2. 该机：把 `bootstrap.fresh.local.json` 拷过去，**尽快**跑 §8.6.3 的宽扫命令 →
   `0` = 该机能跑 P4（充分证据）；`2` = 须与第 1 项合看。
3. 若第 1 项有结果而第 2 项为 `2` → 按应用 / 进程放行，查该机安全软件的出站规则；
   本项目进程需显式放行，或仍换网。
4. 换网后（唯一有效路径）：诊断退出码 `0` → 刷新 `nodes.dat` → 按 G0 参数
   （`sample_size=3` / `rate_limit_pps=0.2`）重跑 P4 → 由实验执行人人工回填 `S2.status`。

**状态不变**：`S2.status = not_run`、`PROTOCOL_VERSION = 1`、T0 golden files 未动、
RISK R-02 三桶整体作废、未发布任何测试 Key。

> **v2.4.14 更正**：§10.4 / §10.5 的「本网无独立客户端可跑 P4 / 换网重跑」结论已被 §11
> 否证并撤回 —— 那些 0/N 全部发往字节序颠倒的错误地址。**不需要换网，也不需要开防火墙。**

## 11. 根因归位：联系人 IP 字节序缺陷，高位 UDP 从未被阻断（2026-11-10，v2.4.14）

### 11.1 新证据：eMule 运行时状态

项目所有者提供三张该机 eMule **运行中**的截图（数值转录，公网 IP / 端口不入库）：

| 截图 | 观测 | 含义 |
| :--- | :--- | :--- |
| 传输列表 | 当日 11:12 加入的文件正在以 MB/s 级速度下载；展开的来源里**多行 `Kad`** 与 `eD2K Se...` 同时「传输中」，另有排队 / 正在连接 / 等待 Kad | KAD 来源查找与 eD2k 来源查找都在工作 |
| Kad 页签 | 节点行全绿，`0(8)/0(9)/0(10)`；`当前搜索 (1)`：`搜索关键字` / 名字 `windows` / **数量 93** / 状态 停止 / `已发送包 7|2` / **回应总数 600** | 一次 KAD 关键字搜索收到 600 个节点应答、列出 93 个结果 |
| Kad 页签 | `当前搜索 (2)`：两条 `节点查找`，`已发送包 2|0` 与 `1|0`，`回应总数 2` 与 `1` | 节点查找也收到了应答 |

[eMule 官方帮助 · Kad 窗口](https://www.emule-project.com/home/perl/help.cgi?l=1&topic_id=1266&rm=show_topic)：
「当前搜索」显示的是 Kad **此刻**的活动，节点绿点表示「已验证在线、数小时内稳定」；
[Kad 搜索详情图](https://www.emule-project.com/home/perl/help.cgi?l=1&rm=show_topic&topic_id=1552)：
搜索结果由远端节点以 UDP 应答送回。所以这台机器上 `eMule.exe` 确实在收发公网 KAD UDP 包。

### 11.2 真正的关键环节：把联系人变成 UDP 目的地时的 IP 字节序

KAD 是纯 UDP 的 DHT，`BOOTSTRAP_REQ → BOOTSTRAP_RES → KADEMLIA2_REQ/RES 迭代 →
PUBLISH → SEARCH` 全链条第一步就是「把联系人列表里的 4 字节 IP 变成 `sendto()` 的地址」。
这一步错了，后面全部零响应，且现象与「防火墙丢弃」完全无法区分。

源码链（aMule master）：

| 位置 | 事实 |
| :--- | :--- |
| `src/NetworkFunctions.h` | `KadIPToString(ip)` 用 `ip>>24` 作第一字节 → KAD 内部 IP 是 **host order**；`Uint32toStringIP` 才用「anti-host order」 |
| `src/KadDlg.cpp` | 注释：「Ip is reversed since StringIPtoUint32 returns **anti-host** and **kad expects host order**」 |
| `src/SafeFile.cpp` | `WriteUInt32` / `ReadUInt32` 在 x86 上是**小端** |
| `KademliaUDPListener.cpp::SendPacket` | `wxUINT32_SWAP_ALWAYS(destinationHost)` 再交给 socket 层；`OnPacketReceived` 反向交换 |
| `KademliaUDPListener.cpp::ProcessKademlia2Request` | `hostIP = wxUINT32_SWAP_ALWAYS(contactIP)` 后才做 `IsGoodIPPort` |

结论：host-order 整数 `a<<24|b<<16|c<<8|d` 以小端写出后，线上 4 字节是 **`d.c.b.a`**，
即点分十进制的**反序**。我们的三个解码器都按 `a.b.c.d` 读了。

统计验证（真实 `nodes.dat`，fileVersion 2 / 200 条，两条解码路径对比）：

| 解码 | 全局可路由 | 私网 / 保留 | 首字节分布 |
| :--- | :--- | :--- | :--- |
| 原样 `a.b.c.d` | 181/200 | **19 条 RFC1918**，且出现 225/236/246/253 等组播 / 保留首字节 | 近似均匀随机 |
| 反序 `d.c.b.a` | **200/200** | 0 | 集中在 `79/81/82/84/88/91/93/185` —— 典型宽带 ISP 段 |

同一规律在 03:12 UTC 的 LAN `BOOTSTRAP_RES` 20 条联系人上复现（反序后 20/20 全局可路由）。
这不是「节点列表被投毒」，是**我们自己把地址读反了**。

### 11.3 修复内容

- [`nodes_dat_to_bootstrap.py`](nodes_dat_to_bootstrap.py)：新增 `decode_kad_ip()`，反转 4 字节；`--selftest` 回归。
- [`bootstrap_res_to_targets.py`](bootstrap_res_to_targets.py)：同上；`--selftest` 回归。
- `kad-mini/src/wire.rs`：`Contact::encode/decode` 统一以「点分顺序」为内存表示、线上反序；
  新增 `contact_ip_wire_order_is_reversed` 单元测试（`cargo test` 8/8）。
- 重新生成三份 gitignored 目标清单（public / live / fresh），并把 fresh 放到本机 Desktop。
- 协议侧另经源码核实：`KADEMLIA2_BOOTSTRAP_REQ` 无需载荷 / KAD ID / HELLO / UDP key，
  `Process2BootstrapRequest` 无条件回 20 个联系人 —— 我们 2 字节 `0xE4 0x01` 报文**是合法的**。

### 11.4 决定性复验（本机，G0 参数内）

| 实验 | 结果 |
| :--- | :--- |
| `kad_egress_probe.py` 修正后 10 目标宽扫（3 条 Key 上限内的纯诊断，0.2 pps） | **9/10 收到 `0xE4 0x09` / 523 B `BOOTSTRAP_RES`**，RTT 46–1275 ms；1 条 `WSAECONNRESET`；`RESULT: REACHABLE`、退出码 0 |
| P4 重跑 A（修正后的 `nodes.dat` 3 目标） | `public_kad_contacted=true`；1/3 `BOOTSTRAP_RES`（517 B `0xE5` zlib 压缩帧）、20 联系人；2 次 `0x21/0x29` 迭代；3 hops / 30 联系人；**`tolerance_reached=false`** → 3/3 样本 `skipped`，未发布、未搜索 |
| P4 重跑 B（03:12 新鲜 10 目标） | `public_kad_contacted=true`；2/3 `BOOTSTRAP_RES`；3 hops / 34 与 30 联系人；**`tolerance_reached=false`** → 3/3 `skipped`，未发布、未搜索 |

高位 UDP 一直是通的。`udp_egress_check.py` 里 STUN 3478/19302 的 0/3 是 **STUN 专属过滤**，
不是 KAD 的代理指标；v2.4.11 已指出 STUN ≠ KAD，此处得到实证。

### 11.5 撤回清单

| 版本 | 结论 | 判定 |
| :--- | :--- | :--- |
| 2.4.9 / 2.4.10 | 出口按目标端口白名单过滤高位 UDP；整网无主机可跑 P4 | **撤回** |
| 2.4.11 | STUN ≠ KAD；本机由 KAD 探针「确证不可达」 | 前半**保留**；「确证不可达」**撤回**（目标地址错误） |
| 2.4.12 | 「UDP 打开」是入站测试，不证明能跑 P4 | **保留**（源码复核本身正确） |
| 2.4.13 | 本网无独立客户端可跑 P4，工程决策换网重跑 | **撤回**；eD2k/KAD 判据区分仍有效 |
| RISK R-02 三桶 | 历史超时记为「环境前置条件失败」 | 改判为**测错对象**，整体作废，不进入任何桶 |

`S2.status` 全程 `not_run`；两次 P4 重跑都 `evidence_valid_for_pass=false`；
**未发布任何测试 Key**；`PROTOCOL_VERSION = 1`、T0 golden 未动。

### 11.6 新的关键环节：路由收敛（criteria #2），不是网络

P4 现在能完成 bootstrap 与 `KADEMLIA2_REQ/RES` 迭代，但每样本只向**每跳 2 个**候选、
最多 **3 跳**发请求，累计仅 30–34 个联系人；而 aMule 的
`SEARCHTOLERANCE = 16777216`（2^24）比较的是 XOR 距离的**最高 32 位**
（`Get32BitChunk(0)` 为最高位，已在 `UInt128.h` 核实），随机命中概率约 1/256，
30 个联系人的命中率仅约 11% —— 所以「未收敛」是**我方迭代宽度不足**，不是节点拒绝。

另经 `src/include/protocol/kad/Constants.h` 核实：`KADEMLIA_FIND_VALUE = 0x02`、
`KADEMLIA_STORE = 0x04`、`KADEMLIA_FIND_NODE = 0x0B`（同时是请求返回的联系人数），
`ALPHA_QUERY = 5`；我们当前 `0x21` 首字节用了 `5`（非标准类型值，节点仍回了 5 个联系人）。

下一步修客户端（不涉及网络 / 防火墙）：每跳取 `ALPHA_QUERY` 个最近候选、用
`KADEMLIA_FIND_NODE = 0x0B` 请求更多联系人、把跳数上限提高到能覆盖邻域，
再按 G0 参数重跑 P4。`nodes.dat` 已过期（3 条中 2 条无响应），应统一改用
`bootstrap_res_to_targets.py` 从实时 `BOOTSTRAP_RES` 生成的目标清单。

### 11.7 状态与下一步

1. 修 §11.6 的收敛逻辑 → `cargo test` / `selftest` / `live --check` 全绿。
2. 按 G0 参数（`sample_size=3` / `rate_limit_pps=0.2`）重跑 P4，逐次留原始报文与三桶分类。
3. 由实验执行人依据证据**手工回填** `S2.status`；脚本禁止自动置 `pass`。
4. §7–§10 保留为「错误归因的完整审计轨迹」，但其结论不再作为决策依据。
