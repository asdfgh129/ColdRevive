# `ed2k-mini` — ADR-011 L2 最小 eD2k 传输栈（Spike）

> 状态：**已完成首轮实现与验证（2026-11-11）**。本目录是 **协议探针 / 参考实现**，不是生产 crate。
> 所有常量与载荷逐条取自 aMule `master` @ `6a1105634d81242a581a0f36a7e00379b12a83f3`
> （与 `kad-mini`、[PROTO §2.0](../../../../docs/04-protocol/PROTO.md) 同一基线）。

## 1. 为什么放在 `tests/spikes/` 而不是 `src/crates/cr-ed2k`

[ROADMAP G0](../../../../docs/06-plan/ROADMAP.md) 于 2026-11-13 已由项目所有者批准[条件冻结](../../../../docs/06-plan/G0-REVIEW.md)（本节写作时为「未通过」，S1 / S3–S7 当时仍 `not_run`）；当前 S3 / S5 / S6 仍 `not_run` 但已豁免 G0 时点结论、M1/M2 必测回填，
[src/README.md](../../../../src/README.md) 的门禁是「技术路线冻结后才创建 `Cargo.toml` 与 crate 实现」——
该门禁现已满足（条件冻结生效），但 **M1 二进制与 `CliAdapter` 工程接线未完成**，
ADR-011 批准的 L2 产物 `cr-ed2k`（M-12）因此仍暂不落在生产 workspace。
本 spike 持有**可审计、已跑通的参考实现**，M1 按门禁接线后整体迁入 `src/crates/cr-ed2k`，
不改变 P1「零协议侵入」：只实现标准 eD2K 报文，不改其格式。

零依赖。`MD4` / `ed2k_hash` / KAD tag codec 通过 `#[path]` 从
[`../kad-mini/src/wire.rs`](../kad-mini/src/wire.rs) 引入，仓库里**只有一份哈希实现**。

## 2. 实现范围（ADR-011 四层能力中的 ②④）

②「我们从 eMule 下载」与 ④「旧客户端下到我们」是**同一份栈的两端**，本 spike 用
`stack::download()` 与 `stack::serve()` 一次实现：

| 方向 | 报文序列 |
| :--- | :--- |
| 客户端 | `OP_HELLO(0x01)` → `OP_HELLOANSWER(0x4C)` → `OP_REQUESTFILENAME(0x58)` → `OP_REQFILENAMEANSWER(0x59)` → （多分块时 `OP_HASHSETREQUEST(0x51)` / `OP_HASHSETANSWER(0x52)`）→ `OP_STARTUPLOADREQ(0x54)` → `OP_ACCEPTUPLOADREQ(0x55)` 或 `OP_QUEUERANKING(0xC5/0x60)` → `OP_REQUESTPARTS(0x47)` → `OP_SENDINGPART(0x46)` → `OP_END_OF_DOWNLOAD(0x49)` |
| 服务端 | 对应的上传侧 + **单槽最小上传队列**（无槽时回 `OP_QUEUERANKING`，FIFO rank） |

帧格式：`src/OtherStructs.h` 的 `Header_Struct` =
`<protocol u8><packetlength u32 LE><opcode u8><payload>`，`packetlength = 1 + payload.len()`。
`CPacket::GetPacketSizeFromHeader` 会减去 opcode 那 1 字节。

**明确不做**（是范围声明，不是隐藏缺口）：压缩 `0x40`、混淆、积分 / credit / 安全标识、
源交换、AICH、>4 GiB 的 I64 分片（`0xA2` / `0xA3`）、eD2K 服务器连接、NAT 穿透。
ADR-011 明确把 credit 留到 M1 实测后再决定是否补做。

### 两个易错点（源码核实结论）

1. **HELLO 的 tag 布局与 KAD 相同，与 ed2k/met 的紧凑形式不同。**
   `CTag::WriteTagToFile` → `CFileDataIO::WriteTag` 写 `[type][u16 name_len][name][value]`；
   `CTag::CTag(CFileDataIO&)` 两种形式都能读（`[type|0x80][name:1]` 也接受）。
   `WriteNewEd2kTag` 的紧凑形式**不能**用在 KAD 路径上 —— 那正是
   [PLAN §19](../PLAN.md) 修掉的 v2.4.26 缺陷。本 spike 写标准形式、读两种。
2. **`OP_REQUESTPARTS` 的偏移是分组的，不是交错的。**
   `DownloadClient.cpp::SendBlockRequests` 先写 3 个 start、再写 3 个 end（恒 3 组，不足补零）；
   `UploadClient.cpp::ProcessRequestPartsPacket` 按同序读取。写成 3 个 `<start,end>` 对会被静默忽略。
   `OP_SENDINGPART` 则是 `<hash 16><start u32><end u32><data>`。

## 3. 关键修正：`ed2k_hash` 必须用 aMule 的 Red 变体

