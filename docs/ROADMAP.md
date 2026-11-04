# Roadmap

## Phases

1. **Engine and CLI.** Segmented HTTP(S) downloads, dynamic splitting, resume, retries. Benchmarked locally. Done.
2. **Core, Bridge and Extension.** The download manager with SQLite state, the native messaging bridge, the WXT extension for Chromium and Firefox, settings sync (D5). Done.
3. **Desktop UI.** The desktop app hosting the core, first on Tauri 2 and now drawn in Rust on GPUI (D95, D107): download list, queues in the sidebar, schedules, speed limits, command palette, settings export and import (D6), tray, single instance. Done.
4. **Streams.** HLS and DASH detection in the extension and parallel segment download with joining (`wirecatch-streams`). Done.
5. **Media add-on.** One-click yt-dlp, ffmpeg and Deno, pinned by SHA-256, yt-dlp updates verified by signature and a two-day delay (D20, D21). Done; smoke-tested end to end against a real Wikimedia Commons video.
6. **Remaining v1 protocols.** FTP and FTPS done; HTTP/3 available as an experimental option (A7).
7. **Release readiness.** Done: final security review and fixes, Linux suite green in Docker, quiet-machine benchmark, Windows installer built (5.2 MiB), commit history cleaned. Left for the maintainer: GitHub organization, code signing, the updater signing key, store listings for the extension (which also gives Chromium browsers a trusted extension id), and the user actions in D21.
8. **Interface revamp.** One design system for the desktop app and the extension, in two phases. Phase 1 done: the foundation, the main window, the add-download flow, the download window and the extension's pages. Phase 2 done: the settings, the dialogs, the rail, the media button, the installer's pictures and Copy diagnostics.

## Where things stand and what comes next

As of 2026-10-31 the code quality audit has W0 to W5, W7, W8 and W14 done, most of W6, and W9's findings that could break a download (CORE-4 to CORE-7, CORE-9, CORE-10, CORE-12). The GPUI desktop app is on main (D95, D107), and so are queues that decide how their downloads run (D72), proxy profiles (D73), queue folders (D94) and the rebuilt download list (D115). Items 1 to 7 under "Before the first release" are done; items 8 to 12, the audit's remaining code fixes, come next, then the release setup that is the maintainer's; this list is the order after it, which the maintainer may change.

## After the first release

1. The rest of W6: shared test support, CI, the backlog of proposed gate checks listed under W6, and a way to test the GPUI app's views (blocked until gpui-kit's test support resolves alongside gpui-pre).
2. W15 last: comments only where they earn their place.

## Before the first release

Set with the maintainer on 2026-10-30, and extended on 2026-10-31 with items 8 to 12, the audit's remaining code fixes. The release waits for the items below, in this order (reordered the same day so nothing is built twice for D95), and is delayed for them if need be; W6 and W15 do not (see "After the first release"). A bug found on the way is fixed whatever part of the app it is in.

