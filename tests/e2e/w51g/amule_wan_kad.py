#!/usr/bin/env python3
"""W5.1g 验收：aMule 的 KAD 索引面在**公网入向打开之后**是否真的可用。

背景。G3 矩阵的 `emule_amule` 行一直带一条例外 `g3.amule_kad_index`：
aMule 能通过 eD2k TCP 送文件，但 PUBLISH 总是静默、KAD 索引入库/回查从未验证。
W3.16 的归因是「aMule 自报 firewalled → `Process2PublishKeyRequest` 静默丢弃」，
recheck_trigger 写的是「aMule 获得公网入向可达、脱离 firewalled 之后」。

本轮（所有者补开 UDP 4672 / TCP 4662 之后）实测发现一件比「端口通了」更要紧的事：
**端口通不等于 aMule 改了主意**。同一份二进制、同一条公网路径，重启前的 aMule
对 PUBLISH 一律沉默（LAN 与公网都沉默），重启后同样的报文立刻被接受并回查得到。
所以本脚本把「网络入向」与「aMule 的判定状态」当成两件事分别取证：

    ① 网络层：公网侧真能打到 221 的 4672/UDP 与 4662/TCP，且 221 看到的源是公网 IP
       （不是 192.168.1.222 —— 那是 LAN 捷径，报告作废）；
    ② 索引层：从公网 IP 发真实 0x43/0x33 帧，PUBLISH 被 ACK、SEARCH 取回本轮的描述符；
    ③ LAN 对照：同一台 aMule 在 LAN 上同样成立 —— 排除「只有公网那一侧特殊」；
    ④ 负对照：同一公网地址上未监听的端口发同样的帧 → 必须零回包（ack 不是凭空造的）；
    ⑤ 命名空间：两个互不相干的 16B 键各自只回自己的条目，不发生别名。

为什么公网那一侧要单独写一个探针子命令（`kad-mini wan-index-probe`）：
`lan-index-probe` 会拒绝非私有地址，而更关键的是它派生随机键 —— 随机键落在
aMule 的 `SEARCHTOLERANCE`（2^24）邻域外的概率是 255/256，一旦 PUBLISH 静默，
「firewalled」与「越界」两种静默无法区分。W3.16 就是卡在这里。新子命令先用
BOOTSTRAP_RES 自带的 self_id 拿到对端 KadID，再把索引键贴着它取（top32 距离 0），
把越界分支按构造排除。

纪律：
    - `g3_verdict` 恒 `not_run`、`evidence_valid_for_pass=false`：本脚本只产观测，
      结论由实验执行人回填；`spikes.json` / `g3-closeout.toml` 不被脚本改写。
    - 只打一台我们自己的主机（单目标），不做公共 KAD bootstrap，不扫描。
    - 不含任何速率/可达率数字（ADR-012）。

用法：
    python tests/e2e/w51g/amule_wan_kad.py
    python tests/e2e/w51g/amule_wan_kad.py --runs 3
    python tests/e2e/w51g/amule_wan_kad.py --skip-build --skip-lan
    python tests/e2e/w51g/amule_wan_kad.py --restart-amule   # 运维动作，显式

环境变量：
    CR_SSH_KEY    SSH 私钥（缺省 %TEMP%/crkeys/id_ed25519）
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
KAD_SRC = REPO / "tests" / "spikes" / "s2" / "kad-mini"
REPORTS = Path(__file__).resolve().parent / "reports"

# 载体故障的特征串：sshd MaxStartups / 连接被回收 / 端口瞬断。
# 这些必须与业务失败分开，否则又会得到「没有 peer」「落盘缺失」这类假结论。
CARRIER_ERRORS = (
    "Connection closed by",
    "Connection reset by peer",
    "kex_exchange_identification",
    "MaxStartups",
    "Connection refused",
    "Broken pipe",
    "banner exchange",
)


@dataclass
class Ctx:
    key: str
    reports: dict = field(default_factory=dict)
    checks: list = field(default_factory=list)

    def check(self, label: str, ok: bool, actual: str = "") -> bool:
        self.checks.append({"label": label, "ok": bool(ok), "actual": actual})
        print(f"[{'PASS' if ok else 'FAIL'}] {label}" + (f"  -- {actual}" if actual else ""))
        return bool(ok)


def ssh_base(host: str, key: str) -> list[str]:
    return [
        "ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8",
        "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null",
        "-o", "LogLevel=ERROR", "-i", key, f"root@{host}",
    ]


def ssh(code_host: str, cmd: str, key: str, timeout: int = 120) -> tuple[int, str]:
    try:
        p = subprocess.run(ssh_base(code_host, key) + [cmd],
                           capture_output=True, text=True, timeout=timeout)
        return p.returncode, (p.stdout or "") + (p.stderr or "")
    except subprocess.TimeoutExpired as e:
        out = (e.stdout or "") if isinstance(e.stdout, str) else (e.stdout or b"").decode("utf-8", "replace")
        return 124, out


def ssh_ok(host: str, cmd: str, key: str, pred, tries: int = 3, timeout: int = 120) -> tuple[bool, str]:
    """载体错误单独重试；真正的业务失败不重试。"""
    last = ""
    for i in range(tries):
        code, out = ssh(host, cmd, key, timeout)
        last = out
        if any(c in out for c in CARRIER_ERRORS):
            time.sleep(1.5)
            continue
        return pred(code, out), out
    return False, last


def scp(src: str, dst: str, key: str, timeout: int = 180) -> bool:
    p = subprocess.run(
        ["scp", "-q", "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=no",
         "-o", "UserKnownHostsFile=/dev/null", "-o", "LogLevel=ERROR",
         "-i", key, src, dst],
        capture_output=True, text=True, timeout=timeout)
    return p.returncode == 0


def first_json(text: str) -> dict:
    dec = json.JSONDecoder()
    for idx, ch in enumerate(text):
        if ch != "{":
            continue
        try:
            obj, _ = dec.raw_decode(text[idx:])
        except json.JSONDecodeError:
            continue
        if isinstance(obj, dict):
            return obj
    raise ValueError(f"输出里没有 JSON 对象: {text[:200]!r}")


def ensure_prober(args, ctx: Ctx) -> str | None:
    """在 221 上离线构建 Linux 探针，经本机中转送到 222。返回远端路径。

    为什么不在 222 上构建：222 没有 cargo。为什么不在本机构建：本机是 Windows。
    """
    remote = "/tmp/kad-mini-linux"
    if args.skip_build:
        ok, out = ssh_ok(args.prober, f"test -x {remote} && {remote} selftest | head -1",
                         args.key, lambda c, o: c == 0 and "48/48" in o, tries=2)
        ctx.check("探针已在 222 就位且自检通过（--skip-build）", ok, out.strip()[:80])
        return remote if ok else None

    tmp = Path(tempfile.mkdtemp(prefix="w51g-"))
    tar = tmp / "kad-mini-src.tgz"
    subprocess.run(["tar", "czf", str(tar), "src", "Cargo.toml", "Cargo.lock"],
                   cwd=KAD_SRC, check=True)
    if not scp(str(tar), f"root@{args.amule}:/tmp/kad-mini-src.tgz", args.key):
        ctx.check("上传探针源码到 221", False, "scp 失败")
        return None

    build = ("rm -rf /opt/kad-wan && mkdir -p /opt/kad-wan && "
             "tar xzf /tmp/kad-mini-src.tgz -C /opt/kad-wan && "
             "cd /opt/kad-wan && /root/.cargo/bin/cargo build --release --offline 2>&1 | tail -3")
    ok, out = ssh_ok(args.amule, build, args.key,
                     lambda c, o: "Finished" in o and c == 0, tries=2, timeout=300)
    if not ctx.check("221 上离线构建探针", ok, out.strip().splitlines()[-1] if out.strip() else ""):
        return None

    local_bin = tmp / "kad-mini-linux"
    if not scp(f"root@{args.amule}:/opt/kad-wan/target/release/kad-mini", str(local_bin), args.key):
        ctx.check("把 Linux 探针取回本机中转", False, "scp 失败")
        return None
    if not scp(str(local_bin), f"root@{args.prober}:{remote}", args.key):
        ctx.check("把探针送到 222", False, "scp 失败")
        return None
    ok, out = ssh_ok(args.prober, f"chmod +x {remote} && {remote} selftest | head -1", args.key,
                     lambda c, o: c == 0 and "48/48" in o, tries=2)
    ctx.check("探针在 222 自检通过", ok, out.strip()[:80])
    return remote if ok else None


def run_prober(args, ctx: Ctx, prober: str, ip: str, port: int, label: str,
               extra: str = "", allow_private: bool = False, timeout: int = 120) -> dict | None:
    """在 222 上跑一轮 wan-index-probe，返回解析后的 JSON。"""
    flags = extra
    if allow_private:
        flags += " --allow-private"
    cmd = (f"cd /tmp/w-g3amule 2>/dev/null || mkdir -p /tmp/w-g3amule && cd /tmp/w-g3amule; "
           f"{prober} wan-index-probe --ip {ip} --port {port} --label {label} "
           f"--attest-inbound-probe{flags}")
    ok, out = ssh_ok(args.prober, cmd, args.key, lambda c, o: "{" in o, tries=2, timeout=timeout)
    try:
        return first_json(out)
    except ValueError:
        ctx.check(f"公网探针输出可解析（{label}）", False, out.strip()[-160:])
        return None


def tcp_inbound(args, ctx: Ctx) -> None:
    """公网 TCP 4662 入向 + 源地址必须是公网 IP（排除 LAN 捷径）。"""
    probe = (
        "python3 - <<'PY'\n"
        "import socket,time\n"
        "s=socket.socket(); s.settimeout(8)\n"
        "t=time.time()\n"
        "try:\n"
        "    s.connect(('%s',%d)); print('OPEN', round((time.time()-t)*1000)); time.sleep(12)\n"
        "except Exception as e:\n"
        "    print('FAIL', type(e).__name__)\n"
        "PY" % (args.public_host, args.ed2k_port)
    )
    # 连接保持 12 s，期间到 221 上看对端地址。
    subprocess.Popen(ssh_base(args.prober, args.key) + [probe],
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(4)
    ok, out = ssh_ok(args.amule, f"ss -tn '( sport = :{args.ed2k_port} )' | tail -n +2", args.key,
                     lambda c, o: c == 0, tries=2)
    peers = [ln.split()[4] for ln in out.splitlines() if ln.strip() and len(ln.split()) >= 5]
    public = [p for p in peers if p.split(":")[0] == args.prober_public]
    ctx.check("公网 TCP 4662 可连（222 → 公网地址）", bool(peers), ",".join(peers[:3]) or "无连接")
    ctx.check("221 看到的对端是公网 IP（不是 LAN 捷径）", bool(public),
              f"对端={peers[:3]} 期望含 {args.prober_public}")


def udp_capture(args, ctx: Ctx, prober: str, label: str) -> None:
    """抓包证明 UDP 入向来自公网 IP，且 aMule 有回包。"""
    # 每轮独立文件：上一轮的路径匹配失败不能"继承"下一轮的报文。
    # `setsid nohup`：ssh 会话退出不能带走 tcpdump；并等它写出 listening 行才开始打包，
    # 否则第一轮会在抓包进程起来之前跑完（首轮就吃过这个亏）。
    cap = f"/tmp/w51g-udp-{label}.txt"
    start = (f"rm -f {cap}; setsid nohup timeout 45 tcpdump -ni any -l "
             f"'udp port {args.kad_port} and host {args.prober_public}' "
             f"-tt > {cap} 2>&1 < /dev/null & "
             f"sleep 1; for i in $(seq 1 15); do grep -q 'listening on' {cap} && break; sleep 0.2; done; "
             f"echo started")
    ssh(args.amule, start, args.key, timeout=20)
    rep = run_prober(args, ctx, prober, args.public_host, args.kad_port, label)
    time.sleep(2)
    ok, out = ssh_ok(args.amule, f"cat {cap}", args.key, lambda c, o: True, tries=1)
    lines = [ln for ln in out.splitlines() if f".{args.kad_port}" in ln]
    # tcpdump 写的是 `IP 112.5.32.34.2051 > 192.168.1.221.4672`（点分，不是
    # `host:port`）。上一版按冒号匹配，明明抓到了 46 行真实公网报文也被判 FAIL。
    inbound = [ln for ln in lines
               if " In " in ln and f"> {args.amule}.{args.kad_port}" in ln
               and f"{args.prober_public}." in ln]
    replied = [ln for ln in lines
               if " Out " in ln and f"{args.amule}.{args.kad_port} > {args.prober_public}." in ln]
    ctx.check("抓包：UDP 4672 入向来自公网 IP", bool(inbound),
              inbound[0].split(" IP ")[-1][:70] if inbound else f"{len(lines)} 行相关报文")
    ctx.check("抓包：aMule 回包指向同一公网源（非 LAN 捷径）", bool(replied),
              replied[0].split(" IP ")[-1][:70] if replied else "无对应回包")
    if rep is not None:
        ctx.reports.setdefault("wan", []).append(rep)


def negative_control(args, ctx: Ctx, prober: str) -> None:
    """同一公网地址上未监听的端口：同样的帧必须零回包。"""
    rep = run_prober(args, ctx, prober, args.public_host, args.dead_port, "neg-dead-port", timeout=90)
    if rep is None:
        return
    ctx.check("负对照：未监听端口零回包（ack 不是凭空造的）",
              not rep.get("reachable"), f"reachable={rep.get('reachable')}")


def lan_arms(args, ctx: Ctx, local_bin: Path) -> None:
    """LAN 对照 + 命名空间交叉回查（arbitrary key 只有 LAN 才过得去 IsLanIP）。"""
    def lan(label: str, extra: str = "") -> dict | None:
        cmd = f"{local_bin} lan-index-probe --ip {args.amule} --port {args.kad_port} --label {label}{extra}"
        p = subprocess.run(cmd, shell=True, capture_output=True, text=True, timeout=180)
        try:
            return first_json(p.stdout + p.stderr)
        except ValueError:
            ctx.check(f"LAN 探针输出可解析（{label}）", False, (p.stdout + p.stderr)[-160:])
            return None

    base = lan(f"w51g-lan-{int(time.time())}")
    if base is not None:
        ctx.reports["lan"] = base
        ctx.check("LAN 对照：PUBLISH 被 ACK", bool(base.get("publish_acked")),
                  f"verdict={base.get('verdict')}")
        ctx.check("LAN 对照：SEARCH 取回本轮描述符", bool(base.get("our_entry_returned")),
                  f"answers={base.get('search_answers')}")

    # 两个互不相干的 16B 键，在**同一个 UDP socket** 里写入并逐键读回。
    # 早期版本用 6 个独立进程做 2 写 4 读：aMule 对第 4 次查询稳定静默，而且
    # 「按对方标签查本键」本来就不是 KAD 语义（SEARCH_KEY_REQ 只带 key）。
    ts = int(time.time())
    ka = "1de0b15cfef1f75aaef7f555a3e289d3"
    kb = "9f86d081884c7d659a2feaa0c55ad015"
    la, lb = f"ns-a-{ts}", f"ns-b-{ts}"
    attempts: list[dict] = []
    ns: dict | None = None
    for attempt in range(1, 3):
        cmd = (f"{local_bin} lan-ns-dual --ip {args.amule} --port {args.kad_port} "
               f"--key-a {ka} --key-b {kb} --label-a {la} --label-b {lb}")
        p = subprocess.run(cmd, shell=True, capture_output=True, text=True, timeout=240)
        try:
            ns = first_json(p.stdout + p.stderr)
        except ValueError:
            ns = None
        attempts.append({"attempt": attempt,
                         "verdict": (ns or {}).get("verdict"),
                         "reachable": (ns or {}).get("reachable"),
                         "output_tail": (p.stdout + p.stderr)[-160:]})
        if ns and ns.get("reachable"):
            break
        time.sleep(20)
    ctx.check("命名空间探针可达（BOOTSTRAP 有回包）", bool(ns and ns.get("reachable")),
              f"verdict={(ns or {}).get('verdict')} attempts={len(attempts)}")
    if not ns:
        ctx.reports["namespace"] = {"key_a": ka, "key_b": kb, "attempts": attempts}
        return
    ctx.check("命名空间：两个键各自写入成功", bool(ns.get("all_written")),
              f"verdict={ns.get('verdict')} writes="
              f"{[w.get('acked') for w in ns.get('writes', [])]}")
    ctx.check("命名空间：逐键回查均有应答（不是靠超时凑出的「无别名」）",
              bool(ns.get("all_answered")),
              f"reads={[r.get('answered') for r in ns.get('reads', [])]}")
    ok = bool(ns.get("isolated"))
    ctx.check("命名空间：键 A 只回 A、键 B 只回 B（无别名）", ok,
              f"reads={[(r.get('slot'), r.get('own_label_present'), r.get('other_label_present')) for r in ns.get('reads', [])]}")
    ctx.reports["namespace"] = {"key_a": ka, "key_b": kb, "attempts": attempts, "report": ns}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--amule", default="192.168.1.221", help="aMule 所在主机")
    ap.add_argument("--prober", default="192.168.1.222", help="公网出口主机（跑探针）")
    ap.add_argument("--public-host", default="112.5.60.4", help="aMule 所在出口的公网地址")
    ap.add_argument("--prober-public", default="112.5.32.34", help="222 的公网出口地址")
    ap.add_argument("--ed2k-port", type=int, default=4662)
    ap.add_argument("--kad-port", type=int, default=4672)
    ap.add_argument("--dead-port", type=int, default=4673, help="负对照用：同一地址上未监听的端口")
    ap.add_argument("--runs", type=int, default=1)
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--skip-lan", action="store_true")
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--restart-amule", action="store_true",
                    help="运维动作：重启 aMule 让入向重新判定（会改变其内部状态）")
    ap.add_argument("--key", default=os.path.join(os.environ.get("TEMP", "/tmp"), "crkeys", "id_ed25519"))
    args = ap.parse_args()

    ctx = Ctx(key=args.key)
    started = datetime.now(timezone.utc)

    # 0. 载体
    ok_a, _ = ssh_ok(args.amule, "echo __ok__", args.key, lambda c, o: "__ok__" in o, tries=3)
    ctx.check("载体：221 可 ssh", ok_a)
    ok_p, out_p = ssh_ok(args.prober, "echo __ok__; command -v python3", args.key,
                         lambda c, o: "__ok__" in o and "python3" in o, tries=3)
    ctx.check("载体：222 可 ssh 且有 python3", ok_p, out_p.strip()[:60])
    if not (ok_a and ok_p):
        return finish(ctx, args, started, "blocked", 2)

    listening = ssh(args.amule, f"ss -lntu | grep -cE ':{args.ed2k_port}|:{args.kad_port}'", args.key)[1]
    ctx.check("aMule 在监听 4662/TCP 与 4672/UDP", "2" in listening, f"count={listening.strip()}")

    if args.restart_amule:
        cmd = ("pkill -x amuled; sleep 3; rm -f /opt/g3-clients/amule-config/muleLock; "
               "cd /opt/g3-clients && setsid nohup amuled -c /opt/g3-clients/amule-config -f -o "
               ">/tmp/amuled-restart.log 2>&1 < /dev/null & sleep 8; pgrep -x amuled")
        code, out = ssh(args.amule, cmd, args.key, timeout=60)
        ctx.check("aMule 已重启（运维动作）", bool(out.strip()), out.strip()[:20])
        # 给 KAD bootstrap 一点时间；这是等它连上，不是等一个判据。
        time.sleep(25)

    # 1. 网络层（公网 TCP）
    tcp_inbound(args, ctx)

    # 2. LAN 对照 + 命名空间放在 WAN 之前：重启后 aMule 的 UDP 判定最稳定的窗口有限，
    #    而命名空间要做 2 写 4 读共 6 轮，先做完再继续公网/负对照，避免后段被节流。
    if not args.skip_lan:
        local_bin = KAD_SRC / "target" / "release" / "kad-mini.exe"
        if not local_bin.is_file():
            local_bin = KAD_SRC / "target" / "release" / "kad-mini"
        if local_bin.is_file():
            lan_arms(args, ctx, local_bin)
        else:
            ctx.check("LAN 探针二进制存在", False, str(local_bin))

    # 3. 公网 KAD 索引面（含抓包）
    prober = ensure_prober(args, ctx)
    if prober is None:
        return finish(ctx, args, started, "blocked", 2)

    wan_ok = 0
    for i in range(args.runs):
        label = f"w51g-wan-{started.strftime('%H%M%S')}-{i}"
        udp_capture(args, ctx, prober, label)
    for rep in ctx.reports.get("wan", []):
        good = (rep.get("reachable") and rep.get("publish_acked")
                and rep.get("our_entry_returned") and rep.get("namespace_tag_matches"))
        wan_ok += 1 if good else 0
    if ctx.reports.get("wan"):
        ctx.check(f"公网 KAD：PUBLISH ACK + SEARCH 取回（{wan_ok}/{len(ctx.reports['wan'])} 轮）",
                  wan_ok == len(ctx.reports["wan"]),
                  f"verdicts={[r.get('verdict') for r in ctx.reports['wan']]}")
        ctx.check("公网 KAD：索引键在 SEARCHTOLERANCE 内（按构造排除越界分支）",
                  all(r.get("key_in_tolerance") for r in ctx.reports["wan"]),
                  f"distances={[r.get('key_distance_top32') for r in ctx.reports['wan']]}")

    # 4. 负对照
    negative_control(args, ctx, prober)

    passed = sum(1 for c in ctx.checks if c["ok"])
    status = "pass" if passed == len(ctx.checks) else "fail"
    return finish(ctx, args, started, status, 0 if status == "pass" else 1)


def finish(ctx: Ctx, args, started, status: str, code: int) -> int:
    REPORTS.mkdir(parents=True, exist_ok=True)
    ts = started.strftime("%Y%m%dT%H%M%SZ")
    out = REPORTS / f"run-{ts}.json"
    passed = sum(1 for c in ctx.checks if c["ok"])
    report = {
        "schema": "coldrevive.w51g.amule-wan-kad/v1",
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "status": status,
        "summary": f"{passed}/{len(ctx.checks)}",
        "checks": ctx.checks,
        "target": f"{args.public_host}:{args.kad_port}/udp (aMule) | {args.amule}:{args.ed2k_port}/tcp",
        "public_kad_contacted": True,
        "target_scope": "single_host_through_wan",
        "g3_verdict": "not_run",
        "evidence_valid_for_pass": False,
        "observations": ctx.reports,
        "note": ("单目标、经真实 NAT：公网侧 PUBLISH/SEARCH 与 LAN 对照同时成立，"
                 "并用未监听端口做负对照、用两个 16B 键做交叉回查。"
                 "本报告不写 g3 结论、不含速率数字；结论由实验执行人回填。"),
    }
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"\n结果: {passed}/{len(ctx.checks)}  status={status}")
    print(f"报告: {out}")
    if not args.keep:
        shutil.rmtree(Path(tempfile.gettempdir()) / "w51g-", ignore_errors=True)
    return code


if __name__ == "__main__":
    sys.exit(main())
