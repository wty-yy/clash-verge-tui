#[path = "support/fixtures.rs"]
mod fixtures;
use clash_verge_tui::{
    app::{Action, App, Field, Modal, SaveTarget},
    core::{Command, CoreEvent, Snapshot},
    live::{LiveState, ManagedSettings},
    model::*,
    settings::Kind,
    subscriptions::FetchedProfile,
    ui,
};
use ratatui::{backend::TestBackend, Terminal};
use serde_json::json;
fn snapshot() -> Snapshot {
    Snapshot {
        version: "fixture".into(),
        config: fixtures::response("/configs"),
        proxies: fixtures::response("/proxies"),
        rules: fixtures::response("/rules"),
        providers: fixtures::response("/providers/rules"),
        connections: fixtures::response("/connections"),
    }
}
fn app() -> App {
    let mut app = App::new(DemoState::default(), "/tmp/fixture".into());
    app.live = Some(LiveState::new("http://localhost".into(), vec![], None));
    app.handle_core(CoreEvent::Snapshot(Box::new(snapshot())));
    app
}
#[test]
fn live_group_membership_and_pending_operations_never_simulate_success() {
    let mut a = app();
    a.navigate(Page::Proxies);
    a.sub = a
        .state
        .groups
        .iter()
        .position(|g| g.name == "Main / #?")
        .unwrap();
    let rows = a.rows();
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .all(|r| ["First", "Second"].contains(&a.state.nodes[r.id].name.as_str())));
    a.selected = 1;
    a.activate();
    assert!(
        matches!(&a.live.as_ref().unwrap().outbox[0],Command::Select{node,..} if node=="Second")
    );
    assert_eq!(a.state.groups[a.sub].selected, "First");
    a.handle_core(CoreEvent::Completed(Err("HTTP 500".into())));
    assert!(!a.live.as_ref().unwrap().pending);
    assert_eq!(a.state.groups[a.sub].selected, "First");
    a.handle_core(CoreEvent::Offline("down".into()));
    let count = a.live.as_ref().unwrap().outbox.len();
    a.activate();
    assert_eq!(a.live.as_ref().unwrap().outbox.len(), count);
    a.handle_core(CoreEvent::Snapshot(Box::new(snapshot())));
    assert!(a.live.as_ref().unwrap().connected);
    a.sub = a
        .state
        .groups
        .iter()
        .position(|g| g.name == "AUTO")
        .unwrap();
    a.selected = 0;
    a.activate();
    assert!(
        matches!(a.live.as_ref().unwrap().outbox.last(),Some(Command::Select{group,..}) if group=="AUTO")
    );
}
#[test]
fn real_rule_index_and_connection_uuid_are_used_after_confirmation() {
    let mut a = app();
    a.navigate(Page::Rules);
    a.activate();
    assert!(matches!(
        a.live.as_ref().unwrap().outbox[0],
        Command::DisableRule {
            index: 7,
            disabled: true
        }
    ));
    a.handle_core(CoreEvent::Completed(Ok(())));
    a.live.as_mut().unwrap().outbox.clear();
    a.navigate(Page::Connections);
    a.command('d');
    a.key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(
        matches!(&a.live.as_ref().unwrap().outbox[0],Command::Close(Some(id)) if id=="uuid-connection")
    );
}
#[test]
fn live_startup_empty_groups_and_disconnect_render_without_demo_fixtures() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = App::new_live(
        dir.path().into(),
        "http://127.0.0.1:9090".into(),
        vec![],
        0,
        None,
    )
    .unwrap();
    for page in Page::ALL {
        a.navigate(page);
        let mut terminal = Terminal::new(TestBackend::new(76, 24)).unwrap();
        terminal.draw(|f| ui::draw(f, &mut a)).unwrap();
        assert!(a.state.nodes.is_empty());
        assert!(a.state.connections.is_empty());
        assert!(a.state.logs.is_empty());
    }
    a.handle_core(CoreEvent::Snapshot(Box::new(snapshot())));
    a.handle_core(CoreEvent::Offline("offline".into()));
    assert!(a.live.as_ref().unwrap().down_rate.is_none());
    a.navigate(Page::Proxies);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|f| ui::draw(f, &mut a)).unwrap();
}
#[test]
fn live_preferences_do_not_persist_core_config_credentials_or_connections() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = App::new_live(
        dir.path().into(),
        "http://localhost".into(),
        vec![],
        0,
        None,
    )
    .unwrap();
    a.handle_core(CoreEvent::Snapshot(Box::new(snapshot())));
    a.state
        .settings
        .insert("secret".into(), "DO-NOT-SAVE".into());
    a.state.settings.insert("theme".into(), "浅色".into());
    a.save_preferences().unwrap();
    let text = std::fs::read_to_string(dir.path().join("live-preferences.json")).unwrap();
    assert!(!text.contains("DO-NOT-SAVE"));
    assert!(!text.contains("example.com"));
    assert!(!dir.path().join("demo-state.json").exists());
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["theme"], json!("浅色"));
}

