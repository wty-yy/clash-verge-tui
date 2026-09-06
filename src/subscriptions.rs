use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_yaml_ng::{Mapping, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub proxy: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredProfile {
    pub name: String,
    pub url: String,
    pub file: PathBuf,
    pub proxies: usize,
    pub groups: usize,
    pub user_info: Option<String>,
    #[serde(default)]
    pub proxy: Option<String>,
}
#[derive(Clone, Debug)]
pub struct ImportResult {
    pub name: String,
    pub result: Result<StoredProfile, String>,
}

pub fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&tmp)?;
    use std::io::Write;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(tmp, path)?;
    Ok(())
}
pub fn parse_config(text: &str) -> Result<Value> {
    let config: Value =
        serde_yaml_ng::from_str(text).map_err(|_| anyhow!("订阅不是有效的 Clash YAML 配置"))?;
    if !config.is_mapping() {
        bail!("订阅需要 Clash YAML 对象，暂不支持 Base64 / URI 列表");
    }
    let proxies = config["proxies"].as_sequence().map(Vec::len).unwrap_or(0);
    let providers = config["proxy-providers"]
        .as_mapping()
        .map(Mapping::len)
        .unwrap_or(0);
    if proxies == 0 && providers == 0 {
        bail!("订阅不包含代理节点或代理集合");
    }
    Ok(config)
}
pub async fn download(source: &Source, dir: &Path, index: usize) -> Result<StoredProfile> {
    let url = Url::parse(&source.url).map_err(|_| anyhow!("订阅地址无效"))?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("订阅仅支持 HTTP(S)");
    }
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(5));
    if let Some(proxy) = &source.proxy {
        builder = builder
            .no_proxy()
            .proxy(reqwest::Proxy::all(proxy).map_err(|_| anyhow!("订阅下载代理地址无效"))?);
    }
    let client = builder.build()?;
    let response = client
        .get(url)
        .header("User-Agent", "clash-verge-tui/0.2.0")
        .send()
        .await
        .map_err(|e| anyhow!("下载失败：{}", e.without_url()))?;
    if !response.status().is_success() {
        bail!("订阅服务器 HTTP {}", response.status().as_u16());
    }
    let info = response
        .headers()
        .get("subscription-userinfo")
        .and_then(|h| h.to_str().ok())
        .map(str::to_string);
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| anyhow!("订阅下载中断"))?;
        if bytes.len() + chunk.len() > 8 * 1024 * 1024 {
            bail!("订阅超过 8 MiB 限制");
        }
        bytes.extend_from_slice(&chunk);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| anyhow!("订阅不是 UTF-8 文本"))?;
    let config = parse_config(text)?;
    let file = dir.join(format!("profile-{index}.yaml"));
    private_write(&file, &bytes)?;
    Ok(StoredProfile {
        name: source.name.clone(),
        url: source.url.clone(),
        file,
        proxies: config["proxies"].as_sequence().map(Vec::len).unwrap_or(0),
        groups: config["proxy-groups"]
            .as_sequence()
            .map(Vec::len)
            .unwrap_or(0),
        user_info: info,
        proxy: source.proxy.clone(),
    })
}
pub async fn import_file(
    path: &Path,
    dir: &Path,
    proxy_override: Option<&str>,
) -> Result<Vec<ImportResult>> {
    let sources: Vec<Source> = serde_json::from_slice(&fs::read(path)?)
        .map_err(|_| anyhow!("订阅清单格式无效，应为 name/url 对象数组"))?;
    if sources.is_empty() {
        bail!("订阅清单为空");
    }
    let existing = load_profiles(dir)?;
    let mut profiles = existing;
    let mut results = Vec::new();
    for mut source in sources {
        if let Some(proxy) = proxy_override {
            source.proxy = Some(proxy.into());
        }
        let index = profiles
            .iter()
            .position(|p| p.url == source.url)
            .unwrap_or(profiles.len());
        match download(&source, dir, index).await {
            Ok(profile) => {
                if index < profiles.len() {
                    profiles[index] = profile.clone();
                } else {
                    profiles.push(profile.clone());
                }
                results.push(ImportResult {
                    name: source.name.clone(),
                    result: Ok(profile),
                });
            }
            Err(e) => results.push(ImportResult {
                name: source.name.clone(),
                result: Err(e.to_string()),
            }),
        }
    }
    private_write(
        &dir.join("index.json"),
        &serde_json::to_vec_pretty(&profiles)?,
    )?;
    Ok(results)
}
pub fn load_profiles(dir: &Path) -> Result<Vec<StoredProfile>> {
    let file = dir.join("index.json");
    if !file.exists() {
        return Ok(Vec::new());
    }
    serde_json::from_slice(&fs::read(file)?)
        .map_err(|_| anyhow!("订阅索引损坏；请保留原文件并检查"))
}

