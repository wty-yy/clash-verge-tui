use anyhow::{anyhow, bail, Context, Result};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
fn workspace_hash(dir: &Path) -> u64 {
    let mut hash = 14695981039346656037u64;
    for byte in dir.as_os_str().as_encoded_bytes() {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(1099511628211);
    }
    hash
}
pub fn name(dir: &Path) -> String {
    let hash = workspace_hash(dir);
    format!("clash-verge-tui-{hash:016x}.service")
}

const TUN_HELPER: &str = "/usr/libexec/clash-verge-tui/tun-helper";
const SYSTEMD_DIR: &str = "/etc/systemd/system";

pub fn tun_base_name(dir: &Path, uid: u32) -> String {
    format!("clash-verge-tui-tun-{uid}-{:016x}", workspace_hash(dir))
}

fn tun_capability_output(text: &str) -> bool {
    text.contains("cap_net_admin") && (text.contains("=ep") || text.contains("+ep"))
}

pub fn tun_capable(binary: &Path) -> bool {
    ["/usr/sbin/getcap", "/sbin/getcap", "getcap"]
        .into_iter()
        .find_map(|program| {
            std::process::Command::new(program)
                .arg(binary)
                .stdin(Stdio::null())
                .output()
                .ok()
        })
        .filter(|output| output.status.success())
        .map(|output| tun_capability_output(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or(false)
}

pub fn tun_units(app: &Path, dir: &Path, uid: u32) -> Result<(String, String)> {
    for value in [app, dir] {
        if !value.is_absolute()
            || value
                .as_os_str()
                .as_encoded_bytes()
                .iter()
                .any(|byte| matches!(byte, b'\0' | b'\n' | b'\r'))
        {
            bail!("TUN 服务路径必须为不含换行的绝对路径");
        }
    }
    let base = tun_base_name(dir, uid);
    let core_path = dir.join("core/mihomo");
    let core = quote(&core_path.to_string_lossy());
    let service = format!(
        "# Managed by clash-verge-tui\n[Unit]\nDescription=Maintain Clash Verge TUI core capabilities\n\n[Service]\nType=oneshot\nExecStart={} --tun-helper apply --tun-uid {} --data-dir {}\nCapabilityBoundingSet=CAP_SETFCAP CAP_DAC_READ_SEARCH CAP_FOWNER\nNoNewPrivileges=true\nProtectSystem=strict\nProtectHome=read-only\nReadWritePaths={}\nRestrictAddressFamilies=AF_UNIX\n",
        quote(&app.to_string_lossy()),
        uid,
        quote(&dir.to_string_lossy()),
        core
    );
    let path = format!(
        "# Managed by clash-verge-tui\n[Unit]\nDescription=Watch Clash Verge TUI core for capability updates\n\n[Path]\nPathChanged={}\nUnit={base}.service\n\n[Install]\nWantedBy=multi-user.target\n",
        path_directive(&core_path.to_string_lossy())
    );
    Ok((service, path))
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
fn path_directive(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            ' ' => escaped.push_str("\\x20"),
            '\t' => escaped.push_str("\\x09"),
            '\\' => escaped.push_str("\\x5c"),
            '"' => escaped.push_str("\\x22"),
            '\'' => escaped.push_str("\\x27"),
            '%' => escaped.push_str("%%"),
            _ => escaped.push(character),
        }
    }
    escaped
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
pub async fn wait_ready(dir: &Path) -> Result<()> {
    let endpoint = format!("unix://{}", dir.join("core/controller.sock").display());
    let secret = std::fs::read_to_string(dir.join("core/controller.secret"))?;
    let client = crate::core::CoreClient::new(&endpoint, secret.trim().into())?;
    let start = std::time::Instant::now();
    loop {
        if client.get(&["version"]).await.is_ok() {
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(60) {
            bail!("自管服务重启后内核未就绪");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
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
                let actual_port = config["mixed-port"]
                    .as_u64()
                    .unwrap_or(crate::network::DEFAULT_MIXED_PORT as u64)
                    as u16;
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
        .unwrap_or(crate::network::DEFAULT_MIXED_PORT);
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
    let tun_enabled = state
        .state
        .overrides
        .get(serde_yaml_ng::Value::from("tun"))
        .and_then(|tun| tun.get("enable"))
        .and_then(serde_yaml_ng::Value::as_bool)
        .unwrap_or(false);
    if tun_enabled && unsafe { libc::geteuid() } != 0 && !tun_capable(&owned) {
        for _ in 0..30 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if tun_capable(&owned) {
                break;
            }
        }
        if !tun_capable(&owned) {
            bail!("TUN 已启用，但权限服务尚未为新内核授权；请运行 --tun-service install");
        }
    }
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

fn privileged_program(name: &str) -> Option<&'static str> {
    match name {
        "setcap" if Path::new("/usr/sbin/setcap").is_file() => Some("/usr/sbin/setcap"),
        "setcap" if Path::new("/sbin/setcap").is_file() => Some("/sbin/setcap"),
        "systemctl" if Path::new("/usr/bin/systemctl").is_file() => Some("/usr/bin/systemctl"),
        "systemctl" if Path::new("/bin/systemctl").is_file() => Some("/bin/systemctl"),
        _ => None,
    }
}

fn root_write(path: &Path, content: &[u8], mode: u32) -> Result<()> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let parent = path.parent().context("系统文件路径无效")?;
    std::fs::create_dir_all(parent)?;
    let staging = parent.join(format!(".clash-verge-tui-{}.tmp", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(mode);
    let mut file = options.open(&staging)?;
    file.write_all(content)?;
    file.sync_all()?;
    #[cfg(unix)]
    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(mode))?;
    std::fs::rename(staging, path)?;
    Ok(())
}

fn root_systemctl(args: &[&str]) -> Result<()> {
    let program = privileged_program("systemctl").context("系统缺少 systemctl")?;
    let status = std::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .status()?;
    if !status.success() {
        bail!("systemd TUN 权限服务操作失败");
    }
    Ok(())
}

#[cfg(unix)]
fn validate_tun_core(dir: &Path, uid: u32) -> Result<PathBuf> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let dir = std::fs::canonicalize(dir)?;
    if std::fs::metadata(&dir)?.uid() != uid {
        bail!("TUN 工作区不属于请求用户");
    }
    let core = dir.join("core/mihomo");
    let metadata = std::fs::symlink_metadata(&core)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        bail!("TUN 内核必须为普通文件");
    }
    if metadata.uid() != uid || metadata.permissions().mode() & 0o022 != 0 {
        bail!("TUN 内核所有者或权限不安全");
    }
    if !crate::core_manager::matches_release_hash(&core) {
        bail!("TUN 内核不是当前应用固定的官方版本");
    }
    Ok(core)
}

#[cfg(not(unix))]
fn validate_tun_core(_: &Path, _: u32) -> Result<PathBuf> {
    bail!("TUN 权限服务只支持 Linux")
}

fn apply_tun_capability(dir: &Path, uid: u32) -> Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        bail!("TUN 权限助手必须以 root 运行");
    }
    let core = validate_tun_core(dir, uid)?;
    let setcap = privileged_program("setcap")
        .context("系统缺少 setcap；Debian/Ubuntu 请安装 libcap2-bin")?;
    let status = std::process::Command::new(setcap)
        .args(["cap_net_admin,cap_net_bind_service+ep"])
        .arg(&core)
        .stdin(Stdio::null())
        .status()?;
    if !status.success() || !tun_capable(&core) {
        bail!("无法为 mihomo 安装 TUN 所需能力");
    }
    Ok(())
}

