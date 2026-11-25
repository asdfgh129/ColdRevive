# G2 出口决定单：S6 是否折算进 G2 出口（DOC-32）

- 状态：**已签**（2026-11-25，所有者；落点已于同日同步，见第 5 节末）
- 版本：v1.2（2026-11-25 签署；v1.1 更正 punched-direct 的表述：3/30 有真实命中，非「零」）
- 提出：agent｜决定权：项目所有者
- 关联：ADR-016（R1/R2/R3）、`tests/fixtures/g2-spike-verdicts.toml`、DOC-31 `G2-REVIEW-BRIEF-2026-11-25.md`

---

## 0. 这份文件是什么，不是什么

**是**：一张只有你签了才生效的决定单，用来把「S6 算不算 G2 出口的一项」写成可引用的文字，
免得下次又靠聊天记录推断。

**不是**：

- 不是 S6 的判定。S6 已于 2026-11-17 判 `pass(closed_with_exceptions)`，本文件不重判、不推翻。
- 不是 G2 达标声明。签完这个决定，`g2_claim` **仍然是 `false`**。G2 达标是另一个独立动作，
  且必须在你明确说「G2 达标」之后才动。
- 不是判据变更。`class_coverage_required_for_pass=false` 等一律不动。

---

## 1. 先把「折算」拆成两个层次

「S6 折算进 G2 出口」这句话在两种意义上被用过，混在一起就会吵不清：

| 层次 | 含义 | 影响什么 |
|---|---|---|
| **L1 记账** | G2 出口清单里「NAT 可达性」这一项，由 S6 的结论来关闭 | 出口清单的完成度、下一个人读到的状态 |
| **L2 对外表述** | 能不能对外说「公网可达性已按 eMule 语义结案」 | 对外措辞、README/发布说明 |

L1 与 L2 可以不同答案（例如：记账上不计入，但对外可以说 S6 已结案）。本决定单要求分别勾选。

---

## 2. 现状事实（可逐条复核）

1. S6 判定：`status = "pass"`，`verdict_kind = "closed_with_exceptions"`，
   `claim_wording = "按 eMule 语义结案，带已知限制"`。位置 `tests/fixtures/g2-spike-verdicts.toml`。
2. S6 证据：两份真实报告 + 一份 aggregate，均 `evidence_valid=true`；
   HighID 侧 direct 3/30 + relay 30/30，LowID 侧 direct 0/30 + relay 30/30。
   2026-11-25 已归档为可 clone 复核的副本：`tests/ARCHIVE/evidence/s6-verdict-20261117/`（含 sha256 清单）。
3. 类别覆盖：3/4（`full-cone` ×1 / `port-restricted-cone` ×2 / `symmetric` ×1）。
   `restricted-cone` 已按 ADR-016 §R2 退出待测清单；`CGNAT` 仍未覆盖。
   两者都**不构成**判据失败——`class_coverage_required_for_pass` 自始为 `false`。
4. **`punched-direct` 只有 3/30，不是零也不是稳定**。S6 highid 报告的 direct 路径 30 次里
   有 3 次真打通（`stun-mapped 112.5.60.4:47488`、`ack_status=OK`、`punch_acked=true`），
   端口是 NAT 为出站流量临时分配的映射口（内部监听 `17801`），所以这是打洞，不是 DNAT；
   其余 27 次 `acked=false`，**原因未定位**。
   另：BT 公网闭环（1 MiB 落地 md5 一致）走的是**运营侧固定 DNAT**，属 `direct-via-static-dnat`，
   不构成打洞证据。三种形态的分界见 ADR-016「术语：三种『直连』不得互相冒充」。
5. **一处措辞不一致，需一并处理**：同一份报告的 `limitation` 自述
   「不构成 HighID direct 可达证据」（因为 27/30 的失败源于 symmetric 请求者的 back 映射不稳），
   而判定文件依预注册判据 `direct succeeded > 0` 把它计为 HighID 证据。
   判定本身没有违反预注册条文，但「HighID 可达」与「不构成 HighID 证据」两种说法同时存在，
   签署时应在本单据里选定一种表述，避免下一个人各引一句。
