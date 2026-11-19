# The extension as a shell

Status: built, recorded as D280 in `docs/DECISIONS.md`. The plan below is kept as it was settled; what was built differently, and what is still open, is listed first.

## As built

Differences from the plan:

- **Resume state of a private download.** The engine has no in-memory mode. The resume file stays beside the part file while the download runs, as a browser's own private download writes its partial file. Its partial files are listed by path alone, so the next start deletes what a crash left, and forgetting the download removes them.
- **Logs.** A private download's work runs inside a span the log leaves out (`wirecatch_logging::PRIVATE_SPAN`), so no line naming its address, page or file is written, whichever crate logs it. Every task and blocking job of a download's work starts in the caller's span through `wirecatch_engine::task`, which the `task-span` check holds to in `engine`, `streams`, `media` and `core`; a `JoinSet` takes `.in_current_span()` by hand.
- **Notifications.** A private download's notifications are brief and never kept in the system's list, rather than withdrawn later, and show only as the system's, never as a notice in a browser, where another window or browser could show them.
- **The private session's id** is kept in `storage.local`, not `storage.session`, so an update or a reload of the extension keeps the session and the app needs no guess that a new id is an old session renamed. An id found at start with no private window open is ended then. A connection claims every session it names, also one first named on a capture. A `hello` with no private window does not end the connection's earlier session at once, and an end the extension finds at start goes nowhere while the app is not connected: the grace ends both. When the app leaves a capture because its session ended while a private window is still open, the extension starts a new session, so only that capture stays in the browser.
- **`frame_url`** reaches the app with every check, for rules about frames; no rule reads it yet.
- **Capture filters.** Capture off, ignored sites, file types and the minimum size apply to the browser's own downloads only, as before: what the person sends from the menu or the media button goes whatever they say.
- **The Save As deadline** is `CAPTURE_DEADLINE_MS`, 12 seconds: the bridge's start budget, the app's answer to `hello`, and room for `check_download`.

Still open:

- Measuring the deadline with the app connected, closed and missing in real Chromium and Firefox builds.
- The Firefox sources zip still holds only `apps/extension`, and there is no check that rebuilds the extension from it.
- Listing the add-on on addons.mozilla.org, which only the maintainer can do.

## What we want

1. **The extension rarely changes.** Adding, removing or renaming an option, changing its keys, values, choices or wording, and adding rules about which downloads are taken all happen in the app. A new app release works with the extension already in people's browsers, and a new extension release reaches them by itself.
2. **Few submissions, each easy to accept.** How fast a store reviews is up to the store. What Wirecatch controls is how often it submits and what a reviewer finds: a small, readable extension inside both stores' rules, a package that rebuilds from its sources, and notes that let a reviewer try every feature.

## The problem today

The app already keeps every setting (D61), but the extension still knows each one by name, so most app changes need a new extension build:

- `ExtensionSettings` and `BrowserControls` in `crates/protocol/src/extension.rs` are typed structs the popup reads field by field. Unknown fields are passed over (`isExtensionSettings`), but a new enum value fails the whole message (D32.6), and every new field needs new popup code.
- `apps/extension/src/ui/settings.ts` lists every row the popup shows, with its label and help text.
- `packages/protocol/src/media-choices.ts` lists the choices of each media setting.
- `apps/extension/src/lib/capture/should-capture.ts` holds every rule about which downloads the app takes.
- `apps/extension/src/lib/media/detect.ts` holds the media tables: video and audio extensions, media types, and the 100 KB minimum.
- The extension bundles the Public Suffix List (`tldts`), and its freshness is checked before every release (D35), a recurring reason to publish. It decides three things: the `Cookie` header kept for apps on protocol version 1 (D112.3), whether media is cross-site (`isCrossSite`, `hub/media-sender.ts`), and which site to read partitioned cookies for and Firefox's `firstPartyDomain` (`cookieReads`, `lib/cookies.ts`).
- The Firefox build is signed on the unlisted channel and has no `update_url` (`apps/extension/PUBLISHING.md`), so every new version has to be installed by hand.

## The line between the two

**The extension owns how things look, what only a browser can do, and the person's privacy choices. The app owns what exists, its values, its checks and the final decision.**

Stays in the extension:

