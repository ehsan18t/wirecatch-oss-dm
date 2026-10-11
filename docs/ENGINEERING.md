# Engineering standard

This is how code in Wirecatch is written. It applies to every package and every contributor, human or agent. Each rule says how it is enforced: **gate** means `pnpm gate` (and CI, which runs the same checks) fails when it is broken, **review** means a reviewer checks it. A rule enforced only by review is a candidate for a new check; propose one when you find a way.

The Rust lint rules marked **gate** apply to a crate once it follows the workspace lints. Until every crate does ([AUDIT](AUDIT.md) W14; the crates still to go are `LINT_PENDING` in `scripts/gate.mjs`), they are review rules in the others. The ESLint rules apply everywhere, with the violations that predate them recorded in each app's `eslint-suppressions.json`.

The gates are a floor. Passing them does not make a change good; breaking them makes it wrong.

## 1. One source for every fact

A fact written in two places will disagree with itself. Each kind of fact has one home, and everything else reads or generates from it.

| Fact | Its one home | Enforced by |
|---|---|---|
| Product identity: names, ids, app identifiers, host and pipe names, folder names, URLs, emails | `config/product.toml`, generated into Rust, TypeScript and every manifest. Being introduced ([AUDIT](AUDIT.md) W2); until then `config/extension-ids.json` holds the extension ids and the rest are named constants | review (a gate check that generated files are current comes with W2) |
| Anything that crosses a process boundary: types, command and event names, limits, defaults, error codes | `crates/protocol`, generated into `packages/protocol` (D15) | gate (`protocol`) |
| Commands every check runs, and the versions of the cargo tools they use | `scripts/gate.mjs` | review |
| Rust lint policy | `[workspace.lints]` in `Cargo.toml` and `clippy.toml` | gate (`lint-policy` checks every crate follows it or is listed as pending) |
| TypeScript and Svelte lint policy | `packages/eslint-config` | review |
| Formatting | `rustfmt.toml`, `.prettierrc.json` | gate for Rust and the two apps; review for `packages/` and `scripts/` |
| Shared Rust dependency versions | `[workspace.dependencies]` | review |
| Shared JavaScript tool versions | the `catalog` in `pnpm-workspace.yaml` | review |
| The Rust release | `rust-toolchain.toml` (rustup and CI read it) | review |
| The engine's minimum Rust release | `rust-version` in `crates/engine/Cargo.toml` | gate (`msrv`) |

Generated files are never edited by hand. Change the source and regenerate.

## 2. No magic values

Every literal that encodes a decision gets a name and a doc comment saying why it has that value: timeouts, retry counts, sizes, limits, buffer lengths, thresholds, colors, spacing, z-indexes, durations, file names, environment variable names, CLI flags passed to other programs.

- A value used by one module is a `const` at the top of that module.
- A value two packages need lives in its one home (section 1); it is never copied.
- Literals that read as themselves need no name: `0`, `1`, `2`, `-1`, an empty string, an index.
- Units are part of the name or the type: `Duration` and `PROGRESS_REFRESH`, never `100` and a comment.
- UI values come from design tokens in `app.css`, never raw pixels or colors in a component.

Enforced by: gate for TypeScript (`no-magic-numbers`), review for Rust and CSS.

## 3. Reuse before writing

Before writing a helper, search for one. Duplicated logic is a bug waiting to diverge, and the audit found many that already had.

- Code needed by two crates moves to the crate that owns the concept (the engine for transfer, `crates/protocol` for contracts, core for the manager), never copied.
- Test servers and test helpers come from the shared test support (being introduced, [AUDIT](AUDIT.md) W6), never a new copy.
- A third copy of anything is never acceptable; a second copy needs a reason in the review.

Enforced by: review.

## 4. Rust

### Errors

- Libraries define their error types with `thiserror`, one enum per crate or module, with a variant per cause a caller may act on. `anyhow` is for binaries only.
- Never `Result<_, String>` and never `Box<dyn Error>` in library code.
- Never decide anything from the text of an error message. Match on a variant or an error code.
- An error says what failed and where: the operation and the path, URL host or id it concerned. Wrap the source with `#[source]`; never flatten it into a string.
- Errors shown to people are written for them (section 8) and are produced at the edge that shows them, not deep in a library.
- Never discard an error with `let _ =` or an empty `if let Err(_)`. Handle it, propagate it, or log it with a reason.

### Panics

- No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or indexing that can fail on data from outside the process: files, the network, other processes, settings, the clock. Use `?`, `get`, checked arithmetic and `try_from`.
- Arithmetic on untrusted numbers is checked or saturating. Conversions use `try_from`, not `as`. Durations from untrusted input use `Duration::try_from_secs_f64`.
- Release builds abort on panic, so one panic kills every running download. Treat a reachable panic as a critical bug.

Enforced by: gate (`unwrap_used`, `expect_used`, `todo`, `unimplemented`, pedantic cast lints); review for `panic!` and indexing.

### Exceptions to a lint

Write `#[expect(clippy::lint_name, reason = "why this case is correct")]` on the smallest item that needs it. Never `allow`, and never a crate-wide exception without a reason that covers the whole crate. The reason must convince a reviewer; "needed" is not a reason.

Enforced by: gate (`allow_attributes`, `allow_attributes_without_reason`).

### Unsafe

