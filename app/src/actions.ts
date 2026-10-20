import { api, type Pair } from "./api";
import { useStore } from "./store";

export async function run<T>(p: Promise<T>): Promise<T | undefined> {
  try {
    return await p;
  } catch (e) {
    useStore.getState().showToast(String(e));
    return undefined;
  }
}

export async function togglePin(exchange: string, pair: Pair) {
  const s = useStore.getState().settings;
  const pinned = s?.exchanges[exchange]?.selection.pinned.includes(pair.symbol) ?? false;
  const settings = await run(api.setPinned(exchange, pair.symbol, !pinned));
  if (settings) useStore.getState().setSettings(settings);
}

export async function unpin(exchange: string, symbol: string) {
  const settings = await run(api.setPinned(exchange, symbol, false));
  if (settings) useStore.getState().setSettings(settings);
}

export function isPinned(exchange: string, symbol: string): boolean {
  return useStore.getState().settings?.exchanges[exchange]?.selection.pinned.includes(symbol) ?? false;
}

export const openMarket = (exchange: string, pair: Pair) => run(api.openMarket(exchange, pair));

export const openLink = (url: string) => run(api.openLink(url));

export async function block(exchange: string, symbol: string, durationMs: number | null) {
  await run(api.blockAlerts(exchange, symbol, durationMs));
  const label = durationMs == null ? "permanently" : `for ${BLOCK_TIMES.find((b) => b.ms === durationMs)?.label ?? ""}`;
  useStore.getState().showToast(`Alerts for ${symbol} blocked ${label}`);
}

export const BLOCK_TIMES: { label: string; ms: number | null }[] = [
  { label: "1 hour", ms: 3600_000 },
  { label: "4 hours", ms: 4 * 3600_000 },
  { label: "12 hours", ms: 12 * 3600_000 },
  { label: "1 day", ms: 24 * 3600_000 },
  { label: "Permanently", ms: null },
];

export async function updateToggles(patch: Partial<import("./api").Toggles>) {
  const s = useStore.getState().settings;
  if (!s) return;
  const settings = await run(api.saveToggles({ ...s.toggles, ...patch }));
  if (settings) useStore.getState().setSettings(settings);
}
