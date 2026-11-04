# Tool results

These are leads for reviewers, not findings. Raw output is in `%TEMP%\wirecatch-audit\2026-11-desktop\`.

## Coverage (`cargo llvm-cov`)

Run with `--workspace --features wirecatch-bridge/test-overrides --all-targets --ignore-run-fail`. Line coverage is 72.4% overall.

1,610 tests passed, 1 was ignored and 19 failed. All 19 are in `crates/streams/tests/process.rs` and `ffmpeg.rs`, and all report that a fake program was missing from the coverage build's examples folder. They are not product failures: `cargo test -p wirecatch-streams` passes all 146 tests. Coverage of `crates/streams` is therefore understated for code that runs programs.

| Crate        | Lines  | Covered |
| ------------ | ------ | ------- |
| desktop      | 16,682 | 11%     |
| cli          | 267    | 58%     |
| product      | 35     | 69%     |
| shell        | 3,656  | 74%     |
| e2e          | 551    | 89%     |
| bridge       | 2,667  | 90%     |
| core         | 10,233 | 93%     |
| ipc          | 1,787  | 93%     |
| server       | 443    | 93%     |
| engine       | 6,100  | 94%     |
| messages     | 462    | 94%     |
| streams      | 5,065  | 94%     |
| test-support | 1,738  | 94%     |
| files        | 340    | 95%     |
| settings     | 2,191  | 95%     |
| logging      | 1,399  | 96%     |
| sites        | 805    | 96%     |
| store        | 2,056  | 96%     |
| updates      | 183    | 96%     |
| addons       | 1,192  | 97%     |
| media        | 4,643  | 97%     |
| protocol     | 3,002  | 97%     |
| queues       | 918    | 98%     |
| records      | 203    | 99%     |

Least covered outside `crates/desktop`, among files of 80 lines or more: `crates/shell/src/teller.rs` (93 lines, 0%) and `crates/shell/src/api.rs` (510 lines, 40%).

High coverage shows only that lines ran. The mutation run below tests whether the tests would notice a wrong result.

## Duplicates (`jscpd`)

Run over `crates` and `scripts`, with at least 12 lines and 80 tokens. It found 11 exact clones covering 148 of 115,249 lines (0.13%). Only these cross files:

- `crates/desktop/src/add_dialog.rs` and `crates/desktop/src/main_window/queue_dialog.rs` (17 lines)
- `crates/desktop/src/main_window/proxy_dialog.rs` and `crates/desktop/src/main_window/queue_dialog.rs` (15 lines)

The other 9 are within one file:

| File                                         | Clones              |
| -------------------------------------------- | ------------------- |
| `crates/core/src/manager/media_run/tests.rs` | 2 (14 lines each)   |
| `crates/settings/src/settings_file.rs`       | 2 (14 and 13 lines) |
| `crates/e2e/tests/e2e.rs`                    | 1 (19 lines)        |
| `crates/protocol/src/frame.rs`               | 1 (14 lines)        |
| `crates/media/src/ytdlp/mod.rs`              | 1 (13 lines)        |
| `crates/core/tests/manager/lifecycle.rs`     | 1 (13 lines)        |
| `crates/streams/tests/hls.rs`                | 1 (13 lines)        |

This finds copied text only, not the same logic written differently.

## Mutation (`cargo-mutants`)

Run on the 15 files named in `## Mutation targets` of `CHUNK-PLAN.md`, with a 2-hour cap.

| Crate | Missed | Timed out |
| --- | --- | --- |
| addons | 15 | 0 |
| updates | 5 | 0 |
| sites | 22 | 0 |
| engine | 35 | 18 |

A missed mutant is a change to the code that no test noticed. Each is a lead, not a finding. Line numbers are from commit `37603b330`.

