# Changelog

[简体中文](CHANGELOG.zh-CN.md)

## Unreleased

See the [feature mapping](docs/FEATURES.md) for planned work.

## v0.1.1 · 2026-09-06

### Fixed

- Resample traffic history across the available width; scale chart height and align axis labels when resizing.
- Use checkboxes consistently for settings toggles, proxy selection, active profiles, and rule states.
- Show `j/k` navigation hints in the footer while Vim navigation is enabled; hide them when disabled.
- Read the package version for the small-terminal notice.

## v0.1.0 · 2026-09-06

### Added

- Eight terminal pages: home, proxies, profiles, connections, rules, logs, unlock checks, and settings.
- Dark and light themes, accent colors, compact navigation, and terminal resizing.
- Keyboard and mouse navigation, filtering, page palette, and shortcut help.
- Profile and rule CRUD, enhancement editing and reordering, connection details and close confirmations.
- Forms for DNS, TUN, ports, external controllers, core options, appearance, backups, and related settings.
- Chinese text input, multiline editing, basic validation, isolated persistence, and local demo backup history, restore, and deletion.
- Text and SVG snapshots, UI previews, and a GitHub Actions check workflow.
- Rendering, form, interaction, and persistence regression tests; bilingual README and CHANGELOG files.

### Scope

- All network data and actions are local simulations; mihomo is not connected.
- Subscription downloads, enhancement execution, TUN permissions, system proxy, WebDAV, and core updates remain pending.
- Desktop tray and window features use terminal equivalents or are marked inapplicable in the feature mapping.
