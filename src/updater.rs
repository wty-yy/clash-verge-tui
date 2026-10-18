//! Release updates run independently of controller requests and commit the app last.
use anyhow::{anyhow, bail, Context, Result};
use flate2::read::GzDecoder;
use futures_util::StreamExt;
use semver::Version;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    sync::mpsc::Sender,
    time::{Duration, Instant},
};

const MAX_DOWNLOAD: u64 = 192 * 1024 * 1024;
const MAX_UNPACKED: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Default)]
pub enum Phase {
    #[default]
    Idle,
    Checking,
    Available,
    Downloading {
        received: u64,
        total: Option<u64>,
    },
    Verifying,
    Installing,
    Installed(PathBuf),
    Failed(String),
}
impl Phase {
    pub fn busy(&self) -> bool {
        matches!(
            self,
            Self::Checking | Self::Downloading { .. } | Self::Verifying | Self::Installing
        )
    }
}
#[derive(Clone, Debug, Default)]
pub struct State {
    pub latest: Option<Version>,
    pub phase: Phase,
}
impl State {
    pub fn visible(&self) -> bool {
        self.latest.is_some() || !matches!(self.phase, Phase::Idle)
    }
}
#[derive(Clone, Debug)]
pub enum Command {
    Check,
    Install(Version),
}
#[derive(Debug)]
pub enum Event {
    Checked(Result<Option<Version>, String>),
    Progress(Phase),
    Finished(Result<PathBuf, String>),
}
fn emit(tx: &Sender<crate::core::CoreEvent>, event: Event) {
    let _ = tx.send(crate::core::CoreEvent::ApplicationUpdate(event));
}
fn release_version(text: &str) -> Result<Version> {
    let version = Version::parse(text.trim().trim_start_matches('v'))?;
    if !version.pre.is_empty() || !version.build.is_empty() {
        bail!("更新版本必须为正式版本");
    }
    Ok(version)
}

pub async fn check() -> Result<Option<Version>> {
    let direct = reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(12))
        .user_agent("clash-verge-tui")
        .build()?;
    if let Ok(response) = direct
        .get(crate::sources::latest_version_url())
        .send()
        .await
    {
        if let Ok(text) = small_response(response).await {
            if let Ok(version) = release_version(&text) {
                return Ok(Some(version));
            }
        }
    }
    // Releases exclude unpublished tags, drafts, and prereleases.
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .user_agent("clash-verge-tui")
        .build()?;
    let response = client
        .get(format!(
            "{}/repos/{}/releases/latest",
            crate::sources::GITHUB_API_ROOT,
            crate::sources::PROJECT_REPOSITORY
        ))
        .send()
        .await
        .map_err(|e| anyhow!("检查更新失败：{}", e.without_url()))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let value: serde_json::Value = serde_json::from_str(&small_response(response).await?)?;
    // The API payload is larger than checksum responses, but still bounded.
    Ok(Some(release_version(
        value["tag_name"].as_str().context("更新版本无效")?,
    )?))
}

async fn small_response(response: reqwest::Response) -> Result<String> {
    if !response.status().is_success() {
        bail!("更新服务器 HTTP {}", response.status().as_u16());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(&chunk.map_err(|e| anyhow!("更新下载失败：{}", e.without_url()))?);
        if bytes.len() > 128 * 1024 {
            bail!("更新响应过大");
        }
    }
    Ok(String::from_utf8(bytes)?)
}

pub async fn execute(command: Command, tx: Sender<crate::core::CoreEvent>) {
    match command {
        Command::Check => {
            emit(&tx, Event::Progress(Phase::Checking));
            emit(
                &tx,
                Event::Checked(check().await.map_err(|e| e.to_string())),
            );
        }
        Command::Install(version) => {
            let result = download_install(&version, &tx)
                .await
                .map_err(|e| format!("{e:#}"));
            emit(&tx, Event::Finished(result));
        }
    }
}

fn checksum(text: &str, name: &str) -> Result<String> {
    for line in text.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() == 2
            && fields[1].trim_start_matches('*') == name
            && fields[0].len() == 64
            && fields[0].bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Ok(fields[0].to_ascii_lowercase());
        }
    }
    bail!("发行包 SHA-256 校验文件无效");
}

