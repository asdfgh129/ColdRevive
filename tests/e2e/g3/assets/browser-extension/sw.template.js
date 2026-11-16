// G3 浏览器插件取证：服务 worker 启动时把一条**真实**的 .torrent 链接推送到 ColdRevive。
//
// 这是模板：`tests/e2e/g3/browser_push.py` 会把 __RPC_URL__ / __LINK__ 替换成本次运行的
// 实际地址后渲染到临时目录，再用 --load-extension 加载。模板里刻意不放占位链接，
// 避免把 example.com 这种假链接当成证据留在仓库里。
//
// 插件只负责「推送链接」，不下载内容、不伪造任务状态；任务是否真的被解析由
// ColdRevive 的 /jsonrpc 回包与 tellStatus 证明。
const RPC = '__RPC_URL__';
const LINK = '__LINK__';

fetch(RPC, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    jsonrpc: '2.0', id: 1, method: 'aria2.addUri',
    params: ['token:none', [LINK]]
  })
}).then(r => r.json())
  .then(j => console.log('[coldrevive-g3] pushed, gid=' + (j.result || JSON.stringify(j.error))))
  .catch(e => console.log('[coldrevive-g3] push failed: ' + e));
