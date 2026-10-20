import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type ExchangeKind = "cex" | "dex";
export type FeedKind = "streaming" | "polling";

export interface ExchangeMeta {
  id: string;
  name: string;
  color: string;
  kind: ExchangeKind;
  feed: FeedKind;
  /** Default selection for a new tab: [quote, top N]. */
  defaultTop: [string, number][];
}

export interface Pair {
  symbol: string;
  base: string;
  quote: string;
}

export interface MarketInfo {
  pair: Pair;
  quoteVolume24h: number;
  lastPrice: number | null;
}

export interface Candle {
  ts: number;
  open: number;
  high: number;
  low: number;
  close: number;
}

export interface PeriodThresholds {
  periodSec: number;
  enabled: boolean;
  requiredRise: number;
  requiredFall: number;
  requiredRelRise: number;
  requiredRelFall: number;
  sufficientRelRise: number;
  sufficientRelFall: number;
}

export interface AlertSettings {
  periods: PeriodThresholds[];
  minLiquidityEnabled: boolean;
  minLiquidity: number;
  blockSubsequent: boolean;
  allowSubsequent2x: boolean;
}

export interface PairCriterion {
  quote: string;
  /** Minimum 24h quote volume, used when `top` isn't set. */
  minVolume: number;
  /** The N pairs with the highest 24h volume. */
  top?: number | null;
}

export interface PairSelection {
  criteria: PairCriterion[];
  manual: string[];
  blacklist: string[];
  pinned: string[];
}

export interface AlertBlock {
  symbol: string;
  until: number | null;
}

export interface ExchangeSettings {
  selection: PairSelection;
  alerts: AlertSettings;
  blocks: AlertBlock[];
}

export interface General {
  browserCommand: string | null;
  soundRising: string | null;
  soundFalling: string | null;
  /** px; null follows the system */
  fontSize: number | null;
  theme: "system" | "dark" | "light";
  connectionNotifications: boolean;
  coingeckoDemoKey: string | null;
  coingeckoProKey: string | null;
  newsFeeds: string[];
}

export interface Toggles {
  sound: boolean;
  browser: boolean;
  notification: boolean;
  powerSave: boolean;
}

export interface Settings {
  general: General;
  toggles: Toggles;
  activeExchanges: string[];
  exchanges: Record<string, ExchangeSettings>;
  hiddenVersion: string | null;
  manyPairsWarningShown: boolean;
}

export type EngineStatus = "connecting" | "connected" | "reconnecting" | "noUpdates" | "noPairs" | "error" | "stopped";

export interface PeriodUpdate {
  periodSec: number;
  percentChange: number;
  relativeChange: number | null;
  lastPercentChange: number;
  relativeLastChange: number | null;
}

export interface PairUpdate {
  symbol: string;
  lastPrice: number;
  periods: PeriodUpdate[];
}

export interface PriceAlert {
  exchange: string;
  pair: Pair;
  periodSec: number;
  ts: number;
  priceChange: number;
  relativeChange: number;
  referencePrice: number;
  finalPrice: number;
  changeTimeSec: number;
  finalPriceTs: number;
  referencePriceTs: number;
}

export interface AlertEvent {
  exchange: string;
  id: number;
  alert: PriceAlert;
  chart: Candle[];
  url: string;
}

export interface CoinInfo {
  id: string;
  symbol: string;
  name: string;
  image: string | null;
  homepage: string | null;
  geckoUrl: string;
  priceUsd: number | null;
  change24h: number | null;
  change7d: number | null;
  marketCap: number | null;
  rank: number | null;
  volume24h: number | null;
  description: string | null;
}

export interface NewsItem {
  title: string;
  url: string;
  source: string;
  publishedAt: number;
}

export interface CoinNews {
  coin: { symbol: string; name: string | null; geckoId: string | null };
  items: NewsItem[];
}

export interface Snapshot {
  status: EngineStatus | null;
  message: string | null;
  pairs: Pair[];
  rows: PairUpdate[];
  lastTickTs: number | null;
}

export interface InitData {
  exchanges: ExchangeMeta[];
  settings: Settings;
  version: string;
  maxPinned: number;
  snapshots: Record<string, Snapshot>;
  systemFontPx: number | null;
}

export interface Release {
  version: string;
  url: string;
}

export const api = {
  init: () => invoke<InitData>("init"),
  startExchange: (id: string) => invoke<Settings>("start_exchange", { id }),
  stopExchange: (id: string) => invoke<Settings>("stop_exchange", { id }),
  reconnectExchange: (id: string) => invoke<void>("reconnect_exchange", { id }),
  markets: (id: string) => invoke<MarketInfo[]>("markets", { id }),
  saveSelection: (id: string, selection: Omit<PairSelection, "pinned">) => invoke<Settings>("save_selection", { id, selection }),
  saveAlertSettings: (id: string, alerts: AlertSettings, allExchanges: boolean) =>
    invoke<Settings>("save_alert_settings", { id, alerts, allExchanges }),
  blockAlerts: (id: string, symbol: string, durationMs: number | null) => invoke<AlertBlock[]>("block_alerts", { id, symbol, durationMs }),
  unblockAlerts: (id: string, symbol: string) => invoke<AlertBlock[]>("unblock_alerts", { id, symbol }),
  alertBlocks: () => invoke<Record<string, AlertBlock[]>>("alert_blocks"),
  setPinned: (id: string, symbol: string, pinned: boolean) => invoke<Settings>("set_pinned", { id, symbol, pinned }),
  chart: (id: string, symbol: string) => invoke<Candle[]>("chart", { id, symbol }),
  openMarket: (id: string, pair: Pair) => invoke<void>("open_market", { id, pair }),
  openLink: (url: string) => invoke<void>("open_link", { url }),
  saveGeneral: (general: General) => invoke<Settings>("save_general", { general }),
  saveToggles: (toggles: Toggles) => invoke<Settings>("save_toggles", { toggles }),
  setFlags: (flags: { hiddenVersion?: string; manyPairsWarningShown?: boolean }) =>
    invoke<Settings>("set_flags", { hiddenVersion: flags.hiddenVersion ?? null, manyPairsWarningShown: flags.manyPairsWarningShown ?? null }),
  coinInfo: (symbol: string) => invoke<CoinInfo | null>("coin_info", { symbol }),
  coinNews: (symbol: string) => invoke<CoinNews>("coin_news", { symbol }),
  checkUpdate: () => invoke<Release | null>("check_update"),
};

export interface StatusEvent {
  exchange: string;
  status: EngineStatus;
  message: string | null;
}
export interface ProgressEvent {
  exchange: string;
  done: number;
  total: number;
}
export interface PairsEvent {
  exchange: string;
  pairs: Pair[];
}
export interface UpdatesEvent {
  exchange: string;
  lastTickTs: number | null;
  updates: PairUpdate[];
}

export function onEngineEvents(handlers: {
  status: (e: StatusEvent) => void;
  progress: (e: ProgressEvent) => void;
  pairs: (e: PairsEvent) => void;
  updates: (e: UpdatesEvent) => void;
  alert: (e: AlertEvent) => void;
}): Promise<UnlistenFn[]> {
  return Promise.all([
    listen<StatusEvent>("cn:status", (e) => handlers.status(e.payload)),
    listen<ProgressEvent>("cn:progress", (e) => handlers.progress(e.payload)),
    listen<PairsEvent>("cn:pairs", (e) => handlers.pairs(e.payload)),
    listen<UpdatesEvent>("cn:updates", (e) => handlers.updates(e.payload)),
    listen<AlertEvent>("cn:alert", (e) => handlers.alert(e.payload)),
  ]);
}
