//! Bounded readiness notifications, NOT delivery acknowledgements. Payloads
//! stay encrypted in the ordinary receipt path, with its fresh authorization.
//! A watch is a set of durable request IDs; replacement/reconnect always checks
//! committed state before waiting, so no event cursor or command replay is
//! needed to recover a missed notification.
use super::*;
use axum::extract::{
    ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    FromRequestParts,
};
use std::collections::BTreeSet;

const FRAME: usize = 4096;
const LIFETIME: Duration = Duration::from_secs(60);

impl App {
    pub(super) async fn result_stream(
        &self,
        unread: &mut Option<Request>,
        c: Credential,
        admission: ResourcePermit,
    ) -> Result<Response> {
        let request = unread.take().ok_or(Error::BAD)?;
        let (mut parts, _) = request.into_parts();
        let slot = parts
            .extensions
            .get::<ConnectionSlot>()
            .cloned()
            .ok_or(Error::BAD)?;
        let ws = WebSocketUpgrade::from_request_parts(&mut parts, &())
            .await
            .map_err(|_| Error::BAD)?;
        let deadline = Instant::now() + LIFETIME;
        let class = admission.class;
        let lease = self.poll(&c, 65.0, admission).await?;
        let app = self.clone();
        let tracker = self.store.db.tasks.clone();
        Ok(ws
            .read_buffer_size(FRAME)
            .write_buffer_size(0)
            .max_message_size(FRAME)
            .max_frame_size(FRAME)
            .max_write_buffer_size(FRAME * 2)
            .on_upgrade(move |socket| async move {
                // Registration precedes work; shutdown tracks upgraded streams
                // and RAII cleanup, rather than detached unbounded sockets.
                let _ = tracker
                    .spawn(async move {
                        let _slot = slot.0;
                        let _lease = lease;
                        let _ = tokio::time::timeout_at(
                            deadline.into(),
                            app.stream_results(socket, c, class, deadline),
                        )
                        .await;
                    })
                    .await;
            }))
    }

    async fn stream_results(
        &self,
        mut socket: WebSocket,
        c: Credential,
        class: ServiceClass,
        deadline: Instant,
    ) -> Result<()> {
        let topic = format!("workspace:{}", c.workspace);
        // Subscribe BEFORE the first metadata query; commits between the query
        // and wait remain queued. PG NOTIFY wakes other relay workers too.
        let mut wake = self.store.db.wake.subscribe();
        let mut ids = Vec::new();
        let mut sent = BTreeSet::new();
        let mut messages = 0;
        let mut refresh = false;
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            if refresh {
                let database = self.admit(&self.database, 1, class).await?;
                let rows = self.store.encrypted_result_hints(&c, &ids).await?;
                drop(database); // NEVER hold a database permit during socket I/O.
                for row in rows {
                    let id = row["id"].as_str().ok_or(Error::BAD)?;
                    if sent.insert(id.to_owned()) {
                        let frame = json!({"type":"ready","request_id":id,"state":row["state"]})
                            .to_string();
                        tokio::time::timeout(
                            Duration::from_secs(2),
                            socket.send(Message::Text(frame.into())),
                        )
                        .await
                        .map_err(|_| Error::TIMEOUT)?
                        .map_err(|_| Error::UNAVAILABLE)?;
                    }
                }
                refresh = false;
            }
            tokio::select! {
                biased;
                _ = self.stop.cancelled() => return Ok(()),
                _ = tokio::time::sleep_until((deadline - Duration::from_secs(1)).into()) => {
                    // Normal rotation must not look like a network failure and
                    // disable the optimization for the client's cooldown.
                    let _ = tokio::time::timeout(Duration::from_secs(1), socket.send(Message::Close(Some(CloseFrame { code: 1000, reason: "".into() })))).await;
                    return Ok(());
                }
                incoming = socket.recv() => {
                    let Some(Ok(incoming)) = incoming else { return Ok(()); };
                    match incoming {
                        Message::Text(text) => {
                            messages += 1;
                            if messages > 128 { return Err(Error::BUSY); }
                            let value = json::parse(text.as_bytes(), 4, 64)?;
                            protocol::fields(&value, &["type", "request_ids"], &[])?;
                            if value["type"] != "watch" { return Err(Error::BAD); }
                            let raw = value["request_ids"].as_array().filter(|v| v.len() <= 16).ok_or(Error::BAD)?;
                            let mut next = BTreeSet::new();
                            for id in raw { next.insert(protocol::uuid(id, false)?.to_owned()); }
                            sent.retain(|id| next.contains(id));
                            ids = next.into_iter().collect();
                            refresh = true;
                        }
                        Message::Close(_) => return Ok(()),
                        Message::Ping(_) | Message::Pong(_) => {},
                        Message::Binary(_) => return Err(Error::BAD),
                    }
                }
                notice = wake.recv() => {
                    refresh = !matches!(notice, Ok(s) if !s.is_empty() && s != topic);
                }
                _ = tick.tick() => { refresh = true; }
            }
        }
    }
}
