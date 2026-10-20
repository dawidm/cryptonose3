import type { Candle } from "../api";

interface Props {
  candles: Candle[];
  width?: number;
  height?: number;
  /** Price of the alert reference, drawn as a dashed line. */
  reference?: number;
}

/** Close price line chart. Color follows the direction from the first to the last close. */
export function Sparkline({ candles, width = 120, height = 32, reference }: Props) {
  if (candles.length < 2) return <svg className="spark" width={width} height={height} />;
  const closes = candles.map((c) => c.close);
  const lo = Math.min(...candles.map((c) => c.low), reference ?? Infinity);
  const hi = Math.max(...candles.map((c) => c.high), reference ?? -Infinity);
  const range = hi - lo || 1;
  const x = (i: number) => (i / (closes.length - 1)) * (width - 2) + 1;
  const y = (p: number) => height - 2 - ((p - lo) / range) * (height - 4);
  const d = closes.map((c, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(c).toFixed(1)}`).join("");
  const up = closes[closes.length - 1] >= closes[0];
  return (
    <svg className={`spark ${up ? "up" : "down"}`} width={width} height={height} viewBox={`0 0 ${width} ${height}`} aria-hidden>
      {reference != null && <line x1={0} x2={width} y1={y(reference)} y2={y(reference)} className="spark-ref" />}
      <path d={d} />
      <circle cx={x(closes.length - 1)} cy={y(closes[closes.length - 1])} r={2} />
    </svg>
  );
}
