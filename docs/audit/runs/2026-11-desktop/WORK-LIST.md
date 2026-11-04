# Work list: desktop audit fixes

Take the first open PR whose dependencies are merged, make its branch from main, and start from its proof if it has one. Check first that each item still applies; if one does not, mark it in `AUDIT-LOG.md` rather than fixing it. In the same PR, mark its items fixed in `AUDIT-LOG.md` and `docs/ROADMAP.md`, and mark the PR `merged` here. Proofs were made on the audit's commit, so they may need small edits; the first line of a patch says how it ran, and any `--profile` there was only the audit's own build folder.

## All pull requests

| PR     | Title                                              | Branch                           | Items                        | Severity | Effort | Depends on   | Status |
| ------ | -------------------------------------------------- | -------------------------------- | ---------------------------- | -------- | ------ | ------------ | ------ |
| PR-01  | fix(core): stop two downloads writing one file     | `fix/one-writer-per-file`        | 1                            | P0       | large  | none         | merged |
| PR-02  | fix(core): keep folders when removing a download   | `fix/remove-keeps-folders`       | 2                            | P0       | small  | none         | merged |
| PR-03  | fix(engine): stop calling a cut-off file complete  | `fix/cut-off-unknown-size`       | 3                            | P0       | small  | none         | merged |
| PR-04  | fix(core): save whether a row wrote its own file   | `fix/row-wrote-its-file`         | 4                            | P1       | medium | none         | merged |
| PR-05  | fix(core): keep a browser capture that asks first  | `fix/capture-meets-ask`          | 5                            | P1       | small  | none         | merged |
| PR-06  | fix(desktop): trust APPIMAGE only for this program | `fix/appimage-variable`          | 6                            | P1       | small  | none         | merged |
| PR-07  | fix(desktop): name the official online sources     | `fix/official-sources`           | 7                            | P1       | small  | PR-06        | merged |
| PR-08  | fix(core): copy moved files under a temporary name | `fix/staged-move-copy`           | 8, 104                       | P2       | medium | none         | merged |
| PR-09  | fix(core): wait for moves before quitting          | `fix/quit-waits-for-moves`       | 9                            | P1       | medium | PR-08        | open   |
| PR-10  | fix(desktop): end the process at the quit limit    | `fix/quit-limit-ends-process`    | 10                           | P1       | medium | PR-08, PR-09 | open   |
| PR-11  | fix(core): apply the file-exists rule to all names | `fix/file-exists-rule-all-names` | 11                           | P1       | medium | PR-01, PR-05 | open   |
| PR-12  | fix(settings): protect app folders on Windows      | `fix/protect-run-folders`        | 12                           | P1       | medium | none         | merged |
| PR-13  | fix(core): tie a media work folder to its run      | `fix/work-folder-follows-run`    | 13                           | P2       | medium | none         | merged |
| PR-14  | fix(core): stop reusing another link's media parts | `fix/media-work-identity`        | 14                           | P1       | medium | PR-13        | open   |
| PR-15  | fix(desktop): let text fields keep their own keys  | `fix/field-keys-first`           | 15                           | P1       | small  | none         | open   |
| PR-16  | fix(streams): limit the qualities of HLS playlists | `fix/limit-hls-qualities`        | 16                           | P1       | small  | none         | merged |
| PR-17  | fix(streams): refuse segments that are not media   | `fix/check-segment-bodies`       | 17                           | P1       | medium | none         | merged |
| PR-18  | fix(ipc): refuse a lower-integrity stand-in pipe   | `fix/pipe-integrity-check`       | 18                           | P1       | medium | none         | open   |
| PR-19  | fix(streams): limit DASH addresses while building  | `fix/dash-address-limit`         | 19                           | P1       | small  | none         | merged |
| PR-20  | fix(engine): fail a run when a flush fails         | `fix/keep-failed-flush`          | 20                           | P1       | small  | none         | merged |
| PR-21  | fix(engine): renew an expired redirect target      | `fix/expired-redirect-target`    | 21                           | P1       | small  | none         | merged |
| PR-22  | fix(store): open a list with an unreadable value   | `fix/unreadable-value-opens`     | 22                           | P1       | medium | none         | open   |
| PR-23  | fix(media): offer only sources ffmpeg can read     | `fix/offer-readable-sources`     | 23                           | P1       | medium | none         | merged |
| PR-24  | fix(media): fetch cookie-gated formats with yt-dlp | `fix/cookie-formats-ytdlp`       | 24                           | P1       | medium | none         | merged |
| PR-25  | fix(store): say when saving fails or recovers      | `fix/save-failure-signal`        | 25, 84, 233                  | P2       | medium | none         | open   |
| PR-26  | fix(store): keep values it cannot read             | `fix/keep-unreadable-values`     | 26                           | P2       | medium | none         | open   |
| PR-27  | fix(core): keep a removal's record until it ends   | `fix/durable-removal`            | 27, 176                      | P2       | medium | none         | merged |
| PR-28  | fix(engine): carry whether a download can resume   | `fix/can-resume-fact`            | 28                           | P2       | medium | none         | open   |
| PR-29  | fix(desktop): show suffix list state in Settings   | `fix/suffix-list-settings`       | 29, 107                      | P2       | medium | none         | open   |
| PR-30  | perf(desktop): stop redoing list work every frame  | `perf/list-work-per-frame`       | 30                           | P2       | large  | none         | open   |
| PR-31  | fix(desktop): correct links, folder in add dialog  | `fix/add-dialog-links`           | 31, 56, 128                  | P2       | small  | none         | open   |
| PR-32  | fix(engine): read server file names correctly      | `fix/server-file-names`          | 32, 50, 64                   | P2       | medium | none         | merged |
| PR-33  | fix(streams): finish a joined video safely         | `fix/finish-video-safely`        | 33, 67, 200                  | P2       | medium | none         | merged |
| PR-34  | fix(bridge): start the app on a damaged key        | `fix/bridge-damaged-key`         | 34, 153                      | P2       | small  | none         | open   |
| PR-35  | fix(core): keep rows right through file questions  | `fix/rows-through-questions`     | 35, 66, 85                   | P2       | medium | none         | open   |
| PR-36  | fix(settings): read proxy values so they can work  | `fix/usable-proxy-values`        | 36, 115, 220                 | P2       | medium | none         | merged |
| PR-37  | fix(core): keep a name when a link becomes a page  | `fix/page-keeps-name`            | 37, 175                      | P2       | medium | none         | merged |
| PR-38  | fix(media): read all of yt-dlp's page description  | `fix/read-whole-page-info`       | 38, 61                       | P2       | medium | none         | merged |
| PR-39  | fix(engine): reuse part files under other names    | `fix/find-own-part-file`         | 39                           | P2       | medium | none         | merged |
| PR-40  | fix(core): report whether the add-on works         | `fix/addon-usable`               | 40, 53                       | P2       | medium | none         | open   |
| PR-41  | fix(desktop): keep notices in step with downloads  | `fix/notices-follow-downloads`   | 41, 123, 232                 | P2       | medium | none         | open   |
| PR-42  | fix(engine): hold FTP to the local network rule    | `fix/ftp-local-network`          | 42, 43                       | P2       | medium | none         | merged |
| PR-43  | refactor(sites): merge the host-under-site checks  | `refactor/one-host-under-site`   | 44                           | P3       | medium | none         | merged |
| PR-44  | fix(engine): normalise ftps hosts when parsing     | `fix/normal-ftps-hosts`          | 45                           | P2       | small  | PR-43        | open   |
| PR-45  | fix: hide only what is secret in a log line        | `fix/log-redaction-edges`        | 46, 222                      | P2       | small  | none         | merged |
| PR-46  | fix(core): keep link passwords out of questions    | `fix/question-link-password`     | 47                           | P2       | medium | PR-45        | open   |
| PR-47  | fix(core): see finished files beside resume files  | `fix/finished-beside-resume`     | 48, 174                      | P2       | small  | PR-04        | open   |
| PR-48  | fix(media): make any video title safe as a name    | `fix/safe-video-titles`          | 49, 95                       | P2       | small  | none         | merged |
| PR-49  | fix(messages): give advice that fits the failure   | `fix/advice-fits-failure`        | 51, 105, 106, 206            | P2       | medium | none         | open   |
| PR-50  | fix(store): keep a set-aside list for a full month | `fix/set-aside-month`            | 52, 156, 237                 | P2       | small  | none         | open   |
| PR-51  | fix(desktop): correct what download questions say  | `fix/download-questions`         | 54, 122, 231                 | P2       | medium | none         | open   |
| PR-52  | fix(desktop): give download windows the right fix  | `fix/window-fix-buttons`         | 55, 81                       | P2       | medium | none         | open   |
| PR-53  | fix(desktop): make settings actions answer back    | `fix/settings-answers`           | 57, 127, 230                 | P2       | medium | none         | open   |
| PR-54  | fix(core): handle typed names in media downloads   | `fix/typed-media-names`          | 58, 132                      | P2       | medium | none         | open   |
| PR-55  | fix(desktop): follow an installer through setup    | `fix/installer-lifecycle`        | 59, 72                       | P2       | medium | none         | merged |
| PR-56  | fix(core): offer updates only when they can work   | `fix/honest-update-offers`       | 60, 185, 186, 187            | P2       | medium | none         | open   |
| PR-57  | fix(streams): stop trusting bad work folder data   | `fix/work-folder-trust`          | 62, 74                       | P2       | medium | none         | merged |
| PR-58  | fix(streams): report how a program really ended    | `fix/program-end-report`         | 63, 77, 157                  | P2       | medium | none         | merged |
| PR-59  | fix(cli): report stops, speed, errors correctly    | `fix/cli-reports`                | 65, 91, 182                  | P2       | medium | none         | merged |
| PR-60  | fix(core): apply proxy exceptions to yt-dlp        | `fix/ytdlp-proxy-exceptions`     | 68, 69                       | P2       | small  | none         | merged |
| PR-61  | fix(core): move path checks off async threads      | `fix/path-checks-off-async`      | 70                           | P2       | medium | none         | merged |
| PR-62  | fix(engine): handle resume files more safely       | `fix/safer-resume-files`         | 71, 88, 89, 158              | P2       | medium | none         | open   |
| PR-63  | fix(desktop): make an AppImage restart reliable    | `fix/appimage-restart`           | 73, 184                      | P2       | medium | none         | merged |
| PR-64  | fix(core): read the page again after a refusal     | `fix/reread-refused-page`        | 75, 134, 145                 | P2       | medium | none         | open   |
| PR-65  | fix(desktop): show all an import will change       | `fix/full-import-preview`        | 76, 148, 207                 | P2       | medium | none         | open   |
| PR-66  | fix(desktop): show moved downloads in their place  | `fix/moved-row-position`         | 78                           | P2       | medium | none         | open   |
| PR-67  | fix(queues): save whether a queue is in a run      | `fix/saved-queue-run`            | 79, 138                      | P2       | medium | none         | open   |
| PR-68  | fix(desktop): keep one window per download         | `fix/one-window-per-download`    | 80, 139                      | P2       | medium | none         | open   |
| PR-69  | fix(core): keep a row's answers after a restart    | `fix/answers-survive-restart`    | 82, 83                       | P2       | medium | PR-04        | open   |
| PR-70  | fix(core): answer a repeated capture once          | `fix/repeated-capture`           | 86, 151, 173                 | P2       | medium | none         | merged |
| PR-71  | fix(engine): count transferred bytes correctly     | `fix/count-bytes`                | 87, 177, 180                 | P2       | small  | none         | open   |
| PR-72  | fix(engine): read range answers as they are        | `fix/range-answers`              | 90, 178                      | P2       | small  | none         | merged |
| PR-73  | fix(desktop): handle odd launches safely           | `fix/odd-launches`               | 92, 131                      | P2       | medium | none         | merged |
| PR-74  | fix(desktop): close the app before uninstalling    | `fix/uninstall-closes-app`       | 93                           | P2       | small  | none         | merged |
| PR-75  | fix: correct faults in the linux packages          | `fix/linux-packages`             | 94, 188, 189, 190            | P2       | medium | none         | open   |
| PR-76  | fix(media): track, limit yt-dlp's own downloads    | `fix/ytdlp-progress-limit`       | 96, 98                       | P2       | small  | none         | open   |
| PR-77  | fix(media): make quality offers match downloads    | `fix/quality-offers`             | 97, 99, 197                  | P2       | medium | none         | open   |
| PR-78  | fix(streams): read HLS masters, keys correctly     | `fix/hls-masters-keys`           | 100, 101, 102, 198           | P2       | medium | none         | open   |
| PR-79  | fix(desktop): tell loading from a failed list      | `fix/list-open-states`           | 103, 135, 211                | P2       | medium | none         | open   |
| PR-80  | fix(core): correct how the add-on updates itself   | `fix/addon-updates`              | 108, 109, 110, 159, 208, 209 | P2       | medium | none         | merged |
| PR-81  | fix(desktop): act on the rows the view shows       | `fix/selection-commands`         | 111, 136, 212                | P2       | medium | none         | open   |
| PR-82  | fix(desktop): tidy navigation after a page opens   | `fix/navigation-state`           | 112, 162                     | P2       | small  | none         | open   |
| PR-83  | fix(desktop): offer only actions that apply        | `fix/offers-that-apply`          | 113, 163, 166                | P2       | medium | none         | open   |
| PR-84  | fix(sites): correct what counts as a local page    | `fix/what-counts-local`          | 114, 215, 244                | P2       | small  | none         | open   |
| PR-85  | fix(desktop): say what the proxy form will save    | `fix/proxy-form-truth`           | 116, 221                     | P2       | small  | none         | open   |
| PR-86  | fix(queues): hold queue slots to the real limit    | `fix/queue-slots`                | 117, 118                     | P2       | small  | none         | merged |
| PR-87  | fix(queues): make rules match the way they read    | `fix/rules-match-text`           | 119, 223, 224                | P2       | small  | none         | merged |
| PR-88  | fix(desktop): fix small faults in queue dialogs    | `fix/queue-dialogs`              | 120, 121, 165                | P2       | small  | none         | open   |
| PR-89  | fix(desktop): make tray actions answer correctly   | `fix/tray-actions`               | 124, 160                     | P2       | small  | none         | open   |
| PR-90  | fix(desktop): show a processing step as it is      | `fix/processing-step`            | 125, 228                     | P2       | small  | none         | open   |
| PR-91  | fix(desktop): correct wording of app messages      | `fix/app-message-wording`        | 126, 161                     | P2       | small  | none         | open   |
| PR-92  | fix: repair the repo's check scripts               | `fix/check-scripts`              | 129, 191, 238, 239, 240, 241 | P2       | medium | none         | merged |
| PR-93  | fix(engine): lock the part file during a run       | `fix/part-file-lock`             | 130                          | P2       | medium | none         | merged |
| PR-94  | fix(core): keep a video's work until it is placed  | `fix/work-until-placed`          | 133, 169                     | P2       | medium | PR-04        | open   |
| PR-95  | fix(core): apply the local rule to yt-dlp reads    | `fix/ytdlp-local-rule`           | 137                          | P2       | medium | none         | open   |
| PR-96  | fix(engine): check free space before a download    | `fix/free-space-check`           | 140                          | P2       | medium | none         | merged |
| PR-97  | fix(core): record every file of a move up front    | `fix/record-moves`               | 141, 201                     | P2       | medium | PR-04, PR-08 | open   |
| PR-98  | chore: remove code nothing calls                   | `chore/remove-dead-commands`     | 142, 235, 236                | P3       | medium | none         | open   |
| PR-99  | test: wait under PATIENCE in every test            | `test/patience-waits`            | 143, 242                     | P3       | medium | none         | open   |
| PR-100 | test: stop racing real-clock limits in tests       | `test/no-clock-races`            | 144, 167                     | P3       | medium | none         | open   |
| PR-101 | test: share test values through test-support       | `test/shared-test-values`        | 146, 147, 149, 183           | P3       | medium | none         | open   |
| PR-102 | fix(desktop): show startup notices only once       | `fix/startup-notices-once`       | 150, 214                     | P3       | medium | none         | open   |
| PR-103 | fix(core): let a person's stop win                 | `fix/person-stop-wins`           | 152, 172                     | P3       | small  | none         | merged |
| PR-104 | fix(engine): tighten network guard edge cases      | `fix/net-guard-edges`            | 154, 155, 216, 217           | P3       | medium | none         | open   |
| PR-105 | fix(media): word program failures plainly          | `fix/word-program-failures`      | 164                          | P3       | small  | none         | open   |
| PR-106 | refactor(desktop): share repeated form logic       | `refactor/share-form-logic`      | 168                          | P3       | medium | none         | open   |
| PR-107 | fix: write kept files whole before replacing       | `fix/write-kept-files-whole`     | 170, 171                     | P3       | small  | none         | open   |
| PR-108 | refactor(engine): remove code with no effect       | `refactor/engine-no-op-code`     | 179, 181                     | P3       | small  | none         | merged |
| PR-109 | refactor(streams): remove leftovers from the crate | `refactor/streams-leftovers`     | 192, 193, 194                | P3       | small  | none         | open   |
| PR-110 | test(core): repair media run tests, doc comments   | `test/media-run-tidy`            | 195, 196                     | P3       | small  | none         | open   |
| PR-111 | fix(streams): stop media programs on a signal      | `fix/media-programs-on-signal`   | 199                          | P3       | medium | none         | open   |
| PR-112 | fix(desktop): correct system registration details  | `fix/system-registration`        | 202, 203, 204                | P3       | small  | none         | merged |
| PR-113 | fix(desktop): give every window a native title     | `fix/native-window-title`        | 205                          | P3       | small  | none         | merged |
| PR-114 | fix(desktop): steady the list under the pointer    | `fix/list-under-pointer`         | 210, 213                     | P3       | small  | none         | open   |
| PR-115 | fix(core): keep only finished proxy test results   | `fix/proxy-test-results`         | 218, 219                     | P3       | small  | none         | merged |
| PR-116 | fix(desktop): correct what queue pages say         | `fix/queue-page-text`            | 225, 226, 227                | P3       | small  | none         | open   |
| PR-117 | fix(desktop): keep a system accent readable        | `fix/system-accent-contrast`     | 229                          | P3       | small  | none         | open   |
| PR-118 | fix(protocol): correct two language names          | `fix/language-names`             | 234                          | P3       | small  | none         | open   |
| PR-119 | fix(desktop): retry deleting a roaming data folder | `fix/roaming-folder-retry`       | 243                          | P3       | small  | none         | merged |
| PR-120 | test(engine): keep the download fault tests        | `test/keep-fault-tests`          | none                         | none     | medium | PR-39        | merged |

