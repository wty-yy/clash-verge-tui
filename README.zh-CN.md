# Clash Verge TUI

[English](README.md) | [更新记录](CHANGELOG.zh-CN.md)

基于 Rust + Ratatui 的 Clash Verge 风格终端界面，参照 Clash Verge Rev v2.5.2。

`v0.1.1` 为界面预览版。八个主页面、二级表单与本地交互可用；mihomo、订阅下载、系统代理和 TUN 尚未接入。所有网络数据均为演示数据。

![首页预览](docs/previews/home.png)

## 使用

Linux；Rust 1.88+；支持 UTF-8 的终端。建议 120 × 40，最低 76 × 24。

```bash
# 编译并打开终端界面
cargo run --locked --release -- --demo

# 安装到 ~/.cargo/bin
cargo install --locked --path .
clash-verge-tui

# 使用独立演示状态目录
clash-verge-tui --data-dir .local/demo

# 查看版本与命令参数
clash-verge-tui --version
clash-verge-tui --help

# 导出页面快照；不读取或保存用户状态
clash-verge-tui --snapshot proxies --output proxies.svg
clash-verge-tui --snapshot home --width 100 --height 30
```

| 按键 | 操作 |
| --- | --- |
| `1`–`8` | 首页、代理、订阅、连接、规则、日志、解锁检测、设置 |
| `Tab` / `Shift+Tab` | 切换页内分组 |
| `↑` / `↓`、`j` / `k` | 选择条目；`Enter` 执行 |
| `/`、`Esc` | 搜索、清除筛选或取消弹窗 |
| `a` / `e` / `d` | 新建、编辑、删除；以页面工具栏为准 |
| `r`、`s`、`m` | 演示刷新、排序、切换代理模式 |
| `v`、`[` / `]` | 订阅 YAML 编辑、订阅或增强链排序 |
| `b` / `R` | 设置页创建备份、恢复最近备份 |
| `Ctrl+S` | 保存表单；`Ctrl+U` 清空字段 |
| `:`、`?`、`t` | 页面跳转、帮助、主题切换 |
| `q` / `Ctrl+C` | 退出 |

默认启用 Vim 导航，底栏显示 `j/k` 上下选择提示；关闭后隐藏提示。选中状态与开关统一使用 `[✓]` / `[ ]`。流量图随终端宽高缩放，保留同一段历史采样范围。

支持鼠标点击导航、分组、工具按钮和表单；点击列表行选择后按 `Enter` 操作。多行表单使用 `Enter` 换行，`Tab` 切换字段。终端文本选择使用 `Shift` + 鼠标。

## 实现细节

- `src/model.rs`：类型定义与固定演示数据。
- `src/app.rs`：页面状态、输入处理、表单校验、演示交互。
- `src/ui.rs`：布局、主题、表格、图表、弹窗与鼠标区域。
- `src/settings.rs`：系统、内核、界面、高级设置表单。
- `src/storage.rs`：独立状态加载与原子保存。
- `tests/workflows.rs`：页面渲染与关键交互回归。
- [功能对照](docs/FEATURES.md)：已完成界面与后续内核能力。

默认状态文件：`${XDG_STATE_HOME:-$HOME/.local/state}/clash-verge-tui/demo-state.json`。只接受绝对路径形式的 `XDG_STATE_HOME`。`--data-dir` 优先级更高。

修改立即保存；未提交的弹窗修改不保存。状态文件在 Unix 下使用 `0600` 权限，不做加密。损坏或不支持的状态文件会报错并保留原文件，可使用新的 `--data-dir` 启动。单目录同时运行多个实例的并发写入暂不支持。

演示备份保存在同一状态文件中，最多 10 份，包含设置、订阅、配置增强、规则与当前订阅；`R` 恢复最近一份。设置 → 高级 → 备份与恢复提供历史列表、选定恢复和删除。WebDAV 为配置表单，尚不执行同步。

主题、强调色、紧凑导航、流量图、内存显示、鼠标、Vim 键位、启动页、刷新间隔在终端内生效。其余网络及系统设置仅保存演示值。YAML / JavaScript 仅文本编辑，不执行或进行完整语法校验。

## 开发与版本

```bash
# 格式、静态检查与回归测试
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked

# 发布构建
cargo build --locked --release

# 重新生成八个主页面的 SVG 快照
for page in home proxies profiles connections rules logs unlock settings; do
  target/release/clash-verge-tui --snapshot "$page" --output "docs/previews/$page.svg"
done
```

版本从 `v0.1.0` 开始，使用语义化版本与带注释的 Git 标签。迭代流程见 [版本维护](docs/RELEASING.md)。README 与 CHANGELOG 同步维护中英文版本。

上游参考仓库位于 `upstream/clash-verge-rev`（Git 忽略），固定为 `v2.5.2` / `28f2efc`。本项目为独立终端实现，不属于官方 Clash Verge Rev 项目。许可：[GPL-3.0-only](LICENSE)。
