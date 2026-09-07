use crate::{
    app::{Action, App, HomeFocus, Modal, SaveTarget},
    model::Page,
    settings::Kind,
};
use ratatui::{prelude::*, widgets::*};
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy)]
struct Palette {
    bg: Color,
    panel: Color,
    raised: Color,
    border: Color,
    text: Color,
    muted: Color,
    accent: Color,
    green: Color,
    yellow: Color,
}
impl Palette {
    fn new(app: &App) -> Self {
        let light = app.state.value("theme") == "浅色";
        let accent = match app.state.value("accent") {
            "青色" => Color::Rgb(38, 163, 157),
            "蓝色" => Color::Rgb(76, 139, 245),
            _ => {
                if light {
                    Color::Rgb(109, 74, 211)
                } else {
                    Color::Rgb(177, 155, 255)
                }
            }
        };
        if light {
            Self {
                bg: Color::Rgb(243, 244, 250),
                panel: Color::Rgb(252, 252, 255),
                raised: Color::Rgb(229, 226, 246),
                border: Color::Rgb(204, 206, 221),
                text: Color::Rgb(34, 39, 57),
                muted: Color::Rgb(91, 98, 118),
                accent,
                green: Color::Rgb(20, 131, 109),
                yellow: Color::Rgb(159, 98, 19),
            }
        } else {
            Self {
                bg: Color::Rgb(16, 19, 28),
                panel: Color::Rgb(22, 26, 38),
                raised: Color::Rgb(39, 36, 61),
                border: Color::Rgb(48, 54, 74),
                text: Color::Rgb(224, 228, 241),
                muted: Color::Rgb(137, 147, 172),
                accent,
                green: Color::Rgb(94, 215, 185),
                yellow: Color::Rgb(240, 192, 112),
            }
        }
    }
}
fn block(title: impl Into<String>, p: Palette) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(format!(" {} ", title.into()))
        .border_style(Style::default().fg(p.border))
        .title_style(Style::default().fg(p.muted))
        .style(Style::default().bg(p.panel))
}
fn text(f: &mut Frame, r: Rect, s: impl Into<Text<'static>>, color: Color) {
    f.render_widget(Paragraph::new(s).style(Style::default().fg(color)), r);
}
fn inset(r: Rect, x: u16, y: u16) -> Rect {
    r.inner(Margin::new(x, y))
}
fn line_area(r: Rect, y: u16, h: u16) -> Rect {
    Rect::new(
        r.x,
        r.y.saturating_add(y),
        r.width,
        h.min(r.height.saturating_sub(y)),
    )
}
fn button(
    f: &mut Frame,
    app: &mut App,
    r: Rect,
    label: &str,
    action: Action,
    p: Palette,
    primary: bool,
) {
    f.render_widget(
        Paragraph::new(label.to_string()).centered().style(
            Style::default()
                .fg(if primary { p.accent } else { p.muted })
                .bg(if primary { p.raised } else { p.panel }),
        ),
        r,
    );
    app.hits.push((r, action));
}
pub fn draw(f: &mut Frame, app: &mut App) {
    let p = Palette::new(app);
    let area = f.area();
    app.hits.clear();
    f.render_widget(
        Block::default().style(Style::default().bg(p.bg).fg(p.text)),
        area,
    );
    if area.width < 76 || area.height < 24 {
        f.render_widget(Paragraph::new(format!("CLASH VERGE TUI · v{}\n\n请将终端调整至至少 76 × 24\n建议尺寸 120 × 40\n\nq / Ctrl+C 退出", env!("CARGO_PKG_VERSION"))).centered().style(Style::default().fg(p.accent)).block(block("终端尺寸",p)),area);
        return;
    }
    let compact = area.width < 100 || app.state.value("compact") == "开启";
    let shell = Layout::horizontal([
        Constraint::Length(if compact { 17 } else { 23 }),
        Constraint::Min(0),
    ])
    .split(area);
    sidebar(f, app, shell[0], p, compact);
    let main = inset(shell[1], 2, 1);
    let parts = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .split(main);
    let heading = Line::from(vec![
        Span::styled(app.page.title(), Style::default().fg(p.text).bold()),
        Span::styled(
            format!("   / {}", app.page.slug().to_uppercase()),
            Style::default().fg(p.muted),
        ),
    ]);
    text(f, line_area(parts[0], 0, 1), heading, p.text);
    text(
        f,
        line_area(parts[0], 2, 1),
        app.page.description().to_string(),
        p.muted,
    );
    if parts[0].width > 44 {
        let badge = Rect::new(parts[0].right() - 24, parts[0].y, 24, 1);
        let label = match &app.live {
            Some(live) if live.connected => "● LIVE / 已连接 mihomo",
            Some(_) => "○ LIVE / 连接中断",
            None => "● DEMO  /  未连接 mihomo",
        };
        text(
            f,
            badge,
            label,
            if app.live.as_ref().is_some_and(|l| l.connected) {
                p.green
            } else {
                p.yellow
            },
        );
    }
    let banner = Line::from(vec![
        Span::styled(
            if app.live.is_some() {
                " 真实内核 "
            } else {
                " 演示工作区 "
            },
            Style::default()
                .fg(if app.live.is_some() {
                    p.green
                } else {
                    p.yellow
                })
                .bg(p.raised),
        ),
        Span::styled(
            if app.live.is_some() {
                "  数据来自 mihomo · 未接入的操作会单独说明"
            } else {
                "  所有网络数据为示例 · 操作仅保存在本地"
            },
            Style::default().fg(p.muted),
        ),
    ]);
    text(f, parts[1], banner, p.muted);
    if app.page == Page::Home {
        home(f, app, parts[2], p);
    } else {
        page(f, app, parts[2], p);
    }
    text(f, line_area(parts[3], 0, 1), app.status.clone(), p.green);
    let vim = app.state.value("vim") == "开启";
    let vertical = if vim {
        "↑↓ / j/k 选择"
    } else {
        "↑↓ 选择"
    };
    let scope = if app.page == Page::Home {
        "焦点"
    } else {
        "分组"
    };
    let horizontal = if app.page == Page::Home || !app.tabs().is_empty() {
        format!("  {} {scope}", if vim { "←→ / h/l" } else { "←→ / Tab" })
    } else {
        String::new()
    };
    text(
        f,
        line_area(parts[3], 1, 1),
        format!("{vertical}{horizontal}  Enter/双击 操作  ? 帮助  q 退出"),
        p.muted,
    );
    if app.modal.is_some() {
        app.hits.clear();
        modal(f, app, area, p);
    }
}
fn sidebar(f: &mut Frame, app: &mut App, r: Rect, p: Palette, compact: bool) {
    f.render_widget(
        Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(p.border))
            .style(Style::default().bg(p.panel)),
        r,
    );
    let x = r.x + 2;
    let w = r.width - 4;
    text(f, Rect::new(x, 2, w, 1), "╲╱  CLASH", p.accent);
    text(f, Rect::new(x, 3, w, 1), "    VERGE TUI", p.text);
    text(f, Rect::new(x, 5, w, 1), "TERMINAL / 01", p.muted);
    let item_height = if r.height >= 32 { 3 } else { 2 };
    let nav_top = r.y + 7;
    for (i, page) in Page::ALL.into_iter().enumerate() {
        let selected = app.page == page;
        let row = Rect::new(
            r.x + 1,
            nav_top + i as u16 * item_height,
            r.width - 2,
            item_height,
        );
        let style = Style::default()
            .fg(if selected { p.accent } else { p.muted })
            .bg(if selected { p.raised } else { p.panel });
        let outline = if selected {
            Block::default()
                .borders(if item_height == 3 {
                    Borders::ALL
                } else {
                    Borders::LEFT
                })
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(p.accent))
        } else {
            Block::default()
        };
        f.render_widget(outline.style(style), row);
        f.render_widget(
            Paragraph::new(format!(" {}  {}", i + 1, page.title())).style(style.add_modifier(
                if selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                },
            )),
            Rect::new(row.x + 1, row.y + item_height / 2, row.width - 2, 1),
        );
        // Include padding and the outline, not just the text line, in the hit target.
        app.hits.push((row, Action::Page(page)));
    }
    let nav_bottom = nav_top + Page::ALL.len() as u16 * item_height;
    if r.bottom().saturating_sub(9) >= nav_bottom {
        text(
            f,
            Rect::new(x, r.bottom() - 9, w, 1),
            "──────────────────",
            p.border,
        );
        text(
            f,
            Rect::new(x, r.bottom() - 7, w, 1),
            match &app.live {
                Some(l) if l.connected => "●  内核已连接",
                Some(_) => "○  等待连接",
                None => "○  内核未连接",
            },
            if app.live.as_ref().is_some_and(|l| l.connected) {
                p.green
            } else {
                p.yellow
            },
        );
        text(
            f,
            Rect::new(x, r.bottom() - 5, w, 1),
            if app.live.is_some() {
                "MIHOMO / LIVE"
            } else if compact {
                "UI PREVIEW"
            } else {
                "本地演示 · 独立状态"
            },
            p.muted,
        );
    }
    if r.bottom().saturating_sub(3) >= nav_bottom {
        text(
            f,
            Rect::new(x, r.bottom() - 3, w, 1),
            format!("v{}", env!("CARGO_PKG_VERSION")),
            p.accent,
        );
    }
}
fn resample(history: &[u64], width: usize) -> Vec<u64> {
    if history.is_empty() || width == 0 {
        return Vec::new();
    }
    if width == 1 {
        return vec![*history.last().unwrap()];
    }
    (0..width)
        .map(|column| {
            let position = column as f64 * (history.len() - 1) as f64 / (width - 1) as f64;
            let left = position.floor() as usize;
            let right = (left + 1).min(history.len() - 1);
            let fraction = position - left as f64;
            (history[left] as f64 * (1.0 - fraction) + history[right] as f64 * fraction).round()
                as u64
        })
        .collect()
}

