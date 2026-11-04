# Audit brief

Final audit before the first release of the desktop app. The extension (`apps/extension`) and `packages/` are out of scope. Input arriving through `crates/bridge` is treated as untrusted.

You review one chunk. Everything you need is in the repo, so a new session can resume any chunk.

## Files

- `docs/audit/manifests/NN.txt`: your files, with line counts.
- `docs/audit/chunks/NN.md`: your output.
- `docs/audit/LEDGER.md`: status of all chunks. Only the coordinating session edits it.
- `docs/audit/chunks/*.md` of lower layers: read only their `## Contract` sections, then open a lower-level file only when a finding depends on it.

Read `docs/ENGINEERING.md` first. Search `docs/DECISIONS.md` for what a suspect choice touches; a recorded decision is not a finding unless it causes a real failure. `docs/ROADMAP.md` lists known open items: do not report them again. Do not use git history to find old audit reports.

## Reading

- Read every manifest file completely. Files over 2000 lines need several `Read` calls with `offset`. Do not sample, and do not rely on grep in place of reading.
- Read up to 10 files in parallel in one message, to save requests.
- If `chunks/NN.md` already exists, resume: skip files under `## Files read`, keep existing findings.
- After each batch, immediately update `chunks/NN.md` with new findings and the files finished. A stop at any moment must lose no more than one batch.

## What to look for

Bugs, broken features, wrong implementations, inefficiency with a real cost, duplicated logic, dead code, code that does nothing, fake tests (tests that cannot fail, assert only mocks, or do not cover what their name claims), and design faults (DRY, SOLID) that cause a concrete problem. For a download manager, attend to: file corruption, lost progress, resume, partial writes, cancellation, races, hangs, leaks, unbounded memory, panics on external input, path traversal, integer overflow on large files, and error paths.

**Real issues only.** A finding needs a concrete failure or cost you can describe. No style opinions, no speculative hardening, no "could be cleaner". An empty findings list is a valid and good result; never pad it.

Do not edit any code. Do not run the full gate. You may run a focused test or small scratch program under the scratchpad to confirm a suspicion.

## Output format of `chunks/NN.md`

```markdown
# Chunk NN: <scope>

## Files read
<paths, one per line>

## Contract
<at most one page: public API, guarantees and invariants that later chunks rely on, as the code really behaves>

## Findings

### NN-1 [P0|P1|P2|P3] <one-line title>
- Where: `path:line`
- Category: bug | broken feature | wrong implementation | inefficiency | duplicate | dead code | no-op code | fake test | design
- Failure: exact input or state, then the wrong result or cost
- Evidence: the code facts that prove it, and any test run
- Confidence: certain | likely
```

Severity: P0 data loss, file corruption or a security hole. P1 broken feature, crash, hang or failed recovery. P2 wrong implementation, or inefficiency with a measurable cost. P3 duplicate or dead code, no-op code, fake tests, or a design fault with a concrete cost.

No em dashes. Do not commit; the coordinating session commits.

When done, reply in at most 10 lines: counts by severity, the titles of P0 and P1 findings, and any manifest file you could not read.

## Verification

A verifier never reviewed the chunk it checks. For each finding it reads the code cited and tries to disprove it, then adds a line to the finding: `- Verdict: confirmed | disproved | downgraded to Pn, <reason>`. P0 and P1 need a failing test or reproduction where one is possible, kept in the scratchpad and described in the verdict.
