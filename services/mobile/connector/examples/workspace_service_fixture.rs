//! Never packaged. A private owned process/journal and native-child barrier.
#[cfg(all(target_os = "linux", feature = "test-hooks"))]
mod fixture {
    use serde_json::{json, Value};
    use std::{
        path::{Path, PathBuf},
        rc::Rc,
    };
    use zerus_mobile_connector::{
        self as connector, native,
        runtime::{Connector, Settings},
    };
    pub fn main() {
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        let (check, path) = match args.as_slice() {
            [a, b, c, d] if a == "--check" && b == "--json" && c == "--config" => {
                (true, PathBuf::from(d))
            }
            [a, b] if a == "--config" => (false, PathBuf::from(b)),
            _ => std::process::exit(2),
        };
        if check {
            std::process::exit(connector::check::run(&path, true, true))
        }
        let value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let started = connector::start(
            &path,
            true,
            &connector::config::Environment::current().unwrap(),
            connector::now(),
        )
        .unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        tokio::task::LocalSet::new().block_on(&rt,async move {
            let settings=Settings{interval:5.0,binding:started.config.binding.clone(),default_timeout:45.0,clock_scale:1.0};
            let native=native::Process::new(started.config.hgs_path.clone(),45.0);
            let state=started.config.state_dir.clone();
            let engine=Rc::new(Connector::new(native,started.journal,settings,Box::new(connector::now)));
            if started.config.encrypted_workspace.is_some() {
                let generation=connector::workspace_lifecycle::publish(&state).unwrap();
                let signal=connector::workspace_service_fixture_signals(&engine.stopping);
                let monitor=connector::workspace_lifecycle::monitor(&engine.stopping,&state,&generation);
                let _=futures_util::future::select(std::pin::pin!(signal),std::pin::pin!(monitor)).await;
            }else{
                let id="00000000-0000-4000-8000-000000000351";
                let target=value["fixture_target"].as_str().unwrap();
                let session="codex/example/service-fixture";
                {let mut state=engine.state.borrow_mut();state.sessions.insert(session.into());state.snapshot_seen_at=Some(engine.clock());}
                // Ordinary send retains the production 45s default native
                // budget. No artificial timeout hook or production change.
                let request=json!({"request_id":id,"operation":"send","session":session,"payload":{"request_id":id,"expected_run_id":"fixture-run","expected_conversation_id":"fixture-conversation","text":"fixture held message"}});
                let work=async {
                    engine.execute(request).await.unwrap();
                    std::fs::write(Path::new(target).parent().unwrap().join("fixture-outcome.json"),engine.journal.replay(id).unwrap()).unwrap();
                };
                let signals=connector::workspace_service_fixture_signals(&engine.stopping);
                let _=futures_util::future::select(std::pin::pin!(signals),std::pin::pin!(work)).await;
            }
        });
    }
}
#[cfg(all(target_os = "linux", feature = "test-hooks"))]
fn main() {
    fixture::main()
}
#[cfg(not(all(target_os = "linux", feature = "test-hooks")))]
fn main() {
    eprintln!("This isolated fixture requires Linux and the test-hooks feature");
    std::process::exit(2)
}
