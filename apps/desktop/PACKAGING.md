# Packaging the desktop app

How Wirecatch is bundled into installers (D19). Release builds are made in Wave 3; nothing here needs to run during everyday development.

## Building installers

```bash
pnpm --filter @wirecatch/desktop bundle       # release installers
pnpm --filter @wirecatch/desktop build:tauri  # debug installers, for checking the packaging
```

Both first run `scripts/prepare-bridge.mjs`; with `--release` (the `bundle` script) it first runs `node scripts/gate.mjs publish`, which fails while `config/product.toml` still holds a placeholder, so no installer ships one (D29). Before `cargo publish -p wirecatch-engine`, run `node scripts/gate.mjs publish` too: the engine's `homepage` comes from `config/product.toml`. The script builds `wirecatch-bridge` and copies it to `src-tauri/binaries/wirecatch-bridge-<target triple>[.exe]` (ignored by git). Then `tauri build` runs with `src-tauri/tauri.bundle.conf.json` merged in, which lists the bridge as an external binary and turns on `build.windows.staticVCRuntime`, so the app in every installer, debug or release, links the Visual C++ runtime statically; plain cargo builds of the app link it dynamically (D33). The bundler installs it next to the app with its plain name, `wirecatch-bridge(.exe)`, which matters: the bridge starts `wirecatch-app --background` from its own folder, and the app runs `wirecatch-bridge install` from its own folder.

The bridge is built without any cargo feature in both profiles. Which extensions it trusts never depends on how it was built: every build trusts the one Chromium extension id in `config/product.toml` and the Firefox add-on id, and `install` lists that Chromium id in `allowed_origins` for every Chromium browser (D22). Its `test-overrides` feature is for tests only, and `prepare-bridge.mjs` refuses to bundle a bridge built with any feature.

The external binary lives in a separate config file because Tauri's build script refuses to build when a listed external binary is missing, and plain `cargo build` and `cargo test` should not depend on building the bridge first.

| Platform | Output                                                  | Notes                                                                                                                          |
| -------- | ------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Windows  | NSIS installer, per machine (`installMode: perMachine`) | Installs to Program Files for every user (D25). Installing and updating ask for administrator rights once; running never does. |
| Linux    | `.deb`, `.rpm`                                          | App and bridge in `/usr/bin`.                                                                                                  |
| Linux    | AppImage                                                | See "AppImage" below.                                                                                                          |

## Browser registration and uninstall

The Windows installer registers the bridge for every user (`wirecatch-bridge install --all-users`, D25). The app also runs `<its folder>/wirecatch-bridge install` on a background thread at every start (D19), which does nothing for a bridge the machine registration already covers. It is idempotent, so installers, AppImages and portable copies all register themselves, and a registration broken by a browser update or a moved install repairs itself. Failures are logged and never block startup. Debug builds never register it: they run as a separate app, "Wirecatch Dev", with their own identifier, data folder and endpoint, so a development copy runs beside an installed Wirecatch without touching it or pointing your browsers at a source checkout.

Registration covers Google Chrome, Chromium, ungoogled-chromium, Microsoft Edge, Brave, Vivaldi, Opera, Opera GX and Arc (Windows), Thorium, Yandex Browser (Linux), and Firefox with LibreWolf, Waterfox, Zen and Floorp. Many of them read another browser's location (on Windows most Chromium browsers read Google Chrome's key, and the Firefox forks read Mozilla's key and `~/.mozilla/native-messaging-hosts`), so the bridge writes only the locations browsers really read; `crates/bridge/src/install.rs` lists each one with its source. Flatpak and Snap browsers cannot start a program outside their sandbox, so nothing is written inside them: Firefox as a Snap or Flatpak reaches the bridge through the WebExtensions portal where the distribution provides it, and Chromium-based Flatpaks and Snaps are not supported.

Uninstalling removes the registration before the files go:

- **Windows**: `src-tauri/windows/hooks.nsh` registers for every user after installing (`NSIS_HOOK_POSTINSTALL`) and, on a real uninstall (`NSIS_HOOK_PREUNINSTALL`), runs `"$INSTDIR\wirecatch-bridge.exe" uninstall --all-users`, then a per-user `uninstall` and the start-at-sign-in cleanup for the account running it.
  - When registering for every user fails, the installer says so and asks to run it again; the bridge's own message is in the install details.
  - Installing over a copy an earlier build installed for one account (per user, in that account's own folder) removes that copy first, so it no longer registers itself for that account or starts at sign-in beside the new one. Only the account running the installer is reached; another account with such a copy removes it in Settings, Apps.
  - Each account that turned on starting with Windows has its own entry, and an uninstall removes only the one of the account running it. The others stay behind, pointing at a program that is gone; Windows skips them, and Task Manager lists them under Startup apps, where they can be removed.
- **Linux packages**: `src-tauri/linux/prerm.sh` is the `.deb` and `.rpm` pre-removal script. On removal (not on upgrade) it runs `/usr/bin/wirecatch-bridge uninstall` as every regular user (uid 1000 to 59999 with an existing home folder), because each user's browsers have their own registration.
- **AppImage**: there is no uninstaller. Run `~/.local/share/wirecatch/bin/wirecatch-bridge uninstall`, then delete that folder.

### AppImage

An AppImage runs from a temporary mount (`/tmp/.mount_XXXX`) that disappears when it exits; registering the bridge there would point every browser at a path another local user could later fill with a program of their own. So under an AppImage (the `APPIMAGE` variable is set) the app copies the bridge to `~/.local/share/wirecatch/bin` (folder and files mode 0700, owned by the user, never written through a link) and registers that copy. Next to it, a two-line `wirecatch-app` script runs `exec '<path of the .AppImage>' "$@"`, because the bridge starts `wirecatch-app --background` from its own folder. Both are rewritten on every start, so moving the AppImage repairs itself on its next start. A bridge in a temporary folder is never registered, and an AppImage kept in `/tmp` is refused with a message asking to move it.

## Updates (switched off for now)

`tauri-plugin-updater` is compiled in, but the app only registers it when `tauri.conf.json` has a `plugins.updater` section with a public key. That needs the GitHub organization and a signing key first (D19). To switch it on:

1. Make a signing key pair: `pnpm tauri signer generate -w ~/.tauri/wirecatch.key`. Keep the private key and its password secret (CI secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`).
2. Add to `tauri.conf.json`:

   ```json
   "bundle": { "createUpdaterArtifacts": true },
   "plugins": {
     "updater": {
       "pubkey": "<contents of wirecatch.key.pub>",
       "endpoints": ["https://github.com/<org>/wirecatch/releases/latest/download/latest.json"],
       "windows": { "installMode": "passive" }
     }
   }
   ```

3. Publish `latest.json` and the signed installers with each GitHub release.
4. That is all: the app checks a minute after start and then daily, downloads a newer version in the background (the updater refuses it unless its signature matches the public key), shows a notification, and installs it when the person quits, so a download is never interrupted. `src-tauri/tests/updates.rs` tests this against a local server with a throwaway key.
