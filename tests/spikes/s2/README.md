# S2 实验目录（`tests/spikes/s2/`）

> **本目录不是 S2 结论。** 按 [tests/spikes/README.md §5](../README.md) 与 `spikes.json` 的
> `S2.criteria`，本地回环私有 KAD 子网只能作为预演，**不得据此把 S2 判为 `pass`**。
> 实网实施排期与门禁见 **[PLAN.md](PLAN.md)**。

## 1. 三层产出

| 层 | 路径 | 作用 |
| :--- | :--- | :--- |
| L1 逻辑预演 | [`s2_loopback.py`](s2_loopback.py) | JSON 消息的私有子网双节点，只验流程与命名空间概念，**不是真实协议** |
| L2 **真实 wire 闭环** | [`kad-mini/`](kad-mini/README.md) | 独立零依赖 Rust Cargo 工程，真实 `0xE4` Kademlia2 报文 + `0xE5` zlib 解包，BOOTSTRAP / HELLO / 迭代查找 / PUBLISH_KEY / SEARCH_KEY |
| L2.5 **真实客户端 LAN 互操作** | `kad-mini lan-probe` | RFC1918 内真实 eMule，只发 BOOTSTRAP / HELLO，验报文可解析性，**不发布 Key** |
| L3 实网小样本 | `kad-mini live` | ⚠️ **2026-11-10 已执行但 inconclusive**：3 × `BOOTSTRAP_REQ` 发出、0 响应、0 发布；环境过滤公网高位 UDP，须换网络重跑（[PLAN.md §7](PLAN.md)） |

```bash
python3 tests/spikes/s2/s2_loopback.py                    # L1
cd tests/spikes/s2/kad-mini && cargo test                 # L2 单测 7/7
cd tests/spikes/s2/kad-mini && cargo run -- selftest      # L2 常量/编解码/zlib/门禁断言 37/37
cd tests/spikes/s2/kad-mini && cargo run -- loopback      # L2 真实报文闭环
cd tests/spikes/s2/kad-mini && cargo run -- lan-probe --ip <RFC1918_IP> --port <UDP>   # L2.5
cd tests/spikes/s2/kad-mini && cargo run -- live --check  # L3 门禁检查（不发包）
```

用户提供的 `nodes.dat` / `server.met` 由 [`nodes_dat_to_bootstrap.py`](nodes_dat_to_bootstrap.py)
只读解析，输出到 `reports/*.local.json`（已忽略，**仓库不含任何节点 IP**）。

报告落在 `reports/`（不纳入版本控制），固定标注
`public_kad_contacted: false`、`evidence_valid_for_pass: false`、`s2_status: "not_run"`。

## 2. L1 逻辑预演

只用 Python 标准库，在 `127.0.0.1` 起两个 UDP 节点：A 发布 5 个 `ColdRevive_V1:`
前缀 Key（Key ID 用 SHA-256 前 16 字节映射到 128-bit 空间），B 按 XOR 距离选最近节点发
`FIND_VALUE` 并命中；额外发布一个无前缀同名 Key 验证 ID 隔离。发布 5/5、命中 5/5。

**注意**：这层的 `FIND_VALUE` / `FIND_VALUE_REPLY` 是自定义的 JSON 消息，eMule KAD 里
并不存在这两个操作。真实映射见 L2。

## 3. L2 真实 wire 闭环（2026-11-09 已跑通）

[kad-mini](kad-mini/README.md) 零第三方依赖，报文字节逐条对齐 aMule 源码。三端点
（发布者 A / 索引节点 / 查询者 B）全程 `127.0.0.1`：

```
BOOTSTRAP_REQ/RES → HELLO_REQ/RES → KADEMLIA2_REQ/RES (迭代查找)
→ PUBLISH_KEY_REQ/RES → SEARCH_KEY_REQ/RES (B 命中 A 的描述符)
```

实测 5/5 发布被 ACK、5/5 查询命中且描述符 Tag 匹配、RTT p50 0.23 ms、
84 条收发报文的完整十六进制逐次记录、0 协议错误。命名空间隔离验证通过
（带前缀与无前缀 Key ID 不同，控制条目未泄漏到带前缀查询）。

**两层 ID 口径不同**：L1 用 SHA-256 前 16 字节、L2 用 SHA-1 前 16 字节派生 Key ID，
同一逻辑 Key 在两层得到不同 ID，结果不可直接比较；两者都是临时映射，
最终规则待 ADR / PROTO 决策（见 [PLAN.md §0](PLAN.md)）。

**关键发现**：5 个 Key 相对索引节点的 `xor_top32` 全部远超 `SEARCHTOLERANCE = 2^24`，
回环靠 aMule 的 LAN 豁免才被接受；在 WAN 上会被**静默丢弃、无任何响应**。
实网发布必须先多跳迭代查找到 ID 落在 Key 邻域的节点。详见 [PLAN.md §0](PLAN.md)。

## 4. 为什么还没有实网结论

**P0–P3 已全部完成**（协议核实、wire 闭环、LAN 互操作、G0 参数与 bootstrap 清单），
`S2.public_kad_approval.pending` 已清除并写入 `confirmed`（`sample_size=3`、`rate_limit_pps=0.2`）。
P4 已于 **2026-11-10** 按 G0 参数显式执行，但**没有产出可用结论**：3 条 `BOOTSTRAP_REQ`
发出后零响应，学不到任何联系人，`live` 按 `criteria #2` 正确跳过全部 3 个样本、未发布任何 Key。
对照实验证明是执行环境过滤公网高位 UDP（详见 [PLAN.md §7](PLAN.md)），与 KAD 协议和命名空间无关。
因此仍缺的依旧是**一次有效的 P4**：逐次实网原始记录（发布成功率、查询时延分布、丢弃/拒绝观测）。

