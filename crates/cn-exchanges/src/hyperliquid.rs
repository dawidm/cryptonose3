//! Hyperliquid, an on-chain order book DEX. Perpetuals use the quote "PERP", spot markets their
//! quote token (mostly USDC). Live prices come from the `allMids` subscription, which streams mid
//! prices of every market on each block.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::bail;
use async_trait::async_trait;
use cn_core::{now_ms, Candle, Exchange, ExchangeKind, ExchangeMeta, FeedEvent, FeedKind, IntervalSec, MarketInfo, Pair, Tick, Ts};
use futures::stream::BoxStream;
use serde_json::{json, Value};
use std::time::Duration;

use crate::util::{int, num, post_json};
use crate::ws::{ws_feed, Decoded, Frame, WsDecoder, WsProtocol};

const INFO: &str = "https://api.hyperliquid.xyz/info";
pub const PERP: &str = "PERP";

static META: ExchangeMeta = ExchangeMeta {
    id: "hyperliquid",
    name: "Hyperliquid",
    color: "#50d2c1",
    kind: ExchangeKind::Dex,
    feed: FeedKind::Streaming,
    default_top: &[(PERP, 20), ("USDC", 20)],
};

pub struct Hyperliquid;

#[async_trait]
impl Exchange for Hyperliquid {
    fn meta(&self) -> &ExchangeMeta {
        &META
    }

    fn market_url(&self, pair: &Pair) -> String {
        if &*pair.quote == PERP {
            format!("https://app.hyperliquid.xyz/trade/{}", pair.symbol)
        } else {
            format!("https://app.hyperliquid.xyz/trade/{}/{}", pair.base, pair.quote)
        }
    }

    async fn markets(&self) -> anyhow::Result<Vec<MarketInfo>> {
        let perps: Value = post_json(INFO, &json!({"type": "metaAndAssetCtxs"})).await?;
        let spot: Value = post_json(INFO, &json!({"type": "spotMetaAndAssetCtxs"})).await?;
        let mut m = parse_perps(&perps)?;
        m.extend(parse_spot(&spot)?);
        Ok(m)
    }

    async fn candles(&self, pair: &Pair, interval: IntervalSec, from: Ts, to: Ts) -> anyhow::Result<Vec<Candle>> {
        let iv = match interval {
            60 => "1m",
            300 => "5m",
            _ => bail!("unsupported interval {interval}"),
        };
        let v: Value = post_json(
            INFO,
            &json!({"type": "candleSnapshot", "req": {"coin": &*pair.symbol, "interval": iv, "startTime": from, "endTime": to - 1}}),
        )
        .await?;
        parse_candles(&v)
    }

    fn rest_delay_ms(&self) -> u64 {
        // info requests weigh 20 of the 1200/min budget
        1100
    }

    fn feed(&self, pairs: Vec<Pair>) -> BoxStream<'static, FeedEvent> {
        ws_feed(META.id, Arc::new(Proto), pairs)
    }
}

fn parse_perps(v: &Value) -> anyhow::Result<Vec<MarketInfo>> {
    let (Some(universe), Some(ctxs)) = (v[0]["universe"].as_array(), v[1].as_array()) else {
        bail!("unexpected metaAndAssetCtxs response")
    };
    Ok(universe
        .iter()
        .zip(ctxs)
        .filter(|(u, _)| u["isDelisted"] != true)
        .filter_map(|(u, c)| {
            let name = u["name"].as_str()?;
            Some(MarketInfo {
                pair: Pair::new(name, name, PERP),
                quote_volume_24h: num(&c["dayNtlVlm"]).unwrap_or(0.0),
                last_price: num(&c["midPx"]).or_else(|| num(&c["markPx"])),
            })
        })
        .collect())
}

fn parse_spot(v: &Value) -> anyhow::Result<Vec<MarketInfo>> {
    let (Some(tokens), Some(universe), Some(ctxs)) = (v[0]["tokens"].as_array(), v[0]["universe"].as_array(), v[1].as_array()) else {
        bail!("unexpected spotMetaAndAssetCtxs response")
    };
    let token_names: HashMap<i64, &str> = tokens.iter().filter_map(|t| Some((int(&t["index"])?, t["name"].as_str()?))).collect();
    let ctx_by_coin: HashMap<&str, &Value> = ctxs.iter().filter_map(|c| Some((c["coin"].as_str()?, c))).collect();
    Ok(universe
        .iter()
        .filter_map(|u| {
            let coin = u["name"].as_str()?;
            let t = u["tokens"].as_array()?;
            let base = token_names.get(&int(t.first()?)?)?;
            let quote = token_names.get(&int(t.get(1)?)?)?;
            let ctx = ctx_by_coin.get(coin)?;
            Some(MarketInfo {
                pair: Pair::new(coin, *base, *quote),
                quote_volume_24h: num(&ctx["dayNtlVlm"]).unwrap_or(0.0),
                last_price: num(&ctx["midPx"]).or_else(|| num(&ctx["markPx"])),
            })
        })
        .collect())
}

