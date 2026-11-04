# Code audit plan

How to run a code audit of this repository. An audit is done in small sessions across usage-limit resets, so a session is pointed at this file with one piece of work, for example:

> Follow `docs/audit/AUDIT-PLAN.md`. Scope: desktop. Run stage 0.

> Follow `docs/audit/AUDIT-PLAN.md`. Scope: desktop. Run the tools and fault tests.

> Follow `docs/audit/AUDIT-PLAN.md`. Scope: desktop. Review the next 2 chunks, then stop.

> Follow `docs/audit/AUDIT-PLAN.md`. Scope: desktop. Validate the next 2 batches, then stop.

> Follow `docs/audit/AUDIT-PLAN.md`. Scope: desktop. Run stage 3.

## Goal

Wirecatch should be the most stable and correct download manager it can be. Nothing it has should be broken: whatever exists must work properly. An audit finds bugs, broken features, wrong implementations, inefficient code, duplicate and dead code, and fake tests, then puts them in the best order to fix. Review the way a senior developer would.

**Report real issues only.** Fixing imagined issues is worse than finding nothing. A chunk with no findings is a good result; never pad it.

A better way of doing something that already works is not an issue. It is a suggestion.

## The stages

| Stage       | What happens                                                                                            | Result                                                       |
| ----------- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| 0. Prepare  | Decide how to review: dependency graph, chunks, order. Map the features. Run the tools and fault tests. | `CHUNK-PLAN.md`, `FEATURES.md`, `tools.md`, `fault-tests.md` |
| 1. Review   | Review chunks, cross-module flows and the UI, writing findings.                                         | `chunks/`, `contracts/`, `cross/`, `ui.md`                   |
| 2. Validate | Group findings by related cause and double-check them in batches.                                       | `GROUPS.md`, `proofs/`                                       |
| 3. Organize | Put the valid findings in the exact order to fix them.                                                  | `AUDIT-LOG.md`                                               |

After stage 3 the audit is finished. Fixing works down the list, one item at a time, whenever there is time.

## Before acting

1. **Find the run.** Scope names are `desktop` and `extension`. The run is the folder in `docs/audit/runs/` named `<YYYY-MM>-<scope>` whose `CHUNK-PLAN.md` still has unfinished rows. Never work out the name from today's date: a session in a later month continues the same run. A new folder is made only when stage 0 was requested and no unfinished run exists.
2. **Check the branch.** Work on the branch recorded in `CHUNK-PLAN.md`. If the checkout is on another one, report that and stop.
3. **Check `## Progress` and `## Approvals`.** If an earlier step or approval is missing, report what is missing and stop, unless the request names that step. A pending pilot overrides a larger chunk count.
4. **Read `## Bar corrections`,** and apply them over this file.

## Roles

- **Coordinator:** the session the person talks to. It gives out work, updates `CHUNK-PLAN.md`, commits, and gets approvals. It holds only the short replies, not the findings. The person sets how many workers run at once and on which model; record that in `## Bar corrections`.
- **Worker:** an agent given one unit: a chunk, a flow, a set of UI states or a batch. It writes only that unit's files. It does not delegate, commit, edit `CHUNK-PLAN.md` or wait for approval. It replies in at most 10 lines: counts by severity, titles of P0 and P1 findings, and any file it could not read.

## Ground rules

- Do only what the request says, then stop. Never start or widen a review without being asked.
- Usage limits are finite. Run only the amount of work requested, save it, and report how far the audit got.
- An audit changes no product code. Anything that needs a code change, such as a proof or a temporary seed, is built in a temporary worktree outside the repo and saved as a patch.
- Read code under review whole. Never sample it, and never let a search stand in for reading it: partial reading causes invented findings. Files over 2000 lines need several reads.
- Read `docs/ENGINEERING.md` first. Search `docs/DECISIONS.md` by crate and topic before reporting a choice that may be deliberate: a recorded decision is a finding only if it causes a real failure. Do not report what `docs/ROADMAP.md` already lists as open.
- Do not read earlier audits, or other chunks' findings. Reviews stay independent.
- Do not run the full gate. Run focused tests when they confirm a suspicion.
- Testing on Linux, real installers and real updates is done by hand after the fixes, and is not part of the audit.

