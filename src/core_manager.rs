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
const MAX_ARCHIVE_SIZE: usize = 32 * 1024 * 1024;
pub const GEOSITE_FILE: &str = "GeoSite.dat";
pub const GEOSITE_SHA256: &str = "c5fe9448d979391192f5bd553b5e28c39efdc9bd857b7c879a7d995fded0c3fe";
/// GeoData mode uses the meta database; mihomo would otherwise fetch it from
/// GitHub during the blocking initial configuration parse.
pub const GEODATA_FILE: &str = "geoip.metadb";
pub const GEODATA_SHA256: &str = "4eda34a0851c96259fdc2330aeb2173beec58f2534b80da6e3a89e488a18c672";
pub const UI_INDEX: &str = "index.html";

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

/// Seed release-bundled GeoData and Web UI before the core parses a config.
/// Without them mihomo downloads from GitHub through direct connections during
/// the blocking initial configuration parse.
pub fn prepare_assets(dir: &Path, binary: &Path) -> Result<()> {
    let roots = bundled_roots(binary, dir);
    let named = |name: &str| -> Vec<PathBuf> { roots.iter().map(|root| root.join(name)).collect() };
    seed_file(
        dir,
        GEOSITE_FILE,
        GEOSITE_SHA256,
        GEOSITE_FILE,
        &named(GEOSITE_FILE),
    )?;
    seed_file(
        dir,
        GEODATA_FILE,
        GEODATA_SHA256,
        GEODATA_FILE,
        &named(GEODATA_FILE),
    )?;
    seed_ui(dir, &named("ui"))
}

fn bundled_roots(binary: &Path, dir: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(parent) = binary.parent().filter(|parent| *parent != dir) {
        roots.push(parent.to_path_buf());
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(bin) = executable.parent() {
            roots.push(bin.join("../lib/clash-verge-tui"));
            roots.push(bin.join("../libexec/clash-verge-tui"));
        }
    }
    roots.push(install_root());
    roots.retain(|root| root != dir);
    roots
}

/// Copy a release file only when the workspace has no local copy, so a user
/// update or a mirror refresh is never overwritten by an older bundle.
fn seed_file(
    dir: &Path,
    name: &str,
    sha256: &str,
    label: &str,
    candidates: &[PathBuf],
) -> Result<()> {
    fs::create_dir_all(dir)?;
    // Mihomo accepts case-insensitive names; preserve locally updated data too.
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(name)
        {
            if !entry.file_type()?.is_file() {
                bail!("{label} 必须为普通文件");
            }
            if entry.metadata()?.len() > 0 {
                return Ok(());
            }
        }
    }
    for source in candidates {
        let Ok(metadata) = fs::symlink_metadata(source) else {
            continue;
        };
        if !metadata.is_file() {
            bail!("随包 {label} 必须为普通文件");
        }
        let bytes = fs::read(source)?;
        if format!("{:x}", Sha256::digest(&bytes)) != sha256 {
            bail!("随包 {label} SHA-256 校验失败；请重新安装发行包");
        }
        crate::subscriptions::private_write(&dir.join(name), &bytes)?;
        return Ok(());
    }
    // Source-only installations may not have a release data bundle.
    Ok(())
}

fn seed_ui(dir: &Path, candidates: &[PathBuf]) -> Result<()> {
    let target = dir.join("ui");
    if target.exists() {
        if !target.is_dir() {
            bail!("随包网页界面必须为目录");
        }
        if fs::read_dir(&target)?.next().is_some() {
            return Ok(());
        }
    }
    for source in candidates {
        let Ok(metadata) = fs::symlink_metadata(source) else {
            continue;
        };
        if !metadata.is_dir() {
            bail!("随包网页界面必须为目录");
        }
        if !source.join(UI_INDEX).is_file() {
            bail!("随包网页界面缺少 index.html");
        }
        copy_ui(source, &target)?;
        return Ok(());
    }
    // Source-only installations may not have a release UI bundle.
    Ok(())
}

