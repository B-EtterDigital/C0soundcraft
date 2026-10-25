//! The C0VIBE host's requests (`c0.*`), answered by the app on the UI thread.
//!
//! Not upstream's: part of the C0VIBE guest page (`c0/soundcraft-host/README.md`). Every edit goes
//! through the engine's own paths (import, session open, undo), so history, dirty state and saving
//! behave as for a user edit.
//!
//! | Method | Params | Result |
//! |---|---|---|
//! | `c0.inspect` | `{}` | the open session, selection and chrome (see [`inspect`]) |
//! | `c0.goTo` | exactly one of `{marker}` (id or name), `{samples}`, `{track}` (id or name) | `{playhead, ...}` |
//! | `c0.documentBytes` | `{}` | `{name, format: "scraftz", bytes (base64), revision}` |
//! | `c0.openFile` | `{name, bytes (base64)}` | `{opened, kind: "session" or "audio" or "midi", ...}` |
//! | `c0.newDocument` | `{}` | `{created, pending?}` |
//! | `c0.cancelFileDialog` | `{}` | `{cancelled}` |
//! | `c0.set` | `{themeTokens?, panels?}` | `{themeTokens?: {applied, unknown, invalid}, panels?: {...}}` |
//! | `c0.selection` | `{maxSeconds?}` | `{kind: "audio", sampleRate, start, end, tracks, clips, wav}` |
//! | `c0.place` | `{wav or mp3 or flac or ogg (base64), name, track?, at?}` | `{placed: "audio", track, clip, source, start, end}` |

use crate::SoundApp;
use serde_json::{Value, json};

pub mod ai;
pub mod b64;
pub mod files;
pub mod inspect;
pub mod panels;

/// Every method this module answers.
pub const METHODS: [&str; 9] =
    ["c0.inspect", "c0.goTo", "c0.documentBytes", "c0.openFile", "c0.newDocument", "c0.cancelFileDialog", "c0.set", "c0.selection", "c0.place"];

/// Answers one `c0.*` request with the control channel's reply: `{"ok": true, "result": ...}` or
/// `{"ok": false, "error": ...}`.
pub fn handle(app: &mut SoundApp, ctx: &egui::Context, method: &str, p: &Value) -> Value {
    let result = match method {
        "c0.inspect" => Ok(inspect::inspect(app)),
        "c0.goTo" => inspect::go_to(app, p),
        "c0.documentBytes" => files::document_bytes(app),
        "c0.openFile" => files::open_file(app, p),
        "c0.newDocument" => files::new_document(app),
        "c0.cancelFileDialog" => Ok(files::cancel_file_dialog(app)),
        "c0.set" => panels::set(app, ctx, p),
        "c0.selection" => ai::selection(app, p),
        "c0.place" => ai::place(app, p),
        other => Err(format!("unknown method `{other}`")),
    };
    match result {
        Ok(result) => json!({"ok": true, "result": result}),
        Err(error) => json!({"ok": false, "error": error}),
    }
}
