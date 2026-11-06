//! Current original ownership, never consent or an executing service lease.
//! The native helper independently consumes its typed original confirmation.
use super::*;
use std::time::Instant;
use zerus_client_core::worktree_programs::stop::Source;

enum Evidence {
    Service { grant: Grant, epoch: String },
    Native(Status),
    Absent,
}
pub(in crate::desktop) struct Current {
    location: Location,
    source: Source,
    origin: Origin,
    database: Option<super::super::revoke::hold::Verification>,
    evidence: Evidence,
}
fn live(until: Instant) -> Result<()> {
    ensure!(
        Instant::now() < until,
        "original program-stop ownership expired"
    );
    Ok(())
}
fn bound(source: &Source, current: &Row, origin: &Origin) -> Result<()> {
    ensure!(
        current.status.identity_current && current.origin.as_ref() == Some(origin),
        "original program-stop frontend changed"
    );
    match source {
        Source::Service { epoch: _ } => ensure!(
            current.status.state == State::Service && !current.status.rearm_required,
            "original program-stop Service mode changed"
        ),
        Source::Native {
            request_id,
            journal,
            revision,
        } => ensure!(
            current.status.state == State::Native
                && current.status.request_id.as_ref() == Some(request_id)
                && current.status.journal == *journal
                && current.status.revision == *revision
                && current.status.owner_epoch.is_none(),
            "original program-stop Native origin changed"
        ),
    }
    Ok(())
}
fn service(location: &Location, db: &Connection, epoch: &str) -> Result<()> {
    let original: (String, String) = db.query_row(
        "SELECT substr(configuration,1,65),substr(epoch,1,37) FROM service WHERE id=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    ensure!(
        original == (location.configuration.clone(), epoch.into()),
        "original program-stop service changed"
    );
    // A historical Service row is not a live runtime. Only actual contention
    // on its existing owner lock counts; missing files and other errors refuse.
    match location.existing_lock() {
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(platform::lock_contended) => {}
        _ => anyhow::bail!("original program-stop service is unavailable"),
    }
    Ok(())
}
fn absent(location: &Location) -> Result<()> {
    // The existing read excludes an unreachable running owner and checks all
    // absent-state companions. It never initializes a DB or manufactures Ready.
    let actual: Status = serde_json::from_value(offline_read(location, None)?)?;
    ensure!(
        actual == super::absent(&location.configuration),
        "original absent Native scope changed"
    );
    Ok(())
}
impl Current {
    /// The origin comes only from the SAME canonical native proof, not input.
    pub(in crate::desktop) fn capture(
        location: &Location,
        source: &Source,
        origin: Origin,
        until: Instant,
    ) -> Result<Self> {
        live(until)?;
        ensure!(
            source.valid() && origin.current(),
            "original program-stop origin unavailable"
        );
        location.check_existing()?;
        super::super::store::database_files(location)?;
        let missing = !location.root.join("state.sqlite3").try_exists()?;
        let (database, evidence) = if missing {
            ensure!(
                matches!(
                    source,
                    Source::Native {
                        journal: None,
                        revision: 0,
                        ..
                    }
                ),
                "original ownership database missing"
            );
            absent(location)?;
            (None, Evidence::Absent)
        } else {
            let (db, verification) = super::super::revoke::hold::database(location)?;
            let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            ensure!(
                (15..=super::super::store::SCHEMA).contains(&version),
                "unsupported program-stop ownership schema"
            );
            let configuration: String = db.query_row(
                "SELECT substr(configuration,1,65) FROM service WHERE id=1",
                [],
                |r| r.get(0),
            )?;
            ensure!(
                configuration == location.configuration,
                "program-stop configuration changed"
            );
            let current = row(&db, &configuration)?;
            bound(source, &current, &origin)?;
            let evidence = match source {
                Source::Service { epoch } => {
                    service(location, &db, epoch)?;
                    let grant = grant_from_row(&configuration, current)?
                        .context("original Service Grant missing")?;
                    grant.revalidate_image()?;
                    Evidence::Service {
                        grant,
                        epoch: epoch.clone(),
                    }
                }
                Source::Native { .. } => Evidence::Native(current.status),
            };
            (Some(verification), evidence)
        };
        let value = Self {
            location: Location {
                root: location.root.clone(),
                configuration: location.configuration.clone(),
            },
            source: source.clone(),
            origin,
            database,
            evidence,
        };
        value.check(until)?;
        Ok(value)
    }
    /// Fresh data check on the SAME original database/Grant or absent scope.
    /// This does not authenticate again, renew proof time or mint consent.
    pub(in crate::desktop) fn check(&self, until: Instant) -> Result<()> {
        live(until)?;
        ensure!(
            self.origin.current(),
            "original program-stop frontend image changed"
        );
        match &self.evidence {
            Evidence::Absent => absent(&self.location)?,
            Evidence::Service { .. } | Evidence::Native(_) => {
                let (db, verification) = super::super::revoke::hold::database(&self.location)?;
                let held = self
                    .database
                    .as_ref()
                    .context("original program-stop database missing")?
                    .identity()?;
                ensure!(
                    held.1 != 0 && held == verification.identity()?,
                    "original program-stop database replaced"
                );
                let tx = db.unchecked_transaction()?;
                let configuration: String = tx.query_row(
                    "SELECT substr(configuration,1,65) FROM service WHERE id=1",
                    [],
                    |r| r.get(0),
                )?;
                ensure!(
                    configuration == self.location.configuration,
                    "original program-stop configuration changed"
                );
                let current = row(&tx, &self.location.configuration)?;
                bound(&self.source, &current, &self.origin)?;
                if let Evidence::Native(original) = &self.evidence {
                    ensure!(&current.status == original, "original Native state changed");
                }
                if let Evidence::Service { grant, epoch } = &self.evidence {
                    service(&self.location, &tx, epoch)?;
                    ensure!(
                        grant.matches(&current),
                        "original program-stop Grant changed"
                    );
                    grant.revalidate_image()?;
                }
                tx.commit()?;
                crate::sqlite_file::bind_private(
                    &db,
                    &self.location.root.join("state.sqlite3"),
                    crate::sqlite_file::Identity {
                        device: held.0,
                        inode: held.1,
                    },
                )?;
            }
        }
        live(until)
    }
}
