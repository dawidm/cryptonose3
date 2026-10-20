//! Per-exchange engine: selects pairs, loads history, consumes the live feed and emits price
//! changes and alerts. All state is owned by a single tokio task; the outside world talks to it
//! through [`EngineHandle`] commands and [`EngineEvent`]s.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use futures::stream::BoxStream;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{interval, sleep, Instant, MissedTickBehavior};
use tracing::{debug, info, warn};

use crate::alerts::{AlertBlock, AlertChecker, AlertSettings, PriceAlert};
use crate::candles::CandleSeries;
use crate::changes::{PairChanges, PriceChanges};
use crate::exchange::{Exchange, FeedEvent, FeedState};
use crate::indicators::{liquidity_factor, relative_info, RelativeInfo};
use crate::types::{now_ms, Candle, MarketInfo, Pair, Ts};

/// Interval of history candles fetched from exchanges; longer periods are aggregated from it.
const BASE_INTERVAL_SEC: u32 = 300;
const SEED_INTERVAL_SEC: u32 = 60;
const RETRY_DELAY: Duration = Duration::from_secs(60);
const NO_PAIRS_RETRY: Duration = Duration::from_secs(300);
const CANDLE_RETRIES: usize = 2;
/// Number of base candles shown as a chart with alerts (4 hours of 5 minute candles).
const ALERT_CHART_CANDLES: usize = 48;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairCriterion {
    pub quote: String,
    /// Minimum 24h quote volume, used when `top` is not set.
    #[serde(default)]
    pub min_volume: f64,
    /// The N pairs with the highest 24h volume.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<usize>,
}

impl PairCriterion {
    pub fn volume(quote: impl Into<String>, min_volume: f64) -> Self {
        Self { quote: quote.into(), min_volume, top: None }
    }

