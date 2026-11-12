# Lumen

[![Website](https://img.shields.io/badge/website-farvisun.github.io%2Fgnome--launcher-2f6fd0)](https://farvisun.github.io/gnome-launcher/)
[![Release](https://img.shields.io/github/v/release/farvisun/gnome-launcher)](https://github.com/farvisun/gnome-launcher/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

<p align="center">
  <img src="docs/assets/lumen-search-dark.png" alt="Lumen searching applications" width="640">
</p>

A native command launcher for **Ubuntu 26.04, GNOME 50, and Wayland**. Press **Super+Space** (the Windows/Cmd key + Space), type, and press Enter.

- Search and launch installed apps, including Flatpak and Snap desktop entries.
- Search indexed file/folder names and paths; open files, reveal them in Files, or copy paths.
- Calculate expressions such as `sqrt(144) * 3`, `200 * 15%`, or `2^8`. Enter copies the result.
- Favorite results and rank suggestions using local usage history.
- Install JavaScript/TypeScript extensions from a local package or HTTPS URL.
- Use native extension lists, text details, forms, clipboard actions, and persistent storage.

Lumen uses GTK4/libadwaita with a Rust core. A small GNOME Shell extension handles the shortcut, focus, and positioning. **File indexing reuses GNOME LocalSearch**; Lumen does not run another filesystem crawler. Search text is not stored or transmitted. Extension programs can use the network themselves.

## Build

Install Rust 1.92 or newer, Node.js 22 or newer, and the native headers:

```sh
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev \
  localsearch libtinysparql-3.0-0 python3-gi
cargo build --release --locked
npm ci --ignore-scripts
```

On this Ubuntu machine, the runtime libraries are present. An optional helper downloads and extracts **build headers only** into `.build-deps/`, without root:

```sh
python3 scripts/bootstrap-build.py
bash -c 'source scripts/dev-env.sh; cargo build --release --locked'
```

Keep `Cargo.lock` and `package-lock.json` for reproducible dependency resolution. The local-header helper requires the corresponding Ubuntu runtime libraries; it is not a portable sysroot.

## Install for your user

```sh
python3 scripts/install.py
```

This installs the binary, app entry, icon, D-Bus service, session autostart, and Shell integration. **Log out and back in once if GNOME has not discovered the new Shell extension.** On subsequent sessions Lumen stays resident, ready for the shortcut. No reboot is needed.

Super+Space is removed from keyboard-layout switching while the integration is enabled. Forward layout switching moves to **Super+Shift+Space**; the hardware layout key is preserved. Overview shortcuts remain available. Disable/uninstall restores only shortcut values Lumen still owns, preserving later user edits.

```sh
~/.local/bin/gnome-launcher --settings
~/.local/bin/gnome-launcher --quit
python3 scripts/uninstall.py
```

Uninstall keeps local preferences and installed extension data. To build a system package instead:

```sh
python3 scripts/package-deb.py
# Install the resulting dist/lumen-launcher_0.1.0_amd64.deb using your package manager.
gnome-extensions enable lumen@gnome-launcher.local
```

Avoid keeping both a system and per-user installation. Disable the Shell extension before removing a system package to restore its shortcut changes. The `.deb` targets GNOME 50 specifically.

## Keyboard

| Key | Action |
| --- | --- |
| Super+Space | Open or close the palette |
| Up / Down, Ctrl+N / Ctrl+P | Select a result |
| Page Up / Page Down | Jump five results |
| Enter | Open app/file, copy a calculation, or run an extension command |
| Ctrl+K | Actions for the selected result |
| Ctrl+, | Settings |
| Escape | Close actions, return from an extension, clear the query, or hide the palette |
| Backspace | Return from an extension when the query is empty |

Settings includes a configurable accelerator, history clearing, file-index status, and extension management. File coverage follows GNOME's indexed folders and exclusions. Open **Settings → Files → Open Search Settings** to change those folders. File contents are not searched in this release.

## Extensions

Extensions are **trusted local programs**, with the same filesystem/network privileges as your user. Review the package source before enabling. Capability declarations govern SDK host methods; they are not an operating-system sandbox. Third-party code runs outside GNOME Shell, with bounded protocol messages, cancellation, and a ten-second request timeout.

Build the included examples:

```sh
npm run bundle:examples
target/release/gnome-launcher pack dist/quick-links dist/quick-links.lumen-extension
target/release/gnome-launcher pack dist/scratchpad dist/scratchpad.lumen-extension
```

Open **Settings → Extensions**, choose a package, select Review, then Install & Enable. Quick Links demonstrates searchable lists and multiple actions; Scratchpad demonstrates forms, notifications, clipboard access, and storage. Packages are gzip tar archives with `extension.json` at their root. Installation never executes package scripts. Updates require explicitly reviewing and replacing an installed package.

For local development, load the project folder from Settings or run:

```sh
target/release/gnome-launcher dev examples/quick-links
# Reopen Settings to refresh developer registrations.
```

See [the extension SDK](sdk/README.md) for authoring, TypeScript bundling, and the protocol. There is no hosted marketplace or Raycast SDK compatibility in this release.

## Validate

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
npm test
glib-compile-schemas --strict shell/schemas
desktop-file-validate data/io.github.gnomelauncher.Launcher.desktop
```

With a running GNOME display and LocalSearch session:

```sh
GTK_IM_MODULE=gtk-im-context-simple LUMEN_SCREENSHOT=dist/screenshots \
  cargo test --locked -- --ignored --nocapture --test-threads=1
python3 tests/shell-smoke.py
```

The native test exercises searching, stale-query cancellation, settings, and extension views; `LUMEN_SCREENSHOT=DIR` saves GTK-only light and dark renders of several views into `DIR`. `GTK_IM_MODULE=gtk-im-context-simple` avoids a crash in GTK's Wayland input-method module when the test destroys windows. The Shell smoke test creates a private D-Bus session and headless Wayland compositor, verifies activation and shortcut migration/restoration, and leaves the live desktop settings alone. It requires GNOME's normal graphics/runtime dependencies. Details go to `dist/shell-smoke.log`.

Use a separate test data directory when validating destructive extension-management flows. Manual checks still needed on each supported desktop setup: real keyboard activation, multi-monitor placement, mixed scaling, screen readers, opening external apps/files, and package review/replace/remove.

## Diagnostics and performance

```sh
gnome-launcher doctor
gnome-launcher search 'report'
LUMEN_PROFILE=1 gnome-launcher --background
```

Profiling logs report application-side presentation and file-query times; they do not measure compositor-to-screen latency. Targets are warm activation within 150 ms and indexed results within 200 ms on the development machine. Queries are debounced for 60 ms, capped at 40 file results, and cancelled when superseded. The LocalSearch connection is reused across queries, and the app catalog is lowercased once rather than per keystroke. Extension subprocesses start on demand and are reaped after 60–90 seconds idle. A 30-second timer checks idle processes; extension-request polling exists only while a request is pending; file results are awaited without polling.

Validated on GNOME 50.1 / Ubuntu 26.04: the isolated compositor delivered Super+Space and reopened the palette in approximately 4 ms of application work (first presentation approximately 224 ms). Live LocalSearch queries over roughly 33,000 indexed files took 90–100 ms before debounce. The installed hidden process used approximately 46 MiB RSS / 13 MiB PSS and accumulated zero CPU ticks during a 1.2-second idle sample. These are development measurements, not guarantees for other machines.

Per-user installation logs: `$XDG_DATA_HOME/gnome-launcher.log` (normally `~/.local/share/gnome-launcher.log`). Preferences, usage counts, extension storage, and packages live under `$XDG_DATA_HOME/gnome-launcher/`. To select a specific Node executable, set `LUMEN_NODE` in the launcher's session environment.

The first version does not include clipboard history, snippets, window-management commands, full-text search, or cloud services.

## License

[MIT](LICENSE)
