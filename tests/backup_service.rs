mod support;
use clash_verge_tui::{
    backup::{self, BackupCommand as B},
    core::CoreClient,
    model::Enhancement,
    service,
    workspace::{self, WorkspaceCommand as W, WorkspaceContext},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
#[tokio::test]
async fn encrypted_webdav_backup_roundtrip_preserves_data_and_rejects_wrong_password() {
    let files = Arc::new(Mutex::new(BTreeMap::<String, String>::new()));
    let stored = files.clone();
    let server = support::Server::new(move |req| {
        let _ = &req.headers;
        if req.path.starts_with("/dav") {
            let name = req.path.rsplit('/').next().unwrap_or("").to_string();
            match req.method.as_str() {
                "PUT" => {
                    stored.lock().unwrap().insert(name, req.body.clone());
                    (201, String::new())
                }
                "GET" => (
                    200,
                    stored
                        .lock()
                        .unwrap()
                        .get(&name)
                        .cloned()
                        .unwrap_or_default(),
                ),
                "DELETE" => {
                    stored.lock().unwrap().remove(&name);
                    (204, String::new())
                }
                "PROPFIND" => {
                    let body = stored
                        .lock()
                        .unwrap()
                        .keys()
                        .map(|name| {
                            format!("<d:response><d:href>/dav/{name}</d:href></d:response>")
                        })
                        .collect::<String>();
                    (
                        207,
                        format!("<d:multistatus xmlns:d=\"DAV:\">{body}</d:multistatus>"),
                    )
                }
                _ => (405, String::new()),
            }
        } else {
            (200, support::response(&req.path).to_string())
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let ctx = WorkspaceContext {
        dir: dir.path().into(),
        binary: "/bin/true".into(),
        controller: "127.0.0.1:19097".into(),
        secret: "fixture-secret".into(),
        port: 17897,
    };
    let client = CoreClient::new(&server.url, String::new()).unwrap();
    workspace::execute(
        &ctx,
        &client,
        W::PutProfile {
            index: None,
            name: "private original".into(),
            url: String::new(),
            proxy: None,
            content: Some(support::YAML.into()),
            interval: 0,
        },
    )
    .await
    .unwrap();
    workspace::execute(
        &ctx,
        &client,
        W::Settings(BTreeMap::from([
            ("backup_password".into(), "test passphrase".into()),
            ("webdav_url".into(), format!("{}/dav", server.url)),
            ("webdav_user".into(), "fixture".into()),
            ("webdav_password".into(), "fixture".into()),
        ])),
    )
    .await
    .unwrap();
    let result = backup::execute(&ctx, &client, B::Create).await.unwrap();
    let file = result.items[0].file.clone();
    assert!(result.items[0].encrypted);
    assert!(
        !std::fs::read_to_string(dir.path().join("backups").join(&file))
            .unwrap()
            .contains("private original")
    );
    backup::execute(&ctx, &client, B::Upload(file.clone()))
        .await
        .unwrap();
    let remote = backup::execute(&ctx, &client, B::List { remote: true })
        .await
        .unwrap();
    assert_eq!(remote.items.len(), 1);
    workspace::execute(
        &ctx,
        &client,
        W::Settings(BTreeMap::from([("backup_password".into(), "wrong".into())])),
    )
    .await
    .unwrap();
    let before = std::fs::read(dir.path().join("workspace-state.json")).unwrap();
    assert!(backup::execute(
        &ctx,
        &client,
        B::Restore {
            file: file.clone(),
            remote: true
        }
    )
    .await
    .is_err());
    assert_eq!(
        std::fs::read(dir.path().join("workspace-state.json")).unwrap(),
        before
    );
    workspace::execute(
        &ctx,
        &client,
        W::Settings(BTreeMap::from([(
            "backup_password".into(),
            "test passphrase".into(),
        )])),
    )
    .await
    .unwrap();
    workspace::execute(
        &ctx,
        &client,
        W::PutEnhancement {
            index: None,
            item: Enhancement {
                name: "later change".into(),
                kind: "YAML".into(),
                enabled: true,
                content: "mode: global".into(),
            },
        },
    )
    .await
    .unwrap();
    let result = backup::execute(
        &ctx,
        &client,
        B::Restore {
            file: file.clone(),
            remote: true,
        },
    )
    .await
    .unwrap();
    assert!(result.restored.unwrap().state.enhancements.is_empty());
    assert_eq!(
        workspace::load(dir.path()).unwrap().profiles[0].name,
        "private original"
    );
    backup::execute(&ctx, &client, B::Delete { file, remote: true })
        .await
        .unwrap();
    assert!(files.lock().unwrap().is_empty());
    assert!(server
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|r| r.method == "PROPFIND"));
    assert!(backup::execute(
        &ctx,
        &client,
        B::Delete {
            file: "../escape".into(),
            remote: false
        }
    )
    .await
    .is_err());
}
#[test]
fn service_unit_uses_quoted_paths_and_stable_workspace_identity() {
    let dir = std::path::Path::new("/tmp/work space");
    let text = service::unit(
        std::path::Path::new("/opt/client app"),
        dir,
        &BTreeMap::new(),
    )
    .unwrap();
    assert!(text.contains("\"/opt/client app\" --daemon --data-dir \"/tmp/work space\""));
    assert_eq!(service::name(dir), service::name(dir));
    assert_ne!(
        service::name(dir),
        service::name(std::path::Path::new("/tmp/other"))
    );
    assert!(service::unit(std::path::Path::new("relative"), dir, &BTreeMap::new()).is_err());
}
#[test]
fn access_checks_distinguish_verification_login_and_region_blocks() {
    use clash_verge_tui::extras::classify;
    assert!(classify(403, "cf-chl-challenge", false).contains("未知"));
    assert!(classify(200, "", true).contains("登录"));
    assert!(classify(451, "", false).contains("地区"));
    assert!(classify(200, "hello", false).contains("网页"));
}
