# kad-mini — S2 最小 eMule KAD 客户端

> **协议探针，不是产品代码。** 独立 Cargo 工程（项目根仍无 `Cargo.toml`，`src/` 未动）。
> 零第三方依赖，可完全离线构建。实施排期见 [../PLAN.md](../PLAN.md)。

## 1. 它做什么

用**真实的 eMule Kademlia v2 UDP 报文**（`0xE4` 帧头）在 `127.0.0.1` 回环跑通：

```
A(发布者)                 index(索引节点)              B(查询者)
   |-- BOOTSTRAP_REQ ------->|                            |
   |<-- BOOTSTRAP_RES -------|                            |
   |-- HELLO_REQ ----------->|                            |
   |<-- HELLO_RES -----------|                            |
   |                          |<-- BOOTSTRAP/HELLO -------|
   |-- KADEMLIA2_REQ ------->|  (迭代查找 Key 附近节点)    |
   |<-- KADEMLIA2_RES -------|                            |
   |-- PUBLISH_KEY_REQ ----->|  (写入 namespaced Key)     |
   |<-- PUBLISH_RES ---------|                            |
   |                          |<-- SEARCH_KEY_REQ ---------|
   |                          |-- SEARCH_RES ------------>|  命中 A 的描述符
```

每个收发报文都记录时间戳、方向、opcode、完整十六进制，写入上一级
`../reports/`（即 `tests/spikes/s2/reports/`，已由 `.gitignore` 排除）。
报告固定标注 `public_kad_contacted=false` 与 `evidence_valid_for_pass=false`，**不自动置 `pass`**。

## 2. 构建与运行

```bash
cd tests/spikes/s2/kad-mini
cargo test                    # 7 个单测：SHA-1 向量、Tag/Contact/报文轮编、容差语义
cargo run -- selftest         # 37 项协议常量 / 编解码 / zlib / 门禁断言，输出 JSON
cargo run -- loopback         # 真实报文回环闭环，输出 JSON 报告
cargo run -- lan-probe --ip <RFC1918_IP> --port <UDP>   # 真实客户端互操作探测，不发布 Key
cargo run -- live --check --bootstrap <PUBLIC_LIST> --sample-size 3 --rate-limit 0.2 \
                             --attest-i-am-the-experiment-operator   # 只检查实网门禁，不发送任何报文
```

`lan-probe` 硬编码拒绝非私有地址（10/8、172.16/12、192.168/16、127/8），只发
`BOOTSTRAP_REQ` / `HELLO_REQ`，报告固定 `public_kad_contacted=false` 与 `publish_sent=false`。

`loopback` 可选 `--keys N`（默认 5，上限 5）与 `--rate R`（默认 50 包/秒）。

## 3. 协议依据（源码级，非二手资料）

固定来源：aMule `master` @ `6a1105634d81242a581a0f36a7e00379b12a83f3`（2026-06-25）。

| 常量 / 布局 | 值 | 源码路径 |
| :--- | :--- | :--- |
| 帧头 `OP_KADEMLIAHEADER` | `0xE4` | `src/include/protocol/Protocols.h` |
| 压缩帧 `OP_KADEMLIAPACKEDPROT` | `0xE5` + opcode + zlib(payload)，解码见 `src/inflate.rs` | `ClientUDPSocket.cpp` `OP_KADEMLIAPACKEDPROT` 分支（`uncompress(buffer+2, len-2)`） |
| 帧结构 | `[0xE4][opcode][payload]` | `src/kademlia/net/KademliaUDPListener.cpp::ProcessPacket` |
| `KADEMLIA_VERSION` | `0x08` | `src/include/protocol/kad2/Constants.h` |
| `SEARCHTOLERANCE` | `16777216` (2^24) | `src/kademlia/kademlia/Defines.h` |
| `K` / `ALPHA_QUERY` | 10 / 5 | 同上 |
| Kademlia2 opcode 全表 | 见 `src/wire.rs::op` | `src/include/protocol/kad2/Client2Client/UDP.h` |
| Contact 布局（25 字节） | `ID16 + IP4 + UDP2 + TCP2 + Ver1` | `KademliaUDPListener.cpp::Process2BootstrapRequest` |
| Tag 类型 / 名称 | `src/wire.rs::tag_type` | `src/include/tags/TagTypes.h` / `FileTags.h` |
| Tag 编码 | `type[|0x80] + name + value` | `src/Tag.cpp::WriteNewEd2kTag` / `CTag::CTag` |
| Tag 列表计数 | 1 字节 | `src/SafeFile.cpp::WriteTagPtrList` / `ReadTagPtrList` |
| `PUBLISH_KEY_REQ` 布局 | `key16 + count:u16 + (sourceID16 + tagcount:u8 + tags)*` | `Search.cpp::StorePacket` / `Process2PublishKeyRequest` |
| `SEARCH_RES` 布局 | `sender16 + target16 + count:u16 + (answer16 + tagcount:u8 + tags)*` | `Process2SearchResponse` / `ProcessSearchResponse` |
| 发布距离校验 | `top32(XOR) > 2^24 且非 LAN → 静默丢弃` | `Process2PublishKeyRequest` |
| `Get32BitChunk(0)` | 最高 32 位 | `src/kademlia/utils/UInt128.h::Set32BitChunk` 注释 |

