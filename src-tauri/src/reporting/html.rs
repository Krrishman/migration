//! Minimal, dependency-free HTML builder for self-contained reports.
//! Every dynamic value goes through [`esc`]; reports contain no scripts and
//! no external resources, so they open safely offline in any browser.

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

const STYLE: &str = r#"
:root{--bg:#f7f9fb;--card:#fff;--text:#14212b;--muted:#5b6b78;--line:#dde4ea;--accent:#0f6cbd;--ok:#0e7a4f;--warn:#9a5b00;--err:#b42318;--chip:#eef3f8}
@media (prefers-color-scheme:dark){:root{--bg:#0f1720;--card:#16202b;--text:#e6edf3;--muted:#9fb0c0;--line:#2a3743;--accent:#5aa9f0;--ok:#3fbf87;--warn:#e0a54a;--err:#ff7b72;--chip:#1e2b38}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--text);font:14px/1.5 "Segoe UI",system-ui,sans-serif}
main{max-width:1100px;margin:0 auto;padding:24px 16px 64px}h1{font-size:24px;margin:0 0 4px}h2{font-size:18px;margin:32px 0 8px;border-bottom:1px solid var(--line);padding-bottom:4px}
.muted{color:var(--muted)}.card{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:16px;margin:12px 0}
table{width:100%;border-collapse:collapse;background:var(--card);border:1px solid var(--line);border-radius:8px;overflow:hidden;font-size:13px}
th,td{text-align:left;padding:6px 10px;border-bottom:1px solid var(--line);vertical-align:top;word-break:break-word}th{background:var(--chip);font-weight:600}
.chip{display:inline-block;padding:1px 8px;border-radius:999px;background:var(--chip);font-size:12px;margin:1px 2px}
.ok{color:var(--ok)}.warn{color:var(--warn)}.err{color:var(--err)}code{font-family:Consolas,monospace;font-size:12px}
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(200px,1fr));gap:8px}.kv{margin:0}.kv dt{color:var(--muted);font-size:12px}.kv dd{margin:0 0 8px;font-weight:600}
@media print{body{background:#fff;color:#000}.card,table{border-color:#bbb}}
"#;

pub struct Html {
    buf: String,
}

impl Html {
    pub fn page(title: &str) -> Self {
        let mut buf = String::new();
        buf.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">");
        buf.push_str("<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src data:\">");
        buf.push_str(&format!("<title>{}</title><style>{STYLE}</style></head><body><main>", esc(title)));
        Self { buf }
    }
    pub fn raw(&mut self, s: &str) -> &mut Self {
        self.buf.push_str(s);
        self
    }
    pub fn h1(&mut self, s: &str) -> &mut Self {
        self.raw(&format!("<h1>{}</h1>", esc(s)))
    }
    pub fn h2(&mut self, s: &str) -> &mut Self {
        self.raw(&format!("<h2>{}</h2>", esc(s)))
    }
    pub fn p(&mut self, s: &str) -> &mut Self {
        self.raw(&format!("<p>{}</p>", esc(s)))
    }
    pub fn muted(&mut self, s: &str) -> &mut Self {
        self.raw(&format!("<p class=\"muted\">{}</p>", esc(s)))
    }
    pub fn kv(&mut self, pairs: &[(&str, String)]) -> &mut Self {
        self.raw("<div class=\"card\"><dl class=\"kv grid\">");
        for (k, v) in pairs {
            self.raw(&format!("<div><dt>{}</dt><dd>{}</dd></div>", esc(k), esc(v)));
        }
        self.raw("</dl></div>")
    }
    pub fn table(&mut self, headers: &[&str], rows: &[Vec<String>]) -> &mut Self {
        if rows.is_empty() {
            return self.muted("None.");
        }
        self.raw("<table><thead><tr>");
        for h in headers {
            self.raw(&format!("<th>{}</th>", esc(h)));
        }
        self.raw("</tr></thead><tbody>");
        for r in rows {
            self.raw("<tr>");
            for c in r {
                self.raw(&format!("<td>{}</td>", esc(c)));
            }
            self.raw("</tr>");
        }
        self.raw("</tbody></table>")
    }
    pub fn list(&mut self, items: &[String]) -> &mut Self {
        if items.is_empty() {
            return self.muted("None.");
        }
        self.raw("<ul>");
        for i in items {
            self.raw(&format!("<li>{}</li>", esc(i)));
        }
        self.raw("</ul>")
    }
    pub fn details(&mut self, summary: &str, inner: impl FnOnce(&mut Self)) -> &mut Self {
        self.raw(&format!("<details class=\"card\"><summary>{}</summary>", esc(summary)));
        inner(self);
        self.raw("</details>")
    }
    pub fn finish(mut self, footer: &str) -> String {
        self.buf.push_str(&format!("<p class=\"muted\" style=\"margin-top:40px\">{}</p></main></body></html>", esc(footer)));
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_markup() {
        assert_eq!(esc("<script>alert('x')</script>&"), "&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;&amp;");
        let mut h = Html::page("t");
        h.p("<b>");
        let out = h.finish("f");
        assert!(out.contains("&lt;b&gt;"));
        assert!(!out.contains("<script"));
    }
}
