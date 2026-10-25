//! The C0VIBE host's requests (`c0.*`): every reply shape, the chrome switches (at their defaults
//! the app is upstream's), file traffic, and the AI hooks. The colour tokens are in `c0_host_theme.rs`
//! (they are process-wide, so they need a binary of their own).
use egui::{Context, RawInput, Rect, ViewportId, ViewportInfo, pos2, vec2};
use egui_kittest::{Harness, kittest::Queryable};
use serde_json::{Value, json};
use soundcraft_time::Range;
use soundcraft_ui_egui::{ControlRequest, Services, SoundApp, c0_host, control};
use std::sync::mpsc::Sender;

struct Rig {
    app: SoundApp,
    ctx: Context,
    tx: Sender<ControlRequest>,
}

fn rig_with(engine: soundcraft_engine::Engine) -> Rig {
    let (tx, rx) = std::sync::mpsc::channel();
    let app = SoundApp::new(engine, None, Services::default()).with_control(rx);
    Rig { app, ctx: Context::default(), tx }
}

fn rig() -> Rig {
    rig_with(soundcraft_engine::demo::demo_engine())
}

fn raw() -> RawInput {
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 900.0));
    let viewport = ViewportInfo { inner_rect: Some(rect), native_pixels_per_point: Some(1.0), ..Default::default() };
    RawInput { screen_rect: Some(rect), max_texture_side: Some(8192), viewports: [(ViewportId::ROOT, viewport)].into_iter().collect(), ..Default::default() }
}

impl Rig {
    fn call(&mut self, method: &str, params: Value) -> Value {
        let (req, reply) = ControlRequest::new(method, params);
        self.tx.send(req).unwrap();
        control::drain(&mut self.app, &self.ctx);
        reply.try_recv().unwrap_or_else(|_| panic!("{method}: no reply"))
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let reply = self.call(method, params);
        assert_eq!(reply["ok"], true, "{method}: {reply}");
        reply["result"].clone()
    }

    fn err(&mut self, method: &str, params: Value) -> String {
        let reply = self.call(method, params);
        assert_eq!(reply["ok"], false, "{method}: {reply}");
        assert!(reply.get("result").is_none(), "{reply}");
        reply["error"].as_str().unwrap_or_default().to_owned()
    }

    /// One real UI pass; every text it painted.
    fn painted(&mut self) -> Vec<String> {
        let mut out = self.ctx.run_ui(raw(), |ui| {
            self.app.logic(ui.ctx());
            self.app.ui(ui);
        });
        out.textures_delta.clear();
        let mut texts = Vec::new();
        for clipped in &out.shapes {
            collect(&clipped.shape, &mut texts);
        }
        texts
    }

    fn painted_after_settling(&mut self) -> Vec<String> {
        for _ in 0..5 {
            self.painted();
        }
        self.painted()
    }
}

fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(t) => out.push(t.galley.text().to_owned()),
        egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
        _ => {}
    }
}

fn has(texts: &[String], needle: &str) -> bool {
    texts.iter().any(|t| t.contains(needle))
}

fn unb64(text: &str) -> Vec<u8> {
    c0_host::b64::decode(text).expect("base64")
}

fn b64(bytes: &[u8]) -> String {
    control::base64(bytes)
}

fn tracks(r: &Rig) -> usize {
    r.app.engine.session().tracks.len()
}

fn clips(r: &Rig) -> usize {
    r.app.engine.session().tracks.iter().map(|t| t.clips().len()).sum()
}

/// A rendered second of the demo, as a WAV the importer reads back.
fn demo_wav(r: &mut Rig) -> String {
    r.app.engine.session_mut().edit.selection = Range::new(0, 48_000);
    r.ok("c0.selection", json!({}))["wav"].as_str().unwrap().to_owned()
}

#[test]
fn the_host_methods_are_the_nine_the_page_bridge_knows() {
    assert_eq!(
        c0_host::METHODS,
        ["c0.inspect", "c0.goTo", "c0.documentBytes", "c0.openFile", "c0.newDocument", "c0.cancelFileDialog", "c0.set", "c0.selection", "c0.place"]
    );
    let mut r = rig();
    assert!(r.err("c0.nope", json!({})).contains("unknown method"));
}

