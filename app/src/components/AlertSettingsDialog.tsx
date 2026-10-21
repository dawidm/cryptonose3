import { useState } from "react";
import { api, type AlertSettings, type PeriodThresholds } from "../api";
import { useStore } from "../store";
import { run } from "../actions";
import { Modal, NumberInput } from "./Modal";

type Preset = "low" | "medium" | "high";

/** Same values as the engine's ThresholdPreset. */
function preset(periodSec: number, p: Preset): PeriodThresholds {
  const short = periodSec <= 300;
  const [pct, rel, suff] = {
    low: short ? [3, 3, 6] : [6, 3, 6],
    medium: short ? [4, 3.5, 8] : [8, 3.5, 8],
    high: short ? [5, 4, 10] : [10, 4, 10],
  }[p];
  return {
    periodSec,
    enabled: true,
    requiredRise: pct,
    requiredFall: pct,
    requiredRelRise: rel,
    requiredRelFall: rel,
    sufficientRelRise: suff,
    sufficientRelFall: suff,
  };
}

const FIELDS: { label: string; hint: string; rise: keyof PeriodThresholds; fall: keyof PeriodThresholds }[] = [
  { label: "Required % change", hint: "Minimum price change, together with the required relative change", rise: "requiredRise", fall: "requiredFall" },
  { label: "Required relative change", hint: "Change divided by the typical candle range", rise: "requiredRelRise", fall: "requiredRelFall" },
  { label: "Sufficient relative change", hint: "Relative change that triggers an alert on its own", rise: "sufficientRelRise", fall: "sufficientRelFall" },
];

export function AlertSettingsDialog({ exchange }: { exchange: string }) {
  const settings = useStore((s) => s.settings);
  const meta = useStore((s) => s.metas.find((m) => m.id === exchange));
  const setSettings = useStore((s) => s.setSettings);
  const setDialog = useStore((s) => s.setDialog);
  const [a, setA] = useState<AlertSettings>(() => structuredClone(settings!.exchanges[exchange].alerts));
  const [confirmAll, setConfirmAll] = useState(false);

  const setPeriod = (i: number, p: Partial<PeriodThresholds>) => setA({ ...a, periods: a.periods.map((x, j) => (j === i ? { ...x, ...p } : x)) });

  const save = async (all: boolean) => {
    const s = await run(api.saveAlertSettings(exchange, a, all));
    if (s) {
      setSettings(s);
      setDialog(null);
    }
  };

  return (
    <Modal
      title={`${meta?.name} alert conditions`}
      onClose={() => setDialog(null)}
      footer={
        confirmAll ? (
          <>
            <span className="confirm">Replace alert conditions of all exchanges with these?</span>
            <span className="spacer" />
            <button onClick={() => setConfirmAll(false)}>Cancel</button>
            <button className="primary danger" onClick={() => save(true)}>
              Save for all exchanges
            </button>
          </>
        ) : (
          <>
            <span className="spacer" />
            <button onClick={() => setDialog(null)}>Cancel</button>
            <button onClick={() => setConfirmAll(true)}>Save for all exchanges</button>
            <button className="primary" onClick={() => save(false)}>
              Save
            </button>
          </>
        )
      }
    >
      <p className="muted">
        An alert fires when the price change since the lowest/highest price in the period is at least the required % change and the required relative
        change, or when the relative change alone reaches the sufficient value. Relative change compares the move with the pair's typical candle range.
      </p>
      {a.periods.map((p, i) => (
        <fieldset key={p.periodSec} className="period">
          <legend>
            <label>
              <input type="checkbox" checked={p.enabled} onChange={(e) => setPeriod(i, { enabled: e.target.checked })} /> {p.periodSec / 60} minute alerts
            </label>
          </legend>
          <div className="presets">
            Presets:
            {(["low", "medium", "high"] as Preset[]).map((pr) => (
              <button key={pr} onClick={() => setPeriod(i, { ...preset(p.periodSec, pr), enabled: p.enabled })}>
                {pr[0].toUpperCase() + pr.slice(1)}
              </button>
            ))}
          </div>
          <table className="simple thresholds">
            <thead>
              <tr>
                <th />
                <th className="num">Rising</th>
                <th className="num">Falling</th>
              </tr>
            </thead>
            <tbody>
              {FIELDS.map((f) => (
                <tr key={f.label} title={f.hint}>
                  <td>{f.label}</td>
                  <td className="num">
                    <NumberInput value={p[f.rise] as number} min={0} max={100} onChange={(v) => setPeriod(i, { [f.rise]: v })} />
                  </td>
                  <td className="num">
                    <NumberInput value={p[f.fall] as number} min={0} max={100} onChange={(v) => setPeriod(i, { [f.fall]: v })} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </fieldset>
      ))}
      <fieldset>
        <legend>Filtering</legend>
        <label className="row">
          <input type="checkbox" checked={a.minLiquidityEnabled} onChange={(e) => setA({ ...a, minLiquidityEnabled: e.target.checked })} />
          Minimum liquidity
          <NumberInput value={a.minLiquidity} min={0} max={1} step={0.05} onChange={(v) => setA({ ...a, minLiquidity: v })} />
          <span className="muted">share of recent 5m candles with any trades (0–1)</span>
        </label>
        <label className="row">
          <input type="checkbox" checked={a.blockSubsequent} onChange={(e) => setA({ ...a, blockSubsequent: e.target.checked })} />
          Block repeated alerts for the same pair for 30 minutes
        </label>
        <label className="row">
          <input type="checkbox" checked={a.allowSubsequent2x} onChange={(e) => setA({ ...a, allowSubsequent2x: e.target.checked })} />
          Still alert when the change is at least 2× the previous alert
        </label>
      </fieldset>
    </Modal>
  );
}
