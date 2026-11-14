# T1 双机 + 真实 NAT 全链路验收

> **状态（2026-11-13）：harness 已落地，真实验收 `BLOCKED`；G1 尚未通过，M2 继续 `blocked_by_G1`。**
> [TEST §2](../../../docs/05-quality/TEST.md) 定义 T1 为「局域网双机，真实 NAT 内网，验证 UPnP 与 UDP 唤醒」；
> [ROADMAP §3](../../../docs/06-plan/ROADMAP.md) 规定 **G1 = T0/T1 全链路闭环成功**。T0 的
> `evidence_valid=true` 只覆盖 `127.0.0.1` 单机回环，**不能替代 T1**。

## 1. 本轮做了什么

| 项 | 内容 |
| :--- | :--- |
| M1 跨主机支持 | `Config.NodeSpec.host`（缺省回落 `bind`，T0 完全兼容）、`Config::host/me_host/peer_host/url`；守护进程新增 `--self-host` / `--workdir` / `--sample` / `--downloader`；CLI 按节点 host 连接；`/health` 输出真实 `host` / `machine_id` / `pid` |
| T1 harness | `t1_config.json`（双机启动契约）、`t1_cases.json`（10 条用例 / 40 条断言）、`t1_accept.py`（编排 + 断言 + 报告）、`t1_netprobe.py`（真实 UDP 可达性探针） |
| 证据闸门 | `evidence_valid=true` **仅当**：`--mode real` + 全部用例 PASS 且无 BLOCKED/SKIP + `/health` 实测 A/B 的 `machine_id` 与 `host` 均不同 + 真实 NAT 报告存在且非合成 + 观测来源为 `real` |

`--self-test` 用合成观测验证编排与断言引擎（含「注入坏观测必须判 FAIL」的 T1-00 自检），
`evidence_valid` 恒为 `false`。`--lan-precheck` 用真实二进制和真实 socket 跑全链路，
但 `two_distinct_hosts=false`，`evidence_valid` 恒为 `false`。

## 2. 本轮实跑结果（均为真实观测）

| 运行 | 结果 | 证据 |
| :--- | :--- | :--- |
| `--self-test` | 11/11 PASS | `evidence_valid=false`（合成观测） |
| `--lan-precheck --address-mode aliases` | 10 PASS / 1 SKIP（T1-01 双机身份） | `evidence_valid=false`；A=`127.0.0.2`、B=`127.0.0.3`，验证按节点 host 寻址路径 |
| `--lan-precheck --address-mode nic` | 10 PASS / 1 SKIP（T1-01 双机身份） | `evidence_valid=false`；A/B 绑真实物理网卡 `192.168.1.210` 不同端口，非回环 |
| `--mode real`（无远端前置条件） | 10 BLOCKED / 退出码 2 | 明确报未证明两台不同物理主机，不计入 G1 |

预检中真实量测（两模式一致）：唤醒时延 **P95 = 2.0 ms**（30 样本、0 超时，NFR-04 < 3000 ms）、
复活成功率 **30/30 = 1.0**、伴随种子真实 InfoHash 由 info 字典原始字节独立计算、
第三方 aria2 跨地址完成标准 BT 下载并逐片 SHA-1 校验、NAT 报告为 S6 真实 STUN 样本
（`112.5.60.4:36764`、4/6 台 STUN 应答、公网 UDP 出站可用）。

**这些数字只证明 M1 跨主机代码路径在单机上可跑通，不是 T1、不是 G1、不能解除 M2 门禁。**

## 3. 为什么单机无法伪装成双机（真实量测，非推断）

`t1_netprobe.py` 对 5 个本地 IP 做 25 组真实 UDP `bind(src) → sendto(dst) → recvfrom`：

- 同一 IP：`5/5` 可达。
- 跨 IP：`6/20` 可达，且**全部只在回环别名之间**（`127.0.0.1/2/3`）。
- 物理网卡 `192.168.1.210` ↔ Hyper-V 交换机 `172.26.96.1` 双向不可达；
  回环别名 → 物理网卡一律 `WSAENETUNREACH (10051)`；物理网卡 → 回环别名一律 `WSAEADDRNOTAVAIL (10049)`。

