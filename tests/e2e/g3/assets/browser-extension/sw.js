// G3 浏览器插件取证：服务 worker 启动时把一条标准下载链接推送到 ColdRevive。
// 只推送链接并等待 /jsonrpc 返回 gid，不下载内容、不伪造任务状态。
const RPC = 'http://127.0.0.1:18800/jsonrpc';
const LINK = 'https://example.com/coldrevive-g3-browser-push.bin';

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
