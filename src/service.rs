use anyhow::{anyhow, bail, Result};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
pub fn name(dir: &Path) -> String {
    let mut hash = 14695981039346656037u64;
    for byte in dir.as_os_str().as_encoded_bytes() {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(1099511628211);
    }
    format!("clash-verge-tui-{hash:016x}.service")
}
fn quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
            .replace('$', "$$")
    )
}
pub fn unit(app: &Path, dir: &Path, environment: &BTreeMap<String, String>) -> Result<String> {
    for value in [app, dir] {
        if !value.is_absolute() || value.as_os_str().as_encoded_bytes().contains(&b'\n') {
            bail!("服务路径必须为不含换行的绝对路径");
        }
    }
    let mut text=format!("# Managed by clash-verge-tui\n[Unit]\nDescription=Clash Verge TUI background service\nAfter=network-online.target\n\n[Service]\nType=simple\nExecStart={} --daemon --data-dir {}\nRestart=on-failure\nRestartSec=3\nTimeoutStopSec=10\nKillMode=control-group\n",quote(&app.to_string_lossy()),quote(&dir.to_string_lossy()));
    for (key, value) in environment {
        if key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && !value.contains(['\n', '\r'])
        {
            text.push_str(&format!(
                "Environment={}\n",
                quote(&format!("{key}={value}"))
            ));
        }
    }
    text.push_str("\n[Install]\nWantedBy=default.target\n");
    Ok(text)
}
async fn systemctl(args: &[&str]) -> Result<String> {
    let mut command = tokio::process::Command::new("systemctl");
    command
        .arg("--user")
        .args(args)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .map_err(|_| anyhow!("用户服务请求超时"))??;
    if !output.status.success() {
        bail!("用户服务操作失败，请检查 systemctl --user 状态");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().into())
}
fn path(dir: &Path) -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    config.join("systemd/user").join(name(dir))
}
pub async fn status(dir: &Path) -> String {
    systemctl(&["show", &name(dir), "--property=ActiveState", "--value"])
        .await
        .unwrap_or("not-installed".into())
}
pub async fn install(dir: &Path, enabled: bool) -> Result<()> {
    let app = std::fs::canonicalize(std::env::current_exe()?)?;
    let mut env = BTreeMap::new();
    if let Ok(path) = std::env::var("PATH") {
        env.insert("PATH".into(), path);
    }
    for key in ["GSETTINGS_BACKEND", "XDG_CONFIG_HOME"] {
        if let Ok(value) = std::env::var(key) {
            env.insert(key.into(), value);
        }
    }
    let file = path(dir);
    if file.exists() {
        let previous = std::fs::read(&file)?;
        crate::subscriptions::private_write(&dir.join("service.previous"), &previous)?;
    }
    crate::subscriptions::private_write(&file, unit(&app, dir, &env)?.as_bytes())?;
    systemctl(&["daemon-reload"]).await?;
    if enabled {
        systemctl(&["enable", &name(dir)]).await?;
    } else {
        systemctl(&["disable", &name(dir)]).await?;
    }
    Ok(())
}
pub async fn action(dir: &Path, action: &str) -> Result<String> {
    match action {
        "start" | "stop" | "restart" => systemctl(&[action, &name(dir)]).await,
        "status" => Ok(status(dir).await),
        "uninstall" => {
            let _ = systemctl(&["stop", &name(dir)]).await;
            let _ = systemctl(&["disable", &name(dir)]).await;
            let file = path(dir);
            if file.exists() {
                if !std::fs::read_to_string(&file)?.starts_with("# Managed by clash-verge-tui") {
                    bail!("拒绝删除非本工具管理的服务文件");
                }
                std::fs::remove_file(file)?;
            }
            systemctl(&["daemon-reload"]).await?;
            Ok("uninstalled".into())
        }
        _ => bail!("未知服务操作"),
    }
}

