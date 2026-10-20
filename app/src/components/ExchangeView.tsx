import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import { fmtAgo } from "../format";
import { run } from "../actions";
import { PriceTable, type TableItem } from "./PriceTable";

export const STATUS_LABEL: Record<string, string> = {
  connecting: "Connecting",
  connected: "Connected",
  reconnecting: "Reconnecting",
  noUpdates: "No updates",
  noPairs: "No pairs",
  error: "Error",
  stopped: "Stopped",
};

export function useNow(intervalMs = 1000) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(t);
  }, [intervalMs]);
  return now;
}

export function ExchangeView({ id }: { id: string }) {
  const ex = useStore((s) => s.exchanges[id]);
  const meta = useStore((s) => s.metas.find((m) => m.id === id));
  const settings = useStore((s) => s.settings);
  const setDialog = useStore((s) => s.setDialog);
  const setSettings = useStore((s) => s.setSettings);
  const powerSave = settings?.toggles.powerSave ?? false;
  const now = useNow();

  const items: TableItem[] = useMemo(() => [...(ex?.pairs.values() ?? [])].map((pair) => ({ exchange: id, pair })), [ex?.pairs, id]);

  // warn once when many pairs are selected, it can make alerts noisy
  useEffect(() => {
    if (settings && !settings.manyPairsWarningShown && items.length > 50) {
      useStore.getState().showToast(`${items.length} pairs selected. Many pairs may mean many alerts; you can raise the minimum volume in Pairs.`);
      run(api.setFlags({ manyPairsWarningShown: true })).then((s) => s && setSettings(s));
    }
  }, [items.length, settings, setSettings]);

  const status = ex?.status ?? "connecting";
  return (
    <div className="exchange-view">
      <div className="ex-toolbar">
        <span className={`status-dot st-${status}`} />
        <span className="status-text">
          {STATUS_LABEL[status]}
          {ex?.message ? ` · ${ex.message}` : ""}
        </span>
        {ex?.progress && (
          <span className="progress" title="Loading price history">
            <progress value={ex.progress.done} max={ex.progress.total} /> {ex.progress.done}/{ex.progress.total}
          </span>
        )}
        <span className="muted">
          {items.length} pairs{ex?.lastUpdateAt ? ` · updated ${fmtAgo(ex.lastUpdateAt, now)}` : ""}
          {meta?.feed === "polling" ? " · near real time" : ""}
        </span>
        <span className="spacer" />
        <button onClick={() => setDialog({ kind: "pairs", exchange: id })}>Pairs</button>
        <button onClick={() => setDialog({ kind: "alertSettings", exchange: id })}>Alert conditions</button>
        <button onClick={() => run(api.reconnectExchange(id))} title="Reload pairs and reconnect">
          Reconnect
        </button>
      </div>
      {status === "noPairs" && (
        <div className="notice">
          No pairs match your selection. <button onClick={() => setDialog({ kind: "pairs", exchange: id })}>Change pairs</button> Retrying in 5 minutes.
        </div>
      )}
      {powerSave ? (
        <div className="empty">Power save mode: tables are hidden, alerts still work. Press Ctrl+P to show them.</div>
      ) : (
        <PriceTable items={items} emptyText={status === "connected" ? "No pairs" : "Loading pairs…"} />
      )}
    </div>
  );
}

export function FindView() {
  const find = useStore((s) => s.find);
  const exchanges = useStore((s) => s.exchanges);
  const q = find.trim().toUpperCase();
  const items: TableItem[] = useMemo(() => {
    const out: TableItem[] = [];
    for (const [exchange, st] of Object.entries(exchanges)) {
      for (const pair of st.pairs.values()) {
        if (pair.base.toUpperCase().includes(q) || pair.quote.toUpperCase() === q || `${pair.base}/${pair.quote}`.includes(q)) {
          out.push({ exchange, pair });
        }
      }
    }
    return out;
    // pairs maps change identity when the pair list changes
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [q, ...Object.values(exchanges).map((e) => e.pairs)]);
  return (
    <div className="exchange-view">
      <div className="ex-toolbar">
        <span className="status-text">
          Results for “{find}” in all open exchanges: {items.length}
        </span>
      </div>
      <PriceTable items={items} showExchange emptyText="No matching pairs" />
    </div>
  );
}
