# 测试过程与结果归档（TEST ARCHIVE）

> 建立于 2026-11-24。本目录是**测试归档的唯一权威账本**：
> 任何真实测试（cargo test、e2e、探针、抓包、远程主机检查、手动 A/B）
> 在过程与结果被文档、变更日志或门禁判断引用之前，必须先在此登记。
> 归档不改变任何门禁常量：`g2_claim=false`、`s6_status=not_run`、
> `evidence_valid_for_pass=false`、`auto_pass=false`。

## 1. 为什么需要它

2026-11-24 审计发现：项目此前**没有统一的测试归档机制**。

- 机器报告分散落在各 `tests/e2e/*/reports/`，其中大量目录被 `.gitignore` 排除，
  clone 仓库后无法复现；
- 测试过程（命令、环境、判据、失败原因）主要写在 gitignored 的
  `.workbuddy/memory/YYYY-MM-DD.md` 工作日志里，甚至只在会话中；
- 部分测试只留下结论，没有原始证据；少数报告被后续运行覆盖。

因此本项目采用**两层归档**：

1. **原始证据**（raw）：JSON 报告、帧日志、抓包、命令行 stdout/stderr。
   体积大或含本机地址的原始文件可以继续留在 gitignored 路径；
   必须能在账本中按路径 + size + sha256 定位。
2. **归档账本**（ledger）：`TEST-RUNS-YYYY-MM-DD.md`，版本化、追加式。
   每次测试一条记录，含命令/操作、判据、结果、证据路径与哈希、门禁影响。
   聊天记录和工作日志**不算**归档，只能作为待补录的来源。

## 2. 强制规范（以后每次测试都要遵守）

一次真实测试结束时，执行：

```bash
python3 scripts/archive_test_run.py \
  --date YYYY-MM-DD \
  --suite "FR-24" \
  --title "一句说明" \
  --result PASS \
  --command "实际执行的命令或操作（可重复）" \
  --criteria "通过判据" \
  --gate-impact "none" \
  --evidence tests/e2e/w24-pub/reports/xxx.json
```

规则：

- `--result` 只允许 `PASS / FAIL / BLOCKED / SKIPPED / INFO`；失败与 BLOCKED 同样必须归档。
- 证据文件必须存在，脚本自动登记 size + sha256；确实没有原始证据时
  必须显式传 `--missing-evidence`，并在 `notes` 写明原因，禁止补造。
- 密码、token、私钥、SSH 凭据一律不写进归档；脚本会对常见写法脱敏。
  含用户隐私或本机地址的原始回传（如家宽 kit 的 `results*.zip`）只登记路径/哈希，不入库。
- 门禁常量不得由归档脚本或测试脚本自动改写；`gate_impact` 只能是说明。
- 账本为**追加式**：更正用新条目说明，不删除或改写旧条目。

### 2.1 sha256 的口径（CRLF 陷阱，2026-11-25 补）

账本里的 `sha256` 是**登记时证据文件的实际字节**。本仓库在 Windows 上工作，
`core.autocrlf` 会把文本文件在提交时归一化成 LF、检出时还原成 CRLF，于是：

- 未在 `tests/ARCHIVE/evidence/.gitattributes` 里声明 `-text` 的文件，
  `git show HEAD:<path> | sha256sum` 与账本里的值**可能不同**（差的是 `\r`）。
  这不是篡改，是换行归一化；比对前先看该文件有没有 `-text`。
- 需要**字节级可复现**的证据（原始探针输出、抓包派生、与门禁判断直接相关的 JSON），
  必须在 `.gitattributes` 里加 `-text`。已锁的有：`s6-verdict-20261117/*`、
  `nfr02-idle-cpu-20261125.raw.json`、`nfr02-engine-idle-priority-20261125.{on,off}.json`、
  `regression-full-20261125e.log`、`nfr02-macos-idle-priority-20261125.json`。
  对这些文件，`git show HEAD:<path> | sha256sum` 必须等于账本值；不等就是真问题。
- 新证据入库时：先定 `-text`，再登记哈希，最后用上面那条命令自查一遍。

## 3. 目录内容

| 文件 | 用途 |
| :--- | :--- |
| `README.md` | 本规范与审计结论 |
| `TEST-RUNS-YYYY-MM-DD.md` | 逐次测试账本（追加式，版本化） |
| `BACKFILL-TEST-RUNS.md` | 从历史 gitignored 工作日志生成的补档；由 `scripts/build_test_archive_backfill.py` 重建，勿手改 |
| `NOT-TESTED.md` | 明确区分「测过但证据缺失」与「根本没测」，防止把缺口写成证据 |
| `evidence/` | 为可移植性而专门保存的轻量原始证据（报告/日志），路径与 sha256 在账本中登记 |

重建历史补档：

```bash
python3 scripts/build_test_archive_backfill.py
```

## 4. 历史补档的边界

`BACKFILL-TEST-RUNS.md` 忠实搬运 2026-11-09～11-24 工作日志中的测试相关行，
并对能解析的 evidence 路径计算 sha256。它证明「当时记录过什么」，
**不补造当时没有保存的原始证据**。标为 `log-only` 的条目不得升级为
门禁证据；`NOT-TESTED.md` 列出的项目必须如实保持未验证状态。
