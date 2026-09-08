mod support;
use clash_verge_tui::{
    core::CoreClient,
    model::Enhancement,
    workspace::{self, WorkspaceCommand as W, WorkspaceContext},
};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
fn context(dir: &Path) -> WorkspaceContext {
    WorkspaceContext {
        dir: dir.into(),
        binary: "/bin/true".into(),
        controller: "127.0.0.1:19097".into(),
        secret: "fixture-secret".into(),
        port: 17897,
    }
}
fn local(index: Option<usize>, name: &str) -> W {
    W::PutProfile {
        index,
        name: name.into(),
        url: String::new(),
        proxy: None,
        content: Some(support::YAML.into()),
        interval: 0,
    }
}
#[test]
fn initialization_creates_the_authoritative_private_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let state = workspace::initialize(dir.path()).unwrap();
    assert!(state.profiles.is_empty());
    assert_eq!(state.state.overrides["mode"], "rule");
    assert_eq!(state.state.overrides["dns"]["enable"], true);
    let manifest = dir.path().join("workspace-state.json");
    assert!(manifest.is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(manifest).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[tokio::test]
async fn legacy_runtime_defaults_migrate_once_without_changing_subscriptions() {
    let dir = tempfile::tempdir().unwrap();
    let server = support::Server::new(|_| (200, "{}".into()));
    let ctx = context(dir.path());
    let client = CoreClient::new(&server.url, String::new()).unwrap();
    let mut state = workspace::execute(&ctx, &client, local(None, "fixture"))
        .await
        .unwrap();
    let profile = state.profiles[0].file.clone();
    let original = std::fs::read(&profile).unwrap();
    state.state.preferences.remove("runtime_defaults_version");
    state.state.preferences.insert("dns".into(), "关闭".into());
    state
        .state
        .preferences
        .insert("mode".into(), "global".into());
    state
        .state
        .preferences
        .insert("language".into(), "en".into());
    state.state.overrides.insert("mode".into(), "global".into());
    state
        .state
        .overrides
        .insert("mixed-port".into(), 17897.into());
    state.state.overrides.insert(
        "dns".into(),
        serde_yaml_ng::from_str("enable: false\nnameserver: [192.0.2.1]").unwrap(),
    );
    let manifest = dir.path().join("workspace-state.json");
    clash_verge_tui::subscriptions::private_write(&manifest, &serde_json::to_vec(&state).unwrap())
        .unwrap();
    let migrated = workspace::initialize(dir.path()).unwrap();
    assert_eq!(migrated.state.overrides["mode"], "rule");
    assert_eq!(migrated.state.preferences["mode"], "rule");
    assert_eq!(migrated.state.preferences["dns"], "开启");
    assert_eq!(migrated.state.preferences["language"], "en");
    assert_eq!(migrated.state.overrides["mixed-port"], 17897);
    let runtime: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&workspace::compose(&migrated, &ctx).await.unwrap()).unwrap();
    assert_eq!(runtime["mode"], "rule");
    assert_eq!(runtime["dns"]["enable"], true);
    assert_eq!(runtime["dns"]["enhanced-mode"], "fake-ip");
    assert_eq!(
        runtime["dns"]["nameserver"][1],
        "https://1.12.12.12/dns-query"
    );
    assert_eq!(std::fs::read(&profile).unwrap(), original);

    let changed = workspace::execute(
        &ctx,
        &client,
        W::Settings(BTreeMap::from([
            ("mode".into(), "global".into()),
            ("dns".into(), "关闭".into()),
        ])),
    )
    .await
    .unwrap();
    let reloaded = workspace::initialize(dir.path()).unwrap();
    assert_eq!(reloaded.state.overrides, changed.state.overrides);
    assert_eq!(reloaded.state.preferences["dns"], "关闭");
    assert_eq!(std::fs::read(&profile).unwrap(), original);
}
#[tokio::test]
async fn workspace_profile_lifecycle_tracks_active_file_and_transaction_failure() {
    let fail = Arc::new(AtomicBool::new(false));
    let flag = fail.clone();
    let server = support::Server::new(move |req| {
        let _ = &req.headers;
        let _ = &req.body;
        let _ = &req.method;
        let _ = &req.path;
        if flag.load(Ordering::Relaxed) {
            (500, String::new())
        } else {
            (200, support::response("/version").to_string())
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let ctx = context(dir.path());
    let client = CoreClient::new(&server.url, String::new()).unwrap();
    let a = workspace::execute(&ctx, &client, local(None, "first"))
        .await
        .unwrap();
    let original = a.profiles[0].file.clone();
    assert!(original.exists());
    workspace::execute(&ctx, &client, local(None, "second"))
        .await
        .unwrap();
    let b = workspace::execute(
        &ctx,
        &client,
        W::Move {
            index: 0,
            destination: 1,
        },
    )
    .await
    .unwrap();
    assert_eq!(b.active, 1);
    assert_eq!(b.profiles[1].name, "first");
    let before = std::fs::read(dir.path().join("workspace-state.json")).unwrap();
    fail.store(true, Ordering::Relaxed);
    assert!(
        workspace::execute(&ctx, &client, local(Some(1), "replacement"))
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read(dir.path().join("workspace-state.json")).unwrap(),
        before
    );
    assert!(original.exists());
    fail.store(false, Ordering::Relaxed);
    let c = workspace::execute(&ctx, &client, W::Delete(1))
        .await
        .unwrap();
    assert_eq!(c.active, 0);
    assert_eq!(c.profiles[0].name, "second");
    assert!(!original.exists());
    assert!(workspace::execute(&ctx, &client, W::Delete(0))
        .await
        .unwrap()
        .profiles
        .is_empty());
    assert!(server
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|r| r.path.starts_with("/configs")));
}
#[tokio::test]
async fn yaml_and_javascript_enhancements_apply_in_order_and_reject_timeouts() {
    let server = support::Server::new(|_| (200, "{}".into()));
    let dir = tempfile::tempdir().unwrap();
    let ctx = context(dir.path());
    let client = CoreClient::new(&server.url, String::new()).unwrap();
    workspace::execute(&ctx, &client, local(None, "fixture"))
        .await
        .unwrap();
    workspace::execute(
        &ctx,
        &client,
        W::PutEnhancement {
            index: None,
            item: Enhancement {
                name: "yaml".into(),
                kind: "YAML".into(),
                enabled: true,
                content: "mode: global\nlog-level: debug\ndns:\n  enable: true\n".into(),
            },
        },
    )
    .await
    .unwrap();
    let state = workspace::execute(
        &ctx,
        &client,
        W::PutEnhancement {
            index: None,
            item: Enhancement {
                name: "script".into(),
                kind: "JavaScript".into(),
                enabled: true,
                content: "function main(config) { config.mode='direct'; config['log-level']='warning'; return config; }".into(),
            },
        },
    )
    .await
    .unwrap();
    let cfg: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&workspace::compose(&state, &ctx).await.unwrap()).unwrap();
    assert_eq!(cfg["mode"].as_str(), Some("rule"));
    assert_eq!(cfg["log-level"].as_str(), Some("warning"));
    assert_eq!(cfg["dns"]["enable"].as_bool(), Some(true));
    let before = std::fs::read(dir.path().join("workspace-state.json")).unwrap();
    assert!(workspace::execute(
        &ctx,
        &client,
        W::PutEnhancement {
            index: Some(1),
            item: Enhancement {
                name: "loop".into(),
                kind: "JavaScript".into(),
                enabled: true,
                content: "function main(config) { while(true){} }".into()
            }
        }
    )
    .await
    .is_err());
    assert_eq!(
        std::fs::read(dir.path().join("workspace-state.json")).unwrap(),
        before
    );
    let state = workspace::execute(&ctx, &client, W::ToggleEnhancement(1))
        .await
        .unwrap();
    let cfg: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&workspace::compose(&state, &ctx).await.unwrap()).unwrap();
    assert_eq!(cfg["mode"].as_str(), Some("rule"));
    assert_eq!(cfg["log-level"].as_str(), Some("debug"));
}
#[tokio::test]
async fn scheduled_updates_back_off_and_workspace_lock_excludes_writers() {
    let server = support::Server::new(|req| {
        if req.path == "/sub" {
            (200, support::YAML.into())
        } else {
            (200, "{}".into())
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let ctx = context(dir.path());
    let client = CoreClient::new(&server.url, String::new()).unwrap();
    let mut state = workspace::execute(
        &ctx,
        &client,
        W::PutProfile {
            index: None,
            name: "remote".into(),
            url: format!("{}/sub", server.url),
            proxy: None,
            content: None,
            interval: 1,
        },
    )
    .await
    .unwrap();
    let schedule = state.state.schedules.values_mut().next().unwrap();
    schedule.updated_at = 0;
    schedule.checked_at = 0;
    std::fs::write(
        dir.path().join("workspace-state.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    assert_eq!(workspace::due(dir.path()).unwrap(), Some(0));
    assert_eq!(workspace::due(dir.path()).unwrap(), None);
    let lock = workspace::Lock::acquire(dir.path()).unwrap();
    assert!(workspace::execute(&ctx, &client, W::Read).await.is_err());
    drop(lock);
    let old = workspace::load(dir.path()).unwrap().profiles[0]
        .file
        .clone();
    let state = workspace::execute(&ctx, &client, W::Refresh(0))
        .await
        .unwrap();
    assert_ne!(state.profiles[0].file, old);
    assert!(!old.exists());
}

#[tokio::test]
async fn tun_changes_are_staged_for_restart_without_hot_reload_and_can_rollback() {
    let server = support::Server::new(|req| (200, support::response(&req.path).to_string()));
    let dir = tempfile::tempdir().unwrap();
    let ctx = context(dir.path());
    let client = CoreClient::new(&server.url, String::new()).unwrap();
    workspace::initialize(dir.path()).unwrap();
    let original = workspace::load(dir.path()).unwrap();
    server.requests.lock().unwrap().clear();

    let changed = workspace::execute(
        &ctx,
        &client,
        W::Settings(BTreeMap::from([
            ("tun".into(), "关闭".into()),
            ("tun_device".into(), "cvtun0".into()),
        ])),
    )
    .await
    .unwrap();
    assert_eq!(
        changed.state.overrides["tun"]["enable"].as_bool(),
        Some(false)
    );
    assert!(!server
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|request| request.method == "PUT" && request.path.starts_with("/configs")));

    workspace::restore_restart_snapshot(dir.path(), &original).unwrap();
    assert_eq!(
        workspace::load(dir.path()).unwrap().state.overrides,
        original.state.overrides
    );
}