首次 `--lan-precheck` 试图用两块网卡跑，UDP 唤醒确实报了 `10051`、用例如实 FAIL/BLOCKED
（报告 `reports/t1-20261113T083632Z-lan-precheck.json`，本地产物）。因此改为两种可达的地址模式，
并在报告中把 `scope` 标成 `single-host/aliases` 或 `single-host/nic`。

> WSL / 第二台虚拟机在本次环境被安全策略禁止，未做任何绕过尝试。

## 4. 如何在两台真实机器上跑 `--mode real`

前置：两台物理主机处于真实 NAT 内网；B 主机**无需编译**（只需拷 `coldrevived.exe` + 仓库源码/夹具，
从仓库根目录启动即可，配置路径已相对化、无需编辑）。下载器用 **Rayburst 捆绑的 aria2 命令行**
（`aria2-next.exe`，**不是** `rayburst.exe` 图形界面），无需额外安装 aria2。

```bash
# ① A 主机：生成「可移植」配置（路径相对仓库根，拷到 B 后无需改）；写出 tests/e2e/t1/.run/t1-config.json
export T1_HOST_A=<A_IP> T1_HOST_B=<B_IP>
export COLDREVIVE_DAEMON=$PWD/target/release/coldrevived
export COLDREVIVE_CLI=$PWD/target/release/coldrevive-cli
export T0_DOWNLOADER=/path/to/aria2c
python3 tests/e2e/t1/t1_accept.py --mode real --prepare-only
# 把这份 t1-config.json 拷到 B（连同整个仓库源码/夹具；B 上不编译）

# ② B 主机（远端）：从仓库根目录启动守护进程（--config 用拷过去的相对配置，路径自动解析）
cd /path/on/B/ColdRevive
./target/release/coldrevived --config tests/e2e/t1/.run/t1-config.json \
  --node-id B --bind <B_IP> --self-host <B_IP> \
  --workdir tests/e2e/t1/.run/B --downloader /path/on/B/aria2c
# 确认：在 A 上 curl http://<B_IP>:17900/health 应返回 status=ok

# ③ A 主机：确认 B 已起，正式跑完整 harness
export T1_HOST_A=<A_IP> T1_HOST_B=<B_IP> T1_REMOTE_PRESTARTED=1
python3 tests/e2e/t1/t1_accept.py --mode real --refresh-nat
# 报告 tests/e2e/t1/reports/t1-*-real.json；evidence_valid=true 且全用例 PASS → G1 解锁
```

`--refresh-nat` 会现场调用 `tests/spikes/s6/stun_probe.py` 重新采集真实 NAT 报告；
不带该参数则复用 S6 先导报告。`--mode real` **不会**替你远程拉起 B 进程，也不接受
`T1_REMOTE_PRESTARTED` 之外的任何隐式确认。配置现已相对仓库根路径，B 端从仓库根启动即可，
**无需任何路径编辑**（新增 `--prepare-only` 仅生成配置、不触发远端守卫）。

## 4.1 Windows CMD 操作（A/B 机具体指令）

