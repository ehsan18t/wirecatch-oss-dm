# Verdicts: Mutation survivors

Line numbers in the table are those of `tools.md`, from commit `37603b330`. Findings cite functions.

Each `cargo-mutants` run used only the mutated crate's own tests, so a mutant can be missed there and still be caught by another crate's test. Those catches were read from the tests, not run. The run was on Windows, so a mutant in `#[cfg(unix)]` or `#[cfg(not(windows))]` code was never compiled into it; those are marked as not built, and Linux tests were read, not run.

Counts: 22 equivalent, 48 low value, 7 real gap. No mutant exposed a bug in the code.

The 18 timed-out engine mutants count as caught: each one loops forever or stalls a transfer, and nothing suggests otherwise.

## Missed mutants

| Mutant                                                                | Verdict    | Why                                                                                                                 |
| --------------------------------------------------------------------- | ---------- | ------------------------------------------------------------------------------------------------------------------- |
| `addons/archive.rs:293` `write_file` with `Ok((0, default))`          | Equivalent | Not built: `cfg(unix)`. On Linux, the unpack tests read size and content                                            |
| `addons/archive.rs:280` `Hashing::flush` with `Ok(())`                | Equivalent | `File::flush` does nothing                                                                                          |
| `addons/archive.rs:293` `write_file` with `Ok((1, default))`          | Equivalent | Not built: `cfg(unix)`, as above                                                                                    |
| `addons/archive.rs:333` `==` to `!=` in `copy_file`                   | Low value  | Only swaps "damaged" and "disk" wording, for bytes that already passed their checksum                               |
| `addons/manifest.rs:49` `ComponentId` `Display`                       | Low value  | `core/tests/manager/addons.rs` asserts the "yt-dlp ..., ffmpeg ..., Deno ..." text                                  |
| `addons/manifest.rs:104` `\|` to `^` in `from_hex`                    | Equivalent | High and low nibbles share no bit                                                                                   |
| `addons/manifest.rs:119` `Sha256Digest` `Debug`                       | Low value  | Debug text only                                                                                                     |
| `addons/manifest.rs:325` `for_release` drops `version`                | Low value  | Core add-on update tests fail: no update is seen                                                                    |
| `addons/manifest.rs:326` `for_release` drops `release`                | Low value  | Core add-on update tests read the new file's content                                                                |
| `addons/manifest.rs:505` `ytdlp_asset` with `""`                      | Equivalent | Not built: `cfg(not(windows))`. Its Windows twin is caught by the real checksum list test, which also runs on Linux |
| `addons/manifest.rs:505` `ytdlp_asset` with `"xyzzy"`                 | Equivalent | Not built, as above                                                                                                 |
| `addons/manifest.rs:511` `program_name` with `"xyzzy"`                | Low value  | One `format!`; the first real install fails loudly, with "missing from the archive" for Deno                        |
| `addons/manifest.rs:629` `asset_stem` with `""`                       | Low value  | Linux CI's ffmpeg step runs the path built from it, and fails                                                       |
| `addons/manifest.rs:629` `asset_stem` with `"xyzzy"`                  | Low value  | Same CI step                                                                                                        |
| `addons/signature.rs:65` `Fingerprint` `Display`                      | Low value  | The check reads `as_str`; display text only                                                                         |
| `updates/manifest.rs:72` `System::current` with `None`                | Low value  | `core` `app_update` tests unwrap it and expect a download                                                           |
| `updates/manifest.rs:86` `Arch::current` with `None`                  | Low value  | Same tests                                                                                                          |
| `updates/release.rs:38` `Release::asset` with `None`                  | Low value  | `core` `app_update` tests: no manifest is found                                                                     |
| `updates/manifest.rs:143` `&&` to `\|\|` in `is_sha256`               | Low value  | `fetch_app_update` still refuses it through `Sha256Digest::from_hex`, before any download                           |
| `updates/release.rs:38` `==` to `!=` in `Release::asset`              | Low value  | `core` `app_update` tests: the wrong asset fails the signature check                                                |
| `sites/local_network.rs:37` `private_network` with `Default`          | Low value  | `core` `run.rs` and `media_run` tests pin all three mappings                                                        |
| `sites/local_network.rs:163` `local_site` with `Ok(None)`             | Low value  | `settings` tests pin the cleaned list                                                                               |
| `sites/local_network.rs:163` `local_site` with `Ok(Some(""))`         | Low value  | Same tests                                                                                                          |
| `sites/local_network.rs:163` `local_site` with `Ok(Some("xyzzy"))`    | Low value  | Same tests                                                                                                          |
| `sites/local_network.rs:180` `is_on_own_network` with `true`          | Real gap   | M-4                                                                                                                 |
| `sites/local_network.rs:180` `is_on_own_network` with `false`         | Real gap   | M-4                                                                                                                 |
| `sites/local_network.rs:180` `\|\|` to `&&` in `is_on_own_network`    | Real gap   | M-4                                                                                                                 |
| `sites/public_suffix.rs:37` `*` to `+` in `MAX_LIST_BYTES`            | Low value  | Cap of 1 MiB is still three times the real list                                                                     |
| `sites/public_suffix.rs:55` `SuffixList` `Debug`                      | Low value  | Debug text only                                                                                                     |
| `sites/public_suffix.rs:133` `<` to `<=` in `accept`                  | Low value  | Only a list at exactly nine tenths                                                                                  |
| `sites/public_suffix.rs:148` `>` to `>=` in `accept`                  | Low value  | Only a list at exactly twice the rules                                                                              |
| `sites/public_suffix.rs:154` `>` to `>=` in `accept`                  | Low value  | Only a list dated at the exact millisecond                                                                          |
| `sites/public_suffix.rs:163` `>` to `>=` in `read`                    | Low value  | Only a file of exactly the cap                                                                                      |
| `sites/public_suffix.rs:185` guard with `true` in `load`              | Equivalent | Both arms return the built-in list; only a log line differs                                                         |
| `sites/public_suffix.rs:185` guard with `false` in `load`             | Equivalent | Same                                                                                                                |
| `sites/public_suffix.rs:185` `==` to `!=` in `load`                   | Equivalent | Same                                                                                                                |
| `sites/public_suffix.rs:192` `>` to `>=` in `load`                    | Low value  | Only a kept list with the built-in list's exact date                                                                |
| `sites/public_suffix.rs:236` `+` to `-` in `read_kept`                | Low value  | Only a kept file over the cap, which the app does not write                                                         |
| `sites/public_suffix.rs:236` `+` to `*` in `read_kept`                | Low value  | Same                                                                                                                |
| `sites/public_suffix.rs:255` `<` to `<=` in `whole`                   | Low value  | A loud panic, and only with a line after the last marker                                                            |
| `sites/public_suffix.rs:257` second `&&` to `\|\|` in `whole`         | Low value  | Also counts comments, in every list alike; the count is only a plausibility check on a list fetched over HTTPS      |
| `sites/public_suffix.rs:257` first `&&` to `\|\|` in `whole`          | Low value  | Same                                                                                                                |
| `engine/naming.rs:64` guard with `true` in `split_params`             | Low value  | Only an escaped quote followed by `;`; the name comes out shorter, still sanitized                                  |
| `engine/naming.rs:64` `==` to `!=` in `split_params`                  | Low value  | Same, and a value starting with a quote, which no real header has                                                   |
| `engine/naming.rs:64` `-` to `/` in `split_params`                    | Low value  | Same as the guard mutant                                                                                            |
| `engine/naming.rs:123` control guard with `false` in `clean`          | Real gap   | M-1                                                                                                                 |
| `engine/naming.rs:131` `\|\|` to `&&` in `clean`                      | Equivalent | `out` can never be `.` or `..` after the trailing dots are trimmed, so those two tests are dead                     |
| `engine/naming.rs:205` `&&` to `\|\|` in `truncate_keeping_extension` | Low value  | A longer "extension" is kept; the name stays within its limit                                                       |
| `engine/naming.rs:205` `<` to `<=` in `truncate_keeping_extension`    | Equivalent | `max` is always 200 and that arm's `ext` at most 32                                                                 |
| `engine/naming.rs:223` guard with `true` in `split_ext`               | Low value  | A dotfile's numbered name only: ` (1).env` for `.env (1)`                                                           |
| `engine/naming.rs:223` `>` to `>=` in `split_ext`                     | Low value  | Same                                                                                                                |
| `engine/naming.rs:290` `rename_no_replace` with `Ok(())`              | Low value  | `core` `relocate` tests fail on the wrapper                                                                         |
| `engine/naming.rs:309` guard with `true` in `create_free_part`        | Real gap   | M-3                                                                                                                 |
| `engine/naming.rs:357` guard with `true` in `next_name`               | Low value  | Only a numbered name with an "extension" over 32 bytes                                                              |
| `engine/net_guard.rs:33` `Guard` `Debug`                              | Low value  | Debug text only                                                                                                     |
| `engine/net_guard.rs:376` `&&` to `\|\|` in `NetworkGuard::check`     | Low value  | Refuses more, not less: every named host fails at once in any real use                                              |
| `engine/retry.rs:214` `>` to `==` in `after_failure`                  | Low value  | A lone refusal at the longest pause waits instead of stepping back; the next one steps back                         |
| `engine/retry.rs:214` `>` to `>=` in `after_failure`                  | Equivalent | Differs only with exactly zero jitter, and only by stepping back one refusal sooner                                 |
| `engine/retry.rs:231` drop `PrematureEof` in `is_refusal`             | Low value  | Still retried, and the segment is released for the other connections; slower, not wrong                             |
| `engine/segment.rs:72` `<` to `<=` in `from_remaining`                | Equivalent | `ranges_fit` already refuses empty ranges, and an empty segment is never claimed                                    |
| `engine/segment.rs:100` `>` to `>=` in `acquire`                      | Equivalent | An empty assignment ends at once in `run_assignment` and is taken only once                                         |
| `engine/segment.rs:120` `<` to `<=` in `acquire`                      | Low value  | Split boundary: speed only                                                                                          |
| `engine/segment.rs:120` `*` to `+` in `acquire`                       | Low value  | Smaller splits than asked for: speed only, offsets stay right                                                       |
| `engine/segment.rs:258` `>` to `>=` in `request_end`                  | Equivalent | `request_limit` already filters out 0                                                                               |
| `engine/state.rs:21` `*` to `+` in `MAX_STATE_BYTES`                  | Low value  | A cap of 1 MiB still far exceeds any resume file                                                                    |
| `engine/state.rs:86` `>` to `>=` in `load`                            | Low value  | Only a file of exactly the cap, which starts over either way                                                        |
| `engine/target.rs:62` `Target::name` with `""`                        | Low value  | Only the CLI's printed name                                                                                         |
| `engine/target.rs:62` `Target::name` with `"xyzzy"`                   | Low value  | Same                                                                                                                |
| `engine/target.rs:264` guard with `false` in `saved_state`            | Equivalent | `None` either way; only a log line differs                                                                          |
| `engine/target.rs:276` guard with `true` in `part_len`                | Equivalent | `None` either way; only a log line differs                                                                          |
| `engine/target.rs:276` guard with `false` in `part_len`               | Equivalent | Same                                                                                                                |
| `engine/target.rs:276` `==` to `!=` in `part_len`                     | Equivalent | Same                                                                                                                |
| `engine/target.rs:290` guard with `true` in `discard`                 | Equivalent | Nothing returned; only a log line differs                                                                           |
| `engine/target.rs:290` guard with `false` in `discard`                | Equivalent | Same                                                                                                                |
| `engine/target.rs:290` `==` to `!=` in `discard`                      | Equivalent | Same                                                                                                                |
| `engine/target.rs:301` `same_download` with `true`                    | Real gap   | M-2                                                                                                                 |
| `engine/target.rs:301` `==` to `!=` in `same_download`                | Real gap   | M-2                                                                                                                 |