```text
crates/addons/src/archive.rs:293:5: replace write_file -> Result<(u64, Sha256Digest), ArchiveError> with Ok((0, Default::default()))
crates/addons/src/archive.rs:280:9: replace <impl Write for Hashing>::flush -> io::Result<()> with Ok(())
crates/addons/src/archive.rs:293:5: replace write_file -> Result<(u64, Sha256Digest), ArchiveError> with Ok((1, Default::default()))
crates/addons/src/archive.rs:333:21: replace == with != in copy_file
crates/addons/src/manifest.rs:49:9: replace <impl std::fmt::Display for ComponentId>::fmt -> std::fmt::Result with Ok(Default::default())
crates/addons/src/manifest.rs:104:36: replace | with ^ in Sha256Digest::from_hex
crates/addons/src/manifest.rs:119:9: replace <impl std::fmt::Debug for Sha256Digest>::fmt -> std::fmt::Result with Ok(Default::default())
crates/addons/src/manifest.rs:325:13: delete field version from struct Self expression in AddonComponent::for_release
crates/addons/src/manifest.rs:326:13: delete field release from struct Self expression in AddonComponent::for_release
crates/addons/src/manifest.rs:505:5: replace ytdlp_asset -> &'static str with ""
crates/addons/src/manifest.rs:505:5: replace ytdlp_asset -> &'static str with "xyzzy"
crates/addons/src/manifest.rs:511:5: replace program_name -> String with "xyzzy".into()
crates/addons/src/manifest.rs:629:5: replace asset_stem -> &str with ""
crates/addons/src/manifest.rs:629:5: replace asset_stem -> &str with "xyzzy"
crates/addons/src/signature.rs:65:9: replace <impl std::fmt::Display for Fingerprint>::fmt -> std::fmt::Result with Ok(Default::default())
crates/updates/src/manifest.rs:72:9: replace System::current -> Option<Self> with None
crates/updates/src/manifest.rs:86:9: replace Arch::current -> Option<Self> with None
crates/updates/src/release.rs:38:9: replace Release::asset -> Option<&Asset> with None
crates/updates/src/manifest.rs:143:22: replace && with || in is_sha256
crates/updates/src/release.rs:38:52: replace == with != in Release::asset
crates/sites/src/local_network.rs:37:9: replace ChosenBy::private_network -> PrivateNetwork with Default::default()
crates/sites/src/local_network.rs:163:5: replace local_site -> Result<Option<String>, NotALocalSite> with Ok(None)
crates/sites/src/local_network.rs:163:5: replace local_site -> Result<Option<String>, NotALocalSite> with Ok(Some(String::new()))
crates/sites/src/local_network.rs:163:5: replace local_site -> Result<Option<String>, NotALocalSite> with Ok(Some("xyzzy".into()))
crates/sites/src/local_network.rs:180:5: replace is_on_own_network -> bool with true
crates/sites/src/local_network.rs:180:5: replace is_on_own_network -> bool with false
crates/sites/src/local_network.rs:180:39: replace || with && in is_on_own_network
crates/sites/src/public_suffix.rs:37:35: replace * with +
crates/sites/src/public_suffix.rs:55:9: replace <impl std::fmt::Debug for SuffixList>::fmt -> std::fmt::Result with Ok(Default::default())
crates/sites/src/public_suffix.rs:133:67: replace < with <= in SuffixList::accept
crates/sites/src/public_suffix.rs:148:23: replace > with >= in SuffixList::accept
crates/sites/src/public_suffix.rs:154:27: replace > with >= in SuffixList::accept
crates/sites/src/public_suffix.rs:163:62: replace > with >= in SuffixList::read
crates/sites/src/public_suffix.rs:185:23: replace match guard e.kind() == io::ErrorKind::NotFound with true in SuffixList::load
crates/sites/src/public_suffix.rs:185:23: replace match guard e.kind() == io::ErrorKind::NotFound with false in SuffixList::load
crates/sites/src/public_suffix.rs:185:32: replace == with != in SuffixList::load
crates/sites/src/public_suffix.rs:192:40: replace > with >= in SuffixList::load
crates/sites/src/public_suffix.rs:236:30: replace + with - in read_kept
crates/sites/src/public_suffix.rs:236:30: replace + with * in read_kept
crates/sites/src/public_suffix.rs:255:17: replace < with <= in whole
crates/sites/src/public_suffix.rs:257:53: replace && with || in whole
crates/sites/src/public_suffix.rs:257:33: replace && with || in whole
crates/engine/src/naming.rs:64:21: replace match guard i == 0 || bytes[i - 1] != b'\\' with true in split_params
crates/engine/src/naming.rs:64:23: replace == with != in split_params
crates/engine/src/naming.rs:64:39: replace - with / in split_params
crates/engine/src/naming.rs:123:18: replace match guard c.is_control() with false in clean
crates/engine/src/naming.rs:131:37: replace || with && in clean
crates/engine/src/naming.rs:205:57: replace && with || in truncate_keeping_extension
crates/engine/src/naming.rs:205:70: replace < with <= in truncate_keeping_extension
crates/engine/src/naming.rs:223:20: replace match guard i > 0 with true in split_ext
crates/engine/src/naming.rs:223:22: replace > with >= in split_ext
crates/engine/src/naming.rs:290:5: replace rename_no_replace -> io::Result<()> with Ok(())
crates/engine/src/naming.rs:309:23: replace match guard e.kind() == io::ErrorKind::AlreadyExists with true in create_free_part
crates/engine/src/naming.rs:357:28: replace match guard ext.len() <= MAX_EXTENSION_BYTES with true in Candidates<'_>::next_name
crates/engine/src/net_guard.rs:33:9: replace <impl std::fmt::Debug for Guard>::fmt -> std::fmt::Result with Ok(Default::default())
crates/engine/src/net_guard.rs:376:53: replace && with || in NetworkGuard::check
crates/engine/src/retry.rs:214:56: replace > with == in Attempts::after_failure
crates/engine/src/retry.rs:214:56: replace > with >= in Attempts::after_failure
crates/engine/src/retry.rs:231:9: delete match arm Error::PrematureEof{..} in is_refusal
crates/engine/src/segment.rs:72:33: replace < with <= in SegmentTable::from_remaining
crates/engine/src/segment.rs:100:63: replace > with >= in SegmentTable::acquire
crates/engine/src/segment.rs:120:22: replace < with <= in SegmentTable::acquire
crates/engine/src/segment.rs:120:26: replace * with + in SegmentTable::acquire
crates/engine/src/segment.rs:258:30: replace > with >= in request_end
crates/engine/src/state.rs:21:33: replace * with +
crates/engine/src/state.rs:86:29: replace > with >= in ResumeState::load
crates/engine/src/target.rs:62:9: replace Target::name -> String with String::new()
crates/engine/src/target.rs:62:9: replace Target::name -> String with "xyzzy".into()
crates/engine/src/target.rs:264:36: replace match guard e.kind() == io::ErrorKind::NotFound with false in saved_state
crates/engine/src/target.rs:276:19: replace match guard e.kind() == io::ErrorKind::NotFound with true in part_len
crates/engine/src/target.rs:276:19: replace match guard e.kind() == io::ErrorKind::NotFound with false in part_len
crates/engine/src/target.rs:276:28: replace == with != in part_len
crates/engine/src/target.rs:290:19: replace match guard e.kind() == io::ErrorKind::NotFound with true in discard
crates/engine/src/target.rs:290:19: replace match guard e.kind() == io::ErrorKind::NotFound with false in discard
crates/engine/src/target.rs:290:28: replace == with != in discard
crates/engine/src/target.rs:301:5: replace same_download -> bool with true
crates/engine/src/target.rs:301:11: replace == with != in same_download
```