pub struct RunningCore {
    pub child: Option<crate::subscriptions::ManagedCore>,
    pub context: crate::workspace::WorkspaceContext,
    pub endpoint: String,
    pub active: usize,
    pub profiles: Vec<crate::subscriptions::StoredProfile>,
}
pub async fn open(
    dir: &Path,
    binary: &Path,
    port: Option<u16>,
    controller_port: Option<u16>,
    profile: Option<usize>,
) -> Result<RunningCore> {
    use crate::{
        core::CoreClient,
        subscriptions::{self, StoredProfile},
        workspace,
    };
    std::fs::create_dir_all(dir)?;
    let dir = std::fs::canonicalize(dir)?;
    let mut state = workspace::initialize(&dir)?;
    let old_active = state.active;
    let active = profile.unwrap_or(state.active);
    if !state.profiles.is_empty() && active >= state.profiles.len() {
        bail!("订阅编号不存在");
    }
    state.active = active;
    let endpoint = format!("unix://{}", dir.join("core/controller.sock").display());
    if let Ok(secret) = std::fs::read_to_string(dir.join("core/controller.secret")) {
        if let Ok(client) = CoreClient::new(&endpoint, secret.trim().into()) {
            if let Ok(config) = client.get(&["configs"]).await {
                let running_version = client
                    .get(&["version"])
                    .await
                    .ok()
                    .and_then(|value| value["version"].as_str().map(str::to_owned));
                let expected_version = format!("v{}", crate::core_manager::MIHOMO_VERSION);
                if running_version.as_deref() != Some(expected_version.as_str()) {
                    bail!(
                        "运行中的内核不是 v{}；请执行 clash-verge-tui --service restart",
                        crate::core_manager::MIHOMO_VERSION
                    );
                }
                let actual_port = config["mixed-port"].as_u64().unwrap_or(17897) as u16;
                if port.is_some_and(|p| p != actual_port) {
                    bail!("已有内核正在运行，停止服务后再更改启动端口");
                }
                let port = actual_port;
                let saved: serde_yaml_ng::Value =
                    std::fs::read_to_string(dir.join("core/config.yaml"))
                        .ok()
                        .and_then(|s| serde_yaml_ng::from_str(&s).ok())
                        .unwrap_or_default();
                let controller = saved["external-controller"].as_str().unwrap_or("").into();
                let context = workspace::WorkspaceContext {
                    dir: dir.clone(),
                    binary: dir.join("core/mihomo"),
                    controller,
                    secret: secret.trim().into(),
                    port,
                };
                if profile.is_some() && active != old_active {
                    state = workspace::execute(
                        &context,
                        &client,
                        workspace::WorkspaceCommand::Select(active),
                    )
                    .await?;
                }
                return Ok(RunningCore {
                    child: None,
                    context,
                    endpoint,
                    active: state.active,
                    profiles: state.profiles,
                });
            }
        }
    }
    if let Some(script) = state
        .state
        .preferences
        .get("start_script")
        .filter(|s| !s.trim().is_empty())
    {
        if std::env::var("CLASH_VERGE_SKIP_STARTUP").ok().as_deref() != Some("1") {
            let log = dir.join("startup.log");
            crate::subscriptions::private_write(&log, b"")?;
            let out = std::fs::OpenOptions::new().append(true).open(log)?;
            let mut child = tokio::process::Command::new("/bin/sh")
                .args(["-c", script])
                .current_dir(&dir)
                .stdin(Stdio::null())
                .stdout(out.try_clone()?)
                .stderr(out)
                .kill_on_drop(true)
                .spawn()?;
            let status = tokio::time::timeout(Duration::from_secs(30), child.wait())
                .await
                .map_err(|_| {
                    anyhow!("启动脚本超时；可设置 CLASH_VERGE_SKIP_STARTUP=1 恢复启动")
                })??;
            if !status.success() {
                bail!("启动脚本失败，详情见私有 startup.log");
            }
            state = workspace::load(&dir)?;
            state.active = active;
        }
    }
    let port = port
        .or_else(|| {
            state
                .state
                .overrides
                .get(serde_yaml_ng::Value::from("mixed-port"))
                .and_then(serde_yaml_ng::Value::as_u64)
                .map(|p| p as u16)
        })
        .unwrap_or(17897);
    let configured = state
        .state
        .preferences
        .get("controller_addr")
        .and_then(|s| s.parse::<std::net::SocketAddr>().ok());
    let controller_port = controller_port
        .or(configured.map(|a| a.port()))
        .unwrap_or(19097);
    let controller = if state
        .state
        .preferences
        .get("controller")
        .is_some_and(|s| s == "关闭")
    {
        String::new()
    } else {
        std::net::SocketAddr::new(
            configured
                .map(|a| a.ip())
                .unwrap_or(std::net::Ipv4Addr::LOCALHOST.into()),
            controller_port,
        )
        .to_string()
    };
    let secret = subscriptions::ensure_secret(
        &dir.join("core"),
        state.state.preferences.get("secret").map(String::as_str),
    )?;
    let owned = subscriptions::prepare_binary(binary, &dir.join("core"))?;
    let context = workspace::WorkspaceContext {
        dir: dir.clone(),
        binary: owned.clone(),
        controller,
        secret,
        port,
    };
    state
        .state
        .overrides
        .insert("mixed-port".into(), port.into());
    let payload = workspace::compose(&state, &context).await?;
    workspace::validate(&payload, &context).await?;
    let bootstrap;
    let profile = if let Some(p) = state.profiles.get(active) {
        p
    } else {
        let file = dir.join("core/bootstrap.yaml");
        subscriptions::private_write(&file, payload.as_bytes())?;
        bootstrap = StoredProfile {
            name: "初始直连".into(),
            url: String::new(),
            file,
            proxies: 0,
            groups: 0,
            user_info: None,
            proxy: None,
        };
        &bootstrap
    };
    let mut child = crate::subscriptions::ManagedCore::start_with_payload(
        &owned,
        &dir.join("core"),
        profile,
        port,
        controller_port,
        Some(payload),
    )?;
    let client = CoreClient::new(&child.controller, child.secret.clone())?;
    child.wait_ready(&client).await?;
    Ok(RunningCore {
        endpoint: child.controller.clone(),
        child: Some(child),
        context,
        active,
        profiles: state.profiles,
    })
}
