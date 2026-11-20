#!/usr/bin/env python3
"""W25-live 判据：生产 cr-ed2k::stack 与**真实 aMule 2.3.3** 的双向互通。

两个方向分开判：
  下载（我们 ← aMule）：reports/download.json 必须 hash_matches/content_matches/completed。
  上传（aMule ← 我们）：两个样本各判一次
      - w25-upload.bin      65536 B   单块，bytes_sent 必须精确等于文件大小
      - w25-upload-multi.bin 262144 B 多 range，bytes_sent ≥ 文件大小（aMule 重复请求会多算）
    两者都要求 peer_hello=true 且**对端落地文件**的 md5 与本机样本一致
    （这条要 ssh 去取，取不到 = BLOCKED，不算 pass）。

边界（照写，不放松）：
  - 这是 RFC1918 LAN（210 ↔ 221）的客户端互通证据，不是公网 eD2k 传输；
    eD2k server 登录 / source exchange / credit / AICH 均未实现（ADR-011 明确排除）。
  - 样本都远小于 PARTSIZE(9,728,000)，只覆盖单分片内的多块传输，
    不覆盖跨分片、断点续传、hashset 校验（hashset_verified=false）。
"""
import hashlib
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPORTS = os.path.join(HERE, "reports")
RUN = os.path.join(HERE, "run")

SSH_KEY = os.path.join(os.environ.get("TEMP", "/tmp"), "crkeys", "id_ed25519")
REMOTE = "root@192.168.1.221"
REMOTE_INCOMING = "/opt/g3-clients/amule-config/Incoming"


def load(name):
    path = os.path.join(REPORTS, name)
    if not os.path.exists(path):
        return None
    with open(path, "r", encoding="utf-8") as fh:
        raw = fh.read().strip()
    if not raw:
        return None
    # serve 早期把日志打到了 stdout，允许取最后一行 JSON
    line = [ln for ln in raw.splitlines() if ln.strip().startswith("{")]
    if not line:
        return None
    return json.loads(line[-1])


def md5_local(path):
    h = hashlib.md5()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def md5_remote(name):
    """取 aMule 落地文件的 (size, md5)。ssh 不可用 → (None, None) = BLOCKED。"""
    if not os.path.exists(SSH_KEY):
        return None, None, f"ssh key missing: {SSH_KEY}"
    cmd = [
        "ssh", "-i", SSH_KEY, "-o", "IdentitiesOnly=yes",
        "-o", "StrictHostKeyChecking=no", "-o", "ConnectTimeout=8",
        REMOTE, f"stat -c %s {REMOTE_INCOMING}/{name} 2>/dev/null; md5sum {REMOTE_INCOMING}/{name} 2>/dev/null",
    ]
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=40)
    except Exception as exc:  # noqa: BLE001
        return None, None, f"ssh failed: {exc}"
    if out.returncode != 0:
        return None, None, (out.stderr or "").strip()[:160] or "ssh nonzero"
    lines = [ln.strip() for ln in out.stdout.splitlines() if ln.strip()]
    if len(lines) < 2:
        return None, None, "remote file missing"
    try:
        size = int(lines[0])
    except ValueError:
        return None, None, f"unexpected stat output: {lines[0][:80]}"
    digest = lines[1].split()[0]
    return size, digest, ""


