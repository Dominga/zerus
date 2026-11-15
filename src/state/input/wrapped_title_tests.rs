//! Synthetic ANSI frame: native geometry/styles, sanitized folder and title.
use super::*;
const FRAME: &str = include_str!("../../../tests/fixtures/codex-composer-wrapped-title-0169.txt");
#[test]
fn codex_wrapped_native_title_is_an_empty_composer() {
    let record =
        json!({"agent":"codex","model":"gpt-6.1-sol","cwd":"/fixture/root/fixture-project"});
    assert_eq!(FRAME.lines().count(), 24);
    assert!(codex_wrapped_empty(&record, FRAME, 2, 19, 80));
    assert!(codex_wrapped_columns(&record, FRAME, 2, 19).is_some());
}
