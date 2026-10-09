//! Self-contained HTML export: the viewer with every asset and the report
//! data inlined, so it opens from disk or as a CI artifact without a server.

use super::assets::{self, ASSETS};
use crate::report::Report;
use anyhow::{Context, Result};
use std::path::Path;

/// Render the report as one standalone HTML document.
pub fn render_html(report: &Report) -> Result<String> {
    let index = assets::lookup("/assets/app/index.html").context("missing index.html asset")?;
    let mut html = std::str::from_utf8(index.bytes)
        .context("index.html is not UTF-8")?
        .to_string();

    for a in ASSETS {
        let body = std::str::from_utf8(a.bytes).unwrap_or("");
        if a.mime.starts_with("text/css") {
            let tag = format!(r#"<link rel="stylesheet" href="{}" />"#, a.path);
            html = html.replace(&tag, &format!("<style>\n{body}\n</style>"));
        } else if a.mime == "application/javascript" {
            let tag = format!(r#"<script src="{}"></script>"#, a.path);
            html = html.replace(&tag, &format!("<script>\n{}\n</script>", escape_code(body)));
        }
    }

    let json = serde_json::to_string(report).context("failed to serialise report")?;
    let data = format!(
        "<script>window.SHACL_REPORT = {}; window.SHACL_REPORT_VERSION = \"{}\";</script>\n",
        escape_json(&json),
        env!("CARGO_PKG_VERSION")
    );
    let marker = "<!--REPORT-DATA-->";
    if html.contains(marker) {
        html = html.replacen(marker, &data, 1);
    } else {
        html = html.replacen("</body>", &format!("{data}</body>"), 1);
    }
    Ok(html)
}

/// Write the standalone report to `path`.
pub fn write_html(report: &Report, path: &Path) -> Result<()> {
    let html = render_html(report)?;
    std::fs::write(path, html).with_context(|| format!("failed to write {}", path.display()))
}

/// Make JSON safe to embed inside a `<script>` element. `<\/` is a valid
/// escape inside JSON strings, which is the only place `<` can occur.
fn escape_json(s: &str) -> String {
    s.replace("</", "<\\/").replace("<!--", "<\\u0021--")
}

/// Make JavaScript source safe to embed inside a `<script>` element. Only the
/// sequences the HTML parser reacts to are touched, so regex literals such as
/// `/</g` keep working.
fn escape_code(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail
            .get(..8)
            .is_some_and(|t| t.eq_ignore_ascii_case("</script"))
        {
            out.push_str("<\\/");
            rest = &tail[2..];
        } else {
            out.push('<');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out.replace("<!--", "<\\!--")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_closing_script_tags() {
        assert_eq!(escape_json(r#""a</script>b""#), r#""a<\/script>b""#);
        assert_eq!(escape_code("x = '</SCRIPT>'"), r"x = '<\/SCRIPT>'");
        assert_eq!(escape_code("s.replace(/</g, '')"), "s.replace(/</g, '')");
    }
}
