//! Data-only answers from this actual journal owner, for the declared canonical
//! installer configuration. This is not Stop, native consent or an input path.
use crate::{
    native::{Call, Native},
    private_state::Store,
    runtime::Connector,
};
use std::{
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use zerus_watch_protocol::session_labels as wire;

// This build routes current-session reads and project membership by ID. The
// original nonce still needs the selected CLI's fresh complete-label context.
const ROUTING_BY_ID: bool = true;

pub(crate) struct Selection {
    path: PathBuf,
    directory: PathBuf,
    binding: String,
    generation: String,
}

fn now() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis()
        .try_into()
        .ok()
}
fn canonical_configuration() -> PathBuf {
    let absolute = |name| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let home = if cfg!(windows) {
        absolute("HOME").or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
    } else {
        std::env::var_os("HOME").map(PathBuf::from)
    };
    wire::configuration_path(
        home,
        absolute("XDG_CONFIG_HOME"),
        absolute("HGS_CONFIG_DIR"),
        cfg!(windows),
    )
}
fn binding(path: &Path, bytes: &[u8]) -> Option<String> {
    let canonical = path.canonicalize().ok()?;
    Some(zerus_relay::json::digest(wire::configuration_binding(
        canonical.to_str()?,
        bytes,
    )?))
}
impl Selection {
    pub(crate) fn new(path: &Path, bytes: &[u8]) -> Option<Self> {
        Self::at(path, bytes, &canonical_configuration())
    }
    fn at(path: &Path, bytes: &[u8], canonical: &Path) -> Option<Self> {
        let path = path.canonicalize().ok()?;
        if path != canonical.canonicalize().ok()? {
            return None;
        }
        Some(Self {
            directory: path.parent()?.to_owned(),
            binding: binding(&path, bytes)?,
            path,
            generation: crate::private_state::random_id().ok()?,
        })
    }
    fn current(&self) -> bool {
        let Ok(Some(store)) = Store::try_lock(&self.directory, false) else {
            return false;
        };
        let Some(name) = self.path.file_name().and_then(|name| name.to_str()) else {
            return false;
        };
        store
            .read(name)
            .ok()
            .flatten()
            .filter(|bytes| bytes.len() <= wire::CONFIGURATION_BYTES)
            .is_some_and(|bytes| binding(&self.path, &bytes).as_ref() == Some(&self.binding))
    }
    fn query(&self, now: u64) -> Option<wire::Query> {
        let store = Store::try_lock(&self.directory, false).ok()??;
        let bytes = store.read(wire::QUERY_FILE).ok()??;
        let query = wire::Query::parse(&bytes, now)?;
        (query.connector_configuration == self.binding).then_some(query)
    }
    fn publish(&self, query: &wire::Query, context: Option<wire::Context>, now: u64) {
        if !query.valid(now) || !self.current() {
            return;
        }
        let Ok(Some(store)) = Store::try_lock(&self.directory, false) else {
            return;
        };
        let Some(current) = store
            .read(wire::QUERY_FILE)
            .ok()
            .flatten()
            .and_then(|bytes| wire::Query::parse(&bytes, now))
        else {
            return;
        };
        if current != *query {
            return;
        }
        let reply = wire::Reply {
            schema: wire::SCHEMA,
            query: query.clone(),
            generation: self.generation.clone(),
            routing_by_id: ROUTING_BY_ID,
            context,
        };
        let Ok(bytes) = serde_json::to_vec(&reply) else {
            return;
        };
        if bytes.len() > wire::MAX_BYTES {
            return;
        }
        let Ok(previous) = store.read(wire::REPLY_FILE) else {
            return;
        };
        let _ = store.replace(
            wire::REPLY_FILE,
            &Store::revision(previous.as_deref()),
            &bytes,
        );
    }
}

