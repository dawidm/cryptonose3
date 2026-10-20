use std::collections::HashMap;
use std::sync::Arc;

use anyhow::bail;
use async_trait::async_trait;
use cn_core::{Candle, Exchange, ExchangeKind, ExchangeMeta, FeedEvent, FeedKind, IntervalSec, MarketInfo, Pair, Tick, Ts};
use futures::stream::BoxStream;
use serde_json::{json, Value};

use crate::util::{get_json, int, num};
use crate::ws::{ws_feed, Decoded, Frame, WsDecoder, WsProtocol};

const API: &str = "https://api-pub.bitfinex.com";

static META: ExchangeMeta = ExchangeMeta {
    id: "bitfinex",
    name: "Bitfinex",
    color: "#16b157",
    kind: ExchangeKind::Cex,
    feed: FeedKind::Streaming,
    default_top: &[("USD", 20), ("USDT", 20)],
};

pub struct Bitfinex;

#[async_trait]
impl Exchange for Bitfinex {
    fn meta(&self) -> &ExchangeMeta {
        &META
    }

    fn market_url(&self, pair: &Pair) -> String {
        let (b, q) = split_symbol(&pair.symbol).unwrap_or_default();
        format!("https://trading.bitfinex.com/t/{b}:{q}?type=exchange")
    }

    async fn markets(&self) -> anyhow::Result<Vec<MarketInfo>> {
        let v: Value = get_json(&format!("{API}/v2/tickers?symbols=ALL")).await?;
        parse_tickers(&v)
    }

    async fn candles(&self, pair: &Pair, interval: IntervalSec, from: Ts, to: Ts) -> anyhow::Result<Vec<Candle>> {
        let tf = match interval {
            60 => "1m",
            300 => "5m",
            _ => bail!("unsupported interval {interval}"),
        };
        let v: Value = get_json(&format!(
            "{API}/v2/candles/trade:{tf}:{}/hist?start={from}&end={}&limit=10000&sort=1",
            pair.symbol,
            to - 1
        ))
        .await?;
        parse_candles(&v)
    }

    fn rest_delay_ms(&self) -> u64 {
        // candles endpoint allows 30 requests per minute
        2100
    }

    fn feed(&self, pairs: Vec<Pair>) -> BoxStream<'static, FeedEvent> {
        ws_feed(META.id, Arc::new(Proto), pairs)
    }
}

/// Bitfinex uses its own short codes for some currencies.
fn normalize(c: &str) -> &str {
    match c {
        "UST" => "USDT",
        "UDC" => "USDC",
        "EUT" => "EURT",
        other => other,
    }
}

/// "tBTCUSD" -> ("BTC", "USD"), "tTESTBTC:TESTUSD" -> ("TESTBTC", "TESTUSD").
fn split_symbol(symbol: &str) -> Option<(&str, &str)> {
    let s = symbol.strip_prefix('t')?;
    if let Some((b, q)) = s.split_once(':') {
        Some((b, q))
    } else if s.len() == 6 {
        Some((&s[..3], &s[3..]))
    } else {
        None
    }
}

fn parse_tickers(v: &Value) -> anyhow::Result<Vec<MarketInfo>> {
    let Some(rows) = v.as_array() else { bail!("unexpected tickers response: {v}") };
    Ok(rows
        .iter()
        .filter_map(|r| {
            let symbol = r[0].as_str()?;
            let (b, q) = split_symbol(symbol)?;
            // derivatives (BTCF0:USTF0) and test markets come in the same list
            if [b, q].iter().any(|c| c.ends_with("F0") || c.starts_with("TEST")) {
                return None;
            }
            let last = num(&r[7])?;
            let volume = num(&r[8]).unwrap_or(0.0);
            Some(MarketInfo { pair: Pair::new(symbol, normalize(b), normalize(q)), quote_volume_24h: volume * last, last_price: Some(last) })
        })
        .collect())
}

fn parse_candles(v: &Value) -> anyhow::Result<Vec<Candle>> {
    let Some(rows) = v.as_array() else { bail!("unexpected candles response: {v}") };
    // [MTS, OPEN, CLOSE, HIGH, LOW, VOLUME]
    Ok(rows
        .iter()
        .filter_map(|r| {
            Some(Candle { ts: int(&r[0])?, open: num(&r[1])?, close: num(&r[2])?, high: num(&r[3])?, low: num(&r[4])? })
        })
        .collect())
}

