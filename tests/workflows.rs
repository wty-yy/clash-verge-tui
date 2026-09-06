use clash_verge_tui::{
    app::{Action, App, Field, Modal},
    model::{DemoState, Page},
    settings::{self, Kind},
    storage, ui,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};
use std::path::PathBuf;
fn app() -> App {
    App::new(DemoState::default(), PathBuf::from("/tmp/clash-verge-test"))
}
fn key(app: &mut App, code: KeyCode) {
    app.key(KeyEvent::new(code, KeyModifiers::NONE));
}
fn save(app: &mut App) {
    app.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
}
fn set(app: &mut App, name: &str, value: &str) {
    if let Some(Modal::Form { fields, .. }) = &mut app.modal {
        let f = fields.iter_mut().find(|f| f.key == name).unwrap();
        f.value = value.into();
        f.cursor = value.len();
    } else {
        panic!("expected form");
    }
}
fn render(app: &mut App, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| ui::draw(f, app)).unwrap();
    let buffer = t.backend().buffer();
    let mut text = String::new();
    for y in 0..h {
        let mut skip = 0;
        for x in 0..w {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let symbol = buffer[(x, y)].symbol();
            text.push_str(symbol);
            skip = unicode_width::UnicodeWidthStr::width(symbol).saturating_sub(1);
        }
        text.push('\n');
    }
    text
}
#[test]
fn every_page_and_tab_renders_at_supported_sizes_and_in_both_themes() {
    for theme in ["深色", "浅色"] {
        for (w, h) in [(76, 24), (100, 30), (120, 40), (180, 52)] {
            for page in Page::ALL {
                let mut a = app();
                a.state.settings.insert("theme".into(), theme.into());
                a.navigate(page);
                for sub in 0..a.tabs().len().max(1) {
                    a.sub = sub;
                    let s = render(&mut a, w, h);
                    assert!(s.contains("演示工作区"), "{page:?} {w} {h}");
                    assert!(s.contains(page.title()));
                }
            }
        }
    }
}
#[test]
fn undersized_terminal_is_actionable() {
    let mut a = app();
    assert!(render(&mut a, 60, 18).contains("76 × 24"));
    key(&mut a, KeyCode::Char('q'));
    assert!(a.quit);
}
#[test]
fn all_settings_forms_scroll_to_every_field_and_save_defaults() {
    for (i, s) in settings::sections().iter().enumerate() {
        let mut a = app();
        a.navigate(Page::Settings);
        a.sub = s.category;
        a.selected = a.rows().iter().position(|r| r.id == i).unwrap();
        a.activate();
        if matches!(a.modal, Some(Modal::Backups { .. })) {
            key(&mut a, KeyCode::Char('e'));
        }
        for size in [(76, 24), (120, 40)] {
            if let Some(Modal::Form { fields, .. }) = &a.modal {
                let count = fields.len();
                for active in 0..count {
                    if let Some(Modal::Form { active: x, .. }) = &mut a.modal {
                        *x = active;
                    }
                    let screen = render(&mut a, size.0, size.1);
                    assert!(
                        screen.contains(s.fields[active].label),
                        "{} field {}",
                        s.name,
                        active
                    );
                }
            } else {
                render(&mut a, size.0, size.1);
            }
        }
        if !s.fields.is_empty() {
            save(&mut a);
            assert!(a.modal.is_none(), "{} default form failed", s.name);
        }
    }
}
#[test]
fn filtered_and_sorted_proxy_selection_uses_underlying_node() {
    let mut a = app();
    a.navigate(Page::Proxies);
    a.query = "日本".into();
    a.sort = true;
    a.selected = 1;
    let name = a.state.nodes[a.selected_id().unwrap()].name.clone();
    a.activate();
    assert_eq!(a.state.groups[0].selected, name);
    a.query = "absent".into();
    a.activate();
    assert_eq!(a.state.groups[0].selected, name);
}
#[test]
fn profile_crud_validation_cancel_reorder_and_empty_state() {
    let mut a = app();
    a.navigate(Page::Profiles);
    a.command('a');
    save(&mut a);
    assert!(matches!(a.modal, Some(Modal::Form { .. })));
    set(&mut a, "name", "测试订阅");
    set(&mut a, "url", "https://");
    save(&mut a);
    assert!(a.modal.is_some());
    set(&mut a, "url", "https://example.com/profile");
    set(&mut a, "interval", "0");
    save(&mut a);
    assert!(a.modal.is_some());
    set(&mut a, "interval", "60");
    save(&mut a);
    assert_eq!(a.state.profiles.len(), 3);
    a.selected = 2;
    a.activate();
    a.command('[');
    assert_eq!(a.state.active_name(), "测试订阅");
    assert_eq!(a.state.active_profile, 1);
    a.command('e');
    set(&mut a, "name", "取消的修改");
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.state.active_name(), "测试订阅");
    a.command('e');
    set(&mut a, "name", "重命名");
    save(&mut a);
    assert_eq!(a.state.active_name(), "重命名");
    a.query = "备用".into();
    a.selected = 0;
    a.command('d');
    key(&mut a, KeyCode::Enter);
    assert!(!a.state.profiles.iter().any(|p| p.name == "备用线路"));
    a.query.clear();
    while !a.state.profiles.is_empty() {
        a.selected = 0;
        a.command('d');
        key(&mut a, KeyCode::Enter);
    }
    assert_eq!(a.state.active_name(), "未选择订阅");
    a.navigate(Page::Home);
    render(&mut a, 76, 24);
}
#[test]
fn chinese_text_editing_respects_utf8_and_paste_is_sanitized() {
    let mut f = Field::new("name", "名称", "香港节点", Kind::Text);
    f.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    f.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    f.insert("测试");
    assert_eq!(f.value, "香港测试点");
    f.key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    f.key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
    f.insert("\x1b[31m\n");
    assert!(!f.value.contains('\x1b'));
    assert!(!f.value.contains('\n'));
    let mut f = Field::new("content", "脚本", "", Kind::Multiline);
    f.insert("中文\r\n测试");
    assert_eq!(f.value, "中文\n测试");
}
#[test]
fn enhancement_and_rule_workflows() {
    let mut a = app();
    a.navigate(Page::Profiles);
    a.sub = 1;
    a.command('a');
    set(&mut a, "name", "增强");
    set(&mut a, "content", "mode: rule\n");
    save(&mut a);
    assert_eq!(a.state.enhancements.len(), 3);
    a.selected = 2;
    a.command('[');
    assert_eq!(a.state.enhancements[1].name, "增强");
    a.activate();
    assert!(!a.state.enhancements[1].enabled);
    a.navigate(Page::Rules);
    a.command('a');
    set(&mut a, "payload", "example.org");
    save(&mut a);
    assert_eq!(a.state.rules.last().unwrap().payload, "example.org");
    a.query = "example.org".into();
    a.command('e');
    set(&mut a, "payload", "example.net");
    save(&mut a);
    assert_eq!(a.state.rules.last().unwrap().payload, "example.net");
    a.query = "example.net".into();
    a.activate();
    assert!(!a.state.rules.last().unwrap().enabled);
    a.command('d');
    key(&mut a, KeyCode::Enter);
    assert!(!a.state.rules.iter().any(|r| r.payload == "example.net"));
}
#[test]
fn connections_confirmations_logs_pause_and_unlock_filters() {
    let mut a = app();
    a.navigate(Page::Connections);
    a.query = "youtube".into();
    a.command('d');
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.state.connections.len(), 6);
    a.command('d');
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.state.connections.len(), 5);
    a.command('D');
    key(&mut a, KeyCode::Enter);
    assert!(a.state.connections.is_empty());
    a.command('r');
    assert_eq!(a.state.connections.len(), 6);
    a.navigate(Page::Logs);
    a.command('p');
    let n = a.state.logs.len();
    for _ in 0..20 {
        a.tick();
    }
    assert_eq!(a.state.logs.len(), n);
    a.command('p');
    for _ in 0..10 {
        a.tick();
    }
    assert!(a.state.logs.len() > n);
    a.navigate(Page::Unlock);
    a.sub = 2;
    a.command('r');
    assert!(a
        .state
        .unlocks
        .iter()
        .filter(|u| u.category == "AI")
        .all(|u| u.result.contains("演示")));
    assert!(a
        .state
        .unlocks
        .iter()
        .filter(|u| u.category != "AI")
        .all(|u| u.result == "未检测"));
}
#[test]
fn backups_restore_and_persist_without_reading_live_verge() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = app();
    a.navigate(Page::Settings);
    a.command('b');
    a.state.profiles.clear();
    a.state.toggle("tun");
    a.command('R');
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.state.profiles.len(), 2);
    assert_eq!(a.state.value("tun"), "关闭");
    storage::save(dir.path(), &a.state).unwrap();
    let b = storage::load(dir.path()).unwrap();
    assert_eq!(b.backups.len(), 1);
    assert_eq!(b.profiles.len(), 2);
    assert!(!dir.path().join("demo-state.json.tmp").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(dir.path().join("demo-state.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    std::fs::write(dir.path().join("demo-state.json"), "broken").unwrap();
    assert!(storage::load(dir.path()).is_err());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("demo-state.json")).unwrap(),
        "broken"
    );
}
#[test]
fn invalid_ports_are_rejected_and_escape_never_saves() {
    let mut a = app();
    a.navigate(Page::Settings);
    a.sub = 1;
    a.selected = 1;
    a.activate();
    set(&mut a, "mixed_port", "70000");
    save(&mut a);
    assert!(a.modal.is_some());
    assert_eq!(a.state.value("mixed_port"), "7897");
    set(&mut a, "mixed_port", "7890");
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.state.value("mixed_port"), "7897");
}
#[test]
fn palette_mouse_navigation_closes_overlay_and_search_does_not_quit() {
    let mut a = app();
    key(&mut a, KeyCode::Char(':'));
    render(&mut a, 120, 40);
    a.action(Action::Page(Page::Logs));
    assert!(a.modal.is_none());
    assert_eq!(a.page, Page::Logs);
    key(&mut a, KeyCode::Char('/'));
    key(&mut a, KeyCode::Char('q'));
    assert!(!a.quit);
    assert_eq!(a.query, "q");
    key(&mut a, KeyCode::Esc);
    assert!(a.query.is_empty());
}
#[test]
fn home_selection_and_active_tab_remain_visible_in_small_terminal() {
    let mut a = app();
    a.selected = 5;
    assert!(render(&mut a, 76, 24).contains("环境变量"));
    a.navigate(Page::Proxies);
    a.sub = 3;
    let s = render(&mut a, 76, 24);
    assert!(s.contains("AI 服务"));
    assert!(a.hits.iter().any(|(_, x)| matches!(x, Action::Sub(3))));
}
#[test]
fn empty_searches_and_resizes_do_not_break_keyboard_paths() {
    let mut a = app();
    for p in Page::ALL {
        a.navigate(p);
        a.query = "不存在的示例".into();
        for c in ['r', 'e', 'v', 'd', 's', '[', ']'] {
            a.command(c);
            a.modal = None;
        }
        key(&mut a, KeyCode::End);
        a.activate();
        render(&mut a, 76, 24);
        render(&mut a, 180, 52);
    }
}

#[test]
fn backup_history_supports_selection_restore_delete_and_webdav_form() {
    let mut a = app();
    a.navigate(Page::Settings);
    a.sub = 3;
    a.command('b');
    a.state.profiles[0].name = "第二版".into();
    a.command('b');
    a.selected = 0;
    a.activate();
    assert!(matches!(a.modal, Some(Modal::Backups { .. })));
    render(&mut a, 76, 24);
    key(&mut a, KeyCode::Up);
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.state.active_name(), "日常订阅");
    a.activate();
    key(&mut a, KeyCode::Char('d'));
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.state.backups.len(), 1);
    a.activate();
    key(&mut a, KeyCode::Char('e'));
    set(&mut a, "webdav_user", "demo-user");
    save(&mut a);
    assert_eq!(a.state.value("webdav_user"), "demo-user");
    for _ in 0..12 {
        a.command('b');
    }
    assert_eq!(a.state.backups.len(), 10);
    let names: std::collections::HashSet<_> = a.state.backups.iter().map(|b| &b.name).collect();
    assert_eq!(names.len(), 10);
}
