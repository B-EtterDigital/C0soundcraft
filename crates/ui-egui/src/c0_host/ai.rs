//! `c0.selection` and `c0.place`: what the C0VIBE AI actions read from the session and put back
//! (speech, music, sound effects, transcription, voice isolation, stems).
//!
//! `c0.selection` renders the time selection to a WAV; `c0.place` imports audio onto a track as ONE
//! undo step.
//!
//! | Method | Params | Result |
//! |---|---|---|
//! | `c0.selection` | `{maxSeconds?: 300}` | `{kind:"audio", sampleRate, start, end, tracks, clips, channels, wav}`; `empty:true` and no `wav` without a time selection |
//! | `c0.place` | `{wav\|mp3\|flac\|ogg: base64, name?, track?: id\|"new", at?: samples}` | `{placed:"audio", track, clip, source, start, end}` |

use super::b64;
use crate::SoundApp;
use serde_json::{Value, json};
use soundcraft_audio_io::{AudioBuffer, BitDepth, EncodeOptions, FileFormat};
use soundcraft_model::{Session, TrackId, TrackKind};
use soundcraft_time::Range;

/// The default and the largest `maxSeconds`.
pub const DEFAULT_SECONDS: f64 = 300.0;
pub const MAX_SECONDS: f64 = 600.0;
/// The most bytes of audio `c0.place` takes, decoded.
pub const MAX_PLACE_BYTES: usize = 60 * 1024 * 1024;
/// The formats `c0.place` reads, by the key that carries them.
pub const PLACE_FORMATS: [&str; 4] = ["wav", "mp3", "flac", "ogg"];

/// The session as it sounds with only `tracks` playing: the others are muted and nobody solos, so
/// the render keeps inserts, faders, pans and busses of the tracks that stay.
fn only_tracks(session: &Session, tracks: &[TrackId]) -> Session {
    let mut s = session.clone();
    for t in &mut s.tracks {
        t.mixer.solo = false;
        if matches!(t.kind, TrackKind::Audio | TrackKind::Midi | TrackKind::Instrument | TrackKind::Video) && !tracks.contains(&t.id) {
            t.mixer.mute = true;
        }
    }
    s
}

/// `c0.selection`: the time selection, rendered from the selected tracks (the whole mix when no track
/// is selected) as a 16-bit WAV, at most `maxSeconds` long.
pub fn selection(app: &SoundApp, p: &Value) -> Result<Value, String> {
    let max_seconds = match p.get("maxSeconds") {
        None | Some(Value::Null) => DEFAULT_SECONDS,
        Some(v) => v.as_f64().filter(|s| s.is_finite() && *s > 0.0).ok_or("`maxSeconds` must be a positive number of seconds")?.min(MAX_SECONDS),
    };
    let s = app.engine.session();
    let rate = s.sample_rate.hz();
    let sel = s.edit.selection;
    let mut out = json!({
        "kind": "audio", "sampleRate": rate, "start": sel.start, "end": sel.end,
        "tracks": s.edit.selected_tracks.iter().map(|t| t.0).collect::<Vec<_>>(),
        "clips": s.edit.selected_clips.iter().map(|c| c.0).collect::<Vec<_>>(),
    });
    if sel.is_empty() || sel.start < 0 {
        out["empty"] = json!(true);
        return Ok(out);
    }
    let cap = (max_seconds * f64::from(rate)) as i64;
    let end = sel.end.min(sel.start.saturating_add(cap));
    if end < sel.end {
        out["end"] = json!(end);
        out["truncated"] = json!(true);
    }
    let range = Range::new(sel.start, end);
    let channels = if s.edit.selected_tracks.is_empty() {
        soundcraft_mix::render_range(s, range, 1024)
    } else {
        soundcraft_mix::render_range(&only_tracks(s, &s.edit.selected_tracks), range, 1024)
    };
    let buffer = AudioBuffer { sample_rate: rate, channels };
    if buffer.frames() == 0 {
        return Err("the selection rendered no audio".into());
    }
    let opts = EncodeOptions { format: FileFormat::Wav, bit_depth: BitDepth::Int16, dither: false, bwf: None };
    let wav = soundcraft_audio_io::encode(&buffer, &opts).map_err(|e| format!("the selection could not be encoded: {e}"))?;
    out["channels"] = json!(buffer.num_channels());
    out["wav"] = json!(crate::control::base64(&wav));
    Ok(out)
}

/// `c0.place`: audio onto a track (a new one unless `track` names an audio track) at `at` samples
/// (the playhead by default), as one undo step. Nothing changes when it errors.
pub fn place(app: &mut SoundApp, p: &Value) -> Result<Value, String> {
    let given: Vec<&str> = PLACE_FORMATS.into_iter().filter(|k| p.get(*k).is_some_and(|v| !v.is_null())).collect();
    let [format] = given.as_slice() else {
        return Err(if given.is_empty() {
            "nothing to place: give the audio as base64 under `wav`, `mp3`, `flac` or `ogg`".into()
        } else {
            "place one file at a time: give only one of `wav`, `mp3`, `flac` or `ogg`".into()
        });
    };
    let text = p[*format].as_str().ok_or_else(|| format!("`{format}` must be the audio as base64 text"))?;
    if text.len() / 4 * 3 > MAX_PLACE_BYTES + 3 {
        return Err(format!("the audio is larger than {} MB", MAX_PLACE_BYTES >> 20));
    }
    let bytes = b64::decode(text).ok_or_else(|| format!("`{format}` is not valid base64"))?;
    if bytes.is_empty() {
        return Err("the audio is empty".into());
    }
    if bytes.len() > MAX_PLACE_BYTES {
        return Err(format!("the audio is larger than {} MB", MAX_PLACE_BYTES >> 20));
    }
    let target = match p.get("track") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s == "new" => None,
        Some(v) => {
            let id = TrackId(v.as_u64().ok_or("`track` must be a track id or \"new\"")?);
            match app.engine.session().track(id) {
                Some(t) if t.kind == TrackKind::Audio => Some(id),
                Some(_) => return Err(format!("track {} is not an audio track", id.0)),
                None => return Err(format!("there is no track {}", id.0)),
            }
        }
    };
    let at = match p.get("at") {
        None | Some(Value::Null) => app.position(),
        Some(v) => v.as_i64().filter(|a| *a >= 0).ok_or("`at` must be a position in samples, 0 or more")?,
    };
    let stem = p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).unwrap_or("AI audio");
    // The format key says what the bytes are; a name with another ending must not decide it.
    let stem = stem.rsplit_once('.').filter(|(_, ext)| PLACE_FORMATS.contains(&ext.to_ascii_lowercase().as_str())).map_or(stem, |(stem, _)| stem);
    let file = format!("{stem}.{format}");
    let e = &mut app.engine;
    let before = e.session_arc();
    let placed = soundcraft_engine::io::import_audio_bytes(e, &file, &bytes, None, target, at).map_err(|err| format!("the audio could not be read: {err}"))?;
    e.push_undo("Place Audio", before);
    let clip = placed.get("clip").and_then(Value::as_u64).ok_or("the importer returned no clip")?;
    let track = placed.get("track").and_then(Value::as_u64).ok_or("the importer returned no track")?;
    let end = e.session().find_clip(soundcraft_model::ClipId(clip)).map_or(at, |(_, c)| c.end());
    app.ui.status = format!("Placed {stem}");
    Ok(json!({"placed": "audio", "track": track, "clip": clip, "source": placed["source"], "start": at, "end": end}))
}
