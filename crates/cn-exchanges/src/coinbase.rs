//! Coinbase (Advanced Trade public market data).

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::bail;
use async_trait::async_trait;
use cn_core::{Candle, Exchange, ExchangeKind, ExchangeMeta, FeedEvent, FeedKind, IntervalSec, MarketInfo, Pair, Tick, Ts};
use futures::stream::BoxStream;
use serde_json::{json, Value};

use crate::util::{get_json, int, num, rfc3339_ms};
use crate::ws::{ws_feed, Decoded, Frame, WsDecoder, WsProtocol};

const API: &str = "https://api.coinbase.com/api/v3/brokerage/market";

static META: ExchangeMeta = ExchangeMeta {
    id: "coinbase",
    name: "Coinbase",
    color: "#0052ff",
    kind: ExchangeKind::Cex,
    feed: FeedKind::Streaming,
    default_top: &[("USD", 30)],
};

pub struct Coinbase;

#[async_trait]
impl Exchange for Coinbase {
    fn meta(&self) -> &ExchangeMeta {
        &META
    }

    fn market_url(&self, pair: &Pair) -> String {
        format!("https://www.coinbase.com/advanced-trade/spot/{}", pair.symbol)
    }

    async fn markets(&self) -> anyhow::Result<Vec<MarketInfo>> {
        let v: Value = get_json(&format!("{API}/products?product_type=SPOT")).await?;
        parse_products(&v)
    }

    async fn candles(&self, pair: &Pair, interval: IntervalSec, from: Ts, to: Ts) -> anyhow::Result<Vec<Candle>> {
        let g = match interval {
            60 => "ONE_MINUTE",
            300 => "FIVE_MINUTE",
            _ => bail!("unsupported interval {interval}"),
        };
        // at most 350 candles per request
        let v: Value = get_json(&format!(
            "{API}/products/{}/candles?start={}&end={}&granularity={g}",
            pair.symbol,
            from / 1000,
            (to - 1) / 1000
        ))
        .await?;
        Ok(parse_candles(&v)?.into_iter().filter(|c| c.ts >= from && c.ts < to).collect())
    }

    fn rest_delay_ms(&self) -> u64 {
        150
    }

    fn feed(&self, pairs: Vec<Pair>) -> BoxStream<'static, FeedEvent> {
        ws_feed(META.id, Arc::new(Proto), pairs)
    }
}

fn parse_products(v: &Value) -> anyhow::Result<Vec<MarketInfo>> {
    let Some(list) = v["products"].as_array() else { bail!("unexpected products response") };
    Ok(list
        .iter()
        .filter(|p| p["status"] == "online" && p["trading_disabled"] != true && p["is_disabled"] != true && p["view_only"] != true)
        // USD and USDC books are merged: BTC-USDC (with alias BTC-USD) is the same market as BTC-USD
        // (alias_to BTC-USDC), and trades are streamed as BTC-USD, so the aliased canonical product is skipped
        .filter(|p| p["alias"].as_str().is_none_or(str::is_empty))
        .filter_map(|p| {
            Some(MarketInfo {
                pair: Pair::new(p["product_id"].as_str()?, p["base_display_symbol"].as_str()?, p["quote_display_symbol"].as_str()?),
                quote_volume_24h: num(&p["approximate_quote_24h_volume"]).unwrap_or(0.0),
                last_price: num(&p["price"]),
            })
        })
        .collect())
}

fn parse_candles(v: &Value) -> anyhow::Result<Vec<Candle>> {
    let Some(rows) = v["candles"].as_array() else { bail!("unexpected candles response: {v}") };
    let mut out: Vec<Candle> = rows
        .iter()
        .filter_map(|r| {
            Some(Candle { ts: int(&r["start"])? * 1000, open: num(&r["open"])?, high: num(&r["high"])?, low: num(&r["low"])?, close: num(&r["close"])? })
        })
        .collect();
    out.sort_by_key(|c| c.ts);
    Ok(out)
}

struct Proto;

