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
    assert!(render(&mut a, 120, 40).contains("a 链接导入"));
    a.command('a');
    if let Some(Modal::Form { fields, .. }) = &a.modal {
        assert_eq!(fields[0].key, "url");
        assert_eq!(fields[1].key, "content");
    } else {
        panic!("expected profile import form");
    }
    let form = render(&mut a, 120, 40);
    assert!(form.contains("订阅文件链接"));
    assert!(form.contains("导入"));
    assert!(a
        .hits
        .iter()
        .any(|(_, action)| matches!(action, Action::ImportProfile)));
    if let Some(Modal::Form { active, .. }) = &mut a.modal {
        *active = 0;
    }
    let compact = render(&mut a, 76, 24);
    assert!(compact.contains("订阅文件链接"));
    assert!(compact.contains("导入"));
    a.action(Action::ImportProfile);
    assert!(matches!(&a.modal, Some(Modal::Form { error, .. }) if error.contains("演示模式")));
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
    assert_eq!(a.state.value("mixed_port"), "7890");
    set(&mut a, "mixed_port", "7891");
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.state.value("mixed_port"), "7890");
}
#[test]
fn home_mixed_port_uses_7890_and_plain_s_applies_the_quick_form() {
    let mut app = app();
    let row = &app.rows()[3];
    assert_eq!(row.cells, ["混合代理端口", "7890"]);
    app.selected = 3;
    assert!(render(&mut app, 76, 24).contains("混合代理端口"));
    app.activate();
    assert!(matches!(app.modal, Some(Modal::Form { .. })));
    set(&mut app, "mixed_port", "7891");
    let form = render(&mut app, 76, 24);
    assert!(form.contains("s 保存并应用"));
    key(&mut app, KeyCode::Char('s'));
    assert!(app.modal.is_none());
    assert_eq!(app.state.value("mixed_port"), "7891");
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
    a.selected = 6;
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

#[test]
fn traffic_chart_fills_width_and_scales_height_after_terminal_resizes() {
    let mut a = app();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut heights = Vec::new();
    for (width, height) in [(120, 40), (180, 52), (76, 24), (220, 70), (120, 40)] {
        terminal.backend_mut().resize(width, height);
        terminal
            .resize(ratatui::layout::Rect::new(0, 0, width, height))
            .unwrap();
        terminal.draw(|f| ui::draw(f, &mut a)).unwrap();
        let buffer = terminal.backend().buffer();
        let left = if width < 100 { 21 } else { 27 };
        let label_y = (0..height)
            .find(|&y| buffer[(left, y)].symbol() == "-" && buffer[(left + 1, y)].symbol() == "6")
            .unwrap();
        let title_y = (0..label_y)
            .find(|&y| buffer[(left, y)].symbol() == "流")
            .unwrap();
        heights.push(label_y - title_y);
        for x in left..width - 4 {
            let symbol = buffer[(x, label_y - 1)].symbol();
            assert!(
                symbol
                    .chars()
                    .any(|c| ('\u{2581}'..='\u{2588}').contains(&c)),
                "unfilled chart at {x} in {width}x{height}: {symbol:?}"
            );
        }
        assert_eq!(
            buffer[(width - 6, label_y)].symbol(),
            "载",
            "axis end should align to chart edge"
        );
    }
    assert!(heights[1] > heights[0]);
    assert!(heights[3] > heights[1]);
    assert_eq!(heights[0], heights[4]);
    assert_eq!(a.tick, 0, "resizing must not advance history");
}

#[test]
fn checkbox_markers_are_consistent_in_forms_and_selection_tables() {
    let mut a = app();
    a.state
        .settings
        .insert("system_proxy".into(), "开启".into());
    a.navigate(Page::Settings);
    a.activate();
    let screen = render(&mut a, 120, 40);
    assert!(screen.contains("[✓]  开启"));
    assert!(!screen.contains("[●]"));
    key(&mut a, KeyCode::Char(' '));
    assert!(render(&mut a, 120, 40).contains("[ ]  关闭"));
    key(&mut a, KeyCode::Esc);
    for page in [Page::Proxies, Page::Profiles, Page::Rules] {
        a.navigate(page);
        let screen = render(&mut a, 120, 40);
        assert!(screen.contains("[✓]"), "{page:?}");
        assert!(!screen.contains("[●]"));
    }
    a.navigate(Page::Profiles);
    assert!(render(&mut a, 120, 40).contains("[✓] 当前"));
}

#[test]
fn vim_navigation_hint_tracks_enabled_setting_and_keyboard_behavior() {
    let mut a = app();
    for size in [(76, 24), (120, 40)] {
        let screen = render(&mut a, size.0, size.1);
        assert!(screen.lines().any(|line| line.contains("j/k 选择")));
    }
    key(&mut a, KeyCode::Char('j'));
    assert_eq!(a.selected, 1);
    key(&mut a, KeyCode::Char('k'));
    assert_eq!(a.selected, 0);
    a.state.settings.insert("vim".into(), "关闭".into());
    assert!(!render(&mut a, 120, 40).contains("j/k"));
    key(&mut a, KeyCode::Char('j'));
    assert_eq!(a.selected, 0);
    key(&mut a, KeyCode::Down);
    assert_eq!(a.selected, 1);
}

fn click_row(a: &mut App, index: usize, backup: bool) {
    render(a, 120, 40);
    let (rect, _) = a
        .hits
        .iter()
        .find(|(_, action)| match action {
            Action::Select(i) => !backup && *i == index,
            Action::BackupSelect(i) => backup && *i == index,
            _ => false,
        })
        .unwrap();
    let event = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: rect.x + 3,
        row: rect.y,
        modifiers: KeyModifiers::NONE,
    };
    a.mouse(event);
    a.mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
        ..event
    });
}

