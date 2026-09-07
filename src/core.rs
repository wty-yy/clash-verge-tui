use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use reqwest::{Client, Method, Response};
use serde_json::{json, Value};
use std::{sync::mpsc, thread, time::Duration};
use tokio::sync::{mpsc as async_mpsc, watch};
use url::Url;

#[derive(Clone)]
pub struct CoreClient {
    client: Client,
    endpoint: Url,
    secret: String,
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub version: String,
    pub config: Value,
    pub proxies: Value,
    pub rules: Value,
    pub providers: Value,
    pub connections: Value,
}
#[derive(Clone, Debug)]
pub enum Command {
    Refresh,
    Workspace(crate::workspace::WorkspaceCommand),
    Backup(crate::backup::BackupCommand),
    Extra(crate::extras::ExtraCommand),
    ImportProfile {
        request: u64,
        url: String,
        proxy: Option<String>,
    },
    Upgrade {
        kind: String,
        channel: Option<String>,
    },
    Unfix(String),
    PollEvery(u64),
    Select {
        group: String,
        node: String,
    },
    Mode(String),
    Close(Option<String>),
    Delay {
        group: String,
        url: String,
        timeout: u32,
    },
    DisableRule {
        index: usize,
        disabled: bool,
    },
    UpdateProvider(String),
    Patch(Value),
    Reload(String),
}
#[derive(Debug)]
pub enum CoreEvent {
    Snapshot(Box<Snapshot>),
    Workspace(Box<crate::workspace::WorkspaceSnapshot>),
    Offline(String),
    SystemProxyStatus(String),
    ServiceStatus(String),
    Backups(crate::backup::BackupResult),
    Extra(crate::extras::ExtraResult),
    ProfileImported {
        request: u64,
        result: Result<crate::subscriptions::FetchedProfile, String>,
    },
    BackgroundNotice(String),
    Completed(Result<(), String>),
}
#[derive(Debug)]
pub enum LogEvent {
    Entry { level: String, message: String },
    Status(String),
}

