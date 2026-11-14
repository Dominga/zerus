use super::harness::*;
use crate::teams::{catalog, cli::actor::Caller};
use serde_json::{json, Value};
use zerus_team_plan::catalog::{Catalog, Template};

fn input(step: u32) -> Value {
    json!({"request_id":uid(0xd000+step),"team":TEAM,"expected_revision":0,
        "project":"example-project","name":"Confirmed team","goal":"Respect the confirmed settings.",
        "home":MACHINE,"integration_branch":"zerus/team/confirmed","base_commit":COMMIT})
}
fn save(h: &Harness, t: &Template) {
    let (r, exit) = h.cli(
        &Caller::Person,
        "template save",
        &serde_json::to_string(t).unwrap(),
    );
    assert_eq!(exit, 0, "{r}");
}
fn show(h: &Harness, id: &str) -> String {
    let (r, exit) = h.cli(&Caller::Person, "catalog show", "");
    assert_eq!(exit, 0, "{r}");
    r["catalog"]["templates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == id)
        .unwrap()["digest"]
        .as_str()
        .expect("template digest")
        .to_owned()
}
fn custom() -> Template {
    let mut t = zerus_team_plan::builtin()
        .templates
        .into_iter()
        .find(|t| t.id == "bugfix")
        .unwrap();
    t.id = "confirmed".into();
    t.settings.delegations.merge_accepted = false;
    t
}
fn create(h: &Harness, id: &str, value: &Value) -> (Value, i32) {
    h.cli(
        &Caller::Person,
        &format!("create --template {id}"),
        &value.to_string(),
    )
}
fn refused_changed(h: &Harness, id: &str, digest: &str) -> Value {
    let mut request = input(1);
    request["expected_template"] = json!(digest);
    let (r, exit) = create(h, id, &request);
    assert_eq!((exit, code(&r)), (1, "template_changed"), "{r}");
    assert_eq!(
        r["error"]["message"],
        "The template changed since it was shown. Review it again."
    );
    assert!(!h.state().join("teams/teams-v1.sqlite3").exists());
    r
}

#[test]
fn a_replaced_saved_template_is_refused_until_it_is_shown_again() {
    let h = Harness::new();
    let mut t = custom();
    save(&h, &t);
    let old = show(&h, &t.id);
    t.settings.delegations.merge_accepted = true;
    save(&h, &t);
    refused_changed(&h, &t.id, &old);
    let mut request = input(2);
    request["expected_template"] = json!(show(&h, &t.id));
    let (r, exit) = create(&h, &t.id, &request);
    assert_eq!(exit, 0, "{r}");
    assert!(
        h.store()
            .team(TEAM)
            .unwrap()
            .unwrap()
            .settings
            .delegations
            .merge_accepted
    );
}

#[test]
fn a_personal_shadow_does_not_use_the_built_in_digest() {
    let h = Harness::new();
    let old = show(&h, "bugfix");
    let mut t = custom();
    t.id = "bugfix".into();
    save(&h, &t);
    assert_ne!(show(&h, "bugfix"), old);
    refused_changed(&h, "bugfix", &old);
}

#[test]
fn delegations_override_the_template_and_are_reported_in_status() {
    let h = Harness::new();
    let mut request = input(1);
    request["expected_template"] = json!(show(&h, "bugfix"));
    let delegations = json!({"accept_results":false,"answer_members":true,"confirm_trust":false,"resume_exited":true,"merge_accepted":false});
    request["delegations"] = delegations.clone();
    let (r, exit) = create(&h, "bugfix", &request);
    assert_eq!(exit, 0, "{r}");
    let (status, exit) = h.cli(
        &Caller::Person,
        &format!("status --team {TEAM} --bundle"),
        "",
    );
    assert_eq!(exit, 0, "{status}");
    assert_eq!(status["team"]["settings"]["delegations"], delegations);
    assert!(
        status["team"]["bundle"]["template"]["settings"]
            .get("delegations")
            .is_none(),
        "the source template's delegations are not shown twice: {status}"
    );
    let team = h.store().team(TEAM).unwrap().unwrap();
    assert!(
        team.bundle.template.settings.delegations.merge_accepted,
        "the pinned source template is unchanged"
    );
}

#[test]
fn partial_delegations_are_refused_without_creating_a_team() {
    let full = serde_json::to_value(zerus_team_plan::model::Delegations::default()).unwrap();
    for missing in full.as_object().unwrap().keys() {
        let h = Harness::new();
        let mut request = input(1);
        let mut partial = full.clone();
        partial.as_object_mut().unwrap().remove(missing);
        request["delegations"] = partial;
        let (r, exit) = create(&h, "bugfix", &request);
        assert_eq!((exit, code(&r)), (1, "invalid_state"), "{r}");
        assert!(!h.state().join("teams/teams-v1.sqlite3").exists());
    }
}

#[test]
fn a_create_replays_after_its_template_is_changed_or_removed() {
    for bound in [false, true] {
        let h = Harness::new();
        let mut t = custom();
        save(&h, &t);
        let mut request = input(1);
        if bound {
            request["expected_template"] = json!(show(&h, &t.id));
        }
        let (first, exit) = create(&h, &t.id, &request);
        assert_eq!(exit, 0, "{first}");
        t.settings.delegations.merge_accepted = true;
        save(&h, &t);
        let (repeat, exit) = create(&h, &t.id, &request);
        assert_eq!(exit, 0, "{repeat}");
        assert_eq!(repeat, first);
        let (r, exit) = h.cli(&Caller::Person, &format!("template remove {}", t.id), "");
        assert_eq!(exit, 0, "{r}");
        let (repeat, exit) = create(&h, &t.id, &request);
        assert_eq!(exit, 0, "{repeat}");
        assert_eq!(repeat, first);
        assert_eq!(h.store().journal(TEAM, 200, None).unwrap().rows.len(), 1);
        let mut changed = request.clone();
        changed["name"] = json!("A different decision");
        let (r, exit) = create(&h, &t.id, &changed);
        assert_eq!((exit, code(&r)), (1, "duplicate"), "{r}");
    }
}

#[test]
fn the_digest_is_stable_across_catalog_rewrites_that_change_nothing() {
    let h = Harness::new();
    let before = show(&h, "bugfix");
    let c = zerus_team_plan::builtin();
    let (r, exit) = h.cli(
        &Caller::Person,
        "catalog import",
        &serde_json::to_string(&c).unwrap(),
    );
    assert_eq!(exit, 0, "{r}");
    assert_eq!(show(&h, "bugfix"), before);
    std::fs::write(
        catalog::path(&h.config.dir),
        serde_json::to_vec(&c).unwrap(),
    )
    .unwrap();
    assert_eq!(show(&h, "bugfix"), before);
}
fn changes_digest(change: impl FnOnce(&mut Catalog)) {
    let h = Harness::new();
    let before = show(&h, "bugfix");
    let mut c = zerus_team_plan::builtin();
    change(&mut c);
    let (r, exit) = h.cli(
        &Caller::Person,
        "catalog import",
        &serde_json::to_string(&c).unwrap(),
    );
    assert_eq!(exit, 0, "{r}");
    assert_ne!(show(&h, "bugfix"), before);
}
#[test]
fn the_digest_covers_delegations() {
    changes_digest(|c| {
        c.templates
            .iter_mut()
            .find(|t| t.id == "bugfix")
            .unwrap()
            .settings
            .delegations
            .merge_accepted = false
    });
}
#[test]
fn the_digest_covers_roster_agents() {
    changes_digest(|c| {
        c.roles
            .iter_mut()
            .find(|r| r.id == "developer")
            .unwrap()
            .agent = "codex".into()
    });
}
#[test]
fn the_digest_covers_pipeline_acceptance() {
    changes_digest(|c| {
        c.pipelines
            .iter_mut()
            .find(|p| p.id == "reviewed")
            .unwrap()
            .accept = zerus_team_plan::model::Acceptance::Pipeline
    });
}

#[test]
fn the_digest_is_checked_against_the_frozen_bundle_not_a_second_catalog_read() {
    let h = Harness::new();
    let t = custom();
    save(&h, &t);
    let old = show(&h, &t.id);
    let mut replacement = catalog::load(&h.config.dir).unwrap();
    replacement
        .templates
        .iter_mut()
        .find(|entry| entry.id == t.id)
        .unwrap()
        .settings
        .delegations
        .merge_accepted = true;
    let dir = h.config.dir.clone();
    crate::teams::cli::create::set_after_snapshot(move || {
        catalog::save(&dir, &replacement).unwrap()
    });
    let mut request = input(1);
    request["expected_template"] = json!(old);
    let (r, exit) = create(&h, &t.id, &request);
    assert_eq!(exit, 0, "{r}");
    let team = h.store().team(TEAM).unwrap().unwrap();
    assert!(
        !team.settings.delegations.merge_accepted,
        "the confirmed frozen bundle is the one applied"
    );
    assert_ne!(
        show(&h, &t.id),
        request["expected_template"].as_str().unwrap()
    );
}

pub(super) fn changed_refusal_fixture() -> Value {
    let h = Harness::new();
    let old = show(&h, "bugfix");
    let mut t = custom();
    t.id = "bugfix".into();
    save(&h, &t);
    refused_changed(&h, "bugfix", &old)
}

#[test]
fn the_changed_template_reply_is_the_command_reply() {
    let r = changed_refusal_fixture();
    assert_eq!(code(&r), "template_changed");
}


#[test]
fn a_request_id_replayed_with_another_template_is_refused() {
    let h = Harness::new();
    let request = input(1);
    let (first, exit) = create(&h, "bugfix", &request);
    assert_eq!(exit, 0, "{first}");
    let (replay, exit) = create(&h, "feature-delivery", &request);
    assert_eq!(exit, 1, "{replay}");
    assert_eq!(code(&replay), "duplicate", "{replay}");
    assert_eq!(
        replay["error"]["message"],
        "Request ID was used with a different operation or template."
    );
    assert_eq!(h.store().journal(TEAM, 200, None).unwrap().rows.len(), 1);
}

#[test]
fn a_null_or_empty_expected_template_is_its_own_refusal() {
    for value in [Value::Null, json!("")] {
        let h = Harness::new();
        let mut request = input(1);
        request["expected_template"] = value;
        let (reply, exit) = create(&h, "bugfix", &request);
        assert_eq!(exit, 1, "{reply}");
        assert_eq!(code(&reply), "invalid_state", "{reply}");
        assert_eq!(
            reply["error"]["message"],
            "expected_template must be a template digest, or left out."
        );
        assert_eq!(reply["error"]["refs"][0], "expected_template");
        assert!(!h.state().join("teams/teams-v1.sqlite3").exists());
    }
}