#[test]
fn inspect_reports_the_session_the_selection_and_the_chrome() {
    let mut r = rig();
    let v = r.ok("c0.inspect", json!({}));
    for key in ["title", "dirty", "revision", "canUndo", "canRedo", "playing", "playheadSamples", "sampleRate", "tempo", "tracks", "clips", "markers", "selection", "window", "panels", "theme"] {
        assert!(v.get(key).is_some(), "missing {key}: {v}");
    }
    assert_eq!(v["window"], "edit");
    assert_eq!(v["playing"], false);
    assert!(v["sampleRate"].as_u64().unwrap() > 0 && v["tempo"].as_f64().unwrap() > 0.0);
    let tracks = v["tracks"].as_array().unwrap();
    assert!(!tracks.is_empty() && !v["clips"].as_array().unwrap().is_empty());
    for t in tracks {
        for key in ["id", "name", "kind", "muted", "solo", "selected"] {
            assert!(t.get(key).is_some(), "track {t}");
        }
    }
    for c in v["clips"].as_array().unwrap() {
        for key in ["id", "name", "track", "start", "end"] {
            assert!(c.get(key).is_some(), "clip {c}");
        }
    }
    for key in ["start", "end", "tracks", "clips"] {
        assert!(v["selection"].get(key).is_some(), "selection {key}");
    }
    assert_eq!(v["theme"]["mode"], "dark");
    // The counts match the engine's own.
    assert_eq!(tracks.len(), self::tracks(&r));
    assert_eq!(v["markers"].as_array().unwrap().len(), r.app.engine.session().markers.len());
}

#[test]
fn go_to_moves_the_playhead_to_a_position_a_marker_or_a_track() {
    let mut r = rig();
    assert_eq!(r.ok("c0.goTo", json!({"samples": 96_000}))["playhead"], 96_000);
    assert_eq!(r.ok("c0.inspect", json!({}))["playheadSamples"], 96_000);
    r.app.run("markers.add", json!({"name": "Verse", "at": 144_000})).unwrap();
    let marker = r.ok("c0.inspect", json!({}))["markers"].as_array().unwrap().iter().find(|m| m["name"] == "Verse").unwrap().clone();
    assert_eq!(marker["name"], "Verse");
    assert_eq!(r.ok("c0.goTo", json!({"marker": "Verse"}))["playhead"], marker["at"]);
    r.ok("c0.goTo", json!({"samples": 0}));
    assert_eq!(r.ok("c0.goTo", json!({"marker": marker["id"]}))["marker"], marker["id"]);
    let second = r.ok("c0.inspect", json!({}))["tracks"][1]["id"].clone();
    r.ok("c0.goTo", json!({"track": second}));
    let v = r.ok("c0.inspect", json!({}));
    assert_eq!(v["selection"]["tracks"], json!([second]));
    assert!(r.err("c0.goTo", json!({})).contains("one of"));
    assert!(r.err("c0.goTo", json!({"samples": 1, "track": 1})).contains("one place"));
    assert!(r.err("c0.goTo", json!({"samples": -4})).contains("samples"));
    assert!(r.err("c0.goTo", json!({"marker": "Nowhere"})).contains("no marker"));
    assert!(r.err("c0.goTo", json!({"track": 999_999})).contains("track"));
}

#[test]
fn document_bytes_round_trips_through_open_file_and_reads_only() {
    let mut a = rig();
    let revision = a.app.engine.revision;
    let doc = a.ok("c0.documentBytes", json!({}));
    assert_eq!(doc["format"], "scraftz");
    assert!(doc["name"].as_str().unwrap().ends_with(".scraftz"));
    assert_eq!(doc["revision"], revision);
    assert_eq!(a.app.engine.revision, revision, "reading changes nothing");
    let bytes = unb64(doc["bytes"].as_str().unwrap());
    assert!(bytes.starts_with(b"PK\x03\x04"));

    let mut b = rig_with(soundcraft_engine::Engine::default());
    assert_eq!(tracks(&b), 0);
    let opened = b.ok("c0.openFile", json!({"name": doc["name"], "bytes": doc["bytes"]}));
    assert_eq!((opened["opened"].clone(), opened["kind"].clone()), (json!(true), json!("session")));
    assert_eq!(tracks(&b), tracks(&a));
    assert_eq!(clips(&b), clips(&a));
    assert_eq!(b.app.engine.session().name, a.app.engine.session().name);
}

