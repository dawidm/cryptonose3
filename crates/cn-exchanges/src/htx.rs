//! HTX (formerly Huobi). Websocket frames are gzip compressed.

use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;

use anyhow::bail;
use async_trait::async_trait;
use cn_core::{Candle, Exchange, ExchangeKind, ExchangeMeta, FeedEvent, FeedKind, IntervalSec, MarketInfo, Pair, Tick, Ts};
use flate2::read::GzDecoder;
use futures::stream::BoxStream;
use serde_json::{json, Value};

use crate::util::{get_json, int, num};
use crate::ws::{ws_feed, Decoded, Frame, WsDecoder, WsProtocol};

const API: &str = "https://api.huobi.pro";

static META: ExchangeMeta = ExchangeMeta {
    id: "htx",
    name: "HTX",
    color: "#2a7de1",
    kind: ExchangeKind::Cex,
    feed: FeedKind::Streaming,
    default_top: &[("USDT", 20)],
};

pub struct Htx;

#[async_trait]
impl Exchange for Htx {
    fn meta(&self) -> &ExchangeMeta {
        &META
    }

    fn market_url(&self, pair: &Pair) -> String {
        format!("https://www.htx.com/trade/{}_{}", pair.base.to_lowercase(), pair.quote.to_lowercase())
    }

    async fn markets(&self) -> anyhow::Result<Vec<MarketInfo>> {
        let symbols: Value = get_json(&format!("{API}/v2/settings/common/symbols")).await?;
        let tickers: Value = get_json(&format!("{API}/market/tickers")).await?;
        parse_markets(&symbols, &tickers)
    }

    async fn candles(&self, pair: &Pair, interval: IntervalSec, from: Ts, to: Ts) -> anyhow::Result<Vec<Candle>> {
        let period = match interval {
            60 => "1min",
            300 => "5min",
            _ => bail!("unsupported interval {interval}"),
        };
        // the endpoint only returns the latest `size` candles
        let step = interval as i64 * 1000;
        let size = ((Ts::max(to, cn_core::now_ms()) - from) / step + 2).clamp(1, 2000);
        let v: Value = get_json(&format!("{API}/market/history/kline?symbol={}&period={period}&size={size}", pair.symbol)).await?;
        Ok(parse_candles(&v)?.into_iter().filter(|c| c.ts >= from && c.ts < to).collect())
    }

    fn rest_delay_ms(&self) -> u64 {
        100
    }

    fn feed(&self, pairs: Vec<Pair>) -> BoxStream<'static, FeedEvent> {
        ws_feed(META.id, Arc::new(Proto), pairs)
    }
}

fn parse_markets(symbols: &Value, tickers: &Value) -> anyhow::Result<Vec<MarketInfo>> {
    let volumes: HashMap<&str, (f64, Option<f64>)> = tickers["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| Some((t["symbol"].as_str()?, (num(&t["vol"]).unwrap_or(0.0), num(&t["close"])))))
        .collect();
    let Some(list) = symbols["data"].as_array() else { bail!("unexpected symbols response") };
    Ok(list
        .iter()
        .filter(|s| s["state"] == "online")
        .filter_map(|s| {
            let sym = s["sc"].as_str()?;
            let (vol, last) = volumes.get(sym).copied().unwrap_or((0.0, None));
            Some(MarketInfo {
                pair: Pair::new(sym, s["bcdn"].as_str()?.to_uppercase(), s["qcdn"].as_str()?.to_uppercase()),
                quote_volume_24h: vol,
                last_price: last,
            })
        })
        .collect())
}

fn parse_candles(v: &Value) -> anyhow::Result<Vec<Candle>> {
    if v["status"] != "ok" {
        bail!("kline error: {v}");
    }
    let mut out: Vec<Candle> = v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            Some(Candle { ts: int(&r["id"])? * 1000, open: num(&r["open"])?, high: num(&r["high"])?, low: num(&r["low"])?, close: num(&r["close"])? })
        })
        .collect();
    out.sort_by_key(|c| c.ts);
    Ok(out)
}

struct Proto;

#[async_trait]
impl WsProtocol for Proto {
    async fn url(&self) -> anyhow::Result<String> {
        Ok("wss://api.huobi.pro/ws".into())
    }

    fn pairs_per_connection(&self) -> usize {
        100
    }

    fn subscribe_msgs(&self, pairs: &[Pair]) -> Vec<String> {
        pairs.iter().map(|p| json!({"sub": format!("market.{}.trade.detail", p.symbol), "id": &*p.symbol}).to_string()).collect()
    }

