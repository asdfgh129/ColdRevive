# provenance 更正说明（g2-fairness-v2-20261128）

## 发生了什么

`provenance.txt` 是 harness 的原样输出，其中：

```
git_commit=unknown
git_dirty=0
```

原因是构建目录 `/opt/cr-ledbat` 是**用 tar 从本地工作树同步过去的快照，没有 `.git`**。
`git -C … rev-parse HEAD` 因此失败，脚本按设计写了 `unknown`；`git_dirty=0` 是空输出的
行数，**不代表工作树干净**——这一行本身没有信息量。

按 EX-1 C4 第 2 条，`git_commit` 是必填 provenance。**用未知充数等于把 C4 的欠账又欠一次**，
所以这里做的是「事后推导 + 可复核」，不是「补个好看的字符串」：

## 怎么推导出来的（可复核）

用 git 的 blob 哈希比对「实际跑的那份源码」与提交 `a98ddc1`：

| 文件 | a98ddc1 里的 blob | 221 上实际文件的 blob | 结论 |
| :--- | :--- | :--- | :--- |
| `cr-nat/src/relay_udp_stream.rs` | `56206c4c926b3faacde52b40e7b1eba0ce67537c` | 同左 | 完全一致 |
| `cr-nat/src/lib.rs` | `5034dbb918b08cdd779b542334c935886faf35a9` | 同左 | 完全一致 |
| `bins/coldrevive-fairness-probe/src/main.rs` | `8259617d646a80996f0d47e56955f0eec618ede2` | 同左 | 完全一致 |
| `cr-core/src/lib.rs` | `d847130f3df1377ce8db22129d62d54ffc643d09` | `6fa752bc…`（CRLF） | **仅换行不同**：`\r\n → \n` 后 hash 等于 `d847130…` |

复核命令（在 221 上执行）：

```bash
cd /opt/cr-ledbat
git hash-object src/crates/cr-nat/src/relay_udp_stream.rs   # 56206c4c…
python3 -c 'b=open("src/crates/cr-core/src/lib.rs","rb").read().replace(b"\r\n",b"\n");open("/tmp/n.rs","wb").write(b)'
git hash-object /tmp/n.rs                                   # d847130f…
```

再加一道与源码无关的锚：**实际运行的探针二进制**
`sha256 0c8c41ea23ba83ff51341067663cb46a098e69f18b4d5e8f26af9f419b516f90`（698 512 B，
release）。即使有人改源码，二进制哈希对不上也就无法声称是这份证据。

## 结论的精确强度

可以说：**跑出这份数据的 cr-nat / cr-core / probe 源码，与提交 `a98ddc1` 一致**
（三份逐字节相同，一份仅行尾不同）。

不可以说：整个 `/opt/cr-ledbat` 树等于 `a98ddc1`——同步之后本地又改过
`dht_daemon.rs`（闸门拒绝时跳过该地址的修正），那份改动**不在**这次的二进制里，
也不影响公平性测量（公平性只用到 `cr-nat` + probe）。

## 下次怎么避免

harness 已经改成「缺判据文件直接退出」，但没能力在无 `.git` 的目录里生成 `git_commit`。
正确做法二选一，下一轮采样前必须做掉其中一个：

1. 在 221 上 `git clone` 到干净目录并 `git checkout <sha>`，从那里构建 —— harness 自然能
   写出真实 `git_commit`；
2. 或让同步脚本写一个 `SOURCE-TREE.sha256`（对全部参与编译的源码逐文件哈希），
   把它当 `git_commit` 的等价锚点。

本轮用的是事后推导，**属可接受但不推荐**；已经在 `docs/05-quality/TEST.md` 里记为欠账。
