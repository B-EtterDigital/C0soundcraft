# Patch debt: lines of ours inside upstream files

Only allowed to go down. Each line names the reason and whether it was offered upstream.

| File | Lines | What | Why | Offered upstream |
|---|---|---|---|---|
| `apps/soundcraft-web/Cargo.toml` | 9-15 (7) | `[features] page-control` (with its comment) and an optional native `serde_json` | Turns on the page control bridge; `serde_json` lets the bridge's request rules be tested natively | Not yet (generic hook) |
| `apps/soundcraft-web/src/main.rs` | 24-26 (3) | `#[cfg(feature = "page-control")] mod page_control;` and a blank line | Declares our `page_control.rs` for the web build and the crate's native tests | Not yet |
| `apps/soundcraft-web/src/web.rs` | 17-18, 35-38, 49-50 (8, 1 replaced) | `page_control::install_deliver(download)` before upstream's `io::set_deliver(download)` (the hook is a `OnceLock`, the first set wins); the runner gets a name (`let runner = eframe::WebRunner::new();`, `let result = runner`) and is handed to `page_control::runner(&runner)`; `let app = page_control::attach(app, &cc.egui_ctx)`; all behind the feature | Gives the page the app's own control channel (`with_control`), the runner it needs to answer a request in a pass of its own, and a delivery hook that drops downloads only while a `c0.*` request runs | Not yet |
| `crates/engine/catalog/menus.txt` | 505 (1 replaced) | `Window > Pro Tools Search` is `Window > Search` | "Pro Tools" is an Avid trademark | Not yet |
| `crates/ui-egui/src/menus.rs` | 60, 241-242 (3 replaced) | the same menu path in `UI_COMMANDS`; the app menu's separator and "Quit" drawn only when `ui.quit_menu` | Trademark; the host owns quitting | Not yet |
| `crates/ui-egui/src/control.rs` | 446 (1) | one arm at the top of `handle`: a method starting with `c0.` goes to `c0_host::handle` | Every host rule lives in our `c0_host.rs` and `c0_host/`, and a native test runs them through the real channel | Not yet |
| `crates/ui-egui/src/edit_window.rs` | 109-111 (3 replaced) | the Edit toolbar, status bar and lower dock tab strip each drawn only when their `UiState` switch is on (`app.ui.edit_toolbar.then(...)`, same panel calls) | The host draws its own chrome; upstream had no switch for these three | Not yet (same pattern as upstream's `show_*` switches) |
| `crates/ui-egui/src/lib.rs` | 9, 131-136, 197-201 (12) | `pub mod c0_host;`; `UiState.edit_toolbar`, `status_bar`, `dock_tabs`, `community_links`, `quit_menu` (with one comment line) under the struct's existing `#[serde(default)]`, and their `true` defaults | The chrome switches, settable through upstream's `ui.set`; all on is upstream's look | Not yet |
| `crates/ui-egui/src/panels.rs` | 711-712 (2 replaced) | the About window's getartcraft.com and Discord links drawn only when `ui.community_links` (the name, authors and licence stay) | An embedding app credits SoundCraft in its own About | Not yet |
| `crates/ui-egui/src/theme.rs` | 6-7, 389 (2, 1 replaced) | `pub mod host;` declares our `theme/host.rs`; `Tokens::current()` returns `host::resolve(is_light())` | The host sets colours at run time; `current()` is the one place every frame reads its tokens. With no override set, `resolve` returns the same preset as before | Not yet (a generic run-time token override) |

Total: 43 added lines (11 of them replacing upstream lines) in 10 files.
Ratchet: 43 (added column of `git diff --numstat --diff-filter=M main c0/main -- . ':!c0'`, `main` = upstream 0a95b6d; measured 2026-10-26).

New files of ours (no debt): `apps/soundcraft-web/src/page_control.rs`, `apps/soundcraft-web/src/page_control/bridge.rs`,
`crates/ui-egui/src/c0_host.rs`, `crates/ui-egui/src/c0_host/{ai,b64,files,inspect,panels}.rs`,
`crates/ui-egui/src/theme/host.rs`, `crates/ui-egui/tests/c0_host_contract.rs`, `crates/ui-egui/tests/c0_host_theme.rs`,
`c0/soundcraft-host/`.

## Couplings without an edited line (re-check on every pull)

| Ours | Leans on | Check |
|---|---|---|
| `theme/host.rs` `token_table!` | the field list of `Tokens` (80 colours, 2 pairs) | `c0_host_theme.rs` `the_host_can_set_every_token_upstream_declares` counts the fields in `theme.rs` |
| `c0_host/files.rs` | `dialogs::open_session_bytes`, `Dialog::SaveChanges { then }` with `ui.open_pending_session`, `Dialog::PathPrompt`, `io::session_bundle`, `io::import_audio_bytes` / `import_midi_bytes` | `document_bytes_round_trips_*`, `open_file_*`, `cancel_file_dialog_*` |
| `c0_host/ai.rs` | `soundcraft_mix::render_range`, `soundcraft_audio_io::encode`, `Engine::push_undo`, `Session.edit` selection fields | `selection_renders_*`, `place_*` |
| `c0_host/inspect.rs` `go_to` | upstream commands `transport.locate` and `track.scroll_to` | `go_to_moves_the_playhead_*` |
| `page_control/bridge.rs` `Pump` | eframe's `WebRunner::add_event_listener` lending the app runner, `AppRunner::logic()` running one unpainted pass (eframe 0.37) | Compiled for wasm32 only; no test runs it. `page_control::tests::the_web_shell_hands_its_runner_to_the_bridge` checks `web.rs` still wires it |
| `page_control/bridge.rs` `install_deliver` | `io::set_deliver` being a first-set-wins `OnceLock` | Not covered by a test; if upstream makes it replaceable, upstream's later `set_deliver(download)` would win and host reads would download again |
