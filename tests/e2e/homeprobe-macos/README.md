# ColdRevive 家宽取证 kit（macOS Ventura / Sequoia）

> 版本 1.4 · 2026-11-21 · 目标机器：macOS 13 Ventura / 15 Sequoia；x86_64（Intel）与 arm64 均可
> （分发包名保留 `intel` 只是沿用首版命名，脚本是纯 Python，不含架构相关代码）。
> 依赖：**只要 macOS 自带的 Python 3**（或任意 Python 3.8+）；不需要 Rust、Homebrew、管理员权限。
> 本 kit 只采集证据，**不会自动判 pass**，也不会修改项目门禁常量：
> `g2_claim=false`、`s6_status=not_run`、`evidence_valid_for_pass=false`。

## 1. 它能回答什么

| 探针对应的欠账 | 能回答 | 不能回答 |
| :--- | :--- | :--- |
| S6 / FR-17：新的真实 NAT 出口样本 | 家宽出口的映射行为、过滤行为、NAT 子型、UPnP/NAT-PMP 可用性、是否疑似 CGNAT | 单个出口不是 NAT 矩阵；不能折算可达率，不能翻转 `s6_status` |
| FR-18：公网 DHT 只读可达性 | 家宽能否与自有公网 DHT 节点和公共引导节点完成 KRPC ping / get_peers；`token_len` 与 peers | 不发 `announce_peer`，不能宣称加入 Mainline；不启用 daemon DHT |
| FR-24：KAD 专用 UDP 出口 | 家宽能否让真实 KAD `0xE401 BOOTSTRAP_REQ` 得到公网节点应答 | 不发 SEARCH / PUBLISH，无法解除 G0 `protocol_scope` 限制；0/10 不能区分“被过滤”与“联系人失效”，需与正例对照 |
| 平台覆盖 | 首个 macOS Intel 真实运行样本（OS/CPU/Python/公网出口） | 不等于全平台兼容认证 |

**代码/授权类欠账不靠换网络解决**：FR-01 目录监控、FR-04 SQLite、FR-05 TTL、FR-09 限流、FR-10 守护进程多样种、FR-25 daemon 接线与跨分片、FR-19/20/21 Web UI/`rpc-secret`/WebSocket、可靠层拥塞语义、TCP 打洞接 BT 数据面、G0 scope 放宽、SRS 评审与 FR-24 编号重复等，仍按原状开放。详见仓库 `docs/06-plan/STAGE-DEBT-AUDIT-2026-11-21.md`。

## 2. 运行步骤

1. 解压得到的目录，例如 `coldrevive-homeprobe-macos-intel-v1/`。
2. 打开“终端”，进入该目录：
   ```bash
   cd ~/Downloads/coldrevive-homeprobe-macos-intel-v1
   bash run_all.sh
   ```
   **优先用 `bash run_all.sh`**：它直接把脚本交给 bash 读取，不需要可执行位，也不会被下载 ZIP 的 macOS 隔离属性（quarantine）拦住。若你更想用 `./run_all.sh`，先按下面的排障步骤处理。
   如果没有 Python 3，先执行一次 `xcode-select --install`（只装命令行工具，不需要完整 Xcode）。
3. 全程约 1–2 分钟。它只发低速、只读的标准 STUN / KRPC / KAD bootstrap 报文，不下载文件、不安装软件、不发布任何 Key。
4. 结束后回传整个 `results/` 目录（里面有 `summary.json`、各探针 JSON、以及记录退出码的 `.exit` 文件）。**脚本启动和结束都会打印 `results:` 的绝对路径**，按那个路径找，不要去 Downloads 或其他目录找。**不要把 kit 放在“废纸篓/.Trash”里跑**（可能被清空）；解压到 `~/Downloads` 或桌面固定目录。
5. 想单独跑某一项：
   ```bash
   python3 probes/nat_behavior.py --self-test
   python3 probes/platform_info.py --out results/platform.json
   python3 probes/dht_readonly_probe.py --self-test
   ```

## 2.1 IP 绑定、动态 IP 与 Sequoia

- **不绑定本机 IP**：所有 UDP 套接字都绑 `0.0.0.0`（或 `""`）的**临时端口**，由系统按当前默认路由选出口。`run_all.sh` 不给 KAD 探针传 `--source-port`，所以也没有固定源端口。
- **公网 IP 是运行时观测值**：`platform_info.py` 通过 HTTP 尽力查询，`stun_probe.py` / `nat_behavior.py` 用 STUN 映射地址交叉确认，结果写进 JSON。家里拨号换 IP 不需要改配置；换 IP 后重跑即可，报告里带 `recorded_at` 便于区分场次。
- **配置里出现的 IP 是目标端，不是本地绑定**：`119.29.150.254:3478`（自有 STUN）、`82.157.68.159:6881`（自有 DHT）、`config/kad-targets.json` 的 10 个 KAD 联系人，以及脚本自检里 `127.0.0.1` / 示例 IP，都不是要求家宽具备的地址。
- **Sequoia 可用且不限于系统 Python**：Command Line Tools 自带 Python 3.9.6；用户实跑的 macOS 15.5 / x86_64 上是 Python 3.14.5，本 kit 两种都跑通。Apple Silicon 上 `platform.machine()` 会记成 `arm64`，不影响判据。
- **Sequoia 的本地网络权限**：若首次运行弹出“本地网络”授权，请允许“终端”；否则只影响 SSDP 发现 UPnP IGD 与 NAT-PMP 这类**局域网**探测，会把相应项如实记为不可用，不影响 STUN / DHT / KAD 的出向取证。若没弹窗无需处理。

