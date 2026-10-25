//! `c0.inspect` and `c0.goTo`: what the host reads of the session, and where it moves the playhead.

use crate::SoundApp;
use serde_json::{Value, json};
use soundcraft_model::TrackKind;

/// The most clips `c0.inspect` lists; `clipsTruncated` says so when there are more.
pub const MAX_CLIPS: usize = 5000;

fn kind_name(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Audio => "audio",
        TrackKind::Aux => "aux",
        TrackKind::Master => "master",
        TrackKind::Midi => "midi",
        TrackKind::Instrument => "instrument",
        TrackKind::Vca => "vca",
        TrackKind::Folder => "folder",
        TrackKind::Video => "video",
    }
}

/// `c0.inspect`: the session, the selection, the window and the chrome switches.
pub fn inspect(app: &SoundApp) -> Value {
    let e = &app.engine;
    let s = e.session();
    let selected = &s.edit.selected_tracks;
    let tracks: Vec<Value> = s
        .tracks
        .iter()
        .map(|t| {
            json!({"id": t.id.0, "name": t.name, "kind": kind_name(t.kind), "muted": t.mixer.mute, "solo": t.mixer.solo, "selected": selected.contains(&t.id)})
        })
        .collect();
    let mut clips = Vec::new();
    let mut total = 0usize;
    for t in &s.tracks {
        for c in t.clips() {
            total += 1;
            if clips.len() < MAX_CLIPS {
                clips.push(json!({"id": c.id.0, "name": c.name, "track": t.id.0, "start": c.start, "end": c.end()}));
            }
        }
    }
    let markers: Vec<Value> = s.markers.iter().map(|m| json!({"id": m.id.0, "name": m.name, "at": m.start})).collect();
    json!({
        "title": s.name,
        "dirty": e.is_dirty(),
        "revision": e.revision,
        "canUndo": e.can_undo(),
        "canRedo": e.can_redo(),
        "playing": app.is_playing(),
        "playheadSamples": app.position(),
        "sampleRate": s.sample_rate.hz(),
        "tempo": s.tempo.tempo_at_tick(0),
        "tracks": tracks,
        "clips": clips,
        "clipsTruncated": total > clips.len(),
        "markers": markers,
        "selection": {
            "start": s.edit.selection.start, "end": s.edit.selection.end,
            "tracks": s.edit.selected_tracks.iter().map(|t| t.0).collect::<Vec<_>>(),
            "clips": s.edit.selected_clips.iter().map(|c| c.0).collect::<Vec<_>>(),
        },
        "window": match app.ui.window { crate::MainWindow::Edit => "edit", crate::MainWindow::Mix => "mix" },
        "panels": super::panels::state(app),
        "theme": {
            "mode": app.ui.theme.id(),
            "light": crate::theme::is_light(),
            "overrides": crate::theme::host::active(),
        },
    })
}

/// `c0.goTo {marker | samples | track}`: moves the playhead to a marker or a position, or selects
/// a track (the Edit window scrolls it into view).
pub fn go_to(app: &mut SoundApp, p: &Value) -> Result<Value, String> {
    let given: Vec<&str> = ["marker", "samples", "track"].into_iter().filter(|k| p.get(*k).is_some_and(|v| !v.is_null())).collect();
    match given.as_slice() {
        ["marker"] => {
            let key = &p["marker"];
            let found = app.engine.session().markers.iter().find(|m| match (key.as_u64(), key.as_str()) {
                (Some(id), _) => m.id.0 == id,
                (_, Some(name)) => m.name == name,
                _ => false,
            });
            let Some(marker) = found else { return Err(format!("no marker {key}: give a marker id or name from `c0.inspect`")) };
            let (id, at) = (marker.id.0, marker.start);
            app.run("transport.locate", json!({"at": at}))?;
            Ok(json!({"playhead": app.position(), "marker": id}))
        }
        ["samples"] => {
            let v = &p["samples"];
            let at = v.as_i64().or_else(|| v.as_f64().filter(|f| f.is_finite() && f.fract() == 0.0).map(|f| f as i64));
            let Some(at) = at.filter(|a| *a >= 0) else { return Err("`samples` must be a whole number of samples, 0 or more".into()) };
            app.run("transport.locate", json!({"at": at}))?;
            Ok(json!({"playhead": app.position()}))
        }
        ["track"] => {
            let key = &p["track"];
            if !key.is_u64() && !key.is_string() {
                return Err("`track` must be a track id or name".into());
            }
            let done = app.run("track.scroll_to", json!({"track": key}))?;
            Ok(json!({"playhead": app.position(), "track": done["track"]}))
        }
        [] => Err("give one of `marker`, `samples` or `track`".into()),
        _ => Err("go to one place at a time: give only one of `marker`, `samples` or `track`".into()),
    }
}
