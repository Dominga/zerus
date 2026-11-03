use super::*;

struct Payload {
    message: Vec<u8>,
    message_bytes: usize,
    target: Vec<u8>,
    target_bytes: usize,
}
impl Payload {
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            message: row.get(0)?,
            message_bytes: row.get(1)?,
            target: row.get(2)?,
            target_bytes: row.get(3)?,
        })
    }
}

// The old cursor continues to bound ordinary documents. A second insertion
// bound fences groups that were unfinished when that cursor was captured.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Position {
    upper: i64,
    at: f64,
    parent: i64,
    part: i64,
    seen: i64,
    native_done: bool,
    done: bool,
}
impl Position {
    pub(crate) fn valid(&self) -> bool {
        self.upper > 0
            && self.at.is_finite()
            && self.at >= 0.0
            && self.parent > 0
            && self.part >= 0
            && self.seen >= 0
            && (self.part != 0 || self.seen == self.parent)
    }
}

impl Index {
    pub(crate) fn parts_position(&self) -> Result<Option<Position>> {
        if !self.has_parts()? {
            return Ok(None);
        }
        let counter: String = self.db.query_row(
            "SELECT value FROM meta WHERE key='part_publication'",
            [],
            |r| r.get(0),
        )?;
        let upper: i64 = counter.parse()?;
        ensure!(upper >= 0, "invalid part publication sequence");
        Ok((upper > 0).then_some(Position {
            upper,
            at: f64::MAX,
            parent: i64::MAX,
            part: i64::MAX,
            seen: 0,
            native_done: false,
            done: false,
        }))
    }

