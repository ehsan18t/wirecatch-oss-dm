# Packaging the desktop app

How the desktop app becomes installers (D106). What the installers do is D19 and D25; this page says how they are made and what each one installs.

## Building installers

```bash
node scripts/package.mjs
```

It builds for this machine; on Windows `--target <triple>` picks another, such as `aarch64-pc-windows-msvc`. Linux packages are made only on a Linux machine of their own architecture, because the AppImage carries that machine's libraries, and any other Linux target is refused. It first runs `node scripts/gate.mjs release-build`, which fails when a file generated from `config/product.toml` is out of date and warns while the file still holds a placeholder: such an installer is for testing only, and publishing it is refused (D29, D49). It then prepares the pinned Rust dependency backports and builds the app and the bridge in release, with no cargo feature on the bridge (D22), and writes the installers to `target/packages`:

| System  | Files                                                          | Made by                                   |
| ------- | -------------------------------------------------------------- | ----------------------------------------- |
| Windows | `Wirecatch-<version>-windows-<arch>-setup.exe`                 | Inno Setup 6.7 or later, installed first  |
| Linux   | `Wirecatch-<version>-linux-<arch>.deb`, `.rpm` and `.AppImage` | nfpm and appimagetool, downloaded, pinned |

On Windows the build links the Visual C++ runtime statically (D33), and the compiler is looked for in `Program Files (x86)\Inno Setup 6` unless `ISCC` names it. The Linux tools are downloaded to `target/tools` at the versions `scripts/package.mjs` pins and checked against their SHA-256; appimagetool unpacks itself instead of mounting, so no FUSE is needed. On Linux the build needs the system packages CI installs (`scripts/ci/linux-packages.txt`).

Each installer carries `THIRD-PARTY-NOTICES.txt`, generated for that build from `Cargo.lock` by `scripts/notices/` and never committed (D117). It lists the crates built in, with the licence texts they require. `node scripts/gate.mjs notices` checks it can be generated for every target. When it stops on a licence, it says what to do: add the text to `config/licenses`, or replace the package or allow its licence in `deny.toml`.

The app's executable, `wirecatch-app`, is installed next to `wirecatch-bridge`: the bridge starts the app from its own folder by that name, and the app registers the bridge from its own folder (D19).

## What each installer does

- **Windows** (`packaging/windows/installer.iss`): installs the app, the bridge, `LICENSE.txt` and the third-party notices in Program Files for every user, with one administrator prompt (D25). A Start menu shortcut is made, and a desktop shortcut when the person ticks it. After installing it runs `wirecatch-bridge install --all-users` and says so when that fails; it starts the app as the person who ran it, not elevated. A running copy is closed before its files are replaced. Uninstalling runs `wirecatch-bridge uninstall --all-users` and the per-user `uninstall` before the files go, removes the start-at-sign-in entry of the account running it, and then closes the bridge and the app if they run from the install folder, for every account; other accounts' entries stay, pointing at a program that is gone, which Windows skips. A program that still cannot be closed is removed at the next restart.
- **`.deb` and `.rpm`**: the app and the bridge in `/usr/bin`, the `.desktop` entry and the icons under `/usr/share`, the licence and the third-party notices under `/usr/share/licenses`. The pre-removal script, filled from `config/templates/prerm.sh`, runs `wirecatch-bridge uninstall` for every regular user on removal, not on upgrade. The post-install script, filled from `config/templates/postinst.sh`, stops any copy still running from the program an upgrade replaced and waits for it to exit (D221). They depend on the libraries GPUI loads: fontconfig, FreeType, xkbcommon, Wayland, XCB and the Vulkan loader, and on the C library version read from the binaries, which follows the machine that built them. A system with an older C library refuses to install them.
- **AppImage**: the same files in one executable, with the keyboard libraries a desktop without GTK or Qt may lack (D110), which only the app and the bridge look for, so programs the app starts use the system's own (D142). Its notices add the licences of those libraries and of the AppImage runtime. Under an AppImage the app copies the bridge to `~/.local/share/wirecatch/bin` and registers that copy (D19). To remove it, run `~/.local/share/wirecatch/bin/wirecatch-bridge uninstall`, then delete that folder.

The `.desktop` entry is named after the app's identifier, which every window also carries as its app id, so Linux desktops show the right icon and group the windows under it.

## Releases and updates

A push of a version tag (`v1.2.3`, or `v1.2.3-beta.1` for a pre-release) starts `.github/workflows/release.yml`. The tag must match `product.version` in `config/product.toml`, and the workflow repository must match `links.repository`. The workflow builds all eight installers on Windows and Linux, x64 and ARM64, checks the Linux packages on fresh systems, and drafts a release with the installers and the browser extension's store packages. It checks the uploaded update assets against the build outputs. Publish the draft after **Draft and verify the release** succeeds. Manual workflow runs build artifacts for testing.

The desktop build and release workflow take their verification mode from `[updates] verification` (D288):

- **`github`** uses GitHub's release metadata to verify the selected installer's size and SHA-256. It needs no updater key, secret, or published manifest. In this mode, access to publish releases in the configured repo grants authority to supply updates. Restrict who can create release tags and publish releases. Keep the published installer names compatible with earlier builds of the updater.
- **`signed`** publishes `update.json` with the version and installer checksums, and `update.json.sig` with its Ed25519 signature. The app verifies the signature with its embedded public key. Manifest signing is separate from operating system code signing.

Release builds check a minute after startup, once a day, and on demand under **Settings > General > Updates**. Pre-releases require **Get pre-release versions**. On Windows the user approves the setup prompt and the setup opens the updated app. An AppImage is replaced and relaunched. A `.deb` or `.rpm` install opens the release page for manual package installation.

### Enabling signed updates

Generate the key pair in a private terminal:

```bash
node scripts/release/updater-key.mjs
```

1. Create or configure the GitHub Actions environment `updater-signing` with a reviewer and restrict it to approved release tags. Store the private half as its environment secret `UPDATER_PRIVATE_KEY`. Remove any former repository-level copy of that secret and keep a secure offline backup.
2. Set `[updates] verification = "signed"`, add `public_key` with the public half, and run `pnpm product:generate`. The config loader checks the public key's encoding. Use a newer version for the signed build and run `node scripts/gate.mjs publish`.
3. Check that **Sign the update manifest** succeeds and that **Draft and verify the release** succeeds with both manifest files in the draft. The signing job checks that its private key signs for the configured public key before it hands the files to the draft job.

Existing GitHub-mode installs use GitHub's digest for the upgrade to a signed build. After that build starts, future releases must carry a valid manifest signed with its key. Keep signing subsequent releases for those installs. If the key is lost or compromised, distribute a trusted installer with a new key for users to install manually. Keep the approved tag and signing workflow under maintainer control.

### Rehearsing an upgrade

Before the first stable release, resolve the product identity placeholders and run the publish gate. Prepare two versioned prereleases, such as `0.1.0-beta.1` and `0.1.0-beta.2`, each with regenerated product files and a matching tag. After the maintainer publishes the drafts, confirm that the public GitHub release API lists the expected uploaded installers with SHA-256 digests. Install the older build on Windows and Linux AppImage, enable pre-release updates, and use **Check for updates**. Verify the Windows approval prompt, the AppImage replacement, and that each app starts again at the new version. Check that the `.deb` and `.rpm` builds open the release page.

Before activating signing for stable users, also rehearse an upgrade from a GitHub-mode build to a signed build and from that signed build to a later signed release. Test missing or invalid signatures against a local fixture so a broken candidate stays off the public feed.
