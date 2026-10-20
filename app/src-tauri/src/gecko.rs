//! CoinGecko coin info (free public/Demo API).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::Context;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::Mutex;

const API: &str = "https://api.coingecko.com/api/v3";
const LIST_TTL: Duration = Duration::from_secs(3600);
const INFO_TTL: Duration = Duration::from_secs(300);

/// A coin as known to news providers.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoinRef {
    pub symbol: String,
    pub name: Option<String>,
    pub gecko_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoinInfo {
    pub id: String,
    pub symbol: String,
    pub name: String,
    pub image: Option<String>,
    pub homepage: Option<String>,
    pub gecko_url: String,
    pub price_usd: Option<f64>,
    pub change_24h: Option<f64>,
    pub change_7d: Option<f64>,
    pub market_cap: Option<f64>,
    pub rank: Option<u64>,
    pub volume_24h: Option<f64>,
    pub description: Option<String>,
}

/// Exchange-specific base symbols to the commonly used ticker.
pub fn normalize_symbol(base: &str) -> String {
    // Hyperliquid "kPEPE" = 1000 PEPE, wrapped spot tokens "UBTC", "UETH", "USOL"
    if let Some(rest) = base.strip_prefix('k').filter(|r| r.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())) {
        return rest.to_string();
    }
    match base {
        "UBTC" => "BTC".into(),
        "UETH" => "ETH".into(),
        "USOL" => "SOL".into(),
        "XBT" => "BTC".into(),
        _ => base.to_uppercase(),
    }
}

pub struct Gecko {
    demo_key: Mutex<Option<String>>,
    /// lower-case symbol -> coin ids
    list: Mutex<(Option<Instant>, HashMap<String, Vec<String>>)>,
    resolved: Mutex<HashMap<String, Option<String>>>,
    info: Mutex<HashMap<String, (Instant, CoinInfo)>>,
}

impl Gecko {
    pub fn new(demo_key: Option<String>) -> Self {
        Self {
            demo_key: Mutex::new(demo_key),
            list: Mutex::new((None, HashMap::new())),
            resolved: Mutex::new(HashMap::new()),
            info: Mutex::new(HashMap::new()),
        }
    }

    pub async fn set_demo_key(&self, key: Option<String>) {
        *self.demo_key.lock().await = key.filter(|k| !k.trim().is_empty());
    }

    async fn get(&self, path: &str) -> anyhow::Result<Value> {
        let mut req = cn_exchanges::util::http().get(format!("{API}{path}"));
        if let Some(k) = self.demo_key.lock().await.as_deref() {
            req = req.header("x-cg-demo-api-key", k);
        }
        let resp = req.send().await.with_context(|| format!("CoinGecko {path}"))?;
        if resp.status().as_u16() == 429 {
            anyhow::bail!("CoinGecko rate limit reached, try again in a minute");
        }
        Ok(resp.error_for_status()?.json().await?)
    }

    async fn ids_for(&self, symbol: &str) -> anyhow::Result<Vec<String>> {
        let mut list = self.list.lock().await;
        if list.0.is_none_or(|t| t.elapsed() > LIST_TTL) {
            let v = self.get("/coins/list").await?;
            let mut map: HashMap<String, Vec<String>> = HashMap::new();
            for c in v.as_array().into_iter().flatten() {
                if let (Some(s), Some(id)) = (c["symbol"].as_str(), c["id"].as_str()) {
                    map.entry(s.to_lowercase()).or_default().push(id.to_string());
                }
            }
            *list = (Some(Instant::now()), map);
        }
        Ok(list.1.get(&symbol.to_lowercase()).cloned().unwrap_or_default())
    }

    /// CoinGecko id for a ticker; when several coins share it, the one with the highest volume.
    pub async fn resolve(&self, symbol: &str) -> anyhow::Result<Option<String>> {
        let symbol = normalize_symbol(symbol);
        if let Some(r) = self.resolved.lock().await.get(&symbol) {
            return Ok(r.clone());
        }
        let ids = self.ids_for(&symbol).await?;
        let id = match ids.len() {
            0 => None,
            1 => Some(ids[0].clone()),
            _ => {
                let v = self.get(&format!("/coins/markets?vs_currency=usd&ids={}", ids.join(","))).await?;
                v.as_array()
                    .into_iter()
                    .flatten()
                    .max_by(|a, b| a["total_volume"].as_f64().unwrap_or(0.0).total_cmp(&b["total_volume"].as_f64().unwrap_or(0.0)))
                    .and_then(|c| c["id"].as_str().map(String::from))
            }
        };
        self.resolved.lock().await.insert(symbol, id.clone());
        Ok(id)
    }

