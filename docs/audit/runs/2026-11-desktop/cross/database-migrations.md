# Cross-module flow: database-migrations

Every step from an old database to the current one, a crash during a migration, opening a database made by a newer version, repair, and whether what core and records expect matches what each migration produces.

Result: the chain itself holds. Each of the 14 steps runs in a transaction together with its `user_version`, so a crash or a failing step leaves a whole earlier version that the next start carries on from. The partial `.bak-v` copy is removed and made again. JSON updates are guarded by `json_valid`, and the columns and JSON each step produces read back as what the store and core expect. The findings below are in what surrounds the chain: the kept copy of a damaged list, the failed-open window and the tests.

Pre-release context: D72.11 and D81.3 record that nobody runs a released build yet, so the database changes in place until the first release. Old settings and schedule shapes that no longer load are therefore not reported.

Already recorded in `contracts/03.md` and `contracts/04.md` as other chunks' findings, confirmed along this flow and not repeated: a cell that fails conversion fails the open, unreadable settings read as a first start, and a dropped queue becomes permanent because queues are written back at every start.

## Files read

Whole:

crates/store/src/lib.rs
crates/store/src/error.rs
crates/store/tests/samples.rs
crates/records/src/media/mod.rs
crates/records/src/batch.rs
crates/records/src/origin.rs
crates/records/src/proxy.rs
crates/records/src/lib.rs
crates/core/src/manager/restore.rs
crates/core/src/manager/startup.rs
crates/shell/src/host.rs
docs/audit/runs/2026-11-desktop/contracts/03.md

In part, for the steps this flow passes through:

crates/records/src/media/output.rs (serde attributes and fields)
crates/core/src/manager/mod.rs (`Manager::open`, `load_beside_the_list`)
crates/core/src/error.rs (`CoreError::Store`)
crates/core/tests/manager/restart.rs (the damaged list test)
crates/desktop/src/backend.rs (`serve`, `Backend::run`)
crates/desktop/src/library.rs (`load`)
crates/desktop/src/notices.rs (failed open, startup notices)
crates/desktop/src/main_window/notices_bar.rs (`tell_startup_notices`, `show_set_aside`)
crates/desktop/src/main_window/mod.rs (`MainWindow::new`)
crates/desktop/src/main_window/empty.rs (`unreachable_engine`)
crates/protocol/src/app.rs (`QueueSettings`, `AppSettings`)
crates/protocol/src/rules.rs (`QueueReason`)
crates/store/samples (media, last_ids, origin, settings)
docs/DECISIONS.md (D17, D72.11, D81)
docs/audit/runs/2026-11-desktop/contracts/04.md (Restore)

Not run: no build or test. Nothing below was confirmed by a test run.

## Findings

### Xdatabase-migrations-1 [P2] A damaged list last written over a month ago is deleted on the next start

- Where: `crates/store/src/lib.rs`, `Store::open_with`, `set_aside`, `remove_old_copies`
- Category: bug
- Failure: the app has not written its database for more than 720 hours (not opened for a month, say) and the file is found damaged at this start. It is set aside and the person is told "The damaged list is kept at ...". At the next start `remove_old_copies` deletes the copy and its side files. The person gets one session, not the month the code promises, and the only copy of the old list is gone.
- Evidence: `set_aside` uses `std::fs::rename`, which keeps the file's modified time, and the integrity check runs before anything writes to the file (`assert_set_aside` asserts the bytes are unchanged). `remove_old_copies` judges age only by `metadata().modified()`, and it runs before opening, so the first start does not delete the copy and the second does. The doc on `KEEP_COPIES_FOR` says "a month leaves time to recover a list by hand". `old_copies_of_the_list_are_removed` covers only pre-aged copies, never one set aside with an old time. The `.bak-v` copy is not affected: `VACUUM INTO` writes a new file.
- Confidence: certain

### Xdatabase-migrations-2 [P2] "Try again" can never succeed after the list failed to open, and the title names the wrong problem