fn parse_candles(v: &Value) -> anyhow::Result<Vec<Candle>> {
    let Some(rows) = v.as_array() else { bail!("unexpected candleSnapshot response: {v}") };
    Ok(rows
        .iter()
        .filter_map(|r| {
            Some(Candle { ts: int(&r["t"])?, open: num(&r["o"])?, high: num(&r["h"])?, low: num(&r["l"])?, close: num(&r["c"])? })
        })
        .collect())
}

struct Proto;

#[async_trait]
impl WsProtocol for Proto {
    async fn url(&self) -> anyhow::Result<String> {
        Ok("wss://api.hyperliquid.xyz/ws".into())
    }

    fn pairs_per_connection(&self) -> usize {
        usize::MAX
    }

    fn subscribe_msgs(&self, _pairs: &[Pair]) -> Vec<String> {
        vec![json!({"method": "subscribe", "subscription": {"type": "allMids"}}).to_string()]
    }

    fn client_ping(&self) -> Option<(Duration, String)> {
        Some((Duration::from_secs(30), json!({"method": "ping"}).to_string()))
    }

    fn decoder(&self, pairs: &[Pair]) -> Box<dyn WsDecoder> {
        Box::new(Decoder {
            symbols: pairs.iter().map(|p| (p.symbol.to_string(), p.symbol.clone())).collect(),
            last: HashMap::new(),
        })
    }
}

struct Decoder {
    symbols: HashMap<String, Arc<str>>,
    last: HashMap<Arc<str>, f64>,
}

impl WsDecoder for Decoder {
    fn decode(&mut self, frame: Frame<'_>) -> anyhow::Result<Decoded> {
        let Frame::Text(t) = frame else { return Ok(Decoded::default()) };
        let v: Value = serde_json::from_str(t)?;
        if v["channel"] != "allMids" {
            return Ok(Decoded::default());
        }
        let Some(mids) = v["data"]["mids"].as_object() else { return Ok(Decoded::default()) };
        // the feed has no timestamps; every message is a snapshot of the current block
        let ts = now_ms();
        let mut ticks = Vec::new();
        for (coin, px) in mids {
            let (Some(symbol), Some(price)) = (self.symbols.get(coin), num(px)) else { continue };
            if self.last.get(symbol) != Some(&price) {
                self.last.insert(symbol.clone(), price);
                ticks.push(Tick { symbol: symbol.clone(), price, ts });
            }
        }
        Ok(Decoded::ticks(ticks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markets() {
        let perps = json!([
            {"universe": [{"name": "BTC", "szDecimals": 5}, {"name": "OLD", "isDelisted": true}]},
            [{"dayNtlVlm": "2602937690.05", "midPx": "82500.5", "markPx": "82497.2"}, {"dayNtlVlm": "0", "markPx": "1"}]
        ]);
        let m = parse_perps(&perps).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].pair, Pair::new("BTC", "BTC", PERP));
        assert_eq!(m[0].last_price, Some(82500.5));

        let spot = json!([
            {"tokens": [{"name": "USDC", "index": 0}, {"name": "PURR", "index": 1}, {"name": "HYPE", "index": 150}],
             "universe": [{"tokens": [1, 0], "name": "PURR/USDC", "index": 0}, {"tokens": [150, 0], "name": "@107", "index": 107}]},
            [{"coin": "PURR/USDC", "dayNtlVlm": "965504.7", "midPx": "0.0968"}, {"coin": "@107", "dayNtlVlm": "1000", "midPx": "40.1"}]
        ]);
        let m = parse_spot(&spot).unwrap();
        assert_eq!(m[1].pair, Pair::new("@107", "HYPE", "USDC"));
        assert_eq!(m[0].quote_volume_24h, 965504.7);
    }

    #[test]
    fn candles() {
        let v = json!([{"t": 1792521000000i64, "T": 1792521299999i64, "s": "BTC", "i": "5m", "o": "82520.0", "c": "82504.0", "h": "82530.0", "l": "82457.0", "v": "42.7", "n": 622}]);
        assert_eq!(parse_candles(&v).unwrap(), vec![Candle { ts: 1792521000000, open: 82520.0, high: 82530.0, low: 82457.0, close: 82504.0 }]);
    }

    #[test]
    fn decode_all_mids_only_changes() {
        let mut d = Proto.decoder(&[Pair::new("BTC", "BTC", PERP), Pair::new("@107", "HYPE", "USDC")]);
        let msg = r##"{"channel":"allMids","data":{"mids":{"BTC":"82500.5","@107":"40.1","ETH":"2400","#1":"0.5"}}}"##;
        let mut t = d.decode(Frame::Text(msg)).unwrap().ticks;
        t.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        assert_eq!(t.iter().map(|t| (&*t.symbol, t.price)).collect::<Vec<_>>(), vec![("@107", 40.1), ("BTC", 82500.5)]);
        let msg = r#"{"channel":"allMids","data":{"mids":{"BTC":"82501","@107":"40.1"}}}"#;
        let t = d.decode(Frame::Text(msg)).unwrap().ticks;
        assert_eq!(t.iter().map(|t| (&*t.symbol, t.price)).collect::<Vec<_>>(), vec![("BTC", 82501.0)]);
        assert!(d.decode(Frame::Text(r#"{"channel":"pong"}"#)).unwrap().ticks.is_empty());
    }
}
