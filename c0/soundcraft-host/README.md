# c0/soundcraft-host

The page C0VIBE loads to host SoundCraft as a wrapped module (C0sound). It is ours; upstream SoundCraft
does not ship it.

- Builds the unchanged `soundcraft-web` app with the `page-control` feature. `page-control`
  (`apps/soundcraft-web/src/page_control.rs` and `page_control/bridge.rs`) feeds SoundCraft's own control
  channel (`SoundApp::with_control`, the one the desktop app serves over its socket) from the page as
  `window.__soundcraftControl`; this page aliases it as `window.__c0hostRequest` for the host.
- The page keeps upstream's AudioContext-resume script (audio starts on the first gesture) and is
  transparent until the app paints, so the host's own loading state shows through.
- No ArtCraft name, logo or community link once the host sets `community_links: false`.

Call shape, from the host:

    const reply = JSON.parse(await window.__c0hostRequest(JSON.stringify({ method, params })));

Every reply is `{"ok": true, "result": ...}` or `{"ok": false, "error": "plain text"}`. The desktop
protocol's requests (`engine.execute`, `engine.commands`, `session.inspect`, `ui.inspect`, `ui.set`,
`ui.menu.list`, `ui.menu.invoke`, `ui.click` and the other input, `ui.screenshot`) work as on the
desktop. A `c0.*` request may be up to 96 MiB (a file or audio travels in it as base64); every other
request keeps 1 MiB.

## When a request is answered

A request does not wait for the display. The app handles control requests at the start of a frame,
and a browser gives a window that is not being presented about one frame a second, so the bridge runs
one egui pass of its own inside the call (the queue is drained, the interface laid out, nothing painted)
and the reply is ready when the call returns.

- A request that only reads (`c0.inspect`, `c0.documentBytes`, `c0.selection`, `ui.inspect`,
  `session.inspect`, `ui.menu.list`, `engine.commands`, and `engine.execute` of `session.inspect`)
  asks for no frame.
- Any other request asks for a repaint afterwards so the change is seen.
- `ui.click`, `ui.key` and the other scripted input are played one batch per pass, up to 32 passes inside
  the call; what is left gets a pass every 16 ms while it waits.
- `ui.screenshot` reads painted pixels and still waits for painted frames.
- Before the app has started, the request wakes the app and waits for its first frame.

While a `c0.*` request runs, a file the app would hand the person as a download is dropped (the host
reads bytes through `c0.documentBytes` and `c0.selection`). Saves and bounces the person starts still
download.

## The host's requests: `c0.*`

Code: `crates/ui-egui/src/c0_host.rs` and `c0_host/`; tests: `crates/ui-egui/tests/c0_host_contract.rs`
and `c0_host_theme.rs`. None shows a dialog, writes to disk or takes a path. Positions are in samples at
the session's sample rate. Base64 is the standard alphabet, no `data:` prefix.

