//! The main-thread watchdog, as the Qt GUI has it
//! (`tray/src/MainThreadWatchdog.{h,cpp}`, `tray/src/main.cpp:163-166` and
//! `:385-423`). Qt added it after its menu-bar item hung inside AppKit for six
//! days on macOS 26 while the process kept "running" (`tray/src/QuietTrayIcon.h`).
//!
//! A timer on the main thread records a beat every quarter of [`HANG_MS`]; a
//! thread named `hgs-watchdog` checks every half. Once the main thread has not
//! beaten for longer than [`HANG_MS`], it prints why and aborts, at most once:
//! on macOS launchd (`KeepAlive Crashed`, `tray/packaging/com.hgdev.hgs-tray.plist`)
//! starts the client again, and the crash report shows where the main thread
//! stood; a plain exit would not be restarted. On Linux no unit supervises the
//! client (`hgs-tray.service` runs the Qt GUI), so an abort only ends that run.
//!
//! The clock stops while the machine sleeps ([`now_ms`]). Around sleep macOS
//! leaves the main thread unscheduled for longer than the threshold while the
//! rest of the process runs: during the transition into sleep and in a dark
//! wake. [`Pulse::will_sleep`] and [`Pulse::powered_on`] (IOKit,
//! `platform/macos_power.rs`) hold the check from the sleep notice until the
//! main thread has beaten after a wake, for [`WAKE_GRACE_MS`] of uptime at
//! most, so a main thread that never runs again after a wake still counts.
//!
//! Not a hang: an open menu or a modal panel, since the macOS timer runs in
//! the common run-loop modes as Qt's does. A main thread starved of CPU trips
//! it too, whatever its stack shows.
use std::io;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

/// `kMainThreadHangMs`: half a minute, far above any honest pause of the
/// event loop and far below the week a hung menu-bar item once stayed.
pub const HANG_MS: i64 = 30_000;
/// `MainThreadWatchdog`'s default `wakeGraceMs`: ten minutes of uptime.
pub const WAKE_GRACE_MS: i64 = 10 * 60 * 1000;
/// The checking thread's name, which crash reports show.
pub const THREAD: &str = "hgs-watchdog";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub hang_ms: i64,
    pub wake_grace_ms: i64,
}

impl Limits {
    /// Qt's values.
    pub const QT: Limits = Limits {
        hang_ms: HANG_MS,
        wake_grace_ms: WAKE_GRACE_MS,
    };

    /// How often the main thread beats: a quarter of the threshold.
    pub fn beat_every(self) -> Duration {
        Duration::from_millis((self.hang_ms / 4).max(1).unsigned_abs())
    }

    /// How often the watchdog checks: half of the threshold.
    pub fn check_every(self) -> Duration {
        Duration::from_millis((self.hang_ms / 2).max(1).unsigned_abs())
    }
}

/// What the watchdog knows, written from any thread.
#[derive(Debug)]
pub struct Pulse {
    last_beat: AtomicI64,
    power_event: AtomicI64,
    sleeping: AtomicBool,
    awaiting_beat: AtomicBool,
}

impl Pulse {
    pub fn new(now: i64) -> Self {
        Self {
            last_beat: AtomicI64::new(now),
            power_event: AtomicI64::new(0),
            sleeping: AtomicBool::new(false),
            awaiting_beat: AtomicBool::new(false),
        }
    }

    /// The main thread ran: the heartbeat timer fired.
    pub fn beat(&self, now: i64) {
        self.last_beat.store(now, Ordering::SeqCst);
        self.awaiting_beat.store(false, Ordering::SeqCst);
    }

    /// The system is about to sleep: hold the check until it powers on.
    /// Only macOS reports it (`platform/macos_power.rs`).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn will_sleep(&self, now: i64) {
        self.power_event.store(now, Ordering::SeqCst);
        self.sleeping.store(true, Ordering::SeqCst);
    }

    /// The system powered on, also for a dark wake: hold the check until the
    /// main thread beats.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn powered_on(&self, now: i64) {
        self.power_event.store(now, Ordering::SeqCst);
        // Before `sleeping` clears, so that the check never sees neither hold.
        self.awaiting_beat.store(true, Ordering::SeqCst);
        self.sleeping.store(false, Ordering::SeqCst);
    }

    /// How long the main thread has been silent.
    pub fn silent_ms(&self, now: i64) -> i64 {
        now - self.last_beat.load(Ordering::SeqCst)
    }

    /// The check (`MainThreadWatchdog.cpp:62-71`): silent past the threshold
    /// and not held by a sleep or a wake within the grace.
    pub fn hung(&self, now: i64, limits: Limits) -> bool {
        if self.silent_ms(now) <= limits.hang_ms {
            return false;
        }
        let held =
            self.sleeping.load(Ordering::SeqCst) || self.awaiting_beat.load(Ordering::SeqCst);
        !(held && now - self.power_event.load(Ordering::SeqCst) <= limits.wake_grace_ms)
    }
}

