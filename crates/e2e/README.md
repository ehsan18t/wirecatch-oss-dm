# wirecatch-e2e

End-to-end tests for Wirecatch. A scripted fake extension starts the real `wirecatch-bridge` executable the way Chrome or Firefox does (the caller on the command line, length-prefixed JSON on piped stdin and stdout). The bridge connects over a real `wirecatch-ipc` endpoint to a real `wirecatch_core::Manager`, which downloads from a local HTTP server. Nothing reaches the internet.

## Running

```bash
cargo test -p wirecatch-e2e
```

The tests need a bridge built with its `test-overrides` feature, which lets them point it at a private endpoint (`WIRECATCH_BRIDGE_TEST_ENDPOINT`) and a stand-in app (`WIRECATCH_BRIDGE_TEST_LAUNCH`). Cargo cannot build another package's binary for a test on stable, so the first test that needs the bridge runs this itself, in the same target folder:

```bash
cargo build -p wirecatch-bridge --features test-overrides
```

When nothing changed this takes well under a second. To use a bridge you built yourself instead, set `WIRECATCH_E2E_BRIDGE` to its path. The workspace test command in `CLAUDE.md` (with `--features wirecatch-bridge/test-overrides`) works too. Never ship a bridge built with that feature.

Two tests are ignored until the protocol server handles each request in its own task (see the `#[ignore]` reasons). Run them with:

```bash
cargo test -p wirecatch-e2e -- --include-ignored
```

## Layout

- `src/bridge.rs`: finds or builds the bridge.
- `src/app.rs`: a temporary folder and private endpoint per test, the app (manager plus listener, every connection in its own task), and `run`, which gives each test its own runtime and deletes its folder afterwards.
- `src/extension.rs`: the fake extension (`hello`, `add_download`, `withdraw_download`, `get_extension_settings`, `update_extension_settings`, `list_queues`, `pause_all`, `resume_all`, events).
- `src/server.rs`: the HTTP server; a held server takes requests and answers none until released.
- `src/bin/wirecatch-e2e-app.rs`: stands in for `wirecatch-app --background` when the bridge has to start the app.
- `tests/e2e.rs`: the scenarios.
