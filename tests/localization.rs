use clash_verge_tui::{
    app::{Action, App, Modal},
    live::LiveState,
    locale::{self, Language},
    model::{DemoState, Page},
    settings, storage, ui,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};
use std::process::Command;

fn app(language: &str) -> App {
    let mut state = DemoState::default();
    state.settings.insert("language".into(), language.into());
    App::new(state, "/tmp/localization-fixture".into())
}
fn render(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| ui::draw(f, app)).unwrap();
    let mut text = String::new();
    for y in 0..height {
        let mut x = 0;
        while x < width {
            let symbol = terminal.backend().buffer()[(x, y)].symbol();
            text.push_str(symbol);
            x += unicode_width::UnicodeWidthStr::width(symbol).max(1) as u16;
        }
        text.push('\n');
    }
    text
}
fn appearance(app: &mut App) {
    app.navigate(Page::Settings);
    app.sub = 2;
    let id = settings::sections()
        .iter()
        .position(|section| section.name == "外观与布局")
        .unwrap();
    app.selected = app.rows().iter().position(|row| row.id == id).unwrap();
    app.activate();
}
#[test]
fn locale_detection_covers_scripts_regions_and_precedence() {
    for code in ["zh_CN.UTF-8", "zh-SG", "zh-Hans-TW", "zh"] {
        // An explicit script takes precedence over the region.
        assert_eq!(
            Language::from_locale(code),
            Language::SimplifiedChinese,
            "{code}"
        );
    }
    for code in ["zh_TW.UTF-8", "zh_HK", "zh_MO", "zh-Hant-CN", "zh-TW:en"] {
        assert_eq!(
            Language::from_locale(code),
            Language::TraditionalChinese,
            "{code}"
        );
    }
    for code in ["C", "C.UTF-8", "POSIX", "en_US.UTF-8", "de_DE", ""] {
        assert_eq!(Language::from_locale(code), Language::English);
    }
    let vars = [
        ("LC_ALL", "zh_TW.UTF-8"),
        ("LC_MESSAGES", "en_US"),
        ("LANGUAGE", "zh_CN:en"),
        ("LANG", "en_US"),
    ];
    for (skip, expected) in [
        (0, Language::TraditionalChinese),
        (1, Language::English),
        (2, Language::SimplifiedChinese),
        (3, Language::English),
    ] {
        assert_eq!(
            Language::from_environment(|key| vars[skip..]
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())),
            expected
        );
    }
}
#[test]
fn every_page_tab_and_form_supports_all_languages_and_sizes() {
    for language in ["en", "zh-CN", "zh-TW"] {
        for (width, height) in [(76, 24), (120, 40)] {
            let mut app = app(language);
            for page in Page::ALL {
                app.navigate(page);
                for sub in 0..app.tabs().len().max(1) {
                    app.sub = sub;
                    let screen = render(&mut app, width, height);
                    assert!(
                        screen.contains(&page.localized_title(app.language)),
                        "{language} {page:?} {width}"
                    );
                    assert_eq!(
                        app.hits
                            .iter()
                            .filter(|(_, action)| matches!(action, Action::Page(_)))
                            .count(),
                        8
                    );
                }
            }
            for (id, section) in settings::sections()
                .iter()
                .enumerate()
                .filter(|(_, s)| !s.fields.is_empty())
            {
                app.navigate(Page::Settings);
                app.sub = section.category;
                app.selected = app.rows().iter().position(|row| row.id == id).unwrap();
                app.activate();
                if matches!(app.modal, Some(Modal::Backups { .. })) {
                    app.command('e');
                }
                if let Some(Modal::Form { fields, .. }) = &app.modal {
                    let n = fields.len();
                    for active in 0..n {
                        if let Some(Modal::Form { active: index, .. }) = &mut app.modal {
                            *index = active;
                        }
                        render(&mut app, width, height);
                        assert!(app
                            .hits
                            .iter()
                            .any(|(_, action)| matches!(action,Action::Field(i) if *i==active)));
                    }
                }
                app.modal = None;
            }
        }
    }
}
#[test]
fn language_switch_is_immediate_persistent_and_offline_safe() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app("zh-CN");
    app.data_dir = dir.path().into();
    appearance(&mut app);
    if let Some(Modal::Form { fields, active, .. }) = &mut app.modal {
        assert_eq!(fields[0].key, "language");
        *active = 0;
        fields[0].value = "en".into();
    } else {
        panic!("appearance form");
    }
    app.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    assert_eq!(app.language, Language::English);
    assert!(render(&mut app, 120, 40).contains("Appearance and layout"));
    storage::save(dir.path(), &app.state).unwrap();
    let restored = App::new(storage::load(dir.path()).unwrap(), dir.path().into());
    assert_eq!(restored.language, Language::English);
    app.live = Some(LiveState::new("http://127.0.0.1:1".into(), vec![], None));
    app.live.as_mut().unwrap().pending = true;
    appearance(&mut app);
    if let Some(Modal::Form { fields, .. }) = &mut app.modal {
        fields[0].value = "zh-TW".into();
    }
    app.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    assert_eq!(app.language, Language::TraditionalChinese);
    assert!(app.live.as_ref().unwrap().outbox.is_empty());
    app.save_preferences().unwrap();
    let reopened = App::new_live(
        dir.path().into(),
        "http://127.0.0.1:1".into(),
        vec![],
        0,
        None,
    )
    .unwrap();
    assert_eq!(reopened.language, Language::TraditionalChinese);
}
#[test]
fn user_content_is_unchanged_and_import_button_remains_clickable() {
    let mut app = app("en");
    let name = "日常订阅 开启 设置";
    app.state.profiles[0].name = name.into();
    assert!(render(&mut app, 120, 40).contains(name));
    app.navigate(Page::Profiles);
    assert!(render(&mut app, 120, 40).contains(name));
    app.command('a');
    for (w, h) in [(76, 24), (120, 40)] {
        let screen = render(&mut app, w, h);
        assert!(screen.contains("[ Import ]"));
        assert!(screen.contains("Profile file URL"));
        let (area, action) = app
            .hits
            .iter()
            .find(|(_, a)| matches!(a, Action::ImportProfile))
            .unwrap()
            .clone();
        assert!(area.width >= 10 && area.height == 2);
        app.action(action);
        assert!(matches!(&app.modal,Some(Modal::Form{error,..}) if error.contains("演示模式")));
    }
    assert_eq!(app.state.profiles[0].name, name);
    assert_eq!(
        locale::translate(&format!("演示：已选择订阅 {name}"), Language::English),
        format!("Demo: selected profile {name}")
    );
}
#[test]
fn english_help_and_traditional_palette_work() {
    let mut app = app("en");
    app.key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    let screen = render(&mut app, 120, 40);
    assert!(screen.contains("Navigation"));
    assert!(!screen.contains("导航"));
    app.modal = None;
    app.state.settings.insert("language".into(), "zh-TW".into());
    app.sync_language();
    app.key(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE));
    for c in "設定".chars() {
        app.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert!(render(&mut app, 120, 40).contains("設定"));
    app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.page, Page::Settings);
}
#[test]
fn command_line_override_beats_system_and_snapshots_do_not_save_it() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_clash-verge-tui"))
        .env("LC_ALL", "zh_TW.UTF-8")
        .args([
            "--language",
            "en",
            "--snapshot",
            "home",
            "--data-dir",
            dir.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Quick controls"));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    let output = Command::new(env!("CARGO_BIN_EXE_clash-verge-tui"))
        .env("LC_ALL", "zh_TW.UTF-8")
        .args(["--snapshot", "home"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("首頁"));
}

#[test]
fn nested_errors_are_translated_without_replacing_user_names() {
    assert_eq!(
        locale::translate("操作失败：端口 7890 已被占用", Language::English),
        "Operation failed: Port 7890 is already in use"
    );
    assert_eq!(
        locale::translate("操作失败：端口 7890 已被占用", Language::TraditionalChinese),
        "操作失敗：連接埠 7890 已被佔用"
    );
}

#[test]
fn profiles_save_with_s_from_the_button_and_keep_s_in_text() {
    let mut app = app("en");
    app.navigate(Page::Profiles);
    app.command('a');
    app.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    if let Some(Modal::Form { fields, .. }) = &mut app.modal {
        assert_eq!(fields[0].value, "https://s");
        for field in fields {
            if field.key == "url" {
                field.value = "https://example.com/profile.yaml".into();
                field.cursor = field.value.len();
            }
            if field.key == "name" {
                field.value = "My profile".into();
                field.cursor = field.value.len();
            }
        }
    } else {
        panic!("profile form");
    }
    assert!(render(&mut app, 120, 40).contains("s Save"));
    app.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert!(app.form_save_focused);
    app.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert!(app.modal.is_none());
    assert_eq!(app.state.profiles.last().unwrap().name, "My profile");
}

#[test]
fn home_language_shortcut_scrolls_and_saves_without_core_requests() {
    use clash_verge_tui::app::Action;
    for live in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut a = app("en");
        a.data_dir = dir.path().into();
        if live {
            a.live = Some(LiveState::new("http://127.0.0.1:1".into(), vec![], None));
            a.live.as_mut().unwrap().pending = true;
        }
        a.selected = a.rows().iter().position(|r| r.id == 7).unwrap();
        for (w, h) in [(76, 24), (120, 40)] {
            assert!(render(&mut a, w, h).contains("Interface language"));
            assert!(a
                .hits
                .iter()
                .any(|(_, action)| matches!(action, Action::Select(i) if *i == a.selected)));
        }
        a.activate();
        assert!(render(&mut a, 76, 24).contains("s Save"));
        let Some(Modal::Form { fields, .. }) = &mut a.modal else {
            panic!("language form")
        };
        assert_eq!(fields.len(), 1);
        fields[0].value = "zh-TW".into();
        a.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert!(a.modal.is_none());
        assert_eq!(a.language, Language::TraditionalChinese);
        a.save_preferences().unwrap();
        if live {
            assert!(a.live.as_ref().unwrap().outbox.is_empty());
            assert_eq!(
                App::new_live(
                    dir.path().into(),
                    "http://127.0.0.1:1".into(),
                    vec![],
                    0,
                    None
                )
                .unwrap()
                .language,
                Language::TraditionalChinese
            );
        } else {
            storage::save(dir.path(), &a.state).unwrap();
            assert_eq!(
                App::new(storage::load(dir.path()).unwrap(), dir.path().into()).language,
                Language::TraditionalChinese
            );
        }
    }
}

#[test]
fn rule_and_settings_forms_use_s_without_consuming_text_input() {
    use clash_verge_tui::app::{Action, SaveTarget};
    for page in [Page::Rules, Page::Settings] {
        let dir = tempfile::tempdir().unwrap();
        let mut a = app("en");
        a.data_dir = dir.path().into();
        a.navigate(page);
        if page == Page::Rules {
            a.command('a');
        } else {
            a.activate();
        }
        let Some(Modal::Form { fields, active, .. }) = &mut a.modal else {
            panic!("editable form")
        };
        *active = fields
            .iter()
            .position(|f| matches!(f.kind, settings::Kind::Text))
            .unwrap();
        fields[*active].value.clear();
        fields[*active].cursor = 0;
        a.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        let Some(Modal::Form { fields, active, .. }) = &a.modal else {
            panic!("s must not submit text")
        };
        assert_eq!(fields[*active].value, "s");
        for (w, h) in [(76, 24), (120, 40)] {
            let screen = render(&mut a, w, h);
            assert!(screen.contains("s Save"));
            assert!(!screen.contains("Ctrl+S Save"));
            assert!(a
                .hits
                .iter()
                .any(|(_, action)| matches!(action, Action::Submit)));
        }
        if let Some(Modal::Form {
            fields,
            active,
            target,
            ..
        }) = &mut a.modal
        {
            if matches!(target, SaveTarget::Rule(_)) {
                fields
                    .iter_mut()
                    .find(|f| f.key == "payload")
                    .unwrap()
                    .value = "example.com".into();
            }
            *active = 0;
        }
        a.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert!(a.form_save_focused);
        a.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert!(a.modal.is_none());
    }
}
