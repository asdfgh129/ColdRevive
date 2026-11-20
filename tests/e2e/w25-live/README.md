# W25-live — FR-25 与真实 aMule 2.3.3 双向互通（RFC1918 LAN）

生产 `cr-ed2k::stack`（下载/上传共用一份，由 `coldrevive-ed2k` 暴露为 `download` /
`serve` 子命令）对**真实 eMule 系客户端** aMule 2.3.3（`192.168.1.221`，amuled
`-c /opt/g3-clients/amule-config`）做的双向互通实测。

## 结论（2026-11-20）

`python tests/e2e/w25-live/make_report.py` → **22/22 pass，0 fail，0 blocked**

| 方向 | 样本 | 关键观测 |
|---|---|---|
| 下载（我们 ← aMule） | `ed2k-sample-small.bin` 262144 B | `peer_hello=true`、`bytes_received=262144`、`blocks_received=25`、`hash_matches=true`、`content_matches=true`、`completed=true` |
| 上传（aMule ← 我们）单块 | `w25-upload.bin` 65536 B | `peer_hello=true`、`queue_granted=true`、`blocks_sent=1`、`bytes_sent=65536`，对端落地 md5 `ca9e1458…` 一致 |
| 上传（aMule ← 我们）多块 | `w25-upload-multi.bin` 262144 B | `peer_hello=true`、`blocks_sent=3`、`bytes_sent=339968`，对端落地 md5 `b04f5194…` 一致 |

两个上传样本的 `file_id` 都由**文件内容**导出（`coldrevive-ed2k link`，MD4 over
9.28 MB 分块 + Red 变体规则），不是从别处抄的字符串。

## 怎么看 `bytes_sent=339968 > 262144`

多块样本里 aMule 对**已收区间又发了一次 `REQUESTPARTS`**，重复发送的字节如实计入
`bytes_sent`（观测不打折）。帧序列是：

```
HELLO → HELLOANSWER → REQUESTFILENAME → REQFILENAMEANSWER
→ STARTUPLOADREQ → ACCEPTUPLOADREQ
→ REQUESTPARTS(40B, 3 range 槽) → SENDINGPART(77824) + SENDINGPART(184320)   # 正好 262144
→ REQUESTPARTS(40B)            → SENDINGPART(77824)                          # 重复区间
→ CANCELTRANSFER
```

正确性由**对端落地文件 md5** 判，不靠字节数相等。单块样本里没有重复请求，所以那里仍
用「精确相等」这条更严的判据。

## 复现

```bash
# 0. 样本（内容固定，可重放；不要改，哈希会变）
python - <<'PY'
open('tests/e2e/w25-live/run/w25-upload.bin','wb').write(bytes(((i*37+19)%256) for i in range(65536)))
open('tests/e2e/w25-live/run/w25-upload-multi.bin','wb').write(bytes(((i*91+7)%256) for i in range(262144)))
PY

# 1. 从内容导出 ed2k 链接
./target/release/coldrevive-ed2k.exe link --file tests/e2e/w25-live/run/w25-upload.bin

# 2. 起供片端（只服务一个连接，结束即退出），再让 aMule 拉
./target/release/coldrevive-ed2k.exe serve \
  --file tests/e2e/w25-live/run/w25-upload.bin --port 4663 --timeout-ms 180000 \
  > tests/e2e/w25-live/reports/upload.json 2>/dev/null
ssh -i "$TEMP/crkeys/id_ed25519" root@192.168.1.221 \
  "amulecmd -p 4712 -P coldrevive -c 'Add ed2k://|file|w25-upload.bin|65536|<hash>|/|sources,192.168.1.210:4663|/'"

# 3. 反向：从 aMule 取片到本机
./target/release/coldrevive-ed2k.exe download \
  --file tests/e2e/w25-live/run/ed2k-sample-small.bin --host 192.168.1.221:4662 \
  > tests/e2e/w25-live/reports/download.json

# 4. 判据（会 ssh 去读 aMule 落地目录的 md5；读不到记 BLOCKED，不记 pass）
python tests/e2e/w25-live/make_report.py
```

`amulecmd` 的 `ECPassword` 是 `md5(明文)`，本机配置里对应明文 `coldrevive`；
改过得重启 amuled 才生效。

## 边界（不许外推）

- **只有 RFC1918 LAN 客户端互通**（`192.168.1.210` ↔ `192.168.1.221`）。公网 eD2k
  传输仍 **BLOCKED**：没有 eD2k server 登录、没有 source exchange、没有公网 peer TCP 证据。
- 未实现且 ADR-011 明确排除：eD2k server(`LoginServer`)、source exchange、credit、
  AICH、压缩/混淆、>4 GiB I64 包、NAT 穿透。
- 样本都远小于 `PARTSIZE = 9,728,000`，因此只覆盖**单分片内的多块**：
  跨分片、断点续传、`OP_HASHSETREQUEST` 校验都没被覆盖（`hashset_verified=false`）。
- `queue_rank_sent=""` 表示队列**立即授予**（`UploadQueue::new(2)` 有空槽），
  没观察到排队等待；`OP_QUEUERANKING` 分支未在真实客户端上被触发过。
- 这只是「栈对真实客户端可用」的组件级证据，不等于 FR-25 的
  「守护进程生产路径 + 双轨发布 + 公网」全量可用；`g2_claim=false`、`s6_status=not_run` 不变。

## 产物

- `reports/download.json` — 下载方向观测（生产 `stack::download`）
- `reports/upload.json` / `reports/upload-multi.json` — 上传方向观测（生产 `stack::serve`）
- `reports/upload-link.json` / `reports/upload-multi-link.json` — 样本的内容导出链接
- `run/*.bin` — 固定内容样本（可重放）