## Findings

### M-1 [P2] No test feeds a control character to `clean`

- Where: `crates/engine/src/naming.rs`, `clean`, and through it `sanitize` and `is_safe_name`
- Category: fake test
- Failure: a server sends `filename*=UTF-8''a%0Ab.txt`, or a link path with `%01`. With the control-character arm of `clean` removed, `sanitize` keeps the character and `is_safe_name` accepts it, so a name Windows refuses, or a Linux name with a line break in it, reaches the disk. Every test still passes.
- Evidence: the unit tests cover the forbidden characters and format characters, and the `sanitize_returns_a_name_it_keeps` property's character class has no control character. `crates/engine/tests`, `crates/core` and `crates/media` have no such case either. `only_a_safe_name_is_taken` is the matching test on the `is_safe_name` side, and has none. The code is right today. P2 because this is a path safety check.
- Confidence: certain

### M-2 [P3] Nothing tests that another link's partial download is left alone

- Where: `crates/engine/src/target.rs`, `same_download`, as used by `plan_in_folder`
- Category: fake test
- Failure: `f.bin.wcstate` and a full-size part file were left by `https://a/f.bin`, and a download of `https://b/f.bin` arrives in the same folder under the same name, with the same size and the same (or no) validators. If `same_download` answers true, it continues the other link's part file and finishes with bytes of both, silently. Should take `f (1).bin`.
- Evidence: `a_paused_download_into_a_folder_is_continued` covers the same link; `downloads_of_one_name_in_one_folder_never_share_a_file` has no resume file, so the check never runs; `an_old_resume_file_with_a_password_is_continued_and_cleaned` covers a match only. Reached by the CLI and the published engine API, which use `Destination::InFolder`. The outcome would be corruption, but this is not a path, signature or checksum check, so P3. The code is right today.
- Confidence: certain

