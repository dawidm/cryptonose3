import { useStore } from "../store";
import { openLink } from "../actions";
import { Modal } from "./Modal";

const SHORTCUTS: [string, string][] = [
  ["Ctrl+S", "Toggle alert sound"],
  ["Ctrl+B", "Toggle opening the browser on alerts"],
  ["Ctrl+N", "Toggle alert notifications"],
  ["Ctrl+P", "Power save: hide tables and pinned tickers"],
  ["F", "Find a pair in all exchanges"],
  ["E", "Next exchange tab"],
  ["↑ ↓", "Move in the table"],
  ["Enter", "Open the selected market"],
  ["Space", "Pin or unpin the selected pair"],
];

export function HelpDialog() {
  const setDialog = useStore((s) => s.setDialog);
  const version = useStore((s) => s.version);
  return (
    <Modal title="Cryptonose 3" onClose={() => setDialog(null)}>
      <p>Version {version}</p>
      <p>
        Cryptonose 3 watches live prices on exchanges and alerts you about sharp price moves. Each pair shows the price change over the last 5 and 30
        minutes, both in % and relative to the pair's typical 5 or 30 minute range, so a 3% move in a calm pair stands out more than in a volatile one.
      </p>
      <h3>Keyboard shortcuts</h3>
      <table className="simple">
        <tbody>
          {SHORTCUTS.map(([k, d]) => (
            <tr key={k}>
              <td>
                <kbd>{k}</kbd>
              </td>
              <td>{d}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="muted">
        Coin data from CoinGecko. News from the feeds configured in Settings.{" "}
        <button className="link" onClick={() => openLink("https://github.com/dawidm/cryptonose3")}>
          Source code
        </button>
      </p>
    </Modal>
  );
}