1. **Notices that come back** (bug; done, D97). Notices the person closed show again a little later, after switching tabs or after a minute or so, and a notice from minutes ago can show a second time, such as a failure for a download that is no longer in the list. The cause is found and named first, with a test that fails; a closed notice never comes back, a notice about a download that is gone is not shown, and Clear all is always there while several notices show.
2. **Every download follows "If the file is already in the folder"** (feature; done, D96). The setting (Ask me, Replace the file, Keep the existing file, Keep both, with the question's "Remember my choice") covers every kind of download, however it arrives: a file added in the app, from the command line or captured from the browser (done, D88), and now also a stream (HLS or DASH) and a video or audio download read by the media add-on, which today always take the next free name. Those are checked when their job ends and their file is placed, since that is when the name is final (D88.5). The setting decides whenever a file of that name is in the folder, whatever the file is, not only for a likely duplicate (D96). The question says which it is: the same file again, or a different file with the same name.
3. **A notice opens the app at its download** (feature; done, D98; the notice already offers Open and Show in folder, D90). The main window's part is carried into the GPUI interface. Clicking a notice's text, in the browser or the system's notification, brings the main window up showing the queue the download is in, with the download selected, scrolled into view and highlighted for a moment, so the person sees at once where it is. The buttons keep doing what they say: Open opens the file, Show in folder shows it in its folder.
4. **Save to says who chose the folder** (change to D94.4; done, D94.6). The window's part is carried into the GPUI interface. A download window shows the folder the app chose, from the queue's own folder or else the folder for the file type, marked as the app's choice ("Chosen by the Overnight queue", "By file type"). Picking another folder makes it the person's choice, marked as such, with a way back to the app's; picking the folder the app already chose leaves it the app's choice. The person's choice wins over the queue's, as D94.2 says.
5. **The desktop interface in GPUI** (rebuild; done, D95, D106, D107: every screen, the installers, and the Tauri app removed). The desktop app's interface is rebuilt in Rust on GPUI with gpui-component, calling the core directly, and Tauri and the webview go. The look follows Zed's rather than copying today's screen for screen, keeping every screen, state and behaviour the decisions describe. It comes after the items whose work is mostly in the core, and before the About page and the updates, which are built for it rather than twice; the browser extension keeps its web interface and `packages/ui`.
6. **An About page** (feature; done, D109). Settings gets an About page with the app's name and version, its licence, the maintainer, the repository, the website and where to report a problem, all read from `config/product.toml`, plus the versions of the media add-on's programs when installed.
7. **Updates from GitHub Releases** (feature; done, D111; the maintainer makes the key). The app checks GitHub Releases for a newer version and offers to install it. A setting, "Get pre-release versions", off by default, also offers pre-releases; off, only full releases are offered. The release workflow builds every installer for Windows and Linux, on x64 and ARM64, names each file by one scheme (product, version, system, architecture), writes the update manifests for both channels, and drafts the GitHub release with all of it for the maintainer to publish. Needs the updater signing key, which is free to make and the maintainer's to keep (D19); code signing stays out of the first release.
8. **Fixes in the GPUI app** (done; AUDIT, "After the move to GPUI"). The help of the hosts that skip the proxy says in plain words what `<local>` covers, and the bridge takes `CREATE_NO_WINDOW` from `windows-sys` (CMD-12). The program warning never opened on Enter in the GPUI app (D43.1).
9. **W11, the desktop shell** (done). The seven findings that still apply to `crates/shell` and `crates/desktop`: DRS-10, DRS-12, DRS-16, DRS-17, DRS-20, DRS-22 and DRS-23.
10. **W10, IPC, bridge and CLI** (done). IPB-6, IPB-7, IPB-17, IPB-18, IPB-19 and CORE-21.
11. **W13, the extension** (done, D112). EXT-4, EXT-5, EXT-10, EXT-11, EXT-12, EXT-13 and EXT-16, and D44.6: the extension takes the cookie scope from the app's Public Suffix List, which the app keeps current (D44), instead of the copy bundled with each extension release. It changes what the app and the extension send each other, which after the release protocol version 1 freezes (D32), and a stale list can send a site's cookies with another site's download.
12. **The rest of W9, the core** (done). CORE-8, CORE-11, CORE-13, CORE-14, CORE-19, CORE-23, CORE-25, CORE-26 and the small items, with CORE-23 and CORE-8, which change what a download does, last.

Open, found on 2026-11-04. Whether the release waits for them is not decided.

To fix:

- **A failed video shows the add-on's raw output.** The message from `crates/media/src/describe.rs` quotes yt-dlp, such as `[generic] 1: Unable to download webpage: HTTP Error 404`, in the list and the download window. D83 asks for wording an established app would use.
- **The wrong-size update message is awkward.** "the installer is not the size its list of files gives", in `crates/core/src/manager/app_update.rs`.
- **The Download folder field may read as a placeholder.** Saving files shows a dim "Downloads" where other screens show the full path. Not yet checked whether this is intended.
- **Empty space in a short download window.** The "already answered" note leaves a gap under Close, because the window keeps its minimum height.
- **On Linux, yt-dlp and ffmpeg keep running after the app crashes.** Only a live app kills their process group (`crates/streams/src/process/tree.rs`). `PR_SET_PDEATHSIG` does not solve it: it fires when the thread that started the program exits, and the runtime threads come and go. It needs another mechanism, such as a small supervisor that kills the group when the pipe to the app closes, with a test that kills the app and finds both programs gone.

Not tested yet:

- **A real update.** A successful download, install and restart has never run; the banner was only seen up to a failed checksum, with its stage seeded by a temporary change. It needs a signed release to test against.
- **Real installers.** None has been built with the third-party notices (D117), so the first run of the release workflow is the first test of its Linux steps.
- **Certificates on Linux (D116).** The change has run only in CI, not on a Linux computer.
- **A video that fails before its window first reads the question.** The logic has a test; the timing was not reproduced.
- **Light theme.** Of the screens changed on 2026-11-04, only the update banner, the list and the failed video note were seen in light theme.

Audit fixes, in the order to fix them. Findings, proofs and fix directions are in `docs/audit/runs/2026-11-desktop/AUDIT-LOG.md`.

To work through them, follow `docs/audit/runs/2026-11-desktop/WORK-LIST.md`: what to take next, on which branch, as which pull request.

1. Two downloads can end up writing the same file (fixed)
2. Removing a finished download can delete a whole folder (fixed)
3. A cut-off download of unknown size is reported as complete (fixed)
4. A restart can mark a download finished on a file it never downloaded (open)
5. A browser download that meets Ask is dropped and goes back to the browser (open)
6. An inherited APPIMAGE variable can make the app overwrite or start another program (fixed)
7. The app never looks for updates, and every proxy test fails (fixed)
8. A move across drives can leave a cut-off file under the final name (fixed)
9. Quitting during a move leaves the list pointing at the wrong place (open)
10. The quit time limit does not end the process (open)
11. "If the file is already in the folder" is skipped for names from the link or browser (open)
12. A settings file can send downloads into folders whose programs the app runs (open)
13. Replace on a file with another extension downloads everything again and strands the old folder (open)
14. A new media download can save another download's video (open)
15. Typing a space in search pauses downloads, and Enter can open a file (open)
16. A very large HLS playlist freezes inspection for minutes to hours (fixed)
17. A video with a web page or nothing in place of a segment is reported as complete (fixed)
18. A lower-integrity program can stand in for the app and receive downloads' cookies (open)
19. A DASH template can ask for an address of hundreds of gigabytes and abort the app (fixed)
20. A failed flush while saving progress is forgotten (open)
21. An expired redirect target fails a download whose link still works (fixed)
22. One unreadable value in the list stops the app opening at every start (open)
23. Sources are offered that ffmpeg then refuses after the whole download (open)
24. A video format that needs cookies always fails when the engine fetches it (fixed)
25. The could-not-save banner is wrong both ways, and its Try again saves nothing (open)
26. Settings, queues and proxies that cannot be read are silently replaced (open)
27. A removal under way at quit comes back, and a capture is downloaded twice (open)
28. Nothing knows whether a download can continue, so messages promise what will not happen (open)
29. The Public Suffix List's date and Check now were never put in Settings (open)
30. The window redoes whole-list work on every frame and progress tick (open)
31. The Add dialog's link rule leaves out ftpes, so such links cannot be added (open)
32. A raw non-ASCII byte or another charset in a header loses the file name or fails a redirect (open)
33. Failing to delete a temporary file turns a saved video into a failure (open)
34. A damaged or squatted key does not lead the bridge to start the app and renew it (open)
35. An open file-exists question outlives the download state it asked about (open)
36. Proxy exceptions are accepted that can never match (open)
37. A typed file name is dropped when the download turns out to be a page (open)
38. Pages yt-dlp describes at the top level, or with several videos, are misread (open)
39. The CLI strands files under another name when a resume file is damaged or the name is numbered (open)
40. The add-on's state mixes an operation with whether the tools work (open)
41. A download retried or removed just after it ended is still told as ended (open)
42. The network guard's local-address rule is written twice (open)
43. The local network rule does not apply to FTP (open)
44. "This host is under that site" is written four times, and the copies differ (open)
45. ftps and ftpes hosts are read raw by every check (open)
46. An apostrophe in a link leaves its password or signed query in the log (open)
47. A link's password reaches the browser extension, and a crash's partial file is not picked up (open)
48. A finished file with a leftover resume file is downloaded again (open)
49. A title holding % or $ sends yt-dlp's file to another folder or stops it (open)
50. Path parameters such as ;type=i or ;jsessionid end up in the file name (open)
51. Add-on installs and app updates get advice about links and folders they do not have (open)
52. A damaged list set aside can be deleted at the next start (open)
53. The quality form loses the person's choice or says nothing around an add-on install (open)
54. The file-exists question never says whether it is the same file (open)
55. Cancel in a download window discards the partial file with no question (open)
56. A pasted link is cut at an apostrophe, a space or a bracket, so another address is downloaded (open)
57. Choosing a file or folder without a file chooser portal does nothing and says nothing (open)
58. A typed file name with a dot loses the words after it (open)
59. Downloaded installers are never removed (open)
60. An update with no usable file is shown as a package manager update or opens the release page (open)
61. A conversion demands audio or video that a format did not describe (open)
62. A torn last line of a stream's log can truncate its joined track (open)
63. Blank lines hide a program's failure reason and grow memory without limit (open)
64. No test covers control characters in names or a part file that cannot be created (open)
65. Ctrl+C in the CLI can exit with the wrong code or abort (fixed)
66. Keep existing, Replace and captures leave the wrong category on the row (open)
67. A finished media file is not flushed before its sources are deleted (open)
68. Leftover proxy code from the earlier form has no caller (open)
69. yt-dlp ignores proxy exceptions and the page's scheme (open)
70. Choosing a path blocks an async thread on the disk, so a dead share stalls every command (open)
71. A passing read error or a failed rename deletes a download's data (open)
72. The app quits before the Windows setup has asked for approval, so a refusal reaches no one (open)
73. An AppImage restart can be lost while the old copy exits (open)
74. A stored bad part can never be thrown away, so Refresh link cannot help (open)
75. A refused HLS or yt-dlp address fails instead of reading the page again (open)
76. The import preview hides proxy and local site changes (open)
77. A missing library is not seen as a broken add-on, and can raise a system dialog (open)
78. A download moved to another queue shows in the wrong place (open)
79. The window and a restart disagree with what a queue's run really is (open)
80. Answering the file-exists question opens a second window for the same download (open)
81. Download windows offer only Try again, whatever fix the failure needs (open)
82. Refreshing the link cannot clear the check against what the browser saw (open)
83. An answered Replace and a capture's size check are lost at a restart (open)
84. A write that can never pass raises a notification for every batch (open)
85. Choosing the folder the guess already points at is not remembered (open)
86. A capture joining an unfinished download is answered Started too early (open)
87. Skipped bytes are not counted against the speed limit (open)
88. A stale resume file that could not be deleted makes zeros count as downloaded (open)
89. A resume file is rewritten every second even when nothing changed (open)
90. A valid short partial answer is waited out as a failed connection (open)
91. CLI speed and ETA are wrong for half a minute after a resume (fixed)
92. A command-line argument that is not valid Unicode aborts the app (fixed)
93. Uninstalling while the app runs leaves its program behind (open)
94. The AppImage's library folder leaks into programs the app starts (open)
95. A video's title is cut at its last slash (open)
96. yt-dlp progress goes backwards for a video with separate audio (open)
97. A quality says 60 fps but may download 30 fps (open)
98. Speed limits do not reach yt-dlp (open)
99. The best audio is picked by rate alone, so a dubbed track can replace the original (open)
100. HLS key addresses count against no memory budget (open)
101. Automatic audio picks another language when the default is inside the video (open)
102. A subtitle codec turns an MP4-ready video into MKV (open)
103. While the list loads, the window says nothing was ever downloaded (open)
104. Moving a file to another drive drops its Mark of the Web (fixed)
105. Replacing a file another program has open blames the folder (open)
106. A stream manifest over the limit offers a retry that cannot help (open)
107. A suffix list that could not be saved blames the internet connection (open)
108. An add-on repair downloads everything and puts yt-dlp back to the pinned version (open)
109. The add-on check acts on what it read before its wait, so a removed add-on comes back (open)
110. One bad yt-dlp release stops the check, and a failed check forgets its offers (open)
111. Cancel on a selection stops at a finished download (open)
112. The drawer stays open over a page opened from its foot (open)
113. Change queue inside the details opens the same details again (open)
114. A page address that is not a web address counts as on the person's network (open)
115. A Windows socks= proxy fails on sites whose first address is IPv6 (open)
116. The proxy form says a saved password is kept, then forgets it (open)
117. A download that fails to start leaves its slot empty (open)
118. A closing queue's slots are given away at once, exceeding the limit (open)
119. Regular expressions differing only in case count as one rule (open)
120. Enter in the delete-queue field closes the question and deletes nothing (open)
121. Saving a queue changes its speed limit (open)
122. Confirming a file unedited stops the server from naming it (open)
123. A failure notice in the browser loses its buttons after 32 later notices (open)
124. A tray action that fails tells the person nothing (open)
125. An unfinished download reads 100% (open)
126. A finished download whose file is gone gets advice it cannot follow (open)
127. Settings, Media shows an empty Resolution after a rarer quality (open)
128. The Add dialog says By file type for downloads going to a queue's folder (open)
129. scripts/linux-test.sh can never pass (open)
130. A second running copy of one CLI download takes over the first one's files (fixed)
131. A launch handed over while the list is opening pops a box and loses links (fixed)
132. A typed extension picks a container ffmpeg refuses, with no way out (open)
133. A stream whose placing fails is downloaded again (open)
134. A saved format the page no longer lists can never succeed (open)
135. Try again can never succeed after the list failed to open (open)
136. Dragging moves rows the selection no longer shows (open)
137. Reading a website's page with yt-dlp escapes the local network rule (open)
138. A queue's run ends while its last download only waits (open)
139. Nothing opens a download's window again after Hide (open)
140. A file too large for the disk is not noticed until it is full (open)
141. A crash at the end of a move leaves the list and the disk disagreeing (open)
142. Commands and items left from the removed web interface are dead (open)
143. Tests give waits a fixed short limit, and the timing check cannot see them (open)
144. Tests race the machine against a real-clock limit of the code under test (open)
145. A page's HLS part runs outside the run's file work (open)
146. Bridge and test values are written separately in several crates (open)
147. Test servers repeat their range parsing and behave unlike their docs (open)
148. An import's summary event and the every-video answer have no reader (open)
149. Tests repeat their fixtures and data generators (open)
150. A reload after missed events repeats notices and leaves open windows stale (open)
151. A capture's check and its record sit in separate lock sections (open)
152. A found duplicate overrides a stop the person chose in the same moment (fixed)
153. A link or folder in place of the key gets a raw system error (open)
154. Cancelling does not interrupt name lookups (open)
155. The rebinding tests never reach the filtering resolver, and its refusal ends as a network failure (open)
156. Tests do not cover the stored forms of enums and old settings (open)
157. A held-open output loses a program's collected output, and its test cannot fail (open)
158. A resume file's temporary file can stay for good (open)
159. Add-on release files of skipped versions stay in the work folder (open)
160. Tray ticks flip on a repeat click, and & in a name shows wrongly (open)
161. Messages name the product with a literal (open)
162. A navigation item left behind keeps its hover colour until the mouse moves (open)
163. Empty pages offer Pause all or Resume all (open)
164. Listed above as "A failed video shows the add-on's raw output" (open)
165. New queue and New proxy do not put the cursor in the Name field (open)
166. The row, the menu and the details decide offers and words on their own (open)
167. Two test files depend on an order only timing decides (open)
168. The same small pieces of logic are written many times in the forms (open)
169. Dying between placing a video and deleting its work folder gives a second copy (open)
170. Exporting settings over an earlier file is not written whole (open)
171. The bridge's manifest write has no flush (open)
172. Pause all while a cancel is stopping keeps the download (fixed)
173. The same link with another #fragment opens a second question (open)
174. A file beside a leftover part or resume file is never asked about (open)
175. A saved download that gives way to its page loses its link if the app stops first (open)
176. A removal gives up the path before it deletes the files (open)
177. A failed write takes too much back from the progress count (open)
178. A range past the end of a shrunken file is reported as a server error (open)
179. The step-back rule's long-wait test is true only because of jitter (open)
180. Fetcher::save promises data survives a drop, but up to 256 KiB is lost (open)
181. bench_local sleeps for no time after each accept (open)
182. A redirect error tells the CLI user to allow something the CLI cannot (fixed)
183. async_and_blocking_reads_agree never does a blocking read (open)
184. The AppImage replacement is renamed into place without a flush (open)
185. The unsigned release page address reaches the system shell unchecked (open)
186. Turning off pre-release versions keeps offering the one already found (open)
187. No test holds a manifest's version and size rules (open)
188. The packaging example for ARM64 on x64 cannot work (open)
189. ARM64 Linux packages are drafted without the fresh-system check (open)
190. The .deb and .rpm state no C library version (open)
191. The icon holds its own unchecked copy of the mark (open)
192. Two copies of the read-until-full loop have drifted (open)
193. Code keeps work folders of a layout that was never released (open)
194. axum is a dev-dependency nothing in streams uses (open)
195. The path-escape half of a media test cannot fail (open)
196. Doc comments on media runs sit on the wrong items (open)
197. Sizes and bit rates a page chooses are added without a check (open)
198. The key error prints the whole signed address (open)
199. On Linux, media programs may outlive an app ended by a signal (open)
200. Placing a finished video has its own weaker no-replace move (open)
201. Moving a finished video leaves its subtitle files behind (open)
202. A failed browser registration logs no reason (open)
203. The autostart Exec line escapes reserved characters once (open)
204. The autostart Launcher trait has one implementation and no test (open)
205. Every window has an empty native title (open)
206. invalid_link ignores its argument, and its doc and test claim otherwise (open)
207. An import can leave two queues with the same name (open)
208. An update offer names a yt-dlp older than the one installed (open)
209. swap_in spells out the release file's name (open)
210. Ticking the first row pushes the list down under the pointer (open)
211. The empty queue's Add button opens the dialog past the window's record (open)
212. `let _theme = cx.theme()` does nothing (open)
213. The link's hover popover covers the row's menu (open)
214. The damaged-list notice squeezes its text and breaks the path (open)
215. A top-level domain held only through a wildcard counts as a local name (open)
216. direct_uses_no_proxy passes whatever Direct does (open)
217. Module docs describe proxy behaviour that has changed (open)
218. A proxy test cancelled by quitting is saved as its last result (open)
219. A test named for keeping the last result does not check it (open)
220. Copy from system turns a SOCKS4 server into a SOCKS5 profile (open)
221. An impossible port is saved as the usual port without a word (open)
222. A file name or host holding a secret word loses the rest of its log line (open)
223. The rules help says the highest queue wins, but for sites the most specific does (open)
224. A dotted file type is accepted in a category rule and never matches (open)
225. Queue page numbers start at 2 and number failed rows (open)
226. A queue using a proxy with no name reads "via" and nothing after it (open)
227. The queue page's facts line is cut off with no way to read the end (open)
228. The details panel says Merging for every processing step (open)
229. The system accent breaks the promised contrast (open)
230. Check for updates and Copy from system give no answer when there is nothing to do (open)
231. A download waiting on the question reads Paused at Size unknown (open)
232. The complete window stays open and blank once its rows are removed (open)
233. The notice of a failed save ignores Do Not Disturb (open)
234. The language list names Twi "Akan" and gives Ossetic itself as its other name (open)
235. The comment on RecvError::Closed in the server's loop is wrong (open)
236. Manager::update_settings has no caller outside tests (open)
237. lock_path repeats with_suffix (open)
238. Product tests are pinned to today's config values (open)
239. Two copies of the pnpm-workspace parser (open)
240. Dead code in the product generator (open)
241. Comments in the checked configuration describe things that are gone (open)
242. Two test helpers wait for progress with no deadline (open)
243. A roaming data folder that could not be deleted keeps the list with its cookies (fixed)
244. The `<local>` queue rule is never tested against the real network check (open)

Done before the release:

- **Queues save to their own folder** (feature; done, D94).
- **What a download does when its file already exists** (feature; done for files, D88; videos are item 4 above).
- **One download window per link** (bug; done, D89). Clicking the same link again while its download window is open brings that window to the front instead of opening a second one.
- **The in-page media button cannot start a video twice** (bug; done, D89). Clicking the button again for a video that is being read or whose window is open brings that window to the front; the button stays as it is. Today repeated clicks start several reads, and none gets past reading.
- **Toasts show above an open dialog** (bug; done). Dialogs open in the browser's top layer, so a toast raised while one is open (a proxy test that failed, say) sits under its blurred backdrop, where nobody can read it. The toasts move into the topmost open dialog while there is one, so they show above it, can be clicked and are announced.
- **Finished downloads are told in the browser** (bug; settled by D90: a system notification is the default, since the browser is rarely in front when a download finishes). With "Show a notice in the browser" chosen, a Windows notification shows instead even with the browser in front: the app waits one second (`NOTICE_ANSWER_WAIT_MS`) for the extension to confirm, and a first notice in a tab must wake the background, place the frame, load the notices page and answer through the bridge. Measured first, then fixed where the time goes. A browser not in front, or a tab no extension can draw in, still falls back by design (D65.5).
- **The browser notice offers Open** (amends D65.4; done, D90). Pressing it opens a document, picture, video or audio file directly; a program, a script or an unknown type opens only after the app's own confirmation window, which no website can cover, so a click a website tricked never runs a program. The setting's description stops promising "the same buttons" until it is true.
- **Settings and downloads switch at the sidebar's foot** (change to D47; done, D91). On the settings screen the foot of the sidebar shows "Downloads" where the downloads screen shows "Settings", and the "Back to downloads" item at the top goes; Escape leaves settings when its search is empty.
- **Where a video's subtitles go** (settings change; done, D93). "Save subtitles as separate files" becomes a choice: embed them in the video, or save them as separate files. With embedding chosen, a second choice says what happens when the format cannot hold subtitles (the "Original" format, or audio only): save them as separate files (today's behaviour) or leave them out. The quality question follows the same rule. The Media page is split into a Video group and an Audio group, so the labels drop "for videos" and "for audio" ("Subtitle languages"); "Save audio as" heads the Audio group and says that it converts.
- **The proxy list's buttons sit in its header** (layout; done). "Copy from system" and "New proxy" move from below the list to the "Proxy servers" header row, so they stay in one place however many servers there are, and an empty list reads as one line saying there are none yet.
- **The queue list's layout** (layout; done). "New queue" moves to the "Your queues" header row, as the proxy list's buttons do; "Test a link" stops being a panel inside the card and becomes its own row with a full-width field, a Try button as tall as the field, and its answer below; the queue and proxy lists share one list component, so their rows, buttons and empty state look and behave alike.
- **A modern title bar** (design; done, D92). New window buttons in place of the "wire" glyphs: small rounded buttons with plain Windows 11 shapes and no background at rest, the one under the pointer filled (grey, red for Close), as variant D of the title bar mockup; the search box keeps its shape; every part of the bar that is not a control drags the window and a double-click on it maximizes, since today the wrappers of the search box and of the button groups refuse a drag across a wide band; the search box shows no keyboard shortcuts; and the speed readout is hidden while nothing downloads instead of saying "Idle", without moving the rest of the bar when it comes and goes.
- **CORE-9 (done):** moving a download to another folder never replaces a file there, and a move that cannot delete the original leaves no duplicate behind; core uses the engine's no-replace move.
- **CORE-4 (done):** withdrawing a browser capture while it is being confirmed never downloads it twice.
- **Dropped downloads' file work (done):** a stopped run's queued file work cannot delete a new run's files (the drain handle).
- **One state for a download (CORE-12), one kind of question (rest of CORE-5) and cancel safety (CORE-6) (done):** the flags become one phase, which removes the class of bug behind 13 and 14.
- **Saves and scans (done):** progress saves write only the progress (CORE-7), state changes stop scanning every download (CORE-10), and the system proxy is read off the manager's lock.