L1 的 63 字节探针文件没跨过分块边界，掩盖了一个会导致**死条目**的缺陷：
文件大小恰为 `PARTSIZE = 9_728_000` 的整数倍时，两套互不兼容的 ED2K 哈希存在分歧 ——
crates.io `ed2k::Ed2k`（Blue）不补空分块，而 aMule / eMule（Red）会追加
`MD4("") = 31D6CFE0D16AE931B73C59D7E0C089C0` 再对整张哈希表做 MD4。
若用 Blue，生成的 `fileID` 与所有旧客户端都不同：搜索得到、永远下不动。

三条源码依据（同一基线）：

| 出处 | 事实 |
| :--- | :--- |
| `src/KnownFile.cpp::SetFileSize` 注释 | 给出真实样本：1×PARTSIZE 文件有 2 个分块哈希，末个是 `31D6CFE0…`，文件哈希 `A72CA8DF7F07154E217C236C89C17619` |
| `src/ThreadTasks.cpp::CreateNextPartHash` | `if (partLength == PARTSIZE && file.Eof()) m_hashlist.push_back(CMD4Hash(g_emptyMD4Hash));` |
| `src/KnownFile.cpp::CreateHashFromHashlist` | 对整张哈希表做 MD4 |

`kad-mini/src/wire.rs::ed2k_hash` 已按 Red 修正，并用上游自己的已知答案向量做回归：
`MD4(4891ED2E… || 31D6CFE0…) == A72CA8DF7F07154E217C236C89C17619` → 通过。
L1 中原先断言「精确整数倍不补空分块」的测试已撤回并反转。

## 4. 运行方式

```bash
cd tests/spikes/s2/ed2k-mini
cargo test                     # 31/31：wire codec + 队列 + 哈希（含上游向量）
cargo run -- selftest          # 26/26，输出 JSON 报告
cargo run -- loopback          # 真实 TCP 回环，我们自己的客户端 ↔ 我们自己的服务端
cargo run -- lan-hello --ip <RFC1918_IP> --port <TCP>   # 真实 eMule/aMule 握手，硬校验私有地址
```

报告落到 `tests/spikes/s2/reports/`，固定 `public_network_contacted=false`、
`evidence_valid_for_pass=false`；**不改 `spikes.json`、不判任何 pass**。
`lan-hello` 只发标准 `HELLO` + 两次只读交换，不下载文件内容、不发布索引。

## 5. 首轮证据（2026-11-11）

| 项 | 结果 |
| :--- | :--- |
| `cargo test` | **31/31**（含 `kad-mini` 复用模块的 21 个 codec/crypto 测试） |
| `selftest` | **26/26** |
| `loopback` | 3/3 用例完整闭环：4 KiB 单分块（无 hashset）、恰好 1×PARTSIZE（Red 空分块哈希）、>1×PARTSIZE（多分块）。每例 `hash_matches=true` / `content_matches=true`，53 个分片收发一致，最小上传队列放行 1 槽 |
| `lan-hello`（`172.16.29.209:31969`，真实 eMule） | `OP_HELLO` 104 B → `OP_HELLOANSWER` 107 B，解析出 nick `https://www.emule-project.org`、`CT_VERSION=0x3c`、`CT_EMULE_UDPPORTS=0xa150a150`（41296）；`OP_ASKSHAREDFILES` → `OP_ASKSHAREDFILESANSWER`，共享文件数 **0** |
| 公网报文 | **0** |

### 证据边界（必须跟结论写在一起）

- **握手是真客户端给的，完整分片传输是我们自己两端给的。** 二者不能互相替代。
  这台 LAN eMule 共享文件数为 0，因此能力 ②「从旧客户端下载」**无法对它实测**；
  须在 M1 换一台有共享文件的对端补测，同时实测 ADR-011 关注的 credit 缺失排队代价。
- `OP_SETREQFILEID (0x4F)` 对一个不可能存在的 fileID：aMule `ClientTCPSocket.cpp:425`
  规定应回 `OP_FILEREQANSNOFIL (0x48)`，真实 LAN eMule 在 4 s（另做 12 s 独立复核）内静默。
  这是**源码基线与真实客户端的差异，已原样登记为开放观测**，没有改写成通过或失败。
  `OP_REQUESTFILENAME (0x58)` 对未知 fileID 静默则是 aMule 源码的设计行为（`:392` 的 `break`）。
- 本轮**没有**做公网发布；任何公网 `0x43` / `0x44` 发布都需执行人重新显式 attest
  （见 [PLAN §22.7](../PLAN.md)）。
- 索引面（③ 旧客户端搜到我们）由同仓 `kad-mini l2-source-probe` 单独验证：
  `0x43` + `0x44` 双轨发布、`0x33` + `0x34` 双轨取回，见 [PLAN §23](../PLAN.md)。
  那一侧也只在 RFC1918 上跑，0 个公网报文。

## 6. 相关文档

[ADR-011](../../../../docs/02-architecture/adr/ADR-011-ed2k-interop.md) ·
[PROTO §2.2](../../../../docs/04-protocol/PROTO.md) ·
[PLAN §23](../PLAN.md) ·
[OPEN-QUESTIONS Q9 / V-04](../../../../docs/06-plan/OPEN-QUESTIONS.md) ·
[kad-mini README](../kad-mini/README.md)
