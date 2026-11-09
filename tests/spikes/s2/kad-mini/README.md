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
cargo run -- selftest         # 29 项协议常量与编解码断言，输出 JSON
cargo run -- loopback         # 真实报文回环闭环，输出 JSON 报告
cargo run -- live --check     # 只检查实网门禁，不发送任何报文（当前退出码 2）
```

`loopback` 可选 `--keys N`（默认 5，上限 5）与 `--rate R`（默认 50 包/秒）。

## 3. 协议依据（源码级，非二手资料）

固定来源：aMule `master` @ `6a1105634d81242a581a0f36a7e00379b12a83f3`（2026-06-25）。

| 常量 / 布局 | 值 | 源码路径 |
| :--- | :--- | :--- |
| 帧头 `OP_KADEMLIAHEADER` | `0xE4` | `src/include/protocol/Protocols.h` |
| 压缩帧 `OP_KADEMLIAPACKEDPROT` | `0xE5`（本探针不支持） | 同上 |
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
| sender/receiver UDP key、RC4 混淆 | aMule ≥ v6 节点可能静默丢弃明文包 —— 这正是 S2-C2 待观测项 |
| `0xE5` zlib 压缩帧 | >200 字节的响应可能无法解析；本探针报文均 <200 字节 |
| 完整 routing zone 二叉树 / `nodes.dat` / 防火墙检测 | 只有单索引节点，迭代查找只有一跳 |
| `KADEMLIA_FIND_VALUE_MORE` 扩展查询 | 不支持扩大搜索宽度 |
| 真实 KAD 节点身份校验、IP 反欺骗 | 回环环境不需要 |

## 5. 协议与设计基线的错配（发现，不擅自改）

1. **ID 位宽**：[PROTO §1](../../../../docs/04-protocol/PROTO.md) 的 `Target_Key = SHA1(...)` 是 160-bit，
   eMule KAD 是 128-bit。本探针取 SHA-1 前 16 字节，**截断规则仍待 ADR 决策**。
2. **`FIND_VALUE` 不存在**：判据里的术语在 KAD 里对应
   `SEARCH_KEY_REQ (0x33) → SEARCH_RES (0x3B)`，发布对应 `PUBLISH_KEY_REQ (0x43)`。
3. **KAD 不是 KV 存储**：Value 只能是 `(fileID, Tag 列表)`；`Node_Descriptor` 已映射为
   Tag（`ColdRevive_V1:descriptor` / `:namespace`），符合 ADR-006「不写种子本体」。
4. **前缀在线上不可见**：`ColdRevive_V1:` 只影响本地哈希输入，线上只有 128-bit ID。
   因此「公共节点因前缀而拒绝」这一观测口径不成立，S2-C3 应改为「不同哈希空间不互相覆盖」。
5. **发布必须先收敛到 tolerance zone**：回环靠 LAN 豁免通过；实测 5 个 Key 的
   `xor_top32` 全部远超 2^24，在 WAN 上会被静默丢弃。实网必须先做多跳迭代查找，
   找到 ID 落在 Key 邻域的节点再发布 —— 这是 P4 的关键步骤。

## 6. 实网门禁

`live` 模式在发任何非回环报文前，要求全部通过：

`spikes.json` 的 `approved=true` 且 `pending` 已清除 · 用户提供 `--bootstrap FILE`（仓库不内置任何节点）·
`--sample-size 1..=10` · `--rate-limit (0, 1.0]` 包/秒 · `--attest-i-am-the-experiment-operator`。

当前 `live --check` 会拒绝并列出 5 项阻塞。详见 [../PLAN.md §2/§3](../PLAN.md)。
