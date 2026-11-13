#!/usr/bin/env python3
"""ColdRevive 文档站构建器（Docs as Code）

用法：
    python3 scripts/build_docs.py            # 生成配套 .html，并把全部文档内嵌进 index.html
    python3 scripts/build_docs.py --check    # 仅校验 front matter 与相对链接（CI 使用）

约定：
    - docs/**/*.md 是唯一事实来源；生成的 .html 不纳入版本控制。
    - docs/index.html 由人工维护，本脚本只替换其中 DOCS_PAYLOAD 标记之间的数据块，
      把全部文档的渲染结果与 Markdown 源码内嵌进索引页，使其在预览沙箱 / 离线环境下
      点击「阅读」时无需加载同目录的其他文件。
    - 09-archive/ 同样生成 HTML（便于回查），但不强制要求 front matter，
      且页面顶部会显示「已废止」横幅。

依赖：Python 3.9+、markdown>=3.4（python3 -m pip install markdown）
"""

from __future__ import annotations

import argparse
import html
import json
import re
import sys
from datetime import date
from pathlib import Path

try:
    import markdown
except ImportError:
    print("[ERROR] 缺少依赖 markdown，请执行：python3 -m pip install markdown", file=sys.stderr)
    sys.exit(2)

ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs"
INDEX = DOCS / "index.html"
ARCHIVE_DIR = "09-archive"

# index.html 中由本脚本维护的数据块标记
PAYLOAD_START = "<!-- DOCS_PAYLOAD_START -->"
PAYLOAD_END = "<!-- DOCS_PAYLOAD_END -->"

# docs/ 之外、但被文档引用到的工程文件，一并内嵌，保证索引页内所有链接都可就地打开
# （T0 骨架 README 由 TEST §2.1 / ROADMAP §6 / tests/README.md 引用）
EMBED_EXTRA = (
    "README.md",
    "CHANGELOG.md",
    "src/README.md",
    "tests/README.md",
    "tests/e2e/t0/README.md",
    "tests/e2e/t1/README.md",
    "tests/spikes/README.md",
    "tests/spikes/s5/README.md",
    "scripts/README.md",
    "config/coldrevive.example.toml",
)

REQUIRED_FIELDS = ("doc_id", "title", "category", "version", "status", "owner", "updated")
MD_EXTS = ("tables", "fenced_code", "toc", "sane_lists", "attr_list")

STATUS_STYLE = {
    "有效": "ok", "已接受": "ok", "持续维护": "live", "待评审": "review",
    "待定": "review", "提议": "review", "草稿": "draft",
    "已废止": "deprecated", "已取代": "deprecated",
}

