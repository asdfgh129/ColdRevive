---
doc_id: ADR-017
title: 做种活跃池由 cr-sched 唯一决策，引擎 activate 不得绕过
category: 架构决策
version: 1.0
status: 已接受
owner: 内核组
updated: 2026-11-18
---

# ADR-017：做种活跃池由 `cr-sched` 唯一决策

| 项 | 值 |
| :--- | :--- |
| 状态 | **已接受**（2026-11-18，W4.3 实现） |
| 决策者 | 内核组（依 M2/W4.3 与 ADR-015 实现约束） |
| 性质 | 实现层决策：把「谁能同时供片」从散落的调用点收拢成一处 |
| 关联 | [ADR-002](ADR-002-bt-engine.md) · [ADR-015](ADR-015-dependency-layering.md) · [ROADMAP](../../06-plan/ROADMAP.md) · [CONFIG](../../04-protocol/CONFIG.md) |

## 背景

W5.2 起，`WAKEUP_REQ` 到达时的动作是「解除该任务的引擎暂停」(`Engine::activate`)。
在单种子场景下这没问题；冷存储场景下它会变成一个没有上限的增长过程：
每来一次唤醒就多一个活跃任务，而**没有任何一处代码知道「同时能开几个」**。
失败形态是渐进的——先磁盘随机读变慢，再上传带宽被摊薄，最后第三方客户端看到
「连上了但没速度」。这类退化不会被任何门禁判据捕获，只会被用户当成「这软件不稳定」。

M2 欠的「LRU 调度接引擎」正是这个洞。执行层面有三个可选做法：

1. 在 `cr-engine` 内部做活跃池（引擎自己数任务）；
2. 在 `coldrevived` 的唤醒路径里内联一段 LRU；
3. 单独 crate（`cr-sched`）持有决策，`coldrevived` 负责把决策变成引擎调用。

## 决策

**采用 3。** 具体约束：

- `cr-sched` 只做**纯逻辑**：活跃集合、逻辑时钟、容量、逐出顺序。不做 I/O、不认识 librqbit。
- `coldrevived` 的 `sched::wake_seed()` 是**唯一**允许调用 `Engine::activate` 的入口。
  它按顺序执行：申请名额 → （若逐出）暂停被逐出者 → 激活请求者 → 如实返回发生了什么。
- `AlreadyActive` 时**不重复激活**：librqbit 对 live 任务 `unpause` 会 `bail`，
  重复唤醒是正常流量，重复激活会把「已在供片」翻成失败（W5.2 已踩过）。
  只有引擎状态确实不是 HOT 才重试一次（自愈），不拿调度器的记账当引擎事实。
- 逐出时**先暂停再激活**；暂停失败必须上报，但不阻止新任务激活——唤醒的语义是
  「让请求方拿到片」，资源越界是另一件事，两件都要说，不许合成一句「成功」。
- 激活失败要**释放名额**并把被逐出者放回池子：否则一次失败会永久少一个名额。
- 容量来自 `engine.max_active_seeds`，缺省用 `cr-sched::MAX_ACTIVE_SEEDS`（8）。
  配置层拒绝 0，不在运行期静默夹到 1——这个数字决定节点能不能供片，
  「配错了」不能长得像「配好了」。

## 2026-11-18 补证：真实引擎下的逐出

第一版只有 `FakeOps` 单测，属于「我们按约定调了 pause」。第二轮补了
`coldrevived::sched::tests::real_engine_eviction_pauses_the_previous_seed`：
真 `RqbitEngine` + 两个真 `.torrent`（各 96 KiB）、`paused` 加入、容量 1，
按 A→B→A 唤醒，断言 `Engine::state` 观测到 `Cold → Hot → Cold → Hot`。
Windows 与 Linux 两端均通过（全仓 232 passed）。

**仍然存在的限制**：守护进程只持有一个做种任务（`st.seed_task: Option<TaskId>`），
所以这是**组件级**证据。「多任务守护进程」需要先改产品形态（多样本入库 + 每任务映射），
不在本轮范围；不得据此声称「生产多任务调度已验证」。

## 后果

- 正面：活跃任务数有硬上限且可观测（`/health` 的 `seeding_active` / `seeding_cap` /
  `seeding_evictions`）；逐出语义有 14 条单测（8 条 `cr-sched` + 6 条 `wake_seed`）钉住，
  其中逐出路径用 `FakeOps` 覆盖，不依赖 librqbit 也不依赖真实磁盘。
- 负面/限制：**当前守护进程只持有一个做种任务**（`seed_task`），因此逐出路径在真实运行中
  不会被触发，只有单测覆盖。多任务能力要等索引层把「多个文件各自成种」接进来；
  在那之前不得声称「多任务调度已在生产验证」。
- 约束：任何新代码若直接调 `Engine::activate` 即为回归。这是代码审阅项，不靠文档自律。

## 备选方案与否决理由

- **引擎内建池**：`cr-engine` 会被迫知道「任务优先级/冷热」这类业务概念，
  且 `Engine` Trait 的边界（ADR-015：业务 crate 只经 Trait）会被撑破。
- **唤醒路径内联 LRU**：决策逻辑与 HTTP/UDP 处理耦合，无法在没有引擎的环境里测试，
  而「没有引擎的测试」正是这次能把逐出路径讲清楚的原因。
