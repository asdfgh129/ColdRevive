---
doc_id: DOC-30
title: TCP 映射端口发现与 EIM 判定 Runbook
category: 运维
version: 1.3
status: 持续维护
owner: 内核组 / 测试组 / 运维组
updated: 2026-11-25
---

> 编号 DOC-30 · 版本 1.3 · 状态 **持续维护** · 责任人 内核组 / 测试组 / 运维组 · 更新 2026-11-25

# TCP 映射端口发现与 EIM 判定 Runbook

## 1. 目的与边界

本文把「常规打洞失效后，先用外部观测点发现 NAT 映射端口，再把该端口作为信令交给对端」固化为可复用的**应对方法**，并规定 EIM（endpoint-independent mapping，端点无关映射）的判定口径与 `coldrevive-nat` 的 CLI 参数。

**它回答**：这个出口的 TCP 源端口映射是否 endpoint-independent？映射成了哪个端口？发现端口能否作为对端回连的信令？

**它不回答**：这条路径能否算 NAT 矩阵通过？能否解锁 G2/S6？答案是**不能**。本方法只产出组件级证据；门禁常量始终为 `g2_claim=false`、`s6_status=not_run`、`evidence_valid_for_pass=false`、`auto_pass=false`。

背景与原始证据见 [阶段欠账复核 §20.3–20.5](../06-plan/STAGE-DEBT-AUDIT-2026-11-21.md) 与 [测试计划 §11](../05-quality/TEST.md)。

## 2. 术语与 EIM 判定表

| 术语 | 含义 |
| :--- | :--- |
| 固定本地端口 | 发现阶段与打洞阶段都由本机绑定并主动发起的源端口，如 `192.168.1.221:19910`。 |
| observer | 公网可达的 TCP 观测点。对每个入向连接回写 `CR-MAPPED <ip>:<port>`，其中端点为 `accept()` 看到的**映射后源端点**。 |
| 发现端口 | observer 回写的公网端口，是给对端的**信令字段**，不是配置里假设的端口。 |
| EIM | 同一本地端口对不同目的地址映射为同一公网端点；端口可能随会话变化，但在同一会话内对目的无关。 |

判定由 `cr_nat::tcp_discovery::judge_observations` 纯函数完成，与网络 I/O 解耦：

| 成功观测数 | 端点一致性 | `eim` | `discovered_port` | 允许结论 |
| ---: | :--- | :--- | :--- | :--- |
| 0（含未提供 observer） | — | `false` | `null` | **BLOCKED**：无证据，不合成端口。 |
| 1 | — | `null` | `P` | 只得到候选端口；**未证 EIM**。可用于同会话即时尝试，但必须经真实握手验证，不得外推、不得写成 EIM 已证。 |
| ≥2 | 一致 | `true` | `P` | EIM 成立（仅限本次本地端口 + 这些目的 + 此刻）。把 `P` 作为信令交给对端。 |
| ≥2 | 不一致 | `false` | `null` | 非 EIM / 映射随目的变化；**发现端口不可外推**，本方法不适用，回退中继。 |
| ≥2 | 一致但端点解析不出端口 | `false` | `null` | 观测数据损坏；按无证据处理。 |

`eim` 的三态必须原样保留：`null` 是「未证」，`false` 是「已反证或无证据」，`true` 是「已证」。JSON 输出中 `null` 不得被脚本改写成 `false`。

真实 EIM 证据示例：221 从 `192.168.1.221:19910` 分别连两个独立公网 IP，两台观测机 tcpdump 都看到 `112.5.60.4:20811`，判 EIM 成立；证据见 `tests/ARCHIVE/evidence/tcp-eim-two-observers-tcpdump-20261124.json`。注意早先抓到的 `18011` 是**另一会话**的映射端口，不是同一会话的端口漂移。

## 3. CLI 参数

两个模式互斥，命中后即返回，不在一次调用里与其它探测模式混用。