## Lanes

A PR in one lane touches separate code from a PR in another, so the two can run side by side. Inside a lane, go one at a time, in order. Dependencies still apply across lanes.

- **Engine and CLI** (`crates/engine`, `crates/cli`): PR-120, PR-03, PR-20, PR-21, PR-32, PR-39, PR-42, PR-44, PR-59, PR-71, PR-72, PR-93, PR-96, PR-108.
- **Core downloads** (`crates/core` outside media runs, proxies, queues and updates): PR-01, PR-02, PR-04, PR-05, PR-08, PR-09, PR-11, PR-27, PR-35, PR-47, PR-61, PR-69, PR-70, PR-80, PR-97, PR-103.
- **Video and audio** (`crates/streams`, `crates/media`, media runs in `crates/core`): PR-13, PR-14, PR-16, PR-17, PR-19, PR-23, PR-24, PR-33, PR-37, PR-38, PR-48, PR-54, PR-57, PR-58, PR-64, PR-76, PR-77, PR-78, PR-94, PR-95, PR-105, PR-109, PR-110, PR-111.
- **Settings, proxies, sites, queues** (`crates/settings`, `crates/sites`, `crates/queues`, their parts of `crates/core`): PR-12, PR-36, PR-43, PR-60, PR-84, PR-86, PR-87, PR-115.
- **Saved list** (`crates/store`): PR-22, PR-25, PR-26, PR-50.
- **Local endpoint** (`crates/ipc`, `crates/bridge`): PR-18, PR-34.
- **Main window** (`crates/desktop/src/main_window`, the Add dialog, the tray): PR-15, PR-30, PR-31, PR-53, PR-66, PR-79, PR-81, PR-82, PR-83, PR-85, PR-88, PR-89, PR-90, PR-102, PR-106, PR-114, PR-116, PR-117.
- **Download windows** (`crates/desktop/src/download_windows`, the windows and notices in `crates/shell`): PR-41, PR-51, PR-52, PR-68.
- **Start, quit, updates** (`crates/shell`, the app's start and update code in `crates/desktop`): PR-06, PR-07, PR-10, PR-55, PR-56, PR-63, PR-73, PR-112, PR-113, PR-119.
- **Repo and packaging** (`scripts`, `config`, `.github`, packaging, `crates/logging`): PR-45, PR-74, PR-75, PR-92.

These cross lanes, so run each with nothing else touching the crates it names: PR-28, PR-29, PR-40, PR-46, PR-49, PR-62, PR-65, PR-67, PR-91, PR-98, PR-99, PR-100, PR-101, PR-104, PR-107, PR-118.

Known overlaps, where a PR reaches into another lane, so do not run these pairs at once: PR-13 changes `replace_at` as PR-01 does. PR-04, PR-69 and PR-97 add to the saved row in `crates/records` and `crates/store`, so keep them apart from the Saved list lane. PR-25 touches `notices_bar.rs` and `library.rs`, and PR-51 and PR-68 touch `list.rs`, `text.rs` or `row_menu.rs`, all in the Main window lane. PR-60 changes callers in media runs, in the Video and audio lane. PR-31 and PR-73 both touch `crates/shell/src/launch.rs`. PR-30 changes how `crates/desktop/src/library.rs` tells the window of changes, so finish it before other PRs that observe the list, such as PR-66 and PR-102.

## Pull requests

### PR-01 fix(core): stop two downloads writing one file

- **Branch:** `fix/one-writer-per-file`
- **Items:** 1 (P0). Highest: P0.
- **Effort:** large
- **Why:** P0, its own PR.
- **Depends on:** none
- **Touches:** `crates/core`
- **Proof:** `05-1.patch`. `Xconcurrency-1.patch` shows the same bug but sleeps a fixed time, against ENGINEERING section 7, so do not keep it.
- **Checks:** `cargo test -p wirecatch-core`, `cargo clippy -p wirecatch-core`
- **Status:** merged

### PR-02 fix(core): keep folders when removing a download

- **Branch:** `fix/remove-keeps-folders`
- **Items:** 2 (P0). Highest: P0.
- **Effort:** small
- **Why:** P0, its own PR.
- **Depends on:** none
- **Touches:** `crates/core`, `clippy.toml`
- **Proof:** `Xsecurity-path-handling-1.patch`
- **Checks:** `cargo test -p wirecatch-core`, `cargo clippy -p wirecatch-core`. The log proposes a `disallowed-methods` entry for `std::fs::remove_dir_all`, allowed only in the helper for work folders; that is lint config, so run `node scripts/gate.mjs rust-lint` once on the final tree.
- **Status:** merged

### PR-03 fix(engine): stop calling a cut-off file complete

- **Branch:** `fix/cut-off-unknown-size`
- **Items:** 3 (P0). Highest: P0.
- **Effort:** small
- **Why:** P0, its own PR.
- **Depends on:** none
- **Touches:** `crates/engine`
- **Proof:** `Xsecond-review-disk-writes-2.patch`, 2 tests in a new file.
- **Checks:** `cargo test -p wirecatch-engine`, `cargo clippy -p wirecatch-engine`
- **Status:** merged

### PR-04 fix(core): save whether a row wrote its own file

- **Branch:** `fix/row-wrote-its-file`
- **Items:** 4 (P1). Highest: P1.
- **Effort:** medium
- **Why:** P1, its own PR.
- **Depends on:** none
- **Touches:** `crates/core`, `crates/records`, `crates/store`
- **Proof:** `04-1.patch`, 2 tests in two test files.
- **Checks:** `cargo test -p wirecatch-core -p wirecatch-records -p wirecatch-store`, `cargo clippy -p wirecatch-core -p wirecatch-records -p wirecatch-store`
- **Status:** merged

### PR-05 fix(core): keep a browser capture that asks first

- **Branch:** `fix/capture-meets-ask`
- **Items:** 5 (P1). Highest: P1.
- **Effort:** small
- **Why:** P1, its own PR.
- **Depends on:** none
- **Touches:** `crates/core`
- **Proof:** `05-3.patch`. `Xconcurrency-3-and-6.patch` holds 2 tests: only `a_finished_file_captured_again_keeps_its_question` belongs here, the other is for PR-70.
- **Checks:** `cargo test -p wirecatch-core`, `cargo clippy -p wirecatch-core`
- **Status:** merged

### PR-06 fix(desktop): trust APPIMAGE only for this program

- **Branch:** `fix/appimage-variable`
- **Items:** 6 (P1). Highest: P1.
- **Effort:** small
- **Why:** P1, its own PR.
- **Depends on:** none
- **Touches:** `crates/shell`, `crates/desktop/src/lifecycle.rs`
- **Proof:** `12-1.patch`, 2 tests. The `updates.rs` one is Unix only and has not been run.
- **Checks:** `cargo test -p wirecatch-shell`, `cargo clippy -p wirecatch-shell -p wirecatch-desktop`
- **Status:** merged

### PR-07 fix(desktop): name the official online sources

- **Branch:** `fix/official-sources`
- **Items:** 7 (P1). Highest: P1.
- **Effort:** small
- **Why:** P1, its own PR.
- **Depends on:** PR-06
- **Touches:** `crates/desktop/src/backend.rs`
- **Proof:** `Xfeature-behaviour-1.patch`, `Xfeature-behaviour-2.patch`. Each test reads `backend.rs` as text rather than building the desktop crate.
- **Checks:** `cargo test -p wirecatch-desktop`, `cargo clippy -p wirecatch-desktop`. The log proposes a test on `config()` that every official source is set.
- **Status:** merged

### PR-08 fix(core): copy moved files under a temporary name

- **Branch:** `fix/staged-move-copy`
- **Items:** 8 (P2), 104 (P2). Highest: P2.
- **Effort:** medium
- **Why:** Same function, `move_file`: the Mark of the Web is carried over in the staged copy.
- **Order:** A P2 above the P1s because PR-09 and PR-10 need the staged copy.
- **Depends on:** none
- **Touches:** `crates/core`
- **Proof:** `05-4.patch` for item 104 only; it needs `AUDIT_OTHER_DRIVE` set to a drive other than the one holding `%TEMP%`. Item 8 has none.
- **Checks:** `cargo test -p wirecatch-core`, `cargo clippy -p wirecatch-core`
- **Status:** merged

### PR-09 fix(core): wait for moves before quitting

- **Branch:** `fix/quit-waits-for-moves`
- **Items:** 9 (P1). Highest: P1.
- **Effort:** medium
- **Why:** P1, its own PR.
- **Depends on:** PR-08
- **Touches:** `crates/core`
- **Proof:** `Xshutdown-1.patch`, 2 tests.
- **Checks:** `cargo test -p wirecatch-core`, `cargo clippy -p wirecatch-core`
- **Status:** open

### PR-10 fix(desktop): end the process at the quit limit

- **Branch:** `fix/quit-limit-ends-process`
- **Items:** 10 (P1). Highest: P1.
- **Effort:** medium
- **Why:** P1, its own PR.
- **Depends on:** PR-08, PR-09
- **Touches:** `crates/shell/src/quit.rs`, `crates/desktop/src/backend.rs`, `crates/desktop/src/lifecycle.rs`
- **Proof:** `Xshutdown-3.patch` is a reproduction, not a test: it adds trace hooks to `main.rs`. Use it to see the failure, and do not commit it.
- **Checks:** `cargo test -p wirecatch-shell -p wirecatch-desktop`, `cargo clippy -p wirecatch-shell -p wirecatch-desktop`
- **Status:** open

### PR-11 fix(core): apply the file-exists rule to all names

- **Branch:** `fix/file-exists-rule-all-names`
- **Items:** 11 (P1). Highest: P1.
- **Effort:** medium
- **Why:** P1, its own PR.
- **Depends on:** PR-01, PR-05
- **Touches:** `crates/core`
- **Proof:** `05-2.patch`, 2 tests.
- **Checks:** `cargo test -p wirecatch-core`, `cargo clippy -p wirecatch-core`
- **Status:** open

### PR-12 fix(settings): protect app folders on Windows

- **Branch:** `fix/protect-run-folders`
- **Items:** 12 (P1). Highest: P1.
- **Effort:** medium
- **Why:** P1, its own PR.
- **Depends on:** none
- **Touches:** `crates/settings`
- **Proof:** `Xsecurity-path-handling-2.patch`, 2 tests, Windows only.
- **Checks:** `cargo test -p wirecatch-settings`, `cargo clippy -p wirecatch-settings`
- **Status:** merged

### PR-13 fix(core): tie a media work folder to its run

- **Branch:** `fix/work-folder-follows-run`
- **Items:** 13 (P2). Highest: P2.
- **Effort:** medium
- **Why:** One item. It strands the folders that PR-14 has to trust.
- **Order:** A P2 above the later P1s because PR-14 needs it.
- **Depends on:** none
- **Touches:** `crates/core`
- **Proof:** `17-2.patch`
- **Checks:** `cargo test -p wirecatch-core`, `cargo clippy -p wirecatch-core`
- **Status:** merged

### PR-14 fix(core): stop reusing another link's media parts

- **Branch:** `fix/media-work-identity`
- **Items:** 14 (P1). Highest: P1.
- **Effort:** medium
- **Why:** P1, its own PR.
- **Depends on:** PR-13
- **Touches:** `crates/core`, `crates/streams`
- **Proof:** `17-1.patch`, 2 tests.
- **Checks:** `cargo test -p wirecatch-core -p wirecatch-streams`, `cargo clippy -p wirecatch-core -p wirecatch-streams`
- **Status:** open

### PR-15 fix(desktop): let text fields keep their own keys

- **Branch:** `fix/field-keys-first`
- **Items:** 15 (P1). Highest: P1.
- **Effort:** small
- **Why:** P1, its own PR.
- **Depends on:** none
- **Touches:** `crates/desktop/src/main_window`
- **Proof:** `25-1.patch`, 3 tests in `keymap.rs`.
- **Checks:** `cargo test -p wirecatch-desktop`, `cargo clippy -p wirecatch-desktop`
- **Status:** open

### PR-16 fix(streams): limit the qualities of HLS playlists

- **Branch:** `fix/limit-hls-qualities`
- **Items:** 16 (P1). Highest: P1.
- **Effort:** small
- **Why:** P1, its own PR.
- **Depends on:** none
- **Touches:** `crates/streams`
- **Proof:** `14-2.patch`. The wall-time test in it is ignored by default; use the allocation test.
- **Checks:** `cargo test -p wirecatch-streams`, `cargo clippy -p wirecatch-streams`
- **Status:** merged

### PR-17 fix(streams): refuse segments that are not media

- **Branch:** `fix/check-segment-bodies`
- **Items:** 17 (P1). Highest: P1.
- **Effort:** medium
- **Why:** P1, its own PR.
- **Depends on:** none
- **Touches:** `crates/streams`, `crates/core`
- **Proof:** `15-1.patch`, 4 tests; two of them need a real ffmpeg.
- **Checks:** `cargo test -p wirecatch-streams -p wirecatch-core`, `cargo clippy -p wirecatch-streams -p wirecatch-core`
- **Status:** merged

### PR-18 fix(ipc): refuse a lower-integrity stand-in pipe

- **Branch:** `fix/pipe-integrity-check`
- **Items:** 18 (P1). Highest: P1.
- **Effort:** medium
- **Why:** P1, its own PR.
- **Depends on:** none
- **Touches:** `crates/ipc`
- **Proof:** `08-1.patch`, run at high and again at medium integrity. `B04-p2-checks.patch` holds 5 tests: only the two `audit_08_2_` ones belong here, the others are for PR-34.
- **Checks:** `cargo test -p wirecatch-ipc`, `cargo clippy -p wirecatch-ipc`
- **Status:** open

### PR-19 fix(streams): limit DASH addresses while building

- **Branch:** `fix/dash-address-limit`
- **Items:** 19 (P1). Highest: P1.
- **Effort:** small
- **Why:** P1, its own PR.
- **Depends on:** none
- **Touches:** `crates/streams`
- **Proof:** `14-1.patch`
- **Checks:** `cargo test -p wirecatch-streams`, `cargo clippy -p wirecatch-streams`
- **Status:** merged

### PR-20 fix(engine): fail a run when a flush fails

- **Branch:** `fix/keep-failed-flush`
- **Items:** 20 (P1). Highest: P1.
- **Effort:** small
- **Why:** P1, its own PR.
- **Depends on:** none
- **Touches:** `crates/engine`
- **Proof:** `Xsecond-review-disk-writes-1.patch`. It injects a failed flush into `flush_then`, so keep the test but not that hook as written.
- **Checks:** `cargo test -p wirecatch-engine`, `cargo clippy -p wirecatch-engine`
- **Status:** merged

### PR-21 fix(engine): renew an expired redirect target

- **Branch:** `fix/expired-redirect-target`
- **Items:** 21 (P1). Highest: P1.
- **Effort:** small
- **Why:** P1, its own PR.
- **Depends on:** none
- **Touches:** `crates/engine`
- **Proof:** `02-1.patch`
- **Checks:** `cargo test -p wirecatch-engine`, `cargo clippy -p wirecatch-engine`
- **Status:** merged

### PR-22 fix(store): open a list with an unreadable value

- **Branch:** `fix/unreadable-value-opens`
- **Items:** 22 (P1). Highest: P1.
- **Effort:** medium
- **Why:** P1, its own PR.
- **Depends on:** none
- **Touches:** `crates/store`
- **Proof:** `03-1.patch`, 2 tests. Apply it alone: it does not stack with `03-3.patch`, `03-5.patch` or `25-2.patch`.
- **Checks:** `cargo test -p wirecatch-store`, `cargo clippy -p wirecatch-store`
- **Status:** open

### PR-23 fix(media): offer only sources ffmpeg can read

- **Branch:** `fix/offer-readable-sources`
- **Items:** 23 (P1). Highest: P1.
- **Effort:** medium
- **Why:** P1, its own PR.
- **Depends on:** none
- **Touches:** `crates/media`, `crates/streams/src/ffmpeg.rs`
- **Proof:** `Xpath-media-download-1.patch`
- **Checks:** `cargo test -p wirecatch-media -p wirecatch-streams`, `cargo test -p wirecatch-core --test manager`, `cargo clippy -p wirecatch-media -p wirecatch-streams`. The log proposes one test that ties every offered source to `InputFormat`.
- **Status:** merged

### PR-24 fix(media): fetch cookie-gated formats with yt-dlp

- **Branch:** `fix/cookie-formats-ytdlp`
- **Items:** 24 (P1). Highest: P1.
- **Effort:** medium
- **Why:** P1, its own PR.
- **Depends on:** none
- **Touches:** `crates/media`, `crates/core`
- **Proof:** `Xpath-media-download-2.patch`; the 403 in it comes from the test server.
- **Checks:** `cargo test -p wirecatch-media -p wirecatch-core`, `cargo clippy -p wirecatch-media -p wirecatch-core`
- **Status:** merged

### PR-25 fix(store): say when saving fails or recovers

- **Branch:** `fix/save-failure-signal`
- **Items:** 25 (P2), 84 (P2), 233 (P3). Highest: P2.
- **Effort:** medium
- **Why:** All three are how a failed save is told: the same `write_until_closed` spell and the same `storage_failed` notice.
- **Depends on:** none
- **Touches:** `crates/store`, `crates/core`, `crates/desktop/src/library.rs`, `crates/desktop/src/notices.rs`, `crates/desktop/src/main_window`
- **Proof:** `25-2.patch`, `03-3.patch`. Run each alone: they do not stack.
- **Checks:** `cargo test -p wirecatch-store -p wirecatch-core -p wirecatch-desktop`, `cargo clippy -p wirecatch-store -p wirecatch-core -p wirecatch-desktop`
- **Status:** open

### PR-26 fix(store): keep values it cannot read

- **Branch:** `fix/keep-unreadable-values`
- **Items:** 26 (P2). Highest: P2.
- **Effort:** medium
- **Why:** One item. It works in the same reading code as PR-22 but fixes a different failure.
- **Depends on:** none
- **Touches:** `crates/store`, `crates/core`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-store -p wirecatch-core`, `cargo clippy -p wirecatch-store -p wirecatch-core`
- **Status:** open

### PR-27 fix(core): keep a removal's record until it ends

- **Branch:** `fix/durable-removal`
- **Items:** 27 (P2), 176 (P3). Highest: P2.
- **Effort:** medium
- **Why:** Same function, `finish_removing`: what a removal gives up before it has finished.
- **Depends on:** none
- **Touches:** `crates/core`
- **Proof:** `Xconcurrency-2.patch`, `Xshutdown-2.patch`
- **Checks:** `cargo test -p wirecatch-core`, `cargo clippy -p wirecatch-core`
- **Status:** merged

### PR-28 fix(engine): carry whether a download can resume

- **Branch:** `fix/can-resume-fact`
- **Items:** 28 (P2). Highest: P2.
- **Effort:** medium
- **Why:** One item: one type change that reaches every message promising a continuation.
- **Depends on:** none
- **Touches:** `crates/engine`, `crates/cli`, `crates/messages`, `crates/core`, `crates/desktop/src/main_window`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-engine -p wirecatch-cli -p wirecatch-messages -p wirecatch-core`, `cargo clippy -p wirecatch-engine -p wirecatch-cli -p wirecatch-messages -p wirecatch-core -p wirecatch-desktop`
- **Status:** open

### PR-29 fix(desktop): show suffix list state in Settings

- **Branch:** `fix/suffix-list-settings`
- **Items:** 29 (P2), 107 (P2). Highest: P2.
- **Effort:** medium
- **Why:** The failure note Settings will show is the wording item 107 corrects.
- **Depends on:** none
- **Touches:** `crates/desktop/src/main_window/settings`, `crates/shell/src/api.rs`, `crates/core`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop -p wirecatch-shell -p wirecatch-core`, `cargo clippy -p wirecatch-desktop -p wirecatch-shell -p wirecatch-core`; look at the new controls in both themes.
- **Status:** open

### PR-30 perf(desktop): stop redoing list work every frame

- **Branch:** `perf/list-work-per-frame`
- **Items:** 30 (P2). Highest: P2.
- **Effort:** large
- **Why:** One large item, kept on its own.
- **Depends on:** none
- **Touches:** `crates/desktop/src/main_window`, `crates/desktop/src/library.rs`, `crates/desktop/src/tray.rs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop`, `cargo clippy -p wirecatch-desktop`
- **Status:** open

### PR-31 fix(desktop): correct links, folder in add dialog

- **Branch:** `fix/add-dialog-links`
- **Items:** 31 (P2), 56 (P2), 128 (P2). Highest: P2.
- **Effort:** small
- **Why:** 31 and 56 are one regular expression in `links.rs`; 128 is the same dialog saying the wrong folder.
- **Depends on:** none
- **Touches:** `crates/desktop/src/links.rs`, `crates/desktop/src/add_dialog.rs`, `crates/shell/src/launch.rs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop -p wirecatch-shell`, `cargo clippy -p wirecatch-desktop -p wirecatch-shell`. The log proposes a test that `links` and `launch` agree on every scheme.
- **Status:** open

### PR-32 fix(engine): read server file names correctly

- **Branch:** `fix/server-file-names`
- **Items:** 32 (P2), 50 (P2), 64 (P2). Highest: P2.
- **Effort:** medium
- **Why:** All are what name a file gets from a server, in `naming.rs` and its header reading, and their proofs share files.
- **Depends on:** none
- **Touches:** `crates/engine`, `clippy.toml`
- **Proof:** `02-2.patch` and `B11-1.patch` are one file with both tests; apply only one. `01-7.patch`, `21-2.patch` and `B11-2.patch` are one file with all three tests; apply only one. Item 64 has none.
- **Checks:** `cargo test -p wirecatch-engine`, `cargo clippy -p wirecatch-engine`. The log proposes a `disallowed-methods` entry for `HeaderValue::to_str` in the engine; that is lint config, so run `node scripts/gate.mjs rust-lint` once on the final tree.
- **Status:** merged

### PR-33 fix(streams): finish a joined video safely

- **Branch:** `fix/finish-video-safely`
- **Items:** 33 (P2), 67 (P2), 200 (P3). Highest: P2.
- **Effort:** medium
- **Why:** All are the last steps of a video: flush it, put it in place without replacing, then delete what it came from.
- **Depends on:** none
- **Touches:** `crates/streams`, `crates/core`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-streams -p wirecatch-core`, `cargo clippy -p wirecatch-streams -p wirecatch-core`
- **Status:** merged

### PR-34 fix(bridge): start the app on a damaged key

- **Branch:** `fix/bridge-damaged-key`
- **Items:** 34 (P2), 153 (P3). Highest: P2.
- **Effort:** small
- **Why:** Both are how a bad key is read and answered, and they share a proof.
- **Depends on:** none
- **Touches:** `crates/bridge`, `crates/ipc`
- **Proof:** `B04-p2-checks.patch` holds 5 tests: take `audit_06_1_`, `audit_08_3_` and `audit_b04_1_`. The two `audit_08_2_` ones are for PR-18, so the `windows.rs` part may clash once that is merged.
- **Checks:** `cargo test -p wirecatch-ipc`, `cargo test -p wirecatch-bridge --features test-overrides`, `cargo clippy -p wirecatch-ipc -p wirecatch-bridge`
- **Status:** open

### PR-35 fix(core): keep rows right through file questions

- **Branch:** `fix/rows-through-questions`
- **Items:** 35 (P2), 66 (P2), 85 (P2). Highest: P2.
- **Effort:** medium
- **Why:** All are a row's state around the file-exists question: `answer_file_exists`, `keep_existing`, `replace_at` and `set_download_folder`.
- **Depends on:** none
- **Touches:** `crates/core`
- **Proof:** `04-4.patch`
- **Checks:** `cargo test -p wirecatch-core`, `cargo clippy -p wirecatch-core`
- **Status:** open

### PR-36 fix(settings): read proxy values so they can work

- **Branch:** `fix/usable-proxy-values`
- **Items:** 36 (P2), 115 (P2), 220 (P3). Highest: P2.
- **Effort:** medium
- **Why:** All are proxy values taken from a person or from the system that then cannot work, in `clean_hosts`, `exceptions`, `adopt` and `parse_server`.
- **Depends on:** none
- **Touches:** `crates/settings`, `crates/engine/src/proxy.rs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-settings -p wirecatch-engine`, `cargo clippy -p wirecatch-settings -p wirecatch-engine`
- **Status:** merged

### PR-37 fix(core): keep a name when a link becomes a page

- **Branch:** `fix/page-keeps-name`
- **Items:** 37 (P2), 175 (P3). Highest: P2.
- **Effort:** medium
- **Why:** Same step, `read_answered_page`: what a download loses when it gives way to its page.
- **Depends on:** none
- **Touches:** `crates/core`, `crates/desktop/src/add_dialog.rs`
- **Proof:** `B07-p2-tests.patch` holds 10 tests for several PRs: take `audit_a_typed_name_survives_a_page_without_a_video` and the helper it calls.
- **Checks:** `cargo test -p wirecatch-core -p wirecatch-desktop`, `cargo clippy -p wirecatch-core -p wirecatch-desktop`
- **Status:** merged

### PR-38 fix(media): read all of yt-dlp's page description

- **Branch:** `fix/read-whole-page-info`
- **Items:** 38 (P2), 61 (P2). Highest: P2.
- **Effort:** medium
- **Why:** Same types, `Info` and `Format`: both read yt-dlp's description too narrowly, and an audio format can pass as video through either.
- **Depends on:** none
- **Touches:** `crates/media`
- **Proof:** `B07-p2-tests.patch` holds 10 tests for several PRs: take the two `audit_top_level_` HLS and headers tests, `audit_top_level_audio_is_offered_as_audio` and `audit_a_page_with_several_videos_is_not_nothing`.
- **Checks:** `cargo test -p wirecatch-media`, `cargo clippy -p wirecatch-media`
- **Status:** merged

### PR-39 fix(engine): reuse part files under other names

- **Branch:** `fix/find-own-part-file`
- **Items:** 39 (P2). Highest: P2.
- **Effort:** medium
- **Why:** One item, and it brings the fault harness in.
- **Depends on:** none
- **Touches:** `crates/engine`, `crates/cli`, `crates/test-support`
- **Proof:** `FT-kill.patch` holds the fault harness and 12 tests, 2 of them failing; keep it as a permanent test, as the plan asks. Every `FT-` patch carries its own copy of the harness, so they do not stack. `FT-ranges`, `FT-change`, `FT-large` and `FT-manager-kill` pass today and belong to no item, so PR-120 keeps them.
- **Checks:** `cargo test -p wirecatch-engine`, `cargo test -p wirecatch-cli --test fault_kill`, `cargo clippy -p wirecatch-engine -p wirecatch-cli -p wirecatch-test-support`, `node scripts/gate.mjs unused-deps`
- **Status:** merged

### PR-40 fix(core): report whether the add-on works

- **Branch:** `fix/addon-usable`
- **Items:** 40 (P2), 53 (P2). Highest: P2.
- **Effort:** medium
- **Why:** Same form, `MediaForm`: item 53 reads the fact item 40 adds, and is blocked by it.
- **Depends on:** none
- **Touches:** `crates/protocol`, `packages/protocol`, `crates/core`, `crates/desktop/src/download_windows`, `crates/desktop/src/main_window/settings`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-core -p wirecatch-desktop`, `cargo clippy -p wirecatch-core -p wirecatch-desktop`, `pnpm protocol:generate`, `pnpm gate` once on the final tree, since `crates/protocol` is a wide change.
- **Status:** open

### PR-41 fix(desktop): keep notices in step with downloads

- **Branch:** `fix/notices-follow-downloads`
- **Items:** 41 (P2), 123 (P2), 232 (P3). Highest: P2.
- **Effort:** medium
- **Why:** All are a notice or the complete window going stale after what it tells of has changed, through `Teller`, `outdated_by` and `follow`.
- **Depends on:** none
- **Touches:** `crates/shell`, `crates/desktop/src/notices.rs`, `crates/desktop/src/download_windows`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-shell -p wirecatch-desktop`, `cargo clippy -p wirecatch-shell -p wirecatch-desktop`
- **Status:** open

### PR-42 fix(engine): hold FTP to the local network rule

- **Branch:** `fix/ftp-local-network`
- **Items:** 42 (P3), 43 (P2). Highest: P2.
- **Effort:** medium
- **Why:** Item 42 merges the rule's two copies in `net_guard.rs` so item 43 can call it; apart they would touch the same lines twice.
- **Depends on:** none
- **Touches:** `crates/engine`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-engine`, `cargo clippy -p wirecatch-engine`
- **Status:** merged

### PR-43 refactor(sites): merge the host-under-site checks

- **Branch:** `refactor/one-host-under-site`
- **Items:** 44 (P3). Highest: P3.
- **Effort:** medium
- **Why:** One item, a refactor across crates that PR-44 builds on.
- **Order:** A P3 among the P2s because the fix in PR-44 needs one rule.
- **Depends on:** none
- **Touches:** `crates/sites`, `crates/queues`, `crates/media`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-sites -p wirecatch-queues -p wirecatch-media`, `cargo clippy -p wirecatch-sites -p wirecatch-queues -p wirecatch-media`
- **Status:** merged

### PR-44 fix(engine): normalise ftps hosts when parsing

- **Branch:** `fix/normal-ftps-hosts`
- **Items:** 45 (P2). Highest: P2.
- **Effort:** small
- **Why:** One item.
- **Depends on:** PR-43
- **Touches:** `crates/engine`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-engine`, `cargo clippy -p wirecatch-engine`
- **Status:** open

### PR-45 fix: hide only what is secret in a log line

- **Branch:** `fix/log-redaction-edges`
- **Items:** 46 (P2), 222 (P3). Highest: P2.
- **Effort:** small
- **Why:** Same redaction scanners and the same sample file: one hides too little, the other too much.
- **Depends on:** none
- **Touches:** `crates/logging`, `crates/test-support/data/redaction.json`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-logging -p wirecatch-test-support`, `cargo clippy -p wirecatch-logging`, `npx vitest run apps/extension/tests/redact.test.ts`, since the extension loads the same samples. The log proposes adding apostrophe samples to `redaction.json`.
- **Status:** merged

### PR-46 fix(core): keep link passwords out of questions

- **Branch:** `fix/question-link-password`
- **Items:** 47 (P2). Highest: P2.
- **Effort:** medium
- **Why:** One item: the real and the shown link are told apart once.
- **Depends on:** PR-45
- **Touches:** `crates/core`, `crates/desktop/src/text.rs`, `crates/desktop/src/main_window`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-core -p wirecatch-desktop`, `cargo clippy -p wirecatch-core -p wirecatch-desktop`
- **Status:** open

### PR-47 fix(core): see finished files beside resume files

- **Branch:** `fix/finished-beside-resume`
- **Items:** 48 (P2), 174 (P3). Highest: P2.
- **Effort:** small
- **Why:** Same function, `finished_size`; item 48 blocks item 174.
- **Depends on:** PR-04
- **Touches:** `crates/core`
- **Proof:** `Xcrash-and-resume-2.patch`
- **Checks:** `cargo test -p wirecatch-core`, `cargo clippy -p wirecatch-core`
- **Status:** open

### PR-48 fix(media): make any video title safe as a name

- **Branch:** `fix/safe-video-titles`
- **Items:** 49 (P2), 95 (P2). Highest: P2.
- **Effort:** small
- **Why:** Same behaviour: a title damaged on its way to a file name, through `file_name`.
- **Depends on:** none
- **Touches:** `crates/media`
- **Proof:** `B07-p2-tests.patch` holds 10 tests for several PRs: take `audit_a_title_keeps_what_is_before_its_slash`.
- **Checks:** `cargo test -p wirecatch-media`, `cargo clippy -p wirecatch-media`
- **Status:** merged

### PR-49 fix(messages): give advice that fits the failure

- **Branch:** `fix/advice-fits-failure`
- **Items:** 51 (P2), 105 (P2), 106 (P2), 206 (P3). Highest: P2.
- **Effort:** medium
- **Why:** All are in `describe` and `disk_failure`: a message offering a fix that does not apply. Item 51 also carries the wrong-size update wording from the roadmap.
- **Depends on:** none
- **Touches:** `crates/messages`, `crates/core`
- **Proof:** `28-2.patch`, Windows only, for item 105.
- **Checks:** `cargo test -p wirecatch-messages -p wirecatch-core`, `cargo clippy -p wirecatch-messages -p wirecatch-core`
- **Status:** open

### PR-50 fix(store): keep a set-aside list for a full month

- **Branch:** `fix/set-aside-month`
- **Items:** 52 (P2), 156 (P3), 237 (P3). Highest: P2.
- **Effort:** small
- **Why:** Same crate: the fix, the missing tests of stored forms, and a one-line helper cleanup in the file those tests live in.
- **Depends on:** none
- **Touches:** `crates/store`
- **Proof:** `03-5.patch`. Apply it alone: it does not stack with the other store proofs.
- **Checks:** `cargo test -p wirecatch-store`, `cargo clippy -p wirecatch-store`
- **Status:** open

### PR-51 fix(desktop): correct what download questions say

- **Branch:** `fix/download-questions`
- **Items:** 54 (P2), 122 (P2), 231 (P3). Highest: P2.
- **Effort:** medium
- **Why:** All are what a download window shows or sends while it asks about a file, in `DownloadWindow`.
- **Depends on:** none
- **Touches:** `crates/desktop/src/download_windows`, `crates/desktop/src/main_window/list.rs`, `crates/desktop/src/text.rs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop`, `cargo clippy -p wirecatch-desktop`
- **Status:** open

### PR-52 fix(desktop): give download windows the right fix

- **Branch:** `fix/window-fix-buttons`
- **Items:** 55 (P2), 81 (P2). Highest: P2.
- **Effort:** medium
- **Why:** Same function, `progress_buttons`: what a click on a window's buttons does.
- **Depends on:** none
- **Touches:** `crates/desktop/src/download_windows`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop`, `cargo clippy -p wirecatch-desktop`. If `crates/protocol` changes, `pnpm protocol:generate` and `pnpm gate` once on the final tree.
- **Status:** open

### PR-53 fix(desktop): make settings actions answer back

- **Branch:** `fix/settings-answers`
- **Items:** 57 (P2), 127 (P2), 230 (P3). Highest: P2.
- **Effort:** medium
- **Why:** Same behaviour on the settings pages: an action that does nothing visible, or a value that shows empty.
- **Depends on:** none
- **Touches:** `crates/desktop/src/files.rs`, `crates/desktop/src/main_window`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop`, `cargo clippy -p wirecatch-desktop`
- **Status:** open

### PR-54 fix(core): handle typed names in media downloads

- **Branch:** `fix/typed-media-names`
- **Items:** 58 (P2), 132 (P2). Highest: P2.
- **Effort:** medium
- **Why:** Same behaviour: a typed name deciding too much through `insert_chosen`, `final_path` and `joined_container`.
- **Depends on:** none
- **Touches:** `crates/core`, `crates/streams`, `crates/media`, `clippy.toml`
- **Proof:** `17-3.patch` for item 58. Item 132 has none.
- **Checks:** `cargo test -p wirecatch-core -p wirecatch-streams -p wirecatch-media`, `cargo clippy -p wirecatch-core -p wirecatch-streams -p wirecatch-media`. The log proposes a `disallowed-methods` entry for `Path::with_extension`; that is lint config, so run `node scripts/gate.mjs rust-lint` once on the final tree.
- **Status:** open

### PR-55 fix(desktop): follow an installer through setup

- **Branch:** `fix/installer-lifecycle`
- **Items:** 59 (P2), 72 (P2). Highest: P2.
- **Effort:** medium
- **Why:** Same function, `install`, and the same question: what happens to a downloaded installer afterwards.
- **Depends on:** none
- **Touches:** `crates/core`, `crates/shell/src/updates.rs`, `crates/desktop/src/app_update.rs`, `crates/desktop/packaging/windows`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-core -p wirecatch-shell -p wirecatch-desktop`, `cargo clippy -p wirecatch-core -p wirecatch-shell -p wirecatch-desktop`. A real update has never run, so say what could not be tested.
- **Status:** merged

### PR-56 fix(core): offer updates only when they can work

- **Branch:** `fix/honest-update-offers`
- **Items:** 60 (P2), 185 (P3), 186 (P3), 187 (P3). Highest: P2.
- **Effort:** medium
- **Why:** All are what `check_app_update` decides to offer, with the tests that hold it.
- **Depends on:** none
- **Touches:** `crates/core`, `crates/desktop/src/app_update.rs`, `crates/desktop/src/main_window`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-core -p wirecatch-desktop`, `cargo clippy -p wirecatch-core -p wirecatch-desktop`
- **Status:** open

### PR-57 fix(streams): stop trusting bad work folder data

- **Branch:** `fix/work-folder-trust`
- **Items:** 62 (P2), 74 (P2). Highest: P2.
- **Effort:** medium
- **Why:** Same behaviour: what a stream's work folder keeps counts as good when it is not, in `Log::read` and `WorkDir::open`.
- **Depends on:** none
- **Touches:** `crates/streams`, `crates/media/src/describe.rs`
- **Proof:** `Xsecond-review-disk-writes-4.patch` for item 62. Item 74 has none.
- **Checks:** `cargo test -p wirecatch-streams -p wirecatch-media`, `cargo clippy -p wirecatch-streams -p wirecatch-media`
- **Status:** merged

### PR-58 fix(streams): report how a program really ended

- **Branch:** `fix/program-end-report`
- **Items:** 63 (P2), 77 (P2), 157 (P3). Highest: P2.
- **Effort:** medium
- **Why:** Same module, `process/run.rs`: blank lines, a missing library and a held output all hide why a program stopped.
- **Depends on:** none
- **Touches:** `crates/streams/src/process`, `crates/streams/tests`, `crates/media`
- **Proof:** `13-1.patch`, 2 tests, for item 63. The others have none.
- **Checks:** `cargo test -p wirecatch-streams -p wirecatch-media`, `cargo clippy -p wirecatch-streams -p wirecatch-media`
- **Status:** merged

### PR-59 fix(cli): report stops, speed, errors correctly

- **Branch:** `fix/cli-reports`
- **Items:** 65 (P2), 91 (P2), 182 (P3). Highest: P2.
- **Effort:** medium
- **Why:** All three are small faults in what `crates/cli/src/main.rs` tells the person.
- **Depends on:** none
- **Touches:** `crates/cli`, `crates/engine/src/error.rs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-cli -p wirecatch-engine`, `cargo clippy -p wirecatch-cli -p wirecatch-engine`
- **Status:** merged

### PR-60 fix(core): apply proxy exceptions to yt-dlp

- **Branch:** `fix/ytdlp-proxy-exceptions`
- **Items:** 68 (P3), 69 (P2). Highest: P2.
- **Effort:** small
- **Why:** Item 68 removes the unused copy of the same rule that item 69 fixes, and blocks it.
- **Depends on:** none
- **Touches:** `crates/settings`, `crates/core`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-settings -p wirecatch-core`, `cargo clippy -p wirecatch-settings -p wirecatch-core`
- **Status:** merged

### PR-61 fix(core): move path checks off async threads

- **Branch:** `fix/path-checks-off-async`
- **Items:** 70 (P2). Highest: P2.
- **Effort:** medium
- **Why:** One item.
- **Depends on:** none
- **Touches:** `crates/core`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-core`, `cargo clippy -p wirecatch-core`
- **Status:** merged

### PR-62 fix(engine): handle resume files more safely

- **Branch:** `fix/safer-resume-files`
- **Items:** 71 (P2), 88 (P2), 89 (P2), 158 (P3). Highest: P2.
- **Effort:** medium
- **Why:** All four are how a resume file is read, deleted or written, mostly in `Destination::prepare` and `save`.
- **Depends on:** none
- **Touches:** `crates/engine`, `crates/streams/src/workdir.rs`, `crates/core/src/naming.rs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-engine -p wirecatch-streams -p wirecatch-core`, `cargo clippy -p wirecatch-engine -p wirecatch-streams -p wirecatch-core`
- **Status:** open

### PR-63 fix(desktop): make an AppImage restart reliable

- **Branch:** `fix/appimage-restart`
- **Items:** 73 (P2), 184 (P3). Highest: P2.
- **Effort:** medium
- **Why:** Both are the AppImage hand-over: `replace_appimage` and the restart after it.
- **Depends on:** none
- **Touches:** `crates/shell/src/updates.rs`, `crates/desktop/src/lifecycle.rs`, `crates/desktop/src/main.rs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-shell -p wirecatch-desktop`, `cargo clippy -p wirecatch-shell -p wirecatch-desktop`, `node scripts/gate.mjs clippy-linux`. This is Linux only and has not been run.
- **Status:** merged

### PR-64 fix(core): read the page again after a refusal

- **Branch:** `fix/reread-refused-page`
- **Items:** 75 (P2), 134 (P2), 145 (P3). Highest: P2.
- **Effort:** medium
- **Why:** Same code, `page_attempt` and `fetch_part`: one rule for reading the page again, plus a one-line fix in the same HLS branch.
- **Depends on:** none
- **Touches:** `crates/core`, `crates/media`
- **Proof:** `B07-p2-tests.patch` holds 10 tests for several PRs: take `audit_a_refused_hls_address_reads_the_page_again`.
- **Checks:** `cargo test -p wirecatch-core -p wirecatch-media`, `cargo clippy -p wirecatch-core -p wirecatch-media`
- **Status:** open

### PR-65 fix(desktop): show all an import will change

- **Branch:** `fix/full-import-preview`
- **Items:** 76 (P2), 148 (P3), 207 (P3). Highest: P2.
- **Effort:** medium
- **Why:** Same behaviour: what an import does against what it says. The `every_video` half of item 148 has no tie to import and may be split off.
- **Depends on:** none
- **Touches:** `crates/protocol`, `packages/protocol`, `crates/core`, `crates/settings`, `crates/desktop/src/main_window/backup_dialog.rs`, `crates/desktop/src/download_windows`
- **Proof:** `19-2.patch` for item 207. The others have none.
- **Checks:** `cargo test -p wirecatch-core -p wirecatch-settings -p wirecatch-desktop`, `cargo clippy -p wirecatch-core -p wirecatch-settings -p wirecatch-desktop`, `pnpm protocol:generate`, `pnpm gate` once on the final tree, since `crates/protocol` is a wide change.
- **Status:** open

### PR-66 fix(desktop): show moved downloads in their place

- **Branch:** `fix/moved-row-position`
- **Items:** 78 (P2). Highest: P2.
- **Effort:** medium
- **Why:** One item.
- **Depends on:** none
- **Touches:** `crates/desktop/src/library.rs`, `crates/desktop/src/main_window`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop`, `cargo clippy -p wirecatch-desktop`. If `crates/protocol` changes, `pnpm protocol:generate` and `pnpm gate` once on the final tree.
- **Status:** open

### PR-67 fix(queues): save whether a queue is in a run

- **Branch:** `fix/saved-queue-run`
- **Items:** 79 (P2), 138 (P2). Highest: P2.
- **Effort:** medium
- **Why:** Same state, `Gate` and `has_work`; item 79 blocks item 138, whose only way out is Start now.
- **Depends on:** none
- **Touches:** `crates/queues`, `crates/core`, `crates/protocol`, `packages/protocol`, `crates/desktop/src/main_window/sidebar.rs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-queues -p wirecatch-core -p wirecatch-desktop`, `cargo clippy -p wirecatch-queues -p wirecatch-core -p wirecatch-desktop`, `pnpm protocol:generate`, `pnpm gate` once on the final tree, since `crates/protocol` is a wide change.
- **Status:** open

### PR-68 fix(desktop): keep one window per download

- **Branch:** `fix/one-window-per-download`
- **Items:** 80 (P2), 139 (P2). Highest: P2.
- **Effort:** medium
- **Why:** Same code, the `Registry` and `show_download`: finding the window that already shows a download.
- **Depends on:** none
- **Touches:** `crates/desktop/src/download_windows`, `crates/shell/src/download_windows.rs`, `crates/desktop/src/main_window/row_menu.rs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop -p wirecatch-shell`, `cargo clippy -p wirecatch-desktop -p wirecatch-shell`
- **Status:** open

### PR-69 fix(core): keep a row's answers after a restart

- **Branch:** `fix/answers-survive-restart`
- **Items:** 82 (P2), 83 (P2). Highest: P2.
- **Effort:** medium
- **Why:** Same field, `expect`: item 83 saves it and item 82 keeps a way to clear it, so item 82 must not come later.
- **Depends on:** PR-04
- **Touches:** `crates/core`, `crates/records`, `crates/store`
- **Proof:** `Xpath-normal-download-3.patch`, `05-6.patch`, `Xcrash-and-resume-5.patch`. The first and third append to `protocol.rs`, so apply one at a time.
- **Checks:** `cargo test -p wirecatch-core -p wirecatch-records -p wirecatch-store`, `cargo clippy -p wirecatch-core -p wirecatch-records -p wirecatch-store`
- **Status:** open

### PR-70 fix(core): answer a repeated capture once

- **Branch:** `fix/repeated-capture`
- **Items:** 86 (P2), 151 (P3), 173 (P3). Highest: P2.
- **Effort:** medium
- **Why:** All three are a capture meeting an earlier one of the same link, in `capture_arrived`, `join_unfinished` and `hold_for_confirmation`.
- **Depends on:** none
- **Touches:** `crates/core`
- **Proof:** `Xconcurrency-3-and-6.patch` holds 2 tests: take `two_clicks_at_once_make_one_download`; the other is for PR-05.
- **Checks:** `cargo test -p wirecatch-core`, `cargo clippy -p wirecatch-core`
- **Status:** merged

### PR-71 fix(engine): count transferred bytes correctly

- **Branch:** `fix/count-bytes`
- **Items:** 87 (P2), 177 (P3), 180 (P3). Highest: P2.
- **Effort:** small
- **Why:** All are what counts as received in a transfer, in `Fetcher` and `stream`, with one doc line that promises otherwise.
- **Depends on:** none
- **Touches:** `crates/engine`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-engine`, `cargo clippy -p wirecatch-engine`
- **Status:** open

### PR-72 fix(engine): read range answers as they are

- **Branch:** `fix/range-answers`
- **Items:** 90 (P2), 178 (P3). Highest: P2.
- **Effort:** small
- **Why:** Same behaviour: a short `206` and a `416` both read wrongly against the range asked for.
- **Depends on:** none
- **Touches:** `crates/engine`, `crates/cli`, `crates/test-support`
- **Proof:** `FT-sizes.patch` for item 90 holds the fault harness and 15 tests. Once PR-39 is merged the harness is there, so keep only `fault_sizes.rs`. Item 178 has none.
- **Checks:** `cargo test -p wirecatch-engine`, `cargo test -p wirecatch-cli --test fault_sizes`, `cargo clippy -p wirecatch-engine -p wirecatch-cli`, `node scripts/gate.mjs unused-deps` if the harness comes with it.
- **Status:** merged

### PR-73 fix(desktop): handle odd launches safely

- **Branch:** `fix/odd-launches`
- **Items:** 92 (P2), 131 (P2). Highest: P2.
- **Effort:** medium
- **Why:** Same start path in `main`: arguments and a launch handed to a copy still opening.
- **Depends on:** none
- **Touches:** `crates/desktop/src/main.rs`, `crates/desktop/src/backend.rs`, `crates/shell/src/launch.rs`, `clippy.toml`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop -p wirecatch-shell`, `cargo clippy -p wirecatch-desktop -p wirecatch-shell`. The log proposes a `disallowed-methods` entry for `std::env::args`; that is lint config, so run `node scripts/gate.mjs rust-lint` once on the final tree.
- **Status:** merged

### PR-74 fix(desktop): close the app before uninstalling

- **Branch:** `fix/uninstall-closes-app`
- **Items:** 93 (P2). Highest: P2.
- **Effort:** small
- **Why:** One item.
- **Depends on:** none
- **Touches:** `crates/desktop/packaging/windows/installer.iss`
- **Proof:** none
- **Checks:** Build with `node scripts/package.mjs` on Windows and uninstall while the app runs. No installer has been run yet, so say if this could not be done.
- **Status:** merged

### PR-75 fix: correct faults in the linux packages

- **Branch:** `fix/linux-packages`
- **Items:** 94 (P2), 188 (P3), 189 (P3), 190 (P3). Highest: P2.
- **Effort:** medium
- **Why:** All are how the Linux packages are built, stated and checked, mostly in `scripts/package.mjs`.
- **Depends on:** none
- **Touches:** `scripts/package.mjs`, `.github/workflows/release.yml`, `crates/desktop/PACKAGING.md`
- **Proof:** none
- **Checks:** `node --test "scripts/**/*.test.mjs"`, `node scripts/gate.mjs repo`. A real package build on Linux has not run, so say if this could not be done.
- **Status:** open

### PR-76 fix(media): track, limit yt-dlp's own downloads

- **Branch:** `fix/ytdlp-progress-limit`
- **Items:** 96 (P2), 98 (P2). Highest: P2.
- **Effort:** small
- **Why:** Same code, `ytdlp/download.rs`: what the app follows and sets while yt-dlp downloads by itself.
- **Depends on:** none
- **Touches:** `crates/media`, `crates/core`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-media -p wirecatch-core`, `cargo clippy -p wirecatch-media -p wirecatch-core`
- **Status:** open

### PR-77 fix(media): make quality offers match downloads

- **Branch:** `fix/quality-offers`
- **Items:** 97 (P2), 99 (P2), 197 (P3). Highest: P2.
- **Effort:** medium
- **Why:** Same code, `choice.rs`: how an offer is put together from a page's formats.
- **Depends on:** none
- **Touches:** `crates/media`
- **Proof:** `B07-p2-tests.patch` holds 10 tests for several PRs: take `audit_a_row_labelled_60_fps_downloads_60_fps`, `audit_best_audio_prefers_the_original_language` and `audit_huge_sizes_do_not_overflow`.
- **Checks:** `cargo test -p wirecatch-media`, `cargo clippy -p wirecatch-media`
- **Status:** open

### PR-78 fix(streams): read HLS masters, keys correctly

- **Branch:** `fix/hls-masters-keys`
- **Items:** 100 (P2), 101 (P2), 102 (P2), 198 (P3). Highest: P2.
- **Effort:** medium
- **Why:** All are small faults in reading an HLS playlist's tracks and key addresses, and 101 and 102 share a proof.
- **Depends on:** none
- **Touches:** `crates/streams`
- **Proof:** `14-4-and-14-5.patch` holds 3 tests, for items 101 and 102. The others have none.
- **Checks:** `cargo test -p wirecatch-streams`, `cargo clippy -p wirecatch-streams`
- **Status:** open

### PR-79 fix(desktop): tell loading from a failed list

- **Branch:** `fix/list-open-states`
- **Items:** 103 (P2), 135 (P2), 211 (P3). Highest: P2.
- **Effort:** medium
- **Why:** Same file, `empty.rs`: what the list area says and offers around opening.
- **Depends on:** none
- **Touches:** `crates/desktop/src/main_window/empty.rs`, `crates/desktop/src/main_window/mod.rs`, `crates/shell/src/host.rs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop -p wirecatch-shell`, `cargo clippy -p wirecatch-desktop -p wirecatch-shell`; look at the loading and failed states in both themes.
- **Status:** open

### PR-80 fix(core): correct how the add-on updates itself

- **Branch:** `fix/addon-updates`
- **Items:** 108 (P2), 109 (P2), 110 (P2), 159 (P3), 208 (P3), 209 (P3). Highest: P2.
- **Effort:** medium
- **Why:** All are small faults in the add-on's update steps, around `addon_plan`, `check_addon_updates` and `swap_in`; item 108 blocks item 109.
- **Depends on:** none
- **Touches:** `crates/core`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-core`, `cargo clippy -p wirecatch-core`
- **Status:** merged

### PR-81 fix(desktop): act on the rows the view shows

- **Branch:** `fix/selection-commands`
- **Items:** 111 (P2), 136 (P2), 212 (P3). Highest: P2.
- **Effort:** medium
- **Why:** 111 and 136 are commands that ignore `selection::each` and `selected_rows`; 212 is three unread bindings in the same `confirm.rs`.
- **Depends on:** none
- **Touches:** `crates/desktop/src/main_window`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop`, `cargo clippy -p wirecatch-desktop`
- **Status:** open

### PR-82 fix(desktop): tidy navigation after a page opens

- **Branch:** `fix/navigation-state`
- **Items:** 112 (P2), 162 (P3). Highest: P2.
- **Effort:** small
- **Why:** Same behaviour: the sidebar or drawer showing the wrong state once a page has changed.
- **Depends on:** none
- **Touches:** `crates/desktop/src/main_window`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop`, `cargo clippy -p wirecatch-desktop`; look at the drawer and the sidebar in both themes.
- **Status:** open

### PR-83 fix(desktop): offer only actions that apply

- **Branch:** `fix/offers-that-apply`
- **Items:** 113 (P2), 163 (P3), 166 (P3). Highest: P2.
- **Effort:** medium
- **Why:** Same behaviour: the row, the menu, the details and the empty pages each decide what to offer.
- **Depends on:** none
- **Touches:** `crates/desktop/src/main_window`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop`, `cargo clippy -p wirecatch-desktop`
- **Status:** open

### PR-84 fix(sites): correct what counts as a local page

- **Branch:** `fix/what-counts-local`
- **Items:** 114 (P2), 215 (P3), 244 (P3). Highest: P2.
- **Effort:** small
- **Why:** All are `is_local_by_itself` and what relies on it, with the test against the real check.
- **Depends on:** none
- **Touches:** `crates/sites`, `crates/core`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-sites -p wirecatch-core`, `cargo clippy -p wirecatch-sites`
- **Status:** open

### PR-85 fix(desktop): say what the proxy form will save

- **Branch:** `fix/proxy-form-truth`
- **Items:** 116 (P2), 221 (P3). Highest: P2.
- **Effort:** small
- **Why:** Same form, `ProxyForm`: it promises or saves something other than what was typed.
- **Depends on:** none
- **Touches:** `crates/desktop/src/main_window/proxy_dialog.rs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop`, `cargo clippy -p wirecatch-desktop`
- **Status:** open

### PR-86 fix(queues): hold queue slots to the real limit

- **Branch:** `fix/queue-slots`
- **Items:** 117 (P2), 118 (P2). Highest: P2.
- **Effort:** small
- **Why:** Same behaviour: a plan that counts slots wrongly, one way left empty and the other way over the limit.
- **Depends on:** none
- **Touches:** `crates/queues`, `crates/core`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-queues -p wirecatch-core`, `cargo clippy -p wirecatch-queues -p wirecatch-core`
- **Status:** merged

### PR-87 fix(queues): make rules match the way they read

- **Branch:** `fix/rules-match-text`
- **Items:** 119 (P2), 223 (P3), 224 (P3). Highest: P2.
- **Effort:** small
- **Why:** Same behaviour: a rule accepted or explained one way and applied another, in `cleaned`, `HELP` and `category_of`.
- **Depends on:** none
- **Touches:** `crates/queues`, `crates/settings`, `crates/desktop/src/main_window/queue_rules.rs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-queues -p wirecatch-settings`, `cargo clippy -p wirecatch-queues -p wirecatch-settings -p wirecatch-desktop`
- **Status:** merged

### PR-88 fix(desktop): fix small faults in queue dialogs

- **Branch:** `fix/queue-dialogs`
- **Items:** 120 (P2), 121 (P2), 165 (P3). Highest: P2.
- **Effort:** small
- **Why:** Same dialogs, each a one-place fix: Enter in the delete question, a limit changed on save, and the first cursor.
- **Depends on:** none
- **Touches:** `crates/desktop/src/main_window`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop`, `cargo clippy -p wirecatch-desktop`
- **Status:** open

### PR-89 fix(desktop): make tray actions answer correctly

- **Branch:** `fix/tray-actions`
- **Items:** 124 (P2), 160 (P3). Highest: P2.
- **Effort:** small
- **Why:** Same code, `on_click` and `menu` in `tray.rs`.
- **Depends on:** none
- **Touches:** `crates/desktop/src/tray.rs`, `crates/shell`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop -p wirecatch-shell`, `cargo clippy -p wirecatch-desktop -p wirecatch-shell`
- **Status:** open

### PR-90 fix(desktop): show a processing step as it is

- **Branch:** `fix/processing-step`
- **Items:** 125 (P2), 228 (P3). Highest: P2.
- **Effort:** small
- **Why:** Same file, `text.rs`, and the same state: 100% beside Converting, and Merging for every step.
- **Depends on:** none
- **Touches:** `crates/desktop/src/text.rs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop`, `cargo clippy -p wirecatch-desktop`
- **Status:** open

### PR-91 fix(desktop): correct wording of app messages

- **Branch:** `fix/app-message-wording`
- **Items:** 126 (P2), 161 (P3). Highest: P2.
- **Effort:** small
- **Why:** Both reword messages in `crates/shell/src/api.rs` and its neighbours.
- **Depends on:** none
- **Touches:** `crates/shell`, `crates/core/src/naming.rs`, `scripts/gate`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-shell -p wirecatch-core`, `cargo clippy -p wirecatch-shell -p wirecatch-core`. The log proposes a gate check for the product name in string literals; a gate script change is wide, so run `pnpm gate` once on the final tree if it is added.
- **Status:** open

### PR-92 fix: repair the repo's check scripts

- **Branch:** `fix/check-scripts`
- **Items:** 129 (P2), 191 (P3), 238 (P3), 239 (P3), 240 (P3), 241 (P3). Highest: P2.
- **Effort:** medium
- **Why:** All are small faults in the repo's scripts and checked config, and the unchecked icon needs a check of the same kind.
- **Depends on:** none
- **Touches:** `scripts`, `config`, `Cargo.toml`, `.githooks`, `crates/desktop/assets/icons`
- **Proof:** none
- **Checks:** `node --test "scripts/**/*.test.mjs"`, `node scripts/gate.mjs repo`, `pnpm product:generate` if `config/product.toml` changes, `pnpm gate` once on the final tree, since gate scripts are a wide change.
- **Status:** merged

### PR-93 fix(engine): lock the part file during a run

- **Branch:** `fix/part-file-lock`
- **Items:** 130 (P2). Highest: P2.
- **Effort:** medium
- **Why:** One item. It shares `plan_in_folder` with PR-39 but is a separate change.
- **Depends on:** none
- **Touches:** `crates/engine`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-engine`, `cargo clippy -p wirecatch-engine`, `node scripts/gate.mjs clippy-linux`
- **Status:** merged

### PR-94 fix(core): keep a video's work until it is placed

- **Branch:** `fix/work-until-placed`
- **Items:** 133 (P2), 169 (P3). Highest: P2.
- **Effort:** medium
- **Why:** Same step between placing a video and deleting its work: a failed place and a crash there both lose track.
- **Depends on:** PR-04
- **Touches:** `crates/core`, `crates/streams`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-core -p wirecatch-streams`, `cargo clippy -p wirecatch-core -p wirecatch-streams`
- **Status:** open

### PR-95 fix(core): apply the local rule to yt-dlp reads

- **Branch:** `fix/ytdlp-local-rule`
- **Items:** 137 (P2). Highest: P2.
- **Effort:** medium
- **Why:** One item, which needs a rule of its own because the engine's guard cannot reach another process.
- **Depends on:** none
- **Touches:** `crates/core`, `crates/media`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-core -p wirecatch-media`, `cargo clippy -p wirecatch-core -p wirecatch-media`
- **Status:** open

### PR-96 fix(engine): check free space before a download

- **Branch:** `fix/free-space-check`
- **Items:** 140 (P2). Highest: P2.
- **Effort:** medium
- **Why:** One item.
- **Depends on:** none
- **Touches:** `crates/engine`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-engine`, `cargo clippy -p wirecatch-engine`, `node scripts/gate.mjs clippy-linux`. A full disk through a real download is untested, so say if this could not be done.
- **Status:** merged

### PR-97 fix(core): record every file of a move up front

- **Branch:** `fix/record-moves`
- **Items:** 141 (P2), 201 (P3). Highest: P2.
- **Effort:** medium
- **Why:** Same code, `move_files`: a record of a move has to list the subtitle files that a move leaves behind today.
- **Depends on:** PR-04, PR-08
- **Touches:** `crates/core`, `crates/records`, `crates/store`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-core -p wirecatch-records -p wirecatch-store`, `cargo clippy -p wirecatch-core -p wirecatch-records -p wirecatch-store`
- **Status:** open

### PR-98 chore: remove code nothing calls

- **Branch:** `chore/remove-dead-commands`
- **Items:** 142 (P3), 235 (P3), 236 (P3). Highest: P3.
- **Effort:** medium
- **Why:** Dead-code cleanup in the app's interface layers, with a wrong comment on the same event loop. Item 142 needs a note on D43.1 and new decisions for D65.2 and D50.4.
- **Depends on:** none
- **Touches:** `crates/shell`, `crates/protocol`, `packages/protocol`, `crates/core`, `crates/server`, `crates/e2e`, `scripts/gate`, `docs/DECISIONS.md`
- **Proof:** none
- **Checks:** `pnpm protocol:generate`, `pnpm gate` once on the final tree, since `crates/protocol` is a wide change. The log proposes a gate check listing public items of `crates/shell` with no caller.
- **Status:** open

### PR-99 test: wait under PATIENCE in every test

- **Branch:** `test/patience-waits`
- **Items:** 143 (P3), 242 (P3). Highest: P3.
- **Effort:** medium
- **Why:** Same fix: every wait in a test goes under `PATIENCE`, and the gate learns to see the ones it misses.
- **Depends on:** none
- **Touches:** `crates/engine/tests`, `crates/core/tests`, `crates/bridge/tests`, `crates/e2e`, `scripts/gate/test-timing.mjs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-engine -p wirecatch-core -p wirecatch-bridge -p wirecatch-e2e`, `node --test scripts/gate/test-timing.test.mjs`, `node scripts/gate.mjs test-timing`. The log proposes widening `test-timing` to `recv_timeout`, a local `Duration` passed to a wait, and a measured time compared through a method; a gate script change is wide, so run `pnpm gate` once on the final tree.
- **Status:** open

### PR-100 test: stop racing real-clock limits in tests

- **Branch:** `test/no-clock-races`
- **Items:** 144 (P3), 167 (P3). Highest: P3.
- **Effort:** medium
- **Why:** Same fault across crates: a test that depends on timing the machine does not promise. No pattern check can see these.
- **Depends on:** none
- **Touches:** `crates/store`, `crates/sites`, `crates/streams/tests`, `crates/bridge/tests`, `crates/core/tests`, `crates/engine/tests`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-store -p wirecatch-sites -p wirecatch-streams -p wirecatch-bridge -p wirecatch-core -p wirecatch-engine`
- **Status:** open

### PR-101 test: share test values through test-support

- **Branch:** `test/shared-test-values`
- **Items:** 146 (P3), 147 (P3), 149 (P3), 183 (P3). Highest: P3.
- **Effort:** medium
- **Why:** Test cleanups by area: repeated names, range parsing and fixtures move to one place in `crates/test-support`, and one bridge test starts comparing what it says.
- **Depends on:** none
- **Touches:** `crates/test-support`, `crates/bridge`, `crates/ipc/tests`, `crates/shell/src/launch.rs`, `crates/core/tests`, `crates/core/src/manager/media_run/tests.rs`, `crates/e2e`, `crates/engine/src/http3/tests.rs`, `scripts/gate/rust.mjs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-test-support -p wirecatch-bridge -p wirecatch-ipc -p wirecatch-core -p wirecatch-engine -p wirecatch-e2e`, `node --test scripts/gate/rust.test.mjs`
- **Status:** open

### PR-102 fix(desktop): show startup notices only once

- **Branch:** `fix/startup-notices-once`
- **Items:** 150 (P3), 214 (P3). Highest: P3.
- **Effort:** medium
- **Why:** Same function, `tell_startup_notices`: when its notices show and how they read.
- **Depends on:** none
- **Touches:** `crates/desktop/src/library.rs`, `crates/desktop/src/main_window/notices_bar.rs`, `crates/desktop/src/download_windows`, `crates/shell/src/download_windows.rs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop -p wirecatch-shell`, `cargo clippy -p wirecatch-desktop -p wirecatch-shell`; look at the notice in both themes.
- **Status:** open

### PR-103 fix(core): let a person's stop win

- **Branch:** `fix/person-stop-wins`
- **Items:** 152 (P3), 172 (P3). Highest: P3.
- **Effort:** small
- **Why:** Same rule, `Entry::stop`: what overrides a stop a person chose.
- **Depends on:** none
- **Touches:** `crates/core`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-core`, `cargo clippy -p wirecatch-core`
- **Status:** merged

### PR-104 fix(engine): tighten network guard edge cases

- **Branch:** `fix/net-guard-edges`
- **Items:** 154 (P3), 155 (P3), 216 (P3), 217 (P3). Highest: P3.
- **Effort:** medium
- **Why:** All are `net_guard.rs` and the proxy docs and tests around it, where the tests cannot fail and the docs have aged.
- **Depends on:** none
- **Touches:** `crates/engine`, `crates/core/src/manager/media_run`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-engine -p wirecatch-core`, `cargo clippy -p wirecatch-engine -p wirecatch-core`
- **Status:** open

### PR-105 fix(media): word program failures plainly

- **Branch:** `fix/word-program-failures`
- **Items:** 164 (P3). Highest: P3.
- **Effort:** small
- **Why:** One item, the roadmap's "A failed video shows the add-on's raw output".
- **Depends on:** none
- **Touches:** `crates/media`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-media`, `cargo clippy -p wirecatch-media`
- **Status:** open

### PR-106 refactor(desktop): share repeated form logic

- **Branch:** `refactor/share-form-logic`
- **Items:** 168 (P3). Highest: P3.
- **Effort:** medium
- **Why:** One item: one helper for each repeated piece, including the 15 copies of `Backend::run`'s failure match.
- **Depends on:** none
- **Touches:** `crates/desktop`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop`, `cargo clippy -p wirecatch-desktop`
- **Status:** open

### PR-107 fix: write kept files whole before replacing

- **Branch:** `fix/write-kept-files-whole`
- **Items:** 170 (P3), 171 (P3). Highest: P3.
- **Effort:** small
- **Why:** Same finding: two writers that go around `whole_file::replace` and lose what it guarantees.
- **Depends on:** none
- **Touches:** `crates/core/src/manager/preferences.rs`, `crates/bridge/src/install/host.rs`, `clippy.toml`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-core -p wirecatch-bridge`, `cargo clippy -p wirecatch-core -p wirecatch-bridge`. The log proposes a `disallowed-methods` entry for `std::fs::write`; that is lint config, so run `node scripts/gate.mjs rust-lint` once on the final tree.
- **Status:** open

### PR-108 refactor(engine): remove code with no effect

- **Branch:** `refactor/engine-no-op-code`
- **Items:** 179 (P3), 181 (P3). Highest: P3.
- **Effort:** small
- **Why:** Dead-code cleanup by area: a comparison and a sleep in the engine that change nothing.
- **Depends on:** none
- **Touches:** `crates/engine`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-engine`, `cargo clippy -p wirecatch-engine`
- **Status:** merged

### PR-109 refactor(streams): remove leftovers from the crate

- **Branch:** `refactor/streams-leftovers`
- **Items:** 192 (P3), 193 (P3), 194 (P3). Highest: P3.
- **Effort:** small
- **Why:** Dead-code cleanup by area: a copied loop, a layout never released and an unused dependency.
- **Depends on:** none
- **Touches:** `crates/streams`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-streams`, `cargo clippy -p wirecatch-streams`, `node scripts/gate.mjs unused-deps`
- **Status:** open

### PR-110 test(core): repair media run tests, doc comments

- **Branch:** `test/media-run-tidy`
- **Items:** 195 (P3), 196 (P3). Highest: P3.
- **Effort:** small
- **Why:** Test and wording cleanup by area, all in media runs.
- **Depends on:** none
- **Touches:** `crates/core`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-core`, `cargo clippy -p wirecatch-core`
- **Status:** open

### PR-111 fix(streams): stop media programs on a signal

- **Branch:** `fix/media-programs-on-signal`
- **Items:** 199 (P3). Highest: P3.
- **Effort:** medium
- **Why:** One item, which extends the roadmap's "On Linux, yt-dlp and ffmpeg keep running after the app crashes".
- **Depends on:** none
- **Touches:** `crates/streams/src/process/tree.rs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-streams`, `cargo clippy -p wirecatch-streams`, `node scripts/gate.mjs clippy-linux`. This is Linux only and has not been run.
- **Status:** open

### PR-112 fix(desktop): correct system registration details

- **Branch:** `fix/system-registration`
- **Items:** 202 (P3), 203 (P3), 204 (P3). Highest: P3.
- **Effort:** small
- **Why:** All are how the app registers itself with the system, for the browser and at sign-in.
- **Depends on:** none
- **Touches:** `crates/shell/src/bridge.rs`, `crates/shell/src/autostart.rs`, `crates/bridge/src/install`, `crates/desktop/src/sign_in.rs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-shell -p wirecatch-bridge -p wirecatch-desktop`, `cargo clippy -p wirecatch-shell -p wirecatch-bridge -p wirecatch-desktop`
- **Status:** merged

### PR-113 fix(desktop): give every window a native title

- **Branch:** `fix/native-window-title`
- **Items:** 205 (P3). Highest: P3.
- **Effort:** small
- **Why:** One item.
- **Depends on:** none
- **Touches:** `crates/desktop/src/main.rs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop`, `cargo clippy -p wirecatch-desktop`
- **Status:** merged

### PR-114 fix(desktop): steady the list under the pointer

- **Branch:** `fix/list-under-pointer`
- **Items:** 210 (P3), 213 (P3). Highest: P3.
- **Effort:** small
- **Why:** Same behaviour: the list moving or covering what the pointer is about to click.
- **Depends on:** none
- **Touches:** `crates/desktop/src/main_window`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop`, `cargo clippy -p wirecatch-desktop`; look at the list in both themes.
- **Status:** open

### PR-115 fix(core): keep only finished proxy test results

- **Branch:** `fix/proxy-test-results`
- **Items:** 218 (P3), 219 (P3). Highest: P3.
- **Effort:** small
- **Why:** Same code, `keep_test`: the fix and the test that should have caught it.
- **Depends on:** none
- **Touches:** `crates/core`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-core`, `cargo clippy -p wirecatch-core`
- **Status:** merged

### PR-116 fix(desktop): correct what queue pages say

- **Branch:** `fix/queue-page-text`
- **Items:** 225 (P3), 226 (P3), 227 (P3). Highest: P3.
- **Effort:** small
- **Why:** Wording cleanup by area: numbers, the proxy name and the facts line of a queue page.
- **Depends on:** none
- **Touches:** `crates/desktop/src/main_window`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop`, `cargo clippy -p wirecatch-desktop`; look at a queue page in both themes.
- **Status:** open

### PR-117 fix(desktop): keep a system accent readable

- **Branch:** `fix/system-accent-contrast`
- **Items:** 229 (P3). Highest: P3.
- **Effort:** small
- **Why:** One item.
- **Depends on:** none
- **Touches:** `crates/desktop/src/theme.rs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-desktop`, `cargo clippy -p wirecatch-desktop`; look at it in both themes.
- **Status:** open

### PR-118 fix(protocol): correct two language names

- **Branch:** `fix/language-names`
- **Items:** 234 (P3). Highest: P3.
- **Effort:** small
- **Why:** One item, kept apart because `crates/protocol` is a wide change.
- **Depends on:** none
- **Touches:** `crates/protocol`, `packages/protocol`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-protocol`, `cargo clippy -p wirecatch-protocol`, `pnpm protocol:generate`, `pnpm gate` once on the final tree, since `crates/protocol` is a wide change.
- **Status:** open

### PR-119 fix(desktop): retry deleting a roaming data folder

- **Branch:** `fix/roaming-folder-retry`
- **Items:** 243 (P3). Highest: P3.
- **Effort:** small
- **Why:** One item.
- **Depends on:** none
- **Touches:** `crates/shell/src/host.rs`
- **Proof:** none
- **Checks:** `cargo test -p wirecatch-shell`, `cargo clippy -p wirecatch-shell`
- **Status:** merged

### PR-120 test(engine): keep the download fault tests

- **Branch:** `test/keep-fault-tests`
- **Items:** none. These tests pass today, and the audit plan keeps them as permanent regression tests.
- **Effort:** medium
- **Why:** Fault tests that no fix item carries, using the harness PR-39 brings in.
- **Depends on:** PR-39
- **Touches:** `crates/engine`, `crates/core`, `crates/test-support`
- **Proof:** `FT-ranges.patch`, `FT-change.patch`, `FT-large.patch` and `FT-manager-kill.patch`. Each carries its own copy of the harness, so take the tests and reuse the one from PR-39.
- **Checks:** `cargo test -p wirecatch-engine`, `cargo test -p wirecatch-core`, `cargo clippy -p wirecatch-engine -p wirecatch-core -p wirecatch-test-support`
- **Status:** merged