#[test]
fn open_file_asks_about_unsaved_changes_and_cancel_dismisses_the_question() {
    let mut a = rig();
    let doc = a.ok("c0.documentBytes", json!({}));
    let mut b = rig_with(soundcraft_engine::Engine::default());
    b.app.run("track.new", json!({"name": "Unsaved"})).unwrap();
    let before = tracks(&b);
    let r = b.ok("c0.openFile", json!({"name": doc["name"], "bytes": doc["bytes"]}));
    assert_eq!((r["opened"].clone(), r["pending"].clone()), (json!(false), json!("unsaved-changes")));
    assert_eq!(tracks(&b), before, "nothing opened before the answer");
    assert_eq!(b.ok("c0.cancelFileDialog", json!({}))["cancelled"], true);
    assert!(b.app.dialogs.open.is_none());
    assert_eq!(b.ok("c0.cancelFileDialog", json!({}))["cancelled"], false, "nothing left to dismiss");
}

#[test]
fn cancel_file_dialog_dismisses_a_path_prompt() {
    let mut r = rig();
    r.ok("ui.menu.invoke", json!({"id": "session.open"}));
    assert_eq!(r.app.dialogs.open_name(), Some("path"));
    assert_eq!(r.ok("c0.cancelFileDialog", json!({}))["cancelled"], true);
    assert!(r.app.dialogs.open.is_none());
}

#[test]
fn open_file_imports_audio_on_a_new_track_at_the_start_as_one_undo_step() {
    let mut r = rig();
    let wav = demo_wav(&mut r);
    let (t, c) = (tracks(&r), clips(&r));
    let v = r.ok("c0.openFile", json!({"name": "Voice.wav", "bytes": wav}));
    assert_eq!((v["opened"].clone(), v["kind"].clone(), v["start"].clone()), (json!(true), json!("audio"), json!(0)));
    assert_eq!((tracks(&r), clips(&r)), (t + 1, c + 1));
    assert!(v["end"].as_i64().unwrap() > 0 && v["track"].is_u64() && v["clip"].is_u64());
    assert!(r.app.engine.undo());
    assert_eq!((tracks(&r), clips(&r)), (t, c), "one undo takes the whole import back");
    assert!(r.err("c0.openFile", json!({"name": "x.wav", "bytes": "!!"})).contains("base64"));
    assert!(r.err("c0.openFile", json!({"name": "x.wav", "bytes": b64(b"not audio at all")})).contains("x.wav"));
    assert!(r.err("c0.openFile", json!({"bytes": "AAAA"})).contains("name"));
    assert_eq!((tracks(&r), clips(&r)), (t, c));
}

#[test]
fn new_document_is_an_empty_session_without_the_demo() {
    let mut r = rig();
    assert!(tracks(&r) > 0);
    // The demo is not dirty, so no question.
    let v = r.ok("c0.newDocument", json!({}));
    assert_eq!(v["created"], true);
    assert_eq!(tracks(&r), 0);
    assert_eq!(clips(&r), 0);
    // With changes it asks first.
    r.app.run("track.new", json!({"name": "A"})).unwrap();
    let v = r.ok("c0.newDocument", json!({}));
    assert_eq!((v["created"].clone(), v["pending"].clone()), (json!(false), json!("unsaved-changes")));
    assert_eq!(tracks(&r), 1);
}

