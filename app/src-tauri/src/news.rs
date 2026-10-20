//! Coin news. Providers implement [`NewsProvider`]; results of all enabled providers are merged.
//!
//! - [`RssNews`]: crypto news sites' RSS/Atom feeds, matched against coin name and ticker.
//! - [`GeckoNews`]: CoinGecko `/news` endpoint, only with a paid API key.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use quick_xml::events::Event;
use quick_xml::Reader;
use regex::Regex;
use serde::Serialize;
use tokio::sync::Mutex;

use crate::gecko::CoinRef;

const RSS_REFRESH: Duration = Duration::from_secs(300);
const KEEP_MS: i64 = 7 * 24 * 3600 * 1000;
const CACHE_TTL: Duration = Duration::from_secs(600);
const MAX_ITEMS: usize = 30;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsItem {
    pub title: String,
    pub url: String,
    pub source: String,
    /// Unix ms.
    pub published_at: i64,
}

#[async_trait]
pub trait NewsProvider: Send + Sync {
    async fn news(&self, coin: &CoinRef) -> anyhow::Result<Vec<NewsItem>>;
}

#[derive(Debug, Clone)]
struct FeedItem {
    item: NewsItem,
    /// Title and summary, for matching.
    text: String,
}

pub struct RssNews {
    feeds: Vec<String>,
    state: Mutex<(Option<Instant>, Vec<FeedItem>)>,
}

impl RssNews {
    pub fn new(feeds: Vec<String>) -> Self {
        Self { feeds, state: Mutex::new((None, vec![])) }
    }

    async fn items(&self) -> Vec<FeedItem> {
        let mut st = self.state.lock().await;
        if st.0.is_none_or(|t| t.elapsed() > RSS_REFRESH) {
            let fetched = futures::future::join_all(self.feeds.iter().map(|u| fetch_feed(u))).await;
            let now = cn_core::now_ms();
            let mut all: Vec<FeedItem> = Vec::new();
            for (url, r) in self.feeds.iter().zip(fetched) {
                match r {
                    Ok(items) => all.extend(items),
                    Err(e) => {
                        tracing::warn!("news feed {url} failed: {e:#}");
                        // keep previous items of a failing feed
                        all.extend(st.1.iter().filter(|i| feed_host(url) == feed_host(&i.item.url)).cloned());
                    }
                }
            }
            all.retain(|i| now - i.item.published_at < KEEP_MS);
            *st = (Some(Instant::now()), all);
        }
        st.1.clone()
    }
}

fn feed_host(url: &str) -> String {
    let host = url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or("");
    host.trim_start_matches("www.").to_string()
}

#[async_trait]
impl NewsProvider for RssNews {
    async fn news(&self, coin: &CoinRef) -> anyhow::Result<Vec<NewsItem>> {
        let matcher = CoinMatcher::new(coin);
        Ok(self.items().await.into_iter().filter(|i| matcher.matches(&i.text)).map(|i| i.item).collect())
    }
}

async fn fetch_feed(url: &str) -> anyhow::Result<Vec<FeedItem>> {
    let body = cn_exchanges::util::http().get(url).send().await?.error_for_status()?.text().await?;
    parse_feed(&body, &feed_host(url))
}

/// Parse RSS 2.0 or Atom.
fn parse_feed(xml: &str, fallback_source: &str) -> anyhow::Result<Vec<FeedItem>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut out = Vec::new();
    let mut source = String::new();
    let mut in_item = false;
    let mut field = String::new();
    let (mut title, mut link, mut summary, mut date) = (String::new(), String::new(), String::new(), String::new());
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e) if matches!(e.local_name().as_ref(), b"item" | b"entry") => {
                in_item = true;
                (title, link, summary, date) = Default::default();
            }
            Event::Start(e) => {
                field = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if in_item && field == "link" {
                    // Atom: <link href="..."/>
                    if let Some(href) = e.try_get_attribute("href")? {
                        link = href.unescape_value()?.into_owned();
                    }
                }
            }
            Event::Empty(e) => {
                if in_item && e.local_name().as_ref() == b"link" && link.is_empty() {
                    if let Some(href) = e.try_get_attribute("href")? {
                        link = href.unescape_value()?.into_owned();
                    }
                }
            }
            Event::Text(t) => append(&mut field, in_item, &t.unescape().unwrap_or_default(), &mut source, &mut title, &mut link, &mut summary, &mut date),
            Event::CData(t) => {
                let s = String::from_utf8_lossy(&t).into_owned();
                append(&mut field, in_item, &s, &mut source, &mut title, &mut link, &mut summary, &mut date)
            }
            Event::End(e) if matches!(e.local_name().as_ref(), b"item" | b"entry") => {
                in_item = false;
                if let (false, false, Some(ts)) = (title.is_empty(), link.is_empty(), parse_date(&date)) {
                    let summary_text = strip_tags(&summary);
                    out.push(FeedItem {
                        text: format!("{title} {summary_text}"),
                        item: NewsItem {
                            title: title.trim().to_string(),
                            url: link.trim().to_string(),
                            source: if source.is_empty() { fallback_source.to_string() } else { source.clone() },
                            published_at: ts,
                        },
                    });
                }
            }
            Event::End(_) => field.clear(),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn append(field: &mut String, in_item: bool, text: &str, source: &mut String, title: &mut String, link: &mut String, summary: &mut String, date: &mut String) {
    match (in_item, field.as_str()) {
        (false, "title") if source.is_empty() => *source = text.trim().to_string(),
        (true, "title") => title.push_str(text),
        (true, "link") => link.push_str(text),
        (true, "description" | "summary" | "content") => summary.push_str(text),
        (true, "pubDate" | "published" | "updated" | "date") if date.is_empty() => date.push_str(text),
        _ => {}
    }
}

