//! Poloniex, API v3.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;
use async_trait::async_trait;
use cn_core::{Candle, Exchange, ExchangeKind, ExchangeMeta, FeedEvent, FeedKind, IntervalSec, MarketInfo, Pair, Tick, Ts};
use futures::stream::BoxStream;
use serde_json::{json, Value};

use crate::util::{get_json, int, num};
use crate::ws::{ws_feed, Decoded, Frame, WsDecoder, WsProtocol};

const API: &str = "https://api.poloniex.com";

static META: ExchangeMeta = ExchangeMeta {
    id: "poloniex",
    name: "Poloniex",
    color: "#00a6a6",
    kind: ExchangeKind::Cex,
    feed: FeedKind::Streaming,
    default_top: &[("USDT", 20)],
};

pub struct Poloniex;

#[async_trait]
impl Exchange for Poloniex {
    fn meta(&self) -> &ExchangeMeta {
        &META
    }

    fn market_url(&self, pair: &Pair) -> String {
        format!("https://poloniex.com/trade/{}?type=spot", pair.symbol)
    }

    async fn markets(&self) -> anyhow::Result<Vec<MarketInfo>> {
        let markets: Value = get_json(&format!("{API}/markets")).await?;
        let tickers: Value = get_json(&format!("{API}/markets/ticker24h")).await?;
        parse_markets(&markets, &tickers)
    }

    async fn candles(&self, pair: &Pair, interval: IntervalSec, from: Ts, to: Ts) -> anyhow::Result<Vec<Candle>> {
        let iv = match interval {
            60 => "MINUTE_1",
            300 => "MINUTE_5",
            _ => bail!("unsupported interval {interval}"),
        };
        let v: Value = get_json(&format!(
            "{API}/markets/{}/candles?interval={iv}&startTime={from}&endTime={}&limit=500",
            pair.symbol,
            to - 1
        ))
        .await?;
        parse_candles(&v)
    }

    fn rest_delay_ms(&self) -> u64 {
        200
    }

    fn feed(&self, pairs: Vec<Pair>) -> BoxStream<'static, FeedEvent> {
        ws_feed(META.id, Arc::new(Proto), pairs)
    }
}

fn parse_markets(markets: &Value, tickers: &Value) -> anyhow::Result<Vec<MarketInfo>> {
    let volumes: HashMap<&str, (f64, Option<f64>)> = tickers
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| Some((t["symbol"].as_str()?, (num(&t["amount"]).unwrap_or(0.0), num(&t["close"])))))
        .collect();
    let Some(list) = markets.as_array() else { bail!("unexpected markets response: {markets}") };
    Ok(list
        .iter()
        .filter(|m| m["state"] == "NORMAL")
        .filter_map(|m| {
            let sym = m["symbol"].as_str()?;
            let (vol, last) = volumes.get(sym).copied().unwrap_or((0.0, None));
            Some(MarketInfo {
                pair: Pair::new(sym, m["baseCurrencyName"].as_str()?, m["quoteCurrencyName"].as_str()?),
                quote_volume_24h: vol,
                last_price: last,
            })
        })
        .collect())
}

fn parse_candles(v: &Value) -> anyhow::Result<Vec<Candle>> {
    // [low, high, open, close, amount, quantity, buyTakerAmount, buyTakerQuantity, tradeCount, ts,
    //  weightedAverage, interval, startTime, closeTime]
    let Some(rows) = v.as_array() else { bail!("unexpected candles response: {v}") };
    let mut out: Vec<Candle> = rows
        .iter()
        .filter_map(|r| {
            Some(Candle { ts: int(&r[12])?, low: num(&r[0])?, high: num(&r[1])?, open: num(&r[2])?, close: num(&r[3])? })
        })
        .collect();
    out.sort_by_key(|c| c.ts);
    Ok(out)
}

struct Proto;

#[async_trait]
impl WsProtocol for Proto {
    async fn url(&self) -> anyhow::Result<String> {
        Ok("wss://ws.poloniex.com/ws/public".into())
    }

    fn pairs_per_connection(&self) -> usize {
        100
    }

    fn subscribe_msgs(&self, pairs: &[Pair]) -> Vec<String> {
        let symbols: Vec<&str> = pairs.iter().map(|p| &*p.symbol).collect();
        vec![json!({"event": "subscribe", "channel": ["trades"], "symbols": symbols}).to_string()]
    }

    fn client_ping(&self) -> Option<(Duration, String)> {
        Some((Duration::from_secs(20), json!({"event": "ping"}).to_string()))
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
        if v["channel"] != "trades" {
            return Ok(Decoded::default());
        }
        let ticks = v["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| {
                Some(Tick { symbol: self.symbols.get(d["symbol"].as_str()?)?.clone(), price: num(&d["price"])?, ts: int(&d["createTime"])? })
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
        let markets = json!([
            {"symbol": "BTC_USDT", "baseCurrencyName": "BTC", "quoteCurrencyName": "USDT", "state": "NORMAL"},
            {"symbol": "X_USDT", "baseCurrencyName": "X", "quoteCurrencyName": "USDT", "state": "PAUSE"}
        ]);
        let tickers = json!([{"symbol": "BTC_USDT", "close": "82500", "amount": "864896.45"}]);
        let m = parse_markets(&markets, &tickers).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].pair, Pair::new("BTC_USDT", "BTC", "USDT"));
        assert_eq!(m[0].quote_volume_24h, 864896.45);
    }

    #[test]
    fn candles() {
        let v: Value = serde_json::from_str(r#"[["82505.78","82625.72","82583.64","82508.41","864896.45","10.47","465270.79","5.63",511,1792521600000,"82567.47","MINUTE_5",1792521600000,1792521899999]]"#).unwrap();
        assert_eq!(parse_candles(&v).unwrap(), vec![Candle { ts: 1792521600000, low: 82505.78, high: 82625.72, open: 82583.64, close: 82508.41 }]);
    }

    #[test]
    fn decode_trades() {
        let mut d = Proto.decoder(&[Pair::new("BTC_USDT", "BTC", "USDT")]);
        let msg = r#"{"channel":"trades","data":[{"symbol":"BTC_USDT","amount":"70","takerSide":"buy","quantity":"0.001","createTime":1792522000000,"price":"82500.5","id":"1","ts":1792522000010}]}"#;
        assert_eq!(d.decode(Frame::Text(msg)).unwrap().ticks, vec![Tick { symbol: "BTC_USDT".into(), price: 82500.5, ts: 1792522000000 }]);
        assert!(d.decode(Frame::Text(r#"{"event":"pong"}"#)).unwrap().ticks.is_empty());
    }
}
