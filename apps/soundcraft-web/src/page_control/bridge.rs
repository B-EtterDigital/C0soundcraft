//! The browser half of the page bridge: the page function, the request channel into the app, and
//! the passes that answer a request without waiting for the display (see the parent module).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use serde_json::{Value, json};
use soundcraft_engine::io::{self, DeliverHook};
use soundcraft_ui_egui::{ControlRequest, SoundApp};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::prelude::*;

/// The function the page calls.
pub const PAGE_FUNCTION: &str = "__soundcraftControl";
/// How often a waiting request checks for its reply.
const POLL_MS: i32 = 4;
/// The event a pass is asked for with; it never leaves [`Pump::target`].
const PUMP_EVENT: &str = "soundcraft-control-pass";

thread_local! {
    /// The web runner, given by `web.rs` before it starts the app ([`runner`]).
    static RUNNER: RefCell<Option<eframe::WebRunner>> = const { RefCell::new(None) };
    /// The pump, made on the first request (eframe drops every listener when it starts the app).
    static PUMP: RefCell<Option<Pump>> = const { RefCell::new(None) };
    /// Set while a `c0.*` request runs: downloads are dropped then ([`deliver`]).
    static QUIET: Cell<bool> = const { Cell::new(false) };
}

/// Where a download goes when nothing is quiet: the web shell's own `download`.
static FORWARD: OnceLock<DeliverHook> = OnceLock::new();

/// The engine's delivery hook: hands a saved or exported file to the person, except while a host
/// request runs (that one asks for bytes through the page function).
fn deliver(name: &str, bytes: &[u8]) -> Result<(), String> {
    if QUIET.with(Cell::get) {
        log::info!("page-control: dropped the download of {name} ({} bytes) during a host request", bytes.len());
        return Ok(());
    }
    match FORWARD.get() {
        Some(download) => download(name, bytes),
        None => Err("saving files is not available in this build".into()),
    }
}

/// Installs the delivery hook in place of `download` (call before the shell would set its own).
pub fn install_deliver(download: DeliverHook) {
    let _ = FORWARD.set(download);
    io::set_deliver(deliver);
}

/// Marks a `c0.*` request as running for as long as it lives.
struct Quiet(bool);

impl Quiet {
    fn on(active: bool) -> Self {
        Quiet(QUIET.with(|q| q.replace(active)))
    }
}

impl Drop for Quiet {
    fn drop(&mut self) {
        QUIET.with(|q| q.set(self.0));
    }
}

/// Gives the bridge the web runner the app is about to start in. Without it every request waits
/// for the display's next frame.
pub fn runner(runner: &eframe::WebRunner) {
    RUNNER.with(|slot| *slot.borrow_mut() = Some(runner.clone()));
}

/// Runs egui passes on request instead of on the display's schedule.
///
/// eframe lends its app runner only to event listeners it registered itself, so a pass is asked for
/// by sending an event to a private target: eframe's listener runs at once, inside `dispatch_event`,
/// and calls the same `logic()` a frame calls. It does not paint; eframe paints what a pass laid
/// out with the next frame it is asked for.
#[derive(Clone)]
struct Pump {
    target: web_sys::EventTarget,
    /// Set by the listener: `dispatch_event` returning says nothing about whether the pass ran
    /// (eframe skips the listener after a panic, before the app exists, and inside a frame).
    ran: Rc<Cell<bool>>,
    /// When the last pass ran (`Date.now()`).
    last: Rc<Cell<f64>>,
}

impl Pump {
    fn get() -> Option<Self> {
        if let Some(pump) = PUMP.with(|slot| slot.borrow().clone()) {
            return Some(pump);
        }
        let runner = RUNNER.with(|slot| slot.borrow().clone())?;
        let pump = Self { target: web_sys::EventTarget::new().ok()?, ran: Rc::default(), last: Rc::default() };
        let ran = pump.ran.clone();
        runner
            .add_event_listener::<web_sys::Event>(&pump.target, PUMP_EVENT, move |_event, app_runner| {
                app_runner.logic();
                ran.set(true);
            })
            .ok()?;
        PUMP.with(|slot| *slot.borrow_mut() = Some(pump.clone()));
        Some(pump)
    }

    /// Runs one pass now. `false` when it could not run; the caller then wakes the app instead.
    fn pass(&self) -> bool {
        self.ran.set(false);
        let Ok(event) = web_sys::Event::new(PUMP_EVENT) else { return false };
        let _ = self.target.dispatch_event(&event);
        if self.ran.get() {
            self.last.set(js_sys::Date::now());
        }
        self.ran.get()
    }

    /// Runs passes back to back, at most `passes` of them, until `rx` has the reply, and returns it.
    /// `None` when the reply is not there yet or a pass could not run; the caller then waits as usual.
    fn drive(&self, rx: &Receiver<Value>, passes: usize) -> Option<Value> {
        for _ in 0..passes {
            match rx.try_recv() {
                Ok(value) => return Some(value),
                Err(TryRecvError::Disconnected) => return None,
                Err(TryRecvError::Empty) => {
                    if !self.pass() {
                        return None;
                    }
                }
            }
        }
        rx.try_recv().ok()
    }