struct Proto;

#[async_trait]
impl WsProtocol for Proto {
    async fn url(&self) -> anyhow::Result<String> {
        Ok("wss://api-pub.bitfinex.com/ws/2".into())
    }

    fn pairs_per_connection(&self) -> usize {
        25
    }

    fn subscribe_msgs(&self, pairs: &[Pair]) -> Vec<String> {
        pairs.iter().map(|p| json!({"event": "subscribe", "channel": "trades", "symbol": &*p.symbol}).to_string()).collect()
    }

    fn subscribe_interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(50)
    }

    fn decoder(&self, pairs: &[Pair]) -> Box<dyn WsDecoder> {
        Box::new(Decoder {
            symbols: pairs.iter().map(|p| (p.symbol.to_string(), p.symbol.clone())).collect(),
            channels: HashMap::new(),
        })
    }
}

struct Decoder {
    symbols: HashMap<String, Arc<str>>,
    channels: HashMap<i64, Arc<str>>,
}

impl WsDecoder for Decoder {
    fn decode(&mut self, frame: Frame<'_>) -> anyhow::Result<Decoded> {
        let Frame::Text(t) = frame else { return Ok(Decoded::default()) };
        let v: Value = serde_json::from_str(t)?;
        if v["event"] == "subscribed" {
            if let (Some(id), Some(sym)) = (int(&v["chanId"]), v["symbol"].as_str().and_then(|s| self.symbols.get(s))) {
                self.channels.insert(id, sym.clone());
            }
            return Ok(Decoded::default());
        }
        // [chanId, "te", [ID, MTS, AMOUNT, PRICE]]; snapshots and "tu" duplicates are skipped
        let Some(arr) = v.as_array() else { return Ok(Decoded::default()) };
        if arr.len() < 3 || arr[1] != "te" {
            return Ok(Decoded::default());
        }
        let tick = (|| {
            let symbol = self.channels.get(&int(&arr[0])?)?.clone();
            Some(Tick { symbol, ts: int(&arr[2][1])?, price: num(&arr[2][3])? })
        })();
        Ok(Decoded::ticks(tick.into_iter().collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tickers() {
        let v: Value = serde_json::from_str(r#"[["tBTCUSD",82523,1.7,82531,1.79,542,0.0066,82530,431.09,83470,81209],["tDOGE:UST",1,1,1,1,0,0,2,10,1,1],["tTESTBTC:TESTUST",1,1,1,1,0,0,2,10,1,1],["tBTCF0:USTF0",1,1,1,1,0,0,2,10,1,1],["fUSD",0.0001,1,1,1,1,1,1,1,1,1,1,1,1,1,1]]"#).unwrap();
        let m = parse_tickers(&v).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].pair, Pair::new("tBTCUSD", "BTC", "USD"));
        assert!((m[0].quote_volume_24h - 431.09 * 82530.0).abs() < 1e-6);
        assert_eq!(m[1].pair, Pair::new("tDOGE:UST", "DOGE", "USDT"));
    }

    #[test]
    fn candles() {
        let v: Value = serde_json::from_str("[[1760000100000,121290,121310,121420,121250,4.65]]").unwrap();
        assert_eq!(parse_candles(&v).unwrap(), vec![Candle { ts: 1760000100000, open: 121290.0, close: 121310.0, high: 121420.0, low: 121250.0 }]);
    }

    #[test]
    fn decode_trades() {
        let mut d = Proto.decoder(&[Pair::new("tBTCUSD", "BTC", "USD")]);
        d.decode(Frame::Text(r#"{"event":"subscribed","channel":"trades","chanId":17,"symbol":"tBTCUSD","pair":"BTCUSD"}"#)).unwrap();
        assert!(d.decode(Frame::Text("[17,[[1,1792522000000,0.1,82500]]]")).unwrap().ticks.is_empty());
        assert!(d.decode(Frame::Text(r#"[17,"hb"]"#)).unwrap().ticks.is_empty());
        let t = d.decode(Frame::Text(r#"[17,"te",[2,1792522000500,-0.2,82510.5]]"#)).unwrap().ticks;
        assert_eq!(t, vec![Tick { symbol: "tBTCUSD".into(), price: 82510.5, ts: 1792522000500 }]);
        assert!(d.decode(Frame::Text(r#"[17,"tu",[2,1792522000500,-0.2,82510.5]]"#)).unwrap().ticks.is_empty());
    }
}
