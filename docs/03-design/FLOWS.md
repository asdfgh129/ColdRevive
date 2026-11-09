---
doc_id: DOC-07
title: 端到端流程与状态机（FLOWS）
category: 设计
version: 2.0
status: 待评审
owner: 架构组 / 内核组
updated: 2026-11-09
---

# 端到端流程与状态机（FLOWS）

> 编号 DOC-07 · 版本 2.0 · 状态 待评审 · 责任人 架构组 / 内核组 · 更新 2026-11-09
> 本文描述**运行时行为**；模块职责见 [LLD](LLD.md)，报文格式见 [PROTO](../04-protocol/PROTO.md)。

## 1. F1 文件入库与伴随造种

```mermaid
flowchart LR
  A["文件落入共享目录"] --> B["M-01 事件捕获<br/>等待写入完成/去抖"]
  B --> C["M-02 低优先级队列<br/>计算 SHA-256 / MD4 / 分片 SHA-1"]
  C --> D["M-03 生成伴随 .torrent<br/>缓存到元数据目录"]
  D --> E["M-04 写入影子索引<br/>state = COLD"]
  E --> F["M-05 发布 KAD 命名空间 Key<br/>M-06 发布 Mainline DHT"]
  F --> G{"M-09 活跃池有空位?"}
  G -->|是| H["state = HOT<br/>立即做种"]
  G -->|否| I["state = COLD<br/>仅保留影子索引"]
```

**关键约束**：文件重命名/修改会改变指纹，必须精确重算并重发索引；造种任务在系统 Idle 优先级执行，避免 4 GB 文件哈希抢占前台 I/O。

## 2. F2 搜索与发现

1. 用户输入关键词 → 本地分词、归一化（大小写、分隔符、繁简可选）→ 生成多个 `Keyword_Key`。
2. 向 KAD 发起 `FIND_VALUE`，获取节点描述符列表。
3. 结果与本地索引、黑名单、信誉分合并去重；按「在线概率 / 历史成功率 / 文件大小匹配度」排序。
4. 展示可选资源及其 InfoHash；用户也可直接粘贴 InfoHash / magnet 精确查找。

> **KAD 精确匹配陷阱**：KAD 关键词基于哈希精确匹配，不支持通配符与模糊查找。缓解手段是客户端侧分词后**逐词分别发布**多个 Key，并在结果侧做模糊重排与别名归一；代价是发布/查询量上升，因此需限制单词条数上限（默认 8）并对发布做限速（M0 压测确定，见 [OPEN-QUESTIONS Q2](../06-plan/OPEN-QUESTIONS.md)）。

## 3. F3 唤醒与激活（核心时序）

```mermaid
sequenceDiagram
  autonumber
  participant U as 用户 / Web UI
  participant B as 客户端 B（下载方）
  participant K as KAD / DHT 网络
  participant A as 客户端 A（持有者）
  participant S as A 的 LRU 调度器
  participant T as A 的 BT 做种上下文

  U->>B: 搜索「冷门资源」
  B->>K: FIND_VALUE(专属命名空间 Key)
  K-->>B: 节点描述符列表 [A: IP/Port/InfoHash]
  B->>B: 去重·信誉排序·黑名单过滤
  B->>A: UDP WAKEUP_REQ(File_Hash, Ver, Challenge, Nonce)
  A->>A: 校验签名 + 限流 + 查本地白名单目录
  alt 文件存在且校验通过
    A->>S: 申请切入活跃池（必要时 LRU 淘汰）
    S->>T: 加载 .torrent 元数据，实例化做种上下文
    T-->>S: 做种就绪（监听标准 BT 端口）
    A-->>B: WAKEUP_ACK(OK, magnet, torrent_url, ttl)
    B->>B: 校验 magnet 中的 InfoHash 与请求一致
    B->>T: 标准 BT Handshake（Peer Wire）
    T-->>B: 分片传输 + 逐片 SHA-1 校验
    B->>B: 完成 → 落盘至共享目录 → 触发 F1 闭环
  else 文件不存在 / 校验失败 / 超限
    A-->>B: WAKEUP_REJECT(错误码)
    B->>B: 标记节点不可用，尝试下一个候选
  end
```

## 4. F4 标准下载与闭环

