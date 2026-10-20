import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, type CoinInfo, type CoinNews } from "../api";
import { useStore } from "../store";
import { fmtAgo, fmtCompact, fmtPct, fmtPrice, signClass } from "../format";
import { openLink } from "../actions";

function Info({ base }: { base: string }) {
  const [info, setInfo] = useState<CoinInfo | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.coinInfo(base).then(setInfo).catch((e) => setError(String(e)));
  }, [base]);
  if (error) return <div className="notice error">{error}</div>;
  if (info === undefined) return <div className="empty">Loading…</div>;
  if (info === null) return <div className="empty">{base} not found on CoinGecko.</div>;
  return (
    <div className="coin-info">
      <div className="coin-head">
        {info.image && <img src={info.image} alt="" width={28} height={28} />}
        <div>
          <strong>{info.name}</strong> <span className="muted">{info.symbol}</span>
          {info.rank && <span className="badge">#{info.rank}</span>}
        </div>
      </div>
      <dl>
        <dt>Price</dt>
        <dd className="mono">${fmtPrice(info.priceUsd)}</dd>
        <dt>24h</dt>
        <dd className={`mono ${signClass(info.change24h)}`}>{fmtPct(info.change24h)}</dd>
        <dt>7d</dt>
        <dd className={`mono ${signClass(info.change7d)}`}>{fmtPct(info.change7d)}</dd>
        <dt>Market cap</dt>
        <dd className="mono">${fmtCompact(info.marketCap)}</dd>
        <dt>24h volume</dt>
        <dd className="mono">${fmtCompact(info.volume24h)}</dd>
      </dl>
      {info.description && <p className="desc">{info.description}</p>}
      <div className="links">
        <button className="link" onClick={() => openLink(info.geckoUrl)}>
          CoinGecko
        </button>
        {info.homepage && (
          <button className="link" onClick={() => openLink(info.homepage!)}>
            Website
          </button>
        )}
      </div>
    </div>
  );
}

function News({ base }: { base: string }) {
  const [news, setNews] = useState<CoinNews | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.coinNews(base).then(setNews).catch((e) => setError(String(e)));
  }, [base]);
  if (error) return <div className="notice error">{error}</div>;
  if (!news) return <div className="empty">Loading news…</div>;
  const name = news.coin.name ?? base;
  if (!news.items.length)
    return (
      <div className="empty">
        No recent news about {name} in the last 7 days.
        {news.coin.geckoId && (
          <div>
            <button className="link" onClick={() => openLink(`https://www.coingecko.com/en/coins/${news.coin.geckoId}`)}>
              Open {name} on CoinGecko
            </button>
          </div>
        )}
      </div>
    );
  return (
    <ul className="news">
      {news.items.map((n) => (
        <li key={n.url}>
          <button className="link news-title" onClick={() => openLink(n.url)}>
            {n.title}
          </button>
          <div className="muted">
            {n.source} · {fmtAgo(n.publishedAt)}
          </div>
        </li>
      ))}
    </ul>
  );
}

export function CoinPopup() {
  const popup = useStore((s) => s.popup);
  const setPopup = useStore((s) => s.setPopup);
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: 0, top: 0 });

  useLayoutEffect(() => {
    if (!popup || !ref.current) return;
    const r = ref.current.getBoundingClientRect();
    const left = Math.max(8, Math.min(popup.anchor.x - r.width / 2, window.innerWidth - r.width - 8));
    const below = popup.anchor.y + 12;
    const top = below + r.height > window.innerHeight - 8 ? Math.max(8, popup.anchor.y - r.height - 12) : below;
    setPos({ left, top });
  }, [popup]);

  useEffect(() => {
    if (!popup) return;
    const onDown = (e: MouseEvent) => ref.current && !ref.current.contains(e.target as Node) && setPopup(null);
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setPopup(null);
    setTimeout(() => window.addEventListener("mousedown", onDown));
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [popup, setPopup]);

  if (!popup) return null;
  return (
    <div className="popup" ref={ref} style={pos} role="dialog" aria-label={popup.kind === "coin" ? "Coin info" : "News"}>
      <div className="popup-tabs">
        <button className={popup.kind === "coin" ? "active" : ""} onClick={() => setPopup({ ...popup, kind: "coin" })}>
          Info
        </button>
        <button className={popup.kind === "news" ? "active" : ""} onClick={() => setPopup({ ...popup, kind: "news" })}>
          News
        </button>
        <span className="spacer" />
        <button className="icon" onClick={() => setPopup(null)} aria-label="Close">
          ×
        </button>
      </div>
      <div className="popup-body">{popup.kind === "coin" ? <Info base={popup.base} /> : <News base={popup.base} />}</div>
    </div>
  );
}