> **核查中纠正了一处二手资料错误**：DeepWiki 把 `PUBLISH_KEY_REQ` 写成 `0x35`，
> 但 aMule 源码里 `0x35` 是 `SEARCH_NOTES_REQ`，`PUBLISH_KEY_REQ` 实为 `0x43`。
> 本工程所有常量直接取自源码，不采用二手表格。

## 4. 已知的能力边界（刻意保留）

| 未实现 | 影响 |
| :--- | :--- |
| sender/receiver UDP key、RC4 混淆 | **刻意不实现**：源码核实表明 `0xE4` 明文帧不会被「因未加密」丢弃；key 只用于反 IP 欺骗与「已验证联系人」标记且绑定源 IP。`HELLO_RES` 对 kadVersion ≥ 6 才用 RC4，P4 流程跳过 HELLO |
| `0xE5` zlib 压缩帧解码 | ✅ v2.4.7 已补齐：std-only `src/inflate.rs` 覆盖 stored / fixed / dynamic 三种 DEFLATE 块型并校验 Adler-32；`decode_wire()` 对 `0xE4` / `0xE5` 统一返回 `(opcode, payload)`，压缩帧不再被误记为协议层失败 |
| 完整 routing zone 二叉树 / `nodes.dat` 持久化 / 防火墙检测 | 只有单索引节点，迭代查找只有一跳 |
| `KADEMLIA_FIND_VALUE_MORE` 扩展查询 | 不支持扩大搜索宽度 |
| 真实 KAD 节点身份校验、IP 反欺骗 | 回环环境不需要 |

## 5. 协议与设计基线的错配（v2.4.6 已按项目所有者指示回写）

1. **ID 位宽**：[PROTO §1](../../../../docs/04-protocol/PROTO.md) 原 `Target_Key = SHA1(...)` 是 160-bit，
   eMule KAD 是 128-bit。✅ 已定义 `KAD_ID(x) = x[0..16]`，**截断规则仍标「暂定待 ADR」**。
2. **`FIND_VALUE` 不存在**：判据里的术语在 KAD 里对应
   `SEARCH_KEY_REQ (0x33) → SEARCH_RES (0x3B)`，发布对应 `PUBLISH_KEY_REQ (0x43)`。
   ✅ `S2.criteria` 与 PROTO §2.1 已改。
3. **KAD 不是 KV 存储**：Value 只能是 `(fileID, Tag 列表)`；`Node_Descriptor` 已映射为
   Tag（`ColdRevive_V1:descriptor` / `:namespace`）。✅ PROTO §2 已改。
4. **前缀在线上不可见**：`ColdRevive_V1:` 只影响本地哈希输入，线上只有 128-bit ID。
   ✅ 观测口径已改为「不同哈希空间不互相覆盖」。
5. **发布必须先收敛到 tolerance zone**：回环靠 LAN 豁免通过；实测 5 个 Key 的
   `xor_top32` 全部远超 2^24，在 WAN 上会被静默丢弃。✅ 已写入 `S2.criteria` 第 2 条
   （发布前 `KADEMLIA2_REQ/RES` 多跳迭代）。
6. **明文 / UDP key / RC4（新增核实）**：`ClientUDPSocket.cpp::DecryptReceivedClient` 对首字节
   `0xE4` 直接放行；`PacketTracking.cpp::InTrackListIsAllowedPacket` 的 key 形参是
   `bool /*bValidSenderkey*/`，实际未启用。`CKadUDPKey` 为 `(m_key, m_ip)`，
   `GetKeyValue(myIP)` 只在 IP 匹配时返回，**绑定原机器公网 IP、不能从 `nodes.dat` 复制**。
   `BOOTSTRAP_RES` / `KADEMLIA2_RES` / `PUBLISH_RES` / `SEARCH_RES` 的发送端都传
   `cryptTargetID = NULL`（明文）；只有 `SendMyDetails`（`HELLO_RES`）对 kadVersion ≥ 6 用 RC4。
   → **结论：保持明文、跳过 HELLO，不实现 key / RC4**。

## 6. 实网门禁

`live` 模式在发任何非回环报文前，要求全部通过：

`spikes.json` 的 `approved=true`、`pending` 已清除、`confirmed.sample_size` / `rate_limit_pps` 存在 ·
用户提供 `--bootstrap FILE`（仓库不内置任何节点） · `--sample-size` / `--rate-limit`
**必须与 G0 确认值逐项一致**（当前 3 / 0.2） · 硬上限 1..=10 与 (0, 1.0] 包/秒 ·
`--attest-i-am-the-experiment-operator` · bootstrap 清单必须**实际解析为公网可路由 IPv4**
（v2.4.7：拒绝 RFC1918 / loopback / link-local / CGNAT / 组播 / 文档保留段，并拒绝
`lan-interop-probe-only` 文件）。