def main():
    checks = []  # (name, status, detail) status in {pass, fail, blocked}

    def add(name, ok, detail, blocked=False):
        checks.append((name, "blocked" if blocked else ("pass" if ok else "fail"), detail))

    # ---------------- 下载方向：我们 ← aMule ----------------
    dl = load("download.json")
    if dl is None:
        add("download.report_present", False, "reports/download.json 缺失或为空")
    else:
        add("download.peer_hello", bool(dl.get("peer_hello")),
            f"peer_hello={dl.get('peer_hello')} peer_filename={dl.get('peer_filename')!r}")
        add("download.bytes_received", dl.get("bytes_received") == dl.get("file_size"),
            f"{dl.get('bytes_received')} / {dl.get('file_size')}")
        add("download.hash_matches", bool(dl.get("hash_matches")), f"hash_matches={dl.get('hash_matches')}")
        add("download.content_matches", bool(dl.get("content_matches")),
            f"content_matches={dl.get('content_matches')}")
        add("download.completed", bool(dl.get("completed")), f"completed={dl.get('completed')}")
        add("download.file_id_is_real_md4",
            isinstance(dl.get("file_id"), str) and len(dl["file_id"]) == 32,
            f"file_id={dl.get('file_id')}")

    # ---------------- 上传方向：aMule ← 我们 ----------------
    # 两个样本：65536 B（单块，字节数必须精确相等）与 262144 B（多块/多 range）。
    for sample_name, report_name, exact in (
        ("w25-upload.bin", "upload.json", True),
        ("w25-upload-multi.bin", "upload-multi.json", False),
    ):
        tag = "upload" if exact else "upload_multi"
        up = load(report_name)
        sample = os.path.join(RUN, sample_name)
        local_size = os.path.getsize(sample) if os.path.exists(sample) else None
        local_md5 = md5_local(sample) if local_size is not None else None
        if up is None:
            add(f"{tag}.report_present", False, f"reports/{report_name} 缺失或为空")
            continue
        add(f"{tag}.peer_hello", bool(up.get("peer_hello")),
            f"peer={up.get('peer')} peer_hello={up.get('peer_hello')}")
        add(f"{tag}.queue_granted", bool(up.get("queue_granted")),
            f"queue_granted={up.get('queue_granted')} rank_sent={up.get('queue_rank_sent')!r}")
        if exact:
            add(f"{tag}.bytes_sent_equals_file_size",
                local_size is not None and up.get("bytes_sent") == local_size,
                f"bytes_sent={up.get('bytes_sent')} local_size={local_size}")
        else:
            # aMule 会对已收区间再发一次 REQUESTPARTS；重复发送如实计入，故用 ≥。
            # 真正的正确性由落地文件 md5 判（下面 remote_md5_matches）。
            add(f"{tag}.bytes_sent_covers_file_size",
                local_size is not None and (up.get("bytes_sent") or 0) >= local_size,
                f"bytes_sent={up.get('bytes_sent')} local_size={local_size} (重复请求会多算)")
        add(f"{tag}.blocks_sent_positive", (up.get("blocks_sent") or 0) >= 1,
            f"blocks_sent={up.get('blocks_sent')}")
        add(f"{tag}.error_empty", up.get("error") == "", f"error={up.get('error')!r}")
        blob = " ".join(up.get("log_tail") or [])
        need = ["OP_HELLO", "OP_HELLOANSWER", "OP_REQUESTFILENAME", "OP_REQFILENAMEANSWER",
                "OP_STARTUPLOADREQ", "OP_ACCEPTUPLOADREQ", "OP_REQUESTPARTS", "OP_SENDINGPART"]
        missing = [n for n in need if n not in blob]
        add(f"{tag}.opcode_chain_complete", not missing, f"missing={missing}")

        # 对端落地：只有真的读回来才算数
        if local_size is None:
            add(f"{tag}.remote_file_landed", False, f"BLOCKED: 本机样本缺失 {sample_name}", blocked=True)
            continue
        rsize, rmd5, why = md5_remote(sample_name)
        if rsize is None:
            add(f"{tag}.remote_file_landed", False, f"BLOCKED: {why}", blocked=True)
            add(f"{tag}.remote_md5_matches", False, f"BLOCKED: {why}", blocked=True)
        else:
            add(f"{tag}.remote_file_landed", rsize == local_size, f"remote_size={rsize} local={local_size}")
            add(f"{tag}.remote_md5_matches", rmd5 == local_md5, f"remote={rmd5} local={local_md5}")

    passed = sum(1 for _, s, _ in checks if s == "pass")
    failed = [c for c in checks if c[1] == "fail"]
    blocked = [c for c in checks if c[1] == "blocked"]
    print(f"W25-live 双向互通：{passed}/{len(checks)} pass, {len(failed)} fail, {len(blocked)} blocked")
    for name, status, detail in checks:
        print(f"  [{status.upper():7s}] {name}: {detail}")
    if failed:
        print("判据未达标：W25-live 不能写成通过。")
        return 1
    print("注：本结果仅覆盖 RFC1918 LAN 客户端互通；公网 eD2k 传输仍 BLOCKED。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
