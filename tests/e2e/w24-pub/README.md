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