pub fn tun_helper(action: &str, dir: &Path, uid: u32) -> Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        bail!("TUN 权限助手必须通过 sudo 运行");
    }
    let dir = std::fs::canonicalize(dir)?;
    let base = tun_base_name(&dir, uid);
    let service_path = Path::new(SYSTEMD_DIR).join(format!("{base}.service"));
    let watch_path = Path::new(SYSTEMD_DIR).join(format!("{base}.path"));
    match action {
        "apply" => apply_tun_capability(&dir, uid),
        "install" => {
            validate_tun_core(&dir, uid)?;
            let current = std::fs::canonicalize(std::env::current_exe()?)?;
            let helper = Path::new(TUN_HELPER);
            let binary = std::fs::read(current)?;
            root_write(helper, &binary, 0o755)?;
            let (service, path) = tun_units(helper, &dir, uid)?;
            root_write(&service_path, service.as_bytes(), 0o644)?;
            root_write(&watch_path, path.as_bytes(), 0o644)?;
            root_systemctl(&["daemon-reload"])?;
            apply_tun_capability(&dir, uid)?;
            root_systemctl(&["enable", "--now", &format!("{base}.path")])?;
            root_systemctl(&["start", &format!("{base}.service")])?;
            Ok(())
        }
        "uninstall" => {
            let _ = root_systemctl(&["disable", "--now", &format!("{base}.path")]);
            if let Ok(core) = validate_tun_core(&dir, uid) {
                if tun_capable(&core) {
                    if let Some(setcap) = privileged_program("setcap") {
                        let _ = std::process::Command::new(setcap)
                            .args(["-r"])
                            .arg(core)
                            .stdin(Stdio::null())
                            .status();
                    }
                }
            }
            for file in [service_path, watch_path] {
                if file.exists() {
                    if !std::fs::read_to_string(&file)?.starts_with("# Managed by clash-verge-tui")
                    {
                        bail!("拒绝删除非本工具管理的 TUN 服务文件");
                    }
                    std::fs::remove_file(file)?;
                }
            }
            root_systemctl(&["daemon-reload"])
        }
        _ => bail!("未知 TUN 权限助手操作"),
    }
}

