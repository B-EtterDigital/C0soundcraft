//! `c0.documentBytes`, `c0.openFile`, `c0.newDocument` and `c0.cancelFileDialog`: the file traffic
//! between the host and the open session. No dialog is ever shown for any of them.

use super::b64;
use crate::SoundApp;
use crate::dialogs::Dialog;
use serde_json::{Value, json};
use soundcraft_engine::io;
use soundcraft_model::ClipId;

/// The most bytes `c0.openFile` takes, decoded.
pub const MAX_FILE_BYTES: usize = 96 * 1024 * 1024;

/// The command the unsaved-changes question carries on with to open the file (`dialogs.rs`).
const OPEN_PENDING: &str = "ui.open_pending_session";

const SESSION_EXTS: &[&str] = &[io::BUNDLE_EXTENSION, soundcraft_model::SESSION_EXTENSION];
const MIDI_EXTS: &[&str] = &["mid", "midi", "smf"];

/// The part of a name that is safe as a file name.
fn file_stem(name: &str) -> String {
    let clean: String = name.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c }).collect();
    let clean = clean.trim().trim_matches('.').to_owned();
    if clean.is_empty() { "Untitled".into() } else { clean }
}

/// `c0.documentBytes`: the open session as one `.scraftz` bundle, read only.
pub fn document_bytes(app: &SoundApp) -> Result<Value, String> {
    let (bytes, _) = io::session_bundle(app.engine.session()).map_err(|e| e.to_string())?;
    Ok(json!({
        "name": format!("{}.{}", file_stem(&app.engine.session().name), io::BUNDLE_EXTENSION),
        "format": io::BUNDLE_EXTENSION,
        "bytes": crate::control::base64(&bytes),
        "revision": app.engine.revision,
    }))
}

fn extension(name: &str) -> String {
    name.rsplit_once('.').map(|(_, x)| x.to_ascii_lowercase()).unwrap_or_default()
}

/// `c0.openFile {name, bytes}`: a `.scraftz` or `.scraft` opens as the session (through the app's
/// unsaved-changes question when the open session has changes); a MIDI file or any audio file the
/// importer reads goes onto a new track at the start, as one undo step.
pub fn open_file(app: &mut SoundApp, p: &Value) -> Result<Value, String> {
    let name = p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).ok_or("`name` must be the file name, with its extension")?;
    let text = p.get("bytes").and_then(Value::as_str).ok_or("`bytes` must be the file as base64 text")?;
    if text.len() / 4 * 3 > MAX_FILE_BYTES + 3 {
        return Err(format!("the file is larger than {} MiB", MAX_FILE_BYTES >> 20));
    }
    let bytes = b64::decode(text).ok_or("`bytes` is not valid base64")?;
    if bytes.is_empty() {
        return Err("the file is empty".into());
    }
    let ext = extension(name);
    if SESSION_EXTS.contains(&ext.as_str()) {
        crate::dialogs::open_session_bytes(app, name.to_owned(), bytes);
        if waiting_to_open(app) {
            return Ok(json!({"opened": false, "kind": "session", "name": name, "pending": "unsaved-changes"}));
        }
        let status = app.ui.status.clone();
        return if status.starts_with("Opened") { Ok(json!({"opened": true, "kind": "session", "name": name, "status": status})) } else { Err(status) };
    }
    let kind = if MIDI_EXTS.contains(&ext.as_str()) { "midi" } else { "audio" };
    let e = &mut app.engine;
    let before = e.session_arc();
    let placed = if kind == "midi" {
        let tracks = io::import_midi_bytes(e, &bytes, 0, true).map_err(|err| format!("{name}: {err}"))?;
        json!({"tracks": tracks})
    } else {
        io::import_audio_bytes(e, name, &bytes, None, None, 0).map_err(|err| format!("{name}: {err}"))?
    };
    e.push_undo(if kind == "midi" { "Import MIDI" } else { "Import Audio" }, before);
    app.ui.status = format!("Imported {name}");
    let mut out = json!({"opened": true, "kind": kind, "name": name, "start": 0});
    if let Some(clip) = placed.get("clip").and_then(Value::as_u64)
        && let Some((track, c)) = app.engine.session().find_clip(ClipId(clip))
    {
        out["track"] = json!(track.0);
        out["clip"] = json!(clip);
        out["end"] = json!(c.end());
    } else {
        out["imported"] = placed;
    }
    Ok(out)
}

/// `c0.newDocument`: an empty session, no demo. Asks first when the open session has changes.
pub fn new_document(app: &mut SoundApp) -> Result<Value, String> {
    crate::dialogs::run_or_ask(app, "session.new", json!({"name": "Untitled"}))?;
    if matches!(app.dialogs.open, Some(Dialog::SaveChanges { .. })) {
        return Ok(json!({"created": false, "pending": "unsaved-changes"}));
    }
    Ok(json!({"created": true, "title": app.engine.session().name}))
}

/// Whether the app is asking about unsaved changes before it opens a file the browser read (the
/// question `dialogs::open_session_bytes` asks; its command id is upstream's `ui.open_pending_session`).
fn waiting_to_open(app: &SoundApp) -> bool {
    matches!(&app.dialogs.open, Some(Dialog::SaveChanges { then }) if then.0 == OPEN_PENDING)
}

/// `c0.cancelFileDialog`: dismisses an open path prompt, and the question that waits to open a file.
pub fn cancel_file_dialog(app: &mut SoundApp) -> Value {
    let cancelled = matches!(app.dialogs.open, Some(Dialog::PathPrompt { .. })) || waiting_to_open(app);
    if cancelled {
        app.dialogs.open = None;
    }
    json!({"cancelled": cancelled})
}