> 以下均为 **Windows 命令提示符（CMD）** 写法。`set` 设置的环境变量**只在该 CMD 窗口内有效**，
> 必须与执行命令在同一个窗口；路径引用用 `%CD%`；命令续行用 `^`（不是 bash 的 `\`）。
> 因 config 已相对仓库根，B 机拿到文件后**零路径编辑**，进仓库根目录直接启动。

> **下载器（关键）：用 Rayburst 捆绑的 aria2 命令行程序 `aria2-next.exe`，而不要用图形界面 `rayburst.exe`。**
> ColdRevive 守护进程会以子进程方式调用下载器并传入 aria2 专属参数
> （`--bt-tracker` / `--listen-port` / `--enable-dht=false` …），且等待其**下载完成后退出**。
> Rayburst 安装目录为 `C:\Program Files\Rayburst\`，其捆绑的 `aria2-next.exe` 实为 **aria2-next 3.1.0**
> （完整路径 `C:\Program Files\Rayburst\aria2-next.exe`）。注意是 `aria2-next.exe`，**不是** `rayburst.exe`
> 那个 Electron 图形界面，后者不认 aria2 参数、也不会在下载后退出。已实测该文件 `--version` 输出
> `aria2-next version 3.1.0` 且接受 ColdRevive 传入的全部参数（`--bt-tracker` / `--enable-dht=false` /
> `--listen-port` / `--seed-time=0` / `--file-allocation=none` 等）。**无需另外安装 aria2，也无需启动图形界面。**
> 若你的安装路径不同，按实际位置替换下面所有 `C:\Program Files\Rayburst\aria2-next.exe`。

### 前提（A、B 两台都要）
```cmd
REM 放行端口（管理员运行 CMD；A 示例，B 把 17800→17900 / 17801→17901 / 17802→17902）
netsh advfirewall firewall add rule name=cr-http dir=in action=allow protocol=TCP localport=17800
netsh advfirewall firewall add rule name=cr-wake dir=in action=allow protocol=UDP localport=17801
netsh advfirewall firewall add rule name=cr-bt   dir=in action=allow protocol=TCP localport=17802
REM A 上确认 Python：py --version（不行就换 python）。下载器用 Rayburst 捆绑的 aria2-next，无需额外安装。
```

### ① A 机：编译 + 生成可移植配置（同一个 CMD 窗口）
```cmd
cd /d C:\Users\bemin\WorkBuddy\ColdRevive

REM 编译 release（产物 target\release\coldrevived.exe）
cargo build --release --workspace --offline

REM 设置环境变量（仅本窗口有效）
set COLDREVIVE_DAEMON=%CD%\target\release\coldrevived.exe
set COLDREVIVE_CLI=%CD%\target\release\coldrevive-cli.exe
set T0_DOWNLOADER=C:\Program Files\Rayburst\aria2-next.exe
set T1_HOST_A=<A 的 IP，例如 192.168.1.210>
set T1_HOST_B=<B 的 IP，例如 192.168.1.211>

REM 仅生成配置，不触发远端守卫
py tests/e2e\t1\t1_accept.py --mode real --prepare-only
```
生成文件：`tests\e2e\t1\.run\t1-config.json`（四路径已相对化，拷到 B 不用改）。

### ② 把文件弄到 B（B **不需要源码**，只拷运行必需项）

B 的守护进程只跑预编译的 `coldrevived.exe`（std-only、自包含），**不编译、不需要 Rust 源码 / Cargo.toml / `target` 构建目录 / Python harness / 其它二进制**。
它运行时按 **相对仓库根** 读取以下文件（路径来自 `t1-config.json`），所以 B 启动守护进程的目录（`C:\ColdRevive`）下必须存在这些相对路径：

- `tests\fixtures\t0\golden\wakeup_req.v1.json` 与 `wakeup_ack.v1.json`（唤醒签名夹具）
- `tests\e2e\t1\.run\t1-config.json`（配置本身）
- `tests\e2e\t1\.run\B\`（workdir，运行时自动创建）

> **为什么之前说"拷整仓库"**：纯粹是为了让这些相对路径能解析；整仓库里有用的其实只有 `tests/` 子树 + 那个 exe。
> **样本文件 `sample_path` 仅 A 节点（holder）启动时才读取**（`bootstrap_holder` 只在 `node_id=="A"` 时执行），**B 节点不会读，可不拷**（拷了也无害）。

```cmd
REM 推荐：只拷 tests 子树 + 编译好的 exe（远小于整仓库）
robocopy C:\Users\bemin\WorkBuddy\ColdRevive\tests E:\ColdRevive\tests /E
copy C:\Users\bemin\WorkBuddy\ColdRevive\target\release\coldrevived.exe E:\ColdRevive\
REM 然后把 E:\ColdRevive（仅 tests + exe）拷到 B 的 C:\ColdRevive，B 从该目录启动守护进程