fn parse_date(s: &str) -> Option<i64> {
    let s = s.trim();
    chrono::DateTime::parse_from_rfc2822(s)
        .or_else(|_| chrono::DateTime::parse_from_rfc3339(s))
        .ok()
        .map(|d| d.timestamp_millis())
}

fn strip_tags(s: &str) -> String {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"<[^>]*>").unwrap()).replace_all(s, " ").into_owned()
}

/// Tickers that are common words or too short to match on their own.
const AMBIGUOUS: &[&str] = &[
    "ONE", "GAS", "OP", "AI", "ME", "IT", "ON", "BE", "GO", "NOW", "FUN", "KEY", "SUN", "CAT", "DOG", "TOP", "BIG", "HOT", "ALL",
    "ANY", "ARE", "CAN", "FOR", "NEW", "NOT", "OUT", "PEOPLE", "REAL", "SAFE", "TRUE", "WIN", "YES", "BAD", "GOOD", "MOVE", "LINK",
    "OPEN", "JUST", "POL", "MEME", "SEED", "DATA", "FORM", "HIGH", "LOW", "MAX", "PEPE", "SAND", "BOND", "GRASS", "PUMP", "COOKIE",
];

struct CoinMatcher {
    name: Option<Regex>,
    ticker: Option<Regex>,
}

impl CoinMatcher {
    fn new(coin: &CoinRef) -> Self {
        let sym = coin.symbol.to_uppercase();
        let name = coin
            .name
            .as_deref()
            .filter(|n| !n.eq_ignore_ascii_case(&sym) || n.len() > 3)
            .and_then(|n| Regex::new(&format!(r"(?i)\b{}\b", regex::escape(n))).ok());
        let ambiguous = sym.len() <= 2 || AMBIGUOUS.contains(&sym.as_str());
        let ticker = if ambiguous {
            Regex::new(&format!(r"\${}\b", regex::escape(&sym))).ok()
        } else {
            Regex::new(&format!(r"(?:\$|\b){}\b", regex::escape(&sym))).ok()
        };
        Self { name, ticker }
    }

    fn matches(&self, text: &str) -> bool {
        self.name.as_ref().is_some_and(|r| r.is_match(text)) || self.ticker.as_ref().is_some_and(|r| r.is_match(text))
    }
}

pub struct GeckoNews {
    pub pro_key: String,
}

#[async_trait]
impl NewsProvider for GeckoNews {
    async fn news(&self, coin: &CoinRef) -> anyhow::Result<Vec<NewsItem>> {
        let Some(id) = &coin.gecko_id else { return Ok(vec![]) };
        let v: serde_json::Value = cn_exchanges::util::http()
            .get(format!("https://pro-api.coingecko.com/api/v3/news?coin_id={id}&per_page=20&type=news"))
            .header("x-cg-pro-api-key", &self.pro_key)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let items = v.as_array().or_else(|| v["data"].as_array()).cloned().unwrap_or_default();
        Ok(items
            .iter()
            .filter_map(|i| {
                Some(NewsItem {
                    title: i["title"].as_str()?.to_string(),
                    url: i["url"].as_str()?.to_string(),
                    source: i["source_name"].as_str().unwrap_or("CoinGecko").to_string(),
                    published_at: parse_date(i["posted_at"].as_str()?)?,
                })
            })
            .collect())
    }
}

