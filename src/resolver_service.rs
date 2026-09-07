//! Narrow root service for the four systemd-resolved operations used by sing-tun.
//! Neither the TUI nor its core needs to run as root. No shell or arbitrary
//! command can be requested through this socket.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{FileTypeExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const LIMIT: u64 = 2048;
#[derive(Serialize, Deserialize)]
struct Reply {
    ok: bool,
    message: String,
}

pub fn socket_path(dir: &Path, uid: u32) -> PathBuf {
    Path::new("/run/clash-verge-tui")
        .join(format!("{}.sock", crate::service::tun_base_name(dir, uid)))
}

pub fn shim(dir: &Path, uid: u32) -> String {
    let quoted = format!("'{}'", dir.to_string_lossy().replace('\'', "'\\''"));
    format!("#!/bin/sh\n# Managed by clash-verge-tui: root DNS service protocol 1\nexec /usr/libexec/clash-verge-tui/tun-helper --tun-helper dns-client --tun-uid {uid} --data-dir {quoted} -- \"$@\"\n")
}

fn peer_uid(stream: &UnixStream) -> Result<u32> {
    use std::os::fd::AsRawFd;
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut _,
            &mut length,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(cred.uid)
}

pub fn request(dir: &Path, uid: u32, args: &[String]) -> Result<()> {
    let mut stream = UnixStream::connect(socket_path(dir, uid))
        .context("TUN DNS service unavailable; install or restart the permission service")?;
    if peer_uid(&stream)? != 0 {
        bail!("TUN DNS socket is not served by root");
    }
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut message = serde_json::to_vec(args)?;
    if message.len() >= LIMIT as usize {
        bail!("TUN DNS request too large");
    }
    message.push(b'\n');
    stream.write_all(&message)?;
    let mut reply = String::new();
    BufReader::new(stream)
        .take(4096)
        .read_to_string(&mut reply)?;
    let reply: Reply = serde_json::from_str(&reply)?;
    if !reply.ok {
        bail!("{}", reply.message);
    }
    Ok(())
}

pub fn ready(dir: &Path, uid: u32) -> bool {
    // One small local request is needed only when starting/restarting the core.
    request(dir, uid, &["status".into()]).is_ok()
}

/// Validate syntax before looking up any privileged state.
pub fn validate_args(args: &[String]) -> Result<()> {
    if args.len() < 2 || args.len() > 10 {
        bail!("Unsupported TUN DNS request");
    }
    let device = &args[1];
    if device.is_empty()
        || device.len() > 15
        || !device
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
    {
        bail!("Invalid TUN interface name");
    }
    match args[0].as_str() {
        "domain" if args.len() == 3 => {
            let domain = args[2]
                .strip_prefix('~')
                .context("Only routing domains are supported")?;
            if domain.is_empty()
                || domain.len() > 253
                || !domain
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c))
            {
                bail!("Invalid routing domain");
            }
        }
        "default-route" if args.len() == 3 && ["true", "false"].contains(&args[2].as_str()) => {}
        "dns" if args.len() >= 3 => {
            for address in &args[2..] {
                address
                    .parse::<std::net::IpAddr>()
                    .context("Invalid TUN DNS address")?;
            }
        }
        "revert" if args.len() == 2 => {}
        _ => bail!("Unsupported TUN DNS operation"),
    }
    Ok(())
}

fn authorize_interface(dir: &Path, device: &str, known: &mut BTreeMap<String, u32>) -> Result<u32> {
    let net = Path::new("/sys/class/net").join(device);
    if !net.join("tun_flags").is_file() {
        bail!("DNS request does not target a TUN interface");
    }
    let index = fs::read_to_string(net.join("ifindex"))?
        .trim()
        .parse::<u32>()?;
    if known.get(device) == Some(&index) {
        return Ok(index);
    }
    let file = fs::File::open(dir.join("core/config.yaml"))?;
    let mut text = String::new();
    file.take(8 * 1024 * 1024 + 1).read_to_string(&mut text)?;
    if text.len() > 8 * 1024 * 1024 {
        bail!("Runtime configuration is too large");
    }
    let config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text)?;
    if config["tun"]["device"].as_str().unwrap_or("Meta") != device
        || !config["tun"]["enable"].as_bool().unwrap_or(false)
    {
        bail!("TUN interface does not belong to this workspace");
    }
    // Keep the old index until teardown so changing the configured device does
    // not prevent sing-tun from reverting its previous interface.
    known.retain(|name, old_index| {
        fs::read_to_string(Path::new("/sys/class/net").join(name).join("ifindex"))
            .is_ok_and(|value| value.trim().parse::<u32>().ok() == Some(*old_index))
    });
    known.insert(device.into(), index);
    Ok(index)
}

fn apply(dir: &Path, args: &[String], known: &mut BTreeMap<String, u32>) -> Result<()> {
    if args == ["status"] {
        return Ok(());
    }
    validate_args(args)?;
    // Interface deletion already clears resolved's link state.
    if args[0] == "revert" && !Path::new("/sys/class/net").join(&args[1]).exists() {
        known.remove(&args[1]);
        return Ok(());
    }
    let index = authorize_interface(dir, &args[1], known)?;
    let mut child = Command::new("/usr/bin/resolvectl")
        .arg(&args[0])
        .arg(index.to_string())
        .args(&args[2..])
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                bail!("System DNS {} failed ({status})", args[0]);
            }
            break;
        }
        if started.elapsed() > Duration::from_secs(3) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("System DNS request timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if args[0] == "revert" {
        known.remove(&args[1]);
    }
    Ok(())
}

pub fn serve(dir: &Path, uid: u32) -> Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        bail!("DNS service must run as root");
    }
    let socket = socket_path(dir, uid);
    fs::create_dir_all(socket.parent().unwrap())?;
    if let Ok(metadata) = fs::symlink_metadata(&socket) {
        if !metadata.file_type().is_socket() {
            bail!("Refusing to replace a non-socket DNS endpoint");
        }
        fs::remove_file(&socket)?;
    }
    let listener = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    let path = std::ffi::CString::new(socket.as_os_str().as_encoded_bytes())?;
    if unsafe { libc::chown(path.as_ptr(), uid, u32::MAX) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut known = BTreeMap::new();
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else {
            continue;
        };
        if ![0, uid].contains(&peer_uid(&stream)?) {
            continue;
        }
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let result = (|| -> Result<()> {
            let mut line = String::new();
            BufReader::new((&stream).take(LIMIT)).read_line(&mut line)?;
            if line.len() >= LIMIT as usize {
                bail!("TUN DNS request too large");
            }
            let args: Vec<String> = serde_json::from_str(&line)?;
            apply(dir, &args, &mut known)
        })();
        let reply = match result {
            Ok(()) => Reply {
                ok: true,
                message: String::new(),
            },
            Err(error) => Reply {
                ok: false,
                message: error.to_string(),
            },
        };
        let _ = stream.write_all(&serde_json::to_vec(&reply)?);
    }
    Ok(())
}