#[test]
fn double_click_matches_enter_across_every_main_page_and_section() {
    for page in Page::ALL {
        let count = {
            let mut a = app();
            a.navigate(page);
            a.tabs().len().max(1)
        };
        for sub in 0..count {
            let mut mouse = app();
            mouse.navigate(page);
            mouse.sub = sub;
            let before = serde_json::to_value(&mouse.state).unwrap();
            click_row(&mut mouse, 0, false);
            assert_eq!(
                serde_json::to_value(&mouse.state).unwrap(),
                before,
                "single click mutated {page:?}"
            );
            assert!(mouse.modal.is_none());
            click_row(&mut mouse, 0, false);
            let mut keyboard = app();
            keyboard.navigate(page);
            keyboard.sub = sub;
            key(&mut keyboard, KeyCode::Enter);
            assert_eq!(
                serde_json::to_value(&mouse.state).unwrap(),
                serde_json::to_value(&keyboard.state).unwrap(),
                "{page:?} section {sub}"
            );
            assert_eq!(
                render(&mut mouse, 120, 40),
                render(&mut keyboard, 120, 40),
                "{page:?} section {sub}"
            );
        }
    }
}

#[test]
fn double_click_backup_opens_confirmation_without_restoring_immediately() {
    let mut a = app();
    a.navigate(Page::Settings);
    a.sub = 3;
    a.command('b');
    a.state.profiles[0].name = "未恢复".into();
    a.activate();
    click_row(&mut a, 0, true);
    assert!(matches!(a.modal, Some(Modal::Backups { .. })));
    click_row(&mut a, 0, true);
    assert!(matches!(a.modal, Some(Modal::Confirm { .. })));
    assert_eq!(a.state.active_name(), "未恢复");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.state.active_name(), "日常订阅");
}

#[test]
fn double_click_respects_filtered_and_sorted_row_identity() {
    let mut a = app();
    a.navigate(Page::Proxies);
    a.query = "日本".into();
    a.sort = true;
    let expected = a.state.nodes[a.rows()[1].id].name.clone();
    click_row(&mut a, 1, false);
    click_row(&mut a, 1, false);
    assert_eq!(a.state.groups[0].selected, expected);
}

#[test]
fn home_horizontal_focus_preserves_selection_and_activates_profile_card() {
    use clash_verge_tui::app::HomeFocus;
    let mut a = app();
    a.selected = 2;
    key(&mut a, KeyCode::Right);
    assert_eq!(a.home_focus, HomeFocus::Profile);
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Down);
    assert_eq!(a.selected, 2);
    for (w, h) in [(76, 24), (120, 40)] {
        assert!(render(&mut a, w, h).contains("› 进入订阅管理 ↵"));
    }
    key(&mut a, KeyCode::Left);
    assert_eq!(a.home_focus, HomeFocus::Controls);
    assert_eq!(a.selected, 2);
    key(&mut a, KeyCode::Tab);
    assert_eq!(a.home_focus, HomeFocus::Profile);
    key(&mut a, KeyCode::BackTab);
    assert_eq!(a.home_focus, HomeFocus::Controls);
    key(&mut a, KeyCode::Char('l'));
    key(&mut a, KeyCode::Char('h'));
    assert_eq!(a.home_focus, HomeFocus::Controls);
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.page, Page::Profiles);
    assert_eq!(a.state.mode, 0);
}

