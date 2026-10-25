# Lane report: sow-c0sound-fork (C0sound guest page of SoundCraft)

Branch `c0/main` from upstream `0a95b6d` (2026-10-25). Not pushed; `main` untouched (no local `main` ref exists in this work tree, so the ratchet is measured against `0a95b6d`).

## Commits
- `a1f9d56` feat(c0host): page control bridge, c0.* host requests and chrome switches
- `4cffcc8` feat(c0host): C0sound guest page, README and patch debt
- (this file) docs(c0host): lane report

## Upstream files edited (added lines, `git diff --numstat --diff-filter=M 0a95b6d c0/main -- . ':!c0'`)
apps/soundcraft-web/Cargo.toml 7 · apps/soundcraft-web/src/main.rs 3 · apps/soundcraft-web/src/web.rs 8 ·
crates/engine/catalog/menus.txt 1 · crates/ui-egui/src/control.rs 1 · crates/ui-egui/src/edit_window.rs 3 ·
crates/ui-egui/src/lib.rs 12 · crates/ui-egui/src/menus.rs 3 · crates/ui-egui/src/panels.rs 2 · crates/ui-egui/src/theme.rs 3.
Total 43 added lines in 10 files (11 replace upstream lines). Ratchet: 43.

## Tests (song, native)
- `cargo test -p soundcraft-ui-egui --test c0_host_contract --test c0_host_theme`: 17 + 3 passed, 0 failed.
- `cargo test -p soundcraft-ui-egui` (whole crate, upstream's tests included): 408 passed, 0 failed (lib 193, c0_host_contract 17, c0_host_theme 3, other upstream test binaries 195).
- `cargo test -p soundcraft-web --features page-control`: 8 passed.
- `cargo check --target wasm32-unknown-unknown -p soundcraft-web --features page-control`: clean; same without the feature: clean.
- Load (song, 16 cores): contract tests before 10.37 / after 16.82; full suites before 16.43 / after 11.81 (the binaries were already built, so these runs took seconds; song's load is other lanes).

## Page build
`NO_COLOR=true trunk build --release` in c0/soundcraft-host under `smoac-slot run heavy`: success in 1 m 08 s.
Load before 4.11 8.49 13.20, after 24.35 14.13 14.72 (the after figure includes other lanes on song).
`dist/c0host`: wasm 17,501,820 bytes (16.7 MiB; 6.5 MB gzip -9), js 170,864 bytes, total 17,675,885 bytes. The wasm contains `__soundcraftControl`; index.html aliases `__c0hostRequest` and keeps the AudioContext resume.

## c0.selection
Request: `{"method":"c0.selection","params":{"maxSeconds":300}}`
Reply: `{"ok":true,"result":{"kind":"audio","sampleRate":48000,"start":0,"end":48000,"tracks":[2],"clips":[],"channels":2,"wav":"UklGR..."}}`
No time selection: `{"ok":true,"result":{"kind":"audio","sampleRate":48000,"start":0,"end":0,"tracks":[],"clips":[],"empty":true}}`.
Cut at maxSeconds: `end` moved and `"truncated":true`.

## c0.place
Request: `{"method":"c0.place","params":{"wav":"UklGR...","name":"AI voice.wav","track":"new","at":96000}}`
Reply: `{"ok":true,"result":{"placed":"audio","track":14,"clip":31,"source":9,"start":96000,"end":144000}}`
Error: `{"ok":false,"error":"`wav` is not valid base64"}` (nothing changed).
(Ids in these examples are illustrative; the shapes and the 48 kHz one-second figures are what the tests assert.)

## Assumptions I could not confirm
- The browser half of the bridge (`page_control/bridge.rs`: the in-call egui pass through `WebRunner::add_event_listener`, the delivery hook) compiles for wasm32 but no test runs it; it was not loaded in a browser in this lane.
- Audio in the guest page (cpal WebAudio inside the C0VIBE webview) and the AudioContext resume were not heard or measured.
- `c0.selection` renders with `soundcraft_mix::render_range` from the session; plug-in inserts that only run natively may render differently in the browser.
- The chrome switches are saved with the rest of `UiState` where upstream saves it (the struct is `#[serde(default)]`); a desktop profile that once ran with `community_links: false` would keep it.
