# Privacy

Wirecatch is a download manager: an app on your computer and a browser extension that hands it the downloads you start. Neither has an account, a server of its own, analytics or telemetry. This page says what each one reads and where it goes.

## The browser extension

The extension reads only what it needs to hand a download to the Wirecatch app:

- **Downloads you start**, the links you choose "Download with Wirecatch" on, and **video and audio** a page plays: their addresses, the address of the page they come from, and the file name and size the browser knows.
- **The cookies** your browser would send with that download or that media, and the `Cookie` header it did send with the media, so Wirecatch can download what needs your sign-in.
- **The settings** the Wirecatch app sends it.

All of it goes to one place: the Wirecatch app on the same computer, through the browser's native messaging. The extension sends nothing to any server. It treats private windows like any other, if the browser lets it run there; turn off "Work in private windows" to leave them to the browser.

While the browser is open, the extension keeps some of this in the browser's session storage, so it can pick up where it was after the browser pauses it: the settings the app sent, the downloads being handed over, each tab's list of media with the `Cookie` header sent for it, its messages to you, and its own error log, which leaves out cookies, passwords and tokens. The browser clears all of it when it closes, and web pages cannot read it.

## The app

The app downloads what you asked for from the sites it comes from, with the cookies and the referring page the extension handed it, as your browser would.

- **Your downloads** (addresses, file names, folders, and the cookies a download needs to resume) are kept in the app's database in your user folder, readable only by your account. A download that has not finished within a week forgets its cookies, and removing a download removes them.
- **Logs** stay on your computer and leave out cookies, passwords and tokens.
- **Settings** stay on your computer, unless you export them to a file.

The app connects to other servers only for these:

- The sites your downloads come from.
- Once a day, Wirecatch's own update address, to ask whether a newer version exists.
- The media add-on, if you install it: its programs are downloaded, and updated unless you turn that off, from their publishers' release pages, and it reads the video pages you give it.
- Once a week, the list of public domain names from publicsuffix.org, which you can turn off in Settings, Advanced.

None of these requests says anything about your downloads beyond the download itself.

## Questions

Wirecatch is open source, so everything above can be checked in its code. For questions, use the contact in [SECURITY.md](SECURITY.md).
