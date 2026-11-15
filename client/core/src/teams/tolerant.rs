//! Reading a reply as the plan decides ("A reader of a reply ignores what it
//! does not know", decisions during wave 3): what is sent stays strict, and
//! what is read ignores unknown fields at every level.
//!
//! The command line's types refuse unknown fields, so that a save or an import
//! never drops one. A reply read through [`read`] gives every struct, at every
//! level, only the fields it declares, so a field the store adds later is
//! ignored instead of refusing the whole reply in a client that is already
//! installed. Everything else is still refused: a missing field, a value of
//! the wrong type, an unknown enum variant. Maps keep all their entries (their
//! keys are data, not field names).
//!
//! One kind of unknown field is never dropped silently: a field of what the
//! person is asked to confirm. [`read_noting`] also gives the path of every
//! field it left out (`templates/0/settings/delegations/future_power`), so the
//! reader can keep that fact on the row it shows.
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess,
    Visitor,
};
use serde::forward_to_deserialize_any;
use serde_json::{Error, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// `T` from a reply, ignoring the fields `T` does not declare at any level.
pub fn read<T: DeserializeOwned>(value: Value) -> Result<T, Error> {
    read_noting(value).map(|(read, _)| read)
}

/// `T` from a reply as [`read`] gives it, and the path of every field it left
/// out, segments joined by `/` (a list's item by its index), in reading order.
pub fn read_noting<T: DeserializeOwned>(value: Value) -> Result<(T, Vec<String>), Error> {
    let dropped = Rc::new(RefCell::new(Vec::new()));
    let read = T::deserialize(Tolerant {
        value,
        path: String::new(),
        dropped: dropped.clone(),
    })?;
    let paths = dropped.borrow().clone();
    Ok((read, paths))
}

type Dropped = Rc<RefCell<Vec<String>>>;

fn join(path: &str, segment: &str) -> String {
    if path.is_empty() {
        segment.to_owned()
    } else {
        format!("{path}/{segment}")
    }
}

struct Tolerant {
    value: Value,
    path: String,
    dropped: Dropped,
}

impl<'de> de::Deserializer<'de> for Tolerant {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.value {
            Value::Array(items) => visitor.visit_seq(Items {
                items: items.into_iter(),
                index: 0,
                path: self.path,
                dropped: self.dropped,
            }),
            Value::Object(map) => visitor.visit_map(Entries::new(
                map.into_iter().collect(),
                self.path,
                self.dropped,
            )),
            other => de::Deserializer::deserialize_any(other, visitor),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.value {
            Value::Null => visitor.visit_none(),
            other => visitor.visit_some(Tolerant {
                value: other,
                path: self.path,
                dropped: self.dropped,
            }),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self.value {
            Value::Object(map) => {
                let mut kept = Vec::with_capacity(map.len());
                for (key, value) in map {
                    if fields.contains(&key.as_str()) {
                        kept.push((key, value));
                    } else {
                        self.dropped.borrow_mut().push(join(&self.path, &key));
                    }
                }
                visitor.visit_map(Entries::new(kept, self.path, self.dropped))
            }
            other => Tolerant {
                value: other,
                path: self.path,
                dropped: self.dropped,
            }
            .deserialize_any(visitor),
        }
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self.value {
            Value::String(variant) => visitor.visit_enum(Variant {
                name: variant,
                content: None,
                path: self.path,
                dropped: self.dropped,
            }),
            Value::Object(map) if map.len() == 1 => {
                let (variant, content) = map.into_iter().next().expect("one entry");
                visitor.visit_enum(Variant {
                    path: join(&self.path, &variant),
                    name: variant,
                    content: Some(content),
                    dropped: self.dropped,
                })
            }
            other => de::Deserializer::deserialize_enum(other, name, variants, visitor),
        }
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct map identifier ignored_any
    }
}

struct Items {
    items: std::vec::IntoIter<Value>,
    index: usize,
    path: String,
    dropped: Dropped,
}

impl<'de> SeqAccess<'de> for Items {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        let Some(item) = self.items.next() else {
            return Ok(None);
        };
        let path = join(&self.path, &self.index.to_string());
        self.index += 1;
        seed.deserialize(Tolerant {
            value: item,
            path,
            dropped: self.dropped.clone(),
        })
        .map(Some)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.items.len())
    }
}