- The popup's design: layout, header, capture card, the list of media on the page, the footer, themes, motion, and the look of every control.
- The button over videos: its design and placement.
- What only an extension can do: seeing and pausing browser downloads, reading cookies, watching requests for media, the context menu, the keyboard shortcut, the notices on pages, native messaging.
- Facts only the browser knows: a download already finished, one the extension started itself, a `blob:` or `data:` address, a download from the browser's own pages, one the browser flagged as dangerous.
- The privacy gates, applied before any cookie is read or anything is sent, with the values the app sends in `ShellConfig` (below). The values live in the app, but the code that enforces them is in the package a store reviewer reads.

Moves to the app:

- Which options the popup shows, in which groups and in which order, with their keys, labels, help, choices, limits, values and the summary line of each fold.
- The checks on every change, and changes that move several values at once.
- The final yes or no on each download, including any rule added later.
- Which cookies go with a download or a video, decided with the app's own current suffix list (`wirecatch_sites`).
- The media tables and the filter values, as data.
- Every word that belongs to an option, and its translations.

## Design

### Options the app describes

The app sends an option list: a revision, then groups, each with a stable `id`, a title, a summary line and whether it starts folded, and in each group a list of options. An option has:

- `id`: a stable, opaque string the extension never interprets, only sends back.
- `kind`: one of a closed set of controls the extension draws (below).
- `label`, `help`, and a `disabled` reason when it cannot be changed.
- `value`, plus what the kind needs: the choices (`value` and `label` each), or the unit, minimum, maximum, step and decimals of a number.

The control kinds, decided once and drawn in the extension's own design:

- `toggle`: on or off.
- `choice`: one of a list. The app includes a saved value that is not among the usual choices (a resolution set in the app, say).
- `choices`: several of a list, such as subtitle languages, with saved ones included the same way.
- `number`: a number in a unit, with decimals where the unit needs them.
- `limit`: a switch with a number under it, where off means no limit. The global speed limit in MB/s is this kind.
- `action`: a button that asks the app to do something by id. The extension adds the page's site when the action asks for it, from a closed list of what it may add.
- `note`: a line of text with no control.
- `browser`: a row the extension fills itself, for what belongs to the browser. The app places it and names which one, from a closed list. The keyboard shortcut is the first, with the browser's shortcut text and its "Change" button (Chromium only).

The two hardest settings today are the proof: the speed limit (`limit`, fractional MB/s, off for no limit), and the audio format, which changes the audio quality with it. The app answers a change of format with both new values and the new quality choices in one new list, so the popup never works out a dependency itself.

### Changing a value and running an action

- `set_option` sends an option id and a change. It is always applied, whatever revision the popup last saw: each change touches one option, so the last change wins, as it does between the app's own window and a popup today. For `choices` the change is what to add and what to remove, never the whole list, so a language added at the same time in another browser is not lost, as the ignored sites work today (`CaptureCard.svelte`). The app checks the change like any change made in the app, applies every value it moves at once, saves, and pushes the new list to every connected browser. A refused change comes back with the app's message, shown on that row. Sending the same change twice changes nothing.
- `invoke_action` sends an action id and the page context it asked for. It is its own call, so retrying a save can never repeat an action.
- A revision is the app's instance id, new at every start, and a counter. The extension keeps the newest it has seen from the current instance and drops an older reply that arrives late. A new instance id, after the app restarts, always wins, and the extension forgets what it had seen on every reconnect. Stable group and option ids keep keyboard focus, open folds and a number being typed across a redraw.

### What the extension does with something new or broken

The generated validators cannot do this: they accept only the enum values they know, and one bad item fails a whole array (D32.6). So in the protocol `kind` is an open string, not an enum, and each option travels as raw JSON that a hand-written parser in the extension reads one option at a time. The option list stays out of `the_values_of_enums_inside_messages_are_settled`. Known kinds are validated strictly. An option whose `kind` the extension does not know is drawn as its label and an "Open in Wirecatch" button. A malformed option is left out on its own and the rest of the list still shows. Fields it does not know are passed over. Lists and strings have size limits, and anything past them is cut. So the app can add kinds and fields years after an extension was released.

### Rows that belong to one tab

A row about the current page, such as "Ignore example.com", never travels in the list sent to every browser. When the popup opens, it asks the app with `page_options` and the tab's host name only, never the full address. In a private tab it asks only while private windows are taken, and the same gate holds for an `action` that adds the page's site. The answer names the site to show, and when a parent entry ignores the page (`example.com` for `video.example.com`), it names that entry, so turning the switch off removes it as the popup does today.

### `ShellConfig`: what the extension acts on

A small typed structure the app sends next to the option list, with the same kind of revision, for what the extension itself does:

- whether the button over videos shows
- whether capture is on
- whether private windows are taken ("Work in private windows", D82)
- the ignored sites, the file types and the minimum size, as plain lists and numbers
- the sites with video pages (D51)
- the media tables (below)

New fields are additive and optional. The extension keeps the last valid copy in `storage.session`, so a restarted background picks it up, and an update it cannot read or an older revision leaves the last valid copy in place. The media tables have built-in defaults, since they hold no choice of the person's. Those defaults and the app's initial values come from one generated source, so they never drift.

### Before `ShellConfig` is known

A browser session starts with no copy, and the person's choices are unknown: a setting turned off yesterday must not be taken as on. What the extension does depends on whether the person acted, since only an action may start the app (D25.6):

- **An action** (a browser download, the media button, the menu, the shortcut, "Connect Wirecatch" in the popup) first asks the app for `ShellConfig` alone, with nothing about the download or page attached. Like every request caused by the person, it starts a closed app. A browser download is paused first, right after the browser facts (below), so it cannot finish while the app starts. When the fetch misses the deadline, the download is resumed in the browser and nothing about it is sent. Opening the popup is not such an action: it only connects to an app already running, as today (`hub.ts`, `addPopup`).
- **Passive watching** never starts the app and never sends anything: it reads only the copy the extension holds, and makes no request. In a normal tab the watcher runs on the built-in tables, keeps each media request's `Cookie` header in memory, and shows the button over videos, as today (D61.6). In a private tab, an unknown copy means private windows are off: nothing is read, no button shows, and no notice is placed there. This changes today's code, where `privateWindowsOn` (`hub/app.ts`) asks the app, which starts a closed one, and counts an unreachable app as on, and the watcher (`media/watch.ts`), the button check (`media/center.ts`) and notices (`notices/focus.ts`) all call it passively. Nothing seen passively leaves the extension until the person acts, and that action waits for `ShellConfig` first.

### A capture, step by step

The same steps serve a browser download, the menu, the media button and the shortcut.

1. **Browser facts, in the extension.** A capture stays in the browser, untouched, when one of the browser facts applies.
2. **Pause.** A browser download is paused right after step 1, as today (`capture/handover.ts`), before anything waits on the app. Firefox reports it only in `onCreated`, when it is already writing, so waiting for the app without pausing would let a small file finish first. Pausing sends nothing, so it is safe before the privacy gates. Every "stays in the browser" below resumes it.
3. **Privacy gates, in the extension.** With no `ShellConfig` yet, it is fetched now (above). A capture stays in the browser when capture is off, when it comes from a private window and "Work in private windows" is off, or when it fails the ignored sites, file types or minimum size.
4. **`check_download`, with no cookies.** It carries what rules need: the address and the page's address, the file name, the type, the known size, whether it comes from a private window, and for media the address of the frame that requested it. The app answers take or leave. A take also says which cookies to read: the site whose partition to read, Firefox's first-party domain, and for media whether to send the `Cookie` header the request carried instead (D112.4). These are values the extension's fixed reading code uses, so the extension needs no suffix list.
5. **The handover, as today.** Only an accepted capture has its cookies read and goes out with `add_download`. When that answer is lost, the extension asks the app to withdraw it, `already_started` means the app has it, and when even that fails the person is warned it may be in both places (`handover.ts`).