6. 四个机器常量当前值：`g2_claim=false`、`s6_status=not_run`、
   `evidence_valid_for_pass=false`、`auto_pass=false`。

---

## 3. 三个选项与后果

### 选项 ① 直接计入（不附加条件）

- **做法**：G2 出口清单第「NAT 可达性」项标记为「由 S6 关闭」。
- **后果**：清单干净，但**例外被藏起来**——未来读的人只会看到「已关闭」，
  看不到 direct 3/30、看不到类别 3/4、看不到 punched-direct 缺失。
- **风险**：这正是这套判据一直防的「用一次判定覆盖一片空白」。**不推荐**。

### 选项 ② 不计入（S6 独立结案）

- **做法**：S6 只作为 Spike 结论存在；G2 出口清单里的「公网直连可达性」另立一项，
  且该项当前**未完成**（因为 punched-direct 没有正面结果）。
- **后果**：G2 出口多一项未完成；对外只能写「S6 已结案（带已知限制）」，
  不能写「G2 的可达性项已关闭」。
- **代价**：把一个本来已经说清楚的事，重新拆成两个都不完整的状态。
- **适用**：如果你认为「没有 punched-direct」就等于「公网可达性没验证」，选这个。

### 选项 ③ 条件计入（推荐）

- **做法**：计入，但**关闭时必须连同例外清单一起写入 G2 出口记录**。例外清单固定为四条：

  1. 类别覆盖 3/4，`restricted-cone` 按 ADR-016 §R2 排除，`CGNAT` 未覆盖；
  2. HighID 侧 direct 3/30，属描述性观测，不含可达率含义；
  3. `symmetric` 一侧 direct 0/30 是 NAT 行为，不是缺陷，按设计回退中继；
  4. **`punched-direct` 仅 3/30**（`stun-mapped` `112.5.60.4:47488`，`ack OK`，27/30 无应答且原因未定位）；
     BT 公网闭环属 `direct-via-static-dnat`，不构成打洞证据；报告 `limitation` 与判定措辞的
     不一致按第 5 条选定的一种表述写。

- **后果**：G2 出口的「NAT 可达性」项可关闭，但读者一定能看到上面四条。
  对外措辞上限为：「S6 按 eMule 语义结案，计入 G2 出口，带四项明写例外」。
- **理由**：它同时满足「把账结掉」和「不许用一次判定覆盖空白」两个要求，
  且例外是**结构化**的——不能靠谁记得。

---

## 4. 无论选哪个都不变的事

- 判据：真实设备、两台不同物理主机、每路径 ≥30 次真实 `WAKEUP_ACK`、不编数字、不自动 pass，一条不改。
- 常量：`s6_status=not_run`、`g2_claim=false`、`evidence_valid_for_pass=false`、`auto_pass=false`。
  **机器字段永远不由人工判定回写**，它表达的是「工具没有替人下结论」，与本文互不覆盖。
- 禁止措辞：`NAT 矩阵通过`、`G2 达标`、`任意可达率 / 成功率`、`restricted-cone 已覆盖`。
- 本决定**不能**替代「G2 是否达标」的总判。G2 还压着公平性一项（按预注册判据整体 FAIL，
  当前状态是「所有者记例外、暂缓」，例外 ≠ PASS）。

---

## 5. 签署

> 请直接在下面填写。签完我按其结果同步落点，不改任何机器常量。

- **L1 记账**（G2 出口清单是否由 S6 关闭）：☐ ① 直接计入　☐ ② 不计入　☑ ③ 条件计入（推荐）
- **L2 对外表述**（是否可写「公网可达性按 eMule 语义结案」）：☑ 可以（须带例外清单）　☐ 只写 S6 结论，不提 G2
- **HighID 措辞**（第 2 节第 5 条，二选一或并列）：
  ☐ 写「HighID 可达，direct 3/30」　☐ 写「不构成 HighID 证据，仅记录 3/30 命中」　☑ 两者并列
