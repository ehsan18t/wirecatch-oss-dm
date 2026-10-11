# Packaging the desktop app

How Wirecatch is bundled into installers (D19). Release builds are made in Wave 3; nothing here needs to run during everyday development.

## Building installers

```bash
pnpm --filter @wirecatch/desktop bundle       # release installers
pnpm --filter @wirecatch/desktop build:tauri  # debug installers, for checking the packaging
```

Both first run `scripts/prepare-bridge.mjs`, which builds `wirecatch-bridge` and copies it to `src-tauri/binaries/wirecatch-bridge-<target triple>[.exe]` (ignored by git). Then `tauri build` runs with `src-tauri/tauri.bundle.conf.json` merged in, which lists the bridge as an external binary. The bundler installs it next to the app with its plain name, `wirecatch-bridge(.exe)`, which matters: the bridge starts `wirecatch-app --background` from its own folder, and the app runs `wirecatch-bridge install` from its own folder.

The external binary lives in a separate config file because Tauri's build script refuses to build when a listed external binary is missing, and plain `cargo build` and `cargo test` should not depend on building the bridge first.

| Platform | Output                                                | Notes                                                            |
| -------- | ----------------------------------------------------- | ---------------------------------------------------------------- |
| Windows  | NSIS installer, per user (`installMode: currentUser`) | No administrator rights. Installs to `%LOCALAPPDATA%\Wirecatch`. |
| Linux    | `.deb`, `.rpm`                                        | App and bridge in `/usr/bin`.                                    |
| Linux    | AppImage                                              | See "AppImage" below.                                            |

## Browser registration and uninstall

The app runs `<its folder>/wirecatch-bridge install` on a background thread at every start (D19). It is idempotent, so installers, AppImages and portable copies all register themselves, and a registration broken by a browser update or a moved install repairs itself. Failures are logged and never block startup. Debug builds never register it: they run as a separate app, "Wirecatch Dev", with their own identifier, data folder and endpoint, so a development copy runs beside an installed Wirecatch without touching it or pointing your browsers at a source checkout.

Uninstalling removes the registration before the files go:

- **Windows**: `src-tauri/windows/hooks.nsh` (the `NSIS_HOOK_PREUNINSTALL` macro) runs `"$INSTDIR\wirecatch-bridge.exe" uninstall`. The installer is per user, so this runs as the user whose registration it removes.
- **Linux packages**: `src-tauri/linux/prerm.sh` is the `.deb` and `.rpm` pre-removal script. On removal (not on upgrade) it runs `/usr/bin/wirecatch-bridge uninstall` as every regular user (uid 1000 to 59999 with an existing home folder), because each user's browsers have their own registration.
- **AppImage**: there is no uninstaller. Run `wirecatch-bridge uninstall` from the extracted image, or leave it: a registration whose bridge is gone only makes the extension report that Wirecatch is unavailable.

### AppImage caveat

An AppImage runs from a temporary mount that changes on every start, so the registered bridge path is only valid while that copy of the app runs. The extension then works while Wirecatch is open but cannot start it. A fix (copying the app and bridge to a stable folder on first start) is planned for Wave 3 with the Linux verification.

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
4. Add the check itself: the plugin is registered, but nothing asks for updates yet. Call `app.updater()?.check()` from Rust after start (for example once a day), and install quietly when the app quits, so a download is never interrupted.
