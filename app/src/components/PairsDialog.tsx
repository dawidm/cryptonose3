import { useEffect, useMemo, useState } from "react";
import { api, type MarketInfo, type PairCriterion } from "../api";
import { useStore } from "../store";
import { fmtCompact, pairName } from "../format";
import { run } from "../actions";
import { Modal, NumberInput } from "./Modal";

type Mode = "volume" | "top";

interface QuoteRow {
  quote: string;
  pairs: number;
  active: boolean;
  mode: Mode;
  minVolume: number;
  top: number;
}

const DEFAULT_TOP = 20;

function toCriterion(q: QuoteRow): PairCriterion {
  return q.mode === "top" ? { quote: q.quote, minVolume: 0, top: q.top } : { quote: q.quote, minVolume: q.minVolume };
}

/** Same selection as the engine's PairSelection::select. */
function selectPairs(markets: MarketInfo[], criteria: PairCriterion[], forced: Set<string>, black: Set<string>): Set<string> {
  const allowed = markets.filter((m) => !black.has(m.pair.symbol));
  const out = new Set(allowed.filter((m) => forced.has(m.pair.symbol)).map((m) => m.pair.symbol));
  for (const c of criteria) {
    let quoted = allowed.filter((m) => m.pair.quote.toUpperCase() === c.quote.toUpperCase());
    quoted = c.top != null ? [...quoted].sort((a, b) => b.quoteVolume24h - a.quoteVolume24h).slice(0, c.top) : quoted.filter((m) => m.quoteVolume24h >= c.minVolume);
    for (const m of quoted) out.add(m.pair.symbol);
  }
  return out;
}