struct Entries {
    entries: std::vec::IntoIter<(String, Value)>,
    value: Option<(String, Value)>,
    path: String,
    dropped: Dropped,
}

impl Entries {
    fn new(entries: Vec<(String, Value)>, path: String, dropped: Dropped) -> Self {
        Self {
            entries: entries.into_iter(),
            value: None,
            path,
            dropped,
        }
    }
}

impl<'de> MapAccess<'de> for Entries {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        match self.entries.next() {
            Some((key, value)) => {
                self.value = Some((key.clone(), value));
                seed.deserialize(Value::String(key)).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        let (key, value) = self
            .value
            .take()
            .ok_or_else(|| de::Error::custom("a value without its key"))?;
        seed.deserialize(Tolerant {
            value,
            path: join(&self.path, &key),
            dropped: self.dropped.clone(),
        })
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.entries.len())
    }
}

struct Variant {
    name: String,
    content: Option<Value>,
    path: String,
    dropped: Dropped,
}

impl<'de> EnumAccess<'de> for Variant {
    type Error = Error;
    type Variant = Content;

    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Content), Error> {
        let variant = seed.deserialize(Value::String(self.name))?;
        Ok((
            variant,
            Content {
                content: self.content,
                path: self.path,
                dropped: self.dropped,
            },
        ))
    }
}

struct Content {
    content: Option<Value>,
    path: String,
    dropped: Dropped,
}

impl Content {
    fn tolerant(self, value: Value) -> Tolerant {
        Tolerant {
            value,
            path: self.path,
            dropped: self.dropped,
        }
    }
}

