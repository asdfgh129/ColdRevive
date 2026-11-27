//! 最小内嵌 Web UI（FR-19 / W6.1 / W6.4）。
//!
//! HTML/CSS/JS 全部在编译期嵌进二进制，运行时不写盘、不访问 CDN。
//! 默认关闭（`api.web_ui=false`）；开启时只占 `GET /` 与 `GET /ui/*`，不遮蔽任何既有端点。
//! 页面只调用守护进程自己的 HTTP/JSON-RPC 面，所有数字都来自真实 `/health` 与引擎任务。

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
<h1>ColdRevive console</h1>
<p class="hint">只读状态 + aria2 兼容任务控制；所有请求都发往本进程。</p>
<section>
<h2>任务</h2>
<form id="add">
<input id="uri" placeholder="magnet: 或 http(s)://.../*.torrent" size="46">
<input id="token" placeholder="rpc-secret（可选）" size="18" autocomplete="off">
<button type="submit">addUri</button>
</form>
<div id="tasks">loading…</div>
</section>
<section>
<h2>节点状态</h2>
<pre id="status">loading…</pre>
</section>
<div id="err"></div>
<script src="/ui/app.js"></script>
</body>
</html>
"#;

pub const STYLE_CSS: &str = r#"body{font:14px/1.5 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;background:#111;color:#ddd;margin:2rem;max-width:900px}
h1{font-size:1.25rem}h2{font-size:1rem;margin-top:1.5rem;color:#7bd}
.hint{color:#888}input,button{font:inherit;background:#1b1b1b;color:#ddd;border:1px solid #555;padding:.35rem .6rem}
button{cursor:pointer;background:#26333b}#status{background:#181818;border:1px solid #333;padding:.8rem;overflow:auto}
.task{border-bottom:1px solid #333;padding:.5rem 0}.task .gid{color:#7bd}.task .st{color:#fd8}
#err{color:#f77;min-height:1.2rem}"#;

pub const APP_JS: &str = r#""use strict";
const q = id => document.getElementById(id);
function esc(v){return String(v).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));}
function params(){const t=q('token').value.trim();return t?[['token:'+t],[]]:[[]];}
function tokenParam(){const t=q('token').value.trim();return t?'token:'+t:null;}
async function rpc(method,p){
  const body={jsonrpc:'2.0',id:Date.now(),method,params:p};
  const r=await fetch('/jsonrpc',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(body)});
  const j=await r.json();
  if(j.error) throw new Error(j.error.message||('rpc '+j.error.code));
  return j.result;
}
async function refresh(){
  try{
    const r=await fetch('/api/v1/status',{cache:'no-store'});
    if(!r.ok) throw new Error('HTTP '+r.status);
    q('status').textContent=JSON.stringify(await r.json(),null,2);
    const active=await rpc('aria2.tellActive',[[]]);
    const waiting=await rpc('aria2.tellWaiting',[[]]);
    const stopped=await rpc('aria2.tellStopped',[[]]);
    const all=[].concat(active||[],waiting||[],stopped||[]);
    q('tasks').innerHTML=all.length?all.map(t=>{
      const g=t.gid||'',st=t.status||'',n=t.name||t.uri||'';
      return '<div class="task"><span class="gid">'+esc(g)+'</span> <span class="st">'+esc(st)+
        '</span> '+esc(n)+' <button data-act="pause" data-gid="'+esc(g)+'">pause</button>'+
        '<button data-act="resume" data-gid="'+esc(g)+'">resume</button>'+
        '<button data-act="remove" data-gid="'+esc(g)+'">remove</button></div>';
    }).join(''):'<p class="hint">暂无任务</p>';
    q('err').textContent='';
  }catch(e){q('err').textContent='read failed: '+esc(e.message);}
}
q('add').addEventListener('submit',async e=>{
  e.preventDefault();const uri=q('uri').value.trim();if(!uri)return;
  try{const t=tokenParam();await rpc('aria2.addUri',t?[t,[uri]]:[[uri]]);q('uri').value='';await refresh();}
  catch(err){q('err').textContent='addUri failed: '+esc(err.message);}
});
q('tasks').addEventListener('click',async e=>{
  const b=e.target.closest('button');if(!b)return;
  const act=b.dataset.act,g=b.dataset.gid;if(!g)return;
  const m=act==='remove'?'aria2.removeDownloadResult':('aria2.'+act);
  try{const t=tokenParam();await rpc(m,t?[t,g]:[g]);await refresh();}
  catch(err){q('err').textContent=act+' failed: '+esc(err.message);}
});
refresh();setInterval(refresh,5000);
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
        assert!(INDEX_HTML.len() < 4096);
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
