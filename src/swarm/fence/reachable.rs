//! Current-cohort migration. Captures are frozen data, never acknowledgements
//! or native authority. Only the final ordinary fence admission may dispatch.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use zerus_replica::{Capture, CutoverSeal, NotCutOver};

#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Request {
    Prepare,
    Export {
        seal: String,
    },
    Adopt {
        capture: Capture,
        bytes: String,
        seal: String,
        source_node: String,
        source_store: String,
        catch_up: bool,
    },
}

fn open(config: &Config, project: &str, deadline: Instant) -> Result<ProjectStore> {
    ProjectStore::open_within(&config.state.join("replica"), project, left(deadline))?
        .ok_or_else(|| anyhow::anyhow!("the task store is busy past the original deadline"))
}

pub(super) fn floor_born(
    config: &Config,
    project: &str,
    uuid: &str,
    deadline: Instant,
) -> Result<()> {
    let _catalog = LockedStore::load_until(config, deadline)?
        .ok_or_else(|| anyhow::anyhow!("the native catalog admission is busy"))?;
    let store = open(config, project, deadline)?;
    store.settle_cutover(uuid)?;
    let held = store.project()?.cutover_hold().is_some_and(|hold| hold.on);
    let mut admitted = store
        .admit_until(deadline)
        .ok_or_else(|| anyhow::anyhow!("the native task admission is busy"))?;
    let mut task = admitted.load_complete(uuid)?;
    if task.floor_record().is_none() && task.cutover_seal()?.is_some() {
        ensure!(!held, "the project's cutover is held");
        admitted.cutover_ready(&task)?;
        ensure!(
            task.cutover_seal()?.is_some_and(|seal| seal.root.is_none()),
            "the historical execution floor is missing"
        );
        task.floor()?;
        admitted.save(&mut task)?;
    }
    Ok(())
}

fn request(
    universe: &Universe,
    node: &Node,
    project: &str,
    uuid: &str,
    action: Request,
    deadline: Instant,
) -> Value {
    json!({"swarm_id":universe.swarm,"project":project,"task":uuid,"version":{},"updates":[],
        "store":node.store,"remaining_ms":left(deadline).as_millis() as u64,"cutover":action})
}

fn ask(
    net: &dyn Net,
    universe: &Universe,
    node: &Node,
    project: &str,
    uuid: &str,
    action: Request,
    deadline: Instant,
) -> Result<Value> {
    let stores = universe
        .stores(node)
        .ok_or_else(|| anyhow::anyhow!("the task source has no verified route"))?;
    let reply = net.call(
        &universe.swarm,
        Route {
            alias: &node.alias,
            nodes: &node.route,
            stores: &stores,
        },
        "replica-task",
        Some(&request(universe, node, project, uuid, action, deadline)),
        deadline.min(Instant::now() + Duration::from_secs(2)),
    )?;
    ensure!(
        reply["node_id"] == node.node
            && reply["store"].as_str() == node.store.as_deref()
            && reply["task"] == uuid,
        "the task source changed computer, store or task"
    );
    let answer = reply["cutover"].clone();
    ensure!(
        answer["kind"] != "refused",
        "{}",
        answer["error"]
            .as_str()
            .unwrap_or("the cutover was refused")
    );
    Ok(answer)
}