#[test]
fn set_reports_each_panel_key_and_changes_only_the_good_ones() {
    let mut r = rig();
    let defaults = r.ok("c0.inspect", json!({}))["panels"].clone();
    for key in ["menu_bar", "edit_toolbar", "status_bar", "dock_tabs", "community_links", "quit_menu"] {
        assert_eq!(defaults[key], true, "{key} defaults on");
    }
    let v = r.ok(
        "c0.set",
        json!({"panels": {"menu_bar": false, "edit_toolbar": false, "status_bar": false, "dock_tabs": false, "community_links": false, "quit_menu": false,
            "transport": true, "tracks_list": false, "clip_list": false, "window": "mix", "bogus": true, "dock_tabs_x": 1, "undo_history": 3}}),
    );
    let report = &v["panels"];
    let list = |k: &str| -> Vec<String> { report[k].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_owned()).collect() };
    let mut applied = list("applied");
    applied.sort();
    let mut want: Vec<String> = ["menu_bar", "edit_toolbar", "status_bar", "dock_tabs", "community_links", "quit_menu", "transport", "tracks_list", "clip_list", "window"]
        .map(String::from)
        .to_vec();
    want.sort();
    assert_eq!(applied, want);
    let mut unknown = list("unknown");
    unknown.sort();
    assert_eq!(unknown, ["bogus", "dock_tabs_x", "undo_history"]);
    assert!(list("invalid").is_empty());
    let panels = r.ok("c0.inspect", json!({}))["panels"].clone();
    assert_eq!(panels["menu_bar"], false);
    assert_eq!(panels["window"], "mix");
    assert_eq!(panels["transport"], true);
    assert_eq!(panels["tracks_list"], false);
    assert_eq!(panels["quit_menu"], false);
    assert!(r.app.native_menu_bar, "menu_bar off hands the menu to the host");
    // Bad values are named and change nothing.
    let v = r.ok("c0.set", json!({"panels": {"window": "sideways", "status_bar": "yes", "edit_toolbar": true}}));
    let mut invalid: Vec<&str> = v["panels"]["invalid"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
    invalid.sort_unstable();
    assert_eq!(invalid, ["status_bar", "window"]);
    assert_eq!(v["panels"]["applied"], json!(["edit_toolbar"]));
    let panels = r.ok("c0.inspect", json!({}))["panels"].clone();
    assert_eq!((panels["window"].clone(), panels["status_bar"].clone(), panels["edit_toolbar"].clone()), (json!("mix"), json!(false), json!(true)));
    assert!(r.err("c0.set", json!({"colours": {}})).contains("unknown field"));
    assert!(r.err("c0.set", json!({"panels": [1]})).contains("object"));
}

#[test]
fn the_switches_at_their_defaults_leave_the_chrome_as_upstream_draws_it() {
    let mut r = rig();
    let texts = r.painted_after_settling();
    assert!(has(&texts, "Nudge"), "edit toolbar");
    assert!(has(&texts, " Hz · "), "status bar");
    assert!(has(&texts, "MIDI EDITOR"), "lower dock tabs");
    assert!(has(&texts, "SoundCraft"), "menu bar");
    // The UiState defaults are the ones the app always had.
    let ui = serde_json::to_value(soundcraft_ui_egui::UiState::default()).unwrap();
    for key in ["edit_toolbar", "status_bar", "dock_tabs", "community_links", "quit_menu"] {
        assert_eq!(ui[key], true, "{key}");
    }
    // A saved preferences file from before the switches loads with them on.
    let old: soundcraft_ui_egui::UiState = serde_json::from_value(json!({"show_transport": true})).unwrap();
    assert!(old.edit_toolbar && old.status_bar && old.dock_tabs && old.community_links && old.quit_menu);
}

#[test]
fn the_edit_toolbar_the_status_bar_and_the_dock_tabs_each_hide_on_their_own() {
    for (key, gone) in [("edit_toolbar", "Nudge"), ("status_bar", " Hz · "), ("dock_tabs", "MIDI EDITOR")] {
        let mut r = rig();
        r.ok("c0.set", json!({"panels": {key: false}}));
        let texts = r.painted_after_settling();
        assert!(!has(&texts, gone), "{key} off still draws {gone}");
        for (other, shown) in [("edit_toolbar", "Nudge"), ("status_bar", " Hz · "), ("dock_tabs", "MIDI EDITOR")] {
            if other != key {
                assert!(has(&texts, shown), "{key} off must not hide {other}");
            }
        }
        r.ok("c0.set", json!({"panels": {key: true}}));
        assert!(has(&r.painted_after_settling(), gone), "{key} comes back");
    }
}

