# W24-pub — FR-24 公共轨：真实 aMuleD KAD 关键词检索 + 标准 ed2k:// 链接

> 2026-11-20 · `make_report.py` **12/12** · 口径：`g2_claim=false`、`s6_status=not_run` 不变。

## 证明什么

在所有者批准的公共轨上，生产工具 `coldrevive-ed2k search` 对**真实 aMuleD KAD 节点**
（`192.168.1.221:4672`，自身连接公共 KAD）发 `BOOTSTRAP_REQ` + KAD2 `0x33
SEARCH_KEY_REQ`，读回 `0x3B SEARCH_RES`，解析 KAD tag，生成标准 `ed2k://` 链接。
Key 是无前缀 `MD4(UTF-8(term))`（A 轨），不是私有命名空间 Key。

## 机制

1. 用 S2 `kad-mini ed2k-dict-probe` 先向 aMuleD **发布**唯一关键词条目，确认
   `publish_acked=true` 且 `our_entry_returned=true`（已重启 aMuleD 恢复 KAD）。
2. 生产工具独立执行：bootstrap → `0x33` → `0x3B`，逐 tag 解析 filename/filesize，
   用 `fileID`（answer.source_id）拼 `ed2k://|file|<name>|<size>|<hash>|/`。
3. `make_report.py` 对账两次报告里的 key、file hash、size、filename 和链接。

## 证据（tests/e2e/w24-pub/reports/report.json）

- 关键词 `fr24-after-restart`，Key `ad50ee0f…a42e` 两侧完全一致。
- aMuleD 回显 `target` 与 Key 一致，`bootstrap_ms=273`、`rtt_ms=6`。
- `answer_count=1`，`file_id=28654c6a…c91c` 与 S2 发布的 `ed2k_hash` 一致。
- 链接：`ed2k://|file|coldrevive-ed2k-probe-1795181883359.txt|63|28654c6abc132c91cdcd855084db8e7c|/`。
- KAD tag 真解出 type 2 字符串（文件名）与 type 9 整数（大小），没有把原始切片当成「已解析」。

## 边界（照写）

- 通过口径是**局域网友好节点上的真实 aMuleD KAD**，该节点自身连着公共 KAD；
  `public_kad_contacted=false` 不能读成「A 主机直连公网节点成功」。
- `public-search.json` 记录了 A 主机直连 5 个公网 KAD 节点的失败尝试
  （多数 BOOTSTRAP_RES 超时），该次 `answer_count=0`，不作为通过证据。
- 只证明**检索与链接生成**，不证明下载；FR-25 的真实客户端传输另测。
- 不实现 credit / 上传队列；不改 `g2_claim` / `s6_status`。

## 复现

```bash
# 1) 生产工具
cargo build --release --offline -p coldrevive-ed2k

# 2) 先让真实 aMuleD 持有关键词条目（S2 报告作为发布侧证据）
./tests/spikes/s2/kad-mini/target/release/kad-mini.exe ed2k-dict-probe \
  --ip 192.168.1.221 --port 4672 --keyword fr24-after-restart --rate 1.0 \
  > tests/e2e/w24-pub/reports/publish-after-restart.json

# 3) 生产工具独立检索
./target/release/coldrevive-ed2k.exe search \
  --term fr24-after-restart --bootstrap 192.168.1.221:4672 \
  --timeout-ms 1800 --retry 2 > tests/e2e/w24-pub/reports/search-after-publish.json

# 4) 判定
python tests/e2e/w24-pub/make_report.py
```

> 联系非 RFC1918 节点时必须显式加 `--public`；工具不会后台自动探测公共 KAD。
> G0 `protocol_scope` 已于 2026-11-22 由项目所有者书面解除，公共 `0x33`/`0x43`
> 不再行政 BLOCKED；`--public` 运行时默认按 G0 口径 0.2 pps 发包节流，
> 可用 `--rate-limit-pps` 覆盖（0=不限速，仅建议回环调试）。
> 检索命中仍只认真实 `SEARCH_RES` + 可解析 tag；0 命中如实记 0，不得外推 LAN 结果。

## 公网 0 命中的根因与修正（2026-11-23，v2.4.105）

