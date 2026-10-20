//! Exchange connectors. To add an exchange: create a module implementing [`cn_core::Exchange`]
//! (usually with a [`ws::WsProtocol`] or [`poll::PollProtocol`] for the live feed) and add it to
//! [`all`].

use std::sync::Arc;

use cn_core::Exchange;

pub mod binance;
pub mod bitfinex;
pub mod coinbase;
pub mod htx;
pub mod hyperliquid;
pub mod kraken;
pub mod kucoin;
pub mod poll;
pub mod poloniex;
pub mod util;
pub mod ws;

pub fn all() -> Vec<Arc<dyn Exchange>> {
    vec![
        Arc::new(binance::Binance),
        Arc::new(bitfinex::Bitfinex),
        Arc::new(coinbase::Coinbase),
        Arc::new(htx::Htx),
        Arc::new(kraken::Kraken),
        Arc::new(kucoin::KuCoin),
        Arc::new(poloniex::Poloniex),
        Arc::new(hyperliquid::Hyperliquid),
    ]
}

pub fn by_id(id: &str) -> Option<Arc<dyn Exchange>> {
    all().into_iter().find(|e| e.meta().id == id)
}
