//! The attention notifier's delivery, apart from the system: Qt's
//! `MacAttentionNotifier` (`tray/src/MacAttentionNotifier.mm:51-114`) around
//! the core's [`Delivery`] state. One authorization check at a time; a token
//! withdrawn meanwhile is never posted, and one withdrawn while its request
//! was being added goes once the add returns. The system's center is a
//! [`Center`]: `platform/macos_notify.rs` on macOS, a fake in the tests.
//!
//! Qt keeps the state on its GUI thread and queues every completion there;
//! here a [`Hop`] does the same (the main thread in the app).
// The notifier exists only on macOS in this phase; the tests run everywhere.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::sync::{Arc, Mutex, PoisonError};

use zerus_client_core::attention::{Delivery, DeliveryStep, Notice};

/// A completion, called once on any thread.
pub type Done<T> = Box<dyn FnOnce(T) + Send>;

/// The system's notification center (`UNUserNotificationCenter`), as the
/// notifier uses it.
pub trait Center: Send + Sync {
    /// Whether banners may post: the settings first, and a request for alerts
    /// and sound only while undecided (`MacAttentionNotifier.mm:56-75`).
    /// `done` gets the answer and the system's error text, if any.
    fn authorize(&self, done: Done<(bool, Option<String>)>);
    /// Adds one request for `token` (`MacAttentionNotifier.mm:94-113`);
    /// `done` gets the system's error text, if any.
    fn add(&self, token: &str, title: &str, body: &str, done: Done<Option<String>>);
    /// Withdraws `token`'s request, pending and delivered
    /// (`MacAttentionNotifier.mm:77-82`).
    fn remove(&self, token: &str);
}

/// Runs a completion where the state lives, as Qt queues each one on its
/// GUI thread (`Qt::QueuedConnection`).
pub type Hop = Arc<dyn Fn(Work) + Send + Sync>;

/// Work a [`Hop`] runs.
pub type Work = Box<dyn FnOnce() + Send>;

/// A failed delivery's token and reason (`AttentionNotifier::failed`).
pub type Failed = Box<dyn Fn(String, String) + Send + Sync>;

struct Shared {
    delivery: Mutex<Delivery>,
    center: Box<dyn Center>,
    hop: Hop,
    failed: Failed,
}

/// The notifier: clones share it.
#[derive(Clone)]
pub struct Notifier(Arc<Shared>);

impl Notifier {
    pub fn new(center: Box<dyn Center>, hop: Hop, failed: Failed) -> Self {
        Self(Arc::new(Shared {
            delivery: Mutex::new(Delivery::default()),
            center,
            hop,
            failed,
        }))
    }

