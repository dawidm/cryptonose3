//! Price changes over sliding time windows.
//!
//! Each window is anchored on the timestamp of the most recent tick (not wall clock), like the
//! original engine. Min/max and the "max after min" / "min after max" values are maintained with
//! monotonic deques, so every update is O(1) amortized and a query is O(log n).

use std::collections::VecDeque;

use serde::Serialize;

use crate::indicators::RelativeInfo;
use crate::types::Ts;

#[derive(Debug, Clone, Copy)]
struct Pt {
    seq: u64,
    ts: Ts,
    price: f64,
}

#[derive(Debug, Clone)]
pub struct PeriodWindow {
    period_ms: i64,
    seq: u64,
    ticks: VecDeque<Pt>,
    // increasing prices, front = min
    minq: VecDeque<Pt>,
    // decreasing prices, front = max
    maxq: VecDeque<Pt>,
}

impl PeriodWindow {
    pub fn new(period_sec: u32) -> Self {
        Self {
            period_ms: period_sec as i64 * 1000,
            seq: 0,
            ticks: VecDeque::new(),
            minq: VecDeque::new(),
            maxq: VecDeque::new(),
        }
    }

    pub fn period_sec(&self) -> u32 {
        (self.period_ms / 1000) as u32
    }

    pub fn is_empty(&self) -> bool {
        self.ticks.is_empty()
    }

    pub fn push(&mut self, ts: Ts, price: f64) {
        if !price.is_finite() || price <= 0.0 {
            return;
        }
        // out of order ticks are clamped to the last timestamp
        let ts = self.ticks.back().map_or(ts, |last| ts.max(last.ts));
        self.seq += 1;
        let pt = Pt { seq: self.seq, ts, price };
        self.ticks.push_back(pt);
        // on ties keep the most recent point, so changes are measured from the latest extreme
        while self.minq.back().is_some_and(|b| b.price >= price) {
            self.minq.pop_back();
        }
        self.minq.push_back(pt);
        while self.maxq.back().is_some_and(|b| b.price <= price) {
            self.maxq.pop_back();
        }
        self.maxq.push_back(pt);

        let cutoff = ts - self.period_ms;
        while self.ticks.front().is_some_and(|f| f.ts <= cutoff) {
            let removed = self.ticks.pop_front().unwrap();
            if self.minq.front().is_some_and(|f| f.seq <= removed.seq) {
                self.minq.pop_front();
            }
            if self.maxq.front().is_some_and(|f| f.seq <= removed.seq) {
                self.maxq.pop_front();
            }
        }
    }

    pub fn changes(&self) -> Option<PriceChanges> {
        let last = *self.ticks.back()?;
        let first = *self.ticks.front()?;
        let min = *self.minq.front()?;
        let max = *self.maxq.front()?;
        let max_after_min = first_from(&self.maxq, min.seq);
        let min_after_max = first_from(&self.minq, max.seq);
        Some(PriceChanges {
            period_sec: self.period_sec(),
            first_ts: first.ts,
            last_price: last.price,
            last_ts: last.ts,
            min_price: min.price,
            min_ts: min.ts,
            max_price: max.price,
            max_ts: max.ts,
            max_after_min_price: max_after_min.price,
            max_after_min_ts: max_after_min.ts,
            min_after_max_price: min_after_max.price,
            min_after_max_ts: min_after_max.ts,
            relative: None,
        })
    }
}

/// First point in a monotonic deque with sequence >= `seq`. The deques are ordered by sequence, so
/// this is the extreme value over the range [seq, last].
fn first_from(q: &VecDeque<Pt>, seq: u64) -> Pt {
    let idx = q.partition_point(|p| p.seq < seq);
    q[idx.min(q.len() - 1)]
}

/// Snapshot of price changes for one pair and one time period.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PriceChanges {
    pub period_sec: u32,
    /// Oldest tick still in the window, tells how much of the period is covered by data.
    pub first_ts: Ts,
    pub last_price: f64,
    pub last_ts: Ts,
    pub min_price: f64,
    pub min_ts: Ts,
    pub max_price: f64,
    pub max_ts: Ts,
    pub max_after_min_price: f64,
    pub max_after_min_ts: Ts,
    pub min_after_max_price: f64,
    pub min_after_max_ts: Ts,
    pub relative: Option<RelativeInfo>,
}

impl PriceChanges {
    fn rising(&self) -> bool {
        self.max_ts > self.min_ts
    }

    /// max - min if the max is more recent, otherwise min - max.
    pub fn change(&self) -> f64 {
        if self.rising() {
            self.max_price - self.min_price
        } else {
            self.min_price - self.max_price
        }
    }

    pub fn percent_change(&self) -> f64 {
        if self.rising() {
            100.0 * (self.max_price - self.min_price) / self.min_price
        } else {
            100.0 * (self.min_price - self.max_price) / self.max_price
        }
    }

    /// Change between the last price and the min or max, whichever is bigger.
    pub fn last_change(&self) -> f64 {
        let rise = self.last_price - self.min_price;
        let drop = self.max_price - self.last_price;
        if rise > drop {
            rise
        } else {
            -drop
        }
    }