/// Runs before the parent opens a project, so source verification never
/// reacquires a native writer or holds a store lock over the network.
pub(super) fn serve(
    config: &Config,
    net: &dyn Net,
    project: &str,
    uuid: &str,
    native: (&str, &str),
    action: Request,
    deadline: Instant,
) -> Result<Value> {
    let (node, own_store) = native;
    match action {
        Request::Prepare => {
            let store = open(config, project, deadline)?;
            store.settle_cutover(uuid)?;
            let (capture, bytes) = store.prepare_cutover(uuid, node)?;
            ensure!(
                capture.store == own_store,
                "the replica store changed during prepare"
            );
            let admitted = store
                .export_cutover(uuid)
                .ok()
                .map(|(seal, _)| seal.identity());
            Ok(
                json!({"kind":"prepared","capture":capture,"bytes":STANDARD.encode(bytes),"admitted_seal":admitted}),
            )
        }
        Request::Export { seal } => {
            let store = open(config, project, deadline)?;
            let (admitted, bytes) = store.export_cutover(uuid)?;
            ensure!(
                seal.is_empty() || admitted.identity() == seal,
                "the source holds another admitted seal"
            );
            Ok(json!({"kind":"admitted","seal":admitted.identity(),"bytes":STANDARD.encode(bytes)}))
        }
        Request::Adopt {
            capture,
            bytes,
            seal,
            source_node,
            source_store,
            catch_up,
        } => {
            ensure!(
                capture.node == node && capture.store == own_store,
                "the adoption names another native computer or store"
            );
            let bytes = STANDARD.decode(bytes)?;
            let candidate = TaskDoc::from_bytes(&bytes)?;
            candidate.validate_cutover_history()?;
            ensure!(
                candidate.uuid() == uuid
                    && candidate.view()?.project == project
                    && candidate
                        .cutover_seal()?
                        .as_ref()
                        .map(CutoverSeal::identity)
                        .as_deref()
                        == Some(&seal),
                "the proposal names another task, project or seal"
            );
            {
                let store = open(config, project, deadline)?;
                store.retain_cutover_proposal(&bytes, &capture)?;
            }
            if catch_up {
                let universe = universe_with(config, net, deadline)?;
                let source = universe
                    .nodes
                    .get(&source_node)
                    .filter(|source| {
                        source.closed.is_none() && source.store.as_deref() == Some(&source_store)
                    })
                    .ok_or_else(|| {
                        anyhow::anyhow!("the admitted seal source has no fresh verified route")
                    })?;
                let proof = ask(
                    net,
                    &universe,
                    source,
                    project,
                    uuid,
                    Request::Export { seal: seal.clone() },
                    deadline,
                )?;
                ensure!(
                    proof["kind"] == "admitted" && proof["seal"] == seal,
                    "the source did not export an admitted immutable seal"
                );
                let source_doc = TaskDoc::from_bytes(
                    &STANDARD.decode(
                        proof["bytes"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("the source export has no bytes"))?,
                    )?,
                )?;
                source_doc.validate_cutover_history()?;
                ensure!(
                    source_doc.uuid() == uuid
                        && source_doc.view()?.project == project
                        && source_doc.cutover_seal()? == candidate.cutover_seal()?,
                    "the source export has another root"
                );
            } else {
                let proposed = candidate.cutover_seal()?.expect("checked above");
                ensure!(
                    proposed
                        .captures
                        .get(&source_node)
                        .is_some_and(|source| source.store == source_store)
                        || source_node == node && source_store == own_store,
                    "the proposal has no bound producing capture"
                );
            }
            let store = open(config, project, deadline)?;
            ensure!(
                ProjectStore::store_id_until(&config.state.join("replica"), deadline)?.as_deref()
                    == Some(own_store),
                "the native store changed before adoption"
            );
            if catch_up {
                store.catch_up_cutover(&bytes, &capture, &source_node, &source_store, &seal)?;
            } else {
                store.adopt_cutover(&bytes, Some(&capture))?;
            }
            let current = store.load(uuid)?;
            Ok(json!({"kind":"adopted","seal":seal,"version":current.applied_version()}))
        }
    }
}

/// An uncut prefix must look for an already admitted current root before
/// considering the historical C2 first-floor constructor. This read-only
/// probe cannot freeze, mint a root or authorize an offer by itself.
pub(super) fn catch_up_existing(
    config: &Config,
    net: &dyn Net,
    project: &str,
    uuid: &str,
    deadline: Instant,
) -> Result<bool> {
    let first = universe_with(config, net, deadline)?;
    if first.closed().is_some() {
        return Ok(false);
    }
    let readers: Vec<_> = first
        .nodes
        .values()
        .filter(|node| node.closed.is_none() && !node.no_replica)
        .collect();
    let probe_end = probe_pool_end(net, deadline);
    let mut found = false;
    for (index, node) in readers.iter().enumerate() {
        // A slow optional reader gets only its fair share. A healthy source
        // later in native-ID order must still be asked for its original seal.
        let share = left(probe_end) / u32::try_from(readers.len() - index).unwrap_or(u32::MAX);
        let probe_deadline = deadline.min(probe_end).min(Instant::now() + share);
        if ask(
            net,
            &first,
            node,
            project,
            uuid,
            Request::Export {
                seal: String::new(),
            },
            probe_deadline,
        )
        .is_ok_and(|answer| {
            answer["kind"] == "admitted"
                && answer["seal"].as_str().is_some_and(|seal| seal.len() == 64)
        }) {
            found = true;
            break;
        }
    }
    if found {
        upgrade(config, net, &first, project, uuid, deadline)?;
    }
    Ok(found)
}

