# S2 实网实施排期（最小 eMule KAD 客户端）

> 状态：**P0–P4 已完成，`S2.status = pass`（2026-11-10 由项目所有者依证据判定）**；
> ADR-011 的 L1（§22）与 L2（§23）已于 2026-11-11 在 `tests/spikes/s2/` 落地，属**互操作能力的探针证据**，
> 不改写 S2 结论、不回填 `spikes.json`。
> 下方 §0–§21 各轮次里的「当时状态」按轮次原样保留（含已被撤回的归因），不代表当前值；
> 当前值以 `spikes.json` 与本段为准。
> **v2.4.14 重大更正**：§7–§10 的「出口过滤高位 UDP / 换网重跑」结论**整体作废**。根因是我方把 KAD 联系人 4 字节 IP 按点分十进制顺序读取，而线上是**反序**；所有历史 0/N 结果都是把包发给了不存在的地址。修正后本机 10 目标探针 **9/10 收到真实 `BOOTSTRAP_RES`**，高位 UDP 一直是通的。详见 §11。
> **2026-11-10 第七轮（§20）**：修好 Kad tag 帧头后首次跑出公网闭环 —— `PUBLISH_RES`（`load=1`）
> 与 `SEARCH_RES` 命中，5 个 tag 原样返回。判据 1/2/4/5/6 满足，判据 3 仅 n=1、且 A/B 为同一进程，
> 故 `S2.status` 仍未回填，见 §20.6–20.7。
> 本文件是「排期 + 门禁 + 证据归档」，不是结论本身；唯一结论入口是 `spikes.json`
> （当前 `S2.status = pass`，证据见 §20 / §21）。
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
| **P4** 实网小样本执行 | 逐次发布/查询原始记录、成功率、RTT 分布、丢弃/拒绝观测 | 由实验执行人依据证据人工回填 `S2.status` | ✅ **2026-11-10 完成并判 `pass`**：第七轮 `live` 得 `PUBLISH_RES load=1` + `SEARCH_RES` 命中（§20），两进程 A/B 独立取回（§21）。早期 0 响应的「环境过滤高位 UDP」归因已被 v2.4.14 撤回，真因是 IP 字节序 + Kad tag 帧头两处编码缺陷，见 §11 / §19 |

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

## 12. 发布目标筛选：从「最近 10 个」改为「已验证在责任区内的应答者」（2026-11-10，v2.4.19）

### 12.1 为什么改

v2.4.17 的实网记录里，seq0 / seq1 **已经收敛**，但 publish / search 仍 **0/10 全部超时**；
同版本回环 3/3 发布确认 + 3/3 命中，说明 wire 与 aMule 逐字节一致。所以 0/10 不是 codec 问题，
而是「发给了不会接收发布的节点」。两个机制：

| 机制 | 说明 | 我们的应对 |
| :--- | :--- | :--- |
| **KadID 陈旧** | eMule 换 ID 后会继续应答一段时间；aMule 用请求里的 `check` 是否等于自己当前 ID 判定，不等即**静默丢弃**。联系人表里被广告的 ID 因此不可信 | 只把「0x29 成功应答过」的地址当目标 |
| **UDP firewalled 位** | aMule `Process2PublishKeyRequest` 对判定为 firewalled 的 UDP 源会丢弃，不回应 | 无法事前识别，只能多目标 + 逐次记录 |

「取 known 里 XOR 最近的 10 个」必然混入这两类节点，0/10 是设计使然，不是网络不接纳。

### 12.2 改动

- **0x29 应答即 KadID 证明**：`KADEMLIA2_REQ` 的 `check` 字段必须是接收方自己的 KadID，
  aMule 不等则静默丢弃。因此任何成功的 `0x21 → 0x29` 都证明该地址的真实 KadID 就是我们发的那个。
  新增 `verified: HashSet<SocketAddr>`，在应答成功时登记。
- **发布 / 搜索目标** = `verified ∩ 全局可路由 IPv4 ∩ top_chunk(id,key) ≤ SEARCHTOLERANCE`，
  上限 `PUBLISH_FANOUT`。不再用「最近 N 个」兜底。
- **循环条件**改为「直到出现 verified 且落在责任区内的节点，或路由表停止增长」；
  plateau 按 `known` 是否增长判定（2.4.18 已修，此前按 `in_zone` 判会导致 2 跳即停）。
- **报告字段**新增 `verified_contacts`、`zone_verified_contacts`、`publish_target_policy=verified_in_zone`。
- 找不到 verified 在区节点时**跳过该样本**并写明原因，不盲发。

### 12.3 这是收窄目标、不是降低判据

S2.criteria #1 判的是「命名空间被公共 KAD 接纳」。把目标限定为「真实 KadID 在责任区内、且 UDP 应答可达」
的节点，恰好是 aMule 接受发布的必要条件（`Process2PublishKeyRequest` 的两个 gate）。
盲发到不满足 gate 的节点只会制造 0 响应，属于 RISK R-02 的第一桶（协议层噪声），不构成接纳性结论。

### 12.4 回归

- `cargo test` **13/13**、`selftest` **37/37**、`loopback` **3/3 发布确认 + 3/3 命中 + 0 协议错误**。
- `live --check`：唯一 blocker 仍是 `--attest-i-am-the-experiment-operator`；
  `sample_size=3` / `rate_limit_pps=0.2` 已与 G0 确认值一致，bootstrap 10 节点公网 IPv4 校验通过。
- 本轮**未发送任何报文、未接触公共 KAD**。

### 12.5 预期与残余风险

- 若首轮能找到 verified 在区节点，发布成功率应显著高于 v2.4.17 的 0/10；
  若找不到，会记录 `zone_verified_contacts = 0` 并跳过 —— 这是「没找到可接收者」，
  与「命名空间被拒」必须分开记账。
- 仍存盲点：firewalled-UDP 节点可以应答 0x21 却不接收发布，事前无法识别；
  只能靠多目标与逐次原始记录体现。
- 结论仍由实验执行人手工回填，程序不自动置 `pass`。

## 13. P4 两轮实网执行（2026-11-10，v2.4.19 / v2.4.20）

### 13.1 配置与结果

两轮均为 G0 参数：`--sample-size 3 --rate-limit 0.2`，明文 `0xE4`，跳过 HELLO，
bootstrap 由执行人提供（10 个公网节点）。原始记录分别在
`kad-mini/live-run-v2419.json`（63 条）与 `live-run-v2420.json`（70 条）。

| 轮次 | 版本改动 | seq | known | verified | zone_verified | hops | PUBLISH |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| A | 目标筛选 `verified_in_zone` | 0 / 1 / 2 | 103 / 88 / 114 | 8 / 8 / 9 | 0 / **1** / **1** | 3 / 3 / 2 | 0 确认，2 次 `10060` 超时 |
| B | 深度 16 跳 + `MIN_ZONE_TARGETS=3` + 只查未访问候选 | 0 / 1 / 2 | 113 / 86 / 127 | 11 / 7 / 12 | 0 / **1** / 0 | 5 / 3 / 4 | 0 确认，1 次 `10060` 超时 |

三轮 `SEARCH_KEY_REQ` 与 PUBLISH 同结果：全部超时，0 命中。

### 13.2 两轮共同观测

1. **目标筛选生效**：不再盲发最近 10 个，PUBLISH 只发往 `verified ∩ 在责任区内` 的地址；
   `known` 从 v2.4.14 的 30–34 提升到 86–127，`verified` 7–12，说明漏斗确实在走。
2. **在区命中数仍是 0–1**，与「随机联系人」的期望一致（127/256 ≈ 0.5）。
   即漏斗还没真正进入 KAD 的近距离区域，只是把样本量堆大了。
3. **同一个在区节点两轮都是「答 0x21、不答 0x43 与 0x33」**：`82.63.191.18:4672`
   （A 轮 seq1 与 B 轮 seq1 相同 Key 命中同一节点，属确定性结果）。
   它不是死节点（否则 0x21 也不会回），而是**策略性不应答**索引操作。
4. 因此当前 0/N **既不是「命名空间被拒」（索引层），也不是报文问题**：
   属于 criteria 未覆盖的第三类 —— **目标侧服务策略**（firewalled-UDP 位、
   不服务陌生对等体、或仅对已互认节点开放索引操作）。

### 13.3 结论与下一步

`S2.status` 保持 `not_run`。这两轮能证明的只有：wire 正确（回环 3/3 + 3/3）、
地址与 KadID 正确（`BOOTSTRAP_RES` 与 `0x29` 应答）、目标筛选生效。
**尚不能证明命名空间是否被公共 KAD 接纳**，因为没有任何一次 PUBLISH 得到确认。

下一步两条，建议并行做，都不涉及扩大样本：

| # | 动作 | 判读 |
| :--- | :--- | :--- |
| D1 | 报告增加 `closest_top_chunk` / `closest_verified_top_chunk`（XOR 距离的位长） | 若 16 跳后仍在 2^30 量级，说明 `0x29` 返回的联系人未被按距离重排利用，需对响应联系人重排序并只沿最靠近的分支走 |
| D2 | 对 3 个 `verified` **非在区**节点发 `SEARCH_KEY_REQ` | 若全部无应答，则「陌生节点不服务索引操作」成立，S2 必须先补 HELLO / 对等体互认，再谈发布 |

### 13.4 记账

- 未获得任何 `PUBLISH_RES`；未命中任何 `SEARCH_RES`；`S2.status = not_run`。
- 两轮均为执行人显式 `--attest-i-am-the-experiment-operator`，`PROTOCOL_VERSION = 1`，T0 golden 未动。
- `spikes.json` 的结论字段**未回填**（按规则只由人填写），待 D1/D2 出结果后一并登记。

## 14. P4 第三轮：修验证覆盖 + 索引操作策略探针（2026-11-10，v2.4.21）

### 14.1 为什么要先修覆盖，而不是加大样本

「扩大样本能否提高成功率」这个问题可以先用前两轮的数据算清楚，不必再花钱试。

| 量 | 观测值 | 含义 |
| :--- | :--- | :--- |
| 每样本 `verified` | 7–12（均值 9.2） | 只有「回过 `0x29`」的节点能当发布目标 |
| 每样本 `known` | 86–127 | 路由表里 90% 的联系人从未被验证，因而不可用 |
| 在区率（`zone_verified / verified`） | 3/55 = 5.5% | 随机期望 1/256 = 0.39%，**漏斗有约 14 倍富集，方向对但幅度不够** |
| `P(样本含 ≥1 在区目标)` | 3/6 = 0.50 | 一半样本根本没有可发布对象 |
| 每样本发布次数 | 0.5 | —— |

于是：`sample_size=3` → 期望 1.5 次发布；`=5` → 2.5 次；`=10` → 5 次。
**加大样本只是线性增加「试错次数」，而每次试错有一半概率根本没发出包。**
反过来，把 `verified` 从 9 提到 ~30（同样的 16 跳预算，只是不再提前 break），
在区目标期望就从 0.5 升到 ~1.7，等效于把样本放大 3 倍，而公网报文量不变。