P4 发送前还会先用 `KADEMLIA2_REQ/RES` 最多 3 跳迭代收敛到 `SEARCHTOLERANCE` 邻域；
找不到邻域节点就跳过该样本不发布。`live` 不发 `HELLO_REQ`，socket 用 `bind_any()`。

P3 已完成，因此带齐参数 + 公网清单运行 `live --check` 会返回 `gate.passed=true`（**仍不发包**）；
LAN 清单或私有 IP 清单会被拦截（v2.4.7 实测 `gate.passed=false`、退出码 2）；
只有去掉 `--check` 才会真正执行 P4。详见 [../PLAN.md §2/§3](../PLAN.md)。

> **⚠️ v2.4.14 更正（下方 P4 首次执行与 v2.4.9–v2.4.13 段落为已作废的历史归因）**：
> `Contact::encode/decode` 曾按点分顺序读取 IP，但 KAD 线上 4 字节是**反序**（aMule
> host-order 小端），导致所有公网目标地址错误。现已修复并加
> `contact_ip_wire_order_is_reversed` 单测。修正后本机 10 目标探针 **9/10 收到 523 B
> `BOOTSTRAP_RES`**；P4 按 G0 参数两度重跑均已 `public_kad_contacted=true`，收到 `0xE5`
> 压缩 `BOOTSTRAP_RES` 与 `0x21/0x29` 迭代应答，但每跳 2 候选 × 3 跳仅 30–34 联系人、
> `tolerance_reached=false`，3/3 样本跳过发布。**高位 UDP 一直可用，不需换网。**
> 下一步：每跳 `ALPHA_QUERY=5` 个候选、`KADEMLIA_FIND_NODE=0x0B`、提高跳数上限，
> 用实时 `BOOTSTRAP_RES` 目标清单重跑 P4。详见 [PLAN.md §11](../PLAN.md)。

**P4 首次执行（2026-11-10）**：门禁通过、3 条 `0xE4 0x01` `BOOTSTRAP_REQ` 以 5 s 间隔发出、
0 响应、0 发布、3 个样本全部 `skipped`（未收敛到 tolerance zone）。对照实验确认是执行环境
过滤公网高位 UDP，而非报文格式或节点拒绝；LAN 真实 eMule 对同一帧正常回 523 B / 20 contacts。
结论 `inconclusive_environment_blocked`，`S2.status` 保持 `not_run`。重跑前先执行
[`udp_egress_check.py`](../udp_egress_check.py)（v2.4.9 升级，退出码 0 才跑）。
详见 [PLAN.md §7/§8.6/§9](../PLAN.md)：LAN 探测不经过公网出口、不能证明公网可达；v2.4.10 的
跨主机 **STUN** 对照在 LAN eMule 机器上同样 `BLOCKED`，但 v2.4.11 指出 STUN 不等于 KAD ——
新增 [`kad_egress_probe.py`](../kad_egress_probe.py) 用真实 `0xE4 0x01` 复测，本机 0/3 确证
不可达。v2.4.12 该机 `Kad 网络` 面板两行均「打开」否证 firewalled 假设，但 aMule 源码显示
「打开」是入站 DNAT 即可满足的入站测试，仍不证明该机可跑 P4；须在该机跑该探针并核对 KAD
收发包计数，详见 §9。**v2.4.13**：该机探针已跑出 0/3 / 退出码 2，本机另用
[`bootstrap_res_to_targets.py`](../bootstrap_res_to_targets.py) 从新鲜 LAN `BOOTSTRAP_RES`
生成的 10 个公网联系人宽扫 0/10；按 eMule 官方帮助，「下载/搜索正常」不等于 KAD 出站可用
（下载走 TCP、默认搜索可为 eD2k 服务器搜索）。工程决策为换网重跑 P4，详见
[PLAN.md §10](../PLAN.md)。

## 7. LAN 真实客户端互操作探测（2026-11-09）

`lan-probe` 对 RFC1918 内的真实 eMule 客户端（IP / 端口由 `--ip` / `--port` 传入，不写进仓库）只发两个握手包：

| 探测 | 结果 |
| :--- | :--- |
| `BOOTSTRAP_REQ (0x01)` | 一次 `0xE4 0x09` 明文 523 B（RTT ≈ 13 ms），另一次 `0xE5 0x09` zlib 压缩 519 B；节点可达、KAD 在线，响应形态不稳定 |
| `HELLO_REQ (0x11)`，v8 / v7 | 均无响应（超时） |

这直接支持第 6 条的决策：明文能被接受，问题在压缩帧与 HELLO 路径，而非 UDP key。
v2.4.7 复验：新探测解出 `0xE4` 明文的 521 B payload / **20 个联系人**；
此前捕获的真实 519 B `0xE5` 帧也经 `inflate.rs` 解出同样的 521 B payload（与 Python
`zlib.decompress` 一致），证明压缩帧已可正确归类，不再污染 RISK R-02 的三桶计数。
报告落在 `../reports/kad-lan-probe-*.json`（已忽略），固定
`public_kad_contacted=false`、`evidence_valid_for_pass=false`、`publish_sent=false`。
LAN 节点**不是公共 KAD 证据**，也禁止在其上发布测试 Key。
