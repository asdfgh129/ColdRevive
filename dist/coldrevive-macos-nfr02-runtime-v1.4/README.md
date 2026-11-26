# ColdRevive NFR-02 macOS 真机运行包 v1.4

给 **macOS 真机**补一份可归档的证据：验证 `PRIO_DARWIN_BG` 分支在真实 darwin 上是否按读回谓词生效，并做负向对照。注意 Darwin 两侧编码不同：`setpriority` 传 `PRIO_DARWIN_BG=0x1000`，`getpriority` 返回布尔值 `1`（后台）/`0`（前台），不能用 `0x1000` 去比读回值。

- 源码快照：`v2.4.152`，HEAD `e698d4bef371aeb7b6922c84c2d593051800a466`（见 `SOURCE_INFO.txt`）
- 判据来源：`docs/06-plan/G2-EXIT-RECORD-2026-11-25.md` v1.5 §10「真机收尾清单」
- 本包**不**修改 `g2_claim`，**不**自动宣布 NFR-02 达标 / macOS 已验证 / 三平台达标。
- **不需要第二台机器**：握手唤醒功能复核由同一台 mac 上的探针连本机 `127.0.0.1:19602` 完成。
- 所有动作都在 `127.0.0.1`，不连公网 KAD、不连中继、不向外部节点发包。

## 1. 前置条件

1. macOS（Intel 或 Apple Silicon 都行；不提供预编译二进制，在本机构建）。
2. Xcode Command Line Tools：`xcode-select --install`，然后 `xcode-select -p` 能打印路径。
3. Rust（rustup）：`cargo --version` / `rustc --version` 可用。
4. Python 3.8+ 与 `curl`（macOS 通常自带）。
5. 首次构建需要联网拉 crates.io 依赖；之后可在同一台机器重复运行。
6. 本机 `127.0.0.1:19600/19601/19602` 空闲（有旧 `coldrevived` 就先停掉）。

## 2. 运行

```bash
unzip coldrevive-macos-nfr02-runtime-v1.4.zip
cd coldrevive-macos-nfr02-runtime-v1.4
bash run_all.sh
```

**直接用 `bash run_all.sh`，不要先 `chmod` 再 `./run_all.sh`。** 从聊天/网页下载的 zip 会被 macOS 打上 `com.apple.quarantine`，`./run_all.sh` 会被 Gatekeeper 拦成 `operation not permitted`；`bash run_all.sh` 只读脚本内容，不受这个限制。若确实想用 `./`：

```bash
xattr -dr com.apple.quarantine .
chmod +x run_all.sh
./run_all.sh
```

脚本六个阶段：

1. `cargo test -p cr-engine --lib --locked -- --test-threads=1`  
   其中 `idle_applies_to_the_calling_thread_only` 与 `runtime_worker_task_runs_at_idle_priority` 在 macOS 上会断言 `policy == 1`（后台布尔值）；两条 darwin 判定用例也必须在，其中一条专门拒绝把设置侧 `0x1000` 当读回值。
2. `cargo build --bin coldrevived --locked`（debug 构建即可）。
3. 用 `config/daemon-on.json`（`engine.idle_priority=true`，`guard_handshake=true`）起本地 holder，抓 3 次 `/health` 快照。
4. **同机**握手唤醒功能复核：探针连 `127.0.0.1:19602`，发 68 B 标准 BT 握手，取回整片样本验 md5；结果落 `guard-wake.json`。
5. 用 `config/daemon-off.json`（`idle_priority=false`）起负向对照，再抓 3 次 `/health` 快照。
6. `probes/collect_evidence.py` 汇总成 `results/<运行时间戳>/summary.json`。

v1.4 修复首次真机 FAIL 的两处缺陷：①`PRIO_DARWIN_BG` 常量由错误的 `0x10000` 改为 Apple 头文件定义的 `0x1000`；②把 `cr-ed2k` 通过 `#[path]` 引用的三个 `tests/spikes/**` 源文件纳入包，修掉 `couldn't find file sha1.rs`。仍保留 v1.3 的 `PYTHON` 环境隔离与 `bash run_all.sh` 用法。

本机复跑另带五处修复（见 `SOURCE_INFO.txt`，随 v2.4.154 入库）：①`cr-nat/tcp_punch.rs` 把 `#[cfg(unix)]` 中硬编码的 Linux errno 符号、socket 常量、`EINPROGRESS` 和 `poll` nfds 类型拆成 Linux/Darwin 两路，修掉 macOS 上的 `___errno_location` 链接错误；②`run_all.sh` 启动 daemon 时加 `exec`，让 `$!` 指向真正的 daemon PID，避免 `kill` 后残留旧进程让负向对照假失败；③`prio.rs` 把设置侧标志位 `0x1000` 与读回侧布尔值 `1` 拆成两个常量，按 `man 2 setpriority` 与 XNU 源码修正判读；④`coldrevived/data_plane.rs` 在 `TcpStream::from_std` 前设非阻塞（原先设阻塞会被 tokio 1.54.1 拒绝）；⑤`cr-nat/tcp_discovery.rs` 单测夹具先 bind 再 accept（带超时）并等监听就绪，修掉端口竞态挂死。五处都**没有修改任何判定阈值**，只是让实现/夹具符合平台与库 API 语义。

