use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;
use async_trait::async_trait;
use cn_core::{Candle, Exchange, ExchangeKind, ExchangeMeta, FeedEvent, FeedKind, IntervalSec, MarketInfo, Pair, Tick, Ts};
use futures::stream::BoxStream;
use serde_json::{json, Value};

use crate::util::{get_json, http, int, num};
use crate::ws::{ws_feed, Decoded, Frame, WsDecoder, WsProtocol};

const API: &str = "https://api.kucoin.com";

static META: ExchangeMeta = ExchangeMeta {
    id: "kucoin",
    name: "KuCoin",
    color: "#1fb886",
    kind: ExchangeKind::Cex,
    feed: FeedKind::Streaming,
    default_top: &[("USDT", 20)],
};

pub struct KuCoin;

#[async_trait]
impl Exchange for KuCoin {
    fn meta(&self) -> &ExchangeMeta {
        &META
    }

    fn market_url(&self, pair: &Pair) -> String {
        format!("https://www.kucoin.com/trade/{}", pair.symbol)
    }

    async fn markets(&self) -> anyhow::Result<Vec<MarketInfo>> {
        let symbols: Value = get_json(&format!("{API}/api/v2/symbols")).await?;
        let tickers: Value = get_json(&format!("{API}/api/v1/market/allTickers")).await?;
        parse_markets(&symbols, &tickers)
    }

    async fn candles(&self, pair: &Pair, interval: IntervalSec, from: Ts, to: Ts) -> anyhow::Result<Vec<Candle>> {
        let t = match interval {
            60 => "1min",
            300 => "5min",
            _ => bail!("unsupported interval {interval}"),
        };
        let v: Value = get_json(&format!(
            "{API}/api/v1/market/candles?type={t}&symbol={}&startAt={}&endAt={}",
            pair.symbol,
            from / 1000,
            (to - 1) / 1000
        ))
        .await?;
        Ok(parse_candles(&v)?.into_iter().filter(|c| c.ts >= from && c.ts < to).collect())
    }

    fn rest_delay_ms(&self) -> u64 {
        200
    }

    fn feed(&self, pairs: Vec<Pair>) -> BoxStream<'static, FeedEvent> {
        ws_feed(META.id, Arc::new(Proto), pairs)
    }
}

fn data(v: &Value) -> anyhow::Result<&Value> {
    if v["code"] != "200000" {
        bail!("KuCoin error: {v}");
    }
    Ok(&v["data"])
}

fn parse_markets(symbols: &Value, tickers: &Value) -> anyhow::Result<Vec<MarketInfo>> {
    let volumes: HashMap<&str, (f64, Option<f64>)> = data(tickers)?["ticker"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| Some((t["symbol"].as_str()?, (num(&t["volValue"]).unwrap_or(0.0), num(&t["last"])))))
        .collect();
    let Some(list) = data(symbols)?.as_array() else { bail!("unexpected symbols response") };
    Ok(list
        .iter()
        .filter(|s| s["enableTrading"] == true)
        .filter_map(|s| {
            let sym = s["symbol"].as_str()?;
            let (vol, last) = volumes.get(sym).copied().unwrap_or((0.0, None));
            Some(MarketInfo {
                pair: Pair::new(sym, s["baseCurrency"].as_str()?, s["quoteCurrency"].as_str()?),
                quote_volume_24h: vol,
                last_price: last,
            })
        })
        .collect())
}

fn parse_candles(v: &Value) -> anyhow::Result<Vec<Candle>> {
    // [time(s), open, close, high, low, volume, turnover], newest first
    let mut out: Vec<Candle> = data(v)?
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            Some(Candle { ts: int(&r[0])? * 1000, open: num(&r[1])?, close: num(&r[2])?, high: num(&r[3])?, low: num(&r[4])? })
        })
        .collect();
    out.sort_by_key(|c| c.ts);
    Ok(out)
}

struct Proto;

#[async_trait]
impl WsProtocol for Proto {
    async fn url(&self) -> anyhow::Result<String> {
        let resp = http().post(format!("{API}/api/v1/bullet-public")).send().await?;
        let v: Value = resp.json().await?;
        let d = data(&v)?;
        let (Some(token), Some(endpoint)) = (d["token"].as_str(), d["instanceServers"][0]["endpoint"].as_str()) else {
            bail!("unexpected bullet-public response: {v}")
        };
        Ok(format!("{endpoint}?token={token}&connectId={}", rand::random::<u64>()))
    }

    fn pairs_per_connection(&self) -> usize {
        300
    }

    fn subscribe_msgs(&self, pairs: &[Pair]) -> Vec<String> {
        pairs
            .chunks(100)
            .enumerate()
            .map(|(i, c)| {
                let symbols: Vec<&str> = c.iter().map(|p| &*p.symbol).collect();
                json!({"id": i + 1, "type": "subscribe", "topic": format!("/market/match:{}", symbols.join(",")), "privateChannel": false, "response": true}).to_string()
            })
            .collect()
    }

    fn client_ping(&self) -> Option<(Duration, String)> {
        Some((Duration::from_secs(18), json!({"id": "ping", "type": "ping"}).to_string()))
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
        if v["type"] != "message" || v["subject"] != "trade.l3match" {
            return Ok(Decoded::default());
        }
        let d = &v["data"];
        let tick = (|| {
            // time is in nanoseconds
            Some(Tick { symbol: self.symbols.get(d["symbol"].as_str()?)?.clone(), price: num(&d["price"])?, ts: int(&d["time"])? / 1_000_000 })
        })();
        Ok(Decoded::ticks(tick.into_iter().collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markets() {
        let symbols = json!({"code": "200000", "data": [
            {"symbol": "AVA-USDT", "baseCurrency": "AVA", "quoteCurrency": "USDT", "enableTrading": true},
            {"symbol": "OFF-USDT", "baseCurrency": "OFF", "quoteCurrency": "USDT", "enableTrading": false}
        ]});
        let tickers = json!({"code": "200000", "data": {"time": 1, "ticker": [{"symbol": "AVA-USDT", "last": "0.5", "volValue": "17107.94"}]}});
        let m = parse_markets(&symbols, &tickers).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].quote_volume_24h, 17107.94);
        assert!(parse_markets(&json!({"code": "400100"}), &tickers).is_err());
    }

    #[test]
    fn candles() {
        let v = json!({"code": "200000", "data": [["1792521900", "82478.9", "82520.6", "82556.3", "82473.7", "1.13", "93238.3"], ["1792521600", "1", "2", "3", "0.5", "1", "1"]]});
        let c = parse_candles(&v).unwrap();
        assert_eq!(c[0].ts, 1792521600000);
        assert_eq!(c[1], Candle { ts: 1792521900000, open: 82478.9, close: 82520.6, high: 82556.3, low: 82473.7 });
    }

    #[test]
    fn decode_match() {
        let mut d = Proto.decoder(&[Pair::new("BTC-USDT", "BTC", "USDT")]);
        let msg = r#"{"type":"message","topic":"/market/match:BTC-USDT","subject":"trade.l3match","data":{"makerOrderId":"a","price":"82500.1","sequence":"1","side":"buy","size":"0.01","symbol":"BTC-USDT","takerOrderId":"b","time":"1792522000123456789","tradeId":"1","type":"match"}}"#;
        assert_eq!(d.decode(Frame::Text(msg)).unwrap().ticks, vec![Tick { symbol: "BTC-USDT".into(), price: 82500.1, ts: 1792522000123 }]);
        assert!(d.decode(Frame::Text(r#"{"id":"x","type":"welcome"}"#)).unwrap().ticks.is_empty());
    }
}
