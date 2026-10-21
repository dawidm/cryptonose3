{% include menu.md %}

# Cryptonose 3

<!-- IMAGE PLACEHOLDER: images/desktop-screenshot.png - main window: exchange tab with price table, alerts panel on the right, pinned tickers and a desktop notification -->
[![cryptonose main window with price alerts and notification]({{ site.baseurl }}/images/screenshot_main.png "Main window, price alerts")]({{ site.baseurl }}/images/screenshot_main.png)

<p style="text-align: center; margin-top: 40px; margin-bottom: 40px;"><a class="download-button" href="{{ site.baseurl }}/download/">Download Cryptonose 3</a></p>

## What is Cryptonose 3?
**TLDR: Cryptonose 3 will allert you which crypto markets are making significant moves right now.** It's mostly usefull for day trading/scalping for fast notification of which markets are making moves.

Cryptonose 3 is a **free and open source** desktop application that keeps track of significant price changes within small time periods (5 minutes, 30 minutes) on cryptocurrency markets. It works in real time, connecting directly to supported cryptocurrency exchanges, giving you instant alert when price rises or drops. It's also useful just for tracking prices and provides small charts showing price movements in the last 4 hours.

Cryptonose 3 is a complete rewrite of Cryptonose 2. It's a lighter and faster native application (Rust and Tauri instead of Java), with new exchanges, news about coins and a refreshed interface.

## Do I need Cryptonose 3?
You may want to try Cryptonose 3 if you are day trader on cryptocurrency markets. I started creating Cryptonose as personal project. I've been using it in trading for many years.

## The most important features of Cryptonose 3
* Cryptonose 3 uses **real-time connection** to an exchange. For selected cryptocurrency pairs, it uses all exchange transactions (fills) to track the price.
* Aside from tracking percent price changes, it also tracks **relative price change**. Some currency pairs are much more volatile than others. Relative price change helps to determine when price change is significant for given currency. You can find the details in manual (link at the top of the site).
* When price change meets price alert conditions Cryptonose 3 can: send **desktop notification**, play **sound**, **run web browser** with exchange website.
* Pairs can be chosen by minimum 24h volume or as **top N pairs by volume**; the list is refreshed every hour, so it follows the market.
* **Coin info and news** for every pair: data from CoinGecko and latest headlines from crypto news sites.

## Supported exchanges
Currently supported spot exchanges: **Binance, Bitfinex, Coinbase, HTX (formerly Huobi), Kraken, KuCoin, Poloniex**. Also a decentralized exchange (DEX) **Hyperliquid** is available (perpetuals and spot). All of them stream prices in real time. Another exchanges may be added in the future, but existing ones provide data for most of popular markets, so you can use Cryptonose 3 even if you trade on another exchange.