//! One-time, best-effort import of Cryptonose 2 settings stored with Java Preferences
//! (`~/.java/.userPrefs/pl/dmotyka/cryptonose2/controllers/<category>/<Exchange>/prefs.xml` on Linux).

use std::collections::HashMap;
use std::path::Path;

use cn_core::alerts::AlertBlock;
use cn_core::engine::PairCriterion;
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::settings::{Settings, MAX_PINNED};

const V2_TO_V3: &[(&str, &str)] = &[("Binance", "binance"), ("Bitfinex", "bitfinex"), ("Huobi", "htx"), ("KuCoin", "kucoin"), ("Poloniex", "poloniex")];

fn read_prefs(path: &Path) -> HashMap<String, String> {
    let Ok(xml) = std::fs::read_to_string(path) else { return HashMap::new() };
    let mut reader = Reader::from_str(&xml);
    let mut out = HashMap::new();
    while let Ok(ev) = reader.read_event() {
        match ev {
            Event::Empty(e) | Event::Start(e) if e.local_name().as_ref() == b"entry" => {
                let get = |k: &str| e.try_get_attribute(k).ok().flatten().and_then(|a| a.unescape_value().ok()).map(|v| v.into_owned());
                if let (Some(k), Some(v)) = (get("key"), get("value")) {
                    out.insert(k, v);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
}

fn list(s: Option<&String>) -> Vec<String> {
    s.map(|s| s.split(',').map(str::trim).filter(|p| !p.is_empty()).map(String::from).collect()).unwrap_or_default()
}

/// Legacy Poloniex symbols were QUOTE_BASE, API v3 uses BASE_QUOTE.
fn convert_symbol(id: &str, s: &str) -> String {
    match (id, s.split_once('_')) {
        ("poloniex", Some((q, b))) => format!("{b}_{q}"),
        _ => s.to_string(),
    }
}

/// True if a v2 pinned entry is a real pin (v2 stored the pin timestamp, and 0 for unpinned pairs).
fn is_v2_pin(value: &str) -> bool {
    value.parse::<i64>().is_ok_and(|ts| ts > 1_000_000_000_000)
}

/// Remove pins that v2 marked as unpinned or that aren't pairs ("lastPos"). Pins added in v3 are kept.
pub fn remove_unpinned(home: &Path, settings: &mut Settings) {
    let root = home.join(".java/.userPrefs/pl/dmotyka/cryptonose2/controllers");
    for (v2, id) in V2_TO_V3 {
        let v2_pins = read_prefs(&root.join(format!("cryptonosePreferences/{v2}/pinned/prefs.xml")));
        let Some(es) = settings.exchanges.get_mut(*id) else { continue };
        let not_pinned: Vec<String> = v2_pins.iter().filter(|(_, v)| !is_v2_pin(v)).map(|(k, _)| convert_symbol(id, k)).collect();
        es.selection.pinned.retain(|p| p != "lastPos" && !not_pinned.contains(p));
    }
}

/// Returns Ok(true) if anything was imported.
pub fn import(home: &Path, settings: &mut Settings) -> anyhow::Result<bool> {
    let root = home.join(".java/.userPrefs/pl/dmotyka/cryptonose2/controllers");
    if !root.is_dir() {
        return Ok(false);
    }
    let general = read_prefs(&root.join("cryptonosePreferences/prefs.xml"));
    let gui = read_prefs(&root.join("guiState/prefs.xml"));
    let b = |m: &HashMap<String, String>, k: &str| m.get(k).map(|v| v == "true");
    if let Some(dark) = b(&general, "darkMode") {
        settings.general.theme = if dark { "dark" } else { "light" }.into();
    }
    if let Some(size) = general.get("fontSizePt").and_then(|v| v.parse().ok()) {
        settings.general.font_size = Some(size);
    }
    if b(&general, "tryUseDefBrowser") == Some(false) {
        settings.general.browser_command = general.get("browserPath").cloned();
    }
    if let Some(v) = b(&general, "connectionStatusNotif") {
        settings.general.connection_notifications = v;
    }
    if let Some(v) = b(&gui, "enableAlertSound") {
        settings.toggles.sound = v;
    }
    if let Some(v) = b(&gui, "enableAlertBrowser") {
        settings.toggles.browser = v;
    }
    if let Some(v) = b(&gui, "enableAlertNotification") {
        settings.toggles.notification = v;
    }
    let active: Vec<String> = list(gui.get("activeExchanges"))
        .iter()
        .filter_map(|n| V2_TO_V3.iter().find(|(v2, _)| v2.eq_ignore_ascii_case(n)).map(|(_, id)| id.to_string()))
        .collect();
    if !active.is_empty() {
        settings.active_exchanges = active;
    }

    let mut pinned_ts: Vec<(i64, String, String)> = Vec::new();
    for (v2, id) in V2_TO_V3 {
        let Some(ex) = cn_exchanges::by_id(id) else { continue };
        let alerts = read_prefs(&root.join(format!("alertPreferences/{v2}/prefs.xml")));
        let pairs = read_prefs(&root.join(format!("pairsPreferences/{v2}/prefs.xml")));
        let pinned = read_prefs(&root.join(format!("cryptonosePreferences/{v2}/pinned/prefs.xml")));
        if alerts.is_empty() && pairs.is_empty() && pinned.is_empty() {
            continue;
        }
        let es = settings.exchange(&*ex);
        let f = |k: &str| alerts.get(k).and_then(|v| v.parse::<f64>().ok());
        for t in &mut es.alerts.periods {
            let p = t.period_sec;
            let enabled_key = if p <= 300 { "enableM5Alerts" } else { "enableM30Alerts" };
            if let Some(v) = alerts.get(enabled_key) {
                t.enabled = v == "true";
            }
            for (key, field) in [
                ("requiredRisingValue", &mut t.required_rise),
                ("requiredFallingValue", &mut t.required_fall),
                ("requiredRelativeRisingValue", &mut t.required_rel_rise),
                ("requiredRelativeFallingValue", &mut t.required_rel_fall),
                ("sufficientRelativeRisingValue", &mut t.sufficient_rel_rise),
                ("sufficientRelativeFallingValue", &mut t.sufficient_rel_fall),
            ] {
                if let Some(v) = f(&format!("{key}{p}")) {
                    *field = v;
                }
            }
        }
        if let Some(v) = alerts.get("enableMinCnLiquidity0") {
            es.alerts.min_liquidity_enabled = v == "true";
        }
        if let Some(v) = f("minCnLiquidity0") {
            es.alerts.min_liquidity = (v * 100.0).round() / 100.0;
        }
        if let Some(v) = alerts.get("enableBlockSubsequentAlerts0") {
            es.alerts.block_subsequent = v == "true";
        }
        if let Some(v) = alerts.get("enableAllowSubsequent2xAlerts0") {
            es.alerts.allow_subsequent_2x = v == "true";
        }
        es.blocks = list(alerts.get("permanentAlertBlocks0").or(alerts.get("permanentAlertBlocks")))
            .iter()
            .map(|s| AlertBlock { symbol: convert_symbol(id, s).into(), until: None })
            .collect();

        let markets = list(pairs.get("markets"));
        if !markets.is_empty() {
            es.selection.criteria = markets
                .iter()
                .map(|m| PairCriterion::volume(m.clone(), pairs.get(m).and_then(|v| v.parse().ok()).unwrap_or(200.0)))
                .collect();
        }
        es.selection.manual = list(pairs.get("pairsApiSymbols")).iter().map(|s| convert_symbol(id, s)).collect();
        es.selection.blacklist = list(pairs.get("blacklistPairsApiSymbols")).iter().map(|s| convert_symbol(id, s)).collect();
        // v2 kept unpinned pairs with value 0, and a non-pair "lastPos" entry; pins hold the pin timestamp
        pinned_ts.extend(
            pinned
                .iter()
                .filter(|(_, v)| is_v2_pin(v))
                .filter_map(|(k, v)| Some((v.parse::<i64>().ok()?, id.to_string(), convert_symbol(id, k)))),
        );
    }
    // newest pins first, up to the limit
    pinned_ts.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, id, symbol) in pinned_ts.into_iter().take(MAX_PINNED) {
        if let Some(es) = settings.exchanges.get_mut(&id) {
            es.selection.pinned.push(symbol);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, entries: &[(&str, &str)]) {
        let p = root.join(".java/.userPrefs/pl/dmotyka/cryptonose2/controllers").join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let body: String = entries.iter().map(|(k, v)| format!("<entry key=\"{k}\" value=\"{v}\"/>")).collect();
        std::fs::write(p, format!("<?xml version=\"1.0\"?><!DOCTYPE map SYSTEM \"http://java.sun.com/dtd/preferences.dtd\"><map MAP_XML_VERSION=\"1.0\">{body}</map>")).unwrap();
    }

    #[test]
    fn imports() {
        let home = std::env::temp_dir().join(format!("cn-import-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        write(&home, "cryptonosePreferences/prefs.xml", &[("darkMode", "false"), ("fontSizePt", "15")]);
        write(&home, "guiState/prefs.xml", &[("activeExchanges", "Huobi,Poloniex"), ("enableAlertBrowser", "true")]);
        write(&home, "alertPreferences/Poloniex/prefs.xml", &[("requiredRisingValue300", "7.5"), ("enableM30Alerts", "false"), ("permanentAlertBlocks0", "USDT_DOGE")]);
        write(&home, "pairsPreferences/Poloniex/prefs.xml", &[("markets", "USDT,BTC"), ("USDT", "50000"), ("pairsApiSymbols", "USDT_XMR")]);
        write(&home, "cryptonosePreferences/Poloniex/pinned/prefs.xml", &[("USDT_BTC", "1690000000000"), ("USDT_ETH", "0"), ("lastPos", "47")]);
        let mut s = Settings::default();
        assert!(import(&home, &mut s).unwrap());
        assert_eq!(s.general.theme, "light");
        assert_eq!(s.general.font_size, Some(15));
        assert!(s.toggles.browser);
        assert_eq!(s.active_exchanges, vec!["htx", "poloniex"]);
        let p = &s.exchanges["poloniex"];
        assert_eq!(p.alerts.periods[0].required_rise, 7.5);
        assert!(!p.alerts.periods[1].enabled);
        assert_eq!(&*p.blocks[0].symbol, "DOGE_USDT");
        assert_eq!(p.selection.criteria, vec![PairCriterion::volume("USDT", 50000.0), PairCriterion::volume("BTC", 200.0)]);
        assert_eq!(p.selection.manual, vec!["XMR_USDT"]);
        assert_eq!(p.selection.pinned, vec!["BTC_USDT"]);

        // pins from the buggy import: unpinned v2 pairs are removed, v3 pins kept
        s.exchanges.get_mut("poloniex").unwrap().selection.pinned = vec!["BTC_USDT".into(), "ETH_USDT".into(), "lastPos".into(), "SOL_USDT".into()];
        remove_unpinned(&home, &mut s);
        assert_eq!(s.exchanges["poloniex"].selection.pinned, vec!["BTC_USDT", "SOL_USDT"]);

        let _ = std::fs::remove_dir_all(&home);
        assert!(!import(&home, &mut Settings::default()).unwrap());
    }
}
