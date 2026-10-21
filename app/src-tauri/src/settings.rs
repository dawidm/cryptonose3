use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cn_core::alerts::{AlertBlock, AlertSettings};
use cn_core::engine::{EngineConfig, PairCriterion, PairSelection};
use cn_core::{now_ms, Exchange};
use serde::{Deserialize, Serialize};

pub const MAX_PINNED: usize = 10;

pub const DEFAULT_NEWS_FEEDS: &[&str] = &[
    "https://www.coindesk.com/arc/outboundfeeds/rss/",
    "https://cointelegraph.com/rss",
    "https://decrypt.co/feed",
    "https://www.theblock.co/rss.xml",
    "https://bitcoinmagazine.com/.rss/full/",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct General {
    /// Command used to open market pages, `%s` is replaced with the URL. System browser if empty.
    pub browser_command: Option<String>,
    /// Custom alert sound files; bundled sounds if empty.
    pub sound_rising: Option<String>,
    pub sound_falling: Option<String>,
    /// Font size in px, `None` follows the system.
    pub font_size: Option<u32>,
    /// "system", "dark" or "light".
    pub theme: String,
    /// Notify about every connection status change.
    pub connection_notifications: bool,
    /// Notify about connection errors only (included in `connection_notifications`).
    pub connection_error_notifications: bool,
    /// Experimental: tray icon, closing the window hides it to the tray.
    #[serde(default)]
    pub close_to_tray: bool,
    /// Optional CoinGecko Demo API key (higher rate limits).
    pub coingecko_demo_key: Option<String>,
    /// Optional paid CoinGecko API key, enables CoinGecko news.
    pub coingecko_pro_key: Option<String>,
    pub news_feeds: Vec<String>,
}

impl Default for General {
    fn default() -> Self {
        Self {
            browser_command: None,
            sound_rising: None,
            sound_falling: None,
            font_size: None,
            theme: "system".into(),
            connection_notifications: false,
            connection_error_notifications: true,
            close_to_tray: false,
            coingecko_demo_key: None,
            coingecko_pro_key: None,
            news_feeds: DEFAULT_NEWS_FEEDS.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// Global alert action switches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Toggles {
    pub sound: bool,
    pub browser: bool,
    pub notification: bool,
    pub power_save: bool,
}

impl Default for Toggles {
    fn default() -> Self {
        Self { sound: true, browser: false, notification: true, power_save: false }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeSettings {
    pub selection: PairSelection,
    pub alerts: AlertSettings,
    /// Timed and permanent alert blocks.
    #[serde(default)]
    pub blocks: Vec<AlertBlock>,
}

impl ExchangeSettings {
    pub fn default_for(ex: &dyn Exchange) -> Self {
        let selection = PairSelection {
            criteria: ex
                .meta()
                .default_top
                .iter()
                .map(|(q, n)| PairCriterion::top(*q, *n))
                .collect(),
            manual: vec![],
            blacklist: vec![],
            pinned: vec![],
        };
        let cfg = EngineConfig::new(selection.clone());
        Self { selection, alerts: cfg.alerts, blocks: vec![] }
    }

    pub fn engine_config(&self) -> EngineConfig {
        let now = now_ms();
        let mut cfg = EngineConfig::new(self.selection.clone());
        cfg.alerts = self.alerts.clone();
        cfg.blocks = self.blocks.iter().filter(|b| b.until.is_none_or(|u| u > now)).cloned().collect();
        cfg
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub general: General,
    pub toggles: Toggles,
    /// Exchanges with an open tab, in tab order.
    pub active_exchanges: Vec<String>,
    pub exchanges: BTreeMap<String, ExchangeSettings>,
    pub hidden_version: Option<String>,
    pub many_pairs_warning_shown: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            general: General::default(),
            toggles: Toggles::default(),
            active_exchanges: vec![],
            exchanges: BTreeMap::new(),
            hidden_version: None,
            many_pairs_warning_shown: false,
        }
    }
}

impl Settings {
    pub fn exchange(&mut self, ex: &dyn Exchange) -> &mut ExchangeSettings {
        self.exchanges.entry(ex.meta().id.to_string()).or_insert_with(|| ExchangeSettings::default_for(ex))
    }
}

pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(dir: &Path) -> Self {
        Self { path: dir.join("settings.json") }
    }

    pub fn load(&self) -> Settings {
        match std::fs::read_to_string(&self.path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
                tracing::error!("invalid settings file {}: {e}", self.path.display());
                let _ = std::fs::copy(&self.path, self.path.with_extension("json.bad"));
                Settings::default()
            }),
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self, s: &Settings) -> anyhow::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(s)?)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_defaults_for_missing_fields() {
        let mut s = Settings::default();
        s.exchange(&cn_exchanges::binance::Binance).blocks.push(AlertBlock { symbol: "X".into(), until: None });
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
        let partial: Settings = serde_json::from_str(r#"{"toggles": {"sound": false}}"#).unwrap();
        assert!(!partial.toggles.sound);
        assert!(partial.toggles.notification);
        assert_eq!(partial.general.font_size, None);
    }

    #[test]
    fn expired_blocks_not_passed_to_engine() {
        let mut s = ExchangeSettings::default_for(&cn_exchanges::binance::Binance);
        s.blocks = vec![AlertBlock { symbol: "A".into(), until: Some(1) }, AlertBlock { symbol: "B".into(), until: None }];
        let cfg = s.engine_config();
        assert_eq!(cfg.blocks.len(), 1);
        assert_eq!(&*cfg.blocks[0].symbol, "B");
    }
}