#[test]
fn busy_workspace_preserves_unsaved_form_input() {
    let mut a = app();
    a.navigate(Page::Settings);
    a.sub = 2;
    a.activate();
    let fields = if let Some(clash_verge_tui::app::Modal::Form { fields, .. }) = &a.modal {
        fields.clone()
    } else {
        panic!("expected form")
    };
    a.live.as_mut().unwrap().pending = true;
    a.save_live_form(&fields, &clash_verge_tui::app::SaveTarget::Settings(0));
    assert!(
        matches!(&a.modal,Some(clash_verge_tui::app::Modal::Form{error,..}) if error.contains("Ctrl+S"))
    );
}

#[test]
fn profile_link_import_keeps_the_form_open_and_fills_the_yaml_editor() {
    let mut app = app();
    app.form(
        "订阅配置",
        vec![
            Field::new("name", "名称", "", Kind::Text),
            Field::new("proxy", "下载代理", "", Kind::Text),
            Field::new(
                "url",
                "订阅文件链接（可直接导入）",
                "  https://example.com/profile.yaml  ",
                Kind::Text,
            ),
            Field::new("content", "订阅配置 YAML", "", Kind::Multiline),
        ],
        SaveTarget::Profile(None),
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    let import_area = app
        .hits
        .iter()
        .find_map(|(area, action)| matches!(action, Action::ImportProfile).then_some(*area))
        .expect("import button hit area");
    app.mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: import_area.x + import_area.width / 2,
        row: import_area.y,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    let request = match app.live.as_ref().unwrap().outbox.last().unwrap() {
        Command::ImportProfile {
            request,
            url,
            proxy,
        } => {
            assert_eq!(url, "https://example.com/profile.yaml");
            assert!(proxy.is_none());
            *request
        }
        command => panic!("unexpected command: {command:?}"),
    };
    assert!(!app.live.as_ref().unwrap().pending);
    app.handle_core(CoreEvent::ProfileImported {
        request,
        result: Ok(FetchedProfile {
            content: "proxies: [{name: Imported, type: direct}]\nrules: [MATCH,DIRECT]\n".into(),
            proxies: 1,
            groups: 0,
            user_info: None,
            suggested_name: "自动命名".into(),
        }),
    });
    assert!(!app.live.as_ref().unwrap().pending);
    assert!(app.status.contains("1 个节点"));
    let Modal::Form { fields, error, .. } = app.modal.as_ref().unwrap() else {
        panic!("import should keep the form open")
    };
    assert!(error.is_empty());
    assert!(fields
        .iter()
        .find(|field| field.key == "content")
        .unwrap()
        .value
        .contains("Imported"));
    assert_eq!(
        fields
            .iter()
            .find(|field| field.key == "name")
            .unwrap()
            .value,
        "自动命名"
    );
}

#[test]
fn profile_link_import_preserves_a_name_already_entered_by_the_user() {
    let mut app = app();
    app.form(
        "订阅配置",
        vec![
            Field::new(
                "url",
                "订阅文件链接",
                "https://example.com/profile.yaml",
                Kind::Text,
            ),
            Field::new("content", "订阅配置 YAML", "", Kind::Multiline),
            Field::new("name", "名称", "我的订阅", Kind::Text),
        ],
        SaveTarget::Profile(None),
    );
    app.action(Action::ImportProfile);
    let request = match app.live.as_ref().unwrap().outbox.last().unwrap() {
        Command::ImportProfile { request, .. } => *request,
        command => panic!("unexpected command: {command:?}"),
    };
    app.handle_core(CoreEvent::ProfileImported {
        request,
        result: Ok(FetchedProfile {
            content: "proxies: [{name: Imported, type: direct}]\n".into(),
            proxies: 1,
            groups: 0,
            user_info: None,
            suggested_name: "服务商名称".into(),
        }),
    });
    let Modal::Form { fields, .. } = app.modal.as_ref().unwrap() else {
        panic!("import should keep the form open")
    };
    assert_eq!(
        fields
            .iter()
            .find(|field| field.key == "name")
            .unwrap()
            .value,
        "我的订阅"
    );
}

#[test]
fn invalid_profile_link_stays_in_the_form_without_queuing_a_request() {
    let mut app = app();
    app.form(
        "订阅配置",
        vec![
            Field::new("url", "订阅文件链接", "not-a-url", Kind::Text),
            Field::new("content", "订阅配置 YAML", "", Kind::Multiline),
        ],
        SaveTarget::Profile(None),
    );
    let before = app.live.as_ref().unwrap().outbox.len();
    app.action(Action::ImportProfile);
    assert_eq!(app.live.as_ref().unwrap().outbox.len(), before);
    assert!(matches!(&app.modal, Some(Modal::Form { error, .. }) if error.contains("HTTP(S)")));
}

#[test]
fn live_profile_form_opens_with_the_link_and_import_button_above_yaml() {
    let mut app = app();
    app.live.as_mut().unwrap().managed = Some(ManagedSettings {
        controller: "127.0.0.1:9090".into(),
        secret: String::new(),
        port: 7890,
        binary: "/bin/true".into(),
    });
    app.navigate(Page::Profiles);
    app.command('a');
    let Modal::Form {
        title,
        fields,
        active,
        ..
    } = app.modal.as_ref().unwrap()
    else {
        panic!("expected live profile form")
    };
    assert!(title.contains("链接导入"));
    assert_eq!(*active, 0);
    assert_eq!(fields[0].key, "url");
    assert_eq!(fields[1].key, "content");
    if let Some(Modal::Form { fields, .. }) = &mut app.modal {
        let url = fields.iter_mut().find(|field| field.key == "url").unwrap();
        url.value = "https://example.com/minimum.yaml".into();
        url.cursor = url.value.len();
    }
    app.live.as_mut().unwrap().pending = true;
    let mut terminal = Terminal::new(TestBackend::new(76, 24)).unwrap();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    let import_area = app
        .hits
        .iter()
        .rev()
        .find_map(|(area, action)| matches!(action, Action::ImportProfile).then_some(*area))
        .expect("import button hit area");
    assert_eq!(import_area.height, 2);
    app.mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: import_area.right() - 1,
        row: import_area.bottom() - 1,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    let request = match app.live.as_ref().unwrap().outbox.last() {
        Some(Command::ImportProfile { request, url, .. }) => {
            assert_eq!(url, "https://example.com/minimum.yaml");
            *request
        }
        command => panic!("unexpected command: {command:?}"),
    };
    assert!(app.live.as_ref().unwrap().pending);
    app.handle_core(CoreEvent::Completed(Ok(())));
    app.handle_core(CoreEvent::ProfileImported {
        request,
        result: Ok(FetchedProfile {
            content: "proxies: [{name: Imported, type: direct}]\n".into(),
            proxies: 1,
            groups: 0,
            user_info: None,
            suggested_name: "小窗口订阅".into(),
        }),
    });
    let Modal::Form { fields, error, .. } = app.modal.as_ref().unwrap() else {
        panic!("import should keep the form open")
    };
    assert!(error.is_empty());
    assert_eq!(
        fields
            .iter()
            .find(|field| field.key == "name")
            .unwrap()
            .value,
        "小窗口订阅"
    );
}

#[test]
fn live_home_port_form_applies_with_plain_s_and_tun_requests_privilege_setup() {
    let mut app = app();
    app.live.as_mut().unwrap().managed = Some(ManagedSettings {
        controller: "127.0.0.1:9090".into(),
        secret: String::new(),
        port: 7890,
        binary: "/bin/true".into(),
    });
    app.selected = 3;
    app.activate();
    let Modal::Form { fields, .. } = app.modal.as_mut().unwrap() else {
        panic!("expected mixed port form")
    };
    fields[0].value = "7891".into();
    fields[0].cursor = 4;
    app.key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('s'),
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(matches!(
        app.live.as_ref().unwrap().outbox.last(),
        Some(Command::Workspace(clash_verge_tui::workspace::WorkspaceCommand::Settings(values)))
            if values.get("mixed_port").is_some_and(|value| value == "7891")
    ));

    app.handle_core(CoreEvent::Completed(Ok(())));
    app.selected = 1;
    app.activate();
    assert!(matches!(
        app.modal,
        Some(Modal::Confirm {
            target: clash_verge_tui::app::Confirm::TunService,
            ..
        })
    ));
    app.key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));
    let Modal::Form { fields, target, .. } = app.modal.as_mut().unwrap() else {
        panic!("expected in-TUI password form")
    };
    assert!(matches!(target, SaveTarget::TunServicePassword));
    fields[0].value = "fixture-password".into();
    fields[0].cursor = fields[0].value.len();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    let mut screen = String::new();
    for cell in terminal.backend().buffer().content() {
        screen.push_str(cell.symbol());
    }
    assert!(!screen.contains("fixture-password"));
    assert!(screen.contains("••••"));
    app.key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(matches!(
        app.live.as_ref().unwrap().outbox.last(),
        Some(Command::InstallTunService { password })
            if password.expose() == "fixture-password"
    ));
    assert!(!format!("{:?}", app.live.as_ref().unwrap().outbox.last()).contains("fixture-password"));
    app.handle_core(CoreEvent::TunServiceInstalled(Ok(())));
    assert!(app.restart_core);
    app.resume_tun_after_restart();
    assert!(matches!(
        app.live.as_ref().unwrap().outbox.last(),
        Some(Command::Workspace(clash_verge_tui::workspace::WorkspaceCommand::Settings(values)))
            if values.get("tun").is_some_and(|value| value == "开启")
    ));
}