    /// Another pass for a request that is still waiting, when the app asked for one and a display
    /// frame's time has gone by.
    fn keep_going(&self, ctx: &egui::Context) {
        if super::pump_due(ctx.has_requested_repaint(), js_sys::Date::now(), self.last.get()) {
            self.pass();
        }
    }
}

/// Wires the page function to `app`'s control channel.
pub fn attach(app: SoundApp, ctx: &egui::Context) -> SoundApp {
    let (tx, rx) = channel::<ControlRequest>();
    install(tx, ctx.clone());
    app.with_control(rx)
}

fn install(tx: Sender<ControlRequest>, ctx: egui::Context) {
    let Some(window) = web_sys::window() else {
        log::error!("page-control: no window");
        return;
    };
    let call = Closure::<dyn FnMut(JsValue) -> js_sys::Promise>::new(move |text: JsValue| {
        let reply = parse(&text).and_then(|(method, params)| {
            let reads_only = super::reads_only(&method, &params);
            let inline = super::inline_passes(&method, &params);
            let host = method.starts_with("c0.");
            Ok((submit(&tx, method, params)?, reads_only, inline, host))
        });
        match reply {
            Ok((rx, reads_only, inline, host)) => {
                let _quiet = Quiet::on(host);
                // Answer now, in a pass of our own, instead of on the display's next frame.
                let pump = Pump::get();
                if !pump.as_ref().is_some_and(Pump::pass) {
                    // No pass could be run: wake the app and wait for its frame.
                    ctx.request_repaint();
                } else if !reads_only {
                    // Show the change. A delayed request is one frame; an immediate one is two.
                    ctx.request_repaint_after(std::time::Duration::from_millis(1));
                }
                // Scripted input: play its batches now, in passes of our own, not on a timer.
                let early = pump.as_ref().filter(|_| inline > 0).and_then(|pump| pump.drive(&rx, inline));
                wasm_bindgen_futures::future_to_promise(wait(rx, ctx.clone(), pump, early))
            }
            Err(error) => js_sys::Promise::resolve(&JsValue::from_str(&encode(&json!({"ok": false, "error": error})))),
        }
    });
    if let Err(error) = js_sys::Reflect::set(&window, &JsValue::from_str(PAGE_FUNCTION), call.as_ref().unchecked_ref()) {
        log::error!("page-control: could not install {PAGE_FUNCTION}: {error:?}");
        return;
    }
    // The page keeps the function for the life of the app.
    call.forget();
}

fn parse(text: &JsValue) -> Result<(String, Value), String> {
    let Some(text) = text.as_string() else { return Err("the request must be JSON text".into()) };
    if text.len() > super::C0_MAX_REQUEST_BYTES {
        return Err(format!("the request is larger than {} MiB", super::C0_MAX_REQUEST_BYTES >> 20));
    }
    let value: Value = serde_json::from_str(&text).map_err(|e| format!("the request is not JSON: {e}"))?;
    let Some(method) = value.get("method").and_then(Value::as_str) else { return Err("the request has no method".into()) };
    if text.len() > super::request_limit(method) {
        return Err("the request is larger than 1 MiB".into());
    }
    Ok((method.to_owned(), value.get("params").cloned().unwrap_or(Value::Null)))
}

fn submit(tx: &Sender<ControlRequest>, method: String, params: Value) -> Result<Receiver<Value>, String> {
    let (request, rx) = ControlRequest::new(method, params);
    tx.send(request).map_err(|_| "SoundCraft has stopped".to_owned())?;
    Ok(rx)
}

async fn wait(rx: Receiver<Value>, ctx: egui::Context, pump: Option<Pump>, early: Option<Value>) -> Result<JsValue, JsValue> {
    if let Some(value) = early {
        return Ok(JsValue::from_str(&encode(&value)));
    }
    loop {
        match rx.try_recv() {
            Ok(value) => return Ok(JsValue::from_str(&encode(&value))),
            Err(TryRecvError::Disconnected) => {
                return Ok(JsValue::from_str(&encode(&json!({"ok": false, "error": "SoundCraft dropped the request"}))));
            }
            Err(TryRecvError::Empty) => {
                sleep(POLL_MS).await?;
                // Not answered by its first pass (scripted input, a job): keep the passes coming.
                if let Some(pump) = &pump {
                    pump.keep_going(&ctx);
                }
            }
        }
    }
}

async fn sleep(ms: i32) -> Result<(), JsValue> {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        if let Some(window) = web_sys::window() {
            // A failed timer resolves at once; the loop then asks again.
            if window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms).is_err() {
                let _ = resolve.call0(&JsValue::NULL);
            }
        } else {
            let _ = resolve.call0(&JsValue::NULL);
        }
    });
    wasm_bindgen_futures::JsFuture::from(promise).await.map(|_| ())
}

fn encode(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| r#"{"ok":false,"error":"the reply could not be encoded"}"#.to_owned())
}
