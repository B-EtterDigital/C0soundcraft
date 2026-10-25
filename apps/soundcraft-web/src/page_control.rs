//! The control protocol for a page that embeds SoundCraft (cargo feature `page-control`).
//!
//! The desktop app serves its control channel over a local socket; a browser cannot listen on one.
//! With this feature the same requests reach the app through one function on the page:
//!
//! ```js
//! const reply = JSON.parse(await window.__soundcraftControl(JSON.stringify({ method, params })));
//! ```
//!
//! Requests and replies are the desktop protocol's (`engine.execute`, `session.inspect`, `ui.*`),
//! handled by the same `control::drain` through
//! [`SoundApp::with_control`](soundcraft_ui_egui::SoundApp::with_control), plus the host's `c0.*`
//! requests (`soundcraft_ui_egui::c0_host`, which a native test runs). Every reply is
//! `{"ok":true,"result":...}` or `{"ok":false,"error":"..."}`. Off by default: a plain web build
//! exposes nothing to the page.
//!
//! ## When a request is answered
//!
//! The app handles control requests at the start of a frame, and the browser decides when frames
//! happen: every 16 ms while the window is on screen, about once a second while it is not being
//! presented. A request therefore runs one egui pass of its own, at once, inside the call: the same
//! pass a frame runs (the queue is drained first, then the whole interface is laid out), only it is
//! not painted. The reply is ready when the call returns, whatever the display does.
//!
//! - A request that only reads ([`reads_only`]) asks for no frame at all.
//! - Any other request asks for one frame afterwards, so the change is seen.
//! - A request of scripted input ([`is_synthetic_input`]) is played one batch per pass and answers
//!   after the last; those passes run inside the same call, back to back, up to [`inline_passes`].
//! - A request still waiting gets passes at up to [`PUMP_MS`] apart, again without the display.
//! - `ui.screenshot` reads pixels the display painted, so it waits for painted frames.
//! - Before the app has started, or if a pass cannot be run, the request falls back to waking the
//!   app and waiting for its next frame.
//!
//! A file the app would hand to the person as a download (a save, a bounce) is dropped while a
//! `c0.*` request runs: the host asks for bytes with `c0.documentBytes` and `c0.selection`, and a
//! stray download would be a surprise. Saves and exports the person starts still download.

use serde_json::Value;

#[cfg(target_arch = "wasm32")]
mod bridge;
#[cfg(target_arch = "wasm32")]
pub use bridge::{attach, install_deliver, runner};

/// The least time between two passes run for a request that is still waiting, in milliseconds: a
/// display frame, so a waiting request costs no more passes than a window on screen would.
pub const PUMP_MS: f64 = 16.0;

/// The largest ordinary request.
pub const MAX_REQUEST_BYTES: usize = 1024 * 1024;
/// The largest `c0.*` request: it carries a file or audio as base64 (a session of 72 MiB is 96 MiB).
pub const C0_MAX_REQUEST_BYTES: usize = 96 * 1024 * 1024;

/// The most bytes a request of `method` may be.
pub fn request_limit(method: &str) -> usize {
    if method.starts_with("c0.") { C0_MAX_REQUEST_BYTES } else { MAX_REQUEST_BYTES }
}

/// `engine.execute` commands that only read.
const READ_COMMANDS: &[&str] = &["session.inspect", "document.inspect", "engine.commands"];

/// Whether a request only reads state. Answering one changes nothing on screen, so no frame is
/// asked for on its behalf. Anything not listed here is taken to change something.
pub fn reads_only(method: &str, params: &Value) -> bool {
    match method {
        "ui.inspect" | "session.inspect" | "document.inspect" | "ui.menu.list" | "engine.commands" | "engine.parity" | "c0.inspect" | "c0.documentBytes"
        | "c0.selection" => true,
        "engine.execute" | "command" => params.get("command").and_then(Value::as_str).is_some_and(|command| READ_COMMANDS.contains(&command)),
        _ => false,
    }
}

/// The control requests that queue scripted input: the app plays them one batch per pass and
/// answers after the last, so their reply is never ready after the first pass.
pub const SYNTHETIC_INPUT: &[&str] = &["ui.click", "ui.move", "ui.drag", "ui.scroll", "ui.zoom", "ui.key", "ui.text"];

/// The most passes run inside one call for a scripted input request; what is left is played while
/// the request waits ([`pump_due`]), so a drag of 200 steps does not stall the page.
pub const MAX_INLINE_PASSES: usize = 32;

/// Whether `method` is a scripted input request.
pub fn is_synthetic_input(method: &str) -> bool {
    SYNTHETIC_INPUT.contains(&method)
}

/// The passes to run inside the call after the one that read the request: enough for its batches,
/// at most [`MAX_INLINE_PASSES`]; the loop stops at the reply, so too many cost nothing. None for
/// any other request.
pub fn inline_passes(method: &str, _params: &Value) -> usize {
    if is_synthetic_input(method) { MAX_INLINE_PASSES } else { 0 }
}

