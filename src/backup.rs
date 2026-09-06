use crate::{
    core::CoreClient,
    subscriptions::private_write,
    workspace::{self, WorkspaceCommand, WorkspaceContext, WorkspaceSnapshot},
};
use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
use anyhow::{anyhow, bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path, time::Duration};
const LIMIT: usize = 32 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackupMeta {
    pub file: String,
    pub created: u64,
    pub profiles: usize,
    pub encrypted: bool,
    #[serde(default = "valid_default")]
    pub valid: bool,
}
fn valid_default() -> bool {
    true
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    schema: u32,
    meta: BackupMeta,
    salt: String,
    nonce: String,
    data: String,
}
#[derive(Serialize, Deserialize)]
struct Archive {
    snapshot: WorkspaceSnapshot,
    files: BTreeMap<String, String>,
}
#[derive(Clone, Debug)]
pub enum BackupCommand {
    List { remote: bool },
    Create,
    Delete { file: String, remote: bool },
    Restore { file: String, remote: bool },
    Upload(String),
}
#[derive(Clone, Debug)]
pub struct BackupResult {
    pub items: Vec<BackupMeta>,
    pub remote: bool,
    pub restored: Option<WorkspaceSnapshot>,
    pub message: String,
}
fn key(password: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let mut output = [0; 32];
    scrypt::scrypt(
        password.as_bytes(),
        salt,
        &scrypt::Params::new(14, 8, 1, 32).map_err(|_| anyhow!("备份加密参数无效"))?,
        &mut output,
    )
    .map_err(|_| anyhow!("无法生成备份密钥"))?;
    Ok(output)
}
fn filename(file: &str) -> Result<&str> {
    if !file.starts_with("backup-")
        || !file.ends_with(".cvt")
        || !file
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.'))
    {
        bail!("备份文件名无效");
    }
    Ok(file)
}
pub fn create(dir: &Path) -> Result<BackupMeta> {
    let mut snapshot = workspace::load(dir)?;
    let password = snapshot
        .state
        .preferences
        .remove("backup_password")
        .unwrap_or_default();
    let mut files = BTreeMap::new();
    let mut total = 0;
    for p in &snapshot.profiles {
        let name = p
            .file
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow!("配置文件名无效"))?;
        let content = fs::read_to_string(&p.file)?;
        total += content.len();
        if total > LIMIT {
            bail!("备份内容超过 32 MiB");
        }
        files.insert(name.into(), content);
    }
    let bytes = serde_json::to_vec(&Archive {
        snapshot: snapshot.clone(),
        files,
    })?;
    let mut salt = [0; 16];
    let mut nonce = [0; 12];
    getrandom::getrandom(&mut salt).map_err(|_| anyhow!("随机数生成失败"))?;
    getrandom::getrandom(&mut nonce).map_err(|_| anyhow!("随机数生成失败"))?;
    let suffix = salt[..4]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let created = workspace::timestamp();
    let meta = BackupMeta {
        file: format!("backup-{created}-{suffix}.cvt"),
        created,
        profiles: snapshot.profiles.len(),
        encrypted: !password.is_empty(),
        valid: true,
    };
    let data = if password.is_empty() {
        bytes
    } else {
        Aes256Gcm::new_from_slice(&key(&password, &salt)?)
            .unwrap()
            .encrypt((&nonce).into(), bytes.as_ref())
            .map_err(|_| anyhow!("备份加密失败"))?
    };
    let envelope = Envelope {
        schema: 1,
        meta: meta.clone(),
        salt: STANDARD.encode(salt),
        nonce: STANDARD.encode(nonce),
        data: STANDARD.encode(data),
    };
    private_write(
        &dir.join("backups").join(&meta.file),
        &serde_json::to_vec(&envelope)?,
    )?;
    for old in local_list(dir)?
        .into_iter()
        .filter(|item| item.file != meta.file)
        .skip(9)
    {
        let _ = fs::remove_file(dir.join("backups").join(old.file));
    }
    Ok(meta)
}
pub fn local_list(dir: &Path) -> Result<Vec<BackupMeta>> {
    let path = dir.join("backups");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if filename(name).is_err() || !entry.file_type()?.is_file() {
            continue;
        }
        let envelope = if entry.metadata()?.len() <= LIMIT as u64 * 2 {
            fs::read(entry.path())
                .ok()
                .and_then(|b| serde_json::from_slice::<Envelope>(&b).ok())
                .filter(|e| e.schema == 1)
        } else {
            None
        };
        let Some(envelope) = envelope else {
            items.push(BackupMeta {
                file: name.into(),
                created: 0,
                profiles: 0,
                encrypted: false,
                valid: false,
            });
            continue;
        };
        let mut meta = envelope.meta;
        meta.file = name.into();
        items.push(meta);
    }
    items.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.file.cmp(&a.file)));
    Ok(items)
}
fn decode(bytes: &[u8], password: &str) -> Result<Archive> {
    if bytes.len() > LIMIT * 2 {
        bail!("备份文件过大");
    }
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_| anyhow!("备份格式无效"))?;
    if envelope.schema != 1 {
        bail!("不支持的备份版本");
    }
    let data = STANDARD
        .decode(envelope.data)
        .map_err(|_| anyhow!("备份编码无效"))?;
    let data = if envelope.meta.encrypted {
        let salt = STANDARD
            .decode(envelope.salt)
            .map_err(|_| anyhow!("备份盐值无效"))?;
        let nonce = STANDARD
            .decode(envelope.nonce)
            .map_err(|_| anyhow!("备份随机值无效"))?;
        if nonce.len() != 12 || salt.len() != 16 {
            bail!("备份加密参数无效");
        }
        Aes256Gcm::new_from_slice(&key(password, &salt)?)
            .unwrap()
            .decrypt(nonce.as_slice().into(), data.as_ref())
            .map_err(|_| anyhow!("备份密码不正确或文件已损坏"))?
    } else {
        data
    };
    serde_json::from_slice(&data).map_err(|_| anyhow!("备份内容无效"))
}
struct Dav {
    client: reqwest::Client,
    url: url::Url,
    user: String,
    password: String,
}
impl Dav {
    fn new(values: &BTreeMap<String, String>) -> Result<Self> {
        let url = url::Url::parse(values.get("webdav_url").map(String::as_str).unwrap_or(""))
            .map_err(|_| anyhow!("请先设置 WebDAV 地址"))?;
        if !matches!(url.scheme(), "https" | "http") {
            bail!("WebDAV 仅支持 HTTP(S)");
        }
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            url,
            user: values.get("webdav_user").cloned().unwrap_or_default(),
            password: values.get("webdav_password").cloned().unwrap_or_default(),
        })
    }
    async fn request(
        &self,
        method: &str,
        file: Option<&str>,
        body: Option<Vec<u8>>,
    ) -> Result<Vec<u8>> {
        let mut url = self.url.clone();
        if let Some(file) = file {
            url.path_segments_mut()
                .map_err(|_| anyhow!("WebDAV 路径无效"))?
                .pop_if_empty()
                .push(filename(file)?);
        }
        let mut req = self
            .client
            .request(reqwest::Method::from_bytes(method.as_bytes())?, url)
            .basic_auth(&self.user, Some(&self.password));
        if method == "PROPFIND" {
            req=req.header("Depth","1").header("Content-Type","application/xml").body("<propfind xmlns=\"DAV:\"><prop><getcontentlength/><getlastmodified/></prop></propfind>");
        } else if let Some(body) = body {
            req = req.body(body);
        }
        let response = req
            .send()
            .await
            .map_err(|e| anyhow!("WebDAV 请求失败：{}", e.without_url()))?;
        if !response.status().is_success() {
            bail!("WebDAV HTTP {}", response.status().as_u16());
        }
        let mut stream = response.bytes_stream();
        let mut data = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| anyhow!("WebDAV 传输中断"))?;
            if data.len() + chunk.len() > LIMIT * 2 {
                bail!("WebDAV 响应过大");
            }
            data.extend_from_slice(&chunk);
        }
        Ok(data)
    }
    async fn list(&self) -> Result<Vec<BackupMeta>> {
        let bytes = self.request("PROPFIND", None, None).await?;
        let text = std::str::from_utf8(&bytes).map_err(|_| anyhow!("WebDAV 列表编码无效"))?;
        let doc = roxmltree::Document::parse(text).map_err(|_| anyhow!("WebDAV 列表 XML 无效"))?;
        let mut items = Vec::new();
        for node in doc
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "href")
        {
            if let Some(file) = node
                .text()
                .and_then(|v| v.rsplit('/').next())
                .filter(|s| filename(s).is_ok())
            {
                items.push(BackupMeta {
                    file: file.into(),
                    created: file
                        .split('-')
                        .nth(1)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0),
                    profiles: 0,
                    encrypted: false,
                    valid: true,
                });
            }
        }
        items.sort_by(|a, b| b.created.cmp(&a.created));
        items.dedup_by(|a, b| a.file == b.file);
        Ok(items)
    }
}
pub async fn execute(
    context: &WorkspaceContext,
    client: &CoreClient,
    command: BackupCommand,
) -> Result<BackupResult> {
    let state = workspace::load(&context.dir)?;
    let mut restored = None;
    let mut remote = false;
    match command {
        BackupCommand::Create => {
            let _lock = workspace::Lock::acquire(&context.dir)?;
            create(&context.dir)?;
        }
        BackupCommand::List { remote: r } => remote = r,
        BackupCommand::Delete { file, remote: r } => {
            remote = r;
            if r {
                Dav::new(&state.state.preferences)?
                    .request("DELETE", Some(&file), None)
                    .await?;
            } else {
                fs::remove_file(context.dir.join("backups").join(filename(&file)?))?;
            }
        }
        BackupCommand::Upload(file) => {
            let bytes = fs::read(context.dir.join("backups").join(filename(&file)?))?;
            Dav::new(&state.state.preferences)?
                .request("PUT", Some(&file), Some(bytes))
                .await?;
        }
        BackupCommand::Restore { file, remote: r } => {
            remote = r;
            let bytes = if r {
                Dav::new(&state.state.preferences)?
                    .request("GET", Some(&file), None)
                    .await?
            } else {
                fs::read(context.dir.join("backups").join(filename(&file)?))?
            };
            let password = state
                .state
                .preferences
                .get("backup_password")
                .map(String::as_str)
                .unwrap_or("");
            let mut archive = decode(&bytes, password)?;
            archive
                .snapshot
                .state
                .preferences
                .insert("backup_password".into(), password.into());
            restored = Some(
                workspace::execute(
                    context,
                    client,
                    WorkspaceCommand::Restore {
                        snapshot: Box::new(archive.snapshot),
                        files: archive.files,
                    },
                )
                .await?,
            );
        }
    }
    let items = if remote {
        Dav::new(&state.state.preferences)?.list().await?
    } else {
        local_list(&context.dir)?
    };
    Ok(BackupResult {
        items,
        remote,
        restored,
        message: "备份操作已完成".into(),
    })
}