    pub(crate) fn query_parts(
        &self,
        gram: &str,
        pattern: &regex::Regex,
        mut cursor: Cursor,
        take: usize,
    ) -> Result<Page> {
        ensure!(
            self.has_parts()?,
            "part index is unavailable; start a new search"
        );
        let _read = self
            .db
            .is_autocommit()
            .then(|| self.db.unchecked_transaction())
            .transpose()?;
        let position = cursor.parts.as_mut().expect("a part cursor");
        // Split the same posting budget between two indexed prefixes. Bodies
        // are looked up only for the chosen head; prefixes retain small tuples.
        let bound = CANDIDATES / 2;
        let native = if position.native_done {
            Vec::new()
        } else {
            let mut statement = self.db.prepare_cached(
                "SELECT at,id FROM grams WHERE gram=?1 AND (at,id)<(?2,?3)
                ORDER BY at DESC,id DESC LIMIT ?4",
            )?;
            let rows = statement.query_map(params![gram, cursor.at, cursor.id, bound], |r| {
                Ok((r.get::<_, f64>(0)?, r.get::<_, i64>(1)?))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let parts = if position.done {
            Vec::new()
        } else {
            let mut statement = self.db.prepare_cached(
                "SELECT at,parent,part FROM part_grams
                WHERE gram=?1 AND (at,parent,part)<(?2,?3,?4)
                ORDER BY at DESC,parent DESC,part DESC LIMIT ?5",
            )?;
            let rows = statement.query_map(
                params![gram, position.at, position.parent, position.part, bound],
                |r| {
                    Ok((
                        r.get::<_, f64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            )?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let (mut n, mut p, mut candidates, mut bytes) = (0, 0, 0, 0);
        let mut hits = Vec::new();
        let more = loop {
            // An exhausted full prefix may hide a newer head than the other
            // stream. Continue in a new bounded page, never skip past it.
            if n == native.len() {
                if !position.native_done && native.len() == bound {
                    break true;
                }
                position.native_done = true;
            }
            if p == parts.len() {
                if !position.done && parts.len() == bound {
                    break true;
                }
                position.done = true;
            }
            if position.native_done && position.done {
                break false;
            }
            if hits.len() == take || candidates == CANDIDATES {
                break true;
            }
            // All ordinary messages at a timestamp precede part groups at the
            // same timestamp, so each original's parts remain consecutive.
            let ordinary = !position.native_done && (position.done || native[n].0 >= parts[p].0);
            let hit = if ordinary {
                let (_, id) = native[n];
                let raw: Option<(Payload, bool)> = self
                    .db
                    .query_row(
                        "SELECT substr(CAST(m.body AS BLOB),1,?2),length(CAST(m.body AS BLOB)),
                        substr(CAST(s.target AS BLOB),1,?3),length(CAST(s.target AS BLOB)),s.active
                    FROM messages m JOIN sources s ON s.id=m.source WHERE m.id=?1",
                        params![id, BATCH_BYTES, 20 << 10],
                        |r| Ok((Payload::read(r)?, r.get(4)?)),
                    )
                    .optional()?;
                match raw {
                    Some((payload, true)) if id <= cursor.upper => decode(pattern, &payload, None)?,
                    _ => None,
                }
            } else {
                let (_, parent, part) = parts[p];
                if position.seen == parent {
                    None
                } else {
                    let raw: Option<(Payload, Vec<u8>, i64, i64)> = self
                        .db
                        .query_row(
                            "SELECT substr(CAST(p.body AS BLOB),1,?3),length(CAST(p.body AS BLOB)),
                            substr(CAST(s.target AS BLOB),1,?4),length(CAST(s.target AS BLOB)),
                            substr(CAST(g.message AS BLOB),1,513),p.offset,g.total FROM message_parts p
                        JOIN message_groups g ON g.id=p.parent JOIN sources s ON s.id=g.source
                        WHERE p.id=?1 AND p.parent=?2 AND g.state=1 AND s.active=1
                            AND g.publication>0 AND g.publication<=?5
                            AND g.fingerprint=p.fingerprint AND p.source=g.source",
                            params![part, parent, BATCH_BYTES, 20 << 10, position.upper],
                            |r| {
                                Ok((
                                    Payload::read(r)?,
                                    r.get(4)?,
                                    r.get(5)?,
                                    r.get(6)?,
                                ))
                            },
                        )
                        .optional()?;
                    match raw {
                        Some((payload, original, offset, total)) => {
                            let part = PartMatch {
                                offset: usize::try_from(offset)?,
                                total: usize::try_from(total)?,
                            };
                            let hit = decode(pattern, &payload, Some(part))?;
                            if let Some(hit) = &hit {
                                ensure!(
                                    hit.0.message.id.as_bytes() == original,
                                    "part original message changed"
                                );
                            }
                            hit
                        }
                        None => None,
                    }
                }
            };
            if let Some((hit, size)) = hit {
                if bytes + size > PAGE_BYTES {
                    break true;
                }
                bytes += size;
                if !ordinary {
                    position.seen = parts[p].1;
                }
                hits.push(hit);
            }
            if ordinary {
                (cursor.at, cursor.id) = native[n];
                n += 1;
            } else {
                let (at, parent, part) = parts[p];
                p += 1;
                position.at = at;
                position.parent = parent;
                position.part = part;
                if position.seen == parent {
                    // The remaining windows are the same already-returned
                    // original, including across the next cursor boundary.
                    position.part = 0;
                    while p < parts.len() && parts[p].0 == at && parts[p].1 == parent {
                        p += 1;
                    }
                }
            }
            candidates += 1;
        };
        Ok(Page {
            hits,
            candidates,
            coverage: self.status()?,
            next_cursor: if more {
                Some(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&cursor)?))
            } else {
                None
            },
        })
    }
}

fn decode(
    pattern: &regex::Regex,
    payload: &Payload,
    part: Option<PartMatch>,
) -> Result<Option<(Hit, usize)>> {
    ensure!(
        payload.message_bytes <= BATCH_BYTES && payload.target_bytes <= 20 << 10,
        "damaged search index; rebuild it"
    );
    let message: Message = serde_json::from_slice(&payload.message)?;
    message.validate()?;
    if let Some(part) = &part {
        ensure!(
            part.offset <= part.total && message.text.len() <= part.total - part.offset,
            "invalid public message part span"
        );
    }
    let Some(found) = pattern.find(&message.text) else {
        return Ok(None);
    };
    let target: Target = serde_json::from_slice(&payload.target)?;
    target.key()?;
    let bytes = payload.message.len() + serde_json::to_vec(&target)?.len();
    Ok(Some((
        Hit {
            target,
            match_start: found.start(),
            match_end: found.end(),
            message,
            part,
        },
        bytes,
    )))
}

#[cfg(test)]
mod tests;