REM 备选：若想最省心，也可整仓库拷（跳过 target/.git），但源码其实用不上
REM robocopy C:\Users\bemin\WorkBuddy\ColdRevive E:\ColdRevive /E /XD target .git
REM robocopy C:\Users\bemin\WorkBuddy\ColdRevive\target\release E:\ColdRevive\target\release coldrevived.exe
```
**B 的下载器同样用其 Rayburst 捆绑的 aria2-next**：`C:\Program Files\Rayburst\aria2-next.exe`（路径以 B 实际安装位置为准；B 可不拷此文件、直接用已安装路径）。

### ③ B 机：从仓库根目录启动守护进程（CMD 窗口，保持开着别关）
```cmd
cd /d C:\ColdRevive

REM 确认夹具在
dir tests\fixtures\t0\golden\wakeup_req.v1.json

REM 启动 B 守护进程（downloader 用 B 上 Rayburst 捆绑的 aria2-next，路径带空格必须加引号）
coldrevived.exe --config tests\e2e\t1\.run\t1-config.json ^
  --node-id B --bind <B_IP> --self-host <B_IP> ^
  --workdir tests\e2e\t1\.run\B --downloader "C:\Program Files\Rayburst\aria2-next.exe"
```
- 若报「找不到 VCRUNTIME140.dll」→ `winget install Microsoft.VCRedist.2015+.x64` 装一次即可。
- 在 **A 机**验证 B 已起：`curl http://<B_IP>:17900/health` 应返回含 `status=ok`。

### ④ A 机：正式跑 T1 真实双机（回 ① 那个 CMD 窗口）
```cmd
set T1_REMOTE_PRESTARTED=1
py tests\e2e\t1\t1_accept.py --mode real --refresh-nat
```
- 报告：`tests\e2e\t1\reports\t1-*-real.json`；`evidence_valid=true` 且全用例 PASS → **G1 解锁**。

### 收尾
- 以后 A 重新 `cargo build --release` 后，**只需把新 `coldrevived.exe` 再拷到 B 覆盖**；golden 与 config 路径不变则不动。B 永远零工具链。
- 跑通后本地提交（不 push）：`git add -A && git commit -m "..."`

## 5. 已知 T1 设计缺口（必须在 G1 前解决）

1. **M1 CLI MVP 没有任何 NAT 穿透实现**：无 UPnP IGD / NAT-PMP / PCP、无 STUN 打洞、无中继。
   S6 先导样本还显示本机 UPnP IGD 与 NAT-PMP **均不可用**。若 T1 的两机被不同 NAT 隔开，
   当前 `WAKEUP_REQ` 的 UDP 单播与 tracker 返回的私网 peer 地址都不可达，T1 会真实 FAIL。
   → 需要在 M1 补穿透栈（打洞 + 中继/LowID 回退），或把 T1 限定为「同一 NAT 内两机」并写清边界。
2. `T1-08` 目前只做**单侧** NAT 特征取证；S6 判据要求的四类 NAT（含 CGNAT）可达率矩阵属 M2。
3. `--mode real` 的 B 端 `/health` 与 CLI 取数依赖两机路由可达，须先确认防火墙放行
   `http_port` / `wake_udp_port` / `bt_port`。

## 6. 相关文件

- 用例与阈值：[t1_cases.json](t1_cases.json) · [t1_config.json](t1_config.json)
- 编排：[t1_accept.py](t1_accept.py) · 环境探针：[t1_netprobe.py](t1_netprobe.py)
- 报告目录：`tests/e2e/t1/reports/`（本地产物，按 T0 惯例不入库）
- 门禁定义：[TEST §2](../../../docs/05-quality/TEST.md) · [ROADMAP §3](../../../docs/06-plan/ROADMAP.md) ·
  [G0-REVIEW](../../../docs/06-plan/G0-REVIEW.md)