    pub async fn info(&self, symbol: &str) -> anyhow::Result<Option<CoinInfo>> {
        let Some(id) = self.resolve(symbol).await? else { return Ok(None) };
        if let Some((t, info)) = self.info.lock().await.get(&id) {
            if t.elapsed() < INFO_TTL {
                return Ok(Some(info.clone()));
            }
        }
        let v = self
            .get(&format!("/coins/{id}?localization=false&tickers=false&community_data=false&developer_data=false&sparkline=false"))
            .await?;
        let info = parse_info(&v).context("unexpected CoinGecko response")?;
        self.info.lock().await.insert(id, (Instant::now(), info.clone()));
        Ok(Some(info))
    }

    pub async fn coin_ref(&self, symbol: &str) -> CoinRef {
        let info = self.info(symbol).await.ok().flatten();
        CoinRef {
            symbol: normalize_symbol(symbol),
            name: info.as_ref().map(|i| i.name.clone()),
            gecko_id: info.map(|i| i.id),
        }
    }
}

fn parse_info(v: &Value) -> Option<CoinInfo> {
    let id = v["id"].as_str()?.to_string();
    let md = &v["market_data"];
    let description = v["description"]["en"].as_str().filter(|d| !d.is_empty()).map(|d| {
        let plain = regex::Regex::new(r"<[^>]*>").unwrap().replace_all(d, "");
        let plain = plain.trim();
        if plain.chars().count() > 800 {
            let cut: String = plain.chars().take(800).collect();
            format!("{}…", cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head))
        } else {
            plain.to_string()
        }
    });
    Some(CoinInfo {
        gecko_url: format!("https://www.coingecko.com/en/coins/{id}"),
        id,
        symbol: v["symbol"].as_str()?.to_uppercase(),
        name: v["name"].as_str()?.to_string(),
        image: v["image"]["small"].as_str().map(String::from),
        homepage: v["links"]["homepage"].as_array().and_then(|a| a.iter().filter_map(Value::as_str).find(|s| !s.is_empty())).map(String::from),
        price_usd: md["current_price"]["usd"].as_f64(),
        change_24h: md["price_change_percentage_24h"].as_f64(),
        change_7d: md["price_change_percentage_7d"].as_f64(),
        market_cap: md["market_cap"]["usd"].as_f64(),
        rank: v["market_cap_rank"].as_u64(),
        volume_24h: md["total_volume"]["usd"].as_f64(),
        description,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols() {
        assert_eq!(normalize_symbol("kPEPE"), "PEPE");
        assert_eq!(normalize_symbol("UBTC"), "BTC");
        assert_eq!(normalize_symbol("eth"), "ETH");
        assert_eq!(normalize_symbol("kava"), "KAVA");
    }

    #[test]
    fn info() {
        let v = serde_json::json!({
            "id": "solana", "symbol": "sol", "name": "Solana", "market_cap_rank": 6,
            "image": {"small": "https://img/sol.png"},
            "links": {"homepage": ["", "https://solana.com"]},
            "description": {"en": "Solana is a <a href=\"x\">blockchain</a>."},
            "market_data": {"current_price": {"usd": 105.9}, "price_change_percentage_24h": 1.5, "price_change_percentage_7d": -3.0,
                            "market_cap": {"usd": 5.0e10}, "total_volume": {"usd": 2.0e9}}
        });
        let i = parse_info(&v).unwrap();
        assert_eq!(i.symbol, "SOL");
        assert_eq!(i.homepage.as_deref(), Some("https://solana.com"));
        assert_eq!(i.description.as_deref(), Some("Solana is a blockchain."));
        assert_eq!(i.rank, Some(6));
    }
}