## 2.2 排障：`operation not permitted` / `permission denied`

从浏览器或聊天工具下载的 ZIP 解压后，macOS 会给文件加 `com.apple.quarantine` 扩展属性。表现是即使 `chmod +x` 已把权限改成 `-rwxr-xr-x`，执行仍报：

```
zsh: operation not permitted: ./run_all.sh
```

任选一种办法：

```bash
# 办法 A（最省事，推荐）：绕过执行位，直接让 bash 读取脚本
bash run_all.sh

# 办法 B：先去掉隔离属性，再正常执行
xattr -dr com.apple.quarantine .
chmod +x run_all.sh
./run_all.sh

# 想先确认是不是隔离属性：
xattr -l run_all.sh
```

注意：`run_all.sh` 内部对每个探针都是 `"$python3" probes/xxx.py`，**不会直接执行 `probes/*.py`**，所以探针文件带 quarantine 也不影响 `bash run_all.sh`。若执行时弹出“本地网络”权限请求，请允许终端访问（只影响局域网发现项，不影响 STUN/DHT/KAD 出向证据）。

## 2.3 KAD 联系人清单（v1.3 已换掉失效联系人）

- 首轮家宽回传的 `NO_REPLY (0/5)` **不计入 S2 证据**：那 5 个目标来自 2026-11-20 的 `tests/e2e/w24-pub/reports/public-search.json`，本身就是过期联系人（原运行中也只有 1/5 拿到 BOOTSTRAP_RES），且 KAD 探针的 `0/N` 无法区分“出口被过滤”和“联系人已死”。
- v1.3 已用**新鲜 BOOTSTRAP_RES** 重建 `config/kad-targets.json`：2026-11-21T12:29:40Z 从真 aMuleD 的 LAN `0xE4/0xE5` BOOTSTRAP_RES 解出 20 个联系人，经 `tests/spikes/s2/bootstrap_res_to_targets.py` 按 **KAD host-order LE 反序**解码（该脚本 `--selftest` 通过），取前 10 个公网可路由目标。
- **正例对照**：同一份清单在 Windows 主机上以 `--sample-size 10 --rate-limit 0.2` 实跑，**8/10** 收到真实 `0x09 BOOTSTRAP_RES`（明文 523 B 与 zlib 打包 520–521 B 两种帧都出现），证明清单本身有效、字节序正确。
- 因此请**重新下载 v1.3+ 包再跑一次**（`run_all.sh` 已改为 `--sample-size 10`）。若这次仍是 `0/10`，可以和上面的正例直接对照，排除“联系人失效”这个混淆项；但 `0/10` 依然只是诊断，不能自动写成 S2 结论。
- **怎么判断你跑的是不是旧包**：控制台 KAD 段落若列出 **5 个**目标、或出现 `run_all.sh: line 30: code?: unbound variable`、或 `kit:` 路径在 `~/.Trash/`，都是 v1.2 或更早的副本。新包控制台会打印 `results: .../results` 绝对路径，KAD 列出 **10 个**目标，且最后生成 `results/summary.json`。可用 `shasum -a 256 coldrevive-homeprobe-macos-intel-v1.zip` 校验你下载的文件。

## 3. 退出码口径

| 退出码 | 含义 |
| :--- | :--- |
| 0 | 探针执行完成（STUN 全超时也算“完成”，不是 pass） |
| 1 | 脚本/参数错误，或环境缺依赖 |
| 2 | KAD 探针专属：0/N 应答，记为 `NO_REPLY`；**不能单独作为被过滤的充分证据** |

`run_all.sh` 把每次退出码写进 `results/<name>.exit`，即使失败也继续跑下一项，避免“因为一个探针挂了就丢掉其他证据”。

## 4. 结果怎么读

- `results/summary.json`：`observed.stun` / `observed.nat_behavior` / `observed.dht` / `observed.kad_egress` 是原始观测；`missing` 列出未采到的项目。
- NAT 判定只有在**对照成立**时才给具体子型；对照不全时是 `undetermined`，这是诚实结果，不是失败。
- DHT 的 `token_len=0` 与超时都按原样记录；自有节点回查能拿到 `peers=1` 也只说明这条既有宣告仍可查。
- KAD 的 0 应答必须结合“联系人是否仍存活”判断；清单里的 5 个联系人来自 2026-11-20 的 W24-pub 证据，可能已经失效。

## 5. 覆盖范围声明

- 目标端固定：`119.29.150.254:3478`（项目自有 RFC 5780 STUN）、`82.157.68.159:6881`（自有 DHT）、KAD 联系人清单、公共 DHT 引导节点。
- 不向公共网络写入任何测试 Key / infohash / 文件；KAD 探针需要显式 operator attestation 参数，`run_all.sh` 已包含该参数——**运行 `run_all.sh` 即表示你确认自己是实验操作者并同意上述低速只读探测**。
- 公网出口 IP 由 HTTP 尽力查询，失败不影响 STUN 观测；不得用本机内网 IP 冒充公网 IP。
- 证据判读仍由项目所有者完成；任何脚本都不得把本 kit 的结果写回 `tests/spikes/spikes.json`。
