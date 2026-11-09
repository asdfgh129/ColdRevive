---
doc_id: DOC-08
title: 数据模型与迁移（DATA）
category: 设计
version: 2.0
status: 待评审
owner: 内核组
updated: 2026-11-09
---

# 数据模型与迁移（DATA）

> 编号 DOC-08 · 版本 2.0 · 状态 待评审 · 责任人 内核组 · 更新 2026-11-09
> 存储引擎：SQLite（WAL 模式，`rusqlite`）。Schema 是唯一事实来源，字段变更须走迁移脚本并同步本文。

## 1. 设计原则

1. **索引与做种上下文生命周期解耦**：销毁 `TorrentContext` 不影响 `files` 行，这是影子索引（[LLD §3](LLD.md)）成立的前提。
2. **大对象不入库**：`.torrent` 文件落盘到元数据目录，DB 只存 `torrent_path` 引用。
3. **隐私最小化**：`path` 存相对共享根目录的路径，日志中脱敏，绝不留绝对路径（[SRS NFR-09](../01-product/SRS.md)）。
4. **可重建**：索引库始终可从共享目录全量重建，重建低优先级、可中断（[SRS NFR-06](../01-product/SRS.md)）。

## 2. Schema

```sql
CREATE TABLE files (
  file_hash      TEXT PRIMARY KEY,        -- SHA-256
  path           TEXT NOT NULL,           -- 相对共享根目录，日志中脱敏
  size           INTEGER NOT NULL,
  mtime          INTEGER NOT NULL,
  ed2k_hash      TEXT,
  infohash       TEXT UNIQUE,
  piece_size     INTEGER,
  torrent_path   TEXT,                    -- 伴随种子缓存路径
  state          TEXT NOT NULL DEFAULT 'COLD',
                 -- SCANNING/HASHING/INDEXED/COLD/WAKING/HOT/SERVING/STALE
  last_active_at INTEGER,
  wake_count     INTEGER DEFAULT 0,
  created_at     INTEGER NOT NULL,
  UNIQUE(file_hash, piece_size)
);
CREATE INDEX idx_files_state        ON files(state);
CREATE INDEX idx_files_last_active ON files(last_active_at);
CREATE INDEX idx_files_infohash    ON files(infohash);

CREATE TABLE keywords (
  keyword_hash   TEXT NOT NULL,
  file_hash      TEXT NOT NULL REFERENCES files(file_hash) ON DELETE CASCADE,
  PRIMARY KEY (keyword_hash, file_hash)
);

CREATE TABLE kad_publishes (
  target_key     TEXT PRIMARY KEY,
  file_hash      TEXT REFERENCES files(file_hash) ON DELETE CASCADE,
  published_at   INTEGER,
  expires_at     INTEGER,
  ack_count      INTEGER DEFAULT 0
);

CREATE TABLE peers (
  node_id        TEXT PRIMARY KEY,
  last_addr      TEXT,
  reputation     REAL DEFAULT 0.5,        -- 0..1
  success_count  INTEGER DEFAULT 0,
  fail_count     INTEGER DEFAULT 0,
  blacklisted    INTEGER DEFAULT 0,
  updated_at     INTEGER
);

CREATE TABLE wake_log (
  trace_id       TEXT PRIMARY KEY,
  file_hash      TEXT,
  peer_node      TEXT,
  result         TEXT,                    -- OK / REJECT_* / TIMEOUT / HASH_MISMATCH
  latency_ms     INTEGER,
  created_at     INTEGER
);

CREATE TABLE config (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
```

## 3. 表职责

| 表 | 写入方 | 读取方 | 生命周期 |
| :--- | :--- | :--- | :--- |
| `files` | M-01 / M-02 / M-03 / M-09 | 全模块 | 跟随共享目录；文件移出时删除 |
| `keywords` | M-01 分词 | M-05 发布与查询 | 随 `files` 级联删除 |
| `kad_publishes` | M-05 | M-05 续期 / 撤销 | 随 TTL 滚动；`TOMBSTONE` 后自然过期 |
| `peers` | M-08 / M-15 | M-05 排序、M-08 过滤 | 长期，按 `updated_at` 老化清理 |
| `wake_log` | M-08 | 可观测性、`/api/v1/metrics` | 滚动保留（默认 7 天） |
| `config` | M-14 | 全模块 | 持久，热更新写回 |

### 3.1 `files.state` 取值

与 [FLOWS §7 状态机](FLOWS.md) 一一对应：

| 值 | 含义 |
| :--- | :--- |
| `SCANNING` | 事件已捕获，等待写入完成去抖 |
| `HASHING` | 指纹与分片哈希计算中 |
| `INDEXED` | 伴随种子已生成，等待发布 |
| `COLD` | 影子索引在线，无做种上下文 |
| `WAKING` | 切入中（加载元数据、申请 LRU 资源） |
| `HOT` | 做种上下文就绪，无活跃连接 |
| `SERVING` | 正在传输 |
| `STALE` | 文件被修改/重命名，等待重算与旧 Key 撤销 |

### 3.2 `peers.reputation`

- 取值 `0..1`，初值 `0.5`；仅作**本地**评分，不跨节点同步（[ADR-005](../02-architecture/adr/ADR-005-incentive.md)）。
- 唤醒成功 / 传输校验通过 → 上调；`HASH_MISMATCH` / 超时 / 污染 → 下调，低于阈值写入 `blacklisted`。
- 用于 M-05 结果排序与 M-08 黑名单过滤，不作为全局信用凭证。

### 3.3 `wake_log.result`

与 [PROTO §3 错误码](../04-protocol/PROTO.md) 对齐，便于按 `trace_id` 做端到端追踪（[SRS NFR-11](../01-product/SRS.md)）。

## 4. 迁移策略

| 项 | 约定 |
| :--- | :--- |
| 版本管理 | 独立 `schema_version` 表，顺序编号迁移脚本，`PRAGMA user_version` 作为校验 |
| 兼容窗口 | 向下兼容至少 2 个版本 |
| 迁移时机 | 守护进程启动前，事务内执行；失败则拒绝启动并保留原库 |
| 备份 | 升级前自动备份 `.db` 文件（[OPS §4](../07-operations/OPS.md)） |
| 破坏性变更 | 必须提供「重建索引」路径，并声明重建代价（大文件全量重算） |
| 协议变更 | `PROTOCOL_VERSION` 变更时新旧 KAD 命名空间并存一个 TTL 周期后下线旧版 |

## 5. 容量与性能约束

- 目标 ≥ 50,000 文件（[SRS NFR-05](../01-product/SRS.md)）；1 万文件空闲 RSS ≤100 MB（NFR-01）。
- `files` 行是 COLD 状态的主要常驻开销，必须保持「一行索引 + 无上下文」，禁止缓存 Bitfield 或分片哈希数组。
- 启动索引加载目标 < 10 s（[TEST 压测基线](../05-quality/TEST.md)）；必要时按 `state` 分批惰性加载。
- `wake_log` 定期清理，避免长期运行无限增长（[TEST Soak 72 h](../05-quality/TEST.md)）。

## 相关文档

- [DOC-06 LLD §2 / §3](LLD.md)
- [DOC-07 FLOWS 状态机](FLOWS.md)
- [DOC-09 PROTO 错误码](../04-protocol/PROTO.md)
- [DOC-11 CONFIG 配置规范](../04-protocol/CONFIG.md)
