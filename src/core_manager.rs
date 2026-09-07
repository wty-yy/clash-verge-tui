use anyhow::{anyhow, bail, Context, Result};
use flate2::read::GzDecoder;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

pub const MIHOMO_VERSION: &str = "1.19.29";
const RELEASE_ROOT: &str = "https://github.com/MetaCubeX/mihomo/releases/download/v1.19.29";
const MAX_ARCHIVE_SIZE: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Asset {
    pub arch: &'static str,
    pub name: &'static str,
    pub sha256: &'static str,
    pub binary_sha256: &'static str,
}

pub fn asset() -> Result<Asset> {
    match std::env::consts::ARCH {
        "x86_64" => Ok(Asset {
            arch: "x86_64",
            name: "mihomo-linux-amd64-v1.19.29.gz",
            sha256: "60de76a35a6cbf7b4fa4a20f5c257c24345d1d635ab1aa3877022a1997ef413c",
            binary_sha256: "9c397be7489538628fae781bc005e4c5b8cd7b0961b8bb2ca815c8150f193577",
        }),
        "aarch64" => Ok(Asset {
            arch: "aarch64",
            name: "mihomo-linux-arm64-v1.19.29.gz",
            sha256: "9a868b5e4e0ad91d9d71e1b41b0cfce78aaba44360c30df74a723f8e3926a86c",
            binary_sha256: "8e02308f672e89c076bfc2fa1b03379bd54e58b0bafa81ffb01113fcf6da348d",
        }),
        arch => bail!("暂不支持 {arch}；Linux 发行包支持 x86_64 与 aarch64"),
    }
}

pub fn matches_release_hash(binary: &Path) -> bool {
    if !binary.is_file()
        || fs::symlink_metadata(binary)
            .ok()
            .is_some_and(|metadata| metadata.file_type().is_symlink())
    {
        return false;
    }
    let exact_release = asset()
        .ok()
        .and_then(|asset| fs::read(binary).ok().map(|bytes| (asset, bytes)))
        .is_some_and(|(asset, bytes)| {
            format!("{:x}", Sha256::digest(bytes)) == asset.binary_sha256
        });
    exact_release
}

fn version_matches(binary: &Path) -> bool {
    matches_release_hash(binary)
        && Command::new(binary)
            .arg("-v")
            .stdin(Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| {
                let text = String::from_utf8_lossy(&output.stdout);
                text.split_whitespace()
                    .any(|word| word == format!("v{MIHOMO_VERSION}"))
            })
            .unwrap_or(false)
}

pub fn verify(binary: &Path) -> Result<PathBuf> {
    let binary = fs::canonicalize(binary)
        .with_context(|| format!("无法读取 mihomo 内核：{}", binary.display()))?;
    if !version_matches(&binary) {
        bail!("mihomo 内核必须为 v{MIHOMO_VERSION}：{}", binary.display());
    }
    Ok(binary)
}

fn install_root() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local/share")
        })
        .join("clash-verge-tui/core")
}

fn bundled_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = std::env::var_os("CLASH_VERGE_TUI_CORE_PATH") {
        paths.push(PathBuf::from(path));
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(bin) = executable.parent() {
            paths.push(bin.join(format!(
                "../lib/clash-verge-tui/core/v{MIHOMO_VERSION}/mihomo"
            )));
            paths.push(bin.join("../lib/clash-verge-tui/mihomo"));
            paths.push(bin.join(format!(
                "../libexec/clash-verge-tui/core/v{MIHOMO_VERSION}/mihomo"
            )));
            paths.push(bin.join("../libexec/clash-verge-tui/mihomo"));
        }
    }
    paths.push(install_root().join(format!("v{MIHOMO_VERSION}/mihomo")));
    paths
}

pub async fn ensure(workspace: &Path, override_path: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = override_path {
        return verify(path);
    }
    let owned = workspace.join("core/mihomo");
    if version_matches(&owned) {
        return Ok(fs::canonicalize(owned)?);
    }
    for candidate in bundled_candidates() {
        if version_matches(&candidate) {
            return Ok(fs::canonicalize(candidate)?);
        }
    }
    let destination = install_root().join(format!("v{MIHOMO_VERSION}/mihomo"));
    if version_matches(&destination) {
        return Ok(fs::canonicalize(destination)?);
    }
    download(&destination).await?;
    verify(&destination)
}

async fn download(destination: &Path) -> Result<()> {
    let asset = asset()?;
    let url = format!("{RELEASE_ROOT}/{}", asset.name);
    eprintln!("正在安装 mihomo v{MIHOMO_VERSION} ({})…", asset.arch);
    let response = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?
        .get(url)
        .header(
            "User-Agent",
            concat!("clash-verge-tui/", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .await
        .map_err(|error| anyhow!("mihomo 下载失败：{}", error.without_url()))?;
    if !response.status().is_success() {
        bail!("mihomo 下载失败：HTTP {}", response.status().as_u16());
    }
    let mut archive = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| anyhow!("mihomo 下载中断"))?;
        if archive.len() + chunk.len() > MAX_ARCHIVE_SIZE {
            bail!("mihomo 压缩包超过 32 MiB 限制");
        }
        archive.extend_from_slice(&chunk);
    }
    install_archive(&archive, asset.sha256, destination)
}

pub fn install_archive(archive: &[u8], expected_sha256: &str, destination: &Path) -> Result<()> {
    let actual = format!("{:x}", Sha256::digest(archive));
    if actual != expected_sha256 {
        bail!("mihomo 压缩包校验失败");
    }
    let mut decoder = GzDecoder::new(archive);
    let mut binary = Vec::new();
    decoder
        .read_to_end(&mut binary)
        .context("mihomo 压缩包无法解压")?;
    if binary.len() > 96 * 1024 * 1024 || binary.len() < 1024 {
        bail!("mihomo 解压后大小异常");
    }
    let parent = destination.parent().context("mihomo 安装目录无效")?;
    fs::create_dir_all(parent)?;
    let staging = parent.join(format!(".mihomo-{}.tmp", std::process::id()));
    crate::subscriptions::private_write(&staging, &binary)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staging, fs::Permissions::from_mode(0o700))?;
    }
    fs::rename(staging, destination)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};
    use std::io::Write;

    #[test]
    fn architecture_asset_is_pinned() {
        let asset = asset().unwrap();
        assert!(asset.name.contains(MIHOMO_VERSION));
        assert_eq!(asset.sha256.len(), 64);
        assert_eq!(asset.binary_sha256.len(), 64);
    }

    #[test]
    fn archive_install_checks_hash_and_permissions() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&vec![b'x'; 2048]).unwrap();
        let archive = encoder.finish().unwrap();
        let checksum = format!("{:x}", Sha256::digest(&archive));
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("mihomo");
        install_archive(&archive, &checksum, &output).unwrap();
        assert_eq!(fs::read(&output).unwrap(), vec![b'x'; 2048]);
        assert!(install_archive(&archive, "bad", &output).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(output).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
    }
}