#[test]
fn horizontal_keys_cycle_all_tab_groups_and_filter_logs() {
    for page in Page::ALL {
        let mut a = app();
        a.navigate(page);
        let count = a.tabs().len();
        if count == 0 {
            continue;
        }
        for expected in (1..count).chain(std::iter::once(0)) {
            key(&mut a, KeyCode::Right);
            assert_eq!(a.sub, expected, "{page:?}");
        }
        key(&mut a, KeyCode::Left);
        assert_eq!(a.sub, count - 1);
        key(&mut a, KeyCode::Char('l'));
        assert_eq!(a.sub, 0);
        a.state.settings.insert("vim".into(), "关闭".into());
        key(&mut a, KeyCode::Char('l'));
        assert_eq!(a.sub, 0);
        key(&mut a, KeyCode::Right);
        assert_eq!(a.sub, 1 % count);
    }
    let mut a = app();
    a.navigate(Page::Logs);
    key(&mut a, KeyCode::Right);
    assert!(a.rows().iter().all(|r| r.cells[1] == "INFO"));
    key(&mut a, KeyCode::Right);
    assert!(a.rows().iter().all(|r| r.cells[1] == "DEBUG"));
    key(&mut a, KeyCode::Left);
    assert!(a.rows().iter().all(|r| r.cells[1] == "INFO"));
    a.navigate(Page::Profiles);
    a.command('a');
    key(&mut a, KeyCode::Char('h'));
    key(&mut a, KeyCode::Char('l'));
    if let Some(Modal::Form { fields, .. }) = &a.modal {
        assert!(fields[0].value.ends_with("hl"));
    } else {
        panic!("expected form");
    }
}

#[test]
fn home_mouse_selection_and_double_click_cooperate_with_keyboard_focus() {
    use clash_verge_tui::app::HomeFocus;
    let mut a = app();
    key(&mut a, KeyCode::Right);
    click_row(&mut a, 0, false);
    assert_eq!(a.home_focus, HomeFocus::Controls);
    assert_eq!(a.state.value("system_proxy"), "关闭");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.state.value("system_proxy"), "开启");
    render(&mut a, 120, 40);
    let (rect, _) = a
        .hits
        .iter()
        .find(|(_, action)| matches!(action, Action::HomeFocus(HomeFocus::Profile)))
        .unwrap();
    let event = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: rect.x + 3,
        row: rect.y + 1,
        modifiers: KeyModifiers::NONE,
    };
    a.mouse(event);
    assert_eq!(a.home_focus, HomeFocus::Profile);
    assert_eq!(a.page, Page::Home);
    render(&mut a, 120, 40);
    a.mouse(event);
    assert_eq!(a.page, Page::Profiles);
}

#[test]
fn home_profile_button_selects_before_opening_without_a_double_click_deadline() {
    use clash_verge_tui::app::HomeFocus;
    for (width, height) in [(76, 24), (120, 40)] {
        let mut a = app();
        render(&mut a, width, height);
        let (rect, _) = a
            .hits
            .iter()
            .find(|(_, action)| matches!(action, Action::ProfileButton))
            .unwrap();
        let event = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: rect.x + 3,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        };
        a.mouse(event);
        assert_eq!(a.page, Page::Home);
        assert_eq!(a.home_focus, HomeFocus::Profile);
        assert!(render(&mut a, width, height).contains("› 进入订阅管理 ↵"));
        key(&mut a, KeyCode::Left);
        a.mouse(event);
        assert_eq!(a.page, Page::Home);
        assert_eq!(a.home_focus, HomeFocus::Profile);
        // The second click relies on focus, not on the transient double-click record.
        a.cancel_pending_click();
        render(&mut a, width, height);
        a.mouse(event);
        assert_eq!(a.page, Page::Profiles);
        a.navigate(Page::Home);
        key(&mut a, KeyCode::Right);
        render(&mut a, width, height);
        a.mouse(event);
        assert_eq!(a.page, Page::Profiles);
    }
}

