# 脚本目录（scripts/）

| 脚本 | 用途 |
| :--- | :--- |
| `build_docs.py` | Markdown → HTML 文档站构建；把全部文档内嵌进 `docs/index.html`；`--check` 校验 front matter、相对链接与索引页数据块 |

> 测试脚本不放在本目录：T0 验收（`t0_harness.py` / `t0_adapters.py` / `t0_cli.py` / `t0_accept.py`）位于 `tests/e2e/t0/`，M0 就绪度检查位于 `tests/spikes/`，用法见 [T0 骨架说明](../tests/e2e/t0/README.md) 与 [M0 实验包](../tests/spikes/README.md)。

## 用法

```bash
# 生成各文档配套 HTML 页面，并更新 docs/index.html 的 DOCS_PAYLOAD 数据块
python3 scripts/build_docs.py

# 仅校验，不写文件（供 CI 使用）
python3 scripts/build_docs.py --check

# 依赖
python3 -m pip install markdown
```

## 约定

- `docs/**/*.md` 是唯一事实来源，生成的 `.html` 已由 `.gitignore` 排除（`docs/index.html` 除外）。
- **`docs/index.html` 是自包含单文件门户**：构建器把 33 份文档正文 + Markdown 源码、以及 `docs/` 外被引用的 8 个工程文件（含 `tests/e2e/t0/README.md`、`tests/spikes/README.md`）写入页内 `DOCS_PAYLOAD` 区块。只复制这一个 HTML 文件即可离线读完全套文档，点击「阅读」不会加载同目录其他文件（预览沙箱 / `srcdoc` 下也不会失效）。手工编辑 `index.html` 时不要改动两个 `DOCS_PAYLOAD` 标记之间的内容。
- 修改任一 `.md` 后必须重跑构建，否则索引页内嵌内容不会同步更新。
- 新增文档必须填写 front matter（`doc_id` / `title` / `category` / `version` / `status` / `owner` / `updated`），否则 `--check` 会失败。
- 新增文档后需在 `docs/index.html` 的 `DOCS` 清单中登记，并同步 `docs/README.md` 的文档编号表。
- 后续计划补充：`gen_config_schema.py`（配置校验）、`dht_simulator.py`（T3 环境）、`bench_index.py`（NFR-01 压测）。
