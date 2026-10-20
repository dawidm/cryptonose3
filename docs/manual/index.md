{% include menu.md %}

# Manual

I've tried to make Cryptonose 3 easy to use, but in case I've failed, here is the manual ;)

## First run
After first run, you will see main application window with no exchanges and the alerts panel on the right. Choose an exchange from the list in the middle of the window to start tracking price changes. To add more exchanges later, click on _+ Exchange_ above the table. If you used Cryptonose 2 on Linux before, its settings are imported automatically: open exchanges, alert conditions, pairs, pinned tickers and blocked pairs.

Each exchange starts with a default set of currency pairs (the top pairs by 24h volume of its main markets), so you will see prices and alerts right away. To change the pairs, click _Pairs_ in the exchange tab. There are two sections in pairs selection window:

* ***Markets***. Most of the exchanges provide markets with more than one quote currency. Example of BTC markets (BTC as quote currency): ETH/BTC, LTC/BTC, XMR/BTC. Mark a market as _Active_ and choose a _Condition_:
  * _Top N by volume_ - watch the N pairs of this market with the highest 24h volume,
  * _Min 24h volume_ - watch all pairs with at least this 24h volume (in the quote currency).
* ***All pairs***. All pairs of the exchange, sorted by 24h volume, with a filter field. The _Watched_ column shows what will be watched with current settings. Use _Always_ to watch a pair regardless of the markets conditions, and _Never_ to exclude a pair (for example a stablecoin pair).

The number of pairs that will be watched is shown at the bottom of the window. Click _Save and reload_ to apply.

The list of pairs is **refreshed every hour**, so _Top N_ follows volume changes. To avoid constant changes, a pair that is already watched stays until it falls about 20% below the cutoff (for example below the 24th place for top 20).

<!-- IMAGE PLACEHOLDER: images/pairs-settings.png - Pairs window: markets with condition (Top N / Min 24h volume) and the all pairs list -->
![cryptonose pairs configuration]({{ site.baseurl }}/images/pairs-settings.png "Pairs configuration window")

**Note that choosing a lot of currency pairs will cause longer initialization** (Cryptonose 3 needs to get chart data from the exchange). Some exchanges have strict limits for downloading chart data - getting data for one currency pair takes about 2 seconds on Kraken and about 4 seconds on Bitfinex. You can see the progress in the exchange tab.  
Very low volume (so with low liquidity) currency pairs could trigger a lot of undesirable price alerts. You should choose currency pairs carefully.

## Main window
<!-- IMAGE PLACEHOLDER: images/main-window.png - whole main window with header, pinned tickers, exchange tabs and alerts panel -->
[![cryptonose main window]({{ site.baseurl }}/images/main-window.png "Main window")]({{ site.baseurl }}/images/main-window.png)

