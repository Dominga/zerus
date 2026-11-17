//! Regression rows for the Teams browser, through real operations and views.
use super::harness::*;
use crate::teams::cli::actor::Caller;
use crate::teams::ops::{Actor, Operation};
use serde_json::{json, Value};
use zerus_client_core::{swarm::edit, upstream_teams::view, views::projects::SwarmState};

fn read(h: &Harness, command: &str) -> Value {
    let (reply, exit) = h.cli(&Caller::Person, command, "");
    assert_eq!(exit, 0, "{reply}");
    reply
}

fn team(h: &Harness, id: &str) -> Value {
    serde_json::to_value(view::team(
        &read(h, &format!("status --team {id}")),
        &read(h, &format!("queue --team {id}")),
    ))
    .unwrap()
}

fn rebind() -> Value {
    let h = Harness::new();
    let id = uid(0x5001);
    h.create_team(&id, "Rebound", "zerus/team/p130-rebind");
    let unbound = view::list(&read(&h, "list"));
    h.bind_for(&id, "lead", &format!("launch_id:{}", uid(0x6001)));
    let before_list = view::list(&read(&h, "list"));
    let before_team = team(&h, &id);
    h.apply_for(
        &id,
        &Actor::Service {
            person: "local".into(),
            team: id.clone(),
        },
        Operation::UnbindMember {
            member: "lead".into(),
        },
    );
    let between_list = view::list(&read(&h, "list"));
    h.bind_for(&id, "lead", &format!("launch_id:{}", uid(0x6002)));
    json!({
        "unbound": unbound,
        "before_list": before_list,
        "before_team": before_team,
        "between_list": between_list,
        "after_list": view::list(&read(&h, "list")),
        "after_team": team(&h, &id),
    })
}

fn long_names() -> Value {
    let h = Harness::new();
    let names = [
        "Очень длинное название команды для проверки переноса строк на узком экране при крупном шрифте",
        "НепрерывноеНазваниеКомандыБезПробеловКотороеДолжноПереноситьсяВнутриСтрокиСписка",
        "Short",
    ];
    for (n, name) in names.iter().enumerate() {
        let id = uid(0x7001 + n as u32);
        h.create_team(&id, name, &format!("zerus/team/p130-long-{n}"));
        h.bind_for(
            &id,
            "lead",
            &format!("launch_id:{}", uid(0x8001 + n as u32)),
        );
    }
    json!({ "long": view::list(&read(&h, "list")) })
}

fn projects() -> Value {
    let h = Harness::new();
    let mut seed = fixture().team;
    let project = "00000000-0000-4000-8000-0000000000ee";
    seed.bundle.pipelines[0].stages.clear();
    h.apply_for(
        &uid(0x9001),
        &person(),
        Operation::Create {
            expected_template: None,
            delegations: None,
            project: project.into(),
            name: "Project team".into(),
            goal: seed.goal,
            home: seed.home,
            bundle: Box::new(seed.bundle),
            integration_branch: "zerus/team/project-name".into(),
            base_commit: seed.base_commit,
        },
    );
    // The real shared-edit writer creates and renames the projects; the same
    // public projection used by the screens hub returns every display row.
    let mut organization = json!({"projects": []});
    for (id, name) in [
        (project, "Example project"),
        (
            "example-project",
            "Проект с длинным названием для проверки строк команды",
        ),
    ] {
        organization = edit::apply(
            &organization,
            &edit::Intent::Create {
                id: id.into(),
                name: name.into(),
                members: Vec::new(),
            },
        )
        .unwrap();
    }
    let mut catalog = SwarmState::default();
    catalog.finish(Ok(
        json!({"schema": 1, "organization": organization.clone()}),
    ));
    let projects = catalog.view(&[]);
    organization = edit::apply(
        &organization,
        &edit::Intent::Rename {
            project: project.into(),
            name: "Renamed project".into(),
        },
    )
    .unwrap();
    catalog.finish(Ok(json!({"schema": 1, "organization": organization})));
    json!({
        "list": view::list(&read(&h, "list")),
        "projects": projects,
        "renamed_projects": catalog.view(&[]),
    })
}

fn path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("client/src/ui/screens/fixtures")
        .join(format!("teams-{name}.json"))
}

fn pin(name: &str, actual: Value) {
    let expected: Value =
        serde_json::from_str(&std::fs::read_to_string(path(name)).unwrap()).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn lead_rebind_rows_are_written_by_real_unbind_and_bind_operations() {
    pin("rebind", rebind());
}

#[test]
fn long_team_names_are_written_by_real_creates_and_binds() {
    pin("long", long_names());
}

#[test]
fn project_names_are_written_by_the_product_catalog_projection() {
    pin("projects", projects());
}

#[test]
#[ignore = "regenerates the follow-up UI fixtures from product operations and projections"]
fn regenerate_teams_followup_fixtures() {
    for (name, value) in [
        ("rebind", rebind()),
        ("long", long_names()),
        ("projects", projects()),
    ] {
        std::fs::write(
            path(name),
            format!("{}\n", serde_json::to_string_pretty(&value).unwrap()),
        )
        .unwrap();
    }
}
