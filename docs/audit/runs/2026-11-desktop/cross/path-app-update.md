# Cross-module flow: path-app-update

An app update from the release workflow writing its manifest, through the check, the banner, the download and verification, to installing and restarting. Nothing was built or run; the only outside checks were reading the execution level in the built installer's manifest and one request to a GitHub release asset.

## Files read

.github/workflows/release.yml
scripts/package.mjs
scripts/release/manifest.mjs
scripts/release/updater-key.mjs
crates/updates/src/manifest.rs
crates/updates/src/release.rs
crates/core/src/manager/app_update.rs
crates/core/src/manager/mod.rs (config, `shutdown`, in part)
crates/core/src/headers.rs (`download_options`, in part)
crates/engine/src/target.rs
crates/engine/src/fetch.rs (`Fetcher`, `get`, in part)
crates/shell/src/updates.rs
crates/shell/src/appimage.rs
crates/shell/src/launch.rs
crates/shell/src/quit.rs
crates/shell/src/host.rs
crates/shell/src/api.rs (update calls only)
crates/shell/src/autostart.rs (`program` only)
crates/server/src/lib.rs (`ShowWindow` only)
crates/desktop/src/app_update.rs
crates/desktop/src/backend.rs
crates/desktop/src/lifecycle.rs
crates/desktop/src/main.rs
crates/desktop/src/main_window/notices_bar.rs
crates/desktop/src/main_window/settings/about.rs
crates/desktop/src/main_window/settings/general.rs (update parts)
crates/desktop/src/main_window/sidebar.rs (`update_waiting` only)
crates/desktop/src/main_window/mod.rs (`bring_up` only)
crates/desktop/packaging/windows/installer.iss
docs/DECISIONS.md (D106, D110, D111, D113)

Left out: `scripts/release/manifest.test.mjs`, `crates/updates/samples/update.json` and the engine's download loop, which chunks 12 and the engine chunks cover and where no mismatch between layers was suspected.

Checked and consistent: the file names `scripts/package.mjs` writes and `manifest.mjs` accepts, for every kind and for a pre-release version; the key format between `updater-key.mjs`, `manifest.mjs` and `Manifest::verified`; the tag check against `PRODUCT_VERSION`; the pre-release flag between the workflow and `newest`; a GitHub `browser_download_url` still redirects with the `Accept: application/vnd.github+json` header `fetch_small` sends (302, tested once).

## Findings

### Xpath-app-update-1 [P1] The app never looks for updates: no build sets where its releases are

- Where: `crates/desktop/src/backend.rs`, `config`; `crates/core/src/manager/mod.rs`, `ManagerConfig::with_official_app_releases`; `crates/core/src/manager/app_update.rs`, `check_app_update`
- Category: broken feature
- Failure: any release build with a real update key. The check a minute after start, the daily check and "Check for updates" all end at `check_app_update`, which finds `app_releases` is `None` and returns `Ok(None)`. The app shows "You have the newest version." whatever is published, and no installed copy can ever update itself.
- Evidence: `config` is `ManagerConfig::new(...).with_official_suffix_list()`. `ManagerConfig::new` sets `app_releases: None`, and a search of `crates` finds no caller of `with_official_app_releases` outside its definition. The core tests use `with_app_releases` on their own harness and `no_updates()` only reads the key, so nothing notices. `with_official_proxy_test` has no caller either, so `proxy_test_url` is `None` in the app and the proxy's Test connection has nothing to reach (`manager/proxy.rs`); that belongs to the proxy flow, but it is the same omission in the same function. A test on `config()` that both official sources are set would catch this kind of omission.
- Confidence: certain

### Xpath-app-update-2 [P1] Declining the administrator prompt, or a setup that fails, leaves the app closed with no message

- Where: `crates/shell/src/updates.rs`, `run_setup`; `crates/desktop/src/app_update.rs`, `install`; `crates/desktop/packaging/windows/installer.iss`
- Category: wrong implementation
- Failure: on Windows the person presses Install and restart, then answers No at the administrator prompt, or has no administrator password, or the silent setup fails (a full disk, a file it cannot replace). The app has already quit and stopped every download. Nothing is installed, nothing starts again and nothing says why. D111.4 promises "a failure shows there with Try again and Later", and that path cannot be reached for any of these.
- Evidence: `run_setup` treats a `ShellExecuteW` result above 32 as success, and `install` then calls `lifecycle::quit` at once. Its comment and contract 12 assume the shell asks for approval before returning, but the built `target/packages/Wirecatch-0.1.0-windows-x64-setup.exe` and Inno Setup's `SetupLdr.e64` and `Setup.e32` carry `requestedExecutionLevel level="asInvoker"`. So the shell starts the setup without a prompt and returns, and the prompt comes from the setup raising itself afterwards (`PrivilegesRequired=admin`), when the app is already on its way out. The only start of the new version is a `[Run]` entry, which a declined or failed setup never reaches, and `/SUPPRESSMSGBOXES` hides the setup's own errors. The app does not need to quit first: `CloseApplications=force` closes it. Not run against a real prompt; the order follows from the manifest.
- Confidence: likely

### Xpath-app-update-3 [P2] A replaced AppImage is started while the old copy still holds the endpoint, so the restart can be lost

