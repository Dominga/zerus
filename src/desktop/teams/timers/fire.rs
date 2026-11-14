//! The timers. Times the store holds (a hand-out outcome, a report item, an
//! inbox item) are read from it; times only the service saw (a member idle
//! since, a lead not working since) live here, so after a restart a timer
//! may be late and is never early. Each list is a page of live rows.
use super::{ports::Presence, record, Result};
use crate::teams::{
    ops::{Operation, TimerKind},
    store::Store,
};
use std::collections::BTreeMap;
use zerus_team_plan::model::Team;

pub(super) const PAGE: u32 = 32;
const MINUTE: i64 = 60_000;

#[derive(Default)]
pub(super) struct Memory {
    /// (team, member) to (run, seen idle since).
    idle: BTreeMap<(String, String), (String, i64)>,
    /// Team to (lead run, seen not working since).
    quiet: BTreeMap<String, (String, i64)>,
    pages: BTreeMap<String, Pages>,
}
#[derive(Clone, Copy, Default)]
struct Pages {
    unreported: u32,
    unstarted: u32,
    undecided: u64,
}
impl Memory {
    pub fn forget(&mut self, team: &str) {
        self.idle.retain(|(t, _), _| t != team);
        self.quiet.remove(team);
        self.pages.remove(team);
    }
    /// Replace the team's idle counts with this pass's observations.
    pub fn members(&mut self, team: &str, seen: &[(String, Option<Presence>)], now: i64) {
        let mut next = BTreeMap::new();
        for (member, presence) in seen {
            let key = (team.to_owned(), member.clone());
            if let Some(p) = presence.as_ref().filter(|p| p.idle()) {
                let since = match self.idle.get(&key) {
                    Some((run, since)) if run == &p.run => *since,
                    _ => now,
                };
                next.insert(key, (p.run.clone(), since));
            }
        }
        self.idle.retain(|(t, _), _| t != team);
        self.idle.extend(next);
    }
    /// The time since which the lead has been seen not working, if it was.
    pub fn lead(&mut self, team: &str, seen: Option<&Presence>, now: i64) -> Option<i64> {
        let Some(p) = seen.filter(|p| p.quiet()) else {
            self.quiet.remove(team);
            return None;
        };
        let entry = self
            .quiet
            .entry(team.to_owned())
            .or_insert((p.run.clone(), now));
        if entry.0 != p.run {
            *entry = (p.run.clone(), now);
        }
        Some(entry.1)
    }
    fn idle_since(&self, team: &str, member: &str) -> Option<i64> {
        self.idle
            .get(&(team.to_owned(), member.to_owned()))
            .map(|(_, since)| *since)
    }
}

pub(super) fn step(
    store: &mut Store,
    memory: &mut Memory,
    team: &Team,
    quiet: Option<i64>,
    now: i64,
) -> Result<()> {
    let mut pages = memory.pages.get(&team.id).copied().unwrap_or_default();
    unreported(store, memory, &mut pages, team, now)?;
    unstarted(store, &mut pages, team, now)?;
    undecided(store, &mut pages, team, now)?;
    memory.pages.insert(team.id.clone(), pages);
    silent(store, team, quiet, now)
}
fn next<T>(page: &[T], key: impl Fn(&T) -> u64) -> u64 {
    match page.last() {
        Some(last) if page.len() == PAGE as usize => key(last),
        _ => 0,
    }
}
fn timer(timer: TimerKind, node: u32, generation: u32, item: u64, since_ms: i64) -> Operation {
    Operation::FireTimer {
        timer,
        node,
        generation,
        item,
        since_ms,
    }
}
fn unreported(
    store: &mut Store,
    memory: &Memory,
    pages: &mut Pages,
    team: &Team,
    now: i64,
) -> Result<()> {
    let page = store.unreported(&team.id, pages.unreported, PAGE)?;
    pages.unreported = next(&page, |a| a.node.into()) as u32;
    let wait = i64::from(team.settings.timers.report_timeout_seconds) * 1000;
    for a in page {
        let Some(idle) = memory.idle_since(&team.id, &a.member) else {
            continue;
        };
        // Idleness before the delivery never counts toward this attempt.
        let since = idle.max(a.outcome_ms);
        if now.saturating_sub(since) >= wait {
            record(
                store,
                team,
                timer(TimerKind::NoReport, a.node, a.generation, 0, since),
                now,
            )?;
        }
    }
    Ok(())
}
fn unstarted(store: &mut Store, pages: &mut Pages, team: &Team, now: i64) -> Result<()> {
    let page = store.unstarted(&team.id, pages.unstarted, PAGE)?;
    pages.unstarted = next(&page, |a| a.node.into()) as u32;
    let wait = i64::from(team.settings.timers.start_timeout_seconds) * 1000;
    for a in page {
        if now.saturating_sub(a.outcome_ms) >= wait {
            record(
                store,
                team,
                timer(TimerKind::NotStarted, a.node, a.generation, 0, 0),
                now,
            )?;
        }
    }
    Ok(())
}
fn undecided(store: &mut Store, pages: &mut Pages, team: &Team, now: i64) -> Result<()> {
    let page = store.undecided(&team.id, pages.undecided, PAGE)?;
    pages.undecided = next(&page, |w| w.item.seq);
    let t = &team.settings.timers;
    for w in page {
        let i = &w.item;
        let waited = now.saturating_sub(i.created_ms);
        if !w.lead_reminded && waited >= i64::from(t.remind_lead_minutes) * MINUTE {
            record(
                store,
                team,
                timer(TimerKind::RemindLead, i.node, i.generation, i.seq, 0),
                now,
            )?;
        }
        if !w.person_reminded && waited >= i64::from(t.remind_person_minutes) * MINUTE {
            record(
                store,
                team,
                timer(TimerKind::RemindPerson, i.node, i.generation, i.seq, 0),
                now,
            )?;
        }
    }
    Ok(())
}
fn silent(store: &mut Store, team: &Team, quiet: Option<i64>, now: i64) -> Result<()> {
    let (Some(quiet), Some(view)) = (quiet, store.inbox_view(&team.id)?) else {
        return Ok(());
    };
    let Some(oldest) = view.oldest else {
        return Ok(());
    };
    let wait = i64::from(team.settings.timers.lead_stall_minutes) * MINUTE;
    let due = now.saturating_sub(oldest.created_ms) >= wait && now.saturating_sub(quiet) >= wait;
    if due
        && !store.timer_fired(
            &team.id,
            "silent_lead",
            oldest.node,
            oldest.generation,
            oldest.seq,
        )?
    {
        let op = timer(
            TimerKind::SilentLead,
            oldest.node,
            oldest.generation,
            oldest.seq,
            quiet,
        );
        record(store, team, op, now)?;
    }
    Ok(())
}
