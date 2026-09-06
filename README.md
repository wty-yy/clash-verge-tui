# Clash Verge TUI

[简体中文](README.zh-CN.md) | [Changelog](CHANGELOG.md)

A mihomo terminal client built with Rust and Ratatui, with an interface referencing Clash Verge Rev v2.5.2.

`v1.0.1` connects to an existing core or starts an isolated mihomo instance from subscriptions. Profiles and enhancements, network settings, system proxy, TUN, services, backups/WebDAV, page reachability checks, core maintenance, and diagnostics are connected. Demo mode retains the complete UI preview.

![Demo interface](docs/previews/home.png)

## Usage

Linux; Rust 1.88+; a UTF-8 terminal. Recommended size: 120 × 40. Minimum: 76 × 24. The UI is in Simplified Chinese. Managed mode requires an installed mihomo or verge-mihomo binary.

```bash
# Build and install
cargo build --locked --release
cargo install --locked --path .

# Demo mode is also the default without connection arguments
clash-verge-tui --demo

# Attach to a core with its external controller enabled; MIHOMO_SECRET is also supported
clash-verge-tui --connect http://127.0.0.1:9090 --secret-file /path/to/controller.secret

# Connection diagnostics: version, proxy entries, groups, rules, and connection counts
clash-verge-tui --connect http://127.0.0.1:9090 --secret-file /path/to/controller.secret --check
```

Create `.local/sources.json` and replace the example URL with your subscription address:

```json
[
  {"name": "Primary", "url": "https://example.com/subscription.yaml"}
]
```

```bash
# Import subscriptions and start an isolated core; use your own mihomo path
clash-verge-tui --core /usr/bin/verge-mihomo --subscriptions-file .local/sources.json --data-dir .local/live

# Reuse downloaded subscriptions; profile numbers start at 1
clash-verge-tui --core /usr/bin/verge-mihomo --data-dir .local/live --profile 1

# Verify that a subscription can load without opening the UI
clash-verge-tui --core /usr/bin/verge-mihomo --data-dir .local/live --profile 1 --check

# Download or update only; partial failure returns a nonzero status and retains successes
clash-verge-tui --import-only --subscriptions-file .local/sources.json --data-dir .local/live

# For proxy-only downloads, start a working core first and import another manifest in a second terminal
clash-verge-tui --import-only --subscriptions-file .local/extra-sources.json --subscription-proxy http://127.0.0.1:17897 --data-dir .local/live
```

Managed mode defaults to mixed proxy `127.0.0.1:17897` and controller `127.0.0.1:19097`; override them with `--mixed-port` / `--controller-port`. A controller secret is generated automatically. Subscription listener addresses, TUN, external controllers, and provider file paths are overridden for isolated operation. The system proxy is unchanged by default and can be enabled explicitly in Settings.

Without a background service, exiting `--core` stops the child it started. When a service is already running, `--core` attaches and UI exit leaves the service running. Switching from foreground to service can briefly restart the core. `--connect` neither manages the existing core's lifetime nor takes over its subscriptions. A subscription can specify a `proxy` field, or use `--subscription-proxy`; that download proxy must already be running.

The managed profile page supports `a/e/d` for create/edit/delete, `r` for remote updates, `R` for rereading the local manifest, `Enter` to apply, `v` for YAML editing, `i` for usage and expiration, and `[/]` for ordering. Forms accept a remote URL, local file, or YAML content. An interval of 0 disables scheduled updates.

Enhancements support ordered YAML overrides and JavaScript `main(config)`. Changes are validated by mihomo before application; failures preserve the previous configuration. JavaScript requires local Node.js 18+, with execution-time and memory limits, and is intended for user-written or trusted scripts. An empty workspace can start with `--core` in direct mode, then import profiles from the UI.

| Key | Action |
| --- | --- |
| `1`–`8` | Home, proxies, profiles, connections, rules, logs, unlock checks, settings |
| `←` / `→`, `Tab` / `Shift+Tab` | Switch home panels or page sections |
| `↑` / `↓`, `j` / `k` | Select a row; Vim keys are enabled by default, with `h/l` for horizontal navigation |
| `Enter` / double-click | Activate a row while preserving confirmation dialogs |
| `/`, `Esc` | Search, clear filtering, or cancel a dialog |
| `r`, `s`, `m` | Refresh / test / reread profiles per the toolbar, sort, change mode |
| `d` / `D` | Close selected / all connections on the connection page |
| `p` / `c` | Pause logs / clear the UI log buffer |
| `Ctrl+S`, `Ctrl+U` | Save a form, clear a field |
| `:`, `?`, `t` | Page palette, help, theme switch |
| `q` / `Ctrl+C` | Quit |

Sidebar items have large click targets and full-area highlighting; content lists use consecutive rows. A single click selects a row, and a double click within 400 ms acts as `Enter`. The home profile-management button first takes focus and opens on a later click while focused, without a time limit. In multiline forms, `Enter` inserts a newline and `Tab` changes fields; Vim letters remain text input.

## Services, TUN, and backups

