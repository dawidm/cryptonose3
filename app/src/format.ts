export function fmtPrice(p: number | null | undefined): string {
  if (p == null || !isFinite(p)) return "–";
  const abs = Math.abs(p);
  const digits = abs >= 1000 ? 2 : abs >= 1 ? 4 : abs >= 0.01 ? 6 : 8;
  return p.toLocaleString("en-US", { minimumFractionDigits: 0, maximumFractionDigits: digits });
}

export function fmtPct(p: number | null | undefined, digits = 2): string {
  if (p == null || !isFinite(p)) return "–";
  return `${p > 0 ? "+" : ""}${p.toFixed(digits)}%`;
}

export function fmtRel(r: number | null | undefined): string {
  if (r == null || !isFinite(r)) return "–";
  return `${r > 0 ? "+" : ""}${r.toFixed(2)}`;
}

export function fmtCompact(v: number | null | undefined): string {
  if (v == null || !isFinite(v)) return "–";
  return v.toLocaleString("en-US", { notation: "compact", maximumFractionDigits: 2 });
}

export function fmtTime(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function fmtAgo(ms: number, now = Date.now()): string {
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return `${s}s ago`;
  if (s < 3600) return `${Math.round(s / 60)}m ago`;
  if (s < 86400) return `${Math.round(s / 3600)}h ago`;
  return `${Math.round(s / 86400)}d ago`;
}

/** Time left until `ms`, e.g. "3h". */
export function fmtLeft(ms: number, now = Date.now()): string {
  return fmtAgo(now - Math.max(0, ms - now), now).replace(" ago", "");
}

export function fmtDuration(sec: number): string {
  if (sec < 60) return `${sec}s`;
  const m = Math.floor(sec / 60);
  const s = sec % 60;
  return s ? `${m}m ${s}s` : `${m}m`;
}

export function signClass(v: number | null | undefined): string {
  if (v == null || v === 0 || !isFinite(v)) return "";
  return v > 0 ? "up" : "down";
}

export const pairName = (p: { base: string; quote: string }) => (p.quote === "PERP" ? `${p.base}-PERP` : `${p.base}/${p.quote}`);
