//! Near-real-time feed for exchanges without a websocket: polls a bulk "all tickers" REST endpoint
//! and emits ticks for prices that changed since the previous poll.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use cn_core::{now_ms, FeedEvent, FeedState, Pair, Tick};
use futures::stream::BoxStream;
use futures::StreamExt;
use tokio::time::{interval, MissedTickBehavior};
use tracing::warn;

#[async_trait]
pub trait PollProtocol: Send + Sync + 'static {
    /// Current prices by exchange symbol.
    async fn prices(&self) -> anyhow::Result<Vec<(String, f64)>>;

    /// Polling interval, as short as the exchange's rate limit allows.
    fn poll_interval(&self) -> Duration;
}

pub fn poll_feed<P: PollProtocol>(proto: Arc<P>, pairs: Vec<Pair>) -> BoxStream<'static, FeedEvent> {
    let wanted: HashMap<String, Arc<str>> = pairs.iter().map(|p| (p.symbol.to_string(), p.symbol.clone())).collect();
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    let handle = tokio::spawn(async move {
        let mut last: HashMap<Arc<str>, f64> = HashMap::new();
        let mut timer = interval(proto.poll_interval());
        timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut connected = false;
        let _ = tx.send(FeedEvent::State(FeedState::Connecting)).await;
        loop {
            timer.tick().await;
            match proto.prices().await {
                Ok(prices) => {
                    if !connected {
                        connected = true;
                        let _ = tx.send(FeedEvent::State(FeedState::Connected)).await;
                    }
                    let ts = now_ms();
                    let ticks = changed_ticks(&wanted, &mut last, prices, ts);
                    if !ticks.is_empty() && tx.send(FeedEvent::Ticks(ticks)).await.is_err() {
                        return;
                    }
                }
                Err(e) => {
                    warn!("polling prices failed: {e:#}");
                    connected = false;
                    if tx.send(FeedEvent::State(FeedState::Reconnecting(format!("{e:#}")))).await.is_err() {
                        return;
                    }
                }
            }
        }
    });
    struct Abort(tokio::task::JoinHandle<()>);
    impl Drop for Abort {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    futures::stream::unfold((rx, Abort(handle)), |(mut rx, g)| async move { rx.recv().await.map(|e| (e, (rx, g))) }).boxed()
}

fn changed_ticks(wanted: &HashMap<String, Arc<str>>, last: &mut HashMap<Arc<str>, f64>, prices: Vec<(String, f64)>, ts: i64) -> Vec<Tick> {
    let mut seen = HashSet::new();
    prices
        .into_iter()
        .filter_map(|(s, price)| {
            let symbol = wanted.get(&s)?.clone();
            if !seen.insert(symbol.clone()) || last.get(&symbol) == Some(&price) {
                return None;
            }
            last.insert(symbol.clone(), price);
            Some(Tick { symbol, price, ts })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fake(AtomicUsize);

    #[async_trait]
    impl PollProtocol for Fake {
        async fn prices(&self) -> anyhow::Result<Vec<(String, f64)>> {
            let n = self.0.fetch_add(1, Ordering::SeqCst);
            Ok(vec![("A".into(), if n < 2 { 1.0 } else { 2.0 }), ("B".into(), 5.0), ("X".into(), n as f64)])
        }
        fn poll_interval(&self) -> Duration {
            Duration::from_millis(10)
        }
    }

    #[tokio::test]
    async fn emits_only_changes_for_wanted_pairs() {
        let pairs = vec![Pair::new("A", "A", "U"), Pair::new("B", "B", "U")];
        let mut s = poll_feed(Arc::new(Fake(AtomicUsize::new(0))), pairs);
        assert_eq!(s.next().await, Some(FeedEvent::State(FeedState::Connecting)));
        assert_eq!(s.next().await, Some(FeedEvent::State(FeedState::Connected)));
        let Some(FeedEvent::Ticks(t)) = s.next().await else { panic!() };
        assert_eq!(t.iter().map(|t| (&*t.symbol, t.price)).collect::<Vec<_>>(), vec![("A", 1.0), ("B", 5.0)]);
        // second poll has no changes, third changes A only
        let Some(FeedEvent::Ticks(t)) = s.next().await else { panic!() };
        assert_eq!(t.iter().map(|t| (&*t.symbol, t.price)).collect::<Vec<_>>(), vec![("A", 2.0)]);
    }
}