#[async_trait]
impl WsProtocol for Proto {
    async fn url(&self) -> anyhow::Result<String> {
        Ok("wss://advanced-trade-ws.coinbase.com".into())
    }

    fn pairs_per_connection(&self) -> usize {
        200
    }

    fn subscribe_msgs(&self, pairs: &[Pair]) -> Vec<String> {
        // heartbeats keep the connection open when pairs are quiet
        let mut msgs = vec![json!({"type": "subscribe", "channel": "heartbeats"}).to_string()];
        msgs.extend(pairs.chunks(100).map(|c| {
            let ids: Vec<&str> = c.iter().map(|p| &*p.symbol).collect();
            json!({"type": "subscribe", "product_ids": ids, "channel": "market_trades"}).to_string()
        }));
        msgs
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
        if v["channel"] != "market_trades" {
            return Ok(Decoded::default());
        }
        let ticks = v["events"]
            .as_array()
            .into_iter()
            .flatten()
            // the snapshot on subscribe holds old trades
            .filter(|e| e["type"] == "update")
            .flat_map(|e| e["trades"].as_array().into_iter().flatten())
            .filter_map(|tr| {
                Some(Tick { symbol: self.symbols.get(tr["product_id"].as_str()?)?.clone(), price: num(&tr["price"])?, ts: rfc3339_ms(tr["time"].as_str()?)? })
            })
            .collect();
        Ok(Decoded::ticks(ticks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn products() {
        let v = json!({"products": [
            {"product_id": "BTC-USD", "base_display_symbol": "BTC", "quote_display_symbol": "USD", "price": "82567.84",
             "approximate_quote_24h_volume": "420469299.22", "status": "online", "trading_disabled": false, "is_disabled": false, "view_only": false,
             "alias": "", "alias_to": ["BTC-USDC"]},
            {"product_id": "BTC-USDC", "base_display_symbol": "BTC", "quote_display_symbol": "USD", "price": "82567.84",
             "approximate_quote_24h_volume": "420469299.22", "status": "online", "alias": "BTC-USD", "alias_to": []},
            {"product_id": "OLD-USD", "base_display_symbol": "OLD", "quote_display_symbol": "USD", "status": "delisted"}
        ]});
        let m = parse_products(&v).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].pair, Pair::new("BTC-USD", "BTC", "USD"));
        assert_eq!(m[0].quote_volume_24h, 420469299.22);
    }

    #[test]
    fn candles() {
        let v = json!({"candles": [
            {"start": "1792521600", "low": "82374.05", "high": "82501.23", "open": "82452.81", "close": "82380.09", "volume": "12.18"},
            {"start": "1792521300", "low": "1", "high": "2", "open": "1", "close": "2", "volume": "1"}
        ]});
        let c = parse_candles(&v).unwrap();
        assert_eq!(c[0].ts, 1792521300000);
        assert_eq!(c[1], Candle { ts: 1792521600000, open: 82452.81, high: 82501.23, low: 82374.05, close: 82380.09 });
    }

    #[test]
    fn decode_trades() {
        let mut d = Proto.decoder(&[Pair::new("BTC-USD", "BTC", "USD")]);
        let update = r#"{"channel": "market_trades", "timestamp": "2026-10-20T20:11:22.734996352Z", "sequence_num": 4, "events": [{"type": "update", "trades": [{"product_id": "BTC-USD", "trade_id": "1108539414", "price": "82573.16", "size": "0.00000006", "time": "2026-10-20T20:11:22.65824Z", "side": "BUY"}]}]}"#;
        assert_eq!(d.decode(Frame::Text(update)).unwrap().ticks, vec![Tick { symbol: "BTC-USD".into(), price: 82573.16, ts: 1792527082658 }]);
        let snapshot = update.replace("\"update\"", "\"snapshot\"");
        assert!(d.decode(Frame::Text(&snapshot)).unwrap().ticks.is_empty());
        assert!(d.decode(Frame::Text(r#"{"channel": "heartbeats", "events": [{"current_time": "x", "heartbeat_counter": 1}]}"#)).unwrap().ticks.is_empty());
    }
}