    pub fn last_percent_change(&self) -> f64 {
        let rise = self.last_price - self.min_price;
        let drop = self.max_price - self.last_price;
        if rise > drop {
            100.0 * rise / self.min_price
        } else {
            -100.0 * drop / self.max_price
        }
    }

    /// Rise from the min to the highest price after it (>= 0).
    pub fn rise_change(&self) -> f64 {
        self.max_after_min_price - self.min_price
    }

    /// Drop from the max to the lowest price after it (<= 0).
    pub fn drop_change(&self) -> f64 {
        self.min_after_max_price - self.max_price
    }

    /// Duration of `change()` in seconds.
    pub fn change_time_sec(&self) -> i64 {
        (self.min_ts - self.max_ts).abs() / 1000
    }

    pub fn final_ts(&self) -> Ts {
        self.min_ts.max(self.max_ts)
    }

    /// Price the last change is measured from.
    pub fn reference_to_last_price(&self) -> f64 {
        if self.last_percent_change() > 0.0 {
            self.min_price
        } else {
            self.max_price
        }
    }

    pub fn reference_to_last_ts(&self) -> Ts {
        if self.last_percent_change() > 0.0 {
            self.min_ts
        } else {
            self.max_ts
        }
    }

    pub fn relative_change(&self) -> Option<f64> {
        self.relative.map(|r| self.change() / r.hi_lo_diff)
    }

    pub fn relative_last_change(&self) -> Option<f64> {
        self.relative.map(|r| self.last_change() / r.hi_lo_diff)
    }
}

/// Windows for all configured periods of a single pair.
#[derive(Debug, Clone)]
pub struct PairChanges {
    windows: Vec<PeriodWindow>,
}

impl PairChanges {
    pub fn new(periods_sec: &[u32]) -> Self {
        Self { windows: periods_sec.iter().map(|p| PeriodWindow::new(*p)).collect() }
    }

    pub fn push(&mut self, ts: Ts, price: f64) {
        for w in &mut self.windows {
            w.push(ts, price);
        }
    }

    pub fn changes(&self) -> Vec<PriceChanges> {
        self.windows.iter().filter_map(|w| w.changes()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(w: &mut PeriodWindow, pts: &[(i64, f64)]) {
        for (t, p) in pts {
            w.push(t * 1000, *p);
        }
    }

    #[test]
    fn rising_move() {
        let mut w = PeriodWindow::new(300);
        feed(&mut w, &[(0, 10.0), (60, 9.0), (120, 12.0), (180, 11.0)]);
        let c = w.changes().unwrap();
        assert_eq!((c.min_price, c.max_price, c.last_price), (9.0, 12.0, 11.0));
        assert_eq!(c.change(), 3.0);
        assert!((c.percent_change() - 100.0 / 3.0).abs() < 1e-9);
        assert_eq!(c.last_change(), 2.0);
        assert_eq!(c.change_time_sec(), 60);
        assert_eq!(c.max_after_min_price, 12.0);
        assert_eq!(c.min_after_max_price, 11.0);
        assert_eq!(c.min_after_max_ts, 180_000);
        assert_eq!(c.rise_change(), 3.0);
        assert_eq!(c.drop_change(), -1.0);
        assert_eq!(c.reference_to_last_price(), 9.0);
    }

    #[test]
    fn falling_move_and_eviction() {
        let mut w = PeriodWindow::new(300);
        feed(&mut w, &[(0, 1.0), (100, 20.0), (200, 15.0), (350, 10.0)]);
        // tick at 0 (price 1.0) is out of window (cutoff 50)
        let c = w.changes().unwrap();
        assert_eq!(c.min_price, 10.0);
        assert_eq!(c.max_price, 20.0);
        assert_eq!(c.change(), -10.0);
        assert_eq!(c.last_change(), -10.0);
        assert!((c.last_percent_change() + 50.0).abs() < 1e-9);
        assert_eq!(c.first_ts, 100_000);
        // max after min is the min itself
        assert_eq!(c.max_after_min_price, 10.0);
    }

    #[test]
    fn matches_brute_force() {
        let mut w = PeriodWindow::new(30);
        let mut all: Vec<(i64, f64)> = vec![];
        let mut x: u64 = 7;
        for i in 0..2000 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let price = 100.0 + ((x >> 33) % 50) as f64;
            let ts = i * 1000 + ((x >> 20) % 3) as i64 * 400;
            w.push(ts, price);
            let ts = all.last().map_or(ts, |l: &(i64, f64)| ts.max(l.0));
            all.push((ts, price));
            let cutoff = ts - 30_000;
            let win: Vec<_> = all.iter().filter(|p| p.0 > cutoff).collect();
            let min = win.iter().map(|p| p.1).fold(f64::MAX, f64::min);
            let max = win.iter().map(|p| p.1).fold(f64::MIN, f64::max);
            let min_pos = win.iter().rposition(|p| p.1 == min).unwrap();
            let max_after_min = win[min_pos..].iter().map(|p| p.1).fold(f64::MIN, f64::max);
            let c = w.changes().unwrap();
            assert_eq!(c.min_price, min);
            assert_eq!(c.max_price, max);
            assert_eq!(c.max_after_min_price, max_after_min);
        }
    }
}