fn copy_ui(source: &Path, target: &Path) -> Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let destination = target.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            fs::create_dir_all(&destination)?;
            copy_ui(&entry.path(), &destination)?;
        } else if kind.is_file() {
            crate::subscriptions::private_write(&destination, &fs::read(entry.path())?)?;
        }
    }
    Ok(())
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
    eprintln!("Installing mihomo v{MIHOMO_VERSION} ({})…", asset.arch);
    let mut last = None;
    // The mirror keeps working when GitHub is unreachable; both sources are
    // pinned by the same archive and binary SHA-256.
    for url in [
        crate::sources::core_url(MIHOMO_VERSION, asset.name),
        crate::sources::core_github_url(MIHOMO_VERSION, asset.name),
    ] {
        match download_from(&url, asset, destination).await {
            Ok(()) => return Ok(()),
            Err(error) => last = Some(error),
        }
    }
    Err(last.unwrap_or_else(|| anyhow!("mihomo 下载失败")))
}

async fn download_from(url: &str, asset: Asset, destination: &Path) -> Result<()> {
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
    fn seeded_data_preserves_existing_files_and_rejects_corrupt_bundles() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("bundle.dat");
        fs::write(&bundle, b"truncated download").unwrap();
        let workspace = dir.path().join("workspace");
        assert!(seed_file(
            &workspace,
            GEOSITE_FILE,
            GEOSITE_SHA256,
            GEOSITE_FILE,
            std::slice::from_ref(&bundle)
        )
        .is_err());
        assert!(!workspace.join(GEOSITE_FILE).exists());
        fs::write(workspace.join("geosite.dat"), b"locally updated data").unwrap();
        seed_file(
            &workspace,
            GEOSITE_FILE,
            GEOSITE_SHA256,
            GEOSITE_FILE,
            std::slice::from_ref(&bundle),
        )
        .unwrap();
        assert_eq!(
            fs::read(workspace.join("geosite.dat")).unwrap(),
            b"locally updated data"
        );
        assert!(!workspace.join(GEOSITE_FILE).exists());

        let geodata = b"metadata fixture".to_vec();
        let geodata_bundle = dir.path().join("geoip.metadb");
        fs::write(&geodata_bundle, &geodata).unwrap();
        let checksum = format!("{:x}", Sha256::digest(&geodata));
        seed_file(
            &workspace,
            GEODATA_FILE,
            &checksum,
            GEODATA_FILE,
            std::slice::from_ref(&geodata_bundle),
        )
        .unwrap();
        assert_eq!(fs::read(workspace.join(GEODATA_FILE)).unwrap(), geodata);
        fs::write(workspace.join(GEODATA_FILE), b"updated via mirror").unwrap();
        seed_file(
            &workspace,
            GEODATA_FILE,
            GEODATA_SHA256,
            GEODATA_FILE,
            std::slice::from_ref(&geodata_bundle),
        )
        .unwrap();
        assert_eq!(
            fs::read(workspace.join(GEODATA_FILE)).unwrap(),
            b"updated via mirror"
        );
    }

    #[test]
    fn bundled_ui_is_copied_once_and_stays_private() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("ui");
        fs::create_dir_all(bundle.join("_next")).unwrap();
        fs::write(bundle.join(UI_INDEX), b"<html></html>").unwrap();
        fs::write(bundle.join("_next/app.js"), b"console.log(1)").unwrap();
        let workspace = dir.path().join("workspace");
        seed_ui(&workspace, std::slice::from_ref(&bundle)).unwrap();
        assert_eq!(
            fs::read(workspace.join("ui/index.html")).unwrap(),
            b"<html></html>"
        );
        fs::write(workspace.join("ui/index.html"), b"user update").unwrap();
        seed_ui(&workspace, std::slice::from_ref(&bundle)).unwrap();
        assert_eq!(
            fs::read(workspace.join("ui/index.html")).unwrap(),
            b"user update"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(workspace.join("ui/_next/app.js"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
        let missing = dir.path().join("missing");
        assert!(seed_ui(
            &dir.path().join("source-only"),
            std::slice::from_ref(&missing)
        )
        .is_ok());
        let corrupt = dir.path().join("corrupt-ui");
        fs::create_dir(&corrupt).unwrap();
        assert!(seed_ui(&dir.path().join("broken"), std::slice::from_ref(&corrupt)).is_err());
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