- Where: `crates/desktop/src/main_window/empty.rs`, `unreachable_engine`; `crates/shell/src/host.rs`, `Core::opened`
- Category: broken feature
- Failure: the list cannot open because it was saved by a newer version, because a migration failed, or because the database was busy or the disk full at start. The window says "Wirecatch cannot reach its download engine" and offers "Try again". Pressing it calls `Library::load`, which gets the same stored error. The button does nothing even when the cause has gone, such as a backup tool letting go of the file or disk space being freed. Only quitting the app helps. The title also describes no real engine problem: the note beneath it says the download list could not be read, or needs a newer version, which retrying cannot fix.
- Evidence: `Core.cell` is a `OnceCell<Result<Manager, Unopened>>` that stores the error, and the doc on `Core::manager` says "every later call answers with it again". `Backend::run` goes through `core.opened()`. The `StoreError::Database` text itself says to "restart Wirecatch".
- Confidence: certain

### Xdatabase-migrations-3 [P3] The notice about a repaired list comes back when the library reloads

- Where: `crates/desktop/src/library.rs`, `Library::load`; `crates/desktop/src/main_window/notices_bar.rs`, `tell_startup_notices`
- Category: bug
- Failure: a list was repaired at this start and, later in the session, the library falls behind the 1024-event buffer. The reload shows "Wirecatch started a new download list" (or the other repair notices) again, as if it had just happened.
- Evidence: `RecvError::Lagged` calls `Self::load`, which reads `manager.startup_notices()` again, and those stay on the manager for the whole session. It sets `library.startup_notices`, and the observer in `MainWindow::new` then shows them. `tell_startup_notices` takes them from the library only, while its doc says "once". The same reload path runs when "Try again" is pressed.
- Confidence: certain

### Xdatabase-migrations-4 [P3] A roaming data folder that could not be deleted keeps the whole list with its cookies for good

- Where: `crates/shell/src/host.rs`, `merge`, `copy_then_delete`, `needs_move`
- Category: bug
- Failure: the data folder moves out of the roaming profile by copying, and deleting the old folder then fails (a sync tool or virus scanner holding a file). The old database, holding cookies and proxy passwords, stays in the roaming folder. Later starts never try again, so it stays there and can follow the profile to other computers, which D21.5 keeps this data local to prevent.
- Evidence: the failure is only logged, as "the old one is only clutter now". `needs_move` is false from then on because the new folder holds a list, and nothing else removes the old folder. The store limits copies of the list to a month for this same reason (`KEEP_COPIES_FOR`: both copies "hold every download's cookies"). A killed process between the last rename and the delete in `merge` leaves the same state.
- Confidence: likely

### Xdatabase-migrations-5 [P3] The migration tests seed today's settings, so they do not cover old stored settings

- Where: `crates/store/src/lib.rs`, tests `database_with_settings`, `the_window_before_each_download_is_switched_on_once`
- Category: fake test
- Failure: a change that removes `default_true` from `show_progress_windows`, or the `serde(default)` of any other setting, still passes these tests, although a settings row saved before that field existed would then fail to load and read as a first start. `assert!(settings.show_progress_windows, "a new setting takes its default")` cannot fail for the reason it states.
- Evidence: `database_with_settings` is documented as "Stored settings as a database at `version` holds them", but it writes `to_json(&sample_settings())`, which has every current field. `samples/settings/defaults.json` holds `"show_progress_windows": true`, so the assertion reads the stored value, not the default. `settings_saved_before_the_accent_existed_take_the_appearance_defaults` shows the working pattern, removing the fields first, but only for `theme` and `accent`.
- Confidence: certain

## Suggestions

### Xdatabase-migrations-S1 Read the version before the integrity check

- Today: `open_database` runs `check_integrity` before `migrate` reads `user_version`, so the integrity check comes before the `TooNew` check. SQLite reports a schema it cannot parse as `SQLITE_CORRUPT`, which `is_damage` treats as damage.
- Better: read `user_version` first and return `TooNew` before anything else. No current migration uses SQL the bundled SQLite cannot read, so this fails only once a later version does.
- Value: a person who goes back to an older build sees "Update Wirecatch" with their list in place, rather than an empty list and a healthy list set aside.

### Xdatabase-migrations-S2 Keep a real database file from each release as a test sample

- Today: `crates/store/samples` holds JSON values only, and migration tests build old databases by running the earlier `MIGRATIONS` entries. Editing a released entry therefore changes the test's starting point too, and the rule "Never edit a released entry" has no check behind it.
- Better: from the first release, keep one small database file per released schema version and test that each opens with its rows intact. This extends D81.3's samples to the schema.
- Value: an upgrade that would lose or hide someone's downloads fails a test before release.