One deadline runs from the browser's event and covers fetching `ShellConfig`, connecting and `check_download`. The native client's own waits do not bound it (up to the bridge's start budget plus 5 seconds to connect, then 10 seconds per request, `native/timing.ts`). Past it the download goes on in the browser, and an acceptance that arrives later never starts a handover. After an acceptance, reading cookies and `add_download` keep today's limits, and a lost answer adds the 3-second withdraw. On Chromium the whole path, handover included, runs inside `onDeterminingFilename` before the Save As prompt, as today, so the prototype measures the whole path with the app connected, closed and missing, and sets the deadline and the worst case from those numbers. Firefox has no prompt to hold, but the same deadline keeps a paused download from waiting on an app that never answers.

### Media tables

Watching every request stays in the extension, since asking the app about each one would be too slow. The app sends the tables: video extensions, audio extensions, the HLS and DASH extensions and media types, the media types of segments that are never listed, and the smallest file worth listing. How they are applied stays in the extension's code: HLS before DASH before direct files, segments left out, the total size read from partial answers, an unknown size kept.

The watcher keeps each request's `Cookie` header in memory until its answer comes (`media/watch.ts`), before it knows whether the tab may be watched. It checks the tab first instead, so a private tab is never read while private windows are off or not yet known.

### Private windows

Wirecatch handles a private download the way the browser itself does: it works while the private session lasts and leaves no record afterwards. Mozilla's rule is about storing private browsing data, so not storing it is what keeps the add-on accepted. A second consent switch would not: both browsers already make the person allow the extension in private windows ("Run in Private Windows", "Allow in Incognito", off by default).

- "Work in private windows" stays the app's setting (D82), on by default, sent in `ShellConfig` and enforced by the extension. Off, nothing from a private window reaches the app: no download, no media, no cookies.
- Each private session gets a random id when its first private window opens, kept in `storage.local` and sent with every private capture. Every `hello` says whether a private window is open, and the session id. An update or a reload of the extension keeps the id, so the session goes on; an id found at start with no private window open has ended.
- The app keeps a private download in memory only: its cookies, headers, addresses and resume state. Nothing goes to the database, and no resume file is written, since quitting the app forgets the download anyway (the engine's resume file, `crates/engine/src/state.rs`, gains an in-memory mode). On Windows the saved file's Mark of the Web holds only `ZoneId`, without `HostUrl` or `ReferrerUrl` (`crates/files/src/zone.rs`).
- The log never names a private download's address, host, page or file. The log redaction hides only secrets (`crates/logging/src/redact.rs`), so private downloads are logged by their id alone.
- A private download's notifications do not stay in the system's notification centre (`Lasts::Kept`, `crates/shell/src/notifications.rs`): they are transient and withdrawn when the download is forgotten.
- The download shows in the list, marked private, while its session lasts. The app forgets a session's downloads when the extension says its last private window closed, when a `hello` from that connection says no private window is open, when no connection has claimed the session for a short grace time after a disconnect (a browser that quit or crashed), and when the app quits. It refuses a capture for a session that has ended, so a late `add_download` cannot bring one back.
- A forgotten download that is still running is stopped, and its partial files are removed, including the work folders of HLS and DASH downloads (`.wcstream`, `crates/streams`) and of videos. The finished file stays where it was saved, as a browser's private download does.
- The app empties the yt-dlp cookies folder at every start, and removes the partial files of private downloads a crash left, since a crash or a kill skips the cleanup that runs when they are dropped (`crates/media/src/ytdlp/cookies.rs`). To find those after a crash, the app keeps only their paths, never their addresses, in a list it empties at start.
- The same holds in both browsers, so there is one code path.

### While the app is not connected

The popup shows only why and the "Connect Wirecatch" button (`ConnectionNotice.svelte`), as today. A download still wakes the app, through the `ShellConfig` fetch or `check_download`.

### Words

The app translates every option's label, help, summary and choice. The extension translates its own words: the header, the capture card, the controls, the errors, the disconnected screen, and "Open in Wirecatch".

### Protocol

Version 2 was never released, so it is redefined as this design: `PROTOCOL_VERSION` stays 2 and `MIN_PROTOCOL_VERSION` becomes 2. The extension and the app drop every version 1 path, including the typed settings calls (`get_extension_settings`, `update_extension_settings`, `extension_settings_changed` and the others).

`hello` no longer sends `BrowserInfo` (`Hello.browser`, `crates/protocol/src/wire.rs`): the app never reads it, only the end-to-end tests do, and Firefox counts browser details as `technicalAndInteraction`, which must be an optional consent. The `user_agent` sent with a download stays, as a request header the download needs.

Version 1's snapshot in `crates/protocol/released/v1/` stays as it is, as a record. Two tests in `crates/protocol/src/version.rs` assume version 1 is spoken: `every_released_version_keeps_exactly_what_it_had` checks that its calls are still known, and `every_newer_error_code_has_an_older_equivalent` checks how newer error codes map to it. The decision amends D32.8 and D32.3: a released version below `MIN_PROTOCOL_VERSION` is kept only as a record, the first test checks only released versions from the minimum up (none until version 2 is released, when its snapshot is added), and the error-code mapping to version 1 is removed with its test cases. The commit is marked with `!` and a `BREAKING CHANGE:` footer naming version 1, as D32.7 requires.

A test checks the update message in both directions: the version 2 extension against a recorded version 1 `hello` exchange, and the version 2 app against a client that offers only version 1. After this, every change is additive (D32.5), and most app changes no longer touch the protocol at all.

With version 1 gone, the cross-site choice made by the app, and the cookie sites coming in the `check_download` answer, nothing in the extension needs the suffix list, so `tldts` and its release check leave it (D112.3). They leave only once those three are built and tested.

## Store acceptance

### The rules that apply

- **Chrome Web Store, Manifest V3 requirements**: the extension may load data from outside its package, but that data "must not contain any logic". The listed violations are a script loaded from outside the package, `eval` or anything that runs a fetched string, and an interpreter that runs commands fetched as data. Its review guide names broad host permissions, sensitive permissions and the amount of code as what lengthens a review, and every update is reviewed again.
- **addons.mozilla.org**: add-ons must be self-contained and not load remote code, ask only for the permissions they need, and the privacy rules cover every piece of data sent over native messaging. Data from private browsing must not be stored. Since November 2025 new add-ons declare the data they send in the manifest (`data_collection_permissions`), which Firefox shows at install. Implicit consent applies only to add-ons hosted on addons.mozilla.org, only to data sent on a deliberate command, and never to cookies (policy 6.2.2.2), so it does not cover Wirecatch.

### How the design stays inside them

- The app sends values, words, lists and ids, never logic: no expressions, conditions, templates, scripts, regular expressions or HTML. The cookie sites in the `check_download` answer are values too.
- Control kinds, browser rows, action context and media tables are closed sets the extension's own code handles. Anything unknown is shown as "Open in Wirecatch" or passed over, never interpreted.
- `no-eval`, `no-new-func` and Svelte's `no-at-html-tags` are already on. A rule refusing `innerHTML` and `outerHTML` in `apps/extension` is added.
- Private windows follow the section above.
- The Firefox manifest declares under `required`, instead of `none`, the data the extension sends to the app, so Firefox's own consent shows it at install: addresses of downloads, pages and media as `browsingActivity`, cookies and request headers as `websiteContent`, and the act of downloading as `websiteActivity`. `PRIVACY.md` and the Chrome listing answers in `apps/extension/PUBLISHING.md` say the same.
- The Firefox add-on is listed on addons.mozilla.org, which reviews it and updates every copy by itself, so a new extension release reaches people without a reinstall. Unlisted signing stays only for test builds. Listed add-ons must not have "the sole purpose of" launching an outside application (policy 2.5), and a thin extension looks closer to that, so the listing and the reviewer notes lead with what runs in the browser: capturing downloads, finding the video and audio a page plays, and the privacy gates.
- The current manifest declares `required: ['none']` (`wxt.config.ts`) while the extension already sends addresses and cookies, so the declaration is fixed before the next `sign:firefox`, not with the rest of the store work.
- The Firefox source zip holds everything the build needs: `apps/extension`, the workspace packages it uses, and the lockfile. A release check rebuilds the extension from that zip alone and compares the result.
- Each submission comes with reviewer notes: what the companion app is, how to install it, an example option list, and steps that show a capture, a video and the popup.

### What still slows a review

Access to every site stays, because media detection and signed-in downloads must work on any site, not only for the button over videos. Before the first submission, a test covers what still works on a site the person withheld access to, and the reviewer notes explain it. The thin extension helps in two ways that are in our hands: less code to review, and far fewer submissions.

## What still needs an extension release

- A new control kind, browser row, action context, `ShellConfig` field, media table or way of reading cookies.
- A new browser permission (the browser asks the person again, and the store reviews it).
- Browser API changes, and bugs in the extension.
- How the button over videos finds and sits on a particular site's player.
- Any change to the popup's own design.

## Decisions

1. **Scope**: the app defines the options, their checks, the final decision on each capture and which cookies it takes. The extension keeps the design, the browser capabilities and the privacy gates.
2. **Protocol**: version 2 is redefined as this design and becomes the minimum. No compatibility with version 1, whose snapshot stays as a record.
3. **Host permissions**: access to every site stays.
4. **Popup while the app is not connected**: unchanged, only why and the "Connect Wirecatch" button. Downloads still wake the app, and passive watching never does.
5. **An unanswered capture**: no answer before the deadline leaves it in the browser. A lost answer to `add_download` keeps today's withdraw and warning.
6. **Rows about the current page**: asked for by each popup with `page_options` and the host name, never broadcast.
7. **Private windows**: the setting stays in the app and is enforced in the extension, and an unknown setting counts as off for passive watching. A private download lives in memory only and leaves nothing in the database, the log, a lasting notification, the file's Mark of the Web or a resume file. It is forgotten when its private session ends by any route, and an extension update does not end it.
8. **Firefox distribution**: listed on addons.mozilla.org, so updates reach every copy.

## Work, in order

1. One generated source for the default media tables and the app's initial `ShellConfig` values, loaded by both the app and the extension. Alongside it, the Firefox manifest's data declaration under `required` (addresses as `browsingActivity`, cookies and request headers as `websiteContent`, downloading as `websiteActivity`), so no build is signed with `none` again.
2. Prove the riskiest parts end to end before moving every row:
   - the speed limit and the audio format through the option list, with a change from another browser at the same time and an app restart while the popup is open
   - a capture through `check_download` and `add_download`, with the reply lost after the app took it
   - the deadline, measured over the whole Chromium path with the app connected, closed and missing, and an acceptance after the deadline starting nothing
   - a fresh browser session: a Firefox download before `ShellConfig` is known is paused first and sends nothing about itself until the copy arrives, opening the popup or loading a page (private tabs included) with the app closed starts nothing, and a private tab with the copy unknown reads no `Cookie` header and shows no button
   - private media with "Work in private windows" off: opening the popup and playing media reads no `Cookie` header and sends no page address
   - private media with it on: a signed-in video through an interruption and a browser crash, leaving no cookie file, work folder, resume file, history entry, log line naming it, lasting notification or address in the Mark of the Web, only the saved file, and an app restart that finds it gone with its partial files removed
   - an extension update while a private download runs: the download goes on
   - a partitioned cookie (such as `cf_clearance`) and Firefox with first-party isolation, read through the sites in the `check_download` answer
   - cross-site media whose cookie the browser withheld: Wirecatch withholds it too
3. Record the decision in `docs/DECISIONS.md`, with the deadline and anything the prototype changed, including the amendments to D32.3 and D32.8.
4. `crates/protocol`: the option list, `set_option`, `invoke_action`, `page_options`, `ShellConfig`, `check_download` with its cookie sites, the private session id in `hello` and `private_session_ended`, version 2 as the minimum, no `BrowserInfo` in `hello`, `kind` as an open string with each option as raw JSON, and the version tests checking only versions from the minimum, with the version 1 error-code mapping removed. Regenerate TypeScript.
5. `crates/core`, `crates/shell`, `crates/engine`, `crates/files`, `crates/streams`, `crates/logging` and `crates/media`: build the option list from the settings, apply changes and actions through the existing checks, answer `check_download` with the cookie sites, keep private downloads in memory with no resume file (an in-memory mode in `crates/engine`), write only `ZoneId` for them, log them by id only, keep their notifications transient, forget private sessions by every route, remove their stream and video work folders, and at start empty the cookies folder and remove partial files a crash left.
6. `crates/desktop`: the private mark in the download list.
7. `apps/extension`: draw the option list, the unknown-kind fallback, the per-tab rows, the private session id, the capture steps with the pause before any wait, `ShellConfig` fetched on an action and kept for the session, passive paths (`media/watch.ts`, `media/center.ts`, `notices/focus.ts`) reading only the held copy with unknown as off for private tabs, the private gate on `page_options` and page actions, the hand-written option parser, the media watcher checking the tab before reading a `Cookie` header, cookie reads from the answer's sites. Remove `ui/settings.ts`, the per-setting validators and the version 1 paths.
8. Remove `tldts` and its release check, once step 7's cookie reads pass the tests from step 2.
9. Store: the listing on addons.mozilla.org, written around what runs in the browser, `PRIVACY.md`, `PUBLISHING.md`, the full source zip and its rebuild check, the `innerHTML` rule, the reviewer notes.
10. Tests: an older extension against an app with new kinds, fields and broken rows, and an older app against a newer extension. Every setting changed from the popup and from the app. Late replies against a newer revision, an app restart, and one unknown or malformed option among valid ones. Captures declined by the gates, by the app, and lost after the app took them. A private download leaving nothing behind and forgotten by every route.

## Assumptions

- No copy of the extension or app has been released, so redefining version 2 costs no user anything.
- The extension's existing controls cover the kinds above, so the popup keeps its current look.
- The media tables change more often than the way media is watched, so data is enough for them.
- A deadline exists that keeps the Save As prompt quick with the app connected and still lets a closed app start. Step 2 of the work measures it before anything else depends on it.
- The ways of reading cookies (partition site, first-party domain, the header a request carried) are stable enough to stay in the extension's code, with the app choosing the values.