impl<'de> VariantAccess<'de> for Content {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        match self.content {
            None | Some(Value::Null) => Ok(()),
            Some(_) => Err(de::Error::custom("a unit variant carries no content")),
        }
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
        match self.content.clone() {
            Some(value) => seed.deserialize(self.tolerant(value)),
            None => Err(de::Error::custom("a newtype variant needs its content")),
        }
    }

    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value, Error> {
        match self.content.clone() {
            Some(value @ Value::Array(_)) => {
                de::Deserializer::deserialize_any(self.tolerant(value), visitor)
            }
            _ => Err(de::Error::custom("a tuple variant needs a list")),
        }
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self.content.clone() {
            Some(value) => {
                de::Deserializer::deserialize_struct(self.tolerant(value), "", fields, visitor)
            }
            None => Err(de::Error::custom("a struct variant needs its fields")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Exit;
    use crate::teams::catalog::form_catalog;
    use crate::teams::read::{decode, TeamRead};
    use crate::teams::view::{list, team, waiting};
    use serde_json::json;

    const TEAM: &str = "00000000-0000-4000-8000-00000000beef";

    /// The command line's pinned replies (`crates/team-plan/fixtures`).
    fn fixture(name: &str) -> Value {
        serde_json::from_str(match name {
            "catalog" => include_str!("../../../../crates/team-plan/fixtures/cli-catalog.json"),
            "list" => include_str!("../../../../crates/team-plan/fixtures/cli-list.json"),
            "status" => include_str!("../../../../crates/team-plan/fixtures/cli-status.json"),
            "status-stopped" => {
                include_str!("../../../../crates/team-plan/fixtures/cli-status-stopped.json")
            }
            "queue" => include_str!("../../../../crates/team-plan/fixtures/cli-queue.json"),
            "cards" => include_str!("../../../../crates/team-plan/fixtures/cli-cards.json"),
            "waiting" => include_str!("../../../../crates/team-plan/fixtures/cli-waiting.json"),
            "template-changed" => {
                include_str!("../../../../crates/team-plan/fixtures/cli-template-changed.json")
            }
            "storage-failed" => {
                include_str!("../../../../crates/team-plan/fixtures/cli-storage-failed.json")
            }
            other => panic!("no fixture {other}"),
        })
        .unwrap()
    }

    /// The same reply with a field this version does not know in every object,
    /// at every level, and how many objects received it.
    fn with_unknown_everywhere(value: &Value) -> (Value, usize) {
        match value {
            Value::Object(map) => {
                let mut added = 1;
                let mut out = serde_json::Map::new();
                for (key, item) in map {
                    let (item, count) = with_unknown_everywhere(item);
                    added += count;
                    out.insert(key.clone(), item);
                }
                out.insert(
                    "added_later".into(),
                    json!({"by": "a newer store", "items": [1, {"deeper": true}]}),
                );
                (Value::Object(out), added)
            }
            Value::Array(items) => {
                let mut added = 0;
                let items = items
                    .iter()
                    .map(|item| {
                        let (item, count) = with_unknown_everywhere(item);
                        added += count;
                        item
                    })
                    .collect();
                (Value::Array(items), added)
            }
            other => (other.clone(), 0),
        }
    }

    fn reply(value: &Value) -> Exit {
        Exit::Code {
            code: 0,
            stdout: value.to_string().into_bytes(),
            stderr: String::new(),
        }
    }

    /// A read's decode and view of a reply, with and without the unknown fields.
    fn same_read(read: TeamRead, name: &str, view: impl Fn(&Value) -> Value) {
        let original = fixture(name);
        let (later, added) = with_unknown_everywhere(&original);
        assert!(added > 3, "{name}: {added} objects");
        let first = decode(&read, reply(&original)).unwrap();
        let second =
            decode(&read, reply(&later)).unwrap_or_else(|error| panic!("{name}: {error:?}"));
        assert_eq!(view(&second), view(&first), "{name}");
    }

    /// The catalog ignores unknown fields at every level, except in what a
    /// template asks the person to confirm (the plan's correction): those are
    /// kept as facts of its row (catalog::tests pins their paths), and the rest
    /// of the catalog is read as without them.
    #[test]
    fn the_catalog_ignores_unknown_fields_at_every_level() {
        let original = fixture("catalog");
        let (later, added) = with_unknown_everywhere(&original);
        assert!(
            added > 30,
            "catalog, its templates, roster seats, settings, delegations, roles, pipelines and stages: {added}"
        );
        let read =
            form_catalog(&later).expect("a catalog with fields this version does not know is read");
        let without_facts = |form: &Value| {
            let mut form = form.clone();
            for row in form["catalog"]["templates"].as_array_mut().unwrap() {
                let row = row.as_object_mut().unwrap();
                row.remove("unknown_permissions");
                row.remove("unknown_fields");
            }
            form
        };
        let first = form_catalog(&original).unwrap();
        assert_eq!(without_facts(&read), without_facts(&first));
        for row in read["catalog"]["templates"].as_array().unwrap() {
            assert!(row["digest"].is_string());
            assert_eq!(
                row["unknown_permissions"],
                json!(["added_later"]),
                "{}",
                row["id"]
            );
            let fields = row["unknown_fields"].as_array().unwrap();
            assert!(
                fields.contains(&json!("settings/added_later"))
                    && fields.contains(&json!("roster/0/added_later"))
            );
            assert!(
                fields
                    .iter()
                    .all(|path| !path.as_str().unwrap().starts_with("roles/")),
                "a role's unknown field is not shown for confirmation"
            );
        }
        for row in first["catalog"]["templates"].as_array().unwrap() {
            assert_eq!(
                (&row["unknown_permissions"], &row["unknown_fields"]),
                (&json!([]), &json!([]))
            );
        }
    }

    #[test]
    fn the_list_ignores_unknown_fields_at_every_level() {
        same_read(TeamRead::List { after: None }, "list", |value| {
            serde_json::to_value(list(value)).unwrap()
        });
    }

    // The board stays readable with future fields. Editing settings is stricter:
    // an unknown field must disable replacement instead of disappearing on Save.
    fn board_with_settings_admission(value: &Value, queue: &Value) -> Value {
        let mut view = team(value, queue);
        assert_eq!(
            view.settings.is_some(),
            value["team"]["settings"].get("added_later").is_none()
        );
        view.settings = None;
        serde_json::to_value(view).unwrap()
    }

    #[test]
    fn the_status_and_its_members_ignore_unknown_fields_at_every_level() {
        let queue = fixture("queue");
        same_read(TeamRead::Status { team: TEAM.into() }, "status", |value| {
            board_with_settings_admission(value, &queue)
        });
    }

    /// A stopped team's status with the service's items and the store's
    /// refusals (zerus-63rru.40): this client reads the team as it did, with
    /// or without those fields and with unknown fields at every level.
    #[test]
    fn a_stopped_teams_status_with_service_items_reads_as_before() {
        let queue = fixture("queue");
        same_read(
            TeamRead::Status { team: TEAM.into() },
            "status-stopped",
            |value| board_with_settings_admission(value, &queue),
        );
        let stopped = fixture("status-stopped");
        assert!(stopped["service_items"]["items"]
            .as_array()
            .is_some_and(|i| !i.is_empty()));
        let mut without = stopped.clone();
        for key in ["service_items", "key_refusals", "cancel_refusals"] {
            without.as_object_mut().unwrap().remove(key);
        }
        let read = TeamRead::Status { team: TEAM.into() };
        let with = decode(&read, reply(&stopped)).unwrap();
        let without = decode(&read, reply(&without)).unwrap();
        assert_eq!(
            serde_json::to_value(team(&with, &queue)).unwrap(),
            serde_json::to_value(team(&without, &queue)).unwrap()
        );
    }

    #[test]
    fn the_queue_and_its_rows_ignore_unknown_fields_at_every_level() {
        let status = fixture("status");
        same_read(
            TeamRead::Queue {
                team: TEAM.into(),
                after: None,
            },
            "queue",
            |value| serde_json::to_value(team(&status, value)).unwrap(),
        );
    }

    #[test]
    fn the_waiting_cards_ignore_unknown_fields_at_every_level() {
        same_read(TeamRead::Waiting { after: None }, "waiting", |value| {
            serde_json::to_value(waiting(value)).unwrap()
        });
        // The team cards reply's rows, read as the waiting view reads its cards.
        let as_waiting = |cards: &Value| {
            let rows: Vec<Value> = cards["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| {
                    let mut row = row.clone();
                    row["team"] = json!(TEAM);
                    row
                })
                .collect();
            serde_json::to_value(waiting(
                &json!({"ok": true, "actor": {"kind": "person"}, "rows": rows, "next": null}),
            ))
            .unwrap()
        };
        let cards = fixture("cards");
        let (later, added) = with_unknown_everywhere(&cards);
        assert!(added > 3);
        assert_eq!(as_waiting(&later), as_waiting(&cards));
    }

    #[test]
    fn a_problem_ignores_unknown_fields_and_its_detail() {
        for name in ["storage-failed", "template-changed"] {
            let original = fixture(name);
            let (mut later, _) = with_unknown_everywhere(&original);
            later["error"]["detail"] = json!({"retry_after_ms": 50, "refs": ["x"]});
            let parse = |value: &Value| crate::teams::refusal::parse(value.to_string().as_bytes());
            assert_eq!(parse(&later), parse(&original), "{name}");
            assert!(parse(&later).is_some(), "{name}");
        }
    }

    #[test]
    fn the_reader_still_refuses_what_is_wrong_and_the_types_stay_strict() {
        use zerus_team_plan::catalog::Catalog;
        let catalog = fixture("catalog")["catalog"].clone();
        let (later, _) = with_unknown_everywhere(&catalog);
        assert!(
            serde_json::from_value::<Catalog>(later.clone()).is_err(),
            "the strict type still refuses unknown fields"
        );
        assert!(read::<Catalog>(later).is_ok());
        let mut missing = catalog.clone();
        missing["templates"][0]
            .as_object_mut()
            .unwrap()
            .remove("roster");
        assert!(
            read::<Catalog>(missing).is_err(),
            "a missing field is refused"
        );
        let mut wrong = catalog.clone();
        wrong["templates"][0]["roster"] = json!("two");
        assert!(read::<Catalog>(wrong).is_err(), "a wrong type is refused");
        let mut variant = catalog;
        variant["pipelines"][0]["stages"][0]["kind"] = json!("deploy");
        assert!(
            read::<Catalog>(variant).is_err(),
            "an unknown variant is refused"
        );
    }
}
