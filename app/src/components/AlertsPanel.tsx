import { useState } from "react";
import type { AlertEvent } from "../api";
import { useStore } from "../store";
import { fmtDuration, fmtPct, fmtPrice, fmtRel, fmtTime, pairName } from "../format";
import { BLOCK_TIMES, block, openLink } from "../actions";
import { Sparkline } from "./Sparkline";

function BlockMenu({ a, onClose }: { a: AlertEvent; onClose: () => void }) {
  return (
    <div className="menu" onMouseLeave={onClose}>
      <div className="menu-title">Block alerts for {pairName(a.alert.pair)}</div>
      {BLOCK_TIMES.map((b) => (
        <button
          key={b.label}
          onClick={(e) => {
            e.stopPropagation();
            block(a.exchange, a.alert.pair.symbol, b.ms);
            onClose();
          }}
        >
          {b.label}
        </button>
      ))}
    </div>
  );
}

function AlertRow({ a }: { a: AlertEvent }) {
  const meta = useStore((s) => s.metas.find((m) => m.id === a.exchange));
  const setPopup = useStore((s) => s.setPopup);
  const [menu, setMenu] = useState(false);
  const al = a.alert;
  const up = al.priceChange > 0;
  return (
    <div className={`alert ${up ? "alert-up" : "alert-down"}`} onClick={() => openLink(a.url)} title="Click to open the market">
      <div className="alert-main">
        <span className="arrow">{up ? "▲" : "▼"}</span>
        <div className="alert-text">
          <div className="alert-line1">
            <strong>{pairName(al.pair)}</strong>
            <span className="pct mono">{fmtPct(al.priceChange)}</span>
            <span className="muted mono">rel {fmtRel(al.relativeChange)}</span>
          </div>
          <div className="alert-line2 muted">
            <span className="ex-tag" style={{ color: meta?.color }}>
              {meta?.name ?? a.exchange}
            </span>
            {al.periodSec / 60}m · in {fmtDuration(al.changeTimeSec)} · {fmtPrice(al.referencePrice)} → {fmtPrice(al.finalPrice)} · {fmtTime(al.finalPriceTs)}
          </div>
        </div>
        <Sparkline candles={a.chart} width={96} height={30} reference={al.referencePrice} />
      </div>
      <div className="alert-actions" onClick={(e) => e.stopPropagation()}>
        <button className="mini" onClick={(e) => setPopup({ kind: "coin", base: al.pair.base, anchor: { x: e.clientX, y: e.clientY } })}>
          CG
        </button>
        <button className="mini" onClick={(e) => setPopup({ kind: "news", base: al.pair.base, anchor: { x: e.clientX, y: e.clientY } })}>
          N
        </button>
        <span className="menu-wrap">
          <button className="mini" onClick={() => setMenu((m) => !m)} title="Block alerts for this pair">
            Block
          </button>
          {menu && <BlockMenu a={a} onClose={() => setMenu(false)} />}
        </span>
      </div>
    </div>
  );
}

export function AlertsPanel() {
  const alerts = useStore((s) => s.alerts);
  const clear = useStore((s) => s.clearAlerts);
  const setDialog = useStore((s) => s.setDialog);
  const [confirmClear, setConfirmClear] = useState(false);
  return (
    <aside className="alerts-panel">
      <div className="panel-head">
        <h2>Alerts</h2>
        <span className="spacer" />
        <button className="link" onClick={() => setDialog({ kind: "blocks" })}>
          Blocked pairs
        </button>
        {alerts.length > 0 &&
          (confirmClear ? (
            <span className="confirm">
              Clear {alerts.length} alerts?
              <button
                className="mini danger"
                onClick={() => {
                  clear();
                  setConfirmClear(false);
                }}
              >
                Clear
              </button>
              <button className="mini" onClick={() => setConfirmClear(false)}>
                Cancel
              </button>
            </span>
          ) : (
            <button className="link" onClick={() => setConfirmClear(true)}>
              Clear
            </button>
          ))}
      </div>
      <div className="alerts-list">
        {alerts.length === 0 ? (
          <div className="empty">
            No alerts yet. Alerts appear here when a price moves sharply compared to its usual range.
          </div>
        ) : (
          alerts.map((a) => <AlertRow key={a.id} a={a} />)
        )}
      </div>
    </aside>
  );
}
