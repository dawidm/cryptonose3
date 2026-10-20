//! Kraken. Pairs use the websocket v2 names ("BTC/USD"), which the REST API accepts too; REST
//! metadata still uses legacy codes (XBT, XDG) that are mapped.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;
use async_trait::async_trait;
use cn_core::{Candle, Exchange, ExchangeKind, ExchangeMeta, FeedEvent, FeedKind, IntervalSec, MarketInfo, Pair, Tick, Ts};
use futures::stream::BoxStream;
use serde_json::{json, Value};

use crate::util::{get_json, int, num, rfc3339_ms};
use crate::ws::{ws_feed, Decoded, Frame, WsDecoder, WsProtocol};

const API: &str = "https://api.kraken.com/0/public";

static META: ExchangeMeta = ExchangeMeta {
    id: "kraken",
    name: "Kraken",
    color: "#7132f5",
    kind: ExchangeKind::Cex,
    feed: FeedKind::Streaming,
    default_top: &[("USD", 20), ("EUR", 10)],
};

pub struct Kraken;

#[async_trait]
impl Exchange for Kraken {
    fn meta(&self) -> &ExchangeMeta {
        &META
    }

    fn market_url(&self, pair: &Pair) -> String {
        format!("https://pro.kraken.com/app/trade/{}-{}", pair.base.to_lowercase(), pair.quote.to_lowercase())
    }

    async fn markets(&self) -> anyhow::Result<Vec<MarketInfo>> {
        let pairs: Value = get_json(&format!("{API}/AssetPairs")).await?;
        let tickers: Value = get_json(&format!("{API}/Ticker")).await?;
        parse_markets(&pairs, &tickers)
    }

    async fn candles(&self, pair: &Pair, interval: IntervalSec, from: Ts, to: Ts) -> anyhow::Result<Vec<Candle>> {
        let iv = match interval {
            60 => 1,
            300 => 5,
            _ => bail!("unsupported interval {interval}"),
        };
        // returns at most the latest 720 candles after `since`
        let v: Value = get_json(&format!("{API}/OHLC?pair={}&interval={iv}&since={}", pair.symbol, from / 1000 - 1)).await?;
        Ok(parse_ohlc(&v)?.into_iter().filter(|c| c.ts >= from && c.ts < to).collect())
    }

    fn rest_delay_ms(&self) -> u64 {
        // public endpoints allow about one call per second
        1100
    }

    fn feed(&self, pairs: Vec<Pair>) -> BoxStream<'static, FeedEvent> {
        ws_feed(META.id, Arc::new(Proto), pairs)
    }
}

fn result(v: &Value) -> anyhow::Result<&Value> {
    if let Some(errors) = v["error"].as_array().filter(|e| !e.is_empty()) {
        bail!("Kraken error: {errors:?}");
    }
    Ok(&v["result"])
}

/// Legacy currency codes to the names used by websocket v2.
fn currency(c: &str) -> &str {
    match c {
        "XBT" => "BTC",
        "XDG" => "DOGE",
        other => other,
    }
}

fn parse_markets(pairs: &Value, tickers: &Value) -> anyhow::Result<Vec<MarketInfo>> {
    let tickers = result(tickers)?;
    let Some(list) = result(pairs)?.as_object() else { bail!("unexpected AssetPairs response") };
    Ok(list
        .iter()
        .filter(|(_, p)| p["status"] == "online")
        .filter_map(|(key, p)| {
            let (b, q) = p["wsname"].as_str()?.split_once('/')?;
            let (base, quote) = (currency(b), currency(q));
            let t = &tickers[key];
            let last = num(&t["c"][0]);
            // 24h base volume times 24h volume weighted average price
            let volume = num(&t["v"][1]).zip(num(&t["p"][1])).map_or(0.0, |(v, p)| v * p);
            Some(MarketInfo { pair: Pair::new(format!("{base}/{quote}"), base, quote), quote_volume_24h: volume, last_price: last })
        })
        .collect())
}

fn parse_ohlc(v: &Value) -> anyhow::Result<Vec<Candle>> {
    // {"result": {"<pair>": [[time, open, high, low, close, vwap, volume, count], ...], "last": n}}
    let Some(obj) = result(v)?.as_object() else { bail!("unexpected OHLC response") };
    let rows = obj.iter().find(|(k, _)| *k != "last").and_then(|(_, r)| r.as_array()).cloned().unwrap_or_default();
    Ok(rows
        .iter()
        .filter_map(|r| {
            Some(Candle { ts: int(&r[0])? * 1000, open: num(&r[1])?, high: num(&r[2])?, low: num(&r[3])?, close: num(&r[4])? })
        })
        .collect())
}

