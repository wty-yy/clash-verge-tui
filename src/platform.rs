use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Setting {
    schema: String,
    key: String,
    value: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ProxyBackup {
    before: Vec<Setting>,
    applied: Vec<Setting>,
}
pub struct SystemProxy {
    dir: PathBuf,
    environment: BTreeMap<String, String>,
}
impl SystemProxy {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            environment: BTreeMap::new(),
        }
    }
    pub fn isolated(dir: PathBuf, config: PathBuf) -> Self {
        Self {
            dir,
            environment: BTreeMap::from([
                ("GSETTINGS_BACKEND".into(), "keyfile".into()),
                (
                    "XDG_CONFIG_HOME".into(),
                    config.to_string_lossy().into_owned(),
                ),
            ]),
        }
    }
    async fn run(&self, args: &[&str]) -> Result<String> {
        let mut command = tokio::process::Command::new("gsettings");
        command
            .args(args)
            .envs(&self.environment)
            .stdin(Stdio::null())
            .kill_on_drop(true);
        let output = tokio::time::timeout(std::time::Duration::from_secs(5), command.output())
            .await
            .map_err(|_| anyhow!("GNOME 设置请求超时"))?
            .map_err(|_| anyhow!("系统代理需要 GNOME gsettings"))?;
        if !output.status.success() {
            bail!("GNOME 系统代理设置失败");
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().into())
    }
    async fn read(&self, schema: &str, key: &str) -> Result<Setting> {
        Ok(Setting {
            schema: schema.into(),
            key: key.into(),
            value: self.run(&["get", schema, key]).await?,
        })
    }
    async fn write(&self, setting: &Setting) -> Result<()> {
        self.run(&["set", &setting.schema, &setting.key, &setting.value])
            .await?;
        Ok(())
    }
    fn backup_path(&self) -> PathBuf {
        self.dir.join("system-proxy-backup.json")
    }
    pub async fn status(&self, host: &str, port: u16) -> Result<String> {
        let mode = self.run(&["get", "org.gnome.system.proxy", "mode"]).await?;
        if mode.contains("none") {
            return Ok("关闭".into());
        }
        if mode.contains("auto") {
            let current = self
                .run(&["get", "org.gnome.system.proxy", "autoconfig-url"])
                .await?;
            let owned = std::fs::read(self.backup_path())
                .ok()
                .and_then(|v| serde_json::from_slice::<ProxyBackup>(&v).ok())
                .and_then(|b| b.applied.into_iter().find(|s| s.key == "autoconfig-url"))
                .is_some_and(|s| {
                    s.value.trim_matches(['\'', '"']) == current.trim_matches(['\'', '"'])
                });
            return Ok(if owned { "开启" } else { "外部代理" }.into());
        }

        let h = self
            .run(&["get", "org.gnome.system.proxy.http", "host"])
            .await?;
        let p = self
            .run(&["get", "org.gnome.system.proxy.http", "port"])
            .await?;
        if h.trim_matches(['\'', '"']) == host && p == port.to_string() {
            Ok("开启".into())
        } else {
            Ok("外部代理".into())
        }
    }
    pub async fn restore_if_owned(&self) -> Result<()> {
        let path = self.backup_path();
        if !path.exists() {
            return Ok(());
        }
        let backup: ProxyBackup = serde_json::from_slice(&std::fs::read(&path)?)?;
        for item in backup.applied.iter().filter(|s| {
            matches!(s.key.as_str(), "mode" | "autoconfig-url")
                || s.schema.ends_with(".http") && matches!(s.key.as_str(), "host" | "port")
        }) {
            let current = self.read(&item.schema, &item.key).await?;
            if current.value.trim_matches(['\'', '"']) != item.value.trim_matches(['\'', '"']) {
                std::fs::remove_file(path)?;
                return Ok(());
            }
        }
        self.restore().await
    }
    pub async fn restore(&self) -> Result<()> {
        let path = self.backup_path();
        if !path.exists() {
            return Ok(());
        }
        let backup: ProxyBackup = serde_json::from_slice(&std::fs::read(&path)?)
            .map_err(|_| anyhow!("系统代理恢复记录损坏"))?;
        for item in &backup.before {
            self.write(item).await?;
        }
        std::fs::remove_file(path)?;
        Ok(())
    }
    pub async fn enable(
        &self,
        host: &str,
        port: u16,
        bypass: &str,
        pac: Option<&str>,
    ) -> Result<()> {
        if host.parse::<std::net::IpAddr>().is_err() && url::Host::parse(host).is_err() {
            bail!("代理主机无效");
        }
        if port == 0 {
            bail!("代理端口不能为 0");
        }
        let mut applied = Vec::new();
        let quote = |s: &str| serde_json::to_string(s).unwrap();
        for schema in [
            "org.gnome.system.proxy.http",
            "org.gnome.system.proxy.https",
            "org.gnome.system.proxy.socks",
        ] {
            applied.push(Setting {
                schema: schema.into(),
                key: "host".into(),
                value: quote(host),
            });
            applied.push(Setting {
                schema: schema.into(),
                key: "port".into(),
                value: port.to_string(),
            });
        }
        applied.push(Setting {
            schema: "org.gnome.system.proxy.http".into(),
            key: "use-authentication".into(),
            value: "false".into(),
        });
        let bypass: Vec<_> = bypass
            .split([';', ',', '\n'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        applied.push(Setting {
            schema: "org.gnome.system.proxy".into(),
            key: "ignore-hosts".into(),
            value: serde_json::to_string(&bypass)?,
        });
        applied.push(Setting {
            schema: "org.gnome.system.proxy".into(),
            key: "autoconfig-url".into(),
            value: quote(pac.unwrap_or("")),
        });
        applied.push(Setting {
            schema: "org.gnome.system.proxy".into(),
            key: "mode".into(),
            value: quote(if pac.is_some() { "auto" } else { "manual" }),
        });
        let mut immediate = Vec::new();
        for setting in &applied {
            immediate.push(self.read(&setting.schema, &setting.key).await?);
        }
        let path = self.backup_path();
        let before = if path.exists() {
            let old: ProxyBackup = serde_json::from_slice(&std::fs::read(&path)?)?;
            old.before
        } else {
            immediate.clone()
        };
        let backup = ProxyBackup {
            before,
            applied: applied.clone(),
        };
        crate::subscriptions::private_write(&path, &serde_json::to_vec_pretty(&backup)?)?;
        for setting in &applied {
            if let Err(error) = self.write(setting).await {
                for previous in &immediate {
                    let _ = self.write(previous).await;
                }
                return Err(error);
            }
        }
        Ok(())
    }
    pub async fn guard(&self) -> Result<()> {
        let path = self.backup_path();
        if !path.exists() {
            return Ok(());
        }
        let backup: ProxyBackup = serde_json::from_slice(&std::fs::read(path)?)?;
        for item in backup.applied {
            let current = self.read(&item.schema, &item.key).await?;
            if current.value != item.value {
                self.write(&item).await?;
            }
        }
        Ok(())
    }
}
pub fn pac_script(host: &str, port: u16, bypass: &str) -> String {
    let patterns: Vec<_> = bypass
        .split([';', ',', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let patterns = serde_json::to_string(&patterns).unwrap();
    let target = serde_json::to_string(&format!("PROXY {host}:{port}; DIRECT")).unwrap();
    format!("function FindProxyForURL(url, host) {{ var bypass={patterns}; if(isPlainHostName(host)) return 'DIRECT'; for(var i=0;i<bypass.length;i++){{if(shExpMatch(host,bypass[i])) return 'DIRECT';}} return {target}; }}")
}
pub async fn apply_proxy(dir: &Path, values: &BTreeMap<String, String>, port: u16) -> Result<()> {
    let proxy = SystemProxy::new(dir.into());
    if values.get("system_proxy").map(String::as_str) != Some("开启") {
        return proxy.restore_if_owned().await;
    }
    let host = values
        .get("proxy_host")
        .map(String::as_str)
        .unwrap_or("127.0.0.1");
    let bypass = values
        .get("bypass")
        .map(String::as_str)
        .unwrap_or("localhost;127.*;192.168.*;10.*");
    let pac = if values.get("pac").map(String::as_str) == Some("开启") {
        let script = values
            .get("pac_script")
            .filter(|s| !s.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| pac_script(host, port, bypass));
        let path = dir.join("proxy.pac");
        crate::subscriptions::private_write(&path, script.as_bytes())?;
        Some(
            url::Url::from_file_path(std::fs::canonicalize(path)?)
                .map_err(|_| anyhow!("PAC 文件路径无效"))?
                .to_string(),
        )
    } else {
        None
    };
    proxy.enable(host, port, bypass, pac.as_deref()).await
}