export function PairsDialog({ exchange }: { exchange: string }) {
  const settings = useStore((s) => s.settings);
  const meta = useStore((s) => s.metas.find((m) => m.id === exchange));
  const setSettings = useStore((s) => s.setSettings);
  const setDialog = useStore((s) => s.setDialog);
  const sel = settings?.exchanges[exchange]?.selection;
  const [markets, setMarkets] = useState<MarketInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [quotes, setQuotes] = useState<QuoteRow[]>([]);
  const [manual, setManual] = useState<string[]>(sel?.manual ?? []);
  const [blacklist, setBlacklist] = useState<string[]>(sel?.blacklist ?? []);
  const [filter, setFilter] = useState("");

  useEffect(() => {
    api
      .markets(exchange)
      .then((m) => {
        setMarkets(m);
        const counts = new Map<string, number>();
        for (const x of m) counts.set(x.pair.quote, (counts.get(x.pair.quote) ?? 0) + 1);
        const crit = new Map((sel?.criteria ?? []).map((c) => [c.quote.toUpperCase(), c]));
        setQuotes(
          [...counts.entries()]
            .map(([quote, pairs]): QuoteRow => {
              const c = crit.get(quote.toUpperCase());
              return {
                quote,
                pairs,
                active: !!c,
                // quotes not configured yet start with top N
                mode: c && c.top == null ? "volume" : "top",
                minVolume: c && c.top == null ? c.minVolume : defaultMinVolume(quote),
                top: c?.top ?? DEFAULT_TOP,
              };
            })
            .sort((a, b) => Number(b.active) - Number(a.active) || b.pairs - a.pairs),
        );
      })
      .catch((e) => setError(String(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [exchange]);

  const criteria: PairCriterion[] = quotes.filter((q) => q.active).map(toCriterion);
  const selected = useMemo(() => {
    if (!markets) return new Set<string>();
    return selectPairs(markets, criteria, new Set([...manual, ...(sel?.pinned ?? [])]), new Set(blacklist));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [markets, quotes, manual, blacklist]);

  const shown = useMemo(() => {
    const f = filter.trim().toUpperCase();
    return (markets ?? [])
      .filter((m) => !f || pairName(m.pair).toUpperCase().includes(f) || m.pair.symbol.toUpperCase().includes(f))
      .sort((a, b) => b.quoteVolume24h - a.quoteVolume24h)
      .slice(0, 300);
  }, [markets, filter]);

  const toggleIn = (list: string[], set: (l: string[]) => void, sym: string) => set(list.includes(sym) ? list.filter((x) => x !== sym) : [...list, sym]);

  const save = async () => {
    const s = await run(api.saveSelection(exchange, { criteria, manual, blacklist }));
    if (s) {
      setSettings(s);
      setDialog(null);
    }
  };

  return (
    <Modal
      title={`${meta?.name} pairs`}
      onClose={() => setDialog(null)}
      wide
      footer={
        <>
          <span className="muted">{markets ? `${selected.size} pairs will be watched` : ""}</span>
          <span className="spacer" />
          <button onClick={() => setDialog(null)}>Cancel</button>
          <button className="primary" onClick={save} disabled={!markets}>
            Save and reload
          </button>
        </>
      }
    >
      {error && <div className="notice error">Could not load markets: {error}</div>}
      {!markets && !error && <div className="empty">Loading markets…</div>}
      {markets && (
        <div className="pairs-grid">
          <section>
            <h3>Markets</h3>
            <p className="muted">
              For each quote currency, watch pairs with at least a minimum 24h volume (in the quote currency), or the top N pairs by 24h volume. The list is
              refreshed every hour, so top N follows volume changes; a watched pair stays until it falls about 20% below the cutoff.
            </p>
            <table className="simple">
              <thead>
                <tr>
                  <th>Active</th>
                  <th>Quote</th>
                  <th className="num">Pairs</th>
                  <th>Condition</th>
                  <th className="num">Value</th>
                </tr>
              </thead>
              <tbody>
                {quotes.map((q, i) => (
                  <tr key={q.quote}>
                    <td>
                      <input type="checkbox" checked={q.active} onChange={(e) => setQuotes(quotes.map((x, j) => (j === i ? { ...x, active: e.target.checked } : x)))} />
                    </td>
                    <td>{q.quote}</td>
                    <td className="num">{q.pairs}</td>
                    <td>
                      <select value={q.mode} onChange={(e) => setQuotes(quotes.map((x, j) => (j === i ? { ...x, mode: e.target.value as Mode } : x)))}>
                        <option value="volume">Min 24h volume</option>
                        <option value="top">Top N by volume</option>
                      </select>
                    </td>
                    <td className="num">
                      {q.mode === "top" ? (
                        <NumberInput
                          key="top"
                          value={q.top}
                          min={1}
                          step={1}
                          onChange={(v) => setQuotes(quotes.map((x, j) => (j === i ? { ...x, top: Math.max(1, Math.round(v)) } : x)))}
                        />
                      ) : (
                        <NumberInput key="volume" value={q.minVolume} min={0} step={1000} onChange={(v) => setQuotes(quotes.map((x, j) => (j === i ? { ...x, minVolume: v } : x)))} />
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
          <section>
            <h3>All pairs</h3>
            <input className="filter" placeholder="Filter pairs" value={filter} onChange={(e) => setFilter(e.target.value)} />
            <table className="simple">
              <thead>
                <tr>
                  <th>Pair</th>
                  <th className="num">24h volume</th>
                  <th>Watched</th>
                  <th title="Always watch, regardless of volume">Always</th>
                  <th title="Never watch">Never</th>
                </tr>
              </thead>
              <tbody>
                {shown.map((m) => (
                  <tr key={m.pair.symbol}>
                    <td>{pairName(m.pair)}</td>
                    <td className="num mono">{fmtCompact(m.quoteVolume24h)}</td>
                    <td>{selected.has(m.pair.symbol) ? "✓" : ""}</td>
                    <td>
                      <input type="checkbox" checked={manual.includes(m.pair.symbol)} onChange={() => toggleIn(manual, setManual, m.pair.symbol)} />
                    </td>
                    <td>
                      <input type="checkbox" checked={blacklist.includes(m.pair.symbol)} onChange={() => toggleIn(blacklist, setBlacklist, m.pair.symbol)} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            {(markets?.length ?? 0) > shown.length && <p className="muted">Showing the top {shown.length} by volume; use the filter to find others.</p>}
          </section>
        </div>
      )}
    </Modal>
  );
}

function defaultMinVolume(quote: string): number {
  const q = quote.toUpperCase();
  if (q === "BTC") return 200;
  if (q === "ETH") return 1000;
  if (q === "BNB") return 10000;
  return 1_000_000;
}
