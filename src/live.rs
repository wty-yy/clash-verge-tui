use crate::{
    app::{App, Confirm, Field, SaveTarget},
    core::{Command, CoreEvent, LogEvent, Snapshot},
    model::*,
    settings::{self},
    subscriptions::{self, StoredProfile},
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    path::PathBuf,
    time::Instant,
};

pub struct ManagedSettings {
    pub controller: String,
    pub secret: String,
    pub port: u16,
}
pub struct LiveState {
    pub endpoint: String,
    pub connected: bool,
    pub pending: bool,
    pub version: String,
    pub error: String,
    pub log_status: String,
    pub config: Value,
    pub members: BTreeMap<String, Vec<String>>,
    pub connection_ids: Vec<String>,
    pub rule_indices: Vec<usize>,
    pub providers: Vec<Vec<String>>,
    pub downloaded: u64,
    pub uploaded: u64,
    pub memory: Option<u64>,
    pub down_rate: Option<u64>,
    pub up_rate: Option<u64>,
    pub history: VecDeque<u64>,
    pub sample_at: Option<Instant>,
    pub outbox: Vec<Command>,
    pub profiles: Vec<StoredProfile>,
    pub managed: Option<ManagedSettings>,
    pub pending_profile: Option<usize>,
}
impl LiveState {
    pub fn new(
        endpoint: String,
        profiles: Vec<StoredProfile>,
        managed: Option<ManagedSettings>,
    ) -> Self {
        Self {
            endpoint,
            connected: false,
            pending: false,
            version: String::new(),
            error: String::new(),
            log_status: "日志流连接中".into(),
            config: Value::Null,
            members: BTreeMap::new(),
            connection_ids: vec![],
            rule_indices: vec![],
            providers: vec![],
            downloaded: 0,
            uploaded: 0,
            memory: None,
            down_rate: None,
            up_rate: None,
            history: VecDeque::new(),
            sample_at: None,
            outbox: vec![],
            profiles,
            managed,
            pending_profile: None,
        }
    }
}
pub fn bytes(value: u64) -> String {
    let n = value as f64;
    for (unit, scale) in [("GiB", 1073741824.0), ("MiB", 1048576.0), ("KiB", 1024.0)] {
        if n >= scale {
            return format!("{:.2} {unit}", n / scale);
        }
    }
    format!("{value} B")
}
fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}
fn arr(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn flag(value: bool) -> String {
    if value { "开启" } else { "关闭" }.into()
}
pub const UI_KEYS: &[&str] = &[
    "theme",
    "accent",
    "compact",
    "traffic_graph",
    "memory",
    "start_page",
    "mouse",
    "vim",
    "env_type",
    "refresh",
    "test_url",
    "timeout",
];

impl App {
    pub fn new_live(
        dir: PathBuf,
        endpoint: String,
        profiles: Vec<StoredProfile>,
        active: usize,
        managed: Option<ManagedSettings>,
    ) -> anyhow::Result<Self> {
        let mut state = DemoState::default();
        state.nodes.clear();
        state.groups.clear();
        state.profiles.clear();
        state.enhancements.clear();
        state.connections.clear();
        state.rules.clear();
        state.logs.clear();
        state.unlocks.clear();
        state.backups.clear();
        for profile in &profiles {
            state.profiles.push(Profile {
                name: profile.name.clone(),
                url: profile.url.clone(),
                interval: "—".into(),
                used: 0,
                total: 0,
                updated: "已下载".into(),
                content: String::new(),
            });
        }
        state.active_profile = active.min(profiles.len().saturating_sub(1));
        state
            .settings
            .insert("system_proxy".into(), "未接入".into());
        let file = dir.join("live-preferences.json");
        if file.exists() {
            let preferences: BTreeMap<String, String> = serde_json::from_slice(&fs::read(file)?)
                .map_err(|_| anyhow::anyhow!("界面偏好文件损坏"))?;
            for (key, value) in preferences {
                if UI_KEYS.contains(&key.as_str()) {
                    state.settings.insert(key, value);
                }
            }
        }
        let mut app = Self::new(state, dir);
        app.live = Some(LiveState::new(endpoint, profiles, managed));
        app.status = "正在连接 mihomo…".into();
        Ok(app)
    }
    pub fn save_preferences(&self) -> anyhow::Result<()> {
        let values: BTreeMap<_, _> = self
            .state
            .settings
            .iter()
            .filter(|(k, _)| UI_KEYS.contains(&k.as_str()))
            .collect();
        subscriptions::private_write(
            &self.data_dir.join("live-preferences.json"),
            &serde_json::to_vec_pretty(&values)?,
        )
    }
    pub fn handle_core(&mut self, event: CoreEvent) {
        match event {
            CoreEvent::Snapshot(snapshot) => self.apply_snapshot(*snapshot),
            CoreEvent::Offline(error) => {
                let live = self.live.as_mut().unwrap();
                live.connected = false;
                live.error = error.clone();
                live.sample_at = None;
                live.down_rate = None;
                live.up_rate = None;
                self.status = format!("连接断开，自动重试：{error}");
            }
            CoreEvent::Completed(result) => {
                let live = self.live.as_mut().unwrap();
                live.pending = false;
                let profile = live.pending_profile.take();
                match result {
                    Ok(()) => {
                        if let Some(index) = profile {
                            self.state.active_profile = index;
                            let path = self.data_dir.join("profiles/active.json");
                            if subscriptions::private_write(&path, index.to_string().as_bytes())
                                .is_err()
                            {
                                self.status = "订阅已应用，但保存当前订阅索引失败".into();
                                return;
                            }
                        }
                        self.status = if live.connected {
                            "mihomo 操作已完成"
                        } else {
                            "操作已提交；状态刷新失败，正在重连"
                        }
                        .into();
                    }
                    Err(error) => self.status = format!("操作失败：{error}"),
                }
            }
        }
    }
    pub fn handle_log(&mut self, event: LogEvent) {
        match event {
            LogEvent::Status(status) => self.live.as_mut().unwrap().log_status = status,
            LogEvent::Entry { level, message } => {
                if self.paused {
                    return;
                }
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                self.state.logs.push(Log {
                    time: format!(
                        "{:02}:{:02}:{:02}",
                        now / 3600 % 24,
                        now / 60 % 60,
                        now % 60
                    ),
                    level,
                    message,
                });
                if self.state.logs.len() > 300 {
                    self.state.logs.remove(0);
                }
            }
        }
    }
    fn apply_snapshot(&mut self, snapshot: Snapshot) {
        let previous_group = self.state.groups.get(self.sub).map(|g| g.name.clone());
        let previous_node = if self.page == Page::Proxies {
            self.selected_id()
                .and_then(|i| self.state.nodes.get(i))
                .map(|n| n.name.clone())
        } else {
            None
        };
        let live = self.live.as_mut().unwrap();
        let was_connected = live.connected;
        let now = Instant::now();
        let down = snapshot.connections["downloadTotal"].as_u64().unwrap_or(0);
        let up = snapshot.connections["uploadTotal"].as_u64().unwrap_or(0);
        if let Some(last) = live.sample_at {
            let elapsed = now.duration_since(last).as_secs_f64().max(0.001);
            if down >= live.downloaded && up >= live.uploaded {
                live.down_rate = Some(((down - live.downloaded) as f64 / elapsed) as u64);
                live.up_rate = Some(((up - live.uploaded) as f64 / elapsed) as u64);
                live.history.push_back(live.down_rate.unwrap());
                if live.history.len() > 60 {
                    live.history.pop_front();
                }
            } else {
                live.down_rate = None;
                live.up_rate = None;
                live.history.clear();
            }
        }
        live.sample_at = Some(now);
        live.downloaded = down;
        live.uploaded = up;
        live.memory = snapshot.connections["memory"].as_u64();
        live.connected = true;
        live.error.clear();
        live.version = snapshot.version;
        self.state.mode = match snapshot.config["mode"].as_str() {
            Some("global") => 1,
            Some("direct") => 2,
            _ => 0,
        };
        for (field, key) in [
            ("allow_lan", "allow-lan"),
            ("ipv6", "ipv6"),
            ("unified_delay", "unified-delay"),
        ] {
            self.state.settings.insert(
                field.into(),
                flag(snapshot.config[key].as_bool().unwrap_or(false)),
            );
        }
        self.state.settings.insert(
            "tun".into(),
            flag(snapshot.config["tun"]["enable"].as_bool().unwrap_or(false)),
        );
        for (field, key) in [
            ("mixed_port", "mixed-port"),
            ("socks_port", "socks-port"),
            ("http_port", "port"),
            ("redir_port", "redir-port"),
            ("tproxy_port", "tproxy-port"),
        ] {
            self.state.settings.insert(
                field.into(),
                snapshot.config[key].as_u64().unwrap_or(0).to_string(),
            );
        }
        self.state.settings.insert(
            "log_level".into(),
            snapshot.config["log-level"]
                .as_str()
                .unwrap_or("info")
                .into(),
        );
        live.config = snapshot.config;
        self.state.nodes.clear();
        self.state.groups.clear();
        live.members.clear();
        if let Some(proxies) = snapshot.proxies["proxies"].as_object() {
            for (name, proxy) in proxies {
                let delay = arr(&proxy["history"])
                    .last()
                    .and_then(|v| v["delay"].as_u64())
                    .filter(|v| *v > 0)
                    .map(|v| v.min(65535) as u16);
                self.state.nodes.push(Node {
                    name: name.clone(),
                    protocol: s(&proxy["type"]),
                    region: "—".into(),
                    delay,
                });
                if proxy["all"].is_array() {
                    self.state.groups.push(Group {
                        name: name.clone(),
                        kind: s(&proxy["type"]),
                        selected: s(&proxy["now"]),
                    });
                    live.members.insert(
                        name.clone(),
                        arr(&proxy["all"])
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect(),
                    );
                }
            }
        }
        self.state.rules.clear();
        live.rule_indices.clear();
        for (i, rule) in arr(&snapshot.rules["rules"]).iter().enumerate() {
            self.state.rules.push(Rule {
                kind: s(&rule["type"]),
                payload: s(&rule["payload"]),
                target: s(&rule["proxy"]),
                enabled: !rule["extra"]["disabled"].as_bool().unwrap_or(false),
            });
            live.rule_indices
                .push(rule["index"].as_u64().map(|n| n as usize).unwrap_or(i));
        }
        self.state.connections.clear();
        live.connection_ids.clear();
        for (i, c) in arr(&snapshot.connections["connections"]).iter().enumerate() {
            let m = &c["metadata"];
            let host = m["host"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or(m["destinationIP"].as_str())
                .unwrap_or("—");
            self.state.connections.push(Connection {
                id: i as u32 + 1,
                host: format!("{host}:{}", m["destinationPort"].as_str().unwrap_or("")),
                process: m["process"]
                    .as_str()
                    .or(m["processPath"].as_str())
                    .unwrap_or("—")
                    .into(),
                network: s(&m["network"]),
                chain: arr(&c["chains"])
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" → "),
                rule: s(&c["rule"]),
                down: bytes(c["download"].as_u64().unwrap_or(0)),
                up: bytes(c["upload"].as_u64().unwrap_or(0)),
            });
            live.connection_ids.push(s(&c["id"]));
        }
        live.providers = snapshot.providers["providers"]
            .as_object()
            .map(|p| {
                p.iter()
                    .map(|(name, v)| {
                        vec![
                            name.clone(),
                            s(&v["behavior"]),
                            v["ruleCount"].as_u64().unwrap_or(0).to_string(),
                            s(&v["vehicleType"]),
                            s(&v["updatedAt"]),
                        ]
                    })
                    .collect()
            })
            .unwrap_or_default();
        if self.page == Page::Proxies {
            self.sub = previous_group
                .and_then(|name| self.state.groups.iter().position(|g| g.name == name))
                .unwrap_or(0);
        }
        if let Some(name) = previous_node {
            self.selected = self
                .rows()
                .iter()
                .position(|row| self.state.nodes[row.id].name == name)
                .unwrap_or(0);
        }
        self.selected = self.selected.min(self.rows().len().saturating_sub(1));
        if !was_connected {
            self.status = "已连接 mihomo · 数据来自真实内核".into();
        }
    }
    pub fn queue_core(&mut self, command: Command) {
        let live = self.live.as_mut().unwrap();
        if !live.connected {
            self.status = "内核尚未连接，等待重连后再操作".into();
            return;
        }
        if live.pending {
            self.status = "上一个内核操作尚未完成".into();
            return;
        }
        live.pending = true;
        live.outbox.push(command);
        self.status = "正在执行 mihomo 操作…".into();
    }
    pub fn activate_live(&mut self) {
        let Some(id) = self.selected_id() else { return };
        match self.page {
            Page::Home => match id {
                0 | 1 => self.detail(
                    "系统集成尚未接入",
                    "当前版本连接 mihomo API；系统代理与 TUN 权限管理尚未实现。",
                ),
                2 => {
                    self.command_live('m');
                }
                3 => self.navigate(Page::Profiles),
                4 => self.runtime_live(),
                _ => self.detail(
                    "代理地址",
                    format!(
                        "Mixed 端口：{}\n请按实际运行机器设置应用的代理地址。",
                        self.state.value("mixed_port")
                    ),
                ),
            },
            Page::Proxies => {
                let Some(group) = self.state.groups.get(self.sub) else {
                    return;
                };
                if !matches!(group.kind.as_str(), "Selector" | "select") {
                    self.status = "该策略组由内核自动选择；此版本只支持手动选择组".into();
                    return;
                }
                self.queue_core(Command::Select {
                    group: group.name.clone(),
                    node: self.state.nodes[id].name.clone(),
                });
            }
            Page::Profiles => {
                if self.sub != 0 {
                    return;
                }
                let live = self.live.as_ref().unwrap();
                let Some(managed) = &live.managed else {
                    self.status = "附加到现有内核时不接管其订阅；请使用 --core 独立运行".into();
                    return;
                };
                let result = fs::read_to_string(&live.profiles[id].file)
                    .map_err(anyhow::Error::from)
                    .and_then(|text| {
                        subscriptions::normalized_config(
                            &text,
                            &managed.controller,
                            &managed.secret,
                            managed.port,
                        )
                    });
                match result {
                    Ok(payload) => {
                        if live.connected && !live.pending {
                            self.live.as_mut().unwrap().pending_profile = Some(id);
                            self.queue_core(Command::Reload(payload));
                        }
                    }
                    Err(_) => self.status = "本地订阅读取或校验失败".into(),
                }
            }
            Page::Connections => {
                let c = &self.state.connections[id];
                self.detail("连接详情",format!("目标地址：{}\n进程：{}\n网络：{}\n出站链：{}\n规则：{}\n下载：{}\n上传：{}\n\n数据来源：mihomo",c.host,c.process,c.network,c.chain,c.rule,c.down,c.up));
            }
            Page::Rules if self.sub == 0 => {
                let index = self.live.as_ref().unwrap().rule_indices[id];
                self.queue_core(Command::DisableRule {
                    index,
                    disabled: self.state.rules[id].enabled,
                });
            }
            Page::Rules => self.detail(
                "规则集合",
                self.live.as_ref().unwrap().providers[id].join("\n"),
            ),
            Page::Logs => {
                let l = &self.state.logs[id];
                self.detail(format!("{} · {} UTC", l.level, l.time), l.message.clone());
            }
            Page::Unlock => self.detail("解锁检测", "真实解锁检测尚未接入。"),
            Page::Settings => {
                let section = settings::sections().remove(id);
                match section.name {
                    "外观与布局" | "热键与终端" => {
                        let fields = section
                            .fields
                            .iter()
                            .filter(|f| UI_KEYS.contains(&f.key))
                            .map(|f| Field::from_spec(f, self.state.value(f.key)))
                            .collect();
                        self.form(section.name, fields, SaveTarget::Settings(id));
                    }
                    "基础网络" => {
                        let fields = section
                            .fields
                            .iter()
                            .filter(|f| ["ipv6", "unified_delay", "log_level"].contains(&f.key))
                            .map(|f| Field::from_spec(f, self.state.value(f.key)))
                            .collect();
                        self.form("内核网络参数", fields, SaveTarget::Settings(id));
                    }
                    "运行配置" => self.runtime_live(),
                    "关于 Clash Verge TUI" | "诊断与目录" => {
                        let live = self.live.as_ref().unwrap();
                        self.detail(section.name,format!("Clash Verge TUI v{}\nmihomo {}\n控制器：{}\n日志：{}\n状态目录：{}\n\n节点切换、模式、测速、连接关闭和规则启停已接入。\nMIT",env!("CARGO_PKG_VERSION"),live.version,live.endpoint,live.log_status,self.data_dir.display()));
                    }
                    _ => self.detail(
                        section.name,
                        "此设置尚未接入真实后端，当前不会修改系统或内核配置。",
                    ),
                }
            }
        }
    }
    pub fn command_live(&mut self, c: char) -> bool {
        match c {
            't' | 's' | 'p' => return false,
            'm' if matches!(self.page, Page::Home | Page::Proxies) => self.queue_core(
                Command::Mode(["rule", "global", "direct"][(self.state.mode + 1) % 3].into()),
            ),
            'r' => match self.page {
                Page::Proxies => {
                    if let Some(g) = self.state.groups.get(self.sub) {
                        self.queue_core(Command::Delay {
                            group: g.name.clone(),
                            url: self.state.value("test_url").into(),
                            timeout: self
                                .state
                                .value("timeout")
                                .parse::<u32>()
                                .unwrap_or(5000)
                                .clamp(100, 30000),
                        });
                    }
                }
                Page::Rules if self.sub == 1 => {
                    if let Some(i) = self.selected_id() {
                        self.queue_core(Command::UpdateProvider(
                            self.live.as_ref().unwrap().providers[i][0].clone(),
                        ));
                    }
                }
                Page::Profiles => self.reload_profile_index(),
                Page::Unlock => self.detail("解锁检测", "真实检测尚未接入。"),
                _ => self.queue_core(Command::Refresh),
            },
            'd' if self.page == Page::Connections => {
                if let Some(i) = self.selected_id() {
                    let id = self.live.as_ref().unwrap().connection_ids[i].clone();
                    self.confirm(
                        "关闭连接",
                        "关闭选中的真实网络连接？",
                        Confirm::CoreConnection(id),
                    );
                }
            }
            'D' if self.page == Page::Connections => self.confirm(
                "关闭全部连接",
                "关闭内核当前的全部网络连接？",
                Confirm::AllConnections,
            ),
            'c' if self.page == Page::Logs => {
                self.state.logs.clear();
                self.status = "已清空界面日志缓存".into();
            }
            'e' if self.page == Page::Settings => self.activate_live(),
            'a' | 'e' | 'd' | 'v' | 'b' | 'R' | '[' | ']' => self.detail(
                "功能尚未接入",
                "当前版本不执行此真实操作。演示模式仍可预览完整交互。",
            ),
            _ => return false,
        }
        true
    }
    fn reload_profile_index(&mut self) {
        let live = self.live.as_mut().unwrap();
        if live.managed.is_none() {
            self.status = "附加模式不接管外部内核的订阅".into();
            return;
        }
        match subscriptions::load_profiles(&self.data_dir.join("profiles")) {
            Ok(profiles) => {
                let active_url = live
                    .profiles
                    .get(self.state.active_profile)
                    .map(|p| p.url.clone());
                self.state.active_profile = active_url
                    .and_then(|url| profiles.iter().position(|p| p.url == url))
                    .unwrap_or(0);
                self.state.profiles = profiles
                    .iter()
                    .map(|p| Profile {
                        name: p.name.clone(),
                        url: p.url.clone(),
                        interval: "—".into(),
                        used: 0,
                        total: 0,
                        updated: "已下载".into(),
                        content: String::new(),
                    })
                    .collect();
                live.profiles = profiles;
                self.selected = self
                    .selected
                    .min(self.state.profiles.len().saturating_sub(1));
                self.status =
                    "已重读本地订阅；Enter 应用。远端更新使用 --subscriptions-file".into();
            }
            Err(_) => self.status = "订阅索引读取失败；原列表保留".into(),
        }
    }

    pub fn confirm_live(&mut self, target: &Confirm) -> bool {
        match target {
            Confirm::CoreConnection(id) => self.queue_core(Command::Close(Some(id.clone()))),
            Confirm::AllConnections => self.queue_core(Command::Close(None)),
            _ => self.status = "当前确认操作不支持真实模式".into(),
        }
        true
    }
    pub fn save_live_form(&mut self, fields: &[Field], target: &SaveTarget) {
        if !matches!(target, SaveTarget::Settings(_)) {
            self.status = "当前表单不支持真实模式".into();
            self.modal = None;
            return;
        }
        let mut patch = serde_json::Map::new();
        for f in fields {
            match f.key.as_str() {
                "ipv6" | "unified_delay" => {
                    patch.insert(
                        if f.key == "ipv6" {
                            "ipv6"
                        } else {
                            "unified-delay"
                        }
                        .into(),
                        json!(f.value == "开启"),
                    );
                }
                "log_level" => {
                    patch.insert("log-level".into(), json!(f.value));
                }
                key if UI_KEYS.contains(&key) => {
                    self.state.settings.insert(f.key.clone(), f.value.clone());
                }
                _ => {}
            }
        }
        self.modal = None;
        if patch.is_empty() {
            self.note("界面偏好已保存");
        } else {
            self.queue_core(Command::Patch(Value::Object(patch)));
        }
    }
    fn runtime_live(&mut self) {
        self.detail(
            "内核运行参数",
            serde_json::to_string_pretty(&self.live.as_ref().unwrap().config).unwrap_or_default(),
        );
    }
}