### M-3 [P3] No test makes `create_free_part` fail

- Where: `crates/engine/src/naming.rs`, `create_free_part`
- Category: fake test
- Failure: the folder cannot be written or is gone, so creating the part file fails with something other than `AlreadyExists`. If that error were treated as a taken name, the loop tries names forever on a blocking thread and the download hangs instead of reporting the error.
- Evidence: the mutant that treats every error as taken survives the whole engine suite. Its twin in `move_to_free_name` times out because `a_failed_move_leaves_no_file_behind` covers that path; this function has no such test. Calling it with a missing folder and expecting `NotFound` covers it on both systems. The code is right today.
- Confidence: certain

### M-4 [P3] The `<local>` queue rule is never tested against the real network check

- Where: `crates/sites/src/local_network.rs`, `is_on_own_network`, as used by `rule_among` in `crates/core/src/manager/placement.rs`
- Category: fake test
- Failure: a queue has the site rule `<local>`. If `is_on_own_network` answers true, that queue takes every download, with its folder and schedule; if false or the two checks are joined wrongly, downloads from the person's network or listed sites miss the queue. Either way, no error appears.
- Evidence: all three mutants survive in `sites`. `crates/queues/src/sites.rs` tests `<local>` with a stand-in callback only, and the `placement` and `core/tests/manager/rules.rs` tests use named sites. The two checks it joins are tested on their own, the join is not. The code is right today.
- Confidence: certain
