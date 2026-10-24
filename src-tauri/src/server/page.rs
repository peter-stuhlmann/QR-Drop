//! The phone pages: single HTML files embedded in the binary, completed with
//! the shared CSS/JS and the share's data, served with a strict CSP.

use std::sync::LazyLock;

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use base64::Engine;
use serde::Serialize;
use sha2::{Digest, Sha256};

const COMMON_CSS: &str = include_str!("../../../landing/common.css");
const COMMON_JS: &str = include_str!("../../../landing/common.js");
const CSS_MARKER: &str = "__COMMON_CSS__";
const JS_MARKER: &str = "__COMMON_JS__";
const DATA_MARKER: &str = "__SHARE_DATA__";

/// Download page (computer → phone).
pub(super) static LANDING: LazyLock<Page> =
    LazyLock::new(|| Page::new(include_str!("../../../landing/index.html")));

/// Upload page (phone → computer).
pub(super) static UPLOAD: LazyLock<Page> =
    LazyLock::new(|| Page::new(include_str!("../../../landing/upload.html")));

pub(super) struct Page {
    /// HTML with the shared CSS/JS inserted; only the data is still missing.
    template: String,
    /// Allows exactly the inline `<style>`/`<script>` blocks of this page.
    csp: HeaderValue,
}

impl Page {
    fn new(source: &str) -> Self {
        let template = source
            .replace(CSS_MARKER, COMMON_CSS)
            .replace(JS_MARKER, COMMON_JS);
        let csp = content_security_policy(&template);
        Self {
            csp: HeaderValue::from_str(&csp)
                .unwrap_or_else(|_| HeaderValue::from_static("default-src 'none'")),
            template,
        }
    }

    /// The page as HTTP response, see [`html`](Self::html).
    pub fn render<T: Serialize>(&self, status: StatusCode, data: &T) -> Response {
        let mut res = (status, Html(self.html(data))).into_response();
        res.headers_mut()
            .insert(header::CONTENT_SECURITY_POLICY, self.csp.clone());
        res
    }

    /// The page with `data` as JSON in its data block.
    fn html<T: Serialize>(&self, data: &T) -> String {
        let json = serde_json::to_string(data).unwrap_or_else(|_| "{}".to_string());
        // Safe inside <script type="application/json">: no "</script>", and
        // no line separators that older engines treat as line breaks.
        let json = json
            .replace('<', "\\u003c")
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029");
        self.template.replace(DATA_MARKER, &json)
    }
}

/// Builds the CSP: no external resources at all, inline code only by hash.
/// The JSON data block is not executed, so it needs (and gets) no hash --
/// which is why the per-request data does not change the policy.
fn content_security_policy(html: &str) -> String {
    let scripts = source_list(
        inline_blocks(html, "script")
            .into_iter()
            .filter(|(attributes, _)| !attributes.contains("application/json"))
            .map(|(_, body)| body),
    );
    let styles = source_list(
        inline_blocks(html, "style")
            .into_iter()
            .map(|(_, body)| body),
    );
    format!(
        "default-src 'none'; img-src 'self'; connect-src 'self'; style-src {styles}; \
         script-src {scripts}; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
    )
}

/// `'sha256-…'` for each block, `'none'` if there is none.
fn source_list<'a>(bodies: impl Iterator<Item = &'a str>) -> String {
    let hashes: Vec<String> = bodies
        .map(|body| {
            let digest = Sha256::digest(body.as_bytes());
            format!(
                "'sha256-{}'",
                base64::engine::general_purpose::STANDARD.encode(digest)
            )
        })
        .collect();
    if hashes.is_empty() {
        "'none'".to_string()
    } else {
        hashes.join(" ")
    }
}

/// `(attributes, content)` of every `<tag …>content</tag>` block. Enough
/// for our own, known pages; not a general HTML parser.
fn inline_blocks<'a>(html: &'a str, tag: &str) -> Vec<(&'a str, &'a str)> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut blocks = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find(&open) {
        let after_name = &rest[start + open.len()..];
        // `<scripts>` or `<styled>` would be other tags.
        if !after_name.starts_with(['>', ' ', '\t', '\n', '\r']) {
            rest = after_name;
            continue;
        }
        let Some(tag_end) = after_name.find('>') else {
            break;
        };
        let attributes = &after_name[..tag_end];
        let content = &after_name[tag_end + 1..];
        let Some(end) = content.find(&close) else {
            break;
        };
        blocks.push((attributes, &content[..end]));
        rest = &content[end + close.len()..];
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "<html><head><style>body{color:red}</style></head><body>\
        <script id=\"d\" type=\"application/json\">__SHARE_DATA__</script>\
        <script>run()</script></body></html>";

    fn hash(s: &str) -> String {
        format!(
            "'sha256-{}'",
            base64::engine::general_purpose::STANDARD.encode(Sha256::digest(s.as_bytes()))
        )
    }

    #[test]
    fn csp_allows_exactly_the_inline_blocks() {
        let csp = content_security_policy(PAGE);
        assert!(csp.contains(&format!("style-src {};", hash("body{color:red}"))));
        assert!(csp.contains(&format!("script-src {};", hash("run()"))));
        assert!(!csp.contains("unsafe-inline"));
        assert!(csp.contains("frame-ancestors 'none'"));
        assert!(!csp.contains("data:"));
        assert!(content_security_policy("<p>x</p>").contains("script-src 'none'"));
    }

    #[test]
    fn data_cannot_break_out_of_its_block() {
        let page = Page::new(PAGE);
        let html = page.html(&serde_json::json!({ "t": "</script><b>" }));
        assert!(
            html.contains("{\"t\":\"\\u003c/script>\\u003cb>\"}"),
            "{html}"
        );
        assert!(!html.contains("</script><b>"), "{html}");
        let res = page.render(StatusCode::NOT_FOUND, &serde_json::json!({}));
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        assert!(res.headers().contains_key(header::CONTENT_SECURITY_POLICY));
    }

    #[test]
    fn real_pages_are_complete() {
        for page in [&*LANDING, &*UPLOAD] {
            assert!(!page.template.contains(CSS_MARKER));
            assert!(!page.template.contains(JS_MARKER));
            assert!(page.template.contains(DATA_MARKER));
            // Every block is found, so none is blocked by the CSP.
            assert_eq!(
                inline_blocks(&page.template, "script").len(),
                page.template.matches("<script").count()
            );
            assert_eq!(
                inline_blocks(&page.template, "style").len(),
                page.template.matches("<style").count()
            );
            let csp = page.csp.to_str().unwrap();
            assert!(csp.contains("'sha256-"), "{csp}");
            assert!(!csp.contains("unsafe-inline"), "{csp}");
        }
    }
}