- Where: `crates/shell/src/updates.rs`, `replace_appimage`; `crates/desktop/src/app_update.rs`, `install`; `crates/desktop/src/lifecycle.rs`, `quit`; `crates/desktop/src/main.rs`, `main`
- Category: wrong implementation
- Failure: `replace_appimage` spawns the new copy, and only then does `install` call `lifecycle::quit`. If the new copy finds the endpoint taken, it hands its launch to the old one and exits. That works while the old copy is stopping, through `start_again_later`. If the old copy passes `again.take()` before the request is handled, the request is answered and ignored, and no copy runs. If the old copy exits between the new copy's `bind` and its `hand_over`, the new copy says "Wirecatch is already running for you" and closes, with nothing running.
- Evidence: `main` returns after `hand_over` on `AlreadyInUse`, and shows `say_already_running` when it fails. `Quitting.again` is read once after `stopped.await`, and the listener stays open until the process exits, so `Call::ShowWindow` can still get `Reply::Done` after that read. How often it happens depends on how long the stop takes against the AppImage's start, so it is not every time. `Quitting.again` with `start_again` already restarts after the endpoint is freed, using `autostart::program()`, which is the AppImage's path; asking for that instead of spawning removes the race. Linux only, reasoned from the code, not run.
- Confidence: likely

### Xpath-app-update-4 [P2] An inherited `APPIMAGE` variable makes the update overwrite another program

- Where: `crates/shell/src/updates.rs`, `install_kind` and `replace_appimage`
- Category: bug
- Failure: a `.deb` or `.rpm` Wirecatch is started from inside another AppImage, such as an editor's terminal or a launcher packaged that way, and inherits `APPIMAGE=/path/to/Other.AppImage`. The app then counts as an AppImage, is offered Install and restart, and the rename puts Wirecatch over `Other.AppImage`, then starts it. The other program is gone.
- Evidence: `install_kind` tests only that the variable is set, and `replace_appimage` takes its target from it with no check that this process runs from that file. The AppImage runtime sets the variable for everything it starts, and children inherit it. Nothing compares `current_exe` with `APPDIR` or the file's name. `autostart::program` and `crates/shell/src/bridge.rs` read the same variable, so the check belongs in one place.
- Confidence: certain

### Xpath-app-update-5 [P2] Every downloaded installer stays in the data folder for good

- Where: `crates/core/src/manager/app_update.rs`, `fetch_app_update`; `crates/desktop/src/app_update.rs`, `start`
- Category: inefficiency
- Failure: each update leaves its whole installer in `<data folder>/updates`, and a cancelled one leaves its part and resume files when a newer version takes over under another name. The folder grows by one installer per release for as long as the app is kept.
- Evidence: `UPDATES_FOLDER` is used only by `fetch_app_update` and its test, which delete only a file that fails its check. Each layer leaves this to another: the core returns the path for the shell to run, the shell cannot delete a setup that is running, and the new version's start never looks at the folder.
- Confidence: certain

### Xpath-app-update-6 [P3] The banner tells a person on Windows to use a package manager

- Where: `crates/desktop/src/main_window/notices_bar.rs`, `update_available`; `crates/core/src/manager/app_update.rs`, `check_app_update`
- Category: wrong implementation
- Failure: a release whose manifest has no file for this system and architecture, or whose asset size differs from the manifest's. The core offers the update with `download: None`, and the banner reads "Install it with your package manager." on any system, including Windows, with no What's new.
- Evidence: `download` is `None` for four reasons in the core, while the banner reads it as only the system package case. The workflow's `needs` makes a missing file unlikely, so this takes a release edited by hand or an asset that was replaced.
- Confidence: certain

### Xpath-app-update-7 [P3] Turning off pre-release versions keeps offering the one already found

- Where: `crates/desktop/src/app_update.rs`, `check`; `crates/desktop/src/main_window/settings/general.rs`, `updates`
- Category: wrong implementation
- Failure: with "Get pre-release versions" on, a beta is found, and the person turns the setting off. The banner, the About dot and the Settings row still offer the beta and install it, until the app restarts. The Settings row has no check button in this stage either.
- Evidence: the setting is read only inside `Manager::check_app_update`. The switch only sends a settings change, and a quiet check returns early on `Stage::Available` and `Stage::InstallFailed`.
- Confidence: certain

## Suggestions

### Xpath-app-update-S1 Check the signature against the app's key before drafting

- Today: `manifest.mjs` signs with whatever `UPDATER_PRIVATE_KEY` holds and never compares it with `updates.public_key`. With a wrong secret the release looks fine, and every installed copy refuses it. Only the newest release is considered, so older good ones are not offered either.
- Better: verify the new signature against the public key in `config/product.toml` in the same step, and fail the draft.
- Value: a key mistake stops the release instead of quietly ending updates for everyone until the next one.

### Xpath-app-update-S2 Let an older app read a newer manifest

- Today: `Manifest` and `UpdateFile` use `deny_unknown_fields`, and `FileKind` has no fallback. A future release that adds a field or a new kind of installer is `Unreadable` for every older copy, so those copies can never update to it.
- Better: since the signature already proves who wrote the manifest, ignore unknown fields and skip files of an unknown kind.
- Value: the manifest can grow without leaving installed copies behind.
