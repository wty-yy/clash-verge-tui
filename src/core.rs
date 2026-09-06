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
    Offline(String),
    Completed(Result<(), String>),
}
#[derive(Debug)]
pub enum LogEvent {
    Entry { level: String, message: String },
    Status(String),
}

impl CoreClient {
    pub fn new(endpoint: &str, secret: String) -> Result<Self> {
        let endpoint = Url::parse(endpoint).map_err(|_| anyhow!("控制器地址无效"))?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            bail!("控制器地址需为 http(s) URL；密钥请使用文件或环境变量传入");
        }
        let client = Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
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
            loop {
                let command=tokio::select! {_=stopped.changed()=>break, command=receiver.recv()=>{if command.is_none(){break;}command},_=interval.tick()=>None};
                if let Some(command)=command {
                    let result=tokio::select! {_=stopped.changed()=>break,result=client.execute(&command)=>result};
                    let result=result.map_err(|e|e.to_string());
                    if result.is_err() {let _=events_tx.send(CoreEvent::Completed(result));continue;}
                    let snapshot=tokio::select! {_=stopped.changed()=>break,result=client.snapshot()=>result};
                    match snapshot {Ok(snapshot)=>{let _=events_tx.send(CoreEvent::Snapshot(Box::new(snapshot)));},Err(e)=>{let _=events_tx.send(CoreEvent::Offline(e.to_string()));}}
                    let _=events_tx.send(CoreEvent::Completed(Ok(())));
                } else {
                    let result=tokio::select! {_=stopped.changed()=>break,result=client.snapshot()=>result};
                    match result {Ok(snapshot)=>{let _=events_tx.send(CoreEvent::Snapshot(Box::new(snapshot)));},Err(e)=>{let _=events_tx.send(CoreEvent::Offline(e.to_string()));}}
                }
            }
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
