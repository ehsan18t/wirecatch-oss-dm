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

A version tag (`v1.2.3`, or `v1.2.3-beta.1` for a pre-release) starts `.github/workflows/release.yml`. It builds every installer on Windows and Linux, x64 and ARM64, checks each Linux package of both architectures on a fresh system, and drafts the GitHub release with all of them and the browser extension's store packages. The tag must name the version in `config/product.toml`, the workflow must run in the repository `links.repository` names, and all eight installers must be there. After the upload, each file's size and SHA-256 digest on the draft must match the built file. A failed draft job can be run again: it uploads into the same draft, and stops once the release is published. A file on the draft that this run did not build, such as an `update.json` left from a signed run, fails the check until it is deleted from the draft.

The app looks at the repository's releases a minute after it starts and once a day, and offers the newest release newer than itself, pre-releases only with "Get pre-release versions" on. It installs a file only when its size and SHA-256 match the ones `[updates] verification` trusts (D288):

- **`github`**: the size and digest GitHub reports for the release's file. Anyone who can publish a release in the repository can supply updates, and the repository is never renamed or transferred while such builds are installed (D288).
- **`signed`**: the release also carries `update.json`, listing each installer with its system, architecture, kind, size and SHA-256 (`crates/updates/samples/update.json` shows the shape), and `update.json.sig`, its Ed25519 signature made by the workflow's signing job with `UPDATER_PRIVATE_KEY`. The app trusts the manifest only when the signature matches the public key in `config/product.toml`.

On Windows it runs the setup silently and quits once the administrator's prompt is approved; the setup starts the new version, or the old one again if it fails. An AppImage takes the place of the one running. A `.deb` or `.rpm` install opens the release page, since the system's package manager installs those.

Installer names start with `product.slug`, its first letter capitalized. Keep the slug the same after the first release, so installed builds find the new installers.

### Enabling signed updates

To make the key pair once:

```bash
node scripts/release/updater-key.mjs
```

Put the public half in `config/product.toml` (`[updates] verification = "signed"` and `public_key`, then `pnpm product:generate`). Create the environment `updater-signing` (Settings > Environments), limit its deployments to `v*` tags, optionally require a reviewer, and store the private half there as the environment secret `UPDATER_PRIVATE_KEY`, never as a repository secret. The workflow's signing job is the only one that reads it, and the draft waits for it. Before signed builds reach users, rehearse an upgrade from a `github` build to a signed one and on to a later signed release. A signed build needs every later release signed with its key: going back to `github`, or losing the key, means signed users install a new build by hand.

### Securing the repository

Before the first public release, add a ruleset that lets only maintainers create, update or delete `v*` tags, review who has Write access and which apps, tokens and workflows have `contents: write`, and enable immutable releases.

### Rehearsing an upgrade

Resolve the product identity placeholders and run the publish gate. Prepare two prereleases, such as `0.1.0-beta.1` and `0.1.0-beta.2`, each with regenerated product files and a matching tag, and publish them once their drafts pass. Install the older build on Windows and Linux AppImage, enable pre-release updates, and use **Check for updates**. Verify the Windows approval prompt, the AppImage replacement, and that each app starts again at the new version. Check that the `.deb` and `.rpm` builds open the release page. The rehearsal runs on x64: ARM64 installers are built, and the Linux ones checked on fresh systems in CI, but they are not installed on ARM64 hardware.
