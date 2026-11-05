//! Refresh spends the unused Update allowance once, inside the unchanged
//! outer bound. Deadlines are local timing limits, never native authority.
use super::*;
use std::time::{Duration, Instant};

pub(in crate::desktop) const PHASES: [Budget; 6] = [
    Budget::Preflight,
    Budget::Hold,
    Budget::Stop,
    Budget::Start,
    Budget::Readiness,
    Budget::Release,
];
pub(in crate::desktop) fn duration(phase: Budget) -> Result<Duration> {
    Ok(match phase {
        Budget::Stop => Budget::Stop.duration() + Budget::Update.duration(),
        Budget::Preflight | Budget::Hold | Budget::Start | Budget::Readiness | Budget::Release => {
            phase.duration()
        }
        Budget::Rollback | Budget::Update => {
            bail!("Refresh cannot spend a Rollback or Update phase.")
        }
    })
}

pub(in crate::desktop) struct Deadlines {
    overall: Instant,
    until: Instant,
    next: usize,
}
impl Deadlines {
    pub(in crate::desktop) fn new(now: Instant, overall: Instant) -> Result<Self> {
        ensure!(
            overall
                .checked_duration_since(now)
                .is_some_and(|remaining| remaining > Duration::ZERO
                    && remaining <= Budget::exchange_timeout()),
            "Original refresh overall deadline expired or exceeds the runner bound."
        );
        Ok(Self {
            overall,
            until: (now + duration(Budget::Preflight)?).min(overall),
            next: 1,
        })
    }
    pub(in crate::desktop) fn until(&self) -> Instant {
        self.until
    }
    pub(in crate::desktop) fn check(&self, now: Instant) -> Result<()> {
        ensure!(
            now < self.until && now < self.overall,
            "Original refresh deadline expired; retain its original UUID."
        );
        Ok(())
    }
    pub(in crate::desktop) fn enter(&mut self, phase: Budget, now: Instant) -> Result<Instant> {
        self.check(now)?;
        ensure!(
            self.next < PHASES.len()
                && std::mem::discriminant(&PHASES[self.next]) == std::mem::discriminant(&phase),
            "Original refresh phase cannot be repeated, renewed or skipped."
        );
        let until = (now + duration(phase)?).min(self.overall);
        self.until = until;
        self.next += 1;
        Ok(until)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_one_allocation_keeps_ordinary_and_outer_budgets_and_forbids_spending_update_again() {
        assert_eq!(Budget::Stop.duration(), Duration::from_secs(2));
        assert_eq!(duration(Budget::Stop).unwrap(), Duration::from_secs(4));
        let total = PHASES
            .into_iter()
            .map(|p| duration(p).unwrap())
            .sum::<Duration>();
        assert_eq!(total, Duration::from_secs(21));
        assert_eq!(Budget::exchange_timeout(), Duration::from_secs(30));
        assert!(total + Duration::from_secs(3) <= Budget::exchange_timeout());
        assert!(duration(Budget::Update).is_err() && duration(Budget::Rollback).is_err());
        let now = Instant::now();
        let mut clock = Deadlines::new(now, now + Budget::exchange_timeout()).unwrap();
        clock.enter(Budget::Hold, now).unwrap();
        let stop = clock.enter(Budget::Stop, now).unwrap();
        assert_eq!(stop, now + Duration::from_secs(4));
        for elapsed in [1, 2, 3] {
            let at = now + Duration::from_secs(elapsed);
            clock.check(at).unwrap();
            assert!(clock.enter(Budget::Stop, at).is_err());
            assert!(clock.enter(Budget::Update, at).is_err());
            assert!(clock.enter(Budget::Rollback, at).is_err());
            assert_eq!(clock.until(), stop, "a refused phase changed the deadline");
        }
        assert!(clock.check(stop).is_err());
        assert!(clock.enter(Budget::Start, stop).is_err());
        assert_eq!(clock.until(), stop, "expiry renewed the original deadline");
    }
    #[test]
    fn every_phase_is_clipped_to_the_same_declared_overall_deadline() {
        let now = Instant::now();
        assert!(Deadlines::new(now, now).is_err());
        assert!(Deadlines::new(now, now + Duration::from_secs(31)).is_err());
        let overall = now + Duration::from_secs(6);
        let mut clock = Deadlines::new(now, overall).unwrap();
        clock
            .enter(Budget::Hold, now + Duration::from_secs(1))
            .unwrap();
        assert_eq!(
            clock
                .enter(Budget::Stop, now + Duration::from_secs(3))
                .unwrap(),
            overall
        );
        assert!(clock.check(overall).is_err());
        assert!(clock.enter(Budget::Start, overall).is_err());
        assert_eq!(clock.until(), overall);
    }
}
