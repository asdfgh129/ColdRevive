#!/usr/bin/env python3
"""FR-25 公网 daemon A/B：两端都走 coldrevived 的 /cli 生产路径，判据为落地 md5。

A: 221 daemon serve（占用已映射的公网 TCP 4662） → 222 daemon download --out
B: 221 真 aMule serve（恢复 4662）               → 222 daemon download --out

脚本只写报告，不改门禁；aMule 临时停止后一定用 systemd-run 恢复。
"""
from __future__ import annotations

import datetime
import hashlib
import json
import os
import pathlib
import shlex
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[3]
REPORT_DIR = ROOT / "tests" / "e2e" / "w25-daemon" / "reports"
KEY = os.path.join(os.environ.get("TEMP", "/tmp"), "crkeys", "id_ed25519")
H221 = "192.168.1.221"
H222 = "192.168.1.222"
PUBLIC_221 = "112.5.60.4"
SAMPLE_BIG = "/opt/g3-clients/amule-config/Incoming/w25-reverse-20261124102018.bin"
SAMPLE_SMALL = "/opt/g3-clients/amule-config/Incoming/ed2k-sample-small.bin"
LOCAL_BIG = ROOT / "tests" / "e2e" / "w25-daemon" / "run" / "fr25-pub-daemon-big.bin"
LOCAL_SMALL = ROOT / "tests" / "e2e" / "w25-daemon" / "run" / "fr25-pub-daemon-small.bin"
REMOTE_BIG = "/tmp/fr25-pub-daemon-big.bin"
REMOTE_SMALL = "/tmp/fr25-pub-daemon-small.bin"
RUN221 = "/opt/fr25-pub-daemon/221"
RUN222 = "/tmp/fr25-pub-daemon/222"
HTTP221 = 18600
HTTP222 = 18601
BIN221 = "/opt/cr-current/target/release/coldrevived"
BIN222 = "/tmp/coldrevived"
UNIT221 = "fr25pub221"
UNIT222 = "fr25pub222"
AMULE_UNIT = "cr-amuled3"


def ssh(host: str, *cmd: str, timeout: int = 120, check: bool = False) -> subprocess.CompletedProcess:
    args = ["ssh", "-i", KEY, "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", f"root@{host}"]
    args.extend(cmd)
    p = subprocess.run(args, capture_output=True, text=True, timeout=timeout)
    if check and p.returncode != 0:
        raise RuntimeError(f"ssh {host} {' '.join(cmd)} failed: {p.stdout[-800:]} {p.stderr[-800:]}")
    return p


def shell(host: str, script: str, timeout: int = 120, check: bool = False) -> subprocess.CompletedProcess:
    return ssh(host, "bash -lc", shlex.quote(script), timeout=timeout, check=check)


def local_md5(path: pathlib.Path) -> str:
    return hashlib.md5(path.read_bytes()).hexdigest()


def remote_md5(host: str, path: str) -> str:
    p = ssh(host, "md5sum", path, timeout=180, check=True)
    return p.stdout.strip().split()[0]


def config_json(run: str, node: str, port: int, sample: str) -> str:
    cfg = {
        "bind": "127.0.0.1",
        "nodes": {node: {"http_port": port, "wake_udp_port": port + 1, "bt_port": port + 2}},
        "workdir": run,
        "sample_path": sample,
        "golden_req": f"{run}/req.json",
        "golden_ack": f"{run}/ack.json",
        "keywords": ["fr25-public-daemon"],
        "namespace": "ColdRevive_V1:",
    }
    return json.dumps(cfg, ensure_ascii=False, indent=2)


def write_config(host: str, run: str, node: str, port: int, sample: str) -> None:
    cfg = config_json(run, node, port, sample)
    shell(
        host,
        f"mkdir -p {shlex.quote(run)} && "
        f"cat > {shlex.quote(run + '/config.json')} <<'JSON'\n{cfg}\nJSON\n"
        f"printf '{{}}' > {shlex.quote(run + '/req.json')} && "
        f"printf '{{}}' > {shlex.quote(run + '/ack.json')}",
        check=True,
    )


