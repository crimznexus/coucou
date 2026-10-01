// Web tools for the local model (Ollama): search the web and read a page.
// Read-only — no files, no commands.

use std::time::Duration;

use serde_json::{json, Value};

const MAX_OUT: usize = 8000;
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Coucou/0.1";

pub fn definitions() -> Value {
    json!([
        {
            "name": "web_search",
            "description": "Search the web. Returns titles, URLs and snippets. Use fetch_url to read a result.",
            "input_schema": { "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] }
        },
        {
            "name": "fetch_url",
            "description": "Download a web page and return its readable text.",
            "input_schema": { "type": "object", "properties": { "url": { "type": "string" } }, "required": ["url"] }
        }
    ])
}

pub async fn run(name: &str, input: &Value) -> String {
    let arg = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let out = match name {
        "web_search" => web_search(&arg("query")).await,
        "fetch_url" => fetch_url(&arg("url")).await,
        other => Err(format!("Unknown tool: {other}")),
    };
    let text = out.unwrap_or_else(|e| format!("Error: {e}"));
    truncate(text)
}

fn truncate(s: String) -> String {
    if s.chars().count() <= MAX_OUT {
        return s;
    }
    let cut: String = s.chars().take(MAX_OUT).collect();
    format!("{cut}\n…[truncated]")
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())
}

async fn web_search(query: &str) -> Result<String, String> {
    if query.trim().is_empty() {
        return Err("empty query".into());
    }
    let html = client()?
        .post("https://html.duckduckgo.com/html/")
        .form(&[("q", query)])
        .send()
        .await
        .map_err(|e| format!("search failed: {e}"))?
        .text()
        .await
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    let mut rest = html.as_str();
    while let Some(i) = rest.find("class=\"result__a\"") {
        rest = &rest[i..];
        let href = between(rest, "href=\"", "\"").unwrap_or_default();
        let title = between(rest, ">", "</a>").map(strip_tags).unwrap_or_default();
        let snippet = rest
            .find("class=\"result__snippet\"")
            .and_then(|j| between(&rest[j..], ">", "</a>"))
            .map(strip_tags)
            .unwrap_or_default();
        results.push(format!("{title}\n{}\n{snippet}", real_url(&href)));
        rest = &rest["class=\"result__a\"".len()..];
        if results.len() >= 6 {
            break;
        }
    }
    if results.is_empty() {
        return Err("no results (the search engine may be rate-limiting; try again later)".into());
    }
    Ok(results.join("\n\n"))
}

/// DuckDuckGo wraps links as //duckduckgo.com/l/?uddg=<percent-encoded url>.
fn real_url(href: &str) -> String {
    match href.split("uddg=").nth(1) {
        Some(enc) => percent_decode(enc.split('&').next().unwrap_or(enc)),
        None => href.to_string(),
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() + 0 && i + 2 <= b.len() - 1 {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<String> {
    let a = s.find(start)? + start.len();
    let b = s[a..].find(end)? + a;
    Some(s[a..b].to_string())
}

fn strip_tags(s: String) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

async fn fetch_url(url: &str) -> Result<String, String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("only http(s) URLs".into());
    }
    let body = client()?
        .get(url)
        .send()
        .await
        .map_err(|e| format!("fetch failed: {e}"))?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    // Drop scripts and styles before stripping the rest of the markup.
    let mut cleaned = body;
    for tag in ["script", "style", "noscript"] {
        while let Some(a) = cleaned.to_lowercase().find(&format!("<{tag}")) {
            match cleaned.to_lowercase()[a..].find(&format!("</{tag}>")) {
                Some(b) => cleaned.replace_range(a..a + b + tag.len() + 3, " "),
                None => break,
            }
        }
    }
    Ok(strip_tags(cleaned))
}

