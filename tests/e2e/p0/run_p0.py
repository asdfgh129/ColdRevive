#!/usr/bin/env python3
"""P0 e2e：FR-01/02/04/05/06/07/09/10/12/13/15 的运行期证据（本机回环，双进程）。

只驱动真实守护进程的 HTTP `/cli` 与 `/health`，以及真实 BT 握手/真实文件读写。
所有断言读的都是运行期 JSON、真实磁盘内容或守护进程日志；脚本不写任何
「预期值」到被测代码里，也不改门禁常量。

用法（仓库根目录）：
    python3 tests/e2e/p0/run_p0.py

阶段划分（配置不同 → 必须两段，否则限流会打乱功能检查）：
  A 段（限流关闭）：FR-01 目录增量 + FR-02 多哈希 + FR-05/06 周期重发布
                   + FR-10 多任务做种容量/逐出
  B 段（限流开启，同一 workdir 重启）：FR-04 索引重启存活（loaded_v2）
                   + FR-09 控制面按 IP 限流（429 语义，/health 不被限）
  C–G 段：FR-15 HOT/RSS/CPU 阈值、FR-07 分词/同义词/语言识别与 publish ACK。
"""
from __future__ import annotations

import hashlib
import json
import os
import shutil
import socket
import struct
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
BASE = ROOT / "tests" / "e2e" / "p0"
RUN = BASE / "run"
REPORTS = BASE / "reports"
BIN = ROOT / "target" / "release" / ("coldrevived.exe" if os.name == "nt" else "coldrevived")

# 独立实现（纯 Python RFC 1320 MD4）：eD2k fileID 必须由它复算一致，
# 不能只把守护进程自己算的数读回来——那样两边同时算错也「通过」。
sys.path.insert(0, str(ROOT / "tests" / "e2e" / "w24"))
from verify_ed2k import ed2k as ed2k_fileid  # noqa: E402  (路径注入后才能导入)

SHARE = RUN / "share"
WORK = RUN / "work"
SAMPLE_A = SHARE / "sample_primary.bin"
SAMPLE_B = SHARE / "sample_second.bin"
DROPPED = SHARE / "dropped_late.bin"

checks: list[tuple[str, bool, str]] = []


def check(name: str, ok: bool, detail) -> None:
    checks.append((name, bool(ok), str(detail)))
    print(f"[{'OK' if ok else 'XX'}] {name}: {detail}", flush=True)


def hx(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()


def make_bytes(seed: str, size: int) -> bytes:
    """可复现的非重复内容（避免全零样本掩盖偏移错误）。"""
    out = bytearray()
    i = 0
    while len(out) < size:
        out += hashlib.sha256(f"{seed}:{i}".encode()).digest()
        i += 1
    return bytes(out[:size])


def free_udp_port() -> int:
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]
    finally:
        s.close()


def free_tcp_port() -> int:
    s = socket.socket()
    try:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]
    finally:
        s.close()