| 参数 | 参数值 | 默认值 | 说明 |
| :--- | :--- | :--- | :--- |
| `--serve-observer` | `<ip:port>` | 无 | observer 模式：监听并回写 `CR-MAPPED <peer>\n`。公网机建议绑 `0.0.0.0`，安全组只放通需要的观测端口。 |
| `--observer-secs` | `<秒>` | `20` | observer 存活秒数；只需覆盖发现窗口，打洞阶段不要求 observer 常驻。 |
| `--discover-tcp-port` | `<本地 ip:port> <observer ip:port[,ip:port...]>` | 无 | 客户端模式：从固定本地端口依次向每个 observer 发真 TCP 连接，读回映射端点。多个 observer 用逗号分隔。 |
| `--timeout` | `<毫秒>` | `3000` | 每个 observer 的连接 + 读回超时；发现模式内部下限 `500 ms`。 |
| `--json` | 无 | 关 | 输出稳定字段的 JSON；脚本必须按 JSON 解析，不要抓日志文本。 |

输出字段：

- observer：`mode`、`bind`、`served`、`write_failed`、`elapsed_ms`、`s6_status`。
- 发现：`mode`、`local`、`observations[]`（`observer` / `mapped_endpoint` / `error`）、`eim`、`discovered_endpoint`、`discovered_port`、`reason`、`s6_status`、`g2_claim`、`auto_pass`。
- 任何读不到、解析不出映射端点的 observer 都写入 `error`，不会合成 `mapped_endpoint`。

## 4. 适用性判断：常规方法失效时是否改用本办法

```mermaid
flowchart TD
  A[常规打洞失效] --> B{能绑定固定本地端口?}
  B -- 否 --> Z[不适用: 回退中继 / 保持 BLOCKED]
  B -- 是 --> C{有 >=2 个独立公网 observer?}
  C -- 否 --> Z
  C -- 是 --> D[--discover-tcp-port]
  D --> E{eim}
  E -- true --> F[取 discovered_port 作即时信令]
  E -- null --> G[仅候选: 补第二观测或独立抓包]
  G --> Z
  E -- false --> Z
  F --> H{真实握手 + 双向字节?}
  H -- 是 --> I[组件级 PASS: 归档, 不宣称 G2]
  H -- 否 --> Z
```

**触发条件**（常规方法已失效且出现下列现象之一）：

1. 入口 DNAT 已验证可承载双向 TCP，但 simultaneous-open 仍 `Connection refused`，抓包显示对端 SYN  destination 是配置端口而非实际映射端口（源端口被 PAT 改写）。
2. 上游设备没有「保持源端口 / 转换后源端口」选项，公网 IP 为多设备共用，无法做整主机 1:1 NAT。
3. 中继可用，但需要验证是否存在真实直连路径；目标只是组件级结论，不是 NAT 矩阵覆盖率。

**适用前提**（全部满足才进入探测）：

1. 本机可从固定源端口发起连接，且发现与打洞用**同一个**本地端口。
2. 至少两个公网观测点，优先不同 IP / 不同上游；安全组允许客户端连入 observer 端口。若只放通已有端口，可在该端口上做临时 REDIRECT 到 observer，跑完立即撤销并复测原服务。
3. 对端能收到发现端口，并在映射存活期内回连；对端自身若为对称 NAT，应保持主动连接方角色。
4. 已有真实握手与双向字节的验证手段（`--tcp-punch --echo` 或 `tests/e2e/w56/public_tcp_punch_discovered.py`）。

**不适用 / 立即停止**：

- observer 回写不一致 → `eim=false`，退出，不挑选其中一个端口「试运气」。
- 只有一个 observer → `eim=null`，不得写 EIM 已证；补第二观测点或独立 tcpdump 证据后再判。
- 无信令通道、映射生命周期短于信令往返时间、或本地端口无法重绑。
- 需求是 S6 矩阵覆盖或 G2 自动解禁 —— 本办法在原理上不产生这类证据。

## 5. 标准操作

以 221（本地 `192.168.1.221:19910`）和两个公网观测点为例：

