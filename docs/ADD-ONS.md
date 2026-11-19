# Add-ons

Status: proposed, not agreed. The phases and costs below are estimates for one person working with AI. At P0 it becomes a decision in `docs/DECISIONS.md`, and this file is then kept as the settled plan, the way `docs/EXTENSION-SHELL.md` is.

## The name

These are **add-ons**, not extensions. "Extension" already means the browser extension throughout this repository, and two meanings for one word would cost a paragraph of explanation in every future decision.

## What we want

1. **A site changes, and the fix does not wait for a Wirecatch release.** Today every new site, every new transport and every new post-step is ours to ship.
2. **The app stays the only thing that touches payload bytes.** Every guarantee the download manager has (resume, checksums, the queue, the scheduler, the progress UI, the work folder) survives any add-on, because no add-on moves data.
3. **A person can write one.** A stranger with a text editor and a documented contract can add a site we have never seen.
4. **An add-on cannot break the app.** A crash, a hang or a bad answer is one failed download with a message, never a corrupted install or a frozen window.
5. **No add-on can reach the local network.** The rule that a link from the internet cannot reach the person's own network (D34, D39) applies to add-ons as strictly as it applies to yt-dlp (D136).

## The problem today

`crates/addons` installs exactly one add-on: the media add-on, whose components are a closed set (`ComponentId`), in one folder, from one signing key, with one update check (D20, D21). That machinery is good and most of it is reusable. What it cannot do is hold a second add-on, or one we did not build.

The extension point that exists today is the transport enum. `Format::transport` in `crates/media/src/ytdlp/info.rs` answers `File`, `Hls` or `YtDlp`, and `crates/core/src/manager/media_run/page.rs` fetches with the engine when every chosen format is one of the first two and has an address, otherwise it hands the same selection to yt-dlp. A new transport, a site yt-dlp reads badly, or a step after the download is a new Wirecatch release.

## The rule: describe, don't fetch

An add-on answers two questions: **what is at this address**, and **what should happen after this download finishes**. It never moves payload bytes.

That single restriction is what makes the rest of the design work:

- The engine stays the only HTTP client for payload, so resume, chunking, checksums, retries and the disk work folder keep working unchanged.
- An add-on cannot break a transfer it does not perform.
- A bad or hostile add-on can lie about a video. It cannot corrupt a file, and it cannot bypass the engine's address rules.
- Any add-on that cannot work this way is not supported, and that is deliberate rather than a gap to close later.

## The five kinds

| Kind | Gets | Returns | Network | Files |
| --- | --- | --- | --- | --- |
| Extractor | A page address | Representations, shaped like yt-dlp's `Info` | Yes, through the guard | No |
| Transform | One fetched segment or file's bytes | New bytes | No | No |
| Postprocessor | A finished file's path | Nothing, or a new path | No, unless declared | Yes |
| Metadata | A page address | Title, cover, episode, description | Yes, through the guard | No |
| Sink | A finished file's path | Nothing | Yes, through the guard | Read only |

One rule cuts across the table: **no kind gets both a writable file and the network**, except a sink, whose whole purpose is to send one named file somewhere and which the person turns on for that add-on. That closes the shortest path from "an add-on I installed" to "everything in my Documents folder, uploaded".

Extractors come first. The other four are designed for, not built, until the extractor contract has survived contact with real pages.

## The contract

**One process per request.** The runner starts the program, writes one request to its standard input, reads replies until it ends, and kills the process tree. No long-lived worker in v1: the supervisor in `crates/streams/src/process` is built for exactly this shape, and a crash cannot poison later calls. A kept-alive worker is a performance decision to make later, against a measurement.

**JSON Lines.** One object per line, in both directions. Not the bridge's length-prefixed frames (D14): an add-on in any language must be able to speak this with its standard library, and a line of JSON is the smallest thing that makes that true.

**Version first.** The request names the protocol version it speaks, and the app refuses an add-on whose declared range does not include its own, the way D32 refuses a client that shares no version. The add-on ABI versions independently of the app protocol, and does not reuse the bridge's `hello`.

