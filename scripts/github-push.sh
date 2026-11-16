#!/usr/bin/env bash
# github-push.sh — GitHub 连接受限时的推送重试梯子
#
# 2026-11-16 实测过的路径（A 机网络，国内）：
#   1 现有配置          走环境代理（WorkBuddy 沙箱注入，端口每会话随机）
#   2 显式直连          清代理，git 智能 HTTP 默认版本
#   3 直连 + HTTP/1.1   关键后备：代理 502、默认 HTTP/2 卡住时这条实测成功推过 3 个提交
#   4 显式代理          直连被限制时用
#   5 代理 + HTTP/1.1   代理能握上但 HTTP/2 出问题时的最后一条传输层路径
#   api.github.com      实测代理/直连都 200，gh CLI 已登录且有 repo scope —— 传输层全挂时
#                       可用 GitHub Git Data API 逐文件提交（不是 push，需人工执行，本脚本不伪造）
#   ssh.github.com:443  TCP 可达；默认密钥与实验室密钥都 Permission denied（公钥未注册），
#                       先把 ~/.ssh 的 ed25519 公钥加到 GitHub 账户才会成为可用后备
#   github.com:22       TCP 可达但与 443 一样缺注册公钥；早先的"22 超时"是当时的网络状态，
#                       不要当永久结论，也不要当成可用路径
#
# 用法: scripts/github-push.sh [remote] [branch]     （默认 origin main）
# 纪律: 每一级都要真实执行 git push，失败如实退出 1，绝不伪造成功。
set -u
set -o pipefail   # 管道里取 git push 的退出码而不是 tail 的，否则失败被吞

REMOTE="${1:-origin}"
BRANCH="${2:-main}"
PROXY="${HTTPS_PROXY:-${https_proxy:-}}"

attempt() {
  label="$1"; shift
  echo "== $label =="
  # -c 是 git 全局选项，必须放在子命令 push 之前；写成 git push -c ... 会打印 usage 并假失败
  if git "$@" push "$REMOTE" "$BRANCH" 2>&1 | tail -3; then
    return 0
  fi
  return 1
}

# 1: 现有配置
if attempt "现有配置 (proxy=${PROXY:-none})"; then exit 0; fi

# 2: 显式直连
if attempt "直连(无代理)" -c http.proxy= -c https.proxy=; then
  echo ">> 结论: 代理异常，直连可用"; exit 0
fi

# 3: 直连 + HTTP/1.1（本轮救场路径）
if attempt "直连 + HTTP/1.1" -c http.proxy= -c https.proxy= -c http.version=HTTP/1.1; then
  echo ">> 结论: HTTP/2 或代理有问题，直连 HTTP/1.1 可用"; exit 0
fi

# 4: 显式走环境代理
if [ -n "$PROXY" ]; then
  if attempt "显式代理 $PROXY" -c http.proxy="$PROXY" -c https.proxy="$PROXY"; then
    echo ">> 结论: 直连受限，代理可用"; exit 0
  fi
  # 5: 代理 + HTTP/1.1
  if attempt "显式代理 $PROXY + HTTP/1.1" \
       -c http.proxy="$PROXY" -c https.proxy="$PROXY" -c http.version=HTTP/1.1; then
    echo ">> 结论: 代理可用但 HTTP/2 有问题，改用 HTTP/1.1"; exit 0
  fi
fi

echo "FAIL: 五条传输层路径都不通。" >&2
# 只做诊断，不把 API 可达当成 push 成功。
if command -v gh >/dev/null 2>&1; then
  echo "诊断: gh CLI 状态：" >&2
  gh auth status 2>&1 | head -3 >&2
  echo "若 api.github.com 可达，可用 Git Data API 逐文件提交；需人工确认，本脚本不代办。" >&2
fi
echo "另需把 ~/.ssh 公钥注册到 GitHub 后，ssh.github.com:443 才会成为可用后备。" >&2
exit 1