#[test]
fn the_menu_bar_switch_hides_the_menu_and_quit_menu_hides_only_quit() {
    let mut r = rig();
    r.ok("c0.set", json!({"panels": {"menu_bar": false}}));
    assert!(!has(&r.painted_after_settling(), "SoundCraft"), "no menu bar");

    for (quit_menu, shown) in [(true, true), (false, false)] {
        let mut app = SoundApp::new(soundcraft_engine::Engine::default(), None, Services::default());
        app.ui.quit_menu = quit_menu;
        let mut h = Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SoundApp| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(3);
        h.get_by_label("SoundCraft").click();
        h.run_steps(4);
        assert!(h.query_by_label("About SoundCraft").is_some(), "the app menu opened (quit_menu {quit_menu})");
        assert_eq!(h.query_by_label("Quit").is_some(), shown, "quit_menu {quit_menu}");
    }
}

#[test]
fn community_links_off_removes_both_links_and_keeps_the_credit() {
    for (links, shown) in [(true, true), (false, false)] {
        let mut r = rig();
        r.ok("c0.set", json!({"panels": {"community_links": links}}));
        r.app.ui.show_about = true;
        let texts = r.painted_after_settling();
        assert!(has(&texts, "Version "), "About is open");
        assert!(has(&texts, "Dual-licensed MIT OR Apache-2.0"), "the licence stays");
        assert!(has(&texts, "ArtCraft team"), "the authors stay named");
        assert_eq!(has(&texts, "getartcraft.com"), shown, "ArtCraft link, community_links {links}");
        assert_eq!(has(&texts, "Join us on Discord"), shown, "Discord link, community_links {links}");
    }
}

#[test]
fn the_menu_names_the_search_window_without_the_trademark() {
    let catalog = soundcraft_engine::catalog::catalog();
    assert!(catalog.iter().any(|p| *p == "Window > Search"));
    assert!(!catalog.iter().any(|p| p.contains("Pro Tools Search")));
    assert_eq!(soundcraft_ui_egui::menus::command_for_path("Window > Search", &soundcraft_ui_egui::menus::ui_aliases()).as_deref(), Some("window.search"));
}

fn wav_info(bytes: &[u8]) -> (u16, u32, usize) {
    assert!(bytes.len() > 44 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE", "a WAV header");
    let (mut channels, mut rate) = (0, 0);
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]) as usize;
        match &bytes[at..at + 4] {
            b"fmt " => {
                channels = u16::from_le_bytes([bytes[at + 10], bytes[at + 11]]);
                rate = u32::from_le_bytes([bytes[at + 12], bytes[at + 13], bytes[at + 14], bytes[at + 15]]);
            }
            b"data" => return (channels, rate, size),
            _ => {}
        }
        at += 8 + size + (size & 1);
    }
    panic!("no data chunk");
}

#[test]
fn selection_renders_the_time_selection_to_a_wav() {
    let mut r = rig();
    let rate = r.app.engine.session().sample_rate.hz();
    // No time selection: nothing to render.
    r.app.engine.session_mut().edit.selection = Range::point(0);
    let v = r.ok("c0.selection", json!({}));
    assert_eq!((v["kind"].clone(), v["empty"].clone()), (json!("audio"), json!(true)));
    assert!(v.get("wav").is_none());
    for key in ["sampleRate", "start", "end", "tracks", "clips"] {
        assert!(v.get(key).is_some(), "{key}");
    }

    // A known range of the whole mix.
    r.app.engine.session_mut().edit.selection = Range::new(0, i64::from(rate));
    let v = r.ok("c0.selection", json!({}));
    assert_eq!((v["kind"].clone(), v["start"].clone(), v["end"].clone(), v["sampleRate"].clone()), (json!("audio"), json!(0), json!(rate), json!(rate)));
    let wav = unb64(v["wav"].as_str().unwrap());
    let (channels, wav_rate, data) = wav_info(&wav);
    assert_eq!(wav_rate, rate);
    assert_eq!(channels as u64, v["channels"].as_u64().unwrap());
    assert_eq!(data, rate as usize * channels as usize * 2, "one second of 16-bit audio");
    assert!(wav[44..].iter().any(|b| *b != 0), "the demo mix is not silence");

    // Only the selected tracks sound; their ids come back.
    let first = r.app.engine.session().tracks.iter().find(|t| !t.clips().is_empty()).unwrap().id;
    r.app.engine.session_mut().edit.selected_tracks = vec![first];
    let one = r.ok("c0.selection", json!({}));
    assert_eq!(one["tracks"], json!([first.0]));
    let (_, _, data_one) = wav_info(&unb64(one["wav"].as_str().unwrap()));
    assert_eq!(data_one, data);

    // The cap.
    r.app.engine.session_mut().edit.selection = Range::new(0, i64::from(rate) * 10);
    let v = r.ok("c0.selection", json!({"maxSeconds": 2}));
    assert_eq!((v["end"].clone(), v["truncated"].clone()), (json!(rate * 2), json!(true)));
    let (channels, _, data) = wav_info(&unb64(v["wav"].as_str().unwrap()));
    assert_eq!(data, rate as usize * 2 * channels as usize * 2);
    assert!(r.err("c0.selection", json!({"maxSeconds": "ten"})).contains("maxSeconds"));
    assert!(r.err("c0.selection", json!({"maxSeconds": 0})).contains("maxSeconds"));
}

