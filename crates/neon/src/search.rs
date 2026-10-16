use scraper::{Html, Selector};
use std::{io::Read, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: Option<String>,
}

pub fn search_duckduckgo(query: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("search query must not be empty".into());
    }
    if query.len() > 1024 {
        return Err("search query exceeds 1 KiB".into());
    }
    if limit == 0 || limit > 20 {
        return Err("search limit must be in 1..=20".into());
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent("NEON/0.1 (DuckDuckGo on-demand search)")
        .build()
        .map_err(|e| format!("cannot create search client: {e}"))?;
    let mut url =
        reqwest::Url::parse("https://html.duckduckgo.com/html/").map_err(|e| e.to_string())?;
    url.query_pairs_mut().append_pair("q", query);
    let response = client
        .get(url)
        .send()
        .map_err(|e| format!("DuckDuckGo request failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("DuckDuckGo returned HTTP {}", response.status()));
    }
    let mut bytes = Vec::new();
    response
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read search results: {e}"))?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("DuckDuckGo response exceeds 2 MiB".into());
    }
    let html = std::str::from_utf8(&bytes).map_err(|_| "DuckDuckGo response is not UTF-8")?;
    parse_results(html, limit)
}

pub fn parse_results(html: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
    if limit == 0 || limit > 20 {
        return Err("search limit must be in 1..=20".into());
    }
    let document = Html::parse_document(html);
    let result_selector = Selector::parse("div.result").map_err(|e| e.to_string())?;
    let link_selector = Selector::parse("a.result__a").map_err(|e| e.to_string())?;
    let snippet_selector = Selector::parse(".result__snippet").map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for result in document.select(&result_selector) {
        let Some(link) = result.select(&link_selector).next() else {
            continue;
        };
        let title = link
            .text()
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let Some(href) = link.value().attr("href") else {
            continue;
        };
        let target = resolve_target(href)?;
        if title.is_empty() || target.is_empty() {
            continue;
        }
        let snippet = result
            .select(&snippet_selector)
            .next()
            .map(|n| {
                n.text()
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .filter(|s| !s.is_empty());
        out.push(SearchResult {
            title,
            url: target,
            snippet,
        });
        if out.len() >= limit {
            break;
        }
    }
    Ok(out)
}
fn resolve_target(href: &str) -> Result<String, String> {
    let candidate = if href.starts_with("//") {
        format!("https:{href}")
    } else {
        href.to_string()
    };
    let parsed = reqwest::Url::parse(&candidate).map_err(|_| "invalid result URL".to_string())?;
    if parsed.host_str() == Some("duckduckgo.com") && parsed.path() == "/l/" {
        let target = parsed
            .query_pairs()
            .find(|(k, _)| k == "uddg")
            .map(|(_, v)| v.into_owned())
            .ok_or("DuckDuckGo redirect omitted destination")?;
        let decoded = reqwest::Url::parse(&target)
            .map_err(|_| "invalid DuckDuckGo destination".to_string())?;
        if !matches!(decoded.scheme(), "http" | "https") {
            return Err("search result URL must use HTTP or HTTPS".into());
        }
        return Ok(decoded.to_string());
    }
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("search result URL must use HTTP or HTTPS".into());
    }
    Ok(parsed.to_string())
}