B 拿到的是**完全标准**的 magnet / `.torrent`；数据传输 100% 走 BT Peer Wire，ColdRevive 私有协议不再参与。下载完成的文件默认落入共享目录，立即触发 F1 自动造种、入库、发布——**每一次下载都在扩充网络的可发现资源总量**，这是对抗冷启动的产品级设计（[LLD §13](LLD.md)）。

## 5. F5 被动唤醒（兼容旧客户端 / 未发信令直连）

```mermaid
flowchart LR
  A["任意 Peer 发起 BT Handshake"] --> B["M-10 无状态 Reactor 常驻<br/>开销极低"]
  B --> C["提取 InfoHash"]
  C --> D{"在影子索引中?"}
  D -->|否| E["按普通未知种子处理 / 拒绝"]
  D -->|是| F["触发切入：从磁盘加载元数据<br/>申请 LRU 资源"]
  F --> G["完成标准握手，开始做种"]
  G --> H["N 分钟无连接 → 切出休眠"]
```

这条路径保证：**旧客户端即使完全不知道 ColdRevive 的存在，只要通过公共 DHT 或 magnet 连上来，也能把冷文件「叫醒」**。

## 6. F6 休眠切出

活跃上下文在「无数据交换 + 无新连接」持续 `idle_sleep_minutes`（默认 10 分钟）后：销毁连接与缓存、释放 Bitfield、状态回写 `COLD`；**KAD / Mainline DHT 的通告不撤销**，形成影子索引。会话级硬上限 `wake_session_ttl`（默认 2 小时）防止异常请求长期占用。

## 7. 文件状态机

```mermaid
stateDiagram-v2
  [*] --> SCANNING: 文件落入共享目录
  SCANNING --> HASHING: 写入完成
  HASHING --> INDEXED: 伴随种子生成完毕
  INDEXED --> COLD: 发布 KAD 索引（默认）
  INDEXED --> HOT: 活跃池有空位 / 刚下载完成
  COLD --> WAKING: 收到 WAKEUP_REQ 或 Handshake
  WAKING --> HOT: 元数据加载 + 做种就绪
  HOT --> COLD: 空闲超时 / LRU 淘汰
  HOT --> SERVING: 有 Peer 连接并传输
  SERVING --> HOT: 传输结束
  COLD --> [*]: 文件被移出共享目录
  HOT --> [*]: 文件被移出共享目录
  note right of COLD
    物理休眠，逻辑在线
    影子索引持续通告
  end note
```

状态值落库为 [DATA `files.state`](DATA.md)，实现约定见 [LLD §3 / §6](LLD.md)。

## 8. 异常与降级路径

| 场景 | 检测 | 降级行为 |
| :--- | :--- | :--- |
| 节点已离线 | UDP 唤醒超时 / TCP 不可达 | 立即切换下一候选，不阻塞 UI（避免长 TCP 超时） |
| 文件已被删除或改名 | 唤醒时索引命中但 stat 失败 | 返回 `REJECT_FILE_GONE`，撤销该 Key 的 KAD 发布 |
| 活跃池已满 | LRU 无空位且全忙 | 返回 `REJECT_BUSY` 并附重试建议；或淘汰最久未活跃者 |
| 内存 / 带宽超阈值 | M-13 采样越界 | 暂停新唤醒、降低后台限速、强制切出最冷项 |
| NAT 双向不可达 | M-07 判定 Symmetric NAT / CGNAT | LowID 回退：经中继节点转发信令；数据面限速并提示用户 |
| KAD 查询无结果 | `FIND_VALUE` 空 | 回退 Mainline DHT 精确查 → 可选 Tracker → 本地索引 |
| 返回种子哈希不符 | 元数据 / 分片校验失败 | 丢弃、拉黑节点、信誉扣分 |
| 索引污染 / 洪泛 | 结果异常率、请求频率超阈 | 限流 + 黑名单 + 只采信多重校验结果 |
| 造种队列积压 | 队列长度 > 阈值 | 降低并发、Idle 优先级、延迟发布（先索引后造种） |

> 错误码定义见 [PROTO §3](../04-protocol/PROTO.md)。本表是 [TEST 故障注入用例](../05-quality/TEST.md) 的需求来源。

## 相关文档

- [DOC-06 LLD 模块详细设计](LLD.md)
- [DOC-08 DATA 数据模型](DATA.md)
- [DOC-09 PROTO 协议规范](../04-protocol/PROTO.md)
- [DOC-12 TEST 测试计划](../05-quality/TEST.md)