pub(crate) async fn serve<N: Native + 'static>(
    connector: &Rc<Connector<N>>,
    selection: Option<&Selection>,
) {
    let Some(selection) = selection else {
        connector.stopping.wait().await;
        return;
    };
    let mut seen = None;
    loop {
        if connector.stopping.is_set() {
            return;
        }
        if let Some(query) = now()
            .and_then(|now| selection.query(now))
            .filter(|query| seen.as_ref() != Some(&query.nonce))
        {
            seen = Some(query.nonce.clone());
            if selection.current() {
                let remaining = now().and_then(|now| query.expires_at_millis.checked_sub(now));
                if let Some(remaining) = remaining.filter(|remaining| *remaining > 0) {
                    let mut call = Call::new(["swarm", "session-labels-context", "--json"])
                        .timeout((remaining.min(1_000) as f64) / 1_000.0);
                    call.max_output_bytes = Some(wire::MAX_BYTES);
                    let context = connector
                        .native(call)
                        .await
                        .ok()
                        .and_then(|output| {
                            serde_json::from_value::<wire::Context>(output.json()).ok()
                        })
                        .filter(wire::Context::valid)
                        .filter(|_| {
                            let flags = connector.flags.get();
                            flags.session_labels && flags.session_selector && flags.peer_session_id
                        });
                    if !connector.stopping.is_set() {
                        if let Some(now) = now() {
                            selection.publish(&query, context, now);
                        }
                    }
                }
            }
        }
        if crate::runtime::until_stopped(
            &connector.stopping,
            tokio::time::sleep(Duration::from_millis(200)),
        )
        .await
        .is_none()
        {
            return;
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{
        journal::Journal,
        native::{NativeResult, Output},
        runtime::Settings,
    };
    use std::{
        cell::{Cell, RefCell},
        os::unix::fs::PermissionsExt,
    };

    fn make_fifo(path: &std::path::Path) {
        use std::os::unix::fs::FileTypeExt;
        // rustix::fs::mkfifoat is unavailable on Apple targets. POSIX mkfifo
        // preserves real FIFO coverage without unsafe code or a new crate.
        let output = std::process::Command::new("mkfifo")
            .env_clear()
            .args(["-m", "600"])
            .arg(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let metadata = std::fs::symlink_metadata(path).unwrap();
        assert!(metadata.file_type().is_fifo());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    }

    fn fixture() -> (tempfile::TempDir, Selection, wire::Query, wire::Context) {
        let temp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = temp.path().join("connector.json");
        std::fs::write(&path, b"{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let selection = Selection::at(&path, b"{}", &path).unwrap();
        let query = wire::Query {
            schema: wire::SCHEMA,
            nonce: "a".repeat(32),
            machine: "11111111-1111-4111-8111-111111111111".into(),
            configuration: "b".repeat(64),
            connector_configuration: selection.binding.clone(),
            expires_at_millis: now().unwrap() + 2_000,
        };
        let context = wire::Context {
            schema: wire::SCHEMA,
            marker: wire::MARKER.into(),
            machine: query.machine.clone(),
            configuration: query.configuration.clone(),
            version: "fixture".into(),
            membership_by_id: true,
            labels_by_id: true,
        };
        (temp, selection, query, context)
    }
    fn put(selection: &Selection, name: &str, value: &impl serde::Serialize) {
        let store = Store::lock(&selection.directory, false).unwrap();
        let old = store.read(name).unwrap();
        store
            .replace(
                name,
                &Store::revision(old.as_deref()),
                &serde_json::to_vec(value).unwrap(),
            )
            .unwrap();
    }
    fn reply(selection: &Selection) -> Option<Vec<u8>> {
        Store::lock(&selection.directory, false)
            .unwrap()
            .read(wire::REPLY_FILE)
            .unwrap()
    }
    #[test]
    fn only_the_same_canonical_configuration_answers_an_original_query() {
        let (_temp, selection, query, context) = fixture();
        assert!(Selection::at(
            &selection.path,
            b"{}",
            &selection.path.with_file_name("custom.json")
        )
        .is_none());
        put(&selection, wire::QUERY_FILE, &query);
        assert_eq!(selection.query(now().unwrap()), Some(query.clone()));
        // A later query cannot receive this earlier query's reply.
        let mut next = query.clone();
        next.nonce = "d".repeat(32);
        put(&selection, wire::QUERY_FILE, &next);
        selection.publish(&query, Some(context.clone()), now().unwrap());
        assert!(reply(&selection).is_none());
        selection.publish(&next, Some(context.clone()), now().unwrap());
        let bytes = reply(&selection).unwrap();
        let answer: wire::Reply = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(answer.query, next);
        assert!(answer.routing_by_id);
        assert!(wire::Reply::parse(&bytes, &next, now().unwrap()).is_some());
        // A still-running foundation connector remains negative after only
        // its selected CLI is upgraded; the connector's own flag is required.
        let mut old = answer.clone();
        old.routing_by_id = false;
        assert!(
            wire::Reply::parse(&serde_json::to_vec(&old).unwrap(), &next, now().unwrap()).is_none()
        );
        let mut partial = answer.clone();
        partial.context.as_mut().unwrap().labels_by_id = false;
        assert!(wire::Reply::parse(
            &serde_json::to_vec(&partial).unwrap(),
            &next,
            now().unwrap()
        )
        .is_none());
        // Replacement of the selected configuration closes even a valid nonce.
        put(
            &selection,
            "connector.json",
            &serde_json::json!({"changed":true}),
        );
        selection.publish(&query, Some(context), now().unwrap());
        assert!(!selection.current());
    }
    #[test]
    fn busy_irregular_and_oversized_inputs_never_block_or_prove_ready() {
        use std::os::unix::fs::symlink;
        let (_temp, selection, query, context) = fixture();
        let held = Store::lock(&selection.directory, false).unwrap();
        assert!(selection.query(now().unwrap()).is_none());
        assert!(!selection.current());
        drop(held);
        let path = selection.directory.join(wire::QUERY_FILE);
        symlink(&selection.path, &path).unwrap();
        assert!(selection.query(now().unwrap()).is_none());
        std::fs::remove_file(&path).unwrap();
        make_fifo(&path);
        let start = std::time::Instant::now();
        assert!(selection.query(now().unwrap()).is_none());
        assert!(start.elapsed() < Duration::from_secs(1));
        std::fs::remove_file(&path).unwrap();
        put(
            &selection,
            wire::QUERY_FILE,
            &"x".repeat(wire::MAX_BYTES + 1),
        );
        assert!(selection.query(now().unwrap()).is_none());
        put(&selection, wire::QUERY_FILE, &query);
        std::fs::remove_file(&selection.path).unwrap();
        make_fifo(&selection.path);
        selection.publish(&query, Some(context), now().unwrap());
        assert!(reply(&selection).is_none());
    }

    struct NativeFixture {
        context: wire::Context,
        calls: RefCell<Vec<Call>>,
        replace: Cell<bool>,
        path: PathBuf,
    }
    impl Native for NativeFixture {
        async fn call(&self, call: Call) -> NativeResult {
            self.calls.borrow_mut().push(call);
            if self.replace.get() {
                std::fs::write(&self.path, b"{\"changed\":true}").unwrap();
            }
            Ok(Output::Json(serde_json::to_value(&self.context).unwrap()))
        }
    }
    async fn run(replace: bool, ready: bool) {
        let (temp, selection, query, context) = fixture();
        let journal = Journal::open(&temp.path().join("journal"), "fixture", 1_000.0).unwrap();
        let connector = Rc::new(Connector::new(
            NativeFixture {
                context,
                calls: RefCell::default(),
                replace: Cell::new(replace),
                path: selection.path.clone(),
            },
            journal,
            Settings {
                interval: 5.0,
                binding: "fixture".into(),
                default_timeout: 1.0,
                clock_scale: 1.0,
            },
            Box::new(|| 1_000.0),
        ));
        if ready {
            connector.flags.set(crate::capabilities::Flags::from_help(
                "session-labels-v1\nsession-id-selector-v1\nmobile-peer-session-id-v1",
            ));
        }
        put(&selection, wire::QUERY_FILE, &query);
        let observe = async {
            tokio::time::timeout(Duration::from_secs(1), async {
                loop {
                    if !connector.native.calls.borrow().is_empty() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
            tokio::time::sleep(Duration::from_millis(110)).await;
            assert_eq!(connector.native.calls.borrow().len(), 1);
            if replace {
                assert!(reply(&selection).is_none());
            } else {
                let bytes = reply(&selection).unwrap();
                let answer: wire::Reply = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(answer.query, query);
                assert_eq!(
                    answer.context.as_ref(),
                    ready.then_some(&connector.native.context)
                );
                assert!(answer.routing_by_id);
                assert_eq!(
                    wire::Reply::parse(&bytes, &query, now().unwrap()).is_some(),
                    ready
                );
            }
            let calls = connector.native.calls.borrow();
            assert_eq!(calls[0].argv, ["swarm", "session-labels-context", "--json"]);
            assert!(calls[0].payload.is_none() && calls[0].selected.is_none());
            assert_eq!(calls[0].max_output_bytes, Some(wire::MAX_BYTES));
            connector.stopping.set();
        };
        futures_util::future::join(serve(&connector, Some(&selection)), observe).await;
    }
    #[tokio::test]
    async fn a_live_journal_owner_answers_once_with_only_a_bounded_context_read() {
        run(false, true).await;
    }
    #[tokio::test]
    async fn a_missing_runtime_routing_marker_never_gives_a_positive() {
        run(false, false).await;
    }
    #[tokio::test]
    async fn a_configuration_changed_during_the_native_read_cannot_answer() {
        run(true, true).await;
    }
}