fn home(f: &mut Frame, app: &mut App, r: Rect, p: Palette) {
    let parts = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(5),
        Constraint::Length((r.height / 3).max(5)),
        Constraint::Min(0),
    ])
    .split(r);
    let cards = Layout::horizontal([
        Constraint::Percentage(34),
        Constraint::Percentage(33),
        Constraint::Percentage(33),
    ])
    .spacing(1)
    .split(parts[1]);
    let down = 2.40 + (app.tick % 19) as f64 / 10.0;
    let up = 128 + app.tick % 80;
    let values = if let Some(live) = &app.live {
        [
            (
                "下载速率",
                live.down_rate
                    .map(|v| format!("{}/s", crate::live::bytes(v)))
                    .unwrap_or("等待采样".into()),
                format!("累计 {}", crate::live::bytes(live.downloaded)),
            ),
            (
                "上传速率",
                live.up_rate
                    .map(|v| format!("{}/s", crate::live::bytes(v)))
                    .unwrap_or("等待采样".into()),
                format!("累计 {}", crate::live::bytes(live.uploaded)),
            ),
            (
                "活动连接",
                format!("{} 会话", app.state.connections.len()),
                if app.state.value("memory") == "开启" {
                    live.memory
                        .map(|v| format!("内存 {}", crate::live::bytes(v)))
                        .unwrap_or("内核未提供内存数据".into())
                } else {
                    "内存显示已关闭".into()
                },
            ),
        ]
    } else {
        [
            (
                "下载速率 · 演示",
                format!("{down:.2} MiB/s"),
                "累计  1.82 GiB".to_string(),
            ),
            (
                "上传速率 · 演示",
                format!("{up} KiB/s"),
                "累计  248.6 MiB".to_string(),
            ),
            (
                "活动连接 · 演示",
                format!("{} 会话", app.state.connections.len()),
                if app.state.value("memory") == "开启" {
                    "内存  48.2 MiB"
                } else {
                    "内存显示已关闭"
                }
                .to_string(),
            ),
        ]
    };
    for (i, (title, value, extra)) in values.into_iter().enumerate() {
        f.render_widget(block(title, p), cards[i]);
        let inner = inset(cards[i], 2, 1);
        text(
            f,
            line_area(inner, 0, 1),
            Line::from(Span::styled(value, Style::default().bold())),
            if i == 1 { p.accent } else { p.green },
        );
        text(f, line_area(inner, 2, 1), extra, p.muted);
    }
    let graph = inset(parts[2], 0, 0);
    f.render_widget(
        block(
            if app.live.is_some() {
                "流量趋势 / 最近 60 个采样"
            } else {
                "流量趋势 / 最近 60 个采样 · 演示"
            },
            p,
        ),
        graph,
    );
    if app.state.value("traffic_graph") == "开启" {
        let inner = inset(graph, 2, 1);
        let lines = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
        let history: Vec<u64> = if let Some(live) = &app.live {
            live.history.iter().copied().collect()
        } else {
            (0..60)
                .map(|i| {
                    let n = (i + app.tick) % 60;
                    15 + (n * 17 % 31) + if (20..35).contains(&n) { 35 } else { 0 }
                })
                .collect()
        };
        // Preserve the same history interval at every terminal width.
        let data = resample(&history, lines[0].width as usize);
        f.render_widget(
            Sparkline::default()
                .data(&data)
                .max(if app.live.is_some() {
                    history.iter().copied().max().unwrap_or(1).max(1)
                } else {
                    90
                })
                .style(Style::default().fg(p.green)),
            lines[0],
        );
        let labels =
            Layout::horizontal([Constraint::Min(0), Constraint::Length(14)]).split(lines[1]);
        text(
            f,
            labels[0],
            if app.live.is_some() {
                format!("-{}", history.len())
            } else {
                "-60".into()
            },
            p.muted,
        );
        f.render_widget(
            Paragraph::new("现在   ↓ 下载")
                .right_aligned()
                .style(Style::default().fg(p.muted)),
            labels[1],
        );
    } else {
        text(
            f,
            inset(graph, 2, 1),
            "流量图已关闭 · 设置 → 界面 → 外观与布局",
            p.muted,
        );
    }
    let bottom = Layout::horizontal([Constraint::Percentage(51), Constraint::Percentage(49)])
        .spacing(1)
        .split(parts[3]);
    let controls_focused = app.home_focus == HomeFocus::Controls;
    let profile_focused = app.home_focus == HomeFocus::Profile;
    f.render_widget(
        block("快捷控制 · ←", p).border_style(Style::default().fg(if controls_focused {
            p.accent
        } else {
            p.border
        })),
        bottom[0],
    );
    let inner = inset(bottom[0], 2, 1);
    let rows = app.rows();
    let offset = app
        .selected
        .saturating_sub(inner.height.saturating_sub(1) as usize);
    for (i, row) in rows
        .iter()
        .enumerate()
        .skip(offset)
        .take(inner.height as usize)
    {
        let line = Line::from(vec![
            Span::styled(
                if controls_focused && i == app.selected {
                    "› "
                } else {
                    "  "
                },
                Style::default().fg(p.accent),
            ),
            Span::raw(format!("{:<10}", row.cells[0])),
            Span::styled(
                format!("  {}", row.cells[1]),
                Style::default().fg(if row.cells[1] == "开启" {
                    p.green
                } else {
                    p.accent
                }),
            ),
        ]);
        let rect = line_area(inner, (i - offset) as u16, 1);
        f.render_widget(
            Paragraph::new(line).style(Style::default().bg(
                if controls_focused && i == app.selected {
                    p.raised
                } else {
                    p.panel
                },
            )),
            rect,
        );
        app.hits.push((rect, Action::Select(i)));
    }
    f.render_widget(
        block("当前订阅 · →", p).border_style(Style::default().fg(if profile_focused {
            p.accent
        } else {
            p.border
        })),
        bottom[1],
    );
    app.hits
        .push((bottom[1], Action::HomeFocus(HomeFocus::Profile)));
    let inner = inset(bottom[1], 2, 1);
    text(
        f,
        line_area(inner, 0, 1),
        app.state.active_name().to_string(),
        p.text,
    );
    if let Some(profile) = app.state.profiles.get(app.state.active_profile) {
        text(
            f,
            line_area(inner, 2, 1),
            if let Some(live) = &app.live {
                live.profiles
                    .get(app.state.active_profile)
                    .map(|p| p.usage_label())
                    .unwrap_or("外部内核管理".into())
            } else {
                format!("{} / {} GB · 示例配额", profile.used, profile.total)
            },
            p.muted,
        );
        let quota = app
            .live
            .as_ref()
            .and_then(|l| l.profiles.get(app.state.active_profile))
            .and_then(|p| p.usage())
            .map(|(used, total, _)| (used as f64 / total as f64).clamp(0.0, 1.0))
            .or_else(|| {
                if app.live.is_none() {
                    Some(
                        (f64::from(profile.used) / f64::from(profile.total.max(1))).clamp(0.0, 1.0),
                    )
                } else {
                    None
                }
            });
        if let Some(ratio) = quota.filter(|_| inner.height > 5) {
            f.render_widget(
                Gauge::default()
                    .ratio(ratio)
                    .label(format!("{:.0}%", ratio * 100.0))
                    .gauge_style(Style::default().fg(p.accent).bg(p.raised)),
                line_area(inner, 4, 1),
            );
        }
    }
    if inner.height > 0 {
        button(
            f,
            app,
            line_area(inner, 6.min(inner.height - 1), 1),
            if profile_focused {
                "› 进入订阅管理 ↵"
            } else {
                "进入订阅管理 →"
            },
            Action::ProfileButton,
            p,
            profile_focused,
        );
    }
}
fn toolbar(app: &App) -> Vec<(&'static str, Action)> {
    if app.live.is_some() {
        return match app.page {
            Page::Proxies => vec![
                ("Enter 选择", Action::Activate),
                ("r 测速", Action::Key('r')),
                ("s 排序", Action::Key('s')),
                ("m 模式", Action::Key('m')),
                ("c 解除固定", Action::Key('c')),
            ],
            Page::Profiles => vec![
                ("a 新建", Action::Key('a')),
                ("e 编辑", Action::Key('e')),
                ("d 删除", Action::Key('d')),
                ("r 更新", Action::Key('r')),
                ("v YAML", Action::Key('v')),
                ("i 详情", Action::Key('i')),
                ("[ 上移", Action::Key('[')),
                ("] 下移", Action::Key(']')),
            ],
            Page::Connections => vec![
                ("Enter 详情", Action::Activate),
                ("d 关闭", Action::Key('d')),
                ("D 全部关闭", Action::Key('D')),
                ("s 排序", Action::Key('s')),
            ],
            Page::Rules if app.sub == 0 => vec![
                ("Enter 启停", Action::Activate),
                ("a 新建", Action::Key('a')),
                ("e 编辑", Action::Key('e')),
                ("d 删除", Action::Key('d')),
                ("r 刷新", Action::Key('r')),
            ],
            Page::Rules => vec![
                ("Enter 详情", Action::Activate),
                ("r 更新集合", Action::Key('r')),
            ],
            Page::Logs => vec![
                (
                    if app.paused { "p 继续" } else { "p 暂停" },
                    Action::Key('p'),
                ),
                ("c 清空", Action::Key('c')),
                ("Enter 详情", Action::Activate),
            ],
            Page::Settings => vec![
                ("Enter 打开", Action::Activate),
                ("u 更新", Action::Key('u')),
                ("g Geo", Action::Key('g')),
                ("o 打开", Action::Key('o')),
                ("x 导出", Action::Key('x')),
            ],
            Page::Unlock => vec![
                ("r 检测全部", Action::Key('r')),
                ("Enter 单项检测", Action::Activate),
            ],
            _ => vec![],
        };
    }
    match app.page {
        Page::Proxies => vec![
            ("Enter 选择", Action::Activate),
            ("r 测速", Action::Key('r')),
            ("s 排序", Action::Key('s')),
            ("m 模式", Action::Key('m')),
        ],
        Page::Profiles => {
            let mut b = vec![
                ("a 新建", Action::Key('a')),
                ("e 编辑", Action::Key('e')),
                ("d 删除", Action::Key('d')),
            ];
            if app.sub == 0 {
                b.extend([("r 更新", Action::Key('r')), ("v YAML", Action::Key('v'))]);
            }
            b.extend([("[ 上移", Action::Key('[')), ("] 下移", Action::Key(']'))]);
            b
        }
        Page::Connections => vec![
            ("Enter 详情", Action::Activate),
            ("d 关闭", Action::Key('d')),
            ("D 全部关闭", Action::Key('D')),
            ("s 排序", Action::Key('s')),
            ("r 重载", Action::Key('r')),
        ],
        Page::Rules if app.sub == 0 => vec![
            ("Enter 开关", Action::Activate),
            ("a 新建", Action::Key('a')),
            ("e 编辑", Action::Key('e')),
            ("d 删除", Action::Key('d')),
        ],
        Page::Rules => vec![
            ("Enter 详情", Action::Activate),
            ("r 更新集合", Action::Key('r')),
        ],
        Page::Logs => vec![
            (
                if app.paused { "p 继续" } else { "p 暂停" },
                Action::Key('p'),
            ),
            ("c 清空", Action::Key('c')),
            ("Enter 详情", Action::Activate),
        ],
        Page::Unlock => vec![
            ("r 检测全部", Action::Key('r')),
            ("Enter 检测选中", Action::Activate),
        ],
        Page::Settings => vec![
            ("Enter 打开", Action::Activate),
            ("b 备份", Action::Key('b')),
            ("R 恢复", Action::Key('R')),
            ("r 版本", Action::Key('r')),
        ],
        _ => vec![],
    }
}
fn page(f: &mut Frame, app: &mut App, r: Rect, p: Palette) {
    let has_tabs = !app.tabs().is_empty();
    let parts = Layout::vertical([
        Constraint::Length(if has_tabs { 2 } else { 1 }),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(if r.height >= 23 { 3 } else { 2 }),
    ])
    .split(r);
    if has_tabs {
        let mut x = parts[0].x;
        let tabs = app.tabs();
        let mut start = 0;
        while start < app.sub
            && tabs[start..=app.sub]
                .iter()
                .map(|s| s.width() + 5)
                .sum::<usize>()
                > parts[0].width as usize
        {
            start += 1;
        }
        for (i, label) in tabs.iter().enumerate().skip(start) {
            let w = (label.width() + 4) as u16;
            if x + w > parts[0].right() {
                break;
            }
            let rect = Rect::new(x, parts[0].y + 1, w, 1);
            button(
                f,
                app,
                rect,
                &format!(" {label} "),
                Action::Sub(i),
                p,
                i == app.sub,
            );
            x += w + 1;
        }
    }
    let summary = match app.page {
        Page::Proxies => app
            .state
            .groups
            .get(app.sub)
            .map(|g| {
                format!(
                    "{} / {}  当前：{}  模式：{}",
                    g.name,
                    g.kind,
                    g.selected,
                    ["规则", "全局", "直连"][app.state.mode]
                )
            })
            .unwrap_or("等待内核策略组数据".into()),
        Page::Profiles if app.sub == 0 => format!(
            "{} 份订阅    当前：{}",
            app.state.profiles.len(),
            app.state.active_name()
        ),
        Page::Profiles => "增强链按列表顺序应用 · Enter 开关 · [ / ] 调整顺序".into(),
        Page::Connections => format!(
            "{} 个活动会话    示例流量  ↓ 223.1 MiB   ↑ 2.2 MiB",
            app.state.connections.len()
        ),
        Page::Rules => "规则按顺序匹配 · 第一条命中生效 · 当前为演示规则".into(),
        Page::Logs => format!(
            "{} 条记录    {}    最多保留 300 条演示日志",
            app.state.logs.len(),
            if app.paused {
                "已暂停"
            } else {
                "持续采样"
            }
        ),
        Page::Unlock => "示例出口：日本 / 美国 · 检测结果仅用于展示界面".into(),
        Page::Settings => format!(
            "{}设置    所有表单支持本地保存    {} 份演示备份",
            app.tabs()[app.sub],
            app.state.backups.len()
        ),
        _ => String::new(),
    };
    let summary = if let Some(live) = &app.live {
        match app.page {
            Page::Connections => format!(
                "{} 个活动会话    累计 ↓ {}   ↑ {}",
                app.state.connections.len(),
                crate::live::bytes(live.downloaded),
                crate::live::bytes(live.uploaded)
            ),
            Page::Rules => "规则来自 mihomo；启停为运行时操作".into(),
            Page::Logs => format!(
                "{} 条日志 · {} · UTC 时间",
                app.state.logs.len(),
                if app.paused {
                    "已暂停"
                } else {
                    live.log_status.as_str()
                }
            ),
            Page::Settings => "真实模式 · 仅已接入的网络参数可修改".into(),
            Page::Unlock => "真实网页可达性检测 · 出口地区参考 · 不确定结果单独标记".into(),
            Page::Profiles if app.sub == 1 => "按顺序应用 YAML / JavaScript；失败保留原配置".into(),
            _ => summary,
        }
    } else {
        summary
    };
    f.render_widget(
        Paragraph::new(summary)
            .style(Style::default().fg(p.accent))
            .block(block("", p)),
        parts[1],
    );
    let mut x = parts[2].x;
    for (label, action) in toolbar(app) {
        let w = label.width() as u16 + 2;
        if x + w > parts[2].right() {
            break;
        }
        button(
            f,
            app,
            Rect::new(x, parts[2].y, w, 1),
            label,
            action,
            p,
            false,
        );
        x += w + 1;
    }
    let rows = app.rows();
    let count = rows.len();
    let title = if app.searching || !app.query.is_empty() {
        format!(
            "搜索 / {}{}  · {} 项",
            app.query,
            if app.searching { "▏" } else { "" },
            count
        )
    } else {
        format!("{}  · {} 项    / 搜索", app.page.title(), count)
    };
    let (headers, widths) = columns(app);
    let table_rows: Vec<Row> = rows
        .iter()
        .map(|r| {
            Row::new(r.cells.iter().enumerate().map(|(i, s)| {
                let color = if s.contains("超时") || s.contains("不可用") || s == "ERROR" {
                    p.yellow
                } else if s == "开启" || s.contains("可用") || s == "[✓]" || s == "[✓] 当前"
                {
                    p.green
                } else if i == 0 {
                    p.text
                } else {
                    p.muted
                };
                Cell::from(s.clone()).style(Style::default().fg(color))
            }))
            .height(1)
            .bottom_margin(0)
        })
        .collect();
    let table = Table::new(table_rows, widths)
        .header(
            Row::new(headers)
                .height(1)
                .bottom_margin(0)
                .style(Style::default().fg(p.muted)),
        )
        .block(block(title, p))
        .column_spacing(2)
        .row_highlight_style(Style::default().bg(p.raised).fg(p.accent))
        .highlight_symbol("› ");
    let mut state = TableState::default()
        .with_offset(app.table_offset)
        .with_selected(if count > 0 {
            Some(app.selected.min(count - 1))
        } else {
            None
        });
    f.render_stateful_widget(table, parts[3], &mut state);
    app.table_offset = state.offset();
    let inner = inset(parts[3], 1, 1);
    if count == 0 {
        text(
            f,
            Rect::new(inner.x + 2, inner.y + 1, inner.width.saturating_sub(4), 1),
            if app.live.is_some() && app.query.is_empty() {
                "暂无真实数据 · 连接或功能状态见上方说明"
            } else if app.query.is_empty() {
                "暂无条目 · 使用上方操作添加或重新载入"
            } else {
                "没有匹配结果 · Esc 清除筛选"
            },
            p.muted,
        );
    }
    for i in 0..inner.height.saturating_sub(1) {
        let index = state.offset() + i as usize;
        if index >= count {
            break;
        }
        let rect = Rect::new(inner.x, inner.y + 1 + i, inner.width, 1);
        if rect.y < inner.bottom() {
            app.hits.push((rect, Action::Select(index)));
        }
    }
    let detail = rows
        .get(app.selected)
        .map(|r| match app.page {
            Page::Proxies => format!(
                "{} · {} · {}    Enter 选用此节点",
                r.cells[1], r.cells[2], r.cells[3]
            ),
            Page::Profiles if app.sub == 0 => {
                let pr = &app.state.profiles[r.id];
                format!("{}\n{} · Enter 设为当前订阅", pr.name, redact_url(&pr.url))
            }
            Page::Profiles => format!(
                "{}\n{}",
                r.cells[1],
                if app.live.is_some() {
                    "配置内容使用 e 编辑；按顺序执行并通过内核校验。"
                } else {
                    "演示模式只保存内容，不执行配置增强。"
                }
            ),
            Page::Settings => format!("{}\n{}", r.cells[0], r.cells[1]),
            _ => r.cells.join("   ·   "),
        })
        .unwrap_or_else(|| "使用 / 搜索，Esc 清除筛选。".into());
    f.render_widget(
        Paragraph::new(detail)
            .style(Style::default().fg(p.muted))
            .wrap(Wrap { trim: false })
            .block(Block::default().padding(Padding::new(1, 1, 1, 0))),
        parts[4],
    );
}
fn columns(app: &App) -> (Vec<&'static str>, Vec<Constraint>) {
    use Constraint::*;
    match app.page {
        Page::Proxies => (
            vec!["状态", "节点名称", "协议", "地区", "延迟"],
            vec![Length(4), Min(14), Length(12), Length(4), Length(8)],
        ),
        Page::Profiles if app.sub == 0 => (
            vec!["状态", "订阅名称", "用量", "更新间隔", "最近更新"],
            vec![Length(8), Min(12), Length(13), Length(9), Length(18)],
        ),
        Page::Profiles => (
            vec!["顺序", "名称", "类型", "状态"],
            vec![Length(5), Min(15), Length(12), Length(6)],
        ),
        Page::Connections => (
            vec!["目标地址", "进程", "网络", "出站链", "下载", "上传"],
            vec![
                Min(16),
                Length(8),
                Length(5),
                Percentage(23),
                Length(10),
                Length(9),
            ],
        ),
        Page::Rules if app.sub == 0 => (
            vec!["状态", "类型", "匹配内容", "出站策略"],
            vec![Length(4), Length(16), Min(15), Length(12)],
        ),
        Page::Rules => (
            if app.live.is_some() {
                vec!["集合名称", "行为", "规则数", "来源", "更新"]
            } else {
                vec!["集合名称", "行为", "规则数", "策略", "状态"]
            },
            vec![Min(12), Length(18), Length(8), Length(8), Length(10)],
        ),
        Page::Logs => (
            vec!["时间", "等级", "内容"],
            vec![Length(8), Length(6), Min(18)],
        ),
        Page::Unlock => (
            vec!["服务", "类型", "结果", "区域"],
            vec![Min(20), Length(10), Length(18), Length(5)],
        ),
        _ => (
            vec!["设置项", "说明", ""],
            vec![Length(20), Min(14), Length(2)],
        ),
    }
}
fn redact_url(url: &str) -> String {
    let base = url.split('?').next().unwrap_or(url);
    let Some((scheme, rest)) = base.split_once("://") else {
        return "本地配置".into();
    };
    let host = rest
        .split('/')
        .next()
        .unwrap_or("")
        .rsplit('@')
        .next()
        .unwrap_or("");
    format!("{scheme}://{host}/…")
}
fn modal(f: &mut Frame, app: &mut App, area: Rect, p: Palette) {
    let width = area.width.saturating_sub(8).min(88);
    let height = area.height.saturating_sub(4).min(34);
    let r = Rect::new(
        (area.width - width) / 2,
        (area.height - height) / 2,
        width,
        height,
    );
    f.render_widget(Clear, r);
    f.render_widget(Block::default().style(Style::default().bg(p.panel)), r);
    let current = app.modal.clone().unwrap();
    match current {
        Modal::Backups { selected } => {
            f.render_widget(
                block(
                    if let Some(live) = &app.live {
                        if live.backups_remote {
                            "备份与恢复 / WebDAV"
                        } else {
                            "备份与恢复 / 本地"
                        }
                    } else {
                        "备份与恢复 / 本地演示"
                    },
                    p,
                )
                .border_style(Style::default().fg(p.accent)),
                r,
            );
            let inner = inset(r, 3, 2);
            text(
                f,
                line_area(inner, 0, 1),
                if app.live.is_some() {
                    "←→ 本地 / WebDAV   u 上传选中备份   e 设置"
                } else {
                    "本地快照    /    WebDAV 配置按 e 打开"
                },
                p.accent,
            );
            text(
                f,
                line_area(inner, 2, 1),
                "包含设置、订阅、增强链与规则 · 最多保留 10 份",
                p.muted,
            );
            let records: Vec<(String, String)> = if let Some(live) = &app.live {
                live.backups
                    .iter()
                    .map(|b| {
                        (
                            b.file.clone(),
                            if live.backups_remote {
                                "远端文件".into()
                            } else if !b.valid {
                                "文件损坏，可删除".into()
                            } else {
                                format!(
                                    "{} 份订阅 · {}",
                                    b.profiles,
                                    if b.encrypted {
                                        "已加密"
                                    } else {
                                        "未加密"
                                    }
                                )
                            },
                        )
                    })
                    .collect()
            } else {
                app.state
                    .backups
                    .iter()
                    .map(|b| {
                        (
                            b.name.clone(),
                            format!("{} 份订阅 · {} 条规则", b.profiles.len(), b.rules.len()),
                        )
                    })
                    .collect()
            };
            let capacity = inner.height.saturating_sub(9) as usize;
            let offset = selected.saturating_sub(capacity.saturating_sub(1));
            for (i, (name, info)) in records.iter().enumerate().skip(offset).take(capacity) {
                let rect = line_area(inner, 3 + (i - offset) as u16, 1);
                f.render_widget(
                    Paragraph::new(format!(
                        "{} {}  {}",
                        if i == selected { "›" } else { " " },
                        name,
                        info
                    ))
                    .style(
                        Style::default()
                            .fg(if i == selected { p.accent } else { p.text })
                            .bg(if i == selected { p.raised } else { p.panel }),
                    ),
                    rect,
                );
                app.hits.push((rect, Action::BackupSelect(i)));
            }
            if records.is_empty() {
                text(
                    f,
                    line_area(inner, 5, 1),
                    "尚无备份 · b 创建，←→ 切换本地 / WebDAV",
                    p.muted,
                );
            }
            if app.live.is_some() {
                text(
                    f,
                    line_area(inner, inner.height - 5, 1),
                    app.status.clone(),
                    p.yellow,
                );
            }
            text(
                f,
                line_area(inner, inner.height - 3, 1),
                "↑↓ 选择   Enter/双击 恢复   d 删除   Esc 关闭",
                p.muted,
            );
            button(
                f,
                app,
                Rect::new(inner.x, inner.bottom() - 1, 15, 1),
                "b 创建快照",
                Action::Key('b'),
                p,
                true,
            );
            button(
                f,
                app,
                Rect::new(inner.x + 17, inner.bottom() - 1, 17, 1),
                "e WebDAV 设置",
                Action::Key('e'),
                p,
                false,
            );
        }
        Modal::Form {
            title,
            fields,
            active,
            mut scroll,
            error,
            target,
        } => {
            f.render_widget(
                block(
                    format!(
                        "{title}  /  {}",
                        if app.live.is_some() {
                            "真实模式"
                        } else {
                            "本地演示"
                        }
                    ),
                    p,
                )
                .border_style(Style::default().fg(p.accent)),
                r,
            );
            let inner = inset(r, 2, 1);
            let field_area = Rect::new(
                inner.x,
                inner.y + 1,
                inner.width,
                inner.height.saturating_sub(5),
            );
            let h = |i: usize| {
                if matches!(fields[i].kind, Kind::Multiline) {
                    7u16.min(field_area.height)
                } else {
                    2
                }
            };
            if active < scroll {
                scroll = active;
            }
            while (scroll..=active).map(h).sum::<u16>() > field_area.height && scroll < active {
                scroll += 1;
            }
            if let Some(Modal::Form { scroll: s, .. }) = &mut app.modal {
                *s = scroll;
            }
            text(
                f,
                line_area(inner, 0, 1),
                format!(
                    "字段 {}/{}    Tab 切换 · Ctrl+U 清空 · Ctrl+S 保存",
                    active + 1,
                    fields.len()
                ),
                p.muted,
            );
            let mut y = field_area.y;
            for (i, field) in fields.iter().enumerate().skip(scroll) {
                let height = h(i);
                if y + height > field_area.bottom() {
                    break;
                }
                let selected = i == active;
                let color = if selected { p.accent } else { p.muted };
                text(
                    f,
                    Rect::new(field_area.x, y, field_area.width, 1),
                    format!("{} {}", if selected { "›" } else { " " }, field.label),
                    color,
                );
                let has_import = field.key == "url" && matches!(&target, SaveTarget::Profile(_));
                let available_width = field_area.width.saturating_sub(2);
                let input = Rect::new(
                    field_area.x + 2,
                    y + 1,
                    if has_import {
                        available_width.saturating_sub(11)
                    } else {
                        available_width
                    },
                    height.saturating_sub(1).max(1),
                );
                let value = match field.kind {
                    Kind::Toggle => format!(
                        "{}  {}    ← →",
                        if field.value == "开启" {
                            "[✓]"
                        } else {
                            "[ ]"
                        },
                        field.value
                    ),
                    Kind::Choice(_) => format!("‹  {}  ›", field.value),
                    Kind::Secret => format!(
                        "{}{}",
                        "•".repeat(field.value.chars().count()),
                        if selected { "▏" } else { "" }
                    ),
                    _ => {
                        let mut value = field.value.clone();
                        if selected {
                            value.insert(field.cursor, '▏');
                        }
                        value
                    }
                };
                let value = if matches!(field.kind, Kind::Multiline) {
                    let cursor_line = field.value[..field.cursor]
                        .lines()
                        .count()
                        .saturating_sub(1)
                        + usize::from(field.value[..field.cursor].ends_with('\n'));
                    let offset = if selected {
                        cursor_line.saturating_sub(input.height.saturating_sub(1) as usize)
                    } else {
                        0
                    };
                    value
                        .lines()
                        .skip(offset)
                        .map(|line| fit_input(line, input.width as usize, selected))
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    fit_input(&value, input.width as usize, selected)
                };
                f.render_widget(
                    Paragraph::new(value).style(
                        Style::default()
                            .fg(if selected { p.text } else { p.muted })
                            .bg(p.raised),
                    ),
                    input,
                );
                app.hits.push((
                    Rect::new(field_area.x, y, input.width + 2, height),
                    Action::Field(i),
                ));
                if has_import {
                    button(
                        f,
                        app,
                        Rect::new(input.x + input.width + 1, input.y, 10, 1),
                        if app.profile_import_pending.is_some() {
                            "导入中"
                        } else {
                            "导入"
                        },
                        Action::ImportProfile,
                        p,
                        selected,
                    );
                }
                y += height;
            }
            let hint = if error.is_empty() {
                if matches!(&target, SaveTarget::Profile(_)) {
                    "订阅链接按 Enter 或点击导入；确认配置后 Ctrl+S 保存".into()
                } else if app.live.is_some() {
                    "网络参数发送至内核；Esc 取消未保存修改".into()
                } else {
                    "网络设置仅保存演示值；Esc 取消未保存修改".to_string()
                }
            } else {
                error
            };
            text(
                f,
                Rect::new(inner.x, inner.bottom() - 3, inner.width, 1),
                hint,
                p.yellow,
            );
            button(
                f,
                app,
                Rect::new(inner.x, inner.bottom() - 1, 19, 1),
                "Ctrl+S 保存",
                Action::Submit,
                p,
                true,
            );
            button(
                f,
                app,
                Rect::new(inner.x + 21, inner.bottom() - 1, 15, 1),
                "Esc 取消",
                Action::Cancel,
                p,
                false,
            );
        }
        Modal::Detail {
            title,
            body,
            scroll,
        } => {
            f.render_widget(
                block(title, p).border_style(Style::default().fg(p.accent)),
                r,
            );
            let inner = inset(r, 2, 1);
            f.render_widget(
                Paragraph::new(body)
                    .wrap(Wrap { trim: false })
                    .scroll((scroll, 0))
                    .style(Style::default().fg(p.text)),
                Rect::new(
                    inner.x,
                    inner.y + 1,
                    inner.width,
                    inner.height.saturating_sub(3),
                ),
            );
            button(
                f,
                app,
                Rect::new(inner.x, inner.bottom() - 1, 18, 1),
                "Esc 关闭",
                Action::Cancel,
                p,
                true,
            );
            text(
                f,
                Rect::new(inner.x + 20, inner.bottom() - 1, inner.width - 20, 1),
                "↑ ↓ 滚动 / PgUp PgDn",
                p.muted,
            );
        }
        Modal::Confirm { title, body, .. } => {
            f.render_widget(
                block(title, p).border_style(Style::default().fg(p.yellow)),
                r,
            );
            let inner = inset(r, 3, 2);
            text(f, line_area(inner, 1, 1), "确认操作", p.yellow);
            f.render_widget(
                Paragraph::new(body)
                    .wrap(Wrap { trim: false })
                    .style(Style::default().fg(p.text)),
                Rect::new(inner.x, inner.y + 4, inner.width, inner.height - 7),
            );
            button(
                f,
                app,
                Rect::new(inner.x, inner.bottom() - 1, 18, 1),
                "Enter 确认",
                Action::Submit,
                p,
                true,
            );
            button(
                f,
                app,
                Rect::new(inner.x + 20, inner.bottom() - 1, 16, 1),
                "Esc 取消",
                Action::Cancel,
                p,
                false,
            );
        }
        Modal::Palette { query, selected } => {
            f.render_widget(
                block("页面跳转", p).border_style(Style::default().fg(p.accent)),
                r,
            );
            let inner = inset(r, 3, 2);
            text(
                f,
                line_area(inner, 0, 1),
                format!("搜索页面  / {query}▏"),
                p.accent,
            );
            let entries = App::palette_entries(&query);
            for (i, page) in entries.iter().enumerate() {
                let rect = line_area(inner, 2 + i as u16, 1);
                if rect.height == 0 {
                    break;
                }
                button(
                    f,
                    app,
                    rect,
                    &format!("{}   {} / {}", page.index() + 1, page.title(), page.slug()),
                    Action::Page(*page),
                    p,
                    i == selected,
                );
            }
            if entries.is_empty() {
                text(f, line_area(inner, 4, 1), "没有匹配页面", p.muted);
            }
            button(
                f,
                app,
                Rect::new(inner.x, inner.bottom() - 1, 16, 1),
                "Esc 关闭",
                Action::Cancel,
                p,
                false,
            );
        }
    }
}
fn fit_input(value: &str, width: usize, focused: bool) -> String {
    if value.width() <= width {
        return value.into();
    }
    let chars: Vec<char> = value.chars().collect();
    let cursor = if focused {
        chars
            .iter()
            .position(|c| *c == '▏')
            .unwrap_or(chars.len().saturating_sub(1))
    } else {
        0
    };
    let mut start = 0;
    while chars[start..=cursor].iter().collect::<String>().width() > width.saturating_sub(1)
        && start < cursor
    {
        start += 1;
    }
    let mut out = if start > 0 {
        "‹".to_string()
    } else {
        String::new()
    };
    for c in chars.into_iter().skip(start) {
        let mut next = out.clone();
        next.push(c);
        if next.width() > width {
            break;
        }
        out = next;
    }
    out
}
