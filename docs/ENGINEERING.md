# Engineering standard

This is how code in Wirecatch is written. It applies to every package and every contributor, human or agent. Each rule says how it is enforced: **gate** means `pnpm gate` (and CI, which runs the same checks) fails when it is broken, **review** means a reviewer checks it. A rule enforced only by review is a candidate for a new check; propose one when you find a way.

Every crate follows the workspace lints (`[lints] workspace = true`; the gate's `lint-policy` check fails a crate that does not), and every workspace package uses the one shared ESLint configuration at full strength (the `js-policy` check fails a package whose configuration is not built with `wirecatch()`, differs from the baseline rules or linter options for any of its source files, ignores a source file outside the generated and fixture folders the check names, or that has a `lint` or `format` script of its own; both run once from the root, whose `lint` script must be exactly the one the check derives from `pnpm-workspace.yaml`, and which also lints `scripts/`, held the same way (D42)). Nothing is recorded as an accepted violation: there are no suppression files. The few exceptions are single `#[expect(..., reason = "...")]` attributes, one ESLint disable comment with a `--` reason, or one `svelte-ignore` with its reason in parentheses (`<!-- svelte-ignore code (reason) -->`), each where the rule is wrong for that code, listed with its reason in [AUDIT](AUDIT.md) W14 and by file and lint in `LINT_EXCEPTIONS` in `scripts/gate/exceptions.mjs`. The `exceptions` check holds the code to that list: it fails on any `#[allow]` or `#[expect]` (also inside `cfg_attr`), ESLint disable comment, ESLint configuration comment or `svelte-ignore` in the files the linters read that is not listed, and on a listed one that is gone.

The gates are a floor. Passing them does not make a change good; breaking them makes it wrong.

## 1. One source for every fact

A fact written in two places will disagree with itself. Each kind of fact has one home, and everything else reads or generates from it.

| Fact | Its one home | Enforced by |
|---|---|---|
| Product identity: names, ids, app identifiers, host and pipe names, folder names, URLs, emails, the product version | `config/product.toml`; `pnpm product:generate` writes the `wirecatch-product` crate, the `@wirecatch/product` package and the identity in every manifest and package hook (D29) | gate (`product` fails when a generated file or Cargo.lock is stale or missing, or a crate does not inherit its metadata; the `publish` group, which every step that publishes runs first, fails while a placeholder is left anywhere; release builds only warn, D49); review for a literal that should use the generated constant |
| Anything that crosses a process boundary: types, command and event names, limits, defaults, error codes | `crates/protocol`, generated into `packages/protocol` (D15) | gate (`protocol`) |
| Data two languages or two packages need, test data included: samples, expected outputs, the cases of a rule both languages apply | One file both load, next to the loader of the side that owns it, such as `crates/test-support/data/redaction.json`, which `wirecatch_test_support::redaction` and `@wirecatch/protocol/testing/redaction` read (D40.10); cases Rust writes from its own functions are generated into `packages/protocol` (D37.2) | review: an implementer looks for the file before writing a list, and a reviewer flags any second copy |
| Commands every check runs, and the versions of the cargo tools they use | `scripts/gate.mjs` and its modules in `scripts/gate/` | review |
| Rust lint policy | `[workspace.lints]` in `Cargo.toml` and `clippy.toml` | gate (`lint-policy` checks every crate follows it; `clippy` lints for the system it runs on; `clippy-linux` lints for Linux from Windows and macOS every crate whose dependencies build no native code (today bridge, ipc, protocol, queues, and test-support without its `servers` feature, whose TLS reaches ring), so their code behind `cfg(unix)` is checked before Linux CI sees it. The crates that reach ring, SQLite or system libraries (cli, core, desktop, e2e, engine, streams) are linted for Linux only by Linux CI; `clippy-linux` names them on every run) |
| TypeScript and Svelte lint policy | `packages/eslint-config` | gate (`js-policy` compares the configuration ESLint computes for every source file of every package, and of `scripts/`, with the shared baseline, fails on an ignored source, and checks the root `lint` script) |
| Formatting | `rustfmt.toml`, `.prettierrc.json` | gate for Rust and every workspace package (the root `lint` script format-checks every package and `scripts/` in one Prettier run, whose only ignore files are the root `.gitignore` and `.prettierignore`; `js-policy` fails on any other in those folders (D42); `pnpm protocol:generate` formats the generated types) |
| Where the JavaScript tools are installed and run | the root `package.json`: ESLint, Prettier and its Svelte plugin, and the `lint` and `format` scripts (D42) | gate (`js-policy` fails on a package `lint` or `format` script); review for a package listing a tool its own code does not import |
| Shared Rust dependency versions | `[workspace.dependencies]` | review |
| Shared JavaScript tool versions | the `catalog` in `pnpm-workspace.yaml` | review |
| The Rust release | `rust-toolchain.toml` (rustup and CI read it) | review |
| The engine's minimum Rust release | `rust-version` in `crates/engine/Cargo.toml` | gate (`msrv`) |

Generated files are never edited by hand. Change the source and regenerate.

## 2. No magic values

Every literal that encodes a decision gets a name that says what it is, and a comment only when the name cannot say why it has that value (section 8): timeouts, retry counts, sizes, limits, buffer lengths, thresholds, colors, spacing, z-indexes, durations, file names, environment variable names, CLI flags passed to other programs.

- A value used by one module is a `const` at the top of that module.
- A value two packages need lives in its one home (section 1); it is never copied.
- Literals that read as themselves need no name: `0`, `1`, `2`, `-1`, an empty string, an index.
- Units are part of the name or the type: `Duration` and `PROGRESS_REFRESH`, never `100` and a comment.
- UI values come from the design tokens in `packages/ui/src/tokens.css` (D36), never raw pixels or colors in a component, and controls come from the design system's components, never the browser's own select, checkbox, number or time fields (D48).

Enforced by: gate for TypeScript (`wirecatch/no-magic-numbers`, which reports every number except those that build the value of a module-level `const`, so `const MAX_URL_LENGTH = 8 * 1024` is fine) and for the code built on the design system (`wirecatch/tokens-only`, on `packages/ui`, each app's `src/ui/` and the desktop's `src/lib/components/`, which reports a colour, length, opacity or duration written out instead of named; `wirecatch/use-ui-primitives` with `controls`, which reports a browser-native form control), review for Rust and CSS.

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
- Never discard an error with `let _ =`, `.ok();`, `drop(result)` or an empty `if let Err(_)`. Handle it, propagate it, or log it saying what failed and what it means. Gate for `let _ =` on a `#[must_use]` value (`let_underscore_must_use`); review for the other forms.
- Where a failure truly cannot matter, pass the result to a small function whose name says why (`client_may_hang_up`, `stderr_may_be_closed`), taking it by reference and matching both outcomes (`let (Ok(_) | Err(_)) = result;`), so the reason is stated once and a change of the result's type stops compiling.

### Panics

- No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or indexing that can fail on data from outside the process: files, the network, other processes, settings, the clock. Use `?`, `get`, checked arithmetic and `try_from`.
- Arithmetic on untrusted numbers is checked or saturating. Conversions use `try_from`, not `as`. Durations from untrusted input use `Duration::try_from_secs_f64`.
- Release builds abort on panic, so one panic kills every running download. Treat a reachable panic as a critical bug.

Enforced by: gate (`unwrap_used`, `expect_used`, `todo`, `unimplemented`, pedantic cast lints); review for `panic!` and indexing.

### Exceptions to a lint

Write `#[expect(clippy::lint_name, reason = "why this case is correct")]` on the smallest item that needs it. Never `allow`, and never a crate-wide exception without a reason that covers the whole crate. The reason must convince a reviewer; "needed" is not a reason.

Enforced by: gate (`allow_attributes`, `allow_attributes_without_reason`, and `cfg-allow`, which fails any `allow` inside `cfg_attr`: clippy cannot see an attribute that is switched off on the system it runs on, so such an `allow` passes on one system and fails on another). A lint passed into a macro (`#[allow($lint)]` inside `macro_rules!`) is left to review: the gate does not expand macros.

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
- What the store saves (the types in `wirecatch-records`, the settings, the queue settings) is read back by every later build. A change to how one is saved adds a sample to `crates/store/samples` and never edits an existing one; a change an old sample cannot load with needs a serde `alias` or `default`, or a migration. A saved field or variant whose meaning changes gets a new name, so the samples test sees the change (D81).

### Async and concurrency

- No blocking calls on async tasks: file system work uses `tokio::fs` or `spawn_blocking`, and so does any blocking system call.
- No lock is held across `.await`.
- Every spawned task has an owner that stops it: a `JoinSet`, a stored `JoinHandle` or a `CancellationToken`. No fire-and-forget tasks.
- Public async functions are cancel-safe, or their docs say what cancelling them leaves behind.
- No polling where an event exists. Wait on a channel, `Notify` or watch with a deadline instead of sleeping in a loop. Gate for `std::thread::sleep` (`disallowed_methods` in `clippy.toml`); review for `tokio::time::sleep` in a loop.

### Structure

- A module has one responsibility. A file past about 500 lines or a function past about 80 is a signal to split; clippy's `too_many_lines` enforces the function limit.
- Lower layers never import from higher ones (for example, the engine's HTTP layer never reaches into the download orchestrator).
- A doc comment is written where it says what the signature cannot; no item gets one to satisfy a lint (D76).

## 5. TypeScript and Svelte

- Svelte 5 runes only. `$derived` for values computed from state, `$effect` only for side effects, and every effect that subscribes, listens or starts a timer returns its cleanup.
- Strict TypeScript. No `any`. No `as` casts or `!` assertions on data from outside the program (IPC, storage, messages, the DOM); validate it with the generated validators instead. Gate (`wirecatch/no-assertion-on-input`, which reports an `as`, `<T>` or `!` on any value whose type says nothing about its shape: `any`, `unknown`, `object`, `{}`, `Record<string, unknown>`, or a type parameter constrained to one of them or not at all, also inside unions, containers and properties, in every package; an object literal, `[]` or a constructor called with no arguments written in the assertion is allowed; `@typescript-eslint/no-unsafe-type-assertion`, which reports any assertion that narrows a type, on data from inside the program too; and `wirecatch/no-type-argument-assertion`, which reports a type argument that only says what a call returns, as `listen<UiEvent>(...)`, `invoke<T>(...)` or `querySelector<HTMLInputElement>(...)` would, since nothing checks it, or a callback parameter annotation that does the same (`listen(name, (e: Event<UiEvent>) => ...)`, or a parameter the caller is given as `any`), also on a call given no arguments unless it only declares (`$state<T>()`, `createContext<T>()`); tests are exempt from the three).
- Ids crossing a boundary are branded (`DownloadId`, `QueueId`, `QuestionId`, D37.3), so one cannot pass for another. An id comes from the app, from its generated guard (`isQueueId`) or from its constructor in `@wirecatch/protocol` (`queueId(3)`), never from an assertion. Gate (`wirecatch/no-brand-assertion`, which reports an `as` or `<T>` to a branded type, also inside unions, lists and object types written in the assertion, everywhere but the constructors' module `packages/protocol/src/ids.ts`; `as const` is allowed; it also reports a guard written by hand for a branded id or for a type generated into `packages/protocol/src/generated`, whose generated guard checks it whole, and a call of an id's guard on a value that is already another id, which would pass one for the other). A generated id guard accepts only a whole number from 0 up that JavaScript holds exactly, and the constructors check with it.
- Layers in the desktop app: components and download windows get what they need from state and never import the backend or hold it by any other path; the state classes call the backend; the pages (`src/pages`) wire the two together; only `src/lib/backend` talks to Tauri. In the extension, only `src/lib/native` talks to the app. Gate (`no-restricted-imports`, `no-restricted-syntax`, including dynamic `import()` and destructuring, and `wirecatch/no-value-of-type`, a desktop layer rule that asks the type checker for a value of a backend type in components, their helpers and window components: through members, calls, names, `await`, `as`, destructuring, parameters, `Reflect.get`, and generics such as `Promise`, Svelte stores, `Pick` and `Partial`; window components may import backend types only). Review for names bound by `{#each}` or a snippet, functions returning the backend, and look-alike types under another name.
- Icons and form controls in both apps go through the design system (D48): no icon from Lucide or unplugin-icons (or a module the app adds to the rule) is rendered except through `Icon`, whether as its own tag, through a name it was given (`const Close = X; <Close />`) or with `<svelte:component>`, and no browser-native select, textarea, checkbox, radio, range, number, date, time or colour control is drawn. Gate (`wirecatch/use-ui-primitives` with `controls`); review for what the rule cannot follow: an icon passed in as a prop or returned by a function, and an `<input>` whose `type` is not written out.
- Every `{#each}` has a key. Lists that can be long are virtualized. A progress tick updates the rows that changed, not the whole list.
- Functions stay within 80 lines and a cyclomatic complexity of 12, files within 400 lines, counting neither blank lines nor comments; blocks nest at most 4 deep and functions take at most 4 parameters. Gate (`max-lines-per-function`, `complexity`, `max-lines`, `max-depth`, `max-params`; tests are exempt from the line limits).

There are no suppression files, and none is added: fix a violation at its cause. A disable comment is only for a rule that is truly wrong in one place: it names the rule, gives its reason after `--`, and covers only those lines (`eslint-disable-next-line`, or an `eslint-disable` switched back on by `eslint-enable`), never a whole file. An ESLint comment in Svelte markup gives its reason the same way; a `svelte-ignore` gives it in parentheses after its codes (`<!-- svelte-ignore a11y_x (the reason) -->`), since `svelte/no-unused-svelte-ignore` would read the words after `--` as codes. The configuration lives in `eslint.config.js`: no rule settings, globals or environments in comments. Gate: in scripts, `require-description`, `no-unlimited-disable`, `disable-enable-pair` and `no-use` from `@eslint-community/eslint-plugin-eslint-comments` in the shared config; in Svelte markup, which that plugin does not read, the `text` check requires the reason and allows only the comments `no-use` allows in scripts; review for a markup `eslint-disable` that is never switched back on.

## 6. Logging

- Rust logs with `tracing`, and every binary sets it up through `wirecatch-logging` (D40): lines go to stderr and to a size-capped file in the app's log folder (`logs/` in the data folder), `WIRECATCH_LOG` chooses them, and every field and line has its secrets hidden as it is written. A new binary calls `wirecatch_logging::start` first thing in `main`; a test that starts a binary points `WIRECATCH_LOG_DIR` at its own folder.
- `error` means a person will see a failure, `warn` means something went wrong and was handled, `info` marks lifecycle events, `debug` and `trace` are for development.
- Never log secrets: cookies, authorization headers, passwords, or URLs that carry credentials or signed tokens. The redaction in `wirecatch-logging` is a net under this rule, not leave to break it: never log a request's headers or a program's arguments.
- What a child program writes to stderr is kept bounded and redacted (`StderrTail`), logged when it fails, and cut short in a person's message.
- What a secret is (the names, the sign-in headers, the scheme words and the service key prefixes, and the rules that read them) lives in `crates/protocol` (`secrets.rs`), generated and ported for TypeScript; the log's redaction and the extension's both read it, and both are tested against the samples in `crates/test-support/data/redaction.json` (D40.10). A secret form found later is added to that file, never to one redaction's tests alone; a form one side skips on purpose is marked there. No file holds a string shaped like a provider's credential, not even a test sample: a sample is stored in pieces (`["AKIA", "IOSFODNN7EXAMPLE"]`) and joined by the loader (gate, `text`).
- The extension logs through `src/lib/diag/log.ts` (`log.warn(scope, message, error)`, or `fallback(...)` as a rejection handler that logs and answers the old fallback value). It keeps a bounded ring buffer per context, counting a repeat instead of adding it; the background and the popup save theirs to `storage.session`, for the popup's planned "copy diagnostics" action ([AUDIT](AUDIT.md), W5 carried items). Every text is redacted first, percent-encoded parts decoded: an address keeps only its site, and file paths, e-mail addresses, cookies, authorization headers and every value named or shaped like a secret are hidden, wherever the name sits (a header, a pair, a JSON key, a header list entry, a string inside a string). Entries made while a private window is open, or before the background knows whether one is, are forgotten when the last one closes, saved copies included; the popup is private when the window it was opened in is, whatever process it runs in. In development it also writes to the console from `src/lib/diag/console.ts`, the one file the shared configuration lets call `console` (gate, `no-console`). Uncaught errors and unhandled rejections in the background and the popup are logged too. A caught error is never dropped silently: it is logged or handed on, or a comment directly inside the handler says in words why it is expected; a rejection handler made by a call is trusted only when the call is `fallback` imported from the logger (gate, `wirecatch/no-silent-catch`, in the extension, with `fallback` named in its `factories` option).

## 7. Tests

- New behavior comes with tests that would fail without it. A bug fix starts with a test that reproduces the bug.
- Tests never reach the internet. Servers are local.
- No fixed sleeps. Wait for an event with a deadline, and fail with a message when the deadline passes. A test that can hang forever is a bug. Gate for `std::thread::sleep` in Rust (`disallowed_methods` in `clippy.toml`); review for async sleeps and for TypeScript.
- Every wait for something to happen is bounded by `PATIENCE` from `crates/test-support` (two minutes; Vitest's `testTimeout` is the same). It bounds a hang, never the work: a test that takes a second alone takes many times that on a machine busy with other builds, and must pass there too. A window in which something must not happen is short and named, so it can only miss a fault, never fail a correct run.
- A test never races the machine against a time limit of the code under test, nor against a sleep. To show that a limit does not run out, drive the clock (a paused tokio clock, a fake program that steps only when the test says). To act while a client waits, hold the server (`hold`, `hang`) and wait for the request to arrive. To bound waste, count what each request or transfer sent. An answer whose size would grow with the machine's load (a probe the client breaks off once it is ready) is held by the server (`hold_bodies_from`, `hold_probe`) so it sends nothing meanwhile, and it stays in the count: a client that reads it on waits and fails. The `test-timing` gate check refuses, in a Rust test outside tokio's paused clock, an upper bound on a measured duration (`started.elapsed() < limit`) and a fixed `Duration` written into a wait that has to succeed (`timeout(Duration::from_secs(2), ..)`); a `while` window and a wait expected to run out are allowed.
- No silent skips. A test that needs something optional (a real ffmpeg, a display) says why it skipped, and CI sets the variable that makes it required.
- Tests are named for the behavior they prove (`paused_download_keeps_its_part_file`), not for a ticket or a review round.
- Hostile input gets its own tests: oversized numbers, truncated frames, malformed manifests, paths with `..`, executables disguised as documents.

## 8. Writing

Everything a person reads: UI copy, CLI output, errors, docs, comments and commit messages.

- Plain language. An error says what happened and how to fix it.
- No em dashes. Use a comma, colon, parentheses or a new sentence. Gate (`text`).
- Never hard-wrap a paragraph. One paragraph is one line.
- A comment, doc comments included, is only for what the code cannot say: behaviour of the platform or a dependency the code cannot show, a deliberate choice whose obvious alternative brings back a defect, the decision or invariant the code holds up, code that must not be simplified, a `SAFETY:` proof. One line by default, three at most. Never restate the code or what a name already says, and never narrate history ("now", "no longer", "used to", what a change did): that belongs in the commit message. Review; the existing comments are cleaned up in [AUDIT](AUDIT.md) W15.

## 9. Commits

`type(scope): subject` in Conventional Commits form: lower case, imperative, no trailing period, at most 72 characters. Types: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`, `chore`, `style`, `revert`. The scope is the package (`engine`, `protocol`, `core`, `ipc`, `bridge`, `streams`, `queues`, `server`, `sites`, `files`, `addons`, `records`, `store`, `messages`, `cli`, `e2e`, `test-support`, `desktop`, `extension`, `units`, `ui`, `eslint-config`), or none for a repository-wide change. Breaking changes use `!` after the scope and a `BREAKING CHANGE:` footer. The subject says plainly what the change does, in everyday words a teammate understands from it alone: no internal shorthand, decision IDs or invented phrasing. The body describes what changed and nothing else: the behaviour, the areas touched and what was left out, as a short list when there are several, one line per paragraph or item. A reason belongs there only when the change is unclear without it, and then it is about the code. No backstory, no account of how the work was done, no repeating the diff, no filler. Merges into main: `chore: merge <what> (<work package>)`. No `Co-Authored-By` or other tool or AI credit lines; the author is the person who owns the repository.

Before a branch merges into main, its commits are regrouped: one commit per problem solved, each review fix folded into the commit it fixes, and the merges of main into the branch dropped, so main's history reads as the problems solved rather than the order they were worked on. The regrouped commits are not built or checked one by one: the final tree must be identical to the reviewed one. Local checks match what changed (D78): a branch that touches only documentation runs the `text`, `commits` and `docs` groups, a change inside a few crates or packages their lint and tests, and only a wide change the full gate; the pull request's CI runs every check before it is squash-merged. The branch with the history as it was reviewed is kept until the merge is done.

The same holds for pull request titles and descriptions (a squashed pull request's title becomes the subject and its description the body) and for issue titles and details, which state the problem as it is in the product: what happens, what should happen, how to reproduce it. None of them ever carries anything personal: they are public and permanent, so they never name or describe what a person visited, used, ran into, asked for or said ("the maintainer found", "reported by"), and an example site, file or account is a generic one (`example.com`), never one taken from someone's request. A pull request description opens with a sentence or two on what changed, then, only where it applies, what to look at closely, how it was tested and what is not tested yet.

Once a branch's work is on main, the branch is deleted, locally and on the remote, and its worktree and build folder are removed; so are the backup branches made for a regroup. A branch that has served its purpose is not kept as history: main is the record.

Enforced by: gate for the form, the length, lower case, the trailing period, em dashes and credit lines (`commits`, which CI runs over every pull request and push, and the commit-msg hook that `pnpm install` turns on); review for the imperative mood, a body that describes only the change and nothing personal, `BREAKING CHANGE:` footers, the merge form, since git writes merge messages itself, and the regrouping before a merge.

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