#[test]
fn place_adds_one_clip_and_one_undo_removes_it() {
    let mut r = rig();
    let wav = demo_wav(&mut r);
    let (t, c) = (tracks(&r), clips(&r));
    r.app.run("transport.locate", json!({"at": 96_000})).unwrap();

    // A new track, at the playhead.
    let v = r.ok("c0.place", json!({"wav": wav, "name": "AI voice.wav"}));
    assert_eq!(v["placed"], "audio");
    assert_eq!(v["start"], 96_000);
    assert_eq!(v["end"], 96_000 + 48_000);
    for key in ["track", "clip", "source"] {
        assert!(v[key].is_u64(), "{key}: {v}");
    }
    assert_eq!((tracks(&r), clips(&r)), (t + 1, c + 1));
    let (_, clip) = r.app.engine.session().find_clip(soundcraft_model::ClipId(v["clip"].as_u64().unwrap())).unwrap();
    assert_eq!((clip.start, clip.name.clone()), (96_000, "AI voice".to_owned()));
    assert!(r.app.engine.undo());
    assert_eq!((tracks(&r), clips(&r)), (t, c), "one undo step removes the clip and its track");

    // Onto an audio track, at a position.
    let existing = r.app.engine.session().tracks.iter().find(|t| t.kind == soundcraft_model::TrackKind::Audio).unwrap().id;
    let v = r.ok("c0.place", json!({"wav": wav, "track": existing.0, "at": 5_000}));
    assert_eq!((v["track"].clone(), v["start"].clone()), (json!(existing.0), json!(5_000)));
    assert_eq!((tracks(&r), clips(&r)), (t, c + 1));
    r.ok("c0.place", json!({"wav": wav, "track": "new"}));
    assert_eq!(tracks(&r), t + 1);
}

#[test]
fn place_refuses_what_it_cannot_place_and_changes_nothing() {
    let mut r = rig();
    let wav = demo_wav(&mut r);
    let midi = r.app.engine.session().tracks.iter().find(|t| t.kind != soundcraft_model::TrackKind::Audio).map(|t| t.id);
    let (t, c, rev) = (tracks(&r), clips(&r), r.app.engine.revision);
    let mut cases = vec![
        (json!({"wav": "not base64 !"}), "base64"),
        (json!({"mp3": b64(b"these bytes are not an mp3")}), "could not be read"),
        (json!({"wav": b64(b"RIFF....WAVE-ish")}), "could not be read"),
        (json!({}), "nothing to place"),
        (json!({"wav": wav, "flac": wav}), "one file at a time"),
        (json!({"wav": 5}), "base64"),
        (json!({"wav": wav, "track": 987_654}), "no track"),
        (json!({"wav": wav, "track": "second"}), "track"),
        (json!({"wav": wav, "at": -1}), "`at`"),
        (json!({"wav": "A".repeat(90_000_000)}), "60 MB"),
    ];
    if let Some(id) = midi {
        cases.push((json!({"wav": wav, "track": id.0}), "not an audio track"));
    }
    for (params, expect) in cases {
        let error = r.err("c0.place", params.clone());
        assert!(error.contains(expect), "{error} should contain {expect}");
        assert_eq!((tracks(&r), clips(&r), r.app.engine.revision), (t, c, rev), "{expect}: nothing changed");
    }
}