**`describe` is the only operation in v1.** A `probe` step, which answers "is this address mine" in milliseconds before the expensive call, waits until there is a second extractor that needs disambiguating. Routing by declared hosts is enough for one.

**The result is a subset of yt-dlp's `Info`**, not a new vocabulary. This is the decision that keeps the integration small: `crates/media/src/choice.rs` already builds the offer list from `Info` (`from_page`), and the offer list is what carries the remembered resolution, the quality fallback and the ask-each-time rule (D24, D200). A result shape that fed `crates/streams/src/select.rs` instead would lose all of that, because `select.rs` picks tracks from a parsed manifest and knows nothing about the quality window.

The consequence is that the first conforming add-on can be yt-dlp itself, re-emitting what it already prints. That is the cheapest possible proof that the contract matches reality.

**Errors are typed.** A closed set of codes (`unsupported`, `not_found`, `needs_login`, `refused`, `too_large`, `stopped`) and one sentence of reason. A stack trace goes to the add-on's standard error, which the runner keeps as a tail for diagnostics, and never to the person. Every failure becomes a message with a fix, through `crates/messages`, like any other download failure.

**Every address an add-on returns is re-checked by the app**, with the same rules as an address the person typed: the local-network classifier (D34), the redirect rules, and the scheme. The add-on's opinion about where a file lives is never trusted, only proposed.

## The manifest

The existing manifest gains four fields: an id, the kinds it offers, the hosts it claims, and the protocol range it speaks. Everything else stays: version, per-file SHA-256, safe unpacking, atomic swap, the record written last.

Hosts are globs on the host only, never a path claim, so routing needs no add-on process. Declared hosts build the routing table at startup. A host nobody claims keeps today's path: a plain download that becomes a page read when the server answers with a page (D51.3).

## The runner

`crates/streams/src/process` already provides the hard part: an argument vector with no shell, a job object on Windows and a process group on Unix, killed as a whole on cancel, timeout, stall or drop, no console window, Python's environment stripped, and standard output capped at 64 MiB. The add-on runner reuses it rather than growing a second supervisor.

What it adds is the request channel (one line in, replies out) and the mapping from a failed run to a message. A run that stalls for `STALL_TIMEOUT` is already stopped; a describe gets a deadline of its own, the media path's read timeout, not a new number.

## Trust, privacy and the network

**Official add-ons** are signed by us and update themselves, exactly as the media add-on does today. **A user's own add-on** is unsigned, and the first install pins the file's hash. If the hash changes, the app asks again. Nothing unsigned updates itself.

**The network guard is part of the contract, not a later hardening pass.** A describe call reads pages, so it makes requests, and D136 already decided how a Wirecatch subprocess is allowed to do that: a loopback proxy per run, resolving each host itself, refusing any host with a local address, one request per connection, no redirect hops followed blindly. An add-on that bypassed that would undo a property the app spends real code on. The add-on's HTTP client reaches the internet through the same guard, or the add-on gets no network at all.

**v1 takes public addresses only, from an explicit act by the person.** No cookies, no session tokens, no private-session downloads (D82, D279.6). Signed-in media stays on the existing yt-dlp path, which already handles cookies, private spans and the guard together. That boundary is the one that makes the first external add-on cheap to reason about; moving it is a later decision with its own entry.

## Installation

The runner comes before generalizing install. That order is deliberate. Once one add-on has run through the contract, the manifest knows what an add-on actually needs to declare, and generalizing `crates/addons` is a mechanical change with a known target. Done the other way round, the installer is designed against guesses and rebuilt.

## Phases

Costs are AI-speed work with review, not calendar time. Human gates are listed where they block.

### P0. The boundary (0.5 day, no code)

Settle this document, record the decision, and amend D15 explicitly: a third-party ABI may live in `wirecatch-protocol` under its own version constant, because D15 currently says the crate is the single source of truth for every type crossing a process boundary, and that sentence is now either widened or contradicted.

