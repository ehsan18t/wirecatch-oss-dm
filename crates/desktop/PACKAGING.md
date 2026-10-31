# Packaging the desktop app

How the desktop app becomes installers (D106). What the installers do is D19 and D25; this page says how they are made and what each one installs.

## Building installers

```bash
node scripts/package.mjs
```

It builds for this machine; `--target <triple>` picks another, such as `aarch64-unknown-linux-gnu`. It first runs `node scripts/gate.mjs release-build`, which fails when a file generated from `config/product.toml` is out of date and warns while the file still holds a placeholder: such an installer is for testing only, and publishing it is refused (D29, D49). It then builds the app and the bridge in release, with no cargo feature on the bridge (D22), and writes the installers to `target/packages`:

| System  | Files                                                          | Made by                                   |
| ------- | -------------------------------------------------------------- | ----------------------------------------- |
| Windows | `Wirecatch-<version>-windows-<arch>-setup.exe`                 | Inno Setup 6.7 or later, installed first  |
| Linux   | `Wirecatch-<version>-linux-<arch>.deb`, `.rpm` and `.AppImage` | nfpm and appimagetool, downloaded, pinned |

On Windows the build links the Visual C++ runtime statically (D33), and the compiler is looked for in `Program Files (x86)\Inno Setup 6` unless `ISCC` names it. The Linux tools are downloaded to `target/tools` at the versions `scripts/package.mjs` pins and checked against their SHA-256; appimagetool unpacks itself instead of mounting, so no FUSE is needed. On Linux the build needs the system packages CI installs (`scripts/ci/linux-packages.txt`).

The app's executable, `wirecatch-app`, is installed next to `wirecatch-bridge`: the bridge starts the app from its own folder by that name, and the app registers the bridge from its own folder (D19).

## What each installer does

- **Windows** (`packaging/windows/installer.iss`): installs the app, the bridge and `LICENSE.txt` in Program Files for every user, with one administrator prompt (D25). A Start menu shortcut is made, and a desktop shortcut when the person ticks it. After installing it runs `wirecatch-bridge install --all-users` and says so when that fails; it starts the app as the person who ran it, not elevated. A running copy is closed before its files are replaced. Uninstalling runs `wirecatch-bridge uninstall --all-users` and the per-user `uninstall` before the files go, and removes the start-at-sign-in entry of the account running it; other accounts' entries stay, pointing at a program that is gone, which Windows skips.
- **`.deb` and `.rpm`**: the app and the bridge in `/usr/bin`, the `.desktop` entry and the icons under `/usr/share`, the licence under `/usr/share/licenses`. The pre-removal script, filled from `config/templates/prerm.sh`, runs `wirecatch-bridge uninstall` for every regular user on removal, not on upgrade. They depend on the libraries GPUI loads: fontconfig, FreeType, xkbcommon, Wayland, XCB and the Vulkan loader.
- **AppImage**: the same files in one executable, with the two keyboard libraries a desktop without GTK or Qt may lack (D110). Under an AppImage the app copies the bridge to `~/.local/share/wirecatch/bin` and registers that copy (D19). To remove it, run `~/.local/share/wirecatch/bin/wirecatch-bridge uninstall`, then delete that folder.

The `.desktop` entry is named after the app's identifier, which every window also carries as its app id, so Linux desktops show the right icon and group the windows under it.
