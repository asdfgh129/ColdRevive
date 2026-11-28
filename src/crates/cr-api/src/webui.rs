//! 内嵌 Web UI（FR-19 / W6.1 / W6.4；DOC-35 决定 2 选 B 后的唯一界面形态）。
//!
//! HTML/CSS/JS 全部在编译期嵌进二进制，运行时不写盘、不访问 CDN。
//! 默认关闭（`api.web_ui=false`）；开启时只占 `GET /` 与 `GET /ui/*`，不遮蔽任何既有端点。
//!
//! **不轮询**（ADR-008）：节点计数走 `/ws` 每秒推送，任务列表只在「收到推送且
//! `seed_tasks` 变化」「用户点了操作」「用户点了刷新」时拉取。这样断线时界面会
//! 如实变成「未连接」，而不是继续显示上一次的陈旧数字。
//!
//! 决定 2 选 B 意味着本项目**不做托盘 / 浏览器下载捕获 / UPnP 续租**：本文件就是
//! 用户界面，没有原生外壳。要关掉界面直接关浏览器标签，下载与做种不受影响。

/// 首页 HTML。外链只允许同源的 `/ui/*` 静态资产。
pub const INDEX_HTML: &str = r#"<!doctype html>
<html lang="zh">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>ColdRevive console</title>
<link rel="stylesheet" href="/ui/style.css">
</head>
<body>
<header>
<h1>ColdRevive</h1>
<span id="conn" class="conn off" title="与守护进程的实时连接">未连接</span>
</header>
<p class="hint">状态由守护进程通过 WebSocket 推送；任务操作走 aria2 兼容 JSON-RPC。关闭本页不影响下载与做种。</p>

<section>
<h2>节点</h2>
<div id="node" class="cards"><span class="hint">等待推送…</span></div>
</section>

<section>
<h2>任务 <button id="reload" class="ghost" type="button">刷新</button></h2>
<form id="add">
<input id="uri" placeholder="magnet: 或 http(s)://…/*.torrent" size="46" autocomplete="off">
<input id="token" type="password" placeholder="rpc-secret（如已启用）" size="18" autocomplete="off">
<button type="submit">添加</button>
</form>
<div id="tasks"><span class="hint">尚未加载</span></div>
</section>

<div id="err" role="status"></div>
<script src="/ui/app.js"></script>
</body>
</html>
"#;

