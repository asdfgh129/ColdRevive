#!/usr/bin/env python3
"""ColdRevive T0 双节点验收入口：用例驱动、断言引擎、JSON 报告、退出码。

设计红线：stub 自检永远标记 evidence_valid=false；real 模式未接线永远 BLOCKED。
绝不把「没跑」显示成「绿」。

    python3 tests/e2e/t0/t0_accept.py --list
    python3 tests/e2e/t0/t0_accept.py --self-test
    python3 tests/e2e/t0/t0_accept.py --mode real
"""

from __future__ import annotations

import argparse
import json
import math
import os
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable

from t0_adapters import NotWiredError, Observation, StubAdapter, make_adapter
from t0_harness import ROOT, Harness, load_cases, load_config

HERE = Path(__file__).resolve().parent

# ---------------------------------------------------------------------------
# 断言表：t0_cases.json 定义「验什么」，这里定义「怎么验」，按 id 一一对应。
# 每项 = (说明, 观测字段, 操作符, 期望值)；期望值可用 lambda ctx 从配置/样本取值。
# ---------------------------------------------------------------------------

Spec = tuple[str, str, str, Any]

ASSERTS: dict[str, list[Spec]] = {
    "T0-01": [
        ("样本文件已生成", "fixture_exists", "true", None),
        ("SHA-256 与确定性样本一致", "fixture_sha256", "eq", lambda c: c["fixture"].sha256),
        ("影子索引写入成功", "indexed", "true", None),
        ("伴随种子已生成", "torrent_generated", "true", None),
        ("伴随种子已缓存到元数据目录", "torrent_cached", "true", None),
    ],
    "T0-02": [
        ("文件状态为 COLD（物理休眠）", "file_state", "eq", "COLD"),
        ("影子索引保留通告（不撤销）", "shadow_index", "true", None),
        ("KAD 命名空间 Key 已发布", "kad_published", "true", None),
    ],
    "T0-03": [
        ("使用专属命名空间 ColdRevive_V1:", "namespace_ok", "true", None),
        ("关键词 Key 数不超过 8", "keyword_count", "lte", 8),
        ("发布 Key 含 Target_Key", "published_keys", "contains", lambda c: c["fixture"].target_key),
        ("FIND_VALUE 命中持有者 A", "find_hit_node", "eq", "A"),
        ("结果集非空", "find_result_count", "gte", 1),
    ],
    "T0-04": [
        ("WAKEUP_REQ 字段合法", "wake_req_valid", "true", None),
        ("Challenge 校验通过", "challenge_ok", "true", None),
        ("nonce 唯一（未重放）", "nonce_unique", "true", None),
        ("时间窗 ±5 min 内", "time_window_ok", "true", None),
    ],
    "T0-05": [
        ("ACK status = OK", "wake_status", "eq", "OK"),
        ("ACK 必填字段齐全", "ack_fields_ok", "true", None),
        ("ACK 含 magnet", "ack_has_magnet", "true", None),
        ("ACK 含 torrent_url", "ack_has_torrent_url", "true", None),
        ("ttl 大于 0", "ack_ttl", "gte", 1),
        ("bt_port 与 A 配置一致", "ack_bt_port", "eq", lambda c: c["cfg"]["nodes"]["A"]["bt_port"]),
    ],
    "T0-06": [
        ("magnet InfoHash 与请求一致", "magnet_infohash", "eq_key", "expected_infohash"),
        ("伴随种子可从 torrent_url 取得", "torrent_fetch_ok", "true", None),
        ("伴随种子可解析", "torrent_parse_ok", "true", None),
    ],
    "T0-07": [
        ("状态跃迁 COLD → WAKING → HOT", "state_transition", "eq", ["COLD", "WAKING", "HOT"]),
        ("标准 BT 端口已就绪", "bt_port_listening", "true", None),
    ],
    "T0-08": [
        ("数据面不含私有唤醒协议", "private_protocol_in_data_plane", "false", None),
        ("第三方 BT 客户端完成下载", "download_complete", "true", None),
        ("下载文件 SHA-256 与样本一致", "downloaded_sha256", "eq", lambda c: c["fixture"].sha256),
        ("逐片 SHA-1 校验通过", "piece_sha1_ok", "true", None),
    ],
    "T0-09": [
        ("下载完成触发 F1 入库闭环", "loopback_index_ok", "true", None),
        ("B 成为新持有者并发布索引", "loopback_published", "true", None),
    ],
    "T0-10": [
        ("采样数达到配置要求", "wake_latency_samples_ms", "len_gte",
         lambda c: c["cfg"]["thresholds"]["wake_sample_count"]),
        ("唤醒时延 P95 低于阈值", "wake_latency_samples_ms", "p95_lt",
         lambda c: c["cfg"]["thresholds"]["wake_p95_ms"]),
        ("无超阈值失败样本", "wake_latency_samples_ms", "all_lte",
         lambda c: c["cfg"]["thresholds"]["wake_p95_ms"]),
    ],
    "T0-11": [
        ("复活成功数等于尝试总数", "revival_success", "eq_key", "revival_total"),
        ("样本量非零", "revival_total", "gte", 1),
    ],
    "T0-12": [
        ("返回 REJECT_FILE_GONE", "reject_code", "eq", "REJECT_FILE_GONE"),
        ("撤销该 Key 的 KAD 发布", "kad_revoked", "true", None),
    ],
    "T0-13": [
        ("返回 REJECT_BAD_CHALLENGE", "reject_code", "eq", "REJECT_BAD_CHALLENGE"),
        ("重试一次后放弃", "retry_once", "true", None),
    ],
    "T0-14": [
        ("返回 REJECT_NOT_FOUND", "reject_code", "eq", "REJECT_NOT_FOUND"),
        ("不泄露文件存在性", "existence_leak", "false", None),
    ],
    "T0-15": [
        ("请求方判定 HASH_MISMATCH", "reject_code", "eq", "HASH_MISMATCH"),
        ("拉黑该节点", "node_blacklisted", "true", None),
    ],
    "T0-16": [
        ("1.5 s 内超时", "wake_timeout_ms", "lte", 1500),
        ("切换到下一候选", "candidate_switched", "true", None),
        ("不阻塞 UI", "ui_blocked", "false", None),
    ],
    "T0-17": [
        ("nonce 重放被识别", "replay_detected", "true", None),
        ("未重复激活做种", "duplicate_activation", "false", None),
    ],
    "T0-18": [
        ("返回 REJECT_VERSION", "reject_code", "eq", "REJECT_VERSION"),
    ],
    "T0-19": [
        ("WAKEUP_REQ golden 与 PROTO v1 一致", "golden_req_ok", "true", None),
        ("WAKEUP_ACK golden 与 PROTO v1 一致", "golden_ack_ok", "true", None),
    ],
    "T0-20": [
        ("默认仅回环监听", "bind_loopback", "true", None),
        ("双实例端口互不冲突", "ports_distinct", "true", None),
        ("进程退出无残留", "residual_pids", "eq", []),
        ("默认共享目录为空", "shared_dirs_empty_by_default", "true", None),
        ("非回环必须配置 rpc_secret", "rpc_secret_required_ok", "true", None),
    ],
}


