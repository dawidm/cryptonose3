use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::Serialize;

use crate::types::{Candle, IntervalSec, MarketInfo, Pair, Tick, Ts};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ExchangeKind {
    Cex,
    Dex,
}

/// How live prices are delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FeedKind {
    /// Websocket streaming.
    Streaming,
    /// REST polling, near real time.
    Polling,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeMeta {
    pub id: &'static str,
    pub name: &'static str,
    /// Brand-neutral accent color for the UI.
    pub color: &'static str,
    pub kind: ExchangeKind,
    pub feed: FeedKind,
    /// Default pair selection for a new exchange tab: (quote, top N pairs by 24h volume).
    pub default_top: &'static [(&'static str, usize)],
}

#[derive(Debug, Clone, PartialEq)]
pub enum FeedState {
    Connecting,
    Connected,
    Reconnecting(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum FeedEvent {
    Ticks(Vec<Tick>),
    State(FeedState),
}

/// An exchange connector. Implementations live in `cn-exchanges`; adding an exchange means
/// implementing this trait and registering it there.
#[async_trait]
pub trait Exchange: Send + Sync {
    fn meta(&self) -> &ExchangeMeta;

    /// Web page of the pair's market.
    fn market_url(&self, pair: &Pair) -> String;

    /// All tradable pairs with 24h quote volume.
    async fn markets(&self) -> anyhow::Result<Vec<MarketInfo>>;

    /// Candles starting in [from, to), oldest first. Must support 60 and 300 second intervals.
    async fn candles(&self, pair: &Pair, interval: IntervalSec, from: Ts, to: Ts) -> anyhow::Result<Vec<Candle>>;

    /// Delay between consecutive REST requests when loading history.
    fn rest_delay_ms(&self) -> u64;

    /// Live prices for the given pairs. The stream reconnects on its own and should never end.
    fn feed(&self, pairs: Vec<Pair>) -> BoxStream<'static, FeedEvent>;
}
