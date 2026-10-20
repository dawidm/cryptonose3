//! Candle series kept up to date from live ticks.

use std::collections::VecDeque;

use crate::types::{Candle, Ts};

#[derive(Debug, Clone)]
pub struct CandleSeries {
    period_ms: i64,
    max_len: usize,
    closed: VecDeque<Candle>,
    current: Option<Candle>,
}

impl CandleSeries {
    pub fn new(period_sec: u32, max_len: usize) -> Self {
        Self { period_ms: period_sec as i64 * 1000, max_len, closed: VecDeque::new(), current: None }
    }

    pub fn period_sec(&self) -> u32 {
        (self.period_ms / 1000) as u32
    }

    fn start_of(&self, ts: Ts) -> Ts {
        ts.div_euclid(self.period_ms) * self.period_ms
    }

    /// Replace contents with history from the exchange. A candle that hasn't ended at `now` becomes
    /// the current (open) candle. Gaps are filled with flat candles.
    pub fn load(&mut self, mut candles: Vec<Candle>, now: Ts) {
        candles.sort_by_key(|c| c.ts);
        candles.dedup_by_key(|c| c.ts);
        self.closed.clear();
        self.current = None;
        for c in candles {
            if c.ts + self.period_ms > now {
                self.current = Some(c);
                break;
            }
            self.push_closed(c);
        }
        self.trim();
    }

    fn push_closed(&mut self, c: Candle) {
        if let Some(prev) = self.closed.back().copied() {
            let mut ts = prev.ts + self.period_ms;
            while ts < c.ts {
                self.closed.push_back(Candle::flat(ts, prev.close));
                ts += self.period_ms;
            }
        }
        self.closed.push_back(c);
    }

    fn trim(&mut self) {
        while self.closed.len() > self.max_len {
            self.closed.pop_front();
        }
    }

    fn last_close(&self) -> Option<f64> {
        self.current.map(|c| c.close).or_else(|| self.closed.back().map(|c| c.close))
    }

    /// Close finished candles up to `now`. Returns true if any candle was closed.
    pub fn roll(&mut self, now: Ts) -> bool {
        let mut changed = false;
        loop {
            let open_start = match self.current {
                Some(c) => c.ts,
                None => match self.closed.back() {
                    Some(c) => c.ts + self.period_ms,
                    None => return changed,
                },
            };
            if open_start + self.period_ms > now {
                break;
            }
            let c = match self.current.take() {
                Some(c) => c,
                None => Candle::flat(open_start, self.last_close().unwrap()),
            };
            self.push_closed(c);
            changed = true;
        }
        if changed {
            self.trim();
        }
        changed
    }

    pub fn insert(&mut self, ts: Ts, price: f64) -> bool {
        let start = self.start_of(ts);
        let closed = match self.current {
            Some(c) if start > c.ts => self.roll(start),
            None if self.closed.back().is_some_and(|c| start > c.ts + self.period_ms) => self.roll(start),
            _ => false,
        };
        match &mut self.current {
            // late tick for an already closed candle is applied to the open one
            Some(c) => {
                c.high = c.high.max(price);
                c.low = c.low.min(price);
                c.close = price;
            }
            None => {
                let open = self.closed.back().map_or(price, |c| c.close);
                let start = self.closed.back().map_or(start, |c| (c.ts + self.period_ms).max(start));
                self.current = Some(Candle { ts: start, open, high: open.max(price), low: open.min(price), close: price });
            }
        }
        closed
    }

    /// Closed candles, oldest first.
    pub fn closed(&self) -> &VecDeque<Candle> {
        &self.closed
    }

    pub fn current(&self) -> Option<Candle> {
        self.current
    }

    /// Closed candles plus the current one.
    pub fn all(&self) -> Vec<Candle> {
        self.closed.iter().copied().chain(self.current).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: i64 = 60_000;

    #[test]
    fn builds_and_fills_gaps() {
        let mut s = CandleSeries::new(60, 10);
        s.load(vec![Candle { ts: 0, open: 1.0, high: 2.0, low: 1.0, close: 2.0 }], 30_000);
        assert!(s.closed().is_empty());
        assert_eq!(s.current().unwrap().ts, 0);
        s.insert(10_000, 3.0);
        assert_eq!(s.current().unwrap().high, 3.0);
        // jump over two periods
        assert!(s.insert(3 * M + 5, 4.0));
        let closed: Vec<_> = s.closed().iter().map(|c| (c.ts, c.close)).collect();
        assert_eq!(closed, vec![(0, 3.0), (M, 3.0), (2 * M, 3.0)]);
        let cur = s.current().unwrap();
        assert_eq!((cur.ts, cur.open, cur.close), (3 * M, 3.0, 4.0));
    }

    #[test]
    fn roll_without_ticks_creates_flat_candles() {
        let mut s = CandleSeries::new(60, 3);
        s.load(vec![Candle::flat(0, 5.0), Candle::flat(M, 6.0)], 2 * M);
        assert_eq!(s.closed().len(), 2);
        assert!(s.roll(6 * M + 1));
        assert_eq!(s.closed().len(), 3);
        assert_eq!(s.closed().back().unwrap().ts, 5 * M);
        assert!(s.closed().iter().all(|c| c.close == 6.0));
        assert!(s.current().is_none());
        s.insert(6 * M + 2, 7.0);
        assert_eq!(s.current().unwrap().ts, 6 * M);
    }

    #[test]
    fn load_fills_gaps_and_trims() {
        let mut s = CandleSeries::new(60, 3);
        s.load(vec![Candle::flat(0, 1.0), Candle::flat(4 * M, 2.0)], 10 * M);
        let ts: Vec<_> = s.closed().iter().map(|c| c.ts).collect();
        assert_eq!(ts, vec![2 * M, 3 * M, 4 * M]);
    }
}