fn normalize_title(t: &str) -> String {
    t.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Merge results, newest first, without duplicates by URL or title.
pub fn merge(lists: Vec<Vec<NewsItem>>) -> Vec<NewsItem> {
    let mut all: Vec<NewsItem> = lists.into_iter().flatten().collect();
    all.sort_by(|a, b| b.published_at.cmp(&a.published_at));
    let mut urls = HashSet::new();
    let mut titles = HashSet::new();
    all.retain(|i| urls.insert(i.url.trim_end_matches('/').to_string()) && titles.insert(normalize_title(&i.title)));
    all.truncate(MAX_ITEMS);
    all
}

pub struct News {
    providers: Vec<Arc<dyn NewsProvider>>,
    cache: Mutex<HashMap<String, (Instant, Vec<NewsItem>)>>,
}

impl News {
    pub fn new(providers: Vec<Arc<dyn NewsProvider>>) -> Self {
        Self { providers, cache: Mutex::new(HashMap::new()) }
    }

    pub async fn news(&self, coin: &CoinRef) -> Vec<NewsItem> {
        let key = coin.symbol.to_uppercase();
        if let Some((t, items)) = self.cache.lock().await.get(&key) {
            if t.elapsed() < CACHE_TTL {
                return items.clone();
            }
        }
        let results = futures::future::join_all(self.providers.iter().map(|p| p.news(coin))).await;
        let lists = results
            .into_iter()
            .filter_map(|r| r.map_err(|e| tracing::warn!("news provider failed: {e:#}")).ok())
            .collect();
        let items = merge(lists);
        self.cache.lock().await.insert(key, (Instant::now(), items.clone()));
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Crypto Daily</title>
        <item><title>Solana ETF approved</title><link>https://ex.com/a</link><description><![CDATA[<p>SOL jumps 10%</p>]]></description><pubDate>Mon, 19 Oct 2026 10:00:00 +0000</pubDate></item>
        <item><title>One more thing about markets</title><link>https://ex.com/b</link><description>Gas fees fall</description><pubDate>Mon, 19 Oct 2026 11:00:00 GMT</pubDate></item>
        <item><title>Harmony $ONE rallies</title><link>https://ex.com/c</link><description>x</description><pubDate>Mon, 19 Oct 2026 12:00:00 GMT</pubDate></item>
        </channel></rss>"#;

    const ATOM: &str = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Atom News</title>
        <entry><title>Bitcoin hits new high</title><link href="https://ex.org/btc"/><updated>2026-10-19T09:00:00Z</updated><summary>BTC above 100k</summary></entry>
        </feed>"#;

    fn coin(symbol: &str, name: Option<&str>) -> CoinRef {
        CoinRef { symbol: symbol.into(), name: name.map(Into::into), gecko_id: None }
    }

    #[test]
    fn parses_rss_and_atom() {
        let items = parse_feed(RSS, "ex.com").unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].item.source, "Crypto Daily");
        assert_eq!(items[0].item.title, "Solana ETF approved");
        assert!(items[0].text.contains("SOL jumps"));
        assert_eq!(items[0].item.published_at, 1792404000000);
        let atom = parse_feed(ATOM, "ex.org").unwrap();
        assert_eq!(atom[0].item.url, "https://ex.org/btc");
        assert_eq!(atom[0].item.source, "Atom News");
    }

    #[test]
    fn matching() {
        let items = parse_feed(RSS, "ex.com").unwrap();
        let find = |c: CoinRef| -> Vec<String> {
            let m = CoinMatcher::new(&c);
            items.iter().filter(|i| m.matches(&i.text)).map(|i| i.item.url.clone()).collect()
        };
        assert_eq!(find(coin("SOL", Some("Solana"))), vec!["https://ex.com/a"]);
        assert_eq!(find(coin("sol", None)), vec!["https://ex.com/a"]);
        // ambiguous tickers need $TICKER or the coin name
        assert_eq!(find(coin("ONE", Some("Harmony"))), vec!["https://ex.com/c"]);
        assert_eq!(find(coin("ONE", None)), vec!["https://ex.com/c"]);
        assert!(find(coin("GAS", None)).is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn live_default_feeds() {
        for url in crate::settings::DEFAULT_NEWS_FEEDS {
            let items = fetch_feed(url).await.unwrap_or_else(|e| panic!("{url}: {e:#}"));
            assert!(!items.is_empty(), "{url}: no items");
            println!("{url}: {} items, newest: {}", items.len(), items[0].item.title);
        }
    }

    #[test]
    fn merge_dedupes_and_sorts() {
        let n = |t: &str, u: &str, ts: i64| NewsItem { title: t.into(), url: u.into(), source: "s".into(), published_at: ts };
        let merged = merge(vec![vec![n("A b", "u1", 1), n("C", "u2", 3)], vec![n("a B!", "u3", 2), n("D", "u2/", 4)]]);
        assert_eq!(merged.iter().map(|i| i.url.as_str()).collect::<Vec<_>>(), vec!["u2/", "u3"]);
    }
}
