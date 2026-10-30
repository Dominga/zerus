//! Bounded display reads. Internal numeric cursors never cross private IPC.
use super::{feed, protocol, store::Store};
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use zerus_desktop_catalog::{self as catalog, Cursor};

pub const ABI: &str = "desktop-catalog-v1";
pub const WALKS: usize = 64;
const IDLE: Duration = Duration::from_secs(120);
const LIFETIME: Duration = Duration::from_secs(600);

#[derive(Clone)]
struct Walk {
    cursor: Cursor,
    idle: Instant,
    until: Instant,
}
#[derive(Default)]
pub struct Reader {
    // At most 64 small walk cursors, never session rows or prior output pages.
    walks: BTreeMap<String, Walk>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    request_id: String,
    op: String,
    computer: String,
    cursor: Option<String>,
}
impl Reader {
    pub fn status(&self) -> Value {
        json!({"abi":ABI,"retained_walks":self.walks.len(),"limit":WALKS,
            "idle_ms":IDLE.as_millis(),"lifetime_ms":LIFETIME.as_millis()})
    }
    pub fn request(
        &mut self,
        store: &Store,
        feed: &feed::Broker,
        epoch: &str,
        value: &Value,
    ) -> Value {
        self.at(store, feed, epoch, value, Instant::now())
    }
    fn at(
        &mut self,
        store: &Store,
        feed: &feed::Broker,
        epoch: &str,
        value: &Value,
        now: Instant,
    ) -> Value {
        self.walks
            .retain(|_, walk| now < walk.idle && now < walk.until);
        let id = value
            .get("request_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        let refused =
            |code| json!({"request_id":id,"accepted":false,"durable":false,"abi":ABI,"code":code});
        let Ok(request) = serde_json::from_value::<Request>(value.clone()) else {
            return refused("invalid_catalog_request");
        };
        if !uuid::Uuid::parse_str(&request.request_id)
            .is_ok_and(|u| u.to_string() == request.request_id)
            || request.computer.len() > 255
            || (!request.computer.is_empty()
                && (request.computer.starts_with(['-', '@'])
                    || request
                        .computer
                        .chars()
                        .any(|c| c.is_control() || c.is_whitespace())))
            || !matches!(request.op.as_str(), "catalog_page" | "catalog_close")
        {
            return refused("invalid_catalog_request");
        }
        let old = match request.cursor.as_ref() {
            None => None,
            Some(token) => {
                if !uuid::Uuid::parse_str(token).is_ok_and(|u| u.to_string() == *token) {
                    return refused("invalid_catalog_cursor");
                }
                let Some(walk) = self.walks.get(token) else {
                    return refused("catalog_cursor_expired");
                };
                if walk.cursor.computer != request.computer {
                    return refused("catalog_cursor_scope");
                }
                // A continuation is one-use even if a later read/encoding fails.
                // Unknown outcomes restart a display walk; no rows are cached.
                self.walks.remove(token)
            }
        };
        if request.op == "catalog_close" {
            return if old.is_some() {
                json!({"request_id":id,"accepted":true,"durable":false,"abi":ABI,"closed":true})
            } else {
                refused("invalid_catalog_cursor")
            };
        }
        match self.page(store, feed, epoch, &request, old, now) {
            Ok(value) => value,
            Err(_) => refused("catalog_read_refused"),
        }
    }
    fn page(
        &mut self,
        store: &Store,
        feed: &feed::Broker,
        epoch: &str,
        request: &Request,
        old: Option<Walk>,
        now: Instant,
    ) -> Result<Value> {
        let page = catalog::page(
            &store.db,
            &request.computer,
            old.as_ref().map(|w| &w.cursor),
        )?;
        let next = page.next.map(|cursor| Walk {
            cursor,
            idle: now + IDLE,
            until: old.as_ref().map_or(now + LIFETIME, |w| w.until),
        });
        ensure!(
            next.is_none() || self.walks.len() < WALKS,
            "catalog walk capacity"
        );
        let rows = page
            .rows
            .into_iter()
            .map(|r| {
                let row: Value =
                    serde_json::from_str(&r.json).context("invalid private catalog row")?;
                Ok(json!({"key":r.key,"row":row}))
            })
            .collect::<Result<Vec<_>>>()?;
        let token = next.as_ref().map(|_| uuid::Uuid::new_v4().to_string());
        let value = json!({"request_id":request.request_id,"accepted":true,"durable":false,"abi":ABI,
            "epoch":epoch,"computer":request.computer,"display_only":true,"authority":false,
            "complete":page.complete && feed.coverage(&request.computer),"items":rows,"next_cursor":token});
        // Size the exact outer envelope at the largest possible sequence number.
        // Escaping a key can exceed its stored charge: refuse only this read,
        // never let the actor's final encode terminate the service.
        let mut framed = super::frame(epoch, u64::MAX, "response", value);
        protocol::encode(&framed)?;
        if let (Some(token), Some(walk)) = (token, next) {
            self.walks.insert(token, walk);
        }
        Ok(framed["value"].take())
    }
}

#[cfg(test)]
mod tests;
