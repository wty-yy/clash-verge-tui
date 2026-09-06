#[path = "support/fixtures.rs"]
mod fixtures;
use clash_verge_tui::{
    app::App,
    core::{Command, CoreEvent, Snapshot},
    live::LiveState,
    model::*,
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
    assert!(a.status.contains("自动选择"));
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
