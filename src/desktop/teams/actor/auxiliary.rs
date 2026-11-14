//! Persistent package-10 state. The actor owns the entry points and
//! their retry scopes; each effect remains behind its package's ports.
use super::super::timers;
use super::native::Native;
use super::*;
use std::path::PathBuf;
pub(in crate::desktop::teams) struct State {
    state: PathBuf,
    engine: timers::Timers,
    ledger: Option<timers::Ledger>,
}
impl State {
    pub fn new(state: &Path, epoch: &str) -> Self {
        Self {
            state: state.into(),
            engine: timers::Timers::new(epoch),
            ledger: None,
        }
    }
    fn prepare(&mut self) -> Result<()> {
        if self.ledger.is_none() {
            self.ledger = Some(timers::Ledger::open(&self.state)?);
        }
        Ok(())
    }
    pub fn team(&mut self, n: &Native, store: &mut Store, team: &Team, now: i64) -> Result<()> {
        self.prepare()?;
        let mut sessions = SessionRows(n);
        let mut effects = timers::Native {
            config: &n.config,
            machine: &n.machine,
            stop: &n.stop,
        };
        self.engine.team(
            store,
            self.ledger.as_ref().expect("opened timer ledger"),
            &mut sessions,
            &mut effects,
            &team.id,
            now,
        )
    }
    pub fn answers(&mut self, n: &Native, store: &mut Store, team: &Team, now: i64) -> Result<()> {
        self.prepare()?;
        let mut sessions = SessionRows(n);
        let mut effects = timers::Native {
            config: &n.config,
            machine: &n.machine,
            stop: &n.stop,
        };
        self.engine.answers(
            store,
            self.ledger.as_ref().expect("opened timer ledger"),
            &mut sessions,
            &mut effects,
            &team.id,
            now,
        )
    }
    pub fn terminal(&mut self, n: &Native, store: &mut Store, now: i64) -> Result<()> {
        self.prepare()?;
        let mut effects = timers::Native {
            config: &n.config,
            machine: &n.machine,
            stop: &n.stop,
        };
        self.engine.terminal(
            store,
            self.ledger.as_mut().expect("opened timer ledger"),
            &mut effects,
            now,
        )
    }
    /// The teams whose key sweep reached its end since the last call.
    pub fn finished_sweeps(&mut self) -> Vec<timers::Finished> {
        self.engine.finished_sweeps()
    }
}
struct SessionRows<'a>(&'a Native);
impl timers::Sessions for SessionRows<'_> {
    fn presence(&mut self, team: &Team, member: &Member) -> Option<timers::Presence> {
        presence(self.0, team, member)
    }
}
pub(super) fn presence(n: &Native, team: &Team, member: &Member) -> Option<timers::Presence> {
    if member.session.as_ref()?.machine != n.machine {
        return None;
    }
    let row = super::availability::record(&n.config, &n.epoch, &team.id, member, false).ok()??;
    let named = super::fleet::Identity::of(&row);
    if named.name.is_empty()
        || named.name.len() > 256
        || named.run.is_empty()
        || named.conversation.is_empty()
    {
        return None;
    }
    // The same reader as availability: unknown stays unknown, so a row with
    // an unknown count gives no presence.
    let facts = super::fleet::Facts::of(&row);
    let seen = facts.seen()?;
    if !facts.free() && !seen.working && !seen.background && !seen.pending {
        return None;
    }
    Some(timers::Presence {
        name: named.name.into(),
        run: named.run.into(),
        conversation: named.conversation.into(),
        live: seen.live,
        working: seen.working,
        background: seen.background,
        pending: seen.pending,
    })
}
impl Runtime {
    pub(super) fn team_entry(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        now: i64,
    ) -> Result<()> {
        let stamp = if team.status == TeamStatus::Paused {
            "paused"
        } else {
            "running"
        };
        if !self.due(store, team, "timers:team", stamp, now)? {
            return Ok(());
        }
        match ports.team_timers(&mut self.auxiliary, store, team, now) {
            Ok(()) => self.recovered(store, team, "timers:team", now),
            Err(error) => self.failure(store, team, "timers:team", stamp, &error, now),
        }
    }
    pub(super) fn answers_entry(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        now: i64,
    ) -> Result<()> {
        let stamp = if team.status == TeamStatus::Paused {
            "paused"
        } else {
            "running"
        };
        if !self.due(store, team, "timers:answers", stamp, now)? {
            return Ok(());
        }
        match ports.answer_timers(&mut self.auxiliary, store, team, now) {
            Ok(()) => self.recovered(store, team, "timers:answers", now),
            Err(error) => self.failure(store, team, "timers:answers", stamp, &error, now),
        }
    }
    pub(super) fn terminal_entry(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        now: i64,
    ) -> Result<()> {
        if ports.stopped()
            || self
                .ledger
                .terminal_retry()?
                .is_some_and(|r| now < r.next_ms)
        {
            return Ok(());
        }
        let cancelled = self.terminal_cancels(store, ports, now);
        let timers = ports.terminal_timers(&mut self.auxiliary, store, now);
        let swept = self.sweep_items(now);
        match cancelled.and(timers).and(swept) {
            Ok(()) => self.ledger.terminal_succeeded(),
            Err(error) => {
                eprintln!("hgs: Teams terminal page will retry: {}", error.code);
                self.ledger.terminal_failed(now, &error.message)
            }
        }
    }
}