## Scope

Name the scope in the request. Without one, ask.

| Scope       | Covers                                                                                                                                                                                                                  |
| ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `desktop`   | `crates/`, `scripts/`, `.github/`, `config/`, `.githooks/`, `.cargo/config.toml`, and the root `Cargo.toml`, `clippy.toml`, `deny.toml` and `rust-toolchain.toml`. Input arriving through `crates/bridge` is untrusted. |
| `extension` | `apps/extension` and `packages/`.                                                                                                                                                                                       |

Left out everywhere: generated code, data snapshots, binary files and standard licence texts.

## Results

Everything is in the run's folder. Commit after every finished row of `## Progress`, so that hitting a limit loses at most the unit in progress.

- `CHUNK-PLAN.md`: the run's plan and state:
  - the branch, and the commit the plan was made from
  - the dependency graph
  - every chunk with its job, risk, why it sits where it does in the order, its files and its line count
  - `## Progress`: one row per unit of every stage (each tool, fault test, chunk, flow, UI window, batch), each `pending`, `in progress` or `done`
  - `## Approvals`: the chunk plan, the pilot and the fix order
  - `## Bar corrections`: anything the person corrected, which every worker reads
- `FEATURES.md`: the feature map.
- `tools.md` and `fault-tests.md`: what the tool runs and fault tests showed.
- `chunks/NN.md` and `contracts/NN.md`: a chunk's review and its contract.
- `cross/<flow>.md`: a cross-module review.
- `ui.md`: the list of UI states and their review. Screenshots go in `%TEMP%\wirecatch-audit\<run>\`, outside the repo.
- `GROUPS.md`: findings grouped by related cause, with each batch's findings.
- `verdicts/<batch>.md`: each batch's verdicts.
- `proofs/<id>.patch`: a failing test or reproduction against the recorded commit, with one line on how to run it and the output it gave.
- `AUDIT-LOG.md`: the final ordered list.

## Stage 0: Prepare

### Plan the chunks

Do this right before the review, because the code changes between audits. This file never holds a graph or a chunk list.

A chunk is a set of files that do one job together, and its place in the order is a judgement about what is most worth reviewing next. A script may supply facts, such as the dependency graph and line counts, and check that every file is assigned. It does not decide the groups or the order.

1. **Generate the dependency graph** fresh from `cargo metadata --no-deps --format-version 1`. It shows what relies on what. It is an input, not a route to walk.
2. **Understand each crate before dividing it.** Read its `lib.rs` or `main.rs`, its module tree, its public API and the decisions that mention it. Work out what each module is responsible for, which modules share state, and which flows run through several of them.
3. **Group by responsibility.** Keep together the modules that serve one job or one flow, such as resume and the code it depends on to write to disk. Each test goes with the code it exercises, judged by what it tests rather than by its file name.
4. **Treat size as a limit, not a target.** A chunk must fit one reader, about 8,000 lines or 300 KB at most. Adjust this after the pilot and record the change. Split a larger group only at a natural seam, where the two parts have a small, clear interface. A small chunk is fine; do not merge unrelated crates just because each is small.
5. **Decide the order by value.** Start where a defect would hurt most and where review is most likely to find one: code with real logic that a lot depends on. Move code with little logic to the end even when the graph puts it first; a crate of mostly type definitions gives a poor return as a starting point. Among chunks of similar value, review what others depend on first, so its contract is ready for them.
6. **Assign every file in scope to exactly one chunk.**
7. **List the rest of the work** in `## Progress`.
8. **Show the plan and wait.** Record the approval before any review starts.

