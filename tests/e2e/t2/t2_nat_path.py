#!/usr/bin/env python3
"""T2 — M2 跨 NAT 复活路径接线验收（**本地预检，不是 S6 证据**）。

验证 `cr-nat` 接入 `coldrevived` 后：
  - STUN 探测复用唤醒监听 socket（`nat probe listener`）；
  - 端点候选计划 / 真实 UDP 打洞与唤醒复用同一 socket（`nat plan` / `nat wake`）；
  - 中继回退经真实 TCP 帧 + 真实 UDP 转发（`nat wake ... relay`）；
  - tracker 按请求方 IP 做私网/公网 peer 地址协商（`/announce` + `nat tracker`）；
  - M1 既有 `wake` / `find` 命令在 nat 打开时仍按 PROTO v1 工作。

**红线**：本机回环 + mock STUN + mock 中继桥，报告 `evidence_valid=false`、
`s6_status=not_run`。四类真实 NAT（含 CGNAT）可达率仍须 M2 多环境实测回填。

用法：
  python3 tests/e2e/t2/t2_nat_path.py --self-test
  python3 tests/e2e/t2/t2_nat_path.py --mode local
环境变量 `COLDREVIVE_DAEMON` / `COLDREVIVE_CLI` 可覆盖二进制路径，
缺省使用 `target/release/`。
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import random
import shutil
import socket
import struct
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
CFG_PATH = HERE / "t2_config.json"
RUN_DIR = ROOT / "tests/e2e/t2/.run"
REPORT_DIR = ROOT / "tests/e2e/t2/reports"
MAGIC = 0x2112A442
MAGIC_BYTES = MAGIC.to_bytes(4, "big")


def utc_stamp() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def recv_exact(conn: socket.socket, n: int) -> bytes:
    out = bytearray()
    while len(out) < n:
        chunk = conn.recv(n - len(out))
        if not chunk:
            return b""
        out.extend(chunk)
    return bytes(out)


# ---------------------------------------------------------------------------
# 本地 mock：STUN / 中继桥（真实 UDP / TCP socket，不合成业务应答）
# ---------------------------------------------------------------------------

class MockStun:
    """RFC 5389 Binding 响应。按源端口把 A/B 的唤醒端口映射到不同的回环别名，
    模拟「NAT 外部地址 ≠ 内部私网地址」；应答中的地址来自真实观测的源端口。"""

    def __init__(self, ports: dict[int, str], stop: threading.Event):
        self.ports = ports
        self.stop = stop
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.sock.bind(("127.0.0.1", 0))
        self.sock.settimeout(0.15)
        self.port = self.sock.getsockname()[1]
        self.responses = 0
        self.thread = threading.Thread(target=self._loop, daemon=True)

    def start(self) -> None:
        self.thread.start()

    def close(self) -> None:
        self.sock.close()

    @staticmethod
    def xor_address(ip: str, port: int) -> bytes:
        ipn = struct.unpack("!I", socket.inet_aton(ip))[0]
        return (b"\x00\x01"
                + struct.pack("!H", port ^ (MAGIC >> 16))
                + struct.pack("!I", ipn ^ MAGIC))

    def _loop(self) -> None:
        while not self.stop.is_set():
            try:
                data, addr = self.sock.recvfrom(2048)
            except (socket.timeout, OSError):
                continue
            if (len(data) < 20 or data[0:2] != b"\x00\x01"
                    or data[4:8] != MAGIC_BYTES):
                continue
            tid = data[8:20]
            mapped_ip = self.ports.get(addr[1], addr[0])
            value = self.xor_address(mapped_ip, addr[1])
            attr = struct.pack("!HH", 0x0020, len(value)) + value
            resp = (struct.pack("!HH", 0x0101, len(attr))
                    + MAGIC_BYTES + tid + attr)
            self.sock.sendto(resp, addr)
            self.responses += 1


class MockRelay:
    """TCP 帧中继桥：把 B 的 RELAY 帧负载经真实 UDP 发给 A 的唤醒端口，
    再把 A 的**真实** UDP 应答封装成 DELIVERED 帧回给 B。不合成 ACK。"""

    def __init__(self, wake_port: int, stop: threading.Event):
        self.wake_port = wake_port
        self.stop = stop
        self.srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.srv.bind(("127.0.0.1", 0))
        self.srv.listen(8)
        self.srv.settimeout(0.15)
        self.port = self.srv.getsockname()[1]
        self.relayed = 0
        self.thread = threading.Thread(target=self._accept_loop, daemon=True)

    def start(self) -> None:
        self.thread.start()

    def close(self) -> None:
        self.srv.close()

    def _accept_loop(self) -> None:
        while not self.stop.is_set():
            try:
                conn, _ = self.srv.accept()
            except (socket.timeout, OSError):
                continue
            threading.Thread(target=self._conn, args=(conn,), daemon=True).start()

    def _conn(self, conn: socket.socket) -> None:
        conn.settimeout(5)
        node_id = b"\x00" * 16
        with conn:
            while True:
                head = recv_exact(conn, 21)
                if len(head) < 21:
                    return
                kind = head[0]
                node = head[1:17]
                length = struct.unpack("!I", head[17:21])[0]
                payload = recv_exact(conn, length) if length else b""
                if kind == 0x01:  # REGISTER
                    node_id = node
                    conn.sendall(b"\x04" + b"\x00" * 16 + struct.pack("!I", 0))
                elif kind == 0x02:  # RELAY -> 真实 UDP 转发
                    wake = payload[16:]
                    udp = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
                    udp.settimeout(2)
                    reply = b""
                    try:
                        udp.sendto(wake, ("127.0.0.1", self.wake_port))
                        reply, _ = udp.recvfrom(65535)
                    except OSError:
                        pass
                    finally:
                        udp.close()
                    self.relayed += 1
                    conn.sendall(b"\x03" + node_id
                                 + struct.pack("!I", len(reply)) + reply)


# ---------------------------------------------------------------------------
# Harness
# ---------------------------------------------------------------------------

class T2:
    def __init__(self, cfg: dict[str, Any], mode: str, keep: bool = False):
        self.cfg = cfg
        self.mode = mode
        self.keep = keep
        self.cases: list[dict[str, Any]] = []
        self.procs: dict[str, subprocess.Popen] = {}
        self.logs: list[Any] = []
        self.stop = threading.Event()
        self.stun: MockStun | None = None
        self.relay: MockRelay | None = None
        self.runtime_cfg = RUN_DIR / "t2-runtime.json"
        self.sample = RUN_DIR / "sample" / cfg["fixture"]["filename"]
        self.file_hash = ""
        self.infohash_stub = ""
        self.observations: dict[str, Any] = {}

    # ---------- 准备 ----------

    def binaries(self) -> tuple[str, str]:
        suffix = ".exe" if os.name == "nt" else ""
        daemon = os.environ.get(self.cfg["bin"]["daemon_env"],
                                str(ROOT / f"target/release/coldrevived{suffix}"))
        cli = os.environ.get(self.cfg["bin"]["cli_env"],
                             str(ROOT / f"target/release/coldrevive-cli{suffix}"))
        return daemon, cli

    def prepare(self) -> None:
        daemon, cli = self.binaries()
        for p in (daemon, cli):
            if not Path(p).is_file():
                raise RuntimeError(f"缺少二进制: {p}（先 cargo build --release --workspace --offline）")
        if RUN_DIR.exists():
            shutil.rmtree(RUN_DIR)
        RUN_DIR.mkdir(parents=True, exist_ok=True)
        self.sample.parent.mkdir(parents=True, exist_ok=True)
        REPORT_DIR.mkdir(parents=True, exist_ok=True)
        data = random.Random(self.cfg["fixture"]["seed"]).randbytes(
            self.cfg["fixture"]["size_bytes"])
        self.sample.write_bytes(data)
        self.file_hash = hashlib.sha256(data).hexdigest()
        self.infohash_stub = hashlib.sha1(data).hexdigest()

        ports = {n["wake_udp_port"]: n["mock_external_ip"]
                 for n in self.cfg["nodes"].values()}
        self.stun = MockStun(ports, self.stop)
        self.stun.start()
        self.relay = MockRelay(self.cfg["nodes"]["A"]["wake_udp_port"], self.stop)
        self.relay.start()

        nodes = {}
        for node, spec in self.cfg["nodes"].items():
            nodes[node] = {
                "host": spec["host"],
                "http_port": spec["http_port"],
                "wake_udp_port": spec["wake_udp_port"],
                "bt_port": spec["bt_port"],
            }
        golden = ROOT / self.cfg["paths"]["golden_dir"]
        content = {
            "suite": "T2",
            "schema": "coldrevive.daemon.config/v1",
            "bind": self.cfg["bind"],
            "nodes": nodes,
            "sample_path": self.sample.relative_to(ROOT).as_posix(),
            "keywords": self.cfg["fixture"]["keywords"],
            "namespace": self.cfg["protocol"]["namespace"],
            "workdir": RUN_DIR.relative_to(ROOT).as_posix(),
            "golden_req": (golden / "wakeup_req.v1.json").relative_to(ROOT).as_posix(),
            "golden_ack": (golden / "wakeup_ack.v1.json").relative_to(ROOT).as_posix(),
            "downloader": "",
            "challenge_window_sec": self.cfg["protocol"]["challenge_window_sec"],
            "nat": {
                "enabled": True,
                "stun_servers": [f"127.0.0.1:{self.stun.port}"],
                "relay": f"127.0.0.1:{self.relay.port}"
            },
        }
        self.runtime_cfg.write_text(json.dumps(content, ensure_ascii=False, indent=2) + "\n",
                                    encoding="utf-8")

    def spawn(self, node: str) -> None:
        daemon, _ = self.binaries()
        workdir = RUN_DIR / node
        workdir.mkdir(parents=True, exist_ok=True)
        log = (RUN_DIR / f"{node}.log").open("wb")
        self.logs.append(log)
        self.procs[node] = subprocess.Popen(
            [daemon, "--config", str(self.runtime_cfg), "--node-id", node,
             "--bind", self.cfg["bind"], "--self-host", self.cfg["nodes"][node]["host"],
             "--workdir", str(workdir), "--sample", str(self.sample)],
            stdout=log, stderr=subprocess.STDOUT, cwd=str(ROOT))

    def health(self, node: str) -> dict[str, Any] | None:
        url = f"http://127.0.0.1:{self.cfg['nodes'][node]['http_port']}/health"
        try:
            with urllib.request.urlopen(url, timeout=self.cfg["timeouts_sec"]["health"]) as r:
                return json.loads(r.read().decode("utf-8"))
        except (urllib.error.URLError, OSError, ValueError):
            return None

    def wait_health(self) -> bool:
        deadline = time.monotonic() + self.cfg["timeouts_sec"]["startup"]
        while time.monotonic() < deadline:
            if all(self.health(n) is not None for n in self.cfg["nodes"]):
                return True
            time.sleep(0.15)
        return False

    def cli(self, node: str, command: str, *args: str) -> dict[str, Any]:
        _, cli = self.binaries()
        env = dict(os.environ)
        env["COLDREVIVE_T0_CONFIG"] = str(self.runtime_cfg)
        try:
            p = subprocess.run([cli, "--json", "--node", node, command, *args],
                               env=env, cwd=str(ROOT), capture_output=True, text=True,
                               timeout=self.cfg["timeouts_sec"]["cli"])
        except subprocess.TimeoutExpired as e:
            return {"error": f"CLI 超时: {e}"}
        if p.returncode != 0:
            return {"error": f"CLI 退出码 {p.returncode}: {p.stderr.strip()}"}
        try:
            out = json.loads(p.stdout.strip())
            return out if isinstance(out, dict) else {"error": "CLI 输出非对象"}
        except ValueError as e:
            return {"error": f"CLI 非法 JSON: {e}: {p.stdout[:200]}"}

    def http_get(self, path: str) -> bytes:
        url = f"http://127.0.0.1:{self.cfg['nodes']['A']['http_port']}{path}"
        with urllib.request.urlopen(url, timeout=3) as r:
            return r.read()

    # ---------- 断言引擎 ----------

    def case(self, cid: str, title: str, predicate: Callable[[], tuple[bool, str, Any]]) -> None:
        try:
            ok, detail, evidence = predicate()
        except Exception as e:  # 观测异常一律 FAIL，不吞掉
            ok, detail, evidence = False, f"观测异常 {type(e).__name__}: {e}", None
        self.cases.append({"id": cid, "title": title, "result": "PASS" if ok else "FAIL",
                           "detail": detail, "evidence": evidence})
        print(f"  [{'PASS' if ok else 'FAIL'}] {cid} {title} — {detail}")

    # ---------- 用例 ----------

    def run_cases(self) -> None:
        a_port = self.cfg["nodes"]["A"]["wake_udp_port"]
        b_port = self.cfg["nodes"]["B"]["wake_udp_port"]

        # 健康检查：真实进程 / 真实 HTTP
        self.case("T2-00", "A/B 守护进程真实存活", lambda: (
            all(self.health(n) is not None for n in ("A", "B")),
            "A/B /health 均返回 200" if all(self.health(n) is not None for n in ("A", "B"))
            else "至少一个 /health 不可用",
            {n: self.health(n) for n in ("A", "B")}))

        # T2-01：STUN 复用唤醒监听 socket
        probes: dict[str, Any] = {}
        for node, port, alias in (("A", a_port, "127.0.0.2"), ("B", b_port, "127.0.0.3")):
            probes[node] = self.cli(node, "nat", "probe", "listener", "600")
        self.observations["probes"] = probes

        def c01() -> tuple[bool, str, Any]:
            ok = all(
                p.get("public_udp_egress_ok") is True
                and p.get("stun_socket") == "wake-listener"
                and p.get("s6_status") == "not_run"
                and p.get("evidence_valid_for_pass") is False
                for p in probes.values()
            ) and probes["A"].get("mapped_address") == f"127.0.0.2:{a_port}" \
              and probes["B"].get("mapped_address") == f"127.0.0.3:{b_port}"
            return ok, (f"A={probes['A'].get('mapped_address')} "
                        f"B={probes['B'].get('mapped_address')} "
                        f"STUN 应答 {self.stun.responses} 次"
                        if self.stun else "mock STUN 未启动"), probes
        self.case("T2-01", "STUN 复用唤醒监听 socket，映射端口 = 实际唤醒端口", c01)

        # T2-02 / 03：端点通告与交换
        adv_a = self.cli("A", "nat", "advertise")
        adv_path = RUN_DIR / "a-advertise.json"
        adv_path.write_text(json.dumps(adv_a, ensure_ascii=False, indent=2), encoding="utf-8")
        peer = self.cli("B", "nat", "peer", str(adv_path))
        self.observations["advertise_a"] = adv_a
        self.observations["peer"] = peer

        def c02() -> tuple[bool, str, Any]:
            kinds = {e.get("kind") for e in adv_a.get("endpoints", [])}
            ok = (adv_a.get("node_id") == "A"
                  and {"private-lan", "stun-mapped"}.issubset(kinds)
                  and adv_a.get("bt_external") is None
                  and adv_a.get("s6_status") == "not_run"
                  and adv_a.get("evidence_valid_for_pass") is False)
            return ok, f"端点 kinds={sorted(kinds)}, bt_external=null（无伪造公网 BT 映射）", adv_a
        self.case("T2-02", "A 端点通告只含真实证据，无伪造公网 BT 映射", c02)

        def c03() -> tuple[bool, str, Any]:
            ok = peer.get("ok") is True and peer.get("node_id") == "A" \
                and peer.get("endpoints", 0) >= 2
            return ok, f"node_id={peer.get('node_id')}, endpoints={peer.get('endpoints')}", peer
        self.case("T2-03", "B 经真实文件取回 A 的端点通告", c03)

        # T2-04：候选计划
        plan = self.cli("B", "nat", "plan")
        self.observations["plan"] = plan

        def c04() -> tuple[bool, str, Any]:
            kinds = [e.get("kind") for e in plan.get("candidates", [])]
            ok = ("private-lan" in kinds and "stun-mapped" in kinds
                  and "relay" in kinds
                  and kinds.index("private-lan") < kinds.index("stun-mapped")
                  and kinds.index("stun-mapped") < kinds.index("relay")
                  and plan.get("s6_status") == "not_run"
                  and plan.get("evidence_valid_for_pass") is False)
            return ok, f"候选顺序={kinds}, strategy={plan.get('strategy')}", plan
        self.case("T2-04", "候选计划顺序 = 私网 → STUN 映射 → 中继兜底", c04)

        # T2-05：强制走 STUN 映射候选（真实 UDP 打洞 + 同 socket 唤醒）
        wake_mapped = self.cli("B", "nat", "wake", self.file_hash,
                               self.infohash_stub, "stun-mapped")
        self.observations["wake_stun_mapped"] = wake_mapped

        def c05() -> tuple[bool, str, Any]:
            ok = (wake_mapped.get("ok") is True
                  and wake_mapped.get("path") == "stun-mapped"
                  and wake_mapped.get("ack_status") == "OK"
                  and len(wake_mapped.get("attempts", [])) == 1
                  and wake_mapped["attempts"][0].get("ok") is True
                  and wake_mapped.get("evidence_valid_for_pass") is False)
            return (ok,
                    f"path={wake_mapped.get('path')}, "
                    f"ack={wake_mapped.get('ack_status')}, "
                    f"detail={wake_mapped.get('attempts', [{}])[0].get('detail')}",
                    wake_mapped)
        self.case("T2-05", "跨 NAT 地址候选：单 socket 打洞 + 真实 WAKEUP_ACK", c05)

        # T2-06：M1 既有复活命令在 nat 打开时仍按 PROTO v1 工作
        legacy_ack = self.cli("B", "wake", "ack", self.file_hash, self.infohash_stub)
        legacy_probe = self.cli("B", "wake", "probe", self.file_hash, self.infohash_stub)
        find_res = self.cli("B", "find", self.cfg["fixture"]["keywords"][0])
        self.observations["legacy_ack"] = legacy_ack
        self.observations["legacy_probe"] = legacy_probe
        self.observations["find"] = find_res

        def c06() -> tuple[bool, str, Any]:
            ack = legacy_ack.get("ack", {})
            probe = legacy_probe.get("req", {})
            ok = (ack.get("fields_ok") is True and ack.get("torrent_fetch_ok") is True
                  and ack.get("torrent_parse_ok") is True
                  and probe.get("valid") is True and probe.get("challenge_ok") is True
                  and probe.get("nonce_unique") is True
                  and find_res.get("result", {}).get("hit_node") == "A")
            return (ok,
                    f"fields_ok={ack.get('fields_ok')}, "
                    f"torrent_parse_ok={ack.get('torrent_parse_ok')}, "
                    f"find.hit_node={find_res.get('result', {}).get('hit_node')}",
                    {"ack": ack, "probe": probe, "find": find_res})
        self.case("T2-06", "M1 wake/find 在 nat 打开时仍满足 PROTO v1 契约", c06)

        # T2-07：中继回退（真实 TCP 帧 → 真实 UDP 转发 → A 的真实 ACK）
        wake_relay = self.cli("B", "nat", "wake", self.file_hash,
                              self.infohash_stub, "relay")
        self.observations["wake_relay"] = wake_relay

        def c07() -> tuple[bool, str, Any]:
            attempts = wake_relay.get("attempts", [])
            ok = (wake_relay.get("ok") is True
                  and wake_relay.get("path") == "relay"
                  and wake_relay.get("ack_status") == "OK"
                  and len(attempts) == 1 and attempts[0].get("kind") == "relay"
                  and attempts[0].get("ok") is True
                  and self.relay.relayed >= 1)
            return (ok,
                    f"path={wake_relay.get('path')}, "
                    f"mock 中继真实转发 {self.relay.relayed} 次, "
                    f"ack={wake_relay.get('ack_status')}",
                    wake_relay)
        self.case("T2-07", "中继回退：真实 TCP 帧 + A 的真实 UDP 应答", c07)

        # T2-08：tracker 私网/公网 peer 地址协商
        t_private = self.cli("A", "nat", "tracker", "127.0.0.1")
        t_public = self.cli("A", "nat", "tracker", "8.8.8.8")
        raw_announce = b""
        try:
            raw_announce = self.http_get("/announce?compact=1")
        except Exception as e:
            raw_announce = f"ERROR:{e}".encode()
        self.observations["tracker"] = {
            "private": t_private, "public": t_public,
            "announce_len": len(raw_announce),
        }

        def c08() -> tuple[bool, str, Any]:
            expected_port = self.cfg["nodes"]["A"]["bt_port"]
            marker = b"5:peers6:"
            peer_bytes = b""
            if marker in raw_announce:
                i = raw_announce.index(marker) + len(marker)
                peer_bytes = raw_announce[i:i + 6]
            compact_ok = (len(peer_bytes) == 6
                          and peer_bytes[:4] == bytes([127, 0, 0, 1])
                          and struct.unpack("!H", peer_bytes[4:])[0] == expected_port)
            ok = (t_private.get("peer_ip") == "127.0.0.1"
                  and t_private.get("peer_port") == expected_port
                  and t_public.get("requester_is_public") is True
                  and t_public.get("peer_ip") is None
                  and t_public.get("peer_port") == 0
                  and compact_ok)
            return (ok,
                    f"私网请求→{t_private.get('peer_ip')}:{t_private.get('peer_port')}, "
                    f"公网请求→{t_public.get('peer_ip')}（空 peer，不泄漏 RFC1918）, "
                    f"/announce compact peer={peer_bytes.hex()}",
                    {"private": t_private, "public": t_public,
                     "announce_peer_hex": peer_bytes.hex()})
        self.case("T2-08", "tracker 不向公网请求方通告 RFC1918 私网 peer", c08)

        # T2-09：失败路径如实报错，不回填默认应答
        dead = self.cli("B", "nat", "wake", self.file_hash,
                        self.infohash_stub, "pcp-mapped")
        self.observations["wake_no_candidate"] = dead

        def c09() -> tuple[bool, str, Any]:
            ok = dead.get("ok") is False and dead.get("path") is None \
                and dead.get("attempts") == [] and "error" in dead
            return ok, f"error={dead.get('error')}", dead
        self.case("T2-09", "无匹配候选时如实报错，不合成成功应答", c09)

    # ---------- 生命周期 ----------

    def run(self) -> dict[str, Any]:
        self.prepare()
        try:
            for node in self.cfg["nodes"]:
                self.spawn(node)
            if not self.wait_health():
                raise RuntimeError("A/B 守护进程未就绪")
            self.run_cases()
        finally:
            self.cleanup()
        passed = sum(c["result"] == "PASS" for c in self.cases)
        report = {
            "suite": "T2",
            "schema": "coldrevive.t2.report/v1",
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "mode": self.mode,
            "scope": self.cfg["environment"]["scope"],
            "cases_total": len(self.cases),
            "cases_passed": passed,
            "cases_failed": len(self.cases) - passed,
            "evidence_valid": False,
            "evidence_reason": self.cfg["evidence"]["reason"],
            "s6_status": "not_run",
            "g2_claim": False,
            "protocol_version": self.cfg["protocol"]["version"],
            "mock": {
                "stun_responses": self.stun.responses if self.stun else 0,
                "relay_forwarded": self.relay.relayed if self.relay else 0,
            },
            "cases": self.cases,
            "observations": self.observations,
        }
        REPORT_DIR.mkdir(parents=True, exist_ok=True)
        out = REPORT_DIR / f"t2-{utc_stamp()}-local.json"
        out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n",
                       encoding="utf-8")
        report["report_path"] = str(out.relative_to(ROOT).as_posix())
        return report

    def cleanup(self) -> None:
        self.stop.set()
        for p in self.procs.values():
            try:
                p.terminate()
                p.wait(timeout=5)
            except Exception:
                try:
                    p.kill()
                except Exception:
                    pass
        for f in self.logs:
            try:
                f.close()
            except Exception:
                pass
        if self.stun:
            self.stun.close()
        if self.relay:
            self.relay.close()
        if not self.keep and RUN_DIR.exists():
            # 仅清理本套件自建目录，保留 reports/ 作为本地证据
            shutil.rmtree(RUN_DIR, ignore_errors=True)


# ---------------------------------------------------------------------------
# --self-test：只验证 mock 与断言引擎，不启动守护进程
# ---------------------------------------------------------------------------

def self_test() -> int:
    print("T2 --self-test：mock STUN / 中继 wire 自检（无守护进程，evidence_valid=false）")
    cfg = json.loads(CFG_PATH.read_text(encoding="utf-8"))
    stop = threading.Event()
    # self-test 不占用真实守护进程端口：用临时源端口，并在 mock 映射表里登记它。
    probe_sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    probe_sock.bind(("127.0.0.1", 0))
    probe_port = probe_sock.getsockname()[1]
    ports = {probe_port: "127.0.0.2"}
    for n in cfg["nodes"].values():
        ports[n["wake_udp_port"]] = n["mock_external_ip"]
    stun = MockStun(ports, stop)
    stun.start()
    relay = MockRelay(cfg["nodes"]["A"]["wake_udp_port"], stop)
    relay.start()
    results = []
    try:
        # 1) 真实 STUN UDP 往返 + XOR-MAPPED 解码
        sock = probe_sock
        sock.settimeout(2)
        tid = bytes(range(12))
        req = struct.pack("!HH", 0x0001, 0) + MAGIC_BYTES + tid
        sock.sendto(req, ("127.0.0.1", stun.port))
        data, _ = sock.recvfrom(2048)
        parsed_tid = data[8:20]
        atype, alen = struct.unpack("!HH", data[20:24])
        val = data[24:24 + alen]
        xport = struct.unpack("!H", val[2:4])[0] ^ (MAGIC >> 16)
        xip = struct.unpack("!I", val[4:8])[0] ^ MAGIC
        ip = socket.inet_ntoa(struct.pack("!I", xip))
        ok = (data[0:2] == b"\x01\x01" and parsed_tid == tid
              and atype == 0x0020 and ip == "127.0.0.2"
              and xport == probe_port)
        results.append(("mock-stun-wire", ok, f"XOR-MAPPED={ip}:{xport}"))
        sock.close()

        # 2) 坏观测注入：非 STUN 报文不得触发应答
        bad = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        bad.bind(("127.0.0.1", 0))
        bad.settimeout(0.4)
        bad.sendto(b"not-a-stun-frame", ("127.0.0.1", stun.port))
        got_reply = True
        try:
            bad.recvfrom(64)
        except socket.timeout:
            got_reply = False
        results.append(("bad-frame-rejected", not got_reply,
                        "畸形帧未得到应答" if not got_reply else "畸形帧被误应答"))
        bad.close()

        # 3) 中继帧协议：REGISTER -> ACK，RELAY -> DELIVERED
        conn = socket.create_connection(("127.0.0.1", relay.port), timeout=2)
        node_id = b"B" + b"\x00" * 15
        conn.sendall(b"\x01" + node_id + struct.pack("!I", 16) + node_id)
        ack = recv_exact(conn, 21)
        payload = b"A" + b"\x00" * 15 + b'{"type":"WAKEUP_REQ"}'
        conn.sendall(b"\x02" + node_id + struct.pack("!I", len(payload)) + payload)
        head = recv_exact(conn, 21)
        kind = head[0] if head else 0
        results.append(("relay-frame-protocol", ack[0] == 4 and kind == 3,
                        f"REGISTER→ACK={ack[0] == 4}, RELAY→DELIVERED={kind == 3}"))
        conn.close()
    finally:
        stop.set()
        probe_sock.close()
        stun.close()
        relay.close()

    for name, ok, detail in results:
        print(f"  [{'PASS' if ok else 'FAIL'}] {name} — {detail}")
    passed = sum(ok for _, ok, _ in results)
    print(f"self-test: {passed}/{len(results)} PASS（全部为本地 mock wire 自检，非 S6 证据）")
    return 0 if passed == len(results) else 1


def main() -> int:
    ap = argparse.ArgumentParser(description="T2 M2 跨 NAT 复活路径接线验收")
    ap.add_argument("--mode", choices=["local"], default="local")
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("--keep", action="store_true", help="保留 .run/ 便于排查")
    args = ap.parse_args()
    cfg = json.loads(CFG_PATH.read_text(encoding="utf-8"))
    if args.self_test:
        return self_test()
    try:
        report = T2(cfg, args.mode, args.keep).run()
    except Exception as e:
        print(f"T2 执行失败: {type(e).__name__}: {e}", file=sys.stderr)
        return 2
    print(f"\nT2 {args.mode}: {report['cases_passed']}/{report['cases_total']} PASS, "
          f"evidence_valid={report['evidence_valid']}, s6_status={report['s6_status']}")
    print(f"报告: {report['report_path']}")
    return 0 if report["cases_failed"] == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
