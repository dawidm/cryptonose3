use std::collections::HashMap;
use std::sync::Arc;

use anyhow::bail;
use async_trait::async_trait;
use cn_core::{Candle, Exchange, ExchangeKind, ExchangeMeta, FeedEvent, FeedKind, IntervalSec, MarketInfo, Pair, Tick, Ts};
use futures::stream::BoxStream;
use serde_json::{json, Value};

use crate::util::{get_json, int, num};
use crate::ws::{ws_feed, Decoded, Frame, WsDecoder, WsProtocol};

const API: &str = "https://api.binance.com";

static META: ExchangeMeta = ExchangeMeta {
    id: "binance",
    name: "Binance",
    color: "#e0a800",
    kind: ExchangeKind::Cex,
    feed: FeedKind::Streaming,
    default_top: &[("USDT", 30), ("USDC", 10)],
};

pub struct Binance;

#[async_trait]
impl Exchange for Binance {
    fn meta(&self) -> &ExchangeMeta {
        &META
    }

    fn market_url(&self, pair: &Pair) -> String {
        format!("https://www.binance.com/en/trade/{}_{}", pair.base, pair.quote)
    }

    async fn markets(&self) -> anyhow::Result<Vec<MarketInfo>> {
        let info: Value = get_json(&format!("{API}/api/v3/exchangeInfo?permissions=SPOT&symbolStatus=TRADING")).await?;
        let tickers: Value = get_json(&format!("{API}/api/v3/ticker/24hr?type=MINI")).await?;
        parse_markets(&info, &tickers)
    }

    async fn candles(&self, pair: &Pair, interval: IntervalSec, from: Ts, to: Ts) -> anyhow::Result<Vec<Candle>> {
        let iv = match interval {
            60 => "1m",
            300 => "5m",
            _ => bail!("unsupported interval {interval}"),
        };
        let v: Value = get_json(&format!(
            "{API}/api/v3/klines?symbol={}&interval={iv}&startTime={from}&endTime={}&limit=1000",
            pair.symbol,
            to - 1
        ))
        .await?;
        parse_klines(&v)
    }

    fn rest_delay_ms(&self) -> u64 {
        100
    }

    fn feed(&self, pairs: Vec<Pair>) -> BoxStream<'static, FeedEvent> {
        ws_feed(META.id, Arc::new(Proto), pairs)
    }
}

fn parse_markets(info: &Value, tickers: &Value) -> anyhow::Result<Vec<MarketInfo>> {
    let volumes: HashMap<&str, (f64, Option<f64>)> = tickers
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| Some((t["symbol"].as_str()?, (num(&t["quoteVolume"])?, num(&t["lastPrice"])))))
        .collect();
    let Some(symbols) = info["symbols"].as_array() else { bail!("no symbols in exchangeInfo") };
    Ok(symbols
        .iter()
        .filter(|s| s["status"] == "TRADING")
        .filter_map(|s| {
            let symbol = s["symbol"].as_str()?;
            let (vol, last) = volumes.get(symbol).copied().unwrap_or((0.0, None));
            Some(MarketInfo {
                pair: Pair::new(symbol, s["baseAsset"].as_str()?, s["quoteAsset"].as_str()?),
                quote_volume_24h: vol,
                last_price: last,
            })
        })
        .collect())
}

fn parse_klines(v: &Value) -> anyhow::Result<Vec<Candle>> {
    let Some(rows) = v.as_array() else { bail!("unexpected klines response: {v}") };
    Ok(rows
        .iter()
        .filter_map(|r| {
            Some(Candle { ts: int(&r[0])?, open: num(&r[1])?, high: num(&r[2])?, low: num(&r[3])?, close: num(&r[4])? })
        })
        .collect())
}

struct Proto;

#[async_trait]
impl WsProtocol for Proto {
    async fn url(&self) -> anyhow::Result<String> {
        Ok("wss://stream.binance.com:9443/stream".into())
    }

    fn pairs_per_connection(&self) -> usize {
        200
    }

    fn subscribe_msgs(&self, pairs: &[Pair]) -> Vec<String> {
        pairs
            .chunks(100)
            .enumerate()
            .map(|(i, c)| {
                let params: Vec<String> = c.iter().map(|p| format!("{}@aggTrade", p.symbol.to_lowercase())).collect();
                json!({"method": "SUBSCRIBE", "params": params, "id": i + 1}).to_string()
            })
            .collect()
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
        let d = &v["data"];
        if d["e"] != "aggTrade" {
            return Ok(Decoded::default());
        }
        let tick = (|| {
            Some(Tick { symbol: self.symbols.get(d["s"].as_str()?)?.clone(), price: num(&d["p"])?, ts: int(&d["T"])? })
        })();
        Ok(Decoded::ticks(tick.into_iter().collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markets() {
        let info = json!({"symbols": [
            {"symbol": "BTCUSDT", "status": "TRADING", "baseAsset": "BTC", "quoteAsset": "USDT"},
            {"symbol": "OLDBTC", "status": "BREAK", "baseAsset": "OLD", "quoteAsset": "BTC"}
        ]});
        let tickers = json!([{"symbol": "BTCUSDT", "lastPrice": "82475.47", "quoteVolume": "1153543.62"}]);
        let m = parse_markets(&info, &tickers).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].pair, Pair::new("BTCUSDT", "BTC", "USDT"));
        assert_eq!(m[0].quote_volume_24h, 1153543.62);
    }

    #[test]
    fn klines() {
        let v: Value = serde_json::from_str(r#"[[1792521600000,"82547.89","82586.01","82473.04","82475.47","13.97",1792521899999,"1153543.62",6056,"6.04","498779.41","0"]]"#).unwrap();
        assert_eq!(parse_klines(&v).unwrap(), vec![Candle { ts: 1792521600000, open: 82547.89, high: 82586.01, low: 82473.04, close: 82475.47 }]);
    }

    #[test]
    fn decode_agg_trade() {
        let mut d = Proto.decoder(&[Pair::new("BTCUSDT", "BTC", "USDT")]);
        let msg = r#"{"stream":"btcusdt@aggTrade","data":{"e":"aggTrade","E":1792522000001,"s":"BTCUSDT","a":1,"p":"82500.10","q":"0.01","f":1,"l":1,"T":1792522000000,"m":true}}"#;
        let r = d.decode(Frame::Text(msg)).unwrap();
        assert_eq!(r.ticks, vec![Tick { symbol: "BTCUSDT".into(), price: 82500.10, ts: 1792522000000 }]);
        assert!(d.decode(Frame::Text(r#"{"result":null,"id":1}"#)).unwrap().ticks.is_empty());
    }

    #[test]
    fn subscriptions_are_chunked() {
        let pairs: Vec<Pair> = (0..150).map(|i| Pair::new(format!("P{i}USDT"), format!("P{i}"), "USDT")).collect();
        let msgs = Proto.subscribe_msgs(&pairs);
        assert_eq!(msgs.len(), 2);
        assert!(msgs[0].contains("p0usdt@aggTrade"));
    }
}