If anything about the order or grouping is unclear, ask rather than guess.

For a repeat audit, review only the chunks whose files changed since the commit recorded in the last audit's chunk plan.

### Map the features

List what the product does, from the code, `README.md`, `docs/ROADMAP.md` and `docs/DECISIONS.md`. For core features only (such as segmented downloads, resume, queues, scheduling, media capture and failure handling), note how established download managers handle the same thing, searching the internet where needed. Keep this short: it must not use up the limit the review needs. Reviewers use this to judge how a feature should work; a difference from other apps is not a finding by itself.

### Run the tools and fault tests

These come before the reviews, because reading code finds logic errors well but rarely finds races, hangs and corruption after a crash. Failures tell reviewers where to look. They cost machine time more than usage limit. Nothing is installed into the repo, and all output goes outside it.

| Looking for                          | Tool                                                     | How                                                                                                                               |
| ------------------------------------ | -------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| Untested code                        | `cargo llvm-cov`                                         | `--workspace --no-fail-fast --json --summary-only`                                                                                |
| Tests that cannot fail               | `cargo-mutants` (`cargo install cargo-mutants --locked`) | Only named high-risk files with `--file`, plus `--timeout`, a low `--jobs`, `--output` outside the repo and `--iterate` to resume |
| Duplicate code                       | `jscpd` (`npx --yes jscpd@4`)                            | Rust and JavaScript                                                                                                               |
| Unused TypeScript, `extension` scope | `knip`                                                   |                                                                                                                                   |

A mutation run can last days, so it gets a time cap, 2 hours unless the person says otherwise. Record in `tools.md` what did not run. Tool results are leads: the reviewer of that code decides whether they are findings.

Then test the real downloader against a local server that injects faults: dropped connections, no range support, wrong sizes, a file changing mid-download, a full disk, files over 4 GB, and a killed process followed by a resume. Compare each finished file's checksum with the original.

- **First list which faults existing tests already prove** (`crates/engine/tests`, `crates/core/tests`), and write only the rest.
- **Targets:** the CLI for the engine, `Manager` in `crates/core` for resume at the download-list level. The CLI does not reach `core` or `store`.
- **Server:** the server in `crates/test-support` holds the whole body in memory and cannot change it mid-download. Large files and changing bodies need a small server that generates bytes from the offset, built in a temporary worktree.
- **Full disk:** only on a small dedicated volume the person provides, otherwise record it as untested. Never fill a real disk.
- **Results:** each failure is a finding `F-n` in `fault-tests.md`, in the format below. Each new fault test is saved in `proofs/`; the fixing work turns them into permanent regression tests.

## Stage 1: Review

### Chunks

Start with a pilot: review one high-risk chunk, then stop so the person can read its findings, correct the bar and see what a chunk costs in usage. Record the approval and any corrections. Continue only after that.

Give each chunk to its own worker, in the plan's order. The worker reads `FEATURES.md`, `tools.md`, `fault-tests.md` and the `contracts/` of chunks already reviewed. It opens code outside its chunk only when a finding depends on it. It reads up to 10 files in parallel per message to save requests. After each batch, it updates `chunks/NN.md` with new findings and finished files. If that file already exists, it resumes from it.

Look for:

- **Bugs and wrong implementations:** behaviour that differs from what the code, its docs or the relevant spec promise.
- **Broken features:** stubs, settings nothing reads, buttons or commands that do nothing or only partly work.
- **Inefficient code** with a real cost in time, memory or disk.
- **Duplicate logic and code that breaks DRY**, mainly within an area, since areas were built as separate tracks.
- **Dead code, and code that adds lines without doing anything.**
- **Fake tests:** tests that cannot fail, assert only their own mocks, or do not cover what their name claims.
- **Design faults, including SOLID violations,** only with a concrete cost, such as a change that must be made in several places, or copies that have drifted apart.