def start_daemon(host: str, unit: str, binary: str, run: str, node: str, port: int) -> None:
    shell(host, f"systemctl stop {unit} 2>/dev/null || true", check=False)
    shell(host, f"systemctl reset-failed {unit} 2>/dev/null || true", check=False)
    shell(
        host,
        f"systemd-run --unit={unit} --collect --property=WorkingDirectory={run} "
        f"{binary} --config {run}/config.json --node-id {node} "
        f"--bind 127.0.0.1 --workdir {run}",
        check=True,
    )
    deadline = time.time() + 20
    while time.time() < deadline:
        p = shell(host, f"curl -fsS http://127.0.0.1:{port}/health", check=False)
        if p.returncode == 0 and '"status"' in p.stdout:
            return
        time.sleep(0.5)
    raise RuntimeError(f"{host} daemon health 未通过: {p.stdout[-500:]} {p.stderr[-500:]}")


def cli(host: str, port: int, command: str, *args: str, timeout: int = 60) -> dict:
    payload = json.dumps({"command": command, "args": list(args)}, ensure_ascii=False)
    p = shell(
        host,
        f"curl -fsS -H 'Content-Type: application/json' -d {shlex.quote(payload)} "
        f"http://127.0.0.1:{port}/cli",
        timeout=timeout,
        check=True,
    )
    return json.loads(p.stdout)


def task_id(resp: dict, label: str) -> str:
    got = resp.get("id")
    if not got:
        raise RuntimeError(f"{label} 未登记任务，真实响应: {json.dumps(resp, ensure_ascii=False)[:1200]}")
    return str(got)


def poll_task(host: str, port: int, task_id: str, timeout_s: float) -> dict:
    deadline = time.time() + timeout_s
    last: dict = {"status": "unknown"}
    while time.time() < deadline:
        last = cli(host, port, "ed2k-status", "--task", task_id)
        if last.get("status") in ("complete", "error"):
            return last
        time.sleep(1)
    return last