    /// Withdraws, then posts (`TrayAgent::observeAttention`,
    /// `TrayAgent.cpp:558-559`).
    pub fn deliver(&self, cleared: &[String], raised: &[Notice]) {
        let steps = {
            let mut delivery = self.0.lock();
            let mut steps = Vec::new();
            for token in cleared {
                steps.extend(delivery.withdraw(token));
            }
            for notice in raised {
                steps.extend(delivery.post(&notice.token, &notice.title, &notice.body));
            }
            steps
        };
        execute(&self.0, steps);
    }
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, Delivery> {
        self.delivery.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Runs each step without holding the state's lock: a center may answer at
/// once, and its answer takes the lock again.
fn execute(shared: &Arc<Shared>, steps: Vec<DeliveryStep>) {
    for step in steps {
        match step {
            DeliveryStep::CheckAuthorization => {
                let owner = Arc::clone(shared);
                shared.center.authorize(Box::new(move |(granted, error)| {
                    later(owner, move |shared| {
                        shared.lock().authorized(granted, error)
                    });
                }));
            }
            DeliveryStep::Add { token, title, body } => {
                let owner = Arc::clone(shared);
                let added = token.clone();
                shared.center.add(
                    &token,
                    &title,
                    &body,
                    Box::new(move |error| {
                        later(owner, move |shared| shared.lock().added(&added, error));
                    }),
                );
            }
            DeliveryStep::Remove { token } => shared.center.remove(&token),
            DeliveryStep::Fail { token, detail } => (shared.failed)(token, detail),
        }
    }
}

/// A completion's next steps, decided and run where the state lives.
fn later(shared: Arc<Shared>, decide: impl FnOnce(&Shared) -> Vec<DeliveryStep> + Send + 'static) {
    let hop = Arc::clone(&shared.hop);
    hop(Box::new(move || {
        let steps = decide(&shared);
        execute(&shared, steps);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerus_client_core::attention::DENIED;

    /// What the fake center was asked, and the completions it holds back.
    #[derive(Default)]
    struct Calls {
        log: Vec<String>,
        authorizations: Vec<Done<(bool, Option<String>)>>,
        adds: Vec<(String, Done<Option<String>>)>,
    }

    #[derive(Clone, Default)]
    struct FakeCenter(Arc<Mutex<Calls>>);

    impl FakeCenter {
        fn calls(&self) -> std::sync::MutexGuard<'_, Calls> {
            self.0.lock().unwrap()
        }

        fn log(&self) -> Vec<String> {
            std::mem::take(&mut self.calls().log)
        }

        fn answer_authorization(&self, granted: bool, error: Option<&str>) {
            let done = self.calls().authorizations.remove(0);
            done((granted, error.map(str::to_owned)));
        }

        fn answer_add(&self, token: &str, error: Option<&str>) {
            let index = self
                .calls()
                .adds
                .iter()
                .position(|(added, _)| added == token)
                .expect("an add of that token waits");
            let (_, done) = self.calls().adds.remove(index);
            done(error.map(str::to_owned));
        }
    }

    impl Center for FakeCenter {
        fn authorize(&self, done: Done<(bool, Option<String>)>) {
            let mut calls = self.calls();
            calls.log.push("authorize".into());
            calls.authorizations.push(done);
        }

        fn add(&self, token: &str, title: &str, body: &str, done: Done<Option<String>>) {
            let mut calls = self.calls();
            calls.log.push(format!("add {token}: {title}: {body}"));
            calls.adds.push((token.to_owned(), done));
        }

        fn remove(&self, token: &str) {
            self.calls().log.push(format!("remove {token}"));
        }
    }

    /// The main thread: completions wait in a queue until the test runs them.
    #[derive(Clone, Default)]
    struct Queue(Arc<Mutex<Vec<Work>>>);

    impl Queue {
        fn hop(&self) -> Hop {
            let queue = self.clone();
            Arc::new(move |work| queue.0.lock().unwrap().push(work))
        }

        /// Runs what waits, including what it queues.
        fn run(&self) -> usize {
            let mut ran = 0;
            loop {
                let next = {
                    let mut queue = self.0.lock().unwrap();
                    (!queue.is_empty()).then(|| queue.remove(0))
                };
                let Some(work) = next else { return ran };
                work();
                ran += 1;
            }
        }
    }

    struct Fixture {
        center: FakeCenter,
        main: Queue,
        failed: Arc<Mutex<Vec<(String, String)>>>,
        notifier: Notifier,
    }

    impl Fixture {
        fn new() -> Self {
            let center = FakeCenter::default();
            let main = Queue::default();
            let failed = Arc::new(Mutex::new(Vec::new()));
            let sink = Arc::clone(&failed);
            let notifier = Notifier::new(
                Box::new(center.clone()),
                main.hop(),
                Box::new(move |token, detail| sink.lock().unwrap().push((token, detail))),
            );
            Self {
                center,
                main,
                failed,
                notifier,
            }
        }

        fn post(&self, token: &str) {
            self.notifier.deliver(
                &[],
                &[Notice {
                    token: token.into(),
                    title: "Zerus \u{2014} Needs attention".into(),
                    body: format!("claude/sample-project/{token} / studio\nApproval needed"),
                }],
            );
        }

        fn withdraw(&self, token: &str) {
            self.notifier.deliver(&[token.to_owned()], &[]);
        }

        fn failed(&self) -> Vec<(String, String)> {
            std::mem::take(&mut self.failed.lock().unwrap())
        }
    }

    fn added(token: &str) -> String {
        format!(
            "add {token}: Zerus \u{2014} Needs attention: claude/sample-project/{token} / studio\nApproval needed"
        )
    }

    #[test]
    fn a_banner_posts_after_one_authorization_check() {
        let f = Fixture::new();
        f.post("a");
        f.post("b");
        f.post("a");
        assert_eq!(
            f.center.log(),
            ["authorize"],
            "one check for every post meanwhile"
        );
        f.center.answer_authorization(true, None);
        assert!(
            f.center.log().is_empty(),
            "the answer waits for the main thread"
        );
        assert_eq!(f.main.run(), 1);
        assert_eq!(f.center.log(), [added("a"), added("b")], "each token once");
        f.center.answer_add("a", None);
        f.center.answer_add("b", None);
        f.main.run();
        assert!(f.center.log().is_empty());
        assert!(f.failed().is_empty());
        f.post("a");
        assert!(
            f.center.log().is_empty(),
            "a posted token is not posted again"
        );
    }

    #[test]
    fn a_token_withdrawn_before_its_add_never_posts() {
        let f = Fixture::new();
        f.post("a");
        f.post("b");
        f.withdraw("a");
        assert_eq!(f.center.log(), ["authorize", "remove a"]);
        f.center.answer_authorization(true, None);
        f.main.run();
        assert_eq!(f.center.log(), [added("b")]);
    }

    #[test]
    fn a_token_withdrawn_while_it_was_added_goes_once_the_add_returns() {
        let f = Fixture::new();
        f.post("a");
        f.center.answer_authorization(true, None);
        f.main.run();
        assert_eq!(f.center.log(), ["authorize".to_owned(), added("a")]);
        f.withdraw("a");
        assert_eq!(f.center.log(), ["remove a"]);
        // The add finishes after the removal reached the center.
        f.center.answer_add("a", None);
        f.main.run();
        assert_eq!(
            f.center.log(),
            ["remove a"],
            "withdrawn again (`MacAttentionNotifier.mm:108`)"
        );
        assert!(f.failed().is_empty());
    }

    #[test]
    fn without_permission_every_pending_token_fails_with_qts_text() {
        let f = Fixture::new();
        f.post("a");
        f.post("b");
        f.center.answer_authorization(false, None);
        f.main.run();
        assert_eq!(
            f.failed(),
            [
                ("a".to_owned(), DENIED.to_owned()),
                ("b".to_owned(), DENIED.to_owned())
            ]
        );
        assert_eq!(f.center.log(), ["authorize"], "nothing added");
        // A failed token may post again (its retry), with a new check.
        f.post("a");
        assert_eq!(f.center.log(), ["authorize"]);
        f.center
            .answer_authorization(false, Some("Notifications are not allowed."));
        f.main.run();
        assert_eq!(
            f.failed(),
            [("a".to_owned(), "Notifications are not allowed.".to_owned())],
            "the system's own reason when it gives one"
        );
    }

    #[test]
    fn a_failed_add_reports_the_systems_reason() {
        let f = Fixture::new();
        f.post("a");
        f.center.answer_authorization(true, None);
        f.main.run();
        f.center.log();
        f.center.answer_add("a", Some("The request was refused."));
        assert!(f.failed().is_empty(), "decided on the main thread");
        f.main.run();
        assert_eq!(
            f.failed(),
            [("a".to_owned(), "The request was refused.".to_owned())]
        );
        f.post("a");
        assert_eq!(f.center.log(), ["authorize"], "the retry posts again");
    }

    #[test]
    fn a_center_that_answers_at_once_does_not_deadlock() {
        // The system may call a completion before its call returns.
        struct Immediate(Arc<Mutex<Vec<String>>>);
        impl Center for Immediate {
            fn authorize(&self, done: Done<(bool, Option<String>)>) {
                done((true, None));
            }
            fn add(&self, token: &str, _: &str, _: &str, done: Done<Option<String>>) {
                self.0.lock().unwrap().push(format!("add {token}"));
                done(None);
            }
            fn remove(&self, token: &str) {
                self.0.lock().unwrap().push(format!("remove {token}"));
            }
        }
        let log = Arc::new(Mutex::new(Vec::new()));
        let notifier = Notifier::new(
            Box::new(Immediate(Arc::clone(&log))),
            Arc::new(|work| work()),
            Box::new(|_, _| panic!("nothing fails")),
        );
        notifier.deliver(
            &[],
            &[Notice {
                token: "a".into(),
                title: "Zerus".into(),
                body: "sample".into(),
            }],
        );
        notifier.deliver(&["a".to_owned()], &[]);
        assert_eq!(*log.lock().unwrap(), ["add a", "remove a"]);
    }
}
