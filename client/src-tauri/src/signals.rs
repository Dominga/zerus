//! SIGTERM, SIGINT and SIGHUP quit like the menu's Quit
//! (`tray/src/TerminationSignals.h`): `launchctl bootout` and `unload` stop
//! the app with SIGTERM, and `RunEvent::Exit` then runs what must happen
//! before the process ends. The handler only writes the signal number to a
//! self-pipe; a thread calls `on_quit`. Each handler resets to the default
//! action when it fires (SA_RESETHAND), so a second signal still ends an app
//! that cannot finish quitting. Signals ignored at start stay ignored.
use std::io;
use std::sync::atomic::{AtomicI32, Ordering};

pub const SIGNALS: [libc::c_int; 3] = [libc::SIGTERM, libc::SIGINT, libc::SIGHUP];

static WRITE_END: AtomicI32 = AtomicI32::new(-1);

extern "C" fn on_signal(signal: libc::c_int) {
    let fd = WRITE_END.load(Ordering::Relaxed);
    if fd >= 0 {
        let byte = signal as u8;
        // SAFETY: write(2) is async-signal-safe; the byte outlives the call.
        unsafe { libc::write(fd, (&byte as *const u8).cast(), 1) };
    }
}

/// Installs the handlers for [`SIGNALS`]; returns the signals now handled.
pub fn install(on_quit: impl FnOnce(i32) + Send + 'static) -> io::Result<Vec<i32>> {
    install_for(&SIGNALS, on_quit)
}

fn install_for(
    signals: &[libc::c_int],
    on_quit: impl FnOnce(i32) + Send + 'static,
) -> io::Result<Vec<i32>> {
    let mut fds = [0; 2];
    // SAFETY: fds has room for both ends.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    for fd in fds {
        // SAFETY: fd is a descriptor this call just created.
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    WRITE_END.store(fds[1], Ordering::Relaxed);
    let mut handled = Vec::new();
    for &signal in signals {
        // SAFETY: plain sigaction(2) calls with zeroed, then filled structs.
        unsafe {
            let mut current: libc::sigaction = std::mem::zeroed();
            if libc::sigaction(signal, std::ptr::null(), &mut current) != 0
                || current.sa_sigaction == libc::SIG_IGN
            {
                continue;
            }
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_signal as *const () as libc::sighandler_t;
            action.sa_flags = libc::SA_RESETHAND | libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            if libc::sigaction(signal, &action, std::ptr::null_mut()) == 0 {
                handled.push(signal);
            }
        }
    }
    let read = fds[0];
    std::thread::Builder::new()
        .name("zerus-client-signals".into())
        .spawn(move || {
            let mut byte = 0u8;
            loop {
                // SAFETY: reads one byte into a local.
                let count = unsafe { libc::read(read, (&mut byte as *mut u8).cast(), 1) };
                if count == 1 {
                    on_quit(i32::from(byte));
                    return;
                }
                if count < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return;
            }
        })?;
    Ok(handled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// `install_for` points the one write end at its own pipe, so the tests
    /// that install handlers take turns.
    static INSTALLS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn turn() -> std::sync::MutexGuard<'static, ()> {
        INSTALLS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The signal's current action.
    fn action(signal: libc::c_int) -> libc::sigaction {
        // SAFETY: reads the action into a zeroed struct.
        unsafe {
            let mut current: libc::sigaction = std::mem::zeroed();
            assert_eq!(libc::sigaction(signal, std::ptr::null(), &mut current), 0);
            current
        }
    }

    /// Gives a signal one action for a test and puts the previous one back.
    struct Saved(libc::c_int, libc::sigaction);

    impl Saved {
        fn set(signal: libc::c_int, handler: libc::sighandler_t) -> Self {
            // SAFETY: plain sigaction(2) calls with zeroed, then filled structs.
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = handler;
                libc::sigemptyset(&mut action.sa_mask);
                let mut previous: libc::sigaction = std::mem::zeroed();
                assert_eq!(libc::sigaction(signal, &action, &mut previous), 0);
                Self(signal, previous)
            }
        }
    }

    impl Drop for Saved {
        fn drop(&mut self) {
            // SAFETY: restores the action `set` replaced.
            unsafe { libc::sigaction(self.0, &self.1, std::ptr::null_mut()) };
        }
    }

    /// The action `install_for` gives a signal it takes.
    fn ours() -> libc::sighandler_t {
        on_signal as *const () as libc::sighandler_t
    }

    /// Another program's handler, already set when the client starts.
    extern "C" fn theirs(_: libc::c_int) {}

    #[test]
    fn a_signal_reaches_the_quit_handler() {
        let _turn = turn();
        // SIGUSR1 at its default action: a handled SIGINT would make the test
        // process ignore Ctrl-C, and nohup leaves SIGHUP ignored.
        let _saved = Saved::set(libc::SIGUSR1, libc::SIG_DFL);
        let (sender, receiver) = std::sync::mpsc::channel();
        let handled = install_for(&[libc::SIGUSR1], move |signal| {
            let _ = sender.send(signal);
        })
        .unwrap();
        assert_eq!(handled, [libc::SIGUSR1]);
        assert_eq!(action(libc::SIGUSR1).sa_sigaction, ours());
        // SAFETY: raises SIGUSR1 in this process; the handler above takes it.
        unsafe { libc::raise(libc::SIGUSR1) };
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(5)).unwrap(),
            libc::SIGUSR1
        );
        assert_eq!(
            action(libc::SIGUSR1).sa_sigaction,
            libc::SIG_DFL,
            "a second signal ends an app that cannot finish quitting"
        );
    }

    #[test]
    fn a_handler_set_at_start_is_taken_over() {
        let _turn = turn();
        let _saved = Saved::set(libc::SIGUSR1, theirs as *const () as libc::sighandler_t);
        let handled = install_for(&[libc::SIGUSR1], |_| {}).unwrap();
        assert_eq!(handled, [libc::SIGUSR1]);
        assert_eq!(action(libc::SIGUSR1).sa_sigaction, ours());
    }

    #[test]
    fn a_signal_ignored_at_start_stays_ignored() {
        let _turn = turn();
        // As nohup leaves SIGHUP; SIGUSR2 is one no other test uses.
        let _saved = Saved::set(libc::SIGUSR2, libc::SIG_IGN);
        let handled = install_for(&[libc::SIGUSR2], |_| {}).unwrap();
        assert!(handled.is_empty(), "an ignored signal is not taken over");
        assert_eq!(
            action(libc::SIGUSR2).sa_sigaction,
            libc::SIG_IGN,
            "it stays ignored"
        );
    }
}
