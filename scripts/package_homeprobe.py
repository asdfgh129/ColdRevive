#!/usr/bin/env python3
"""打包 macOS Intel 家宽取证 kit 为一个自包含 zip。

用法：
  python3 scripts/package_homeprobe.py
输出：
  dist/coldrevive-homeprobe-macos-intel-v<KIT_VERSION>.zip
（例如 v1.4；文件名带版本，避免新旧副本混淆。）
zip 内含 MANIFEST.json（逐文件 sha256 与 kit 版本），并要求 .sh / .py 保留可执行位。
"""

from __future__ import annotations

import hashlib
import json
import os
import stat
import sys
import zipfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "tests" / "e2e" / "homeprobe-macos"
KIT_VERSION = "1.4"
BASE = "coldrevive-homeprobe-macos-intel-v" + KIT_VERSION
OUT = ROOT / "dist" / (BASE + ".zip")
SKIP_DIRS = {"results", "reports", "__pycache__"}
SKIP_SUFFIXES = (".pyc", ".pyo")


def collect() -> List[Path]:
    files: List[Path] = []
    for path in sorted(SOURCE.rglob("*")):
        if not path.is_file():
            continue
        if any(part in SKIP_DIRS for part in path.relative_to(SOURCE).parts):
            continue
        if path.name.endswith(SKIP_SUFFIXES) or path.name == ".DS_Store":
            continue
        files.append(path)
    return files


def main() -> int:
    files = collect()
    if not files:
        print("error: no files found under %s" % SOURCE, file=sys.stderr)
        return 1

    manifest: Dict = {
        "schema": "coldrevive.homeprobe.kit-manifest/v1",
        "kit_version": KIT_VERSION,
        "built_at": datetime.now(timezone.utc).isoformat(),
        "target": "macOS Ventura 13.x / x86_64 (Intel), Python 3.8+",
        "base_dir": BASE,
        "files": [],
    }
    for path in files:
        relative = path.relative_to(SOURCE).as_posix()
        manifest["files"].append({
            "path": relative,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "bytes": path.stat().st_size,
        })

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(OUT, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
        for path in files:
            relative = path.relative_to(SOURCE).as_posix()
            mode = 0o755 if path.suffix in (".sh", ".py") else 0o644
            info = zipfile.ZipInfo("%s/%s" % (BASE, relative), date_time=datetime.now().timetuple()[:6])
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (stat.S_IFREG | mode) << 16
            zf.writestr(info, path.read_bytes())
        manifest_bytes = (json.dumps(manifest, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
        info = zipfile.ZipInfo("%s/MANIFEST.json" % BASE, date_time=datetime.now().timetuple()[:6])
        info.compress_type = zipfile.ZIP_DEFLATED
        info.external_attr = (stat.S_IFREG | 0o644) << 16
        zf.writestr(info, manifest_bytes)

    with zipfile.ZipFile(OUT) as zf:
        bad = zf.testzip()
        names = zf.namelist()
    if bad:
        print("error: corrupt zip member %s" % bad, file=sys.stderr)
        return 1
    print("package: %s" % OUT)
    print("  files: %d (+MANIFEST.json), bytes=%d" % (len(files), OUT.stat().st_size))
    print("  sha256: %s" % hashlib.sha256(OUT.read_bytes()).hexdigest())
    print("  contains: %s" % ", ".join(n.split("/", 1)[1] for n in names if n.endswith(("README.md", "run_all.sh"))))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
