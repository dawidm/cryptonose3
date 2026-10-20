//! Generic websocket feed. An exchange describes its protocol with [`WsProtocol`]; [`ws_feed`]
//! handles sharding pairs across connections, subscriptions, keepalive pings, idle detection and
//! reconnecting with exponential backoff.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use cn_core::{FeedEvent, FeedState, Pair, Tick};
use futures::stream::BoxStream;
use futures::{SinkExt, StreamExt};
use rand::Rng;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{interval_at, sleep, timeout, Instant};
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};

pub enum Frame<'a> {
    Text(&'a str),
    Binary(&'a [u8]),
}

#[derive(Default, Debug, PartialEq)]
pub struct Decoded {
    pub ticks: Vec<Tick>,
    /// Message to send back, e.g. a pong.
    pub reply: Option<String>,
}

impl Decoded {
    pub fn ticks(ticks: Vec<Tick>) -> Self {
        Self { ticks, reply: None }
    }

    pub fn reply(reply: String) -> Self {
        Self { ticks: vec![], reply: Some(reply) }
    }
}

/// Per-connection message decoder.
pub trait WsDecoder: Send {
    fn decode(&mut self, frame: Frame<'_>) -> anyhow::Result<Decoded>;
}

#[async_trait]
pub trait WsProtocol: Send + Sync + 'static {
    /// Websocket URL for a new connection (may require a REST call, e.g. for a token).
    async fn url(&self) -> anyhow::Result<String>;

    /// Max number of pairs handled by a single connection.
    fn pairs_per_connection(&self) -> usize;

    /// Messages subscribing to the given pairs (one connection's share).
    fn subscribe_msgs(&self, pairs: &[Pair]) -> Vec<String>;

    /// Delay between subscription messages, to stay within exchange limits.
    fn subscribe_interval(&self) -> Duration {
        Duration::from_millis(250)
    }

    /// Client-side ping message and its interval, if the exchange requires one.
    fn client_ping(&self) -> Option<(Duration, String)> {
        None
    }

    /// Reconnect if nothing is received for this long.
    fn idle_timeout(&self) -> Duration {
        Duration::from_secs(60)
    }

    fn decoder(&self, pairs: &[Pair]) -> Box<dyn WsDecoder>;
}

struct AbortOnDrop(Vec<JoinHandle<()>>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        for h in &self.0 {
            h.abort();
        }
    }
}

pub fn ws_feed<P: WsProtocol>(name: &'static str, proto: Arc<P>, pairs: Vec<Pair>) -> BoxStream<'static, FeedEvent> {
    let (tx, rx) = mpsc::channel::<FeedEvent>(1024);
    let per_conn = proto.pairs_per_connection().max(1);
    let handles: Vec<_> = pairs
        .chunks(per_conn)
        .enumerate()
        .map(|(i, chunk)| {
            let proto = proto.clone();
            let tx = tx.clone();
            let chunk = chunk.to_vec();
            tokio::spawn(async move {
                // spread connection attempts, some exchanges limit connections per second
                sleep(Duration::from_millis(i as u64 * 500)).await;
                connection_loop(name, i, proto, chunk, tx).await;
            })
        })
        .collect();
    let guard = AbortOnDrop(handles);
    futures::stream::unfold((rx, guard), |(mut rx, guard)| async move { rx.recv().await.map(|e| (e, (rx, guard))) }).boxed()
}

async fn connection_loop<P: WsProtocol>(name: &'static str, idx: usize, proto: Arc<P>, pairs: Vec<Pair>, tx: mpsc::Sender<FeedEvent>) {
    let mut backoff = Duration::from_secs(1);
    let _ = tx.send(FeedEvent::State(FeedState::Connecting)).await;
    loop {
        let started = Instant::now();
        let reason = match run_connection(&*proto, &pairs, &tx).await {
            Ok(()) => "connection closed".to_string(),
            Err(e) => format!("{e:#}"),
        };
        if tx.is_closed() {
            return;
        }
        warn!(exchange = name, conn = idx, "websocket disconnected: {reason}");
        let _ = tx.send(FeedEvent::State(FeedState::Reconnecting(reason))).await;
        if started.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_secs(1);
        }
        let jitter = Duration::from_millis(rand::rng().random_range(0..1000));
        sleep(backoff + jitter).await;
        backoff = (backoff * 2).min(Duration::from_secs(60));
    }
}

async fn run_connection<P: WsProtocol + ?Sized>(proto: &P, pairs: &[Pair], tx: &mpsc::Sender<FeedEvent>) -> anyhow::Result<()> {
    let url = proto.url().await?;
    let (mut ws, _) = timeout(Duration::from_secs(20), tokio_tungstenite::connect_async(&url))
        .await
        .map_err(|_| anyhow::anyhow!("connect timeout"))??;
    info!(pairs = pairs.len(), "websocket connected");
    let mut decoder = proto.decoder(pairs);
    for (i, msg) in proto.subscribe_msgs(pairs).into_iter().enumerate() {
        if i > 0 {
            sleep(proto.subscribe_interval()).await;
        }
        ws.send(Message::text(msg)).await?;
    }
    let _ = tx.send(FeedEvent::State(FeedState::Connected)).await;

    let ping = proto.client_ping();
    let ping_every = ping.as_ref().map_or(Duration::from_secs(3600), |p| p.0);
    let mut ping_timer = interval_at(Instant::now() + ping_every, ping_every);
    let idle = proto.idle_timeout();
    loop {
        tokio::select! {
            msg = timeout(idle, ws.next()) => {
                let msg = match msg {
                    Err(_) => anyhow::bail!("no data for {idle:?}"),
                    Ok(None) => return Ok(()),
                    Ok(Some(m)) => m?,
                };
                let decoded = match &msg {
                    Message::Text(t) => decoder.decode(Frame::Text(t.as_str())),
                    Message::Binary(b) => decoder.decode(Frame::Binary(b)),
                    Message::Close(c) => anyhow::bail!("closed by server: {c:?}"),
                    _ => continue,
                };
                match decoded {
                    Ok(d) => {
                        if let Some(r) = d.reply {
                            ws.send(Message::text(r)).await?;
                        }
                        if !d.ticks.is_empty() && tx.send(FeedEvent::Ticks(d.ticks)).await.is_err() {
                            return Ok(());
                        }
                    }
                    Err(e) => debug!("undecodable message: {e:#}"),
                }
            }
            _ = ping_timer.tick(), if ping.is_some() => {
                ws.send(Message::text(ping.as_ref().unwrap().1.clone())).await?;
            }
            _ = tx.closed() => return Ok(()),
        }
    }
}
