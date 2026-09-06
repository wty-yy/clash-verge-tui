use crate::{
    model::*,
    settings::{self, Kind, Spec},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct DataRow {
    pub id: usize,
    pub cells: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomeFocus {
    Controls,
    Profile,
}

#[derive(Clone, Debug)]
pub enum Action {
    Page(Page),
    Sub(usize),
    Select(usize),
    Activate,
    Key(char),
    Field(usize),
    BackupSelect(usize),
    HomeFocus(HomeFocus),
    ProfileButton,
    Submit,
    Cancel,
}
#[derive(Clone, Debug)]
pub enum SaveTarget {
    Settings(usize),
    Profile(Option<usize>),
    Enhancement(Option<usize>),
    Rule(Option<usize>),
    ProfileContent(usize),
}
#[derive(Clone, Debug)]
pub enum Confirm {
    Profile(usize),
    Enhancement(usize),
    Connection(usize),
    AllConnections,
    Rule(usize),
    Logs,
    Restore(usize),
    Backup(usize),
}
#[derive(Clone, Debug)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub value: String,
    pub kind: Kind,
    pub cursor: usize,
}
impl Field {
    pub fn new(key: &str, label: &str, value: &str, kind: Kind) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            value: value.into(),
            kind,
            cursor: value.len(),
        }
    }
    pub fn from_spec(spec: &Spec, value: &str) -> Self {
        Self::new(spec.key, spec.label, value, spec.kind.clone())
    }
    pub fn cycle(&mut self, reverse: bool) {
        match &self.kind {
            Kind::Toggle => {
                self.value = if self.value == "开启" {
                    "关闭"
                } else {
                    "开启"
                }
                .into()
            }
            Kind::Choice(choices) => {
                let index = choices.iter().position(|c| *c == self.value).unwrap_or(0);
                let next = if reverse {
                    (index + choices.len() - 1) % choices.len()
                } else {
                    (index + 1) % choices.len()
                };
                self.value = choices[next].into();
            }
            _ => {}
        }
        self.cursor = self.value.len();
    }
    pub fn insert(&mut self, s: &str) {
        if matches!(self.kind, Kind::Toggle | Kind::Choice(_)) {
            return;
        }
        let s = s.replace("\r\n", "\n").replace('\r', "\n");
        let s: String = s
            .chars()
            .filter(|c| !c.is_control() || (*c == '\n' && matches!(self.kind, Kind::Multiline)))
            .collect();
        if self.value.len() + s.len() > 65536 {
            return;
        }
        self.value.insert_str(self.cursor, &s);
        self.cursor += s.len();
    }
    pub fn key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.value.clear();
                self.cursor = 0;
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if matches!(self.kind, Kind::Toggle | Kind::Choice(_)) {
                    if c == ' ' {
                        self.cycle(false);
                    }
                } else {
                    self.insert(&c.to_string());
                }
            }
            KeyCode::Left if matches!(self.kind, Kind::Toggle | Kind::Choice(_)) => {
                self.cycle(true)
            }
            KeyCode::Right if matches!(self.kind, Kind::Toggle | Kind::Choice(_)) => {
                self.cycle(false)
            }
            KeyCode::Left => {
                self.cursor = self.value[..self.cursor]
                    .char_indices()
                    .last()
                    .map(|(i, _)| i)
                    .unwrap_or(0)
            }
            KeyCode::Right => {
                self.cursor += self.value[self.cursor..]
                    .chars()
                    .next()
                    .map(char::len_utf8)
                    .unwrap_or(0)
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.value.len(),
            KeyCode::Backspace if self.cursor > 0 => {
                let start = self.value[..self.cursor].char_indices().last().unwrap().0;
                self.value.drain(start..self.cursor);
                self.cursor = start;
            }
            KeyCode::Delete if self.cursor < self.value.len() => {
                let end =
                    self.cursor + self.value[self.cursor..].chars().next().unwrap().len_utf8();
                self.value.drain(self.cursor..end);
            }
            KeyCode::Enter if matches!(self.kind, Kind::Multiline) => self.insert("\n"),
            _ => {}
        }
    }
}
#[derive(Clone, Debug)]
pub enum Modal {
    Backups {
        selected: usize,
    },
    Form {
        title: String,
        fields: Vec<Field>,
        active: usize,
        scroll: usize,
        error: String,
        target: SaveTarget,
    },
    Detail {
        title: String,
        body: String,
        scroll: u16,
    },
    Confirm {
        title: String,
        body: String,
        target: Confirm,
    },
    Palette {
        query: String,
        selected: usize,
    },
}
const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(400);

#[derive(Debug, PartialEq, Eq)]
enum ClickTarget {
    HomeProfile(String),
    Row {
        page: Page,
        sub: usize,
        id: usize,
        cells: Vec<String>,
    },
    Backup {
        index: usize,
        name: String,
    },
}

struct PendingClick {
    target: ClickTarget,
    area: Rect,
    column: u16,
    row: u16,
    at: Instant,
}