def resolve_expect(expect: Any, ctx: dict[str, Any]) -> Any:
    return expect(ctx) if callable(expect) else expect


def compare(actual: Any, op: str, expect: Any, values: dict[str, Any]) -> tuple[bool, Any]:
    """返回 (是否通过, 报告中展示的实际值)。未知操作符一律判失败。"""
    try:
        if op == "eq":
            return actual == expect, actual
        if op == "eq_key":
            return actual == values.get(expect, "<缺失>"), actual
        if op == "ne":
            return actual != expect, actual
        if op == "true":
            return actual is True, actual
        if op == "false":
            return actual is False, actual
        if op == "lte":
            return actual <= expect, actual
        if op == "gte":
            return actual >= expect, actual
        if op == "lt":
            return actual < expect, actual
        if op == "contains":
            return expect in actual, actual
        if op == "len_gte":
            return len(actual) >= expect, len(actual)
        if op == "all_lte":
            return all(x <= expect for x in actual), f"max={max(actual)}"
        if op == "p95_lt":
            ordered = sorted(actual)
            idx = max(0, math.ceil(0.95 * len(ordered)) - 1)
            p95 = ordered[idx]
            return p95 < expect, f"P95={p95} ms"
    except (TypeError, ValueError, KeyError, IndexError, ZeroDivisionError) as exc:
        return False, f"比较异常: {exc}"
    return False, f"未知操作符 {op}"