```bash
# 1) 两台公网机分别启动 observer（端口按安全组实际放通值调整）
coldrevive-nat --serve-observer 0.0.0.0:19110 --observer-secs 120 --json
coldrevive-nat --serve-observer 0.0.0.0:19111 --observer-secs 120 --json

# 2) 221 从固定源端口发现映射端点
coldrevive-nat --discover-tcp-port 192.168.1.221:19910 \
  82.157.68.159:19110,119.29.150.254:19111 \
  --timeout 3000 --json

# 3) 只在 eim=true 时取 discovered_port，作为信令交给对端，立即打洞
python3 tests/e2e/w56/public_tcp_punch_discovered.py --report-out <路径>

# 4) 归档原始 JSON / tcpdump / 报告，并写明组件级边界
python3 scripts/archive_test_run.py --evidence <路径> --notes "组件级发现端口打洞；不宣称 G2"
```

若安全组限制导致 observer 无法直连，可沿用 v2.4.120 的做法：在已放通端口上临时 REDIRECT 到 observer，发现完成后撤销并确认原端口服务恢复。真实实验里双观测点 EIM 用 tcpdump 抓 SYN 取证，发现端口用 82 的临时 REDIRECT 取证，两者互相印证。

## 6. 测试策略：不要只靠全量串行

全量串行是最终回归，不是本方法的日常判据。按顺序先跑下面两层定点测试：

```bash
# A) Rust 定点单测：判定纯函数 + observer 回写 + 异常输入（秒级）
CARGO_INCREMENTAL=0 cargo test --offline -p cr-nat tcp_discovery -- --nocapture

# B) CLI 回环冒烟：双观测一致 / 单观测未证 / 不可达 / 不一致不出端口（6 项）
CARGO_INCREMENTAL=0 cargo build --offline -p coldrevive-nat
python3 tests/e2e/w56/tcp_discovery_selftest.py \
  --bin target/debug/coldrevive-nat \
  --report-out tests/e2e/w56/reports/tcp-discovery-selftest.json

# C) 最终回归（可选，不能替代 A/B）
CARGO_INCREMENTAL=0 cargo test --workspace --offline -- --test-threads=1
```

A 覆盖 `judge_observations` 的全部四态、`parse_port` 拒绝 0/垃圾值、observer 回写格式、无前缀/坏端口/空回写、不可达 observer、JSON 门禁常量。B 在真实 CLI 上覆盖 observer 进程、参数解析、JSON 输出和「不一致不合成端口」。C 只用于确认没有跨模块回归；默认并行下既存计时型 flaky 不等于本方法失败。

**B 的回环 PASS 只证明控制路径正确，不证明 NAT EIM。** NAT EIM 仍必须由公网双观测点证据（`tcp-eim-two-observers-tcpdump-20261124.json`）或等价真实抓包证明。

## 6.1 守护进程 punch-route 使用发现端口（v2.4.122）

发现端口不只属于独立探针：守护进程的 `nat punch-route` 控制命令接受第三个
可选参数作为**发现端口覆盖**：

```bash
# 先用第 5 节拿到 discovered_port（例如 21249），确认 eim=true，立即调用。
# 注意 /cli 的真实形状是 {"command":...,"args":[...]}，不是 method/params：
curl -X POST http://127.0.0.1:17800/cli \
  -H 'Content-Type: application/json' \
  -d '{"command":"nat","args":["punch-route","B","192.168.1.221:19910","21249"]}'
```

双端真实同时打开还要传**同一个共享 nonce**（第四参数，32 位十六进制）。
两端各调一次 `nat punch-route`，nonce 不同则裁决不匹配、按 fail-closed 回落中继：
`["punch-route","B","192.168.1.221:19910","<B_MAPPED_PORT>","<NONCE_HEX>"]`
与 `["punch-route","A","192.168.34.222:19920","<A_MAPPED_PORT>","<NONCE_HEX>"]`。
新中继连接必须先 `register` 再交换裁决，否则帧会被服务端丢弃；这两项修复
已有定点单测并在 221 原生构建后部署。

安全约束（已由 `parse_discovered_port` / `parse_shared_nonce` 单测覆盖）：

- 只替换**端口**，IP 仍取已登记的对端 `bt_external`，不接 `ip:port` 字符串，
  防止把任意地址塞进打洞目标。
- 端口必须是 1..65535；0、越界、非数字一律报错，不静默回退。
- 不给第三个参数时行为与旧版逐字节相同。
- 第四参数 nonce 必须是恰好 32 位十六进制；缺参数保持旧随机 nonce，非法值直接报错。
- 成功结果带 `discovered_port_used`，便于审计本次是否用了发现端口。