pub struct App {
    pub state: DemoState,
    pub page: Page,
    pub home_focus: HomeFocus,
    pub sub: usize,
    pub selected: usize,
    pub query: String,
    pub searching: bool,
    pub modal: Option<Modal>,
    pub status: String,
    pub tick: u64,
    pub paused: bool,
    pub sort: bool,
    pub dirty: bool,
    pub quit: bool,
    pub data_dir: PathBuf,
    pub hits: Vec<(Rect, Action)>,
    pub table_offset: usize,
    pending_click: Option<PendingClick>,
}
impl App {
    pub fn new(state: DemoState, data_dir: PathBuf) -> Self {
        let page = Page::ALL
            .into_iter()
            .find(|p| p.title() == state.value("start_page"))
            .unwrap_or(Page::Home);
        Self {
            state,
            page,
            home_focus: HomeFocus::Controls,
            sub: 0,
            selected: 0,
            query: String::new(),
            searching: false,
            modal: None,
            status: "欢迎使用 · 所有网络数据与操作均为本地演示".into(),
            tick: 0,
            paused: false,
            sort: false,
            dirty: false,
            quit: false,
            data_dir,
            hits: vec![],
            table_offset: 0,
            pending_click: None,
        }
    }
    pub fn navigate(&mut self, page: Page) {
        self.cancel_pending_click();
        self.page = page;
        self.home_focus = HomeFocus::Controls;
        self.sub = 0;
        self.selected = 0;
        self.query.clear();
        self.searching = false;
        self.table_offset = 0;
    }
    pub fn tabs(&self) -> Vec<String> {
        match self.page {
            Page::Proxies => self.state.groups.iter().map(|g| g.name.clone()).collect(),
            Page::Profiles => vec!["订阅配置".into(), "配置增强".into()],
            Page::Rules => vec!["路由规则".into(), "规则集合".into()],
            Page::Logs => ["全部", "INFO", "DEBUG", "WARN", "ERROR"]
                .map(str::to_string)
                .to_vec(),
            Page::Unlock => ["全部", "流媒体", "AI", "社交"]
                .map(str::to_string)
                .to_vec(),
            Page::Settings => settings::CATEGORIES.map(str::to_string).to_vec(),
            _ => vec![],
        }
    }
    pub fn rows(&self) -> Vec<DataRow> {
        let row = |id, cells| DataRow { id, cells };
        let mut rows: Vec<DataRow> = match self.page {
            Page::Home => vec![
                row(
                    0,
                    vec!["系统代理".into(), self.state.value("system_proxy").into()],
                ),
                row(
                    1,
                    vec!["虚拟网卡 TUN".into(), self.state.value("tun").into()],
                ),
                row(
                    2,
                    vec![
                        "代理模式".into(),
                        ["规则", "全局", "直连"][self.state.mode].into(),
                    ],
                ),
                row(3, vec!["当前订阅".into(), self.state.active_name().into()]),
                row(4, vec!["运行配置".into(), "查看".into()]),
                row(5, vec!["环境变量".into(), "查看".into()]),
            ],
            Page::Proxies => self
                .state
                .nodes
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    row(
                        i,
                        vec![
                            if self.state.groups[self.sub].selected == n.name {
                                "[✓]".into()
                            } else {
                                "[ ]".into()
                            },
                            n.name.clone(),
                            n.protocol.clone(),
                            n.region.clone(),
                            n.delay.map(|d| format!("{d} ms")).unwrap_or("超时".into()),
                        ],
                    )
                })
                .collect(),
            Page::Profiles if self.sub == 0 => self
                .state
                .profiles
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    row(
                        i,
                        vec![
                            if i == self.state.active_profile {
                                "[✓] 当前".into()
                            } else {
                                "[ ]".into()
                            },
                            p.name.clone(),
                            format!("{} / {} GB", p.used, p.total),
                            format!("{} min", p.interval),
                            p.updated.clone(),
                        ],
                    )
                })
                .collect(),
            Page::Profiles => self
                .state
                .enhancements
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    row(
                        i,
                        vec![
                            (i + 1).to_string(),
                            e.name.clone(),
                            e.kind.clone(),
                            if e.enabled { "开启" } else { "关闭" }.into(),
                        ],
                    )
                })
                .collect(),
            Page::Connections => self
                .state
                .connections
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    row(
                        i,
                        vec![
                            c.host.clone(),
                            c.process.clone(),
                            c.network.clone(),
                            c.chain.clone(),
                            c.down.clone(),
                            c.up.clone(),
                        ],
                    )
                })
                .collect(),
            Page::Rules if self.sub == 0 => self
                .state
                .rules
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    row(
                        i,
                        vec![
                            if r.enabled { "[✓]" } else { "[ ]" }.into(),
                            r.kind.clone(),
                            r.payload.clone(),
                            r.target.clone(),
                        ],
                    )
                })
                .collect(),
            Page::Rules => [
                ("streaming", "HTTP / classical", "1,248", "流媒体"),
                ("reject", "HTTP / domain", "8,421", "REJECT"),
                ("direct", "HTTP / ipcidr", "5,320", "DIRECT"),
            ]
            .into_iter()
            .enumerate()
            .map(|(i, (n, t, c, p))| {
                row(
                    i,
                    vec![n.into(), t.into(), c.into(), p.into(), "演示数据".into()],
                )
            })
            .collect(),
            Page::Logs => self
                .state
                .logs
                .iter()
                .enumerate()
                .filter(|(_, l)| {
                    self.sub == 0 || l.level == ["全部", "INFO", "DEBUG", "WARN", "ERROR"][self.sub]
                })
                .map(|(i, l)| row(i, vec![l.time.clone(), l.level.clone(), l.message.clone()]))
                .collect(),
            Page::Unlock => self
                .state
                .unlocks
                .iter()
                .enumerate()
                .filter(|(_, u)| {
                    self.sub == 0 || u.category == ["全部", "流媒体", "AI", "社交"][self.sub]
                })
                .map(|(i, u)| {
                    row(
                        i,
                        vec![
                            u.name.clone(),
                            u.category.clone(),
                            u.result.clone(),
                            u.region.clone(),
                        ],
                    )
                })
                .collect(),
            Page::Settings => settings::sections()
                .iter()
                .enumerate()
                .filter(|(_, s)| s.category == self.sub)
                .map(|(i, s)| row(i, vec![s.name.into(), s.description.into(), "›".into()]))
                .collect(),
        };
        if !self.query.is_empty() {
            let q = self.query.to_lowercase();
            rows.retain(|r| r.cells.iter().any(|s| s.to_lowercase().contains(&q)));
        }
        if self.sort {
            match self.page {
                Page::Proxies => {
                    rows.sort_by_key(|r| self.state.nodes[r.id].delay.unwrap_or(u16::MAX))
                }
                Page::Connections => rows.sort_by(|a, b| a.cells[0].cmp(&b.cells[0])),
                _ => {}
            }
        }
        rows
    }
    pub fn selected_id(&self) -> Option<usize> {
        self.rows().get(self.selected).map(|r| r.id)
    }
    pub fn step(&mut self, delta: isize) {
        if self.page == Page::Home && self.home_focus == HomeFocus::Profile {
            return;
        }
        let n = self.rows().len();
        self.selected = if n == 0 {
            0
        } else {
            (self.selected as isize + delta).clamp(0, n as isize - 1) as usize
        };
    }
    pub fn change_sub(&mut self, delta: isize) {
        self.cancel_pending_click();
        if self.page == Page::Home {
            self.home_focus = if self.home_focus == HomeFocus::Controls {
                HomeFocus::Profile
            } else {
                HomeFocus::Controls
            };
            return;
        }
        let n = self.tabs().len();
        if n > 0 {
            self.sub = (self.sub as isize + delta).rem_euclid(n as isize) as usize;
            self.selected = 0;
            self.table_offset = 0;
        }
    }
    fn move_horizontal(&mut self, delta: isize) {
        if self.page == Page::Home {
            self.home_focus = if delta < 0 {
                HomeFocus::Controls
            } else {
                HomeFocus::Profile
            };
        } else {
            self.change_sub(delta);
        }
    }

    pub fn note(&mut self, message: impl Into<String>) {
        self.status = message.into();
        self.dirty = true;
    }
    pub fn detail(&mut self, title: impl Into<String>, body: impl Into<String>) {
        self.modal = Some(Modal::Detail {
            title: title.into(),
            body: body.into(),
            scroll: 0,
        });
    }
    pub fn form(&mut self, title: impl Into<String>, fields: Vec<Field>, target: SaveTarget) {
        self.modal = Some(Modal::Form {
            title: title.into(),
            fields,
            active: 0,
            scroll: 0,
            error: String::new(),
            target,
        });
    }
    pub fn confirm(&mut self, title: &str, body: &str, target: Confirm) {
        self.modal = Some(Modal::Confirm {
            title: title.into(),
            body: body.into(),
            target,
        });
    }
    pub fn palette_entries(query: &str) -> Vec<Page> {
        Page::ALL
            .into_iter()
            .filter(|p| {
                format!("{} {}", p.title(), p.slug())
                    .to_lowercase()
                    .contains(&query.to_lowercase())
            })
            .collect()
    }
    pub fn key(&mut self, key: KeyEvent) {
        self.cancel_pending_click();
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return;
        }
        if self.modal.is_some() {
            self.modal_key(key);
            return;
        }
        if self.searching {
            match key.code {
                KeyCode::Esc => {
                    self.query.clear();
                    self.searching = false;
                }
                KeyCode::Enter => self.searching = false,
                KeyCode::Backspace => {
                    self.query.pop();
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.query.push(c)
                }
                _ => {}
            }
            self.selected = 0;
            return;
        }
        match key.code {
            KeyCode::Char(c @ '1'..='8') => self.navigate(Page::ALL[c as usize - '1' as usize]),
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.help(),
            KeyCode::Char(':') => {
                self.modal = Some(Modal::Palette {
                    query: String::new(),
                    selected: 0,
                })
            }
            KeyCode::Char('/') => {
                self.searching = true;
                self.query.clear();
                self.selected = 0;
            }
            KeyCode::Esc => {
                self.query.clear();
                self.selected = 0;
            }
            KeyCode::Up => self.step(-1),
            KeyCode::Down => self.step(1),
            KeyCode::Char('j') if self.state.value("vim") == "开启" => self.step(1),
            KeyCode::Char('k') if self.state.value("vim") == "开启" => self.step(-1),
            KeyCode::PageUp => self.step(-10),
            KeyCode::PageDown => self.step(10),
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.selected = self.rows().len().saturating_sub(1),
            KeyCode::Tab => self.change_sub(1),
            KeyCode::BackTab => self.change_sub(-1),
            KeyCode::Right => self.move_horizontal(1),
            KeyCode::Left => self.move_horizontal(-1),
            KeyCode::Char('h') if self.state.value("vim") == "开启" => self.move_horizontal(-1),
            KeyCode::Char('l') if self.state.value("vim") == "开启" => self.move_horizontal(1),
            KeyCode::Enter | KeyCode::Char(' ') => self.activate(),
            KeyCode::Char(c) => self.command(c),
            _ => {}
        }
    }
    pub fn cancel_pending_click(&mut self) {
        self.pending_click = None;
    }

    pub fn mouse(&mut self, event: MouseEvent) {
        self.mouse_at(event, Instant::now());
    }

    fn mouse_at(&mut self, event: MouseEvent, now: Instant) {
        if self.state.value("mouse") != "开启" {
            self.cancel_pending_click();
            return;
        }
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let previous = self.pending_click.take();
                let Some((area, action)) = self
                    .hits
                    .iter()
                    .rev()
                    .find(|(r, _)| r.contains((event.column, event.row).into()))
                    .cloned()
                else {
                    return;
                };
                // Only selectable rows need a double click; buttons already act on a single click.
                let target = match (&action, &self.modal) {
                    (Action::HomeFocus(HomeFocus::Profile), None) if self.page == Page::Home => {
                        Some(ClickTarget::HomeProfile(self.state.active_name().into()))
                    }
                    (Action::Select(index), None) => {
                        self.rows().get(*index).map(|row| ClickTarget::Row {
                            page: self.page,
                            sub: self.sub,
                            id: row.id,
                            cells: row.cells.clone(),
                        })
                    }
                    (Action::BackupSelect(index), Some(Modal::Backups { .. })) => self
                        .state
                        .backups
                        .get(*index)
                        .map(|backup| ClickTarget::Backup {
                            index: *index,
                            name: backup.name.clone(),
                        }),
                    _ => None,
                };
                let double_click = match (&previous, &target) {
                    (Some(previous), Some(target)) => {
                        previous.target == *target
                            && previous.area == area
                            && previous.row == event.row
                            && previous.column.abs_diff(event.column) <= 2
                            && now.duration_since(previous.at) <= DOUBLE_CLICK_INTERVAL
                    }
                    _ => false,
                };
                self.action(action);
                if double_click {
                    // Use the same handler as Enter, including any confirmation dialog it opens.
                    self.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                } else if let Some(target) = target {
                    self.pending_click = Some(PendingClick {
                        target,
                        area,
                        column: event.column,
                        row: event.row,
                        at: now,
                    });
                }
            }
            MouseEventKind::ScrollDown => {
                self.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            }
            MouseEventKind::ScrollUp => self.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            MouseEventKind::Down(_) | MouseEventKind::Drag(_) => self.cancel_pending_click(),
            _ => {}
        }
    }
    pub fn action(&mut self, action: Action) {
        self.cancel_pending_click();
        match action {
            Action::Page(p) => {
                self.modal = None;
                self.navigate(p);
            }
            Action::Sub(i) => {
                self.sub = i;
                self.selected = 0;
            }
            Action::Select(i) => {
                self.selected = i;
                if self.page == Page::Home {
                    self.home_focus = HomeFocus::Controls;
                }
            }
            Action::HomeFocus(focus) => self.home_focus = focus,
            Action::ProfileButton => {
                if self.page == Page::Home {
                    if self.home_focus == HomeFocus::Profile {
                        self.activate();
                    } else {
                        self.home_focus = HomeFocus::Profile;
                    }
                }
            }
            Action::Activate => self.activate(),
            Action::Key(c) => self.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
            Action::Field(i) => {
                if let Some(Modal::Form { active, fields, .. }) = &mut self.modal {
                    *active = i;
                    if matches!(fields[i].kind, Kind::Choice(_) | Kind::Toggle) {
                        fields[i].cycle(false);
                    }
                }
            }
            Action::BackupSelect(i) => {
                if let Some(Modal::Backups { selected }) = &mut self.modal {
                    *selected = i;
                }
            }
            Action::Submit => {
                self.modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
            }
            Action::Cancel => self.modal = None,
        }
    }
    pub fn paste(&mut self, text: &str) {
        self.cancel_pending_click();
        if let Some(Modal::Form { fields, active, .. }) = &mut self.modal {
            fields[*active].insert(text);
        } else if self.searching {
            self.query
                .extend(text.chars().filter(|c| !c.is_control()).take(256));
            self.selected = 0;
        }
    }
    fn modal_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.modal = None;
            return;
        }
        if let Some(Modal::Backups { selected }) = self.modal.clone() {
            match key.code {
                KeyCode::Up => {
                    self.modal = Some(Modal::Backups {
                        selected: selected.saturating_sub(1),
                    })
                }
                KeyCode::Down => {
                    self.modal = Some(Modal::Backups {
                        selected: (selected + 1).min(self.state.backups.len().saturating_sub(1)),
                    })
                }
                KeyCode::Char('b') => {
                    self.backup();
                    self.modal = Some(Modal::Backups {
                        selected: self.state.backups.len() - 1,
                    });
                }
                KeyCode::Char('e') => {
                    let sections = settings::sections();
                    let id = sections
                        .iter()
                        .position(|s| s.name == "备份与恢复")
                        .unwrap();
                    let fields = sections[id]
                        .fields
                        .iter()
                        .map(|s| Field::from_spec(s, self.state.value(s.key)))
                        .collect();
                    self.form("WebDAV 与自动备份", fields, SaveTarget::Settings(id));
                }
                KeyCode::Enter if selected < self.state.backups.len() => self.confirm(
                    "恢复备份",
                    &format!(
                        "恢复 {}？将覆盖当前演示设置、订阅、增强链与规则。",
                        self.state.backups[selected].name
                    ),
                    Confirm::Restore(selected),
                ),
                KeyCode::Char('d') if selected < self.state.backups.len() => self.confirm(
                    "删除备份",
                    &format!("删除 {}？", self.state.backups[selected].name),
                    Confirm::Backup(selected),
                ),
                _ => {}
            }
            return;
        }
        let save = key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL);
        if save && matches!(self.modal, Some(Modal::Form { .. })) {
            self.save_form();
            return;
        }
        let mut confirm = None;
        let mut destination = None;
        match self.modal.as_mut().unwrap() {
            Modal::Backups { .. } => unreachable!("backup input handled above"),
            Modal::Form { fields, active, .. } => match key.code {
                KeyCode::Tab | KeyCode::Down => *active = (*active + 1) % fields.len(),
                KeyCode::BackTab | KeyCode::Up => {
                    *active = (*active + fields.len() - 1) % fields.len()
                }
                KeyCode::Enter if !matches!(fields[*active].kind, Kind::Multiline) => {
                    if matches!(fields[*active].kind, Kind::Choice(_) | Kind::Toggle) {
                        fields[*active].cycle(false);
                    } else {
                        *active = (*active + 1) % fields.len();
                    }
                }
                _ => fields[*active].key(key),
            },
            Modal::Detail { scroll, body, .. } => match key.code {
                KeyCode::Down | KeyCode::Char('j') => {
                    *scroll = scroll
                        .saturating_add(1)
                        .min(body.lines().count().saturating_sub(1) as u16)
                }
                KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                KeyCode::PageDown => {
                    *scroll = scroll
                        .saturating_add(10)
                        .min(body.lines().count().saturating_sub(1) as u16)
                }
                KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                KeyCode::Home => *scroll = 0,
                _ => {}
            },
            Modal::Confirm { target, .. } => {
                if key.code == KeyCode::Enter || save {
                    confirm = Some(target.clone());
                }
            }
            Modal::Palette { query, selected } => match key.code {
                KeyCode::Down => {
                    *selected =
                        (*selected + 1).min(Self::palette_entries(query).len().saturating_sub(1))
                }
                KeyCode::Up => *selected = selected.saturating_sub(1),
                KeyCode::Backspace => {
                    query.pop();
                    *selected = 0;
                }
                KeyCode::Char(c) => {
                    query.push(c);
                    *selected = 0;
                }
                KeyCode::Enter => {
                    destination = Self::palette_entries(query).get(*selected).copied()
                }
                _ => {}
            },
        }
        if let Some(target) = confirm {
            self.execute_confirm(target);
            self.modal = None;
        }
        if let Some(page) = destination {
            self.modal = None;
            self.navigate(page);
        }
    }
    pub fn activate(&mut self) {
        if self.page == Page::Home && self.home_focus == HomeFocus::Profile {
            self.navigate(Page::Profiles);
            return;
        }
        let Some(id) = self.selected_id() else { return };
        match self.page {
            Page::Home=>match id {0=>{self.state.toggle("system_proxy");self.note("演示：系统代理状态已切换");},1=>{self.state.toggle("tun");self.note("演示：TUN 状态已切换");},2=>self.command('m'),3=>self.navigate(Page::Profiles),4=>self.runtime(),_=>self.environment()},
            Page::Proxies=>{self.state.groups[self.sub].selected=self.state.nodes[id].name.clone();if self.state.value("close_connections")=="开启" {self.state.connections.clear();}self.note(format!("演示：{} → {}",self.state.groups[self.sub].name,self.state.nodes[id].name));},
            Page::Profiles if self.sub==0=>{self.state.active_profile=id;self.note(format!("演示：已选择订阅 {}",self.state.profiles[id].name));},
            Page::Profiles=>{self.state.enhancements[id].enabled = !self.state.enhancements[id].enabled;self.note("演示：配置增强状态已切换");},
            Page::Connections=>{let c=&self.state.connections[id];self.detail("连接详情",format!("连接 #{}\n\n目标地址    {}\n进程        {}\n网络        {}\n出站链      {}\n命中规则    {}\n下载        {}\n上传        {}\n\n数据来源    本地演示",c.id,c.host,c.process,c.network,c.chain,c.rule,c.down,c.up));},
            Page::Rules if self.sub==0=>{self.state.rules[id].enabled = !self.state.rules[id].enabled;self.note("演示：路由规则状态已切换");},
            Page::Rules=>self.detail("规则集合详情",format!("名称：{}\n来源：HTTP\n状态：本地演示\n\n集合更新入口：r\n实际下载将在 mihomo 接入阶段实现。",self.rows()[self.selected].cells[0])),
            Page::Logs=>{let l=&self.state.logs[id];self.detail(format!("{} · {}",l.level,l.time),l.message.clone());},
            Page::Unlock=>self.run_unlock(vec![id]),
            Page::Settings=>self.open_setting(id),
        }
    }
    pub fn command(&mut self, c: char) {
        let id = self.selected_id();
        match c {
            'm' if matches!(self.page, Page::Home | Page::Proxies) => {
                self.state.mode = (self.state.mode + 1) % 3;
                self.note(format!(
                    "演示：代理模式切换为 {}",
                    ["规则", "全局", "直连"][self.state.mode]
                ));
            }
            's' if matches!(self.page, Page::Proxies | Page::Connections) => {
                self.sort = !self.sort;
                self.selected = 0;
            }
            'p' if self.page == Page::Logs => {
                self.paused = !self.paused;
                self.status = if self.paused {
                    "日志已暂停"
                } else {
                    "日志已继续"
                }
                .into();
            }
            'a' if self.page == Page::Profiles => self.edit_profile(None),
            'a' if self.page == Page::Rules && self.sub == 0 => self.edit_rule(None),
            'e' if self.page == Page::Profiles => {
                if let Some(i) = id {
                    self.edit_profile(Some(i));
                }
            }
            'e' if self.page == Page::Rules && self.sub == 0 => {
                if let Some(i) = id {
                    self.edit_rule(Some(i));
                }
            }
            'e' if self.page == Page::Settings => self.activate(),
            'v' if self.page == Page::Profiles && self.sub == 0 => {
                if let Some(i) = id {
                    self.form(
                        "编辑订阅配置",
                        vec![Field::new(
                            "content",
                            "YAML 配置",
                            &self.state.profiles[i].content,
                            Kind::Multiline,
                        )],
                        SaveTarget::ProfileContent(i),
                    );
                }
            }
            'd' if self.page == Page::Profiles => {
                if let Some(i) = id {
                    let target = if self.sub == 0 {
                        Confirm::Profile(i)
                    } else {
                        Confirm::Enhancement(i)
                    };
                    self.confirm(
                        "删除条目",
                        "删除当前演示条目？此操作只修改本地演示状态。",
                        target,
                    );
                }
            }
            'd' if self.page == Page::Connections => {
                if let Some(i) = id {
                    self.confirm(
                        "关闭连接",
                        "从演示连接列表移除该会话？",
                        Confirm::Connection(i),
                    );
                }
            }
            'D' if self.page == Page::Connections => self.confirm(
                "关闭全部连接",
                "清空当前全部演示连接？",
                Confirm::AllConnections,
            ),
            'd' if self.page == Page::Rules && self.sub == 0 => {
                if let Some(i) = id {
                    self.confirm("删除规则", "删除当前演示路由规则？", Confirm::Rule(i));
                }
            }
            'c' if self.page == Page::Logs => {
                self.confirm("清空日志", "清空本地演示日志？", Confirm::Logs)
            }
            'r' => self.refresh(),
            '[' | ']' if self.page == Page::Profiles => {
                if let Some(i) = id {
                    let len = if self.sub == 0 {
                        self.state.profiles.len()
                    } else {
                        self.state.enhancements.len()
                    };
                    let other = if c == '[' {
                        i.saturating_sub(1)
                    } else {
                        (i + 1).min(len.saturating_sub(1))
                    };
                    if self.sub == 0 {
                        self.state.profiles.swap(i, other);
                        if self.state.active_profile == i {
                            self.state.active_profile = other;
                        } else if self.state.active_profile == other {
                            self.state.active_profile = i;
                        }
                    } else {
                        self.state.enhancements.swap(i, other);
                    }
                    self.query.clear();
                    self.selected = other;
                    self.note("演示：条目顺序已更新");
                }
            }
            'b' if self.page == Page::Settings => self.backup(),
            'R' if self.page == Page::Settings => {
                if let Some(i) = self.state.backups.len().checked_sub(1) {
                    self.confirm(
                        "恢复最近备份",
                        "恢复最近一次本地演示快照中的设置、订阅、增强链和规则？",
                        Confirm::Restore(i),
                    );
                } else {
                    self.note("尚无备份；按 b 创建本地演示快照");
                }
            }
            't' => {
                let value = if self.state.value("theme") == "深色" {
                    "浅色"
                } else {
                    "深色"
                };
                self.state.settings.insert("theme".into(), value.into());
                self.note("界面主题已切换");
            }
            _ => {}
        }
    }
    fn refresh(&mut self) {
        match self.page {
        Page::Proxies=>{for (i,node) in self.state.nodes.iter_mut().enumerate() {node.delay=if i==7 {None} else {Some(25+i as u16*21+(self.tick%17) as u16)};}self.note("演示测速完成 · 延迟为本地生成数据");},
        Page::Profiles if self.sub==0=>{if let Some(i)=self.selected_id() {self.state.profiles[i].updated=format!("演示刷新 #{:03}",self.tick);self.note("演示订阅刷新完成；未发起网络请求");}},
        Page::Connections=>{self.state.connections=DemoState::default().connections;self.note("已重新载入示例连接");},
        Page::Unlock=>self.run_unlock(self.rows().iter().map(|r|r.id).collect()),
        Page::Rules=>self.note("演示：规则集合刷新流程已完成；未下载规则"),
        Page::Settings=>self.detail("更新检查",format!("当前版本：v{}\n\n这是界面预览版，未连接更新服务器。\n发布记录请查看项目 CHANGELOG.md。",env!("CARGO_PKG_VERSION"))),
        _=>{}
    }
    }
    fn run_unlock(&mut self, ids: Vec<usize>) {
        for id in ids {
            let u = &mut self.state.unlocks[id];
            u.result = if id == 4 {
                "演示 · 不可用"
            } else {
                "演示 · 可用"
            }
            .into();
            u.region = if id > 4 { "US" } else { "JP" }.into();
        }
        self.note("演示检测完成 · 结果不代表实际网络可用性");
    }
    fn edit_profile(&mut self, id: Option<usize>) {
        if self.sub == 0 {
            let p = id.map(|i| &self.state.profiles[i]);
            self.form(
                if id.is_some() {
                    "编辑订阅"
                } else {
                    "导入订阅"
                },
                vec![
                    Field::new(
                        "name",
                        "订阅名称",
                        p.map(|p| p.name.as_str()).unwrap_or(""),
                        Kind::Text,
                    ),
                    Field::new(
                        "url",
                        "订阅 URL（仅保存）",
                        p.map(|p| p.url.as_str()).unwrap_or("https://"),
                        Kind::Text,
                    ),
                    Field::new(
                        "interval",
                        "更新间隔 / 分钟",
                        p.map(|p| p.interval.as_str()).unwrap_or("720"),
                        Kind::Number,
                    ),
                ],
                SaveTarget::Profile(id),
            );
        } else {
            let e = id.map(|i| &self.state.enhancements[i]);
            self.form(
                "配置增强",
                vec![
                    Field::new(
                        "name",
                        "名称",
                        e.map(|e| e.name.as_str()).unwrap_or(""),
                        Kind::Text,
                    ),
                    Field::new(
                        "kind",
                        "类型",
                        e.map(|e| e.kind.as_str()).unwrap_or("YAML"),
                        Kind::Choice(&["YAML", "JavaScript"]),
                    ),
                    Field::new(
                        "content",
                        "内容（预览版不执行）",
                        e.map(|e| e.content.as_str()).unwrap_or("# 配置增强\n"),
                        Kind::Multiline,
                    ),
                ],
                SaveTarget::Enhancement(id),
            );
        }
    }
    fn edit_rule(&mut self, id: Option<usize>) {
        let r = id.map(|i| &self.state.rules[i]);
        self.form(
            "路由规则",
            vec![
                Field::new(
                    "kind",
                    "规则类型",
                    r.map(|r| r.kind.as_str()).unwrap_or("DOMAIN-SUFFIX"),
                    Kind::Choice(&[
                        "DOMAIN",
                        "DOMAIN-SUFFIX",
                        "DOMAIN-KEYWORD",
                        "IP-CIDR",
                        "GEOIP",
                        "RULE-SET",
                        "MATCH",
                    ]),
                ),
                Field::new(
                    "payload",
                    "匹配内容",
                    r.map(|r| r.payload.as_str()).unwrap_or(""),
                    Kind::Text,
                ),
                Field::new(
                    "target",
                    "出站策略",
                    r.map(|r| r.target.as_str()).unwrap_or("节点选择"),
                    Kind::Choice(&[
                        "节点选择",
                        "自动选择",
                        "流媒体",
                        "AI 服务",
                        "DIRECT",
                        "REJECT",
                    ]),
                ),
            ],
            SaveTarget::Rule(id),
        );
    }
    fn open_setting(&mut self, id: usize) {
        let section = settings::sections().remove(id);
        if section.name == "备份与恢复" {
            self.modal = Some(Modal::Backups {
                selected: self.state.backups.len().saturating_sub(1),
            });
            return;
        }
        if !section.fields.is_empty() {
            let fields = section
                .fields
                .iter()
                .map(|s| Field::from_spec(s, self.state.value(s.key)))
                .collect();
            self.form(section.name, fields, SaveTarget::Settings(id));
            return;
        }
        match section.name {
            "运行配置"=>self.runtime(),
            "诊断与目录"=>self.detail("诊断与目录",format!("Clash Verge TUI v{}\n状态目录：{}\n状态文件：demo-state.json\n数据模式：本地演示\n后端连接：未接入\n备份数量：{}\n\n未读取 Clash Verge 的真实配置、密钥或订阅。\n网络设置仅保存为演示值；实际生效的是主题、导航、\n图表、鼠标、Vim 键位、启动页与刷新间隔。",env!("CARGO_PKG_VERSION"),self.data_dir.display(),self.state.backups.len())),
            "桌面功能映射"=>self.detail("桌面功能映射","终端适配\n\n桌面导航 → 数字键 1–8 / 鼠标侧栏\n托盘快捷操作 → 首页快捷控制\n全局热键 → 终端内快捷键\n配置编辑器 → 多行表单\n文件选择 → 路径与文本输入\n开发者工具 → 诊断页 / --snapshot\n\n不适用的视觉设置\n窗口标题栏、托盘图标、字体、CSS 注入由终端或桌面管理。\n终端语言：简体中文。v0.1.0 未提供多语言切换。\n\n后续系统集成\nTUN 权限、代理守卫、后台服务、系统自启与真实备份同步。"),
            _=>self.detail("关于 Clash Verge TUI",format!("CLASH VERGE / TERMINAL\n\nv{}  ·  UI PREVIEW\n\n独立的终端客户端界面，以 Clash Verge Rev v2.5.2 为参照。\nRust + Ratatui + Crossterm\nMIT\n\n本版本覆盖八个主页面和常用二级设置表单。\n网络能力尚未接入；所有网络指标明确标记为演示。\n\n源码参考：https://github.com/clash-verge-rev/clash-verge-rev\n内核计划：https://github.com/MetaCubeX/mihomo",env!("CARGO_PKG_VERSION"))),
        }
    }
    fn runtime(&mut self) {
        let content = self
            .state
            .profiles
            .get(self.state.active_profile)
            .map(|p| p.content.as_str())
            .unwrap_or("# 未选择订阅");
        self.detail("运行配置预览",format!("# 本地演示；增强链尚未实际执行\n# 订阅：{}\n# 界面模式：{}\n# Mixed 端口：{}\n\n{}",self.state.active_name(),["rule","global","direct"][self.state.mode],self.state.value("mixed_port"),content));
    }
    fn environment(&mut self) {
        let host = self.state.value("proxy_host");
        let port = self.state.value("mixed_port");
        let url = format!("http://{host}:{port}");
        let text = match self.state.value("env_type") {
            "fish" => format!("set -gx http_proxy {url}\nset -gx https_proxy {url}"),
            "powershell" => format!("$env:http_proxy = '{url}'\n$env:https_proxy = '{url}'"),
            _ => format!("export http_proxy='{url}'\nexport https_proxy='{url}'"),
        };
        self.detail("环境变量预览",format!("# 基于演示设置生成；复制前请核对实际代理端口\n\n{text}\n\n# 终端可使用 Shift + 鼠标选择文本"));
    }
    fn backup(&mut self) {
        self.state.backups.push(Backup {
            name: format!(
                "演示快照 {:03}",
                self.state
                    .backups
                    .iter()
                    .filter_map(|b| b.name.split_whitespace().last()?.parse::<u32>().ok())
                    .max()
                    .unwrap_or(0)
                    + 1
            ),
            settings: self.state.settings.clone(),
            profiles: self.state.profiles.clone(),
            active_profile: self.state.active_profile,
            enhancements: self.state.enhancements.clone(),
            rules: self.state.rules.clone(),
        });
        if self.state.backups.len() > 10 {
            self.state.backups.remove(0);
        }
        self.note("本地演示快照已创建 · R 恢复最近备份 · 最多保留 10 份");
    }
    fn execute_confirm(&mut self, target: Confirm) {
        match target {
            Confirm::Backup(i) => {
                self.state.backups.remove(i);
            }
            Confirm::Profile(i) => {
                self.state.profiles.remove(i);
                if i < self.state.active_profile {
                    self.state.active_profile -= 1;
                }
                self.state.active_profile = self
                    .state
                    .active_profile
                    .min(self.state.profiles.len().saturating_sub(1));
            }
            Confirm::Enhancement(i) => {
                self.state.enhancements.remove(i);
            }
            Confirm::Connection(i) => {
                self.state.connections.remove(i);
            }
            Confirm::AllConnections => self.state.connections.clear(),
            Confirm::Rule(i) => {
                self.state.rules.remove(i);
            }
            Confirm::Logs => self.state.logs.clear(),
            Confirm::Restore(i) => {
                let b = self.state.backups[i].clone();
                self.state.settings = b.settings;
                self.state.profiles = b.profiles;
                self.state.active_profile = b.active_profile;
                self.state.enhancements = b.enhancements;
                self.state.rules = b.rules;
            }
        }
        self.selected = self.selected.min(self.rows().len().saturating_sub(1));
        self.note("本地演示操作已完成");
    }
    fn validate(fields: &[Field], target: &SaveTarget) -> Result<(), String> {
        for f in fields {
            if matches!(f.kind, Kind::Number) {
                let n = f
                    .value
                    .parse::<u32>()
                    .map_err(|_| format!("{}：请输入非负整数", f.label))?;
                if f.key.ends_with("_port") && (n > 65535 || (f.key == "mixed_port" && n == 0)) {
                    return Err(format!("{}：端口超出范围", f.label));
                }
                if !f.key.ends_with("_port") && n == 0 {
                    return Err(format!("{}：必须大于 0", f.label));
                }
                if f.key == "refresh" && n < 100 {
                    return Err("刷新间隔不能小于 100 毫秒".into());
                }
            }
            if (f.key == "name" || f.key == "payload") && f.value.trim().is_empty() {
                return Err(format!("{}不能为空", f.label));
            }
            if f.key == "url"
                && !(f.value.starts_with("https://") || f.value.starts_with("http://"))
            {
                return Err("订阅 URL 必须以 http:// 或 https:// 开头".into());
            }
            if f.key == "url"
                && (f
                    .value
                    .split("://")
                    .nth(1)
                    .unwrap_or("")
                    .split('/')
                    .next()
                    .unwrap_or("")
                    .is_empty()
                    || f.value.chars().any(char::is_whitespace))
            {
                return Err("请输入完整且不含空白的订阅 URL".into());
            }
        }
        if matches!(target, SaveTarget::ProfileContent(_)) && fields[0].value.trim().is_empty() {
            return Err("配置内容不能为空".into());
        }
        Ok(())
    }
    fn save_form(&mut self) {
        let Some(Modal::Form { fields, target, .. }) = self.modal.as_ref() else {
            return;
        };
        if let Err(message) = Self::validate(fields, target) {
            if let Some(Modal::Form { error, .. }) = &mut self.modal {
                *error = message;
            }
            return;
        }
        let fields = fields.clone();
        let target = target.clone();
        let get = |key: &str| {
            fields
                .iter()
                .find(|f| f.key == key)
                .map(|f| f.value.clone())
                .unwrap_or_default()
        };
        match target {
            SaveTarget::Settings(_) => {
                for f in fields {
                    self.state.settings.insert(f.key, f.value);
                }
            }
            SaveTarget::Profile(id) => {
                if let Some(i) = id {
                    let p = &mut self.state.profiles[i];
                    p.name = get("name");
                    p.url = get("url");
                    p.interval = get("interval");
                } else {
                    self.state.profiles.push(Profile {
                        name: get("name"),
                        url: get("url"),
                        interval: get("interval"),
                        used: 0,
                        total: 100,
                        updated: "未更新 · 演示".into(),
                        content: "# 新建演示订阅\nmode: rule\n".into(),
                    });
                }
            }
            SaveTarget::Enhancement(id) => {
                let e = Enhancement {
                    name: get("name"),
                    kind: get("kind"),
                    content: get("content"),
                    enabled: id
                        .map(|i| self.state.enhancements[i].enabled)
                        .unwrap_or(true),
                };
                if let Some(i) = id {
                    self.state.enhancements[i] = e;
                } else {
                    self.state.enhancements.push(e);
                }
            }
            SaveTarget::Rule(id) => {
                let r = Rule {
                    kind: get("kind"),
                    payload: get("payload"),
                    target: get("target"),
                    enabled: id.map(|i| self.state.rules[i].enabled).unwrap_or(true),
                };
                if let Some(i) = id {
                    self.state.rules[i] = r;
                } else {
                    self.state.rules.push(r);
                }
            }
            SaveTarget::ProfileContent(i) => self.state.profiles[i].content = get("content"),
        }
        self.modal = None;
        self.note("已保存到本地演示状态 · 网络配置尚未应用到 mihomo");
    }
    pub fn tick(&mut self) {
        self.tick += 1;
        if !self.paused && self.tick.is_multiple_of(10) {
            self.state.logs.push(Log {
                time: format!("00:{:02}:{:02}", self.tick / 60 % 60, self.tick % 60),
                level: "INFO".into(),
                message: format!(
                    "[演示] 状态采样 #{} · {} 个连接",
                    self.tick,
                    self.state.connections.len()
                ),
            });
            if self.state.logs.len() > 300 {
                self.state.logs.remove(0);
            }
            self.dirty = true;
        }
        self.selected = self.selected.min(self.rows().len().saturating_sub(1));
    }
    pub fn help(&mut self) {
        self.detail("键盘操作","导航\n  1–8               切换主页面\n  Tab / Shift+Tab   切换区域或分组\n  ← → / h l         首页左右区域；其他页面切换分组\n  ↑ ↓ / j k         选择条目\n  PgUp / PgDn       快速翻页\n  Enter / Space     执行主操作\n  /                 搜索当前列表\n  :                 页面跳转面板\n  Esc               取消弹窗 / 清除搜索\n  t                 切换深色 / 浅色主题\n  q / Ctrl+C        退出\n\n页面操作\n  a / e / d         新建 / 编辑 / 删除\n  r                 演示刷新或检测\n  s                 排序（代理 / 连接）\n  m                 代理模式切换\n  v                 编辑订阅 YAML\n  [ / ]             上移 / 下移订阅或增强链\n  D                 关闭全部演示连接\n  p / c             暂停 / 清空日志\n  b / R             创建 / 恢复最近演示备份（设置页）\n\n表单\n  Tab / ↑ ↓         切换字段\n  ← → / Space       切换开关或选项\n  Home / End        文本首尾\n  Ctrl+U            清空当前字段\n  Enter             下一字段；多行字段换行\n  Ctrl+S            校验并保存\n\n鼠标\n  点击侧栏、标签、工具按钮；单击行选择，双击等同 Enter\n  同一行同一位置附近 400 毫秒内双击；备份恢复仍需确认\n  滚轮移动选择；Shift+鼠标使用终端原生文本选择\n\n所有网络行为均为本地演示。表单只做基础校验，\nYAML、JavaScript 与完整 mihomo 配置校验留待内核接入。" );
    }
}