Release setup, beside the list, for later: the four `TODO-PRODUCT-` values in `config/product.toml` (the maintainer's), a Chrome Web Store package from the release workflow, and the store listings (ROADMAP item 7).

For the maintainer:

- Left for release, as before: the GitHub organization, code signing, the updater signing key and the extension store listings.

## Phase 1 acceptance checklist

Each item is observable from the CLI or the test suite.

1. `wirecatch <url>` downloads a file using several parallel connections, and the result is byte-identical to the source.
2. When a server does not support byte ranges, the download falls back to one connection and still completes correctly.
3. When a connection finishes its part, it takes over half of the largest remaining part (dynamic splitting), so no single slow tail holds up the end.
4. Interrupting a download (Ctrl+C or a killed process) and running the same command again resumes from where it stopped instead of starting over.
5. If the file changed on the server between runs (different size, ETag or Last-Modified), the download restarts cleanly instead of producing a corrupt file.
6. A dropped connection or a server error (5xx, 429, timeout) is retried with backoff, and the download still completes.
7. While downloading, data goes to `<name>.wcpart` next to a `<name>.wcstate` resume file; on success it is renamed to `<name>` and the state file is removed.
8. The file name comes from `Content-Disposition`, then the URL, and is made safe for Windows and Linux. An existing file is never overwritten; a free name like `file (1).zip` is used instead.
9. A local benchmark shows the speedup of several connections over one against a server that limits speed per connection.
10. The disk space for the file is reserved up front without writing it twice (sparse file on NTFS, `fallocate` on Linux).