## 3. 预期结果

`summary.json` 的 `checks` 里应有五项 `pass` 和一项 `not_verified`：

| 检查 | 期望 |
|---|---|
| `cr-engine unit tests on macOS` | `pass`：退出码 0，5 条关键用例都出现 `... ok` |
| `coldrevived builds on macOS` | `pass`：构建退出码 0、二进制存在 |
| `daemon idle_priority on` | `pass`：3/3 快照 `requested=true, applied>=1, failed=0, unsupported=0`，且 `seed_tasks>=1` |
| `wake path functional (local BT handshake -> sample md5)` | `pass`：握手计数与 `woken` 增长、`seed_active>=1`、收到 262144 B 且 md5 与样本一致 |
| `daemon idle_priority off (negative control)` | `pass`：3/3 快照 `requested=false, applied=0, failed=0, unsupported=0` |
| `wake path not downgraded (guard/data-plane)` | `not_verified`：Darwin `getpriority(PRIO_DARWIN_THREAD, 0)` 只能读调用线程，本包没有给唤醒线程加进程内读取器 |

第 4 步只证明**功能链路还通**；最后一项按 DOC-34 §10 允许如实写「无法逐线程区分」，**不要**用功能复核或进程级读数顶替。`gate_claim` 固定为 `null`。

## 4. 回传

把整个本次运行目录压缩后发回：

```bash
cd results
zip -r nfr02-macos-<运行时间戳>.zip <运行时间戳>
```

里面应包含：`preflight.txt`、`platform.json`、两份 cargo 日志与 `.exit`、两份 daemon 日志、6 份 `/health` JSON（外加 2 份 probe 快照）、`guard-info.txt`、`guard-wake.json` / `.stdout` / `.exit`、pid/ready 文件、`03-collect.stdout` 与 `summary.json`。

请同时说明：机器型号 / macOS 版本 / 是否 Intel / 运行时是否改过任何配置或阈值。

## 5. 排错

- `./run_all.sh` 报 `zsh: operation not permitted` → 下载包的 quarantine 属性，改用 `bash run_all.sh`，或先 `xattr -dr com.apple.quarantine .`。
- `bash run_all.sh` 报 `找不到 ??请先装` → 你的 shell 把 `PYTHON` 设成了 `?`；改用 v1.4（已忽略该变量），或临时用 `env -u PYTHON bash run_all.sh`。
- `bash run_all.sh` 报 `line 18: tool?: unbound variable` → 你用的是 v1.0/v1.1；改用 v1.4，或临时把 `set -uo pipefail` 改成 `set -o pipefail`。
- `xcode-select -p` 报错 → 先装 Command Line Tools；`aws-lc-sys` 这类 C 依赖在 mac 上没有工具链会直接失败。
- `127.0.0.1:19600 已有服务` → 停掉旧进程再跑；脚本故意拒绝，避免把旧进程的 `/health` 当成新证据。
- daemon 提前退出 → 看 `results/<运行时间戳>/daemon-on.log` / `daemon-off.log`。常见原因是端口冲突或 workdir 权限。
- `applied=0` 且 `failed>0` → 真机读回没通过（期望读回 `1`，不是 `0x1000`），**不要**手工改成 pass；把日志原样回传。
- 握手唤醒功能项 `blocked` → 看 `guard-info.txt` 是否拿到 infohash、`guard-wake.stdout` 里的报错；如实回传，不要改阈值。
- 构建或网络失败导致部分阶段没跑 → `summary.json` 中相应项会是 `blocked`/`fail`，同样如实回传。

## 6. 口径提醒

在所有者书面改口径之前，仍然只能说：

- ✅ macOS 分支已实现、判定逻辑有单测、分支经类型检查；
- ✅ 本次真机运行产生了 `/health` 读回证据与负向对照；
- ✅ 同机握手唤醒功能复核通过（若该项为 pass）；
- ❌ 不说「NFR-02 三平台达标」「macOS 已验证」——即使六项中五项 `pass`，最后一项仍是
  `not_verified`，且口径改判只能由所有者书面决定；
- ❌ 不把 `not_verified` 的唤醒线程优先级项写成通过；
- ❌ 不改判据、不调阈值、不重抽样后改判。