/// The checking thread. Stopping or dropping it ends the thread at once, not
/// at its next check.
pub struct Watchdog {
    pulse: Arc<Pulse>,
    stopping: Arc<(Mutex<bool>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl Watchdog {
    /// Starts checking; `on_hang` runs at most once, on the watchdog thread,
    /// with how long the main thread has been silent.
    pub fn start(
        limits: Limits,
        clock: fn() -> i64,
        on_hang: impl FnOnce(i64) + Send + 'static,
    ) -> io::Result<Self> {
        let pulse = Arc::new(Pulse::new(clock()));
        let stopping = Arc::new((Mutex::new(false), Condvar::new()));
        let (watched, signal) = (Arc::clone(&pulse), Arc::clone(&stopping));
        let (ready, holding) = mpsc::sync_channel(0);
        let thread = std::thread::Builder::new()
            .name(THREAD.into())
            .spawn(move || {
                let (lock, wake) = &*signal;
                let mut stopped = lock.lock().unwrap_or_else(PoisonError::into_inner);
                // Held from here into the wait: a stop always finds the thread waiting.
                let _ = ready.send(());
                loop {
                    // Returns early only when stopped; a timeout is the next check.
                    stopped = wake
                        .wait_timeout_while(stopped, limits.check_every(), |stopped| !*stopped)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                    if *stopped {
                        return;
                    }
                    let now = clock();
                    if watched.hung(now, limits) {
                        drop(stopped);
                        on_hang(watched.silent_ms(now));
                        return;
                    }
                }
            })?;
        let _ = holding.recv();
        Ok(Self {
            pulse,
            stopping,
            thread: Some(thread),
        })
    }

    /// What the heartbeat and the power notices write to.
    pub fn pulse(&self) -> Arc<Pulse> {
        Arc::clone(&self.pulse)
    }

    /// Stops checking. Quitting is not a hang: Qt destroys its watchdog before
    /// the tray is torn down (`main.cpp:385-389`), and the client stops it
    /// first thing on `RunEvent::Exit`, so a slow flush never ends in an abort
    /// that launchd would answer with a restart.
    pub fn stop(&self) {
        *self
            .stopping
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = true;
        self.stopping.1.notify_all();
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Milliseconds that stop while the machine sleeps (`MainThreadWatchdog::nowMs`):
/// uptime (`CLOCK_UPTIME_RAW`) on macOS, `CLOCK_MONOTONIC` on Linux.
pub fn now_ms() -> i64 {
    #[cfg(target_os = "macos")]
    let clock = libc::CLOCK_UPTIME_RAW;
    #[cfg(not(target_os = "macos"))]
    let clock = libc::CLOCK_MONOTONIC;
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime writes only the struct it is given.
    unsafe { libc::clock_gettime(clock, &mut time) };
    // Both fields are 64-bit on the client's targets.
    time.tv_sec * 1000 + time.tv_nsec / 1_000_000
}

/// Qt's exit path (`main.cpp:410-417`): say why, then abort. The line is
/// built on the stack and written with one `write(2)`: the main thread may
/// hang holding Rust's stderr lock or the allocator's.
pub fn abort_on_hang(silent_ms: i64) -> ! {
    let mut line = [0; 256];
    let length = hang_line(&mut line, silent_ms);
    // SAFETY: writes initialized bytes of a stack buffer to stderr.
    unsafe { libc::write(libc::STDERR_FILENO, line.as_ptr().cast(), length) };
    std::process::abort()
}

/// The abort line, written into `buffer` without allocating; its length.
fn hang_line(buffer: &mut [u8; 256], silent_ms: i64) -> usize {
    let mut cursor = io::Cursor::new(&mut buffer[..]);
    // A line longer than the buffer is cut short, not lost.
    let _ = io::Write::write_fmt(
        &mut cursor,
        format_args!(
            "zerus-client: main thread unresponsive for {} s (AppKit deadlock? see tray/src/QuietTrayIcon.h); \
             aborting so the service manager restarts it\n",
            silent_ms / 1000
        ),
    );
    cursor.position() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: Limits = Limits {
        hang_ms: 300,
        wake_grace_ms: 600,
    };

    #[test]
    fn qt_values_and_periods() {
        assert_eq!(
            Limits::QT,
            Limits {
                hang_ms: 30_000,
                wake_grace_ms: 600_000
            }
        );
        assert_eq!(Limits::QT.beat_every(), Duration::from_millis(7_500));
        assert_eq!(Limits::QT.check_every(), Duration::from_millis(15_000));
    }

    #[test]
    fn a_silent_main_thread_is_hung_past_the_threshold() {
        // firesOnceWhenTheMainThreadStopsPumpingEvents
        let pulse = Pulse::new(1_000);
        assert!(
            !pulse.hung(1_300, LIMITS),
            "exactly the threshold is not a hang"
        );
        assert!(pulse.hung(1_301, LIMITS));
    }

    #[test]
    fn beats_keep_it_quiet() {
        // staysQuietWhileEventsFlow
        let pulse = Pulse::new(0);
        for now in (75..=3_000).step_by(75) {
            pulse.beat(now);
            assert!(!pulse.hung(now + 75, LIMITS));
        }
    }

    #[test]
    fn going_to_sleep_holds_until_power_on() {
        // holdsWhileTheSystemGoesToSleep: beats in between do not end the hold.
        let pulse = Pulse::new(0);
        pulse.will_sleep(0);
        pulse.beat(200);
        assert!(!pulse.hung(600, LIMITS));
        assert!(
            pulse.hung(601, LIMITS),
            "the hold lasts the wake grace at most"
        );
    }

    #[test]
    fn a_wake_holds_until_the_main_thread_beats() {
        // holdsAfterAWakeUntilTheMainThreadRuns
        let pulse = Pulse::new(0);
        pulse.will_sleep(0);
        pulse.powered_on(10);
        assert!(
            !pulse.hung(500, LIMITS),
            "silent after the wake, within the grace"
        );
        pulse.beat(500);
        assert!(!pulse.hung(800, LIMITS));
        assert!(pulse.hung(801, LIMITS), "watched as usual once it ran");
    }

    #[test]
    fn a_beat_after_a_wake_ends_the_hold_at_once() {
        // The heartbeat clears the wait for a beat (MainThreadWatchdog.cpp:46-49): once
        // the main thread ran after a wake, a later silence counts within the grace too.
        let pulse = Pulse::new(0);
        pulse.powered_on(0);
        pulse.beat(100);
        assert!(!pulse.hung(400, LIMITS));
        assert!(
            pulse.hung(401, LIMITS),
            "the main thread already ran after the wake"
        );
    }

    #[test]
    fn a_main_thread_silent_past_the_wake_grace_is_hung() {
        // firesWhenTheMainThreadStaysSilentPastTheWakeGrace
        let pulse = Pulse::new(0);
        pulse.powered_on(0);
        assert!(!pulse.hung(600, LIMITS));
        assert!(pulse.hung(601, LIMITS));
    }

    #[test]
    fn the_thread_fires_once_on_its_own_name() {
        let (sender, fired) = std::sync::mpsc::channel();
        let dog = Watchdog::start(
            Limits {
                hang_ms: 100,
                wake_grace_ms: 100,
            },
            now_ms,
            move |silent| {
                let _ = sender.send((std::thread::current().name().map(str::to_owned), silent));
            },
        )
        .unwrap();
        let (name, silent) = fired.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(name.as_deref(), Some(THREAD));
        assert!(silent > 100, "{silent}");
        assert!(
            fired.recv_timeout(Duration::from_millis(400)).is_err(),
            "at most once"
        );
        drop(dog);
    }

    #[test]
    fn a_quit_is_not_a_hang() {
        let (sender, fired) = std::sync::mpsc::channel::<i64>();
        let dog = Watchdog::start(
            Limits {
                hang_ms: 100,
                wake_grace_ms: 0,
            },
            now_ms,
            move |silent| {
                let _ = sender.send(silent);
            },
        )
        .unwrap();
        dog.stop();
        assert!(
            fired.recv_timeout(Duration::from_millis(500)).is_err(),
            "stopped before the main thread went quiet"
        );
    }

    #[test]
    fn a_dropped_watchdog_stops_at_once() {
        let (sender, fired) = std::sync::mpsc::channel::<i64>();
        let dog = Watchdog::start(
            Limits {
                hang_ms: 10_000,
                wake_grace_ms: 0,
            },
            now_ms,
            move |silent| {
                let _ = sender.send(silent);
            },
        )
        .unwrap();
        // start() returned with the thread on its way into the wait: only the
        // stop's wake-up ends it before the next check.
        let started = std::time::Instant::now();
        drop(dog);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "not at the next check, 5 s later"
        );
        assert!(fired.recv_timeout(Duration::from_millis(200)).is_err());
    }

    #[test]
    fn the_abort_line_fits_on_the_stack() {
        let mut buffer = [0; 256];
        let length = hang_line(&mut buffer, 45_500);
        assert_eq!(
            std::str::from_utf8(&buffer[..length]).unwrap(),
            "zerus-client: main thread unresponsive for 45 s (AppKit deadlock? see tray/src/QuietTrayIcon.h); \
             aborting so the service manager restarts it\n"
        );
        assert!(hang_line(&mut buffer, i64::MAX) < 256);
    }

    #[test]
    fn the_clock_moves_forward() {
        let first = now_ms();
        std::thread::sleep(Duration::from_millis(20));
        assert!(now_ms() >= first + 15);
    }
}
