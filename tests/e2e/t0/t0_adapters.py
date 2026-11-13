#!/usr/bin/env python3
"""T0 观测契约：把「怎么取观测值」与「怎么断言」解耦。

- StubAdapter：合成观测，用于验证用例编排、断言引擎与报告格式；绝不可作为 G1 证据。
- CliAdapter：真实观测入口，当前未接线（M1 二进制尚不存在），调用即抛 NotWiredError。

接线时只改本文件与 t0_config.json 的 launch 契约，断言与用例清单保持不变。
"""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

# PROTO §3.2 / §3.3 的字段契约，用于 golden 校验
REQ_FIELDS = {
    "v", "type", "trace_id", "file_hash", "infohash", "nonce", "ts",
    "challenge", "client_version", "capabilities",
}
ACK_FIELDS = {
    "v", "type", "trace_id", "status", "magnet", "torrent_url",
    "file_size", "piece_size", "ttl", "bt_port",
}

# 接线后必须实现的观测方法（对应 t0_cases.json 中 T0-01～T0-20）
REQUIRED_OBSERVERS = tuple(f"observe_t0_{i:02d}" for i in range(1, 21))


class NotWiredError(RuntimeError):
    """真实观测尚未接线。用于把用例标记为 BLOCKED，而不是伪装成 FAIL/PASS。"""


@dataclass
class Observation:
    case_id: str
    values: dict[str, Any] = field(default_factory=dict)
    source: str = "stub"
    note: str = ""


class BaseAdapter:
    name = "base"
    wired = False
    evidence_valid = False

    def observe(self, case_id: str, ctx: dict[str, Any]) -> Observation:
        raise NotImplementedError


class StubAdapter(BaseAdapter):
    """确定性合成观测。golden 契约、端口与命名空间这几项从真实文件/配置计算。"""

    name = "stub"
    evidence_valid = False

    def observe(self, case_id: str, ctx: dict[str, Any]) -> Observation:
        cfg = ctx["cfg"]
        fixture = ctx["fixture"]
        values = self._ideal(cfg, fixture)
        values.update(self._case_specific(case_id, cfg, fixture, ctx))
        return Observation(
            case_id=case_id,
            values=values,
            source="stub",
            note="合成观测（部分字段取自真实配置与 golden 文件）；仅验证编排与断言引擎，不可作为 G1 门禁证据",
        )

    @staticmethod
    def _ideal(cfg: dict[str, Any], fixture: Any) -> dict[str, Any]:
        n = int(cfg["thresholds"]["wake_sample_count"])
        ports = [int(spec[k]) for spec in cfg["nodes"].values()
                 for k in ("http_port", "wake_udp_port", "bt_port")]
        return {
            # F1 入库与伴随造种
            "fixture_exists": True,
            "fixture_sha256": fixture.sha256,
            "indexed": True,
            "torrent_generated": True,
            "torrent_cached": True,
            # 影子索引与冷态
            "file_state": "COLD",
            "shadow_index": True,
            "kad_published": True,
            "published_keys": [fixture.target_key] + list(fixture.keyword_keys),
            "namespace_ok": True,
            "keyword_count": len(fixture.keyword_keys),
            # F2 发现
            "find_hit_node": "A",
            "find_result_count": 1,
            # F3 唤醒
            "wake_req_valid": True,
            "challenge_ok": True,
            "nonce_unique": True,
            "time_window_ok": True,
            "wake_status": "OK",
            "ack_fields_ok": True,
            "ack_has_magnet": True,
            "ack_has_torrent_url": True,
            "ack_ttl": 7200,
            "ack_bt_port": cfg["nodes"]["A"]["bt_port"],
            "magnet_infohash": fixture.infohash_stub,
            "expected_infohash": fixture.infohash_stub,
            "torrent_fetch_ok": True,
            "torrent_parse_ok": True,
            # 状态跃迁与按需做种
            "state_transition": ["COLD", "WAKING", "HOT"],
            "bt_port_listening": True,
            # F4 标准数据面
            "private_protocol_in_data_plane": False,
            "download_client": "third-party-bt (stub)",
            "download_complete": True,
            "downloaded_sha256": fixture.sha256,
            "piece_sha1_ok": True,
            # 下载闭环
            "loopback_index_ok": True,
            "loopback_published": True,
            # 指标
            "wake_latency_samples_ms": [180 + (i * 37) % 420 for i in range(n)],
            "revival_success": n,
            "revival_total": n,
            # 环境与契约（从真实配置计算）
            "bind_loopback": cfg["bind"] == "127.0.0.1",
            "ports_distinct": len(ports) == len(set(ports)),
            "residual_pids": [],
            "default_reject": None,
        }

    @staticmethod
    def _case_specific(case_id: str, cfg: dict[str, Any],
                       fixture: Any, ctx: dict[str, Any]) -> dict[str, Any]:
        overrides: dict[str, dict[str, Any]] = {
            "T0-12": {"reject_code": "REJECT_FILE_GONE", "kad_revoked": True},
            "T0-13": {"reject_code": "REJECT_BAD_CHALLENGE", "retry_once": True},
            "T0-14": {"reject_code": "REJECT_NOT_FOUND", "existence_leak": False},
            "T0-15": {"reject_code": "HASH_MISMATCH", "node_blacklisted": True},
            "T0-16": {"wake_timeout_ms": 1500, "candidate_switched": True, "ui_blocked": False},
            "T0-17": {"replay_detected": True, "duplicate_activation": False},
            "T0-18": {"reject_code": "REJECT_VERSION"},
        }
        out = dict(overrides.get(case_id, {}))

        if case_id == "T0-19":
            req = ctx["golden"].get("wakeup_req.v1.json", {})
            ack = ctx["golden"].get("wakeup_ack.v1.json", {})
            out["golden_req_ok"] = (
                set(req) == REQ_FIELDS
                and req.get("v") == 1
                and req.get("type") == "WAKEUP_REQ"
                and req.get("capabilities", {}).get("ed2k") is False
            )
            out["golden_ack_ok"] = (
                set(ack) == ACK_FIELDS
                and ack.get("v") == 1
                and ack.get("type") == "WAKEUP_ACK"
                and ack.get("status") == "OK"
            )
        if case_id == "T0-20":
            out["shared_dirs_empty_by_default"] = True
            out["rpc_secret_required_ok"] = True
        return out


