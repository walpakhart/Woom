//! `@web:<url>` mention backend — fetch a URL and reduce its HTML to
//! readable, markdown-ish plain text for injection as prompt context.
//!
//! Zero-dependency by design: no html2md/scraper/regex crate (NFR — reuse
//! what's already vendored; only `reqwest` is needed and it's already a dep).
//! The reducer is a byte-scan tag stripper, so NO tags survive into the
//! output — the result is plain text, never rendered as HTML, so there's no
//! script-execution path.

use std::time::Duration;

const MAX_BODY_BYTES: usize = 2 * 1024 * 1024; // 2 MB fetched HTML cap
const MAX_OUTPUT_BYTES: usize = 200 * 1024; // 200 KB markdown cap
const FETCH_TIMEOUT_SECS: u64 = 15;

/// Fetch `url` (http/https only) and return readable markdown-ish text.
pub async fn fetch_markdown(url: &str) -> Result<String, String> {
    let u = url.trim();
    if !(u.starts_with("http://") || u.starts_with("https://")) {
        return Err("only http/https URLs are supported".into());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
        .user_agent("Woom/0.2 (+https://github.com/walpakhart/Woom)")
        .build()
        .map_err(|e| format!("client build failed: {e}"))?;

    let resp = client
        .get(u)
        .send()
        .await
        .map_err(|e| format!("fetch failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }

    // Reject non-text content (PDF/images/binaries) — markdown-only scope.
    if let Some(ct) = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
    {
        let ct = ct.to_ascii_lowercase();
        let is_text = ct.starts_with("text/")
            || ct.contains("html")
            || ct.contains("xml")
            || ct.contains("json");
        if !is_text {
            return Err(format!("unsupported content-type: {ct}"));
        }
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("read body failed: {e}"))?;
    let slice = &bytes[..bytes.len().min(MAX_BODY_BYTES)];
    let html = String::from_utf8_lossy(slice);

    let mut md = html_to_markdown(&html);
    if md.len() > MAX_OUTPUT_BYTES {
        md.truncate(MAX_OUTPUT_BYTES);
        md.push_str("\n… [truncated]");
    }
    Ok(md)
}

/// Reduce HTML to readable text: drop `<script>`/`<style>` content, turn
/// block-level tags into newlines, strip every remaining tag, decode a few
/// common entities, collapse runs of blank lines.
fn html_to_markdown(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len() / 2);
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            // Drop <script>…</script> and <style>…</style> wholesale.
            if lower[i..].starts_with("<script") {
                if let Some(end) = lower[i..].find("</script>") {
                    i += end + "</script>".len();
                    continue;
                }
                break;
            }
            if lower[i..].starts_with("<style") {
                if let Some(end) = lower[i..].find("</style>") {
                    i += end + "</style>".len();
                    continue;
                }
                break;
            }
            // Find tag end.
            let Some(rel) = html[i..].find('>') else { break };
            let tag = &lower[i..i + rel + 1];
            // Block-level boundaries → newline.
            let is_break = tag.starts_with("</p")
                || tag.starts_with("<br")
                || tag.starts_with("</div")
                || tag.starts_with("</li")
                || tag.starts_with("</tr")
                || tag.starts_with("</h1")
                || tag.starts_with("</h2")
                || tag.starts_with("</h3")
                || tag.starts_with("</h4")
                || tag.starts_with("</h5")
                || tag.starts_with("</h6")
                || tag.starts_with("<hr");
            if is_break {
                out.push('\n');
            }
            i += rel + 1;
            continue;
        }
        // Copy a UTF-8 char verbatim.
        let ch_len = utf8_len(bytes[i]);
        let end = (i + ch_len).min(bytes.len());
        out.push_str(&html[i..end]);
        i = end;
    }
    let decoded = decode_entities(&out);
    collapse_blank_lines(&decoded)
}

fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b & 0b1110_0000 == 0b1100_0000 {
        2
    } else if b & 0b1111_0000 == 0b1110_0000 {
        3
    } else if b & 0b1111_1000 == 0b1111_0000 {
        4
    } else {
        1
    }
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank_run = 0usize;
    for line in s.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 2 {
                out.push('\n');
            }
        } else {
            blank_run = 0;
            out.push_str(trimmed);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_to_markdown_strips_tags_and_scripts() {
        let html = "<html><head><style>.x{color:red}</style></head>\
            <body><h1>Title</h1><script>alert('xss')</script>\
            <p>Hello &amp; welcome</p><p>Second line</p></body></html>";
        let md = html_to_markdown(html);
        assert!(!md.contains("alert"), "script content survived: {md:?}");
        assert!(!md.contains("color:red"), "style content survived: {md:?}");
        assert!(!md.contains('<'), "a tag survived: {md:?}");
        assert!(md.contains("Title"), "text dropped: {md:?}");
        assert!(md.contains("Hello & welcome"), "entity not decoded: {md:?}");
        assert!(md.contains("Second line"));
    }
}
