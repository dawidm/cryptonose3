import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, type Candle } from "../api";
import { useStore } from "../store";
import { fmtPct, fmtPrice, pairName, signClass } from "../format";
import { openMarket, unpin } from "../actions";
import { Sparkline } from "./Sparkline";

const CHART_REFRESH_MS = 60_000;

function PinnedTicker({ exchange, symbol }: { exchange: string; symbol: string }) {
  const pair = useStore((s) => s.exchanges[exchange]?.pairs.get(symbol));
  const row = useStore((s) => s.exchanges[exchange]?.rows.get(symbol));
  useStore((s) => s.exchanges[exchange]?.version);
  const meta = useStore((s) => s.metas.find((m) => m.id === exchange));
  const [chart, setChart] = useState<Candle[]>([]);

  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    // retry soon while the engine is still loading history
    const load = () =>
      api
        .chart(exchange, symbol)
        .catch(() => [] as Candle[])
        .then((c) => {
          if (!alive) return;
          setChart(c.slice(-49));
          timer = setTimeout(load, c.length ? CHART_REFRESH_MS : 5000);
        });
    load();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [exchange, symbol]);

  // the text never gets narrower, so a changing price or % doesn't keep moving the other tickers
  const textRef = useRef<HTMLDivElement>(null);
  const [minWidth, setMinWidth] = useState(0);
  useLayoutEffect(() => {
    const w = textRef.current?.offsetWidth ?? 0;
    if (w > minWidth) setMinWidth(w);
  });
  const fontSize = useStore((s) => s.settings?.general.fontSize);
  // widths measured at another font size don't apply
  useEffect(() => setMinWidth(0), [fontSize]);

  const last = row?.lastPrice ?? chart[chart.length - 1]?.close;
  const first = chart[0]?.open;
  const change4h = last != null && first ? (100 * (last - first)) / first : null;
  const live = chart.length ? [...chart.slice(0, -1), { ...chart[chart.length - 1], close: last ?? chart[chart.length - 1].close }] : chart;
  return (
    <div className="pinned" onDoubleClick={() => pair && openMarket(exchange, pair)} title={`${meta?.name ?? exchange} · double-click to open`}>
      <div className="pinned-text" ref={textRef} style={{ minWidth: minWidth || undefined }}>
        <div>
          <span className="ex-dot" style={{ background: meta?.color }} />
          <strong>{pair ? pairName(pair) : symbol}</strong>
          {!pair && <span className="muted" title="Pair not watched or not traded anymore"> (inactive)</span>}
        </div>
        <div className="mono">
          {fmtPrice(last, `${exchange}:${symbol}`)} <span className={signClass(change4h)}>{fmtPct(change4h, 1)}</span>
          <span className="muted"> 4h</span>
        </div>
      </div>
      <Sparkline candles={live} width={72} height={30} />
      {/* in the corner, away from the text and chart that are double-clicked */}
      <button className="unpin" title="Unpin" aria-label="Unpin" onClick={() => unpin(exchange, symbol)}>
        ×
      </button>
    </div>
  );
}

export function PinnedBar() {
  const settings = useStore((s) => s.settings);
  if (!settings || settings.toggles.powerSave) return null;
  const pins = settings.activeExchanges.flatMap((ex) => (settings.exchanges[ex]?.selection.pinned ?? []).map((symbol) => ({ ex, symbol })));
  if (!pins.length) return null;
  return (
    <div className="pinned-bar">
      {pins.map((p) => (
        <PinnedTicker key={`${p.ex}:${p.symbol}`} exchange={p.ex} symbol={p.symbol} />
      ))}
    </div>
  );
}