def main() -> int:
    stamp = datetime.datetime.now().strftime("%Y%m%d%H%M%S")
    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    report: dict = {
        "schema": "coldrevive.fr25.public-daemon-ab/v1",
        "generated_at": datetime.datetime.now().astimezone().isoformat(timespec="seconds"),
        "scope": "221/222 真公网路径；两端 daemon /cli 生产栈；判据=对端落地 md5",
        "arms": {},
        "gate_snapshot": {
            "g2_claim": False,
            "s6_status": "not_run",
            "evidence_valid_for_pass": False,
            "auto_pass": False,
        },
    }

    out = REPORT_DIR / f"public-daemon-ab-{stamp}.json"
    try:
        # 前置：二进制、样本、端口
        shell(H221, f"test -x {BIN221}", check=True)
        shell(H222, f"test -x {BIN222}", check=True)
        shell(H221, f"test -f {SAMPLE_BIG} && test -f {SAMPLE_SMALL}", check=True)
        LOCAL_BIG.parent.mkdir(parents=True, exist_ok=True)
        for src, local, remote in [
            (SAMPLE_BIG, LOCAL_BIG, REMOTE_BIG),
            (SAMPLE_SMALL, LOCAL_SMALL, REMOTE_SMALL),
        ]:
            subprocess.run(
                ["scp", "-i", KEY, "-o", "BatchMode=yes", f"root@{H221}:{src}", str(local)],
                check=True, timeout=180,
            )
            subprocess.run(
                ["scp", "-i", KEY, "-o", "BatchMode=yes", str(local), f"root@{H222}:{remote}"],
                check=True, timeout=180,
            )
            if local_md5(local) != remote_md5(H222, remote):
                raise RuntimeError(f"样本未完整复制到 222: {remote}")
        sha_big = remote_md5(H221, SAMPLE_BIG)
        sha_small = remote_md5(H221, SAMPLE_SMALL)
        report["samples"] = {"big_md5": sha_big, "small_md5": sha_small}

        # Arm A：221 daemon serve -> 222 daemon download（占用映射 4662，先停 aMule）
        shell(H221, f"systemctl stop {AMULE_UNIT} 2>/dev/null || true", check=False)
        time.sleep(1)
        write_config(H221, RUN221, "A", HTTP221, SAMPLE_BIG)
        write_config(H222, RUN222, "B", HTTP222, REMOTE_BIG)
        start_daemon(H221, UNIT221, BIN221, RUN221, "A", HTTP221)
        start_daemon(H222, UNIT222, BIN222, RUN222, "B", HTTP222)

        name_big = f"fr25-daemon-pub-{stamp}.bin"
        served = cli(H221, HTTP221, "ed2k-serve", "--file", SAMPLE_BIG, "--name", name_big,
                     "--port", "4662", "--timeout-ms", "300000", "--compressed-parts")
        downloaded = cli(H222, HTTP222, "ed2k-download", "--file", REMOTE_BIG, "--name", name_big,
                         "--host", f"{PUBLIC_221}:4662", "--timeout-ms", "300000",
                         "--out", f"{RUN222}/landed-big.bin")
        report["arms"]["A_started"] = {"serve": served, "download": downloaded}
        st = poll_task(H221, HTTP221, task_id(served, "A serve"), 330)
        dt = poll_task(H222, HTTP222, task_id(downloaded, "A download"), 330)
        landed_big = remote_md5(H222, f"{RUN222}/landed-big.bin") if dt.get("status") == "complete" else ""
        report["arms"]["A_daemon_to_daemon_public"] = {
            "serve_started": served,
            "serve_task": st,
            "download_started": downloaded,
            "download_task": dt,
            "landed_md5": landed_big,
            "expected_md5": sha_big,
            "pass": bool(
                (st.get("result") or {}).get("bytes_sent", 0) > 0
                and (dt.get("result") or {}).get("completed") is True
                and landed_big == sha_big
            ),
        }

        # 恢复 aMule 并停掉 221 daemon，避免 4662 冲突
        shell(H221, f"systemctl stop {UNIT221} 2>/dev/null || true", check=False)
        shell(H221, f"systemctl reset-failed {AMULE_UNIT} 2>/dev/null || true", check=False)
        shell(
            H221,
            f"systemd-run --unit={AMULE_UNIT} --collect /usr/bin/amuled "
            f"-c /opt/g3-clients/amule-config -o",
            check=True,
        )
        time.sleep(3)
        act = shell(H221, f"systemctl is-active {AMULE_UNIT}", check=False)
        report["amule_restored"] = act.stdout.strip()

        # Arm B：222 daemon <- 221 真 aMule 公网 4662
        name_small = "ed2k-sample-small.bin"
        d2 = cli(H222, HTTP222, "ed2k-download", "--file", REMOTE_SMALL, "--name", name_small,
                 "--host", f"{PUBLIC_221}:4662", "--timeout-ms", "300000",
                 "--out", f"{RUN222}/landed-small.bin")
        report["arms"]["B_started"] = {"download": d2}
        dt2 = poll_task(H222, HTTP222, task_id(d2, "B download"), 330)
        landed_small = remote_md5(H222, f"{RUN222}/landed-small.bin") if dt2.get("status") == "complete" else ""
        report["arms"]["B_amule_to_daemon_public"] = {
            "download_started": d2,
            "download_task": dt2,
            "landed_md5": landed_small,
            "expected_md5": sha_small,
            "pass": bool(
                (dt2.get("result") or {}).get("completed") is True
                and landed_small == sha_small
            ),
        }
        verdicts = [a["pass"] for a in report["arms"].values() if "pass" in a]
        report["status"] = "PASS" if len(verdicts) == 2 and all(verdicts) else "FAIL"
    except Exception as e:
        report["status"] = "BLOCKED"
        report["reason"] = f"{type(e).__name__}: {e}"
    finally:
        for host, unit in [(H221, UNIT221), (H222, UNIT222)]:
            shell(host, f"systemctl stop {unit} 2>/dev/null || true", check=False)
        # 无论结果如何，确保 aMule 不在我们手里 permanently 停掉。
        active = shell(H221, f"systemctl is-active {AMULE_UNIT}", check=False).stdout.strip()
        if active != "active":
            shell(H221, f"systemctl reset-failed {AMULE_UNIT} 2>/dev/null || true", check=False)
            shell(
                H221,
                f"systemd-run --unit={AMULE_UNIT} --collect /usr/bin/amuled "
                f"-c /opt/g3-clients/amule-config -o",
                check=False,
            )
        out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({"report": str(out), "status": report.get("status"),
                          "reason": report.get("reason", "")}, ensure_ascii=False, indent=2))
    return 0 if report.get("status") == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
