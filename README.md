<div align="center">
  <h1>Clash Verge TUI</h1>
  <p><strong>A Linux terminal proxy client built with Rust, Ratatui, and mihomo</strong></p>
  <p><strong>🌎 English</strong>&nbsp;&nbsp;·&nbsp;&nbsp;<a href="README.zh-CN.md">🇨🇳 中文</a></p>
  <p>
    <img alt="Platform: Linux only" src="https://img.shields.io/badge/platform-Linux%20only-FCC624?logo=linux&logoColor=black">
    <a href="https://github.com/wty-yy/clash-verge-tui/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/wty-yy/clash-verge-tui?label=release"></a>
  </p>
</div>

Clash Verge TUI is a **Linux-only** (x86_64/aarch64) terminal client for a self-managed mihomo core that follows the [Clash Verge Rev](https://github.com/clash-verge-rev/clash-verge-rev) layout, so the desktop app feels familiar from the first screen.

## Features

- **Familiar Clash Verge interface** — the same Home / Proxies / Profiles / Connections / Rules / Logs / Unlock checks / Settings pages
- **System proxy and TUN** — mixed proxy port plus TUN mode; the first TUN enable asks for the system password inside the TUI
- **Runs in the background** — quit the TUI and keep the core and system proxy running through a systemd user service, freeing the terminal
- **Self-managed core** — pinned mihomo v1.19.29 and a pinned `GeoSite.dat` snapshot; static musl builds with no runtime dependencies
- **Profiles and enhancements** — link import, YAML / JavaScript enhancement chains, scheduled updates
- **Everyday operations** — live traffic and sessions, rule toggles, latency tests, unlock checks, encrypted backups with WebDAV
- **Terminal-native** — Simplified Chinese, Traditional Chinese, and English; Vim keys, mouse, dark/light themes, and a layout that stays readable at any zoom level

![English TUI demo](docs/previews/demo-en.gif)

Created for personal use with development assistance from ChatGPT. The interface and feature mapping follow [Clash Verge Rev](https://github.com/clash-verge-rev/clash-verge-rev) v2.5.2. This is an unofficial project, independently developed and unaffiliated with the Clash Verge / Clash Verge Rev teams.

## Install

```bash
# Install the latest release to ~/.local/bin with the pinned mihomo v1.19.29
curl -fsSL https://github.com/wty-yy/clash-verge-tui/releases/latest/download/install.sh | sh

# Use the project Cloudflare mirror (gh.wty-yy.top) when GitHub is unreachable
curl -fsSL https://gh.wty-yy.top/install.sh | sh -s -- --source proxy

# Launch the live TUI; the first launch creates the managed workspace
clash-verge-tui
```

The installer verifies the release against the published SHA-256 checksum and also works with older curl versions such as Ubuntu 20.04’s 7.68. `--source proxy` uses the project-maintained mirror `gh.wty-yy.top` (a Cloudflare Worker, see [deploy/gh-mirror](deploy/gh-mirror/README.md)) for both `install.sh` and the release archives, and `--github-proxy https://gh-proxy.com` switches to that community prefix. Add `~/.local/bin` to `PATH` if needed, and set `CLASH_VERGE_TUI_VERSION` to install a specific published version.

## Usage

- **Interface language** — follows the system by default; switch in Home quick controls or start with `--language en`, `zh-CN`, `zh-TW`, or `auto`.
- **Profiles** — press `a` on Profiles and paste a subscription URL: `Enter` or `[ Import ]` downloads and validates the YAML, then save with the `s Save` button. `--subscriptions-file FILE` imports a JSON list, `--import-only` updates without launching, and `--profile 1` selects a profile at startup. JavaScript enhancements need Node.js 18+.
- **Mixed proxy port** — defaults to `127.0.0.1:7890`, shown and editable in Home quick controls (`Enter` or double-click); a busy saved port automatically moves to the next free one. `--mixed-port` overrides the first-start value.
- **System proxy and TUN** — both are off by default and enabled from Home quick controls or Settings. The first TUN enable asks for the system password inside the TUI and installs a workspace-scoped permission service; `--tun-service install|status|uninstall` works from a terminal. When several default routes exist, the core pins the lowest-metric egress interface, and an explicit outbound interface always wins.
- **Background service** — while the TUI owns the core, `q` offers **Keep in background** or **Quit and stop the core**; `--service install|start|status|stop|uninstall` manages it directly, and `q` on an attached TUI only closes the interface.
- **Workspace** — defaults to `${XDG_STATE_HOME:-$HOME/.local/state}/clash-verge-tui`; use `--data-dir` for another location. Configuration and secrets stay private (`0600`).
- **Keys** — `1`–`8` pages, `↑/↓` or `j/k` select, `←/→` or `h/l` sections, `Enter` or double-click activate, `/` search, `:` page palette, `?` help, `t` theme, `q` quit.
- **Diagnostics** — `--check` validates the app, core, and configuration without opening the UI; `--demo` opens isolated demo data; `--snapshot home --output home.svg` exports the real Ratatui buffer.

## Development

```bash
# Build from source with Rust 1.88+ (pinned by rust-toolchain.toml)
cargo build --locked --release

# Run the source build; the first launch downloads and verifies the pinned core
./target/release/clash-verge-tui

# Release checks
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

- `src/core_manager.rs`: pinned core versions, bundled-core discovery, official downloads, dual SHA-256 checks, and automatic repair.
- `src/workspace.rs`, `src/subscriptions.rs`: authoritative manifests, profiles, enhancements, and transactional rollback.
- `src/core.rs`, `src/live.rs`: mihomo API, logs, reconnection, and live action queues.
- `src/platform.rs`, `src/service.rs`: system proxy, TUN, and systemd user services.
- More: [feature mapping](docs/FEATURES.md), [release maintenance](docs/RELEASING.md), [validation](docs/VALIDATION.md).

Pushing a `v*.*.*` tag makes GitHub Actions build both musl architectures and publish a GitHub Release with `install.sh`.

## License and third-party components

- TUI source: [MIT](LICENSE)
- Bundled core [mihomo](https://github.com/MetaCubeX/mihomo/tree/v1.19.29) v1.19.29: GPL-3.0, full text in [docs/LICENSE-GPL-3.0](docs/LICENSE-GPL-3.0), upstream source recorded in the release manifest
- Rust dependencies: MIT / Apache-2.0, key components are [ratatui](https://github.com/ratatui/ratatui), [crossterm](https://github.com/crossterm-rs/crossterm), [tokio](https://github.com/tokio-rs/tokio), [reqwest](https://github.com/seanmonstar/reqwest), and [serde](https://github.com/serde-rs/serde); complete inventory in [third-party licenses](docs/THIRD-PARTY-LICENSES.md)
- [Clash Verge Rev](https://github.com/clash-verge-rev/clash-verge-rev) is the interface and feature reference (GPL-3.0); the optional JavaScript enhancement runtime [Node.js](https://github.com/nodejs/node/blob/main/LICENSE) uses MIT
- Static release bundles include the [musl license and copyright notices](docs/LICENSE-MUSL); bundled GeoSite data comes from [MetaCubeX/meta-rules-dat](https://github.com/MetaCubeX/meta-rules-dat) under GPL-3.0