**daemon 端口布局**：若引擎继续绑 `0.0.0.0:19910`，守护进程自己的
`punch_tcp` 会因 `EADDRINUSE` 拿不到同一个 LAN 源端口。部署时用
`engine.listen = "127.0.0.1:<bt_port>"` 把本地引擎退到回环：holder 的
`relay-sub` 拨号目标仍由 `bt_port` 得出、保持正确，而 LAN 上的
`19910/19920` 留给真打洞 socket。`bt_external` / 通告端口保持不变。
这是部署布局，不改 NAT 判据，也不代表公网入向 DNAT 仍直接可达引擎。

边界不变：这仍是控制面接线，返回 `s6_status=not_run` / `g2_claim=false`；
必须有真实 EIM 观测、发现端口即时使用，且不外推到对称 NAT 的对端。

## 7. 实测边界与回退

- v2.4.120 首次发现端口 `112.5.60.4:21211` 作信令，222 回连该端口而非固定 `19910`，两侧 `ok=true` + `bytes_roundtrip=true`，连续 3 轮带 tcpdump 全 PASS；证据 `tests/ARCHIVE/evidence/tcp-punch-discovered-3rounds-verify-20261124.json`。
- 221 的 EIM 不外推到 222。2026-11-25 双侧实测：221 从 `19910` 连 82/119
  均映射为 `112.5.60.4:34656`（EIM）；222 从 `19920` 连 82/119 分别映射为
  `112.5.32.34:2050` / `:2055`（不一致，非 EIM）。按上表规则，B 侧不得给出
  `discovered_port`，本次 daemon 双侧打洞如实判 BLOCKED，没有拿单观测凑 PASS。
- 发现端口有生命周期，必须即时使用；跨会话重连要重新发现，不缓存旧端口。
- 该场景下真实 BT 文件已改走**中继兜底**并落地 md5 一致
  （`tests/ARCHIVE/evidence/bt-over-public-relay-20261125.json`）；这是 relay
  组件证据，不写成 punched-direct 通过。
- **直连（非打洞）路径 v2.4.132 已闭环**：A `112.5.60.4` 拨 B 的固定 DNAT
  `112.5.32.34:19920`，入向握手唤醒冷种子，1 MiB 落地 md5 一致
  （`tests/ARCHIVE/evidence/bt-wake-on-handshake-public-20261125.json`）。它与「打洞」
  是两件事：这条靠运营侧固定映射，不消耗 EIM 发现端口，也不能替代 B 侧非 EIM 的结论。
- 任一失败分支都回退中继，并把 `eim` / `reason` / `error` 原文归档；不得把 BLOCKED 改写成 PASS，不得通过重复抽样「碰运气」。

## 8. 公网可达性判定的三个陷阱（2026-11-25 补）

这三条都真实误导过我一次，写进来避免重复踩。

### 8.1 远程接入端口不是 22

| 主机 | 公网入口 | 对应内网 |
| :--- | :--- | :--- |
| 221（`ubuntu`） | `ssh -p 54022 root@112.5.60.4` | `192.168.1.221:22` |
| 222（`ubuntu23`） | `ssh -p 54022 root@112.5.32.34` | `192.168.34.222:22` |

确认方法（主机密钥指纹，避免连错机器）：

```bash
ssh-keyscan -p 54022 -t ed25519 112.5.60.4 | ssh-keygen -lf -   # 4cOI… → 221
ssh-keyscan -p 54022 -t ed25519 112.5.32.34 | ssh-keygen -lf -   # ZoUZ… → 222
```

在本机（同处 `192.168.1.0/24`）连 `112.5.60.4:54022` 会**超时**，因为家宽不支持
hairpin；连 `112.5.32.34` 属于另一条宽带、不经过 hairpin，所以正常。判断某端口是否
公网可达，必须从**外部观测机**（`82.157.68.159`、`119.29.150.254`）发起。

### 8.2 DNAT 端口回 RST ≠ 映射没了