#[cfg(test)]
mod mouse_tests {
    use super::*;

    fn fixture() -> (App, MouseEvent, Instant) {
        let mut app = App::new(DemoState::default(), PathBuf::from("/tmp/demo"));
        app.hits.push((Rect::new(20, 10, 30, 1), Action::Select(0)));
        let event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 24,
            row: 10,
            modifiers: KeyModifiers::NONE,
        };
        (app, event, Instant::now())
    }

    #[test]
    fn double_click_allows_small_motion_and_release_but_does_not_repeat_on_third_click() {
        let (mut app, mut event, start) = fixture();
        app.mouse_at(event, start);
        assert_eq!(app.state.value("system_proxy"), "关闭");
        event.kind = MouseEventKind::Up(MouseButton::Left);
        app.mouse_at(event, start + Duration::from_millis(10));
        event.kind = MouseEventKind::Down(MouseButton::Left);
        event.column += 1;
        app.mouse_at(event, start + Duration::from_millis(150));
        assert_eq!(app.state.value("system_proxy"), "开启");
        app.mouse_at(event, start + Duration::from_millis(200));
        assert_eq!(app.state.value("system_proxy"), "开启");
    }

    #[test]
    fn delayed_or_distant_clicks_are_only_selection() {
        for (delay, shift) in [(401, 0), (100, 8)] {
            let (mut app, mut event, start) = fixture();
            app.mouse_at(event, start);
            event.column += shift;
            app.mouse_at(event, start + Duration::from_millis(delay));
            assert_eq!(app.state.value("system_proxy"), "关闭");
        }
    }

    #[test]
    fn intervening_input_navigation_and_resize_cancel_detection() {
        for interruption in 0..6 {
            let (mut app, event, start) = fixture();
            app.mouse_at(event, start);
            match interruption {
                0 => app.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
                1 => app.cancel_pending_click(),
                2 => app.mouse_at(
                    MouseEvent {
                        kind: MouseEventKind::ScrollDown,
                        ..event
                    },
                    start,
                ),
                3 => app.mouse_at(
                    MouseEvent {
                        kind: MouseEventKind::Drag(MouseButton::Left),
                        ..event
                    },
                    start,
                ),
                4 => app.navigate(Page::Home),
                _ => app.mouse_at(
                    MouseEvent {
                        column: 0,
                        row: 0,
                        ..event
                    },
                    start,
                ),
            }
            app.mouse_at(event, start + Duration::from_millis(100));
            assert_eq!(app.state.value("system_proxy"), "关闭");
        }
    }

    #[test]
    fn same_screen_location_with_a_different_row_never_activates() {
        let (mut app, event, start) = fixture();
        app.mouse_at(event, start);
        app.hits[0].1 = Action::Select(1);
        app.mouse_at(event, start + Duration::from_millis(100));
        assert_eq!(app.selected, 1);
        assert_eq!(app.state.value("tun"), "关闭");
        assert_eq!(app.state.value("system_proxy"), "关闭");
    }

    #[test]
    fn mouse_disabled_does_not_select_or_activate() {
        let (mut app, event, start) = fixture();
        app.state.settings.insert("mouse".into(), "关闭".into());
        app.selected = 1;
        app.mouse_at(event, start);
        app.mouse_at(event, start + Duration::from_millis(100));
        assert_eq!(app.selected, 1);
        assert_eq!(app.state.value("system_proxy"), "关闭");
    }
}
