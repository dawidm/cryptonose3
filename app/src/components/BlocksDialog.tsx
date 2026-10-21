import { useEffect, useState } from "react";
import { api, type AlertBlock } from "../api";
import { useStore } from "../store";
import { fmtLeft, pairName } from "../format";
import { run } from "../actions";
import { Modal } from "./Modal";

export function BlocksDialog() {
  const setDialog = useStore((s) => s.setDialog);
  const metas = useStore((s) => s.metas);
  const exchanges = useStore((s) => s.exchanges);
  const alerts = useStore((s) => s.alerts);
  const [blocks, setBlocks] = useState<Record<string, AlertBlock[]> | null>(null);

  const load = () => run(api.alertBlocks()).then((b) => b && setBlocks(b));
  useEffect(() => {
    load();
  }, []);

  const unblock = async (ex: string, symbol: string) => {
    await run(api.unblockAlerts(ex, symbol));
    load();
  };

  const entries = Object.entries(blocks ?? {});
  // blocks store the exchange symbol (e.g. "@465" on Hyperliquid spot); show the pair name when it's known
  const nameOf = (ex: string, symbol: string) => {
    const pair = exchanges[ex]?.pairs.get(symbol) ?? alerts.find((a) => a.exchange === ex && a.alert.pair.symbol === symbol)?.alert.pair;
    return pair ? pairName(pair) : symbol;
  };
  return (
    <Modal title="Blocked pairs" onClose={() => setDialog(null)}>
      {blocks && entries.length === 0 && <div className="empty">No pairs are blocked. Use “Block” on an alert to mute a pair.</div>}
      {entries.map(([ex, list]) => (
        <section key={ex}>
          <h3>{metas.find((m) => m.id === ex)?.name ?? ex}</h3>
          <table className="simple">
            <tbody>
              {list.map((b) => (
                <tr key={b.symbol}>
                  <td title={b.symbol}>{nameOf(ex, b.symbol)}</td>
                  <td className="muted">{b.until == null ? "permanently" : `until ${new Date(b.until).toLocaleString()} (${fmtLeft(b.until)} left)`}</td>
                  <td className="num">
                    <button onClick={() => unblock(ex, b.symbol)}>Unblock</button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      ))}
    </Modal>
  );
}