FM_RE = re.compile(r"\A---\s*\n(.*?)\n---\s*\n", re.S)
LINK_RE = re.compile(r"\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
HREF_RE = re.compile(r'href="([^"]*)"')
MERMAID_RE = re.compile(r'<pre><code class="language-mermaid">(.*?)</code></pre>', re.S)

CSS = """
:root{--bg:#f6f7f9;--surface:#fff;--border:#e3e6ea;--text:#1f2328;--muted:#626b78;
--accent:#0b63ce;--accent-soft:#eaf2fe;--code-bg:#f2f4f7;--shadow:0 1px 3px rgba(16,24,40,.06)}
*{box-sizing:border-box}
html{scroll-behavior:smooth}
body{margin:0;background:var(--bg);color:var(--text);
font:15px/1.75 -apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Hiragino Sans GB","Microsoft YaHei",sans-serif}
a{color:var(--accent);text-decoration:none}a:hover{text-decoration:underline}
.wrap{max-width:960px;margin:0 auto;padding:0 24px 80px}
.topbar{background:var(--surface);border-bottom:1px solid var(--border)}
.topbar .inner{max-width:960px;margin:0 auto;padding:14px 24px;display:flex;align-items:center;gap:14px;flex-wrap:wrap}
.brand{font-weight:700;font-size:16px;color:var(--text)}
.crumb{color:var(--muted);font-size:13px;flex:1}
.crumb a{color:var(--muted)}
.hero{background:linear-gradient(180deg,#fff,var(--bg));border-bottom:1px solid var(--border);padding:36px 0 30px}
.hero h1{margin:0 0 10px;font-size:27px;letter-spacing:-.4px}
.hero p{margin:0;color:var(--muted);max-width:720px}
.meta{display:flex;gap:8px;flex-wrap:wrap;margin-top:16px}
.chip{display:inline-block;padding:3px 10px;border-radius:999px;font-size:12px;font-weight:600;
background:var(--accent-soft);color:var(--accent);border:1px solid #cfe1fb}
.chip.muted{background:#f0f2f5;color:var(--muted);border-color:var(--border)}
.chip.ok{background:#e7f6ed;color:#177245;border-color:#c7e9d4}
.chip.live{background:#e8f1ff;color:#0b63ce;border-color:#cfe1fb}
.chip.review{background:#fff4e0;color:#9a6300;border-color:#f5dfb5}
.chip.draft{background:#f0f2f5;color:#626b78;border-color:var(--border)}
.chip.deprecated{background:#fdeeee;color:#a33a3a;border-color:#f3cccc}
.banner{padding:12px 16px;border-radius:8px;margin:22px 0;font-size:14px}
.banner.warn{background:#fff4e0;border:1px solid #f5dfb5;color:#7a4e00}
.banner.info{background:var(--accent-soft);border:1px solid #cfe1fb;color:#0a4c9e}
.card{background:var(--surface);border:1px solid var(--border);border-radius:10px;
padding:26px 30px;margin-top:26px;box-shadow:var(--shadow)}
.card h2{margin:0 0 18px;font-size:19px;padding-bottom:10px;border-bottom:1px solid var(--border)}
.card h3{font-size:16px;margin:24px 0 10px}
h1.doc-title{font-size:26px;margin:0 0 6px}
p.lead{color:var(--muted);margin:0 0 20px}
table{border-collapse:collapse;width:100%;margin:14px 0;font-size:14px}
th,td{border:1px solid var(--border);padding:8px 11px;text-align:left;vertical-align:top}
th{background:#f4f6f9;font-weight:600}
tr:nth-child(even) td{background:#fbfcfd}
pre{background:var(--code-bg);border:1px solid var(--border);border-radius:8px;
padding:14px 16px;overflow:auto;font-size:13px;line-height:1.6}
code{font-family:"SFMono-Regular",Consolas,"Liberation Mono",monospace;font-size:13px}
p code,li code,td code{background:var(--code-bg);padding:1px 5px;border-radius:4px}
pre code{background:none;padding:0}
blockquote{margin:16px 0;padding:10px 16px;border-left:3px solid var(--accent);
background:var(--accent-soft);border-radius:0 6px 6px 0;color:#294a75}
blockquote p{margin:4px 0}
img{max-width:100%}
hr{border:0;border-top:1px solid var(--border);margin:28px 0}
.footer{text-align:center;color:var(--muted);font-size:13px;margin-top:36px}
.src-link{float:right;font-size:13px;font-weight:500}
ul,ol{padding-left:24px}
.mermaid{background:var(--surface);border:1px dashed var(--border);border-radius:8px;padding:12px;text-align:center}
@media(max-width:640px){.wrap{padding:0 16px 60px}.card{padding:18px 16px}.hero h1{font-size:22px}}
"""

PAGE_TPL = """<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} · ColdRevive 文档</title>
<style>{css}</style>
</head>
<body>
<div class="topbar"><div class="inner">
  <a class="brand" href="{home}index.html">ColdRevive 文档</a>
  <span class="crumb"><a href="{home}index.html">文档索引</a> / {category} / {title}</span>
</div></div>
<header class="hero"><div class="wrap">
  <h1>{title}</h1>
  <p>{subtitle}</p>
  <div class="meta">{chips}</div>
</div></header>
<main class="wrap">
  {banner}
  <div class="card">
    <a class="src-link" href="{src}">查看 Markdown 源文件 →</a>
    <div class="markdown-body">
{body}
    </div>
  </div>
  <div class="footer">
    <a href="{home}index.html">← 返回文档索引</a> · 由 <code>scripts/build_docs.py</code> 生成 · {today}
  </div>
</main>
<script src="https://cdn.jsdelivr.net/npm/mermaid@10/dist/mermaid.min.js" onerror="void 0"></script>
<script>
if (window.mermaid) {{ mermaid.initialize({{ startOnLoad: true, theme: 'neutral' }}); }}
</script>
</body></html>
"""


def parse_front_matter(text: str) -> tuple[dict, str]:
    """返回 (元数据, 去掉 front matter 的正文)。无 front matter 时返回 ({}, 原文)。"""
    m = FM_RE.match(text)
    if not m:
        return {}, text
    meta: dict[str, str] = {}
    for line in m.group(1).splitlines():
        line = line.strip()
        if not line or line.startswith("#") or ":" not in line:
            continue
        key, _, value = line.partition(":")
        meta[key.strip()] = value.strip().strip('"').strip("'")
    return meta, text[m.end():]


def rewrite_links(body_html: str, src_path: Path, docs_md: set[Path]) -> str:
    """把指向 docs 内 .md 的相对链接改为 .html，使生成的网页之间可互相跳转。

    docs/ 之外的 Markdown（根 README、CHANGELOG、src/、tests/ 等）没有配套 HTML，
    保持原样，避免生成断链。
    """
    def repl(m: re.Match) -> str:
        href = m.group(1)
        if href.startswith(("http://", "https://", "mailto:", "//", "#", "data:")):
            return m.group(0)
        if "#" in href:
            path, anchor = href.split("#", 1)
            suffix = "#" + anchor
        else:
            path, suffix = href, ""
        if path.endswith(".md") and (src_path.parent / path).resolve() in docs_md:
            path = path[:-3] + ".html"
        return 'href="' + path + suffix + '"'
    return HREF_RE.sub(repl, body_html)


def render_mermaid(body_html: str) -> str:
    """将 fenced mermaid 代码块转成 mermaid 容器；离线时 JS 不加载，代码仍可读。"""
    return MERMAID_RE.sub(
        lambda m: '<pre class="mermaid">' + m.group(1) + "</pre>", body_html
    )


def render_body(path: Path, docs_md: set[Path]) -> tuple[dict, str, str]:
    """返回 (front matter, 渲染后的 HTML 片段, 原始文本)。

    同一份渲染逻辑同时供独立 HTML 页面与 index.html 的内嵌阅读器使用，
    避免两条路径产生内容差异。
    """
    text = path.read_text(encoding="utf-8")
    if path.suffix.lower() == ".md":
        meta, body_md = parse_front_matter(text)
        body_html = markdown.markdown(body_md, extensions=MD_EXTS)
        body_html = rewrite_links(body_html, path, docs_md)
        body_html = render_mermaid(body_html)
        # 去掉正文中重复的一级标题，标题由页面 / 阅读器单独展示
        body_html = re.sub(r"\A\s*<h1[^>]*>.*?</h1>", "", body_html, count=1, flags=re.S)
    else:
        meta = {}
        body_html = '<pre class="raw-source"><code>' + html.escape(text) + "</code></pre>"
    return meta, body_html, text


def embed_payload(md_files: list[Path], docs_md: set[Path]) -> int:
    """把全部文档（含 docs/ 外被引用的工程文件）内嵌进 docs/index.html。

    payload 的 key 一律是「相对项目根目录的 POSIX 路径」，与预览环境的 URL 无关，
    因此无论以 file://、http:// 还是 srcdoc 沙箱打开索引页都能正确解析链接。
    """
    if not INDEX.is_file():
        print(f"[WARN] 未找到 {INDEX.relative_to(ROOT)}，跳过内嵌。", file=sys.stderr)
        return 0

    original = INDEX.read_text(encoding="utf-8")
    if PAYLOAD_START not in original or PAYLOAD_END not in original:
        print("[WARN] index.html 缺少 DOCS_PAYLOAD 标记，跳过内嵌。", file=sys.stderr)
        return 0

    sources = sorted(
        {p.resolve() for p in [*md_files, *(ROOT / e for e in EMBED_EXTRA if (ROOT / e).is_file())]},
        key=lambda p: p.relative_to(ROOT).as_posix(),
    )

    entries: dict[str, dict] = {}
    for path in sources:
        key = path.relative_to(ROOT).as_posix()
        meta, fragment, raw = render_body(path, docs_md)
        entries[key] = {
            "t": meta.get("title") or path.stem,
            "id": meta.get("doc_id", ""),
            "s": meta.get("status", ""),
            "h": fragment,
            "m": raw,
        }

    # 不含构建日期：保证相同 Markdown 输入产生完全一致的 index.html，便于 CI 用 git diff 校验同步
    payload = {"docs": entries}
    data = json.dumps(payload, ensure_ascii=False, separators=(",", ":"))
    # 防止内容中的 </script> 提前闭合数据块；<\/ 是合法 JSON 转义
    data = data.replace("</", "<\\/").replace("\u2028", "\\u2028").replace("\u2029", "\\u2029")

    block = (
        PAYLOAD_START
        + '\n<script id="docs-payload" type="application/json">'
        + data
        + "</script>\n"
        + PAYLOAD_END
    )
    # 用「首个 START + 最后一个 END」切片，而不是正则：文档正文里可能原样引用
    # 这两个标记名，非贪婪正则会提前截断并逐次破坏 index.html。
    start_i = original.find(PAYLOAD_START)
    end_i = original.rfind(PAYLOAD_END)
    if start_i < 0 or end_i < start_i:
        print("[WARN] index.html 的 DOCS_PAYLOAD 标记不完整，跳过内嵌。", file=sys.stderr)
        return 0

    updated = original[:start_i] + block + original[end_i + len(PAYLOAD_END):]
    if updated != original:
        INDEX.write_text(updated, encoding="utf-8")
    print(f"[OK] 已内嵌 {len(entries)} 份文档到 docs/index.html（{len(data):,} 字节）。")
    return len(entries)


def collect_md() -> list[Path]:
    return sorted(p for p in DOCS.rglob("*.md") if p.is_file())


def check_links(md_files: list[Path]) -> list[str]:
    errors: list[str] = []
    for path in md_files:
        text = path.read_text(encoding="utf-8")
        for m in LINK_RE.finditer(text):
            target = m.group(1).strip()
            if target.startswith(("http://", "https://", "mailto:", "#", "data:", "//")):
                continue
            rel = target.split("#", 1)[0]
            if not rel:
                continue
            resolved = (path.parent / rel).resolve()
            if not resolved.exists():
                errors.append(f"{path.relative_to(ROOT)} → 断链: {target}")
    return errors


def build(md_files: list[Path], check_only: bool) -> int:
    errors: list[str] = []
    generated = 0
    docs_md = {p.resolve() for p in md_files}

    for path in md_files:
        rel = path.relative_to(DOCS)
        is_archive = rel.parts[0] == ARCHIVE_DIR
        meta = parse_front_matter(path.read_text(encoding="utf-8"))[0]

        if not is_archive:
            missing = [k for k in REQUIRED_FIELDS if not meta.get(k)]
            if missing:
                errors.append(f"{path.relative_to(ROOT)} → front matter 缺字段: {', '.join(missing)}")

        if check_only:
            continue

        title = meta.get("title") or path.stem
        status = meta.get("status") or ("已废止" if is_archive else "草稿")
        category = meta.get("category") or ("归档" if is_archive else "文档")
        style = STATUS_STYLE.get(status, "draft")

        chips = [
            f'<span class="chip">{html.escape(meta.get("doc_id", "—"))}</span>',
            f'<span class="chip muted">v{html.escape(meta.get("version", "—"))}</span>',
            f'<span class="chip {style}">{html.escape(status)}</span>',
            f'<span class="chip muted">{html.escape(meta.get("owner", "—"))}</span>',
            f'<span class="chip muted">{html.escape(meta.get("updated", "—"))}</span>',
        ]

        banner = ""
        if is_archive:
            banner = ('<div class="banner warn"><strong>已废止 / 归档基线</strong>：'
                      '本文件内容不作为设计、开发或评审依据，仅供追溯决策演化。'
                      '请以 <a href="../index.html">现行文档</a> 为准。</div>')

        body_html = render_body(path, docs_md)[1]

        home = "../" * (len(rel.parts) - 1)
        page = PAGE_TPL.format(
            title=html.escape(title),
            subtitle=html.escape(meta.get("category", "") + " · ColdRevive 项目开发文档"),
            css=CSS,
            home=home,
            category=html.escape(category),
            chips="".join(chips),
            banner=banner,
            src=path.name,
            body=body_html,
            today=date.today().isoformat(),
        )
        out = path.with_suffix(".html")
        out.write_text(page, encoding="utf-8")
        generated += 1

    link_errors = check_links(md_files)
    errors.extend(link_errors)

    idx_text = INDEX.read_text(encoding="utf-8") if INDEX.is_file() else ""
    if PAYLOAD_START not in idx_text or PAYLOAD_END not in idx_text:
        errors.append("docs/index.html → 缺少 DOCS_PAYLOAD 标记（内嵌阅读器无法工作）")

    if check_only:
        if errors:
            print(f"[FAIL] 校验未通过，共 {len(errors)} 个问题：")
            for e in errors:
                print("  - " + e)
            return 1
        print(f"[OK] 校验通过：{len(md_files)} 份 Markdown，front matter、相对链接与索引页数据块均有效。")
        return 0

    if errors:
        print(f"[WARN] 生成完成，但发现 {len(errors)} 个问题：")
        for e in errors:
            print("  - " + e)

    embed_payload(md_files, docs_md)
    print(f"[OK] 已生成 {generated} 个独立 HTML 页面；docs/index.html 仅更新 DOCS_PAYLOAD 数据块。")
    return 1 if link_errors else 0


def main() -> int:
    ap = argparse.ArgumentParser(description="ColdRevive 文档站构建器")
    ap.add_argument("--check", action="store_true", help="仅校验，不写文件")
    args = ap.parse_args()

    if not DOCS.is_dir():
        print(f"[ERROR] 文档目录不存在：{DOCS}", file=sys.stderr)
        return 2

    md_files = collect_md()
    if not md_files:
        print("[ERROR] 未找到任何 Markdown 文档", file=sys.stderr)
        return 2

    return build(md_files, args.check)


if __name__ == "__main__":
    sys.exit(main())