async fn download_install(
    version: &Version,
    tx: &Sender<crate::core::CoreEvent>,
) -> Result<PathBuf> {
    if release_version(&version.to_string())? <= Version::parse(env!("CARGO_PKG_VERSION"))? {
        bail!("更新版本必须高于当前版本");
    }
    let executable = std::env::current_exe()?.canonicalize()?;
    let github = format!(
        "{}/{}/releases/download/v{version}",
        crate::sources::GITHUB_ROOT,
        crate::sources::PROJECT_REPOSITORY
    );
    let sources = [
        format!("{}/v{version}", crate::sources::MIRROR_ROOT),
        github,
    ];
    download_install_to(version, executable, sources, tx).await
}
async fn download_install_to(
    version: &Version,
    executable: PathBuf,
    sources: [String; 2],
    tx: &Sender<crate::core::CoreEvent>,
) -> Result<PathBuf> {
    let bin = executable.parent().context("无法确定安装目录")?;
    let lock_path = bin.join(".clash-verge-tui-update.lock");
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)?;
    fs2::FileExt::try_lock_exclusive(&lock).context("另一个应用更新正在进行")?;
    let staging = tempfile::Builder::new()
        .prefix(".clash-update-")
        .tempdir_in(bin)
        .context("安装目录不可写，请使用安装脚本更新")?;
    let name = format!(
        "clash-verge-tui-v{version}-linux-{}.tar.gz",
        crate::core_manager::asset()?.arch
    );
    let archive = staging.path().join(&name);
    let mut downloaded = false;
    let mut error = None;
    for (index, base) in sources.iter().enumerate() {
        let mut builder = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(300))
            .user_agent("clash-verge-tui");
        if index == 0 {
            builder = builder.no_proxy();
        }
        let client = builder.build()?;
        let result = async {
            let response = client
                .get(format!("{base}/{name}.sha256"))
                .timeout(Duration::from_secs(15))
                .send()
                .await
                .map_err(|e| anyhow!("更新下载失败：{}", e.without_url()))?;
            let expected = checksum(&small_response(response).await?, &name)?;
            download(&client, &format!("{base}/{name}"), &archive, tx).await?;
            emit(tx, Event::Progress(Phase::Verifying));
            let actual = format!("{:x}", Sha256::digest(fs::read(&archive)?));
            if actual != expected {
                bail!("发行包 SHA-256 校验失败");
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        match result {
            Ok(()) => {
                downloaded = true;
                break;
            }
            Err(e) => error = Some(e),
        }
    }
    if !downloaded {
        return Err(error.context("更新下载失败")?);
    }
    emit(tx, Event::Progress(Phase::Installing));
    let version = version.clone();
    let target = executable.clone();
    tokio::task::spawn_blocking(move || {
        let _lock = lock;
        let package = staging.path().join("package");
        unpack(&archive, &package)?;
        install(&package, &target, &version)?;
        Ok(target)
    })
    .await?
}

async fn download(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
    tx: &Sender<crate::core::CoreEvent>,
) -> Result<()> {
    let response = tokio::time::timeout(Duration::from_secs(15), client.get(url).send())
        .await
        .context("更新下载停滞，正在切换下载源")?
        .map_err(|e| anyhow!("更新下载失败：{}", e.without_url()))?;
    if !response.status().is_success() {
        bail!("更新服务器 HTTP {}", response.status().as_u16());
    }
    let total = response.content_length();
    if total.is_some_and(|size| size > MAX_DOWNLOAD) {
        bail!("发行包过大");
    }
    let mut file = fs::File::create(path)?;
    let mut stream = response.bytes_stream();
    let mut received = 0;
    let mut reported = Instant::now();
    emit(tx, Event::Progress(Phase::Downloading { received, total }));
    loop {
        let next = tokio::time::timeout(Duration::from_secs(15), stream.next())
            .await
            .context("更新下载停滞，正在切换下载源")?;
        let Some(chunk) = next else { break };
        let chunk = chunk.map_err(|e| anyhow!("更新下载失败：{}", e.without_url()))?;
        received += chunk.len() as u64;
        if received > MAX_DOWNLOAD {
            bail!("发行包过大");
        }
        file.write_all(&chunk)?;
        if reported.elapsed() >= Duration::from_millis(100) {
            emit(tx, Event::Progress(Phase::Downloading { received, total }));
            reported = Instant::now();
        }
    }
    if total.is_some_and(|size| size != received) {
        bail!("发行包下载不完整");
    }
    file.sync_all()?;
    emit(tx, Event::Progress(Phase::Downloading { received, total }));
    Ok(())
}