- **例外清单**（选 ③ 时生效，选 ① 时移入附录）：☑ 接受第 3 节四条　☐ 需要增删（请注明）
- **签署人**：项目所有者　**日期**：2026-11-25

**签署口径说明（执行方补记，供复核）**：

- L1 = ③、L2 = 可以、例外清单 = 接受四条：均为本单据标注的推荐项/主项。
- HighID 措辞一项本单据未标推荐项（三个选项并列）。所有者回复为「三个都勾了」，
  未指定单选值。执行方取 **「两者并列」**，理由是它是三选中**唯一不丢失信息**的一项：
  既保留预注册判据（`direct succeeded > 0`）成立的「HighID direct 3/30」，
  也保留报告 `limitation` 的自述「27/30 无应答，不构成 HighID direct 可达证据」。
  单向性：若所有者要改成单选，改动只涉及 `g2_exit_fold_in_highid_wording` 一个字段与本节勾选，
  不影响 L1/L2 与例外清单。

### 签署后需要同步的落点（由执行方做，逐条可勾）

1. `tests/fixtures/g2-spike-verdicts.toml` → `[[spike]] S6` 增 `g2_exit_fold_in = "<选项>"` 与 `fold_in_exceptions = [...]`（**不改** `status`/`verdict_kind`/`claim_wording`）。
2. `docs/02-architecture/adr/ADR-016-s6-emule-like-reach.md` → `## 复审记录` 增 `### R3（折算决定）`，只引用本单据编号与结论，不重述理由。
3. `docs/06-plan/G2-REVIEW-BRIEF-2026-11-25.md` → 第 6 节第 3 项「仍未决」改为已决并指向本单据。
4. `CHANGELOG.md` 一条记录。
5. **不动**：`g2_claim`、`s6_status`、`evidence_valid_for_pass`、`auto_pass`。

### 落点同步结果（2026-11-25，逐条可核）

| # | 落点 | 实际写入 |
|---|---|---|
| 1 | `tests/fixtures/g2-spike-verdicts.toml` → `[[spike]] S6` | `g2_exit_fold_in="conditional"`、`fold_in_exceptions`（四条）、`g2_exit_fold_in_highid_wording="both"`、`_signed_at`/`_decided_by`/`_decision_ref`；`status`/`verdict_kind`/`claim_wording` 未动 |
| 2 | `ADR-016` → `## 复审记录` | 新增 `### R3（2026-11-25）— 折算决定`，只引本单据编号与结论 |
| 3 | `docs/06-plan/G2-REVIEW-BRIEF-2026-11-25.md` 第 6 节第 3 项 | 「仍未决」改为已决，指向本单据；第 7 节一句话结论同步 |
| 4 | `CHANGELOG.md` | v2.4.139 一条 |
| 5 | 四个机器常量 | `g2_claim=false`、`s6_status=not_run`、`evidence_valid_for_pass=false`、`auto_pass=false` —— 一个未动 |

**同步后能说什么、不能说什么**：

- 能说：G2 出口清单的「NAT 可达性」一项按 S6 关闭（记账层面），关闭记录带四条明写例外；
  「公网可达性按 eMule 语义结案」可以对外写，但必须同时给出四条例外。
- 不能说：`G2 达标`、`NAT 矩阵通过`、任意可达率/成功率、`restricted-cone 已覆盖`。
  也不因为本单据生效而让 `g2_claim` 变成 `true`——G2 总判仍压在公平性一项上。

---

## 6. 附：为什么「不测 restricted-cone」不能替代这份决定单

两件事的对象不同：

- 「不测 restricted-cone」改的是 **S6 的已知限制怎么写**（ADR-016 §R2），对象是 S6 自身。
- 「是否折算」改的是 **G2 出口清单里这一项由谁关闭**，对象是 G2。

前者的结论是「3/4 就是上限，接受」，后者的结论是「这个上限要不要写进 G2 的出口记录」。
前者成立不蕴含后者，所以必须单独签。**把两者混起来，得到的就是「因为不测了所以 G2 完成了」——
这句话在本仓库里是禁止的。**
