# Clash Verge TUI

[简体中文](README.zh-CN.md) | [Changelog](CHANGELOG.md)

A Linux terminal proxy client built with Rust, Ratatui, and mihomo. Its interface and feature mapping follow Clash Verge Rev v2.5.2.

`v1.3.2` starts a self-managed workspace and the application-pinned mihomo v1.19.29 by default. Release archives contain both the TUI and the core. Home quick controls show and edit the current mixed proxy port, which defaults to `127.0.0.1:7890`.

![Interface preview](docs/previews/home.png)

## Install and run

Linux x86_64 and aarch64 are supported. Use a UTF-8 terminal; `120 × 40` is recommended and `76 × 24` is the minimum.

```bash
# Install the latest release to ~/.local/bin with its pinned mihomo v1.19.29
curl -fsSL https://github.com/wty-yy/clash-verge-tui/releases/latest/download/install.sh | sh

# Add ~/.local/bin to PATH once if necessary
export PATH="$HOME/.local/bin:$PATH"

# Open the live TUI and create the managed workspace on first launch
clash-verge-tui

# Check the application, core, and initial configuration without opening the UI
clash-verge-tui --check

# Open the isolated demo without starting mihomo
clash-verge-tui --demo
```

The installer downloads the application/core bundle from the current GitHub Release and checks the archive SHA-256 before installing:

| Path | Contents |
| --- | --- |
| `~/.local/bin/clash-verge-tui` | TUI executable |
| `~/.local/lib/clash-verge-tui/core/v1.19.29/mihomo` | Versioned core paired with the current application release |
| `~/.local/lib/clash-verge-tui/release.json` | Application, core, architecture, and upstream checksum metadata |
| `~/.local/lib/clash-verge-tui/MIHOMO-LICENSE` | GPL-3.0 license text for the bundled Mihomo core |

Source builds work as well. If no bundled core is found, the program downloads official Mihomo v1.19.29 into `${XDG_DATA_HOME:-$HOME/.local/share}/clash-verge-tui/core/`, verifies both the archive and extracted binary, and copies it into the active workspace. A damaged, replaced, or independently upgraded workspace core is restored to the application-pinned version on the next launch.

```bash
# Build from source with Rust 1.88+
cargo build --locked --release

# Run the source build; the first launch installs the core automatically
./target/release/clash-verge-tui
```

## Profiles and workspace

Press `a Link import` on Profiles to open the form. Its first row is always **Profile file URL + [ Import ]**, immediately followed by the YAML editor, with the URL focused. Enter or click the button to download and validate the complete Clash YAML asynchronously. When the name is empty, a successful import fills it from the profile title, attachment filename, configuration name, or source domain without replacing a name entered by the user. The profile is saved only after `Ctrl+S`. Local files, direct YAML editing, and private JSON manifests are also supported:

![Profile link import](docs/previews/profile-import.svg)

```json
[
  {"name": "Primary", "url": "https://example.com/subscription.yaml"}
]
```

```bash
# Import profiles and start the managed core
clash-verge-tui --subscriptions-file ~/.config/clash-verge-tui/sources.json

# Download or update only; partial failure is nonzero and retains successful entries
clash-verge-tui --import-only --subscriptions-file ~/.config/clash-verge-tui/sources.json

# Explicitly use an already-running proxy when a profile requires one for download
clash-verge-tui --import-only --subscriptions-file ~/.config/clash-verge-tui/sources.json \
  --subscription-proxy http://127.0.0.1:7890

# Select a downloaded profile at startup; numbering starts at 1
clash-verge-tui --profile 1
```

The mixed proxy defaults to `127.0.0.1:7890`; internal control uses a private Unix socket. Home → Quick controls shows the active mixed port; press `Enter` or double-click to edit it. `--mixed-port` can also override the first-start port. A controller secret is generated automatically. Listener addresses, TUN, external controller settings, and provider paths from subscriptions are replaced by workspace-owned settings. System proxy is off by default and must be enabled explicitly in Settings.

The Profiles page supports CRUD, ordering, remote updates, usage and expiry details, YAML editing, and scheduled refresh. Enhancements apply ordered YAML overrides and JavaScript `main(config)` functions. A separate mihomo process validates each composed configuration before it replaces the running one. JavaScript enhancements require Node.js 18+.

The default workspace is `${XDG_STATE_HOME:-$HOME/.local/state}/clash-verge-tui`; use `--data-dir` for another location. The program owns these private files:

