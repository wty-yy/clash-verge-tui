use crate::model::DemoState;
use anyhow::{bail, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn default_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local/state")
        })
        .join("clash-verge-tui")
}
pub fn load(dir: &Path) -> Result<DemoState> {
    let path = dir.join("demo-state.json");
    if !path.exists() {
        return Ok(DemoState::default());
    }
    let mut state: DemoState = serde_json::from_slice(&fs::read(&path)?).with_context(|| {
        format!(
            "无法读取 {}；请备份后修复，或使用 --data-dir 指定新目录",
            path.display()
        )
    })?;
    if state.schema != 1 {
        bail!("不支持的演示状态版本：{}", state.schema);
    }
    if state.groups.is_empty() || state.nodes.is_empty() {
        bail!("演示状态缺少代理组或节点");
    }
    state.mode %= 3;
    state.active_profile = state
        .active_profile
        .min(state.profiles.len().saturating_sub(1));
    for (key, value) in crate::settings::defaults() {
        state.settings.entry(key).or_insert(value);
    }
    Ok(state)
}
pub fn save(dir: &Path, state: &DemoState) -> Result<()> {
    fs::create_dir_all(dir)?;
    let path = dir.join("demo-state.json");
    let tmp = dir.join("demo-state.json.tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    use std::io::Write;
    file.write_all(&serde_json::to_vec_pretty(state)?)?;
    file.sync_all()?;
    fs::rename(tmp, path)?;
    Ok(())
}
