//! The host's colour tokens (`c0.set {themeTokens}`). The override is process-wide, so these tests
//! have a binary of their own and take turns.
use egui::{Color32, Context};
use serde_json::{Value, json};
use soundcraft_ui_egui::theme::{self, Tokens, host};
use soundcraft_ui_egui::{ControlRequest, Services, SoundApp, control};
use std::sync::{Mutex, MutexGuard, PoisonError};

fn one_at_a_time() -> MutexGuard<'static, ()> {
    static ONE: Mutex<()> = Mutex::new(());
    let guard = ONE.lock().unwrap_or_else(PoisonError::into_inner);
    host::clear();
    guard
}

fn call(app: &mut SoundApp, ctx: &Context, tx: &std::sync::mpsc::Sender<ControlRequest>, params: Value) -> Value {
    let (req, reply) = ControlRequest::new("c0.set", params);
    tx.send(req).unwrap();
    control::drain(app, ctx);
    reply.try_recv().unwrap()
}

fn rig() -> (SoundApp, Context, std::sync::mpsc::Sender<ControlRequest>) {
    let (tx, rx) = std::sync::mpsc::channel();
    (SoundApp::new(soundcraft_engine::Engine::default(), None, Services::default()).with_control(rx), Context::default(), tx)
}

/// The token fields of `Tokens` as theme.rs declares them: a field added upstream fails here until
/// `theme/host.rs` lists it.
fn declared_tokens() -> (Vec<String>, Vec<String>) {
    let source = include_str!("../src/theme.rs");
    let body = source.split("pub struct Tokens {").nth(1).and_then(|rest| rest.split("\n}\n").next()).expect("the Tokens struct");
    let (mut one, mut two) = (Vec::new(), Vec::new());
    for line in body.lines().map(str::trim) {
        let Some(rest) = line.strip_prefix("pub ") else { continue };
        let Some((name, ty)) = rest.split_once(':') else { continue };
        match ty.trim() {
            "Color32," => one.push(name.to_owned()),
            "[Color32; 2]," => two.push(name.to_owned()),
            other => panic!("a token of a type the host cannot set: {name}: {other}"),
        }
    }
    (one, two)
}

#[test]
fn the_host_can_set_every_token_upstream_declares() {
    let _one = one_at_a_time();
    let (one, two) = declared_tokens();
    assert_eq!((one.len(), two.len()), (80, 2), "count the fields of Tokens in theme.rs");
    let mut declared: Vec<String> = one.iter().chain(&two).cloned().collect();
    declared.sort();
    let mut known: Vec<String> = host::TOKEN_NAMES.iter().map(|s| (*s).to_owned()).collect();
    known.sort();
    assert_eq!(declared, known, "theme/host.rs lists exactly the fields of Tokens");

    // Every name is accepted, with the right shape.
    let mut all = serde_json::Map::new();
    for name in &one {
        all.insert(name.clone(), json!("#102030"));
    }
    for name in &two {
        all.insert(name.clone(), json!(["#102030", "#40506080"]));
    }
    let report = host::set(&Value::Object(all)).unwrap();
    assert_eq!(report.applied.len(), 82);
    assert!(report.unknown.is_empty() && report.invalid.is_empty());
    let t = Tokens::current();
    assert_eq!(t.window_bg, Color32::from_rgb(0x10, 0x20, 0x30));
    assert_eq!(t.slot_mark, Color32::from_rgb(0x10, 0x20, 0x30));
    assert_eq!(t.slot_face, [Color32::from_rgb(0x10, 0x20, 0x30), Color32::from_rgba_unmultiplied(0x40, 0x50, 0x60, 0x80)]);
    host::clear();
}

#[test]
fn tokens_apply_unknown_and_invalid_are_reported_by_name() {
    let _one = one_at_a_time();
    let (mut app, ctx, tx) = rig();
    let base = Tokens::current();
    let reply = call(
        &mut app,
        &ctx,
        &tx,
        json!({"themeTokens": {
            "accent": "#ff8000", "panel_bg": "#11223344", "slot_face": ["#000000", "#ffffff"],
            "not_a_token": "#ffffff", "also_missing": 3, "text": "orange", "button": 7, "slot_bypassed": ["#000000"], "rec": null}}),
    );
    assert_eq!(reply["ok"], true, "{reply}");
    let r = &reply["result"]["themeTokens"];
    let names = |k: &str| -> Vec<String> {
        let mut v: Vec<String> = r[k].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_owned()).collect();
        v.sort();
        v
    };
    assert_eq!(names("applied"), ["accent", "panel_bg", "rec", "slot_face"]);
    assert_eq!(names("unknown"), ["also_missing", "not_a_token"]);
    assert_eq!(names("invalid"), ["button", "slot_bypassed", "text"]);

    let t = Tokens::current();
    assert_eq!(t.accent, Color32::from_rgb(255, 128, 0));
    assert_eq!(t.panel_bg, Color32::from_rgba_unmultiplied(0x11, 0x22, 0x33, 0x44));
    assert_eq!(t.slot_face, [Color32::BLACK, Color32::WHITE]);
    assert_eq!((t.text, t.button, t.slot_bypassed, t.rec), (base.text, base.button, base.slot_bypassed, base.rec), "refused names change nothing");
    // The egui visuals are built from the tokens, so they follow.
    assert_eq!(ctx.global_style().visuals.selection.bg_fill, Color32::from_rgb(255, 128, 0));
    assert_eq!(host::active(), ["accent", "panel_bg", "slot_face"]);

    // A later call merges; null takes one away; null for all takes all away.
    let reply = call(&mut app, &ctx, &tx, json!({"themeTokens": {"accent": null, "text": "#010203"}}));
    assert_eq!(reply["result"]["themeTokens"]["applied"], json!(["accent", "text"]));
    let t = Tokens::current();
    assert_eq!((t.accent, t.panel_bg, t.text), (base.accent, Color32::from_rgba_unmultiplied(0x11, 0x22, 0x33, 0x44), Color32::from_rgb(1, 2, 3)));
    let reply = call(&mut app, &ctx, &tx, json!({"themeTokens": null}));
    assert_eq!(reply["ok"], true);
    assert!(host::active().is_empty());
    let t = Tokens::current();
    assert_eq!((t.accent, t.panel_bg, t.text, t.slot_face), (base.accent, base.panel_bg, base.text, base.slot_face), "back to upstream's own colours");
    assert_eq!(call(&mut app, &ctx, &tx, json!({"themeTokens": 5}))["ok"], false);
}

#[test]
fn the_overrides_stay_when_the_person_switches_light_and_dark() {
    let _one = one_at_a_time();
    let (mut app, ctx, tx) = rig();
    call(&mut app, &ctx, &tx, json!({"themeTokens": {"playhead": "#00ff00"}}));
    assert_eq!(Tokens::current().playhead, Color32::from_rgb(0, 255, 0));
    theme::apply(&ctx, true);
    assert!(theme::is_light());
    assert_eq!(Tokens::current().playhead, Color32::from_rgb(0, 255, 0));
    assert_eq!(Tokens::current().panel_bg, Tokens::LIGHT.panel_bg, "only the named token changes");
    theme::apply(&ctx, false);
    host::clear();
    assert_eq!(Tokens::current().playhead, Tokens::DARK.playhead);
}
