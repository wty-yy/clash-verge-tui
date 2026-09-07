mod support;
use clash_verge_tui::{
    core::{Command, CoreClient, CoreEvent, LogEvent, Worker},
    subscriptions::{self, Source},
    workspace::{self, WorkspaceCommand, WorkspaceContext},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use support::*;

#[tokio::test]
async fn authenticated_reads_encoded_actions_and_sanitized_runtime_config() {
    let server = Server::new(|req| {
        if req.path.starts_with("/api/") {
            (204, String::new())
        } else {
            (200, response(&req.path).to_string())
        }
    });
    let client = CoreClient::new(&server.url, "test-secret".into()).unwrap();
    let snapshot = client.snapshot().await.unwrap();
    assert_eq!(snapshot.version, "test-mihomo");
    assert!(snapshot.config.get("secret").is_none());
    assert!(snapshot.config.get("authentication").is_none());
    let client = CoreClient::new(&(server.url.clone() + "/api"), "test-secret".into()).unwrap();
    for command in [
        Command::Select {
            group: "Main / #?".into(),
            node: "Second".into(),
        },
        Command::Mode("global".into()),
        Command::Close(Some("uuid-connection".into())),
        Command::Close(None),
        Command::DisableRule {
            index: 7,
            disabled: true,
        },
        Command::Patch(json!({"ipv6":true})),
        Command::Reload("mode: rule".into()),
    ] {
        client.execute(&command).await.unwrap();
    }
    let requests = server.requests.lock().unwrap();
    assert!(requests.iter().all(|r| r
        .headers
        .to_lowercase()
        .contains("authorization: bearer test-secret")));
    let selection = requests
        .iter()
        .find(|r| r.method == "PUT" && r.path.contains("proxies/"))
        .unwrap();
    assert_eq!(selection.path, "/api/proxies/Main%20%2F%20%23%3F");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&selection.body).unwrap(),
        json!({"name":"Second"})
    );
    assert!(requests
        .iter()
        .any(|r| r.path == "/api/configs?force=true" && r.body.contains("mode: rule")));
    assert!(requests
        .iter()
        .any(|r| r.path == "/api/rules/disable" && r.body == "{\"7\":true}"));
}
#[tokio::test]
async fn errors_do_not_expose_response_contents_or_credentials() {
    let server = Server::new(|_| (401, "DO-NOT-LEAK-RESPONSE".into()));
    let client = CoreClient::new(&server.url, "DO-NOT-LEAK-KEY".into()).unwrap();
    let error = client.snapshot().await.unwrap_err().to_string();
    assert!(error.contains("401"));
    assert!(!error.contains("DO-NOT-LEAK"));
    assert!(CoreClient::new("http://user:password@localhost:1", String::new()).is_err());
    assert!(CoreClient::new("file:///tmp/config", String::new()).is_err());
}
#[test]
fn worker_receives_live_events_recovers_and_stops_without_waiting_for_streams() {
    let fail = Arc::new(AtomicBool::new(true));
    let flag = fail.clone();
    let server = Server::new(move |req| {
        if flag.load(Ordering::Relaxed) {
            return (503, String::new());
        }
        if req.path.starts_with("/logs") {
            (
                200,
                "{\"type\":\"warning\",\"payload\":\"fixture log\"}\n".into(),
            )
        } else {
            (200, response(&req.path).to_string())
        }
    });
    let worker = Worker::spawn(CoreClient::new(&server.url, String::new()).unwrap()).unwrap();
    assert!(matches!(
        worker.events.recv_timeout(Duration::from_secs(3)).unwrap(),
        CoreEvent::Offline(_)
    ));
    fail.store(false, Ordering::Relaxed);
    assert!(matches!(
        worker.events.recv_timeout(Duration::from_secs(3)).unwrap(),
        CoreEvent::Snapshot(_)
    ));
    let start = Instant::now();
    let mut saw_log = false;
    while start.elapsed() < Duration::from_secs(4) {
        if matches!(
            worker.logs.recv_timeout(Duration::from_secs(1)),
            Ok(LogEvent::Entry { .. })
        ) {
            saw_log = true;
            break;
        }
    }
    assert!(saw_log);
    let start = Instant::now();
    drop(worker);
    assert!(start.elapsed() < Duration::from_secs(1));
}
#[tokio::test]
async fn subscriptions_validate_before_replacing_cached_content() {
    let fail = Arc::new(AtomicBool::new(false));
    let flag = fail.clone();
    let server = Server::new(move |_| {
        if flag.load(Ordering::Relaxed) {
            (403, "secret-response".into())
        } else {
            (200, YAML.into())
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let source = Source {
        name: "fixture".into(),
        url: format!("{}/private-token", server.url),
        proxy: None,
    };
    let stored = subscriptions::download(&source, dir.path(), 0)
        .await
        .unwrap();
    assert_eq!(stored.proxies, 1);
    assert_eq!(std::fs::read_to_string(&stored.file).unwrap(), YAML);
    fail.store(true, Ordering::Relaxed);
    let error = subscriptions::download(&source, dir.path(), 0)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("403"));
    assert!(!error.contains("private-token"));
    assert!(!error.contains("secret-response"));
    assert_eq!(std::fs::read_to_string(&stored.file).unwrap(), YAML);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(stored.file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(subscriptions::parse_config("<html>challenge</html>").is_err());
    assert!(subscriptions::parse_config("mode: rule").is_err());
}
#[test]
fn managed_configuration_is_loopback_only_and_rewrites_provider_paths() {
    let source=format!("{YAML}mixed-port: 7897\nallow-lan: true\nexternal-controller: 0.0.0.0:9090\ntun:\n  enable: true\nlisteners: [untrusted]\ntunnels: [untrusted]\nrule-providers:\n  fixture:\n    type: http\n    path: /tmp/outside.yaml\n    url: https://example.com/rules\ndns:\n  enable: true\n  listen: 0.0.0.0:53\n");
    let result =
        subscriptions::normalized_config(&source, "127.0.0.1:19097", "fixture-secret", 17897)
            .unwrap();
    let config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&result).unwrap();
    assert_eq!(config["mixed-port"].as_u64(), Some(17897));
    assert_eq!(config["allow-lan"].as_bool(), Some(false));
    assert_eq!(config["tun"]["enable"].as_bool(), Some(false));
    assert_eq!(config["bind-address"].as_str(), Some("127.0.0.1"));
    assert!(config.get("listeners").is_none());
    assert!(config.get("tunnels").is_none());
    assert!(config["dns"].get("listen").is_none());
    assert_eq!(
        config["rule-providers"]["fixture"]["path"].as_str(),
        Some("providers/rule-providers-0.yaml")
    );
    assert_eq!(
        config["rules"],
        serde_yaml_ng::from_str::<serde_yaml_ng::Value>(YAML).unwrap()["rules"]
    );
}

#[tokio::test]
async fn explicit_subscription_proxy_routes_download_without_direct_dns() {
    let proxy = Server::new(|_| (200, YAML.into()));
    let dir = tempfile::tempdir().unwrap();
    let source = Source {
        name: "proxied fixture".into(),
        url: "http://unresolvable.invalid/subscription".into(),
        proxy: Some(proxy.url.clone()),
    };
    let profile = subscriptions::download(&source, dir.path(), 0)
        .await
        .unwrap();
    assert_eq!(profile.proxies, 1);
    assert_eq!(proxy.requests.lock().unwrap()[0].path, source.url);
    assert_eq!(profile.proxy, Some(proxy.url.clone()));
}

#[tokio::test]
async fn profile_link_fetch_returns_the_complete_validated_yaml_without_writing_a_file() {
    let server = Server::new(|request| {
        if request.path == "/profile.yaml" {
            (200, YAML.into())
        } else {
            (404, String::new())
        }
    });
    let source = Source {
        name: String::new(),
        url: format!("{}/profile.yaml", server.url),
        proxy: None,
    };
    let fetched = subscriptions::fetch(&source).await.unwrap();
    assert_eq!(fetched.content, YAML);
    assert_eq!(fetched.proxies, 1);
    assert_eq!(fetched.groups, 1);
}

#[test]
fn worker_imports_profile_links_without_blocking_the_ui_thread() {
    let server = Server::new(|request| {
        if request.path == "/profile.yaml" {
            (200, YAML.into())
        } else {
            (200, response(&request.path).to_string())
        }
    });
    let client = CoreClient::new(&server.url, String::new()).unwrap();
    let worker = Worker::spawn(client).unwrap();
    worker
        .send(Command::ImportProfile {
            request: 42,
            url: format!("{}/profile.yaml", server.url),
            proxy: None,
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let event = worker
            .events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        if let CoreEvent::ProfileImported { request, result } = event {
            let fetched = result.unwrap();
            assert_eq!(request, 42);
            assert_eq!(fetched.content, YAML);
            break;
        }
    }
}

#[test]
fn worker_marks_tun_workspace_changes_for_a_core_restart() {
    let server = Server::new(|request| (200, response(&request.path).to_string()));
    let dir = tempfile::tempdir().unwrap();
    workspace::initialize(dir.path()).unwrap();
    let context = WorkspaceContext {
        dir: dir.path().into(),
        binary: "/bin/true".into(),
        controller: "127.0.0.1:19097".into(),
        secret: "fixture-secret".into(),
        port: 17897,
    };
    let worker = Worker::spawn_with_workspace(
        CoreClient::new(&server.url, String::new()).unwrap(),
        Some(context),
    )
    .unwrap();
    worker
        .send(Command::Workspace(WorkspaceCommand::Settings(
            BTreeMap::from([
                ("tun".into(), "关闭".into()),
                ("tun_device".into(), "cvtun0".into()),
            ]),
        )))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut restart = false;
    while Instant::now() < deadline {
        match worker
            .events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap()
        {
            CoreEvent::WorkspaceRestart(_) => restart = true,
            CoreEvent::Completed(Ok(())) => break,
            _ => {}
        }
    }
    assert!(restart);
}

#[tokio::test]
async fn partial_import_preserves_successes_and_returns_each_failure() {
    let server = Server::new(|req| {
        if req.path == "/valid" {
            (200, YAML.into())
        } else {
            (403, String::new())
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path().join("sources.json");
    std::fs::write(&manifest, serde_json::to_vec(&json!([{"name":"one","url":format!("{}/valid",server.url)},{"name":"two","url":format!("{}/blocked",server.url)}])).unwrap()).unwrap();
    let result = subscriptions::import_file(&manifest, dir.path(), None)
        .await
        .unwrap();
    assert!(result[0].result.is_ok());
    assert!(result[1].result.is_err());
    let profiles = subscriptions::load_profiles(dir.path()).unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].name, "one");
}

#[tokio::test]
async fn maintenance_actions_use_expected_core_routes() {
    let server = Server::new(|_| (204, String::new()));
    let client = CoreClient::new(&server.url, "fixture".into()).unwrap();
    client
        .execute(&Command::Upgrade {
            kind: "geo".into(),
            channel: None,
        })
        .await
        .unwrap();
    client
        .execute(&Command::Upgrade {
            kind: "ui".into(),
            channel: None,
        })
        .await
        .unwrap();
    client
        .execute(&Command::Upgrade {
            kind: "core".into(),
            channel: Some("alpha".into()),
        })
        .await
        .unwrap();
    client
        .execute(&Command::Unfix("auto / group".into()))
        .await
        .unwrap();
    let req = server.requests.lock().unwrap();
    assert!(req
        .iter()
        .any(|r| r.method == "POST" && r.path == "/upgrade/geo"));
    assert!(req
        .iter()
        .any(|r| r.method == "POST" && r.path == "/upgrade/ui"));
    assert!(req.iter().any(|r| r.path == "/upgrade?channel=alpha"));
    assert!(req
        .iter()
        .any(|r| r.method == "DELETE" && r.path == "/proxies/auto%20%2F%20group"));
}
