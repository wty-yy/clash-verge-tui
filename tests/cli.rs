use std::process::Command;
fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_clash-verge-tui"))
}
#[test]
fn version_matches_package_and_non_terminal_start_is_actionable() {
    let out = binary().arg("--version").output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains(env!("CARGO_PKG_VERSION")));
    let help = binary().arg("--help").output().unwrap();
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(help.contains("启动自管内核"));
    assert!(!help.contains("--connect"));
    assert!(!help.contains("--core"));
    let out = binary().output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--snapshot home"));
}
#[test]
fn snapshots_are_deterministic_and_do_not_touch_existing_state() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("demo-state.json");
    std::fs::write(&state, "do not read or overwrite").unwrap();
    let args = [
        "--snapshot",
        "home",
        "--data-dir",
        dir.path().to_str().unwrap(),
    ];
    let first = binary().args(args).output().unwrap();
    let second = binary().args(args).output().unwrap();
    assert!(first.status.success());
    assert_eq!(first.stdout, second.stdout);
    assert!(String::from_utf8_lossy(&first.stdout).contains("演示工作区"));
    assert_eq!(
        std::fs::read_to_string(state).unwrap(),
        "do not read or overwrite"
    );
}
#[test]
fn svg_export_and_invalid_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let svg = dir.path().join("proxies.svg");
    let out = binary()
        .args(["--snapshot", "proxies", "--output", svg.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let content = std::fs::read_to_string(svg).unwrap();
    assert!(content.starts_with("<svg"));
    assert!(content.contains("actual terminal buffer"));
    for args in [
        vec!["--snapshot", "unknown"],
        vec!["--output", "ignored.txt"],
        vec!["--snapshot", "home", "--width", "1000"],
    ] {
        assert!(!binary().args(args).output().unwrap().status.success());
    }
}
