//! Price alerts: thresholds, de-duplication of subsequent alerts and per-pair blocks.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::changes::PriceChanges;
use crate::types::{Pair, Ts};

/// How long subsequent alerts for the same pair and period are blocked (if enabled).
pub const ALERTS_PAUSE_MS: i64 = 1800 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodThresholds {
    pub period_sec: u32,
    pub enabled: bool,
    /// Required % change.
    pub required_rise: f64,
    pub required_fall: f64,
    /// Required relative change (together with the required % change).
    pub required_rel_rise: f64,
    pub required_rel_fall: f64,
    /// Relative change that triggers an alert on its own.
    pub sufficient_rel_rise: f64,
    pub sufficient_rel_fall: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThresholdPreset {
    Low,
    Medium,
    High,
}

impl PeriodThresholds {
    fn symmetric(period_sec: u32, pct: f64, rel: f64, suff: f64) -> Self {
        Self {
            period_sec,
            enabled: true,
            required_rise: pct,
            required_fall: pct,
            required_rel_rise: rel,
            required_rel_fall: rel,
            sufficient_rel_rise: suff,
            sufficient_rel_fall: suff,
        }
    }

    pub fn default_for(period_sec: u32) -> Self {
        if period_sec <= 300 {
            Self::symmetric(period_sec, 3.0, 4.0, 8.0)
        } else {
            Self::symmetric(period_sec, 5.0, 4.0, 8.0)
        }
    }

    pub fn preset(period_sec: u32, preset: ThresholdPreset) -> Self {
        let short = period_sec <= 300;
        let (pct, rel, suff) = match (preset, short) {
            (ThresholdPreset::Low, true) => (3.0, 3.0, 6.0),
            (ThresholdPreset::Medium, true) => (4.0, 3.5, 8.0),
            (ThresholdPreset::High, true) => (5.0, 4.0, 10.0),
            (ThresholdPreset::Low, false) => (6.0, 3.0, 6.0),
            (ThresholdPreset::Medium, false) => (8.0, 3.5, 8.0),
            (ThresholdPreset::High, false) => (10.0, 4.0, 10.0),
        };
        Self::symmetric(period_sec, pct, rel, suff)
    }

    fn triggers(&self, pct: f64, rel: f64) -> bool {
        (pct > self.required_rise && rel >= self.required_rel_rise)
            || rel >= self.sufficient_rel_rise
            || (pct < -self.required_fall && rel <= -self.required_rel_fall)
            || rel <= -self.sufficient_rel_fall
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertSettings {
    /// Sorted by period, shortest first.
    pub periods: Vec<PeriodThresholds>,
    pub min_liquidity_enabled: bool,
    pub min_liquidity: f64,
    pub block_subsequent: bool,
    pub allow_subsequent_2x: bool,
}

impl AlertSettings {
    pub fn default_for(periods_sec: &[u32]) -> Self {
        Self {
            periods: periods_sec.iter().map(|p| PeriodThresholds::default_for(*p)).collect(),
            min_liquidity_enabled: true,
            min_liquidity: 0.25,
            block_subsequent: true,
            allow_subsequent_2x: true,
        }
    }

    fn thresholds(&self, period_sec: u32) -> Option<&PeriodThresholds> {
        self.periods.iter().find(|t| t.period_sec == period_sec)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceAlert {
    pub exchange: Arc<str>,
    pub pair: Pair,
    pub period_sec: u32,
    /// Timestamp of the later of min/max.
    pub ts: Ts,
    /// % change of the last price from the reference (min or max).
    pub price_change: f64,
    pub relative_change: f64,
    pub reference_price: f64,
    pub final_price: f64,
    pub change_time_sec: i64,
    pub final_price_ts: Ts,
    pub reference_price_ts: Ts,
}

impl PriceAlert {
    pub fn is_rising(&self) -> bool {
        self.price_change > 0.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertBlock {
    pub symbol: Arc<str>,
    /// Wall clock end of the block, `None` for a permanent block.
    pub until: Option<Ts>,
}

impl AlertBlock {
    fn active(&self, now: Ts) -> bool {
        self.until.is_none_or(|u| now < u)
    }
}

pub struct AlertChecker {
    exchange: Arc<str>,
    settings: AlertSettings,
    previous: HashMap<(Arc<str>, u32), PriceAlert>,
    blocks: Vec<AlertBlock>,
}

impl AlertChecker {
    pub fn new(exchange: Arc<str>, settings: AlertSettings, blocks: Vec<AlertBlock>) -> Self {
        Self { exchange, settings, previous: HashMap::new(), blocks }
    }

    pub fn settings(&self) -> &AlertSettings {
        &self.settings
    }

    pub fn set_settings(&mut self, settings: AlertSettings) {
        self.settings = settings;
    }

    /// Check changes of a single pair. `liquidity` is the liquidity factor of the pair, if known.
    pub fn check(&mut self, pair: &Pair, changes: &[PriceChanges], liquidity: Option<f64>, now: Ts) -> Vec<PriceAlert> {
        let mut alerts = Vec::new();
        if self.settings.min_liquidity_enabled && liquidity.is_some_and(|l| l < self.settings.min_liquidity) {
            return alerts;
        }
        let shortest = self.settings.periods.first().copied();
        for ch in changes {
            let Some(th) = self.settings.thresholds(ch.period_sec).copied() else { continue };
            if !th.enabled {
                continue;
            }
            // a quick move inside a longer period is already reported by the shorter period
            if let Some(s) = shortest {
                if s.enabled && ch.period_sec > s.period_sec && ch.change_time_sec() <= s.period_sec as i64 {
                    continue;
                }
            }
            let Some(rel) = ch.relative_last_change() else { continue };
            let pct = ch.last_percent_change();
            if !th.triggers(pct, rel) {
                continue;
            }
            let reference_ts = ch.reference_to_last_ts();
            let alert = PriceAlert {
                exchange: self.exchange.clone(),
                pair: pair.clone(),
                period_sec: ch.period_sec,
                ts: ch.final_ts(),
                price_change: pct,
                relative_change: rel,
                reference_price: ch.reference_to_last_price(),
                final_price: ch.last_price,
                change_time_sec: (ch.last_ts - ch.min_ts.min(ch.max_ts)) / 1000,
                final_price_ts: ch.last_ts,
                reference_price_ts: reference_ts,
            };
            if self.is_subsequent(&alert) || self.is_blocked(&pair.symbol, now) {
                continue;
            }
            alerts.push(alert);
        }
        alerts
    }

    /// Returns true if a previous alert for the same pair and period suppresses this one. Otherwise
    /// remembers the alert.
    fn is_subsequent(&mut self, alert: &PriceAlert) -> bool {
        let key = (alert.pair.symbol.clone(), alert.period_sec);
        let allow = match self.previous.get(&key) {
            None => true,
            Some(old) => {
                if self.settings.allow_subsequent_2x && alert.price_change.abs() >= 2.0 * old.price_change.abs() {
                    true
                } else if self.settings.block_subsequent {
                    alert.ts - old.ts > ALERTS_PAUSE_MS
                } else {
                    // direction reversed after the previous alert
                    alert.reference_price_ts >= old.final_price_ts
                        && alert.change_time_sec != 0
                        && alert.price_change * old.price_change < 0.0
                }
            }
        };
        if allow {
            self.previous.insert(key, alert.clone());
        }
        !allow
    }

    pub fn is_blocked(&mut self, symbol: &str, now: Ts) -> bool {
        self.blocks.retain(|b| b.active(now));
        self.blocks.iter().any(|b| &*b.symbol == symbol)
    }

    /// Block alerts for a pair; `duration_ms = None` blocks permanently. A longer existing block wins.
    pub fn block(&mut self, symbol: Arc<str>, duration_ms: Option<i64>, now: Ts) {
        let until = duration_ms.map(|d| now + d);
        let longer_exists = self.blocks.iter().any(|b| {
            b.symbol == symbol && b.active(now) && match (b.until, until) {
                (None, _) => true,
                (Some(_), None) => false,
                (Some(a), Some(n)) => a >= n,
            }
        });
        if !longer_exists {
            self.blocks.retain(|b| b.symbol != symbol);
            self.blocks.push(AlertBlock { symbol, until });
        }
    }

    pub fn unblock(&mut self, symbol: &str) {
        self.blocks.retain(|b| &*b.symbol != symbol);
    }

    pub fn blocks(&mut self, now: Ts) -> &[AlertBlock] {
        self.blocks.retain(|b| b.active(now));
        &self.blocks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicators::RelativeInfo;

    fn pair() -> Pair {
        Pair::new("ABCUSDT", "ABC", "USDT")
    }

    fn changes(period: u32, min: f64, min_ts: i64, max: f64, max_ts: i64, last: f64, last_ts: i64) -> PriceChanges {
        PriceChanges {
            period_sec: period,
            first_ts: 0,
            last_price: last,
            last_ts: last_ts * 1000,
            min_price: min,
            min_ts: min_ts * 1000,
            max_price: max,
            max_ts: max_ts * 1000,
            max_after_min_price: max,
            max_after_min_ts: max_ts * 1000,
            min_after_max_price: last,
            min_after_max_ts: last_ts * 1000,
            relative: Some(RelativeInfo { hi_lo_diff: 1.0, rel_std_dev: 0.5 }),
        }
    }

    fn checker() -> AlertChecker {
        AlertChecker::new("test".into(), AlertSettings::default_for(&[300, 1800]), vec![])
    }

    #[test]
    fn rise_triggers_with_required_values() {
        let mut c = checker();
        // +5% (> 3%) with relative 5 (>= 4)
        let a = c.check(&pair(), &[changes(300, 100.0, 0, 105.0, 60, 105.0, 60)], None, 0);
        assert_eq!(a.len(), 1);
        assert!(a[0].is_rising());
        assert_eq!(a[0].reference_price, 100.0);
        // relative too small, % too small for sufficient
        let mut c = checker();
        let mut ch = changes(300, 100.0, 0, 105.0, 60, 105.0, 60);
        ch.relative = Some(RelativeInfo { hi_lo_diff: 2.0, rel_std_dev: 0.5 });
        assert!(c.check(&pair(), &[ch], None, 0).is_empty());
    }

    #[test]
    fn sufficient_relative_alone_triggers() {
        let mut c = checker();
        // +1% but relative 10
        let mut ch = changes(300, 100.0, 0, 101.0, 60, 101.0, 60);
        ch.relative = Some(RelativeInfo { hi_lo_diff: 0.1, rel_std_dev: 0.5 });
        assert_eq!(c.check(&pair(), &[ch], None, 0).len(), 1);
    }

    #[test]
    fn fall_triggers() {
        let mut c = checker();
        let a = c.check(&pair(), &[changes(300, 95.0, 60, 100.0, 0, 95.0, 60)], None, 0);
        assert_eq!(a.len(), 1);
        assert!(!a[0].is_rising());
    }

    #[test]
    fn long_period_skipped_for_quick_moves() {
        let mut c = checker();
        assert!(c.check(&pair(), &[changes(1800, 100.0, 0, 110.0, 200, 110.0, 200)], None, 0).is_empty());
        assert_eq!(c.check(&pair(), &[changes(1800, 100.0, 0, 110.0, 400, 110.0, 400)], None, 0).len(), 1);
    }

    #[test]
    fn subsequent_alerts() {
        let mut c = checker();
        assert_eq!(c.check(&pair(), &[changes(300, 100.0, 0, 105.0, 60, 105.0, 60)], None, 0).len(), 1);
        // same move again, blocked
        assert!(c.check(&pair(), &[changes(300, 100.0, 0, 106.0, 90, 106.0, 90)], None, 0).is_empty());
        // 2x bigger, allowed
        assert_eq!(c.check(&pair(), &[changes(300, 100.0, 0, 111.0, 120, 111.0, 120)], None, 0).len(), 1);
        // after the pause, allowed
        assert_eq!(c.check(&pair(), &[changes(300, 100.0, 2000, 112.0, 2100, 112.0, 2100)], None, 0).len(), 1);
    }

    #[test]
    fn reversal_allowed_when_not_blocking_subsequent() {
        let mut c = checker();
        c.settings.block_subsequent = false;
        c.settings.allow_subsequent_2x = false;
        assert_eq!(c.check(&pair(), &[changes(300, 100.0, 0, 105.0, 60, 105.0, 60)], None, 0).len(), 1);
        assert!(c.check(&pair(), &[changes(300, 100.0, 0, 106.0, 90, 106.0, 90)], None, 0).is_empty());
        assert_eq!(c.check(&pair(), &[changes(300, 100.0, 150, 106.0, 90, 100.0, 150)], None, 0).len(), 1);
    }

    #[test]
    fn blocks_and_liquidity() {
        let mut c = checker();
        c.block("ABCUSDT".into(), Some(1000), 0);
        assert!(c.check(&pair(), &[changes(300, 100.0, 0, 105.0, 60, 105.0, 60)], None, 500).is_empty());
        let mut c = checker();
        c.block("ABCUSDT".into(), Some(1000), 0);
        assert_eq!(c.check(&pair(), &[changes(300, 100.0, 0, 105.0, 60, 105.0, 60)], None, 1000).len(), 1);
        let mut c = checker();
        assert!(c.check(&pair(), &[changes(300, 100.0, 0, 105.0, 60, 105.0, 60)], Some(0.1), 0).is_empty());
        // permanent block is not shortened
        c.block("X".into(), None, 0);
        c.block("X".into(), Some(10), 0);
        assert_eq!(c.blocks(100).len(), 1);
    }
}