```bash
# Install and start this data directory's systemd user service; installation enables login startup
clash-verge-tui --service install --core /usr/bin/verge-mihomo --data-dir .local/live
clash-verge-tui --service start --data-dir .local/live
clash-verge-tui --service status --data-dir .local/live

# Open the service's control interface
clash-verge-tui --core /usr/bin/verge-mihomo --data-dir .local/live

# Stop or uninstall the service
clash-verge-tui --service stop --data-dir .local/live
clash-verge-tui --service uninstall --data-dir .local/live

# An administrator can grant TUN access after the core is created; repeat after updates if necessary
sudo setcap cap_net_admin,cap_net_bind_service+ep .local/live/core/mihomo
```

Service names are derived from data directories for isolated workspaces. User services start after login without opening a terminal, so desktop silent-start is not a separate setting. Startup scripts run with the current user's permissions; use `CLASH_VERGE_SKIP_STARTUP=1` for recovery after a script failure.

Settings → Advanced → Backup and restore: `b` creates, `Enter` restores, `d` deletes, `←/→` switches local/WebDAV, `u` uploads, and `e` configures. The latest 10 local backups are retained. A configured backup password enables scrypt + AES-256-GCM encryption; otherwise files are private plaintext. WebDAV requires the user's own server and credentials and retains TLS verification.

## Implementation

- `src/platform.rs`, `src/service.rs`: GNOME proxy integration, restoration, user services, and process lifetime.
- `src/backup.rs`, `src/extras.rs`: encrypted backups, WebDAV, reachability, update checks, and log maintenance.
- `src/workspace.rs`: profile transactions, scheduled updates, enhancements, isolated validation, and workspace locking.
- `src/core.rs`: authenticated HTTP API, background communication, log streaming, and reconnection.
- `src/subscriptions.rs`: downloads, isolated configuration, and core process management.
- `src/live.rs`: live state mapping, command queues, confirmations, and UI preferences.
- `src/model.rs`, `src/app.rs`: models, demo fixtures, page state, and interaction.
- `src/ui.rs`, `src/settings.rs`: layouts, themes, forms, and mouse regions.
- `src/storage.rs`: demo state loading and atomic writes.
- [Feature mapping](docs/FEATURES.md), [agent guidelines](AGENTS.md), [validation](docs/VALIDATION.md).

Default directory: `${XDG_STATE_HOME:-$HOME/.local/state}/clash-verge-tui`. Only absolute `XDG_STATE_HOME` paths are accepted; `--data-dir` takes precedence.

| Path | Contents |
| --- | --- |
| `demo-state.json` | Demo state, compatible with earlier UI releases |
| `live-preferences.json` | Live UI preferences, excluding core connection data |
| `workspace-state.json` | Authoritative atomic manifest for profiles, active selection, and enhancements |
| `profiles/index.json`, `profiles/profile-N.yaml` | Private subscription index and original configurations |
| `profiles/active.json` | Applied profile index |
| `backups/`, `reports/` | Private backups, exported logs, and sanitized diagnostics |
| `core/config.yaml`, `core/controller.secret` | Isolated core configuration and access secret |
| `core/core.log` | Child process logs, which may contain subscription information |

These state files use Unix `0600` permissions and are not encrypted. Do not commit subscription URLs, configurations, or secrets; the project's `.local/` directory is ignored. Corrupt state files cause an error and remain intact. Concurrent writes to a shared data directory are not supported.

Live mode does not fabricate traffic or successful operations. Rates are calculated from cumulative byte differences and sample intervals; logs use their UTC reception time. Background requests do not block keyboard input. Actions are followed by core state reads, and disconnected views retain their last data with an explicit status.

System proxy integration supports GNOME, original-setting restoration, a guard, and generated or custom PAC scripts. TUN requires `CAP_NET_ADMIN` and has been verified in an isolated network namespace. Website checks report page reachability and exit-region information, not paid playback authorization; verification and login pages are marked separately.

Network settings are validated by mihomo before persistence. Controller address and secret changes take effect after restart; internal control uses a Unix socket, so external HTTP can be disabled. On Settings, `u` updates the selected core/WebUI or checks app versions, `g` updates GeoData, `o` opens directories/WebUI, and `x` exports logs or sanitized diagnostics. App updates provide version checks and source-install commands; core updates only replace the workspace copy.

## Development and versions

```bash
# Formatting, linting, and regression tests
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release

# Export demo snapshots without reading or saving user state
clash-verge-tui --snapshot proxies --output proxies.svg
clash-verge-tui --snapshot home --width 100 --height 30

# Regenerate SVG snapshots for all eight main pages
for page in home proxies profiles connections rules logs unlock settings; do
  target/release/clash-verge-tui --snapshot "$page" --output "docs/previews/$page.svg"
done
```

Versions start at `v0.1.0`. Commit subjects use `vVERSION: English summary`. The main branch is `master`, with annotated version tags; see [release maintenance](docs/RELEASING.md). README and CHANGELOG files are maintained in English and Chinese.

The reference checkout at `upstream/clash-verge-rev` is ignored by Git and pinned to `v2.5.2` / `28f2efc`. This is an independent implementation, not an official Clash Verge Rev project. License: [MIT](LICENSE). The previous GPL reference text is archived at [docs/LICENSE-GPL-3.0](docs/LICENSE-GPL-3.0).