Decisions fixed here: one process per request, JSON Lines, public addresses only, no cookies, the five kinds, and the no-file-plus-network rule.

Exit: the decision exists, and a reviewer can read this file and say what is in and what is out.

### P1. The runner and the contract (2 to 3 days)

The wire types, the runner on the existing supervisor, and a test add-on that returns a fixed `Info`. No real site, no install changes.

Exit: a test add-on describes an address; its whole process tree dies on cancel and on timeout; a crash becomes one typed error with a reason; the 64 MiB cap holds; a describe through the D136 guard reaches a public host and is refused a local one.

That last clause is the one real unknown in the phase. If the guard cannot carry a general HTTP client (it refuses `Expect` and allows one request per connection), the answer is either a guard mode for add-ons or no network in v1. Either is fine, but it is settled here and not after P3.

### P2. One real extractor, measured (2 to 3 days)

The media path reads a page through a conforming add-on, the result becomes `Info`, and `from_page` builds the offers with nothing changed in the quality window. The yt-dlp fallback is not touched and stays available.

Exit is a number, not a feeling: a fixture list of public pages, and the share of them the contract covers without cookies, with and without the fallback. That number goes into this document.

### P3. Installation (1 to 2 days)

Multiple add-ons in `crates/addons`: an id per add-on, a folder and record each, an index, list, enable, disable, remove. The media add-on becomes an ordinary entry rather than a special case.

Exit: two add-ons installed side by side, one updated, one removed, with a kill in the middle of each leaving a coherent state.

### P4. The author kit (1 to 2 days)

A scaffold command, a Python SDK and a Rust SDK, and a conformance binary that runs an add-on against a fixture corpus. The conformance suite is the gate for listing anything official, not polish.

Exit: someone who has never seen the code writes an add-on from the documents alone and gets it passing without asking a question.

### P5. Trust and lifecycle (1 day)

Trust on first use, hash pinning, re-consent on change, no auto-update for anything unsigned, and a permission line in the UI that says what the add-on gets in plain words.

Exit: an unsigned add-on's file changing on disk produces a prompt, not a silent run.

### P6. The second kind (not scheduled)

A postprocessor, because it is the smallest proof that the contract generalizes beyond extraction. Transforms, metadata and sinks follow only if someone needs them.

## What would prove this wrong

These are the measurements that should stop the plan, kept here so a later phase cannot quietly continue past them.

1. **Coverage without cookies is close to zero on the fixture list.** Then the describe contract is a power-user tool, not a product feature. The official media add-on stays the answer, and we stop at P3 rather than hand over session credentials.
2. **The D136 guard cannot carry a general client.** Then add-ons get no network in v1, which makes an extractor impossible, which reduces the system to postprocessors. Still useful, much smaller.
3. **Process startup dominates.** If a describe takes longer before the add-on starts than the add-on takes to answer, routing needs a kept-alive worker and P1's shape changes. Measure it in P1 on Windows, where process creation is slowest.
4. **Nobody writes one.** If the SDK and the scaffold produce no third-party add-on within a release cycle, the contract was for us all along, and it should be judged as an internal refactor that made the media add-on ordinary, not as an ecosystem.

## Open questions

- Whether formats may carry their own `http_headers`, which yt-dlp formats sometimes do. It is not payload, but it lets an add-on influence the engine's requests.
- Whether add-ons must be self-contained programs or may need a language runtime, which changes the install story completely.
- Whether there is an official index at all in v1, or only the signed media add-on plus sideloading.
- Fixture pages must be synthetic or public-domain, so the conformance corpus can live in the repository.

## Deliberately out of scope

- Loading an add-on into the app's process.
- Any add-on that returns payload bytes.
- Add-ons that depend on other add-ons: that is a dependency resolver, and there is no appetite for one.
- Arbitrary UI contributed by an add-on. At most, typed settings fields and one row in a details view, and not before P6.
- A marketplace with payment.
- Add-ons that use the person's cookies or a private session, in v1.