pub const STYLE_CSS: &str = r#":root{color-scheme:dark}
body{font:14px/1.55 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;background:#111;color:#ddd;margin:0 auto;padding:1.5rem;max-width:960px}
header{display:flex;align-items:baseline;gap:.8rem}h1{font-size:1.3rem;margin:0}
h2{font-size:1rem;margin:0 0 .6rem;color:#7bd;display:flex;align-items:center;gap:.6rem}
section{margin-top:1.6rem}.hint{color:#888}
.conn{font-size:.8rem;padding:.1rem .5rem;border-radius:999px;border:1px solid #444}
.conn.on{color:#7f7;border-color:#2a5}.conn.off{color:#f77;border-color:#633}
input,button{font:inherit;background:#1b1b1b;color:#ddd;border:1px solid #555;padding:.35rem .6rem;border-radius:4px}
button{cursor:pointer;background:#26333b}button.ghost{background:transparent;font-size:.85rem}
#add{display:flex;gap:.4rem;flex-wrap:wrap;margin-bottom:.8rem}
.cards{display:flex;gap:.6rem;flex-wrap:wrap}
.card{background:#181818;border:1px solid #333;border-radius:6px;padding:.6rem .9rem;min-width:8rem}
.card .k{color:#888;font-size:.8rem}.card .v{font-size:1.15rem}
.task{border:1px solid #333;border-radius:6px;padding:.6rem .8rem;margin-bottom:.5rem;background:#161616}
.task .row{display:flex;justify-content:space-between;gap:.6rem;flex-wrap:wrap}
.task .gid{color:#7bd}.task .st{color:#fd8}.task .name{word-break:break-all}
.bar{height:6px;background:#2a2a2a;border-radius:3px;margin:.5rem 0;overflow:hidden}
.bar>i{display:block;height:100%;background:#3a8;transition:width .3s}
.acts{display:flex;gap:.3rem;margin-top:.4rem}.acts button{font-size:.8rem;padding:.15rem .5rem}
#err{color:#f77;min-height:1.3rem;margin-top:1rem;white-space:pre-wrap}"#;

pub const APP_JS: &str = r#""use strict";
const q = id => document.getElementById(id);
const esc = v => String(v).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const fmtBytes = n => {
  n = Number(n) || 0;
  const u = ['B','KiB','MiB','GiB','TiB']; let i = 0;
  while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
  return (i ? n.toFixed(1) : n) + ' ' + u[i];
};
const setErr = m => { q('err').textContent = m || ''; };

function token() {
  const t = q('token').value.trim();
  return t ? 'token:' + t : null;
}
// 记住 token 但只存在本标签页：不落 localStorage，避免共享机器上留痕。
try { q('token').value = sessionStorage.getItem('cr-token') || ''; } catch (e) {}
q('token').addEventListener('change', () => {
  try { sessionStorage.setItem('cr-token', q('token').value.trim()); } catch (e) {}
});

async function rpc(method, params) {
  const body = {jsonrpc:'2.0', id:Date.now(), method, params};
  const r = await fetch('/jsonrpc', {method:'POST', headers:{'content-type':'application/json'}, body:JSON.stringify(body)});
  if (!r.ok) throw new Error('HTTP ' + r.status);
  const j = await r.json();
  if (j.error) throw new Error(j.error.message || ('rpc ' + j.error.code));
  return j.result;
}
const withToken = args => { const t = token(); return t ? [t].concat(args) : args; };

function renderNode(s) {
  const cards = [
    ['节点', s.node || '—'],
    ['活跃种子', s.seed_tasks],
    ['累计上传', fmtBytes(s.served_bytes)],
    ['推送时间', new Date(s.ts_ms || Date.now()).toLocaleTimeString()],
  ];
  q('node').innerHTML = cards.map(([k, v]) =>
    '<div class="card"><div class="k">' + esc(k) + '</div><div class="v">' + esc(v) + '</div></div>').join('');
}

function renderTasks(all) {
  if (!all.length) { q('tasks').innerHTML = '<span class="hint">暂无任务</span>'; return; }
  q('tasks').innerHTML = all.map(t => {
    const gid = t.gid || '', st = t.status || '', name = t.name || t.uri || '';
    const done = Number(t.completedLength || 0), total = Number(t.totalLength || 0);
    const pct = total ? Math.min(100, done * 100 / total) : 0;
    const speed = Number(t.downloadSpeed || 0);
    const act = st === 'active'
      ? '<button data-act="pause" data-gid="' + esc(gid) + '">暂停</button>'
      : '<button data-act="unpause" data-gid="' + esc(gid) + '">继续</button>';
    return '<div class="task"><div class="row"><span class="gid">' + esc(gid) + '</span>' +
      '<span class="st">' + esc(st) + '</span></div>' +
      '<div class="name">' + esc(name) + '</div>' +
      '<div class="bar"><i style="width:' + pct.toFixed(1) + '%"></i></div>' +
      '<div class="row"><span>' + fmtBytes(done) + ' / ' + fmtBytes(total) + '</span>' +
      (st === 'active' ? '<span>' + fmtBytes(speed) + '/s</span>' : '<span class="hint">—</span>') +
      '</div><div class="acts">' + act +
      '<button data-act="remove" data-gid="' + esc(gid) + '">移除记录</button></div></div>';
  }).join('');
}

async function loadTasks() {
  try {
    const [a, w, s] = await Promise.all([
      rpc('aria2.tellActive', withToken([[]])),
      rpc('aria2.tellWaiting', withToken([[]])),
      rpc('aria2.tellStopped', withToken([[]])),
    ]);
    renderTasks([].concat(a || [], w || [], s || []));
    setErr('');
  } catch (e) {
    // 失败必须说清是「鉴权」还是「进程不可达」，不能只显示一个红点。
    setErr('读取任务失败：' + e.message + (token() ? '' : '（若已启用 rpc-secret，请在上面填写）'));
  }
}

q('add').addEventListener('submit', async e => {
  e.preventDefault();
  const uri = q('uri').value.trim();
  if (!uri) return;
  try { await rpc('aria2.addUri', withToken([[uri]])); q('uri').value = ''; setErr(''); await loadTasks(); }
  catch (err) { setErr('添加失败：' + err.message); }
});
q('reload').addEventListener('click', loadTasks);
q('tasks').addEventListener('click', async e => {
  const b = e.target.closest('button');
  if (!b) return;
  const act = b.dataset.act, gid = b.dataset.gid;
  if (!gid) return;
  // 显式方法表：按钮名不直接拼进 RPC 方法，避免未知 data-act
  // 变成任意 aria2 方法调用；也让「有哪些任务操作」在代码里可审计。
  const methods = {
    pause: 'aria2.pause',
    unpause: 'aria2.unpause',
    remove: 'aria2.removeDownloadResult',
  };
  const method = methods[act];
  if (!method) { setErr('未知操作：' + act); return; }
  try { await rpc(method, withToken([gid])); setErr(''); await loadTasks(); }
  catch (err) { setErr(act + ' 失败：' + err.message); }
});

// ---- 实时连接：/ws 每秒推一帧。断线就显示断线，不拿旧数字充数。 ----
let ws = null, lastSeedTasks = null, retry = 0;
function setConn(on, label) {
  const el = q('conn');
  el.className = 'conn ' + (on ? 'on' : 'off');
  el.textContent = label;
}
function connect() {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  setConn(false, retry ? '重连中…' : '连接中…');
  ws = new WebSocket(proto + '//' + location.host + '/ws');
  ws.onopen = () => { retry = 0; setConn(true, '已连接'); setErr(''); };
  ws.onmessage = ev => {
    let s;
    try { s = JSON.parse(ev.data); } catch (e) { return; }
    renderNode(s);
    // 种子数变化 = 任务集合可能变了；这是事件驱动，不是定时轮询。
    if (lastSeedTasks !== null && s.seed_tasks !== lastSeedTasks) loadTasks();
    lastSeedTasks = s.seed_tasks;
  };
  ws.onclose = () => {
    setConn(false, '已断开');
    setErr('与守护进程的连接已断开（守护进程可能已停止）。重试中…');
    retry = Math.min(retry + 1, 6);
    setTimeout(connect, Math.min(1000 * retry, 8000));
  };
  ws.onerror = () => { try { ws.close(); } catch (e) {} };
}
connect();
loadTasks();
"#;

pub const CONTENT_TYPE: &str = "text/html; charset=utf-8";
pub const CSS_CONTENT_TYPE: &str = "text/css; charset=utf-8";
pub const JS_CONTENT_TYPE: &str = "application/javascript; charset=utf-8";

/// 只有首页与同源静态资产交给 Web UI；任何已存在的端点都不被它遮蔽。
pub fn is_web_ui_path(path: &str, enabled: bool) -> bool {
    enabled && matches!(path, "/" | "/index.html")
}

/// 编译期嵌入的 `/ui/*` 资产。没有外部资源，也没有路径拼接，查表命中才返回。
pub fn asset(path: &str, enabled: bool) -> Option<(&'static str, &'static str)> {
    if !enabled { return None; }
    match path {
        "/ui/style.css" => Some((CSS_CONTENT_TYPE, STYLE_CSS)),
        "/ui/app.js" => Some((JS_CONTENT_TYPE, APP_JS)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_are_self_contained() {
        assert!(INDEX_HTML.starts_with("<!doctype html>"));
        assert!(INDEX_HTML.contains("/ui/app.js") && INDEX_HTML.contains("/ui/style.css"));
        for needle in ["http://", "https://", "<script src=\"http", "@import"] {
            assert!(!INDEX_HTML.contains(needle), "外部资源引用: {needle}");
        }
        for (name, body) in [("css", STYLE_CSS), ("js", APP_JS)] {
            assert!(!body.contains("http://") && !body.contains("https://"),
                    "{name} 引用了外部资源");
        }
        // 决定 2 选 B 之后界面会继续长，但必须仍然是「一个内嵌页面」，不能膨胀成
        // 需要构建步骤的 SPA。上限按可读性给，不按字节抠。
        assert!(INDEX_HTML.len() < 8192, "首页过大: {} B", INDEX_HTML.len());
        assert!(APP_JS.len() < 24576, "脚本过大: {} B", APP_JS.len());
    }

    #[test]
    fn ui_uses_push_not_polling() {
        // ADR-008：状态推送必须走 WebSocket，禁止定时轮询。
        assert!(APP_JS.contains("new WebSocket"), "界面必须建立 /ws 连接");
        assert!(APP_JS.contains("/ws"), "必须连到 /ws");
        assert!(!APP_JS.contains("setInterval"), "禁止定时轮询：改用 WS 推送驱动");
    }

    #[test]
    fn disconnects_are_reported_not_hidden() {
        // 断线时不能保留旧数字装作正常。
        assert!(APP_JS.contains("onclose"), "必须处理断线");
        assert!(APP_JS.contains("已断开"), "断线必须有可见文案");
        assert!(APP_JS.contains("setTimeout(connect"), "断线后要有退避重连");
    }

    #[test]
    fn ui_exposes_token_hint_and_task_actions() {
        // 决定 2 选 B：没有托盘，浏览器页面就是唯一界面。可交付的最低条件：
        // ① 提示 rpc-secret 在哪填；② 任务增删暂停可用；③ 错误分鉴权/连接两类。
        assert!(APP_JS.contains("rpc-secret"), "必须提示 token 输入位置");
        for method in [
            "aria2.tellActive", "aria2.tellWaiting", "aria2.tellStopped",
            "aria2.addUri", "aria2.pause", "aria2.unpause",
            "aria2.removeDownloadResult",
        ] {
            assert!(APP_JS.contains(method), "缺少任务操作 {method}");
        }
        assert!(APP_JS.contains("读取任务失败"), "读取失败必须可见");
        assert!(APP_JS.contains("添加失败"), "添加失败必须可见");
        assert!(APP_JS.contains("连接已断开"), "断线原因必须可见");
    }

    #[test]
    fn web_ui_does_not_shadow_existing_endpoints() {
        for p in ["/health", "/api/v1/status", "/cli", "/inject", "/jsonrpc", "/announce",
                  "/scrape", "/torrent/abc", "/nat/advertise", "/nat/public",
                  "/openapi.json", "/ws", "/health?x=1"] {
            assert!(!is_web_ui_path(p, true), "Web UI 遮蔽了 {p}");
            assert!(asset(p, true).is_none(), "静态资产遮蔽了 {p}");
        }
        assert!(is_web_ui_path("/", true));
        assert!(is_web_ui_path("/index.html", true));
        assert!(!is_web_ui_path("/", false));
        assert!(asset("/ui/app.js", true).is_some());
        assert!(asset("/ui/style.css", true).is_some());
        assert!(asset("/ui/app.js", false).is_none());
    }
}
