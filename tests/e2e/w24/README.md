# W24 — FR-24 本地可验证部分：真实内容 → 标准 `ed2k://` 链接

> 2026-11-20 · 独立 MD4 复算 **2/2** · 口径：公网 KAD 词典检索 **BLOCKED**，`g2_claim=false` 不变。

## 做了什么

1. `cr-hash`：给 `kad_keyword_hash`（无前缀 `MD4(UTF-8(term))`）补 RFC 1320 已知答案测试，
   防止悄悄换成 SHA-1 / 加命名空间前缀而测试仍绿。
2. `cr-kad::kad_ed2k`：把 S2 `kad-mini` 审计过的 `0x33 SEARCH_KEY_REQ` /
   `0x3B SEARCH_RES` 字节编解码与 A 轨 Key 纳入生产 crate（离线可验证原语，不对公网发包）。
3. `coldrevived` 新增 `/cli ed2k [file_hash]`：对本地索引项 `fs::read(path)` 后算
   **真实** `ed2k_hash`（MD4 over 9.28 MB 分块，Red 边界），输出标准 `ed2k://|file|…|/`
   与每关键词的无前缀 MD4 Key。`fileID` 由内容导出，不伪造。

## 证据（tests/e2e/w24/ed2k_links.json）

- 两个 256 KiB 样本：守护进程输出 `ed2k://|file|sample_a.bin|262144|49e4cf…2389|/`、
  `sample_b.bin|…|39ca23…3b0|`。
- `verify_ed2k.py` 是**独立**纯 Python RFC 1320 MD4 实现（含空串/"abc" 已知答案断言），
  对磁盘文件复算 `ed2k_hash`：**2/2 与守护进程输出逐字节一致**。

## 边界（照写）

- **不查公网 eMule 词典**：G0 `protocol_scope` 仍是「跳过 HELLO」；OPEN-QUESTIONS §1 记录
  公网 `0x33`/`0x43` 索引操作 0/19 全静默。`/cli ed2k` 的 `public_search=BLOCKED` 明写此点。
- 关键词 Key 是哈希原语，不是「检索到了结果」；没有产生任何对公网 KAD 的查询包。
- 这是 FR-24 的本地可验证部分，不代表 FR-24 全项完成。

## 复现

```bash
python tests/e2e/w24/verify_ed2k.py
```
