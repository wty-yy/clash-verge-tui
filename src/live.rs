use crate::{
    app::{App, Confirm, Field, SaveTarget},
    core::{Command, CoreEvent, LogEvent, Snapshot},
    model::*,
    settings::{self, Kind},
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
    pub binary: PathBuf,
}
pub struct LiveState {
    pub endpoint: String,
    pub connected: bool,
    pub pending: bool,
    pub version: String,
    pub error: String,
    pub log_status: String,
    pub system_proxy_status: String,
    pub service_status: String,
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
    pub workspace: crate::workspace::WorkspaceState,
    pub backups: Vec<crate::backup::BackupMeta>,
    pub backups_remote: bool,
    pub tun_capable: bool,
}
impl LiveState {
    pub fn new(
        endpoint: String,
        profiles: Vec<StoredProfile>,
        managed: Option<ManagedSettings>,
    ) -> Self {
        let tun_capable = managed
            .as_ref()
            .is_some_and(|settings| crate::service::tun_capable(&settings.binary));
        Self {
            endpoint,
            connected: false,
            pending: false,
            version: String::new(),
            error: String::new(),
            log_status: "日志流连接中".into(),
            system_proxy_status: "检测中".into(),
            service_status: "检测中".into(),
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
            workspace: crate::workspace::WorkspaceState::default(),
            backups: Vec::new(),
            backups_remote: false,
            tun_capable,
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
        state.unlocks = crate::extras::initial();
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
        state.settings.insert("system_proxy".into(), "关闭".into());
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
        if app.live.as_ref().unwrap().managed.is_some() {
            let snapshot = crate::workspace::load(&app.data_dir)?;
            app.apply_workspace(snapshot);
            app.state.active_profile = active;
        }
        if let Some(managed) = app.live.as_ref().and_then(|l| l.managed.as_ref()) {
            app.state.settings.insert(
                "controller".into(),
                if managed.controller.is_empty() {
                    "关闭"
                } else {
                    "开启"
                }
                .into(),
            );
            app.state
                .settings
                .insert("controller_addr".into(), managed.controller.clone());
            app.state
                .settings
                .insert("secret".into(), managed.secret.clone());
            if !app
                .live
                .as_ref()
                .unwrap()
                .workspace
                .preferences
                .contains_key("webui_url")
            {
                if let Ok(address) = managed.controller.parse::<std::net::SocketAddr>() {
                    let host = if address.ip().is_unspecified() {
                        std::net::Ipv4Addr::LOCALHOST.into()
                    } else {
                        address.ip()
                    };
                    app.state.settings.insert(
                        "webui_url".into(),
                        format!(
                            "http://{}/ui/",
                            std::net::SocketAddr::new(host, address.port())
                        ),
                    );
                }
            }
        }
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
            CoreEvent::Extra(result) => match result {
                crate::extras::ExtraResult::Detected(items) => {
                    for (i, item) in items {
                        if let Some(row) = self.state.unlocks.get_mut(i) {
                            *row = item;
                        }
                    }
                    self.status = "检测完成；结果表示网页可达性，地区为出口参考".into();
                }
                crate::extras::ExtraResult::Update(message) => self.detail("应用更新", message),
                crate::extras::ExtraResult::Opened => self.status = "已请求打开".into(),
            },
            CoreEvent::Backups(result) => {
                if let Some(snapshot) = result.restored {
                    self.apply_workspace(snapshot);
                }
                let live = self.live.as_mut().unwrap();
                live.backups = result.items;
                live.backups_remote = result.remote;
                self.status = result.message;
            }
            CoreEvent::ServiceStatus(status) => self.live.as_mut().unwrap().service_status = status,
            CoreEvent::SystemProxyStatus(status) => {
                self.live.as_mut().unwrap().system_proxy_status = status
            }
            CoreEvent::BackgroundNotice(status) => {
                if !self.live.as_ref().unwrap().pending {
                    self.status = status;
                }
            }
            CoreEvent::ProfileImported { request, result } => {
                self.live.as_mut().unwrap().pending = false;
                if self.profile_import_pending.take() != Some(request) {
                    return;
                }
                let requested_url = self.profile_import_url.take();
                match result {
                    Ok(imported) => {
                        let crate::subscriptions::FetchedProfile {
                            content,
                            proxies,
                            groups,
                            ..
                        } = imported;
                        if let Some(crate::app::Modal::Form {
                            fields,
                            error,
                            target: SaveTarget::Profile(_),
                            ..
                        }) = &mut self.modal
                        {
                            let unchanged =
                                fields.iter().find(|field| field.key == "url").is_some_and(
                                    |field| Some(field.value.trim()) == requested_url.as_deref(),
                                );
                            if unchanged {
                                if let Some(field) =
                                    fields.iter_mut().find(|field| field.key == "content")
                                {
                                    field.value = content;
                                    field.cursor = field.value.len();
                                }
                                error.clear();
                                self.status = format!(
                                    "已导入 {proxies} 个节点、{groups} 个策略组；按 Ctrl+S 保存"
                                );
                            } else {
                                self.status = "订阅链接已更改，本次导入结果未写入表单".into();
                            }
                        } else {
                            self.status = "订阅已下载，但表单已关闭，未保存".into();
                        }
                    }
                    Err(message) => {
                        if let Some(crate::app::Modal::Form { error, .. }) = &mut self.modal {
                            *error = format!("导入失败：{message}");
                        }
                        self.status = "订阅配置导入失败".into();
                    }
                }
            }
            CoreEvent::TunServiceInstalled(result) => {
                self.live.as_mut().unwrap().pending = false;
                match result {
                    Ok(()) => {
                        self.restart_core = true;
                        self.status = "TUN 权限服务已安装，正在重启自管内核…".into();
                    }
                    Err(error) => {
                        self.tun_after_restart = None;
                        self.status = format!("TUN 权限服务安装失败：{error}");
                    }
                }
            }
            CoreEvent::Workspace(snapshot) => self.apply_workspace(*snapshot),
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
                self.profile_import_pending = None;
                self.profile_import_url = None;
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
    pub fn apply_workspace(&mut self, snapshot: crate::workspace::WorkspaceSnapshot) {
        self.state.active_profile = snapshot.active;
        self.state.enhancements = snapshot.state.enhancements.clone();
        self.state
            .settings
            .extend(snapshot.state.preferences.clone());
        self.state.profiles = snapshot
            .profiles
            .iter()
            .map(|p| {
                let schedule = snapshot
                    .state
                    .schedules
                    .get(&p.file.to_string_lossy().into_owned());
                Profile {
                    name: p.name.clone(),
                    url: p.url.clone(),
                    interval: schedule
                        .map(|s| s.interval_minutes.to_string())
                        .unwrap_or("0".into()),
                    used: 0,
                    total: 0,
                    updated: schedule
                        .map(|s| format!("{}", s.updated_at))
                        .unwrap_or("已下载".into()),
                    content: String::new(),
                }
            })
            .collect();
        let live = self.live.as_mut().unwrap();
        live.profiles = snapshot.profiles;
        live.workspace = snapshot.state;
        self.selected = self.selected.min(self.rows().len().saturating_sub(1));
    }
    fn workspace_command(&mut self, command: crate::workspace::WorkspaceCommand) {
        if self.live.as_ref().unwrap().managed.is_none() {
            self.status = "此操作需要自管内核工作区".into();
            return;
        }
        let needs_tun_service = matches!(
            &command,
            crate::workspace::WorkspaceCommand::Settings(values)
                if values.get("tun").is_some_and(|value| value == "开启")
                    && self.live.as_ref().is_some_and(|live| !live.tun_capable)
        );
        if needs_tun_service {
            self.tun_after_restart = Some(command);
            self.confirm(
                "安装 TUN 权限服务",
                "TUN 需要 CAP_NET_ADMIN。确认后系统会提示输入管理员密码，安装仅维护当前工作区内核能力的 systemd 服务；完成后自动重启内核并继续开启 TUN。",
                Confirm::TunService,
            );
            return;
        }
        self.queue_core(Command::Workspace(command));
    }
    pub fn resume_tun_after_restart(&mut self) {
        self.restart_core = false;
        if let Some(command) = self.tun_after_restart.take() {
            self.queue_core(Command::Workspace(command));
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
        self.try_queue_core(command);
    }

    fn try_queue_core(&mut self, command: Command) -> bool {
        let live = self.live.as_mut().unwrap();
        let local_settings = matches!(&command,Command::Workspace(crate::workspace::WorkspaceCommand::Settings(values)) if values.keys().all(|k|["service","auto_launch","start_script","silent"].contains(&k.as_str())));
        if !live.connected
            && !local_settings
            && !matches!(
                &command,
                Command::Backup(_)
                    | Command::Extra(_)
                    | Command::ImportProfile { .. }
                    | Command::InstallTunService { .. }
                    | Command::Workspace(crate::workspace::WorkspaceCommand::Read)
            )
        {
            self.status = "内核尚未连接，等待重连后再操作".into();
            return false;
        }
        if live.pending {
            self.status = "上一个内核操作尚未完成".into();
            return false;
        }
        let importing = matches!(&command, Command::ImportProfile { .. });
        live.pending = true;
        live.outbox.push(command);
        self.status = if importing {
            "正在下载并校验订阅配置…"
        } else {
            "正在执行 mihomo 操作…"
        }
        .into();
        true
    }

    pub fn import_profile_url(&mut self) {
        let Some(crate::app::Modal::Form { fields, target, .. }) = &self.modal else {
            return;
        };
        if !matches!(target, SaveTarget::Profile(_)) {
            return;
        }
        let get = |key: &str| {
            fields
                .iter()
                .find(|field| field.key == key)
                .map(|field| field.value.trim().to_owned())
                .unwrap_or_default()
        };
        let url = get("url");
        if !url::Url::parse(&url).is_ok_and(|parsed| {
            matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some()
        }) {
            if let Some(crate::app::Modal::Form { error, .. }) = &mut self.modal {
                *error = "请先填写完整的 HTTP(S) 订阅文件链接".into();
            }
            return;
        }
        let proxy = get("proxy");
        if let Some(crate::app::Modal::Form { fields, .. }) = &mut self.modal {
            for field in fields {
                if field.key == "url" {
                    field.value = url.clone();
                    field.cursor = field.value.len();
                } else if field.key == "proxy" {
                    field.value = proxy.clone();
                    field.cursor = field.value.len();
                }
            }
        }
        self.profile_import_nonce = self.profile_import_nonce.wrapping_add(1);
        let request = self.profile_import_nonce;
        if self.try_queue_core(Command::ImportProfile {
            request,
            url: url.clone(),
            proxy: (!proxy.is_empty()).then_some(proxy),
        }) {
            self.profile_import_pending = Some(request);
            self.profile_import_url = Some(url);
            if let Some(crate::app::Modal::Form { error, .. }) = &mut self.modal {
                *error = "正在下载并校验订阅配置…".into();
            }
        }
    }
    pub fn activate_live(&mut self) {
        let Some(id) = self.selected_id() else { return };
        match self.page {
            Page::Home => match id {
                0 => self.workspace_command(crate::workspace::WorkspaceCommand::Settings(
                    BTreeMap::from([(
                        "system_proxy".into(),
                        if self.live.as_ref().unwrap().system_proxy_status == "开启" {
                            "关闭"
                        } else {
                            "开启"
                        }
                        .into(),
                    )]),
                )),
                1 => self.workspace_command(crate::workspace::WorkspaceCommand::Settings(
                    BTreeMap::from([
                        (
                            "tun".into(),
                            if self.state.value("tun") == "开启" {
                                "关闭"
                            } else {
                                "开启"
                            }
                            .into(),
                        ),
                        ("tun_device".into(), self.state.value("tun_device").into()),
                    ]),
                )),
                2 => {
                    self.command_live('m');
                }
                3 => self.mixed_port_form(),
                4 => self.navigate(Page::Profiles),
                5 => self.runtime_live(),
                _ => {
                    if let Some(proxy) = self.proxy_url() {
                        let command = match self.state.value("env_type") {
                            "fish" => format!(
                                "set -gx http_proxy '{proxy}'\nset -gx https_proxy '{proxy}'"
                            ),
                            "powershell" => {
                                format!("$env:http_proxy = '{proxy}'\n$env:https_proxy = '{proxy}'")
                            }
                            _ => {
                                format!("export http_proxy='{proxy}'\nexport https_proxy='{proxy}'")
                            }
                        };
                        self.detail("环境变量", command);
                    } else {
                        self.status = "没有可用代理端口".into();
                    }
                }
            },
            Page::Proxies => {
                let Some(group) = self.state.groups.get(self.sub) else {
                    return;
                };
                if matches!(group.kind.as_str(), "LoadBalance" | "Relay") {
                    self.status = "此策略组不支持手动固定选择".into();
                    return;
                }
                self.queue_core(Command::Select {
                    group: group.name.clone(),
                    node: self.state.nodes[id].name.clone(),
                });
            }
            Page::Profiles => {
                if self.sub == 0 {
                    self.workspace_command(crate::workspace::WorkspaceCommand::Select(id));
                } else {
                    self.workspace_command(crate::workspace::WorkspaceCommand::ToggleEnhancement(
                        id,
                    ));
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
            Page::Unlock => self.detect_live(vec![id]),
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
                    "启动与服务" | "系统代理" | "DNS 覆写" | "端口设置" | "虚拟网卡 TUN"
                    | "流量隧道"
                        if self.live.as_ref().unwrap().managed.is_some() =>
                    {
                        let fields = section
                            .fields
                            .iter()
                            .filter(|f|f.key!="silent")
                            .map(|f| Field::from_spec(f, self.state.value(f.key)))
                            .collect();
                        self.form(section.name, fields, SaveTarget::Settings(id));
                    }
                    "基础网络" if self.live.as_ref().unwrap().managed.is_some() => {
                        let fields = section
                            .fields
                            .iter()
                            .map(|f| Field::from_spec(f, self.state.value(f.key)))
                            .collect();
                        self.form(section.name, fields, SaveTarget::Settings(id));
                    }
                    "备份与恢复" if self.live.as_ref().unwrap().managed.is_some() => {
                        self.modal = Some(crate::app::Modal::Backups { selected: 0 });
                        self.queue_core(Command::Backup(crate::backup::BackupCommand::List {
                            remote: false,
                        }));
                    }
                    "外部控制器" if self.live.as_ref().unwrap().managed.is_some() => {
                        let fields = section
                            .fields
                            .iter()
                            .map(|f| Field::from_spec(f, self.state.value(f.key)))
                            .collect();
                        self.form("外部控制器 · 重启后生效", fields, SaveTarget::Settings(id));
                    }
                    "轻量模式" | "杂项设置" | "内核与 GeoData" | "网页界面"
                        if self.live.as_ref().unwrap().managed.is_some() =>
                    {
                        let fields = section
                            .fields
                            .iter()
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
                    "桌面功能映射"=>self.detail("终端适配","托盘快捷操作对应首页控制；后台自启不打开窗口。\n字体、窗口装饰、桌面热键由终端和桌面环境管理。\n本客户端面向 Linux，界面为简体中文。"),
                    _=>self.detail(section.name,"此设置需要应用自管工作区，重启后再试。"),
                }
            }
        }
    }
    pub fn command_live(&mut self, c: char) -> bool {
        match c {
            't' | 's' | 'p' => return false,
            'm' if matches!(self.page, Page::Home | Page::Proxies) => {
                let mode = ["rule", "global", "direct"][(self.state.mode + 1) % 3].to_string();
                if self.live.as_ref().unwrap().managed.is_some() {
                    self.workspace_command(crate::workspace::WorkspaceCommand::Settings(
                        BTreeMap::from([("mode".into(), mode)]),
                    ));
                } else {
                    self.queue_core(Command::Mode(mode));
                }
            }
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
                Page::Profiles => {
                    if self.sub == 0 {
                        if let Some(i) = self.selected_id() {
                            self.workspace_command(crate::workspace::WorkspaceCommand::Refresh(i));
                        }
                    } else {
                        self.workspace_command(crate::workspace::WorkspaceCommand::Read);
                    }
                }
                Page::Unlock => self.detect_live(self.rows().iter().map(|r| r.id).collect()),
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
            'c' if self.page == Page::Proxies => {
                if let Some(g) = self.state.groups.get(self.sub) {
                    if matches!(g.kind.as_str(), "URLTest" | "Fallback") {
                        self.queue_core(Command::Unfix(g.name.clone()));
                    } else {
                        self.status = "此组没有自动选择固定状态".into();
                    }
                }
            }
            'u' if self.page == Page::Settings => {
                let name = self
                    .selected_id()
                    .map(|i| settings::sections()[i].name)
                    .unwrap_or("");
                match name {
                    "内核与 GeoData" => {
                        self.status = format!(
                            "mihomo v{} 随 Clash Verge TUI 一起更新",
                            crate::core_manager::MIHOMO_VERSION
                        )
                    }
                    "网页界面" => self.queue_core(Command::Upgrade {
                        kind: "ui".into(),
                        channel: None,
                    }),
                    _ => self.queue_core(Command::Extra(crate::extras::ExtraCommand::CheckUpdates)),
                }
            }
            'g' if self.page == Page::Settings => self.queue_core(Command::Upgrade {
                kind: "geo".into(),
                channel: None,
            }),
            'o' if self.page == Page::Settings => {
                let name = self
                    .selected_id()
                    .map(|i| settings::sections()[i].name)
                    .unwrap_or("");
                let target = if name == "网页界面" {
                    self.state.value("webui_url").into()
                } else {
                    self.data_dir.to_string_lossy().into_owned()
                };
                self.queue_core(Command::Extra(crate::extras::ExtraCommand::Open(target)));
            }
            'x' if matches!(self.page, Page::Settings | Page::Logs) => self.export_live(),
            'e' if self.page == Page::Settings => self.activate_live(),
            'b' if self.page == Page::Settings => {
                self.queue_core(Command::Backup(crate::backup::BackupCommand::Create))
            }
            'R' if self.page == Page::Settings => {
                self.modal = Some(crate::app::Modal::Backups { selected: 0 });
                self.queue_core(Command::Backup(crate::backup::BackupCommand::List {
                    remote: false,
                }));
            }
            'a' | 'e' if self.page == Page::Rules && self.sub == 0 => {
                let index = if c == 'e' { self.selected_id() } else { None };
                let r = index.and_then(|i| self.state.rules.get(i));
                self.form(
                    "覆写当前规则列表",
                    vec![
                        Field::new(
                            "kind",
                            "规则类型",
                            r.map(|r| r.kind.as_str()).unwrap_or("DOMAIN-SUFFIX"),
                            Kind::Text,
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
                            r.map(|r| r.target.as_str()).unwrap_or("DIRECT"),
                            Kind::Text,
                        ),
                    ],
                    SaveTarget::Rule(index),
                );
            }
            'd' if self.page == Page::Rules && self.sub == 0 => {
                if let Some(i) = self.selected_id() {
                    self.confirm(
                        "删除规则",
                        "删除后持久化覆写当前规则列表？",
                        Confirm::Rule(i),
                    );
                }
            }
            'R' if self.page == Page::Rules && self.sub == 0 => {
                self.workspace_command(crate::workspace::WorkspaceCommand::ResetRules)
            }
            'a' if self.page == Page::Profiles => self.edit_workspace_profile(None),
            'e' if self.page == Page::Profiles => {
                if let Some(i) = self.selected_id() {
                    self.edit_workspace_profile(Some(i));
                }
            }
            'i' if self.page == Page::Profiles && self.sub == 0 => {
                if let Some(i) = self.selected_id() {
                    let p = &self.live.as_ref().unwrap().profiles[i];
                    let origin = url::Url::parse(&p.url)
                        .ok()
                        .map(|u| u.origin().ascii_serialization())
                        .unwrap_or("本地文件".into());
                    let expiry = p
                        .usage()
                        .and_then(|(_, _, e)| e)
                        .map(|e| {
                            format!(
                                "{} 天",
                                e.saturating_sub(crate::workspace::timestamp())
                                    .div_ceil(86400)
                            )
                        })
                        .unwrap_or("未提供".into());
                    self.detail(
                        "订阅详情",
                        format!(
                            "名称：{}\n来源：{}\n节点：{}\n策略组：{}\n用量：{}\n到期剩余：{}",
                            p.name,
                            origin,
                            p.proxies,
                            p.groups,
                            p.usage_label(),
                            expiry
                        ),
                    );
                }
            }
            'v' if self.page == Page::Profiles && self.sub == 0 => {
                if let Some(i) = self.selected_id() {
                    match fs::read_to_string(&self.live.as_ref().unwrap().profiles[i].file) {
                        Ok(text) => self.form(
                            "编辑订阅 YAML",
                            vec![Field::new("content", "配置内容", &text, Kind::Multiline)],
                            SaveTarget::ProfileContent(i),
                        ),
                        Err(_) => self.status = "配置文件读取失败".into(),
                    }
                }
            }
            'd' if self.page == Page::Profiles => {
                if let Some(i) = self.selected_id() {
                    self.confirm(
                        "删除条目",
                        "删除该条目并重新校验当前配置？",
                        if self.sub == 0 {
                            Confirm::Profile(i)
                        } else {
                            Confirm::Enhancement(i)
                        },
                    );
                }
            }
            '[' | ']' if self.page == Page::Profiles => {
                if let Some(i) = self.selected_id() {
                    let len = if self.sub == 0 {
                        self.state.profiles.len()
                    } else {
                        self.state.enhancements.len()
                    };
                    let destination = if c == '[' {
                        i.saturating_sub(1)
                    } else {
                        (i + 1).min(len - 1)
                    };
                    self.workspace_command(if self.sub == 0 {
                        crate::workspace::WorkspaceCommand::Move {
                            index: i,
                            destination,
                        }
                    } else {
                        crate::workspace::WorkspaceCommand::MoveEnhancement {
                            index: i,
                            destination,
                        }
                    });
                }
            }
            'R' if self.page == Page::Profiles => {
                self.workspace_command(crate::workspace::WorkspaceCommand::Read)
            }
            'a' | 'e' | 'd' | 'v' | 'b' | 'R' | '[' | ']' => {
                self.detail("当前页面没有此操作", "可用操作见页面工具栏和帮助。")
            }
            _ => return false,
        }
        true
    }
    fn edit_workspace_profile(&mut self, index: Option<usize>) {
        let live = self.live.as_ref().unwrap();
        if live.managed.is_none() {
            self.status = "此操作需要自管内核工作区".into();
            return;
        }
        if self.sub == 0 {
            let p = index.and_then(|i| live.profiles.get(i));
            let interval = p
                .and_then(|p| {
                    live.workspace
                        .schedules
                        .get(&p.file.to_string_lossy().into_owned())
                })
                .map(|s| s.interval_minutes)
                .unwrap_or(720)
                .to_string();
            self.form(
                if index.is_some() {
                    "编辑订阅 / 链接导入"
                } else {
                    "添加订阅 / 链接导入"
                },
                vec![
                    Field::new(
                        "url",
                        "订阅文件链接",
                        p.map(|p| p.url.as_str()).unwrap_or(""),
                        Kind::Text,
                    ),
                    Field::new(
                        "content",
                        "订阅配置 YAML",
                        &p.and_then(|profile| fs::read_to_string(&profile.file).ok())
                            .unwrap_or_default(),
                        Kind::Multiline,
                    ),
                    Field::new(
                        "name",
                        "名称",
                        p.map(|p| p.name.as_str()).unwrap_or(""),
                        Kind::Text,
                    ),
                    Field::new(
                        "proxy",
                        "下载代理（可留空）",
                        p.and_then(|p| p.proxy.as_deref()).unwrap_or(""),
                        Kind::Text,
                    ),
                    Field::new(
                        "interval",
                        "更新间隔 / 分钟（0 关闭）",
                        &interval,
                        Kind::Number,
                    ),
                    Field::new("local_file", "导入本地文件（可留空）", "", Kind::Text),
                ],
                SaveTarget::Profile(index),
            );
        } else {
            let item = index.and_then(|i| self.state.enhancements.get(i));
            self.form(
                "配置增强 · 执行用户编写的脚本",
                vec![
                    Field::new(
                        "name",
                        "名称",
                        item.map(|e| e.name.as_str()).unwrap_or(""),
                        Kind::Text,
                    ),
                    Field::new(
                        "kind",
                        "类型",
                        item.map(|e| e.kind.as_str()).unwrap_or("YAML"),
                        Kind::Choice(&["YAML", "JavaScript"]),
                    ),
                    Field::new(
                        "content",
                        "内容",
                        item.map(|e| e.content.as_str()).unwrap_or(""),
                        Kind::Multiline,
                    ),
                ],
                SaveTarget::Enhancement(index),
            );
        }
    }
    pub fn validate_live_form(&self, fields: &[Field], target: &SaveTarget) -> Result<(), String> {
        if matches!(target, SaveTarget::TunServicePassword)
            && fields
                .iter()
                .find(|field| field.key == "system_password")
                .is_none_or(|field| field.value.is_empty())
        {
            return Err("请输入系统密码".into());
        }
        for f in fields {
            if f.key == "name" && f.value.trim().is_empty() {
                return Err("名称不能为空".into());
            }
            if matches!(f.kind, Kind::Number) {
                let n = f
                    .value
                    .parse::<u64>()
                    .map_err(|_| format!("{}需要非负整数", f.label))?;
                if f.key.ends_with("_port") && n > u64::from(u16::MAX) {
                    return Err(format!("{}不能大于 65535", f.label));
                }
                if f.key == "mixed_port" && n == 0 {
                    return Err("混合代理端口应在 1–65535".into());
                }
                if f.key == "refresh" && n < 100 {
                    return Err("刷新间隔不能小于 100 毫秒".into());
                }
            }
            if f.key == "url"
                && !f.value.is_empty()
                && !url::Url::parse(&f.value).is_ok_and(|u| matches!(u.scheme(), "http" | "https"))
            {
                return Err("订阅 URL 无效".into());
            }
        }
        Ok(())
    }
    pub fn backup_key(&mut self, key: crossterm::event::KeyCode, selected: usize) {
        use crate::backup::BackupCommand as B;
        let live = self.live.as_ref().unwrap();
        let remote = live.backups_remote;
        let length = live.backups.len();
        match key {
            crossterm::event::KeyCode::Up => {
                self.modal = Some(crate::app::Modal::Backups {
                    selected: selected.saturating_sub(1),
                })
            }
            crossterm::event::KeyCode::Down => {
                self.modal = Some(crate::app::Modal::Backups {
                    selected: (selected + 1).min(length.saturating_sub(1)),
                })
            }
            crossterm::event::KeyCode::Left
            | crossterm::event::KeyCode::Right
            | crossterm::event::KeyCode::Tab => {
                self.modal = Some(crate::app::Modal::Backups { selected: 0 });
                self.queue_core(Command::Backup(B::List { remote: !remote }));
            }
            crossterm::event::KeyCode::Char('b') => self.queue_core(Command::Backup(B::Create)),
            crossterm::event::KeyCode::Char('e') => {
                let sections = settings::sections();
                let id = sections
                    .iter()
                    .position(|s| s.name == "备份与恢复")
                    .unwrap();
                let fields = sections[id]
                    .fields
                    .iter()
                    .map(|f| Field::from_spec(f, self.state.value(f.key)))
                    .collect();
                self.form("备份与 WebDAV", fields, SaveTarget::Settings(id));
            }
            crossterm::event::KeyCode::Enter
            | crossterm::event::KeyCode::Char('d')
            | crossterm::event::KeyCode::Char('u')
                if selected < length =>
            {
                let file = live.backups[selected].file.clone();
                let command = match key {
                    crossterm::event::KeyCode::Enter => B::Restore { file, remote },
                    crossterm::event::KeyCode::Char('d') => B::Delete { file, remote },
                    _ => B::Upload(file),
                };
                self.confirm(
                    "确认备份操作",
                    "执行所选操作？恢复会替换订阅、增强脚本和设置。",
                    Confirm::LiveBackup(command),
                );
            }
            _ => {}
        }
    }
    fn proxy_url(&self) -> Option<String> {
        let host = url::Url::parse(&self.live.as_ref()?.endpoint)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or("127.0.0.1".into());
        let host = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]")
        } else {
            host
        };
        for (key, scheme) in [
            ("mixed_port", "http"),
            ("http_port", "http"),
            ("socks_port", "socks5h"),
        ] {
            if let Ok(port) = self.state.value(key).parse::<u16>() {
                if port > 0 {
                    return Some(format!("{scheme}://{host}:{port}"));
                }
            }
        }
        None
    }
    fn detect_live(&mut self, indices: Vec<usize>) {
        if let Some(proxy) = self.proxy_url() {
            self.queue_core(Command::Extra(crate::extras::ExtraCommand::Detect {
                proxy,
                indices,
            }));
        } else {
            self.status = "没有可用的代理监听端口".into();
        }
    }
    fn export_live(&mut self) {
        let path = self
            .data_dir
            .join("reports")
            .join(if self.page == Page::Logs {
                "logs.txt"
            } else {
                "diagnostics.json"
            });
        let text = if self.page == Page::Logs {
            self.rows()
                .iter()
                .map(|r| r.cells.join(" "))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            let live = self.live.as_ref().unwrap();
            serde_json::to_string_pretty(&json!({"app":env!("CARGO_PKG_VERSION"),"core":live.version,"connected":live.connected,"nodes":self.state.nodes.len(),"groups":self.state.groups.len(),"rules":self.state.rules.len(),"connections":self.state.connections.len(),"mode":self.state.mode,"managed":live.managed.is_some()})).unwrap()
        };
        self.status = match subscriptions::private_write(&path, text.as_bytes()) {
            Ok(()) => format!("已导出到 {}", path.display()),
            Err(_) => "导出失败".into(),
        };
    }
    pub fn confirm_live(&mut self, target: &Confirm) -> bool {
        match target {
            Confirm::CoreUpgrade => {
                self.status = format!(
                    "mihomo v{} 由应用发行包管理",
                    crate::core_manager::MIHOMO_VERSION
                )
            }
            Confirm::TunService => self.tun_password_form(),
            Confirm::LiveBackup(command) => self.queue_core(Command::Backup(command.clone())),
            Confirm::Profile(i) => {
                self.workspace_command(crate::workspace::WorkspaceCommand::Delete(*i))
            }
            Confirm::Enhancement(i) => {
                self.workspace_command(crate::workspace::WorkspaceCommand::DeleteEnhancement(*i))
            }
            Confirm::CoreConnection(id) => self.queue_core(Command::Close(Some(id.clone()))),
            Confirm::Rule(i) => {
                self.workspace_command(crate::workspace::WorkspaceCommand::EditRule {
                    index: Some(self.live.as_ref().unwrap().rule_indices[*i]),
                    rule: None,
                })
            }
            Confirm::AllConnections => self.queue_core(Command::Close(None)),
            _ => self.status = "当前确认操作不支持真实模式".into(),
        }
        true
    }
    pub fn save_live_form(&mut self, fields: &[Field], target: &SaveTarget) {
        let live = self.live.as_ref().unwrap();
        let offline_safe = fields.iter().all(|f| {
            UI_KEYS.contains(&f.key.as_str())
                || ["service", "auto_launch", "start_script", "silent"].contains(&f.key.as_str())
        });
        if live.pending || !live.connected && !offline_safe {
            if let Some(crate::app::Modal::Form { error, .. }) = &mut self.modal {
                *error = if live.pending {
                    "上一项操作尚未完成，请稍后按 Ctrl+S 重试"
                } else {
                    "内核未连接，输入已保留"
                }
                .into();
            }
            return;
        }

        let get = |key: &str| {
            fields
                .iter()
                .find(|f| f.key == key)
                .map(|f| f.value.clone())
                .unwrap_or_default()
        };
        use crate::workspace::WorkspaceCommand as W;
        let command = match target {
            SaveTarget::Profile(index) => {
                let text = if !get("local_file").is_empty() {
                    match fs::read_to_string(get("local_file")) {
                        Ok(s) if s.len() <= 8 * 1024 * 1024 => s,
                        _ => {
                            self.status = "本地文件读取失败或超过 8 MiB".into();
                            return;
                        }
                    }
                } else {
                    get("content")
                };
                let content = if !text.trim().is_empty() {
                    Some(text)
                } else {
                    None
                };
                if get("url").is_empty() && content.is_none() {
                    self.status = "本地配置需要 YAML 内容或文件路径".into();
                    return;
                }
                Some(W::PutProfile {
                    index: *index,
                    name: get("name"),
                    url: get("url"),
                    proxy: if get("proxy").is_empty() {
                        None
                    } else {
                        Some(get("proxy"))
                    },
                    content,
                    interval: get("interval").parse().unwrap_or(0),
                })
            }
            SaveTarget::ProfileContent(index) => {
                let p = &self.live.as_ref().unwrap().profiles[*index];
                let interval = self
                    .live
                    .as_ref()
                    .unwrap()
                    .workspace
                    .schedules
                    .get(&p.file.to_string_lossy().into_owned())
                    .map(|s| s.interval_minutes)
                    .unwrap_or(0);
                Some(W::PutProfile {
                    index: Some(*index),
                    name: p.name.clone(),
                    url: p.url.clone(),
                    proxy: p.proxy.clone(),
                    content: Some(get("content")),
                    interval,
                })
            }
            SaveTarget::Rule(index) => {
                let kind = match get("kind").as_str() {
                    "Domain" => "DOMAIN".into(),
                    "DomainSuffix" => "DOMAIN-SUFFIX".into(),
                    "DomainKeyword" => "DOMAIN-KEYWORD".into(),
                    "GeoIP" => "GEOIP".into(),
                    "IPCIDR" => "IP-CIDR".into(),
                    "RuleSet" => "RULE-SET".into(),
                    "Match" => "MATCH".into(),
                    value => value.to_ascii_uppercase(),
                };
                let rule = if kind == "MATCH" {
                    format!("MATCH,{}", get("target"))
                } else {
                    format!("{},{},{}", kind, get("payload"), get("target"))
                };
                Some(W::EditRule {
                    index: index.map(|i| self.live.as_ref().unwrap().rule_indices[i]),
                    rule: Some(rule),
                })
            }
            SaveTarget::Enhancement(index) => Some(W::PutEnhancement {
                index: *index,
                item: Enhancement {
                    name: get("name"),
                    kind: get("kind"),
                    content: get("content"),
                    enabled: index
                        .and_then(|i| self.state.enhancements.get(i))
                        .map(|e| e.enabled)
                        .unwrap_or(true),
                },
            }),
            _ => None,
        };
        if let Some(command) = command {
            self.modal = None;
            self.workspace_command(command);
            return;
        }
        if !matches!(target, SaveTarget::Settings(_)) {
            self.status = "此表单尚未接入".into();
            return;
        }
        let network = fields.iter().any(|f| !UI_KEYS.contains(&f.key.as_str()));
        if network && self.live.as_ref().unwrap().managed.is_some() {
            self.modal = None;
            self.workspace_command(W::Settings(
                fields
                    .iter()
                    .map(|f| (f.key.clone(), f.value.clone()))
                    .collect(),
            ));
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
        fn redact(value: &mut serde_yaml_ng::Value) {
            match value {
                serde_yaml_ng::Value::Mapping(map) => {
                    for (key, value) in map.iter_mut() {
                        let key = key.as_str().unwrap_or("").to_ascii_lowercase();
                        if [
                            "secret",
                            "password",
                            "uuid",
                            "private-key",
                            "authentication",
                            "token",
                            "authorization",
                            "cookie",
                        ]
                        .contains(&key.as_str())
                        {
                            *value = "[hidden]".into();
                        } else if key == "url" {
                            if let Some(text) = value.as_str() {
                                if let Ok(url) = url::Url::parse(text) {
                                    *value =
                                        format!("{}/…", url.origin().ascii_serialization()).into();
                                }
                            }
                        } else {
                            redact(value);
                        }
                    }
                }
                serde_yaml_ng::Value::Sequence(values) => {
                    for value in values {
                        redact(value);
                    }
                }
                _ => {}
            }
        }
        let live = self.live.as_ref().unwrap();
        let mut value = if live.managed.is_some() {
            fs::read_to_string(self.data_dir.join("core/config.yaml"))
                .ok()
                .and_then(|s| serde_yaml_ng::from_str(&s).ok())
                .unwrap_or(serde_yaml_ng::Value::Null)
        } else {
            serde_yaml_ng::to_value(&live.config).unwrap_or_default()
        };
        redact(&mut value);
        self.detail(
            "当前配置 · 认证信息已隐藏",
            serde_yaml_ng::to_string(&value).unwrap_or_default(),
        );
    }
}