def http(url: str, payload=None, timeout: float = 10.0):
    """返回 (status, json)。4xx/5xx 不抛异常——限流断言需要读 429 的响应体。"""
    data = None if payload is None else json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        url, data=data,
        headers={"Content-Type": "application/json"} if data else {},
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, json.loads(r.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        raw = e.read().decode("utf-8", "replace")
        try:
            return e.code, json.loads(raw)
        except json.JSONDecodeError:
            return e.code, {"raw": raw}


def cli(port: int, command: str, *args: str, timeout: float = 30.0):
    return http(f"http://127.0.0.1:{port}/cli",
                {"command": command, "args": [str(a) for a in args]}, timeout)


def health(port: int):
    st, j = http(f"http://127.0.0.1:{port}/health", None, 5.0)
    if st != 200:
        raise RuntimeError(f"/health 返回 {st}")
    return j


def wait_health(port: int, timeout: float = 20.0):
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        try:
            return health(port)
        except Exception as e:  # noqa: BLE001 启动期连接失败是预期的
            last = e
            time.sleep(0.2)
    raise RuntimeError(f"守护进程 {port} 端口健康检查超时: {last}")


def wait_until(fn, timeout: float, period: float = 0.25):
    """轮询到 fn() 为真；返回 (ok, 最后一次的值)。"""
    deadline = time.time() + timeout
    val = None
    while time.time() < deadline:
        val = fn()
        if val:
            return True, val
        time.sleep(period)
    return False, val


def bt_wake(bt_port: int, infohash_hex: str, timeout: float = 10.0) -> bool:
    """真实 BT 握手：拿到的 68 字节应答证明对端接受握手；唤醒是它的副作用。"""
    ih = bytes.fromhex(infohash_hex)
    assert len(ih) == 20, f"infohash 长度错误 {infohash_hex}"
    s = socket.create_connection(("127.0.0.1", bt_port), timeout=timeout)
    try:
        peer_id = b"-CRP0001-" + hashlib.sha256(infohash_hex.encode()).hexdigest()[:12].encode()
        s.sendall(b"\x13BitTorrent protocol" + b"\x00" * 8 + ih + peer_id)
        buf = bytearray()
        s.settimeout(timeout)
        while len(buf) < 68:
            try:
                c = s.recv(4096)
            except OSError:
                # 被 guard 拒绝时对端会直接关连接（RST）：这是「没放行」，
                # 不是「握手成功」，如实返回 False。
                return False
            if not c:
                return False
            buf.extend(c)
        return bytes(buf[28:48]) == ih
    finally:
        try:
            s.close()
        except OSError:
            pass


def write_config(path: Path, node_ports: dict, extra: dict) -> Path:
    cfg = {
        "role": "holder",
        "bind": "127.0.0.1",
        "nodes": {
            "A": {"host": "127.0.0.1", **node_ports["A"]},
            "B": {"host": "127.0.0.1", **node_ports["B"]},
        },
        "keywords": ["p0", "indexer", "watch"],
        "sample_path": str(SAMPLE_A.relative_to(ROOT)),
        "samples": [str(SAMPLE_B.relative_to(ROOT))],
        "workdir": str(WORK.relative_to(ROOT)),
        "golden_req": str((RUN / "golden" / "req.json").relative_to(ROOT)),
        "golden_ack": str((RUN / "golden" / "ack.json").relative_to(ROOT)),
        "downloader": str((RUN / "downloads").relative_to(ROOT)),
        "engine": {"guard_handshake": True, "max_active_seeds": "1"},
    }
    cfg.update(extra)
    path.write_text(json.dumps(cfg, ensure_ascii=False, indent=2), encoding="utf-8")
    return path


def spawn(name: str, cfg: Path, node_id: str) -> subprocess.Popen:
    log = (RUN / f"node-{name}.log").open("w", encoding="utf-8")
    proc = subprocess.Popen(
        [str(BIN), "--config", str(cfg), "--node-id", node_id],
        cwd=str(ROOT), stdout=log, stderr=subprocess.STDOUT,
    )
    proc._cr_log = log  # type: ignore[attr-defined]  # 保活文件句柄，别被 GC 关掉
    return proc


def stop(proc: subprocess.Popen | None) -> None:
    if proc is None:
        return
    try:
        proc.terminate()
        proc.wait(timeout=10)
    except Exception:  # noqa: BLE001
        try:
            proc.kill()
        except Exception:  # noqa: BLE001
            pass
    log = getattr(proc, "_cr_log", None)
    if log is not None:
        try:
            log.close()
        except Exception:  # noqa: BLE001
            pass


def entries(port: int) -> dict:
    st, j = cli(port, "inspect", "entries")
    if st != 200 or "entries" not in j:
        raise RuntimeError(f"inspect entries 失败: {st} {j}")
    return j


def by_name(port: int, name: str):
    for e in entries(port)["entries"]:
        if e["name"] == name:
            return e
    return None


# --------------------------------------------------------------------------- 准备
def prepare() -> dict:
    if not BIN.is_file():
        sys.exit(f"缺少 release 二进制: {BIN}（先 cargo build --release -p coldrevived）")
    if RUN.exists():
        shutil.rmtree(RUN)
    for d in (SHARE, WORK, RUN / "golden", RUN / "downloads", REPORTS):
        d.mkdir(parents=True, exist_ok=True)
    # 两个样本都小于 PARTSIZE：本脚本验证的是运行期行为，跨分片已有 W25 覆盖。
    a = make_bytes("p0-primary", 256 * 1024)
    b = make_bytes("p0-second", 192 * 1024)
    SAMPLE_A.write_bytes(a)
    SAMPLE_B.write_bytes(b)
    (RUN / "golden" / "req.json").write_text('{"v":1,"type":"WAKEUP_REQ"}', encoding="utf-8")
    (RUN / "golden" / "ack.json").write_text('{"v":1,"type":"WAKEUP_ACK"}', encoding="utf-8")

    ports = {
        "A": {"http_port": free_tcp_port(), "wake_udp_port": free_udp_port(),
              "bt_port": free_tcp_port()},
        "B": {"http_port": free_tcp_port(), "wake_udp_port": free_udp_port(),
              "bt_port": free_tcp_port()},
    }
    return {"ports": ports, "sample_a": a, "sample_b": b}


def save(name: str, obj) -> None:
    (REPORTS / name).write_text(json.dumps(obj, ensure_ascii=False, indent=2), encoding="utf-8")


# --------------------------------------------------------------------------- A 段
def phase_a(prep: dict) -> dict:
    ports = prep["ports"]
    a, b = ports["A"], ports["B"]
    cfg_a = write_config(RUN / "config-a.json", ports, {
        "indexer": {"watch_secs": 2},
        # ttl 取 9s：刷新窗口在 ttl/3=3s 处打开，3s 的检查周期能在 20s 内命中
        "kad": {"republish_secs": 3, "ttl_secs": 9},
    })
    cfg_b = write_config(RUN / "config-b.json", ports, {"role": "member"})
    # B 是「发现方」：只负责接 KAD_PUBLISH 并回 ACK，不做种、不限流。
    cfg_b_json = json.loads(cfg_b.read_text(encoding="utf-8"))
    cfg_b_json.pop("indexer", None)
    cfg_b_json.pop("kad", None)
    cfg_b_json["engine"] = {}
    cfg_b.write_text(json.dumps(cfg_b_json, ensure_ascii=False, indent=2), encoding="utf-8")

    proc_a = proc_b = None
    out: dict = {}
    try:
        proc_b = spawn("B", cfg_b, "B")
        wait_health(b["http_port"])
        proc_a = spawn("A", cfg_a, "A")
        h0 = wait_health(a["http_port"])
        save("health_a_initial.json", h0)

        # ---- FR-04 之前置：启动即入库的多样本条目
        e0 = entries(a["http_port"])
        check("startup_indexes_both_samples",
              e0["open_status"] == "fresh" and e0["schema_version"] == 2
              and len(e0["entries"]) == 2,
              f"open_status={e0['open_status']} schema={e0['schema_version']} "
              f"entries={len(e0['entries'])}")
        names = sorted(x["name"] for x in e0["entries"])
        check("startup_entries_are_the_two_samples",
              names == ["sample_primary.bin", "sample_second.bin"], names)
        check("startup_entries_carry_multi_hash",
              all(len(x["sha1"]) == 40 and len(x["ed2k_hash"]) == 32 for x in e0["entries"]),
              [(x["name"], len(x["sha1"]), len(x["ed2k_hash"])) for x in e0["entries"]])
        check("startup_entries_carry_infohash",
              all(len(x["infohash"]) == 40 for x in e0["entries"]),
              [len(x["infohash"]) for x in e0["entries"]])
        check("seeding_cap_from_config", h0["seeding"]["cap"] == 1,
              h0["seeding"])
        check("starts_cold", h0["seed_active"] == 0 and h0["seed_tasks"] == 2,
              f"seed_active={h0['seed_active']} seed_tasks={h0['seed_tasks']}")

        # ---- FR-02：多哈希来自同一次读盘，且 ed2k 链接优先用索引
        st, idx = cli(a["http_port"], "index", str(DROPPED))
        check("index_missing_file_is_reported", st == 200 and idx.get("indexed") is False
              and bool(idx.get("error")),
              f"status={st} indexed={idx.get('indexed')} error={idx.get('error')}")

        dropped = make_bytes("p0-dropped", 96 * 1024)
        DROPPED.write_bytes(dropped)
        want_sha1 = hashlib.sha1(dropped).hexdigest()

        # ---- FR-01：目录增量（不重启、不手工 index）
        ok, val = wait_until(
            lambda: by_name(a["http_port"], DROPPED.name), 3 * 2 + 6)
        e_drop = val
        check("watcher_indexed_dropped_file", ok and e_drop is not None,
              "未在 3 个周期内看到新文件" if not ok else f"entries={len(entries(a['http_port'])['entries'])}")
        if e_drop:
            check("watcher_used_real_digests",
                  e_drop["sha1"] == want_sha1 and len(e_drop["ed2k_hash"]) == 32,
                  f"sha1={e_drop['sha1'][:12]}… expect={want_sha1[:12]}…")
            check("watcher_entry_size_matches", e_drop["size"] == len(dropped),
                  f"{e_drop['size']} vs {len(dropped)}")
            # FR-02：显式 index 同一文件 → already_known，哈希必须与监听看到的一致
            st, idx2 = cli(a["http_port"], "index", str(DROPPED))
            check("explicit_index_matches_watcher",
                  st == 200 and idx2.get("indexed") is True and idx2.get("already_known") is True
                  and idx2.get("sha1") == want_sha1 and idx2.get("ed2k_hash") == e_drop["ed2k_hash"],
                  f"already_known={idx2.get('already_known')} sha1_ok={idx2.get('sha1') == want_sha1}")

            # ---- FR-12：ed2k 链接走索引快路径，不再读盘
            st, link = cli(a["http_port"], "ed2k", e_drop["file_hash"])
            links = link.get("links") or []
            first = links[0] if links else {}
            check("ed2k_link_from_index",
                  st == 200 and first.get("link_source") == "index"
                  and link.get("backfilled_from_disk") == 0,
                  f"link_source={first.get('link_source')} "
                  f"backfilled={link.get('backfilled_from_disk')}")
            # 独立复算：fileID 必须等于纯 Python MD4 的结果，且与链接里的段一致
            want_id = ed2k_fileid(str(DROPPED))
            segments = (first.get("ed2k_link") or "").split("|")
            in_link = segments[4].lower() if len(segments) > 4 else ""
            check("ed2k_fileid_independently_recomputed",
                  want_id == e_drop["ed2k_hash"].lower() == in_link,
                  f"md4={want_id} index={e_drop['ed2k_hash']} link={in_link}")
            check("ed2k_link_shape_matches_file",
                  len(segments) >= 5 and segments[0] == "ed2k://" and segments[1] == "file"
                  and segments[2] == DROPPED.name and segments[3] == str(len(dropped)),
                  "|".join(segments[:5]))

        # ---- FR-01 出库：文件删掉后索引条目消失
        DROPPED.unlink()
        ok, val = wait_until(
            lambda: by_name(a["http_port"], DROPPED.name) is None, 3 * 2 + 6)
        check("watcher_removed_deleted_file", ok,
              f"剩余条目={[x['name'] for x in entries(a['http_port'])['entries']]}")

        # ---- FR-05/06：周期重发布（真实往 B 发 KAD_PUBLISH 并收到 ACK）
        ok, val = wait_until(
            lambda: (lambda h: h["kad_publish"].get("cycles", 0) >= 1
                     and (h["kad_publish"].get("last") or {}).get("sent") is True)(
                         health(a["http_port"])), 4 * 3 + 8)
        h_rep = health(a["http_port"])
        save("health_a_republish.json", h_rep)
        kp = h_rep["kad_publish"]
        check("kad_republish_cycles_grew", kp.get("cycles", 0) >= 1, kp.get("cycles"))
        check("kad_republish_frame_acked",
              (kp.get("last") or {}).get("sent") is True and (kp.get("last") or {}).get("keys", 0) > 0,
              kp.get("last"))
        # FR-05：刷新是「到期才发」，不是「每周期无脑发」；上报的 ttl 必须是配置值。
        last_rep = kp.get("last") or {}
        check("kad_republish_is_ttl_driven",
              last_rep.get("ttl_secs") == 9 and last_rep.get("due", 0) >= 1
              and last_rep.get("refresh_failed", 0) == 0,
              {k: last_rep.get(k) for k in ("ttl_secs", "due", "refresh_failed")})
        st_h, kad_h = cli(a["http_port"], "kad-health")
        check("kad_health_readable",
              st_h == 200 and kad_h.get("ttl_secs") == 9
              and kad_h.get("published_entries", 0) >= 2
              and kad_h.get("keys_unique", 0) <= kad_h.get("keys_total", 0),
              {k: kad_h.get(k) for k in ("ttl_secs", "published_entries",
                                         "keys_total", "keys_unique")})

        # ---- FR-10：多任务做种，容量 1 下真实逐出
        e_a = next(x for x in entries(a["http_port"])["entries"] if x["name"] == "sample_primary.bin")
        e_b = next(x for x in entries(a["http_port"])["entries"] if x["name"] == "sample_second.bin")
        woke1 = bt_wake(a["bt_port"], e_a["infohash"])
        ok, _ = wait_until(lambda: health(a["http_port"])["seed_active"] == 1, 10)
        h1 = health(a["http_port"])
        save("health_a_after_wake1.json", h1)
        check("first_sample_wakes_hot", woke1 and ok,
              f"handshake={woke1} seed_active={h1['seed_active']} seeding={h1['seeding']}")
        check("wake_count_never_exceeds_cap", h1["seed_active"] <= h1["seeding"]["cap"],
              f"active={h1['seed_active']} cap={h1['seeding']['cap']}")

        woke2 = bt_wake(a["bt_port"], e_b["infohash"])
        ok, _ = wait_until(
            lambda: health(a["http_port"])["seeding"]["evictions"] >= 1, 12)
        h2 = health(a["http_port"])
        save("health_a_after_wake2.json", h2)
        check("second_infohash_wakes_its_own_task", woke2,
              f"handshake={woke2}")
        check("lru_evicted_under_cap", h2["seeding"]["evictions"] >= 1,
              h2["seeding"])
        check("seed_active_still_within_cap",
              h2["seed_active"] <= h2["seeding"]["cap"] == 1,
              f"active={h2['seed_active']} cap={h2['seeding']['cap']}")
        check("scheduler_tracks_single_slot", h2["seeding"]["scheduled"] <= 1,
              h2["seeding"])
        out["entries_after_a"] = entries(a["http_port"])
        out["health_after_a"] = h2
    finally:
        stop(proc_a)
        stop(proc_b)
    return out


# --------------------------------------------------------------------------- B 段
def phase_b(prep: dict) -> dict:
    ports = prep["ports"]
    a = ports["A"]
    want_rate, want_burst = 4, 4
    cfg = write_config(RUN / "config-rate.json", ports, {
        "api": {"rate_per_sec": want_rate, "rate_burst": want_burst},
    })
    proc = None
    out: dict = {}
    try:
        proc = spawn("A2", cfg, "A")
        h = wait_health(a["http_port"])
        save("health_a_restart.json", h)

        # ---- FR-04：重启后索引原样回来，且不是「fresh/recovered」伪装
        e = entries(a["http_port"])
        save("entries_after_restart.json", e)
        check("restart_loads_schema_v2",
              e["open_status"] == "loaded_v2" and e["schema_version"] == 2,
              f"open_status={e['open_status']} schema={e['schema_version']}")
        check("restart_keeps_sample_entries", len(e["entries"]) == 2,
              [x["name"] for x in e["entries"]])
        check("restart_keeps_deleted_file_out",
              all(x["name"] != DROPPED.name for x in e["entries"]),
              [x["name"] for x in e["entries"]])
        check("restart_keeps_multi_hash",
              all(len(x["sha1"]) == 40 and len(x["ed2k_hash"]) == 32 for x in e["entries"]),
              [(x["name"], x["ed2k_hash"][:8]) for x in e["entries"]])
        (REPORTS / "state_after_restart.sha256").write_text(
            hashlib.sha256((WORK / "state-A.json").read_bytes()).hexdigest() + "\n",
            encoding="utf-8")
        # ---- FR-09：突发超限 → 429 + reason/retry_after；/health 不受影响
        time.sleep(1.2)  # 让令牌桶回满，避免把上一段的余量读成「限流生效」
        burst = []
        health_storm = []
        for i in range(24):
            st, j = cli(a["http_port"], "inspect", "entries", timeout=5)
            burst.append((st, j.get("reason"), j.get("retry_after_ms"), j.get("limit_per_sec")))
            if i % 6 == 0:
                try:
                    health_storm.append(health(a["http_port"])["status"])
                except Exception as e:  # noqa: BLE001
                    health_storm.append(f"error:{e}")
        limited = [x for x in burst if x[0] == 429]
        ok200 = [x for x in burst if x[0] == 200]
        save("rate_burst.json", {"burst": burst, "health": health_storm})
        check("burst_gets_rate_limited", len(limited) > 0,
              f"limits={len(limited)}/24 statuses={[x[0] for x in burst]}")
        check("rate_limit_reason_is_explicit",
              all(x[1] == "rate_limited" for x in limited) and bool(limited),
              sorted({x[1] for x in limited}))
        check("rate_limit_reports_retry_after",
              all(isinstance(x[2], (int, float)) and x[2] > 0 for x in limited) and bool(limited),
              sorted({x[2] for x in limited})[:6])
        check("rate_limit_reports_configured_rate",
              all(x[3] == want_rate for x in limited) and bool(limited),
              sorted({x[3] for x in limited}))
        check("burst_lets_some_through", len(ok200) > 0, f"200={len(ok200)}")
        check("health_never_rate_limited",
              bool(health_storm) and all(s == "ok" for s in health_storm), health_storm)

        # 限流恢复：等 1 个周期后必须重新放行（不是永久封禁）
        time.sleep(max(1.0, 1.6 * want_burst / want_rate))
        st, _ = cli(a["http_port"], "inspect", "entries", timeout=5)
        check("rate_limit_recovers", st == 200, f"after_wait status={st}")
        out["rate"] = {"limited": len(limited), "ok": len(ok200)}
    finally:
        stop(proc)
    return out


# --------------------------------------------------------------------------- C 段
def phase_c(prep: dict) -> dict:
    """FR-15：阈值满了**拒绝新唤醒**（与 FR-10 的「逐出后继续服务」是两种语义）。"""
    ports = prep["ports"]
    a, b = ports["A"], ports["B"]
    cfg = write_config(RUN / "config-limit.json", ports, {
        "engine": {"guard_handshake": True, "max_active_seeds": "2"},
        "limits": {"max_hot_seeds": "1"},
    })
    cfg_b = RUN / "config-b.json"  # A 段已生成：role=member，不做种、不限流
    proc_a = proc_b = None
    out: dict = {}
    try:
        proc_b = spawn("B3", cfg_b, "B")
        wait_health(b["http_port"])
        proc_a = spawn("A3", cfg, "A")
        h0 = wait_health(a["http_port"])
        save("health_a_limits.json", h0)
        check("limit_reported_in_health",
              h0["limits"]["max_hot_seeds"] == 1 and h0["limits"]["hot"] == 0
              and h0["seeding"]["cap"] == 2,
              f"limits={h0['limits']} cap={h0['seeding']['cap']}")
        # 没配的阈值必须显式 enabled=false，而不是「字段缺失」或「假装是 0」
        check("unconfigured_thresholds_report_disabled",
              h0["limits"]["max_peers"]["enabled"] is False
              and h0["limits"]["max_upload_bytes_per_sec"]["enabled"] is False
              and h0["limits"]["max_rss_mb"]["enabled"] is False
              and h0["limits"]["max_seeding_cpu_millipct"]["enabled"] is False,
              {k: h0["limits"][k].get("enabled")
               for k in ("max_peers", "max_upload_bytes_per_sec", "max_rss_mb",
                         "max_seeding_cpu_millipct")})

        es = {x["name"]: x for x in entries(a["http_port"])["entries"]}
        prim, sec = es["sample_primary.bin"], es["sample_second.bin"]

        st, r1 = cli(b["http_port"], "wake", "raw", prim["file_hash"], prim["infohash"])
        ack1 = (r1 or {}).get("ack") or {}
        check("first_wake_admitted", st == 200 and ack1.get("status") == "OK", ack1.get("status"))
        h1 = health(a["http_port"])
        check("first_wake_became_hot", h1["limits"]["hot"] == 1 and h1["seed_active"] == 1,
              f"{h1['limits']} seed_active={h1['seed_active']}")

        # 同一个样本重复唤醒必须仍然放行（幂等），不能把「已在供片」记成超限
        st, r1b = cli(b["http_port"], "wake", "raw", prim["file_hash"], prim["infohash"])
        ack1b = (r1b or {}).get("ack") or {}
        check("repeat_wake_of_hot_sample_still_ok", ack1b.get("status") == "OK",
              ack1b.get("status"))

        st, r2 = cli(b["http_port"], "wake", "raw", sec["file_hash"], sec["infohash"])
        ack2 = (r2 or {}).get("ack") or {}
        check("second_wake_refused_over_threshold",
              ack2.get("status") == "REJECT_RESOURCE_LIMIT", ack2.get("status"))
        check("refusal_names_limit_and_numbers",
              ack2.get("limit_name") == "max_hot_seeds" and ack2.get("limit") == 1
              and ack2.get("current") == 1,
              {k: ack2.get(k) for k in ("limit_name", "limit", "current")})

        h2 = health(a["http_port"])
        save("health_a_after_refusal.json", h2)
        check("refusal_is_not_eviction",
              h2["seeding"]["evictions"] == 0 and h2["limits"]["hot"] == 1
              and h2["seed_active"] == 1,
              f"evictions={h2['seeding']['evictions']} hot={h2['limits']['hot']} "
              f"seed_active={h2['seed_active']}")

        # 裸 BT 握手（被动唤醒）必须受同一条阈值约束，否则绕过 bt_port 就能顶满
        bt_wake(a["bt_port"], sec["infohash"])
        h3 = health(a["http_port"])
        save("health_a_after_guard_refusal.json", h3)
        check("guard_handshake_honours_threshold",
              h3["limits"]["hot"] == 1 and h3["seed_active"] == 1
              and h3["guard"]["missed"] >= 1,
              f"hot={h3['limits']['hot']} missed={h3['guard']['missed']}")
        out["limits"] = {"ack_ok": ack1.get("status"), "ack_refused": ack2.get("status")}
    finally:
        stop(proc_a)
        stop(proc_b)
    return out


# --------------------------------------------------------------------------- D 段
def phase_d(prep: dict) -> dict:
    """FR-07：配置没给关键词时按文件名分词；以及 KAD 关键词的去重健康度。"""
    ports = prep["ports"]
    a = ports["A"]
    cfg = write_config(RUN / "config-token.json", ports, {
        "keywords": [],
        "kad": {"republish_secs": 3, "ttl_secs": 9},
    })
    # 两个名字共享 cold/revive 两个词：去重后才能看出「发了 6 个 key、只有 4 个不同」。
    f1 = SHARE / "Cold.Revive.Alpha.2024.mkv"
    f2 = SHARE / "Cold.Revive.Beta.x265.mkv"
    f1.write_bytes(make_bytes("p0-token-a", 8192))
    f2.write_bytes(make_bytes("p0-token-b", 8192))
    proc = None
    out: dict = {}
    try:
        proc = spawn("A4", cfg, "A")
        wait_health(a["http_port"])
        st1, r1 = cli(a["http_port"], "index", str(f1))
        kws = r1.get("keywords") or []
        check("tokenizer_used_when_config_has_no_keywords",
              st1 == 200 and r1.get("keywords_source") == "tokenized" and kws,
              f"source={r1.get('keywords_source')} keywords={kws}")
        check("tokenizer_drops_noise_and_digits",
              "mkv" not in kws and "2024" not in kws and "revive" in kws,
              kws)
        st2, r2 = cli(a["http_port"], "index", str(f2))
        check("second_file_indexed",
              st2 == 200 and r2.get("file_hash") and r2.get("file_hash") != r1.get("file_hash"),
              f"{r1.get('file_hash')} vs {r2.get('file_hash')}")

        st3, h = cli(a["http_port"], "kad-health")
        save("kad-health.json", h)
        out["kad_health"] = h
        check("kad_health_counts_tokens",
              h.get("tokenized_entries", 0) >= 2
              and h.get("keywords_unique", 0) < h.get("keywords_total", 0),
              {k: h.get(k) for k in ("tokenized_entries", "keywords_total", "keywords_unique")})
        # 两条条目共享 cold/revive，所以这两个 key 各被发两次 -> 去重后少 2 个
        check("kad_health_reports_duplicate_keys",
              h.get("duplicate_keys", 0) >= 2
              and h.get("keys_unique", 0) + h.get("duplicate_keys", 0) == h.get("keys_total", -1),
              {k: h.get(k) for k in ("keys_total", "keys_unique", "duplicate_keys")})
        check("kad_health_does_not_claim_public_search",
              "BLOCKED" in (h.get("public_search") or ""),
              h.get("public_search"))
    finally:
        stop(proc)
    return out


# --------------------------------------------------------------------------- E 段
def phase_e(prep: dict) -> dict:
    """FR-15：内存阈值**真实生效**（RSS 读得到 -> 超限即拒绝），不是只报字段。"""
    ports = prep["ports"]
    a, b = ports["A"], ports["B"]
    cfg = write_config(RUN / "config-rss.json", ports, {
        "engine": {"guard_handshake": True, "max_active_seeds": "2"},
        # 1 MiB：任何真实守护进程都超，所以这条阈值必然触发；它不是「大概会触发」。
        "limits": {"max_rss_mb": "1"},
    })
    cfg_b = RUN / "config-b.json"
    proc_a = proc_b = None
    out: dict = {}
    try:
        proc_b = spawn("B5", cfg_b, "B")
        wait_health(b["http_port"])
        proc_a = spawn("A5", cfg, "A")
        wait_health(a["http_port"])
        # 采样线程 1s 一次；等到真的读到 RSS 再断言，避免拿「还没采样」当 0。
        ok, _ = wait_until(
            lambda: (health(a["http_port"])["limits"]["max_rss_mb"].get("current") or 0) >= 1, 10)
        h0 = health(a["http_port"])
        save("health_a_rss_limit.json", h0)
        rss = h0["limits"]["max_rss_mb"]
        check("rss_threshold_reads_real_process_memory",
              ok and rss.get("enabled") is True and rss.get("limit") == 1
              and (rss.get("current") or 0) >= 1,
              rss)

        es = {x["name"]: x for x in entries(a["http_port"])["entries"]}
        prim = es["sample_primary.bin"]
        st, r = cli(b["http_port"], "wake", "raw", prim["file_hash"], prim["infohash"])
        ack = (r or {}).get("ack") or {}
        out["ack"] = ack
        check("rss_threshold_refuses_wake",
              st == 200 and ack.get("status") == "REJECT_RESOURCE_LIMIT",
              ack.get("status"))
        check("rss_refusal_names_limit_and_numbers",
              ack.get("limit_name") == "max_rss_mb" and ack.get("limit") == 1
              and (ack.get("current") or 0) >= 1 and ack.get("limit_unit") == "MiB",
              {k: ack.get(k) for k in ("limit_name", "limit", "current", "limit_unit")})
        h1 = health(a["http_port"])
        save("health_a_after_rss_refusal.json", h1)
        check("rss_refusal_leaves_no_hot_state",
              h1["limits"]["hot"] == 0 and h1["seed_active"] == 0,
              f"hot={h1['limits']['hot']} seed_active={h1['seed_active']}")
    finally:
        stop(proc_a)
        stop(proc_b)
    return out



# --------------------------------------------------------------------------- F 段
def phase_f(prep: dict) -> dict:
    """FR-15：造种 CPU 阈值读到真实进程 CPU 并拒绝新唤醒（1 毫百分点即可触发）。"""
    ports = prep["ports"]
    a, b = ports["A"], ports["B"]
    cfg = write_config(RUN / "config-cpu.json", ports, {
        "engine": {"guard_handshake": True, "max_active_seeds": "2"},
        # 1 milli-percent = 0.001% 单核：任何真实 CPU 采样都能越过，
        # 避免为了测阈值去人为烧 CPU（那会让被测进程的读数变成测试自己造的）。
        "limits": {"max_seeding_cpu_millipct": "1"},
    })
    cfg_b = RUN / "config-b.json"
    proc_a = proc_b = None
    out: dict = {}
    try:
        proc_b = spawn("B6", cfg_b, "B")
        wait_health(b["http_port"])
        proc_a = spawn("A6", cfg, "A")
        wait_health(a["http_port"])
        ok, _ = wait_until(
            lambda: (health(a["http_port"])["limits"]["max_seeding_cpu_millipct"]
                     .get("current") or 0) >= 1, 12)
        h0 = health(a["http_port"])
        save("health_a_cpu_limit.json", h0)
        cpu = h0["limits"]["max_seeding_cpu_millipct"]
        out["health"] = cpu
        check("cpu_threshold_reads_real_process_cpu",
              ok and cpu.get("enabled") is True and cpu.get("limit") == 1
              and (cpu.get("current") or 0) >= 1
              and (cpu.get("current_percent") or 0) > 0,
              cpu)
        check("cpu_threshold_reports_percent_and_millipct_consistently",
              abs((cpu.get("current_percent") or -1) - (cpu.get("current") or 0) / 1000.0)
              < 1e-6,
              {k: cpu.get(k) for k in ("current", "current_percent", "unit")})

        es = {x["name"]: x for x in entries(a["http_port"])["entries"]}
        prim = es["sample_primary.bin"]
        st, r = cli(b["http_port"], "wake", "raw", prim["file_hash"], prim["infohash"])
        ack = (r or {}).get("ack") or {}
        out["ack"] = ack
        check("cpu_threshold_refuses_wake",
              st == 200 and ack.get("status") == "REJECT_RESOURCE_LIMIT",
              ack.get("status"))
        check("cpu_refusal_names_limit_and_numbers",
              ack.get("limit_name") == "max_seeding_cpu_millipct"
              and ack.get("limit") == 1 and (ack.get("current") or 0) >= 1
              and ack.get("limit_unit") == "milli-percent",
              {k: ack.get(k) for k in ("limit_name", "limit", "current", "limit_unit")})
        h1 = health(a["http_port"])
        save("health_a_after_cpu_refusal.json", h1)
        check("cpu_refusal_leaves_no_hot_state",
              h1["limits"]["hot"] == 0 and h1["seed_active"] == 0,
              f"hot={h1['limits']['hot']} seed_active={h1['seed_active']}")
        log_f = (RUN / "node-A6.log").read_text(encoding="utf-8", errors="replace")
        log_f = log_f + (RUN / "node-B6.log").read_text(encoding="utf-8", errors="replace")
        check("log_shows_cpu_refusal", "max_seeding_cpu_millipct" in log_f,
              "node-A6.log / node-B6.log")
    finally:
        stop(proc_a)
        stop(proc_b)
    return out


# --------------------------------------------------------------------------- G 段
def phase_g(prep: dict) -> dict:
    """FR-07：确定性同义词 + Unicode 脚本语言识别，且扩展只进 KAD key、不污染原词。"""
    ports = prep["ports"]
    a, b = ports["A"], ports["B"]
    ns = "ColdRevive_P0_Syn:"
    cfg = write_config(RUN / "config-syn.json", ports, {
        "namespace": ns,
        "keywords": [],
        "kad": {"republish_secs": 3, "ttl_secs": 9},
    })
    f_en = SHARE / "Movie.Film.Collection.2024.mkv"
    f_zh = SHARE / "冷启动复活.电影.测试样本.bin"
    f_en.write_bytes(make_bytes("p0-syn-en", 8192))
    f_zh.write_bytes(make_bytes("p0-syn-zh", 8192))
    proc = proc_b = None
    out: dict = {}
    try:
        # publish 会把 KAD_PUBLISH 真发给对端并等 ACK，所以 B 必须在场。
        proc_b = spawn("B7", RUN / "config-b.json", "B")
        wait_health(b["http_port"])
        proc = spawn("A7", cfg, "A")
        wait_health(a["http_port"])
        st1, r1 = cli(a["http_port"], "index", str(f_en))
        exp1 = r1.get("keywords_expanded") or []
        check("latin_synonyms_are_deterministic",
              st1 == 200 and r1.get("language") == "en"
              and "film" in exp1 and "cinema" in exp1
              # movie/film 已经在原词里，同组只新增 cinema 一个 -> 至少 1 个。
              and (r1.get("synonym_added") or 0) >= 1,
              {k: r1.get(k) for k in ("language", "keywords", "keywords_expanded",
                                      "synonym_added")})
        check("synonyms_do_not_pollute_source_keywords",
              "cinema" not in (r1.get("keywords") or [])
              and r1.get("keywords_source") == "tokenized",
              r1.get("keywords"))

        st2, r2 = cli(a["http_port"], "index", str(f_zh))
        exp2 = r2.get("keywords_expanded") or []
        check("cjk_synonyms_use_cjk_table",
              st2 == 200 and r2.get("language") == "zh"
              and "影片" in exp2 and "片子" in exp2
              and "movie" not in exp2,
              {k: r2.get(k) for k in ("language", "keywords", "keywords_expanded")})
        check("cjk_run_is_not_split_per_character",
              "冷启动复活" in (r2.get("keywords") or [])
              and "测试样本" in (r2.get("keywords") or []),
              r2.get("keywords"))

        # 重新索引同一文件：原词、扩展词、发布 key 必须逐项相同（可复算）。
        st3, r3 = cli(a["http_port"], "index", str(f_en))
        check("reindex_is_byte_for_byte_deterministic",
              st3 == 200 and r3.get("keywords_expanded") == exp1,
              f"{r1.get('keywords_expanded')} vs {r3.get('keywords_expanded')}")
        stp, pub = cli(a["http_port"], "publish", str(f_en))
        out["publish"] = pub
        check("publish_requires_real_kad_publish_ack",
              stp == 200 and pub.get("published") is True
              and pub.get("ack_t") == "KAD_PUBLISH_ACK"
              and (pub.get("keys") or 0) > len(exp1),
              {k: pub.get(k) for k in ("published", "ack_t", "keys")})
        # inspect entries 只给每条的 key **数量**；逐 key 清单在 inspect kad。
        st5, ik = cli(a["http_port"], "inspect", "kad")
        cinema_key = hashlib.sha1((ns + "kw:cinema").encode("utf-8")).hexdigest()
        check("synonym_reaches_published_kad_keys",
              st5 == 200 and cinema_key in ((ik.get("kad") or {}).get("published_keys") or []),
              {"expected": cinema_key,
               "keys": (ik.get("kad") or {}).get("published_keys")})

        st4, h = cli(a["http_port"], "kad-health")
        save("kad-health-synonyms.json", h)
        out["kad_health"] = h
        check("kad_health_reports_languages_and_synonyms",
              st4 == 200 and (h.get("languages") or {}).get("zh", 0) >= 1
              and (h.get("synonym_added") or 0) >= 3
              and (h.get("published_entries") or 0) >= 1,
              {k: h.get(k) for k in ("languages", "synonym_added")})
        check("synonym_expansion_keeps_public_search_blocked",
              "BLOCKED" in (h.get("public_search") or ""),
              h.get("public_search"))
    finally:
        stop(proc)
        stop(proc_b)
    return out


def main() -> int:
    prep = prepare()
    log_total = time.time()
    a_out = phase_a(prep)
    b_out = phase_b(prep)
    c_out = phase_c(prep)
    d_out = phase_d(prep)
    e_out = phase_e(prep)
    f_out = phase_f(prep)
    g_out = phase_g(prep)

    # 日志证据：守护进程真的打印过入库/出库/重发布，而不只是状态字段变了
    log_a = (RUN / "node-A.log").read_text(encoding="utf-8", errors="replace")
    check("log_shows_watch_ingest", "FR-01 入库" in log_a, "node-A.log")
    check("log_shows_watch_removal", "FR-01 出库" in log_a, "node-A.log")
    check("log_shows_republish_cycle", "FR-05 重发布" in log_a, "node-A.log")
    check("log_shows_watch_started", "FR-01 目录监听已启动" in log_a, "node-A.log")
    log_c = (RUN / "node-A3.log").read_text(encoding="utf-8", errors="replace")
    check("log_shows_wake_refusal", "FR-15 拒绝唤醒" in log_c, "node-A3.log")
    check("log_shows_guard_refusal", "FR-15 拒绝握手唤醒" in log_c, "node-A3.log")
    log_e = (RUN / "node-A5.log").read_text(encoding="utf-8", errors="replace")
    check("log_shows_rss_refusal", "max_rss_mb" in log_e, "node-A5.log")

    passed = sum(1 for _, ok, _ in checks if ok)
    report = {
        "schema": "coldrevive.p0.e2e/v1",
        "scope": ("FR-01/02/04/05/06/07/09/10/12/13/15 运行期行为，本机回环双进程，"
                  "真实文件/HTTP/BT 握手/进程 RSS/进程 CPU/Unicode 脚本判定"),
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "elapsed_sec": round(time.time() - log_total, 1),
        "checks": [{"name": n, "ok": ok, "detail": d} for n, ok, d in checks],
        "passed": passed,
        "total": len(checks),
        "boundary": (
            "本机回环：A(holder)+B(member) 两个真实进程，样本 < PARTSIZE（跨分片见 W25）。"
            "FR-05/06 的 sent=true 仅表示「发出 KAD_PUBLISH 并收到对端 ACK」，"
            "**不等于公网可 KAD 检索**（FR-24 仍 BLOCKED，受 G0 protocol_scope 约束）。"
            "FR-09 只覆盖按来源 IP 的令牌桶，不含分布式/全局配额。FR-15 的带宽阈值靠 1s 采样差分，是滞后指标，只作拒绝新唤醒的理由。"
            "FR-01 后端是轮询快照，不是内核目录通知（Windows ReadDirectoryChangesW 未接入）。"
            "门禁常量不变：g2_claim=false、s6_status=not_run。"
        ),
        "phase_a": {"entries": a_out.get("entries_after_a"),
                    "health": a_out.get("health_after_a")},
        "phase_b": b_out,
        "phase_c": c_out,
        "phase_d": d_out,
        "phase_e": e_out,
        "phase_f": f_out,
        "phase_g": g_out,
    }
    save("report.json", report)
    print(f"\npassed={passed}/{len(checks)}  elapsed={report['elapsed_sec']}s")
    return 0 if passed == len(checks) else 1


if __name__ == "__main__":
    raise SystemExit(main())