def run_assertions(case_id: str, obs: Observation, ctx: dict[str, Any]) -> list[dict[str, Any]]:
    results: list[dict[str, Any]] = []
    for label, key, op, raw_expect in ASSERTS.get(case_id, []):
        expect = resolve_expect(raw_expect, ctx)
        if key not in obs.values:
            results.append({"label": label, "ok": False, "actual": "<观测字段缺失>",
                            "expect": repr(expect), "key": key})
            continue
        ok, shown = compare(obs.values[key], op, expect, obs.values)
        results.append({"label": label, "ok": ok, "actual": shown,
                        "expect": expect, "key": key})
    return results


def framework_check(ctx: dict[str, Any]) -> list[dict[str, Any]]:
    """T0-00：证明断言引擎既不会永远 PASS，也不会误报 FAIL。"""
    adapter = StubAdapter()
    good = adapter.observe("T0-08", ctx)
    bad = Observation("T0-08", {**good.values, "downloaded_sha256": "deadbeef"},
                      "stub", "故障注入：故意损坏哈希")
    good_results = run_assertions("T0-08", good, ctx)
    bad_results = run_assertions("T0-08", bad, ctx)
    failed = [r["label"] for r in bad_results if not r["ok"]]
    return [
        {"label": "理想观测全部通过", "ok": all(r["ok"] for r in good_results),
         "actual": f"{sum(r['ok'] for r in good_results)}/{len(good_results)} PASS",
         "expect": "全部 PASS", "key": "—"},
        {"label": "损坏哈希必须被至少一条断言捕获", "ok": len(failed) > 0,
         "actual": f"捕获 {len(failed)} 条: {', '.join(failed) or '无'}",
         "expect": ">= 1 条 FAIL", "key": "—"},
    ]


def setup_real_env(cfg: dict[str, Any], harness: Any, fixture: Any) -> tuple[bool, str]:
    """M1 接线：写双节点配置、启动 A/B 守护进程、等待健康检查。

    只做真实环境准备，不产生任何观测值、不判任何 pass。任一二进制缺失或健康
    检查失败时返回 (False, 原因)，调用方把本次 real 运行标记为 BLOCKED。
    """
    binaries = {
        name: os.environ.get(env, "").strip()
        for name, env in (("daemon", cfg["bin"]["daemon_env"]),
                          ("cli", cfg["bin"]["cli_env"]),
                          ("downloader", cfg["bin"]["downloader_env"]))
    }
    for name, path in binaries.items():
        if not path or not Path(path).is_file():
            return False, f"{name} 二进制未就绪: {path or '<未设置>'}"

    nodes_cfg = {
        "bind": cfg["bind"],
        "nodes": cfg["nodes"],
        "sample_path": str(fixture.path),
        "keywords": list(fixture.keywords),
        "namespace": cfg["protocol"]["namespace"],
        "workdir": str(harness.workdir),
        "golden_req": str(harness.golden_dir / "wakeup_req.v1.json"),
        "golden_ack": str(harness.golden_dir / "wakeup_ack.v1.json"),
        "downloader": binaries["downloader"],
        "challenge_window_sec": cfg["protocol"]["challenge_window_sec"],
    }
    config_path = harness.workdir / "t0-nodes.json"
    config_path.write_text(json.dumps(nodes_cfg, ensure_ascii=False, indent=2) + "\n",
                           encoding="utf-8")
    os.environ["COLDREVIVE_T0_CONFIG"] = str(config_path)

    for node, spec in cfg["nodes"].items():
        argv = harness.render_argv(
            cfg["launch"]["daemon_argv"], daemon=binaries["daemon"],
            config=str(config_path), node=node, bind=cfg["bind"],
            http_port=str(spec["http_port"]), wake_udp_port=str(spec["wake_udp_port"]),
            bt_port=str(spec["bt_port"]),
        )
        harness.spawn(f"daemon-{node}", argv)
    for node, spec in cfg["nodes"].items():
        url = f"http://{cfg['bind']}:{spec['http_port']}/health"
        if not harness.wait_health(url, float(cfg["timeouts_sec"]["startup"])):
            return False, f"节点 {node} 健康检查未通过: {url}"
    return True, "A/B 双节点已启动且健康检查通过"