    fn subscribe_interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(50)
    }

    fn decoder(&self, pairs: &[Pair]) -> Box<dyn WsDecoder> {
        Box::new(Decoder { symbols: pairs.iter().map(|p| (p.symbol.to_string(), p.symbol.clone())).collect(), since: cn_core::now_ms() })
    }
}

struct Decoder {
    symbols: HashMap<String, Arc<str>>,
    /// Connection time; HTX replays the last (possibly old) trade on subscribe, those are skipped.
    since: Ts,
}

impl WsDecoder for Decoder {
    fn decode(&mut self, frame: Frame<'_>) -> anyhow::Result<Decoded> {
        let text = match frame {
            Frame::Binary(b) => {
                let mut s = String::new();
                GzDecoder::new(b).read_to_string(&mut s)?;
                s
            }
            Frame::Text(t) => t.to_string(),
        };
        let v: Value = serde_json::from_str(&text)?;
        if let Some(ping) = v.get("ping") {
            return Ok(Decoded::reply(json!({"pong": ping}).to_string()));
        }
        // {"ch":"market.btcusdt.trade.detail","tick":{"data":[{"price":..,"ts":..}]}}
        let Some(ch) = v["ch"].as_str() else { return Ok(Decoded::default()) };
        let Some(symbol) = ch.strip_prefix("market.").and_then(|s| s.strip_suffix(".trade.detail")).and_then(|s| self.symbols.get(s)) else {
            return Ok(Decoded::default());
        };
        let ticks = v["tick"]["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| Some(Tick { symbol: symbol.clone(), price: num(&d["price"])?, ts: int(&d["ts"])? }))
            .filter(|t| t.ts >= self.since - 1000)
            .collect();
        Ok(Decoded::ticks(ticks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use std::io::Write;

    fn gzip(s: &str) -> Vec<u8> {
        let mut e = GzEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(s.as_bytes()).unwrap();
        e.finish().unwrap()
    }

    #[test]
    fn markets() {
        let symbols = json!({"status": "ok", "data": [
            {"sc": "btcusdt", "bcdn": "BTC", "qcdn": "USDT", "state": "online"},
            {"sc": "mcontentusdt", "bcdn": "MCONTENT", "qcdn": "USDT", "state": "offline"}
        ]});
        let tickers = json!({"data": [{"symbol": "btcusdt", "close": 82464.1, "vol": 3487517.02}]});
        let m = parse_markets(&symbols, &tickers).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].pair, Pair::new("btcusdt", "BTC", "USDT"));
        assert_eq!(m[0].quote_volume_24h, 3487517.02);
    }

    #[test]
    fn candles_sorted_oldest_first() {
        let v: Value = serde_json::from_str(r#"{"ch":"market.btcusdt.kline.5min","status":"ok","data":[{"id":1792521900,"open":2,"close":3,"low":1,"high":4},{"id":1792521600,"open":1,"close":2,"low":1,"high":2}]}"#).unwrap();
        let c = parse_candles(&v).unwrap();
        assert_eq!(c[0].ts, 1792521600000);
        assert_eq!(c[1], Candle { ts: 1792521900000, open: 2.0, high: 4.0, low: 1.0, close: 3.0 });
    }

    #[test]
    fn decode_gzip_trades_and_ping() {
        let mut d = Decoder { symbols: HashMap::from([("btcusdt".to_string(), "btcusdt".into())]), since: 1792522000000 };
        let r = d.decode(Frame::Binary(&gzip(r#"{"ping":1792522000000}"#))).unwrap();
        assert_eq!(r.reply.as_deref(), Some(r#"{"pong":1792522000000}"#));
        let msg = r#"{"ch":"market.btcusdt.trade.detail","ts":1792522000100,"tick":{"id":1,"ts":1792522000100,"data":[{"id":5,"ts":1792522000090,"tradeId":1,"amount":0.1,"price":82500.5,"direction":"buy"},{"id":6,"ts":1792522000095,"tradeId":2,"amount":0.1,"price":82501,"direction":"sell"}]}}"#;
        let t = d.decode(Frame::Binary(&gzip(msg))).unwrap().ticks;
        assert_eq!(t.len(), 2);
        assert_eq!(t[1], Tick { symbol: "btcusdt".into(), price: 82501.0, ts: 1792522000095 });
        let old = r#"{"ch":"market.btcusdt.trade.detail","tick":{"data":[{"ts":1792521000000,"price":1}]}}"#;
        assert!(d.decode(Frame::Text(old)).unwrap().ticks.is_empty());
    }
}
