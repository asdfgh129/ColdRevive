---
doc_id: DOC-09
title: 协议规范（PROTO）
category: 协议
version: 2.5
status: 待评审
owner: 架构组 / 内核组
updated: 2026-11-11
---

# 协议规范（PROTO）

> 编号 DOC-09 · 版本 2.5 · 状态 待评审 · 责任人 架构组 / 内核组 · 更新 2026-11-11
> **本协议是私有控制面契约**。数据面 100% 使用标准 BT / eD2K 协议，不受本文约束。
> 任何字段变更必须递增 `PROTOCOL_VERSION` 并新增 [ADR](../02-architecture/adr/README.md)，禁止静默修改。
> **v2.1 只修正 §1/§2 的 eMule KAD 集成映射（外部协议映射）**，§3 私有唤醒报文的字段一字未改，
> 故 `PROTOCOL_VERSION` 保持 1、T0 golden files 不变；依据见 [§2.0](#20-kad-集成映射基线2026-11-09-源码核实)。
> **v2.3 修正 KAD tag 帧头（§2.0 末条）**：aMule 的 Kad 路径用 `CFileDataIO::ReadTag`，
> 名字恒为 `u16 长度 + 字节`，且不认 `TAGTYPE_STR1..STR22` 与 `TAGTYPE_BLOB`；按 ed2k/met
> 的 `[type|0x80][name:1]` 编码会被整包拒绝。源码核实 + 局域网真实 aMule 实测，见 v2.4.26。
> **v2.4 新增双轨 Key（[ADR-011](../02-architecture/adr/ADR-011-ed2k-interop.md)）**：兼容轨的关键词 Key
> 是 `MD4(UTF8(term))`，**不加命名空间前缀**；私有轨仍用 §1 的 `Keyword_Key`。只改 §1/§2 的外部
> KAD 集成映射，§3 `WAKEUP_REQ` 字段一字未动，故 `PROTOCOL_VERSION` 保持 1、T0 golden 不变。
> **v2.5 补齐 L2 的源字典与 eD2k TCP 数据面映射（§2.2）**：`0x44` / `0x34` 载荷、标准
> eD2k 客户端间 TCP 报文序列，并**更正 `Emule_File_Key` 的分块边界规则**（aMule 用 Red 变体）。
> 仍只改 §1/§2 的外部协议映射，§3 私有唤醒字段一字未动，`PROTOCOL_VERSION = 1` 与 T0 golden 不变。

## 1. 命名空间与常量

```
NAMESPACE_PREFIX = "ColdRevive_V1:"     # 只作用于本地哈希输入，线上不可见
KAD_ID_LEN       = 16                   # 128 bit，eMule KAD ID 空间（不是 SHA-1 的 160 bit）
KAD_ID(x)        = x[0..16]             # 暂定截断规则，待 ADR 定稿，见 §2.0
Target_Key       = KAD_ID(SHA1(NAMESPACE_PREFIX + file_hash))
Keyword_Key      = KAD_ID(SHA1(NAMESPACE_PREFIX + "kw:" + normalized_term))
Emule_Keyword_Key = MD4(UTF8(normalized_term))  # 无前缀，eMule 关键词字典（ADR-011 兼容轨）
Emule_File_Key    = ed2k_hash                  # MD4 over 9.28 MB 分块哈希，必须由真实内容导出；
                                             # 分块整数倍边界按 aMule Red 规则补空分块，见 §2.2.3
Node_Descriptor  = { ip, wake_port, bt_port, proto_flags, infohash_prefix,
                     version, timestamp, signature }   # 发布时编码为 KAD Tag 列表
PROTOCOL_VERSION = 1                    # 私有控制面版本，本次不变
```

> 版本前缀写进 Key，未来协议不兼容时可通过 `V2` 平滑隔离，不污染旧索引。
> **前缀不是线上字符串**：公共节点只看到 128-bit ID，「命名空间隔离」由哈希输入实现，
> 不能假设节点会识别或拒绝 `ColdRevive_V1:` 字面量（§2.0）。
> `node_secret_seed` 为节点本地随机种子，仅用于 Challenge-Response 签名，不参与跨节点身份体系。
>
> **兼容轨与私有轨是两套哈希，不得复用函数**（源码依据：`Kademlia.cpp::KadGetKeywordHash`
> 用 `CryptoPP::Weak::MD4` 对 `unicode2UTF8(term)` 求摘要，`CUInt128::SetValueBE` 写入；
> `UInt128.cpp::ToByteArray` 是其逆运算，故线上 16 字节即 MD4 摘要原值）。该函数**不做**小写化、
> trim 或 Unicode 归一化，归一化由调用方负责。兼容轨 `fileID` 必须是真实 `ed2k_hash`，
> 否则旧客户端搜到的是永远下不动的死条目；双轨发布使发布量翻倍，须按 [RISK R-02](../05-quality/RISK.md) 复审。

## 2. KAD 发布 / 查询规范

### 2.0 KAD 集成映射基线（2026-11-09 源码核实）

固定依据：aMule `master` @ `6a1105634d81242a581a0f36a7e00379b12a83f3`。逐条出处与探针实现见
[tests/spikes/s2/kad-mini/README.md](../../tests/spikes/s2/kad-mini/README.md) 与
[S2 排期](../../tests/spikes/s2/PLAN.md)。**不引用二手资料的 opcode 表**（核查中发现
DeepWiki 把 `PUBLISH_KEY_REQ` 误记为 `0x35`，源码中 `0x35` 实为 `SEARCH_NOTES_REQ`）。

| 设计文档旧表述 | 真实 eMule Kademlia v2 | 本规范采用 |
| :--- | :--- | :--- |
| `FIND_VALUE` | **无此操作**。关键词查询是 `KADEMLIA2_SEARCH_KEY_REQ (0x33)` → `KADEMLIA2_SEARCH_RES (0x3B)` | 全文以 `SEARCH_KEY` 取代 `FIND_VALUE` |
| `Target_Key = SHA1(...)` | KAD ID 空间 **128-bit**，帧头 `0xE4`，Kademlia2 版本 `0x08` | `SHA1` 前 16 字节；截断规则待 ADR 定稿 |
| KAD = 通用 KV | `PUBLISH_KEY_REQ (0x43)` 存的是 `(fileID/sourceID, Tag 列表)` | `Node_Descriptor` 编码为命名空间 Tag |
| 前缀可线上识别 | 线上只有 128-bit ID | 隔离 = 不同哈希空间不互相覆盖 |
| 发布到 bootstrap 即可 | `Process2PublishKeyRequest` 有 `SEARCHTOLERANCE = 2^24`，越界**静默丢弃** | 发布前必须 `KADEMLIA2_REQ/RES (0x21/0x29)` 多跳迭代收敛 |
| 查询必有响应 | **索引里没有该 Key 时不回应**：`CIndexed::SendValidKeywordResult` 整个响应构造都在 `if (m_Keyword_map.find(keyID) != end())` 内，miss 只走末尾的 `Clean()` | 客户端必须把 `SEARCH_RES` 超时视为「无结果」，且**不可区分**「无人持有」与「对方不服务」；查询需多目标冗余 |
| 发布必有响应 | **三条静默丢弃**：① 对端自身 UDP firewalled（`IsFirewalledUDP(true)` → `return`，**无 LAN 豁免**）② `top_chunk(对端 KadID, Key) > SEARCHTOLERANCE`（LAN 源地址豁免）③ entry 构造里 `ReadTag` 抛异常，被 `catch(...) { delete entry; throw; }` 原样抛出，`PUBLISH_RES` 那一行永远到不了 | 发布只能发往「距离在区内」的节点，且必须多目标扇出；tag 帧头必须按 Kad 格式编码（下条），否则任何节点都不会回包 |
| Tag 编码 `type[\|0x80] + name + value` | Kad 路径是 `type + u16(name_len) + name + value`，数值名字就是长度 1 的字符串（`TAG_FILENAME = wxT("\x01")`）；`ReadTag` 的 switch 里没有 `STRn` / `BLOB`，命中 default 即 `throw` | 所有字符串一律 `TAGTYPE_STRING (0x02)`，禁止 `STRn`；`0x80` 短名形式只属于 ed2k/met 的 `CTag::CTag`，Kad 上无效 |
| 索引操作需先 `HELLO` 互认 | **不成立**（v2.4.22 源码核实）：`Process2SearchKeyRequest` / `Process2PublishKeyRequest` 均不查询路由表、不校验发送方是否为已知 contact | v1 继续跳过 `HELLO`；索引可达性不依赖对等体互认 |
| 响应都是明文 `0xE4` | >200 字节的响应改用 `0xE5` + zlib 压缩帧；`HELLO_RES` 对 kadVersion ≥ 6 可能 RC4 混淆 | 客户端须能分类 `0xE4` / `0xE5` / 非 KAD 帧，并分开计数 |

> **tag 帧头是第三条静默路径，也是最容易自证清白的一条**：`Process2PublishKeyRequest`
> 把 entry 构造整个包在 `try/catch(...)` 里，`ReadTag` 一抛就把异常原样抛出去，
> 对端看到的是「完全无响应」，与「无人持有」「对端 firewalled」在线上不可区分。
> 本项目五轮公网 PUBLISH 全部 0 回应，正是这条路径造成的——离线"逐字节自洽"当时对照的是
> `CTag::CTag(CFileDataIO&, bool)`（`.met` 路径），而不是 Kad 路径的 `CFileDataIO::ReadTag`，
> 于是编码器验证编码器，自己给自己发了合格证。详见 [PLAN §19](../../tests/spikes/s2/PLAN.md)。

`sender/receiver UDP key` 的作用是**反 IP 欺骗与标记联系人已验证**，不是「明文包一律丢弃」的加密门禁：
`ClientUDPSocket.cpp` 对首字节 `0xE4` 直接放行，`PacketTracking.cpp` 的 key 形参实际未启用。
该 key 形如 `(m_key, m_ip)` 且 `GetKeyValue(myIP)` 仅在 `myIP == m_ip` 时返回，**绑定原机器公网 IP，
不可从 `nodes.dat` 复制到别的主机使用**。因此 v1 不实现它，也不实现 RC4；实网小样本跳过 `HELLO` 路径
（`BOOTSTRAP_RES` / `KADEMLIA2_RES` / `PUBLISH_RES` / `SEARCH_RES` 的发送端均传 `cryptTargetID = NULL`）。

| 项 | 规定 |
| :--- | :--- |
| Key | **双轨**：兼容轨 `Emule_Keyword_Key`（无前缀 MD4）+ 私有轨 §1 命名空间 Key；见 §1 与 [ADR-011](../02-architecture/adr/ADR-011-ed2k-interop.md) |
| Value | **仅轻量节点描述符**（IP、端口、协议标识、InfoHash 前缀、时间戳、签名），编码为 KAD Tag 列表；**不存放 `.torrent` 本体**（[ADR-006](../02-architecture/adr/ADR-006-metadata-distribution.md)） |
| 发布流程 | `BOOTSTRAP_REQ/RES` → `KADEMLIA2_REQ/RES` 迭代收敛 → `PUBLISH_KEY_REQ/RES`；查询为 `SEARCH_KEY_REQ/RES` |
| 元数据获取 | 唤醒响应返回 `torrent_url`（本机 HTTP `/torrent/<infohash>`），或使用 magnet 的 `xs=` 参数 |
| 发布频率 | TTL 30 min 续期；产品侧全量启动分批限速 20/s（[ADR-001](../02-architecture/adr/ADR-001-kad-network.md)）。**M0 实网实验另按 G0 确认的 0.2 包/秒执行**，二者不可混淆 |
| 关键词上限 | 每文件 ≤ 8 个 Key，避免 KAD 存储与查询负担 |
| 撤销 | 文件移出共享目录时发布 `TOMBSTONE` 并等待 TTL 自然过期 |
| 结果处理 | 去重 → 黑名单过滤 → 信誉排序 → 上限 200 条 → 逐条尝试唤醒 |

### 2.1 查询与降级顺序

1. 关键词 → 私有轨多个 `Keyword_Key` **与**兼容轨 `Emule_Keyword_Key` → KAD `SEARCH_KEY_REQ` 迭代查找，两轨结果合并去重
2. 精确 `Target_Key` 查询（已知 `file_hash` 时）
3. 无结果 → Mainline DHT 按 InfoHash 精确查
4. 仍无结果 → 可选 Tracker announce
5. 最后回退本地索引（自建节点自查）

> KAD 关键词为**精确哈希匹配**，不支持通配符；分词与模糊重排策略见 [FLOWS §2](../03-design/FLOWS.md)。
> 兼容轨结果转标准 `ed2k://|file|<name>|<size>|<hash>|/` 链接并可交由任意旧客户端下载（FR-24）；
> 自有客户端另用私有轨拿唤醒端口与 BT 通道。

### 2.2 L2 互操作映射基线（2026-11-11 源码核实，ADR-011）

固定依据仍是 aMule `master` @ `6a1105634d81242a581a0f36a7e00379b12a83f3`。
探针实现与实测见 [tests/spikes/s2/ed2k-mini/README.md](../../tests/spikes/s2/ed2k-mini/README.md)、
[kad-mini README](../../tests/spikes/s2/kad-mini/README.md) 与 [S2 PLAN §23](../../tests/spikes/s2/PLAN.md)。
本节全部是**外部协议映射**，不新增私有报文。

#### 2.2.1 源字典：`PUBLISH_SOURCE_REQ (0x44)` 与 `SEARCH_SOURCE_REQ (0x34)`

关键词条目（`0x43`）只让旧客户端**搜到**文件；要能**下载**，同一 `fileID` 还必须在
KAD 源字典里有源条目。两者是两笔独立发布，缺一不可。

```
PUBLISH_SOURCE_REQ (0x44) = <fileID 16><sourceID 16><tagcount u8><Kad ReadTag tags...>
SEARCH_SOURCE_REQ  (0x34) = <fileID 16><startPosition u16><fileSize u64>
PUBLISH_RES        (0x4B) = <fileID 16><load u8>
SEARCH_RES         (0x3B) = <senderID 16><target 16><count u16>[<answer 16><tagcount u8><tags>]*  # 与 §2.0 同构
```

| 约束 | 源码依据 | 违反后果 |
| :--- | :--- | :--- |
| 载荷**没有** `u16` 条目数，只带**一个**源 | `KademliaUDPListener.cpp::SendPublishSourcePacket` 只写 `WriteUInt128(targetID)` + `WriteUInt128(contactID)` + `WriteTagPtrList(tags)`；`Process2PublishSourceRequest` 相应地只读两个 ID | 解析错位，整包丢弃 |
| tag 数是 **1 字节**，不是 `0x43` 里的 2 字节 | `CFileDataIO::WriteTagPtrList` 写 `WriteUInt8(count)`；处理器 `uint32 tags = bio.ReadUInt8()` | 同上 |
| `SEARCHTOLERANCE` 校验用 **`fileID`**（不是 keyword key） | `distance = GetKadID() ^ file; if (distance.Get32BitChunk(0) > SEARCHTOLERANCE && !IsLanIP(ip)) return;` | 发错节点，静默丢弃 |
| 必须有 `TAG_SOURCETYPE (0xFF)` tag | 处理器只在命中该 tag 时置 `m_bSource = true`；否则 `delete entry`，**不回 `PUBLISH_RES`** | 无响应，与 firewalled / 越界不可区分 |
| 源 IP / UDP 端口取自 **UDP 报头**，不由 tag 提供 | `entry->m_uIP = ip; entry->m_uUDPport = port;`；命中 `TAG_SOURCETYPE` 时接收方自行补 `TAG_SOURCEIP` | 伪造 tag 无效 |
| `CIndexed::AddSources` 还要 `TCPport != 0 && UDPport != 0 && tagcount != 0` | `Indexed.cpp::AddSources` | 静默拒绝 |
| `0x34` 的 `startPosition` 按 `& 0x7FFF` 掩码 | `Process2SearchSourceRequest` | 高位被解释为分页 |

非防火墙（HighID）源的标准 tag 集（`Search.cpp` case `STOREFILE` 的 `!IsFirewalled()` 分支）：
`TAG_SOURCETYPE=1`、`TAG_SOURCEPORT (0xFD)=TCP 端口`、
`TAG_SOURCEUPORT (0xFC)=KAD UDP 端口`、`TAG_FILESIZE (0x02)`、`TAG_ENCRYPTION (0xF3)=0`。
全部按 §2.0 末条的 Kad `ReadTag` 格式编码。

#### 2.2.2 eD2k 客户端间 TCP 数据面

| 项 | 规定 |
| :--- | :--- |
| 帧头 | `<protocol u8><packetlength u32 LE><opcode u8><payload>`；`packetlength = 1 + payload.len()`（`OtherStructs.h::Header_Struct` / `Packet.cpp::GetPacketSizeFromHeader`） |
| 协议字节 | `0xE3` 标准 eDonkey、`0xC5` 扩展 eMule、`0xD4` 压缩帧（`Protocols.h`） |
| 握手 | `OP_HELLO (0x01)` → `OP_HELLOANSWER (0x4C)`。`HELLO` 载荷 = `<0x10><userhash 16><clientID u32><port u16><tagcount u32><tags><serverIP u32><serverPort u16>`；`HELLOANSWER` **没有**开头的 `0x10`（`BaseClient.cpp::SendHelloPacket` / `SendHelloAnswer` / `ProcessHelloPacket`） |
| 握手 tag | `CTag::CTag(CFileDataIO&)` 接受 `[type|0x80][name:1]` 与 `[type][u16 name_len][name]` 两种；`CTag::WriteTagToFile` → `CFileDataIO::WriteTag` 实际写后者，即**与 KAD `ReadTag` 相同的布局**。KAD 路径禁用紧凑形式，反之 eD2k 路径可读两种 |
| 文件请求 | `OP_REQUESTFILENAME (0x58) <hash 16>` → `OP_REQFILENAMEANSWER (0x59) <hash 16><u16 name_len><name>`。未知 fileID 时 `0x58` **静默 break**（`ClientTCPSocket.cpp:392`）；`OP_SETREQFILEID (0x4F)` 才回 `OP_FILEREQANSNOFIL (0x48)` |
| hashset | 文件 ≥ `PARTSIZE` 时 `OP_HASHSETREQUEST (0x51)` → `OP_HASHSETANSWER (0x52) = <hash 16><count u16><partHash*16>` |
| 上传队列 | `OP_STARTUPLOADREQ (0x54)` → `OP_ACCEPTUPLOADREQ (0x55)` 或 `OP_QUEUERANKING (0xC5/0x60, 恒 12 B)`；旧客户端可能发 `OP_QUEUERANK (0x5C, u32)`，两者都要处理 |
| 分片请求 | `OP_REQUESTPARTS (0x47)` = `<hash 16><start[0..3] u32><end[0..3] u32>`，**先 3 个 start 再 3 个 end，恒 3 组、不足补 0**；不是交错的 `<start,end>` 对（`DownloadClient.cpp::SendBlockRequests` / `UploadClient.cpp::ProcessRequestPartsPacket`） |
| 分片应答 | `OP_SENDINGPART (0x46)` = `<hash 16><start u32><end u32><data[end-start]>`（`UploadDiskIOThread.cpp::CreateNormalPackets`）；`BLOCKSIZE = 184320`、`PARTSIZE = 9728000`（`ed2k/Constants.h`） |
| 收尾 | `OP_END_OF_DOWNLOAD (0x49)` / `OP_CANCELTRANSFER (0x56)` |

> **实现口径**：v1 最小栈只做上述标准报文，不实现压缩 `0x40`、混淆、积分 / credit / 安全标识、
> 源交换、AICH 与 >4 GiB 的 I64 分片（`0xA2` / `0xA3`）。最小上传队列属传输栈、必须实现；
> 完整 eMule 信用体系仍属 [PRD 非目标](../01-product/PRD.md)，M1 实测排队代价后再定（ADR-011）。

#### 2.2.3 `ed2k_hash` 的分块边界规则（**更正 §1**）

`Emule_File_Key` 必须与真实 eMule/aMule 客户端**逐字节一致**，否则旧客户端搜到的是
永远下不动的死条目。文件大小恰为 `PARTSIZE` 整数倍时存在两套互不兼容的算法：

```
size < PARTSIZE           : ed2k_hash = MD4(file)                      # 无 hashset
size >= PARTSIZE          : ed2k_hash = MD4(concat(MD4(each 9.28 MB part)))
size % PARTSIZE == 0      : 额外追加 MD4("") = 31D6CFE0D16AE931B73C59D7E0C089C0   # aMule "Red"
```

aMule 采用 **Red**：`KnownFile.cpp::SetFileSize` 给出样本（1×PARTSIZE 文件的文件哈希
`A72CA8DF7F07154E217C236C89C17619`，哈希表第二项即空分块哈希）；
`ThreadTasks.cpp::CreateNextPartHash` 在 `partLength == PARTSIZE && file.Eof()` 时显式
`push_back(g_emptyMD4Hash)`；`CreateHashFromHashlist` 对整表做 MD4。
即 `m_iED2KPartHashCount = size/PARTSIZE; if != 0 { +1 }`。

> ⚠️ crates.io `ed2k::Ed2k` 默认别名是 **Blue**（不补空分块）并在文档里称其为「official」。
> **直接复用该默认值会生成与 eMule 不同的 `fileID`**；`Ed2kRed` 才与旧客户端一致。
> 本项目在 `kad-mini/src/wire.rs` 自实现并已按 Red 修正，用上游自己的已知答案向量做回归。
> 详见 [OPEN-QUESTIONS Q9 / V-04](../06-plan/OPEN-QUESTIONS.md) 与 [PLAN §23.2](../../tests/spikes/s2/PLAN.md)。

#### 2.2.4 与 §3 私有唤醒契约的口径冲突（**待 M1 定稿，本轮不改**）

ADR-011 已把标准 eD2k 传输栈排入 v1，而 [ADR-004](../02-architecture/adr/ADR-004-ed2k-scope.md)
当初规定 §3.2 `WAKEUP_REQ.capabilities.ed2k` 在 v1 **固定 `false`**，T0 golden 也断言该值为 `false`。
两者字面冲突。改 §3 字段须按 §4 递增 `PROTOCOL_VERSION` 并重做 golden，代价远大于收益：
该字段位于**私有控制面**，而 ADR-011 的 eD2k 是**公共数据面**，二者本就不是同一能力。
本轮保持 `PROTOCOL_VERSION = 1`、§3 与 T0 golden 一字不动，把「明确该字段语义为
*私有唤醒面是否启用 eD2k*」登记为 [OPEN-QUESTIONS Q10](../06-plan/OPEN-QUESTIONS.md)，M1 定稿。

## 3. 私有唤醒协议（PROTOCOL_VERSION = 1）

### 3.1 传输

- UDP 单包请求/响应（轻量、无连接、便于打洞）。
- 大响应（`.torrent` 本体）一律改走 HTTP 接口；UDP 只回 URL 与摘要。
- 请求方 1.5 s 超时重试一次，随后切换候选节点，避免 TCP 长超时。

### 3.2 `WAKEUP_REQ`

```json
{
  "v": 1,
  "type": "WAKEUP_REQ",
  "trace_id": "uuid",
  "file_hash": "sha256...",
  "infohash": "optional-bt-infohash",
  "nonce": "random-16b",
  "ts": 1762650000,
  "challenge": "hmac(...)",
  "client_version": "coldrevive/0.1.0",
  "capabilities": { "bt": true, "utp": true, "ed2k": false }
}
```

- `challenge = HMAC(node_secret_seed, file_hash || utc_date || nonce)`
- 接收方校验时间窗 ±5 min、nonce 去重、文件在共享白名单内。
- `capabilities.ed2k` 在 v1 固定为 `false`（[ADR-004](../02-architecture/adr/ADR-004-ed2k-scope.md)）。

### 3.3 `WAKEUP_ACK`

```json
{
  "v": 1,
  "type": "WAKEUP_ACK",
  "trace_id": "uuid",
  "status": "OK",
  "magnet": "magnet:?xt=urn:btih:...&xs=http%3A%2F%2F...",
  "torrent_url": "http://<node>:16800/torrent/<infohash>",
  "file_size": 123456789,
  "piece_size": 1048576,
  "ttl": 7200,
  "bt_port": 16802
}
```

请求方必须校验 `magnet` 中的 InfoHash 与请求一致，不一致按 `HASH_MISMATCH` 处理并拉黑节点。

### 3.4 错误码

| 码 | 含义 | 请求方动作 |
| :--- | :--- | :--- |
| `REJECT_NOT_FOUND` | 文件不在共享白名单 | 换节点，不拉黑 |
| `REJECT_FILE_GONE` | 索引命中但文件已删除/改名 | 换节点；接收方须撤销该 Key 的 KAD 发布 |
| `REJECT_BAD_CHALLENGE` | 签名/时间窗/nonce 校验失败 | 重试一次后放弃 |
| `REJECT_RATE_LIMITED` | 触发限流 | 按 `retry_after` 退避 |
| `REJECT_BUSY` | 活跃池/并发已满 | 退避后重试或换节点 |
| `REJECT_RESOURCE` | 内存/带宽越限 | 换节点 |
| `REJECT_VERSION` | 协议版本不兼容 | 换节点 |
| `HASH_MISMATCH` | 请求方侧元数据校验失败 | **拉黑 + 信誉扣分** |

错误码落库为 [DATA `wake_log.result`](../03-design/DATA.md)，异常路径见 [FLOWS §8](../03-design/FLOWS.md)。

## 4. 版本与兼容

| 规则 | 说明 |
| :--- | :--- |
| 版本字段 | 所有报文带 `v`；不兼容版本直接 `REJECT_VERSION` |
| 命名空间隔离 | 协议不兼容时 Key 前缀递增为 `V2`，新旧并存一个 TTL 周期 |
| 演进顺序 | 新增 ADR → 递增 `PROTOCOL_VERSION` → 更新本文 → 更新契约测试 golden files |
| 映射修订例外 | 只改 §1/§2 的**外部 KAD 集成映射**、不动 §3 `WAKEUP_REQ` / `WAKEUP_ACK` 字段时，递增本文 `version`（2.0 → 2.1）即可，`PROTOCOL_VERSION` 与 T0 golden 保持不变；须在本文记录源码依据 |
| 双轨 Key（v2.4） | 新增 `Emule_Keyword_Key = MD4(UTF8(term))` 属 §1/§2 外部 KAD 集成映射，按上条「映射修订例外」只递增本文 `version`（2.3 → 2.4），`PROTOCOL_VERSION` 与 T0 golden 保持不变；依据见 [ADR-011](../02-architecture/adr/ADR-011-ed2k-interop.md) |
| L2 映射（v2.5） | §2.2 的 `0x44` / `0x34` 载荷、eD2k TCP 报文序列与 `ed2k_hash` Red 规则同属外部协议映射，按「映射修订例外」只递增本文 `version`（2.4 → 2.5）；`PROTOCOL_VERSION`、§3 字段与 T0 golden 均不变 |
| 禁止事项 | 禁止静默改字段、禁止复用旧错误码语义、禁止在 Value 中塞种子本体 |

## 5. 安全约束

- 所有外部输入（DHT 返回值、唤醒报文、种子元数据）必须校验后才采信。
- 限流默认：单 IP 5 次/分钟、全局并发 8、单文件并发 3。
- `REJECT_NOT_FOUND` 用于非白名单文件，避免成为文件存在性探测接口。
- 详见 [LLD §5 / §11](../03-design/LLD.md) 与 [SRS NFR-08](../01-product/SRS.md)。

## 相关文档

- [DOC-07 FLOWS 端到端流程](../03-design/FLOWS.md)
- [DOC-08 DATA 数据模型](../03-design/DATA.md)
- [DOC-10 API 接口规范](API.md)
- [DOC-11 CONFIG 配置规范](CONFIG.md)
- [DOC-12 TEST 契约测试](../05-quality/TEST.md)