| Request | Params | Result |
|---|---|---|
| `c0.inspect` | `{}` | `{title, dirty, revision, canUndo, canRedo, playing, playheadSamples, sampleRate, tempo, tracks: [{id, name, kind, muted, solo, selected}], clips: [{id, name, track, start, end}], clipsTruncated, markers: [{id, name, at}], selection: {start, end, tracks, clips}, window: "edit" \| "mix", panels: {...the c0.set keys}, theme: {mode, light, overrides}}`. `kind` is `audio`, `aux`, `master`, `midi`, `instrument`, `vca`, `folder` or `video`; at most 5,000 clips are listed |
| `c0.goTo` | exactly one of `{marker}` (id or name), `{samples}`, `{track}` (id or name) | `{playhead, marker?}` or `{playhead, track}`: moves the playhead, or selects the track and scrolls the Edit window to it |
| `c0.documentBytes` | `{}` | `{name, format: "scraftz", bytes, revision}`: the session as one bundle (the `.scraft` and its audio), as File > Save writes it in the browser; read only (dirty, history and revision do not change) |
| `c0.openFile` | `{name, bytes}` | `.scraftz` / `.scraft`: opens as the session, through the app's unsaved-changes question: `{opened: true, kind: "session", name, status}`, or `{opened: false, kind: "session", pending: "unsaved-changes"}` while the question is open. Any audio file the importer reads (or MIDI) goes onto a new track at the start as one undo step: `{opened: true, kind: "audio" \| "midi", name, start, track, clip, end}` |
| `c0.newDocument` | `{}` | `{created: true, title}`: an empty session, no demo; `{created: false, pending: "unsaved-changes"}` while the app asks about changes |
| `c0.cancelFileDialog` | `{}` | `{cancelled}`: dismisses an open in-page path prompt, or the question that waits to open a file |
| `c0.set` | `{themeTokens?, panels?}` | `{themeTokens?: {applied, unknown, invalid}, panels?: {applied, unknown, invalid}}`, each by name. A bad name or value changes nothing for that name and does not stop the others. Any other top-level key fails the request |
| `c0.selection` | `{maxSeconds?: 300}` (at most 600) | `{kind: "audio", sampleRate, start, end, tracks, clips, channels, wav}`: the time selection rendered from the selected tracks (the whole mix when none is selected; other tracks are muted, nobody solos, so inserts, faders and busses stay) as a 16-bit WAV, mono or stereo as rendered. Longer than `maxSeconds`: cut there, `end` moved, `truncated: true`. No time selection: `empty: true` and no `wav` |
| `c0.place` | `{wav \| mp3 \| flac \| ogg: base64, name?, track?: id \| "new", at?: samples}` | `{placed: "audio", track, clip, source, start, end}`: imported onto that audio track (a new one by default) at `at` (the playhead by default), one undo step named "Place Audio". Errors change nothing: no or two formats, bad base64, an unreadable file, more than 60 MB, an unknown or non-audio track, a negative `at` |

### `c0.set {panels}`

| Key | Value | Default | What |
|---|---|---|---|
| `menu_bar` | bool | true | the in-window menu bar (the inverse of the app's `native_menu_bar`) |
| `edit_toolbar` | bool | true | the Edit window toolbar |
| `status_bar` | bool | true | the Edit window status bar |
| `dock_tabs` | bool | true | the lower dock's tab strip |
| `community_links` | bool | true | the About window's getartcraft.com and Discord links (About still names SoundCraft, its authors and licence) |
| `quit_menu` | bool | true | "Quit" in the SoundCraft menu (the host owns quitting) |
| `transport` | bool | upstream's | the floating Transport window (`show_transport`) |
| `tracks_list` | bool | upstream's | the Tracks list (`show_tracks_list`) |
| `clip_list` | bool | upstream's | the Clip list (`show_clip_list`) |
| `window` | `"edit"` \| `"mix"` | `"edit"` | the main window |

The first five are `UiState` fields, so upstream's `ui.set` sets them too. At their defaults SoundCraft
draws exactly upstream's chrome (tested).

### `c0.set {themeTokens}`

    {"method": "c0.set", "params": {"themeTokens": {"window_bg": "#101114", "accent": "#7c5cffcc", "slot_face": ["#2a2b30", "#1d1e22"]}}}

Names are the fields of `Tokens` in `crates/ui-egui/src/theme.rs` (80 colours and the two pairs
`slot_face`, `slot_bypassed`, which take an array of two colours). Colours are `"#rrggbb"` or
`"#rrggbbaa"` (alpha not premultiplied). A call merges into what is set; `null` for a name takes that
override away, `null` for `themeTokens` takes all of them away. The overrides lie over both presets, so
they stay when the person switches Light and Dark (`crates/ui-egui/src/theme/host.rs`, read by
`Tokens::current()` every frame). `unknown` lists names that are not tokens (a token renamed upstream shows
up here after a pull); `invalid` lists tokens whose value is not a colour.

## Build, on a fleet host only

    cd c0/soundcraft-host && NO_COLOR=true trunk build --release

Output: `dist/c0host/` (index.html, one .js, one .wasm).

## What the host relies on

`crates/ui-egui/tests/c0_host_contract.rs` and `c0_host_theme.rs` fail when a pull changes a reply
shape, a switch, a theme token or the trademark strip. Run them after every pull:

    cargo test -p soundcraft-ui-egui --test c0_host_contract --test c0_host_theme
    cargo test -p soundcraft-web --features page-control
    cargo check --target wasm32-unknown-unknown -p soundcraft-web --features page-control

## Branches

`main` mirrors upstream and holds nothing of ours. Our work is on `c0/main`; upstream is merged into it,
never rebased. Every edit inside an upstream file is listed in `PATCH-DEBT.md`.