另外，`--sample-size` 受门禁约束：`live_gate` 要求 CLI 值等于
`spikes.json → S2.public_kad_approval.confirmed.sample_size`（=3），
超出的写法会被直接拦下（实测 `--sample-size=10` → `与 G0 确认值 3 不一致`）。
改这个值属于**所有者确认参数的变更**，不是代码改动。

### 14.2 改动（v2.4.21）

| 改动 | 理由 |
| :--- | :--- |
| **plateau 不再作为退出条件** | 「路由表停止增长」≠「没有可问的节点」。此前 2 跳零增长就 break，16 跳预算只用了 3–5 跳，110–130 个联系人里只验证了 ~10 个 |
| **D1 距离诊断**：新增 `closest_top_chunk` / `closest_verified_top_chunk` / `queried_contacts` / `stale_hops` | 区分「路由没走到」（known 里没有近的）与「验证没覆盖」（有近的但没确认） |
| **D2 索引操作探针**：`index_op_probe_*`，对 ≤3 个「已验证据、但不在区」的节点各发一个 `SEARCH_KEY_REQ` | 这些节点几十秒前刚回过 `0x21`，是活节点。它们对 `0x33` 的态度直接区分「索引操作需要特定目标」与「索引操作需要特定对等体关系」 |
| **应答必须来自被问的地址** | 原实现接受任意来源的报文：迟到的 `0x29` 会被解成「expected 0x43, got 0x29」，迟到的 `0x43` 会被记到错误的目标头上。改为最多轮询 4 次直到来源匹配 |

### 14.3 结果（G0 参数：sample_size=3，0.2 pps，明文 0xE4，跳过 HELLO）

| seq | hops | known | verified | queried | zone_verified | closest_top_chunk | PUBLISH | SEARCH | 探针 |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| 0 | 7 | 165 | 29 | 33 | 1 | 9,826,055（在区） | 1 发 0 回 | 0 回 | 3 发 0 回 |
| 1 | 7 | 138 | 27 | 34 | 1 | 13,899,520（在区） | 1 发 0 回 | 0 回 | 3 发 0 回 |
| 2 | 7 | 162 | 30 | 32 | 0 | 31,927,299（越界） | 跳过 | — | 3 发 0 回 |

原始记录 204 条：`0x21`×99、`0x29`×86、`0x33`×11、`0x43`×2、`0x01`×3、`0x09`×3。

**覆盖修复有效**：单样本 `verified` 从 8–12 升到 27–30（约 3 倍），
`known` 从 86–127 升到 138–165，且 `closest_top_chunk` 首次报出与在区门槛
（`SEARCHTOLERANCE = 16,777,216`）同量级的距离，说明漏斗确实走到了 2^24 附近。

**但结果仍然是 0。**

### 14.4 这一轮真正的结论：失败与「目标是谁」无关