async fn tun_helper_request(
    action: &str,
    dir: &Path,
    password: Option<&crate::core::SecretInput>,
) -> Result<()> {
    if action == "install" && !Path::new("/dev/net/tun").exists() {
        bail!("系统缺少 /dev/net/tun，请先启用 Linux TUN 设备");
    }
    if action == "install" && privileged_program("setcap").is_none() {
        bail!("系统缺少 setcap；Debian/Ubuntu 请安装 libcap2-bin");
    }
    let app = std::fs::canonicalize(std::env::current_exe()?)?;
    let dir = std::fs::canonicalize(dir)?;
    let uid = unsafe { libc::geteuid() };
    if uid == 0 {
        return tun_helper(action, &dir, 0);
    }
    let sudo = ["/usr/bin/sudo", "/bin/sudo"]
        .into_iter()
        .find(|path| Path::new(path).is_file())
        .context("系统缺少 sudo")?;
    let uid = uid.to_string();
    let mut command = tokio::process::Command::new(sudo);
    if password.is_some() {
        command.args(["-S", "-p", ""]);
    }
    command
        .arg("--")
        .arg(app)
        .args(["--tun-helper", action, "--tun-uid", &uid, "--data-dir"])
        .arg(&dir)
        .kill_on_drop(true);
    if password.is_some() {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .env("LC_ALL", "C");
    }
    let mut child = command.spawn().context("无法启动 sudo")?;
    if let Some(password) = password {
        use tokio::io::AsyncWriteExt;
        use zeroize::Zeroize;
        let mut input = password.expose().as_bytes().to_vec();
        input.push(b'\n');
        let mut stdin = child.stdin.take().context("无法连接 sudo 输入")?;
        let result = stdin.write_all(&input).await;
        input.zeroize();
        result?;
        stdin.shutdown().await?;
    }
    if password.is_some() {
        let output = tokio::time::timeout(Duration::from_secs(180), child.wait_with_output())
            .await
            .map_err(|_| anyhow!("系统密码授权超时"))??;
        if !output.status.success() {
            bail!(tun_helper_failure(&String::from_utf8_lossy(&output.stderr)));
        }
    } else {
        let status = tokio::time::timeout(Duration::from_secs(180), child.wait())
            .await
            .map_err(|_| anyhow!("sudo 授权超时"))??;
        if !status.success() {
            bail!("sudo 或 TUN 权限助手执行失败");
        }
    }
    Ok(())
}

fn tun_helper_failure(stderr: &str) -> String {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("sorry, try again")
        || lower.contains("incorrect password")
        || lower.contains("authentication failure")
    {
        return "系统密码验证失败，请重新输入".into();
    }
    if lower.contains("not in the sudoers")
        || lower.contains("not allowed to execute")
        || lower.contains("may not run sudo")
    {
        return "当前用户没有执行 TUN 权限助手所需的 sudo 授权".into();
    }
    let line = stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("sudo 或 TUN 权限助手执行失败");
    let detail = line.strip_prefix("Error: ").unwrap_or(line);
    let detail: String = detail
        .chars()
        .filter(|character| !character.is_control())
        .take(180)
        .collect();
    if detail.is_empty() {
        "sudo 或 TUN 权限助手执行失败".into()
    } else {
        detail
    }
}

pub async fn install_tun_service(dir: &Path, password: &crate::core::SecretInput) -> Result<()> {
    tun_helper_request("install", dir, Some(password)).await?;
    let core = std::fs::canonicalize(dir)?.join("core/mihomo");
    if !tun_capable(&core) {
        bail!("TUN 权限服务已返回，但内核能力校验失败");
    }
    Ok(())
}

pub async fn install_tun_service_cli(dir: &Path) -> Result<()> {
    tun_helper_request("install", dir, None).await?;
    let core = std::fs::canonicalize(dir)?.join("core/mihomo");
    if !tun_capable(&core) {
        bail!("TUN 权限服务已返回，但内核能力校验失败");
    }
    Ok(())
}

pub async fn uninstall_tun_service(dir: &Path) -> Result<()> {
    tun_helper_request("uninstall", dir, None).await
}

#[cfg(test)]
mod tun_tests {
    use super::{path_directive, tun_capability_output, tun_helper_failure};

    #[test]
    fn parses_libcap_getcap_output() {
        assert!(tun_capability_output(
            "/tmp/mihomo cap_net_bind_service,cap_net_admin=ep"
        ));
        assert!(tun_capability_output("/tmp/mihomo cap_net_admin+ep"));
        assert!(!tun_capability_output("/usr/bin/ping cap_net_raw=ep"));
    }

    #[test]
    fn systemd_paths_and_sudo_errors_are_actionable() {
        assert_eq!(
            path_directive("/home/user/work space/100%/'core'/\"mihomo\""),
            "/home/user/work\\x20space/100%%/\\x27core\\x27/\\x22mihomo\\x22"
        );
        assert_eq!(
            tun_helper_failure("sudo: 1 incorrect password attempt\n"),
            "系统密码验证失败，请重新输入"
        );
        assert_eq!(
            tun_helper_failure("Error: systemd TUN 权限服务操作失败\n"),
            "systemd TUN 权限服务操作失败"
        );
    }
}
