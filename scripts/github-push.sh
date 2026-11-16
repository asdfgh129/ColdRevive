#!/usr/bin/env bash
# github-push.sh — GitHub 连接受限时的推送重试梯子
# 2026-11-16 实测路径（本仓库 A 机网络）：
#   路径 1  现有配置（走环境代理 http://127.0.0.1:<port>，代理活着时最快 ~1s）
#   路径 2  显式直连（清空代理）——代理 502/拒绝/空响应时用，实测 git 协议直连可用
#   路径 3  显式走环境代理——直连被限制时用
#   ssh://git@github.com:22 实测超时，不要试；ssh.github.com:443 TCP 可达，
#   但本机未注册 GitHub 公钥（Permission denied），仅作长期备份（需先把
#   ~/.ssh 的 ed25519 公钥加到 GitHub 账户）。
# 用法: scripts/github-push.sh [remote] [branch]     （默认 origin main）
# 纪律: 每一级都要真实执行 git push，失败如实退出 1，绝不伪造成功。
set -u
set -o pipefail   # 关键：管道里取 git push 的退出码而不是 tail 的，否则失败被吞

REMOTE="${1:-origin}"
BRANCH="${2:-main}"
PROXY="${HTTPS_PROXY:-${https_proxy:-}}"

attempt() {
  label="$1"; shift
  echo "== $label =="
  if git push "$@" 2>&1 | tail -3; then
    return 0
  fi
  return 1
}

# 路径 1: 现有配置
if attempt "现有配置 (proxy=${PROXY:-none})" "$REMOTE" "$BRANCH"; then
  exit 0
fi

# 路径 2: 显式直连
if attempt "直连(无代理)" -c http.proxy= -c https.proxy= "$REMOTE" "$BRANCH"; then
  echo ">> 结论: 代理异常，直连可用"
  exit 0
fi

# 路径 3: 显式走环境代理
if [ -n "$PROXY" ]; then
  if attempt "显式代理 $PROXY" -c http.proxy="$PROXY" -c https.proxy="$PROXY" "$REMOTE" "$BRANCH"; then
    echo ">> 结论: 直连受限，代理可用"
    exit 0
  fi
fi

echo "FAIL: 三条路径都不通。备选：检查代理进程是否存活(curl -x $PROXY https://github.com)，
或配置 ssh.github.com:443 公钥认证后改用 ssh 远程。" >&2
exit 1