> **⚠️ 本节判读已在 [§15](#15-h2a-mule-源码逐字段比对2026-11-10v2422) 更正。**
> 源码核实表明：aMule 对「索引里没有该 Key」的搜索**按设计就不回应**，
> 因此下面 0/19 的静默 **不构成**「请求类别级门禁」的证据。本节保留原始观测与当时的推理，
> 结论以 §15 为准。


把三轮的应答率放在一起：

| 操作类别 | 应答 | 静默 | 应答率 |
| :--- | :--- | :--- | :--- |
| `KADEMLIA2_REQ (0x21)` → `0x29` | 86 | 13 | **87%** |
| 索引操作（`0x33` / `0x43`） | 0 | 19 | **0%** |

Fisher 精确检验双侧 **p = 3.7e-14**。同一批活节点、同一 UDP 源端口、同一加密信封，
回答节点查找、拒绝索引操作——差异不在网络、不在目标、也不在报文可解密性。

9 个探针目标尤其关键：它们是**逐个**验证据实（刚回过 `0x29`）、分散在不同 IP / AS、
且全部**不在**责任区内。如果 0 回应的原因是「找错了节点」，这 9 个里至少应有若干应答；
全部静默说明这是**请求类别级的门禁**，与目标选择无关。

因此：**继续加大样本只会重复同一个 0/N。** 样本量在这里不是自变量。

### 14.5 两个仍然可能的解释（下一轮区分）

| # | 假设 | 检验方式 | 预期 |
| :--- | :--- | :--- | :--- |
| H1 | 索引操作要求对等体**先互认**（`HELLO` 之后才在对方路由表里成为一个 contact），而 `0x21` 是无状态处理的，所以我们「能问路、不能存取」 | 对同一批探针目标先 `HELLO_REQ (0x11)` 再 `SEARCH_KEY_REQ` | 若 H1 成立，HELLO 后 `0x33` 开始有应答 |
| H2 | 我们的 `0x33` / `0x43` 载荷与 aMule 解析器不完全一致，解析失败即静默丢弃（回环 3/3 无法证伪：两侧都是本实现，自洽的编码错误照样通过） | 与 aMule `master` 的 `CSearchKeyReq` / `CPublishKeyReq` 逐字节比对，或抓一个真实 aMule 客户端的同类请求对照 | 若 H2 成立，HELLO 也无济于事 |

两条都需要动 `HELLO` 路径。而当前 G0 确认的 `protocol_scope` 明确写着
「保持明文 `0xE4`，**跳过 HELLO 路径**」，所以 H1 的检验要先由项目所有者
**书面放宽该 scope**，本轮不做。

### 14.6 记账

- `S2.status` 保持 `not_run`；无 `PUBLISH_RES`、无 `SEARCH_RES`、0 命中。
- 本轮为执行人显式 `--attest-i-am-the-experiment-operator`，`PROTOCOL_VERSION = 1`，T0 golden 未动。
- `spikes.json` 结论字段未回填（只由人填写）。
- 报文总量 204 条，全程 0.2 pps；无批量发布（2 次 PUBLISH、11 次 SEARCH、9 次探针）。

## 15. H2：aMule 源码逐字段比对（2026-11-10，v2.4.22）

按 H2 假设（「我们的 `0x33`/`0x43` 载荷与 aMule 解析器不一致」）做的**纯离线**核对，未发送任何报文。
依据 aMule `master`（PROTO §2.0 已固定 commit `6a11056`），主要文件：
`src/kademlia/kademlia/Search.cpp`（`CSearch::StorePacket`）、
`src/kademlia/net/KademliaUDPListener.cpp`（`Process2SearchKeyRequest` / `Process2PublishKeyRequest` / `ProcessPacket`）、
`src/kademlia/kademlia/Defines.h`、`src/kademlia/kademlia/Indexed.cpp`（`SendValidKeywordResult`）。

### 15.1 逐字段比对

| 项 | aMule 侧 | 本实现 | 结论 |
| :--- | :--- | :--- | :--- |
| `SEARCH_KEY_REQ` 载荷 | `WriteUInt128(target)` + `WriteUInt16(0)`（非 restrictive 时） | `key(16)` + `u16(0)` | **一致**（且值 0 使字节序问题不成立） |
| `SEARCH_KEY_REQ` 目标计数 | `startPosition & 0x7FFF`，`0x8000` 为限制性表达式标志 | `u16(0)` | **一致** |
| `PUBLISH_KEY_REQ` 载荷 | `file(16)` → `count(u16)` → 每项 `sourceID(16)` + `tagCount(u8)` + tags | 同序、同宽度 | **一致** |
| tag 计数宽度 | `uint32_t tags = bio.ReadUInt8();` | `b.u8(tags.len())` | **一致** |
| `SEARCHTOLERANCE` | `#define SEARCHTOLERANCE 16777216` | `16_777_216` | **一致**（此前怀疑的常量错误不成立） |
| `K` / `ALPHA_QUERY` | `10` / `5` | `10` / `5` | **一致** |
| 距离比较 | `distance.Get32BitChunk(0) > SEARCHTOLERANCE`（chunk 0 = 最高 32 位） | `top_chunk() = xor >> 96` | **一致** |
| `PUBLISH_RES` 响应体 | `file(16)` + `load(u8)` | 同 | **一致** |

H2 的载荷层假设**不成立**：编码没有偏差。

### 15.2 顺带证伪 H1（不需要动 `protocol_scope`）

两个处理函数都**不查询路由表**，不检查发送方是否是已知 contact：

```cpp
void CKademliaUDPListener::Process2SearchKeyRequest(...)
{
    CMemFile bio(packetData, lenPacket);
    CUInt128 target = bio.ReadUInt128();
    uint16_t startPosition = bio.ReadUInt16();
    ...
    CKademlia::GetIndexed()->SendValidKeywordResult(target, pSearchTerms, ip, port, false, startPosition, senderKey);
}
```

`Process2PublishKeyRequest` 的全部前置条件只有两条：

```cpp
	if (CUDPFirewallTester::IsFirewalledUDP(true)) {
		//We are firewalled. We should not index this entry and give publisher a false report.
		return;
	}
	...
	if (distance.Get32BitChunk(0) > SEARCHTOLERANCE && !::IsLanIP(wxUINT32_SWAP_ALWAYS(ip))) {
		return;
	}
```

**所以「必须先 HELLO 互认」不成立**，`HELLO` 路径无须实现，G0 的 `protocol_scope` 无须放宽。

### 15.3 但这两条前置条件都是「静默返回」

| 静默分支 | 触发者 | 我们能否事前识别 |
| :--- | :--- | :--- |
| `IsFirewalledUDP(true)` → 直接 return | **对端自己**的 UDP 是否可被动接入 | 不能。firewalled 节点照样能答 `0x21`（无状态），只是拒绝索引 |
| `distance > SEARCHTOLERANCE` → return | 对端 KadID 与 Key 的距离 | 能（就是 `top_chunk` 过滤） |
| 解析抛异常 → 无回应 | 报文解析失败 | —— 编码已比对一致，风险显著下降 |

也就是说：**发布没有回应是「这个对端是 firewalled 节点」的正常表现，不是拒绝、不是 bug。**
公网中 UDP-firewalled 的 eD2K 客户端占比很高，`0/2` 两次发布全撞上静默节点完全可以发生。

### 15.4 最重的一条更正：搜索静默是设计要求，不是门禁证据

```cpp
void CIndexed::SendValidKeywordResult(const CUInt128& keyID, ...)
{
    KeyHashMap::iterator itKeyHash = m_Keyword_map.find(keyID);
    if (itKeyHash != m_Keyword_map.end())     // <- 整个响应构造都在这个 if 里
    { ...   SendPacket(packetdata, KADEMLIA2_SEARCH_RES, ...); ... }
    Clean();                                   // miss 时只走这一行
}
```

**索引里没有该 Key → 一个字节都不回（连「0 结果」的空包都没有）。**
我们搜的是自己的私有命名空间 Key，全网没有任何节点持有它，
所以 `0x33` 的 0/11、以及 9 个探针的 0/9 **是必然结果**，与对端策略、与我们是否正确毫无关系。

因此：

- §14.4 的「请求类别级门禁」结论**撤销**。
- `p` 值 3.7e-14 虽然数值上成立，但它比较的两类操作里，有一类（search）的信道本身就是关闭的，
  这个检验**不成立**——比较的不是同一件事。
- v2.4.21 的 `index_op_probe` 已从代码中删除：它测不出任何东西。

### 15.5 修正后的结论与下一步

`0/N` 的成因重新归因为三件事，按贡献排序：

| # | 成因 | 状态 |
| :--- | :--- | :--- |
| 1 | 在区目标太少（0–1 个/样本），一次 firewalled 就归零 | 已定位，v2.4.22 修 |
| 2 | 对端本身 UDP-firewalled，静默拒收 | 无法事前识别，只能靠多目标冗余 |
| 3 | tag 解析偏差 | 编码已比对，但 `ReadTag` 尚未逐字节核对（唯一残余风险） |

**v2.4.22 的代码改动**：候选窗口从 `ALPHA_QUERY * 6`（=30）改为**整张表**。

> **⚠️ 这条判断已被 [§16](#16-p4-第四轮与逐跳复现2026-11-10v2423) 推翻。** 去掉窗口上限之后，
> 把最近几个联系人问完，下一跳只能跳到 700M–1400M 之外的节点，爬行走成了震荡而非下降。
> 「窗口 30 导致结构性截断」不成立；真正缺的是**前沿护栏**（只问已到达距离带以内的节点）。

此前 v2.4.21 在第 7 跳退出、`queried=33`，而 `known` 里还有 138–165 个从未问过的联系人——
`known` 排 31 位之后的全被窗口挡住，`closest_top_chunk` 因此卡在 ~2^23（正好差一跳跨过 2^24）。
改后爬行可以继续往深处走，目标是单样本拿到 ≥3 个在区目标，从而发出多次发布。

`S2.status` 仍为 `not_run`；本轮**未发送任何报文**。

## 16. P4 第四轮与逐跳复现（2026-11-10，v2.4.23）

### 16.1 执行结果

G0 参数不变（`--sample-size 3 --rate-limit 0.2`，明文 `0xE4`，跳过 HELLO），执行人显式
`--attest-i-am-the-experiment-operator`。原始记录 448 条（`kad-mini/live-run-v2422.json`）。

| seq | hops | known | verified | queried | zone_verified | closest_top_chunk | PUBLISH |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| 0 | 16 | 231 | 62 | 80 | 0 | 38,291,981 | 跳过（无在区目标） |
| 1 | 16 | 198 | 70 | 80 | 1 | 13,899,520 | 1 发 0 回 |
| 2 | 16 | 220 | 68 | 80 | 0 | 53,186,560 | 跳过 |

报文：`0x21`×240、`0x29`×200、`0x01`×3、`0x09`×3、`0x33`×1、`0x43`×1。

覆盖修复是有效的（`verified` 27–30 → 62–70），16 跳全部用满且 `stale_hops = 0`（路由表到
最后一跳仍在增长），**但在区目标仍是 0/1/0，最近距离反而比上一轮更差**（38M / 53M vs
9.8M / 31.9M）。这个组合本身不自洽：`known` 翻倍、跳数用满、表在增长，距离却不降。

### 16.2 用本轮原始报文做的四项离线复现（未发包）

**(1) `0x29` 载荷结构核实。** 解出来是 `target(16) + count(u8) + count×25`，`count = 0x0B = 11`
——与 aMule `ProcessKademlia2Request` 的写包顺序逐字段一致。此前一度怀疑的「17 字节相同前缀」
是本人偏移读错（实为 target 16 字节 + count 1 字节），**报文与解析都没有问题**。

**(2) `check` 语义复核，`verified` 成立。** 源码 `if (CKademlia::GetPrefs()->GetKadID() == check)`
包住整个响应构造，`check` 不等则连回应都没有。200 次应答即 200 次真实 KadID 证明。

**(3) 距离度量未落在错误分块上。** 按 16 字节的四个 32 位分块分别统计在区率：

| 分块 | 在区率 |
| :--- | :--- |
| **chunk0（最高 32 位，本实现所用）** | **1.14%（25/2198）** |
| chunk1 | 0.27% |
| chunk2 | 0.36% |
| chunk3 | 0.14% |
| 随机基线 | 0.39% |

只有 chunk0 出现富集（**2.9 倍**），端序与分块选择正确。但 2.9 倍意味着**16 跳只下降了约
1.5 比特**——真正的 Kademlia lookup 应当下降 20 位以上。

**(4) 逐跳复现：爬行在震荡。** 把 240 次 `0x21` 按每 5 个一组还原成跳，读出每跳实际询问的
距离（`top_chunk(对端ID, key)`）：

| 跳 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | … | 16 |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| key `ccf0cb9c` 询问距离（M） | 324 | **43** | 54 | 272 | 743 | **38** | 266 | … | 1342 |
| key `fac258d1` 询问距离（M） | 178 | **20** | **14** | 278 | 481 | 609 | 640 | … | 1234 |

**距离不降反升，且大幅漂移。** 同时应答质量是正常的：198 次精确配对中，76% 的应答返回了
比它自己更近的联系人，30% 近了 8 倍以上；返回 11 个联系人对 key 的中位距离 2.3B，恰为均匀
分布中位数。即**对端按 target 返回了最近的 10 个左右联系人，是我们的选点把它浪费了**。

### 16.3 根因

v2.4.22 把候选窗口从 `ALPHA_QUERY * 6` 改为整张表后，选点逻辑变成「取全表最近的未问节点」，
**没有距离上限**。最近的那几个联系人 2–3 跳就问完了，之后每一跳只能去够 700M–1400M 的节点；
这些远端节点返回的最近邻居相对 key 又接近均匀分布，于是下一跳被带回中距离，形成
`43M → 54M → 272M → 743M → 38M → 266M → 1342M` 的震荡。

这违反 Kademlia 的基本收敛条件：**lookup 只在已到达的距离带以内继续提问，带宽不自行放宽**。
缺了这条护栏，报文越多、`known` 越大，震荡幅度越大——这解释了为什么 v2.4.22 的联系人数量
翻倍而距离变差。

### 16.4 修复（v2.4.23）

- 新增 `FRONTIER_FACTOR = 8`：只用「`top_chunk(联系人, key) ≤ 8 × 当前最近距离`」的未问节点作候选，
  带宽下限取 `SEARCHTOLERANCE`（保证不会因为护栏把最后一次跨过 2^24 的机会挡掉）。
- 前沿耗尽时**放宽一档到全表**，并计入 `frontier_widened_hops`——使「始终在带内推进」与
  「被迫盲探」可以事后区分。
- 报告新增 `frontier_widened_hops`。报文预算不变（`ALPHA_QUERY = 5`、`MAX_FIND_HOPS = 16`），
  即本轮与 v2.4.22 **只差选点这一个变量**，可直接对比。

回归：`cargo test` 13/13、`selftest` 37/37、`loopback` 3/3 发布确认 + 3/3 命中 + 0 协议错误。

### 16.5 判据与残余风险

- 若护栏生效，`frontier_widened_hops` 应显著小于 16，`closest_top_chunk` 应随跳数单调下降，
  在区目标应从 0–1 提升到 3 个以上；届时发布样本才有统计意义（当前 4 轮累计仅 **2 个不同
  在区主机**，2 次发布全静默，无法区分「firewalled 占多数」与「发布另有门禁」）。
- 若护栏生效但在区目标仍为 0–1，则说明 2.9 倍富集是网络表稀疏的上限，需改为**加宽前沿**
  （`ALPHA_QUERY`↑）而非加深跳数。
- `S2.status` 仍 `not_run`；本轮无 `PUBLISH_RES`，`spikes.json` 结论字段未回填。

## 17. P4 第五轮：前沿护栏上线，在区目标翻倍，发布仍 0 回应（2026-11-10，v2.4.23）

### 17.1 执行结果

G0 参数不变（`--sample-size 3 --rate-limit 0.2`，明文 `0xE4`，跳过 HELLO，执行人显式 attest）。
耗时 **17 分 53 秒**，原始记录 **395 条**（`kad-mini/live-run-v2423.json`）。

| seq | hops | widened | known | verified | queried | zone_verified | closest_top_chunk | PUBLISH |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| 0 | 16 | 9 | 202 | 49 | 58 | 0 | 26,557,193 | 跳过 |
| 1 | 16 | 13 | 177 | 65 | 75 | **2** | 13,899,520 | 2 发 0 回 |
| 2 | 16 | 11 | 214 | 62 | 71 | **2** | 15,672,328 | 2 发 0 回 |

报文：`0x21`×204、`0x29`×177、`0x43`×4、`0x33`×4、`0x01`×3、`0x09`×3。

**这是四轮以来第一次在同一轮拿到 4 个不同的在区目标**，并真的发出了 4 次 PUBLISH：

| 目标 | hi32 距离 | lo32 距离 | 结果 |
| :--- | :--- | :--- | :--- |
| `119.112.72.25:39763` | 14,808,839 ✅ | 764,524,882 | `10060` 超时 |
| `82.63.191.18:4672` | 13,899,520 ✅ | 3,847,238,183 | `10060` 超时 |
| `93.38.125.19:37642` | 15,672,328 ✅ | 3,881,207,678 | `10060` 超时 |
| `85.51.5.149:4673` | 16,538,118 ✅ | 3,518,096,737 | `10060` 超时 |

四次 `SEARCH_KEY_REQ` 同样全部超时；错误码一律 `10060`（UDP 读超时），**无 ICMP 端口不可达**。

### 17.2 前沿护栏的效果与局限

护栏确实改变了行为：v2.4.22 的询问距离在 43M↔1342M 之间震荡，本轮 `closest_top_chunk`
稳定在 **13.9M–26.6M**，即贴着 `SEARCHTOLERANCE = 16,777,216` 的边界。seq1/seq2 已进入区内。

但 `frontier_widened_hops` = **9 / 13 / 11**（占 16 跳的 56–81%）：绝大多数跳里，
「≤ 8 × 当前最近距离」的带内已经没有未询问节点，只能放宽到全表盲探。这说明
**不是护栏太紧，而是已知路由表在 2^24 附近本身稀疏**——继续加深跳数的边际收益会快速衰减。

### 17.3 逐字节复核发布的 0x43 载荷（离线，未发包）

对本轮实际发出的 237 字节 `0x43` 帧按 aMule `CTag(const CFileDataIO&, bool)` 的规则完整解码：

| # | 类型 | name | value |
| :--- | :--- | :--- | :--- |
| 0 | `STRING (0x02)` | `0x01` TAG_FILENAME | `ColdRevive_V1:coldrevive:t0:wakeup_req.v1` |
| 1 | `UINT32 (0x03)` | `0x02` TAG_FILESIZE | `1024` |
| 2 | `UINT16 (0x08)` | `0xFC` TAG_SOURCEUPORT | `54582` |
| 3 | `STRING (0x02)` | `ColdRevive_V1:descriptor` | `v=1;node=live-probe;wake_port=54582;proto_flags=bt-only;infohash_prefix=none` |
| 4 | `STR14 (0x1E)` | `ColdRevive_V1:namespace` | `ColdRevive_V1:` |

**235 字节载荷被精确消耗，零残余、零非法类型。** 帧结构 `file(16) + count(u16) + [sourceID(16)
+ tagCount(u8) + tags]` 与 `CSearch::StorePacket` 一致；tag 计数是 u8，与 `bio.ReadUInt8()` 一致；
`type 在前 / 0x80 表示 name 为 1 字节 ID` 的编码顺序与 `CTag` 构造器一致。

### 17.4 三条源码级排除项

**(1) 距离判据的 chunk 语义正确。** `UInt128.h` 原文：

```cpp
/* Chunk 0 being the most significant */
uint32_t Get32BitChunk(unsigned val) const noexcept { return val < 4 ? m_data.u32_data[3 - val] : 0; }
```

`Get32BitChunk(0)` 取最高 32 位，与本地 `top_chunk = xor >> 96` 完全一致。此前一度怀疑
「aMule 按低 32 位判区」，本轮 177 个验证节点的数据也排除了它：4 个在区目标的高 32 位全部
≤ 2^24，而低 32 位全部在 7.6e8–3.9e9，若 aMule 用低 32 位则应有 0/177 命中（实测正是 0/177，
与该假设自洽，但两个假设都能解释该数据集，最终以源码为准）。

**(2) `IsFirewalledUDP(true)` 默认 false。** `UDPFirewallTester.cpp` 原文：

```cpp
bool CUDPFirewallTester::m_firewalledUDP = false;
bool CUDPFirewallTester::m_firewalledLastStateUDP = false;
```

只有当节点**自己跑过 UDP 防火墙测试并失败**（`m_fwChecksFinishedUDP >= UDP_FIREWALLTEST_CLIENTSTOASK`）
或超时 6 分钟后，`m_firewalledUDP` 才置 true。**未做过测试的节点不会拒绝索引操作。**
因此「公网节点大多 firewalled 所以拒收」这个解释的强度被显著下调：一个从未测试的
NAT 后节点会照常处理并回应 PUBLISH。

**(3) 发布响应是无条件的。** 完整函数体里只有两条 early return（firewalled、超距），
此后即使 `AddKeyword` 返回 false（节点索引已满）**仍然发送** `PUBLISH_RES`：

```cpp
CMemFile packetdata(17);
packetdata.WriteUInt128(file);
packetdata.WriteUInt8(load);
SendPacket(packetdata, KADEMLIA2_PUBLISH_RES, ip, port, senderKey, NULL);
```

`PUBLISH_RES` 载荷 `file(16) + load(u8)` 与本实现的解析一致。第三条静默路径是 tag 解析抛异常
（`catch(...) { delete entry; throw; }`）——已由 §17.3 逐字节排除。

### 17.5 因此 0/4 说明了什么、没说明什么

**能证明的**：报文格式、tag 编码、距离判据、`check` 语义、目标选择、UDP 通路（`0x29` 双向可达）
全部正确；4 个真实在区目标在数十秒前刚回过 `0x29`（即其 UDP 端口可接收我们的首包），
之后对 `0x43` 一律静默。

**不能证明的**：公共 KAD 是否接纳该命名空间。4 个目标全部无应答，既有可能是对端
「测试过且判定 firewalled」的节点自身状态，也有可能是别的原因，**样本量不足以区分**。

**一个统计上的注意事项**：4 次发布虽为 4 个不同主机，但只覆盖 2 个 key（seq1 的 `fac258d1`
两个目标、seq2 的 `5f149f36` 两个目标）。key 与目标节点的组合才是独立试验单位，
本轮实际只有 4 个组合，且集中在 2 个 key 上。

### 17.6 下一步的三个选项

| # | 动作 | 代价 | 能得到什么 |
| :--- | :--- | :--- | :--- |
| A | `MAX_FIND_HOPS` 16 → 32（代码常量，不动 G0 参数） | 每样本约 2 倍报文，单轮约 60–70 分钟 | 在区目标可能到 6–10 个；但仍受「带内稀疏」限制，`frontier_widened_hops` 占比会更高 |
| B | `ALPHA_QUERY` 5 → 8，同时 `MAX_FIND_HOPS` → 24 | 每样本约 2.2 倍报文 | 加宽而非加深，对应 §16.5 预留的判读分支 |
| C | 把 `sample_size` 3 → 10（**需所有者改 `spikes.json` 的 confirmed 值**） | 报文 3.3 倍，单轮 2–3 小时 | 线性增加在区目标与发布次数，样本量最足，但触及 G0 确认参数 |

**建议先做 A**：它是纯代码改动（与 v2.4.21 的 10→16 同类），不动任何 G0 确认值；
若 32 跳能把在区目标推到 8 个以上，就能直接回答「4/4 静默是巧合还是结构」。

### 17.7 记账

- `S2.status` 保持 `not_run`；无 `PUBLISH_RES`、无 `SEARCH_RES`、0 命中。
- 本轮门禁 `passed = true`（执行人 attest 已提供），`public_kad_contacted = true`。
- `PROTOCOL_VERSION = 1`，T0 golden 未动，`spikes.json` 结论字段未回填（只由人填写）。
- 全程 0.2 pps，204 个发送报文；无批量发布。

## 18. P4 第六轮：跳数翻倍（选项 A）没换来在区目标，只换来翻倍的盲探（2026-11-10，v2.4.24 / v2.4.25）

### 18.1 改动与执行

唯一改动：`MAX_FIND_HOPS` 16 → 32（`src/main.rs` 常量，与 v2.4.21 的 10→16 同类）。
G0 参数一字未改（`sample_size=3` / `0.2 pps` / 明文 `0xE4` / 跳过 HELLO / `verified_in_zone` 目标策略），
执行人显式 attest，门禁 `passed = true`。耗时 **31 分 35 秒**，原始记录 **698 条**
（`kad-mini/live-run-v2424.json`），是 §17 的 1.77 倍。

| seq | hops | widened | known | verified | queried | zone_verified | closest_top_chunk | PUBLISH |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| 0 | 32 | 19 | 230 | 101 | 123 | 0 | 41,292,802 | 跳过（未收敛） |
| 1 | 32 | 25 | 245 | 123 | 140 | **1** | 13,899,520 | 1 发 0 回 |
| 2 | 32 | 18 | 241 | 91 | 112 | 0 | 31,927,299 | 跳过（未收敛） |

报文：`0x21` 发 375、`0x29` 收 315（**应答率 84%**）、`0x43`×1、`0x33`×1、`0x01`/`0x09` 各 3。

### 18.2 选项 A 的预期被直接证伪

选 A 的假设是「32 跳能把在区目标推到 6–10 个」。实测 **0 / 1 / 0**，全轮只有 1 次发布，
比 §17（16 跳，4 个在区目标）更差。更关键的是同 key 对照：

| key | v2.4.23（16 跳） | v2.4.24（32 跳） |
| :--- | :--- | :--- |
| `fac258d1…`（`wakeup_req.v1`） | closest **13,899,520**，在区 2，目标 `82.63.191.18:4672` | closest **13,899,520**，在区 1，目标 `82.63.191.18:4672` |

**多给的 16 跳没有把任何节点拉近 1 个 bit，也没有换出第二个目标。** `frontier_widened_hops`
从 9/13/11（占 16 跳的 56–81%）变成 19/25/18（占 32 跳的 59–78%）：绝对数翻倍、比例不降，
说明新增的跳数**全部**花在带外盲探上，没有一跳落在收窄区间里。

### 18.3 为什么饱和：在区密度就是随机抽样的密度

距离判据是 `top32(KadID XOR key) ≤ 2^24`，即随机节点落在区内的概率为
`2^24 / 2^32 = 2^-8 ≈ 0.39%`。本轮 `known` 230–245，**期望在区数 0.90–0.96 个**；
实测 0 / 1 / 0，§17 实测 0 / 2 / 2 —— 两者与「路由表相对该 key 近似随机」完全自洽。

也就是说：爬行确实在下降（从 2^128 量级压到 2^24 量级，约 3.2 bit/跳），但**最后 1–2 bit 到不了**，
因为末端节点的 11 个邻居已经给不出更近的接续，`FRONTIER_FACTOR` 带内随即枯竭。
要稳定拿到 ≥5 个在区目标，按同一密度需要 `known ≈ 5 / 0.0039 ≈ 1280`，即爬行规模再翻 5 倍——
这不是「再加 16 跳」能给的量级。

### 18.4 处置：跳数回退 16

A 无收益、单轮成本翻倍（17→32 分钟，报文 1.77 倍），且更深的爬行会让 §15 的 0.2 pps 预算更容易撞墙。
v2.4.25 已把 `MAX_FIND_HOPS` 回退为 16，并在常量注释里保留本次实验结论；要复现选 A 就改回 32。
这一条是**代码常量回退，不是判据放宽**：`verified ∧ global IPv4 ∧ top_chunk ≤ 2^24` 三道过滤一字未动。

### 18.5 五轮累计的 0/5 到底算什么

| 轮次 | 在区目标 | 发布 | 回 `PUBLISH_RES` | 同目标此前是否回过 `0x29` |
| :--- | :--- | :--- | :--- | :--- |
| v2.4.23 | 4 | 4 | 0 | 是（数十秒前） |
| v2.4.24 | 1 | 1 | 0 | 是 |
| **合计** | 5 次（4 个不同主机，`82.63.191.18` 重复） | 5 | **0** | 5/5 |

按 §17.4 的源码结论，`Process2PublishKeyRequest` 只有两条 early return（自身 UDP firewalled、超距），
其余情况**无条件**回 `PUBLISH_RES`；超距已被 `verified_in_zone` 排除，tag 解析异常已被 §17.3 逐字节排除。
所以静默唯一自洽的解释仍是「对端自己认定 UDP firewalled」。设公网 NAT 后节点自判 firewalled 的比例为 p，
5 次全静默的概率是 `p^5`：p=0.6 → 7.8%，p=0.7 → 16.8%。**这个混淆不可能靠增加目标消除**——
每多一个目标，只是多一次同分布的抽样；只能换测量工具。

**仍然不能得出的结论**：公共 KAD 拒绝该命名空间。本轮没有拿到任何一个明示拒绝（`PUBLISH_RES` 的 load 字段、
载荷上限、Key 被丢弃），只有静默。

### 18.6 下一步：改用 LAN 真实 aMule 做索引闭环（选项 D）

| # | 动作 | 代价 | 能得到什么 |
| :--- | :--- | :--- | :--- |
| D | 把 `lan-probe` 扩到索引操作：对 `172.16.29.209:41296` 发 `0x43`，拿到 `PUBLISH_RES` 后再发 `0x33` 取回 | 纯代码 + 局域网友好节点，约 10 分钟 | 用**真实 aMule**闭环「发布被接收 + 随后可检索」，把「我们的索引路径不对」这一支彻底排除 |
| B | `ALPHA_QUERY` 5→8 + `MAX_FIND_HOPS` 24 | 报文 2.2 倍 | 作用在同一个稀疏机制上；本轮已证明宽探会推高带外比例，不建议 |
| C | `sample_size` 3→10（需所有者改 confirmed 值） | 报文 3.3 倍，2–3 小时 | 只线性增加抽样次数，在区目标仍约 1 个/key |

**建议做 D 而不是 B/C**：B/C 都在同一个「抽到几个陌生人」的机制上加码，而 D 换掉的是混淆源本身。
LAN 源地址在 aMule 里走 `IsLanIP` 豁免（不判距离），且局域网内没有 NAT 防火墙测试失败的问题，
所以它能给出的信号是「真实 aMule 会不会收、收了能不能查回来」——这一支若成立，
剩下的不确定就纯属公网防火墙，而不是命名空间被拒。D 只读证据、固定
`public_kad_contacted=false` / `evidence_valid_for_pass=false`，不判 pass。

### 18.7 记账

- `S2.status` 保持 `not_run`；无 `PUBLISH_RES`、无 `SEARCH_RES`、0 命中。
- 门禁 `passed = true`（执行人 attest 已提供），`public_kad_contacted = true`。
- `PROTOCOL_VERSION = 1`，T0 golden 未动，`spikes.json` 结论字段未回填（只由人填写）。
- 全程 0.2 pps；发送 380 个（375×`0x21` + 1×`0x43` + 1×`0x33` + 3×`0x01`）、接收 318 个
  （315×`0x29` + 3×`0x09`），合计 698 条原始记录，与 `live-run-v2424.json` 的 `raw_records` 长度吻合。

## 19. 选项 D：局域网友好节点做裁判，定位到 Kad tag 帧头写错（2026-11-10，v2.4.26）

### 19.1 为什么要换测量工具

§18.5 得出的结论是「0/5 静默只有『对端自判 firewalled』一个自洽解释」。这个结论有一个隐含前提：
**我们的报文是合法的**。前五轮全部靠离线自证来支撑这个前提，而离线自证是拿我们自己的解码器
去验我们自己的编码器——裁判和选手是同一套代码。局域网里的那台真实 aMule 可以直接当裁判：
它走 `IsLanIP(ip)` 豁免距离判据，不存在公网 NAT 的混淆，回不回包只剩「它认不认我们的包」。

### 19.2 第一次执行：连局域网也静默

`172.16.29.209:41296`（真实 aMule，与执行机 `172.16.29.210` 同网段）：

| 步骤 | 结果 |
| :--- | :--- |
| `BOOTSTRAP_REQ` → `0x09` | ✅ 20 个联系人，RTT 15.6 ms（双向 UDP 通） |
| `0x43` 发布（5 tag） → `0x4B` | ❌ `10060` 超时 |
| `0x33` 查询 → `0x3B` | ❌ 未发送（发布未确认，跳过） |

`distance_top32 = 3,428,064,205 > 2^24`，但 LAN 豁免本应让这条不成立。所以**不是公网、不是
firewalled、不是距离**能解释的，问题在报文本身。

### 19.3 二分：0 个 tag 能回，1 个 tag 就静默

先按 tag 数量切（`--bisect`），再按单 tag 结构切（`--tag-probe`），每个变体独立 key：

| tag 数 | 帧长 | 回 `PUBLISH_RES` |
| :--- | :--- | :--- |
| 0 | 37 B | ✅ `load=0` |
| 1（FILENAME） | 84 B | ❌ 静默 |
| 2（+FILESIZE） | 90 B | ❌ |
| 3（+SOURCEUPORT） | 94 B | ❌ |
| 4（+descriptor 长名） | 204 B | ❌ |
| 5（+namespace STR14） | 244 B | ❌ |

单 tag 六种取值（1 字符文件名 / 43 字符文件名 / UINT32 / UINT16 / 长名短值 / 长名 14 字节）
**全部静默**。与 tag 的类型、名字长度、值长度都无关——是 tag 的**帧头**本身。

### 19.4 源码定位：Kad 用 `ReadTag`，不是 `CTag::CTag`

`Process2PublishKeyRequest` 里读 tag 的是 `bio.ReadTag()`，即 `CFileDataIO::ReadTag`
（`src/SafeFile.cpp`）：

```cpp
type = ReadUInt8();
name = ReadString(false);          // 恒为 uint16 长度 + 字节，不处理 0x80 标志
switch (type) {
  case TAGTYPE_HASH16 / STRING / UINT64 / UINT32 / UINT16 / UINT8 / FLOAT32 / BSOB: ...
  default: throw wxString(CFormat("Invalid Kad tag type; type=0x%02x ..."));
}
```

与 `.met` 路径的 `CTag::CTag(CFileDataIO&, bool)` 完全不同——后者才处理 `0x80` 短名，
才认 `TAGTYPE_STR1..STR22`。而 `FileTags.h` 写明 `#define TAG_FILENAME wxT("\x01")`：
**数值名字在线上就是一个长度 1 的字符串**。

我们写的是 ed2k/met 格式 `[type|0x80][name:1]`：`02|0x80 = 0x82` 落进 `default`，
直接 `throw "Invalid Kad tag type; type=0x82"`；即使侥幸过了解析，`namespace` 的 14 字节值
被编成 `TAGTYPE_STR14 (0x1E)`，同样落进 `default`。异常在
`catch(...) { delete entry; throw; }` 里原样抛出，`PUBLISH_RES` 那一行永远执行不到。

**这就是五轮公网发布 0 回应的真实原因。** §17.3「tag 编码逐字节合法」的结论作废——
当时对照的是 `CTag::CTag`（met 路径），而不是 Kad 路径的 `ReadTag`：
用 `CTag` 的规则解 ed2k 格式当然"零残余、零非法类型"，但 aMule 根本不走那个解析器。

### 19.5 修正（v2.4.26）

| 位置 | 改动 |
| :--- | :--- |
| `wire.rs::Tag::encode` | 帧头改为 `[type:u8][name_len:u16][name][value]`；字符串一律 `TAGTYPE_STRING`，不再自动选 `STRn` |
| `wire.rs::Tag::decode` | 同步；并显式拒绝 `STRn` / `BLOB`（aMule 的 `ReadTag` 读到就会抛） |
| `wire.rs` 单测 | 新增 `tag_kad_layout_matches_amule_readtag`（字节固定向量：`02 01 00 01 02 00 'h' 'i'`）与 `tag_decode_rejects_types_amule_cannot_read` |
| `kad-mini/README.md` §3 | tag 编码拆成「Kad 路径」与「ed2k/met 路径（Kad 上无效）」两行 |
| `PROTO` §2.0 | 升 v2.3：静默路径由两条改三条，新增 tag 帧头一行 |

回归：`cargo test` **15/15**、`selftest` **37/37**、`loopback` **5/5 发布 + 5/5 命中**、0 协议错误。

### 19.6 修好后的局域网闭环（同一台 aMule）

```
verdict                        = confirmed_publish_then_retrieve
publish_acked                  = true     publish_load = 1   rtt 5.2 ms
publish_key_echo_matches       = true     frame 252 B
search_responded               = true     answers = 1       rtt 8.5 ms
target_echo_matches            = true
our_descriptor_returned        = true
namespace_tag_matches          = true
```

即：真实 aMule 收下了我们的命名空间条目，并用 `0x33` 把它原样交还——**索引路径本身是通的**。

### 19.7 对前面五轮的影响

- 五轮 0/5 静默**不是**「公共 KAD 拒绝该命名空间」，也**不是**「对端 firewalled」。
  §18.5 那个 `p^5` 概率论证建立在「我们的报文合法」这一前提上，前提已被推翻，结论作废。
- 反过来说，`verified_in_zone` 目标策略、前沿护栏、0.2 pps 限速这些工作没有白做：
  修好帧头之后，它们正好是「把合法报文送到正确的节点」的那一半。
- **方法论教训**：离线自证只能证明自洽，不能证明互操作。一个 10 分钟的局域网真实节点
  比五轮实网更有信息量。后续任何 wire 变更，都应先过 `lan-index-probe` 再上公网。

### 19.8 下一步

重跑公网 P4（G0 参数不变，`sample_size=3` / `0.2 pps` / 明文 `0xE4` / 跳过 HELLO）。
这次预期第一次看到 `PUBLISH_RES`；判据仍是 RISK R-02 的三桶计数，`S2.status` 只由人回填。
需要执行人重新 attest。

### 19.9 记账与副作用

- `lan-index-probe` 的 `public_kad_contacted=false`、`evidence_valid_for_pass=false`，不判 pass。
- 局域网那台 aMule 的索引里会留下本轮若干条测试条目（每变体一个 key）。aMule 的索引在内存里，
  **重启该客户端即全部清空**；它若同时连着公共 KAD，理论上会在 TTL 续期时把条目重发一次。
- 本轮全部为私有地址，0 个公网报文；公网 KAD 未接触。

## 20. P4 第七轮：修好 tag 帧头后的首次公网闭环（2026-11-10，v2.4.26 代码 / 本轮无改动）

### 20.1 执行

`live --bootstrap reports/bootstrap.fresh.local.json --sample-size 3 --rate-limit 0.2`，
执行人显式 attest，门禁 `passed=true` / 0 blocker。G0 参数与目标策略一字未改
（`sample_size=3`、`0.2 pps`、明文 `0xE4`、跳过 HELLO、`verified_in_zone`）。
耗时 **10 分 53 秒**，原始记录 **236 条**（`kad-mini/live-run-v2427.json`）。

### 20.2 结果

| seq | logical | hops | known | verified | 在区 | closest | PUBLISH | SEARCH | 命中 |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| 0 | `coldrevive-t0-sample.bin` | 16 | 200 | 49 | 0 | 28,011,374 | 跳过（无在区目标） | — | — |
| 1 | `wakeup_req.v1` | 16 | 197 | 52 | **2 / 2** | **13,899,520** | **1 发 1 回**（`load=1`，302 ms） | 1 发 1 回（1 条答案） | **是** |
| 2 | `wakeup_ack.v1` | 0 | 0 | 0 | 0 | — | bootstrap 超时（`81.33.83.203:4672`） | — | — |

报文：`0x21` 发 124 / `0x29` 收 101（**应答率 81.5%**）、`0x43`×2、`0x33`×2、`0x4b`×1、`0x3b`×1、`0x01`×3、`0x09`×2。

### 20.3 同一个对端的前后对照

`82.63.191.18:4672` 在 v2.4.23 / v2.4.24 两轮里对同一个 key `fac258d1…` 的 `0x43` **两次都静默**；
本轮同一台机器、同一个 key、同一份 bootstrap 清单，**回了 `PUBLISH_RES`（`load=1`，302 ms）**，
随后 `0x33` 也回了 `SEARCH_RES`。唯一的差别是 tag 帧头（§19.5）。
这是本轮最干净的因果证据：对端没变、key 没变、路径没变，只有编码变了。

### 20.4 命中是真命中：把 `0x3b` 解到底

`0xE5`（zlib）帧，线上 230 B，解压后 265 B，**265/265 字节全部消费、零残余**：

```
sender  = fa164fd17af5c568d89b4cdb0b978869
target  = fac258d1e7ef42ae83e81c18eec7ba4e   ← 与我们发布的 Key 逐字节相同
count   = 1
entry[0].source_id = dd0156fe27de81fd4eed919f8ef488dc  ← 与本轮发布条目的 source id 相同
  tag 0x02 name="\x01"                        value="ColdRevive_V1:coldrevive:t0:wakeup_req.v1"
  tag 0x08 name="\x02"                        value=1024
  tag 0x08 name="\xFC"                        value=63247
  tag 0x02 name="ColdRevive_V1:descriptor"    value="v=1;node=live-probe;wake_port=63247;..."
  tag 0x02 name="ColdRevive_V1:namespace"     value="ColdRevive_V1:"
  tag 0x03 name="3"                           value=16843752
```

五个 tag 全部原样返回，包括两个 24 字符自定义名。**这是公共 KAD 的第三方节点存储并服务回我们命名空间条目的直接证据**，
不是本地内存回显（该进程从不读自己的发布记录）。

### 20.5 静默的那一半

`170.253.12.128:42566`（第二个在区目标）对 `0x43` 与 `0x33` **都超时**（`10060`）。
它在几秒前刚回过 `0x29`（否则不会进 `verified` 集合），所以不是地址错误、不是距离越界，
只能是它自身的 UDP 入站不可达（NAT/firewall）。**这与 §18.5 的推论一致：静默确实是"对端不可入"，
而不是"索引层拒绝"**——同一个包里，能回的那个回了，不能回的那个对两种 opcode 一样不回。

### 20.6 判据逐条对账（`spikes.json` 的 `S2.criteria`）

| # | 判据 | 本轮证据 | 状态 |
| :--- | :--- | :--- | :--- |
| 1 | 双节点闭环：A `0x43` 发布命名空间 Key，B `0x33` 命中 A 的描述符 Tag | 发布被第三方节点接受，`0x33` 取回 5 个 tag | ✅（**但 A、B 是同一进程、同一 KadID、同一源地址**，见 §20.7） |
| 2 | 发布前 `0x21/0x29` 多跳收敛到 `2^24` 邻域 | `closest = 13,899,520 < 16,777,216`，`在区 2` | ✅ |
| 3 | 记录成功率/时延；协议层无响应与索引层拒绝分开计数 | 2 目标：1 接受（302 ms）、1 UDP 超时、**0 明示拒绝** | ⚠️ 样本 n=1，拒绝率区间 `[0, 79%]`（95% 上界），不足以判 R-02 |
| 4 | 隔离口径：前缀只作用于本地哈希输入 | 线上只有 128-bit ID；`namespace` tag 仅供读取方过滤 | ✅ |
| 5 | 实网小样本，非回环/局域网 | `public_kad_contacted=true`，全部报文发往公网 IPv4 | ✅ |
| 6 | G0 样本量与限速 + 逐次原始报文 | `sample_size=3` / `0.2 pps` / 236 条原始记录 | ✅ |

### 20.7 两个必须写在结论旁边的限定

1. **A 与 B 是同一个进程**。本轮由同一个探针先发布、5 秒后对同一节点查询。存储者是第三方节点，
   取回也来自第三方节点的索引（探针不读自己发的包），所以"存储 + 服务"是真实的；
   但"另一台主机能否取回"没有被测到。若要满足判据字面的"双节点"，需要 B 用**另一个 KadID/另一台主机**查询同一个 Key。
2. **n = 1**。3 个样本里只有 1 个既收敛又在区（seq 0 无在区目标，seq 2 bootstrap 直接超时）。
   `publish_load=1` 只说明这一个节点收下了，不构成"公共 KAD 普遍接纳"的统计结论。
   要把 R-02 的"拒绝率 > 20%"判据做实，至少需要 10 个以上不同节点的接受/拒绝计数。

### 20.8 记账

- `S2.status` 仍 `not_run`（程序不自动置 pass；结论由执行人回填 `spikes.json`）。
- 报告固定写 `public_kad_contacted=true` / `evidence_valid_for_pass=false`——
  后者是"程序永不自证"的常量，与历轮一致，不是对本轮结果的否定。
- `PROTOCOL_VERSION = 1`，T0 golden 未动，`spikes.json` 未改。
- 全程 0.2 pps；发送 131 个报文（124 `0x21` + 2 `0x43` + 2 `0x33` + 3 `0x01`）。
- 本轮代码零改动，仅重跑；`live-run-v2427.json` 为原始证据。

## 21. 两进程 A/B：把「A 发布、B 命中」做成字面意义上的跨节点（2026-11-10，v2.4.28 → S2 = pass）

### 21.1 为什么还要补这一轮

§20 的闭环里，发布与查询是**同一个进程、同一个 socket、同一个源端口、同一个本地 KadID**。
存储者是第三方节点，取回也来自它的索引（探针不读自己发的包），所以「第三方节点存储并服务」成立；
但 `S2.criteria` 第 1 条的字面是「A 发布…**B** 命中」，A 与 B 应当是不同节点。这一条此前是缺口。

### 21.2 工具：`split-probe`（新增，复用同一 `live_gate`）

```
kad-mini split-probe --stage publish --bootstrap FILE --handoff FILE \
        --sample-size 3 --rate-limit 0.2 --attest-i-am-the-experiment-operator
kad-mini split-probe --stage search  --bootstrap FILE --handoff FILE \
        --sample-size 3 --rate-limit 0.2 --attest-i-am-the-experiment-operator
```

- **A（`split-a`）**：走与 `live` 相同的 bootstrap + 前沿护栏漏斗，发布到 `verified ∧ 在区` 的目标，
  把「确认地址 + 条目 source id + Key」写进 handoff（TSV），**不查询**。
- **B（`split-b`）**：另一个进程，`bind_any` 拿到**不同源端口**，本地 KadID 种子不同，bootstrap 入口也不同
  （`nodes[(row+3) % len]`）。它只从 handoff 知道 Key 与 A 被确认的地址，**自己重走一遍 tolerance zone**，
  然后先查 A 的确认地址、再查自己的在区集合。
- 门禁完全复用：`live_gate` 的批准记录、G0 参数一致性、公网 bootstrap 校验、attest 一个不少。
  未加 attest 时实测直接拒绝（“门禁未通过：缺少 --attest-i-am-the-experiment-operator”），0 报文。
- 两个阶段都固定写 `public_kad_contacted=true` / `evidence_valid_for_pass=false` / `s2_status=not_run`：
  程序不自证，pass 只能由人写进 `spikes.json`。

### 21.3 A 侧（`live-split-a.json`，14 分 30 秒）

| 节点 | 本地 KadID | 源端口 |
| :--- | :--- | :--- |
| `split-a` | `e465a455…06cd` | 61195 |

| seq | logical | hops | known / verified | 在区 | closest | 发布结果 |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| 0 | `coldrevive-t0-sample.bin` | 16 | 189 / 43 | 0 | 28,011,374 | 跳过（无在区目标） |
| 1 | `wakeup_req.v1` | 16 | 167 / 57 | **1** | **13,899,520** | **`PUBLISH_RES` @305 ms，load=0** |
| 2 | `wakeup_ack.v1` | 16 | 158 / 45 | 0 | 61,240,267 | 跳过（无在区目标） |

handoff 只有一行：`v1  1  coldrevive:t0:wakeup_req.v1  fac258d1…   a89b9f21…  82.63.191.18:4672`。
（`load=0` 与 §20 的 `load=1` 差异在 `PUBLISH_RES` 的回显字段，不影响「已确认」；两种取值下对端都确实回包。）

### 21.4 B 侧（`live-split-b.json`，5 分 15 秒）

| 节点 | 本地 KadID | 源端口 | bootstrap 入口 |
| :--- | :--- | :--- | :--- |
| `split-b` | `0dab3f81…16e4` | **49876** | `79.116.53.69:54709`（A 用的是 `5.94.6.235:10035`） |

B 自己走完 16 跳：`known=194`、`verified=47`、在区 1、`closest=13,899,520`，然后对
`82.63.191.18:4672` 发 `0x33` → `SEARCH_RES`（312.7 ms，**2 条答案**），其中一条：

```
source_id = a89b9f21cf19e86908d522b329806888      ← 正是 A 写入 handoff 的条目
  "\x01"                     = "ColdRevive_V1:coldrevive:t0:wakeup_req.v1"
  "\x02"                     = 1024
  "ü"                        = 61195
  "ColdRevive_V1:descriptor" = "v=1;node=split-a;wake_port=61195;proto_flags=bt-only;..."
  "ColdRevive_V1:namespace"  = "ColdRevive_V1:"
  "3"                        = 16843252
```

**判别项**：条目里的 `node=split-a;wake_port=61195` 是 **A 的**源端口，而 B 自己的源端口是 49876。
所以这不是 B 读自己的记录，也不是把 B 的发布读回来 —— 条目由 A 创建，B 从第三方索引取回。

### 21.5 独立性盘点（哪些维度真的不同）

| 维度 | A | B | 是否独立 |
| :--- | :--- | :--- | :--- |
| 进程 | `split-a` 进程 | `split-b` 进程 | ✅ |
| UDP 源端口 | 61195 | 49876 | ✅ |
| 本地 KadID 种子 | `e465a455…` | `0dab3f81…` | ✅ |
| bootstrap 入口 | `5.94.6.235:10035` | `79.116.53.69:54709` | ✅ |
| 路由路径 | 自建路由表 167 联系人 | 自建路由表 194 联系人 | ✅ |
| 公网 IP / 主机 | 同一台 | 同一台 | ❌ **未覆盖**（只有一台机器） |

最后一行是这一轮唯一的缺口，但不影响判据：KAD 的「节点身份」是 `(KadID, UDP 地址)`，
两个进程在这两项上都不同；换成两台机器只会进一步增加差异，不会减少结论强度。

### 21.6 结论与记账

`S2.status` 由项目所有者指令「补一轮 B，成功后直接判 pass」登记为 **`pass`**（2026-11-10），
证据登记在 `tests/spikes/spikes.json` 的 `observations.p4_execution_v2427` 与
`observations.ab_two_process_2026_11_10`；下游 `TECH-STACK §2`、`ROADMAP`、`TEST §2`、
`RISK R-02`、`OPEN-QUESTIONS Q1`、`ADR-001` 已同步。

- 本轮公网报文：A 侧 324 条（发 `0x21`×171、收 `0x29`×145、`0x01`×3、`0x43`×1、`0x4b`×1），
  B 侧 113 条（发 `0x21`×62、收 `0x29`×47、`0x01`×1、`0x33`×1、`0x3b`×1）；
  B 的目标集合去重后只剩 1 个（它自己的在区节点恰好就是 A 的确认地址），全程 0.2 pps，无批量发布。
- 判决边界：只判**接纳性**（能存、能取）。R-02 的「运营速率下是否被当污染」判据仍需 M4 前按实测拒绝率复审，
  `OPEN-QUESTIONS Q1` 已收窄为只剩这一半。
- `PROTOCOL_VERSION = 1`，T0 golden 未动，`PROTO` 无需再改（v2.3 的 tag 帧头修正已覆盖本轮）。

## 22. ADR-011 L1：eMule 关键词字典互通（2026-11-11，v2.4.29 / v2.4.30）

### 22.1 决策与范围

项目所有者 2026-11-11 定下 D1–D4（全部文件公开、eD2k 不区分冷热、现在实现 eD2k 传输栈、
法务不作前置门禁），批准 **L1 + L2 开工**、**否决 L3 唤醒网关**。详见
[ADR-011](../../../docs/02-architecture/adr/ADR-011-ed2k-interop.md)。

审查结论要点：上一轮把「BT 未活跃做种」误当成「文件位于远端休眠节点」。核对本项目自身设计后
（`LLD §6` 的 `COLD` = 仅一条索引行、切入时「从磁盘加载元数据」；`ADR-006` 元数据 URL 是本机；
`PRD US-06` 守护进程常驻；`LLD §7` M-10 是常在线监听器），文件始终在本机磁盘、节点常在线，
eD2k 源只需常在线监听读盘发片，与 M-10 同构。**网关是伪需求**。节点真下线时源消失，
与 eMule 源 TTL 一致，不引入额外机制。

### 22.2 源码核实：eMule 关键词 Key 是 MD4，不是 SHA-1

`src/kademlia/kademlia/Kademlia.cpp::KadGetKeywordHash`（aMule `6a110563`）：

```cpp
uint8_t Output[16];
CryptoPP::Weak::MD4 md4_hasher;
char* ansi_buffer = strdup(unicode2UTF8(rstrKeyword));
md4_hasher.CalculateDigest(Output, (const unsigned char*)ansi_buffer, strlen(ansi_buffer));
pKadID->SetValueBE(Output);
```

`UInt128.cpp::ToByteArray` 是 `SetValueBE` 的逆，所以**线上 16 字节即 MD4 摘要原值**。
函数内不做小写化 / trim / Unicode 归一化。这与 PROTO §1 的
`Keyword_Key = SHA1("ColdRevive_V1:kw:" + term)[0..16]` 是两套算法，双轨不得复用同一哈希函数。
`PUBLISH_SOURCE_REQ = 0x44` 已在 `wire.rs::op` 的源码核实表里，但 PROTO §2.0 尚未描述其载荷，
L2 实现前须按 `Process2PublishSourceRequest` 补齐。

### 22.3 L1 实现（全部在 `kad-mini` 探针内，未触碰生产 crate）

| 位置 | 内容 |
| :--- | :--- |
| `wire.rs::md4` | RFC 1320 纯 Rust 实现，零依赖；5 条已知答案向量（`""` / `a` / `abc` / `message digest` / 26 字母） |
| `wire.rs::kad_keyword_hash` | `MD4(UTF-8(term))`，兼容轨关键词 Key |
| `wire.rs::ed2k_hash` | 单分块 = `MD4(file)`；多分块 = `MD4(concat(MD4(part)))`，`ED2K_PART_SIZE = 9_500 * 1024 = 9_728_000 B`；**精确整数倍追加 `MD4("")`**（aMule Red 规则，v2.4.31 更正，见 §23.2；本行原记「不补空分块」，已撤回） |
| `main.rs::ed2k-dict-probe` | RFC1918 硬校验 → `BOOTSTRAP_REQ` 证明双向 → `0x43` 发布无前缀关键词条目 → `0x33` 取回 → 产出 `ed2k://` 链接；固定 `public_kad_contacted=false` / `evidence_valid_for_pass=false` |

兼容轨条目：`fileID = ed2k_hash`（真实内容导出），tag 为 `TAG_FILENAME "\x01"` 与
`TAG_FILESIZE "\x02"`，均按 §19 修正后的 Kad `ReadTag` 格式。

### 22.4 局域网真实 aMule 实测（`reports/kad-ed2k-dict-probe-20261111012841.json`）

目标 `172.16.29.209:41296`，关键词 `coldrevive-ed2k-probe-1794360511391`（唯一无意义词，
避免撞进真实用户的关键词桶）。

```
verdict                   = confirmed_unnamespaced_keyword_publish_then_retrieve
keyword_key               = 0dbd5fcd4be789dd6c5414cb521ce121   (MD4(UTF-8(term))，无前缀)
ed2k_hash / fileID        = fb7678827ad305fda43f0799ebbf0cc9
ed2k_link                 = ed2k://|file|coldrevive-ed2k-probe-...txt|63|fb7678...|/
bootstrap                 = 20 contacts
PUBLISH_KEY_REQ (90 B)    -> PUBLISH_RES  load=1, key 回显匹配, 7.0 ms
SEARCH_KEY_REQ            -> SEARCH_RES   1 条答案, 11.7 ms, our_entry_returned=true
返回 tag                  = "\x01"=文件名, "\x02"=63, "3"=16843252（aMule 自行附加）
```

即：**真实 aMule 收下并服务了一个不带 `ColdRevive_V1:` 前缀、fileID 为真实 eD2k 哈希的
关键词条目**。L1 的字典互通路径打通。

### 22.5 证据边界（必须跟结论写在一起）

- 本探针证明的是「aMule 按我们发过去的 target Key 存下并回服务条目」。
  「用户在 aMule 界面搜这个词时，aMule 自己算出的 Key 与我们一致」是**源码核实**
  （`KadGetKeywordHash`），不是本轮运行时验证 —— 探针无法驱动 aMule 的 UI 搜索。
- 本轮 **0 个公网报文**，`public_kad_contacted=false`，不判任何 pass，不回填 `spikes.json`。
- 副作用：若这台 aMule 同时连着公共 KAD，它可能为自己持有的索引条目做 TTL 续期并重发一次；
  探针因此只用唯一无意义关键词，并在报告 `note` 里保留该说明。

### 22.6 回归

`cargo test` **18/18**（新增 MD4 向量、关键词哈希、eD2k 单/多分块 3 项）、
`selftest` **42/42**（新增 5 项）、`loopback` **5/5 发布 + 5/5 命中**、0 协议错误、
`build_docs --check` 35 份通过。`PROTOCOL_VERSION = 1`、T0 golden 未动。

### 22.7 当时记录的下一步（L2）—— 第 1、2 项已于 2026-11-11 在 §23 执行

1. ~~按 [OPEN-QUESTIONS Q9](../../../docs/06-plan/OPEN-QUESTIONS.md) 核实 `ed2k-rs` 维护状态与可复用边界（V-04）~~ → **已完成并结案**，见 §23.1；Q9 已从待决问题表移出，V-04 标记为「已确认」。
2. ~~按源码补齐 `PUBLISH_SOURCE_REQ (0x44)` 载荷，实现 `cr-ed2k` 最小下载/上传栈 + 最小上传队列~~ → **已完成**，但按 G0 冻结落在 `tests/spikes/s2/`（见 §23.0），不是 `src/crates/cr-ed2k`。
3. 双轨发布**全量共享文件**（D1）并按 R-02 在 M4 前实测拒绝率 → **仍待 M1/M4**，本轮只做了单条唯一关键词的单文件探针。
4. 任何公网发布都需执行人**重新显式 attest** → 本轮未做、也未自动加该参数；此红线继续有效。

> 原第 1 项的相对链接曾写成 `../../docs/...`（解析到不存在的 `tests/docs/`），v2.4.31 一并修正为
> `../../../docs/...`。

## 23. L2 落地：源字典 + 最小 eD2k 传输栈（2026-11-11）

> 本节归档 [ADR-011 L2](../../../docs/02-architecture/adr/ADR-011-ed2k-interop.md) 的实施与证据，
> 执行的是 §22.7 第 1、2 项。**全部证据只在 RFC1918 / 127.0.0.1 上产生**：
> `public_kad_contacted=false`、`public_network_contacted=false`、`evidence_valid_for_pass=false`，
> **不判任何 pass、不回填 `spikes.json`**。协议映射的正式定义见
> [PROTO §2.2](../../../docs/04-protocol/PROTO.md)（v2.5）。

### 23.0 为什么仍在 `tests/spikes/`：G0 未通过

> **2026-11-13 更新**：本节记录的是 v2.4.31 当时口径。G0 已于 2026-11-13 由项目所有者批准
> [条件冻结](../../../docs/06-plan/G0-REVIEW.md)，技术路线冻结已生效；但 M1 二进制与 `CliAdapter` 接线未完成，
> 本 L2 参考实现**仍留在 `tests/spikes/`**，待 M1 按门禁迁入 `cr-ed2k`。下方历史论证保留供追溯。

[ROADMAP G0](../../../docs/06-plan/ROADMAP.md) 未通过（S1 / S3–S7 仍 `not_run`），
[src/README.md](../../../src/README.md) 规定「技术路线冻结后才创建 `Cargo.toml` 与 crate 实现」。
因此 L2 产物是 `kad-mini` 新增的 `l2-source-probe` 与新 spike crate `ed2k-mini`，
**不是** `src/crates/cr-ed2k`；G0 通过后整体迁入 M-12。
`ed2k-mini` 零依赖，`MD4` / `ed2k_hash` / KAD tag codec 通过 `#[path]` 复用 `kad-mini/src/wire.rs`，
仓库里**只有一份哈希实现**。

### 23.1 Q9 / V-04：`ed2k-rs` 与 `ed2k` 都不能作为传输栈复用

| 核实项 | 结果 |
| :--- | :--- |
| crates.io `ed2k-rs` | **不存在该 crate**（API 返回 `crate ed2k-rs does not exist`）；只有 GitHub `runfalk/ed2k-rs` |
| `runfalk/ed2k-rs` | alpha、**仅哈希**（MD4 / eD2k），无 AICH、无任何传输栈；最后 push `2023-08-28T16:28:05Z`，4 stars / 0 forks / 0 issues |
| crates.io `ed2k` 1.0.1 | **仅哈希**，最后更新 `2023-05-19T22:55:00.744168Z`；提供 Red / Blue 两种变体，**默认别名 `Ed2k` = Blue** |
| 结论 | 两者都不提供可复用的完整 eD2k 传输栈；最多借鉴哈希实现与测试向量。v1 保留仓内可审计的自实现，不引入停更依赖（[RISK R-12](../../../docs/05-quality/RISK.md)） |

**Q9 已结案**：从 [OPEN-QUESTIONS](../../../docs/06-plan/OPEN-QUESTIONS.md) 待决表移出（结论与依据留痕于 v2.4.31 变更记录），
[REFERENCES V-04](../../../docs/08-reference/REFERENCES.md) 标记「已确认」。
credit 缺失的实际排队代价（R-15）仍属 M1 实测项，Q9 原来的「最小栈自研 vs 复用」部分已经回答：**自研**。

### 23.2 源码核实连带发现并修正 `ed2k_hash` 的死条目缺陷（**更正 §22.3**）

L1 的 63 B 探针文件从未跨过 `PARTSIZE` 边界，掩盖了一个会让 A 轨变成**死条目**的缺陷。
文件大小恰为 `9_728_000` 的整数倍时存在两套互不兼容算法：

```
size < PARTSIZE           : ed2k_hash = MD4(file)
size >= PARTSIZE          : ed2k_hash = MD4(concat(MD4(each part)))
size % PARTSIZE == 0      : 额外追加 MD4("") = 31D6CFE0D16AE931B73C59D7E0C089C0   # aMule "Red"
```

三条 aMule `6a110563` 依据：`KnownFile.cpp::SetFileSize` 给出真实样本（1×PARTSIZE 文件有
2 个分块哈希、文件哈希 `A72CA8DF7F07154E217C236C89C17619`）、
`ThreadTasks.cpp::CreateNextPartHash` 在 `partLength == PARTSIZE && file.Eof()` 时显式
`push_back(g_emptyMD4Hash)`、`KnownFile.cpp::CreateHashFromHashlist` 对整表做 MD4。
crates.io `ed2k` 的默认 **Blue** 变体不补空分块 —— 直接复用会生成与所有旧客户端都不同的
`fileID`：搜得到、永远下不动。

修正落在 `kad-mini/src/wire.rs`：`ed2k_hash` 按 Red 追加空分块、新增 `ed2k_part_hashes`、
新增测试 `ed2k_hash_matches_amule_documented_one_part_file`（用上游自己的已知答案向量），
并把 §22.3 中「精确整数倍不补空分块」的旧断言**反转**。正式口径见
[PROTO §2.2.3](../../../docs/04-protocol/PROTO.md)。

### 23.3 索引面：`0x44` 源发布 + `0x34` 源查询，在真实 aMule 上双轨闭环

关键词条目（`0x43`）只让旧客户端**搜到**文件；要能**下载**，同一 `fileID` 还必须在 KAD
源字典里有源条目。两笔是独立发布，缺一不可。源码核实结论（逐条出处见 PROTO §2.2.1）：

| 约束 | 结论 |
| :--- | :--- |
| 载荷布局 | `<fileID 16><sourceID 16><tagcount u8><Kad ReadTag tags>`，**没有** `0x43` 里的 `u16` 条目数，一笔只带一个源 |
| tag 数宽度 | 1 字节（`CFileDataIO::WriteTagPtrList`），不是 2 字节 |
| 距离校验 | 用 **`fileID`** 而非 keyword key 判 `SEARCHTOLERANCE`；LAN 源由 `IsLanIP` 豁免 |
| 必需 tag | `TAG_SOURCETYPE (0xFF)` 缺失时处理器 `delete entry` 且**不回 `PUBLISH_RES`**，与越界/防火墙不可区分 |
| 源地址 | IP / UDP 端口由接收方取自 **UDP 报头**；`TAG_SOURCEIP` 是接收方自行补的，伪造无效 |

HighID 源 tag 集照 `Search.cpp` 的 `STOREFILE` 分支：
`TAG_SOURCETYPE=1`、`TAG_SOURCEPORT (0xFD)`、`TAG_SOURCEUPORT (0xFC)`、
`TAG_FILESIZE (0x02)`、`TAG_ENCRYPTION (0xF3)=0`，全部按 Kad `ReadTag` 格式。

实现：`wire.rs` 新增 `publish_source_req` / `parse_publish_source_req` / `source_tags` /
`search_source_req` / `parse_search_source_req` / `ed2k_part_hashes`，`main.rs` 新增
`l2-source-probe` 子命令。流程：RFC1918 硬校验 → `BOOTSTRAP_REQ` 证明双向 → 对同一 `fileID`
分别发 `0x43` 关键词条目与 `0x44` 源条目 → 用 `0x33` / `0x34` 各自取回。

**局域网实测**（`172.16.29.209:41296`，报告
[`kad-l2-source-probe-20261111023613.json`](reports/kad-l2-source-probe-20261111023613.json)）：

```
BOOTSTRAP_RES                      20 contacts
0x43 PUBLISH_KEY_REQ    -> PUBLISH_RES  load=1, 关键词条目返回 true
0x44 PUBLISH_SOURCE_REQ -> PUBLISH_RES  load=1, key 回显匹配（65 B / 5 tags）
0x33 SEARCH_KEY_REQ     -> SEARCH_RES   1 条答案, our_entry_returned=true
0x34 SEARCH_SOURCE_REQ  -> SEARCH_RES   1 条答案, our_source_returned=true
返回 tag                           \x02=55, \xfe=<IP>, \xff=1, \xfd=4662, \xfc=<UDP>, \xf3=0
verdict                           confirmed_dual_track_keyword_and_source_publish_then_retrieve
```

即：**真实 aMule 收下并同时回服务了我们的关键词条目与源条目**，③「旧客户端搜到我们」的
索引面打通。副作用同 §22.5：关键词与文件名都用唯一无意义串，该 aMule 若同时连公共 KAD
可能为持有条目做 TTL 续期并重发一次。

### 23.4 传输面：`ed2k-mini` 一次实现 ② 与 ④

②「我们从 eMule 下载」与 ④「旧客户端下到我们」是**同一份栈的两端**，
`stack::download()` / `stack::serve()` 一次实现：

| 方向 | 报文序列 |
| :--- | :--- |
| 客户端 | `OP_HELLO(0x01)` → `OP_HELLOANSWER(0x4C)` → `OP_REQUESTFILENAME(0x58)` → `OP_REQFILENAMEANSWER(0x59)` →（多分块）`OP_HASHSETREQUEST(0x51)` / `OP_HASHSETANSWER(0x52)` → `OP_STARTUPLOADREQ(0x54)` → `OP_ACCEPTUPLOADREQ(0x55)` 或 `OP_QUEUERANKING(0xC5/0x60)` → `OP_REQUESTPARTS(0x47)` → `OP_SENDINGPART(0x46)` → `OP_END_OF_DOWNLOAD(0x49)` |
| 服务端 | 对应上传侧 + **单槽最小上传队列**（无槽回 `OP_QUEUERANKING`，FIFO rank） |

帧格式 `src/OtherStructs.h::Header_Struct` =
`<protocol u8><packetlength u32 LE><opcode u8><payload>`，`packetlength = 1 + payload.len()`。
两个源码核实过的易错点：① HELLO tag 写 `[type][u16 name_len][name][value]`（与 KAD `ReadTag`
同布局，读侧兼容紧凑形式），这正是 §19 修掉的 v2.4.26 缺陷；② `OP_REQUESTPARTS` 是
**先 3 个 start 再 3 个 end**，不是交错的 `<start,end>` 对，写错会被静默忽略。

明确不做（范围声明）：压缩 `0x40`、混淆、积分 / credit / 安全标识、源交换、AICH、
>4 GiB 的 I64 分片（`0xA2`/`0xA3`）、eD2K 服务器连接、NAT 穿透。详见
[ed2k-mini/README.md](ed2k-mini/README.md)。

### 23.5 首轮回归与证据（2026-11-11 复跑一致）

| 项 | 结果 |
| :--- | :--- |
| `kad-mini cargo test` | **21/21**（新增上游 Red 向量、源发布/查询载荷、关键词哈希等） |
| `kad-mini selftest` | **48/48**（L1 为 42/42，L2 新增 6 项） |
| `kad-mini loopback` | 发布 ACK **5/5**、查询命中 **5/5**、协议错误 **0** |
| `kad-mini l2-source-probe` | 见 §23.3，`confirmed_dual_track_keyword_and_source_publish_then_retrieve` |
| `ed2k-mini cargo test` | **31/31**（其中 21 项来自通过 `#[path]` 复用的 `kad-mini/wire.rs`） |
| `ed2k-mini selftest` | **26/26** |
| `ed2k-mini loopback` | 3/3 用例完整闭环：`single_part_4KiB`（4 096 B / 1 块）、`exact_one_part_boundary`（9 728 000 B / 53 块 / 2 个分块哈希含 Red 空分块）、`multi_part`（9 736 192 B / 53 块）；每例 `hash_matches=true` / `content_matches=true` / `completed=true` |
| `ed2k-mini lan-hello` | 真实 eMule `172.16.29.209:31969`：`OP_HELLO` 104 B → `OP_HELLOANSWER`，nick `https://www.emule-project.org`、`CT_VERSION=0x3c`；`OP_ASKSHAREDFILES` → `OP_ASKSHAREDFILESANSWER`，**共享文件数 0** |
| 公网报文 | **0**；`PROTOCOL_VERSION = 1`、§3 私有唤醒字段与 T0 golden 未动 |

报告：[`ed2k-mini-loopback-20261111023642.json`](reports/ed2k-mini-loopback-20261111023642.json)、
[`ed2k-mini-lan-hello-20261111023647.json`](reports/ed2k-mini-lan-hello-20261111023647.json)。
另修掉 `wire.rs::md4` 中 L1 重构遗留的一组重复 `g` 计算（编译告警），MD4 行为不变，21/21 仍全绿。

### 23.6 证据边界（必须跟结论写在一起）

- **握手是真客户端给的，完整分片传输是我们自己两端给的**，二者不可互相替代。这台 LAN eMule
  共享文件数为 0，能力 ②「从旧客户端下载」**无法对它实测**；M1 须换一台真有共享文件的对端补测，
  并同时实测 ADR-011 / R-15 关注的 credit 缺失排队代价。
- `OP_SETREQFILEID (0x4F)` 对不可能存在的 fileID：aMule `ClientTCPSocket.cpp:425` 规定应回
  `OP_FILEREQANSNOFIL (0x48)`，真实 LAN eMule 在 4 s（另做 12 s 独立复核）内静默。
  这是**源码基线与真实客户端的差异**，原样登记为 `open_observations`，没有改写成通过或失败。
  `OP_REQUESTFILENAME (0x58)` 对未知 fileID 静默则是 aMule 源码的设计行为（`:392` 的 `break`）。
- 源条目是被**同一个进程**发回并取回的，未验证第三方旧客户端 UI 搜索能命中；
  「aMule 自己算出的关键词 Key 与我们一致」仍是源码核实（`KadGetKeywordHash`），不是运行时验证。
- 本轮**没有**公网发布，也没有对全部共享文件做双轨发布；任何公网 `0x43` / `0x44` 发布
  都需执行人**重新显式 attest**（§22.7 第 4 项继续有效）。

### 23.7 下一步（M1 / M4，本轮不做）

1. **M1**：把 `ed2k-mini` 参考实现迁进 `src/crates/cr-ed2k`（**须先过 G0**）；找一台真有共享文件的
   eMule/aMule，实测能力 ② 与 credit 缺失的排队代价（R-15）；复核 §23.6 的 `0x4F` 差异。
2. **M1**：[OPEN-QUESTIONS Q10](../../../docs/06-plan/OPEN-QUESTIONS.md) 定稿
   `WAKEUP_REQ.capabilities.ed2k` 的语义（PROTO §2.2.4 登记的口径冲突，本轮不改 §3 / T0 golden）。
3. **M4 前**：按 D1 对全量共享文件双轨发布，实测拒绝率复审 R-02；补 S8 / R-11 书面法务意见（D4）。
4. 公网发布一律**重新显式 attest**，本轮未做、未自动加该参数。