#[test]
fn sidebar_highlight_and_click_targets_include_padding_without_overlapping() {
    for (width, height) in [
        (76, 24),
        (100, 30),
        (120, 32),
        (120, 34),
        (120, 40),
        (180, 52),
    ] {
        let mut a = app();
        render(&mut a, width, height);
        let items: Vec<_> = a
            .hits
            .iter()
            .filter_map(|(rect, action)| {
                if let Action::Page(page) = action {
                    Some((*rect, *page))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(items.len(), 8);
        for adjacent in items.windows(2) {
            assert!(adjacent[0].0.bottom() <= adjacent[1].0.y);
        }
        for (rect, page) in items {
            assert_eq!(rect.height, if height >= 32 { 3 } else { 2 });
            assert!(rect.bottom() <= height);
            for y in rect.y..rect.bottom() {
                for x in [rect.x, rect.right() - 1] {
                    a.mouse(crossterm::event::MouseEvent {
                        kind: crossterm::event::MouseEventKind::Down(
                            crossterm::event::MouseButton::Left,
                        ),
                        column: x,
                        row: y,
                        modifiers: KeyModifiers::NONE,
                    });
                    assert_eq!(a.page, page);
                }
            }
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| ui::draw(f, &mut a)).unwrap();
            let buffer = terminal.backend().buffer();
            assert_eq!(
                buffer[(rect.x + 1, rect.y)].bg,
                buffer[(rect.x + 1, rect.bottom() - 1)].bg
            );
            assert_ne!(buffer[(rect.x + 1, rect.y)].bg, buffer[(0, rect.y)].bg);
            assert_eq!(
                buffer[(rect.x, rect.y)].symbol(),
                if rect.height == 3 { "╭" } else { "│" }
            );
        }
    }
}

#[test]
fn page_tables_use_consecutive_rows_including_after_scrolling() {
    for page in Page::ALL.into_iter().skip(1) {
        let mut a = app();
        a.navigate(page);
        for sub in 0..a.tabs().len().max(1) {
            a.sub = sub;
            for (w, h) in [(76, 24), (120, 40)] {
                a.selected = a.rows().len().saturating_sub(1);
                render(&mut a, w, h);
                let targets: Vec<_> = a
                    .hits
                    .iter()
                    .filter_map(|(r, action)| {
                        if let Action::Select(i) = action {
                            Some((*r, *i))
                        } else {
                            None
                        }
                    })
                    .collect();
                assert!(!targets.is_empty(), "{page:?} {sub}");
                for pair in targets.windows(2) {
                    assert_eq!(pair[0].0.bottom(), pair[1].0.y, "blank row in {page:?}");
                    assert_eq!(pair[0].1 + 1, pair[1].1);
                }
                assert!(targets.iter().any(|(_, i)| *i == a.selected));
            }
        }
    }
    let mut a = app();
    a.navigate(Page::Logs);
    a.state.logs = (0..40)
        .map(|i| clash_verge_tui::model::Log {
            time: format!("01:00:{i:02}"),
            level: "INFO".into(),
            message: format!("第 {i} 条日志"),
        })
        .collect();
    a.selected = 39;
    let screen = render(&mut a, 76, 24);
    assert!(a.table_offset > 0);
    let targets: Vec<_> = a
        .hits
        .iter()
        .filter_map(|(r, action)| {
            if let Action::Select(i) = action {
                Some((*r, *i))
            } else {
                None
            }
        })
        .collect();
    for (r, i) in targets {
        assert!(screen
            .lines()
            .nth(r.y as usize)
            .unwrap()
            .contains(&a.state.logs[i].time));
        a.mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: r.x + 3,
            row: r.y,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(a.selected, i);
    }
}

#[test]
fn secondary_lists_and_form_fields_do_not_have_spacer_rows() {
    let mut a = app();
    a.navigate(Page::Settings);
    a.activate();
    render(&mut a, 120, 40);
    let fields: Vec<_> = a
        .hits
        .iter()
        .filter_map(|(r, action)| {
            if matches!(action, Action::Field(_)) {
                Some(*r)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(fields[0].height, 2);
    for pair in fields.windows(2) {
        assert_eq!(pair[0].bottom(), pair[1].y);
    }
    key(&mut a, KeyCode::Esc);
    a.sub = 3;
    for _ in 0..10 {
        a.command('b');
    }
    a.activate();
    render(&mut a, 120, 40);
    let backups: Vec<_> = a
        .hits
        .iter()
        .filter_map(|(r, action)| {
            if matches!(action, Action::BackupSelect(_)) {
                Some(*r)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(backups.len(), 10);
    for pair in backups.windows(2) {
        assert_eq!(pair[0].bottom(), pair[1].y);
    }
    key(&mut a, KeyCode::Esc);
    key(&mut a, KeyCode::Char(':'));
    render(&mut a, 76, 24);
    let pages: Vec<_> = a
        .hits
        .iter()
        .filter_map(|(r, action)| {
            if matches!(action, Action::Page(_)) {
                Some(*r)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(pages.len(), 8);
    for pair in pages.windows(2) {
        assert_eq!(pair[0].bottom(), pair[1].y);
    }
}

#[test]
fn multiline_editor_handles_large_profiles_and_vertical_navigation() {
    let content = format!("{}\n中文行\nlast", "a".repeat(100000));
    let mut field = Field::new("content", "YAML", &content, Kind::Multiline);
    field.insert("!");
    assert!(field.value.ends_with("last!"));
    field.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    field.key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    field.insert("前");
    assert!(field.value.contains("\n前中文行\n"));
    field.key(KeyEvent::new(KeyCode::Home, KeyModifiers::CONTROL));
    assert_eq!(field.cursor, 0);
    field.key(KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL));
    assert_eq!(field.cursor, field.value.len());
}
