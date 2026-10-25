//! `c0.set`: the host's chrome switches and colours.

use crate::{MainWindow, SoundApp};
use serde_json::{Map, Value, json};

/// The `panels` keys that are plain on/off switches.
/// `menu_bar` is the inverse of the app's `native_menu_bar` (the host owns an OS menu when that is set).
const SWITCHES: [&str; 9] =
    ["menu_bar", "edit_toolbar", "status_bar", "dock_tabs", "community_links", "quit_menu", "transport", "tracks_list", "clip_list"];

/// Where `ui.set` keeps the upstream switches this request also sets.
fn ui_field(key: &str) -> Option<&'static str> {
    match key {
        "transport" => Some("show_transport"),
        "tracks_list" => Some("show_tracks_list"),
        "clip_list" => Some("show_clip_list"),
        _ => None,
    }
}

/// The current state of every `panels` key.
pub fn state(app: &SoundApp) -> Value {
    json!({
        "menu_bar": !app.native_menu_bar,
        "edit_toolbar": app.ui.edit_toolbar,
        "status_bar": app.ui.status_bar,
        "dock_tabs": app.ui.dock_tabs,
        "community_links": app.ui.community_links,
        "quit_menu": app.ui.quit_menu,
        "transport": app.ui.show_transport,
        "tracks_list": app.ui.show_tracks_list,
        "clip_list": app.ui.show_clip_list,
        "window": match app.ui.window { MainWindow::Edit => "edit", MainWindow::Mix => "mix" },
    })
}

fn panels(app: &mut SoundApp, fields: &Map<String, Value>) -> Value {
    let (mut applied, mut unknown, mut invalid) = (Vec::new(), Vec::new(), Vec::new());
    let mut patch = Map::new();
    let mut via_ui_set = Vec::new();
    for (key, value) in fields {
        if key == "window" {
            match value.as_str().map(str::to_ascii_lowercase).as_deref() {
                Some("edit") => patch.insert("window".into(), json!("Edit")),
                Some("mix") => patch.insert("window".into(), json!("Mix")),
                _ => {
                    invalid.push(key.clone());
                    continue;
                }
            };
            via_ui_set.push(key.clone());
            continue;
        }
        if !SWITCHES.contains(&key.as_str()) {
            unknown.push(key.clone());
            continue;
        }
        let Some(on) = value.as_bool() else {
            invalid.push(key.clone());
            continue;
        };
        if let Some(field) = ui_field(key) {
            patch.insert(field.into(), json!(on));
            via_ui_set.push(key.clone());
            continue;
        }
        match key.as_str() {
            "menu_bar" => app.native_menu_bar = !on,
            "edit_toolbar" => app.ui.edit_toolbar = on,
            "status_bar" => app.ui.status_bar = on,
            "dock_tabs" => app.ui.dock_tabs = on,
            "community_links" => app.ui.community_links = on,
            _ => app.ui.quit_menu = on,
        }
        applied.push(key.clone());
    }
    // The upstream switches go through `ui.set`, which validates the docking state they touch.
    if !patch.is_empty() {
        match app.run("ui.set", Value::Object(patch)) {
            Ok(_) => applied.append(&mut via_ui_set),
            Err(_) => invalid.append(&mut via_ui_set),
        }
    }
    json!({"applied": applied, "unknown": unknown, "invalid": invalid})
}

/// `c0.set {themeTokens?, panels?}`. Each part answers `{applied, unknown, invalid}` by name; a bad
/// name or value in one changes nothing for that name and does not stop the others.
pub fn set(app: &mut SoundApp, ctx: &egui::Context, p: &Value) -> Result<Value, String> {
    let fields = p.as_object().ok_or("params must be an object")?;
    if let Some(key) = fields.keys().find(|k| !matches!(k.as_str(), "themeTokens" | "panels")) {
        return Err(format!("unknown field `{key}`: c0.set takes `themeTokens` and `panels`"));
    }
    let mut out = Map::new();
    if let Some(tokens) = fields.get("themeTokens") {
        let report = crate::theme::host::set(tokens)?;
        // The preset's egui visuals are built from the tokens, so build them again.
        crate::theme::apply(ctx, crate::theme::is_light());
        out.insert("themeTokens".into(), report.to_json());
    }
    if let Some(value) = fields.get("panels") {
        let map = value.as_object().ok_or("`panels` must be an object of panel key to value")?;
        out.insert("panels".into(), panels(app, map));
    }
    Ok(Value::Object(out))
}