def print_list(cases: list[dict[str, Any]]) -> None:
    print(f"T0 用例清单（门禁 G1，共 {len(cases)} 条）")
    print(f"{'ID':<8}{'组':<12}{'必需':<6}{'流程':<16}标题")
    print("-" * 100)
    for c in cases:
        print(f"{c['id']:<8}{c['group']:<12}{'是' if c['mandatory'] else '否':<6}"
              f"{c['flow']:<16}{c['title']}")
        print(f"{'':<8}判据出处: {c['source']}")


def build_report(mode: str, adapter: Any, cfg: dict[str, Any], fixture: Any,
                 results: list[dict[str, Any]], preflight: dict[str, Any] | None) -> dict[str, Any]:
    summary = {
        "total": len(results),
        "passed": sum(r["status"] == "PASS" for r in results),
        "failed": sum(r["status"] == "FAIL" for r in results),
        "blocked": sum(r["status"] == "BLOCKED" for r in results),
        "skipped": sum(r["status"] == "SKIP" for r in results),
        "mandatory_failed": [
            r["id"] for r in results if r["mandatory"] and r["status"] != "PASS"
        ],
    }
    wired = bool(getattr(adapter, "wired", False))
    evidence_valid = (
        mode == "real" and wired and summary["failed"] == 0
        and summary["blocked"] == 0 and not summary["mandatory_failed"]
    )
    if mode == "stub":
        reason = "STUB 合成观测：仅验证编排与断言引擎，不可作为 G1 门禁证据"
    elif not wired:
        reason = "本次运行的真实二进制或双节点链路未就绪（M1 代码层已接线，环境未就绪）"
    elif summary["failed"] or summary["blocked"]:
        reason = "存在 FAIL / BLOCKED 用例"
    else:
        reason = "真实链路全部通过"

    next_actions: list[str] = []
    if mode == "stub":
        next_actions = [
            "G0 条件冻结已生效，M1 二进制与 CliAdapter 代码层已接线；下一步为真实链路复跑",
            "设置三个二进制环境变量后以 --mode real 重新执行（stub 结果永远不构成证据）",
        ]
    elif not wired:
        next_actions = []
        if preflight:
            missing = preflight.get("missing_observers", [])
            size = preflight.get("observe_plan_size", 0)
            if missing:
                next_actions.append(f"待实现观测方法 {len(missing)} 个: {', '.join(missing[:3])} …")
            else:
                next_actions.append(f"观测采集层已实现 {size}/20 条映射（t0_cli.OBSERVE_PLAN）")
            unset = [k for k, v in preflight.get("binaries", {}).items() if not v]
            if unset:
                next_actions.append("未设置环境变量: " + ", ".join(unset))
            next_actions.append("按 t0_config.json 的 launch 契约启动 A / B 两个守护进程，接入第三方 BT 客户端")
            next_actions.append("设置 COLDREVIVE_DAEMON / COLDREVIVE_CLI / T0_DOWNLOADER 三个真实文件路径后重跑 --mode real")
    else:
        failed = [r["id"] for r in results if r["status"] != "PASS"]
        if failed:
            next_actions.append("修复未通过用例: " + ", ".join(failed))

    return {
        "suite": "T0",
        "schema": "coldrevive.t0.report/v1",
        "gate": "G1",
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "mode": mode,
        "adapter": adapter.name,
        "wired": wired,
        "evidence_valid": evidence_valid,
        "reason": reason,
        "thresholds": cfg["thresholds"],
        "fixture": fixture.as_dict(),
        "summary": summary,
        "cases": results,
        "preflight": preflight,
        "next_actions": next_actions,
    }