`kad-mini live --check` 带齐参数后返回 `gate.passed=true`，但 **`--check` 永不发包**；
只有实验执行人显式去掉 `--check` 并给出 `--attest-i-am-the-experiment-operator` 才会执行 P4。
`S2.status` 保持 `not_run`：已接触公网节点但未获得任何协议证据。

**重跑前先做网络前置检查**（新增，v2.4.8）：

```bash
python3 tests/spikes/s2/udp_egress_check.py   # v2.4.9：低位/高位对照 + 可选 --source-port / --lan-ip / --json；0 = 可跑 P4，2 = 被挡
```

本环境实测退出码 2（DNS/53 正常、STUN/19302 与 /3478 全超时、直连 TCP 超时），
所以 P4 必须在**高位 UDP 未被过滤的主机/网络**上重跑。v2.4.9 复核进一步发现：UDP/53 与
UDP/123 其实是通的，STUN 类高位未知端口被出口白名单丢弃；固定源端口、本机防火墙均已排除。
v2.4.11 更正：该白名单结论只对 STUN 成立，真实 KAD 报文须用 `kad_egress_probe.py` 单独测。
另外 `lan-probe` 是纯 LAN 流量，不能证明公网可达。v2.4.10 跨主机 **STUN** 对照在 LAN 那台
eMule 机器（开了入站映射）上同样 `BLOCKED` / 退出码 2。**v2.4.11 收窄**：STUN 不等于 KAD ——
项目所有者提供该机 eD2K HighID 与 Kad 页签 432 个联系人的截图，本机则用新增的
[`kad_egress_probe.py`](kad_egress_probe.py)（真实 `0xE4 0x01`、3 条 / 0.2 包每秒、不发布 Key）
确证本机 0/3 无响应，但该机 KAD 是否真正存活仍待核实。**v2.4.12**：`Kad 网络` 面板已补
（`状态` / `UDP 状态` 均「打开」、无 Buddy 行），firewalled / 陈旧路由表假设被否证；但按
aMule 源码，KAD UDP「打开」是**入站可达性**测试（远端向本机公网 IP:KAD 端口发无载荷 UDP
包，入站 DNAT 即可满足，协调走 TCP），`m_firewalledUDP` 初值还是 `false`，故与「出站高位
UDP 被过滤」不矛盾，也不能证明该机可跑 P4。详见 [PLAN.md §9](PLAN.md)：把该探针拷到该机
跑一次（退出码 `0` 是充分证据、`2` 不是充分否证），并查看 `统计信息` KAD 收发包计数是否增长。
**v2.4.13**：该探针已在该机执行，结果 `NO_REPLY (0/3)` / 退出码 2；但本机与该机用的是同一份
3 目标，不是独立复现。新增 [`bootstrap_res_to_targets.py`](bootstrap_res_to_targets.py)
（只读解析 LAN `BOOTSTRAP_RES`，**不发包**）取 03:12 UTC 新鲜 20 contacts 中的 10 个公网
IPv4（与旧目标重叠 0/10），本机宽扫仍 **0/10**。另外按 eMule 官方帮助，「下载/搜索正常」
不构成 KAD 出站证据：下载走 TCP 与网络选择无关，默认搜索可为 eD2k 服务器 TCP 搜索，
只有明确选 `Kademlia` 方法出结果才要求 UDP 通 —— **不要开下载验证**。
工程结论与归因解耦：本网无独立客户端被证明可跑 P4，**换网重跑**；详见
[PLAN.md §10](PLAN.md)。

**v2.4.7 已补齐 P4 前置实现**（详见 [PLAN.md §6](PLAN.md)）：`live` 发布前先迭代收敛到
`SEARCHTOLERANCE` 邻域、跳过 HELLO、用 `bind_any()` 绑定公网 socket、解码 `0xE5` 压缩帧；
`live_gate` 现在实际解析 bootstrap 并拒绝非公网 IPv4 / `lan-interop-probe-only` 清单。
`cargo test` 7/7、`selftest` 37/37、`loopback` 5/5 发布 + 5/5 命中、0 协议错误均复验通过。
上述复验是预演；P4 已在 2026-11-10 真实执行过一次（3 个公网节点、3 个报文），
但零响应且未发布任何 Key，`evidence_valid_for_pass=false`，结论不可用。

合规红线不变：人工批准、最小样本、限速、禁止批量发布测试 Key、bootstrap 由用户提供且仓库不内置。
观测口径已按 [PROTO §2.0](../../../docs/04-protocol/PROTO.md) 与
[RISK R-02](../../../docs/05-quality/RISK.md) 拆成「协议层不可解析 / 索引层拒绝 / 成功」三桶，
避免把压缩帧或超时误判成「公共网络不接纳」。v2.4.8 进一步补充：
**环境前置条件失败不属于这三个桶中的任何一个**，整次实验应判
`inconclusive_environment_blocked` 并作废，详见 [RISK R-02](../../../docs/05-quality/RISK.md)。
