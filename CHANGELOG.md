# Changelog

[简体中文](CHANGELOG.zh-CN.md)

## Unreleased

See the [feature mapping](docs/FEATURES.md) for planned work.

## v0.1.5 · 2026-09-07

### Repository

- Prepare the GitHub repository: retain its MIT LICENSE and archive the previous GPL text at `docs/LICENSE-GPL-3.0`.
- Standardize commit subjects as `vVERSION: English summary` and align the branch with remote `master`.
- Add root `AGENTS.md` covering interaction requirements, project design, and collaboration conventions.

### Improved

- Remove blank rows between table entries and headers on pages 2–8; reduce excess tab, toolbar, and detail spacing.
- Compact backup history, the page palette, and settings forms while preserving large sidebar click targets.
- Align mouse hit regions and scrolling offsets with the compact layout for accurate selection and double-click activation.

## v0.1.4 · 2026-09-06

### Improved

- Enlarge sidebar items to three-row click targets with outlined, highlighted, bold selection styling.
- Use two-row items in short terminals to keep all eight destinations visible; show footer status only when it fits below navigation.

## v0.1.3 · 2026-09-06

### Fixed

- Focus the home profile-management button on the first click and activate it on a subsequent click, without a double-click time limit.
- Require selection again after returning to the left panel; preserve keyboard Enter behavior.

## v0.1.2 · 2026-09-06

### Added

- Focus and highlight either home panel with horizontal navigation; press Enter on the profile card to open profiles, including in small terminals.
- Show horizontal navigation hints for proxy groups, log levels, and settings categories; add Vim `h/l` navigation without intercepting form text input.
- Double-click rows, home controls, settings entries, and backup history to perform the same action as `Enter`; single clicks only select.
- Match clicks near the same item within 400 ms; reset detection after keyboard input, dragging, scrolling, resizing, or navigation.
- Preserve single-click buttons and backup restore confirmations.

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
