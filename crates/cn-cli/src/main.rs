//! Headless Cryptonose: run the engine for an exchange and print status, top movers and alerts.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use cn_core::alerts::{PeriodThresholds, ThresholdPreset};
use cn_core::engine::{self, EngineConfig, EngineEvent, PairCriterion, PairSelection, PairUpdate};

#[derive(Parser)]
#[command(about = "Cryptonose headless runner")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List supported exchanges.
    Exchanges,
    /// List markets of an exchange, sorted by 24h quote volume.
    Markets {
        exchange: String,
        #[arg(long)]
        quote: Option<String>,
        #[arg(long, default_value_t = 30)]
        limit: usize,
    },
    /// Watch price changes and alerts.
    Watch {
        exchange: String,
        /// Quote currency; uses the exchange defaults if omitted.
        #[arg(long)]
        quote: Option<String>,
        #[arg(long, default_value_t = 1e6)]
        min_vol: f64,
        /// Watch the top N pairs of the quote currency by 24h volume, instead of --min-vol.
        #[arg(long)]
        top: Option<usize>,
        /// Alert thresholds preset: low, medium or high.
        #[arg(long, default_value = "medium")]
        preset: String,
        /// Seconds between top movers printouts.
        #[arg(long, default_value_t = 15)]
        every: u64,
    },
}

fn exchange(id: &str) -> Result<Arc<dyn cn_core::Exchange>> {
    cn_exchanges::by_id(id).ok_or_else(|| anyhow!("unknown exchange {id}, see `cn-cli exchanges`"))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn,cn_core=info".into()))
        .init();
    match Cli::parse().cmd {
        Cmd::Exchanges => {
            for e in cn_exchanges::all() {
                let m = e.meta();
                println!("{:<12} {:<12} {:?} {:?}", m.id, m.name, m.kind, m.feed);
            }
        }
        Cmd::Markets { exchange: id, quote, limit } => {
            let mut markets = exchange(&id)?.markets().await?;
            if let Some(q) = quote {
                markets.retain(|m| m.pair.quote.eq_ignore_ascii_case(&q));
            }
            markets.sort_by(|a, b| b.quote_volume_24h.total_cmp(&a.quote_volume_24h));
            for m in markets.iter().take(limit) {
                println!("{:<20} {:<14} vol {:>16.0}  last {:?}", m.pair.symbol, m.pair.display(), m.quote_volume_24h, m.last_price);
            }
        }
        Cmd::Watch { exchange: id, quote, min_vol, top, preset, every } => watch(&id, quote, min_vol, top, &preset, every).await?,
    }
    Ok(())
}

async fn watch(id: &str, quote: Option<String>, min_vol: f64, top: Option<usize>, preset: &str, every: u64) -> Result<()> {
    let ex = exchange(id)?;
    let criteria = match quote {
        Some(q) => vec![top.map_or_else(|| PairCriterion::volume(q.clone(), min_vol), |n| PairCriterion::top(q.clone(), n))],
        None => ex.meta().default_top.iter().map(|(q, n)| PairCriterion::top(*q, *n)).collect(),
    };
    let preset = match preset {
        "low" => ThresholdPreset::Low,
        "high" => ThresholdPreset::High,
        _ => ThresholdPreset::Medium,
    };
    let mut cfg = EngineConfig::new(PairSelection { criteria, manual: vec![], blacklist: vec![], pinned: vec![] });
    cfg.alerts.periods = cfg.periods_sec.iter().map(|p| PeriodThresholds::preset(*p, preset)).collect();
    let (handle, mut rx) = engine::spawn(ex.clone(), cfg);

    let mut latest: HashMap<Arc<str>, PairUpdate> = HashMap::new();
    let mut names: HashMap<Arc<str>, String> = HashMap::new();
    let mut print = tokio::time::interval(Duration::from_secs(every));
    loop {
        tokio::select! {
            e = rx.recv() => match e {
                None => break,
                Some(EngineEvent::Status { status, message }) => println!("[status] {status:?} {}", message.unwrap_or_default()),
                Some(EngineEvent::Progress { done, total }) => {
                    if done == total || done % 10 == 0 {
                        println!("[loading] {done}/{total}");
                    }
                }
                Some(EngineEvent::Pairs { pairs }) => {
                    println!("[pairs] {} selected", pairs.len());
                    names = pairs.iter().map(|p| (p.symbol.clone(), p.display())).collect();
                }
                Some(EngineEvent::Updates { updates, .. }) => {
                    for u in updates {
                        latest.insert(u.symbol.clone(), u);
                    }
                }
                Some(EngineEvent::Alert(a)) => {
                    let al = &a.alert;
                    println!(
                        "\x07[ALERT] {:<14} {:>4}m {:>+7.2}% rel {:>+6.2}  {} -> {}  in {}s  {}",
                        al.pair.display(), al.period_sec / 60, al.price_change, al.relative_change,
                        al.reference_price, al.final_price, al.change_time_sec, ex.market_url(&al.pair)
                    );
                }
            },
            _ = print.tick() => print_top(&latest, &names),
            _ = tokio::signal::ctrl_c() => {
                handle.stop();
                break;
            }
        }
    }
    Ok(())
}

fn print_top(latest: &HashMap<Arc<str>, PairUpdate>, names: &HashMap<Arc<str>, String>) {
    if latest.is_empty() {
        return;
    }
    let mut rows: Vec<&PairUpdate> = latest.values().collect();
    let key = |u: &PairUpdate| u.periods.first().map_or(0.0, |p| p.percent_change.abs());
    rows.sort_by(|a, b| key(b).total_cmp(&key(a)));
    println!("{:<16} {:>14} {:>9} {:>8} {:>9} {:>8}", "pair", "last", "5m %", "5m rel", "30m %", "30m rel");
    for u in rows.iter().take(10) {
        let p = |i: usize| u.periods.get(i);
        let fmt_rel = |r: Option<f64>| r.map_or("-".to_string(), |r| format!("{r:+.2}"));
        println!(
            "{:<16} {:>14} {:>+9.2} {:>8} {:>+9.2} {:>8}",
            names.get(&u.symbol).cloned().unwrap_or_else(|| u.symbol.to_string()),
            format!("{:.8}", u.last_price).trim_end_matches('0').trim_end_matches('.'),
            p(0).map_or(0.0, |x| x.percent_change),
            fmt_rel(p(0).and_then(|x| x.relative_change)),
            p(1).map_or(0.0, |x| x.percent_change),
            fmt_rel(p(1).and_then(|x| x.relative_change)),
        );
    }
    println!();
}
