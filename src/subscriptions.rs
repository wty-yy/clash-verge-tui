use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_yaml_ng::{Mapping, Value};
use sha2::{Digest, Sha256};
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
impl StoredProfile {
    pub fn usage(&self) -> Option<(u64, u64, Option<u64>)> {
        let fields: std::collections::BTreeMap<_, _> = self
            .user_info
            .as_deref()?
            .split(';')
            .filter_map(|part| {
                let (key, value) = part.trim().split_once('=')?;
                Some((key.trim(), value.trim().parse::<u64>().ok()?))
            })
            .collect();
        let total = *fields.get("total")?;
        if total == 0 {
            return None;
        }
        Some((
            fields
                .get("upload")
                .copied()
                .unwrap_or(0)
                .saturating_add(fields.get("download").copied().unwrap_or(0)),
            total,
            fields.get("expire").copied().filter(|e| *e > 0),
        ))
    }
    pub fn usage_label(&self) -> String {
        self.usage()
            .map(|(used, total, _)| {
                format!(
                    "{} / {}",
                    crate::live::bytes(used),
                    crate::live::bytes(total)
                )
            })
            .unwrap_or_else(|| format!("{} 节点", self.proxies))
    }
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
    if proxies == 0
        && providers == 0
        && config
            .get("rules")
            .and_then(Value::as_sequence)
            .is_none_or(Vec::is_empty)
    {
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
        .header(
            "User-Agent",
            concat!("clash-verge-tui/", env!("CARGO_PKG_VERSION")),
        )
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
    let root = if dir.file_name().is_some_and(|n| n == "profiles") {
        dir.parent().unwrap_or(dir)
    } else {
        dir
    };
    let _lock = crate::workspace::Lock::acquire(root)?;
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
        match download(&source, dir, crate::workspace::next_file_id(dir)).await {
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
    if root.join("workspace-state.json").exists() {
        crate::workspace::synchronize_import(root, profiles)?;
    } else {
        private_write(
            &dir.join("index.json"),
            &serde_json::to_vec_pretty(&profiles)?,
        )?;
    }
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

pub fn prepare_binary(binary: &Path, dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let source = if binary.is_absolute() || binary.components().count() > 1 {
        fs::canonicalize(binary)?
    } else {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|p| p.join(binary))
            .find(|p| p.is_file())
            .ok_or_else(|| anyhow!("找不到 mihomo 可执行文件"))?
    };
    let owned = dir.join("mihomo");
    let metadata = fs::metadata(&source)?;
    let fingerprint = format!(
        "{}:{}:{}",
        source.display(),
        metadata.len(),
        metadata
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );
    let source_record = dir.join("binary-source.txt");
    let same_binary = source == owned
        || (owned.is_file()
            && fs::metadata(&owned)?.len() == metadata.len()
            && Sha256::digest(fs::read(&owned)?) == Sha256::digest(fs::read(&source)?));
    if !same_binary {
        if fs::symlink_metadata(&owned)
            .ok()
            .is_some_and(|m| m.file_type().is_symlink())
        {
            bail!("独立内核文件不能是符号链接");
        }
        let staging = dir.join("mihomo-new");
        fs::copy(&source, &staging)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&staging, fs::Permissions::from_mode(0o700))?;
        }
        fs::rename(staging, &owned)?;
    }
    if fs::symlink_metadata(&owned)?.file_type().is_symlink() {
        bail!("独立内核文件不能是符号链接");
    }
    private_write(&source_record, fingerprint.as_bytes())?;
    private_write(
        &dir.join("managed-core.json"),
        &serde_json::to_vec_pretty(&serde_json::json!({
            "app_version": env!("CARGO_PKG_VERSION"),
            "mihomo_version": crate::core_manager::MIHOMO_VERSION,
            "managed": true
        }))?,
    )?;
    Ok(owned)
}
pub fn ensure_secret(dir: &Path, configured: Option<&str>) -> Result<String> {
    fs::create_dir_all(dir)?;
    let file = dir.join("controller.secret");
    if let Some(value) = configured.filter(|s| !s.is_empty()) {
        private_write(&file, value.as_bytes())?;
        return Ok(value.into());
    }
    if file.exists() {
        let value = fs::read_to_string(&file)?.trim().to_string();
        if !value.is_empty() {
            return Ok(value);
        }
    }
    let mut bytes = [0; 32];
    getrandom::getrandom(&mut bytes).map_err(|_| anyhow!("生成控制器密钥失败"))?;
    let value = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    private_write(&file, value.as_bytes())?;
    Ok(value)
}
pub struct ManagedCore {
    child: Child,
    pub controller: String,
    pub secret: String,
    pub binary: PathBuf,
    pub external_controller: String,
}
impl ManagedCore {
    pub fn start(
        binary: &Path,
        dir: &Path,
        profile: &StoredProfile,
        port: u16,
        controller_port: u16,
    ) -> Result<Self> {
        Self::start_with_payload(binary, dir, profile, port, controller_port, None)
    }
    pub fn start_with_payload(
        binary: &Path,
        dir: &Path,
        profile: &StoredProfile,
        port: u16,
        controller_port: u16,
        prepared: Option<String>,
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
        let secret = ensure_secret(&dir, None)?;
        let default_controller = format!("127.0.0.1:{controller_port}");
        let payload = if let Some(payload) = prepared {
            payload
        } else {
            normalized_config(
                &fs::read_to_string(&profile.file)?,
                &default_controller,
                &secret,
                port,
            )?
        };
        let parsed: Value = serde_yaml_ng::from_str(&payload)?;
        let external_controller = parsed["external-controller"]
            .as_str()
            .unwrap_or(&default_controller)
            .to_string();
        let config = dir.join("config.yaml");
        private_write(&config, payload.as_bytes())?;
        let log = dir.join("core.log");
        if !log.exists() {
            private_write(&log, b"")?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&log, fs::Permissions::from_mode(0o600))?;
        }
        let output = fs::OpenOptions::new().append(true).open(log)?;
        let owned = prepare_binary(binary, &dir)?;
        if dir
            .join("controller.sock")
            .as_os_str()
            .as_encoded_bytes()
            .len()
            > 100
        {
            bail!("数据目录过长，请使用较短路径以创建 Unix socket");
        }
        let mut command = Command::new(&owned);
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                command.pre_exec(|| {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        let child = command
            .args(["-d", dir.to_str().unwrap(), "-f", config.to_str().unwrap()])
            .arg("-ext-ctl-unix")
            .arg(dir.join("controller.sock"))
            .stdin(Stdio::null())
            .stdout(output.try_clone()?)
            .stderr(output)
            .spawn()
            .context("无法启动指定的 mihomo 可执行文件")?;
        Ok(Self {
            child,
            controller: format!("unix://{}", dir.join("controller.sock").display()),
            secret,
            binary: owned,
            external_controller,
        })
    }
    pub fn is_running(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
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
        if self.child.try_wait().ok().flatten().is_some() {
            return;
        }
        #[cfg(unix)]
        unsafe {
            libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
