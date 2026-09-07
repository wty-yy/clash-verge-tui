# Changelog

[简体中文](CHANGELOG.zh-CN.md)

## Unreleased

See the [feature mapping](docs/FEATURES.md) for planned work.

## v1.2.0 · 2026-09-07

### Profile file link import

- Add a profile file URL field and a right-side **Import** button above the YAML editor in profile create/edit forms.
- Download asynchronously by mouse click or Enter on the URL field while keeping the TUI responsive and showing progress.
- Fill the editor only after URL, size, UTF-8, and Clash YAML validation; never save or replace the active profile automatically.
- Keep errors in the current dialog and discard results after the link changes, the form closes, or a request becomes stale.
- Verify complete import with the supplied private test link: 35 nodes and 3 groups, without adding the link or source configuration to the repository.

## v1.1.0 · 2026-09-07

### Managed core and Linux distribution

- Start the live managed workspace by default, retain `--demo` for explicit UI previews, and remove external-controller attachment from the public CLI.
- Pin Mihomo v1.19.29 to match the Clash Verge Rev v2.5.2 baseline on x86_64/aarch64; discover bundled cores or download the official release when absent.
- Verify both the upstream archive and extracted binary SHA-256, repairing damaged, replaced, or independently upgraded workspace cores to the application-pinned version.
- Create the authoritative private workspace manifest on first launch and continue storing profiles, enhancements, runtime configuration, secrets, and UI preferences locally.
- Remove user-provided core paths from systemd units so the daemon and foreground TUI share the same managed core and workspace.
- Add Linux bundles, checksum files, an x86_64/aarch64 release workflow, and a `curl | sh` one-line installer.
- Replace in-app core self-upgrades with the application release policy while retaining independent GeoData and WebUI updates.

## v1.0.1 · 2026-09-07

### Fixed

- Use a keyed reverse sort for remote backup history, satisfying the newer Clippy version used by CI.

## v1.0.0 · 2026-09-07

### Linux feature release

- Connect GNOME manual/PAC proxy, guard and restoration, TUN, DNS, LAN, ports, and traffic tunnels.
- Add systemd user services, login startup, foreground attachment, and core crash recovery.
- Add encrypted local backups, 10-file retention, WebDAV upload/list/restore/delete, and corrupt-file handling.
- Add reachability/region checks, GeoData, workspace-only core upgrades, WebUI, version checks, log rotation, and diagnostics.
- Use private Unix sockets; controller address/secret changes take effect after restart without replacing the system core binary.
- Improve large-file editing, lightweight polling, automatic delay checks, and rollback; see the acceptance scope and limitations.

## v0.3.0 · 2026-09-07

### Added

- Live profile CRUD, ordering, local file/YAML import, usage details, and scheduled refresh.
- Ordered YAML / JavaScript enhancements with core validation before application and rollback on failure.
- Empty-workspace direct startup, atomic manifests, interprocess locks, isolated validation, and Linux parent-death cleanup.
- Preserve unsaved forms while previous operations are pending.

## v0.2.0 · 2026-09-07

### Added

- Attach to an existing HTTP(S) controller with `--connect`, secret-file or `MIHOMO_SECRET` authentication, and headless `--check` diagnostics.
- Start an isolated local mihomo with `--core`, default loopback ports 17897 / 19097, and child-process cleanup on exit.
- JSON subscription manifests, Clash YAML validation, original-config caching, partial-import handling, and explicit download proxies.
- Live group membership, manual selection, group delay tests, modes, connection details and closing, rule toggles, and provider updates.
- Actual rates, totals, memory, log streaming, and reconnection, with background requests independent of terminal input.
- Reread cached profiles and switch configurations in managed mode; persist live UI preferences separately from demo data.

### Improved

- Explain unsupported live operations instead of simulating success; redraw on input and data changes for larger rule lists.
- Remove authentication fields from runtime JSON; store subscription and runtime files privately and exclude test credentials from Git.
- Add regressions for authentication, errors, URL encoding, proxy downloads, reconnection, membership, and live actions; verify two supplied subscriptions in an isolated core.

### Scope

- Clash YAML is supported. Base64 / URI subscriptions, scheduled updates, enhancements, system proxy, TUN management, WebDAV, and unlock detection remain pending.
- `--core` owns a child process. Use `--connect` with an independent service when the core should remain running after UI exit.

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
