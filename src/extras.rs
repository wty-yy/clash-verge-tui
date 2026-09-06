use crate::{model::Unlock, workspace::WorkspaceContext};
use anyhow::{anyhow, bail, Result};
use futures_util::{stream, StreamExt};
use std::{fs, path::Path, time::Duration};
#[derive(Clone, Debug)]
pub enum ExtraCommand {
    Detect { proxy: String, indices: Vec<usize> },
    CheckUpdates,
    Open(String),
}
#[derive(Clone, Debug)]
pub enum ExtraResult {
    Detected(Vec<(usize, Unlock)>),
    Update(String),
    Opened,
}
pub const TARGETS: [(&str, &str, &str); 8] = [
    ("Netflix", "流媒体", "https://www.netflix.com/"),
    (
        "YouTube Premium",
        "流媒体",
        "https://www.youtube.com/premium",
    ),
    ("Disney+", "流媒体", "https://www.disneyplus.com/"),
    ("Spotify", "流媒体", "https://open.spotify.com/"),
    ("TikTok", "社交", "https://www.tiktok.com/"),
    ("ChatGPT", "AI", "https://chatgpt.com/"),
    ("Claude", "AI", "https://claude.ai/"),
    ("Gemini", "AI", "https://gemini.google.com/"),
];
pub fn initial() -> Vec<Unlock> {
    TARGETS
        .iter()
        .map(|(name, category, _)| Unlock {
            name: (*name).into(),
            category: (*category).into(),
            result: "未检测".into(),
            region: "—".into(),
        })
        .collect()
}
pub fn classify(status: u16, body: &str, login_redirect: bool) -> String {
    let lower = body.to_lowercase();
    if lower.contains("cf-chl-")
        || lower.contains("challenge-platform")
        || lower.contains("captcha")
    {
        return "未知：验证页面".into();
    }
    if login_redirect {
        return "需要登录".into();
    }
    if status == 451
        || lower.contains("unsupported_country")
        || lower.contains("not available in your country")
        || lower.contains("not available in your region")
    {
        return "地区受限".into();
    }
    match status {
        200..=299 => "可用（网页）".into(),
        401 => "需要登录".into(),
        403 => "未知：HTTP 403".into(),
        _ => format!("未知：HTTP {status}"),
    }
}
pub async fn execute(command: ExtraCommand) -> Result<ExtraResult> {
    match command {
        ExtraCommand::Detect { proxy, indices } => {
            let client = reqwest::Client::builder()
                .no_proxy()
                .proxy(reqwest::Proxy::all(&proxy).map_err(|_| anyhow!("代理地址无效"))?)
                .timeout(Duration::from_secs(12))
                .redirect(reqwest::redirect::Policy::limited(5))
                .user_agent("Mozilla/5.0 clash-verge-tui")
                .build()?;
            let region = match client
                .get("https://www.cloudflare.com/cdn-cgi/trace")
                .send()
                .await
            {
                Ok(r) => r
                    .text()
                    .await
                    .ok()
                    .and_then(|s| {
                        s.lines()
                            .find_map(|l| l.strip_prefix("loc=").map(str::to_string))
                    })
                    .unwrap_or("—".into()),
                Err(_) => "—".into(),
            };
            let result =
                stream::iter(indices.into_iter().filter(|i| *i < TARGETS.len()).map(|i| {
                    let client = client.clone();
                    let region = region.clone();
                    async move {
                        let (name, category, url) = TARGETS[i];
                        let result = match client.get(url).send().await {
                            Ok(response) => {
                                let status = response.status().as_u16();
                                let login = response.url().host_str().is_some_and(|h| {
                                    h == "accounts.google.com" || h.starts_with("login.")
                                });
                                let mut stream = response.bytes_stream();
                                let mut body = Vec::new();
                                while let Some(Ok(chunk)) = stream.next().await {
                                    let available = (256 * 1024usize).saturating_sub(body.len());
                                    body.extend_from_slice(&chunk[..chunk.len().min(available)]);
                                    if body.len() >= 256 * 1024 {
                                        break;
                                    }
                                }
                                classify(status, &String::from_utf8_lossy(&body), login)
                            }
                            Err(_) => "网络错误 / 超时".into(),
                        };
                        (
                            i,
                            Unlock {
                                name: name.into(),
                                category: category.into(),
                                result,
                                region,
                            },
                        )
                    }
                }))
                .buffer_unordered(4)
                .collect()
                .await;
            Ok(ExtraResult::Detected(result))
        }
        ExtraCommand::CheckUpdates => {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .user_agent("clash-verge-tui")
                .build()?;
            let response = client
                .get("https://api.github.com/repos/wty-yy/clash-verge-tui/tags?per_page=100")
                .send()
                .await
                .map_err(|e| anyhow!("检查更新失败：{}", e.without_url()))?;
            if !response.status().is_success() {
                bail!("更新服务器 HTTP {}", response.status().as_u16());
            }
            let value: serde_json::Value = response.json().await?;
            let latest = value
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|tag| tag["name"].as_str())
                .filter_map(|s| semver::Version::parse(s.trim_start_matches('v')).ok())
                .max();
            let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;
            let message=match latest{Some(version) if version>current=>format!("发现新版本 v{version}。\n\n当前采用源码安装，可执行：\ncargo install --locked --git https://github.com/wty-yy/clash-verge-tui --tag v{version}\n\n项目：https://github.com/wty-yy/clash-verge-tui"),Some(version)=>format!("当前版本 v{current}\n已发布最新版本 v{version}\n无需更新。"),None=>"未发现已发布版本".into()};
            Ok(ExtraResult::Update(message))
        }
        ExtraCommand::Open(target) => {
            if let Ok(url) = url::Url::parse(&target) {
                if !matches!(url.scheme(), "http" | "https" | "file") {
                    bail!("不支持的打开方式");
                }
            } else if !Path::new(&target).exists() {
                bail!("路径不存在");
            }
            tokio::process::Command::new("xdg-open")
                .arg(target)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|_| anyhow!("打开操作需要 xdg-open"))?;
            Ok(ExtraResult::Opened)
        }
    }
}
pub fn rotate_log(context: &WorkspaceContext) -> Result<()> {
    let state = crate::workspace::load(&context.dir)?;
    if state
        .state
        .preferences
        .get("log_clean")
        .is_some_and(|s| s == "关闭")
    {
        return Ok(());
    }
    let size = state
        .state
        .preferences
        .get("log_size")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(10)
        .clamp(1, 1024)
        * 1024
        * 1024;
    let count = state
        .state
        .preferences
        .get("log_count")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(7)
        .clamp(1, 100);
    let log = context.dir.join("core/core.log");
    if fs::metadata(&log).map(|m| m.len() <= size).unwrap_or(true) {
        return Ok(());
    }
    for i in (1..count).rev() {
        let old = context.dir.join(format!("core/core.{i}.log"));
        if old.exists() {
            fs::rename(old, context.dir.join(format!("core/core.{}.log", i + 1)))?;
        }
    }
    let rotated = context.dir.join("core/core.1.log");
    fs::copy(&log, &rotated)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(rotated, fs::Permissions::from_mode(0o600))?;
    }
    fs::OpenOptions::new().write(true).open(log)?.set_len(0)?;
    Ok(())
}