早期公网探针 `BOOTSTRAP_RES 3/3` 后直接对 bootstrap 节点发 `0x33`，全部超时。
S2 `PLAN.md` §6/§20 已给出机制：aMule `Process2*IndexRequest` 用
`SEARCHTOLERANCE = 2^24` 校验「节点 KadID ↔ Key」的 XOR 距离，越界**静默丢弃**；
且 >200 B 的应答可能是 `0xE5` zlib 帧。旧工具既不收敛、也只认 `0xE4`。

生产工具已修：

1. `BOOTSTRAP_RES` 取回对端自报 KadID 与联系人（`0xE5` 自动解包）；
2. `--public` 时先用 `0x21/0x29` 迭代收敛（默认 `--max-hops 12 --alpha 3`），
   只对**回显 target 且落在责任区**的节点发 `0x33`；未收敛就不发，不把静默误记成 0 命中；
3. `--publish-file` 允许 A 轨最小样本发布（同一个唯一 key，最多 3 个已验证在区节点，
   0.2 pps），`PUBLISH_RES` 回显才记 ACK；ACK 也不等于公网可检索；
4. `evidence_valid_for_pass` 恒为 `false`——程序永不自证通过。

实测 `public-lookup-20261123-ubuntu.json`：36 次 `0x21`、33 次 `0x29`、
33 个已验证节点、1 个在区（`top_chunk=8,875,008 < 2^24`）；对该在区节点的
`0x33` 仍超时。  对应 S2 §20.5 的另一类边界：**能答 `0x21` 的在区节点也可能是
  UDP 入站不可达/拒绝索引操作**，不是地址或距离错误。
- 复测：`public-roundtrip-20261123-wide.json` 用 16 跳/alpha=5
  （80 发 `0x21` / 72 收 `0x29`）仍只 1 个在区节点；
  `public-roundtrip-20261123-fresh.json` 从新鲜联系人 2 跳找到 2 个在区节点，
  两者 `0x43`/`0x33` 全静默（`ack_count=0`、`answer_count=0`）。
  **判断边界已修正（v2.4.106）**：在 `192.168.1.221` 本机用真 aMule 搜 `ubuntu`
  可得 361 命中，而同机同出口我们的明文工具 0 命中 → 不是 UDP 入向不可达，
  而是 aMule 的 `0x33/0x3B` 均为 RC4 混淆帧（`CryptoKadUDPKey`），我们只发明文。
  下一步是补齐 aMule KAD v8 的 HELLO/receiver-verify-key 语义（v2.4.107 已接线，
  真机状态见 `local-hello-*.json` 与审计 §16）。

## KAD v8 混淆 / 握手开关（v2.4.107）

`coldrevive-ed2k search` 新增：

- `--hello`：索引前对目标发 `KADEMLIA2_HELLO_REQ (0x11)`，可选解析
  `HELLO_RES (0x19)` 并在对方置 `TAG_KADMISCOPTIONS` bit2 时回 `HELLO_RES_ACK (0x22)`；
- `--hello-plain` / `--hello-try2`：HELLO 分别走明文 / try-2(receiver verify key)，
  默认走 try-0（目标 KadID）并带派生出的 receiverVerifyKey；
- `--peer-udp-key <u32>` + `--our-public-ip <ip>`：按 aMule
  `CPrefs::GetUDPVerifyKey` 从对方 `CryptoKadUDPKey` 与我们可见 IP 派生 receiver key。
  缺任一项时不伪造验证，只发 try-0（rvk=0）。

真机 2026-11-23：明文 `--hello-plain` 与 **try-2 `--hello-try2`** 都拿到真 aMule
`HELLO_RES`（后者 `hello_envelope=try2`、`receiver_verify_key=3224252923`、
`obfuscated_response=true`、`receiver_key_verified=true`），见
`reports/local-hello-try2-verified-20261123.json`。**try-0 在同样条件下仍无应答**
（抓包帧按 aMule 收包语义可自解，但不被对端接受），所以不要把 try-0 成功当默认前提。
`receiver_key_verified` 只在 try-2 真换回 HELLO_RES 时置真。