pub fn normalized_config(
    source: &str,
    controller: &str,
    secret: &str,
    port: u16,
) -> Result<String> {
    let original = parse_config(source)?;
    let mut normalized = Mapping::new();
    // Keep proxy behavior; never inherit subscription-supplied local listeners or controller settings.
    for key in [
        "proxies",
        "proxy-groups",
        "proxy-providers",
        "rule-providers",
        "rules",
        "dns",
        "hosts",
        "sniffer",
        "profile",
        "mode",
        "log-level",
        "ipv6",
        "unified-delay",
        "tcp-concurrent",
        "geodata-mode",
        "geox-url",
        "geo-auto-update",
        "geo-update-interval",
        "global-client-fingerprint",
    ] {
        if let Some(value) = original.get(key) {
            normalized.insert(key.into(), value.clone());
        }
    }
    if normalized
        .get(Value::from("proxy-groups"))
        .and_then(Value::as_sequence)
        .is_none_or(Vec::is_empty)
    {
        let nodes = original["proxies"]
            .as_sequence()
            .map(|p| {
                p.iter()
                    .filter_map(|p| p["name"].as_str())
                    .map(|s| Value::String(s.into()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let uses = original["proxy-providers"]
            .as_mapping()
            .map(|p| p.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let mut group = Mapping::new();
        group.insert("name".into(), "PROXY".into());
        group.insert("type".into(), "select".into());
        if !nodes.is_empty() {
            group.insert("proxies".into(), Value::Sequence(nodes));
        }
        if !uses.is_empty() {
            group.insert("use".into(), Value::Sequence(uses));
        }
        normalized.insert(
            "proxy-groups".into(),
            Value::Sequence(vec![Value::Mapping(group)]),
        );
        normalized.insert("rules".into(), Value::Sequence(vec!["MATCH,PROXY".into()]));
    }
    for key in ["proxy-providers", "rule-providers"] {
        if let Some(Value::Mapping(providers)) = normalized.get_mut(Value::from(key)) {
            for (i, provider) in providers.values_mut().enumerate() {
                if let Value::Mapping(map) = provider {
                    map.insert("path".into(), format!("providers/{key}-{i}.yaml").into());
                }
            }
        }
    }
    if let Some(Value::Mapping(dns)) = normalized.get_mut(Value::from("dns")) {
        dns.remove(Value::from("listen"));
    }
    normalized.insert("mixed-port".into(), port.into());
    normalized.insert("allow-lan".into(), false.into());
    normalized.insert("bind-address".into(), "127.0.0.1".into());
    normalized.insert("external-controller".into(), controller.into());
    normalized.insert("secret".into(), secret.into());
    normalized.insert(
        "tun".into(),
        serde_yaml_ng::from_str("enable: false").unwrap(),
    );
    Ok(serde_yaml_ng::to_string(&Value::Mapping(normalized))?)
}

pub struct ManagedCore {
    child: Child,
    pub controller: String,
    pub secret: String,
}
impl ManagedCore {
    pub fn start(
        binary: &Path,
        dir: &Path,
        profile: &StoredProfile,
        port: u16,
        controller_port: u16,
    ) -> Result<Self> {
        if port == 0 || controller_port == 0 || port == controller_port {
            bail!("代理端口和控制器端口必须为不同的非零端口");
        }
        for p in [port, controller_port] {
            let _ = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, p))
                .map_err(|_| anyhow!("本地端口 {p} 已占用"))?;
        }
        fs::create_dir_all(dir)?;
        let dir = fs::canonicalize(dir)?;
        let secret_file = dir.join("controller.secret");
        let secret = if secret_file.exists() {
            fs::read_to_string(&secret_file)?.trim().to_string()
        } else {
            let mut bytes = [0u8; 32];
            getrandom::getrandom(&mut bytes).map_err(|_| anyhow!("生成控制器密钥失败"))?;
            let value = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
            private_write(&secret_file, value.as_bytes())?;
            value
        };
        if secret.is_empty() {
            bail!("控制器密钥文件为空");
        }
        let controller = format!("127.0.0.1:{controller_port}");
        let payload = normalized_config(
            &fs::read_to_string(&profile.file)?,
            &controller,
            &secret,
            port,
        )?;
        let config = dir.join("config.yaml");
        private_write(&config, payload.as_bytes())?;
        let log = dir.join("core.log");
        private_write(&log, b"")?;
        let output = fs::OpenOptions::new().append(true).open(log)?;
        let child = Command::new(binary)
            .args(["-d", dir.to_str().unwrap(), "-f", config.to_str().unwrap()])
            .stdin(Stdio::null())
            .stdout(output.try_clone()?)
            .stderr(output)
            .spawn()
            .context("无法启动指定的 mihomo 可执行文件")?;
        Ok(Self {
            child,
            controller: format!("http://{controller}"),
            secret,
        })
    }
    pub async fn wait_ready(&mut self, client: &crate::core::CoreClient) -> Result<()> {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait()? {
                bail!("mihomo 提前退出（{status}）；详情保存在独立 core.log 中");
            }
            if client.get(&["version"]).await.is_ok() {
                return Ok(());
            }
            if start.elapsed() > Duration::from_secs(60) {
                bail!("mihomo 启动超时；详情保存在独立 core.log 中");
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}
impl Drop for ManagedCore {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
