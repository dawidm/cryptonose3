use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Unix timestamp in milliseconds.
pub type Ts = i64;

pub fn now_ms() -> Ts {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as Ts)
        .unwrap_or(0)
}

/// Exchange identifier, e.g. "binance".
pub type ExchangeId = Arc<str>;

/// A trading pair on a specific exchange. `symbol` is the exchange's own API symbol and is the key
/// used everywhere inside the engine; `base`/`quote` are normalized upper-case currency codes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Pair {
    pub symbol: Arc<str>,
    pub base: Arc<str>,
    pub quote: Arc<str>,
}

impl Pair {
    pub fn new(symbol: impl Into<Arc<str>>, base: impl Into<Arc<str>>, quote: impl Into<Arc<str>>) -> Self {
        Self { symbol: symbol.into(), base: base.into(), quote: quote.into() }
    }

    pub fn display(&self) -> String {
        format!("{}/{}", self.base, self.quote)
    }
}

impl fmt::Display for Pair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.base, self.quote)
    }
}

/// Single price observation (trade, mid price, ...).
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    pub symbol: Arc<str>,
    pub price: f64,
    pub ts: Ts,
}

/// OHLC candle; `ts` is the candle start time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Candle {
    pub ts: Ts,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

impl Candle {
    pub fn flat(ts: Ts, price: f64) -> Self {
        Self { ts, open: price, high: price, low: price, close: price }
    }

    pub fn range(&self) -> f64 {
        (self.high - self.low).abs()
    }

    /// No price movement inside the candle, usually means no trades.
    pub fn is_empty(&self) -> bool {
        self.high == self.low
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketInfo {
    pub pair: Pair,
    /// 24h volume denominated in the quote currency.
    pub quote_volume_24h: f64,
    pub last_price: Option<f64>,
}

/// Candle interval in seconds.
pub type IntervalSec = u32;