def main() -> int:
    ap = argparse.ArgumentParser(description="ColdRevive T0 双节点验收")
    ap.add_argument("--mode", choices=("stub", "real"), default="stub",
                    help="stub=框架自检（默认）；real=真实链路，需完成接线")
    ap.add_argument("--self-test", action="store_true", help="等价于 --mode stub（显式写法）")
    ap.add_argument("--list", action="store_true", help="只列出用例与判据出处")
    ap.add_argument("--config", type=Path, default=None)
    ap.add_argument("--cases", type=Path, default=None)
    ap.add_argument("--report", type=Path, default=None, help="自定义报告输出路径")
    ap.add_argument("--no-cleanup", action="store_true", help="保留 .run/ 便于排错")
    args = ap.parse_args()

    try:
        cfg = load_config(args.config)
        case_data = load_cases(args.cases)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"[ERROR] 配置载入失败: {exc}", file=sys.stderr)
        return 2

    cases = case_data["cases"]
    if args.list:
        print_list(cases)
        return 0

    mode = "real" if args.mode == "real" else "stub"
    harness = Harness(cfg, mode=mode, keep=args.no_cleanup)
    adapter = make_adapter(mode, harness)
    results: list[dict[str, Any]] = []
    fixture = None
    ctx: dict[str, Any] = {}

    try:
        fixture = harness.prepare()
        ctx = harness.context()

        if mode == "real":
            ready, why = setup_real_env(cfg, harness, fixture)
            adapter.wired = ready
            if not ready:
                print(f"[BLOCKED] {why}", file=sys.stderr)
                print("          见 tests/e2e/t0/README.md §5 接线清单；本次结果不计入 G1。",
                      file=sys.stderr)

        for case in cases:
            cid, title = case["id"], case["title"]
            entry: dict[str, Any] = {
                "id": cid, "title": title, "group": case["group"],
                "mandatory": case["mandatory"], "flow": case["flow"],
                "source": case["source"],
            }
            if cid not in ASSERTS and cid != "T0-00":
                entry.update(status="SKIP", assertions=[], note="断言表未定义该用例")
                results.append(entry)
                print(f"[SKIP] {cid} {title}（断言表未定义）")
                continue

            if cid == "T0-00":
                assertions = framework_check(ctx)
                entry.update(status="PASS" if all(a["ok"] for a in assertions) else "FAIL",
                             assertions=assertions, note="框架自检：注入坏观测验证断言引擎")
            else:
                try:
                    obs = adapter.observe(cid, ctx)
                    assertions = run_assertions(cid, obs, ctx)
                    entry.update(
                        status="PASS" if all(a["ok"] for a in assertions) else "FAIL",
                        assertions=assertions, note=obs.note,
                        observation_source=obs.source,
                    )
                except NotWiredError as exc:
                    entry.update(status="BLOCKED", assertions=[], note=str(exc))
                except Exception as exc:  # 观测异常不能伪装成通过
                    entry.update(status="FAIL", assertions=[],
                                 note=f"观测异常: {type(exc).__name__}: {exc}")

            results.append(entry)
            mark = entry["status"]
            extra = f" — {entry['note']}" if mark == "BLOCKED" else ""
            print(f"[{mark:<7}] {cid} {title}{extra}")

        preflight = adapter.preflight(cfg) if mode == "real" and hasattr(adapter, "preflight") else None
        report = build_report(mode, adapter, cfg, fixture, results, preflight)

        out = Path(args.report) if args.report else (
            harness.report_dir / f"t0-{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}-{mode}.json"
        )
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

        s = report["summary"]
        print("\n" + "=" * 78)
        print(f"T0 {mode.upper()} | 共 {s['total']} 条：PASS {s['passed']} / "
              f"FAIL {s['failed']} / BLOCKED {s['blocked']} / SKIP {s['skipped']}")
        print(f"门禁证据有效: {report['evidence_valid']} — {report['reason']}")
        print(f"报告: {out.relative_to(ROOT) if out.is_relative_to(ROOT) else out}")
        if mode == "stub":
            print("[DEMO] STUB 合成观测，仅证明脚本本身正确；不可作为 G1 门禁证据。")
        for action in report["next_actions"]:
            print("  → " + action)
        print("=" * 78)

        if mode == "real" and not getattr(adapter, "wired", False):
            return 2
        if s["failed"] or s["mandatory_failed"]:
            return 1
        return 0

    except (OSError, ValueError, RuntimeError) as exc:
        print(f"[ERROR] 执行失败: {type(exc).__name__}: {exc}", file=sys.stderr)
        return 2
    finally:
        harness.cleanup()


if __name__ == "__main__":
    sys.exit(main())