- `unsafe` lives only in small wrapper modules that expose a safe API. Every other module is `#![forbid(unsafe_code)]`.
- Every `unsafe` block has a `// SAFETY:` comment proving each requirement of the call it makes, for this call, not in general.
- Prefer a maintained crate (`windows`, `windows-registry`, std's `OwnedHandle`) to hand-written FFI.

Enforced by: gate (`undocumented_unsafe_blocks`); review for the wrapper modules and `forbid(unsafe_code)`.

### Types

- Ids are newtypes, never bare integers or strings.
- A closed set of options is an enum, never a string or a boolean pair. A function never takes two booleans in a row; use an enum or an options struct.
- Validate at construction: a value that exists is valid (parse, do not validate later). Public option structs validate before use.
- Public enums and structs that may grow are `#[non_exhaustive]`.

### Async and concurrency

- No blocking calls on async tasks: file system work uses `tokio::fs` or `spawn_blocking`, and so does any blocking system call.
- No lock is held across `.await`.
- Every spawned task has an owner that stops it: a `JoinSet`, a stored `JoinHandle` or a `CancellationToken`. No fire-and-forget tasks.
- Public async functions are cancel-safe, or their docs say what cancelling them leaves behind.
- No polling where an event exists. Wait on a channel, `Notify` or watch with a deadline instead of sleeping in a loop.

### Structure

- A module has one responsibility. A file past about 500 lines or a function past about 80 is a signal to split; clippy's `too_many_lines` enforces the function limit.
- Lower layers never import from higher ones (for example, the engine's HTTP layer never reaches into the download orchestrator).
- Every public item has a doc comment. Gate (`missing_docs`).

## 5. TypeScript and Svelte

- Svelte 5 runes only. `$derived` for values computed from state, `$effect` only for side effects, and every effect that subscribes, listens or starts a timer returns its cleanup.
- Strict TypeScript. No `any`. No `as` casts or `!` assertions on data from outside the program (IPC, storage, messages, the DOM); validate it with the generated validators instead.
- Layers in the desktop app: components get what they need from state and never import the backend; the state classes call the backend; routes wire the two together; only `src/lib/backend` talks to Tauri. In the extension, only `src/lib/native` talks to the app. Gate (`no-restricted-imports`, `no-restricted-syntax`, including dynamic `import()` and destructuring).
- Every `{#each}` has a key. Lists that can be long are virtualized. A progress tick updates the rows that changed, not the whole list.
- Functions stay under 80 lines and a complexity of 12, files under 400 lines. Gate.

Existing violations are recorded in each app's `eslint-suppressions.json`. That file only shrinks: fix the violations in any file you touch, and never add to it.

## 6. Logging

- Rust logs with `tracing`; each binary writes to a file in the app's log folder as well as to stderr (being fixed, [AUDIT](AUDIT.md) W5).
- `error` means a person will see a failure, `warn` means something went wrong and was handled, `info` marks lifecycle events, `debug` and `trace` are for development.
- Never log secrets: cookies, authorization headers, passwords, or URLs that carry credentials or signed tokens.
- The extension never calls `console` directly (gate, `no-console`) and never swallows an error silently (review). It has no logger yet; [AUDIT](AUDIT.md) W5 adds one, and then every error goes through it.

## 7. Tests

- New behavior comes with tests that would fail without it. A bug fix starts with a test that reproduces the bug.
- Tests never reach the internet. Servers are local.
- No fixed sleeps. Wait for an event with a deadline, and fail with a message when the deadline passes. A test that can hang forever is a bug.
- No silent skips. A test that needs something optional (a real ffmpeg, a display) says why it skipped, and CI sets the variable that makes it required.
- Tests are named for the behavior they prove (`paused_download_keeps_its_part_file`), not for a ticket or a review round.
- Hostile input gets its own tests: oversized numbers, truncated frames, malformed manifests, paths with `..`, executables disguised as documents.

## 8. Writing

Everything a person reads: UI copy, CLI output, errors, docs, comments and commit messages.

- Plain language. An error says what happened and how to fix it.
- No em dashes. Use a comma, colon, parentheses or a new sentence. Gate (`text`).
- Never hard-wrap a paragraph. One paragraph is one line.

## 9. Commits

`type(scope): subject` in Conventional Commits form: lower case, imperative, no trailing period, at most 72 characters. Types: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`, `chore`, `style`, `revert`. The scope is the package (`engine`, `protocol`, `core`, `ipc`, `bridge`, `streams`, `cli`, `e2e`, `desktop`, `extension`), or none for a repository-wide change. Breaking changes use `!` after the scope and a `BREAKING CHANGE:` footer. The body explains why, one line per paragraph. Merges into main: `chore: merge <what> (<work package>)`. No `Co-Authored-By` or other tool or AI credit lines; the author is the person who owns the repository.

Enforced by: gate for the form, the length, lower case, the trailing period, em dashes and credit lines (`commits`, which CI runs over every pull request and push, and the commit-msg hook that `pnpm install` turns on); review for the imperative mood, a body that explains why, `BREAKING CHANGE:` footers and the merge form, since git writes merge messages itself.

## 10. Performance

Performance is a feature of a download manager.

- No allocation per chunk or per progress tick in a hot path; reuse buffers.
- No polling where an event exists (section 4).
- No work proportional to the whole download list on a progress update; touch the rows that changed (section 5).
- A change to the engine's transfer path is checked with `cargo run --release -p wirecatch-engine --example bench_local` before and after.

Enforced by: review, and the store tests that measure how much one progress batch touches.

## 11. Security

- Anything that handles cookies, credentials, paths from outside, downloaded executables or another process's messages gets tests for the hostile case.
- Paths built from outside input are joined with the safe-join helper so they cannot leave their folder.
- Other programs are started with an argument list, never through a shell, and only from a path Wirecatch verified.
- Every message from another process is size-limited and validated before use.
