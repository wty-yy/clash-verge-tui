<div align="center">
  <h1>Clash Verge TUI</h1>
  <p><strong>基于 Rust、Ratatui 和 mihomo 的 Linux 终端代理客户端</strong></p>
  <p><a href="README.md">🌎 English</a>&nbsp;&nbsp;·&nbsp;&nbsp;<strong>🇨🇳 中文</strong></p>
  <p>
    <img alt="仅支持 Linux" src="https://img.shields.io/badge/platform-Linux%20only-FCC624?logo=linux&logoColor=black">
    <a href="https://github.com/wty-yy/clash-verge-tui/releases/latest"><img alt="最新发行版" src="https://img.shields.io/github/v/release/wty-yy/clash-verge-tui?label=release"></a>
  </p>
</div>

Clash Verge TUI 是**仅支持 Linux**（x86_64/aarch64）的 mihomo 终端客户端，沿用 [Clash Verge Rev](https://github.com/clash-verge-rev/clash-verge-rev) 的界面布局与使用流程，打开就能上手。

## 特性

- **贴近 Clash Verge 的界面** — 首页 / 代理 / 订阅 / 连接 / 规则 / 日志 / 解锁检测 / 设置与桌面版一一对应
- **系统代理与 TUN 双模式** — 混合代理端口加 TUN 网卡；首次开启 TUN 在界面内输入系统密码
- **退出后台驻留** — 关闭界面后由 systemd 用户服务继续运行内核与系统代理，不占用前台终端
- **自管内核** — 固定 mihomo v1.19.29 与固定版本 `GeoSite.dat`；musl 静态构建，无运行时依赖
- **订阅与增强** — 链接导入、YAML / JavaScript 增强链、定时更新
- **日常操作** — 实时流量与会话、规则启停、延迟测速、解锁检测、加密备份与 WebDAV 同步
- **终端原生** — 简体中文 / 繁體中文 / English 三语，Vim 按键与鼠标操作，深浅主题，任意缩放窗口都保持可读

![中文 TUI 演示](docs/previews/demo-zh-CN.gif)

该项目为个人使用而制作，使用 ChatGPT 辅助开发，界面与功能映射参考 [Clash Verge Rev](https://github.com/clash-verge-rev/clash-verge-rev) v2.5.2。该项目非 Clash Verge / Clash Verge Rev 官方制作，与其开发团队无隶属关系。

## 安装

```bash
# 一键安装最新发行版到 ~/.local/bin，并同步安装固定 mihomo v1.19.29
curl -fsSL https://github.com/wty-yy/clash-verge-tui/releases/latest/download/install.sh | sh

# GitHub 不可达时使用项目 Cloudflare 镜像 clash-verge-tui.wty-yy.top
curl -fsSL https://clash-verge-tui.wty-yy.top/install.sh | sh -s -- --source proxy

# 启动真实 TUI；首次启动创建自管工作区
clash-verge-tui
```

安装脚本会校验发行包发布的 SHA-256，并兼容 Ubuntu 20.04 的 curl 7.68 等旧版本。`--source proxy` 通过项目维护的 Cloudflare Worker 镜像 `clash-verge-tui.wty-yy.top` 下载脚本与归档（源码与部署说明见 [deploy/gh-mirror](deploy/gh-mirror/README.zh-CN.md)），可用 `--github-proxy https://gh-proxy.com` 切换到社区前缀。`~/.local/bin` 不在 `PATH` 时手动添加；设置 `CLASH_VERGE_TUI_VERSION` 可安装指定的已发布版本。

## 使用

- **界面语言** — 默认跟随系统；可在首页快捷控制切换，或用 `--language en`、`zh-CN`、`zh-TW`、`auto` 启动。
- **订阅** — 在订阅页按 `a` 粘贴订阅链接：`Enter` 或 `[ 导入 ]` 下载并校验 YAML，再点 `s 保存` 按钮保存。`--subscriptions-file 文件` 导入 JSON 清单，`--import-only` 只更新不启动，`--profile 1` 启动时指定订阅；JavaScript 增强需要 Node.js 18+。
- **混合代理端口** — 默认 `127.0.0.1:7890`，首页快捷控制可查看和修改（`Enter` 或双击）；已保存端口被占用时自动改用下一个可用端口。`--mixed-port` 覆盖首次启动端口。
- **系统代理与 TUN** — 默认关闭，可从首页快捷控制或设置页开启。首次开启 TUN 在界面内输入系统密码，安装按工作区隔离的权限服务；`--tun-service install|status|uninstall` 可在终端直接管理。主机有多条默认路由时，内核按最低 metric 绑定出口网卡，显式出口接口始终优先。
- **后台服务** — 界面自管内核时按 `q` 选择**后台运行**或**退出并停止内核**；`--service install|start|status|stop|uninstall` 直接管理服务，已附加服务的界面按 `q` 只关闭界面。
- **工作区** — 默认为 `${XDG_STATE_HOME:-$HOME/.local/state}/clash-verge-tui`，可用 `--data-dir` 更换；配置与密钥保持私有权限（`0600`）。
- **按键** — `1`–`8` 切换页面，`↑/↓` 或 `j/k` 选择，`←/→` 或 `h/l` 切换区域，`Enter` 或双击执行，`/` 搜索，`:` 页面跳转，`?` 帮助，`t` 主题，`q` 退出。
- **诊断** — `--check` 不开界面检查应用、内核与配置；`--demo` 打开独立演示数据；`--snapshot home --output home.svg` 导出真实 Ratatui 缓冲区。

## 开发

```bash
# 从源码构建；Rust 1.88+（由 rust-toolchain.toml 固定）
cargo build --locked --release

# 运行源码构建；首次启动下载并校验固定内核
./target/release/clash-verge-tui

# 发布前检查
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

- `src/core_manager.rs`：固定内核版本、发行包发现、官方下载、双重 SHA-256 校验与自动修复。
- `src/workspace.rs`、`src/subscriptions.rs`：权威清单、订阅、配置增强与事务回滚。
- `src/core.rs`、`src/live.rs`：mihomo API、日志、重连与真实操作队列。
- `src/platform.rs`、`src/service.rs`：系统代理、TUN 与 systemd 用户服务。
- 更多：[功能对照](docs/FEATURES.md)、[版本维护](docs/RELEASING.md)、[验证记录](docs/VALIDATION.md)。

推送 `v*.*.*` 标签后，GitHub Actions 会构建两种 musl 架构并发布带 `install.sh` 的 GitHub Release。

## 许可与第三方组件

- TUI 源码：[MIT](LICENSE)
- 随包内核 [mihomo](https://github.com/MetaCubeX/mihomo/tree/v1.19.29) v1.19.29：GPL-3.0，许可原文见 [docs/LICENSE-GPL-3.0](docs/LICENSE-GPL-3.0)，发行清单记录对应上游源码
- Rust 依赖：MIT / Apache-2.0，主要有 [ratatui](https://github.com/ratatui/ratatui)、[crossterm](https://github.com/crossterm-rs/crossterm)、[tokio](https://github.com/tokio-rs/tokio)、[reqwest](https://github.com/seanmonstar/reqwest)、[serde](https://github.com/serde-rs/serde)，完整清单见 [第三方许可](docs/THIRD-PARTY-LICENSES.md)
- [Clash Verge Rev](https://github.com/clash-verge-rev/clash-verge-rev) 仅作为界面与功能参考（GPL-3.0）；可选 JavaScript 增强运行时 [Node.js](https://github.com/nodejs/node/blob/main/LICENSE) 采用 MIT
- 静态发行包附带 [musl 许可及版权声明](docs/LICENSE-MUSL)；随包 GeoSite 数据来自 [MetaCubeX/meta-rules-dat](https://github.com/MetaCubeX/meta-rules-dat)，采用 GPL-3.0
