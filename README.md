# Clash Verge TUI

[简体中文](README.zh-CN.md) | [Changelog](CHANGELOG.md)

A Clash Verge-style terminal interface built with Rust and Ratatui, referencing Clash Verge Rev v2.5.2.

`v0.1.1` is a UI preview. All eight main pages, secondary forms, and local interactions are available. Mihomo, subscription downloads, system proxy, and TUN are not connected. All network data is simulated.

![Home preview](docs/previews/home.png)

## Usage

Linux; Rust 1.88+; a UTF-8 terminal. Recommended size: 120 × 40. Minimum: 76 × 24. The interface is in Simplified Chinese.

```bash
# Build and open the terminal interface
cargo run --locked --release -- --demo

# Install into ~/.cargo/bin
cargo install --locked --path .
clash-verge-tui

# Use a separate demo state directory
clash-verge-tui --data-dir .local/demo

# Show the version and options
clash-verge-tui --version
clash-verge-tui --help

# Export a page snapshot without reading or saving user state
clash-verge-tui --snapshot proxies --output proxies.svg
clash-verge-tui --snapshot home --width 100 --height 30
```

| Key | Action |
| --- | --- |
| `1`–`8` | Home, proxies, profiles, connections, rules, logs, unlock checks, settings |
| `Tab` / `Shift+Tab` | Switch page sections |
| `↑` / `↓`, `j` / `k` | Select a row; `Enter` activates it |
| `/`, `Esc` | Search, clear filtering, or cancel a dialog |
| `a` / `e` / `d` | Add, edit, delete; see the page toolbar |
| `r`, `s`, `m` | Simulate refresh, sort, change proxy mode |
| `v`, `[` / `]` | Edit profile YAML, reorder profiles or enhancements |
| `b` / `R` | Create a backup or restore the latest one on Settings |
| `Ctrl+S` | Save a form; `Ctrl+U` clears a field |
| `:`, `?`, `t` | Page palette, help, theme switch |
| `q` / `Ctrl+C` | Quit |

The footer shows `j/k` navigation hints when Vim keys are enabled (the default). Selection and toggles use `[✓]` / `[ ]`. The traffic chart scales with the terminal while keeping the same history interval.

Mouse clicks support navigation, sections, toolbar buttons, and forms. Click a table row to select it, then press `Enter`. In multiline fields, `Enter` inserts a newline and `Tab` changes fields. Use `Shift` + mouse for native terminal text selection.

## Implementation

- `src/model.rs`: types and deterministic demo fixtures.
- `src/app.rs`: page state, input, validation, and demo workflows.
- `src/ui.rs`: layouts, themes, tables, charts, dialogs, and mouse regions.
- `src/settings.rs`: system, core, appearance, and advanced forms.
- `src/storage.rs`: isolated state loading and atomic writes.
- `tests/workflows.rs`: rendering and workflow regression tests.
- [Feature mapping](docs/FEATURES.md): completed UI and pending core capabilities.

Default state file: `${XDG_STATE_HOME:-$HOME/.local/state}/clash-verge-tui/demo-state.json`. Only absolute `XDG_STATE_HOME` paths are accepted. `--data-dir` takes precedence.

Changes are saved immediately; unsubmitted form edits are discarded. State files use Unix `0600` permissions and are not encrypted. Corrupt or unsupported files cause an error and remain intact; use a new `--data-dir` to start separately. Concurrent writes by multiple instances sharing a directory are not supported.

Demo backups live in the same state file, with up to 10 snapshots containing settings, profiles, enhancements, rules, and the active profile. `R` restores the latest snapshot. Settings → Advanced → Backup and restore provides history, selection, restore, and deletion. WebDAV has a configuration form but does not synchronize.

Theme, accent, compact navigation, traffic chart, memory display, mouse input, Vim keys, start page, and refresh interval affect the terminal UI. Other network and system settings only store demo values. YAML and JavaScript support text editing without execution or full syntax validation.

## Development and versions

```bash
# Formatting, linting, and regression tests
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked

# Release build
cargo build --locked --release

# Regenerate SVG snapshots for all eight main pages
for page in home proxies profiles connections rules logs unlock settings; do
  target/release/clash-verge-tui --snapshot "$page" --output "docs/previews/$page.svg"
done
```

Versions start at `v0.1.0`, using semantic versioning and annotated Git tags. See [release maintenance](docs/RELEASING.md). README and CHANGELOG files are maintained in English and Chinese.

The reference checkout at `upstream/clash-verge-rev` is ignored by Git and pinned to `v2.5.2` / `28f2efc`. This is an independent terminal implementation, not an official Clash Verge Rev project. License: [GPL-3.0-only](LICENSE).
