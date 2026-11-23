//! 最小内嵌 Web UI（FR-19）。
//!
//! 单个静态 HTML 字符串由 Rust 直接发给客户端：不引 CDN、不写盘、不加载任何外部
//! 脚本或样式。默认关闭（`api.web_ui=false`），开启时只占用 `GET /`。

/// 首页 HTML。JS 只调用已有的只读 HTTP 面（`/health`），不伪造任何数据。
pub const INDEX_HTML: &str = r#"<!doctype html>
<html lang="zh">
<head>
<meta charset="utf-8">
<title>ColdRevive status</title>
<style>
body{font:14px/1.5 monospace;background:#111;color:#ddd;margin:2rem}
h1{font-size:1.2rem} .k{color:#7bd} .v{color:#fd8} #e{color:#f77}
button{font:inherit;background:#222;color:#ddd;border:1px solid #555;padding:.3rem .8rem}
</style>
</head>
<body>
<h1>ColdRevive node status</h1>
<p><button id="r" type="button">Refresh</button> <small>reads /health only</small></p>
<pre id="o">loading…</pre><div id="e"></div>
<script>
function esc(s){return String(s).replace(/[&<>]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;'})[c]);}
async function refresh(){
  try{
    const r=await fetch('/health',{cache:'no-store'});
    if(!r.ok) throw new Error('HTTP '+r.status);
    const h=await r.json();
    document.getElementById('o').textContent=JSON.stringify(h,null,2);
    document.getElementById('e').textContent='';
  }catch(err){document.getElementById('e').textContent='read failed: '+esc(err.message);}
}
document.getElementById('r').addEventListener('click',refresh);
refresh();setInterval(refresh,5000);
</script>
</body>
</html>
"#;

pub const CONTENT_TYPE: &str = "text/html; charset=utf-8";

/// 只有精确的 `GET /` 交给 Web UI；任何已存在的端点都不被它遮蔽。
pub fn is_web_ui_path(path: &str, enabled: bool) -> bool {
    enabled && path == "/"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_is_self_contained() {
        assert!(INDEX_HTML.starts_with("<!doctype html>"));
        assert!(INDEX_HTML.contains("/health"));
        for needle in ["http://", "https://", "<script src", "<link ", "@import"] {
            assert!(!INDEX_HTML.contains(needle), "外部资源引用: {needle}");
        }
        assert!(INDEX_HTML.len() < 4096);
    }

    #[test]
    fn web_ui_does_not_shadow_existing_endpoints() {
        for p in ["/health", "/cli", "/inject", "/jsonrpc", "/announce",
                  "/scrape", "/torrent/abc", "/nat/advertise", "/nat/public",
                  "/openapi.json", "/ws", "/health?x=1"] {
            assert!(!is_web_ui_path(p, true), "Web UI 遮蔽了 {p}");
        }
        assert!(is_web_ui_path("/", true));
        assert!(!is_web_ui_path("/", false));
    }
}
