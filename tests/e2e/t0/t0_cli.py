#!/usr/bin/env python3
"""T0 真实观测采集层（CLI v1 契约实现）。

职责：把 t0_cases.json 的每条用例映射到 coldrevive-cli 的真实 JSON 输出，
外加少量从本机真实文件 / 配置 / golden 独立计算的字段。

红线：**绝不合成观测值**。二进制不存在、命令失败、JSON 解析失败、字段缺失，
一律抛 CliUnavailableError（由 CliAdapter 转成 BLOCKED / NotWiredError），
绝不返回「看起来像通过」的默认值。

CLI v1 契约（提案，M1 定稿时确认）：
    coldrevive-cli --json --node <A|B> <command> [args...]  → stdout 单个 JSON 对象
    health: GET http://<bind>:<port>/health → 200

命令集：inspect / find / wake / scenario / metrics / golden / download / env
完整契约见 tests/e2e/t0/README.md §5.1 与 t0_config.json 的 launch 段。
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
from pathlib import Path
from typing import Any

from t0_adapters import Observation

# ---------------------------------------------------------------------------
# 观测映射表：case_id → 需要发起的真实 CLI 调用 + 字段提取规则
# fields: 断言字段名 → 响应 JSON 中的点分路径（支持列表下标）
# derive: 由本机真实数据独立计算的字段名（不来自产品自报）
# ---------------------------------------------------------------------------

OBSERVE_PLAN: dict[str, dict[str, Any]] = {
    "T0-01": {
        "calls": [{"node": "A", "command": "inspect", "args": ["file", "{file_hash}"],
                   "fields": {"indexed": "file.indexed",
                              "torrent_generated": "file.torrent_generated",
                              "torrent_cached": "file.torrent_cached"}}],
        "derive": ["fixture_exists", "fixture_sha256"],
    },
    "T0-02": {
        "calls": [{"node": "A", "command": "inspect", "args": ["index", "{file_hash}"],
                   "fields": {"file_state": "file.state",
                              "shadow_index": "index.shadow",
                              "kad_published": "index.kad_published"}}],
    },
    "T0-03": {
        "calls": [
            {"node": "A", "command": "inspect", "args": ["kad"],
             "fields": {"namespace_ok": "kad.namespace_ok",
                        "keyword_count": "kad.keyword_count",
                        "published_keys": "kad.published_keys"}},
            {"node": "B", "command": "find", "args": ["{keyword}"],
             "fields": {"find_hit_node": "result.hit_node",
                        "find_result_count": "result.count"}},
        ],
    },
    "T0-04": {
        "calls": [{"node": "B", "command": "wake", "args": ["probe", "{file_hash}", "{infohash}"],
                   "fields": {"wake_req_valid": "req.valid",
                              "challenge_ok": "req.challenge_ok",
                              "nonce_unique": "req.nonce_unique",
                              "time_window_ok": "req.time_window_ok"}}],
    },
    "T0-05": {
        "calls": [{"node": "B", "command": "wake", "args": ["ack", "{file_hash}", "{infohash}"],
                   "fields": {"wake_status": "ack.status",
                              "ack_fields_ok": "ack.fields_ok",
                              "ack_has_magnet": "ack.has_magnet",
                              "ack_has_torrent_url": "ack.has_torrent_url",
                              "ack_ttl": "ack.ttl",
                              "ack_bt_port": "ack.bt_port"}}],
    },
    "T0-06": {
        "calls": [{"node": "B", "command": "wake", "args": ["ack", "{file_hash}", "{infohash}"],
                   "fields": {"magnet_infohash": "ack.magnet_infohash",
                              "torrent_fetch_ok": "ack.torrent_fetch_ok",
                              "torrent_parse_ok": "ack.torrent_parse_ok"}}],
        "derive": ["expected_infohash"],
    },
    "T0-07": {
        "calls": [{"node": "A", "command": "inspect", "args": ["state", "{file_hash}"],
                   "fields": {"state_transition": "state.transitions",
                              "bt_port_listening": "state.bt_port_listening"}}],
    },
    "T0-08": {
        # download 内部由第三方 BT 客户端真实下载，daemon 侧 run_with_timeout 上限 120 s；
        # 观测客户端必须给到 >=120 s，否则 CLI 被提前杀掉而误判 BLOCKED（v2.4.47 修正）。
        "calls": [{"node": "B", "command": "download", "args": ["{magnet}"],
                   "timeout": 190.0,
                   "fields": {"private_protocol_in_data_plane": "download.private_protocol_seen",
                              "download_complete": "download.complete",
                              "downloaded_sha256": "download.sha256",
                              "piece_sha1_ok": "download.piece_sha1_ok"}}],
    },
    "T0-09": {
        "calls": [{"node": "B", "command": "scenario", "args": ["loopback", "{file_hash}"],
                   "fields": {"loopback_index_ok": "loopback.indexed",
                              "loopback_published": "loopback.published"}}],
    },
    "T0-10": {
        "calls": [{"node": "B", "command": "metrics",
                   "args": ["wake-latency", "{sample_count}"],
                   "fields": {"wake_latency_samples_ms": "samples_ms"}}],
    },
    "T0-11": {
        "calls": [{"node": "B", "command": "metrics", "args": ["revival", "{sample_count}"],
                   "fields": {"revival_success": "success", "revival_total": "total"}}],
    },
    "T0-12": {
        "calls": [{"node": "B", "command": "scenario", "args": ["file-gone", "{file_hash}"],
                   "fields": {"reject_code": "reject.code", "kad_revoked": "reject.kad_revoked"}}],
    },
    "T0-13": {
        "calls": [{"node": "B", "command": "scenario", "args": ["bad-challenge", "{file_hash}"],
                   "fields": {"reject_code": "reject.code", "retry_once": "reject.retry_once"}}],
    },
    "T0-14": {
        "calls": [{"node": "B", "command": "scenario", "args": ["not-whitelisted", "{file_hash}"],
                   "fields": {"reject_code": "reject.code", "existence_leak": "reject.existence_leak"}}],
    },
    "T0-15": {
        "calls": [{"node": "B", "command": "scenario", "args": ["hash-mismatch", "{file_hash}"],
                   "fields": {"reject_code": "reject.code", "node_blacklisted": "reject.blacklisted"}}],
    },
    "T0-16": {
        "calls": [{"node": "B", "command": "scenario", "args": ["holder-offline", "{file_hash}"],
                   "fields": {"wake_timeout_ms": "timeout.elapsed_ms",
                              "candidate_switched": "timeout.candidate_switched",
                              "ui_blocked": "timeout.ui_blocked"}}],
    },
    "T0-17": {
        "calls": [{"node": "B", "command": "scenario", "args": ["nonce-replay", "{file_hash}"],
                   "fields": {"replay_detected": "replay.detected",
                              "duplicate_activation": "replay.duplicate_activation"}}],
    },
    "T0-18": {
        "calls": [{"node": "B", "command": "scenario", "args": ["bad-version", "{file_hash}"],
                   "fields": {"reject_code": "reject.code"}}],
    },
    "T0-19": {
        "calls": [{"node": "A", "command": "golden", "args": ["verify", "v1"],
                   "fields": {"golden_req_ok": "req_ok", "golden_ack_ok": "ack_ok"}}],
        "derive": ["golden_req_ok", "golden_ack_ok"],
    },
    "T0-20": {
        "calls": [{"node": "A", "command": "env", "args": ["defaults"],
                   "fields": {"shared_dirs_empty_by_default": "defaults.shared_dirs_empty",
                              "rpc_secret_required_ok": "defaults.rpc_secret_required",
                              "residual_pids": "env.residual_pids"}}],
        "derive": ["bind_loopback", "ports_distinct"],
    },
}


class CliUnavailableError(RuntimeError):
    """真实观测不可用。绝不降级为合成值。"""


def dig(data: Any, path: str) -> tuple[bool, Any]:
    """按点分路径取值；返回 (是否找到, 值)。列表下标用数字段表示。"""
    cur = data
    for part in path.split("."):
        if isinstance(cur, dict) and part in cur:
            cur = cur[part]
        elif isinstance(cur, (list, tuple)) and part.isdigit() and int(part) < len(cur):
            cur = cur[int(part)]
        else:
            return False, None
    return True, cur


class CliClient:
    """coldrevive-cli 的 JSON 调用封装。任何异常都不降级、不伪造。"""

    def __init__(self, cfg: dict[str, Any]):
        self.cfg = cfg
        env_name = cfg["bin"]["cli_env"]
        self.binary = os.environ.get(env_name, "").strip()
        if not self.binary:
            raise CliUnavailableError(f"未设置 {env_name}（coldrevive-cli 路径）")
        if not Path(self.binary).is_file():
            raise CliUnavailableError(f"{env_name} 指向的文件不存在: {self.binary}")

    def call(self, node: str, command: str, args: list[str], timeout: float = 10.0) -> dict[str, Any]:
        template = self.cfg["launch"]["cli_argv"]
        raw = [part.format(cli=self.binary, node=node, command=command)
               for part in template]
        argv = raw + list(args)
        try:
            r = subprocess.run(argv, capture_output=True, text=True,
                               timeout=timeout, check=False)
        except (OSError, subprocess.TimeoutExpired) as exc:
            raise CliUnavailableError(f"CLI 调用失败 ({command}): {exc}") from exc
        if r.returncode != 0:
            raise CliUnavailableError(
                f"CLI 返回 {r.returncode} ({command}): {(r.stderr or r.stdout).strip()[:300]}")
        try:
            data = json.loads(r.stdout)
        except json.JSONDecodeError as exc:
            raise CliUnavailableError(f"CLI 输出不是合法 JSON ({command}): {exc}") from exc
        if not isinstance(data, dict):
            raise CliUnavailableError(f"CLI 输出应为 JSON 对象 ({command})")
        return data

    def torrent_path(self, node: str, file_hash: str) -> Path:
        """从产品取得伴随种子路径；路径本身由产品自报，内容由本机独立解析。"""
        data = self.call(node, "inspect", ["torrent", file_hash])
        ok, path = dig(data, "file.torrent_path")
        if not ok or not isinstance(path, str) or not Path(path).is_file():
            raise CliUnavailableError("伴随种子路径缺失或文件不存在（T0-06 需真实 .torrent）")
        return Path(path)


def _bdecode(data: bytes, i: int = 0) -> tuple[Any, int]:
    """最小 bencode 解码，仅用于定位 torrent 的 info 字典原始字节。"""
    if i >= len(data):
        raise ValueError("bencode 提前结束")
    c = data[i:i + 1]
    if c == b"i":
        j = data.index(b"e", i)
        return int(data[i + 1:j]), j + 1
    if c == b"l":
        i += 1
        out = []
        while data[i:i + 1] != b"e":
            v, i = _bdecode(data, i)
            out.append(v)
        return out, i + 1
    if c == b"d":
        i += 1
        out: dict[bytes, Any] = {}
        while data[i:i + 1] != b"e":
            k, i = _bdecode(data, i)
            v, i = _bdecode(data, i)
            out[k] = v
        return out, i + 1
    if c.isdigit():
        j = data.index(b":", i)
        n = int(data[i:j])
        return data[j + 1:j + 1 + n], j + 1 + n
    raise ValueError(f"非法 bencode 类型: {c!r}")


def torrent_infohash(path: Path) -> str:
    """按 BT 规范对 .torrent 的 info 字典原始字节做 SHA-1，得到真实 InfoHash。

    替代骨架期的 infohash_stub：这是接线清单第 4 条的落地点。
    """
    raw = path.read_bytes()
    top, _ = _bdecode(raw, 0)
    if not isinstance(top, dict) or b"info" not in top:
        raise CliUnavailableError(f"{path} 不是合法 .torrent（缺 info 字典）")
    marker = b"4:info"
    pos = raw.find(marker)
    if pos < 0:
        raise CliUnavailableError(f"{path} 中未找到 info 字段")
    start = pos + len(marker)
    _, end = _bdecode(raw, start)
    return hashlib.sha1(raw[start:end]).hexdigest()


def derive_values(names: list[str], ctx: dict[str, Any], client: CliClient) -> dict[str, Any]:
    """从本机真实数据独立计算的字段，不采信产品自报。"""
    cfg, fixture = ctx["cfg"], ctx["fixture"]
    out: dict[str, Any] = {}
    for name in names:
        if name == "fixture_exists":
            out[name] = fixture.path.is_file()
        elif name == "fixture_sha256":
            out[name] = hashlib.sha256(fixture.path.read_bytes()).hexdigest()
        elif name == "expected_infohash":
            out[name] = torrent_infohash(client.torrent_path("A", fixture.sha256))
        elif name == "bind_loopback":
            out[name] = cfg["bind"] == "127.0.0.1"
        elif name == "ports_distinct":
            ports = [int(spec[k]) for spec in cfg["nodes"].values()
                     for k in ("http_port", "wake_udp_port", "bt_port")]
            out[name] = len(ports) == len(set(ports))
        elif name in ("golden_req_ok", "golden_ack_ok"):
            # 以本机 golden 文件独立校验为准，覆盖产品自报，避免「自己证明自己」
            g = ctx["golden"]
            req = g.get("wakeup_req.v1.json", {})
            ack = g.get("wakeup_ack.v1.json", {})
            if name == "golden_req_ok":
                out[name] = (set(req) == REQ_FIELDS_LOCAL and req.get("v") == 1
                             and req.get("type") == "WAKEUP_REQ"
                             and req.get("capabilities", {}).get("ed2k") is False)
            else:
                out[name] = (set(ack) == ACK_FIELDS_LOCAL and ack.get("v") == 1
                             and ack.get("type") == "WAKEUP_ACK"
                             and ack.get("status") == "OK")
    return out


REQ_FIELDS_LOCAL = {
    "v", "type", "trace_id", "file_hash", "infohash", "nonce", "ts",
    "challenge", "client_version", "capabilities",
}
ACK_FIELDS_LOCAL = {
    "v", "type", "trace_id", "status", "magnet", "torrent_url",
    "file_size", "piece_size", "ttl", "bt_port",
}


def placeholders(ctx: dict[str, Any], client: CliClient) -> dict[str, str]:
    """真实链路必须使用伴随种子的真实 InfoHash，不得沿用骨架期的 infohash_stub。"""
    cfg, f = ctx["cfg"], ctx["fixture"]
    real_infohash = torrent_infohash(client.torrent_path("A", f.sha256))
    return {
        "file_hash": f.sha256,
        "infohash": real_infohash,
        "keyword": f.keywords[0] if f.keywords else "coldrevive",
        "sample_count": str(cfg["thresholds"]["wake_sample_count"]),
        "magnet": "magnet:?xt=urn:btih:" + real_infohash,
    }


def collect(case_id: str, ctx: dict[str, Any]) -> Observation:
    """执行真实观测。任何一步不可用都抛异常，绝不返回合成值。"""
    spec = OBSERVE_PLAN.get(case_id)
    if not spec:
        raise CliUnavailableError(f"OBSERVE_PLAN 缺少 {case_id} 的映射")
    client = CliClient(ctx["cfg"])
    subs = placeholders(ctx, client)
    values: dict[str, Any] = {}
    for call in spec["calls"]:
        args = [a.format(**subs) for a in call.get("args", [])]
        data = client.call(call["node"], call["command"], args,
                           timeout=float(call.get("timeout", 10.0)))
        for key, path in call["fields"].items():
            found, value = dig(data, path)
            if not found:
                # 字段缺失不放进 values：断言引擎会明确报「观测字段缺失」并判 FAIL
                continue
            values[key] = value
    values.update(derive_values(spec.get("derive", []), ctx, client))
    return Observation(
        case_id=case_id,
        values=values,
        source="cli",
        note=f"真实 CLI 观测（{len(spec['calls'])} 次调用"
             + (f" + 本机独立计算 {len(spec.get('derive', []))} 项" if spec.get("derive") else "")
             + "）；字段缺失将由断言引擎判 FAIL",
    )
