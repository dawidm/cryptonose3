import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type General } from "../api";
import { useStore } from "../store";
import { run } from "../actions";
import { playAlertSound } from "../sound";
import { Modal, NumberInput } from "./Modal";

const DEFAULT_FEEDS = [
  "https://www.coindesk.com/arc/outboundfeeds/rss/",
  "https://cointelegraph.com/rss",
  "https://decrypt.co/feed",
  "https://www.theblock.co/rss.xml",
  "https://bitcoinmagazine.com/.rss/full/",
];

export function SettingsDialog() {
  const settings = useStore((s) => s.settings);
  const setSettings = useStore((s) => s.setSettings);
  const setDialog = useStore((s) => s.setDialog);
  const systemFontPx = useStore((s) => s.systemFontPx) ?? 12;
  const [g, setG] = useState<General>(() => structuredClone(settings!.general));
  const [feeds, setFeeds] = useState(g.newsFeeds.join("\n"));

  const pickSound = async (key: "soundRising" | "soundFalling") => {
    const f = await open({ multiple: false, filters: [{ name: "Audio", extensions: ["wav", "mp3", "ogg", "flac"] }] });
    if (typeof f === "string") setG({ ...g, [key]: f });
  };

  const save = async () => {
    const general = { ...g, newsFeeds: feeds.split("\n").map((s) => s.trim()).filter(Boolean) };
    const s = await run(api.saveGeneral(general));
    if (s) {
      setSettings(s);
      setDialog(null);
    }
  };

  const nullIfEmpty = (v: string) => (v.trim() ? v : null);

  return (
    <Modal
      title="Settings"
      onClose={() => setDialog(null)}
      footer={
        <>
          <span className="spacer" />
          <button onClick={() => setDialog(null)}>Cancel</button>
          <button className="primary" onClick={save}>
            Save
          </button>
        </>
      }
    >
      <fieldset>
        <legend>Appearance</legend>
        <label className="row">
          Theme
          <select value={g.theme} onChange={(e) => setG({ ...g, theme: e.target.value as General["theme"] })}>
            <option value="system">System</option>
            <option value="dark">Dark</option>
            <option value="light">Light</option>
          </select>
        </label>
        <label className="row">
          Font size
          <select value={g.fontSize == null ? "system" : "custom"} onChange={(e) => setG({ ...g, fontSize: e.target.value === "system" ? null : systemFontPx })}>
            <option value="system">System ({systemFontPx}px)</option>
            <option value="custom">Custom</option>
          </select>
          {g.fontSize != null && <NumberInput value={g.fontSize} min={9} max={24} step={1} onChange={(v) => setG({ ...g, fontSize: Math.round(v) })} />}
        </label>
      </fieldset>
      <fieldset>
        <legend>Alerts</legend>
        {(["soundRising", "soundFalling"] as const).map((k) => (
          <div className="row" key={k}>
            {k === "soundRising" ? "Rising sound" : "Falling sound"}
            <span className="path">{g[k] ?? "Default"}</span>
            <button onClick={() => pickSound(k)}>Choose…</button>
            {g[k] && <button onClick={() => setG({ ...g, [k]: null })}>Default</button>}
            <button onClick={() => playAlertSound(k === "soundRising")} title="Play the saved sound">
              ▶
            </button>
          </div>
        ))}
        <label className="row">
          Browser command
          <input
            className="grow"
            placeholder="System default browser"
            value={g.browserCommand ?? ""}
            onChange={(e) => setG({ ...g, browserCommand: nullIfEmpty(e.target.value) })}
          />
        </label>
        <p className="muted">For example: firefox --new-tab %s (%s is replaced with the page address).</p>
        <label className="row">
          <input type="checkbox" checked={g.connectionNotifications} onChange={(e) => setG({ ...g, connectionNotifications: e.target.checked })} />
          Connection status notifications
        </label>
        <label className={`row ${g.connectionNotifications ? "disabled" : ""}`} title={g.connectionNotifications ? "Included in connection status notifications" : undefined}>
          <input
            type="checkbox"
            checked={g.connectionNotifications || g.connectionErrorNotifications}
            disabled={g.connectionNotifications}
            onChange={(e) => setG({ ...g, connectionErrorNotifications: e.target.checked })}
          />
          Connection error notifications
        </label>
        <p className="muted">Status: connected, reconnecting, no updates, errors. Errors: the exchange can't be reached, no pairs found, or no price updates for 5 minutes.</p>
        <label className="row">
          <input type="checkbox" checked={g.closeToTray} onChange={(e) => setG({ ...g, closeToTray: e.target.checked })} />
          Show tray icon, closing the window keeps Cryptonose 3 running in the tray
          <span className="badge experimental">Experimental</span>
        </label>
        {g.closeToTray && (
          <p className="muted">Quit from the tray icon menu. On Linux the icon needs a desktop with tray (AppIndicator) support, plain GNOME needs an extension.</p>
        )}
      </fieldset>
      <fieldset>
        <legend>Coin info and news</legend>
        <label className="row">
          CoinGecko Demo API key
          <input className="grow" placeholder="Optional, raises rate limits" value={g.coingeckoDemoKey ?? ""} onChange={(e) => setG({ ...g, coingeckoDemoKey: nullIfEmpty(e.target.value) })} />
        </label>
        <label className="row">
          CoinGecko paid API key
          <input className="grow" placeholder="Optional, adds CoinGecko news" value={g.coingeckoProKey ?? ""} onChange={(e) => setG({ ...g, coingeckoProKey: nullIfEmpty(e.target.value) })} />
        </label>
        <label className="stacked">
          News feeds (RSS or Atom, one per line)
          <textarea rows={5} value={feeds} onChange={(e) => setFeeds(e.target.value)} />
        </label>
        <button className="link" onClick={() => setFeeds(DEFAULT_FEEDS.join("\n"))}>
          Restore default feeds
        </button>
      </fieldset>
    </Modal>
  );
}