2026-11-25 曾据「两台观测机对 `112.5.32.34:19920` 都收到 RST」判定 CPE 映射已撤，
实际是当台主机 `0.0.0.0:19920` 没有监听（`engine.listen=127.0.0.1:19920`，握手代理未启用），
RST 由主机内核发出。启用代理后同一端口立刻握手成功、真实 BT 数据跨公网落地。

判定顺序应是：① 本机 `ss -lntp` 确认后端确实在 `0.0.0.0:<port>` 监听 →
② 对端 tcpdump 确认 SYN 到达（区分「CPE 没转发」与「主机没监听」）→ ③ 才讨论映射。

### 8.3 云主机上「NAT 类别」不是 NAT 决定的

两台云观测机的 `eth0` 是私网（`10.2.0.16` / `10.1.0.2`），公网由云侧 EIP 映射。2026-11-25
实测结论：

- 该映射是**端口保持的 1:1 静态 NAT**：本地绑 `40000` → 公网就是 `:40000`，绑 `40001` →
  `:40001`，且与目的端口无关（EIM）。**不需要、也没法再「设成 1:1 NAT」，它本来就是。**
- 入向过滤是**安全组按目的端口的白名单**，无状态。同机 A/B：放通端口 `3479` 会接受从未
  接触过的源端口（`119.29.150.254:5577`）乃至完全不同的公网 IP（`112.5.60.4`）打进来并
  正常应答（= full-cone）；未放通端口 `40001` 的入向包主机侧一个都看不到（= port-restricted）。
- 所以「云主机是 port-restricted-cone」这句话只对**当时放通的那几个端口之外**成立；放通
  区间一改，类别就变。需要 full-cone 出口时，改安全组即可，不必改代码。
- 主机侧 `iptables` 只有 `YJ-FIREWALL-INPUT` 的 SSH 暴破黑名单，不是过滤源；排查时别看错地方。

证据：`tests/ARCHIVE/evidence/s6-cloud-egress-1to1-nat-20261125.json`。

### 8.4 地址轴怎么测（分类工具做不到时的替代法）

`coldrevive-nat --classify` 的地址轴依赖 STUN 服务端的 CHANGE-IP，而单公网 IP 的
服务端会回 `change_ip_error=420`（能力缺失，不是拦截）。云主机各自只有一个 EIP，
所以工具只能报 `cone-undetermined`，**不能据此认定 NAT 不是 full-cone**。

替代做法（2026-11-25 实测有效）：用一台**目标从未接触过的独立主机**发未请求数据报。

1. 目标机绑定固定本地端口（如 `40100`），向观测点发 STUN，读出自己的公网映射。
2. 独立主机（其公网 IP 从未被目标机访问过）直接向该**映射端口**发一个带标记的 UDP 包。
3. 目标机 socket 收到并回包 ⇒ 两条同时成立：
   - 包被投递给这个 socket ⇒ 该公网端口对不同**目的地址**同样有效（映射 EIM）；
   - 目标机从未向该源发过包却被放行 ⇒ 过滤不限源地址/源端口（EIF）。
   EIM + EIF 即 RFC 4787 **full-cone**。
4. 必做对照：拿一台**未改防火墙**的同型号主机跑同一脚本，应当 `NO-REPLY` 且入向列表为空。
   没有这个对照，探针可能只是自己骗自己。

广州那台放通后实测：`40100/40101` 映射端口稳定、`112.5.60.4:15456` 打入被应答；
北京那台同脚本 `5000/19920/40000` 三个端口零入向。放通范围扫到
`5000–65000` 全通（含 `bt_port` 用的 `19920`），故节点用固定端口也能拿到 full-cone 入向。

证据：`tests/ARCHIVE/evidence/s6-cloud-119-fullcone-20261125.json`。
**红线**：这是「云侧 1:1 静态 NAT + 安全组放通」的组合行为，不能写成「天然 full-cone 主机」，
更不能据此写「NAT 矩阵通过」。

## 9. 相关文档

- [阶段欠账复核 §20.5](../06-plan/STAGE-DEBT-AUDIT-2026-11-21.md)
- [测试计划 §11](../05-quality/TEST.md)
- [CLI 自测脚本](../../tests/e2e/w56/tcp_discovery_selftest.py)
- [发现端口打洞探针](../../tests/e2e/w56/public_tcp_punch_discovered.py)