Pay special attention to what hurts a download manager most: file corruption, lost progress, resume, partial writes, cancellation, races, hangs, leaks, unbounded memory, panics on outside input, path traversal, overflow on large files, and error paths.

Not findings: style preferences, speculative hardening, "could be cleaner".

`contracts/NN.md`: at most one page on the public API, guarantees and invariants other chunks rely on, as the code really behaves.

`chunks/NN.md`:

```markdown
# Chunk NN: <scope>

## Files read

<paths, one per line>

## Findings

### NN-1 [P0|P1|P2|P3] <one-line title>

- Where: `path`, and the function or type (no line numbers: they go wrong as fixes land)
- Category: bug | broken feature | wrong implementation | inefficiency | duplicate | dead code | no-op code | fake test | design
- Failure: the exact input or state, then the wrong result or cost
- Evidence: the code facts that prove it, and any test run
- Confidence: certain | likely

## Suggestions

### NN-S1 <one-line title>

- Today: how it works now
- Better: what would improve it, and how established download managers do it
- Value: what a person gains
```

| Severity | Meaning                                                                                |
| -------- | -------------------------------------------------------------------------------------- |
| P0       | Data loss, file corruption or a security hole                                          |
| P1       | Broken feature, crash, hang or failed recovery                                         |
| P2       | Wrong implementation, or inefficiency with a measurable cost                           |
| P3       | Duplicate or dead code, no-op code, fake tests, or a design fault with a concrete cost |

### Cross-module flows

After the chunks, review what no single chunk can see. Each flow is one unit with its own file, `cross/<flow>.md`, finding IDs `X<flow>-n` and the same size limit as a chunk; split a flow that is too large.

- **Full paths:** a request from its entry point through to the file on disk, for normal downloads, media downloads and app updates.
- **Feature behaviour:** trace each feature in `FEATURES.md` from its button or command to the code that does the work, and confirm it works.
- **Failure and recovery:** crash and resume, shutdown, concurrency, database migrations.
- **Security:** bridge and pipe input, path handling, update signatures, add-on unpacking.
- **Repeated logic** across crates, starting from the `jscpd` results.
- **Second review:** resume and disk writes again, each by a new worker looking only at how they fail, because a single pass misses things.

### UI

UI code is reviewed in its chunks like any other code, including for bugs. How the UI looks and behaves needs the running app.

List every window and flow in `ui.md`, including those that appear only in certain states: the download pop-up, failure notifications, dialogs, empty, running and error states, in both themes. Mark each done as it is analysed, in small batches. Study each screenshot for broken layout, wrong states, unclear wording and UI or UX improvements. Broken behaviour is a finding `U-n`; an improvement is a suggestion `U-Sn`.

Driving the app without taking the person's mouse or keyboard:

- **Start:** `cargo build -p wirecatch-desktop --profile ui`, then run `target/ui/wirecatch-app.exe` with `WIRECATCH_DATA_DIR` set to a scratch folder.
- **See and click:** capture windows with `PrintWindow` (flag 2), and send input with `PostMessage` (`WM_MOUSEMOVE`, `WM_LBUTTONDOWN` and `WM_LBUTTONUP`, `WM_CHAR`, `WM_KEYDOWN`) at the capture's coordinates.
- **Downloads from a browser:** the app's pipe is `\\.\pipe\wirecatch-app-dev-*`, taking a 4-byte little-endian length and JSON. It must be opened at the Identification impersonation level: .NET `NamedPipeClientStream` can, Node cannot. Send a hello first, then `add_download`.
- **The bridge:** a plain `wirecatch-bridge`, even a debug build, reaches the person's installed app and opens windows there. To reach the dev app, build it with `--features test-overrides` and set `WIRECATCH_BRIDGE_TEST_ENDPOINT` to the dev pipe, whose key is in `%LOCALAPPDATA%\Wirecatch Dev\endpoint-key`.
- **Local links:** links to the person's own network are refused, so a local test server also needs a local `page_url`.
- **Files:** downloads go to the real Downloads folder. Use a unique name and delete them afterwards.
- **Update banner:** it needs a temporary seed of `Stage::Available` in `app_update::start`. Build that in a temporary worktree, or record it as untested.
- **Stopping:** stop test processes by command line or path, never by start time.