impl CoreClient {
    pub fn new(endpoint: &str, secret: String) -> Result<Self> {
        let mut endpoint = Url::parse(endpoint).map_err(|_| anyhow!("控制器地址无效"))?;
        let mut socket = None;
        if endpoint.scheme() == "unix" {
            endpoint = Url::parse(&endpoint.as_str().replacen("unix:", "file:", 1))
                .map_err(|_| anyhow!("Unix socket 地址无效"))?;
            socket = Some(
                endpoint
                    .to_file_path()
                    .map_err(|_| anyhow!("Unix socket 需要绝对路径"))?,
            );
            endpoint = Url::parse("http://localhost").unwrap();
        }
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            bail!("控制器地址需为 http(s) URL；密钥请使用文件或环境变量传入");
        }
        let mut builder = Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none());
        #[cfg(unix)]
        if let Some(socket) = socket {
            builder = builder.unix_socket(socket);
        }
        #[cfg(not(unix))]
        if socket.is_some() {
            bail!("当前系统不支持 Unix socket");
        }
        let client = builder.build()?;
        Ok(Self {
            client,
            endpoint,
            secret,
        })
    }
    pub fn endpoint(&self) -> String {
        self.endpoint.to_string()
    }
    fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.endpoint.clone();
        url.path_segments_mut()
            .unwrap()
            .pop_if_empty()
            .extend(segments);
        url
    }
    async fn request(
        &self,
        method: Method,
        segments: &[&str],
        body: Option<Value>,
        query: &[(&str, String)],
        timeout: Duration,
    ) -> Result<Response> {
        let mut req = self
            .client
            .request(method, self.url(segments))
            .query(query)
            .timeout(timeout);
        if !self.secret.is_empty() {
            req = req.bearer_auth(&self.secret);
        }
        if let Some(body) = body {
            req = req.json(&body);
        }
        let response = req
            .send()
            .await
            .map_err(|e| anyhow!("控制器请求失败：{}", e.without_url()))?;
        if !response.status().is_success() {
            bail!("控制器 HTTP {}", response.status().as_u16());
        }
        Ok(response)
    }
    pub async fn get(&self, path: &[&str]) -> Result<Value> {
        let response = self
            .request(Method::GET, path, None, &[], Duration::from_secs(5))
            .await?;
        let bytes = response
            .bytes()
            .await
            .map_err(|_| anyhow!("读取控制器响应失败"))?;
        serde_json::from_slice(&bytes).map_err(|_| anyhow!("控制器返回了无效 JSON"))
    }
    pub async fn snapshot(&self) -> Result<Snapshot> {
        let version = self.get(&["version"]).await?;
        let (mut config, proxies, connections, rules, providers) = tokio::try_join!(
            self.get(&["configs"]),
            self.get(&["proxies"]),
            self.get(&["connections"]),
            self.get(&["rules"]),
            self.get(&["providers", "rules"]),
        )?;
        if !config.is_object()
            || !proxies["proxies"].is_object()
            || !rules["rules"].is_array()
            || !providers["providers"].is_object()
            || !connections.is_object()
        {
            bail!("控制器响应结构不兼容");
        }
        if let Some(config) = config.as_object_mut() {
            config.remove("secret");
            config.remove("authentication");
        }
        Ok(Snapshot {
            version: version["version"].as_str().unwrap_or("unknown").into(),
            config,
            proxies,
            rules,
            providers,
            connections,
        })
    }
    pub async fn execute(&self, command: &Command) -> Result<()> {
        let timeout = Duration::from_secs(10);
        match command {
            Command::Refresh => {}
            Command::Workspace(_)
            | Command::Backup(_)
            | Command::Extra(_)
            | Command::ImportProfile { .. }
            | Command::PollEvery(_) => bail!("工作区命令必须由后台工作线程处理"),
            Command::Upgrade { kind, channel } => {
                let path = match kind.as_str() {
                    "core" => vec!["upgrade"],
                    "geo" => vec!["upgrade", "geo"],
                    "ui" => vec!["upgrade", "ui"],
                    _ => bail!("未知更新类型"),
                };
                let query = channel
                    .as_ref()
                    .map(|s| vec![("channel", s.clone())])
                    .unwrap_or_default();
                self.request(Method::POST, &path, None, &query, Duration::from_secs(300))
                    .await?;
            }
            Command::Unfix(group) => {
                self.request(
                    Method::DELETE,
                    &["proxies", group],
                    None,
                    &[],
                    Duration::from_secs(10),
                )
                .await?;
            }
            Command::Select { group, node } => {
                self.request(
                    Method::PUT,
                    &["proxies", group],
                    Some(json!({"name":node})),
                    &[],
                    timeout,
                )
                .await?;
            }
            Command::Mode(mode) => {
                self.request(
                    Method::PATCH,
                    &["configs"],
                    Some(json!({"mode":mode})),
                    &[],
                    timeout,
                )
                .await?;
            }
            Command::Close(id) => {
                let mut path = vec!["connections"];
                if let Some(id) = id {
                    path.push(id);
                }
                self.request(Method::DELETE, &path, None, &[], timeout)
                    .await?;
            }
            Command::Delay {
                group,
                url,
                timeout: ms,
            } => {
                self.request(
                    Method::GET,
                    &["group", group, "delay"],
                    None,
                    &[("url", url.clone()), ("timeout", ms.to_string())],
                    Duration::from_millis(u64::from(*ms) + 10000),
                )
                .await?;
            }
            Command::DisableRule { index, disabled } => {
                self.request(
                    Method::PATCH,
                    &["rules", "disable"],
                    Some(json!({index.to_string():disabled})),
                    &[],
                    timeout,
                )
                .await?;
            }
            Command::UpdateProvider(name) => {
                self.request(
                    Method::PUT,
                    &["providers", "rules", name],
                    None,
                    &[],
                    Duration::from_secs(30),
                )
                .await?;
            }
            Command::Patch(config) => {
                self.request(
                    Method::PATCH,
                    &["configs"],
                    Some(config.clone()),
                    &[],
                    timeout,
                )
                .await?;
            }
            Command::Reload(payload) => {
                self.request(
                    Method::PUT,
                    &["configs"],
                    Some(json!({"payload":payload})),
                    &[("force", "true".into())],
                    Duration::from_secs(45),
                )
                .await?;
            }
        }
        Ok(())
    }
    async fn logs(&self, tx: mpsc::SyncSender<LogEvent>, mut stop: watch::Receiver<bool>) {
        loop {
            let mut req = self
                .client
                .get(self.url(&["logs"]))
                .query(&[("level", "debug")]);
            if !self.secret.is_empty() {
                req = req.bearer_auth(&self.secret);
            }
            let response =
                tokio::select! { _=stop.changed()=>return, response=req.send()=>response };
            match response {
                Ok(response) if response.status().is_success() => {
                    let _ = tx.try_send(LogEvent::Status("日志流已连接".into()));
                    let mut stream = response.bytes_stream();
                    let mut buffer = Vec::new();
                    loop {
                        let chunk =
                            tokio::select! {_=stop.changed()=>return, chunk=stream.next()=>chunk};
                        let Some(Ok(chunk)) = chunk else { break };
                        buffer.extend_from_slice(&chunk);
                        if buffer.len() > 1024 * 1024 {
                            buffer.clear();
                            break;
                        }
                        while let Some(end) = buffer.iter().position(|b| *b == b'\n') {
                            let line: Vec<u8> = buffer.drain(..=end).collect();
                            if let Ok(value) = serde_json::from_slice::<Value>(&line) {
                                let level = value["type"]
                                    .as_str()
                                    .or(value["level"].as_str())
                                    .unwrap_or("info")
                                    .to_uppercase();
                                let message = value["payload"]
                                    .as_str()
                                    .or(value["message"].as_str())
                                    .unwrap_or("")
                                    .chars()
                                    .filter(|c| !c.is_control())
                                    .take(8192)
                                    .collect();
                                let _ = tx.try_send(LogEvent::Entry {
                                    level: if level == "WARNING" {
                                        "WARN".into()
                                    } else {
                                        level
                                    },
                                    message,
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
            let _ = tx.try_send(LogEvent::Status("日志流断开，正在重连".into()));
            tokio::select! {_=stop.changed()=>return,_=tokio::time::sleep(Duration::from_secs(2))=>{}}
        }
    }
}

pub struct Worker {
    commands: async_mpsc::Sender<Command>,
    pub events: mpsc::Receiver<CoreEvent>,
    pub logs: mpsc::Receiver<LogEvent>,
    stop: watch::Sender<bool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Worker {
    pub fn spawn(client: CoreClient) -> Result<Self> {
        Self::spawn_with_workspace(client, None)
    }
    pub fn spawn_with_workspace(
        client: CoreClient,
        workspace: Option<crate::workspace::WorkspaceContext>,
    ) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let (commands, mut receiver) = async_mpsc::channel(16);
        let (events_tx, events) = mpsc::channel();
        let (log_tx, logs) = mpsc::sync_channel(300);
        let (stop, mut stopped) = watch::channel(false);
        let log_stop = stopped.clone();
        let thread=thread::Builder::new().name("mihomo-controller".into()).spawn(move||runtime.block_on(async move {
            let log_client=client.clone();let log_task=tokio::spawn(async move {log_client.logs(log_tx,log_stop).await;});
            let mut interval=tokio::time::interval(Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut guard_at=std::time::Instant::now();let mut ticks=0u64;let mut poll_every=1u64;let mut delay_at=std::time::Instant::now();let mut auto_delay:Option<tokio::task::JoinHandle<()>>=None;
            let mut auto_update=None;
            if let Some(context)=&workspace{if crate::workspace::load(&context.dir).ok().is_some_and(|w|w.state.preferences.get("auto_check").is_none_or(|v|v=="开启")){let tx=events_tx.clone();auto_update=Some(tokio::spawn(async move{if let Ok(crate::extras::ExtraResult::Update(message))=crate::extras::execute(crate::extras::ExtraCommand::CheckUpdates).await{if message.starts_with("发现新版本"){let _=tx.send(CoreEvent::BackgroundNotice(message));}}}));}}

            loop {
                let mut command=tokio::select! {_=stopped.changed()=>break, command=receiver.recv()=>{if command.is_none(){break;}command},_=interval.tick()=>None};
                let automatic=command.is_none();
                if automatic {ticks+=1;if let Some(context)=&workspace{let proxy=crate::platform::SystemProxy::new(context.dir.clone());if let Ok(state)=crate::workspace::load(&context.dir){let prefs=&state.state.preferences;let delay=prefs.get("guard_interval").and_then(|s|s.parse::<u64>().ok()).unwrap_or(30).max(1);if prefs.get("system_proxy").is_some_and(|s|s=="开启") && prefs.get("guard").is_none_or(|s|s=="开启") && guard_at.elapsed()>=Duration::from_secs(delay){let _=proxy.guard().await;guard_at=std::time::Instant::now();}
if ticks.is_multiple_of(3){let host=prefs.get("proxy_host").map(String::as_str).unwrap_or("127.0.0.1");let port=state.state.overrides.get(serde_yaml_ng::Value::from("mixed-port")).and_then(serde_yaml_ng::Value::as_u64).unwrap_or(context.port as u64) as u16;let status=proxy.status(host,port).await.unwrap_or("不可用".into());let _=events_tx.send(CoreEvent::SystemProxyStatus(status));let service=crate::service::status(&context.dir).await;let _=events_tx.send(CoreEvent::ServiceStatus(service));}}}}
                if command.is_none(){if let Some(context)=&workspace{if let Ok(Some(index))=crate::workspace::due(&context.dir){command=Some(Command::Workspace(crate::workspace::WorkspaceCommand::Refresh(index)));}}}
                if automatic {if let Some(context)=&workspace{if let Ok(state)=crate::workspace::load(&context.dir){let prefs=state.state.preferences;let delay=prefs.get("delay_interval").and_then(|s|s.parse::<u64>().ok()).unwrap_or(300).max(30);if prefs.get("auto_delay").is_some_and(|v|v=="开启")&&delay_at.elapsed()>=Duration::from_secs(delay)&&auto_delay.as_ref().is_none_or(|t|t.is_finished()){delay_at=std::time::Instant::now();let api=client.clone();let tx=events_tx.clone();auto_delay=Some(tokio::spawn(async move{if let Ok(value)=api.get(&["proxies"]).await{if let Some(proxies)=value["proxies"].as_object(){for(name,group)in proxies{if group["type"].as_str()==Some("Selector"){let _=api.execute(&Command::Delay{group:name.clone(),url:prefs.get("test_url").cloned().unwrap_or("https://www.gstatic.com/generate_204".into()),timeout:5000}).await;}}}}let _=tx.send(CoreEvent::BackgroundNotice("自动测速已完成".into()));}));}}}}
                if let Some(command)=command {
                    if let Command::PollEvery(value)=command{poll_every=value.clamp(1,60);continue;}
                    if let Command::ImportProfile { request, url, proxy }=&command {
                        let source=crate::subscriptions::Source{name:String::new(),url:url.clone(),proxy:proxy.clone()};
                        let result=tokio::select!{_=stopped.changed()=>break,result=crate::subscriptions::fetch(&source)=>result}.map_err(|error|error.to_string());
                        let _=events_tx.send(CoreEvent::ProfileImported{request:*request,result});
                        continue;
                    }
                    if let Command::Extra(task)=command {let result=tokio::select!{_=stopped.changed()=>break,result=crate::extras::execute(task)=>result};match result{Ok(result)=>{let _=events_tx.send(CoreEvent::Extra(result));let _=events_tx.send(CoreEvent::Completed(Ok(())));},Err(error)=>{let _=events_tx.send(CoreEvent::Completed(Err(error.to_string())));}}continue;}
                    if let Command::Backup(task)=command {
                        let result=if let Some(context)=&workspace {tokio::select!{_=stopped.changed()=>break,result=crate::backup::execute(context,&client,task)=>result}}else{Err(anyhow!("备份操作需要独立工作区"))};
                        match result{Ok(result)=>{let _=events_tx.send(CoreEvent::Backups(result));let _=events_tx.send(CoreEvent::Completed(Ok(())));},Err(error)=>{let _=events_tx.send(CoreEvent::Completed(Err(error.to_string())));}}
                        continue;
                    }
                    if let Command::Workspace(task)=command {
                        let result=if let Some(context)=&workspace{tokio::select!{_=stopped.changed()=>break,result=crate::workspace::execute(context,&client,task)=>result}}else{Err(anyhow!("此操作需要受管理的工作区"))};
                        match result {Ok(snapshot)=>{let proxy_state=if let Some(context)=&workspace{let host=snapshot.state.preferences.get("proxy_host").map(String::as_str).unwrap_or("127.0.0.1");let port=snapshot.state.overrides.get(serde_yaml_ng::Value::from("mixed-port")).and_then(serde_yaml_ng::Value::as_u64).unwrap_or(context.port as u64)as u16;Some(crate::platform::SystemProxy::new(context.dir.clone()).status(host,port).await.unwrap_or("不可用".into()))}else{None};let _=events_tx.send(CoreEvent::Workspace(Box::new(snapshot)));if let Some(status)=proxy_state{let _=events_tx.send(CoreEvent::SystemProxyStatus(status));}let _=events_tx.send(if automatic{CoreEvent::BackgroundNotice("定时更新完成".into())}else{CoreEvent::Completed(Ok(()))});},Err(error)=>{let _=events_tx.send(if automatic{CoreEvent::BackgroundNotice(format!("定时更新失败：{error}"))}else{CoreEvent::Completed(Err(error.to_string()))});}}
                        continue;
                    }
                    let result=tokio::select! {_=stopped.changed()=>break,result=client.execute(&command)=>result};
                    let mut result=result.map_err(|e|e.to_string());
                    if result.is_ok()&&matches!(&command,Command::Select{..}){if let Some(context)=&workspace{if crate::workspace::load(&context.dir).ok().is_some_and(|s|s.state.preferences.get("close_connections").is_some_and(|v|v=="开启")){result=client.execute(&Command::Close(None)).await.map_err(|e|format!("节点已切换，但关闭连接失败：{e}"));}}}
                    if result.is_err() {let _=events_tx.send(CoreEvent::Completed(result));continue;}
                    let snapshot=tokio::select! {_=stopped.changed()=>break,result=client.snapshot()=>result};
                    match snapshot {Ok(snapshot)=>{let _=events_tx.send(CoreEvent::Snapshot(Box::new(snapshot)));},Err(e)=>{let _=events_tx.send(CoreEvent::Offline(e.to_string()));}}
                    let _=events_tx.send(CoreEvent::Completed(Ok(())));
                } else {
                    if let Some(context)=&workspace{let _=crate::extras::rotate_log(context);}
                    if !ticks.is_multiple_of(poll_every){if let Err(error)=client.get(&["version"]).await{let _=events_tx.send(CoreEvent::Offline(error.to_string()));}continue;}
                    let result=tokio::select! {_=stopped.changed()=>break,result=client.snapshot()=>result};
                    match result {Ok(snapshot)=>{let _=events_tx.send(CoreEvent::Snapshot(Box::new(snapshot)));},Err(e)=>{let _=events_tx.send(CoreEvent::Offline(e.to_string()));}}
                }
            }
            if let Some(task)=auto_delay{task.abort();}
if let Some(task)=auto_update{task.abort();}
            log_task.abort();let _=log_task.await;
        })).context("无法启动内核连接线程")?;
        Ok(Self {
            commands,
            events,
            logs,
            stop,
            thread: Some(thread),
        })
    }
    pub fn send(&self, command: Command) -> Result<()> {
        self.commands
            .try_send(command)
            .map_err(|_| anyhow!("请求队列已满或连接线程已退出"))
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