/// Whether a request that is still waiting gets another pass now: the app asked for a repaint and
/// the last pass was at least [`PUMP_MS`] ago.
pub fn pump_due(wants_repaint: bool, now_ms: f64, last_pass_ms: f64) -> bool {
    wants_repaint && now_ms - last_pass_ms >= PUMP_MS
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn reading_requests_ask_for_no_frame() {
        for (method, params) in [
            ("ui.inspect", json!({})),
            ("session.inspect", json!({})),
            ("ui.menu.list", json!({})),
            ("engine.commands", json!({})),
            ("engine.execute", json!({"command": "session.inspect"})),
            ("c0.inspect", json!({})),
            ("c0.documentBytes", json!({})),
            ("c0.selection", json!({"maxSeconds": 10})),
        ] {
            assert!(reads_only(method, &params), "{method} {params}");
        }
    }

    #[test]
    fn c0_requests_may_carry_files_and_others_may_not() {
        assert_eq!(request_limit("c0.openFile"), C0_MAX_REQUEST_BYTES);
        assert_eq!(request_limit("c0.place"), C0_MAX_REQUEST_BYTES);
        for method in ["ui.set", "engine.execute", "c0", "xc0.place", ""] {
            assert_eq!(request_limit(method), MAX_REQUEST_BYTES, "{method}");
        }
        assert!(C0_MAX_REQUEST_BYTES > 64 * 1024 * 1024, "room for a 48 MiB file as base64");
    }

    /// What changes the session, the dialogs or the chrome asks for a frame.
    #[test]
    fn c0_requests_that_change_something_ask_for_a_frame() {
        for method in ["c0.goTo", "c0.openFile", "c0.newDocument", "c0.cancelFileDialog", "c0.set", "c0.place"] {
            assert!(!reads_only(method, &json!({})), "{method}");
        }
    }

    #[test]
    fn every_host_method_is_classified() {
        // The same nine as `soundcraft_ui_egui::c0_host::METHODS` (the contract test there compares).
        for method in [
            "c0.inspect", "c0.goTo", "c0.documentBytes", "c0.openFile", "c0.newDocument", "c0.cancelFileDialog", "c0.set", "c0.selection", "c0.place",
        ] {
            let reads = matches!(method, "c0.inspect" | "c0.documentBytes" | "c0.selection");
            assert_eq!(reads_only(method, &json!({})), reads, "{method}");
            assert_eq!(request_limit(method), C0_MAX_REQUEST_BYTES, "{method}");
        }
    }

    #[test]
    fn everything_else_asks_for_a_frame() {
        for (method, params) in [
            ("ui.set", json!({"window": "Mix"})),
            ("ui.menu.invoke", json!({"id": "session.new"})),
            ("ui.click", json!({"x": 1, "y": 1})),
            ("ui.screenshot", json!({})),
            ("engine.execute", json!({"command": "edit.undo"})),
            ("engine.execute", json!({})),
            ("engine.execute", json!({"command": 7})),
            ("nope", json!({})),
        ] {
            assert!(!reads_only(method, &params), "{method} {params}");
        }
    }

    /// A scripted input request is not through after its first pass, so the bridge must give it
    /// passes inside the call. Every method that queues input is in the rule; nothing else is.
    #[test]
    fn every_scripted_input_method_gets_inline_passes() {
        for method in SYNTHETIC_INPUT {
            assert!(is_synthetic_input(method), "{method}");
            assert!(inline_passes(method, &json!({})) >= 2, "{method}");
            assert!(!reads_only(method, &json!({})), "{method} changes the screen: it asks for a frame");
        }
        for method in ["ui.inspect", "ui.set", "ui.menu.invoke", "ui.screenshot", "engine.execute", "c0.place", "ui.clicks", ""] {
            assert!(!is_synthetic_input(method), "{method}");
            assert_eq!(inline_passes(method, &json!({"count": 9})), 0, "{method}");
        }
    }

    #[test]
    fn a_waiting_request_is_pumped_no_faster_than_the_display() {
        assert!(pump_due(true, 1016.0, 1000.0));
        assert!(pump_due(true, 5000.0, 1000.0));
        assert!(!pump_due(true, 1015.9, 1000.0), "not twice within one display frame");
        assert!(!pump_due(false, 5000.0, 1000.0), "not when the app is idle");
    }

    /// The bridge leans on these in the web runner; a pull that renames them must fail here, not
    /// in the browser.
    #[test]
    fn the_web_shell_hands_its_runner_to_the_bridge() {
        let web = include_str!("web.rs");
        assert!(web.contains("crate::page_control::runner(&runner);"), "web.rs no longer gives the bridge the runner: requests wait for frames again");
        assert!(web.contains("crate::page_control::attach(app, &cc.egui_ctx)"), "web.rs no longer attaches the bridge");
        assert!(web.contains("crate::page_control::install_deliver(download);"), "web.rs no longer installs the delivery hook");
    }
}
