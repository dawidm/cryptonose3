import { useEffect, useRef, useState } from "react";
import { api, onEngineEvents, type Release } from "./api";
import { useStore } from "./store";
import { run, updateToggles } from "./actions";
import { playAlertSound } from "./sound";
import { Header } from "./components/Header";
import { PinnedBar } from "./components/PinnedBar";
import { ExchangeView, FindView } from "./components/ExchangeView";
import { AlertsPanel } from "./components/AlertsPanel";
import { CoinPopup } from "./components/CoinPopup";
import { PairsDialog } from "./components/PairsDialog";
import { AlertSettingsDialog } from "./components/AlertSettingsDialog";
import { BlocksDialog } from "./components/BlocksDialog";
import { SettingsDialog } from "./components/SettingsDialog";
import { HelpDialog } from "./components/HelpDialog";

function useEngineEvents() {
  useEffect(() => {
    const st = useStore.getState;
    const unlisten = onEngineEvents({
      status: (e) => st().setStatus(e.exchange, e.status, e.message),
      progress: (e) => st().setProgress(e.exchange, e.done, e.total),
      pairs: (e) => st().setPairs(e.exchange, e.pairs),
      updates: (e) => st().applyUpdates(e.exchange, e.lastTickTs, e.updates),
      alert: (e) => {
        st().addAlert(e);
        if (st().settings?.toggles.sound) playAlertSound(e.alert.priceChange > 0);
      },
      settings: (s) => st().setSettings(s),
    });
    // listeners are registered before init starts the engines, so no event is missed
    unlisten.then(() =>
      api
        .init()
        .then((d) => {
          st().setInit(d.exchanges, d.settings, d.version, d.maxPinned, d.systemFontPx);
          for (const [ex, snap] of Object.entries(d.snapshots)) {
            st().setPairs(ex, snap.pairs);
            st().applyUpdates(ex, snap.lastTickTs, snap.rows);
            if (snap.status) st().setStatus(ex, snap.status, snap.message);
          }
        })
        .catch((e) => st().showToast(String(e))),
    );
    return () => {
      unlisten.then((fns) => fns.forEach((f) => f()));
    };
  }, []);
}

const DEFAULT_FONT_PX = 12;

function useTheme() {
  const general = useStore((s) => s.settings?.general);
  const systemFontPx = useStore((s) => s.systemFontPx);
  useEffect(() => {
    if (!general) return;
    const root = document.documentElement;
    if (general.theme === "system") delete root.dataset.theme;
    else root.dataset.theme = general.theme;
    root.style.fontSize = `${general.fontSize ?? systemFontPx ?? DEFAULT_FONT_PX}px`;
  }, [general, systemFontPx]);
}

function useShortcuts(findRef: React.RefObject<HTMLInputElement | null>) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const s = useStore.getState();
      const t = s.settings?.toggles;
      if (!t) return;
      const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLSelectElement;
      if (e.ctrlKey || e.metaKey) {
        const key = { s: "sound", b: "browser", n: "notification", p: "powerSave" }[e.key.toLowerCase()] as keyof typeof t | undefined;
        if (key) {
          e.preventDefault();
          updateToggles({ [key]: !t[key] });
        }
        return;
      }
      if (typing || s.dialog) return;
      if (e.key === "f" || e.key === "F") {
        e.preventDefault();
        findRef.current?.focus();
      } else if (e.key === "e" || e.key === "E") {
        const tabs = s.settings!.activeExchanges;
        if (tabs.length) s.setActiveTab(tabs[(tabs.indexOf(s.activeTab ?? "") + 1) % tabs.length]);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [findRef]);
}

function UpdateBanner() {
  const settings = useStore((s) => s.settings);
  const setSettings = useStore((s) => s.setSettings);
  const [release, setRelease] = useState<Release | null>(null);
  useEffect(() => {
    api.checkUpdate().then(setRelease).catch(() => {});
  }, []);
  if (!release || !settings || settings.hiddenVersion === release.version) return null;
  return (
    <div className="banner">
      Cryptonose 3 version {release.version} is available.
      <button className="link" onClick={() => run(api.openLink(release.url))}>
        Download
      </button>
      <button className="link" onClick={() => run(api.setFlags({ hiddenVersion: release.version })).then((s) => s && setSettings(s))}>
        Hide
      </button>
    </div>
  );
}

async function addExchange(id: string) {
  const s = await run(api.startExchange(id));
  if (s) {
    useStore.getState().setSettings(s);
    useStore.getState().setActiveTab(id);
  }
}

