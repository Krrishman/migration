//! Export captured bookmarks to the browser-neutral Netscape bookmarks HTML
//! format, which every major browser can import. This is the safest browser
//! restore path: it never touches an existing browser profile.

use crate::error::{AppError, AppResult};
use crate::reporting::html::esc;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Folder { title: String, children: Vec<Node> },
    Link { title: String, url: String, add_date: Option<i64> },
    Separator,
}

/// Only plain web/file links are exported; script and data URLs are dropped.
fn allowed_url(url: &str) -> bool {
    let u = url.trim_start().to_ascii_lowercase();
    ["http://", "https://", "ftp://", "file://", "edge://", "chrome://", "about:"].iter().any(|p| u.starts_with(p))
}

fn chromium_node(v: &serde_json::Value) -> Option<Node> {
    let title = v.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
    match v.get("type").and_then(|t| t.as_str()) {
        Some("folder") => Some(Node::Folder {
            title,
            children: v.get("children").and_then(|c| c.as_array()).map(|a| a.iter().filter_map(chromium_node).collect()).unwrap_or_default(),
        }),
        Some("url") => {
            let url = v.get("url").and_then(|u| u.as_str())?.to_string();
            // Chromium stores microseconds since 1601-01-01.
            let add_date = v
                .get("date_added")
                .and_then(|d| d.as_str())
                .and_then(|d| d.parse::<i64>().ok())
                .map(|us| us / 1_000_000 - 11_644_473_600)
                .filter(|s| *s > 0);
            Some(Node::Link { title, url, add_date })
        }
        _ => None,
    }
}

/// Parse a Chromium `Bookmarks` JSON file into folders.
pub fn parse_chromium(json: &[u8]) -> AppResult<Vec<Node>> {
    let v: serde_json::Value = serde_json::from_slice(json).map_err(|e| AppError::InvalidRequest(format!("Bookmarks file is not valid JSON: {e}")))?;
    let roots = v.get("roots").ok_or_else(|| AppError::InvalidRequest("Bookmarks file has no roots".into()))?;
    let mut out = Vec::new();
    for key in ["bookmark_bar", "other", "synced"] {
        if let Some(Node::Folder { title, children }) = roots.get(key).and_then(chromium_node) {
            if !children.is_empty() {
                out.push(Node::Folder { title, children });
            }
        }
    }
    Ok(out)
}

/// Read bookmarks from a Firefox `places.sqlite` (opened read-only).
pub fn parse_firefox(places: &Path) -> AppResult<Vec<Node>> {
    use rusqlite::{Connection, OpenFlags};
    let conn = Connection::open_with_flags(places, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    let mut stmt = conn.prepare(
        "SELECT b.id, b.type, b.parent, b.title, p.url, b.dateAdded, b.guid FROM moz_bookmarks b LEFT JOIN moz_places p ON b.fk = p.id ORDER BY b.parent, b.position",
    )?;
    struct Row {
        id: i64,
        kind: i64,
        parent: i64,
        title: String,
        url: Option<String>,
        added: Option<i64>,
        guid: String,
    }
    let rows: Vec<Row> = stmt
        .query_map([], |r| {
            Ok(Row {
                id: r.get(0)?,
                kind: r.get(1)?,
                parent: r.get(2)?,
                title: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                url: r.get(4)?,
                added: r.get(5)?,
                guid: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
            })
        })?
        .collect::<Result<_, _>>()?;
    fn build(parent: i64, rows: &[Row], depth: usize) -> Vec<Node> {
        if depth > 64 {
            return vec![];
        }
        rows.iter()
            .filter(|r| r.parent == parent && r.id != parent)
            .filter_map(|r| match r.kind {
                1 => r.url.clone().map(|url| Node::Link { title: r.title.clone(), url, add_date: r.added.map(|us| us / 1_000_000) }),
                2 => Some(Node::Folder { title: r.title.clone(), children: build(r.id, rows, depth + 1) }),
                3 => Some(Node::Separator),
                _ => None,
            })
            .collect()
    }
    let mut out = Vec::new();
    for (guid, label) in [("toolbar_____", "Bookmarks Toolbar"), ("menu________", "Bookmarks Menu"), ("unfiled_____", "Other Bookmarks"), ("mobile______", "Mobile Bookmarks")] {
        if let Some(root) = rows.iter().find(|r| r.guid == guid) {
            let children = build(root.id, &rows, 0);
            if !children.is_empty() {
                out.push(Node::Folder { title: label.into(), children });
            }
        }
    }
    Ok(out)
}

fn render(nodes: &[Node], depth: usize, out: &mut String, skipped: &mut usize) {
    let pad = "    ".repeat(depth);
    out.push_str(&format!("{pad}<DL><p>\n"));
    for n in nodes {
        match n {
            Node::Folder { title, children } => {
                out.push_str(&format!("{pad}    <DT><H3>{}</H3>\n", esc(title)));
                render(children, depth + 1, out, skipped);
            }
            Node::Link { title, url, add_date } => {
                if !allowed_url(url) {
                    *skipped += 1;
                    continue;
                }
                let date = add_date.map(|d| format!(" ADD_DATE=\"{d}\"")).unwrap_or_default();
                out.push_str(&format!("{pad}    <DT><A HREF=\"{}\"{date}>{}</A>\n", esc(url), esc(title)));
            }
            Node::Separator => out.push_str(&format!("{pad}    <HR>\n")),
        }
    }
    out.push_str(&format!("{pad}</DL><p>\n"));
}

/// Render Netscape bookmark HTML. Returns (html, links_exported, links_skipped).
pub fn to_netscape_html(title: &str, nodes: &[Node]) -> (String, usize, usize) {
    let mut out = String::from(
        "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n<!-- Exported by Migration Assistant. Import via your browser's \"Import bookmarks from HTML file\". -->\n<META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n",
    );
    out.push_str(&format!("<TITLE>{0}</TITLE>\n<H1>{0}</H1>\n", esc(title)));
    let mut skipped = 0;
    render(nodes, 0, &mut out, &mut skipped);
    fn count(n: &[Node]) -> usize {
        n.iter().map(|x| match x {
            Node::Folder { children, .. } => count(children),
            Node::Link { .. } => 1,
            Node::Separator => 0,
        }).sum()
    }
    let total = count(nodes);
    (out, total - skipped, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chromium_to_html_escapes_and_filters() {
        let json = br#"{"roots":{"bookmark_bar":{"type":"folder","name":"Bookmarks bar","children":[
            {"type":"url","name":"A <b>","url":"https://a.example/?x=1&y=2","date_added":"13300000000000000"},
            {"type":"url","name":"evil","url":"javascript:alert(1)"},
            {"type":"folder","name":"Work","children":[{"type":"url","name":"W","url":"https://w.example/"}]}]},
            "other":{"type":"folder","name":"Other","children":[]}}}"#;
        let nodes = parse_chromium(json).unwrap();
        let (html, ok, skipped) = to_netscape_html("Bookmarks", &nodes);
        assert_eq!((ok, skipped), (2, 1));
        assert!(html.contains("A &lt;b&gt;"));
        assert!(html.contains("https://a.example/?x=1&amp;y=2"));
        assert!(!html.contains("javascript:"));
        assert!(html.contains("<H3>Work</H3>"));
        assert!(html.contains("ADD_DATE=\""));
    }

    #[test]
    fn rejects_invalid_chromium_json() {
        assert!(parse_chromium(b"{}").is_err());
        assert!(parse_chromium(b"nope").is_err());
    }
}