    pub fn top(quote: impl Into<String>, n: usize) -> Self {
        Self { quote: quote.into(), min_volume: 0.0, top: Some(n) }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairSelection {
    pub criteria: Vec<PairCriterion>,
    /// Exchange symbols always included.
    pub manual: Vec<String>,
    pub blacklist: Vec<String>,
    /// Pinned symbols, always included.
    pub pinned: Vec<String>,
}

/// A watched pair stays in a top N selection until it falls below rank N × this factor, so pairs
/// around the cutoff don't change the pair list on every refresh.
pub const TOP_KEEP_FACTOR: f64 = 1.2;

impl PairSelection {
    pub fn select(&self, markets: &[MarketInfo]) -> Vec<Pair> {
        self.select_keeping(markets, &HashSet::new())
    }

    /// Like [`Self::select`], but pairs in `current` stay in top N selections while within the
    /// [`TOP_KEEP_FACTOR`] tolerance.
    pub fn select_keeping(&self, markets: &[MarketInfo], current: &HashSet<&str>) -> Vec<Pair> {
        let forced: HashSet<&str> = self.manual.iter().chain(&self.pinned).map(String::as_str).collect();
        let black: HashSet<&str> = self.blacklist.iter().map(String::as_str).collect();
        let allowed: Vec<&MarketInfo> = markets.iter().filter(|m| !black.contains(&*m.pair.symbol)).collect();
        let mut pairs: Vec<&MarketInfo> = allowed.iter().copied().filter(|m| forced.contains(&*m.pair.symbol)).collect();
        for c in &self.criteria {
            let mut quoted: Vec<&MarketInfo> = allowed.iter().copied().filter(|m| c.quote.eq_ignore_ascii_case(&m.pair.quote)).collect();
            match c.top {
                Some(n) => {
                    quoted.sort_by(|a, b| b.quote_volume_24h.total_cmp(&a.quote_volume_24h));
                    let keep_rank = (n as f64 * TOP_KEEP_FACTOR).ceil() as usize;
                    let mut rank = 0;
                    quoted.retain(|m| {
                        rank += 1;
                        rank <= n || (rank <= keep_rank && current.contains(&*m.pair.symbol))
                    });
                }
                None => quoted.retain(|m| m.quote_volume_24h >= c.min_volume),
            }
            pairs.extend(quoted);
        }
        pairs.sort_by(|a, b| a.pair.symbol.cmp(&b.pair.symbol));
        pairs.dedup_by(|a, b| a.pair.symbol == b.pair.symbol);
        pairs.into_iter().map(|m| m.pair.clone()).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineConfig {
    /// Periods for price changes, multiples of 300 seconds, shortest first.
    pub periods_sec: Vec<u32>,
    /// Number of candles used for relative change and liquidity.
    pub relative_candles: usize,
    pub selection: PairSelection,
    pub alerts: AlertSettings,
    pub blocks: Vec<AlertBlock>,
    pub refresh_interval_sec: u64,
    pub check_delay_ms: u64,
    pub no_updates_warn_sec: u64,
    pub reconnect_after_sec: u64,
}

impl EngineConfig {
    pub fn new(selection: PairSelection) -> Self {
        let periods_sec = vec![300, 1800];
        Self {
            alerts: AlertSettings::default_for(&periods_sec),
            periods_sec,
            relative_candles: 50,
            selection,
            blocks: vec![],
            refresh_interval_sec: 60 * 60,
            check_delay_ms: 250,
            no_updates_warn_sec: 300,
            reconnect_after_sec: 900,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineStatus {
    Connecting,
    Connected,
    Reconnecting,
    NoUpdates,
    NoPairs,
    Error,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodUpdate {
    pub period_sec: u32,
    pub percent_change: f64,
    pub relative_change: Option<f64>,
    pub last_percent_change: f64,
    pub relative_last_change: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairUpdate {
    pub symbol: Arc<str>,
    pub last_price: f64,
    pub periods: Vec<PeriodUpdate>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertEvent {
    pub alert: PriceAlert,
    /// Recent base interval candles, oldest first.
    pub chart: Vec<Candle>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EngineEvent {
    Status { status: EngineStatus, message: Option<String> },
    Progress { done: usize, total: usize },
    Pairs { pairs: Vec<Pair> },
    Updates { last_tick_ts: Option<Ts>, updates: Vec<PairUpdate> },
    Alert(AlertEvent),
}

enum Command {
    SetAlertSettings(AlertSettings),
    Block { symbol: Arc<str>, duration_ms: Option<i64> },
    Unblock(String),
    Blocks(oneshot::Sender<Vec<AlertBlock>>),
    Chart { symbol: String, reply: oneshot::Sender<Vec<Candle>> },
    Reconfigure(Box<EngineConfig>),
    Reconnect,
    Stop,
}

#[derive(Clone)]
pub struct EngineHandle {
    tx: mpsc::UnboundedSender<Command>,
}

impl EngineHandle {
    fn send(&self, c: Command) {
        let _ = self.tx.send(c);
    }

    pub fn set_alert_settings(&self, s: AlertSettings) {
        self.send(Command::SetAlertSettings(s));
    }

    /// Block alerts for a pair, `duration_ms = None` blocks permanently.
    pub fn block(&self, symbol: &str, duration_ms: Option<i64>) {
        self.send(Command::Block { symbol: symbol.into(), duration_ms });
    }

    pub fn unblock(&self, symbol: &str) {
        self.send(Command::Unblock(symbol.to_string()));
    }

    pub async fn blocks(&self) -> Vec<AlertBlock> {
        let (tx, rx) = oneshot::channel();
        self.send(Command::Blocks(tx));
        rx.await.unwrap_or_default()
    }

    /// Recent base interval candles of a pair, for charts.
    pub async fn chart(&self, symbol: &str) -> Vec<Candle> {
        let (tx, rx) = oneshot::channel();
        self.send(Command::Chart { symbol: symbol.to_string(), reply: tx });
        rx.await.unwrap_or_default()
    }

    /// Apply a new configuration; reloads pairs if the selection changed.
    pub fn reconfigure(&self, cfg: EngineConfig) {
        self.send(Command::Reconfigure(Box::new(cfg)));
    }

    pub fn reconnect(&self) {
        self.send(Command::Reconnect);
    }

    pub fn stop(&self) {
        self.send(Command::Stop);
    }
}

pub fn spawn(exchange: Arc<dyn Exchange>, config: EngineConfig) -> (EngineHandle, mpsc::UnboundedReceiver<EngineEvent>) {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (ev_tx, ev_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        Engine::new(exchange, config, ev_tx).run(cmd_rx).await;
    });
    (EngineHandle { tx: cmd_tx }, ev_rx)
}

struct PairState {
    pair: Pair,
    changes: PairChanges,
    /// One series per period.
    series: Vec<CandleSeries>,
    relative: Vec<Option<RelativeInfo>>,
    last_price: Option<f64>,
    dirty: bool,
}

impl PairState {
    fn new(pair: Pair, cfg: &EngineConfig) -> Self {
        let n = cfg.periods_sec.len();
        Self {
            pair,
            changes: PairChanges::new(&cfg.periods_sec),
            series: cfg.periods_sec.iter().map(|p| CandleSeries::new(*p, cfg.relative_candles)).collect(),
            relative: vec![None; n],
            last_price: None,
            dirty: false,
        }
    }

    fn update_relative(&mut self, idx: usize, n: usize) {
        let closed: Vec<Candle> = self.series[idx].closed().iter().copied().collect();
        self.relative[idx] = relative_info(&closed, n);
    }

    fn liquidity(&self, n: usize) -> Option<f64> {
        let closed: Vec<Candle> = self.series.first()?.closed().iter().copied().collect();
        (closed.len() >= n / 2).then(|| liquidity_factor(&closed, n))
    }

    fn price_changes(&self) -> Vec<PriceChanges> {
        let mut changes = self.changes.changes();
        for (c, rel) in changes.iter_mut().zip(&self.relative) {
            c.relative = *rel;
        }
        changes
    }

    fn insert(&mut self, ts: Ts, price: f64) {
        self.changes.push(ts, price);
        for s in &mut self.series {
            s.insert(ts, price);
        }
        self.last_price = Some(price);
        self.dirty = true;
    }
}

/// Aggregate base candles into candles of `period_sec`.
fn aggregate(candles: &[Candle], period_sec: u32) -> Vec<Candle> {
    let period_ms = period_sec as i64 * 1000;
    let mut out: Vec<Candle> = Vec::new();
    for c in candles {
        let start = c.ts.div_euclid(period_ms) * period_ms;
        match out.last_mut() {
            Some(last) if last.ts == start => {
                last.high = last.high.max(c.high);
                last.low = last.low.min(c.low);
                last.close = c.close;
            }
            _ => out.push(Candle { ts: start, ..*c }),
        }
    }
    out
}

/// Pseudo ticks from a candle, in a plausible order: open, low/high, close.
fn candle_ticks(c: &Candle, period_ms: i64) -> [(Ts, f64); 4] {
    let (a, b) = if c.close >= c.open { (c.low, c.high) } else { (c.high, c.low) };
    [(c.ts, c.open), (c.ts + period_ms / 3, a), (c.ts + 2 * period_ms / 3, b), (c.ts + period_ms - 1, c.close)]
}

enum Flow {
    Continue,
    Restart,
    Stop,
}

struct Engine {
    exchange: Arc<dyn Exchange>,
    cfg: EngineConfig,
    ev: mpsc::UnboundedSender<EngineEvent>,
    checker: AlertChecker,
    pairs: HashMap<Arc<str>, PairState>,
    status: Option<EngineStatus>,
}

impl Engine {
    fn new(exchange: Arc<dyn Exchange>, cfg: EngineConfig, ev: mpsc::UnboundedSender<EngineEvent>) -> Self {
        let checker = AlertChecker::new(exchange.meta().id.into(), cfg.alerts.clone(), cfg.blocks.clone());
        Self { exchange, cfg, ev, checker, pairs: HashMap::new(), status: None }
    }

    fn emit(&self, e: EngineEvent) {
        let _ = self.ev.send(e);
    }

    fn set_status(&mut self, status: EngineStatus, message: Option<String>) {
        if self.status != Some(status) || message.is_some() {
            self.status = Some(status);
            self.emit(EngineEvent::Status { status, message });
        }
    }

    fn id(&self) -> &'static str {
        self.exchange.meta().id
    }

    /// Commands that don't need the pairs loaded. Returns the flow for commands that do.
    fn handle_command(&mut self, cmd: Option<Command>) -> Flow {
        match cmd {
            None | Some(Command::Stop) => return Flow::Stop,
            Some(Command::SetAlertSettings(s)) => {
                self.cfg.alerts = s.clone();
                self.checker.set_settings(s);
            }
            Some(Command::Block { symbol, duration_ms }) => self.checker.block(symbol, duration_ms, now_ms()),
            Some(Command::Unblock(s)) => self.checker.unblock(&s),
            Some(Command::Blocks(reply)) => {
                let _ = reply.send(self.checker.blocks(now_ms()).to_vec());
            }
            Some(Command::Chart { symbol, reply }) => {
                let chart = self.pairs.get(symbol.as_str()).map(|p| p.series[0].all()).unwrap_or_default();
                let _ = reply.send(chart);
            }
            Some(Command::Reconfigure(cfg)) => {
                // pinning an already loaded pair doesn't need a reload
                let same_selection = cfg.selection.criteria == self.cfg.selection.criteria
                    && cfg.selection.manual == self.cfg.selection.manual
                    && cfg.selection.blacklist == self.cfg.selection.blacklist
                    && cfg.selection.pinned.iter().all(|p| self.cfg.selection.pinned.contains(p) || self.pairs.contains_key(p.as_str()));
                let reload = !same_selection
                    || cfg.periods_sec != self.cfg.periods_sec
                    || cfg.relative_candles != self.cfg.relative_candles;
                self.checker.set_settings(cfg.alerts.clone());
                self.cfg = *cfg;
                if reload {
                    return Flow::Restart;
                }
            }
            Some(Command::Reconnect) => return Flow::Restart,
        }
        Flow::Continue
    }

    async fn run(mut self, mut cmd_rx: mpsc::UnboundedReceiver<Command>) {
        loop {
            match self.run_once(&mut cmd_rx).await {
                Flow::Stop => break,
                Flow::Restart | Flow::Continue => continue,
            }
        }
        self.set_status(EngineStatus::Stopped, None);
        info!(exchange = self.id(), "engine stopped");
    }

    async fn run_once(&mut self, cmd_rx: &mut mpsc::UnboundedReceiver<Command>) -> Flow {
        self.set_status(EngineStatus::Connecting, None);
        self.pairs.clear();
        let pairs = loop {
            let r = tokio::select! {
                r = self.exchange.markets() => r,
                cmd = cmd_rx.recv() => match self.handle_command(cmd) {
                    Flow::Continue => continue,
                    f => return f,
                },
            };
            match r {
                Ok(markets) => {
                    let pairs = self.cfg.selection.select(&markets);
                    if !pairs.is_empty() {
                        break pairs;
                    }
                    self.emit(EngineEvent::Pairs { pairs: vec![] });
                    self.set_status(EngineStatus::NoPairs, None);
                    if let Some(f) = self.wait(cmd_rx, NO_PAIRS_RETRY).await {
                        return f;
                    }
                }
                Err(e) => {
                    warn!(exchange = self.id(), "getting markets failed: {e:#}");
                    self.set_status(EngineStatus::Error, Some(format!("Getting markets failed: {e:#}")));
                    if let Some(f) = self.wait(cmd_rx, RETRY_DELAY).await {
                        return f;
                    }
                }
            }
        };
        info!(exchange = self.id(), pairs = pairs.len(), "pairs selected");
        self.emit(EngineEvent::Pairs { pairs: pairs.clone() });
        if let Some(f) = self.load_pairs(&pairs, cmd_rx).await {
            return f;
        }
        self.live(pairs, cmd_rx).await
    }

    /// Sleep while handling commands. Returns Some if the engine should restart or stop.
    async fn wait(&mut self, cmd_rx: &mut mpsc::UnboundedReceiver<Command>, d: Duration) -> Option<Flow> {
        let deadline = Instant::now() + d;
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => return None,
                cmd = cmd_rx.recv() => match self.handle_command(cmd) {
                    Flow::Continue => {}
                    f => return Some(f),
                },
            }
        }
    }

    async fn load_pairs(&mut self, pairs: &[Pair], cmd_rx: &mut mpsc::UnboundedReceiver<Command>) -> Option<Flow> {
        let total = pairs.len();
        self.emit(EngineEvent::Progress { done: 0, total });
        for (i, pair) in pairs.iter().enumerate() {
            let fut = load_pair(self.exchange.clone(), pair.clone(), self.cfg.clone());
            tokio::pin!(fut);
            let state = loop {
                tokio::select! {
                    s = &mut fut => break s,
                    cmd = cmd_rx.recv() => match self.handle_command(cmd) {
                        Flow::Continue => {}
                        f => return Some(f),
                    },
                }
            };
            self.pairs.insert(pair.symbol.clone(), state);
            self.emit(EngineEvent::Progress { done: i + 1, total });
        }
        None
    }

    async fn live(&mut self, mut pairs: Vec<Pair>, cmd_rx: &mut mpsc::UnboundedReceiver<Command>) -> Flow {
        let mut feed: BoxStream<'static, FeedEvent> = self.exchange.feed(pairs.clone());
        let mut flush = interval(Duration::from_millis(self.cfg.check_delay_ms.max(10)));
        flush.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut second = interval(Duration::from_secs(1));
        second.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut refresh_at = Instant::now() + Duration::from_secs(self.cfg.refresh_interval_sec);
        let mut last_tick_at = Instant::now();
        let mut last_tick_ts: Option<Ts> = None;
        let mut connected = false;

        loop {
            tokio::select! {
                e = feed.next() => match e {
                    Some(FeedEvent::Ticks(ticks)) => {
                        last_tick_at = Instant::now();
                        for t in ticks {
                            if let Some(p) = self.pairs.get_mut(&t.symbol) {
                                p.insert(t.ts, t.price);
                                last_tick_ts = Some(last_tick_ts.map_or(t.ts, |l| l.max(t.ts)));
                            }
                        }
                        if !connected || self.status == Some(EngineStatus::NoUpdates) {
                            connected = true;
                            self.set_status(EngineStatus::Connected, None);
                        }
                    }
                    Some(FeedEvent::State(FeedState::Connected)) => {
                        connected = true;
                        self.set_status(EngineStatus::Connected, None);
                    }
                    Some(FeedEvent::State(FeedState::Connecting)) => {}
                    Some(FeedEvent::State(FeedState::Reconnecting(reason))) => {
                        connected = false;
                        self.set_status(EngineStatus::Reconnecting, Some(reason));
                    }
                    None => {
                        warn!(exchange = self.id(), "feed ended, restarting it");
                        sleep(Duration::from_secs(1)).await;
                        feed = self.exchange.feed(pairs.clone());
                    }
                },
                _ = flush.tick() => self.flush(last_tick_ts),
                _ = second.tick() => {
                    self.roll_candles();
                    let idle = last_tick_at.elapsed();
                    if idle >= Duration::from_secs(self.cfg.reconnect_after_sec) {
                        warn!(exchange = self.id(), "no updates for {idle:?}, reconnecting");
                        self.set_status(EngineStatus::Reconnecting, Some("No updates, reconnecting".into()));
                        feed = self.exchange.feed(pairs.clone());
                        last_tick_at = Instant::now();
                        connected = false;
                    } else if idle >= Duration::from_secs(self.cfg.no_updates_warn_sec) {
                        self.set_status(EngineStatus::NoUpdates, None);
                    }
                }
                _ = tokio::time::sleep_until(refresh_at) => {
                    refresh_at = Instant::now() + Duration::from_secs(self.cfg.refresh_interval_sec);
                    match self.refresh_pairs(&pairs, cmd_rx).await {
                        Ok(Some(new_pairs)) => {
                            pairs = new_pairs;
                            feed = self.exchange.feed(pairs.clone());
                        }
                        Ok(None) => {}
                        Err(f) => return f,
                    }
                }
                cmd = cmd_rx.recv() => match self.handle_command(cmd) {
                    Flow::Continue => {}
                    f => return f,
                },
            }
        }
    }

    /// Re-select pairs; loads new pairs and drops removed ones. Returns the new pair list if it changed.
    async fn refresh_pairs(&mut self, pairs: &[Pair], cmd_rx: &mut mpsc::UnboundedReceiver<Command>) -> Result<Option<Vec<Pair>>, Flow> {
        let markets = match self.exchange.markets().await {
            Ok(m) => m,
            Err(e) => {
                warn!(exchange = self.id(), "refreshing markets failed: {e:#}");
                return Ok(None);
            }
        };
        let current: HashSet<&str> = pairs.iter().map(|p| &*p.symbol).collect();
        let new_pairs = self.cfg.selection.select_keeping(&markets, &current);
        let old: HashSet<&str> = pairs.iter().map(|p| &*p.symbol).collect();
        let new: HashSet<&str> = new_pairs.iter().map(|p| &*p.symbol).collect();
        if old == new || new_pairs.is_empty() {
            return Ok(None);
        }
        info!(exchange = self.id(), pairs = new_pairs.len(), "pair list changed");
        self.pairs.retain(|s, _| new.contains(&**s));
        let added: Vec<Pair> = new_pairs.iter().filter(|p| !old.contains(&*p.symbol)).cloned().collect();
        if let Some(f) = self.load_pairs(&added, cmd_rx).await {
            return Err(f);
        }
        self.emit(EngineEvent::Pairs { pairs: new_pairs.clone() });
        Ok(Some(new_pairs))
    }

    fn roll_candles(&mut self) {
        let now = now_ms();
        let n = self.cfg.relative_candles;
        for p in self.pairs.values_mut() {
            for i in 0..p.series.len() {
                if p.series[i].roll(now) {
                    p.update_relative(i, n);
                }
            }
        }
    }

    fn flush(&mut self, last_tick_ts: Option<Ts>) {
        let now = now_ms();
        let n = self.cfg.relative_candles;
        let mut updates = Vec::new();
        let mut alerts = Vec::new();
        for p in self.pairs.values_mut() {
            if !p.dirty {
                continue;
            }
            p.dirty = false;
            let changes = p.price_changes();
            if changes.is_empty() {
                continue;
            }
            updates.push(PairUpdate {
                symbol: p.pair.symbol.clone(),
                last_price: p.last_price.unwrap_or(changes[0].last_price),
                periods: changes
                    .iter()
                    .map(|c| PeriodUpdate {
                        period_sec: c.period_sec,
                        percent_change: c.percent_change(),
                        relative_change: c.relative_change(),
                        last_percent_change: c.last_percent_change(),
                        relative_last_change: c.relative_last_change(),
                    })
                    .collect(),
            });
            for alert in self.checker.check(&p.pair, &changes, p.liquidity(n), now) {
                let all = p.series[0].all();
                let chart = all[all.len().saturating_sub(ALERT_CHART_CANDLES + 1)..].to_vec();
                alerts.push(AlertEvent { alert, chart });
            }
        }
        if !updates.is_empty() {
            self.emit(EngineEvent::Updates { last_tick_ts, updates });
        }
        for a in alerts {
            debug!(exchange = self.id(), pair = %a.alert.pair, change = a.alert.price_change, "alert");
            self.emit(EngineEvent::Alert(a));
        }
    }
}

async fn fetch_candles(ex: &dyn Exchange, pair: &Pair, interval: u32, from: Ts, to: Ts) -> Vec<Candle> {
    for attempt in 0..=CANDLE_RETRIES {
        match ex.candles(pair, interval, from, to).await {
            Ok(c) => return c,
            Err(e) => {
                warn!(exchange = ex.meta().id, pair = %pair, attempt, "getting candles failed: {e:#}");
                sleep(Duration::from_millis(ex.rest_delay_ms().max(1000))).await;
            }
        }
    }
    vec![]
}

/// Load history for a pair: base candles aggregated into every period, and recent 1 minute candles
/// turned into ticks so changes are available before live data arrives.
async fn load_pair(ex: Arc<dyn Exchange>, pair: Pair, cfg: EngineConfig) -> PairState {
    let now = now_ms();
    let max_period = *cfg.periods_sec.iter().max().unwrap_or(&BASE_INTERVAL_SEC);
    let delay = Duration::from_millis(ex.rest_delay_ms());
    let history_ms = (max_period as i64 * (cfg.relative_candles as i64 + 1)) * 1000;
    let base = fetch_candles(&*ex, &pair, BASE_INTERVAL_SEC, now - history_ms, now).await;
    sleep(delay).await;
    let seed = fetch_candles(&*ex, &pair, SEED_INTERVAL_SEC, now - max_period as i64 * 1000, now).await;
    sleep(delay).await;

    let mut state = PairState::new(pair, &cfg);
    for (i, period) in cfg.periods_sec.iter().enumerate() {
        let candles = if *period == BASE_INTERVAL_SEC { base.clone() } else { aggregate(&base, *period) };
        state.series[i].load(candles, now);
        state.update_relative(i, cfg.relative_candles);
    }
    let seed_ms = SEED_INTERVAL_SEC as i64 * 1000;
    let mut seed = seed;
    seed.sort_by_key(|c| c.ts);
    for c in &seed {
        for (ts, price) in candle_ticks(c, seed_ms) {
            if ts <= now {
                state.changes.push(ts, price);
            }
        }
        state.last_price = Some(c.close);
    }
    state.dirty = !seed.is_empty();
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Tick;
    use async_trait::async_trait;

    #[test]
    fn selection() {
        let m = |s: &str, q: &str, v: f64| MarketInfo { pair: Pair::new(s, "X", q), quote_volume_24h: v, last_price: None };
        let markets = vec![m("A", "USDT", 100.0), m("B", "USDT", 10.0), m("C", "BTC", 5.0), m("D", "USDT", 1000.0)];
        let sel = PairSelection {
            criteria: vec![PairCriterion::volume("usdt", 50.0)],
            manual: vec!["C".into()],
            blacklist: vec!["D".into()],
            pinned: vec!["B".into(), "Z".into()],
        };
        let syms: Vec<_> = sel.select(&markets).iter().map(|p| p.symbol.to_string()).collect();
        assert_eq!(syms, vec!["A", "B", "C"]);

        // top 1 USDT pair by volume: D is blacklisted, so A
        let sel = PairSelection { criteria: vec![PairCriterion::top("USDT", 1)], manual: vec![], blacklist: vec!["D".into()], pinned: vec![] };
        let syms: Vec<_> = sel.select(&markets).iter().map(|p| p.symbol.to_string()).collect();
        assert_eq!(syms, vec!["A"]);
        let sel = PairSelection { criteria: vec![PairCriterion::top("USDT", 10)], manual: vec![], blacklist: vec![], pinned: vec![] };
        assert_eq!(sel.select(&markets).len(), 3);
        // a watched pair that dropped just below the top N is kept within the tolerance
        let m10: Vec<MarketInfo> = (0..10).map(|i| m(&format!("P{i}"), "USDT", 100.0 - i as f64)).collect();
        let sel = PairSelection { criteria: vec![PairCriterion::top("USDT", 5)], manual: vec![], blacklist: vec![], pinned: vec![] };
        let syms = |v: Vec<Pair>| v.iter().map(|p| p.symbol.to_string()).collect::<Vec<_>>();
        assert_eq!(syms(sel.select(&m10)), vec!["P0", "P1", "P2", "P3", "P4"]);
        // rank 6 (P5) is within ceil(5 * 1.2) = 6, rank 7 (P6) is not
        let current: HashSet<&str> = ["P5", "P6"].into();
        assert_eq!(syms(sel.select_keeping(&m10, &current)), vec!["P0", "P1", "P2", "P3", "P4", "P5"]);

        // saved settings without `top` still mean minimum volume
        let c: PairCriterion = serde_json::from_str(r#"{"quote": "BTC", "minVolume": 5}"#).unwrap();
        assert_eq!(c, PairCriterion::volume("BTC", 5.0));
    }

    #[test]
    fn aggregates() {
        let c = |ts: i64, o: f64, h: f64, l: f64, cl: f64| Candle { ts: ts * 1000, open: o, high: h, low: l, close: cl };
        let agg = aggregate(&[c(1800, 1.0, 2.0, 0.5, 1.5), c(2100, 1.5, 3.0, 1.0, 2.0), c(3600, 2.0, 2.0, 2.0, 2.0)], 1800);
        assert_eq!(agg, vec![c(1800, 1.0, 3.0, 0.5, 2.0), c(3600, 2.0, 2.0, 2.0, 2.0)]);
    }

    struct Fake;

    static META: crate::exchange::ExchangeMeta = crate::exchange::ExchangeMeta {
        id: "fake",
        name: "Fake",
        color: "#888",
        kind: crate::exchange::ExchangeKind::Cex,
        feed: crate::exchange::FeedKind::Streaming,
        default_top: &[],
    };

    #[async_trait]
    impl Exchange for Fake {
        fn meta(&self) -> &crate::exchange::ExchangeMeta {
            &META
        }
        fn market_url(&self, _: &Pair) -> String {
            String::new()
        }
        async fn markets(&self) -> anyhow::Result<Vec<MarketInfo>> {
            Ok(vec![MarketInfo { pair: Pair::new("AB", "A", "B"), quote_volume_24h: 1e9, last_price: None }])
        }
        async fn candles(&self, _: &Pair, interval: u32, from: Ts, to: Ts) -> anyhow::Result<Vec<Candle>> {
            let step = interval as i64 * 1000;
            let mut out = vec![];
            let mut ts = from.div_euclid(step) * step;
            while ts < to {
                // 1% candles around 100
                out.push(Candle { ts, open: 100.0, high: 100.5, low: 99.5, close: 100.0 });
                ts += step;
            }
            Ok(out)
        }
        fn rest_delay_ms(&self) -> u64 {
            0
        }
        fn feed(&self, _: Vec<Pair>) -> BoxStream<'static, FeedEvent> {
            let now = now_ms();
            futures::stream::iter(vec![
                FeedEvent::State(FeedState::Connected),
                FeedEvent::Ticks(vec![Tick { symbol: "AB".into(), price: 110.0, ts: now }]),
            ])
            .chain(futures::stream::pending())
            .boxed()
        }
    }

    #[tokio::test]
    async fn engine_emits_updates_and_alert() {
        let sel = PairSelection { criteria: vec![PairCriterion::volume("B", 1.0)], manual: vec![], blacklist: vec![], pinned: vec![] };
        let (h, mut rx) = spawn(Arc::new(Fake), EngineConfig::new(sel));
        let mut got_alert = None;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
                Ok(Some(EngineEvent::Alert(a))) => {
                    got_alert = Some(a);
                    break;
                }
                Ok(Some(_)) => {}
                _ => break,
            }
        }
        let a = got_alert.expect("alert");
        assert!(a.alert.price_change > 9.0);
        // 10 / 1 (weighted range of 1.0) = 10 > sufficient 8
        assert!(a.alert.relative_change > 9.0);
        assert!(!a.chart.is_empty());
        assert!(!h.chart("AB").await.is_empty());
        h.stop();
    }
}
