mod gecko;
mod news;
mod settings;
mod tray;
mod update;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use cn_core::alerts::{AlertBlock, AlertSettings};
use cn_core::engine::{self, EngineEvent, EngineHandle, EngineStatus, PairCriterion, PairUpdate};
use cn_core::{Candle, Exchange, ExchangeMeta, MarketInfo, Pair};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
#[cfg(not(all(unix, not(target_os = "macos"))))]
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

use crate::gecko::{CoinInfo, CoinRef, Gecko};
use crate::news::{GeckoNews, News, NewsItem, NewsProvider, RssNews};
use crate::settings::{General, Settings, SettingsStore, Toggles, MAX_PINNED};

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Last known state of a running engine, so a reloaded UI can be filled immediately.
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    status: Option<EngineStatus>,
    message: Option<String>,
    pairs: Vec<Pair>,
    rows: Vec<PairUpdate>,
    last_tick_ts: Option<i64>,
}

struct AppState {
    snapshots: Mutex<HashMap<String, Snapshot>>,
    settings: Mutex<Settings>,
    store: SettingsStore,
    engines: Mutex<HashMap<String, EngineHandle>>,
    gecko: Arc<Gecko>,
    news: RwLock<Arc<News>>,
    /// Market URLs of recent alert notifications by notification id (Windows and macOS).
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    notification_urls: Mutex<std::collections::VecDeque<(i32, String)>>,
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
const MAX_NOTIFICATION_URLS: usize = 200;

impl AppState {
    fn save(&self) {
        let s = self.settings.lock().unwrap().clone();
        if let Err(e) = self.store.save(&s) {
            tracing::error!("saving settings failed: {e:#}");
        }
    }

