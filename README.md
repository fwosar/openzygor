# OpenZygor

OpenZygor is an unofficial Linux desktop client for installing and updating Zygor guides for World of Warcraft.
It connects to Zygor's service with your account and installs the guides into your existing WoW folders.
The app uses Rust and Tauri 2 and runs without Wine.

OpenZygor has no affiliation with Zygor or the official Zygor client.
Zygor does not develop, endorse, or support this project.

The code is MIT-licensed and implements the protocol observed in Zygor Client 4.13.2.
This repository contains no Zygor application code, branding assets, or guide content.
You need a Zygor account to download guides, and your subscription and account permissions determine which guides you can access.

## Features

- Sign in with a Zygor username and password, refresh tokens, and optionally remember the session in the desktop keyring.
- List account entitlements, current guide releases, installed versions, and subscription status.
- Detect WoW installations in Bottles, Wine, Lutris, and standard Steam/Proton prefixes, or add folders manually.
- Manage multiple installations of Retail, Classic Era, Mists of Pandaria Classic, TBC Anniversary, and Forever.
- Select a game region, check for updates, install, update, repair, cancel downloads, and uninstall guides.
- Automatically update installed guides every 30 minutes while the app is open, when enabled.
- Keep the latest 100 activity entries across restarts, including dates, times, and errors.
- Preserve custom files during updates and keep a recovery copy of the previous addon.

OpenZygor focuses on login and guide management.
It does not implement Gold Guide auction trend downloads, cloud settings sync, news, bug-report uploads, tray integration, game launching, or client self-updates.
The available editions and releases come from Zygor's service.

## Run on Linux

Install Rust and the [Tauri Linux prerequisites](https://v2.tauri.app/start/prerequisites/#linux), then run the command below from the project directory.
On Arch Linux, the main runtime dependencies are `webkit2gtk-4.1`, `gtk3`, and `dbus`.
Remembered sessions also require an unlocked desktop Secret Service provider, such as GNOME Keyring or a compatible KWallet setup.
If it is unavailable, you can sign in for the current session.

```sh
cargo run --release
```

The app embeds its HTML, CSS, and JavaScript frontend.
You do not need Node.js, npm, or a development web server to build or run it.
The compiled executable is `target/release/openzygor`.

1. Open **Game folders** and review the detected installations.
2. Add any missing installation by selecting its game edition and folder.
3. Set your region in **Preferences**.
4. Sign in with your **Zygor username**, rather than an email address or Battle.net account.
5. Install or update the guides in **My guides**.
6. Type `/reload` in WoW, or restart the game, after installing an update.

Automatic detection is limited to common prefix locations.
For a Steam library on another disk or a custom Lutris prefix, add the folder manually.
The directory chooser accepts a WoW root, an edition folder such as `_retail_`, or that edition's `Interface/AddOns` directory.
OpenZygor verifies that the selected folder contains the expected game executable.

## Storage and recovery

OpenZygor saves settings in `$XDG_CONFIG_HOME/openzygor/settings.json`, normally `~/.config/openzygor/settings.json`.
Set `OPENZYGOR_CONFIG_DIR` to use a different configuration directory.
The settings file stores your username, preferences, and installation paths.
It does not store a password or token.
OpenZygor stores remembered tokens under the desktop keyring service `io.github.openzygor.client`.
Signing out removes the token.
The app reports any keyring failure.

The app saves activity separately in `activity.json` beside the settings file.
It saves each entry as it happens, and **Clear activity** removes the saved history.
History writes are atomic, with a lock to coordinate writes from multiple app instances.
If history cannot be read or saved, the app displays a warning and keeps guide operations available.

Each installation has a hidden working directory next to the addon:

```text
Interface/AddOns/
  ZygorGuidesViewer/
  .openzygor-ZygorGuidesViewer/
    lock
    previous/
    uninstalled/
```

Other editions use their own package directory names.
`previous` contains the addon from the last successful update.
OpenZygor creates `uninstalled` when you explicitly uninstall an addon.
It preserves WoW's `WTF` settings and unrelated addons.

To restore a copy manually, close OpenZygor and WoW, move the current addon somewhere safe if it exists, and move `previous` or `uninstalled` back to the corresponding addon directory.
If an interrupted update leaves the addon directory missing and `previous` exists, OpenZygor restores it automatically when preparing the next installation.
The current addon stays in place throughout downloading and staging.
Changes to its files during a download cause the update to stop before replacement.

Updates require enough free space for a staging copy and a previous version, in addition to the installed addon.
OpenZygor rejects symlinks inside the addon or in its writable directory path.
It also rejects archive traversal, links, writes to unrelated addons, malformed deletion metadata, and oversized archives.
Cancellation applies during preparation, download, and extraction; the short directory replacement finishes once it has started.

## Build packages

Install the Tauri CLI if you want distributable packages:

```sh
cargo install tauri-cli --locked
cargo tauri build
```

The bundle configuration includes AppImage, deb, and rpm targets.
These package builds have not yet been verified.
For distribution across Linux releases, build against the oldest supported system libraries as described in the [Tauri AppImage documentation](https://v2.tauri.app/distribute/appimage/).

## Tests

```sh
cargo test --no-default-features
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
node --check ui/app.js
node --test scripts/ui_workflow.test.cjs
```

The Rust workflow tests exercise synthetic login responses, password encoding, token validation, package listing, checksum uploads, ZIP downloads, licence installation, repair, uninstall, recovery, cancellation, concurrent file edits, and archive path checks.
Two HTTP tests bind loopback sockets; run them outside a sandbox that prohibits local networking.
All guide files and account credentials in tests are synthetic.
The JavaScript workflow tests exercise guide ordering, saved preferences, periodic updates, and activity storage errors through the frontend's event handlers, with a simulated native backend and timer.
The Rust activity tests cover persistence, retention limits, concurrent writes, file permissions, and damaged history.

The native GUI smoke test drives the actual WebKitGTK window, Rust commands, fixture HTTP server, and filesystem installation:

```sh
cargo build --features e2e
python3 scripts/gui_smoke.py
python3 scripts/activity_smoke.py
```

It requires a running Linux desktop session and `WebKitWebDriver`.
The Python scripts use only the standard library and save screenshots under `test-results/`.
They use temporary game folders and configuration, leave the desktop keyring alone, and do not contact Zygor.
The activity test closes and relaunches the native app to verify that entries persist and cleared history stays cleared.
The `e2e` feature accepts only a loopback fixture endpoint and requires `OPENZYGOR_TEST_ENDPOINT`; the smoke-test script sets it.
Rebuild normally before using your real account:

```sh
cargo build --release
./target/release/openzygor
```

Protocol checks used the installed client bundles and selected fields from existing logs, with account secrets excluded.
The native GUI workflows and activity persistence have passed tests against local fixtures.
Live account authentication, paid guide downloads, and desktop keyring behavior still need testing with a real account.

See [docs/protocol.md](docs/protocol.md) for the observed request formats, package mapping, and implementation limits.
