use serde::Serialize;

use crate::types::Candle;

/// Candles with a high-low range not exceeding `last_close * MIN_CANDLE_CHANGE` are ignored when
/// computing the typical range.
pub const MIN_CANDLE_CHANGE: f64 = 0.0001;

/// Typical candle range for a pair and period. Relative change = price change / `hi_lo_diff`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RelativeInfo {
    pub hi_lo_diff: f64,
    pub rel_std_dev: f64,
}

/// Weighted average of |high - low|, newer candles weigh more: Σ i·|H−L| / Σ i, i = 1 (oldest)..n.
pub fn weighted_avg_hilo(candles: &[Candle]) -> f64 {
    let (num, den) = candles.iter().enumerate().fold((0.0, 0.0), |(num, den), (i, c)| {
        let w = (i + 1) as f64;
        (num + w * c.range(), den + w)
    });
    if den == 0.0 {
        0.0
    } else {
        num / den
    }
}

/// Coefficient of variation of |high - low| (sample standard deviation / mean).
pub fn rel_std_dev(candles: &[Candle]) -> f64 {
    let n = candles.len();
    if n < 2 {
        return 0.0;
    }
    let mean = candles.iter().map(Candle::range).sum::<f64>() / n as f64;
    if mean == 0.0 {
        return 0.0;
    }
    let var = candles.iter().map(|c| (c.range() - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
    var.sqrt() / mean
}

/// Share of candles with any price movement among the last `n`.
pub fn liquidity_factor(candles: &[Candle], n: usize) -> f64 {
    let tail = &candles[candles.len().saturating_sub(n)..];
    if tail.is_empty() {
        return 0.0;
    }
    let empty = tail.iter().filter(|c| c.is_empty()).count();
    (tail.len() - empty) as f64 / tail.len() as f64
}

pub fn relative_info(candles: &[Candle], n: usize) -> Option<RelativeInfo> {
    let tail = &candles[candles.len().saturating_sub(n)..];
    let last_close = tail.last()?.close;
    let used: Vec<Candle> = tail.iter().copied().filter(|c| c.range() > last_close * MIN_CANDLE_CHANGE).collect();
    if used.is_empty() {
        return None;
    }
    Some(RelativeInfo { hi_lo_diff: weighted_avg_hilo(&used), rel_std_dev: rel_std_dev(&used) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(h: f64, l: f64) -> Candle {
        Candle { ts: 0, open: l, high: h, low: l, close: h }
    }

    #[test]
    fn weighted() {
        // ranges 1, 2, 3 with weights 1, 2, 3 -> (1 + 4 + 9) / 6
        let v = weighted_avg_hilo(&[c(2.0, 1.0), c(3.0, 1.0), c(4.0, 1.0)]);
        assert!((v - 14.0 / 6.0).abs() < 1e-12);
    }

    #[test]
    fn std_dev() {
        // ranges 1, 3: mean 2, sample sd sqrt(2)
        let v = rel_std_dev(&[c(2.0, 1.0), c(4.0, 1.0)]);
        assert!((v - 2f64.sqrt() / 2.0).abs() < 1e-12);
    }

    #[test]
    fn liquidity_counts_every_candle() {
        let candles = [c(1.0, 1.0), c(2.0, 1.0), c(1.0, 1.0), c(3.0, 1.0)];
        assert_eq!(liquidity_factor(&candles, 4), 0.5);
        assert_eq!(liquidity_factor(&candles, 2), 0.5);
        assert_eq!(liquidity_factor(&candles[..1], 4), 0.0);
    }

    #[test]
    fn relative_ignores_flat_candles() {
        let candles = [c(1.0, 1.0), c(3.0, 1.0)];
        let r = relative_info(&candles, 50).unwrap();
        assert_eq!(r.hi_lo_diff, 2.0);
        assert!(relative_info(&[c(1.0, 1.0)], 50).is_none());
    }
}
