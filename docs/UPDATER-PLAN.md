# Build-time updater verification

Wirecatch ships its first desktop release using the current public GitHub repository as its update source. The desktop build and release workflow use the same setting in `config/product.toml`. Signed update manifests can be enabled in a later build. See [D111 and D288](DECISIONS.md#d288-the-desktop-build-chooses-how-it-verifies-updates).

```toml
[updates]
verification = "github"
```

| Mode     | What the installed app verifies                                                                                             | What the release workflow needs                                                          |
| -------- | --------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `github` | The uploaded installer's size and SHA-256 from the configured repository's GitHub release metadata                          | The complete installer set.                                                              |
| `signed` | The release version and Ed25519 signature, the single matching installer entry, and the installer's signed size and SHA-256 | The complete installer set, matching Ed25519 keys, `update.json`, and `update.json.sig`. |

## Implementation

1. **Product configuration.** Validate the two modes in the product loader and generate the Rust setting. Require a public key in signed mode and omit it in GitHub mode. Derive the release feed from the GitHub web URL in `links.repository`. The loader rejects clone URLs ending in `.git`. Keep the publisher, website, and contact checks in the publish gate. [Product configuration](../config/product.toml)
2. **Desktop updater.** Both modes use the one repository baked into the build; there is no other release feed. Scan the configured GitHub Releases endpoint for the highest eligible version with a canonical `v` tag, so a malformed higher tag or an older page cannot hide a valid release. Report a failed check if the scan is incomplete, a page is longer than asked for, or GitHub redirects the list. Build the release page and asset URLs from the repository and tag; the release's own `html_url` and `browser_download_url` are never read. GitHub mode needs one uploaded asset with a SHA-256 digest for the installer. Signed mode requires unique, nonempty uploaded assets for the signing files and the selected installer, and exactly one signed entry for the target with the expected name and the uploaded size. Verify the downloaded bytes before installation and stage an AppImage in an exclusively created temporary file beside the running one before renaming it into place. Prereleases require opt-in, `.deb` and `.rpm` offer the release page without a matching installer, and debug builds disable updates. [Updater core](../crates/core/src/manager/app_update.rs)
3. **Release workflow.** Use one naming rule in packaging, draft validation, and the app, with a stable prefix derived from `product.slug`. Require the exact two Windows setups and six Linux packages across x64 and ARM64; prerelease versions can be packaged for the rehearsal. A version-tag push builds and verifies a draft for the matching repository and version. The draft step checks the local files against the prepared inventory, and in signed mode the signed manifest against the inventory byte for byte. A retry in the same run resumes its draft after checking the tag, release ID and files. Each upload is read back and compared by size and SHA-256, waiting a bounded time for GitHub to report the digest. The workflow leaves publication to the maintainer. Manual runs build test artifacts. [Release workflow](../.github/workflows/release.yml)
4. **Automated verification.** Exercise both modes with fixtures and disposable keys. Tests cover all eight installer names, the draft's file set, retries, late digests and upload verification, paginated release selection with tag filtering and redirect refusal, the signed asset and manifest checks, and installer integrity. [Release command tests](../scripts/release/manifest.test.mjs) · [Draft tests](../scripts/release/draft.test.mjs) · [Updater tests](../crates/core/src/manager/app_update/tests.rs)

## First release rehearsal

- [ ] Add a ruleset that lets only maintainers create, update or delete `v*` tags. Review who has Write access and which apps, tokens and workflows have `contents: write`: each of them can publish releases, and so supply updates in `github` mode.
- [ ] Enable immutable releases before the first public release, so a published release's assets, and the digests `github`-mode builds trust, cannot change.
- [ ] Fill in the publisher legal name, website, support email, and security email. Regenerate the product files and pass `node scripts/gate.mjs publish`.
- [ ] Prepare two ordered GitHub-mode prereleases, such as `v0.1.0-beta.1` and `v0.1.0-beta.2`. Confirm both draft jobs pass and every draft asset carries a `digest`. Cancel one draft job after its first upload and resume it with **Re-run failed jobs**, never **Re-run all jobs**: that rebuilds the installers, which are not reproducible, and the resumed draft stops on a changed asset. After publication, confirm that the public release API reports the expected installer names, sizes, and SHA-256 digests.
- [ ] Install the older prerelease on Windows and Linux AppImage, enable prerelease updates, and check for the newer build. Verify the Windows approval prompt and restart, the AppImage replacement and restart, and the resulting version. Verify that `.deb` and `.rpm` builds open the release page. The rehearsal runs on x64: ARM64 installers are built, and the Linux ones checked on fresh systems in CI, but they are not installed on ARM64 hardware.

## Repository and trust

The repository URL is baked into every build, and the app refuses a redirected release list. Renaming or transferring the repository therefore breaks update checks for installed builds. Once the old name is free, whoever claims it would supply updates to `github`-mode builds. Never rename or transfer the repository while `github`-mode builds are installed: first ship a build that points at the new location, or switch to `signed`.

In GitHub mode, anyone with Write access to the repository, and any app, token or workflow with `contents: write`, can publish a release and so supply updates. Signing adds the private key as a requirement. The protected `updater-signing` environment guards the key, not what it signs: it signs the inventory produced by the unprotected `prepare` job, so a signed release is only as trustworthy as that job.

## Enabling signing later

To enable signing, configure the protected `updater-signing` environment with `UPDATER_PRIVATE_KEY`, embed the matching public key, and rehearse both GitHub-to-signed and signed-to-signed upgrades. The installed build applies its own verification policy. Switching back to `github` omits both manifest files, so users with signed installations need a manual upgrade. If the key is lost or compromised, distribute a trusted installer with a new key for users to install manually. [Signing and recovery runbook](../crates/desktop/PACKAGING.md#enabling-signed-updates)

## Status

The build-time choice is implemented. The release rehearsal above is pending.