* ***Header*** - global options, search and settings:
  * _Sound_ - play a sound when price alert occurs
  * _Browser_ - run web browser with adequate market on exchange website
  * _Notify_ - show desktop notification with alert details
  * _Power save_ - hide tables and pinned tickers - saving CPU and battery, especially when prices are very volatile. Alerts still work.
  * _Find pair (F)_ - quick search for a currency pair in all open exchanges, see [search](#search-for-a-currency-pair)
  * _Settings_ and _?_ (help, keyboard shortcuts)
* ***Pinned tickers*** - a bar with real-time prices of pinned pairs, see [pinned tickers](#pinned-tickers).
* ***Exchange tabs*** - every exchange runs in separate tab. Near the exchange name there's a little dot showing connection status: green - connected, yellow - connecting, reconnecting or no updates, red - error or no pairs. DEX exchanges are marked with a _DEX_ label. Close a tab with _×_.
* ***Alerts panel*** - recent price alerts from all exchanges, on the right side of the window (below the tables in a narrow window).

## Alerts panel
<!-- IMAGE PLACEHOLDER: images/cryptonose-price-alerts.png - alerts panel with several rising and falling alerts -->
[![cryptonose price alerts]({{ site.baseurl }}/images/cryptonose-price-alerts.png "Alerts panel")]({{ site.baseurl }}/images/cryptonose-price-alerts.png)

The panel shows up to 100 latest alerts, newest at the top. _Clear_ removes all of them (after a confirmation), _Blocked pairs_ shows the [alert blocks](#blocking-alerts).

### What is relative price change?
Relative price change is an indicator reflecting price change in given period in comparison to the typical price change in the same time periods in past. The "past" is set to 50 periods. More technically, it's the price change divided by a weighted average of differences between high and low of past periods (candles) - more recent candles have bigger weight. Candles where the price didn't move are skipped.
To visualize, example values of relative price change:
* around 1 is typical value
* 4 is pretty much - means that probably something interesting is happening with the price
* 8 is very much - almost surely something interesting is happening with the price

### Price alert
<!-- IMAGE PLACEHOLDER: images/cryptonose-price-alert.png - a single alert row (zoomed) -->
[![cryptonose price alert]({{ site.baseurl }}/images/cryptonose-price-alert.png "Price alert")]({{ site.baseurl }}/images/cryptonose-price-alert.png)

* ***Arrow*** - green up arrow for a price rise, red down arrow for a drop.
* ***Pair*** - currency pair name in format _base/quote_ (perpetuals on Hyperliquid: _base-PERP_).
* ***Price change*** - percent change in the price in given period, and _rel_ - relative price change.
* ***Exchange*** - colored label with the exchange name.
* ***Period*** - 5m or 30m, and the real duration of the price change (_in 2m 45s_).
* ***Prices*** - the price before and after the change. Example: if price alert is triggered by price drop from 0.15 to 0.14, you'll see 0.15 → 0.14.
* ***Time*** of the alert.
* ***Chart*** - minimalistic graph showing short (4 hours) price history, with a dashed line at the price before the change. Just to illustrate a recent trend.
* ***Buttons*** - _CG_ and _N_ show [coin info and news](#coin-info-and-news), _Block_ blocks alerts for the pair.

Click on the alert to open the market on the exchange website.

### Blocking alerts

You can block subsequent alerts for currency pairs using _Block_ button on the alert. Blocked means that you will not see or hear any price alert on given currency pair. You can block alerts for 1 hour, 4 hours, 12 hours, 1 day or permanently. All blocks are kept when you close Cryptonose 3 and run it again.

You can see and remove alert blocks by clicking on _Blocked pairs_ in the alerts panel.

## Exchange tab
<!-- IMAGE PLACEHOLDER: images/cryptonose-exchange.png - exchange tab with toolbar and price table -->
[![cryptonose exchange tab]({{ site.baseurl }}/images/cryptonose-exchange.png "Main window, exchange tab")]({{ site.baseurl }}/images/cryptonose-exchange.png)

* ***Toolbar*** - connection status (with a message when something is wrong), progress of loading price history, number of pairs and time of the last update. Buttons:
  * _Pairs_ - [pairs selection](#first-run),
  * _Alert conditions_ - [alert conditions](#alerts-conditions) for this exchange,
  * _Reconnect_ - reload pairs and reconnect.
* ***Table*** - columns: pair, last price, 5m and 30m percent change, 5m and 30m relative change (there was not enough place for the word "change" ;)), and buttons: _CG_, _N_ ([coin info and news](#coin-info-and-news)) and ☆ to pin the pair. Click on a column name to sort; the table is re-sorted every few seconds so rows don't jump all the time. You can open web browser with market web page by double-clicking on a row. You can also select a row with arrow keys and press Enter to open the market or Space to pin it.

### What exactly are price changes in Cryptonose 3?
Price change in Cryptonose 3 is based on maximum (positive or negative) change during recent time period (5 minutes or 30 minutes). You should not confuse it with a change between beginning and ending of the period. For example if price 5 minutes ago was 2.0, then it went up to 4.0 and now it dropped slightly to 3.7, 5m price change values will be calculated for biggest rise - from 2.0 to 4.0 - in percents it's +100%.

## Search for a currency pair

<!-- IMAGE PLACEHOLDER: images/cryptonose-search.png - search results from several exchanges -->
[![cryptonose search]({{ site.baseurl }}/images/cryptonose-search.png "Cryptonose 3 search")]({{ site.baseurl }}/images/cryptonose-search.png)

The _Find pair (F)_ field is always visible at the top of the Cryptonose 3 window. You can click on it or just press F key to start typing, to find desired currency pair in all open exchanges. Results show the same columns as exchange tables, with a colored mark of the exchange. Use arrow keys to select a pair and press Enter to go to its page, or double-click it.

When you've finished your search, press ESC or clear the field.

## Pinned tickers

<!-- IMAGE PLACEHOLDER: images/pinned-tickers.png - pinned tickers bar -->
![cryptonose pinned tickers]({{ site.baseurl }}/images/pinned-tickers.png "Pinned tickers")

In exchange tables and search results you can see a ☆ button. If you pin currency pair, you will see a small box with real-time price, price change in last 4 hours and mini-chart - that's a _pinned ticker_. Tickers will appear in the same order as their currency pairs were pinned. Pinned pairs are always watched, even if they don't meet pairs conditions. Double-click a ticker to open the market, click _×_ to unpin it.

You can pin up to 10 pairs on open exchanges.

## Coin info and news
You can find small _CG_ and _N_ buttons on the alerts panel and in currency pairs tables. They open a window with two tabs:

* **Info** (_CG_ button) - additional data about coin's price, market cap, rank and usually a short description, with links to CoinGecko and the coin's website. All the data are from CoinGecko website. The data are downloaded using coin's symbol (BTC, ETH, DOGE etc.). If there's a situation when there are multiple coins with the same symbol (which is not very likely), data for a coin with highest volume will be shown. So keep in mind there's a minimal chance that data for wrong coin will be shown.
* **News** (_N_ button) - latest news about a coin from the last 7 days. Headlines come from news sites RSS feeds: CoinDesk, Cointelegraph, Decrypt, The Block and Bitcoin Magazine (you can change the list in settings), and from CoinGecko if you have a paid CoinGecko API key. News are found by the coin name or symbol. For symbols that are also common words (ONE, GAS etc.), only the coin name or _$SYMBOL_ is used. Small coins may have no news.

<!-- IMAGE PLACEHOLDER: images/cryptonose-coingecko.png - coin info window (Info tab) -->
![coingecko coin info]({{ site.baseurl }}/images/cryptonose-coingecko.png "Coin info")

<!-- IMAGE PLACEHOLDER: images/cryptonose-news.png - coin news window (News tab) -->
![coin news]({{ site.baseurl }}/images/cryptonose-news.png "Coin news")

## Alerts conditions
The predefined values should be useful. You may want to try them before considering any changes. Try starting with the _Low_ preset and then switching to the next presets if the alerts are to frequent. Conditions are set for each exchange separately; use _Save for all exchanges_ to use the same values everywhere.

<!-- IMAGE PLACEHOLDER: images/cryptonose-thresholds.png - alert conditions window -->
![cryptonose alert conditions]({{ site.baseurl }}/images/cryptonose-thresholds.png "Alert conditions")

You can enable or disable 5 minute and 30 minute alerts separately. Price alert is triggered by:

* significant percent **and** relative price changes (_Required..._ fields)
* significant relative change only (_Sufficient..._ fields).

#### Example

With _Required % change_ 3, _Required relative change_ 4 and _Sufficient relative change_ 8:

* Price rise 3.1%, relative 4.1 will trigger an alert.
* Price drop -3.1%, relative -4.1 will trigger an alert.
* Price rise 3.1%, relative 2.0 will not trigger an alert - relative change is too low.
* Price rise 1.0%, relative 9.3 will trigger the alert - relative change is bigger than _sufficient_ value.

These settings are quite flexible. If you're only interested in percent changes, you can set the required relative change to 0, and sufficient relative change to a highest possible value, which is 100.

A quick price change (5 minutes or shorter) doesn't trigger 30 minute alert when 5 minute alerts are enabled, because it's already reported by a 5 minute alert.

#### Alerts logic settings

* ***Block repeated alerts for the same pair for 30 minutes***. You can use this to make alerts less frequent, but keep in mind that during this time something interesting could happen, and you won't be notified.
* ***Still alert when the change is at least 2× the previous alert***. For example if you get alert for a significant price rise and after a few seconds the price rises even more, more than 2x than the previous change - you'll get a second alert.

#### Minimum liquidity
Liquidity factor is a very simple indicator that could help filter out alerts on currency pairs that are not liquid (and because of that - usually volatile in undesirable way). Liquidity factor is:

(number-of-5m-periods-where-price-changed) / (number-of-5m-periods)

Number of periods is 50, same as for calculating [relative change](#what-is-relative-price-change). Alerts on pairs with liquidity factor below the _Minimum liquidity_ value (by default 0.25) are skipped.

To sum up, low value of liquidity factor for given currency pair means that there were recently many periods when price didn't change.

## Notification

<!-- IMAGE PLACEHOLDER: images/notification.png - desktop notification of a price alert with the Browser button -->
![cryptonose notification]({{ site.baseurl }}/images/notification.png "cryptonose desktop notification")

Notification shows details of price alert: pair, percent change, period, exchange, relative change, and the prices before and after the change. A green or red icon shows if the price went up or down. **Click the _Browser_ button to open the market** in web browser (on Windows and MacOS you can also click on the notification).

If you enable it in settings, Cryptonose 3 also notifies you about connection problems.

## Settings

<!-- IMAGE PLACEHOLDER: images/settings.png - settings window -->
![cryptonose settings]({{ site.baseurl }}/images/settings.png "Settings")

* ***Appearance*** - theme (system, dark or light) and font size (system or custom).
* ***Alerts*** - custom sounds for rising and falling prices, a browser command (for example `firefox --new-tab %s`, where `%s` is replaced with the page address; system default browser if empty) and notifications about connection problems.
* ***Coin info and news*** - optional CoinGecko API keys (a free Demo key raises CoinGecko limits, a paid key adds CoinGecko news) and the list of news feeds.

## Keyboard shortcuts

| Key | Action |
|---|---|
| Ctrl+S | Toggle alert sound |
| Ctrl+B | Toggle opening the browser on alerts |
| Ctrl+N | Toggle alert notifications |
| Ctrl+P | Toggle power save |
| F | Find a pair in all exchanges |
| E | Next exchange tab |
| ↑ ↓ | Move in the table |
| Enter | Open the selected market |
| Space | Pin or unpin the selected pair |

## Comments for Hyperliquid support
Hyperliquid is a decentralized exchange with its own blockchain. Cryptonose 3 supports its perpetual futures (shown as _BTC-PERP_ etc., market _PERP_ in pairs settings) and spot markets (mostly quoted in USDC). Prices are mid prices (between the best buy and sell offers), streamed for every block of the Hyperliquid blockchain, so they are updated in real time.
