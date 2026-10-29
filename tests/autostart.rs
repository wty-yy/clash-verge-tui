use clash_verge_tui::{service, workspace};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn executable(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for program in ["systemctl", "loginctl"] {
            executable(
                &root.path().join(program),
                &format!(
                    "#!/bin/sh\nprintf '%s\\n' '{program}' \"$@\" >> \"$AUTOSTART_CALLS\"\n{}\n",
                    if program == "loginctl" {
                        "exit \"${AUTOSTART_LINGER_FAIL:-0}\""
                    } else {
                        "if [ \"$2\" = show ]; then printf '%s\\n' \"${AUTOSTART_SERVICE_STATE:-inactive}\"; fi\nexit 0"
                    }
                ),
            );
        }
        Self { root }
    }

    fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = Command::new(program);
        command
            .env("HOME", self.root.path())
            .env("XDG_CONFIG_HOME", self.root.path().join("config"))
            .env("PATH", self.root.path())
            .env("AUTOSTART_CALLS", self.root.path().join("calls"));
        command
    }

    fn calls(&self) -> String {
        fs::read_to_string(self.root.path().join("calls")).unwrap_or_default()
    }

    fn install(&self) -> Command {
        let mut command = self.command(std::env::current_exe().unwrap());
        command
            .args(["--exact", "configure_autostart_child", "--nocapture"])
            .env("AUTOSTART_WORKSPACE", self.root.path().join("workspace"));
        command
    }
}

#[test]
fn install_enables_linger_and_the_workspace_service() {
    let fixture = Fixture::new();
    let output = fixture.install().output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let dir = fixture.root.path().join("workspace");
    let unit = fs::read_to_string(
        fixture
            .root
            .path()
            .join("config/systemd/user")
            .join(service::name(&dir)),
    )
    .unwrap();
    assert!(unit.contains("WantedBy=default.target"));
    assert!(unit.contains("--daemon --data-dir"));
    let calls = fixture.calls();
    assert!(calls.contains(&format!(
        "loginctl\n--no-ask-password\nenable-linger\n{}\n",
        unsafe { libc::getuid() }
    )));
    assert!(calls.contains(&format!(
        "systemctl\n--user\nenable\n{}\n",
        service::name(&dir)
    )));
    assert!(!calls.contains("\nstart\n"));
}

#[test]
fn denied_linger_does_not_report_a_successful_install() {
    let fixture = Fixture::new();
    let output = fixture
        .install()
        .env("AUTOSTART_LINGER_FAIL", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("sudo loginctl enable-linger"));
    assert!(!fixture.calls().contains("systemctl"));
    assert!(!fixture
        .root
        .path()
        .join("workspace/workspace-state.json")
        .exists());
}

// Run service operations in a child process so HOME/PATH never affect other tests.
#[test]
fn configure_autostart_child() {
    let Some(dir) = std::env::var_os("AUTOSTART_WORKSPACE") else {
        return;
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = if std::env::var_os("AUTOSTART_DISABLE").is_some() {
        runtime.block_on(service::install(Path::new(&dir), false))
    } else {
        runtime.block_on(service::configure_autostart(Path::new(&dir)))
    };
    if let Err(error) = result {
        eprintln!(
            "{}",
            clash_verge_tui::locale::error_chain(
                &error,
                clash_verge_tui::locale::Language::English
            )
        );
        std::process::exit(1);
    }
}

#[test]
fn automatic_setup_repairs_a_disabled_unit_and_respects_opt_out() {
    let fixture = Fixture::new();
    let dir = fixture.root.path().join("workspace");
    workspace::initialize(&dir).unwrap();
    for enabled in [true, true, false] {
        workspace::save_auto_launch_preference(&dir, enabled).unwrap();
        let _ = fs::remove_file(fixture.root.path().join("calls"));
        let output = fixture
            .command(std::env::current_exe().unwrap())
            .args(["--exact", "configure_autostart_child"])
            .env("AUTOSTART_WORKSPACE", &dir)
            .env("AUTOSTART_SERVICE_STATE", "active")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let calls = fixture.calls();
        if enabled {
            assert!(calls.contains("enable-linger"));
            assert!(calls.contains("\nenable\n"));
            assert_eq!(
                workspace::load(&dir).unwrap().state.preferences["service"],
                "运行中"
            );
        } else {
            assert!(calls.is_empty());
        }
        assert!(!calls.contains("\nstart\n"));
        assert!(!calls.contains("disable-linger"));
    }
    let output = fixture
        .install()
        .env("AUTOSTART_DISABLE", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let calls = fixture.calls();
    assert!(calls.contains("\ndisable\n"));
    assert!(!calls.contains("loginctl"));
}