报告新增 `handshake` 字段，逐目标记录信封、RTT、对端 KadID/版本、是否请求 ACK、
是否真发了 ACK 与错误。**`evidence_valid_for_pass` 仍恒 false**；HELLO 有响应也不等于
公网检索命中，命中只认真实 `SEARCH_RES` 与可解析 tag。try-2 需要预知对端
`CryptoKadUDPKey` 与我们被看到的 IP，公网节点目前仍缺这两个输入。

## NAT 后 HELLO 的端口口径（v2.4.108）

在对称 NAT 后（222）跨公网做 try-2 时，除 `--peer-udp-key` / IP 还需要：

- `--key-hex <32HEX>`：把索引键钉在靶机 KadID 的 `2^24` 责任区内
  （`top_chunk=0`）。这是实验输入，报告 `key_source=hex_override`，
  **不再声称 key 是 `MD4(term)`**；
- `--bind-port <port>`：固定本地 UDP 口，便于把「对端可见端口」测准并复用；
- `--our-public-port <port>`：HELLO 载荷 `TAG_SOURCEUPORT` 必须填**对端实际看到的
  端口**，不是本地端口，也不是 STUN 对另一台服务器给的端口（对称 NAT 会不同）；
- `--stun <ip:port>`：可选，在**同一条 KAD socket** 上取 `XOR-MAPPED-ADDRESS`，
  适合 EIM NAT；对称 NAT 仍须用 `--our-public-port` 指定 KAD 目标可见端口；
- `--index-envelope auto|try2|try0|plain`：只用于诊断对照，不提高判定。

实测：222 → 221 公网路径上 try-2 HELLO 已真成功
（`reports/public-try2-hello-verified-20261123.json`），但 `0x33`/`0x43`
在三种信封下都静默（`top_chunk=0` 排除越界），**索引命中仍 BLOCKED**；
详见审计 §18。

## 更正：0x33/0x43 静默 ≠ 索引接纳门（v2.4.109）

§18 末尾的「索引接纳/节点状态」归因**错误**，见审计 §18.1。aMule 2.3.3 源码事实：

- `Process2SearchKeyRequest`（0x33）**无责任区门**、不要求接触已验证；
  `SendValidKeywordResult` **只在目标节点本地索引命中 key 时才回 0x3B**。
  `--key-hex` 贴的合成 key 不在任何人索引里 → 静默是正确行为。
- `Process2PublishKeyRequest`（0x43）在**接收节点** `IsFirewalledUDP(true)` 时
  静默 return；`InTrackListIsAllowedPacket` 忽略 verify key，只做洪泛限速。
- 干净 A/B：2026-11-17 同一公网目标 112.5.60.4:4672 有完整
  PUBLISH_RES + SEARCH_RES（591 B）闭环（w51g `run-20261117T085411Z.json`）；
  今天该 aMule `Kad: Connected (firewalled)`、`key_index.dat` 28 B 空，
  同一 probe 变 `wan_publish_silent_in_zone`。**唯一变量是目标防火墙状态。**

新增 `--search-closest` 诊断旗标：无在区节点时改取最近已验证节点（≤3、0.2 pps、
`evidence_valid_for_pass=false`），用于真实关键词对照；本次 `ubuntu` 仍 0 命中
（`reports/public-real-keyword-closest-20261123.json`），与 firewalled 一致。

**复现闭环的必要条件**：先把 221 的 aMule 恢复为非 firewalled，再跑同一
`kad-mini wan-index-probe`。不要再换信封或加大发包。

2026-11-23 恢复尝试（证据 `reports/firewall-remediation-attempt-20261123.json`）：
重启 221 aMule 后 ~15 s 内重新声明 firewalled，60 s 内不变；但 222 与独立公网中继
分别发 2 字节 `KAD2_BOOTSTRAP_REQ` 都收到 521/523 B `BOOTSTRAP_RES`，且重启期间抓到
95 个正常 KAD UDP 包 → 入站路径与 KAD 收发正常，firewalled 来自 aMule 自身测试。
因此**重启不是恢复手段**；需要让该测试拿到成功回包，或改用非 firewalled 且
本地索引含目标关键词的节点。中继主机只开放 UDP 7100，不能作为无 NAT 探针主机。