class CliAdapter(BaseAdapter):
    """真实观测入口：按 CLI v1 契约从 coldrevive-cli 取数，绝不合成。

    接线状态（2026-11-13，M1）：观测采集层、M1 二进制与双节点链路均已就位，
    `wired = True` 表示**代码层已接线**；但每次 real 运行仍会动态校验三个二进制
    环境变量确实指向真实文件，缺任一项即抛 NotWiredError（CI 无二进制时仍 BLOCKED）。
    是否构成 G1 门禁证据由 `evidence_valid` 与用例 PASS/FAIL 决定，与本开关无关。
    """

    name = "cli"
    wired = True
    evidence_valid = False

    def __init__(self, harness: Any = None):
        self.harness = harness

    def _require_binaries(self, cfg: dict[str, Any]) -> None:
        """环境级防呆：本机没有真实二进制时，real 模式必须继续 BLOCKED。"""
        import t0_cli  # noqa: F401

        missing = []
        for env in cfg["bin"].values():
            raw = os.environ.get(env, "").strip()
            if not raw or not Path(raw).is_file():
                missing.append(f"{env}={raw or '<未设置>'}")
        if missing:
            raise NotWiredError(
                "真实二进制未就绪，--mode real 保持 BLOCKED：" + "；".join(missing)
            )

    def _collect(self, case_id: str, ctx: dict[str, Any]) -> Observation:
        # 延迟导入，避免与 t0_cli 顶部的 `from t0_adapters import Observation` 形成循环
        import t0_cli

        full_ctx = {**ctx, "harness": self.harness}
        try:
            result = t0_cli.collect(case_id, full_ctx)
        except t0_cli.CliUnavailableError as exc:
            raise NotWiredError(f"{case_id} 真实观测不可用：{exc}") from exc
        result.source = "cli"
        return result

    def preflight(self, cfg: dict[str, Any]) -> dict[str, Any]:
        """返回接线缺口，供 real 模式报告列出 next_actions。"""
        import t0_cli

        binaries = {
            env_name: os.environ.get(env_name) or ""
            for env_name in cfg["bin"].values()
        }
        missing = [name for name in REQUIRED_OBSERVERS if not hasattr(self, name)]
        unset = [k for k, v in binaries.items() if not v]
        return {
            "wired": self.wired,
            "binaries": binaries,
            "contract": cfg["launch"].get("schema", "unknown"),
            "observe_plan_size": len(t0_cli.OBSERVE_PLAN),
            "missing_observers": missing,
            "infohash_source": "伴随 .torrent 的 info 字典 SHA-1（t0_cli.torrent_infohash）",
            "blockers": [
                *([f"未设置环境变量 {k}" for k in unset]),
                *(["观测方法未实现"] if missing else []),
                "CliAdapter.wired 为代码层接线开关；本机二进制就绪性由 real 运行前动态校验",
            ],
        }

    def observe(self, case_id: str, ctx: dict[str, Any]) -> Observation:
        if not self.wired:
            raise NotWiredError(
                "CliAdapter 未接线：本次运行的真实二进制或双节点链路未就绪，"
                "见 tests/e2e/t0/README.md §5 接线清单与 tests/spikes/m0_preflight.py"
            )
        self._require_binaries(ctx["cfg"])
        method = getattr(self, "observe_" + case_id.replace("-", "_").lower(), None)
        if method is None:
            raise NotWiredError(f"缺少 {case_id} 的观测实现")
        result = method(ctx)
        result.source = "cli"
        return result


def _bind_observers() -> None:
    """把 20 条用例显式绑定到 CliAdapter，使 preflight 能真实反映接线完成度。"""
    for name in REQUIRED_OBSERVERS:
        case_id = "T0-" + name[-2:]

        def factory(cid: str):
            def _fn(self: CliAdapter, ctx: dict[str, Any]) -> Observation:
                return self._collect(cid, ctx)
            return _fn

        fn = factory(case_id)
        fn.__name__ = name
        fn.__doc__ = f"{case_id} 的真实 CLI 观测（契约见 t0_cli.OBSERVE_PLAN）"
        setattr(CliAdapter, name, fn)


_bind_observers()


def make_adapter(mode: str, harness: Any = None) -> BaseAdapter:
    if mode == "stub":
        return StubAdapter()
    if mode == "real":
        return CliAdapter(harness)
    raise ValueError(f"未知 mode: {mode!r}（应为 stub 或 real）")
