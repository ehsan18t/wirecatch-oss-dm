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

Not run yet. It waits for the mutation targets in `CHUNK-PLAN.md`.