struct Proto;

#[async_trait]
impl WsProtocol for Proto {
    async fn url(&self) -> anyhow::Result<String> {
        Ok("wss://ws.kraken.com/v2".into())
    }

    fn pairs_per_connection(&self) -> usize {
        200
    }

    fn subscribe_msgs(&self, pairs: &[Pair]) -> Vec<String> {
        pairs
            .chunks(100)
            .map(|c| {
                let symbols: Vec<&str> = c.iter().map(|p| &*p.symbol).collect();
                json!({"method": "subscribe", "params": {"channel": "trade", "symbol": symbols, "snapshot": false}}).to_string()
            })
            .collect()
    }

    fn client_ping(&self) -> Option<(Duration, String)> {
        Some((Duration::from_secs(30), json!({"method": "ping"}).to_string()))
    }

    fn decoder(&self, pairs: &[Pair]) -> Box<dyn WsDecoder> {
        Box::new(Decoder { symbols: pairs.iter().map(|p| (p.symbol.to_string(), p.symbol.clone())).collect() })
    }
}

struct Decoder {
    symbols: HashMap<String, Arc<str>>,
}

impl WsDecoder for Decoder {
    fn decode(&mut self, frame: Frame<'_>) -> anyhow::Result<Decoded> {
        let Frame::Text(t) = frame else { return Ok(Decoded::default()) };
        let v: Value = serde_json::from_str(t)?;
        if v["channel"] != "trade" || v["type"] != "update" {
            return Ok(Decoded::default());
        }
        let ticks = v["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| {
                Some(Tick { symbol: self.symbols.get(d["symbol"].as_str()?)?.clone(), price: num(&d["price"])?, ts: rfc3339_ms(d["timestamp"].as_str()?)? })
            })
            .collect();
        Ok(Decoded::ticks(ticks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markets() {
        let pairs = json!({"error": [], "result": {
            "XXBTZUSD": {"altname": "XBTUSD", "wsname": "XBT/USD", "status": "online"},
            "XDGEUR": {"altname": "XDGEUR", "wsname": "XDG/EUR", "status": "online"},
            "OLDUSD": {"altname": "OLDUSD", "wsname": "OLD/USD", "status": "delisted"}
        }});
        let tickers = json!({"error": [], "result": {
            "XXBTZUSD": {"c": ["82500.0", "0.1"], "v": ["100", "200"], "p": ["82000", "82400"]}
        }});
        let mut m = parse_markets(&pairs, &tickers).unwrap();
        m.sort_by(|a, b| a.pair.symbol.cmp(&b.pair.symbol));
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].pair, Pair::new("BTC/USD", "BTC", "USD"));
        assert_eq!(m[0].quote_volume_24h, 200.0 * 82400.0);
        assert_eq!(m[1].pair, Pair::new("DOGE/EUR", "DOGE", "EUR"));
        assert_eq!(m[1].quote_volume_24h, 0.0);
        assert!(parse_markets(&json!({"error": ["EGeneral:Unavailable"]}), &tickers).is_err());
    }

    #[test]
    fn ohlc() {
        let v: Value = serde_json::from_str(r#"{"error":[],"result":{"BTC/USD":[[1792521000,"82452.0","82467.4","82389.1","82445.9","82419.6","22.15",704]],"last":1792521000}}"#).unwrap();
        assert_eq!(parse_ohlc(&v).unwrap(), vec![Candle { ts: 1792521000000, open: 82452.0, high: 82467.4, low: 82389.1, close: 82445.9 }]);
    }

    #[test]
    fn decode_trades() {
        let mut d = Proto.decoder(&[Pair::new("ETH/USD", "ETH", "USD")]);
        let msg = r#"{"channel":"trade","type":"update","data":[{"symbol":"ETH/USD","side":"buy","price":2437.09,"qty":0.002,"ord_type":"limit","trade_id":68059853,"timestamp":"2026-10-20T20:11:22.945993Z"},{"symbol":"ETH/USD","side":"buy","price":2437.31,"qty":0.0294,"ord_type":"limit","trade_id":68059854,"timestamp":"2026-10-20T20:11:22.945993Z"}]}"#;
        let t = d.decode(Frame::Text(msg)).unwrap().ticks;
        assert_eq!(t.len(), 2);
        assert_eq!(t[1], Tick { symbol: "ETH/USD".into(), price: 2437.31, ts: 1792527082945 });
        assert!(d.decode(Frame::Text(r#"{"channel":"heartbeat"}"#)).unwrap().ticks.is_empty());
    }
}