## Stage 2: Validate

Validate in batches of related findings, not one agent per finding. Related findings share code, so one read covers them all, which costs less and shows connections a single-finding check cannot.

1. **Read all the findings together.** Read only the findings and suggestions in `chunks/`, `cross/`, `ui.md` and `fault-tests.md`. They are small next to the code, so this is cheap.
2. **Find related causes.** Issues in different modules may share one main cause. Group them by cause, then by shared flow or area. A finding with no relation joins a batch for its area. A batch's code must fit one reader, within the chunk size limit. Write the groups to `GROUPS.md` and add a `## Progress` row per batch.
3. **Validate batch by batch.** Each batch goes to one worker that found none of its findings. It reads the code the batch shares, tries to disprove each finding on its own merits, and writes a verdict for each in `verdicts/<batch>.md` (one file per batch, so validators running together never edit the same file): `- <id>: confirmed | disproved | downgraded to Pn, <reason>`. It records whether the group really shares a cause, splits it if not, and reports other places the same cause appears.
4. **Check new findings too.** A place reported in step 3 is a new finding `B<batch>-n`. The next batch's worker, or a small final batch, validates it, never the agent that reported it.
5. **Prove the serious ones.** P0 and P1 findings need a failing test or a reproduction in `proofs/` where one is possible.

Disproved findings do not reach the audit log.

## Stage 3: Organize

Turn the valid findings into one exact fix order. This is a stage of its own because a good order saves a lot of work, and it is cheap: it reads `GROUPS.md`, `proofs/` and little code.

1. **Make fix items.** One item is one change: findings of one cause, or findings too closely coupled to fix apart, become a single item.
2. **Assess each item:** highest severity, findings it resolves, effort (small, medium or large), what it blocks, what blocks it, and what it touches.
3. **Put them in order,** applying these rules in turn:
   1. An item comes after whatever blocks it, whatever its severity.
   2. A P0 comes before everything else it does not depend on.
   3. A change that reshapes shared code comes before the fixes inside that code, so nothing is fixed twice. Merging duplicates comes before fixing a bug in them.
   4. Next comes value: higher severity first, then more findings resolved, then more items unblocked.
   5. Effort settles what remains: among items of similar value, the smaller goes first. An easy fix does not jump above a more severe one, except where it clears the way.
4. **Add a check where it pays.** Where a lint or gate check could catch that kind of bug automatically, name it on the item, so the kind cannot come back.
5. **Write `AUDIT-LOG.md`:**
   - the fix order as a numbered table: number, item, finding IDs, severity, effort, why it sits here in one line, proof, status
   - the findings in full, grouped under their items
   - the suggestions, apart from the fix order; include one only if it clearly helps the person using the app, not because another app differs
   - the chunks found clean, the files read, and what could not be tested, stated as untested rather than as passing
6. **Show the order and wait.** The person may move items. Record the approval. Suggestions get no severity and no place in the order until the person accepts one, one by one.
7. **Finish.** Add the fix items to `docs/ROADMAP.md`, update any docs the audit affects, and run `node scripts/gate.mjs text docs commits`; CI runs the other checks on the pull request.

## After the audit: fixing

Fixes work down the numbered list, one item at a time, each in its own pull request and starting from its proof.

- **Status** is `open`, `fixed` or `won't fix (reason)`. When an item is finished, mark it in `AUDIT-LOG.md` and in `docs/ROADMAP.md`. Items fixed together are marked together.
- **Before starting an item,** check that it still applies, since an earlier fix may have resolved or changed it. If so, mark it and move on.
- **No new audit starts** until every item is fixed or marked won't fix.
