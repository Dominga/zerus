//! Committed review regression N9: the Terminal reader uses the native width.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
struct Frame {
    width: Option<&'static str>,
    reads: AtomicUsize,
}
impl backend::TerminalBackend for Frame {
    fn input_state(&self, _: &backend::InputClaim) -> Result<backend::InputState> {
        Ok(backend::InputState {
            cursor_x: "2".into(),
            cursor_y: "19".into(),
        })
    }
    fn capture(&self, _: &str, _: backend::Capture) -> Result<Vec<u8>> {
        Ok(
            include_bytes!("../../../tests/fixtures/codex-composer-wrapped-status-0169.txt")
                .to_vec(),
        )
    }
    fn pane_info(
        &self,
        target: backend::Target,
        fields: &[backend::PaneField],
    ) -> Result<backend::PaneInfo> {
        assert!(
            matches!(target, backend::Target::Pane("%1"))
                && matches!(fields, [backend::PaneField::Width])
        );
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.width
            .map(|w| backend::PaneInfo::new(format!("{w}\n")))
            .ok_or("width unavailable".into())
    }
}
#[test]
// WhiteWh fork: the Windows Terminal reader asks its host, not a pane width.
#[cfg_attr(windows, ignore)]
fn wrapped_composer_requires_a_successful_native_width_read() {
    let record = json!({"agent":"codex","cwd":"/tmp/hgs-tn-fixture1/work/zerus-team-11111111-1111-4111-8111-111111111111",
        "pane":"%1","name":"codex/fixture/terminal","run_id":"run"});
    for (width, empty) in [
        (Some("80"), true),
        (Some("79"), false),
        (Some("81"), true),
        (Some("200"), false),
        (None, false),
    ] {
        // Content wrapping permits this exact two-row footer at 81 as well.
        // At 79 a physical row is wider; at 200 the second row is unnecessary.
        let frame = Frame {
            width,
            reads: AtomicUsize::new(0),
        };
        assert_eq!(
            composer_now(&frame, &record) == Some(input::composer::Composer::Empty),
            empty,
            "{width:?}"
        );
        assert_eq!(frame.reads.load(Ordering::Relaxed), 1);
    }
}

#[test]
// WhiteWh fork: the Windows Terminal reader asks its host, not a pane width.
#[cfg_attr(windows, ignore)]
fn wrapped_title_read_is_empty_only_with_its_native_width_and_model() {
    struct NamedFrame(Option<&'static str>);
    impl backend::TerminalBackend for NamedFrame {
        fn input_state(&self, _: &backend::InputClaim) -> Result<backend::InputState> {
            Ok(backend::InputState {
                cursor_x: "2".into(),
                cursor_y: "19".into(),
            })
        }
        fn capture(&self, _: &str, _: backend::Capture) -> Result<Vec<u8>> {
            Ok(
                include_bytes!("../../../tests/fixtures/codex-composer-wrapped-title-0169.txt")
                    .to_vec(),
            )
        }
        fn pane_info(
            &self,
            _: backend::Target,
            fields: &[backend::PaneField],
        ) -> Result<backend::PaneInfo> {
            assert!(matches!(fields, [backend::PaneField::Width]));
            self.0
                .map(|v| backend::PaneInfo::new(format!("{v}\n")))
                .ok_or("width unavailable".into())
        }
        fn send(&self, _: &str, _: &[backend::Key]) -> Result<()> {
            panic!("composer inspection must never send keys")
        }
    }
    let record = json!({"agent":"codex","model":"gpt-6.1-sol","cwd":"/fixture/root/fixture-project",
        "pane":"%1","name":"codex/fixture/title","run_id":"pinned-run"});
    for (width, empty) in [
        (Some("80"), true),
        (Some("78"), false),
        (Some("128"), false),
        (Some("bad"), false),
        (None, false),
    ] {
        assert_eq!(
            composer_now(&NamedFrame(width), &record) == Some(input::composer::Composer::Empty),
            empty
        );
    }
    let mut changed = record;
    changed["model"] = json!("gpt-5.4");
    assert_ne!(
        composer_now(&NamedFrame(Some("80")), &changed),
        Some(input::composer::Composer::Empty)
    );
}
