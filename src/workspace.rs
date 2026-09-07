use crate::{
    core::{Command as CoreCommand, CoreClient},
    model::Enhancement,
    subscriptions::{self, Source, StoredProfile},
};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::{Mapping, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::io::AsyncWriteExt;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProfileSchedule {
    pub interval_minutes: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub checked_at: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WorkspaceState {
    #[serde(default)]
    pub schedules: BTreeMap<String, ProfileSchedule>,
    #[serde(default)]
    pub enhancements: Vec<Enhancement>,
    #[serde(default)]
    pub overrides: Mapping,
    #[serde(default)]
    pub preferences: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub profiles: Vec<StoredProfile>,
    pub active: usize,
    pub state: WorkspaceState,
}
#[derive(Clone, Debug)]
pub struct WorkspaceContext {
    pub dir: PathBuf,
    pub binary: PathBuf,
    pub controller: String,
    pub secret: String,
    pub port: u16,
}
#[derive(Clone, Debug)]
pub enum WorkspaceCommand {
    Read,
    Settings(BTreeMap<String, String>),
    EditRule {
        index: Option<usize>,
        rule: Option<String>,
    },
    ResetRules,
    Restore {
        snapshot: Box<WorkspaceSnapshot>,
        files: BTreeMap<String, String>,
    },
    PutProfile {
        index: Option<usize>,
        name: String,
        url: String,
        proxy: Option<String>,
        content: Option<String>,
        interval: u64,
    },
    Refresh(usize),
    Delete(usize),
    Move {
        index: usize,
        destination: usize,
    },
    Select(usize),
    PutEnhancement {
        index: Option<usize>,
        item: Enhancement,
    },
    DeleteEnhancement(usize),
    ToggleEnhancement(usize),
    MoveEnhancement {
        index: usize,
        destination: usize,
    },
}
pub fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub struct Lock(std::fs::File);
impl Lock {
    pub fn acquire(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir)?;
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join("workspace.lock"))?;
        fs2::FileExt::try_lock_exclusive(&file)
            .map_err(|_| anyhow!("另一个工作区操作正在运行，请稍后重试"))?;
        Ok(Self(file))
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}
pub fn load(dir: &Path) -> Result<WorkspaceSnapshot> {
    let manifest = dir.join("workspace-state.json");
    if manifest.exists() {
        return serde_json::from_slice(&fs::read(manifest)?)
            .map_err(|_| anyhow!("工作区清单损坏，原文件已保留"));
    }
    let profiles = subscriptions::load_profiles(&dir.join("profiles"))?;
    let active = fs::read_to_string(dir.join("profiles/active.json"))
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Ok(WorkspaceSnapshot {
        active: active.min(profiles.len().saturating_sub(1)),
        profiles,
        state: WorkspaceState::default(),
    })
}
pub fn initialize(dir: &Path) -> Result<WorkspaceSnapshot> {
    let _lock = Lock::acquire(dir)?;
    let snapshot = load(dir)?;
    save(dir, &snapshot)?;
    Ok(snapshot)
}
fn save(dir: &Path, snapshot: &WorkspaceSnapshot) -> Result<()> {
    // The complete manifest is the single authoritative commit point.
    subscriptions::private_write(
        &dir.join("workspace-state.json"),
        &serde_json::to_vec_pretty(snapshot)?,
    )?;
    let _ = subscriptions::private_write(
        &dir.join("profiles/index.json"),
        &serde_json::to_vec_pretty(&snapshot.profiles)?,
    );
    let _ = subscriptions::private_write(
        &dir.join("profiles/active.json"),
        snapshot.active.to_string().as_bytes(),
    );
    Ok(())
}
pub fn synchronize_import(dir: &Path, profiles: Vec<StoredProfile>) -> Result<()> {
    if !dir.join("workspace-state.json").exists() {
        return Ok(());
    }
    let mut snapshot = load(dir)?;
    let active = snapshot
        .profiles
        .get(snapshot.active)
        .map(|p| p.url.clone());
    snapshot.active = active
        .and_then(|url| profiles.iter().position(|p| p.url == url))
        .unwrap_or(0);
    snapshot.profiles = profiles;
    save(dir, &snapshot)
}
struct DraftFile(Option<PathBuf>);
impl Drop for DraftFile {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_file(path);
        }
    }
}
pub fn set_active_on_start(dir: &Path, index: usize) -> Result<()> {
    let _lock = Lock::acquire(dir)?;
    let mut snapshot = load(dir)?;
    ensure(index, snapshot.profiles.len())?;
    snapshot.active = index;
    save(dir, &snapshot)
}
fn lowercase_root(value: Value) -> Value {
    match value {
        Value::Mapping(map) => Value::Mapping(
            map.into_iter()
                .map(|(key, value)| {
                    (
                        key.as_str()
                            .map(|k| Value::String(k.to_ascii_lowercase()))
                            .unwrap_or(key),
                        value,
                    )
                })
                .collect(),
        ),
        other => other,
    }
}
pub fn merge(base: &mut Value, patch: Value) {
    if let (Value::Mapping(base), Value::Mapping(patch)) = (&mut *base, &patch) {
        for (key, value) in patch {
            if let Some(old) = base.get_mut(key) {
                merge(old, value.clone());
            } else {
                base.insert(key.clone(), value.clone());
            }
        }
    } else {
        *base = patch;
    }
}
async fn script(config: &Value, code: &str) -> Result<Value> {
    // A separate Node process bounds execution time and memory. It receives only JSON over stdin.
    let runner = r#"const vm=require('node:vm');let input='';process.stdin.on('data',x=>input+=x);process.stdin.on('end',()=>{try{const x=JSON.parse(input);const sandbox=Object.create(null);sandbox.json=JSON.stringify(x.config);const ctx=vm.createContext(sandbox,{codeGeneration:{strings:false,wasm:false}});const result=vm.runInContext('const config=JSON.parse(json);\n'+x.code+'\nJSON.stringify(main(config));',ctx,{timeout:1500});if(typeof result!=='string')throw Error();process.stdout.write(result);}catch(e){process.stderr.write('Script failed');process.exitCode=1;}});"#;
    let json_config = serde_json::to_value(config).context("配置无法转换为脚本对象")?;
    let mut child = tokio::process::Command::new("node")
        .args(["--max-old-space-size=64", "-e", runner])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("JavaScript 增强需要已安装 Node.js")?;
    let input = serde_json::to_vec(&serde_json::json!({"config":json_config,"code":code}))?;
    let mut stdin = child.stdin.take().unwrap();
    let writer = tokio::spawn(async move {
        stdin.write_all(&input).await?;
        stdin.shutdown().await
    });
    let result = tokio::time::timeout(Duration::from_secs(3), child.wait_with_output())
        .await
        .map_err(|_| anyhow!("JavaScript 增强执行超时"))??;
    let _ = writer.await;
    if !result.status.success() || result.stdout.len() > 8 * 1024 * 1024 {
        bail!("JavaScript 增强执行失败或结果过大");
    }
    let output: serde_json::Value = serde_json::from_slice(&result.stdout)
        .map_err(|_| anyhow!("JavaScript main 必须返回配置对象"))?;
    if !output.is_object() {
        bail!("JavaScript main 必须返回配置对象");
    }
    Ok(serde_yaml_ng::to_value(output)?)
}
pub async fn compose(snapshot: &WorkspaceSnapshot, context: &WorkspaceContext) -> Result<String> {
    let source = if let Some(profile) = snapshot.profiles.get(snapshot.active) {
        fs::read_to_string(&profile.file).context("无法读取订阅配置")?
    } else {
        "mode: direct\nproxies: []\nproxy-groups: [{name: Default, type: select, proxies: [DIRECT]}]\nrules: ['MATCH,DIRECT']\n".into()
    };
    let mut config = subscriptions::parse_config(&source)?;
    for enhancement in snapshot.state.enhancements.iter().filter(|e| e.enabled) {
        match enhancement.kind.as_str() {
            "YAML" => {
                let patch: Value = serde_yaml_ng::from_str(&enhancement.content)
                    .map_err(|_| anyhow!("YAML 增强格式无效"))?;
                if !patch.is_mapping() {
                    bail!("YAML 增强必须为对象");
                }
                merge(&mut config, lowercase_root(patch));
            }
            "JavaScript" => config = lowercase_root(script(&config, &enhancement.content).await?),
            _ => bail!("未知增强类型"),
        }
    }
    let normal = subscriptions::normalized_config(
        &serde_yaml_ng::to_string(&config)?,
        &context.controller,
        &context.secret,
        context.port,
    )?;
    let mut config: Value = serde_yaml_ng::from_str(&normal)?;
    merge(
        &mut config,
        Value::Mapping(snapshot.state.overrides.clone()),
    );
    // Controller ownership is independent of untrusted subscription/enhancement content.
    let ui = config
        .get("external-ui")
        .and_then(Value::as_str)
        .unwrap_or("ui");
    let ui = ui.strip_prefix("ui/").unwrap_or(ui);
    let relative = if ui == "ui" { "" } else { ui };
    if Path::new(relative)
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        bail!("网页目录必须位于工作区 ui 下");
    }
    config["external-ui"] = context
        .dir
        .join("core/ui")
        .join(relative)
        .to_string_lossy()
        .to_string()
        .into();
    if config.get("external-ui-url").is_none() {
        config["external-ui-url"] =
            "https://github.com/MetaCubeX/metacubexd/archive/refs/heads/gh-pages.zip".into();
    }
    config["external-controller"] = context.controller.clone().into();
    config["secret"] = context.secret.clone().into();
    Ok(serde_yaml_ng::to_string(&config)?)
}
fn copy_validation_resources(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if !kind.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        if name_text.ends_with(".dat")
            || name_text.ends_with(".mmdb")
            || name_text.ends_with(".metadb")
        {
            fs::copy(entry.path(), target.join(name))?;
        }
    }
    let providers = source.join("providers");
    if providers.is_dir() {
        fs::create_dir_all(target.join("providers"))?;
        for entry in fs::read_dir(providers)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                fs::copy(
                    entry.path(),
                    target.join("providers").join(entry.file_name()),
                )?;
            }
        }
    }
    Ok(())
}
pub async fn validate(payload: &str, context: &WorkspaceContext) -> Result<()> {
    fs::create_dir_all(context.dir.join("core"))?;
    let staging = tempfile::Builder::new()
        .prefix("validate-")
        .tempdir_in(context.dir.join("core"))?;
    copy_validation_resources(&context.dir.join("core"), staging.path())?;
    let path = staging.path().join("config.yaml");
    let mut validation: Value = serde_yaml_ng::from_str(payload)?;
    let ui = staging.path().join("ui");
    fs::create_dir_all(&ui)?;
    validation["external-ui"] = ui.to_string_lossy().to_string().into();
    subscriptions::private_write(&path, serde_yaml_ng::to_string(&validation)?.as_bytes())?;
    let log = context.dir.join("core/validation.log");
    subscriptions::private_write(&log, b"")?;
    let output = fs::OpenOptions::new().append(true).open(log)?;
    let mut child = tokio::process::Command::new(&context.binary)
        .arg("-t")
        .arg("-d")
        .arg(staging.path())
        .arg("-f")
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(output)
        .kill_on_drop(true)
        .spawn()
        .context("无法运行 mihomo 配置校验")?;
    let status = tokio::time::timeout(Duration::from_secs(60), child.wait())
        .await
        .map_err(|_| anyhow!("mihomo 配置校验超时"))??;
    if !status.success() {
        bail!("mihomo 拒绝配置；原配置保持不变，详情见 core/validation.log");
    }
    Ok(())
}
fn ensure(index: usize, length: usize) -> Result<()> {
    if index >= length {
        bail!("条目已变化，请刷新后重试");
    }
    Ok(())
}
pub(crate) fn next_file_id(dir: &Path) -> usize {
    fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|e| {
            e.file_name()
                .to_str()?
                .strip_prefix("profile-")?
                .strip_suffix(".yaml")?
                .parse::<usize>()
                .ok()
        })
        .max()
        .map(|i| i + 1)
        .unwrap_or(0)
}
pub async fn execute(
    context: &WorkspaceContext,
    client: &CoreClient,
    command: WorkspaceCommand,
) -> Result<WorkspaceSnapshot> {
    let _lock = Lock::acquire(&context.dir)?;
    let mut draft = DraftFile(None);
    let mut restore_drafts = Vec::new();
    let mut check_profile = None;
    let mut network_fields = None;
    let mut system_changed = false;
    let mut service_changed = false;
    let mut snapshot = load(&context.dir)?;
    let old = snapshot.clone();
    let mut apply = false;
    match command {
        WorkspaceCommand::Read => return Ok(snapshot),
        WorkspaceCommand::Restore {
            snapshot: restored,
            files,
        } => {
            let mut restored = *restored;
            if !restored.profiles.is_empty() && restored.active >= restored.profiles.len() {
                bail!("备份中的活动订阅无效");
            }
            let mut schedules = BTreeMap::new();
            for profile in &mut restored.profiles {
                let name = profile
                    .file
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| anyhow!("备份配置名无效"))?;
                if !name.starts_with("profile-") || !name.ends_with(".yaml") {
                    bail!("备份包含不支持的配置路径");
                }
                let content = files.get(name).ok_or_else(|| anyhow!("备份缺少配置内容"))?;
                subscriptions::parse_config(content)?;
                let path = context.dir.join(format!(
                    "profiles/profile-{}.yaml",
                    next_file_id(&context.dir.join("profiles"))
                ));
                subscriptions::private_write(&path, content.as_bytes())?;
                restore_drafts.push(DraftFile(Some(path.clone())));
                if let Some(schedule) = restored
                    .state
                    .schedules
                    .get(&profile.file.to_string_lossy().into_owned())
                {
                    schedules.insert(path.to_string_lossy().into_owned(), schedule.clone());
                }
                profile.file = path;
            }
            restored.state.schedules = schedules;
            let mut fields = restored.state.preferences.clone();
            let tun = restored.state.overrides.get(Value::from("tun"));
            fields.insert(
                "tun".into(),
                if tun
                    .and_then(|v| v.get("enable"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    "开启"
                } else {
                    "关闭"
                }
                .into(),
            );
            network_fields = Some(fields);
            snapshot = restored;
            apply = true;
            system_changed = true;
        }
        WorkspaceCommand::Settings(values) => {
            if values.get("tun").is_some_and(|value| value == "开启")
                && unsafe { libc::geteuid() } != 0
                && !crate::service::tun_capable(&context.binary)
            {
                bail!("TUN 权限未安装；请从首页启用 TUN 并按提示安装权限服务");
            }
            crate::network::apply(&mut snapshot.state.overrides, &values)?;
            network_fields = Some(values.clone());
            service_changed = values
                .keys()
                .any(|k| ["service", "auto_launch"].contains(&k.as_str()));
            system_changed = values.keys().any(|k| {
                ["system_proxy", "proxy_host", "bypass", "pac", "pac_script"].contains(&k.as_str())
            });
            if values.contains_key("mixed_port")
                && snapshot
                    .state
                    .preferences
                    .get("system_proxy")
                    .is_some_and(|v| v == "开启")
            {
                system_changed = true;
            }
            snapshot.state.preferences.extend(values);
            apply = snapshot.state.overrides != old.state.overrides;
        }
        WorkspaceCommand::EditRule { index, rule } => {
            let config: Value = serde_yaml_ng::from_str(&compose(&snapshot, context).await?)?;
            let mut rules = config["rules"].as_sequence().cloned().unwrap_or_default();
            match (index, rule) {
                (Some(i), Some(rule)) => {
                    ensure(i, rules.len())?;
                    rules[i] = rule.into();
                }
                (Some(i), None) => {
                    ensure(i, rules.len())?;
                    rules.remove(i);
                }
                (None, Some(rule)) => rules.insert(0, rule.into()),
                _ => {}
            }
            snapshot
                .state
                .overrides
                .insert("rules".into(), Value::Sequence(rules));
            apply = true;
        }
        WorkspaceCommand::ResetRules => {
            snapshot.state.overrides.remove(Value::from("rules"));
            apply = true;
        }
        WorkspaceCommand::PutProfile {
            index,
            name,
            url,
            proxy,
            content,
            interval,
        } => {
            if name.trim().is_empty() {
                bail!("订阅名称不能为空");
            }
            if let Some(i) = index {
                ensure(i, snapshot.profiles.len())?;
            }
            let id = next_file_id(&context.dir.join("profiles"));
            let metadata_only = index
                .and_then(|i| snapshot.profiles.get(i))
                .filter(|p| p.url == url && content.is_none())
                .cloned();
            let profile = if let Some(mut profile) = metadata_only {
                profile.name = name;
                profile.proxy = proxy;
                profile
            } else if let Some(content) = content {
                let config = subscriptions::parse_config(&content)?;
                let file = context.dir.join(format!("profiles/profile-{id}.yaml"));
                subscriptions::private_write(&file, content.as_bytes())?;
                StoredProfile {
                    name,
                    url,
                    file,
                    proxy,
                    proxies: config["proxies"].as_sequence().map(Vec::len).unwrap_or(0),
                    groups: config["proxy-groups"]
                        .as_sequence()
                        .map(Vec::len)
                        .unwrap_or(0),
                    user_info: None,
                }
            } else {
                subscriptions::download(
                    &Source { name, url, proxy },
                    &context.dir.join("profiles"),
                    id,
                )
                .await?
            };
            let previous_schedule = index
                .and_then(|i| {
                    snapshot
                        .state
                        .schedules
                        .get(&snapshot.profiles[i].file.to_string_lossy().into_owned())
                })
                .cloned();
            let fresh = index.is_none_or(|i| snapshot.profiles[i].file != profile.file);
            if fresh {
                draft.0 = Some(profile.file.clone());
            }
            check_profile = Some(index.unwrap_or(snapshot.profiles.len()));
            let key = profile.file.to_string_lossy().into_owned();
            snapshot.state.schedules.insert(
                key,
                ProfileSchedule {
                    interval_minutes: interval,
                    updated_at: if fresh {
                        timestamp()
                    } else {
                        previous_schedule
                            .as_ref()
                            .map(|s| s.updated_at)
                            .unwrap_or(0)
                    },
                    checked_at: previous_schedule
                        .as_ref()
                        .map(|s| s.checked_at)
                        .unwrap_or(0),
                },
            );
            if let Some(i) = index {
                snapshot.profiles[i] = profile;
                apply = fresh && i == snapshot.active;
            } else {
                snapshot.profiles.push(profile);
                apply = old.profiles.is_empty();
            }
        }
        WorkspaceCommand::Refresh(i) => {
            ensure(i, snapshot.profiles.len())?;
            let p = &snapshot.profiles[i];
            if p.url.is_empty() {
                bail!("本地配置无需下载");
            }
            let old_key = p.file.to_string_lossy().into_owned();
            let mut schedule = snapshot
                .state
                .schedules
                .get(&old_key)
                .cloned()
                .unwrap_or_default();
            let new = subscriptions::download(
                &Source {
                    name: p.name.clone(),
                    url: p.url.clone(),
                    proxy: p.proxy.clone(),
                },
                &context.dir.join("profiles"),
                next_file_id(&context.dir.join("profiles")),
            )
            .await?;
            draft.0 = Some(new.file.clone());
            check_profile = Some(i);
            schedule.updated_at = timestamp();
            schedule.checked_at = timestamp();
            snapshot.state.schedules.remove(&old_key);
            snapshot
                .state
                .schedules
                .insert(new.file.to_string_lossy().into_owned(), schedule);
            snapshot.profiles[i] = new;
            apply = i == snapshot.active;
        }
        WorkspaceCommand::Delete(i) => {
            ensure(i, snapshot.profiles.len())?;
            snapshot.profiles.remove(i);
            apply = i == snapshot.active;
            if i < snapshot.active {
                snapshot.active -= 1;
            }
            snapshot.active = snapshot
                .active
                .min(snapshot.profiles.len().saturating_sub(1));
        }
        WorkspaceCommand::Move { index, destination } => {
            ensure(index, snapshot.profiles.len())?;
            ensure(destination, snapshot.profiles.len())?;
            snapshot.profiles.swap(index, destination);
            if snapshot.active == index {
                snapshot.active = destination;
            } else if snapshot.active == destination {
                snapshot.active = index;
            }
        }
        WorkspaceCommand::Select(i) => {
            ensure(i, snapshot.profiles.len())?;
            snapshot.active = i;
            apply = true;
        }
        WorkspaceCommand::PutEnhancement { index, item } => {
            if let Some(i) = index {
                ensure(i, snapshot.state.enhancements.len())?;
                snapshot.state.enhancements[i] = item;
            } else {
                snapshot.state.enhancements.push(item);
            }
            apply = true;
        }
        WorkspaceCommand::DeleteEnhancement(i) => {
            ensure(i, snapshot.state.enhancements.len())?;
            snapshot.state.enhancements.remove(i);
            apply = true;
        }
        WorkspaceCommand::ToggleEnhancement(i) => {
            ensure(i, snapshot.state.enhancements.len())?;
            snapshot.state.enhancements[i].enabled = !snapshot.state.enhancements[i].enabled;
            apply = true;
        }
        WorkspaceCommand::MoveEnhancement { index, destination } => {
            ensure(index, snapshot.state.enhancements.len())?;
            ensure(destination, snapshot.state.enhancements.len())?;
            snapshot.state.enhancements.swap(index, destination);
            apply = true;
        }
    }
    if let Some(i) = check_profile.filter(|i| *i != snapshot.active) {
        let mut validation = snapshot.clone();
        validation.active = i;
        validate(&compose(&validation, context).await?, context).await?;
    }
    snapshot.state.schedules.retain(|key, _| {
        snapshot
            .profiles
            .iter()
            .any(|p| p.file.to_string_lossy() == key.as_str())
    });
    let candidate = compose(&snapshot, context).await?;
    validate(&candidate, context).await?;
    if old
        .state
        .preferences
        .get("auto_backup")
        .is_some_and(|v| v == "开启")
    {
        crate::backup::create(&context.dir)?;
    }
    if apply {
        let parsed: Value = serde_yaml_ng::from_str(&candidate)?;
        if let Some(fields) = network_fields.as_mut() {
            if fields.get("tun").is_some_and(|v| v == "关闭")
                && !client.get(&["configs"]).await?["tun"]["enable"]
                    .as_bool()
                    .unwrap_or(false)
            {
                fields.remove("tun");
            }
        }

        if let Some(fields) = &network_fields {
            let current = client.get(&["configs"]).await?;
            let mut previous: serde_json::Value =
                fs::read_to_string(context.dir.join("core/config.yaml"))
                    .ok()
                    .and_then(|s| serde_yaml_ng::from_str::<Value>(&s).ok())
                    .and_then(|v| serde_json::to_value(v).ok())
                    .unwrap_or(serde_json::json!({}));
            if let (Some(previous), Some(current)) = (previous.as_object_mut(), current.as_object())
            {
                previous.extend(current.clone());
            }
            crate::network::preflight(fields, &parsed, &previous).await?;
        }
        client
            .execute(&CoreCommand::Reload(candidate.clone()))
            .await?;
        if let Some(fields) = &network_fields {
            if let Err(error) = crate::network::verify_tun(fields, &parsed).await {
                if let Ok(previous) = compose(&old, context).await {
                    let _ = client.execute(&CoreCommand::Reload(previous)).await;
                }
                return Err(error);
            }
        }
    }
    if system_changed {
        let port = snapshot
            .state
            .overrides
            .get(Value::from("mixed-port"))
            .and_then(Value::as_u64)
            .unwrap_or(context.port as u64) as u16;
        if let Err(error) =
            crate::platform::apply_proxy(&context.dir, &snapshot.state.preferences, port).await
        {
            if apply {
                if let Ok(previous) = compose(&old, context).await {
                    let _ = client.execute(&CoreCommand::Reload(previous)).await;
                }
            }
            return Err(error);
        }
    }
    if let Err(error) = save(&context.dir, &snapshot) {
        if system_changed {
            let _ =
                crate::platform::apply_proxy(&context.dir, &old.state.preferences, context.port)
                    .await;
        }
        if apply {
            if let Ok(previous) = compose(&old, context).await {
                let _ = client.execute(&CoreCommand::Reload(previous)).await;
            }
        }
        return Err(error);
    }
    if apply {
        let _ = subscriptions::private_write(
            &context.dir.join("core/config.yaml"),
            candidate.as_bytes(),
        );
    }
    draft.0 = None;
    for draft in &mut restore_drafts {
        draft.0 = None;
    }
    for profile in &old.profiles {
        if !snapshot.profiles.iter().any(|p| p.file == profile.file)
            && profile.file.parent() == Some(context.dir.join("profiles").as_path())
            && profile
                .file
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("profile-"))
        {
            let _ = fs::remove_file(&profile.file);
        }
    }
    drop(_lock);
    if service_changed {
        let enabled = snapshot
            .state
            .preferences
            .get("auto_launch")
            .is_some_and(|v| v == "开启");
        let state = snapshot
            .state
            .preferences
            .get("service")
            .map(String::as_str)
            .unwrap_or("已停止");
        if state == "未安装" && !enabled {
            crate::service::action(&context.dir, "uninstall").await?;
        } else {
            crate::service::install(&context.dir, enabled).await?;
            crate::service::action(
                &context.dir,
                if state == "运行中" {
                    "start"
                } else {
                    "stop"
                },
            )
            .await?;
        }
    }
    Ok(snapshot)
}
pub fn due(dir: &Path) -> Result<Option<usize>> {
    let _lock = Lock::acquire(dir)?;
    let mut snapshot = load(dir)?;
    let now = timestamp();
    let index = snapshot.profiles.iter().enumerate().find_map(|(i, p)| {
        let schedule = snapshot
            .state
            .schedules
            .get(&p.file.to_string_lossy().into_owned())?;
        if !p.url.is_empty()
            && schedule.interval_minutes > 0
            && now.saturating_sub(schedule.updated_at.max(schedule.checked_at))
                >= schedule.interval_minutes.saturating_mul(60)
        {
            Some(i)
        } else {
            None
        }
    });
    if let Some(i) = index {
        snapshot
            .state
            .schedules
            .get_mut(&snapshot.profiles[i].file.to_string_lossy().into_owned())
            .unwrap()
            .checked_at = now;
        save(dir, &snapshot)?;
    }
    Ok(index)
}