    fn engine(&self, id: &str) -> Option<EngineHandle> {
        self.engines.lock().unwrap().get(id).cloned()
    }
}

fn build_news(g: &General) -> News {
    let mut providers: Vec<Arc<dyn NewsProvider>> = vec![Arc::new(RssNews::new(g.news_feeds.clone()))];
    if let Some(key) = g.coingecko_pro_key.clone().filter(|k| !k.trim().is_empty()) {
        providers.push(Arc::new(GeckoNews { pro_key: key }));
    }
    News::new(providers)
}

fn exchange(id: &str) -> CmdResult<Arc<dyn Exchange>> {
    cn_exchanges::by_id(id).ok_or_else(|| format!("unknown exchange {id}"))
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusPayload {
    exchange: String,
    status: EngineStatus,
    message: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressPayload {
    exchange: String,
    done: usize,
    total: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PairsPayload {
    exchange: String,
    pairs: Vec<Pair>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdatesPayload {
    exchange: String,
    last_tick_ts: Option<i64>,
    updates: Vec<PairUpdate>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AlertPayload {
    exchange: String,
    id: u64,
    alert: cn_core::alerts::PriceAlert,
    chart: Vec<Candle>,
    url: String,
}

/// Open a URL with the configured browser command, or the system default.
fn open_url(app: &AppHandle, url: &str) -> CmdResult<()> {
    let cmd = app.state::<AppState>().settings.lock().unwrap().general.browser_command.clone();
    match cmd.filter(|c| !c.trim().is_empty()) {
        Some(c) => {
            let line = if c.contains("%s") { c.replace("%s", url) } else { format!("{c} {url}") };
            let mut parts = line.split_whitespace();
            let prog = parts.next().ok_or("empty browser command")?;
            std::process::Command::new(prog).args(parts).spawn().map_err(err)?;
            Ok(())
        }
        None => app.opener().open_url(url, None::<&str>).map_err(err),
    }
}

#[derive(Clone, Copy)]
enum NotifIcon {
    App,
    Rising,
    Falling,
}

/// Icons are written to the cache dir and passed as file paths; notification servers otherwise look
/// the icon up by app id, which fails when the app isn't installed and shows a placeholder.
fn notification_icon(app: &AppHandle, icon: NotifIcon) -> Option<String> {
    static ICONS: std::sync::OnceLock<Option<[String; 3]>> = std::sync::OnceLock::new();
    let icons = ICONS.get_or_init(|| {
        let dir = app.path().app_cache_dir().ok()?.join("notification-icons");
        std::fs::create_dir_all(&dir).ok()?;
        let files: [(&str, &[u8]); 3] = [
            ("app.png", include_bytes!("../icons/128x128.png")),
            ("rising.png", include_bytes!("../icons/notifications/rising.png")),
            ("falling.png", include_bytes!("../icons/notifications/falling.png")),
        ];
        let mut paths = Vec::new();
        for (name, bytes) in files {
            let path = dir.join(name);
            std::fs::write(&path, bytes).ok()?;
            paths.push(path.to_string_lossy().into_owned());
        }
        paths.try_into().ok()
    });
    icons.as_ref().map(|i| i[icon as usize].clone())
}

const BROWSER_ACTION: &str = "browser";

/// Show a notification; with `url` it gets a "Browser" button that opens the URL.
fn notify(app: &AppHandle, icon: NotifIcon, title: &str, body: &str, url: Option<&str>) {
    // without a usable icon file no icon is set rather than a placeholder
    let icon = notification_icon(app, icon);
    show_notification(app, icon, title, body, url);
}

#[cfg(all(unix, not(target_os = "macos")))]
fn show_notification(app: &AppHandle, icon: Option<String>, title: &str, body: &str, url: Option<&str>) {
    let (app, title, body, url) = (app.clone(), title.to_string(), body.to_string(), url.map(String::from));
    // waits until the notification is closed; some servers (GNOME) close a notification once
    // the connection that sent it is gone
    tauri::async_runtime::spawn_blocking(move || {
        let mut n = notify_rust::Notification::new();
        n.appname("Cryptonose 3").summary(&title).body(&body);
        if let Some(icon) = &icon {
            n.icon(icon);
        }
        // "default" is a click on the notification itself; KDE shows it as a button instead, so it gets a label
        n.action("default", "Show");
        if url.is_some() {
            n.action(BROWSER_ACTION, "Browser");
        }
        match n.show() {
            Ok(handle) => handle.wait_for_action(|action| match (action, &url) {
                ("default", _) => tray::show_main_window(&app),
                (BROWSER_ACTION, Some(url)) => {
                    if let Err(e) = open_url(&app, url) {
                        tracing::warn!("opening browser failed: {e}");
                    }
                }
                _ => {}
            }),
            Err(e) => tracing::warn!("notification failed: {e}"),
        }
    });
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn show_notification(app: &AppHandle, icon: Option<String>, title: &str, body: &str, url: Option<&str>) {
    static NEXT_ID: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(1);
    let mut builder = app.notification().builder().title(title).body(body);
    if let Some(icon) = icon {
        builder = builder.icon(icon);
    }
    if let Some(url) = url {
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let state = app.state::<AppState>();
        let mut urls = state.notification_urls.lock().unwrap();
        urls.push_back((id, url.to_string()));
        if urls.len() > MAX_NOTIFICATION_URLS {
            urls.pop_front();
        }
        builder = builder.id(id).action_type_id("alert");
    }
    if let Err(e) = builder.show() {
        tracing::warn!("notification failed: {e}");
    }
}

/// Windows and macOS: alert notifications get a "Browser" button that opens the market; a click on
/// any notification brings the window back (from the tray, minimized or behind other windows).
#[cfg(not(all(unix, not(target_os = "macos"))))]
fn register_notification_actions(app: &tauri::App) -> tauri::Result<()> {
    use tauri_plugin_notification::{Action, ActionType};
    app.notification()
        .register_action_types(vec![ActionType::builder("alert").actions(vec![Action::builder(BROWSER_ACTION, "Browser").build()]).build()])
        .map_err(|e| tauri::Error::Anyhow(e.into()))?;
    let handle = app.handle().clone();
    app.notification()
        .on_action(move |action| {
            if action.action_id() == "tap" {
                tray::show_main_window(&handle);
                return;
            }
            if action.action_id() != BROWSER_ACTION {
                return;
            }
            let Some(id) = action.notification().map(|n| n.id()) else { return };
            let url = handle.state::<AppState>().notification_urls.lock().unwrap().iter().find(|(i, _)| *i == id).map(|(_, u)| u.clone());
            if let Some(url) = url {
                if let Err(e) = open_url(&handle, &url) {
                    tracing::warn!("opening browser failed: {e}");
                }
            }
        })
        .map_err(|e| tauri::Error::Anyhow(e.into()))
}

fn start_engine(app: &AppHandle, id: &str) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let ex = exchange(id)?;
    let cfg = state.settings.lock().unwrap().exchange(&*ex).engine_config();
    // check and insert under one lock: concurrent calls (e.g. init twice) must not start two engines
    let rx = {
        let mut engines = state.engines.lock().unwrap();
        if engines.contains_key(id) {
            return Ok(());
        }
        let (handle, rx) = engine::spawn(ex.clone(), cfg);
        engines.insert(id.to_string(), handle);
        rx
    };
    tauri::async_runtime::spawn(forward_events(app.clone(), ex, rx));
    Ok(())
}

/// Forward engine events to the UI. Table updates are batched every 500 ms; alerts also trigger
/// notifications and opening the browser, depending on the toggles.
async fn forward_events(app: AppHandle, ex: Arc<dyn Exchange>, mut rx: tokio::sync::mpsc::UnboundedReceiver<EngineEvent>) {
    static ALERT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = ex.meta().id.to_string();
    let mut pending: HashMap<Arc<str>, PairUpdate> = HashMap::new();
    let mut latest: HashMap<Arc<str>, PairUpdate> = HashMap::new();
    let mut last_tick_ts = None;
    let snapshot = |f: &dyn Fn(&mut Snapshot)| {
        let state = app.state::<AppState>();
        let mut snaps = state.snapshots.lock().unwrap();
        f(snaps.entry(id.clone()).or_default());
    };
    let mut last_status = None;
    let mut timer = tokio::time::interval(Duration::from_millis(500));
    loop {
        tokio::select! {
            ev = rx.recv() => {
                let Some(ev) = ev else { break };
                match ev {
                    EngineEvent::Status { status, message } => {
                        let (all, errors) = {
                            let state = app.state::<AppState>();
                            let s = state.settings.lock().unwrap();
                            (s.general.connection_notifications, s.general.connection_error_notifications)
                        };
                        let changed = last_status.is_some_and(|s| s != status);
                        let error = matches!(status, EngineStatus::Error | EngineStatus::NoPairs | EngineStatus::NoUpdates);
                        // Stopped is closing the tab, Connecting is the start
                        let any = !matches!(status, EngineStatus::Stopped | EngineStatus::Connecting);
                        if changed && ((all && any) || (errors && error)) {
                            notify(&app, NotifIcon::App, &format!("{} {:?}", ex.meta().name, status), message.as_deref().unwrap_or(""), None);
                        }
                        last_status = Some(status);
                        snapshot(&|s| {
                            s.status = Some(status);
                            s.message = message.clone();
                        });
                        let _ = app.emit("cn:status", StatusPayload { exchange: id.clone(), status, message });
                        if status == EngineStatus::Stopped {
                            app.state::<AppState>().snapshots.lock().unwrap().remove(&id);
                            break;
                        }
                    }
                    EngineEvent::Progress { done, total } => {
                        let _ = app.emit("cn:progress", ProgressPayload { exchange: id.clone(), done, total });
                    }
                    EngineEvent::Pairs { pairs } => {
                        pending.clear();
                        latest.retain(|s, _| pairs.iter().any(|p| p.symbol == *s));
                        snapshot(&|s| s.pairs = pairs.clone());
                        let _ = app.emit("cn:pairs", PairsPayload { exchange: id.clone(), pairs });
                    }
                    EngineEvent::Updates { last_tick_ts: ts, updates } => {
                        last_tick_ts = ts.or(last_tick_ts);
                        for u in updates {
                            pending.insert(u.symbol.clone(), u);
                        }
                    }
                    EngineEvent::Alert(a) => {
                        let url = ex.market_url(&a.alert.pair);
                        let toggles = app.state::<AppState>().settings.lock().unwrap().toggles.clone();
                        if toggles.notification {
                            let al = &a.alert;
                            notify(
                                &app,
                                if al.price_change > 0.0 { NotifIcon::Rising } else { NotifIcon::Falling },
                                &format!("{} {:+.2}% ({}m)", al.pair.display(), al.price_change, al.period_sec / 60),
                                &format!("{} · relative {:+.1} · {} → {}", ex.meta().name, al.relative_change, al.reference_price, al.final_price),
                                Some(&url),
                            );
                        }
                        if toggles.browser {
                            if let Err(e) = open_url(&app, &url) {
                                tracing::warn!("opening browser failed: {e}");
                            }
                        }
                        let payload = AlertPayload {
                            exchange: id.clone(),
                            id: ALERT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                            alert: a.alert,
                            chart: a.chart,
                            url,
                        };
                        let _ = app.emit("cn:alert", payload);
                    }
                }
            }
            _ = timer.tick() => {
                if !pending.is_empty() {
                    let updates: Vec<PairUpdate> = pending.drain().map(|(_, u)| u).collect();
                    for u in &updates {
                        latest.insert(u.symbol.clone(), u.clone());
                    }
                    let rows: Vec<PairUpdate> = latest.values().cloned().collect();
                    snapshot(&|s| {
                        s.rows = rows.clone();
                        s.last_tick_ts = last_tick_ts;
                    });
                    let _ = app.emit("cn:updates", UpdatesPayload { exchange: id.clone(), last_tick_ts, updates });
                }
            }
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InitData {
    exchanges: Vec<ExchangeMeta>,
    settings: Settings,
    version: &'static str,
    max_pinned: usize,
    snapshots: HashMap<String, Snapshot>,
    /// Desktop UI font size in px, if it can be read.
    system_font_px: Option<f64>,
}

/// GNOME/KDE UI font size from gsettings, e.g. 'Noto Sans, 10' (points) times the text scaling factor.
#[cfg(target_os = "linux")]
fn system_font_px() -> Option<f64> {
    let get = |key: &str| {
        let out = std::process::Command::new("gsettings").args(["get", "org.gnome.desktop.interface", key]).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().trim_matches('\'').to_string())
    };
    let pt: f64 = get("font-name")?.rsplit([' ', ',']).find(|s| !s.is_empty())?.parse().ok()?;
    let scale: f64 = get("text-scaling-factor").and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let px = pt * scale * 96.0 / 72.0;
    (6.0..=40.0).contains(&px).then_some(px.round())
}

#[cfg(not(target_os = "linux"))]
fn system_font_px() -> Option<f64> {
    None
}

#[tauri::command]
async fn init(app: AppHandle, state: State<'_, AppState>) -> CmdResult<InitData> {
    let ids = state.settings.lock().unwrap().active_exchanges.clone();
    for id in ids {
        if let Err(e) = start_engine(&app, &id) {
            tracing::warn!("starting {id} failed: {e}");
        }
    }
    let settings = state.settings.lock().unwrap().clone();
    Ok(InitData {
        exchanges: cn_exchanges::all().iter().map(|e| e.meta().clone()).collect(),
        settings,
        version: env!("CARGO_PKG_VERSION"),
        max_pinned: MAX_PINNED,
        snapshots: state.snapshots.lock().unwrap().clone(),
        system_font_px: system_font_px(),
    })
}

#[tauri::command]
async fn start_exchange(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<Settings> {
    let ex = exchange(&id)?;
    {
        let mut s = state.settings.lock().unwrap();
        if !s.active_exchanges.contains(&id) {
            s.active_exchanges.push(id.clone());
        }
        s.exchange(&*ex);
    }
    state.save();
    start_engine(&app, &id)?;
    Ok(state.settings.lock().unwrap().clone())
}

#[tauri::command]
fn stop_exchange(state: State<'_, AppState>, id: String) -> Settings {
    if let Some(h) = state.engines.lock().unwrap().remove(&id) {
        h.stop();
    }
    state.settings.lock().unwrap().active_exchanges.retain(|e| e != &id);
    state.save();
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn reconnect_exchange(state: State<'_, AppState>, id: String) {
    if let Some(h) = state.engine(&id) {
        h.reconnect();
    }
}

#[tauri::command]
async fn markets(id: String) -> CmdResult<Vec<MarketInfo>> {
    exchange(&id)?.markets().await.map_err(|e| format!("{e:#}"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectionInput {
    criteria: Vec<PairCriterion>,
    manual: Vec<String>,
    blacklist: Vec<String>,
}

fn reconfigure(state: &AppState, id: &str) {
    let Ok(ex) = exchange(id) else { return };
    let cfg = state.settings.lock().unwrap().exchange(&*ex).engine_config();
    if let Some(h) = state.engine(id) {
        h.reconfigure(cfg);
    }
}

#[tauri::command]
fn save_selection(state: State<'_, AppState>, id: String, selection: SelectionInput) -> CmdResult<Settings> {
    let ex = exchange(&id)?;
    {
        let mut s = state.settings.lock().unwrap();
        let sel = &mut s.exchange(&*ex).selection;
        sel.criteria = selection.criteria;
        sel.manual = selection.manual;
        sel.blacklist = selection.blacklist;
    }
    state.save();
    reconfigure(&state, &id);
    Ok(state.settings.lock().unwrap().clone())
}

#[tauri::command]
fn save_alert_settings(state: State<'_, AppState>, id: String, alerts: AlertSettings, all_exchanges: bool) -> CmdResult<Settings> {
    let ids: Vec<String> = if all_exchanges {
        cn_exchanges::all().iter().map(|e| e.meta().id.to_string()).collect()
    } else {
        vec![id]
    };
    for id in &ids {
        let ex = exchange(id)?;
        state.settings.lock().unwrap().exchange(&*ex).alerts = alerts.clone();
        if let Some(h) = state.engine(id) {
            h.set_alert_settings(alerts.clone());
        }
    }
    state.save();
    Ok(state.settings.lock().unwrap().clone())
}

#[tauri::command]
fn block_alerts(state: State<'_, AppState>, id: String, symbol: String, duration_ms: Option<i64>) -> CmdResult<Vec<AlertBlock>> {
    let ex = exchange(&id)?;
    let now = cn_core::now_ms();
    let blocks = {
        let mut s = state.settings.lock().unwrap();
        let es = s.exchange(&*ex);
        let until = duration_ms.map(|d| now + d);
        let existing = es.blocks.iter().find(|b| *b.symbol == *symbol && b.until.is_none_or(|u| u > now));
        let longer_exists = existing.is_some_and(|b| match (b.until, until) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(a), Some(n)) => a >= n,
        });
        if !longer_exists {
            es.blocks.retain(|b| *b.symbol != *symbol && b.until.is_none_or(|u| u > now));
            es.blocks.push(AlertBlock { symbol: symbol.as_str().into(), until });
        }
        es.blocks.clone()
    };
    if let Some(h) = state.engine(&id) {
        h.block(&symbol, duration_ms);
    }
    state.save();
    Ok(blocks)
}

#[tauri::command]
fn unblock_alerts(state: State<'_, AppState>, id: String, symbol: String) -> CmdResult<Vec<AlertBlock>> {
    let ex = exchange(&id)?;
    let blocks = {
        let mut s = state.settings.lock().unwrap();
        let es = s.exchange(&*ex);
        es.blocks.retain(|b| *b.symbol != *symbol);
        es.blocks.clone()
    };
    if let Some(h) = state.engine(&id) {
        h.unblock(&symbol);
    }
    state.save();
    Ok(blocks)
}

#[tauri::command]
fn alert_blocks(state: State<'_, AppState>) -> HashMap<String, Vec<AlertBlock>> {
    let now = cn_core::now_ms();
    let s = state.settings.lock().unwrap();
    s.exchanges
        .iter()
        .map(|(id, es)| (id.clone(), es.blocks.iter().filter(|b| b.until.is_none_or(|u| u > now)).cloned().collect::<Vec<_>>()))
        .filter(|(_, b)| !b.is_empty())
        .collect()
}

#[tauri::command]
fn set_pinned(state: State<'_, AppState>, id: String, symbol: String, pinned: bool) -> CmdResult<Settings> {
    let ex = exchange(&id)?;
    {
        let mut s = state.settings.lock().unwrap();
        // pins of closed exchanges aren't shown, so they don't count
        let total: usize = s.active_exchanges.iter().filter_map(|e| s.exchanges.get(e)).map(|e| e.selection.pinned.len()).sum();
        let list = &mut s.exchange(&*ex).selection.pinned;
        if pinned && !list.contains(&symbol) {
            if total >= MAX_PINNED {
                return Err(format!("You can pin up to {MAX_PINNED} pairs"));
            }
            list.push(symbol);
        } else if !pinned {
            list.retain(|p| p != &symbol);
        }
    }
    state.save();
    reconfigure(&state, &id);
    Ok(state.settings.lock().unwrap().clone())
}

#[tauri::command]
async fn chart(state: State<'_, AppState>, id: String, symbol: String) -> CmdResult<Vec<Candle>> {
    let h = state.engine(&id).ok_or("exchange not running")?;
    Ok(h.chart(&symbol).await)
}

#[tauri::command]
fn open_market(app: AppHandle, id: String, pair: Pair) -> CmdResult<()> {
    let url = exchange(&id)?.market_url(&pair);
    open_url(&app, &url)
}

#[tauri::command]
fn open_link(app: AppHandle, url: String) -> CmdResult<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("only http(s) links can be opened".into());
    }
    open_url(&app, &url)
}

#[tauri::command]
async fn save_general(app: AppHandle, state: State<'_, AppState>, general: General) -> CmdResult<Settings> {
    let rebuild_news = {
        let mut s = state.settings.lock().unwrap();
        let rebuild = s.general.news_feeds != general.news_feeds || s.general.coingecko_pro_key != general.coingecko_pro_key;
        s.general = general.clone();
        rebuild
    };
    state.gecko.set_demo_key(general.coingecko_demo_key.clone()).await;
    if rebuild_news {
        *state.news.write().unwrap() = Arc::new(build_news(&general));
    }
    state.save();
    tray::sync(&app);
    Ok(state.settings.lock().unwrap().clone())
}

#[tauri::command]
fn save_toggles(app: AppHandle, state: State<'_, AppState>, toggles: Toggles) -> Settings {
    state.settings.lock().unwrap().toggles = toggles;
    state.save();
    tray::sync(&app);
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn set_flags(state: State<'_, AppState>, hidden_version: Option<String>, many_pairs_warning_shown: Option<bool>) -> Settings {
    {
        let mut s = state.settings.lock().unwrap();
        if hidden_version.is_some() {
            s.hidden_version = hidden_version;
        }
        if let Some(v) = many_pairs_warning_shown {
            s.many_pairs_warning_shown = v;
        }
    }
    state.save();
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
async fn coin_info(state: State<'_, AppState>, symbol: String) -> CmdResult<Option<CoinInfo>> {
    state.gecko.info(&symbol).await.map_err(|e| format!("{e:#}"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CoinNews {
    coin: CoinRef,
    items: Vec<NewsItem>,
}

#[tauri::command]
async fn coin_news(state: State<'_, AppState>, symbol: String) -> CmdResult<CoinNews> {
    let coin = state.gecko.coin_ref(&symbol).await;
    let news = state.news.read().unwrap().clone();
    let items = news.news(&coin).await;
    Ok(CoinNews { coin, items })
}

#[tauri::command]
async fn check_update() -> CmdResult<Option<update::Release>> {
    update::check(env!("CARGO_PKG_VERSION")).await.map_err(|e| format!("{e:#}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn,cn_core=info,cryptonose_lib=info".into()))
        .init();
    tauri::Builder::default()
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            let store = SettingsStore::new(&dir);
            let settings = store.load();
            let gecko = Arc::new(Gecko::new(settings.general.coingecko_demo_key.clone()));
            let news = RwLock::new(Arc::new(build_news(&settings.general)));
            app.manage(AppState {
                snapshots: Mutex::new(HashMap::new()),
                settings: Mutex::new(settings),
                store,
                engines: Mutex::new(HashMap::new()),
                gecko,
                news,
                #[cfg(not(all(unix, not(target_os = "macos"))))]
                notification_urls: Mutex::new(Default::default()),
            });
            #[cfg(not(all(unix, not(target_os = "macos"))))]
            register_notification_actions(app)?;
            tray::sync(app.handle());
            Ok(())
        })
        .on_window_event(tray::on_window_event)
        .invoke_handler(tauri::generate_handler![
            init,
            start_exchange,
            stop_exchange,
            reconnect_exchange,
            markets,
            save_selection,
            save_alert_settings,
            block_alerts,
            unblock_alerts,
            alert_blocks,
            set_pinned,
            chart,
            open_market,
            open_link,
            save_general,
            save_toggles,
            set_flags,
            coin_info,
            coin_news,
            check_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Cryptonose 3");
}