struct Frozen<'a> {
    node: Option<&'a Node>,
    capture: Capture,
    bytes: Vec<u8>,
    admitted: Option<String>,
}

pub(super) fn negative(universe: &Universe) -> BTreeMap<String, NotCutOver> {
    universe
        .nodes
        .values()
        .filter_map(|node| {
            node.closed.map(|reason| {
                (
                    node.node.clone(),
                    NotCutOver {
                        node: node.node.clone(),
                        store: node
                            .store
                            .clone()
                            .or_else(|| node.not_cut_over_store.clone()),
                        reason: if [
                            "unroutable",
                            "unreachable",
                            "unverified",
                            "deadline",
                            "task_schema",
                            "fence_unsupported",
                        ]
                        .contains(&reason)
                        {
                            reason
                        } else {
                            "unverified"
                        }
                        .to_owned(),
                    },
                )
            })
        })
        .collect()
}

/// Returns task-local uncut-over facts. A failed/absent participant is never
/// counted as an adopter, and can never enlarge the immutable prior root.
pub(super) fn upgrade(
    config: &Config,
    net: &dyn Net,
    first: &Universe,
    project: &str,
    uuid: &str,
    deadline: Instant,
) -> Result<BTreeMap<String, NotCutOver>> {
    let mut absent = negative(first);
    // An uncertain original is lookup/settlement only, never a new seal.
    let pending = {
        let store = open(config, project, deadline)?;
        store.settle_cutover(uuid)?;
        store
            .original_cutover_proposal(uuid)?
            .zip(store.pending_cutover_capture(uuid)?)
    };
    if let Some((bytes, capture)) = pending {
        let resumed = {
            let store = open(config, project, deadline)?;
            store.adopt_cutover(&bytes, Some(&capture)).is_ok()
        };
        if !resumed {
            let original = TaskDoc::from_bytes(&bytes)?
                .cutover_seal()?
                .ok_or_else(|| anyhow::anyhow!("the original proposal has no seal"))?;
            let mut source = None;
            for node in first
                .nodes
                .values()
                .filter(|node| node.closed.is_none() && !node.no_replica)
            {
                if ask(
                    net,
                    first,
                    node,
                    project,
                    uuid,
                    Request::Export {
                        seal: original.identity(),
                    },
                    deadline,
                )
                .is_ok()
                {
                    source = Some(node);
                    break;
                }
            }
            let source=source.ok_or_else(||anyhow::anyhow!("the original cutover is retained; no current source can confirm its already admitted seal"))?;
            let store = open(config, project, deadline)?;
            store.catch_up_cutover(
                &bytes,
                &capture,
                &source.node,
                source.store.as_deref().expect("verified source"),
                &original.identity(),
            )?;
        }
    }
    let (own, bytes, admitted) = {
        let store = open(config, project, deadline)?;
        let admitted = store
            .export_cutover(uuid)
            .ok()
            .map(|(seal, _)| seal.identity());
        let (capture, bytes) = store.prepare_cutover(uuid, &first.me.node)?;
        (capture, bytes, admitted)
    };
    let mut frames = vec![Frozen {
        node: None,
        capture: own.clone(),
        bytes,
        admitted,
    }];
    for node in first
        .nodes
        .values()
        .filter(|node| !node.no_replica && node.closed.is_none())
    {
        let frozen = (|| -> Result<Frozen<'_>> {
            let answer = ask(net, first, node, project, uuid, Request::Prepare, deadline)?;
            let capture: Capture = serde_json::from_value(answer["capture"].clone())?;
            ensure!(
                capture.node == node.node
                    && Some(&capture.store) == node.store.as_ref()
                    && ids::is_uuid(&capture.token)
                    && canonical_version(&capture.version).as_ref() == Some(&capture.version),
                "the capture has another native identity or clock"
            );
            let encoded = answer["bytes"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("the capture has no bytes"))?;
            ensure!(
                encoded.len() <= zerus_replica::MAX_TASK_DOCUMENT.div_ceil(3) * 4,
                "the capture exceeds its byte bound"
            );
            let bytes = STANDARD.decode(encoded)?;
            let admitted = answer["admitted_seal"].as_str().map(str::to_owned);
            if bytes.is_empty() {
                ensure!(
                    capture.version.is_empty() && admitted.is_none(),
                    "an empty capture claims an admitted history"
                );
            } else {
                let doc = TaskDoc::from_bytes(&bytes)?;
                doc.validate_cutover_history()?;
                ensure!(
                    doc.uuid() == uuid
                        && doc.view()?.project == project
                        && doc.applied_version().as_ref() == Some(&capture.version),
                    "the capture does not match its frozen task history"
                );
                if let Some(expected) = &admitted {
                    ensure!(
                        doc.cutover_seal()?
                            .as_ref()
                            .map(CutoverSeal::identity)
                            .as_ref()
                            == Some(expected),
                        "the capture changed its admitted root"
                    );
                }
            }
            Ok(Frozen {
                node: Some(node),
                capture,
                bytes,
                admitted,
            })
        })();
        match frozen {
            Ok(frame) => frames.push(frame),
            Err(_) => {
                absent.insert(
                    node.node.clone(),
                    NotCutOver {
                        node: node.node.clone(),
                        store: node.store.clone(),
                        reason: "unverified".into(),
                    },
                );
            }
        }
    }

    let selected = frames
        .iter()
        .position(|frame| frame.admitted.is_some())
        .unwrap_or(0);
    if let Some(expected) = &frames[selected].admitted {
        ensure!(
            frames
                .iter()
                .all(|frame| frame.admitted.as_ref().is_none_or(|seal| seal == expected)),
            "conflicting admitted seals hold this task; no root is selected by LWW"
        );
    }
    let mut candidate = TaskDoc::from_bytes(&frames[selected].bytes)?;
    candidate.validate_cutover_history()?;
    ensure!(
        candidate.uuid() == uuid && candidate.view()?.project == project,
        "the capture has another task identity"
    );
    if let Some(expected) = &frames[selected].admitted {
        ensure!(
            candidate
                .cutover_seal()?
                .as_ref()
                .map(CutoverSeal::identity)
                .as_ref()
                == Some(expected),
            "the source changed its admitted seal"
        );
    }
    let source_node = frames[selected].capture.node.clone();
    let source_store = frames[selected].capture.store.clone();
    let mut included = BTreeMap::new();
    for (index, frame) in frames.iter().enumerate() {
        if index != selected && !frame.bytes.is_empty() {
            let result = candidate.import(&frame.bytes);
            if let Err(error) = result {
                if frame.node.is_none() {
                    return Err(error.into());
                }
                absent.insert(
                    frame.capture.node.clone(),
                    NotCutOver {
                        node: frame.capture.node.clone(),
                        store: Some(frame.capture.store.clone()),
                        reason: "stale_copy".into(),
                    },
                );
                continue;
            }
        }
        included.insert(frame.capture.node.clone(), frame.capture.clone());
    }
    let seal = if let Some(seal) = candidate.cutover_seal()? {
        seal
    } else {
        candidate.seal_cutover(included.clone(), absent.clone())?
    };
    let sealed = candidate.snapshot()?;
    {
        let store = open(config, project, deadline)?;
        store.retain_cutover_proposal(&sealed, &own)?;
    }
    for frame in frames
        .iter()
        .filter(|frame| frame.node.is_some() && included.contains_key(&frame.capture.node))
    {
        let node = frame.node.expect("filtered");
        let answer = ask(
            net,
            first,
            node,
            project,
            uuid,
            Request::Adopt {
                capture: frame.capture.clone(),
                bytes: STANDARD.encode(&sealed),
                seal: seal.identity(),
                source_node: if frame.admitted.as_ref() == Some(&seal.identity()) {
                    frame.capture.node.clone()
                } else {
                    source_node.clone()
                },
                source_store: if frame.admitted.as_ref() == Some(&seal.identity()) {
                    frame.capture.store.clone()
                } else {
                    source_store.clone()
                },
                catch_up: frame.admitted.as_ref() != Some(&seal.identity())
                    && frames[selected].admitted.is_some(),
            },
            deadline,
        );
        if !answer.is_ok_and(|answer| {
            answer["kind"] == "adopted"
                && answer["seal"] == seal.identity()
                && answer["version"] == json!(candidate.applied_version())
        }) {
            absent.insert(
                node.node.clone(),
                NotCutOver {
                    node: node.node.clone(),
                    store: node
                        .store
                        .clone()
                        .or_else(|| node.not_cut_over_store.clone()),
                    reason: "unverified".into(),
                },
            );
        }
    }
    let store = open(config, project, deadline)?;
    if frames[0].admitted.as_ref() != Some(&seal.identity()) && frames[selected].admitted.is_some()
    {
        store.catch_up_cutover(&sealed, &own, &source_node, &source_store, &seal.identity())?;
    } else {
        store.adopt_cutover(&sealed, Some(&own))?;
    }
    Ok(absent)
}
