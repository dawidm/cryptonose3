# Cryptonose 3

Desktop app that watches live cryptocurrency prices and alerts you about sharp price moves. Rewrite of
[Cryptonose 2](https://github.com/dawidm/cryptonose2) (Java/JavaFX) as a Tauri app with a Rust core.

For every watched pair it shows the price change over the last 5 and 30 minutes, in % and **relative** to the
pair's typical candle range (change ÷ weighted average high−low of the last 50 candles). An alert fires when a move
is large in % and in relative terms, or very large in relative terms alone.

## Exchanges

| Exchange | Type | Live data |
|---|---|---|
| Binance | CEX | websocket trades (`@aggTrade`) |
| Bitfinex | CEX | websocket trades |
| Coinbase | CEX | websocket trades (Advanced Trade `market_trades`) |
| HTX (Huobi) | CEX | websocket trades (gzip) |
| Kraken | CEX | websocket trades (v2 `trade`) |
| KuCoin | CEX | websocket trades (`/market/match`) |
| Poloniex | CEX | websocket trades (API v3) |
| Hyperliquid | DEX (perps + spot) | websocket `allMids`, every block |

Bittrex (closed) and Uniswap via The Graph hosted service (shut down) from v2 are gone.

## Layout

```
crates/cn-core       engine: price change windows, candles, relative change, alerts (no network code)
crates/cn-exchanges  Exchange trait implementations, websocket and polling feed helpers
crates/cn-cli        headless runner for development
app/src-tauri        Tauri shell: settings, engine wiring, notifications, CoinGecko info, news
app/src              React UI
```

### Adding an exchange

1. Create `crates/cn-exchanges/src/<name>.rs` implementing `cn_core::Exchange`: `markets()` (pairs with 24h
   quote volume), `candles()` (1m and 5m), `market_url()` and `feed()`.
2. For the live feed implement `ws::WsProtocol` (URL, subscribe messages, ping, decoder) and return
   `ws::ws_feed(...)`; the helper handles sharding, reconnects and keepalive. Exchanges without a websocket can
   implement `poll::PollProtocol` instead, which polls a bulk tickers endpoint and emits changed prices
   (shown as "near real time").
3. Register it in `cn_exchanges::all()` and add fixture tests for parsing plus a line in `tests/live.rs`.

## Development

Requirements: Rust (stable), Node 22 with pnpm, and on Linux
`libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev`.

```sh
cargo test --workspace                                   # unit and fixture tests
cargo test -p cn-exchanges --test live -- --ignored      # live smoke tests against real exchanges
cargo run -p cn-cli -- watch hyperliquid --quote PERP --min-vol 5e7
cd app && pnpm install && pnpm tauri dev                  # run the app
cd app && pnpm tauri build                               # installers
```

Settings are stored in `settings.json` in the app config directory (`~/.config/pl.dmotyka.cryptonose` on Linux).
Cryptonose 2 settings (thresholds, pairs, pins, blocks) are imported once on first start.

Releases are built by `.github/workflows/release.yml` when a `v*` tag is pushed.

## Credits

Alert sounds: see `app/public/sounds/LICENSE-sounds.txt`. Coin data: CoinGecko.
