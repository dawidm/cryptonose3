//! Live smoke tests against real exchange APIs: `cargo test -p cn-exchanges -- --ignored live_`

use std::sync::Arc;
use std::time::Duration;

use cn_core::{now_ms, Exchange, FeedEvent};
use futures::StreamExt;

async fn smoke(ex: Arc<dyn Exchange>) {
    let id = ex.meta().id;
    let mut markets = ex.markets().await.unwrap_or_else(|e| panic!("{id} markets: {e:#}"));
    assert!(markets.len() > 10, "{id}: only {} markets", markets.len());
    let quote = ex.meta().default_top[0].0;
    markets.retain(|m| &*m.pair.quote == quote);
    markets.sort_by(|a, b| b.quote_volume_24h.total_cmp(&a.quote_volume_24h));
    let pairs: Vec<_> = markets.iter().take(5).map(|m| m.pair.clone()).collect();
    assert!(!pairs.is_empty(), "{id}: no {quote} markets");

    let now = now_ms();
    for (iv, n) in [(300u32, 20i64), (60, 10)] {
        let c = ex.candles(&pairs[0], iv, now - n * iv as i64 * 1000, now).await.unwrap_or_else(|e| panic!("{id} candles: {e:#}"));
        assert!(c.len() as i64 >= n - 2, "{id}: {} {iv}s candles", c.len());
        assert!(c.windows(2).all(|w| w[0].ts < w[1].ts), "{id}: candles not sorted");
        assert!(c.iter().all(|x| x.ts >= now - n * iv as i64 * 1000 - iv as i64 * 1000 && x.ts < now));
    }

    let mut feed = ex.feed(pairs.clone());
    let mut ticks = 0;
    let _ = tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(e) = feed.next().await {
            if let FeedEvent::Ticks(t) = e {
                for t in &t {
                    assert!(pairs.iter().any(|p| p.symbol == t.symbol));
                    assert!((t.ts - now_ms()).abs() < 120_000, "{id}: tick ts {} too far from now", t.ts);
                }
                ticks += t.len();
                if ticks >= 5 {
                    break;
                }
            }
        }
    })
    .await;
    assert!(ticks >= 5, "{id}: only {ticks} ticks in 30s");
    println!("{id}: ok, {} markets, {ticks} ticks", markets.len());
}

macro_rules! live {
    ($name:ident, $ex:expr) => {
        #[tokio::test]
        #[ignore]
        async fn $name() {
            smoke(Arc::new($ex)).await;
        }
    };
}

live!(live_binance, cn_exchanges::binance::Binance);
live!(live_bitfinex, cn_exchanges::bitfinex::Bitfinex);
live!(live_htx, cn_exchanges::htx::Htx);
live!(live_kucoin, cn_exchanges::kucoin::KuCoin);
live!(live_poloniex, cn_exchanges::poloniex::Poloniex);
live!(live_hyperliquid, cn_exchanges::hyperliquid::Hyperliquid);
live!(live_coinbase, cn_exchanges::coinbase::Coinbase);
live!(live_kraken, cn_exchanges::kraken::Kraken);