| Path | Contents |
| --- | --- |
| `workspace-state.json` | Atomic source of truth for profiles, selection, enhancements, and runtime settings |
| `profiles/` | Original profile configurations, index, and active selection |
| `live-preferences.json` | Theme, layout, Vim, and refresh preferences |
| `core/mihomo`, `core/managed-core.json` | Workspace core copy and application/core version relationship |
| `core/config.yaml`, `core/controller.secret` | Composed runtime configuration and random controller secret |
| `backups/`, `reports/` | Backups, exported logs, and sanitized diagnostics |

Configuration and secret files use Unix mode `0600`; the core uses `0700`. Corrupt files cause an error and remain intact. A workspace lock prevents concurrent writes. Never commit subscription URLs, node credentials, or secrets.

## Service, TUN, and controls

```bash
# Install and enable a systemd user service for the current workspace
clash-verge-tui --service install
clash-verge-tui --service start
clash-verge-tui --service status

# Open the same TUI while the service is active; closing it leaves the service running
clash-verge-tui

# Stop or uninstall the service while retaining user configuration
clash-verge-tui --service stop
clash-verge-tui --service uninstall

# The first TUN enable prompts for the system password and installs the helper service
# It can also be installed or inspected from a normal terminal
clash-verge-tui --tun-service install
clash-verge-tui --tun-service status

# Remove the capability watcher when TUN is no longer needed
clash-verge-tui --tun-service uninstall
```

| Key | Action |
| --- | --- |
| `1`–`8` | Home, proxies, profiles, connections, rules, logs, unlock checks, settings |
| `←/→`, `h/l`, `Tab` | Switch home panels or page sections |
| `↑/↓`, `j/k` | Select a row; Vim navigation is enabled by default |
| `Enter` / double-click | Activate a row while preserving confirmation flows |
| `/`, `Esc` | Search, clear a filter, or cancel a dialog |
| `r`, `s`, `m` | Refresh, test, reread profiles, sort, or change mode as shown by the page |
| `d` / `D` | Close the selected / all connections |
| `p` / `c` | Pause logs / clear the UI log buffer |
| `Ctrl+S`, `Ctrl+U` | Save a form / clear a field |
| `s` | Save and immediately apply the Home mixed-port form |
| `:`, `?`, `t` | Page palette, help, theme switch |
| `q` / `Ctrl+C` | Quit |

A single click selects an ordinary row. A second click on the same row within 400 ms acts as `Enter`. The Home profile-management button takes focus on the first click and opens on a later click. `Enter` inserts a newline in multiline forms, where Vim letters remain normal text.

System proxy integration supports GNOME manual/PAC modes, restoration, and a guard. The first TUN enable opens a masked password form inside the TUI. The password is sent only to `sudo -S` over standard input and never enters arguments, configuration, or logs. Authorization installs a systemd path service scoped to the user and workspace; it verifies the official core and maintains only `CAP_NET_ADMIN` / `CAP_NET_BIND_SERVICE`. Debian/Ubuntu needs `sudo`, systemd, and `libcap2-bin`. Backups support retention, optional encryption, and WebDAV.

![TUN system password form](docs/previews/tun-password.svg)

## Implementation and releases

- `src/core_manager.rs`: pinned versions, bundled-core discovery, official downloads, dual SHA-256 checks, and automatic repair.
- `src/workspace.rs`, `src/subscriptions.rs`: authoritative manifests, profiles, enhancements, core copies, and transactional rollback.
- `src/core.rs`, `src/live.rs`: mihomo API, logs, reconnection, live state, and action queues.
- `src/platform.rs`, `src/service.rs`: GNOME proxy integration, TUN, and systemd user services.
- `scripts/install.sh`, `scripts/package-linux.sh`: one-line installation and Linux release archives.
- [Feature mapping](docs/FEATURES.md), [agent guidelines](AGENTS.md), [validation](docs/VALIDATION.md).

```bash
# Release checks
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release

# Export an actual Ratatui buffer without reading user state
clash-verge-tui --snapshot home --output home.svg
```

Pushing a `v*.*.*` tag makes GitHub Actions build Linux x86_64/aarch64 binaries, download and verify the pinned official Mihomo assets, create bundles and checksum files, and publish them with `install.sh` as a GitHub Release. See [release maintenance](docs/RELEASING.md) for version and commit rules.

The interface reference is pinned to Clash Verge Rev `v2.5.2` / `28f2efc`; the core is pinned to [Mihomo v1.19.29](https://github.com/MetaCubeX/mihomo/releases/tag/v1.19.29). This is an independent implementation, not an official Clash Verge Rev project. The TUI source is [MIT](LICENSE); bundled Mihomo is GPL-3.0, with its license at [docs/LICENSE-GPL-3.0](docs/LICENSE-GPL-3.0) and corresponding upstream source recorded in the release manifest.
