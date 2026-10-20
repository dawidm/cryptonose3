import { useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { Pair, PairUpdate } from "../api";
import { useStore } from "../store";
import { fmtPct, fmtPrice, fmtRel, pairName, signClass } from "../format";
import { isPinned, openMarket, togglePin } from "../actions";

export interface TableItem {
  exchange: string;
  pair: Pair;
}

type SortKey = "pair" | "last" | "p0" | "r0" | "p1" | "r1";

const RESORT_MS = 2500;
const ROW_HEIGHT = 30;

function sortValue(key: SortKey, item: TableItem, row: PairUpdate | undefined): number | string {
  switch (key) {
    case "pair":
      return pairName(item.pair);
    case "last":
      return row?.lastPrice ?? -Infinity;
    case "p0":
      return row?.periods[0]?.percentChange ?? -Infinity;
    case "r0":
      return row?.periods[0]?.relativeChange ?? -Infinity;
    case "p1":
      return row?.periods[1]?.percentChange ?? -Infinity;
    case "r1":
      return row?.periods[1]?.relativeChange ?? -Infinity;
  }
}

interface Props {
  items: TableItem[];
  showExchange?: boolean;
  emptyText?: string;
}

export function PriceTable({ items, showExchange = false, emptyText = "No pairs" }: Props) {
  const exchanges = useStore((s) => s.exchanges);
  const metas = useStore((s) => s.metas);
  const settings = useStore((s) => s.settings);
  const setPopup = useStore((s) => s.setPopup);
  const [sort, setSort] = useState<{ key: SortKey; desc: boolean }>({ key: "p0", desc: true });
  const [tick, setTick] = useState(0);
  const [selected, setSelected] = useState<string | null>(null);
  const parentRef = useRef<HTMLDivElement>(null);

  // re-sort periodically, not on every update, so rows don't jump around constantly
  useEffect(() => {
    const t = setInterval(() => setTick((x) => x + 1), RESORT_MS);
    return () => clearInterval(t);
  }, []);

  const rowOf = (it: TableItem) => exchanges[it.exchange]?.rows.get(it.pair.symbol);

  const sorted = useMemo(() => {
    const st = useStore.getState().exchanges;
    const withVal = items.map((it) => ({ it, v: sortValue(sort.key, it, st[it.exchange]?.rows.get(it.pair.symbol)) }));
    withVal.sort((a, b) => {
      const c = typeof a.v === "string" ? a.v.localeCompare(b.v as string) : (a.v as number) - (b.v as number);
      return sort.desc ? -c : c;
    });
    return withVal.map((x) => x.it);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [items, sort, tick]);

  const virt = useVirtualizer({ count: sorted.length, getScrollElement: () => parentRef.current, estimateSize: () => ROW_HEIGHT, overscan: 12 });

  const key = (it: TableItem) => `${it.exchange}:${it.pair.symbol}`;
  const selIndex = sorted.findIndex((it) => key(it) === selected);

  const onKey = (e: React.KeyboardEvent) => {
    if (!sorted.length) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const i = Math.min(sorted.length - 1, Math.max(0, selIndex + (e.key === "ArrowDown" ? 1 : -1)));
      setSelected(key(sorted[i]));
      virt.scrollToIndex(i);
    } else if (selIndex >= 0 && e.key === "Enter") {
      openMarket(sorted[selIndex].exchange, sorted[selIndex].pair);
    } else if (selIndex >= 0 && e.key === " ") {
      e.preventDefault();
      togglePin(sorted[selIndex].exchange, sorted[selIndex].pair);
    }
  };

  const header = (k: SortKey, label: string, cls = "") => (
    <div
      className={`th ${cls} ${sort.key === k ? "sorted" : ""}`}
      role="columnheader"
      onClick={() => setSort((s) => ({ key: k, desc: s.key === k ? !s.desc : k !== "pair" }))}
    >
      {label}
      {sort.key === k && <span className="sort-ind">{sort.desc ? "▼" : "▲"}</span>}
    </div>
  );

  return (
    <div className={`ptable ${showExchange ? "with-ex" : ""}`} role="grid">
      <div className="ptable-head" role="row">
        {header("pair", "Pair")}
        {header("last", "Last price", "num")}
        {header("p0", "5m %", "num")}
        {header("r0", "5m rel", "num")}
        {header("p1", "30m %", "num")}
        {header("r1", "30m rel", "num")}
        <div className="th actions" />
      </div>
      <div className="ptable-body" ref={parentRef} tabIndex={0} onKeyDown={onKey}>
        {sorted.length === 0 ? (
          <div className="empty">{emptyText}</div>
        ) : (
          <div style={{ height: virt.getTotalSize(), position: "relative" }}>
            {virt.getVirtualItems().map((v) => {
              const it = sorted[v.index];
              const row = rowOf(it);
              const p0 = row?.periods[0];
              const p1 = row?.periods[1];
              const k = key(it);
              const pinned = settings ? isPinned(it.exchange, it.pair.symbol) : false;
              const meta = metas.find((m) => m.id === it.exchange);
              return (
                <div
                  key={k}
                  className={`tr ${selected === k ? "selected" : ""}`}
                  role="row"
                  style={{ transform: `translateY(${v.start}px)`, height: ROW_HEIGHT }}
                  onClick={() => setSelected(k)}
                  onDoubleClick={() => openMarket(it.exchange, it.pair)}
                  title="Double-click or Enter opens the market, Space pins"
                >
                  <div className="td pair">
                    {showExchange && <span className="ex-dot" style={{ background: meta?.color }} title={meta?.name} />}
                    <span className="base">{it.pair.base}</span>
                    <span className="quote">{it.pair.quote === "PERP" ? "-PERP" : `/${it.pair.quote}`}</span>
                  </div>
                  <div className="td num mono">{fmtPrice(row?.lastPrice)}</div>
                  <div className={`td num mono ${signClass(p0?.percentChange)}`}>{fmtPct(p0?.percentChange)}</div>
                  <div className={`td num mono rel ${signClass(p0?.relativeChange)}`}>{fmtRel(p0?.relativeChange)}</div>
                  <div className={`td num mono ${signClass(p1?.percentChange)}`}>{fmtPct(p1?.percentChange)}</div>
                  <div className={`td num mono rel ${signClass(p1?.relativeChange)}`}>{fmtRel(p1?.relativeChange)}</div>
                  <div className="td actions">
                    <button className="mini" title="Coin info (CoinGecko)" onClick={(e) => setPopup({ kind: "coin", base: it.pair.base, anchor: { x: e.clientX, y: e.clientY } })}>
                      CG
                    </button>
                    <button className="mini" title="News" onClick={(e) => setPopup({ kind: "news", base: it.pair.base, anchor: { x: e.clientX, y: e.clientY } })}>
                      N
                    </button>
                    <button className={`mini pin ${pinned ? "on" : ""}`} title={pinned ? "Unpin" : "Pin to top bar"} onClick={() => togglePin(it.exchange, it.pair)}>
                      {pinned ? "★" : "☆"}
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