fn unpack(archive: &Path, destination: &Path) -> Result<()> {
    fs::create_dir(destination)?;
    let mut archive = tar::Archive::new(GzDecoder::new(fs::File::open(archive)?));
    let mut size = 0u64;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        if path
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        {
            bail!("发行包包含不安全路径");
        }
        let kind = entry.header().entry_type();
        if !kind.is_file() && !kind.is_dir() {
            bail!("发行包包含非普通文件");
        }
        size = size.checked_add(entry.size()).context("发行包解压后过大")?;
        if size > MAX_UNPACKED {
            bail!("发行包解压后过大");
        }
        if !entry.unpack_in(destination)? {
            bail!("发行包包含不安全路径");
        }
    }
    Ok(())
}

fn install(package: &Path, executable: &Path, version: &Version) -> Result<()> {
    let source = package.join("lib/clash-verge-tui");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(source.join("release.json"))?)?;
    let core_version = release_version(
        manifest["mihomo_version"]
            .as_str()
            .context("发行包缺少内核版本")?,
    )?;
    if manifest["app_version"].as_str() != Some(version.to_string().as_str())
        || manifest["architecture"].as_str() != Some(crate::core_manager::asset()?.arch)
    {
        bail!("发行包版本或架构不匹配");
    }
    for file in ["mihomo", "GeoSite.dat", "geoip.metadb", "ui/index.html"] {
        if !source.join(file).is_file() {
            bail!("发行包缺少必要文件：{file}");
        }
    }
    let core_hash = format!("{:x}", Sha256::digest(fs::read(source.join("mihomo"))?));
    if manifest["mihomo_binary_sha256"].as_str() != Some(core_hash.as_str()) {
        bail!("发行包内核 SHA-256 校验失败");
    }
    let app = package.join("bin/clash-verge-tui");
    for (binary, expected) in [
        (&app, version.to_string()),
        (&source.join("mihomo"), format!("v{core_version}")),
    ] {
        let output = std::process::Command::new(binary)
            .arg(if binary == &app { "--version" } else { "-v" })
            .stdin(std::process::Stdio::null())
            .output()?;
        if !output.status.success()
            || !String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .any(|word| word == expected)
        {
            bail!("发行包程序版本校验失败");
        }
    }
    let core_dir = source.join(format!("core/v{core_version}"));
    fs::create_dir_all(&core_dir)?;
    fs::rename(source.join("mihomo"), core_dir.join("mihomo"))?;
    for (component, target) in [
        ("clash-verge-tui", "LICENSE"),
        ("mihomo", "MIHOMO-LICENSE"),
        ("meta-rules-dat", "GEOSITE-LICENSE"),
        ("metacubexd", "METACUBEXD-LICENSE"),
        ("musl", "MUSL-LICENSE"),
    ] {
        fs::copy(
            package.join(format!("share/licenses/{component}/LICENSE")),
            source.join(target),
        )?;
    }
    let bin = executable.parent().context("无法确定安装目录")?;
    let library_parent = bin.parent().context("无法确定安装目录")?.join("lib");
    fs::create_dir_all(&library_parent)?;
    // Stage on the library filesystem as well; do not touch the old install until all copies succeed.
    let staging = tempfile::Builder::new()
        .prefix(".clash-update-")
        .tempdir_in(&library_parent)?;
    let new_library = staging.path().join("new");
    copy_tree(&source, &new_library)?;
    let mut new_app = tempfile::NamedTempFile::new_in(bin)?;
    std::io::copy(&mut fs::File::open(&app)?, new_app.as_file_mut())?;
    fs::set_permissions(new_app.path(), fs::metadata(&app)?.permissions())?;
    new_app.as_file().sync_all()?;
    let library = library_parent.join("clash-verge-tui");
    let backup = staging.path().join("old");
    let had_library = library.exists();
    if had_library {
        fs::rename(&library, &backup)?;
    }
    let commit = (|| -> Result<()> {
        fs::rename(&new_library, &library)?;
        new_app.persist(executable).map_err(|e| e.error)?;
        Ok(())
    })();
    if let Err(error) = commit {
        if library.exists() {
            fs::remove_dir_all(&library)?;
        }
        if had_library {
            fs::rename(&backup, &library)?;
        }
        return Err(error);
    }
    Ok(())
}
fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), destination.join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Read,
        net::TcpListener,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread,
    };

    struct Server {
        url: String,
        stop: Arc<AtomicBool>,
        task: Option<thread::JoinHandle<()>>,
    }
    impl Server {
        fn new(handler: impl Fn(&str) -> (u16, Vec<u8>) + Send + 'static) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            listener.set_nonblocking(true).unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let stopped = stop.clone();
            let task = thread::spawn(move || {
                while !stopped.load(Ordering::Relaxed) {
                    if let Ok((mut socket, _)) = listener.accept() {
                        socket
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut request = [0; 4096];
                        let n = socket.read(&mut request).unwrap_or(0);
                        let request = String::from_utf8_lossy(&request[..n]);
                        let path = request.split_whitespace().nth(1).unwrap_or("");
                        let (status, body) = handler(path);
                        let _ = write!(socket, "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                        let _ = socket.write_all(&body);
                    } else {
                        thread::sleep(Duration::from_millis(2));
                    }
                }
            });
            Self {
                url,
                stop,
                task: Some(task),
            }
        }
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            self.task.take().unwrap().join().unwrap();
        }
    }
    fn package(root: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let package = root.join("package-source");
        let lib = package.join("lib/clash-verge-tui");
        fs::create_dir_all(lib.join("ui")).unwrap();
        fs::create_dir(package.join("bin")).unwrap();
        for (path, content) in [
            (
                package.join("bin/clash-verge-tui"),
                "#!/bin/sh\necho 'clash-verge-tui 1.8.0'\n",
            ),
            (
                lib.join("mihomo"),
                "#!/bin/sh\necho 'Mihomo Meta v1.19.32'\n",
            ),
        ] {
            fs::write(&path, content).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        for name in ["GeoSite.dat", "geoip.metadb", "ui/index.html"] {
            fs::write(lib.join(name), "fixture").unwrap();
        }
        let hash = format!(
            "{:x}",
            Sha256::digest(fs::read(lib.join("mihomo")).unwrap())
        );
        fs::write(lib.join("release.json"), serde_json::to_vec(&serde_json::json!({"app_version":"1.8.0", "mihomo_version":"1.19.32", "architecture":crate::core_manager::asset().unwrap().arch, "mihomo_binary_sha256":hash})).unwrap()).unwrap();
        for name in [
            "clash-verge-tui",
            "mihomo",
            "meta-rules-dat",
            "metacubexd",
            "musl",
        ] {
            let dir = package.join(format!("share/licenses/{name}"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("LICENSE"), "fixture license").unwrap();
        }
        package
    }
    fn archive(root: &Path) -> Vec<u8> {
        let package = package(root);
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut tar = tar::Builder::new(gz);
        tar.append_dir_all(".", package).unwrap();
        tar.into_inner().unwrap().finish().unwrap()
    }
    fn old_install(root: &Path) -> PathBuf {
        fs::create_dir(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("lib/clash-verge-tui")).unwrap();
        fs::write(root.join("lib/clash-verge-tui/old"), "keep until success").unwrap();
        let exe = root.join("bin/clash-verge-tui");
        fs::write(&exe, "old executable").unwrap();
        exe
    }
    #[tokio::test]
    async fn mirror_failure_falls_back_and_installs_the_entire_verified_release() {
        let root = tempfile::tempdir().unwrap();
        let body = archive(root.path());
        let executable = old_install(root.path());
        let name = format!(
            "clash-verge-tui-v1.8.0-linux-{}.tar.gz",
            crate::core_manager::asset().unwrap().arch
        );
        let sum = format!("{:x}  {name}\n", Sha256::digest(&body));
        let server = Server::new(move |path| {
            if path.starts_with("/mirror/") {
                (503, b"unavailable".to_vec())
            } else if path.ends_with(".sha256") {
                (200, sum.as_bytes().to_vec())
            } else {
                (200, body.clone())
            }
        });
        let (tx, rx) = std::sync::mpsc::channel();
        download_install_to(
            &Version::new(1, 8, 0),
            executable.clone(),
            [
                format!("{}/mirror", server.url),
                format!("{}/github", server.url),
            ],
            &tx,
        )
        .await
        .unwrap();
        assert!(fs::read_to_string(&executable).unwrap().contains("1.8.0"));
        let lib = root.path().join("lib/clash-verge-tui");
        assert!(lib.join("core/v1.19.32/mihomo").is_file());
        assert!(lib.join("ui/index.html").is_file());
        assert!(lib.join("MIHOMO-LICENSE").is_file());
        assert!(!lib.join("old").exists());
        let events: Vec<_> = rx.try_iter().collect();
        assert!(events.iter().any(|e| matches!(e, crate::core::CoreEvent::ApplicationUpdate(Event::Progress(Phase::Downloading { received, total: Some(total) })) if *received>0 && received==total)));
        assert!(events.iter().any(|e| matches!(
            e,
            crate::core::CoreEvent::ApplicationUpdate(Event::Progress(Phase::Installing))
        )));
    }
    #[tokio::test]
    async fn bad_checksum_preserves_executable_and_library() {
        let root = tempfile::tempdir().unwrap();
        let executable = old_install(root.path());
        let name = format!(
            "clash-verge-tui-v1.8.0-linux-{}.tar.gz",
            crate::core_manager::asset().unwrap().arch
        );
        let server = Server::new(move |path| {
            if path.ends_with(".sha256") {
                (200, format!("{}  {name}\n", "0".repeat(64)).into_bytes())
            } else {
                (200, b"corrupt archive".to_vec())
            }
        });
        let (tx, _) = std::sync::mpsc::channel();
        let result = download_install_to(
            &Version::new(1, 8, 0),
            executable.clone(),
            [server.url.clone(), server.url.clone()],
            &tx,
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("SHA-256"));
        assert_eq!(fs::read_to_string(executable).unwrap(), "old executable");
        assert!(root.path().join("lib/clash-verge-tui/old").exists());
    }
    #[test]
    fn failed_commit_restores_the_old_library() {
        let root = tempfile::tempdir().unwrap();
        let package = package(root.path());
        let executable = old_install(root.path());
        fs::remove_file(&executable).unwrap();
        fs::create_dir(&executable).unwrap();
        assert!(install(&package, &executable, &Version::new(1, 8, 0)).is_err());
        assert!(executable.is_dir());
        assert!(root.path().join("lib/clash-verge-tui/old").is_file());
    }
    #[test]
    fn extraction_rejects_links_and_escaping_paths() {
        for escape in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let file = root.path().join("bad.tar.gz");
            let gz = flate2::write::GzEncoder::new(
                fs::File::create(&file).unwrap(),
                flate2::Compression::default(),
            );
            let mut tar = tar::Builder::new(gz);
            let mut h = tar::Header::new_gnu();
            h.set_size(0);
            h.set_mode(0o644);
            if escape {
                h.as_mut_bytes()[..10].copy_from_slice(b"../outside");
                h.set_entry_type(tar::EntryType::Regular);
            } else {
                h.set_path("link").unwrap();
                h.set_entry_type(tar::EntryType::Symlink);
                h.set_link_name("/tmp").unwrap();
            }
            h.set_cksum();
            tar.append(&h, std::io::empty()).unwrap();
            tar.into_inner().unwrap().finish().unwrap();
            assert!(unpack(&file, &root.path().join("unpacked")).is_err());
            assert!(!root.path().join("outside").exists());
        }
    }
    #[test]
    fn checksum_and_versions_reject_unpublished_or_malformed_inputs() {
        assert!(release_version("v1.8.0-alpha").is_err());
        assert!(release_version("v1.8.0+build").is_err());
        assert!(checksum(&format!("{}  other.tar.gz", "a".repeat(64)), "app.tar.gz").is_err());
        assert_eq!(
            checksum(&format!("{} *app.tar.gz", "A".repeat(64)), "app.tar.gz").unwrap(),
            "a".repeat(64)
        );
    }
    #[test]
    #[ignore = "requires a packaged release in CLASH_VERGE_TUI_TEST_UPDATE_ARCHIVE"]
    fn actual_release_archive_installs_in_an_isolated_prefix() {
        let archive = PathBuf::from(
            std::env::var_os("CLASH_VERGE_TUI_TEST_UPDATE_ARCHIVE").expect("release archive path"),
        );
        let root = tempfile::tempdir().unwrap();
        let executable = old_install(root.path());
        let package = root.path().join("unpacked");
        unpack(&archive, &package).unwrap();
        install(
            &package,
            &executable,
            &Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
        )
        .unwrap();
        let out = std::process::Command::new(executable)
            .arg("--version")
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(
            String::from_utf8(out.stdout).unwrap().trim(),
            format!("clash-verge-tui {}", env!("CARGO_PKG_VERSION"))
        );
        assert!(root
            .path()
            .join(format!(
                "lib/clash-verge-tui/core/v{}/mihomo",
                crate::core_manager::MIHOMO_VERSION
            ))
            .is_file());
    }
}
