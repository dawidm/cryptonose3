import { create } from "zustand";
import type { AlertEvent, EngineStatus, ExchangeMeta, Pair, PairUpdate, Settings } from "./api";

export const MAX_ALERTS = 100;

export interface ExchangeState {
  status: EngineStatus;
  message: string | null;
  progress: { done: number; total: number } | null;
  pairs: Map<string, Pair>;
  rows: Map<string, PairUpdate>;
  lastTickTs: number | null;
  /** Wall clock time of the last received update batch. */
  lastUpdateAt: number | null;
  /** Bumped on each update batch, used to re-render tables. */
  version: number;
}

const emptyExchange = (): ExchangeState => ({
  status: "connecting",
  message: null,
  progress: null,
  pairs: new Map(),
  rows: new Map(),
  lastTickTs: null,
  lastUpdateAt: null,
  version: 0,
});

export type Popup = { kind: "coin" | "news"; base: string; anchor: { x: number; y: number } } | null;
export type Dialog =
  | { kind: "pairs"; exchange: string }
  | { kind: "alertSettings"; exchange: string }
  | { kind: "blocks" }
  | { kind: "settings" }
  | { kind: "help" }
  | null;

interface AppStore {
  ready: boolean;
  version: string;
  maxPinned: number;
  systemFontPx: number | null;
  metas: ExchangeMeta[];
  settings: Settings | null;
  exchanges: Record<string, ExchangeState>;
  alerts: AlertEvent[];
  activeTab: string | null;
  find: string;
  popup: Popup;
  dialog: Dialog;
  toast: string | null;

  setInit: (metas: ExchangeMeta[], settings: Settings, version: string, maxPinned: number, systemFontPx: number | null) => void;
  setSettings: (s: Settings) => void;
  setStatus: (ex: string, status: EngineStatus, message: string | null) => void;
  setProgress: (ex: string, done: number, total: number) => void;
  setPairs: (ex: string, pairs: Pair[]) => void;
  applyUpdates: (ex: string, lastTickTs: number | null, updates: PairUpdate[]) => void;
  addAlert: (a: AlertEvent) => void;
  clearAlerts: () => void;
  removeExchange: (ex: string) => void;
  setActiveTab: (t: string | null) => void;
  setFind: (f: string) => void;
  setPopup: (p: Popup) => void;
  setDialog: (d: Dialog) => void;
  showToast: (t: string | null) => void;
}

function withExchange(s: AppStore, ex: string, f: (e: ExchangeState) => Partial<ExchangeState>) {
  const cur = s.exchanges[ex] ?? emptyExchange();
  return { exchanges: { ...s.exchanges, [ex]: { ...cur, ...f(cur) } } };
}

export const useStore = create<AppStore>((set) => ({
  ready: false,
  version: "",
  maxPinned: 10,
  systemFontPx: null,
  metas: [],
  settings: null,
  exchanges: {},
  alerts: [],
  activeTab: null,
  find: "",
  popup: null,
  dialog: null,
  toast: null,

  setInit: (metas, settings, version, maxPinned, systemFontPx) =>
    set({ ready: true, metas, settings, version, maxPinned, systemFontPx, activeTab: settings.activeExchanges[0] ?? null }),
  setSettings: (settings) => set({ settings }),
  setStatus: (ex, status, message) =>
    set((s) => withExchange(s, ex, (e) => ({ status, message, progress: status === "connected" ? null : e.progress }))),
  setProgress: (ex, done, total) => set((s) => withExchange(s, ex, () => ({ progress: done < total ? { done, total } : null }))),
  setPairs: (ex, pairs) =>
    set((s) =>
      withExchange(s, ex, (e) => {
        const rows = new Map([...e.rows].filter(([sym]) => pairs.some((p) => p.symbol === sym)));
        return { pairs: new Map(pairs.map((p) => [p.symbol, p])), rows, version: e.version + 1 };
      }),
    ),
  applyUpdates: (ex, lastTickTs, updates) =>
    set((s) =>
      withExchange(s, ex, (e) => {
        // rows map is mutated in place for speed; `version` signals the change
        for (const u of updates) e.rows.set(u.symbol, u);
        return { lastTickTs: lastTickTs ?? e.lastTickTs, lastUpdateAt: Date.now(), version: e.version + 1 };
      }),
    ),
  addAlert: (a) => set((s) => ({ alerts: [a, ...s.alerts].slice(0, MAX_ALERTS) })),
  clearAlerts: () => set({ alerts: [] }),
  removeExchange: (ex) =>
    set((s) => {
      const exchanges = { ...s.exchanges };
      delete exchanges[ex];
      return { exchanges };
    }),
  setActiveTab: (activeTab) => set({ activeTab }),
  setFind: (find) => set({ find }),
  setPopup: (popup) => set({ popup }),
  setDialog: (dialog) => set({ dialog }),
  showToast: (toast) => set({ toast }),
}));

export const metaOf = (id: string) => useStore.getState().metas.find((m) => m.id === id);