/** Shown when no exchange is open, e.g. on the first run. */
function NoExchanges() {
  const metas = useStore((s) => s.metas);
  return (
    <div className="no-exchanges">
      <h2>Add exchange to start tracking price changes</h2>
      <p className="muted">Choose an exchange to watch its most traded pairs. You can add more exchanges later with + Exchange.</p>
      <div className="exchange-choices">
        {metas.map((m) => (
          <button key={m.id} onClick={() => addExchange(m.id)}>
            <span className="ex-dot" style={{ background: m.color }} />
            {m.name}
            {m.kind === "dex" && <span className="badge">DEX</span>}
          </button>
        ))}
      </div>
    </div>
  );
}

function Tabs() {
  const settings = useStore((s) => s.settings);
  const metas = useStore((s) => s.metas);
  const exchanges = useStore((s) => s.exchanges);
  const activeTab = useStore((s) => s.activeTab);
  const setActiveTab = useStore((s) => s.setActiveTab);
  const setSettings = useStore((s) => s.setSettings);
  const removeExchange = useStore((s) => s.removeExchange);
  // fixed position, the tab bar scrolls horizontally and would clip a dropdown
  const [adding, setAdding] = useState<{ left: number; top: number } | null>(null);
  if (!settings) return null;
  const active = settings.activeExchanges;
  const available = metas.filter((m) => !active.includes(m.id));

  const add = (id: string) => {
    setAdding(null);
    addExchange(id);
  };
  const close = async (id: string) => {
    const s = await run(api.stopExchange(id));
    if (s) {
      setSettings(s);
      removeExchange(id);
      if (activeTab === id) setActiveTab(s.activeExchanges[0] ?? null);
    }
  };

  return (
    <div className="tabs" role="tablist">
      {active.map((id) => {
        const m = metas.find((x) => x.id === id);
        const st = exchanges[id]?.status ?? "connecting";
        return (
          <div key={id} className={`tab ${activeTab === id ? "active" : ""}`} role="tab" aria-selected={activeTab === id} onClick={() => setActiveTab(id)}>
            <span className={`status-dot st-${st}`} />
            {m?.name ?? id}
            {m?.kind === "dex" && <span className="badge">DEX</span>}
            <button
              className="tab-close"
              title={`Close ${m?.name}`}
              onClick={(e) => {
                e.stopPropagation();
                close(id);
              }}
            >
              ×
            </button>
          </div>
        );
      })}
      {available.length > 0 && (
        <span className="menu-wrap">
          <button className="tab add" onClick={(e) => {
              const r = e.currentTarget.getBoundingClientRect();
              setAdding((a) => (a ? null : { left: r.left, top: r.bottom + 2 }));
            }} title="Add exchange">
            + Exchange
          </button>
          {adding && (
            <div className="menu fixed" style={adding} onMouseLeave={() => setAdding(null)}>
              {available.map((m) => (
                <button key={m.id} onClick={() => add(m.id)}>
                  <span className="ex-dot" style={{ background: m.color }} />
                  {m.name}
                  {m.kind === "dex" && <span className="badge">DEX</span>}
                </button>
              ))}
            </div>
          )}
        </span>
      )}
    </div>
  );
}

function Dialogs() {
  const dialog = useStore((s) => s.dialog);
  switch (dialog?.kind) {
    case "pairs":
      return <PairsDialog exchange={dialog.exchange} />;
    case "alertSettings":
      return <AlertSettingsDialog exchange={dialog.exchange} />;
    case "blocks":
      return <BlocksDialog />;
    case "settings":
      return <SettingsDialog />;
    case "help":
      return <HelpDialog />;
    default:
      return null;
  }
}

function Toast() {
  const toast = useStore((s) => s.toast);
  const showToast = useStore((s) => s.showToast);
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => showToast(null), 6000);
    return () => clearTimeout(t);
  }, [toast, showToast]);
  if (!toast) return null;
  return (
    <div className="toast" role="status" onClick={() => showToast(null)}>
      {toast}
    </div>
  );
}

export default function App() {
  useEngineEvents();
  useTheme();
  const findRef = useRef<HTMLInputElement>(null);
  useShortcuts(findRef);
  const ready = useStore((s) => s.ready);
  const activeTab = useStore((s) => s.activeTab);
  const find = useStore((s) => s.find);
  const hasTabs = useStore((s) => (s.settings?.activeExchanges.length ?? 0) > 0);

  return (
    <div className="app">
      <Header ref={findRef} />
      <UpdateBanner />
      <PinnedBar />
      <main className="main">
        <section className="markets">
          <Tabs />
          {!ready ? (
            <div className="empty">Starting…</div>
          ) : find.trim() ? (
            <FindView />
          ) : activeTab && hasTabs ? (
            <ExchangeView key={activeTab} id={activeTab} />
          ) : (
            <NoExchanges />
          )}
        </section>
        <AlertsPanel />
      </main>
      <CoinPopup />
      <Dialogs />
      <Toast />
    </div>
  );
}
